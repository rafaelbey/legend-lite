// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! Every op of the seam through [`dispatch`] for Databricks, each answer held to what Java's `SqlDialect` answers for
//! it (`Databricks.java`'s overrides, `AnsiSqlRenderer`'s implementations, the interface's defaults naming the
//! dialect). The golden oracles hold the renders (`tests/dialect_differential.rs`).

use super::*;

const DB: &str = r#"{"dialect":"Databricks","server":null}"#;

fn ok(spec: &str, op: &str, args: &str) -> String {
    dispatch(spec, op, args).unwrap_or_else(|r| panic!("{spec} {op} {args}: {r}"))
}

fn refused(spec: &str, op: &str, args: &str) -> Refusal {
    match dispatch(spec, op, args) {
        Ok(answer) => panic!("{spec} {op} {args} answered {answer}"),
        Err(r) => r,
    }
}

fn wrap(key: &str, mir: &str) -> String {
    format!("{{\"{key}\":{mir}}}")
}

#[test]
fn facts_are_databricks() {
    assert_eq!(
        ok(DB, "facts", "{}"),
        r#"{"temporaryTables":true,"loadsRowsInBulk":false,"relational":true,"inProcess":false,"#.to_owned()
            + r#""dropsStagedTablesWithTheirTransaction":false,"needsStaticPivot":true,"serverVersions":null,"#
            + r#""inMemoryPath":null,"scriptAbort":null,"sessionSetup":["SET TIME ZONE 'UTC'","SET ANSI_MODE = true"],"#
            + r#""keysSnapshot":"NONE","resultColumnLimit":null}"#
    );
    // every server version is spelled alike; a spec's server keeps Databricks
    assert_eq!(ok(DB, "forServer", r#"{"version":"15.4"}"#), r#""Databricks""#);
    assert_eq!(dispatch(r#"{"dialect":"Databricks","server":"14"}"#, "facts", "{}").unwrap(), ok(DB, "facts", "{}"));
    assert_eq!(
        refused(r#"{"dialect":"Databricks","quoteIdentifiers":true}"#, "facts", "{}").message,
        "quoteIdentifiers and timeZone configure an engine-text dialect (EngineStyleH2, EngineStyleDB2), not Databricks"
    );
}

/// `SELECT 1 AS "b", 'it''s' AS "s"` with an offset and a limit.
const SELECT: &str = r#"{"_type":"select","projections":[{"expr":{"_type":"intLit","type":{"_type":"typed","sqlType":{"_type":"scalar","name":"BIGINT"},"nullable":false},"value":1},"alias":"b","out":null},{"expr":{"_type":"stringLit","type":{"_type":"typed","sqlType":{"_type":"scalar","name":"VARCHAR"},"nullable":false},"value":"it's"},"alias":"s","out":null}],"distinct":false,"from":{"_type":"dual"},"where":null,"groupBy":[],"having":null,"qualify":null,"orderBy":[],"limit":5,"offset":3,"outputs":[]}"#;

/// A select over raw SQL written for the engine's H2 (`query.render`).
const RAW_SELECT: &str = r#"{"_type":"select","projections":[],"distinct":false,"from":{"_type":"rawSql","sql":"select 1","alias":"_p","outputs":[],"writtenFor":"H2"},"where":null,"groupBy":[],"having":null,"qualify":null,"orderBy":[],"limit":0,"offset":null,"outputs":[]}"#;

#[test]
fn queries_render_as_databricks_writes_them() {
    // an integer literal is a BIGINT; a quote escaped by a backslash; every identifier in backticks
    assert_eq!(
        ok(DB, "query.render", &wrap("query", SELECT)),
        r#""SELECT 1L AS `b`, 'it\\'s' AS `s`\nLIMIT 5\nOFFSET 3""#
    );
    assert_eq!(
        ok(DB, "query.renderInline", &wrap("query", SELECT)),
        r#""SELECT 1L AS `b`, 'it\\'s' AS `s` LIMIT 5 OFFSET 3""#
    );
    assert_eq!(
        ok(DB, "query.renderStatement", &wrap("query", SELECT)),
        r#"{"sql":"SELECT 1L AS `b`, 'it\\'s' AS `s`\nLIMIT 5\nOFFSET 3","binds":[],"regions":[]}"#
    );
    let r = refused(DB, "query.render", &wrap("query", RAW_SELECT));
    assert_eq!(
        (r.kind(), r.message.as_str()),
        ("DIALECT_CAPABILITY", "the engine's H2 text reached Databricks, which has no translation of it")
    );
}

const CREATE_TABLE: &str = r#"{"_type":"createTable","schema":"S","table":"Trades","quoting":{"schema":false,"table":false},"columns":[{"name":"ID","declaredQuoted":false,"type":{"_type":"plain","kind":"TINYINT"},"notNull":true,"primaryKey":true},{"name":"Amt","declaredQuoted":true,"type":{"_type":"plain","kind":"VARCHAR"},"notNull":false,"primaryKey":false}],"temporary":false}"#;

const SESSION_TABLE: &str = r#"{"_type":"createTable","schema":null,"table":"legend_rows","quoting":{"schema":false,"table":false},"columns":[{"name":"ID","declaredQuoted":false,"type":{"_type":"plain","kind":"BIGINT"},"notNull":true,"primaryKey":true},{"name":"Amt","declaredQuoted":true,"type":{"_type":"plain","kind":"DOUBLE"},"notNull":false,"primaryKey":false}],"temporary":true}"#;

const INSERT_VALUES: &str = r#"{"_type":"insertValues","schema":null,"table":"legend_rows","quoting":{"schema":false,"table":false},"columns":["ID","Amt"],"quotedColumns":["Amt"],"rows":[[{"_type":"intLit","type":{"_type":"typed","sqlType":{"_type":"scalar","name":"BIGINT"},"nullable":false},"value":1},{"_type":"nullLit","type":{"_type":"bottom"}}]]}"#;

#[test]
fn ddl_and_dml_render_and_a_session_table_is_a_temporary_view() {
    // a nullable column says nothing; an unbounded VARCHAR is a STRING; every name in backticks
    let create = r#""Create Table `S`.`Trades`(`ID` TINYINT NOT NULL, `Amt` STRING, PRIMARY KEY(`ID`));""#;
    assert_eq!(ok(DB, "ddl.render", &wrap("ddl", CREATE_TABLE)), create);
    let insert = r#""INSERT INTO `legend_rows` (`ID`, `Amt`) VALUES (1L, NULL);""#;
    assert_eq!(ok(DB, "dml.render", &wrap("dml", INSERT_VALUES)), insert);
    // a session's table is one statement: a temporary view over its rows, each column cast to its declared type
    assert_eq!(
        ok(DB, "sessionTable", &format!(r#"{{"table":{SESSION_TABLE},"rows":{INSERT_VALUES}}}"#)),
        r#"["CREATE TEMPORARY VIEW `legend_rows` AS SELECT CAST(`c0` AS BIGINT) AS `ID`, CAST(`c1` AS DOUBLE) AS `Amt` FROM VALUES (1L, NULL) AS `t`(`c0`, `c1`)"]"#
    );
    assert_eq!(
        ok(DB, "sessionTable", &format!(r#"{{"table":{SESSION_TABLE},"rows":null}}"#)),
        r#"["CREATE TEMPORARY VIEW `legend_rows` AS SELECT CAST(NULL AS BIGINT) AS `ID`, CAST(NULL AS DOUBLE) AS `Amt` WHERE FALSE"]"#
    );
    let r = refused(DB, "sessionTable", &format!(r#"{{"table":{CREATE_TABLE},"rows":null}}"#));
    assert_eq!((r.kind(), r.message.as_str()), ("ILLEGAL_ARGUMENT", "a session table is temporary: Trades"));
    let delete = r#"{"dml":{"_type":"deleteWhere","schema":null,"table":"T","quoting":{"schema":false,"table":false},"alias":"t","storedTypes":{},"where":null}}"#;
    assert_eq!(
        refused(DB, "dml.render", delete).message,
        "a DELETE ... WHERE on Databricks: no spelling measured on its database (the store writes run on DuckDB, H2 \
         and Postgres, the writes design's leg 2)"
    );
}

#[test]
fn names_are_databricks() {
    let name = |op: &str, args: &str| ok(DB, op, args);
    assert_eq!(name("physicalName", r#"{"name":"Trades"}"#), r#""`Trades`""#);
    // a double-quoted declaration is its inner name
    assert_eq!(name("physicalName", r#"{"name":"\"date\""}"#), r#""`date`""#);
    assert_eq!(name("storedName", r#"{"name":"Trades","declaredQuoted":true}"#), r#""`Trades`""#);
    assert_eq!(name("insertColumnName", r#"{"name":"a`b","declaredQuoted":false}"#), r#""`a``b`""#);
    let r = refused(DB, "fromEngineText", r#"{"statement":"CREATE TABLE T (X DOUBLE)"}"#);
    assert_eq!(
        (r.kind(), r.message.as_str()),
        ("DIALECT_CAPABILITY", "the engine's H2 text reached Databricks, which has no translation of it")
    );
    assert_eq!(name("raw", r#"{"statement":"select 1","writtenFor":"Databricks"}"#), r#""select 1""#);
    assert_eq!(
        name("tableName", r#"{"name":"Sales.Orders","quoting":{"schema":false,"table":false}}"#),
        r#""`Sales`.`Orders`""#
    );
}

#[test]
fn the_interface_answers_as_databricks() {
    assert_eq!(ok(DB, "wireTypeName", r#"{"driverTypeName":"STRING"}"#), r#""VARCHAR""#);
    assert_eq!(ok(DB, "typingWireName", r#"{"driverTypeName":"INT"}"#), r#""INTEGER""#);
    let row = r#"{"_type":"struct","fields":[{"name":"a","type":{"_type":"scalar","name":"BIGINT"}}]}"#;
    assert_eq!(ok(DB, "deliversOwn", &format!(r#"{{"label":{row},"wire":"JSON"}}"#)), "false");
    assert_eq!(ok(DB, "script", r#"{"statements":["a","b"]}"#), r#""a;\nb;""#);
    assert_eq!(ok(DB, "failingStatement", r#"{"message":"x","statements":["a"]}"#), "null");
    let r = refused(DB, "catalogColumnsQuery", r#"{"schema":"s","table":"t"}"#);
    assert_eq!(
        (r.kind(), r.message.as_str()),
        ("UNSUPPORTED_OPERATION", "the platform asks no catalog question of Databricks")
    );
    let r = refused(DB, "bareNameReaches", r#"{"catalogName":"t"}"#);
    assert_eq!(r.message, "reading this database's catalog names is not implemented for Databricks");
    let r = refused(DB, "host.openDatabase", r#"{"name":"p"}"#);
    assert_eq!(
        r.message,
        "a project's own database on Databricks is not built: only DuckDB makes one so far \
         (docs/ONE_PLATFORM_PROGRAM_2026_10_12.md C5)"
    );
    let reads = |kind: &str| ok(DB, "readsStored", &format!(r#"{{"type":{{"_type":"plain","kind":"{kind}"}}}}"#));
    assert_eq!(reads("OTHER"), "true");
    assert_eq!(reads("JSON"), "false");
    assert_eq!(refused(DB, "duckdb.tables", "{}").message, "'duckdb.tables' is DuckDb's op, not Databricks's");
    assert_eq!(
        refused(DB, "engine.inTempTableSelect", r#"{"table":"t"}"#).message,
        "'engine.inTempTableSelect' is an engine-text dialect's op (EngineStyleH2, EngineStyleDB2, \
         EngineStyleComposite), not Databricks's"
    );
}
