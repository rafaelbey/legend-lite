// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! Java's `WireNames` (`core/src/main/java/com/legend/sql/dialect/WireNames.java`): the platform's wire vocabulary for
//! a result column's type, into which `SqlDialect.wireTypeName` reads a driver's own names by default -- the names that
//! say one type whichever driver gives them (the SQL type names' common synonyms, Postgres's `int8` and `_int4`,
//! Trino's `ROW(...)`, Spark's `STRING`).

use crate::java;

/// `name` in the wire vocabulary; `""` for none (Java's `fold`). Upper-cased (`toUpperCase(Locale.ROOT)`, Rust's
/// `to_uppercase`: the same special casing) and then trimmed as Java's `trim` trims.
pub fn fold(name: Option<&str>) -> String {
    let Some(name) = name else {
        return String::new();
    };
    let upper = name.to_uppercase();
    let n = java::trim(&upper);
    if n.starts_with("STRUCT") || n.starts_with("ROW(") {
        return "STRUCT".to_owned();
    }
    if n.starts_with("MAP") {
        return "MAP".to_owned();
    }
    // an array named by its element (INTEGER[]) or by its element type's internal name (_int4)
    if n.ends_with("[]") || n.starts_with("ARRAY") || n.starts_with('_') {
        return "ARRAY".to_owned();
    }
    if n == "CHARACTER VARYING" || n.starts_with("VARCHAR") {
        return "VARCHAR".to_owned();
    }
    // the same types under their other common names
    let synonym = match n {
        "INT2" => Some("SMALLINT"),
        "INT4" | "INT" => Some("INTEGER"),
        "INT8" => Some("BIGINT"),
        "FLOAT4" => Some("FLOAT"),
        "FLOAT8" => Some("DOUBLE"),
        "BOOL" => Some("BOOLEAN"),
        "TEXT" | "BPCHAR" | "NAME" | "STRING" => Some("VARCHAR"),
        "JSONB" | "VARIANT" => Some("JSON"),
        _ => None,
    };
    if let Some(s) = synonym {
        return s.to_owned();
    }
    if n.starts_with("DECIMAL") || n.starts_with("NUMERIC") {
        return n.replace("NUMERIC", "DECIMAL").replace(' ', "");
    }
    if n.starts_with("TIMESTAMP") {
        return "TIMESTAMP".to_owned();
    }
    n.to_owned()
}

#[cfg(test)]
mod tests {
    use super::fold;

    #[test]
    fn folds_as_java_does() {
        let cases = [
            (None, ""),
            (Some(""), ""),
            (Some("STRUCT(a INTEGER)"), "STRUCT"),
            (Some("row(a integer)"), "STRUCT"),
            (Some("MAP(VARCHAR, INTEGER)"), "MAP"),
            (Some("INTEGER[]"), "ARRAY"),
            (Some("_int4"), "ARRAY"),
            (Some("array<int>"), "ARRAY"),
            (Some("character varying"), "VARCHAR"),
            (Some("varchar(20)"), "VARCHAR"),
            (Some("int2"), "SMALLINT"),
            (Some("int"), "INTEGER"),
            (Some("INTEGER"), "INTEGER"),
            (Some("int8"), "BIGINT"),
            (Some("float4"), "FLOAT"),
            (Some("float8"), "DOUBLE"),
            (Some("bool"), "BOOLEAN"),
            (Some("text"), "VARCHAR"),
            (Some("bpchar"), "VARCHAR"),
            (Some("name"), "VARCHAR"),
            (Some("string"), "VARCHAR"),
            (Some("jsonb"), "JSON"),
            (Some("variant"), "JSON"),
            (Some("numeric(10, 2)"), "DECIMAL(10,2)"),
            (Some("DECIMAL(18,3)"), "DECIMAL(18,3)"),
            (Some("timestamp with time zone"), "TIMESTAMP"),
            (Some("TIMESTAMP_NS"), "TIMESTAMP"),
            (Some("  double\t\n"), "DOUBLE"),
            // trim is Java's: a control below U+0020 goes, a no-break space stays
            (Some("\u{1}date\u{1f}"), "DATE"),
            (Some("\u{a0}date"), "\u{a0}DATE"),
            // upper-cased before it is trimmed, with Java's special casing
            (Some("straße"), "STRASSE"),
            (Some("uuid"), "UUID"),
        ];
        for (name, want) in cases {
            assert_eq!(fold(name), want, "{name:?}");
        }
    }
}
