// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! The model parser's front door (core/'s `com.legend.parser.PmcdParser.parse(text, dialect)`): a text's `###`
//! sections walked in order -- each header refused if it carries residue, each section's elements read by the grammar
//! its name routes to, its imports scoped to it -- into the records: every element in section order, then the section
//! index the engine appends (its spans in the engine's buffer coordinates), and the sections no grammar claims.
//!
//! Ported (docs/rust-port/PHASE2.md §4 step 4): the walk, the section spans and imports, the dialects' routing of an
//! unknown section, `###Pure` (`element_parser`), `###Text` (`sections`), `###Data` (`mapping_parser`),
//! `###Relational` (`database_parser`), `###Mapping` (`mapping_grammar`, `mapping_tests`). Every other grammar answers
//! `not ported` (no Java class) where the walk reaches its first element, one arm of [`parse_one_at`] or
//! [`section_elements`] each: Connection, Runtime, Service, the activator sections, Diagram, and the tail grammars but
//! Text.
//!
//! **Overlays.** A section only a registered overlay grammar (`com.legend.spi.SectionGrammar`, found by Java's
//! `ServiceLoader`) can read is the shim's: Rust cannot call the grammar, so it hands back, beside the records, each
//! overlay section's raw text and the place its elements go ([`OverlaySection`]); the shim calls the grammar and
//! splices its `POpaqueElement`s there, as Java's `OverlayElementSink` does. Rust never makes an opaque element.

// The generated records hold their record-typed list elements boxed (`Vec<Box<T>>`), so the readers build that.
#![allow(clippy::vec_box)]

use crate::cursor::{ElementCursor, R, is_identifier_token, unquote_path_iae};
use crate::database_parser::parse_database;
use crate::mapping_grammar::parse_mapping;
use crate::doors::{Answer, Dialect, JavaError};
use crate::jcat;
use crate::jtypes::JStr;
use crate::lexer::{SectionHeader, TokenStream, TokenType, tokenize};
use crate::mapping_parser::parse_data;
use crate::nodes::boxed;
use crate::protocol::{function_path, qualified_name};
use crate::records::{
    Element, PSection, PSectionIndex, PmcdParserParsed, PmcdParserUnclaimedSection, PureModelContextData, SourceInfo,
};
use crate::sections::{TailGrammar, not_ported};
use crate::ustr::{eq, last_index_of_str, u};

use TokenType as T;

/// A registered overlay grammar, as the shim names it: its section's name, and whether it lets the shared lexer read
/// its content (`SectionGrammar.lexable()`; an overlay's is false unless it says otherwise).
#[derive(Clone, Copy, Debug)]
pub struct OverlayGrammar<'a> {
    pub name: &'a [u16],
    pub lexable: bool,
}

/// A section an overlay grammar owns, for the shim to read: the grammar's `SectionSource` (the section's name, its raw
/// text `text[start_offset, end_offset)`, the line it starts on) and where its elements go -- before the answer's
/// `model.elements[element_index]` (indexes into the answer as Rust gives it: splicing the sections last-first keeps
/// each index right), their paths, each once, into the section index's `sections[section_index].elements`, which Rust
/// leaves empty.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OverlaySection {
    pub name: Vec<u16>,
    pub start_offset: usize,
    pub end_offset: usize,
    pub start_line: usize,
    pub element_index: usize,
    pub section_index: usize,
}

/// The model door's answer: the records (or Java's error), and the overlay sections the walk met -- all of them on
/// success; on an error, those before it, whose grammars Java would have called (and whose own exception would win)
/// before the error was raised -- their name, text range and line, their indexes unset (nothing is spliced).
#[derive(Debug)]
pub struct PmcdAnswer {
    pub parsed: Answer<PmcdParserParsed>,
    pub overlays: Vec<OverlaySection>,
}

/// `PmcdParser.parse(text, dialect)` with the shim's registered overlay grammars.
pub fn parse_with_overlays(text: &[u16], dialect: Dialect, overlays: &[OverlayGrammar<'_>]) -> PmcdAnswer {
    let mut met = Vec::new();
    let parsed = parse_into(text, dialect, overlays, &mut met);
    PmcdAnswer { parsed, overlays: met }
}

/// `PmcdParser.parse(text, dialect)` with no overlay grammar registered.
pub fn parse(text: &[u16], dialect: Dialect) -> Answer<PmcdParserParsed> {
    parse_with_overlays(text, dialect, &[]).parsed
}

/// The parser that reads `IMPORT_AWARE` sections (their index entry lists their imports).
const IMPORT_AWARE: &[&str] = &[
    "Pure",
    "Mapping",
    "Runtime",
    "Connection",
    "Service",
    "ExternalFormat",
    "Diagram",
    "GenerationSpecification",
    "FileGeneration",
    "HostedService",
    "QueryPostProcessor",
    "FunctionJar",
    "Persistence",
    "Deephaven",
    "Elasticsearch",
];

/// `BUILT_IN_SECTIONS`: the sections the parser routes itself, beside the tail sections.
const BUILT_IN_SECTIONS: &[&str] = &[
    "Pure",
    "Runtime",
    "Connection",
    "Relational",
    "Mapping",
    "Data",
    "Snowflake",
    "MemSql",
    "BigQuery",
    "HostedService",
    "FunctionJar",
    "Diagram",
];

/// `PLATFORM_OPEN_SECTIONS`: the sections the platform dialect reads with its own head rule.
const PLATFORM_OPEN_SECTIONS: &[&str] = &["Pure", "Mapping", "Relational", "Runtime", "Connection", "Data"];

/// `CONNECTION_FLAVORS`.
const CONNECTION_FLAVORS: &[&str] = &[
    "JsonModelConnection",
    "XmlModelConnection",
    "ModelChainConnection",
    "RelationalDatabaseConnection",
    "ServiceStoreConnection",
    "DeephavenConnection",
    "MongoDBConnection",
    "Elasticsearch7ClusterConnection",
];

/// The function-activator sections and their element kinds (`SectionGrammarRegistry`'s activator grammars).
fn activator_kinds(section: &str) -> Option<&'static [&'static str]> {
    Some(match section {
        "Snowflake" => &["SnowflakeApp", "SnowflakeM2MUdf", "SnowflakeAppDeploymentConfiguration"],
        "MemSql" => &["MemSqlFunction", "MemSqlFunctionDeploymentConfiguration"],
        "BigQuery" => &["BigQueryFunction", "BigQueryFunctionDeploymentConfiguration"],
        "HostedService" => &["HostedService", "ExecutionEnvironment", "HostedServiceDeploymentConfiguration"],
        "FunctionJar" => &["FunctionJar"],
        _ => return None,
    })
}

fn listed(list: &[&str], name: &[u16]) -> bool {
    list.iter().any(|s| eq(name, s))
}

/// Site kinds (`PmcdParser`'s): 0 Class, 1 Enum, 2 Profile, 3 Association, 4 function, 5 Measure, 6 Runtime, 7
/// Connection, 8 Database, 9 Mapping, 10 Data, 11 an activator element, 12 a tail element, and the platform's and
/// lite's own.
const PRIMITIVE_SITE: i32 = 13;
const NATIVE_SITE: i32 = 14;
const HEADERLESS_SERVICE_SITE: i32 = 15;
const SKIP: i32 = -2;

/// One parsed element (`PmcdParser.DocElement`): the path the section index lists (none for the configurations the
/// engine's walkers register as null), and the record.
struct DocElement {
    path: Option<JStr>,
    element: Element,
}

/// One section (`PmcdParser.DocSection`): its parser's name, its elements, its imports, its span; an overlay's own
/// text range and line when an overlay grammar owns it.
struct DocSection {
    parser_name: Vec<u16>,
    elements: Vec<DocElement>,
    imports: Vec<JStr>,
    span: SourceInfo,
    overlay: Option<(usize, usize, usize)>,
}

/// `PmcdParser.requireCleanHeader`: residue after a section name is the engine's refusal.
fn require_clean_header(h: &SectionHeader) -> R<()> {
    if let Some(r) = &h.residue_token {
        return Err(JavaError::parse(jcat!("Unexpected token '", r, "'"), h.residue_line, h.residue_col));
    }
    Ok(())
}

fn parse_into(
    text: &[u16],
    dialect: Dialect,
    overlays: &[OverlayGrammar<'_>],
    met: &mut Vec<OverlaySection>,
) -> Answer<PmcdParserParsed> {
    let mut unclaimed = Vec::new();
    let sections = parse_sections(text, dialect, overlays, &mut unclaimed, met)?;
    let mut elements = Vec::new();
    let mut p_sections = Vec::new();
    let mut overlay_at = 0;
    for (i, s) in sections.into_iter().enumerate() {
        let mut paths: Vec<Option<JStr>> = Vec::new();
        for e in &s.elements {
            if !paths.contains(&e.path) {
                paths.push(e.path.clone());
            }
        }
        if s.overlay.is_some() {
            met[overlay_at].element_index = elements.len();
            met[overlay_at].section_index = i;
            overlay_at += 1;
        }
        elements.extend(s.elements.into_iter().map(|e| e.element));
        p_sections.push(Box::new(PSection {
            import_aware: listed(IMPORT_AWARE, &s.parser_name),
            parser_name: JStr(s.parser_name),
            elements: paths,
            imports: s.imports,
            source_information: boxed(s.span),
        }));
    }
    elements.push(Element::PSectionIndex(Box::new(PSectionIndex {
        pkg: JStr::from("__internal__"),
        name: JStr::from("SectionIndex"),
        sections: p_sections,
    })));
    Ok(PmcdParserParsed {
        model: Box::new(PureModelContextData { elements, serializer: None, origin: None }),
        unclaimed: unclaimed.into_iter().map(Box::new).collect(),
    })
}

/// One section's bounds (`parseSections`' `Bounds`): its name, where its `###` starts (-1 for the implicit leading
/// `Pure`), where its content starts, where its name starts.
struct Bounds {
    name: Vec<u16>,
    header_start: isize,
    content_start: usize,
    name_offset: isize,
}

/// `PmcdParser.parseSections`: every header checked, then each section in order.
fn parse_sections(
    source: &[u16],
    dialect: Dialect,
    overlays: &[OverlayGrammar<'_>],
    unclaimed: &mut Vec<PmcdParserUnclaimedSection>,
    met: &mut Vec<OverlaySection>,
) -> R<Vec<DocSection>> {
    let ts = tokenize(source);
    let mut bounds = vec![Bounds { name: u("Pure"), header_start: -1, content_start: 0, name_offset: -1 }];
    for h in &ts.section_headers {
        require_clean_header(h)?;
        bounds.push(Bounds {
            name: h.name.clone(),
            header_start: h.name_offset as isize - 3,
            content_start: h.content_start_offset,
            name_offset: h.name_offset as isize,
        });
    }
    let mut out = Vec::new();
    for i in 0..bounds.len() {
        let bd = &bounds[i];
        let content_end = if i + 1 < bounds.len() { bounds[i + 1].header_start - 1 } else { source.len() as isize };
        let span = section_span(&ts, &bd.name, bd.header_start, content_end);
        let range = (bd.content_start as isize, content_end);
        let name = String::from_utf16_lossy(&bd.name);
        let section = if dialect != Dialect::LegendEngine
            && !listed(BUILT_IN_SECTIONS, &bd.name)
            && TailGrammar::of(&name).is_none()
        {
            match registered(&ts, bd, range, dialect, overlays, unclaimed)? {
                None => continue,
                Some((start, end, line)) => {
                    met.push(OverlaySection {
                        name: bd.name.clone(),
                        start_offset: start,
                        end_offset: end,
                        start_line: line,
                        element_index: 0,
                        section_index: 0,
                    });
                    DocSection {
                        parser_name: bd.name.clone(),
                        elements: Vec::new(),
                        imports: Vec::new(),
                        span,
                        overlay: Some((start, end, line)),
                    }
                }
            }
        } else {
            let elements = section_elements(&ts, &name, range, bd.header_start, dialect)?;
            // (a Diagram section's imports are its raw grammar's: not reached, its elements refuse first)
            let imports = if listed(IMPORT_AWARE, &bd.name) { section_imports(&ts, range)? } else { Vec::new() };
            DocSection { parser_name: bd.name.clone(), elements, imports, span, overlay: None }
        };
        out.push(section);
    }
    Ok(out)
}

/// `PmcdParser.registered`: a section no built-in grammar reads, at lite's or the platform's dialect -- an overlay's
/// (its text range and line), or none: lite refuses it by name, the platform records it unclaimed (`None`).
fn registered(
    ts: &TokenStream<'_>,
    bd: &Bounds,
    r: (isize, isize),
    dialect: Dialect,
    overlays: &[OverlayGrammar<'_>],
    unclaimed: &mut Vec<PmcdParserUnclaimedSection>,
) -> R<Option<(usize, usize, usize)>> {
    let skipped = ts.skipped_sections.iter().rev().find(|sk| sk.name_offset as isize == bd.name_offset);
    let name_offset = bd.name_offset as usize;
    let grammar = overlays.iter().rev().find(|g| g.name == bd.name.as_slice());
    let Some(grammar) = grammar else {
        if dialect.refuses_platform_dialect() {
            return Err(JavaError::parse(
                jcat!("'", bd.name, "' is not a known section parser"),
                ts.line_of(name_offset) as i32,
                ts.column_of(name_offset) as i32,
            ));
        }
        let (start, end) = match skipped {
            Some(sk) => (sk.start_offset as i32, sk.end_offset as i32),
            None => (r.0 as i32, r.1 as i32),
        };
        unclaimed.push(PmcdParserUnclaimedSection {
            name: JStr(bd.name.clone()),
            start_offset: start,
            end_offset: end,
        });
        return Ok(None);
    };
    let Some(sk) = skipped else {
        // an SPI grammar is never one of the parser's own lexable grammars
        return Err(JavaError::java(
            "java.lang.IllegalStateException",
            jcat!("section '", bd.name, "' was lexed but its grammar is not lexable"),
        ));
    };
    if grammar.lexable {
        return Err(JavaError::parse(
            jcat!(
                "section '",
                bd.name,
                "' has a lexable grammar registered but was raw-skipped by the lexer — lexer/registry drift; add it \
                 to Lexer.lexableSections()"
            ),
            ts.line_of(name_offset) as i32,
            ts.column_of(name_offset) as i32,
        ));
    }
    Ok(Some((sk.start_offset, sk.end_offset, ts.line_of(sk.start_offset))))
}

/// `PmcdParser.sectionSpan`: a section's span in the engine's buffer `"\n###Pure\n" + source` (a line two down, a
/// section starting at its header's own newline, ending at its last content character; an empty one its header
/// token).
fn section_span(ts: &TokenStream<'_>, name: &[u16], header_start: isize, content_end: isize) -> SourceInfo {
    let si = |sl: i32, sc: i32, el: i32, ec: i32| SourceInfo {
        source_id: JStr::default(),
        start_line: sl,
        start_column: sc,
        end_line: el,
        end_column: ec,
    };
    let (start_line, start_col) = if header_start < 0 {
        (1, 1)
    } else if header_start == 0 {
        (2, 8)
    } else {
        let nl = (header_start - 1) as usize;
        (ts.line_of(nl) as i32 + 2, ts.column_of(nl) as i32)
    };
    let last_content = content_end - 1;
    let first_content = if header_start < 0 { 0 } else { header_start + 3 + name.len() as isize };
    if header_start < 0 && last_content < 0 {
        return if last_content == -1 { si(1, 1, 2, 8) } else { si(1, 1, 1, 8) };
    }
    if last_content < first_content {
        let len = 4 + name.len() as i32;
        if header_start == 0 {
            return si(2, 8, 2, 7 + len);
        }
        return si(start_line, start_col, start_line, start_col + len - 1);
    }
    let lc = last_content as usize;
    si(start_line, start_col, ts.line_of(lc) as i32 + 2, ts.column_of(lc) as i32)
}

/// `skipTo`: the first token starting at or after `offset` (a binary search; token starts are sorted).
fn skip_to(ts: &TokenStream<'_>, offset: isize) -> usize {
    let (mut lo, mut hi) = (0usize, ts.count());
    while lo < hi {
        let mid = (lo + hi) >> 1;
        if (ts.start(mid) as isize) < offset {
            lo = mid + 1;
        } else {
            hi = mid;
        }
    }
    lo
}

/// `stripWildcard`: `a::b::*` is `a::b`.
fn strip_wildcard(imp: &[u16]) -> &[u16] {
    match last_index_of_str(imp, "::") {
        Some(cut) if eq(&imp[cut + 2..], "*") => &imp[..cut],
        _ => imp,
    }
}

/// `PmcdParser.sectionImports`: the leading `import ...;` statements' paths (their token texts joined, the wildcard
/// dropped, unquoted), each once, first kept.
fn section_imports(ts: &TokenStream<'_>, r: (isize, isize)) -> R<Vec<JStr>> {
    let mut out: Vec<JStr> = Vec::new();
    let mut cursor = skip_to(ts, r.0);
    while cursor < ts.count() && (ts.start(cursor) as isize) < r.1 && ts.ty(cursor) == T::IMPORT {
        let mut p: Vec<u16> = Vec::new();
        cursor += 1;
        while cursor < ts.count() && ts.ty(cursor) != T::SEMI_COLON {
            p.extend_from_slice(ts.text(cursor));
            cursor += 1;
        }
        cursor += 1;
        let imp = JStr(unquote_path_iae(strip_wildcard(&p))?);
        if !out.contains(&imp) {
            out.push(imp);
        }
    }
    Ok(out)
}

/// A section's head rule (`PmcdParser.HeadRule`): the site kind of the declaration at a token, or -1.
#[derive(Clone, Copy)]
enum HeadRule<'a> {
    Platform,
    Domain,
    Runtime,
    Connection,
    Relational,
    Mapping,
    Data,
    Activator(&'a [&'a str]),
    Tail,
}

impl HeadRule<'_> {
    fn kind_of(self, ts: &TokenStream<'_>, c: usize) -> i32 {
        match self {
            HeadRule::Platform => platform_head(ts, c),
            HeadRule::Domain => domain_head(ts, c),
            HeadRule::Runtime => {
                if matches!(ts.ty(c), T::RUNTIME | T::SINGLE_CONNECTION_RUNTIME) {
                    6
                } else {
                    -1
                }
            }
            HeadRule::Connection => {
                if listed(CONNECTION_FLAVORS, ts.text(c)) {
                    7
                } else {
                    -1
                }
            }
            HeadRule::Relational => {
                if ts.ty(c) == T::DATABASE
                    || (ts.ty(c) == T::DOC_STRING && c + 1 < ts.count() && ts.ty(c + 1) == T::DATABASE)
                {
                    8
                } else {
                    -1
                }
            }
            HeadRule::Mapping => {
                if ts.ty(c) == T::MAPPING {
                    9
                } else {
                    -1
                }
            }
            HeadRule::Data => {
                if eq(ts.text(c), "Data")
                    || (ts.ty(c) == T::DOC_STRING && c + 1 < ts.count() && eq(ts.text(c + 1), "Data"))
                {
                    10
                } else {
                    -1
                }
            }
            HeadRule::Activator(kinds) => {
                if listed(kinds, ts.text(c)) {
                    11
                } else {
                    -1
                }
            }
            HeadRule::Tail => {
                if is_identifier_token(ts.ty(c)) && (c + 1 >= ts.count() || ts.ty(c + 1) != T::PATH_SEPARATOR) {
                    12
                } else {
                    -1
                }
            }
        }
    }
}

/// `PmcdParser.MARKERS`.
fn marker(t: TokenType) -> Option<i32> {
    Some(match t {
        T::CLASS => 0,
        T::ENUM => 1,
        T::PROFILE => 2,
        T::ASSOCIATION => 3,
        T::FUNCTION => 4,
        _ => return None,
    })
}

/// `pureHead`: `###Pure`'s markers (not followed by `::`), and `Measure`; a documentation literal read through.
fn pure_head(ts: &TokenStream<'_>, cursor: usize) -> i32 {
    let t = ts.ty(cursor);
    if t == T::DOC_STRING && cursor + 1 < ts.count() {
        return pure_head(ts, cursor + 1);
    }
    if let Some(kind) = marker(t)
        && (cursor + 1 >= ts.count() || ts.ty(cursor + 1) != T::PATH_SEPARATOR)
    {
        return kind;
    }
    if t == T::VALID_STRING && eq(ts.text(cursor), "Measure") {
        return 5;
    }
    -1
}

/// `domainHead`: `###Pure` at the engine's and lite's dialects -- the engine's declarations, and `native` and
/// `Primitive` recognized so that their parsers refuse them by name.
fn domain_head(ts: &TokenStream<'_>, cursor: usize) -> i32 {
    let kind = pure_head(ts, cursor);
    if kind >= 0 {
        return kind;
    }
    if ts.ty(cursor) == T::NATIVE {
        return NATIVE_SITE;
    }
    if is_identifier_token(ts.ty(cursor)) && eq(ts.text(cursor), "Primitive") { PRIMITIVE_SITE } else { -1 }
}

/// `platformHead`: any declaration lite's compiler reads, in any of its built-in sections.
fn platform_head(ts: &TokenStream<'_>, cursor: usize) -> i32 {
    let t = ts.ty(cursor);
    if t == T::DOC_STRING && cursor + 1 < ts.count() {
        return platform_head(ts, cursor + 1);
    }
    let kind = pure_head(ts, cursor);
    if kind >= 0 {
        return kind;
    }
    match t {
        T::NATIVE => return NATIVE_SITE,
        T::SERVICE => return HEADERLESS_SERVICE_SITE,
        T::RUNTIME | T::SINGLE_CONNECTION_RUNTIME => return 6,
        T::RELATIONAL_DATABASE_CONNECTION => return 7,
        T::DATABASE => return 8,
        T::MAPPING => return 9,
        T::NEW_SYMBOL | T::PAREN_CLOSE => return SKIP,
        _ => {}
    }
    if is_identifier_token(t) {
        let word = ts.text(cursor);
        if listed(CONNECTION_FLAVORS, word) {
            return 7;
        }
        if eq(word, "Primitive") {
            return PRIMITIVE_SITE;
        }
        if eq(word, "Data") {
            return 10;
        }
        if eq(word, "Diagram") {
            return SKIP;
        }
    }
    -1
}

/// `PmcdParser.sectionElements`: a section's elements, by the grammar its name routes to.
fn section_elements(
    ts: &TokenStream<'_>,
    name: &str,
    r: (isize, isize),
    header_start: isize,
    dialect: Dialect,
) -> R<Vec<DocElement>> {
    let imports_ok = IMPORT_AWARE.contains(&name);
    // the aggLambdaShift anchor: the header's line + 1 (the engine sub-parses content from the header's newline)
    let header_line = ts.line_of(header_start.max(0) as usize) as i64 + 1;
    let walk = |rule: HeadRule<'_>, heads: Option<&mut Vec<TokenType>>, line: i64| {
        strict_walk(ts, r, rule, imports_ok, line, heads, name, dialect)
    };
    if !dialect.refuses_platform_dialect() && PLATFORM_OPEN_SECTIONS.contains(&name) {
        return walk(HeadRule::Platform, None, if name == "Mapping" { header_line } else { -1 });
    }
    match name {
        "Pure" => walk(HeadRule::Domain, None, -1),
        "Runtime" => {
            // the engine's runtime walker collects every `Runtime` first, then every `SingleConnectionRuntime`
            let mut heads = Vec::new();
            let parsed = walk(HeadRule::Runtime, Some(&mut heads), -1)?;
            let (mut first, mut rest) = (Vec::new(), Vec::new());
            for (e, h) in parsed.into_iter().zip(heads) {
                if h == T::RUNTIME { first.push(e) } else { rest.push(e) }
            }
            first.extend(rest);
            Ok(first)
        }
        "Connection" => walk(HeadRule::Connection, None, -1),
        "Relational" => walk(HeadRule::Relational, None, -1),
        "Mapping" => walk(HeadRule::Mapping, None, header_line),
        "Data" => walk(HeadRule::Data, None, -1),
        "Diagram" => Err(not_ported("Diagram")),
        _ => {
            if let Some(kinds) = activator_kinds(name) {
                return walk(HeadRule::Activator(kinds), None, -1);
            }
            if TailGrammar::of(name).is_none() {
                return Err(JavaError::parse(
                    jcat!("'", name, "' is not a known section parser"),
                    ts.line_of(r.0 as usize) as i32,
                    1,
                ));
            }
            Ok(rule_group(name, walk(HeadRule::Tail, None, -1)?))
        }
    }
}

/// `ruleGroup`: the engine's walkers collect by grammar rule where a section owns several element kinds -- schema
/// sets before bindings, services before execution environments.
fn rule_group(parser_name: &str, parsed: Vec<DocElement>) -> Vec<DocElement> {
    let first_kind: fn(&Element) -> bool = match parser_name {
        "ExternalFormat" => |e| matches!(e, Element::PSchemaSet(_)),
        "Service" => |e| matches!(e, Element::PService(_)),
        _ => return parsed,
    };
    let (mut first, rest): (Vec<_>, Vec<_>) = parsed.into_iter().partition(|e| first_kind(&e.element));
    first.extend(rest);
    first
}

/// `PmcdParser.orphan`: a token no declaration starts with, refused at its start -- the column in UTF-16 units
/// (`startColumn`), not the code points other refusals count.
fn orphan(ts: &TokenStream<'_>, cursor: usize) -> JavaError {
    JavaError::parse(
        jcat!("Unexpected token '", ts.text(cursor), "'"),
        ts.line_of(ts.start(cursor)) as i32,
        ts.start_column(cursor) as i32,
    )
}

/// `PmcdParser.strictWalk`: a section's range read in order -- its leading imports (import-aware sections only, each
/// exactly `import pkg(::pkg)*::*;`), then one element after another, each consuming its own extent; anything else
/// refused. `mapping_section_line` is a mapping's anchor line (-1 outside a `###Mapping` section).
#[allow(clippy::too_many_arguments)]
fn strict_walk(
    ts: &TokenStream<'_>,
    r: (isize, isize),
    rule: HeadRule<'_>,
    imports_allowed: bool,
    mapping_section_line: i64,
    mut heads_out: Option<&mut Vec<TokenType>>,
    section: &str,
    dialect: Dialect,
) -> R<Vec<DocElement>> {
    let mut out = Vec::new();
    let mut cursor = skip_to(ts, r.0);
    let mut saw_element = false;
    let n = ts.count();
    while cursor < n && (ts.start(cursor) as isize) < r.1 {
        let t = ts.ty(cursor);
        if t == T::IMPORT {
            if !imports_allowed || saw_element {
                return Err(orphan(ts, cursor));
            }
            cursor += 1;
            let mut wildcard = false;
            while cursor < n && ts.ty(cursor) != T::SEMI_COLON {
                let it = ts.ty(cursor);
                if it == T::STAR {
                    wildcard = true;
                } else if it != T::PATH_SEPARATOR && !is_identifier_token(it) {
                    return Err(orphan(ts, cursor));
                }
                cursor += 1;
            }
            if !wildcard || cursor >= n {
                return Err(orphan(ts, cursor.min(n - 1)));
            }
            cursor += 1;
            continue;
        }
        if t == T::STRING && cursor + 1 < n && rule.kind_of(ts, cursor + 1) >= 0 {
            return Err(JavaError::parse(
                JStr::from(crate::cursor::DOC_MULTI_LINE),
                ts.line_of(ts.start(cursor)) as i32,
                ts.start_column(cursor) as i32,
            ));
        }
        let head = if t == T::DOC_STRING && cursor + 1 < n { cursor + 1 } else { cursor };
        let kind = rule.kind_of(ts, head);
        if kind == SKIP {
            let mut p = ElementCursor::at(ts, cursor, dialect);
            if !p.skip_top_level_non_element()? {
                return Err(orphan(ts, cursor));
            }
            saw_element = true;
            cursor = p.pos;
            continue;
        }
        if kind < 0 {
            return Err(orphan(ts, cursor));
        }
        saw_element = true;
        if let Some(h) = heads_out.as_deref_mut() {
            h.push(ts.ty(head));
        }
        let (de, end) = parse_one_at(ts, cursor, kind, mapping_section_line, section, dialect)?;
        if let Some(de) = de {
            out.push(de);
        }
        cursor = end;
    }
    Ok(out)
}

/// `PmcdParser.parseOneAt`: the element of `kind` at `site`, the path the section index lists it under, and the
/// token where it ended.
fn parse_one_at(
    ts: &TokenStream<'_>,
    site: usize,
    kind: i32,
    mapping_section_line: i64,
    section: &str,
    dialect: Dialect,
) -> R<(Option<DocElement>, usize)> {
    let mut p = ElementCursor::at(ts, site, dialect);
    let (element, path) = match kind {
        PRIMITIVE_SITE => {
            let el = p.parse_primitive_protocol()?;
            let path = el.qualified_name.0.clone();
            (Element::PPrimitive(Box::new(el)), path)
        }
        NATIVE_SITE => {
            let el = p.parse_native_protocol()?;
            let path = match &el {
                Element::PClass(c) => qualified_name(&c.pkg.0, &c.name.0),
                Element::PNativeFunction(f) => f.qualified_name.0.clone(),
                _ => unreachable!("a native declaration is a class or a function"),
            };
            (el, path)
        }
        HEADERLESS_SERVICE_SITE => return Err(not_ported("Service")),
        0 => {
            let c = p.parse_class_definition(false)?;
            let path = qualified_name(&c.pkg.0, &c.name.0);
            (Element::PClass(Box::new(c)), path)
        }
        1 => {
            let e = p.parse_enum_definition()?;
            let path = qualified_name(&e.pkg.0, &e.name.0);
            (Element::PEnumeration(Box::new(e)), path)
        }
        2 => {
            let pr = p.parse_profile_definition()?;
            let path = qualified_name(&pr.pkg.0, &pr.name.0);
            (Element::PProfile(Box::new(pr)), path)
        }
        3 => {
            let el = p.parse_association_definition()?;
            let path = match &el {
                Element::PAssociation(a) => qualified_name(&a.pkg.0, &a.name.0),
                Element::PAssociationProjection(a) => a.qualified_name.0.clone(),
                _ => unreachable!("an association or its projection"),
            };
            (el, path)
        }
        4 => {
            let f = p.parse_function_protocol()?;
            let path = function_path(&f);
            (Element::PFunction(Box::new(f)), path)
        }
        5 => {
            let m = p.parse_measure_definition()?;
            let path = qualified_name(&m.pkg.0, &m.name.0);
            (Element::PMeasure(Box::new(m)), path)
        }
        6 => return Err(not_ported("Runtime")),
        7 => return Err(not_ported("Connection")),
        8 => {
            let (db, end) = parse_database(ts, site, dialect)?;
            let path = qualified_name(&db.pkg.0, &db.name.0);
            return Ok((Some(DocElement { path: Some(JStr(path)), element: Element::PDatabase(Box::new(db)) }), end));
        }
        9 => {
            // a headerless mapping (the platform dialect) anchors to its enclosing ###Mapping header, if any
            let line = if mapping_section_line >= 0 {
                mapping_section_line
            } else {
                ts.section_content_line("Mapping", ts.start(site))
            };
            let (m, end) = parse_mapping(ts, site, line, dialect)?;
            let path = qualified_name(&m.pkg.0, &m.name.0);
            return Ok((Some(DocElement { path: Some(JStr(path)), element: Element::PMapping(Box::new(m)) }), end));
        }
        10 => {
            let (d, end) = parse_data(ts, site, dialect)?;
            let path = qualified_name(&d.pkg.0, &d.name.0);
            return Ok((Some(DocElement { path: Some(JStr(path)), element: Element::PDataElement(Box::new(d)) }), end));
        }
        11 => return Err(not_ported(section)),
        12 => {
            let grammar = TailGrammar::of(section).expect("a tail section has its grammar");
            grammar.parse_one(&mut p)?
        }
        _ => {
            return Err(JavaError::java("java.lang.IllegalStateException", jcat!("unknown site kind ", kind)));
        }
    };
    Ok((Some(DocElement { path: Some(JStr(path)), element }), p.pos))
}

#[cfg(test)]
mod tests {
    use super::*;

    const TOY: &str = "Class my::A { a: String[1]; }\n\n###Toy\nToy my::toys::T1;\n";

    #[test]
    fn an_overlay_section_is_handed_back_where_its_elements_go() {
        let text = u(TOY);
        let toy = u("Toy");
        let a = parse_with_overlays(&text, Dialect::LegendLite, &[OverlayGrammar { name: &toy, lexable: false }]);
        let parsed = a.parsed.expect("parses");
        assert_eq!(parsed.model.elements.len(), 2, "the class and the section index; no opaque element");
        let o = &a.overlays[..];
        assert_eq!(o.len(), 1);
        assert_eq!((o[0].element_index, o[0].section_index, o[0].start_line), (1, 1, 4));
        assert_eq!(&text[o[0].start_offset..o[0].end_offset], &u("Toy my::toys::T1;\n")[..]);
        let Element::PSectionIndex(index) = &parsed.model.elements[1] else { panic!("the index is last") };
        assert!(index.sections[1].elements.is_empty() && index.sections[1].parser_name == "Toy");
    }

    #[test]
    fn an_unknown_section_is_each_dialect_s_own() {
        let text = u(TOY);
        let engine = parse(&text, Dialect::LegendEngine).unwrap_err();
        assert_eq!(engine.message, Some(JStr::from("[4:1] 'Toy' is not a known section parser")));
        let lite = parse(&text, Dialect::LegendLite).unwrap_err();
        assert_eq!(lite.position, Some((3, 4)));
        let platform = parse(&text, Dialect::LegendPlatform).expect("records it unclaimed");
        assert_eq!(platform.unclaimed.len(), 1);
        assert_eq!(platform.unclaimed[0].name, "Toy");
    }

    #[test]
    fn a_grammar_not_ported_refuses_where_the_walk_reaches_it() {
        let refused = parse(&u("Class A {}\n###Runtime\nRuntime r {}\n"), Dialect::LegendLite).unwrap_err();
        assert_eq!(refused.class, "not ported");
        let earlier = parse(&u("Class A {\n###Runtime\nRuntime r {}\n"), Dialect::LegendLite).unwrap_err();
        assert!(earlier.is_parse_exception(), "Java's error before the section is Java's");
        assert!(parse(&u("###Runtime\n"), Dialect::LegendLite).is_ok(), "an empty section parses");
    }
}
