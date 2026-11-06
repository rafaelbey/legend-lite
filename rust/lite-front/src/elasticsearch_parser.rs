// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! `###Elasticsearch` (core/'s `parser.section.ElasticsearchSectionGrammar`, docs/rust-port/PHASE2.md §4 step 4d):
//! `Elasticsearch7Cluster qn { indices: [ name: { properties: [ p: Keyword ]; } ]; }` -- each property's wire triple by
//! its type, a scalar's optional `fields` body, a complex one's required `properties`. Java's answers reproduced, each
//! refusal at Java's position.

// The generated records hold their record-typed list elements boxed (`Vec<Box<T>>`), so the readers build that.
#![allow(clippy::vec_box)]

use crate::cursor::{Cursor, R, once, throw_at};
use crate::jcat;
use crate::jtypes::JStr;
use crate::lexer::TokenType;
use crate::records::{PElasticsearch7Cluster, PElasticsearch7ClusterPEsIndex, PElasticsearch7ClusterPEsProperty};
use crate::sections::{head, string_value};
use crate::ustr::eq;
use std::collections::HashSet;

use TokenType as T;

type Property = PElasticsearch7ClusterPEsProperty;

/// `ElasticsearchSectionGrammar.parseOne`: one cluster, `indices` required (at the declaration).
pub fn parse_cluster<'s>(c: &mut impl Cursor<'s>) -> R<PElasticsearch7Cluster> {
    let h = head(c, "Elasticsearch7Cluster", false)?;
    c.expect(T::BRACE_OPEN)?;
    let mut indices: Option<Vec<Box<PElasticsearch7ClusterPEsIndex>>> = None;
    let mut seen: HashSet<Vec<u16>> = HashSet::new();
    while !c.at_end() && c.peek() != T::BRACE_CLOSE {
        let key = c.parse_identifier()?;
        once(&mut seen, &key, c)?;
        c.expect(T::COLON)?;
        if !eq(&key, "indices") {
            return Err(c.error(jcat!("unknown Elasticsearch7Cluster key '", key, "'")));
        }
        let mut list = Vec::new();
        c.expect(T::BRACKET_OPEN)?;
        loop {
            list.push(Box::new(parse_index(c)?));
            if !c.match_tok(T::COMMA)? {
                break;
            }
        }
        indices = Some(list);
        c.expect(T::BRACKET_CLOSE)?;
        c.expect(T::SEMI_COLON)?;
    }
    c.expect(T::BRACE_CLOSE)?;
    let Some(indices) = indices else {
        return Err(throw_at(c.tokens(), h.decl_start, JStr::from("Field 'indices' is required")));
    };
    Ok(PElasticsearch7Cluster {
        pkg: JStr(h.pkg),
        name: JStr(h.name),
        indices,
        source_information: c.span(h.decl_start, c.pos() - 1),
    })
}

/// The wire triple (key, protocol type, Elasticsearch type) of a property type.
fn triple(kind: &[u16]) -> Option<[&'static str; 3]> {
    Some(match String::from_utf16_lossy(kind).as_str() {
        "Keyword" => ["keyword", "keywordProperty", "keyword"],
        "Text" => ["text", "textProperty", "text"],
        "Date" => ["date", "dateProperty", "date"],
        "Short" => ["_short", "shortNumberProperty", "short"],
        "Byte" => ["_byte", "byteNumberProperty", "byte"],
        "Integer" => ["integer", "integerNumberProperty", "integer"],
        "Long" => ["_long", "longNumberProperty", "long"],
        "Float" => ["_float", "floatNumberProperty", "float"],
        "HalfFloat" => ["half_float", "halfFloatNumberProperty", "half_float"],
        "Double" => ["_double", "doubleNumberProperty", "double"],
        "Boolean" => ["_boolean", "booleanProperty", "boolean"],
        "Object" => ["object", "objectProperty", "object"],
        "Nested" => ["nested", "nestedProperty", "nested"],
        _ => return None,
    })
}

/// `parseProperty`: `name: Type`, a scalar's optional `{ fields: [...]; }`, a complex type's (`Object`, `Nested`)
/// required `{ properties: [...]; }`; a nested list's duplicate name refused (the walker's `Collectors.toMap`).
fn parse_property<'s>(c: &mut impl Cursor<'s>) -> R<Property> {
    let prop_name = name_or_string(c)?;
    c.expect(T::COLON)?;
    let kind_tok = c.pos();
    let kind = c.parse_identifier()?;
    let complex = eq(&kind, "Object") || eq(&kind, "Nested");
    let Some(t) = triple(&kind) else {
        return Err(c.error(jcat!("unmapped Elasticsearch property type '", kind, "'")));
    };
    let mut fields = None;
    let mut children = None;
    if complex && c.peek() != T::BRACE_OPEN {
        return Err(throw_at(c.tokens(), kind_tok, jcat!(kind, " property requires a { properties: [...]; } body")));
    }
    if c.match_tok(T::BRACE_OPEN)? {
        let want = if complex { "properties" } else { "fields" };
        let mut seen_body = false;
        while !c.at_end() && c.peek() != T::BRACE_CLOSE {
            let fk = c.parse_identifier()?;
            if !eq(&fk, want) {
                return Err(c.error(jcat!("unknown ", kind, " property key '", fk, "'")));
            }
            if seen_body {
                return Err(c.error(jcat!("Field '", fk, "' should be specified only once")));
            }
            seen_body = true;
            c.expect(T::COLON)?;
            let mut into = Vec::new();
            c.expect(T::BRACKET_OPEN)?;
            loop {
                into.push(Box::new(parse_property(c)?));
                if !c.match_tok(T::COMMA)? {
                    break;
                }
            }
            c.expect(T::BRACKET_CLOSE)?;
            c.expect(T::SEMI_COLON)?;
            let mut names: HashSet<&[u16]> = HashSet::new();
            for p in &into {
                if !names.insert(&p.property_name.0) {
                    return Err(c.error(jcat!("duplicate property name '", p.property_name, "'")));
                }
            }
            if complex {
                children = Some(into);
            } else {
                fields = Some(into);
            }
        }
        c.expect(T::BRACE_CLOSE)?;
        if complex && children.is_none() {
            return Err(throw_at(c.tokens(), kind_tok, JStr::from("Field 'properties' is required")));
        }
    }
    Ok(Property {
        property_name: JStr(prop_name),
        wire_key: JStr::from(t[0]),
        protocol_type: JStr::from(t[1]),
        es_type: JStr::from(t[2]),
        fields,
        child_properties: children,
    })
}

/// `nameOrString`: a quoted string (decoded) or an identifier.
fn name_or_string<'s>(c: &mut impl Cursor<'s>) -> R<Vec<u16>> {
    if c.peek() == T::STRING {
        return string_value(c);
    }
    c.parse_identifier()
}

/// `parseIndex`: `name: { properties: [...]; }`, `properties` required (at the name) and once.
fn parse_index<'s>(c: &mut impl Cursor<'s>) -> R<PElasticsearch7ClusterPEsIndex> {
    let name_tok = c.pos();
    let index_name = name_or_string(c)?;
    c.expect(T::COLON)?;
    c.expect(T::BRACE_OPEN)?;
    let mut props: Option<Vec<Box<Property>>> = None;
    while !c.at_end() && c.peek() != T::BRACE_CLOSE {
        let pk = c.parse_identifier()?;
        c.expect(T::COLON)?;
        if !eq(&pk, "properties") {
            return Err(c.error(jcat!("unknown index key '", pk, "'")));
        }
        if props.is_some() {
            return Err(c.error_str("Field 'properties' should be specified only once"));
        }
        let mut list = Vec::new();
        c.expect(T::BRACKET_OPEN)?;
        loop {
            list.push(Box::new(parse_property(c)?));
            if !c.match_tok(T::COMMA)? {
                break;
            }
        }
        props = Some(list);
        c.expect(T::BRACKET_CLOSE)?;
        c.expect(T::SEMI_COLON)?;
    }
    c.expect(T::BRACE_CLOSE)?;
    let Some(properties) = props else {
        return Err(throw_at(c.tokens(), name_tok, JStr::from("Field 'properties' is required")));
    };
    Ok(PElasticsearch7ClusterPEsIndex { index_name: JStr(index_name), properties })
}
