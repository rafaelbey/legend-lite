// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! What the type family refuses, as Java throws it: the exception's class and its message, word for word (the
//! oracle holds both), so a refusal is the same refusal on both sides (AGENTS.md invariant 4: fail, never default).

use std::fmt;

/// The Java exception a refusal stands for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Thrown {
    /// a constructor's argument out of range
    IllegalArgument,
    /// an operation over a value it does not apply to
    IllegalState,
    /// `Integer.parseInt` past an int
    NumberFormat,
}

impl Thrown {
    /// The Java class's simple name.
    pub fn simple_name(self) -> &'static str {
        match self {
            Thrown::IllegalArgument => "IllegalArgumentException",
            Thrown::IllegalState => "IllegalStateException",
            Thrown::NumberFormat => "NumberFormatException",
        }
    }
}

/// A refusal: Java's exception and its message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeError {
    pub thrown: Thrown,
    pub message: String,
}

impl TypeError {
    pub(crate) fn argument(message: String) -> TypeError {
        TypeError {
            thrown: Thrown::IllegalArgument,
            message,
        }
    }

    pub(crate) fn state(message: String) -> TypeError {
        TypeError {
            thrown: Thrown::IllegalState,
            message,
        }
    }

    /// `Integer.parseInt(text)`'s refusal of a digit string past an int.
    pub(crate) fn number(text: &str) -> TypeError {
        TypeError {
            thrown: Thrown::NumberFormat,
            message: format!("For input string: \"{text}\""),
        }
    }
}

impl fmt::Display for TypeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.thrown.simple_name(), self.message)
    }
}

impl std::error::Error for TypeError {}
