// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! `TableIndex` (2026-11-12): one database's tables by the spellings a mapping or a join may use, built once when the
//! database is ingested. A bare name is the first table so named in the flat list, then in each named schema in order;
//! `SCHEMA.T` is that schema's table only; `default.T` a top-level table first (a database's top-level tables are schema
//! `default`), then an explicit `Schema default(...)` block's. Tabular functions are named relations reached by the
//! same spellings, a table keeping a name both declare. Names compare exactly.
//!
//! Which tabular functions are the top level's is a question of the record, not its content (PHASE3.md step 0's
//! census: `model.TableIndex.inSchemas`, class (b)): an [`IdSet`], as Java's identity set.

use std::collections::HashMap;
use std::sync::Arc;

use lite_base::IdSet;
use lite_model::{DatabaseDefinition, DatabaseDefinitionTableDefinition, JStr};

type Table = Arc<DatabaseDefinitionTableDefinition>;

/// `TableIndex`.
#[derive(Clone, Debug, Default)]
pub struct TableIndex {
    top: HashMap<JStr, Table>,
    bare: HashMap<JStr, Table>,
    by_schema: HashMap<JStr, HashMap<JStr, Table>>,
}

impl TableIndex {
    /// `TableIndex.of(db)`.
    pub fn of(db: &DatabaseDefinition) -> TableIndex {
        let mut ix = TableIndex::default();
        for t in db.tables.iter() {
            ix.bare.entry(t.name.clone()).or_insert_with(|| t.clone());
        }
        for t in db.default_schema_tables().iter() {
            ix.top.entry(t.name.clone()).or_insert_with(|| t.clone());
        }
        for s in db.schemas.iter() {
            let own = ix.by_schema.entry(s.name.clone()).or_default();
            for t in s.tables.iter() {
                own.entry(t.name.clone()).or_insert_with(|| t.clone());
                ix.bare.entry(t.name.clone()).or_insert_with(|| t.clone());
            }
        }
        let mut in_schemas: IdSet<Table> = IdSet::new();
        for s in db.schemas.iter() {
            let own = ix.by_schema.entry(s.name.clone()).or_default();
            for f in s.tabular_functions.iter() {
                own.entry(f.name.clone()).or_insert_with(|| f.clone());
                in_schemas.insert(f.clone());
            }
        }
        for f in db.tabular_functions.iter() {
            ix.bare.entry(f.name.clone()).or_insert_with(|| f.clone());
            if !in_schemas.contains(f) {
                ix.top.entry(f.name.clone()).or_insert_with(|| f.clone());
            }
        }
        ix
    }

    /// `find(name)`: the table `name` spells in this database.
    pub fn find(&self, name: &JStr) -> Option<&Table> {
        let dot = name.0.iter().position(|&u| u == u16::from(b'.'));
        let dot = match dot {
            Some(d) if d > 0 => d,
            _ => return self.bare.get(name),
        };
        let schema = JStr::from(&name.0[..dot]);
        let table = JStr::from(&name.0[dot + 1..]);
        if schema.eq_str("default")
            && let Some(hit) = self.top.get(&table)
        {
            return Some(hit);
        }
        self.by_schema.get(&schema).and_then(|own| own.get(&table))
    }
}
