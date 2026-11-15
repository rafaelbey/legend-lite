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
fn a_view_is_keyed_by_its_identity_and_an_unported_body_is_never_a_wall() {
    let parsed = model(MODEL);
    let idx = index(&parsed);
    let views = LiftedViews::new(&parsed, &idx);
    let listed = &idx.views_of(&j("a::db"))[0].definition;
    let found = idx.find_view(&j("a::db"), &j("PV")).expect("the view");
    assert!(
        same(listed, found),
        "findView and viewsOf hand out one Arc per view (EF_CENSUS §9.1)"
    );
    // 2026-11-14 (the fill, helper d1): the unported-body half (`views.body(found)` answering
    // `Unported("ViewRelation.viewRelationExpr")`, not memoized, `lift_all` and `functions()` empty) is gone with
    // that stub: the ported body now reaches JoinChainEmission's panicking `collectJoinNavigations` stub. Once every
    // fill is merged the body is whole; re-assert it here then (the lifted function's body, memoized once).
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
fn ported_code_over_unported_code_answers_unported() {
    let parsed = model(MODEL);
    let idx: ModelIndex = index(&parsed);
    let e = crate::normalize(&parsed, &idx, None).unwrap_err();
    assert!(e.is_unported(), "{e:?}");
}
