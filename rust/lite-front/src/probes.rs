// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! What `//parser-equivalence`'s gate tests asked of the Java lexer's and parser's internals, answered by this crate
//! (docs/rust-port/PHASE2.md §2, the `//parser-equivalence` row; step 6a): once the Java lexer and parser are deleted
//! (step 6b) the tests judge the Rust parser through these, through the shim's `lite_front` doors of the same names
//! (`com.legend.parser.RustParsers`). Each is the Java the test called, ported: its order, its sites and its first
//! error are Java's.
//!
//! - [`section_headers`]: `Lexer.tokenize(text).sectionHeaders()`, each header's name and content offset
//!   (`Sectionize.plan`, which places the engine's `###` headers in a sectionless snippet);
//! - [`element_sites`]: every top-level declaration (`ElementParser.topLevelIndexes`) parsed where it stands
//!   (`ElementParser.at` and the kind's reader) -- the strict drop-in surface's second half
//!   (`RejectionParityTest.parseLegendEngine`);
//! - [`database_sites`]: every `Database` a site scan finds, parsed (`DatabaseProtocolParser.parse`) and emitted
//!   (`ProtocolEmitter.emitElement`) -- `ViewFilterParityTest`'s byte parity;
//! - [`known_sections`]: the sections the model parser reads with a grammar of its own -- `SectionGrammarRegistry`'s
//!   built-in half (`SurfaceCensusTest`).

use crate::cursor::ElementCursor;
use crate::database_parser::parse_database;
use crate::doors::{Answer, Dialect};
use crate::emitter::emit_element;
use crate::lexer::{TokenStream, TokenType, tokenize};
use crate::protocol::qualified_name;
use crate::records::Element;

use TokenType as T;

/// `Lexer.tokenize(text).sectionHeaders()`: every `###Section` header the lexer saw, in order -- its name, and the
/// offset (a UTF-16 index) its content starts at.
pub fn section_headers(text: &[u16]) -> Vec<(Vec<u16>, usize)> {
    tokenize(text).section_headers.into_iter().map(|h| (h.name, h.content_start_offset)).collect()
}

/// `ElementParser.topLevelIndexes(ts, marker)`: the indexes of `marker` tokens at bracket, brace and paren depth 0
/// where a declaration can start -- at the stream's start, after a block's `}` or after a `;` -- and not the first
/// segment of a path (`function::x`): keywords are legal identifiers in Pure.
pub fn top_level_indexes(ts: &TokenStream<'_>, marker: TokenType) -> Vec<usize> {
    let mut out = Vec::new();
    let mut depth: i64 = 0;
    for i in 0..ts.count() {
        let t = ts.ty(i);
        match t {
            T::BRACE_OPEN | T::BRACKET_OPEN | T::PAREN_OPEN => depth += 1,
            T::BRACE_CLOSE | T::BRACKET_CLOSE | T::PAREN_CLOSE => depth -= 1,
            _ => {
                if depth == 0
                    && t == marker
                    && (i == 0 || ts.ty(i - 1) == T::BRACE_CLOSE || ts.ty(i - 1) == T::SEMI_COLON)
                    && (i + 1 >= ts.count() || ts.ty(i + 1) != T::PATH_SEPARATOR)
                {
                    out.push(i);
                }
            }
        }
    }
    out
}

/// The strict drop-in surface's element sites (`RejectionParityTest.parseLegendEngine`, after the whole text's
/// parse): every top-level `Class`, `Enum`, `Profile`, `Association` and `function` -- the kinds in that order, each
/// kind's sites in token order ([`top_level_indexes`]) -- parsed where it stands in the whole text's stream
/// (`ElementParser.at(tokens, i, dialect)`) by its kind's reader: `parseClassDefinition(false)`,
/// `parseEnumDefinition`, `parseProfileDefinition`, `parseAssociationDefinition`, `parseFunctionProtocol`. How many
/// sites were parsed, or the first one's error. On the caller's thread, which needs a big stack
/// (`doors::PARSE_STACK`).
pub fn element_sites(text: &[u16], dialect: Dialect) -> Answer<usize> {
    let ts = tokenize(text);
    let mut parsed = 0;
    for marker in [T::CLASS, T::ENUM, T::PROFILE, T::ASSOCIATION, T::FUNCTION] {
        for i in top_level_indexes(&ts, marker) {
            let mut p = ElementCursor::at(&ts, i, dialect);
            match marker {
                T::CLASS => drop(p.parse_class_definition(false)?),
                T::ENUM => drop(p.parse_enum_definition()?),
                T::PROFILE => drop(p.parse_profile_definition()?),
                T::ASSOCIATION => drop(p.parse_association_definition()?),
                _ => drop(p.parse_function_protocol()?),
            }
            parsed += 1;
        }
    }
    Ok(parsed)
}

/// `ViewFilterParityTest`'s Database sites: every `Database` token at the stream's start or after a `)`, `}` or `;`,
/// in token order, parsed where it stands (`DatabaseProtocolParser.parse(tokens, i, dialect)`) and emitted as
/// legend-engine's element JSON (`ProtocolEmitter.emitElement`): each one's qualified name and JSON (a Java string's
/// UTF-16 units), or the first error -- the parser's, or the emitter's refusal. On the caller's thread, which needs a
/// big stack (`doors::PARSE_STACK`).
pub fn database_sites(text: &[u16], dialect: Dialect) -> Answer<Vec<(Vec<u16>, Vec<u16>)>> {
    let ts = tokenize(text);
    let mut out = Vec::new();
    for i in 0..ts.count() {
        if ts.ty(i) != T::DATABASE {
            continue;
        }
        if i > 0 && !matches!(ts.ty(i - 1), T::PAREN_CLOSE | T::BRACE_CLOSE | T::SEMI_COLON) {
            continue;
        }
        let (db, _) = parse_database(&ts, i, dialect)?;
        let name = qualified_name(&db.pkg.0, &db.name.0);
        let json = emit_element(&Element::PDatabase(Box::new(db)))?;
        out.push((name, json));
    }
    Ok(out)
}

/// The sections the model parser reads with a grammar of its own, whatever the dialect: `SectionGrammarRegistry`'s
/// built-in half (`SectionGrammarRegistry.all()` less the overlays the ServiceLoader registers, which stay Java's) --
/// the sections the walk routes itself, then the tail grammars'.
pub fn known_sections() -> Vec<&'static str> {
    crate::model_parser::known_sections()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ustr::u;

    fn names(v: &[(Vec<u16>, usize)]) -> Vec<(String, usize)> {
        v.iter().map(|(n, o)| (String::from_utf16_lossy(n), *o)).collect()
    }

    #[test]
    fn section_headers_name_each_header_and_where_its_content_starts() {
        let text = u("Class a::A {}\n###Relational\nDatabase a::D ()\n###Pure\nEnum a::E { X }\n");
        // a content starts after its header's line
        assert_eq!(names(&section_headers(&text)), vec![("Relational".to_owned(), 28), ("Pure".to_owned(), 53)]);
        assert!(section_headers(&u("Class a::A {}")).is_empty());
    }

    #[test]
    fn a_marker_is_a_declaration_only_where_one_can_start() {
        // the first `function` starts the stream; the one inside the class's braces is nested; `function::x` after the
        // class's `}` is a path; the last follows a `;`
        let text = u("function a::f(): Integer[1] { 1 }\nClass a::A { function: String[1]; }\nfunction::x;\n\
                      function a::g(): Integer[1] { 2 }");
        let ts = tokenize(&text);
        let sites = top_level_indexes(&ts, T::FUNCTION);
        assert_eq!(sites.len(), 2, "{sites:?}");
        assert_eq!(sites[0], 0);
        assert_eq!(ts.ty(sites[1] - 1), T::SEMI_COLON);
        assert_eq!(top_level_indexes(&ts, T::CLASS).len(), 1);
    }

    #[test]
    fn element_sites_parse_each_declaration_and_answer_the_first_error() {
        let ok = u("Class a::A { x: String[1]; }\nEnum a::E { X, Y }\nfunction a::f(): Integer[1] { 1 }");
        assert_eq!(element_sites(&ok, Dialect::LegendEngine), Ok(3));
        // the class's type parameter is the engine's refusal, the lite dialect's extension
        let generic = u("Class a::A<T> { x: T[1]; }");
        let e = element_sites(&generic, Dialect::LegendEngine).unwrap_err();
        assert!(e.is_parse_exception(), "{e:?}");
        assert_eq!(element_sites(&generic, Dialect::LegendLite), Ok(1));
        // classes are read before enumerations: the bad enumeration after a bad class does not answer
        let two = u("Enum a::E { X Y }\nClass a::A { x String[1]; }");
        let first = element_sites(&two, Dialect::LegendEngine).unwrap_err();
        assert_eq!(first.position.map(|p| p.0), Some(2), "{first:?}");
    }

    #[test]
    fn database_sites_answer_each_database_and_its_json() {
        let text = u("###Relational\nDatabase a::D ( Table T ( id INTEGER ) )\nDatabase b::E ( )\n");
        let sites = database_sites(&text, Dialect::LegendEngine).unwrap();
        let names: Vec<String> = sites.iter().map(|(n, _)| String::from_utf16_lossy(n)).collect();
        assert_eq!(names, vec!["a::D", "b::E"]);
        assert!(String::from_utf16_lossy(&sites[0].1).starts_with("{\"_type\":\"relational\""));
        // a Database word inside a body is no site
        assert!(database_sites(&u("Class a::A { x: Database[1]; }"), Dialect::LegendEngine).unwrap().is_empty());
    }

    #[test]
    fn known_sections_are_the_registry_s_built_in_half() {
        let known = known_sections();
        assert_eq!(known.len(), 25, "{known:?}");
        for s in ["Pure", "Mapping", "Relational", "Data", "Connection", "Runtime", "Service", "Diagram", "Text"] {
            assert!(known.contains(&s), "{s}");
        }
        assert!(!known.contains(&"Toy"));
    }
}
