// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! Every op of the seam through [`dispatch`], each answer held to what Java's `SqlDialect` answers for DuckDB
//! (`DuckDb.java`'s overrides, `AnsiSqlRenderer`'s implementations, the interface's defaults), and the records' JSON
//! forms `SEAM.md` gives. The golden dump holds the renders (`tests/dialect_differential.rs`).

use super::*;
use crate::catalog_model::CatalogColumn;
use crate::dialect::SqlDialect;

const DUCKDB: &str = r#"{"dialect":"DuckDb","server":null,"engineScanOrder":false}"#;

fn ok(op: &str, args: &str) -> String {
    dispatch(DUCKDB, op, args).unwrap_or_else(|r| panic!("{op} {args}: {r}"))
}

fn refused(op: &str, args: &str) -> Refusal {
    match dispatch(DUCKDB, op, args) {
        Ok(answer) => panic!("{op} {args} answered {answer}"),
        Err(r) => r,
    }
}

/// A select over raw SQL, as the dump holds one (`query.render`).
const RAW_SELECT: &str = r#"{"_type":"select","projections":[],"distinct":false,"from":{"_type":"rawSql","sql":"select 3 as V","alias":"_p","outputs":[],"writtenFor":"DuckDB"},"where":null,"groupBy":[],"having":null,"qualify":null,"orderBy":[],"limit":0,"offset":null,"outputs":[]}"#;

/// A select of one plan parameter (`query.renderStatement`).
const PARAM_SELECT: &str = r##"{"_type":"select","projections":[{"expr":{"_type":"planParam","type":{"_type":"typed","sqlType":{"_type":"scalar","name":"BIGINT"},"nullable":false},"name":"#count","kind":"OTHER","optional":false,"enumMapFn":null,"byValue":null},"alias":"value","out":{"name":"value","type":{"_type":"scalar","name":"BIGINT"},"nullable":false,"origin":"DERIVED"}}],"distinct":false,"from":{"_type":"dual"},"where":null,"groupBy":[],"having":null,"qualify":null,"orderBy":[],"limit":null,"offset":null,"outputs":[{"name":"value","type":{"_type":"scalar","name":"BIGINT"},"nullable":false,"origin":"DERIVED"}]}"##;

const CREATE_TABLE: &str = r#"{"_type":"createTable","schema":null,"table":"T","quoting":{"schema":false,"table":false},"columns":[{"name":"ID","declaredQuoted":false,"type":{"_type":"plain","kind":"INTEGER"},"notNull":true,"primaryKey":true}],"temporary":false}"#;

const INSERT_VALUES: &str = r#"{"_type":"insertValues","schema":null,"table":"T","quoting":{"schema":false,"table":false},"columns":["ID"],"quotedColumns":[],"rows":[[{"_type":"stringLit","type":{"_type":"typed","sqlType":{"_type":"scalar","name":"VARCHAR"},"nullable":false},"value":"2"}]]}"#;

const INSERT_FROM_TABLE: &str = r#"{"_type":"insertFromTable","schema":"s","table":"t","quoting":{"schema":false,"table":false},"columns":[],"quotedColumns":[],"source":"legend_row_load"}"#;

fn wrap(key: &str, mir: &str) -> String {
    format!("{{\"{key}\":{mir}}}")
}

#[test]
fn facts_are_duckdbs() {
    assert_eq!(
        ok("facts", "{}"),
        r#"{"temporaryTables":true,"loadsRowsInBulk":true,"relational":true,"inProcess":true,"#.to_owned()
            + r#""dropsStagedTablesWithTheirTransaction":false,"needsStaticPivot":false,"serverVersions":null,"#
            + r#""inMemoryPath":":memory:","scriptAbort":"ROLLBACK","sessionSetup":["SET TimeZone='UTC'"],"#
            + r#""keysSnapshot":"TRANSACTION","resultColumnLimit":null}"#
    );
}

/// A dialect that overrides nothing of the interface: Java's defaults, each refusal naming it.
struct Plain;

impl SqlDialect for Plain {
    fn dialect_name(&self) -> &'static str {
        "Plain"
    }
    fn render(&self, _: SqlQuery) -> Result<String, Refusal> {
        Ok("q".into())
    }
    fn render_inline(&self, _: SqlQuery) -> Result<String, Refusal> {
        Ok("q".into())
    }
    fn render_statement(&self, _: SqlQuery) -> Result<RenderedStatement, Refusal> {
        Ok(RenderedStatement::plain("q".into()))
    }
    fn render_ddl(&self, _: &SqlDdl) -> Result<String, Refusal> {
        Ok("ddl".into())
    }
    fn render_dml(&self, _: &SqlDml) -> Result<String, Refusal> {
        Ok("dml".into())
    }
    fn render_dml_statement(&self, _: &SqlDml) -> Result<RenderedStatement, Refusal> {
        Ok(RenderedStatement::plain("dml".into()))
    }
    fn raw(&self, s: &str, _: Option<&str>) -> Result<String, Refusal> {
        Ok(s.into())
    }
    fn from_engine_text(&self, s: &str) -> Result<String, Refusal> {
        Ok(s.into())
    }
    fn stored_name(&self, n: &str, _: bool) -> String {
        n.into()
    }
    fn physical_name(&self, n: &str) -> String {
        n.into()
    }
    fn insert_column_name(&self, n: &str, _: bool) -> String {
        n.into()
    }
    fn reads_stored(&self, _: &lite_sql::ColumnType) -> bool {
        false
    }
    fn table_name_spelled(&self, n: &str, _: lite_sql::NameQuoting) -> String {
        n.into()
    }
}

#[test]
fn the_interfaces_defaults_answer_as_javas() {
    let d = Plain;
    assert_eq!(
        to_compact(&facts(&d)),
        r#"{"temporaryTables":true,"loadsRowsInBulk":false,"relational":true,"inProcess":false,"#.to_owned()
            + r#""dropsStagedTablesWithTheirTransaction":false,"needsStaticPivot":false,"serverVersions":null,"#
            + r#""inMemoryPath":null,"scriptAbort":null,"sessionSetup":[],"#
            + r#""keysSnapshot":{"refused":{"kind":"DIALECT_CAPABILITY","message":"a dynamic pivot's probe plan reached "#
            + r#"Plain, which says no snapshot for it"}},"resultColumnLimit":{"refused":{"kind":"DIALECT_CAPABILITY","#
            + r#""message":"a dynamic pivot's probe plan reached Plain, which says no column limit for it"}}}"#
    );
    assert_eq!(d.script(&["a".into(), "b".into()]), "a;\nb;");
    assert_eq!(d.script(&[]), ";");
    assert_eq!(d.typing_wire_name(Some("int4")).as_deref(), Some("INTEGER"));
    assert!(d.for_server("1.0").is_none());
    let column = CatalogColumn {
        name: "c".into(),
        data_type: "POINT".into(),
        logical_type: None,
        precision: None,
        scale: None,
        not_null: false,
    };
    assert_eq!(
        d.catalog_type(&column).unwrap_err().message,
        "reading this database's catalog type 'POINT' is not implemented for Plain"
    );
    let r = d.catalog_columns_query("s", "t").unwrap_err();
    assert_eq!((r.class, r.kind()), (Refusal::UNSUPPORTED_OPERATION, "UNSUPPORTED_OPERATION"));
    assert_eq!(r.message, "the platform asks no catalog question of Plain");
    assert_eq!(
        d.bare_name_reaches("x").unwrap_err().message,
        "reading this database's catalog names is not implemented for Plain"
    );
    assert_eq!(d.copy_select_list(&[]).unwrap(), "*");
    let one = [Conversion { column: "a".into(), sql: "b".into() }];
    assert_eq!(
        d.copy_select_list(&one).unwrap_err().message,
        "a copy that converts columns in its star select is not implemented for Plain"
    );
    assert_eq!(
        d.host_operations().open_database("p").unwrap_err().message,
        "a project's own database on Plain is not built: only DuckDB makes one so far \
         (docs/ONE_PLATFORM_PROGRAM_2026_10_12.md C5)"
    );
    let table = read_ddl(parse(CREATE_TABLE).unwrap().as_obj().unwrap()).unwrap();
    let rows = read_dml(parse(INSERT_VALUES).unwrap().as_obj().unwrap()).unwrap();
    assert_eq!(d.session_table(&table, Some(&rows)).unwrap(), ["ddl", "dml"]);
    assert_eq!(d.session_table(&table, None).unwrap(), ["ddl"]);
}

#[test]
fn queries_render_as_text_inline_and_statements() {
    assert_eq!(ok("query.render", &wrap("query", RAW_SELECT)), r#""SELECT *\nFROM (select 3 as V) AS _p\nLIMIT 0""#);
    assert_eq!(
        ok("query.renderInline", &wrap("query", RAW_SELECT)),
        r#""SELECT * FROM (select 3 as V) AS _p LIMIT 0""#
    );
    assert_eq!(
        ok("query.renderStatement", &wrap("query", RAW_SELECT)),
        r#"{"sql":"SELECT *\nFROM (select 3 as V) AS _p\nLIMIT 0","binds":[],"regions":[]}"#
    );
    assert_eq!(
        ok("query.renderStatement", &wrap("query", PARAM_SELECT)),
        r##"{"sql":"SELECT ? AS value","binds":[{"parameter":"#count","binding":{"_type":"one","nullType":"BIGINT","hole":null}}],"regions":[]}"##
    );
    // a parameter reaching text is refused, as Java's render refuses it
    let r = refused("query.render", &wrap("query", PARAM_SELECT));
    assert_eq!(r.class, Refusal::ILLEGAL_STATE, "{r}");
    assert_eq!(
        r.message,
        "a statement with bound parameters [Bind[parameter=#count, binding=One[nullType=BIGINT, hole=null]]] was asked \
         for as text: render it as a statement"
    );
}

#[test]
fn the_spec_carries_the_engines_scan_order() {
    let join = r#"{"_type":"select","projections":[],"distinct":false,"from":{"_type":"join","left":{"_type":"table","name":"A","alias":"t0","outputs":[],"call":false,"storedTypes":{},"quoting":{"schema":false,"table":false}},"right":{"_type":"table","name":"B","alias":"t1","outputs":[],"call":false,"storedTypes":{},"quoting":{"schema":false,"table":false}},"kind":"INNER","on":{"_type":"call","type":{"_type":"typed","sqlType":{"_type":"scalar","name":"BOOLEAN"},"nullable":true},"fn":"EQUAL","args":[{"_type":"column","type":{"_type":"unknown"},"table":"t0","name":"K","origin":"PHYSICAL"},{"_type":"column","type":{"_type":"unknown"},"table":"t1","name":"K","origin":"PHYSICAL"}]},"outputs":[]},"where":null,"groupBy":[],"having":null,"qualify":null,"orderBy":[],"limit":3,"offset":null,"outputs":[]}"#;
    let plain = ok("query.render", &wrap("query", join));
    assert!(!plain.contains("rowid"), "{plain}");
    let spec = r#"{"dialect":"DuckDb","server":null,"engineScanOrder":true}"#;
    let fired = || ok("scanOrderFirings", "{}").parse::<u64>().unwrap();
    let before = fired();
    let ordered = dispatch(spec, "query.render", &wrap("query", join)).unwrap();
    assert!(ordered.contains("ORDER BY t0.rowid NULLS LAST, t1.rowid NULLS LAST"), "{ordered}");
    // the statement changed, so the process's count moved (other tests render beside this one: at least one)
    assert!(fired() > before, "StableScanOrder's firings did not move");
    // missing: off; a server: DuckDB's text depends on none
    let bare = dispatch(r#"{"dialect":"DuckDb"}"#, "query.render", &wrap("query", join)).unwrap();
    assert_eq!(bare, plain);
    let served = dispatch(r#"{"dialect":"DuckDb","server":"1.4.1"}"#, "query.render", &wrap("query", join)).unwrap();
    assert_eq!(served, plain);
}

#[test]
fn ddl_and_dml_render() {
    assert_eq!(
        ok("ddl.render", &wrap("ddl", CREATE_TABLE)),
        r#""Create Table T(ID INTEGER NOT NULL, PRIMARY KEY(ID));""#
    );
    assert_eq!(ok("dml.render", &wrap("dml", INSERT_VALUES)), r#""INSERT INTO T (ID) VALUES ('2');""#);
    assert_eq!(
        ok("dml.renderStatement", &wrap("dml", INSERT_FROM_TABLE)),
        r#"{"sql":"INSERT INTO s.t SELECT * FROM legend_row_load;","binds":[],"regions":[]}"#
    );
    // a DDL statement read as DML is refused by MirJson's reader
    let r = refused("dml.render", &wrap("dml", CREATE_TABLE));
    assert_eq!((r.class, r.message.as_str()), (Refusal::ILLEGAL_ARGUMENT, "unknown DML statement tag 'createTable'"));
}

#[test]
fn a_session_table_is_its_table_then_its_rows() {
    assert_eq!(
        ok("sessionTable", &format!(r#"{{"table":{CREATE_TABLE},"rows":{INSERT_VALUES}}}"#)),
        r#"["Create Table T(ID INTEGER NOT NULL, PRIMARY KEY(ID));","INSERT INTO T (ID) VALUES ('2');"]"#
    );
    assert_eq!(
        ok("sessionTable", &format!(r#"{{"table":{CREATE_TABLE},"rows":null}}"#)),
        r#"["Create Table T(ID INTEGER NOT NULL, PRIMARY KEY(ID));"]"#
    );
    let drop = r#"{"_type":"dropTable","schema":null,"table":"T","quoting":{"schema":false,"table":false}}"#;
    assert_eq!(
        refused("sessionTable", &format!(r#"{{"table":{drop},"rows":null}}"#)).message,
        "sessionTable's table is a createTable, not dropTable"
    );
    assert_eq!(
        refused("sessionTable", &format!(r#"{{"table":{CREATE_TABLE},"rows":{INSERT_FROM_TABLE}}}"#)).message,
        "sessionTable's rows are an insertValues, not insertFromTable"
    );
}

#[test]
fn names_are_spelled_by_duckdbs_rules() {
    assert_eq!(ok("physicalName", r#"{"name":"Orders"}"#), r#""Orders""#);
    assert_eq!(ok("physicalName", r#"{"name":"order"}"#), r#""\"order\"""#);
    assert_eq!(ok("physicalName", r#"{"name":"\"date\""}"#), r#""\"date\"""#);
    assert_eq!(ok("physicalName", r#"{"name":"a b"}"#), r#""\"a b\"""#);
    assert_eq!(ok("storedName", r#"{"name":"Orders","declaredQuoted":true}"#), r#""\"Orders\"""#);
    assert_eq!(ok("storedName", r#"{"name":"select","declaredQuoted":false}"#), r#""\"select\"""#);
    // the DDL's column rule: a bare name folded upper, a declared-quoted one as declared
    assert_eq!(ok("insertColumnName", r#"{"name":"id","declaredQuoted":false}"#), r#""ID""#);
    assert_eq!(ok("insertColumnName", r#"{"name":"Amt","declaredQuoted":true}"#), r#""\"Amt\"""#);
    let r = refused("storedName", r#"{"name":"x"}"#);
    assert_eq!((r.class, r.message.as_str()), (Refusal::ILLEGAL_ARGUMENT, "missing field 'declaredQuoted'"));
}

#[test]
fn raw_text_is_translated_only_from_the_engines_h2() {
    assert_eq!(ok("raw", r#"{"statement":"create schema S","writtenFor":"H2"}"#), r#""Create schema if not exists S""#);
    assert_eq!(ok("raw", r#"{"statement":"create schema S","writtenFor":"DuckDB"}"#), r#""create schema S""#);
    assert_eq!(ok("raw", r#"{"statement":"create schema S","writtenFor":null}"#), r#""create schema S""#);
    assert_eq!(ok("raw", r#"{"statement":"create schema S"}"#), r#""create schema S""#);
    assert_eq!(
        ok("fromEngineText", r#"{"statement":"insert into T (id, name) values (1, CURRENT_TIMESTAMP())"}"#),
        r#""insert into T (\"id\", \"name\") values (1, CURRENT_TIMESTAMP)""#
    );
}

#[test]
fn wire_names_fold_and_nothing_is_delivered_by_a_convention() {
    assert_eq!(ok("wireTypeName", r#"{"driverTypeName":null}"#), r#""""#);
    assert_eq!(ok("wireTypeName", r#"{"driverTypeName":"int8"}"#), r#""BIGINT""#);
    assert_eq!(ok("wireTypeName", r#"{"driverTypeName":"STRUCT(a INTEGER)"}"#), r#""STRUCT""#);
    assert_eq!(ok("typingWireName", r#"{"driverTypeName":"numeric(10, 2)"}"#), r#""DECIMAL(10,2)""#);
    assert_eq!(ok("typingWireName", r#"{}"#), r#""""#);
    assert_eq!(ok("deliversOwn", r#"{"label":{"_type":"scalar","name":"BIGINT"},"wire":"BIGINT"}"#), "false");
    assert_eq!(ok("deliversOwn", r#"{"label":{"_type":"decimal","precision":10,"scale":2},"wire":"DOUBLE"}"#), "false");
    assert_eq!(
        refused("deliversOwn", r#"{"label":{"_type":"scalar","name":"NOPE"},"wire":"X"}"#).message,
        "unknown scalar type 'NOPE'"
    );
}

#[test]
fn a_script_is_one_transaction_and_names_no_failing_statement() {
    assert_eq!(
        ok("script", r#"{"statements":["CREATE TABLE a(x INT)","INSERT INTO a VALUES (1)"]}"#),
        r#""BEGIN TRANSACTION;\nCREATE TABLE a(x INT);\nINSERT INTO a VALUES (1);\nCOMMIT;""#
    );
    assert_eq!(ok("script", r#"{"statements":[]}"#), r#""BEGIN TRANSACTION;\n;\nCOMMIT;""#);
    assert_eq!(ok("failingStatement", r#"{"message":"Constraint Error","statements":["a","b"]}"#), "null");
    assert_eq!(refused("script", r#"{"statements":[1]}"#).message, "field 'statements' is an array of strings");
}

fn column(data_type: &str, logical: Option<&str>, precision: Option<i32>, scale: Option<i32>) -> String {
    let text = |s: Option<&str>| s.map_or("null".to_owned(), |s| format!("\"{s}\""));
    let int = |n: Option<i32>| n.map_or("null".to_owned(), |n| n.to_string());
    format!(
        r#"{{"column":{{"name":"c","dataType":"{data_type}","logicalType":{},"precision":{},"scale":{},"notNull":true}}}}"#,
        text(logical),
        int(precision),
        int(scale)
    )
}

#[test]
fn catalog_types_are_duckdbs_decisions() {
    let cases = [
        (
            "VARCHAR",
            Some("VARCHAR"),
            r#"{"read":"AS_STORED","declared":"VARCHAR(4096)","conversion":null,"reason":null}"#,
        ),
        ("BOOLEAN", Some("BOOLEAN"), r#"{"read":"AS_STORED","declared":"BIT","conversion":null,"reason":null}"#),
        ("UTINYINT", Some("UTINYINT"), r#"{"read":"AS_STORED","declared":"SMALLINT","conversion":null,"reason":null}"#),
        (
            "UBIGINT",
            Some("UBIGINT"),
            r#"{"read":"CONVERTED","declared":"DECIMAL(20,0)","conversion":"CAST(%s AS DECIMAL(20,0))","reason":null}"#,
        ),
        (
            "HUGEINT",
            Some("hugeint"),
            r#"{"read":"CONVERTED","declared":"DECIMAL(38,0)","conversion":"CAST(%s AS DECIMAL(38,0))","reason":null}"#,
        ),
        ("FLOAT", Some("FLOAT"), r#"{"read":"AS_STORED","declared":"REAL","conversion":null,"reason":null}"#),
        (
            "TIMESTAMP_NS",
            Some("TIMESTAMP_NS"),
            r#"{"read":"AS_STORED","declared":"TIMESTAMP","conversion":null,"reason":null}"#,
        ),
        (
            "TIMESTAMP WITH TIME ZONE",
            Some("TIMESTAMP WITH TIME ZONE"),
            r#"{"read":"COPY_CONVERTED","declared":"TIMESTAMP","conversion":"CAST(timezone('UTC', %s) AS TIMESTAMP)","reason":null}"#,
        ),
        ("UUID", Some("UUID"), r#"{"read":"AS_STORED","declared":"OTHER","conversion":null,"reason":null}"#),
        (
            "INTEGER[]",
            Some("LIST"),
            r#"{"read":"AS_STORED","declared":"SEMISTRUCTURED","conversion":null,"reason":null}"#,
        ),
        // JSON is an alias of VARCHAR, known by its own name
        (
            "JSON",
            Some("VARCHAR"),
            r#"{"read":"AS_STORED","declared":"SEMISTRUCTURED","conversion":null,"reason":null}"#,
        ),
        (
            "BLOB",
            Some("BLOB"),
            r#"{"read":"LEFT_OUT","declared":null,"conversion":null,"reason":"bytes: no Pure Database type holds them"}"#,
        ),
    ];
    for (data_type, logical, want) in cases {
        assert_eq!(ok("catalogType", &column(data_type, logical, None, None)), want, "{data_type}");
    }
    assert_eq!(
        ok("catalogType", &column("DECIMAL(18,3)", Some("DECIMAL"), Some(18), Some(3))),
        r#"{"read":"AS_STORED","declared":"DECIMAL(18,3)","conversion":null,"reason":null}"#
    );
    let refusals = [
        (
            column("DECIMAL", Some("DECIMAL"), Some(18), None),
            "a DECIMAL column whose catalog gives no precision and scale ('DECIMAL') cannot be declared",
        ),
        (
            column("GEOMETRY", Some("GEOMETRY"), None, None),
            "a column of DuckDB type 'GEOMETRY' cannot be declared in a Pure Database (a spatial value: no Pure Database type holds it)",
        ),
        (
            column("UHUGEINT", Some("UHUGEINT"), None, None),
            "a column of DuckDB type 'UHUGEINT' cannot be declared in a Pure Database (not yet decided)",
        ),
        (
            column("WHAT", Some("WHAT"), None, None),
            "a column of DuckDB type 'WHAT' cannot be declared in a Pure Database (a type this dialect does not read)",
        ),
        (
            column("mood", None, None, None),
            "a column of DuckDB type 'mood' cannot be declared in a Pure Database (its catalog names no canonical type)",
        ),
    ];
    for (args, want) in refusals {
        let r = refused("catalogType", &args);
        assert_eq!((r.class, r.message.as_str()), (Refusal::DIALECT_CAPABILITY, want));
    }
    // every canonical type is declared, refused or the decimal: none is both
    for (name, _) in crate::duckdb::catalog_types() {
        assert!(!crate::duckdb::CATALOG_REFUSED.iter().any(|(r, _)| *r == name), "{name}");
    }
    let r = refused("catalogType", r#"{"column":{"name":"c","dataType":"X","precision":3000000000,"notNull":false}}"#);
    assert_eq!(r.message, "field 'precision' is not an int: 3000000000");
}

#[test]
fn the_catalog_question_names_its_table_by_literals() {
    let want = "SELECT c.column_name, c.data_type, t.logical_type, c.numeric_precision, c.numeric_scale, \
                NOT c.is_nullable AS not_null\n\
                FROM duckdb_columns() c\n\
                LEFT JOIN (SELECT DISTINCT type_oid, logical_type FROM duckdb_types()\n           \
                WHERE internal AND type_oid IS NOT NULL) t ON t.type_oid = c.data_type_id\n\
                WHERE c.database_name = current_database() AND c.schema_name = 'main' AND c.table_name = 'it''s {table}'\n\
                ORDER BY c.column_index";
    let d = DuckDb::new();
    assert_eq!(SqlDialect::catalog_columns_query(&d, "main", "it's {table}").unwrap(), want);
    assert_eq!(
        ok("catalogColumnsQuery", r#"{"schema":"main","table":"it's {table}"}"#),
        to_compact(&Node::Str(want.into()))
    );
    // a NUL is spliced as the renderer's string literal splices it
    assert!(
        SqlDialect::catalog_columns_query(&d, "a\0b", "t").unwrap().contains("c.schema_name = ('a' || chr(0) || 'b')")
    );
}

#[test]
fn bare_names_reach_and_copies_replace_their_converted_columns() {
    assert_eq!(ok("bareNameReaches", r#"{"catalogName":"Orders"}"#), "true");
    assert_eq!(ok("copySelectList", r#"{"conversions":[]}"#), r#""*""#);
    assert_eq!(
        ok(
            "copySelectList",
            r#"{"conversions":[{"column":"a\"b","sql":"to_json(\"a\")"},{"column":"n","sql":"CAST(\"n\" AS DECIMAL(20,0))"}]}"#
        ),
        r#""* REPLACE (to_json(\"a\") AS \"a\"\"b\", CAST(\"n\" AS DECIMAL(20,0)) AS \"n\")""#
    );
}

#[test]
fn host_operations_through_the_seam() {
    assert_eq!(ok("host.openDatabase", r#"{"name":"p1"}"#), r#"["ATTACH ':memory:' AS \"p1\""]"#);
    assert_eq!(ok("host.useDatabase", r#"{"name":"p1"}"#), r#"["USE \"p1\""]"#);
    assert_eq!(ok("host.closeDatabase", r#"{"name":"p1"}"#), r#"["DETACH DATABASE IF EXISTS \"p1\""]"#);
    assert_eq!(
        ok("host.statements", r#"{"operation":{"op":"extensions","names":["httpfs"]}}"#),
        r#"["INSTALL httpfs","LOAD httpfs"]"#
    );
    assert_eq!(ok("host.statements", r#"{"operation":{"op":"secret","name":"s","scope":"x","credentials":{}}}"#), "[]");
    assert_eq!(refused("host.statements", "{}").message, "missing field 'operation'");
    assert_eq!(refused("host.statements", r#"{"operation":"platform"}"#).message, "an operation is a JSON object");
    assert_eq!(refused("host.openDatabase", r#"{"name":null}"#).message, "field 'name' is null, not a string");
}

#[test]
fn the_seam_refuses_what_it_cannot_read() {
    let r = dispatch(r#"{"dialect":"Oracle"}"#, "facts", "{}").unwrap_err();
    assert_eq!(
        (r.class, r.message.as_str()),
        (
            Refusal::ILLEGAL_ARGUMENT,
            "unknown dialect 'Oracle': Rust answers DuckDb, H2, H2Modern, EngineStyleH2, EngineStyleDB2, \
             EngineStyleComposite, Postgres, SQLite, Trino"
        )
    );
    assert_eq!(dispatch("[]", "facts", "{}").unwrap_err().message, "a dialect's spec is a JSON object");
    assert_eq!(dispatch("{}", "facts", "{}").unwrap_err().message, "missing field 'dialect'");
    assert_eq!(
        dispatch(r#"{"dialect":"DuckDb","engineScanOrder":"yes"}"#, "facts", "{}").unwrap_err().message,
        "field 'engineScanOrder' is a string, not a boolean"
    );
    // the JSON reader's own words (Json.parse's IllegalArgumentException)
    let r = dispatch("{", "facts", "{}").unwrap_err();
    assert_eq!(r.class, Refusal::ILLEGAL_ARGUMENT);
    assert!(r.message.contains("at line 1 col 2"), "{}", r.message);
    assert_eq!(refused("render", "{}").message, "unknown dialect op 'render'");
    // the op is refused before its args are read
    assert_eq!(refused("nope", "not json").message, "unknown dialect op 'nope'");
    assert_eq!(refused("facts", "[]").message, "the args of 'facts' are a JSON object");
    assert!(refused("facts", "{").message.contains("at line 1 col 2"), "{}", refused("facts", "{").message);
    assert_eq!(refused("query.render", "{}").message, "missing field 'query'");
    assert_eq!(refused("query.render", r#"{"query":{"_type":"selekt"}}"#).message, "unknown query tag 'selekt'");
    for op in OPS {
        let r = dispatch(DUCKDB, op, "[]").unwrap_err();
        assert_eq!(r.kind(), "ILLEGAL_ARGUMENT", "{op}");
    }
}

#[test]
fn a_mir_nests_as_deep_as_mirjson_reads_it() {
    // `nots` calls of NOT over a typed literal: the args object is level 1, the select 2, its projection list 3, the
    // projection 4, the outer call 5, each call two more (itself and its args), and the literal's type's SQL type two
    // past the literal -- 7 + 2 * nots levels in all, the MIR one fewer
    let typed = r#"{"_type":"typed","sqlType":{"_type":"scalar","name":"BOOLEAN"},"nullable":true}"#;
    let nested = |nots: usize| {
        let mut e = format!(r#"{{"_type":"boolLit","type":{typed},"value":true}}"#);
        for _ in 0..nots {
            e = format!(r#"{{"_type":"call","type":{typed},"fn":"NOT","args":[{e}]}}"#);
        }
        let select = format!(
            r#"{{"_type":"select","projections":[{{"expr":{e},"alias":"x","out":null}}],"distinct":false,"from":{{"_type":"dual"}},"where":null,"groupBy":[],"having":null,"qualify":null,"orderBy":[],"limit":null,"offset":null,"outputs":[]}}"#
        );
        wrap("query", &select)
    };
    let run = |args: String| {
        std::thread::Builder::new()
            .stack_size(256 << 20)
            .spawn(move || dispatch(DUCKDB, "query.render", &args))
            .unwrap()
            .join()
            .unwrap()
    };
    // the MIR exactly MAX_DEPTH deep: read and rendered
    let nots = (MAX_DEPTH + 1 - 7) / 2;
    assert_eq!(7 + 2 * nots, MAX_DEPTH + 1);
    let deep = run(nested(nots)).unwrap_or_else(|r| panic!("{r}"));
    assert_eq!(deep.matches("NOT").count(), nots, "{}", &deep[..200]);
    // two levels more: refused by the parser
    let too_deep = run(nested(nots + 1)).unwrap_err();
    assert_eq!(too_deep.class, Refusal::ILLEGAL_ARGUMENT);
    assert!(too_deep.message.starts_with("Nesting depth exceeds limit of 4097"), "{}", too_deep.message);
}

#[test]
fn a_statement_writes_every_records_components_and_reads_back() {
    let hole = TypeHole::new(
        8,
        vec![
            (ValueKind::DateTimeNanos, TypeSpelling::new("TIMESTAMP_NS", Digits::None)),
            (ValueKind::Integer, TypeSpelling::new("BIGINT", Digits::None)),
            (
                ValueKind::Decimal,
                TypeSpelling { name: "DECIMAL".into(), digits: Digits::PrecisionAndScale, fraction: Some(3) },
            ),
        ],
        ValueKind::Integer,
    )
    .unwrap();
    let s = RenderedStatement::new(
        "SELECT ?, ? FROM t WHERE k IN (?)".into(),
        vec![
            Bind { parameter: "a".into(), binding: Binding::One { null_type: "BIGINT".into(), hole: Some(hole) } },
            Bind { parameter: "b".into(), binding: Binding::Array { element_sql_type: "VARCHAR".into() } },
            Bind { parameter: "k".into(), binding: Binding::key(1).unwrap() },
        ],
        vec![Region::new(28, 29, " OR ".into(), ", ".into(), None, 2, 1, vec![28]).unwrap()],
    )
    .unwrap();
    let text = to_compact(&statement_json(&s));
    assert_eq!(
        text,
        r#"{"sql":"SELECT ?, ? FROM t WHERE k IN (?)","binds":[{"parameter":"a","binding":{"_type":"one","nullType":"BIGINT","hole":{"at":8,"types":{"INTEGER":{"name":"BIGINT","digits":"NONE","fraction":null},"DECIMAL":{"name":"DECIMAL","digits":"PRECISION_AND_SCALE","fraction":3},"DATE_TIME_NANOS":{"name":"TIMESTAMP_NS","digits":"NONE","fraction":null}},"absent":"INTEGER"}}},{"parameter":"b","binding":{"_type":"array","elementSqlType":"VARCHAR"}},{"parameter":"k","binding":{"_type":"key","cell":1}}],"regions":[{"start":28,"end":29,"lead":" OR ","separator":", ","empty":null,"firstBind":2,"bindCount":1,"counters":[28]}]}"#
    );
    let back = read_statement(parse(&text).unwrap().as_obj().unwrap()).unwrap();
    assert_eq!(back, s);
    // the constructors' checks hold on the way back
    let bad = text.replace(r#""cell":1"#, r#""cell":-1"#);
    assert_eq!(read_statement(parse(&bad).unwrap().as_obj().unwrap()).unwrap_err().message, "a key's cell -1");
}

#[test]
fn refusals_fold_to_the_seams_kinds() {
    let kinds = [
        (Refusal::DIALECT_CAPABILITY, "DIALECT_CAPABILITY"),
        (Refusal::ILLEGAL_STATE, "ILLEGAL_STATE"),
        (Refusal::ILLEGAL_ARGUMENT, "ILLEGAL_ARGUMENT"),
        (Refusal::UNSUPPORTED_OPERATION, "UNSUPPORTED_OPERATION"),
        (Refusal::NOT_IMPLEMENTED, "NOT_IMPLEMENTED"),
        (Refusal::NULL_POINTER, "NULL_POINTER"),
        (Refusal::CLASS_CAST, "CLASS_CAST"),
        (Refusal::INDEX_OUT_OF_BOUNDS, "INDEX_OUT_OF_BOUNDS"),
    ];
    for (class, kind) in kinds {
        let r = Refusal { class, message: "m".into() };
        assert_eq!((r.kind(), r.seam_message().as_str()), (kind, "m"));
    }
    for class in [Refusal::NUMBER_FORMAT, "java.lang.StringIndexOutOfBoundsException"] {
        let r = Refusal { class, message: "m".into() };
        assert_eq!((r.kind(), r.seam_message()), ("INTERNAL", format!("{class}: m")));
    }
    assert_eq!(
        to_compact(&refused_json(&Refusal { class: Refusal::NUMBER_FORMAT, message: "x".into() })),
        r#"{"refused":{"kind":"INTERNAL","message":"java.lang.NumberFormatException: x"}}"#
    );
}

/// Java's `DuckDb.LEXICON` reserved words, as its source listed them (held to DuckDB's `duckdb_keywords()` by
/// `DuckDbKeywordsTest`).
const JAVA_RESERVED_WORDS: &[&str] = &[
    "all", "analyse", "analyze", "and", "anti", "any", "array", "as", "asc", "asof", "asymmetric", "at",
    "authorization", "between", "binary", "both", "by", "case", "cast", "check", "collate", "collation", "column",
    "columns", "concurrently", "constraint", "create", "cross", "default", "deferrable", "delete", "desc", "describe",
    "distinct", "do", "drop", "else", "end", "except", "exists", "false", "fetch", "for", "foreign", "freeze", "from",
    "full", "generated", "glob", "group", "having", "ilike", "in", "initially", "inner", "insert", "intersect", "into",
    "is", "isnull", "join", "lambda", "lateral", "leading", "left", "like", "limit", "map", "natural", "not", "notnull",
    "null", "offset", "on", "only", "or", "order", "outer", "overlaps", "pivot", "pivot_longer", "pivot_wider",
    "placing", "positional", "primary", "qualify", "references", "returning", "right", "select", "semi", "show",
    "similar", "some", "struct", "summarize", "symmetric", "table", "tablesample", "then", "to", "trailing", "true",
    "try_cast", "union", "unique", "unpack", "unpivot", "update", "using", "values", "variadic", "verbose", "when",
    "where", "window", "with",
];

#[test]
fn duckdbs_tables_are_javas() {
    let answer = parse(&ok("duckdb.tables", "{}")).unwrap();
    let tables = answer.as_obj().unwrap();
    let lexicon = tables.obj("lexicon").unwrap();
    assert_eq!(lexicon.str("quoteChar").unwrap(), "\"");
    let words: Vec<&str> = lexicon.arr("reservedWords").unwrap().iter().map(|w| w.as_str().unwrap()).collect();
    let mut java = JAVA_RESERVED_WORDS.to_vec();
    java.sort_unstable();
    assert_eq!(words, java, "Java's LEXICON, sorted");
    // CATALOG_TYPES: every canonical type, in Java's order (each decision is catalog_types_are_duckdbs_decisions')
    let types = tables.obj("catalogTypes").unwrap();
    let names: Vec<&str> = types.iter().map(|(name, _)| name).collect();
    assert_eq!(
        names,
        [
            "VARCHAR", "BOOLEAN", "TINYINT", "SMALLINT", "INTEGER", "BIGINT", "UTINYINT", "USMALLINT", "UINTEGER",
            "UBIGINT", "HUGEINT", "FLOAT", "DOUBLE", "DATE", "TIMESTAMP", "TIMESTAMP_S", "TIMESTAMP_MS", "TIMESTAMP_NS",
            "TIMESTAMP WITH TIME ZONE", "TIME", "TIME WITH TIME ZONE", "UUID", "INTERVAL", "BIT", "BIGNUM", "ENUM",
            "STRUCT", "LIST", "MAP", "UNION", "ARRAY", "BLOB",
        ]
    );
    assert_eq!(
        to_compact(types.get("TIMESTAMP WITH TIME ZONE").unwrap()),
        r#"{"read":"COPY_CONVERTED","declared":"TIMESTAMP","conversion":"CAST(timezone('UTC', %s) AS TIMESTAMP)","reason":null}"#
    );
    assert_eq!(
        to_compact(types.get("BLOB").unwrap()),
        r#"{"read":"LEFT_OUT","declared":null,"conversion":null,"reason":"bytes: no Pure Database type holds them"}"#
    );
    // CATALOG_REFUSED, in Java's order
    assert_eq!(
        to_compact(tables.get("catalogRefused").unwrap()),
        r#"{"GEOMETRY":"a spatial value: no Pure Database type holds it","UHUGEINT":"not yet decided","#.to_owned()
            + r#""TIME_NS":"not yet decided","VARIANT":"not yet decided","NULL":"a column of no type","#
            + r#""TYPE":"a type, not a value"}"#
    );
}

#[test]
fn duckdb_reads_its_other_and_distinct_columns_stored() {
    let reads = |t: &str| ok("duckdb.readsStored", &wrap("type", t));
    assert_eq!(reads(r#"{"_type":"plain","kind":"OTHER"}"#), "true");
    assert_eq!(reads(r#"{"_type":"plain","kind":"DISTINCT"}"#), "true");
    assert_eq!(reads(r#"{"_type":"plain","kind":"INTEGER"}"#), "false");
    assert_eq!(reads(r#"{"_type":"plain","kind":"JSON"}"#), "false");
    assert_eq!(refused("duckdb.readsStored", "{}").message, "missing field 'type'");
}
