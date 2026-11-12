// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! legend-lite's platform, ported from core's `com.legend.platform` (the layer `platform` of
//! tools/deps/core-layers.txt; docs/rust-port/PHASE3.md, slice 1b, 2026-11-12): every function declaration the platform
//! knows, one per id, and what executes each.
//!
//! - [`core_fn`]: `CoreFn`, the language forms (the closed vocabulary of core structural constructs).
//! - [`feature`]: `Feature`, the engine's execution feature flags.
//! - [`walled_bodies`]: `WalledBodies`, the Pure bodies the platform carries and refuses to compile.
//! - [`declaration_table`]: `DeclarationTable`, every declaration by id.
//! - [`implementation`], [`registrations`], [`implementation_table`]: what executes each declaration, from what the
//!   platform registers.
//! - [`platform_pure`]: `PlatformPure`, the platform's own Pure (the system metamodel's bodies).
//!
//! Java's semantics exactly: a map Java keeps in a hash map is a hash map here, never iterated where its order would be
//! seen; a `LinkedHashMap` is a [`lite_model::linked::LinkedMap`]; a `Set.of` is answered in declaration order. An
//! exception a caller sees is a `Result` with Java's class and message ([`lite_error::CompileError`] for a
//! `com.legend.error` one, [`implementation::IllegalArgument`] for the JDK's); a class-initialization failure is a panic
//! with Java's message. Held to Java's answers by `tests/platform_tables.rs` over
//! rust/oracles/builtin/platform-tables.tsv.

#![forbid(unsafe_code)]

/// Java's `a + b + ...` over strings, as a [`lite_model::JStr`] of the parts' UTF-16 units (lite-model's `cat!`, which
/// names the protocol crate by its path, written over lite-model's own re-export).
macro_rules! cat {
    ($($part:expr),* $(,)?) => {{
        let mut out: Vec<u16> = Vec::new();
        $( lite_model::text::Units::push_to(&$part, &mut out); )*
        lite_model::JStr(out)
    }};
}

pub mod core_fn;
pub mod declaration_table;
pub mod feature;
pub mod implementation;
pub mod implementation_table;
pub mod platform_pure;
pub mod registrations;
pub mod walled_bodies;

pub use core_fn::CoreFn;
pub use declaration_table::DeclarationTable;
pub use feature::Feature;
pub use implementation::{Form, IllegalArgument, Implementation, Intrinsic, Position, Reason, Refused};
pub use implementation_table::ImplementationTable;
pub use platform_pure::AdoptError;
pub use registrations::Registrations;
pub use walled_bodies::{Kind, Wall};

/// The unit tests' declarations: the catalog's natives by id and a text's functions (the Java tests' `Engine.model`,
/// the engine's dialect). Each runs on the parse stack: the catalog parses its declarations on first read.
#[cfg(test)]
pub(crate) mod testing {
    use std::sync::Arc;

    use lite_front::doors::Dialect;
    use lite_model::{Function, FunctionId, JStr, NativeFunctionDefinition};

    pub const UPPER_ID: &str = "meta::pure::functions::string::toUpper_String_1__String_1_";
    pub const LOWER_ID: &str = "meta::pure::functions::string::toLower_String_1__String_1_";

    /// `Pure.nativeFunctionById(id)`, required.
    pub fn catalog(id: &str) -> Arc<NativeFunctionDefinition> {
        lite_builtin::pure::catalog()
            .native_function_by_id(&JStr::from(id))
            .unwrap_or_else(|| panic!("{id}"))
            .def()
            .clone()
    }

    pub fn native(n: &Arc<NativeFunctionDefinition>) -> Function {
        Function::NativeFunctionDefinition(n.clone())
    }

    pub fn id(qualified: &str) -> FunctionId {
        FunctionId::new(JStr::from(qualified))
    }

    pub fn fqn(n: &Arc<NativeFunctionDefinition>) -> JStr {
        n.qualified_name.clone()
    }

    /// The functions of `source` at the engine's dialect.
    pub fn declare(source: &str) -> Vec<Function> {
        let units: Vec<u16> = source.encode_utf16().collect();
        lite_builtin::door::parse(&units, Dialect::LegendEngine)
            .unwrap_or_else(|e| panic!("{source}: {e:?}"))
            .elements
            .iter()
            .filter_map(|e| e.as_function())
            .collect()
    }

    /// `work` on the parser's deep stack.
    pub fn on_stack<T: Send>(work: impl FnOnce() -> T + Send) -> T {
        lite_front::doors::on_parse_stack(work)
    }
}
