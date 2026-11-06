// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! `###DataSpace` (core/'s `parser.section.DataSpaceSectionGrammar`, docs/rust-port/PHASE2.md §4 step 4c): one
//! `DataSpace` element at a time from its keyword -- the decorated envelope (documentation included), the execution
//! contexts (a mapping or a mapping provider, a default runtime, a test-data island carried raw), the default context,
//! title and description, the executables (a pointer or an inline query, sample values), the diagrams (and the
//! deprecated featured ones, appended), the support info (Email, Combined, the keyword-less full form), the
//! operational metadata and the element scope list. Also `RelationIslands.parseStandaloneRelationAt`, the executable's
//! `sampleValues`. Java's answers reproduced, each refusal at Java's position.

// The generated records hold their record-typed list elements boxed (`Vec<Box<T>>`), so the readers build that.
#![allow(clippy::vec_box)]

use crate::cursor::{Cursor, R, is_identifier_token, once, once_at, throw_at, unquote_path_iae};
use crate::doors::JavaError;
use crate::jcat;
use crate::jtypes::JStr;
use crate::lexer::TokenType;
use crate::mapping_parser::MappingCursor;
use crate::records::{
    PDataSpace, PDataSpaceContext, PDataSpaceDiagram, PDataSpaceElementRef, PDataSpaceEmail, PDataSpaceExecutable,
    PDataSpaceExpertise, PDataSpaceLink, PDataSpaceMappingProvider, PDataSpaceOperationalMetadata, PDataSpaceSupport,
    PDataSpaceSupportPSupportCombined, PDataSpaceSupportPSupportEmail, PDataSpaceSupportPSupportFull,
    PDataSpaceTestData, PRelationElement, SourceInfo,
};
use crate::sections::{lambda_to_semicolon, scan_island, split_last, string_value};
use crate::ustr::{eq, trim};
use std::collections::HashSet;

use TokenType as T;

/// The engine's own enum names (`DataSpaceRegion`, `DataSpaceDeliveryFrequency`), in order: a value is validated as
/// written, the list printed as Java's `List.toString` prints it.
const REGIONS: &[&str] = &["APAC", "EMEA", "LAMR", "NAMR"];
const FREQUENCIES: &[&str] = &["INTRADAY", "DAILY", "WEEKLY", "MONTHLY", "QUARTERLY", "YEARLY", "ON_DEMAND"];

fn listed(list: &[&str], v: &[u16]) -> bool {
    list.iter().any(|s| eq(v, s))
}

/// Java's `List.toString`: `[a, b, c]`.
fn list_text(list: &[&str]) -> String {
    format!("[{}]", list.join(", "))
}

/// `DataSpaceSectionGrammar.parseElement`: `'''doc''' DataSpace <<...>> {...} qn { body }`.
pub fn parse_data_space<'s>(c: &mut impl Cursor<'s>) -> R<PDataSpace> {
    let decl_start = c.pos();
    let doc = c.parse_documentation()?;
    if !is_identifier_token(c.peek()) || !eq(&c.safe_text(), "DataSpace") {
        return Err(c.error(jcat!("unsupported ###DataSpace element: ", c.safe_text())));
    }
    c.advance()?;
    let decorations = c.parse_decorations()?;
    let dec = c.with_documentation(doc, decorations)?;
    let qn = unquote_path_iae(&c.parse_qualified_name()?)?;
    let (pkg, name) = split_last(&qn);
    c.expect(T::BRACE_OPEN)?;
    let mut contexts = Vec::new();
    let mut default_context = None;
    let mut title = None;
    let mut description = None;
    let mut executables = None;
    let mut diagrams: Option<Vec<Box<PDataSpaceDiagram>>> = None;
    let mut featured = Vec::new();
    let mut support_info = None;
    let mut operational_metadata = None;
    let mut elements = None;
    let mut seen: HashSet<Vec<u16>> = HashSet::new();
    while !c.at_end() && c.peek() != T::BRACE_CLOSE {
        let key_start = c.pos();
        let key = c.parse_identifier()?;
        let coordinate = eq(&key, "groupId") || eq(&key, "artifactId") || eq(&key, "versionId");
        if !coordinate {
            // the deprecated coordinates duplicate freely (the walker drops the whole field on more than one)
            once_at(&mut seen, &key, c.tokens(), decl_start)?;
        }
        c.expect(T::COLON)?;
        if coordinate {
            // parsed and dropped: the engine's grammar admits them, its walker never reads them
            string_value(c)?;
            c.expect(T::SEMI_COLON)?;
            continue;
        }
        match String::from_utf16_lossy(&key).as_str() {
            "featuredDiagrams" => {
                // each path a diagram entry with an empty title, both spans the path's; appended after `diagrams`
                c.expect(T::BRACKET_OPEN)?;
                while c.peek() != T::BRACKET_CLOSE {
                    let ds = c.pos();
                    let path = unquote_path_iae(&c.parse_qualified_name()?)?;
                    let span = c.span(ds, c.pos() - 1);
                    featured.push(Box::new(PDataSpaceDiagram {
                        title: JStr::default(),
                        description: None,
                        diagram: JStr(path),
                        diagram_span: span.clone(),
                        source_information: span,
                    }));
                    if !c.match_tok(T::COMMA)? {
                        break;
                    }
                }
                c.expect(T::BRACKET_CLOSE)?;
                c.expect(T::SEMI_COLON)?;
            }
            "executionContexts" => parse_contexts(c, &mut contexts)?,
            "defaultExecutionContext" => {
                default_context = Some(JStr(string_value(c)?));
                c.expect(T::SEMI_COLON)?;
            }
            "title" => {
                title = Some(JStr(string_value(c)?));
                c.expect(T::SEMI_COLON)?;
            }
            "description" => {
                description = Some(JStr(string_value(c)?));
                c.expect(T::SEMI_COLON)?;
            }
            "executables" => {
                let mut list = Vec::new();
                parse_executables(c, &mut list)?;
                executables = Some(list);
            }
            "diagrams" => {
                let mut list = Vec::new();
                parse_diagrams(c, &mut list)?;
                diagrams = Some(list);
            }
            "supportInfo" => {
                support_info = Some(parse_support_info(c)?);
                c.expect(T::SEMI_COLON)?;
            }
            "operationalMetadata" => {
                operational_metadata = Some(Box::new(parse_operational_metadata(c, key_start)?));
            }
            "elements" => {
                // `[ model, -model::experiment ]`: an exclusion's span takes its '-'
                let mut list = Vec::new();
                c.expect(T::BRACKET_OPEN)?;
                while c.peek() != T::BRACKET_CLOSE && !c.at_end() {
                    let s = c.pos();
                    let exclude = c.match_tok(T::MINUS)?;
                    let path = unquote_path_iae(&c.parse_qualified_name()?)?;
                    list.push(Box::new(PDataSpaceElementRef {
                        path: JStr(path),
                        exclude,
                        source_information: c.span(s, c.pos() - 1),
                    }));
                    c.match_tok(T::COMMA)?;
                }
                c.expect(T::BRACKET_CLOSE)?;
                c.expect(T::SEMI_COLON)?;
                elements = Some(list);
            }
            _ => return Err(c.error(jcat!("unknown key '", key, "' inside DataSpace '", qn, "'"))),
        }
    }
    c.expect(T::BRACE_CLOSE)?;
    let diagrams = if featured.is_empty() {
        diagrams
    } else {
        match diagrams {
            None => Some(featured),
            Some(mut declared) => {
                declared.extend(featured);
                Some(declared)
            }
        }
    };
    Ok(PDataSpace {
        pkg: JStr(pkg),
        name: JStr(name),
        stereotypes: dec.stereotypes,
        tagged_values: dec.tagged_values,
        execution_contexts: Some(contexts),
        default_execution_context: default_context,
        title,
        description,
        executables,
        diagrams,
        support_info,
        operational_metadata,
        elements,
        source_information: c.span(decl_start, c.pos() - 1),
    })
}

/// `parseOperationalMetadata`: `{ coverageRegions: [A, B]; updateFrequency: F; };`, its span the key through the `;`.
fn parse_operational_metadata<'s>(c: &mut impl Cursor<'s>, key_start: usize) -> R<PDataSpaceOperationalMetadata> {
    c.expect(T::BRACE_OPEN)?;
    let mut regions = Vec::new();
    let mut frequency = None;
    let mut seen: HashSet<Vec<u16>> = HashSet::new();
    while !c.at_end() && c.peek() != T::BRACE_CLOSE {
        let key = c.parse_identifier()?;
        once_at(&mut seen, &key, c.tokens(), key_start)?;
        c.expect(T::COLON)?;
        if eq(&key, "coverageRegions") {
            regions = Vec::new();
            c.expect(T::BRACKET_OPEN)?;
            while c.peek() != T::BRACKET_CLOSE && !c.at_end() {
                let v_tok = c.pos();
                let v = c.parse_identifier()?;
                if !listed(REGIONS, &v) {
                    return Err(throw_at(
                        c.tokens(),
                        v_tok,
                        jcat!("Unknown coverage region '", v, "'. Valid values: ", list_text(REGIONS)),
                    ));
                }
                regions.push(JStr(v));
                if !c.match_tok(T::COMMA)? {
                    break;
                }
            }
            c.expect(T::BRACKET_CLOSE)?;
        } else if eq(&key, "updateFrequency") {
            let v_tok = c.pos();
            let f = c.parse_identifier()?;
            if !listed(FREQUENCIES, &f) {
                return Err(throw_at(
                    c.tokens(),
                    v_tok,
                    jcat!("Unknown update frequency '", f, "'. Valid values: ", list_text(FREQUENCIES)),
                ));
            }
            frequency = Some(JStr(f));
        } else {
            return Err(c.error(jcat!("unknown operationalMetadata key '", key, "'")));
        }
        c.expect(T::SEMI_COLON)?;
    }
    c.expect(T::BRACE_CLOSE)?;
    c.expect(T::SEMI_COLON)?;
    Ok(PDataSpaceOperationalMetadata {
        coverage_regions: regions,
        update_frequency: frequency,
        source_information: c.span(key_start, c.pos() - 1),
    })
}

/// `parseSupportInfo`: `Email { address: '...'; }`, `Combined { ...; emails: [..]; }` or the keyword-less full form
/// `{ documentation: {..}; ... }`; the span the value's alone. The kind is judged after its body is read.
fn parse_support_info<'s>(c: &mut impl Cursor<'s>) -> R<PDataSpaceSupport> {
    let start = c.pos();
    if c.peek() == T::BRACE_OPEN {
        return parse_support_full(c);
    }
    let kind = c.parse_identifier()?;
    c.expect(T::BRACE_OPEN)?;
    let mut address = None;
    let mut documentation_url = None;
    let mut website = None;
    let mut faq_url = None;
    let mut support_url = None;
    let mut emails = None;
    let mut seen: HashSet<Vec<u16>> = HashSet::new();
    while !c.at_end() && c.peek() != T::BRACE_CLOSE {
        let key = c.parse_identifier()?;
        once_at(&mut seen, &key, c.tokens(), start)?;
        c.expect(T::COLON)?;
        match String::from_utf16_lossy(&key).as_str() {
            "address" => address = Some(JStr(string_value(c)?)),
            "documentationUrl" => documentation_url = Some(JStr(string_value(c)?)),
            "website" => website = Some(JStr(string_value(c)?)),
            "faqUrl" => faq_url = Some(JStr(string_value(c)?)),
            "supportUrl" => support_url = Some(JStr(string_value(c)?)),
            "emails" => {
                let mut list = Vec::new();
                c.expect(T::BRACKET_OPEN)?;
                while c.peek() != T::BRACKET_CLOSE {
                    list.push(JStr(string_value(c)?));
                    if !c.match_tok(T::COMMA)? {
                        break;
                    }
                }
                c.expect(T::BRACKET_CLOSE)?;
                emails = Some(list);
            }
            _ => return Err(c.error(jcat!("unknown supportInfo key '", key, "'"))),
        }
        c.expect(T::SEMI_COLON)?;
    }
    c.expect(T::BRACE_CLOSE)?;
    let span = c.span(start, c.pos() - 1);
    if eq(&kind, "Email") {
        let Some(address) = address else {
            return Err(throw_at(c.tokens(), start, JStr::from("Field 'address' is required")));
        };
        return Ok(PDataSpaceSupport::PDataSpaceSupportPSupportEmail(Box::new(PDataSpaceSupportPSupportEmail {
            address,
            documentation_url,
            source_information: span,
        })));
    }
    if eq(&kind, "Combined") {
        return Ok(PDataSpaceSupport::PDataSpaceSupportPSupportCombined(Box::new(PDataSpaceSupportPSupportCombined {
            documentation_url,
            website,
            faq_url,
            support_url,
            emails,
            source_information: span,
        })));
    }
    Err(c.error(jcat!("unknown supportInfo kind '", kind, "'")))
}

/// `parseSupportFull`: `{ documentation: {label; url}; website: {..}; faqUrl: {..}; supportUrl: {..}; emails:
/// [{title; address}]; expertise: [{description; expertIds: [..]}]; }`.
fn parse_support_full<'s>(c: &mut impl Cursor<'s>) -> R<PDataSpaceSupport> {
    let start = c.pos();
    c.expect(T::BRACE_OPEN)?;
    let mut documentation = None;
    let mut website = None;
    let mut faq_url = None;
    let mut support_url = None;
    let mut emails = None;
    let mut expertise = None;
    let mut seen: HashSet<Vec<u16>> = HashSet::new();
    while !c.at_end() && c.peek() != T::BRACE_CLOSE {
        let key = c.parse_identifier()?;
        once_at(&mut seen, &key, c.tokens(), start)?;
        c.expect(T::COLON)?;
        match String::from_utf16_lossy(&key).as_str() {
            "documentation" => documentation = Some(Box::new(parse_link(c)?)),
            "website" => website = Some(Box::new(parse_link(c)?)),
            "faqUrl" => faq_url = Some(Box::new(parse_link(c)?)),
            "supportUrl" => support_url = Some(Box::new(parse_link(c)?)),
            "emails" => {
                let mut list = Vec::new();
                c.expect(T::BRACKET_OPEN)?;
                while c.peek() != T::BRACKET_CLOSE && !c.at_end() {
                    list.push(Box::new(parse_email(c)?));
                    if !c.match_tok(T::COMMA)? {
                        break;
                    }
                }
                c.expect(T::BRACKET_CLOSE)?;
                emails = Some(list);
            }
            "expertise" => {
                let mut list = Vec::new();
                c.expect(T::BRACKET_OPEN)?;
                while c.peek() != T::BRACKET_CLOSE && !c.at_end() {
                    list.push(Box::new(parse_expertise(c)?));
                    if !c.match_tok(T::COMMA)? {
                        break;
                    }
                }
                c.expect(T::BRACKET_CLOSE)?;
                expertise = Some(list);
            }
            _ => return Err(c.error(jcat!("unknown supportInfo key '", key, "'"))),
        }
        c.expect(T::SEMI_COLON)?;
    }
    c.expect(T::BRACE_CLOSE)?;
    Ok(PDataSpaceSupport::PDataSpaceSupportPSupportFull(Box::new(PDataSpaceSupportPSupportFull {
        documentation,
        website,
        faq_url,
        support_url,
        emails,
        expertise,
        source_information: c.span(start, c.pos() - 1),
    })))
}

/// `parseLink`: `{ label: '..'; url: '..'; }`, the url required.
fn parse_link<'s>(c: &mut impl Cursor<'s>) -> R<PDataSpaceLink> {
    let start = c.pos();
    c.expect(T::BRACE_OPEN)?;
    let mut label = None;
    let mut url = None;
    let mut seen: HashSet<Vec<u16>> = HashSet::new();
    while !c.at_end() && c.peek() != T::BRACE_CLOSE {
        let key = c.parse_identifier()?;
        once_at(&mut seen, &key, c.tokens(), start)?;
        c.expect(T::COLON)?;
        if eq(&key, "label") {
            label = Some(JStr(string_value(c)?));
        } else if eq(&key, "url") {
            url = Some(JStr(string_value(c)?));
        } else {
            return Err(c.error(jcat!("unknown link key '", key, "'")));
        }
        c.expect(T::SEMI_COLON)?;
    }
    c.expect(T::BRACE_CLOSE)?;
    let Some(url) = url else {
        return Err(throw_at(c.tokens(), start, JStr::from("Field 'url' is required")));
    };
    Ok(PDataSpaceLink { label, url, source_information: c.span(start, c.pos() - 1) })
}

/// `parseEmail`: `{ title: '..'; address: '..'; }`, both required.
fn parse_email<'s>(c: &mut impl Cursor<'s>) -> R<PDataSpaceEmail> {
    let start = c.pos();
    c.expect(T::BRACE_OPEN)?;
    let mut title = None;
    let mut address = None;
    let mut seen: HashSet<Vec<u16>> = HashSet::new();
    while !c.at_end() && c.peek() != T::BRACE_CLOSE {
        let key = c.parse_identifier()?;
        once_at(&mut seen, &key, c.tokens(), start)?;
        c.expect(T::COLON)?;
        if eq(&key, "title") {
            title = Some(JStr(string_value(c)?));
        } else if eq(&key, "address") {
            address = Some(JStr(string_value(c)?));
        } else {
            return Err(c.error(jcat!("unknown email key '", key, "'")));
        }
        c.expect(T::SEMI_COLON)?;
    }
    c.expect(T::BRACE_CLOSE)?;
    let Some(title) = title else {
        return Err(throw_at(c.tokens(), start, JStr::from("Field 'title' is required")));
    };
    let Some(address) = address else {
        return Err(throw_at(c.tokens(), start, JStr::from("Field 'address' is required")));
    };
    Ok(PDataSpaceEmail { title, address, source_information: c.span(start, c.pos() - 1) })
}

/// `parseExpertise`: `{ description: '..'; expertIds: ['..', '..']; }`, both optional.
fn parse_expertise<'s>(c: &mut impl Cursor<'s>) -> R<PDataSpaceExpertise> {
    let start = c.pos();
    c.expect(T::BRACE_OPEN)?;
    let mut description = None;
    let mut expert_ids = None;
    let mut seen: HashSet<Vec<u16>> = HashSet::new();
    while !c.at_end() && c.peek() != T::BRACE_CLOSE {
        let key = c.parse_identifier()?;
        once_at(&mut seen, &key, c.tokens(), start)?;
        c.expect(T::COLON)?;
        if eq(&key, "description") {
            description = Some(JStr(string_value(c)?));
        } else if eq(&key, "expertIds") {
            let mut list = Vec::new();
            c.expect(T::BRACKET_OPEN)?;
            while c.peek() != T::BRACKET_CLOSE && !c.at_end() {
                list.push(JStr(string_value(c)?));
                if !c.match_tok(T::COMMA)? {
                    break;
                }
            }
            c.expect(T::BRACKET_CLOSE)?;
            expert_ids = Some(list);
        } else {
            return Err(c.error(jcat!("unknown expertise key '", key, "'")));
        }
        c.expect(T::SEMI_COLON)?;
    }
    c.expect(T::BRACE_CLOSE)?;
    Ok(PDataSpaceExpertise { description, expert_ids, source_information: c.span(start, c.pos() - 1) })
}

/// `parseContexts`: `[ { name; title?; description?; mapping | mappingProvider; defaultRuntime?; testData?; }, ... ];`
/// -- a pointer's span the whole `key: value;` statement.
fn parse_contexts<'s>(c: &mut impl Cursor<'s>, out: &mut Vec<Box<PDataSpaceContext>>) -> R<()> {
    c.expect(T::BRACKET_OPEN)?;
    while c.peek() != T::BRACKET_CLOSE && !c.at_end() {
        let ctx_start = c.pos();
        c.expect(T::BRACE_OPEN)?;
        let mut name = None;
        let mut title = None;
        let mut description = None;
        let mut mapping = None;
        let mut mapping_span = None;
        let mut mapping_provider = None;
        let mut default_runtime = None;
        let mut runtime_span = None;
        let mut test_data = None;
        let mut seen: HashSet<Vec<u16>> = HashSet::new();
        while !c.at_end() && c.peek() != T::BRACE_CLOSE {
            let key_start = c.pos();
            let key = c.parse_identifier()?;
            once_at(&mut seen, &key, c.tokens(), ctx_start)?;
            c.expect(T::COLON)?;
            let mut provider: Option<(Vec<u16>, SourceInfo, Vec<JStr>)> = None;
            match String::from_utf16_lossy(&key).as_str() {
                "name" => name = Some(JStr(string_value(c)?)),
                "title" => title = Some(JStr(string_value(c)?)),
                "description" => description = Some(JStr(string_value(c)?)),
                "mapping" => mapping = Some(JStr(unquote_path_iae(&c.parse_qualified_name()?)?)),
                "mappingProvider" => {
                    // `mappingProvider: fn.k1, k2;`: a typeless element pointer and its keys (empty when none)
                    let e_s = c.pos();
                    let element = unquote_path_iae(&c.parse_qualified_name()?)?;
                    let element_span = c.span_of(e_s, c.pos() - 1);
                    let mut keys = Vec::new();
                    if c.match_tok(T::DOT)? {
                        keys.push(JStr(c.parse_identifier()?));
                        while c.match_tok(T::COMMA)? {
                            keys.push(JStr(c.parse_identifier()?));
                        }
                    }
                    provider = Some((element, element_span, keys));
                }
                "defaultRuntime" => default_runtime = Some(JStr(unquote_path_iae(&c.parse_qualified_name()?)?)),
                "testData" => {
                    // `Reference #{ path }#`: the span the value's (kind through `}#`), no key, no semicolon
                    let vs = c.pos();
                    let kind = c.parse_identifier()?;
                    if !eq(&kind, "Reference") && !eq(&kind, "DataspaceTestData") {
                        return Err(throw_at(c.tokens(), vs, JStr::from("Unexpected token")));
                    }
                    let path = trim(&raw_island(c)?).to_vec();
                    test_data = Some(Box::new(PDataSpaceTestData {
                        kind: JStr(kind),
                        path: JStr(path),
                        source_information: c.span(vs, c.pos() - 1),
                    }));
                }
                _ => return Err(c.error(jcat!("unknown executionContexts key: ", key))),
            }
            c.expect(T::SEMI_COLON)?;
            if eq(&key, "mapping") {
                mapping_span = c.span(key_start, c.pos() - 1);
            } else if eq(&key, "defaultRuntime") {
                runtime_span = c.span(key_start, c.pos() - 1);
            } else if let Some((element, element_span, keys)) = provider {
                mapping_provider = Some(Box::new(PDataSpaceMappingProvider {
                    element: JStr(element),
                    element_span: Some(Box::new(element_span)),
                    keys,
                    source_information: c.span(key_start, c.pos() - 1),
                }));
            }
        }
        c.expect(T::BRACE_CLOSE)?;
        let ctx_span = c.span(ctx_start, c.pos() - 1);
        let Some(name) = name else {
            return Err(throw_at(c.tokens(), ctx_start, JStr::from("Field 'name' is required")));
        };
        if mapping.is_none() && mapping_provider.is_none() {
            return Err(throw_at(
                c.tokens(),
                ctx_start,
                JStr::from("Data space execution context must define either 'mapping' or 'mappingProvider'"),
            ));
        }
        if mapping.is_some() && mapping_provider.is_some() {
            return Err(throw_at(
                c.tokens(),
                ctx_start,
                JStr::from("Data space execution context cannot define both 'mapping' and 'mappingProvider'"),
            ));
        }
        out.push(Box::new(PDataSpaceContext {
            name,
            title,
            description,
            mapping,
            mapping_span,
            mapping_provider,
            default_runtime,
            runtime_span,
            test_data,
            source_information: ctx_span,
        }));
        c.match_tok(T::COMMA)?;
    }
    c.expect(T::BRACKET_CLOSE)?;
    c.expect(T::SEMI_COLON)
}

/// `parseExecutables`: `[ { id; title; description?; executable | query; executionContextKey?; sampleValues?; }, ...
/// ];` -- an id's raw token texts glued (a digit-led `2Id` lexes as two tokens), a pointer's span its statement's.
fn parse_executables<'s>(c: &mut impl Cursor<'s>, out: &mut Vec<Box<PDataSpaceExecutable>>) -> R<()> {
    c.expect(T::BRACKET_OPEN)?;
    while c.peek() != T::BRACKET_CLOSE && !c.at_end() {
        let entry_start = c.pos();
        c.expect(T::BRACE_OPEN)?;
        let mut id: Option<JStr> = None;
        let mut title = None;
        let mut description = None;
        let mut executable: Option<Vec<u16>> = None;
        let mut executable_span = None;
        let mut query = None;
        let mut context_key = None;
        let mut sample_values = None;
        let mut seen: HashSet<Vec<u16>> = HashSet::new();
        while !c.at_end() && c.peek() != T::BRACE_CLOSE {
            let key_start = c.pos();
            let key = c.parse_identifier()?;
            once_at(&mut seen, &key, c.tokens(), entry_start)?;
            c.expect(T::COLON)?;
            match String::from_utf16_lossy(&key).as_str() {
                "id" => {
                    let mut idb = Vec::new();
                    while !c.at_end() && c.peek() != T::SEMI_COLON {
                        idb.extend(c.safe_text());
                        c.advance()?;
                    }
                    id = Some(JStr(idb));
                }
                "title" => title = Some(JStr(string_value(c)?)),
                "description" => description = Some(JStr(string_value(c)?)),
                "executable" => {
                    let mut e = unquote_path_iae(&c.parse_qualified_name()?)?;
                    if c.peek() == T::PAREN_OPEN {
                        // a function pointer with its signature, kept as written
                        e.extend(raw_to_semicolon(c)?);
                    }
                    executable = Some(e);
                }
                "query" => query = Some(lambda_to_semicolon(c)?),
                "executionContextKey" => context_key = Some(JStr(string_value(c)?)),
                "sampleValues" => sample_values = Some(Box::new(parse_standalone_relation_at(c)?)),
                _ => return Err(c.error(jcat!("unknown executables key: ", key))),
            }
            c.expect(T::SEMI_COLON)?;
            if eq(&key, "executable") {
                executable_span = c.span(key_start, c.pos() - 1);
            }
        }
        c.expect(T::BRACE_CLOSE)?;
        if id.is_none() && query.is_some() {
            return Err(throw_at(c.tokens(), entry_start, JStr::from("Field 'id' is required")));
        }
        let Some(title) = title.filter(|_| executable.is_some() || query.is_some()) else {
            return Err(throw_at(
                c.tokens(),
                entry_start,
                JStr::from("an executable needs a title and an executable path or query"),
            ));
        };
        out.push(Box::new(PDataSpaceExecutable {
            id,
            title,
            description,
            executable: executable.map(JStr),
            executable_span,
            query,
            execution_context_key: context_key,
            sample_values,
            source_information: c.span(entry_start, c.pos() - 1),
        }));
        c.match_tok(T::COMMA)?;
    }
    c.expect(T::BRACKET_CLOSE)?;
    c.expect(T::SEMI_COLON)
}

/// `parseDiagrams`: `[ { title; description?; diagram; }, ... ];` -- the duplicate-key refusal at the cursor.
fn parse_diagrams<'s>(c: &mut impl Cursor<'s>, out: &mut Vec<Box<PDataSpaceDiagram>>) -> R<()> {
    c.expect(T::BRACKET_OPEN)?;
    while c.peek() != T::BRACKET_CLOSE && !c.at_end() {
        let entry_start = c.pos();
        c.expect(T::BRACE_OPEN)?;
        let mut title = None;
        let mut description = None;
        let mut diagram = None;
        let mut diagram_span = None;
        let mut seen: HashSet<Vec<u16>> = HashSet::new();
        while !c.at_end() && c.peek() != T::BRACE_CLOSE {
            let key_start = c.pos();
            let key = c.parse_identifier()?;
            once(&mut seen, &key, c)?;
            c.expect(T::COLON)?;
            if eq(&key, "title") {
                title = Some(JStr(string_value(c)?));
            } else if eq(&key, "description") {
                description = Some(JStr(string_value(c)?));
            } else if eq(&key, "diagram") {
                diagram = Some(JStr(unquote_path_iae(&c.parse_qualified_name()?)?));
            } else {
                return Err(c.error(jcat!("unknown diagrams key: ", key)));
            }
            c.expect(T::SEMI_COLON)?;
            if eq(&key, "diagram") {
                diagram_span = c.span(key_start, c.pos() - 1);
            }
        }
        c.expect(T::BRACE_CLOSE)?;
        let (Some(title), Some(diagram)) = (title, diagram) else {
            return Err(throw_at(c.tokens(), entry_start, JStr::from("a diagram entry needs title and diagram")));
        };
        out.push(Box::new(PDataSpaceDiagram {
            title,
            description,
            diagram,
            diagram_span,
            source_information: c.span(entry_start, c.pos() - 1),
        }));
        c.match_tok(T::COMMA)?;
    }
    c.expect(T::BRACKET_CLOSE)?;
    c.expect(T::SEMI_COLON)
}

/// `rawToSemicolon`: the raw source up to (not consuming) the next top-level `;`, every bracket kind counted.
fn raw_to_semicolon<'s>(c: &mut impl Cursor<'s>) -> R<Vec<u16>> {
    let bs = c.pos();
    let mut d = 0i64;
    while !c.at_end() {
        let tk = c.peek();
        match tk {
            T::PAREN_OPEN | T::BRACE_OPEN | T::BRACKET_OPEN => d += 1,
            T::PAREN_CLOSE | T::BRACE_CLOSE | T::BRACKET_CLOSE => d -= 1,
            _ => {}
        }
        if tk == T::SEMI_COLON && d <= 0 {
            break;
        }
        c.advance()?;
    }
    Ok(c.reconstruct_text(bs, c.pos()))
}

/// `rawIsland`: one `#{ ... }#` island's raw content, its `}#` expected after the text is taken.
fn raw_island<'s>(c: &mut impl Cursor<'s>) -> R<Vec<u16>> {
    let (bs, end) = scan_island(c)?;
    let raw = c.reconstruct_text(bs, end);
    c.expect(T::ISLAND_END)?;
    Ok(raw)
}

/// `RelationIslands.parseStandaloneRelationAt(host)`: `Relation #{ ... }#` as one relation element with no path line,
/// read by the Data grammar's relation reader at the host's position; its span from just past the `#{` to the rows'
/// terminating `;` one line earlier than it stands (the engine's inner walker).
pub fn parse_standalone_relation_at<'s>(host: &mut impl Cursor<'s>) -> R<PRelationElement> {
    let (re, brace_tok, end) = {
        let mut p = MappingCursor::at(host.tokens(), host.pos(), host.dialect());
        let kind_tok = p.pos;
        if !(p.peek() == T::VALID_STRING && eq(p.text(), "Relation")) {
            return Err(p.error(jcat!(
                "Data space executable sampleValues must be a standalone Relation element (e.g. sampleValues: \
                 Relation #{ ... }#), got type '",
                p.safe_text(),
                "'"
            )));
        }
        p.advance()?;
        let brace_tok = p.pos;
        let island = p.read_island()?;
        let mut rels = p.parse_relation_elements(&island, false)?;
        if rels.len() != 1 {
            return Err(throw_at(host.tokens(), kind_tok, JStr::from("sampleValues must be ONE relation element")));
        }
        (rels.remove(0), brace_tok, p.pos)
    };
    host.set_pos(end);
    let Some(es) = re.source_information.as_deref() else {
        return Err(JavaError::java(
            "java.lang.NullPointerException",
            JStr::from("a parsed node always carries its span"),
        ));
    };
    let ts = host.tokens();
    let source_information = Some(Box::new(SourceInfo {
        source_id: JStr::default(),
        start_line: ts.start_line(brace_tok) as i32,
        start_column: ts.start_column(brace_tok) as i32 + 2,
        end_line: es.end_line - 1,
        end_column: es.end_column,
    }));
    Ok(PRelationElement { columns: re.columns, paths: re.paths, rows: re.rows, source_information })
}

#[cfg(test)]
mod tests {
    use crate::doors::Dialect;
    use crate::model_parser::parse;
    use crate::records::Element;
    use crate::ustr::u;

    fn data_space(text: &str) -> crate::records::PDataSpace {
        let parsed = parse(&u(text), Dialect::LegendLite).expect("parses");
        match parsed.model.elements.into_iter().next() {
            Some(Element::PDataSpace(d)) => *d,
            other => panic!("a data space, not {other:?}"),
        }
    }

    #[test]
    fn an_id_is_its_raw_tokens_glued_and_featured_diagrams_come_last() {
        // Java's answers (front_show, 2026-11-05)
        let d = data_space(
            "###DataSpace\nDataSpace d::D\n{\n  featuredDiagrams: [ x::F ];\n  diagrams: [ { title: 't'; diagram: x::D; } ];\n  \
             executables: [ { id: 'a' b 3; title: 't'; query: |1; } ];\n}\n",
        );
        assert_eq!(d.executables.expect("executables")[0].id.as_ref().expect("an id").lossy(), "'a'b3");
        let diagrams = d.diagrams.expect("diagrams");
        assert_eq!((diagrams[0].diagram.lossy().as_str(), diagrams[1].title.lossy().as_str()), ("x::D", ""));
        assert_eq!(d.execution_contexts.map(|c| c.len()), Some(0), "an unspelled list is an empty one");
    }
}
