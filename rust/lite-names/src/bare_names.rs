// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! `BareNames` (2026-11-12): THE ONE RULE for a call spelled bare that the resolver's own tiers could not qualify.
//! Three tiers, in order, each a declaration the platform can point at: the engine surface (the bare names
//! legend-engine's `Handlers.java` registers, each to the FQNs the platform declares), the core import group (the
//! name under each package of [`core_imports::SEQUENCE`](crate::core_imports::SEQUENCE)), and the form's owned FQNs
//! spelled like the call. A lite-package FQN is reachable from a bare spelling only on the product surface.
//!
//! [`referents`] is process-wide memoized as Java's `REFERENTS` is (its inputs are static tables; an empty answer is
//! not kept), behind a mutex: lookup, compute outside the lock, first writer wins.

use std::collections::{HashMap, HashSet};
use std::sync::{LazyLock, Mutex};

use lite_builtin::engine_handlers::handlers;
use lite_builtin::pure::{NativeId, catalog, lite};
use lite_model::{JStr, PackageableElement, cat};
use lite_platform::CoreFn;

use crate::core_imports;

/// One tier's answer for a bare name: the tier (`ENGINE`, `CORE`, `FORM`) and an FQN it spells.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TierFqn {
    pub tier: &'static str,
    pub fqn: JStr,
}

/// `tiered(name)`: every (tier, FQN) a bare name may denote, in tier order, the lite partition applied.
pub fn tiered(name: &JStr) -> Vec<TierFqn> {
    let mut out = Vec::new();
    let n = name.lossy();
    for fqn in handlers().fqns_of(&n) {
        out.push(TierFqn {
            tier: "ENGINE",
            fqn: JStr::from(fqn.as_str()),
        });
    }
    for pkg in core_imports::SEQUENCE {
        out.push(TierFqn {
            tier: "CORE",
            fqn: cat!(*pkg, "::", name),
        });
    }
    // the form's owned declarations SPELLED LIKE THE CALL, in the order of Java's TreeSet (UTF-16 order)
    if let Some(form) = CoreFn::by_parse_name(&n).filter(|_| !has_lone_surrogate(name)) {
        let mut owned: Vec<JStr> = form.owned_fqns().iter().map(|f| JStr::from(*f)).collect();
        owned.sort();
        owned.dedup();
        for fqn in owned {
            let sep: Vec<u16> = "::".encode_utf16().collect();
            // Java's fqn.substring(fqn.lastIndexOf("::") + 2): from 1 when the name has no package
            let from = lite_model::text::last_index_of(&fqn.0, &sep).map_or(1, |i| i + 2);
            let simple = &fqn.0[from.min(fqn.0.len())..];
            if simple == &name.0[..] {
                out.push(TierFqn { tier: "FORM", fqn });
            }
        }
    }
    out.retain(|t| !(lite_model::text::starts_with(&t.fqn.0, lite::PKG) && !catalog().is_user_resolvable(&t.fqn)));
    out
}

/// `fqns(name)`: the FQNs a bare name may denote, in tier order, distinct.
pub fn fqns(name: &JStr) -> Vec<JStr> {
    let mut out: Vec<JStr> = Vec::new();
    for t in tiered(name) {
        if !out.contains(&t.fqn) {
            out.push(t.fqn);
        }
    }
    out
}

/// `catalogTiered(name)`: the catalog natives at each (tier, FQN) of [`tiered`] (a name spelled by two tiers is looked
/// up once).
pub fn catalog_tiered(name: &JStr) -> Vec<(TierFqn, Vec<NativeId>)> {
    let mut at: HashMap<JStr, Vec<NativeId>> = HashMap::new();
    let mut out = Vec::new();
    for t in tiered(name) {
        let natives = at
            .entry(t.fqn.clone())
            .or_insert_with(|| {
                catalog()
                    .native_functions_at(&t.fqn)
                    .expect("a tier's FQN is qualified")
                    .to_vec()
            })
            .clone();
        out.push((t, natives));
    }
    out
}

/// The answers by (name, arity).
type Referents = HashMap<(JStr, usize), Vec<JStr>>;

static REFERENTS: LazyLock<Mutex<Referents>> = LazyLock::new(|| Mutex::new(HashMap::new()));

/// The prelude's functions by FQN, each with the arities it declares.
static PRELUDE_ARITIES: LazyLock<HashMap<JStr, HashSet<usize>>> = LazyLock::new(|| {
    let mut at: HashMap<JStr, HashSet<usize>> = HashMap::new();
    for el in lite_builtin::prelude::prelude().elements() {
        if let Some(f) = PackageableElement::as_function(el) {
            at.entry(f.qualified_name().clone())
                .or_default()
                .insert(f.parameters().len());
        }
    }
    at
});

/// `referents(name, arity)`: THE RULE'S ANSWER FOR A CALL -- the full names a bare name may denote at `arity` (the
/// catalog natives at each tier FQN with that many parameters, or a prelude function at it with that arity), in tier
/// order, distinct.
pub fn referents(name: &JStr, arity: usize) -> Vec<JStr> {
    let key = (name.clone(), arity);
    if let Some(hit) = REFERENTS.lock().expect("referents").get(&key) {
        return hit.clone();
    }
    let mut out: Vec<JStr> = Vec::new();
    for (tier, natives) in catalog_tiered(name) {
        let mut declared = natives.iter().any(|n| n.def().parameters.len() == arity);
        if !declared {
            declared = PRELUDE_ARITIES.get(&tier.fqn).is_some_and(|a| a.contains(&arity));
        }
        if declared && !out.contains(&tier.fqn) {
            out.push(tier.fqn);
        }
    }
    if !out.is_empty() {
        REFERENTS
            .lock()
            .expect("referents")
            .entry(key)
            .or_insert_with(|| out.clone());
    }
    out
}

/// A bare name with a lone surrogate names no form (the forms' spellings are ASCII; its lossy text must not match one).
fn has_lone_surrogate(s: &JStr) -> bool {
    char::decode_utf16(s.0.iter().copied()).any(|c| c.is_err())
}
