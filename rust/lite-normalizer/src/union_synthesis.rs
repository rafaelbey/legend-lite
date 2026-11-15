// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! `UnionSynthesis` (UnionSynthesis.java at ad86fa895): the operation sets -- union and inheritance stacks (member
//! order is the SQL UNION order), their key threads (`ID_0`, `id__pk_*` reach SQL), the routed class-typed
//! navigations' classification, and the pins' member-function names. EF_CENSUS §1.3's families: US-1 pins and
//! member-function names (:62-136), US-2 route classification (:159-402), US-3 operation synthesis (:143-155,
//! :412-697), US-4 key threads (:560-577, :765-787, :903-924, :951-1018), US-5 per-arm chain helpers (:798-891),
//! US-6 `prePassedClosure` (:929-937, ported now: `MappingLedger::mapped_in_closure` reads it).
//!
//! RE-EXPRESSED (census §4): the route ordinal is read only for its sign (US:397, :826), so [`UnionRoute`] carries
//! a three-variant [`RouteTarget`] instead of the number; US:311's `getOrDefault` is a `get` (`recordOwner` keys every
//! property). The function-naming rule is spelled twice (here, :62-83, and MN:794-796): both spellings are ported and
//! must agree. Not ported (census §3, US-7): `MEMBER_WITNESS`, `FILTER_FORM`, `record Thread`, `record ScanSource`
//! (:701-753). Write-then-throw (`mixedUnions.put` before a throw, :440-441) must stay: the fact survives the error.

use std::collections::{HashSet, VecDeque};
use std::sync::Arc;

use lite_builtin::pure::catalog;
use lite_error::Phase;
use lite_front::codec::{Codec, Writer};
use lite_front::records as p;
use lite_model::linked::{LinkedMap, list};
use lite_model::{
    ClassMapping, ClassMappingInheritance, ClassMappingRelational, ClassMappingUnion, JStr, KeyThread,
    LegacyMappingDefinition, PoisonKey, PoisonKeyForClass, PropertyMapping, PropertyMappingJoin, RelationalOperation,
    cat,
};
use lite_names::model_index::java_list;
use lite_names::{ModelIndex, calls, synth_fqn};

use crate::error::{Thrown, require};
use crate::jdk::LinkedSet;
use crate::mapping_ledger::MappingLedger;
use crate::mapping_normalizer::{canonical_table, determine_target_table, infer_main_table_quiet, synth_relational};
use crate::mapping_validation;
use crate::pipeline::Pipeline;
use crate::rel_op_translator::{self, TableScope};
use crate::resolved_mapping::ResolvedMapping;

/// Where a route lands, by the sign of Java's `targetOrdinal` (the only thing any reader asks of it).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum RouteTarget {
    /// `targetOrdinal >= 0`: a member of the target's union (or inheritance) operation.
    Member,
    /// `-1`: the root (or sole) set -- the un-routed navigation.
    Root,
    /// `PINNED_SINGLE` (-2): a pinned set that is neither a member nor the root.
    PinnedSingle,
}

/// `UnionSynthesis.UnionRoute` (:48): one PM's route entry -- where it lands and its join.
#[derive(Clone, Debug, PartialEq)]
pub struct UnionRoute {
    pub target: RouteTarget,
    pub join: Arc<PropertyMappingJoin>,
}

/// `UnionSynthesis.PinTarget` (:87): a pinned set and the mapping defining it.
#[derive(Clone, Debug, PartialEq)]
pub struct PinTarget {
    pub defining: Arc<LegacyMappingDefinition>,
    pub set: ClassMapping,
}

/// `UnionSynthesis.LiftMidStep` (:889): one inbound arm step -- its alias, database, table and join condition.
#[derive(Clone, Debug, PartialEq)]
pub struct LiftMidStep {
    pub alias: JStr,
    pub db: JStr,
    pub table: JStr,
    pub cond: Arc<p::LambdaFunction>,
}

/// `UnionSynthesis.TABLE_KEY_SUFFIX` (:765): the shared table key's suffix, `<col>__pk`.
pub const TABLE_KEY_SUFFIX: &str = "__pk";

/// The JDK exceptions of `recordKeyThreads`' cast (:977), unreachable (the note there).
const CLASS_CAST: &str = "java.lang.ClassCastException";
const INDEX_OUT_OF_BOUNDS: &str = "java.lang.IndexOutOfBoundsException";

/// `UnionSynthesis.memberFunction(md, member)` (:62): the FQN the arm calls for `member` (its defining mapping
/// searched in the closure, own record first; `closure().get(0)` otherwise).
pub fn member_function(md: &ResolvedMapping<'_>, member: &ClassMapping) -> Result<JStr, Thrown> {
    let id = ResolvedMapping::id_of(member);
    let closure = md.closure();
    for m in &closure {
        for cm in m.class_mappings.iter() {
            if cm.class_name() == member.class_name() && ResolvedMapping::id_of(cm) == id {
                return member_function_of(&PinTarget {
                    defining: m.clone(),
                    set: member.clone(),
                });
            }
        }
    }
    // `closure().get(0)`: the closure is this record first, never empty
    member_function_of(&PinTarget {
        defining: closure[0].clone(),
        set: member.clone(),
    })
}

/// `UnionSynthesis.memberFunction(pt)` (:75): a pin's member-function FQN (the second spelling of the naming rule).
pub fn member_function_of(pt: &PinTarget) -> Result<JStr, Thrown> {
    let member = &pt.set;
    let sets_of_class = pt
        .defining
        .class_mappings
        .iter()
        .filter(|cm| cm.class_name() == member.class_name())
        .count();
    Ok(if member.root() || sets_of_class <= 1 {
        synth_fqn::mapping_class(&pt.defining.qualified_name, member.class_name())?
    } else {
        synth_fqn::mapping_class_set(
            &pt.defining.qualified_name,
            member.class_name(),
            &ResolvedMapping::id_of(member),
        )?
    })
}

/// `UnionSynthesis.resolvePin` (:103): the set `set_id` pins, from the queried mapping's closure; none when not
/// visible (the "Declared divergence" of :101-102 is ASK 12's).
pub fn resolve_pin(
    md: &ResolvedMapping<'_>,
    set_id: Option<&JStr>,
    model: &ModelIndex,
) -> Result<Option<PinTarget>, Thrown> {
    let Some(set_id) = set_id else {
        return Ok(None);
    };
    if let Some(own) = md.set(Some(set_id))? {
        // the defining mapping: matched by class + effective id (the closure hands out SUBSTITUTED copies, never the
        // same object)
        let closure = md.closure();
        for m in &closure {
            for cm in m.class_mappings.iter() {
                if cm.class_name() == own.class_name() && *set_id == ResolvedMapping::id_of(cm) {
                    return Ok(Some(PinTarget {
                        defining: m.clone(),
                        set: own,
                    }));
                }
            }
        }
        return Ok(Some(PinTarget {
            defining: closure[0].clone(),
            set: own,
        }));
    }
    let mut hits = pin_owners(set_id, model);
    Ok(if hits.len() == 1 { hits.pop() } else { None })
}

/// `UnionSynthesis.pinOwners` (:130): every mapping of the model defining a set `set_id` (the index's
/// `legacy_sets_with_id`).
pub fn pin_owners(set_id: &JStr, model: &ModelIndex) -> Vec<PinTarget> {
    model
        .legacy_sets_with_id(set_id)
        .iter()
        .map(|s| PinTarget {
            defining: s.defining.clone(),
            set: s.set.clone(),
        })
        .collect()
}

/// `UnionSynthesis.stackBody` (:143): `m1() -> concatenate(m2()) -> ...` over the members, recorded as the
/// operation's members.
pub fn stack_body(
    md: &ResolvedMapping<'_>,
    operation: &ClassMapping,
    members: &[ClassMapping],
    ledger: &mut MappingLedger,
) -> Result<p::ValueSpecification, Thrown> {
    let mut out: Option<p::ValueSpecification> = None;
    let mut ids: Vec<JStr> = Vec::with_capacity(members.len());
    for m in members {
        let call = p::ValueSpecification::AppliedFunction(Arc::new(calls::exact(&member_function(md, m)?, Vec::new())));
        out = Some(match out {
            None => call,
            Some(prev) => p::ValueSpecification::AppliedFunction(Arc::new(calls::platform(
                &JStr::from("concatenate"),
                vec![prev, call],
            ))),
        });
        ids.push(ResolvedMapping::id_of(m));
    }
    // the arms as a FACT beside the body (the binding's memberSetIds), recorded before the no-member refusal
    ledger.operation_members.put(ResolvedMapping::id_of(operation), ids);
    require(out, Some("an operation with no members"))
}

/// `UnionSynthesis.pmIdentity` (:159): a PM's identity for the extends merge, `(name, targetSetId)`.
pub fn pm_identity(pm: &PropertyMapping) -> JStr {
    match pm {
        PropertyMapping::PropertyMappingJoin(j) if let Some(target) = &j.target_set_id => {
            cat!(pm.property_name(), " ", target)
        }
        _ => cat!(pm.property_name(), " "),
    }
}

/// `UnionSynthesis.collectRoutedJoins` (:174, the six-argument form): [`collect_routed_joins_splicing`] with a fresh
/// splice stack.
pub fn collect_routed_joins(
    pms: &[PropertyMapping],
    owner_cls: &JStr,
    md: &ResolvedMapping<'_>,
    model: &ModelIndex,
    routed_by_prop: &mut LinkedMap<JStr, Vec<Arc<PropertyMappingJoin>>>,
    owner_by_prop: &mut LinkedMap<JStr, JStr>,
) -> Result<(), Thrown> {
    collect_routed_joins_splicing(
        pms,
        owner_cls,
        md,
        model,
        routed_by_prop,
        owner_by_prop,
        &mut LinkedSet::new(),
    )
}

/// `UnionSynthesis.collectRoutedJoins` (:186, the seven-argument form): the routed Join PMs per property (through
/// embedded blocks and Inline splices, `ResolvedMapping::first_relational`), each property's owner recorded; loud for
/// a splice already on `splicing` (whose order the text prints).
#[allow(clippy::too_many_arguments)]
pub fn collect_routed_joins_splicing(
    pms: &[PropertyMapping],
    owner_cls: &JStr,
    md: &ResolvedMapping<'_>,
    model: &ModelIndex,
    routed_by_prop: &mut LinkedMap<JStr, Vec<Arc<PropertyMappingJoin>>>,
    owner_by_prop: &mut LinkedMap<JStr, JStr>,
    splicing: &mut LinkedSet<JStr>,
) -> Result<(), Thrown> {
    for pm in pms {
        match pm {
            PropertyMapping::PropertyMappingJoin(j) if j.target_set_id.is_some() => {
                routed_by_prop
                    .get_or_insert_with(j.property_name.clone(), Vec::new)
                    .push(j.clone());
                record_owner(owner_by_prop, &j.property_name, owner_cls, md)?;
            }
            PropertyMapping::PropertyMappingEmbedded(emb) => {
                if let Some(inner) = embedded_owner(owner_cls, &emb.property_name, model)? {
                    collect_routed_joins_splicing(
                        &emb.property_mappings,
                        &inner,
                        md,
                        model,
                        routed_by_prop,
                        owner_by_prop,
                        splicing,
                    )?;
                }
            }
            PropertyMapping::PropertyMappingOtherwiseEmbedded(oe) => {
                // the fallback join is a pinned route of the property
                if let PropertyMapping::PropertyMappingJoin(fj) = &oe.fallback {
                    let pin = fj.target_set_id.clone().unwrap_or_else(|| oe.fallback_set_id.clone());
                    // Java's four-argument `Join` (its at-least-one-hop check passed when `fj` was built)
                    routed_by_prop
                        .get_or_insert_with(oe.property_name.clone(), Vec::new)
                        .push(Arc::new(PropertyMappingJoin {
                            property_name: oe.property_name.clone(),
                            database: fj.database.clone(),
                            joins: fj.joins.clone(),
                            target_set_id: Some(pin),
                        }));
                    record_owner(owner_by_prop, &oe.property_name, owner_cls, md)?;
                }
                if let Some(inner) = embedded_owner(owner_cls, &oe.property_name, model)? {
                    collect_routed_joins_splicing(
                        &oe.embedded,
                        &inner,
                        md,
                        model,
                        routed_by_prop,
                        owner_by_prop,
                        splicing,
                    )?;
                }
            }
            PropertyMapping::PropertyMappingInlineEmbedded(ie) => {
                if !splicing.add(ie.set_id.clone()) {
                    return Err(Thrown::model(
                        Phase::Normalize,
                        cat!(
                            "Cycle materializing Inline embedded set '",
                            &ie.set_id,
                            "' via '",
                            &ie.property_name,
                            "' on '",
                            owner_cls,
                            "': the splice chain ",
                            java_list(&splicing.to_vec()),
                            " returns to it; mapping=",
                            md.qualified_name()
                        ),
                        None,
                    ));
                }
                // Java's try/finally: the splice leaves the chain whether the descent answers or throws
                let descended = match md.first_relational(&ie.set_id) {
                    Some(r2) => collect_routed_joins_splicing(
                        &r2.property_mappings,
                        &r2.class_name,
                        md,
                        model,
                        routed_by_prop,
                        owner_by_prop,
                        splicing,
                    ),
                    None => Ok(()),
                };
                splicing.remove(&ie.set_id);
                descended?;
            }
            PropertyMapping::PropertyMappingLocalProperty(lp) => collect_routed_joins_splicing(
                std::slice::from_ref(&lp.body),
                owner_cls,
                md,
                model,
                routed_by_prop,
                owner_by_prop,
                splicing,
            )?,
            _ => {}
        }
    }
    Ok(())
}

/// `UnionSynthesis.recordOwner` (:261): the owner class a routed property resolves against; loud for two owners.
pub fn record_owner(
    owner_by_prop: &mut LinkedMap<JStr, JStr>,
    prop: &JStr,
    owner_cls: &JStr,
    md: &ResolvedMapping<'_>,
) -> Result<(), Thrown> {
    match owner_by_prop.get(prop) {
        None => {
            owner_by_prop.put(prop.clone(), owner_cls.clone());
        }
        Some(prior) if prior != owner_cls => {
            return Err(Thrown::model(
                Phase::Normalize,
                cat!(
                    "property '",
                    prop,
                    "' is routed under two owners, '",
                    prior,
                    "' and '",
                    owner_cls,
                    "', in one class mapping; routes are keyed by property name, so the mapping cannot be \
                     normalized; mapping=",
                    md.qualified_name()
                ),
                None,
            ));
        }
        Some(_) => {}
    }
    Ok(())
}

/// `UnionSynthesis.embeddedOwner` (:275): the class an embedded property's sub-PMs belong to.
pub fn embedded_owner(owner_cls: &JStr, prop: &JStr, model: &ModelIndex) -> Result<Option<JStr>, Thrown> {
    let k = model.knowledge();
    // MissProbe.knownMiss: an owner the knowledge cannot answer descends nowhere
    let pt = match k.hierarchy_class(Some(owner_cls)) {
        None => None,
        Some(oc) => k.property_type(Some(oc), prop)?,
    };
    Ok(match pt {
        Some(p::TypeExpression::TypeExpressionNameRef(nr)) if k.hierarchy_class(Some(&nr.name)).is_some() => {
            Some(nr.name.clone())
        }
        _ => None,
    })
}

/// `UnionSynthesis.classifyUnionRoutes` (:302): each routed property's routes into `p.union_routes` (member, root or
/// pinned single); an unresolvable pin drops the property (`p.dropped_routed_props`) and poisons the class through
/// `p.ledger_mut()`.
pub fn classify_union_routes(
    md: &ResolvedMapping<'_>,
    rcm: &ClassMappingRelational,
    model: &ModelIndex,
    p: &mut Pipeline<'_>,
) -> Result<(), Thrown> {
    let mut routed_by_prop: LinkedMap<JStr, Vec<Arc<PropertyMappingJoin>>> = LinkedMap::new();
    let mut owner_by_prop: LinkedMap<JStr, JStr> = LinkedMap::new();
    collect_routed_joins(
        &rcm.property_mappings,
        &rcm.class_name,
        md,
        model,
        &mut routed_by_prop,
        &mut owner_by_prop,
    )?;
    let k = model.knowledge();
    for (prop, joins) in routed_by_prop.iter() {
        // RE-EXPRESSED (census §4): Java's `getOrDefault(prop, rcm.className())` is a `get` (`record_owner` keyed every
        // property); an owner the knowledge cannot answer is MissProbe.miss (no target: every route root or pinned)
        let owner = k.hierarchy_class(owner_by_prop.get(prop));
        let pt = match owner {
            None => None,
            Some(o) => k.property_type(Some(o), prop)?,
        };
        let target_class = match &pt {
            Some(p::TypeExpression::TypeExpressionNameRef(nr)) if k.hierarchy_class(Some(&nr.name)).is_some() => {
                Some(nr.name.clone())
            }
            _ => None,
        };
        let tu = match &target_class {
            None => None,
            Some(tc) => md.union_of(Some(tc)),
        };
        // FIX-A (audit-17 bucket analysis): an INHERITANCE op is a union at the routing level -- member set ids in the
        // shared enumeration's order
        let mut member_ids: Option<Vec<JStr>> = tu.map(|u| u.member_set_ids.to_vec());
        if member_ids.is_none()
            && let Some(tc) = &target_class
            && let Some(tih) = md.inheritance_of(tc)
        {
            member_ids = Some(
                inheritance_members(md, &tih, model)?
                    .iter()
                    .map(relational_id)
                    .collect(),
            );
        }
        let mut routes: Vec<UnionRoute> = Vec::new();
        let mut poison: Option<JStr> = None;
        // R-target: several distinct pins resolve to the target's root -- a pin outside is dead; ONE distinct pin is
        // that set, member of the root or not
        let distinct_pins = joins
            .iter()
            .map(|j| j.target_set_id.clone())
            .collect::<HashSet<Option<JStr>>>()
            .len();
        for j in joins {
            let Some(pin) = resolve_pin(md, j.target_set_id.as_ref(), model)? else {
                let owners = match &j.target_set_id {
                    None => Vec::new(),
                    Some(set_id) => pin_owners(set_id, model),
                };
                poison = Some(if owners.len() > 1 {
                    let defining: Vec<JStr> = owners.iter().map(|o| o.defining.qualified_name.clone()).collect();
                    cat!(
                        "an AMBIGUOUS mapping set '",
                        null_or(j.target_set_id.as_ref()),
                        "' (not visible through the includes; defined by ",
                        java_list(&defining),
                        ")"
                    )
                } else {
                    cat!("unknown mapping set '", null_or(j.target_set_id.as_ref()), "'")
                });
                break;
            };
            let set = pin.set;
            let ord = match &member_ids {
                None => -1,
                Some(ids) => md.member_ordinal(ids, j.target_set_id.as_ref())?,
            };
            // engine rootClassMappingByClass -- ONE owner, the resolved mapping; a set OUTSIDE this closure is never
            // this mapping's root: its route names the set's function (pinned-single)
            let root_or_sole = matches!(set, ClassMapping::ClassMappingRelational(_))
                && md.set(j.target_set_id.as_ref())?.is_some()
                && md.is_root_or_sole(&set);
            if ord >= 0 {
                routes.push(UnionRoute {
                    target: RouteTarget::Member,
                    join: j.clone(),
                });
            } else if member_ids.is_some() && distinct_pins > 1 {
                // the TARGET class is union-mapped, this route's set is not among the members and the property pins
                // several sets: the router resolves to the root, so the entry is DEAD -- never a root route and never
                // a poison
                continue;
            } else if root_or_sole {
                routes.push(UnionRoute {
                    target: RouteTarget::Root,
                    join: j.clone(),
                });
            } else {
                // a set-pinned route to a NON-root set: a route naming that set's function
                routes.push(UnionRoute {
                    target: RouteTarget::PinnedSingle,
                    join: j.clone(),
                });
            }
        }
        if let Some(poison) = poison {
            p.dropped_routed_props.insert(prop.clone());
            p.ledger_mut()?.poison(
                PoisonKey::PoisonKeyForClass(Arc::new(PoisonKeyForClass {
                    class_fqn: rcm.class_name.clone(),
                })),
                cat!(
                    "property '",
                    prop,
                    "' routes to ",
                    &poison,
                    "; the property is dropped from this synthesis"
                ),
            );
            continue;
        }
        if routes.iter().all(|r| r.target == RouteTarget::Root) {
            continue; // root routes = the un-routed navigation
        }
        p.union_routes.put(prop.clone(), routes);
    }
    Ok(())
}

/// `UnionSynthesis.synthUnion` (:412): an Operation union's stack (a mixed-kind union recorded in
/// `ledger.mixed_unions` BEFORE its throw); its key threads recorded.
pub fn synth_union(
    md: &ResolvedMapping<'_>,
    u: &Arc<ClassMappingUnion>,
    model: &ModelIndex,
    ledger: &mut MappingLedger,
) -> Result<p::ValueSpecification, Thrown> {
    if u.member_set_ids.is_empty() {
        return Err(Thrown::not_implemented(cat!(
            "Operation union with no member sets; class=",
            &u.class_name,
            ", mapping=",
            md.qualified_name()
        )));
    }
    // member sets resolve by setId ACROSS INCLUDES (the includes' sets, this mapping's own on top: Java's `bySetId`
    // is `visibleSets()`'s body); a member may map a SUBCLASS of the operation class
    let by_set_id = md.visible_sets()?;
    let mut member_sets: Vec<ClassMapping> = Vec::new();
    for set_id in u.member_set_ids.iter() {
        let member = by_set_id.get(set_id);
        if let Some(ClassMapping::ClassMappingPure(_)) = member {
            // MIXED-KIND union: the member list recorded for the resolver-side arm synthesis BEFORE the throw (the
            // fact survives the error)
            ledger.mixed_unions.put(u.class_name.clone(), u.member_set_ids.to_vec());
            return Err(Thrown::not_implemented(cat!(
                "Operation union member set '",
                set_id,
                "' of class '",
                &u.class_name,
                "' is a Pure (M2M) set \u{2014} the mixed-kind union extent synthesizes at the resolver; mapping=",
                md.qualified_name()
            )));
        }
        let member = match member {
            Some(m @ (ClassMapping::ClassMappingRelational(_) | ClassMapping::ClassMappingRelationFunction(_))) => m,
            _ => {
                return Err(Thrown::not_implemented(cat!(
                    "Operation union member set '",
                    set_id,
                    "' of class '",
                    &u.class_name,
                    "' is ",
                    if member.is_none() {
                        "missing"
                    } else {
                        "not a Relational or Relation(~func) set"
                    },
                    "; mapping=",
                    md.qualified_name()
                )));
            }
        };
        // a member must map the operation class or a SUBCLASS (audit 8 S8)
        if !model.knowledge().is_subtype(member.class_name(), &u.class_name) {
            return Err(Thrown::model(
                Phase::Normalize,
                cat!(
                    "Operation union member set '",
                    set_id,
                    "' maps '",
                    member.class_name(),
                    "', which is not '",
                    &u.class_name,
                    "' or a subclass; mapping=",
                    md.qualified_name()
                ),
                None,
            ));
        }
        member_sets.push(member.clone());
    }
    // per-pair AssociationMapping entries ([sourceSet, targetSet]) land on their owning MEMBER set as routed
    // class-typed Join PMs
    let pair_entries = md.pair_entries(&u.class_name)?;
    if !pair_entries.is_empty() {
        for slot in member_sets.iter_mut() {
            let ClassMapping::ClassMappingRelational(mr) = &*slot else {
                continue;
            };
            // OWN entries + EXTENDS-routed entries merged, own entries first; the pmIdentity dedup below keeps them
            // authoritative
            let mut add: Vec<Arc<PropertyMappingJoin>> = Vec::new();
            let mut cur = Some(relational_id(mr));
            let mut seen_sets: HashSet<JStr> = HashSet::new();
            while let Some(c) = cur
                && seen_sets.insert(c.clone())
            {
                if let Some(lvl) = pair_entries.get(&c) {
                    add.extend(lvl.iter().cloned());
                }
                cur = match md.set(Some(&c))? {
                    Some(ClassMapping::ClassMappingRelational(ur)) => ur.extends_set_id.clone(),
                    _ => None,
                };
            }
            if add.is_empty() {
                continue;
            }
            // deduped against the GROWING list (two equal entries are added once)
            let mut pms: Vec<PropertyMapping> = mr.property_mappings.to_vec();
            for j in add {
                let j = PropertyMapping::PropertyMappingJoin(j);
                if !pms.iter().any(|q| pm_identity(q) == pm_identity(&j)) {
                    pms.push(j);
                }
            }
            let rebuilt = ClassMappingRelational {
                property_mappings: list(pms),
                ..(**mr).clone()
            };
            *slot = ClassMapping::ClassMappingRelational(Arc::new(rebuilt));
        }
    }
    record_key_threads_of(md, &u.class_name, &member_sets, model, ledger)?;
    stack_body(md, &ClassMapping::ClassMappingUnion(u.clone()), &member_sets, ledger)
}

/// `UnionSynthesis.synthInheritance` (:525): an Inheritance operation's stack over its members.
pub fn synth_inheritance(
    md: &ResolvedMapping<'_>,
    ih: &Arc<ClassMappingInheritance>,
    model: &ModelIndex,
    ledger: &mut MappingLedger,
) -> Result<p::ValueSpecification, Thrown> {
    // ENGINE ALGORITHM (router_operations.pure getMappedLeafTypes): the ordered member enumeration is SHARED with
    // route classification (`inheritance_members`): ordinal alignment by construction
    let members = inheritance_members(md, ih, model)?;
    if members.is_empty() {
        return Err(Thrown::not_implemented(cat!(
            "inheritance Operation for '",
            &ih.class_name,
            "' finds no mapped subclass sets; mapping=",
            md.qualified_name()
        )));
    }
    let operation = ClassMapping::ClassMappingInheritance(ih.clone());
    if members.len() == 1 {
        // the sole member's own synthesis is the class's function (no stack); the fact names the one arm
        ledger
            .operation_members
            .put(ResolvedMapping::id_of(&operation), vec![relational_id(&members[0])]);
        return synth_relational(md, &members[0], model, ledger);
    }
    let members: Vec<ClassMapping> = members.into_iter().map(ClassMapping::ClassMappingRelational).collect();
    record_key_threads_of(md, &ih.class_name, &members, model, ledger)?;
    stack_body(md, &operation, &members, ledger)
}

/// `UnionSynthesis.recordKeyThreadsOf` (:560): the members' key threads into `ledger.union_key_threads`.
pub fn record_key_threads_of(
    md: &ResolvedMapping<'_>,
    class_name: &JStr,
    members: &[ClassMapping],
    model: &ModelIndex,
    ledger: &mut MappingLedger,
) -> Result<(), Thrown> {
    // a member without a declared ~mainTable keys on its INFERRED one (Java asks `inferMainTableQuiet` twice, a pure
    // function of the set: once here)
    let mut ms: Vec<ClassMapping> = Vec::with_capacity(members.len());
    for m in members {
        if let ClassMapping::ClassMappingRelational(mr) = m
            && mr.main_table.is_none()
            && let Some(inferred) = infer_main_table_quiet(mr)?
        {
            ms.push(ClassMapping::ClassMappingRelational(Arc::new(ClassMappingRelational {
                main_table: Some(inferred),
                ..(**mr).clone()
            })));
        } else {
            ms.push(m.clone());
        }
    }
    let shared_keys = own_shared_keys(&ms, model)?;
    record_key_threads(md, class_name, &ms, &shared_keys, model, ledger)
}

/// `UnionSynthesis.inheritanceMembers` (:585): an Inheritance operation's member sets, in the walk's order.
pub fn inheritance_members(
    md: &ResolvedMapping<'_>,
    ih: &ClassMappingInheritance,
    model: &ModelIndex,
) -> Result<Vec<Arc<ClassMappingRelational>>, Thrown> {
    let mut chosen: Vec<ClassMapping> = Vec::new();
    collect_inheritance_members(md, &ih.class_name, model, &mut chosen)?;
    let mut members: Vec<Arc<ClassMappingRelational>> = Vec::new();
    for cm in &chosen {
        match cm {
            ClassMapping::ClassMappingRelational(mr) => members.push(mr.clone()),
            ClassMapping::ClassMappingUnion(u2) => {
                // Java's `bySetId` again, built per union member (lazily: a closure walk is loud here only)
                let by_set_id = md.visible_sets()?;
                for set_id in u2.member_set_ids.iter() {
                    if let Some(ClassMapping::ClassMappingRelational(mr2)) = by_set_id.get(set_id) {
                        members.push(mr2.clone());
                    } else {
                        return Err(Thrown::not_implemented(cat!(
                            "inheritance member union set '",
                            set_id,
                            "' is not a Relational set; mapping=",
                            md.qualified_name()
                        )));
                    }
                }
            }
            ClassMapping::ClassMappingPure(_)
            | ClassMapping::ClassMappingRelationFunction(_)
            | ClassMapping::ClassMappingInheritance(_) => {
                return Err(Thrown::not_implemented(cat!(
                    "inheritance Operation member for '",
                    cm.class_name(),
                    "' is a ",
                    java_simple_name(cm),
                    " mapping \u{2014} not supported yet; mapping=",
                    md.qualified_name()
                )));
            }
        }
    }
    Ok(members)
}

/// Java's record equality of two class mappings (`LinkedHashSet<ClassMapping>`, US:797; the normalizer batch, item 10):
/// their [`record_key`]s are equal.
fn java_record_equal(a: &ClassMapping, b: &ClassMapping) -> bool {
    record_key(a) == record_key(b)
}

/// The components a class mapping's `equals` reads, as one text: two class mappings have one key exactly when Java's
/// `equals` holds (the normalizer's second batch, item 4 (b), ruling 37 (b), 2026-11-15; it was the set's whole
/// lossless codec text, which also writes a protocol node's source position, which a protocol record's `equals`
/// leaves out). A record whose components all have Java's default equality is its codec text (one-to-one with
/// `equals`: a `Double` as `Double.toString`, so `0.0` and `-0.0` two and every `NaN` one, a `BigDecimal` as
/// `toString`, an open slot with its Java class); a record holding a `ValueSpecification` or a `TypeExpression` is a
/// JSON array of its tag and its components in order, each such one by lite-front's `equality_key` (the components
/// its `equals` reads, its position not among them); `propertyTargetSets` its entries in key order, as `Map.equals`
/// reads no order. Every arm spelled, no default.
fn record_key(c: &ClassMapping) -> String {
    let mut w = Writer::new();
    class_mapping_key(&mut w, c);
    w.finish()
}

fn class_mapping_key(w: &mut Writer, c: &ClassMapping) {
    match c {
        ClassMapping::ClassMappingRelational(r) => relational_key(w, r),
        ClassMapping::ClassMappingPure(p) => key_node(w, "Pure", |w| {
            key_str(w, &p.class_name);
            key_opt_str(w, p.set_id.as_ref());
            key_opt_str(w, p.extends_set_id.as_ref());
            key_bool(w, p.root);
            key_opt_str(w, p.source_class.as_ref());
            key_opt_vs(w, p.filter.as_ref());
            key_list(w, p.property_bindings.iter(), |w, b| {
                key_node(w, "PropertyBinding", |w| {
                    key_str(w, &b.property_name);
                    key_vs(w, &b.expression);
                    key_opt_str(w, b.source_set_id.as_ref());
                    key_opt_str(w, b.target_set_id.as_ref());
                    key_bool(w, b.explode);
                    key_bool(w, b.local);
                    key_opt_str(w, b.enum_mapping_id.as_ref());
                })
            });
        }),
        ClassMapping::ClassMappingUnion(u) => u.write(w, false),
        ClassMapping::ClassMappingRelationFunction(f) => key_node(w, "RelationFunction", |w| {
            key_str(w, &f.class_name);
            key_opt_str(w, f.set_id.as_ref());
            key_opt_str(w, f.extends_set_id.as_ref());
            key_bool(w, f.root);
            key_opt_str(w, f.func_ref.as_ref());
            key_opt_vs(w, f.inline_source.as_ref());
            key_list(w, f.columns.iter(), |w, c| col_key(w, c));
            key_list(w, f.primary_key.iter(), |w, s| w.string(s));
        }),
        ClassMapping::ClassMappingInheritance(i) => i.write(w, false),
    }
}

fn relational_key(w: &mut Writer, r: &ClassMappingRelational) {
    key_node(w, "Relational", |w| {
        key_str(w, &r.class_name);
        key_opt_str(w, r.set_id.as_ref());
        key_opt_str(w, r.extends_set_id.as_ref());
        key_bool(w, r.root);
        w.raw(",");
        match &r.main_table {
            None => w.raw("null"),
            Some(t) => t.write(w, false),
        }
        w.raw(",");
        match &r.filter {
            None => w.raw("null"),
            Some(f) => f.write(w, false),
        }
        key_bool(w, r.distinct);
        key_list(w, r.group_by.iter(), |w, o| o.write(w, false));
        key_list(w, r.primary_key.iter(), |w, o| o.write(w, false));
        key_list(w, r.property_mappings.iter(), property_mapping_key);
        key_opt_str(w, r.source_url.as_ref());
        // a map's keys are distinct: its entries in key order are one text exactly when `Map.equals` holds
        let mut targets: Vec<&(JStr, JStr)> = r.property_target_sets.iter().collect();
        targets.sort();
        key_list(w, targets, |w, (k, v)| {
            w.raw("[");
            w.string(k);
            w.raw(",");
            w.string(v);
            w.raw("]");
        });
        w.raw(",");
        match &r.aggregation {
            None => w.raw("null"),
            Some(a) => key_node(w, "AggregationAware", |w| {
                key_list(w, a.views.iter(), |w, v| {
                    key_node(w, "AggregateView", |w| {
                        w.raw(",");
                        w.int(i64::from(v.index));
                        key_bool(w, v.can_aggregate);
                        key_list(w, v.group_by_functions.iter(), |w, f| w.raw(&f.equality_key()));
                        key_list(w, v.aggregate_values.iter(), |w, a| {
                            key_node(w, "AggregateValue", |w| {
                                key_vs(w, &a.map_fn);
                                key_vs(w, &a.aggregate_fn);
                            })
                        });
                        w.raw(",");
                        relational_key(w, &v.set);
                    })
                });
            }),
        }
    });
}

fn property_mapping_key(w: &mut Writer, pm: &PropertyMapping) {
    match pm {
        PropertyMapping::PropertyMappingEnumeratedExpression(x) => x.write(w, false),
        PropertyMapping::PropertyMappingColumn(x) => x.write(w, false),
        PropertyMapping::PropertyMappingEnumeratedColumn(x) => x.write(w, false),
        PropertyMapping::PropertyMappingJoin(x) => x.write(w, false),
        PropertyMapping::PropertyMappingJoinTerminalColumn(x) => x.write(w, false),
        PropertyMapping::PropertyMappingExpression(x) => x.write(w, false),
        PropertyMapping::PropertyMappingEmbedded(e) => key_node(w, "Embedded", |w| {
            key_str(w, &e.property_name);
            key_list(w, e.property_mappings.iter(), property_mapping_key);
            key_list(w, e.primary_key.iter(), |w, o| o.write(w, false));
        }),
        PropertyMapping::PropertyMappingInlineEmbedded(x) => x.write(w, false),
        PropertyMapping::PropertyMappingOtherwiseEmbedded(o) => key_node(w, "OtherwiseEmbedded", |w| {
            key_str(w, &o.property_name);
            key_list(w, o.embedded.iter(), property_mapping_key);
            key_str(w, &o.fallback_set_id);
            w.raw(",");
            property_mapping_key(w, &o.fallback);
        }),
        PropertyMapping::PropertyMappingLocalProperty(l) => key_node(w, "LocalProperty", |w| {
            key_str(w, &l.property_name);
            w.raw(",");
            w.raw(&l.r#type.equality_key());
            // `Multiplicity` has Java's default equality (lite-front's module note)
            w.raw(",");
            l.multiplicity.write(w, false);
            w.raw(",");
            property_mapping_key(w, &l.body);
        }),
    }
}

fn col_key(w: &mut Writer, c: &lite_model::ClassMappingRelationFunctionCol) {
    key_node(w, "Col", |w| {
        key_str(w, &c.property);
        key_opt_str(w, c.column.as_ref());
        key_bool(w, c.local);
        key_opt_str(w, c.enum_mapping_id.as_ref());
        key_list(w, c.embedded.iter(), |w, e| col_key(w, e));
        key_opt_str(w, c.inline_set_id.as_ref());
        key_opt_vs(w, c.expr.as_ref());
    });
}

/// A keyed record: `["Tag"` then each component after a comma, then `]`.
fn key_node(w: &mut Writer, tag: &str, components: impl FnOnce(&mut Writer)) {
    w.raw("[\"");
    w.raw(tag);
    w.raw("\"");
    components(w);
    w.raw("]");
}

fn key_str(w: &mut Writer, s: &JStr) {
    w.raw(",");
    w.string(s);
}

fn key_opt_str(w: &mut Writer, s: Option<&JStr>) {
    w.raw(",");
    match s {
        None => w.raw("null"),
        Some(s) => w.string(s),
    }
}

fn key_bool(w: &mut Writer, b: bool) {
    w.raw(",");
    w.boolean(b);
}

fn key_vs(w: &mut Writer, v: &p::ValueSpecification) {
    w.raw(",");
    w.raw(&v.equality_key());
}

fn key_opt_vs(w: &mut Writer, v: Option<&p::ValueSpecification>) {
    match v {
        None => w.raw(",null"),
        Some(v) => key_vs(w, v),
    }
}

fn key_list<T>(w: &mut Writer, xs: impl IntoIterator<Item = T>, mut item: impl FnMut(&mut Writer, T)) {
    w.raw(",[");
    for (i, x) in xs.into_iter().enumerate() {
        if i > 0 {
            w.raw(",");
        }
        item(w, x);
    }
    w.raw("]");
}

/// `UnionSynthesis.collectInheritanceMembers` (:623): `base`'s mapped subclasses' sets into `chosen` -- Java's
/// `LinkedHashSet<ClassMapping>` (record equality, insertion order): add a set only when no equal one is there. A
/// shadow walker `ShadowWalkerCensusTest` pinned (the census retires with the Java).
pub fn collect_inheritance_members(
    md: &ResolvedMapping<'_>,
    base: &JStr,
    model: &ModelIndex,
    chosen: &mut Vec<ClassMapping>,
) -> Result<(), Thrown> {
    // ROOT class mapping per class, includes first (own definitions win): Java copies `md.roots()`, read by lookup
    let root_by_class = md.roots();
    let k = model.knowledge();
    // the strict subtree of base (a membership set); then the universe order (model classes, then natives -- the
    // member order the rosters pin) restricted to it
    let subtree: HashSet<JStr> = k.subtree(base).into_iter().collect();
    let mut subs: Vec<JStr> = Vec::new();
    for cd in model.classes() {
        if subtree.contains(&cd.qualified_name) {
            subs.push(cd.qualified_name.clone());
        }
    }
    for nc in catalog().native_classes() {
        if subtree.contains(&nc.qualified_name) && !subs.contains(&nc.qualified_name) {
            subs.push(nc.qualified_name.clone());
        }
    }
    // a leaf has no subclass at all
    let leaves: Vec<JStr> = subs.into_iter().filter(|c| k.direct_subtypes(c).is_empty()).collect();
    for leaf in leaves {
        // nearest mapped ancestor at or above the leaf, STRICTLY below base
        let mut level: VecDeque<JStr> = VecDeque::new();
        let mut seen: HashSet<JStr> = HashSet::new();
        level.push_back(leaf);
        'outer: while !level.is_empty() {
            let n = level.len();
            for _ in 0..n {
                let Some(c) = level.pop_front() else {
                    break;
                };
                if !seen.insert(c.clone()) || c == *base {
                    continue;
                }
                // STRICTLY BELOW base means inside base's subtree (a multiple-inheritance leaf's other parent chain
                // leaves it)
                if !subtree.contains(&c) {
                    continue;
                }
                if let Some(cm) = root_by_class.get(&c) {
                    if let ClassMapping::ClassMappingInheritance(_) = cm {
                        collect_inheritance_members(md, &c, model, chosen)?;
                    } else if !chosen.iter().any(|c| java_record_equal(c, cm)) {
                        // Java's LinkedHashSet<ClassMapping>: by record equality, first insertion kept
                        chosen.push(cm.clone());
                    }
                    break 'outer;
                }
                let cd = k.hierarchy_class(Some(&c)).ok_or_else(|| {
                    Thrown::illegal_state(cat!(
                        "F7.8: class unresolved at UnionSynthesis#3 (this default NEVER fired on the corpus census; \
                         a miss here is a real model gap): ",
                        &c
                    ))
                })?;
                // the declared heads that are plain names (a generic head is climbed by no one here)
                for sup in cd.super_classes.iter() {
                    if let p::TypeExpression::TypeExpressionNameRef(nr) = sup {
                        level.push_back(nr.name.clone());
                    }
                }
            }
        }
    }
    Ok(())
}

/// `UnionSynthesis.tableKey` (:768): the sole PRIMARY KEY column of a member set's main table, else none.
pub fn table_key(cm: Option<&ClassMapping>, model: &ModelIndex) -> Result<Option<JStr>, Thrown> {
    let Some(ClassMapping::ClassMappingRelational(r)) = cm else {
        return Ok(None);
    };
    let Some(main) = &r.main_table else {
        return Ok(None);
    };
    // MissProbe.miss: no physical table (a view) has no shared key
    let Some(td) = model.knowledge().table(Some(&main.database), Some(&main.table)) else {
        return Ok(None);
    };
    let mut key: Option<JStr> = None;
    for c in td.columns.iter() {
        if c.primary_key {
            if key.is_some() {
                return Ok(None); // composite key: member-suffixed as before
            }
            key = Some(c.name.clone());
        }
    }
    Ok(key)
}

/// `UnionSynthesis.uniformChainedRoutes` (:798): whether the member routes' chains are one shape.
pub fn uniform_chained_routes(member_js: &[Arc<PropertyMappingJoin>]) -> bool {
    if member_js.is_empty() || !member_js.iter().any(|j| j.joins.len() > 1) {
        return true;
    }
    let first = &member_js[0];
    for j in member_js {
        if j.joins.len() != first.joins.len() {
            return false;
        }
        // every hop but the last: the same join name in the same database (a hop's own database, else its PM's)
        for (a, b) in first
            .joins
            .iter()
            .zip(j.joins.iter())
            .take(j.joins.len().saturating_sub(1))
        {
            let db_a = a.database_name.as_ref().unwrap_or(&first.database);
            let db_b = b.database_name.as_ref().unwrap_or(&j.database);
            if a.join_name != b.join_name || db_a != db_b {
                return false;
            }
        }
    }
    true
}

/// `UnionSynthesis.memberJoins` (:825): the joins of the MEMBER routes, in route order.
pub fn member_joins(routes: &[UnionRoute]) -> Vec<Arc<PropertyMappingJoin>> {
    // Java's `targetOrdinal() >= 0` (census §4: the tag's Member)
    routes
        .iter()
        .filter(|r| r.target == RouteTarget::Member)
        .map(|r| r.join.clone())
        .collect()
}

/// `UnionSynthesis.inboundArmSteps` (:842): a chained route's mid steps pushed into the member's arm (the
/// `nl__<prop>__inb__<join>` aliases), from the member's table back.
pub fn inbound_arm_steps(
    j: &PropertyMappingJoin,
    prop: &JStr,
    member_table: &JStr,
    md: &ResolvedMapping<'_>,
    model: &ModelIndex,
) -> Result<Vec<LiftMidStep>, Thrown> {
    let mut steps: Vec<LiftMidStep> = Vec::new();
    let mut prev_table = member_table.clone();
    let mut prev_alias: Option<JStr> = None;
    // all hops but the FIRST, reversed: from the target member's table back through the mids
    for h in (1..j.joins.len()).rev() {
        let mid_hop = &j.joins[h];
        let mid_db = mid_hop.database_name.clone().unwrap_or_else(|| j.database.clone());
        // in the engine's words since 2026-11-15 (the normalizer's second batch, item 4 (a), S376)
        let mjd = model
            .find_join(Some(&mid_db), &mid_hop.join_name)
            .ok_or_else(|| mapping_validation::join_not_found(&mid_hop.join_name, &mid_db))?;
        let mid_tgt = determine_target_table(
            &mjd.operation,
            Some(&prev_table),
            &mid_hop.join_name,
            Some(prop),
            h as i32,
            &cat!("mapping=", md.qualified_name()),
        )?;
        let ms = Arc::new(free_variable("s"));
        let mt = Arc::new(free_variable("t"));
        let mut mid_scope: TableScope = LinkedMap::new();
        mid_scope.put(
            prev_table.clone(),
            match &prev_alias {
                None => p::ValueSpecification::Variable(ms.clone()),
                // `new AppliedProperty(ms, prevAlias)`: no span, no owner class
                Some(alias) => p::ValueSpecification::AppliedProperty(Arc::new(p::AppliedProperty {
                    receiver: p::ValueSpecification::Variable(ms.clone()),
                    property: alias.clone(),
                    pos: None,
                    owner_class: None,
                })),
            },
        );
        if mid_tgt != prev_table {
            mid_scope.put(mid_tgt.clone(), p::ValueSpecification::Variable(mt.clone()));
        }
        let mid_cond = rel_op_translator::translate(
            &mjd.operation,
            &mid_scope,
            Some(&p::ValueSpecification::Variable(mt.clone())),
            None,
            None,
        )?;
        let mid_alias = cat!("nl__", prop, "__inb__", &mid_hop.join_name);
        steps.push(LiftMidStep {
            alias: mid_alias.clone(),
            db: mid_db,
            table: mid_tgt.clone(),
            cond: Arc::new(p::LambdaFunction {
                parameters: list(vec![ms, mt]),
                body: list(vec![mid_cond]),
                pos: None,
            }),
        });
        prev_table = mid_tgt;
        prev_alias = Some(mid_alias);
    }
    Ok(steps)
}

/// `UnionSynthesis.ownSharedKeys` (:903): `(db, canonical table, key)` -> the first member over it, for a
/// single-table hierarchy's shared key.
pub fn own_shared_keys(members: &[ClassMapping], model: &ModelIndex) -> Result<LinkedMap<Vec<JStr>, i32>, Thrown> {
    let mut by_table: LinkedMap<Vec<JStr>, Vec<i32>> = LinkedMap::new();
    for (o, m) in members.iter().enumerate() {
        if let ClassMapping::ClassMappingRelational(mr) = m
            && let Some(main) = &mr.main_table
            && let Some(key) = table_key(Some(m), model)?
        {
            by_table
                .get_or_insert_with(vec![main.database.clone(), canonical_table(&main.table), key], Vec::new)
                .push(o as i32);
        }
    }
    let mut out: LinkedMap<Vec<JStr>, i32> = LinkedMap::new();
    for (k, ordinals) in by_table.iter() {
        if ordinals.len() > 1 {
            out.put(k.clone(), ordinals[0]);
        }
    }
    Ok(out)
}

/// `UnionSynthesis.prePassedClosure` (:929): the PRE-PASSED records of the closure (each mapping's resolved record
/// where it has one, else the closure's), so a set that extends another navigates with the routes it inherited.
pub fn pre_passed_closure(
    md: &ResolvedMapping<'_>,
    resolved: &LinkedMap<JStr, ResolvedMapping<'_>>,
) -> Vec<Arc<LegacyMappingDefinition>> {
    let mut records = Vec::new();
    for m in md.closure() {
        match resolved.get(&m.qualified_name) {
            Some(r) => records.push(r.raw().clone()),
            None => records.push(m),
        }
    }
    records
}

/// `UnionSynthesis.recordKeyThreads` (:951): the key threads (`<col>_<n>`, `<col>__pk_<table>`) of an operation's
/// row into `ledger.union_key_threads`; a view-backed member gets none (ASK, low).
pub fn record_key_threads(
    // Java's parameter, never read (US:951)
    _md: &ResolvedMapping<'_>,
    class_name: &JStr,
    members: &[ClassMapping],
    shared_keys: &LinkedMap<Vec<JStr>, i32>,
    model: &ModelIndex,
    ledger: &mut MappingLedger,
) -> Result<(), Thrown> {
    let k = model.knowledge();
    let mut threads: Vec<Arc<KeyThread>> = Vec::new();
    for (o, m) in members.iter().enumerate() {
        let ClassMapping::ClassMappingRelational(mr) = m else {
            continue; // a Relation(~func) member has no key table
        };
        let Some(main) = &mr.main_table else {
            continue;
        };
        let (db, table) = (&main.database, &main.table);
        for col in member_primary_key(mr, model)? {
            if shared_keys.contains_key(&vec![db.clone(), canonical_table(table), col.clone()]) {
                continue;
            }
            let kind = k.column_kind(db, table, &col)?.map(JStr::from);
            threads.push(Arc::new(KeyThread::new(
                cat!(&col, "_", o.to_string()),
                kind,
                col,
                o as i32,
            )));
        }
    }
    // the SHARED table keys ride the same fact (ordinal -1): the stack builder projects them once per member over
    // the keyed table
    for (key, first_ordinal) in shared_keys.iter() {
        let (db, table, col) = (&key[0], &key[1], &key[2]);
        // Java's `(ClassMapping.Relational) members.get(i)`: `own_shared_keys` indexes only this list's Relational
        // members, so neither JDK exception is reachable
        let first = match usize::try_from(*first_ordinal).ok().and_then(|i| members.get(i)) {
            Some(ClassMapping::ClassMappingRelational(r)) => r,
            Some(_) => return Err(Thrown::java(CLASS_CAST, None)),
            None => return Err(Thrown::java(INDEX_OUT_OF_BOUNDS, None)),
        };
        let physical_table = &require(first.main_table.as_ref(), None)?.table;
        threads.push(Arc::new(KeyThread {
            name: shared_key_name(table, col),
            pure_kind: k.column_kind(db, physical_table, col)?.map(JStr::from),
            column: col.clone(),
            ordinal: -1,
            store: Some(db.clone()),
            table: Some(table.clone()),
        }));
    }
    ledger.union_key_threads.put(class_name.clone(), threads);
    Ok(())
}

/// `UnionSynthesis.memberPrimaryKey` (:988): a member's key columns -- its `~primaryKey` plain column refs of its
/// table, else the table's PRIMARY KEY columns in declaration order.
pub fn member_primary_key(mr: &ClassMappingRelational, model: &ModelIndex) -> Result<Vec<JStr>, Thrown> {
    let main = require(mr.main_table.as_ref(), None)?;
    let mut declared: Vec<JStr> = Vec::new();
    for op in mr.primary_key.iter() {
        if let RelationalOperation::RelationalOperationColumnRef(cr) = op
            && canonical_table(&cr.table) == canonical_table(&main.table)
        {
            declared.push(cr.column.clone());
        }
    }
    if !declared.is_empty() {
        return Ok(declared);
    }
    // MissProbe.miss: a view-backed member has no physical key
    let Some(td) = model.knowledge().table(Some(&main.database), Some(&main.table)) else {
        return Ok(Vec::new());
    };
    Ok(td
        .columns
        .iter()
        .filter(|c| c.primary_key)
        .map(|c| c.name.clone())
        .collect())
}

/// `UnionSynthesis.sharedKeyName` (:1016): `<col>__pk_<table>`, the table's characters outside `[A-Za-z0-9_]`
/// replaced per code point (`replaceAll`).
pub fn shared_key_name(table: &JStr, col: &JStr) -> JStr {
    // Java's regex reads code points: a surrogate pair is one character outside the class (one `_`), and so is a
    // lone surrogate
    let mut spelled: Vec<u16> = Vec::with_capacity(table.0.len());
    for c in char::decode_utf16(table.0.iter().copied()) {
        match c {
            Ok(ch) if ch.is_ascii_alphanumeric() || ch == '_' => spelled.push(ch as u16),
            _ => spelled.push(u16::from(b'_')),
        }
    }
    cat!(col, TABLE_KEY_SUFFIX, "_", spelled)
}

/// `ResolvedMapping.idOf` of a Relational set (Java's `ResolvedMapping::idOf` over a `ClassMapping.Relational`).
fn relational_id(r: &Arc<ClassMappingRelational>) -> JStr {
    ResolvedMapping::id_of(&ClassMapping::ClassMappingRelational(r.clone()))
}

/// `new Variable(name)`: an untyped free reference (no type, multiplicity or span).
fn free_variable(name: &str) -> p::Variable {
    p::Variable {
        name: JStr::from(name),
        r#type: None,
        multiplicity: None,
        pos: None,
    }
}

/// Java's `"" + s` of a nullable string: its text, or `"null"`.
fn null_or(s: Option<&JStr>) -> JStr {
    s.cloned().unwrap_or_else(|| JStr::from("null"))
}

/// `cm.getClass().getSimpleName()` of a class mapping: its Java record's simple name (`ClassMapping.Pure` is
/// `"Pure"`), spelled per variant.
fn java_simple_name(cm: &ClassMapping) -> &'static str {
    match cm {
        ClassMapping::ClassMappingRelational(_) => "Relational",
        ClassMapping::ClassMappingPure(_) => "Pure",
        ClassMapping::ClassMappingUnion(_) => "Union",
        ClassMapping::ClassMappingRelationFunction(_) => "RelationFunction",
        ClassMapping::ClassMappingInheritance(_) => "Inheritance",
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::rc::Rc;

    use lite_model::linked::empty;
    use lite_model::text::j;
    use lite_model::{
        AssociationMapping, AssociationMappingRelational, AssociationPropertyMapping, ClassMappingPure,
        JoinChainElement, LegacyMappingDefinitionTableReference as TableRef, PropertyMappingEmbedded,
        PropertyMappingInlineEmbedded, PropertyMappingLocalProperty, PropertyMappingOtherwiseEmbedded,
    };

    use super::*;
    use crate::lifted_views::LiftedViews;
    use crate::mapping_closures::MappingClosures;
    use crate::test_support::{cm, column_pm, index, join_pm, legacy, model, relational, union};

    const MODEL: &str = r#"
###Pure
Class a::Person
{
  name: String[1];
  firm: a::Firm[0..1];
  address: a::Address[0..1];
}
Class a::Firm
{
  name: String[1];
}
Class a::Address
{
  street: String[1];
  firm: a::Firm[0..1];
}
Class a::Animal
{
  name: String[1];
}
Class a::Dog extends a::Animal
{
  barks: Boolean[1];
}
Class a::Cat extends a::Animal
{
  purrs: Boolean[1];
}

###Relational
Database a::db
(
  Table PERSON (ID INTEGER PRIMARY KEY, NAME VARCHAR(200))
  Table FIRM (ID INTEGER PRIMARY KEY, NAME VARCHAR(200))
  Table PAIR (A INTEGER PRIMARY KEY, B INTEGER PRIMARY KEY)
  Table LOOSE (NAME VARCHAR(200))
)

###Mapping
Mapping a::X
(
  a::Firm[zz]: Relational
  {
    ~mainTable [a::db]FIRM
    name: [a::db]FIRM.NAME
  }
)
Mapping a::Y
(
  a::Firm[zz]: Relational
  {
    ~mainTable [a::db]FIRM
    name: [a::db]FIRM.NAME
  }
)
Mapping a::Z
(
  a::Firm[solo]: Relational
  {
    ~mainTable [a::db]FIRM
    name: [a::db]FIRM.NAME
  }
)
"#;

    /// A hand-made mapping record resolved over the model (no include: its closure is itself).
    fn resolved<'a>(
        md: LegacyMappingDefinition,
        closures: &'a MappingClosures<'a>,
        views: &'a LiftedViews<'a>,
    ) -> ResolvedMapping<'a> {
        let md = Arc::new(md);
        let closure = closures.closure(&md.qualified_name);
        ResolvedMapping::new(
            md.clone(),
            md,
            Rc::new(HashMap::new()),
            Rc::new(LinkedMap::new()),
            closure,
            views,
        )
    }

    fn over(table: &str, mut r: ClassMappingRelational) -> ClassMappingRelational {
        r.main_table = Some(Arc::new(TableRef {
            database: j("a::db"),
            table: j(table),
        }));
        r
    }

    fn firm_set(id: &str) -> ClassMapping {
        cm(over("LOOSE", relational("a::Firm", Some(id), vec![])))
    }

    fn pure_set(class: &str, id: &str) -> ClassMapping {
        ClassMapping::ClassMappingPure(Arc::new(ClassMappingPure {
            class_name: j(class),
            set_id: Some(j(id)),
            extends_set_id: None,
            root: false,
            source_class: None,
            filter: None,
            property_bindings: empty(),
        }))
    }

    fn person(id: &str, pms: Vec<PropertyMapping>) -> ClassMapping {
        cm(over("PERSON", relational("a::Person", Some(id), pms)))
    }

    fn rel(c: &ClassMapping) -> Arc<ClassMappingRelational> {
        match c {
            ClassMapping::ClassMappingRelational(r) => r.clone(),
            _ => panic!("a Relational set"),
        }
    }

    fn the_union(c: &ClassMapping) -> Arc<ClassMappingUnion> {
        match c {
            ClassMapping::ClassMappingUnion(u) => u.clone(),
            _ => panic!("a Union set"),
        }
    }

    fn hop(name: &str, db: Option<&str>) -> Arc<JoinChainElement> {
        Arc::new(JoinChainElement {
            join_name: j(name),
            join_type: None,
            database_name: db.map(j),
            include_self: false,
        })
    }

    fn chain(prop: &str, hops: Vec<Arc<JoinChainElement>>) -> Arc<PropertyMappingJoin> {
        Arc::new(PropertyMappingJoin {
            property_name: j(prop),
            database: j("db"),
            joins: list(hops),
            target_set_id: None,
        })
    }

    fn fn_name(v: &p::ValueSpecification) -> JStr {
        match v {
            p::ValueSpecification::AppliedFunction(af) => af.function.clone(),
            _ => panic!("a call"),
        }
    }

    fn args(v: &p::ValueSpecification) -> Vec<p::ValueSpecification> {
        match v {
            p::ValueSpecification::AppliedFunction(af) => af.parameters.to_vec(),
            _ => panic!("a call"),
        }
    }

    #[test]
    fn a_member_function_is_named_by_its_defining_mapping_as_the_driver_names_it() {
        let parsed = model(MODEL);
        let idx = index(&parsed);
        let closures = MappingClosures::new(&idx);
        let views = LiftedViews::new(&parsed, &idx);
        let md = resolved(
            legacy("a::M", vec![firm_set("f1"), firm_set("f2"), person("p", vec![])]),
            &closures,
            &views,
        );
        // the sole set of its class: by class
        assert_eq!(
            member_function(&md, &person("p", vec![])).unwrap(),
            j("a::M$class$a::Person")
        );
        // a non-root set of a two-set class: by class and set id
        assert_eq!(
            member_function(&md, &firm_set("f1")).unwrap(),
            j("a::M$class$a::Firm$set$f1")
        );
        // a set the closure does not define: named under the closure's first record
        assert_eq!(
            member_function(&md, &firm_set("zz")).unwrap(),
            j("a::M$class$a::Firm$set$zz")
        );
        // a root set: by class, however many sets its class has
        let mut root = over("LOOSE", relational("a::Firm", Some("f1"), vec![]));
        root.root = true;
        let pt = PinTarget {
            defining: Arc::new(legacy("a::R", vec![cm(root.clone()), firm_set("f2")])),
            set: cm(root),
        };
        assert_eq!(member_function_of(&pt).unwrap(), j("a::R$class$a::Firm"));
    }

    #[test]
    fn a_pin_resolves_in_the_closure_then_model_wide_by_a_unique_id() {
        let parsed = model(MODEL);
        let idx = index(&parsed);
        let closures = MappingClosures::new(&idx);
        let views = LiftedViews::new(&parsed, &idx);
        let md = resolved(legacy("a::M", vec![firm_set("f1")]), &closures, &views);
        assert_eq!(resolve_pin(&md, None, &idx).unwrap(), None);
        let own = resolve_pin(&md, Some(&j("f1")), &idx).unwrap().expect("own set");
        assert_eq!(own.defining.qualified_name, j("a::M"));
        assert_eq!(own.set, firm_set("f1"));
        let outside = resolve_pin(&md, Some(&j("solo")), &idx).unwrap().expect("a unique id");
        assert_eq!(outside.defining.qualified_name, j("a::Z"));
        // two mappings define `zz`: ambiguous, so unresolved; the owners in the model's order
        assert_eq!(resolve_pin(&md, Some(&j("zz")), &idx).unwrap(), None);
        let owners: Vec<JStr> = pin_owners(&j("zz"), &idx)
            .iter()
            .map(|o| o.defining.qualified_name.clone())
            .collect();
        assert_eq!(owners, vec![j("a::X"), j("a::Y")]);
        assert_eq!(resolve_pin(&md, Some(&j("nope")), &idx).unwrap(), None);
        assert!(pin_owners(&j("nope"), &idx).is_empty());
    }

    #[test]
    fn a_pm_identity_is_its_name_and_route() {
        assert_eq!(pm_identity(&join_pm("firm", Some("f1"))), j("firm f1"));
        assert_eq!(pm_identity(&join_pm("firm", None)), j("firm "));
        assert_eq!(pm_identity(&column_pm("name", "PERSON", "NAME")), j("name "));
    }

    #[test]
    fn routed_joins_are_collected_through_embedded_otherwise_inline_and_local_blocks() {
        let parsed = model(MODEL);
        let idx = index(&parsed);
        let closures = MappingClosures::new(&idx);
        let views = LiftedViews::new(&parsed, &idx);
        let inline_target = person("p4", vec![join_pm("firm", Some("f1"))]);
        let md = resolved(legacy("a::M", vec![inline_target]), &closures, &views);
        let embedded = PropertyMapping::PropertyMappingEmbedded(Arc::new(PropertyMappingEmbedded::new(
            j("address"),
            list(vec![join_pm("firm", Some("f2")), column_pm("street", "PERSON", "NAME")]),
        )));
        let mut routed = LinkedMap::new();
        let mut owners = LinkedMap::new();
        collect_routed_joins(&[embedded], &j("a::Person"), &md, &idx, &mut routed, &mut owners).unwrap();
        assert_eq!(routed.get(&j("firm")).map(Vec::len), Some(1));
        // the route resolves against the EMBEDDED class
        assert_eq!(owners.get(&j("firm")), Some(&j("a::Address")));

        // an otherwise block's fallback is a route pinned to its fallback set; an Inline splice and a local property
        // descend into their bodies
        let otherwise = PropertyMapping::PropertyMappingOtherwiseEmbedded(Arc::new(PropertyMappingOtherwiseEmbedded {
            property_name: j("firm"),
            embedded: empty(),
            fallback_set_id: j("f3"),
            fallback: join_pm("firm", None),
        }));
        let inline = PropertyMapping::PropertyMappingInlineEmbedded(Arc::new(PropertyMappingInlineEmbedded {
            property_name: j("other"),
            set_id: j("p4"),
        }));
        let local = PropertyMapping::PropertyMappingLocalProperty(Arc::new(PropertyMappingLocalProperty {
            property_name: j("local"),
            r#type: p::TypeExpression::TypeExpressionNameRef(Box::new(p::TypeExpressionNameRef {
                name: j("a::Firm"),
                pos: None,
            })),
            multiplicity: p::Multiplicity::MultiplicityConcrete(Box::new(p::MultiplicityConcrete {
                lower_bound: 0,
                upper_bound: Some(1),
            })),
            body: join_pm("local", Some("f1")),
        }));
        let mut routed = LinkedMap::new();
        let mut owners = LinkedMap::new();
        collect_routed_joins(
            &[otherwise, inline, local],
            &j("a::Person"),
            &md,
            &idx,
            &mut routed,
            &mut owners,
        )
        .unwrap();
        let firm: Vec<Option<JStr>> = routed
            .get(&j("firm"))
            .unwrap()
            .iter()
            .map(|r| r.target_set_id.clone())
            .collect();
        assert_eq!(firm, vec![Some(j("f3")), Some(j("f1"))]);
        assert_eq!(routed.get(&j("local")).map(Vec::len), Some(1));
        assert_eq!(
            owners.iter().cloned().collect::<Vec<_>>(),
            vec![(j("firm"), j("a::Person")), (j("local"), j("a::Person"))]
        );
    }

    #[test]
    fn a_property_routed_under_two_owners_and_a_splice_cycle_are_loud() {
        let parsed = model(MODEL);
        let idx = index(&parsed);
        let closures = MappingClosures::new(&idx);
        let views = LiftedViews::new(&parsed, &idx);
        let splice = |prop: &str, set: &str| {
            PropertyMapping::PropertyMappingInlineEmbedded(Arc::new(PropertyMappingInlineEmbedded {
                property_name: j(prop),
                set_id: j(set),
            }))
        };
        let md = resolved(
            legacy(
                "a::M",
                vec![
                    person("p1", vec![splice("boss", "p2")]),
                    person("p2", vec![splice("boss", "p1")]),
                ],
            ),
            &closures,
            &views,
        );
        let embedded = PropertyMapping::PropertyMappingEmbedded(Arc::new(PropertyMappingEmbedded::new(
            j("address"),
            list(vec![join_pm("firm", Some("f2"))]),
        )));
        let (mut routed, mut owners) = (LinkedMap::new(), LinkedMap::new());
        let e = collect_routed_joins(
            &[join_pm("firm", Some("f1")), embedded],
            &j("a::Person"),
            &md,
            &idx,
            &mut routed,
            &mut owners,
        )
        .unwrap_err();
        assert!(e.is_model());
        assert_eq!(e.phase(), Some(Phase::Normalize));
        assert_eq!(
            e.message(),
            Some(j(
                "property 'firm' is routed under two owners, 'a::Person' and 'a::Address', in one class mapping; \
                 routes are keyed by property name, so the mapping cannot be normalized; mapping=a::M"
            ))
        );
        let (mut routed, mut owners) = (LinkedMap::new(), LinkedMap::new());
        let mut splicing = LinkedSet::new();
        let e = collect_routed_joins_splicing(
            &[splice("boss", "p2")],
            &j("a::Person"),
            &md,
            &idx,
            &mut routed,
            &mut owners,
            &mut splicing,
        )
        .unwrap_err();
        assert!(e.is_model());
        assert_eq!(
            e.message(),
            Some(j(
                "Cycle materializing Inline embedded set 'p2' via 'boss' on 'a::Person': the splice chain [p2, p1] \
                 returns to it; mapping=a::M"
            ))
        );
        // every splice left the chain on the way out (Java's finally)
        assert!(splicing.is_empty());
    }

    #[test]
    fn an_embedded_owner_is_the_property_s_class() {
        let parsed = model(MODEL);
        let idx = index(&parsed);
        assert_eq!(
            embedded_owner(&j("a::Person"), &j("address"), &idx).unwrap(),
            Some(j("a::Address"))
        );
        assert_eq!(embedded_owner(&j("a::Person"), &j("name"), &idx).unwrap(), None);
        assert_eq!(embedded_owner(&j("a::Nobody"), &j("address"), &idx).unwrap(), None);
    }

    #[test]
    fn routes_classify_as_members_root_pinned_dead_or_poison() {
        let parsed = model(MODEL);
        let idx = index(&parsed);
        let closures = MappingClosures::new(&idx);
        let views = LiftedViews::new(&parsed, &idx);
        let mut fu = union("a::Firm", Some("fu"), &["f1", "f2"]);
        if let ClassMapping::ClassMappingUnion(u) = &mut fu {
            Arc::make_mut(u).root = true;
        }
        let address = cm(over("LOOSE", relational("a::Address", Some("ad1"), vec![])));
        let routed = |id: &str, pins: &[&str]| {
            person(
                id,
                pins.iter()
                    .map(|pin| {
                        if let Some(rest) = pin.strip_prefix("address:") {
                            join_pm("address", Some(rest))
                        } else {
                            join_pm("firm", Some(pin))
                        }
                    })
                    .collect(),
            )
        };
        let sets = vec![
            firm_set("f1"),
            firm_set("f2"),
            firm_set("f3"),
            fu,
            address,
            routed("pa", &["f1", "f2"]),
            routed("pb", &["f1", "f3"]),
            routed("pc", &["nope"]),
            routed("pd", &["zz"]),
            routed("pe", &["f3"]),
            routed("pf", &["address:ad1"]),
        ];
        let md = resolved(legacy("a::M", sets.clone()), &closures, &views);
        let expr = p::ValueSpecification::CString(Arc::new(p::CString {
            value: j("x"),
            pos: None,
            multi_line: false,
        }));
        let classify = |set: &ClassMapping| {
            let mut ledger = MappingLedger::new(HashSet::new());
            let mut p = Pipeline::new(expr.clone(), &mut ledger, &views);
            classify_union_routes(&md, &rel(set), &idx, &mut p).unwrap();
            let routes: Vec<(JStr, Vec<RouteTarget>)> = p
                .union_routes
                .iter()
                .map(|(k, v)| (k.clone(), v.iter().map(|r| r.target).collect()))
                .collect();
            let dropped = p.dropped_routed_props.clone();
            drop(p);
            (routes, dropped, ledger.poisons.iter().cloned().collect::<Vec<_>>())
        };
        let firm = j("firm");
        // every route a member of the target's union: one navigate over the members
        let (routes, dropped, poisons) = classify(&sets[5]);
        assert_eq!(
            routes,
            vec![(firm.clone(), vec![RouteTarget::Member, RouteTarget::Member])]
        );
        assert!(dropped.is_empty() && poisons.is_empty());
        // a non-member pin beside another pin is dead
        assert_eq!(classify(&sets[6]).0, vec![(firm.clone(), vec![RouteTarget::Member])]);
        // an unknown pin drops the property, poisoned on the class
        let (routes, dropped, poisons) = classify(&sets[7]);
        assert!(routes.is_empty());
        assert!(dropped.contains(&firm));
        assert_eq!(
            poisons,
            vec![(
                PoisonKey::PoisonKeyForClass(Arc::new(PoisonKeyForClass {
                    class_fqn: j("a::Person")
                })),
                j("property 'firm' routes to unknown mapping set 'nope'; the property is dropped from this synthesis")
            )]
        );
        // an ambiguous pin names its owners
        assert_eq!(
            classify(&sets[8]).2[0].1,
            j(
                "property 'firm' routes to an AMBIGUOUS mapping set 'zz' (not visible through the includes; defined by \
               [a::X, a::Y]); the property is dropped from this synthesis"
            )
        );
        // a property's ONLY pin to a non-root, non-member set is that set
        assert_eq!(classify(&sets[9]).0, vec![(firm, vec![RouteTarget::PinnedSingle])]);
        // a route to the target's sole set is the un-routed navigation
        let (routes, dropped, poisons) = classify(&sets[10]);
        assert!(routes.is_empty() && dropped.is_empty() && poisons.is_empty());
    }

    #[test]
    fn a_union_stacks_its_members_in_order_and_records_its_facts() {
        let parsed = model(MODEL);
        let idx = index(&parsed);
        let closures = MappingClosures::new(&idx);
        let views = LiftedViews::new(&parsed, &idx);
        let fu = union("a::Firm", Some("fu"), &["f2", "f1"]);
        let md = resolved(
            legacy("a::M", vec![firm_set("f1"), firm_set("f2"), fu.clone()]),
            &closures,
            &views,
        );
        let mut ledger = MappingLedger::new(HashSet::new());
        let body = synth_union(&md, &the_union(&fu), &idx, &mut ledger).unwrap();
        let expected = p::ValueSpecification::AppliedFunction(Arc::new(calls::platform(
            &j("concatenate"),
            vec![
                p::ValueSpecification::AppliedFunction(Arc::new(calls::exact(&j("a::M$class$a::Firm$set$f2"), vec![]))),
                p::ValueSpecification::AppliedFunction(Arc::new(calls::exact(&j("a::M$class$a::Firm$set$f1"), vec![]))),
            ],
        )));
        assert_eq!(body, expected);
        let arms: Vec<JStr> = args(&body).iter().map(fn_name).collect();
        assert_eq!(
            arms,
            vec![j("a::M$class$a::Firm$set$f2"), j("a::M$class$a::Firm$set$f1")]
        );
        assert_eq!(ledger.operation_members.get(&j("fu")), Some(&vec![j("f2"), j("f1")]));
        // no member over a keyed table: the fact is an empty thread list
        assert_eq!(ledger.union_key_threads.get(&j("a::Firm")).map(Vec::len), Some(0));
        assert!(ledger.mixed_unions.is_empty());
    }

    #[test]
    fn a_union_refuses_its_members_in_javas_order_and_a_mixed_union_survives_its_throw() {
        let parsed = model(MODEL);
        let idx = index(&parsed);
        let closures = MappingClosures::new(&idx);
        let views = LiftedViews::new(&parsed, &idx);
        let md = resolved(
            legacy(
                "a::M",
                vec![
                    firm_set("f1"),
                    pure_set("a::Firm", "m2m"),
                    union("a::Firm", Some("inner"), &["f1"]),
                    person("p", vec![]),
                ],
            ),
            &closures,
            &views,
        );
        let refusal = |members: &[&str]| {
            let mut ledger = MappingLedger::new(HashSet::new());
            let e = synth_union(
                &md,
                &the_union(&union("a::Firm", Some("u"), members)),
                &idx,
                &mut ledger,
            )
            .unwrap_err();
            (e, ledger.mixed_unions.iter().cloned().collect::<Vec<_>>())
        };
        let (e, _) = refusal(&[]);
        assert!(e.is_not_implemented());
        assert_eq!(
            e.message(),
            Some(j("Operation union with no member sets; class=a::Firm, mapping=a::M"))
        );
        let (e, mixed) = refusal(&["f1", "m2m"]);
        assert!(e.is_not_implemented());
        assert_eq!(
            e.message(),
            Some(j(
                "Operation union member set 'm2m' of class 'a::Firm' is a Pure (M2M) set \u{2014} the mixed-kind union \
                 extent synthesizes at the resolver; mapping=a::M"
            ))
        );
        assert_eq!(mixed, vec![(j("a::Firm"), vec![j("f1"), j("m2m")])]);
        let (e, mixed) = refusal(&["f1", "ghost"]);
        assert_eq!(
            e.message(),
            Some(j(
                "Operation union member set 'ghost' of class 'a::Firm' is missing; mapping=a::M"
            ))
        );
        assert!(mixed.is_empty());
        let (e, _) = refusal(&["inner"]);
        assert_eq!(
            e.message(),
            Some(j(
                "Operation union member set 'inner' of class 'a::Firm' is not a Relational or Relation(~func) set; \
                 mapping=a::M"
            ))
        );
        let (e, _) = refusal(&["p"]);
        assert!(e.is_model());
        assert_eq!(e.phase(), Some(Phase::Normalize));
        assert_eq!(
            e.message(),
            Some(j(
                "Operation union member set 'p' maps 'a::Person', which is not 'a::Firm' or a subclass; mapping=a::M"
            ))
        );
        // the pair entries are read after every member passed: an unknown association is the closure's refusal
        let mut with_pairs = legacy("a::M", vec![firm_set("f1")]);
        with_pairs.association_mappings = list(vec![AssociationMapping::AssociationMappingRelational(Arc::new(
            AssociationMappingRelational {
                association_name: j("a::Nope"),
                property_mappings: list(vec![Arc::new(AssociationPropertyMapping {
                    source_set_id: Some(j("f1")),
                    target_set_id: Some(j("p")),
                    body: join_pm("employees", None),
                })]),
            },
        ))]);
        let paired = resolved(with_pairs, &closures, &views);
        let mut ledger = MappingLedger::new(HashSet::new());
        let e = synth_union(
            &paired,
            &the_union(&union("a::Firm", Some("u"), &["f1"])),
            &idx,
            &mut ledger,
        )
        .unwrap_err();
        assert!(e.is_model());
        assert_eq!(e.message(), Some(j("Can't find association 'a::Nope'")));
        assert!(ledger.operation_members.is_empty());
        // a stack of no members records its (empty) arms, then refuses
        let mut ledger = MappingLedger::new(HashSet::new());
        let e = stack_body(&md, &union("a::Firm", Some("u"), &[]), &[], &mut ledger).unwrap_err();
        assert_eq!(e, Thrown::null_pointer(Some("an operation with no members")));
        assert_eq!(ledger.operation_members.get(&j("u")), Some(&Vec::new()));
    }

    #[test]
    fn an_inheritance_operation_stacks_its_leaves_mapped_sets() {
        let parsed = model(MODEL);
        let idx = index(&parsed);
        let closures = MappingClosures::new(&idx);
        let views = LiftedViews::new(&parsed, &idx);
        let ih = Arc::new(ClassMappingInheritance {
            class_name: j("a::Animal"),
            set_id: Some(j("an")),
            extends_set_id: None,
            root: true,
        });
        let cat = cm(over("LOOSE", relational("a::Cat", Some("c"), vec![])));
        let dog = cm(over("LOOSE", relational("a::Dog", Some("d"), vec![])));
        let md = resolved(
            legacy(
                "a::M",
                vec![
                    cat.clone(),
                    dog.clone(),
                    ClassMapping::ClassMappingInheritance(ih.clone()),
                ],
            ),
            &closures,
            &views,
        );
        // the model's class order (Dog before Cat), never the mapping's
        let members: Vec<JStr> = inheritance_members(&md, &ih, &idx)
            .unwrap()
            .iter()
            .map(relational_id)
            .collect();
        assert_eq!(members, vec![j("d"), j("c")]);
        let mut ledger = MappingLedger::new(HashSet::new());
        let body = synth_inheritance(&md, &ih, &idx, &mut ledger).unwrap();
        let arms: Vec<JStr> = args(&body).iter().map(fn_name).collect();
        assert_eq!(arms, vec![j("a::M$class$a::Dog"), j("a::M$class$a::Cat")]);
        assert_eq!(ledger.operation_members.get(&j("an")), Some(&vec![j("d"), j("c")]));
        // a member that is no Relational set is refused by its Java class's simple name
        let md = resolved(
            legacy(
                "a::M",
                vec![
                    pure_set("a::Cat", "c"),
                    dog,
                    ClassMapping::ClassMappingInheritance(ih.clone()),
                ],
            ),
            &closures,
            &views,
        );
        let e = inheritance_members(&md, &ih, &idx).unwrap_err();
        assert!(e.is_not_implemented());
        assert_eq!(
            e.message(),
            Some(j(
                "inheritance Operation member for 'a::Cat' is a Pure mapping \u{2014} not supported yet; mapping=a::M"
            ))
        );
        // no mapped subclass at all
        let md = resolved(
            legacy("a::M", vec![ClassMapping::ClassMappingInheritance(ih.clone())]),
            &closures,
            &views,
        );
        let mut ledger = MappingLedger::new(HashSet::new());
        let e = synth_inheritance(&md, &ih, &idx, &mut ledger).unwrap_err();
        assert_eq!(
            e.message(),
            Some(j(
                "inheritance Operation for 'a::Animal' finds no mapped subclass sets; mapping=a::M"
            ))
        );
    }

    #[test]
    fn a_table_key_is_the_sole_primary_key_of_a_physical_main_table() {
        let parsed = model(MODEL);
        let idx = index(&parsed);
        let over_table = |t: &str| cm(over(t, relational("a::Firm", Some("f"), vec![])));
        assert_eq!(table_key(Some(&over_table("FIRM")), &idx).unwrap(), Some(j("ID")));
        assert_eq!(table_key(Some(&over_table("PAIR")), &idx).unwrap(), None);
        assert_eq!(table_key(Some(&over_table("LOOSE")), &idx).unwrap(), None);
        assert_eq!(table_key(Some(&over_table("NOPE")), &idx).unwrap(), None);
        assert_eq!(
            table_key(Some(&cm(relational("a::Firm", None, vec![]))), &idx).unwrap(),
            None
        );
        assert_eq!(table_key(None, &idx).unwrap(), None);
        // no declared ~primaryKey: the table's PRIMARY KEY columns in declaration order; none for no physical table
        let pair = over("PAIR", relational("a::Firm", Some("f"), vec![]));
        assert_eq!(member_primary_key(&pair, &idx).unwrap(), vec![j("A"), j("B")]);
        let view = over("NOPE", relational("a::Firm", Some("f"), vec![]));
        assert!(member_primary_key(&view, &idx).unwrap().is_empty());
        assert_eq!(
            member_primary_key(&relational("a::Firm", Some("f"), vec![]), &idx).unwrap_err(),
            Thrown::null_pointer(None)
        );
    }

    #[test]
    fn a_shared_key_name_replaces_per_code_point() {
        assert_eq!(shared_key_name(&j("my.tab-\u{1D11E}"), &j("ID")), j("ID__pk_my_tab__"));
        let lone = JStr(vec![u16::from(b'a'), 0xD800, u16::from(b'b')]);
        assert_eq!(shared_key_name(&lone, &j("x")), j("x__pk_a_b"));
        assert_eq!(shared_key_name(&j("T_1"), &j("id")), j("id__pk_T_1"));
    }

    #[test]
    fn chained_routes_are_uniform_when_every_prefix_agrees() {
        assert!(uniform_chained_routes(&[]));
        assert!(uniform_chained_routes(&[
            chain("f", vec![hop("A", None)]),
            chain("f", vec![hop("B", None)])
        ]));
        let ab = chain("f", vec![hop("A", None), hop("B", None)]);
        assert!(uniform_chained_routes(&[
            ab.clone(),
            chain("f", vec![hop("A", None), hop("C", None)])
        ]));
        assert!(!uniform_chained_routes(&[
            ab.clone(),
            chain("f", vec![hop("X", None), hop("B", None)])
        ]));
        assert!(!uniform_chained_routes(&[ab.clone(), chain("f", vec![hop("A", None)])]));
        // a hop's own database, else its PM's
        assert!(uniform_chained_routes(&[
            ab.clone(),
            chain("f", vec![hop("A", Some("db")), hop("B", None)])
        ]));
        assert!(!uniform_chained_routes(&[
            ab.clone(),
            chain("f", vec![hop("A", Some("db2")), hop("B", None)])
        ]));
        let route = |target: RouteTarget, prop: &str| UnionRoute {
            target,
            join: chain(prop, vec![hop("A", None)]),
        };
        let joins: Vec<JStr> = member_joins(&[
            route(RouteTarget::Member, "m1"),
            route(RouteTarget::Root, "r"),
            route(RouteTarget::PinnedSingle, "s"),
            route(RouteTarget::Member, "m2"),
        ])
        .iter()
        .map(|j| j.property_name.clone())
        .collect();
        assert_eq!(joins, vec![j("m1"), j("m2")]);
    }

    /// US:797's record equality is Java's `equals` (the normalizer's second batch, item 4 (b), ruling 37 (b),
    /// 2026-11-15): a binding's expression read at another position, or the target-set map in another order, is the
    /// one set (the whole codec text, the comparison before, had each two); a `Double` `0.0` and `-0.0` are two.
    #[test]
    fn two_sets_equal_but_for_a_source_position_or_a_map_order_are_one() {
        let at = |line: i32, v: f64| {
            p::ValueSpecification::CFloat(Arc::new(p::CFloat {
                value: v,
                exact: None,
                pos: Some(Box::new(p::SourceInfo {
                    source_id: j(""),
                    start_line: line,
                    start_column: 1,
                    end_line: line,
                    end_column: 3,
                })),
            }))
        };
        let pure = |e: p::ValueSpecification| {
            ClassMapping::ClassMappingPure(Arc::new(ClassMappingPure {
                class_name: j("a::Firm"),
                set_id: None,
                extends_set_id: None,
                root: true,
                source_class: None,
                filter: None,
                property_bindings: list(vec![Arc::new(lite_model::ClassMappingPurePropertyBinding {
                    property_name: j("name"),
                    expression: e,
                    source_set_id: None,
                    target_set_id: None,
                    explode: false,
                    local: false,
                    enum_mapping_id: None,
                })]),
            }))
        };
        assert!(java_record_equal(&pure(at(1, 1.5)), &pure(at(9, 1.5))));
        assert_ne!(
            lite_front::codec::write(&pure(at(1, 1.5)), false),
            lite_front::codec::write(&pure(at(9, 1.5)), false)
        );
        assert!(!java_record_equal(&pure(at(1, 0.0)), &pure(at(1, -0.0))));
        let mut a = relational("a::Firm", None, vec![]);
        a.property_target_sets = list(vec![(j("x"), j("s1")), (j("y"), j("s2"))]);
        let mut b = a.clone();
        b.property_target_sets = list(vec![(j("y"), j("s2")), (j("x"), j("s1"))]);
        assert!(java_record_equal(&cm(a.clone()), &cm(b)));
        let mut c = a.clone();
        c.property_target_sets = list(vec![(j("x"), j("s2")), (j("y"), j("s1"))]);
        assert!(!java_record_equal(&cm(a), &cm(c)));
    }

    #[test]
    fn an_inbound_arm_has_no_step_for_one_hop_and_is_loud_for_an_unknown_join() {
        let parsed = model(MODEL);
        let idx = index(&parsed);
        let closures = MappingClosures::new(&idx);
        let views = LiftedViews::new(&parsed, &idx);
        let md = resolved(legacy("a::M", vec![]), &closures, &views);
        let one = chain("firm", vec![hop("A", None)]);
        assert!(
            inbound_arm_steps(&one, &j("firm"), &j("FIRM"), &md, &idx)
                .unwrap()
                .is_empty()
        );
        let two = chain("firm", vec![hop("A", None), hop("Missing", Some("a::db"))]);
        let e = inbound_arm_steps(&two, &j("firm"), &j("FIRM"), &md, &idx).unwrap_err();
        assert!(e.is_model());
        // the engine's words since 2026-11-15 (the normalizer's second batch, item 4 (a), S376): it pinned "Join
        // 'Missing' not found in db 'a::db'; PM='firm', mapping=a::M"
        assert_eq!(e.message(), Some(j("Can't find join 'Missing' in database 'db'")));
    }
}
