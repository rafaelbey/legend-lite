// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! Every op of the seam through [`dispatch`] for SQLite, each answer held to what Java's `SqlDialect` answers for it
//! (`SQLite.java`'s overrides, `AnsiSqlRenderer`'s implementations, the interface's defaults naming the dialect). The
//! golden oracles hold the renders (`tests/dialect_differential.rs`).

use super::*;

const SQ: &str = r#"{"dialect":"SQLite","server":null}"#;

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
fn facts_are_sqlites() {
    assert_eq!(
        ok(SQ, "facts", "{}"),
        r#"{"temporaryTables":true,"loadsRowsInBulk":false,"relational":true,"inProcess":true,"#.to_owned()
            + r#""dropsStagedTablesWithTheirTransaction":false,"needsStaticPivot":true,"serverVersions":null,"#
            + r#""inMemoryPath":null,"scriptAbort":null,"sessionSetup":["PRAGMA case_sensitive_like = ON"],"#
            + r#""keysSnapshot":"TRANSACTION","resultColumnLimit":2000}"#
    );
    // every server version is spelled alike; a spec's server keeps SQLite
    assert_eq!(ok(SQ, "forServer", r#"{"version":"3.47.1"}"#), r#""SQLite""#);
    assert_eq!(dispatch(r#"{"dialect":"SQLite","server":"3.0"}"#, "facts", "{}").unwrap(), ok(SQ, "facts", "{}"));
    // an engine-text dialect's configuration is no SQLite's
    assert_eq!(
        refused(r#"{"dialect":"SQLite","quoteIdentifiers":false}"#, "facts", "{}").message,
        "quoteIdentifiers and timeZone configure an engine-text dialect (EngineStyleH2, EngineStyleDB2), not SQLite"
    );
}

/// A select of a parameter typed by its value, a date and a date-time among its kinds.
const VALUE_TYPED_SELECT: &str = r#"{"_type":"select","projections":[{"expr":{"_type":"planParam","type":{"_type":"unknown"},"name":"t","kind":"OTHER","optional":false,"enumMapFn":null,"byValue":{"kinds":["DATE","DATE_TIME_NANOS"],"absent":"DATE"}},"alias":"x","out":null}],"distinct":false,"from":{"_type":"dual"},"where":null,"groupBy":[],"having":null,"qualify":null,"orderBy":[],"limit":null,"offset":null,"outputs":[]}"#;

/// A select over raw SQL written for the engine's H2 (`query.render`).
const RAW_SELECT: &str = r#"{"_type":"select","projections":[],"distinct":false,"from":{"_type":"rawSql","sql":"select 1","alias":"_p","outputs":[],"writtenFor":"H2"},"where":null,"groupBy":[],"having":null,"qualify":null,"orderBy":[],"limit":0,"offset":null,"outputs":[]}"#;

/// `SELECT TRUE AS "b"` with an offset and no limit.
const OFFSET_SELECT: &str = r#"{"_type":"select","projections":[{"expr":{"_type":"boolLit","type":{"_type":"typed","sqlType":{"_type":"scalar","name":"BOOLEAN"},"nullable":false},"value":true},"alias":"b","out":null}],"distinct":false,"from":{"_type":"dual"},"where":null,"groupBy":[],"having":null,"qualify":null,"orderBy":[],"limit":null,"offset":3,"outputs":[]}"#;

#[test]
fn queries_render_as_sqlite_writes_them() {
    // a date and a date-time are bound as their text: the type hole is TEXT for both
    assert_eq!(
        ok(SQ, "query.renderStatement", &wrap("query", VALUE_TYPED_SELECT)),
        r#"{"sql":"SELECT CAST(? AS ) AS x","binds":[{"parameter":"t","binding":{"_type":"one","nullType":"DATE","hole":{"at":17,"types":{"DATE":{"name":"TEXT","digits":"NONE","fraction":null},"DATE_TIME_NANOS":{"name":"TEXT","digits":"NONE","fraction":null}},"absent":"DATE"}}}],"regions":[]}"#
    );
    // a boolean is 1 or 0; an OFFSET takes a LIMIT of -1
    assert_eq!(ok(SQ, "query.render", &wrap("query", OFFSET_SELECT)), r#""SELECT 1 AS b\nLIMIT -1\nOFFSET 3""#);
    assert_eq!(ok(SQ, "query.renderInline", &wrap("query", OFFSET_SELECT)), r#""SELECT 1 AS b LIMIT -1 OFFSET 3""#);
    // raw text written for the engine's H2 is refused: SQLite has no translation of it
    let r = refused(SQ, "query.render", &wrap("query", RAW_SELECT));
    assert_eq!(
        (r.kind(), r.message.as_str()),
        ("DIALECT_CAPABILITY", "the engine's H2 text reached SQLite, which has no translation of it")
    );
}

const CREATE_TABLE: &str = r#"{"_type":"createTable","schema":"S","table":"Trades","quoting":{"schema":false,"table":false},"columns":[{"name":"ID","declaredQuoted":false,"type":{"_type":"plain","kind":"TINYINT"},"notNull":true,"primaryKey":true},{"name":"Amt","declaredQuoted":true,"type":{"_type":"plain","kind":"DOUBLE"},"notNull":false,"primaryKey":false}],"temporary":false}"#;

const INSERT_VALUES: &str = r#"{"_type":"insertValues","schema":null,"table":"Trades","quoting":{"schema":false,"table":false},"columns":["ID","Amt"],"quotedColumns":["Amt"],"rows":[[{"_type":"intLit","type":{"_type":"typed","sqlType":{"_type":"scalar","name":"BIGINT"},"nullable":false},"value":1},{"_type":"nullLit","type":{"_type":"bottom"}}]]}"#;

#[test]
fn ddl_and_dml_render_and_a_session_table_is_its_table_then_its_rows() {
    // a schema's table is one delimited name, "schema.table"; a schema itself is no statement
    let create = r#""Create Table \"S.Trades\"(ID INTEGER NOT NULL, \"Amt\" REAL NULL, PRIMARY KEY(ID));""#;
    assert_eq!(ok(SQ, "ddl.render", &wrap("ddl", CREATE_TABLE)), create);
    assert_eq!(ok(SQ, "ddl.render", r#"{"ddl":{"_type":"createSchema","schema":"S","quoted":false}}"#), r#""SELECT 1;""#);
    assert_eq!(ok(SQ, "ddl.render", r#"{"ddl":{"_type":"dropSchema","schema":"S","quoted":true}}"#), r#""SELECT 1;""#);
    let insert = r#""INSERT INTO Trades (ID, \"Amt\") VALUES (1, NULL);""#;
    assert_eq!(ok(SQ, "dml.render", &wrap("dml", INSERT_VALUES)), insert);
    assert_eq!(
        ok(SQ, "dml.renderStatement", &wrap("dml", INSERT_VALUES)),
        r#"{"sql":"INSERT INTO Trades (ID, \"Amt\") VALUES (1, NULL);","binds":[],"regions":[]}"#
    );
    assert_eq!(
        ok(SQ, "sessionTable", &format!(r#"{{"table":{CREATE_TABLE},"rows":{INSERT_VALUES}}}"#)),
        format!("[{create},{insert}]")
    );
    // the store writes are refused by the base, naming SQLite
    let delete = r#"{"dml":{"_type":"deleteWhere","schema":null,"table":"T","quoting":{"schema":false,"table":false},"alias":"t","storedTypes":{},"where":null}}"#;
    let r = refused(SQ, "dml.render", delete);
    assert_eq!(
        (r.kind(), r.message.as_str()),
        (
            "DIALECT_CAPABILITY",
            "a DELETE ... WHERE on SQLite: no spelling measured on its database (the store writes run on DuckDB, H2 \
             and Postgres, the writes design's leg 2)"
        )
    );
}

#[test]
fn names_are_sqlites() {
    let name = |op: &str, args: &str| ok(SQ, op, args);
    assert_eq!(name("physicalName", r#"{"name":"ORDER"}"#), r#""\"ORDER\"""#);
    assert_eq!(name("physicalName", r#"{"name":"Trades"}"#), r#""Trades""#);
    assert_eq!(name("physicalName", r#"{"name":"a b"}"#), r#""\"a b\"""#);
    assert_eq!(name("storedName", r#"{"name":"Trades","declaredQuoted":true}"#), r#""\"Trades\"""#);
    assert_eq!(name("storedName", r#"{"name":"Trades","declaredQuoted":false}"#), r#""Trades""#);
    assert_eq!(name("insertColumnName", r#"{"name":"key","declaredQuoted":false}"#), r#""\"key\"""#);
    assert_eq!(name("insertColumnName", r#"{"name":"Amt","declaredQuoted":true}"#), r#""\"Amt\"""#);
    // the engine's H2 text is refused; any other text as written
    let r = refused(SQ, "fromEngineText", r#"{"statement":"CREATE TABLE T (X DOUBLE)"}"#);
    assert_eq!(
        (r.kind(), r.message.as_str()),
        ("DIALECT_CAPABILITY", "the engine's H2 text reached SQLite, which has no translation of it")
    );
    let r = refused(SQ, "raw", r#"{"statement":"insert into T (A) values (1)","writtenFor":"H2"}"#);
    assert_eq!(r.message, "the engine's H2 text reached SQLite, which has no translation of it");
    assert_eq!(name("raw", r#"{"statement":"select 1","writtenFor":"SQLite"}"#), r#""select 1""#);
    // a same-package test's question: a stored table's name as a query reads it (TempTableAsTest)
    assert_eq!(
        name("tableName", r#"{"name":"legend_stage_rowsA","quoting":{"schema":false,"table":false}}"#),
        r#""legend_stage_rowsA""#
    );
    assert_eq!(
        name("tableName", r#"{"name":"Sales.Orders","quoting":{"schema":false,"table":false}}"#),
        r#""\"Sales.Orders\"""#
    );
    assert_eq!(name("tableName", r#"{"name":".x","quoting":{"schema":false,"table":true}}"#), r#""\".x\"""#);
}

#[test]
fn the_interface_answers_as_sqlite() {
    // its FLOAT is a REAL value, and NUMERIC its name for a NULL value
    assert_eq!(ok(SQ, "wireTypeName", r#"{"driverTypeName":"FLOAT"}"#), r#""DOUBLE""#);
    assert_eq!(ok(SQ, "wireTypeName", r#"{"driverTypeName":"NUMERIC"}"#), r#""NULL""#);
    assert_eq!(ok(SQ, "wireTypeName", r#"{"driverTypeName":"TEXT"}"#), r#""VARCHAR""#);
    assert_eq!(ok(SQ, "wireTypeName", r#"{"driverTypeName":null}"#), r#""""#);
    assert_eq!(ok(SQ, "typingWireName", r#"{"driverTypeName":"NUMERIC"}"#), r#""NULL""#);
    // its own types: a NULL first value claims none; a boolean and a big integer an INTEGER; text the rest
    let delivers = |label: &str, wire: &str| ok(SQ, "deliversOwn", &format!(r#"{{"label":{label},"wire":"{wire}"}}"#));
    let scalar = |name: &str| format!(r#"{{"_type":"scalar","name":"{name}"}}"#);
    assert_eq!(delivers(&scalar("BIGINT"), "NULL"), "true");
    assert_eq!(delivers(&scalar("BOOLEAN"), "INTEGER"), "true");
    assert_eq!(delivers(&scalar("HUGEINT"), "INTEGER"), "true");
    assert_eq!(delivers(&scalar("BIGINT"), "INTEGER"), "false");
    assert_eq!(delivers(&scalar("DATE"), "VARCHAR"), "true");
    assert_eq!(delivers(&scalar("TIMESTAMPTZ"), "VARCHAR"), "true");
    assert_eq!(delivers(&scalar("VARCHAR"), "VARCHAR"), "false");
    assert_eq!(delivers(r#"{"_type":"decimal","precision":10,"scale":2}"#, "VARCHAR"), "true");
    assert_eq!(delivers(r#"{"_type":"array","element":{"_type":"scalar","name":"BIGINT"}}"#, "VARCHAR"), "true");
    assert_eq!(delivers(&scalar("DOUBLE"), "DOUBLE"), "false");
    // the interface's defaults
    assert_eq!(ok(SQ, "script", r#"{"statements":["a","b"]}"#), r#""a;\nb;""#);
    assert_eq!(ok(SQ, "failingStatement", r#"{"message":"x","statements":["a"]}"#), "null");
    assert_eq!(ok(SQ, "copySelectList", r#"{"conversions":[]}"#), r#""*""#);
    let r = refused(SQ, "copySelectList", r#"{"conversions":[{"column":"c","sql":"x"}]}"#);
    assert_eq!(r.message, "a copy that converts columns in its star select is not implemented for SQLite");
    let r = refused(SQ, "catalogColumnsQuery", r#"{"schema":"s","table":"t"}"#);
    assert_eq!((r.kind(), r.message.as_str()), ("UNSUPPORTED_OPERATION", "the platform asks no catalog question of SQLite"));
    let column = r#"{"column":{"name":"c","dataType":"INTEGER","logicalType":null,"precision":null,"scale":null,"notNull":false}}"#;
    let r = refused(SQ, "catalogType", column);
    assert_eq!(
        (r.kind(), r.message.as_str()),
        ("DIALECT_CAPABILITY", "reading this database's catalog type 'INTEGER' is not implemented for SQLite")
    );
    let r = refused(SQ, "bareNameReaches", r#"{"catalogName":"t"}"#);
    assert_eq!(r.message, "reading this database's catalog names is not implemented for SQLite");
}

#[test]
fn host_operations_are_refused_naming_sqlite() {
    let not_built = "a project's own database on SQLite is not built: only DuckDB makes one so far \
                     (docs/ONE_PLATFORM_PROGRAM_2026_10_12.md C5)";
    for op in ["host.openDatabase", "host.useDatabase", "host.closeDatabase"] {
        let r = refused(SQ, op, r#"{"name":"p"}"#);
        assert_eq!((r.kind(), r.message.as_str()), ("DIALECT_CAPABILITY", not_built), "{op}");
    }
    let count = r#"{"operation":{"op":"count","from":{"query":"SELECT 1"}}}"#;
    assert_eq!(
        refused(SQ, "host.statements", count).message,
        "a host's Count on SQLite is not built (docs/ONE_PLATFORM_PROGRAM_2026_10_12.md D5)"
    );
}

#[test]
fn reads_stored_is_the_bases_and_the_other_dialects_ops_are_refused() {
    let reads =
        |kind: &str| ok(SQ, "readsStored", &format!(r#"{{"type":{{"_type":"plain","kind":"{kind}"}}}}"#));
    for (kind, read) in [("OTHER", "true"), ("DISTINCT", "true"), ("JSON", "false"), ("VARCHAR", "false")] {
        assert_eq!(reads(kind), read, "{kind}");
    }
    let other = r#"{"type":{"_type":"plain","kind":"OTHER"}}"#;
    assert_eq!(refused(SQ, "duckdb.readsStored", other).message, "'duckdb.readsStored' is DuckDb's op, not SQLite's");
    assert_eq!(refused(SQ, "duckdb.tables", "{}").message, "'duckdb.tables' is DuckDb's op, not SQLite's");
    assert_eq!(
        refused(SQ, "engine.renderedAlias", r#"{"alias":"a","rendered":null}"#).message,
        "'engine.renderedAlias' is an engine-text dialect's op (EngineStyleH2, EngineStyleDB2, EngineStyleComposite), \
         not SQLite's"
    );
}
