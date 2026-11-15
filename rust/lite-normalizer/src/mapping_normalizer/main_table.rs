// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! MappingNormalizer family 16 (EF_CENSUS §1.1): the closure's main-table lookups (the table an association anchors
//! on). `mainTableOrNull` skipped an ambiguous root set and answered a non-root set's table (ASK 12, ported as Java had
//! it); since 2026-11-15 a root set with no inferable table anchors nothing (the normalizer's second batch, S390).

use std::sync::Arc;

use lite_error::Phase;
use lite_model::{JStr, LegacyMappingDefinitionTableReference as TableRef, cat};
use lite_names::ModelIndex;

use super::relational::infer_main_table_quiet;
use crate::error::Thrown;
use crate::resolved_mapping::ResolvedMapping;

/// `MappingNormalizer.hasMainTable` (:2333).
pub fn has_main_table(md: &ResolvedMapping<'_>, class_fqn: &JStr, _model: &ModelIndex) -> Result<bool, Thrown> {
    Ok(main_table_or_null(md, Some(class_fqn))?.is_some())
}

/// `MappingNormalizer.mainTableOrNull` (:2344): the root set's main table across the closure, else the first set's
/// with one; none when no set has one (`inferMainTableQuiet` for a set with no `~mainTable`).
pub fn main_table_or_null(md: &ResolvedMapping<'_>, class_fqn: Option<&JStr>) -> Result<Option<Arc<TableRef>>, Thrown> {
    let mut first: Option<Arc<TableRef>> = None;
    for rcm in md.relational_sets(class_fqn) {
        let mt = match &rcm.main_table {
            Some(declared) => Some(declared.clone()),
            None => infer_main_table_quiet(&rcm)?,
        };
        // a set whose inference is ambiguous answers none and is skipped; a ROOT one (not JSON-sourced) anchors
        // nothing, so the class's association is withheld, never anchored on another set's table (since 2026-11-15, the
        // normalizer's second batch, item 3, S390: it was skipped and the first non-root set's table answered)
        let Some(mt) = mt else {
            if rcm.root && rcm.source_url.is_none() {
                return Ok(None);
            }
            continue;
        };
        if rcm.root {
            return Ok(Some(mt));
        }
        if first.is_none() {
            first = Some(mt);
        }
    }
    Ok(first)
}

/// `MappingNormalizer.mainTableDefOf` (:2364): [`main_table_or_null`], loud when none (JoinChainEmission:884's one
/// cause, RE-EXPRESSED there as `main_table_or_null(..)?.is_none()`).
pub fn main_table_def_of(
    md: &ResolvedMapping<'_>,
    class_fqn: Option<&JStr>,
    _model: &ModelIndex,
) -> Result<Arc<TableRef>, Thrown> {
    if let Some(mt) = main_table_or_null(md, class_fqn)? {
        return Ok(mt);
    }
    Err(Thrown::model(
        Phase::Normalize,
        cat!(
            "No ~mainTable for class '",
            class_fqn.cloned().unwrap_or_else(|| JStr::from("null")),
            "' in mapping=",
            md.qualified_name(),
            " (required to synthesize AssociationMapping)"
        ),
        None,
    ))
}

/// `MappingNormalizer.mainTableOf` (:2376): [`main_table_def_of`]'s table.
pub fn main_table_of(md: &ResolvedMapping<'_>, class_fqn: Option<&JStr>, model: &ModelIndex) -> Result<JStr, Thrown> {
    Ok(main_table_def_of(md, class_fqn, model)?.table.clone())
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::rc::Rc;

    use super::*;
    use crate::lifted_views::LiftedViews;
    use crate::mapping_closures::MappingClosures;
    use crate::test_support::{index, mapping, model};
    use lite_model::linked::LinkedMap;
    use lite_model::text::j;

    const MODEL: &str = r#"
###Pure
Class a::P
{
  name: String[1];
}
Class a::Q
{
  name: String[1];
}

###Relational
Database a::db
(
  Table T1 (ID INTEGER PRIMARY KEY, NAME VARCHAR(20))
  Table T2 (ID INTEGER PRIMARY KEY, NAME VARCHAR(20))
)

###Mapping
Mapping a::M
(
  a::P[p1]: Relational
  {
    ~mainTable [a::db]T1
    name: [a::db]T1.NAME
  }
  *a::P[p2]: Relational
  {
    ~mainTable [a::db]T2
    name: [a::db]T2.NAME
  }
  a::Q[q1]: Relational
  {
    ~mainTable [a::db]T2
    name: [a::db]T2.NAME
  }
  a::Q[q2]: Relational
  {
    ~mainTable [a::db]T1
    name: [a::db]T1.NAME
  }
)
"#;

    #[test]
    fn the_root_sets_table_wins_else_the_first_sets() {
        let parsed = model(MODEL);
        let idx = index(&parsed);
        let closures = MappingClosures::new(&idx);
        let views = LiftedViews::new(&parsed, &idx);
        let m = closures.surface(&mapping(&parsed, "a::M"));
        let md = ResolvedMapping::new(
            m.clone(),
            m.clone(),
            Rc::new(HashMap::new()),
            Rc::new(LinkedMap::new()),
            closures.closure(&m.qualified_name),
            &views,
        );
        assert_eq!(main_table_of(&md, Some(&j("a::P")), &idx).unwrap(), j("T2"));
        assert_eq!(main_table_of(&md, Some(&j("a::Q")), &idx).unwrap(), j("T2"));
        assert!(has_main_table(&md, &j("a::P"), &idx).unwrap());
        assert!(!has_main_table(&md, &j("a::R"), &idx).unwrap());
        let e = main_table_def_of(&md, None, &idx).unwrap_err();
        assert!(e.is_model());
        assert_eq!(
            e.message(),
            Some(j(
                "No ~mainTable for class 'null' in mapping=a::M (required to synthesize AssociationMapping)"
            ))
        );
    }
}
