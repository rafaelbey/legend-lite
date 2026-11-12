// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! The names layer's unit tests (2026-11-12): each piece on a small model. The whole is held to the live Java's
//! answers by `tests/names_differential.rs`.

use lite_front::doors::{Dialect, on_parse_stack};
use lite_model::linked::LinkedMap;
use lite_model::protocol::ValueSpecification as VS;
use lite_model::{JStr, PackageableElement, ParsedModel};

use crate::model_index::ModelIndex;
use crate::{core_imports, resolver, symbol_table, synth_fqn};

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
        // a resolved model loses its element spans and unclaimed sections (Java's six-argument ParsedModel)
        assert!(r.unclaimed_sections.is_empty() && r.element_spans.is_empty());
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
