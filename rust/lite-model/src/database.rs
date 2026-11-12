// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! `DatabaseDefinition`'s identity-keyed reads (docs/rust-port/PHASE3_DESIGN.md §1.1, class (b): "a default-schema
//! table is one no named schema owns; two schemas may declare equal tables"): the flat lists share their records with
//! the schemas' lists, as Java's do, so ownership is a question of the record, never of its content.

use std::sync::Arc;

use lite_base::IdSet;

use crate::linked::list;
use crate::records::{DatabaseDefinition, DatabaseDefinitionTableDefinition, DatabaseDefinitionViewDefinition};

impl DatabaseDefinition {
    /// `defaultSchemaTables()`: the tables no named schema owns, in declaration order (by identity).
    pub fn default_schema_tables(&self) -> Arc<[Arc<DatabaseDefinitionTableDefinition>]> {
        let mut owned: IdSet<Arc<DatabaseDefinitionTableDefinition>> = IdSet::new();
        for s in self.schemas.iter() {
            for t in s.tables.iter() {
                owned.insert(t.clone());
            }
        }
        list(self.tables.iter().filter(|t| !owned.contains(t)).cloned().collect())
    }

    /// `defaultSchemaViews()`: the views no named schema owns, in declaration order (by identity).
    pub fn default_schema_views(&self) -> Arc<[Arc<DatabaseDefinitionViewDefinition>]> {
        let mut owned: IdSet<Arc<DatabaseDefinitionViewDefinition>> = IdSet::new();
        for s in self.schemas.iter() {
            for v in s.views.iter() {
                owned.insert(v.clone());
            }
        }
        list(self.views.iter().filter(|v| !owned.contains(v)).cloned().collect())
    }
}
