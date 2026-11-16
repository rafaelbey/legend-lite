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
//!
//! **The kernel seam** (2026-11-15, the family fill; `function_compiler`'s docs): `compile` reaches the classifier
//! and the key-stereotype test through [`ClassKernel`], a crate-private trait the [`TypeClassifier`] implements by
//! forwarding (to its `classify` and `head_fqn`, and to [`equality_keys::is_key_stereotype`]), so that the unit tests
//! compile classes over a stand-in while those are filled beside this file; [`compile`] passes the classifier itself.

use std::sync::Arc;

use lite_base::Alloc;
use lite_front::records as p;
use lite_model::{ClassDefinition, JStr, StereotypeApplication};
use lite_names::{Thrown, synth_fqn};

use crate::equality_keys;
use crate::function_compiler::Classify;
use crate::type_classifier::TypeClassifier;
use crate::typed::{
    EnforcementLevel, Property, PropertyDerived, PropertyStored, TypedClass, TypedConstraint, TypedParameter,
};

/// What class compilation asks beyond [`Classify`]: a supertype's head and the key-stereotype test (the module's
/// docs: the seam).
pub(crate) trait ClassKernel: Classify {
    /// `TypeClassifier.headFqn(te)` (TypeClassifier.java:299).
    fn head_fqn<'t>(&self, te: &'t p::TypeExpression) -> Result<&'t JStr, Thrown>;

    /// `EqualityKeys.isKeyStereotype(st)` (EqualityKeys.java:74).
    fn is_key_stereotype(&self, st: &StereotypeApplication) -> bool;
}

impl ClassKernel for TypeClassifier<'_> {
    fn head_fqn<'t>(&self, te: &'t p::TypeExpression) -> Result<&'t JStr, Thrown> {
        TypeClassifier::head_fqn(te)
    }

    fn is_key_stereotype(&self, st: &StereotypeApplication) -> bool {
        equality_keys::is_key_stereotype(st)
    }
}

/// `compile(cd)` (:35): the typed class of `cd`, its types in `arena`.
pub fn compile<'a, A: Alloc>(
    classifier: &TypeClassifier<'_>,
    arena: &'a A,
    cd: &ClassDefinition,
) -> Result<TypedClass<'a>, Thrown> {
    compile_with(classifier, arena, cd)
}

/// `compile(cd)` (:35) over `kernel` (the module's docs: the seam), in Java's order (the first refusal met is the
/// answer): each supertype's head then its type; each stored property's type, its multiplicity, its key flag; each
/// derived property's parameters (type then multiplicity each), its realizing FQN (the lifted FQN made first, even
/// for a bound one, as Java evaluates the argument), its type, its multiplicity; each constraint's realizing FQN, its
/// level, its message's FQN.
fn compile_with<'a, A: Alloc, K: ClassKernel>(
    kernel: &K,
    arena: &'a A,
    cd: &ClassDefinition,
) -> Result<TypedClass<'a>, Thrown> {
    let type_params = &cd.type_params;

    let mut super_fqns = Vec::with_capacity(cd.super_classes.len());
    // the supertypes WITH their arguments over this class's own parameters (Property<U,V|m> extends
    // AbstractProperty<{U[1]->V[m]}>): the kernel instantiates them per receiver (InferenceKernel.asSuper)
    let mut super_types = Vec::with_capacity(cd.super_classes.len());
    for sup in cd.super_classes.iter() {
        super_fqns.push(kernel.head_fqn(sup)?.clone());
        super_types.push(kernel.classify(arena, sup, type_params)?);
    }

    let mut properties = Vec::new();
    for pd in cd.properties.iter() {
        properties.push(Property::Stored(PropertyStored {
            name: pd.name.clone(),
            r#type: kernel.classify(arena, &pd.r#type, type_params)?,
            multiplicity: TypeClassifier::multiplicity(arena, &pd.multiplicity)?,
            equality_key: pd.stereotypes.iter().any(|st| kernel.is_key_stereotype(st)),
            has_default: pd.has_default,
            default_value: pd.default_value.clone(),
        }));
    }
    for dp in cd.derived_properties.iter() {
        let mut params = Vec::with_capacity(dp.parameters.len());
        for pd in &dp.parameters {
            params.push(TypedParameter {
                name: pd.name.clone(),
                r#type: kernel.classify(arena, &pd.r#type, type_params)?,
                multiplicity: TypeClassifier::multiplicity(arena, &pd.multiplicity)?,
            });
        }
        // the realizing function FQN: the lifted <owner>$prop$<name> for the sugar (Inline) form, the user-bound
        // function FQN for a Door-4 ref binding; reference safety already ran (ModelIntegrity, eager and total)
        let fqn = realized_fqn(&dp.realization, synth_fqn::prop(&cd.qualified_name, &dp.name)?);
        properties.push(Property::Derived(PropertyDerived {
            name: dp.name.clone(),
            r#type: kernel.classify(arena, &dp.r#type, type_params)?,
            multiplicity: TypeClassifier::multiplicity(arena, &dp.multiplicity)?,
            parameters: Arc::from(params),
            body_function_fqn: fqn,
        }));
    }

    let mut constraints = Vec::with_capacity(cd.constraints.len());
    for con in cd.constraints.iter() {
        // the lifted <owner>$constraint$<name> for sugar; the bound FQN for Door 4
        let fqn = realized_fqn(&con.realization, synth_fqn::constraint(&cd.qualified_name, &con.name)?);
        constraints.push(TypedConstraint {
            name: con.name.clone(),
            level: if is_warn(con.enforcement_level.as_ref()) {
                EnforcementLevel::Warn
            } else {
                EnforcementLevel::Error
            },
            predicate_fqn: fqn,
            message_fqn: match &con.message {
                None => None,
                Some(_) => Some(synth_fqn::constraint_msg(&cd.qualified_name, &con.name)?),
            },
        });
    }

    Ok(TypedClass {
        qualified_name: cd.qualified_name.clone(),
        type_parameters: type_params.clone(),
        super_class_fqns: Arc::from(super_fqns),
        super_types: Arc::from(super_types),
        properties: Arc::from(properties),
        constraints: Arc::from(constraints),
        is_native: cd.is_native,
        multiplicity_parameters: cd.multiplicity_params.clone(),
    })
}

/// `"Warn".equalsIgnoreCase(level)` (CC:86): `null` is no match. A fold over ASCII is the JDK's exactly here: no
/// character but the ASCII letters folds onto a letter of `Warn` under `equalsIgnoreCase`'s simple case mappings
/// (`Character.toUpperCase`, then `toLowerCase` of that; the tests check every character), so a UTF-16 unit outside
/// ASCII never matches.
fn is_warn(level: Option<&JStr>) -> bool {
    level.is_some_and(|l| {
        l.0.len() == 4
            && l.0
                .iter()
                .zip(b"warn")
                .all(|(&u, &c)| u8::try_from(u).is_ok_and(|b| b.to_ascii_lowercase() == c))
    })
}

/// `realizedFqn(r, liftedFqn)` (:105; MI:428): the function a behaviour hat is realized by -- the bound FQN of a
/// `Realization.Ref` (Door 4), else the lifted `<owner>$...$<name>` FQN Phase E synthesized for the sugar form.
pub fn realized_fqn(r: &p::Realization, lifted: JStr) -> JStr {
    match r {
        p::Realization::RealizationRef(rf) => rf.function_fqn.clone(),
        p::Realization::RealizationInline(_) => lifted,
    }
}

#[cfg(test)]
mod tests {
    use lite_base::QueryArena;
    use lite_error::Phase;
    use lite_model::PackageableElement;
    use lite_names::ModelIndex;
    use lite_types::{Multiplicity, Primitive, Type};

    use super::*;
    use crate::function_compiler::tests::{StandIn, j, model};

    /// The stand-in's class answers (`function_compiler`'s for types): a name heads a supertype (TC:299's other
    /// arms refuse a structural type; not the tests'), and the key stereotype is `equality.Key` of the platform's
    /// profile (EqualityKeys.java:74).
    impl ClassKernel for StandIn<'_> {
        fn head_fqn<'t>(&self, te: &'t p::TypeExpression) -> Result<&'t JStr, Thrown> {
            match te {
                p::TypeExpression::TypeExpressionNameRef(nr) => Ok(&nr.name),
                _ => panic!("the stand-in heads names only: {te:?}"),
            }
        }

        fn is_key_stereotype(&self, st: &StereotypeApplication) -> bool {
            st.profile_name == *"meta::pure::profiles::equality" && st.stereotype_name == *"Key"
        }
    }

    /// The class `fqn` of a model text and the text's index.
    fn class(text: &str, fqn: &str) -> (ModelIndex, Arc<ClassDefinition>) {
        let parsed = model(text);
        let cd = parsed
            .elements
            .iter()
            .find_map(|e| match e {
                PackageableElement::ClassDefinition(c) if c.qualified_name == *fqn => Some(c.clone()),
                _ => None,
            })
            .expect("the class");
        (ModelIndex::from(&parsed).expect("the model indexes"), cd)
    }

    fn constraint(
        name: &str,
        realization: p::Realization,
        level: Option<&str>,
        message: bool,
    ) -> p::ConstraintDefinition {
        p::ConstraintDefinition {
            name: j(name),
            realization,
            message: message.then(|| {
                p::ValueSpecification::CString(Arc::new(p::CString {
                    value: j("too young"),
                    pos: None,
                    multi_line: false,
                }))
            }),
            enforcement_level: level.map(j),
            external_id: None,
            owner: None,
            pos: None,
        }
    }

    fn inline() -> p::Realization {
        p::Realization::RealizationInline(Box::new(p::RealizationInline { body: Vec::new() }))
    }

    fn bound(fqn: &str) -> p::Realization {
        p::Realization::RealizationRef(Box::new(p::RealizationRef {
            function_fqn: j(fqn),
            source: None,
        }))
    }

    /// A class `w::C` with no members but `constraints`.
    fn with_constraints(constraints: Vec<p::ConstraintDefinition>) -> ClassDefinition {
        ClassDefinition {
            qualified_name: j("w::C"),
            type_params: Arc::from(vec![]),
            type_variables: Arc::from(vec![]),
            super_classes: Arc::from(vec![]),
            properties: Arc::from(vec![]),
            derived_properties: Arc::from(vec![]),
            constraints: Arc::from(constraints.into_iter().map(Box::new).collect::<Vec<_>>()),
            stereotypes: Arc::from(vec![]),
            tagged_values: Arc::from(vec![]),
            is_native: false,
            multiplicity_params: Arc::from(vec![]),
        }
    }

    #[test]
    fn a_class_compiles_its_supers_properties_and_constraints_in_source_order() {
        let (index, cd) = class(
            concat!(
                "Class w::Top {}\n",
                "Class w::Side {}\n",
                "Class w::C<T> extends w::Top, w::Side\n",
                "[\n  positive: $this.n > 0,\n  named(~function: $this.name->isNotEmpty() ~enforcementLevel: Warn ",
                "~message: 'no name')\n]\n",
                "{\n  <<equality.Key>> name: String[1];\n  n: Integer[0..1] = 1;\n  items: T[*];\n",
                "  label(sep: String[1]) { $this.name + $sep }: String[1];\n",
                "  fixed() { w::fixed }: w::Top[*];\n}\n",
                "function w::fixed(c: w::C<Any>[1]): w::Top[*] { [] }\n",
            ),
            "w::C",
        );
        let arena = QueryArena::new();
        let typed = compile_with(&StandIn(&index), &arena, &cd).expect("the class compiles");
        let stored = |name: &str, r#type, multiplicity, key, default: bool| {
            let pd = cd.properties.iter().find(|pd| pd.name == *name).expect("the property");
            assert_eq!(pd.has_default, default);
            Property::Stored(PropertyStored {
                name: j(name),
                r#type,
                multiplicity,
                equality_key: key,
                has_default: default,
                default_value: pd.default_value.clone(),
            })
        };
        let expected = TypedClass {
            qualified_name: j("w::C"),
            type_parameters: Arc::from(vec![j("T")]),
            super_class_fqns: Arc::from(vec![j("w::Top"), j("w::Side")]),
            super_types: Arc::from(vec![Type::ClassType("w::Top"), Type::ClassType("w::Side")]),
            properties: Arc::from(vec![
                stored(
                    "name",
                    Type::Primitive(Primitive::String),
                    Multiplicity::ONE,
                    true,
                    false,
                ),
                stored(
                    "n",
                    Type::Primitive(Primitive::Integer),
                    Multiplicity::ZERO_ONE,
                    false,
                    true,
                ),
                stored("items", Type::TypeVar("T"), Multiplicity::ZERO_MANY, false, false),
                Property::Derived(PropertyDerived {
                    name: j("label"),
                    r#type: Type::Primitive(Primitive::String),
                    multiplicity: Multiplicity::ONE,
                    parameters: Arc::from(vec![TypedParameter {
                        name: j("sep"),
                        r#type: Type::Primitive(Primitive::String),
                        multiplicity: Multiplicity::ONE,
                    }]),
                    body_function_fqn: j("w::C$prop$label"),
                }),
                Property::Derived(PropertyDerived {
                    name: j("fixed"),
                    r#type: Type::ClassType("w::Top"),
                    multiplicity: Multiplicity::ZERO_MANY,
                    parameters: Arc::from(vec![]),
                    body_function_fqn: j("w::fixed"),
                }),
            ]),
            constraints: Arc::from(vec![
                TypedConstraint {
                    name: j("positive"),
                    level: EnforcementLevel::Error,
                    predicate_fqn: j("w::C$constraint$positive"),
                    message_fqn: None,
                },
                TypedConstraint {
                    name: j("named"),
                    level: EnforcementLevel::Warn,
                    predicate_fqn: j("w::C$constraint$named"),
                    message_fqn: Some(j("w::C$constraintMsg$named")),
                },
            ]),
            is_native: false,
            multiplicity_parameters: Arc::from(vec![]),
        };
        assert_eq!(typed, expected);
        let Property::Stored(n) = &typed.properties[1] else {
            panic!("a stored property")
        };
        assert!(n.default_value.is_some());
    }

    #[test]
    fn a_class_with_no_members_needs_no_classifier() {
        let (index, cd) = class("Class w::E {}\n", "w::E");
        let arena = QueryArena::new();
        // the classifier itself: no member asks it anything
        let typed = compile(&TypeClassifier::new(&index), &arena, &cd).expect("the class compiles");
        assert_eq!(
            typed,
            TypedClass {
                qualified_name: j("w::E"),
                type_parameters: Arc::from(vec![]),
                super_class_fqns: Arc::from(vec![]),
                super_types: Arc::from(vec![]),
                properties: Arc::from(vec![]),
                constraints: Arc::from(vec![]),
                is_native: false,
                multiplicity_parameters: Arc::from(vec![]),
            }
        );
    }

    #[test]
    fn a_native_class_keeps_its_parameters_and_flag() {
        let cd = lite_builtin::pure::catalog()
            .find_native_class(&j("meta::pure::metamodel::type::Integer"))
            .expect("a native class");
        let index = ModelIndex::from(&model("Class a::A {}\n")).expect("the model indexes");
        let arena = QueryArena::new();
        let typed = compile_with(&StandIn(&index), &arena, cd).expect("the class compiles");
        assert!(typed.is_native);
        assert_eq!(&*typed.super_class_fqns, &[j("meta::pure::metamodel::type::Number")]);
        assert_eq!(&*typed.super_types, &[Type::Primitive(Primitive::Number)]);
    }

    #[test]
    fn only_warn_ignoring_case_is_warn_and_anything_else_none_included_is_error() {
        // CC:86, ported as it is (census ASK 12): the parser admits only `Error` and `Warn`, so the rest are records
        let levels = [
            (Some("Warn"), EnforcementLevel::Warn),
            (Some("warn"), EnforcementLevel::Warn),
            (Some("WARN"), EnforcementLevel::Warn),
            (Some("wArN"), EnforcementLevel::Warn),
            (Some("Error"), EnforcementLevel::Error),
            (None, EnforcementLevel::Error),
            (Some(""), EnforcementLevel::Error),
            (Some("Warning"), EnforcementLevel::Error),
            (Some("War"), EnforcementLevel::Error),
            (Some("Other"), EnforcementLevel::Error),
            (Some(" Warn"), EnforcementLevel::Error),
            // fullwidth letters fold to fullwidth ones, never to ASCII
            (Some("Ｗａｒｎ"), EnforcementLevel::Error),
        ];
        let cd = with_constraints(
            levels
                .iter()
                .enumerate()
                .map(|(i, (level, _))| constraint(&format!("c{i}"), inline(), *level, false))
                .collect(),
        );
        let index = ModelIndex::from(&model("Class a::A {}\n")).expect("the model indexes");
        let arena = QueryArena::new();
        let typed = compile_with(&StandIn(&index), &arena, &cd).expect("the class compiles");
        let got: Vec<EnforcementLevel> = typed.constraints.iter().map(|c| c.level).collect();
        let want: Vec<EnforcementLevel> = levels.iter().map(|(_, l)| *l).collect();
        assert_eq!(got, want);
    }

    /// `Character.toUpperCase` / `toLowerCase` on a code point: the simple mappings (a full mapping of several
    /// characters keeps the character; U+0130's simple lower case is `i`), as lite-normalizer's `jdk.rs` spells them.
    fn simple_upper(c: char) -> char {
        let mut up = c.to_uppercase();
        match (up.next(), up.next()) {
            (Some(u), None) => u,
            _ => c,
        }
    }

    fn simple_lower(c: char) -> char {
        if c == '\u{0130}' {
            return 'i';
        }
        let mut low = c.to_lowercase();
        match (low.next(), low.next()) {
            (Some(l), None) => l,
            _ => c,
        }
    }

    #[test]
    fn no_character_outside_ascii_folds_onto_a_letter_of_warn() {
        // equalsIgnoreCase's per-character rule: equal, or equal upper cases, or equal lower cases of the upper ones
        let folds = |x: char, y: char| {
            x == y || {
                let (ux, uy) = (simple_upper(x), simple_upper(y));
                ux == uy || simple_lower(ux) == simple_lower(uy)
            }
        };
        let strays: Vec<(char, char)> = (0..=0x10FFFFu32)
            .filter_map(char::from_u32)
            .filter(|c| !c.is_ascii())
            .flat_map(|c| "WARNwarn".chars().map(move |w| (c, w)))
            .filter(|&(c, w)| folds(c, w))
            .collect();
        assert_eq!(strays, vec![]);
        assert!(is_warn(Some(&j("wARN"))) && !is_warn(None) && !is_warn(Some(&j("warn "))));
    }

    #[test]
    fn a_bound_realization_names_its_function_and_a_sugar_one_the_lifted_fqn() {
        let cd = with_constraints(vec![
            constraint("adult", bound("my::funcs::isAdult"), Some("Error"), true),
            constraint("sugar", inline(), None, true),
        ]);
        let index = ModelIndex::from(&model("Class a::A {}\n")).expect("the model indexes");
        let arena = QueryArena::new();
        let typed = compile_with(&StandIn(&index), &arena, &cd).expect("the class compiles");
        assert_eq!(
            &*typed.constraints,
            &[
                TypedConstraint {
                    name: j("adult"),
                    level: EnforcementLevel::Error,
                    predicate_fqn: j("my::funcs::isAdult"),
                    // the message's FQN is the lifted one whatever the predicate's binding
                    message_fqn: Some(j("w::C$constraintMsg$adult")),
                },
                TypedConstraint {
                    name: j("sugar"),
                    level: EnforcementLevel::Error,
                    predicate_fqn: j("w::C$constraint$sugar"),
                    message_fqn: Some(j("w::C$constraintMsg$sugar")),
                },
            ]
        );
        assert_eq!(realized_fqn(&bound("a::f"), j("w::C$prop$p")), j("a::f"));
        assert_eq!(realized_fqn(&inline(), j("w::C$prop$p")), j("w::C$prop$p"));
    }

    #[test]
    fn the_lifted_fqn_is_made_even_for_a_bound_realization() {
        // SynthFqn.constraint(owner, name) is Java's argument, evaluated before realizedFqn: a '$' in the name is
        // refused though the bound FQN would be the answer
        let cd = with_constraints(vec![constraint("a$b", bound("my::funcs::isAdult"), None, false)]);
        let index = ModelIndex::from(&model("Class a::A {}\n")).expect("the model indexes");
        let arena = QueryArena::new();
        let e = compile_with(&StandIn(&index), &arena, &cd).expect_err("refused");
        assert_eq!(
            e,
            Thrown::illegal_argument(j("synth-FQN name contains the reserved '$' sigil: a$b"))
        );
    }

    #[test]
    fn the_first_refusal_in_the_class_is_the_answer() {
        // a stored property's type before its bounds, the stored properties before the derived ones
        let (index, cd) = class(
            "Class w::C { a: String[2..1]; b: String[3..1]; d() { 'x' }: String[1]; }\n",
            "w::C",
        );
        let arena = QueryArena::new();
        let e = compile_with(&StandIn(&index), &arena, &cd).expect_err("refused");
        assert_eq!(e, Thrown::illegal_argument(j("upper (1) must be >= lower (2)")));
        // a supertype the classifier refuses
        let cd = ClassDefinition {
            super_classes: Arc::from(vec![p::TypeExpression::TypeExpressionNameRef(Box::new(
                p::TypeExpressionNameRef {
                    name: j("w::Missing"),
                    pos: None,
                },
            ))]),
            ..with_constraints(vec![])
        };
        let e = compile_with(&StandIn(&index), &arena, &cd).expect_err("refused");
        assert_eq!(
            e,
            Thrown::model(
                Phase::Model,
                j("Unknown type: 'w::Missing' is not a known primitive, class, or enum"),
                None
            )
        );
    }
}
