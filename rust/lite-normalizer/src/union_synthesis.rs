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
use crate::pipeline::Pipeline;
use crate::rel_op_translator::{self, TableScope};
use crate::resolved_mapping::ResolvedMapping;
use crate::unported;

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
    Err(Thrown::Unported("UnionSynthesis.stackBody"))
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
    Err(Thrown::Unported("UnionSynthesis.synthUnion"))
}

/// `UnionSynthesis.synthInheritance` (:525): an Inheritance operation's stack over its members.
pub fn synth_inheritance(
    md: &ResolvedMapping<'_>,
    ih: &Arc<ClassMappingInheritance>,
    model: &ModelIndex,
    ledger: &mut MappingLedger,
) -> Result<p::ValueSpecification, Thrown> {
    Err(Thrown::Unported("UnionSynthesis.synthInheritance"))
}

/// `UnionSynthesis.recordKeyThreadsOf` (:560): the members' key threads into `ledger.union_key_threads`.
pub fn record_key_threads_of(
    md: &ResolvedMapping<'_>,
    class_name: &JStr,
    members: &[ClassMapping],
    model: &ModelIndex,
    ledger: &mut MappingLedger,
) -> Result<(), Thrown> {
    Err(Thrown::Unported("UnionSynthesis.recordKeyThreadsOf"))
}

/// `UnionSynthesis.inheritanceMembers` (:585): an Inheritance operation's member sets, in the walk's order.
pub fn inheritance_members(
    md: &ResolvedMapping<'_>,
    ih: &ClassMappingInheritance,
    model: &ModelIndex,
) -> Result<Vec<Arc<ClassMappingRelational>>, Thrown> {
    Err(Thrown::Unported("UnionSynthesis.inheritanceMembers"))
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
    Err(Thrown::Unported("UnionSynthesis.collectInheritanceMembers"))
}

/// `UnionSynthesis.tableKey` (:768): the sole PRIMARY KEY column of a member set's main table, else none.
pub fn table_key(cm: Option<&ClassMapping>, model: &ModelIndex) -> Result<Option<JStr>, Thrown> {
    Err(Thrown::Unported("UnionSynthesis.tableKey"))
}

/// `UnionSynthesis.uniformChainedRoutes` (:798): whether the member routes' chains are one shape.
pub fn uniform_chained_routes(member_js: &[Arc<PropertyMappingJoin>]) -> bool {
    // UNPORTED: UnionSynthesis.uniformChainedRoutes
    unported!("UnionSynthesis.uniformChainedRoutes")
}

/// `UnionSynthesis.memberJoins` (:825): the joins of the MEMBER routes, in route order.
pub fn member_joins(routes: &[UnionRoute]) -> Vec<Arc<PropertyMappingJoin>> {
    // UNPORTED: UnionSynthesis.memberJoins
    unported!("UnionSynthesis.memberJoins")
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
    Err(Thrown::Unported("UnionSynthesis.inboundArmSteps"))
}

/// `UnionSynthesis.ownSharedKeys` (:903): `(db, canonical table, key)` -> the first member over it, for a
/// single-table hierarchy's shared key.
pub fn own_shared_keys(members: &[ClassMapping], model: &ModelIndex) -> Result<LinkedMap<Vec<JStr>, i32>, Thrown> {
    Err(Thrown::Unported("UnionSynthesis.ownSharedKeys"))
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
    md: &ResolvedMapping<'_>,
    class_name: &JStr,
    members: &[ClassMapping],
    shared_keys: &LinkedMap<Vec<JStr>, i32>,
    model: &ModelIndex,
    ledger: &mut MappingLedger,
) -> Result<(), Thrown> {
    Err(Thrown::Unported("UnionSynthesis.recordKeyThreads"))
}

/// `UnionSynthesis.memberPrimaryKey` (:988): a member's key columns -- its `~primaryKey` plain column refs of its
/// table, else the table's PRIMARY KEY columns in declaration order.
pub fn member_primary_key(mr: &ClassMappingRelational, model: &ModelIndex) -> Result<Vec<JStr>, Thrown> {
    Err(Thrown::Unported("UnionSynthesis.memberPrimaryKey"))
}

/// `UnionSynthesis.sharedKeyName` (:1016): `<col>__pk_<table>`, the table's characters outside `[A-Za-z0-9_]`
/// replaced per code point (`replaceAll`).
pub fn shared_key_name(table: &JStr, col: &JStr) -> JStr {
    // UNPORTED: UnionSynthesis.sharedKeyName
    unported!("UnionSynthesis.sharedKeyName")
}

/// `ResolvedMapping.idOf` of a Relational set (Java's `ResolvedMapping::idOf` over a `ClassMapping.Relational`).
fn relational_id(r: &Arc<ClassMappingRelational>) -> JStr {
    ResolvedMapping::id_of(&ClassMapping::ClassMappingRelational(r.clone()))
}

/// Java's `"" + s` of a nullable string: its text, or `"null"`.
fn null_or(s: Option<&JStr>) -> JStr {
    s.cloned().unwrap_or_else(|| JStr::from("null"))
}
