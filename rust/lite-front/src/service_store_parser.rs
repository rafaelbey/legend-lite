// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! `###ServiceStore` (core/'s `parser.section.ServiceStoreSectionGrammar`, docs/rust-port/PHASE2.md §4 step 4d): an
//! optional description (kept at lite's and the platform's dialects, dropped at the engine's), then services and
//! nested service groups -- each service's path, method, typed request and response, parameters with their facets, and
//! the engine walker's cross-checks. Java's answers reproduced, each refusal at Java's position.

// The generated records hold their record-typed list elements boxed (`Vec<Box<T>>`), so the readers build that.
#![allow(clippy::vec_box)]

use crate::cursor::{Cursor, R, throw_at, unquote_path_iae};
use crate::doors::JavaError;
use crate::jcat;
use crate::jtypes::JStr;
use crate::lexer::TokenType;
use crate::records::{PServiceStoreDefinition, PServiceStoreElement, PSsParam, PSsService, PSsServiceGroup, PSsTypeRef};
use crate::sections::{head, string_value};
use crate::ustr::{ends_with, eq, index_of_units, starts_with, u};
use std::collections::HashSet;

use TokenType as T;

/// `ServiceStoreSectionGrammar.parseOne`: `ServiceStore qn ( description: '..'; Service ... ServiceGroup ... )`, its
/// elements' ids unique (the duplicates listed as Java's `List.toString` prints them).
pub fn parse_service_store<'s>(c: &mut impl Cursor<'s>) -> R<PServiceStoreDefinition> {
    let h = head(c, "ServiceStore", false)?;
    c.expect(T::PAREN_OPEN)?;
    let mut description = None;
    let mut elements = Vec::new();
    while !c.at_end() && c.peek() != T::PAREN_CLOSE {
        let kw = c.safe_text();
        if eq(&kw, "description") {
            if description.is_some() {
                return Err(c.error_str("Field 'description' should be specified only once"));
            }
            c.advance()?;
            c.expect(T::COLON)?;
            description = Some(JStr(string_value(c)?));
            c.expect(T::SEMI_COLON)?;
        } else if eq(&kw, "Service") {
            elements.push(PServiceStoreElement::PSsService(Box::new(parse_service(c)?)));
        } else if eq(&kw, "ServiceGroup") {
            elements.push(PServiceStoreElement::PSsServiceGroup(Box::new(parse_group(c)?)));
        } else {
            return Err(c.error(jcat!("unknown ServiceStore element: ", kw)));
        }
    }
    c.expect(T::PAREN_CLOSE)?;
    let mut ids: HashSet<&[u16]> = HashSet::new();
    let mut dup: Vec<&[u16]> = Vec::new();
    for e in &elements {
        let id = id_of(e);
        if !ids.insert(id) && !dup.contains(&id) {
            dup.push(id);
        }
    }
    if !dup.is_empty() {
        let mut list = u("[");
        for (i, d) in dup.iter().enumerate() {
            if i > 0 {
                list.extend(u(", "));
            }
            list.extend_from_slice(d);
        }
        list.push(u16::from(b']'));
        return Err(throw_at(
            c.tokens(),
            h.decl_start,
            jcat!("Service Store Elements should have unique ids. Multiple elements found with ids - ", list),
        ));
    }
    let description = if c.dialect().refuses_lite_extensions() { None } else { description };
    Ok(PServiceStoreDefinition {
        pkg: JStr(h.pkg),
        name: JStr(h.name),
        description,
        elements,
        source_information: c.span(h.decl_start, c.pos() - 1),
    })
}

/// `parseGroup`: `ServiceGroup id ( path: '..'; Service ... ServiceGroup ... )`, a path required.
fn parse_group<'s>(c: &mut impl Cursor<'s>) -> R<PSsServiceGroup> {
    let start = c.pos();
    c.advance()?;
    let id = c.parse_identifier()?;
    c.expect(T::PAREN_OPEN)?;
    let mut path = None;
    let mut elements = Vec::new();
    while !c.at_end() && c.peek() != T::PAREN_CLOSE {
        let kw = c.safe_text();
        if eq(&kw, "path") {
            c.advance()?;
            c.expect(T::COLON)?;
            path = Some(string_value(c)?);
            c.expect(T::SEMI_COLON)?;
        } else if eq(&kw, "Service") {
            elements.push(PServiceStoreElement::PSsService(Box::new(parse_service(c)?)));
        } else if eq(&kw, "ServiceGroup") {
            elements.push(PServiceStoreElement::PSsServiceGroup(Box::new(parse_group(c)?)));
        } else {
            return Err(c.error(jcat!("unknown ServiceGroup element: ", kw)));
        }
    }
    c.expect(T::PAREN_CLOSE)?;
    let Some(path) = path else {
        return Err(c.error(jcat!("ServiceGroup '", id, "' needs a path")));
    };
    Ok(PSsServiceGroup { id: JStr(id), path: JStr(path), elements, source_information: c.span(start, c.pos() - 1) })
}

fn id_of(e: &PServiceStoreElement) -> &[u16] {
    match e {
        PServiceStoreElement::PSsService(s) => &s.id.0,
        PServiceStoreElement::PSsServiceGroup(g) => &g.id.0,
    }
}

/// `parseService`: `Service id ( path: '/..'; requestBody: T; method: GET; parameters: (...); response: T; security:
/// [...]; )`, then the engine walker's cross-checks.
fn parse_service<'s>(c: &mut impl Cursor<'s>) -> R<PSsService> {
    let start = c.pos();
    c.advance()?;
    let id = c.parse_identifier()?;
    c.expect(T::PAREN_OPEN)?;
    let mut path: Option<Vec<u16>> = None;
    let mut request_body = None;
    let mut method: Option<Vec<u16>> = None;
    let mut parameters: Option<Vec<Box<PSsParam>>> = None;
    let mut response = None;
    while !c.at_end() && c.peek() != T::PAREN_CLOSE {
        let key = c.parse_identifier()?;
        c.expect(T::COLON)?;
        if eq(&key, "path") {
            let p = string_value(c)?;
            if !starts_with(&p, "/") || ends_with(&p, "/") {
                return Err(c.error_str("Path should start with '/' & should not end with '/'"));
            }
            path = Some(p);
        } else if eq(&key, "requestBody") {
            request_body = Some(Box::new(parse_type_ref(c)?));
        } else if eq(&key, "method") {
            if method.is_some() {
                return Err(c.error_str("Field 'method' should be specified only once"));
            }
            let m = c.parse_identifier()?;
            if !eq(&m, "GET") && !eq(&m, "POST") {
                return Err(c.error(jcat!("Unsupported HTTP Method type - ", m, ". Supported types are - GET,POST")));
            }
            method = Some(m);
        } else if eq(&key, "parameters") {
            parameters = Some(parse_parameters(c)?);
        } else if eq(&key, "response") {
            response = Some(Box::new(parse_type_ref(c)?));
        } else if eq(&key, "security") {
            c.expect(T::BRACKET_OPEN)?;
            if c.peek() != T::BRACKET_CLOSE {
                let scheme = c.parse_identifier()?;
                return Err(c.error(jcat!("Unsupported SecurityScheme - ", scheme)));
            }
            c.expect(T::BRACKET_CLOSE)?;
        } else {
            return Err(c.error(jcat!("unknown Service key '", key, "'")));
        }
        c.expect(T::SEMI_COLON)?;
    }
    c.expect(T::PAREN_CLOSE)?;
    let (Some(path), Some(method), Some(response)) = (path, method, response) else {
        return Err(c.error(jcat!("Service '", id, "' needs path, method and response")));
    };
    if eq(&method, "GET") && request_body.is_some() {
        return Err(throw_at(c.tokens(), start, JStr::from("Request Body should not be specified for GET end point")));
    }
    if let Some(params) = &parameters {
        let mut missing: Vec<&[u16]> = Vec::new();
        for pm in params {
            if pm.location == "PATH" {
                if pm.required == Some(false) {
                    let s = pm.source_information.as_deref().expect("a parsed node always carries its span");
                    return Err(JavaError::parse(
                        JStr::from("Path parameters cannot be optional"),
                        s.start_line,
                        s.start_column,
                    ));
                }
                let mut needle = u("{");
                needle.extend_from_slice(&pm.name.0);
                needle.push(u16::from(b'}'));
                if index_of_units(&path, &needle, 0).is_none() {
                    missing.push(&pm.name.0);
                }
            }
        }
        if !missing.is_empty() {
            let mut list = Vec::new();
            for (i, m) in missing.iter().enumerate() {
                if i > 0 {
                    list.push(u16::from(b','));
                }
                list.extend_from_slice(m);
            }
            return Err(throw_at(
                c.tokens(),
                start,
                jcat!(
                    "Path parameters should be specified in path as '{param_name}'. [",
                    list,
                    "] parameters were not found in path ",
                    path
                ),
            ));
        }
    }
    Ok(PSsService {
        id: JStr(id),
        path: JStr(path),
        request_body,
        method: JStr(method),
        parameters,
        response,
        security: Vec::new(),
        source_information: c.span(start, c.pos() - 1),
    })
}

/// `parseParameters`: `( p: T (opts), ... )`.
fn parse_parameters<'s>(c: &mut impl Cursor<'s>) -> R<Vec<Box<PSsParam>>> {
    let mut out = Vec::new();
    c.expect(T::PAREN_OPEN)?;
    while c.peek() != T::PAREN_CLOSE {
        out.push(Box::new(parse_parameter(c)?));
        if !c.match_tok(T::COMMA)? {
            break;
        }
    }
    c.expect(T::PAREN_CLOSE)?;
    Ok(out)
}

/// `bool`: `true` or `false`.
fn bool_value<'s>(c: &mut impl Cursor<'s>) -> R<bool> {
    if c.match_tok(T::TRUE)? {
        return Ok(true);
    }
    if c.match_tok(T::FALSE)? {
        return Ok(false);
    }
    Err(c.error(jcat!("expected true or false, got ", c.safe_text())))
}

/// `parseParameter`: `name: T (allowReserved = b, required = b, location = l, style = s, explode = b, enum = qn)`, a
/// `"quoted"` name's first and last unit off; a location required, a list parameter's style and explode required and a
/// scalar's refused.
fn parse_parameter<'s>(c: &mut impl Cursor<'s>) -> R<PSsParam> {
    let start = c.pos();
    let name = if c.peek() == T::QUOTED_STRING {
        let raw = c.text();
        c.advance()?;
        crate::ustr::substring(raw, 1, raw.len() as isize - 1)?.to_vec()
    } else {
        c.parse_identifier()?
    };
    c.expect(T::COLON)?;
    let ty = parse_type_ref(c)?;
    let mut allow_reserved = None;
    let mut required = None;
    let mut location: Option<Vec<u16>> = None;
    let mut style = None;
    let mut style_span = None;
    let mut explode = None;
    let mut explode_span = None;
    let mut enumeration = None;
    if c.match_tok(T::PAREN_OPEN)? {
        while c.peek() != T::PAREN_CLOSE {
            let opt_start = c.pos();
            let opt = c.parse_identifier()?;
            c.expect(T::EQUAL)?;
            if eq(&opt, "allowReserved") {
                allow_reserved = Some(bool_value(c)?);
            } else if eq(&opt, "required") {
                required = Some(bool_value(c)?);
            } else if eq(&opt, "location") {
                let l = c.parse_identifier()?;
                if !eq(&l, "header") && !eq(&l, "path") && !eq(&l, "query") {
                    return Err(c.error(jcat!(
                        "Unsupported Parameter Location - ",
                        l,
                        ". Supported Locations are - header,path,query"
                    )));
                }
                location = Some(l);
            } else if eq(&opt, "style") {
                style = Some(JStr(c.parse_identifier()?));
                style_span = c.span(opt_start, c.pos() - 1);
            } else if eq(&opt, "explode") {
                explode = Some(bool_value(c)?);
                explode_span = c.span(opt_start, c.pos() - 1);
            } else if eq(&opt, "enum") {
                enumeration = Some(JStr(unquote_path_iae(&c.parse_qualified_name()?)?));
            } else {
                return Err(c.error(jcat!("unknown parameter option '", opt, "'")));
            }
            if !c.match_tok(T::COMMA)? {
                break;
            }
        }
        c.expect(T::PAREN_CLOSE)?;
    }
    let Some(location) = location else {
        return Err(c.error(jcat!("parameter '", name, "' needs a location")));
    };
    let refuse = |m: &str| throw_at(c.tokens(), start, JStr::from(m));
    if ty.list {
        if style.is_none() {
            return Err(refuse("Field 'style' is required"));
        }
        if explode.is_none() {
            return Err(refuse("Field 'explode' is required"));
        }
    } else {
        if style.is_some() {
            return Err(refuse("style should not be provided with non-list service parameter"));
        }
        if explode.is_some() {
            return Err(refuse("explode should not be provided with non-list service parameter"));
        }
    }
    // the location is one of three ASCII words, so `toUpperCase(Locale.ROOT)` is ASCII's
    let location: Vec<u16> =
        location.iter().map(|&ch| if (0x61..=0x7A).contains(&ch) { ch - 32 } else { ch }).collect();
    Ok(PSsParam {
        name: JStr(name),
        r#type: Box::new(ty),
        allow_reserved,
        required,
        location: JStr(location),
        style,
        style_span,
        explode,
        explode_span,
        enumeration,
        source_information: c.span(start, c.pos() - 1),
    })
}

/// `parseTypeRef`: `String`, `[Integer]`, `Class <- binding`, `[Class <- binding]`.
fn parse_type_ref<'s>(c: &mut impl Cursor<'s>) -> R<PSsTypeRef> {
    let start = c.pos();
    let list = c.match_tok(T::BRACKET_OPEN)?;
    let head = unquote_path_iae(&c.parse_qualified_name()?)?;
    let mut complex_type = None;
    let mut binding = None;
    if c.peek() == T::LESS_THAN {
        c.advance()?;
        c.expect(T::MINUS)?;
        complex_type = Some(head.clone());
        binding = Some(JStr(unquote_path_iae(&c.parse_qualified_name()?)?));
    }
    if list {
        c.expect(T::BRACKET_CLOSE)?;
    }
    let span = c.span(start, c.pos() - 1);
    Ok(match complex_type {
        Some(ct) => PSsTypeRef { primitive: None, complex_type: Some(JStr(ct)), binding, list, source_information: span },
        None => PSsTypeRef { primitive: Some(JStr(head)), complex_type: None, binding: None, list, source_information: span },
    })
}
