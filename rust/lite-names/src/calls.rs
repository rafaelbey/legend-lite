// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! THE ONE PLACE A CALL IS BUILT AFTER THE RESOLVER (2026-11-12, the names follow-up: Java's
//! `compiler/spec/Calls`, moved into the names layer by the owner's ruling of 2026-11-12 so the normalizer, the element
//! compiler and the typer reach one builder; docs/rust-port/PHASE3_CRATES.md §2). A parsed call gets its record
//! (`AppliedFunction.referents`: every full name it can mean) from the name resolver; a call the typer, a desugar, the
//! normalizer or validation builds is born with its record here, so no reader after the resolver works a name out from
//! a spelling. Java's `ArchitectureTest.callsAreBuiltOnlyByTheBuilder` keeps every other constructor call out of those
//! packages; rust/lite-guards' `calls_are_built_only_by_the_builder` holds the Rust sources the same way (the parser's
//! crate, lite-front, and the resolver, which records, excepted).
//!
//! Ported line for line: each builder makes the record Java's makes, component for component (`List.copyOf` of the
//! parameters and the record, a null record empty).

use std::sync::Arc;

use lite_model::JStr;
use lite_model::protocol as p;
use lite_platform::core_fn::CoreFn;

use crate::resolver;

fn record(referents: Vec<JStr>) -> Arc<[JStr]> {
    referents.into()
}

fn call(function: JStr, parameters: Vec<p::ValueSpecification>, referents: Vec<JStr>) -> p::AppliedFunction {
    p::AppliedFunction {
        function,
        parameters: parameters.into(),
        referents: record(referents),
        pos: None,
        property_call: false,
        grouped: false,
        infix: false,
        island: false,
        f_control: None,
        owner_class: None,
    }
}

/// `platform(name, args)`: a platform function by its bare name -- the record is the bare-name rule's names at this
/// arity, the list the resolver records for the same call parsed. Empty when the rule names nothing (a form with no
/// signature at this arity); a spelling that may be a full name gets no record and reads as itself.
pub fn platform(name: &JStr, args: Vec<p::ValueSpecification>) -> p::AppliedFunction {
    let named = resolver::built_call_names(name, args.len());
    call(named.function, args, named.referents)
}

/// `form(form, args)`: a language form spelled by its parse name (`asc`, `rename`): the record a bare call of that name
/// gets.
pub fn form(form: CoreFn, args: Vec<p::ValueSpecification>) -> p::AppliedFunction {
    platform(&JStr::from(form.parse_name()), args)
}

/// `exact(fqn, args)`: a function by its full name (the caller's contract: the spelling holds `::`); the name is the
/// one referent, so the record is empty.
pub fn exact(fqn: &JStr, args: Vec<p::ValueSpecification>) -> p::AppliedFunction {
    call(fqn.clone(), args, Vec::new())
}

/// `carrier(spelling, args)`: a parser carrier (`new` and its kin), a spelling the language has no function for; its
/// record is empty by definition.
pub fn carrier(spelling: &JStr, args: Vec<p::ValueSpecification>) -> p::AppliedFunction {
    call(spelling.clone(), args, Vec::new())
}

/// The engine's n-ary carrier: the operands as one collection.
fn operand_run(operands: Vec<p::ValueSpecification>) -> Vec<p::ValueSpecification> {
    vec![p::ValueSpecification::PureCollection(Arc::new(p::PureCollection {
        values: operands.into(),
        pos: None,
    }))]
}

/// `infixRun(op, operands)`: an operator run the compiler synthesizes (`a + b (+ ...)`), the `infix` marker set, with
/// the record a parsed run gets (the operator's one-collection overload: the rule's names at arity one).
pub fn infix_run(op: &JStr, operands: Vec<p::ValueSpecification>) -> p::AppliedFunction {
    let named = resolver::built_call_names(op, 1);
    let mut out = call(named.function, operand_run(operands), named.referents);
    out.infix = true;
    out
}

/// `renamed(original, name, args)`: a call rewritten under another spelling with the original's position and written
/// forms kept; the record is the rule's for the new name at the new arity (the original's names are dropped).
pub fn renamed(original: &p::AppliedFunction, name: &JStr, args: Vec<p::ValueSpecification>) -> p::AppliedFunction {
    let named = resolver::built_call_names(name, args.len());
    p::AppliedFunction {
        function: named.function,
        parameters: args.into(),
        referents: record(named.referents),
        pos: original.pos.clone(),
        property_call: original.property_call,
        grouped: original.grouped,
        infix: original.infix,
        island: original.island,
        f_control: original.f_control.clone(),
        owner_class: original.owner_class.clone(),
    }
}

/// `among(name, referents, args)`: a call whose names a registry row gives (the engine's dynafunction table), recorded
/// as the resolver records an import-resolved call; an empty row is deliberate (the normalizer's second pass resolves
/// it in the user's namespace).
pub fn among(name: &JStr, referents: &[JStr], args: Vec<p::ValueSpecification>) -> p::AppliedFunction {
    call(name.clone(), args, referents.to_vec())
}

/// `infixRunAmong(op, referents, operands)`: [`among`] as an operator run ([`infix_run`]).
pub fn infix_run_among(op: &JStr, referents: &[JStr], operands: Vec<p::ValueSpecification>) -> p::AppliedFunction {
    let mut out = call(op.clone(), operand_run(operands), referents.to_vec());
    out.infix = true;
    out
}

/// `like(original, args)`: a call rebuilt with other arguments and everything else of the original kept -- its record
/// when the arity is the original's; at another arity the original's names joined by the rule's at the new arity (the
/// original's spelling kept when it has a record, the rule's spelling when it has none).
pub fn like(original: &p::AppliedFunction, args: Vec<p::ValueSpecification>) -> p::AppliedFunction {
    if args.len() == original.parameters.len() {
        return original.with_parameters(args.into());
    }
    let named = resolver::built_call_names(&original.function, args.len());
    let function = if original.referents.is_empty() {
        named.function
    } else {
        original.function.clone()
    };
    let mut joined: Option<Vec<JStr>> = None;
    for fqn in named.referents {
        if !original.referents.contains(&fqn) && joined.as_ref().is_none_or(|j| !j.contains(&fqn)) {
            joined.get_or_insert_with(|| original.referents.to_vec()).push(fqn);
        }
    }
    p::AppliedFunction {
        function,
        parameters: args.into(),
        referents: joined.map_or_else(|| original.referents.clone(), Arc::from),
        pos: original.pos.clone(),
        property_call: original.property_call,
        grouped: original.grouped,
        infix: original.infix,
        island: original.island,
        f_control: original.f_control.clone(),
        owner_class: original.owner_class.clone(),
    }
}
