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
use crate::records::{LambdaFunction, PmcdParserParsed, SourceInfo, ValueSpecification};
use crate::spec_parser;

/// The parse's level (Java's `parser.Dialect`), with three inner levels of Java's `LEGEND_ENGINE`, set by the doors alone
/// (docs/rust-port/PHASE2.md section 6, the user's rulings of 2026-11-09).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Dialect {
    LegendPlatform,
    LegendLite,
    /// `LEGEND_ENGINE` as a door is handed it, and in the quoted folds, whose text is a string to the engine.
    LegendEngine,
    /// `LEGEND_ENGINE` at every door that reads a text: held to the engine's grammar for a graph-fetch argument, a dated
    /// path segment's argument and a persistence platform's island as well (rulings 13 and 16).
    LegendEngineDoor,
    /// `LEGEND_ENGINE` when an engine-JSON door reads a text again to place what the emitter would not write (rulings
    /// 11, 12 and 15; [`emitter_refusal`]).
    LegendEngineJson,
    /// `LEGEND_ENGINE` reading a text again past a refusal of the engine's walk, for a later syntax error, which the
    /// engine reports first (ruling 16; [`JavaError::syntax_first`]): what the walk refuses is read, and an island or a
    /// mapping element's body is passed by.
    LegendEngineSyntax,
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
            Dialect::LegendEngine
            | Dialect::LegendEngineDoor
            | Dialect::LegendEngineJson
            | Dialect::LegendEngineSyntax => "LEGEND_ENGINE",
        }
    }

    /// The platform dialect (m3/m2 constructs) refuses here (Java's `refusesPlatformDialect`).
    pub fn refuses_platform_dialect(self) -> bool {
        self != Dialect::LegendPlatform
    }

    /// The declared lite-design extensions refuse here (Java's `refusesLiteExtensions`).
    pub fn refuses_lite_extensions(self) -> bool {
        matches!(
            self,
            Dialect::LegendEngine | Dialect::LegendEngineDoor | Dialect::LegendEngineJson | Dialect::LegendEngineSyntax
        )
    }

    /// What the engine's walk refuses and lite's reading would take is refused: a graph-fetch argument, a dated path
    /// segment's argument, a persistence platform's island (rulings 13 and 16; PARITY rows 63, 73 and 81).
    pub fn holds_to_the_engines_walk(self) -> bool {
        matches!(self, Dialect::LegendEngineDoor | Dialect::LegendEngineJson)
    }

    /// A refusal of a text this level reads stands at the engine's place, not at the parser's own (ruling 16; PARITY row
    /// 69): every reading of a door's text at the engine's dialect, not a quoted one.
    pub fn places_as_the_engine(self) -> bool {
        self.holds_to_the_engines_walk() || self == Dialect::LegendEngineSyntax
    }

    /// The constructs lite's records carry and legend-engine's JSON has no answer for are refused (rulings 11 and 12).
    pub fn answers_engine_json(self) -> bool {
        self == Dialect::LegendEngineJson
    }

    /// The level a door reads its text at.
    fn at_a_door(self) -> Dialect {
        if self == Dialect::LegendEngine {
            Dialect::LegendEngineDoor
        } else {
            self
        }
    }

    /// Whether this is the reading for a later syntax error ([`JavaError::syntax_first`]): what the engine's section parse
    /// takes whole and its walk reads (an island, a mapping element's body) is passed by, its errors the walk's.
    pub(crate) fn reads_for_syntax(self) -> bool {
        self == Dialect::LegendEngineSyntax
    }

    /// The level a quoted tree is read at: not the door's, since the text is a string to the engine.
    pub fn quoted(self) -> Dialect {
        if self.refuses_lite_extensions() {
            Dialect::LegendEngine
        } else {
            self
        }
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

/// The words of the engine's own walkers, in the errors the parser raises as it reads: a later one is no syntax error.
const WALK_WORDS: [&str; 2] = ["Field '", "Unknown "];

/// The refusals by name of constructs the engine's grammar reads and only its walk refuses ([`JavaError::end_of_input_first`]).
const WALK_REFUSALS: [&str; 11] = [
    "Unsupported syntax",
    "Type and/or multiplicity parameters are not authorized",
    "stereotypes/tagged values are not authorized",
    "class projections are not supported",
    "copy-with-update",
    "Bracket operation is not supported",
    "Unit instance not supported",
    "is not supported yet",
    ".allVersionsInRange() is not supported",
    "Unexpected token '^'",
    "Documentation must be written",
];

impl JavaError {
    /// A `ParseException` at a position: its message carries Java's `[line:col] ` prefix when `line > 0`.
    pub fn parse_at(message: &str, line: i32, column: i32) -> JavaError {
        let text = if line > 0 {
            format!("[{line}:{column}] {message}")
        } else {
            message.to_owned()
        };
        JavaError {
            class: PARSE_EXCEPTION.to_owned(),
            message: Some(JStr::from(text)),
            position: Some((line, column)),
        }
    }

    /// Another class's error (no position): `IllegalStateException`, `IllegalArgumentException`, ...
    pub fn of(class: &str, message: Option<&str>) -> JavaError {
        JavaError {
            class: class.to_owned(),
            message: message.map(JStr::from),
            position: None,
        }
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
        JavaError {
            class: PARSE_EXCEPTION.to_owned(),
            message: Some(text),
            position: Some((line, column)),
        }
    }

    /// A `ParseException(message)`, Java's one-argument constructor: no position (line and column 0).
    pub fn parse_unplaced(message: JStr) -> JavaError {
        JavaError {
            class: PARSE_EXCEPTION.to_owned(),
            message: Some(message),
            position: Some((0, 0)),
        }
    }

    /// Another class's error over a message's UTF-16 units.
    pub fn java(class: &str, message: JStr) -> JavaError {
        JavaError {
            class: class.to_owned(),
            message: Some(message),
            position: None,
        }
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
        JavaError {
            class: REFUSED_AT_SECTION.to_owned(),
            message: Some(message),
            position: Some(at),
        }
    }

    /// This error's message, without the `[line:col] ` prefix a placed one carries.
    fn words(&self) -> JStr {
        let text = self.message.clone().unwrap_or_default();
        if !matches!(self.position, Some((line, _)) if line > 0) || self.class == REFUSED_AT_SECTION {
            return text;
        }
        let units = text.units();
        let start = units.iter().position(|&c| c == u16::from(b' ')).map_or(0, |i| i + 1);
        JStr(units[start..].to_vec())
    }

    /// This error's words at another place, a refusal in flight: what the engine's section parse answers before its walk
    /// begins (the user's ruling 18 of 2026-11-09, docs/rust-port/PHASE2.md section 6).
    pub fn at_place(&self, (line, column): (i32, i32)) -> JavaError {
        JavaError::parse(self.words(), line, column).as_refusal()
    }

    /// This refusal one column to the left, its `[line:col] ` prefix with it (a line's column 0 where the end of the input
    /// is an empty last line, as `ParserErrorListener` computes it).
    pub fn a_column_before(self) -> JavaError {
        let Some((line, column)) = self.position else {
            return self;
        };
        JavaError {
            class: self.class.clone(),
            ..JavaError::parse(self.words(), line, column - 1)
        }
    }

    /// A construct refused by name that the engine's grammar reads and its walk refuses, against the end of the input: the
    /// engine parses a section whole before it walks it, so a text cut inside or after the construct is refused at its
    /// end (the user's ruling 18 of 2026-11-09, docs/rust-port/PHASE2.md section 6). `read_again` reads the text at the
    /// platform's dialect, which reads every such construct; `end` is one past the input's last character.
    pub(crate) fn end_of_input_first(
        self,
        level: Dialect,
        end: (i32, i32),
        read_again: impl FnOnce() -> Answer<()>,
    ) -> JavaError {
        let words = self.words().lossy();
        if level == Dialect::LegendPlatform || !WALK_REFUSALS.iter().any(|w| words.contains(w)) {
            return self;
        }
        match read_again() {
            Err(later) if later.position == Some(end) || later.position == Some((end.0, end.1 - 1)) => later,
            _ => self,
        }
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

    /// Of this refusal and `other`, both of one construct the engine reads in one parse, the one placed first: `other` when
    /// both are placed and it stands before this one, else this one (an unplaced refusal keeps the caller's order). The
    /// engine's parser meets the earlier of two refusals first (the user's ruling 27 of 2026-11-11,
    /// docs/rust-port/PHASE2.md section 6).
    pub(crate) fn or_earlier(self, other: JavaError) -> JavaError {
        match (self.position, other.position) {
            (Some(mine), Some(theirs)) if mine.0 > 0 && theirs.0 > 0 && theirs < mine => other,
            _ => self,
        }
    }

    /// A refusal in flight, of either kind.
    pub(crate) fn is_in_flight(&self) -> bool {
        self.class == REFUSED || self.class == REFUSED_AT_SECTION
    }

    /// Whether legend-engine's section parse raises this, before its walk begins, as far as lite's words can say: an
    /// error of the walk ([`WALK_WORDS`], a section catch-all's) is not.
    fn is_syntax_error(&self) -> bool {
        let text = self.message.as_ref().map(JStr::lossy).unwrap_or_default();
        let words = text.split_once("] ").map_or(text.as_str(), |(_, words)| words);
        if self.class == PARSE_EXCEPTION {
            return !WALK_WORDS.iter().any(|w| words.starts_with(w));
        }
        self.class == REFUSED && words.starts_with("Unexpected token")
    }

    /// A refusal of the engine's walk against a later syntax error of the same section: the engine parses a section whole
    /// before it walks it, so the syntax error comes first (the user's ruling 16 of 2026-11-09, docs/rust-port/PHASE2.md
    /// section 6; PARITY row 63). `read_again` reads the section at [`Dialect::LegendEngineSyntax`], past the refusal; a
    /// refusal that reading meets too (one of the engine's syntax errors already, or a section catch-all's) stands.
    pub(crate) fn syntax_first(self, level: Dialect, read_again: impl FnOnce() -> Answer<()>) -> JavaError {
        if !(level.holds_to_the_engines_walk() && self.is_in_flight()) {
            return self;
        }
        match read_again() {
            Err(later) if later.is_syntax_error() => later,
            _ => self,
        }
    }

    /// [`JavaError::syntax_first`] in a mapping element's body: a Pure class mapping's walker checks that its `~src` and
    /// its `~filter` are given once before it reparses any of its expressions
    /// (`PureInstanceClassMappingParseTreeWalker.visitPureInstanceClassMapping`), so such a duplicate the second reading
    /// meets stands before this refusal too, as a syntax error does (the user's ruling 27 of 2026-11-11,
    /// docs/rust-port/PHASE2.md section 6; register S365).
    pub(crate) fn syntax_or_duplicate_first(
        self,
        level: Dialect,
        read_again: impl FnOnce() -> Answer<()>,
    ) -> JavaError {
        if !(level.holds_to_the_engines_walk() && self.is_in_flight()) {
            return self;
        }
        match read_again() {
            Err(later) if later.is_syntax_error() || later.is_a_class_mapping_field_twice() => later,
            _ => self,
        }
    }

    /// A Pure class mapping's `~src` or `~filter` given twice, the walker's words.
    fn is_a_class_mapping_field_twice(&self) -> bool {
        let words = self.words().lossy();
        self.class == PARSE_EXCEPTION
            && (words == "Field '~src' should be specified only once"
                || words == "Field '~filter' should be specified only once")
    }

    /// What a door answers: a refusal in flight as the `ParseException` it is; every other error as it stands.
    pub fn placed(self) -> JavaError {
        if self.class == REFUSED {
            return JavaError {
                class: PARSE_EXCEPTION.to_owned(),
                ..self
            };
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

/// An expression door's reading at `level`: a refusal of the engine's walk behind a later syntax error of the text
/// ([`JavaError::syntax_first`]), a refusal in flight as a door answers it.
fn read_expression<T>(text: &[u16], level: Dialect, parse: impl Fn(Dialect) -> Answer<T>) -> Answer<T> {
    parse(level).map_err(|e| {
        let tokens = crate::lexer::tokenize(text);
        e.end_of_input_first(level, crate::cursor::eof_place(&tokens), || {
            parse(Dialect::LegendPlatform).map(|_| ())
        })
        .syntax_first(level, || parse(Dialect::LegendEngineSyntax).map(|_| ()))
        .placed()
    })
}

/// `SpecParser.parse(text, dialect)`: one expression. On the caller's thread, which needs a big stack
/// ([`PARSE_STACK`]).
pub fn spec(text: &[u16], dialect: Dialect) -> Answer<ValueSpecification> {
    read_expression(text, dialect.at_a_door(), |level| spec_parser::parse(text, level))
}

/// `SpecParser.parseLambda(text)`: a lambda at the engine's dialect, a non-lambda wrapped as a parameterless one.
pub fn lambda(text: &[u16]) -> Answer<LambdaFunction> {
    read_expression(text, Dialect::LegendEngineDoor, |level| {
        spec_parser::parse_lambda(text, level)
    })
}

/// `SpecParser.parseCodeBlock(text, dialect)`: a `;`-separated block. At the engine's dialect a block with no statement
/// is refused, as the engine's `codeBlock` is at least one line (the user's ruling 21 of 2026-11-10), in lite's words
/// and at the place an expression was expected (ruling 24 of 2026-11-10, built 2026-11-11; docs/rust-port/PHASE2.md
/// section 6; register S368): the end of the input, one past its last character, blanks, comments and line breaks
/// counted, where lite places `1 +`'s missing operand -- `[1:1]` for an empty text, `[1:4]` for three blanks, `[2:1]` for
/// a line break or a comment line alone. Not the engine's `[2:1]` for every one, its wrapper's `}` on the line below.
pub fn block(text: &[u16], dialect: Dialect) -> Answer<Vec<ValueSpecification>> {
    read_expression(text, dialect.at_a_door(), |level| {
        spec_parser::parse_code_block(text, level).and_then(|statements| {
            if statements.is_empty() && level.refuses_lite_extensions() {
                let (line, column) = crate::cursor::eof_place(&crate::lexer::tokenize(text));
                return Err(JavaError::parse(
                    JStr::from("expected expression, got end of input"),
                    line,
                    column,
                ));
            }
            Ok(statements)
        })
    })
}

/// `PmcdParser.parse(text, dialect)`: a model text's records -- every element in section order, the section index,
/// the unclaimed sections -- with the shim's registered overlay grammars: beside the records, each overlay section
/// the walk met, whose grammar the shim calls and whose elements it splices (`model_parser::OverlaySection`).
pub fn pmcd(text: &[u16], dialect: Dialect, overlays: &[OverlayGrammar<'_>]) -> PmcdAnswer {
    let level = dialect.at_a_door();
    let answer = records(text, level, overlays);
    PmcdAnswer {
        // at the engine's dialect the record door refuses what the JSON door places (the user's ruling 18 of 2026-11-09,
        // docs/rust-port/PHASE2.md section 6): the emitter knows the engine's kinds, the records have no other judge
        parsed: answer.parsed.and_then(|parsed| {
            if level.holds_to_the_engines_walk()
                && let Err(e) = crate::emitter::emit(&parsed.model)
                && e.is_in_flight()
                // a local property under an AggregationAware main mapping: the engine accepts it, the JSON door alone
                // refuses it (ruling 11, register S329)
                && !e.words().lossy().starts_with("the local property")
            {
                return Err(e.placed());
            }
            Ok(parsed)
        }),
        overlays: answer.overlays,
    }
}

/// [`pmcd`]'s answer and, when it refuses, the errors the text holds after that refusal, sorted by place (the user's
/// ruling 23 of 2026-11-10, built 2026-11-11, docs/rust-port/PHASE2.md section 6; `model_parser::further_errors`): what
/// the shim's model door answers. The first error is [`pmcd`]'s own, byte for byte, so every engine-compatible answer
/// is unchanged; the rest are for the surfaces lite shapes itself (the editor's diagnostics, `Compiler.compileErrors`).
/// A text the emitter refuses has parsed, and holds no other error.
pub fn pmcd_with_errors(
    text: &[u16],
    dialect: Dialect,
    overlays: &[OverlayGrammar<'_>],
) -> (PmcdAnswer, Vec<JavaError>) {
    let answer = pmcd(text, dialect, overlays);
    let rest = match &answer.parsed {
        Err(first) => crate::model_parser::further_errors(text, dialect.at_a_door(), overlays, first),
        Ok(_) => Vec::new(),
    };
    (answer, rest)
}

/// [`pmcd`]'s reading at `level`, before anything judges the records.
fn records(text: &[u16], level: Dialect, overlays: &[OverlayGrammar<'_>]) -> PmcdAnswer {
    let answer = crate::model_parser::parse_with_overlays(text, level, overlays);
    PmcdAnswer {
        parsed: answer.parsed.map_err(JavaError::placed),
        overlays: answer.overlays,
    }
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
    let answer = records(text, Dialect::LegendEngine.at_a_door(), overlays);
    if !answer.overlays.is_empty() {
        return None;
    }
    Some(answer.parsed.and_then(|parsed| {
        let json = crate::emitter::emit(&parsed.model)
            .map_err(|e| emitter_refusal(e, || records(text, Dialect::LegendEngineJson, &[]).parsed.map(|_| ())))?;
        Ok(if spans {
            json
        } else {
            crate::source_information::without_spans(&json)
        })
    }))
}

/// The emitter's refusal as an engine-JSON door answers it. A raw refusal the rulings place (11 and 12 of
/// docs/rust-port/PHASE2.md section 6; PARITY rows 69, 70, 71, 73, 74 and 81) is placed by reading the text again at
/// [`Dialect::LegendEngineJson`], where the parser has the tokens the records do not keep: what it answers is the door's
/// answer. A text that reading accepts keeps the emitter's own refusal: the rulings left it (rows 68, 69's `let`).
fn emitter_refusal(e: JavaError, read_again: impl FnOnce() -> Answer<()>) -> JavaError {
    if e.class.starts_with("java.")
        && let Err(placed) = read_again()
    {
        return placed;
    }
    e.placed()
}

/// `SpecParser.lambdaJson(text, sourceInformation)`: a lambda at the engine's dialect (`parseLambda`), emitted as
/// legend-engine's lambda JSON (`ProtocolEmitter.emitLambda`), cut without its spans when `spans` is off. On the
/// caller's thread, which needs a big stack ([`PARSE_STACK`]).
pub fn lambda_json(text: &[u16], spans: bool) -> Answer<Vec<u16>> {
    let lam = lambda(text)?;
    let json = crate::emitter::emit_lambda(&lam).map_err(|e| {
        emitter_refusal(e, || {
            spec_parser::parse_lambda(text, Dialect::LegendEngineJson)
                .map(|_| ())
                .map_err(JavaError::placed)
        })
    })?;
    Ok(if spans {
        json
    } else {
        crate::source_information::without_spans(&json)
    })
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
    //! The user's rulings 4, 11 to 13, 15 and 16 of 2026-11-09 (docs/rust-port/PHASE2.md section 6): malformed input the
    //! engine refuses is a placed `ParseException`, not a raw JDK exception, at legend-engine's line and column for each
    //! input below, read from its own parser (rust/lite-front/PARITY.md, the rows named); the inputs ruling 11 names, which
    //! the engine accepts, are refused all the same; what the rulings left keeps its raw answer.
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

    /// `text` accepted at lite's and the platform's dialects; the model door at the engine's and the `document` door
    /// refusing it at `place` (ruling 18 of 2026-11-09).
    #[track_caller]
    fn document_refused_at(text: &str, place: (i32, i32)) {
        let units = u(text);
        for d in [Dialect::LegendLite, Dialect::LegendPlatform] {
            assert!(pmcd(&units, d, &[]).parsed.is_ok(), "{text}");
        }
        for e in [
            pmcd(&units, Dialect::LegendEngine, &[])
                .parsed
                .map(|_| ())
                .expect_err(text),
            document(&units, true).map(|_| ()).expect_err(text),
        ] {
            assert!(e.is_parse_exception(), "{text}: {e:?}");
            assert_eq!(e.position, Some(place), "{text}: {e:?}");
        }
    }

    /// `text` accepted by the parser at every dialect, the `document` door alone refusing it at `place`: what the engine
    /// accepts and legend-engine's JSON has no answer for (ruling 11, register S329 and S330).
    #[track_caller]
    fn only_document_refused_at(text: &str, place: (i32, i32)) {
        let units = u(text);
        for d in DIALECTS {
            assert!(pmcd(&units, d, &[]).parsed.is_ok(), "{text}");
        }
        let e = document(&units, true).expect_err(text);
        assert!(e.is_parse_exception(), "{text}: {e:?}");
        assert_eq!(e.position, Some(place), "{text}: {e:?}");
    }

    /// `text` refused at the model door at the engine's dialect and at the `document` door, at `place`; accepted at
    /// lite's and the platform's dialects (ruling 13).
    #[track_caller]
    fn graph_fetch_argument_refused_at(text: &str, place: (i32, i32)) {
        let units = u(text);
        for d in [Dialect::LegendLite, Dialect::LegendPlatform] {
            assert!(pmcd(&units, d, &[]).parsed.is_ok(), "{text}");
        }
        for e in [
            pmcd(&units, Dialect::LegendEngine, &[])
                .parsed
                .map(|_| ())
                .expect_err(text),
            document(&units, true).map(|_| ()).expect_err(text),
        ] {
            assert!(e.is_parse_exception(), "{text}: {e:?}");
            assert_eq!(e.position, Some(place), "{text}: {e:?}");
        }
    }

    /// `text` refused at the `lambda` door and its JSON door at `place` (rulings 12 and 18).
    #[track_caller]
    fn lambda_json_refused_at(text: &str, place: (i32, i32)) {
        lambda_doors_refused_at(text, place);
    }

    /// `text` refused at the `lambda` door and its JSON door at `place` (rulings 13 and 16).
    #[track_caller]
    fn lambda_doors_refused_at(text: &str, place: (i32, i32)) {
        let units = u(text);
        for e in [
            lambda(&units).map(|_| ()).expect_err(text),
            lambda_json(&units, true).map(|_| ()).expect_err(text),
        ] {
            assert!(e.is_parse_exception(), "{text}: {e:?}");
            assert_eq!(e.position, Some(place), "{text}: {e:?}");
        }
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
        model_refused_at(
            "###Service\nService s::S\n{\n  pattern: '/p';\n  documentation: 'd';\n  execution: Single\n  {\n    query: |1;\n    mapping: m::M;\n    runtime: r::R;\n  }\n  test: Single\n  {\n    data: 'x';\n    asserts: [ { [-99999999999999999999], |true } ];\n  }\n}\n",
            (2, 8),
        );
        model_refused_at(
            "###FileGeneration\nProtobuf a::G\n{\n  scopeElements: [a];\n  x: 99999999999999999999;\n}\n",
            (2, 8),
        );
    }

    #[test]
    fn an_exponent_with_no_digits_is_refused_at_its_letter() {
        // row 8
        for t in ["1.5ed", "1.5e+d", "1.5ef", "1.5e", "1.5Ed"] {
            expression_refused_at(t, (1, 4));
        }
        model_refused_at(&fun("1.5ed"), (3, 6));
        model_refused_at(
            "###Relational\nDatabase a::D\n(\n  Table T (ID INTEGER)\n  Join j(T.ID = 1.5ef)\n)\n",
            (5, 20),
        );
        // past an int: the engine's section catch-all
        model_refused_at(&fun("1.5e99999999999d"), (1, 1));
    }

    #[test]
    fn a_short_unicode_escape_is_the_section_catch_all_s() {
        // row 9, the frozen oracle's question 109b58da among them
        model_refused_at("Class 'a\\u12'::B\n{\n}\n", (1, 1));
        model_refused_at("###Pure\nimport 'a\\u12'::*;\nClass a::B\n{\n}\n", (2, 8));
        model_refused_at("###Mapping\nMapping 'a\\u12'::M\n(\n)\n", (2, 8));
        model_refused_at(
            "Profile a::P\n{\n  tags: [doc];\n}\nClass {a::P.doc='''\n\\u12'''} a::B\n{\n}\n",
            (1, 1),
        );
        model_refused_at(&fun("'x\\u12'::f()"), (1, 1));
        expression_refused_at("'x\\u12'::f()", (1, 1));
        // one line is no text block to the engine, which refuses at the string after the first `''`
        expression_refused_at("'''\\u12'''", (1, 3));
    }

    #[test]
    fn a_relational_sign_is_refused_where_it_stands() {
        // row 26
        let db =
            |o: &str| format!("###Relational\nDatabase a::D\n(\n  Table T (ID INTEGER)\n  Join j(T.ID = {o})\n)\n");
        model_refused_at(&db("- x"), (5, 17));
        model_refused_at(&db("- 'a'"), (5, 17));
        model_refused_at(&db("- .."), (5, 17));
        model_refused_at(
            "###Relational\nDatabase a::D\n(\n  Table T (ID INTEGER)\n  Join j(T.ID = -",
            (5, 17),
        );
    }

    #[test]
    fn a_service_store_local_property_multiplicity_parameter_is_the_section_catch_all_s() {
        // row 28
        model_refused_at(
            "###Mapping\nMapping a::M\n(\n  *a::C: ServiceStore\n  {\n    +x: String[m];\n    ~service [a::S] G.S\n  }\n)\n",
            (2, 8),
        );
    }

    #[test]
    fn the_end_of_the_input_is_where_the_engine_puts_it() {
        // rows 29 and 35
        model_refused_at(
            "###Mapping\nMapping a::M\n(\n  MappingTests\n  [\n    t\n    (\n      assert: '{}'",
            (8, 19),
        );
        model_refused_at(
            "###Mapping\nMapping a::M\n(\n  MappingTests\n  [\n    t\n    (\n      assert: '{}'\n",
            (9, 1),
        );
        // a `~src` is re-parsed on its own: one column before the end
        model_refused_at("###Mapping\nMapping a::M\n(\n  *a::C: Relation\n  {\n    ~src", (6, 8));
        model_refused_at(
            "###Service\nService s::S\n{\n  pattern: '/p';\n  documentation: 'd';\n  execution: Single { query: |1; mapping: m::M; runtime: #{",
            (6, 60),
        );
    }

    #[test]
    fn an_empty_aggregate_lambda_is_refused_at_its_terminator() {
        // row 32
        let agg = |g: &str| {
            format!(
                "###Mapping\nMapping a::M\n(\n  a::P[x]: AggregationAware\n  {{\n    Views: [\n      (\n        ~modelOperation: {{\n          ~canAggregate true,\n          ~groupByFunctions ({g}),\n          ~aggregateValues (\n            ( ~mapFn:$this.p , ~aggregateFn: $mapped->sum() )\n          )\n        }},\n        ~aggregateMapping: Pure\n        {{\n          ~src a::S\n          p: $src.p\n        }}\n      )\n    ],\n    ~mainMapping: Pure\n    {{\n      ~src a::P\n      p: $src.p\n    }}\n  }}\n)\n"
            )
        };
        model_refused_at(&agg(" ,"), (10, 31));
        model_refused_at(&agg("\n  ,"), (11, 3));
    }

    #[test]
    fn a_diagram_coordinate_and_a_data_quality_match_are_refused_at_the_token() {
        // row 49
        let dgm = |n: &str| {
            format!(
                "###Diagram\nDiagram a::D\n{{\n  classView id1\n  {{\n    class: a::C;\n    position: ({n}, 3.0);\n    rectangle: (1.0, 1.0);\n  }}\n}}\n"
            )
        };
        model_refused_at(&dgm("1-2"), (7, 16));
        model_refused_at(&dgm("\u{0661}\u{0662}"), (7, 16));
        model_refused_at(&dgm("1.2.3"), (7, 19));
        let dq = |m: &str| {
            format!(
                "###DataQualityValidation\nDataQualityRelationComparison a::R\n{{\n  source: src|#>{{a::D.T}}#;\n  target: tgt|#>{{a::D.T}}#;\n  keys: [A];\n  strategy: MD5Hash;\n  expectedMatch: {m};\n}}\n"
            )
        };
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
        model_refused_at(
            &format!("{p}  serviceOutputTargets: [ #/a::T/h# {{ keys: [ #/a::T/h/k1#,"),
            (7, 60),
        );
    }

    #[test]
    fn a_graph_fetch_argument_is_refused_where_the_engine_refuses_it() {
        // row 63, at the model door at the engine's dialect, the `lambda` door and the two engine-JSON doors
        graph_fetch_argument_refused_at(
            "function y4::b(): Any[*]\n{\n  #{I {product(2015-10-16){name}}}#;\n}\n",
            (3, 20),
        );
        graph_fetch_argument_refused_at(&fun("#{a::B{c(f())}}#"), (3, 13));
        graph_fetch_argument_refused_at(&fun("#{a::B{c(a::E)}}#"), (3, 16));
        graph_fetch_argument_refused_at(&fun("#{a::B{c($x.y)}}#"), (3, 14));
        graph_fetch_argument_refused_at(&fun("#{a::B{c(%10:10)}}#"), (3, 12));
        graph_fetch_argument_refused_at(&fun("#{a::B{c(~a)}}#"), (3, 12));
        graph_fetch_argument_refused_at(&fun("#{a::B{c(^a::B())}}#"), (3, 12));
        graph_fetch_argument_refused_at(&fun("#{a::B{c(1+2)}}#"), (3, 13));
        graph_fetch_argument_refused_at(&fun("#{a::B{c([f()])}}#"), (3, 14));
        // what the engine's walker fails on: its section catch-all
        graph_fetch_argument_refused_at(&fun("#{a::B{c(1.5d)}}#"), (1, 1));
        graph_fetch_argument_refused_at(&fun("#{a::B{c(-1)}}#"), (1, 1));
        graph_fetch_argument_refused_at(
            "Class a::X {}\n###Pure\nfunction a::f():Any[*]\n{\n  #{a::B{c(1.5d)}}#\n}\n",
            (3, 14),
        );
        for (text, place) in [("|#{a::B{c(f())}}#", (1, 12)), ("|#{a::B{c(1.5d)}}#", (1, 11))] {
            for e in [
                lambda(&u(text)).map(|_| ()).expect_err(text),
                lambda_json(&u(text), true).map(|_| ()).expect_err(text),
            ] {
                assert!(e.is_parse_exception(), "{text}: {e:?}");
                assert_eq!(e.position, Some(place), "{text}: {e:?}");
            }
        }
        // a string to the engine: read at the expression level, whatever door the text came through
        assert!(
            pmcd(
                &u(&fun("compileLegendValueSpecification('#{a::B{c(f())}}#')")),
                Dialect::LegendEngine,
                &[]
            )
            .parsed
            .is_ok()
        );
    }

    #[test]
    fn a_persistence_kind_and_a_data_quality_context_the_engine_does_not_name_are_refused() {
        // rows 73 and 76
        let p = "###Persistence\nPersistence a::P\n{\n  doc: 'd';\n  trigger: Manual;\n  service: a::S;\n";
        document_refused_at(&format!("{p}  persister: Foo\n  {{\n  }}\n}}\n"), (7, 14));
        document_refused_at(
            &format!("{p}  serviceOutputTargets: [ x {{ keys: [] }} -> {{ }} ];\n}}\n"),
            (7, 27),
        );
        let v = |c: &str| {
            format!(
                "###DataQualityValidation\nDataQualityValidation a::V\n{{\n  context: {c};\n  validationTree: $[\n    a::P<mustBeNamed>{{\n      name\n    }}\n  ]$;\n}}\n"
            )
        };
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
        model_refused_at(
            "###Pure\nProfile a::p { tags: [doc]; }\nClass {a::p.doc = '''hello'' a::A { x: String[1]; }\n",
            (3, 21),
        );
        model_refused_at("Class {meta::pure::profiles::doc.doc = '''\nDo", (1, 42));
        expression_refused_at("'''\nDo", (1, 3));
        expression_refused_at("'''", (1, 3));
    }

    #[test]
    fn an_impossible_date_with_a_zone_is_refused_at_its_token() {
        // row 3 (ruling 11): the engine reads the literal as text
        for t in ["%2023-02-29T10:30+0100", "%2024-02-30T10:00+0500"] {
            expression_refused_at(t, (1, 1));
            model_refused_at(&fun(t), (3, 3));
            let e = lambda(&u(t)).expect_err(t);
            assert_eq!((e.is_parse_exception(), e.position), (true, Some((1, 1))), "{e:?}");
        }
    }

    #[test]
    fn a_float_past_a_double_is_refused_at_lite_and_the_platform_and_read_at_the_engine() {
        // rows 7 and 8 (ruling 11): lite's and the platform's dialects alone
        let refused = |t: &str, place: (i32, i32)| {
            for d in [Dialect::LegendLite, Dialect::LegendPlatform] {
                let e = spec(&u(t), d).expect_err(t);
                assert_eq!((e.is_parse_exception(), e.position), (true, Some(place)), "{t}: {e:?}");
            }
            assert!(spec(&u(t), Dialect::LegendEngine).is_ok(), "{t}");
        };
        refused("1e999", (1, 1));
        refused("1.0e999", (1, 1));
        refused("-1e999", (1, 2));
        refused("1e999f", (1, 1));
        refused("1.5e99999999999", (1, 1));
        refused("1.5e99999999999f", (1, 1));
    }

    #[test]
    fn a_point_before_an_empty_exponent_is_refused_at_every_dialect() {
        // row 8's rest (ruling 11, corrected): the engine reads `1`, `.` and a name
        for t in ["1.ed", "1.e+d", "1.ef", "1.Ed", "1.e-d", "1.e"] {
            expression_refused_at(t, (1, 3));
            model_refused_at(&fun(t), (3, 5));
            let e = lambda(&u(t)).expect_err(t);
            assert_eq!((e.is_parse_exception(), e.position), (true, Some((1, 3))), "{t}: {e:?}");
        }
        // a sign with nothing after it the engine refuses at the sign, at its dialect
        assert_eq!(
            spec(&u("1.e+"), Dialect::LegendEngine).unwrap_err().position,
            Some((1, 4))
        );
        assert_eq!(
            spec(&u("1.e+"), Dialect::LegendLite).unwrap_err().position,
            Some((1, 3))
        );
    }

    #[test]
    fn an_empty_diagram_section_before_another_header_is_refused_at_its_header() {
        // row 46 (ruling 11)
        model_refused_at("###Diagram\n###Pure\nClass a::B\n{\n}\n", (1, 1));
        model_refused_at("Class a::X {}\n###Diagram\n###Mapping\nMapping a::M\n(\n)\n", (2, 1));
        for accepted in [
            "###Diagram",
            "###Diagram\n",
            "###Diagram\n\n###Pure\nClass a::B\n{\n}\n",
        ] {
            assert!(
                pmcd(&u(accepted), Dialect::LegendLite, &[]).parsed.is_ok(),
                "{accepted}"
            );
        }
    }

    #[test]
    fn a_json_schema_value_of_the_wrong_kind_is_refused_at_the_section() {
        // row 50's rest (ruling 11): the engine ignores these or does not visit them
        for schema in [
            r#"{"bsonType": "object", "title": 1}"#,
            r#"{"bsonType": "object", "description": true}"#,
            r#"{"bsonType": "object", "required": [null]}"#,
            r#"{"bsonType": "object", "minLength": 99999999999999999999}"#,
            r#"{"bsonType": "object", "foo": -99999999999999999999}"#,
            r#"{"bsonType": "object", "properties": {"a": {"bsonType": "string", "title": 1}}}"#,
            r#"{"bsonType": "object", "properties": {"a": {"bsonType": "string", "minLength": "x"}}}"#,
            r#"{"bsonType": "object", "properties": {"a": {"bsonType": "object", "required": [1]}}}"#,
        ] {
            model_refused_at(&mongo(schema), (2, 8));
        }
    }

    #[test]
    fn a_local_property_under_an_aggregation_aware_main_mapping_is_refused_at_it() {
        // row 72 (ruling 11), at the `document` door: the records have a place for it
        let aggregation = "###Mapping\nMapping a::M\n(\n  a::P[x]: AggregationAware\n  {\n    Views: [\n      (\n        ~modelOperation: {\n          ~canAggregate true,\n          ~groupByFunctions ($this.id),\n          ~aggregateValues (\n            ( ~mapFn:$this.p , ~aggregateFn: $mapped->sum() )\n          )\n        },\n        ~aggregateMapping: Pure\n        {\n          ~src a::S\n          p: $src.p\n        }\n      )\n    ],\n    ~mainMapping: Pure\n    {\n      ~src a::P\n      +loc: String[1]: $src.x\n    }\n  }\n)\n";
        only_document_refused_at(aggregation, (25, 8));
    }

    #[test]
    fn a_default_platform_with_a_body_is_refused_at_its_word() {
        // row 73's rest (ruling 11), at the `document` door
        let platform = |p: &str| {
            format!("###Persistence\nPersistenceContext a::C\n{{\n  persistence: a::P;\n  platform: {p};\n}}\n")
        };
        only_document_refused_at(&platform("Default #{ x: 1; }#"), (5, 13));
        only_document_refused_at(&platform("Default\n  #{\n    x: 1;\n  }#"), (5, 13));
        assert!(document(&u(&platform("Default")), true).is_ok());
    }

    #[test]
    fn a_positional_cast_is_refused_where_the_engine_refuses_it() {
        // row 69 (ruling 12), at the two engine-JSON doors
        lambda_json_refused_at("|^a::B($x)", (1, 8));
        lambda_json_refused_at("|^a::B( $x)", (1, 9));
        lambda_json_refused_at("|^a::B(\n  $x)", (2, 3));
        lambda_json_refused_at("|^a::B([1,2])", (1, 8));
        lambda_json_refused_at("|^a::B(-$x)", (1, 9));
        // a literal is the type's variable values: the engine wants the properties after the group
        lambda_json_refused_at("|^a::B(1)", (2, 1));
        lambda_json_refused_at("|^a::B(1)\n", (3, 1));
        lambda_json_refused_at("|\n^a::B(1)\n\n", (5, 1));
        lambda_json_refused_at("|^a::B(-1)", (2, 1));
        lambda_json_refused_at("|^a::B(1)->g()", (1, 10));
        lambda_json_refused_at("|^a::B(1 + 2)", (1, 10));
        lambda_json_refused_at("|f(^a::B(1), 2)", (1, 12));
        document_refused_at(&fun("^a::B($x)"), (3, 9));
        document_refused_at(&fun("^a::B(1)"), (4, 1));
        document_refused_at("function a::f():Any[*]\n{\n  ^a::B(1)\n\n  ->g()\n}\n", (5, 3));
        document_refused_at(
            "Class a::X {}\n###Pure\nfunction a::f():Any[*]\n{\n  ^a::B($x)\n}\n",
            (5, 9),
        );
    }

    #[test]
    fn a_wildcard_relation_column_is_refused_where_the_engine_refuses_it() {
        // row 69 (ruling 12)
        lambda_json_refused_at("|@(a:?)", (1, 6));
        lambda_json_refused_at("|@(a : ?)", (1, 8));
        lambda_json_refused_at("|@(?:Integer)", (1, 4));
        lambda_json_refused_at("|@(a:Integer, b:?)", (1, 17));
        lambda_json_refused_at("|@Relation<(a:?)>", (1, 11));
        lambda_json_refused_at("|@Relation\n<(a:?)>", (2, 1));
        document_refused_at(&fun("@(a:?)"), (3, 7));
        assert!(lambda_json(&u("|@(a:Integer)"), true).is_ok());
        assert!(lambda_json(&u("|@Relation<(a:Integer)>"), true).is_ok());
    }

    #[test]
    fn a_multiplicity_parameter_is_the_section_catch_all_s() {
        // row 70 (ruling 12): an expression has no section, the token
        document_refused_at("Class a::B\n{\n  p: String[m];\n}\n", (1, 1));
        document_refused_at("function a::f(x:String[m]):String[1]\n{\n  'a'\n}\n", (1, 1));
        document_refused_at("Class a::X {}\n###Pure\nClass a::B\n{\n  p: String[m];\n}\n", (3, 14));
        document_refused_at(
            "###Mapping\nMapping a::M\n(\n)\n###Pure\nClass a::B\n{\n  p: String[m];\n}\n",
            (6, 2),
        );
        lambda_json_refused_at("x:String[m]|1", (1, 10));
        lambda_json_refused_at("|@(a:Integer[m])", (1, 14));
    }

    #[test]
    fn a_deephaven_activation_configuration_is_refused_at_its_key() {
        // row 71 (ruling 12)
        let deephaven = |c: &str| {
            format!(
                "###Deephaven\nDeephavenApp a::A\n{{\n  applicationName: 'n';\n  function: a::f():Any[*];\n  ownership: Deployment {{ identifier: 'o' }};\n  {c}\n}}\n"
            )
        };
        document_refused_at(&deephaven("activationConfiguration: a::K;"), (7, 3));
        document_refused_at(&deephaven("activationConfiguration:\n    a::K;"), (7, 3));
    }

    #[test]
    fn a_dated_segments_argument_is_refused_where_the_engine_refuses_it() {
        // row 81 (ruling 12): a first line's columns shifted by the literal's own column and length, as the engine's
        // island writes them
        lambda_doors_refused_at("|#/a::B/c($x)#", (1, 23));
        lambda_doors_refused_at("|#/a::B/d/c($x)#", (1, 27));
        lambda_doors_refused_at("|#/a::B/c(1, $x)#", (1, 29));
        lambda_doors_refused_at("|#/a::B/c( $x )#", (1, 26));
        lambda_doors_refused_at("|#/a::B/c([1, $x])#", (1, 32));
        lambda_doors_refused_at("|#/a::B/c(x)#", (1, 23));
        lambda_doors_refused_at("|#/a::B/c(true)#", (1, 29));
        lambda_doors_refused_at("|#/a::B/c(1+2)#", (1, 25));
        lambda_doors_refused_at("|#/a::B/c(1.5d)#", (1, 25));
        lambda_doors_refused_at("|\n\n#/a::B/c($x)#", (3, 22));
        lambda_doors_refused_at("|#/a::B\n/c($x)#", (2, 4));
        model_refused_at_the_engines(&fun("#/a::B/c($x)#"), (3, 24));
    }

    #[track_caller]
    fn still_raw(text: &str, class: &str, door_answer: Answer<()>) {
        let e = door_answer.expect_err(text);
        assert_eq!(e.class, class, "{text}: {e:?}");
    }

    #[test]
    fn what_the_engine_accepts_and_no_ruling_refuses_keeps_its_answer() {
        // rulings 11 and 12 leave these: rows 68 and 69's `let`, row 81's numbers
        const UNSUPPORTED: &str = "java.lang.UnsupportedOperationException";
        for t in [
            "|letFunction(1, 2)",
            "|let y = %10:10:10",
            "|let y = @(a:Integer)",
            "|#/a::B/c(1.5)#",
            "|#/a::B/c(-1)#",
            "|#/a::B/c([.5])#",
            "|#/a::B/c(1.5f)#",
            "|#/a::B/c(E . X)#",
        ] {
            assert!(lambda(&u(t)).is_ok(), "{t}");
            still_raw(t, UNSUPPORTED, lambda_json(&u(t), true).map(|_| ()));
        }
    }

    fn persistence_test(island: &str) -> String {
        format!(
            "###Persistence\nPersistence a::P\n{{\n  doc: 'd';\n  trigger: Manual;\n  service: a::S;\n  tests:\n  [\n    t1:\n    {{\n      testBatches:\n      [\n        b1:\n        {{\n          data:\n          {{\n            connection:\n            {{\n              ExternalFormat\n              #{{\n                contentType: 'a';\n                data: 'd';\n              }}#\n            }}\n          }}\n          asserts:\n          [\n            a1:\n              EqualToJson {island}\n          ]\n        }}\n      ]\n      isTestDataFromServiceOutput: false;\n    }}\n  ]\n}}\n"
        )
    }

    #[test]
    fn a_persistence_assertion_entry_the_engine_has_no_rule_for_is_refused_where_it_stops() {
        // row 74 (ruling 15), at the `document` door: `expected: <identifier> #{ ... }#;` and nothing else, the columns the
        // document's on the island's first line
        const GOOD: &str = "expected: ExternalFormat #{ contentType: 'a'; data: 'd'; }#;";
        for (entry, place) in [
            ("expected: ['a'];", (29, 40)),
            ("expected: [a, b];", (29, 40)),
            ("expected: [];", (29, 40)),
            ("expected: #/a::B/c#;", (29, 40)),
            ("expected: [#/a::B/c#];", (29, 40)),
            ("expected: [#/a::B/c#, #/a::B/d#];", (29, 40)),
            ("expected: [{x: 1;}];", (29, 40)),
            ("expected   :    ['a'];", (29, 46)),
            ("database: a::D;", (29, 30)),
            ("binding: a::B;", (29, 30)),
            ("foo: ['a'];", (29, 30)),
            ("foo: #/a::B/c#;", (29, 30)),
            ("foo: [{x: 1;}];", (29, 30)),
            (
                "foo: ['a']; expected: ExternalFormat #{ contentType: 'a'; data: 'd'; }#;",
                (29, 30),
            ),
        ] {
            document_refused_at(&persistence_test(&format!("#{{ {entry} }}#")), place);
        }
        document_refused_at(&persistence_test(&format!("#{{ {GOOD} foo: ['a']; }}#")), (29, 91));
        // written with braces, the assertion is refused at its `{`
        document_refused_at(&persistence_test("{ expected: ['a']; }"), (29, 27));
        // on a later line of the island the column is the line's own
        document_refused_at(&persistence_test("#{\n  expected:\n    [a, b];\n}#"), (31, 5));
        document_refused_at(&persistence_test("#{\n  binding: a::B;\n}#"), (30, 3));
        assert!(document(&u(&persistence_test(&format!("#{{ {GOOD} }}#"))), true).is_ok());
    }

    #[test]
    fn a_graph_fetch_argument_is_refused_at_the_expression_doors_at_the_engines_dialect() {
        // row 63 (ruling 16)
        for (t, place) in [
            ("#{a::B{c(f())}}#", (1, 11)),
            ("#{a::B{c(a::E)}}#", (1, 14)),
            ("#{a::B{c($x.y)}}#", (1, 12)),
            ("#{a::B{c(%10:10)}}#", (1, 10)),
            ("#{a::B{c(~a)}}#", (1, 10)),
            ("#{a::B{c(1+2)}}#", (1, 11)),
            ("#{a::B{c([f()])}}#", (1, 12)),
            ("#{a::B{c(1, f())}}#", (1, 14)),
            ("#{a::B{\n  c(f())\n}}#", (2, 6)),
            ("1->from(#{a::B{c(f())}}#)", (1, 19)),
            // the engine's walker fails on these and answers raw: the token that holds it
            ("#{a::B{c(1.5d)}}#", (1, 10)),
            ("#{a::B{c(-1)}}#", (1, 10)),
        ] {
            for e in [
                spec(&u(t), Dialect::LegendEngine).map(|_| ()).expect_err(t),
                block(&u(t), Dialect::LegendEngine).map(|_| ()).expect_err(t),
            ] {
                assert!(e.is_parse_exception(), "{t}: {e:?}");
                assert_eq!(e.position, Some(place), "{t}: {e:?}");
            }
            for d in [Dialect::LegendLite, Dialect::LegendPlatform] {
                assert!(spec(&u(t), d).is_ok(), "{t}");
                assert!(block(&u(t), d).is_ok(), "{t}");
            }
        }
        // what the engine reads, and a tree quoted in a string
        for t in [
            "#{a::B{c(1), d('a'), e($x), f(a::E.X), g}}#",
            "compileLegendValueSpecification('#{a::B{c(f())}}#')",
        ] {
            assert!(spec(&u(t), Dialect::LegendEngine).is_ok(), "{t}");
        }
    }

    #[test]
    fn a_dated_segments_argument_is_refused_at_every_door_at_the_engines_dialect() {
        // row 81 (ruling 16): a first line's columns shifted by the literal's own column and length, as the engine's
        // island writes them
        for (arg, place) in [
            ("a::E.X", (1, 27)),
            ("a::b::E.X", (1, 30)),
            ("E.X.Y", (1, 28)),
            ("1,", (1, 24)),
            (",1", (1, 22)),
            (",", (1, 21)),
            ("1,,2", (1, 26)),
            ("%10:10", (1, 26)),
            (" %10:10 ", (1, 29)),
            ("1, %10:10", (1, 32)),
            ("[a::E.X]", (1, 30)),
            ("[1,]", (1, 27)),
            ("[,1]", (1, 25)),
            ("[%10:10]", (1, 29)),
            ("[[1]]", (1, 26)),
            ("$x", (1, 22)),
            ("x", (1, 22)),
            ("true", (1, 28)),
            ("1e5", (1, 26)),
            ("1.5d", (1, 24)),
            ("1.", (1, 23)),
            ("%x", (1, 22)),
            ("E.", (1, 24)),
        ] {
            let t = format!("#/a::B/c({arg})#");
            let answers = [
                spec(&u(&t), Dialect::LegendEngine).map(|_| ()),
                block(&u(&t), Dialect::LegendEngine).map(|_| ()),
                lambda(&u(&t)).map(|_| ()),
                lambda_json(&u(&t), true).map(|_| ()),
            ];
            for answer in answers {
                let e = answer.expect_err(&t);
                assert!(e.is_parse_exception(), "{t}: {e:?}");
                assert_eq!(e.position, Some(place), "{t}: {e:?}");
            }
            let in_a_function = (3, place.1 + 2);
            let f = fun(&t);
            for e in [
                pmcd(&u(&f), Dialect::LegendEngine, &[])
                    .parsed
                    .map(|_| ())
                    .expect_err(&f),
                document(&u(&f), true).map(|_| ()).expect_err(&f),
            ] {
                assert!(e.is_parse_exception(), "{f}: {e:?}");
                assert_eq!(e.position, Some(in_a_function), "{f}: {e:?}");
            }
        }
        assert_eq!(
            lambda(&u("|\n\n#/a::B/c(a::E.X)#")).unwrap_err().position,
            Some((3, 27))
        );
        assert_eq!(lambda(&u("|#/a::B\n/c(a::E.X)#")).unwrap_err().position, Some((2, 5)));
        // what the engine accepts, and lite's and the platform's dialects
        for arg in [
            "E.X",
            "E . X",
            "1",
            "'a', 1",
            "1 , 2",
            "[E.X]",
            "[]",
            "",
            "%2020-01-01",
            "%2020",
            "%latest",
            "1.5",
            "-1",
            "1.5f",
            ".5",
            "\n1\n",
        ] {
            let t = format!("#/a::B/c({arg})#");
            assert!(spec(&u(&t), Dialect::LegendEngine).is_ok(), "{t}");
        }
        for d in [Dialect::LegendLite, Dialect::LegendPlatform] {
            assert!(spec(&u("#/a::B/c(a::E.X)#"), d).is_ok());
            assert!(spec(&u("#/a::B/c(1,)#"), d).is_ok());
        }
    }

    fn platform(p: &str) -> String {
        format!("###Persistence\nPersistenceContext a::C\n{{\n  persistence: a::P;\n  platform: {p};\n}}\n")
    }

    /// `text` refused at the model door at the engine's dialect and at the `document` door, at `place`; accepted at lite's
    /// and the platform's dialects.
    #[track_caller]
    fn refused_at_the_engines_dialect(text: &str, place: (i32, i32)) {
        let units = u(text);
        for d in [Dialect::LegendLite, Dialect::LegendPlatform] {
            assert!(pmcd(&units, d, &[]).parsed.is_ok(), "{text}");
        }
        for e in [
            pmcd(&units, Dialect::LegendEngine, &[])
                .parsed
                .map(|_| ())
                .expect_err(text),
            document(&units, true).map(|_| ()).expect_err(text),
        ] {
            assert!(e.is_parse_exception(), "{text}: {e:?}");
            assert_eq!(e.position, Some(place), "{text}: {e:?}");
        }
    }

    #[test]
    fn a_platforms_island_is_read_as_the_engine_reads_it() {
        // row 73's family (ruling 16): one character is the walker's cut failing, its section catch-all
        refused_at_the_engines_dialect(&platform("Default #{ }#"), (2, 8));
        refused_at_the_engines_dialect(&platform("Default #{\n}#"), (2, 8));
        // the cut takes the text's last two characters: a one-line island loses its `;` or more
        refused_at_the_engines_dialect(&platform("AwsGlue #{ dataProcessingUnits: 1; }#"), (5, 45));
        refused_at_the_engines_dialect(&platform("AwsGlue #{ dataProcessingUnits: 10; }#"), (5, 46));
        refused_at_the_engines_dialect(&platform("AwsGlue #{dataProcessingUnits:1;}#"), (5, 43));
        refused_at_the_engines_dialect(&platform("AwsGlue #{ dataProcessingUnits: 'a'; }#"), (5, 45));
        refused_at_the_engines_dialect(&platform("AwsGlue #{ dataProcessingUnits: a::B; }#"), (5, 45));
        refused_at_the_engines_dialect(&platform("AwsGlue #{ foo: 10; dataProcessingUnits: 10; }#"), (5, 24));
        refused_at_the_engines_dialect(&platform("AwsGlue #{ dataProcessingUnits: 10; foo: 1; }#"), (5, 49));
        refused_at_the_engines_dialect(
            &platform("AwsGlue #{\n    dataProcessingUnits: 10;\n    foo: 1;\n  }#"),
            (7, 5),
        );
        // the islands the engine reads, and a default's whatever its text
        for p in [
            "Default",
            "Default #{}#",
            "Default #{  }#",
            "AwsGlue #{\n    dataProcessingUnits: 10;\n  }#",
            "AwsGlue\n  #{\n    dataProcessingUnits: 1;\n  }#",
        ] {
            assert!(document(&u(&platform(p)), true).is_ok(), "{p}");
        }
        // refused already: the cut's own answers at the engine's place; what no ruling names where it stood
        for (p, place) in [
            ("AwsGlue #{}#", (5, 13)),
            ("Foo #{ }#", (2, 8)),
            ("Foo", (5, 16)),
            ("Foo #{ x: 1; }#", (5, 17)),
            ("AwsGlue", (5, 20)),
        ] {
            let t = platform(p);
            let e = pmcd(&u(&t), Dialect::LegendEngine, &[])
                .parsed
                .map(|_| ())
                .expect_err(&t);
            assert_eq!(e.position, Some(place), "{t}: {e:?}");
        }
    }

    #[test]
    fn a_positional_casts_second_argument_is_refused_where_the_engine_stops() {
        // row 69 (ruling 16), at every door at the engine's dialect; the record doors read the one-argument form as before
        let expression = |t: &str, place: (i32, i32)| {
            for e in [
                spec(&u(t), Dialect::LegendEngine).map(|_| ()).expect_err(t),
                block(&u(t), Dialect::LegendEngine).map(|_| ()).expect_err(t),
            ] {
                assert_eq!((e.is_parse_exception(), e.position), (true, Some(place)), "{t}: {e:?}");
            }
        };
        expression("^a::B($x, $y)", (1, 7));
        expression("^a::B($x,)", (1, 7));
        expression("^a::B(\n$x,\n$y)", (2, 1));
        // literals are the type's variable values: past the group
        expression("^a::B(1, 2)", (1, 12));
        expression("^a::B('a', 'b')", (1, 16));
        expression("^a::B(1, -2)", (1, 13));
        expression("^a::B(-1, 2)", (1, 13));
        expression("^a::B(1, 2, 3)", (1, 15));
        expression("^a::B(\n1,\n2)", (3, 3));
        expression("^a::B(1, 2)->g()", (1, 12));
        expression("f(^a::B(1, 2), 3)", (1, 14));
        // the first that is no literal
        expression("^a::B(1, $x)", (1, 10));
        expression("^a::B(1,)", (1, 9));
        expression("^a::B(1, f())", (1, 10));
        expression("^a::B(1, -'a')", (1, 11));
        expression("^a::B(1+2, 3)", (1, 8));
        expression("^a::B('a'+'b', 1)", (1, 11));
        // past a text's end the `lambda` door's wrapper is on the line below; a function's `}` where it stands
        for t in ["|^a::B(1, 2)", "|^a::B(1, 2)\n"] {
            let line = if t.ends_with('\n') { 3 } else { 2 };
            assert_eq!(lambda(&u(t)).unwrap_err().position, Some((line, 1)), "{t}");
        }
        model_refused_at_the_engines(&fun("^a::B(1, 2)"), (4, 1));
        model_refused_at_the_engines(&fun("^a::B($x, $y)"), (3, 9));
        // a string before a `+` is a key's `+=`: past the `+` (one column off until 2026-11-09)
        lambda_json_refused_at("|^a::B('a'+'b')", (1, 12));
        lambda_json_refused_at("|^a::B('a' + 'b')", (1, 14));
        lambda_json_refused_at("|^a::B('a'+$x)", (1, 12));
        // lite's and the platform's dialects keep the parser's words
        for d in [Dialect::LegendLite, Dialect::LegendPlatform] {
            assert_eq!(spec(&u("^a::B($x, $y)"), d).unwrap_err().position, Some((1, 9)));
        }
    }

    #[track_caller]
    fn model_refused_at_the_engines(text: &str, place: (i32, i32)) {
        let units = u(text);
        for e in [
            pmcd(&units, Dialect::LegendEngine, &[])
                .parsed
                .map(|_| ())
                .expect_err(text),
            document(&units, true).map(|_| ()).expect_err(text),
        ] {
            assert!(e.is_parse_exception(), "{text}: {e:?}");
            assert_eq!(e.position, Some(place), "{text}: {e:?}");
        }
    }

    #[test]
    fn a_later_syntax_error_of_the_section_comes_before_what_the_walk_refuses() {
        // row 63 (ruling 16): the engine parses a section whole before it walks it
        const BAD: &str = "#{a::B{c(f())}}#";
        // the same function, the next element
        model_refused_at_the_engines(&format!("function a::f():Any[*]\n{{\n  {BAD};\n  )\n}}\n"), (4, 3));
        model_refused_at_the_engines(&format!("{}Class a::B {{ x: }}\n", fun(BAD)), (5, 17));
        // the refusal comes first against a later section, the rest of its island, and what the walk raises after it
        let refused = (3, 13);
        model_refused_at_the_engines(&format!("{}###Pure\nClass a::B {{ x: }}\n", fun(BAD)), refused);
        model_refused_at_the_engines(&fun("#{a::B{c(f()), d(}}#"), refused);
        model_refused_at_the_engines(&fun("#{a::B{c(f()) d}}#"), refused);
        model_refused_at_the_engines(
            &format!("{};\n  #{{a::B{{d(}}}}#;", fun(BAD).trim_end_matches("\n}\n"))
                .replace("\n  #{a::B{d(}}#;", "\n  #{a::B{d(}}#;\n}\n"),
            refused,
        );
        // the words of the engine's walkers
        let service = format!(
            "###Service\nService a::S\n{{\n  pattern: '/p';\n  documentation: 'd';\n  execution: Single\n  {{\n    query: |{BAD};\n    mapping: m::M;\n    runtime: r::R;\n  }}\n}}\n"
        );
        model_refused_at_the_engines(
            &format!("{service}Service a::T\n{{\n  documentation: 'd';\n}}\n"),
            (8, 23),
        );
        model_refused_at_the_engines(&format!("{service}Service a::T\n{{\n  pattern: \n}}\n"), (16, 1));
        // a mapping element's body is the walk's: another's error comes after, its own syntax error before, the mapping's
        // own before both
        let mapping = |rest: &str| {
            format!("###Mapping\nMapping a::M\n(\n  *a::C: Pure\n  {{\n    ~src a::S\n    x: {BAD}{rest}\n)\n")
        };
        model_refused_at_the_engines(&mapping("\n  }\n  *a::D: Pure\n  {\n    ~src\n  }"), (7, 18));
        model_refused_at_the_engines(&mapping(",\n    y: )\n  }"), (8, 8));
        model_refused_at_the_engines(&mapping("\n  }\n  x"), (10, 1));
        // the expression doors and the `lambda` door
        for t in [format!("{BAD} 1 +"), format!("{BAD} x")] {
            assert_eq!(
                spec(&u(&t), Dialect::LegendEngine).unwrap_err().position,
                Some((1, 18)),
                "{t}"
            );
        }
        assert_eq!(lambda(&u(&format!("|{BAD};\n)"))).unwrap_err().position, Some((2, 1)));
        assert_eq!(lambda(&u("|#{a::B{c(f()), d(}}#")).unwrap_err().position, Some((1, 12)));
        // the other refusals of the walk
        model_refused_at_the_engines(&format!("{}Class a::B {{ x: }}\n", fun("#/a::B/c(1,)#")), (5, 17));
        let context = "PersistenceContext a::D\n{\n  persistence: \n}\n";
        model_refused_at_the_engines(&format!("{}{context}", platform("Default #{ }#")), (10, 1));
        model_refused_at_the_engines(
            &format!("{}###Pure\nClass a::B {{ x: }}\n", platform("Default #{ }#")),
            (2, 8),
        );
        // an error the engine's own section parse raises keeps its place
        model_refused_at_the_engines(&fun("1.5ed"), (3, 6));
    }

    /// Each family of the user's ruling 20 of 2026-11-10 (docs/rust-port/PHASE2.md section 6; register S354 to S367): refused
    /// at the engine's dialect, at the place legend-engine's own parser gives (4.145.0, asked 2026-11-10), and read at lite's
    /// and the platform's dialects as before but for the line comment, whose rule is the lexer's.
    #[track_caller]
    fn refused_at_the_engines_alone(kind: &str, text: &str, place: (i32, i32)) {
        let units = u(text);
        if kind == "spec" {
            assert_eq!(
                spec(&units, Dialect::LegendEngine).unwrap_err().position,
                Some(place),
                "{text}"
            );
            for d in [Dialect::LegendLite, Dialect::LegendPlatform] {
                assert!(spec(&units, d).is_ok(), "{text} at {d:?}");
            }
            return;
        }
        model_refused_at_the_engines(text, place);
        for d in [Dialect::LegendLite, Dialect::LegendPlatform] {
            assert!(pmcd(&units, d, &[]).parsed.is_ok(), "{text} at {d:?}");
        }
    }

    /// A v1 persister whose ingest mode's and target shape's last lines are `ingest` and `shape`.
    fn persister(ingest: &str, shape: &str) -> String {
        format!(
            "###Persistence\nPersistence a::P\n{{\n  doc: 'd';\n  trigger: Manual;\n  service: a::S;\n  persister: Batch\n  {{\n    \
             sink: Relational\n    {{\n      database: a::D;\n    }}\n    targetShape: Flat\n    {{\n      targetName: 'T';\n      \
             modelClass: a::C;\n{shape}    }}\n    ingestMode: NontemporalDelta\n    {{\n      mergeStrategy: NoDeletes;\n{ingest}    \
             }}\n  }}\n}}\n"
        )
    }

    #[test]
    fn the_three_looser_inputs_found_by_hand_are_refused_at_the_engines_place() {
        // ruling 26 of 2026-11-10, built 2026-11-11 (register S369 to S371): each input and place read from the engine's
        // parser (`PureGrammarParser`, 4.145.0)
        // a persister's bare kind written with a body or an island (S369)
        let none = "      auditing: None;\n";
        refused_at_the_engines_alone("model", &persister("      auditing: None {}\n", ""), (21, 22));
        refused_at_the_engines_alone(
            "model",
            &persister("      auditing: None { dateTimeName: 'x'; }\n", ""),
            (21, 22),
        );
        refused_at_the_engines_alone("model", &persister("      auditing: None #{ }#\n", ""), (21, 22));
        let no_deletes = persister(none, "").replace("NoDeletes;", "NoDeletes {}");
        refused_at_the_engines_alone("model", &no_deletes, (20, 32));
        for kind in ["None", "AnyVersion"] {
            let shape = format!("      deduplicationStrategy: {kind} {{}}\n");
            refused_at_the_engines_alone("model", &persister(none, &shape), (17, 30 + kind.len() as i32 + 1));
        }
        // a target's island is another grammar's, which takes a body after `None`
        let island = "###Persistence\nPersistence a::P\n{\n  doc: 'd';\n  trigger: Manual;\n  service: a::S;\n  \
                      serviceOutputTargets:\n  [\n    TDS\n    {\n      keys: [ foo ]\n      datasetType: Delta\n      {\n        \
                      actionIndicator: None;\n      }\n      deduplication: AnyVersion;\n    }\n    ->\n    Relational\n    #{\n      \
                      table: TableA;\n      database: a::D;\n      temporality: None\n      {\n        updatesHandling: Overwrite;\n        \
                      auditing: None {}\n      }\n    }#\n  ];\n}\n";
        assert!(pmcd(&u(island), Dialect::LegendEngine, &[]).parsed.is_ok());
        // a constraint's keyword written as a column spec, where the engine's domain lexer reads the text (S370)
        refused_at_the_engines_alone("spec", "[~owner: abc]", (1, 2));
        for word in ["owner", "externalId", "function", "message", "enforcementLevel"] {
            refused_at_the_engines_alone("spec", &format!("$t->extend(~{word}: x|1)"), (1, 12));
        }
        refused_at_the_engines_alone("spec", "$t->extend(~owner\n\t : x|1)", (1, 12));
        refused_at_the_engines_alone("spec", "$t->extend(~owner:Integer[1])", (1, 12));
        refused_at_the_engines_alone(
            "model",
            "function a::f(): Any[*]\n{\n  $t->extend(~owner: x|1)\n}\n",
            (3, 14),
        );
        let e = lambda(&u("|$t->extend(~owner: x|1)")).unwrap_err();
        assert_eq!(e.position, Some((1, 13)));
        // a blank after the `~`, a comment before the `:`, a longer word, or another section's lexer: no constraint token
        for text in [
            "$t->extend(~ owner: x|1)",
            "$t->extend(~owner/*c*/: x|1)",
            "$t->extend(~ownerX: x|1)",
        ] {
            assert!(spec(&u(text), Dialect::LegendEngine).is_ok(), "{text}");
        }
        let mapping = "###Mapping\nMapping my::M ( *model::Mammal: Pure { ~src model::Mammal noOfLegs: \
                       $src->extend(~owner: x|1) } )\n";
        assert!(pmcd(&u(mapping), Dialect::LegendEngine, &[]).parsed.is_ok());
        // DataQuality's comma lists, `item (COMMA item)*`, and a data space's context name a string (S371)
        let tree = |constraints: &str, property: &str, context: &str| {
            format!(
                "###DataQualityValidation\nDataQualityValidation my::Check\n{{\n  context: {context};\n  validationTree: $[\n    \
                 my::P<{constraints}>{{\n      name<{property}>\n    }}\n  ]$;\n}}\n"
            )
        };
        let mapping_and_runtime = "fromMappingAndRuntime(my::M, my::R)";
        refused_at_the_engines_alone("model", &tree("c1,", "c1", mapping_and_runtime), (6, 14));
        refused_at_the_engines_alone("model", &tree("c1", "c1,", mapping_and_runtime), (7, 15));
        refused_at_the_engines_alone("model", &tree("c1", "c1", "fromDataSpace(my::DS, my::x)"), (4, 34));
        assert!(
            pmcd(&u(&tree("c1", "c1", mapping_and_runtime)), Dialect::LegendEngine, &[])
                .parsed
                .is_ok()
        );
        let checks = |validations: &str| {
            format!(
                "###DataQualityValidation\nDataQualityRelationValidation meta::x::v\n{{\n   query: \
                 |#>{{my::Store.myTable}}#->filter(c|$c.name == 'ok');\n   validations: {validations};\n}}\n"
            )
        };
        let v1 = "{ name: 'v1'; assertion: row|$row.name != 'error'; }";
        refused_at_the_engines_alone("model", &checks(&format!("[ {v1}, ]")), (5, 70));
        refused_at_the_engines_alone("model", &checks("[]"), (5, 18));
        let comparison = |keys: &str, columns: &str| {
            format!(
                "###DataQualityValidation\nDataQualityRelationComparison meta::x::r\n{{\n   source: \
                 src|#>{{my::Store.sourceTable}}#->filter(c|$c.active == true);\n   target: \
                 tgt|#>{{my::Store.targetTable}}#->filter(c|$c.active == true);\n   keys: {keys};\n   columnsToCompare: \
                 {columns};\n   strategy: MD5Hash;\n}}\n"
            )
        };
        refused_at_the_engines_alone("model", &comparison("[id,]", "[a]"), (6, 14));
        refused_at_the_engines_alone("model", &comparison("[id]", "[a, b,]"), (7, 28));
        assert!(
            pmcd(&u(&comparison("[]", "[]")), Dialect::LegendEngine, &[])
                .parsed
                .is_ok()
        );
    }

    #[test]
    fn of_two_refusals_in_one_element_the_engines_first_is_answered() {
        // ruling 27 of 2026-11-11 (register S362, S365, S366): each text and place read from the engine's parser
        // (`PureGrammarParser`, 4.145.0); one refusal alone answers as before
        let default = |d: &str| format!("Class a::C\n{{\n  p: Integer[1] = {d};\n}}\n");
        // a default value: where its grammar stops before lite's own reading of it refuses, and the reverse
        model_refused_at_the_engines(&default("x.y + (1 +)"), (3, 23));
        model_refused_at_the_engines(&default("'a' + 'b' + "), (3, 23));
        model_refused_at_the_engines(&default("[1, x.y + 2, (1 +)]"), (3, 27));
        model_refused_at_the_engines(&default("x|1 + "), (3, 20));
        model_refused_at_the_engines(&default("x.y + 1;\n  q: Integer[1 = 2"), (3, 23));
        model_refused_at_the_engines(&default("a::f() + [1, 2 3]"), (3, 26));
        // a `::` the name's grammar has read is refused at what follows it
        model_refused_at_the_engines(&default("[a::b::-c.d]"), (3, 26));
        // a HostedService's lists: refused where they stand, before a later refusal of the element
        let hosted = |close: &str, tail: &str| {
            format!(
                "###HostedService\nHostedService a::hs\n{{\n   pattern: '/x';\n   ownership: Deployment {{ identifier: 'a' }};\n   \
                 function: a::f():String[1];\n   postValidations:\n   [\n      {{\n         description: 'x';\n         \
                 params: [];\n         assertions: [\n            a1: tds: TabularDataSet[1]|$tds->f();\n         ];\n      \
                 }}{close}\n{tail}}}\n"
            )
        };
        model_refused_at_the_engines(&hosted(",\n   ]", "   documentation: 'd'\n"), (16, 4));
        model_refused_at_the_engines(&hosted("\n   ];", "   documentation: 'd'\n"), (16, 5));
        model_refused_at_the_engines(&hosted("\n   ];", "   postValidations: [];\n"), (16, 5));
        model_refused_at_the_engines(&hosted("\n   ];", ""), (16, 5));
        // a Pure class mapping: its body's syntax, then its `~src` and `~filter` given twice, then each expression's reparse
        let pure =
            |body: &str| format!("###Mapping\nMapping a::m\n(\n  a::A: Pure\n  {{\n    ~src a::S\n{body}\n  }}\n)\n");
        model_refused_at_the_engines(&pure("    ~filter a::b\n    ~filter $src.y"), (4, 3));
        model_refused_at_the_engines(&pure("    ~filter a::b\n    ~src a::T"), (4, 3));
        model_refused_at_the_engines(&pure("    name: a::b,\n    other[2]: $src.o"), (8, 11));
        model_refused_at_the_engines(&pure("    name: a::b,\n    other: [1, 2 3]"), (8, 18));
        model_refused_at_the_engines(
            &pure("    name: a::b,\n    other: $src->graphFetch(#{a::B{c(f())}}#)"),
            (7, 15),
        );
        model_refused_at_the_engines(&pure("    name: a::b,\n    other: a::c"), (7, 15));
        model_refused_at_the_engines(&pure("    name[2]: $src.x,\n    other: a::b"), (7, 10));
    }

    #[test]
    fn a_data_space_executables_id_is_the_engines_executable_id() {
        // ruling 28 of 2026-11-11 (register S373): `(VALID_STRING | INTEGER | DECIMAL) SEMI_COLON` by the engine's lexer,
        // each text and place read from the engine's parser (`PureGrammarParser`, 4.145.0)
        let ds = |id: &str| {
            format!(
                "###DataSpace\nDataSpace model::dataSpace\n{{\n  title: 'some title';\n  executables:\n  [\n    {{\n      \
                 id: {id};\n      title: 'Template 1';\n      query: |model::element.all();\n    }}\n  ];\n}}\n"
            )
        };
        for id in [
            "'e'", "1.5", "-1", "true", "title", "all", "%2020", "$x", "''", "1.5f", ".5",
        ] {
            refused_at_the_engines_alone("model", &ds(id), (8, 11));
        }
        // a letter the engine's lexer does not have, which lite's refuses at every dialect
        model_refused_at_the_engines(&ds("\u{e9}"), (8, 11));
        for (id, column) in [
            ("a::b", 12),
            ("e f", 13),
            ("e.f", 12),
            ("1.e5", 12),
            ("x-y", 12),
            ("x.1", 12),
        ] {
            refused_at_the_engines_alone("model", &ds(id), (8, column));
        }
        model_refused_at_the_engines(&ds(""), (8, 11));
        model_refused_at_the_engines(&ds("e").replace("id: e;", "id: e"), (9, 7));
        for id in [
            "1",
            "e",
            "1.5d",
            "1d",
            "2Id",
            "1e5",
            "_x",
            "Class",
            "import",
            "99999999999999999999",
            "0x10",
            "x$y",
            "titles",
            "TITLE",
            ".5d",
            "1.5E+3d",
            "e/*c*/",
            "007",
        ] {
            assert!(pmcd(&u(&ds(id)), Dialect::LegendEngine, &[]).parsed.is_ok(), "{id}");
        }
        // with a later refusal in the element, the id's comes first
        model_refused_at_the_engines(&ds("'e'").replace("'Template 1'", "1"), (8, 11));
        model_refused_at_the_engines(&ds("'e'").replace("  ];\n", "  ]\n"), (8, 11));
    }

    #[test]
    fn a_line_comment_ends_at_a_lone_carriage_return() {
        // a lone `\r` ends a line comment, as the engines' lexers end it
        model_refused_at("// a\r@", (1, 6));
        for d in DIALECTS {
            assert!(pmcd(&u("Class a::B { // c\r}"), d, &[]).parsed.is_ok(), "{d:?}");
        }
    }

    #[test]
    fn persistence_is_held_to_the_engines_grammar_and_walk() {
        // persistence: a test's key order, a service output's keys and the arm its kind takes, the `;` its grammar spells
        refused_at_the_engines_alone(
            "model",
            "###Persistence\nPersistence a::P\n{\n  doc: 'd';\n  trigger: Manual;\n  service: a::S;\n  tests:\n  [\n    t1:\n    {\n      isTestDataFromServiceOutput: false;\n      testBatches:\n      [\n      ]\n    }\n  ]\n}\n",
            (11, 7),
        );
        refused_at_the_engines_alone(
            "model",
            "###Persistence\nPersistence a::P\n{\n  doc: 'd';\n  trigger: Manual;\n  service: a::S;\n  serviceOutputTargets:\n  [\n    #/a::T/p#\n    {\n      keys: [ foo ]\n    }\n    ->\n    {\n    }\n  ];\n}\n",
            (2, 8),
        );
        refused_at_the_engines_alone(
            "model",
            "###Persistence\nPersistence a::P\n{\n  doc: 'd';\n  trigger: Manual;\n  service: a::S;\n  serviceOutputTargets:\n  [\n    TDS\n    {\n      keys: [ #/a::T/q# ]\n    }\n    ->\n    {\n    }\n  ];\n}\n",
            (2, 8),
        );
        refused_at_the_engines_alone(
            "model",
            "###Persistence\nPersistence a::P\n{\n  doc: 'd';\n  trigger: Manual;\n  service: a::S;\n  serviceOutputTargets:\n  [\n    #/a::T/p#\n    {\n      datasetType: Delta { actionIndicator: None; }\n    }\n    ->\n    {\n    }\n  ];\n}\n",
            (9, 5),
        );
        refused_at_the_engines_alone(
            "model",
            "###Persistence\nPersistence a::P\n{\n  doc: 'd';\n  trigger: Manual;\n  service: a::S;\n  serviceOutputTargets:\n  [\n    TDS\n    {\n      keys: [ foo ];\n    }\n    ->\n    {\n    }\n  ];\n}\n",
            (11, 20),
        );
        refused_at_the_engines_alone(
            "model",
            "###Persistence\nPersistence a::P\n{\n  doc: 'd';\n  trigger: Manual;\n  service: a::S;\n  serviceOutputTargets:\n  [\n    TDS\n    {\n      keys: [ foo ]\n      datasetType: Delta { actionIndicator: None; };\n    }\n    ->\n    {\n    }\n  ];\n}\n",
            (12, 52),
        );
        refused_at_the_engines_alone(
            "model",
            "###Persistence\nPersistence a::P\n{\n  doc: 'd';\n  trigger: Manual;\n  service: a::S;\n  serviceOutputTargets:\n  [\n    TDS\n    {\n      keys: [ foo ]\n    }\n    ->\n    {\n    }\n  ]\n}\n",
            (17, 1),
        );
    }

    #[test]
    fn element_orders_and_lists_are_the_engines() {
        // element orders: a `RelationalMapper`'s lists, a generation node's id, a generation list's comma, a MongoDB mapping's `~filter`
        // two comma lists ruling 18's helper had not reached: a DataQuality tree's, a HostedService's `UserList`
        refused_at_the_engines_alone(
            "model",
            "###DataQualityValidation\nDataQualityValidation a::V\n{\n   context: fromMappingAndRuntime(a::M, a::R);\n   validationTree: $[\n      a::P{\n        name,\n      }\n    ]$;\n}\n",
            (8, 7),
        );
        refused_at_the_engines_alone(
            "model",
            "###HostedService\nHostedService a::H\n{\n   pattern: '/a';\n   ownership: UserList { users: ['a',] };\n   function: a::f():TabularDataSet[1];\n   documentation: '';\n   autoActivateUpdates: true;\n}\n",
            (5, 38),
        );
        refused_at_the_engines_alone(
            "model",
            "###QueryPostProcessor\nRelationalMapper a::R\n(\n   TableMappers:\n   [\n      a::DB.s1.t -> 'T'\n   ];\n   SchemaMappers:\n   [\n      a::DB.s1 -> 'S'\n   ];\n)\n",
            (8, 4),
        );
        refused_at_the_engines_alone(
            "model",
            "###GenerationSpecification\nGenerationSpecification a::G\n{\n   generationNodes: [\n      {\n         generationElement: a::E;\n         id: 'x';\n      }\n   ];\n}\n",
            (7, 10),
        );
        refused_at_the_engines_alone(
            "model",
            "###GenerationSpecification\nGenerationSpecification a::G\n{\n   fileGenerations: [a::F, ];\n}\n",
            (4, 28),
        );
        refused_at_the_engines_alone(
            "model",
            "###Mapping\nMapping a::M\n(\n  *a::Person[p]: MongoDB\n  {\n    ~mainCollection [a::Db] Person\n    ~filter [a::Db] f\n  }\n)\n",
            (3, 5),
        );
        refused_at_the_engines_alone(
            "model",
            "Class a::B{}\n###Mapping\nMapping a::M\n(\n  *a::Person[p]: MongoDB { ~mainCollection [a::Db] Person ~filter [a::Db] f }\n)\n",
            (2, 33),
        );
    }

    #[test]
    fn a_percent_literal_ends_where_the_engines_lexer_ends_it() {
        // a `%` literal as the engine's lexer ends it, and a milestoning's infinity date
        refused_at_the_engines_alone("spec", "%19:41:.21", (1, 7));
        refused_at_the_engines_alone("spec", "%204T00:00:00", (1, 5));
        refused_at_the_engines_alone(
            "model",
            "###Relational\nDatabase a::D\n(\n  Table t\n  (\n    milestoning\n    (\n      processing(PROCESSING_IN = i, PROCESSING_OUT = o, INFINITY_DATE = %9999-00:00.0000)\n    )\n    i INT, o INT\n  )\n)\n",
            (8, 81),
        );
        refused_at_the_engines_alone(
            "model",
            "###Relational\nDatabase a::D\n(\n  Table t\n  (\n    milestoning\n    (\n      processing(PROCESSING_IN = i, PROCESSING_OUT = o, INFINITY_DATE = %2020T)\n    )\n    i INT, o INT\n  )\n)\n",
            (8, 79),
        );
        refused_at_the_engines_alone(
            "model",
            "###Relational\nDatabase a::D\n(\n  Table t\n  (\n    milestoning\n    (\n      processing(PROCESSING_IN = i, PROCESSING_OUT = o, INFINITY_DATE = %10:10)\n    )\n    i INT, o INT\n  )\n)\n",
            (8, 73),
        );
    }

    #[test]
    fn relation_data_is_read_by_the_engines_grammar() {
        // relation data by the engine's grammar: a path's words, a reparsed assertion's place, a table cut by the end
        refused_at_the_engines_alone(
            "model",
            "function a::f(): String[1]\n{\n  ''\n}\n{\n  a::DB:\n      Relation\n      #{\n        99 default.T:\n            id\n            1;\n      }#;\n  t1 | f() => Relation\n      #{\n        id\n        1;\n      }#;\n}\n",
            (9, 9),
        );
        refused_at_the_engines_alone(
            "model",
            "function a::f(): String[1]\n{\n  ''\n}\n{\n  t1 | f() => Relation #{ name; 12 }#;\n}\n",
            (6, 34),
        );
        // a table the end cuts, refused at every dialect, at the engine's at its place
        model_refused_at_the_engines(
            "function a::f(): String[1]\n{\n  ''\n}\n{\n  t1 | f() => Relation\n      #{\n        name\n        x\n        y\n      }#;\n}\n",
            (9, 9),
        );
        refused_at_the_engines_alone(
            "model",
            "###Data\nData a::D\n{\n  Relation\n  #{\n    default.F1.5x:\n      ID\n      1;\n  }#\n}\n",
            (6, 15),
        );
    }

    #[test]
    fn embedded_and_element_text_is_read_by_the_engines_grammar() {
        // embedded and element text: a ServiceStore data's end, a data space's test data and executable, a service test's values
        refused_at_the_engines_alone(
            "model",
            "###Data\nData a::D\n{\n  ServiceStore\n  #{\n    [\n    ]\n    x\n  }#\n}\n",
            (8, 5),
        );
        refused_at_the_engines_alone(
            "model",
            "###DataSpace\nDataSpace a::DS\n{\n  executionContexts:\n  [\n    {\n      name: 'c';\n      mapping: a::M;\n      defaultRuntime: a::R;\n      testData:\n        DataspaceTestData #{ |a::b }#;\n    }\n  ];\n  defaultExecutionContext: 'c';\n}\n",
            (11, 9),
        );
        refused_at_the_engines_alone(
            "model",
            "###DataSpace\nDataSpace a::DS\n{\n  executionContexts:\n  [\n    {\n      name: 'c';\n      mapping: a::M;\n      defaultRuntime: a::R;\n    }\n  ];\n  defaultExecutionContext: 'c';\n  executables:\n  [\n    {\n      id: 1;\n      title: 't';\n      executable: a::f():T x;\n    }\n  ];\n}\n",
            (18, 28),
        );
        refused_at_the_engines_alone(
            "model",
            "###Service\nService a::S\n{\n   pattern: '/a';\n   documentation: '';\n   autoActivateUpdates: true;\n   execution: Single\n   {\n      query: |a::C.all();\n      mapping: a::M;\n      runtime: a::R;\n   }\n   test: Single\n   {\n      data: 'x';\n      asserts:\n      [\n         { [st([1])], |'e' }\n      ];\n   }\n}\n",
            (18, 15),
        );
    }

    #[test]
    fn mapping_default_and_column_expressions_are_the_engines() {
        // mapping, default value and column spec expressions
        refused_at_the_engines_alone(
            "model",
            "###Mapping\nMapping a::M\n(\n  a::T: Pure\n  {\n    ~src a::S\n    nested[2]: $src\n  }\n)\n",
            (7, 12),
        );
        refused_at_the_engines_alone(
            "model",
            "###Mapping\nMapping a::M\n(\n  a::C: XStore\n  {\n    p: a::b\n  }\n)\n",
            (6, 12),
        );
        refused_at_the_engines_alone(
            "model",
            "Class a::C\n{\n  p: a::T[*] = [^a::B(), a::f()];\n}\n",
            (3, 32),
        );
        refused_at_the_engines_alone("spec", "$t->extend(~new: x||$x.ied:y|1)", (1, 27));
        refused_at_the_engines_alone("spec", "$t->groupBy(~[a], ~[new: x||$x.ied])", (1, 27));
    }

    #[test]
    fn an_empty_block_is_refused_at_the_engines_dialect() {
        // ruling 21 of 2026-11-10 (register S368): refused in lite's words; ruling 24 (2026-11-10, built 2026-11-11): at
        // the place an expression was expected, the end of the input -- one past its last character, as `1 +` is
        for (t, place) in [
            ("", (1, 1)),
            ("   ", (1, 4)),
            ("\n", (2, 1)),
            ("  \n\t ", (2, 3)),
            ("// c\n", (2, 1)),
            ("/* c */", (1, 8)),
        ] {
            let e = block(&u(t), Dialect::LegendEngine).unwrap_err();
            assert_eq!(
                (e.class.as_str(), e.position, e.words().lossy()),
                (
                    PARSE_EXCEPTION,
                    Some(place),
                    "expected expression, got end of input".to_owned()
                ),
                "{t:?}"
            );
            assert_eq!(
                e.message.unwrap().lossy(),
                format!("[{}:{}] expected expression, got end of input", place.0, place.1),
                "{t:?}"
            );
            assert_eq!(
                spec(&u(&format!("{t}1 +")), Dialect::LegendEngine)
                    .unwrap_err()
                    .position
                    .map(|(l, _)| l),
                Some(place.0),
                "{t:?}: the line `1 +`'s missing operand stands on"
            );
            for d in [Dialect::LegendLite, Dialect::LegendPlatform] {
                assert!(block(&u(t), d).unwrap().is_empty(), "{t:?} at {d:?}");
            }
        }
    }

    #[test]
    fn a_refusal_in_flight_passes_the_recoveries_and_a_door_places_it() {
        let flying = JavaError::refused_at_section(JStr::from("m"), (3, 4));
        assert!(
            !flying.is_parse_exception(),
            "Java's catch (ParseException e) must not take it"
        );
        let token = JavaError::parse(JStr::from("m"), 3, 4).as_refusal();
        assert!(!token.is_parse_exception());
        assert_eq!(token.clone().placed().message.unwrap().lossy(), "[3:4] m");
        assert_eq!(flying.clone().placed().position, Some((3, 4)));
        let unknown = JavaError::refused_at_section(JStr::from("m"), (0, 0));
        assert_eq!(
            unknown.placed().position,
            Some((1, 1)),
            "a door with no section and no token: the text's start"
        );
    }
}

#[cfg(test)]
mod multi_error_tests {
    //! The user's ruling 23 of 2026-11-10, built 2026-11-11 (docs/rust-port/PHASE2.md section 6): a model text's errors
    //! after the first, read on at element and section boundaries only, sorted by place; the first the door's own
    //! answer, byte for byte; an element that leaves its brackets open, or a header's refusal, reported alone.
    use super::*;
    use crate::ustr::u;

    /// The door's answer and the errors after it, each as `(line, column, words)`.
    fn errors(text: &str, dialect: Dialect) -> Vec<(i32, i32, String)> {
        let units = u(text);
        let (answer, rest) = pmcd_with_errors(&units, dialect, &[]);
        let first = answer.parsed.expect_err(text);
        assert_eq!(
            Some(&first),
            pmcd(&units, dialect, &[]).parsed.as_ref().err(),
            "the first is the door's answer"
        );
        std::iter::once(first)
            .chain(rest)
            .map(|e| {
                let (line, column) = e.position.unwrap();
                (line, column, e.words().lossy())
            })
            .collect()
    }

    fn at(line: i32, column: i32, words: &str) -> (i32, i32, String) {
        (line, column, words.to_owned())
    }

    const MISSING_SEMI: &str = "expected SEMI_COLON but found BRACE_CLOSE ('}')";
    const MISSING_COLON: &str = "expected COLON but found VALID_STRING ('Integer')";

    #[test]
    fn several_elements_of_several_sections_each_answer_their_error_in_order() {
        let text = "Class a::A\n{\n  x: String[1]\n}\n\nClass a::B\n{\n  y: Integer[1];\n}\n\nClass a::C\n{\n  z Integer[1];\n}\n\n\
                    function a::f(): String[1]\n{\n  'a' +\n}\n\nEnum a::E\n{\n  X, Y\n}\n\n\
                    ###Relational\nDatabase a::D\n(\n  Table t (id INT PRIMARY KEY,, name VARCHAR(20))\n)\n\n\
                    ###Pure\nClass a::Z { q Integer[1]; }\n";
        let expected = vec![
            at(4, 1, MISSING_SEMI),
            at(13, 5, MISSING_COLON),
            at(18, 8, "expected expression, got end of input"),
            at(29, 31, "expected identifier, got COMMA"),
            at(33, 16, MISSING_COLON),
        ];
        assert_eq!(errors(text, Dialect::LegendLite), expected);
        assert_eq!(errors(text, Dialect::LegendPlatform), expected);
        // the engine's dialect reads the same elements, each refused in its own words
        assert_eq!(errors(text, Dialect::LegendEngine).len(), expected.len());
    }

    #[test]
    fn an_element_that_leaves_its_brackets_open_is_reported_alone() {
        let open = "Class a::A\n{\n  x: String[1]\n\nClass a::B\n{\n  y: Integer[1];\n}\n\nClass a::C\n{\n  z Integer[1];\n}\n";
        assert_eq!(
            errors(open, Dialect::LegendLite),
            vec![at(5, 1, "expected SEMI_COLON but found CLASS ('Class')")]
        );
        // one closer too many
        let stray = "Class a::A { x: String[1]; } }\nClass a::B { y Integer[1]; }\n";
        assert_eq!(
            errors(stray, Dialect::LegendLite),
            vec![at(1, 30, "Unexpected token '}'")]
        );
        // a later element that leaves its brackets open is the last one read
        let later = "Class a::A { x String[1]; }\nClass a::B { y Integer[1];\nClass a::C { z Integer[1]; }\n";
        assert_eq!(
            errors(later, Dialect::LegendLite),
            vec![
                at(1, 16, "expected COLON but found VALID_STRING ('String')"),
                at(2, 16, MISSING_COLON)
            ]
        );
    }

    #[test]
    fn a_header_s_refusal_is_reported_alone() {
        let text = "###Pure x\nClass a::A { x: String[1] }\nClass a::B { y Integer[1]; }\n";
        assert_eq!(
            errors(text, Dialect::LegendLite),
            vec![at(1, 9, "Unexpected token 'x'")]
        );
    }

    #[test]
    fn a_section_with_no_elements_to_go_on_to_is_one_error_and_the_next_section_is_read() {
        let text = "Class a::A { x String[1]; }\n###Diagram\nDiagram a::D { foo }\n###Foo\nwhat\n###Pure\nClass a::B { y Integer[1]; }\n";
        assert_eq!(
            errors(text, Dialect::LegendLite),
            vec![
                at(1, 16, "expected COLON but found VALID_STRING ('String')"),
                at(3, 19, "unknown diagram view 'foo'"),
                at(4, 4, "'Foo' is not a known section parser"),
                at(7, 16, MISSING_COLON),
            ]
        );
    }

    #[test]
    fn an_error_placed_before_the_first_is_not_listed() {
        // at the engine's dialect a refusal of its walk (a graph-fetch argument) stands behind a later syntax error of
        // its section, which is the door's answer (ruling 16): the walk's refusal, placed before it, is not listed
        let text =
            "function f::g():Any[*]{#{a::B{c(f())}}#}\nClass a::C { x Integer[1]; }\nClass a::D { y Integer[1]; }\n";
        assert_eq!(
            errors(text, Dialect::LegendEngine),
            vec![at(2, 16, MISSING_COLON), at(3, 16, MISSING_COLON)]
        );
    }

    #[test]
    fn one_error_lists_none_after_it() {
        let (answer, rest) = pmcd_with_errors(&u("Class a::A { x String[1]; }\n"), Dialect::LegendLite, &[]);
        assert!(answer.parsed.is_err());
        assert!(rest.is_empty());
        let (answer, rest) = pmcd_with_errors(&u("Class a::A { x: String[1]; }\n"), Dialect::LegendLite, &[]);
        assert!(answer.parsed.is_ok());
        assert!(rest.is_empty());
    }
}
