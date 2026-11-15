// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! `JoinChainEmission` (JoinChainEmission.java at ad86fa895): the join hops of a relational set's pipeline -- the
//! `joinSlot`, `legacyNavigate` and `route(...)` steps and their slot names (`exchange_nav_name` etc. reach SQL).
//! EF_CENSUS §1.2's families: JCE-1 pass-1 structural hoisting (:45-225), JCE-2 chain emission (:235-597, :822-911,
//! :1063-1078), JCE-3 slot naming and lookup (:470-477, :603-698), JCE-4 class-typed target predicates (:700-738),
//! JCE-5 JoinNav collectors (:740-817), JCE-6 the INNER mapping-filter source and `nullTolerant` (:921-1051).
//!
//! RE-EXPRESSED (census §4): JCE:884's `catch (ModelException unmappedTarget)` is
//! `mapping_normalizer::main_table_or_null(..)?.is_none()` (the throw's one cause; its message is discarded); the
//! "first Relational set with this id" lookups (:132-139, :758-771) are `ResolvedMapping::first_relational`. The
//! dead null checks after `orElseThrow` are not ported. `p.unionRoutes.remove` (:307) mutates the frame mid-emission:
//! `LinkedMap::remove`. `equalsIgnoreCase` is `jdk::equals_ignore_case` (Java's fold). `nullTolerant`'s
//! `default -> false` (:1041) is ASK 12's: port as Java has it.

use std::sync::Arc;

use lite_builtin::dyna_fn::DynaFn;
use lite_error::Phase;
use lite_front::records as p;
use lite_model::{
    ClassMappingRelational, FilterMappingJoinMediated, JStr, JoinChainElement, PropertyMapping,
    PropertyMappingOtherwiseEmbedded, RelationalOperation, cat,
};
use lite_names::ModelIndex;
use lite_names::calls;
use lite_names::model_index::java_list;
use lite_types::platform::ROWS_MARKER;

use crate::error::Thrown;
use crate::jdk::{LinkedSet, equals_ignore_case};
use crate::mapping_ledger::MappingLedger;
use crate::mapping_normalizer;
use crate::milestoning_facts;
use crate::pipeline::Pipeline;
use crate::resolved_mapping::ResolvedMapping;
use crate::union_synthesis::{self, UnionRoute};
use crate::view_relation;

use p::ValueSpecification as V;

/// `JoinChainEmission.emitHopsForStructuralPm` (:45): pass 1 -- the hops one structural PM (Join, embedded,
/// otherwise-embedded, inline-embedded, join-terminal column) hoists into the top pipeline, in PM order.
#[allow(clippy::too_many_arguments)]
pub fn emit_hops_for_structural_pm(
    p: &mut Pipeline<'_>,
    pm: &PropertyMapping,
    owner_class_fqn: &JStr,
    main_db: &JStr,
    main_table: &JStr,
    row_bind: &Arc<p::Variable>,
    model: &ModelIndex,
    md: &ResolvedMapping<'_>,
) -> Result<(), Thrown> {
    Err(Thrown::Unported("JoinChainEmission.emitHopsForStructuralPm"))
}

/// `JoinChainEmission.emitOtherwiseEmbeddedHop` (:187): an otherwise-embedded block's fallback navigation hop.
#[allow(clippy::too_many_arguments)]
pub fn emit_otherwise_embedded_hop(
    p: &mut Pipeline<'_>,
    oe: &Arc<PropertyMappingOtherwiseEmbedded>,
    owner_class_fqn: &JStr,
    main_db: &JStr,
    main_table: &JStr,
    row_bind: &Arc<p::Variable>,
    model: &ModelIndex,
    md: &ResolvedMapping<'_>,
) -> Result<(), Thrown> {
    Err(Thrown::Unported("JoinChainEmission.emitOtherwiseEmbeddedHop"))
}

/// `JoinChainEmission.perArmHops` (:235): the hops a routed navigation emits at the top (the first only, when the
/// routes' chains are not uniform: the rest are pushed into each arm).
pub fn per_arm_hops(
    p: &Pipeline<'_>,
    prop_name: Option<&JStr>,
    target_class_fqn: Option<&JStr>,
    hops: &[Arc<JoinChainElement>],
) -> Vec<Arc<JoinChainElement>> {
    if target_class_fqn.is_none() || hops.len() < 2 {
        return hops.to_vec();
    }
    let rr = prop_name.and_then(|n| p.union_routes.get(n));
    match rr {
        Some(rr) if !union_synthesis::uniform_chained_routes(&union_synthesis::member_joins(rr)) => {
            vec![hops[0].clone()]
        }
        _ => hops.to_vec(),
    }
}

/// `JoinChainEmission.emitJoinChain` (:257, the eleven-argument form): [`emit_join_chain_routed`] with no routed set.
#[allow(clippy::too_many_arguments)]
pub fn emit_join_chain(
    p: &mut Pipeline<'_>,
    hops: &[Arc<JoinChainElement>],
    chain_db: Option<&JStr>,
    prop_name: Option<&JStr>,
    owner_class_fqn: Option<&JStr>,
    main_db: &JStr,
    main_table: &JStr,
    row_bind: &Arc<p::Variable>,
    model: &ModelIndex,
    md: Option<&ResolvedMapping<'_>>,
    class_typed_terminus: bool,
) -> Result<(), Thrown> {
    Err(Thrown::Unported("JoinChainEmission.emitJoinChain"))
}

/// `JoinChainEmission.emitJoinChain` (:276, the twelve-argument form): one PM's chain onto the pipeline -- shared
/// prefixes dedup by join path; a class-typed terminus is a `legacyNavigate` slot (or the routed union's
/// `route(...)` list), a physical one a `joinSlot`.
#[allow(clippy::too_many_arguments)]
pub fn emit_join_chain_routed(
    p: &mut Pipeline<'_>,
    hops: &[Arc<JoinChainElement>],
    chain_db: Option<&JStr>,
    prop_name: Option<&JStr>,
    owner_class_fqn: Option<&JStr>,
    main_db: &JStr,
    main_table: &JStr,
    row_bind: &Arc<p::Variable>,
    model: &ModelIndex,
    md: Option<&ResolvedMapping<'_>>,
    class_typed_terminus: bool,
    routed_set_id: Option<&JStr>,
) -> Result<(), Thrown> {
    Err(Thrown::Unported("JoinChainEmission.emitJoinChain"))
}

/// `JoinChainEmission.navSlotFor` (:470): the class-typed property's navigate slot; loud when its join was never
/// emitted.
pub fn nav_slot_for(p: &Pipeline<'_>, prop_name: &JStr) -> Result<JStr, Thrown> {
    match p.nav_slot_by_prop.get(prop_name) {
        Some(slot) => Ok(slot.clone()),
        None => Err(Thrown::not_implemented(cat!(
            "class-typed property '",
            prop_name,
            "' has no navigate slot on this pipeline \u{2014} its join was never emitted"
        ))),
    }
}

/// `JoinChainEmission.relationRef` (:481): a hop's relation -- a view's lifted body, else `tableReference(db, name)`.
pub fn relation_ref(
    db: &JStr,
    name: &JStr,
    model: &ModelIndex,
    md: &ResolvedMapping<'_>,
) -> Result<p::ValueSpecification, Thrown> {
    match model.find_view(db, name) {
        Some(view) => md.views().body(view),
        None => Ok(table_reference(db, name)),
    }
}

/// `JoinChainEmission.routeList` (:503): one `route(member(), rows, (s, t) | cond)` per union route, in route order.
#[allow(clippy::too_many_arguments)]
pub fn route_list(
    p: &Pipeline<'_>,
    routes: &[UnionRoute],
    prop_name: Option<&JStr>,
    prev_table: Option<&JStr>,
    prev_alias: Option<&JStr>,
    main_table: &JStr,
    s: &Arc<p::Variable>,
    t: &Arc<p::Variable>,
    model: &ModelIndex,
    md: &ResolvedMapping<'_>,
) -> Result<Vec<p::ValueSpecification>, Thrown> {
    Err(Thrown::Unported("JoinChainEmission.routeList"))
}

/// `JoinChainEmission.recordNavSlotOwner` (:603): a class-typed Join sub-PM's owner, first wins.
pub fn record_nav_slot_owner(
    p: &mut Pipeline<'_>,
    sub: &PropertyMapping,
    owner_cls: &JStr,
    model: &ModelIndex,
) -> Result<(), Thrown> {
    if let PropertyMapping::PropertyMappingJoin(j) = sub
        && class_typed_target(Some(owner_cls), &j.property_name, model)?.is_some()
    {
        p.nav_slot_owner
            .put_if_absent(j.property_name.clone(), owner_cls.clone());
    }
    Ok(())
}

/// `JoinChainEmission.mintNavSlotAlias` (:618): the class-typed property's navigate alias, minted once -- `<prop>_nav`
/// when it collides with a main-table column (`equalsIgnoreCase`), `columns` or the rows marker, then `_` until free.
pub fn mint_nav_slot_alias(
    p: &mut Pipeline<'_>,
    model: &ModelIndex,
    main_db: &JStr,
    main_table: &JStr,
    prop_name: &JStr,
) -> JStr {
    if let Some(known) = p.nav_slot_by_prop.get(prop_name) {
        return known.clone();
    }
    // the schema stripped (`SCHEMA.TABLE` asks the default schema's TABLE: the census's surprise, ported)
    let table_name = match lite_model::text::last_index_of(&main_table.0, &[u16::from(b'.')]) {
        Some(dot) => JStr::from(&main_table.0[dot + 1..]),
        None => main_table.clone(),
    };
    // include-aware: the main table may live in an INCLUDED database. A slot named after the platform's RELATION
    // accessors (`columns`, `rows`) would read as the accessor on the row var -- mint clear. The column test ignores
    // case: the slot and the column share one SQL row, where an unquoted `mid` and `MID` are the same name.
    let collides = model
        .knowledge()
        .table(Some(main_db), Some(&table_name))
        .is_some_and(|td| td.columns.iter().any(|cd| equals_ignore_case(&cd.name.0, &prop_name.0)))
        || prop_name.eq_str("columns")
        || prop_name.eq_str(ROWS_MARKER);
    let mut alias = if collides {
        cat!(prop_name, "_nav")
    } else {
        prop_name.clone()
    };
    // ALWAYS minted past every slot already on the pipeline (audit 2026-09-15 P0-7)
    while p.alias_to_target_table.contains_key(&alias) {
        alias = cat!(&alias, "_");
    }
    p.nav_slot_by_prop.put(prop_name.clone(), alias.clone());
    alias
}

/// `JoinChainEmission.uniqueSlotName` (:662): the path's names joined by `__`, then `__<n>` from 2 until free (a
/// `NullPointerException` for no path, as `String.join` throws).
pub fn unique_slot_name(p: &Pipeline<'_>, path: Option<&[JStr]>) -> Result<JStr, Thrown> {
    // `String.join("__", null)`: `Objects.requireNonNull(elements)`, no message
    let path = crate::error::require(path, None)?;
    let mut base: Vec<u16> = Vec::new();
    for (i, name) in path.iter().enumerate() {
        if i > 0 {
            base.extend("__".encode_utf16());
        }
        base.extend_from_slice(&name.0);
    }
    let base = JStr(base);
    if !p.alias_to_target_table.contains_key(&base) {
        return Ok(base);
    }
    let mut n: i32 = 2;
    loop {
        let candidate = cat!(&base, "__", n.to_string());
        n += 1;
        if !p.alias_to_target_table.contains_key(&candidate) {
            return Ok(candidate);
        }
    }
}

/// `JoinChainEmission.joinPath` (:675): the hops' join names.
pub fn join_path(hops: &[Arc<JoinChainElement>]) -> Vec<JStr> {
    hops.iter().map(|h| h.join_name.clone()).collect()
}

/// `JoinChainEmission.slotFor` (:689): the slot an emitted chain landed in (by its join path); loud when it was never
/// hoisted. The translator's `PipelineView::slot_for`.
pub fn slot_for(p: &Pipeline<'_>, hops: &[Arc<JoinChainElement>]) -> Result<JStr, Thrown> {
    let key = join_path(hops);
    match p.path_to_slot.get(&key) {
        Some(slot) => Ok(slot.clone()),
        None => Err(Thrown::not_implemented(cat!(
            "join chain ",
            java_list(&key),
            " was never emitted on this pipeline \u{2014} the expression",
            " navigates a join that was not hoisted as a slot"
        ))),
    }
}

/// `JoinChainEmission.classTypedTargetIfMapped` (:700): the property's class type when it (or a subclass) is mapped
/// in this mapping's closure.
pub fn class_typed_target_if_mapped(
    owner_class_fqn: Option<&JStr>,
    prop_name: &JStr,
    model: &ModelIndex,
    ledger: &MappingLedger,
) -> Result<Option<JStr>, Thrown> {
    let Some(owner) = model.knowledge().hierarchy_class(owner_class_fqn) else {
        return Ok(None);
    };
    let prop_type = model.knowledge().property_type(Some(owner), prop_name)?;
    // a parameterized declaration (`Class<Any>`, `Property<Nil,Any|*>`, the real m3 ends) targets its RAW class
    let Some(tgt) = prop_type.as_ref().and_then(p::TypeExpression::raw_class_name) else {
        return Ok(None);
    };
    // the declared class itself, or an ABSTRACTION of mapped classes: either way a navigation, never a column read
    Ok((ledger.is_mapped(tgt) || has_mapped_subclass(tgt, model, ledger)).then(|| tgt.clone()))
}

/// `JoinChainEmission.classTypedTarget` (:722): the property's class type, when it is a class the model has.
pub fn class_typed_target(
    owner_class_fqn: Option<&JStr>,
    prop_name: &JStr,
    model: &ModelIndex,
) -> Result<Option<JStr>, Thrown> {
    let Some(owner) = model.knowledge().hierarchy_class(owner_class_fqn) else {
        return Ok(None);
    };
    let prop_type = model.knowledge().property_type(Some(owner), prop_name)?;
    let tgt = prop_type.as_ref().and_then(p::TypeExpression::raw_class_name);
    Ok(tgt
        .filter(|t| model.knowledge().hierarchy_class(Some(t)).is_some())
        .cloned())
}

/// `JoinChainEmission.hasMappedSubclass` (:732): whether a class of `base`'s subtree is mapped here (a shadow walker
/// `ShadowWalkerCensusTest` pinned; the census retires with the Java).
pub fn has_mapped_subclass(base: &JStr, model: &ModelIndex, ledger: &MappingLedger) -> bool {
    // the graph's classes strictly below base (the catalog's are not the graph's), any of them mapped in this
    // mapping's closure
    model
        .knowledge()
        .subtree(base)
        .iter()
        .any(|c| model.find_class(c).is_some() && ledger.is_mapped(c))
}

/// `JoinChainEmission.JoinNavSpec` (:740): a join navigation an expression makes, with its chain's database.
#[derive(Clone, Debug, PartialEq)]
pub struct JoinNavSpec {
    pub chain: Arc<[Arc<JoinChainElement>]>,
    pub chain_db: Option<JStr>,
}

/// `JoinChainEmission.collectJoinNavigationsInPms` (:742, the two-argument form): the PMs' join navigations with no
/// mapping in hand (an Inline splice contributes nothing).
pub fn collect_join_navigations_in_pms(pms: &[PropertyMapping], out: &mut Vec<JoinNavSpec>) {
    collect_join_navigations_in_pms_of(pms, out, None);
}

/// `JoinChainEmission.collectJoinNavigationsInPms` (:750, the three-argument form): the PMs' join navigations in PM
/// order (= pass 2's emission order = the slot names); an Inline splice carries its referenced set's
/// (`ResolvedMapping::first_relational`).
pub fn collect_join_navigations_in_pms_of(
    pms: &[PropertyMapping],
    out: &mut Vec<JoinNavSpec>,
    md: Option<&ResolvedMapping<'_>>,
) {
    for pm in pms {
        match pm {
            PropertyMapping::PropertyMappingEnumeratedExpression(ee) => collect_join_navigations(&ee.expression, out),
            PropertyMapping::PropertyMappingExpression(expr) => collect_join_navigations(&expr.expression, out),
            PropertyMapping::PropertyMappingLocalProperty(lp) => {
                collect_join_navigations_in_pms_of(std::slice::from_ref(&lp.body), out, md)
            }
            PropertyMapping::PropertyMappingEmbedded(emb) => {
                collect_join_navigations_in_pms_of(&emb.property_mappings, out, md)
            }
            PropertyMapping::PropertyMappingOtherwiseEmbedded(oe) => {
                collect_join_navigations_in_pms_of(&oe.embedded, out, md)
            }
            PropertyMapping::PropertyMappingJoinTerminalColumn(jtc) => {
                collect_join_navigations(&jtc.terminal_column, out)
            }
            // an Inline splice CARRIES the referenced set's JoinNavs, resolvable only with the enclosing mapping in hand
            PropertyMapping::PropertyMappingInlineEmbedded(ie) => {
                if let Some(md) = md
                    && let Some(r2) = md.first_relational(&ie.set_id)
                {
                    collect_join_navigations_in_pms_of(&r2.property_mappings, out, Some(md));
                }
            }
            // Join / Column / EnumeratedColumn: Join handled by Pass 1; the others don't carry JoinNav.
            PropertyMapping::PropertyMappingJoin(_) => {}
            PropertyMapping::PropertyMappingColumn(_) => {}
            PropertyMapping::PropertyMappingEnumeratedColumn(_) => {}
        }
    }
}

/// `JoinChainEmission.collectJoinNavigations` (:789): one expression's join navigations, in reading order.
pub fn collect_join_navigations(op: &RelationalOperation, out: &mut Vec<JoinNavSpec>) {
    match op {
        RelationalOperation::RelationalOperationJoinNavigation(jn) => {
            out.push(JoinNavSpec {
                chain: jn.chain.clone(),
                chain_db: jn.database_name.clone(),
            });
            if let Some(terminal) = &jn.terminal {
                collect_join_navigations(terminal, out);
            }
        }
        RelationalOperation::RelationalOperationFunctionCall(fc) => {
            for a in fc.args.iter() {
                collect_join_navigations(a, out);
            }
        }
        RelationalOperation::RelationalOperationComparison(cmp) => {
            collect_join_navigations(&cmp.left, out);
            collect_join_navigations(&cmp.right, out);
        }
        RelationalOperation::RelationalOperationBooleanOp(bo) => {
            collect_join_navigations(&bo.left, out);
            collect_join_navigations(&bo.right, out);
        }
        RelationalOperation::RelationalOperationIsNull(n) => collect_join_navigations(&n.operand, out),
        RelationalOperation::RelationalOperationIsNotNull(n) => collect_join_navigations(&n.operand, out),
        RelationalOperation::RelationalOperationGroup(g) => collect_join_navigations(&g.inner, out),
        RelationalOperation::RelationalOperationArrayLiteral(a) => {
            for e in a.elements.iter() {
                collect_join_navigations(e, out);
            }
        }
        RelationalOperation::RelationalOperationLambda(lam) => collect_join_navigations(&lam.body, out),
        RelationalOperation::RelationalOperationLambdaParam(_) => {}
        RelationalOperation::RelationalOperationColumnRef(_) => {}
        RelationalOperation::RelationalOperationTargetColumnRef(_) => {}
        RelationalOperation::RelationalOperationLiteral(_) => {}
    }
}

/// `JoinChainEmission.HopTarget` (:822): a hop's (view-substituted) condition, its target table, and the view it
/// lands on.
#[derive(Clone, Debug, PartialEq)]
pub struct HopTarget {
    pub cond: RelationalOperation,
    pub table: JStr,
    pub view: Option<JStr>,
}

/// `JoinChainEmission.hopTarget` (:826): pass 2's target of one hop.
#[allow(clippy::too_many_arguments)]
pub fn hop_target(
    join_cond: &RelationalOperation,
    view_target: Option<&JStr>,
    prev_table: Option<&JStr>,
    hop_db: &JStr,
    join_name: &JStr,
    prop_name: Option<&JStr>,
    i: i32,
    p: &Pipeline<'_>,
    model: &ModelIndex,
    md: Option<&ResolvedMapping<'_>>,
) -> Result<HopTarget, Thrown> {
    if let Some(view_target) = view_target {
        return Ok(HopTarget {
            cond: join_cond.clone(),
            table: view_target.clone(),
            view: Some(view_target.clone()),
        });
    }
    // PASS 2: source-side view projections substitute (plain views only); target-side view refs stay VERBATIM, so a
    // view landing is recognizable here too
    let join_cond =
        mapping_normalizer::resolve_view_refs_in_join(join_cond, hop_db, prev_table, model, md, None, None)?;
    // Java passes `ViewRelation.owner(md, hopDb)` (`mapping=<fqn>`) where the callee prints `mapping=` + its
    // argument: the text reads `mapping=mapping=<fqn>` (a Java quirk, ported)
    let nested = JStr::from("<nested>");
    let target_table = mapping_normalizer::determine_target_table(
        &join_cond,
        prev_table,
        join_name,
        Some(prop_name.unwrap_or(&nested)),
        i + 1,
        &view_relation::owner(md, hop_db),
    )?;
    let view = model
        .find_view(hop_db, &target_table)
        .is_some()
        .then(|| target_table.clone());
    Ok(HopTarget {
        cond: join_cond,
        table: target_table,
        view,
    })
}

/// `JoinChainEmission.plainClassViewCond` (:856): a class-typed navigate onto a PLAIN view of the target class's own
/// main table -- the condition substituted to physical; none keeps the expansion (the module's note on JCE:884).
pub fn plain_class_view_cond(
    join_cond: &RelationalOperation,
    view_target: &JStr,
    target_class_fqn: Option<&JStr>,
    hop_db: &JStr,
    model: &ModelIndex,
    md: &ResolvedMapping<'_>,
) -> Result<Option<RelationalOperation>, Thrown> {
    let Some(target_class_fqn) = target_class_fqn else {
        return Ok(None);
    };
    let vd = model
        .find_view(hop_db, view_target)
        .ok_or_else(Thrown::no_such_element)?;
    if vd.filter.is_some() || !vd.group_by_columns.is_empty() || vd.distinct {
        return Ok(None);
    }
    // JOIN-NAVIGATING columns disqualify too: rename-only means every column is a plain ColumnRef
    for vc in vd.column_mappings.iter() {
        let mut navs = Vec::new();
        collect_join_navigations(&vc.expression, &mut navs);
        if !navs.is_empty() {
            return Ok(None);
        }
    }
    let v_phys = model.view_main_table(hop_db, vd)?;
    // JCE:882-886's `catch (ModelException unmappedTarget) { return null; }` around `mainTableOf`, RE-EXPRESSED: its
    // one cause is `mainTableDefOf`'s throw when `mainTableOrNull` is null
    let Some(tgt_main) = mapping_normalizer::main_table_or_null(md, Some(target_class_fqn))? else {
        return Ok(None);
    };
    // the condition speaks the TARGET CLASS's row: a class over the view's PHYSICAL table reads physical columns
    // (substitute); a class over the VIEW itself is its frame -- keep the expansion
    if v_phys != tgt_main.table {
        return Ok(None);
    }
    // SINGLE-SET, group-free, NON-TEMPORAL targets only
    let sets = md.relational_sets(Some(target_class_fqn));
    if sets.len() != 1
        || !sets[0].group_by.is_empty()
        || sets[0].distinct
        || milestoning_facts::is_temporal(target_class_fqn, model)?
    {
        return Ok(None);
    }
    Ok(Some(mapping_normalizer::resolve_view_refs_in_join(
        join_cond,
        hop_db,
        Some(&v_phys),
        model,
        Some(md),
        Some(view_target),
        None,
    )?))
}

/// `JoinChainEmission.innerFilteredSource` (:921): an INNER join-mediated mapping filter's source,
/// `project(filter(...))` (the engine's `getRelationalElementWithInnerJoin`), on a pipeline of its own.
pub fn inner_filtered_source(
    rcm: &Arc<ClassMappingRelational>,
    jm: &Arc<FilterMappingJoinMediated>,
    model: &ModelIndex,
    md: &ResolvedMapping<'_>,
    ledger: &mut MappingLedger,
) -> Result<p::ValueSpecification, Thrown> {
    Err(Thrown::Unported("JoinChainEmission.innerFilteredSource"))
}

/// `JoinChainEmission.nullTolerant` (:1015): whether a filter condition can be true over a null-extended row.
pub fn null_tolerant(op: &RelationalOperation) -> Result<bool, Thrown> {
    Ok(match op {
        RelationalOperation::RelationalOperationIsNull(_) => true,
        RelationalOperation::RelationalOperationFunctionCall(fc) => {
            // IDENTITY THROUGH THE REGISTRY (audit 2026-09-15 P2-4); the registry's names are ASCII, so a lone
            // surrogate's U+FFFD matches none, as the unit-exact name would not
            let dyna = DynaFn::of(&fc.name.lossy());
            if matches!(dyna, Some(DynaFn::IS_NULL | DynaFn::SQL_NULL)) {
                return Ok(true);
            }
            // audit 23: NULL-swallowing functions defeat the classification -- LOUD, never a silent verdict
            if matches!(dyna, Some(DynaFn::COALESCE | DynaFn::CASE | DynaFn::IF)) {
                return Err(Thrown::not_implemented(cat!(
                    "(INNER) mapping-",
                    "filter condition uses '",
                    &fc.name,
                    "' \u{2014} null-tolerance cannot be classified;",
                    " not supported yet"
                )));
            }
            for a in fc.args.iter() {
                if null_tolerant(a)? {
                    return Ok(true);
                }
            }
            false
        }
        RelationalOperation::RelationalOperationBooleanOp(b) => null_tolerant(&b.left)? || null_tolerant(&b.right)?,
        RelationalOperation::RelationalOperationComparison(c) => null_tolerant(&c.left)? || null_tolerant(&c.right)?,
        RelationalOperation::RelationalOperationGroup(g) => null_tolerant(&g.inner)?,
        // Java's `default -> false` (:1049; ASK 12, ported as Java has it): every other operation is classed
        // null-REJECTING, an IsNotNull, an ArrayLiteral, a JoinNavigation and a Lambda included
        RelationalOperation::RelationalOperationColumnRef(_)
        | RelationalOperation::RelationalOperationTargetColumnRef(_)
        | RelationalOperation::RelationalOperationLiteral(_)
        | RelationalOperation::RelationalOperationIsNotNull(_)
        | RelationalOperation::RelationalOperationArrayLiteral(_)
        | RelationalOperation::RelationalOperationJoinNavigation(_)
        | RelationalOperation::RelationalOperationLambda(_)
        | RelationalOperation::RelationalOperationLambdaParam(_) => false,
    })
}

/// `JoinChainEmission.targetColumnNames` (:1063): a hop target's column names (a view's declared columns, a table's
/// columns), in declaration order; loud for an unknown table.
#[allow(clippy::too_many_arguments)]
pub fn target_column_names(
    db_fqn: &JStr,
    target_table: &JStr,
    view_target: Option<&JStr>,
    model: &ModelIndex,
    join_name: &JStr,
    prop_name: Option<&JStr>,
    md: Option<&ResolvedMapping<'_>>,
) -> Result<LinkedSet<JStr>, Thrown> {
    let mut out = LinkedSet::new();
    if let Some(view_target) = view_target {
        if let Some(v) = model.find_view(db_fqn, view_target) {
            for c in v.column_mappings.iter() {
                out.add(c.name.clone());
            }
        }
        return Ok(out);
    }
    let Some(t) = model.knowledge().table(Some(db_fqn), Some(target_table)) else {
        return Err(Thrown::model(
            Phase::Normalize,
            cat!(
                "Join '",
                join_name,
                "' target table '",
                target_table,
                "' not found in db '",
                db_fqn,
                "'; PM='",
                value_of(prop_name),
                "', ",
                view_relation::owner(md, db_fqn)
            ),
            None,
        ));
    };
    for c in t.columns.iter() {
        out.add(c.name.clone());
    }
    Ok(out)
}

// ---- the protocol records' convenience constructors Java's emission calls, and two JDK spellings ----

/// `new Variable(name)`.
pub(crate) fn variable(name: &str) -> Arc<p::Variable> {
    Arc::new(p::Variable {
        name: JStr::from(name),
        r#type: None,
        multiplicity: None,
        pos: None,
    })
}

/// `new LambdaFunction(parameters, body)` (no position).
pub(crate) fn lambda(parameters: Vec<Arc<p::Variable>>, body: Vec<V>) -> Arc<p::LambdaFunction> {
    Arc::new(p::LambdaFunction {
        parameters: parameters.into(),
        body: body.into(),
        pos: None,
    })
}

/// `new AppliedProperty(receiver, property)` (no position, no owner class).
pub(crate) fn property(receiver: V, name: &JStr) -> V {
    V::AppliedProperty(Arc::new(p::AppliedProperty {
        receiver,
        property: name.clone(),
        pos: None,
        owner_class: None,
    }))
}

/// `new ColSpec(name, function1, function2[, alias])`: no arguments, unqualified, no position, type or annotations.
pub(crate) fn col_spec(
    name: &JStr,
    function1: Option<Arc<p::LambdaFunction>>,
    function2: Option<Arc<p::LambdaFunction>>,
    alias: Option<&JStr>,
) -> Arc<p::ColSpec> {
    Arc::new(p::ColSpec {
        name: name.clone(),
        function1,
        function2,
        alias: alias.cloned(),
        args: Arc::default(),
        qualified: false,
        pos: None,
        col_type: None,
        col_type_mult: None,
        stereotypes: Arc::default(),
        tagged_values: Arc::default(),
    })
}

/// A column spec as a value.
pub(crate) fn col(cs: Arc<p::ColSpec>) -> V {
    V::ColumnInstance(p::ColumnInstance::ColSpec(cs))
}

/// `new ColSpecArray(colSpecs)` (no position).
pub(crate) fn col_spec_array(col_specs: Vec<Arc<p::ColSpec>>) -> V {
    V::ColumnInstance(p::ColumnInstance::ColSpecArray(Arc::new(p::ColSpecArray {
        col_specs: col_specs.into(),
        pos: None,
    })))
}

/// A built call as a value.
pub(crate) fn call(af: p::AppliedFunction) -> V {
    V::AppliedFunction(Arc::new(af))
}

/// `Calls.platform("tableReference", [new PackageableElementPtr(db), new CString(table)])`.
fn table_reference(db: &JStr, table: &JStr) -> V {
    let ptr = V::PackageableElementPtr(Arc::new(p::PackageableElementPtr {
        full_path: db.clone(),
        pos: None,
    }));
    let name = V::CString(Arc::new(p::CString {
        value: table.clone(),
        pos: None,
        multi_line: false,
    }));
    call(calls::platform(&JStr::from("tableReference"), vec![ptr, name]))
}

/// `String.valueOf(s)` of a nullable string: `"null"` for none.
fn value_of(s: Option<&JStr>) -> JStr {
    s.cloned().unwrap_or_else(|| JStr::from("null"))
}

/// A property mapping's Java class's `getSimpleName()` (JCE:195, GBS:164): the nested record's name.
pub(crate) fn pm_simple_name(pm: &PropertyMapping) -> &'static str {
    match pm {
        PropertyMapping::PropertyMappingEnumeratedExpression(_) => "EnumeratedExpression",
        PropertyMapping::PropertyMappingColumn(_) => "Column",
        PropertyMapping::PropertyMappingEnumeratedColumn(_) => "EnumeratedColumn",
        PropertyMapping::PropertyMappingJoin(_) => "Join",
        PropertyMapping::PropertyMappingJoinTerminalColumn(_) => "JoinTerminalColumn",
        PropertyMapping::PropertyMappingExpression(_) => "Expression",
        PropertyMapping::PropertyMappingEmbedded(_) => "Embedded",
        PropertyMapping::PropertyMappingInlineEmbedded(_) => "InlineEmbedded",
        PropertyMapping::PropertyMappingOtherwiseEmbedded(_) => "OtherwiseEmbedded",
        PropertyMapping::PropertyMappingLocalProperty(_) => "LocalProperty",
    }
}

#[cfg(test)]
mod tests {
    use std::collections::{HashMap, HashSet};
    use std::rc::Rc;

    use lite_model::linked::LinkedMap;
    use lite_model::text::j;
    use lite_model::{
        LegacyMappingDefinition, PropertyMappingInlineEmbedded, PropertyMappingJoin, RelationalOperationColumnRef,
        RelationalOperationComparison, RelationalOperationFunctionCall, RelationalOperationIsNull,
        RelationalOperationJoinNavigation, RelationalOperationLiteral,
    };

    use super::*;
    use crate::lifted_views::LiftedViews;
    use crate::mapping_closures::MappingClosures;
    use crate::test_support::{index, mapping, model};

    const MODEL: &str = r#"
###Pure
Class a::Firm
{
  name: String[1];
}
Class a::Sub extends a::Firm
{
}
Class a::Person
{
  name: String[1];
  firm: a::Firm[0..1];
  firms: a::Firm[*];
  rows: a::Firm[*];
  firmName: String[0..1];
}

###Relational
Database a::db
(
  Table PERSON (ID INTEGER PRIMARY KEY, NAME VARCHAR(200), FIRM_ID INTEGER, FIRM VARCHAR(20))
  Table FIRM (ID INTEGER PRIMARY KEY, NAME VARCHAR(200))
  View FV (FID: FIRM.ID, FNAME: FIRM.NAME)
  Join PersonFirm(PERSON.FIRM_ID = FIRM.ID)
)

###Mapping
Mapping a::M
(
  a::Firm[f]: Relational
  {
    ~mainTable [a::db]FIRM
    name: [a::db]FIRM.NAME
  }
  a::Person[p]: Relational
  {
    ~mainTable [a::db]PERSON
    name: [a::db]PERSON.NAME,
    firm[f]: [a::db]@PersonFirm,
    firmName: concat([a::db]@PersonFirm | FIRM.NAME, 'x')
  }
)
"#;

    fn resolved<'a>(
        md: Arc<LegacyMappingDefinition>,
        closures: &'a MappingClosures<'a>,
        views: &'a LiftedViews<'a>,
    ) -> ResolvedMapping<'a> {
        let surface = closures.surface(&md);
        let closure = closures.closure(&surface.qualified_name);
        ResolvedMapping::new(
            surface.clone(),
            surface,
            Rc::new(HashMap::new()),
            Rc::new(LinkedMap::new()),
            closure,
            views,
        )
    }

    fn text(s: &str) -> V {
        V::CString(Arc::new(p::CString {
            value: j(s),
            pos: None,
            multi_line: false,
        }))
    }

    fn hop(name: &str) -> Arc<JoinChainElement> {
        Arc::new(JoinChainElement {
            join_name: j(name),
            join_type: None,
            database_name: None,
            include_self: false,
        })
    }

    fn column(table: &str, col: &str) -> RelationalOperation {
        RelationalOperation::RelationalOperationColumnRef(Arc::new(RelationalOperationColumnRef {
            database_name: None,
            table: j(table),
            column: j(col),
        }))
    }

    fn fcall(name: &str, args: Vec<RelationalOperation>) -> RelationalOperation {
        RelationalOperation::RelationalOperationFunctionCall(Arc::new(RelationalOperationFunctionCall {
            name: j(name),
            args: args.into(),
        }))
    }

    fn nav(chain: &[&str], db: Option<&str>, terminal: Option<RelationalOperation>) -> RelationalOperation {
        RelationalOperation::RelationalOperationJoinNavigation(Arc::new(RelationalOperationJoinNavigation {
            database_name: db.map(j),
            chain: chain.iter().map(|h| hop(h)).collect(),
            terminal,
        }))
    }

    fn chain_names(specs: &[JoinNavSpec]) -> Vec<Vec<JStr>> {
        specs.iter().map(|s| join_path(&s.chain)).collect()
    }

    #[test]
    fn a_nav_slot_is_minted_past_columns_accessors_and_taken_slots_once() {
        let parsed = model(MODEL);
        let idx = index(&parsed);
        let views = LiftedViews::new(&parsed, &idx);
        let mut ledger = MappingLedger::new(HashSet::new());
        let mut p = Pipeline::new(text("src"), &mut ledger, &views);
        let (db, person) = (j("a::db"), j("PERSON"));
        // PERSON has a FIRM column: the fold ignores case
        assert_eq!(
            mint_nav_slot_alias(&mut p, &idx, &db, &person, &j("firm")),
            j("firm_nav")
        );
        // first mint wins
        assert_eq!(
            mint_nav_slot_alias(&mut p, &idx, &db, &person, &j("firm")),
            j("firm_nav")
        );
        // the relation accessors
        assert_eq!(
            mint_nav_slot_alias(&mut p, &idx, &db, &person, &j("columns")),
            j("columns_nav")
        );
        assert_eq!(
            mint_nav_slot_alias(&mut p, &idx, &db, &person, &j("rows")),
            j("rows_nav")
        );
        // a slot already on the pipeline (a physical chain's join named like the property) pushes it on
        p.alias_to_target_table.put(j("firms"), j("FIRM"));
        p.alias_to_target_table.put(j("firms_"), j("FIRM"));
        assert_eq!(
            mint_nav_slot_alias(&mut p, &idx, &db, &person, &j("firms")),
            j("firms__")
        );
        // a schema-qualified main table asks its default-schema namesake
        assert_eq!(
            mint_nav_slot_alias(&mut p, &idx, &db, &j("S.PERSON"), &j("name")),
            j("name_nav")
        );
        assert_eq!(
            mint_nav_slot_alias(&mut p, &idx, &db, &j("FIRM"), &j("other")),
            j("other")
        );
        assert_eq!(p.nav_slot_by_prop.get(&j("firms")), Some(&j("firms__")));
        assert_eq!(nav_slot_for(&p, &j("firm")).unwrap(), j("firm_nav"));
        let miss = nav_slot_for(&p, &j("nope")).unwrap_err();
        assert!(miss.is_not_implemented());
        assert_eq!(
            miss.message(),
            Some(j(
                "class-typed property 'nope' has no navigate slot on this pipeline \u{2014} its join was never emitted"
            ))
        );
    }

    #[test]
    fn a_physical_slot_is_its_path_then_numbered_and_found_by_its_path() {
        let parsed = model(MODEL);
        let idx = index(&parsed);
        let views = LiftedViews::new(&parsed, &idx);
        let mut ledger = MappingLedger::new(HashSet::new());
        let mut p = Pipeline::new(text("src"), &mut ledger, &views);
        let path = [j("A"), j("B")];
        assert_eq!(unique_slot_name(&p, Some(&path)).unwrap(), j("A__B"));
        p.alias_to_target_table.put(j("A__B"), j("T"));
        assert_eq!(unique_slot_name(&p, Some(&path)).unwrap(), j("A__B__2"));
        p.alias_to_target_table.put(j("A__B__2"), j("T"));
        assert_eq!(unique_slot_name(&p, Some(&path)).unwrap(), j("A__B__3"));
        assert_eq!(unique_slot_name(&p, None).unwrap_err(), Thrown::null_pointer(None));
        let hops = vec![hop("A"), hop("B")];
        assert_eq!(join_path(&hops), path.to_vec());
        p.path_to_slot.put(path.to_vec(), j("A__B__2"));
        assert_eq!(slot_for(&p, &hops).unwrap(), j("A__B__2"));
        let miss = slot_for(&p, &hops[..1]).unwrap_err();
        assert!(miss.is_not_implemented());
        assert_eq!(
            miss.message(),
            Some(j(
                "join chain [A] was never emitted on this pipeline \u{2014} the expression navigates a join that was \
                 not hoisted as a slot"
            ))
        );
        // routes are asked only for a class-typed chain of two hops or more
        assert_eq!(per_arm_hops(&p, Some(&j("firm")), None, &hops).len(), 2);
        assert_eq!(
            per_arm_hops(&p, Some(&j("firm")), Some(&j("a::Firm")), &hops[..1]).len(),
            1
        );
        assert_eq!(per_arm_hops(&p, Some(&j("firm")), Some(&j("a::Firm")), &hops).len(), 2);
        assert_eq!(per_arm_hops(&p, None, Some(&j("a::Firm")), &hops).len(), 2);
    }

    #[test]
    fn the_class_typed_targets_read_the_declared_type_and_the_ledger() {
        let parsed = model(MODEL);
        let idx = index(&parsed);
        let person = j("a::Person");
        assert_eq!(
            class_typed_target(Some(&person), &j("firm"), &idx).unwrap(),
            Some(j("a::Firm"))
        );
        assert_eq!(class_typed_target(Some(&person), &j("name"), &idx).unwrap(), None);
        assert_eq!(class_typed_target(Some(&person), &j("nope"), &idx).unwrap(), None);
        assert_eq!(class_typed_target(Some(&j("a::Nope")), &j("firm"), &idx).unwrap(), None);
        assert_eq!(class_typed_target(None, &j("firm"), &idx).unwrap(), None);
        let firm_mapped = MappingLedger::new(HashSet::from([j("a::Firm")]));
        let sub_mapped = MappingLedger::new(HashSet::from([j("a::Sub")]));
        let none_mapped = MappingLedger::new(HashSet::new());
        let target = |l: &MappingLedger| class_typed_target_if_mapped(Some(&person), &j("firm"), &idx, l).unwrap();
        assert_eq!(target(&firm_mapped), Some(j("a::Firm")));
        // an abstraction of a mapped class: the declared class, a navigation
        assert_eq!(target(&sub_mapped), Some(j("a::Firm")));
        assert_eq!(target(&none_mapped), None);
        assert!(has_mapped_subclass(&j("a::Firm"), &idx, &sub_mapped));
        assert!(!has_mapped_subclass(&j("a::Firm"), &idx, &firm_mapped));
        assert_eq!(
            class_typed_target_if_mapped(Some(&person), &j("name"), &idx, &firm_mapped).unwrap(),
            None
        );
    }

    #[test]
    fn a_nav_slot_owner_is_recorded_for_a_class_typed_join_first_wins() {
        let parsed = model(MODEL);
        let idx = index(&parsed);
        let views = LiftedViews::new(&parsed, &idx);
        let mut ledger = MappingLedger::new(HashSet::new());
        let mut p = Pipeline::new(text("src"), &mut ledger, &views);
        let join = |prop: &str| {
            PropertyMapping::PropertyMappingJoin(Arc::new(PropertyMappingJoin {
                property_name: j(prop),
                database: j("a::db"),
                joins: vec![hop("PersonFirm")].into(),
                target_set_id: None,
            }))
        };
        record_nav_slot_owner(&mut p, &join("firm"), &j("a::Person"), &idx).unwrap();
        record_nav_slot_owner(&mut p, &join("firm"), &j("a::Other"), &idx).unwrap();
        record_nav_slot_owner(&mut p, &join("name"), &j("a::Person"), &idx).unwrap();
        assert_eq!(p.nav_slot_owner.get(&j("firm")), Some(&j("a::Person")));
        assert_eq!(p.nav_slot_owner.len(), 1);
    }

    #[test]
    fn join_navigations_are_collected_in_reading_order_through_pms_and_inline_splices() {
        let parsed = model(MODEL);
        let idx = index(&parsed);
        let closures = MappingClosures::new(&idx);
        let views = LiftedViews::new(&parsed, &idx);
        let md = resolved(mapping(&parsed, "a::M"), &closures, &views);
        let cmp = RelationalOperation::RelationalOperationComparison(Arc::new(RelationalOperationComparison {
            left: nav(&["A"], Some("a::db"), Some(nav(&["B", "C"], None, None))),
            op: lite_model::ComparisonOp::EQ,
            right: fcall(
                "concat",
                vec![column("T", "X"), nav(&["D"], None, Some(column("T", "Y")))],
            ),
        }));
        let mut out = Vec::new();
        collect_join_navigations(&cmp, &mut out);
        assert_eq!(
            chain_names(&out),
            vec![vec![j("A")], vec![j("B"), j("C")], vec![j("D")]]
        );
        assert_eq!(out[0].chain_db, Some(j("a::db")));
        assert_eq!(out[1].chain_db, None);
        // an Inline splice carries its referenced set's navigations only with the mapping in hand
        let inline = PropertyMapping::PropertyMappingInlineEmbedded(Arc::new(PropertyMappingInlineEmbedded {
            property_name: j("x"),
            set_id: j("p"),
        }));
        let expr = PropertyMapping::PropertyMappingExpression(Arc::new(lite_model::PropertyMappingExpression {
            property_name: j("y"),
            expression: nav(&["E"], None, None),
        }));
        let pms = vec![inline, expr];
        let mut bare = Vec::new();
        collect_join_navigations_in_pms(&pms, &mut bare);
        assert_eq!(chain_names(&bare), vec![vec![j("E")]]);
        let mut spliced = Vec::new();
        collect_join_navigations_in_pms_of(&pms, &mut spliced, Some(&md));
        // the referenced set `p`: its Join PM is pass 1's; its expression's navigation splices in, before `E`
        assert_eq!(chain_names(&spliced), vec![vec![j("PersonFirm")], vec![j("E")]]);
    }

    #[test]
    fn null_tolerance_is_classified_as_java_does() {
        let is_null = RelationalOperation::RelationalOperationIsNull(Arc::new(RelationalOperationIsNull {
            operand: column("T", "X"),
        }));
        let literal = RelationalOperation::RelationalOperationLiteral(Arc::new(RelationalOperationLiteral {
            value: lite_model::JValue::Long(1),
        }));
        let eq = |l: RelationalOperation, r: RelationalOperation| {
            RelationalOperation::RelationalOperationComparison(Arc::new(RelationalOperationComparison {
                left: l,
                op: lite_model::ComparisonOp::EQ,
                right: r,
            }))
        };
        assert!(null_tolerant(&is_null).unwrap());
        assert!(!null_tolerant(&eq(column("T", "X"), literal.clone())).unwrap());
        assert!(null_tolerant(&eq(column("T", "X"), is_null.clone())).unwrap());
        assert!(null_tolerant(&fcall("isNull", vec![column("T", "X")])).unwrap());
        assert!(null_tolerant(&fcall("sqlNull", vec![])).unwrap());
        assert!(!null_tolerant(&fcall("upper", vec![column("T", "X")])).unwrap());
        // the arguments are read left to right and the first tolerant one answers: a later coalesce is never seen
        assert!(null_tolerant(&fcall("concat", vec![is_null.clone(), fcall("coalesce", vec![])])).unwrap());
        let e = null_tolerant(&fcall("concat", vec![literal.clone(), fcall("coalesce", vec![])])).unwrap_err();
        assert!(e.is_not_implemented());
        assert_eq!(
            e.message(),
            Some(j(
                "(INNER) mapping-filter condition uses 'coalesce' \u{2014} null-tolerance cannot be classified; not \
                 supported yet"
            ))
        );
        for name in ["case", "if"] {
            assert!(null_tolerant(&fcall(name, vec![])).unwrap_err().is_not_implemented());
        }
        // `default -> false`: an IsNotNull and a navigation are classed null-rejecting
        let is_not_null =
            RelationalOperation::RelationalOperationIsNotNull(Arc::new(lite_model::RelationalOperationIsNotNull {
                operand: is_null.clone(),
            }));
        assert!(!null_tolerant(&is_not_null).unwrap());
        assert!(!null_tolerant(&nav(&["A"], None, Some(is_null))).unwrap());
    }

    #[test]
    fn a_hop_targets_columns_are_the_tables_or_the_views_in_declaration_order() {
        let parsed = model(MODEL);
        let idx = index(&parsed);
        let db = j("a::db");
        let names = |t: &str, v: Option<&str>| {
            target_column_names(&db, &j(t), v.map(j).as_ref(), &idx, &j("J"), None, None)
                .unwrap()
                .to_vec()
        };
        assert_eq!(names("PERSON", None), vec![j("ID"), j("NAME"), j("FIRM_ID"), j("FIRM")]);
        assert_eq!(names("FV", Some("FV")), vec![j("FID"), j("FNAME")]);
        // a view target the database has not: no columns, no refusal
        assert!(names("NOPE", Some("NOPE")).is_empty());
    }
}
