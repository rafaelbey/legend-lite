// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! `StoreCompiler.tableSchema` (StoreCompiler.java:30 at 18feff3fe): a database's physical table as its Pure row
//! struct, the bare relation type `#>{db.TABLE}#` is typed by. Its column types are THE store column type,
//! [`lite_names::store_compiler::column_type`] (`StoreCompiler.columnType`, moved to lite-names with its first caller,
//! E's `ViewSignatures`: the owner's ruling of 2026-11-12). `declaredType` (the SQL layer's declared type) is G's,
//! not here (EF_CENSUS §2, ruling 27).

use lite_base::Alloc;
use lite_model::DatabaseDefinitionTableDefinition;
use lite_names::Thrown;
use lite_types::RelationType;

/// `tableSchema(table)` (:30): the table's columns in declaration order, each by its bare name with its column type,
/// `[1]` when `NOT NULL` or `PRIMARY KEY`, else `[0..1]`; in `arena`. Two columns of one name are the relation type's
/// `IllegalArgumentException`, a type with no Pure spelling `columnType`'s refusal, both as Java lets them escape.
pub fn table_schema<'a, A: Alloc>(
    arena: &'a A,
    table: &DatabaseDefinitionTableDefinition,
) -> Result<&'a RelationType<'a>, Thrown> {
    unimplemented!("lite-element skeleton: StoreCompiler.tableSchema")
}
