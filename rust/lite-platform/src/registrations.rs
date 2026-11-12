// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! `Registrations`: everything the platform registers about what executes a function -- the implementation table's one
//! input beside the declarations. A value, so a table can be built over any registrations (a test's hand-written ones)
//! and every path of the builder is exercisable; the lowering, which owns the registries, assembles the platform's own.
//!
//! Ported from core's `com.legend.platform.Registrations` (2026-11-12, docs/rust-port/PHASE3.md slice 1b). Java's
//! record copies each map into a `LinkedHashMap` and each set into a `LinkedHashSet`, so their order is the order
//! handed in: a map here is a [`LinkedMap`] (one value per key, insertion order), and a set a list read as a set by the
//! builder (each member once, at its first place). Java's family keys (`Class<? extends NativeFn.Member>`) are
//! [`Family`]s.

use std::sync::Arc;

use lite_builtin::native_fn::Family;
use lite_model::linked::LinkedMap;
use lite_model::{ClassMember, FunctionId, JStr, NativeFunctionDefinition};

use crate::core_fn::CoreFn;
use crate::feature::Feature;
use crate::implementation::Position;
use crate::walled_bodies::Wall;

/// What the platform registers about what executes a function.
#[derive(Clone, Debug, Default)]
pub struct Registrations {
    /// The catalog natives: each lowering key is one of their ids, matched to its definition whole, never parsed.
    pub catalog: Vec<Arc<NativeFunctionDefinition>>,
    /// Per lowering position, the catalog ids registered there (each a set).
    pub lowering_keys: LinkedMap<Position, Vec<FunctionId>>,
    /// Per execution feature flag, the ids whose scalar rule the flag replaces (each a set).
    pub feature_overrides: LinkedMap<Feature, Vec<FunctionId>>,
    /// Per implementer family, its members' overloads (each a list).
    pub families: LinkedMap<Family, Vec<Arc<NativeFunctionDefinition>>>,
    /// Per language form, the exact FQNs whose overloads it owns (each a set).
    pub forms: LinkedMap<CoreFn, Vec<JStr>>,
    /// Natives refused by decision, FQN to reason.
    pub walled_natives: LinkedMap<JStr, JStr>,
    /// Bodies refused by decision, FQN to the wall.
    pub walled_bodies: LinkedMap<JStr, Wall>,
    /// Programs whose value is never needed, by FQN (a set).
    pub subsumed: Vec<JStr>,
    /// Per implementer family, the class members (derived properties) it implements, matched to lifted declarations by
    /// their provenance (each a set).
    pub members: LinkedMap<Family, Vec<ClassMember>>,
    /// The ids the platform implements in its own Pure (a set).
    pub platform_pure: Vec<FunctionId>,
    /// Versions at the platform's own names whose upstream body runs here, id to why.
    pub upstream_bodies: LinkedMap<FunctionId, JStr>,
    /// Versions at the platform's own names refused by decision, id to the wall.
    pub refused_versions: LinkedMap<FunctionId, Wall>,
}

impl Registrations {
    /// No registrations but the catalog's (the tests' `none`).
    pub fn of_catalog(catalog: Vec<Arc<NativeFunctionDefinition>>) -> Registrations {
        Registrations {
            catalog,
            ..Registrations::default()
        }
    }
}
