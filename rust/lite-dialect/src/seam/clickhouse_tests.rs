// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! Every op of the seam through [`dispatch`] for ClickHouse, each answer held to what Java's `SqlDialect` answers for
//! it (`ClickHouse.java`'s overrides, `AnsiSqlRenderer`'s implementations, the interface's defaults naming the
//! dialect). The golden oracles hold the renders (`tests/dialect_differential.rs`).

use super::*;

const CH: &str = r#"{"dialect":"ClickHouse","server":null}"#;

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

/// The SETTINGS clause a root statement closes with, its correlated subqueries' setting off (as JSON text).
const SETTINGS: &str = "SETTINGS join_use_nulls = 1, union_default_mode = 'DISTINCT', cast_keep_nullable = 1, \
                        session_timezone = 'UTC', query_plan_merge_expressions = 0, \
                        allow_experimental_correlated_subqueries = 0";

#[test]
fn facts_are_clickhouses() {
    assert_eq!(
        ok(CH, "facts", "{}"),
        r#"{"temporaryTables":true,"loadsRowsInBulk":false,"relational":true,"inProcess":false,"#.to_owned()
            + r#""dropsStagedTablesWithTheirTransaction":false,"needsStaticPivot":true,"serverVersions":null,"#
            + r#""inMemoryPath":null,"scriptAbort":null,"sessionSetup":[],"keysSnapshot":"NONE","#
            + r#""resultColumnLimit":null}"#
    );
    // every server version is spelled alike; a spec's server keeps ClickHouse
    assert_eq!(ok(CH, "forServer", r#"{"version":"25.8"}"#), r#""ClickHouse""#);
    assert_eq!(dispatch(r#"{"dialect":"ClickHouse","server":"24"}"#, "facts", "{}").unwrap(), ok(CH, "facts", "{}"));
    assert_eq!(
        refused(r#"{"dialect":"ClickHouse","timeZone":"UTC"}"#, "facts", "{}").message,
        "quoteIdentifiers and timeZone configure an engine-text dialect (EngineStyleH2, EngineStyleDB2), not ClickHouse"
    );
}

/// `SELECT 1 AS "b", 'it''s' AS "s"` with an offset and a limit.
const SELECT: &str = r#"{"_type":"select","projections":[{"expr":{"_type":"intLit","type":{"_type":"typed","sqlType":{"_type":"scalar","name":"BIGINT"},"nullable":false},"value":1},"alias":"b","out":null},{"expr":{"_type":"stringLit","type":{"_type":"typed","sqlType":{"_type":"scalar","name":"VARCHAR"},"nullable":false},"value":"it's"},"alias":"s","out":null}],"distinct":false,"from":{"_type":"dual"},"where":null,"groupBy":[],"having":null,"qualify":null,"orderBy":[],"limit":5,"offset":3,"outputs":[]}"#;

#[test]
fn queries_render_as_clickhouse_writes_them() {
    // an integer literal is an Int64; a quote escaped by a backslash; the root closes with its SETTINGS
    let sql = format!("SELECT toInt64(1) AS \\\"b\\\", 'it\\\\'s' AS \\\"s\\\"\\nLIMIT 5\\nOFFSET 3\\n{SETTINGS}");
    assert_eq!(ok(CH, "query.render", &wrap("query", SELECT)), format!("\"{sql}\""));
    assert_eq!(
        ok(CH, "query.renderInline", &wrap("query", SELECT)),
        format!("\"SELECT toInt64(1) AS \\\"b\\\", 'it\\\\'s' AS \\\"s\\\" LIMIT 5 OFFSET 3 {SETTINGS}\"")
    );
    assert_eq!(
        ok(CH, "query.renderStatement", &wrap("query", SELECT)),
        format!("{{\"sql\":\"{sql}\",\"binds\":[],\"regions\":[]}}")
    );
}

const CREATE_TABLE: &str = r#"{"_type":"createTable","schema":"S","table":"Trades","quoting":{"schema":false,"table":false},"columns":[{"name":"ID","declaredQuoted":false,"type":{"_type":"plain","kind":"BIT"},"notNull":true,"primaryKey":true},{"name":"Amt","declaredQuoted":true,"type":{"_type":"plain","kind":"VARCHAR"},"notNull":false,"primaryKey":false}],"temporary":false}"#;

const SESSION_TABLE: &str = r#"{"_type":"createTable","schema":null,"table":"legend_rows","quoting":{"schema":false,"table":false},"columns":[{"name":"ID","declaredQuoted":false,"type":{"_type":"plain","kind":"BIGINT"},"notNull":true,"primaryKey":true}],"temporary":true}"#;

const INSERT_VALUES: &str = r#"{"_type":"insertValues","schema":null,"table":"legend_rows","quoting":{"schema":false,"table":false},"columns":["ID"],"quotedColumns":[],"rows":[[{"_type":"intLit","type":{"_type":"typed","sqlType":{"_type":"scalar","name":"BIGINT"},"nullable":false},"value":1}]]}"#;

#[test]
fn ddl_and_dml_render_and_a_session_table_is_a_temporary_table() {
    // a BIT column is a Bool; every name quoted
    let create = r#""Create Table \"S\".\"Trades\"(\"ID\" Bool NOT NULL, \"Amt\" VARCHAR NULL, PRIMARY KEY(\"ID\"));""#;
    assert_eq!(ok(CH, "ddl.render", &wrap("ddl", CREATE_TABLE)), create);
    let insert = r#""INSERT INTO \"legend_rows\" (\"ID\") VALUES (toInt64(1));""#;
    assert_eq!(ok(CH, "dml.render", &wrap("dml", INSERT_VALUES)), insert);
    // a session's table is the TEMPORARY table and its INSERT, the interface's own renders
    let temporary = r#""Create Temporary Table \"legend_rows\"(\"ID\" BIGINT NOT NULL, PRIMARY KEY(\"ID\"));""#;
    assert_eq!(
        ok(CH, "sessionTable", &format!(r#"{{"table":{SESSION_TABLE},"rows":{INSERT_VALUES}}}"#)),
        format!("[{temporary},{insert}]")
    );
    assert_eq!(
        ok(CH, "sessionTable", &format!(r#"{{"table":{SESSION_TABLE},"rows":null}}"#)),
        format!("[{temporary}]")
    );
    let r = refused(CH, "sessionTable", &format!(r#"{{"table":{CREATE_TABLE},"rows":null}}"#));
    assert_eq!((r.kind(), r.message.as_str()), ("ILLEGAL_ARGUMENT", "a session table is temporary: Trades"));
}

#[test]
fn names_are_clickhouses() {
    let name = |op: &str, args: &str| ok(CH, op, args);
    assert_eq!(name("physicalName", r#"{"name":"Trades"}"#), r#""\"Trades\"""#);
    // a double-quoted declaration is its own spelling
    assert_eq!(name("physicalName", r#"{"name":"\"date\""}"#), r#""\"date\"""#);
    assert_eq!(name("storedName", r#"{"name":"Trades","declaredQuoted":true}"#), r#""\"Trades\"""#);
    assert_eq!(name("insertColumnName", r#"{"name":"a\"b","declaredQuoted":false}"#), r#""\"a\"\"b\"""#);
    let r = refused(CH, "fromEngineText", r#"{"statement":"CREATE TABLE T (X DOUBLE)"}"#);
    assert_eq!(
        (r.kind(), r.message.as_str()),
        ("DIALECT_CAPABILITY", "the engine's H2 text reached ClickHouse, which has no translation of it")
    );
    assert_eq!(name("raw", r#"{"statement":"select 1","writtenFor":"ClickHouse"}"#), r#""select 1""#);
    assert_eq!(
        name("tableName", r#"{"name":"Sales.Orders","quoting":{"schema":false,"table":false}}"#),
        r#""\"Sales\".\"Orders\"""#
    );
}

#[test]
fn the_interface_answers_as_clickhouse() {
    assert_eq!(ok(CH, "wireTypeName", r#"{"driverTypeName":"Nullable(Int64)"}"#), r#""BIGINT""#);
    assert_eq!(ok(CH, "wireTypeName", r#"{"driverTypeName":"DateTime64(6, 'UTC')"}"#), r#""TIMESTAMP""#);
    assert_eq!(ok(CH, "wireTypeName", r#"{"driverTypeName":null}"#), r#""""#);
    assert_eq!(ok(CH, "typingWireName", r#"{"driverTypeName":"Nullable(UInt8)"}"#), r#""INTEGER""#);
    let json = r#"{"_type":"scalar","name":"JSON"}"#;
    assert_eq!(ok(CH, "deliversOwn", &format!(r#"{{"label":{json},"wire":"VARCHAR"}}"#)), "true");
    assert_eq!(ok(CH, "deliversOwn", &format!(r#"{{"label":{json},"wire":"JSON"}}"#)), "false");
    assert_eq!(ok(CH, "script", r#"{"statements":["a","b"]}"#), r#""a;\nb;""#);
    assert_eq!(ok(CH, "failingStatement", r#"{"message":"x","statements":["a"]}"#), "null");
    let r = refused(CH, "catalogColumnsQuery", r#"{"schema":"s","table":"t"}"#);
    assert_eq!(
        (r.kind(), r.message.as_str()),
        ("UNSUPPORTED_OPERATION", "the platform asks no catalog question of ClickHouse")
    );
    let column = r#"{"name":"c","dataType":"Int64","logicalType":null,"precision":null,"scale":null,"notNull":false}"#;
    let r = refused(CH, "catalogType", &format!(r#"{{"column":{column}}}"#));
    assert_eq!(r.message, "reading this database's catalog type 'Int64' is not implemented for ClickHouse");
    let r = refused(CH, "bareNameReaches", r#"{"catalogName":"t"}"#);
    assert_eq!(r.message, "reading this database's catalog names is not implemented for ClickHouse");
    assert_eq!(ok(CH, "copySelectList", r#"{"conversions":[]}"#), r#""*""#);
    // no host operations of its own: each refused, naming the dialect
    let r = refused(CH, "host.openDatabase", r#"{"name":"p"}"#);
    assert_eq!(
        r.message,
        "a project's own database on ClickHouse is not built: only DuckDB makes one so far \
         (docs/ONE_PLATFORM_PROGRAM_2026_10_12.md C5)"
    );
    let reads = |kind: &str| ok(CH, "readsStored", &format!(r#"{{"type":{{"_type":"plain","kind":"{kind}"}}}}"#));
    assert_eq!(reads("OTHER"), "true");
    assert_eq!(reads("JSON"), "false");
    assert_eq!(refused(CH, "duckdb.tables", "{}").message, "'duckdb.tables' is DuckDb's op, not ClickHouse's");
    assert_eq!(
        refused(CH, "engine.selectList", &wrap("query", SELECT)).message,
        "'engine.selectList' is an engine-text dialect's op (EngineStyleH2, EngineStyleDB2, EngineStyleComposite), \
         not ClickHouse's"
    );
}
