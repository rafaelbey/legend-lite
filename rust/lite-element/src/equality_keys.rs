// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! `EqualityKeys.isKeyStereotype` (EqualityKeys.java:74 at 18feff3fe): the one spelling of the `<<equality.Key>>`
//! test, by which the class compiler stamps `PropertyStored::equality_key`. The rest of `EqualityKeys` (`fqnOf`,
//! `resolve`, `declaresKeys`, the key trees) is I's and testplan's (EF_CENSUS §3, ruling 27), ported with them.

use lite_model::StereotypeApplication;

/// `isKeyStereotype(st)` (:74): the stereotype is `Key` of the platform's `meta::pure::profiles::equality`.
pub fn is_key_stereotype(st: &StereotypeApplication) -> bool {
    unimplemented!("lite-element skeleton: EqualityKeys.isKeyStereotype")
}
