// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! legend-lite's user-facing error taxonomy, ported from core's `com.legend.error` (the layer `error` of
//! tools/deps/core-layers.txt; docs/rust-port/PHASE3.md, slice 1b, 2026-11-12): is a failure the user's model or
//! query ([`CompileError`], with the [`Phase`] that rejected it), a feature not built ([`ErrorKind::NotImplemented`]),
//! or our bug (a panic, Java's `IllegalStateException`)?
//!
//! One record for the package's eleven classes: the class is its [`ErrorKind`], each kind's fixed phase as Java's
//! constructors fix it. Messages are UTF-16 units, as Java's are. The phases are the code's (`PARSE` .. `EXECUTE`,
//! `RENDER` among them); AGENTS.md's prose says there is no `RENDER`, a difference listed for the owner, not resolved
//! here.

#![forbid(unsafe_code)]

use std::fmt;

/// `LegendCompileException.Phase`, in declaration order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Phase {
    Parse,
    Resolve,
    Normalize,
    Model,
    Type,
    Mapping,
    Lower,
    Render,
    Execute,
}

impl Phase {
    /// Java's `values()`.
    pub const VALUES: [Phase; 9] = [
        Phase::Parse,
        Phase::Resolve,
        Phase::Normalize,
        Phase::Model,
        Phase::Type,
        Phase::Mapping,
        Phase::Lower,
        Phase::Render,
        Phase::Execute,
    ];

    /// Java's `name()`.
    pub fn name(self) -> &'static str {
        match self {
            Phase::Parse => "PARSE",
            Phase::Resolve => "RESOLVE",
            Phase::Normalize => "NORMALIZE",
            Phase::Model => "MODEL",
            Phase::Type => "TYPE",
            Phase::Mapping => "MAPPING",
            Phase::Lower => "LOWER",
            Phase::Render => "RENDER",
            Phase::Execute => "EXECUTE",
        }
    }
}

/// The package's classes, by what each is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ErrorKind {
    /// `ModelException`: a semantic error in the user's model (phase NORMALIZE or MODEL, as the thrower says).
    Model,
    /// `MappingResolutionException` (MAPPING).
    MappingResolution,
    /// `ResolutionException` (RESOLVE).
    Resolution,
    /// `TypeInferenceException` (TYPE).
    TypeInference,
    /// `SchemaInvariantException`, a `TypeInferenceException` (TYPE).
    SchemaInvariant,
    /// `IslandSyntaxException` (PARSE).
    IslandSyntax,
    /// `NotImplementedException`: the input may be fine; the feature is not built. No phase.
    NotImplemented,
    /// `WalledBodyException`, a `NotImplementedException`. No phase.
    WalledBody,
    /// `AssertFailed`: a test body's assertion failed (no phase).
    AssertFailed,
    /// `DataError`: the data was wrong at execution (no phase).
    DataError,
}

impl ErrorKind {
    /// The Java class's binary name.
    pub fn java_class(self) -> &'static str {
        match self {
            ErrorKind::Model => "com.legend.error.ModelException",
            ErrorKind::MappingResolution => "com.legend.error.MappingResolutionException",
            ErrorKind::Resolution => "com.legend.error.ResolutionException",
            ErrorKind::TypeInference => "com.legend.error.TypeInferenceException",
            ErrorKind::SchemaInvariant => "com.legend.error.SchemaInvariantException",
            ErrorKind::IslandSyntax => "com.legend.error.IslandSyntaxException",
            ErrorKind::NotImplemented => "com.legend.error.NotImplementedException",
            ErrorKind::WalledBody => "com.legend.error.WalledBodyException",
            ErrorKind::AssertFailed => "com.legend.error.AssertFailed",
            ErrorKind::DataError => "com.legend.error.DataError",
        }
    }

    /// Whether the class is a `LegendCompileException` (the user's input at fault, with a phase).
    pub fn is_compile_error(self) -> bool {
        !matches!(
            self,
            ErrorKind::NotImplemented | ErrorKind::WalledBody | ErrorKind::AssertFailed | ErrorKind::DataError
        )
    }
}

/// An error of the package: its class, its phase (a `LegendCompileException`'s), its message (`None` for Java's null),
/// the FQN of the element it is about when the thrower knows it, and an `AssertFailed`'s unjudged reason.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompileError {
    pub kind: ErrorKind,
    pub phase: Option<Phase>,
    pub message: Option<Vec<u16>>,
    pub element: Option<Vec<u16>>,
    pub unjudged_reason: Option<Vec<u16>>,
}

impl CompileError {
    /// `new ModelException(phase, message)` / `new ModelException(phase, message, elementFqn)`.
    pub fn model(phase: Phase, message: Vec<u16>, element: Option<Vec<u16>>) -> CompileError {
        CompileError {
            kind: ErrorKind::Model,
            phase: Some(phase),
            message: Some(message),
            element,
            unjudged_reason: None,
        }
    }

    /// `new MappingResolutionException(message[, elementFqn])` (MAPPING).
    pub fn mapping_resolution(message: Option<Vec<u16>>, element: Option<Vec<u16>>) -> CompileError {
        CompileError {
            kind: ErrorKind::MappingResolution,
            phase: Some(Phase::Mapping),
            message,
            element,
            unjudged_reason: None,
        }
    }

    /// `new ResolutionException(message)` (RESOLVE).
    pub fn resolution(message: Vec<u16>) -> CompileError {
        CompileError::of(ErrorKind::Resolution, Some(Phase::Resolve), message)
    }

    /// `new TypeInferenceException(message)` (TYPE).
    pub fn type_inference(message: Vec<u16>) -> CompileError {
        CompileError::of(ErrorKind::TypeInference, Some(Phase::Type), message)
    }

    /// `new SchemaInvariantException(message)` (TYPE).
    pub fn schema_invariant(message: Vec<u16>) -> CompileError {
        CompileError::of(ErrorKind::SchemaInvariant, Some(Phase::Type), message)
    }

    /// `new IslandSyntaxException(message)` (PARSE).
    pub fn island_syntax(message: Vec<u16>) -> CompileError {
        CompileError::of(ErrorKind::IslandSyntax, Some(Phase::Parse), message)
    }

    /// `new NotImplementedException(message)`.
    pub fn not_implemented(message: Vec<u16>) -> CompileError {
        CompileError::of(ErrorKind::NotImplemented, None, message)
    }

    /// `new WalledBodyException(message)`.
    pub fn walled_body(message: Vec<u16>) -> CompileError {
        CompileError::of(ErrorKind::WalledBody, None, message)
    }

    /// `new AssertFailed(message)`.
    pub fn assert_failed(message: Vec<u16>) -> CompileError {
        CompileError::of(ErrorKind::AssertFailed, None, message)
    }

    /// `AssertFailed.unjudged(reason, message)`.
    pub fn assert_unjudged(reason: Vec<u16>, message: Vec<u16>) -> CompileError {
        CompileError {
            unjudged_reason: Some(reason),
            ..CompileError::assert_failed(message)
        }
    }

    /// `new DataError(message)`.
    pub fn data_error(message: Vec<u16>) -> CompileError {
        CompileError::of(ErrorKind::DataError, None, message)
    }

    fn of(kind: ErrorKind, phase: Option<Phase>, message: Vec<u16>) -> CompileError {
        CompileError {
            kind,
            phase,
            message: Some(message),
            element: None,
            unjudged_reason: None,
        }
    }

    /// The message as Rust text (a lone surrogate as U+FFFD): for display, never for output.
    pub fn message_lossy(&self) -> String {
        self.message
            .as_ref()
            .map_or_else(|| "null".to_owned(), |m| String::from_utf16_lossy(m))
    }
}

impl fmt::Display for CompileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.kind.java_class(), self.message_lossy())
    }
}

impl std::error::Error for CompileError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_phases_are_the_codes_in_declaration_order() {
        let names: Vec<&str> = Phase::VALUES.iter().map(|p| p.name()).collect();
        assert_eq!(
            names,
            [
                "PARSE",
                "RESOLVE",
                "NORMALIZE",
                "MODEL",
                "TYPE",
                "MAPPING",
                "LOWER",
                "RENDER",
                "EXECUTE"
            ]
        );
    }

    #[test]
    fn each_kind_has_its_classes_phase() {
        assert_eq!(CompileError::resolution(vec![]).phase, Some(Phase::Resolve));
        assert_eq!(CompileError::schema_invariant(vec![]).phase, Some(Phase::Type));
        assert_eq!(CompileError::island_syntax(vec![]).phase, Some(Phase::Parse));
        assert_eq!(CompileError::mapping_resolution(None, None).phase, Some(Phase::Mapping));
        assert_eq!(CompileError::not_implemented(vec![]).phase, None);
        assert!(!ErrorKind::WalledBody.is_compile_error());
        assert!(ErrorKind::Model.is_compile_error());
        let a = CompileError::assert_unjudged(vec![u16::from(b'r')], vec![u16::from(b'm')]);
        assert_eq!(a.kind, ErrorKind::AssertFailed);
        assert_eq!(a.unjudged_reason, Some(vec![u16::from(b'r')]));
    }
}
