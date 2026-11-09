// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! `###ExternalFormat` (core/'s `parser.section.ExternalFormatSectionGrammar`, docs/rust-port/PHASE2.md §4 step 4d):
//! `SchemaSet` (a format among the production engine's three and its schemas, each content span the whole `content:
//! '...';` statement) and `Binding` (a schema pointer, a content type, the model include and exclude paths). Java's
//! answers reproduced, each refusal at Java's position.

// The generated records hold their record-typed list elements boxed (`Vec<Box<T>>`), so the readers build that.
#![allow(clippy::vec_box)]

use crate::cursor::{Cursor, R, once, throw_at, unquote_path_iae};
use crate::jcat;
use crate::jtypes::JStr;
use crate::lexer::TokenType;
use crate::records::{Element, PBinding, PSchema, PSchemaSet};
use crate::sections::{head, string_value};
use crate::ustr::eq;
use std::collections::HashSet;

use TokenType as T;

/// `ExternalFormatSectionGrammar.parseOne` and `qualifiedNameOf`.
pub fn parse_element<'s>(c: &mut impl Cursor<'s>) -> R<(Element, Vec<u16>)> {
    let kind = c.safe_text();
    if eq(&kind, "SchemaSet") {
        let s = parse_schema_set(c)?;
        let path = crate::protocol::qualified_name(&s.pkg.0, &s.name.0);
        return Ok((Element::PSchemaSet(Box::new(s)), path));
    }
    if eq(&kind, "Binding") {
        let b = parse_binding(c)?;
        let path = crate::protocol::qualified_name(&b.pkg.0, &b.name.0);
        return Ok((Element::PBinding(Box::new(b)), path));
    }
    Err(c.error(jcat!("unsupported ###ExternalFormat element: ", kind)))
}

/// `parseSchemaSet`: `SchemaSet qn { format: F; schemas: [...]; }`, the format checked after the body (at its key).
fn parse_schema_set<'s>(c: &mut impl Cursor<'s>) -> R<PSchemaSet> {
    let h = head(c, "SchemaSet", false)?;
    c.expect(T::BRACE_OPEN)?;
    let mut format: Option<Vec<u16>> = None;
    let mut format_tok = 0;
    let mut schemas = Vec::new();
    let mut seen: HashSet<Vec<u16>> = HashSet::new();
    while !c.at_end() && c.peek() != T::BRACE_CLOSE {
        let key_tok = c.pos();
        let key = c.parse_identifier()?;
        once(&mut seen, &key, c)?;
        c.expect(T::COLON)?;
        if eq(&key, "format") {
            format_tok = key_tok;
            format = Some(c.parse_identifier()?);
        } else if eq(&key, "schemas") {
            parse_schemas(c, &mut schemas)?;
        } else {
            return Err(c.error(jcat!("unknown SchemaSet key '", key, "'")));
        }
        c.expect(T::SEMI_COLON)?;
    }
    c.expect(T::BRACE_CLOSE)?;
    let Some(format) = format else {
        return Err(c.error_str("SchemaSet needs a format"));
    };
    if !eq(&format, "FlatData") && !eq(&format, "JSON") && !eq(&format, "XSD") {
        return Err(throw_at(
            c.tokens(),
            format_tok,
            jcat!("Unknown schema format: ", format),
        ));
    }
    Ok(PSchemaSet {
        pkg: JStr(h.pkg),
        name: JStr(h.name),
        format: JStr(format),
        schemas,
        source_information: c.span(h.decl_start, c.pos() - 1),
    })
}

/// `parseSchemas`: `[ { id: x; location: '..'; content: '..'; }, ... ]`, at least one, each key once per schema.
fn parse_schemas<'s>(c: &mut impl Cursor<'s>, out: &mut Vec<Box<PSchema>>) -> R<()> {
    c.expect(T::BRACKET_OPEN)?;
    if c.peek() == T::BRACKET_CLOSE {
        return Err(c.error_str("Unexpected token ']'"));
    }
    while c.peek() != T::BRACKET_CLOSE {
        let node_start = c.pos();
        c.expect(T::BRACE_OPEN)?;
        let mut id = None;
        let mut location = None;
        let mut content = None;
        let mut content_span = None;
        let mut seen: HashSet<Vec<u16>> = HashSet::new();
        while c.peek() != T::BRACE_CLOSE {
            let key_start = c.pos();
            let k = c.parse_identifier()?;
            once(&mut seen, &k, c)?;
            c.expect(T::COLON)?;
            if eq(&k, "id") {
                id = Some(JStr(c.parse_identifier()?));
            } else if eq(&k, "location") {
                location = Some(JStr(string_value(c)?));
            } else if eq(&k, "content") {
                content = Some(JStr(string_value(c)?));
            } else {
                return Err(c.error(jcat!("unknown schema key '", k, "'")));
            }
            c.expect(T::SEMI_COLON)?;
            if eq(&k, "content") {
                content_span = c.span(key_start, c.pos() - 1);
            }
        }
        c.expect(T::BRACE_CLOSE)?;
        let (Some(content), Some(_)) = (content, content_span.as_ref()) else {
            return Err(c.error_str("schema needs content"));
        };
        out.push(Box::new(PSchema {
            id,
            location,
            content,
            content_source_information: content_span,
            source_information: c.span(node_start, c.pos() - 1),
        }));
        if !c.match_tok(T::COMMA)? {
            break;
        }
    }
    c.expect(T::BRACKET_CLOSE)
}

/// `parseBinding`: `Binding qn { schemaSet: qn; schemaId: x; contentType: '..'; modelIncludes: [...]; modelExcludes:
/// [...]; }`, a content type and at least one include required.
fn parse_binding<'s>(c: &mut impl Cursor<'s>) -> R<PBinding> {
    let h = head(c, "Binding", false)?;
    c.expect(T::BRACE_OPEN)?;
    let mut schema_set = None;
    let mut schema_id = None;
    let mut content_type = None;
    let mut includes = Vec::new();
    let mut excludes = Vec::new();
    let mut seen: HashSet<Vec<u16>> = HashSet::new();
    while !c.at_end() && c.peek() != T::BRACE_CLOSE {
        let key = c.parse_identifier()?;
        once(&mut seen, &key, c)?;
        c.expect(T::COLON)?;
        if eq(&key, "schemaSet") {
            schema_set = Some(JStr(unquote_path_iae(&c.parse_qualified_name()?)?));
        } else if eq(&key, "schemaId") {
            schema_id = Some(JStr(c.parse_identifier()?));
        } else if eq(&key, "contentType") {
            content_type = Some(JStr(string_value(c)?));
        } else if eq(&key, "modelIncludes") {
            parse_paths(c, &mut includes)?;
        } else if eq(&key, "modelExcludes") {
            parse_paths(c, &mut excludes)?;
        } else {
            return Err(c.error(jcat!("unknown Binding key '", key, "'")));
        }
        c.expect(T::SEMI_COLON)?;
    }
    c.expect(T::BRACE_CLOSE)?;
    let Some(content_type) = content_type else {
        return Err(c.error_str("Binding needs a contentType"));
    };
    if includes.is_empty() {
        return Err(throw_at(
            c.tokens(),
            h.decl_start,
            JStr::from("Field 'modelIncludes' is required"),
        ));
    }
    Ok(PBinding {
        pkg: JStr(h.pkg),
        name: JStr(h.name),
        schema_set,
        schema_id,
        content_type,
        model_includes: includes,
        model_excludes: excludes,
        source_information: c.span(h.decl_start, c.pos() - 1),
    })
}

/// `parsePaths`: `[ qn, ... ]`, each unquoted.
fn parse_paths<'s>(c: &mut impl Cursor<'s>, out: &mut Vec<JStr>) -> R<()> {
    c.expect(T::BRACKET_OPEN)?;
    while c.peek() != T::BRACKET_CLOSE {
        out.push(JStr(unquote_path_iae(&c.parse_qualified_name()?)?));
        if !c.match_tok(T::COMMA)? {
            break;
        }
    }
    c.expect(T::BRACKET_CLOSE)
}
