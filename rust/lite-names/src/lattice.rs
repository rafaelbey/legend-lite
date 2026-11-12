// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! The boot model's class lattice, its names resolved (2026-11-12, the names slice; moved here from lite-builtin, where
//! slice 1b made it under the owner's ruling 22 of docs/rust-port/PHASE3_SHIM_DESIGN.md §10: subtyping held to the live
//! Java, not frozen). The boot layer is `Compiler.boot()`'s first step -- the system metamodel's elements and the
//! prelude's, the prelude's keeping their section imports, resolved by [`resolver::resolve`](crate::resolver::resolve)
//! -- and its classes indexed ([`ModelIndex`]); a class is looked up as F looks it up (`KnowledgeLayer.classDef`: the
//! catalog's native classes first, then the model's), and `ModelContext.isSubtype` (F's relation) is lite-model's one
//! walk in F's shape ([`lite_model::subtyping::is_subtype`]).
//!
//! Slice 1b's lattice read the declarations unresolved, so the 143 classes whose walk meets a supertype the prelude
//! spells by its simple name (`EngineRuntime extends Runtime`) waited for this crate; resolved, every class of the
//! universe answers as the live Java's (`tests/subtype_live.rs`, run by hand while Java exists).
//!
//! `Compiler.boot()` itself (the platform's own Pure merged in, Phase E over the boot layer) is the planner's and waits
//! for it; the merge touches functions alone and the normalizer no class's supertypes, so the lattice is the resolved
//! declarations'.

use std::sync::LazyLock;

use lite_model::{ImportScope, JStr, ParsedModel, linked};

use crate::error::Thrown;
use crate::model_index::ModelIndex;
use crate::resolver;

/// The boot layer's index (its resolved elements) and the lookup F reads it through.
pub struct Lattice {
    index: ModelIndex,
}

/// The boot layer's `ParsedModel` as `Compiler.boot()` builds it before resolution: the system metamodel's elements,
/// then the prelude's, the prelude's offsets, section imports and sources; no imports of its own.
pub fn boot_model() -> Result<ParsedModel, Thrown> {
    let pre = lite_builtin::prelude::prelude().parsed_model();
    lite_builtin::system_metamodel::require_no_system_element_redefined(pre).map_err(Thrown::Compile)?;
    let mut elements = lite_builtin::system_metamodel::elements().to_vec();
    elements.extend(pre.elements.iter().cloned());
    Ok(ParsedModel {
        elements: linked::list(elements),
        imports: ImportScope::empty(),
        source: None,
        element_offsets: pre.element_offsets.clone(),
        element_imports: pre.element_imports.clone(),
        element_sources: pre.element_sources.clone(),
        unclaimed_sections: linked::empty(),
        element_spans: linked::empty(),
    })
}

static BOOT: LazyLock<Result<Lattice, Thrown>> = LazyLock::new(|| {
    let resolved = resolver::resolve(&boot_model()?)?;
    Ok(Lattice {
        index: ModelIndex::from(&resolved)?,
    })
});

/// The boot layer's lattice (built on first read); `Err` with what resolving or indexing the boot layer threw.
pub fn boot() -> Result<&'static Lattice, &'static Thrown> {
    BOOT.as_ref()
}

impl Lattice {
    /// The boot layer's index.
    pub fn index(&self) -> &ModelIndex {
        &self.index
    }

    /// `ModelContext.isSubtype(child, parent)` on the boot model; `Err` with the failure Java would throw.
    pub fn is_subtype(&self, child: &JStr, parent: &JStr) -> Result<bool, &'static str> {
        let k = self.index.knowledge();
        lite_model::subtyping::is_subtype(child, parent, |f| k.class_def(f).map(|c| &**c))
    }
}
