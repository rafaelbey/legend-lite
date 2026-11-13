// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! A temporal class's milestoning strategy (2026-11-12, the names follow-up: Java's
//! `compiler/element/MilestoningStrategy`, moved into the names layer by the owner's ruling of 2026-11-12 -- E's
//! `MilestoningFacts` reaches it first; G, H and `Temporal` read it after): the engine's `temporal.*` stereotypes, typed.
//! `None` means "not temporal".

use lite_model::JStr;
use lite_types::platform;

/// `MilestoningStrategy`: its three constants, in Java's order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MilestoningStrategy {
    Business,
    Processing,
    Bitemporal,
}

/// `MilestoningStrategy.Dimension`: one axis of a (possibly bitemporal) strategy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Dimension {
    Business,
    Processing,
}

impl Dimension {
    /// Every constant, in Java's order (`values()`).
    pub const ALL: [Dimension; 2] = [Dimension::Business, Dimension::Processing];

    /// The constant's Java `name()`.
    pub fn name(self) -> &'static str {
        match self {
            Dimension::Business => "BUSINESS",
            Dimension::Processing => "PROCESSING",
        }
    }
}

impl MilestoningStrategy {
    /// Every constant, in Java's order (`values()`).
    pub const ALL: [MilestoningStrategy; 3] = [
        MilestoningStrategy::Business,
        MilestoningStrategy::Processing,
        MilestoningStrategy::Bitemporal,
    ];

    /// The constant's Java `name()`.
    pub fn name(self) -> &'static str {
        match self {
            MilestoningStrategy::Business => "BUSINESS",
            MilestoningStrategy::Processing => "PROCESSING",
            MilestoningStrategy::Bitemporal => "BITEMPORAL",
        }
    }

    /// `ofStereotypeOrNull(profileName, stereotypeName)`: the stereotype-to-strategy funnel, the only place the surface
    /// spellings live. `None` for a non-temporal profile, and for an unrecognized `temporal.*` stereotype name (Java's
    /// "historical set-membership tolerance", ported as it is).
    pub fn of_stereotype_or_null(profile_name: &JStr, stereotype_name: &JStr) -> Option<MilestoningStrategy> {
        // PlatformTypes.isProfile(profileName, TEMPORAL_PROFILE): the resolved name equals the profile's FQN
        if !profile_name.eq_str(platform::TEMPORAL_PROFILE) {
            return None;
        }
        if stereotype_name.eq_str("businesstemporal") {
            Some(MilestoningStrategy::Business)
        } else if stereotype_name.eq_str("processingtemporal") {
            Some(MilestoningStrategy::Processing)
        } else if stereotype_name.eq_str("bitemporal") {
            Some(MilestoningStrategy::Bitemporal)
        } else {
            None
        }
    }

    /// `has(d)`: whether this strategy carries the given temporal axis.
    pub fn has(self, d: Dimension) -> bool {
        match self {
            MilestoningStrategy::Bitemporal => true,
            MilestoningStrategy::Business => d == Dimension::Business,
            MilestoningStrategy::Processing => d == Dimension::Processing,
        }
    }

    /// `of(d)`: the single-axis strategy of one dimension (a bitemporal class's halves).
    pub fn of(d: Dimension) -> MilestoningStrategy {
        match d {
            Dimension::Business => MilestoningStrategy::Business,
            Dimension::Processing => MilestoningStrategy::Processing,
        }
    }
}
