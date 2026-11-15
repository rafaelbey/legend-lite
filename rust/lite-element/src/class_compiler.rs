// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! `ClassCompiler` (ClassCompiler.java at 18feff3fe): a parsed class to its [`TypedClass`] -- its supertypes (by
//! head FQN and as types over its own parameters), its stored properties (with the `<<equality.Key>>` flag and the
//! default), its derived properties (their signatures and the FQN of the function realizing each), its constraints
//! (their levels and realizing FQNs). Types classify through the [`TypeClassifier`]; reference safety ran before
//! (`model_integrity`, eager and total). Bodies are G's.
//!
//! Java's `ClassCompiler` holds the classifier and the function compiler; its `compile` reads only the classifier, so
//! here it is a function of the classifier and the arena the class's types go in (the model's, for its cell).
//! [`realized_fqn`] is THE one realizing-FQN rule, `ModelIntegrity`'s too (MI:428 duplicates CC:105 in Java; census
//! §2: one shared). CC:86 is ported as Java has it: any enforcement level but `Warn` (case-insensitive), none
//! included, is ERROR (census ASK 12: a register row is the slice's step 6).

use lite_base::Alloc;
use lite_front::records as p;
use lite_model::{ClassDefinition, JStr};
use lite_names::Thrown;

use crate::type_classifier::TypeClassifier;
use crate::typed::TypedClass;

/// `compile(cd)` (:35): the typed class of `cd`, its types in `arena`.
pub fn compile<'a, A: Alloc>(
    classifier: &TypeClassifier<'_>,
    arena: &'a A,
    cd: &ClassDefinition,
) -> Result<TypedClass<'a>, Thrown> {
    unimplemented!("lite-element skeleton: ClassCompiler.compile")
}

/// `realizedFqn(r, liftedFqn)` (:105; MI:428): the function a behaviour hat is realized by -- the bound FQN of a
/// `Realization.Ref` (Door 4), else the lifted `<owner>$...$<name>` FQN Phase E synthesized for the sugar form.
pub fn realized_fqn(r: &p::Realization, lifted: JStr) -> JStr {
    match r {
        p::Realization::RealizationRef(rf) => rf.function_fqn.clone(),
        p::Realization::RealizationInline(_) => lifted,
    }
}
