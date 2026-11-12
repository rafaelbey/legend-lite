// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! `FromProtocol`: protocol records to the model -- the boundary that keeps upstream's wire shape out of the
//! compiler. One function per element kind, each the Java's line for line; positions the compiler does not want are
//! dropped here, as Java drops them.

use std::sync::Arc;

use lite_front::jtypes::JStr;
use lite_front::records as p;

use crate::error::{Thrown, illegal_argument, illegal_state, null_pointer, unsupported_operation};
use crate::linked::{LinkedMap, list};
use crate::names::{bare, is_quoted, join_type_from_identifier};
use crate::records::*;
use crate::rel_op_from_protocol as relop;
use crate::text::is;

fn qn(pkg: &JStr, name: &JStr) -> JStr {
    JStr(lite_front::protocol::qualified_name(&pkg.0, &name.0))
}

/// `FromProtocol.stereotypes`: the model's, positions dropped.
pub fn stereotypes(ss: &[Box<p::PStereotype>]) -> Arc<[Arc<StereotypeApplication>]> {
    list(
        ss.iter()
            .map(|s| {
                Arc::new(StereotypeApplication {
                    profile_name: s.profile.clone(),
                    stereotype_name: s.value.clone(),
                })
            })
            .collect(),
    )
}

/// `FromProtocol.taggedValues`.
pub fn tagged_values(ts: &[Box<p::PTaggedValue>]) -> Arc<[Arc<TaggedValue>]> {
    list(
        ts.iter()
            .map(|t| {
                Arc::new(TaggedValue {
                    profile_name: t.tag.profile.clone(),
                    tag_name: t.tag.value.clone(),
                    value: t.value.clone(),
                })
            })
            .collect(),
    )
}

/// `FromProtocol.toFunctionParams`.
pub fn to_function_params(ps: &[Box<p::ParameterDefinition>]) -> Arc<[Arc<FunctionDefinitionParameterDefinition>]> {
    list(
        ps.iter()
            .map(|p| {
                Arc::new(FunctionDefinitionParameterDefinition {
                    name: p.name.clone(),
                    r#type: p.r#type.clone(),
                    multiplicity: p.multiplicity.clone(),
                })
            })
            .collect(),
    )
}

fn boxed<T: Clone>(v: &[Box<T>]) -> Arc<[Box<T>]> {
    list(v.to_vec())
}

/// `FromProtocol.toFunctionDefinition`: the unmangled FQN (the wire name is the emitter's).
pub fn to_function_definition(f: &p::PFunction) -> FunctionDefinition {
    FunctionDefinition {
        qualified_name: qn(&f.pkg, &f.name),
        type_parameters: list(f.type_params.clone()),
        multiplicity_parameters: list(f.mult_params.clone()),
        parameters: to_function_params(&f.parameters),
        return_type: f.return_type.clone(),
        return_multiplicity: f.return_multiplicity.clone(),
        body: list(f.body.clone()),
        stereotypes: stereotypes(&f.stereotypes),
        tagged_values: tagged_values(&f.tagged_values),
        synthesized_from: None,
        pre_constraints: boxed(&f.pre_constraints),
        post_constraints: boxed(&f.post_constraints),
    }
}

/// `FromProtocol.toAssociationDefinition`: two ends; another count refused with the engine's own text.
pub fn to_association_definition(a: &p::PAssociation) -> Result<AssociationDefinition, Thrown> {
    let name = qn(&a.pkg, &a.name);
    let mut ends: Vec<Arc<AssociationDefinitionAssociationEndDefinition>> = a
        .properties
        .iter()
        .map(|p| {
            Arc::new(AssociationDefinitionAssociationEndDefinition {
                property_name: p.name.clone(),
                target_class: p.r#type.clone(),
                multiplicity: p.multiplicity.clone(),
                stereotypes: stereotypes(&p.stereotypes),
                tagged_values: tagged_values(&p.tagged_values),
            })
        })
        .collect();
    if ends.len() != 2 {
        return Err(illegal_state(crate::cat!(
            "Expected 2 properties for an association '",
            name,
            "'"
        )));
    }
    let property2 = ends.pop().expect("two");
    let property1 = ends.pop().expect("two");
    Ok(AssociationDefinition {
        qualified_name: name,
        property1,
        property2,
        derived_properties: boxed(&a.derived_properties),
        stereotypes: stereotypes(&a.stereotypes),
        tagged_values: tagged_values(&a.tagged_values),
    })
}

/// `FromProtocol.toProfileDefinition`: bare names.
pub fn to_profile_definition(p: &p::PProfile) -> ProfileDefinition {
    ProfileDefinition {
        qualified_name: qn(&p.pkg, &p.name),
        stereotypes: list(p.stereotypes.iter().map(|e| e.value.clone()).collect()),
        tags: list(p.tags.iter().map(|e| e.value.clone()).collect()),
    }
}

/// `FromProtocol.toEnumDefinition`: the value names, and the annotations of the values that have some.
pub fn to_enum_definition(e: &p::PEnumeration) -> EnumDefinition {
    let mut names = Vec::with_capacity(e.values.len());
    let mut annotated = Vec::new();
    for v in &e.values {
        names.push(v.value.clone());
        if !v.stereotypes.is_empty() || !v.tagged_values.is_empty() {
            annotated.push(Arc::new(EnumDefinitionValueAnnotations {
                value: v.value.clone(),
                stereotypes: stereotypes(&v.stereotypes),
                tagged_values: tagged_values(&v.tagged_values),
            }));
        }
    }
    EnumDefinition {
        qualified_name: qn(&e.pkg, &e.name),
        values: list(names),
        stereotypes: stereotypes(&e.stereotypes),
        tagged_values: tagged_values(&e.tagged_values),
        value_annotations: list(annotated),
    }
}

/// `FromProtocol.toDatabaseDefinition`: the default schema first in the flat lookup lists (a bare name means it),
/// `schemas` in the wire's order without the synthetic default; a table in a schema's list and the flat list is one
/// record (identity: `defaultSchemaTables`).
pub fn to_database_definition(db: &p::PDatabase) -> Result<DatabaseDefinition, Thrown> {
    let name = qn(&db.pkg, &db.name);
    let includes: Vec<JStr> = db.included_stores.iter().map(|p| p.path.clone()).collect();
    let mut schemas = Vec::new();
    let mut flat_tables = Vec::new();
    let mut flat_views = Vec::new();
    let mut flat_functions = Vec::new();
    let lookup_order = db
        .schemas
        .iter()
        .filter(|s| is(&s.name.0, "default"))
        .chain(db.schemas.iter().filter(|s| !is(&s.name.0, "default")));
    for s in lookup_order {
        let mut st = Vec::new();
        let mut sv = Vec::new();
        for tb in &s.tables {
            let d = Arc::new(table(tb, false, is_quoted(&s.name.0))?);
            st.push(d.clone());
            flat_tables.push(d);
        }
        let mut sf = Vec::new();
        for tb in &s.tabular_functions {
            let d = Arc::new(table(tb, true, is_quoted(&s.name.0))?);
            sf.push(d.clone());
            flat_functions.push(d);
        }
        for vw in &s.views {
            let d = Arc::new(view(vw, Some(&name))?);
            sv.push(d.clone());
            flat_views.push(d);
        }
        if !is(&s.name.0, "default") {
            schemas.push(Arc::new(DatabaseDefinitionSchemaDefinition {
                name: bare(&s.name),
                tables: list(st),
                views: list(sv),
                tabular_functions: list(sf),
            }));
        }
    }
    let mut joins = Vec::with_capacity(db.joins.len());
    for j in &db.joins {
        joins.push(Arc::new(DatabaseDefinitionJoinDefinition {
            name: j.name.clone(),
            operation: relop::op(&j.operation, Some(&name))?,
        }));
    }
    let mut filters = Vec::new();
    let mut multi_grain = Vec::new();
    for f in &db.filters {
        let d = Arc::new(DatabaseDefinitionFilterDefinition {
            name: f.name.clone(),
            condition: relop::op(&f.operation, Some(&name))?,
        });
        if is(&f.filter_type.0, "multigrain") {
            multi_grain.push(d);
        } else {
            filters.push(d);
        }
    }
    Ok(DatabaseDefinition {
        qualified_name: name,
        includes: list(includes),
        schemas: list(schemas),
        tables: list(flat_tables),
        views: list(flat_views),
        tabular_functions: list(flat_functions),
        joins: list(joins),
        filters: list(filters),
        multi_grain_filters: list(multi_grain),
    })
}

/// A table, its name bare and its quoting remembered (its own, and its schema's).
fn table(t: &p::PDbTable, function: bool, schema_quoted: bool) -> Result<DatabaseDefinitionTableDefinition, Thrown> {
    let mut cols = Vec::with_capacity(t.columns.len());
    for c in &t.columns {
        cols.push(Arc::new(DatabaseDefinitionColumnDefinition {
            name: bare(&c.name),
            data_type: data_type(&c.r#type)?,
            primary_key: t.primary_key.contains(&c.name),
            not_null: !c.nullable,
            quoted: is_quoted(&c.name.0),
        }));
    }
    Ok(DatabaseDefinitionTableDefinition {
        name: bare(&t.name),
        columns: list(cols),
        milestoning: milestoning(&t.milestoning),
        function,
        quoted: is_quoted(&t.name.0),
        schema_quoted,
    })
}

/// The wire's list of milestoning entries, one per dimension, as one record with a business and a processing slot.
fn milestoning(ms: &[p::PMilestoning]) -> Option<Arc<DatabaseDefinitionTableDefinitionMilestoning>> {
    if ms.is_empty() {
        return None;
    }
    let mut business = None;
    let mut processing = None;
    for m in ms {
        match m {
            p::PMilestoning::PBusinessMilestoning(b) => {
                business = Some(Arc::new(DatabaseDefinitionTableDefinitionMilestoningBusiness {
                    from: Some(b.from.clone()),
                    thru: Some(b.thru.clone()),
                    thru_is_inclusive: b.thru_is_inclusive,
                    snapshot_date: None,
                    infinity_date: b.infinity_date.as_ref().map(|d| d.value.clone()),
                }));
            }
            p::PMilestoning::PBusinessSnapshotMilestoning(b) => {
                business = Some(Arc::new(DatabaseDefinitionTableDefinitionMilestoningBusiness {
                    from: None,
                    thru: None,
                    thru_is_inclusive: false,
                    snapshot_date: Some(b.snapshot_date.clone()),
                    infinity_date: None,
                }));
            }
            p::PMilestoning::PProcessingMilestoning(pm) => {
                processing = Some(Arc::new(DatabaseDefinitionTableDefinitionMilestoningProcessing {
                    r#in: Some(pm.r#in.clone()),
                    out: Some(pm.out.clone()),
                    out_is_inclusive: pm.out_is_inclusive,
                    snapshot_date: None,
                    infinity_date: pm.infinity_date.as_ref().map(|d| d.value.clone()),
                }));
            }
            p::PMilestoning::PProcessingSnapshotMilestoning(pm) => {
                processing = Some(Arc::new(DatabaseDefinitionTableDefinitionMilestoningProcessing {
                    r#in: None,
                    out: None,
                    out_is_inclusive: false,
                    snapshot_date: Some(pm.snapshot_date.clone()),
                    infinity_date: None,
                }));
            }
        }
    }
    Some(Arc::new(DatabaseDefinitionTableDefinitionMilestoning {
        business,
        processing,
    }))
}

fn view(v: &p::PDbView, enclosing_db: Option<&JStr>) -> Result<DatabaseDefinitionViewDefinition, Thrown> {
    let mut cms = Vec::with_capacity(v.column_mappings.len());
    for cm in &v.column_mappings {
        cms.push(Arc::new(DatabaseDefinitionViewDefinitionViewColumnMapping {
            name: cm.name.clone(),
            target_set_id: None,
            expression: relop::op(&cm.operation, enclosing_db)?,
            primary_key: v.primary_key.contains(&cm.name),
        }));
    }
    let mut group_by = Vec::with_capacity(v.group_by.len());
    for g in &v.group_by {
        group_by.push(relop::op(g, enclosing_db)?);
    }
    Ok(DatabaseDefinitionViewDefinition {
        name: v.name.clone(),
        filter: filter_mapping(v.filter.as_deref())?,
        group_by_columns: list(group_by),
        distinct: v.distinct,
        column_mappings: list(cms),
    })
}

/// A view's filter: Local or Cross by whether the source named a database; join-mediated when it walks joins.
fn filter_mapping(f: Option<&p::PViewFilter>) -> Result<Option<FilterMapping>, Thrown> {
    let Some(f) = f else {
        return Ok(None);
    };
    let ptr = match &f.db {
        None => FilterPointer::FilterPointerLocal(Arc::new(FilterPointerLocal { name: f.name.clone() })),
        Some(db) => FilterPointer::FilterPointerCross(Arc::new(FilterPointerCross {
            db: db.clone(),
            name: f.name.clone(),
        })),
    };
    if f.joins.is_empty() {
        return Ok(Some(FilterMapping::FilterMappingDirect(Arc::new(
            FilterMappingDirect { filter: ptr },
        ))));
    }
    let mut chain = Vec::with_capacity(f.joins.len());
    let mut source_db: Option<JStr> = None;
    for jp in &f.joins {
        if source_db.is_none() {
            source_db = jp.db.clone();
        }
        chain.push(Arc::new(JoinChainElement {
            join_name: jp.name.clone(),
            join_type: match &jp.join_type {
                None => None,
                Some(t) => Some(join_type_from_identifier(t)?),
            },
            database_name: jp.db.clone(),
            include_self: false,
        }));
    }
    Ok(Some(join_mediated(source_db, chain, ptr, None)?))
}

/// `new FilterMapping.JoinMediated(sourceDb, joins, filter, joinType)`: a source database, at least one hop.
pub fn join_mediated(
    source_db: Option<JStr>,
    joins: Vec<Arc<JoinChainElement>>,
    filter: FilterPointer,
    join_type: Option<JStr>,
) -> Result<FilterMapping, Thrown> {
    if source_db.is_none() {
        return Err(null_pointer(Some(
            "Source database cannot be null for join-mediated filter",
        )));
    }
    if joins.is_empty() {
        return Err(illegal_argument(JStr::from(
            "Join-mediated filter must have at least one join hop; use Direct for unmediated filter references",
        )));
    }
    Ok(FilterMapping::FilterMappingJoinMediated(Arc::new(
        FilterMappingJoinMediated {
            source_db,
            joins: list(joins),
            filter,
            join_type,
        },
    )))
}

/// The wire's data type kind to the model's (`Json` folds to `SemiStructured`, a bare `VARCHAR` is unbounded).
fn data_type(t: &p::PDbType) -> Result<RelationalDataType, Thrown> {
    use RelationalDataType as T;
    // Java's Number.intValue() of a long: its low 32 bits
    let int = |v: Option<i64>| v.map_or(0, |n| n as i32);
    let size = int(t.size);
    let k = &t.kind.0;
    Ok(if is(k, "BigInt") {
        T::RelationalDataTypeBigInt(Arc::new(RelationalDataTypeBigInt {}))
    } else if is(k, "SmallInt") {
        T::RelationalDataTypeSmallInt(Arc::new(RelationalDataTypeSmallInt {}))
    } else if is(k, "TinyInt") {
        T::RelationalDataTypeTinyInt(Arc::new(RelationalDataTypeTinyInt {}))
    } else if is(k, "Integer") {
        T::RelationalDataTypeInteger_(Arc::new(RelationalDataTypeInteger_ {}))
    } else if is(k, "Float") {
        T::RelationalDataTypeFloat_(Arc::new(RelationalDataTypeFloat_ {}))
    } else if is(k, "Double") {
        T::RelationalDataTypeDouble_(Arc::new(RelationalDataTypeDouble_ {}))
    } else if is(k, "Real") {
        T::RelationalDataTypeReal(Arc::new(RelationalDataTypeReal {}))
    } else if is(k, "Bit") {
        T::RelationalDataTypeBit(Arc::new(RelationalDataTypeBit {}))
    } else if is(k, "Timestamp") {
        T::RelationalDataTypeTimestamp(Arc::new(RelationalDataTypeTimestamp {}))
    } else if is(k, "Time") {
        T::RelationalDataTypeTime(Arc::new(RelationalDataTypeTime {}))
    } else if is(k, "TimestampNs") {
        T::RelationalDataTypeTimestampNs(Arc::new(RelationalDataTypeTimestampNs {}))
    } else if is(k, "Date") {
        T::RelationalDataTypeDate_(Arc::new(RelationalDataTypeDate_ {}))
    } else if is(k, "Distinct") {
        T::RelationalDataTypeDistinct(Arc::new(RelationalDataTypeDistinct {}))
    } else if is(k, "Other") {
        T::RelationalDataTypeOther(Arc::new(RelationalDataTypeOther {}))
    } else if is(k, "SemiStructured") || is(k, "Json") {
        T::RelationalDataTypeSemiStructured(Arc::new(RelationalDataTypeSemiStructured {}))
    } else if is(k, "Varchar") {
        T::RelationalDataTypeVarchar(Arc::new(RelationalDataTypeVarchar {
            size: if t.size.is_none() { i32::MAX } else { size },
        }))
    } else if is(k, "Char") {
        T::RelationalDataTypeChar_(Arc::new(RelationalDataTypeChar_ { size }))
    } else if is(k, "Binary") {
        T::RelationalDataTypeBinary(Arc::new(RelationalDataTypeBinary { size }))
    } else if is(k, "Varbinary") {
        T::RelationalDataTypeVarbinary(Arc::new(RelationalDataTypeVarbinary { size }))
    } else if is(k, "Decimal") {
        T::RelationalDataTypeDecimal(Arc::new(RelationalDataTypeDecimal {
            precision: int(t.precision),
            scale: int(t.scale),
        }))
    } else if is(k, "Numeric") {
        T::RelationalDataTypeNumeric(Arc::new(RelationalDataTypeNumeric {
            precision: int(t.precision),
            scale: int(t.scale),
        }))
    } else {
        return Err(unsupported_operation(crate::cat!(
            "no model data type for protocol kind '",
            t.kind,
            "'"
        )));
    })
}

/// `FromProtocol.toClassDefinition`.
pub fn to_class_definition(c: &p::PClass) -> ClassDefinition {
    let props = c
        .properties
        .iter()
        .map(|p| {
            Arc::new(ClassDefinitionPropertyDefinition {
                name: p.name.clone(),
                r#type: p.r#type.clone(),
                multiplicity: p.multiplicity.clone(),
                stereotypes: stereotypes(&p.stereotypes),
                tagged_values: tagged_values(&p.tagged_values),
                has_default: p.default_value.is_some(),
                default_value: p.default_value.as_ref().and_then(|d| d.value.clone()),
            })
        })
        .collect();
    ClassDefinition {
        qualified_name: qn(&c.pkg, &c.name),
        type_params: list(c.type_params.clone()),
        type_variables: boxed(&c.type_variables),
        super_classes: list(c.super_types.iter().map(|s| s.r#type.clone()).collect()),
        properties: list(props),
        derived_properties: boxed(&c.derived_properties),
        constraints: boxed(&c.constraints),
        stereotypes: stereotypes(&c.stereotypes),
        tagged_values: tagged_values(&c.tagged_values),
        is_native: c.is_native,
        multiplicity_params: list(c.multiplicity_params.clone()),
    }
}

fn generic(section: &str, kind: &str, qualified_name: JStr, fields: Vec<(JStr, JStr)>) -> PackageableElement {
    PackageableElement::GenericSectionElementDefinition(Arc::new(GenericSectionElementDefinition {
        section: JStr::from(section),
        kind: JStr::from(kind),
        qualified_name,
        fields: list(fields),
        body_source: None,
    }))
}

/// `FromProtocol.toConnectionElement`: one of the packageable connection kinds.
pub fn to_connection_element(c: &p::PConnection) -> Result<PackageableElement, Thrown> {
    let name = qn(&c.pkg, &c.name);
    Ok(match &c.value {
        p::PConnectionValue::PJsonModelConnection(j) => {
            PackageableElement::ModelConnectionDefinition(Arc::new(ModelConnectionDefinition {
                qualified_name: name,
                kind: ModelConnectionDefinitionKind::JSON,
                class_name: j.class_name.clone(),
                url: j.url.clone(),
            }))
        }
        p::PConnectionValue::PXmlModelConnection(x) => {
            PackageableElement::ModelConnectionDefinition(Arc::new(ModelConnectionDefinition {
                qualified_name: name,
                kind: ModelConnectionDefinitionKind::XML,
                class_name: x.class_name.clone(),
                url: x.url.clone(),
            }))
        }
        p::PConnectionValue::PModelChainConnection(m) => {
            PackageableElement::ModelChainConnectionDefinition(Arc::new(ModelChainConnectionDefinition {
                qualified_name: name,
                mappings: list(m.mappings.clone()),
            }))
        }
        p::PConnectionValue::PRelationalDatabaseConnection(r) => {
            PackageableElement::ConnectionDefinition(Arc::new(to_relational_connection(name, r)?))
        }
        p::PConnectionValue::PServiceStoreConnection(sc) => generic(
            "Connection",
            "ServiceStoreConnection",
            name,
            vec![(JStr::from("baseUrl"), sc.base_url.clone())],
        ),
        p::PConnectionValue::PDeephavenConnection(dc) => generic(
            "Connection",
            "DeephavenConnection",
            name,
            vec![(JStr::from("serverUrl"), dc.server_url.clone())],
        ),
        p::PConnectionValue::PMongoDbConnection(mgc) => {
            PackageableElement::ConnectionDefinition(Arc::new(crate::mongo_declarations::to_connection(name, mgc)?))
        }
        p::PConnectionValue::PElasticsearchConnection(esc) => generic(
            "Connection",
            "Elasticsearch7ClusterConnection",
            name,
            vec![(JStr::from("url"), esc.url.clone())],
        ),
        p::PConnectionValue::PConnectionPointer(_) => {
            return Err(illegal_state(crate::cat!(
                "a connection pointer cannot be a standalone element: ",
                name
            )));
        }
    })
}

/// `FromProtocol.toRuntimeElement`: the bindings in declaration order, several connections per store; an embedded
/// island hoisted to an element named with the reserved `$` sigil and bound to its store, a JSON island kept for the
/// cross-bake list.
pub fn to_runtime_element(r: &p::PRuntime) -> Result<RuntimeDefinition, Thrown> {
    let qn_ = qn(&r.pkg, &r.name);
    let mappings: Vec<JStr> = r.mappings.iter().map(|p| p.path.clone()).collect();
    let mut bindings: LinkedMap<JStr, Vec<JStr>> = LinkedMap::new();
    let mut json_connections = Vec::new();
    let mut inline: Vec<PackageableElement> = Vec::new();
    let mut connection_ids: LinkedMap<JStr, JStr> = LinkedMap::new();
    for sc in &r.connections {
        let store = sc.store.path.clone();
        for ic in &sc.store_connections {
            connection_ids.put(ic.id.clone(), store.clone());
            let synth = || crate::cat!(qn_, "$", store, "$", ic.id);
            match &ic.connection {
                p::PConnectionValue::PConnectionPointer(p) => {
                    bindings
                        .get_or_insert_with(store.clone(), Vec::new)
                        .push(p.connection.clone());
                }
                p::PConnectionValue::PJsonModelConnection(j) => {
                    json_connections.push(Arc::new(JsonModelConnection {
                        class_name: j.class_name.clone(),
                        url: j.url.clone(),
                    }));
                }
                p::PConnectionValue::PXmlModelConnection(x) => {
                    let s = synth();
                    inline.push(PackageableElement::ModelConnectionDefinition(Arc::new(
                        ModelConnectionDefinition {
                            qualified_name: s.clone(),
                            kind: ModelConnectionDefinitionKind::XML,
                            class_name: x.class_name.clone(),
                            url: x.url.clone(),
                        },
                    )));
                    bindings.get_or_insert_with(store.clone(), Vec::new).push(s);
                }
                p::PConnectionValue::PModelChainConnection(m) => {
                    let s = synth();
                    inline.push(PackageableElement::ModelChainConnectionDefinition(Arc::new(
                        ModelChainConnectionDefinition {
                            qualified_name: s.clone(),
                            mappings: list(m.mappings.clone()),
                        },
                    )));
                    bindings.get_or_insert_with(store.clone(), Vec::new).push(s);
                }
                p::PConnectionValue::PRelationalDatabaseConnection(rc) => {
                    let s = synth();
                    let mut cd = to_relational_connection(s.clone(), rc)?;
                    if cd.store_name.is_none() {
                        // an inline island may omit store: the outer binding names it
                        cd.store_name = Some(store.clone());
                    }
                    inline.push(PackageableElement::ConnectionDefinition(Arc::new(cd)));
                    bindings.get_or_insert_with(store.clone(), Vec::new).push(s);
                }
                p::PConnectionValue::PServiceStoreConnection(scv) => {
                    let s = synth();
                    inline.push(generic(
                        "Connection",
                        "ServiceStoreConnection",
                        s.clone(),
                        vec![(JStr::from("baseUrl"), scv.base_url.clone())],
                    ));
                    bindings.get_or_insert_with(store.clone(), Vec::new).push(s);
                }
                p::PConnectionValue::PDeephavenConnection(dcv) => {
                    let s = synth();
                    inline.push(generic(
                        "Connection",
                        "DeephavenConnection",
                        s.clone(),
                        vec![(JStr::from("serverUrl"), dcv.server_url.clone())],
                    ));
                    bindings.get_or_insert_with(store.clone(), Vec::new).push(s);
                }
                p::PConnectionValue::PMongoDbConnection(mcv) => {
                    let s = synth();
                    inline.push(PackageableElement::ConnectionDefinition(Arc::new(
                        crate::mongo_declarations::to_connection(s.clone(), mcv)?,
                    )));
                    bindings.get_or_insert_with(store.clone(), Vec::new).push(s);
                }
                p::PConnectionValue::PElasticsearchConnection(ecv) => {
                    let s = synth();
                    inline.push(generic(
                        "Connection",
                        "Elasticsearch7ClusterConnection",
                        s.clone(),
                        vec![(JStr::from("url"), ecv.url.clone())],
                    ));
                    bindings.get_or_insert_with(store.clone(), Vec::new).push(s);
                }
            }
        }
    }
    // connectionStores: [conn: [stores...]], the reverse direction
    for cs in &r.connection_stores {
        let p::PConnectionValue::PConnectionPointer(p) = &cs.connection_pointer else {
            return Err(illegal_state(crate::cat!(
                "connectionStores entries are pointers by grammar: ",
                qn_
            )));
        };
        for sp in &cs.store_pointers {
            bindings
                .get_or_insert_with(sp.path.clone(), Vec::new)
                .push(p.connection.clone());
        }
    }
    Ok(RuntimeDefinition {
        qualified_name: qn_,
        mappings: list(mappings),
        connection_bindings: bindings.freeze_with(list),
        json_connections: list(json_connections),
        inline_connections: list(inline),
        connection_ids: connection_ids.freeze(),
    })
}

/// `FromProtocol.toServiceSectionElement`: a Service or an ExecutionEnvironment.
pub fn to_service_section_element(e: &p::Element) -> Result<PackageableElement, Thrown> {
    Ok(match e {
        p::Element::PService(s) => PackageableElement::ServiceDefinition(Arc::new(to_service_definition(s)?)),
        p::Element::PExecutionEnvironment(ee) => {
            let mut singles: Vec<&p::PKeyedExecution> = Vec::new();
            for x in &ee.executions {
                match x {
                    p::PExecutionParameters::PKeyedExecution(k) => singles.push(k),
                    p::PExecutionParameters::PMultiKeyedExecution(m) => singles.extend(m.singles.iter().map(|b| &**b)),
                }
            }
            PackageableElement::ExecutionEnvironmentDefinition(Arc::new(ExecutionEnvironmentDefinition {
                qualified_name: qn(&ee.pkg, &ee.name),
                executions: keyed_executions(&singles),
            }))
        }
        other => {
            return Err(illegal_state(crate::cat!(
                "not a service-section element: ",
                java_class_name(other)
            )));
        }
    })
}

fn to_service_definition(s: &p::PService) -> Result<ServiceDefinition, Thrown> {
    let Some(pattern) = s.pattern.clone() else {
        return Err(null_pointer(Some("service pattern")));
    };
    let name = qn(&s.pkg, &s.name);
    let test_suites = s.test_suites.as_ref().map(|t| boxed(t));
    Ok(match &s.execution {
        p::PServiceExecution::PSingleExecution(single) => ServiceDefinition {
            qualified_name: name,
            pattern,
            function_body: model_query(&single.query),
            documentation: s.documentation.clone(),
            mapping_ref: single.mapping.clone(),
            runtime_ref: single.runtime.clone(),
            test_suites,
            owners: list(s.owners.clone()),
            auto_activate_updates: Some(s.auto_activate_updates),
            multi_execution: None,
            test: s.test.clone(),
        },
        p::PServiceExecution::PMultiExecution(multi) => {
            let execs: Vec<&p::PKeyedExecution> = match &multi.executions {
                None => Vec::new(),
                Some(v) => v.iter().map(|b| &**b).collect(),
            };
            ServiceDefinition {
                qualified_name: name,
                pattern,
                function_body: model_query(&multi.query),
                documentation: s.documentation.clone(),
                mapping_ref: None,
                runtime_ref: None,
                test_suites,
                owners: list(s.owners.clone()),
                auto_activate_updates: Some(s.auto_activate_updates),
                multi_execution: Some(Arc::new(ServiceDefinitionMultiExecution {
                    key: multi.execution_key.clone().unwrap_or_default(),
                    executions: keyed_executions(&execs),
                })),
                test: s.test.clone(),
            }
        }
    })
}

/// The protocol query is always a lambda; the model keeps its body when it is a zero-parameter, one-statement one.
fn model_query(q: &p::ValueSpecification) -> p::ValueSpecification {
    if let p::ValueSpecification::LambdaFunction(lf) = q
        && lf.parameters.is_empty()
        && lf.body.len() == 1
    {
        return lf.body[0].clone();
    }
    q.clone()
}

fn keyed_executions(ks: &[&p::PKeyedExecution]) -> Arc<[Arc<ServiceDefinitionKeyedExecution>]> {
    list(
        ks.iter()
            .map(|k| {
                Arc::new(ServiceDefinitionKeyedExecution {
                    key_value: k.key_value.clone(),
                    mapping: k.mapping.clone(),
                    runtime: k.runtime.clone(),
                })
            })
            .collect(),
    )
}

/// `FromProtocol.toMeasureDefinition`.
pub fn to_measure_definition(m: &p::PMeasure) -> MeasureDefinition {
    MeasureDefinition {
        qualified_name: qn(&m.pkg, &m.name),
        canonical_unit: m.canonical_unit.as_ref().map(|u| to_unit(u)),
        non_canonical_units: list(m.non_canonical_units.iter().map(|u| to_unit(u)).collect()),
    }
}

fn to_unit(u: &p::PUnit) -> Arc<MeasureDefinitionUnit> {
    Arc::new(MeasureDefinitionUnit {
        name: u.name.clone(),
        param_name: u.param_name.clone(),
        body: u.body.clone(),
    })
}

/// `FromProtocol.toPersistenceElement`.
pub fn to_persistence_element(e: &p::Element) -> Result<PackageableElement, Thrown> {
    let marker = |present: bool, text: &str| if present { Some(JStr::from(text)) } else { None };
    Ok(match e {
        p::Element::PPersistence(p) => PackageableElement::PersistenceDefinition(Arc::new(PersistenceDefinition {
            qualified_name: qn(&p.pkg, &p.name),
            doc: p.doc.clone(),
            trigger_source: Some(p.trigger_kind.clone()),
            service: p.service.clone(),
            persister_source: p.persister.as_ref().map(|x| x.kind.clone()),
            service_output_targets_source: marker(p.service_output_targets.is_some(), "<targets>"),
            notifier_source: marker(p.notifier.is_some(), "<notifier>"),
            tests_source: marker(p.tests.is_some(), "<tests>"),
        })),
        p::Element::PPersistenceContext(p) => {
            PackageableElement::PersistenceContextDefinition(Arc::new(PersistenceContextDefinition {
                qualified_name: qn(&p.pkg, &p.name),
                persistence: p.persistence.clone(),
                platform_source: p.platform.as_ref().map(|x| x.kind.clone()),
                service_parameters_source: marker(!p.service_parameters.is_empty(), "<serviceParameters>"),
                sink_connection_source: marker(p.sink_connection.is_some(), "<sinkConnection>"),
            }))
        }
        other => {
            return Err(illegal_state(crate::cat!(
                "not a persistence-section element: ",
                java_class_name(other)
            )));
        }
    })
}

/// `FromProtocol.toDataSpaceDefinition`: decorations ride the protocol record only.
pub fn to_data_space_definition(d: &p::PDataSpace) -> DataSpaceDefinition {
    let contexts = d.execution_contexts.as_deref().unwrap_or(&[]);
    let contexts = contexts
        .iter()
        .map(|ctx| {
            Arc::new(DataSpaceDefinitionExecutionContext {
                name: ctx.name.clone(),
                title: ctx.title.clone(),
                description: ctx.description.clone(),
                mapping: ctx.mapping.clone(),
                default_runtime: ctx.default_runtime.clone(),
                test_data_source: ctx.test_data.as_ref().map(|t| crate::cat!(t.kind, " #{", t.path, "}#")),
            })
        })
        .collect();
    let executables = d
        .executables
        .as_deref()
        .unwrap_or(&[])
        .iter()
        .map(|e| {
            Arc::new(DataSpaceDefinitionExecutable {
                id: e.id.clone(),
                title: e.title.clone(),
                description: e.description.clone(),
                executable: e.executable.clone(),
                query_source: e.query.as_ref().map(|_| JStr::from("<query>")),
                execution_context_key: e.execution_context_key.clone(),
            })
        })
        .collect();
    let diagrams = d
        .diagrams
        .as_deref()
        .unwrap_or(&[])
        .iter()
        .map(|g| {
            Arc::new(DataSpaceDefinitionDiagram {
                title: g.title.clone(),
                description: g.description.clone(),
                diagram: g.diagram.clone(),
            })
        })
        .collect();
    let element_scope = d
        .elements
        .as_deref()
        .unwrap_or(&[])
        .iter()
        .map(|r| {
            if r.exclude {
                crate::cat!("-", r.path)
            } else {
                r.path.clone()
            }
        })
        .collect();
    let support_info = match &d.support_info {
        None => None,
        Some(p::PDataSpaceSupport::PDataSpaceSupportPSupportEmail(e)) => Some(crate::cat!("Email ", e.address)),
        Some(p::PDataSpaceSupport::PDataSpaceSupportPSupportCombined(_)) => Some(JStr::from("Combined")),
        Some(p::PDataSpaceSupport::PDataSpaceSupportPSupportFull(_)) => Some(JStr::from("Full")),
    };
    DataSpaceDefinition {
        qualified_name: qn(&d.pkg, &d.name),
        execution_contexts: list(contexts),
        default_execution_context: d.default_execution_context.clone(),
        title: d.title.clone(),
        description: d.description.clone(),
        executables: list(executables),
        diagrams: list(diagrams),
        support_info_source: support_info,
        elements: list(element_scope),
        stereotypes: stereotypes(&d.stereotypes),
    }
}

/// A relational database connection: its database type (a name the model does not know refused, listing the ones it
/// does), its datasource and its authentication folded to the model's shapes.
fn to_relational_connection(
    qualified_name: JStr,
    r: &p::PRelationalDatabaseConnection,
) -> Result<ConnectionDefinition, Thrown> {
    use ConnectionSpecification as S;
    let Some(database_type) = ConnectionDefinitionDatabaseType::value_of(&r.database_type.0) else {
        let names: Vec<&str> = ConnectionDefinitionDatabaseType::VALUES
            .iter()
            .map(|v| v.name())
            .collect();
        return Err(Thrown::Connection(crate::cat!(
            "unknown database type '",
            r.database_type,
            "' (expected one of [",
            names.join(", "),
            "])"
        )));
    };
    let static_ds = |host: JStr, port: i32, database: JStr| {
        S::ConnectionSpecificationStaticDatasource(Arc::new(ConnectionSpecificationStaticDatasource {
            host,
            port,
            database,
        }))
    };
    let in_memory_or_file = |path: &Option<JStr>| match path {
        None => S::ConnectionSpecificationInMemory(Arc::new(ConnectionSpecificationInMemory {})),
        Some(path) => {
            S::ConnectionSpecificationLocalFile(Arc::new(ConnectionSpecificationLocalFile { path: path.clone() }))
        }
    };
    let spec = match &r.datasource_specification {
        p::PDatasourceSpec::PH2Local(h) => {
            S::ConnectionSpecificationLocalH2(Arc::new(ConnectionSpecificationLocalH2 {
                url: None,
                test_data_setup_csv: h.test_data_setup_csv.clone(),
                test_data_setup_sqls: h.test_data_setup_sqls.as_ref().map(|v| list(v.clone())),
            }))
        }
        p::PDatasourceSpec::PStaticSpec(s) => static_ds(s.host.clone(), s.port as i32, s.database_name.clone()),
        p::PDatasourceSpec::PH2EmbeddedSpec(eh) => {
            S::ConnectionSpecificationEmbeddedH2(Arc::new(ConnectionSpecificationEmbeddedH2 {
                database_name: eh.database_name.clone(),
                directory: eh.directory.clone(),
                auto_server_mode: eh.auto_server_mode,
            }))
        }
        p::PDatasourceSpec::PDuckDBSpec(dd) => in_memory_or_file(&dd.path),
        p::PDatasourceSpec::PSQLiteSpec(sq) => in_memory_or_file(&sq.path),
        p::PDatasourceSpec::PSnowflakeSpec(s) => {
            S::ConnectionSpecificationSnowflake(Arc::new(ConnectionSpecificationSnowflake {
                database_name: s.database_name.clone(),
                account_name: s.account_name.clone(),
                warehouse_name: s.warehouse_name.clone(),
                region: s.region.clone(),
                account_type: s.account_type.clone(),
                cloud_type: s.cloud_type.clone(),
                enable_query_tags: s.enable_query_tags,
                organization: s.organization.clone(),
                role: s.role.clone(),
            }))
        }
        p::PDatasourceSpec::PSpannerSpec(s) => {
            S::ConnectionSpecificationSpanner(Arc::new(ConnectionSpecificationSpanner {
                project_id: s.project_id.clone(),
                instance_id: s.instance_id.clone(),
                database_id: s.database_id.clone(),
            }))
        }
        p::PDatasourceSpec::PDatabricksSpec(s) => {
            S::ConnectionSpecificationDatabricks(Arc::new(ConnectionSpecificationDatabricks {
                hostname: s.hostname.clone(),
                port: s.port.clone(),
                protocol: s.protocol.clone(),
                http_path: s.http_path.clone(),
            }))
        }
        p::PDatasourceSpec::PBigQuerySpec(s) => {
            S::ConnectionSpecificationBigQuery(Arc::new(ConnectionSpecificationBigQuery {
                project_id: s.project_id.clone(),
                default_dataset: s.default_dataset.clone(),
            }))
        }
        p::PDatasourceSpec::PRedshiftSpec(s) => static_ds(s.host.clone(), s.port as i32, s.database_name.clone()),
        p::PDatasourceSpec::PTrinoSpec(s) => S::ConnectionSpecificationTrino(Arc::new(ConnectionSpecificationTrino {
            host: s.host.clone(),
            port: s.port as i32,
            catalog: s.catalog.clone(),
            schema: s.schema.clone(),
            client_tags: s.client_tags.clone(),
            ssl: s.ssl_specification.as_ref().is_some_and(|x| x.ssl),
        })),
        p::PDatasourceSpec::PAuroraSpec(s) => static_ds(s.host.clone(), s.port as i32, s.name.clone()),
        p::PDatasourceSpec::PGlobalAuroraSpec(s) => static_ds(s.host.clone(), s.port as i32, s.name.clone()),
        p::PDatasourceSpec::PMemSqlSpec(s) => static_ds(
            s.host.clone(),
            s.port as i32,
            s.database_name.clone().unwrap_or_default(),
        ),
        p::PDatasourceSpec::POracleSpec(s) => static_ds(
            s.host.clone(),
            s.port as i32,
            s.service_name.clone().unwrap_or_default(),
        ),
        p::PDatasourceSpec::PAthenaSpec(s) => static_ds(
            s.athena_endpoint.clone().unwrap_or_else(|| s.region.clone()),
            0,
            s.database.clone().unwrap_or_default(),
        ),
    };
    use AuthenticationSpec as A;
    let auth = match &r.authentication_strategy {
        p::PAuthStrategy::PH2Default(_) => A::AuthenticationSpecDefaultH2(Arc::new(AuthenticationSpecDefaultH2 {})),
        p::PAuthStrategy::PTestAuth(_) => A::AuthenticationSpecTestAuth(Arc::new(AuthenticationSpecTestAuth {})),
        p::PAuthStrategy::PDelegatedKerberos(k) => {
            A::AuthenticationSpecDelegatedKerberos(Arc::new(AuthenticationSpecDelegatedKerberos {
                server_principal: k.server_principal.clone(),
                kerberos_remote_service_name: None,
                kerberos_use_canonical_hostname: None,
            }))
        }
        p::PAuthStrategy::PUserNamePassword(u) => {
            A::AuthenticationSpecVaultUserNamePassword(Arc::new(AuthenticationSpecVaultUserNamePassword {
                base_vault_reference: u.base_vault_reference.clone(),
                user_name_vault_reference: u.user_name_vault_reference.clone(),
                password_vault_reference: u.password_vault_reference.clone(),
            }))
        }
        p::PAuthStrategy::PSnowflakePublic(s) => {
            A::AuthenticationSpecSnowflakePublic(Arc::new(AuthenticationSpecSnowflakePublic {
                public_user_name: s.public_user_name.clone(),
                private_key_vault_reference: s.private_key_vault_reference.clone(),
                pass_phrase_vault_reference: s.pass_phrase_vault_reference.clone(),
            }))
        }
        p::PAuthStrategy::PGCPApplicationDefaultCredentials(_) => {
            A::AuthenticationSpecGCPApplicationDefaultCredentials(Arc::new(
                AuthenticationSpecGCPApplicationDefaultCredentials {},
            ))
        }
        p::PAuthStrategy::PApiToken(a) => A::AuthenticationSpecApiToken(Arc::new(AuthenticationSpecApiToken {
            api_token: a.api_token.clone(),
        })),
        p::PAuthStrategy::PMiddleTierUserNamePassword(m) => {
            A::AuthenticationSpecMiddleTierUserNamePassword(Arc::new(AuthenticationSpecMiddleTierUserNamePassword {
                vault_reference: m.vault_reference.clone(),
            }))
        }
        p::PAuthStrategy::POAuth(o) => A::AuthenticationSpecOAuth(Arc::new(AuthenticationSpecOAuth {
            oauth_key: o.oauth_key.clone(),
            scope_name: o.scope_name.clone(),
        })),
        p::PAuthStrategy::PTrinoKerberosAuth(k) => {
            A::AuthenticationSpecDelegatedKerberos(Arc::new(AuthenticationSpecDelegatedKerberos {
                server_principal: k.server_principal.clone(),
                kerberos_remote_service_name: k.kerberos_remote_service_name.clone(),
                kerberos_use_canonical_hostname: k.kerberos_use_canonical_hostname,
            }))
        }
        p::PAuthStrategy::PGcpWifAuth(w) => A::AuthenticationSpecGcpWorkloadIdentityFederation(Arc::new(
            AuthenticationSpecGcpWorkloadIdentityFederation {
                service_account_email: w.service_account_email.clone(),
                additional_gcp_scopes: list(w.additional_gcp_scopes.clone().unwrap_or_default()),
            },
        )),
    };
    Ok(ConnectionDefinition {
        qualified_name,
        store_name: r.element.clone(),
        database_type,
        specification: spec,
        authentication: auth,
    })
}

/// The Java class's name of a protocol element (`getClass()`'s `toString`, as a message spells it: `class
/// com.legend.protocol.Protocol$PClass`).
pub fn java_class_name(e: &p::Element) -> String {
    format!("class com.legend.protocol.Protocol${}", element_simple_name(e))
}

/// A protocol element's Java simple name.
pub fn element_simple_name(e: &p::Element) -> &'static str {
    match e {
        p::Element::PClass(_) => "PClass",
        p::Element::PAssociation(_) => "PAssociation",
        p::Element::PEnumeration(_) => "PEnumeration",
        p::Element::PFunction(_) => "PFunction",
        p::Element::PProfile(_) => "PProfile",
        p::Element::PSectionIndex(_) => "PSectionIndex",
        p::Element::PMeasure(_) => "PMeasure",
        p::Element::PRuntime(_) => "PRuntime",
        p::Element::PConnection(_) => "PConnection",
        p::Element::PDatabase(_) => "PDatabase",
        p::Element::PService(_) => "PService",
        p::Element::PExecutionEnvironment(_) => "PExecutionEnvironment",
        p::Element::PDataSpace(_) => "PDataSpace",
        p::Element::PPersistence(_) => "PPersistence",
        p::Element::PPersistenceContext(_) => "PPersistenceContext",
        p::Element::PFunctionActivator(_) => "PFunctionActivator",
        p::Element::PDiagram(_) => "PDiagram",
        p::Element::PText(_) => "PText",
        p::Element::PGenerationSpecification(_) => "PGenerationSpecification",
        p::Element::PFileGeneration(_) => "PFileGeneration",
        p::Element::PDeephavenDatabase(_) => "PDeephavenDatabase",
        p::Element::PElasticsearch7Cluster(_) => "PElasticsearch7Cluster",
        p::Element::PMongoDatabase(_) => "PMongoDatabase",
        p::Element::PDataQualityValidation(_) => "PDataQualityValidation",
        p::Element::PDataQualityRelationValidation(_) => "PDataQualityRelationValidation",
        p::Element::PDataQualityRelationComparison(_) => "PDataQualityRelationComparison",
        p::Element::PSchemaSet(_) => "PSchemaSet",
        p::Element::PBinding(_) => "PBinding",
        p::Element::PServiceStoreDefinition(_) => "PServiceStoreDefinition",
        p::Element::PMapping(_) => "PMapping",
        p::Element::PDataElement(_) => "PDataElement",
        p::Element::PRelationalMapper(_) => "PRelationalMapper",
        p::Element::PNativeFunction(_) => "PNativeFunction",
        p::Element::PPrimitive(_) => "PPrimitive",
        p::Element::POpaqueElement(_) => "POpaqueElement",
        p::Element::PAssociationProjection(_) => "PAssociationProjection",
    }
}
