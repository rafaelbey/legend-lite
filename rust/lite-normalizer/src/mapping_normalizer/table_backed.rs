// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! MappingNormalizer family 13 (EF_CENSUS §1.1): the table-backed pipeline -- the main row pipeline (hops, union
//! routes, filter, groupBy, distinct, `^C(...)`). The [`Pipeline`](crate::pipeline::Pipeline) made here holds the
//! ledger from then on (`Pipeline::new(src, ledger, md.views())`): reach it through the pipeline. Where Java writes
//! `p.expr = f(p.expr, .., p, ..)`, read `p.expr` first (a cheap `Arc` clone) and assign after: the callee may move
//! the pipeline itself. An unknown filter is skipped at :1720 (loud later, MN:2240); `~primaryKey` is a deliberate
//! no-op.

use std::sync::Arc;

use lite_front::records as p;
use lite_front::records::ValueSpecification as VS;
use lite_model::linked::{LinkedMap, empty};
use lite_model::{
    ClassMapping, ClassMappingRelational, FilterMapping, FilterPointer, JStr, PropertyMapping, RelationalOperation, cat,
};
use lite_names::{ModelIndex, calls};

use super::Fields;
use super::alias_scope::seed_alias_scope;
use super::fields::translate_pm_to_field;
use super::filter::apply_filter;
use super::instances::build_new_instance_to_one;
use super::relational::canonical_table;
use crate::error::{Thrown, require};
use crate::group_by_synthesis;
use crate::jdk::LinkedSet;
use crate::join_chain_emission::{self, JoinNavSpec};
use crate::mapping_ledger::MappingLedger;
use crate::pipeline::Pipeline;
use crate::rel_op_translator::TableScope;
use crate::resolved_mapping::ResolvedMapping;
use crate::union_synthesis;

/// `MappingNormalizer.RelationalParts` (:1655): a table-backed set's pipeline, row binding and constructor fields,
/// before the `map(row | ^C(...))` that closes them.
#[derive(Clone, Debug)]
pub struct RelationalParts {
    pub pipeline: p::ValueSpecification,
    pub row_bind: Arc<p::Variable>,
    pub fields: Fields,
}

/// `MappingNormalizer.synthTableBackedMapping` (:1659): a table-backed set's body (`source_override` replaces the
/// main table as the pipeline's source: a view's frame, an INNER mapping filter's source).
pub fn synth_table_backed_mapping(
    md: &ResolvedMapping<'_>,
    rcm: &Arc<ClassMappingRelational>,
    model: &ModelIndex,
    ledger: &mut MappingLedger,
    source_override: Option<&p::ValueSpecification>,
) -> Result<p::ValueSpecification, Thrown> {
    let parts = synth_table_backed_parts(md, rcm, model, ledger, source_override)?;
    let body = build_new_instance_to_one(Some(&rcm.class_name), &parts.fields, model)?;
    Ok(platform(
        "map",
        vec![
            parts.pipeline,
            VS::LambdaFunction(Arc::new(p::LambdaFunction {
                parameters: vec![parts.row_bind].into(),
                body: vec![body].into(),
                pos: None,
            })),
        ],
    ))
}

/// `MappingNormalizer.synthTableBackedParts` (:1677): the pipeline (passes 1-3 of JoinChainEmission, union routes,
/// filter, groupBy, distinct) and the fields; also GroupBySynthesis' and AssociationSynthesis' entry.
pub fn synth_table_backed_parts(
    md: &ResolvedMapping<'_>,
    rcm: &Arc<ClassMappingRelational>,
    model: &ModelIndex,
    ledger: &mut MappingLedger,
    source_override: Option<&p::ValueSpecification>,
) -> Result<RelationalParts, Thrown> {
    // a mapping ~filter with an EXPLICIT (INNER) join type row-explodes (engine getRelationalElementWithInnerJoin):
    // its filtered source, then the set again with the filter removed
    if let Some(FilterMapping::FilterMappingJoinMediated(jmi)) = &rcm.filter
        && jmi.join_type.is_some()
    {
        let inner_src = join_chain_emission::inner_filtered_source(rcm, jmi, model, md, ledger)?;
        let no_filter = Arc::new(ClassMappingRelational {
            class_name: rcm.class_name.clone(),
            set_id: rcm.set_id.clone(),
            extends_set_id: rcm.extends_set_id.clone(),
            root: rcm.root,
            main_table: rcm.main_table.clone(),
            filter: None,
            distinct: rcm.distinct,
            group_by: rcm.group_by.clone(),
            primary_key: rcm.primary_key.clone(),
            property_mappings: rcm.property_mappings.clone(),
            // Java's rebuild passes null for `sourceUrl` as well
            source_url: None,
            property_target_sets: rcm.property_target_sets.clone(),
            aggregation: rcm.aggregation.clone(),
        });
        return synth_table_backed_parts(md, &no_filter, model, ledger, Some(&inner_src));
    }

    let m_main = require(rcm.main_table.as_ref(), Some("relational set without ~mainTable"))?;
    let main_db = m_main.database.clone();
    let main_table = canonical_table(&m_main.table);
    let row_bind = Arc::new(p::Variable {
        name: JStr::from("row"),
        r#type: None,
        multiplicity: None,
        pos: None,
    });

    // query-parser parity: the database a PackageableElementPtr, the table a string (`#>{db.TABLE}#`'s shapes)
    let source = match source_override {
        Some(s) => s.clone(),
        None => platform(
            "tableReference",
            vec![
                VS::PackageableElementPtr(Arc::new(p::PackageableElementPtr {
                    full_path: main_db.clone(),
                    pos: None,
                })),
                VS::CString(Arc::new(p::CString {
                    value: main_table.clone(),
                    pos: None,
                    multi_line: false,
                })),
            ],
        ),
    };
    let mut p = Pipeline::new(source, ledger, md.views());
    union_synthesis::classify_union_routes(md, rcm, model, &mut p)?;

    // pass 1: structural chain emission; on a ~groupBy class the Join PMs DEFER (stage 2, after the groupBy)
    let grouped = !rcm.group_by.is_empty();
    for pm in rcm.property_mappings.iter() {
        if grouped && matches!(pm, PropertyMapping::PropertyMappingJoin(_)) {
            continue;
        }
        join_chain_emission::emit_hops_for_structural_pm(
            &mut p,
            pm,
            &rcm.class_name,
            &main_db,
            &main_table,
            &row_bind,
            model,
            md,
        )?;
    }
    // pass 2: the join navigations nested in the PMs, the groupBy keys and a direct filter's condition, each unique
    // chain a join step (an unknown filter is skipped here: it is loud in applyFilter)
    let mut nested: Vec<JoinNavSpec> = Vec::new();
    join_chain_emission::collect_join_navigations_in_pms_of(&rcm.property_mappings, &mut nested, Some(md));
    for key in rcm.group_by.iter() {
        join_chain_emission::collect_join_navigations(key, &mut nested);
    }
    if let Some(FilterMapping::FilterMappingDirect(dfilt)) = &rcm.filter {
        let (db_fqn, name) = match &dfilt.filter {
            FilterPointer::FilterPointerCross(c) => (c.db.clone(), &c.name),
            FilterPointer::FilterPointerLocal(l) => (main_db.clone(), &l.name),
        };
        if let Some(fd) = model.find_filter(Some(&db_fqn), name) {
            join_chain_emission::collect_join_navigations(&fd.condition, &mut nested);
        }
    }
    for spec in &nested {
        join_chain_emission::emit_join_chain(
            &mut p,
            &spec.chain,
            spec.chain_db.as_ref(),
            None,
            Some(&rcm.class_name),
            &main_db,
            &main_table,
            &row_bind,
            model,
            Some(md),
            false,
        )?;
    }
    // pass 3: a join-mediated filter's chain
    if let Some(FilterMapping::FilterMappingJoinMediated(jm)) = &rcm.filter {
        join_chain_emission::emit_join_chain(
            &mut p,
            &jm.joins,
            jm.source_db.as_ref(),
            None,
            Some(&rcm.class_name),
            &main_db,
            &main_table,
            &row_bind,
            model,
            Some(md),
            false,
        )?;
    }

    // ~filter (direct inlined, or anchored on the join chain)
    if rcm.filter.is_some() {
        let src = p.expr.clone();
        p.expr = apply_filter(&src, rcm, &row_bind, &main_db, &main_table, &mut p, model, md)?;
    }

    // ~groupBy, then stage 2: the deferred class-typed Join PMs navigate the GROUPED relation (their source-side
    // reads redirected to the grouped output columns)
    if !rcm.group_by.is_empty() {
        let src = p.expr.clone();
        p.expr = group_by_synthesis::apply_group_by(&src, rcm, &row_bind, &main_table, &mut p, md)?;
        let grouped_names = group_by_synthesis::grouped_key_column_names(rcm, &main_table, md)?;
        for pm in rcm.property_mappings.iter() {
            let PropertyMapping::PropertyMappingJoin(j) = pm else {
                continue;
            };
            if j.joins.len() != 1 {
                return Err(Thrown::not_implemented(cat!(
                    "multi-hop Join PM '",
                    &j.property_name,
                    "' on a ~groupBy class is not supported yet; mapping=",
                    md.qualified_name()
                )));
            }
            join_chain_emission::emit_hops_for_structural_pm(
                &mut p,
                pm,
                &rcm.class_name,
                &main_db,
                &main_table,
                &row_bind,
                model,
                md,
            )?;
            let src = p.expr.clone();
            p.expr = group_by_synthesis::rename_grouped_nav_cond(&src, &grouped_names, &j.property_name, md)?;
        }
    }

    // ~primaryKey is intentionally NOT lowered: object-identity metadata at graph-fetch time in the engine, never a
    // row-level DISTINCT (docs/MAPPING_LEGACY_TO_FUNCTION.md §5.3.6)

    // ~distinct: over the MAPPED columns, never the raw physical row
    if rcm.distinct {
        let mut mapped_cols: LinkedSet<JStr> = LinkedSet::new();
        let mut plain_columns = p.alias_to_target_table.is_empty();
        for pm in rcm.property_mappings.iter() {
            // Java's `&=`: every PM's columns are collected
            plain_columns &= collect_mapped_columns(pm, &mut mapped_cols);
        }
        if plain_columns && !mapped_cols.is_empty() {
            let cols: Vec<Arc<p::ColSpec>> = mapped_cols.iter().map(col_spec).collect();
            let src = p.expr.clone();
            p.expr = platform("select", vec![src, col_spec_array(cols)]);
            let src = p.expr.clone();
            p.expr = platform("distinct", vec![src]);
        } else if !mapped_cols.is_empty() || !p.alias_to_target_table.is_empty() {
            // slot-carrying ~distinct: the mapped main-table columns plus the slot pseudo-columns
            let mut cols: Vec<Arc<p::ColSpec>> = mapped_cols.iter().map(col_spec).collect();
            for (alias, _) in p.alias_to_target_table.iter() {
                cols.push(col_spec(alias));
            }
            let src = p.expr.clone();
            p.expr = platform("distinct", vec![src, col_spec_array(cols)]);
        } else {
            return Err(Thrown::not_implemented(cat!(
                "~distinct on '",
                &rcm.class_name,
                "' maps no main-table column and carries no join slot \u{2014} nothing to dedup by; mapping=",
                md.qualified_name()
            )));
        }
    }

    // the terminal: map(row | ^Class(...))
    let mut table_scope: TableScope = LinkedMap::new();
    table_scope.put(main_table.clone(), VS::Variable(row_bind.clone()));
    seed_alias_scope(&mut table_scope, &mut p, &row_bind, &main_table)?;

    let mut fields = Fields::new();
    for pm in rcm.property_mappings.iter() {
        if p.dropped_routed_props.contains(pm.property_name()) {
            continue; // a dropped route: no binding (loud at demand)
        }
        let cf = translate_pm_to_field(
            pm,
            &row_bind,
            &table_scope,
            &main_table,
            &mut p,
            &rcm.class_name,
            md,
            model,
            !rcm.group_by.is_empty(),
        )?;
        fields.put(
            cf.name,
            p::KeyExpression {
                value: cf.value,
                is_add: false,
                is_local: cf.is_local,
            },
        );
    }
    Ok(RelationalParts {
        pipeline: p.expr,
        row_bind,
        fields,
    })
}

/// `MappingNormalizer.collectMappedColumns` (:1874): the columns one PM reads, into `sink`; whether it is PLAIN (no
/// join, no splice, no fallback).
pub fn collect_mapped_columns(pm: &PropertyMapping, sink: &mut LinkedSet<JStr>) -> bool {
    match pm {
        PropertyMapping::PropertyMappingColumn(c) => {
            sink.add(c.column.clone());
        }
        PropertyMapping::PropertyMappingEnumeratedColumn(ec) => {
            sink.add(ec.column.clone());
        }
        PropertyMapping::PropertyMappingExpression(e) => collect_expr_columns(&e.expression, sink),
        PropertyMapping::PropertyMappingEnumeratedExpression(ee) => collect_expr_columns(&ee.expression, sink),
        PropertyMapping::PropertyMappingLocalProperty(lp) => return collect_mapped_columns(&lp.body, sink),
        PropertyMapping::PropertyMappingEmbedded(emb) => {
            let mut plain = true;
            for sub in emb.property_mappings.iter() {
                plain &= collect_mapped_columns(sub, sink);
            }
            return plain;
        }
        PropertyMapping::PropertyMappingOtherwiseEmbedded(oe) => {
            for sub in oe.embedded.iter() {
                collect_mapped_columns(sub, sink);
            }
            return false;
        }
        PropertyMapping::PropertyMappingJoin(_) => return false,
        PropertyMapping::PropertyMappingJoinTerminalColumn(_) => return false,
        PropertyMapping::PropertyMappingInlineEmbedded(_) => return false,
    }
    true
}

/// `MappingNormalizer.collectExprColumns` (:1910): the columns an expression reads, into `sink`.
pub fn collect_expr_columns(op: &RelationalOperation, sink: &mut LinkedSet<JStr>) {
    match op {
        RelationalOperation::RelationalOperationColumnRef(cr) => {
            sink.add(cr.column.clone());
        }
        RelationalOperation::RelationalOperationFunctionCall(fc) => {
            fc.args.iter().for_each(|a| collect_expr_columns(a, sink));
        }
        RelationalOperation::RelationalOperationComparison(c) => {
            collect_expr_columns(&c.left, sink);
            collect_expr_columns(&c.right, sink);
        }
        RelationalOperation::RelationalOperationBooleanOp(b) => {
            collect_expr_columns(&b.left, sink);
            collect_expr_columns(&b.right, sink);
        }
        RelationalOperation::RelationalOperationIsNull(n) => collect_expr_columns(&n.operand, sink),
        RelationalOperation::RelationalOperationIsNotNull(n) => collect_expr_columns(&n.operand, sink),
        RelationalOperation::RelationalOperationGroup(g) => collect_expr_columns(&g.inner, sink),
        RelationalOperation::RelationalOperationArrayLiteral(a) => {
            a.elements.iter().for_each(|e| collect_expr_columns(e, sink));
        }
        // Java's `default -> op.children().forEach(..)`
        RelationalOperation::RelationalOperationTargetColumnRef(_)
        | RelationalOperation::RelationalOperationLiteral(_)
        | RelationalOperation::RelationalOperationJoinNavigation(_)
        | RelationalOperation::RelationalOperationLambda(_)
        | RelationalOperation::RelationalOperationLambdaParam(_) => {
            op.children().into_iter().for_each(|x| collect_expr_columns(x, sink));
        }
    }
}

/// `MappingNormalizer.propertyPinsOf` (:1940): property -> the set ids its routes pin, in PM order then
/// `propertyTargetSets`' (each id once).
pub fn property_pins_of(rcm: &ClassMappingRelational) -> LinkedMap<JStr, Vec<JStr>> {
    let mut pins: LinkedMap<JStr, Vec<JStr>> = LinkedMap::new();
    for pm in rcm.property_mappings.iter() {
        let body = match pm {
            PropertyMapping::PropertyMappingLocalProperty(lp) => &lp.body,
            _ => pm,
        };
        if let PropertyMapping::PropertyMappingJoin(j) = body
            && let Some(target) = &j.target_set_id
        {
            let l = pins.get_or_insert_with(j.property_name.clone(), Vec::new);
            if !l.contains(target) {
                l.push(target.clone());
            }
        }
        if let PropertyMapping::PropertyMappingOtherwiseEmbedded(oe) = body {
            // a routed Join fallback's pin, else the declared fallback set
            let pin = match &oe.fallback {
                PropertyMapping::PropertyMappingJoin(fj) => {
                    fj.target_set_id.clone().unwrap_or_else(|| oe.fallback_set_id.clone())
                }
                _ => oe.fallback_set_id.clone(),
            };
            let l = pins.get_or_insert_with(oe.property_name.clone(), Vec::new);
            if !l.contains(&pin) {
                l.push(pin);
            }
        }
    }
    for (prop, set_id) in rcm.property_target_sets.iter() {
        let l = pins.get_or_insert_with(prop.clone(), Vec::new);
        if !l.contains(set_id) {
            l.push(set_id.clone());
        }
    }
    pins
}

/// `MappingNormalizer.declaredPrimaryKeyColumns` (:1972): a Relational set's `~primaryKey` column names; empty for
/// any other kind.
pub fn declared_primary_key_columns(cm: &ClassMapping) -> Vec<JStr> {
    let ClassMapping::ClassMappingRelational(rcm) = cm else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for op in rcm.primary_key.iter() {
        if let RelationalOperation::RelationalOperationColumnRef(cr) = op {
            out.push(cr.column.clone());
        }
    }
    out
}

/// `new ColSpec(name, null, null)`: un-aliased, no arguments (so not qualified).
fn col_spec(name: &JStr) -> Arc<p::ColSpec> {
    Arc::new(p::ColSpec {
        name: name.clone(),
        function1: None,
        function2: None,
        alias: None,
        args: empty(),
        qualified: false,
        pos: None,
        col_type: None,
        col_type_mult: None,
        stereotypes: empty(),
        tagged_values: empty(),
    })
}

/// `new ColSpecArray(cols)`.
fn col_spec_array(cols: Vec<Arc<p::ColSpec>>) -> VS {
    VS::ColumnInstance(p::ColumnInstance::ColSpecArray(Arc::new(p::ColSpecArray {
        col_specs: cols.into(),
        pos: None,
    })))
}

/// `Calls.platform(name, args)` as a value.
fn platform(name: &str, args: Vec<VS>) -> VS {
    VS::AppliedFunction(Arc::new(calls::platform(&JStr::from(name), args)))
}

#[cfg(test)]
mod tests {
    use std::collections::{HashMap, HashSet};
    use std::rc::Rc;

    use super::*;
    use crate::lifted_views::LiftedViews;
    use crate::mapping_closures::MappingClosures;
    use crate::test_support::{cm, column_pm, index, join_pm, legacy, model, relational};
    use lite_model::linked::list;
    use lite_model::text::j;
    use lite_model::{
        PropertyMappingEmbedded, PropertyMappingExpression, PropertyMappingLocalProperty,
        PropertyMappingOtherwiseEmbedded, RelationalOperationColumnRef, RelationalOperationFunctionCall,
    };

    fn col(table: &str, column: &str) -> RelationalOperation {
        RelationalOperation::RelationalOperationColumnRef(Arc::new(RelationalOperationColumnRef {
            database_name: None,
            table: j(table),
            column: j(column),
        }))
    }

    #[test]
    fn mapped_columns_are_collected_in_order_and_a_join_is_not_plain() {
        let mut sink = LinkedSet::new();
        assert!(collect_mapped_columns(&column_pm("a", "T", "A"), &mut sink));
        let concat = PropertyMapping::PropertyMappingExpression(Arc::new(PropertyMappingExpression {
            property_name: j("b"),
            expression: RelationalOperation::RelationalOperationFunctionCall(Arc::new(
                RelationalOperationFunctionCall {
                    name: j("concat"),
                    args: list(vec![col("T", "B"), col("T", "A"), col("T", "C")]),
                },
            )),
        }));
        assert!(collect_mapped_columns(&concat, &mut sink));
        assert!(!collect_mapped_columns(&join_pm("j", None), &mut sink));
        let emb = PropertyMapping::PropertyMappingEmbedded(Arc::new(PropertyMappingEmbedded {
            property_name: j("e"),
            property_mappings: list(vec![column_pm("x", "T", "D"), join_pm("k", None)]),
            primary_key: empty(),
        }));
        assert!(!collect_mapped_columns(&emb, &mut sink));
        assert_eq!(sink.to_vec(), vec![j("A"), j("B"), j("C"), j("D")]);
    }

    #[test]
    fn pins_are_the_routed_joins_the_fallbacks_and_the_stamped_targets_each_once() {
        let local = PropertyMapping::PropertyMappingLocalProperty(Arc::new(PropertyMappingLocalProperty {
            property_name: j("loc"),
            r#type: p::TypeExpression::TypeExpressionNameRef(Box::new(p::TypeExpressionNameRef {
                name: j("a::F"),
                pos: None,
            })),
            multiplicity: p::Multiplicity::MultiplicityConcrete(Box::new(p::MultiplicityConcrete {
                lower_bound: 1,
                upper_bound: Some(1),
            })),
            body: join_pm("loc", Some("f2")),
        }));
        let oe = PropertyMapping::PropertyMappingOtherwiseEmbedded(Arc::new(PropertyMappingOtherwiseEmbedded {
            property_name: j("o"),
            embedded: empty(),
            fallback_set_id: j("fb"),
            fallback: join_pm("o", None),
        }));
        let mut rcm = relational(
            "a::P",
            None,
            vec![
                join_pm("firm", Some("f1")),
                join_pm("firm", Some("f1")),
                local,
                oe,
                join_pm("plain", None),
            ],
        );
        rcm.property_target_sets = list(vec![(j("firm"), j("f3")), (j("o"), j("fb"))]);
        let pins = property_pins_of(&rcm);
        let got: Vec<(JStr, Vec<JStr>)> = pins.iter().cloned().collect();
        assert_eq!(
            got,
            vec![
                (j("firm"), vec![j("f1"), j("f3")]),
                (j("loc"), vec![j("f2")]),
                (j("o"), vec![j("fb")]),
            ]
        );
    }

    #[test]
    fn only_a_relational_sets_column_keys_are_declared() {
        let mut rcm = relational("a::P", None, vec![]);
        rcm.primary_key = list(vec![
            col("T", "ID"),
            RelationalOperation::RelationalOperationFunctionCall(Arc::new(RelationalOperationFunctionCall {
                name: j("concat"),
                args: list(vec![col("T", "X")]),
            })),
            col("T", "ID2"),
        ]);
        assert_eq!(declared_primary_key_columns(&cm(rcm)), vec![j("ID"), j("ID2")]);
        let union = crate::test_support::union("a::P", None, &["x"]);
        assert!(declared_primary_key_columns(&union).is_empty());
    }

    #[test]
    fn a_set_with_no_main_table_is_javas_null_pointer() {
        let parsed = model("###Pure\nClass a::P\n{\n  name: String[1];\n}\n");
        let idx = index(&parsed);
        let closures = MappingClosures::new(&idx);
        let views = LiftedViews::new(&parsed, &idx);
        let m = Arc::new(legacy("a::M", vec![]));
        let md = ResolvedMapping::new(
            m.clone(),
            m,
            Rc::new(HashMap::new()),
            Rc::new(LinkedMap::new()),
            closures.closure(&j("a::M")),
            &views,
        );
        let mut ledger = MappingLedger::new(HashSet::new());
        let rcm = Arc::new(relational("a::P", None, vec![column_pm("name", "T", "NAME")]));
        let e = synth_table_backed_mapping(&md, &rcm, &idx, &mut ledger, None).unwrap_err();
        assert_eq!(e, Thrown::null_pointer(Some("relational set without ~mainTable")));
    }
}
