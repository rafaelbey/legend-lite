// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! `ViewRelation` (ViewRelation.java at ad86fa895): a store view as a relation expression -- the lifted view
//! functions' bodies (`LiftedViews::body`), and view-backed sets' PMs re-pointed at the view's columns (the frame
//! rule); the main source of a class. RE-EXPRESSED (census §1): `viewRelationExpr`'s `md` parameter is dropped (its one
//! caller, LiftedViews:101, passes null). The H2-LENIENT `first()` witness (:209-224) is ASK 12's; `pmThroughFrame`'s
//! default passes `EnumeratedExpression` and `OtherwiseEmbedded` through unrewritten (ASK 13): port as Java has them.
//!
//! The five `MissProbe` sites (:62, :109, :301, :369, :383) are "is this name a view?": `find_view`'s `Option`.
//! `RelationalOperation.mapChildren` answers the very object when no child changed; the rewrites here say that as an
//! inner `Option` (`None`: this operation, unchanged), so an untouched tree keeps its `Arc`s, as Java keeps its objects.

use std::collections::HashSet;
use std::sync::Arc;

use lite_builtin::pure::lite;
use lite_error::Phase;
use lite_front::nodes;
use lite_front::records as p;
use lite_model::linked::list;
use lite_model::{
    ClassMappingRelational, DatabaseDefinitionViewDefinition, FilterMapping, FilterPointer, JStr,
    LegacyMappingDefinitionTableReference as TableRef, PropertyMapping, PropertyMappingColumn,
    PropertyMappingEmbedded, PropertyMappingEnumeratedColumn, PropertyMappingExpression,
    PropertyMappingLocalProperty, RelationalOperation, RelationalOperationColumnRef as ColumnRef, cat,
};
use lite_names::{ModelIndex, calls};
use lite_types::{Multiplicity, RelationType};

use crate::error::Thrown;
use crate::group_by_synthesis;
use crate::jdk::equals_ignore_case;
use crate::join_chain_emission::{self, JoinNavSpec};
use crate::lifted_views::LiftedViews;
use crate::mapping_normalizer::{canonical_table, main_table_def_of, seed_alias_scope};
use crate::pipeline::Pipeline;
use crate::rel_op_translator::{self, TableScope};
use crate::resolved_mapping::ResolvedMapping;

/// `ViewRelation.mainSourceRef` (:48): a class's main source (its main table, or the view's lifted body).
pub fn main_source_ref(
    md: &ResolvedMapping<'_>,
    class_fqn: &JStr,
    model: &ModelIndex,
) -> Result<p::ValueSpecification, Thrown> {
    let r = main_table_def_of(md, Some(class_fqn), model)?;
    source_ref_for(&r, model, md)
}

/// `ViewRelation.sourceRefFor` (:57): a table reference's source (a view's lifted body, else `tableReference`).
pub fn source_ref_for(
    r: &TableRef,
    model: &ModelIndex,
    md: &ResolvedMapping<'_>,
) -> Result<p::ValueSpecification, Thrown> {
    let table = canonical_table(&r.table);
    match model.find_view(&r.database, &table) {
        Some(view) => md.views().body(view),
        None => Ok(table_reference(&r.database, table)),
    }
}

/// `ViewRelation.owner` (:80): a wall's owner label (`mapping=<fqn>`, or `store=<db>` with no mapping).
pub fn owner(md: Option<&ResolvedMapping<'_>>, db: &JStr) -> JStr {
    match md {
        None => cat!("store=", db),
        Some(md) => cat!("mapping=", md.qualified_name()),
    }
}

/// `ViewRelation.viewRelationExpr` (:87, its always-null `md` dropped): the view's relation -- its main table (or
/// inner view) through its joins (a view `Pipeline::for_view`), filtered, grouped, distinct, projected to its
/// columns, each column conforming to `signature`.
pub fn view_relation_expr(
    view: &Arc<DatabaseDefinitionViewDefinition>,
    view_name: &JStr,
    db: &JStr,
    model: &ModelIndex,
    views: &LiftedViews<'_>,
    signature: &RelationType<'_>,
) -> Result<p::ValueSpecification, Thrown> {
    // RE-EXPRESSED: Java's `md` here is always null (LiftedViews:101), so every `owner(md, db)` is `store=<db>` and
    // every `emitJoinChain` gets no mapping
    let md: Option<&ResolvedMapping<'_>> = None;
    let phys = model.view_main_table(db, view)?;
    // the columns the SIGNATURE declares [1] -- a PRIMARY KEY of the view, a NOT NULL column -- conform by emission:
    // the store's own trust wrap (TRUST_ONE erases in SQL); the compiler checks the rest
    let mut ones: HashSet<JStr> = HashSet::new();
    for c in signature.columns() {
        if c.multiplicity == Multiplicity::ONE {
            ones.insert(JStr::from(c.name));
        }
    }
    let r = Arc::new(nodes::variable(JStr::from("vr")));
    // VIEW-ON-VIEW: the inferred root is itself a view -- expand it recursively as the SOURCE relation (the outer
    // view's ColumnRefs spell <innerView>.<declaredCol>, the inner relation's output row, so the scope key stays `phys`)
    let source = match model.find_view(db, &phys) {
        Some(inner_view) => views.body(inner_view)?,
        None => table_reference(db, phys.clone()),
    };
    // JOIN-NAVIGATING view columns hoist as slots on a real Pipeline -- the pass-2 machinery PM bodies use
    let mut vp = Pipeline::for_view(source, views);
    for vc0 in view.column_mappings.iter() {
        let mut navs0: Vec<JoinNavSpec> = Vec::new();
        join_chain_emission::collect_join_navigations(&vc0.expression, &mut navs0);
        for nav in &navs0 {
            join_chain_emission::emit_join_chain(
                &mut vp,
                &nav.chain,
                nav.chain_db.as_ref(),
                Some(&vc0.name),
                None,
                db,
                &phys,
                &r,
                model,
                md,
                false,
            )?;
        }
    }
    // the ~filter condition and ~groupBy key ops can navigate joins too -- hoisted the SAME way, BEFORE the
    // pipeline expr is taken
    let mut view_filter_def = None;
    if let Some(filter) = &view.filter {
        let FilterMapping::FilterMappingDirect(direct) = filter else {
            return Err(Thrown::not_implemented(cat!(
                "view '",
                view_name,
                "' used as a join target has a",
                " join-mediated ~filter; only direct view filters",
                " expand as relations. ",
                &owner(md, db)
            )));
        };
        let db_fqn = match &direct.filter {
            FilterPointer::FilterPointerCross(c) => c.db.clone(),
            FilterPointer::FilterPointerLocal(_) => db.clone(),
        };
        let fd = model.find_filter(Some(&db_fqn), direct.filter.name()).ok_or_else(|| {
            Thrown::model(
                Phase::Normalize,
                cat!(
                    "~filter '",
                    direct.filter.name(),
                    "' of view '",
                    view_name,
                    "' not found in db '",
                    &db_fqn,
                    "'; ",
                    &owner(md, db)
                ),
                None,
            )
        })?;
        let mut navs: Vec<JoinNavSpec> = Vec::new();
        join_chain_emission::collect_join_navigations(&fd.condition, &mut navs);
        for nav in &navs {
            join_chain_emission::emit_join_chain(
                &mut vp,
                &nav.chain,
                nav.chain_db.as_ref(),
                Some(&cat!(view_name, "_filter")),
                None,
                db,
                &phys,
                &r,
                model,
                md,
                false,
            )?;
        }
        view_filter_def = Some(fd.clone());
    }
    for key_op in view.group_by_columns.iter() {
        let mut navs: Vec<JoinNavSpec> = Vec::new();
        join_chain_emission::collect_join_navigations(key_op, &mut navs);
        for nav in &navs {
            join_chain_emission::emit_join_chain(
                &mut vp,
                &nav.chain,
                nav.chain_db.as_ref(),
                Some(&cat!(view_name, "_groupBy")),
                None,
                db,
                &phys,
                &r,
                model,
                md,
                false,
            )?;
        }
    }
    let mut src = vp.expr.clone();
    let mut scope = TableScope::new();
    scope.put(phys.clone(), var(&r));
    seed_alias_scope(&mut scope, &mut vp, &r, &phys)?;
    if let Some(fd) = &view_filter_def {
        let cond = rel_op_translator::translate(&fd.condition, &scope, None, Some(&r), vp.view())?;
        src = applied(calls::platform(&JStr::from("filter"), vec![src, lambda(&[&r], cond)]));
    }
    let mut synthetic_key_cols = false;
    if !view.group_by_columns.is_empty() {
        let key_ops = &view.group_by_columns;
        let mut claimed = vec![false; key_ops.len()];
        let mut key_cols: Vec<Arc<p::ColSpec>> = Vec::new();
        let mut agg_cols: Vec<Arc<p::ColSpec>> = Vec::new();
        for vc in view.column_mappings.iter() {
            let expr = &vc.expression;
            if let RelationalOperation::RelationalOperationFunctionCall(fc) = expr
                && group_by_synthesis::is_group_reducer(fc)?
            {
                // `fc.args().get(0)`: a group reducer has exactly one argument (isGroupReducer's first test)
                let arg0 = fc.args.first().ok_or_else(|| {
                    Thrown::java(
                        "java.lang.IndexOutOfBoundsException",
                        Some(JStr::from("Index 0 out of bounds for length 0")),
                    )
                })?;
                let selector = rel_op_translator::translate(arg0, &scope, None, Some(&r), vp.view())?;
                let vals = Arc::new(nodes::variable(JStr::from("vals")));
                let reduced = applied(calls::platform(
                    &rel_op_translator::dyna_fn_name(fc)?,
                    vec![var(&vals)],
                ));
                agg_cols.push(col_spec(
                    vc.name.clone(),
                    Some(lambda_fn(&[&r], selector)),
                    Some(lambda_fn(&[&vals], declared_one(&ones, &vc.name, reduced))),
                ));
                continue;
            }
            let matched =
                (0..key_ops.len()).find(|&i| !claimed[i] && group_by_synthesis::group_by_ops_match(expr, &key_ops[i]));
            let Some(matched) = matched else {
                // H2-LENIENT per-row column under view ~groupBy (modelJoins LegalEntity_View: name/value bare over
                // group by ENTITY_ID -- H2 1.x returns a per-group witness): an implicit first()-reduced aggregate,
                // DB-side (ANY_VALUE); the engine-style text spells the bare column exactly like the golden (ASK 12,
                // ported as Java has it)
                let w_sel = rel_op_translator::translate(expr, &scope, None, Some(&r), vp.view())?;
                let w_vals = Arc::new(nodes::variable(JStr::from("vals")));
                let first = applied(calls::platform(&JStr::from("first"), vec![var(&w_vals)]));
                agg_cols.push(col_spec(
                    vc.name.clone(),
                    Some(lambda_fn(&[&r], w_sel)),
                    Some(lambda_fn(&[&w_vals], declared_one(&ones, &vc.name, first))),
                ));
                continue;
            };
            claimed[matched] = true;
            let key_value = rel_op_translator::translate(expr, &scope, None, Some(&r), vp.view())?;
            key_cols.push(col_spec(
                vc.name.clone(),
                Some(lambda_fn(&[&r], declared_one(&ones, &vc.name, key_value))),
                None,
            ));
        }
        for (i, key_op) in key_ops.iter().enumerate() {
            if !claimed[i] {
                // grouped-but-unprojected key: keep the grouping exact
                let key_value = rel_op_translator::translate(key_op, &scope, None, Some(&r), vp.view())?;
                key_cols.push(col_spec(
                    cat!("k", i.to_string().as_str()),
                    Some(lambda_fn(&[&r], key_value)),
                    None,
                ));
                synthetic_key_cols = true;
            }
        }
        // the view's rows grouped by key EXPRESSIONS: the internal-desugar identity (GROUP_BY_COMPUTED_KEYS)
        src = applied(calls::exact(
            &JStr::from(lite::GROUP_BY_COMPUTED_KEYS),
            vec![src, col_spec_array(key_cols), col_spec_array(agg_cols)],
        ));
    } else {
        let mut cols: Vec<Arc<p::ColSpec>> = Vec::with_capacity(view.column_mappings.len());
        for vc in view.column_mappings.iter() {
            let val = rel_op_translator::translate(&vc.expression, &scope, None, Some(&r), vp.view())?;
            cols.push(col_spec(
                vc.name.clone(),
                Some(lambda_fn(&[&r], declared_one(&ones, &vc.name, val))),
                None,
            ));
        }
        src = applied(calls::platform(&JStr::from("project"), vec![src, col_spec_array(cols)]));
    }
    if view.distinct {
        // ~distinct is over the view's DECLARED columns; the grouped path's synthetic k<i> columns are dropped first
        // so groups agreeing on declared columns collapse (audit 18 finding 4)
        if synthetic_key_cols {
            let vd = Arc::new(nodes::variable(JStr::from("vd")));
            let mut declared: Vec<Arc<p::ColSpec>> = Vec::with_capacity(view.column_mappings.len());
            for vc in view.column_mappings.iter() {
                declared.push(col_spec(
                    vc.name.clone(),
                    Some(lambda_fn(&[&vd], nodes::property(var(&vd), vc.name.clone(), None))),
                    None,
                ));
            }
            src = applied(calls::platform(&JStr::from("project"), vec![src, col_spec_array(declared)]));
        }
        src = applied(calls::platform(&JStr::from("distinct"), vec![src]));
    }
    Ok(src)
}

/// `ViewRelation.declaredOne` (:286): a column the view declares `[1]` (`ones`) wrapped to one.
pub fn declared_one(ones: &HashSet<JStr>, column: &JStr, value: p::ValueSpecification) -> p::ValueSpecification {
    if ones.contains(column) {
        applied(calls::exact(&JStr::from(lite::TRUST_ONE), vec![value]))
    } else {
        value
    }
}

/// `ViewRelation.inlineViewRefs` (:298): a view column's references to other views inlined.
pub fn inline_view_refs(
    op: &RelationalOperation,
    db: &JStr,
    model: &ModelIndex,
) -> Result<RelationalOperation, Thrown> {
    Ok(inline_view_refs_changed(op, db, model)?.unwrap_or_else(|| op.clone()))
}

/// [`inline_view_refs`], `None` for the very operation (`mapChildren`'s identity).
fn inline_view_refs_changed(
    op: &RelationalOperation,
    db: &JStr,
    model: &ModelIndex,
) -> Result<Option<RelationalOperation>, Thrown> {
    if let RelationalOperation::RelationalOperationColumnRef(cr) = op {
        if let Some(view) = model.find_view(db, &cr.table) {
            for vc in view.column_mappings.iter() {
                if vc.name == cr.column {
                    return Ok(Some(inline_view_refs(&vc.expression, db, model)?));
                }
            }
        }
        return Ok(None);
    }
    op.try_map_children(|x| inline_view_refs_changed(x, db, model))
}

/// `ViewRelation.frameRewrite` (:324): an operation over a view-backed set's view re-pointed at the view's columns
/// (the frame rule); loud for a column the view does not project.
pub fn frame_rewrite(
    op: &RelationalOperation,
    view: &Arc<DatabaseDefinitionViewDefinition>,
    view_name: &JStr,
    md: &ResolvedMapping<'_>,
) -> Result<RelationalOperation, Thrown> {
    Ok(frame_rewrite_changed(op, view, view_name, md)?.unwrap_or_else(|| op.clone()))
}

/// [`frame_rewrite`], `None` for the very operation.
fn frame_rewrite_changed(
    op: &RelationalOperation,
    view: &Arc<DatabaseDefinitionViewDefinition>,
    view_name: &JStr,
    md: &ResolvedMapping<'_>,
) -> Result<Option<RelationalOperation>, Thrown> {
    let RelationalOperation::RelationalOperationColumnRef(cr) = op else {
        return op.try_map_children(|x| frame_rewrite_changed(x, view, view_name, md));
    };
    Ok(frame_column(cr, view, view_name, md)?.map(RelationalOperation::RelationalOperationColumnRef))
}

/// [`frame_rewrite`] of one column reference, `None` for the very reference (`pmThroughFrame`'s casts read the
/// rewritten reference's table and column, a column reference always answering one).
fn frame_column(
    cr: &Arc<ColumnRef>,
    view: &DatabaseDefinitionViewDefinition,
    view_name: &JStr,
    md: &ResolvedMapping<'_>,
) -> Result<Option<Arc<ColumnRef>>, Thrown> {
    let table = canonical_table(&cr.table);
    // a reference to the VIEW itself spells the column as the view declares it (unquoted identifiers are
    // case-insensitive; the frame row carries the declared spelling)
    if equals_ignore_case(&table.0, &view_name.0) {
        for vc in view.column_mappings.iter() {
            if equals_ignore_case(&vc.name.0, &cr.column.0) {
                return Ok(if vc.name == cr.column {
                    None
                } else {
                    Some(column_ref(cr.database_name.clone(), cr.table.clone(), vc.name.clone()))
                });
            }
        }
        return Ok(None);
    }
    // a reference to a column the view CARRIES (a declared column whose expression is exactly that table's column)
    // resolves to the declared column; a reference to a table the view reads but a column it does not carry is loud
    let mut reads_table = false;
    for vc in view.column_mappings.iter() {
        if let RelationalOperation::RelationalOperationColumnRef(vcr) = &vc.expression
            && equals_ignore_case(&canonical_table(&vcr.table).0, &table.0)
        {
            reads_table = true;
            if equals_ignore_case(&vcr.column.0, &cr.column.0) {
                return Ok(Some(column_ref(cr.database_name.clone(), view_name.clone(), vc.name.clone())));
            }
        }
    }
    if reads_table {
        return Err(Thrown::not_implemented(cat!(
            "column '",
            &cr.table,
            ".",
            &cr.column,
            "' is read under view '",
            view_name,
            "', which declares no column",
            " carrying it; mapping=",
            md.qualified_name()
        )));
    }
    Ok(None)
}

/// `ViewRelation.frameRewriteIfView` (:367): [`frame_rewrite`] when `table` is a view of `db`.
pub fn frame_rewrite_if_view(
    op: &RelationalOperation,
    db: &JStr,
    table: &JStr,
    md: &ResolvedMapping<'_>,
    model: &ModelIndex,
) -> Result<RelationalOperation, Thrown> {
    let out = match model.find_view(db, table) {
        None => op.clone(),
        Some(view) => frame_rewrite(op, view, table, md)?,
    };
    declared_spelling(&out, db, model)
}

/// `ViewRelation.declaredSpelling` (:377): a join condition's table refs in their declared spelling.
pub fn declared_spelling(
    op: &RelationalOperation,
    db: &JStr,
    model: &ModelIndex,
) -> Result<RelationalOperation, Thrown> {
    Ok(declared_spelling_changed(op, db, model)?.unwrap_or_else(|| op.clone()))
}

/// [`declared_spelling`], `None` for the very operation.
fn declared_spelling_changed(
    op: &RelationalOperation,
    db: &JStr,
    model: &ModelIndex,
) -> Result<Option<RelationalOperation>, Thrown> {
    let RelationalOperation::RelationalOperationColumnRef(cr) = op else {
        return op.try_map_children(|x| declared_spelling_changed(x, db, model));
    };
    let vdb = match &cr.database_name {
        Some(d) if !d.is_empty() => d,
        _ => db,
    };
    let Some(v) = model.find_view(vdb, &canonical_table(&cr.table)) else {
        return Ok(None);
    };
    for vc in v.column_mappings.iter() {
        if equals_ignore_case(&vc.name.0, &cr.column.0) {
            return Ok(if vc.name == cr.column {
                None
            } else {
                Some(RelationalOperation::RelationalOperationColumnRef(column_ref(
                    cr.database_name.clone(),
                    cr.table.clone(),
                    vc.name.clone(),
                )))
            });
        }
    }
    Ok(None)
}

/// `ViewRelation.throughFrame` (:400): a view-backed set with its PMs, filter and keys re-pointed through the frame.
pub fn through_frame(
    rcm: &ClassMappingRelational,
    view: &Arc<DatabaseDefinitionViewDefinition>,
    view_name: &JStr,
    md: &ResolvedMapping<'_>,
) -> Result<Arc<ClassMappingRelational>, Thrown> {
    let mut pms: Vec<PropertyMapping> = Vec::with_capacity(rcm.property_mappings.len());
    for pm in rcm.property_mappings.iter() {
        pms.push(pm_through_frame(pm, view, view_name, md)?);
    }
    let mut group_by: Vec<RelationalOperation> = Vec::with_capacity(rcm.group_by.len());
    for k in rcm.group_by.iter() {
        group_by.push(frame_rewrite(k, view, view_name, md)?);
    }
    let mut pk: Vec<RelationalOperation> = Vec::with_capacity(rcm.primary_key.len());
    for k in rcm.primary_key.iter() {
        pk.push(frame_rewrite(k, view, view_name, md)?);
    }
    // the filter is kept as written, and the source url dropped (Java's `null`, :416)
    Ok(Arc::new(ClassMappingRelational {
        group_by: list(group_by),
        primary_key: list(pk),
        property_mappings: list(pms),
        source_url: None,
        ..rcm.clone()
    }))
}

/// `ViewRelation.pmThroughFrame` (:419): one PM through the frame.
pub fn pm_through_frame(
    pm: &PropertyMapping,
    view: &Arc<DatabaseDefinitionViewDefinition>,
    view_name: &JStr,
    md: &ResolvedMapping<'_>,
) -> Result<PropertyMapping, Thrown> {
    Ok(match pm {
        PropertyMapping::PropertyMappingColumn(col) => {
            let read = column_ref(Some(col.database.clone()), col.table.clone(), col.column.clone());
            let r = frame_column(&read, view, view_name, md)?.unwrap_or(read);
            PropertyMapping::PropertyMappingColumn(Arc::new(PropertyMappingColumn {
                property_name: col.property_name.clone(),
                database: col.database.clone(),
                table: r.table.clone(),
                column: r.column.clone(),
            }))
        }
        PropertyMapping::PropertyMappingEnumeratedColumn(ec) => {
            let read = column_ref(Some(ec.database.clone()), ec.table.clone(), ec.column.clone());
            let r = frame_column(&read, view, view_name, md)?.unwrap_or(read);
            PropertyMapping::PropertyMappingEnumeratedColumn(Arc::new(PropertyMappingEnumeratedColumn {
                property_name: ec.property_name.clone(),
                enum_mapping_id: ec.enum_mapping_id.clone(),
                database: ec.database.clone(),
                table: r.table.clone(),
                column: r.column.clone(),
            }))
        }
        PropertyMapping::PropertyMappingExpression(ex) => {
            PropertyMapping::PropertyMappingExpression(Arc::new(PropertyMappingExpression {
                property_name: ex.property_name.clone(),
                expression: frame_rewrite(&ex.expression, view, view_name, md)?,
            }))
        }
        PropertyMapping::PropertyMappingLocalProperty(lp) => {
            PropertyMapping::PropertyMappingLocalProperty(Arc::new(PropertyMappingLocalProperty {
                property_name: lp.property_name.clone(),
                r#type: lp.r#type.clone(),
                multiplicity: lp.multiplicity.clone(),
                body: pm_through_frame(&lp.body, view, view_name, md)?,
            }))
        }
        PropertyMapping::PropertyMappingEmbedded(em) => {
            let mut subs: Vec<PropertyMapping> = Vec::with_capacity(em.property_mappings.len());
            for s in em.property_mappings.iter() {
                subs.push(pm_through_frame(s, view, view_name, md)?);
            }
            let mut epk: Vec<RelationalOperation> = Vec::with_capacity(em.primary_key.len());
            for k in em.primary_key.iter() {
                epk.push(frame_rewrite(k, view, view_name, md)?);
            }
            PropertyMapping::PropertyMappingEmbedded(Arc::new(PropertyMappingEmbedded {
                property_name: em.property_name.clone(),
                property_mappings: list(subs),
                primary_key: list(epk),
            }))
        }
        // joins depart from the view by name (their conditions are translated against the frame row at emission);
        // the other kinds carry no column of the root -- Java's `default -> pm`, which passes EnumeratedExpression
        // and OtherwiseEmbedded through unrewritten too (ASK 13, ported as Java has it)
        PropertyMapping::PropertyMappingJoin(_)
        | PropertyMapping::PropertyMappingJoinTerminalColumn(_)
        | PropertyMapping::PropertyMappingInlineEmbedded(_)
        | PropertyMapping::PropertyMappingEnumeratedExpression(_)
        | PropertyMapping::PropertyMappingOtherwiseEmbedded(_) => pm.clone(),
    })
}

/// `new RelationalOperation.ColumnRef(databaseName, table, column)`.
fn column_ref(database_name: Option<JStr>, table: JStr, column: JStr) -> Arc<ColumnRef> {
    Arc::new(ColumnRef {
        database_name,
        table,
        column,
    })
}

/// `Calls.platform("tableReference", List.of(new PackageableElementPtr(db), new CString(table)))`.
fn table_reference(db: &JStr, table: JStr) -> p::ValueSpecification {
    applied(calls::platform(
        &JStr::from("tableReference"),
        vec![nodes::ptr(db.clone(), None), nodes::cstring(table, None)],
    ))
}

/// A built call as a value.
fn applied(f: p::AppliedFunction) -> p::ValueSpecification {
    p::ValueSpecification::AppliedFunction(Arc::new(f))
}

/// A variable as a value.
fn var(v: &Arc<p::Variable>) -> p::ValueSpecification {
    p::ValueSpecification::Variable(v.clone())
}

/// `new LambdaFunction(params, List.of(body))`.
fn lambda_fn(params: &[&Arc<p::Variable>], body: p::ValueSpecification) -> Arc<p::LambdaFunction> {
    Arc::new(p::LambdaFunction {
        parameters: list(params.iter().map(|v| (*v).clone()).collect()),
        body: list(vec![body]),
        pos: None,
    })
}

/// [`lambda_fn`] as a value.
fn lambda(params: &[&Arc<p::Variable>], body: p::ValueSpecification) -> p::ValueSpecification {
    p::ValueSpecification::LambdaFunction(lambda_fn(params, body))
}

/// `new ColSpec(name, function1, function2)`: un-aliased, no call arguments, unqualified, untyped.
fn col_spec(
    name: JStr,
    function1: Option<Arc<p::LambdaFunction>>,
    function2: Option<Arc<p::LambdaFunction>>,
) -> Arc<p::ColSpec> {
    Arc::new(p::ColSpec {
        name,
        function1,
        function2,
        alias: None,
        args: list(Vec::new()),
        qualified: false,
        pos: None,
        col_type: None,
        col_type_mult: None,
        stereotypes: list(Vec::new()),
        tagged_values: list(Vec::new()),
    })
}

/// `new ColSpecArray(colSpecs)` as a value.
fn col_spec_array(col_specs: Vec<Arc<p::ColSpec>>) -> p::ValueSpecification {
    p::ValueSpecification::ColumnInstance(p::ColumnInstance::ColSpecArray(Arc::new(p::ColSpecArray {
        col_specs: list(col_specs),
        pos: None,
    })))
}
