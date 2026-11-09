// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! `###MongoDB` (core/'s `parser.section.MongoDBSectionGrammar`, docs/rust-port/PHASE2.md §4 step 4d): `Database qn (
//! Collection name ( validationLevel: ..; validationAction: ..; jsonSchema: {...}; ) ... )` -- includes and joins read
//! and dropped, the collection's keys in their grammar's order, the `jsonSchema` island walked as tokens into Java's
//! JSON values (`LinkedHashMap`, `ArrayList`, `String`, `Long`, `Boolean`, the null sentinel) and mapped to typed BSON
//! schema nodes by Java's casts, each `ClassCastException` as the JVM words it. Java's answers reproduced, each refusal
//! at Java's position.

// The generated records hold their record-typed list elements boxed (`Vec<Box<T>>`), so the readers build that.
#![allow(clippy::vec_box)]

use crate::cursor::{Cursor, R, once, refused_at};
use crate::doors::JavaError;
use crate::jcat;
use crate::jnum::parse_long;
use crate::jtypes::JStr;
use crate::lexer::TokenType;
use crate::records::{PBsonSchema, PMongoDatabase, PMongoDatabasePMongoCollection};
use crate::sections::{head, linked_put};
use crate::ustr::eq;
use std::collections::HashSet;

use TokenType as T;

/// A JSON value as `parseJsonValue` builds it, by its Java class.
#[derive(Clone, Debug, PartialEq)]
enum Json {
    /// `java.util.LinkedHashMap<String, Object>`: insertion order, a repeated key's last value in its first place.
    Map(Vec<(Vec<u16>, Json)>),
    /// `java.util.ArrayList`.
    List(Vec<Json>),
    /// `java.lang.String`.
    Str(Vec<u16>),
    /// `java.lang.Long`.
    Long(i64),
    /// `java.lang.Boolean`.
    Bool(bool),
    /// The `JSON_NULL` sentinel, a bare `java.lang.Object`.
    Null,
}

impl Json {
    fn class(&self) -> &'static str {
        match self {
            Json::Map(_) => "java.util.LinkedHashMap",
            Json::List(_) => "java.util.ArrayList",
            Json::Str(_) => "java.lang.String",
            Json::Long(_) => "java.lang.Long",
            Json::Bool(_) => "java.lang.Boolean",
            Json::Null => "java.lang.Object",
        }
    }
}

/// `Map.get(key)` over a `LinkedHashMap`'s entries.
fn get<'a>(m: &'a [(Vec<u16>, Json)], key: &str) -> Option<&'a Json> {
    m.iter().find(|(k, _)| eq(k, key)).map(|(_, v)| v)
}

/// A checked cast's `ClassCastException`: both classes in `java.base`, of the bootstrap loader.
fn class_cast(v: &Json, to: &str) -> JavaError {
    let from = v.class();
    JavaError::java(
        "java.lang.ClassCastException",
        JStr::from(format!(
            "class {from} cannot be cast to class {to} ({from} and {to} are in module java.base of loader 'bootstrap')"
        )),
    )
}

/// `(String) v`.
fn as_string(v: Option<&Json>) -> R<Option<Vec<u16>>> {
    match v {
        None => Ok(None),
        Some(Json::Str(s)) => Ok(Some(s.clone())),
        Some(other) => Err(class_cast(other, "java.lang.String")),
    }
}

/// `(Long) v`.
fn as_long(v: Option<&Json>) -> R<Option<i64>> {
    match v {
        None => Ok(None),
        Some(Json::Long(n)) => Ok(Some(*n)),
        Some(other) => Err(class_cast(other, "java.lang.Long")),
    }
}

/// `(Map<?, ?>) v` (never null where Java casts).
fn as_map(v: &Json) -> R<&[(Vec<u16>, Json)]> {
    match v {
        Json::Map(m) => Ok(m),
        other => Err(class_cast(other, "java.util.Map")),
    }
}

/// `MongoDBSectionGrammar.parseOne`: `Database qn ( include qn | Join name (...) | Collection ... )*`.
pub fn parse_database<'s>(c: &mut impl Cursor<'s>) -> R<PMongoDatabase> {
    let h = head(c, "Database", false)?;
    c.expect(T::PAREN_OPEN)?;
    let mut collections = Vec::new();
    while !c.at_end() && c.peek() != T::PAREN_CLOSE {
        let ck = c.parse_identifier()?;
        if eq(&ck, "include") {
            c.parse_qualified_name()?;
            continue;
        }
        if eq(&ck, "Join") {
            c.parse_identifier()?;
            c.skip_balanced_block()?;
            continue;
        }
        if !eq(&ck, "Collection") {
            return Err(c.error(jcat!("expected Collection, got ", ck)));
        }
        collections.push(Box::new(parse_collection(c)?));
    }
    c.expect(T::PAREN_CLOSE)?;
    Ok(PMongoDatabase {
        pkg: JStr(h.pkg),
        name: JStr(h.name),
        collections,
        source_information: c.span(h.decl_start, c.pos() - 1),
    })
}

/// `parseCollection`: a `"quoted"` name (its quotes off) or an identifier, then its keys once each in the grammar's
/// order; the level and action defaulting to the wire's `strict` and `error`, a `jsonSchema` required.
fn parse_collection<'s>(c: &mut impl Cursor<'s>) -> R<PMongoDatabasePMongoCollection> {
    let coll_name = if c.peek() == T::QUOTED_STRING {
        let raw = c.text().to_vec();
        c.expect(T::QUOTED_STRING)?;
        let dq = u16::from(b'"');
        if raw.len() >= 2 && raw[0] == dq && raw[raw.len() - 1] == dq { raw[1..raw.len() - 1].to_vec() } else { raw }
    } else {
        c.parse_identifier()?
    };
    c.expect(T::PAREN_OPEN)?;
    let mut level = None;
    let mut action = None;
    let mut schema = None;
    let mut seen: HashSet<Vec<u16>> = HashSet::new();
    let mut last_rank = -1;
    while c.peek() != T::PAREN_CLOSE {
        let key = c.parse_identifier()?;
        once(&mut seen, &key, c)?;
        c.expect(T::COLON)?;
        let rank = if eq(&key, "validationLevel") {
            0
        } else if eq(&key, "validationAction") {
            1
        } else if eq(&key, "jsonSchema") {
            2
        } else {
            return Err(c.error(jcat!("unknown Collection key '", key, "'")));
        };
        if rank < last_rank {
            return Err(c.error(jcat!("Unexpected token '", key, "'")));
        }
        last_rank = rank;
        match rank {
            0 => level = Some(c.parse_identifier()?),
            1 => action = Some(c.parse_identifier()?),
            _ => schema = Some(parse_schema_island(c)?),
        }
        c.expect(T::SEMI_COLON)?;
    }
    c.expect(T::PAREN_CLOSE)?;
    let level = level.unwrap_or_else(|| crate::ustr::u("strict"));
    let action = action.unwrap_or_else(|| crate::ustr::u("error"));
    let Some(schema) = schema else {
        return Err(c.error(jcat!("Collection '", coll_name, "' needs jsonSchema")));
    };
    Ok(PMongoDatabasePMongoCollection {
        name: JStr(coll_name),
        validation_level: JStr(level),
        validation_action: JStr(action),
        schema: Box::new(schema),
    })
}

/// `parseSchemaIsland`: the JSON value at the cursor, an object whose root `bsonType` (if any) is `object`.
fn parse_schema_island<'s>(c: &mut impl Cursor<'s>) -> R<PBsonSchema> {
    let mut past_long = None;
    let parsed = parse_json_value(c, &mut past_long)?;
    let Json::Map(m) = &parsed else {
        return Err(c.error_str("jsonSchema is not a JSON object"));
    };
    if let Some(digits) = past_long {
        return Err(judged(m, c, JavaError::java("java.lang.NumberFormatException", digits)));
    }
    if let Some(root_type) = get(m, "bsonType")
        && !matches!(root_type, Json::Str(s) if eq(s, "object"))
    {
        return Err(c.error_str("jsonSchema root must be an object"));
    }
    schema_of(m, true, c).map_err(|e| if e.class.starts_with("java.") { judged(m, c, e) } else { e })
}

/// A JDK exception `schema_of` raised (PARITY row 50) against what legend-engine's reader does with the same schema: a
/// refusal where it refuses, placed by its section catch-all; where it accepts, the answer stays (the user's list,
/// ruling 4 of docs/rust-port/PHASE2.md section 6).
fn judged<'s>(root: &[(Vec<u16>, Json)], at: &impl Cursor<'s>, raw: JavaError) -> JavaError {
    match engine_refusal(root) {
        Some(message) => at.section_refusal(message),
        None => raw,
    }
}

const NOT_A_SCHEMA_NODE: &str = "a jsonSchema node must be an object with a bsonType";

/// The refusal of legend-engine's `MongoDBSchemaDeserializer.getSchema` for a schema, read from its source: the root
/// node needs a `bsonType` of `object`, and every node it visits (a property's, an array's items, each visited only
/// under an object or an array) a `bsonType` it knows. Lite's wording for the engine's `NullPointerException`.
fn engine_refusal(root: &[(Vec<u16>, Json)]) -> Option<JStr> {
    match get(root, "bsonType") {
        None => Some(JStr::from(NOT_A_SCHEMA_NODE)),
        Some(Json::Str(s)) if eq(s, "object") => object_fields_refusal(root),
        Some(_) => Some(JStr::from("Expected jsonSchema bsonType should be of object type")),
    }
}

/// `processObjectTypeFields`: the properties, and `additionalProperties` when it is a schema of its own.
fn object_fields_refusal(m: &[(Vec<u16>, Json)]) -> Option<JStr> {
    if let Some(Json::Map(properties)) = get(m, "properties") {
        for (_, value) in properties {
            if let Some(refusal) = node_refusal(value) {
                return Some(refusal);
            }
        }
    }
    match get(m, "additionalProperties") {
        Some(Json::Map(additional)) => object_fields_refusal(additional),
        _ => None,
    }
}

/// `getBsonNodeType`.
fn node_refusal(v: &Json) -> Option<JStr> {
    let Json::Map(m) = v else {
        return Some(JStr::from(NOT_A_SCHEMA_NODE));
    };
    let Some(bson_type) = get(m, "bsonType") else {
        return Some(JStr::from(NOT_A_SCHEMA_NODE));
    };
    let Json::Str(name) = bson_type else {
        return Some(JStr::from("Type not supported in jsonSchema model yet: null"));
    };
    match String::from_utf16_lossy(name).as_str() {
        "object" => object_fields_refusal(m),
        "array" => match get(m, "items") {
            None => None,
            Some(Json::List(items)) => items.iter().find_map(node_refusal),
            Some(item) => node_refusal(item),
        },
        "double" | "string" | "binData" | "objectId" | "bool" | "date" | "int" | "timestamp" | "long" | "decimal"
        | "minKey" | "maxKey" => None,
        _ => Some(jcat!("Type not supported in jsonSchema model yet: ", name)),
    }
}

/// A `"quoted"` token's text with its first and last unit off (`raw.substring(1, raw.length() - 1)`); a `"` left open
/// at the end of the input refused there, at the quote (PARITY row 52).
fn inner<'s>(raw: &[u16], c: &impl Cursor<'s>, at: usize) -> R<Vec<u16>> {
    if raw.len() < 2 {
        return Err(refused_at(c.tokens(), at, jcat!("Unexpected token '", raw, "'")));
    }
    Ok(raw[1..raw.len() - 1].to_vec())
}

/// `parseJsonValue`: an object, an array, a `"string"`, an integer (`Long.valueOf`, unguarded), a negative one, a
/// boolean or `null`.
fn parse_json_value<'s>(c: &mut impl Cursor<'s>, past_long: &mut Option<JStr>) -> R<Json> {
    match c.peek() {
        T::BRACE_OPEN => {
            c.advance()?;
            let mut m: Vec<(Vec<u16>, Json)> = Vec::new();
            while c.peek() != T::BRACE_CLOSE {
                let raw = c.slack_text();
                let key_at = c.pos();
                c.expect(T::QUOTED_STRING)?;
                c.expect(T::COLON)?;
                let key = inner(raw, c, key_at)?;
                let v = parse_json_value(c, past_long)?;
                linked_put(&mut m, key, v);
                if !c.match_tok(T::COMMA)? {
                    break;
                }
            }
            c.expect(T::BRACE_CLOSE)?;
            Ok(Json::Map(m))
        }
        T::BRACKET_OPEN => {
            c.advance()?;
            let mut l = Vec::new();
            while c.peek() != T::BRACKET_CLOSE {
                l.push(parse_json_value(c, past_long)?);
                if !c.match_tok(T::COMMA)? {
                    break;
                }
            }
            c.expect(T::BRACKET_CLOSE)?;
            Ok(Json::List(l))
        }
        T::QUOTED_STRING => {
            let raw = c.text();
            let at = c.pos();
            c.advance()?;
            Ok(Json::Str(inner(raw, c, at)?))
        }
        T::INTEGER => {
            let t = c.text();
            c.advance()?;
            Ok(Json::Long(long_value_of(t, past_long)))
        }
        T::MINUS => {
            c.advance()?;
            let t = c.slack_text();
            c.expect(T::INTEGER)?;
            let mut s = crate::ustr::u("-");
            s.extend_from_slice(t);
            Ok(Json::Long(long_value_of(&s, past_long)))
        }
        T::TRUE => {
            c.advance()?;
            Ok(Json::Bool(true))
        }
        T::FALSE => {
            c.advance()?;
            Ok(Json::Bool(false))
        }
        _ => {
            if eq(&c.safe_text(), "null") {
                c.advance()?;
                return Ok(Json::Null);
            }
            Err(c.error(jcat!("unsupported jsonSchema value: ", c.safe_text())))
        }
    }
}

/// `Long.valueOf(s)`, its failure (the first one met, as Java's) kept for [`judged`]: the whole schema is read first,
/// since the engine's verdict on it is the schema's.
fn long_value_of(s: &[u16], past_long: &mut Option<JStr>) -> i64 {
    parse_long(s).unwrap_or_else(|m| {
        past_long.get_or_insert(m);
        0
    })
}

/// `schemaOf(m, root, at)`: a JSON object as a BSON schema node -- its type by `bsonType` (`object` when absent), its
/// properties, required names, items, the object flag, title, description and lengths, each read by Java's cast in
/// Java's order.
fn schema_of<'s>(m: &[(Vec<u16>, Json)], root: bool, at: &impl Cursor<'s>) -> R<PBsonSchema> {
    let bson_type = as_string(get(m, "bsonType"))?;
    let wire_type = match bson_type.as_deref().map(String::from_utf16_lossy).as_deref() {
        None | Some("object") => {
            if root {
                "schema"
            } else {
                "objectType"
            }
        }
        Some("string") => "stringType",
        Some("long") => "longType",
        Some("int") => "intType",
        Some("bool") => "boolType",
        Some("double") => "doubleType",
        Some("decimal") => "decimalType",
        Some("array") => "arrayType",
        Some("objectId") => "objectIdType",
        Some(_) => {
            return Err(at.error(jcat!("unmapped bsonType '", bson_type.unwrap_or_default(), "'")));
        }
    };
    let mut props = Vec::new();
    if let Some(Json::Map(pm)) = get(m, "properties") {
        for (k, v) in pm {
            props.push((JStr(k.clone()), Box::new(schema_of(as_map(v)?, false, at)?)));
        }
    }
    let mut required = Vec::new();
    if let Some(Json::List(rl)) = get(m, "required") {
        for r in rl {
            match r {
                Json::Str(s) => required.push(JStr(s.clone())),
                other => return Err(class_cast(other, "java.lang.String")),
            }
        }
    }
    let addl = get(m, "additionalProperties");
    let items = match get(m, "items") {
        Some(Json::Map(one)) => Some(vec![Box::new(schema_of(one, false, at)?)]),
        Some(Json::List(many)) => {
            let mut out = Vec::new();
            for o in many {
                out.push(Box::new(schema_of(as_map(o)?, false, at)?));
            }
            Some(out)
        }
        _ => None,
    };
    let object_node = wire_type == "schema" || wire_type == "objectType";
    let title = as_string(get(m, "title"))?;
    let description = as_string(get(m, "description"))?;
    let min_length = as_long(get(m, "minLength"))?;
    let max_length = as_long(get(m, "maxLength"))?;
    Ok(PBsonSchema {
        wire_type: JStr::from(wire_type),
        additional_properties_allowed: object_node && matches!(addl, Some(Json::Bool(true))),
        properties: props,
        required,
        title: title.map(JStr),
        description: description.map(JStr),
        min_length,
        max_length,
        items,
    })
}
