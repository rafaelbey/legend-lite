// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! The ported shared types over models parsed from text: the include closure ([`MappingClosures`], through
//! [`ResolvedMapping`]), the lifted views' identity memo ([`LiftedViews`]), the pipeline's view ([`PipelineView`]).

use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::sync::Arc;

use lite_base::same;
use lite_error::Phase;
use lite_front::records as p;
use lite_model::linked::LinkedMap;
use lite_model::text::j;
use lite_model::{ClassMapping, JStr, JoinChainElement, LegacyMappingDefinition};
use lite_names::ModelIndex;

use crate::lifted_views::LiftedViews;
use crate::mapping_closures::MappingClosures;
use crate::mapping_ledger::MappingLedger;
use crate::pipeline::{Pipeline, PipelineView};
use crate::resolved_mapping::ResolvedMapping;
use crate::test_support::{index, mapping, model};

const MODEL: &str = r#"
###Pure
Class a::Person
{
  name: String[1];
}
Class a::Firm
{
  name: String[1];
}

###Relational
Database a::db
(
  Table PERSON (ID INTEGER PRIMARY KEY, NAME VARCHAR(200))
  Table FIRM (ID INTEGER PRIMARY KEY, NAME VARCHAR(200))
  View PV (NAME: PERSON.NAME)
)

###Mapping
Mapping a::Base
(
  *a::Firm[f]: Relational
  {
    ~mainTable [a::db]FIRM
    name: [a::db]FIRM.NAME
  }
)
Mapping a::Top
(
  include a::Base
  a::Person[p1]: Relational
  {
    ~mainTable [a::db]PERSON
    name: [a::db]PERSON.NAME
  }
  a::Person[p2]: Relational
  {
    ~mainTable [a::db]PERSON
    name: [a::db]PERSON.NAME
  }
)
Mapping a::Over
(
  include a::Base
  a::Firm[f]: Relational
  {
    ~mainTable [a::db]FIRM
    name: [a::db]FIRM.NAME
  }
)
Mapping a::Broken
(
  include a::Nope
)
"#;

fn ids(m: &LinkedMap<JStr, ClassMapping>) -> Vec<JStr> {
    m.iter().map(|(k, _)| k.clone()).collect()
}

fn resolved<'a>(
    md: Arc<LegacyMappingDefinition>,
    closures: &'a MappingClosures<'a>,
    views: &'a LiftedViews<'a>,
) -> ResolvedMapping<'a> {
    let surface = closures.surface(&md);
    let closure = closures.closure(&surface.qualified_name);
    ResolvedMapping::new(
        surface.clone(),
        surface,
        Rc::new(HashMap::new()),
        Rc::new(LinkedMap::new()),
        closure,
        views,
    )
}

#[test]
fn a_mappings_closure_is_itself_then_its_includes() {
    let parsed = model(MODEL);
    let idx = index(&parsed);
    let closures = MappingClosures::new(&idx);
    let views = LiftedViews::new(&parsed, &idx);
    let top = resolved(mapping(&parsed, "a::Top"), &closures, &views);
    let names: Vec<JStr> = top.closure().iter().map(|m| m.qualified_name.clone()).collect();
    assert_eq!(names, vec![j("a::Top"), j("a::Base")]);
    assert_eq!(ids(top.included_sets().unwrap()), vec![j("f")]);
    assert_eq!(ids(&top.visible_sets().unwrap()), vec![j("f"), j("p1"), j("p2")]);
    // roots: the include's `*` set; two unmarked sets of one class have none
    assert_eq!(ids(top.roots()), vec![j("a::Firm")]);
    let f = top.set(Some(&j("f"))).unwrap().expect("the include's set");
    assert!(top.is_root_or_sole(&f));
    let p1 = top.set(Some(&j("p1"))).unwrap().expect("an own set");
    assert!(!top.is_root_or_sole(&p1));
    assert_eq!(top.set(None).unwrap(), None);
    assert_eq!(top.set(Some(&j("nope"))).unwrap(), None);
    assert_eq!(top.relational_sets(Some(&j("a::Firm"))).len(), 1);
    assert_eq!(top.relational_sets(Some(&j("a::Person"))).len(), 2);
    assert!(
        top.first_relational(&j("p2"))
            .is_some_and(|r| r.set_id == Some(j("p2")))
    );
    assert!(top.union_of(Some(&j("a::Person"))).is_none());
    assert_eq!(top.member_ordinal(&[j("p1"), j("p2")], Some(&j("p2"))).unwrap(), 1);
    assert_eq!(top.member_ordinal(&[j("p1")], Some(&j("q"))).unwrap(), -1);
    // the same closure state is handed out per mapping
    assert!(same(
        &closures.closure(&j("a::Top")).mappings()[0],
        &top.closure.mappings()[0]
    ));
}

#[test]
fn a_missing_include_is_loud_only_where_the_sets_are_asked() {
    let parsed = model(MODEL);
    let idx = index(&parsed);
    let closures = MappingClosures::new(&idx);
    let views = LiftedViews::new(&parsed, &idx);
    // the record builds: its roots and its mappings skip an include the model has not
    let r = resolved(mapping(&parsed, "a::Broken"), &closures, &views);
    assert_eq!(r.closure().len(), 1);
    assert!(r.roots().is_empty());
    // the own sets answer first; the included sets are walked only when asked, and loudly
    let e = r.set(Some(&j("x"))).unwrap_err();
    assert!(e.is_model());
    assert_eq!(e.phase(), Some(Phase::Mapping));
    assert_eq!(e.message(), Some(j("Can't find mapping 'a::Nope'")));
    // a failure is not cached: the next ask walks and fails again
    assert_eq!(r.included_sets().unwrap_err(), e);
    // a record the model does not hold reads its closure by its name, through the model: nothing included
    let stranger = Arc::new(LegacyMappingDefinition {
        qualified_name: j("a::Stranger"),
        ..(*mapping(&parsed, "a::Broken")).clone()
    });
    assert_eq!(resolved(stranger, &closures, &views).set(Some(&j("x"))).unwrap(), None);
}

#[test]
fn a_set_id_taken_by_an_include_and_its_includer_is_a_duplicate() {
    let parsed = model(MODEL);
    let idx = index(&parsed);
    let closures = MappingClosures::new(&idx);
    let dups = closures.closure(&j("a::Over")).duplicate_ids().unwrap();
    assert_eq!(dups, vec![j("f (a::Base, a::Over)")]);
    assert_eq!(
        closures.closure(&j("a::Top")).duplicate_ids().unwrap(),
        Vec::<JStr>::new()
    );
    let e = closures.closure(&j("a::Nope")).duplicate_ids().unwrap_err();
    assert!(e.is_model());
    assert_eq!(e.phase(), Some(Phase::Mapping));
    assert_eq!(e.message(), Some(j("Can't find mapping 'a::Nope'")));
}

#[test]
fn a_view_is_keyed_by_its_identity_and_its_body_is_memoized() {
    let parsed = model(MODEL);
    let idx = index(&parsed);
    let views = LiftedViews::new(&parsed, &idx);
    let listed = &idx.views_of(&j("a::db"))[0].definition;
    let found = idx.find_view(&j("a::db"), &j("PV")).expect("the view");
    assert!(
        same(listed, found),
        "findView and viewsOf hand out one Arc per view (EF_CENSUS §9.1)"
    );
    // the body, whole since the fill (2026-11-14; until then this half held an unported body): answered, the same
    // answer again from the memo, and one lifted function after lift_all
    let first = views.body(found).expect("the view's body");
    assert_eq!(views.body(found).expect("the memo"), first);
    views.lift_all(None).expect("lift_all");
    let fns = views.functions();
    assert_eq!(fns.len(), 1);
    assert_eq!(fns[0].qualified_name, j("a::db$view$PV"));
    // an equal view that is not the index's own: no database of the model declares it
    let stranger = Arc::new((**found).clone());
    let e = views.body(&stranger).unwrap_err();
    assert!(e.is_model());
    assert_eq!(
        e.message(),
        Some(j("view 'PV' is not declared by any database of this model"))
    );
}

#[test]
fn a_view_pipeline_has_no_ledger_and_none_is_the_empty_view() {
    let parsed = model(MODEL);
    let idx = index(&parsed);
    let views = LiftedViews::new(&parsed, &idx);
    let expr = p::ValueSpecification::CString(Arc::new(p::CString {
        value: j("x"),
        pos: None,
        multi_line: false,
    }));
    let mut vp = Pipeline::for_view(expr.clone(), &views);
    let e = vp.ledger().unwrap_err();
    assert_eq!(e.java_class(), "java.lang.IllegalStateException");
    assert_eq!(
        e.message(),
        Some(j(
            "a view pipeline carries no mapping ledger (it emits physical hops only)"
        ))
    );
    assert!(vp.ledger_mut().is_err());
    let mut ledger = MappingLedger::new(HashSet::from([j("a::Firm")]));
    let mut mp = Pipeline::new(expr, &mut ledger, &views);
    mp.alias_to_target_table.put(j("firm"), j("FIRM"));
    mp.alias_to_target_columns
        .put(j("firm"), [j("ID"), j("NAME")].into_iter().collect());
    mp.ambiguous_tables.insert(j("T"));
    assert!(mp.ledger().unwrap().is_mapped(&j("a::Firm")));
    let view = mp.view();
    assert!(view.has_slots() && view.is_ambiguous(&j("T")) && !view.is_ambiguous(&j("U")));
    assert_eq!(view.target_table(Some(&j("firm"))), Some(&j("FIRM")));
    assert_eq!(view.target_table(None), None);
    assert!(view.target_has_column(Some(&j("firm")), &j("name")));
    assert!(!view.target_has_column(Some(&j("firm")), &j("FIRM_ID")));
    let none: Option<&Pipeline<'_>> = None;
    assert!(!none.has_slots() && !none.is_ambiguous(&j("T")) && none.target_table(Some(&j("firm"))).is_none());
    assert!(!none.target_has_column(Some(&j("firm")), &j("ID")));
    let chain: Vec<Arc<JoinChainElement>> = Vec::new();
    assert_eq!(
        none.slot_for(&chain).unwrap_err().message(),
        Some(j("no pipeline slots in this context"))
    );
}

#[test]
fn the_shared_model_normalizes_with_every_body_ported() {
    // the skeleton's "unported over unported" check, after the fill (2026-11-14): every body is ported, so the
    // shared model answers -- its duplicate set id `f` across the include, refused as Java refuses it (the
    // differential holds every answer to Java's)
    let parsed = model(MODEL);
    let idx: ModelIndex = index(&parsed);
    let e = crate::normalize(&parsed, &idx, None).unwrap_err();
    assert!(e.is_model(), "{e:?}");
    assert_eq!(
        e.message(),
        Some(j(
            "Duplicated class mappings found with ID [f (a::Base, a::Over)] in mapping 'a::Over'"
        ))
    );
}

/// An enumeration mapping id taken twice across a mapping's closure is refused in the engine's words, and a value the
/// enumeration has not is the engine's "Can't find enum value" (2026-11-15, the normalizer batch, item 2, census ASK 13;
/// NormalizerBatchTest's twin).
#[test]
fn enumeration_mappings_are_refused_in_the_engines_words() {
    let twice = model(concat!(
        "Enum w::E { A, B }\n",
        "Class w::P { e: w::E[0..1]; }\n",
        "###Relational\n",
        "Database w::Db ( Table T (ID INTEGER PRIMARY KEY, C VARCHAR(5)) )\n",
        "###Mapping\n",
        "Mapping w::Base ( w::E: EnumerationMapping em { A: 'a', B: 'b' } )\n",
        "###Mapping\n",
        "Mapping w::M (\n",
        "  include w::Base\n",
        "  w::E: EnumerationMapping em { A: 'x', B: 'y' }\n",
        "  w::P: Relational { ~mainTable [w::Db]T e: EnumerationMapping em: [w::Db]T.C }\n",
        ")\n"
    ));
    let e = crate::normalize(&twice, &index(&twice), None).unwrap_err();
    assert!(e.is_model());
    assert_eq!(e.phase(), Some(Phase::Model));
    assert_eq!(e.element(), Some(j("w::M")));
    assert_eq!(
        e.message(),
        Some(j(
            "Duplicated enumeration mappings found with ID 'em' in mapping 'w::M'"
        ))
    );
    let value = model(concat!(
        "Enum w::E { A, B }\n",
        "Class w::P { e: w::E[0..1]; }\n",
        "###Relational\n",
        "Database w::Db ( Table T (ID INTEGER PRIMARY KEY, C VARCHAR(5)) )\n",
        "###Mapping\n",
        "Mapping w::M (\n",
        "  w::E: EnumerationMapping { A: 'x', B: 'y', C: 'z' }\n",
        "  w::P: Relational { ~mainTable [w::Db]T e: EnumerationMapping w_E: [w::Db]T.C }\n",
        ")\n"
    ));
    let e = crate::normalize(&value, &index(&value), None).unwrap_err();
    assert_eq!(e.message(), Some(j("Can't find enum value 'C' in enumeration 'w::E'")));
}

/// A view's join its database has not is refused in the engine's words, the database's own name (2026-11-15, the
/// normalizer batch, item 5, S376; NormalizerBatchTest's twin).
#[test]
fn a_views_missing_join_is_the_engines_refusal() {
    let m = model(concat!(
        "Class w::P { a: String[0..1]; }\n",
        "###Relational\n",
        "Database w::Db (\n",
        "  Table T (ID INTEGER PRIMARY KEY, NAME VARCHAR(20), PID INTEGER)\n",
        "  Table T2 (ID INTEGER PRIMARY KEY, NAME VARCHAR(20))\n",
        "  View V (ID: T.ID PRIMARY KEY, A: @Nope | T2.NAME)\n",
        ")\n",
        "###Mapping\n",
        "Mapping w::M ( w::P: Relational { ~mainTable [w::Db]V a: [w::Db]V.A } )\n"
    ));
    let e = crate::normalize(&m, &index(&m), None).unwrap_err();
    assert!(e.is_model());
    assert_eq!(e.message(), Some(j("Can't find join 'Nope' in database 'Db'")));
}

/// The clean-sheet door's include walk is `MappingClosures`' one walk (2026-11-15, the element slice, step 0): an
/// included mapping the model has not is skipped by that door, where a legacy includer's walk refuses it -- the answers
/// the base gave (MappingNormalizerTest's `aCleanSheetIncludersWalkSkipsAMissingIncludeWhereTheLegacyWalkRefusesIt`).
#[test]
fn a_clean_sheet_includers_walk_skips_a_missing_include_where_the_legacy_walk_refuses_it() {
    let text = |base_include: &str, own_include: &str| {
        let mut s = String::from(concat!(
            "Enum w::E { A, B }\n",
            "Class w::P { e: w::E[0..1]; }\n",
            "function w::pf(): w::P[*]\n",
            "{\n",
            "  #>{w::Db.T}# -> map(r | ^w::P(e = if($r.CODE == 'a', |w::E.A, |w::E.B)))\n",
            "}\n",
            "###Relational\n",
            "Database w::Db ( Table T (ID INTEGER PRIMARY KEY, CODE VARCHAR(20)) )\n",
            "###Mapping\n",
            "Mapping w::Base (\n",
        ));
        s.push_str(base_include);
        s.push_str("  w::E: EnumerationMapping EM { A: 'a', B: 'b' }\n)\nMapping w::M (\n  include w::Base\n");
        s.push_str(own_include);
        s.push_str("  *w::P: Relational { w::pf }\n)\n");
        model(&s)
    };
    let enums_of_m = |nm: &lite_model::NormalizedModel| -> Vec<JStr> {
        nm.elements
            .iter()
            .find_map(|el| match el {
                lite_model::PackageableElement::MappingDefinition(m) if m.qualified_name == j("w::M") => Some(
                    m.enumeration_mappings
                        .iter()
                        .map(|em| em.mapping_id.clone().expect("an id"))
                        .collect(),
                ),
                _ => None,
            })
            .expect("w::M compiled")
    };
    let direct = text("", "  include w::Missing\n");
    let nm =
        crate::normalize(&direct, &index(&direct), None).expect("a clean-sheet includer's missing include skipped");
    assert_eq!(enums_of_m(&nm), vec![j("EM")]);
    let nested = text("  include w::Missing\n", "");
    let e = crate::normalize(&nested, &index(&nested), None).unwrap_err();
    assert!(e.is_model());
    assert_eq!(e.element(), Some(j("w::Base")));
    assert_eq!(e.message(), Some(j("Can't find mapping 'w::Missing'")));
    let mut walls: LinkedMap<JStr, JStr> = LinkedMap::new();
    let nm = crate::normalize(&nested, &index(&nested), Some(&mut walls)).expect("tolerant");
    assert_eq!(walls.get(&j("w::Base")), Some(&j("Can't find mapping 'w::Missing'")));
    assert_eq!(enums_of_m(&nm), vec![j("EM")]);
}
