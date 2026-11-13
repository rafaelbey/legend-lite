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
        assert_eq!(
            types,
            [
                "ID:Integer[1]",
                "HIGH:Number[0..1]",
                "TOTAL:Decimal(12,4)[0..1]",
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

fn varchar(size: i32) -> lite_model::RelationalDataType {
    lite_model::RelationalDataType::RelationalDataTypeVarchar(std::sync::Arc::new(
        lite_model::RelationalDataTypeVarchar { size },
    ))
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
