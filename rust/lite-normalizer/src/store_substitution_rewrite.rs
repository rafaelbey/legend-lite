// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! `StoreSubstitutionRewrite` (StoreSubstitutionRewrite.java at ad86fa895): an include's store substitutions applied
//! to the sets pulled through it; every mapping's resolved stores (`MappingDefinition.resolvedStores`), with "Duplicated
//! mapping include" and "mapping include cycle" (outside every catch: they escape a tolerant build, ASK 10 (b));
//! the import-scope store-ref check.
//!
//! RE-EXPRESSED (census §4): the rewrite takes a STORE MAPPER (`m`: a store's name -> its replacement, Java's
//! `m.getOrDefault(db, db)`), and `collectDatabases` runs it with a recording mapper (Java abused an `AbstractMap`
//! as the visitor); `qualifyStoreRefs` drops `current` (`findDatabase` is exact-only now: `current` is always none;
//! at ad86fa895 the method refuses an unqualified store an import names rather than rewriting it). `storeFqn` is
//! ported as Java writes it (`resolveAllStores` calls it; the identity on every input, so either form answers alike).
//! PARK-26 (an include under two spellings refused as a cycle: `pending` counts list entries) is ruling 30's,
//! settled by the engine's answer: until then port the refusal exactly (a Rust in-degree over a SET would accept it).
//!
//! The mapper is called in Java's evaluation order (a constructor's arguments left to right), so the order
//! `collectDatabases` records the stores in -- which `qualifyStoreRefs` refuses first -- is Java's. Java's
//! `Embedded` rebuild (:137-139) uses the two-argument constructor, so the block's `~primaryKey` is dropped and
//! never visited; kept.

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;

use lite_error::Phase;
use lite_model::linked::{LinkedMap, list};
use lite_model::text::{contains, last_index_of};
use lite_model::{
    AssociationMapping, AssociationMappingRelational, AssociationPropertyMapping, ClassMapping,
    ClassMappingAggregateView, ClassMappingAggregationAware, ClassMappingRelational, FilterMapping,
    FilterMappingDirect, FilterMappingJoinMediated, FilterPointer, FilterPointerCross, JStr, JoinChainElement,
    LegacyMappingDefinition, LegacyMappingDefinitionTableReference, MappingIncludeStoreSubstitution, PropertyMapping,
    PropertyMappingColumn, PropertyMappingEmbedded, PropertyMappingEnumeratedColumn,
    PropertyMappingEnumeratedExpression, PropertyMappingExpression, PropertyMappingJoin,
    PropertyMappingJoinTerminalColumn, PropertyMappingLocalProperty, PropertyMappingOtherwiseEmbedded,
    RelationalOperation, RelationalOperationArrayLiteral, RelationalOperationBooleanOp, RelationalOperationColumnRef,
    RelationalOperationComparison, RelationalOperationFunctionCall, RelationalOperationGroup,
    RelationalOperationIsNotNull, RelationalOperationIsNull, RelationalOperationJoinNavigation,
    RelationalOperationLambda, cat,
};
use lite_names::ModelIndex;
use lite_names::model_index::java_list;

use crate::error::{Thrown, require};
use crate::jdk::LinkedSet;

/// A store mapper: a store's name -> the store to read (the name itself when it is not substituted).
pub type StoreMapper<'f> = dyn FnMut(&JStr) -> JStr + 'f;

/// `StoreSubstitutionRewrite.apply` (:35): `cm` with an include's substitutions applied (the last substitution of a
/// store wins).
pub fn apply(cm: &ClassMapping, subs: &[Arc<MappingIncludeStoreSubstitution>]) -> ClassMapping {
    if subs.is_empty() {
        return cm.clone();
    }
    // `Collectors.toMap(original, replacement, (a, b) -> b)`: read only by `getOrDefault`
    let mut by_original: HashMap<JStr, JStr> = HashMap::new();
    for sub in subs {
        by_original.insert(sub.original_store.clone(), sub.replacement_store.clone());
    }
    apply_with(cm, &mut |db: &JStr| {
        by_original.get(db).cloned().unwrap_or_else(|| db.clone())
    })
}

/// `StoreSubstitutionRewrite.applyWith` (:46): a Relational set's every store reference through `m`.
pub fn apply_with(cm: &ClassMapping, m: &mut StoreMapper<'_>) -> ClassMapping {
    let ClassMapping::ClassMappingRelational(r) = cm else {
        return cm.clone();
    };
    ClassMapping::ClassMappingRelational(Arc::new(relational(r, m)))
}

/// `applyWith`'s Relational arm, the arguments in Java's order (the views' sets are rewritten through it: Java's
/// `(ClassMapping.Relational) applyWith(v.set(), m)`).
fn relational(r: &ClassMappingRelational, m: &mut StoreMapper<'_>) -> ClassMappingRelational {
    let main_table = r.main_table.as_ref().map(|t| {
        Arc::new(LegacyMappingDefinitionTableReference {
            database: m(&t.database),
            table: t.table.clone(),
        })
    });
    let filter = r.filter.as_ref().map(|f| filter(f, m));
    let group_by = list(r.group_by.iter().map(|g| op(g, m)).collect());
    let primary_key = list(r.primary_key.iter().map(|g| op(g, m)).collect());
    let property_mappings = list(r.property_mappings.iter().map(|p| pm(p, m)).collect());
    let aggregation = views(r.aggregation.as_ref(), m);
    ClassMappingRelational {
        class_name: r.class_name.clone(),
        set_id: r.set_id.clone(),
        extends_set_id: r.extends_set_id.clone(),
        root: r.root,
        main_table,
        filter,
        distinct: r.distinct,
        group_by,
        primary_key,
        property_mappings,
        source_url: r.source_url.clone(),
        property_target_sets: r.property_target_sets.clone(),
        aggregation,
    }
}

/// `StoreSubstitutionRewrite.views` (:68): an AggregationAware node's view sets through `m`.
pub fn views(
    agg: Option<&Arc<ClassMappingAggregationAware>>,
    m: &mut StoreMapper<'_>,
) -> Option<Arc<ClassMappingAggregationAware>> {
    let agg = agg?;
    let rewritten = agg
        .views
        .iter()
        .map(|v| {
            Arc::new(ClassMappingAggregateView {
                index: v.index,
                can_aggregate: v.can_aggregate,
                group_by_functions: v.group_by_functions.clone(),
                aggregate_values: v.aggregate_values.clone(),
                set: Arc::new(relational(&v.set, m)),
            })
        })
        .collect();
    Some(Arc::new(ClassMappingAggregationAware { views: list(rewritten) }))
}

/// `StoreSubstitutionRewrite.db` (:80): a store name through `m`; none stays none.
pub fn db(database: Option<&JStr>, m: &mut StoreMapper<'_>) -> Option<JStr> {
    database.map(m)
}

/// `StoreSubstitutionRewrite.filter` (:85).
pub fn filter(f: &FilterMapping, m: &mut StoreMapper<'_>) -> FilterMapping {
    match f {
        FilterMapping::FilterMappingDirect(d) => FilterMapping::FilterMappingDirect(Arc::new(FilterMappingDirect {
            filter: pointer(&d.filter, m),
        })),
        FilterMapping::FilterMappingJoinMediated(j) => {
            let source_db = db(j.source_db.as_ref(), m);
            let joins = list(j.joins.iter().map(|c| chain(c, m)).collect());
            let filter = pointer(&j.filter, m);
            FilterMapping::FilterMappingJoinMediated(Arc::new(FilterMappingJoinMediated {
                source_db,
                joins,
                filter,
                join_type: j.join_type.clone(),
            }))
        }
    }
}

/// `StoreSubstitutionRewrite.pointer` (:96).
pub fn pointer(p: &FilterPointer, m: &mut StoreMapper<'_>) -> FilterPointer {
    match p {
        FilterPointer::FilterPointerLocal(_) => p.clone(),
        FilterPointer::FilterPointerCross(c) => FilterPointer::FilterPointerCross(Arc::new(FilterPointerCross {
            db: m(&c.db),
            name: c.name.clone(),
        })),
    }
}

/// `StoreSubstitutionRewrite.chain` (:104).
pub fn chain(c: &Arc<JoinChainElement>, m: &mut StoreMapper<'_>) -> Arc<JoinChainElement> {
    match &c.database_name {
        None => c.clone(),
        Some(d) => Arc::new(JoinChainElement {
            join_name: c.join_name.clone(),
            join_type: c.join_type,
            database_name: Some(m(d)),
            include_self: c.include_self,
        }),
    }
}

/// `StoreSubstitutionRewrite.pm` (:111).
pub fn pm(p: &PropertyMapping, m: &mut StoreMapper<'_>) -> PropertyMapping {
    match p {
        PropertyMapping::PropertyMappingColumn(c) => {
            PropertyMapping::PropertyMappingColumn(Arc::new(PropertyMappingColumn {
                property_name: c.property_name.clone(),
                database: m(&c.database),
                table: c.table.clone(),
                column: c.column.clone(),
            }))
        }
        PropertyMapping::PropertyMappingEnumeratedExpression(e) => {
            PropertyMapping::PropertyMappingEnumeratedExpression(Arc::new(PropertyMappingEnumeratedExpression {
                property_name: e.property_name.clone(),
                enum_mapping_id: e.enum_mapping_id.clone(),
                expression: op(&e.expression, m),
            }))
        }
        PropertyMapping::PropertyMappingEnumeratedColumn(e) => {
            PropertyMapping::PropertyMappingEnumeratedColumn(Arc::new(PropertyMappingEnumeratedColumn {
                property_name: e.property_name.clone(),
                enum_mapping_id: e.enum_mapping_id.clone(),
                database: m(&e.database),
                table: e.table.clone(),
                column: e.column.clone(),
            }))
        }
        PropertyMapping::PropertyMappingJoin(j) => {
            let database = m(&j.database);
            let joins = list(j.joins.iter().map(|c| chain(c, m)).collect());
            PropertyMapping::PropertyMappingJoin(Arc::new(PropertyMappingJoin {
                property_name: j.property_name.clone(),
                database,
                joins,
                target_set_id: j.target_set_id.clone(),
            }))
        }
        PropertyMapping::PropertyMappingJoinTerminalColumn(j) => {
            let database = m(&j.database);
            let joins = list(j.joins.iter().map(|c| chain(c, m)).collect());
            let terminal_column = op(&j.terminal_column, m);
            PropertyMapping::PropertyMappingJoinTerminalColumn(Arc::new(PropertyMappingJoinTerminalColumn {
                property_name: j.property_name.clone(),
                database,
                joins,
                terminal_column,
                enum_mapping_id: j.enum_mapping_id.clone(),
                enum_mapped: j.enum_mapped,
            }))
        }
        PropertyMapping::PropertyMappingExpression(e) => {
            PropertyMapping::PropertyMappingExpression(Arc::new(PropertyMappingExpression {
                property_name: e.property_name.clone(),
                expression: op(&e.expression, m),
            }))
        }
        // Java's two-argument constructor: the block's `~primaryKey` is dropped (the module's note)
        PropertyMapping::PropertyMappingEmbedded(e) => {
            PropertyMapping::PropertyMappingEmbedded(Arc::new(PropertyMappingEmbedded::new(
                e.property_name.clone(),
                list(e.property_mappings.iter().map(|x| pm(x, m)).collect()),
            )))
        }
        PropertyMapping::PropertyMappingInlineEmbedded(_) => p.clone(),
        PropertyMapping::PropertyMappingOtherwiseEmbedded(oe) => {
            let embedded = list(oe.embedded.iter().map(|x| pm(x, m)).collect());
            let fallback = pm(&oe.fallback, m);
            PropertyMapping::PropertyMappingOtherwiseEmbedded(Arc::new(PropertyMappingOtherwiseEmbedded {
                property_name: oe.property_name.clone(),
                embedded,
                fallback_set_id: oe.fallback_set_id.clone(),
                fallback,
            }))
        }
        PropertyMapping::PropertyMappingLocalProperty(lp) => {
            PropertyMapping::PropertyMappingLocalProperty(Arc::new(PropertyMappingLocalProperty {
                property_name: lp.property_name.clone(),
                r#type: lp.r#type.clone(),
                multiplicity: lp.multiplicity.clone(),
                body: pm(&lp.body, m),
            }))
        }
    }
}

/// `StoreSubstitutionRewrite.op` (:151).
pub fn op(o: &RelationalOperation, m: &mut StoreMapper<'_>) -> RelationalOperation {
    use RelationalOperation as R;
    match o {
        R::RelationalOperationColumnRef(c) => R::RelationalOperationColumnRef(Arc::new(RelationalOperationColumnRef {
            database_name: db(c.database_name.as_ref(), m),
            table: c.table.clone(),
            column: c.column.clone(),
        })),
        R::RelationalOperationTargetColumnRef(_) => o.clone(),
        R::RelationalOperationLiteral(_) => o.clone(),
        R::RelationalOperationFunctionCall(f) => {
            R::RelationalOperationFunctionCall(Arc::new(RelationalOperationFunctionCall {
                name: f.name.clone(),
                args: list(f.args.iter().map(|a| op(a, m)).collect()),
            }))
        }
        R::RelationalOperationComparison(c) => {
            let left = op(&c.left, m);
            let right = op(&c.right, m);
            R::RelationalOperationComparison(Arc::new(RelationalOperationComparison { left, op: c.op, right }))
        }
        R::RelationalOperationBooleanOp(b) => {
            let left = op(&b.left, m);
            let right = op(&b.right, m);
            R::RelationalOperationBooleanOp(Arc::new(RelationalOperationBooleanOp { left, op: b.op, right }))
        }
        R::RelationalOperationIsNull(n) => R::RelationalOperationIsNull(Arc::new(RelationalOperationIsNull {
            operand: op(&n.operand, m),
        })),
        R::RelationalOperationIsNotNull(n) => R::RelationalOperationIsNotNull(Arc::new(RelationalOperationIsNotNull {
            operand: op(&n.operand, m),
        })),
        R::RelationalOperationGroup(g) => {
            R::RelationalOperationGroup(Arc::new(RelationalOperationGroup { inner: op(&g.inner, m) }))
        }
        R::RelationalOperationArrayLiteral(a) => {
            R::RelationalOperationArrayLiteral(Arc::new(RelationalOperationArrayLiteral {
                elements: list(a.elements.iter().map(|e| op(e, m)).collect()),
            }))
        }
        R::RelationalOperationLambda(lam) => R::RelationalOperationLambda(Arc::new(RelationalOperationLambda {
            parameters: lam.parameters.clone(),
            body: op(&lam.body, m),
        })),
        R::RelationalOperationLambdaParam(_) => o.clone(),
        R::RelationalOperationJoinNavigation(j) => {
            let database_name = db(j.database_name.as_ref(), m);
            let chain_ = list(j.chain.iter().map(|c| chain(c, m)).collect());
            let terminal = j.terminal.as_ref().map(|t| op(t, m));
            R::RelationalOperationJoinNavigation(Arc::new(RelationalOperationJoinNavigation {
                database_name,
                chain: chain_,
                terminal,
            }))
        }
    }
}

/// `StoreSubstitutionRewrite.applyAssoc` (:189): a Relational association mapping's PMs through `m`.
pub fn apply_assoc(am: &AssociationMapping, m: &mut StoreMapper<'_>) -> AssociationMapping {
    let AssociationMapping::AssociationMappingRelational(rel) = am else {
        return am.clone();
    };
    AssociationMapping::AssociationMappingRelational(Arc::new(AssociationMappingRelational {
        association_name: rel.association_name.clone(),
        property_mappings: list(
            rel.property_mappings
                .iter()
                .map(|apm| {
                    Arc::new(AssociationPropertyMapping {
                        source_set_id: apm.source_set_id.clone(),
                        target_set_id: apm.target_set_id.clone(),
                        body: pm(&apm.body, m),
                    })
                })
                .collect(),
        ),
    }))
}

/// `StoreSubstitutionRewrite.collectDatabases` (:209): every store name the mapping's sets and association mappings
/// read, in reading order (the rewrite run with a recording mapper; the module's note).
pub fn collect_databases(md: &LegacyMappingDefinition, out: &mut LinkedSet<JStr>) {
    // Java's recorder: `getOrDefault(k, d)` records `k` and answers the default, the store itself
    let mut recorder = |k: &JStr| {
        out.add(k.clone());
        k.clone()
    };
    for cm in md.class_mappings.iter() {
        apply_with(cm, &mut recorder);
    }
    for am in md.association_mappings.iter() {
        apply_assoc(am, &mut recorder);
    }
}

/// `StoreSubstitutionRewrite.resolveAllStores` (:247): every mapping's resolved store substitutions (store -> store),
/// include order, mappings in dependency order; loud for an include listed twice or an include cycle.
///
/// PARK-26 is ported here exactly: `pending` counts an includer's LIST of resolved includes and the decrement fires
/// once per finished mapping, so one mapping included under two spellings is refused as an include cycle (the
/// comment at the `incs.push`).
pub fn resolve_all_stores(
    mappings: &[Arc<LegacyMappingDefinition>],
    model: &ModelIndex,
) -> Result<LinkedMap<JStr, LinkedMap<JStr, JStr>>, Thrown> {
    let mut by_fqn: LinkedMap<JStr, Arc<LegacyMappingDefinition>> = LinkedMap::new();
    for md in mappings {
        by_fqn.put(md.qualified_name.clone(), md.clone());
    }
    // include edges by name: mapping -> its resolvable included mappings
    let mut edges: LinkedMap<JStr, Vec<JStr>> = LinkedMap::new();
    let mut pending: LinkedMap<JStr, i32> = LinkedMap::new();
    for md in mappings {
        let mut incs: Vec<JStr> = Vec::new();
        let mut listed: HashSet<JStr> = HashSet::new();
        for inc in md.includes.iter() {
            // the engine's compiler rejects an include listed twice (MappingCompilerExtension: "Duplicated mapping
            // include"); here, before the ordering, so it never masquerades as a cycle
            if !listed.insert(inc.mapping_path.clone()) {
                return Err(Thrown::model(
                    Phase::Model,
                    cat!(
                        "Duplicated mapping include '",
                        &inc.mapping_path,
                        "' in mapping '",
                        &md.qualified_name,
                        "'"
                    ),
                    Some(&md.qualified_name),
                ));
            }
            // PARK-26 (ruling 30; docs/PARKED_WORK_LEDGER.md): `incs` is a LIST, so an include spelled two ways
            // (`include w::A` and `include A` in package `w`) resolves to one name counted twice here, while the
            // decrement below fires once per finished mapping -- the includer is refused as an include cycle.
            // Ported exactly (an in-degree over a set would accept it) until the engine's answer settles it.
            if let Some(fqn) = include_fqn(&md.qualified_name, &inc.mapping_path, &by_fqn) {
                incs.push(fqn);
            }
        }
        pending.put(md.qualified_name.clone(), incs.len() as i32);
        edges.put(md.qualified_name.clone(), incs);
    }
    // Kahn: a mapping is ready when every include it names is done
    let mut ready: VecDeque<JStr> = VecDeque::new();
    for (fqn, n) in pending.iter() {
        if *n == 0 {
            ready.push_back(fqn.clone());
        }
    }
    let mut out: LinkedMap<JStr, LinkedMap<JStr, JStr>> = LinkedMap::new();
    while let Some(fqn) = ready.pop_front() {
        let md = require(by_fqn.get(&fqn), None)?.clone();
        let mut resolved: LinkedMap<JStr, JStr> = LinkedMap::new();
        for inc in md.includes.iter() {
            let inc_fqn = include_fqn(&fqn, &inc.mapping_path, &by_fqn);
            let empty = LinkedMap::new();
            let deep = inc_fqn.and_then(|f| out.get(&f)).unwrap_or(&empty);
            let mut own: LinkedMap<JStr, JStr> = LinkedMap::new();
            for sub in inc.substitutions.iter() {
                own.put(
                    store_fqn(&sub.original_store, model),
                    store_fqn(&sub.replacement_store, model),
                );
            }
            let mut this_include: LinkedMap<JStr, JStr> = LinkedMap::new();
            for (k, v) in deep.iter() {
                this_include.put(k.clone(), own.get(v).cloned().unwrap_or_else(|| v.clone()));
            }
            for (k, v) in own.iter() {
                this_include.put_if_absent(k.clone(), v.clone());
            }
            for (k, v) in this_include.iter() {
                resolved.put_if_absent(k.clone(), v.clone());
            }
        }
        out.put(fqn.clone(), resolved);
        for (includer, incs) in edges.iter() {
            if incs.contains(&fqn) && !out.contains_key(includer) {
                // `pending.merge(includer, -1, Integer::sum)`
                let left = pending.get_or_insert_with(includer.clone(), || 0);
                *left -= 1;
                if *left == 0 {
                    ready.push_back(includer.clone());
                }
            }
        }
    }
    // the graph's nodes are the NAMES (byFqn), never the parsed list: a mapping defined twice is one node (refused as
    // "Duplicated element" before this phase, Compiler.normalizeLayer); counting the list named an empty cycle
    // (2026-10-11)
    if out.len() != by_fqn.len() {
        let cycle: Vec<JStr> = by_fqn
            .iter()
            .filter(|(k, _)| !out.contains_key(k))
            .map(|(k, _)| k.clone())
            .collect();
        return Err(Thrown::model(
            Phase::Normalize,
            cat!(
                "mapping include cycle: ",
                java_list(&cycle),
                " \u{2014} a mapping cannot include itself, directly or through its includes"
            ),
            Some(&cycle[0]),
        ));
    }
    Ok(out)
}

/// `StoreSubstitutionRewrite.includeFqn` (:331): an include path's mapping among `present` (Java's `byFqn.keySet()`:
/// here the map itself) -- exact, else in the includer's package; none otherwise.
pub fn include_fqn(
    includer: &JStr,
    path: &JStr,
    present: &LinkedMap<JStr, Arc<LegacyMappingDefinition>>,
) -> Option<JStr> {
    if present.contains_key(path) {
        return Some(path.clone());
    }
    let cut = last_index_of(&includer.0, &JStr::from("::").0);
    if !contains(&path.0, "::")
        && let Some(cut) = cut
    {
        let in_pkg = cat!(&includer.0[..cut], "::", path);
        if present.contains_key(&in_pkg) {
            return Some(in_pkg);
        }
    }
    None
}

/// `StoreSubstitutionRewrite.storeFqn` (:346): a store path's database FQN, else the path.
pub fn store_fqn(path: &JStr, model: &ModelIndex) -> JStr {
    model
        .find_database(path)
        .map_or_else(|| path.clone(), |d| d.qualified_name.clone())
}

/// `StoreSubstitutionRewrite.qualifyStoreRefs` (:362): the mapping unchanged, or the refusal of an unqualified store
/// reference one of its import wildcards names (the module's note).
pub fn qualify_store_refs(
    md: &Arc<LegacyMappingDefinition>,
    model: &ModelIndex,
) -> Result<Arc<LegacyMappingDefinition>, Thrown> {
    let scope = model.imports_of(&md.qualified_name);
    if scope.wildcards.is_empty() {
        return Ok(md.clone());
    }
    let mut raws: LinkedSet<JStr> = LinkedSet::new();
    collect_databases(md, &mut raws);
    for r in raws.iter() {
        if contains(&r.0, "::") || model.has_database_exact(r) {
            continue;
        }
        // RE-EXPRESSED (census §4): Java's `current` (`findDatabase(r)`, the same exact lookup `hasDatabaseExact`
        // just answered none) is always none here, so `!cand.equals(current)` always holds
        for w in scope.wildcards.iter() {
            let cand = cat!(w, "::", r);
            if model.has_database_exact(&cand) {
                // a store spelling step D left unqualified that an import of the mapping's section names: the
                // model's error, in the words of the store lookups beside this one (XStore's, ModelJoin's); until
                // 2026-11-14 a rewrite to the import's database no text reached (ruling 30)
                return Err(Thrown::model(
                    Phase::Normalize,
                    cat!(
                        "Mapping references unknown database '",
                        r,
                        "' (name resolution left it unqualified; the import '",
                        w,
                        "::*' names '",
                        &cand,
                        "'); mapping=",
                        &md.qualified_name
                    ),
                    None,
                ));
            }
        }
    }
    Ok(md.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    use lite_model::linked::empty;
    use lite_model::text::j;
    use lite_model::{ClassMapping, MappingInclude, PropertyMappingEmbedded};

    use crate::test_support::{cm, column_pm, index, legacy, model, relational};

    fn include(path: &str, subs: &[(&str, &str)]) -> Arc<MappingInclude> {
        Arc::new(MappingInclude {
            mapping_path: j(path),
            substitutions: list(
                subs.iter()
                    .map(|(o, r)| {
                        Arc::new(MappingIncludeStoreSubstitution {
                            original_store: j(o),
                            replacement_store: j(r),
                        })
                    })
                    .collect(),
            ),
        })
    }

    fn with_includes(fqn: &str, incs: Vec<Arc<MappingInclude>>) -> Arc<LegacyMappingDefinition> {
        Arc::new(LegacyMappingDefinition {
            includes: list(incs),
            ..legacy(fqn, Vec::new())
        })
    }

    fn empty_index() -> ModelIndex {
        index(&model("###Pure\nClass w::X\n{\n  name: String[1];\n}\n"))
    }

    fn pairs(m: &LinkedMap<JStr, JStr>) -> Vec<(JStr, JStr)> {
        m.iter().cloned().collect()
    }

    #[test]
    fn chained_includes_compose_their_substitutions_in_include_order() {
        let idx = empty_index();
        let a = with_includes("w::A", Vec::new());
        let b = with_includes("w::B", vec![include("w::A", &[("db1", "db2")])]);
        // `A` resolves in the includer's package
        let c = with_includes("w::C", vec![include("B", &[("db2", "db3")])]);
        let out = resolve_all_stores(&[c, b, a], &idx).unwrap();
        // dependency order: the mappings with no includes first
        let order: Vec<JStr> = out.iter().map(|(k, _)| k.clone()).collect();
        assert_eq!(order, vec![j("w::A"), j("w::B"), j("w::C")]);
        assert!(out.get(&j("w::A")).unwrap().is_empty());
        assert_eq!(pairs(out.get(&j("w::B")).unwrap()), vec![(j("db1"), j("db2"))]);
        assert_eq!(
            pairs(out.get(&j("w::C")).unwrap()),
            vec![(j("db1"), j("db3")), (j("db2"), j("db3"))]
        );
    }

    #[test]
    fn an_include_listed_twice_is_refused_before_the_ordering() {
        let idx = empty_index();
        let a = with_includes("w::A", Vec::new());
        let aa = with_includes("w::AA", vec![include("w::A", &[]), include("w::A", &[])]);
        let e = resolve_all_stores(&[a, aa], &idx).unwrap_err();
        assert!(e.is_model());
        assert_eq!(e.phase(), Some(Phase::Model));
        assert_eq!(e.element(), Some(j("w::AA")));
        assert_eq!(
            e.message(),
            Some(j("Duplicated mapping include 'w::A' in mapping 'w::AA'"))
        );
    }

    #[test]
    fn an_include_cycle_names_every_mapping_never_ready_in_name_order() {
        let idx = empty_index();
        let a = with_includes("w::A", vec![include("w::B", &[])]);
        let b = with_includes("w::B", vec![include("w::A", &[])]);
        // the includer of a cycle is never ready either: the list over-reports, as Java's does
        let top = with_includes("w::Top", vec![include("w::A", &[])]);
        let free = with_includes("w::Free", Vec::new());
        let e = resolve_all_stores(&[a, b, top, free], &idx).unwrap_err();
        assert!(e.is_model());
        assert_eq!(e.phase(), Some(Phase::Normalize));
        assert_eq!(e.element(), Some(j("w::A")));
        assert_eq!(
            e.message(),
            Some(j(
                "mapping include cycle: [w::A, w::B, w::Top] \u{2014} a mapping cannot include itself, directly or \
                 through its includes"
            ))
        );
    }

    #[test]
    fn park_26_one_mapping_included_under_two_spellings_is_refused_as_a_cycle() {
        let idx = empty_index();
        let a = with_includes("w::A", Vec::new());
        let m = with_includes("w::M", vec![include("w::A", &[]), include("A", &[])]);
        let e = resolve_all_stores(&[a, m], &idx).unwrap_err();
        assert_eq!(
            e.message(),
            Some(j(
                "mapping include cycle: [w::M] \u{2014} a mapping cannot include itself, directly or through its \
                 includes"
            ))
        );
        assert_eq!(e.element(), Some(j("w::M")));
    }

    #[test]
    fn an_include_path_resolves_exactly_else_in_the_includers_package() {
        let mut present = LinkedMap::new();
        present.put(j("w::A"), with_includes("w::A", Vec::new()));
        assert_eq!(include_fqn(&j("w::M"), &j("w::A"), &present), Some(j("w::A")));
        assert_eq!(include_fqn(&j("w::M"), &j("A"), &present), Some(j("w::A")));
        // a qualified path is never re-packaged; an includer in the root package has no package
        assert_eq!(include_fqn(&j("w::M"), &j("x::A"), &present), None);
        assert_eq!(include_fqn(&j("M"), &j("A"), &present), None);
    }

    fn join_chain(db: Option<&str>) -> Arc<JoinChainElement> {
        Arc::new(JoinChainElement {
            join_name: j("J"),
            join_type: None,
            database_name: db.map(j),
            include_self: false,
        })
    }

    fn column_ref(db: Option<&str>) -> RelationalOperation {
        RelationalOperation::RelationalOperationColumnRef(Arc::new(RelationalOperationColumnRef {
            database_name: db.map(j),
            table: j("T"),
            column: j("C"),
        }))
    }

    /// A set reading a store at every kind of place, each store named for its place.
    fn every_place() -> LegacyMappingDefinition {
        let join = PropertyMapping::PropertyMappingJoin(Arc::new(PropertyMappingJoin {
            property_name: j("firm"),
            database: j("joinDb"),
            joins: list(vec![join_chain(Some("hopDb")), join_chain(None)]),
            target_set_id: None,
        }));
        let embedded = PropertyMapping::PropertyMappingEmbedded(Arc::new(PropertyMappingEmbedded {
            property_name: j("address"),
            property_mappings: list(vec![column_pm("street", "T", "STREET")]),
            primary_key: list(vec![column_ref(Some("embeddedPkDb"))]),
        }));
        let set = ClassMappingRelational {
            main_table: Some(Arc::new(LegacyMappingDefinitionTableReference {
                database: j("mainDb"),
                table: j("T"),
            })),
            filter: Some(FilterMapping::FilterMappingJoinMediated(Arc::new(
                FilterMappingJoinMediated {
                    source_db: Some(j("filterSourceDb")),
                    joins: list(vec![join_chain(Some("filterHopDb"))]),
                    filter: FilterPointer::FilterPointerCross(Arc::new(FilterPointerCross {
                        db: j("filterDb"),
                        name: j("F"),
                    })),
                    join_type: None,
                },
            ))),
            group_by: list(vec![column_ref(Some("groupDb")), column_ref(None)]),
            primary_key: list(vec![column_ref(Some("pkDb"))]),
            ..relational("w::P", Some("p"), vec![join, embedded])
        };
        LegacyMappingDefinition {
            association_mappings: list(vec![AssociationMapping::AssociationMappingRelational(Arc::new(
                AssociationMappingRelational {
                    association_name: j("w::A"),
                    property_mappings: list(vec![Arc::new(AssociationPropertyMapping {
                        source_set_id: None,
                        target_set_id: None,
                        body: column_pm("x", "T", "X"),
                    })]),
                },
            ))]),
            ..legacy("w::M", vec![cm(set)])
        }
    }

    #[test]
    fn the_stores_are_collected_in_javas_reading_order() {
        let mut out = LinkedSet::new();
        collect_databases(&every_place(), &mut out);
        // the embedded block's `~primaryKey` is dropped by Java's two-argument rebuild: never read
        assert_eq!(
            out.to_vec(),
            vec![
                j("mainDb"),
                j("filterSourceDb"),
                j("filterHopDb"),
                j("filterDb"),
                j("groupDb"),
                j("pkDb"),
                j("joinDb"),
                j("hopDb"),
                j("db"),
            ]
        );
    }

    #[test]
    fn a_substitution_repoints_every_store_and_the_last_pair_of_a_store_wins() {
        let md = every_place();
        let subs = [
            Arc::new(MappingIncludeStoreSubstitution {
                original_store: j("joinDb"),
                replacement_store: j("first"),
            }),
            Arc::new(MappingIncludeStoreSubstitution {
                original_store: j("joinDb"),
                replacement_store: j("second"),
            }),
        ];
        let ClassMapping::ClassMappingRelational(r) = apply(&md.class_mappings[0], &subs) else {
            panic!("a Relational set stays one");
        };
        let PropertyMapping::PropertyMappingJoin(jn) = &r.property_mappings[0] else {
            panic!("the join");
        };
        assert_eq!(jn.database, j("second"));
        assert_eq!(r.main_table.as_ref().unwrap().database, j("mainDb"));
        // a hop with no store is kept as it is (the same element)
        let ClassMapping::ClassMappingRelational(orig) = &md.class_mappings[0] else {
            unreachable!()
        };
        let PropertyMapping::PropertyMappingJoin(before) = &orig.property_mappings[0] else {
            unreachable!()
        };
        assert!(lite_base::same(&before.joins[1], &jn.joins[1]));
        assert!(!lite_base::same(&before.joins[0], &jn.joins[0]));
        // the embedded block lost its primary key (Java's two-argument constructor)
        let PropertyMapping::PropertyMappingEmbedded(e) = &r.property_mappings[1] else {
            unreachable!()
        };
        assert!(e.primary_key.is_empty());
        // no substitutions: the very set back
        let ClassMapping::ClassMappingRelational(kept) = apply(&md.class_mappings[0], &[]) else {
            unreachable!()
        };
        assert!(lite_base::same(&kept, orig));
    }

    #[test]
    fn only_relational_sets_and_association_mappings_are_rewritten() {
        let union = crate::test_support::union("w::P", Some("u"), &["p"]);
        let mut seen = Vec::new();
        let out = apply_with(&union, &mut |d: &JStr| {
            seen.push(d.clone());
            j("other")
        });
        assert_eq!(out, union);
        assert!(seen.is_empty());
        let cross = AssociationMapping::AssociationMappingCross(Arc::new(lite_model::AssociationMappingCross {
            association_name: j("w::A"),
            property_mappings2: empty(),
        }));
        assert_eq!(apply_assoc(&cross, &mut |d: &JStr| d.clone()), cross);
    }

    #[test]
    fn an_unqualified_store_an_import_names_is_refused() {
        let parsed = model(
            "###Pure\nClass a::P\n{\n  name: String[1];\n}\n\n###Relational\nDatabase a::db\n(\n  Table T (NAME \
             VARCHAR(20))\n)\n\n###Mapping\nimport a::*;\nMapping a::M\n(\n  a::P: Relational\n  {\n    ~mainTable \
             [a::db]T\n    name: [a::db]T.NAME\n  }\n)\n",
        );
        let idx = index(&parsed);
        let authored = crate::test_support::mapping(&parsed, "a::M");
        // step D qualified the store: nothing to refuse, the very mapping back
        assert!(lite_base::same(
            &qualify_store_refs(&authored, &idx).unwrap(),
            &authored
        ));
        // a spelling D left bare, which the section's import names
        let bare = Arc::new(LegacyMappingDefinition {
            class_mappings: list(vec![cm(relational(
                "a::P",
                None,
                vec![PropertyMapping::PropertyMappingColumn(Arc::new(
                    PropertyMappingColumn {
                        property_name: j("name"),
                        database: j("db"),
                        table: j("T"),
                        column: j("NAME"),
                    },
                ))],
            ))]),
            ..(*authored).clone()
        });
        let e = qualify_store_refs(&bare, &idx).unwrap_err();
        assert!(e.is_model());
        assert_eq!(e.phase(), Some(Phase::Normalize));
        assert_eq!(e.element(), None);
        assert_eq!(
            e.message(),
            Some(j(
                "Mapping references unknown database 'db' (name resolution left it unqualified; the import 'a::*' \
                 names 'a::db'); mapping=a::M"
            ))
        );
        // a bare spelling no import names stays
        let nowhere = Arc::new(LegacyMappingDefinition {
            class_mappings: list(vec![cm(relational(
                "a::P",
                None,
                vec![PropertyMapping::PropertyMappingColumn(Arc::new(
                    PropertyMappingColumn {
                        property_name: j("name"),
                        database: j("elsewhere"),
                        table: j("T"),
                        column: j("NAME"),
                    },
                ))],
            ))]),
            ..(*authored).clone()
        });
        assert!(qualify_store_refs(&nowhere, &idx).is_ok());
        // a store's FQN is itself when the model has it, else the path
        assert_eq!(store_fqn(&j("a::db"), &idx), j("a::db"));
        assert_eq!(store_fqn(&j("db"), &idx), j("db"));
    }
}
