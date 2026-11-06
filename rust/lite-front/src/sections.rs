// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! The section grammars the model parser routes a `###` section's elements to (core/'s `parser.section`): the shared
//! helpers (`SectionParse.head`, `stringValue`; `SectionImports.parseImport`) and the tail grammars
//! (`ElementwiseSectionGrammar`s: one element at a time from its keyword). Ported here: `###Text`
//! (`TextSectionGrammar`). The others -- GenerationSpecification, FileGeneration, Deephaven, MongoDB,
//! DataQualityValidation, Elasticsearch, ExternalFormat, ServiceStore, DataSpace, Persistence, Service,
//! QueryPostProcessor -- are later steps', each one arm of [`TailGrammar`] (docs/rust-port/PHASE2.md §4 step 4).

use crate::cursor::{Cursor, Decorations, R, once_at, throw_at, unquote_path_iae};
use crate::doors::JavaError;
use crate::jcat;
use crate::jtypes::JStr;
use crate::lexer::TokenType;
use crate::records::{Element, PText};
use crate::ustr::{eq, last_index_of_str};
use std::collections::HashSet;

use TokenType as T;

/// `SectionParse.Head`: a section element's declaration head -- its first token, its decorations, its package and
/// name (unquoted, then split at the last `::`).
pub struct Head {
    pub decl_start: usize,
    pub dec: Decorations,
    pub pkg: Vec<u16>,
    pub name: Vec<u16>,
}

/// `SectionParse.head(c, expectedKind, admitsDocumentation)`: `documentation? Kind [decorations] qn`.
pub fn head<'s>(c: &mut impl Cursor<'s>, expected_kind: &str, admits_documentation: bool) -> R<Head> {
    let decl_start = c.pos();
    let doc = if admits_documentation { c.parse_documentation()? } else { None };
    let kind = c.safe_text();
    if !eq(&kind, expected_kind) {
        return Err(c.error(jcat!("expected ", expected_kind, ", got ", kind)));
    }
    c.advance()?;
    let decorations = c.parse_decorations()?;
    let dec = c.with_documentation(doc, decorations)?;
    let qn = unquote_path_iae(&c.parse_qualified_name()?)?;
    let (pkg, name) = match last_index_of_str(&qn, "::") {
        None => (Vec::new(), qn),
        Some(cut) => (qn[..cut].to_vec(), qn[cut + 2..].to_vec()),
    };
    Ok(Head { decl_start, dec, pkg, name })
}

/// `SectionParse.stringValue`: a STRING's decoded body, decoded after the cursor moves past it.
pub fn string_value<'s>(c: &mut impl Cursor<'s>) -> R<Vec<u16>> {
    let quoted = c.slack_text();
    c.expect(T::STRING)?;
    c.unquote_and_unescape(quoted)
}

/// `SectionImports.parseImport`: `import a::b::*;`, a wildcard required.
pub fn parse_import<'s>(c: &mut impl Cursor<'s>) -> R<Vec<u16>> {
    c.expect(T::IMPORT)?;
    let mut sb = c.parse_identifier()?;
    let mut starred = false;
    while c.match_tok(T::PATH_SEPARATOR)? {
        sb.extend(crate::ustr::u("::"));
        if c.match_tok(T::STAR)? {
            sb.push(u16::from(b'*'));
            starred = true;
            break;
        }
        sb.extend(c.parse_identifier()?);
    }
    if !starred {
        return Err(c.error(jcat!("an import names a package wildcard — expected '::*' (import ", sb, "::*;)")));
    }
    c.expect(T::SEMI_COLON)?;
    Ok(sb)
}

/// The tail sections (`PmcdParser.TAIL_SECTIONS`, in Java's order): each an elementwise grammar the strict walk hands
/// one element at a time.
pub const TAIL_SECTIONS: &[&str] = &[
    "Text",
    "GenerationSpecification",
    "FileGeneration",
    "Deephaven",
    "MongoDB",
    "DataQualityValidation",
    "Elasticsearch",
    "ExternalFormat",
    "ServiceStore",
    "DataSpace",
    "Persistence",
    "Service",
    "QueryPostProcessor",
];

/// An elementwise grammar (`ElementwiseSectionGrammar`): `parseOne` at the declaration keyword, and the qualified
/// name the section index lists the element under (`qualifiedNameOf`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TailGrammar {
    Text,
    /// A tail grammar not ported yet: its section's name.
    NotPorted(&'static str),
}

impl TailGrammar {
    /// The grammar registered for a tail section's name.
    pub fn of(section: &str) -> Option<TailGrammar> {
        let name = TAIL_SECTIONS.iter().copied().find(|s| *s == section)?;
        Some(if name == "Text" { TailGrammar::Text } else { TailGrammar::NotPorted(name) })
    }

    /// `parseOne(c)` and `qualifiedNameOf(e)`.
    pub fn parse_one<'s>(self, c: &mut impl Cursor<'s>) -> R<(Element, Vec<u16>)> {
        match self {
            TailGrammar::Text => {
                let t = parse_text(c)?;
                let path = crate::protocol::qualified_name(&t.pkg.0, &t.name.0);
                Ok((Element::PText(Box::new(t)), path))
            }
            TailGrammar::NotPorted(section) => Err(not_ported(section)),
        }
    }
}

/// The refusal for a section whose grammar is not ported yet: no Java class, so its questions differ visibly.
pub fn not_ported(section: &str) -> JavaError {
    JavaError::of("not ported", Some(&format!("{section} section")))
}

/// `TextSectionGrammar.parseOne`: `Text qn { type: T; content: '...'; }`, `type` optional, each key once (refused
/// at the declaration), `content` required.
fn parse_text<'s>(c: &mut impl Cursor<'s>) -> R<PText> {
    let h = head(c, "Text", false)?;
    c.expect(T::BRACE_OPEN)?;
    let mut r#type = None;
    let mut content = None;
    let mut seen_keys: HashSet<Vec<u16>> = HashSet::new();
    while !c.at_end() && c.peek() != T::BRACE_CLOSE {
        let key = c.parse_identifier()?;
        once_at(&mut seen_keys, &key, c.tokens(), h.decl_start)?;
        c.expect(T::COLON)?;
        if eq(&key, "type") {
            r#type = Some(JStr(c.parse_identifier()?));
        } else if eq(&key, "content") {
            content = Some(JStr(string_value(c)?));
        } else {
            return Err(c.error(jcat!("unknown Text key '", key, "'")));
        }
        c.expect(T::SEMI_COLON)?;
    }
    c.expect(T::BRACE_CLOSE)?;
    let Some(content) = content else {
        return Err(throw_at(c.tokens(), h.decl_start, JStr::from("Text needs content")));
    };
    Ok(PText {
        pkg: JStr(h.pkg),
        name: JStr(h.name),
        r#type,
        content,
        source_information: c.span(h.decl_start, c.pos() - 1),
    })
}
