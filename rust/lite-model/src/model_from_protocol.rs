// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! `ModelFromProtocol`: THE ONE DOOR from protocol records to the compiler's model -- a `PureModelContextData` read
//! from JSON or parsed from text, to the `ParsedModel` the compiler builds from. Each element through its kind's
//! converter; a section index's imports applied to the elements its section lists; an element's position its record's
//! source information, kept beside the model, or, for a model parsed from a text, an offset into that text.

use std::collections::HashMap;
use std::sync::Arc;

use lite_front::jtypes::JStr;
use lite_front::records as p;

use crate::error::{ILLEGAL_ARGUMENT, ILLEGAL_STATE, ModelError, Thrown, Unrepresentable};
use crate::from_protocol as fp;
use crate::linked::{LinkedMap, empty, list};
use crate::parsed_model::ImportScopeBuilder;
use crate::records::*;
use lite_front::doors::JavaError;

/// `ModelFromProtocol.of(model)`: a model given as records.
pub fn of(model: &p::PureModelContextData) -> Result<ParsedModel, ModelError> {
    of_text(model, None, empty())
}

/// `ModelFromProtocol.of(model, source, unclaimed)`: for a model parsed from `source`, the text kept on the model and
/// the sections no grammar claimed.
pub fn of_text(
    model: &p::PureModelContextData,
    source: Option<&JStr>,
    unclaimed: Arc<[Arc<ParsedModelUnclaimedSection>]>,
) -> Result<ParsedModel, ModelError> {
    let mut section_of: HashMap<JStr, &p::PSection> = HashMap::new();
    // the model's whole import scope: every section's, a section with no element included
    let mut all = ImportScopeBuilder::new();
    let mut indexed = false;
    for e in &model.elements {
        if let p::Element::PSectionIndex(index) = e {
            indexed = true;
            for s in &index.sections {
                for pkg in &s.imports {
                    all.add(&crate::cat!(pkg, "::*")).map_err(java)?;
                }
                for path in s.elements.iter().flatten() {
                    section_of.entry(path.clone()).or_insert(&**s);
                }
            }
        }
    }
    let mut elements: Vec<PackageableElement> = Vec::new();
    let mut element_imports: LinkedMap<JStr, Arc<ImportScope>> = LinkedMap::new();
    let mut spans: LinkedMap<JStr, Arc<ParsedModelPosition>> = LinkedMap::new();
    for e in &model.elements {
        if matches!(e, p::Element::PSectionIndex(_)) {
            continue;
        }
        let el = element(e)?;
        let key = ParsedModel::key_of(&el);
        elements.push(el);
        let section = section_of.get(&path_of(e)).copied();
        if section.is_none() && indexed {
            // a model with a section index lists each element in the section whose imports it reads
            return Err(ModelError::Unrepresentable(unrepresentable(
                e,
                JStr::from("the model's section index lists no section for it"),
            )));
        }
        if let Some(section) = section {
            let mut own = ImportScopeBuilder::new();
            for pkg in &section.imports {
                own.add(&crate::cat!(pkg, "::*")).map_err(java)?;
            }
            element_imports.put_if_absent(key.clone(), own.build());
        }
        if let Some(span) = span_of(e) {
            spans.put_if_absent(
                key,
                Arc::new(ParsedModelPosition {
                    line: span.start_line,
                    column: span.start_column,
                }),
            );
        }
    }
    let imports = all.build();
    let elements = list(elements);
    let element_imports = element_imports.freeze();
    if let Some(source) = source {
        // a model parsed from a text keeps where each element starts as an offset into that text
        let line_starts = line_starts(source);
        let mut offsets: LinkedMap<JStr, i32> = LinkedMap::new();
        for (key, at) in spans.iter() {
            let line = at.line;
            let offset = if line >= 1 && (line as usize) <= line_starts.len() {
                line_starts[line as usize - 1] as i64 + i64::from(at.column) - 1
            } else {
                -1
            };
            if offset < 0 || offset > source.len() as i64 {
                return Err(ModelError::Java(JavaError::java(
                    ILLEGAL_STATE,
                    crate::cat!(
                        key,
                        "'s span ",
                        line.to_string(),
                        ":",
                        at.column.to_string(),
                        " is outside the text it was parsed from"
                    ),
                )));
            }
            offsets.put(key.clone(), offset as i32);
        }
        return ParsedModel::new(
            elements,
            imports,
            Some(source.clone()),
            offsets.freeze(),
            element_imports,
            empty(),
            unclaimed,
            empty(),
        )
        .map_err(java);
    }
    ParsedModel::new(
        elements,
        imports,
        None,
        empty(),
        element_imports,
        empty(),
        unclaimed,
        spans.freeze(),
    )
    .map_err(java)
}

/// The unit offset each line of `text` starts at (line 1 at 0).
fn line_starts(text: &JStr) -> Vec<usize> {
    let mut starts = vec![0];
    for (i, &u) in text.0.iter().enumerate() {
        if u == u16::from(b'\n') {
            starts.push(i + 1);
        }
    }
    starts
}

/// A converter's throw that is no converter refusal: the Java exception as it is.
fn java(t: Thrown) -> ModelError {
    match t {
        Thrown::Java(e) => ModelError::Java(e),
        // the refusals reach here only from element(), which names them; anywhere else they are a defect
        other => ModelError::Java(JavaError::java(
            ILLEGAL_ARGUMENT,
            JStr::from(format!("unexpected refusal {other:?}")),
        )),
    }
}

/// `ModelFromProtocol.element(e)`: one element record as the model, a converter's refusal (an unsupported
/// connection or mapping shape) named as [`Unrepresentable`].
pub fn element(e: &p::Element) -> Result<PackageableElement, ModelError> {
    match converted(e) {
        Ok(el) => Ok(el),
        Err(Thrown::Connection(reason) | Thrown::Mapping(reason) | Thrown::MissingDatabase(reason)) => {
            Err(ModelError::Unrepresentable(unrepresentable(e, reason)))
        }
        Err(Thrown::Java(j)) => Err(ModelError::Java(j)),
    }
}

fn unrepresentable(e: &p::Element, reason: JStr) -> Unrepresentable {
    Unrepresentable {
        path: path_of(e),
        reason,
        at: span_of(e).map(|s| {
            Arc::new(ParsedModelPosition {
                line: s.start_line,
                column: s.start_column,
            })
        }),
    }
}

fn qn(pkg: &JStr, name: &JStr) -> JStr {
    JStr(lite_front::protocol::qualified_name(&pkg.0, &name.0))
}

fn converted(e: &p::Element) -> Result<PackageableElement, Thrown> {
    use PackageableElement as E;
    Ok(match e {
        p::Element::PClass(c) => E::ClassDefinition(Arc::new(fp::to_class_definition(c))),
        p::Element::PAssociation(a) => E::AssociationDefinition(Arc::new(fp::to_association_definition(a)?)),
        p::Element::PEnumeration(en) => E::EnumDefinition(Arc::new(fp::to_enum_definition(en))),
        p::Element::PFunction(f) => E::FunctionDefinition(Arc::new(fp::to_function_definition(f))),
        p::Element::PProfile(pr) => E::ProfileDefinition(Arc::new(fp::to_profile_definition(pr))),
        p::Element::PMeasure(m) => E::MeasureDefinition(Arc::new(fp::to_measure_definition(m))),
        p::Element::PRuntime(r) => E::RuntimeDefinition(Arc::new(fp::to_runtime_element(r)?)),
        p::Element::PConnection(c) => fp::to_connection_element(c)?,
        p::Element::PDatabase(d) => E::DatabaseDefinition(Arc::new(fp::to_database_definition(d)?)),
        p::Element::PMapping(m) => crate::mapping_from_protocol::to_mapping_element(m)?,
        p::Element::PService(_) | p::Element::PExecutionEnvironment(_) => fp::to_service_section_element(e)?,
        p::Element::PDataSpace(d) => E::DataSpaceDefinition(Arc::new(fp::to_data_space_definition(d))),
        p::Element::PPersistence(_) | p::Element::PPersistenceContext(_) => fp::to_persistence_element(e)?,
        p::Element::PFunctionActivator(a) => activator(a),
        p::Element::PDataElement(d) => E::DataDefinition(Arc::new(DataDefinition {
            qualified_name: qn(&d.pkg, &d.name),
            element: d.clone(),
        })),
        // the kinds no compiler phase opens: a named carrier, by section and kind
        p::Element::PDiagram(d) => carried("Diagram", "Diagram", qn(&d.pkg, &d.name)),
        p::Element::PText(t) => carried("Text", "Text", qn(&t.pkg, &t.name)),
        p::Element::PGenerationSpecification(g) => carried(
            "GenerationSpecification",
            "GenerationSpecification",
            qn(&g.pkg, &g.name),
        ),
        p::Element::PFileGeneration(f) => {
            E::GenericSectionElementDefinition(Arc::new(GenericSectionElementDefinition {
                section: JStr::from("FileGeneration"),
                kind: f.r#type.clone(),
                qualified_name: qn(&f.pkg, &f.name),
                fields: empty(),
                body_source: None,
            }))
        }
        p::Element::PDeephavenDatabase(d) => carried("Deephaven", "Deephaven", qn(&d.pkg, &d.name)),
        p::Element::PElasticsearch7Cluster(s) => carried("Elasticsearch", "Elasticsearch7Cluster", qn(&s.pkg, &s.name)),
        p::Element::PMongoDatabase(m) => {
            E::DatabaseDefinition(Arc::new(crate::mongo_declarations::to_database_definition(m)?))
        }
        p::Element::PDataQualityValidation(v) => {
            carried("DataQualityValidation", "DataQualityValidation", qn(&v.pkg, &v.name))
        }
        p::Element::PDataQualityRelationValidation(v) => carried(
            "DataQualityValidation",
            "DataQualityRelationValidation",
            qn(&v.pkg, &v.name),
        ),
        p::Element::PDataQualityRelationComparison(v) => carried(
            "DataQualityValidation",
            "DataQualityRelationComparison",
            qn(&v.pkg, &v.name),
        ),
        p::Element::PSchemaSet(s) => carried("ExternalFormat", "SchemaSet", qn(&s.pkg, &s.name)),
        p::Element::PBinding(b) => carried("ExternalFormat", "Binding", qn(&b.pkg, &b.name)),
        p::Element::PServiceStoreDefinition(s) => carried("ServiceStore", "ServiceStore", qn(&s.pkg, &s.name)),
        p::Element::PRelationalMapper(m) => carried("QueryPostProcessor", "RelationalMapper", qn(&m.pkg, &m.name)),
        // legend-lite's own records (only its parser makes them)
        p::Element::PNativeFunction(n) => E::NativeFunctionDefinition(Arc::new(NativeFunctionDefinition {
            qualified_name: n.qualified_name.clone(),
            type_parameters: list(n.type_params.clone()),
            multiplicity_parameters: list(n.mult_params.clone()),
            parameters: fp::to_function_params(&n.parameters),
            return_type: n.return_type.clone(),
            return_multiplicity: n.return_multiplicity.clone(),
            stereotypes: fp::stereotypes(&n.stereotypes),
            tagged_values: fp::tagged_values(&n.tagged_values),
        })),
        p::Element::PPrimitive(pr) => E::PrimitiveExtensionDefinition(Arc::new(PrimitiveExtensionDefinition {
            qualified_name: pr.qualified_name.clone(),
            base_type_name: pr.base_type_name.clone(),
            type_variables: list(pr.type_variables.to_vec()),
        })),
        p::Element::POpaqueElement(o) => E::OpaqueElementDefinition(Arc::new(OpaqueElementDefinition {
            qualified_name: o.qualified_name.clone(),
            section_name: o.section_name.clone(),
            protocol_json: o.protocol_json.clone(),
        })),
        // a projection association registers its name only, as an empty class
        p::Element::PAssociationProjection(a) => E::ClassDefinition(Arc::new(ClassDefinition {
            qualified_name: a.qualified_name.clone(),
            type_params: empty(),
            type_variables: empty(),
            super_classes: empty(),
            properties: empty(),
            derived_properties: empty(),
            constraints: empty(),
            stereotypes: empty(),
            tagged_values: empty(),
            is_native: false,
            multiplicity_params: empty(),
        })),
        p::Element::PSectionIndex(_) => {
            return Err(Thrown::Java(JavaError::java(
                ILLEGAL_ARGUMENT,
                JStr::from("a section index is not an element: its sections are read by of"),
            )));
        }
    })
}

/// A function activator: its fields as the activator definition keeps them.
fn activator(a: &p::PFunctionActivator) -> PackageableElement {
    let mut fields: LinkedMap<JStr, JStr> = LinkedMap::of(a.scalars.iter().cloned());
    fields.put(JStr::from("function"), a.function_path.clone());
    if let Some(owner) = &a.owner_id {
        fields.put(JStr::from("ownership"), crate::cat!("Deployment ", owner));
    }
    if let Some(users) = &a.user_list_users {
        let mut joined: Vec<u16> = Vec::new();
        for (i, u) in users.iter().enumerate() {
            if i > 0 {
                joined.push(u16::from(b','));
            }
            joined.extend_from_slice(&u.0);
        }
        fields.put(JStr::from("ownership"), crate::cat!("UserList ", joined));
    }
    if let Some(c) = &a.activation_connection {
        fields.put(JStr::from("activationConfiguration"), c.clone());
    }
    PackageableElement::FunctionActivatorDefinition(Arc::new(FunctionActivatorDefinition {
        qualified_name: qn(&a.pkg, &a.name),
        kind: a.kind.clone(),
        fields: fields.freeze(),
    }))
}

fn carried(section: &str, kind: &str, qualified_name: JStr) -> PackageableElement {
    PackageableElement::GenericSectionElementDefinition(Arc::new(GenericSectionElementDefinition {
        section: JStr::from(section),
        kind: JStr::from(kind),
        qualified_name,
        fields: empty(),
        body_source: None,
    }))
}

/// `Protocol.pathOf(e)`: the path an element is listed under in its section index (a function's by its signature).
pub fn path_of(e: &p::Element) -> JStr {
    JStr(match e {
        p::Element::PFunction(f) => lite_front::protocol::function_path(f),
        p::Element::PClass(x) => qn(&x.pkg, &x.name).0,
        p::Element::PAssociation(x) => qn(&x.pkg, &x.name).0,
        p::Element::PEnumeration(x) => qn(&x.pkg, &x.name).0,
        p::Element::PProfile(x) => qn(&x.pkg, &x.name).0,
        p::Element::PMeasure(x) => qn(&x.pkg, &x.name).0,
        p::Element::PRuntime(x) => qn(&x.pkg, &x.name).0,
        p::Element::PConnection(x) => qn(&x.pkg, &x.name).0,
        p::Element::PDatabase(x) => qn(&x.pkg, &x.name).0,
        p::Element::PMapping(x) => qn(&x.pkg, &x.name).0,
        p::Element::PService(x) => qn(&x.pkg, &x.name).0,
        p::Element::PExecutionEnvironment(x) => qn(&x.pkg, &x.name).0,
        p::Element::PDataSpace(x) => qn(&x.pkg, &x.name).0,
        p::Element::PPersistence(x) => qn(&x.pkg, &x.name).0,
        p::Element::PPersistenceContext(x) => qn(&x.pkg, &x.name).0,
        p::Element::PFunctionActivator(x) => qn(&x.pkg, &x.name).0,
        p::Element::PDataElement(x) => qn(&x.pkg, &x.name).0,
        p::Element::PDiagram(x) => qn(&x.pkg, &x.name).0,
        p::Element::PText(x) => qn(&x.pkg, &x.name).0,
        p::Element::PGenerationSpecification(x) => qn(&x.pkg, &x.name).0,
        p::Element::PFileGeneration(x) => qn(&x.pkg, &x.name).0,
        p::Element::PDeephavenDatabase(x) => qn(&x.pkg, &x.name).0,
        p::Element::PElasticsearch7Cluster(x) => qn(&x.pkg, &x.name).0,
        p::Element::PMongoDatabase(x) => qn(&x.pkg, &x.name).0,
        p::Element::PDataQualityValidation(x) => qn(&x.pkg, &x.name).0,
        p::Element::PDataQualityRelationValidation(x) => qn(&x.pkg, &x.name).0,
        p::Element::PDataQualityRelationComparison(x) => qn(&x.pkg, &x.name).0,
        p::Element::PSchemaSet(x) => qn(&x.pkg, &x.name).0,
        p::Element::PBinding(x) => qn(&x.pkg, &x.name).0,
        p::Element::PServiceStoreDefinition(x) => qn(&x.pkg, &x.name).0,
        p::Element::PRelationalMapper(x) => qn(&x.pkg, &x.name).0,
        p::Element::PNativeFunction(x) => x.qualified_name.0.clone(),
        p::Element::PPrimitive(x) => x.qualified_name.0.clone(),
        p::Element::POpaqueElement(x) => x.qualified_name.0.clone(),
        p::Element::PAssociationProjection(x) => x.qualified_name.0.clone(),
        p::Element::PSectionIndex(i) => qn(&i.pkg, &i.name).0,
    })
}

/// An element record's source information, when it carries one.
pub fn span_of(e: &p::Element) -> Option<&p::SourceInfo> {
    match e {
        p::Element::PClass(x) => x.source_information.as_deref(),
        p::Element::PAssociation(x) => x.source_information.as_deref(),
        p::Element::PEnumeration(x) => x.source_information.as_deref(),
        p::Element::PFunction(x) => x.source_information.as_deref(),
        p::Element::PProfile(x) => x.source_information.as_deref(),
        p::Element::PMeasure(x) => x.source_information.as_deref(),
        p::Element::PRuntime(x) => x.source_information.as_deref(),
        p::Element::PConnection(x) => x.source_information.as_deref(),
        p::Element::PDatabase(x) => x.source_information.as_deref(),
        p::Element::PMapping(x) => x.source_information.as_deref(),
        p::Element::PService(x) => x.source_information.as_deref(),
        p::Element::PExecutionEnvironment(x) => x.source_information.as_deref(),
        p::Element::PDataSpace(x) => x.source_information.as_deref(),
        p::Element::PPersistence(x) => x.source_information.as_deref(),
        p::Element::PPersistenceContext(x) => x.source_information.as_deref(),
        p::Element::PFunctionActivator(x) => x.source_information.as_deref(),
        p::Element::PDataElement(x) => x.source_information.as_deref(),
        p::Element::PDiagram(x) => x.source_information.as_deref(),
        p::Element::PText(x) => x.source_information.as_deref(),
        p::Element::PGenerationSpecification(x) => x.source_information.as_deref(),
        p::Element::PFileGeneration(x) => x.source_information.as_deref(),
        p::Element::PDeephavenDatabase(x) => x.source_information.as_deref(),
        p::Element::PElasticsearch7Cluster(x) => x.source_information.as_deref(),
        p::Element::PMongoDatabase(x) => x.source_information.as_deref(),
        p::Element::PDataQualityValidation(x) => x.source_information.as_deref(),
        p::Element::PDataQualityRelationValidation(x) => x.source_information.as_deref(),
        p::Element::PDataQualityRelationComparison(x) => x.source_information.as_deref(),
        p::Element::PSchemaSet(x) => x.source_information.as_deref(),
        p::Element::PBinding(x) => x.source_information.as_deref(),
        p::Element::PServiceStoreDefinition(x) => x.source_information.as_deref(),
        p::Element::PRelationalMapper(x) => x.source_information.as_deref(),
        p::Element::PNativeFunction(x) => x.source_information.as_deref(),
        p::Element::PPrimitive(x) => x.source_information.as_deref(),
        p::Element::POpaqueElement(x) => x.source_information.as_deref(),
        p::Element::PAssociationProjection(x) => x.source_information.as_deref(),
        p::Element::PSectionIndex(_) => None,
    }
}
