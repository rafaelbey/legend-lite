// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! The parser's text doors (core/'s `PmcdParser.parse`, `SpecParser.parse`, `parseLambda`, `parseCodeBlock`) and
//! what they answer: the records, or the error Java raises -- its class, message and, for a `ParseException`, its
//! line and column -- which the shim rethrows as that class (docs/rust-port/PHASE2.md §3). Each door is held to the
//! Java parser's answer for every question of the front oracle (`front_parser_differential_test`), in the canonical
//! form `answer_form` writes (Java's `FrontAnswers.canonical`).

use crate::codec;
use crate::jtypes::JStr;
use crate::records::{LambdaFunction, ValueSpecification};
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
}

pub type Answer<T> = Result<T, JavaError>;

/// `SpecParser.parse(text, dialect)`: one expression.
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

/// An answer in the canonical form Java's `FrontAnswers.canonical` writes: `A` and the codec's JSON (the door's
/// declared type deciding the tags: a `ValueSpecification` or a list's element is tagged, a `LambdaFunction` is not);
/// or `E`, the class, the line and column (empty but for a `ParseException`), the message (`null` without one), tab
/// separated. UTF-16 units, as Java's string.
pub fn answer_form(door: &str, dialect: Dialect, text: &[u16]) -> Vec<u16> {
    let written = match door {
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
        _ => Err(JavaError::of("door not ported", Some(door))),
    };
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
