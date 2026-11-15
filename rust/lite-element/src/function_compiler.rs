// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! `FunctionCompiler` (FunctionCompiler.java at 18feff3fe): F's function-element compiler -- THE native-plus-model
//! overload merge (`functions_at`: the catalog's natives first, then each model version whose id is no native's, in
//! ingest order; the candidate order overload resolution reads), signature compilation (`compile`, `compile_all`),
//! and the reference-safety primitives the class and mapping checks use (`require_function`: existence;
//! `require_shape`: a structural shape). Bodies are G's.
//!
//! A view, made per call (`Model::functions`), over the index and its classifier.
//!
//! **The swallow** (FC:77's `catch (RuntimeException)`, census ASK 7 (a), ruling 30) is a typed `Result`: an
//! overload whose signature is refused (`Err`) is dropped and its siblings compile; when every overload is refused,
//! the first refusal is returned; a panic is never caught, it propagates (Java's `RuntimeException` there is a
//! refusal: a `ModelException` or a bound's raw `IllegalArgumentException`, both `Err` here). DON'T PORT (EF_CENSUS
//! §3): FC:58-60, the `DecisionProbe` hook (a measurement probe, a no-op unless `LL_SHADOW` is set).

use lite_base::Alloc;
use lite_front::records as p;
use lite_model::{Function, JStr};
use lite_names::{ModelIndex, Thrown};

use crate::type_classifier::TypeClassifier;
use crate::typed::TypedFunction;

/// `FunctionCompiler` (FunctionCompiler.java:23).
#[derive(Clone, Copy)]
pub struct FunctionCompiler<'i> {
    model: &'i ModelIndex,
    classifier: TypeClassifier<'i>,
}

impl<'i> FunctionCompiler<'i> {
    /// `new FunctionCompiler(model, classifier)` (:28).
    pub fn new(model: &'i ModelIndex, classifier: TypeClassifier<'i>) -> FunctionCompiler<'i> {
        FunctionCompiler { model, classifier }
    }

    /// The classifier the compiler classifies with.
    pub fn classifier(&self) -> TypeClassifier<'i> {
        self.classifier
    }

    /// `functionsAt(fqn)` (:34): every declaration at `fqn`, the catalog's natives first, then each model function
    /// whose `FunctionId` no native has (one with a native's id is that native); a bare name (no `::`) is no
    /// declaration: none.
    pub fn functions_at(&self, fqn: &JStr) -> Vec<Function> {
        unimplemented!("lite-element skeleton: FunctionCompiler.functionsAt")
    }

    /// `exists(fqn)` (:65): a symbol lookup, no compilation.
    pub fn exists(&self, fqn: &JStr) -> bool {
        !self.functions_at(fqn).is_empty()
    }

    /// `compileAll(fqn)` (:70): every overload at `fqn` compiled to its signature, in `functions_at`'s order, its
    /// types in `arena`; a refused overload dropped, all refused the first refusal (the module's docs: the swallow).
    pub fn compile_all<'a, A: Alloc>(&self, arena: &'a A, fqn: &JStr) -> Result<Vec<TypedFunction<'a>>, Thrown> {
        unimplemented!("lite-element skeleton: FunctionCompiler.compileAll")
    }

    /// `compile(f)` (:102): one definition's signature -- its parameters and return classified over its type
    /// parameters, its body when it has one (a native has none), its definition and its id (`FunctionId::of`).
    pub fn compile<'a, A: Alloc>(&self, arena: &'a A, f: &Function) -> Result<TypedFunction<'a>, Thrown> {
        unimplemented!("lite-element skeleton: FunctionCompiler.compile")
    }

    /// `requireFunction(fqn, site)` (:131): the function a structural element binds to exists, or "<site> binds to
    /// unknown function '<fqn>'" (a `ModelException`, MODEL).
    pub fn require_function(&self, fqn: &JStr, site: &JStr) -> Result<(), Thrown> {
        unimplemented!("lite-element skeleton: FunctionCompiler.requireFunction")
    }

    /// `requireShape(fqn, shape, site, expected)` (:143): some overload at `fqn` has `shape`, or "<site> must be
    /// realized by a function <expected>" (a `ModelException`, MODEL). Existence is checked before.
    pub fn require_shape(
        &self,
        fqn: &JStr,
        shape: impl Fn(&Function) -> bool,
        site: &JStr,
        expected: &str,
    ) -> Result<(), Thrown> {
        unimplemented!("lite-element skeleton: FunctionCompiler.requireShape")
    }

    /// `returnsBooleanOne(f)` (:153): returns exactly `Boolean[1]` (the return type the `Boolean` name, the
    /// multiplicity `Concrete.PURE_ONE`).
    pub fn returns_boolean_one(f: &Function) -> bool {
        unimplemented!("lite-element skeleton: FunctionCompiler.returnsBooleanOne")
    }

    /// `returnsClassMany(f)` (:160): returns a class by name, `[*]` (a kind check; which class is G's subtyping).
    pub fn returns_class_many(&self, f: &Function) -> bool {
        unimplemented!("lite-element skeleton: FunctionCompiler.returnsClassMany")
    }
}

/// `named(t, fqn)` (:165): the type expression is the name `fqn`.
fn named(t: &p::TypeExpression, fqn: &str) -> bool {
    unimplemented!("lite-element skeleton: FunctionCompiler.named")
}
