// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! Every op of the seam through [`dispatch`] for the engine-text dialects (`EngineStyleH2`, `EngineStyleDB2`,
//! `EngineStyleComposite`), each answer held to what Java answers for them: their own overrides, `AnsiSqlRenderer`'s
//! implementations, `SqlDialect`'s defaults naming the dialect, and `EngineTextDialect`'s five methods (`SEAM.md`
//! §3b). The golden oracles hold the renders and the engine ops (`tests/dialect_differential.rs`).

use super::*;

const H2_TEXT: &str = r#"{"dialect":"EngineStyleH2","server":null}"#;
const DB2_TEXT: &str = r#"{"dialect":"EngineStyleDB2","server":null}"#;
const COMPOSITE_TEXT: &str = r#"{"dialect":"EngineStyleComposite","server":null}"#;

/// The three, with their simple names.
const ENGINE_TEXTS: [(&str, &str); 3] =
    [(H2_TEXT, "EngineStyleH2"), (DB2_TEXT, "EngineStyleDB2"), (COMPOSITE_TEXT, "EngineStyleComposite")];

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

/// `select t0.ID as id, t1.NAME as name from personTable as t0 join personTable as t1 on t0.ID = t1.ID where t0.D =
/// <an optional DATETIME parameter>`.
const SELF_JOIN: &str = r#"{"_type":"select","projections":[{"expr":{"_type":"column","type":{"_type":"unknown"},"table":"t0","name":"ID","origin":"PHYSICAL"},"alias":"id","out":null},{"expr":{"_type":"column","type":{"_type":"unknown"},"table":"t1","name":"NAME","origin":"PHYSICAL"},"alias":"name","out":null}],"distinct":false,"from":{"_type":"join","left":{"_type":"table","name":"personTable","alias":"t0","outputs":[],"call":false,"storedTypes":{},"quoting":{"schema":false,"table":false}},"right":{"_type":"table","name":"personTable","alias":"t1","outputs":[],"call":false,"storedTypes":{},"quoting":{"schema":false,"table":false}},"kind":"INNER","on":{"_type":"call","type":{"_type":"unknown"},"fn":"EQUAL","args":[{"_type":"column","type":{"_type":"unknown"},"table":"t0","name":"ID","origin":"PHYSICAL"},{"_type":"column","type":{"_type":"unknown"},"table":"t1","name":"ID","origin":"PHYSICAL"}]},"outputs":[]},"where":{"_type":"call","type":{"_type":"unknown"},"fn":"EQUAL","args":[{"_type":"column","type":{"_type":"unknown"},"table":"t0","name":"D","origin":"PHYSICAL"},{"_type":"planParam","type":{"_type":"unknown"},"name":"d","kind":"DATETIME","optional":true,"enumMapFn":null,"byValue":null}]},"groupBy":[],"having":null,"qualify":null,"orderBy":[],"limit":null,"offset":null,"outputs":[]}"#;

/// `with c as (select 1) select * from c as c0`: no select, so no select list.
const WITH: &str = r#"{"_type":"with","ctes":[{"name":"c","query":{"_type":"select","projections":[{"expr":{"_type":"intLit","type":{"_type":"typed","sqlType":{"_type":"scalar","name":"BIGINT"},"nullable":false},"value":1},"alias":"one","out":null}],"distinct":false,"from":{"_type":"dual"},"where":null,"groupBy":[],"having":null,"qualify":null,"orderBy":[],"limit":null,"offset":null,"outputs":[]},"materialized":false}],"body":{"_type":"select","projections":[],"distinct":false,"from":{"_type":"cte","name":"c","alias":"c0","outputs":[]},"where":null,"groupBy":[],"having":null,"qualify":null,"orderBy":[],"limit":null,"offset":null,"outputs":[]},"recursive":false}"#;

const STRING_PARAM: &str = r#"{"_type":"planParam","type":{"_type":"unknown"},"name":"names","kind":"STRING","optional":false,"enumMapFn":null,"byValue":null}"#;

#[test]
fn the_seam_opens_the_three_with_their_configuration() {
    // the default configuration, its fields missing or given
    for (spec, name) in ENGINE_TEXTS {
        assert_eq!(ok(spec, "forServer", r#"{"version":"2.1.214"}"#), format!("\"{name}\""));
        let given = format!(r#"{{"dialect":"{name}","quoteIdentifiers":false,"timeZone":null}}"#);
        assert_eq!(ok(&given, "physicalName", r#"{"name":"a b"}"#), r#""\"a b\"""#);
    }
    // quoteIdentifiers and timeZone: the plan text's connection (Java's two-argument constructors)
    let quoted = r#"{"dialect":"EngineStyleH2","quoteIdentifiers":true}"#;
    // (in the answer's JSON a quote is \")
    assert_eq!(ok(quoted, "query.render", &wrap("query", SELF_JOIN)).matches(r#"\"ID\""#).count(), 3);
    let zoned = r#"{"dialect":"EngineStyleDB2","quoteIdentifiers":false,"timeZone":"EST"}"#;
    assert!(ok(zoned, "query.render", &wrap("query", SELF_JOIN)).contains("GMTtoTZ( \\\"[EST]\\\" d![])"));
    // refused: a malformed field, a configured Composite, a configured dialect of any other family
    assert_eq!(
        refused(r#"{"dialect":"EngineStyleH2","quoteIdentifiers":"yes"}"#, "facts", "{}").message,
        "field 'quoteIdentifiers' is a string, not a boolean"
    );
    assert_eq!(
        refused(r#"{"dialect":"EngineStyleH2","timeZone":5}"#, "facts", "{}").message,
        "field 'timeZone' is a number, not a string"
    );
    let r = refused(r#"{"dialect":"EngineStyleComposite","timeZone":"GMT"}"#, "facts", "{}");
    assert_eq!(
        (r.class, r.message.as_str()),
        (
            Refusal::ILLEGAL_ARGUMENT,
            "EngineStyleComposite has only its default configuration (Java's one constructor: quoteIdentifiers false, \
             timeZone null)"
        )
    );
    assert_eq!(
        refused(r#"{"dialect":"EngineStyleComposite","quoteIdentifiers":true}"#, "facts", "{}").class,
        Refusal::ILLEGAL_ARGUMENT
    );
    for name in ["DuckDb", "H2", "H2Modern"] {
        let r = refused(&format!(r#"{{"dialect":"{name}","quoteIdentifiers":false}}"#), "facts", "{}");
        assert_eq!(
            r.message,
            format!(
                "quoteIdentifiers and timeZone configure an engine-text dialect (EngineStyleH2, EngineStyleDB2), not {name}"
            )
        );
        assert!(dispatch(&format!(r#"{{"dialect":"{name}","timeZone":null}}"#), "facts", "{}").is_err());
    }
}

#[test]
fn facts_are_the_interfaces_defaults_naming_the_dialect() {
    for (spec, name) in ENGINE_TEXTS {
        assert_eq!(
            ok(spec, "facts", "{}"),
            format!(
                r#"{{"temporaryTables":true,"loadsRowsInBulk":false,"relational":true,"inProcess":false,"dropsStagedTablesWithTheirTransaction":false,"needsStaticPivot":false,"serverVersions":null,"inMemoryPath":null,"scriptAbort":null,"sessionSetup":[],"keysSnapshot":{{"refused":{{"kind":"DIALECT_CAPABILITY","message":"a dynamic pivot's probe plan reached {name}, which says no snapshot for it"}}}},"resultColumnLimit":{{"refused":{{"kind":"DIALECT_CAPABILITY","message":"a dynamic pivot's probe plan reached {name}, which says no column limit for it"}}}}}}"#
            )
        );
    }
}

#[test]
fn queries_render_as_the_engines_text_and_bind_nothing() {
    let holder = r#"${varPlaceHolderToString(d![] \"TIMESTAMP'\" \"'\" {} \"null\")}"#;
    let h2 = format!(
        r#""select \"root\".ID as \"id\", \"persontable_1\".NAME as \"name\" from personTable as \"root\" inner join personTable as \"persontable_1\" on (\"root\".ID = \"persontable_1\".ID) where \"root\".D is not distinct from {holder}""#
    );
    assert_eq!(ok(H2_TEXT, "query.render", &wrap("query", SELF_JOIN)), h2);
    // one line already: inline is the same text
    assert_eq!(ok(H2_TEXT, "query.renderInline", &wrap("query", SELF_JOIN)), h2);
    // DB2: no IS NOT DISTINCT FROM, the OR form
    assert!(
        ok(DB2_TEXT, "query.render", &wrap("query", SELF_JOIN))
            .ends_with(&format!(r#"where (\"root\".D = {holder} or (\"root\".D is null and {holder} is null))""#))
    );
    for (spec, _) in ENGINE_TEXTS {
        let r = refused(spec, "query.renderStatement", &wrap("query", SELF_JOIN));
        assert_eq!(
            (r.class, r.message.as_str()),
            (
                Refusal::DIALECT_CAPABILITY,
                "the legacy engine-text plan binds no parameters: its parameters are template variables in its text \
                 (render)"
            )
        );
    }
}

const CREATE_TABLE: &str = r#"{"_type":"createTable","schema":"S","table":"T","quoting":{"schema":true,"table":false},"columns":[{"name":"ID","declaredQuoted":false,"type":{"_type":"plain","kind":"INTEGER"},"notNull":true,"primaryKey":true},{"name":"order","declaredQuoted":false,"type":{"_type":"plain","kind":"OTHER"},"notNull":false,"primaryKey":false}],"temporary":false}"#;

const INSERT_VALUES: &str = r#"{"_type":"insertValues","schema":"S","table":"T","quoting":{"schema":false,"table":false},"columns":["ID","order"],"quotedColumns":[],"rows":[[{"_type":"stringLit","type":{"_type":"typed","sqlType":{"_type":"scalar","name":"VARCHAR"},"nullable":false},"value":"2"},{"_type":"nullLit","type":{"_type":"bottom"}}]]}"#;

#[test]
fn ddl_and_seed_rows_are_the_engines_text() {
    for (spec, _) in ENGINE_TEXTS {
        let create = r#""Create Table S.T(ID INT NOT NULL,\"order\" OTHER NULL, PRIMARY KEY(ID));""#;
        assert_eq!(ok(spec, "ddl.render", &wrap("ddl", CREATE_TABLE)), create);
        let insert = r#""insert into S.T (ID,\"order\") values ('2',null);""#;
        assert_eq!(ok(spec, "dml.render", &wrap("dml", INSERT_VALUES)), insert);
        assert_eq!(
            ok(spec, "dml.renderStatement", &wrap("dml", INSERT_VALUES)),
            format!(r#"{{"sql":{insert},"binds":[],"regions":[]}}"#)
        );
        assert_eq!(
            ok(spec, "sessionTable", &format!(r#"{{"table":{CREATE_TABLE},"rows":{INSERT_VALUES}}}"#)),
            format!("[{create},{insert}]")
        );
    }
    // a store write: the base's refusal, naming the dialect
    let delete = r#"{"_type":"deleteWhere","schema":null,"table":"T","quoting":{"schema":false,"table":false},"alias":"t","storedTypes":{},"where":null}"#;
    for (spec, name) in ENGINE_TEXTS {
        let r = refused(spec, "dml.render", &wrap("dml", delete));
        assert_eq!(
            r.message,
            format!(
                "a DELETE ... WHERE on {name}: no spelling measured on its database (the store writes run on DuckDB, \
                 H2 and Postgres, the writes design's leg 2)"
            )
        );
    }
}

#[test]
fn names_raw_text_and_the_interfaces_defaults() {
    for (spec, name) in ENGINE_TEXTS {
        // a stored name as the store wrote it, quoted only where the base's identifier rule needs it
        assert_eq!(ok(spec, "physicalName", r#"{"name":"Orders"}"#), r#""Orders""#);
        assert_eq!(ok(spec, "physicalName", r#"{"name":"order"}"#), r#""order""#);
        assert_eq!(ok(spec, "storedName", r#"{"name":"Orders","declaredQuoted":true}"#), r#""Orders""#);
        assert_eq!(ok(spec, "insertColumnName", r#"{"name":"order","declaredQuoted":false}"#), r#""\"order\"""#);
        assert_eq!(ok(spec, "insertColumnName", r#"{"name":"a$b","declaredQuoted":false}"#), r#""\"a$b\"""#);
        assert_eq!(ok(spec, "insertColumnName", r#"{"name":"Amt","declaredQuoted":true}"#), r#""\"Amt\"""#);
        // the engine's H2 text is this text's own language
        assert_eq!(ok(spec, "fromEngineText", r#"{"statement":"select 1"}"#), r#""select 1""#);
        assert_eq!(ok(spec, "raw", r#"{"statement":"select 1","writtenFor":"H2"}"#), r#""select 1""#);
        assert_eq!(ok(spec, "wireTypeName", r#"{"driverTypeName":"CHARACTER VARYING"}"#), r#""VARCHAR""#);
        assert_eq!(ok(spec, "typingWireName", r#"{"driverTypeName":null}"#), r#""""#);
        assert_eq!(ok(spec, "deliversOwn", r#"{"label":{"_type":"scalar","name":"BIGINT"},"wire":"BIGINT"}"#), "false");
        assert_eq!(ok(spec, "script", r#"{"statements":["a","b"]}"#), r#""a;\nb;""#);
        assert_eq!(ok(spec, "failingStatement", r#"{"message":"SQL statement: a","statements":["a"]}"#), "null");
        // the catalog and the host: the interface's refusals, naming the dialect by its own simple name
        let r = refused(spec, "catalogColumnsQuery", r#"{"schema":"s","table":"t"}"#);
        assert_eq!(
            (r.class, r.message),
            (Refusal::UNSUPPORTED_OPERATION, format!("the platform asks no catalog question of {name}"))
        );
        let column = r#"{"column":{"name":"c","dataType":"INTEGER","notNull":true}}"#;
        assert_eq!(
            refused(spec, "catalogType", column).message,
            format!("reading this database's catalog type 'INTEGER' is not implemented for {name}")
        );
        let r = refused(spec, "bareNameReaches", r#"{"catalogName":"T"}"#);
        assert_eq!(
            (r.class, r.message),
            (
                Refusal::DIALECT_CAPABILITY,
                format!("reading this database's catalog names is not implemented for {name}")
            )
        );
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
        for op in ["duckdb.tables", "duckdb.readsStored"] {
            let r = refused(spec, op, r#"{"type":{"_type":"plain","kind":"OTHER"}}"#);
            assert_eq!(
                (r.class, r.message),
                (Refusal::ILLEGAL_ARGUMENT, format!("'{op}' is DuckDb's op, not {name}'s"))
            );
        }
        // the process's scan-order firings, every dialect's: a count
        assert!(ok(spec, "scanOrderFirings", "{}").parse::<u64>().is_ok());
    }
}

#[test]
fn the_engine_text_ops_answer_as_engine_text_dialect_does() {
    // renderAsAliased: the aliases the query carries
    assert!(ok(H2_TEXT, "engine.renderAsAliased", &wrap("query", SELF_JOIN)).starts_with(
        r#""select \"t0\".ID as \"id\", \"t1\".NAME as \"name\" from personTable as \"t0\" inner join personTable as \"t1\""#
    ));
    // selectList: the first select's list after the alias plan, null for a WITH
    assert_eq!(
        ok(DB2_TEXT, "engine.selectList", &wrap("query", SELF_JOIN)),
        r#""\"root\".ID as \"id\", \"persontable_1\".NAME as \"name\"""#
    );
    assert_eq!(ok(H2_TEXT, "engine.selectList", &wrap("query", WITH)), "null");
    // collectionSplice: the renderCollection template, DB2's boolean quoted
    assert_eq!(
        ok(H2_TEXT, "engine.collectionSplice", &wrap("param", STRING_PARAM)),
        r#""${renderCollection(names![] \",\" \"'\" \"'\" {\"'\" : \"''\" } \"null\")}""#
    );
    let boolean = STRING_PARAM.replace("STRING", "BOOLEAN");
    assert_eq!(
        ok(COMPOSITE_TEXT, "engine.collectionSplice", &wrap("param", &boolean)),
        r#""${renderCollection(names![] \",\" \"'\" \"'\" {} \"null\")}""#
    );
    let raw = STRING_PARAM.replace("STRING", "RAW");
    assert_eq!(
        refused(H2_TEXT, "engine.collectionSplice", &wrap("param", &raw)),
        Refusal::illegal_state("RAW plan params are bare splices, never collections")
    );
    let r = refused(H2_TEXT, "engine.collectionSplice", r#"{"param":{"_type":"nullLit","type":{"_type":"bottom"}}}"#);
    assert_eq!(
        (r.class, r.message.as_str()),
        (Refusal::ILLEGAL_ARGUMENT, "engine.collectionSplice's param is a planParam, not nullLit")
    );
    // renderedAlias: the alias itself on a dialect that rendered nothing; after a render, that render's plan
    assert_eq!(ok(H2_TEXT, "engine.renderedAlias", r#"{"alias":"t1"}"#), r#""t1""#);
    assert_eq!(ok(H2_TEXT, "engine.renderedAlias", r#"{"alias":"t1","rendered":null}"#), r#""t1""#);
    let after = |alias: &str| format!(r#"{{"alias":"{alias}","rendered":{SELF_JOIN}}}"#);
    assert_eq!(ok(H2_TEXT, "engine.renderedAlias", &after("t0")), r#""root""#);
    assert_eq!(ok(DB2_TEXT, "engine.renderedAlias", &after("t1")), r#""persontable_1""#);
    assert_eq!(ok(H2_TEXT, "engine.renderedAlias", &after("t7")), r#""t7""#);
    assert_eq!(refused(H2_TEXT, "engine.renderedAlias", r#"{"rendered":null}"#).message, "missing field 'alias'");
    // inTempTableSelect: the table's own lowercase alias
    assert_eq!(
        ok(DB2_TEXT, "engine.inTempTableSelect", r#"{"table":"SESSION.tempTableForIn_x"}"#),
        r#""select \"session.temptableforin_x_0\".ColumnForStoringInCollection as ColumnForStoringInCollection from SESSION.tempTableForIn_x as \"session.temptableforin_x_0\"""#
    );
    // every other dialect refuses the engine ops by name
    for name in ["DuckDb", "H2", "H2Modern"] {
        let r = refused(&format!(r#"{{"dialect":"{name}"}}"#), "engine.inTempTableSelect", r#"{"table":"t"}"#);
        assert_eq!(
            (r.class, r.message),
            (
                Refusal::ILLEGAL_ARGUMENT,
                format!(
                    "'engine.inTempTableSelect' is an engine-text dialect's op (EngineStyleH2, EngineStyleDB2, \
                     EngineStyleComposite), not {name}'s"
                )
            )
        );
    }
}
