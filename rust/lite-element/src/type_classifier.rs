// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! `TypeClassifier` (TypeClassifier.java at 18feff3fe): F's shared type kernel -- a name to its kind (`find_type`:
//! primitive, primitive extension, the `Enum` metaclass, class, enum, by existence; no structure built) and the one
//! `TypeExpression -> Type` boundary (`classify`), with the declared-type checks (`require_arguments`: a class's
//! type and multiplicity argument counts, a primitive's type-variable values, in legend-engine's words). An unknown
//! name is refused ("Unknown type: ..."), never defaulted (AGENTS.md invariant 4).
//!
//! A view over the index, `Copy`, made per call (the model owns the index, so it cannot hold one: `Model::classifier`).
//! Every method that makes a `Type` takes the arena it allocates in; [`TypeClassifier::require_arguments`] makes none
//! (where it needs a name's kind it may classify in a scratch arena, or test the kind without one).
//!
//! RE-EXPRESSED (EF_CENSUS §4): TC:47's `InferenceKernel.ENUM_METACLASS_FQN` is [`lite_types::platform::ENUM`], the
//! same string, so the element-to-spec edge goes; TC:111 (`val : P(8)[1]`, a primitive with its constraint values)
//! classifies the name directly, where Java built a protocol `NameRef` to classify (`ArchitectureTest`'s
//! `PROTOCOL_DESUGAR_DEBT`).

use std::sync::Arc;

use lite_base::Alloc;
use lite_builtin::pure::catalog;
use lite_error::Phase;
use lite_front::records as p;
use lite_model::{ClassDefinition, EnumDefinition, JStr, cat};
use lite_names::{ModelIndex, Thrown};
use lite_types::{Multiplicity, Op, Type};

/// `TypeClassifier` (TypeClassifier.java:23): the kind manifest and the classification, over the index.
#[derive(Clone, Copy)]
pub struct TypeClassifier<'i> {
    model: &'i ModelIndex,
}

impl<'i> TypeClassifier<'i> {
    /// `new TypeClassifier(model)` (:27).
    pub fn new(model: &'i ModelIndex) -> TypeClassifier<'i> {
        TypeClassifier { model }
    }

    /// The index the classifier reads.
    pub fn model(&self) -> &'i ModelIndex {
        self.model
    }

    /// `findType(fqn)` (:32): the kind an FQN names -- a platform primitive, a primitive extension (its base
    /// primitive), the `Enum` metaclass (an enumeration-value type), a class, an enum -- or none. A class's or an
    /// enum's FQN is put in `arena`.
    pub fn find_type<'a, A: Alloc>(&self, arena: &'a A, fqn: &JStr) -> Option<Type<'a>> {
        unimplemented!("lite-element skeleton: TypeClassifier.findType")
    }

    /// `isClassFqn(fqn)` (:59): a class of the model or of the native catalog.
    pub fn is_class_fqn(&self, fqn: &JStr) -> bool {
        self.model.find_class(fqn).is_some() || catalog().find_native_class(fqn).is_some()
    }

    /// `isEnumFqn(fqn)` (:63): an enum of the model or of the native catalog.
    pub fn is_enum_fqn(&self, fqn: &JStr) -> bool {
        self.model.find_enum(fqn).is_some() || catalog().find_native_enum(fqn).is_some()
    }

    /// `classDef(fqn)` (:76): the definition behind a class FQN, the native catalog first (`KnowledgeLayer.classDef`).
    pub fn class_def(&self, fqn: &JStr) -> Option<&'i Arc<ClassDefinition>> {
        self.model.knowledge().class_def(fqn)
    }

    /// `enumDef(fqn)` (:81): the definition behind an enum FQN, the user model first, then the native catalog.
    pub fn enum_def(&self, fqn: &JStr) -> Option<&'i Arc<EnumDefinition>> {
        self.model.find_enum(fqn).or_else(|| catalog().find_native_enum(fqn))
    }

    /// `classify(te, typeParams)` (:91): a type expression as a kinded type in `arena` -- a type parameter is a
    /// `TypeVar`, `?` the anonymous one, a name its kind (erased by [`lite_types::platform::erase_tds_row`]) or
    /// "Unknown type"; a generic's head must exist, its multiplicity arguments read by `Multiplicity::of_argument`;
    /// function, relation and schema-algebra types structurally.
    pub fn classify<'a, A: Alloc>(
        &self,
        arena: &'a A,
        te: &p::TypeExpression,
        type_params: &[JStr],
    ) -> Result<Type<'a>, Thrown> {
        unimplemented!("lite-element skeleton: TypeClassifier.classify")
    }

    /// `requireArguments(te, typeParams)` (:162): a declared type names each class with as many type and
    /// multiplicity arguments as it declares parameters, and each primitive with its type variables' values
    /// (legend-engine's `CompileContext.newGenericType` texts); a bare `Result` is the engine's `Result<Any|1..*>`.
    pub fn require_arguments(&self, te: &p::TypeExpression, type_params: &[JStr]) -> Result<(), Thrown> {
        unimplemented!("lite-element skeleton: TypeClassifier.requireArguments")
    }

    /// `requireTypeVariables(fqn, values)` (:223): a primitive's type-variable values against its declaration --
    /// their count, each a literal of its variable's type ("Wrong type variables count (N) for type: ...", "Error for
    /// type: ..., 'v' is not an instance of T", Java's literal spellings).
    fn require_type_variables(&self, fqn: &JStr, values: &[p::ValueSpecification]) -> Result<(), Thrown> {
        unimplemented!("lite-element skeleton: TypeClassifier.requireTypeVariables")
    }

    /// `requireArity(fqn, types, multiplicities)` (:277): a class's argument counts ("Wrong type|multiplicity
    /// arguments count (N) for type: C<T|m>", `arityMismatch` :288); a class with no definition is Java's
    /// `IllegalStateException`.
    fn require_arity(&self, fqn: &JStr, types: usize, multiplicities: usize) -> Result<(), Thrown> {
        unimplemented!("lite-element skeleton: TypeClassifier.requireArity")
    }

    /// `headFqn(te)` (:299): the FQN heading a supertype or generic reference; a function, relation or
    /// schema-algebra type cannot head one (a `ModelException` each, in its words).
    pub fn head_fqn(te: &p::TypeExpression) -> Result<&JStr, Thrown> {
        unimplemented!("lite-element skeleton: TypeClassifier.headFqn")
    }

    /// `multiplicity(m)` (:314): `Multiplicity.from`, its `IllegalArgumentException` a raw Java throw, as Java lets it
    /// escape ([`Thrown::from_type_error`]).
    pub fn multiplicity<'a, A: Alloc>(arena: &'a A, m: &p::Multiplicity) -> Result<Multiplicity<'a>, Thrown> {
        Multiplicity::from(arena, m).map_err(Thrown::from_type_error)
    }
}

/// `unknownType(name)` (:151): the refusal of a name that is no primitive, class or enum.
fn unknown_type(name: &JStr) -> Thrown {
    Thrown::model(
        Phase::Model,
        cat!("Unknown type: '", name, "' is not a known primitive, class, or enum"),
        None,
    )
}

/// `op(op)` (:318): a schema-algebra operator, constant for constant.
fn op(op: p::TypeExpressionOp) -> Op {
    match op {
        p::TypeExpressionOp::EQUAL => Op::Equal,
        p::TypeExpressionOp::UNION => Op::Union,
        p::TypeExpressionOp::DIFFERENCE => Op::Difference,
        p::TypeExpressionOp::SUBSET => Op::Subset,
    }
}
