// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! MappingNormalizer family 15 (EF_CENSUS §1.1): `~filter` -> `filter(row | cond)`, a direct filter or one mediated
//! by a join chain (its terminal slot read through the pipeline).

use std::sync::Arc;

use lite_error::Phase;
use lite_front::records as p;
use lite_front::records::ValueSpecification as VS;
use lite_model::linked::LinkedMap;
use lite_model::{
    ClassMappingRelational, DatabaseDefinitionFilterDefinition, FilterMapping, FilterMappingDirect,
    FilterMappingJoinMediated, FilterPointer, JStr, cat,
};
use lite_names::{ModelIndex, calls};

use super::alias_scope::seed_alias_scope;
use crate::error::Thrown;
use crate::join_chain_emission;
use crate::pipeline::Pipeline;
use crate::rel_op_translator::{self, TableScope};
use crate::resolved_mapping::ResolvedMapping;
use crate::view_relation;

/// `MappingNormalizer.applyFilter` (:2240): `source` filtered by the set's `~filter`.
#[allow(clippy::too_many_arguments)]
pub fn apply_filter(
    source: &p::ValueSpecification,
    rcm: &ClassMappingRelational,
    row_bind: &Arc<p::Variable>,
    main_db: &JStr,
    main_table: &JStr,
    p: &mut Pipeline<'_>,
    model: &ModelIndex,
    md: &ResolvedMapping<'_>,
) -> Result<p::ValueSpecification, Thrown> {
    match &rcm.filter {
        None => Ok(source.clone()),
        Some(FilterMapping::FilterMappingDirect(direct)) => {
            apply_direct_filter(source, rcm, row_bind, main_db, main_table, p, model, direct, md)
        }
        Some(FilterMapping::FilterMappingJoinMediated(jm)) => {
            apply_join_mediated_filter(source, rcm, row_bind, main_db, main_table, p, model, jm, md)
        }
    }
}

/// `MappingNormalizer.applyDirectFilter` (:2256).
#[allow(clippy::too_many_arguments)]
pub fn apply_direct_filter(
    source: &p::ValueSpecification,
    rcm: &ClassMappingRelational,
    row_bind: &Arc<p::Variable>,
    main_db: &JStr,
    main_table: &JStr,
    p: &mut Pipeline<'_>,
    model: &ModelIndex,
    direct: &Arc<FilterMappingDirect>,
    md: &ResolvedMapping<'_>,
) -> Result<p::ValueSpecification, Thrown> {
    let db_fqn = match &direct.filter {
        FilterPointer::FilterPointerCross(c) => c.db.clone(),
        FilterPointer::FilterPointerLocal(_) => main_db.clone(),
    };
    let fd = find_filter(model, Some(&db_fqn), &direct.filter, rcm, md)?;
    let mut scope: TableScope = LinkedMap::new();
    scope.put(main_table.clone(), VS::Variable(row_bind.clone()));
    seed_alias_scope(&mut scope, p, row_bind, main_table)?;
    // a set over a VIEW: the filter's references to the view's root table resolve through the frame; a set over a
    // TABLE whose filter names a view's columns reads the view's expressions (the base table in scope)
    let fcond = if model.find_view(main_db, main_table).is_some() {
        view_relation::frame_rewrite_if_view(&fd.condition, main_db, main_table, md, model)?
    } else {
        view_relation::inline_view_refs(&fd.condition, &db_fqn, model)?
    };
    let cond = rel_op_translator::translate(&fcond, &scope, None, Some(row_bind), p.view())?;
    Ok(filter_call(source, row_bind, cond))
}

/// `MappingNormalizer.applyJoinMediatedFilter` (:2286).
#[allow(clippy::too_many_arguments)]
pub fn apply_join_mediated_filter(
    source: &p::ValueSpecification,
    rcm: &ClassMappingRelational,
    row_bind: &Arc<p::Variable>,
    main_db: &JStr,
    main_table: &JStr,
    p: &mut Pipeline<'_>,
    model: &ModelIndex,
    jm: &Arc<FilterMappingJoinMediated>,
    md: &ResolvedMapping<'_>,
) -> Result<p::ValueSpecification, Thrown> {
    // a local filter's database is the chain's (Java's nullable `jm.sourceDb()`)
    let db_fqn: Option<JStr> = match &jm.filter {
        FilterPointer::FilterPointerCross(c) => Some(c.db.clone()),
        FilterPointer::FilterPointerLocal(_) => jm.source_db.clone(),
    };
    let fd = find_filter(model, db_fqn.as_ref(), &jm.filter, rcm, md)?;
    // the chain was emitted as join hops in pass 3: the terminal table's row is `$row.<terminalAlias>`
    let terminal_alias = join_chain_emission::slot_for(p, &jm.joins)?;
    let terminal_row = VS::AppliedProperty(Arc::new(p::AppliedProperty {
        receiver: VS::Variable(row_bind.clone()),
        property: terminal_alias.clone(),
        pos: None,
        owner_class: None,
    }));
    let mut scope: TableScope = LinkedMap::new();
    scope.put(main_table.clone(), VS::Variable(row_bind.clone()));
    seed_alias_scope(&mut scope, p, row_bind, main_table)?;
    if let Some(terminal_table) = p.alias_to_target_table.get(&terminal_alias) {
        scope.put_if_absent(terminal_table.clone(), terminal_row.clone());
    }
    let framed = view_relation::frame_rewrite_if_view(&fd.condition, main_db, main_table, md, model)?;
    let cond = rel_op_translator::translate(&framed, &scope, Some(&terminal_row), Some(row_bind), p.view())?;
    // an EXPLICIT (INNER) filter never reaches here: synthTableBackedParts builds its row-exploding source first
    Ok(filter_call(source, row_bind, cond))
}

/// `model.findFilter(db, pointer.name()).orElseThrow(..)`: the filter, or the normalize-phase refusal.
fn find_filter<'m>(
    model: &'m ModelIndex,
    db_fqn: Option<&JStr>,
    pointer: &FilterPointer,
    rcm: &ClassMappingRelational,
    md: &ResolvedMapping<'_>,
) -> Result<&'m Arc<DatabaseDefinitionFilterDefinition>, Thrown> {
    let name = match pointer {
        FilterPointer::FilterPointerCross(c) => &c.name,
        FilterPointer::FilterPointerLocal(l) => &l.name,
    };
    model.find_filter(db_fqn, name).ok_or_else(|| {
        Thrown::model(
            Phase::Normalize,
            cat!(
                "~filter '",
                name,
                "' not found in db '",
                db_fqn.cloned().unwrap_or_else(|| JStr::from("null")),
                "'; class=",
                &rcm.class_name,
                ", mapping=",
                md.qualified_name()
            ),
            None,
        )
    })
}

/// `Calls.platform("filter", [source, {row | cond}])`.
fn filter_call(source: &VS, row_bind: &Arc<p::Variable>, cond: VS) -> VS {
    VS::AppliedFunction(Arc::new(calls::platform(
        &JStr::from("filter"),
        vec![
            source.clone(),
            VS::LambdaFunction(Arc::new(p::LambdaFunction {
                parameters: vec![row_bind.clone()].into(),
                body: vec![cond].into(),
                pos: None,
            })),
        ],
    )))
}

#[cfg(test)]
mod tests {
    use std::collections::{HashMap, HashSet};
    use std::rc::Rc;

    use super::*;
    use crate::lifted_views::LiftedViews;
    use crate::mapping_closures::MappingClosures;
    use crate::mapping_ledger::MappingLedger;
    use crate::test_support::{index, legacy, model, relational};
    use lite_model::text::j;
    use lite_model::{FilterMappingDirect, FilterPointerCross, FilterPointerLocal};

    #[test]
    fn no_filter_is_the_source_and_an_unknown_one_is_loud() {
        let parsed = model("###Relational\nDatabase a::db\n(\n  Table T (ID INTEGER PRIMARY KEY)\n)\n");
        let idx = index(&parsed);
        let closures = MappingClosures::new(&idx);
        let views = LiftedViews::new(&parsed, &idx);
        let m = Arc::new(legacy("a::M", vec![]));
        let md = ResolvedMapping::new(
            m.clone(),
            m,
            Rc::new(HashMap::new()),
            Rc::new(LinkedMap::new()),
            closures.closure(&j("a::M")),
            &views,
        );
        let row = Arc::new(p::Variable {
            name: j("row"),
            r#type: None,
            multiplicity: None,
            pos: None,
        });
        let src = VS::Variable(row.clone());
        let mut ledger = MappingLedger::new(HashSet::new());
        let mut pl = Pipeline::new(src.clone(), &mut ledger, &views);
        let mut rcm = relational("a::P", None, vec![]);
        let out = apply_filter(&src, &rcm, &row, &j("a::db"), &j("T"), &mut pl, &idx, &md).unwrap();
        assert_eq!(out, src);
        rcm.filter = Some(FilterMapping::FilterMappingDirect(Arc::new(FilterMappingDirect {
            filter: FilterPointer::FilterPointerLocal(Arc::new(FilterPointerLocal { name: j("F") })),
        })));
        let e = apply_filter(&src, &rcm, &row, &j("a::db"), &j("T"), &mut pl, &idx, &md).unwrap_err();
        assert!(e.is_model());
        assert_eq!(
            e.message(),
            Some(j("~filter 'F' not found in db 'a::db'; class=a::P, mapping=a::M"))
        );
        // a local filter of a chain with no database: 'null'
        rcm.filter = Some(FilterMapping::FilterMappingJoinMediated(Arc::new(
            FilterMappingJoinMediated {
                source_db: None,
                joins: lite_model::linked::empty(),
                filter: FilterPointer::FilterPointerLocal(Arc::new(FilterPointerLocal { name: j("F") })),
                join_type: None,
            },
        )));
        let e = apply_filter(&src, &rcm, &row, &j("a::db"), &j("T"), &mut pl, &idx, &md).unwrap_err();
        assert_eq!(
            e.message(),
            Some(j("~filter 'F' not found in db 'null'; class=a::P, mapping=a::M"))
        );
        rcm.filter = Some(FilterMapping::FilterMappingDirect(Arc::new(FilterMappingDirect {
            filter: FilterPointer::FilterPointerCross(Arc::new(FilterPointerCross {
                db: j("a::other"),
                name: j("G"),
            })),
        })));
        let e = apply_filter(&src, &rcm, &row, &j("a::db"), &j("T"), &mut pl, &idx, &md).unwrap_err();
        assert_eq!(
            e.message(),
            Some(j("~filter 'G' not found in db 'a::other'; class=a::P, mapping=a::M"))
        );
    }
}
