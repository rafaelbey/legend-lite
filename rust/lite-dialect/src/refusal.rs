// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! A Java exception as a value (docs/rust-port/TRIAGE.md A2): every `throw` of the renderers is a `Refusal`, carrying
//! the class Java threw and its exact message -- the dump records `{"throws": <class>, "message": <text>}`, and the
//! JVM shim rethrows a `DialectCapability` refusal as one (docs/rust-port/PHASE1.md §3).

use lite_sql::SqlExpr;

/// A refusal to render: the Java exception the renderer would have thrown.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refusal {
    /// The Java class's binary name (`com.legend.sql.dialect.DialectCapability`).
    pub class: &'static str,
    /// The exception's message, as Java built it.
    pub message: String,
}

impl Refusal {
    /// A renderer capability wall (Java's `DialectCapability`, an `IllegalStateException`).
    pub const DIALECT_CAPABILITY: &'static str = "com.legend.sql.dialect.DialectCapability";
    pub const ILLEGAL_STATE: &'static str = "java.lang.IllegalStateException";
    pub const ILLEGAL_ARGUMENT: &'static str = "java.lang.IllegalArgumentException";
    pub const NULL_POINTER: &'static str = "java.lang.NullPointerException";
    pub const INDEX_OUT_OF_BOUNDS: &'static str = "java.lang.IndexOutOfBoundsException";
    pub const CLASS_CAST: &'static str = "java.lang.ClassCastException";
    pub const NUMBER_FORMAT: &'static str = "java.lang.NumberFormatException";

    /// Java's `new DialectCapability(message)`.
    pub fn capability(message: impl Into<String>) -> Refusal {
        Refusal { class: Refusal::DIALECT_CAPABILITY, message: message.into() }
    }

    /// Java's `new IllegalStateException(message)`.
    pub fn illegal_state(message: impl Into<String>) -> Refusal {
        Refusal { class: Refusal::ILLEGAL_STATE, message: message.into() }
    }

    /// Java's `new IllegalArgumentException(message)`.
    pub fn illegal_argument(message: impl Into<String>) -> Refusal {
        Refusal { class: Refusal::ILLEGAL_ARGUMENT, message: message.into() }
    }

    /// Java's `Objects.requireNonNull(x, message)` failing.
    pub fn null_pointer(message: impl Into<String>) -> Refusal {
        Refusal { class: Refusal::NULL_POINTER, message: message.into() }
    }

    /// A cast Java's code makes without a check (`(SqlExpr.StringLit) e`) failing: the message names the classes
    /// by their simple names only (the JVM's spells the binary names and the module).
    pub fn class_cast(got: &str, wanted: &str) -> Refusal {
        Refusal {
            class: Refusal::CLASS_CAST,
            message: format!(
                "class com.legend.sql.SqlExpr${got} cannot be cast to class com.legend.sql.SqlExpr${wanted}"
            ),
        }
    }

    /// Whether Java's `catch (DialectCapability e)` catches it.
    pub fn is_capability(&self) -> bool {
        self.class == Refusal::DIALECT_CAPABILITY
    }
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.class, self.message)
    }
}

impl std::error::Error for Refusal {}

/// `a.get(i)`: Java's list read, refused past the end as `ArrayList.get` (and `Objects.checkIndex`) word it.
pub fn arg(a: &[SqlExpr], i: usize) -> Result<&SqlExpr, Refusal> {
    a.get(i).ok_or_else(|| Refusal {
        class: Refusal::INDEX_OUT_OF_BOUNDS,
        message: format!("Index {i} out of bounds for length {}", a.len()),
    })
}

/// `a.subList(from, a.size())`, refused as `subList` words a start past the end.
pub fn tail(a: &[SqlExpr], from: usize) -> Result<&[SqlExpr], Refusal> {
    if from > a.len() {
        return Err(Refusal {
            class: Refusal::INDEX_OUT_OF_BOUNDS,
            message: format!("fromIndex = {from}, toIndex = {}, size = {}", a.len(), a.len()),
        });
    }
    Ok(&a[from..])
}
