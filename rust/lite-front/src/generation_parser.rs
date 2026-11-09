// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! `###GenerationSpecification` and `###FileGeneration` (core/'s `parser.section.GenerationSpecificationSectionGrammar`
//! and `FileGenerationSectionGrammar`, docs/rust-port/PHASE2.md §4 step 4d): one element at a time from its keyword --
//! a generation specification's nodes (an id defaulting to the element path) and file-generation pointers; a file
//! generation's open kind keyword, its hoisted `scopeElements` and `generationOutputPath`, and its configuration
//! properties (a quoted name kept with its quotes). Java's answers reproduced, each refusal at Java's position.

// The generated records hold their record-typed list elements boxed (`Vec<Box<T>>`), so the readers build that.
#![allow(clippy::vec_box)]

use crate::cursor::{Cursor, R, is_identifier_token, once_at, throw_at, unquote_path_iae};
use crate::doors::JavaError;
use crate::jcat;
use crate::jnum::parse_long;
use crate::jtypes::JStr;
use crate::lexer::TokenType;
use crate::records::{
    PConfigProperty, PConfigValue, PConfigValuePCBoolean, PConfigValuePCInteger, PConfigValuePCMap,
    PConfigValuePCString, PConfigValuePCStrings, PFileGeneration, PGenerationNode, PGenerationSpecification, PPointer,
};
use crate::sections::{head, linked_put, lower_first, raw_string_token, split_last, string_value};
use crate::ustr::eq;
use std::collections::HashSet;

use TokenType as T;

/// `GenerationSpecificationSectionGrammar.parseOne`: `GenerationSpecification qn { generationNodes: [...];
/// fileGenerations: [...]; }`, each key once (refused at the declaration).
pub fn parse_generation_specification<'s>(c: &mut impl Cursor<'s>) -> R<PGenerationSpecification> {
    let h = head(c, "GenerationSpecification", false)?;
    c.expect(T::BRACE_OPEN)?;
    let mut nodes = Vec::new();
    let mut file_gens = Vec::new();
    let mut seen: HashSet<Vec<u16>> = HashSet::new();
    while !c.at_end() && c.peek() != T::BRACE_CLOSE {
        let key = c.parse_identifier()?;
        once_at(&mut seen, &key, c.tokens(), h.decl_start)?;
        c.expect(T::COLON)?;
        if eq(&key, "generationNodes") {
            parse_nodes(c, &mut nodes)?;
        } else if eq(&key, "fileGenerations") {
            parse_pointers(c, &mut file_gens)?;
        } else {
            return Err(c.error(jcat!("unknown GenerationSpecification key '", key, "'")));
        }
        c.expect(T::SEMI_COLON)?;
    }
    c.expect(T::BRACE_CLOSE)?;
    Ok(PGenerationSpecification {
        pkg: JStr(h.pkg),
        name: JStr(h.name),
        generation_nodes: nodes,
        file_generations: file_gens,
        source_information: c.span(h.decl_start, c.pos() - 1),
    })
}

/// `parseNodes`: `[ { id: '..'; generationElement: qn; }, ... ]`, the id the element path when unwritten.
fn parse_nodes<'s>(c: &mut impl Cursor<'s>, nodes: &mut Vec<Box<PGenerationNode>>) -> R<()> {
    c.expect(T::BRACKET_OPEN)?;
    while c.peek() != T::BRACKET_CLOSE {
        let node_start = c.pos();
        c.expect(T::BRACE_OPEN)?;
        let mut id = None;
        let mut element = None;
        while c.peek() != T::BRACE_CLOSE {
            let nk = c.parse_identifier()?;
            c.expect(T::COLON)?;
            if eq(&nk, "id") {
                id = Some(string_value(c)?);
            } else if eq(&nk, "generationElement") {
                element = Some(unquote_path_iae(&c.parse_qualified_name()?)?);
            } else {
                return Err(c.error(jcat!("unknown generation-node key '", nk, "'")));
            }
            c.expect(T::SEMI_COLON)?;
        }
        c.expect(T::BRACE_CLOSE)?;
        let Some(element) = element else {
            return Err(c.error_str("generation node needs a generationElement"));
        };
        let id = id.unwrap_or_else(|| element.clone());
        nodes.push(Box::new(PGenerationNode {
            generation_element: JStr(element),
            id: JStr(id),
            source_information: c.span(node_start, c.pos() - 1),
        }));
        if !c.match_tok(T::COMMA)? {
            break;
        }
    }
    c.expect(T::BRACKET_CLOSE)
}

/// `parsePointers`: `[ qn, ... ]`, each a `FILE_GENERATION` pointer.
fn parse_pointers<'s>(c: &mut impl Cursor<'s>, out: &mut Vec<Box<PPointer>>) -> R<()> {
    c.expect(T::BRACKET_OPEN)?;
    while c.peek() != T::BRACKET_CLOSE {
        let s = c.pos();
        let path = unquote_path_iae(&c.parse_qualified_name()?)?;
        out.push(Box::new(PPointer {
            r#type: JStr::from("FILE_GENERATION"),
            path: JStr(path),
            source_information: c.span(s, c.pos() - 1),
        }));
        if !c.match_tok(T::COMMA)? {
            break;
        }
    }
    c.expect(T::BRACKET_CLOSE)
}

/// The refusal of a reserved name whose value has another shape, at the declaration.
fn reserved<'s>(c: &impl Cursor<'s>, decl_start: usize, key: &[u16]) -> JavaError {
    throw_at(c.tokens(), decl_start, jcat!("Can't have config property with reserved name '", key, "'"))
}

/// `FileGenerationSectionGrammar.parseOne`: `Kind qn { scopeElements: [...]; generationOutputPath: '..'; name: value;
/// ... }` -- the kind any identifier (its first letter lowered on the wire), no decorations.
pub fn parse_file_generation<'s>(c: &mut impl Cursor<'s>) -> R<PFileGeneration> {
    let decl_start = c.pos();
    if !is_identifier_token(c.peek()) {
        return Err(c.error(jcat!("unsupported ###FileGeneration element: ", c.safe_text())));
    }
    let kind = c.safe_text();
    let type_span = c.span(decl_start, decl_start);
    c.advance()?;
    if c.peek() == T::LESS_THAN || c.peek() == T::BRACE_OPEN {
        return Err(c.error(jcat!("Unexpected token '", c.safe_text(), "'")));
    }
    let qn = unquote_path_iae(&c.parse_qualified_name()?)?;
    let (pkg, name) = split_last(&qn);
    c.expect(T::BRACE_OPEN)?;
    let mut output_path = None;
    let mut named_seen: HashSet<Vec<u16>> = HashSet::new();
    let mut scope_elements = Vec::new();
    let mut props = Vec::new();
    while !c.at_end() && c.peek() != T::BRACE_CLOSE {
        let key_start = c.pos();
        let key = if c.peek() == T::STRING { raw_string_token(c)? } else { c.parse_identifier()? };
        let scope = eq(&key, "scopeElements");
        let output = eq(&key, "generationOutputPath");
        if (scope || output) && !named_seen.insert(key.clone()) {
            return Err(throw_at(c.tokens(), decl_start, jcat!("Field '", key, "' should be specified only once")));
        }
        c.expect(T::COLON)?;
        if scope {
            if c.peek() != T::BRACKET_OPEN {
                return Err(reserved(c, decl_start, &key));
            }
            c.expect(T::BRACKET_OPEN)?;
            while c.peek() != T::BRACKET_CLOSE {
                scope_elements.push(JStr(unquote_path_iae(&c.parse_qualified_name()?)?));
                if !c.match_tok(T::COMMA)? {
                    break;
                }
            }
            c.expect(T::BRACKET_CLOSE)?;
            c.expect(T::SEMI_COLON)?;
            continue;
        }
        if output {
            if c.peek() != T::STRING {
                return Err(reserved(c, decl_start, &key));
            }
            output_path = Some(JStr(string_value(c)?));
            c.expect(T::SEMI_COLON)?;
            continue;
        }
        let v = parse_config_value(c)?;
        c.expect(T::SEMI_COLON)?;
        props.push(Box::new(PConfigProperty {
            name: JStr(key),
            value: v,
            source_information: c.span(key_start, c.pos() - 1),
        }));
    }
    c.expect(T::BRACE_CLOSE)?;
    Ok(PFileGeneration {
        pkg: JStr(pkg),
        name: JStr(name),
        r#type: JStr(lower_first(&kind)),
        type_source_information: type_span,
        generation_output_path: output_path,
        scope_elements,
        configuration_properties: props,
        source_information: c.span(decl_start, c.pos() - 1),
    })
}

/// `parseConfigValue`: a boolean, an integer (`Long.parseLong`, unguarded), a string, a string list or a `{k: '..';}`
/// map (a `LinkedHashMap`: a key written twice keeps its first place and its last value).
fn parse_config_value<'s>(c: &mut impl Cursor<'s>) -> R<PConfigValue> {
    match c.peek() {
        T::TRUE => {
            c.advance()?;
            Ok(PConfigValue::PConfigValuePCBoolean(Box::new(PConfigValuePCBoolean { value: true })))
        }
        T::FALSE => {
            c.advance()?;
            Ok(PConfigValue::PConfigValuePCBoolean(Box::new(PConfigValuePCBoolean { value: false })))
        }
        T::INTEGER => {
            // past a long: the engine's walker parses it uncaught too, so its section catch-all answers
            let v = parse_long(c.text()).map_err(|m| c.section_refusal(m))?;
            c.advance()?;
            Ok(PConfigValue::PConfigValuePCInteger(Box::new(PConfigValuePCInteger { value: v })))
        }
        T::STRING => Ok(PConfigValue::PConfigValuePCString(Box::new(PConfigValuePCString { value: JStr(string_value(c)?) }))),
        T::BRACKET_OPEN => {
            c.advance()?;
            let mut vs = Vec::new();
            while c.peek() != T::BRACKET_CLOSE {
                vs.push(JStr(string_value(c)?));
                if !c.match_tok(T::COMMA)? {
                    break;
                }
            }
            c.expect(T::BRACKET_CLOSE)?;
            Ok(PConfigValue::PConfigValuePCStrings(Box::new(PConfigValuePCStrings { values: vs })))
        }
        T::BRACE_OPEN => {
            c.advance()?;
            let mut entries: Vec<(JStr, JStr)> = Vec::new();
            while c.peek() != T::BRACE_CLOSE {
                let k = c.parse_identifier()?;
                c.expect(T::COLON)?;
                let v = string_value(c)?;
                linked_put(&mut entries, JStr(k), JStr(v));
                c.expect(T::SEMI_COLON)?;
            }
            c.expect(T::BRACE_CLOSE)?;
            Ok(PConfigValue::PConfigValuePCMap(Box::new(PConfigValuePCMap { entries })))
        }
        _ => Err(c.error(jcat!("unsupported config value: ", c.safe_text()))),
    }
}
