// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! The parser's text doors (core/'s `PmcdParser.parse`, `SpecParser.parse`, `parseLambda`, `parseCodeBlock`) and
//! what they answer: the records, or the error Java raises -- its class, message and, for a `ParseException`, its
//! line and column -- which the shim rethrows as that class (docs/rust-port/PHASE2.md §3). Each door is held to the
//! Java parser's answer for every question of the front oracle (`front_parser_differential_test`), in the canonical
//! form `answer_form` writes (Java's `FrontAnswers.canonical`).

use crate::codec;
use crate::jtypes::JStr;
use crate::model_parser::{OverlayGrammar, PmcdAnswer};
use crate::records::{Element, LambdaFunction, PmcdParserParsed, SourceInfo, ValueSpecification};
use crate::spec_parser;

/// The parse's level (Java's `parser.Dialect`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Dialect {
    LegendPlatform,
    LegendLite,
    LegendEngine,
}

impl Dialect {
    pub fn from_java(name: &str) -> Option<Dialect> {
        match name {
            "LEGEND_PLATFORM" => Some(Dialect::LegendPlatform),
            "LEGEND_LITE" => Some(Dialect::LegendLite),
            "LEGEND_ENGINE" => Some(Dialect::LegendEngine),
            _ => None,
        }
    }

    pub fn java_name(self) -> &'static str {
        match self {
            Dialect::LegendPlatform => "LEGEND_PLATFORM",
            Dialect::LegendLite => "LEGEND_LITE",
            Dialect::LegendEngine => "LEGEND_ENGINE",
        }
    }

    /// The platform dialect (m3/m2 constructs) refuses here (Java's `refusesPlatformDialect`).
    pub fn refuses_platform_dialect(self) -> bool {
        self != Dialect::LegendPlatform
    }

    /// The declared lite-design extensions refuse here (Java's `refusesLiteExtensions`).
    pub fn refuses_lite_extensions(self) -> bool {
        self == Dialect::LegendEngine
    }
}

/// An error a door raises, as Java raises it: the Java class to throw, its message (Java's `getMessage`, `None` for
/// null), and -- for `com.legend.parser.ParseException` -- its line and column (0 when unknown).
#[derive(Clone, Debug, PartialEq)]
pub struct JavaError {
    pub class: String,
    pub message: Option<JStr>,
    pub position: Option<(i32, i32)>,
}

pub const PARSE_EXCEPTION: &str = "com.legend.parser.ParseException";

/// A refusal of malformed input that the Java parser answered with a raw JDK exception and legend-engine refuses: the
/// user's ruling of 2026-11-09 (docs/rust-port/PHASE2.md section 6, ruling 4; rust/lite-front/PARITY.md, the rows it
/// names). In flight it is neither a `ParseException` nor a JDK class: the places Java recovers (`catch
/// (ParseException e)`, [`JavaError::is_parse_exception`]) pass it on, as they passed the exception it replaces, so no
/// text that was refused becomes accepted. The doors make it the `ParseException` it is ([`JavaError::placed`]).
const REFUSED: &str = "lite.front.Refused";

/// A [`REFUSED`] whose place is legend-engine's own catch-all: `PureGrammarParser.visitSection` answers any exception a
/// section's walker lets out at the section's start. The walk ([`JavaError::at_section`]) knows the section; a door with
/// no section ([`JavaError::placed`]) places it at the token that held it, or the text's start when no token is known.
const REFUSED_AT_SECTION: &str = "lite.front.RefusedAtSection";

impl JavaError {
    /// A `ParseException` at a position: its message carries Java's `[line:col] ` prefix when `line > 0`.
    pub fn parse_at(message: &str, line: i32, column: i32) -> JavaError {
        let text = if line > 0 { format!("[{line}:{column}] {message}") } else { message.to_owned() };
        JavaError { class: PARSE_EXCEPTION.to_owned(), message: Some(JStr::from(text)), position: Some((line, column)) }
    }

    /// Another class's error (no position): `IllegalStateException`, `IllegalArgumentException`, ...
    pub fn of(class: &str, message: Option<&str>) -> JavaError {
        JavaError { class: class.to_owned(), message: message.map(JStr::from), position: None }
    }

    /// A `ParseException(message, line, column)` (Java's three-argument constructor) over a message's UTF-16 units:
    /// the `[line:col] ` prefix when `line > 0`, the bare message otherwise.
    pub fn parse(message: JStr, line: i32, column: i32) -> JavaError {
        let text = if line > 0 {
            let mut t: Vec<u16> = format!("[{line}:{column}] ").encode_utf16().collect();
            t.extend_from_slice(message.units());
            JStr(t)
        } else {
            message
        };
        JavaError { class: PARSE_EXCEPTION.to_owned(), message: Some(text), position: Some((line, column)) }
    }

    /// A `ParseException(message)`, Java's one-argument constructor: no position (line and column 0).
    pub fn parse_unplaced(message: JStr) -> JavaError {
        JavaError { class: PARSE_EXCEPTION.to_owned(), message: Some(message), position: Some((0, 0)) }
    }

    /// Another class's error over a message's UTF-16 units.
    pub fn java(class: &str, message: JStr) -> JavaError {
        JavaError { class: class.to_owned(), message: Some(message), position: None }
    }

    /// Whether this is a `ParseException` (the class Java's `catch (ParseException e)` takes).
    pub fn is_parse_exception(&self) -> bool {
        self.class == PARSE_EXCEPTION
    }

    /// This `ParseException` as a refusal in flight ([`REFUSED`]): its place and its text kept.
    pub fn as_refusal(mut self) -> JavaError {
        self.class = REFUSED.to_owned();
        self
    }

    /// A refusal the engine answers from its section catch-all ([`REFUSED_AT_SECTION`]); `at` is the token that held
    /// it (`(0, 0)` when the site knows none), for a door with no section.
    pub fn refused_at_section(message: JStr, at: (i32, i32)) -> JavaError {
        JavaError { class: REFUSED_AT_SECTION.to_owned(), message: Some(message), position: Some(at) }
    }

    /// A section catch-all's refusal, its token named now that the caller knows it.
    pub fn held_at(mut self, at: (i32, i32)) -> JavaError {
        if self.class == REFUSED_AT_SECTION && self.position == Some((0, 0)) {
            self.position = Some(at);
        }
        self
    }

    /// The walk's section is the refusal's place, where the engine's catch-all puts it: the section's start.
    pub fn at_section(self, span: &SourceInfo) -> JavaError {
        if self.class != REFUSED_AT_SECTION {
            return self;
        }
        let message = self.message.unwrap_or_default();
        JavaError::parse(message, span.start_line, span.start_column).as_refusal()
    }

    /// What a door answers: a refusal in flight as the `ParseException` it is; every other error as it stands.
    pub fn placed(self) -> JavaError {
        if self.class == REFUSED {
            return JavaError { class: PARSE_EXCEPTION.to_owned(), ..self };
        }
        if self.class == REFUSED_AT_SECTION {
            let (line, column) = match self.position {
                Some((l, c)) if l > 0 => (l, c),
                _ => (1, 1),
            };
            return JavaError::parse(self.message.unwrap_or_default(), line, column);
        }
        self
    }
}

pub type Answer<T> = Result<T, JavaError>;

/// The stack a door's parse needs: the parser recurses through every nested expression, as Java's does (whose
/// answers hold no `StackOverflowError`), and a Rust frame is larger than a JVM's (docs/rust-port/PHASE2.md §3,
/// "Recursion depth"). The doors parse on the caller's thread -- the shim's calls run on the calling thread's
/// long-lived worker in `lite-ffi` (since `5e5d7e118`), whose stack is at least this size (`WORKER_STACK_BYTES`, the
/// deeper of this and the dialect calls' `DIALECT_STACK_BYTES`) -- and [`answer_form`] on one of this size.
pub const PARSE_STACK: usize = 512 << 20;

/// `work` on a thread of its own with [`PARSE_STACK`] of stack, joined before this returns; a panic re-raised here.
pub fn on_parse_stack<T: Send>(work: impl FnOnce() -> T + Send) -> T {
    std::thread::scope(|scope| {
        let handle = std::thread::Builder::new()
            .name("lite-front-parse".to_owned())
            .stack_size(PARSE_STACK)
            .spawn_scoped(scope, work)
            .expect("the parse thread starts");
        match handle.join() {
            Ok(v) => v,
            Err(panic) => std::panic::resume_unwind(panic),
        }
    })
}

/// `SpecParser.parse(text, dialect)`: one expression. On the caller's thread, which needs a big stack
/// ([`PARSE_STACK`]).
pub fn spec(text: &[u16], dialect: Dialect) -> Answer<ValueSpecification> {
    spec_parser::parse(text, dialect).map_err(JavaError::placed)
}

/// `SpecParser.parseLambda(text)`: a lambda at the engine's dialect, a non-lambda wrapped as a parameterless one.
pub fn lambda(text: &[u16]) -> Answer<LambdaFunction> {
    spec_parser::parse_lambda(text).map_err(JavaError::placed)
}

/// `SpecParser.parseCodeBlock(text, dialect)`: a `;`-separated block.
pub fn block(text: &[u16], dialect: Dialect) -> Answer<Vec<ValueSpecification>> {
    spec_parser::parse_code_block(text, dialect).map_err(JavaError::placed)
}

/// `PmcdParser.parse(text, dialect)`: a model text's records -- every element in section order, the section index,
/// the unclaimed sections -- with the shim's registered overlay grammars: beside the records, each overlay section
/// the walk met, whose grammar the shim calls and whose elements it splices (`model_parser::OverlaySection`).
pub fn pmcd(text: &[u16], dialect: Dialect, overlays: &[OverlayGrammar<'_>]) -> PmcdAnswer {
    let answer = crate::model_parser::parse_with_overlays(text, dialect, overlays);
    PmcdAnswer { parsed: answer.parsed.map_err(JavaError::placed), overlays: answer.overlays }
}

/// The shim's half of an overlay section, as the differential plays it: the text, the door's answer, and the records
/// with each overlay grammar's elements spliced in (or the error the grammar, or the door, raises first).
pub type Splice<'a> = &'a (dyn Fn(&[u16], PmcdAnswer) -> Answer<PmcdParserParsed> + Sync);

/// The model door's canonical form (as [`answer_form`]) with overlay grammars registered, their sections spliced by
/// `splice`. On a thread of [`PARSE_STACK`].
pub fn pmcd_answer_form(
    dialect: Dialect,
    text: &[u16],
    overlays: &[OverlayGrammar<'_>],
    splice: Splice<'_>,
) -> Vec<u16> {
    // the door's declared PmcdParser.Parsed fixes the class: no tag
    let written = on_parse_stack(|| splice(text, pmcd(text, dialect, overlays)).map(|v| codec::write(&v, true)));
    canonical(written)
}

/// An answer in the canonical form Java's `FrontAnswers.canonical` writes: `A` and the codec's JSON (the door's
/// declared type deciding the tags: a `ValueSpecification` or a list's element is tagged, a `LambdaFunction` is not);
/// or `E`, the class, the line and column (empty but for a `ParseException`), the message (`null` without one), tab
/// separated. UTF-16 units, as Java's string. The door parses on a thread of [`PARSE_STACK`]; the model door with no
/// overlay grammar registered.
pub fn answer_form(door: &str, dialect: Dialect, text: &[u16]) -> Vec<u16> {
    let written = on_parse_stack(|| match door {
        "spec" => spec(text, dialect).map(|v| codec::write(&v, false)),
        "lambda" => lambda(text).map(|v| codec::write(&v, true)),
        "block" => block(text, dialect).map(|vs| {
            let mut w = codec::Writer::new();
            w.raw("[");
            for (i, v) in vs.iter().enumerate() {
                if i > 0 {
                    w.raw(",");
                }
                codec::Codec::write(v, &mut w, false);
            }
            w.raw("]");
            w.finish()
        }),
        // the door's declared PmcdParser.Parsed fixes the class: no tag
        "pmcd" => pmcd(text, dialect, &[]).parsed.map(|v| codec::write(&v, true)),
        _ => Err(JavaError::of("door not ported", Some(door))),
    });
    canonical(written)
}

/// `PmcdParser.parseDocument(text, sourceInformation)`: a model text at the engine's dialect, its records emitted as
/// legend-engine's PMCD JSON (`ProtocolEmitter.emit`), cut without its spans when `spans` is off
/// (`SourceInformation.withoutSpans`): the JSON, or the parser's error, or the emitter's refusal of a model it parsed.
/// The JSON is a Java string's UTF-16 units (a lone surrogate kept raw, as the emitter writes it). On the caller's
/// thread, which needs a big stack ([`PARSE_STACK`]).
pub fn document(text: &[u16], spans: bool) -> Answer<Vec<u16>> {
    document_with_overlays(text, spans, &[]).expect("no overlay grammar is registered")
}

/// [`document`] with the shim's registered overlay grammars: `None` when the walk met an overlay section, whose
/// grammar Java calls -- the overlay path, where Java answers (`ProtocolEmitter.emit` over `PmcdParser.parse`'s
/// spliced records). At the engine's dialect the walk never meets one (an unknown section is refused there, PARITY
/// row 17), so no text takes it; it is there by construction.
pub fn document_with_overlays(text: &[u16], spans: bool, overlays: &[OverlayGrammar<'_>]) -> Option<Answer<Vec<u16>>> {
    let answer = pmcd(text, Dialect::LegendEngine, overlays);
    if !answer.overlays.is_empty() {
        return None;
    }
    Some(answer.parsed.and_then(|parsed| {
        let json = crate::emitter::emit(&parsed.model).map_err(|e| emitter_refusal(e, &parsed))?;
        Ok(if spans { json } else { crate::source_information::without_spans(&json) })
    }))
}

/// The emitter's refusal as a door answers it: one the engine answers from its section catch-all at the section that
/// holds the refused node. The section index's spans are in the engine's buffer (`"\n###Pure\n" + source`), a section's
/// start one line below its header in the text.
fn emitter_refusal(e: JavaError, parsed: &PmcdParserParsed) -> JavaError {
    let line = e.position.map_or(0, |p| p.0);
    let section = parsed.model.elements.iter().rev().find_map(|element| match element {
        Element::PSectionIndex(index) => {
            index.sections.iter().rev().filter_map(|s| s.source_information.as_deref()).find(|s| s.start_line - 1 <= line)
        }
        _ => None,
    });
    match section {
        Some(span) => e.at_section(span).placed(),
        None => e.placed(),
    }
}

/// `SpecParser.lambdaJson(text, sourceInformation)`: a lambda at the engine's dialect (`parseLambda`), emitted as
/// legend-engine's lambda JSON (`ProtocolEmitter.emitLambda`), cut without its spans when `spans` is off. On the
/// caller's thread, which needs a big stack ([`PARSE_STACK`]).
pub fn lambda_json(text: &[u16], spans: bool) -> Answer<Vec<u16>> {
    let lam = lambda(text)?;
    let json = crate::emitter::emit_lambda(&lam).map_err(JavaError::placed)?;
    Ok(if spans { json } else { crate::source_information::without_spans(&json) })
}

/// An engine door's two canonical forms, as Java's `FrontEngineAnswers.forms` writes them: with its source information
/// and without it (`A\t<json>`, or the error's form twice). `door` is `document` or `lambdaJson`. On a thread of
/// [`PARSE_STACK`].
pub fn engine_forms(door: &str, text: &[u16]) -> (Vec<u16>, Vec<u16>) {
    let answer = on_parse_stack(|| match door {
        "document" => document(text, true),
        "lambdaJson" => lambda_json(text, true),
        _ => Err(JavaError::of("door not ported", Some(door))),
    });
    match answer {
        Ok(json) => {
            let bare = crate::source_information::without_spans(&json);
            (canonical_units(Ok(json)), canonical_units(Ok(bare)))
        }
        Err(e) => {
            let form = canonical_units(Err(e));
            (form.clone(), form)
        }
    }
}

/// [`canonical`] over an answer of UTF-16 units.
pub fn canonical_units(written: Answer<Vec<u16>>) -> Vec<u16> {
    match written {
        Ok(json) => {
            let mut out: Vec<u16> = Vec::with_capacity(json.len() + 2);
            out.extend("A\t".encode_utf16());
            out.extend_from_slice(&json);
            out
        }
        Err(e) => canonical(Err(e)),
    }
}

/// `A\t<json>` or `E\t<class>\t<line>\t<column>\t<message>`.
fn canonical(written: Answer<String>) -> Vec<u16> {
    let mut out: Vec<u16> = Vec::new();
    let put = |s: &str, out: &mut Vec<u16>| out.extend(s.encode_utf16());
    match written {
        Ok(json) => {
            put("A\t", &mut out);
            put(&json, &mut out);
        }
        Err(e) => {
            put("E\t", &mut out);
            put(&e.class, &mut out);
            put("\t", &mut out);
            if let Some((l, c)) = e.position {
                put(&format!("{l}\t{c}"), &mut out);
            } else {
                put("\t", &mut out);
            }
            put("\t", &mut out);
            match &e.message {
                None => put("null", &mut out),
                Some(m) => out.extend_from_slice(m.units()),
            }
        }
    }
    out
}

#[cfg(test)]
mod refusal_tests {
    //! The user's ruling 4 of 2026-11-09 (docs/rust-port/PHASE2.md section 6): malformed input the engine refuses is a
    //! placed `ParseException`, not a raw JDK exception; legend-engine's line and column for each input below were read
    //! from its own parser (rust/lite-front/PARITY.md, the rows named). The inputs the engine accepts keep their answer
    //! and come back to the user.
    use super::*;
    use crate::ustr::u;

    const DIALECTS: [Dialect; 3] = [Dialect::LegendEngine, Dialect::LegendLite, Dialect::LegendPlatform];

    fn fun(body: &str) -> String {
        format!("function a::f():Any[*]\n{{\n  {body}\n}}\n")
    }

    /// `text` refused at each dialect's model door at `place`, its `document` door too.
    #[track_caller]
    fn model_refused_at(text: &str, place: (i32, i32)) {
        let units = u(text);
        for d in DIALECTS {
            let e = pmcd(&units, d, &[]).parsed.expect_err(text);
            assert!(e.is_parse_exception(), "{text}: {e:?}");
            assert_eq!(e.position, Some(place), "{text}: {e:?}");
        }
        let e = document(&units, true).expect_err(text);
        assert!(e.is_parse_exception(), "{text}: {e:?}");
        assert_eq!(e.position, Some(place), "{text}: {e:?}");
    }

    /// `text` accepted by the parser at every dialect, the `document` door refusing it at `place`.
    #[track_caller]
    fn document_refused_at(text: &str, place: (i32, i32)) {
        let units = u(text);
        for d in DIALECTS {
            assert!(pmcd(&units, d, &[]).parsed.is_ok(), "{text}");
        }
        let e = document(&units, true).expect_err(text);
        assert!(e.is_parse_exception(), "{text}: {e:?}");
        assert_eq!(e.position, Some(place), "{text}: {e:?}");
    }

    #[track_caller]
    fn expression_refused_at(text: &str, place: (i32, i32)) {
        let units = u(text);
        for d in DIALECTS {
            let e = spec(&units, d).expect_err(text);
            assert!(e.is_parse_exception(), "{text}: {e:?}");
            assert_eq!(e.position, Some(place), "{text}: {e:?}");
        }
    }

    #[test]
    fn a_number_past_a_long_is_the_section_catch_all_s() {
        // row 6: the engine's message at the section's start; an expression has no section, so the token
        model_refused_at(&fun("@Varchar(99999999999999999999)"), (1, 1));
        expression_refused_at("@Varchar(99999999999999999999)", (1, 10));
        // a section header is the engine's [line below the header : the end of the line before it]
        model_refused_at("###Service\nService s::S\n{\n  pattern: '/p';\n  documentation: 'd';\n  execution: Single\n  {\n    query: |1;\n    mapping: m::M;\n    runtime: r::R;\n  }\n  test: Single\n  {\n    data: 'x';\n    asserts: [ { [-99999999999999999999], |true } ];\n  }\n}\n", (2, 8));
        model_refused_at("###FileGeneration\nProtobuf a::G\n{\n  scopeElements: [a];\n  x: 99999999999999999999;\n}\n", (2, 8));
    }

    #[test]
    fn an_exponent_with_no_digits_is_refused_at_its_letter() {
        // row 8
        for t in ["1.5ed", "1.5e+d", "1.5ef", "1.5e", "1.5Ed"] {
            expression_refused_at(t, (1, 4));
        }
        model_refused_at(&fun("1.5ed"), (3, 6));
        model_refused_at("###Relational\nDatabase a::D\n(\n  Table T (ID INTEGER)\n  Join j(T.ID = 1.5ef)\n)\n", (5, 20));
        // past an int: the engine's section catch-all
        model_refused_at(&fun("1.5e99999999999d"), (1, 1));
    }

    #[test]
    fn a_short_unicode_escape_is_the_section_catch_all_s() {
        // row 9, the frozen oracle's question 109b58da among them
        model_refused_at("Class 'a\\u12'::B\n{\n}\n", (1, 1));
        model_refused_at("###Pure\nimport 'a\\u12'::*;\nClass a::B\n{\n}\n", (2, 8));
        model_refused_at("###Mapping\nMapping 'a\\u12'::M\n(\n)\n", (2, 8));
        model_refused_at("Profile a::P\n{\n  tags: [doc];\n}\nClass {a::P.doc='''\n\\u12'''} a::B\n{\n}\n", (1, 1));
        model_refused_at(&fun("'x\\u12'::f()"), (1, 1));
        expression_refused_at("'x\\u12'::f()", (1, 1));
        // one line is no text block to the engine, which refuses at the string after the first `''`
        expression_refused_at("'''\\u12'''", (1, 3));
    }

    #[test]
    fn a_relational_sign_is_refused_where_it_stands() {
        // row 26
        let db = |o: &str| format!("###Relational\nDatabase a::D\n(\n  Table T (ID INTEGER)\n  Join j(T.ID = {o})\n)\n");
        model_refused_at(&db("- x"), (5, 17));
        model_refused_at(&db("- 'a'"), (5, 17));
        model_refused_at(&db("- .."), (5, 17));
        model_refused_at("###Relational\nDatabase a::D\n(\n  Table T (ID INTEGER)\n  Join j(T.ID = -", (5, 17));
    }

    #[test]
    fn a_service_store_local_property_multiplicity_parameter_is_the_section_catch_all_s() {
        // row 28
        model_refused_at("###Mapping\nMapping a::M\n(\n  *a::C: ServiceStore\n  {\n    +x: String[m];\n    ~service [a::S] G.S\n  }\n)\n", (2, 8));
    }

    #[test]
    fn the_end_of_the_input_is_where_the_engine_puts_it() {
        // rows 29 and 35
        model_refused_at("###Mapping\nMapping a::M\n(\n  MappingTests\n  [\n    t\n    (\n      assert: '{}'", (8, 19));
        model_refused_at("###Mapping\nMapping a::M\n(\n  MappingTests\n  [\n    t\n    (\n      assert: '{}'\n", (9, 1));
        // a `~src` is re-parsed on its own: one column before the end
        model_refused_at("###Mapping\nMapping a::M\n(\n  *a::C: Relation\n  {\n    ~src", (6, 8));
        model_refused_at("###Service\nService s::S\n{\n  pattern: '/p';\n  documentation: 'd';\n  execution: Single { query: |1; mapping: m::M; runtime: #{", (6, 60));
    }

    #[test]
    fn an_empty_aggregate_lambda_is_refused_at_its_terminator() {
        // row 32
        let agg = |g: &str| format!("###Mapping\nMapping a::M\n(\n  a::P[x]: AggregationAware\n  {{\n    Views: [\n      (\n        ~modelOperation: {{\n          ~canAggregate true,\n          ~groupByFunctions ({g}),\n          ~aggregateValues (\n            ( ~mapFn:$this.p , ~aggregateFn: $mapped->sum() )\n          )\n        }},\n        ~aggregateMapping: Pure\n        {{\n          ~src a::S\n          p: $src.p\n        }}\n      )\n    ],\n    ~mainMapping: Pure\n    {{\n      ~src a::P\n      p: $src.p\n    }}\n  }}\n)\n");
        model_refused_at(&agg(" ,"), (10, 31));
        model_refused_at(&agg("\n  ,"), (11, 3));
    }

    #[test]
    fn a_diagram_coordinate_and_a_data_quality_match_are_refused_at_the_token() {
        // row 49
        let dgm = |n: &str| format!("###Diagram\nDiagram a::D\n{{\n  classView id1\n  {{\n    class: a::C;\n    position: ({n}, 3.0);\n    rectangle: (1.0, 1.0);\n  }}\n}}\n");
        model_refused_at(&dgm("1-2"), (7, 16));
        model_refused_at(&dgm("\u{0661}\u{0662}"), (7, 16));
        model_refused_at(&dgm("1.2.3"), (7, 19));
        let dq = |m: &str| format!("###DataQualityValidation\nDataQualityRelationComparison a::R\n{{\n  source: src|#>{{a::D.T}}#;\n  target: tgt|#>{{a::D.T}}#;\n  keys: [A];\n  strategy: MD5Hash;\n  expectedMatch: {m};\n}}\n");
        model_refused_at(&dq("'x'"), (8, 18));
        model_refused_at(&dq("x"), (8, 18));
    }

    const MONGO: &str = "###MongoDB\nDatabase a::D\n(\n  Collection C\n  (\n    validationLevel: strict;\n    validationAction: warn;\n    jsonSchema: ";

    fn mongo(schema: &str) -> String {
        format!("{MONGO}{schema};\n  )\n)\n")
    }

    #[test]
    fn a_json_schema_the_engine_refuses_is_the_section_catch_all_s() {
        // row 50, the shapes its reader fails on
        for schema in [
            r#"{"bsonType": "object", "properties": {"a": 1}}"#,
            r#"{"properties": {"a": 1}}"#,
            r#"{"required": [null]}"#,
            r#"{"title": 1}"#,
            r#"{"minLength": 99999999999999999999}"#,
            r#"{"bsonType": "object", "properties": {"a": {"bsonType": null}}}"#,
            r#"{"bsonType": "object", "properties": {"a": {"title": 1}}}"#,
            r#"{"bsonType": "object", "properties": {"a": {"bsonType": "array", "items": [1]}}}"#,
        ] {
            model_refused_at(&mongo(schema), (2, 8));
        }
    }

    #[test]
    fn an_open_double_quote_is_refused_at_the_quote() {
        // row 52
        model_refused_at(&format!("{MONGO}{{\"a\": \""), (8, 23));
    }

    #[test]
    fn a_persistence_island_or_path_cut_by_the_end_of_the_input_is_refused_there() {
        // rows 55 and 56
        let p = "###Persistence\nPersistence a::P\n{\n  doc: 'd';\n  trigger: Manual;\n  service: a::S;\n";
        let c = "###Persistence\nPersistenceContext a::C\n{\n  persistence: a::P;\n";
        model_refused_at(&format!("{p}  persister: Batch #{{"), (7, 20));
        model_refused_at(&format!("{c}  platform: AwsGlue #{{"), (5, 23));
        model_refused_at(&format!("{c}  sinkConnection: #{{"), (5, 21));
        model_refused_at(&format!("{c}  serviceParameters: [ foo = #{{"), (5, 32));
        model_refused_at(&format!("{p}  serviceOutputTargets: [ #/a::T/h# {{ keys: [ #/a::T/h/k1#,"), (7, 60));
    }

    #[test]
    fn a_graph_fetch_argument_is_refused_where_the_engine_refuses_it() {
        // row 63, at the engine-JSON doors alone: the parser's doors keep their records
        document_refused_at("function y4::b(): Any[*]\n{\n  #{I {product(2015-10-16){name}}}#;\n}\n", (3, 20));
        document_refused_at(&fun("#{a::B{c(f())}}#"), (3, 13));
        document_refused_at(&fun("#{a::B{c(a::E)}}#"), (3, 16));
        document_refused_at(&fun("#{a::B{c($x.y)}}#"), (3, 14));
        document_refused_at(&fun("#{a::B{c(%10:10)}}#"), (3, 12));
        document_refused_at(&fun("#{a::B{c(~a)}}#"), (3, 12));
        document_refused_at(&fun("#{a::B{c(^a::B())}}#"), (3, 12));
        document_refused_at(&fun("#{a::B{c(1+2)}}#"), (3, 13));
        document_refused_at(&fun("#{a::B{c([f()])}}#"), (3, 14));
        // what the engine's walker fails on: its section catch-all
        document_refused_at(&fun("#{a::B{c(1.5d)}}#"), (1, 1));
        document_refused_at(&fun("#{a::B{c(-1)}}#"), (1, 1));
        document_refused_at("Class a::X {}\n###Pure\nfunction a::f():Any[*]\n{\n  #{a::B{c(1.5d)}}#\n}\n", (3, 14));
        let e = lambda_json(&u("|#{a::B{c(1.5d)}}#"), true).expect_err("a decimal");
        assert!(e.is_parse_exception(), "{e:?}");
    }

    #[test]
    fn a_persistence_kind_and_a_data_quality_context_the_engine_does_not_name_are_refused() {
        // rows 73 and 76
        let p = "###Persistence\nPersistence a::P\n{\n  doc: 'd';\n  trigger: Manual;\n  service: a::S;\n";
        document_refused_at(&format!("{p}  persister: Foo\n  {{\n  }}\n}}\n"), (7, 14));
        document_refused_at(&format!("{p}  serviceOutputTargets: [ x {{ keys: [] }} -> {{ }} ];\n}}\n"), (7, 27));
        let v = |c: &str| format!("###DataQualityValidation\nDataQualityValidation a::V\n{{\n  context: {c};\n  validationTree: $[\n    a::P<mustBeNamed>{{\n      name\n    }}\n  ]$;\n}}\n");
        document_refused_at(&v("fromMappingAndRuntime(a::M)"), (4, 38));
        document_refused_at(&v("fromDataSpace(a::DS)"), (4, 31));
        document_refused_at(&v("fromSomewhere(a::B)"), (4, 12));
        // the record has no place for a strategy: the parser refuses it at the engine's dialect alone
        let r = "###DataQualityValidation\nDataQualityRelationComparison a::R\n{\n  source: src|#>{a::D.T}#;\n  target: tgt|#>{a::D.T}#;\n  keys: [A];\n  strategy: SHA256;\n}\n";
        let e = document(&u(r), true).expect_err("a strategy the engine does not name");
        assert_eq!((e.is_parse_exception(), e.position), (true, Some((7, 13))), "{e:?}");
    }

    #[test]
    fn an_island_or_a_path_cut_at_its_opener_is_refused() {
        // rows 83 and 84, both found by mutating the oracle's texts: the raw exceptions no row named
        model_refused_at("function a::f():Any[*]\n{#{", (2, 2));
        expression_refused_at("#{", (1, 1));
        model_refused_at("function test(): Any[*]\n{\n  #/", (3, 3));
        expression_refused_at("#/", (1, 1));
    }

    #[test]
    fn a_type_argument_the_engine_has_no_grammar_for_is_refused_at_its_bracket() {
        // row 87, found by mutating the oracle's texts
        document_refused_at("Class P\n{\n  items: List<Pers-on>[*];\n}\n", (3, 14));
    }

    #[test]
    fn a_quote_left_open_at_the_end_of_the_input_is_refused() {
        // rows 85 and 86, found by mutating the oracle's texts
        let m = "###Mapping\nMapping a::M\n(\n  MappingTests\n  [\n    t\n    (\n";
        model_refused_at(&format!("{m}      assert: '"), (8, 15));
        model_refused_at("###Pure\nProfile a::p { tags: [doc]; }\nClass {a::p.doc = '''hello'' a::A { x: String[1]; }\n", (3, 21));
        model_refused_at("Class {meta::pure::profiles::doc.doc = '''\nDo", (1, 42));
        expression_refused_at("'''\nDo", (1, 3));
        expression_refused_at("'''", (1, 3));
    }

    #[track_caller]
    fn still_raw(text: &str, class: &str, door_answer: Answer<()>) {
        let e = door_answer.expect_err(text);
        assert_eq!(e.class, class, "{text}: {e:?}");
    }

    #[test]
    fn what_the_engine_accepts_keeps_its_answer_and_comes_back_to_the_user() {
        // the ruling's carve-out: rows 3, 7, 8 (a part), 46, 50 (a part), 72 and 73 (a part)
        let m = |t: &str| pmcd(&u(t), Dialect::LegendLite, &[]).parsed.map(|_| ());
        still_raw("row 3", "java.time.DateTimeException", spec(&u("%2024-02-30T10:00+0500"), Dialect::LegendEngine).map(|_| ()));
        still_raw("row 7", "java.lang.NumberFormatException", spec(&u("1e999"), Dialect::LegendLite).map(|_| ()));
        still_raw("row 8", "java.lang.NumberFormatException", spec(&u("1.ed"), Dialect::LegendEngine).map(|_| ()));
        still_raw("row 46", "java.lang.StringIndexOutOfBoundsException", m("###Diagram\n###Pure\nClass a::B\n{\n}\n"));
        still_raw("row 50", "java.lang.ClassCastException", m(&mongo(r#"{"bsonType": "object", "title": 1}"#)));
        still_raw("row 50", "java.lang.NumberFormatException", m(&mongo(r#"{"bsonType": "object", "minLength": 99999999999999999999}"#)));
        let aggregation = "###Mapping\nMapping a::M\n(\n  a::P[x]: AggregationAware\n  {\n    Views: [\n      (\n        ~modelOperation: {\n          ~canAggregate true,\n          ~groupByFunctions ($this.id),\n          ~aggregateValues (\n            ( ~mapFn:$this.p , ~aggregateFn: $mapped->sum() )\n          )\n        },\n        ~aggregateMapping: Pure\n        {\n          ~src a::S\n          p: $src.p\n        }\n      )\n    ],\n    ~mainMapping: Pure\n    {\n      ~src a::P\n      +loc: String[1]: $src.x\n    }\n  }\n)\n";
        still_raw("row 72", "java.lang.NullPointerException", document(&u(aggregation), true).map(|_| ()));
        let platform = "###Persistence\nPersistenceContext a::C\n{\n  persistence: a::P;\n  platform: Default #{ x: 1; }#;\n}\n";
        still_raw("row 73", "java.lang.IllegalStateException", document(&u(platform), true).map(|_| ()));
    }

    #[test]
    fn a_refusal_in_flight_passes_the_recoveries_and_a_door_places_it() {
        let flying = JavaError::refused_at_section(JStr::from("m"), (3, 4));
        assert!(!flying.is_parse_exception(), "Java's catch (ParseException e) must not take it");
        let token = JavaError::parse(JStr::from("m"), 3, 4).as_refusal();
        assert!(!token.is_parse_exception());
        assert_eq!(token.clone().placed().message.unwrap().lossy(), "[3:4] m");
        assert_eq!(flying.clone().placed().position, Some((3, 4)));
        let unknown = JavaError::refused_at_section(JStr::from("m"), (0, 0));
        assert_eq!(unknown.placed().position, Some((1, 1)), "a door with no section and no token: the text's start");
    }
}
