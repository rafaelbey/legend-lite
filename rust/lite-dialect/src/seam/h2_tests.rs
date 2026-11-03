// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! Every op of the seam through [`dispatch`] for H2 and H2Modern, each answer held to what Java's `SqlDialect`
//! answers for them (`H2.java`'s and `H2Modern.java`'s overrides, `AnsiSqlRenderer`'s implementations, the
//! interface's defaults naming the dialect). The golden oracles hold the renders (`tests/dialect_differential.rs`).

use super::*;

const H2: &str = r#"{"dialect":"H2","server":null}"#;
const H2_MODERN: &str = r#"{"dialect":"H2Modern","server":null}"#;

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

/// The facts both share, before `serverVersions`.
const SHARED_FACTS: &str = r#"{"temporaryTables":true,"loadsRowsInBulk":false,"relational":true,"inProcess":true,"dropsStagedTablesWithTheirTransaction":true,"needsStaticPivot":true,"serverVersions":"#;

/// The facts both share, after `serverVersions`.
const SHARED_TAIL: &str = r#","inMemoryPath":null,"scriptAbort":null,"sessionSetup":[],"keysSnapshot":"REPEATABLE_READ","resultColumnLimit":16384}"#;

const MODERN_REFUSES_SERVERS: &str = r#"{"refused":{"kind":"DIALECT_CAPABILITY","message":"H2Modern: a plan is written for H2's servers [2.1, 2.2], and H2Modern is chosen for any other at run time"}}"#;

#[test]
fn facts_are_h2s_and_h2moderns() {
    assert_eq!(ok(H2, "facts", "{}"), format!(r#"{SHARED_FACTS}["2.1","2.2"]{SHARED_TAIL}"#));
    // H2Modern is chosen at run time, never planned with: its server versions are refused, not null
    assert_eq!(ok(H2_MODERN, "facts", "{}"), format!("{SHARED_FACTS}{MODERN_REFUSES_SERVERS}{SHARED_TAIL}"));
}

#[test]
fn a_server_chooses_h2_or_h2modern_by_its_versions_prefix() {
    let for_server = |spec: &str, version: &str| ok(spec, "forServer", &format!(r#"{{"version":"{version}"}}"#));
    // H2.SERVERS.stream().anyMatch(version::startsWith) ? this : new H2Modern()
    assert_eq!(for_server(H2, "2.1.214"), r#""H2""#);
    assert_eq!(for_server(H2, "2.2.224"), r#""H2""#);
    assert_eq!(for_server(H2, "2.3.232"), r#""H2Modern""#);
    assert_eq!(for_server(H2, "2.4.240"), r#""H2Modern""#);
    assert_eq!(for_server(H2, ""), r#""H2Modern""#);
    // a prefix, as Java's startsWith: 2.10 starts with 2.1
    assert_eq!(for_server(H2, "2.10"), r#""H2""#);
    // H2Modern is already the 2.3+ profile; DuckDB keeps itself
    assert_eq!(for_server(H2_MODERN, "2.1.214"), r#""H2Modern""#);
    assert_eq!(for_server(H2_MODERN, "2.4.240"), r#""H2Modern""#);
    assert_eq!(for_server(r#"{"dialect":"DuckDb"}"#, "1.4.1"), r#""DuckDb""#);
    assert_eq!(refused(H2, "forServer", "{}").message, "missing field 'version'");
    assert_eq!(refused(H2, "forServer", r#"{"version":null}"#).message, "field 'version' is null, not a string");
    // a spec naming a server opens the dialect for it
    assert_eq!(
        dispatch(r#"{"dialect":"H2","server":"2.3.232"}"#, "facts", "{}").unwrap(),
        ok(H2_MODERN, "facts", "{}")
    );
    assert_eq!(dispatch(r#"{"dialect":"H2","server":"2.1.214"}"#, "facts", "{}").unwrap(), ok(H2, "facts", "{}"));
    assert_eq!(
        dispatch(r#"{"dialect":"H2","server":"2.3.232"}"#, "forServer", r#"{"version":"2.1"}"#).unwrap(),
        r#""H2Modern""#
    );
}

/// A select over raw SQL (`query.render`).
const RAW_SELECT: &str = r#"{"_type":"select","projections":[],"distinct":false,"from":{"_type":"rawSql","sql":"select 3 as V","alias":"_p","outputs":[],"writtenFor":"H2"},"where":null,"groupBy":[],"having":null,"qualify":null,"orderBy":[],"limit":0,"offset":null,"outputs":[]}"#;

/// A select of one typed plan parameter (`query.renderStatement`).
const PARAM_SELECT: &str = r##"{"_type":"select","projections":[{"expr":{"_type":"planParam","type":{"_type":"typed","sqlType":{"_type":"scalar","name":"TIMESTAMP"},"nullable":false},"name":"#at","kind":"OTHER","optional":false,"enumMapFn":null,"byValue":null},"alias":"value","out":{"name":"value","type":{"_type":"scalar","name":"TIMESTAMP"},"nullable":false,"origin":"DERIVED"}}],"distinct":false,"from":{"_type":"dual"},"where":null,"groupBy":[],"having":null,"qualify":null,"orderBy":[],"limit":null,"offset":null,"outputs":[{"name":"value","type":{"_type":"scalar","name":"TIMESTAMP"},"nullable":false,"origin":"DERIVED"}]}"##;

/// A select of a parameter typed by its value (`query.renderStatement`).
const VALUE_TYPED_SELECT: &str = r#"{"_type":"select","projections":[{"expr":{"_type":"planParam","type":{"_type":"unknown"},"name":"n","kind":"OTHER","optional":false,"enumMapFn":null,"byValue":{"kinds":["FLOATING","INTEGER"],"absent":"INTEGER"}},"alias":"x","out":null}],"distinct":false,"from":{"_type":"dual"},"where":null,"groupBy":[],"having":null,"qualify":null,"orderBy":[],"limit":null,"offset":null,"outputs":[]}"#;

#[test]
fn queries_render_as_h2_writes_them() {
    // every alias quoted; raw text written for the engine's H2 is H2's own
    assert_eq!(
        ok(H2, "query.render", &wrap("query", RAW_SELECT)),
        r#""SELECT *\nFROM (select 3 as V) AS \"_p\"\nLIMIT 0""#
    );
    assert_eq!(
        ok(H2, "query.renderInline", &wrap("query", RAW_SELECT)),
        r#""SELECT * FROM (select 3 as V) AS \"_p\" LIMIT 0""#
    );
    // a typed placeholder is cast to its type, a date-time to the nanosecond
    assert_eq!(
        ok(H2, "query.renderStatement", &wrap("query", PARAM_SELECT)),
        r##"{"sql":"SELECT CAST(? AS TIMESTAMP(9)) AS \"value\"","binds":[{"parameter":"#at","binding":{"_type":"one","nullType":"TIMESTAMP","hole":null}}],"regions":[]}"##
    );
    // a value-typed one to a type hole of every kind, in the kinds' declared order
    assert_eq!(
        ok(H2_MODERN, "query.renderStatement", &wrap("query", VALUE_TYPED_SELECT)),
        r#"{"sql":"SELECT CAST(? AS ) AS \"x\"","binds":[{"parameter":"n","binding":{"_type":"one","nullType":"BIGINT","hole":{"at":17,"types":{"INTEGER":{"name":"BIGINT","digits":"NONE","fraction":null},"FLOATING":{"name":"DECFLOAT","digits":"PRECISION","fraction":null}},"absent":"INTEGER"}}}],"regions":[]}"#
    );
    let r = refused(H2, "query.render", &wrap("query", PARAM_SELECT));
    assert_eq!(r.class, Refusal::ILLEGAL_STATE);
    assert!(r.message.starts_with("a statement with bound parameters [Bind[parameter=#at"), "{}", r.message);
}

const CREATE_TABLE: &str = r#"{"_type":"createTable","schema":"S","table":"T","quoting":{"schema":false,"table":false},"columns":[{"name":"ID","declaredQuoted":false,"type":{"_type":"plain","kind":"TIME"},"notNull":true,"primaryKey":true},{"name":"value","declaredQuoted":true,"type":{"_type":"scaled","kind":"DECIMAL","precision":10,"scale":2},"notNull":false,"primaryKey":false}],"temporary":false}"#;

const INSERT_VALUES: &str = r#"{"_type":"insertValues","schema":null,"table":"T","quoting":{"schema":false,"table":false},"columns":["ID"],"quotedColumns":[],"rows":[[{"_type":"stringLit","type":{"_type":"typed","sqlType":{"_type":"scalar","name":"VARCHAR"},"nullable":false},"value":"2"}]]}"#;

#[test]
fn ddl_and_dml_render_and_a_session_table_is_its_table_then_its_rows() {
    let create = r#""Create Table S.T(ID TIME(9) NOT NULL, \"value\" DECIMAL(10, 2) NULL, PRIMARY KEY(ID));""#;
    assert_eq!(ok(H2, "ddl.render", &wrap("ddl", CREATE_TABLE)), create);
    assert_eq!(ok(H2_MODERN, "dml.render", &wrap("dml", INSERT_VALUES)), r#""INSERT INTO T (ID) VALUES ('2');""#);
    assert_eq!(
        ok(H2, "sessionTable", &format!(r#"{{"table":{CREATE_TABLE},"rows":{INSERT_VALUES}}}"#)),
        format!(r#"[{create},"INSERT INTO T (ID) VALUES ('2');"]"#)
    );
    let unspelled = CREATE_TABLE.replace(r#""kind":"TIME""#, r#""kind":"ARRAY""#);
    let r = refused(H2, "ddl.render", &wrap("ddl", &unspelled));
    assert_eq!(
        (r.class, r.message.as_str()),
        (Refusal::ILLEGAL_STATE, "no DDL spelling for declared column type ARRAY")
    );
}

#[test]
fn names_are_spelled_by_h2s_rules() {
    for spec in [H2, H2_MODERN] {
        let physical =
            |name: &str| ok(spec, "physicalName", &format!(r#"{{"name":{}}}"#, to_compact(&Node::Str(name.into()))));
        // bare unless it is no plain word, or a reserved one (the engine's list and `right`); `$` is plain
        assert_eq!(physical("Orders"), r#""Orders""#);
        assert_eq!(physical("a$b"), r#""a$b""#);
        assert_eq!(physical("order"), r#""\"order\"""#);
        assert_eq!(physical("RIGHT"), r#""\"RIGHT\"""#);
        assert_eq!(physical("_rowid_"), r#""\"_rowid_\"""#);
        assert_eq!(physical("a b"), r#""\"a b\"""#);
        assert_eq!(physical("1a"), r#""\"1a\"""#);
        // a pre-quoted name is its own spelling
        assert_eq!(physical("\"date\""), r#""\"date\"""#);
        assert_eq!(ok(spec, "storedName", r#"{"name":"Orders","declaredQuoted":true}"#), r#""\"Orders\"""#);
        assert_eq!(ok(spec, "storedName", r#"{"name":"select","declaredQuoted":false}"#), r#""\"select\"""#);
        // the DDL's identifier rule: bare unless needed (no `$`), a declared-quoted one delimited
        assert_eq!(ok(spec, "insertColumnName", r#"{"name":"id","declaredQuoted":false}"#), r#""id""#);
        assert_eq!(ok(spec, "insertColumnName", r#"{"name":"a$b","declaredQuoted":false}"#), r#""\"a$b\"""#);
        assert_eq!(ok(spec, "insertColumnName", r#"{"name":"Amt","declaredQuoted":true}"#), r#""\"Amt\"""#);
    }
}

#[test]
fn the_engines_h2_text_is_h2s_own() {
    for spec in [H2, H2_MODERN] {
        let text = r#""insert into T (id, name) values (1, CURRENT_TIMESTAMP())""#;
        assert_eq!(ok(spec, "fromEngineText", &format!(r#"{{"statement":{text}}}"#)), text);
        assert_eq!(ok(spec, "raw", r#"{"statement":"create schema S","writtenFor":"H2"}"#), r#""create schema S""#);
        assert_eq!(ok(spec, "raw", r#"{"statement":"create schema S","writtenFor":null}"#), r#""create schema S""#);
    }
}

#[test]
fn wire_names_and_scripts_are_the_interfaces_defaults() {
    assert_eq!(ok(H2, "wireTypeName", r#"{"driverTypeName":"CHARACTER VARYING"}"#), r#""VARCHAR""#);
    assert_eq!(ok(H2, "typingWireName", r#"{"driverTypeName":"INTEGER"}"#), r#""INTEGER""#);
    assert_eq!(ok(H2, "deliversOwn", r#"{"label":{"_type":"scalar","name":"BIGINT"},"wire":"BIGINT"}"#), "false");
    assert_eq!(
        ok(H2, "script", r#"{"statements":["CREATE TABLE a(x INT)","INSERT INTO a VALUES (1)"]}"#),
        r#""CREATE TABLE a(x INT);\nINSERT INTO a VALUES (1);""#
    );
    assert_eq!(ok(H2_MODERN, "script", r#"{"statements":[]}"#), r#"";""#);
}

#[test]
fn h2_names_the_failing_statement_of_a_script() {
    let failing = |spec: &str, message: &str, statements: &[&str]| {
        let mut args = Obj::new();
        args.insert("message", Node::Str(message.into()));
        args.insert("statements", Node::Arr(statements.iter().map(|s| Node::Str((*s).into())).collect()));
        ok(spec, "failingStatement", &to_compact(&Node::Obj(args)))
    };
    let script = [" CREATE TABLE a(x INT) ", "INSERT INTO a VALUES (1)", "SELECT 2"];
    // the quote from the failing statement on, each statement stripped
    assert_eq!(
        failing(H2, "Unique index violated; SQL statement: INSERT INTO a VALUES (1);\nSELECT 2 [23505-214]", &script),
        "1"
    );
    assert_eq!(failing(H2_MODERN, "boom; SQL statement:   CREATE TABLE a(x INT); [42000-214]", &script), "0");
    // no marker (a newline is no space), or a quote no statement starts
    assert_eq!(failing(H2, "boom; SQL statement:\nSELECT 2", &script), "null");
    assert_eq!(failing(H2, "boom; SQL statement: DROP TABLE a", &script), "null");
    // an empty statement starts every quote
    assert_eq!(failing(H2, "SQL statement: anything", &["SELECT 1", "  "]), "1");
}

fn column(data_type: &str) -> String {
    format!(
        r#"{{"column":{{"name":"c","dataType":"{data_type}","logicalType":null,"precision":null,"scale":null,"notNull":true}}}}"#
    )
}

#[test]
fn the_catalog_and_the_host_are_the_interfaces_refusals_naming_the_dialect() {
    for (spec, name) in [(H2, "H2"), (H2_MODERN, "H2Modern")] {
        let r = refused(spec, "catalogType", &column("INTEGER"));
        assert_eq!(
            (r.class, r.message),
            (
                Refusal::DIALECT_CAPABILITY,
                format!("reading this database's catalog type 'INTEGER' is not implemented for {name}")
            )
        );
        let r = refused(spec, "catalogColumnsQuery", r#"{"schema":"s","table":"t"}"#);
        assert_eq!(
            (r.class, r.message),
            (Refusal::UNSUPPORTED_OPERATION, format!("the platform asks no catalog question of {name}"))
        );
        let r = refused(spec, "bareNameReaches", r#"{"catalogName":"T"}"#);
        assert_eq!(r.message, format!("reading this database's catalog names is not implemented for {name}"));
        assert_eq!(ok(spec, "copySelectList", r#"{"conversions":[]}"#), r#""*""#);
        let r = refused(spec, "copySelectList", r#"{"conversions":[{"column":"a","sql":"b"}]}"#);
        assert_eq!(r.message, format!("a copy that converts columns in its star select is not implemented for {name}"));
        for op in ["host.openDatabase", "host.useDatabase", "host.closeDatabase"] {
            let r = refused(spec, op, r#"{"name":"p1"}"#);
            assert_eq!(
                (r.class, r.message),
                (
                    Refusal::DIALECT_CAPABILITY,
                    format!(
                        "a project's own database on {name} is not built: only DuckDB makes one so far \
                         (docs/ONE_PLATFORM_PROGRAM_2026_10_12.md C5)"
                    )
                )
            );
        }
        let r = refused(spec, "host.statements", r#"{"operation":{"op":"platform"}}"#);
        assert_eq!(
            r.message,
            format!("a host's Platform on {name} is not built (docs/ONE_PLATFORM_PROGRAM_2026_10_12.md D5)")
        );
        // DuckDB's own ops are DuckDB's
        let r = refused(spec, "duckdb.tables", "{}");
        assert_eq!(
            (r.class, r.message),
            (Refusal::ILLEGAL_ARGUMENT, format!("'duckdb.tables' is DuckDb's op, not {name}'s"))
        );
        let r = refused(spec, "duckdb.readsStored", r#"{"type":{"_type":"plain","kind":"OTHER"}}"#);
        assert_eq!(r.message, format!("'duckdb.readsStored' is DuckDb's op, not {name}'s"));
    }
}

#[test]
fn the_seam_opens_the_h2_family_and_names_every_dialect_it_answers() {
    let r = dispatch(r#"{"dialect":"Oracle"}"#, "facts", "{}").unwrap_err();
    assert_eq!(
        r.message,
        "unknown dialect 'Oracle': Rust answers DuckDb, H2, H2Modern, EngineStyleH2, EngineStyleDB2, EngineStyleComposite"
    );
    // the engine's scan order is DuckDB's switch: read as a boolean, ignored by H2
    assert_eq!(dispatch(r#"{"dialect":"H2","engineScanOrder":true}"#, "facts", "{}").unwrap(), ok(H2, "facts", "{}"));
    for op in OPS {
        assert_eq!(dispatch(H2, op, "[]").unwrap_err().kind(), "ILLEGAL_ARGUMENT", "{op}");
    }
}
