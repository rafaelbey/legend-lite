// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! `ResolvedNames` (2026-11-12): the full names a resolved call names -- the one reading of the resolver's output every
//! front-door pass shares. An exact FQN names itself; a bare call carries its record (`AppliedFunction.referents`), the
//! program's functions and the platform's, worked out once by the resolver (or, for a built call, by the typer's
//! `Calls`). An empty record on a bare spelling is a call nothing declares.

use lite_builtin::pure::{NativeId, catalog};
use lite_model::JStr;
use lite_model::protocol::AppliedFunction;
use lite_platform::CoreFn;

fn qualified(s: &JStr) -> bool {
    lite_model::text::contains(&s.0, "::")
}

/// `referents(af)`.
pub fn referents(af: &AppliedFunction) -> Vec<JStr> {
    if qualified(&af.function) {
        vec![af.function.clone()]
    } else {
        af.referents.to_vec()
    }
}

/// `declaredNatives(af)`: the catalog natives among the call's referents; a bare referent is `Pure.nativeFunctionsAt`'s
/// refusal (Java's `IllegalArgumentException`, its message), which the resolver's full-name records never meet.
pub fn declared_natives(af: &AppliedFunction) -> Result<Vec<NativeId>, String> {
    let mut out = Vec::new();
    for fqn in referents(af) {
        out.extend_from_slice(catalog().native_functions_at(&fqn)?);
    }
    Ok(out)
}

/// `names(af, fqn)`: whether the call names `fqn` (exactly, or among its referents).
pub fn names(af: &AppliedFunction, fqn: &str) -> bool {
    referents(af).iter().any(|r| r.eq_str(fqn))
}

/// `form(af)`: the language form a call is -- the form owning a full name the call resolves to; a full name no form
/// owns read as written; a bare name with no referent at all by its spelling; a bare name whose referents no form owns
/// is no form.
pub fn form(af: &AppliedFunction) -> Option<CoreFn> {
    let refs = referents(af);
    for fqn in &refs {
        if let Some(owner) = CoreFn::owning(&fqn.lossy()) {
            return Some(owner);
        }
    }
    if refs.is_empty() || refs.contains(&af.function) {
        CoreFn::of(&af.function.lossy())
    } else {
        None
    }
}
