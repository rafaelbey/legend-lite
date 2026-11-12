// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! What the names layer throws, as Java throws it (2026-11-12): a `com.legend.error` class with its phase, message and
//! element ([`CompileError`], lite-error's), or a JDK exception by its class and message (an `IllegalStateException` the
//! index raises on a model it cannot hold, an `IllegalArgumentException` a synthesized name refuses).

use lite_error::{CompileError, Phase};
use lite_model::JStr;

pub const ILLEGAL_STATE: &str = "java.lang.IllegalStateException";
pub const ILLEGAL_ARGUMENT: &str = "java.lang.IllegalArgumentException";

/// A throw of the names layer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Thrown {
    /// A `LegendCompileException` (`ResolutionException`, `ModelException`).
    Compile(CompileError),
    /// A JDK exception: its class's binary name and its message.
    Java { class: &'static str, message: Option<JStr> },
}

impl Thrown {
    /// `new ResolutionException(message)`.
    pub fn resolution(message: JStr) -> Thrown {
        Thrown::Compile(CompileError::resolution(message.0))
    }

    /// `new ModelException(phase, message[, element])`.
    pub fn model(phase: Phase, message: JStr, element: Option<&JStr>) -> Thrown {
        Thrown::Compile(CompileError::model(phase, message.0, element.map(|e| e.0.clone())))
    }

    /// `new IllegalStateException(message)`.
    pub fn illegal_state(message: JStr) -> Thrown {
        Thrown::Java {
            class: ILLEGAL_STATE,
            message: Some(message),
        }
    }

    /// `new IllegalArgumentException(message)`.
    pub fn illegal_argument(message: JStr) -> Thrown {
        Thrown::Java {
            class: ILLEGAL_ARGUMENT,
            message: Some(message),
        }
    }

    /// The thrown class's binary name.
    pub fn java_class(&self) -> &'static str {
        match self {
            Thrown::Compile(e) => e.kind.java_class(),
            Thrown::Java { class, .. } => class,
        }
    }

    /// `getMessage()`.
    pub fn message(&self) -> Option<JStr> {
        match self {
            Thrown::Compile(e) => e.message.clone().map(JStr),
            Thrown::Java { message, .. } => message.clone(),
        }
    }

    /// Whether this is a `ResolutionException` (what the model resolution catches per element).
    pub fn is_resolution(&self) -> bool {
        matches!(self, Thrown::Compile(e) if e.kind == lite_error::ErrorKind::Resolution)
    }

    /// Whether this is a `ModelException` (what the association adoption catches per association).
    pub fn is_model(&self) -> bool {
        matches!(self, Thrown::Compile(e) if e.kind == lite_error::ErrorKind::Model)
    }

    /// `String.valueOf(e.getMessage()).split("\n")[0]`: a wall's line.
    pub fn first_line(&self) -> JStr {
        match self.message() {
            None => JStr::from("null"),
            Some(m) => first_line(&m),
        }
    }
}

/// `s.split("\n")[0]`: the units before the first line feed (Java's `split` drops trailing empty strings, so an empty
/// text is its own first part).
pub fn first_line(s: &JStr) -> JStr {
    match s.0.iter().position(|&u| u == u16::from(b'\n')) {
        Some(i) => JStr::from(&s.0[..i]),
        None => s.clone(),
    }
}
