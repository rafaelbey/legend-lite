// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! `FunctionCompiler` (FunctionCompiler.java at 18feff3fe): F's function-element compiler -- THE native-plus-model
//! overload merge (`functions_at`: the catalog's natives first, then each model version whose id is no native's, in
//! ingest order; the candidate order overload resolution reads), signature compilation (`compile`, `compile_all`),
//! and the reference-safety primitives the class and mapping checks use (`require_function`: existence;
//! `require_shape`: a structural shape). Bodies are G's.
//!
//! A view, made per call (`Model::function_compiler`), over the index and its classifier.
//!
//! **The swallow** (FC:77's `catch (RuntimeException)`, census ASK 7 (a), ruling 30) is a typed `Result`: an
//! overload whose signature is refused (`Err`) is dropped and its siblings compile; when every overload is refused,
//! the first refusal is returned; a panic is never caught, it propagates (Java's `RuntimeException` there is a
//! refusal: a `ModelException` or a bound's raw `IllegalArgumentException`, both `Err` here). DON'T PORT (EF_CENSUS
//! §3): FC:58-60, the `DecisionProbe` hook (a measurement probe, a no-op unless `LL_SHADOW` is set).
//!
//! **The classifier seam** (2026-11-15, the family fill): `compile` reaches the classifier through [`Classify`], a
//! crate-private trait the [`TypeClassifier`] implements by forwarding, so that the unit tests compile signatures
//! over a stand-in while the classifier is filled beside this file; the public methods pass the view's own classifier,
//! so the answers are the classifier's.

use std::collections::HashSet;
use std::sync::Arc;

use lite_base::Alloc;
use lite_builtin::pure::catalog;
use lite_error::Phase;
use lite_front::records as p;
use lite_model::{Function, FunctionId, JStr, cat};
use lite_names::{ModelIndex, Thrown};
use lite_types::{Primitive, Type};

use crate::type_classifier::TypeClassifier;
use crate::typed::{TypedFunction, TypedParameter};

/// `FunctionCompiler` (FunctionCompiler.java:23).
#[derive(Clone, Copy)]
pub struct FunctionCompiler<'i> {
    model: &'i ModelIndex,
    classifier: TypeClassifier<'i>,
}

/// What signature compilation asks of the classifier: `classifier.classify(te, typeParams)` (the module's docs: the
/// seam).
pub(crate) trait Classify {
    /// `TypeClassifier.classify(te, typeParams)` (TypeClassifier.java:91).
    fn classify<'a, A: Alloc>(
        &self,
        arena: &'a A,
        te: &p::TypeExpression,
        type_params: &[JStr],
    ) -> Result<Type<'a>, Thrown>;
}

impl Classify for TypeClassifier<'_> {
    fn classify<'a, A: Alloc>(
        &self,
        arena: &'a A,
        te: &p::TypeExpression,
        type_params: &[JStr],
    ) -> Result<Type<'a>, Thrown> {
        TypeClassifier::classify(self, arena, te, type_params)
    }
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
        if !lite_model::text::contains(&fqn.0, "::") {
            // A BARE name is not a declaration: a call's names are on its record and a reader looks up each recorded
            // full name here (FC:36-38).
            return Vec::new();
        }
        let natives = catalog()
            .native_functions_at(fqn)
            .unwrap_or_else(|e| unreachable!("a name with '::' is no bare name: {e}"));
        let mut all: Vec<Function> = natives
            .iter()
            .map(|n| Function::NativeFunctionDefinition(n.def().clone()))
            .collect();
        // membership only (census §9.2: FC:49); a native's id is `FunctionId.of(n)`, computed once by the catalog
        let catalog_ids: HashSet<&FunctionId> = natives.iter().map(|n| n.id()).collect();
        for def in self.model.find_function(fqn) {
            if !catalog_ids.contains(&FunctionId::of(def)) {
                all.push(def.clone());
            }
        }
        all
    }

    /// `exists(fqn)` (:65): a symbol lookup, no compilation.
    pub fn exists(&self, fqn: &JStr) -> bool {
        !self.functions_at(fqn).is_empty()
    }

    /// `compileAll(fqn)` (:70): every overload at `fqn` compiled to its signature, in `functions_at`'s order, its
    /// types in `arena`; a refused overload dropped, all refused the first refusal (the module's docs: the swallow).
    pub fn compile_all<'a, A: Alloc>(&self, arena: &'a A, fqn: &JStr) -> Result<Vec<TypedFunction<'a>>, Thrown> {
        self.compile_all_with(&self.classifier, arena, fqn)
    }

    /// `compileAll(fqn)` (:70) over `classifier` (the module's docs: the seam).
    fn compile_all_with<'a, A: Alloc, C: Classify>(
        &self,
        classifier: &C,
        arena: &'a A,
        fqn: &JStr,
    ) -> Result<Vec<TypedFunction<'a>>, Thrown> {
        let defs = self.functions_at(fqn);
        let mut typed = Vec::with_capacity(defs.len());
        let mut first: Option<Thrown> = None;
        for f in &defs {
            match compile_with(classifier, arena, f) {
                Ok(t) => typed.push(t),
                // DROP-AT-OVERLOAD (FC:78-89): a tolerant build keeps a signature-broken function in the model, but
                // candidate collection omits it; a strict build's integrity pass refuses it first
                Err(e) => {
                    if first.is_none() {
                        first = Some(e);
                    }
                }
            }
        }
        if typed.is_empty()
            && let Some(e) = first
        {
            // ALL overloads broken: surface the real reason
            return Err(e);
        }
        Ok(typed)
    }

    /// `compile(f)` (:102): one definition's signature -- its parameters and return classified over its type
    /// parameters, its body when it has one (a native has none), its definition and its id (`FunctionId::of`).
    pub fn compile<'a, A: Alloc>(&self, arena: &'a A, f: &Function) -> Result<TypedFunction<'a>, Thrown> {
        compile_with(&self.classifier, arena, f)
    }

    /// `requireFunction(fqn, site)` (:131): the function a structural element binds to exists, or "<site> binds to
    /// unknown function '<fqn>'" (a `ModelException`, MODEL).
    pub fn require_function(&self, fqn: &JStr, site: &JStr) -> Result<(), Thrown> {
        if !self.exists(fqn) {
            return Err(Thrown::model(
                Phase::Model,
                cat!(site, " binds to unknown function '", fqn, "'"),
                None,
            ));
        }
        Ok(())
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
        if !self.functions_at(fqn).iter().any(shape) {
            return Err(Thrown::model(
                Phase::Model,
                cat!(site, " must be realized by a function ", expected),
                None,
            ));
        }
        Ok(())
    }

    /// `returnsBooleanOne(f)` (:153): returns exactly `Boolean[1]` (the return type the `Boolean` name, the
    /// multiplicity `Concrete.PURE_ONE`).
    pub fn returns_boolean_one(f: &Function) -> bool {
        named(f.return_type(), Primitive::Boolean.qualified_name()) && is_concrete(f.return_multiplicity(), 1, Some(1))
    }

    /// `returnsClassMany(f)` (:160): returns a class by name, `[*]` (a kind check; which class is G's subtyping).
    pub fn returns_class_many(&self, f: &Function) -> bool {
        matches!(f.return_type(), p::TypeExpression::TypeExpressionNameRef(nr) if self.classifier.is_class_fqn(&nr.name))
            && is_concrete(f.return_multiplicity(), 0, None)
    }
}

/// `compile(f)` (:102) over `classifier` (the module's docs: the seam), in Java's order: each parameter's type then
/// its multiplicity, then the return's type, then its multiplicity (the first refusal met is the answer).
fn compile_with<'a, A: Alloc, C: Classify>(
    classifier: &C,
    arena: &'a A,
    f: &Function,
) -> Result<TypedFunction<'a>, Thrown> {
    let type_params = f.type_parameters();
    let mut params = Vec::with_capacity(f.parameters().len());
    for pd in f.parameters().iter() {
        params.push(TypedParameter {
            name: pd.name.clone(),
            r#type: classifier.classify(arena, &pd.r#type, type_params)?,
            multiplicity: TypeClassifier::multiplicity(arena, &pd.multiplicity)?,
        });
    }
    let body = match f {
        Function::FunctionDefinition(fd) => Some(fd.body.clone()),
        Function::NativeFunctionDefinition(_) => None,
    };
    let return_type = classifier.classify(arena, f.return_type(), type_params)?;
    let return_multiplicity = TypeClassifier::multiplicity(arena, f.return_multiplicity())?;
    Ok(TypedFunction {
        qualified_name: f.qualified_name().clone(),
        type_parameters: type_params.clone(),
        multiplicity_parameters: f.multiplicity_parameters().clone(),
        parameters: Arc::from(params),
        return_type,
        return_multiplicity,
        body,
        is_native: matches!(f, Function::NativeFunctionDefinition(_)),
        definition: f.clone(),
        id: FunctionId::of(f),
    })
}

/// `named(t, fqn)` (:165): the type expression is the name `fqn`.
fn named(t: &p::TypeExpression, fqn: &str) -> bool {
    matches!(t, p::TypeExpression::TypeExpressionNameRef(nr) if nr.name == *fqn)
}

/// `Multiplicity.Concrete(lower, upper).equals(m)` (the record's equality: `PURE_ONE` is `(1, 1)`, `ZERO_MANY` is
/// `(0, null)`; a multiplicity parameter is never one).
fn is_concrete(m: &p::Multiplicity, lower: i32, upper: Option<i32>) -> bool {
    matches!(m, p::Multiplicity::MultiplicityConcrete(c) if c.lower_bound == lower && c.upper_bound == upper)
}

#[cfg(test)]
pub(crate) mod tests {
    use lite_base::QueryArena;
    use lite_front::doors::Dialect;
    use lite_model::{FunctionDefinition, ParsedModel};
    use lite_types::Multiplicity;

    use super::*;

    /// A model text parsed and its names resolved (strict), as `rust/lite-normalizer/src/test_support.rs` does.
    pub(crate) fn model(text: &str) -> ParsedModel {
        let units: Vec<u16> = text.encode_utf16().collect();
        let parsed = lite_builtin::door::parse(&units, Dialect::LegendLite).expect("the model parses");
        lite_names::resolver::resolve(&parsed).expect("the model's names resolve")
    }

    /// The model's index.
    pub(crate) fn index(text: &str) -> ModelIndex {
        ModelIndex::from(&model(text)).expect("the model indexes")
    }

    pub(crate) fn j(s: &str) -> JStr {
        JStr::from(s)
    }

    /// The tests' stand-in for the classifier (the module's docs: the seam), its answers the classifier's for what
    /// the tests declare (TC:91's name arm, in `findType`'s order): a type parameter is a `TypeVar`, a platform
    /// primitive its constant, a class of the model or the catalog a `ClassType`, an enum an `EnumType`, anything
    /// else TypeClassifier's "Unknown type" refusal (TC:151's text). `?`, a primitive extension, the `Enum`
    /// metaclass, the row erasure, a generic, a function, a relation or a schema-algebra type is not the tests'.
    pub(crate) struct StandIn<'i>(pub(crate) &'i ModelIndex);

    impl Classify for StandIn<'_> {
        fn classify<'a, A: Alloc>(
            &self,
            arena: &'a A,
            te: &p::TypeExpression,
            type_params: &[JStr],
        ) -> Result<Type<'a>, Thrown> {
            let p::TypeExpression::TypeExpressionNameRef(nr) = te else {
                panic!("the stand-in classifies names only: {te:?}")
            };
            if type_params.contains(&nr.name) {
                return Ok(Type::TypeVar(arena.alloc_str(&nr.name.lossy())));
            }
            if let Some(prim) = Primitive::find_by_fqn(&nr.name.lossy()) {
                return Ok(Type::Primitive(prim));
            }
            if TypeClassifier::new(self.0).is_class_fqn(&nr.name) {
                return Ok(Type::ClassType(arena.alloc_str(&nr.name.lossy())));
            }
            if TypeClassifier::new(self.0).is_enum_fqn(&nr.name) {
                return Ok(Type::EnumType(arena.alloc_str(&nr.name.lossy())));
            }
            Err(Thrown::model(
                Phase::Model,
                cat!(
                    "Unknown type: '",
                    &nr.name,
                    "' is not a known primitive, class, or enum"
                ),
                None,
            ))
        }
    }

    const STRING: &str = "meta::pure::metamodel::type::String";
    const BOUND: &str = "upper (1) must be >= lower (2)";

    fn compiler(index: &ModelIndex) -> FunctionCompiler<'_> {
        FunctionCompiler::new(index, TypeClassifier::new(index))
    }

    fn model_function(index: &ModelIndex, fqn: &str, i: usize) -> Function {
        index.find_function(&j(fqn))[i].clone()
    }

    fn assert_model_refusal(e: &Thrown, message: &str) {
        let Thrown::Compile(ce) = e else {
            panic!("a ModelException, got {e:?}")
        };
        assert_eq!(e.java_class(), "com.legend.error.ModelException");
        assert_eq!(e.message(), Some(j(message)));
        assert_eq!(ce.phase, Some(Phase::Model));
        assert_eq!(ce.element, None);
    }

    #[test]
    fn a_bare_name_is_no_declaration() {
        let index = index("function f(x: String[1]): String[1] { $x }\n");
        let fc = compiler(&index);
        assert!(fc.functions_at(&j("f")).is_empty());
        assert!(fc.functions_at(&j("toUpper")).is_empty());
        assert!(!fc.exists(&j("f")));
    }

    #[test]
    fn the_catalogs_natives_come_first_in_declaration_order_then_the_models_in_ingest_order() {
        let fqn = "meta::pure::functions::string::toUpper";
        let natives = catalog().native_functions_at(&j(fqn)).expect("an FQN");
        assert!(!natives.is_empty());
        // two model versions at a native's FQN, neither with a native's id: both join, after the natives
        let index = index(concat!(
            "function meta::pure::functions::string::toUpper(s: String[1], t: String[1]): String[1] { $s }\n",
            "function meta::pure::functions::string::toUpper(s: Integer[1]): String[1] { 'n' }\n",
        ));
        let at = compiler(&index).functions_at(&j(fqn));
        let expected: Vec<Function> = natives
            .iter()
            .map(|n| Function::NativeFunctionDefinition(n.def().clone()))
            .chain([model_function(&index, fqn, 0), model_function(&index, fqn, 1)])
            .collect();
        assert_eq!(at, expected);
        assert!(matches!(&at[natives.len()], Function::FunctionDefinition(fd) if fd.parameters.len() == 2));
    }

    #[test]
    fn a_model_version_with_a_natives_id_is_that_native() {
        let fqn = j("meta::pure::functions::string::toUpper");
        let native = catalog().native_functions_at(&fqn).expect("an FQN")[0];
        // the model's own version of the native: its very signature, with a body
        let n = native.def();
        let twin = Function::FunctionDefinition(Arc::new(FunctionDefinition::user_written(
            n.qualified_name.clone(),
            n.type_parameters.clone(),
            n.multiplicity_parameters.clone(),
            n.parameters.clone(),
            n.return_type.clone(),
            n.return_multiplicity.clone(),
            Arc::from(Vec::new()),
            n.stereotypes.clone(),
            n.tagged_values.clone(),
        )));
        assert_eq!(&FunctionId::of(&twin), native.id());
        let mut index = index("Class a::A {}\n");
        index.add(&[twin.as_element()]).expect("the twin indexes");
        assert_eq!(index.find_function(&fqn).len(), 1);
        let at = compiler(&index).functions_at(&fqn);
        assert_eq!(at.len(), catalog().native_functions_at(&fqn).expect("an FQN").len());
        assert!(at.iter().all(|f| matches!(f, Function::NativeFunctionDefinition(_))));
    }

    #[test]
    fn a_model_only_fqn_answers_its_versions_and_an_unknown_one_none() {
        let index = index(concat!(
            "function w::h(x: String[1]): String[1] { $x }\n",
            "function w::h(x: Integer[1]): String[1] { 'n' }\n",
        ));
        let fc = compiler(&index);
        assert_eq!(
            fc.functions_at(&j("w::h")),
            vec![model_function(&index, "w::h", 0), model_function(&index, "w::h", 1)]
        );
        assert!(fc.exists(&j("w::h")));
        assert!(fc.functions_at(&j("w::nope")).is_empty());
        assert!(!fc.exists(&j("w::nope")));
    }

    #[test]
    fn a_signature_compiles_its_parameters_return_body_definition_and_id() {
        let index = index("Class w::P {}\nfunction w::f<T>(x: String[1], ys: T[*]): w::P[0..1] { let a = 1; $x; }\n");
        let arena = QueryArena::new();
        let f = model_function(&index, "w::f", 0);
        let typed = compile_with(&StandIn(&index), &arena, &f).expect("the signature compiles");
        let Function::FunctionDefinition(fd) = &f else {
            panic!("a function definition")
        };
        let expected = TypedFunction {
            qualified_name: j("w::f"),
            type_parameters: Arc::from(vec![j("T")]),
            multiplicity_parameters: Arc::from(vec![]),
            parameters: Arc::from(vec![
                TypedParameter {
                    name: j("x"),
                    r#type: Type::Primitive(Primitive::String),
                    multiplicity: Multiplicity::ONE,
                },
                TypedParameter {
                    name: j("ys"),
                    r#type: Type::TypeVar("T"),
                    multiplicity: Multiplicity::ZERO_MANY,
                },
            ]),
            return_type: Type::ClassType("w::P"),
            return_multiplicity: Multiplicity::ZERO_ONE,
            body: Some(fd.body.clone()),
            is_native: false,
            definition: f.clone(),
            id: FunctionId::of(&f),
        };
        assert_eq!(typed, expected);
        assert_eq!(fd.body.len(), 2);
    }

    #[test]
    fn a_native_compiles_with_no_body() {
        let index = index("Class a::A {}\n");
        let arena = QueryArena::new();
        let native = catalog()
            .native_functions_at(&j("meta::pure::functions::string::toUpper"))
            .expect("an FQN")[0];
        let f = Function::NativeFunctionDefinition(native.def().clone());
        let typed = compile_with(&StandIn(&index), &arena, &f).expect("the native compiles");
        assert!(typed.is_native);
        assert_eq!(typed.body, None);
        assert_eq!(&typed.id, native.id());
        assert_eq!(typed.definition, f);
    }

    #[test]
    fn an_empty_body_is_present() {
        let fd = FunctionDefinition::user_written(
            j("w::e"),
            Arc::from(vec![]),
            Arc::from(vec![]),
            Arc::from(vec![]),
            p::TypeExpression::TypeExpressionNameRef(Box::new(p::TypeExpressionNameRef {
                name: j(STRING),
                pos: None,
            })),
            p::Multiplicity::MultiplicityConcrete(Box::new(p::MultiplicityConcrete {
                lower_bound: 1,
                upper_bound: Some(1),
            })),
            Arc::from(vec![]),
            Arc::from(vec![]),
            Arc::from(vec![]),
        );
        let index = index("Class a::A {}\n");
        let arena = QueryArena::new();
        let typed = compile_with(&StandIn(&index), &arena, &Function::FunctionDefinition(Arc::new(fd)))
            .expect("the signature compiles");
        assert_eq!(typed.body.map(|b| b.len()), Some(0));
    }

    #[test]
    fn the_first_refusal_in_the_signature_is_the_answer() {
        // a parameter's type before its bounds, the parameters before the return
        let index = index(concat!(
            "function w::a(x: String[2..1], y: Integer[1]): String[2..1] { 'a' }\n",
            "function w::b(x: String[1]): String[2..1] { 'b' }\n",
        ));
        let arena = QueryArena::new();
        let a = compile_with(&StandIn(&index), &arena, &model_function(&index, "w::a", 0)).expect_err("refused");
        assert_eq!(
            a,
            Thrown::Java {
                class: "java.lang.IllegalArgumentException",
                message: Some(j(BOUND))
            }
        );
        let b = compile_with(&StandIn(&index), &arena, &model_function(&index, "w::b", 0)).expect_err("refused");
        assert_eq!(b.message(), Some(j(BOUND)));
    }

    #[test]
    fn a_broken_overload_is_dropped_and_an_all_broken_set_returns_the_first_refusal() {
        // ElementRefusalsTest.aBrokenOverloadIsDroppedAndAnAllBrokenSetRethrows, at the compiler
        let index = index(concat!(
            "function w::h(x: String[1]): String[1]\n{\n  $x\n}\n",
            "function w::h(x: Integer[2..1]): String[1]\n{\n  'n'\n}\n",
            "function w::k(x: Integer[2..1]): String[1]\n{\n  'n'\n}\n",
            "function w::k(x: Integer[3..1]): String[1]\n{\n  'n'\n}\n",
        ));
        let fc = compiler(&index);
        let arena = QueryArena::new();
        let h = fc
            .compile_all_with(&StandIn(&index), &arena, &j("w::h"))
            .expect("one overload compiles");
        assert_eq!(h.len(), 1);
        assert_eq!(h[0].definition, model_function(&index, "w::h", 0));
        let k = fc
            .compile_all_with(&StandIn(&index), &arena, &j("w::k"))
            .expect_err("every overload is refused");
        assert_eq!(k.java_class(), "java.lang.IllegalArgumentException");
        assert_eq!(k.message(), Some(j(BOUND)));
        // no overload: none, no refusal
        assert_eq!(
            fc.compile_all_with(&StandIn(&index), &arena, &j("w::none"))
                .expect("none"),
            vec![]
        );
    }

    #[test]
    fn the_public_compile_all_of_no_declaration_is_none() {
        // through the view's own classifier: nothing to classify, so no refusal and no overload
        let index = index("function w::h(x: String[1]): String[1] { $x }\n");
        let fc = compiler(&index);
        let arena = QueryArena::new();
        assert_eq!(fc.compile_all(&arena, &j("w::none")), Ok(vec![]));
        assert_eq!(fc.compile_all(&arena, &j("h")), Ok(vec![]));
    }

    #[test]
    fn the_overloads_compile_in_candidate_order() {
        let fqn = j("meta::pure::functions::string::toUpper");
        let index = index("function meta::pure::functions::string::toUpper(s: Integer[1]): String[1] { 'n' }\n");
        let fc = compiler(&index);
        let arena = QueryArena::new();
        let typed = fc
            .compile_all_with(&StandIn(&index), &arena, &fqn)
            .expect("the overloads compile");
        let definitions: Vec<Function> = typed.into_iter().map(|t| t.definition).collect();
        assert_eq!(definitions, fc.functions_at(&fqn));
        assert!(matches!(definitions.last(), Some(Function::FunctionDefinition(_))));
    }

    #[test]
    fn a_missing_binding_names_the_site_and_the_function() {
        let index = index("function model::funcs::present(): String[1] { 'x' }\n");
        let fc = compiler(&index);
        let site = j("derived property 'fullName' of model::Person");
        let e = fc
            .require_function(&j("model::funcs::missing"), &site)
            .expect_err("refused");
        assert_model_refusal(
            &e,
            "derived property 'fullName' of model::Person binds to unknown function 'model::funcs::missing'",
        );
        assert_eq!(fc.require_function(&j("model::funcs::present"), &site), Ok(()));
        // a native counts; a bare name never does
        assert_eq!(
            fc.require_function(&j("meta::pure::functions::string::toUpper"), &site),
            Ok(())
        );
        let bare = fc.require_function(&j("present"), &site).expect_err("refused");
        assert_model_refusal(
            &bare,
            "derived property 'fullName' of model::Person binds to unknown function 'present'",
        );
    }

    #[test]
    fn a_shape_is_met_by_some_overload_or_refused_in_the_sites_words() {
        let index = index(concat!(
            "Class model::Person { age: Integer[1]; }\n",
            "function my::funcs::wrong(p: model::Person[1]): String[1] { 'x' }\n",
            "function my::funcs::isAdult(p: model::Person[1]): Boolean[1] { true }\n",
            "function my::funcs::isAdult(p: model::Person[1], n: Integer[1]): String[1] { 'x' }\n",
            "function my::funcs::people(): model::Person[*] { model::Person.all() }\n",
            "function my::funcs::notAClass(): String[1] { 'x' }\n",
        ));
        let fc = compiler(&index);
        let constraint = j("constraint 'adult' of model::P2");
        let e = fc
            .require_shape(
                &j("my::funcs::wrong"),
                FunctionCompiler::returns_boolean_one,
                &constraint,
                "returning Boolean[1]",
            )
            .expect_err("refused");
        assert_model_refusal(
            &e,
            "constraint 'adult' of model::P2 must be realized by a function returning Boolean[1]",
        );
        // one overload of two has the shape
        assert_eq!(
            fc.require_shape(
                &j("my::funcs::isAdult"),
                FunctionCompiler::returns_boolean_one,
                &constraint,
                "returning Boolean[1]"
            ),
            Ok(())
        );
        let binding = j("class binding for 'model::Person' in mapping my::M");
        let class_many = |f: &Function| f.parameters().is_empty() && fc.returns_class_many(f);
        assert_eq!(
            fc.require_shape(
                &j("my::funcs::people"),
                class_many,
                &binding,
                "of the form (): Class[*]"
            ),
            Ok(())
        );
        let e = fc
            .require_shape(
                &j("my::funcs::notAClass"),
                class_many,
                &binding,
                "of the form (): Class[*]",
            )
            .expect_err("refused");
        assert_model_refusal(
            &e,
            "class binding for 'model::Person' in mapping my::M must be realized by a function of the form (): Class[*]",
        );
        // existence is the caller's: no function at all is the shape's refusal
        let e = fc
            .require_shape(
                &j("my::funcs::missing"),
                FunctionCompiler::returns_boolean_one,
                &constraint,
                "returning Boolean[1]",
            )
            .expect_err("refused");
        assert_model_refusal(
            &e,
            "constraint 'adult' of model::P2 must be realized by a function returning Boolean[1]",
        );
    }

    #[test]
    fn the_shape_predicates_read_the_declared_return_by_name_and_exact_multiplicity() {
        let index = index(concat!(
            "Class model::Person { age: Integer[1]; }\n",
            "Enum model::E { A }\n",
            "function t::b1(): Boolean[1] { true }\n",
            "function t::b01(): Boolean[0..1] { true }\n",
            "function t::bm(): Boolean[*] { true }\n",
            "function t::s1(): String[1] { 'x' }\n",
            "function t::pm(): model::Person[*] { model::Person.all() }\n",
            "function t::p1m(): model::Person[1..*] { model::Person.all() }\n",
            "function t::p1(): model::Person[1] { model::Person.all()->toOne() }\n",
            "function t::em(): model::E[*] { [] }\n",
            "function t::gm<T>(): T[*] { [] }\n",
            "function t::mm<T|m>(): model::Person[m] { model::Person.all() }\n",
        ));
        let fc = compiler(&index);
        let f = |name: &str| model_function(&index, &format!("t::{name}"), 0);
        let boolean_one: Vec<bool> = ["b1", "b01", "bm", "s1"]
            .iter()
            .map(|n| FunctionCompiler::returns_boolean_one(&f(n)))
            .collect();
        assert_eq!(boolean_one, [true, false, false, false]);
        let class_many: Vec<bool> = ["pm", "p1m", "p1", "em", "gm", "mm", "s1"]
            .iter()
            .map(|n| fc.returns_class_many(&f(n)))
            .collect();
        // an enum, a type variable and a multiplicity parameter are not [*] classes
        assert_eq!(class_many, [true, false, false, false, false, false, false]);
        // a native's declared return: `toUpper` returns String[1]
        let native = Function::NativeFunctionDefinition(
            catalog()
                .native_functions_at(&j("meta::pure::functions::string::toUpper"))
                .expect("an FQN")[0]
                .def()
                .clone(),
        );
        assert!(!FunctionCompiler::returns_boolean_one(&native) && !fc.returns_class_many(&native));
    }
}
