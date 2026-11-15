// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! F's records (2026-11-15, the element slice's skeleton): Java's `TypedClass`, `Property` (`Stored`, `Derived`),
//! `TypedConstraint`, `TypedEnum`, `TypedParameter` and `TypedFunction` at 18feff3fe, component for component, in
//! Java's order. Pure data: a nominal type is an FQN (`lite_types::Type`'s `ClassType`/`EnumType`), never a live
//! reference, so a walk goes through the model's lookups (`TypedClass.java`'s contract; AGENTS.md invariant 5).
//!
//! - Lists are `Arc<[T]>` (Java's `List.copyOf`: the record's own, immutable; a `null` list is the empty one), names
//!   `JStr`, types the type family's [`Type<'m>`] and [`Multiplicity<'m>`], whose records live in the model's arena
//!   (`'m`); every record is covariant in `'m`, so a model's is a query's with no conversion.
//! - Equality is the record's: derived over every component, as Java's record `equals` is
//!   (`TypedNativeCall`/`TypedUserCall` compare their callee by it). A default value and a body are protocol records
//!   (`ValueSpecification`), whose equality is derived too.
//! - **Not ported** (EF_CENSUS §3, §4): `TypedConstraint.externalId` (always empty, never read) and
//!   `TypedConstraint.of` (no main caller); `TypedFunction`'s 8- and 9-argument constructors and its null
//!   `definition`/`id` (test-only), so `definition` and `id` are plain components here and Java's `id()` throw on a
//!   null one has no Rust form; `TypedNominal` and `TypedElement` (no reader); `Property.Stored`'s 4- and 5-argument
//!   constructors (no main caller; its 3-argument one is [`PropertyStored::new`], `PureModelContext.findProperty`'s
//!   association end).
//! - **`Hash`** (2026-11-15, the brief's question). No map or set is keyed by a `TypedFunction` itself by value: the
//!   one keyed by it, `SpecCompiler.memo`, is an `IdentityHashMap` (keyed by the instance the function cell hands
//!   out, census §9.1). But one is hashed by value through its call: `TypedNativeCall.hashCode` and
//!   `TypedUserCall.hashCode` hash their callee (`Objects.hash(callee, ...)`), and H's resolver puts typed nodes in
//!   value-keyed sets -- `ConstructedRowForm.chains` (:49, :58) a `toOne` `TypedNativeCall` itself,
//!   `StackBuilder`'s `LinkedHashSet<TypedSpec>`s (:1157, :1452, :1480, :1525) join conditions, calls among them
//!   (searched at 18feff3fe). So `TypedFunction` writes `Hash` by hand: Java's derives it over every component, but a
//!   body and a definition hold protocol records, which derive `PartialEq` alone, so it hashes the components that
//!   have `Hash` -- the name, the id, the parameters, the return type and multiplicity -- a subset, so two equal
//!   functions hash alike (only a hash's consistency with equality is seen: a `LinkedHashSet`'s order is its
//!   insertions'). It is not `Eq` for the same reason (a protocol record is `PartialEq` alone): G's typed tree
//!   decides, with its set. The other records: no `Set` or `Map` in core's main sources is keyed by a `TypedClass`,
//!   `Property`, `TypedEnum`, `TypedConstraint` or `TypedParameter`, and no typed node (`compiler/spec/typed`) holds
//!   one (searched at 18feff3fe), so only `TypedParameter` is hashed, inside a `TypedFunction`'s hash; it and
//!   `EnforcementLevel` derive `Hash` because their components have it.

use std::hash::{Hash, Hasher};
use std::sync::Arc;

use lite_front::records as p;
use lite_model::{Function, FunctionId, JStr};
use lite_types::{Multiplicity, Type};

/// `TypedClass` (TypedClass.java:28): a compiled class -- its locally declared members only (inherited and
/// association-injected ones are the model's lookups'), its supertypes by FQN and as types over its own parameters.
#[derive(Debug, Clone, PartialEq)]
pub struct TypedClass<'m> {
    pub qualified_name: JStr,
    /// generic type parameter names (`<T, U>`)
    pub type_parameters: Arc<[JStr]>,
    /// direct superclass FQNs (each supertype's head)
    pub super_class_fqns: Arc<[JStr]>,
    /// the supertypes with their arguments over this class's own parameters
    /// (`Property<U,V|m> extends AbstractProperty<{U[1]->V[m]}>`)
    pub super_types: Arc<[Type<'m>]>,
    /// stored, then derived, in source order
    pub properties: Arc<[Property<'m>]>,
    pub constraints: Arc<[TypedConstraint]>,
    /// a bootstrap native class (the platform's catalog)
    pub is_native: bool,
    /// the multiplicity parameters (`<T|m>`), apart from the type parameters
    pub multiplicity_parameters: Arc<[JStr]>,
}

/// `Property` (Property.java:28, sealed): a class member as the model's member walk returns it, stored or derived,
/// with one `name`/`type`/`multiplicity` surface.
#[derive(Debug, Clone, PartialEq)]
pub enum Property<'m> {
    Stored(PropertyStored<'m>),
    Derived(PropertyDerived<'m>),
}

/// `Property.Stored` (Property.java:52): `name: Type[multiplicity]`, its `<<equality.Key>>` flag, whether it declares
/// a default and the default's (un-typed) expression.
#[derive(Debug, Clone, PartialEq)]
pub struct PropertyStored<'m> {
    pub name: JStr,
    pub r#type: Type<'m>,
    pub multiplicity: Multiplicity<'m>,
    /// carries `<<equality.Key>>` (X5: a keyed class's instances compare by their key properties)
    pub equality_key: bool,
    /// declares a default (`distinct: Boolean[1] = false`): exempt from `^new`'s missing-required check
    pub has_default: bool,
    pub default_value: Option<p::ValueSpecification>,
}

/// `Property.Derived` (Property.java:90): a computed property's signature and the FQN of its externalized body
/// function (`model::Person$prop$fullName`, or the function a `Realization.Ref` binds).
#[derive(Debug, Clone, PartialEq)]
pub struct PropertyDerived<'m> {
    pub name: JStr,
    pub r#type: Type<'m>,
    pub multiplicity: Multiplicity<'m>,
    pub parameters: Arc<[TypedParameter<'m>]>,
    pub body_function_fqn: JStr,
}

impl<'m> PropertyStored<'m> {
    /// `new Property.Stored(name, type, multiplicity)` (Property.java:79): no key, no default.
    pub fn new(name: JStr, r#type: Type<'m>, multiplicity: Multiplicity<'m>) -> PropertyStored<'m> {
        PropertyStored {
            name,
            r#type,
            multiplicity,
            equality_key: false,
            has_default: false,
            default_value: None,
        }
    }
}

impl<'m> Property<'m> {
    /// `name()`: the unqualified name.
    pub fn name(&self) -> &JStr {
        match self {
            Property::Stored(s) => &s.name,
            Property::Derived(d) => &d.name,
        }
    }

    /// `type()`: the classified value type.
    pub fn r#type(&self) -> Type<'m> {
        match self {
            Property::Stored(s) => s.r#type,
            Property::Derived(d) => d.r#type,
        }
    }

    /// `multiplicity()`.
    pub fn multiplicity(&self) -> Multiplicity<'m> {
        match self {
            Property::Stored(s) => s.multiplicity,
            Property::Derived(d) => d.multiplicity,
        }
    }
}

/// `TypedConstraint` (TypedConstraint.java:24), without `externalId` (census §4): a constraint's name, level and the
/// FQNs of its externalized predicate and message functions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypedConstraint {
    pub name: JStr,
    pub level: EnforcementLevel,
    /// the synthesized `this -> Boolean[1]` predicate (`model::Person$constraint$validAge`), or the bound function
    pub predicate_fqn: JStr,
    /// the synthesized `this -> String[1]` message (`...$constraintMsg$validAge`), when the constraint has a message
    pub message_fqn: Option<JStr>,
}

/// `TypedConstraint.EnforcementLevel` (TypedConstraint.java:32): FINOS stores a string; ERROR is the default.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EnforcementLevel {
    Error,
    Warn,
}

impl EnforcementLevel {
    /// Every constant, in Java's order.
    pub const ALL: [EnforcementLevel; 2] = [EnforcementLevel::Error, EnforcementLevel::Warn];

    /// The constant's Java `name()`.
    pub fn name(self) -> &'static str {
        match self {
            EnforcementLevel::Error => "ERROR",
            EnforcementLevel::Warn => "WARN",
        }
    }
}

/// `TypedEnum` (TypedEnum.java:18): an enumeration's name and its value names in declaration order (an empty enum is
/// legal Legend).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypedEnum {
    pub qualified_name: JStr,
    pub values: Arc<[JStr]>,
}

/// `TypedParameter` (TypedParameter.java:17): a named, classified parameter of a function or a derived property.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TypedParameter<'m> {
    pub name: JStr,
    pub r#type: Type<'m>,
    pub multiplicity: Multiplicity<'m>,
}

/// `TypedFunction` (TypedFunction.java:53): a compiled function's signature and its parsed, un-type-checked body --
/// present for a function compiled from source, absent for a native -- with its source definition and its identity,
/// computed once when it is compiled. Overloads share a name and are the model's `find_function` list, one `Arc` per
/// overload handed out by the function cell (G's memo keys by the instance, census §9.1).
#[derive(Debug, Clone, PartialEq)]
pub struct TypedFunction<'m> {
    /// shared by every overload
    pub qualified_name: JStr,
    pub type_parameters: Arc<[JStr]>,
    pub multiplicity_parameters: Arc<[JStr]>,
    /// in source order
    pub parameters: Arc<[TypedParameter<'m>]>,
    pub return_type: Type<'m>,
    pub return_multiplicity: Multiplicity<'m>,
    /// the statements, `None` for a native (an empty body `{}` is `Some` of none)
    pub body: Option<Arc<[p::ValueSpecification]>>,
    /// a `native function` declaration
    pub is_native: bool,
    /// the source definition (census §4: non-optional; Java's null form is test-only)
    pub definition: Function,
    /// `FunctionId.of(definition)`: what every rule table is keyed by and every callee compare uses
    pub id: FunctionId,
}

/// Over the components that have a `Hash` (the module's docs: the body and the definition are protocol records), a
/// subset of the derived equality's, so equal functions hash alike.
impl Hash for TypedFunction<'_> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.qualified_name.hash(state);
        self.id.hash(state);
        self.parameters.hash(state);
        self.return_type.hash(state);
        self.return_multiplicity.hash(state);
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use lite_types::{Multiplicity, Primitive, Type};

    use super::*;

    /// THE PROOF OF COVARIANCE: a model's records are a query's with no conversion.
    #[allow(dead_code)]
    fn shorten<'q, 'm: 'q>(
        c: &'q TypedClass<'m>,
        f: &'q TypedFunction<'m>,
    ) -> (&'q TypedClass<'q>, &'q TypedFunction<'q>) {
        (c, f)
    }

    const fn assert_send_sync<T: Send + Sync>() {}
    const _: () = assert_send_sync::<TypedClass<'static>>();
    const _: () = assert_send_sync::<TypedFunction<'static>>();
    const _: () = assert_send_sync::<TypedEnum>();

    #[test]
    fn a_property_answers_its_variants_name_type_and_multiplicity() {
        let stored = Property::Stored(PropertyStored::new(
            JStr::from("name"),
            Type::Primitive(Primitive::String),
            Multiplicity::ONE,
        ));
        let derived = Property::Derived(PropertyDerived {
            name: JStr::from("fullName"),
            r#type: Type::Primitive(Primitive::String),
            multiplicity: Multiplicity::ZERO_ONE,
            parameters: Arc::from(vec![TypedParameter {
                name: JStr::from("sep"),
                r#type: Type::Primitive(Primitive::String),
                multiplicity: Multiplicity::ONE,
            }]),
            body_function_fqn: JStr::from("a::P$prop$fullName"),
        });
        assert_eq!(
            (stored.name(), stored.r#type(), stored.multiplicity()),
            (
                &JStr::from("name"),
                Type::Primitive(Primitive::String),
                Multiplicity::ONE
            )
        );
        assert_eq!(
            (derived.name(), derived.multiplicity()),
            (&JStr::from("fullName"), Multiplicity::ZERO_ONE)
        );
        let Property::Stored(s) = &stored else {
            panic!("a stored property")
        };
        assert!(!s.equality_key && !s.has_default && s.default_value.is_none());
    }

    #[test]
    fn equal_functions_hash_alike_and_a_renamed_copy_is_another_function() {
        use std::hash::{BuildHasher, RandomState};

        let native = lite_builtin::pure::catalog().all()[0].clone();
        let definition = Function::NativeFunctionDefinition(native);
        let f = TypedFunction {
            qualified_name: definition.qualified_name().clone(),
            type_parameters: Arc::from(vec![]),
            multiplicity_parameters: Arc::from(vec![]),
            parameters: Arc::from(vec![]),
            return_type: Type::Primitive(Primitive::Boolean),
            return_multiplicity: Multiplicity::ONE,
            body: None,
            is_native: true,
            id: FunctionId::of(&definition),
            definition,
        };
        let hasher = RandomState::new();
        let copy = f.clone();
        assert_eq!(copy, f);
        assert_eq!(hasher.hash_one(&copy), hasher.hash_one(&f));
        // SignatureApart's renamed copy keeps the id: not equal (equality is over every component, not the id)
        let renamed = TypedFunction {
            qualified_name: JStr::from("a::renamed"),
            ..f.clone()
        };
        assert_ne!(renamed, f);
    }

    #[test]
    fn the_enforcement_levels_are_javas_constants() {
        assert_eq!(EnforcementLevel::ALL.map(EnforcementLevel::name), ["ERROR", "WARN"]);
    }
}
