// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! MappingNormalizer families 10, 11 and 12 (EF_CENSUS §1.1): the relational dispatch (JSON / view / table), the
//! main-table inference and the `RelationalSource` stamp; JSON-source sets; view-backed sets (the view frame as the
//! pipeline's source, `md.views().body(view)`).
//!
//! RE-EXPRESSED (census §4): `inferMainTableQuiet`'s catch-as-probe is [`infer_main_table`] answering
//! `Result<Option<_>>` and [`infer_main_table_quiet`] mapping a `ModelException` (only) to `None`; anything else
//! (an unported body included) still propagates, so the quiet form is a `Result` too.

use std::sync::Arc;

use lite_error::Phase;
use lite_front::records as p;
use lite_model::linked::list;
use lite_model::{
    ClassMappingRelational, DatabaseDefinitionViewDefinition, JStr, LegacyMappingDefinitionTableReference as TableRef,
    MappingDefinitionEnumColumn, MappingDefinitionRelationalSource, MappingDefinitionRelationalSourceJson,
    MappingDefinitionRelationalSourceTable, PropertyMapping, RelationalOperation, cat,
};
use lite_names::{ModelIndex, calls};

use super::{Fields, build_new_instance_to_one, synth_table_backed_mapping};
use crate::error::{Thrown, require};
use crate::jdk::LinkedSet;
use crate::mapping_ledger::MappingLedger;
use crate::resolved_mapping::ResolvedMapping;
use crate::view_relation;

/// `MappingNormalizer.synthRelational` (:1327): a Relational set's body -- JSON source, view-backed, or table-backed
/// (a set with no `~mainTable` takes its inferred one; loud when there is none).
pub fn synth_relational(
    md: &ResolvedMapping<'_>,
    rcm: &Arc<ClassMappingRelational>,
    model: &ModelIndex,
    ledger: &mut MappingLedger,
) -> Result<p::ValueSpecification, Thrown> {
    // JSON-source: synthesized by ModelBuilder cross-baking from a RuntimeDefinition's JsonModelConnection; mainTable
    // is null, sourceUrl carries the inline VARIANT subquery source
    if rcm.source_url.is_some() {
        return synth_json_source_mapping(md, rcm, model);
    }
    let mut rcm = rcm.clone();
    if rcm.main_table.is_none() {
        let Some(inferred) = resolved_main_table(&rcm)? else {
            // a set reading no table and extending none: the model's error, in the engine's words (since 2026-11-15,
            // S390); a set extending another keeps the deferred refusal
            if rcm.extends_set_id.is_none() {
                return Err(Thrown::model(Phase::Mapping, main_table_refusal(&rcm), None));
            }
            return Err(Thrown::not_implemented(cat!(
                "Relational mapping without ~mainTable, sourceUrl, or an inferable column binding; class=",
                &rcm.class_name,
                ", mapping=",
                md.qualified_name(),
                ". See docs/MAPPING_LEGACY_TO_FUNCTION.md \u{a7}5.2.3."
            )));
        };
        rcm = Arc::new(ClassMappingRelational {
            main_table: Some(inferred),
            ..(*rcm).clone()
        });
    }
    let v_main = require(rcm.main_table.as_ref(), Some("table-backed set without ~mainTable"))?;
    // MissProbe.miss: not a view -> table-backed (the miss is the answer)
    if let Some(view) = model.find_view(&v_main.database, &v_main.table) {
        return synth_view_backed_mapping(md, &rcm, view, model, ledger);
    }
    synth_table_backed_mapping(md, &rcm, model, ledger, None)
}

/// `MappingNormalizer.resolvedMainTable` (:1372): the set's main table, declared or inferred; none for a JSON source.
pub fn resolved_main_table(rcm: &ClassMappingRelational) -> Result<Option<Arc<TableRef>>, Thrown> {
    if rcm.source_url.is_some() {
        return Ok(None);
    }
    match &rcm.main_table {
        Some(main) => Ok(Some(main.clone())),
        None => infer_main_table(rcm),
    }
}

/// `MappingNormalizer.relationalSourceOf` (:1387): the binding's `RelationalSource` stamp (JSON, or the table with
/// `aggregationAwareMain` and the enumerated columns); an internal error for a set whose synthesis must have walled.
pub fn relational_source_of(rcm: &ClassMappingRelational) -> Result<MappingDefinitionRelationalSource, Thrown> {
    if let Some(url) = &rcm.source_url {
        return Ok(
            MappingDefinitionRelationalSource::MappingDefinitionRelationalSourceJson(Arc::new(
                MappingDefinitionRelationalSourceJson { url: url.clone() },
            )),
        );
    }
    let Some(main) = resolved_main_table(rcm)? else {
        // unreachable on a lifted binding: synthRelational THROWS on exactly this condition before the binding is
        // constructed
        return Err(Thrown::illegal_state(cat!(
            "stamping a set whose synthesis must have walled: ",
            &rcm.class_name
        )));
    };
    let mut ecs = Vec::new();
    for pm in rcm.property_mappings.iter() {
        if let PropertyMapping::PropertyMappingEnumeratedColumn(ec) = pm
            && let Some(id) = &ec.enum_mapping_id
        {
            ecs.push(Arc::new(MappingDefinitionEnumColumn {
                table: ec.table.clone(),
                column: ec.column.clone(),
                enum_mapping_id: id.clone(),
            }));
        }
    }
    Ok(
        MappingDefinitionRelationalSource::MappingDefinitionRelationalSourceTable(Arc::new(
            MappingDefinitionRelationalSourceTable {
                database: main.database.clone(),
                table: main.table.clone(),
                aggregation_aware_main: rcm.aggregation_aware_main(),
                enum_columns: list(ecs),
            },
        )),
    )
}

/// `MappingNormalizer.inferMainTable` (:1419): the one table the PMs read (by its name, whatever database a reference
/// spells: the first's); none when they read none; loud for two table names, in legend-engine's words (since
/// 2026-11-15, the normalizer's second batch, item 3, S390: two databases were refused and the words were lite's).
pub fn infer_main_table(rcm: &ClassMappingRelational) -> Result<Option<Arc<TableRef>>, Thrown> {
    let mut refs: Vec<Arc<TableRef>> = Vec::new();
    for pm in rcm.property_mappings.iter() {
        collect_main_tables(pm, &mut refs);
    }
    let mut names: LinkedSet<JStr> = LinkedSet::new();
    for r in &refs {
        names.add(canonical_table(&r.table));
    }
    if names.len() > 1 {
        return Err(Thrown::model(Phase::Mapping, main_table_refusal(rcm), None));
    }
    let Some(first) = refs.first() else {
        return Ok(None);
    };
    // the CANONICAL name: scope([db]default.personTable) columns record 'default.personTable' while join conditions
    // and the store lookup say 'personTable'
    Ok(Some(Arc::new(TableRef {
        database: first.database.clone(),
        table: canonical_table(&first.table),
    })))
}

/// `MappingNormalizer.mainTableRefusal`: legend-engine's refusal of a set whose main table it cannot infer, the set's id
/// as written (`null` for an unlabelled one).
pub fn main_table_refusal(rcm: &ClassMappingRelational) -> JStr {
    cat!(
        "Can't find the main table for class '",
        &or_null_id(rcm.set_id.as_ref()),
        "'. Please specify a main table using the ~mainTable directive."
    )
}

fn or_null_id(id: Option<&JStr>) -> JStr {
    id.cloned().unwrap_or_else(|| JStr::from("null"))
}

/// `MappingNormalizer.canonicalTable` (:1463): `KnowledgeLayer.canonicalTable` (lite-names').
pub fn canonical_table(table: &JStr) -> JStr {
    lite_names::knowledge::canonical_table(table)
}

/// `MappingNormalizer.inferMainTableQuiet` (:1468): [`infer_main_table`] with a `ModelException` answered as none
/// (the module's note).
pub fn infer_main_table_quiet(rcm: &ClassMappingRelational) -> Result<Option<Arc<TableRef>>, Thrown> {
    match infer_main_table(rcm) {
        Ok(main) => Ok(main),
        Err(e) if e.is_model() => Ok(None),
        Err(e) => Err(e),
    }
}

/// `MappingNormalizer.collectMainTables` (:1477): the tables one PM reads, into `sink` in read order.
pub fn collect_main_tables(pm: &PropertyMapping, sink: &mut Vec<Arc<TableRef>>) {
    match pm {
        PropertyMapping::PropertyMappingColumn(c) => sink.push(Arc::new(TableRef {
            database: c.database.clone(),
            table: c.table.clone(),
        })),
        PropertyMapping::PropertyMappingEnumeratedColumn(ec) => sink.push(Arc::new(TableRef {
            database: ec.database.clone(),
            table: ec.table.clone(),
        })),
        PropertyMapping::PropertyMappingEmbedded(emb) => {
            for inner in emb.property_mappings.iter() {
                collect_main_tables(inner, sink);
            }
        }
        PropertyMapping::PropertyMappingLocalProperty(lp) => collect_main_tables(&lp.body, sink),
        // a computed column's DIRECT column references count; a join navigation inside it contributes nothing
        PropertyMapping::PropertyMappingExpression(ex) => collect_expr_tables(&ex.expression, sink),
        // DELIBERATE non-contributors (exhaustive, no default): joins and join terminals reference OTHER tables; enum
        // expressions and inline embeddeds carry no direct column
        PropertyMapping::PropertyMappingJoin(_) => {}
        PropertyMapping::PropertyMappingJoinTerminalColumn(_) => {}
        PropertyMapping::PropertyMappingEnumeratedExpression(_) => {}
        PropertyMapping::PropertyMappingInlineEmbedded(_) => {}
        // an otherwise-embedded block's own property mappings read the owner's row like a plain embedded block
        PropertyMapping::PropertyMappingOtherwiseEmbedded(oe) => {
            for inner in oe.embedded.iter() {
                collect_main_tables(inner, sink);
            }
        }
    }
}

/// `MappingNormalizer.collectExprTables` (:1509): the tables an expression reads, into `sink`.
pub fn collect_expr_tables(op: &RelationalOperation, sink: &mut Vec<Arc<TableRef>>) {
    if let RelationalOperation::RelationalOperationColumnRef(cr) = op
        && let Some(db) = &cr.database_name
        && !db.is_empty()
    {
        sink.push(Arc::new(TableRef {
            database: db.clone(),
            table: cr.table.clone(),
        }));
        return;
    }
    match op {
        RelationalOperation::RelationalOperationFunctionCall(fc) => {
            for a in fc.args.iter() {
                collect_expr_tables(a, sink);
            }
        }
        RelationalOperation::RelationalOperationComparison(c) => {
            collect_expr_tables(&c.left, sink);
            collect_expr_tables(&c.right, sink);
        }
        RelationalOperation::RelationalOperationBooleanOp(b) => {
            collect_expr_tables(&b.left, sink);
            collect_expr_tables(&b.right, sink);
        }
        RelationalOperation::RelationalOperationIsNull(n) => collect_expr_tables(&n.operand, sink),
        RelationalOperation::RelationalOperationIsNotNull(n) => collect_expr_tables(&n.operand, sink),
        RelationalOperation::RelationalOperationGroup(g) => collect_expr_tables(&g.inner, sink),
        RelationalOperation::RelationalOperationArrayLiteral(a) => {
            for e in a.elements.iter() {
                collect_expr_tables(e, sink);
            }
        }
        RelationalOperation::RelationalOperationLambda(lam) => collect_expr_tables(&lam.body, sink),
        RelationalOperation::RelationalOperationLambdaParam(_) => {}
        // DELIBERATE non-contributors (exhaustive, no default): literals carry no table; a bare/target column ref
        // without a database qualifier cannot name one; join navigations reference OTHER tables by construction
        RelationalOperation::RelationalOperationColumnRef(_) => {}
        RelationalOperation::RelationalOperationTargetColumnRef(_) => {}
        RelationalOperation::RelationalOperationLiteral(_) => {}
        RelationalOperation::RelationalOperationJoinNavigation(_) => {}
    }
}

/// `MappingNormalizer.synthJsonSourceMapping` (:1565): `sourceUrl(url) -> map(row | ^C(p = to(get(...))))`.
pub fn synth_json_source_mapping(
    md: &ResolvedMapping<'_>,
    rcm: &Arc<ClassMappingRelational>,
    model: &ModelIndex,
) -> Result<p::ValueSpecification, Thrown> {
    let url = require(
        rcm.source_url.as_ref(),
        Some("sourceUrl-backed set without a source url"),
    )?;
    let source = applied(calls::exact(
        &JStr::from(lite_builtin::pure::lite::SOURCE_URL),
        vec![c_string(url.clone())],
    ));
    let row_bind = variable("row");
    let cd = model
        .knowledge()
        .hierarchy_class(Some(&rcm.class_name))
        .ok_or_else(|| {
            Thrown::model(
                Phase::Normalize,
                cat!(
                    "JSON-source mapping references unknown class '",
                    &rcm.class_name,
                    "'; mapping=",
                    md.qualified_name()
                ),
                None,
            )
        })?;
    let mut fields = Fields::new();
    for prop in cd.properties.iter() {
        // get($row.data, 'propName') -- 2-arg variant access (the only `get` native is get(Variant[1], Any[1])); the
        // single VARIANT `data` column fanned into property values by key (ours: the engine reads a
        // JsonModelConnection in memory and generates no SQL for it)
        let get = applied(calls::platform(
            &JStr::from("get"),
            vec![
                p::ValueSpecification::AppliedProperty(Arc::new(p::AppliedProperty {
                    receiver: p::ValueSpecification::Variable(row_bind.clone()),
                    property: JStr::from("data"),
                    pos: None,
                    owner_class: None,
                })),
                c_string(prop.name.clone()),
            ],
        ));
        // to(get(...), @Type) -- typed text extraction + cast
        let value = match &prop.r#type {
            p::TypeExpression::TypeExpressionNameRef(nr) => applied(calls::platform(
                &JStr::from("to"),
                vec![
                    get,
                    p::ValueSpecification::TypeAnnotation(p::TypeAnnotation::TypeAnnotationNamed(Arc::new(
                        p::TypeAnnotationNamed {
                            r#type: p::TypeExpression::TypeExpressionNameRef(Box::new(p::TypeExpressionNameRef {
                                name: nr.name.clone(),
                                pos: None,
                            })),
                            pos: None,
                        },
                    ))),
                ],
            )),
            _ => get,
        };
        fields.put(
            prop.name.clone(),
            p::KeyExpression {
                value,
                is_add: false,
                is_local: false,
            },
        );
    }
    let terminal = build_new_instance_to_one(Some(&rcm.class_name), &fields, model)?;
    Ok(applied(calls::platform(
        &JStr::from("map"),
        vec![
            source,
            p::ValueSpecification::LambdaFunction(Arc::new(p::LambdaFunction {
                parameters: list(vec![row_bind]),
                body: list(vec![terminal]),
                pos: None,
            })),
        ],
    )))
}

/// `MappingNormalizer.synthViewBackedMapping` (:1625): a set on a view -- the view's lifted body as the source, the
/// PMs re-pointed through the frame (`ViewRelation.throughFrame`).
pub fn synth_view_backed_mapping(
    md: &ResolvedMapping<'_>,
    rcm: &Arc<ClassMappingRelational>,
    view: &Arc<DatabaseDefinitionViewDefinition>,
    model: &ModelIndex,
    ledger: &mut MappingLedger,
) -> Result<p::ValueSpecification, Thrown> {
    // Java's `mainDb` local (`requireNonNull(rcm.mainTable(), ..).database()`) is read by nothing; its null check is
    // the one that reaches an answer
    let main = require(rcm.main_table.as_ref(), Some("view-backed set without ~mainTable"))?;
    // the view is the set's FRAME (its subselect); every reference the set makes to the view's root table resolves to
    // the declared column that carries it (ViewRelation.frameRewrite); joins depart from the view by name
    let view_name = main.table.clone();
    let view_source = md.views().body(view)?;
    let over_view = view_relation::through_frame(rcm, view, &view_name, md)?;
    synth_table_backed_mapping(md, &over_view, model, ledger, Some(&view_source))
}

/// A built call as a value.
fn applied(af: p::AppliedFunction) -> p::ValueSpecification {
    p::ValueSpecification::AppliedFunction(Arc::new(af))
}

/// `new CString(value)`.
fn c_string(value: JStr) -> p::ValueSpecification {
    p::ValueSpecification::CString(Arc::new(p::CString {
        value,
        pos: None,
        multi_line: false,
    }))
}

/// `new Variable(name)`.
fn variable(name: &str) -> Arc<p::Variable> {
    Arc::new(p::Variable {
        name: JStr::from(name),
        r#type: None,
        multiplicity: None,
        pos: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{column_pm, join_pm, relational};
    use lite_model::text::j;
    use lite_model::{PropertyMappingEnumeratedColumn, PropertyMappingExpression, RelationalOperationColumnRef};

    fn table(db: &str, t: &str) -> Arc<TableRef> {
        Arc::new(TableRef {
            database: j(db),
            table: j(t),
        })
    }

    #[test]
    fn the_main_table_is_the_one_table_the_direct_pms_read_canonically() {
        let r = relational(
            "a::P",
            None,
            vec![
                column_pm("name", "default.PERSON", "NAME"),
                join_pm("firm", None),
                column_pm("id", "PERSON", "ID"),
            ],
        );
        assert_eq!(infer_main_table(&r), Ok(Some(table("db", "PERSON"))));
        assert_eq!(resolved_main_table(&r), Ok(Some(table("db", "PERSON"))));
        // nothing to infer from
        assert_eq!(
            infer_main_table(&relational("a::P", None, vec![join_pm("firm", None)])),
            Ok(None)
        );
    }

    #[test]
    fn two_tables_are_loud_and_the_quiet_probe_answers_none() {
        let r = relational(
            "a::P",
            None,
            vec![column_pm("name", "PERSON", "NAME"), column_pm("firm", "FIRM", "NAME")],
        );
        // in legend-engine's words, the set's id as written (since 2026-11-15, S390: "Can't find the main table for class
        // 'a::P': property mappings span tables [PERSON, FIRM]. Please specify ...", phase NORMALIZE)
        let e = infer_main_table(&r).unwrap_err();
        assert!(e.is_model());
        assert_eq!(e.phase(), Some(Phase::Mapping));
        assert_eq!(
            e.message(),
            Some(j(
                "Can't find the main table for class 'null'. Please specify a main table using the ~mainTable directive."
            ))
        );
        assert_eq!(infer_main_table_quiet(&r), Ok(None));
    }

    #[test]
    fn a_table_is_one_by_its_name_whatever_database_spells_it() {
        let expr = PropertyMapping::PropertyMappingExpression(Arc::new(PropertyMappingExpression {
            property_name: j("x"),
            expression: RelationalOperation::RelationalOperationColumnRef(Arc::new(RelationalOperationColumnRef {
                database_name: Some(j("db2")),
                table: j("FIRM"),
                column: j("ID"),
            })),
        }));
        // two table names, the engine's refusal (since 2026-11-15, S390: two databases were refused first, "Inconsistent
        // database definitions for the mapping of class 'a::P': [db, db2]")
        let r = relational("a::P", None, vec![column_pm("name", "PERSON", "NAME"), expr]);
        assert_eq!(
            infer_main_table(&r).unwrap_err().message(),
            Some(j(
                "Can't find the main table for class 'null'. Please specify a main table using the ~mainTable directive."
            ))
        );
        // one table name in two databases: the first's (the engine's alias by schema and table)
        let same = PropertyMapping::PropertyMappingExpression(Arc::new(PropertyMappingExpression {
            property_name: j("x"),
            expression: RelationalOperation::RelationalOperationColumnRef(Arc::new(RelationalOperationColumnRef {
                database_name: Some(j("db2")),
                table: j("PERSON"),
                column: j("ID"),
            })),
        }));
        let r3 = relational("a::P", None, vec![column_pm("name", "PERSON", "NAME"), same]);
        assert_eq!(infer_main_table(&r3), Ok(Some(table("db", "PERSON"))));
        // a column ref with no database names no table
        let bare = PropertyMapping::PropertyMappingExpression(Arc::new(PropertyMappingExpression {
            property_name: j("x"),
            expression: RelationalOperation::RelationalOperationColumnRef(Arc::new(RelationalOperationColumnRef {
                database_name: Some(j("")),
                table: j("FIRM"),
                column: j("ID"),
            })),
        }));
        let r2 = relational("a::P", None, vec![column_pm("name", "PERSON", "NAME"), bare]);
        assert_eq!(infer_main_table(&r2), Ok(Some(table("db", "PERSON"))));
    }

    #[test]
    fn the_stamp_is_json_or_the_table_with_its_enum_columns() {
        let mut json = relational("a::P", None, vec![]);
        json.source_url = Some(j("data:application/json,[]"));
        assert_eq!(resolved_main_table(&json), Ok(None));
        assert_eq!(
            relational_source_of(&json),
            Ok(
                MappingDefinitionRelationalSource::MappingDefinitionRelationalSourceJson(Arc::new(
                    MappingDefinitionRelationalSourceJson {
                        url: j("data:application/json,[]")
                    }
                ))
            )
        );
        let ec = |id: Option<&str>| {
            PropertyMapping::PropertyMappingEnumeratedColumn(Arc::new(PropertyMappingEnumeratedColumn {
                property_name: j("kind"),
                enum_mapping_id: id.map(j),
                database: j("db"),
                table: j("PERSON"),
                column: j("KIND"),
            }))
        };
        let r = relational("a::P", None, vec![ec(Some("k")), ec(None)]);
        assert_eq!(
            relational_source_of(&r),
            Ok(
                MappingDefinitionRelationalSource::MappingDefinitionRelationalSourceTable(Arc::new(
                    MappingDefinitionRelationalSourceTable {
                        database: j("db"),
                        table: j("PERSON"),
                        aggregation_aware_main: false,
                        enum_columns: list(vec![Arc::new(MappingDefinitionEnumColumn {
                            table: j("PERSON"),
                            column: j("KIND"),
                            enum_mapping_id: j("k"),
                        })]),
                    }
                ))
            )
        );
        let walled = relational("a::P", None, vec![join_pm("firm", None)]);
        assert_eq!(
            relational_source_of(&walled),
            Err(Thrown::illegal_state(j(
                "stamping a set whose synthesis must have walled: a::P"
            )))
        );
    }
}
