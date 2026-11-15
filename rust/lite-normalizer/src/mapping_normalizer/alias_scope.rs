// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! MappingNormalizer family 18 (EF_CENSUS §1.1): the alias scope (which row a bare column reads: the first alias per
//! table wins; a table two sub-row slots reach is marked ambiguous), view references inside join conditions, and a
//! join's target table.

use std::collections::HashSet;
use std::sync::Arc;

use lite_error::Phase;
use lite_front::records::ValueSpecification as VS;
use lite_model::linked::LinkedMap;
use lite_model::{
    JStr, RelationalOperation as Op, RelationalOperationBooleanOp, RelationalOperationComparison,
    RelationalOperationFunctionCall, RelationalOperationGroup, RelationalOperationIsNotNull,
    RelationalOperationIsNull, cat,
};
use lite_names::ModelIndex;
use lite_names::model_index::java_list;

use crate::error::{Thrown, require};
use crate::jdk::LinkedSet;
use crate::pipeline::Pipeline;
use crate::rel_op_translator::{self, TableScope};
use crate::resolved_mapping::ResolvedMapping;
use crate::view_relation;

/// `MappingNormalizer.seedAliasScope` (:2504): each non-class slot's target table bound to `row.<alias>` in `scope`
/// (first alias wins), a non-main table reached by two slots added to `p.ambiguous_tables` instead (Java's
/// `requireNonNull` on a per-table count: a `Result`).
pub fn seed_alias_scope(
    scope: &mut TableScope,
    p: &mut Pipeline<'_>,
    row_bind: &Arc<lite_front::records::Variable>,
    main_table: &JStr,
) -> Result<(), Thrown> {
    // physical (non-class) sub-rows per target table
    let mut per_table: LinkedMap<JStr, i32> = LinkedMap::new();
    for (alias, table) in p.alias_to_target_table.iter() {
        if p.class_slots.contains(alias) {
            continue;
        }
        *per_table.get_or_insert_with(table.clone(), || 0) += 1;
    }
    for (alias, table) in p.alias_to_target_table.iter() {
        if p.class_slots.contains(alias) {
            continue;
        }
        // a NON-main table reached by more than one physical sub-row is ambiguous for bare column refs: left unbound
        // and recorded, so reads fail loudly (the main table is exempt: a bare ref means the top row)
        if table != main_table && *require(per_table.get(table), None)? > 1 {
            p.ambiguous_tables.insert(table.clone());
            continue;
        }
        scope.put_if_absent(
            table.clone(),
            VS::AppliedProperty(Arc::new(lite_front::records::AppliedProperty {
                receiver: VS::Variable(row_bind.clone()),
                property: alias.clone(),
                pos: None,
                owner_class: None,
            })),
        );
    }
    Ok(())
}

/// `MappingNormalizer.resolveViewRefsInJoin` (:2535, the seven-argument form): [`resolve_view_refs_in_join_keeping`]
/// with no kept target view, one side.
pub fn resolve_view_refs_in_join(
    op: &Op,
    db: &JStr,
    source_table: Option<&JStr>,
    model: &ModelIndex,
    md: Option<&ResolvedMapping<'_>>,
    backing_view: Option<&JStr>,
    only_view: Option<&JStr>,
) -> Result<Op, Thrown> {
    resolve_view_refs_in_join_keeping(op, db, source_table, model, md, backing_view, only_view, None, false)
}

/// `MappingNormalizer.resolveViewRefsInJoin` (:2550, the nine-argument form): a join condition's references to plain
/// views substituted by the views' columns (`any_side`: on either side of the join).
#[allow(clippy::too_many_arguments)]
pub fn resolve_view_refs_in_join_keeping(
    op: &Op,
    db: &JStr,
    source_table: Option<&JStr>,
    model: &ModelIndex,
    md: Option<&ResolvedMapping<'_>>,
    backing_view: Option<&JStr>,
    only_view: Option<&JStr>,
    keep_target_view: Option<&JStr>,
    any_side: bool,
) -> Result<Op, Thrown> {
    let rec = |x: &Op| {
        resolve_view_refs_in_join_keeping(
            x,
            db,
            source_table,
            model,
            md,
            backing_view,
            only_view,
            keep_target_view,
            any_side,
        )
    };
    match op {
        Op::RelationalOperationColumnRef(cr) => {
            if let Some(only) = only_view
                && cr.table != *only
            {
                return Ok(op.clone());
            }
            let cr_db = cr.database_name.as_ref().unwrap_or(db);
            let Some(view) = model.find_view(cr_db, &cr.table) else {
                return Ok(op.clone());
            };
            // the view's physical root, asked before either side test (it is loud for a view with no main table)
            let phys = model.view_main_table(cr_db, view)?;
            let keep = if any_side {
                Some(&cr.table) == backing_view
                    || Some(&cr.table) == source_table
                    || Some(&cr.table) == keep_target_view
            } else {
                !view_chain_reaches(&phys, source_table, cr_db, md, model)?
            };
            if keep {
                return Ok(op.clone());
            }
            if (view.filter.is_some() || !view.group_by_columns.is_empty() || view.distinct)
                && Some(&cr.table) != backing_view
                && only_view.is_none()
            {
                // substituting the column alone would drop the view's row semantics (filter/distinct/groupBy);
                // exempt: the class's own backing view (its pipeline applies them)
                let semantics = if view.filter.is_some() {
                    "~filter"
                } else if view.distinct {
                    "~distinct"
                } else {
                    "~groupBy"
                };
                return Err(Thrown::not_implemented(cat!(
                    "Join references view '",
                    &cr.table,
                    "' with ",
                    semantics,
                    " semantics as its source side; joins over non-plain views are a roadmap feature. ",
                    view_relation::owner(md, db)
                )));
            }
            for vc in view.column_mappings.iter() {
                if vc.name == cr.column {
                    // a view-on-view column re-resolves (against `db`) so the chain flattens to the physical root;
                    // pass 1 (`only_view`) stays one layer
                    return if only_view.is_none() {
                        resolve_view_refs_in_join_keeping(
                            &vc.expression,
                            db,
                            source_table,
                            model,
                            md,
                            backing_view,
                            None,
                            keep_target_view,
                            any_side,
                        )
                    } else {
                        Ok(vc.expression.clone())
                    };
                }
            }
            Ok(op.clone())
        }
        Op::RelationalOperationComparison(c) => Ok(Op::RelationalOperationComparison(Arc::new(
            RelationalOperationComparison {
                left: rec(&c.left)?,
                op: c.op,
                right: rec(&c.right)?,
            },
        ))),
        Op::RelationalOperationBooleanOp(b) => Ok(Op::RelationalOperationBooleanOp(Arc::new(
            RelationalOperationBooleanOp {
                left: rec(&b.left)?,
                op: b.op,
                right: rec(&b.right)?,
            },
        ))),
        Op::RelationalOperationGroup(g) => Ok(Op::RelationalOperationGroup(Arc::new(RelationalOperationGroup {
            inner: rec(&g.inner)?,
        }))),
        Op::RelationalOperationIsNull(n) => Ok(Op::RelationalOperationIsNull(Arc::new(RelationalOperationIsNull {
            operand: rec(&n.operand)?,
        }))),
        Op::RelationalOperationIsNotNull(n) => Ok(Op::RelationalOperationIsNotNull(Arc::new(
            RelationalOperationIsNotNull {
                operand: rec(&n.operand)?,
            },
        ))),
        Op::RelationalOperationFunctionCall(f) => {
            let args = f.args.iter().map(rec).collect::<Result<Vec<Op>, Thrown>>()?;
            Ok(Op::RelationalOperationFunctionCall(Arc::new(RelationalOperationFunctionCall {
                name: f.name.clone(),
                args: args.into(),
            })))
        }
        // `op.mapChildren(..)`: only the identity of an unchanged node differs, and nothing reads it
        Op::RelationalOperationTargetColumnRef(_)
        | Op::RelationalOperationLiteral(_)
        | Op::RelationalOperationArrayLiteral(_)
        | Op::RelationalOperationJoinNavigation(_)
        | Op::RelationalOperationLambda(_)
        | Op::RelationalOperationLambdaParam(_) => Ok(op
            .try_map_children(|x| rec(x).map(Some))?
            .unwrap_or_else(|| op.clone())),
    }
}

/// `MappingNormalizer.viewChainReaches` (:2637): whether the view chain from `start` (each view's main table) reaches
/// `source_table`.
pub fn view_chain_reaches(
    start: &JStr,
    source_table: Option<&JStr>,
    db: &JStr,
    _md: Option<&ResolvedMapping<'_>>,
    model: &ModelIndex,
) -> Result<bool, Thrown> {
    let mut walk = start.clone();
    let mut seen: HashSet<JStr> = HashSet::new();
    while seen.insert(walk.clone()) {
        if Some(&walk) == source_table {
            return Ok(true);
        }
        let Some(v) = model.find_view(db, &walk) else {
            return Ok(false);
        };
        walk = model.view_main_table(db, v)?;
    }
    Ok(false)
}

/// `MappingNormalizer.requireNonViewTarget` (:2663): loud when a join's target is a view the frame cannot reach.
pub fn require_non_view_target(
    target_table: &JStr,
    db: &JStr,
    join_name: &JStr,
    model: &ModelIndex,
    md: &ResolvedMapping<'_>,
) -> Result<(), Thrown> {
    if model.find_view(db, target_table).is_some() {
        return Err(Thrown::not_implemented(cat!(
            "Join '",
            join_name,
            "' targets view '",
            target_table,
            "'; views as JOIN TARGETS are a roadmap feature (the view must expand as a relation at the join hop). \
             mapping=",
            md.qualified_name()
        )));
    }
    Ok(())
}

/// `MappingNormalizer.determineTargetTable` (:2674): the one non-source table a join condition reads (the source
/// table for a `{target}` marker); loud for none or several.
pub fn determine_target_table(
    cond: &Op,
    source_table: Option<&JStr>,
    join_name: &JStr,
    owner_label: Option<&JStr>,
    hop_index: i32,
    mapping_fqn: &JStr,
) -> Result<JStr, Thrown> {
    if contains_target_column_ref(cond)? {
        return require(
            source_table.cloned(),
            Some("target-column-ref join condition with unknown source table"),
        );
    }
    let mut tables: LinkedSet<JStr> = LinkedSet::new();
    rel_op_translator::collect_tables_in(cond, &mut tables)?;
    if let Some(source) = source_table {
        tables.remove(source);
    }
    if let (1, Some(only)) = (tables.len(), tables.iter().next()) {
        return Ok(only.clone());
    }
    if tables.is_empty() {
        return Err(Thrown::model(
            Phase::Normalize,
            cat!(
                "Join '",
                join_name,
                "' references no table other than source '",
                or_null(source_table),
                "' and has no {target} marker; owner=",
                or_null(owner_label),
                ", hop ",
                hop_index.to_string(),
                ", mapping=",
                mapping_fqn
            ),
            None,
        ));
    }
    Err(Thrown::not_implemented(cat!(
        "Join '",
        join_name,
        "' references multiple non-source tables ",
        java_list(&tables.to_vec()),
        "; multi-table joins not supported. owner=",
        or_null(owner_label),
        ", hop ",
        hop_index.to_string(),
        ", mapping=",
        mapping_fqn
    )))
}

/// `MappingNormalizer.containsTargetColumnRef` (:2697): whether the condition reads a `{target}` column.
pub fn contains_target_column_ref(op: &Op) -> Result<bool, Thrown> {
    // Java's `||` and `anyMatch` stop at the first true: a join navigation after it is never reached
    Ok(match op {
        Op::RelationalOperationTargetColumnRef(_) => true,
        Op::RelationalOperationColumnRef(_) => false,
        Op::RelationalOperationLiteral(_) => false,
        Op::RelationalOperationFunctionCall(fc) => any_target_column_ref(&fc.args)?,
        Op::RelationalOperationComparison(c) => {
            contains_target_column_ref(&c.left)? || contains_target_column_ref(&c.right)?
        }
        Op::RelationalOperationBooleanOp(b) => {
            contains_target_column_ref(&b.left)? || contains_target_column_ref(&b.right)?
        }
        Op::RelationalOperationIsNull(n) => contains_target_column_ref(&n.operand)?,
        Op::RelationalOperationIsNotNull(n) => contains_target_column_ref(&n.operand)?,
        Op::RelationalOperationGroup(g) => contains_target_column_ref(&g.inner)?,
        Op::RelationalOperationArrayLiteral(a) => any_target_column_ref(&a.elements)?,
        Op::RelationalOperationLambda(lam) => contains_target_column_ref(&lam.body)?,
        Op::RelationalOperationLambdaParam(_) => false,
        Op::RelationalOperationJoinNavigation(_) => {
            return Err(Thrown::model(
                Phase::Normalize,
                JStr::from("JoinNavigation inside join condition"),
                None,
            ));
        }
    })
}

/// `stream().anyMatch(MappingNormalizer::containsTargetColumnRef)`.
fn any_target_column_ref(ops: &[Op]) -> Result<bool, Thrown> {
    for o in ops {
        if contains_target_column_ref(o)? {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Java's `"" + s` for a nullable string.
fn or_null(s: Option<&JStr>) -> JStr {
    s.cloned().unwrap_or_else(|| JStr::from("null"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lifted_views::LiftedViews;
    use crate::mapping_ledger::MappingLedger;
    use crate::test_support::{index, model};
    use lite_model::text::j;
    use lite_model::{
        RelationalOperationColumnRef, RelationalOperationJoinNavigation, RelationalOperationTargetColumnRef,
    };

    const MODEL: &str = r#"
###Relational
Database a::db
(
  Table T (ID INTEGER PRIMARY KEY, NAME VARCHAR(20))
  Table U (ID INTEGER PRIMARY KEY, T_ID INTEGER)
  View V (vid: T.ID, vname: T.NAME)
  View VV (vvid: V.vid)
)
"#;

    fn col(table: &str, column: &str) -> Op {
        Op::RelationalOperationColumnRef(Arc::new(RelationalOperationColumnRef {
            database_name: None,
            table: j(table),
            column: j(column),
        }))
    }

    fn eq(l: Op, r: Op) -> Op {
        Op::RelationalOperationComparison(Arc::new(RelationalOperationComparison {
            left: l,
            op: lite_model::ComparisonOp::EQ,
            right: r,
        }))
    }

    fn target(column: &str) -> Op {
        Op::RelationalOperationTargetColumnRef(Arc::new(RelationalOperationTargetColumnRef { column: j(column) }))
    }

    fn join_nav() -> Op {
        Op::RelationalOperationJoinNavigation(Arc::new(RelationalOperationJoinNavigation {
            database_name: None,
            chain: lite_model::linked::empty(),
            terminal: None,
        }))
    }

    #[test]
    fn a_target_marker_stops_at_the_first_true() {
        assert!(contains_target_column_ref(&eq(target("X"), join_nav())).unwrap());
        let e = contains_target_column_ref(&eq(col("T", "ID"), join_nav())).unwrap_err();
        assert!(e.is_model());
        assert_eq!(e.message(), Some(j("JoinNavigation inside join condition")));
        assert!(!contains_target_column_ref(&eq(col("T", "ID"), col("U", "T_ID"))).unwrap());
        // a marker: the source table, loud when there is none
        assert_eq!(
            determine_target_table(&eq(target("X"), col("T", "ID")), Some(&j("T")), &j("J"), None, 0, &j("m")),
            Ok(j("T"))
        );
        assert_eq!(
            determine_target_table(&eq(target("X"), col("T", "ID")), None, &j("J"), None, 0, &j("m")),
            Err(Thrown::null_pointer(Some("target-column-ref join condition with unknown source table")))
        );
    }

    #[test]
    fn a_plain_view_column_is_substituted_through_its_chain() {
        let parsed = model(MODEL);
        let idx = index(&parsed);
        let db = j("a::db");
        // VV.vvid -> V.vid -> T.ID, its root reaching the source table T
        let cond = eq(col("VV", "vvid"), col("U", "T_ID"));
        let out = resolve_view_refs_in_join(&cond, &db, Some(&j("T")), &idx, None, None, None).unwrap();
        assert_eq!(out, eq(col("T", "ID"), col("U", "T_ID")));
        // a source that the view's chain does not reach: verbatim
        let out = resolve_view_refs_in_join(&cond, &db, Some(&j("U")), &idx, None, None, None).unwrap();
        assert_eq!(out, cond);
        // pass 1: only the backing view, one layer
        let out = resolve_view_refs_in_join(&cond, &db, Some(&j("T")), &idx, None, None, Some(&j("VV"))).unwrap();
        assert_eq!(out, eq(col("V", "vid"), col("U", "T_ID")));
        // either side, the source frame kept verbatim
        let out = resolve_view_refs_in_join_keeping(&cond, &db, Some(&j("VV")), &idx, None, None, None, None, true)
            .unwrap();
        assert_eq!(out, cond);
        assert!(view_chain_reaches(&j("VV"), Some(&j("T")), &db, None, &idx).unwrap());
        assert!(!view_chain_reaches(&j("VV"), Some(&j("U")), &db, None, &idx).unwrap());
    }

    #[test]
    fn a_join_onto_a_view_is_loud() {
        let parsed = model(MODEL);
        let idx = index(&parsed);
        let closures = crate::mapping_closures::MappingClosures::new(&idx);
        let views = LiftedViews::new(&parsed, &idx);
        let m = Arc::new(crate::test_support::legacy("a::M", vec![]));
        let md = ResolvedMapping::new(
            m.clone(),
            m,
            std::rc::Rc::new(std::collections::HashMap::new()),
            std::rc::Rc::new(LinkedMap::new()),
            closures.closure(&j("a::M")),
            &views,
        );
        let db = j("a::db");
        assert!(require_non_view_target(&j("U"), &db, &j("J"), &idx, &md).is_ok());
        let e = require_non_view_target(&j("V"), &db, &j("J"), &idx, &md).unwrap_err();
        assert!(e.is_not_implemented());
        assert_eq!(
            e.message(),
            Some(j(
                "Join 'J' targets view 'V'; views as JOIN TARGETS are a roadmap feature (the view must expand as a \
                 relation at the join hop). mapping=a::M"
            ))
        );
    }

    #[test]
    fn the_first_alias_per_table_binds_and_a_twice_reached_table_is_ambiguous() {
        let parsed = model(MODEL);
        let idx = index(&parsed);
        let views = LiftedViews::new(&parsed, &idx);
        let mut ledger = MappingLedger::new(HashSet::new());
        let expr = VS::Variable(Arc::new(lite_front::records::Variable {
            name: j("src"),
            r#type: None,
            multiplicity: None,
            pos: None,
        }));
        let mut p = Pipeline::new(expr, &mut ledger, &views);
        p.alias_to_target_table.put(j("u1"), j("U"));
        p.alias_to_target_table.put(j("t1"), j("T"));
        p.alias_to_target_table.put(j("u2"), j("U"));
        p.alias_to_target_table.put(j("nav"), j("W"));
        p.class_slots.insert(j("nav"));
        p.alias_to_target_table.put(j("t2"), j("T"));
        let row = Arc::new(lite_front::records::Variable {
            name: j("row"),
            r#type: None,
            multiplicity: None,
            pos: None,
        });
        let mut scope: TableScope = LinkedMap::new();
        seed_alias_scope(&mut scope, &mut p, &row, &j("T")).unwrap();
        // the main table T: its first alias; U: ambiguous, unbound; the class slot: skipped
        let keys: Vec<JStr> = scope.iter().map(|(k, _)| k.clone()).collect();
        assert_eq!(keys, vec![j("T")]);
        let Some(VS::AppliedProperty(ap)) = scope.get(&j("T")) else { panic!() };
        assert_eq!(ap.property, j("t1"));
        assert!(p.ambiguous_tables.contains(&j("U")) && !p.ambiguous_tables.contains(&j("T")));
    }
}
