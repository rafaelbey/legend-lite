// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! Every op of the seam through [`dispatch`] for Postgres, each answer held to what Java's `SqlDialect` answers for it
//! (`Postgres.java`'s overrides, `PostgresHostOperations`, `AnsiSqlRenderer`'s implementations, the interface's
//! defaults naming the dialect), and the two ops every dialect answers for core's same-package tests, `readsStored`
//! and `tableName`. The golden oracles hold the renders (`tests/dialect_differential.rs`).

use super::*;

const PG: &str = r#"{"dialect":"Postgres","server":null}"#;

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
fn facts_are_postgres() {
    assert_eq!(
        ok(PG, "facts", "{}"),
        r#"{"temporaryTables":true,"loadsRowsInBulk":false,"relational":true,"inProcess":false,"#.to_owned()
            + r#""dropsStagedTablesWithTheirTransaction":false,"needsStaticPivot":true,"serverVersions":null,"#
            + r#""inMemoryPath":null,"scriptAbort":"ROLLBACK","sessionSetup":["SET TimeZone='UTC'"],"#
            + r#""keysSnapshot":"REPEATABLE_READ","resultColumnLimit":1664}"#
    );
    // every server version is spelled alike; a spec's server keeps Postgres
    assert_eq!(ok(PG, "forServer", r#"{"version":"16.4"}"#), r#""Postgres""#);
    assert_eq!(dispatch(r#"{"dialect":"Postgres","server":"17.0"}"#, "facts", "{}").unwrap(), ok(PG, "facts", "{}"));
    // an engine-text dialect's configuration is no Postgres's
    assert_eq!(
        refused(r#"{"dialect":"Postgres","timeZone":null}"#, "facts", "{}").message,
        "quoteIdentifiers and timeZone configure an engine-text dialect (EngineStyleH2, EngineStyleDB2), not Postgres"
    );
}

/// A select of one typed plan parameter (`query.renderStatement`).
const PARAM_SELECT: &str = r##"{"_type":"select","projections":[{"expr":{"_type":"planParam","type":{"_type":"typed","sqlType":{"_type":"scalar","name":"TIMESTAMP"},"nullable":false},"name":"#at","kind":"OTHER","optional":false,"enumMapFn":null,"byValue":null},"alias":"value","out":{"name":"value","type":{"_type":"scalar","name":"TIMESTAMP"},"nullable":false,"origin":"DERIVED"}}],"distinct":false,"from":{"_type":"dual"},"where":null,"groupBy":[],"having":null,"qualify":null,"orderBy":[],"limit":null,"offset":null,"outputs":[{"name":"value","type":{"_type":"scalar","name":"TIMESTAMP"},"nullable":false,"origin":"DERIVED"}]}"##;

/// A select of a parameter typed by its value, a date-time finer than a microsecond among its kinds.
const VALUE_TYPED_SELECT: &str = r#"{"_type":"select","projections":[{"expr":{"_type":"planParam","type":{"_type":"unknown"},"name":"t","kind":"OTHER","optional":false,"enumMapFn":null,"byValue":{"kinds":["DATE_TIME","DATE_TIME_NANOS"],"absent":"DATE_TIME"}},"alias":"x","out":null}],"distinct":false,"from":{"_type":"dual"},"where":null,"groupBy":[],"having":null,"qualify":null,"orderBy":[],"limit":null,"offset":null,"outputs":[]}"#;

/// A select over raw SQL written for the engine's H2 (`query.render`).
const RAW_SELECT: &str = r#"{"_type":"select","projections":[],"distinct":false,"from":{"_type":"rawSql","sql":"CREATE TABLE T (A TINYINT)","alias":"_p","outputs":[],"writtenFor":"H2"},"where":null,"groupBy":[],"having":null,"qualify":null,"orderBy":[],"limit":0,"offset":null,"outputs":[]}"#;

#[test]
fn queries_render_as_postgres_writes_them() {
    // a placeholder bound bare; a date-time's to a type hole, the finer one a TIMESTAMP of six digits
    assert_eq!(
        ok(PG, "query.renderStatement", &wrap("query", PARAM_SELECT)),
        r##"{"sql":"SELECT ? AS \"value\"","binds":[{"parameter":"#at","binding":{"_type":"one","nullType":"TIMESTAMP","hole":null}}],"regions":[]}"##
    );
    assert_eq!(
        ok(PG, "query.renderStatement", &wrap("query", VALUE_TYPED_SELECT)),
        r#"{"sql":"SELECT CAST(? AS ) AS \"x\"","binds":[{"parameter":"t","binding":{"_type":"one","nullType":"TIMESTAMP","hole":{"at":17,"types":{"DATE_TIME":{"name":"TIMESTAMP","digits":"NONE","fraction":null},"DATE_TIME_NANOS":{"name":"TIMESTAMP","digits":"NONE","fraction":6}},"absent":"DATE_TIME"}}}],"regions":[]}"#
    );
    // raw text written for the engine's H2 is translated, with Postgres's types and folded names
    assert_eq!(
        ok(PG, "query.render", &wrap("query", RAW_SELECT)),
        r#""SELECT *\nFROM (CREATE TABLE T (\"a\" SMALLINT)) AS \"_p\"\nLIMIT 0""#
    );
    assert_eq!(
        ok(PG, "query.renderInline", &wrap("query", RAW_SELECT)),
        r#""SELECT * FROM (CREATE TABLE T (\"a\" SMALLINT)) AS \"_p\" LIMIT 0""#
    );
    let r = refused(PG, "query.render", &wrap("query", PARAM_SELECT));
    assert_eq!(r.class, Refusal::ILLEGAL_STATE);
    assert!(r.message.starts_with("a statement with bound parameters [Bind[parameter=#at"), "{}", r.message);
}

const CREATE_TABLE: &str = r#"{"_type":"createTable","schema":"S","table":"Trades","quoting":{"schema":false,"table":false},"columns":[{"name":"ID","declaredQuoted":false,"type":{"_type":"plain","kind":"TINYINT"},"notNull":true,"primaryKey":true},{"name":"Amt","declaredQuoted":true,"type":{"_type":"plain","kind":"DOUBLE"},"notNull":false,"primaryKey":false}],"temporary":false}"#;

const INSERT_VALUES: &str = r#"{"_type":"insertValues","schema":null,"table":"Trades","quoting":{"schema":false,"table":false},"columns":["ID","Amt"],"quotedColumns":["Amt"],"rows":[[{"_type":"intLit","type":{"_type":"typed","sqlType":{"_type":"scalar","name":"BIGINT"},"nullable":false},"value":1},{"_type":"nullLit","type":{"_type":"bottom"}}]]}"#;

#[test]
fn ddl_and_dml_render_and_a_session_table_is_its_table_then_its_rows() {
    let create = r#""Create Table s.trades(id SMALLINT NOT NULL, \"Amt\" DOUBLE PRECISION NULL, PRIMARY KEY(id));""#;
    assert_eq!(ok(PG, "ddl.render", &wrap("ddl", CREATE_TABLE)), create);
    let insert = r#""INSERT INTO trades (id, \"Amt\") VALUES (1, NULL);""#;
    assert_eq!(ok(PG, "dml.render", &wrap("dml", INSERT_VALUES)), insert);
    assert_eq!(
        ok(PG, "dml.renderStatement", &wrap("dml", INSERT_VALUES)),
        r#"{"sql":"INSERT INTO trades (id, \"Amt\") VALUES (1, NULL);","binds":[],"regions":[]}"#
    );
    assert_eq!(
        ok(PG, "sessionTable", &format!(r#"{{"table":{CREATE_TABLE},"rows":{INSERT_VALUES}}}"#)),
        format!("[{create},{insert}]")
    );
    let unspelled = CREATE_TABLE.replace(r#""kind":"TINYINT""#, r#""kind":"TIMESTAMP_NS""#);
    let r = refused(PG, "ddl.render", &wrap("ddl", &unspelled));
    assert_eq!(
        (r.kind(), r.message.as_str()),
        ("DIALECT_CAPABILITY", "a TIMESTAMP_NS column: Postgres keeps microseconds")
    );
}

#[test]
fn names_are_postgres() {
    let name = |op: &str, args: &str| ok(PG, op, args);
    assert_eq!(name("physicalName", r#"{"name":"ORDER"}"#), r#""\"order\"""#);
    assert_eq!(name("physicalName", r#"{"name":"Trades"}"#), r#""trades""#);
    assert_eq!(name("physicalName", r#"{"name":"\"Trades\""}"#), r#""\"Trades\"""#);
    assert_eq!(name("storedName", r#"{"name":"Trades","declaredQuoted":true}"#), r#""\"Trades\"""#);
    assert_eq!(name("storedName", r#"{"name":"Trades","declaredQuoted":false}"#), r#""trades""#);
    assert_eq!(name("insertColumnName", r#"{"name":"Amt","declaredQuoted":false}"#), r#""amt""#);
    assert_eq!(name("insertColumnName", r#"{"name":"Amt","declaredQuoted":true}"#), r#""\"Amt\"""#);
    // the engine's H2 text translated; any other text as written
    assert_eq!(
        name("fromEngineText", r#"{"statement":"CREATE TABLE T (X DOUBLE, Y FLOAT)"}"#),
        r#""CREATE TABLE T (\"x\" DOUBLE PRECISION, \"y\" DOUBLE PRECISION)""#
    );
    assert_eq!(
        name("raw", r#"{"statement":"insert into T (A) values (1)","writtenFor":"H2"}"#),
        r#""insert into T (\"a\") values (1)""#
    );
    assert_eq!(name("raw", r#"{"statement":"select 1","writtenFor":null}"#), r#""select 1""#);
    // a same-package test's question: a stored table's name as a query reads it (TempTableAsTest)
    assert_eq!(
        name("tableName", r#"{"name":"legend_stage_rowsA","quoting":{"schema":false,"table":false}}"#),
        r#""legend_stage_rowsa""#
    );
    assert_eq!(
        name("tableName", r#"{"name":"Sales.Orders","quoting":{"schema":true,"table":false}}"#),
        r#""\"Sales\".orders""#
    );
    assert_eq!(refused(PG, "tableName", r#"{"name":"t"}"#).message, "missing field 'quoting'");
}

#[test]
fn the_interface_answers_as_postgres() {
    // the interface's defaults
    assert_eq!(ok(PG, "wireTypeName", r#"{"driverTypeName":"int8"}"#), r#""BIGINT""#);
    assert_eq!(ok(PG, "typingWireName", r#"{"driverTypeName":"int8"}"#), r#""BIGINT""#);
    assert_eq!(ok(PG, "failingStatement", r#"{"message":"x","statements":["a"]}"#), "null");
    assert_eq!(ok(PG, "copySelectList", r#"{"conversions":[]}"#), r#""*""#);
    let r = refused(PG, "copySelectList", r#"{"conversions":[{"column":"c","sql":"x"}]}"#);
    assert_eq!(r.message, "a copy that converts columns in its star select is not implemented for Postgres");
    let r = refused(PG, "catalogColumnsQuery", r#"{"schema":"s","table":"t"}"#);
    assert_eq!(
        (r.kind(), r.message.as_str()),
        ("UNSUPPORTED_OPERATION", "the platform asks no catalog question of Postgres")
    );
    // Postgres's own
    assert_eq!(ok(PG, "script", r#"{"statements":["a","b"]}"#), r#""BEGIN TRANSACTION;\na;\nb;\nCOMMIT;""#);
    assert_eq!(ok(PG, "script", r#"{"statements":[]}"#), r#""BEGIN TRANSACTION;\n;\nCOMMIT;""#);
    let map = r#"{"_type":"map","key":{"_type":"scalar","name":"VARCHAR"},"value":{"_type":"scalar","name":"BIGINT"}}"#;
    assert_eq!(ok(PG, "deliversOwn", &format!(r#"{{"label":{map},"wire":"ARRAY"}}"#)), "true");
    assert_eq!(ok(PG, "deliversOwn", &format!(r#"{{"label":{map},"wire":"VARCHAR"}}"#)), "false");
    assert_eq!(ok(PG, "bareNameReaches", r#"{"catalogName":"orders"}"#), "true");
    assert_eq!(ok(PG, "bareNameReaches", r#"{"catalogName":"Orders"}"#), "false");
    let column = |data: &str, logical: &str| {
        format!(
            r#"{{"column":{{"name":"c","dataType":"{data}","logicalType":"{logical}","precision":null,"scale":null,"notNull":false}}}}"#
        )
    };
    assert_eq!(
        ok(PG, "catalogType", &column("integer", "int4")),
        r#"{"read":"AS_STORED","declared":"INTEGER","conversion":null,"reason":null}"#
    );
    assert_eq!(
        ok(PG, "catalogType", &column("timestamp with time zone", "timestamptz")),
        r#"{"read":"COPY_CONVERTED","declared":"TIMESTAMP","conversion":"CAST(timezone('UTC', %s) AS TIMESTAMP)","reason":null}"#
    );
    assert_eq!(
        ok(PG, "catalogType", &column("numeric", "numeric")),
        r#"{"read":"AS_STORED","declared":"DOUBLE","conversion":null,"reason":null}"#
    );
    assert_eq!(
        ok(PG, "catalogType", &column("bytea", "bytea")),
        r#"{"read":"LEFT_OUT","declared":null,"conversion":null,"reason":"bytes: no Pure Database type holds them"}"#
    );
    let r = refused(PG, "catalogType", &column("geometry", "geometry"));
    assert_eq!(
        (r.kind(), r.message.as_str()),
        (
            "DIALECT_CAPABILITY",
            "a column of Postgres type 'geometry' cannot be declared in a Pure Database (a type this dialect does not \
             read)"
        )
    );
}

#[test]
fn postgres_host_operations_count_a_query_and_refuse_every_other() {
    // HostOperationsPostgresTest: a planner's query counted, its subquery aliased
    let count = r#"{"operation":{"op":"count","from":{"query":"SELECT x FROM g"}}}"#;
    assert_eq!(ok(PG, "host.statements", count), r#"["SELECT count(*) AS n FROM (SELECT x FROM g) AS counted"]"#);
    let not_built = "a project's own database on Postgres is not built: only DuckDB makes one so far \
                     (docs/ONE_PLATFORM_PROGRAM_2026_10_12.md C5)";
    for op in ["host.openDatabase", "host.useDatabase", "host.closeDatabase"] {
        let r = refused(PG, op, r#"{"name":"p"}"#);
        assert_eq!((r.kind(), r.message.as_str()), ("DIALECT_CAPABILITY", not_built), "{op}");
    }
    // a count of a relation, and any other operation, refused by name
    let relation = r#"{"operation":{"op":"count","from":{"relation":{"name":"t"}}}}"#;
    assert_eq!(
        refused(PG, "host.statements", relation).message,
        "a host's Count on Postgres is not built (docs/ONE_PLATFORM_PROGRAM_2026_10_12.md D5)"
    );
    assert_eq!(
        refused(PG, "host.statements", r#"{"operation":{"op":"platform"}}"#).message,
        "a host's Platform on Postgres is not built (docs/ONE_PLATFORM_PROGRAM_2026_10_12.md D5)"
    );
}

#[test]
fn reads_stored_answers_for_every_dialect() {
    let reads =
        |spec: &str, kind: &str| ok(spec, "readsStored", &format!(r#"{{"type":{{"_type":"plain","kind":"{kind}"}}}}"#));
    // Postgres reads a nested value as jsonb, and OTHER as text (StoredReadsTest)
    for (kind, read) in
        [("JSON", "true"), ("ARRAY", "true"), ("OBJECT", "true"), ("OTHER", "true"), ("VARCHAR", "false")]
    {
        assert_eq!(reads(PG, kind), read, "{kind}");
    }
    // every other dialect's rule: DuckDB's and the base's (only OTHER and DISTINCT, as text) ...
    for spec in [r#"{"dialect":"DuckDb"}"#, r#"{"dialect":"H2"}"#, r#"{"dialect":"H2Modern"}"#] {
        assert_eq!(reads(spec, "DISTINCT"), "true", "{spec}");
        assert_eq!(reads(spec, "JSON"), "false", "{spec}");
    }
    // ... and the engine text's override: every column as stored (EngineStyleH2.readsStored, false)
    for spec in
        [r#"{"dialect":"EngineStyleH2"}"#, r#"{"dialect":"EngineStyleDB2"}"#, r#"{"dialect":"EngineStyleComposite"}"#]
    {
        assert_eq!(reads(spec, "DISTINCT"), "false", "{spec}");
        assert_eq!(reads(spec, "OTHER"), "false", "{spec}");
    }
    // DuckDB's own op keeps answering, and only for DuckDB
    let other = r#"{"type":{"_type":"plain","kind":"OTHER"}}"#;
    assert_eq!(ok(r#"{"dialect":"DuckDb"}"#, "duckdb.readsStored", other), "true");
    assert_eq!(refused(PG, "duckdb.readsStored", other).message, "'duckdb.readsStored' is DuckDb's op, not Postgres's");
    assert_eq!(refused(PG, "duckdb.tables", "{}").message, "'duckdb.tables' is DuckDb's op, not Postgres's");
    assert_eq!(refused(PG, "readsStored", "{}").message, "missing field 'type'");
    assert_eq!(
        refused(PG, "engine.inTempTableSelect", r#"{"table":"t"}"#).message,
        "'engine.inTempTableSelect' is an engine-text dialect's op (EngineStyleH2, EngineStyleDB2, \
         EngineStyleComposite), not Postgres's"
    );
}
