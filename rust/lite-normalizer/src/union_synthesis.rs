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

use std::sync::Arc;

use lite_front::records as p;
use lite_model::linked::LinkedMap;
use lite_model::{
    ClassMapping, ClassMappingInheritance, ClassMappingRelational, ClassMappingUnion, JStr, LegacyMappingDefinition,
    PropertyMapping, PropertyMappingJoin,
};
use lite_names::ModelIndex;

use crate::error::Thrown;
use crate::jdk::LinkedSet;
use crate::mapping_ledger::MappingLedger;
use crate::pipeline::Pipeline;
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
    Err(Thrown::Unported("UnionSynthesis.memberFunction"))
}

/// `UnionSynthesis.memberFunction(pt)` (:75): a pin's member-function FQN (the second spelling of the naming rule).
pub fn member_function_of(pt: &PinTarget) -> Result<JStr, Thrown> {
    Err(Thrown::Unported("UnionSynthesis.memberFunction"))
}

/// `UnionSynthesis.resolvePin` (:103): the set `set_id` pins, from the queried mapping's closure; none when not
/// visible (the "Declared divergence" of :101-102 is ASK 12's).
pub fn resolve_pin(
    md: &ResolvedMapping<'_>,
    set_id: Option<&JStr>,
    model: &ModelIndex,
) -> Result<Option<PinTarget>, Thrown> {
    Err(Thrown::Unported("UnionSynthesis.resolvePin"))
}

/// `UnionSynthesis.pinOwners` (:130): every mapping of the model defining a set `set_id` (the index's
/// `legacy_sets_with_id`).
pub fn pin_owners(set_id: &JStr, model: &ModelIndex) -> Vec<PinTarget> {
    // UNPORTED: UnionSynthesis.pinOwners
    unported!("UnionSynthesis.pinOwners")
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
    // UNPORTED: UnionSynthesis.pmIdentity
    unported!("UnionSynthesis.pmIdentity")
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
    Err(Thrown::Unported("UnionSynthesis.collectRoutedJoins"))
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
    Err(Thrown::Unported("UnionSynthesis.collectRoutedJoins"))
}

/// `UnionSynthesis.recordOwner` (:261): the owner class a routed property resolves against; loud for two owners.
pub fn record_owner(
    owner_by_prop: &mut LinkedMap<JStr, JStr>,
    prop: &JStr,
    owner_cls: &JStr,
    md: &ResolvedMapping<'_>,
) -> Result<(), Thrown> {
    Err(Thrown::Unported("UnionSynthesis.recordOwner"))
}

/// `UnionSynthesis.embeddedOwner` (:275): the class an embedded property's sub-PMs belong to.
pub fn embedded_owner(owner_cls: &JStr, prop: &JStr, model: &ModelIndex) -> Result<Option<JStr>, Thrown> {
    Err(Thrown::Unported("UnionSynthesis.embeddedOwner"))
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
    Err(Thrown::Unported("UnionSynthesis.classifyUnionRoutes"))
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
