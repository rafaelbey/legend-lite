// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! What the model's converters throw, as Java throws it. The converters' own refusals are named
//! (`FromProtocol.UnsupportedConnectionShape`, `MappingFromProtocol.UnsupportedMappingShape` and its
//! `MissingDatabase`), since the door catches them by class; anything else a converter or a record's constructor
//! throws is a Java exception by its class and message ([`JavaError`], rust/lite-front's), which goes out unchanged.

use lite_front::doors::JavaError;
use lite_front::jtypes::JStr;

use crate::records::ParsedModelPosition;
use std::sync::Arc;

pub const ILLEGAL_STATE: &str = "java.lang.IllegalStateException";
pub const ILLEGAL_ARGUMENT: &str = "java.lang.IllegalArgumentException";
pub const UNSUPPORTED_OPERATION: &str = "java.lang.UnsupportedOperationException";
pub const NULL_POINTER: &str = "java.lang.NullPointerException";

/// A converter's throw.
#[derive(Clone, Debug, PartialEq)]
pub enum Thrown {
    /// `FromProtocol.UnsupportedConnectionShape(reason)`.
    Connection(JStr),
    /// `MappingFromProtocol.UnsupportedMappingShape(reason)`.
    Mapping(JStr),
    /// `MappingFromProtocol.MissingDatabase(reason)`, an `UnsupportedMappingShape` the relational arm catches.
    MissingDatabase(JStr),
    /// Any other exception, as Java raises it.
    Java(JavaError),
}

impl From<JavaError> for Thrown {
    fn from(e: JavaError) -> Thrown {
        Thrown::Java(e)
    }
}

/// `new IllegalStateException(message)`.
pub fn illegal_state(message: JStr) -> Thrown {
    Thrown::Java(JavaError::java(ILLEGAL_STATE, message))
}

/// `new IllegalArgumentException(message)`.
pub fn illegal_argument(message: JStr) -> Thrown {
    Thrown::Java(JavaError::java(ILLEGAL_ARGUMENT, message))
}

/// `new UnsupportedOperationException(message)`.
pub fn unsupported_operation(message: JStr) -> Thrown {
    Thrown::Java(JavaError::java(UNSUPPORTED_OPERATION, message))
}

/// `Objects.requireNonNull(null)` (no message) or `Objects.requireNonNull(null, message)`.
pub fn null_pointer(message: Option<&str>) -> Thrown {
    Thrown::Java(JavaError::of(NULL_POINTER, message))
}

/// `Objects.requireNonNull(v, message)`: the value, or the NullPointerException.
pub fn require<T>(v: Option<T>, message: Option<&str>) -> Result<T, Thrown> {
    v.ok_or_else(|| null_pointer(message))
}

/// `ModelFromProtocol.Unrepresentable`: a shape the compiler's model cannot represent, named -- the element's path,
/// the reason, and where the element starts when its record says.
#[derive(Clone, Debug, PartialEq)]
pub struct Unrepresentable {
    pub path: JStr,
    pub reason: JStr,
    pub at: Option<Arc<ParsedModelPosition>>,
}

/// What `ModelFromProtocol` throws: an [`Unrepresentable`], which the text's door answers as a parse error, or any
/// other exception, which goes out as it is.
#[derive(Clone, Debug, PartialEq)]
pub enum ModelError {
    Unrepresentable(Unrepresentable),
    Java(JavaError),
}

impl From<JavaError> for ModelError {
    fn from(e: JavaError) -> ModelError {
        ModelError::Java(e)
    }
}
