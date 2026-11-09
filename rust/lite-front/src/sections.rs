// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! The section grammars the model parser routes a `###` section's elements to (core/'s `parser.section`): the shared
//! helpers (`SectionParse.head`, `stringValue`, `booleanValue`, `rawStringToken`, `lowerFirst`, `specToSemicolon`,
//! `lambdaToSemicolon`; `SectionImports.parseImport`) and the tail grammars (`ElementwiseSectionGrammar`s: one element
//! at a time from its keyword, at the walk's `ElementParser`), each one arm of [`TailGrammar`]: `###Text`
//! (`TextSectionGrammar`, here), `###GenerationSpecification` and `###FileGeneration` (`generation_parser`),
//! `###Deephaven` (`deephaven_parser`), `###MongoDB` (`mongodb_parser`), `###DataQualityValidation`
//! (`data_quality_parser`), `###Elasticsearch` (`elasticsearch_parser`), `###ExternalFormat`
//! (`external_format_parser`), `###ServiceStore` (`service_store_parser`), `###DataSpace` (`dataspace_parser`),
//! `###Persistence` (`persistence_parser`), `###Service` (`service_parser`) and `###QueryPostProcessor`
//! (`query_post_processor_parser`). The grammars' SPI feed (`ElementwiseSectionGrammar.parse`, a `SliceCursor` over the
//! re-lexed section) is the overlay path's, which the model door never takes: not ported, by design.

use crate::cursor::{Cursor, Decorations, ElementCursor, R, once_at, throw_at, unquote_path_iae};
use crate::jcat;
use crate::jtypes::JStr;
use crate::lexer::TokenType;
use crate::nodes::lambda;
use crate::records::{Element, PText, ValueSpecification};
use crate::spec_parser::parse_tokens;
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
    let doc = if admits_documentation {
        c.parse_documentation()?
    } else {
        None
    };
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
    Ok(Head {
        decl_start,
        dec,
        pkg,
        name,
    })
}

/// A section element's unquoted qualified name split at its last `::` (`qn.lastIndexOf("::")`, as the Connection,
/// Runtime, Service and DataSpace grammars split it): the package and the name, `("", qn)` without one.
pub fn split_last(qn: &[u16]) -> (Vec<u16>, Vec<u16>) {
    match last_index_of_str(qn, "::") {
        None => (Vec::new(), qn.to_vec()),
        Some(cut) => (qn[..cut].to_vec(), qn[cut + 2..].to_vec()),
    }
}

/// `SectionParse.stringValue`: a STRING's decoded body, decoded after the cursor moves past it.
pub fn string_value<'s>(c: &mut impl Cursor<'s>) -> R<Vec<u16>> {
    let quoted = c.slack_text();
    c.expect(T::STRING)?;
    c.unquote_and_unescape(quoted)
}

/// `SectionParse.rawStringToken`: a STRING's raw text, its quotes included.
pub fn raw_string_token<'s>(c: &mut impl Cursor<'s>) -> R<Vec<u16>> {
    let raw = c.slack_text().to_vec();
    c.expect(T::STRING)?;
    Ok(raw)
}

/// `SectionParse.lowerFirst`: the first unit lowered (`Character.toLowerCase(kind.charAt(0))`). Its callers' kinds are
/// identifier tokens, whose first unit is ASCII (a letter, a digit, `_`, or a quoted name's `'`), where Java's case
/// mapping is ASCII's.
pub fn lower_first(kind: &[u16]) -> Vec<u16> {
    let mut out = kind.to_vec();
    if let Some(first) = out.first_mut()
        && (u16::from(b'A')..=u16::from(b'Z')).contains(first)
    {
        *first += 32;
    }
    out
}

/// `LinkedHashMap.put`: a new key appended, a known one's value replaced in its first place.
pub fn linked_put<K: PartialEq, V>(entries: &mut Vec<(K, V)>, key: K, value: V) {
    match entries.iter_mut().find(|(k, _)| *k == key) {
        Some(slot) => slot.1 = value,
        None => entries.push((key, value)),
    }
}

/// `SectionParse.booleanValue`: `true` or `false`.
pub fn boolean_value<'s>(c: &mut impl Cursor<'s>) -> R<bool> {
    if c.peek() == T::TRUE {
        c.advance()?;
        return Ok(true);
    }
    if c.peek() == T::FALSE {
        c.advance()?;
        return Ok(false);
    }
    Err(c.error(jcat!("expected true or false, got ", c.safe_text())))
}

/// `SectionParse.specToSemicolon`: a value expression riding to the next top-level `;` (every bracket kind counted,
/// the `;` not consumed), parsed by the expression parser on that slice of the stream (file-absolute spans).
pub fn spec_to_semicolon<'s>(c: &mut impl Cursor<'s>) -> R<ValueSpecification> {
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
    parse_tokens(&c.tokens().slice(bs, c.pos()), c.dialect())
}

/// `SectionParse.lambdaToSemicolon`: [`spec_to_semicolon`], a bare expression wrapped as a parameterless lambda with
/// no span (the engine's walkers call `visitLambda` unconditionally).
pub fn lambda_to_semicolon<'s>(c: &mut impl Cursor<'s>) -> R<ValueSpecification> {
    Ok(as_lambda(spec_to_semicolon(c)?))
}

/// A query or an assertion as the wire carries it: a lambda as it is, any other expression wrapped as a parameterless
/// lambda with no span (`SectionParse.lambdaToSemicolon`'s wrap, `ServiceSectionGrammar.lambda`).
pub fn as_lambda(v: ValueSpecification) -> ValueSpecification {
    match v {
        ValueSpecification::LambdaFunction(_) => v,
        other => ValueSpecification::LambdaFunction(Box::new(lambda(Vec::new(), vec![other], None))),
    }
}

/// The depth-aware island scan every grammar here repeats: past the `#...{` opener at the cursor, then to its own
/// `}#` (nested `#{`...`}#` pairs counted), not consumed -- the content's first token and the closer's (or the end):
/// each caller takes the text (`reconstructText`) and expects the `}#` in its own order.
pub fn scan_island<'s>(c: &mut impl Cursor<'s>) -> R<(usize, usize)> {
    c.advance()?;
    let start = c.pos();
    let mut depth = 0i64;
    while !c.at_end() {
        let t = c.peek();
        if t == T::ISLAND_START {
            depth += 1;
        } else if t == T::ISLAND_END {
            if depth == 0 {
                break;
            }
            depth -= 1;
        }
        c.advance()?;
    }
    Ok((start, c.pos()))
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
        return Err(c.error(jcat!(
            "an import names a package wildcard — expected '::*' (import ",
            sb,
            "::*;)"
        )));
    }
    c.expect(T::SEMI_COLON)?;
    Ok(sb)
}

/// An elementwise grammar (`ElementwiseSectionGrammar`): `parseOne` at the declaration keyword, and the qualified
/// name the section index lists the element under (`qualifiedNameOf`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TailGrammar {
    Text,
    GenerationSpecification,
    FileGeneration,
    Deephaven,
    MongoDB,
    DataQualityValidation,
    Elasticsearch,
    ExternalFormat,
    ServiceStore,
    DataSpace,
    Persistence,
    Service,
    QueryPostProcessor,
}

/// Every tail section's name: the names [`TailGrammar::of`] maps, in its order.
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

impl TailGrammar {
    /// The grammar registered for a tail section's name.
    pub fn of(section: &str) -> Option<TailGrammar> {
        Some(match section {
            "Text" => TailGrammar::Text,
            "GenerationSpecification" => TailGrammar::GenerationSpecification,
            "FileGeneration" => TailGrammar::FileGeneration,
            "Deephaven" => TailGrammar::Deephaven,
            "MongoDB" => TailGrammar::MongoDB,
            "DataQualityValidation" => TailGrammar::DataQualityValidation,
            "Elasticsearch" => TailGrammar::Elasticsearch,
            "ExternalFormat" => TailGrammar::ExternalFormat,
            "ServiceStore" => TailGrammar::ServiceStore,
            "DataSpace" => TailGrammar::DataSpace,
            "Persistence" => TailGrammar::Persistence,
            "Service" => TailGrammar::Service,
            "QueryPostProcessor" => TailGrammar::QueryPostProcessor,
            _ => return None,
        })
    }

    /// `parseOne(c)` and `qualifiedNameOf(e)`.
    pub fn parse_one(self, c: &mut ElementCursor<'_, '_>) -> R<(Element, Vec<u16>)> {
        let qn = crate::protocol::qualified_name;
        Ok(match self {
            TailGrammar::Text => {
                let t = parse_text(c)?;
                let path = qn(&t.pkg.0, &t.name.0);
                (Element::PText(Box::new(t)), path)
            }
            TailGrammar::GenerationSpecification => {
                let g = crate::generation_parser::parse_generation_specification(c)?;
                let path = qn(&g.pkg.0, &g.name.0);
                (Element::PGenerationSpecification(Box::new(g)), path)
            }
            TailGrammar::FileGeneration => {
                let f = crate::generation_parser::parse_file_generation(c)?;
                let path = qn(&f.pkg.0, &f.name.0);
                (Element::PFileGeneration(Box::new(f)), path)
            }
            TailGrammar::Deephaven => crate::deephaven_parser::parse_element(c)?,
            TailGrammar::MongoDB => {
                let m = crate::mongodb_parser::parse_database(c)?;
                let path = qn(&m.pkg.0, &m.name.0);
                (Element::PMongoDatabase(Box::new(m)), path)
            }
            TailGrammar::DataQualityValidation => crate::data_quality_parser::parse_element(c)?,
            TailGrammar::Elasticsearch => {
                let e = crate::elasticsearch_parser::parse_cluster(c)?;
                let path = qn(&e.pkg.0, &e.name.0);
                (Element::PElasticsearch7Cluster(Box::new(e)), path)
            }
            TailGrammar::ExternalFormat => crate::external_format_parser::parse_element(c)?,
            TailGrammar::ServiceStore => {
                let s = crate::service_store_parser::parse_service_store(c)?;
                let path = qn(&s.pkg.0, &s.name.0);
                (Element::PServiceStoreDefinition(Box::new(s)), path)
            }
            TailGrammar::DataSpace => {
                let d = crate::dataspace_parser::parse_data_space(c)?;
                let path = qn(&d.pkg.0, &d.name.0);
                (Element::PDataSpace(Box::new(d)), path)
            }
            TailGrammar::Persistence => crate::persistence_parser::parse_element(c)?,
            TailGrammar::Service => crate::service_parser::parse_element(c)?,
            TailGrammar::QueryPostProcessor => {
                let r = crate::query_post_processor_parser::parse_relational_mapper(c)?;
                let path = qn(&r.pkg.0, &r.name.0);
                (Element::PRelationalMapper(Box::new(r)), path)
            }
        })
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Each grammar's place in the enum: exhaustive, so a new grammar does not compile here until it is counted.
    fn ordinal(g: TailGrammar) -> usize {
        match g {
            TailGrammar::Text => 0,
            TailGrammar::GenerationSpecification => 1,
            TailGrammar::FileGeneration => 2,
            TailGrammar::Deephaven => 3,
            TailGrammar::MongoDB => 4,
            TailGrammar::DataQualityValidation => 5,
            TailGrammar::Elasticsearch => 6,
            TailGrammar::ExternalFormat => 7,
            TailGrammar::ServiceStore => 8,
            TailGrammar::DataSpace => 9,
            TailGrammar::Persistence => 10,
            TailGrammar::Service => 11,
            TailGrammar::QueryPostProcessor => 12,
        }
    }

    #[test]
    fn every_tail_grammar_is_listed_once_by_its_section_name() {
        let mut seen = [false; 13];
        for s in TAIL_SECTIONS {
            let g = TailGrammar::of(s).unwrap_or_else(|| panic!("{s} has no tail grammar"));
            assert!(!seen[ordinal(g)], "{s}'s grammar is listed twice");
            seen[ordinal(g)] = true;
        }
        assert!(seen.iter().all(|s| *s), "a tail grammar is not in TAIL_SECTIONS");
    }
}
