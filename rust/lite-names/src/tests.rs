// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! The names layer's unit tests (2026-11-12): each piece on a small model. The whole is held to the live Java's
//! answers by `tests/names_differential.rs`.

use lite_front::doors::{Dialect, on_parse_stack};
use lite_model::linked::LinkedMap;
use lite_model::protocol::ValueSpecification as VS;
use lite_model::{JStr, PackageableElement, ParsedModel};

use crate::milestoning::{Dimension, MilestoningStrategy};
use crate::model_index::ModelIndex;
use crate::{
    calls, core_imports, relational_type_inference, resolver, store_compiler, symbol_table, synth_fqn, view_signatures,
};

fn j(s: &str) -> JStr {
    JStr::from(s)
}

fn model(text: &str) -> ParsedModel {
    let units: Vec<u16> = text.encode_utf16().collect();
    lite_builtin::door::parse(&units, Dialect::LegendLite).expect("the model parses")
}

fn class<'a>(m: &'a ParsedModel, fqn: &str) -> &'a lite_model::ClassDefinition {
    m.elements
        .iter()
        .find_map(|e| match e {
            PackageableElement::ClassDefinition(c) if c.qualified_name == fqn => Some(&**c),
            _ => None,
        })
        .expect("the class")
}

fn super_name(c: &lite_model::ClassDefinition, i: usize) -> JStr {
    c.super_classes[i].raw_class_name().unwrap().clone()
}

#[test]
fn the_symbol_table_interns_densely_and_stably() {
    let mut t = symbol_table::SymbolTable::new();
    assert_eq!(t.intern(&j("a::B")), 0);
    assert_eq!(t.intern(&j("a::C")), 1);
    assert_eq!(t.intern(&j("a::B")), 0);
    assert_eq!(t.resolve_id(&j("a::D")), symbol_table::UNRESOLVED);
    assert_eq!(t.name_of(1), Some(&j("a::C")));
    assert_eq!(t.size(), 2);
}

#[test]
fn a_synthesized_name_refuses_the_sigil_in_an_operand() {
    assert_eq!(synth_fqn::prop(&j("a::B"), &j("x")).unwrap(), j("a::B$prop$x"));
    let e = synth_fqn::view(&j("a::Db$x"), &j("V")).unwrap_err();
    assert_eq!(e.java_class(), "java.lang.IllegalArgumentException");
    assert_eq!(
        e.message().unwrap(),
        j("synth-FQN ownerFqn contains the reserved '$' sigil: a::Db$x")
    );
}

#[test]
fn the_core_import_group_is_the_engines_sequence() {
    assert_eq!(core_imports::SEQUENCE.len(), 32);
    assert_eq!(core_imports::SEQUENCE[0], "meta::pure::metamodel");
    assert_eq!(core_imports::SEQUENCE[31], "meta::pure::precisePrimitives");
}

#[test]
fn a_models_names_resolve_through_its_imports_its_package_and_the_core_group() {
    on_parse_stack(|| {
        let m = model(
            "import other::*;\nClass app::A extends B {}\nClass app::B {}\nClass other::C {}\n\
             Class app::D extends C { x: String[1]; }\n",
        );
        let r = resolver::resolve(&m).unwrap();
        // the own package
        assert_eq!(super_name(class(&r, "app::A"), 0), j("app::B"));
        // the section's import
        assert_eq!(super_name(class(&r, "app::D"), 0), j("other::C"));
        // the M3 primitive, spelled by the prelude
        let ty = &class(&r, "app::D").properties[0].r#type;
        assert_eq!(ty.raw_class_name().unwrap(), &j("meta::pure::metamodel::type::String"));
        // the parsed model has no element spans (a text's) and no unclaimed sections: the resolved one keeps them so
        assert!(r.unclaimed_sections.is_empty() && r.element_spans.is_empty());
    });
}

/// Three places the resolver did not walk (2026-11-13, the names fixes, item 4, ruling 28): an enumerated-expression
/// property mapping's operation resolves as an expression mapping's does; a generic's type-variable values resolve as
/// any value; a quoted tree call's tree resolves in the SECTIONLESS scope (the core imports, never the section's), as
/// the engine compiles the quoted string in a section of its own. A text's type-variable values are integer and string
/// literals (the parser takes no other); a model given as protocol records may carry any value, so the test sets one.
#[test]
fn the_resolver_walks_enumerated_expressions_type_variable_values_and_quoted_trees() {
    on_parse_stack(|| {
        let parsed = model(concat!(
            "###Relational\n",
            "Database app::Db ( Table T (ID INTEGER PRIMARY KEY, K VARCHAR(10)) )\n",
            "Database app::Other ( Table T2 (K VARCHAR(10)) )\n",
            "###Pure\n",
            "import app::*;\n",
            "Enum app::E { A, B }\n",
            "Class app::C { e: E[1]; v: Varchar(8)[1]; }\n",
            "function app::g():Any[*] { compileLegendValueSpecification('#{C{e, x(DurationUnit.DAYS)}}#') }\n",
            "###Mapping\n",
            "import app::*;\n",
            "Mapping app::M (\n",
            "  E: EnumerationMapping EM { A: 'a', B: 'b' }\n",
            "  C: Relational { ~mainTable [Db]T e: EnumerationMapping EM: case(equal([Other]T2.K, 'x'), 'a', 'b') }\n",
            ")\n",
        ));
        // the class's Varchar(8) carries the value E.A instead, as a protocol model may
        let elements: Vec<PackageableElement> = parsed
            .elements
            .iter()
            .map(|e| match e {
                PackageableElement::ClassDefinition(c) if c.qualified_name == "app::C" => {
                    let mut props: Vec<_> = c.properties.to_vec();
                    let lite_model::protocol::TypeExpression::TypeExpressionGeneric(g) = &props[1].r#type else {
                        panic!("a generic")
                    };
                    let mut g = (**g).clone();
                    g.type_variable_values =
                        vec![VS::EnumValue(std::sync::Arc::new(lite_model::protocol::EnumValue {
                            full_path: j("E"),
                            value: j("A"),
                            enumeration_pos: None,
                            pos: None,
                            node: false,
                        }))];
                    props[1] = std::sync::Arc::new(lite_model::ClassDefinitionPropertyDefinition {
                        r#type: lite_model::protocol::TypeExpression::TypeExpressionGeneric(Box::new(g)),
                        ..(*props[1]).clone()
                    });
                    PackageableElement::ClassDefinition(std::sync::Arc::new(lite_model::ClassDefinition {
                        properties: props.into(),
                        ..(**c).clone()
                    }))
                }
                other => other.clone(),
            })
            .collect();
        let m = ParsedModel {
            elements: elements.into(),
            ..parsed
        };
        let r = resolver::resolve(&m).unwrap();
        let text = |fqn: &str| {
            let el = r
                .elements
                .iter()
                .find(|e| e.qualified_name() == fqn)
                .expect("the element");
            lite_front::codec::write(el, true)
        };
        let mapping = text("app::M");
        assert!(
            mapping.contains("\"databaseName\":\"app::Other\"") && !mapping.contains("\"databaseName\":\"Other\""),
            "the enumerated expression's [Other] resolves: {mapping}"
        );
        let class = text("app::C");
        assert!(
            class.contains("\"fullPath\":\"app::E\""),
            "the type-variable value resolves: {class}"
        );
        let function = text("app::g");
        assert!(
            function.contains("meta::pure::functions::date::DurationUnit"),
            "the tree's qualifier argument resolves through the core imports: {function}"
        );
    });
}

/// A rebuilt record keeps every component but the names it resolved (2026-11-13, the names fixes, items 1 and 2,
/// ruling 28): a resolved model its unclaimed sections; a name reference, a collection and an enum value their
/// positions, the enum value its wire spelling too. Java's convenience constructors dropped each (and the enum value
/// became the real `enumValue` node).
#[test]
fn a_rebuilt_record_keeps_every_component_but_its_names() {
    on_parse_stack(|| {
        // the platform's dialect leaves a section no grammar claims unclaimed (lite's refuses it)
        let units: Vec<u16> = concat!(
            "import app::*;\n",
            "Class app::A { b: B[1]; }\n",
            "Class app::B { b: String[0..1]; }\n",
            "Enum app::E { x }\n",
            "function app::g():Any[*] { [E.x, E.x] }\n",
            "###Unknown\nwhatever\n",
        )
        .encode_utf16()
        .collect();
        let m = lite_builtin::door::parse(&units, Dialect::LegendPlatform).expect("the model parses");
        assert_eq!(m.unclaimed_sections.len(), 1);
        let r = resolver::resolve(&m).unwrap();
        assert_eq!(
            r.unclaimed_sections, m.unclaimed_sections,
            "a resolved model keeps its unclaimed sections"
        );
        let a = class(&r, "app::A");
        let lite_model::protocol::TypeExpression::TypeExpressionNameRef(nr) = &a.properties[0].r#type else {
            panic!("a name reference")
        };
        assert_eq!(nr.name, j("app::B"));
        assert!(nr.pos.is_some(), "a rebuilt name reference keeps its position");
        let body = r
            .elements
            .iter()
            .find_map(|e| match e {
                PackageableElement::FunctionDefinition(f) if f.qualified_name == "app::g" => Some(f.body[0].clone()),
                _ => None,
            })
            .expect("the function");
        let VS::PureCollection(coll) = body else {
            panic!("a collection")
        };
        assert!(coll.pos.is_some(), "a rebuilt collection keeps its position");
        let VS::EnumValue(ev) = &coll.values[0] else {
            panic!("an enum value")
        };
        assert_eq!(ev.full_path, j("app::E"));
        assert!(
            ev.pos.is_some() && ev.enumeration_pos.is_some(),
            "a rebuilt enum value keeps its positions"
        );
        assert!(
            !ev.node,
            "and its wire spelling: a property on an element pointer, not the enumValue node"
        );
    });
}

#[test]
fn an_ambiguous_name_is_refused_strictly_and_walled_tolerantly() {
    on_parse_stack(|| {
        let m = model(
            "import p::*;\nimport q::*;\nClass p::X {}\nClass q::X {}\nClass app::A extends X {}\nClass app::B {}\n",
        );
        let e = resolver::resolve(&m).unwrap_err();
        assert_eq!(e.java_class(), "com.legend.error.ModelException");
        let lite_error_phase = match &e {
            crate::Thrown::Compile(c) => c.phase,
            _ => None,
        };
        assert_eq!(lite_error_phase, Some(lite_error::Phase::Resolve));
        let mut walls = LinkedMap::new();
        let r = resolver::resolve_tolerant(&m, &mut walls).unwrap();
        assert_eq!(walls.len(), 1);
        let (key, line) = walls.iter().next().unwrap();
        assert_eq!(key, &j("app::A"));
        assert!(line.lossy().starts_with("ambiguous reference 'X'"));
        // poison, not drop: the walled element stays, unresolved
        assert_eq!(super_name(class(&r, "app::A"), 0), j("X"));
    });
}

/// A qualified call is itself (2026-11-13, the names fixes, item 5, ruling 28): the call tiers no longer prefix it
/// with a package, so `b::f()` under `import a::*;` is not `a::b::f` (Pure has no relative qualified names).
#[test]
fn a_qualified_call_is_never_prefixed_by_a_tier() {
    on_parse_stack(|| {
        let m = model(
            "import a::*;\nfunction a::b::f():String[1] { 'x' }\nfunction c::g():String[1] { b::f() }\n\
             function c::h():String[1] { a::b::f() }\n",
        );
        let r = resolver::resolve(&m).unwrap();
        let call = |fqn: &str| {
            let body = r
                .elements
                .iter()
                .find_map(|e| match e {
                    PackageableElement::FunctionDefinition(f) if f.qualified_name == fqn => Some(f.body[0].clone()),
                    _ => None,
                })
                .expect("the function");
            match body {
                VS::AppliedFunction(af) => (af.function.clone(), af.referents.to_vec()),
                other => panic!("not a call: {other:?}"),
            }
        };
        assert_eq!(call("c::g"), (j("b::f"), vec![]));
        assert_eq!(call("c::h"), (j("a::b::f"), vec![]));
    });
}

/// A runtime's connection ids resolve whatever else changed, and two binding keys that name one store keep both
/// connection lists under the first key's place (2026-11-13, the names fixes, item 3, ruling 28: the ids resolved only
/// on a rebuild, and the second key's list replaced the first's).
#[test]
fn a_runtimes_bindings_and_ids_resolve_and_merge() {
    on_parse_stack(|| {
        let m = model(concat!(
            "###Relational\nDatabase a::S ( Table T (ID INTEGER) )\n",
            "###Connection\n",
            "RelationalDatabaseConnection a::C1 { store: a::S; type: H2; specification: LocalH2 {}; auth: DefaultH2; }\n",
            "RelationalDatabaseConnection a::C2 { store: a::S; type: H2; specification: LocalH2 {}; auth: DefaultH2; }\n",
            "###Runtime\nimport a::*;\n",
            "Runtime a::R { mappings: []; connections: [ S: [ id1: a::C1 ], a::S: [ id2: a::C2 ] ]; }\n",
            "Runtime a::Q { mappings: []; connections: [ a::S: [ id3: a::C1 ] ]; }\n",
        ));
        let r = resolver::resolve(&m).unwrap();
        let runtime = |fqn: &str| {
            r.elements
                .iter()
                .find_map(|e| match e {
                    PackageableElement::RuntimeDefinition(rd) if rd.qualified_name == fqn => Some(rd.clone()),
                    _ => None,
                })
                .expect("the runtime")
        };
        let rd = runtime("a::R");
        let bindings: Vec<(String, Vec<String>)> = rd
            .connection_bindings
            .iter()
            .map(|(k, v)| (k.lossy(), v.iter().map(|c| c.lossy()).collect()))
            .collect();
        assert_eq!(
            bindings,
            [("a::S".to_owned(), vec!["a::C1".to_owned(), "a::C2".to_owned()])]
        );
        let ids: Vec<(String, String)> = rd.connection_ids.iter().map(|(k, v)| (k.lossy(), v.lossy())).collect();
        assert_eq!(
            ids,
            [
                ("id1".to_owned(), "a::S".to_owned()),
                ("id2".to_owned(), "a::S".to_owned())
            ]
        );
        // a text spells an id's store as its binding key (so they change together); a model given as protocol records
        // need not: the id's store spelled bare under a qualified key resolves too
        let elements: Vec<PackageableElement> = m
            .elements
            .iter()
            .map(|e| match e {
                PackageableElement::RuntimeDefinition(rd) if rd.qualified_name == "a::Q" => {
                    PackageableElement::RuntimeDefinition(std::sync::Arc::new(lite_model::RuntimeDefinition {
                        connection_ids: vec![(j("id3"), j("S"))].into(),
                        ..(**rd).clone()
                    }))
                }
                other => other.clone(),
            })
            .collect();
        let bare = ParsedModel {
            elements: elements.into(),
            ..m.clone()
        };
        let r2 = resolver::resolve(&bare).unwrap();
        let q = r2
            .elements
            .iter()
            .find_map(|e| match e {
                PackageableElement::RuntimeDefinition(rd) if rd.qualified_name == "a::Q" => Some(rd.clone()),
                _ => None,
            })
            .unwrap();
        assert_eq!(q.connection_ids[0].1, j("a::S"));
    });
}

/// The index's three fixes (2026-11-13, the names fixes, item 6, ruling 28): a database registered twice is last-wins
/// in every index (the first's joins and views stayed when the second had none); an association end injects through a
/// generic supertype's head too (only a bare name was climbed); a join-only view's root reads its join over the include
/// closure (only the database's own joins, else refused).
/// `refuseDuplicates` over the index (2026-11-13, the names fixes: ruling 28 placed it beside `duplicate_elements`):
/// strict, the first duplicate's `ModelException` with its FQN; tolerant, each walled under its FQN, an earlier wall
/// keeping its reason.
#[test]
fn the_index_refuses_its_duplicates_strictly_and_walls_them_tolerantly() {
    on_parse_stack(|| {
        let m = model("Class a::A {}\nClass a::A {}\nClass a::B {}\nClass a::B {}\nClass a::C {}\n");
        let ix = ModelIndex::from(&m).unwrap();
        let e = ix.refuse_duplicates(None).unwrap_err();
        assert_eq!(e.java_class(), "com.legend.error.ModelException");
        assert_eq!(e.message().unwrap(), j("Duplicated element 'a::A'"));
        match &e {
            crate::Thrown::Compile(c) => {
                assert_eq!(c.phase, Some(lite_error::Phase::Model));
                assert_eq!(c.element.as_deref(), Some(&*j("a::A").0));
            }
            other => panic!("not a compile error: {other:?}"),
        }
        let mut walls = LinkedMap::new();
        walls.put(j("a::B"), j("an earlier reason"));
        ix.refuse_duplicates(Some(&mut walls)).unwrap();
        let w: Vec<(String, String)> = walls.iter().map(|(k, v)| (k.lossy(), v.lossy())).collect();
        assert_eq!(
            w,
            [
                ("a::B".to_owned(), "an earlier reason".to_owned()),
                ("a::A".to_owned(), "Duplicated element 'a::A'".to_owned())
            ]
        );
        let clean = ModelIndex::from(&model("Class a::A {}\n")).unwrap();
        clean.refuse_duplicates(None).unwrap();
    });
}

#[test]
fn the_index_is_last_wins_climbs_generic_heads_and_reads_joins_through_includes() {
    on_parse_stack(|| {
        let m = model(concat!(
            "###Relational\n",
            "Database a::Db ( Table T (ID INTEGER) Join J (T.ID = T.ID) View V (id: T.ID) )\n",
            "Database a::Db ( Table T (ID INTEGER) )\n",
        ));
        let ix = ModelIndex::from(&m).unwrap();
        assert_eq!(ix.duplicate_elements().count(), 1);
        assert!(
            ix.find_join(Some(&j("a::Db")), &j("J")).is_none(),
            "the second definition has no join"
        );
        assert!(
            ix.find_view(&j("a::Db"), &j("V")).is_none(),
            "the second definition has no view"
        );
        let m = model(concat!(
            "Class a::B {}\nClass a::C extends a::B<String> {}\nClass a::X {}\n",
            "Association a::BX { bs: a::B[*]; x: a::X[1]; }\n",
        ));
        let ix = ModelIndex::from(&m).unwrap();
        let hit = ix.find_association_of(&j("a::C"), &j("x")).unwrap();
        assert_eq!(hit.map(|a| a.qualified_name.clone()), Some(j("a::BX")));
        let m = model(concat!(
            "###Relational\n",
            "Database a::Base ( Table P (ID INTEGER, F INTEGER) Table F (ID INTEGER, N VARCHAR(10)) ",
            "Join PF (P.F = F.ID) )\n",
            "Database a::Top ( include a::Base View V ( name: @PF | F.N ) )\n",
        ));
        let ix = ModelIndex::from(&m).unwrap();
        let v = ix.find_view(&j("a::Top"), &j("V")).unwrap().clone();
        assert_eq!(ix.view_main_table(&j("a::Top"), &v).unwrap(), j("P"));
    });
}

#[test]
fn a_query_resolves_in_the_sectionless_scope_and_against_a_models_universe() {
    on_parse_stack(|| {
        let units: Vec<u16> = "|my::Person.all()->filter(p|$p.name == 'x')".encode_utf16().collect();
        let q = lite_front::doors::spec(&units, Dialect::LegendLite).unwrap();
        let r = resolver::resolve_query(&q).unwrap();
        // a full name is itself; a bare platform call records its referents
        let VS::LambdaFunction(l) = &r else { panic!("a lambda") };
        let VS::AppliedFunction(filter) = &l.body[0] else {
            panic!("a call")
        };
        assert_eq!(filter.function, j("filter"));
        assert!(!filter.referents.is_empty());
        // against a model: a bare class name the model's imports make visible
        let m = model("Class my::Person { name: String[1]; }\n");
        let ix = ModelIndex::from(&resolver::resolve(&m).unwrap()).unwrap();
        let u = resolver::IndexUniverse { index: &ix };
        let units: Vec<u16> = "|Person.all()".encode_utf16().collect();
        let q = lite_front::doors::spec(&units, Dialect::LegendLite).unwrap();
        let mut b = lite_model::parsed_model::ImportScopeBuilder::new();
        b.add(&j("my::*")).unwrap();
        let r = resolver::resolve_query_in(&q, b.build(), &u).unwrap();
        let VS::LambdaFunction(l) = &r else { panic!("a lambda") };
        let VS::AppliedFunction(all) = &l.body[0] else {
            panic!("a call")
        };
        let VS::PackageableElementPtr(ptr) = &all.parameters[0] else {
            panic!("a pointer")
        };
        assert_eq!(ptr.full_path, j("my::Person"));
    });
}

#[test]
fn the_index_records_a_duplicate_and_answers_by_registration_order() {
    on_parse_stack(|| {
        let m = model("Class a::A {}\nClass a::B extends a::A {}\nEnum a::A { X }\n");
        let ix = ModelIndex::from(&m).unwrap();
        let dups: Vec<_> = ix.duplicate_elements().collect();
        assert_eq!(dups.len(), 1);
        assert_eq!(dups[0].1, j("Duplicated element 'a::A'"));
        let names: Vec<_> = ix.classes().map(|c| c.qualified_name.clone()).collect();
        assert_eq!(names, vec![j("a::A"), j("a::B")]);
        assert_eq!(ix.direct_subclasses(&j("a::A")), &[j("a::B")]);
        assert!(ix.has_element(&j("a::B")) && !ix.has_element(&j("a::C")));
        let k = ix.knowledge();
        assert!(k.is_subtype(&j("a::B"), &j("a::A")));
        assert!(!k.is_subtype(&j("a::A"), &j("a::B")));
        assert_eq!(k.ancestors_and_self(&j("a::B")).unwrap(), vec![j("a::B"), j("a::A")]);
        assert_eq!(k.subtree(&j("a::A")), vec![j("a::B")]);
    });
}

#[test]
fn a_schema_view_is_resolved_once_for_both_its_lists() {
    on_parse_stack(|| {
        let m = model("###Relational\nDatabase app::Db ( Schema s ( Table T (ID INT) View V ( id: T.ID ) ) )\n");
        let r = resolver::resolve(&m).unwrap();
        let ix = ModelIndex::from(&r).unwrap();
        assert!(ix.find_view(&j("app::Db"), &j("s.V")).is_some());
        assert!(
            ix.find_view(&j("app::Db"), &j("V")).is_some(),
            "a schema view is reachable bare too"
        );
        let views = ix.views_of(&j("app::Db"));
        assert_eq!(views.len(), 1);
        assert_eq!(views[0].lift.spelling, j("s.V"));
        // the column reference's own spelling: the schema-qualified table
        assert_eq!(
            ix.view_main_table(&j("app::Db"), &views[0].definition).unwrap(),
            j("s.T")
        );
        assert!(ix.find_table_definition(&j("app::Db"), &j("s.T")).is_some());
        assert!(ix.find_table_definition(&j("app::Db"), &j("T")).is_some());
    });
}

#[test]
fn an_association_qualified_property_adopts_into_its_owner() {
    on_parse_stack(|| {
        let m = model(
            "Class a::P {}\nClass a::F {}\nAssociation a::PF { p: a::P[*]; f: a::F[1]; \
             q() { $this.p->at(0) }: a::P[1]; }\n",
        );
        let r = resolver::resolve(&m).unwrap();
        let adopted = crate::knowledge::adopt_association_qualified_properties(&r, None).unwrap();
        // q returns a P, so the F end owns it
        assert_eq!(class(&adopted, "a::F").derived_properties.len(), 1);
        assert_eq!(class(&adopted, "a::P").derived_properties.len(), 0);
    });
}

/// A qualified property returning a type that names no class is refused with the type's kind in words (2026-11-13,
/// the names fixes, item 7, ruling 29): Java spelled it by the record's `toString()`, and the port refused one holding a
/// type-variable value. A name keeps its quoted spelling.
#[test]
fn a_qualified_property_returning_no_class_is_refused_by_its_kind() {
    on_parse_stack(|| {
        let refused = |text: &str| {
            let r = resolver::resolve(&model(text)).unwrap();
            let e = crate::knowledge::adopt_association_qualified_properties(&r, None).unwrap_err();
            assert_eq!(e.java_class(), "com.legend.error.ModelException");
            e.message().unwrap().lossy()
        };
        assert_eq!(
            refused(
                "Class a::P {}\nClass a::F {}\nAssociation a::PF { p: a::P[*]; f: a::F[1]; \
                 q() { [] }: {String[1]->Varchar(8)[1]}[1]; }\n"
            ),
            "association 'a::PF' qualified property 'q' returns a function type, which does not identify a unique \
             owning end"
        );
        assert_eq!(
            refused(
                "Class a::P {}\nClass a::F {}\nAssociation a::PF { p: a::P[*]; f: a::F[1]; \
                 q() { [] }: a::Other[1]; }\n"
            ),
            "association 'a::PF' qualified property 'q' returns 'a::Other', which does not identify a unique owning end"
        );
    });
}

/// The index hands out the database's own view records, one record per view whatever the lookup (PHASE3.md step 0's
/// census, class (b): the normalizer's `LiftedViews` keys its identity maps by the view record, so a lookup that handed
/// out a copy would miss there and wall the view as declared by no database).
#[test]
fn a_view_is_the_databases_own_record_from_every_lookup() {
    on_parse_stack(|| {
        let m = model(
            "###Relational\nDatabase app::Db ( Table T (ID INT) View V ( id: T.ID ) \
             Schema s ( Table U (ID INT) View W ( id: U.ID ) ) )\n",
        );
        let r = resolver::resolve(&m).unwrap();
        let PackageableElement::DatabaseDefinition(db) = &r.elements[0] else {
            panic!("a database")
        };
        let ix = ModelIndex::from(&r).unwrap();
        let views = ix.views_of(&j("app::Db"));
        assert_eq!(views.len(), 2);
        // the top-level view: the flat list's record, from findView, viewsOf and findView by schema
        let v = db.views.iter().find(|v| v.name == "V").unwrap();
        assert!(lite_base::same(ix.find_view(&j("app::Db"), &j("V")).unwrap(), v));
        assert!(lite_base::same(&views[0].definition, v));
        assert!(lite_base::same(
            ix.find_view_in_schema(&j("app::Db"), Some(&j("default")), &j("V"))
                .unwrap(),
            v
        ));
        // the schema view: its schema's record, the very one the flat list mirrors, by every spelling
        let w = &db.schemas[0].views[0];
        assert!(
            db.views.iter().any(|x| lite_base::same(x, w)),
            "the flat list mirrors the schema's record"
        );
        assert!(lite_base::same(ix.find_view(&j("app::Db"), &j("s.W")).unwrap(), w));
        assert!(lite_base::same(ix.find_view(&j("app::Db"), &j("W")).unwrap(), w));
        assert!(lite_base::same(&views[1].definition, w));
    });
}

/// The index holds the very records it is given, the boot layer's among them when they are added at the gate (the
/// census's `model.ModelBuilder.indexed.*` and F's `CheckedLayer`, class (b): the boot layer's elements are recognized
/// by identity, so a copy in a graph's index would be checked as the graph's own); a batch added again adds nothing.
#[test]
fn the_index_holds_the_boot_layers_own_records() {
    on_parse_stack(|| {
        let m = model("Class app::A {}\n");
        let mut ix = ModelIndex::from(&resolver::resolve(&m).unwrap()).unwrap();
        let boot = resolver::resolve(&crate::lattice::boot_model().unwrap()).unwrap();
        ix.add(&boot.elements).unwrap();
        let classes: Vec<_> = boot
            .elements
            .iter()
            .filter_map(|e| match e {
                PackageableElement::ClassDefinition(c) => Some(c),
                _ => None,
            })
            .collect();
        assert!(!classes.is_empty());
        for c in &classes {
            let held = ix.find_class(&c.qualified_name).expect("the boot class is indexed");
            // the boot layer declares each class once: the slot holds its own record
            assert!(
                lite_base::same(held, c),
                "{} is held as the boot layer's own record",
                c.qualified_name
            );
        }
        let functions = boot.elements.iter().filter_map(|e| e.as_function());
        for f in functions.take(50) {
            let held = ix.find_function(f.qualified_name());
            assert!(
                held.iter().any(|h| match (h, &f) {
                    (lite_model::Function::FunctionDefinition(a), lite_model::Function::FunctionDefinition(b)) =>
                        lite_base::same(a, b),
                    (
                        lite_model::Function::NativeFunctionDefinition(a),
                        lite_model::Function::NativeFunctionDefinition(b),
                    ) => lite_base::same(a, b),
                    _ => false,
                }),
                "{} is held as the boot layer's own record",
                f.qualified_name()
            );
        }
        let before: Vec<_> = ix.duplicate_elements().cloned().collect();
        let order: Vec<_> = ix.classes().map(|c| c.qualified_name.clone()).collect();
        ix.add(&boot.elements).unwrap();
        let after: Vec<_> = ix.duplicate_elements().cloned().collect();
        assert_eq!(before, after, "the same records again are no duplicates");
        assert_eq!(
            order,
            ix.classes().map(|c| c.qualified_name.clone()).collect::<Vec<_>>()
        );
    });
}

fn database<'a>(m: &'a ParsedModel, fqn: &str) -> &'a std::sync::Arc<lite_model::DatabaseDefinition> {
    m.elements
        .iter()
        .find_map(|e| match e {
            PackageableElement::DatabaseDefinition(d) if d.qualified_name == fqn => Some(d),
            _ => None,
        })
        .expect("the database")
}

/// `columnKind`'s walk keys each step on its (database, table, column) triple (2026-11-13, the names fixes, item 13,
/// ruling 28): a schema view `S.V`'s column `c` reading a top-level view `S`'s column `V.c` is two steps, where the
/// joined text `a::Db@S.V.c` made them one and the walk answered nothing. A text cannot name that column (a dotted
/// name is quoted, and a definition keeps its quotes where a reference drops them): a model given as protocol records
/// can, so the test renames the column as such a model would carry it.
#[test]
fn the_column_kind_walk_keys_each_step_on_its_triple() {
    on_parse_stack(|| {
        let parsed = model(concat!(
            "###Relational\n",
            "Database a::Db (\n",
            "  Table T (ID INTEGER PRIMARY KEY, X VARCHAR(10))\n",
            "  View S (Vc: T.X)\n",
            "  Schema S ( View V (c: default.S.\"V.c\") )\n",
            ")\n"
        ));
        let mut elements: Vec<PackageableElement> = parsed.elements.to_vec();
        for el in elements.iter_mut() {
            if let PackageableElement::DatabaseDefinition(db) = el {
                let views: Vec<_> = db
                    .views
                    .iter()
                    .map(|v| {
                        if v.name != "S" {
                            return v.clone();
                        }
                        let cms: Vec<_> = v
                            .column_mappings
                            .iter()
                            .map(|cm| {
                                std::sync::Arc::new(lite_model::DatabaseDefinitionViewDefinitionViewColumnMapping {
                                    name: j("V.c"),
                                    ..(**cm).clone()
                                })
                            })
                            .collect();
                        std::sync::Arc::new(lite_model::DatabaseDefinitionViewDefinition {
                            column_mappings: cms.into(),
                            ..(**v).clone()
                        })
                    })
                    .collect();
                *db = std::sync::Arc::new(lite_model::DatabaseDefinition {
                    views: views.into(),
                    ..(**db).clone()
                });
            }
        }
        let m = ParsedModel {
            elements: elements.into(),
            ..parsed
        };
        let ix = ModelIndex::from(&m).unwrap();
        let k = ix.knowledge();
        let db = j("a::Db");
        assert_eq!(k.column_kind(&db, &j("S"), &j("V.c")).unwrap(), Some("String"));
        assert_eq!(
            k.column_kind(&db, &j("S.V"), &j("c")).unwrap(),
            Some("String"),
            "two triples that join alike are two steps"
        );
    });
}

/// The type inference refuses a view-on-view cycle by name (2026-11-13, the names fixes, item 8, ruling 29): a computed
/// column whose expression reaches its own view through a column reference was typed until the stack was gone (Java's
/// `StackOverflowError`, the Rust process aborted). Two reads of one column side by side are no cycle.
#[test]
fn the_type_inference_refuses_a_view_cycle_by_name() {
    on_parse_stack(|| {
        let m = model(concat!(
            "###Relational\n",
            "Database a::Db (\n",
            "  Table T (ID INTEGER PRIMARY KEY, S VARCHAR(10))\n",
            "  View P (A: concat(Q.B, 'x'))\n",
            "  View Q (B: concat(P.A, 'y'))\n",
            "  View R (C: concat(T.S, T.S), D: concat(R2.E, R2.E))\n",
            "  View R2 (E: concat(T.S, 'z'))\n",
            ")\n"
        ));
        let ix = ModelIndex::from(&m).unwrap();
        let k = ix.knowledge();
        let db = j("a::Db");
        let refusal = "view 'P' of 'a::Db' reads its own columns (cyclic view-on-view chain)";
        let e = k.column_kind(&db, &j("P"), &j("A")).unwrap_err();
        assert_eq!(e.java_class(), "com.legend.error.ModelException");
        assert_eq!(
            e.message().unwrap(),
            j("view 'Q' of 'a::Db' reads its own columns (cyclic view-on-view chain)")
        );
        let scope = Some(&**database(&m, "a::Db"));
        let q = ix.find_view(&db, &j("Q")).unwrap().clone();
        let e = relational_type_inference::infer(
            &q.column_mappings[0].expression,
            scope,
            Some(&ix as &dyn crate::StoreLookups),
        )
        .unwrap_err();
        assert_eq!(e.message().unwrap(), j(refusal));
        let arena = lite_base::QueryArena::new();
        let p = ix.find_view(&db, &j("P")).unwrap().clone();
        let e = view_signatures::of(&arena, &ix, &db, &p).unwrap_err();
        assert_eq!(
            e.message().unwrap(),
            j("view 'Q' of 'a::Db' reads its own columns (cyclic view-on-view chain)")
        );
        assert_eq!(k.column_kind(&db, &j("R"), &j("C")).unwrap(), Some("String"));
        assert_eq!(
            k.column_kind(&db, &j("R"), &j("D")).unwrap(),
            Some("String"),
            "one column read twice side by side is no cycle"
        );
    });
}

/// The store helpers the names follow-up moved here (2026-11-12): a view's signature and its columns' kinds through
/// views of views, a computed column's inferred type (two decimals widened, two sizes summed), a cyclic chain refused
/// by the signature and answered nothing by the kind. The differential holds every view of the corpus to Java's.
#[test]
fn the_store_helpers_answer_over_a_small_database() {
    on_parse_stack(|| {
        let m = model(concat!(
            "###Relational\n",
            "Database a::Db (\n",
            "  Table T (ID INTEGER PRIMARY KEY, D DECIMAL(10,2), N NUMERIC(5,4), S VARCHAR(10))\n",
            "  View V (ID: T.ID, HIGH: max(T.D), TOTAL: plus(T.D, T.N), LABEL: concat(T.S, 'xy'))\n",
            "  View W (ID: V.ID, HIGH: V.HIGH)\n",
            "  View X (A: Y.A)\n",
            "  View Y (A: X.A)\n",
            ")\n"
        ));
        let ix = ModelIndex::from(&m).unwrap();
        let k = ix.knowledge();
        let db = j("a::Db");
        assert_eq!(k.column_kind(&db, &j("T"), &j("S")).unwrap(), Some("String"));
        assert_eq!(k.column_kind(&db, &j("V"), &j("HIGH")).unwrap(), Some("Decimal"));
        assert_eq!(k.column_kind(&db, &j("W"), &j("HIGH")).unwrap(), Some("Decimal"));
        assert_eq!(
            k.column_kind(&db, &j("X"), &j("A")).unwrap(),
            None,
            "a cyclic chain of column references is seen once"
        );
        assert_eq!(k.column_kind(&db, &j("V"), &j("absent")).unwrap(), None);
        let arena = lite_base::QueryArena::new();
        let v = ix.find_view(&db, &j("V")).unwrap().clone();
        let sig = view_signatures::of(&arena, &ix, &db, &v).unwrap();
        let types: Vec<String> = sig.columns().iter().map(|c| c.to_string()).collect();
        // TOTAL is plus over a DECIMAL(10,2) and a NUMERIC(5,4): the first argument's type, a safe type over both being
        // possible (the engine's rule, 2026-11-13, S378, ruling 34; it was their safe type, DECIMAL(12,4))
        assert_eq!(
            types,
            [
                "ID:Integer[1]",
                "HIGH:Number[0..1]",
                "TOTAL:Decimal(10,2)[0..1]",
                "LABEL:String[0..1]"
            ]
        );
        let w = ix.find_view(&db, &j("W")).unwrap().clone();
        let sig = view_signatures::of(&arena, &ix, &db, &w).unwrap();
        assert_eq!(
            sig.columns()[0].to_string(),
            "ID:Integer[1]",
            "a view's column through the inner view"
        );
        let x = ix.find_view(&db, &j("X")).unwrap().clone();
        let e = view_signatures::of(&arena, &ix, &db, &x).unwrap_err();
        assert_eq!(e.java_class(), "com.legend.error.ModelException");
        assert_eq!(
            e.message().unwrap(),
            j("view 'X' of 'a::Db' reads its own columns (cyclic view-on-view chain)")
        );
        let scope = Some(&**database(&m, "a::Db"));
        let label = &v.column_mappings[3].expression;
        assert_eq!(
            relational_type_inference::infer(label, scope, Some(&ix as &dyn crate::StoreLookups)).unwrap(),
            Some(lite_model::RelationalDataType::RelationalDataTypeVarchar(
                std::sync::Arc::new(lite_model::RelationalDataTypeVarchar { size: 12 })
            ))
        );
        assert_eq!(
            relational_type_inference::infer(label, scope, None).unwrap(),
            Some(varchar(2)),
            "no lookups: 0 + 2"
        );
    });
}

fn varchar(size: i64) -> lite_model::RelationalDataType {
    lite_model::RelationalDataType::RelationalDataTypeVarchar(std::sync::Arc::new(
        lite_model::RelationalDataTypeVarchar { size },
    ))
}

/// The type inference's three engine rules (2026-11-13, the names fixes, items 9 to 11, ruling 28): a decimal is safe
/// with the integer and float kinds alone (with a string there is no safe type: it beat any type); `sub` with no typed
/// operand has no type (it defaulted to INTEGER); `max` and `min` over several arguments are the safe type over all of
/// them when one exists, else the first's (the first's alone).
#[test]
fn the_type_inference_keeps_the_engines_safe_type_rules() {
    on_parse_stack(|| {
        let m = model(concat!(
            "###Relational\n",
            "Database a::Db (\n",
            "  Table T (ID INTEGER PRIMARY KEY, D DECIMAL(10,2), S VARCHAR(10), I INTEGER)\n",
            "  View V (CD: case(equal(T.ID, 1), T.D, T.S), CI: case(equal(T.ID, 1), T.D, T.I), ",
            "SB: sub(nope(T.ID), nope(T.ID)), MX: max(T.I, T.D), MS: max(T.I, T.S), MN: min(T.D))\n",
            ")\n"
        ));
        let ix = ModelIndex::from(&m).unwrap();
        let db = j("a::Db");
        let scope = Some(&**database(&m, "a::Db"));
        let v = ix.find_view(&db, &j("V")).unwrap().clone();
        let infer = |i: usize| {
            relational_type_inference::infer(
                &v.column_mappings[i].expression,
                scope,
                Some(&ix as &dyn crate::StoreLookups),
            )
            .unwrap()
        };
        let dec = lite_model::RelationalDataType::RelationalDataTypeDecimal(std::sync::Arc::new(
            lite_model::RelationalDataTypeDecimal {
                precision: 10,
                scale: 2,
            },
        ));
        let int = lite_model::RelationalDataType::RelationalDataTypeInteger_(std::sync::Arc::new(
            lite_model::RelationalDataTypeInteger_ {},
        ));
        assert_eq!(infer(0), None, "a decimal against a string has no safe type");
        assert_eq!(infer(1), Some(dec.clone()), "a decimal beats an integer");
        assert_eq!(infer(2), None, "sub with no typed operand has no type");
        assert_eq!(infer(3), Some(dec.clone()), "max over two: their safe type");
        assert_eq!(infer(4), Some(int), "max over two with no safe type: the first's");
        assert_eq!(infer(5), Some(dec), "min over one: its own");
    });
}

/// A concatenation with an operand typed other than CHAR or VARCHAR has no type (2026-11-13, the names fixes, item 10,
/// ruling 28: the engine's one concat rule asks every typed operand to be one); an operand with no type drops out. It
/// summed the string operands' sizes and skipped the rest (`concat(INT, 'ab')` was VARCHAR(2)).
#[test]
fn a_concatenation_of_a_non_string_has_no_type() {
    on_parse_stack(|| {
        let m = model(concat!(
            "###Relational\n",
            "Database a::Db (\n",
            "  Table T (ID INTEGER PRIMARY KEY, S VARCHAR(10), I INTEGER)\n",
            "  View V (CI: concat(T.S, T.I), CN: concat(T.S, nope(T.ID)), CS: concat(T.S, 'ab'))\n",
            ")\n"
        ));
        let ix = ModelIndex::from(&m).unwrap();
        let db = j("a::Db");
        let scope = Some(&**database(&m, "a::Db"));
        let v = ix.find_view(&db, &j("V")).unwrap().clone();
        let infer = |i: usize| {
            relational_type_inference::infer(
                &v.column_mappings[i].expression,
                scope,
                Some(&ix as &dyn crate::StoreLookups),
            )
            .unwrap()
        };
        assert_eq!(infer(0), None, "an INTEGER operand leaves no type");
        assert_eq!(infer(1), Some(varchar(10)), "an operand with no type drops out");
        assert_eq!(infer(2), Some(varchar(12)));
    });
}

/// A decimal Pure cannot hold is `PrecisionDecimal`'s refusal; a nested value a `Variant`; the milestoning funnel's
/// spellings and axes.
#[test]
fn the_type_boundary_and_the_milestoning_funnel_answer_as_java() {
    use lite_model::RelationalDataType as T;
    let bad = T::RelationalDataTypeDecimal(std::sync::Arc::new(lite_model::RelationalDataTypeDecimal {
        precision: 2,
        scale: 5,
    }));
    let e = store_compiler::column_type(&bad).unwrap_err();
    assert_eq!(e.java_class(), "java.lang.IllegalArgumentException");
    assert_eq!(
        e.message().unwrap(),
        j("scale must be in [0, precision], got scale=5, precision=2")
    );
    let nested = T::RelationalDataTypeArray(std::sync::Arc::new(lite_model::RelationalDataTypeArray {
        element: varchar(3),
    }));
    assert_eq!(
        store_compiler::column_type(&nested).unwrap(),
        lite_types::Type::ClassType(lite_types::platform::VARIANT)
    );
    let temporal = j(lite_types::platform::TEMPORAL_PROFILE);
    assert_eq!(
        MilestoningStrategy::of_stereotype_or_null(&temporal, &j("bitemporal")),
        Some(MilestoningStrategy::Bitemporal)
    );
    assert_eq!(MilestoningStrategy::of_stereotype_or_null(&temporal, &j("other")), None);
    assert_eq!(
        MilestoningStrategy::of_stereotype_or_null(&j("temporal"), &j("bitemporal")),
        None
    );
    assert!(MilestoningStrategy::Bitemporal.has(Dimension::Business));
    assert!(!MilestoningStrategy::Processing.has(Dimension::Business));
    assert_eq!(
        MilestoningStrategy::of(Dimension::Processing),
        MilestoningStrategy::Processing
    );
}

/// The builder: a full name records nothing, an operator run is one collection under the infix marker, a call at
/// another arity keeps its record and joins the rule's names.
#[test]
fn the_builder_records_as_the_resolver_would() {
    on_parse_stack(|| {
        let arg = |s: &str| {
            VS::CString(std::sync::Arc::new(lite_model::protocol::CString {
                value: j(s),
                pos: None,
                multi_line: false,
            }))
        };
        let exact = calls::exact(&j("my::pkg::f"), vec![arg("a")]);
        assert!(exact.referents.is_empty() && exact.function == j("my::pkg::f"));
        let run = calls::infix_run(&j("plus"), vec![arg("a"), arg("b")]);
        assert!(run.infix && run.parameters.len() == 1 && !run.referents.is_empty());
        let built = calls::platform(&j("filter"), vec![arg("a"), arg("b")]);
        assert_eq!(
            &*built.referents,
            &*resolver::built_call_names(&j("filter"), 2).referents
        );
        let mut original = built.clone();
        original.referents = vec![j("x::filter")].into();
        let same = calls::like(&original, vec![arg("c"), arg("d")]);
        assert_eq!(&*same.referents, [j("x::filter")], "the same arity keeps the record");
        let wider = calls::like(&original, vec![arg("c"), arg("d"), arg("e")]);
        let rule = resolver::built_call_names(&j("filter"), 3).referents;
        assert_eq!(wider.referents[0], j("x::filter"));
        assert_eq!(
            wider.referents.len(),
            1 + rule.iter().filter(|r| **r != j("x::filter")).count()
        );
    });
}

/// The pieces the differential does not ask Java directly: a built call's names are the resolver's decision in the
/// sectionless scope (memoized), a derived fact is computed once per index, `default.T` is the top level's `T`.
#[test]
fn the_compositions_answer_as_their_pieces() {
    on_parse_stack(|| {
        let made = resolver::built_call_names(&j("filter"), 2);
        assert!(made.found && made.function == j("filter") && !made.referents.is_empty());
        assert_eq!(
            resolver::built_call_names(&j("filter"), 2),
            made,
            "memoized: the same answer again"
        );
        let nothing = resolver::built_call_names(&j("noSuchFunctionAnywhere"), 1);
        assert!(!nothing.found && nothing.referents.is_empty());
        assert_eq!(crate::knowledge::canonical_table(&j("default.T")), j("T"));
        assert_eq!(crate::knowledge::canonical_table(&j("s.T")), j("s.T"));
        let ix = ModelIndex::from(&model("Class a::A {}\n")).unwrap();
        let first = ix.derived(|m| m.classes().count());
        let again = ix.derived(|_| 99usize);
        assert!(
            lite_base::same(&first, &again),
            "a derived fact is computed once per index"
        );
        assert_eq!(*again, 1);
        let names = resolver::resolve_names(
            &[lite_front::doors::spec(&"String".encode_utf16().collect::<Vec<u16>>(), Dialect::LegendLite).unwrap()],
            lite_model::ImportScope::empty(),
            resolver::platform_fqns(),
        )
        .unwrap();
        assert_eq!(names.len(), 1);
    });
}

/// A primitive extension's type variables, fully (2026-11-13, the names landings, ruling 32): a rebuilt primitive
/// extension keeps them (Java's two-argument form dropped them), and every type variable's declared type -- a primitive
/// extension's and a class's -- resolves in the element's scope (neither was walked). Whether a value fits its variable
/// is F's (Java's TypeClassifier, held to the engine's four cases by NameResolverTest's twin).
#[test]
fn a_primitive_extensions_type_variables_ride_the_rebuild_and_their_types_resolve() {
    on_parse_stack(|| {
        let units: Vec<u16> = concat!(
            "import a::*;\n",
            "Primitive a::P(x:C[1]) extends a::Base\n",
            "Primitive a::Base extends Integer\n",
            "Class a::C\n",
            "{\n",
            "  x : Integer[1];\n",
            "}\n",
        )
        .encode_utf16()
        .collect();
        let m = lite_builtin::door::parse(&units, Dialect::LegendPlatform).expect("the model parses");
        let r = resolver::resolve(&m).unwrap();
        let pe = r
            .elements
            .iter()
            .find_map(|e| match e {
                PackageableElement::PrimitiveExtensionDefinition(pe) if pe.qualified_name == "a::P" => Some(pe.clone()),
                _ => None,
            })
            .expect("the extension");
        assert_eq!(pe.base_type_name, j("a::Base"), "its base resolves");
        assert_eq!(
            pe.type_variables.len(),
            1,
            "the rebuilt extension keeps its type variable"
        );
        let text = lite_front::codec::write(&pe.type_variables[0].r#type, false);
        assert!(
            text.contains("\"a::C\""),
            "and its type resolves in the element's scope: {text}"
        );
    });
}

/// S376 (2026-11-13, the names landings, ruling 32): a view column whose operation reads a column or a table its
/// database does not have is refused in the engine's words (`Can't find column 'c'`, `Can't find table 'T' in schema 'S'
/// and database 'D'`), the plain column mapping's and a nested reference alike; an aggregate is typed at any arity as
/// the engine types it (KnowledgeLayerTest's twin).
#[test]
fn a_view_reading_a_missing_column_is_refused_and_an_aggregate_is_typed_at_any_arity() {
    on_parse_stack(|| {
        let m = model(concat!(
            "###Relational\n",
            "Database a::Db (\n",
            "  Table T (ID INTEGER PRIMARY KEY, S VARCHAR(10))\n",
            "  View V1 (X: T.NOPE)\n",
            "  View V2 (X: concat(T.S, T.NOPE))\n",
            "  View V3 (X: U.ID)\n",
            "  View V4 (C0: count(), C2: count(T.ID, T.S), M2: max(T.ID, T.ID), A2: average(T.ID, T.ID))\n",
            ")\n"
        ));
        let ix = ModelIndex::from(&m).unwrap();
        let db = j("a::Db");
        let arena = lite_base::QueryArena::new();
        let signature = |name: &str| {
            let v = ix.find_view(&db, &j(name)).unwrap().clone();
            match view_signatures::of(&arena, &ix, &db, &v) {
                Ok(sig) => sig
                    .columns()
                    .iter()
                    .map(|c| c.to_string())
                    .collect::<Vec<_>>()
                    .join(", "),
                Err(e) => format!("REFUSED {}", e.message().unwrap().lossy()),
            }
        };
        assert_eq!(signature("V1"), "REFUSED Can't find column 'NOPE'");
        assert_eq!(signature("V2"), "REFUSED Can't find column 'NOPE'");
        assert_eq!(
            signature("V3"),
            "REFUSED Can't find table 'U' in schema 'default' and database 'Db'"
        );
        assert_eq!(
            signature("V4"),
            "C0:Integer[1], C2:Integer[1], M2:Integer[0..1], A2:Float[1]"
        );
    });
}

/// S378 (2026-11-13, the names landings, ruling 32): the type inference's safe type is the engine's safeTypeMap,
/// isSafeTypePossible and getSafeType exactly; count is BIGINT; substring and its family a VARCHAR sized by their first
/// argument's CHAR or VARCHAR, no type for another; sub its first argument's type when its first two share a kind, else
/// none; group_concat no rule (KnowledgeLayerTest's twin).
#[test]
fn the_type_inference_safe_type_and_rules_are_the_engines() {
    use lite_model::RelationalDataType as T;
    use std::sync::Arc;
    on_parse_stack(|| {
        let m = model(concat!(
            "###Relational\n",
            "Database a::Db (\n",
            "  Table T (ID INTEGER PRIMARY KEY, F FLOAT, B BIGINT, C CHAR(3), K CHAR(5), V VARCHAR(4), ",
            "N NUMERIC(6,2), I INTEGER)\n",
            "  View V (FI: case(equal(T.ID, 1), T.F, T.I), BF: case(equal(T.ID, 1), T.B, T.F), ",
            "CC: case(equal(T.ID, 1), T.C, T.K), CV: case(equal(T.ID, 1), T.C, T.V), ",
            "NN: case(equal(T.ID, 1), T.N, T.N), CT: count(T.ID), SC: substring(T.C, 1, 2), ",
            "SI: substring(T.I, 1, 2), SV: sub(T.V, T.V), SM: sub(T.V, T.I), GC: group_concat(T.V))\n",
            ")\n"
        ));
        let ix = ModelIndex::from(&m).unwrap();
        let db = j("a::Db");
        let scope = Some(&**database(&m, "a::Db"));
        let v = ix.find_view(&db, &j("V")).unwrap().clone();
        let infer = |i: usize| {
            relational_type_inference::infer(
                &v.column_mappings[i].expression,
                scope,
                Some(&ix as &dyn crate::StoreLookups),
            )
            .unwrap()
        };
        let varchar = |size| T::RelationalDataTypeVarchar(Arc::new(lite_model::RelationalDataTypeVarchar { size }));
        assert_eq!(
            infer(0),
            Some(T::RelationalDataTypeFloat_(Arc::new(
                lite_model::RelationalDataTypeFloat_ {}
            )))
        );
        assert_eq!(infer(1), None, "BIGINT with FLOAT: no safe type");
        assert_eq!(infer(2), Some(varchar(5)), "two CHARs: VARCHAR(5)");
        assert_eq!(infer(3), Some(varchar(4)), "CHAR(3), VARCHAR(4): VARCHAR(4)");
        assert_eq!(
            infer(4),
            Some(T::RelationalDataTypeDecimal(Arc::new(
                lite_model::RelationalDataTypeDecimal { precision: 6, scale: 2 }
            ))),
            "two NUMERICs: DECIMAL"
        );
        assert_eq!(
            infer(5),
            Some(T::RelationalDataTypeBigInt(Arc::new(
                lite_model::RelationalDataTypeBigInt {}
            )))
        );
        assert_eq!(infer(6), Some(varchar(3)), "substring of a CHAR(3)");
        assert_eq!(infer(7), None, "substring of an INTEGER: no type");
        assert_eq!(infer(8), Some(varchar(4)), "sub of two VARCHARs: the first's");
        assert_eq!(infer(9), None, "sub of a VARCHAR and an INTEGER: no rule");
        assert_eq!(infer(10), None, "group_concat: no rule");
    });
}

/// S378's remainder (2026-11-13, the names follow-ups, ruling 34): the operators, `sum`, `average`, a float literal and
/// `upper`/`lower` typed by the engine's own rules -- `plus` over one argument widens it, `plus` and `minus` over
/// several are their first argument's type when a safe type is possible, `times` the safe type of its first two,
/// `divide` and `average` DOUBLE, `sum` by its first argument's kind (a DECIMAL capped at precision 31, a FLOAT a DOUBLE,
/// no rule for a REAL), a float literal FLOAT, `upper` and `lower` no rule (a view column `Any[0..1]`)
/// (KnowledgeLayerTest's twin).
#[test]
fn the_operators_and_aggregates_are_typed_by_the_engines_rules() {
    use lite_model::RelationalDataType as T;
    use std::sync::Arc;
    on_parse_stack(|| {
        let m = model(concat!(
            "###Relational\n",
            "Database a::Db (\n",
            "  Table T (ID INTEGER PRIMARY KEY, S SMALLINT, B BIGINT, F FLOAT, D DECIMAL(40,2), N NUMERIC(6,2), ",
            "V VARCHAR(4), R REAL)\n",
            "  View V (P1: plus(T.S), P2: plus(T.ID), P3: plus(T.D), P4: plus(T.F), P5: plus(T.V), ",
            "P6: plus(T.ID, T.F), P7: plus(T.B, T.F), M1: minus(T.N, T.ID), M2: minus(T.ID), ",
            "T1: times(T.ID, T.F, T.V), T2: times(T.ID), D1: divide(T.ID, T.ID), A1: average(T.V), S1: sum(T.S), ",
            "S2: sum(T.B), S3: sum(T.D), S4: sum(T.F), L1: case(equal(T.ID, 1), 1.5, 2.5), U1: upper(T.V), ",
            "U2: lower(T.V), S5: sum(T.R))\n",
            ")\n"
        ));
        let ix = ModelIndex::from(&m).unwrap();
        let db = j("a::Db");
        let scope = Some(&**database(&m, "a::Db"));
        let v = ix.find_view(&db, &j("V")).unwrap().clone();
        let infer = |i: usize| {
            relational_type_inference::infer(
                &v.column_mappings[i].expression,
                scope,
                Some(&ix as &dyn crate::StoreLookups),
            )
            .unwrap()
        };
        let integer = Some(T::RelationalDataTypeInteger_(Arc::new(
            lite_model::RelationalDataTypeInteger_ {},
        )));
        let big_int = Some(T::RelationalDataTypeBigInt(Arc::new(
            lite_model::RelationalDataTypeBigInt {},
        )));
        let double = Some(T::RelationalDataTypeDouble_(Arc::new(
            lite_model::RelationalDataTypeDouble_ {},
        )));
        let float = Some(T::RelationalDataTypeFloat_(Arc::new(
            lite_model::RelationalDataTypeFloat_ {},
        )));
        let decimal_31_2 = Some(T::RelationalDataTypeDecimal(Arc::new(
            lite_model::RelationalDataTypeDecimal {
                precision: 31,
                scale: 2,
            },
        )));
        assert_eq!(infer(0), integer, "plus of a SMALLINT: INTEGER");
        assert_eq!(infer(1), big_int, "plus of an INTEGER: BIGINT");
        assert_eq!(infer(2), decimal_31_2, "plus of a DECIMAL(40,2): capped at 31");
        assert_eq!(infer(3), double, "plus of a FLOAT: DOUBLE");
        assert_eq!(infer(4), None, "plus of a VARCHAR: no rule");
        assert_eq!(infer(5), integer, "plus of an INTEGER and a FLOAT: the first's");
        assert_eq!(infer(6), None, "plus of a BIGINT and a FLOAT: no safe type, no rule");
        assert_eq!(
            infer(7),
            Some(T::RelationalDataTypeNumeric(Arc::new(
                lite_model::RelationalDataTypeNumeric { precision: 6, scale: 2 }
            ))),
            "minus of a NUMERIC and an INTEGER: the first's"
        );
        assert_eq!(infer(8), None, "minus of one argument: no rule");
        assert_eq!(
            infer(9),
            float,
            "times: the safe type of the first two, the third unread"
        );
        assert_eq!(infer(10), None, "times of one argument: no type");
        assert_eq!(infer(11), double, "divide: DOUBLE");
        assert_eq!(infer(12), double, "average: DOUBLE");
        assert_eq!(infer(13), integer, "sum of a SMALLINT: INTEGER");
        assert_eq!(infer(14), big_int, "sum of a BIGINT: BIGINT");
        assert_eq!(infer(15), decimal_31_2, "sum of a DECIMAL(40,2): capped at 31");
        assert_eq!(infer(16), double, "sum of a FLOAT: DOUBLE");
        assert_eq!(infer(17), float, "float literals: FLOAT");
        assert_eq!(infer(18), None, "upper: no rule");
        assert_eq!(infer(19), None, "lower: no rule");
        assert_eq!(infer(20), None, "sum of a REAL: no rule");
        let arena = lite_base::QueryArena::new();
        let sig = view_signatures::of(&arena, &ix, &db, &v).unwrap();
        assert_eq!(
            sig.columns()[18].to_string(),
            "U1:meta::pure::metamodel::type::Any[0..1]",
            "a column no rule types: Any[0..1]"
        );
        assert_eq!(sig.columns()[17].to_string(), "L1:Float[0..1]");
    });
}

/// The engine's type-inference cases (2026-11-14, the names batch: register row S378, rulings 34 and 35), one Database
/// whose every view column is one case, every arm of each rule; each answer is legend-engine 4.145.0's, asked of the
/// engine (KnowledgeLayerTest's `ENGINE_CASES` says how), `-` its EMPTY or a throw (no type here). The same text and
/// table as KnowledgeLayerTest's.
const ENGINE_CASES: &str = concat!(
    "###Relational\n",
    "Database nb::Db (\n",
    "  Table T (ID INTEGER PRIMARY KEY, I INTEGER, TI TINYINT, SI SMALLINT, B BIGINT, F FLOAT, DB DOUBLE, R REAL,\n",
    "    D DECIMAL(10,2), N NUMERIC(6,2), C CHAR(3), K CHAR(5), V VARCHAR(4), W VARCHAR(10), X VARCHAR(5000),\n",
    "    H VARCHAR(2000000000), DT DATE, TS TIMESTAMP, BT BIT)\n",
    "  View VABS (AB1: abs(T.I), AB2: abs(T.D), AB3: abs(T.V), AB4: abs(T.I, T.I), AB5: abs(nope(T.I)), AB6: abs(1),\n",
    "    AB7: abs(abs(T.F)), AB8: abs())\n",
    "  View VADD (AD1: add(T.V, T.W), AD2: add(T.C, T.V), AD3: add(T.I, T.F), AD4: add(T.I, T.V), AD5: add(T.B, T.F),\n",
    "    AD6: add(sqlNull(), T.V), AD7: add(T.D, T.I), AD8: add(T.I), AD9: add(T.I, T.I, T.V), AD10: add(nope(T.I), T.I),\n",
    "    AD11: add(T.C, T.K), AD12: add('ab', 'cde'), AD13: add(T.V, sqlNull()), AD14: add(sqlNull(), sqlNull()),\n",
    "    AD15: add(T.R, T.R), AD16: add(T.DT, T.DT))\n",
    "  View VBIT (BA1: bitAnd(T.I, T.I), BA2: bitAnd(nope(T.I), T.V), BL1: bitShiftLeft(T.I, 2), BL2: bitShiftLeft(),\n",
    "    BX1: bitXor(T.I, T.I), BX2: bitXor(T.V))\n",
    "  View VCONST (CB1: cbrt(T.I), CB2: cbrt(nope(T.I)), CV1: convertVarchar128(T.I), CV2: convertVarchar128(nope(T.I)),\n",
    "    DP1: datePart(T.TS), DP2: datePart(T.V), DM1: dayOfMonth(T.DT), DY1: dayOfYear(T.DT), FM1: firstDayOfMonth(T.DT),\n",
    "    FQ1: firstDayOfQuarter(T.DT), FW1: firstDayOfWeek(T.DT), FY1: firstDayOfYear(T.DT), FH1: firstHourOfDay(T.TS),\n",
    "    FN1: firstMinuteOfHour(T.TS), FS1: firstSecondOfMinute(T.TS), LG1: log(T.I), M51: md5(T.V), M52: md5(nope(T.I)),\n",
    "    MO1: mod(T.I, 2), MO2: mod(T.F, T.F), MN1: monthNumber(T.DT), SH1: sha256(T.V), SG1: sign(T.F),\n",
    "    SG2: sign(nope(T.I)), SN1: sin(T.I), SQ1: sqrt(T.I), WY1: weekOfYear(T.DT), FH2: firstHourOfDay())\n",
    "  View VCOAL (CO1: coalesce(T.I, T.F), CO2: coalesce(T.V, T.K), CO3: coalesce(T.I, T.V), CO4: coalesce(T.I),\n",
    "    CO5: coalesce(nope(T.I), T.I), CO6: coalesce(sqlNull(), T.I), CO7: coalesce(T.D, T.N),\n",
    "    CO8: coalesce(T.I, T.F, T.V), CO9: coalesce(), CO10: coalesce(T.R), CO11: coalesce(T.R, T.R),\n",
    "    CO12: coalesce(T.I, sqlNull()), CO13: coalesce(T.C, T.K), CO14: coalesce(T.TI, T.SI, T.B))\n",
    "  View VIF (IF1: if(equal(T.ID, 1), T.I, T.F), IF2: if(equal(T.ID, 1), T.V, T.K), IF3: if(equal(T.ID, 1), T.I, T.V),\n",
    "    IF4: if(equal(T.ID, 1), T.I), IF5: if(equal(T.ID, 1), nope(T.I), T.I), IF6: if(nope(T.I), T.I, T.I),\n",
    "    IF7: if(equal(T.ID, 1), T.D, T.I), IF8: if(equal(T.ID, 1), T.I, T.F, T.V), IF9: if(equal(T.ID, 1), T.I, sqlNull()),\n",
    "    IF10: if(equal(T.ID, 1), T.R, T.R), IF11: if(equal(T.ID, 1), T.B, T.F), ZZ: T.ID)\n",
    "  View VLPAD (LP1: lpad(T.V, 10, ' '), LP2: lpad(T.V, T.I, ' '), LP3: lpad(T.V, 'x'), LP4: lpad(T.V),\n",
    "    LP5: lpad(nope(T.I), 7), LP6: lpad(T.V, 2.5), LP7: lpad(T.V, plus(1, 2)))\n",
    "  View VREPL (RP1: replace(T.V, 'a', 'b'), RP2: replace(T.C, 'a', 'b'), RP3: replace(T.I, 'a', 'b'),\n",
    "    RP4: replace(nope(T.I), 'a', 'b'), RP5: replace('abc', 'a', 'b'), RP6: replace(T.H, 'a', 'b'), RP7: replace(),\n",
    "    RP8: replace(sqlNull(), 'a', 'b'))\n",
    "  View VLITE (LK1: like(T.V, 'a%'), PO1: position('a', T.V), CI1: charindex('a', T.V), LO1: locate('a', T.V),\n",
    "    LK2: like())\n",
    "  View VCASE (UA1: ABS(T.I), UA2: Sum(T.I), UA3: COUNT(T.I), UA4: Max(T.I), UA5: CONCAT(T.V, T.V),\n",
    "    UA6: Coalesce(T.I), UA7: abs(T.I), UA8: SUM(T.I), UA9: count(T.I), UA10: AVERAGE(T.I))\n",
    "  View VJS (JS1: joinStrings(T.V, ','), JS2: joinStrings(T.I, ','), JS3: joinStrings(T.X, ','),\n",
    "    JS4: joinStrings(nope(T.I), ','), JS5: joinStrings(T.C, ','), JS6: joinStrings(T.V), JS7: joinStrings(),\n",
    "    JS8: joinStrings(sqlNull(), ','), JS9: joinStrings(T.V, T.X))\n",
    "  View VBIG (BG1: add(T.H, T.H), BG2: concat(T.H, T.H), BG3: concat(T.H, T.H, T.H, T.H, T.H),\n",
    "    BG4: replace(replace(T.H, 'a', 'b'), 'a', 'b'), BG5: concat(replace(T.H, 'a', 'b'), T.V),\n",
    "    BG6: coalesce(replace(T.H, 'a', 'b'), T.V), BG7: joinStrings(replace(T.H, 'a', 'b'), ','),\n",
    "    BG8: if(equal(T.ID, 1), replace(T.H, 'a', 'b'), T.V), BG9: substring(replace(T.H, 'a', 'b'), 1, 2))\n",
    ")\n"
);

/// Every view column of `nb::Db` in `text`, in the order `views` names them, typed by the inference: `VIEW.COLUMN=TYPE`
/// tokens, space-separated (`-` no type); KnowledgeLayerTest's `inferenceTable`.
fn inference_table(text: &str, views: &[&str]) -> String {
    use lite_model::RelationalDataType as T;
    let m = model(text);
    let ix = ModelIndex::from(&m).unwrap();
    let db = j("nb::Db");
    let scope = Some(&**database(&m, "nb::Db"));
    let mut out: Vec<String> = Vec::new();
    for view in views {
        let v = ix.find_view(&db, &j(view)).unwrap().clone();
        for cm in v.column_mappings.iter() {
            let t =
                relational_type_inference::infer(&cm.expression, scope, Some(&ix as &dyn crate::StoreLookups)).unwrap();
            let text = match t {
                None => "-".to_string(),
                Some(T::RelationalDataTypeVarchar(v)) => format!("Varchar({})", v.size),
                Some(T::RelationalDataTypeChar_(c)) => format!("Char({})", c.size),
                Some(T::RelationalDataTypeDecimal(d)) => format!("Decimal({},{})", d.precision, d.scale),
                Some(T::RelationalDataTypeNumeric(n)) => format!("Numeric({},{})", n.precision, n.scale),
                Some(T::RelationalDataTypeInteger_(_)) => "Integer".to_string(),
                Some(T::RelationalDataTypeBigInt(_)) => "BigInt".to_string(),
                Some(T::RelationalDataTypeFloat_(_)) => "Float".to_string(),
                Some(T::RelationalDataTypeDouble_(_)) => "Double".to_string(),
                Some(T::RelationalDataTypeReal(_)) => "Real".to_string(),
                Some(T::RelationalDataTypeBit(_)) => "Bit".to_string(),
                Some(T::RelationalDataTypeDate_(_)) => "Date".to_string(),
                Some(T::RelationalDataTypeTimestamp(_)) => "Timestamp".to_string(),
                Some(T::RelationalDataTypeOther(_)) => "Other".to_string(),
                Some(other) => panic!("a kind the table does not spell: {other:?}"),
            };
            out.push(format!("{view}.{}={text}", cm.name.lossy()));
        }
    }
    out.join(" ")
}

/// The 30 rules the seeded Database texts use (2026-11-14, the names batch, item 1: register row S378, ruling 35; until
/// then no rule, no type), every arm, typed as the engine types them (`ENGINE_CASES`: the engine's own answers). RP6, a
/// VARCHAR(2000000000) replaced, is VARCHAR(4000000000), the engine's Integer (2026-11-14, the normalizer batch, item 1,
/// ruling 34: lite answered VARCHAR(-294967296) until a VARCHAR's size was an i64; S378) (KnowledgeLayerTest's twin).
#[test]
fn the_thirty_rules_the_seeds_use_are_the_engines() {
    on_parse_stack(|| {
        let expected = concat!(
            "VABS.AB1=Integer VABS.AB2=Decimal(10,2) VABS.AB3=Varchar(4) VABS.AB4=- VABS.AB5=- VABS.AB6=Integer\n",
            "VABS.AB7=Float VABS.AB8=- VADD.AD1=Varchar(14) VADD.AD2=Varchar(7) VADD.AD3=Float VADD.AD4=- VADD.AD5=-\n",
            "VADD.AD6=- VADD.AD7=Decimal(10,2) VADD.AD8=- VADD.AD9=Integer VADD.AD10=- VADD.AD11=Varchar(8)\n",
            "VADD.AD12=Varchar(5) VADD.AD13=- VADD.AD14=Other VADD.AD15=- VADD.AD16=Date VBIT.BA1=Integer\n",
            "VBIT.BA2=Integer VBIT.BL1=Integer VBIT.BL2=Integer VBIT.BX1=Integer VBIT.BX2=Integer VCONST.CB1=Double\n",
            "VCONST.CB2=Double VCONST.CV1=Varchar(128) VCONST.CV2=Varchar(128) VCONST.DP1=Date VCONST.DP2=Date\n",
            "VCONST.DM1=Integer VCONST.DY1=Integer VCONST.FM1=Date VCONST.FQ1=Date VCONST.FW1=Date VCONST.FY1=Date\n",
            "VCONST.FH1=Timestamp VCONST.FN1=Timestamp VCONST.FS1=Timestamp VCONST.LG1=Double VCONST.M51=Varchar(32)\n",
            "VCONST.M52=Varchar(32) VCONST.MO1=Integer VCONST.MO2=Integer VCONST.MN1=Integer VCONST.SH1=Varchar(64)\n",
            "VCONST.SG1=Integer VCONST.SG2=Integer VCONST.SN1=Double VCONST.SQ1=Double VCONST.WY1=Integer\n",
            "VCONST.FH2=Timestamp VCOAL.CO1=Float VCOAL.CO2=Varchar(5) VCOAL.CO3=- VCOAL.CO4=Integer VCOAL.CO5=-\n",
            "VCOAL.CO6=Integer VCOAL.CO7=Decimal(10,2) VCOAL.CO8=- VCOAL.CO9=- VCOAL.CO10=Real VCOAL.CO11=-\n",
            "VCOAL.CO12=Integer VCOAL.CO13=Varchar(5) VCOAL.CO14=BigInt VIF.IF1=Float VIF.IF2=Varchar(5) VIF.IF3=-\n",
            "VIF.IF4=- VIF.IF5=- VIF.IF6=Integer VIF.IF7=Decimal(10,2) VIF.IF8=Float VIF.IF9=Integer VIF.IF10=-\n",
            "VIF.IF11=- VIF.ZZ=Integer VLPAD.LP1=Varchar(10) VLPAD.LP2=- VLPAD.LP3=- VLPAD.LP4=- VLPAD.LP5=Varchar(7)\n",
            "VLPAD.LP6=- VLPAD.LP7=- VREPL.RP1=Varchar(8) VREPL.RP2=Varchar(6) VREPL.RP3=- VREPL.RP4=-\n",
            "VREPL.RP5=Varchar(6) VREPL.RP6=Varchar(4000000000) VREPL.RP7=- VREPL.RP8=-\n"
        );
        let expected: Vec<&str> = expected.split_whitespace().collect();
        assert_eq!(
            inference_table(
                ENGINE_CASES,
                &["VABS", "VADD", "VBIT", "VCONST", "VCOAL", "VIF", "VLPAD", "VREPL"]
            ),
            expected.join(" ")
        );
    });
}

/// The four rules only lite had (2026-11-14, the names batch, item 2: register row S378, ruling 34): like, position,
/// charindex and locate are keys of no entry of the engine's map, so no type (`ENGINE_CASES`: the engine answers EMPTY
/// for each); like was BIT here and the three INTEGER (KnowledgeLayerTest's twin).
#[test]
fn the_rules_only_lite_had_are_gone() {
    on_parse_stack(|| {
        assert_eq!(
            inference_table(ENGINE_CASES, &["VLITE"]),
            "VLITE.LK1=- VLITE.PO1=- VLITE.CI1=- VLITE.LO1=- VLITE.LK2=-"
        );
    });
}

/// A function's name matched exactly (2026-11-14, the names batch, item 3: register row S378, ruling 34), as the
/// engine's map `get` matches it: a name spelled otherwise than its key has no rule (the engine answers EMPTY), in the
/// inference and in the view signature's aggregate switch alike (KnowledgeLayerTest's twin).
#[test]
fn a_functions_name_is_matched_exactly() {
    on_parse_stack(|| {
        assert_eq!(
            inference_table(ENGINE_CASES, &["VCASE"]),
            concat!(
                "VCASE.UA1=- VCASE.UA2=- VCASE.UA3=- VCASE.UA4=- VCASE.UA5=- VCASE.UA6=- VCASE.UA7=Integer VCASE.UA8=- ",
                "VCASE.UA9=BigInt VCASE.UA10=-"
            )
        );
        let m = model(ENGINE_CASES);
        let ix = ModelIndex::from(&m).unwrap();
        let db = j("nb::Db");
        let v = ix.find_view(&db, &j("VCASE")).unwrap().clone();
        let arena = lite_base::QueryArena::new();
        let sig = view_signatures::of(&arena, &ix, &db, &v).unwrap();
        assert_eq!(
            sig.columns()[2].to_string(),
            "UA3:meta::pure::metamodel::type::Any[0..1]",
            "COUNT is no aggregate"
        );
        assert_eq!(
            sig.columns()[7].to_string(),
            "UA8:meta::pure::metamodel::type::Any[0..1]",
            "SUM is no aggregate"
        );
        assert_eq!(sig.columns()[8].to_string(), "UA9:Integer[1]", "count is");
    });
}

/// A view column read through a schema its database lacks (2026-11-14, the names batch, item 6: register row S376, ruling
/// 34) is refused in the engine's words, `Can't find schema 's9' in database 'Db'`, nested in an operation too; a schema
/// an included database declares exists, and a table it lacks is the table's refusal (KnowledgeLayerTest's twin).
#[test]
fn a_view_reading_a_missing_schema_is_refused_in_the_engines_words() {
    on_parse_stack(|| {
        let refusal = |text: &str, db: &str| {
            let m = model(text);
            let ix = ModelIndex::from(&m).unwrap();
            let db = j(db);
            let v = ix.find_view(&db, &j("V1")).unwrap().clone();
            let arena = lite_base::QueryArena::new();
            match view_signatures::of(&arena, &ix, &db, &v) {
                Ok(_) => panic!("the view compiled"),
                Err(e) => e.message().unwrap().lossy(),
            }
        };
        assert_eq!(
            refusal(
                concat!(
                    "###Relational\n",
                    "Database nb::six::Db ( Schema s1 ( Table T (ID INTEGER PRIMARY KEY, V VARCHAR(4)) )\n",
                    "  View V1 (A: s9.T.V, Z: s1.T.ID) )\n"
                ),
                "nb::six::Db"
            ),
            "Can't find schema 's9' in database 'Db'"
        );
        assert_eq!(
            refusal(
                concat!(
                    "###Relational\n",
                    "Database nb::six::Db ( Schema s1 ( Table T (ID INTEGER PRIMARY KEY, V VARCHAR(4)) )\n",
                    "  View V1 (A: concat(s1.T.V, s7.T.V), Z: s1.T.ID) )\n"
                ),
                "nb::six::Db"
            ),
            "Can't find schema 's7' in database 'Db'"
        );
        assert_eq!(
            refusal(
                concat!(
                    "###Relational\n",
                    "Database nb::six::Inner ( Schema s1 ( Table T (ID INTEGER PRIMARY KEY, V VARCHAR(4)) ) )\n",
                    "Database nb::six::Outer ( include nb::six::Inner\n",
                    "  View V1 (A: s1.T.V, B: s1.Q.V, Z: s1.T.ID) )\n"
                ),
                "nb::six::Outer"
            ),
            "Can't find table 'Q' in schema 's1' and database 'Outer'"
        );
    });
}

/// joinStrings (2026-11-14, the names batch, item 4: register row S378, ruling 34): a VARCHAR of the larger of 4000 and
/// its first argument's CHAR or VARCHAR size, no rule for another first argument (the engine's own answers); it was
/// VARCHAR(4000) whatever its arguments (KnowledgeLayerTest's twin).
#[test]
fn join_strings_is_typed_by_the_engines_rule() {
    on_parse_stack(|| {
        assert_eq!(
            inference_table(ENGINE_CASES, &["VJS"]),
            concat!(
                "VJS.JS1=Varchar(4000) VJS.JS2=- VJS.JS3=Varchar(5000) VJS.JS4=- VJS.JS5=Varchar(4000) ",
                "VJS.JS6=Varchar(4000) VJS.JS7=- VJS.JS8=- VJS.JS9=Varchar(4000)"
            )
        );
    });
}

/// Sizes past an int (2026-11-14, the normalizer batch, item 1: register row S378, ruling 34): a VARCHAR's size is the
/// engine's Integer, so every sum, doubling and maximum of sizes past 2^31 is the engine's (`ENGINE_CASES`' VBIG, asked of
/// the engine), where an i32 wrapped each (KnowledgeLayerTest's twin).
#[test]
fn varchar_sizes_past_an_int_are_the_engines() {
    on_parse_stack(|| {
        assert_eq!(
            inference_table(ENGINE_CASES, &["VBIG"]),
            concat!(
                "VBIG.BG1=Varchar(4000000000) VBIG.BG2=Varchar(4000000000) VBIG.BG3=Varchar(10000000000) ",
                "VBIG.BG4=Varchar(8000000000) VBIG.BG5=Varchar(4000000004) VBIG.BG6=Varchar(4000000000) ",
                "VBIG.BG7=Varchar(4000000000) VBIG.BG8=Varchar(4000000000) VBIG.BG9=Varchar(4000000000)"
            )
        );
    });
}

/// S376's join terminal (2026-11-13, the names follow-ups, ruling 34): a join navigation's terminal is compiled as any
/// other operation once the navigation's joins resolve, in the database of its last join, so a terminal reading a column
/// or table its database lacks refuses the view in the engine's words, plain, nested in the terminal, or with the
/// navigation inside a function; a navigation whose join is missing is left to the lift (KnowledgeLayerTest's twin).
#[test]
fn a_views_join_terminal_reading_a_missing_column_is_refused() {
    on_parse_stack(|| {
        let m = model(concat!(
            "###Relational\n",
            "Database b::Far (\n",
            "  Table T3 (ID INTEGER PRIMARY KEY, C VARCHAR(10))\n",
            "  Table T4 (ID INTEGER PRIMARY KEY)\n",
            "  Join K(T4.ID = T3.ID)\n",
            ")\n",
            "Database a::Main (\n",
            "  Table T (ID INTEGER PRIMARY KEY, S VARCHAR(10))\n",
            "  Table T2 (ID INTEGER PRIMARY KEY, C VARCHAR(10))\n",
            "  Join J(T.ID = T2.ID)\n",
            "  View V1 (X: T.ID PRIMARY KEY, Y: @J | T2.NOPE)\n",
            "  View V2 (X: T.ID PRIMARY KEY, Y: @J | U.C)\n",
            "  View V3 (X: T.ID PRIMARY KEY, Y: @J | concat(T2.C, T2.NOPE))\n",
            "  View V4 (X: T.ID PRIMARY KEY, Y: concat(T.S, @J | T2.NOPE))\n",
            "  View V5 (X: T.ID PRIMARY KEY, Y: @Nope | T2.NOPE)\n",
            "  View V6 (X: T.ID PRIMARY KEY, Y: [a::Main]@J > [b::Far]@K | T3.C)\n",
            "  View V7 (X: T.ID PRIMARY KEY, Y: [a::Main]@J > [b::Far]@K | T3.NOPE)\n",
            "  View V8 (X: T.ID PRIMARY KEY, Y: @J | T2.C)\n",
            ")\n"
        ));
        let ix = ModelIndex::from(&m).unwrap();
        let db = j("a::Main");
        let arena = lite_base::QueryArena::new();
        let signature = |name: &str| {
            let v = ix.find_view(&db, &j(name)).unwrap().clone();
            match view_signatures::of(&arena, &ix, &db, &v) {
                Ok(sig) => sig
                    .columns()
                    .iter()
                    .map(|c| c.to_string())
                    .collect::<Vec<_>>()
                    .join(", "),
                Err(e) => format!("REFUSED {}", e.message().unwrap().lossy()),
            }
        };
        assert_eq!(signature("V1"), "REFUSED Can't find column 'NOPE'");
        assert_eq!(
            signature("V2"),
            "REFUSED Can't find table 'U' in schema 'default' and database 'Main'"
        );
        assert_eq!(signature("V3"), "REFUSED Can't find column 'NOPE'");
        assert_eq!(signature("V4"), "REFUSED Can't find column 'NOPE'");
        assert_eq!(
            signature("V5"),
            "X:Integer[1], Y:meta::pure::metamodel::type::Any[0..1]",
            "a missing join is the lift's to refuse, first"
        );
        assert_eq!(
            signature("V6"),
            "X:Integer[1], Y:String[0..1]",
            "the terminal reads its last join's database"
        );
        assert_eq!(signature("V7"), "REFUSED Can't find column 'NOPE'");
        assert_eq!(signature("V8"), "X:Integer[1], Y:String[0..1]");
    });
}
