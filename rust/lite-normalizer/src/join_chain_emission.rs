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

use lite_front::records as p;
use lite_model::{
    ClassMappingRelational, FilterMappingJoinMediated, JStr, JoinChainElement, PropertyMapping,
    PropertyMappingOtherwiseEmbedded, RelationalOperation,
};
use lite_names::ModelIndex;

use crate::error::Thrown;
use crate::jdk::LinkedSet;
use crate::mapping_ledger::MappingLedger;
use crate::pipeline::Pipeline;
use crate::resolved_mapping::ResolvedMapping;
use crate::union_synthesis::UnionRoute;
use crate::unported;

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
    // UNPORTED: JoinChainEmission.perArmHops
    unported!("JoinChainEmission.perArmHops")
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
    Err(Thrown::Unported("JoinChainEmission.navSlotFor"))
}

/// `JoinChainEmission.relationRef` (:481): a hop's relation -- a view's lifted body, else `tableReference(db, name)`.
pub fn relation_ref(
    db: &JStr,
    name: &JStr,
    model: &ModelIndex,
    md: &ResolvedMapping<'_>,
) -> Result<p::ValueSpecification, Thrown> {
    Err(Thrown::Unported("JoinChainEmission.relationRef"))
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
    Err(Thrown::Unported("JoinChainEmission.recordNavSlotOwner"))
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
    // UNPORTED: JoinChainEmission.mintNavSlotAlias
    unported!("JoinChainEmission.mintNavSlotAlias")
}

/// `JoinChainEmission.uniqueSlotName` (:662): the path's names joined by `__`, then `__<n>` from 2 until free (a
/// `NullPointerException` for no path, as `String.join` throws).
pub fn unique_slot_name(p: &Pipeline<'_>, path: Option<&[JStr]>) -> Result<JStr, Thrown> {
    Err(Thrown::Unported("JoinChainEmission.uniqueSlotName"))
}

/// `JoinChainEmission.joinPath` (:675): the hops' join names.
pub fn join_path(hops: &[Arc<JoinChainElement>]) -> Vec<JStr> {
    // UNPORTED: JoinChainEmission.joinPath
    unported!("JoinChainEmission.joinPath")
}

/// `JoinChainEmission.slotFor` (:689): the slot an emitted chain landed in (by its join path); loud when it was never
/// hoisted. The translator's `PipelineView::slot_for`.
pub fn slot_for(p: &Pipeline<'_>, hops: &[Arc<JoinChainElement>]) -> Result<JStr, Thrown> {
    Err(Thrown::Unported("JoinChainEmission.slotFor"))
}

/// `JoinChainEmission.classTypedTargetIfMapped` (:700): the property's class type when it (or a subclass) is mapped
/// in this mapping's closure.
pub fn class_typed_target_if_mapped(
    owner_class_fqn: Option<&JStr>,
    prop_name: &JStr,
    model: &ModelIndex,
    ledger: &MappingLedger,
) -> Result<Option<JStr>, Thrown> {
    Err(Thrown::Unported("JoinChainEmission.classTypedTargetIfMapped"))
}

/// `JoinChainEmission.classTypedTarget` (:722): the property's class type, when it is a class the model has.
pub fn class_typed_target(
    owner_class_fqn: Option<&JStr>,
    prop_name: &JStr,
    model: &ModelIndex,
) -> Result<Option<JStr>, Thrown> {
    Err(Thrown::Unported("JoinChainEmission.classTypedTarget"))
}

/// `JoinChainEmission.hasMappedSubclass` (:732): whether a class of `base`'s subtree is mapped here (a shadow walker
/// `ShadowWalkerCensusTest` pinned; the census retires with the Java).
pub fn has_mapped_subclass(base: &JStr, model: &ModelIndex, ledger: &MappingLedger) -> bool {
    // UNPORTED: JoinChainEmission.hasMappedSubclass
    unported!("JoinChainEmission.hasMappedSubclass")
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
    // UNPORTED: JoinChainEmission.collectJoinNavigationsInPms
    unported!("JoinChainEmission.collectJoinNavigationsInPms")
}

/// `JoinChainEmission.collectJoinNavigationsInPms` (:750, the three-argument form): the PMs' join navigations in PM
/// order (= pass 2's emission order = the slot names); an Inline splice carries its referenced set's
/// (`ResolvedMapping::first_relational`).
pub fn collect_join_navigations_in_pms_of(
    pms: &[PropertyMapping],
    out: &mut Vec<JoinNavSpec>,
    md: Option<&ResolvedMapping<'_>>,
) {
    // UNPORTED: JoinChainEmission.collectJoinNavigationsInPms
    unported!("JoinChainEmission.collectJoinNavigationsInPms")
}

/// `JoinChainEmission.collectJoinNavigations` (:789): one expression's join navigations, in reading order.
pub fn collect_join_navigations(op: &RelationalOperation, out: &mut Vec<JoinNavSpec>) {
    // UNPORTED: JoinChainEmission.collectJoinNavigations
    unported!("JoinChainEmission.collectJoinNavigations")
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
    Err(Thrown::Unported("JoinChainEmission.hopTarget"))
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
    Err(Thrown::Unported("JoinChainEmission.plainClassViewCond"))
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
    Err(Thrown::Unported("JoinChainEmission.nullTolerant"))
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
    Err(Thrown::Unported("JoinChainEmission.targetColumnNames"))
}
