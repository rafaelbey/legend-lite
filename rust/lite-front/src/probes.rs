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
//!
//! And one the prelude's generator asks (step 6b; spec/src/gen's `PreludeGenerator`, a build action that cut each
//! declaration's verbatim text with the Java lexer and `ElementParser.at`):
//!
//! - [`declaration_extents`]: every top-level declaration at its brace depth 0, its kind and its extent -- from its
//!   first token to where its kind's reader leaves the cursor.

use crate::cursor::ElementCursor;
use crate::database_parser::parse_database;
use crate::doors::{Answer, Dialect, JavaError};
use crate::emitter::emit_element;
use crate::jcat;
use crate::lexer::{TokenStream, TokenType, tokenize};
use crate::protocol::qualified_name;
use crate::records::Element;

use TokenType as T;

/// `Lexer.tokenize(text).sectionHeaders()`: every `###Section` header the lexer saw, in order -- its name, and the
/// offset (a UTF-16 index) its content starts at.
pub fn section_headers(text: &[u16]) -> Vec<(Vec<u16>, usize)> {
    tokenize(text)
        .section_headers
        .into_iter()
        .map(|h| (h.name, h.content_start_offset))
        .collect()
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
    element_sites_walk(text, dialect).map_err(JavaError::placed)
}

fn element_sites_walk(text: &[u16], dialect: Dialect) -> Answer<usize> {
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
    database_sites_walk(text, dialect).map_err(JavaError::placed)
}

fn database_sites_walk(text: &[u16], dialect: Dialect) -> Answer<Vec<(Vec<u16>, Vec<u16>)>> {
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

/// A top-level declaration [`declaration_extents`] found: its head's kind -- `Class`, `Enum`, `Profile`, `function`
/// or `native function` -- and its extent, `text[start, end)` in UTF-16 units: from its first token (its
/// documentation literal, when one comes first) to the end of the last token its kind's reader consumed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DeclarationExtent {
    pub kind: &'static str,
    pub start: usize,
    pub end: usize,
}

/// The prelude generator's cut (spec/src/gen's `PreludeGenerator`: its `functionsIn` brace scan, and its
/// `declarationText`, which parsed a class, an enumeration or a profile where it starts with `ElementParser.at`): every
/// declaration in the whole text's tokens, in token order, at brace depth 0 (braces counted, nothing else) and at a
/// declaration's position -- the stream's first token, or right after a `;` or a `}`, a documentation literal before
/// its head passing the position on and starting the declaration. Its head:
///
/// - `native function`: sliced to its `;` (`native function` with no `;` after it is an `IllegalStateException`), the
///   scan going on after the `;`;
/// - `function`: read by `parseFunctionProtocol`, the scan going on at the next token, as the generator's did (inside
///   the declaration nothing is at depth 0 and a declaration's position);
/// - `Class`, `Enum`, `Profile`, not followed by `::` (keywords are legal identifiers in Pure): read by
///   `parseClassDefinition(false)`, `parseEnumDefinition`, `parseProfileDefinition`.
///
/// Each read at `dialect` where it stands (`ElementParser.at(tokens, i, dialect)`); the first reader's error is the
/// answer. On the caller's thread, which needs a big stack (`doors::PARSE_STACK`).
pub fn declaration_extents(text: &[u16], dialect: Dialect) -> Answer<Vec<DeclarationExtent>> {
    declaration_extents_walk(text, dialect).map_err(JavaError::placed)
}

fn declaration_extents_walk(text: &[u16], dialect: Dialect) -> Answer<Vec<DeclarationExtent>> {
    let ts = tokenize(text);
    let n = ts.count();
    let mut out = Vec::new();
    let mut depth: i64 = 0;
    let mut k = 0;
    while k < n {
        let t = ts.ty(k);
        if t == T::BRACE_OPEN {
            depth += 1;
        } else if t == T::BRACE_CLOSE {
            depth -= 1;
        }
        let decl = if k > 0 && ts.ty(k - 1) == T::DOC_STRING {
            k - 1
        } else {
            k
        };
        let at_start = decl == 0 || matches!(ts.ty(decl - 1), T::SEMI_COLON | T::BRACE_CLOSE);
        if depth != 0 || !at_start {
            k += 1;
            continue;
        }
        if t == T::NATIVE && k + 1 < n && ts.ty(k + 1) == T::FUNCTION {
            let mut semi = k + 1;
            while semi < n && ts.ty(semi) != T::SEMI_COLON {
                semi += 1;
            }
            if semi >= n {
                return Err(JavaError::java(
                    "java.lang.IllegalStateException",
                    jcat!("a native declaration without ';' at token ", k),
                ));
            }
            out.push(DeclarationExtent {
                kind: "native function",
                start: ts.start(decl),
                end: ts.end(semi),
            });
            k = semi + 1;
            continue;
        }
        let path_next = k + 1 < n && ts.ty(k + 1) == T::PATH_SEPARATOR;
        let kind = match t {
            T::FUNCTION => "function",
            T::CLASS if !path_next => "Class",
            T::ENUM if !path_next => "Enum",
            T::PROFILE if !path_next => "Profile",
            _ => {
                k += 1;
                continue;
            }
        };
        let mut p = ElementCursor::at(&ts, decl, dialect);
        match t {
            T::CLASS => drop(p.parse_class_definition(false)?),
            T::ENUM => drop(p.parse_enum_definition()?),
            T::PROFILE => drop(p.parse_profile_definition()?),
            _ => drop(p.parse_function_protocol()?),
        }
        out.push(DeclarationExtent {
            kind,
            start: ts.start(decl),
            end: ts.end(p.pos - 1),
        });
        k += 1;
    }
    Ok(out)
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
        assert_eq!(
            names(&section_headers(&text)),
            vec![("Relational".to_owned(), 28), ("Pure".to_owned(), 53)]
        );
        assert!(section_headers(&u("Class a::A {}")).is_empty());
    }

    #[test]
    fn a_marker_is_a_declaration_only_where_one_can_start() {
        // the first `function` starts the stream; the one inside the class's braces is nested; `function::x` after the
        // class's `}` is a path; the last follows a `;`
        let text = u(
            "function a::f(): Integer[1] { 1 }\nClass a::A { function: String[1]; }\nfunction::x;\n\
                      function a::g(): Integer[1] { 2 }",
        );
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
        assert!(
            database_sites(&u("Class a::A { x: Database[1]; }"), Dialect::LegendEngine)
                .unwrap()
                .is_empty()
        );
    }

    fn extents(text: &str) -> Vec<(&'static str, String)> {
        let units = u(text);
        declaration_extents(&units, Dialect::LegendPlatform)
            .unwrap()
            .into_iter()
            .map(|d| (d.kind, String::from_utf16_lossy(&units[d.start..d.end])))
            .collect()
    }

    #[test]
    fn a_declaration_s_extent_runs_from_its_first_token_to_where_its_reader_stops() {
        let text = "import a::*;\n\
                    Class <<doc.deprecated>> {doc.doc = 'x'} a::A extends B\n[ c: $this.x > 0 ]\n{ x: Integer[1]; }\n\
                    '''\nthe doc\n''' Enum a::E { X, Y }\n\
                    Profile a::P { stereotypes: [s]; tags: [t]; }\n\
                    native function a::n(x: String[1]): String[1];\n\
                    function <<test.Test>> a::f(): Integer[1] { 1 }\n{ suite ( t | f() => 1; ) }\n\
                    '''\ndoc\n''' function a::g(): Integer[1] { let x = {| 2}; $x->eval(); }";
        assert_eq!(
            extents(text),
            vec![
                (
                    "Class",
                    "Class <<doc.deprecated>> {doc.doc = 'x'} a::A extends B\n[ c: $this.x > 0 ]\n{ x: Integer[1]; }"
                        .to_owned()
                ),
                ("Enum", "'''\nthe doc\n''' Enum a::E { X, Y }".to_owned()),
                ("Profile", "Profile a::P { stereotypes: [s]; tags: [t]; }".to_owned()),
                (
                    "native function",
                    "native function a::n(x: String[1]): String[1];".to_owned()
                ),
                (
                    "function",
                    "function <<test.Test>> a::f(): Integer[1] { 1 }\n{ suite ( t | f() => 1; ) }".to_owned()
                ),
                (
                    "function",
                    "'''\ndoc\n''' function a::g(): Integer[1] { let x = {| 2}; $x->eval(); }".to_owned()
                ),
            ]
        );
    }

    #[test]
    fn a_head_is_a_declaration_only_at_brace_depth_zero_and_a_declaration_s_position() {
        // a property named `function` and a profile's `Class` stereotype sit inside braces; `Class` as a type name
        // follows a `:` -- none is a declaration; `Class::x` after a `;` is a path, not a class
        let text = "Class a::A { function: String[1]; }\nProfile a::P { stereotypes: [Class]; }\n\
                    function a::f(c: Class<Any>[1]): Integer[1] { 1 }\nClass::x;";
        let found: Vec<&str> = extents(text).into_iter().map(|(k, _)| k).collect();
        assert_eq!(found, vec!["Class", "Profile", "function"]);
        // a section's header lexes to nothing: a declaration after a block ends' `}` is one, whatever the header
        assert_eq!(extents("Class a::A {}\n###Pure\nClass a::B {}").len(), 2);
    }

    #[test]
    fn a_reader_s_error_and_an_unterminated_native_are_the_answer() {
        let bad = u("Class a::A { x String[1]; }");
        let e = declaration_extents(&bad, Dialect::LegendPlatform).unwrap_err();
        assert!(e.is_parse_exception(), "{e:?}");
        let open = u("native function a::n(): String[1]");
        let e = declaration_extents(&open, Dialect::LegendPlatform).unwrap_err();
        assert_eq!(e.class, "java.lang.IllegalStateException");
        assert_eq!(
            e.message.map(|m| String::from_utf16_lossy(m.units())).as_deref(),
            Some("a native declaration without ';' at token 0")
        );
    }

    #[test]
    fn known_sections_are_the_registry_s_built_in_half() {
        let known = known_sections();
        assert_eq!(known.len(), 25, "{known:?}");
        for s in [
            "Pure",
            "Mapping",
            "Relational",
            "Data",
            "Connection",
            "Runtime",
            "Service",
            "Diagram",
            "Text",
        ] {
            assert!(known.contains(&s), "{s}");
        }
        assert!(!known.contains(&"Toy"));
    }
}
