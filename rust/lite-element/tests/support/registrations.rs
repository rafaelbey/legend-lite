// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! The platform's registrations F is handed (ElementAnswers' `REGISTRATIONS`): the lowering's assembly but for its own
//! (no lowering keys, no feature overrides, no members: slice I's), the walled natives and each form's FQNs sorted --
//! rust/oracles/builtin's PlatformTables' set, copied from rust/lite-platform/tests/tables/platform.rs's
//! `platform_registrations` (2026-11-15). F reads them for one answer, `implementations()`.

use std::sync::Arc;

use lite_builtin::native_fn::Family;
use lite_builtin::pure::{WALLED_NATIVES, catalog};
use lite_builtin::subsumed::Subsumed;
use lite_model::linked::LinkedMap;
use lite_model::{JStr, NativeFunctionDefinition};
use lite_platform::{CoreFn, Registrations, platform_pure, walled_bodies};

/// `ElementAnswers.registrations()` (ElementAnswers.java:161).
pub fn platform_registrations() -> Registrations {
    let mut families: LinkedMap<Family, Vec<Arc<NativeFunctionDefinition>>> = LinkedMap::new();
    for f in Family::VALUES {
        for (_, _, overloads) in f.members() {
            families
                .get_or_insert_with(f, Vec::new)
                .extend(overloads.iter().map(|n| n.def().clone()));
        }
    }
    let mut forms: LinkedMap<CoreFn, Vec<JStr>> = LinkedMap::new();
    for f in CoreFn::VALUES {
        if !f.owned_fqns().is_empty() {
            let mut owned: Vec<&str> = f.owned_fqns().to_vec();
            owned.sort();
            forms.put(f, owned.into_iter().map(JStr::from).collect());
        }
    }
    let mut walled: Vec<(&str, &str)> = WALLED_NATIVES.to_vec();
    walled.sort();
    Registrations {
        catalog: catalog().all().to_vec(),
        families,
        forms,
        walled_natives: LinkedMap::of(walled.into_iter().map(|(f, r)| (JStr::from(f), JStr::from(r)))),
        walled_bodies: walled_bodies::reasons().clone(),
        subsumed: Subsumed::VALUES.iter().map(|s| JStr::from(s.fqn())).collect(),
        platform_pure: platform_pure::ids(),
        upstream_bodies: platform_pure::upstream_bodies().clone(),
        refused_versions: platform_pure::refused_versions().clone(),
        ..Registrations::default()
    }
}
