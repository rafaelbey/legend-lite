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
    pub const UNSUPPORTED_OPERATION: &'static str = "java.lang.UnsupportedOperationException";
    pub const NOT_IMPLEMENTED: &'static str = "com.legend.error.NotImplementedException";

    /// The seam's kinds (`SEAM.md`, "Refusals"): each Java class the JVM rethrows by kind, by its exact class -- a
    /// subclass not listed (`StringIndexOutOfBoundsException`) is `INTERNAL` like any other.
    pub const KINDS: &'static [(&'static str, &'static str)] = &[
        (Refusal::DIALECT_CAPABILITY, "DIALECT_CAPABILITY"),
        (Refusal::ILLEGAL_STATE, "ILLEGAL_STATE"),
        (Refusal::ILLEGAL_ARGUMENT, "ILLEGAL_ARGUMENT"),
        (Refusal::UNSUPPORTED_OPERATION, "UNSUPPORTED_OPERATION"),
        (Refusal::NOT_IMPLEMENTED, "NOT_IMPLEMENTED"),
        (Refusal::NULL_POINTER, "NULL_POINTER"),
        (Refusal::CLASS_CAST, "CLASS_CAST"),
        (Refusal::INDEX_OUT_OF_BOUNDS, "INDEX_OUT_OF_BOUNDS"),
    ];

    /// The kind every other class is answered as, its class named in the message ([`Refusal::seam_message`]).
    pub const INTERNAL: &'static str = "INTERNAL";

    /// The refusal's kind at the seam: its class's, by [`Refusal::KINDS`], else `INTERNAL`.
    pub fn kind(&self) -> &'static str {
        Refusal::KINDS.iter().find(|(class, _)| *class == self.class).map_or(Refusal::INTERNAL, |(_, kind)| kind)
    }

    /// The refusal's message at the seam: Java's message, prefixed `<class>: ` when its kind is `INTERNAL` (the kind
    /// alone would not say which exception it was).
    pub fn seam_message(&self) -> String {
        if self.kind() == Refusal::INTERNAL {
            format!("{}: {}", self.class, self.message)
        } else {
            self.message.clone()
        }
    }

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
