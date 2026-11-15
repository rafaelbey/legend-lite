// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! `MilestoningFacts` (MilestoningFacts.java at ad86fa895, ported line for line): a class's TEMPORAL stereotype as a
//! fold over its lineage (the kernel's walk): a class is temporal when it or any ancestor carries a milestoning
//! stereotype. Its one caller is JoinChainEmission (`plainClassViewCond`, JCE:898).

use lite_model::JStr;
use lite_names::{MilestoningStrategy, ModelIndex};

use crate::error::Thrown;

/// `MilestoningFacts.isTemporal` (:20). The lineage walk is loud for a class it cannot resolve (lite-names'
/// `Knowledge::lineage`).
pub fn is_temporal(class_fqn: &JStr, model: &ModelIndex) -> Result<bool, Thrown> {
    for cd in model.knowledge().lineage(class_fqn)? {
        for st in cd.stereotypes.iter() {
            if MilestoningStrategy::of_stereotype_or_null(&st.profile_name, &st.stereotype_name).is_some() {
                return Ok(true);
            }
        }
    }
    Ok(false)
}
