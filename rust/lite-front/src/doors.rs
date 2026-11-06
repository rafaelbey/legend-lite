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
use crate::records::{LambdaFunction, PmcdParserParsed, ValueSpecification};
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
}

pub type Answer<T> = Result<T, JavaError>;

/// The stack a door's parse needs: the parser recurses through every nested expression, as Java's does (whose
/// answers hold no `StackOverflowError`), and a Rust frame is larger than a JVM's (docs/rust-port/PHASE2.md §3,
/// "Recursion depth"). The doors parse on the caller's thread -- the shim's runs on its own thread of 256 MiB
/// (`lite_ffi::DIALECT_STACK_BYTES`) -- and [`answer_form`] on one of this size.
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
    spec_parser::parse(text, dialect)
}

/// `SpecParser.parseLambda(text)`: a lambda at the engine's dialect, a non-lambda wrapped as a parameterless one.
pub fn lambda(text: &[u16]) -> Answer<LambdaFunction> {
    spec_parser::parse_lambda(text)
}

/// `SpecParser.parseCodeBlock(text, dialect)`: a `;`-separated block.
pub fn block(text: &[u16], dialect: Dialect) -> Answer<Vec<ValueSpecification>> {
    spec_parser::parse_code_block(text, dialect)
}

/// `PmcdParser.parse(text, dialect)`: a model text's records -- every element in section order, the section index,
/// the unclaimed sections -- with the shim's registered overlay grammars: beside the records, each overlay section
/// the walk met, whose grammar the shim calls and whose elements it splices (`model_parser::OverlaySection`).
pub fn pmcd(text: &[u16], dialect: Dialect, overlays: &[OverlayGrammar<'_>]) -> PmcdAnswer {
    crate::model_parser::parse_with_overlays(text, dialect, overlays)
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
