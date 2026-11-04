// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! Every op of the seam through [`dispatch`] for MongoDb, each answer held to what Java's `SqlDialect` answers for it
//! (`MongoDb.java`'s overrides, `AnsiSqlRenderer`'s implementations, the interface's defaults naming the dialect). The
//! golden oracles hold the renders (`tests/dialect_differential.rs`).

use super::*;

const MONGO: &str = r#"{"dialect":"MongoDb","server":null}"#;

fn ok(op: &str, args: &str) -> String {
    dispatch(MONGO, op, args).unwrap_or_else(|r| panic!("{op} {args}: {r}"))
}

fn refused(op: &str, args: &str) -> Refusal {
    match dispatch(MONGO, op, args) {
        Ok(answer) => panic!("{op} {args} answered {answer}"),
        Err(r) => r,
    }
}

fn wrap(key: &str, mir: &str) -> String {
    format!("{{\"{key}\":{mir}}}")
}

#[test]
fn facts_are_mongodbs() {
    assert_eq!(
        ok("facts", "{}"),
        r#"{"temporaryTables":false,"loadsRowsInBulk":false,"relational":false,"inProcess":false,"dropsStagedTablesWithTheirTransaction":false,"needsStaticPivot":false,"serverVersions":null,"inMemoryPath":null,"scriptAbort":null,"sessionSetup":[],"keysSnapshot":"NONE","resultColumnLimit":null}"#
    );
    // every server version is spelled alike; a spec's server keeps the dialect
    assert_eq!(ok("forServer", r#"{"version":"8.0.17"}"#), "\"MongoDb\"");
    let served = MONGO.replace("\"server\":null", "\"server\":\"8.0.17\"");
    assert_eq!(dispatch(&served, "facts", "{}").unwrap(), ok("facts", "{}"));
    let configured = MONGO.replace("\"server\":null", "\"timeZone\":\"UTC\"");
    assert_eq!(
        dispatch(&configured, "facts", "{}").unwrap_err().message,
        "quoteIdentifiers and timeZone configure an engine-text dialect (EngineStyleH2, EngineStyleDB2), not MongoDb"
    );
}

/// `SELECT 1 AS "b"` over no table, with an offset and a limit.
const SELECT: &str = r#"{"_type":"select","projections":[{"expr":{"_type":"intLit","type":{"_type":"typed","sqlType":{"_type":"scalar","name":"BIGINT"},"nullable":false},"value":1},"alias":"b","out":null}],"distinct":false,"from":{"_type":"dual"},"where":null,"groupBy":[],"having":null,"qualify":null,"orderBy":[],"limit":5,"offset":3,"outputs":[]}"#;

/// `SELECT ? AS "p"` over no table: a plan parameter `n` of BIGINT.
const PARAM: &str = r#"{"_type":"select","projections":[{"expr":{"_type":"planParam","type":{"_type":"typed","sqlType":{"_type":"scalar","name":"BIGINT"},"nullable":false},"name":"n","kind":"OTHER","optional":false,"enumMapFn":null,"byValue":null},"alias":"p","out":null}],"distinct":false,"from":{"_type":"dual"},"where":null,"groupBy":[],"having":null,"qualify":null,"orderBy":[],"limit":null,"offset":null,"outputs":[]}"#;

#[test]
fn queries_are_aggregate_commands() {
    let text = r#""{\"aggregate\": 1, \"pipeline\": [{\"$documents\": [{}]}, {\"$replaceWith\": {\"b\": {\"$numberLong\": \"1\"}}}, {\"$skip\": 3}, {\"$limit\": 5}], \"cursor\": {}}""#;
    assert_eq!(ok("query.render", &wrap("query", SELECT)), text);
    // a pipeline has no lines to put on one: the inline render is the render
    assert_eq!(ok("query.renderInline", &wrap("query", SELECT)), text);
    // a plan parameter is the marker document the connector replaces, bound by its place among the markers
    assert_eq!(
        ok("query.renderStatement", &wrap("query", PARAM)),
        r#"{"sql":"{\"aggregate\": 1, \"pipeline\": [{\"$documents\": [{}]}, {\"$replaceWith\": {\"p\": {\"$legendParam\": \"?\"}}}], \"cursor\": {}}","binds":[{"parameter":"n","binding":{"_type":"one","nullType":"BIGINT","hole":null}}],"regions":[]}"#
    );
    let r = refused("query.render", &wrap("query", PARAM));
    assert_eq!(
        (r.kind(), r.message.as_str()),
        (
            "ILLEGAL_STATE",
            "a statement with bound parameters [Bind[parameter=n, binding=One[nullType=BIGINT, hole=null]]] was asked \
             for as text: render it as a statement"
        )
    );
}

const CREATE: &str = r#"{"_type":"createTable","schema":null,"table":"Trades","quoting":{"schema":false,"table":false},"columns":[{"name":"ID","declaredQuoted":false,"type":{"_type":"plain","kind":"BIGINT"},"notNull":true,"primaryKey":true}],"temporary":false}"#;

const INSERT: &str = r#"{"_type":"insertValues","schema":null,"table":"Trades","quoting":{"schema":false,"table":false},"columns":["ID"],"quotedColumns":[],"rows":[[{"_type":"intLit","type":{"_type":"typed","sqlType":{"_type":"scalar","name":"BIGINT"},"nullable":false},"value":1}]]}"#;

#[test]
fn ddl_and_dml_are_collection_commands() {
    let create = r#""{\"create\": \"Trades\"}""#;
    let insert = r#""{\"insert\": \"Trades\", \"documents\": [{\"ID\": {\"$numberLong\": \"1\"}}]}""#;
    assert_eq!(ok("ddl.render", &wrap("ddl", CREATE)), create);
    let drop =
        r#"{"ddl":{"_type":"dropTable","schema":"default","table":"Trades","quoting":{"schema":false,"table":false}}}"#;
    assert_eq!(ok("ddl.render", drop), r#""{\"drop\": \"Trades\"}""#);
    assert_eq!(ok("dml.render", &wrap("dml", INSERT)), insert);
    assert_eq!(
        ok("dml.renderStatement", &wrap("dml", INSERT)),
        format!(r#"{{"sql":{insert},"binds":[],"regions":[]}}"#)
    );
    let delete =
        r#"{"dml":{"_type":"deleteAll","schema":null,"table":"Trades","quoting":{"schema":false,"table":false}}}"#;
    assert_eq!(ok("dml.render", delete), r#""{\"delete\": \"Trades\", \"deletes\": [{\"q\": {}, \"limit\": 0}]}""#);
    let schema = CREATE.replace("\"schema\":null", "\"schema\":\"S\"");
    let r = refused("ddl.render", &wrap("ddl", &schema));
    assert_eq!(
        (r.kind(), r.message.as_str()),
        ("DIALECT_CAPABILITY", "a schema-qualified table (S.Trades) reached MongoDb: a collection has no schema")
    );
    let r = refused("ddl.render", r#"{"ddl":{"_type":"createSchema","schema":"S","quoted":false}}"#);
    assert_eq!(
        r.message,
        "a schema (S) reached MongoDb: a collection's database is the connection's, and has no schema"
    );
    // the interface's default session table: the table, then its insert -- a temporary one has no collection
    assert_eq!(ok("sessionTable", &format!(r#"{{"table":{CREATE},"rows":{INSERT}}}"#)), format!("[{create},{insert}]"));
    let temporary = CREATE.replace("\"temporary\":false", "\"temporary\":true");
    let r = refused("sessionTable", &format!(r#"{{"table":{temporary},"rows":null}}"#));
    assert_eq!(
        (r.kind(), r.message.as_str()),
        ("DIALECT_CAPABILITY", "a temporary table reached MongoDb, which has no temporary collection")
    );
}

#[test]
fn names_are_json_strings() {
    // no quoting, no reserved words
    assert_eq!(ok("physicalName", r#"{"name":"select"}"#), r#""select""#);
    assert_eq!(ok("storedName", r#"{"name":"a b","declaredQuoted":true}"#), r#""a b""#);
    assert_eq!(ok("storedName", r#"{"name":"Orders","declaredQuoted":false}"#), r#""Orders""#);
    assert_eq!(ok("insertColumnName", r#"{"name":"x y","declaredQuoted":false}"#), r#""x y""#);
    assert_eq!(
        ok("tableName", r#"{"name":"Sales.Orders","quoting":{"schema":false,"table":false}}"#),
        r#""Sales.Orders""#
    );
    assert_eq!(ok("tableName", r#"{"name":"Order","quoting":{"schema":false,"table":true}}"#), r#""Order""#);
    let r = refused("fromEngineText", r#"{"statement":"CREATE TABLE T (X DOUBLE)"}"#);
    assert_eq!(
        (r.kind(), r.message.as_str()),
        ("DIALECT_CAPABILITY", "the engine's H2 text reached MongoDb, which runs no SQL text")
    );
    assert_eq!(ok("raw", r#"{"statement":"{\"find\": 1}","writtenFor":null}"#), r#""{\"find\": 1}""#);
    assert_eq!(ok("raw", r#"{"statement":"x","writtenFor":"MongoDb"}"#), r#""x""#);
    assert_eq!(refused("raw", r#"{"statement":"x","writtenFor":"H2"}"#).message, r.message);
}

#[test]
fn the_interface_answers_with_its_defaults() {
    // the wire's fold, no carrier convention of its own
    assert_eq!(ok("wireTypeName", r#"{"driverTypeName":"BIGINT"}"#), r#""BIGINT""#);
    assert_eq!(ok("typingWireName", r#"{"driverTypeName":"VARCHAR"}"#), r#""VARCHAR""#);
    assert_eq!(ok("wireTypeName", r#"{"driverTypeName":null}"#), r#""""#);
    assert_eq!(ok("deliversOwn", r#"{"label":{"_type":"scalar","name":"BIGINT"},"wire":"DECIMAL(19,0)"}"#), "false");
    assert_eq!(ok("script", r#"{"statements":["a","b"]}"#), r#""a;\nb;""#);
    assert_eq!(ok("failingStatement", r#"{"message":"x","statements":["a"]}"#), "null");
    let r = refused("catalogColumnsQuery", r#"{"schema":"s","table":"t"}"#);
    assert_eq!(
        (r.kind(), r.message.as_str()),
        ("UNSUPPORTED_OPERATION", "the platform asks no catalog question of MongoDb")
    );
    assert_eq!(
        refused("bareNameReaches", r#"{"catalogName":"t"}"#).message,
        "reading this database's catalog names is not implemented for MongoDb"
    );
    let column =
        r#"{"column":{"name":"n","dataType":"long","logicalType":null,"precision":19,"scale":0,"notNull":false}}"#;
    assert_eq!(
        refused("catalogType", column).message,
        "reading this database's catalog type 'long' is not implemented for MongoDb"
    );
    assert_eq!(ok("copySelectList", r#"{"conversions":[]}"#), r#""*""#);
    assert_eq!(
        refused("copySelectList", r#"{"conversions":[{"column":"c","sql":"x"}]}"#).message,
        "a copy that converts columns in its star select is not implemented for MongoDb"
    );
    for op in ["host.openDatabase", "host.useDatabase", "host.closeDatabase"] {
        let r = refused(op, r#"{"name":"p"}"#);
        assert!(r.message.contains("on MongoDb"), "{op}: {r}");
    }
    let r = refused("host.statements", r#"{"operation":{"op":"platform"}}"#);
    assert!(r.message.contains(" on MongoDb is not built"), "{r}");
    // an objectId (OTHER) is read as its hex text; no other stored type, DISTINCT included, is
    let reads = |kind: &str| ok("readsStored", &format!(r#"{{"type":{{"_type":"plain","kind":"{kind}"}}}}"#));
    assert_eq!(reads("OTHER"), "true");
    assert_eq!(reads("DISTINCT"), "false");
    assert_eq!(reads("VARCHAR"), "false");
    assert_eq!(refused("duckdb.tables", "{}").message, "'duckdb.tables' is DuckDb's op, not MongoDb's");
    assert_eq!(
        refused("engine.selectList", &wrap("query", SELECT)).message,
        "'engine.selectList' is an engine-text dialect's op (EngineStyleH2, EngineStyleDB2, EngineStyleComposite), not \
         MongoDb's"
    );
}
