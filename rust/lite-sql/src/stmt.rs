// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! The statements a dialect renders beside its queries: Java's `SqlDdl` and `SqlDml`, and the declared column types
//! (`SqlDdl.ColumnType`) a stored read and a table source carry too.

use crate::mir::{NameQuoting, SqlExpr, SqlQuery};
use crate::names::ColumnKind;

/// Java's `SqlDdl.ColumnType`: a column's declared store type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ColumnType {
    Plain { kind: ColumnKind },
    Sized { kind: String, size: i32 },
    Scaled { kind: String, precision: i32, scale: i32 },
}

/// Java's `SqlDdl.Column`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DdlColumn {
    pub name: String,
    pub declared_quoted: bool,
    pub r#type: ColumnType,
    pub not_null: bool,
    pub primary_key: bool,
}

/// Java's `SqlDdl`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SqlDdl {
    CreateTable {
        schema: Option<String>,
        table: String,
        quoting: NameQuoting,
        columns: Vec<DdlColumn>,
        temporary: bool,
    },
    DropTable {
        schema: Option<String>,
        table: String,
        quoting: NameQuoting,
    },
    CreateSchema {
        schema: String,
        quoted: bool,
    },
    DropSchema {
        schema: String,
        quoted: bool,
    },
}

/// Java's `SqlDml`. A `quoted_columns` is Java's `Set`, in its order.
#[derive(Debug, Clone, PartialEq)]
pub enum SqlDml {
    InsertValues {
        schema: Option<String>,
        table: String,
        quoting: NameQuoting,
        columns: Vec<String>,
        quoted_columns: Vec<String>,
        rows: Vec<Vec<SqlExpr>>,
    },
    InsertFromTable {
        schema: Option<String>,
        table: String,
        quoting: NameQuoting,
        columns: Vec<String>,
        quoted_columns: Vec<String>,
        source: String,
    },
    DeleteAll {
        schema: Option<String>,
        table: String,
        quoting: NameQuoting,
    },
    CreateTempTableAs {
        table: String,
        query: SqlQuery,
        columns: Vec<String>,
        staged: bool,
    },
    InsertSelect {
        schema: Option<String>,
        table: String,
        quoting: NameQuoting,
        columns: Vec<String>,
        quoted_columns: Vec<String>,
        query: SqlQuery,
    },
    Update {
        schema: Option<String>,
        table: String,
        quoting: NameQuoting,
        alias: String,
        stored_types: Vec<(String, ColumnType)>,
        assignments: Vec<Assignment>,
        r#where: Option<SqlExpr>,
    },
    DeleteWhere {
        schema: Option<String>,
        table: String,
        quoting: NameQuoting,
        alias: String,
        stored_types: Vec<(String, ColumnType)>,
        r#where: Option<SqlExpr>,
    },
    Merge {
        schema: Option<String>,
        table: String,
        quoting: NameQuoting,
        columns: Vec<String>,
        quoted_columns: Vec<String>,
        keys: Vec<String>,
        query: SqlQuery,
        alias: String,
        source_alias: String,
    },
}

/// Java's `SqlDml.Update.Assignment`.
#[derive(Debug, Clone, PartialEq)]
pub struct Assignment {
    pub column: String,
    pub quoted: bool,
    pub value: SqlExpr,
}
