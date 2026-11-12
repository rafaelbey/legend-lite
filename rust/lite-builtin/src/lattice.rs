// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! The platform's class lattice (2026-11-12, the owner's ruling 22 of docs/rust-port/PHASE3_SHIM_DESIGN.md §10:
//! subtyping held to the live Java in slice 1b, with the model): `ModelContext.isSubtype` on the boot model -- the
//! catalog's native classes, the prelude's classes and the system metamodel's, every supertype spelled as an FQN -- by
//! lite-model's walk over their declarations ([`lite_model::subtyping`]). Held to Java's answer for every ordered pair
//! of the boot layer's classes (and `Nil` and two absent names) by `tests/subtype_live.rs`, against the live Java
//! (rust/oracles/builtin/SubtypeProbe.java), not a frozen file: the ruling keeps it live until F's slice.
//!
//! Measured 2026-11-12: of the 509 classes of that universe, the 366 whose walk meets only FQN-spelled supertypes agree
//! with Java on every pair; the other 143 meet a supertype the prelude spells by its simple name under its section's
//! imports (`EngineRuntime extends Runtime`), which Java resolves before F compiles the class -- name resolution, the
//! names slice's (step D), so the declared walk cannot answer them yet, and does not guess.

use std::collections::HashMap;
use std::sync::{Arc, LazyLock};

use lite_front::jtypes::JStr;
use lite_model::{ClassDefinition, PackageableElement};

/// The boot layer's class declarations by FQN: an FQN's first declaration in the order system metamodel, prelude,
/// catalog.
pub struct Lattice {
    classes: HashMap<JStr, Arc<ClassDefinition>>,
}

static BOOT: LazyLock<Lattice> = LazyLock::new(|| {
    let mut classes: HashMap<JStr, Arc<ClassDefinition>> = HashMap::new();
    let elements = crate::system_metamodel::elements()
        .iter()
        .chain(crate::prelude::prelude().elements());
    for e in elements {
        if let PackageableElement::ClassDefinition(c) = e {
            classes.entry(c.qualified_name.clone()).or_insert_with(|| c.clone());
        }
    }
    for c in crate::pure::catalog().native_classes() {
        classes.entry(c.qualified_name.clone()).or_insert_with(|| c.clone());
    }
    Lattice { classes }
});

/// The boot layer's lattice (built on first read).
pub fn boot() -> &'static Lattice {
    &BOOT
}

impl Lattice {
    /// The boot layer's declaration of `fqn`.
    pub fn find(&self, fqn: &JStr) -> Option<&ClassDefinition> {
        self.classes.get(fqn).map(|c| &**c)
    }

    /// Whether the boot layer declares a class at `fqn`.
    pub fn declares(&self, fqn: &JStr) -> bool {
        self.classes.contains_key(fqn)
    }

    /// `ModelContext.isSubtype(child, parent)` on the boot model; `Err` with the failure Java would throw.
    pub fn is_subtype(&self, child: &JStr, parent: &JStr) -> Result<bool, &'static str> {
        lite_model::subtyping::is_subtype(child, parent, |f| self.find(f))
    }
}
