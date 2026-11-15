// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! `MappingValidation` (MappingValidation.java at ad86fa895): the invalid sets of a mapping BY SET ID, in declaration
//! order, each with the engine's compile-time rejection -- recorded, never thrown here (the driver applies strict /
//! module); a mapping-level error (a duplicated set id across the include closure) throws in both builds. The per-set
//! `catch (ModelException)` keeps the exception (its full message becomes a poison). The switch's `default -> {}`
//! is spelled as the two no-op kinds (census §1).

use std::collections::HashSet;
use std::sync::Arc;

use lite_error::Phase;
use lite_model::linked::LinkedMap;
use lite_model::{
    ClassMapping, ClassMappingPure, ClassMappingRelational, ClassMappingUnion, JStr, JoinChainElement,
    LegacyMappingDefinition, PropertyMapping, RelationalOperation, cat,
};
use lite_names::ModelIndex;
use lite_names::model_index::java_list;

use crate::error::Thrown;
use crate::m2m_route_guards;
use crate::mapping_normalizer;
use crate::resolved_mapping::ResolvedMapping;

/// `MappingValidation.run` (:43): set id -> its `ModelException`, declaration order.
pub fn run(r: &ResolvedMapping<'_>, model: &ModelIndex) -> Result<LinkedMap<JStr, Thrown>, Thrown> {
    let md = r.raw();
    // MAPPING-level errors the engine's compiler raises (R5): a class mapping id taken by two distinct sets across the
    // include closure. Thrown in BOTH builds -- a module build walls the whole mapping through the pre-pass's own catch
    let closure = r.closures().closure(&md.qualified_name);
    let dup_ids = closure.duplicate_ids()?;
    if !dup_ids.is_empty() {
        return Err(Thrown::model(
            Phase::Model,
            cat!(
                "Duplicated class mappings found with ID ",
                java_list(&dup_ids),
                " in mapping '",
                &md.qualified_name,
                "'"
            ),
            Some(&md.qualified_name),
        ));
    }
    // an enumeration mapping id taken twice across the closure, in the engine's words, checked next as the engine checks
    // it (MappingValidator.collectAndValidateEnumerationMappingIds; 2026-11-15, the normalizer batch, item 2, ASK 13)
    if let Some(dup) = closure.duplicate_enumeration_mapping_id()? {
        return Err(Thrown::model(
            Phase::Model,
            cat!(
                "Duplicated enumeration mappings found with ID '",
                &dup,
                "' in mapping '",
                &md.qualified_name,
                "'"
            ),
            Some(&md.qualified_name),
        ));
    }
    let mut invalid: LinkedMap<JStr, Thrown> = LinkedMap::new();
    for cm in md.class_mappings.iter() {
        let checked = match cm {
            ClassMapping::ClassMappingRelational(rcm) => {
                validate_pm_names(rcm, model, md).and_then(|()| validate_join_sources(rcm, model))
            }
            ClassMapping::ClassMappingPure(pcm) => validate_m2m_routes(pcm, model, r),
            ClassMapping::ClassMappingUnion(u) => validate_union_members(u, r),
            // Java's `default -> { }`: the two kinds nothing here validates
            ClassMapping::ClassMappingInheritance(_) | ClassMapping::ClassMappingRelationFunction(_) => Ok(()),
        };
        match checked {
            Ok(()) => {}
            // `catch (ModelException e)`: the exception kept, by the set's id
            Err(e) if e.is_model() => {
                invalid.put(ResolvedMapping::id_of(cm), e);
            }
            Err(e) => return Err(e),
        }
    }
    Ok(invalid)
}

/// `MappingValidation.validateUnionMembers` (:76): every member set is a class mapping of the mapping or its
/// includes ("Can't find class mapping").
pub fn validate_union_members(u: &ClassMappingUnion, r: &ResolvedMapping<'_>) -> Result<(), Thrown> {
    let mut ids: HashSet<JStr> = r.included_sets()?.iter().map(|(id, _)| id.clone()).collect();
    for cm in r.raw().class_mappings.iter() {
        ids.insert(ResolvedMapping::id_of(cm));
    }
    for member in u.member_set_ids.iter() {
        if !ids.contains(member) {
            return Err(Thrown::model(
                Phase::Mapping,
                cat!(
                    "Can't find class mapping '",
                    member,
                    "' in mapping '",
                    &r.raw().qualified_name,
                    "'"
                ),
                Some(&r.raw().qualified_name),
            ));
        }
    }
    Ok(())
}

/// `MappingValidation.validatePmNames` (:91): every PM names a property its class declares or inherits.
pub fn validate_pm_names(
    rcm: &ClassMappingRelational,
    model: &ModelIndex,
    md: &LegacyMappingDefinition,
) -> Result<(), Thrown> {
    let knowledge = model.knowledge();
    // `MissProbe.knownMiss`: a set of a class the model has not skips the check
    let Some(cd) = knowledge.hierarchy_class(Some(&rcm.class_name)) else {
        return Ok(());
    };
    for pm in rcm.property_mappings.iter() {
        if matches!(pm, PropertyMapping::PropertyMappingLocalProperty(_)) {
            continue;
        }
        if knowledge.property_type(Some(cd), pm.property_name())?.is_none() {
            return Err(Thrown::model(
                Phase::Model,
                cat!(
                    "PropertyMapping '",
                    pm.property_name(),
                    "' references property not declared on class '",
                    &rcm.class_name,
                    "'; mapping=",
                    &md.qualified_name
                ),
                None,
            ));
        }
    }
    Ok(())
}

/// `MappingValidation.validateJoinSources`: every join chain a property mapping walks departs from the table it is
/// at -- the first join contains the set's main table, each next join the table the previous one lands on
/// (legend-engine 4.145.0 `HelperRelationalBuilder.validatePropertyMappings`, its words: "Mapping error: the join J does
/// not contain the source table T", the table by its own name). Since 2026-11-15 (the normalizer's second batch, item
/// 1, JCE:843); a join the database lacks, a set with no main table and an ambiguous landing are left to the refusals
/// that already name them.
pub fn validate_join_sources(rcm: &ClassMappingRelational, model: &ModelIndex) -> Result<(), Thrown> {
    let main = match &rcm.main_table {
        Some(m) => Some(m.clone()),
        None => mapping_normalizer::infer_main_table_quiet(rcm)?,
    };
    let Some(main) = main else {
        return Ok(());
    };
    if rcm.source_url.is_some() {
        return Ok(());
    }
    let start = mapping_normalizer::canonical_table(&main.table);
    for pm in rcm.property_mappings.iter() {
        validate_pm_join_sources(pm, &start, model)?;
    }
    Ok(())
}

fn validate_pm_join_sources(pm: &PropertyMapping, start: &JStr, model: &ModelIndex) -> Result<(), Thrown> {
    match pm {
        PropertyMapping::PropertyMappingJoin(j) => validate_chain(&j.joins, &j.database, start, model),
        PropertyMapping::PropertyMappingJoinTerminalColumn(j) => validate_chain(&j.joins, &j.database, start, model),
        PropertyMapping::PropertyMappingExpression(e) => validate_navigations(&e.expression, start, model),
        PropertyMapping::PropertyMappingEnumeratedExpression(e) => validate_navigations(&e.expression, start, model),
        PropertyMapping::PropertyMappingEmbedded(e) => {
            for x in e.property_mappings.iter() {
                validate_pm_join_sources(x, start, model)?;
            }
            Ok(())
        }
        PropertyMapping::PropertyMappingOtherwiseEmbedded(oe) => {
            for x in oe.embedded.iter() {
                validate_pm_join_sources(x, start, model)?;
            }
            validate_pm_join_sources(&oe.fallback, start, model)
        }
        PropertyMapping::PropertyMappingLocalProperty(lp) => validate_pm_join_sources(&lp.body, start, model),
        _ => Ok(()),
    }
}

fn validate_navigations(op: &RelationalOperation, start: &JStr, model: &ModelIndex) -> Result<(), Thrown> {
    for o in op.descendants() {
        if let RelationalOperation::RelationalOperationJoinNavigation(nav) = o
            && let Some(db) = &nav.database_name
        {
            validate_chain(&nav.chain, db, start, model)?;
        }
    }
    Ok(())
}

fn validate_chain(chain: &[Arc<JoinChainElement>], db: &JStr, start: &JStr, model: &ModelIndex) -> Result<(), Thrown> {
    let mut at = start.clone();
    for hop in chain.iter() {
        let hop_db = hop.database_name.as_ref().unwrap_or(db);
        let Some(jd) = model.find_join(Some(hop_db), &hop.join_name) else {
            return Ok(());
        };
        let mut tables: Vec<JStr> = Vec::new();
        for t in jd.operation.tables() {
            let c = mapping_normalizer::canonical_table(&t);
            if !tables.contains(&c) {
                tables.push(c);
            }
        }
        if !tables.contains(&at) {
            return Err(Thrown::model(
                Phase::Mapping,
                cat!(
                    "Mapping error: the join ",
                    &hop.join_name,
                    " does not contain the source table ",
                    &bare_table(&at)
                ),
                None,
            ));
        }
        tables.retain(|t| t != &at);
        if tables.len() > 1 {
            return Ok(());
        }
        if let Some(next) = tables.first() {
            at = next.clone();
        }
    }
    Ok(())
}

/// A table's own name, its schema dropped (the engine prints `Table._name()`).
fn bare_table(table: &JStr) -> JStr {
    match lite_model::text::last_index_of(&table.0, &[u16::from(b'.')]) {
        Some(dot) => JStr::from(&table.0[dot + 1..]),
        None => table.clone(),
    }
}

/// `MappingValidation.validateM2mRoutes` (:107): every routed M2M binding names a benign route.
pub fn validate_m2m_routes(
    pcm: &Arc<ClassMappingPure>,
    model: &ModelIndex,
    md: &ResolvedMapping<'_>,
) -> Result<(), Thrown> {
    // `MissProbe.knownMiss`: none for a class the model has not
    let tgt = model.knowledge().hierarchy_class(Some(&pcm.class_name));
    for pb in pcm.property_bindings.iter() {
        m2m_route_guards::require_benign_route(pb, pcm, tgt, md, model)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::collections::HashMap;
    use std::rc::Rc;

    use lite_front::records as p;
    use lite_model::linked::list;
    use lite_model::text::j;
    use lite_model::{ClassMappingInheritance, PropertyMappingLocalProperty};

    use crate::lifted_views::LiftedViews;
    use crate::mapping_closures::MappingClosures;
    use crate::test_support::{cm, column_pm, index, mapping, model, relational, union};

    const MODEL: &str = r#"
###Pure
Class m::P
{
  name: String[1];
}

###Relational
Database m::db
(
  Table T (NAME VARCHAR(20))
)

###Mapping
Mapping m::Base
(
  *m::P[f]: Relational
  {
    ~mainTable [m::db]T
    name: [m::db]T.NAME
  }
)
Mapping m::Over
(
  include m::Base
  m::P[f]: Relational
  {
    ~mainTable [m::db]T
    name: [m::db]T.NAME
  }
)
Mapping m::Top
(
  include m::Base
)
"#;

    fn resolved<'a>(
        md: Arc<LegacyMappingDefinition>,
        closures: &'a MappingClosures<'a>,
        views: &'a LiftedViews<'a>,
    ) -> ResolvedMapping<'a> {
        let closure = closures.closure(&md.qualified_name);
        ResolvedMapping::new(
            md.clone(),
            md,
            Rc::new(HashMap::new()),
            Rc::new(LinkedMap::new()),
            closure,
            views,
        )
    }

    #[test]
    fn a_set_id_taken_twice_across_the_closure_throws_for_the_mapping() {
        let parsed = model(MODEL);
        let idx = index(&parsed);
        let closures = MappingClosures::new(&idx);
        let views = LiftedViews::new(&parsed, &idx);
        let e = run(&resolved(mapping(&parsed, "m::Over"), &closures, &views), &idx).unwrap_err();
        assert!(e.is_model());
        assert_eq!(e.phase(), Some(Phase::Model));
        assert_eq!(e.element(), Some(j("m::Over")));
        assert_eq!(
            e.message(),
            Some(j(
                "Duplicated class mappings found with ID [f (m::Base, m::Over)] in mapping 'm::Over'"
            ))
        );
    }

    #[test]
    fn each_invalid_set_is_recorded_by_its_id_in_declaration_order() {
        let parsed = model(MODEL);
        let idx = index(&parsed);
        let closures = MappingClosures::new(&idx);
        let views = LiftedViews::new(&parsed, &idx);
        let top = mapping(&parsed, "m::Top");
        let local = PropertyMapping::PropertyMappingLocalProperty(Arc::new(PropertyMappingLocalProperty {
            property_name: j("local"),
            r#type: p::TypeExpression::TypeExpressionNameRef(Box::new(p::TypeExpressionNameRef {
                name: j("String"),
                pos: None,
            })),
            multiplicity: p::Multiplicity::MultiplicityConcrete(Box::new(p::MultiplicityConcrete {
                lower_bound: 1,
                upper_bound: Some(1),
            })),
            body: column_pm("local", "T", "NAME"),
        }));
        let md = Arc::new(LegacyMappingDefinition {
            class_mappings: list(vec![
                cm(relational(
                    "m::P",
                    Some("bad"),
                    vec![local, column_pm("nope", "T", "NAME")],
                )),
                union("m::P", Some("u"), &["f", "bad", "gone"]),
                // a class the model has not: its PMs are not read
                cm(relational(
                    "m::Unknown",
                    Some("x"),
                    vec![column_pm("nope", "T", "NAME")],
                )),
                ClassMapping::ClassMappingInheritance(Arc::new(ClassMappingInheritance {
                    class_name: j("m::P"),
                    set_id: Some(j("ih")),
                    extends_set_id: None,
                    root: false,
                })),
                cm(relational("m::P", Some("good"), vec![column_pm("name", "T", "NAME")])),
            ]),
            ..(*top).clone()
        });
        let invalid = run(&resolved(md, &closures, &views), &idx).unwrap();
        let got: Vec<(JStr, Option<JStr>, Option<Phase>)> = invalid
            .iter()
            .map(|(id, e)| {
                assert!(e.is_model() && e.element().is_none() == (id == &j("bad")));
                (id.clone(), e.message(), e.phase())
            })
            .collect();
        assert_eq!(
            got,
            vec![
                (
                    j("bad"),
                    Some(j(
                        "PropertyMapping 'nope' references property not declared on class 'm::P'; mapping=m::Top"
                    )),
                    Some(Phase::Model)
                ),
                // the include's `f` and the mapping's own sets are members; `gone` is none
                (
                    j("u"),
                    Some(j("Can't find class mapping 'gone' in mapping 'm::Top'")),
                    Some(Phase::Mapping)
                ),
            ]
        );
    }

    #[test]
    fn a_join_that_leaves_the_table_its_chain_is_at_is_the_engines_refusal() {
        // legend-engine 4.145.0's validatePropertyMappings, asked: "Mapping error: the join AC does not contain the
        // source table T" (the normalizer's second batch, item 1, JCE:843, 2026-11-15)
        let text = r#"
###Pure
Class w::P { name: String[0..1]; b: w::B[0..1]; }
Class w::B { name: String[0..1]; }
###Relational
Database w::Db
(
  Table T (ID INTEGER PRIMARY KEY, NAME VARCHAR(20), BID INTEGER)
  Table A (ID INTEGER PRIMARY KEY, NAME VARCHAR(20), CID INTEGER)
  Table C (ID INTEGER PRIMARY KEY, NAME VARCHAR(20))
  Join TA(T.BID = A.ID)
  Join AC(A.CID = C.ID)
  Join TC(T.BID = C.ID)
)
###Mapping
Mapping w::M
(
  w::P[a]: Relational { ~mainTable [w::Db]T name: [w::Db]T.NAME, b: [w::Db]@AC }
  w::P[b]: Relational { ~mainTable [w::Db]T name: [w::Db]T.NAME, b: [w::Db]@TA > @TC }
  w::P[c]: Relational { ~mainTable [w::Db]T name: [w::Db]T.NAME, b: [w::Db]@TA > @AC }
  w::B: Relational { ~mainTable [w::Db]C name: [w::Db]C.NAME }
)
"#;
        let parsed = model(text);
        let idx = index(&parsed);
        let closures = MappingClosures::new(&idx);
        let views = LiftedViews::new(&parsed, &idx);
        let invalid = run(&resolved(mapping(&parsed, "w::M"), &closures, &views), &idx).unwrap();
        let reasons: Vec<(JStr, Option<JStr>)> = invalid.iter().map(|(id, e)| (id.clone(), e.message())).collect();
        assert_eq!(
            reasons,
            vec![
                (
                    j("a"),
                    Some(j("Mapping error: the join AC does not contain the source table T"))
                ),
                (
                    j("b"),
                    Some(j("Mapping error: the join TC does not contain the source table A"))
                ),
            ]
        );
    }
}
