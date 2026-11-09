// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! core/'s `com.legend.protocol.TailEmitter`, ported (docs/rust-port/PHASE2.md §4 step 5): the wire of the tail
//! sections' elements -- Text, GenerationSpecification, FileGeneration, the Deephaven, Elasticsearch and MongoDB stores,
//! the ExternalFormat schema sets and bindings, the ServiceStore, Diagram, DataSpace, Persistence and its context,
//! Service and ExecutionEnvironment, and the DataQuality trio. Byte for byte Java's, its duplicated `_type` and
//! `_pure_protocol_type` keys (the engine serializer's own output) included, its refusals in Java's order.

use crate::doors::JavaError;
use crate::emitter::{
    self, Buf, Em, comma_each, illegal_state, npe, src_info, str, strs_each, unsupported, value_spec,
};
use crate::jcat;
use crate::jtypes::JStr;
use crate::mapping_emitter;
use crate::records::*;
use crate::ustr::eq;

fn d2s(v: f64) -> String {
    lite_javacompat::double_to_string(v)
}

/// `(SomeSpec) v` where the record holds a `ValueSpecification`: a path literal, or Java's `ClassCastException`.
fn as_path_literal(v: &ValueSpecification) -> Result<&PathLiteral, JavaError> {
    if let ValueSpecification::PathLiteral(pl) = v {
        return Ok(pl);
    }
    let from = spec_class_name(v);
    let to = "com.legend.protocol.spec.PathLiteral";
    Err(JavaError::java(
        "java.lang.ClassCastException",
        JStr::from(format!(
            "class {from} cannot be cast to class {to} ({from} and {to} are in unnamed module of loader 'app')"
        )),
    ))
}

/// A value specification's Java class's `getName()`.
fn spec_class_name(v: &ValueSpecification) -> String {
    let simple = emitter::simple_name(v);
    match v {
        ValueSpecification::TypeAnnotation(_) => format!("com.legend.protocol.spec.TypeAnnotation${simple}"),
        _ => format!("com.legend.protocol.spec.{simple}"),
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// Text, generations
// ---------------------------------------------------------------------------------------------------------------------

pub fn text(b: &mut Buf, t: &PText) -> Em {
    b.a("{\"_type\":\"text\",\"content\":");
    str(b, &t.content);
    b.a(",\"name\":");
    str(b, &t.name);
    b.a(",\"package\":");
    str(b, &t.pkg);
    b.a(",\"sourceInformation\":");
    src_info(b, t.source_information.as_deref());
    if let Some(ty) = &t.r#type {
        b.a(",\"type\":");
        str(b, ty);
    }
    b.ch(b'}');
    Ok(())
}

pub fn generation_specification(b: &mut Buf, g: &PGenerationSpecification) -> Em {
    b.a("{\"_type\":\"generationSpecification\",\"fileGenerations\":[");
    for (i, p) in g.file_generations.iter().enumerate() {
        if i > 0 {
            b.ch(b',');
        }
        b.a("{\"path\":");
        str(b, &p.path);
        b.a(",\"sourceInformation\":");
        src_info(b, p.source_information.as_deref());
        b.a(",\"type\":\"FILE_GENERATION\"}");
    }
    b.a("],\"generationNodes\":[");
    for (i, n) in g.generation_nodes.iter().enumerate() {
        if i > 0 {
            b.ch(b',');
        }
        b.a("{\"generationElement\":");
        str(b, &n.generation_element);
        b.a(",\"id\":");
        str(b, &n.id);
        b.a(",\"sourceInformation\":");
        src_info(b, n.source_information.as_deref());
        b.ch(b'}');
    }
    b.a("],\"name\":");
    str(b, &g.name);
    b.a(",\"package\":");
    str(b, &g.pkg);
    b.a(",\"sourceInformation\":");
    src_info(b, g.source_information.as_deref());
    b.ch(b'}');
    Ok(())
}

pub fn file_generation(b: &mut Buf, f: &PFileGeneration) -> Em {
    b.a("{\"_type\":\"fileGeneration\",\"configurationProperties\":[");
    for (i, p) in f.configuration_properties.iter().enumerate() {
        if i > 0 {
            b.ch(b',');
        }
        b.a("{\"name\":");
        str(b, &p.name);
        b.a(",\"sourceInformation\":");
        src_info(b, p.source_information.as_deref());
        b.a(",\"value\":");
        config_value(b, &p.value);
        b.ch(b'}');
    }
    b.ch(b']');
    if let Some(path) = &f.generation_output_path {
        b.a(",\"generationOutputPath\":");
        str(b, path);
    }
    b.a(",\"name\":");
    str(b, &f.name);
    b.a(",\"package\":");
    str(b, &f.pkg);
    b.a(",\"scopeElements\":[");
    strs_each(b, &f.scope_elements);
    b.a("],\"sourceInformation\":");
    src_info(b, f.source_information.as_deref());
    b.a(",\"type\":");
    str(b, &f.r#type);
    b.a(",\"typeSourceInformation\":");
    src_info(b, f.type_source_information.as_deref());
    b.ch(b'}');
    Ok(())
}

fn config_value(b: &mut Buf, v: &PConfigValue) {
    match v {
        PConfigValue::PConfigValuePCString(s) => str(b, &s.value),
        PConfigValue::PConfigValuePCBoolean(bo) => b.boolean(bo.value),
        PConfigValue::PConfigValuePCInteger(i) => b.int(i.value),
        // written back as read (the engine's own writer leaves a null out, which reads back as the same null)
        PConfigValue::PConfigValuePCNull(_) => b.a("null"),
        PConfigValue::PConfigValuePCStrings(ss) => {
            b.ch(b'[');
            strs_each(b, &ss.values);
            b.ch(b']');
        }
        PConfigValue::PConfigValuePCMap(m) => {
            // a LinkedHashMap: insertion order
            b.ch(b'{');
            for (i, (k, v)) in m.entries.iter().enumerate() {
                if i > 0 {
                    b.ch(b',');
                }
                str(b, k);
                b.ch(b':');
                str(b, v);
            }
            b.ch(b'}');
        }
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// Deephaven, Elasticsearch, MongoDB
// ---------------------------------------------------------------------------------------------------------------------

pub fn deephaven_store(b: &mut Buf, d: &PDeephavenDatabase) -> Em {
    b.a("{\"_type\":\"deephavenStore\",\"includedStores\":[],\"name\":");
    str(b, &d.name);
    b.a(",\"package\":");
    str(b, &d.pkg);
    b.a(",\"sourceInformation\":");
    src_info(b, d.source_information.as_deref());
    b.a(",\"tables\":[");
    for (t, table) in d.tables.iter().enumerate() {
        if t > 0 {
            b.ch(b',');
        }
        b.a("{\"columns\":[");
        for (i, col) in table.columns.iter().enumerate() {
            if i > 0 {
                b.ch(b',');
            }
            b.a("{\"_type\":\"column\",\"name\":");
            str(b, &col.name);
            // the engine serializer prints the type discriminator twice
            b.a(",\"type\":{\"_type\":\"");
            b.units(&col.kind.0);
            b.a("\",\"_type\":\"");
            b.units(&col.kind.0);
            b.ch(b'"');
            if let Some(p) = col.precision {
                b.a(",\"precision\":");
                b.int(i64::from(p));
                b.a(",\"scale\":");
                // append(Integer): a null scale is "null"
                match col.scale {
                    Some(s) => b.int(i64::from(s)),
                    None => b.a("null"),
                }
            }
            b.a("}}");
        }
        b.a("],\"name\":");
        str(b, &table.name);
        b.ch(b'}');
    }
    b.a("]}");
    Ok(())
}

/// One typed property value (`"<key>":{...}`): the engine serializer doubles `_pure_protocol_type`.
fn es_property_body(b: &mut Buf, prop: &PElasticsearch7ClusterPEsProperty) {
    b.ch(b'"');
    b.units(&prop.wire_key.0);
    b.a("\":{\"_pure_protocol_type\":\"");
    b.units(&prop.protocol_type.0);
    b.a("\",\"_pure_protocol_type\":\"");
    b.units(&prop.protocol_type.0);
    b.ch(b'"');
    es_child_map(b, "fields", prop.fields.as_deref());
    es_child_map(b, "properties", prop.child_properties.as_deref());
    b.a(",\"type\":\"");
    b.units(&prop.es_type.0);
    b.a("\"}");
}

/// A property's children, sorted by name (the engine's `ORDER_MAP_ENTRIES_BY_KEYS`; `String.compareTo`, stable).
fn es_child_map(b: &mut Buf, label: &str, children: Option<&[Box<PElasticsearch7ClusterPEsProperty>]>) {
    let Some(children) = children else {
        return;
    };
    b.a(",\"");
    b.a(label);
    b.a("\":{");
    let mut sorted: Vec<&PElasticsearch7ClusterPEsProperty> = children.iter().map(|c| &**c).collect();
    sorted.sort_by(|x, y| x.property_name.0.cmp(&y.property_name.0));
    for (i, f) in sorted.iter().enumerate() {
        if i > 0 {
            b.ch(b',');
        }
        str(b, &f.property_name);
        b.a(":{");
        es_property_body(b, f);
        b.ch(b'}');
    }
    b.ch(b'}');
}

pub fn elasticsearch_store(b: &mut Buf, s: &PElasticsearch7Cluster) -> Em {
    b.a("{\"_type\":\"elasticsearch7Store\",\"includedStores\":[],\"indices\":[");
    for (i, idx) in s.indices.iter().enumerate() {
        if i > 0 {
            b.ch(b',');
        }
        b.a("{\"indexName\":");
        str(b, &idx.index_name);
        b.a(",\"properties\":[");
        for (p, prop) in idx.properties.iter().enumerate() {
            if p > 0 {
                b.ch(b',');
            }
            b.a("{\"property\":{");
            es_property_body(b, prop);
            b.a("},\"propertyName\":");
            str(b, &prop.property_name);
            b.ch(b'}');
        }
        b.a("]}");
    }
    b.a("],\"name\":");
    str(b, &s.name);
    b.a(",\"package\":");
    str(b, &s.pkg);
    b.a(",\"sourceInformation\":");
    src_info(b, s.source_information.as_deref());
    b.ch(b'}');
    Ok(())
}

pub fn mongo_database(b: &mut Buf, m: &PMongoDatabase) -> Em {
    b.a("{\"_type\":\"MongoDatabase\",\"collections\":[");
    for (i, coll) in m.collections.iter().enumerate() {
        if i > 0 {
            b.ch(b',');
        }
        b.a("{\"name\":");
        str(b, &coll.name);
        b.a(",\"validator\":{\"validationAction\":");
        str(b, &coll.validation_action);
        b.a(",\"validationLevel\":");
        str(b, &coll.validation_level);
        b.a(",\"validatorExpression\":{\"_type\":\"jsonSchemaExpression\",\"schemaExpression\":");
        bson_schema(b, &coll.schema);
        b.a("}}}");
    }
    b.a("],\"includedStores\":[],\"name\":");
    str(b, &m.name);
    b.a(",\"package\":");
    str(b, &m.pkg);
    b.a(",\"sourceInformation\":");
    src_info(b, m.source_information.as_deref());
    b.a(",\"views\":[]}");
    Ok(())
}

/// The BSON schema wire: object nodes carry additionalPropertiesAllowed/properties/required; scalar nodes only their
/// facets. Slots alphabetical, arrays always present.
fn bson_schema(b: &mut Buf, s: &PBsonSchema) {
    b.a("{\"_type\":\"");
    b.units(&s.wire_type.0);
    b.a("\",\"_enum\":[]");
    let is_object = s.wire_type == "schema" || s.wire_type == "objectType";
    let is_array = s.wire_type == "arrayType";
    if is_array {
        b.a(",\"additionalItemsAllowed\":false");
    }
    if is_object {
        b.a(",\"additionalPropertiesAllowed\":");
        b.boolean(s.additional_properties_allowed);
    }
    b.a(",\"allOf\":[],\"anyOf\":[]");
    if let Some(items) = &s.items {
        b.a(",\"items\":[");
        for (i, it) in items.iter().enumerate() {
            if i > 0 {
                b.ch(b',');
            }
            bson_schema(b, it);
        }
        b.ch(b']');
    }
    if let Some(d) = &s.description {
        b.a(",\"description\":");
        str(b, d);
    }
    if let Some(m) = s.max_length {
        b.a(",\"maxLength\":");
        b.int(m);
    }
    if let Some(m) = s.min_length {
        b.a(",\"minLength\":");
        b.int(m);
    }
    b.a(",\"oneOf\":[]");
    if is_array {
        b.a(",\"uniqueItems\":false");
    }
    if is_object {
        b.a(",\"properties\":[");
        for (i, (k, v)) in s.properties.iter().enumerate() {
            if i > 0 {
                b.ch(b',');
            }
            b.a("{\"key\":");
            str(b, k);
            b.a(",\"value\":");
            bson_schema(b, v);
            b.ch(b'}');
        }
        b.a("],\"required\":[");
        strs_each(b, &s.required);
        b.ch(b']');
        if let Some(t) = &s.title {
            b.a(",\"title\":");
            str(b, t);
        }
    }
    b.ch(b'}');
}

// ---------------------------------------------------------------------------------------------------------------------
// ExternalFormat
// ---------------------------------------------------------------------------------------------------------------------

pub fn schema_set(b: &mut Buf, s: &PSchemaSet) -> Em {
    b.a("{\"_type\":\"externalFormatSchemaSet\",\"format\":");
    str(b, &s.format);
    b.a(",\"name\":");
    str(b, &s.name);
    b.a(",\"package\":");
    str(b, &s.pkg);
    b.a(",\"schemas\":[");
    for (i, sc) in s.schemas.iter().enumerate() {
        if i > 0 {
            b.ch(b',');
        }
        b.a("{\"content\":");
        str(b, &sc.content);
        b.a(",\"contentSourceInformation\":");
        src_info(b, sc.content_source_information.as_deref());
        if let Some(id) = &sc.id {
            b.a(",\"id\":");
            str(b, id);
        }
        if let Some(loc) = &sc.location {
            b.a(",\"location\":");
            str(b, loc);
        }
        b.a(",\"sourceInformation\":");
        src_info(b, sc.source_information.as_deref());
        b.ch(b'}');
    }
    b.a("],\"sourceInformation\":");
    src_info(b, s.source_information.as_deref());
    b.ch(b'}');
    Ok(())
}

pub fn binding(b: &mut Buf, bd: &PBinding) -> Em {
    b.a("{\"_type\":\"binding\",\"contentType\":");
    str(b, &bd.content_type);
    b.a(",\"modelUnit\":{\"packageableElementExcludes\":[");
    strs_each(b, &bd.model_excludes);
    b.a("],\"packageableElementIncludes\":[");
    strs_each(b, &bd.model_includes);
    b.a("]},\"name\":");
    str(b, &bd.name);
    b.a(",\"package\":");
    str(b, &bd.pkg);
    if let Some(id) = &bd.schema_id {
        b.a(",\"schemaId\":");
        str(b, id);
    }
    if let Some(set) = &bd.schema_set {
        b.a(",\"schemaSet\":");
        str(b, set);
    }
    b.a(",\"sourceInformation\":");
    src_info(b, bd.source_information.as_deref());
    b.ch(b'}');
    Ok(())
}

// ---------------------------------------------------------------------------------------------------------------------
// ServiceStore
// ---------------------------------------------------------------------------------------------------------------------

pub fn service_store(b: &mut Buf, s: &PServiceStoreDefinition) -> Em {
    b.a("{\"_type\":\"serviceStore\"");
    if let Some(d) = &s.description {
        // lite's dialect keeps a description (the engine's parser drops it), in the engine's alphabetical order
        b.a(",\"description\":");
        str(b, d);
    }
    b.a(",\"elements\":[");
    comma_each(b, &s.elements, service_store_element)?;
    b.a("],\"includedStores\":[],\"name\":");
    str(b, &s.name);
    b.a(",\"package\":");
    str(b, &s.pkg);
    b.a(",\"sourceInformation\":");
    src_info(b, s.source_information.as_deref());
    b.ch(b'}');
    Ok(())
}

fn service_store_element(b: &mut Buf, e: &PServiceStoreElement) -> Em {
    match e {
        PServiceStoreElement::PSsService(sv) => ss_service(b, sv),
        PServiceStoreElement::PSsServiceGroup(g) => {
            b.a("{\"_type\":\"serviceGroup\",\"elements\":[");
            comma_each(b, &g.elements, service_store_element)?;
            b.a("],\"id\":");
            str(b, &g.id);
            b.a(",\"path\":");
            str(b, &g.path);
            b.a(",\"sourceInformation\":");
            src_info(b, g.source_information.as_deref());
            b.ch(b'}');
            Ok(())
        }
    }
}

fn ss_service(b: &mut Buf, sv: &PSsService) -> Em {
    b.a("{\"_type\":\"service\",\"id\":");
    str(b, &sv.id);
    b.a(",\"method\":");
    str(b, &sv.method);
    if let Some(params) = &sv.parameters {
        b.a(",\"parameters\":[");
        comma_each(b, params, |b, p| ss_param(b, p))?;
        b.ch(b']');
    }
    b.a(",\"path\":");
    str(b, &sv.path);
    if let Some(rb) = &sv.request_body {
        b.a(",\"requestBody\":");
        ss_type_ref(b, rb)?;
    }
    b.a(",\"response\":");
    ss_type_ref(b, &sv.response)?;
    if !sv.security.is_empty() {
        return Err(illegal_state(JStr::from(
            "non-empty ServiceStore security wire is unprobed",
        )));
    }
    b.a(",\"security\":[],\"sourceInformation\":");
    src_info(b, sv.source_information.as_deref());
    b.ch(b'}');
    Ok(())
}

fn ss_param(b: &mut Buf, p: &PSsParam) -> Em {
    b.ch(b'{');
    if let Some(ar) = p.allow_reserved {
        b.a("\"allowReserved\":");
        b.boolean(ar);
        b.ch(b',');
    }
    if let Some(en) = &p.enumeration {
        b.a("\"enumeration\":");
        str(b, en);
        b.ch(b',');
    }
    b.a("\"location\":");
    str(b, &p.location);
    b.a(",\"name\":");
    str(b, &p.name);
    if let Some(r) = p.required {
        b.a(",\"required\":");
        b.boolean(r);
    }
    b.a(",\"serializationFormat\":{");
    if let Some(ex) = p.explode {
        b.a("\"explode\":");
        b.boolean(ex);
        b.a(",\"explodeSourceInformation\":");
        src_info(b, p.explode_span.as_deref());
    }
    if let Some(style) = &p.style {
        if p.explode.is_some() {
            b.ch(b',');
        }
        b.a("\"style\":");
        str(b, style);
        b.a(",\"styleSourceInformation\":");
        src_info(b, p.style_span.as_deref());
    }
    b.a("},\"sourceInformation\":");
    src_info(b, p.source_information.as_deref());
    b.a(",\"type\":");
    ss_type_ref(b, &p.r#type)?;
    b.ch(b'}');
    Ok(())
}

/// `String.toLowerCase(Locale.ROOT)` over UTF-16 units: Unicode's full lower-case mapping with its final-sigma rule
/// (`str::to_lowercase`, as the JDK's `ConditionalSpecialCasing`), each run between lone surrogates lowered alone (a
/// lone surrogate kept).
fn to_lower_root(s: &[u16]) -> Vec<u16> {
    let mut out = Vec::with_capacity(s.len());
    let mut run = String::new();
    let flush = |run: &mut String, out: &mut Vec<u16>| {
        out.extend(run.to_lowercase().encode_utf16());
        run.clear();
    };
    for r in char::decode_utf16(s.iter().copied()) {
        match r {
            Ok(c) => run.push(c),
            Err(e) => {
                flush(&mut run, &mut out);
                out.push(e.unpaired_surrogate());
            }
        }
    }
    flush(&mut run, &mut out);
    out
}

fn ss_type_ref(b: &mut Buf, t: &PSsTypeRef) -> Em {
    if let Some(complex) = &t.complex_type {
        b.a("{\"_type\":\"complex\",\"binding\":");
        let Some(binding) = &t.binding else {
            return Err(npe(None));
        };
        str(b, binding);
        b.a(",\"list\":");
        b.boolean(t.list);
        b.a(",\"sourceInformation\":");
        src_info(b, t.source_information.as_deref());
        b.a(",\"type\":");
        str(b, complex);
        b.ch(b'}');
    } else {
        b.a("{\"_type\":\"");
        let Some(primitive) = &t.primitive else {
            return Err(npe(None));
        };
        b.units(&to_lower_root(&primitive.0));
        b.a("\",\"list\":");
        b.boolean(t.list);
        b.a(",\"sourceInformation\":");
        src_info(b, t.source_information.as_deref());
        b.ch(b'}');
    }
    Ok(())
}

// ---------------------------------------------------------------------------------------------------------------------
// Diagram
// ---------------------------------------------------------------------------------------------------------------------

pub fn diagram(b: &mut Buf, d: &PDiagram) -> Em {
    b.a("{\"_type\":\"diagram\",\"classViews\":[");
    for (i, v) in d.class_views.iter().enumerate() {
        if i > 0 {
            b.ch(b',');
        }
        b.a("{\"class\":");
        str(b, &v.class_path);
        b.a(",\"classSourceInformation\":");
        src_info(b, v.class_span.as_deref());
        if let Some(h) = v.hide_properties {
            b.a(",\"hideProperties\":");
            b.boolean(h);
        }
        if let Some(h) = v.hide_stereotypes {
            b.a(",\"hideStereotypes\":");
            b.boolean(h);
        }
        if let Some(h) = v.hide_tagged_values {
            b.a(",\"hideTaggedValues\":");
            b.boolean(h);
        }
        b.a(",\"id\":");
        str(b, &v.id);
        b.a(",\"position\":{\"x\":");
        b.a(&d2s(v.x));
        b.a(",\"y\":");
        b.a(&d2s(v.y));
        b.a("},\"rectangle\":{\"height\":");
        b.a(&d2s(v.height));
        b.a(",\"width\":");
        b.a(&d2s(v.width));
        b.a("},\"sourceInformation\":");
        src_info(b, v.source_information.as_deref());
        b.ch(b'}');
    }
    b.a("],\"generalizationViews\":[");
    for (i, v) in d.generalization_views.iter().enumerate() {
        if i > 0 {
            b.ch(b',');
        }
        b.ch(b'{');
        diagram_line(b, &v.points);
        b.a(",\"sourceInformation\":");
        src_info(b, v.source_information.as_deref());
        diagram_ends(
            b,
            &v.source_view,
            v.source_view_span.as_deref(),
            &v.target_view,
            v.target_view_span.as_deref(),
        );
        b.ch(b'}');
    }
    b.a("],\"name\":");
    str(b, &d.name);
    b.a(",\"package\":");
    str(b, &d.pkg);
    b.a(",\"propertyViews\":[");
    for (i, v) in d.property_views.iter().enumerate() {
        if i > 0 {
            b.ch(b',');
        }
        b.ch(b'{');
        diagram_line(b, &v.points);
        b.a(",\"property\":{\"class\":");
        str(b, &v.property_class);
        b.a(",\"property\":");
        str(b, &v.property_name);
        b.a(",\"sourceInformation\":");
        src_info(b, v.property_span.as_deref());
        b.a("},\"sourceInformation\":");
        src_info(b, v.source_information.as_deref());
        diagram_ends(
            b,
            &v.source_view,
            v.source_view_span.as_deref(),
            &v.target_view,
            v.target_view_span.as_deref(),
        );
        b.ch(b'}');
    }
    b.a("],\"sourceInformation\":");
    src_info(b, d.source_information.as_deref());
    b.ch(b'}');
    Ok(())
}

fn diagram_line(b: &mut Buf, points: &[Box<PDiagramPoint>]) {
    b.a("\"line\":{\"points\":[");
    for (i, p) in points.iter().enumerate() {
        if i > 0 {
            b.ch(b',');
        }
        b.a("{\"x\":");
        b.a(&d2s(p.x));
        b.a(",\"y\":");
        b.a(&d2s(p.y));
        b.ch(b'}');
    }
    b.a("]}");
}

fn diagram_ends(
    b: &mut Buf,
    source_view: &JStr,
    source_view_span: Option<&SourceInfo>,
    target_view: &JStr,
    target_view_span: Option<&SourceInfo>,
) {
    b.a(",\"sourceView\":");
    str(b, source_view);
    b.a(",\"sourceViewSourceInformation\":");
    src_info(b, source_view_span);
    b.a(",\"targetView\":");
    str(b, target_view);
    b.a(",\"targetViewSourceInformation\":");
    src_info(b, target_view_span);
}

// ---------------------------------------------------------------------------------------------------------------------
// DataSpace
// ---------------------------------------------------------------------------------------------------------------------

pub fn data_space(b: &mut Buf, d: &PDataSpace) -> Em {
    b.a("{\"_type\":\"dataSpace\"");
    if let Some(c) = &d.default_execution_context {
        b.a(",\"defaultExecutionContext\":");
        str(b, c);
    }
    if let Some(desc) = &d.description {
        b.a(",\"description\":");
        str(b, desc);
    }
    if let Some(diagrams) = &d.diagrams {
        b.a(",\"diagrams\":[");
        for (i, g) in diagrams.iter().enumerate() {
            if i > 0 {
                b.ch(b',');
            }
            b.ch(b'{');
            if let Some(desc) = &g.description {
                b.a("\"description\":");
                str(b, desc);
                b.ch(b',');
            }
            b.a("\"diagram\":{\"path\":");
            str(b, &g.diagram);
            b.a(",\"sourceInformation\":");
            src_info(b, g.diagram_span.as_deref());
            b.a("},\"sourceInformation\":");
            src_info(b, g.source_information.as_deref());
            b.a(",\"title\":");
            str(b, &g.title);
            b.ch(b'}');
        }
        b.ch(b']');
    }
    if let Some(elements) = &d.elements {
        b.a(",\"elements\":[");
        for (i, r) in elements.iter().enumerate() {
            if i > 0 {
                b.ch(b',');
            }
            b.ch(b'{');
            if r.exclude {
                b.a("\"exclude\":true,");
            }
            b.a("\"path\":");
            str(b, &r.path);
            b.a(",\"sourceInformation\":");
            src_info(b, r.source_information.as_deref());
            b.ch(b'}');
        }
        b.ch(b']');
    }
    if let Some(executables) = &d.executables {
        b.a(",\"executables\":[");
        comma_each(b, executables, |b, e| data_space_executable(b, e))?;
        b.ch(b']');
    }
    if let Some(contexts) = &d.execution_contexts {
        // optional since 4.145.0: unspelled -> the slot is omitted
        b.a(",\"executionContexts\":[");
        comma_each(b, contexts, |b, c| data_space_context(b, c))?;
        b.ch(b']');
    }
    b.a(",\"name\":");
    str(b, &d.name);
    if let Some(om) = &d.operational_metadata {
        b.a(",\"operationalMetadata\":{");
        if !om.coverage_regions.is_empty() {
            // an empty list is omitted on the wire (NON_EMPTY)
            b.a("\"coverageRegions\":[");
            strs_each(b, &om.coverage_regions);
            b.a("],");
        }
        b.a("\"sourceInformation\":");
        src_info(b, om.source_information.as_deref());
        if let Some(f) = &om.update_frequency {
            b.a(",\"updateFrequency\":");
            str(b, f);
        }
        b.ch(b'}');
    }
    b.a(",\"package\":");
    str(b, &d.pkg);
    b.a(",\"sourceInformation\":");
    src_info(b, d.source_information.as_deref());
    b.a(",\"stereotypes\":");
    emitter::stereotypes(b, &d.stereotypes);
    if let Some(s) = &d.support_info {
        b.a(",\"supportInfo\":");
        data_space_support(b, s);
    }
    b.a(",\"taggedValues\":");
    emitter::tagged_values(b, &d.tagged_values);
    if let Some(t) = &d.title {
        b.a(",\"title\":");
        str(b, t);
    }
    b.ch(b'}');
    Ok(())
}

fn data_space_context(b: &mut Buf, ctx: &PDataSpaceContext) -> Em {
    b.ch(b'{');
    let mut first = true;
    if let Some(rt) = &ctx.default_runtime {
        b.a("\"defaultRuntime\":");
        pointer(b, rt, ctx.runtime_span.as_deref(), "RUNTIME");
        first = false;
    }
    if let Some(desc) = &ctx.description {
        b.a(if first { "" } else { "," });
        b.a("\"description\":");
        str(b, desc);
        first = false;
    }
    if let Some(m) = &ctx.mapping {
        b.a(if first { "" } else { "," });
        b.a("\"mapping\":");
        pointer(b, m, ctx.mapping_span.as_deref(), "MAPPING");
        first = false;
    }
    if let Some(mp) = &ctx.mapping_provider {
        // 4.145.0: a typeless element pointer plus its keys
        b.a(if first { "" } else { "," });
        b.a("\"mappingProvider\":{\"element\":{\"path\":");
        str(b, &mp.element);
        b.a(",\"sourceInformation\":");
        src_info(b, mp.element_span.as_deref());
        b.a("},\"keys\":[");
        strs_each(b, &mp.keys);
        b.a("],\"sourceInformation\":");
        src_info(b, mp.source_information.as_deref());
        b.ch(b'}');
        first = false;
    }
    b.a(if first { "" } else { "," });
    b.a("\"name\":");
    str(b, &ctx.name);
    b.a(",\"sourceInformation\":");
    src_info(b, ctx.source_information.as_deref());
    if let Some(td) = &ctx.test_data {
        // Reference -> DATA pointer; DataspaceTestData -> DATASPACE pointer
        let ptr_type = if td.kind == "Reference" {
            "DATA"
        } else if td.kind == "DataspaceTestData" {
            "DATASPACE"
        } else {
            return Err(illegal_state(jcat!("unprobed dataspace testData kind: ", td.kind)));
        };
        b.a(",\"testData\":{\"_type\":\"reference\",\"dataElement\":");
        pointer(b, &td.path, td.source_information.as_deref(), ptr_type);
        b.a(",\"sourceInformation\":");
        src_info(b, td.source_information.as_deref());
        b.ch(b'}');
    }
    if let Some(t) = &ctx.title {
        b.a(",\"title\":");
        str(b, t);
    }
    b.ch(b'}');
    Ok(())
}

fn data_space_executable(b: &mut Buf, e: &PDataSpaceExecutable) -> Em {
    let template = e.query.is_some();
    b.a("{\"_type\":\"");
    b.a(if template {
        "dataSpaceTemplateExecutable"
    } else {
        "dataSpacePackageableElementExecutable"
    });
    b.ch(b'"');
    if let Some(desc) = &e.description {
        b.a(",\"description\":");
        str(b, desc);
    }
    if !template {
        let Some(path) = &e.executable else {
            return Err(npe(None));
        };
        b.a(",\"executable\":{\"path\":");
        str(b, path);
        b.a(",\"sourceInformation\":");
        src_info(b, e.executable_span.as_deref());
        // a function-pointer executable (signature form) carries a type tag; a plain element pointer does not
        if path.0.contains(&u16::from(b'(')) {
            b.a(",\"type\":\"FUNCTION\"");
        }
        b.ch(b'}');
    }
    if let Some(k) = &e.execution_context_key {
        b.a(",\"executionContextKey\":");
        str(b, k);
    }
    if let Some(id) = &e.id {
        b.a(",\"id\":");
        str(b, id);
    }
    if let Some(q) = &e.query {
        b.a(",\"query\":");
        value_spec(b, q)?;
    }
    if let Some(sv) = &e.sample_values {
        // 4.145.0: a standalone relation element
        b.a(",\"sampleValues\":");
        mapping_emitter::relation_element(b, sv)?;
    }
    b.a(",\"sourceInformation\":");
    src_info(b, e.source_information.as_deref());
    b.a(",\"title\":");
    str(b, &e.title);
    b.ch(b'}');
    Ok(())
}

fn data_space_link(b: &mut Buf, l: &PDataSpaceLink) {
    b.ch(b'{');
    if let Some(label) = &l.label {
        b.a("\"label\":");
        str(b, label);
        b.ch(b',');
    }
    b.a("\"sourceInformation\":");
    src_info(b, l.source_information.as_deref());
    b.a(",\"url\":");
    str(b, &l.url);
    b.ch(b'}');
}

fn data_space_support(b: &mut Buf, s: &PDataSpaceSupport) {
    match s {
        PDataSpaceSupport::PDataSpaceSupportPSupportFull(f) => {
            // 4.145.0: the keyword-less full form
            b.a("{\"_type\":\"full\"");
            if let Some(d) = &f.documentation {
                b.a(",\"documentation\":");
                data_space_link(b, d);
            }
            if let Some(emails) = &f.emails {
                b.a(",\"emails\":[");
                for (i, em) in emails.iter().enumerate() {
                    if i > 0 {
                        b.ch(b',');
                    }
                    b.a("{\"address\":");
                    str(b, &em.address);
                    b.a(",\"sourceInformation\":");
                    src_info(b, em.source_information.as_deref());
                    b.a(",\"title\":");
                    str(b, &em.title);
                    b.ch(b'}');
                }
                b.ch(b']');
            }
            if let Some(expertise) = &f.expertise {
                b.a(",\"expertise\":[");
                for (i, ex) in expertise.iter().enumerate() {
                    if i > 0 {
                        b.ch(b',');
                    }
                    b.ch(b'{');
                    let mut first = true;
                    if let Some(d) = &ex.description {
                        b.a("\"description\":");
                        str(b, d);
                        first = false;
                    }
                    if let Some(ids) = &ex.expert_ids {
                        b.a(if first { "" } else { "," });
                        b.a("\"expertIds\":[");
                        strs_each(b, ids);
                        b.ch(b']');
                        first = false;
                    }
                    b.a(if first { "" } else { "," });
                    b.a("\"sourceInformation\":");
                    src_info(b, ex.source_information.as_deref());
                    b.ch(b'}');
                }
                b.ch(b']');
            }
            if let Some(u) = &f.faq_url {
                b.a(",\"faqUrl\":");
                data_space_link(b, u);
            }
            b.a(",\"sourceInformation\":");
            src_info(b, f.source_information.as_deref());
            if let Some(u) = &f.support_url {
                b.a(",\"supportUrl\":");
                data_space_link(b, u);
            }
            if let Some(w) = &f.website {
                b.a(",\"website\":");
                data_space_link(b, w);
            }
            b.ch(b'}');
        }
        PDataSpaceSupport::PDataSpaceSupportPSupportEmail(e) => {
            b.a("{\"_type\":\"email\",\"address\":");
            str(b, &e.address);
            if let Some(u) = &e.documentation_url {
                b.a(",\"documentationUrl\":");
                str(b, u);
            }
            b.a(",\"sourceInformation\":");
            src_info(b, e.source_information.as_deref());
            b.ch(b'}');
        }
        PDataSpaceSupport::PDataSpaceSupportPSupportCombined(cb) => {
            b.a("{\"_type\":\"combined\"");
            if let Some(u) = &cb.documentation_url {
                b.a(",\"documentationUrl\":");
                str(b, u);
            }
            if let Some(emails) = &cb.emails {
                b.a(",\"emails\":[");
                strs_each(b, emails);
                b.ch(b']');
            }
            if let Some(u) = &cb.faq_url {
                b.a(",\"faqUrl\":");
                str(b, u);
            }
            b.a(",\"sourceInformation\":");
            src_info(b, cb.source_information.as_deref());
            if let Some(u) = &cb.support_url {
                b.a(",\"supportUrl\":");
                str(b, u);
            }
            if let Some(w) = &cb.website {
                b.a(",\"website\":");
                str(b, w);
            }
            b.ch(b'}');
        }
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// Persistence
// ---------------------------------------------------------------------------------------------------------------------

/// `TailEmitter.PERSISTENCE_TYPES`: `(slot:kind) -> wire _type` for the persistence sub-DSL.
fn persistence_types(key: &str) -> Option<&'static str> {
    Some(match key {
        "trigger:Manual" => "manualTrigger",
        "trigger:Cron" => "cronTrigger",
        "persister:Batch" => "batchPersister",
        "persister:Streaming" => "streamingPersister",
        "sink:Relational" => "relationalSink",
        "sink:ObjectStorage" => "objectStorageSink",
        "ingestMode:BitemporalSnapshot" => "bitemporalSnapshot",
        "ingestMode:NontemporalSnapshot" => "nontemporalSnapshot",
        "ingestMode:UnitemporalSnapshot" => "unitemporalSnapshot",
        "ingestMode:NontemporalDelta" => "nontemporalDelta",
        "ingestMode:UnitemporalDelta" => "unitemporalDelta",
        "ingestMode:BitemporalDelta" => "bitemporalDelta",
        "ingestMode:AppendOnly" => "appendOnly",
        "transactionMilestoning:BatchIdAndDateTime" => "batchIdAndDateTimeTransactionMilestoning",
        "transactionMilestoning:BatchId" => "batchIdTransactionMilestoning",
        "transactionMilestoning:DateTime" => "dateTimeTransactionMilestoning",
        "validityMilestoning:DateTime" => "dateTimeValidityMilestoning",
        "derivation:SourceSpecifiesFromDateTime" => "sourceSpecifiesFromDateTime",
        "derivation:SourceSpecifiesFromAndThruDateTime" => "sourceSpecifiesFromAndThruDateTime",
        "derivation:SourceSpecifiesInDateTime" => "sourceSpecifiesInDateTime",
        "derivation:SourceSpecifiesInAndOutDateTime" => "sourceSpecifiesInAndOutDateTime",
        "targetShape:Flat" => "flatTarget",
        "targetShape:MultiFlat" => "multiFlatTarget",
        "notifyee:Email" => "emailNotifyee",
        "notifyee:PagerDuty" => "pagerDutyNotifyee",
        "serviceOutput:TDS" => "tdsServiceOutput",
        "serviceOutput:GraphFetch" => "graphFetchServiceOutput",
        "persistenceTarget:Relational" => "relationalPersistenceTarget",
        "datasetType:Snapshot" => "snapshot",
        "datasetType:Delta" => "delta",
        "partitioning:None" => "noPartitioning",
        "partitioning:FieldBased" => "fieldBasedForTds",
        "partitioning@gf:FieldBased" => "fieldBasedForGraphFetch",
        "deduplicationStrategy:DuplicateCount" => "duplicateCountDeduplicationStrategy",
        "deduplication:None" => "noDeduplication",
        "deduplication:AnyVersion" => "anyVersion",
        "deduplication:MaxVersion" => "maxVersionForTds",
        "deduplication@gf:MaxVersion" => "maxVersionForGraphFetch",
        "emptyDatasetHandling:NoOp" => "noOp",
        "emptyDatasetHandling:DeleteTargetData" => "deleteTargetDataset",
        "updatesHandling:Overwrite" => "overwrite",
        "updatesHandling:AppendOnly" => "appendOnly",
        "temporality:None" => "none",
        "temporality:Unitemporal" => "unitemporalTemporality",
        "temporality:Bitemporal" => "bitemporalTemporality",
        "auditing@tgt:DateTime" => "auditingDateTime",
        "connectionData:ExternalFormat" => "externalFormat",
        "assertion:EqualToJson" => "equalToJson",
        "platform:AwsGlue" => "awsGlue",
        "sourceFields:StartAndEnd" => "sourceTimeStartAndEnd",
        "sourceFields:Start" => "sourceTimeStart",
        "processingDimension:BatchId" => "batchId",
        "processingDimension:DateTime" => "processingTime",
        // the relational-target protocol names this subtype batchIdAndProcessingTime, not the kind spelling
        "processingDimension:BatchIdAndDateTime" => "batchIdAndProcessingTime",
        "sourceDerivedDimension:DateTime" => "sourceDerivedTime",
        "appendStrategy:AllowDuplicates" => "allowDuplicates",
        "appendStrategy:FailOnDuplicates" => "failOnDuplicates",
        "appendStrategy:FilterDuplicates" => "filterDuplicates",
        "deduplicationStrategy:None" => "noDeduplicationStrategy",
        "deduplicationStrategy:MaxVersion" => "maxVersionDeduplicationStrategy",
        "deduplicationStrategy:AnyVersion" => "anyVersionDeduplicationStrategy",
        "actionIndicator@gf:DeleteIndicator" => "deleteIndicatorForGraphFetch",
        "actionIndicator:None" => "noActionIndicator",
        "actionIndicator@gf:None" => "noActionIndicator",
        "actionIndicator:DeleteIndicator" => "deleteIndicatorForTds",
        "auditing:None" => "noAuditing",
        "auditing:DateTime" => "dateTimeAuditing",
        "mergeStrategy:NoDeletes" => "noDeletesMergeStrategy",
        "mergeStrategy:DeleteIndicator" => "deleteIndicatorMergeStrategy",
        _ => return None,
    })
}

/// A table lookup over the key's units (a key holding a lone surrogate is in no table).
fn lookup(slot: &str, sep: &str, kind: &JStr) -> Option<&'static str> {
    let kind = String::from_utf16(&kind.0).ok()?;
    persistence_types(&format!("{slot}{sep}{kind}"))
}

/// `TailEmitter.persistenceType(slot, kind, gf, tgt)`.
fn persistence_type(slot: &[u16], kind: &JStr, gf: bool, tgt: bool) -> Result<&'static str, JavaError> {
    let slot_s = String::from_utf16(slot).ok();
    let mut t = None;
    if let Some(slot_s) = &slot_s {
        if tgt {
            t = lookup(slot_s, "@tgt:", kind);
        }
        if t.is_none() && gf {
            t = lookup(slot_s, "@gf:", kind);
        }
        if t.is_none() {
            t = lookup(slot_s, ":", kind);
        }
    }
    t.ok_or_else(|| {
        illegal_state(jcat!(
            "unprobed persistence node: ",
            slot,
            ":",
            kind,
            if gf { " (graphFetch)" } else { "" },
            if tgt { " (target)" } else { "" }
        ))
    })
}

pub fn persistence(b: &mut Buf, p: &PPersistence) -> Em {
    b.a("{\"_type\":\"persistence\"");
    if let Some(doc) = &p.doc {
        b.a(",\"documentation\":");
        str(b, doc);
    }
    b.a(",\"name\":");
    str(b, &p.name);
    b.a(",\"notifier\":{\"notifyees\":[");
    if let Some(nf) = &p.notifier {
        comma_each(b, &nf.notifyees, |b, n| {
            persistence_node(b, &crate::ustr::u("notifyee"), n, false, false)
        })?;
    }
    b.ch(b']');
    if let Some(nf) = &p.notifier
        && let Some(span) = &nf.source_information
    {
        b.a(",\"sourceInformation\":");
        src_info(b, Some(span));
    }
    b.a("},\"package\":");
    str(b, &p.pkg);
    if let Some(persister) = &p.persister {
        b.a(",\"persister\":");
        persistence_node(b, &crate::ustr::u("persister"), persister, false, false)?;
    }
    if let Some(service) = &p.service {
        b.a(",\"service\":");
        pointer(b, service, p.service_span.as_deref(), "SERVICE");
    }
    b.a(",\"serviceOutputTargets\":[");
    if let Some(targets) = &p.service_output_targets {
        for (i, t) in targets.iter().enumerate() {
            if i > 0 {
                b.ch(b',');
            }
            b.ch(b'{');
            if t.persistence_target.kind != "__empty__" {
                b.a("\"persistenceTarget\":");
                persistence_node(
                    b,
                    &crate::ustr::u("persistenceTarget"),
                    &t.persistence_target,
                    false,
                    false,
                )?;
                b.ch(b',');
            }
            b.a("\"serviceOutput\":");
            persistence_node(b, &crate::ustr::u("serviceOutput"), &t.service_output, false, false)?;
            b.a(",\"sourceInformation\":");
            src_info(b, t.source_information.as_deref());
            b.ch(b'}');
        }
    }
    b.a("],\"sourceInformation\":");
    src_info(b, p.source_information.as_deref());
    if let Some(tests) = &p.tests {
        b.a(",\"tests\":[");
        comma_each(b, tests, |b, t| persistence_test(b, t))?;
        b.ch(b']');
    }
    b.a(",\"trigger\":{\"_type\":\"");
    b.a(persistence_type(
        &crate::ustr::u("trigger"),
        &p.trigger_kind,
        false,
        false,
    )?);
    b.a("\"}}");
    Ok(())
}

/// An entry's key (`PPersistenceEntry.key()`).
fn entry_key(e: &PPersistenceEntry) -> &JStr {
    match e {
        PPersistenceEntry::PPersistenceEntryScalar(x) => &x.key,
        PPersistenceEntry::PPersistenceEntryNode(x) => &x.key,
        PPersistenceEntry::PPersistenceEntryPointer(x) => &x.key,
        PPersistenceEntry::PPersistenceEntryStrings(x) => &x.key,
        PPersistenceEntry::PPersistenceEntryPathValue(x) => &x.key,
        PPersistenceEntry::PPersistenceEntryNodeList(x) => &x.key,
        PPersistenceEntry::PPersistenceEntryPathList(x) => &x.key,
    }
}

/// An entry's Java class's `getSimpleName()`.
fn entry_simple_name(e: &PPersistenceEntry) -> &'static str {
    match e {
        PPersistenceEntry::PPersistenceEntryScalar(_) => "Scalar",
        PPersistenceEntry::PPersistenceEntryNode(_) => "Node",
        PPersistenceEntry::PPersistenceEntryPointer(_) => "Pointer",
        PPersistenceEntry::PPersistenceEntryStrings(_) => "Strings",
        PPersistenceEntry::PPersistenceEntryPathValue(_) => "PathValue",
        PPersistenceEntry::PPersistenceEntryNodeList(_) => "NodeList",
        PPersistenceEntry::PPersistenceEntryPathList(_) => "PathList",
    }
}

/// `TailEmitter.wireKey(e)`: `deleteField` and `versionField` path values wire as `...Path`.
fn wire_key_plain(e: &PPersistenceEntry) -> Vec<u16> {
    let key = entry_key(e);
    if let PPersistenceEntry::PPersistenceEntryPathValue(_) = e {
        if *key == "deleteField" {
            return crate::ustr::u("deleteFieldPath");
        }
        if *key == "versionField" {
            return crate::ustr::u("versionFieldPath");
        }
    }
    key.0.clone()
}

/// `TailEmitter.wireKey(e, tgt)`: the v2 target dialect's key renames.
fn wire_key(e: &PPersistenceEntry, tgt: bool) -> Vec<u16> {
    if !tgt {
        return wire_key_plain(e);
    }
    let key = entry_key(e);
    let renamed = if *key == "batchDateTimeName" || *key == "dateTimeName" {
        "auditingDateTimeName"
    } else if *key == "dateTimeIn" {
        "timeIn"
    } else if *key == "dateTimeOut" {
        "timeOut"
    } else if *key == "dateTimeStart" {
        "timeStart"
    } else if *key == "dateTimeEnd" {
        "timeEnd"
    } else if *key == "sourceFields" {
        "sourceTimeFields"
    } else {
        return wire_key_plain(e);
    };
    crate::ustr::u(renamed)
}

/// An entry as the walk holds it: the node's own, or the path head's synthesized `path` value.
enum EntryRef<'a> {
    Own(&'a PPersistenceEntry),
    HeadPath(&'a ValueSpecification),
}

/// A node kind the emitter's tables do not name: the engine's grammar names the kinds too, and refuses the word at the
/// node's place (PARITY row 73). A platform written with an island is the exception: the engine accepts it, so the
/// answer stays (the user's list, ruling 4 of docs/rust-port/PHASE2.md section 6).
fn unknown_persistence_kind(
    raw: JavaError,
    slot: &[u16],
    n: &PPersistenceNode,
    outer: Option<&SourceInfo>,
) -> JavaError {
    match n.source_information.as_deref().or(outer) {
        Some(at) if !eq(slot, "platform") => {
            JavaError::parse(jcat!("Unexpected token '", n.kind, "'"), at.start_line, at.start_column).as_refusal()
        }
        _ => raw,
    }
}

/// The generic node walk: `_type` first, entries alphabetical by wire key with `sourceInformation` merged into sorted
/// position. `gf`: inside a graphFetch service output; `tgt`: inside a persistenceTarget.
fn persistence_node(b: &mut Buf, slot: &[u16], n: &PPersistenceNode, gf: bool, tgt: bool) -> Em {
    persistence_node_in(b, slot, n, gf, tgt, None)
}

/// [`persistence_node`], `outer` the span of the nearest enclosing node that keeps one (the walker writes few).
fn persistence_node_in(
    b: &mut Buf,
    slot: &[u16],
    n: &PPersistenceNode,
    gf: bool,
    tgt: bool,
    outer: Option<&SourceInfo>,
) -> Em {
    let part = n.kind == "__part__";
    let path_head = n.kind == "#path";
    let gf = gf || path_head;
    let tgt = tgt || eq(slot, "persistenceTarget");
    let wire_type = if part {
        None
    } else if path_head {
        Some("graphFetchServiceOutput")
    } else {
        Some(persistence_type(slot, &n.kind, gf, tgt).map_err(|e| unknown_persistence_kind(e, slot, n, outer))?)
    };
    b.ch(b'{');
    let mut first = true;
    if let Some(wt) = wire_type {
        b.a("\"_type\":\"");
        b.a(wt);
        b.ch(b'"');
        first = false;
    }
    let mut entries: Vec<EntryRef<'_>> = n.entries.iter().map(EntryRef::Own).collect();
    if path_head {
        let Some(head) = &n.head_path else {
            return Err(npe(None));
        };
        entries.push(EntryRef::HeadPath(head));
    }
    let gf_partition = gf && wire_type == Some("fieldBasedForGraphFetch");
    let key_of = |e: &EntryRef<'_>| -> Vec<u16> {
        let k = match e {
            EntryRef::Own(e) => wire_key(e, tgt),
            // a PathValue keyed `path`: neither rename applies
            EntryRef::HeadPath(_) => crate::ustr::u("path"),
        };
        if gf_partition && eq(&k, "partitionFields") {
            crate::ustr::u("partitionFieldPaths")
        } else {
            k
        }
    };
    let mut keyed: Vec<(Vec<u16>, EntryRef<'_>)> = entries.into_iter().map(|e| (key_of(&e), e)).collect();
    keyed.sort_by(|x, y| x.0.cmp(&y.0));
    let source_information = crate::ustr::u("sourceInformation");
    let mut span_emitted = false;
    for (key, e) in &keyed {
        if !span_emitted && key[..] > source_information[..] {
            if !first {
                b.ch(b',');
            }
            first = false;
            b.a("\"sourceInformation\":");
            src_info(b, n.source_information.as_deref());
            span_emitted = true;
        }
        if !first {
            b.ch(b',');
        }
        first = false;
        b.ch(b'"');
        b.units(key);
        b.a("\":");
        let e = match e {
            EntryRef::Own(e) => e,
            EntryRef::HeadPath(spec) => {
                emitter::path_value(b, as_path_literal(spec)?)?;
                continue;
            }
        };
        match e {
            PPersistenceEntry::PPersistenceEntryScalar(s) => {
                if !s.quoted
                    && (s.value == "true"
                        || s.value == "false"
                        || s.value.0.iter().all(|&c| lite_javacompat::character::is_digit(c)))
                {
                    b.units(&s.value.0);
                } else {
                    str(b, &s.value);
                }
            }
            PPersistenceEntry::PPersistenceEntryNode(nd) => persistence_node_in(
                b,
                &nd.key.0,
                &nd.node,
                gf,
                tgt,
                n.source_information.as_deref().or(outer),
            )?,
            PPersistenceEntry::PPersistenceEntryPointer(pt) => {
                if pt.key == "binding" {
                    // binding pointers carry no type tag
                    b.a("{\"path\":");
                    str(b, &pt.path);
                    b.a(",\"sourceInformation\":");
                    src_info(b, pt.source_information.as_deref());
                    b.ch(b'}');
                } else {
                    pointer(b, &pt.path, pt.source_information.as_deref(), "STORE");
                }
            }
            PPersistenceEntry::PPersistenceEntryStrings(ss) => {
                b.ch(b'[');
                strs_each(b, &ss.values);
                b.ch(b']');
            }
            PPersistenceEntry::PPersistenceEntryPathValue(pv) => emitter::path_value(b, as_path_literal(&pv.spec)?)?,
            PPersistenceEntry::PPersistenceEntryPathList(pl) => {
                b.ch(b'[');
                for (i, spec) in pl.specs.iter().enumerate() {
                    if i > 0 {
                        b.ch(b',');
                    }
                    emitter::path_value(b, as_path_literal(spec)?)?;
                }
                b.ch(b']');
            }
            PPersistenceEntry::PPersistenceEntryNodeList(nl) => {
                b.ch(b'[');
                let within = n.source_information.as_deref().or(outer);
                comma_each(b, &nl.nodes, |b, node| {
                    persistence_node_in(b, &nl.key.0, node, gf, tgt, within)
                })?;
                b.ch(b']');
            }
        }
    }
    if !span_emitted {
        if !first {
            b.ch(b',');
        }
        b.a("\"sourceInformation\":");
        src_info(b, n.source_information.as_deref());
    }
    b.ch(b'}');
    Ok(())
}

pub fn persistence_context(b: &mut Buf, pc: &PPersistenceContext) -> Em {
    b.a("{\"_type\":\"persistenceContext\",\"name\":");
    str(b, &pc.name);
    b.a(",\"package\":");
    str(b, &pc.pkg);
    b.a(",\"persistence\":");
    pointer(b, &pc.persistence, pc.persistence_span.as_deref(), "PERSISTENCE");
    // platform is always on the wire -- {_type:"default"} when unspelled
    b.a(",\"platform\":");
    match &pc.platform {
        None => b.a("{\"_type\":\"default\"}"),
        Some(platform) if platform.entries.is_empty() => {
            // a bare kind: `platform: Default;` -- _type is the kind lower-cased first, with its span
            let k = &platform.kind.0;
            let Some(&c0) = k.first() else {
                return Err(JavaError::java(
                    "java.lang.StringIndexOutOfBoundsException",
                    JStr::from("Index 0 out of bounds for length 0"),
                ));
            };
            b.a("{\"_type\":\"");
            b.u.push(char_to_lower_case(c0));
            b.units(&k[1..]);
            b.a("\",\"sourceInformation\":");
            src_info(b, platform.source_information.as_deref());
            b.ch(b'}');
        }
        Some(platform) => persistence_node(b, &crate::ustr::u("platform"), platform, false, false)?,
    }
    b.a(",\"serviceParameters\":[");
    for (i, p) in pc.service_parameters.iter().enumerate() {
        if i > 0 {
            b.ch(b',');
        }
        b.a("{\"name\":");
        str(b, &p.name);
        b.a(",\"sourceInformation\":");
        src_info(b, p.source_information.as_deref());
        b.a(",\"value\":");
        match &p.value {
            PCtxParamValue::PCtxParamValuePrimitive(pv) => {
                b.a("{\"_type\":\"primitiveTypeValue\",\"primitiveType\":");
                value_spec(b, &pv.spec)?;
                b.ch(b'}');
            }
            PCtxParamValue::PCtxParamValueConnectionPtr(cp) => {
                b.a("{\"_type\":\"connectionValue\",\"connection\":{\"_type\":\"connectionPointer\",\"connection\":");
                str(b, &cp.path);
                b.a(",\"sourceInformation\":");
                src_info(b, cp.source_information.as_deref());
                b.a("}}");
            }
            PCtxParamValue::PCtxParamValueConnectionVal(cv) => {
                b.a("{\"_type\":\"connectionValue\",\"connection\":");
                emitter::connection_value(b, &cv.connection)?;
                b.ch(b'}');
            }
        }
        b.ch(b'}');
    }
    b.ch(b']');
    if let Some(sink) = &pc.sink_connection {
        b.a(",\"sinkConnection\":");
        emitter::connection_value(b, sink)?;
    }
    b.a(",\"sourceInformation\":");
    src_info(b, pc.source_information.as_deref());
    b.ch(b'}');
    Ok(())
}

/// `Character.toLowerCase(char)`: the simple lower-case mapping of one UTF-16 unit (a surrogate is itself).
fn char_to_lower_case(c: u16) -> u16 {
    if c < 0x80 {
        return u16::from((c as u8).to_ascii_lowercase());
    }
    match char::from_u32(u32::from(c)) {
        Some(ch) => {
            let mut lower = ch.to_lowercase();
            match (lower.next(), lower.next()) {
                (Some(l), None) if (l as u32) < 0x10000 => l as u32 as u16,
                _ => c,
            }
        }
        None => c,
    }
}

fn persistence_test(b: &mut Buf, t: &PPersistenceTest) -> Em {
    b.a("{\"_type\":\"test\"");
    if let Some(gfp) = &t.graph_fetch_path {
        b.a(",\"graphFetchPath\":");
        emitter::path_value(b, as_path_literal(gfp)?)?;
    }
    b.a(",\"id\":");
    str(b, &t.id);
    // always written, a null as null: left out it would read back as the engine's true (S37)
    b.a(",\"isTestDataFromServiceOutput\":");
    match t.is_test_data_from_service_output {
        Some(v) => b.boolean(v),
        None => b.a("null"),
    }
    b.a(",\"sourceInformation\":");
    src_info(b, t.source_information.as_deref());
    if let Some(batches) = &t.test_batches {
        b.a(",\"testBatches\":[");
        persistence_test_batches(b, batches)?;
        b.ch(b']');
    }
    b.ch(b'}');
    Ok(())
}

/// Each batch: its assertions, test data and the data's connection only when it has them.
fn persistence_test_batches(b: &mut Buf, batches: &[Box<PPersistenceTestBatch>]) -> Em {
    for (i, tb) in batches.iter().enumerate() {
        if i > 0 {
            b.ch(b',');
        }
        b.ch(b'{');
        if let Some(asserts) = &tb.asserts {
            b.a("\"assertions\":[");
            persistence_asserts(b, asserts)?;
            b.a("],");
        }
        // batchId auto-numbers in source order
        b.a("\"batchId\":");
        b.int(i as i64);
        b.a(",\"id\":");
        str(b, &tb.id);
        b.a(",\"sourceInformation\":");
        src_info(b, tb.source_information.as_deref());
        if tb.has_test_data {
            b.a(",\"testData\":{");
            if let Some(connection) = &tb.connection_data {
                b.a("\"connection\":{\"data\":");
                persistence_node(b, &crate::ustr::u("connectionData"), connection, false, false)?;
                b.a(",\"sourceInformation\":");
                src_info(b, tb.connection_span.as_deref());
                b.a("},");
            }
            b.a("\"sourceInformation\":");
            src_info(b, tb.data_span.as_deref());
            b.ch(b'}');
        }
        b.ch(b'}');
    }
    Ok(())
}

fn persistence_asserts(b: &mut Buf, asserts: &[Box<PPersistenceAssert>]) -> Em {
    for (a, as_) in asserts.iter().enumerate() {
        if a > 0 {
            b.ch(b',');
        }
        b.a("{\"_type\":\"");
        b.a(
            persistence_type(&crate::ustr::u("assertion"), &as_.assertion.kind, false, false).map_err(|raw| {
                // the engine's walker refuses a test assertion kind it does not know, at the kind (PARITY row 73)
                match as_
                    .assertion
                    .source_information
                    .as_deref()
                    .or(as_.source_information.as_deref())
                {
                    Some(at) => JavaError::parse(
                        jcat!("Unknown test assertion type: ", as_.assertion.kind),
                        at.start_line,
                        at.start_column,
                    )
                    .as_refusal(),
                    None => raw,
                }
            })?,
        );
        b.ch(b'"');
        for e in &as_.assertion.entries {
            b.a(",\"");
            b.units(&entry_key(e).0);
            b.a("\":");
            match e {
                PPersistenceEntry::PPersistenceEntryNode(nd) => {
                    persistence_node(b, &crate::ustr::u("connectionData"), &nd.node, false, false)?;
                }
                PPersistenceEntry::PPersistenceEntryScalar(s) => str(b, &s.value),
                other => {
                    return Err(unsupported(jcat!(
                        "TailEmitter has no wire rule for persistence assertion entry '",
                        entry_key(other),
                        "' (",
                        entry_simple_name(other),
                        ") \u{2014} probe, do not guess."
                    )));
                }
            }
        }
        b.a(",\"id\":");
        str(b, &as_.id);
        b.a(",\"sourceInformation\":");
        src_info(b, as_.source_information.as_deref());
        b.ch(b'}');
    }
    Ok(())
}

/// One legacy assert entry: the assert lambda and its parameter values; a `list([...])` value wired as a
/// `listInstance`.
fn legacy_assert(b: &mut Buf, a: &PLegacyServiceTestPLegacyAssert) -> Em {
    b.a("{\"assert\":");
    value_spec(b, &a.assertion)?;
    b.a(",\"parametersValues\":[");
    for (i, v) in a.parameters_values.iter().enumerate() {
        if i > 0 {
            b.ch(b',');
        }
        match v {
            ValueSpecification::AppliedFunction(af)
                if af.function == "list"
                    && af.parameters.len() == 1
                    && matches!(af.parameters[0], ValueSpecification::PureCollection(_)) =>
            {
                let ValueSpecification::PureCollection(pc) = &af.parameters[0] else {
                    unreachable!("matched above")
                };
                // list([...]) wires as classInstance/listInstance; outer and inner span the whole call's
                b.a("{\"_type\":\"classInstance\",\"sourceInformation\":");
                src_info(b, af.pos.as_deref());
                b.a(",\"type\":\"listInstance\",\"value\":{\"sourceInformation\":");
                src_info(b, af.pos.as_deref());
                b.a(",\"values\":[");
                comma_each(b, &pc.values, value_spec)?;
                b.a("]}}");
            }
            _ => value_spec(b, v)?,
        }
    }
    b.a("],\"sourceInformation\":");
    src_info(b, a.source_information.as_deref());
    b.ch(b'}');
    Ok(())
}

// ---------------------------------------------------------------------------------------------------------------------
// Service and ExecutionEnvironment
// ---------------------------------------------------------------------------------------------------------------------

pub fn service(b: &mut Buf, sv: &PService) -> Em {
    b.a("{\"_type\":\"service\"");
    b.a(",\"autoActivateUpdates\":");
    b.boolean(sv.auto_activate_updates);
    if let Some(doc) = &sv.documentation {
        b.a(",\"documentation\":");
        str(b, doc);
    }
    b.a(",\"execution\":");
    service_execution(b, &sv.execution)?;
    if let Some(m) = &sv.mcp_server {
        b.a(",\"mcpServer\":");
        str(b, m);
    }
    b.a(",\"name\":");
    str(b, &sv.name);
    b.a(",\"owners\":[");
    strs_each(b, &sv.owners);
    b.ch(b']');
    if let Some(kind) = &sv.ownership_kind {
        if *kind != "DID" && *kind != "UserList" {
            return Err(illegal_state(jcat!("unprobed service ownership kind: ", kind)));
        }
        if let Some(users) = &sv.ownership_users {
            b.a(",\"ownership\":{\"_type\":\"userListOwnership\",\"users\":[");
            strs_each(b, users);
            b.a("]}");
        } else {
            b.a(",\"ownership\":{\"_type\":\"deploymentOwnership\",\"identifier\":");
            let Some(id) = &sv.ownership_id else {
                return Err(npe(None));
            };
            str(b, id);
            b.ch(b'}');
        }
    }
    b.a(",\"package\":");
    str(b, &sv.pkg);
    if let Some(p) = &sv.pattern {
        b.a(",\"pattern\":");
        str(b, p);
    }
    b.a(",\"postValidations\":[");
    if let Some(pvs) = &sv.post_validations {
        for (i, pv) in pvs.iter().enumerate() {
            if i > 0 {
                b.ch(b',');
            }
            b.a("{\"assertions\":[");
            for (j, a) in pv.assertions.iter().enumerate() {
                if j > 0 {
                    b.ch(b',');
                }
                b.a("{\"assertion\":");
                value_spec(b, &a.assertion)?;
                b.a(",\"id\":");
                str(b, &a.id);
                b.a(",\"sourceInformation\":");
                src_info(b, a.source_information.as_deref());
                b.ch(b'}');
            }
            b.a("],\"description\":");
            str(b, &pv.description);
            b.a(",\"parameters\":[");
            comma_each(b, &pv.parameters, value_spec)?;
            b.a("],\"sourceInformation\":");
            src_info(b, pv.source_information.as_deref());
            b.ch(b'}');
        }
    }
    b.a("],\"sourceInformation\":");
    src_info(b, sv.source_information.as_deref());
    b.a(",\"stereotypes\":");
    emitter::stereotypes(b, &sv.stereotypes);
    b.a(",\"taggedValues\":");
    emitter::tagged_values(b, &sv.tagged_values);
    let mut test = sv.test.as_deref();
    if let Some(t) = test
        && t.kind == "Multi"
    {
        b.a(",\"test\":{\"_type\":\"multiExecutionTest\",\"sourceInformation\":");
        src_info(b, t.source_information.as_deref());
        b.a(",\"tests\":[");
        for (i, kt) in t.keyed_tests.iter().enumerate() {
            if i > 0 {
                b.ch(b',');
            }
            b.a("{\"asserts\":[");
            comma_each(b, &kt.asserts, |b, a| legacy_assert(b, a))?;
            b.a("],\"data\":");
            str(b, &kt.data);
            b.a(",\"key\":");
            str(b, &kt.key);
            b.a(",\"sourceInformation\":");
            src_info(b, kt.source_information.as_deref());
            b.ch(b'}');
        }
        b.a("]}");
        test = None;
    }
    if let Some(t) = test {
        b.a(",\"test\":{\"_type\":\"singleExecutionTest\",\"asserts\":[");
        comma_each(b, &t.asserts, |b, a| legacy_assert(b, a))?;
        b.a("],\"data\":");
        let Some(data) = &t.data else {
            return Err(npe(None));
        };
        str(b, data);
        b.a(",\"sourceInformation\":");
        src_info(b, t.source_information.as_deref());
        b.ch(b'}');
    }
    if let Some(suites) = &sv.test_suites {
        b.a(",\"testSuites\":[");
        comma_each(b, suites, |b, s| service_test_suite(b, s))?;
        b.ch(b']');
    }
    if let Some(t) = &sv.title {
        b.a(",\"title\":");
        str(b, t);
    }
    b.ch(b'}');
    Ok(())
}

pub fn execution_environment(b: &mut Buf, ee: &PExecutionEnvironment) -> Em {
    b.a("{\"_type\":\"executionEnvironmentInstance\",\"executionParameters\":[");
    for (i, e) in ee.executions.iter().enumerate() {
        if i > 0 {
            b.ch(b',');
        }
        match e {
            PExecutionParameters::PKeyedExecution(single) => single_execution_parameters(b, single)?,
            PExecutionParameters::PMultiKeyedExecution(m) => {
                // no span of its own; masterKey then the singles
                b.a("{\"_type\":\"multiExecutionParameters\",\"masterKey\":");
                str(b, &m.master_key);
                b.a(",\"singleExecutionParameters\":[");
                for (j, s) in m.singles.iter().enumerate() {
                    if j > 0 {
                        b.ch(b',');
                    }
                    single_execution_parameters(b, s)?;
                }
                b.a("]}");
            }
        }
    }
    b.a("],\"name\":");
    str(b, &ee.name);
    b.a(",\"package\":");
    str(b, &ee.pkg);
    b.a(",\"sourceInformation\":");
    src_info(b, ee.source_information.as_deref());
    b.ch(b'}');
    Ok(())
}

fn single_execution_parameters(b: &mut Buf, k: &PKeyedExecution) -> Em {
    b.a("{\"_type\":\"singleExecutionParameters\",\"key\":");
    str(b, &k.key_value);
    if let Some(m) = &k.mapping {
        b.a(",\"mapping\":");
        str(b, m);
        b.a(",\"mappingSourceInformation\":");
        src_info(b, k.mapping_span.as_deref());
    }
    if k.runtime.is_some() || k.embedded_runtime.is_some() {
        // "runtime" sorts before "runtimeComponents"; a keyed execution may carry both
        service_runtime(
            b,
            k.runtime.as_ref(),
            k.runtime_span.as_deref(),
            k.embedded_runtime.as_deref(),
        )?;
    }
    if let Some(rc) = &k.runtime_components {
        b.a(",\"runtimeComponents\":{\"binding\":");
        emitter::pointer(b, &rc.binding);
        b.a(",\"clazz\":");
        emitter::pointer(b, &rc.clazz);
        b.a(",\"runtime\":{\"_type\":\"runtimePointer\",\"runtime\":");
        str(b, &rc.runtime);
        b.a(",\"sourceInformation\":");
        src_info(b, rc.runtime_span.as_deref());
        b.a("}}");
    }
    b.a(",\"sourceInformation\":");
    src_info(b, k.source_information.as_deref());
    b.ch(b'}');
    Ok(())
}

fn test_assertion_expected(b: &mut Buf, expected: &PAssertionValue) -> Em {
    match expected {
        PAssertionValue::PExternalFormatData(ef) => {
            b.a("{\"_type\":\"equalToJson\",\"expected\":");
            mapping_emitter::external_format_data(b, ef)?;
        }
        PAssertionValue::PRelationElement(re) => {
            // the compact form's `=> Relation #{...}#`: a bare relation element
            b.a("{\"_type\":\"equalToRelation\",\"expected\":");
            mapping_emitter::relation_element(b, re)?;
        }
        PAssertionValue::PEqualToValue(ev) => {
            b.a("{\"_type\":\"equalTo\",\"expected\":");
            value_spec(b, &ev.value)?;
        }
    }
    Ok(())
}

fn service_test_suite(b: &mut Buf, st: &PServiceTestSuite) -> Em {
    b.a("{\"_type\":\"serviceTestSuite\"");
    if let Some(doc) = &st.doc {
        b.a(",\"doc\":");
        str(b, doc);
    }
    b.a(",\"id\":");
    str(b, &st.id);
    b.a(",\"sourceInformation\":");
    src_info(b, st.source_information.as_deref());
    if let Some(td) = &st.test_data {
        b.a(",\"testData\":{");
        let cds = &td.connections_test_data;
        if !cds.is_empty() {
            b.a("\"connectionsTestData\":[");
            for (i, cd) in cds.iter().enumerate() {
                if i > 0 {
                    b.ch(b',');
                }
                b.a("{\"data\":");
                mapping_emitter::embedded_data_value(b, &cd.data)?;
                b.a(",\"id\":");
                str(b, &cd.id);
                b.a(",\"sourceInformation\":");
                src_info(b, cd.source_information.as_deref());
                b.ch(b'}');
            }
            b.a("],");
        }
        if let Some(rds) = &td.service_test_data
            && !rds.is_empty()
        {
            // compact-form resolver entries: the elementPointer carries no "type" key
            b.a("\"serviceTestData\":[");
            for (i, rd) in rds.iter().enumerate() {
                if i > 0 {
                    b.ch(b',');
                }
                b.a(if rd.data.is_some() {
                    "{\"_type\":\"baseDataResolver\",\"data\":"
                } else {
                    "{\"_type\":\"referenceDataResolver\""
                });
                if let Some(data) = &rd.data {
                    mapping_emitter::embedded_data_value(b, data)?;
                }
                b.a(",\"elementPointer\":{\"path\":");
                str(b, &rd.element_path);
                b.a(",\"sourceInformation\":");
                src_info(b, rd.element_source_information.as_deref());
                b.a("},\"sourceInformation\":");
                src_info(b, rd.source_information.as_deref());
                b.ch(b'}');
            }
            b.a("],");
        }
        b.a("\"sourceInformation\":");
        src_info(b, td.source_information.as_deref());
        b.ch(b'}');
    }
    b.a(",\"tests\":[");
    for (i, t) in st.tests.iter().enumerate() {
        if i > 0 {
            b.ch(b',');
        }
        b.a("{\"_type\":\"serviceTest\",\"assertions\":[");
        for (j, a) in t.assertions.iter().enumerate() {
            if j > 0 {
                b.ch(b',');
            }
            test_assertion_expected(b, &a.expected)?;
            b.a(",\"id\":");
            str(b, &a.id);
            b.a(",\"sourceInformation\":");
            src_info(b, a.source_information.as_deref());
            b.ch(b'}');
        }
        b.ch(b']');
        if let Some(doc) = &t.doc {
            b.a(",\"doc\":");
            str(b, doc);
        }
        b.a(",\"id\":");
        str(b, &t.id);
        b.a(",\"keys\":[");
        strs_each(b, &t.keys);
        b.ch(b']');
        if let Some(params) = &t.parameters {
            b.a(",\"parameters\":[");
            for (j, p) in params.iter().enumerate() {
                if j > 0 {
                    b.ch(b',');
                }
                b.a("{\"name\":");
                str(b, &p.name);
                b.a(",\"value\":");
                param_value(b, &p.value)?;
                b.ch(b'}');
            }
            b.ch(b']');
        }
        if let Some(f) = &t.serialization_format {
            b.a(",\"serializationFormat\":");
            str(b, f);
        }
        b.a(",\"sourceInformation\":");
        src_info(b, t.source_information.as_deref());
        b.ch(b'}');
    }
    b.a("]}");
    Ok(())
}

fn service_execution(b: &mut Buf, e: &PServiceExecution) -> Em {
    match e {
        PServiceExecution::PSingleExecution(se) => {
            b.a("{\"_type\":\"pureSingleExecution\",\"func\":");
            value_spec(b, &se.query)?;
            if let Some(m) = &se.mapping {
                b.a(",\"mapping\":");
                str(b, m);
                b.a(",\"mappingSourceInformation\":");
                src_info(b, se.mapping_span.as_deref());
            }
            service_runtime(
                b,
                se.runtime.as_ref(),
                se.runtime_span.as_deref(),
                se.embedded_runtime.as_deref(),
            )?;
            b.a(",\"sourceInformation\":");
            src_info(b, se.source_information.as_deref());
            b.ch(b'}');
        }
        PServiceExecution::PMultiExecution(me) => {
            b.a("{\"_type\":\"pureMultiExecution\"");
            if let Some(k) = &me.execution_key {
                b.a(",\"executionKey\":");
                str(b, k);
            }
            let Some(executions) = &me.executions else {
                b.a(",\"func\":");
                value_spec(b, &me.query)?;
                b.a(",\"sourceInformation\":");
                src_info(b, me.source_information.as_deref());
                b.ch(b'}');
                return Ok(());
            };
            b.a(",\"executionParameters\":[");
            for (i, k) in executions.iter().enumerate() {
                if i > 0 {
                    b.ch(b',');
                }
                b.a("{\"key\":");
                str(b, &k.key_value);
                if let Some(m) = &k.mapping {
                    b.a(",\"mapping\":");
                    str(b, m);
                    b.a(",\"mappingSourceInformation\":");
                    src_info(b, k.mapping_span.as_deref());
                }
                service_runtime(
                    b,
                    k.runtime.as_ref(),
                    k.runtime_span.as_deref(),
                    k.embedded_runtime.as_deref(),
                )?;
                b.a(",\"sourceInformation\":");
                src_info(b, k.source_information.as_deref());
                b.ch(b'}');
            }
            b.a("],\"func\":");
            value_spec(b, &me.query)?;
            b.a(",\"sourceInformation\":");
            src_info(b, me.source_information.as_deref());
            b.ch(b'}');
        }
    }
    Ok(())
}

/// Test-parameter values: a real `enumValue` node (span the whole dotted text) where expression position uses the
/// property form; a collection holding one rewritten one level deep.
fn param_value(b: &mut Buf, v: &ValueSpecification) -> Em {
    if let ValueSpecification::EnumValue(e) = v {
        let (ep, vp) = (e.enumeration_pos.as_deref(), e.pos.as_deref());
        b.a("{\"_type\":\"enumValue\",\"fullPath\":");
        str(b, &e.full_path);
        b.a(",\"sourceInformation\":");
        match (ep, vp) {
            (Some(ep), Some(vp)) => src_info(
                b,
                Some(&SourceInfo {
                    source_id: ep.source_id.clone(),
                    start_line: ep.start_line,
                    start_column: ep.start_column,
                    end_line: vp.end_line,
                    end_column: vp.end_column,
                }),
            ),
            _ => src_info(b, None),
        }
        b.a(",\"value\":");
        str(b, &e.value);
        b.ch(b'}');
        return Ok(());
    }
    if let ValueSpecification::PureCollection(pc) = v
        && pc.values.iter().any(|x| matches!(x, ValueSpecification::EnumValue(_)))
    {
        b.a("{\"_type\":\"collection\",\"multiplicity\":{\"lowerBound\":");
        b.int(pc.values.len() as i64);
        b.a(",\"upperBound\":");
        b.int(pc.values.len() as i64);
        b.a("},\"sourceInformation\":");
        src_info(b, pc.pos.as_deref());
        b.a(",\"values\":[");
        comma_each(b, &pc.values, param_value)?;
        b.a("]}");
        return Ok(());
    }
    value_spec(b, v)
}

/// `runtimePointer | engineRuntime`: the embedded form reuses the runtime element's array emission.
fn service_runtime(
    b: &mut Buf,
    runtime: Option<&JStr>,
    runtime_span: Option<&SourceInfo>,
    embedded: Option<&PEmbeddedRuntime>,
) -> Em {
    if let Some(rt) = runtime {
        b.a(",\"runtime\":{\"_type\":\"runtimePointer\",\"runtime\":");
        str(b, rt);
        b.a(",\"sourceInformation\":");
        src_info(b, runtime_span);
        b.ch(b'}');
    } else if let Some(embedded) = embedded {
        b.a(",\"runtime\":{\"_type\":\"engineRuntime\"");
        emitter::runtime_arrays(
            b,
            &embedded.connection_stores,
            &embedded.connections,
            &embedded.mappings,
        )?;
        b.a(",\"sourceInformation\":");
        src_info(b, embedded.source_information.as_deref());
        b.ch(b'}');
    }
    Ok(())
}

// ---------------------------------------------------------------------------------------------------------------------
// DataQuality
// ---------------------------------------------------------------------------------------------------------------------

pub fn data_quality_validation(b: &mut Buf, v: &PDataQualityValidation) -> Em {
    b.a("{\"_type\":\"dataQualityValidation\",\"context\":");
    dq_context(b, v)?;
    b.a(",\"dataQualityRootGraphFetchTree\":");
    dq_tree(b, &v.validation_tree, true)?;
    if let Some(f) = &v.filter {
        b.a(",\"filter\":");
        value_spec(b, f)?;
    }
    b.a(",\"name\":");
    str(b, &v.name);
    b.a(",\"package\":");
    str(b, &v.pkg);
    b.a(",\"sourceInformation\":");
    src_info(b, v.source_information.as_deref());
    b.a(",\"stereotypes\":");
    emitter::stereotypes(b, &v.stereotypes);
    b.a(",\"taggedValues\":");
    emitter::tagged_values(b, &v.tagged_values);
    b.ch(b'}');
    Ok(())
}

fn dq_context(b: &mut Buf, v: &PDataQualityValidation) -> Em {
    if v.context_kind == "fromMappingAndRuntime" {
        b.a("{\"_type\":\"mappingAndRuntimeDataQualityExecutionContext\",\"_type\":\"mappingAndRuntimeDataQualityExecutionContext\",\"mapping\":");
        pointer(b, &v.context_path, v.context_path_span.as_deref(), "MAPPING");
        b.a(",\"runtime\":");
        let Some(second) = &v.context_second else {
            return Err(missing_second_context_argument(v));
        };
        pointer(b, second, v.context_second_span.as_deref(), "RUNTIME");
        b.ch(b'}');
    } else if v.context_kind == "fromDataSpace" {
        b.a("{\"_type\":\"dataSpaceDataQualityExecutionContext\",\"_type\":\"dataSpaceDataQualityExecutionContext\",\"context\":");
        let Some(second) = &v.context_second else {
            return Err(missing_second_context_argument(v));
        };
        str(b, second);
        b.a(",\"dataSpace\":");
        pointer(b, &v.context_path, v.context_path_span.as_deref(), "DATASPACE");
        b.ch(b'}');
    } else {
        // the engine's grammar names the two contexts, and refuses the word, one `(` before the first argument
        let (line, column) = v.context_path_span.as_deref().map_or((0, 0), |at| {
            (
                at.start_line,
                at.start_column - crate::ustr::u(&v.context_kind.lossy()).len() as i32 - 1,
            )
        });
        return Err(JavaError::parse(jcat!("Unexpected token '", v.context_kind, "'"), line, column).as_refusal());
    }
    Ok(())
}

/// A context with one argument where the engine's grammar wants two: it refuses the `)`, just after the first (PARITY
/// row 76).
fn missing_second_context_argument(v: &PDataQualityValidation) -> JavaError {
    let (line, column) = v
        .context_path_span
        .as_deref()
        .map_or((0, 0), |at| (at.end_line, at.end_column + 1));
    JavaError::parse(JStr::from("Unexpected token ')'"), line, column).as_refusal()
}

/// `TailEmitter.pointer(b, path, span, type)`: a typed element pointer.
fn pointer(b: &mut Buf, path: &JStr, span: Option<&SourceInfo>, ty: &str) {
    b.a("{\"path\":");
    str(b, path);
    b.a(",\"sourceInformation\":");
    src_info(b, span);
    b.a(",\"type\":\"");
    b.a(ty);
    b.a("\"}");
}

fn dq_tree(b: &mut Buf, n: &PDqTreeNode, root: bool) -> Em {
    let t = if root {
        "dataQualityRootGraphFetchTree"
    } else {
        "dataQualityPropertyGraphFetchTree"
    };
    b.a("{\"_type\":\"");
    b.a(t);
    b.a("\",\"_type\":\"");
    b.a(t);
    b.ch(b'"');
    if root {
        b.a(",\"class\":");
        let Some(class) = &n.class_name else {
            return Err(npe(None));
        };
        str(b, class);
    }
    if let Some(alias) = &n.alias {
        b.a(",\"alias\":");
        str(b, alias);
    }
    b.a(",\"constraints\":[");
    strs_each(b, &n.constraints);
    b.ch(b']');
    if !root {
        b.a(",\"parameters\":[");
        comma_each(b, &n.parameters, value_spec)?;
        b.a("],\"property\":");
        let Some(property) = &n.property else {
            return Err(npe(None));
        };
        str(b, property);
    }
    b.a(",\"sourceInformation\":");
    src_info(b, n.source_information.as_deref());
    b.a(",\"subTrees\":[");
    comma_each(b, &n.sub_trees, |b, s| dq_tree(b, s, false))?;
    b.ch(b']');
    if let Some(st) = &n.sub_type {
        b.a(",\"subType\":");
        str(b, st);
    }
    b.a(",\"subTypeTrees\":[]}");
    Ok(())
}

pub fn data_quality_relation_validation(b: &mut Buf, v: &PDataQualityRelationValidation) -> Em {
    b.a("{\"_type\":\"dataqualityRelationValidation\",\"name\":");
    str(b, &v.name);
    b.a(",\"package\":");
    str(b, &v.pkg);
    b.a(",\"query\":");
    value_spec(b, &v.query)?;
    b.a(",\"sourceInformation\":");
    src_info(b, v.source_information.as_deref());
    b.a(",\"stereotypes\":");
    emitter::stereotypes(b, &v.stereotypes);
    b.a(",\"taggedValues\":");
    emitter::tagged_values(b, &v.tagged_values);
    if let Some(suites) = &v.test_suites {
        b.a(",\"testSuites\":");
        dq_test_suites(b, suites, "dataQualityRelationValidation")?;
    }
    b.a(",\"validations\":[");
    for (i, ch) in v.validations.iter().enumerate() {
        if i > 0 {
            b.ch(b',');
        }
        b.a("{\"assertion\":");
        value_spec(b, &ch.assertion)?;
        if let Some(d) = &ch.description {
            b.a(",\"description\":");
            str(b, d);
        }
        b.a(",\"name\":");
        str(b, &ch.name);
        if let Some(t) = &ch.r#type {
            b.a(",\"type\":");
            str(b, t);
        }
        b.ch(b'}');
    }
    b.a("]}");
    Ok(())
}

pub fn data_quality_relation_comparison(b: &mut Buf, v: &PDataQualityRelationComparison) -> Em {
    b.a("{\"_type\":\"dataQualityRelationComparison\",\"columnsToCompare\":[");
    strs_each(b, &v.columns_to_compare);
    b.ch(b']');
    if let Some(m) = v.expected_match {
        b.a(",\"expectedMatch\":");
        b.a(&d2s(m));
    }
    b.a(",\"keys\":[");
    strs_each(b, &v.keys);
    b.a("],\"name\":");
    str(b, &v.name);
    b.a(",\"package\":");
    str(b, &v.pkg);
    b.a(",\"source\":");
    value_spec(b, &v.source)?;
    b.a(",\"sourceInformation\":");
    src_info(b, v.source_information.as_deref());
    b.a(",\"strategy\":{\"_type\":\"");
    if v.strategy.kind != "MD5Hash" {
        return Err(illegal_state(jcat!("unmapped comparison strategy: ", v.strategy.kind)));
    }
    b.a("md5Hash");
    b.ch(b'"');
    if let Some(h) = v.strategy.aggregated_hash {
        b.a(",\"aggregatedHash\":");
        b.boolean(h);
    }
    if let Some(c) = &v.strategy.source_hash_column {
        b.a(",\"sourceHashColumn\":");
        str(b, c);
    }
    if let Some(c) = &v.strategy.target_hash_column {
        b.a(",\"targetHashColumn\":");
        str(b, c);
    }
    b.a("},\"target\":");
    value_spec(b, &v.target)?;
    if let Some(suites) = &v.test_suites {
        b.a(",\"testSuites\":");
        dq_test_suites(b, suites, "dataQualityRelationComparison")?;
    }
    b.ch(b'}');
    Ok(())
}

/// The Testable block of a relation-level DataQuality element: suites and tests carry the owner's type name prefix.
fn dq_test_suites(b: &mut Buf, suites: &[Box<PDqTestSuite>], owner: &str) -> Em {
    b.ch(b'[');
    for (i, s) in suites.iter().enumerate() {
        if i > 0 {
            b.ch(b',');
        }
        b.a("{\"_type\":\"");
        b.a(owner);
        b.a("TestSuite\",\"id\":");
        str(b, &s.id);
        b.a(",\"sourceInformation\":");
        src_info(b, s.source_information.as_deref());
        if let Some(td) = &s.test_data {
            b.a(",\"testData\":{\"sourceInformation\":");
            src_info(b, td.source_information.as_deref());
            b.a(",\"testData\":[");
            for (j, d) in td.test_data.iter().enumerate() {
                if j > 0 {
                    b.ch(b',');
                }
                b.a("{\"data\":");
                mapping_emitter::embedded_data_value(b, &d.data)?;
                b.a(",\"packageableElementPointer\":{\"path\":");
                str(b, &d.store);
                b.a(",\"sourceInformation\":");
                src_info(b, d.store_span.as_deref());
                b.a(",\"type\":\"STORE\"},\"sourceInformation\":");
                src_info(b, d.source_information.as_deref());
                b.ch(b'}');
            }
            b.a("]}");
        }
        b.a(",\"tests\":[");
        for (j, t) in s.tests.iter().enumerate() {
            if j > 0 {
                b.ch(b',');
            }
            b.a("{\"_type\":\"");
            b.a(owner);
            b.a("Test\",\"assertions\":[");
            for (k, a) in t.assertions.iter().enumerate() {
                if k > 0 {
                    b.ch(b',');
                }
                test_assertion(b, a)?;
            }
            b.a("],\"id\":");
            str(b, &t.id);
            b.a(",\"sourceInformation\":");
            src_info(b, t.source_information.as_deref());
            b.ch(b'}');
        }
        b.a("]}");
    }
    b.ch(b']');
    Ok(())
}

/// One named test assertion: the three expected-value spellings, keyed by the assertion's own id.
fn test_assertion(b: &mut Buf, a: &PTestAssertion) -> Em {
    test_assertion_expected(b, &a.expected)?;
    b.a(",\"id\":");
    str(b, &a.id);
    b.a(",\"sourceInformation\":");
    src_info(b, a.source_information.as_deref());
    b.ch(b'}');
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn persistence_types_as_java_maps_them() {
        assert_eq!(
            persistence_type(
                &crate::ustr::u("processingDimension"),
                &JStr::from("BatchIdAndDateTime"),
                false,
                false
            )
            .unwrap(),
            "batchIdAndProcessingTime"
        );
        assert_eq!(
            persistence_type(&crate::ustr::u("partitioning"), &JStr::from("FieldBased"), true, false).unwrap(),
            "fieldBasedForGraphFetch"
        );
        assert_eq!(
            persistence_type(&crate::ustr::u("auditing"), &JStr::from("DateTime"), false, true).unwrap(),
            "auditingDateTime"
        );
        let e = persistence_type(&crate::ustr::u("trigger"), &JStr::from("Hourly"), true, true).unwrap_err();
        assert_eq!(e.class, "java.lang.IllegalStateException");
        assert_eq!(
            e.message.unwrap().lossy(),
            "unprobed persistence node: trigger:Hourly (graphFetch) (target)"
        );
    }
}
