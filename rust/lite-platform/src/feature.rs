// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! `Feature`: the engine's execution feature flags -- `meta::pure::executionPlan::features::Feature`
//! (executionPlanFeature.pure), one closed enum, mirrored member for member. A flag is a compile-time fact of one query,
//! consumed in the lowering only: it selects an emission for a call, never a different typed tree.
//!
//! Ported from core's `com.legend.platform.Feature` (2026-11-12, docs/rust-port/PHASE3.md slice 1b).

/// One flag (Java's constant names).
#[allow(non_camel_case_types)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Feature {
    /// The engine transforms enum values in SQL (on) or in Java after the query (off); execution only ever transforms
    /// in SQL. Consumer: the plan-text channel.
    PUSH_DOWN_ENUM_TRANSFORM,
    /// Variant-typed inputs to externalize / internalize. No consumer: loud when set.
    VARIANT_TYPE_AS_INPUT,
    /// Pre-#4900 equality: plain `=` where the platform emits IS NOT DISTINCT FROM. Consumer: the lowerer's equality
    /// rewrite.
    LEGACY_SQL_NULL_UNSAFE_EQUALS,
    /// Pure's 0-based, end-exclusive substring indexes become SQL's 1-based start and length. Consumer: the scalar
    /// rules' flagged rule for the `substring` overloads.
    CORRECT_SQL_SUBSTRING_INDEXING,
    /// Implicit sorts drop the NULLS clause. No claim: loud when set.
    USE_DB_NATIVE_IMPLICIT_NULL_ORDERING,
}

impl Feature {
    /// Java's `values()`.
    pub const VALUES: [Feature; 5] = [
        Feature::PUSH_DOWN_ENUM_TRANSFORM,
        Feature::VARIANT_TYPE_AS_INPUT,
        Feature::LEGACY_SQL_NULL_UNSAFE_EQUALS,
        Feature::CORRECT_SQL_SUBSTRING_INDEXING,
        Feature::USE_DB_NATIVE_IMPLICIT_NULL_ORDERING,
    ];

    /// The engine enum's fully qualified name.
    pub const FQN: &str = "meta::pure::executionPlan::features::Feature";

    /// Java's `name()`.
    pub fn name(self) -> &'static str {
        match self {
            Feature::PUSH_DOWN_ENUM_TRANSFORM => "PUSH_DOWN_ENUM_TRANSFORM",
            Feature::VARIANT_TYPE_AS_INPUT => "VARIANT_TYPE_AS_INPUT",
            Feature::LEGACY_SQL_NULL_UNSAFE_EQUALS => "LEGACY_SQL_NULL_UNSAFE_EQUALS",
            Feature::CORRECT_SQL_SUBSTRING_INDEXING => "CORRECT_SQL_SUBSTRING_INDEXING",
            Feature::USE_DB_NATIVE_IMPLICIT_NULL_ORDERING => "USE_DB_NATIVE_IMPLICIT_NULL_ORDERING",
        }
    }
}
