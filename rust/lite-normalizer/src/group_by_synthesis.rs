// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! `GroupBySynthesis` (GroupBySynthesis.java at ad86fa895): a `~groupBy` set's `groupByComputedKeys` body; the key
//! names (`k0__PRODUCT_ID`) reach SQL. Withholding Join PMs and per-row PMs outside the keys, where the engine
//! accepts the mapping (:158-185), is ASK 12's: port as Java has it.
//!
//! RE-EXPRESSED (census §4): the two copies of the key-naming loop (`groupedKeyColumnNames` :43-76 and
//! `applyGroupBy`'s :140-188) are one -- the private `key_names_and_aggregates`, which [`apply_group_by`] and
//! [`grouped_key_column_names`] both call (the copies differ only where `applyGroupBy` throws, and it runs first on
//! the same set: MN:1771 before :1778).

use std::collections::HashSet;
use std::sync::Arc;

use lite_builtin::dyna_fn::DynaFn;
use lite_builtin::dyna_fn_decisions::Resolution;
use lite_builtin::pure::{catalog, lite};
use lite_front::records as p;
use lite_model::linked::LinkedMap;
use lite_model::{
    ClassMappingRelational, JStr, JValue, PropertyMapping, RelationalOperation, RelationalOperationColumnRef,
    RelationalOperationFunctionCall, RelationalOperationJoinNavigation, RelationalOperationLiteral, cat,
};
use lite_names::calls;

use crate::error::Thrown;
use crate::join_chain_emission::{call, col_spec, col_spec_array, lambda, pm_simple_name, property, variable};
use crate::mapping_normalizer;
use crate::pipeline::Pipeline;
use crate::rel_op_translator::{self, TableScope};
use crate::resolved_mapping::ResolvedMapping;

use p::ValueSpecification as V;

/// `GroupBySynthesis.groupedKeyColumnNames` (:43): each grouped main-table key column -> its key name. The one naming
/// loop (module note) is `applyGroupBy`'s, which refuses a PM no key can carry (:163, the mapping named in its text:
/// hence `md`); `groupedKeyColumnNames` skipped it, but runs only after `applyGroupBy` passed on the same set.
pub fn grouped_key_column_names(
    rcm: &ClassMappingRelational,
    main_table: &JStr,
    md: &ResolvedMapping<'_>,
) -> Result<LinkedMap<JStr, JStr>, Thrown> {
    let (key_names, _) = key_names_and_aggregates(rcm, md)?;
    let mut out = LinkedMap::new();
    for (i, key) in rcm.group_by.iter().enumerate() {
        // Java's `cr.table() == null ||` is dead: `ColumnRef`'s constructor refuses a null table
        if let RelationalOperation::RelationalOperationColumnRef(cr) = key
            && cr.table == *main_table
        {
            out.put(cr.column.clone(), key_names[i].clone());
        }
    }
    Ok(out)
}

/// The aggregate PMs of a grouped set, in PM order: each one's property name and its call.
type AggregatePms = Vec<(JStr, Arc<RelationalOperationFunctionCall>)>;

/// The one key-naming loop (`applyGroupBy` :140-188; the module's note): each key's name -- the claiming PM's
/// property name, else `k<i>__<base>` / `k<i>` -- and the aggregate PMs (their property names and calls) in PM order.
/// A PM is an aggregate, else a Join (withheld), else a key expression; one that is none of them is refused (:163); a
/// per-row PM no unclaimed key matches is withheld (:176-185).
fn key_names_and_aggregates(
    rcm: &ClassMappingRelational,
    md: &ResolvedMapping<'_>,
) -> Result<(Vec<JStr>, AggregatePms), Thrown> {
    let key_ops = &rcm.group_by;
    let mut key_names: Vec<JStr> = key_ops
        .iter()
        .enumerate()
        .map(|(i, k)| match key_base_name(k) {
            Some(base) => cat!("k", i.to_string(), "__", &base),
            None => cat!("k", i.to_string()),
        })
        .collect();
    // Walk PMs once: align non-agg PMs to keys by structural equality, collect aggregate PMs, reject orphan formulas.
    let mut claimed: HashSet<usize> = HashSet::new();
    let mut agg_pms = Vec::new();
    for pm in rcm.property_mappings.iter() {
        if let Some(fc) = aggregate_call(pm)? {
            agg_pms.push((pm.property_name().clone(), fc));
            continue;
        }
        // A JOIN PM (class-typed navigation) is not part of the grouped ROW here (the engine navigates it inside the
        // grouped select, which lite's relation groupBy cannot carry: S382, ruled the engine's form, held for slice I's
        // lowering, 2026-11-15): WITHHOLD the property instead of sinking the whole class mapping (stage 1, audit-17) --
        // touching it raises the ordinary not-mapped error, loud.
        if let PropertyMapping::PropertyMappingJoin(_) = pm {
            continue;
        }
        let Some(pm_op) = pm_as_relational_op(pm) else {
            return Err(Thrown::not_implemented(cat!(
                "PropertyMapping '",
                pm_simple_name(pm),
                "' for property '",
                pm.property_name(),
                "' is not supported under ~groupBy (only Column, Expression, ",
                "JoinTerminalColumn, and aggregate Expression PMs are allowed). ",
                "Mapping=",
                md.qualified_name()
            )));
        };
        let match_idx = (0..key_ops.len()).find(|i| !claimed.contains(i) && group_by_ops_match(&pm_op, &key_ops[*i]));
        // a PER-ROW PM outside the key list: the engine's compiler ACCEPTS the mapping and its SQL carries the column
        // ungrouped whenever it takes it (a .all() and any query isolating the grouped set, unread too; the database
        // decides whether it takes it: H2 by the data, DuckDB never -- S382, 2026-11-15, the comment said "only the query
        // reading the property fails"); the same stage-1 rule as the Join PM above: WITHHOLD it, keep the set
        let Some(match_idx) = match_idx else { continue };
        claimed.insert(match_idx);
        key_names[match_idx] = pm.property_name().clone();
    }
    Ok((key_names, agg_pms))
}

/// `GroupBySynthesis.renameGroupedNavCond` (:83): a navigation condition over a grouped set's source reads renamed to
/// the key names.
pub fn rename_grouped_nav_cond(
    expr: &p::ValueSpecification,
    grouped_names: &LinkedMap<JStr, JStr>,
    prop_name: &JStr,
    md: &ResolvedMapping<'_>,
) -> Result<p::ValueSpecification, Thrown> {
    let single = match expr {
        V::AppliedFunction(nav) if nav.function.eq_str(lite::LEGACY_NAVIGATE) && nav.parameters.len() == 4 => {
            match &nav.parameters[3] {
                V::LambdaFunction(cond) if cond.parameters.len() == 2 => Some((nav, cond)),
                _ => None,
            }
        }
        _ => None,
    };
    let Some((nav, cond)) = single else {
        return Err(Thrown::not_implemented(cat!(
            "~groupBy Join PM '",
            prop_name,
            "' did not emit a single navigate step; mapping=",
            md.qualified_name()
        )));
    };
    let src_var = &cond.parameters[0].name;
    let mut new_body = Vec::with_capacity(cond.body.len());
    for b in cond.body.iter() {
        new_body.push(rename_source_reads(b, src_var, grouped_names, prop_name, md)?);
    }
    let mut ps: Vec<V> = nav.parameters.to_vec();
    ps[3] = V::LambdaFunction(lambda(cond.parameters.to_vec(), new_body));
    Ok(V::AppliedFunction(Arc::new(nav.with_parameters(ps.into()))))
}

/// `GroupBySynthesis.renameSourceReads` (:105): `src_var`'s column reads renamed to the key names; loud for a read of
/// a column that is no key.
pub fn rename_source_reads(
    v: &p::ValueSpecification,
    src_var: &JStr,
    grouped_names: &LinkedMap<JStr, JStr>,
    prop_name: &JStr,
    md: &ResolvedMapping<'_>,
) -> Result<p::ValueSpecification, Thrown> {
    Ok(renamed_reads(v, src_var, grouped_names, prop_name, md)?.unwrap_or_else(|| v.clone()))
}

/// [`rename_source_reads`]' walk, `None` where Java answers the very object `v` (its default arm's `mapChildren`
/// found every child unchanged): the rebuild Java's `mapChildren` makes when a child is a new object drops what its
/// constructors drop (lite-front's `spec_children`), so which nodes are new is part of the answer. Java's two
/// `AppliedProperty` arms always make a new node (the two-argument constructor: no position, no owner class), and its
/// `AppliedFunction` arm always does (`withParameters`).
fn renamed_reads(
    v: &V,
    src_var: &JStr,
    grouped_names: &LinkedMap<JStr, JStr>,
    prop_name: &JStr,
    md: &ResolvedMapping<'_>,
) -> Result<Option<V>, Thrown> {
    if let V::AppliedProperty(ap) = v
        && let V::Variable(rv) = &ap.receiver
        && rv.name == *src_var
    {
        let Some(renamed) = grouped_names.get(&ap.property) else {
            return Err(Thrown::not_implemented(cat!(
                "~groupBy Join PM '",
                prop_name,
                "' joins on column '",
                &ap.property,
                "', which is not a ~groupBy key \u{2014} a grouped set",
                " cannot be navigated on a non-key; mapping=",
                md.qualified_name()
            )));
        };
        return Ok(Some(property(V::Variable(rv.clone()), renamed)));
    }
    match v {
        V::AppliedFunction(af) => {
            let mut ps = Vec::with_capacity(af.parameters.len());
            for x in af.parameters.iter() {
                ps.push(rename_source_reads(x, src_var, grouped_names, prop_name, md)?);
            }
            Ok(Some(V::AppliedFunction(Arc::new(af.with_parameters(ps.into())))))
        }
        V::AppliedProperty(ap2) => {
            let receiver = rename_source_reads(&ap2.receiver, src_var, grouped_names, prop_name, md)?;
            Ok(Some(property(receiver, &ap2.property)))
        }
        _ => v.try_map_children(|x| renamed_reads(x, src_var, grouped_names, prop_name, md)),
    }
}

/// `GroupBySynthesis.applyGroupBy` (:132): `source` grouped by the set's keys (`groupByComputedKeys`), the aggregate
/// PMs as its aggregates.
pub fn apply_group_by(
    source: &p::ValueSpecification,
    rcm: &ClassMappingRelational,
    row_bind: &Arc<p::Variable>,
    main_table: &JStr,
    p: &mut Pipeline<'_>,
    md: &ResolvedMapping<'_>,
) -> Result<p::ValueSpecification, Thrown> {
    let mut scope = TableScope::new();
    scope.put(main_table.clone(), V::Variable(row_bind.clone()));
    mapping_normalizer::seed_alias_scope(&mut scope, p, row_bind, main_table)?;

    let (key_names, agg_pms) = key_names_and_aggregates(rcm, md)?;
    // Build key ColSpecs.
    let mut key_cols = Vec::with_capacity(rcm.group_by.len());
    for (i, key) in rcm.group_by.iter().enumerate() {
        let key_value = rel_op_translator::translate(key, &scope, None, Some(row_bind), p.view())?;
        key_cols.push(col_spec(
            &key_names[i],
            Some(lambda(vec![row_bind.clone()], vec![key_value])),
            None,
            None,
        ));
    }
    // Build aggregate ColSpecs with fn1 (selector) + fn2 (aggregate).
    let mut agg_cols = Vec::with_capacity(agg_pms.len());
    for (name, fc) in &agg_pms {
        if fc.args.len() != 1 {
            return Err(Thrown::not_implemented(cat!(
                "Aggregate PM '",
                name,
                "' uses '",
                &fc.name,
                "' with ",
                fc.args.len().to_string(),
                " args; only single-argument ",
                "aggregates lift to the two-stage AggColSpec form. Mapping=",
                md.qualified_name()
            )));
        }
        let selector = rel_op_translator::translate(&fc.args[0], &scope, None, Some(row_bind), p.view())?;
        let vals = variable("vals");
        // the reducer is the engine dynafunction's pure passthrough
        let agg_body = call(calls::platform(
            &rel_op_translator::dyna_fn_name(fc)?,
            vec![V::Variable(vals.clone())],
        ));
        agg_cols.push(col_spec(
            name,
            Some(lambda(vec![row_bind.clone()], vec![selector])),
            Some(lambda(vec![vals], vec![agg_body])),
            None,
        ));
    }
    // the mapped TABLE's rows are grouped by key EXPRESSIONS: the internal-desugar identity
    // (Pure.Lite.GROUP_BY_COMPUTED_KEYS -- upstream's relation groupBy takes bare column names; batch 5 leg 5d)
    Ok(call(calls::exact(
        &JStr::from(lite::GROUP_BY_COMPUTED_KEYS),
        vec![source.clone(), col_spec_array(key_cols), col_spec_array(agg_cols)],
    )))
}

/// `GroupBySynthesis.isGroupReducer` (:238): whether a one-argument call is an aggregate (a PURE name whose catalog
/// overload takes a collection; `Pure.nativeFunctionsAt` is lite-builtin's fallible `native_functions_at`).
pub fn is_group_reducer(fc: &RelationalOperationFunctionCall) -> Result<bool, Thrown> {
    if fc.args.len() != 1 {
        return Ok(false);
    }
    // the registry's names are ASCII: a lone surrogate's U+FFFD matches none, as the unit-exact name would not
    let d = DynaFn::of(&fc.name.lossy());
    let Some(d) = d.filter(|d| d.resolution() == Resolution::PURE) else {
        return Ok(false);
    };
    for fqn in d.fqns() {
        // `Pure.nativeFunctionsAt`'s refusal of a bare name: its IllegalArgumentException (Pure.java:854)
        let natives = catalog()
            .native_functions_at(&JStr::from(fqn.as_str()))
            .map_err(|m| Thrown::illegal_argument(JStr::from(m)))?;
        for native in natives {
            if let Some(first) = native.def().parameters.first()
                && is_collection(&first.multiplicity)
            {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

/// `GroupBySynthesis.isCollection` (:257): whether a multiplicity's upper bound exceeds one.
pub fn is_collection(m: &p::Multiplicity) -> bool {
    match m {
        p::Multiplicity::MultiplicityConcrete(c) => c.upper_bound.is_none_or(|u| u > 1),
        p::Multiplicity::MultiplicityParameter(_) => true,
    }
}

/// `GroupBySynthesis.isAggregatePm` (:263): whether a PM's expression is an aggregate.
pub fn is_aggregate_pm(pm: &PropertyMapping) -> Result<bool, Thrown> {
    Ok(aggregate_call(pm)?.is_some())
}

/// [`is_aggregate_pm`] with the aggregate's call in hand (Java's `applyGroupBy` casts the PM back to it, :200-201).
fn aggregate_call(pm: &PropertyMapping) -> Result<Option<Arc<RelationalOperationFunctionCall>>, Thrown> {
    if let PropertyMapping::PropertyMappingExpression(expr) = pm
        && let RelationalOperation::RelationalOperationFunctionCall(fc) = &expr.expression
        && is_group_reducer(fc)?
    {
        return Ok(Some(fc.clone()));
    }
    Ok(None)
}

/// `GroupBySynthesis.pmAsRelationalOp` (:271): a PM's relational expression, if it has one.
pub fn pm_as_relational_op(pm: &PropertyMapping) -> Option<RelationalOperation> {
    match pm {
        PropertyMapping::PropertyMappingColumn(col) => Some(RelationalOperation::RelationalOperationColumnRef(
            Arc::new(RelationalOperationColumnRef {
                database_name: Some(col.database.clone()),
                table: col.table.clone(),
                column: col.column.clone(),
            }),
        )),
        PropertyMapping::PropertyMappingExpression(expr) => Some(expr.expression.clone()),
        PropertyMapping::PropertyMappingJoinTerminalColumn(jtc) => Some(
            RelationalOperation::RelationalOperationJoinNavigation(Arc::new(RelationalOperationJoinNavigation {
                database_name: Some(jtc.database.clone()),
                chain: jtc.joins.clone(),
                terminal: Some(jtc.terminal_column.clone()),
            })),
        ),
        _ => None,
    }
}

/// `GroupBySynthesis.groupByOpsMatch` (:290): whether two key expressions are one (structurally, table-insensitive).
/// Java's `Objects.equals` on two records is [`java_equals`]: the derived equality but for a `Double` literal, which
/// Java's `Double.equals` compares by its bits (`0.0` and `-0.0` two values, every `NaN` one) -- 2026-11-15, the
/// normalizer batch, item 10: it was the derived `==`.
pub fn group_by_ops_match(a: &RelationalOperation, b: &RelationalOperation) -> bool {
    if let (
        RelationalOperation::RelationalOperationColumnRef(ca),
        RelationalOperation::RelationalOperationColumnRef(cb),
    ) = (a, b)
    {
        // The qualifier is ignored only when ONE side lacks it (the view-rewrite stamps the mapping's db onto PMs while
        // view keys parse unqualified); two EXPLICIT different dbs never match (same-named tables across included dbs).
        let db_ok = ca.database_name.is_none() || cb.database_name.is_none() || ca.database_name == cb.database_name;
        return db_ok && ca.table == cb.table && ca.column == cb.column;
    }
    java_equals(a, b)
}

/// Java's record equality of two relational operations (`Objects.equals`): the derived `==` over each operation with
/// every `Double` literal replaced by its `Double.doubleToLongBits` (a `NaN` its canonical bits), so `0.0` and `-0.0`
/// differ and two `NaN`s are equal, as `Double.equals` has them (the normalizer batch, item 10).
fn java_equals(a: &RelationalOperation, b: &RelationalOperation) -> bool {
    canonical_doubles(a) == canonical_doubles(b)
}

/// `op` with each `Double` literal spelled by its bits (a marker no other literal holds: a `BigInteger` reads digits).
fn canonical_doubles(op: &RelationalOperation) -> RelationalOperation {
    if let RelationalOperation::RelationalOperationLiteral(l) = op
        && let JValue::Double(d) = l.value
    {
        let bits = if d.is_nan() {
            0x7ff8_0000_0000_0000_u64
        } else {
            d.to_bits()
        };
        return RelationalOperation::RelationalOperationLiteral(Arc::new(RelationalOperationLiteral {
            value: JValue::BigInteger(format!("double-bits:{bits:016x}")),
        }));
    }
    op.map_children(|c| Some(canonical_doubles(c)))
        .unwrap_or_else(|| op.clone())
}

/// `GroupBySynthesis.keyBaseName` (:305): a key expression's base name (its column's), else none.
pub fn key_base_name(op: &RelationalOperation) -> Option<JStr> {
    match op {
        RelationalOperation::RelationalOperationColumnRef(cr) => Some(cr.column.clone()),
        RelationalOperation::RelationalOperationTargetColumnRef(tr) => Some(tr.column.clone()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::rc::Rc;

    use lite_model::text::j;
    use lite_model::{
        ClassMapping, LegacyMappingDefinition, PropertyMappingEmbedded, PropertyMappingInlineEmbedded,
        RelationalOperationLiteral,
    };

    use super::*;
    use crate::lifted_views::LiftedViews;
    use crate::mapping_closures::MappingClosures;
    use crate::test_support::{index, mapping, model};

    const MODEL: &str = r#"
###Pure
Class a::Sale
{
  id: Integer[1];
  region: String[1];
  other: String[1];
  total: Float[1];
}

###Relational
Database a::db
(
  Table SALES (PRODUCT_ID INTEGER, REGION VARCHAR(10), AMOUNT FLOAT, OTHER VARCHAR(10))
)

###Mapping
Mapping a::M
(
  a::Sale[s]: Relational
  {
    ~groupBy([a::db]SALES.PRODUCT_ID, [a::db]SALES.REGION, concat([a::db]SALES.REGION, 'x'))
    ~mainTable [a::db]SALES
    total: sum([a::db]SALES.AMOUNT),
    id: [a::db]SALES.PRODUCT_ID,
    other: [a::db]SALES.OTHER
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

    fn the_set(md: &ResolvedMapping<'_>) -> Arc<ClassMappingRelational> {
        match md.set(Some(&j("s"))).unwrap() {
            Some(ClassMapping::ClassMappingRelational(r)) => r,
            other => panic!("the set: {other:?}"),
        }
    }

    fn column(db: Option<&str>, table: &str, col: &str) -> RelationalOperation {
        RelationalOperation::RelationalOperationColumnRef(Arc::new(RelationalOperationColumnRef {
            database_name: db.map(j),
            table: j(table),
            column: j(col),
        }))
    }

    fn fcall(name: &str, args: Vec<RelationalOperation>) -> RelationalOperationFunctionCall {
        RelationalOperationFunctionCall {
            name: j(name),
            args: args.into(),
        }
    }

    fn var(name: &str) -> V {
        V::Variable(variable(name))
    }

    #[test]
    fn a_key_takes_its_claiming_pms_name_else_its_index_and_column() {
        let parsed = model(MODEL);
        let idx = index(&parsed);
        let closures = MappingClosures::new(&idx);
        let views = LiftedViews::new(&parsed, &idx);
        let md = resolved(mapping(&parsed, "a::M"), &closures, &views);
        let rcm = the_set(&md);
        // `id` claims key 0; `other` matches no key (withheld); the computed key 2 is no column: not navigable
        let names = grouped_key_column_names(&rcm, &j("SALES"), &md).unwrap();
        let entries: Vec<(JStr, JStr)> = names.iter().cloned().collect();
        assert_eq!(
            entries,
            vec![(j("PRODUCT_ID"), j("id")), (j("REGION"), j("k1__REGION"))]
        );
        // another main table's keys are not this set's
        assert!(grouped_key_column_names(&rcm, &j("OTHER"), &md).unwrap().is_empty());
        let (key_names, aggs) = key_names_and_aggregates(&rcm, &md).unwrap();
        assert_eq!(key_names, vec![j("id"), j("k1__REGION"), j("k2")]);
        assert_eq!(aggs.len(), 1);
        assert_eq!(aggs[0].0, j("total"));
        assert!(is_aggregate_pm(&rcm.property_mappings[0]).unwrap());
        assert!(!is_aggregate_pm(&rcm.property_mappings[1]).unwrap());
    }

    #[test]
    fn a_pm_no_key_can_carry_is_refused_naming_its_kind() {
        let parsed = model(MODEL);
        let idx = index(&parsed);
        let closures = MappingClosures::new(&idx);
        let views = LiftedViews::new(&parsed, &idx);
        let md = resolved(mapping(&parsed, "a::M"), &closures, &views);
        let mut rcm = (*the_set(&md)).clone();
        let mut pms = rcm.property_mappings.to_vec();
        pms.push(PropertyMapping::PropertyMappingInlineEmbedded(Arc::new(
            PropertyMappingInlineEmbedded {
                property_name: j("inl"),
                set_id: j("x"),
            },
        )));
        pms.push(PropertyMapping::PropertyMappingEmbedded(Arc::new(
            PropertyMappingEmbedded {
                property_name: j("emb"),
                property_mappings: Arc::default(),
                primary_key: Arc::default(),
            },
        )));
        rcm.property_mappings = pms.into();
        let e = grouped_key_column_names(&rcm, &j("SALES"), &md).unwrap_err();
        assert!(e.is_not_implemented());
        assert_eq!(
            e.message(),
            Some(j(
                "PropertyMapping 'InlineEmbedded' for property 'inl' is not supported under ~groupBy (only Column, \
                 Expression, JoinTerminalColumn, and aggregate Expression PMs are allowed). Mapping=a::M"
            ))
        );
    }

    #[test]
    fn the_key_matches_ignore_one_missing_database_qualifier() {
        let qualified = column(Some("a::db"), "T", "C");
        let bare = column(None, "T", "C");
        assert!(group_by_ops_match(&qualified, &bare));
        assert!(group_by_ops_match(&bare, &qualified));
        assert!(!group_by_ops_match(&qualified, &column(Some("b::db"), "T", "C")));
        assert!(!group_by_ops_match(&bare, &column(None, "U", "C")));
        let call = RelationalOperation::RelationalOperationFunctionCall(Arc::new(fcall("upper", vec![bare.clone()])));
        assert!(group_by_ops_match(&call, &call.clone()));
        // structurally, beyond a column: the qualifier counts there
        let call_q = RelationalOperation::RelationalOperationFunctionCall(Arc::new(fcall("upper", vec![qualified])));
        assert!(!group_by_ops_match(&call, &call_q));
        assert_eq!(key_base_name(&bare), Some(j("C")));
        assert_eq!(key_base_name(&call), None);
    }

    #[test]
    fn a_reducer_is_a_pure_dynafunction_over_a_collection() {
        let one = vec![column(None, "T", "C")];
        assert!(is_group_reducer(&fcall("sum", one.clone())).unwrap());
        assert!(is_group_reducer(&fcall("count", one.clone())).unwrap());
        assert!(!is_group_reducer(&fcall("sum", vec![column(None, "T", "C"), column(None, "T", "D")])).unwrap());
        assert!(!is_group_reducer(&fcall("upper", one.clone())).unwrap());
        assert!(!is_group_reducer(&fcall("noSuchDyna", one)).unwrap());
        let concrete = |upper: Option<i32>| {
            p::Multiplicity::MultiplicityConcrete(Box::new(p::MultiplicityConcrete {
                lower_bound: 0,
                upper_bound: upper,
            }))
        };
        assert!(is_collection(&concrete(None)));
        assert!(is_collection(&concrete(Some(2))));
        assert!(!is_collection(&concrete(Some(1))));
        let literal = PropertyMapping::PropertyMappingExpression(Arc::new(lite_model::PropertyMappingExpression {
            property_name: j("x"),
            expression: RelationalOperation::RelationalOperationLiteral(Arc::new(RelationalOperationLiteral {
                value: lite_model::JValue::Long(1),
            })),
        }));
        assert!(!is_aggregate_pm(&literal).unwrap());
        assert_eq!(
            pm_as_relational_op(&literal),
            Some(RelationalOperation::RelationalOperationLiteral(Arc::new(
                RelationalOperationLiteral {
                    value: lite_model::JValue::Long(1),
                }
            )))
        );
    }

    #[test]
    fn a_grouped_navigates_source_reads_are_renamed_to_the_keys() {
        let parsed = model(MODEL);
        let idx = index(&parsed);
        let closures = MappingClosures::new(&idx);
        let views = LiftedViews::new(&parsed, &idx);
        let md = resolved(mapping(&parsed, "a::M"), &closures, &views);
        let names = LinkedMap::of([(j("PRODUCT_ID"), j("id"))]);
        let (s, t) = (variable("s"), variable("t"));
        let cond = |col_name: &str| {
            let read = property(V::Variable(s.clone()), &j(col_name));
            let body = call(calls::platform(
                &j("equal"),
                vec![read, property(V::Variable(t.clone()), &j("ID"))],
            ));
            V::LambdaFunction(lambda(vec![s.clone(), t.clone()], vec![body]))
        };
        let navigate = |c: V| {
            call(calls::exact(
                &JStr::from(lite::LEGACY_NAVIGATE),
                vec![var("rel"), var("slot"), var("rows"), c],
            ))
        };
        let renamed = rename_grouped_nav_cond(&navigate(cond("PRODUCT_ID")), &names, &j("firm"), &md).unwrap();
        assert_eq!(renamed, navigate(cond("id")));
        let e = rename_grouped_nav_cond(&navigate(cond("REGION")), &names, &j("firm"), &md).unwrap_err();
        assert!(e.is_not_implemented());
        assert_eq!(
            e.message(),
            Some(j(
                "~groupBy Join PM 'firm' joins on column 'REGION', which is not a ~groupBy key \u{2014} a grouped set \
                 cannot be navigated on a non-key; mapping=a::M"
            ))
        );
        // a routed (three-argument) navigate is no single navigate step
        let routed = call(calls::exact(
            &JStr::from(lite::LEGACY_NAVIGATE),
            vec![var("rel"), var("slot"), var("routes")],
        ));
        let e = rename_grouped_nav_cond(&routed, &names, &j("firm"), &md).unwrap_err();
        assert_eq!(
            e.message(),
            Some(j(
                "~groupBy Join PM 'firm' did not emit a single navigate step; mapping=a::M"
            ))
        );
        // a read of another variable, and a leaf, are the very values
        let other = property(var("t"), &j("REGION"));
        assert_eq!(
            rename_source_reads(&other, &j("s"), &names, &j("firm"), &md).unwrap(),
            other
        );
        assert!(
            renamed_reads(&var("s"), &j("s"), &names, &j("firm"), &md)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn a_double_literal_matches_by_javas_double_equals() {
        // GBS:342's Objects.equals: a Double literal by Double.equals -- 0.0 and -0.0 two values, every NaN one (2026-11-15,
        // the normalizer batch, item 10: the derived `==` had both the other way round)
        let lit = |d: f64| {
            RelationalOperation::RelationalOperationLiteral(Arc::new(RelationalOperationLiteral {
                value: lite_model::JValue::Double(d),
            }))
        };
        let call = |a: RelationalOperation| {
            RelationalOperation::RelationalOperationFunctionCall(Arc::new(RelationalOperationFunctionCall {
                name: j("plus"),
                args: Arc::from(vec![a]),
            }))
        };
        assert!(group_by_ops_match(&lit(1.5), &lit(1.5)));
        assert!(!group_by_ops_match(&lit(1.5), &lit(2.5)));
        assert!(!group_by_ops_match(&lit(0.0), &lit(-0.0)));
        assert!(group_by_ops_match(&lit(f64::NAN), &lit(f64::NAN)));
        assert!(group_by_ops_match(&call(lit(f64::NAN)), &call(lit(f64::NAN))));
        assert!(!group_by_ops_match(&call(lit(0.0)), &call(lit(-0.0))));
    }
}
