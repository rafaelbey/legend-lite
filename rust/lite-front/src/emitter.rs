// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! The protocol emitter (core/'s `com.legend.protocol.ProtocolEmitter`, ported: docs/rust-port/PHASE2.md §4 step 5):
//! the records written as the exact bytes legend-engine's object mapper writes -- `_type` first, then the fields
//! alphabetically, nulls omitted, arrays in source order, the section index last. Java is the spec: every byte, every
//! refusal (its class and message), every quirk, in Java's order (the first refusal Java meets is the answer).
//!
//! The output is a Java `String`'s UTF-16 units ([`Buf`]): [`str`] writes a value as `Json.escapeTo(b, v, true)` does,
//! every unit but `"`, `\` and the controls raw -- a lone surrogate included. An absent span is the NUL mark [`ABSENT`],
//! which [`finish`] takes out with its key and one comma, as Java's `finish` does.
//!
//! The family: this module (`ProtocolEmitter`), `tail_emitter` (`TailEmitter`), `mapping_emitter` (`MappingEmitter`),
//! `connection_emitters` (`ConnectionEmitters`, `AuthSpecEmitter`), `gql_emitter` (`GqlEmitter`); the doors' span cut is
//! `source_information` (`SourceInformation.withoutSpans`).

use crate::doors::JavaError;
use crate::jtypes::{JStr, JValue};
use crate::protocol::island_position;
use crate::records::*;
use crate::ustr::Part;
use crate::{connection_emitters, gql_emitter, jcat, mapping_emitter, tail_emitter};

/// What an emitting step answers: nothing, or the exception Java throws there.
pub type Em = Result<(), JavaError>;

/// A Java `StringBuilder`: the UTF-16 units written so far.
#[derive(Default, Clone, Debug)]
pub struct Buf {
    pub u: Vec<u16>,
}

impl Buf {
    pub fn new() -> Buf {
        Buf::default()
    }

    pub fn with_capacity(n: usize) -> Buf {
        Buf {
            u: Vec::with_capacity(n),
        }
    }

    /// `append(String)` of Rust text (the emitter's own literals).
    #[inline]
    pub fn a(&mut self, s: &str) {
        if s.is_ascii() {
            self.u.extend(s.bytes().map(u16::from));
        } else {
            self.u.extend(s.encode_utf16());
        }
    }

    /// `append(char)` of an ASCII character.
    #[inline]
    pub fn ch(&mut self, c: u8) {
        self.u.push(u16::from(c));
    }

    /// `append(int)` / `append(long)`.
    pub fn int(&mut self, v: i64) {
        let mut digits = [0u8; 20];
        let mut n = v.unsigned_abs();
        let mut i = digits.len();
        loop {
            i -= 1;
            digits[i] = b'0' + (n % 10) as u8;
            n /= 10;
            if n == 0 {
                break;
            }
        }
        if v < 0 {
            self.ch(b'-');
        }
        self.u.extend(digits[i..].iter().map(|&d| u16::from(d)));
    }

    /// `append(boolean)`.
    pub fn boolean(&mut self, v: bool) {
        self.a(if v { "true" } else { "false" });
    }

    /// `append(String)` of a Java string's units, unescaped.
    pub fn units(&mut self, u: &[u16]) {
        self.u.extend_from_slice(u);
    }

    /// `append(Object)` of a message part.
    pub fn p<P: Part + ?Sized>(&mut self, p: &P) {
        p.put(&mut self.u);
    }

    pub fn len(&self) -> usize {
        self.u.len()
    }

    pub fn is_empty(&self) -> bool {
        self.u.is_empty()
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// Java's exceptions, by class
// ---------------------------------------------------------------------------------------------------------------------

pub fn unsupported(message: JStr) -> JavaError {
    JavaError::java("java.lang.UnsupportedOperationException", message)
}

pub fn illegal_argument(message: JStr) -> JavaError {
    JavaError::java("java.lang.IllegalArgumentException", message)
}

pub fn illegal_state(message: JStr) -> JavaError {
    JavaError::java("java.lang.IllegalStateException", message)
}

/// `Objects.requireNonNull(x)` (no message) or `requireNonNull(x, message)`.
pub fn npe(message: Option<JStr>) -> JavaError {
    JavaError {
        class: "java.lang.NullPointerException".to_owned(),
        message,
        position: None,
    }
}

/// `ProtocolEmitter.require`: a construct the emitter cannot put on the wire stops it, by name.
pub fn require(emitted: bool, what: &str, where_: &[u16]) -> Em {
    if emitted {
        return Ok(());
    }
    Err(unsupported(jcat!(
        "ProtocolEmitter has no rule for ",
        what,
        " (at ",
        where_,
        "). Add the emit rule \u{2014} do not drop it."
    )))
}

/// `ProtocolEmitter.placeless`: text lite's dialect keeps that legend-engine's JSON cannot carry, refused by name.
pub fn placeless(absent: bool, what: &str, where_: &[u16]) -> Em {
    if absent {
        return Ok(());
    }
    Err(unsupported(jcat!(
        what,
        " is not written as legend-engine's JSON (at ",
        where_,
        "): its parser drops it, and lite's dialect keeps it rather than drop it here"
    )))
}

/// `ProtocolEmitter.liteOnly`: one of legend-lite's own records, refused by name.
pub(crate) fn lite_only(what: &[u16], qualified_name: &[u16]) -> JavaError {
    illegal_argument(jcat!(
        what,
        " (",
        qualified_name,
        ") is legend-lite's own dialect: legend-engine's protocol has no element for it"
    ))
}

// ---------------------------------------------------------------------------------------------------------------------
// Strings, spans and the finish
// ---------------------------------------------------------------------------------------------------------------------

/// `ProtocolEmitter.str`: a JSON string as `Json.escapeTo(b, v, true)` writes it -- `"`, `\` and the named controls
/// escaped, every other control as `\u00XX` in upper-case hex, every other unit raw.
pub fn str<S: AsRef<[u16]> + ?Sized>(b: &mut Buf, v: &S) {
    let v = v.as_ref();
    b.ch(b'"');
    let mut plain = 0;
    while plain < v.len() {
        let c = v[plain];
        if c < 0x20 || c == 0x22 || c == 0x5C {
            break;
        }
        plain += 1;
    }
    b.u.extend_from_slice(&v[..plain]);
    for &c in &v[plain..] {
        match c {
            0x22 => b.a("\\\""),
            0x5C => b.a("\\\\"),
            0x0A => b.a("\\n"),
            0x0D => b.a("\\r"),
            0x09 => b.a("\\t"),
            0x08 => b.a("\\b"),
            0x0C => b.a("\\f"),
            _ if c < 0x20 => {
                const HEX: &[u8; 16] = b"0123456789ABCDEF";
                b.a("\\u00");
                b.ch(HEX[usize::from(c >> 4)]);
                b.ch(HEX[usize::from(c & 0xF)]);
            }
            _ => b.u.push(c),
        }
    }
    b.ch(b'"');
}

/// [`str`] of Rust text (the emitter's own literals).
pub fn strs(b: &mut Buf, v: &str) {
    let units: Vec<u16> = v.encode_utf16().collect();
    str(b, &units);
}

/// The absent-span mark: a NUL, which [`str`] never writes raw.
pub const ABSENT: u16 = 0;

/// `ProtocolEmitter.srcInfo`: a span, or the [`ABSENT`] mark when there is none.
pub fn src_info(b: &mut Buf, s: Option<&SourceInfo>) {
    let Some(s) = s else {
        b.u.push(ABSENT);
        return;
    };
    b.a("{\"endColumn\":");
    b.int(i64::from(s.end_column));
    b.a(",\"endLine\":");
    b.int(i64::from(s.end_line));
    b.a(",\"sourceId\":");
    str(b, &s.source_id);
    b.a(",\"startColumn\":");
    b.int(i64::from(s.start_column));
    b.a(",\"startLine\":");
    b.int(i64::from(s.start_line));
    b.ch(b'}');
}

/// `ProtocolEmitter.finish`: the written wire with every absent span's `"key":` and one adjacent comma taken out.
pub fn finish(b: Buf) -> Vec<u16> {
    if !b.u.contains(&ABSENT) {
        return b.u;
    }
    let b = b.u;
    let mut out: Vec<u16> = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        if c != ABSENT {
            out.push(c);
            i += 1;
            continue;
        }
        // out ends with "key": -- the key's opening quote is the last quote before its closing one
        // (out.lastIndexOf("\"", out.length() - 3))
        let from = out.len() as isize - 3;
        let mut open = from.min(out.len() as isize - 1);
        while open >= 0 && out[open as usize] != u16::from(b'"') {
            open -= 1;
        }
        out.truncate(open.max(0) as usize);
        if out.last() == Some(&u16::from(b',')) {
            out.pop();
        } else if i + 1 < b.len() && b[i + 1] == u16::from(b',') {
            i += 1;
        }
        i += 1;
    }
    out
}

/// `,"<key>":<str>` when the value is there.
pub(crate) fn opt_field(b: &mut Buf, key: &str, value: Option<&JStr>) {
    if let Some(v) = value {
        b.a(",\"");
        b.a(key);
        b.a("\":");
        str(b, v);
    }
}

/// Each item written by `each`, comma-separated.
pub fn comma_each<T>(b: &mut Buf, items: &[T], mut each: impl FnMut(&mut Buf, &T) -> Em) -> Em {
    for (i, x) in items.iter().enumerate() {
        if i > 0 {
            b.ch(b',');
        }
        each(b, x)?;
    }
    Ok(())
}

/// Each string, comma-separated.
pub fn strs_each(b: &mut Buf, items: &[JStr]) {
    for (i, x) in items.iter().enumerate() {
        if i > 0 {
            b.ch(b',');
        }
        str(b, x);
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// The entry points
// ---------------------------------------------------------------------------------------------------------------------

/// `ProtocolEmitter.emit(pmcd)`: a whole model's JSON.
pub fn emit(pmcd: &PureModelContextData) -> Result<Vec<u16>, JavaError> {
    let mut b = Buf::with_capacity(1024);
    b.a("{\"_type\":\"data\",\"elements\":[");
    comma_each(&mut b, &pmcd.elements, element)?;
    b.ch(b']');
    if let Some(origin) = &pmcd.origin {
        b.a(",\"origin\":{\"_type\":\"pointer\",\"sdlcInfo\":");
        sdlc(&mut b, &origin.sdlc_info);
        if let Some(s) = &origin.serializer {
            b.a(",\"serializer\":");
            serializer(&mut b, s);
        }
        b.ch(b'}');
    }
    if let Some(s) = &pmcd.serializer {
        b.a(",\"serializer\":");
        serializer(&mut b, s);
    }
    b.ch(b'}');
    Ok(finish(b))
}

/// `{"name":..,"version":..}`, a null field left out.
pub(crate) fn serializer(b: &mut Buf, s: &PSerializer) {
    b.ch(b'{');
    let mut first = true;
    if let Some(n) = &s.name {
        b.a("\"name\":");
        str(b, n);
        first = false;
    }
    if let Some(v) = &s.version {
        b.a(if first { "" } else { "," });
        b.a("\"version\":");
        str(b, v);
    }
    b.ch(b'}');
}

/// An SDLC's fields in the engine's (alphabetical) order, a null one left out.
pub(crate) fn sdlc(b: &mut Buf, s: &PSdlc) {
    b.a("{\"_type\":");
    str(b, &s.kind);
    opt_field(b, "artifactId", s.artifact_id.as_ref());
    opt_field(b, "baseVersion", s.base_version.as_ref());
    opt_field(b, "groupId", s.group_id.as_ref());
    if s.kind == "workspace" {
        b.a(",\"isGroupWorkspace\":");
        b.boolean(s.is_group_workspace);
    }
    opt_field(b, "overrideUrl", s.override_url.as_ref());
    b.a(",\"packageableElementPointers\":[");
    for (i, p) in s.packageable_element_pointers.iter().enumerate() {
        if i > 0 {
            b.ch(b',');
        }
        pointer(b, p);
    }
    b.ch(b']');
    opt_field(b, "project", s.project.as_ref());
    b.a(",\"version\":");
    str(b, &s.version);
    b.ch(b'}');
}

/// `ProtocolEmitter.emitElement(e)`: one element's JSON.
pub fn emit_element(e: &Element) -> Result<Vec<u16>, JavaError> {
    let mut b = Buf::with_capacity(512);
    element(&mut b, e)?;
    Ok(finish(b))
}

/// `ProtocolEmitter.emitLambda(lam)`: a lambda as upstream `grammarToJson/lambda` writes it.
pub fn emit_lambda(lam: &LambdaFunction) -> Result<Vec<u16>, JavaError> {
    let mut b = Buf::new();
    lambda(&mut b, lam)?;
    Ok(finish(b))
}

/// Exhaustive over [`Element`]: `ProtocolEmitter.element`.
pub(crate) fn element(b: &mut Buf, e: &Element) -> Em {
    match e {
        Element::PClass(c) => pclass(b, c),
        Element::PMeasure(m) => measure(b, m),
        Element::PAssociation(a) => association(b, a),
        Element::PFunction(f) => function(b, f),
        Element::PEnumeration(en) => {
            enumeration(b, en);
            Ok(())
        }
        Element::PProfile(p) => {
            profile(b, p);
            Ok(())
        }
        Element::PSectionIndex(s) => {
            section_index(b, s);
            Ok(())
        }
        Element::PRuntime(r) => runtime(b, r),
        Element::PConnection(c) => connection(b, c),
        Element::PDatabase(d) => database(b, d),
        Element::PService(sv) => tail_emitter::service(b, sv),
        Element::PExecutionEnvironment(ee) => tail_emitter::execution_environment(b, ee),
        Element::PDataSpace(ds) => tail_emitter::data_space(b, ds),
        Element::PPersistence(pp) => tail_emitter::persistence(b, pp),
        Element::PPersistenceContext(pc) => tail_emitter::persistence_context(b, pc),
        Element::PFunctionActivator(fa) => function_activator(b, fa),
        Element::PText(t) => tail_emitter::text(b, t),
        Element::PGenerationSpecification(gs) => tail_emitter::generation_specification(b, gs),
        Element::PFileGeneration(fg) => tail_emitter::file_generation(b, fg),
        Element::PDeephavenDatabase(dh) => tail_emitter::deephaven_store(b, dh),
        Element::PElasticsearch7Cluster(es) => tail_emitter::elasticsearch_store(b, es),
        Element::PMongoDatabase(mg) => tail_emitter::mongo_database(b, mg),
        Element::PDataQualityValidation(dq) => tail_emitter::data_quality_validation(b, dq),
        Element::PDataQualityRelationValidation(dq) => tail_emitter::data_quality_relation_validation(b, dq),
        Element::PDataQualityRelationComparison(dq) => tail_emitter::data_quality_relation_comparison(b, dq),
        Element::PSchemaSet(ss) => tail_emitter::schema_set(b, ss),
        Element::PBinding(bd) => tail_emitter::binding(b, bd),
        Element::PServiceStoreDefinition(sd) => tail_emitter::service_store(b, sd),
        Element::PDiagram(dg) => tail_emitter::diagram(b, dg),
        Element::PMapping(m) => mapping_emitter::mapping(b, m),
        Element::PRelationalMapper(rm) => {
            relational_mapper(b, rm);
            Ok(())
        }
        Element::PNativeFunction(n) => Err(lite_only(
            &crate::ustr::u("a native function declaration"),
            &n.qualified_name.0,
        )),
        Element::PPrimitive(p) => Err(lite_only(
            &crate::ustr::u("a Primitive declaration"),
            &p.qualified_name.0,
        )),
        Element::POpaqueElement(o) => Err(lite_only(
            &jcat!("an element of the foreign section '", o.section_name, "'").0,
            &o.qualified_name.0,
        )),
        Element::PAssociationProjection(a) => Err(lite_only(
            &crate::ustr::u("a projection association"),
            &a.qualified_name.0,
        )),
        Element::PDataElement(de) => {
            b.a("{\"_type\":\"dataElement\"");
            if let Some(v) = &de.body.value {
                b.a(",\"data\":");
                mapping_emitter::embedded_data_value(b, v)?;
            }
            if !de.body.resolvers.is_empty() {
                b.a(",\"dataResolvers\":[");
                comma_each(b, &de.body.resolvers, |b, r| data_resolver(b, r))?;
                b.ch(b']');
            }
            b.a(",\"name\":");
            str(b, &de.name);
            b.a(",\"package\":");
            str(b, &de.pkg);
            b.a(",\"sourceInformation\":");
            src_info(b, de.source_information.as_deref());
            b.a(",\"stereotypes\":");
            stereotypes(b, &de.stereotypes);
            b.a(",\"taggedValues\":");
            tagged_values(b, &de.tagged_values);
            b.ch(b'}');
            Ok(())
        }
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// Runtimes, connections, relational mappers, data resolvers, pointers
// ---------------------------------------------------------------------------------------------------------------------

/// `_type:"runtime"`: the element and its runtimeValue share one span.
pub(crate) fn runtime(b: &mut Buf, r: &PRuntime) -> Em {
    b.a("{\"_type\":\"runtime\",\"name\":");
    str(b, &r.name);
    b.a(",\"package\":");
    str(b, &r.pkg);
    b.a(",\"runtimeValue\":{\"_type\":");
    strs(
        b,
        if r.single {
            "localEngineRuntime"
        } else {
            "engineRuntime"
        },
    );
    runtime_arrays(b, &r.connection_stores, &r.connections, &r.mappings)?;
    b.a(",\"sourceInformation\":");
    src_info(b, r.source_information.as_deref());
    b.a("},\"sourceInformation\":");
    src_info(b, r.source_information.as_deref());
    b.ch(b'}');
    Ok(())
}

/// The engineRuntime/localEngineRuntime arrays: `,"connectionStores":[...],"connections":[...],"mappings":[...]`.
pub fn runtime_arrays(
    b: &mut Buf,
    connection_stores: &[Box<PConnectionStores>],
    connections: &[Box<PStoreConnections>],
    mappings: &[Box<PPointer>],
) -> Em {
    b.a(",\"connectionStores\":[");
    for (i, cs) in connection_stores.iter().enumerate() {
        if i > 0 {
            b.ch(b',');
        }
        b.a("{\"connectionPointer\":");
        connection_value(b, &cs.connection_pointer)?;
        b.a(",\"sourceInformation\":");
        src_info(b, cs.source_information.as_deref());
        b.a(",\"storePointers\":[");
        for (j, sp) in cs.store_pointers.iter().enumerate() {
            if j > 0 {
                b.ch(b',');
            }
            b.a("{\"path\":");
            str(b, &sp.path);
            b.a(",\"sourceInformation\":");
            src_info(b, sp.source_information.as_deref());
            if let Some(t) = &sp.r#type {
                b.a(",\"type\":");
                str(b, t);
            }
            b.ch(b'}');
        }
        b.a("]}");
    }
    b.a("],\"connections\":[");
    comma_each(b, connections, |b, sc| store_connections(b, sc))?;
    b.a("],\"mappings\":[");
    for (i, m) in mappings.iter().enumerate() {
        if i > 0 {
            b.ch(b',');
        }
        pointer(b, m);
    }
    b.ch(b']');
    Ok(())
}

pub(crate) fn store_connections(b: &mut Buf, sc: &PStoreConnections) -> Em {
    b.a("{\"sourceInformation\":");
    src_info(b, sc.source_information.as_deref());
    b.a(",\"store\":");
    pointer(b, &sc.store);
    b.a(",\"storeConnections\":[");
    for (i, ic) in sc.store_connections.iter().enumerate() {
        if i > 0 {
            b.ch(b',');
        }
        b.a("{\"connection\":");
        connection_value(b, &ic.connection)?;
        b.a(",\"id\":");
        str(b, &ic.id);
        b.a(",\"sourceInformation\":");
        src_info(b, ic.source_information.as_deref());
        b.ch(b'}');
    }
    b.a("]}");
    Ok(())
}

/// `_type:"relationalMapper"`.
pub(crate) fn relational_mapper(b: &mut Buf, rm: &PRelationalMapper) {
    b.a("{\"_type\":\"relationalMapper\",\"databaseMappers\":[");
    for (i, dm) in rm.database_mappers.iter().enumerate() {
        if i > 0 {
            b.ch(b',');
        }
        b.a("{\"databaseName\":");
        str(b, &dm.database_name);
        b.a(",\"schemas\":[");
        for (j, s) in dm.schemas.iter().enumerate() {
            if j > 0 {
                b.ch(b',');
            }
            schema_pointer(b, s);
        }
        b.a("]}");
    }
    b.a("],\"name\":");
    str(b, &rm.name);
    b.a(",\"package\":");
    str(b, &rm.pkg);
    b.a(",\"schemaMappers\":[");
    for (i, sm) in rm.schema_mappers.iter().enumerate() {
        if i > 0 {
            b.ch(b',');
        }
        b.a("{\"from\":");
        schema_pointer(b, &sm.from);
        b.a(",\"to\":");
        str(b, &sm.to);
        b.ch(b'}');
    }
    b.a("],\"sourceInformation\":");
    src_info(b, rm.source_information.as_deref());
    b.a(",\"tableMappers\":[");
    for (i, tm) in rm.table_mappers.iter().enumerate() {
        if i > 0 {
            b.ch(b',');
        }
        let tp = &tm.from;
        b.a("{\"from\":{\"_type\":\"Table\",\"database\":");
        str(b, &tp.database);
        b.a(",\"schema\":");
        str(b, &tp.schema);
        b.a(",\"sourceInformation\":");
        src_info(b, tp.source_information.as_deref());
        b.a(",\"table\":");
        str(b, &tp.table);
        b.a("},\"to\":");
        str(b, &tm.to);
        b.ch(b'}');
    }
    b.a("]}");
}

/// `_type:"Schema"` pointer.
pub(crate) fn schema_pointer(b: &mut Buf, sp: &PSchemaPointer) {
    b.a("{\"_type\":\"Schema\",\"database\":");
    str(b, &sp.database);
    b.a(",\"schema\":");
    str(b, &sp.schema);
    b.a(",\"sourceInformation\":");
    src_info(b, sp.source_information.as_deref());
    b.ch(b'}');
}

/// One `store::S: <value>;` entry of a store-keyed ###Data element.
pub(crate) fn data_resolver(b: &mut Buf, r: &PDataResolver) -> Em {
    let Some(data) = &r.data else {
        b.a("{\"_type\":\"referenceDataResolver\"");
        b.a(",\"elementPointer\":{\"path\":");
        str(b, &r.element_pointer.path);
        b.a(",\"sourceInformation\":");
        src_info(b, r.element_pointer.source_information.as_deref());
        b.a("},\"sourceInformation\":");
        src_info(b, r.source_information.as_deref());
        b.ch(b'}');
        return Ok(());
    };
    b.a("{\"_type\":\"baseDataResolver\",\"data\":");
    mapping_emitter::embedded_data_value(b, data)?;
    b.a(",\"elementPointer\":{\"path\":");
    str(b, &r.element_pointer.path);
    b.a(",\"sourceInformation\":");
    src_info(b, r.element_pointer.source_information.as_deref());
    b.a("},\"sourceInformation\":");
    src_info(b, r.source_information.as_deref());
    b.ch(b'}');
    Ok(())
}

pub fn pointer(b: &mut Buf, p: &PPointer) {
    b.a("{\"path\":");
    str(b, &p.path);
    b.a(",\"sourceInformation\":");
    src_info(b, p.source_information.as_deref());
    b.a(",\"type\":");
    str(b, &p.r#type);
    b.ch(b'}');
}

pub fn join_ptr(b: &mut Buf, jp: &PJoinPtr) {
    b.ch(b'{');
    if let Some(db) = &jp.db {
        b.a("\"db\":");
        str(b, db);
        b.ch(b',');
    }
    if let Some(t) = &jp.join_type {
        b.a("\"joinType\":");
        str(b, t);
        b.ch(b',');
    }
    b.a("\"name\":");
    str(b, &jp.name);
    b.a(",\"sourceInformation\":");
    src_info(b, jp.source_information.as_deref());
    b.ch(b'}');
}

pub fn table_ptr(b: &mut Buf, t: &PTablePtr) {
    b.a("{\"_type\":");
    match &t.written_type {
        Some(w) => str(b, w),
        None => strs(b, "Table"),
    }
    opt_field(b, "database", t.database.as_ref());
    opt_field(b, "mainTableDb", t.main_table_db.as_ref());
    b.a(",\"schema\":");
    str(b, &t.schema);
    b.a(",\"sourceInformation\":");
    src_info(b, t.source_information.as_deref());
    b.a(",\"table\":");
    str(b, &t.table);
    b.ch(b'}');
}

// ---------------------------------------------------------------------------------------------------------------------
// Databases and the relational operation
// ---------------------------------------------------------------------------------------------------------------------

/// `_type:"relational"`: a Database element.
pub(crate) fn database(b: &mut Buf, d: &PDatabase) -> Em {
    b.a("{\"_type\":\"relational\",\"filters\":[");
    for (i, f) in d.filters.iter().enumerate() {
        if i > 0 {
            b.ch(b',');
        }
        b.a("{\"_type\":\"");
        b.units(&f.filter_type.0);
        b.a("\",\"name\":");
        str(b, &f.name);
        b.a(",\"operation\":");
        rel_op(b, &f.operation, true);
        b.a(",\"sourceInformation\":");
        src_info(b, f.source_information.as_deref());
        b.ch(b'}');
    }
    b.ch(b']');
    if !d.included_store_specifications.is_empty() {
        b.a(",\"includedStoreSpecifications\":[");
        for (i, sp) in d.included_store_specifications.iter().enumerate() {
            if i > 0 {
                b.ch(b',');
            }
            b.a("{\"packageableElementPointer\":{\"path\":");
            str(b, &sp.path);
            b.a(",\"sourceInformation\":");
            src_info(b, sp.source_information.as_deref());
            b.a("},\"sourceInformation\":");
            src_info(b, sp.source_information.as_deref());
            b.a(",\"storeType\":");
            str(b, &sp.store_type);
            b.ch(b'}');
        }
        b.ch(b']');
    }
    b.a(",\"includedStores\":[");
    for (i, p) in d.included_stores.iter().enumerate() {
        if i > 0 {
            b.ch(b',');
        }
        pointer(b, p);
    }
    b.a("],\"joins\":[");
    for (i, j) in d.joins.iter().enumerate() {
        if i > 0 {
            b.ch(b',');
        }
        b.a("{\"name\":");
        str(b, &j.name);
        b.a(",\"operation\":");
        rel_op(b, &j.operation, true);
        b.a(",\"sourceInformation\":");
        src_info(b, j.source_information.as_deref());
        b.ch(b'}');
    }
    b.a("],\"name\":");
    str(b, &d.name);
    b.a(",\"package\":");
    str(b, &d.pkg);
    b.a(",\"schemas\":[");
    for (i, s) in d.schemas.iter().enumerate() {
        if i > 0 {
            b.ch(b',');
        }
        db_schema(b, s);
    }
    b.a("],\"sourceInformation\":");
    src_info(b, d.source_information.as_deref());
    b.a(",\"stereotypes\":");
    stereotypes(b, &d.stereotypes);
    if !d.tagged_values.is_empty() {
        b.a(",\"taggedValues\":");
        tagged_values(b, &d.tagged_values);
    }
    b.ch(b'}');
    Ok(())
}

pub(crate) fn db_schema(b: &mut Buf, sc: &PDbSchema) {
    b.a("{\"name\":");
    str(b, &sc.name);
    b.a(",\"sourceInformation\":");
    src_info(b, sc.source_information.as_deref());
    if !sc.stereotypes.is_empty() {
        b.a(",\"stereotypes\":");
        stereotypes(b, &sc.stereotypes);
    }
    b.a(",\"tables\":[");
    for (i, t) in sc.tables.iter().enumerate() {
        if i > 0 {
            b.ch(b',');
        }
        db_table(b, t);
    }
    b.a("],\"tabularFunctions\":[");
    for (i, tf) in sc.tabular_functions.iter().enumerate() {
        if i > 0 {
            b.ch(b',');
        }
        b.ch(b'{');
        db_columns(b, &tf.columns);
        b.a(",\"name\":");
        str(b, &tf.name);
        b.a(",\"sourceInformation\":");
        src_info(b, tf.source_information.as_deref());
        b.ch(b'}');
    }
    b.ch(b']');
    if !sc.tagged_values.is_empty() {
        b.a(",\"taggedValues\":");
        tagged_values(b, &sc.tagged_values);
    }
    b.a(",\"views\":[");
    for (i, v) in sc.views.iter().enumerate() {
        if i > 0 {
            b.ch(b',');
        }
        db_view(b, v);
    }
    b.a("]}");
}

pub(crate) fn db_table(b: &mut Buf, t: &PDbTable) {
    b.ch(b'{');
    db_columns(b, &t.columns);
    b.a(",\"milestoning\":[");
    for (i, m) in t.milestoning.iter().enumerate() {
        if i > 0 {
            b.ch(b',');
        }
        milestoning(b, m);
    }
    b.a("],\"name\":");
    str(b, &t.name);
    b.a(",\"primaryKey\":[");
    strs_each(b, &t.primary_key);
    b.a("],\"sourceInformation\":");
    src_info(b, t.source_information.as_deref());
    if !t.stereotypes.is_empty() {
        b.a(",\"stereotypes\":");
        stereotypes(b, &t.stereotypes);
    }
    if !t.tagged_values.is_empty() {
        b.a(",\"taggedValues\":");
        tagged_values(b, &t.tagged_values);
    }
    b.ch(b'}');
}

pub(crate) fn db_columns(b: &mut Buf, cols: &[Box<PDbColumn>]) {
    b.a("\"columns\":[");
    for (i, c) in cols.iter().enumerate() {
        if i > 0 {
            b.ch(b',');
        }
        b.a("{\"name\":");
        str(b, &c.name);
        b.a(",\"nullable\":");
        b.boolean(c.nullable);
        b.a(",\"sourceInformation\":");
        src_info(b, c.source_information.as_deref());
        if !c.stereotypes.is_empty() {
            b.a(",\"stereotypes\":");
            stereotypes(b, &c.stereotypes);
        }
        if !c.tagged_values.is_empty() {
            b.a(",\"taggedValues\":");
            tagged_values(b, &c.tagged_values);
        }
        b.a(",\"type\":{\"_type\":\"");
        b.units(&c.r#type.kind.0);
        b.ch(b'"');
        if let Some(p) = c.r#type.precision {
            b.a(",\"precision\":");
            b.int(p);
        }
        if let Some(s) = c.r#type.scale {
            b.a(",\"scale\":");
            b.int(s);
        }
        if let Some(s) = c.r#type.size {
            b.a(",\"size\":");
            b.int(s);
        }
        b.a("}}");
    }
    b.ch(b']');
}

pub(crate) fn milestoning(b: &mut Buf, m: &PMilestoning) {
    match m {
        PMilestoning::PBusinessMilestoning(bm) => {
            b.a("{\"_type\":\"businessMilestoning\",\"from\":");
            str(b, &bm.from);
            if let Some(d) = &bm.infinity_date {
                b.a(",\"infinityDate\":");
                date_time_lit(b, d);
            }
            b.a(",\"sourceInformation\":");
            src_info(b, bm.source_information.as_deref());
            b.a(",\"thru\":");
            str(b, &bm.thru);
            b.a(",\"thruIsInclusive\":");
            b.boolean(bm.thru_is_inclusive);
            b.ch(b'}');
        }
        PMilestoning::PProcessingSnapshotMilestoning(psm) => {
            b.a("{\"_type\":\"processingSnapshotMilestoning\",\"snapshotDate\":");
            str(b, &psm.snapshot_date);
            b.a(",\"sourceInformation\":");
            src_info(b, psm.source_information.as_deref());
            b.ch(b'}');
        }
        PMilestoning::PBusinessSnapshotMilestoning(sm) => {
            b.a("{\"_type\":\"businessSnapshotMilestoning\",\"snapshotDate\":");
            str(b, &sm.snapshot_date);
            b.a(",\"sourceInformation\":");
            src_info(b, sm.source_information.as_deref());
            b.ch(b'}');
        }
        PMilestoning::PProcessingMilestoning(pm) => {
            b.a("{\"_type\":\"processingMilestoning\",\"in\":");
            str(b, &pm.r#in);
            if let Some(d) = &pm.infinity_date {
                b.a(",\"infinityDate\":");
                date_time_lit(b, d);
            }
            b.a(",\"out\":");
            str(b, &pm.out);
            b.a(",\"outIsInclusive\":");
            b.boolean(pm.out_is_inclusive);
            b.a(",\"sourceInformation\":");
            src_info(b, pm.source_information.as_deref());
            b.ch(b'}');
        }
    }
}

/// `PDateTimeLit.wireType()`: a value whose text holds a `T` is a dateTime, else a strictDate.
pub(crate) fn date_time_lit(b: &mut Buf, d: &PDateTimeLit) {
    b.a("{\"_type\":\"");
    b.a(date_time_wire_type(&d.value.0));
    b.a("\",\"sourceInformation\":");
    src_info(b, d.source_information.as_deref());
    b.a(",\"value\":");
    str(b, &d.value);
    b.ch(b'}');
}

pub(crate) fn db_view(b: &mut Buf, v: &PDbView) {
    b.a("{\"columnMappings\":[");
    for (i, cm) in v.column_mappings.iter().enumerate() {
        if i > 0 {
            b.ch(b',');
        }
        b.a("{\"name\":");
        str(b, &cm.name);
        b.a(",\"operation\":");
        rel_op(b, &cm.operation, true);
        b.a(",\"sourceInformation\":");
        src_info(b, cm.source_information.as_deref());
        b.ch(b'}');
    }
    b.a("],\"distinct\":");
    b.boolean(v.distinct);
    if let Some(f) = &v.filter {
        b.a(",\"filter\":{\"filter\":{");
        if let Some(db) = &f.db {
            b.a("\"db\":");
            str(b, db);
            b.ch(b',');
        }
        b.a("\"name\":");
        str(b, &f.name);
        b.a("},\"joins\":[");
        for (i, j) in f.joins.iter().enumerate() {
            if i > 0 {
                b.ch(b',');
            }
            join_ptr(b, j);
        }
        b.a("],\"sourceInformation\":");
        src_info(b, f.source_information.as_deref());
        b.ch(b'}');
    }
    b.a(",\"groupBy\":[");
    for (i, g) in v.group_by.iter().enumerate() {
        if i > 0 {
            b.ch(b',');
        }
        rel_op(b, g, true);
    }
    b.ch(b']');
    b.a(",\"name\":");
    str(b, &v.name);
    b.a(",\"primaryKey\":[");
    strs_each(b, &v.primary_key);
    b.a("],\"sourceInformation\":");
    src_info(b, v.source_information.as_deref());
    if !v.stereotypes.is_empty() {
        b.a(",\"stereotypes\":");
        stereotypes(b, &v.stereotypes);
    }
    if !v.tagged_values.is_empty() {
        b.a(",\"taggedValues\":");
        tagged_values(b, &v.tagged_values);
    }
    b.ch(b'}');
}

/// Opens an operation object: `{"_type":"<tag>",` when typed, a bare `{` when the element rides an `Object` field.
pub(crate) fn open(b: &mut Buf, typed: bool, tag: &str) {
    b.ch(b'{');
    if typed {
        b.a("\"_type\":\"");
        b.a(tag);
        b.a("\",");
    }
}

/// `ProtocolEmitter.relOp(b, op, typed)`.
pub fn rel_op(b: &mut Buf, op: &PRelOp, typed: bool) {
    match op {
        PRelOp::PRelLambda(l) => {
            open(b, typed, "relationalLambda");
            b.a("\"body\":");
            rel_op(b, &l.body, true);
            b.a(",\"parameterNames\":[");
            strs_each(b, &l.parameter_names);
            b.a("],\"sourceInformation\":");
            src_info(b, l.source_information.as_deref());
            b.ch(b'}');
        }
        PRelOp::PLambdaParam(p) => {
            open(b, typed, "lambdaParameter");
            b.a("\"name\":");
            str(b, &p.name);
            b.a(",\"sourceInformation\":");
            src_info(b, p.source_information.as_deref());
            b.ch(b'}');
        }
        PRelOp::PDynaFunc(f) => {
            open(b, typed, "dynaFunc");
            b.a("\"funcName\":");
            str(b, &f.func_name);
            b.a(",\"parameters\":[");
            for (i, p) in f.parameters.iter().enumerate() {
                if i > 0 {
                    b.ch(b',');
                }
                rel_op(b, p, true);
            }
            b.a("],\"sourceInformation\":");
            src_info(b, f.source_information.as_deref());
            b.ch(b'}');
        }
        PRelOp::PColumnRef(c) => {
            open(b, typed, "column");
            b.a("\"column\":");
            str(b, &c.column);
            b.a(",\"sourceInformation\":");
            src_info(b, c.source_information.as_deref());
            b.a(",\"table\":");
            table_ptr(b, &c.table);
            b.a(",\"tableAlias\":");
            str(b, &c.table_alias);
            b.ch(b'}');
        }
        PRelOp::PElemtWithJoins(ej) => {
            open(b, typed, "elemtWithJoins");
            b.a("\"joins\":[");
            for (i, j) in ej.joins.iter().enumerate() {
                if i > 0 {
                    b.ch(b',');
                }
                join_ptr(b, j);
            }
            b.ch(b']');
            if let Some(re) = &ej.relational_element {
                b.a(",\"relationalElement\":");
                rel_op(b, re, true);
            }
            b.a(",\"sourceInformation\":");
            src_info(b, ej.source_information.as_deref());
            b.ch(b'}');
        }
        PRelOp::PRelLiteralList(ll) => {
            open(b, typed, "literalList");
            b.a("\"sourceInformation\":");
            src_info(b, ll.source_information.as_deref());
            b.a(",\"values\":[");
            for (i, v) in ll.values.iter().enumerate() {
                if i > 0 {
                    b.ch(b',');
                }
                // each element rides a Literal wrapper whose value is an Object field: the element emits untyped
                b.a("{\"_type\":\"literal\",\"value\":");
                rel_op(b, v, false);
                b.ch(b'}');
            }
            b.a("]}");
        }
        PRelOp::PRelLiteral(l) => {
            open(b, typed, "literal");
            b.a("\"sourceInformation\":");
            src_info(b, l.source_information.as_deref());
            b.a(",\"value\":");
            match &l.value {
                JValue::String(sv) => str(b, sv),
                other => append_value(b, other),
            }
            b.ch(b'}');
        }
    }
}

/// `StringBuilder.append(Object)` of a number or boolean leaf: its `toString`.
pub fn append_value(b: &mut Buf, v: &JValue) {
    match v {
        JValue::String(s) => b.units(&s.0),
        JValue::Long(n) => b.int(*n),
        JValue::Integer(n) => b.int(i64::from(*n)),
        JValue::Boolean(x) => b.boolean(*x),
        JValue::Double(d) => b.a(&lite_javacompat::double_to_string(*d)),
        JValue::BigInteger(digits) => b.a(digits),
        JValue::BigDecimal(d) => b.a(&d.to_java_string()),
    }
}

/// `PDateTimeLit.wireType`.
pub(crate) fn date_time_wire_type(value: &[u16]) -> &'static str {
    if value.contains(&u16::from(b'T')) {
        "dateTime"
    } else {
        "strictDate"
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// Connections
// ---------------------------------------------------------------------------------------------------------------------

/// `_type:"connection"`: the element and its value share one span.
pub(crate) fn connection(b: &mut Buf, c: &PConnection) -> Em {
    b.a("{\"_type\":\"connection\",\"connectionValue\":");
    connection_value(b, &c.value)?;
    b.a(",\"name\":");
    str(b, &c.name);
    b.a(",\"package\":");
    str(b, &c.pkg);
    b.a(",\"sourceInformation\":");
    src_info(b, c.source_information.as_deref());
    b.ch(b'}');
    Ok(())
}

/// `ProtocolEmitter.connectionValue`.
pub fn connection_value(b: &mut Buf, v: &PConnectionValue) -> Em {
    match v {
        PConnectionValue::PConnectionPointer(cp) => {
            b.a("{\"_type\":\"connectionPointer\",\"connection\":");
            str(b, &cp.connection);
            b.a(",\"sourceInformation\":");
            src_info(b, cp.source_information.as_deref());
            b.ch(b'}');
        }
        PConnectionValue::PJsonModelConnection(jc) => model_connection(
            b,
            "JsonModelConnection",
            &jc.class_name,
            jc.class_source_information.as_deref(),
            jc.element.as_ref(),
            &jc.url,
            jc.source_information.as_deref(),
        ),
        PConnectionValue::PXmlModelConnection(xc) => model_connection(
            b,
            "XmlModelConnection",
            &xc.class_name,
            xc.class_source_information.as_deref(),
            xc.element.as_ref(),
            &xc.url,
            xc.source_information.as_deref(),
        ),
        PConnectionValue::PModelChainConnection(mc) => {
            b.a("{\"_type\":\"ModelChainConnection\"");
            if let Some(e) = &mc.element {
                b.a(",\"element\":");
                str(b, e);
            }
            b.a(",\"mappings\":[");
            strs_each(b, &mc.mappings);
            b.a("],\"mappingsSourceInformation\":");
            src_info(b, mc.mappings_source_information.as_deref());
            b.a(",\"sourceInformation\":");
            src_info(b, mc.source_information.as_deref());
            b.ch(b'}');
        }
        PConnectionValue::PServiceStoreConnection(sc) => {
            b.a("{\"_type\":\"serviceStore\",\"baseUrl\":");
            str(b, &sc.base_url);
            if let Some(e) = &sc.element {
                b.a(",\"element\":");
                str(b, e);
                b.a(",\"elementSourceInformation\":");
                src_info(b, sc.element_source_information.as_deref());
            }
            b.a(",\"sourceInformation\":");
            src_info(b, sc.source_information.as_deref());
            b.ch(b'}');
        }
        PConnectionValue::PDeephavenConnection(dc) => {
            b.a("{\"_type\":\"deephavenConnection\",\"authSpec\":{\"_type\":\"PSK\",\"psk\":");
            str(b, &dc.psk);
            b.ch(b'}');
            if let Some(e) = &dc.element {
                b.a(",\"element\":");
                str(b, e);
                b.a(",\"elementSourceInformation\":");
                src_info(b, dc.element_source_information.as_deref());
            }
            b.a(",\"sourceInformation\":");
            src_info(b, dc.source_information.as_deref());
            b.a(",\"sourceSpec\":{\"url\":");
            str(b, &dc.server_url);
            b.a("}}");
        }
        PConnectionValue::PElasticsearchConnection(ec) => elasticsearch_connection(b, ec)?,
        PConnectionValue::PMongoDbConnection(mc2) => {
            b.a("{\"_type\":\"MongoDBConnection\",\"authenticationSpecification\":");
            connection_emitters::es_auth_spec(b, &mc2.auth)?;
            b.a(",\"dataSourceSpecification\":{\"databaseName\":");
            str(b, &mc2.database_name);
            b.a(",\"serverURLs\":[");
            for (i, s) in mc2.server_urls.iter().enumerate() {
                if i > 0 {
                    b.ch(b',');
                }
                b.a("{\"baseUrl\":");
                str(b, &s.base_url);
                b.a(",\"port\":");
                b.int(s.port);
                b.ch(b'}');
            }
            b.a("]}");
            if let Some(e) = &mc2.element {
                b.a(",\"element\":");
                str(b, e);
                b.a(",\"elementSourceInformation\":");
                src_info(b, mc2.element_source_information.as_deref());
            }
            b.a(",\"sourceInformation\":");
            src_info(b, mc2.source_information.as_deref());
            b.a(",\"type\":\"MongoDb\"}");
        }
        PConnectionValue::PRelationalDatabaseConnection(rc) => relational_connection(b, rc)?,
    }
    Ok(())
}

pub(crate) fn relational_connection(b: &mut Buf, rc: &PRelationalDatabaseConnection) -> Em {
    b.a("{\"_type\":\"RelationalDatabaseConnection\",\"authenticationStrategy\":");
    connection_emitters::auth_strategy(b, &rc.authentication_strategy)?;
    b.a(",\"databaseType\":");
    str(b, &rc.database_type);
    b.a(",\"datasourceSpecification\":");
    connection_emitters::datasource_spec(b, &rc.datasource_specification)?;
    if let Some(e) = &rc.element {
        b.a(",\"element\":");
        str(b, e);
        b.a(",\"elementSourceInformation\":");
        src_info(b, rc.element_source_information.as_deref());
    }
    if let Some(l) = rc.local_mode {
        b.a(",\"localMode\":");
        b.boolean(l);
    }
    b.a(",\"postProcessorWithParameter\":[]");
    if !rc.post_processors.is_empty() {
        b.a(",\"postProcessors\":[");
        for (i, pp) in rc.post_processors.iter().enumerate() {
            if i > 0 {
                b.ch(b',');
            }
            match pp {
                PPostProcessor::PMapperPostProcessor(mp) => {
                    b.a("{\"_type\":\"mapper\",\"mappers\":[");
                    comma_each(b, &mp.mappers, connection_emitters::mapper)?;
                    b.a("]}");
                }
                PPostProcessor::PRelationalMapperPostProcessor(rp) => {
                    b.a("{\"_type\":\"relationalMapper\",\"relationalMappers\":[");
                    for (j, p) in rp.relational_mappers.iter().enumerate() {
                        if j > 0 {
                            b.ch(b',');
                        }
                        pointer(b, p);
                    }
                    b.a("]}");
                }
                PPostProcessor::PExtractSubQueriesAsCtesPostProcessor(_) => {
                    b.a("{\"_type\":\"ExtractSubQueriesAsCTEsPostProcessor\"}");
                }
            }
        }
        b.ch(b']');
    }
    if let Some(qgs) = &rc.query_generation_configs {
        b.a(",\"queryGenerationConfigs\":[");
        for (i, g) in qgs.iter().enumerate() {
            if i > 0 {
                b.ch(b',');
            }
            b.a("{\"_type\":\"generationFeaturesConfig\",\"disabled\":[");
            strs_each(b, &g.disabled);
            b.a("],\"enabled\":[");
            strs_each(b, &g.enabled);
            b.a("],\"sourceInformation\":");
            src_info(b, g.source_information.as_deref());
            b.ch(b'}');
        }
        b.ch(b']');
    }
    if let Some(t) = rc.query_time_out_in_seconds {
        b.a(",\"queryTimeOutInSeconds\":");
        b.int(t);
    }
    if let Some(q) = rc.quote_identifiers {
        b.a(",\"quoteIdentifiers\":");
        b.boolean(q);
    }
    b.a(",\"sourceInformation\":");
    src_info(b, rc.source_information.as_deref());
    if let Some(tz) = &rc.time_zone {
        b.a(",\"timeZone\":");
        str(b, tz);
    }
    b.a(",\"type\":");
    str(b, &rc.database_type);
    b.ch(b'}');
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn model_connection(
    b: &mut Buf,
    ty: &str,
    class_name: &JStr,
    class_span: Option<&SourceInfo>,
    element: Option<&JStr>,
    url: &JStr,
    span: Option<&SourceInfo>,
) {
    b.a("{\"_type\":\"");
    b.a(ty);
    b.a("\",\"class\":");
    str(b, class_name);
    b.a(",\"classSourceInformation\":");
    src_info(b, class_span);
    if let Some(e) = element {
        b.a(",\"element\":");
        str(b, e);
    }
    b.a(",\"sourceInformation\":");
    src_info(b, span);
    b.a(",\"url\":");
    str(b, url);
    b.ch(b'}');
}

pub(crate) fn elasticsearch_connection(b: &mut Buf, ec: &PElasticsearchConnection) -> Em {
    b.a("{\"_type\":\"elasticsearch7StoreConnection\",\"authSpec\":");
    connection_emitters::es_auth_spec(b, &ec.auth)?;
    b.a(",\"element\":");
    str(b, &ec.element);
    b.a(",\"elementSourceInformation\":");
    src_info(b, ec.element_source_information.as_deref());
    b.a(",\"sourceInformation\":");
    src_info(b, ec.source_information.as_deref());
    b.a(",\"sourceSpec\":{\"url\":");
    str(b, &ec.url);
    b.a("}}");
    Ok(())
}

/// One vault secret: the AWS secrets manager's, or a property/environment/system-property one, its fields sitting
/// alphabetically around `sourceInformation`.
pub fn vault_secret(b: &mut Buf, secret: &PVaultSecret) {
    match secret {
        PVaultSecret::PAwsSecret(aws) => {
            b.a("{\"_type\":\"awssecretsmanager\",\"awsCredentials\":{\"_type\":");
            str(b, &aws.creds_kind);
            if let Some(k) = &aws.access_key_id {
                b.a(",\"accessKeyId\":");
                mongo_secret(b, k);
            }
            if let Some(k) = &aws.secret_access_key {
                b.a(",\"secretAccessKey\":");
                mongo_secret(b, k);
            }
            if let Some(s) = &aws.creds_span {
                b.a(",\"sourceInformation\":");
                src_info(b, Some(s));
            }
            b.a("},\"secretId\":");
            str(b, &aws.secret_id);
            if let Some(v) = &aws.version_id {
                b.a(",\"versionId\":");
                str(b, v);
            }
            if let Some(v) = &aws.version_stage {
                b.a(",\"versionStage\":");
                str(b, v);
            }
            b.ch(b'}');
        }
        PVaultSecret::PMongoSecret(sec) => mongo_secret(b, sec),
    }
}

/// A `PMongoSecret` through `vaultSecret` (its field key before or after `sourceInformation`, by `compareTo`).
pub(crate) fn mongo_secret(b: &mut Buf, sec: &PMongoSecret) {
    b.a("{\"_type\":");
    str(b, &sec.kind);
    let source_information: Vec<u16> = "sourceInformation".encode_utf16().collect();
    if sec.field_key.0[..] < source_information[..] {
        b.a(",\"");
        b.units(&sec.field_key.0);
        b.a("\":");
        str(b, &sec.value);
        b.a(",\"sourceInformation\":");
        src_info(b, sec.source_information.as_deref());
    } else {
        b.a(",\"sourceInformation\":");
        src_info(b, sec.source_information.as_deref());
        b.a(",\"");
        b.units(&sec.field_key.0);
        b.a("\":");
        str(b, &sec.value);
    }
    b.ch(b'}');
}

// ---------------------------------------------------------------------------------------------------------------------
// Function activators
// ---------------------------------------------------------------------------------------------------------------------

/// The function-activator family's wire.
pub(crate) fn function_activator(b: &mut Buf, fa: &PFunctionActivator) -> Em {
    if fa.kind == "HostedServiceDeploymentConfiguration" {
        b.a("{\"_type\":\"hostedServiceDeploymentConfiguration\",\"port\":0}");
        return Ok(());
    }
    if fa.kind == "BigQueryFunctionDeploymentConfiguration" || fa.kind == "MemSqlFunctionDeploymentConfiguration" {
        b.a("{\"_type\":\"");
        b.a(if crate::ustr::starts_with(&fa.kind.0, "BigQuery") {
            "bigQueryFunctionConfig"
        } else {
            "memSqlFunctionConfig"
        });
        b.a("\",\"activationConnection\":{\"_type\":\"connectionPointer\",\"connection\":");
        let Some(conn) = &fa.activation_connection else {
            return Err(npe(None));
        };
        str(b, conn);
        b.a(",\"sourceInformation\":");
        let Some(span) = &fa.activation_connection_span else {
            return Err(npe(None));
        };
        src_info(b, Some(span));
        b.a("}}");
        return Ok(());
    }
    let kind = fa.kind.lossy();
    let wire_type = match kind.as_str() {
        "SnowflakeApp" => "snowflakeApp",
        "SnowflakeM2MUdf" => "snowflakeM2MUdf",
        "MemSqlFunction" => "memSqlFunction",
        "BigQueryFunction" => "bigQueryFunction",
        "HostedService" => "hostedService",
        "FunctionJar" => "functionJar",
        "DeephavenApp" => "DeephavenApp",
        _ => return Err(illegal_state(jcat!("unknown activator kind: ", fa.kind))),
    };
    b.a("{\"_type\":\"");
    b.a(wire_type);
    b.a("\",\"actions\":[]");
    if let Some(conn) = &fa.activation_connection
        && fa.kind != "FunctionJar"
        && fa.kind != "HostedService"
    {
        let cfg_type = match kind.as_str() {
            "SnowflakeApp" => "snowflakeDeploymentConfiguration",
            "SnowflakeM2MUdf" => "snowflakeM2MUdfDeploymentConfiguration",
            "MemSqlFunction" => "memSqlFunctionConfig",
            "BigQueryFunction" => "bigQueryFunctionConfig",
            _ => {
                return Err(unsupported(jcat!(
                    "activationConfiguration wire for ",
                    fa.kind,
                    " is unprobed"
                )));
            }
        };
        b.a(",\"activationConfiguration\":{\"_type\":\"");
        b.a(cfg_type);
        b.a("\",\"activationConnection\":{\"_type\":\"connectionPointer\",\"connection\":");
        str(b, conn);
        if crate::ustr::starts_with(&fa.kind.0, "Snowflake") {
            b.a(",\"sourceInformation\":");
            src_info(b, fa.activation_connection_span.as_deref());
        }
        b.a("}}");
    }
    scalar_slot(b, fa, "applicationName");
    if let Some((_, auto)) = fa.booleans.iter().find(|(k, _)| *k == "autoActivateUpdates") {
        b.a(",\"autoActivateUpdates\":");
        b.boolean(*auto);
    }
    scalar_slot(b, fa, "deploymentSchema");
    scalar_slot(b, fa, "deploymentStage");
    scalar_slot(b, fa, "description");
    scalar_slot(b, fa, "documentation");
    b.a(",\"function\":{\"path\":");
    str(b, &fa.function_path);
    b.a(",\"sourceInformation\":");
    src_info(b, fa.function_span.as_deref());
    b.a(",\"type\":\"FUNCTION\"}");
    scalar_slot(b, fa, "functionName");
    if fa.kind == "HostedService" {
        b.a(",\"generateLineage\":false");
    }
    b.a(",\"name\":");
    str(b, &fa.name);
    if let Some(owner) = &fa.owner_id {
        b.a(",\"ownership\":{\"_type\":\"DeploymentOwner\",\"id\":");
        str(b, owner);
        b.ch(b'}');
    } else if let Some(users) = &fa.user_list_users {
        b.a(",\"ownership\":{\"_type\":\"userList\",\"users\":[");
        strs_each(b, users);
        b.a("]}");
    }
    b.a(",\"package\":");
    str(b, &fa.pkg);
    scalar_slot(b, fa, "pattern");
    scalar_slot(b, fa, "permissionScheme");
    b.a(",\"sourceInformation\":");
    src_info(b, fa.source_information.as_deref());
    b.a(",\"stereotypes\":");
    stereotypes(b, &fa.stereotypes);
    if fa.kind == "HostedService" {
        b.a(",\"storeModel\":false");
    }
    b.a(",\"taggedValues\":");
    tagged_values(b, &fa.tagged_values);
    scalar_slot(b, fa, "udfName");
    scalar_slot(b, fa, "usageRole");
    b.ch(b'}');
    Ok(())
}

pub(crate) fn scalar_slot(b: &mut Buf, fa: &PFunctionActivator, key: &str) {
    if let Some((_, v)) = fa.scalars.iter().find(|(k, _)| *k == key) {
        b.a(",\"");
        b.a(key);
        b.a("\":");
        str(b, v);
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// Islands
// ---------------------------------------------------------------------------------------------------------------------

/// A `classInstance` of type GQL: its value the GraphQL document's wire.
pub(crate) fn gql_island(b: &mut Buf, gi: &GqlIsland, span_override: Option<&SourceInfo>) -> Em {
    b.a("{\"_type\":\"classInstance\",\"sourceInformation\":");
    src_info(b, span_override.or(gi.pos.as_deref()));
    b.a(",\"type\":\"GQL\",\"value\":");
    gql_emitter::document(b, &gi.document)?;
    b.ch(b'}');
    Ok(())
}

/// `#SQL{ ... }#` (and the Cypher and IQ islands): a classInstance whose value is the text.
pub(crate) fn sql_island(b: &mut Buf, si: &SqlIsland, span: Option<&SourceInfo>) {
    b.a("{\"_type\":\"classInstance\",\"sourceInformation\":");
    src_info(b, span.or(si.pos.as_deref()));
    b.a(",\"type\":");
    str(b, &si.language);
    b.a(if si.language == "Cypher" {
        ",\"value\":{\"cypher\":"
    } else {
        ",\"value\":{\"sql\":"
    });
    str(b, &si.sql);
    if let Some(p) = &si.profile {
        b.a(",\"profile\":");
        str(b, p);
    }
    b.a("}}");
}

/// `#TDS{ ... }#`: a classInstance of type TDS whose value is the untrimmed inner text.
pub(crate) fn tds_literal(b: &mut Buf, tl: &TdsLiteral, span: Option<&SourceInfo>) {
    b.a("{\"_type\":\"classInstance\",\"sourceInformation\":");
    src_info(b, span.or(tl.pos.as_deref()));
    b.a(",\"type\":\"TDS\",\"value\":{\"tdsString\":");
    str(b, &tl.tds_string);
    b.a("}}");
}

// ---------------------------------------------------------------------------------------------------------------------
// Functions and their tests
// ---------------------------------------------------------------------------------------------------------------------

/// `_type:"function"`: the wire name signature-mangled, typed var parameters, the body the bare statement list.
pub(crate) fn function(b: &mut Buf, f: &PFunction) -> Em {
    require(
        f.type_params.is_empty() && f.mult_params.is_empty(),
        "function type/multiplicity parameters",
        &crate::protocol::qualified_name(&f.pkg.0, &f.name.0),
    )?;
    b.a("{\"_type\":\"function\",\"body\":[");
    comma_each(b, &f.body, value_spec)?;
    b.a("],\"name\":");
    match crate::protocol::mangled_name(f) {
        Ok(name) => str(b, &name),
        Err(message) => return Err(unsupported(JStr::from(message))),
    }
    b.a(",\"package\":");
    str(b, &f.pkg);
    b.a(",\"parameters\":[");
    for (i, p) in f.parameters.iter().enumerate() {
        if i > 0 {
            b.ch(b',');
        }
        b.a("{\"_type\":\"var\",\"genericType\":");
        generic_type(b, &p.r#type)?;
        b.a(",\"multiplicity\":");
        multiplicity(b, &p.multiplicity)?;
        b.a(",\"name\":");
        str(b, &p.name);
        b.a(",\"sourceInformation\":");
        src_info(b, p.pos.as_deref());
        b.ch(b'}');
    }
    b.a("],\"postConstraints\":[");
    function_constraints(b, f, true, &f.post_constraints)?;
    b.a("],\"preConstraints\":[");
    function_constraints(b, f, false, &f.pre_constraints)?;
    b.a("],\"returnGenericType\":");
    generic_type(b, &f.return_type)?;
    b.a(",\"returnMultiplicity\":");
    multiplicity(b, &f.return_multiplicity)?;
    b.a(",\"sourceInformation\":");
    src_info(b, f.source_information.as_deref());
    b.a(",\"stereotypes\":");
    stereotypes(b, &f.stereotypes);
    b.a(",\"taggedValues\":");
    tagged_values(b, &f.tagged_values);
    b.a(",\"tests\":[");
    comma_each(b, &f.test_suites, |b, s| test_suite(b, s))?;
    b.a("]}");
    Ok(())
}

/// One `functionTestSuite`.
pub(crate) fn test_suite(b: &mut Buf, s: &PTestSuite) -> Em {
    b.a("{\"_type\":\"functionTestSuite\",\"id\":");
    match &s.id {
        Some(id) => str(b, id),
        None => strs(b, "default"),
    }
    b.a(",\"sourceInformation\":");
    src_info(b, s.source_information.as_deref());
    b.a(",\"testData\":[");
    for (i, d) in s.test_data.iter().enumerate() {
        if i > 0 {
            b.ch(b',');
        }
        b.a("{\"data\":");
        test_payload(b, &d.data);
        b.a(",\"packageableElementPointer\":{\"path\":");
        str(b, &d.store_path);
        b.a(",\"sourceInformation\":");
        src_info(b, d.store_span.as_deref());
        if let Some(t) = &d.pointer_type {
            b.a(",\"type\":");
            str(b, t);
        }
        b.a("},\"sourceInformation\":");
        src_info(b, d.source_information.as_deref());
        b.ch(b'}');
    }
    b.a("],\"tests\":[");
    for (i, t) in s.tests.iter().enumerate() {
        if i > 0 {
            b.ch(b',');
        }
        b.a("{\"_type\":\"functionTest\",\"assertions\":[");
        if let Some(a) = &t.assertion {
            assertion(b, a)?;
        }
        b.ch(b']');
        if let Some(doc) = &t.doc {
            b.a(",\"doc\":");
            str(b, doc);
        }
        b.a(",\"id\":");
        str(b, &t.id);
        if !t.parameters.is_empty() {
            b.a(",\"parameters\":[");
            for (k, pa) in t.parameters.iter().enumerate() {
                if k > 0 {
                    b.ch(b',');
                }
                b.ch(b'{');
                if let Some(n) = &pa.name {
                    b.a("\"name\":");
                    str(b, n);
                    b.ch(b',');
                }
                b.a("\"sourceInformation\":");
                src_info(b, pa.source_information.as_deref());
                b.a(",\"value\":");
                value_spec(b, &pa.value)?;
                b.ch(b'}');
            }
            b.ch(b']');
        }
        b.a(",\"sourceInformation\":");
        src_info(b, t.source_information.as_deref());
        b.ch(b'}');
    }
    b.a("]}");
    Ok(())
}

/// The three assertion spellings; the assertion id is always `default`.
pub(crate) fn assertion(b: &mut Buf, a: &PAssertion) -> Em {
    match a {
        PAssertion::PAssertionEqualTo(eq) => {
            b.a("{\"_type\":\"equalTo\",\"expected\":");
            value_spec(b, &eq.expected)?;
            b.a(",\"id\":\"default\",\"sourceInformation\":");
            src_info(b, eq.span.as_deref());
            b.ch(b'}');
        }
        PAssertion::PAssertionEqualToJson(ej) => {
            b.a("{\"_type\":\"equalToJson\",\"expected\":");
            test_payload_external_format(b, &ej.expected);
            b.a(",\"id\":\"default\",\"sourceInformation\":");
            src_info(b, ej.span.as_deref());
            b.ch(b'}');
        }
        PAssertion::PAssertionEqualToRelation(er) => {
            b.a("{\"_type\":\"equalToRelation\",\"expected\":");
            relation_element(b, &er.expected);
            b.a(",\"id\":\"default\",\"sourceInformation\":");
            src_info(b, er.span.as_deref());
            b.ch(b'}');
        }
    }
    Ok(())
}

pub(crate) fn test_payload_external_format(b: &mut Buf, ef: &PTestPayloadExternalFormat) {
    b.a("{\"_type\":\"externalFormat\",\"contentType\":");
    str(b, &ef.content_type);
    b.a(",\"data\":");
    str(b, &ef.data);
    b.a(",\"sourceInformation\":");
    src_info(b, ef.source_information.as_deref());
    b.ch(b'}');
}

pub(crate) fn test_payload(b: &mut Buf, p: &PTestPayload) {
    match p {
        PTestPayload::PTestPayloadExternalFormat(ef) => test_payload_external_format(b, ef),
        PTestPayload::PTestPayloadReference(r) => {
            b.a("{\"_type\":\"reference\",\"dataElement\":{\"path\":");
            str(b, &r.path);
            b.a(",\"sourceInformation\":");
            src_info(b, r.source_information.as_deref());
            b.a(",\"type\":");
            match &r.ref_type {
                Some(t) => str(b, t),
                None => strs(b, "DATA"),
            }
            b.a("},\"sourceInformation\":");
            src_info(b, r.source_information.as_deref());
            b.ch(b'}');
        }
        PTestPayload::PTestPayloadRelationElements(re) => {
            b.a("{\"_type\":\"relationAccessor\",\"relationElements\":[");
            for (i, e) in re.elements.iter().enumerate() {
                if i > 0 {
                    b.ch(b',');
                }
                relation_element(b, e);
            }
            b.a("],\"sourceInformation\":");
            src_info(b, re.source_information.as_deref());
            b.ch(b'}');
        }
        PTestPayload::PTestPayloadModelStoreData(ms) => {
            b.a("{\"_type\":\"modelStore\",\"modelData\":[");
            for (i, me) in ms.model_data.iter().enumerate() {
                if i > 0 {
                    b.ch(b',');
                }
                b.a("{\"_type\":\"modelEmbeddedData\",\"data\":");
                test_payload_external_format(b, &me.data);
                b.a(",\"model\":");
                str(b, &me.model);
                b.a(",\"sourceInformation\":");
                src_info(b, me.source_information.as_deref());
                b.ch(b'}');
            }
            b.a("],\"sourceInformation\":");
            src_info(b, ms.source_information.as_deref());
            b.ch(b'}');
        }
        PTestPayload::PTestPayloadRelationalCsv(rc) => {
            b.a("{\"_type\":\"relationalCSVData\",\"sourceInformation\":");
            src_info(b, rc.source_information.as_deref());
            b.a(",\"tables\":[");
            for (i, t) in rc.tables.iter().enumerate() {
                if i > 0 {
                    b.ch(b',');
                }
                b.a("{\"schema\":");
                str(b, &t.schema);
                b.a(",\"sourceInformation\":");
                src_info(b, t.source_information.as_deref());
                b.a(",\"table\":");
                str(b, &t.table);
                if let Some(v) = &t.values {
                    b.a(",\"values\":");
                    str(b, v);
                }
                b.ch(b'}');
            }
            b.a("]}");
        }
    }
}

/// The bare columns/paths/rows shape, every cell a string.
pub(crate) fn relation_element(b: &mut Buf, el: &PTestPayloadRelationElement) {
    b.a("{\"columns\":[");
    strs_each(b, &el.columns);
    b.a("],\"paths\":[");
    strs_each(b, &el.paths);
    b.a("],\"rows\":[");
    for (i, row) in el.rows.iter().enumerate() {
        if i > 0 {
            b.ch(b',');
        }
        b.a("{\"values\":[");
        strs_each(b, row);
        b.a("]}");
    }
    b.a("],\"sourceInformation\":");
    src_info(b, el.source_information.as_deref());
    b.ch(b'}');
}

// ---------------------------------------------------------------------------------------------------------------------
// Associations, profiles, enumerations, classes
// ---------------------------------------------------------------------------------------------------------------------

pub(crate) fn association(b: &mut Buf, a: &PAssociation) -> Em {
    b.a("{\"_type\":\"association\",\"name\":");
    str(b, &a.name);
    b.a(",\"originalMilestonedProperties\":[");
    comma_each(b, &a.original_milestoned_properties, |b, p| property(b, p))?;
    b.a("],\"package\":");
    str(b, &a.pkg);
    b.a(",\"properties\":[");
    comma_each(b, &a.properties, |b, p| property(b, p))?;
    b.a("],\"qualifiedProperties\":[");
    comma_each(b, &a.derived_properties, |b, d| qualified_property(b, d))?;
    b.a("],\"sourceInformation\":");
    src_info(b, a.source_information.as_deref());
    b.a(",\"stereotypes\":");
    stereotypes(b, &a.stereotypes);
    b.a(",\"taggedValues\":");
    tagged_values(b, &a.tagged_values);
    b.ch(b'}');
    Ok(())
}

pub(crate) fn profile(b: &mut Buf, p: &PProfile) {
    b.a("{\"_type\":\"profile\",\"name\":");
    str(b, &p.name);
    b.a(",\"package\":");
    str(b, &p.pkg);
    b.a(",\"sourceInformation\":");
    src_info(b, p.source_information.as_deref());
    b.a(",\"stereotypes\":");
    profile_entries(b, &p.stereotypes);
    b.a(",\"tags\":");
    profile_entries(b, &p.tags);
    b.ch(b'}');
}

pub(crate) fn profile_entries(b: &mut Buf, es: &[Box<PProfileEntry>]) {
    b.ch(b'[');
    for (i, e) in es.iter().enumerate() {
        if i > 0 {
            b.ch(b',');
        }
        b.a("{\"sourceInformation\":");
        src_info(b, e.source_information.as_deref());
        b.a(",\"value\":");
        str(b, &e.value);
        b.ch(b'}');
    }
    b.ch(b']');
}

/// `_type:"Enumeration"`, capitalized (an engine quirk).
pub(crate) fn enumeration(b: &mut Buf, e: &PEnumeration) {
    b.a("{\"_type\":\"Enumeration\",\"name\":");
    str(b, &e.name);
    b.a(",\"package\":");
    str(b, &e.pkg);
    b.a(",\"sourceInformation\":");
    src_info(b, e.source_information.as_deref());
    b.a(",\"stereotypes\":");
    stereotypes(b, &e.stereotypes);
    b.a(",\"taggedValues\":");
    tagged_values(b, &e.tagged_values);
    b.a(",\"values\":[");
    for (i, v) in e.values.iter().enumerate() {
        if i > 0 {
            b.ch(b',');
        }
        b.a("{\"sourceInformation\":");
        src_info(b, v.source_information.as_deref());
        b.a(",\"stereotypes\":");
        stereotypes(b, &v.stereotypes);
        b.a(",\"taggedValues\":");
        tagged_values(b, &v.tagged_values);
        b.a(",\"value\":");
        str(b, &v.value);
        b.ch(b'}');
    }
    b.a("]}");
}

pub(crate) fn pclass(b: &mut Buf, c: &PClass) -> Em {
    require(
        c.type_params.is_empty() && c.multiplicity_params.is_empty(),
        "class type parameters",
        &crate::protocol::qualified_name(&c.pkg.0, &c.name.0),
    )?;
    b.a("{\"_type\":\"class\",\"constraints\":[");
    comma_each(b, &c.constraints, |b, k| constraint(b, k, None))?;
    b.a("],\"name\":");
    str(b, &c.name);
    b.a(",\"originalMilestonedProperties\":[");
    comma_each(b, &c.original_milestoned_properties, |b, p| property(b, p))?;
    b.a("],\"package\":");
    str(b, &c.pkg);
    b.a(",\"properties\":[");
    comma_each(b, &c.properties, |b, p| property(b, p))?;
    b.a("],\"qualifiedProperties\":[");
    comma_each(b, &c.derived_properties, |b, d| qualified_property(b, d))?;
    b.a("],\"sourceInformation\":");
    src_info(b, c.source_information.as_deref());
    b.a(",\"stereotypes\":");
    stereotypes(b, &c.stereotypes);
    b.a(",\"superTypes\":[");
    comma_each(b, &c.super_types, |b, st| super_type(b, st))?;
    b.a("],\"taggedValues\":");
    tagged_values(b, &c.tagged_values);
    b.ch(b'}');
    Ok(())
}

/// `{"path":…,"sourceInformation":…,"type":"CLASS"}`: a generic supertype's arguments are dropped at the engine's
/// dialect and refused by name at lite's (S89).
pub(crate) fn super_type(b: &mut Buf, st: &PSuperType) -> Em {
    let path = match &st.r#type {
        TypeExpression::TypeExpressionNameRef(n) => &n.name,
        TypeExpression::TypeExpressionGeneric(g) => {
            placeless(
                g.arguments.is_empty() && g.multiplicity_arguments.is_empty() && g.type_variable_values.is_empty(),
                "a supertype's type arguments",
                &g.name.0,
            )?;
            &g.name
        }
        other => {
            return Err(unsupported(jcat!(
                "ProtocolEmitter has no rule for a supertype of shape ",
                crate::protocol::type_expression_simple_name(other),
                " \u{2014} add the emit rule."
            )));
        }
    };
    b.a("{\"path\":");
    str(b, path);
    b.a(",\"sourceInformation\":");
    src_info(b, st.source_information.as_deref());
    b.a(",\"type\":\"CLASS\"}");
    Ok(())
}

pub(crate) fn property(b: &mut Buf, p: &PProperty) -> Em {
    b.ch(b'{');
    if let Some(agg) = &p.aggregation {
        b.a("\"aggregation\":");
        str(b, agg);
        b.ch(b',');
    }
    if let Some(dv) = &p.default_value {
        b.a("\"defaultValue\":{\"sourceInformation\":");
        src_info(b, dv.source_information.as_deref());
        b.a(",\"value\":");
        let Some(v) = &dv.value else {
            return Err(unsupported(jcat!(
                "ProtocolEmitter has no rule for this defaultValue expression (at ",
                p.name,
                ") \u{2014} the parser accepted it but built no value-spec; extend SpecParser coverage, do not drop it."
            )));
        };
        value_spec(b, v)?;
        b.a("},");
    }
    b.a("\"genericType\":");
    generic_type(b, &p.r#type)?;
    b.a(",\"multiplicity\":");
    multiplicity(b, &p.multiplicity)?;
    b.a(",\"name\":");
    str(b, &p.name);
    b.a(",\"sourceInformation\":");
    src_info(b, p.source_information.as_deref());
    b.a(",\"stereotypes\":");
    stereotypes(b, &p.stereotypes);
    b.a(",\"taggedValues\":");
    tagged_values(b, &p.tagged_values);
    b.ch(b'}');
    Ok(())
}

/// `[{"profile":…,"profileSourceInformation":…,"sourceInformation":…,"value":…}]`
pub fn stereotypes(b: &mut Buf, ss: &[Box<PStereotype>]) {
    b.ch(b'[');
    for (i, st) in ss.iter().enumerate() {
        if i > 0 {
            b.ch(b',');
        }
        b.a("{\"profile\":");
        str(b, &st.profile);
        b.a(",\"profileSourceInformation\":");
        src_info(b, st.profile_source_information.as_deref());
        b.a(",\"sourceInformation\":");
        src_info(b, st.source_information.as_deref());
        b.a(",\"value\":");
        str(b, &st.value);
        b.ch(b'}');
    }
    b.ch(b']');
}

/// `[{"sourceInformation":…,"tag":{…},"value":…}]`: a `'''...'''` value an object carrying its own `_type`.
pub fn tagged_values(b: &mut Buf, ts: &[Box<PTaggedValue>]) {
    b.ch(b'[');
    for (i, tv) in ts.iter().enumerate() {
        if i > 0 {
            b.ch(b',');
        }
        b.a("{\"sourceInformation\":");
        src_info(b, tv.source_information.as_deref());
        b.a(",\"tag\":{\"profile\":");
        str(b, &tv.tag.profile);
        b.a(",\"profileSourceInformation\":");
        src_info(b, tv.tag.profile_source_information.as_deref());
        b.a(",\"sourceInformation\":");
        src_info(b, tv.tag.source_information.as_deref());
        b.a(",\"value\":");
        str(b, &tv.tag.value);
        b.a("},\"value\":");
        if tv.multi_line {
            b.a("{\"_type\":\"string\",\"multiLine\":true,\"value\":");
            str(b, &tv.value);
            b.ch(b'}');
        } else {
            str(b, &tv.value);
        }
        b.ch(b'}');
    }
    b.ch(b']');
}

// ---------------------------------------------------------------------------------------------------------------------
// Types and multiplicities
// ---------------------------------------------------------------------------------------------------------------------

/// The wire's `genericType`.
pub(crate) fn generic_type(b: &mut Buf, t: &TypeExpression) -> Em {
    match t {
        TypeExpression::TypeExpressionNameRef(n) => generic_type_of(b, &n.name, &[], &[], &[], n.pos.as_deref()),
        TypeExpression::TypeExpressionGeneric(g) => {
            // the engine's grammar has no schema algebra (`List<A-B>`): it refuses at the `<` (PARITY row 87)
            if let Some(at) = g.pos.as_deref()
                && g.arguments
                    .iter()
                    .any(|a| matches!(a, TypeExpression::TypeExpressionSchemaAlgebra(_)))
            {
                let column = at.start_column + g.name.0.len() as i32;
                return Err(JavaError::parse(JStr::from("Unexpected token '<'"), at.start_line, column).as_refusal());
            }
            generic_type_of(
                b,
                &g.name,
                &g.arguments,
                &g.multiplicity_arguments,
                &g.type_variable_values,
                g.pos.as_deref(),
            )
        }
        TypeExpression::TypeExpressionRelationType(rt) => {
            b.a("{\"multiplicityArguments\":[],\"rawType\":{\"_type\":\"relationType\",\"columns\":[");
            for (i, col) in rt.columns.iter().enumerate() {
                if i > 0 {
                    b.ch(b',');
                }
                b.a("{\"genericType\":");
                generic_type(b, &col.r#type)?;
                b.a(",\"multiplicity\":");
                multiplicity(b, &col.multiplicity)?;
                b.a(",\"name\":");
                str(b, &col.name);
                b.a(",\"sourceInformation\":");
                src_info(b, col.pos.as_deref());
                b.ch(b'}');
            }
            b.a("]},\"typeArguments\":[],\"typeVariableValues\":[]}");
            Ok(())
        }
        other => Err(unsupported(jcat!(
            "ProtocolEmitter has no rule for type expression ",
            crate::protocol::type_expression_simple_name(other),
            " \u{2014} add the emit rule, do not drop it."
        ))),
    }
}

pub(crate) fn generic_type_of(
    b: &mut Buf,
    path: &JStr,
    args: &[TypeExpression],
    mult_args: &[JStr],
    type_var_values: &[ValueSpecification],
    pos: Option<&SourceInfo>,
) -> Em {
    // engine backward-compat (DomainParseTreeWalker.processType): a bare 'Result' defaults to
    // <meta::pure::metamodel::type::Any|1>, the synthesized Any spanless, the multiplicity with no upper bound
    if *path == "Result" && args.is_empty() && mult_args.is_empty() {
        b.a("{\"multiplicityArguments\":[{\"lowerBound\":1}],\"rawType\":{\"_type\":\"packageableType\",\"fullPath\":\"Result\",\"sourceInformation\":");
        src_info(b, pos);
        b.a("},\"typeArguments\":[{\"multiplicityArguments\":[],\"rawType\":{\"_type\":\"packageableType\",\"fullPath\":\"meta::pure::metamodel::type::Any\"},\"typeArguments\":[],\"typeVariableValues\":[]}],\"typeVariableValues\":[]}");
        return Ok(());
    }
    b.a("{\"multiplicityArguments\":[");
    for (i, m) in mult_args.iter().enumerate() {
        if i > 0 {
            b.ch(b',');
        }
        multiplicity(b, &parse_mult_arg(&m.0, &path.0)?)?;
    }
    b.a("],\"rawType\":{\"_type\":\"packageableType\",\"fullPath\":");
    str(b, path);
    b.a(",\"sourceInformation\":");
    src_info(b, pos);
    b.a("},\"typeArguments\":[");
    comma_each(b, args, generic_type)?;
    b.a("],\"typeVariableValues\":[");
    comma_each(b, type_var_values, value_spec)?;
    b.a("]}");
    Ok(())
}

pub(crate) fn multiplicity(b: &mut Buf, m: &Multiplicity) -> Em {
    let Multiplicity::MultiplicityConcrete(c) = m else {
        return Err(unsupported(JStr::from(
            "ProtocolEmitter has no rule for a multiplicity PARAMETER \u{2014} add the emit rule.",
        )));
    };
    b.a("{\"lowerBound\":");
    b.int(i64::from(c.lower_bound));
    if let Some(up) = c.upper_bound {
        b.a(",\"upperBound\":");
        b.int(i64::from(up));
    }
    b.ch(b'}');
    Ok(())
}

/// `new Multiplicity.Concrete(lower, upper)`: a negative lower bound refused.
pub(crate) fn concrete(lower: i32, upper: Option<i32>) -> Result<Multiplicity, JavaError> {
    if lower < 0 {
        return Err(illegal_argument(jcat!("lowerBound must be >= 0, got ", lower)));
    }
    Ok(Multiplicity::MultiplicityConcrete(Box::new(MultiplicityConcrete {
        lower_bound: lower,
        upper_bound: upper,
    })))
}

/// `ProtocolEmitter.parseMultArg`: a multiplicity argument as the grammar spells it (`*`, `1`, `0..1`, `1..*`); a
/// parameter's name refused.
pub fn parse_mult_arg(text: &[u16], where_: &[u16]) -> Result<Multiplicity, JavaError> {
    if crate::ustr::eq(text, "*") {
        return concrete(0, None);
    }
    let refuse = || {
        unsupported(jcat!(
            "ProtocolEmitter has no rule for a multiplicity PARAMETER '",
            text,
            "' in generic ",
            where_,
            " \u{2014} add the emit rule."
        ))
    };
    match crate::ustr::index_of_units(text, &crate::ustr::u(".."), 0) {
        None => {
            let n = crate::jnum::parse_int(text).map_err(|_| refuse())?;
            concrete(n, Some(n))
        }
        Some(dots) => {
            let lo = crate::jnum::parse_int(&text[..dots]).map_err(|_| refuse())?;
            let hi = &text[dots + 2..];
            let upper = if crate::ustr::eq(hi, "*") {
                None
            } else {
                Some(crate::jnum::parse_int(hi).map_err(|_| refuse())?)
            };
            concrete(lo, upper)
        }
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// Constraints and qualified properties
// ---------------------------------------------------------------------------------------------------------------------

/// A function's conditions: each predicate a lambda over the function's parameters (a post-condition's over `return`
/// too); a message's lambda over none.
pub(crate) fn function_constraints(b: &mut Buf, f: &PFunction, post: bool, cs: &[Box<ConstraintDefinition>]) -> Em {
    let mut params = Buf::new();
    for p in &f.parameters {
        condition_parameter(&mut params, &p.name, &p.r#type, &p.multiplicity)?;
    }
    if post {
        condition_parameter(
            &mut params,
            &JStr::from("return"),
            &f.return_type,
            &f.return_multiplicity,
        )?;
    }
    for (i, c) in cs.iter().enumerate() {
        if i > 0 {
            b.ch(b',');
        }
        constraint(b, c, Some(&params.u))?;
    }
    Ok(())
}

pub(crate) fn condition_parameter(b: &mut Buf, name: &JStr, ty: &TypeExpression, m: &Multiplicity) -> Em {
    if !b.is_empty() {
        b.ch(b',');
    }
    b.a("{\"_type\":\"var\",\"genericType\":");
    generic_type(b, ty)?;
    b.a(",\"multiplicity\":");
    multiplicity(b, m)?;
    b.a(",\"name\":");
    str(b, name);
    b.ch(b'}');
    Ok(())
}

/// One constraint: `condition_parameters` `None` for a class's (its lambdas over `$this`), else a function's (its
/// predicate over those parameters, its message over none).
pub(crate) fn constraint(b: &mut Buf, c: &ConstraintDefinition, condition_parameters: Option<&[u16]>) -> Em {
    let on_this = condition_parameters.is_none();
    let cbody = realization_body(&c.realization, &jcat!("constraint ", c.name).0)?;
    b.ch(b'{');
    if let Some(level) = &c.enforcement_level {
        b.a("\"enforcementLevel\":");
        str(b, level);
        b.ch(b',');
    }
    if let Some(id) = &c.external_id {
        b.a("\"externalId\":");
        str(b, id);
        b.ch(b',');
    }
    b.a("\"functionDefinition\":");
    constraint_lambda(b, &cbody, if on_this { None } else { condition_parameters })?;
    if let Some(m) = &c.message {
        b.a(",\"messageFunction\":");
        constraint_lambda(b, std::slice::from_ref(m), if on_this { None } else { Some(&[]) })?;
    }
    b.a(",\"name\":");
    str(b, &c.name);
    if let Some(owner) = &c.owner {
        b.a(",\"owner\":");
        str(b, owner);
    }
    b.a(",\"sourceInformation\":");
    src_info(b, c.pos.as_deref());
    b.ch(b'}');
    Ok(())
}

/// One qualified (derived) property: the body the bare statement list, the declared parameters typed vars.
pub(crate) fn qualified_property(b: &mut Buf, d: &DerivedPropertyDefinition) -> Em {
    let qbody = realization_body(&d.realization, &jcat!("qualified property ", d.name).0)?;
    b.a("{\"body\":[");
    comma_each(b, &qbody, value_spec)?;
    b.a("],\"name\":");
    str(b, &d.name);
    b.a(",\"parameters\":[");
    for (i, p) in d.parameters.iter().enumerate() {
        if i > 0 {
            b.ch(b',');
        }
        b.a("{\"_type\":\"var\",\"genericType\":");
        generic_type(b, &p.r#type)?;
        b.a(",\"multiplicity\":");
        multiplicity(b, &p.multiplicity)?;
        b.a(",\"name\":");
        str(b, &p.name);
        b.a(",\"sourceInformation\":");
        src_info(b, p.pos.as_deref());
        b.ch(b'}');
    }
    b.a("],\"returnGenericType\":");
    generic_type(b, &d.r#type)?;
    b.a(",\"returnMultiplicity\":");
    multiplicity(b, &d.multiplicity)?;
    b.a(",\"sourceInformation\":");
    src_info(b, d.pos.as_deref());
    b.a(",\"stereotypes\":");
    stereotypes(b, &d.stereotypes);
    b.a(",\"taggedValues\":");
    tagged_values(b, &d.tagged_values);
    b.ch(b'}');
    Ok(())
}

/// A constraint's lambda: over `$this` for a class's (`parameters` `None`), else over the parameters written.
pub(crate) fn constraint_lambda(b: &mut Buf, body: &[ValueSpecification], parameters: Option<&[u16]>) -> Em {
    let Some(parameters) = parameters else {
        return this_lambda(b, body);
    };
    b.a("{\"_type\":\"lambda\",\"body\":[");
    comma_each(b, body, value_spec)?;
    b.a("],\"parameters\":[");
    b.units(parameters);
    b.a("]}");
    Ok(())
}

pub(crate) fn this_lambda(b: &mut Buf, body: &[ValueSpecification]) -> Em {
    b.a("{\"_type\":\"lambda\",\"body\":[");
    comma_each(b, body, value_spec)?;
    b.a("],\"parameters\":[{\"_type\":\"var\",\"multiplicity\":{\"lowerBound\":1,\"upperBound\":1},\"name\":\"this\"}]}");
    Ok(())
}

/// A realization's wire body (`Realization.writtenBody`): the inline statements, or the kept reference node.
pub(crate) fn realization_body(r: &Realization, what: &[u16]) -> Result<Vec<ValueSpecification>, JavaError> {
    match r {
        Realization::RealizationInline(inl) => Ok(inl.body.clone()),
        Realization::RealizationRef(rf) => match &rf.source {
            Some(s) => Ok(vec![s.clone()]),
            None => Err(unsupported(jcat!(
                "ProtocolEmitter needs the parsed reference node for ",
                what,
                " and the parser did not keep one \u{2014} fix the parse site."
            ))),
        },
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// Value specifications
// ---------------------------------------------------------------------------------------------------------------------

/// A value specification's Java class's `getSimpleName()`.
pub fn simple_name(v: &ValueSpecification) -> &'static str {
    use ValueSpecification as V;
    match v {
        V::AppliedFunction(_) => "AppliedFunction",
        V::AppliedProperty(_) => "AppliedProperty",
        V::CBoolean(_) => "CBoolean",
        V::CDate(_) => "CDate",
        V::CDecimal(_) => "CDecimal",
        V::CFloat(_) => "CFloat",
        V::CInteger(_) => "CInteger",
        V::CLatestDate(_) => "CLatestDate",
        V::GraphFetchLiteral(_) => "GraphFetchLiteral",
        V::QuotedTreeCall(_) => "QuotedTreeCall",
        V::QuotedGrammarCall(_) => "QuotedGrammarCall",
        V::PathLiteral(_) => "PathLiteral",
        V::SqlIsland(_) => "SqlIsland",
        V::GqlIsland(_) => "GqlIsland",
        V::TdsLiteral(_) => "TdsLiteral",
        V::CString(_) => "CString",
        V::CByteArray(_) => "CByteArray",
        V::CTime(_) => "CTime",
        V::ColumnInstance(ColumnInstance::ColSpec(_)) => "ColSpec",
        V::ColumnInstance(ColumnInstance::ColSpecArray(_)) => "ColSpecArray",
        V::EnumValue(_) => "EnumValue",
        V::LambdaFunction(_) => "LambdaFunction",
        V::NewInstance(_) => "NewInstance",
        V::NewInstanceCast(_) => "NewInstanceCast",
        V::PackageableElementPtr(_) => "PackageableElementPtr",
        V::PureCollection(_) => "PureCollection",
        V::TypeAnnotation(TypeAnnotation::TypeAnnotationNamed(_)) => "Named",
        V::TypeAnnotation(TypeAnnotation::TypeAnnotationRelationShape(_)) => "RelationShape",
        V::TypeAnnotation(TypeAnnotation::TypeAnnotationMultiplicityRef(_)) => "MultiplicityRef",
        V::TypeAnnotation(TypeAnnotation::TypeAnnotationWildcard(_)) => "Wildcard",
        V::Variable(_) => "Variable",
    }
}

/// `{"_type":…,"sourceInformation":…,"value":…}`, the value written by `rendered`.
pub(crate) fn literal(b: &mut Buf, ty: &str, pos: Option<&SourceInfo>, rendered: impl FnOnce(&mut Buf)) {
    b.a("{\"_type\":\"");
    b.a(ty);
    b.a("\",\"sourceInformation\":");
    src_info(b, pos);
    b.a(",\"value\":");
    rendered(b);
    b.ch(b'}');
}

pub(crate) fn c_string(b: &mut Buf, c: &CString, pos: Option<&SourceInfo>) {
    if c.multi_line {
        b.a("{\"_type\":\"string\",\"multiLine\":true,\"sourceInformation\":");
        src_info(b, pos);
        b.a(",\"value\":");
        str(b, &c.value);
        b.ch(b'}');
    } else {
        literal(b, "string", pos, |b| str(b, &c.value));
    }
}

pub(crate) fn c_float(b: &mut Buf, value: f64, pos: Option<&SourceInfo>) {
    literal(b, "float", pos, |b| b.a(&lite_javacompat::double_to_string(value)));
}

pub(crate) fn variable(b: &mut Buf, var: &Variable, pos: Option<&SourceInfo>) -> Em {
    require(
        var.r#type.is_none() == var.multiplicity.is_none(),
        "variable reference with a type or a multiplicity alone",
        &var.name.0,
    )?;
    b.a("{\"_type\":\"var\"");
    if let Some(t) = &var.r#type {
        b.a(",\"genericType\":");
        generic_type(b, t)?;
        b.a(",\"multiplicity\":");
        match &var.multiplicity {
            Some(m) => multiplicity(b, m)?,
            None => return Err(npe(None)),
        }
    }
    b.a(",\"name\":");
    str(b, &var.name);
    b.a(",\"sourceInformation\":");
    src_info(b, pos);
    b.ch(b'}');
    Ok(())
}

pub(crate) fn applied_property(b: &mut Buf, p: &AppliedProperty, pos: Option<&SourceInfo>) -> Em {
    b.a("{\"_type\":\"property\"");
    written_detail(b, "class", p.owner_class.as_ref());
    b.a(",\"parameters\":[");
    value_spec(b, &p.receiver)?;
    b.a("],\"property\":");
    str(b, &p.property);
    b.a(",\"sourceInformation\":");
    src_info(b, pos);
    b.ch(b'}');
    Ok(())
}

pub(crate) fn element_ptr(b: &mut Buf, full_path: &JStr, pos: Option<&SourceInfo>) {
    // the root package spelled '::' reaches the engine's serializer as a Java null: the wire carries literal null
    if *full_path == "::" {
        b.a("null");
        return;
    }
    b.a(if full_path.0.contains(&u16::from(b'~')) {
        "{\"_type\":\"unitType\",\"fullPath\":"
    } else {
        "{\"_type\":\"packageableElementPtr\",\"fullPath\":"
    });
    str(b, full_path);
    b.a(",\"sourceInformation\":");
    src_info(b, pos);
    b.ch(b'}');
}

pub(crate) fn enum_value(b: &mut Buf, e: &EnumValue, pos: Option<&SourceInfo>) {
    if e.node {
        b.a("{\"_type\":\"enumValue\",\"fullPath\":");
        str(b, &e.full_path);
        b.a(",\"sourceInformation\":");
        src_info(b, pos);
        b.a(",\"value\":");
        str(b, &e.value);
        b.ch(b'}');
        return;
    }
    b.a("{\"_type\":\"property\",\"parameters\":[{\"_type\":\"packageableElementPtr\",\"fullPath\":");
    str(b, &e.full_path);
    b.a(",\"sourceInformation\":");
    src_info(b, e.enumeration_pos.as_deref());
    b.a("}],\"property\":");
    str(b, &e.value);
    b.a(",\"sourceInformation\":");
    src_info(b, pos);
    b.ch(b'}');
}

pub(crate) fn latest_date(b: &mut Buf, pos: Option<&SourceInfo>) {
    b.a("{\"_type\":\"latestDate\",\"sourceInformation\":");
    src_info(b, pos);
    b.ch(b'}');
}

pub(crate) fn c_date(b: &mut Buf, d: &CDate, pos: Option<&SourceInfo>) -> Em {
    let Some(written) = &d.written else {
        return Err(unsupported(JStr::from(
            "ProtocolEmitter needs the verbatim source spelling of a date literal and the parser did not thread it.",
        )));
    };
    let day = matches!(d.value, PureDateLiteral::PureDateLiteralStrictDate(_));
    let month = matches!(d.value, PureDateLiteral::PureDateLiteralYearMonth(_));
    b.a(if day {
        "{\"_type\":\"strictDate\",\"sourceInformation\":"
    } else {
        "{\"_type\":\"dateTime\",\"sourceInformation\":"
    });
    src_info(b, pos);
    b.a(",\"value\":");
    if month {
        str(b, &jcat!("%", written));
    } else {
        str(b, written);
    }
    b.ch(b'}');
    Ok(())
}

pub(crate) fn type_annotation_named(b: &mut Buf, named: &TypeAnnotationNamed, pos: Option<&SourceInfo>) -> Em {
    b.a("{\"_type\":\"genericTypeInstance\",\"genericType\":");
    match &named.r#type {
        TypeExpression::TypeExpressionNameRef(un) if un.name.0.contains(&u16::from(b'~')) => {
            b.a("{\"multiplicityArguments\":[],\"rawType\":{\"_type\":\"packageableType\",\"fullPath\":");
            str(b, &un.name);
            b.a("},\"typeArguments\":[],\"typeVariableValues\":[]}");
            b.a(",\"sourceInformation\":");
            src_info(b, un.pos.as_deref());
        }
        t => {
            generic_type(b, t)?;
            b.a(",\"sourceInformation\":");
            src_info(b, pos);
        }
    }
    b.ch(b'}');
    Ok(())
}

pub(crate) fn relation_shape(b: &mut Buf, rs: &TypeAnnotationRelationShape) -> Em {
    let bare = rs.spelled_name.is_none();
    b.a("{\"_type\":\"genericTypeInstance\",\"genericType\":{\"multiplicityArguments\":[],");
    match &rs.spelled_name {
        None => b.a("\"rawType\":{\"_type\":\"relationType\",\"columns\":["),
        Some(spelled) => {
            b.a("\"rawType\":{\"_type\":\"packageableType\",\"fullPath\":");
            str(b, spelled);
            b.a(",\"sourceInformation\":");
            src_info(b, rs.type_span.as_deref());
            b.a("},\"typeArguments\":[{\"multiplicityArguments\":[],\"rawType\":{\"_type\":\"relationType\",\"columns\":[");
        }
    }
    for (i, col) in rs.columns.iter().enumerate() {
        if i > 0 {
            b.ch(b',');
        }
        let (TypeAnnotation::TypeAnnotationNamed(nt), Some(name)) = (&col.r#type, &col.name) else {
            return Err(unsupported(JStr::from(
                "ProtocolEmitter has no rule for wildcard @Relation columns (wire shape unprobed) \u{2014} probe, do not guess.",
            )));
        };
        b.a("{\"genericType\":");
        generic_type(b, &nt.r#type)?;
        b.a(",\"multiplicity\":");
        multiplicity(b, &col.multiplicity)?;
        b.a(",\"name\":");
        str(b, name);
        b.a(",\"sourceInformation\":");
        src_info(b, col.pos.as_deref());
        b.ch(b'}');
    }
    if bare {
        b.a("]},\"typeArguments\":[],\"typeVariableValues\":[]},\"sourceInformation\":");
    } else {
        b.a("]},\"typeArguments\":[],\"typeVariableValues\":[]}],\"typeVariableValues\":[]},\"sourceInformation\":");
    }
    src_info(b, rs.pos.as_deref());
    b.ch(b'}');
    Ok(())
}

/// `ProtocolEmitter.valueSpec`: the wire's value-specification encoding.
pub fn value_spec(b: &mut Buf, v: &ValueSpecification) -> Em {
    use ValueSpecification as V;
    match v {
        V::CBoolean(c) => literal(b, "boolean", c.pos.as_deref(), |b| b.boolean(c.value)),
        V::CByteArray(c) => {
            b.a("{\"_type\":\"byteArray\",\"sourceInformation\":");
            src_info(b, c.pos.as_deref());
            b.a(",\"value\":");
            str(b, &c.value);
            b.ch(b'}');
        }
        V::CInteger(c) => literal(b, "integer", c.pos.as_deref(), |b| append_value(b, &c.value)),
        V::CString(c) => c_string(b, c, c.pos.as_deref()),
        V::Variable(var) => variable(b, var, var.pos.as_deref())?,
        V::AppliedProperty(p) => applied_property(b, p, p.pos.as_deref())?,
        V::PureCollection(c) => collection(b, &c.values, c.pos.as_deref())?,
        V::AppliedFunction(f) => applied_function(b, f, None)?,
        V::CFloat(c) => c_float(b, c.value, c.pos.as_deref()),
        V::PackageableElementPtr(ptr) => element_ptr(b, &ptr.full_path, ptr.pos.as_deref()),
        V::EnumValue(e) => enum_value(b, e, e.pos.as_deref()),
        V::CLatestDate(l) => latest_date(b, l.pos.as_deref()),
        V::CDecimal(dec) => literal(b, "decimal", dec.pos.as_deref(), |b| b.a(&dec.value.to_java_string())),
        V::TypeAnnotation(TypeAnnotation::TypeAnnotationNamed(named)) => {
            type_annotation_named(b, named, named.pos.as_deref())?
        }
        V::TypeAnnotation(TypeAnnotation::TypeAnnotationRelationShape(rs)) => relation_shape(b, rs)?,
        V::NewInstance(ni) => new_instance(b, ni, None, None)?,
        V::ColumnInstance(ColumnInstance::ColSpec(cs)) => col_spec(b, cs, None)?,
        V::ColumnInstance(ColumnInstance::ColSpecArray(ca)) => col_spec_array(b, ca, ca.pos.as_deref())?,
        V::PathLiteral(pl) => path_literal(b, pl, None)?,
        V::SqlIsland(si) => sql_island(b, si, None),
        V::GqlIsland(gi) => gql_island(b, gi, None)?,
        V::TdsLiteral(tl) => tds_literal(b, tl, None),
        V::CTime(t) => {
            let written = quoted_written(t.written.as_ref(), "time literal")?;
            literal(b, "strictTime", t.pos.as_deref(), |b| b.units(&written.u));
        }
        V::GraphFetchLiteral(gf) => graph_fetch(b, gf, None)?,
        V::QuotedTreeCall(q) => applied_function(b, &q.original, None)?,
        V::QuotedGrammarCall(q) => applied_function(b, &q.original, None)?,
        V::LambdaFunction(lam) => lambda_at(b, lam, lam.pos.as_deref())?,
        V::CDate(d) => c_date(b, d, d.pos.as_deref())?,
        other => {
            return Err(unsupported(jcat!(
                "ProtocolEmitter has no rule for value specification ",
                simple_name(other),
                " \u{2014} add the emit rule, do not drop it."
            )));
        }
    }
    Ok(())
}

/// `ProtocolEmitter.lambda`: an inline lambda; an untyped parameter the bare `{"_type":"var","name":…}`, a typed one
/// with its type, multiplicity and span.
pub fn lambda(b: &mut Buf, lam: &LambdaFunction) -> Em {
    lambda_at(b, lam, lam.pos.as_deref())
}

pub(crate) fn lambda_at(b: &mut Buf, lam: &LambdaFunction, pos: Option<&SourceInfo>) -> Em {
    b.a("{\"_type\":\"lambda\",\"body\":[");
    comma_each(b, &lam.body, value_spec)?;
    b.a("],\"parameters\":[");
    for (i, p) in lam.parameters.iter().enumerate() {
        if i > 0 {
            b.ch(b',');
        }
        match &p.r#type {
            None => {
                b.a("{\"_type\":\"var\"");
                if let Some(m) = &p.multiplicity {
                    b.a(",\"multiplicity\":");
                    multiplicity(b, m)?;
                }
                b.a(",\"name\":");
                str(b, &p.name);
                b.ch(b'}');
            }
            Some(t) => {
                b.a("{\"_type\":\"var\",\"genericType\":");
                generic_type(b, t)?;
                b.a(",\"multiplicity\":");
                let Some(m) = &p.multiplicity else {
                    return Err(npe(Some(jcat!(
                        "typed lambda parameter without multiplicity: ",
                        p.name
                    ))));
                };
                multiplicity(b, m)?;
                b.a(",\"name\":");
                str(b, &p.name);
                b.a(",\"sourceInformation\":");
                src_info(b, p.pos.as_deref());
                b.ch(b'}');
            }
        }
    }
    b.ch(b']');
    // a walker-synthesized lambda is positionless on the engine's wire: the key omitted, not defaulted
    if let Some(pos) = pos {
        b.a(",\"sourceInformation\":");
        src_info(b, Some(pos));
    }
    b.ch(b'}');
    Ok(())
}

/// `ProtocolEmitter.valueSpecWithSpan`: a node with its top-level span replaced (the let-value rule).
pub(crate) fn value_spec_with_span(b: &mut Buf, v: &ValueSpecification, span: Option<&SourceInfo>) -> Em {
    use ValueSpecification as V;
    match v {
        V::CBoolean(c) => literal(b, "boolean", span, |b| b.boolean(c.value)),
        V::CInteger(c) => literal(b, "integer", span, |b| append_value(b, &c.value)),
        V::CString(c) => c_string(b, c, span),
        V::CFloat(c) => c_float(b, c.value, span),
        V::CDecimal(c) => literal(b, "decimal", span, |b| b.a(&c.value.to_java_string())),
        V::CDate(c) => c_date(b, c, span)?,
        V::Variable(var) => variable(b, var, span)?,
        V::PureCollection(c) => collection(b, &c.values, span)?,
        V::AppliedProperty(pr) => applied_property(b, pr, span)?,
        V::EnumValue(e) => enum_value(b, e, span),
        V::PackageableElementPtr(ptr) => element_ptr(b, &ptr.full_path, span),
        V::CLatestDate(_) => latest_date(b, span),
        V::ColumnInstance(ColumnInstance::ColSpecArray(ca)) => col_spec_array(b, ca, span)?,
        V::ColumnInstance(ColumnInstance::ColSpec(cs)) => col_spec(b, cs, span)?,
        // ^X(...) carries no span on the wire at all
        V::NewInstance(ni) => new_instance(b, ni, None, None)?,
        V::TypeAnnotation(TypeAnnotation::TypeAnnotationNamed(named)) => type_annotation_named(b, named, span)?,
        V::AppliedFunction(af) => applied_function(b, af, span)?,
        V::GraphFetchLiteral(gf) => graph_fetch(b, gf, span)?,
        V::QuotedTreeCall(q) => applied_function(b, &q.original, span)?,
        V::QuotedGrammarCall(q) => applied_function(b, &q.original, span)?,
        V::PathLiteral(pl) => path_literal(b, pl, span)?,
        V::SqlIsland(si) => sql_island(b, si, span),
        V::GqlIsland(gi) => gql_island(b, gi, span)?,
        V::TdsLiteral(tl) => tds_literal(b, tl, span),
        V::LambdaFunction(lam) => lambda_at(b, lam, span)?,
        other => {
            return Err(unsupported(jcat!(
                "ProtocolEmitter has no let-value span rule for ",
                simple_name(other),
                " \u{2014} probe, do not guess."
            )));
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------------------------------------------------
// Measures
// ---------------------------------------------------------------------------------------------------------------------

pub(crate) fn measure(b: &mut Buf, m: &PMeasure) -> Em {
    b.a("{\"_type\":\"measure\"");
    if let Some(u) = &m.canonical_unit {
        b.a(",\"canonicalUnit\":");
        unit(b, u)?;
    }
    b.a(",\"name\":");
    str(b, &m.name);
    b.a(",\"nonCanonicalUnits\":[");
    comma_each(b, &m.non_canonical_units, |b, u| unit(b, u))?;
    b.a("],\"package\":");
    str(b, &m.pkg);
    b.a(",\"sourceInformation\":");
    src_info(b, m.source_information.as_deref());
    b.ch(b'}');
    Ok(())
}

pub(crate) fn unit(b: &mut Buf, u: &PUnit) -> Em {
    b.ch(b'{');
    if let Some(body) = &u.body {
        b.a("\"conversionFunction\":{\"_type\":\"lambda\",\"body\":[");
        value_spec(b, body)?;
        b.a("],\"parameters\":[{\"_type\":\"var\",\"name\":");
        let Some(param) = &u.param_name else {
            return Err(npe(Some(JStr::from("unit param"))));
        };
        str(b, param);
        b.a("}]},");
    }
    b.a("\"measure\":");
    str(b, &u.measure_fqn);
    b.a(",\"name\":");
    str(b, &u.name);
    b.a(",\"sourceInformation\":");
    src_info(b, u.source_information.as_deref());
    b.ch(b'}');
    Ok(())
}

// ---------------------------------------------------------------------------------------------------------------------
// Calls
// ---------------------------------------------------------------------------------------------------------------------

/// `ProtocolEmitter.appliedFunction`: a call, a property call, a let, a `new`, a table-reference island.
pub(crate) fn applied_function(b: &mut Buf, f: &AppliedFunction, top_span_override: Option<&SourceInfo>) -> Em {
    use ValueSpecification as V;
    if f.property_call {
        let milestoned = f.parameters.iter().skip(1).any(|p| matches!(p, V::CLatestDate(_)));
        require(f.f_control.is_none(), "a property call with an fControl", &f.function.0)?;
        b.a("{\"_type\":\"property\"");
        written_detail(b, "class", f.owner_class.as_ref());
        b.a(",\"parameters\":[");
        for (i, p) in f.parameters.iter().enumerate() {
            if i > 0 {
                b.ch(b',');
            }
            if milestoned && matches!(p, V::CLatestDate(_)) {
                b.a("{\"_type\":\"latestDate\"}");
            } else {
                value_spec(b, p)?;
            }
        }
        b.a("],\"property\":");
        str(b, &f.function);
        b.a(",\"sourceInformation\":");
        src_info(b, top_span_override.or(f.pos.as_deref()));
        b.ch(b'}');
        return Ok(());
    }
    if f.function == "letFunction" {
        let first = match f.parameters.first() {
            Some(V::CString(s)) if f.parameters.len() == 2 => Some(s),
            _ => None,
        };
        require(
            first.is_some(),
            "malformed letFunction",
            &crate::ustr::u(&f.parameters.len().to_string()),
        )?;
        let name = first.expect("checked above");
        let let_span = f.pos.as_deref();
        b.a("{\"_type\":\"func\"");
        written_detail(b, "fControl", f.f_control.as_ref());
        b.a(",\"function\":\"letFunction\",\"parameters\":[{\"_type\":\"string\",\"value\":");
        str(b, &name.value);
        b.a("},");
        value_spec_with_span(b, &f.parameters[1], let_span)?;
        b.a("],\"sourceInformation\":");
        src_info(b, let_span);
        b.ch(b'}');
        return Ok(());
    }
    if f.function == "new"
        && let Some(V::NewInstance(ni)) = f.parameters.last()
    {
        return new_instance(b, ni, top_span_override, f.f_control.as_ref());
    }
    if f.island
        && f.parameters.len() == 1
        && let V::PackageableElementPtr(store) = &f.parameters[0]
    {
        let span = top_span_override.or(f.pos.as_deref());
        b.a("{\"_type\":\"classInstance\",\"sourceInformation\":");
        src_info(b, span);
        b.a(",\"type\":\">\",\"value\":{\"path\":[");
        str(b, &store.full_path);
        b.a("],\"sourceInformation\":");
        src_info(b, span);
        b.a("}}");
        return Ok(());
    }
    if f.island
        && f.parameters.len() == 2
        && let (V::PackageableElementPtr(db), V::CString(tbl)) = (&f.parameters[0], &f.parameters[1])
    {
        let span = top_span_override.or(f.pos.as_deref());
        b.a("{\"_type\":\"classInstance\",\"sourceInformation\":");
        src_info(b, span);
        b.a(",\"type\":\">\",\"value\":{\"path\":[");
        str(b, &db.full_path);
        let t = &tbl.value.0;
        let mut start = 0;
        while start <= t.len() {
            let dot = t[start..].iter().position(|&c| c == u16::from(b'.')).map(|d| start + d);
            b.ch(b',');
            str(b, &t[start..dot.unwrap_or(t.len())]);
            let Some(dot) = dot else {
                break;
            };
            start = dot + 1;
        }
        b.a("],\"sourceInformation\":");
        src_info(b, span);
        b.a("}}");
        return Ok(());
    }
    require(f.owner_class.is_none(), "a call with a class", &f.function.0)?;
    b.a("{\"_type\":\"func\"");
    written_detail(b, "fControl", f.f_control.as_ref());
    b.a(",\"function\":");
    str(b, &f.function);
    b.a(",\"parameters\":[");
    comma_each(b, &f.parameters, value_spec)?;
    b.a("],\"sourceInformation\":");
    src_info(b, top_span_override.or(f.pos.as_deref()));
    b.ch(b'}');
    Ok(())
}

/// A written detail of older JSON the record keeps (`fControl` on a call, `class` on a property).
pub(crate) fn written_detail(b: &mut Buf, key: &str, value: Option<&JStr>) {
    if let Some(v) = value {
        b.a(",\"");
        b.a(key);
        b.a("\":");
        str(b, v);
    }
}

/// `^X(k=v,…)` on the wire: `func "new"` with its `Class<X>` instance, an empty name and the key expressions; the
/// caret specials (`^Pair`, `^BasicColumnSpecification`, `^TdsOlapRank`) as their calls.
pub(crate) fn new_instance(b: &mut Buf, ni: &NewInstance, span: Option<&SourceInfo>, f_control: Option<&JStr>) -> Em {
    require(
        !ni.class_name.is_empty(),
        "new-instance on a variable receiver",
        &crate::ustr::u("^$x(...)"),
    )?;
    let special = crate::spec_parser::caret_special(ni).map_err(unsupported)?;
    if let Some(special) = special {
        require(
            f_control.is_none(),
            "a special class's new with an fControl",
            &ni.class_name.0,
        )?;
        return applied_function(b, &special, span);
    }
    b.a("{\"_type\":\"func\"");
    written_detail(b, "fControl", f_control);
    b.a(",\"function\":\"new\",\"parameters\":[{\"_type\":\"genericTypeInstance\",\"genericType\":{\"multiplicityArguments\":[],\"rawType\":{\"_type\":\"packageableType\",\"fullPath\":\"meta::pure::metamodel::type::Class\"},\"typeArguments\":[{\"multiplicityArguments\":[");
    for (i, m) in ni.type_multiplicity_arguments.iter().enumerate() {
        if i > 0 {
            b.ch(b',');
        }
        multiplicity(b, &parse_mult_arg(&m.0, &ni.class_name.0)?)?;
    }
    b.a("],\"rawType\":{\"_type\":\"packageableType\",\"fullPath\":");
    str(b, &ni.class_name);
    b.a("},\"typeArguments\":[");
    comma_each(b, &ni.type_arguments, generic_type)?;
    b.a("],\"typeVariableValues\":[]}],\"typeVariableValues\":[]}},{\"_type\":\"string\",\"value\":\"\"},{\"_type\":\"collection\",\"multiplicity\":{\"lowerBound\":");
    b.int(ni.properties.len() as i64);
    b.a(",\"upperBound\":");
    b.int(ni.properties.len() as i64);
    b.a("},\"values\":[");
    let mut first = true;
    for kb in &ni.properties {
        if !first {
            b.ch(b',');
        }
        first = false;
        require(!kb.expression.is_local, "local key expression", &kb.key.0)?;
        if let ValueSpecification::PackageableElementPtr(root) = &kb.expression.value
            && root.full_path == "::"
        {
            b.a("{\"_type\":\"keyExpression\",\"add\":false,\"key\":{\"_type\":\"string\",\"value\":");
            str(b, &kb.key);
            b.a("}}");
            continue;
        }
        placeless(
            !kb.key.0.contains(&u16::from(b'.')) || crate::ustr::starts_with(&kb.key.0, "'"),
            "a dotted key (the engine keys addr.city = .. by addr)",
            &kb.key.0,
        )?;
        b.a("{\"_type\":\"keyExpression\",\"add\":");
        b.boolean(kb.expression.is_add);
        b.a(",\"expression\":");
        value_spec(b, &kb.expression.value)?;
        b.a(",\"key\":{\"_type\":\"string\",\"value\":");
        str(b, &kb.key);
        b.a("}}");
    }
    b.a("]}]");
    if let Some(span) = span {
        b.a(",\"sourceInformation\":");
        src_info(b, Some(span));
    }
    b.ch(b'}');
    Ok(())
}

// ---------------------------------------------------------------------------------------------------------------------
// Path literals
// ---------------------------------------------------------------------------------------------------------------------

/// `#/Root/prop#` on the wire: a classInstance of type `path`, its spans shifted as the engine's island re-parse
/// shifts them; the outer span the let's when one overrides it.
pub(crate) fn path_literal(b: &mut Buf, pl: &PathLiteral, outer_override: Option<&SourceInfo>) -> Em {
    b.a("{\"_type\":\"classInstance\",\"sourceInformation\":");
    match outer_override {
        Some(s) => src_info(b, Some(s)),
        None => src_info(b, island_span(pl).as_ref()),
    }
    b.a(",\"type\":\"path\",\"value\":");
    path_value(b, pl)?;
    b.ch(b'}');
    Ok(())
}

/// A span inside a path literal as the engine writes it (its island re-lexed: the first line's columns shifted by the
/// literal's column and length).
pub(crate) fn shifted(pl: &PathLiteral, from: i32, to: i32) -> Option<SourceInfo> {
    let lit = pl.pos.as_deref()?;
    let start = island_position(pl, lit, from);
    let end = island_position(pl, lit, to);
    Some(SourceInfo {
        source_id: lit.source_id.clone(),
        start_line: start.0,
        start_column: start.1,
        end_line: end.0,
        end_column: end.1,
    })
}

/// The whole island's span: its first character to its end-of-input token, five columns past its end.
pub(crate) fn island_span(pl: &PathLiteral) -> Option<SourceInfo> {
    let lit = pl.pos.as_deref()?;
    let start = island_position(pl, lit, 0);
    let eof = island_position(pl, lit, pl.literal_length - 2);
    Some(SourceInfo {
        source_id: lit.source_id.clone(),
        start_line: start.0,
        start_column: start.1,
        end_line: eof.0,
        end_column: eof.1 + 4,
    })
}

/// The path value object alone (no classInstance wrapper); persistence graphFetch slots embed it directly.
pub fn path_value(b: &mut Buf, pl: &PathLiteral) -> Em {
    let outer = island_span(pl);
    b.ch(b'{');
    if let Some(alias) = &pl.alias {
        b.a("\"name\":");
        str(b, alias);
        b.ch(b',');
    }
    b.a("\"path\":[");
    for (i, seg) in pl.segments.iter().enumerate() {
        if i > 0 {
            b.ch(b',');
        }
        require(
            !seg.unsupported_arg,
            "dated path segment with a non-%latest argument",
            &seg.name.0,
        )?;
        b.a("{\"_type\":\"propertyPath\",\"parameters\":[");
        path_args(b, &seg.args, pl);
        b.a("],\"property\":");
        str(b, &seg.name);
        b.a(",\"sourceInformation\":");
        src_info(b, shifted(pl, seg.inner_start - 2, seg.inner_end - 1).as_ref());
        b.ch(b'}');
    }
    b.a("],\"sourceInformation\":");
    src_info(b, outer.as_ref());
    b.a(",\"startType\":");
    str(b, &pl.start_type);
    b.ch(b'}');
    Ok(())
}

/// Dated-segment arguments under the shifted-span rules; collections recurse.
pub(crate) fn path_args(b: &mut Buf, args: &[PathLiteralPathArg], pl: &PathLiteral) {
    for (a, arg) in args.iter().enumerate() {
        if a > 0 {
            b.ch(b',');
        }
        match arg {
            PathLiteralPathArg::PathLiteralPathArgLatest(r) => {
                b.a("{\"_type\":\"latestDate\",\"sourceInformation\":");
                src_info(b, shifted(pl, r.start - 1, r.end - 1).as_ref());
                b.ch(b'}');
            }
            PathLiteralPathArg::PathLiteralPathArgDateArg(r) => {
                b.a("{\"_type\":\"dateTime\",\"sourceInformation\":");
                src_info(b, shifted(pl, r.start - 1, r.end - 1).as_ref());
                b.a(",\"value\":");
                str(b, &r.value);
                b.ch(b'}');
            }
            PathLiteralPathArg::PathLiteralPathArgEnumArg(e) => {
                b.a("{\"_type\":\"enumValue\",\"fullPath\":");
                str(b, &e.full_path);
                b.a(",\"value\":");
                str(b, &e.value);
                b.ch(b'}');
            }
            PathLiteralPathArg::PathLiteralPathArgIntArg(n) => {
                b.a("{\"_type\":\"integer\",\"sourceInformation\":");
                src_info(b, shifted(pl, n.start - 1, n.end - 1).as_ref());
                b.a(",\"value\":");
                b.int(n.value);
                b.ch(b'}');
            }
            PathLiteralPathArg::PathLiteralPathArgStrArg(st) => {
                b.a("{\"_type\":\"string\",\"sourceInformation\":");
                src_info(b, shifted(pl, st.start - 1, st.end - 1).as_ref());
                b.a(",\"value\":");
                str(b, &st.value);
                b.ch(b'}');
            }
            PathLiteralPathArg::PathLiteralPathArgCollectionArg(col) => {
                b.a("{\"_type\":\"collection\",\"multiplicity\":{\"lowerBound\":");
                b.int(col.elements.len() as i64);
                b.a(",\"upperBound\":");
                b.int(col.elements.len() as i64);
                b.a("},\"values\":[");
                path_args(b, &col.elements, pl);
                b.a("]}");
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// Graph fetch trees
// ---------------------------------------------------------------------------------------------------------------------

/// `#{Root {a, k {b}}}#`: a classInstance of type `rootGraphFetchTree`, the `_type` key doubled as the engine writes
/// it; the outer span the let's when one overrides it.
pub(crate) fn graph_fetch(b: &mut Buf, gf: &GraphFetchLiteral, outer_span: Option<&SourceInfo>) -> Em {
    let pos = gf.pos.as_deref();
    b.a("{\"_type\":\"classInstance\",\"sourceInformation\":");
    src_info(b, outer_span.or(pos));
    b.a(",\"type\":\"rootGraphFetchTree\",\"value\":{\"_type\":\"rootGraphFetchTree\",\"_type\":\"rootGraphFetchTree\",\"class\":");
    str(b, &gf.class_name);
    b.a(",\"sourceInformation\":");
    src_info(b, pos);
    b.a(",\"subTrees\":[");
    graph_nodes(b, &gf.sub_trees)?;
    b.a("],\"subTypeTrees\":[");
    graph_sub_types(b, &gf.sub_type_trees)?;
    b.a("]}}");
    Ok(())
}

pub(crate) fn graph_sub_types(b: &mut Buf, sts: &[Box<GraphFetchLiteralSubTypeNode>]) -> Em {
    for (i, st) in sts.iter().enumerate() {
        if i > 0 {
            b.ch(b',');
        }
        b.a("{\"_type\":\"subTypeGraphFetchTree\",\"_type\":\"subTypeGraphFetchTree\",\"sourceInformation\":");
        src_info(b, st.pos.as_deref());
        b.a(",\"subTrees\":[");
        graph_nodes(b, &st.sub_trees)?;
        b.a("],\"subTypeClass\":");
        str(b, &st.sub_type_class);
        b.a(",\"subTypeTrees\":[]}");
    }
    Ok(())
}

pub(crate) fn graph_nodes(b: &mut Buf, nodes: &[Box<GraphFetchLiteralNode>]) -> Em {
    for (i, n) in nodes.iter().enumerate() {
        if i > 0 {
            b.ch(b',');
        }
        b.a("{\"_type\":\"propertyGraphFetchTree\",\"_type\":\"propertyGraphFetchTree\",");
        if let Some(alias) = &n.alias {
            b.a("\"alias\":");
            str(b, alias);
            b.ch(b',');
        }
        b.a("\"parameters\":[");
        // a node with no span of its own is refused where the first argument stands
        let first_argument = n.pos.as_deref().map_or((0, 0), |at| {
            (at.start_line, at.start_column + n.property.0.len() as i32 + 1)
        });
        comma_each(b, &n.parameters, |b, p| gft_param(b, p, first_argument))?;
        b.a("],\"property\":");
        str(b, &n.property);
        b.a(",\"sourceInformation\":");
        src_info(b, n.pos.as_deref());
        b.a(",\"subTrees\":[");
        graph_nodes(b, &n.sub_trees)?;
        b.ch(b']');
        if let Some(st) = &n.sub_type {
            b.a(",\"subType\":");
            str(b, st);
        }
        b.a(",\"subTypeTrees\":[]}");
    }
    Ok(())
}

/// A graph-fetch property argument in its graph-position wire spelling.
pub(crate) fn gft_param(b: &mut Buf, p: &ValueSpecification, first_argument: (i32, i32)) -> Em {
    use ValueSpecification as V;
    match p {
        V::CDate(d) => {
            b.a("{\"_type\":\"dateTime\",\"sourceInformation\":");
            src_info(b, d.pos.as_deref());
            b.a(",\"value\":");
            let Some(written) = &d.written else {
                return Err(npe(Some(JStr::from("graph-fetch date argument written form"))));
            };
            str(b, &jcat!("%", written));
            b.ch(b'}');
        }
        V::EnumValue(e) => {
            b.a("{\"_type\":\"enumValue\",\"fullPath\":");
            str(b, &e.full_path);
            b.a(",\"sourceInformation\":");
            let value = e.pos.as_deref();
            let enumeration = e.enumeration_pos.as_deref().or(value);
            match (value, enumeration) {
                (Some(value), Some(enumeration)) => src_info(
                    b,
                    Some(&SourceInfo {
                        source_id: value.source_id.clone(),
                        start_line: enumeration.start_line,
                        start_column: enumeration.start_column,
                        end_line: value.end_line,
                        end_column: value.end_column,
                    }),
                ),
                _ => src_info(b, None),
            }
            b.a(",\"value\":");
            str(b, &e.value);
            b.ch(b'}');
        }
        V::PureCollection(col) => {
            b.a("{\"_type\":\"collection\",\"multiplicity\":{\"lowerBound\":");
            b.int(col.values.len() as i64);
            b.a(",\"upperBound\":");
            b.int(col.values.len() as i64);
            b.a("},\"values\":[");
            comma_each(b, &col.values, |b, v| gft_param(b, v, first_argument))?;
            b.a("]}");
        }
        V::CString(_) | V::CInteger(_) | V::CBoolean(_) | V::CFloat(_) => value_spec(b, p)?,
        V::Variable(v) => {
            let at = v.pos.as_deref().map(|at| SourceInfo {
                source_id: at.source_id.clone(),
                start_line: at.start_line,
                start_column: at.start_column + 1,
                end_line: at.end_line,
                end_column: at.end_column,
            });
            variable(b, v, at.as_ref())?;
        }
        other => return Err(crate::protocol::graph_fetch_argument_refusal(other, first_argument)),
    }
    Ok(())
}

/// A literal's required written form, quoted for the wire.
pub(crate) fn quoted_written(written: Option<&JStr>, what: &str) -> Result<Buf, JavaError> {
    let Some(written) = written else {
        return Err(unsupported(jcat!(
            "ProtocolEmitter requires the source-written form for ",
            what,
            " \u{2014} synthesized nodes are not emittable."
        )));
    };
    let mut q = Buf::new();
    str(&mut q, written);
    Ok(q)
}

// ---------------------------------------------------------------------------------------------------------------------
// Column specifications and collections
// ---------------------------------------------------------------------------------------------------------------------

/// `~name` on the wire: a classInstance of type `colSpec`; the outer span the let's when one overrides it.
pub(crate) fn col_spec(b: &mut Buf, cs: &ColSpec, outer_override: Option<&SourceInfo>) -> Em {
    require(
        cs.alias.is_none() && cs.args.is_empty() && !cs.qualified,
        "colSpec with alias/args",
        &cs.name.0,
    )?;
    let pos = cs.pos.as_deref();
    b.a("{\"_type\":\"classInstance\",\"sourceInformation\":");
    src_info(b, outer_override.or(pos));
    b.a(",\"type\":\"colSpec\",\"value\":");
    col_spec_value(b, cs, pos)?;
    b.ch(b'}');
    Ok(())
}

pub(crate) fn col_spec_value(b: &mut Buf, cs: &ColSpec, pos: Option<&SourceInfo>) -> Em {
    b.ch(b'{');
    if let Some(f1) = &cs.function1 {
        b.a("\"function1\":");
        lambda(b, f1)?;
        b.ch(b',');
    }
    if let Some(f2) = &cs.function2 {
        b.a("\"function2\":");
        lambda(b, f2)?;
        b.ch(b',');
    }
    if let Some(t) = &cs.col_type {
        b.a("\"genericType\":");
        generic_type(b, t)?;
        b.ch(b',');
        if let Some(m) = &cs.col_type_mult {
            b.a("\"multiplicity\":");
            multiplicity(b, m)?;
            b.ch(b',');
        }
    }
    b.a("\"name\":");
    str(b, &cs.name);
    b.a(",\"sourceInformation\":");
    src_info(b, pos);
    if !cs.stereotypes.is_empty() {
        b.a(",\"stereotypes\":");
        stereotypes(b, &cs.stereotypes);
    }
    if !cs.tagged_values.is_empty() {
        b.a(",\"taggedValues\":");
        tagged_values(b, &cs.tagged_values);
    }
    b.ch(b'}');
    Ok(())
}

/// `~[a, b]`: a classInstance of type `colSpecArray`.
pub(crate) fn col_spec_array(b: &mut Buf, ca: &ColSpecArray, pos: Option<&SourceInfo>) -> Em {
    b.a("{\"_type\":\"classInstance\",\"sourceInformation\":");
    src_info(b, pos);
    b.a(",\"type\":\"colSpecArray\",\"value\":{\"colSpecs\":[");
    for (i, cs) in ca.col_specs.iter().enumerate() {
        if i > 0 {
            b.ch(b',');
        }
        col_spec_value(b, cs, cs.pos.as_deref())?;
    }
    b.a("]}}");
    Ok(())
}

/// `{"_type":"collection","multiplicity":{n,n},"sourceInformation":…,"values":[…]}`, the span key only with a span.
pub(crate) fn collection(b: &mut Buf, values: &[ValueSpecification], pos: Option<&SourceInfo>) -> Em {
    b.a("{\"_type\":\"collection\",\"multiplicity\":{\"lowerBound\":");
    b.int(values.len() as i64);
    b.a(",\"upperBound\":");
    b.int(values.len() as i64);
    b.ch(b'}');
    if let Some(pos) = pos {
        b.a(",\"sourceInformation\":");
        src_info(b, Some(pos));
    }
    b.a(",\"values\":[");
    comma_each(b, values, value_spec)?;
    b.a("]}");
    Ok(())
}

/// A ModelStore `instances` collection: the engine builds the node in its data walker, spanless, its leaves keeping
/// theirs.
pub fn instances_collection(b: &mut Buf, instances: &ValueSpecification) -> Em {
    if let ValueSpecification::PureCollection(c) = instances
        && c.pos.is_none()
    {
        return spanless_instances(b, &c.values);
    }
    data_instance(b, instances)
}

pub(crate) fn spanless_instances(b: &mut Buf, values: &[ValueSpecification]) -> Em {
    b.a("{\"_type\":\"collection\",\"multiplicity\":{\"lowerBound\":");
    b.int(values.len() as i64);
    b.a(",\"upperBound\":");
    b.int(values.len() as i64);
    b.a("},\"values\":[");
    comma_each(b, values, data_instance)?;
    b.a("]}");
    Ok(())
}

/// A non-instance leaf inside ModelStore data: an enum access a real `enumValue` node.
pub(crate) fn data_leaf(b: &mut Buf, v: &ValueSpecification) -> Em {
    use ValueSpecification as V;
    match v {
        V::EnumValue(e) => {
            enum_value_node(
                b,
                &e.full_path,
                &e.value,
                join_spans(e.enumeration_pos.as_deref(), e.pos.as_deref()),
            );
            Ok(())
        }
        V::AppliedProperty(p) if matches!(p.receiver, V::PackageableElementPtr(_)) => {
            let V::PackageableElementPtr(ptr) = &p.receiver else {
                unreachable!("matched above")
            };
            enum_value_node(
                b,
                &ptr.full_path,
                &p.property,
                join_spans(ptr.pos.as_deref(), p.pos.as_deref()),
            );
            Ok(())
        }
        _ => value_spec(b, v),
    }
}

/// One span running from `from`'s start to `to`'s end.
pub(crate) fn join_spans(from: Option<&SourceInfo>, to: Option<&SourceInfo>) -> Option<SourceInfo> {
    let (from, to) = (from?, to?);
    Some(SourceInfo {
        source_id: from.source_id.clone(),
        start_line: from.start_line,
        start_column: from.start_column,
        end_line: to.end_line,
        end_column: to.end_column,
    })
}

pub(crate) fn enum_value_node(b: &mut Buf, full_path: &JStr, value: &JStr, span: Option<SourceInfo>) {
    b.a("{\"_type\":\"enumValue\",\"fullPath\":");
    str(b, full_path);
    b.a(",\"sourceInformation\":");
    src_info(b, span.as_ref());
    b.a(",\"value\":");
    str(b, value);
    b.ch(b'}');
}

/// One instance inside ModelStore data, as the engine's data walker builds its `new` call.
pub(crate) fn data_instance(b: &mut Buf, v: &ValueSpecification) -> Em {
    use ValueSpecification as V;
    let mut node = v;
    if let V::AppliedFunction(f) = node
        && f.function == "new"
        && let Some(last @ V::NewInstance(_)) = f.parameters.last()
    {
        node = last;
    }
    let V::NewInstance(ni) = node else {
        return data_leaf(b, node);
    };
    b.a("{\"_type\":\"func\",\"function\":\"new\",\"parameters\":[{\"_type\":\"packageableElementPtr\",\"fullPath\":");
    str(b, &ni.class_name);
    b.a("},{\"_type\":\"string\",\"value\":\"dummy\"},{\"_type\":\"collection\",\"multiplicity\":{\"lowerBound\":");
    b.int(ni.properties.len() as i64);
    b.a(",\"upperBound\":");
    b.int(ni.properties.len() as i64);
    b.a("},\"values\":[");
    let mut first = true;
    for kb in &ni.properties {
        if !first {
            b.ch(b',');
        }
        first = false;
        require(!kb.expression.is_local, "local key expression", &kb.key.0)?;
        require(!kb.expression.is_add, "'+=' in model store data", &kb.key.0)?;
        b.a("{\"_type\":\"keyExpression\",\"add\":false,\"expression\":");
        // instancesCollection over a new spanless collection of the value's elements (or the value alone)
        match &kb.expression.value {
            V::PureCollection(kc) => spanless_instances(b, &kc.values)?,
            kv => spanless_instances(b, std::slice::from_ref(kv))?,
        }
        b.a(",\"key\":{\"_type\":\"string\",\"value\":");
        str(b, &kb.key);
        b.a("}}");
    }
    b.a("]}]}");
    Ok(())
}

// ---------------------------------------------------------------------------------------------------------------------
// The section index
// ---------------------------------------------------------------------------------------------------------------------

pub(crate) fn section_index(b: &mut Buf, s: &PSectionIndex) {
    b.a("{\"_type\":\"sectionIndex\",\"name\":");
    str(b, &s.name);
    b.a(",\"package\":");
    str(b, &s.pkg);
    b.a(",\"sections\":[");
    for (i, sec) in s.sections.iter().enumerate() {
        if i > 0 {
            b.ch(b',');
        }
        section(b, sec);
    }
    b.a("]}");
}

pub(crate) fn section(b: &mut Buf, s: &PSection) {
    b.a(if s.import_aware {
        "{\"_type\":\"importAware\""
    } else {
        "{\"_type\":\"default\""
    });
    b.a(",\"elements\":[");
    for (i, e) in s.elements.iter().enumerate() {
        if i > 0 {
            b.ch(b',');
        }
        match e {
            // the BigQuery deployment-config walker registers null
            None => b.a("null"),
            Some(e) => str(b, e),
        }
    }
    b.ch(b']');
    if s.import_aware {
        b.a(",\"imports\":[");
        strs_each(b, &s.imports);
        b.ch(b']');
    }
    b.a(",\"parserName\":");
    str(b, &s.parser_name);
    b.a(",\"sourceInformation\":");
    src_info(b, s.source_information.as_deref());
    b.ch(b'}');
}

#[cfg(test)]
mod tests {
    use super::*;

    fn units(s: &str) -> Vec<u16> {
        s.encode_utf16().collect()
    }

    #[test]
    fn str_escapes_as_jackson_does() {
        let mut b = Buf::new();
        str(&mut b, &[u16::from(b'a'), 0x22, 0x5C, 0x0A, 0x01, 0x1F, 0xD800, 0xE9]);
        let mut want = units("\"a\\\"\\\\\\n\\u0001\\u001F");
        want.extend([0xD800, 0xE9, u16::from(b'"')]);
        assert_eq!(b.u, want);
    }

    #[test]
    fn finish_takes_out_an_absent_span_and_one_comma() {
        let mut b = Buf::new();
        b.a("{\"a\":1,\"sourceInformation\":");
        src_info(&mut b, None);
        b.a(",\"b\":2}");
        assert_eq!(finish(b), units("{\"a\":1,\"b\":2}"));
        let mut b = Buf::new();
        b.a("{\"sourceInformation\":");
        src_info(&mut b, None);
        b.a(",\"b\":2}");
        assert_eq!(finish(b), units("{\"b\":2}"));
    }

    #[test]
    fn ints_as_java_appends_them() {
        let mut b = Buf::new();
        b.int(0);
        b.ch(b' ');
        b.int(-42);
        b.ch(b' ');
        b.int(i64::MIN);
        assert_eq!(b.u, units("0 -42 -9223372036854775808"));
    }
}
