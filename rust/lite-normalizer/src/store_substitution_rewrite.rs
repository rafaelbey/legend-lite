// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! `StoreSubstitutionRewrite` (StoreSubstitutionRewrite.java at ad86fa895): an include's store substitutions applied
//! to the sets pulled through it; every mapping's resolved stores (`MappingDefinition.resolvedStores`), with "Duplicated
//! mapping include" and "mapping include cycle" (outside every catch: they escape a tolerant build, ASK 10 (b));
//! the import-scope store-ref check.
//!
//! RE-EXPRESSED (census §4): the rewrite takes a STORE MAPPER (`m`: a store's name -> its replacement, Java's
//! `m.getOrDefault(db, db)`), and `collectDatabases` runs it with a recording mapper (Java abused an `AbstractMap`
//! as the visitor); `qualifyStoreRefs` drops `current` and `storeFqn` (`findDatabase` is exact-only now: `current` is
//! always none; at ad86fa895 the method refuses an unqualified store an import names rather than rewriting it).
//! PARK-26 (an include under two spellings refused as a cycle: `pending` counts list entries) is ruling 30's,
//! settled by the engine's answer: until then port the refusal exactly (a Rust in-degree over a SET would accept it).

use std::sync::Arc;

use lite_model::linked::LinkedMap;
use lite_model::{
    AssociationMapping, ClassMapping, ClassMappingAggregationAware, FilterMapping, FilterPointer, JStr,
    JoinChainElement, LegacyMappingDefinition, MappingIncludeStoreSubstitution, PropertyMapping, RelationalOperation,
};
use lite_names::ModelIndex;

use crate::error::Thrown;
use crate::jdk::LinkedSet;
use crate::unported;

/// A store mapper: a store's name -> the store to read (the name itself when it is not substituted).
pub type StoreMapper<'f> = dyn FnMut(&JStr) -> JStr + 'f;

/// `StoreSubstitutionRewrite.apply` (:35): `cm` with an include's substitutions applied (the last substitution of a
/// store wins).
pub fn apply(cm: &ClassMapping, subs: &[Arc<MappingIncludeStoreSubstitution>]) -> ClassMapping {
    // UNPORTED: StoreSubstitutionRewrite.apply
    unported!("StoreSubstitutionRewrite.apply")
}

/// `StoreSubstitutionRewrite.applyWith` (:46): a Relational set's every store reference through `m`.
pub fn apply_with(cm: &ClassMapping, m: &mut StoreMapper<'_>) -> ClassMapping {
    // UNPORTED: StoreSubstitutionRewrite.applyWith
    unported!("StoreSubstitutionRewrite.applyWith")
}

/// `StoreSubstitutionRewrite.views` (:68): an AggregationAware node's view sets through `m`.
pub fn views(
    agg: Option<&Arc<ClassMappingAggregationAware>>,
    m: &mut StoreMapper<'_>,
) -> Option<Arc<ClassMappingAggregationAware>> {
    // UNPORTED: StoreSubstitutionRewrite.views
    unported!("StoreSubstitutionRewrite.views")
}

/// `StoreSubstitutionRewrite.db` (:80): a store name through `m`; none stays none.
pub fn db(database: Option<&JStr>, m: &mut StoreMapper<'_>) -> Option<JStr> {
    // UNPORTED: StoreSubstitutionRewrite.db
    unported!("StoreSubstitutionRewrite.db")
}

/// `StoreSubstitutionRewrite.filter` (:85).
pub fn filter(f: &FilterMapping, m: &mut StoreMapper<'_>) -> FilterMapping {
    // UNPORTED: StoreSubstitutionRewrite.filter
    unported!("StoreSubstitutionRewrite.filter")
}

/// `StoreSubstitutionRewrite.pointer` (:96).
pub fn pointer(p: &FilterPointer, m: &mut StoreMapper<'_>) -> FilterPointer {
    // UNPORTED: StoreSubstitutionRewrite.pointer
    unported!("StoreSubstitutionRewrite.pointer")
}

/// `StoreSubstitutionRewrite.chain` (:104).
pub fn chain(c: &Arc<JoinChainElement>, m: &mut StoreMapper<'_>) -> Arc<JoinChainElement> {
    // UNPORTED: StoreSubstitutionRewrite.chain
    unported!("StoreSubstitutionRewrite.chain")
}

/// `StoreSubstitutionRewrite.pm` (:111).
pub fn pm(p: &PropertyMapping, m: &mut StoreMapper<'_>) -> PropertyMapping {
    // UNPORTED: StoreSubstitutionRewrite.pm
    unported!("StoreSubstitutionRewrite.pm")
}

/// `StoreSubstitutionRewrite.op` (:151).
pub fn op(o: &RelationalOperation, m: &mut StoreMapper<'_>) -> RelationalOperation {
    // UNPORTED: StoreSubstitutionRewrite.op
    unported!("StoreSubstitutionRewrite.op")
}

/// `StoreSubstitutionRewrite.applyAssoc` (:189): a Relational association mapping's PMs through `m`.
pub fn apply_assoc(am: &AssociationMapping, m: &mut StoreMapper<'_>) -> AssociationMapping {
    // UNPORTED: StoreSubstitutionRewrite.applyAssoc
    unported!("StoreSubstitutionRewrite.applyAssoc")
}

/// `StoreSubstitutionRewrite.collectDatabases` (:209): every store name the mapping's sets and association mappings
/// read, in reading order (the rewrite run with a recording mapper; the module's note).
pub fn collect_databases(md: &LegacyMappingDefinition, out: &mut LinkedSet<JStr>) {
    // UNPORTED: StoreSubstitutionRewrite.collectDatabases
    unported!("StoreSubstitutionRewrite.collectDatabases")
}

/// `StoreSubstitutionRewrite.resolveAllStores` (:247): every mapping's resolved store substitutions (store -> store),
/// include order, mappings in dependency order; loud for an include listed twice or an include cycle.
pub fn resolve_all_stores(
    mappings: &[Arc<LegacyMappingDefinition>],
    model: &ModelIndex,
) -> Result<LinkedMap<JStr, LinkedMap<JStr, JStr>>, Thrown> {
    Err(Thrown::Unported("StoreSubstitutionRewrite.resolveAllStores"))
}

/// `StoreSubstitutionRewrite.includeFqn` (:331): an include path's mapping among `present` (Java's `byFqn.keySet()`:
/// here the map itself) -- exact, else in the includer's package; none otherwise.
pub fn include_fqn(
    includer: &JStr,
    path: &JStr,
    present: &LinkedMap<JStr, Arc<LegacyMappingDefinition>>,
) -> Option<JStr> {
    // UNPORTED: StoreSubstitutionRewrite.includeFqn
    unported!("StoreSubstitutionRewrite.includeFqn")
}

/// `StoreSubstitutionRewrite.storeFqn` (:346): a store path's database FQN, else the path.
pub fn store_fqn(path: &JStr, model: &ModelIndex) -> JStr {
    // UNPORTED: StoreSubstitutionRewrite.storeFqn
    unported!("StoreSubstitutionRewrite.storeFqn")
}

/// `StoreSubstitutionRewrite.qualifyStoreRefs` (:362): the mapping unchanged, or the refusal of an unqualified store
/// reference one of its import wildcards names (the module's note).
pub fn qualify_store_refs(
    md: &Arc<LegacyMappingDefinition>,
    model: &ModelIndex,
) -> Result<Arc<LegacyMappingDefinition>, Thrown> {
    Err(Thrown::Unported("StoreSubstitutionRewrite.qualifyStoreRefs"))
}
