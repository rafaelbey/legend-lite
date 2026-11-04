// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! Every op of the seam through [`dispatch`] for Snowflake, each answer held to what Java's `SqlDialect` answers for
//! it (`Snowflake.java`'s overrides, `AnsiSqlRenderer`'s implementations, the interface's defaults naming the
//! dialect). The golden oracles hold the renders (`tests/dialect_differential.rs`).

use super::*;

const SF: &str = r#"{"dialect":"Snowflake","server":null}"#;

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
fn facts_are_snowflakes() {
    assert_eq!(
        ok(SF, "facts", "{}"),
        r#"{"temporaryTables":true,"loadsRowsInBulk":false,"relational":true,"inProcess":false,"#.to_owned()
            + r#""dropsStagedTablesWithTheirTransaction":false,"needsStaticPivot":true,"serverVersions":null,"#
            + r#""inMemoryPath":null,"scriptAbort":null,"sessionSetup":["ALTER SESSION SET TIMEZONE = 'UTC', "#
            + r#"WEEK_START = 1, WEEK_OF_YEAR_POLICY = 1, TIMESTAMP_TYPE_MAPPING = 'TIMESTAMP_NTZ', "#
            + r#"QUOTED_IDENTIFIERS_IGNORE_CASE = FALSE, MULTI_STATEMENT_COUNT = 0, JSON_INDENT = 0, "#
            + r#"STRICT_JSON_OUTPUT = TRUE"],"keysSnapshot":"NONE","resultColumnLimit":null}"#
    );
    // every server version is spelled alike; a spec's server keeps Snowflake
    assert_eq!(ok(SF, "forServer", r#"{"version":"10.36.101"}"#), r#""Snowflake""#);
    assert_eq!(dispatch(r#"{"dialect":"Snowflake","server":"9"}"#, "facts", "{}").unwrap(), ok(SF, "facts", "{}"));
    assert_eq!(
        refused(r#"{"dialect":"Snowflake","timeZone":"UTC"}"#, "facts", "{}").message,
        "quoteIdentifiers and timeZone configure an engine-text dialect (EngineStyleH2, EngineStyleDB2), not Snowflake"
    );
}

/// `SELECT 1 AS "b", 'it''s' AS "s"` with an offset and a limit.
const SELECT: &str = r#"{"_type":"select","projections":[{"expr":{"_type":"intLit","type":{"_type":"typed","sqlType":{"_type":"scalar","name":"BIGINT"},"nullable":false},"value":1},"alias":"b","out":null},{"expr":{"_type":"stringLit","type":{"_type":"typed","sqlType":{"_type":"scalar","name":"VARCHAR"},"nullable":false},"value":"it's"},"alias":"s","out":null}],"distinct":false,"from":{"_type":"dual"},"where":null,"groupBy":[],"having":null,"qualify":null,"orderBy":[],"limit":5,"offset":3,"outputs":[]}"#;

/// `SELECT 1 AS "b"` with an offset and no limit.
const OFFSET_ONLY: &str = r#"{"_type":"select","projections":[{"expr":{"_type":"intLit","type":{"_type":"typed","sqlType":{"_type":"scalar","name":"BIGINT"},"nullable":false},"value":1},"alias":"b","out":null}],"distinct":false,"from":{"_type":"dual"},"where":null,"groupBy":[],"having":null,"qualify":null,"orderBy":[],"limit":null,"offset":3,"outputs":[]}"#;

#[test]
fn queries_render_as_snowflake_writes_them() {
    // every identifier quoted; a quote doubled
    assert_eq!(
        ok(SF, "query.render", &wrap("query", SELECT)),
        r#""SELECT 1 AS \"b\", 'it''s' AS \"s\"\nLIMIT 5\nOFFSET 3""#
    );
    assert_eq!(
        ok(SF, "query.renderInline", &wrap("query", SELECT)),
        r#""SELECT 1 AS \"b\", 'it''s' AS \"s\" LIMIT 5 OFFSET 3""#
    );
    assert_eq!(
        ok(SF, "query.renderStatement", &wrap("query", SELECT)),
        r#"{"sql":"SELECT 1 AS \"b\", 'it''s' AS \"s\"\nLIMIT 5\nOFFSET 3","binds":[],"regions":[]}"#
    );
    // OFFSET needs a LIMIT: LIMIT NULL
    assert_eq!(ok(SF, "query.render", &wrap("query", OFFSET_ONLY)), r#""SELECT 1 AS \"b\"\nLIMIT NULL\nOFFSET 3""#);
}

const CREATE_TABLE: &str = r#"{"_type":"createTable","schema":"S","table":"Trades","quoting":{"schema":false,"table":false},"columns":[{"name":"ID","declaredQuoted":false,"type":{"_type":"plain","kind":"TINYINT"},"notNull":true,"primaryKey":true},{"name":"Amt","declaredQuoted":true,"type":{"_type":"scaled","kind":"DECIMAL","precision":40,"scale":2},"notNull":false,"primaryKey":false},{"name":"J","declaredQuoted":false,"type":{"_type":"plain","kind":"JSON"},"notNull":false,"primaryKey":false}],"temporary":false}"#;

const SESSION_TABLE: &str = r#"{"_type":"createTable","schema":null,"table":"legend_rows","quoting":{"schema":false,"table":false},"columns":[{"name":"ID","declaredQuoted":false,"type":{"_type":"plain","kind":"BIGINT"},"notNull":true,"primaryKey":true}],"temporary":true}"#;

const INSERT_VALUES: &str = r#"{"_type":"insertValues","schema":null,"table":"legend_rows","quoting":{"schema":false,"table":false},"columns":["ID"],"quotedColumns":[],"rows":[[{"_type":"intLit","type":{"_type":"typed","sqlType":{"_type":"scalar","name":"BIGINT"},"nullable":false},"value":1}]]}"#;

#[test]
fn ddl_and_dml_render_and_a_session_table_is_a_temporary_table() {
    // the base's DDL in Snowflake's types; every identifier quoted
    let create = r#""Create Table \"S\".\"Trades\"(\"ID\" TINYINT NOT NULL, \"Amt\" DECIMAL(40, 2) NULL, \"J\" VARIANT NULL, PRIMARY KEY(\"ID\"));""#;
    assert_eq!(ok(SF, "ddl.render", &wrap("ddl", CREATE_TABLE)), create);
    let insert = r#""INSERT INTO \"legend_rows\" (\"ID\") VALUES (1);""#;
    assert_eq!(ok(SF, "dml.render", &wrap("dml", INSERT_VALUES)), insert);
    // the interface's default: the table, then its INSERT
    let temporary = r#""Create Temporary Table \"legend_rows\"(\"ID\" BIGINT NOT NULL, PRIMARY KEY(\"ID\"));""#;
    assert_eq!(
        ok(SF, "sessionTable", &format!(r#"{{"table":{SESSION_TABLE},"rows":{INSERT_VALUES}}}"#)),
        format!("[{temporary},{insert}]")
    );
    assert_eq!(ok(SF, "sessionTable", &format!(r#"{{"table":{SESSION_TABLE},"rows":null}}"#)), format!("[{temporary}]"));
    let delete =
        r#"{"dml":{"_type":"deleteAll","schema":"default","table":"T","quoting":{"schema":false,"table":true}}}"#;
    assert_eq!(ok(SF, "dml.render", delete), r#""DELETE FROM \"T\";""#);
    assert_eq!(ok(SF, "dml.renderStatement", delete), r#"{"sql":"DELETE FROM \"T\";","binds":[],"regions":[]}"#);
    let drop = r#"{"ddl":{"_type":"dropSchema","schema":"S","quoted":true}}"#;
    assert_eq!(ok(SF, "ddl.render", drop), r#""Drop schema if exists \"S\" cascade;""#);
}

#[test]
fn names_are_snowflakes() {
    let name = |op: &str, args: &str| ok(SF, op, args);
    assert_eq!(name("physicalName", r#"{"name":"Trades"}"#), r#""\"Trades\"""#);
    // a quote-bearing declaration is already its own spelling
    assert_eq!(name("physicalName", r#"{"name":"\"date\""}"#), r#""\"date\"""#);
    assert_eq!(name("storedName", r#"{"name":"a\"b","declaredQuoted":true}"#), r#""\"a\"\"b\"""#);
    assert_eq!(name("insertColumnName", r#"{"name":"x","declaredQuoted":false}"#), r#""\"x\"""#);
    let r = refused(SF, "fromEngineText", r#"{"statement":"CREATE TABLE T (X DOUBLE)"}"#);
    assert_eq!(
        (r.kind(), r.message.as_str()),
        ("DIALECT_CAPABILITY", "the engine's H2 text reached Snowflake, which has no translation of it")
    );
    assert_eq!(name("raw", r#"{"statement":"select 1","writtenFor":"Snowflake"}"#), r#""select 1""#);
    let r = refused(SF, "raw", r#"{"statement":"select 1","writtenFor":"H2"}"#);
    assert_eq!(r.message, "the engine's H2 text reached Snowflake, which has no translation of it");
    assert_eq!(
        name("tableName", r#"{"name":"Sales.Orders","quoting":{"schema":false,"table":false}}"#),
        r#""\"Sales\".\"Orders\"""#
    );
}

#[test]
fn the_interface_answers_as_snowflake() {
    // its driver names every fixed-point number NUMBER: DECIMAL at the wire, nothing to type by
    assert_eq!(ok(SF, "wireTypeName", r#"{"driverTypeName":"NUMBER"}"#), r#""DECIMAL""#);
    assert_eq!(ok(SF, "wireTypeName", r#"{"driverTypeName":" number "}"#), r#""DECIMAL""#);
    assert_eq!(ok(SF, "typingWireName", r#"{"driverTypeName":"NUMBER"}"#), "null");
    assert_eq!(ok(SF, "wireTypeName", r#"{"driverTypeName":"VARCHAR"}"#), r#""VARCHAR""#);
    assert_eq!(ok(SF, "typingWireName", r#"{"driverTypeName":"VARCHAR"}"#), r#""VARCHAR""#);
    assert_eq!(ok(SF, "wireTypeName", r#"{"driverTypeName":null}"#), r#""""#);
    let bigint = r#"{"_type":"scalar","name":"BIGINT"}"#;
    assert_eq!(ok(SF, "deliversOwn", &format!(r#"{{"label":{bigint},"wire":"DECIMAL(38,0)"}}"#)), "true");
    assert_eq!(ok(SF, "deliversOwn", &format!(r#"{{"label":{bigint},"wire":"DECIMAL(38,2)"}}"#)), "false");
    let map = r#"{"_type":"map","key":{"_type":"scalar","name":"VARCHAR"},"value":{"_type":"scalar","name":"BIGINT"}}"#;
    assert_eq!(ok(SF, "deliversOwn", &format!(r#"{{"label":{map},"wire":"OBJECT"}}"#)), "true");
    assert_eq!(ok(SF, "script", r#"{"statements":["a","b"]}"#), r#""a;\nb;""#);
    assert_eq!(ok(SF, "failingStatement", r#"{"message":"x","statements":["a"]}"#), "null");
    let r = refused(SF, "catalogColumnsQuery", r#"{"schema":"s","table":"t"}"#);
    assert_eq!(
        (r.kind(), r.message.as_str()),
        ("UNSUPPORTED_OPERATION", "the platform asks no catalog question of Snowflake")
    );
    let r = refused(SF, "bareNameReaches", r#"{"catalogName":"t"}"#);
    assert_eq!(r.message, "reading this database's catalog names is not implemented for Snowflake");
    let column = r#"{"column":{"name":"n","dataType":"NUMBER","logicalType":null,"precision":38,"scale":0,"notNull":false}}"#;
    let r = refused(SF, "catalogType", column);
    assert_eq!(r.message, "reading this database's catalog type 'NUMBER' is not implemented for Snowflake");
    assert_eq!(ok(SF, "copySelectList", r#"{"conversions":[]}"#), r#""*""#);
    let r = refused(SF, "copySelectList", r#"{"conversions":[{"column":"c","sql":"x"}]}"#);
    assert_eq!(r.message, "a copy that converts columns in its star select is not implemented for Snowflake");
    for op in ["host.openDatabase", "host.useDatabase", "host.closeDatabase"] {
        let r = refused(SF, op, r#"{"name":"p"}"#);
        assert!(r.message.contains("on Snowflake"), "{op}: {r}");
    }
    let reads = |kind: &str| ok(SF, "readsStored", &format!(r#"{{"type":{{"_type":"plain","kind":"{kind}"}}}}"#));
    assert_eq!(reads("OTHER"), "true");
    assert_eq!(reads("JSON"), "false");
    assert_eq!(
        refused(SF, "duckdb.tables", "{}").message,
        "'duckdb.tables' is DuckDb's op, not Snowflake's"
    );
    assert_eq!(
        refused(SF, "engine.selectList", &wrap("query", SELECT)).message,
        "'engine.selectList' is an engine-text dialect's op (EngineStyleH2, EngineStyleDB2, EngineStyleComposite), \
         not Snowflake's"
    );
}
