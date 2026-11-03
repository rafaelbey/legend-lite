// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! The seam the JVM calls a Rust dialect through (docs/rust-port/PHASE1.md §3; `lite-ffi`'s `lite_dialect`): one op of
//! a dialect's `SqlDialect` surface, JSON in and JSON out. `SEAM.md` is its contract -- every op, its args, its
//! answer, its refusals -- and the JVM's side is written against it.
//!
//! A dialect is made fresh for each call from its spec (DuckDB carries no state from one render to the next). A MIR is
//! read with `lite_sql`, nested at most [`MAX_DEPTH`] deep; every other record has the JSON form `SEAM.md` gives it:
//! an object of the Java record's components by name, in declaration order, a nested record the same way, a Java
//! enum by its `name()`. Every refusal is a [`Refusal`]: Java's class and message.

use crate::catalog_model::{CatalogColumn, CatalogType, Conversion};
use crate::dialect::SqlDialect;
use crate::duckdb::DuckDb;
use crate::host_ops;
use crate::refusal::Refusal;
use crate::statement::{Bind, Binding, Digits, Region, RenderedStatement, TypeHole, TypeSpelling};
use lite_json::{Node, Num, Obj, parse, parse_with_depth, to_compact};
use lite_sql::{MAX_DEPTH, SqlDdl, SqlDml, SqlQuery, ValueKind, read_ddl, read_dml, read_query, read_type};

/// The dialects the seam opens, by the name a spec gives (Java's simple class name).
pub const DIALECTS: &[&str] = &["DuckDb"];

/// Every op the seam answers (`SEAM.md`, "Ops").
pub const OPS: &[&str] = &[
    "facts",
    "query.render",
    "query.renderInline",
    "query.renderStatement",
    "ddl.render",
    "dml.render",
    "dml.renderStatement",
    "sessionTable",
    "physicalName",
    "storedName",
    "insertColumnName",
    "raw",
    "fromEngineText",
    "wireTypeName",
    "typingWireName",
    "deliversOwn",
    "script",
    "failingStatement",
    "catalogType",
    "catalogColumnsQuery",
    "bareNameReaches",
    "copySelectList",
    "host.openDatabase",
    "host.useDatabase",
    "host.closeDatabase",
    "host.statements",
    "duckdb.tables",
    "duckdb.readsStored",
];

/// One op of the dialect `spec` names, over `args`: the answer as compact JSON text, or Java's refusal. Refused in
/// this order: the spec, the op, the args' JSON, then whatever the op itself refuses.
pub fn dispatch(spec: &str, op: &str, args: &str) -> Result<String, Refusal> {
    let dialect = open(spec)?;
    if !OPS.contains(&op) {
        return Err(Refusal::illegal_argument(format!("unknown dialect op '{op}'")));
    }
    // the args object holds the MIR one level down: the MIR itself nests MAX_DEPTH deep, as MirJson reads it
    let args = parse_with_depth(args, MAX_DEPTH + 1).map_err(json)?;
    let Node::Obj(args) = args else {
        return Err(Refusal::illegal_argument(format!("the args of '{op}' are a JSON object")));
    };
    Ok(to_compact(&answer(dialect.as_ref(), op, &args)?))
}

/// The dialect a spec names (`{"dialect": name, "server": version or null, "engineScanOrder": bool}`), made fresh and
/// then taken for its server (`forServer`) when the spec names one.
pub fn open(spec: &str) -> Result<Box<dyn SqlDialect>, Refusal> {
    let spec = parse(spec).map_err(json)?;
    let Node::Obj(spec) = spec else {
        return Err(Refusal::illegal_argument("a dialect's spec is a JSON object"));
    };
    let name = spec.str("dialect").map_err(json)?;
    let server = spec.str_or_null("server").map_err(json)?;
    let engine_scan_order = match spec.get("engineScanOrder") {
        None => false,
        Some(_) => spec.bool("engineScanOrder").map_err(json)?,
    };
    let dialect: Box<dyn SqlDialect> = match name {
        "DuckDb" => {
            let mut d = DuckDb::new();
            d.engine_scan_order = engine_scan_order;
            Box::new(d)
        }
        other => {
            return Err(Refusal::illegal_argument(format!(
                "unknown dialect '{other}': Rust answers {}",
                DIALECTS.join(", ")
            )));
        }
    };
    Ok(match server.and_then(|version| dialect.for_server(version)) {
        Some(for_server) => for_server,
        None => dialect,
    })
}

fn answer(d: &dyn SqlDialect, op: &str, a: &Obj) -> Result<Node, Refusal> {
    Ok(match op {
        "facts" => facts(d),
        "duckdb.tables" => duckdb_tables(),
        // DuckDB's `readsStored(ColumnType)` (a function of the type alone), which the JVM's DuckDb still offers
        "duckdb.readsStored" => {
            let t = lite_sql::read_column_type(a.obj("type").map_err(json)?).map_err(json)?;
            Node::Bool(<DuckDb as crate::renderer::AnsiSqlRenderer>::reads_stored(&t))
        }
        "query.render" => text(d.render(query(a)?)?),
        "query.renderInline" => text(d.render_inline(query(a)?)?),
        "query.renderStatement" => statement_json(&d.render_statement(query(a)?)?),
        "ddl.render" => text(d.render_ddl(&read_ddl(a.obj("ddl").map_err(json)?).map_err(json)?)?),
        "dml.render" => text(d.render_dml(&read_dml(a.obj("dml").map_err(json)?).map_err(json)?)?),
        "dml.renderStatement" => {
            statement_json(&d.render_dml_statement(&read_dml(a.obj("dml").map_err(json)?).map_err(json)?)?)
        }
        "sessionTable" => {
            let (table, rows) = session_table_args(a)?;
            texts(d.session_table(&table, rows.as_ref())?)
        }
        "physicalName" => text(d.physical_name(string(a, "name")?)),
        "storedName" => text(d.stored_name(string(a, "name")?, boolean(a, "declaredQuoted")?)),
        "insertColumnName" => text(d.insert_column_name(string(a, "name")?, boolean(a, "declaredQuoted")?)),
        "raw" => text(d.raw(string(a, "statement")?, string_or_null(a, "writtenFor")?)?),
        "fromEngineText" => text(d.from_engine_text(string(a, "statement")?)?),
        "wireTypeName" => text(d.wire_type_name(string_or_null(a, "driverTypeName")?)),
        "typingWireName" => d.typing_wire_name(string_or_null(a, "driverTypeName")?).map_or(Node::Null, Node::Str),
        "deliversOwn" => {
            let label = read_type(a.obj("label").map_err(json)?).map_err(json)?;
            Node::Bool(d.delivers_own(&label, string(a, "wire")?))
        }
        "script" => text(d.script(&strings(a, "statements")?)),
        "failingStatement" => d
            .failing_statement(string(a, "message")?, &strings(a, "statements")?)
            .map_or(Node::Null, |i| number(i as i64)),
        "catalogType" => catalog_type_json(&d.catalog_type(&read_catalog_column(a.obj("column").map_err(json)?)?)?),
        "catalogColumnsQuery" => text(d.catalog_columns_query(string(a, "schema")?, string(a, "table")?)?),
        "bareNameReaches" => Node::Bool(d.bare_name_reaches(string(a, "catalogName")?)?),
        "copySelectList" => {
            let conversions = a
                .arr("conversions")
                .map_err(json)?
                .iter()
                .map(|n| read_conversion(n.as_obj().map_err(json)?))
                .collect::<Result<Vec<_>, Refusal>>()?;
            text(d.copy_select_list(&conversions)?)
        }
        "host.openDatabase" => texts(d.host_operations().open_database(string(a, "name")?)?),
        "host.useDatabase" => texts(d.host_operations().use_database(string(a, "name")?)?),
        "host.closeDatabase" => texts(d.host_operations().close_database(string(a, "name")?)?),
        "host.statements" => {
            let Some(operation) = a.get("operation") else {
                return Err(Refusal::illegal_argument("missing field 'operation'"));
            };
            texts(d.host_operations().statements(&host_ops::read_operation(operation)?)?)
        }
        other => return Err(Refusal::illegal_argument(format!("unknown dialect op '{other}'"))),
    })
}

/// The dialect's facts, read once by the JVM: its `SqlDialect` answers that take no argument.
fn facts(d: &dyn SqlDialect) -> Node {
    let keys_snapshot = match d.keys_snapshot() {
        Ok(k) => Node::Str(k.as_str().to_owned()),
        Err(r) => refused_json(&r),
    };
    let result_column_limit = match d.result_column_limit() {
        Ok(limit) => limit.map_or(Node::Null, |n| number(i64::from(n))),
        Err(r) => refused_json(&r),
    };
    object(vec![
        ("temporaryTables", Node::Bool(d.temporary_tables())),
        ("loadsRowsInBulk", Node::Bool(d.loads_rows_in_bulk())),
        ("relational", Node::Bool(d.relational())),
        ("inProcess", Node::Bool(d.in_process())),
        ("dropsStagedTablesWithTheirTransaction", Node::Bool(d.drops_staged_tables_with_their_transaction())),
        ("needsStaticPivot", Node::Bool(d.needs_static_pivot())),
        ("serverVersions", d.server_versions().map_or(Node::Null, texts)),
        ("inMemoryPath", d.in_memory_path().map_or(Node::Null, Node::Str)),
        ("scriptAbort", d.script_abort().map_or(Node::Null, Node::Str)),
        ("sessionSetup", texts(d.session_setup())),
        ("keysSnapshot", keys_snapshot),
        ("resultColumnLimit", result_column_limit),
    ])
}

/// DuckDB's own tables, which the JVM's `com.legend.sql.dialect.DuckDb` still offers by name: its `LEXICON` (the quote
/// character and the reserved words, sorted), `CATALOG_TYPES` (each canonical type and how a Database declares it, in
/// its order) and `CATALOG_REFUSED` (each refused type and its reason).
fn duckdb_tables() -> Node {
    let lexicon = crate::duckdb::lexicon();
    let mut words: Vec<&str> = lexicon.reserved_words().collect();
    words.sort_unstable();
    object(vec![
        (
            "lexicon",
            object(vec![
                ("quoteChar", Node::Str(lexicon.quote_char().to_string())),
                ("reservedWords", Node::Arr(words.into_iter().map(|w| Node::Str(w.to_owned())).collect())),
            ]),
        ),
        (
            "catalogTypes",
            object(crate::duckdb::catalog_types().iter().map(|(name, t)| (*name, catalog_type_json(t))).collect()),
        ),
        (
            "catalogRefused",
            object(
                crate::duckdb::CATALOG_REFUSED.iter().map(|(name, why)| (*name, Node::Str((*why).to_owned()))).collect(),
            ),
        ),
    ])
}

/// A fact's refusal: `{"refused": {"kind": <the seam's kind>, "message": <the seam's message>}}`.
pub fn refused_json(r: &Refusal) -> Node {
    object(vec![(
        "refused",
        object(vec![("kind", Node::Str(r.kind().to_owned())), ("message", Node::Str(r.seam_message()))]),
    )])
}

fn query(a: &Obj) -> Result<SqlQuery, Refusal> {
    read_query(a.obj("query").map_err(json)?).map_err(json)
}

/// `sessionTable`'s table (a `createTable`, Java's `SqlDdl.CreateTable`) and rows (an `insertValues` or null).
fn session_table_args(a: &Obj) -> Result<(SqlDdl, Option<SqlDml>), Refusal> {
    let table_json = a.obj("table").map_err(json)?;
    let table = read_ddl(table_json).map_err(json)?;
    if !matches!(table, SqlDdl::CreateTable { .. }) {
        return Err(Refusal::illegal_argument(format!(
            "sessionTable's table is a createTable, not {}",
            table_json.str("_type").unwrap_or("?")
        )));
    }
    let rows = match a.obj_or_null("rows").map_err(json)? {
        None => None,
        Some(rows_json) => {
            let rows = read_dml(rows_json).map_err(json)?;
            if !matches!(rows, SqlDml::InsertValues { .. }) {
                return Err(Refusal::illegal_argument(format!(
                    "sessionTable's rows are an insertValues, not {}",
                    rows_json.str("_type").unwrap_or("?")
                )));
            }
            Some(rows)
        }
    };
    Ok((table, rows))
}

// ---- the records' JSON forms

/// A rendered statement: `{"sql", "binds": [Bind], "regions": [Region]}`.
pub fn statement_json(s: &RenderedStatement) -> Node {
    object(vec![
        ("sql", Node::Str(s.sql.clone())),
        ("binds", Node::Arr(s.binds.iter().map(bind_json).collect())),
        ("regions", Node::Arr(s.regions.iter().map(region_json).collect())),
    ])
}

fn bind_json(b: &Bind) -> Node {
    object(vec![("parameter", Node::Str(b.parameter.clone())), ("binding", binding_json(&b.binding))])
}

/// A binding, tagged by its record: `one`, `array` or `key`.
pub fn binding_json(b: &Binding) -> Node {
    match b {
        Binding::One { null_type, hole } => object(vec![
            ("_type", Node::Str("one".to_owned())),
            ("nullType", Node::Str(null_type.clone())),
            ("hole", hole.as_ref().map_or(Node::Null, type_hole_json)),
        ]),
        Binding::Array { element_sql_type } => object(vec![
            ("_type", Node::Str("array".to_owned())),
            ("elementSqlType", Node::Str(element_sql_type.clone())),
        ]),
        Binding::Key { cell } => {
            object(vec![("_type", Node::Str("key".to_owned())), ("cell", number(i64::from(*cell)))])
        }
    }
}

/// A type hole: `{"at", "types": {<ValueKind>: TypeSpelling, ...} in the kinds' declared order, "absent"}`.
pub fn type_hole_json(h: &TypeHole) -> Node {
    let mut types = Obj::new();
    for (kind, spelling) in &h.types {
        types.insert(kind.as_str(), type_spelling_json(spelling));
    }
    object(vec![
        ("at", number(h.at as i64)),
        ("types", Node::Obj(types)),
        ("absent", Node::Str(h.absent.as_str().to_owned())),
    ])
}

fn type_spelling_json(t: &TypeSpelling) -> Node {
    object(vec![
        ("name", Node::Str(t.name.clone())),
        ("digits", Node::Str(t.digits.as_str().to_owned())),
        ("fraction", t.fraction.map_or(Node::Null, |f| number(i64::from(f)))),
    ])
}

fn region_json(r: &Region) -> Node {
    object(vec![
        ("start", number(r.start as i64)),
        ("end", number(r.end as i64)),
        ("lead", Node::Str(r.lead.clone())),
        ("separator", Node::Str(r.separator.clone())),
        ("empty", r.empty.clone().map_or(Node::Null, Node::Str)),
        ("firstBind", number(r.first_bind as i64)),
        ("bindCount", number(r.bind_count as i64)),
        ("counters", Node::Arr(r.counters.iter().map(|&c| number(c as i64)).collect())),
    ])
}

/// A catalog type: `{"read", "declared", "conversion", "reason"}`, each absent part null.
pub fn catalog_type_json(t: &CatalogType) -> Node {
    let text_or_null = |s: &Option<String>| s.clone().map_or(Node::Null, Node::Str);
    object(vec![
        ("read", Node::Str(t.read.as_str().to_owned())),
        ("declared", text_or_null(&t.declared)),
        ("conversion", text_or_null(&t.conversion)),
        ("reason", text_or_null(&t.reason)),
    ])
}

/// A catalog column: `{"name", "dataType", "logicalType", "precision", "scale", "notNull"}`, the nullable ones null
/// or missing.
pub fn read_catalog_column(o: &Obj) -> Result<CatalogColumn, Refusal> {
    Ok(CatalogColumn {
        name: o.str("name").map_err(json)?.to_owned(),
        data_type: o.str("dataType").map_err(json)?.to_owned(),
        logical_type: o.str_or_null("logicalType").map_err(json)?.map(str::to_owned),
        precision: int_or_null(o, "precision")?,
        scale: int_or_null(o, "scale")?,
        not_null: o.bool("notNull").map_err(json)?,
    })
}

/// A conversion: `{"column", "sql"}`.
pub fn read_conversion(o: &Obj) -> Result<Conversion, Refusal> {
    Ok(Conversion { column: o.str("column").map_err(json)?.to_owned(), sql: o.str("sql").map_err(json)?.to_owned() })
}

/// A rendered statement read back from [`statement_json`]'s form, through the records' constructors and their checks
/// (the JVM's reading of an answer; the tests'.)
pub fn read_statement(o: &Obj) -> Result<RenderedStatement, Refusal> {
    let binds = o
        .arr("binds")
        .map_err(json)?
        .iter()
        .map(|n| {
            let b = n.as_obj().map_err(json)?;
            Ok(Bind {
                parameter: b.str("parameter").map_err(json)?.to_owned(),
                binding: read_binding(b.obj("binding").map_err(json)?)?,
            })
        })
        .collect::<Result<Vec<_>, Refusal>>()?;
    let regions = o
        .arr("regions")
        .map_err(json)?
        .iter()
        .map(|n| {
            let r = n.as_obj().map_err(json)?;
            Region::new(
                index(r, "start")?,
                index(r, "end")?,
                r.str("lead").map_err(json)?.to_owned(),
                r.str("separator").map_err(json)?.to_owned(),
                r.str_or_null("empty").map_err(json)?.map(str::to_owned),
                index(r, "firstBind")?,
                index(r, "bindCount")?,
                r.arr("counters").map_err(json)?.iter().map(index_of).collect::<Result<_, _>>()?,
            )
        })
        .collect::<Result<Vec<_>, Refusal>>()?;
    RenderedStatement::new(o.str("sql").map_err(json)?.to_owned(), binds, regions)
}

fn read_binding(o: &Obj) -> Result<Binding, Refusal> {
    match o.str("_type").map_err(json)? {
        "one" => {
            let hole = match o.obj_or_null("hole").map_err(json)? {
                None => None,
                Some(h) => {
                    let mut types = Vec::new();
                    for (kind, spelling) in h.obj("types").map_err(json)?.iter() {
                        let s = spelling.as_obj().map_err(json)?;
                        let digits = s.str("digits").map_err(json)?;
                        types.push((
                            value_kind(kind)?,
                            TypeSpelling {
                                name: s.str("name").map_err(json)?.to_owned(),
                                digits: Digits::from_name(digits).ok_or_else(|| {
                                    Refusal::illegal_argument(format!("unknown type digits '{digits}'"))
                                })?,
                                fraction: int_or_null(s, "fraction")?,
                            },
                        ));
                    }
                    Some(TypeHole::new(index(h, "at")?, types, value_kind(h.str("absent").map_err(json)?)?)?)
                }
            };
            Ok(Binding::One { null_type: o.str("nullType").map_err(json)?.to_owned(), hole })
        }
        "array" => Ok(Binding::Array { element_sql_type: o.str("elementSqlType").map_err(json)?.to_owned() }),
        "key" => {
            let cell = o.i64("cell").map_err(json)?;
            Binding::key(i32::try_from(cell).map_err(|_| Refusal::illegal_argument(format!("a key's cell {cell}")))?)
        }
        other => Err(Refusal::illegal_argument(format!("unknown binding tag '{other}'"))),
    }
}

fn value_kind(name: &str) -> Result<ValueKind, Refusal> {
    ValueKind::from_name(name).ok_or_else(|| Refusal::illegal_argument(format!("unknown value kind '{name}'")))
}

// ---- reading and writing JSON

/// A JSON or MirJson reading failure: Java's `IllegalArgumentException`, its message the reader's.
fn json(e: lite_json::Error) -> Refusal {
    Refusal::illegal_argument(e.0)
}

fn object(fields: Vec<(&str, Node)>) -> Node {
    let mut o = Obj::new();
    for (k, v) in fields {
        o.insert(k, v);
    }
    Node::Obj(o)
}

fn number(n: i64) -> Node {
    Node::Num(Num::from_i64(n))
}

fn text(s: String) -> Node {
    Node::Str(s)
}

fn texts(items: Vec<String>) -> Node {
    Node::Arr(items.into_iter().map(Node::Str).collect())
}

fn string<'a>(a: &'a Obj, key: &str) -> Result<&'a str, Refusal> {
    a.str(key).map_err(json)
}

fn string_or_null<'a>(a: &'a Obj, key: &str) -> Result<Option<&'a str>, Refusal> {
    a.str_or_null(key).map_err(json)
}

fn boolean(a: &Obj, key: &str) -> Result<bool, Refusal> {
    a.bool(key).map_err(json)
}

fn strings(a: &Obj, key: &str) -> Result<Vec<String>, Refusal> {
    a.arr(key)
        .map_err(json)?
        .iter()
        .map(|n| match n {
            Node::Str(s) => Ok(s.clone()),
            _ => Err(Refusal::illegal_argument(format!("field '{key}' is an array of strings"))),
        })
        .collect()
}

/// An `Integer` component: an int, or null when missing or null.
fn int_or_null(o: &Obj, key: &str) -> Result<Option<i32>, Refusal> {
    if o.get_or_null(key).is_none() {
        return Ok(None);
    }
    let v = o.i64(key).map_err(json)?;
    i32::try_from(v).map(Some).map_err(|_| Refusal::illegal_argument(format!("field '{key}' is not an int: {v}")))
}

/// A position or count: a non-negative int.
fn index(o: &Obj, key: &str) -> Result<usize, Refusal> {
    let v = o.i64(key).map_err(json)?;
    usize::try_from(v).map_err(|_| Refusal::illegal_argument(format!("field '{key}' is not a position: {v}")))
}

fn index_of(n: &Node) -> Result<usize, Refusal> {
    match n {
        Node::Num(num) => num
            .as_i64()
            .and_then(|v| usize::try_from(v).ok())
            .ok_or_else(|| Refusal::illegal_argument(format!("not a position: {}", num.token()))),
        _ => Err(Refusal::illegal_argument("a position is a number")),
    }
}

#[cfg(test)]
mod tests;
