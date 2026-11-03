// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! Java's `InformationSchemaCatalog`: the catalog grids (`SqlSource::Catalog`) read from the SQL standard's
//! `information_schema`, for the dialects whose databases are proven to keep it so.

use crate::refusal::Refusal;
use crate::renderer::AnsiSqlRenderer;
use crate::writer::SqlWriter;
use lite_sql::{CatalogGrid, SqlSource};

/// The system schemas no grid lists.
const NOT_SYSTEM: &str = " NOT IN ('information_schema','pg_catalog')";

/// How a database's catalog names a column's type (Java's `InformationSchemaCatalog.Types`): the expression of the
/// name its driver reports, the expression of the name its `java.sql.Types` code is read by, and each such name's
/// code, in order.
#[derive(Debug, Clone)]
pub struct Types {
    pub type_name: &'static str,
    pub type_key: &'static str,
    pub codes: &'static [(&'static str, TypeCode)],
}

/// The `java.sql.Types` codes a catalog grid reports (Java's `InformationSchemaCatalog.TypeCode`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TypeCode {
    Char,
    Numeric,
    Decimal,
    Integer,
    Smallint,
    Float,
    Real,
    Double,
    Varchar,
    Boolean,
    Date,
    Time,
    Timestamp,
    Binary,
    Varbinary,
    Bigint,
    Tinyint,
    Blob,
    TimestampWithTimezone,
}

impl TypeCode {
    /// The constant's Java `name()`.
    pub fn name(self) -> &'static str {
        match self {
            TypeCode::Char => "CHAR",
            TypeCode::Numeric => "NUMERIC",
            TypeCode::Decimal => "DECIMAL",
            TypeCode::Integer => "INTEGER",
            TypeCode::Smallint => "SMALLINT",
            TypeCode::Float => "FLOAT",
            TypeCode::Real => "REAL",
            TypeCode::Double => "DOUBLE",
            TypeCode::Varchar => "VARCHAR",
            TypeCode::Boolean => "BOOLEAN",
            TypeCode::Date => "DATE",
            TypeCode::Time => "TIME",
            TypeCode::Timestamp => "TIMESTAMP",
            TypeCode::Binary => "BINARY",
            TypeCode::Varbinary => "VARBINARY",
            TypeCode::Bigint => "BIGINT",
            TypeCode::Tinyint => "TINYINT",
            TypeCode::Blob => "BLOB",
            TypeCode::TimestampWithTimezone => "TIMESTAMP_WITH_TIMEZONE",
        }
    }

    /// The JDBC specification's number.
    pub fn code(self) -> i32 {
        match self {
            TypeCode::Char => 1,
            TypeCode::Numeric => 2,
            TypeCode::Decimal => 3,
            TypeCode::Integer => 4,
            TypeCode::Smallint => 5,
            TypeCode::Float => 6,
            TypeCode::Real => 7,
            TypeCode::Double => 8,
            TypeCode::Varchar => 12,
            TypeCode::Boolean => 16,
            TypeCode::Date => 91,
            TypeCode::Time => 92,
            TypeCode::Timestamp => 93,
            TypeCode::Binary => -2,
            TypeCode::Varbinary => -3,
            TypeCode::Bigint => -5,
            TypeCode::Tinyint => -6,
            TypeCode::Blob => 2004,
            TypeCode::TimestampWithTimezone => 2014,
        }
    }
}

/// The grid's query (`catalog` the `SqlSource::Catalog` node), written by `d` (its string literals), its types named
/// by `types`.
pub fn write<D: AnsiSqlRenderer>(d: &D, w: &mut SqlWriter, catalog: &SqlSource, types: &Types) -> Result<(), Refusal> {
    let SqlSource::Catalog { grid, schema_pattern, table_pattern, column_pattern, outputs, .. } = catalog else {
        return Err(Refusal::illegal_state("a catalog grid's query of a source that is no catalog grid"));
    };
    let projections: Vec<String> = match grid {
        CatalogGrid::Schemas => vec!["upper(schema_name)".into(), "upper(catalog_name)".into()],
        CatalogGrid::Tables => vec![
            "upper(table_catalog)".into(),
            "upper(table_schema)".into(),
            "upper(table_name)".into(),
            "table_type".into(),
            "NULL".into(),
        ],
        CatalogGrid::Columns => vec![
            "upper(table_catalog)".into(),
            "upper(table_schema)".into(),
            "upper(table_name)".into(),
            "upper(column_name)".into(),
            type_case(d, types, &|t| t.code().to_string(), "1111"),
            types.type_name.to_owned(),
            type_case(d, types, &|t| d.string_lit(t.name()), &d.string_lit("OTHER")),
        ],
        CatalogGrid::PrimaryKeys => vec![
            "NULL".into(),
            "upper(k.table_schema)".into(),
            "upper(k.table_name)".into(),
            "upper(k.column_name)".into(),
            "k.ordinal_position".into(),
            "NULL".into(),
        ],
    };
    if projections.len() != outputs.len() {
        return Err(Refusal::illegal_state(format!(
            "the {grid} catalog grid has {} columns, its source {}",
            projections.len(),
            outputs.len()
        )));
    }
    w.append("SELECT ");
    let q = d.lexicon().quote_char();
    for (i, p) in projections.iter().enumerate() {
        // each output quoted, its JDBC name's case kept on every database
        let text = format!("{}{p} AS {q}{}{q}", if i == 0 { "" } else { ", " }, outputs[i].name);
        w.append(&text);
    }
    let pattern = |w: &mut SqlWriter, expr: &str, pattern: &Option<String>| {
        if let Some(p) = pattern {
            let text = format!(" AND {expr} LIKE {}", d.string_lit(p));
            w.append(&text);
        }
    };
    match grid {
        CatalogGrid::Schemas => {
            w.append(" FROM information_schema.schemata WHERE schema_name").append(NOT_SYSTEM);
            pattern(w, "upper(schema_name)", schema_pattern);
            w.append(" ORDER BY 1");
        }
        CatalogGrid::Tables => {
            w.append(" FROM information_schema.tables WHERE table_schema").append(NOT_SYSTEM);
            pattern(w, "upper(table_schema)", schema_pattern);
            pattern(w, "upper(table_name)", table_pattern);
            w.append(" ORDER BY 2, 3");
        }
        CatalogGrid::Columns => {
            w.append(" FROM information_schema.columns WHERE table_schema").append(NOT_SYSTEM);
            pattern(w, "upper(table_schema)", schema_pattern);
            pattern(w, "upper(table_name)", table_pattern);
            pattern(w, "upper(column_name)", column_pattern);
            w.append(" ORDER BY 2, 3, ordinal_position");
        }
        // the standard key_column_usage and table_constraints pair
        CatalogGrid::PrimaryKeys => {
            w.append(
                " FROM information_schema.key_column_usage k JOIN information_schema.table_constraints t ON \
                 k.constraint_name = t.constraint_name AND k.table_schema = t.table_schema AND k.table_name = \
                 t.table_name WHERE t.constraint_type = 'PRIMARY KEY' AND k.table_schema",
            )
            .append(NOT_SYSTEM);
            pattern(w, "upper(k.table_schema)", schema_pattern);
            pattern(w, "upper(k.table_name)", table_pattern);
            w.append(" ORDER BY 2, 3, 5");
        }
    }
    Ok(())
}

/// `CASE <typeKey> WHEN <name> THEN <value> ... ELSE <otherwise> END`, a value for each type name.
fn type_case<D: AnsiSqlRenderer>(d: &D, types: &Types, value: &dyn Fn(TypeCode) -> String, otherwise: &str) -> String {
    let mut sql = format!("CASE {}", types.type_key);
    for (name, code) in types.codes {
        sql.push_str(&format!(" WHEN {} THEN {}", d.string_lit(name), value(*code)));
    }
    sql.push_str(&format!(" ELSE {otherwise} END"));
    sql
}
