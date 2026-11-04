// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! Every op of the seam through [`dispatch`] for BigQuery, each answer held to what Java's `SqlDialect` answers for it
//! (`BigQuery.java`'s overrides, `AnsiSqlRenderer`'s implementations, the interface's defaults naming the dialect).
//! The golden oracles hold the renders (`tests/dialect_differential.rs`).

use super::*;

const BQ: &str = r#"{"dialect":"BigQuery","server":null}"#;

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
fn facts_are_bigquerys() {
    assert_eq!(
        ok(BQ, "facts", "{}"),
        r#"{"temporaryTables":true,"loadsRowsInBulk":false,"relational":true,"inProcess":false,"#.to_owned()
            + r#""dropsStagedTablesWithTheirTransaction":false,"needsStaticPivot":true,"serverVersions":null,"#
            + r#""inMemoryPath":null,"scriptAbort":null,"sessionSetup":[],"keysSnapshot":"NONE","#
            + r#""resultColumnLimit":10000}"#
    );
    // every server version is spelled alike; a spec's server keeps BigQuery
    assert_eq!(ok(BQ, "forServer", r#"{"version":"1"}"#), r#""BigQuery""#);
    assert_eq!(dispatch(r#"{"dialect":"BigQuery","server":"2"}"#, "facts", "{}").unwrap(), ok(BQ, "facts", "{}"));
    assert_eq!(
        refused(r#"{"dialect":"BigQuery","quoteIdentifiers":false}"#, "facts", "{}").message,
        "quoteIdentifiers and timeZone configure an engine-text dialect (EngineStyleH2, EngineStyleDB2), not BigQuery"
    );
}

/// `SELECT 1 AS "b", 'it''s' AS "s"` with an offset and a limit.
const SELECT: &str = r#"{"_type":"select","projections":[{"expr":{"_type":"intLit","type":{"_type":"typed","sqlType":{"_type":"scalar","name":"BIGINT"},"nullable":false},"value":1},"alias":"b","out":null},{"expr":{"_type":"stringLit","type":{"_type":"typed","sqlType":{"_type":"scalar","name":"VARCHAR"},"nullable":false},"value":"it's"},"alias":"s","out":null}],"distinct":false,"from":{"_type":"dual"},"where":null,"groupBy":[],"having":null,"qualify":null,"orderBy":[],"limit":5,"offset":3,"outputs":[]}"#;

/// `SELECT 1 AS "b"` with an offset and no limit.
const OFFSET_ONLY: &str = r#"{"_type":"select","projections":[{"expr":{"_type":"intLit","type":{"_type":"typed","sqlType":{"_type":"scalar","name":"BIGINT"},"nullable":false},"value":1},"alias":"b","out":null}],"distinct":false,"from":{"_type":"dual"},"where":null,"groupBy":[],"having":null,"qualify":null,"orderBy":[],"limit":null,"offset":3,"outputs":[]}"#;

#[test]
fn queries_render_as_bigquery_writes_them() {
    // a quote escaped by a backslash; every identifier in backticks
    assert_eq!(
        ok(BQ, "query.render", &wrap("query", SELECT)),
        r#""SELECT 1 AS `b`, 'it\\'s' AS `s`\nLIMIT 5\nOFFSET 3""#
    );
    assert_eq!(
        ok(BQ, "query.renderInline", &wrap("query", SELECT)),
        r#""SELECT 1 AS `b`, 'it\\'s' AS `s` LIMIT 5 OFFSET 3""#
    );
    assert_eq!(
        ok(BQ, "query.renderStatement", &wrap("query", SELECT)),
        r#"{"sql":"SELECT 1 AS `b`, 'it\\'s' AS `s`\nLIMIT 5\nOFFSET 3","binds":[],"regions":[]}"#
    );
    // an OFFSET only after a LIMIT: the largest one
    assert_eq!(
        ok(BQ, "query.render", &wrap("query", OFFSET_ONLY)),
        r#""SELECT 1 AS `b`\nLIMIT 9223372036854775807\nOFFSET 3""#
    );
}

const CREATE_TABLE: &str = r#"{"_type":"createTable","schema":"S","table":"Trades","quoting":{"schema":false,"table":false},"columns":[{"name":"ID","declaredQuoted":false,"type":{"_type":"plain","kind":"TINYINT"},"notNull":true,"primaryKey":true},{"name":"Amt","declaredQuoted":true,"type":{"_type":"scaled","kind":"DECIMAL","precision":40,"scale":2},"notNull":false,"primaryKey":false}],"temporary":false}"#;

const SESSION_TABLE: &str = r#"{"_type":"createTable","schema":null,"table":"legend_rows","quoting":{"schema":false,"table":false},"columns":[{"name":"ID","declaredQuoted":false,"type":{"_type":"plain","kind":"BIGINT"},"notNull":true,"primaryKey":true}],"temporary":true}"#;

const INSERT_VALUES: &str = r#"{"_type":"insertValues","schema":null,"table":"legend_rows","quoting":{"schema":false,"table":false},"columns":["ID"],"quotedColumns":[],"rows":[[{"_type":"intLit","type":{"_type":"typed","sqlType":{"_type":"scalar","name":"BIGINT"},"nullable":false},"value":1}]]}"#;

#[test]
fn ddl_and_dml_render_and_a_session_table_is_a_temp_table() {
    // a key declared NOT ENFORCED; a DECIMAL past NUMERIC's digits is a BIGNUMERIC
    let create = r#""CREATE TABLE `S`.`Trades` (`ID` INT64 NOT NULL, `Amt` BIGNUMERIC(40, 2), PRIMARY KEY (`ID`) NOT ENFORCED);""#;
    assert_eq!(ok(BQ, "ddl.render", &wrap("ddl", CREATE_TABLE)), create);
    let insert = r#""INSERT INTO `legend_rows` (`ID`) VALUES (1);""#;
    assert_eq!(ok(BQ, "dml.render", &wrap("dml", INSERT_VALUES)), insert);
    // the interface's default: the TEMP table, then its INSERT
    let temporary = r#""CREATE TEMP TABLE `legend_rows` (`ID` INT64 NOT NULL, PRIMARY KEY (`ID`) NOT ENFORCED);""#;
    assert_eq!(
        ok(BQ, "sessionTable", &format!(r#"{{"table":{SESSION_TABLE},"rows":{INSERT_VALUES}}}"#)),
        format!("[{temporary},{insert}]")
    );
    // a DELETE needs a WHERE clause
    let delete =
        r#"{"dml":{"_type":"deleteAll","schema":"default","table":"T","quoting":{"schema":false,"table":true}}}"#;
    assert_eq!(ok(BQ, "dml.render", delete), r#""DELETE FROM `T` WHERE TRUE;""#);
    assert_eq!(
        ok(BQ, "dml.renderStatement", delete),
        r#"{"sql":"DELETE FROM `T` WHERE TRUE;","binds":[],"regions":[]}"#
    );
    let drop = r#"{"ddl":{"_type":"dropSchema","schema":"S","quoted":true}}"#;
    assert_eq!(ok(BQ, "ddl.render", drop), r#""DROP SCHEMA IF EXISTS `S` CASCADE;""#);
}

#[test]
fn names_are_bigquerys() {
    let name = |op: &str, args: &str| ok(BQ, op, args);
    assert_eq!(name("physicalName", r#"{"name":"Trades"}"#), r#""`Trades`""#);
    // a double-quoted declaration is its inner name; a backtick and a backslash escaped by a backslash
    assert_eq!(name("physicalName", r#"{"name":"\"date\""}"#), r#""`date`""#);
    assert_eq!(name("insertColumnName", r#"{"name":"a`b","declaredQuoted":false}"#), r#""`a\\`b`""#);
    let r = refused(BQ, "fromEngineText", r#"{"statement":"CREATE TABLE T (X DOUBLE)"}"#);
    assert_eq!(
        (r.kind(), r.message.as_str()),
        ("DIALECT_CAPABILITY", "the engine's H2 text reached BigQuery, which has no translation of it")
    );
    assert_eq!(name("raw", r#"{"statement":"select 1","writtenFor":"BigQuery"}"#), r#""select 1""#);
    assert_eq!(
        name("tableName", r#"{"name":"Sales.Orders","quoting":{"schema":false,"table":false}}"#),
        r#""`Sales`.`Orders`""#
    );
}

#[test]
fn the_interface_answers_as_bigquery() {
    assert_eq!(ok(BQ, "wireTypeName", r#"{"driverTypeName":"INT64"}"#), r#""BIGINT""#);
    assert_eq!(ok(BQ, "wireTypeName", r#"{"driverTypeName":null}"#), r#""""#);
    assert_eq!(ok(BQ, "typingWireName", r#"{"driverTypeName":"DATETIME"}"#), r#""TIMESTAMP""#);
    let map = r#"{"_type":"map","key":{"_type":"scalar","name":"VARCHAR"},"value":{"_type":"scalar","name":"BIGINT"}}"#;
    assert_eq!(ok(BQ, "deliversOwn", &format!(r#"{{"label":{map},"wire":"ARRAY"}}"#)), "true");
    let decimal = r#"{"_type":"decimal","precision":10,"scale":2}"#;
    assert_eq!(ok(BQ, "deliversOwn", &format!(r#"{{"label":{decimal},"wire":"BIGNUMERIC"}}"#)), "true");
    assert_eq!(ok(BQ, "deliversOwn", &format!(r#"{{"label":{decimal},"wire":"NUMERIC"}}"#)), "false");
    assert_eq!(ok(BQ, "script", r#"{"statements":["a","b"]}"#), r#""a;\nb;""#);
    assert_eq!(ok(BQ, "failingStatement", r#"{"message":"x","statements":["a"]}"#), "null");
    let r = refused(BQ, "catalogColumnsQuery", r#"{"schema":"s","table":"t"}"#);
    assert_eq!(
        (r.kind(), r.message.as_str()),
        ("UNSUPPORTED_OPERATION", "the platform asks no catalog question of BigQuery")
    );
    let r = refused(BQ, "bareNameReaches", r#"{"catalogName":"t"}"#);
    assert_eq!(r.message, "reading this database's catalog names is not implemented for BigQuery");
    let r = refused(BQ, "host.useDatabase", r#"{"name":"p"}"#);
    assert!(r.message.contains("on BigQuery"), "{r}");
    let reads = |kind: &str| ok(BQ, "readsStored", &format!(r#"{{"type":{{"_type":"plain","kind":"{kind}"}}}}"#));
    assert_eq!(reads("DISTINCT"), "true");
    assert_eq!(reads("VARCHAR"), "false");
    assert_eq!(
        refused(BQ, "duckdb.readsStored", r#"{"type":{"_type":"plain","kind":"OTHER"}}"#).message,
        "'duckdb.readsStored' is DuckDb's op, not BigQuery's"
    );
    assert_eq!(
        refused(BQ, "engine.inTempTableSelect", r#"{"table":"t"}"#).message,
        "'engine.inTempTableSelect' is an engine-text dialect's op (EngineStyleH2, EngineStyleDB2, \
         EngineStyleComposite), not BigQuery's"
    );
}
