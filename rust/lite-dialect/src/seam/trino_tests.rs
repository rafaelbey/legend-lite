// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! Every op of the seam through [`dispatch`] for Trino, each answer held to what Java's `SqlDialect` answers for it
//! (`Trino.java`'s overrides, `AnsiSqlRenderer`'s implementations, the interface's defaults naming the dialect). The
//! golden oracles hold the renders (`tests/dialect_differential.rs`).

use super::*;

const TR: &str = r#"{"dialect":"Trino","server":null}"#;

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
fn facts_are_trinos() {
    assert_eq!(
        ok(TR, "facts", "{}"),
        r#"{"temporaryTables":false,"loadsRowsInBulk":false,"relational":true,"inProcess":false,"#.to_owned()
            + r#""dropsStagedTablesWithTheirTransaction":false,"needsStaticPivot":true,"serverVersions":null,"#
            + r#""inMemoryPath":null,"scriptAbort":null,"sessionSetup":["SET TIME ZONE 'UTC'"],"#
            + r#""keysSnapshot":"NONE","resultColumnLimit":null}"#
    );
    // every server version is spelled alike; a spec's server keeps Trino
    assert_eq!(ok(TR, "forServer", r#"{"version":"483"}"#), r#""Trino""#);
    assert_eq!(dispatch(r#"{"dialect":"Trino","server":"400"}"#, "facts", "{}").unwrap(), ok(TR, "facts", "{}"));
    // an engine-text dialect's configuration is no Trino's
    assert_eq!(
        refused(r#"{"dialect":"Trino","timeZone":null}"#, "facts", "{}").message,
        "quoteIdentifiers and timeZone configure an engine-text dialect (EngineStyleH2, EngineStyleDB2), not Trino"
    );
}

/// `SELECT 1 AS "b"` with an offset and a limit.
const OFFSET_SELECT: &str = r#"{"_type":"select","projections":[{"expr":{"_type":"intLit","type":{"_type":"typed","sqlType":{"_type":"scalar","name":"BIGINT"},"nullable":false},"value":1},"alias":"b","out":null}],"distinct":false,"from":{"_type":"dual"},"where":null,"groupBy":[],"having":null,"qualify":null,"orderBy":[],"limit":5,"offset":3,"outputs":[]}"#;

/// A select over raw SQL written for the engine's H2 (`query.render`).
const RAW_SELECT: &str = r#"{"_type":"select","projections":[],"distinct":false,"from":{"_type":"rawSql","sql":"select 1","alias":"_p","outputs":[],"writtenFor":"H2"},"where":null,"groupBy":[],"having":null,"qualify":null,"orderBy":[],"limit":0,"offset":null,"outputs":[]}"#;

#[test]
fn queries_render_as_trino_writes_them() {
    // an integer literal is a BIGINT; OFFSET comes before LIMIT; every identifier quoted
    assert_eq!(
        ok(TR, "query.render", &wrap("query", OFFSET_SELECT)),
        r#""SELECT BIGINT '1' AS \"b\"\nOFFSET 3\nLIMIT 5""#
    );
    assert_eq!(
        ok(TR, "query.renderInline", &wrap("query", OFFSET_SELECT)),
        r#""SELECT BIGINT '1' AS \"b\" OFFSET 3 LIMIT 5""#
    );
    assert_eq!(
        ok(TR, "query.renderStatement", &wrap("query", OFFSET_SELECT)),
        r#"{"sql":"SELECT BIGINT '1' AS \"b\"\nOFFSET 3\nLIMIT 5","binds":[],"regions":[]}"#
    );
    // raw text written for the engine's H2 is refused: Trino has no translation of it
    let r = refused(TR, "query.render", &wrap("query", RAW_SELECT));
    assert_eq!(
        (r.kind(), r.message.as_str()),
        ("DIALECT_CAPABILITY", "the engine's H2 text reached Trino, which has no translation of it")
    );
}

const CREATE_TABLE: &str = r#"{"_type":"createTable","schema":"S","table":"Trades","quoting":{"schema":false,"table":false},"columns":[{"name":"ID","declaredQuoted":false,"type":{"_type":"plain","kind":"TINYINT"},"notNull":true,"primaryKey":true},{"name":"Amt","declaredQuoted":true,"type":{"_type":"plain","kind":"DOUBLE"},"notNull":false,"primaryKey":false},{"name":"At","declaredQuoted":false,"type":{"_type":"plain","kind":"TIMESTAMP_NS"},"notNull":false,"primaryKey":false}],"temporary":false}"#;

const INSERT_VALUES: &str = r#"{"_type":"insertValues","schema":null,"table":"Trades","quoting":{"schema":false,"table":false},"columns":["ID","Amt"],"quotedColumns":["Amt"],"rows":[[{"_type":"intLit","type":{"_type":"typed","sqlType":{"_type":"scalar","name":"BIGINT"},"nullable":false},"value":1},{"_type":"nullLit","type":{"_type":"bottom"}}]]}"#;

#[test]
fn ddl_and_dml_render_and_a_session_table_is_refused() {
    // a nullable column says nothing (an explicit NULL is a syntax error); every name quoted
    let create = r#""Create Table \"S\".\"Trades\"(\"ID\" TINYINT NOT NULL, \"Amt\" DOUBLE, \"At\" TIMESTAMP(9), PRIMARY KEY(\"ID\"));""#;
    assert_eq!(ok(TR, "ddl.render", &wrap("ddl", CREATE_TABLE)), create);
    assert_eq!(
        ok(TR, "ddl.render", r#"{"ddl":{"_type":"createSchema","schema":"S","quoted":false}}"#),
        r#""Create Schema if not exists \"S\";""#
    );
    let insert = r#""INSERT INTO \"Trades\" (\"ID\", \"Amt\") VALUES (BIGINT '1', NULL);""#;
    assert_eq!(ok(TR, "dml.render", &wrap("dml", INSERT_VALUES)), insert);
    assert_eq!(
        ok(TR, "dml.renderStatement", &wrap("dml", INSERT_VALUES)),
        r#"{"sql":"INSERT INTO \"Trades\" (\"ID\", \"Amt\") VALUES (BIGINT '1', NULL);","binds":[],"regions":[]}"#
    );
    // Trino has no temporary table, so no table only one session sees
    let r = refused(TR, "sessionTable", &format!(r#"{{"table":{CREATE_TABLE},"rows":{INSERT_VALUES}}}"#));
    assert_eq!(
        (r.kind(), r.message.as_str()),
        ("DIALECT_CAPABILITY", "a session's own table reached Trino, which has no temporary tables: Trades")
    );
    // the store writes are refused by the base, naming Trino
    let delete = r#"{"dml":{"_type":"deleteWhere","schema":null,"table":"T","quoting":{"schema":false,"table":false},"alias":"t","storedTypes":{},"where":null}}"#;
    let r = refused(TR, "dml.render", delete);
    assert_eq!(
        (r.kind(), r.message.as_str()),
        (
            "DIALECT_CAPABILITY",
            "a DELETE ... WHERE on Trino: no spelling measured on its database (the store writes run on DuckDB, H2 \
             and Postgres, the writes design's leg 2)"
        )
    );
}

#[test]
fn names_are_trinos() {
    let name = |op: &str, args: &str| ok(TR, op, args);
    // every identifier quoted; a double-quoted declaration is already its own spelling
    assert_eq!(name("physicalName", r#"{"name":"Trades"}"#), r#""\"Trades\"""#);
    assert_eq!(name("physicalName", r#"{"name":"\"date\""}"#), r#""\"date\"""#);
    assert_eq!(name("physicalName", r#"{"name":"a\"b"}"#), r#""\"a\"\"b\"""#);
    assert_eq!(name("storedName", r#"{"name":"Trades","declaredQuoted":true}"#), r#""\"Trades\"""#);
    assert_eq!(name("storedName", r#"{"name":"Trades","declaredQuoted":false}"#), r#""\"Trades\"""#);
    assert_eq!(name("insertColumnName", r#"{"name":"key","declaredQuoted":false}"#), r#""\"key\"""#);
    // the engine's H2 text is refused; any other text as written
    let r = refused(TR, "fromEngineText", r#"{"statement":"CREATE TABLE T (X DOUBLE)"}"#);
    assert_eq!(
        (r.kind(), r.message.as_str()),
        ("DIALECT_CAPABILITY", "the engine's H2 text reached Trino, which has no translation of it")
    );
    let r = refused(TR, "raw", r#"{"statement":"insert into T (A) values (1)","writtenFor":"H2"}"#);
    assert_eq!(r.message, "the engine's H2 text reached Trino, which has no translation of it");
    assert_eq!(name("raw", r#"{"statement":"select 1","writtenFor":"Trino"}"#), r#""select 1""#);
    // the base's table name: a schema-qualified one each part delimited
    assert_eq!(
        name("tableName", r#"{"name":"legend_stage_rowsA","quoting":{"schema":false,"table":false}}"#),
        r#""\"legend_stage_rowsA\"""#
    );
    assert_eq!(
        name("tableName", r#"{"name":"Sales.Orders","quoting":{"schema":false,"table":false}}"#),
        r#""\"Sales\".\"Orders\"""#
    );
}

#[test]
fn the_interface_answers_as_trino() {
    // the interface's wire names
    assert_eq!(ok(TR, "wireTypeName", r#"{"driverTypeName":"varchar"}"#), r#""VARCHAR""#);
    // a timestamp of any precision is a TIMESTAMP (WireNames.fold)
    assert_eq!(ok(TR, "wireTypeName", r#"{"driverTypeName":"timestamp(6)"}"#), r#""TIMESTAMP""#);
    assert_eq!(ok(TR, "wireTypeName", r#"{"driverTypeName":null}"#), r#""""#);
    assert_eq!(ok(TR, "typingWireName", r#"{"driverTypeName":"bigint"}"#), r#""BIGINT""#);
    // a ROW column is delivered as its JSON text
    let delivers = |label: &str, wire: &str| ok(TR, "deliversOwn", &format!(r#"{{"label":{label},"wire":"{wire}"}}"#));
    let row = r#"{"_type":"struct","fields":[{"name":"a","type":{"_type":"scalar","name":"BIGINT"}}]}"#;
    assert_eq!(delivers(row, "JSON"), "true");
    assert_eq!(delivers(row, "VARCHAR"), "false");
    assert_eq!(delivers(r#"{"_type":"scalar","name":"JSON"}"#, "JSON"), "false");
    // the interface's defaults
    assert_eq!(ok(TR, "script", r#"{"statements":["a","b"]}"#), r#""a;\nb;""#);
    assert_eq!(ok(TR, "failingStatement", r#"{"message":"x","statements":["a"]}"#), "null");
    assert_eq!(ok(TR, "copySelectList", r#"{"conversions":[]}"#), r#""*""#);
    let r = refused(TR, "copySelectList", r#"{"conversions":[{"column":"c","sql":"x"}]}"#);
    assert_eq!(r.message, "a copy that converts columns in its star select is not implemented for Trino");
    let r = refused(TR, "catalogColumnsQuery", r#"{"schema":"s","table":"t"}"#);
    assert_eq!((r.kind(), r.message.as_str()), ("UNSUPPORTED_OPERATION", "the platform asks no catalog question of Trino"));
    let column = r#"{"column":{"name":"c","dataType":"bigint","logicalType":null,"precision":null,"scale":null,"notNull":false}}"#;
    let r = refused(TR, "catalogType", column);
    assert_eq!(
        (r.kind(), r.message.as_str()),
        ("DIALECT_CAPABILITY", "reading this database's catalog type 'bigint' is not implemented for Trino")
    );
    let r = refused(TR, "bareNameReaches", r#"{"catalogName":"t"}"#);
    assert_eq!(r.message, "reading this database's catalog names is not implemented for Trino");
}

#[test]
fn host_operations_are_refused_naming_trino() {
    let not_built = "a project's own database on Trino is not built: only DuckDB makes one so far \
                     (docs/ONE_PLATFORM_PROGRAM_2026_10_12.md C5)";
    for op in ["host.openDatabase", "host.useDatabase", "host.closeDatabase"] {
        let r = refused(TR, op, r#"{"name":"p"}"#);
        assert_eq!((r.kind(), r.message.as_str()), ("DIALECT_CAPABILITY", not_built), "{op}");
    }
    let count = r#"{"operation":{"op":"count","from":{"query":"SELECT 1"}}}"#;
    assert_eq!(
        refused(TR, "host.statements", count).message,
        "a host's Count on Trino is not built (docs/ONE_PLATFORM_PROGRAM_2026_10_12.md D5)"
    );
}

#[test]
fn reads_stored_is_the_bases_and_the_other_dialects_ops_are_refused() {
    let reads =
        |kind: &str| ok(TR, "readsStored", &format!(r#"{{"type":{{"_type":"plain","kind":"{kind}"}}}}"#));
    for (kind, read) in [("OTHER", "true"), ("DISTINCT", "true"), ("JSON", "false"), ("VARCHAR", "false")] {
        assert_eq!(reads(kind), read, "{kind}");
    }
    let other = r#"{"type":{"_type":"plain","kind":"OTHER"}}"#;
    assert_eq!(refused(TR, "duckdb.readsStored", other).message, "'duckdb.readsStored' is DuckDb's op, not Trino's");
    assert_eq!(refused(TR, "duckdb.tables", "{}").message, "'duckdb.tables' is DuckDb's op, not Trino's");
    assert_eq!(
        refused(TR, "engine.selectList", &wrap("query", OFFSET_SELECT)).message,
        "'engine.selectList' is an engine-text dialect's op (EngineStyleH2, EngineStyleDB2, EngineStyleComposite), \
         not Trino's"
    );
}
