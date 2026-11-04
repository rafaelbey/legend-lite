// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! Every op of the seam through [`dispatch`] for MariaDb and MySql, each answer held to what Java's `SqlDialect`
//! answers for it (`MariaDb.java`'s and `MySql.java`'s overrides, `AnsiSqlRenderer`'s implementations, the interface's
//! defaults naming the dialect). The golden oracles hold the renders (`tests/dialect_differential.rs`).

use super::*;

const MARIA: &str = r#"{"dialect":"MariaDb","server":null}"#;
const MY: &str = r#"{"dialect":"MySql","server":null}"#;

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

/// Each dialect's spec and its simple name.
const BOTH: [(&str, &str); 2] = [(MARIA, "MariaDb"), (MY, "MySql")];

#[test]
fn facts_are_each_databases() {
    let tail = r#""keysSnapshot":"REPEATABLE_READ","resultColumnLimit":4096}"#;
    let head = r#"{"temporaryTables":true,"loadsRowsInBulk":false,"relational":true,"inProcess":false,"#.to_owned()
        + r#""dropsStagedTablesWithTheirTransaction":false,"needsStaticPivot":true,"serverVersions":null,"#
        + r#""inMemoryPath":null,"scriptAbort":null,"sessionSetup":["#;
    let mode = "SET SESSION sql_mode = 'ANSI_QUOTES,PIPES_AS_CONCAT,NO_BACKSLASH_ESCAPES,STRICT_ALL_TABLES,\
                NO_ENGINE_SUBSTITUTION";
    let common = r#""SET SESSION time_zone = '+00:00'","SET NAMES utf8mb4 COLLATE utf8mb4_bin","#.to_owned()
        + r#""SET SESSION group_concat_max_len = 1073741824","#;
    assert_eq!(
        ok(MARIA, "facts", "{}"),
        format!(
            "{head}\"SET SESSION optimizer_switch = 'condition_pushdown_for_derived=off'\",\"{mode}'\",{common}\"SET \
             SESSION max_recursive_iterations = 100000000\",\"SET SESSION div_precision_increment = 30\"],{tail}"
        )
    );
    assert_eq!(
        ok(MY, "facts", "{}"),
        format!(
            "{head}\"{mode},NO_UNSIGNED_SUBTRACTION'\",{common}\"SET SESSION cte_max_recursion_depth = 100000000\",\
             \"SET SESSION div_precision_increment = 30\"],{tail}"
        )
    );
    for (spec, name) in BOTH {
        // every server version is spelled alike; a spec's server keeps the dialect
        assert_eq!(ok(spec, "forServer", r#"{"version":"11.4.13"}"#), format!("\"{name}\""));
        let served = spec.replace("\"server\":null", "\"server\":\"8.4.3\"");
        assert_eq!(dispatch(&served, "facts", "{}").unwrap(), ok(spec, "facts", "{}"));
        let configured = spec.replace("\"server\":null", "\"quoteIdentifiers\":true");
        assert_eq!(
            refused(&configured, "facts", "{}").message,
            format!(
                "quoteIdentifiers and timeZone configure an engine-text dialect (EngineStyleH2, EngineStyleDB2), not \
                 {name}"
            )
        );
    }
}

/// `SELECT 1 AS "b", 'it''s' AS "s"` with an offset and a limit.
const SELECT: &str = r#"{"_type":"select","projections":[{"expr":{"_type":"intLit","type":{"_type":"typed","sqlType":{"_type":"scalar","name":"BIGINT"},"nullable":false},"value":1},"alias":"b","out":null},{"expr":{"_type":"stringLit","type":{"_type":"typed","sqlType":{"_type":"scalar","name":"VARCHAR"},"nullable":false},"value":"it's"},"alias":"s","out":null}],"distinct":false,"from":{"_type":"dual"},"where":null,"groupBy":[],"having":null,"qualify":null,"orderBy":[],"limit":5,"offset":3,"outputs":[]}"#;

/// `SELECT 1 AS "b"` with an offset and no limit.
const OFFSET_ONLY: &str = r#"{"_type":"select","projections":[{"expr":{"_type":"intLit","type":{"_type":"typed","sqlType":{"_type":"scalar","name":"BIGINT"},"nullable":false},"value":1},"alias":"b","out":null}],"distinct":false,"from":{"_type":"dual"},"where":null,"groupBy":[],"having":null,"qualify":null,"orderBy":[],"limit":null,"offset":3,"outputs":[]}"#;

#[test]
fn queries_render_as_each_database_writes_them() {
    // every identifier quoted; MariaDB doubles a quote, MySQL writes it as its own character
    assert_eq!(
        ok(MARIA, "query.render", &wrap("query", SELECT)),
        r#""SELECT 1 AS \"b\", 'it''s' AS \"s\"\nLIMIT 5\nOFFSET 3""#
    );
    assert_eq!(
        ok(MY, "query.render", &wrap("query", SELECT)),
        r#""SELECT 1 AS \"b\", CONCAT('', 'it', CHAR(39 USING utf8mb4), 's') AS \"s\"\nLIMIT 5\nOFFSET 3""#
    );
    assert_eq!(
        ok(MARIA, "query.renderInline", &wrap("query", SELECT)),
        r#""SELECT 1 AS \"b\", 'it''s' AS \"s\" LIMIT 5 OFFSET 3""#
    );
    assert_eq!(
        ok(MARIA, "query.renderStatement", &wrap("query", SELECT)),
        r#"{"sql":"SELECT 1 AS \"b\", 'it''s' AS \"s\"\nLIMIT 5\nOFFSET 3","binds":[],"regions":[]}"#
    );
    // an OFFSET takes a LIMIT here
    for (spec, _) in BOTH {
        assert_eq!(
            ok(spec, "query.render", &wrap("query", OFFSET_ONLY)),
            r#""SELECT 1 AS \"b\"\nLIMIT 9223372036854775807\nOFFSET 3""#
        );
    }
}

const CREATE_TABLE: &str = r#"{"_type":"createTable","schema":"S","table":"Trades","quoting":{"schema":false,"table":false},"columns":[{"name":"ID","declaredQuoted":false,"type":{"_type":"plain","kind":"TINYINT"},"notNull":true,"primaryKey":true},{"name":"Amt","declaredQuoted":true,"type":{"_type":"scaled","kind":"DECIMAL","precision":40,"scale":2},"notNull":false,"primaryKey":false},{"name":"J","declaredQuoted":false,"type":{"_type":"plain","kind":"JSON"},"notNull":false,"primaryKey":false},{"name":"B","declaredQuoted":false,"type":{"_type":"sized","kind":"VARBINARY","size":16},"notNull":false,"primaryKey":false},{"name":"V","declaredQuoted":false,"type":{"_type":"sized","kind":"VARCHAR","size":200},"notNull":false,"primaryKey":false}],"temporary":false}"#;

const SESSION_TABLE: &str = r#"{"_type":"createTable","schema":null,"table":"legend_rows","quoting":{"schema":false,"table":false},"columns":[{"name":"ID","declaredQuoted":false,"type":{"_type":"plain","kind":"BIGINT"},"notNull":true,"primaryKey":true}],"temporary":true}"#;

const INSERT_VALUES: &str = r#"{"_type":"insertValues","schema":null,"table":"legend_rows","quoting":{"schema":false,"table":false},"columns":["ID"],"quotedColumns":[],"rows":[[{"_type":"intLit","type":{"_type":"typed","sqlType":{"_type":"scalar","name":"BIGINT"},"nullable":false},"value":1}]]}"#;

#[test]
fn ddl_and_dml_render_and_a_session_table_is_a_temporary_table() {
    for (spec, _) in BOTH {
        // the base's DDL in MariaDB's types; every identifier quoted
        let create = r#""Create Table \"S\".\"Trades\"(\"ID\" TINYINT NOT NULL, \"Amt\" DECIMAL(40,2) NULL, \"J\" LONGTEXT NULL, \"B\" LONGBLOB NULL, \"V\" VARCHAR(200) NULL, PRIMARY KEY(\"ID\"));""#;
        assert_eq!(ok(spec, "ddl.render", &wrap("ddl", CREATE_TABLE)), create);
        let insert = r#""INSERT INTO \"legend_rows\" (\"ID\") VALUES (1);""#;
        assert_eq!(ok(spec, "dml.render", &wrap("dml", INSERT_VALUES)), insert);
        // the interface's default: the table, then its INSERT
        let temporary = r#""Create Temporary Table \"legend_rows\"(\"ID\" BIGINT NOT NULL, PRIMARY KEY(\"ID\"));""#;
        assert_eq!(
            ok(spec, "sessionTable", &format!(r#"{{"table":{SESSION_TABLE},"rows":{INSERT_VALUES}}}"#)),
            format!("[{temporary},{insert}]")
        );
        assert_eq!(
            ok(spec, "sessionTable", &format!(r#"{{"table":{SESSION_TABLE},"rows":null}}"#)),
            format!("[{temporary}]")
        );
        let delete =
            r#"{"dml":{"_type":"deleteAll","schema":"default","table":"T","quoting":{"schema":false,"table":true}}}"#;
        assert_eq!(ok(spec, "dml.render", delete), r#""DELETE FROM \"T\";""#);
        assert_eq!(ok(spec, "dml.renderStatement", delete), r#"{"sql":"DELETE FROM \"T\";","binds":[],"regions":[]}"#);
        // a nanosecond date-time column: MariaDB's DATETIME keeps microseconds
        let ns = CREATE_TABLE.replace(r#""kind":"TINYINT""#, r#""kind":"TIMESTAMP_NS""#);
        let r = refused(spec, "ddl.render", &wrap("ddl", &ns));
        assert_eq!(
            (r.kind(), r.message.as_str()),
            ("DIALECT_CAPABILITY", "a TIMESTAMP_NS column: MariaDB keeps microseconds")
        );
    }
}

#[test]
fn names_are_each_databases() {
    for (spec, name) in BOTH {
        let answer = |op: &str, args: &str| ok(spec, op, args);
        assert_eq!(answer("physicalName", r#"{"name":"Trades"}"#), r#""\"Trades\"""#);
        // a quote-bearing declaration is already its own spelling
        assert_eq!(answer("physicalName", r#"{"name":"\"date\""}"#), r#""\"date\"""#);
        assert_eq!(answer("storedName", r#"{"name":"a\"b","declaredQuoted":true}"#), r#""\"a\"\"b\"""#);
        assert_eq!(answer("storedName", r#"{"name":"select","declaredQuoted":false}"#), r#""\"select\"""#);
        assert_eq!(answer("insertColumnName", r#"{"name":"x","declaredQuoted":false}"#), r#""\"x\"""#);
        // the engine's H2 text: no translation (the refusal names MariaDB on MySQL too, as Java's)
        let r = refused(spec, "fromEngineText", r#"{"statement":"CREATE TABLE T (X DOUBLE)"}"#);
        assert_eq!(
            (r.kind(), r.message.as_str()),
            ("DIALECT_CAPABILITY", "the engine's H2 text reached MariaDB, which has no translation of it")
        );
        assert_eq!(answer("raw", &format!(r#"{{"statement":"select 1","writtenFor":"{name}"}}"#)), r#""select 1""#);
        assert_eq!(answer("raw", r#"{"statement":"select 1","writtenFor":null}"#), r#""select 1""#);
        let r = refused(spec, "raw", r#"{"statement":"select 1","writtenFor":"H2"}"#);
        assert_eq!(r.message, "the engine's H2 text reached MariaDB, which has no translation of it");
        assert_eq!(
            answer("tableName", r#"{"name":"Sales.Orders","quoting":{"schema":false,"table":false}}"#),
            r#""\"Sales\".\"Orders\"""#
        );
    }
}

#[test]
fn the_interface_answers_as_each_database() {
    for (spec, name) in BOTH {
        let answer = |op: &str, args: &str| ok(spec, op, args);
        // its text types and a UUID are text, its DATETIME the naive date-time; every other name the wire's fold
        assert_eq!(answer("wireTypeName", r#"{"driverTypeName":"LONGTEXT"}"#), r#""VARCHAR""#);
        assert_eq!(answer("wireTypeName", r#"{"driverTypeName":"UUID"}"#), r#""VARCHAR""#);
        assert_eq!(answer("wireTypeName", r#"{"driverTypeName":"DATETIME"}"#), r#""TIMESTAMP""#);
        assert_eq!(answer("wireTypeName", r#"{"driverTypeName":"BIGINT"}"#), r#""BIGINT""#);
        assert_eq!(answer("typingWireName", r#"{"driverTypeName":"CHAR"}"#), r#""VARCHAR""#);
        assert_eq!(answer("wireTypeName", r#"{"driverTypeName":null}"#), r#""""#);
        let delivers =
            |label: &str, wire: &str| answer("deliversOwn", &format!(r#"{{"label":{label},"wire":"{wire}"}}"#));
        let bigint = r#"{"_type":"scalar","name":"BIGINT"}"#;
        let boolean = r#"{"_type":"scalar","name":"BOOLEAN"}"#;
        let decimal = r#"{"_type":"decimal","precision":10,"scale":2}"#;
        let list = r#"{"_type":"array","element":{"_type":"scalar","name":"BIGINT"}}"#;
        assert_eq!(delivers(bigint, "DECIMAL(65,0)"), "true");
        assert_eq!(delivers(bigint, "BIGINT UNSIGNED"), "true");
        assert_eq!(delivers(bigint, "DECIMAL(65,2)"), "false");
        assert_eq!(delivers(boolean, "INTEGER"), "true");
        assert_eq!(delivers(decimal, "DECIMAL(12,2)"), "true");
        assert_eq!(delivers(decimal, "DECIMAL(12,3)"), "false");
        assert_eq!(delivers(list, "JSON"), "true");
        assert_eq!(delivers(list, "VARCHAR"), "true");
        assert_eq!(delivers(bigint, "NULL"), "true");
        assert_eq!(answer("script", r#"{"statements":["a","b"]}"#), r#""a;\nb;""#);
        assert_eq!(answer("failingStatement", r#"{"message":"x","statements":["a"]}"#), "null");
        let r = refused(spec, "catalogColumnsQuery", r#"{"schema":"s","table":"t"}"#);
        assert_eq!(
            (r.kind(), r.message.clone()),
            ("UNSUPPORTED_OPERATION", format!("the platform asks no catalog question of {name}"))
        );
        let r = refused(spec, "bareNameReaches", r#"{"catalogName":"t"}"#);
        assert_eq!(r.message, format!("reading this database's catalog names is not implemented for {name}"));
        let column = r#"{"column":{"name":"n","dataType":"BIGINT","logicalType":null,"precision":19,"scale":0,"notNull":false}}"#;
        let r = refused(spec, "catalogType", column);
        assert_eq!(r.message, format!("reading this database's catalog type 'BIGINT' is not implemented for {name}"));
        assert_eq!(answer("copySelectList", r#"{"conversions":[]}"#), r#""*""#);
        let r = refused(spec, "copySelectList", r#"{"conversions":[{"column":"c","sql":"x"}]}"#);
        assert_eq!(r.message, format!("a copy that converts columns in its star select is not implemented for {name}"));
        for op in ["host.openDatabase", "host.useDatabase", "host.closeDatabase"] {
            let r = refused(spec, op, r#"{"name":"p"}"#);
            assert!(r.message.contains(&format!("on {name}")), "{op}: {r}");
        }
        let r = refused(spec, "host.statements", r#"{"operation":{"op":"platform"}}"#);
        assert!(r.message.contains(&format!(" on {name} is not built")), "{r}");
        let reads = |kind: &str| answer("readsStored", &format!(r#"{{"type":{{"_type":"plain","kind":"{kind}"}}}}"#));
        assert_eq!(reads("OTHER"), "true");
        assert_eq!(reads("DISTINCT"), "true");
        assert_eq!(reads("JSON"), "false");
        assert_eq!(
            refused(spec, "duckdb.tables", "{}").message,
            format!("'duckdb.tables' is DuckDb's op, not {name}'s")
        );
        assert_eq!(
            refused(spec, "engine.selectList", &wrap("query", SELECT)).message,
            format!(
                "'engine.selectList' is an engine-text dialect's op (EngineStyleH2, EngineStyleDB2, \
                 EngineStyleComposite), not {name}'s"
            )
        );
    }
}
