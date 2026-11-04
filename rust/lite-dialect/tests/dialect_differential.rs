// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! The Rust dialects held to Java's own answers: every statement the golden oracles hold for a dialect (`LITE_ORACLES`,
//! the oracle files separated by whitespace, as Bazel's `$(rootpaths)` writes them: JSON lines of `{dialect, config?,
//! fresh, op, kind, mir, answer}`), answered by a fresh Rust dialect of that name and compared with what the JVM's
//! fresh dialect answered -- the SQL text exactly, a statement's binds and regions as Java's record text, a refusal by
//! its class and message. An input both oracles hold (the same op, configuration and MirJson) is one statement, so a
//! dialect's count is of its distinct inputs (an engine-text dialect's default configuration and the first dump's none
//! are one configuration). An engine-text dialect's own ops (`engine.*`) are answered as its `EngineTextDialect`
//! answers them: a query's text, a select list or null, a collection parameter's template, an alias, the temp table's
//! read.
//!
//! One test per dialect asserts that at least its floor (`tests/<dialect>-floor.txt`, or the file of that name among
//! `LITE_DIALECT_FLOORS`) of its statements are identical. Each prints the counts by op and by MIR kind, the first
//! mismatches, and each mismatch's likely cause: the pass whose signature its first difference carries, else the first
//! pass, in the order the dialect runs its passes, whose trigger the statement holds (`rust/lite-dialect/PARITY.md`).
//! With `LITE_DIALECT_MISMATCHES` naming a directory, each dialect writes its every mismatch to `<dialect>.txt` there.
//!
//! A typing test holds the typing rules (`lite_dialect::typing`, Java's `SqlTyping`) to the facts Java stored on every
//! node of every distinct statement of the oracles. A seam test per dialect holds the seam (`lite_dialect::dispatch`,
//! `SEAM.md`) to the renderer: every op of the dialect's statements answered through it exactly as a direct call
//! answers it.

use lite_dialect::rewriter::{Rw, SqlRewriter, rewrite};
use lite_dialect::seam::{open, read_statement, statement_json};
use lite_dialect::statement::list_text;
use lite_dialect::{EngineTextDialect, PlanParam, Refusal, RenderedStatement, SqlDialect, dispatch};
use lite_json::{Node, Obj, parse_with_depth, to_compact};
use lite_sql::{
    AggFn, ColumnKind, ColumnType, JoinKind, MAX_DEPTH, Origin, Projection, Scalar, SqlDml, SqlExpr, SqlFn, SqlQuery,
    SqlSelect, SqlSource, SqlType, TypeFact, read_ddl, read_dml, read_expr, read_query,
};
use std::collections::hash_map::DefaultHasher;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt::Write as _;
use std::hash::{Hash, Hasher};
use std::io::{BufRead, BufReader};

const SHOWN: usize = 30;

// ==================================================================
// The dialects
// ==================================================================

#[test]
#[ignore = "reads the JVM's oracles named by LITE_ORACLES"]
fn duckdb_renders_as_java_does() {
    renders_as_java_does("DuckDb");
}

#[test]
#[ignore = "reads the JVM's oracles named by LITE_ORACLES"]
fn h2_renders_as_java_does() {
    renders_as_java_does("H2");
}

#[test]
#[ignore = "reads the JVM's oracles named by LITE_ORACLES"]
fn h2modern_renders_as_java_does() {
    renders_as_java_does("H2Modern");
}

#[test]
#[ignore = "reads the JVM's oracles named by LITE_ORACLES"]
fn the_seam_answers_as_duckdbs_renderer_does() {
    the_seam_answers_as_the_renderer_does("DuckDb");
}

#[test]
#[ignore = "reads the JVM's oracles named by LITE_ORACLES"]
fn the_seam_answers_as_h2s_renderer_does() {
    the_seam_answers_as_the_renderer_does("H2");
}

#[test]
#[ignore = "reads the JVM's oracles named by LITE_ORACLES"]
fn the_seam_answers_as_h2moderns_renderer_does() {
    the_seam_answers_as_the_renderer_does("H2Modern");
}

#[test]
#[ignore = "reads the JVM's oracles named by LITE_ORACLES"]
fn enginestyleh2_renders_as_java_does() {
    renders_as_java_does("EngineStyleH2");
}

#[test]
#[ignore = "reads the JVM's oracles named by LITE_ORACLES"]
fn enginestyledb2_renders_as_java_does() {
    renders_as_java_does("EngineStyleDB2");
}

#[test]
#[ignore = "reads the JVM's oracles named by LITE_ORACLES"]
fn enginestylecomposite_renders_as_java_does() {
    renders_as_java_does("EngineStyleComposite");
}

#[test]
#[ignore = "reads the JVM's oracles named by LITE_ORACLES"]
fn the_seam_answers_as_enginestyleh2s_renderer_does() {
    the_seam_answers_as_the_renderer_does("EngineStyleH2");
}

#[test]
#[ignore = "reads the JVM's oracles named by LITE_ORACLES"]
fn the_seam_answers_as_enginestyledb2s_renderer_does() {
    the_seam_answers_as_the_renderer_does("EngineStyleDB2");
}

#[test]
#[ignore = "reads the JVM's oracles named by LITE_ORACLES"]
fn the_seam_answers_as_enginestylecomposites_renderer_does() {
    the_seam_answers_as_the_renderer_does("EngineStyleComposite");
}

#[test]
#[ignore = "reads the JVM's oracles named by LITE_ORACLES"]
fn postgres_renders_as_java_does() {
    renders_as_java_does("Postgres");
}

#[test]
#[ignore = "reads the JVM's oracles named by LITE_ORACLES"]
fn the_seam_answers_as_postgress_renderer_does() {
    the_seam_answers_as_the_renderer_does("Postgres");
}

#[test]
#[ignore = "reads the JVM's oracles named by LITE_ORACLES"]
fn sqlite_renders_as_java_does() {
    renders_as_java_does("SQLite");
}

#[test]
#[ignore = "reads the JVM's oracles named by LITE_ORACLES"]
fn the_seam_answers_as_sqlites_renderer_does() {
    the_seam_answers_as_the_renderer_does("SQLite");
}

#[test]
#[ignore = "reads the JVM's oracles named by LITE_ORACLES"]
fn trino_renders_as_java_does() {
    renders_as_java_does("Trino");
}

#[test]
#[ignore = "reads the JVM's oracles named by LITE_ORACLES"]
fn the_seam_answers_as_trinos_renderer_does() {
    the_seam_answers_as_the_renderer_does("Trino");
}

#[test]
#[ignore = "reads the JVM's oracles named by LITE_ORACLES"]
fn databricks_renders_as_java_does() {
    renders_as_java_does("Databricks");
}

#[test]
#[ignore = "reads the JVM's oracles named by LITE_ORACLES"]
fn the_seam_answers_as_databrickss_renderer_does() {
    the_seam_answers_as_the_renderer_does("Databricks");
}

#[test]
#[ignore = "reads the JVM's oracles named by LITE_ORACLES"]
fn clickhouse_renders_as_java_does() {
    renders_as_java_does("ClickHouse");
}

#[test]
#[ignore = "reads the JVM's oracles named by LITE_ORACLES"]
fn the_seam_answers_as_clickhouses_renderer_does() {
    the_seam_answers_as_the_renderer_does("ClickHouse");
}

/// An engine-text dialect's default configuration (Java's no-argument constructor), as the H2 dump writes it: an input
/// the first dump holds with no configuration is the same input.
const DEFAULT_ENGINE_CONFIG: &str = r#"{"quoteIdentifiers":false,"timeZone":null}"#;

/// The spec the JVM's shim of `dialect` sends (`SEAM.md`), with a configured dialect's configuration's fields.
fn spec(dialect: &str, config: Option<&Obj>) -> String {
    let mut spec = Obj::new();
    spec.insert("dialect", Node::Str(dialect.to_owned()));
    spec.insert("server", Node::Null);
    if dialect == "DuckDb" {
        spec.insert("engineScanOrder", Node::Bool(false));
    }
    if let Some(config) = config {
        for (k, v) in config.iter() {
            spec.insert(k, v.clone());
        }
    }
    to_compact(&Node::Obj(spec))
}

/// A fresh dialect, made as the seam makes it.
fn fresh(dialect: &str, config: Option<&Obj>) -> Box<dyn SqlDialect> {
    open(&spec(dialect, config)).unwrap_or_else(|r| panic!("the seam does not open {dialect}: {r}"))
}

// ==================================================================
// The oracles
// ==================================================================

/// The oracle files `LITE_ORACLES` names.
fn oracles() -> Vec<String> {
    let paths = std::env::var("LITE_ORACLES")
        .expect("LITE_ORACLES is unset: it names the JVM's oracle files (JSON lines), separated by whitespace");
    let paths: Vec<String> = paths.split_whitespace().map(str::to_owned).collect();
    assert!(!paths.is_empty(), "LITE_ORACLES names no file");
    paths
}

/// The test thread's stack: the renderer recurses with the tree, which nests thousands deep (`LITE_DIALECT_STACK_MB`
/// measures how deep).
fn on_a_big_stack<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> T {
    let stack_mb: usize = std::env::var("LITE_DIALECT_STACK_MB").ok().and_then(|v| v.parse().ok()).unwrap_or(256);
    std::thread::Builder::new().stack_size(stack_mb << 20).spawn(f).unwrap().join().unwrap()
}

/// One distinct oracle line: where it was first read, and the line.
struct Entry {
    at: String,
    line: String,
}

/// Each oracle line `keep` takes (by its text), in order: where it was read, and the line. Answers each oracle file's
/// count of lines taken.
fn each_line(keep: &dyn Fn(&str) -> bool, each: &mut dyn FnMut(String, String)) -> BTreeMap<String, usize> {
    let mut read = BTreeMap::new();
    for path in oracles() {
        let file = std::fs::File::open(&path).unwrap_or_else(|e| panic!("cannot open {path}: {e}"));
        let name = path.rsplit(['/', '\\']).next().unwrap_or(&path).to_owned();
        for (i, line) in BufReader::new(file).lines().enumerate() {
            let line = line.unwrap_or_else(|e| panic!("{path}:{}: {e}", i + 1));
            if keep(&line) {
                *read.entry(name.clone()).or_default() += 1;
                each(format!("{name}:{}", i + 1), line);
            }
        }
    }
    read
}

/// Each oracle line of `dialect`, once per distinct input (its op, its configuration and its MirJson, read from the
/// line's text); a repeated input's answer must be the first's.
fn distinct(dialect: &str) -> (Vec<Entry>, BTreeMap<String, usize>) {
    let mut seen: HashMap<u128, u128> = HashMap::new();
    let mut entries = Vec::new();
    let read = each_line(&of_dialect(dialect), &mut |at, line| {
        let k = input_key(&line);
        let answer = hash(&[raw_answer(&line)]);
        match seen.get(&k) {
            Some(first) => assert_eq!(*first, answer, "{at}: an input with two answers"),
            None => {
                seen.insert(k, answer);
                entries.push(Entry { at, line });
            }
        }
    });
    (entries, read)
}

/// The text of a line's MirJson, exactly as Java wrote it (between `"mir":` and `,"answer":`).
fn raw_mir(line: &str) -> &str {
    line.find(",\"mir\":")
        .and_then(|start| line.rfind(",\"answer\":").map(|end| &line[start + 7..end]))
        .unwrap_or_else(|| panic!("a line without a mir and an answer: {}", &line[..line.len().min(200)]))
}

fn raw_answer(line: &str) -> &str {
    &line[line.rfind(",\"answer\":").expect("a line with an answer")..]
}

/// 128 bits of the texts: two hashes, seeded apart.
fn hash(parts: &[&str]) -> u128 {
    let mut a = DefaultHasher::new();
    let mut b = DefaultHasher::new();
    0xA5u8.hash(&mut b);
    for p in parts {
        p.hash(&mut a);
        p.hash(&mut b);
    }
    (u128::from(a.finish()) << 64) | u128::from(b.finish())
}

/// A dialect's input: its op, its configuration (the first oracle's lines have none: null) and its MirJson, each the
/// text Java wrote (`{"dialect":..,"config":..,"fresh":..,"op":..,"kind":..,"mir":..,"answer":..}`).
fn input_key(line: &str) -> u128 {
    let field = |name: &str, end: &str| {
        let key = format!(",\"{name}\":");
        let start = line.find(&key)? + key.len();
        Some(&line[start..start + line[start..].find(end)?])
    };
    let op =
        field("op", ",\"kind\":").unwrap_or_else(|| panic!("a line without an op: {}", &line[..line.len().min(200)]));
    let config = match field("config", ",\"fresh\":").unwrap_or("null") {
        DEFAULT_ENGINE_CONFIG => "null",
        other => other,
    };
    hash(&[op, config, raw_mir(line)])
}

/// The dialect's lines: the dialect is the first field.
fn of_dialect(dialect: &str) -> impl Fn(&str) -> bool {
    let prefix = format!("{{\"dialect\":\"{dialect}\",");
    move |line: &str| line.starts_with(&prefix)
}

// ==================================================================
// The differential
// ==================================================================

fn renders_as_java_does(dialect: &'static str) {
    let floor = floor(dialect);
    let tally = on_a_big_stack(move || compare_all(dialect));
    let report = tally.report(dialect);
    eprintln!("{report}");
    if let Ok(dir) = std::env::var("LITE_DIALECT_MISMATCHES") {
        let out = format!("{dir}/{dialect}.txt");
        std::fs::write(&out, tally.mismatches.iter().map(|m| m.full.as_str()).collect::<Vec<_>>().join("\n"))
            .unwrap_or_else(|e| panic!("cannot write {out}: {e}"));
    }
    assert!(tally.statements > 0, "the oracles hold no {dialect} statement");
    assert!(
        tally.identical >= floor,
        "{} of {} {dialect} statements render as Java's, below the floor of {floor}",
        tally.identical,
        tally.statements
    );
}

/// The dialect's floor: its file among `LITE_DIALECT_FLOORS` (Bazel), else the crate's `tests/<dialect>-floor.txt`.
fn floor(dialect: &str) -> usize {
    let file = format!("{}-floor.txt", dialect.to_ascii_lowercase());
    let path = match std::env::var("LITE_DIALECT_FLOORS") {
        Ok(paths) => paths
            .split_whitespace()
            .find(|p| p.rsplit(['/', '\\']).next() == Some(file.as_str()))
            .unwrap_or_else(|| panic!("LITE_DIALECT_FLOORS names no {file}"))
            .to_owned(),
        Err(_) => format!("{}/tests/{file}", option_env!("CARGO_MANIFEST_DIR").unwrap_or("rust/lite-dialect")),
    };
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("cannot read the floor {path}: {e}"));
    text.lines()
        .map(str::trim)
        .find(|l| !l.is_empty() && !l.starts_with('#'))
        .and_then(|l| l.parse().ok())
        .unwrap_or_else(|| panic!("{path} holds no floor (a count of identical statements)"))
}

#[derive(Default)]
struct Tally {
    /// lines read, by oracle file
    read: BTreeMap<String, usize>,
    statements: usize,
    identical: usize,
    /// the identical statements that bind or have a region (their binds and regions Java's record text)
    statements_with_binds: usize,
    /// (op or kind) -> [identical, different, refused wrongly, accepted wrongly]
    by_op: BTreeMap<String, [usize; 4]>,
    by_kind: BTreeMap<String, [usize; 4]>,
    /// Java's refusals, by class
    refusals: BTreeMap<String, usize>,
    /// cause -> count
    causes: BTreeMap<&'static str, usize>,
    /// cause -> the first examples' short diffs
    examples: BTreeMap<&'static str, Vec<String>>,
    mismatches: Vec<Mismatch>,
}

struct Mismatch {
    short: String,
    full: String,
}

/// Java's answer, or Rust's.
#[derive(Debug, PartialEq)]
enum Answer {
    Text(String),
    /// `{"ok": null}`: an engine-text dialect's select list of a query that is no select
    Null,
    Statement {
        sql: String,
        binds: String,
        regions: String,
    },
    Throws {
        class: String,
        message: String,
    },
}

const IDENTICAL: usize = 0;
const DIFFERENT: usize = 1;
const REFUSED_WRONGLY: usize = 2;
const ACCEPTED_WRONGLY: usize = 3;

impl Tally {
    fn report(&self, dialect: &str) -> String {
        let mut out = String::new();
        let read: Vec<String> = self.read.iter().map(|(f, n)| format!("{n} of {f}")).collect();
        let _ = writeln!(
            out,
            "{dialect}: {} distinct statements (lines read: {}), {} identical, {} not (the SQL, the binds and the \
             regions compared exactly)",
            self.statements,
            read.join(", "),
            self.identical,
            self.statements - self.identical
        );
        let _ = writeln!(
            out,
            "of the identical statements, {} bind a parameter or have a region; Java refused {:?}",
            self.statements_with_binds, self.refusals
        );
        let table = |out: &mut String, title: &str, rows: &BTreeMap<String, [usize; 4]>| {
            let _ =
                writeln!(out, "\n{title:<24} {:>9} {:>9} {:>9} {:>9}", "identical", "different", "refused", "accepted");
            for (k, v) in rows {
                let _ = writeln!(out, "{k:<24} {:>9} {:>9} {:>9} {:>9}", v[0], v[1], v[2], v[3]);
            }
        };
        table(&mut out, "by op", &self.by_op);
        table(&mut out, "by MIR kind", &self.by_kind);
        let _ = writeln!(
            out,
            "\n(refused = Rust refused where Java rendered; accepted = Rust rendered where Java refused)"
        );
        let _ = writeln!(out, "\nmismatches by likely cause (the pass whose signature the first difference carries):");
        let mut causes: Vec<_> = self.causes.iter().collect();
        causes.sort_by(|a, b| b.1.cmp(a.1));
        for (cause, n) in causes {
            let _ = writeln!(out, "  {n:>6}  {cause}");
        }
        let _ = writeln!(out, "\nthe first {} mismatches:", SHOWN.min(self.mismatches.len()));
        for m in self.mismatches.iter().take(SHOWN) {
            let _ = writeln!(out, "{}", m.short);
        }
        let _ = writeln!(out, "\nan example of each cause:");
        for (cause, examples) in &self.examples {
            let _ = writeln!(out, "-- {cause}");
            for e in examples.iter().take(2) {
                let _ = writeln!(out, "{e}");
            }
        }
        out
    }
}

fn compare_all(dialect: &'static str) -> Tally {
    let (entries, read) = distinct(dialect);
    let mut tally = Tally { read, ..Tally::default() };
    for e in entries {
        compare(dialect, &e, &mut tally);
    }
    tally
}

fn compare(dialect: &str, e: &Entry, tally: &mut Tally) {
    let doc = parse_with_depth(&e.line, MAX_DEPTH).unwrap();
    let doc = doc.as_obj().unwrap();
    let at = &e.at;
    let op = doc.str("op").unwrap().to_owned();
    let kind = doc.str("kind").unwrap().to_owned();
    let config = doc.obj_or_null("config").unwrap();
    let mir = doc.get("mir").unwrap_or_else(|| panic!("{at}: no mir"));
    let java = java_answer(doc.obj("answer").unwrap_or_else(|e| panic!("{at}: {e}")));
    if let Answer::Throws { class, .. } = &java {
        *tally.refusals.entry(class.rsplit('.').next().unwrap_or(class).to_owned()).or_default() += 1;
    }
    let d = fresh(dialect, config);
    let (rust, query) =
        answer(d.as_ref(), &op, mir).unwrap_or_else(|e| panic!("{at}: {op} {kind}: unreadable MIR: {e}"));
    tally.statements += 1;
    let outcome = match (&java, &rust) {
        (Answer::Throws { .. }, Answer::Throws { .. })
        | (Answer::Text(_), Answer::Text(_))
        | (Answer::Null, Answer::Null)
        | (Answer::Statement { .. }, Answer::Statement { .. }) => {
            if java == rust {
                IDENTICAL
            } else {
                DIFFERENT
            }
        }
        (_, Answer::Throws { .. }) => REFUSED_WRONGLY,
        (Answer::Throws { .. }, _) => ACCEPTED_WRONGLY,
        _ => DIFFERENT,
    };
    tally.by_op.entry(op.clone()).or_default()[outcome] += 1;
    tally.by_kind.entry(kind.clone()).or_default()[outcome] += 1;
    if outcome == IDENTICAL {
        tally.identical += 1;
        if let Answer::Statement { binds, regions, .. } = &java
            && (binds != "[]" || regions != "[]")
        {
            tally.statements_with_binds += 1;
        }
        return;
    }
    let cause = cause(dialect, query.as_ref(), &java, &rust);
    *tally.causes.entry(cause).or_default() += 1;
    let short = format!("{at} {op} {kind} [{cause}]: {}", difference(&java, &rust, 100));
    let examples = tally.examples.entry(cause).or_default();
    if examples.len() < 2 {
        examples.push(short.clone());
    }
    let full = format!("{at} {op} {kind} [{cause}]\n  java: {java:?}\n  rust: {rust:?}");
    tally.mismatches.push(Mismatch { short, full });
}

fn java_answer(a: &Obj) -> Answer {
    if let Some(class) = a.get("throws") {
        return Answer::Throws {
            class: class.as_str().unwrap().to_owned(),
            message: a.str("message").unwrap().to_owned(),
        };
    }
    match a.get("ok").expect("an answer is ok or throws") {
        Node::Str(s) => Answer::Text(s.clone()),
        Node::Null => Answer::Null,
        Node::Obj(o) => Answer::Statement {
            sql: o.str("sql").unwrap().to_owned(),
            binds: o.str("binds").unwrap().to_owned(),
            regions: o.str("regions").unwrap().to_owned(),
        },
        other => panic!("an ok answer neither text, null nor a statement: {other:?}"),
    }
}

/// An engine-text dialect's surface, refused (a test's failure) on any other.
fn engine(d: &dyn SqlDialect) -> &dyn EngineTextDialect {
    d.engine_text().unwrap_or_else(|| panic!("{} is no engine-text dialect", d.dialect_name()))
}

/// Rust's answer for the op, and the query the passes would have seen (a DML statement's own, or its expressions in
/// a select; none for DDL and for text).
fn answer(d: &dyn SqlDialect, op: &str, mir: &Node) -> Result<(Answer, Option<SqlQuery>), lite_sql::Error> {
    let input = |what: &str| mir.as_str().map_err(|e| lite_sql::Error(format!("{what}'s input: {e}")));
    match op {
        "fromEngineText" => return Ok((text(d.from_engine_text(input(op)?)), None)),
        "engine.renderedAlias" => return Ok((Answer::Text(engine(d).rendered_alias(input(op)?)), None)),
        "engine.inTempTableSelect" => return Ok((Answer::Text(engine(d).in_temp_table_select(input(op)?)), None)),
        _ => {}
    }
    let mir = mir.as_obj()?;
    Ok(match op {
        "query.render" => {
            let q = read_query(mir)?;
            (text(d.render(q.clone())), Some(q))
        }
        "query.renderStatement" => {
            let q = read_query(mir)?;
            (statement(d.render_statement(q.clone())), Some(q))
        }
        "ddl.render" => (text(d.render_ddl(&read_ddl(mir)?)), None),
        "dml.render" => {
            let dml = read_dml(mir)?;
            (text(d.render_dml(&dml)), dml_query(&dml))
        }
        "dml.renderStatement" => {
            let dml = read_dml(mir)?;
            (statement(d.render_dml_statement(&dml)), dml_query(&dml))
        }
        "engine.renderAsAliased" => {
            let q = read_query(mir)?;
            (text(engine(d).render_as_aliased(q.clone())), Some(q))
        }
        "engine.selectList" => {
            let q = read_query(mir)?;
            let list = match engine(d).select_list(q.clone()) {
                Ok(Some(s)) => Answer::Text(s),
                Ok(None) => Answer::Null,
                Err(e) => Answer::Throws { class: e.class.to_owned(), message: e.message },
            };
            (list, Some(q))
        }
        "engine.collectionSplice" => {
            let param = read_expr(mir)?;
            let p = PlanParam::of(&param).unwrap_or_else(|| panic!("{op}'s input is no plan parameter"));
            (text(engine(d).collection_splice(&p)), None)
        }
        other => panic!("unknown op {other}"),
    })
}

/// What of a DML statement the passes see: its query, or its expressions as one select's.
fn dml_query(dml: &SqlDml) -> Option<SqlQuery> {
    let select = |exprs: Vec<SqlExpr>, r#where: Option<SqlExpr>| {
        SqlQuery::Select(Box::new(SqlSelect {
            projections: exprs.into_iter().map(|expr| Projection { expr, alias: None, out: None }).collect(),
            distinct: false,
            from: SqlSource::Dual,
            r#where,
            group_by: Vec::new(),
            having: None,
            qualify: None,
            order_by: Vec::new(),
            limit: None,
            offset: None,
            outputs: Vec::new(),
        }))
    };
    match dml {
        SqlDml::CreateTempTableAs { query, .. } | SqlDml::InsertSelect { query, .. } | SqlDml::Merge { query, .. } => {
            Some(query.clone())
        }
        SqlDml::Update { assignments, r#where, .. } => {
            Some(select(assignments.iter().map(|a| a.value.clone()).collect(), r#where.clone()))
        }
        SqlDml::DeleteWhere { r#where, .. } => Some(select(Vec::new(), r#where.clone())),
        SqlDml::InsertValues { rows, .. } => Some(select(rows.concat(), None)),
        SqlDml::InsertFromTable { .. } | SqlDml::DeleteAll { .. } => None,
    }
}

fn text(r: Result<String, Refusal>) -> Answer {
    match r {
        Ok(s) => Answer::Text(s),
        Err(e) => Answer::Throws { class: e.class.to_owned(), message: e.message },
    }
}

fn statement(r: Result<RenderedStatement, Refusal>) -> Answer {
    match r {
        Ok(s) => Answer::Statement { sql: s.sql, binds: list_text(&s.binds), regions: list_text(&s.regions) },
        Err(e) => Answer::Throws { class: e.class.to_owned(), message: e.message },
    }
}

/// An answer as one text: the SQL, or the refusal.
fn flat(a: &Answer) -> String {
    match a {
        Answer::Text(s) => s.clone(),
        Answer::Null => "NULL (no select)".to_owned(),
        Answer::Statement { sql, binds, regions } => format!("{sql} binds={binds} regions={regions}"),
        Answer::Throws { class, message } => format!("THROWS {class}: {message}"),
    }
}

/// The byte where two texts first differ.
fn first_difference(j: &str, r: &str) -> usize {
    let mut at = j.bytes().zip(r.bytes()).position(|(a, b)| a != b).unwrap_or(j.len().min(r.len()));
    while !j.is_char_boundary(at) || !r.is_char_boundary(at) {
        at -= 1;
    }
    at
}

/// Where two answers first differ, with the text around it in each.
fn difference(java: &Answer, rust: &Answer, width: usize) -> String {
    let (j, r) = (flat(java), flat(rust));
    let at = first_difference(&j, &r);
    format!("at byte {at}\n    java: ...{}...\n    rust: ...{}...", around(&j, at, width), around(&r, at, width))
}

fn around(s: &str, at: usize, width: usize) -> String {
    let mut from = at.saturating_sub(width / 2);
    while !s.is_char_boundary(from) {
        from -= 1;
    }
    let mut to = (at + width).min(s.len());
    while !s.is_char_boundary(to) {
        to += 1;
    }
    s[from..to].replace('\n', "\\n")
}

/// The likely cause of a mismatch: the pass whose signature the difference carries (the refusal Rust wrote in its
/// stead, or the text at the first difference), else the first pass, in the order the dialect runs its passes, whose
/// trigger the statement holds.
fn cause(dialect: &str, q: Option<&SqlQuery>, java: &Answer, rust: &Answer) -> &'static str {
    let mut t = Triggers::default();
    if let Some(q) = q {
        t.root(q);
        let _ = rewrite(&mut t, q.clone());
    }
    if let Answer::Throws { message, .. } = rust {
        if message.contains("fold reached a dialect without a fold encoding") {
            return "FoldToListReduce";
        }
        if message.contains("checked defects") || message.contains("checked child's value") {
            return "CheckedDefectsToLists";
        }
        if message.contains("collection membership") || message.contains("collection reduction") {
            return "CarrierStrategies";
        }
    }
    let (j, r) = (flat(java), flat(rust));
    let at = first_difference(&j, &r);
    let (before, java_rest, rust_rest) = (&j[..at], &j[at..], &r[at..]);
    let recent = &before[before.len().saturating_sub(300)..];
    let order: Vec<(bool, &'static str)> = if dialect == "DuckDb" {
        // SubstringClamp: a SUBSTRING start clamped to 1, a literal start replaced
        if java_rest.starts_with("greatest(")
            || (recent.contains("substr(") && rust_rest.starts_with(|c: char| c.is_ascii_digit() || c == '-'))
        {
            return if t.substring {
                "SubstringClamp"
            } else if t.float_canon {
                "FloatCanonText.Expansion, then SubstringClamp over the expansion's SUBSTRINGs"
            } else {
                "SubstringClamp (no SUBSTRING in the input)"
            };
        }
        if rust_rest.starts_with("QUANTILE_")
            || java_rest.starts_with("list_extract(list_")
            || java_rest.starts_with("-(list_extract")
        {
            return "QuantileOrder";
        }
        // a PIVOT's argument still qualified
        if before.contains("(PIVOT ")
            && rust_rest.split([' ', ')', ',', '(']).next().is_some_and(|tok| tok.contains('.'))
        {
            return "UnqualifyPivotArgs";
        }
        if (rust_rest.starts_with('*') || rust_rest.contains(".*"))
            && java_rest[..java_rest.len().min(300)].contains(" AS JSON)")
        {
            return "VariantEgress";
        }
        if java_rest.starts_with("CAST(") && t.stored_reads {
            return "StoredReads";
        }
        vec![
            (t.float_print, "FloatPrintText.Expansion (later passes see the expansion)"),
            (t.float_canon, "FloatCanonText.Expansion (later passes see the expansion)"),
            (t.carrier, "CarrierStrategies"),
            (t.quantile, "QuantileOrder"),
            (t.pivot, "UnqualifyPivotArgs"),
            (t.fold, "FoldToListReduce"),
            (t.checked_defects, "CheckedDefectsToLists"),
            (t.substring, "SubstringClamp"),
            (t.constant_key, "ConstantKeysAsExpressions"),
            (t.outer_join_subquery, "OuterJoinSubqueryFilters"),
            (t.variant_egress, "VariantEgress"),
            (t.stored_reads, "StoredReads"),
        ]
    } else if dialect == "Postgres" {
        // Postgres's passes (Postgres.passes()), in their order
        if java_rest.starts_with("CAST(") && rust_rest.starts_with("CAST(") && recent.ends_with("SELECT ") {
            return "RootNumericTypes";
        }
        vec![
            (t.float_print, "FloatPrintText.Expansion (later passes see the expansion)"),
            (t.float_canon, "FloatCanonText.Expansion (later passes see the expansion)"),
            (t.carrier || t.pivot, "CarrierStrategies"),
            (t.star_except, "StarExceptToColumns"),
            (t.ordered_set_window, "WholePartitionOrderedSets"),
            (t.substring, "SubstringClamp"),
            (t.string_count, "StringCountsAsInt"),
            (t.avg, "AggregatesDeliverDouble"),
            (t.qualify, "QualifyToSubselect (by outputs)"),
            (t.constant_key, "ConstantKeysAsExpressions"),
            (t.stored_reads, "StoredReads"),
        ]
    } else if dialect == "SQLite" {
        // SQLite's passes (SQLite.passes()), in their order
        vec![
            (t.float_print, "FloatPrintText.Expansion (later passes see the expansion)"),
            (t.float_canon, "FloatCanonText.Expansion, bound (later passes see the expansion)"),
            (t.carrier || t.pivot, "CarrierStrategies"),
            (t.star_except, "StarExceptToColumns"),
            (t.substring, "SubstringClamp"),
            (t.ordered_set_window, "WholePartitionOrderedSets"),
            (t.lateral, "ExplosionMarkers"),
            (t.qualify, "QualifyToSubselect (by outputs)"),
            (t.constant_key, "ConstantKeysAsExpressions"),
            (t.stored_reads, "StoredReads"),
        ]
    } else if dialect == "Trino" {
        // Trino's passes (Trino.passes()), in their order
        if java_rest.starts_with("CAST(") && rust_rest.starts_with("CAST(") && recent.ends_with("SELECT ") {
            return "RootNumericTypes";
        }
        vec![
            (t.float_print, "FloatPrintText.Expansion (later passes see the expansion)"),
            (t.float_canon, "FloatCanonText.Expansion (later passes see the expansion)"),
            (t.pivot, "PivotAsGroupBy"),
            (t.as_of, "AsOfAsPick"),
            (t.star_except, "StarExceptToColumns"),
            (t.lateral, "ExplosionMarkers"),
            (t.checked_defects, "CheckedDefectsToLists"),
            (t.substring, "SubstringClamp"),
            (t.avg, "AggregatesDeliverDouble"),
            (t.qualify, "QualifyToSubselect (by outputs)"),
            (t.constant_key, "ConstantKeysAsExpressions"),
        ]
    } else if dialect == "Databricks" {
        // Databricks' passes (Databricks.passes()), in their order
        if java_rest.starts_with("CAST(") && rust_rest.starts_with("CAST(") && recent.ends_with("SELECT ") {
            return "RootNumericTypes";
        }
        vec![
            (t.float_print, "FloatPrintText.Expansion (later passes see the expansion)"),
            (t.float_canon, "FloatCanonText.Expansion (later passes see the expansion)"),
            (t.pivot, "UnqualifyPivotArgs"),
            (t.checked_defects, "CheckedDefectsToLists"),
            (t.substring, "SubstringClamp"),
            (t.avg, "AggregatesDeliverDouble"),
            (t.constant_key, "ConstantKeysAsExpressions"),
        ]
    } else if dialect == "ClickHouse" {
        // ClickHouse's passes (ClickHouse.passes()), in their order
        if java_rest.starts_with("CAST(") && rust_rest.starts_with("CAST(") && recent.ends_with("SELECT ") {
            return "RootNumericTypes";
        }
        vec![
            (t.float_print, "FloatPrintText.Expansion (later passes see the expansion)"),
            (t.float_canon, "FloatCanonText.Expansion (later passes see the expansion)"),
            (t.pivot, "PivotAsGroupBy"),
            (t.outer_join, "OuterJoinResidualsAsMatches"),
            (t.lateral, "ExplosionMarkers, then ArrayJoinColumnsNamed"),
            (t.checked_defects, "CheckedDefectsToLists"),
            (t.substring, "SubstringClamp"),
            (t.avg, "AggregatesDeliverDouble"),
            (t.constant_key, "ConstantKeysAsExpressions"),
        ]
    } else if dialect.starts_with("EngineStyle") {
        // the engine text's passes (EngineStyleH2.passes()): the carrier strategies, the lateral explode
        vec![
            (t.float_print, "FloatPrintText.Expansion (later passes see the expansion)"),
            (t.float_canon, "FloatCanonText.Expansion (later passes see the expansion)"),
            (t.carrier || t.pivot, "CarrierStrategies"),
            (t.lateral, "LateralExplodeToUnion"),
        ]
    } else {
        // the H2 family's passes (H2.passes()): a cast to DOUBLE over AVG, a reference re-spelled by its source
        if java_rest.starts_with("CAST(AVG(") || rust_rest.starts_with("CAST(AVG(") {
            return "AggregatesDeliverDouble";
        }
        if recent.ends_with('.') && (java_rest.starts_with('"') != rust_rest.starts_with('"')) {
            return "SourceSpelling";
        }
        vec![
            (t.float_print, "FloatPrintText.Expansion (later passes see the expansion)"),
            (t.float_canon, "FloatCanonText.Expansion (later passes see the expansion)"),
            (t.carrier || t.pivot, "CarrierStrategies"),
            (t.lateral, "ConstantLateralsAsRows or LateralExplodeToUnion"),
            (t.avg, "AggregatesDeliverDouble"),
            (t.stored_reads, "StoredReads"),
        ]
    };
    if let Some((_, name)) = order.iter().find(|(hit, _)| *hit) {
        return name;
    }
    match rust {
        Answer::Throws { .. } => "unexplained refusal",
        _ => "unexplained text difference",
    }
}

/// The passes' triggers a statement holds, found by the shared walk.
#[derive(Default)]
struct Triggers {
    float_print: bool,
    float_canon: bool,
    carrier: bool,
    quantile: bool,
    pivot: bool,
    fold: bool,
    checked_defects: bool,
    substring: bool,
    constant_key: bool,
    outer_join_subquery: bool,
    variant_egress: bool,
    stored_reads: bool,
    lateral: bool,
    avg: bool,
    star_except: bool,
    ordered_set_window: bool,
    string_count: bool,
    qualify: bool,
    as_of: bool,
    outer_join: bool,
}

impl Triggers {
    /// The root-only triggers: a star (or an empty projection list) at a root select over a stored Variant column.
    fn root(&mut self, q: &SqlQuery) {
        match q {
            SqlQuery::Select(s) => self.root_select(s),
            SqlQuery::Union(u) => u.branches.iter().for_each(|b| self.root(b)),
            SqlQuery::With(w) => self.root(&w.body),
        }
    }

    fn root_select(&mut self, s: &SqlSelect) {
        let starred = s.projections.is_empty()
            || s.projections.iter().any(|p| matches!(p.expr, SqlExpr::Star { .. } | SqlExpr::StarExcept { .. }));
        if starred && holds_variant(&s.from) {
            self.variant_egress = true;
        }
    }
}

fn holds_variant(s: &SqlSource) -> bool {
    let variant = |outputs: &[lite_sql::OutputCol]| {
        outputs.iter().any(|o| {
            matches!(o.origin, Origin::Physical | Origin::PhysicalQuoted) && o.r#type == SqlType::Scalar(Scalar::Json)
        })
    };
    match s {
        SqlSource::Table { outputs, .. } => variant(outputs),
        SqlSource::Join { left, right, .. } => holds_variant(left) || holds_variant(right),
        SqlSource::Subselect { inner, .. } => match inner.as_ref() {
            SqlQuery::Select(s) => variant(&s.outputs),
            _ => false,
        },
        _ => false,
    }
}

fn holds_scalar_subquery(e: &SqlExpr) -> bool {
    struct Find(bool);
    impl SqlRewriter for Find {
        fn name(&self) -> &'static str {
            "find"
        }
        fn expr(&mut self, e: SqlExpr) -> Result<Rw<SqlExpr>, Refusal> {
            self.0 |= matches!(e, SqlExpr::ScalarSubquery { .. });
            Ok(Rw::Same(e))
        }
    }
    let mut f = Find(false);
    let _ = lite_dialect::rewriter::rewrite_expr(&mut f, e.clone());
    f.0
}

impl SqlRewriter for Triggers {
    fn name(&self) -> &'static str {
        "triggers"
    }

    fn select(&mut self, s: SqlSelect) -> Result<Rw<SqlQuery>, Refusal> {
        if s.group_by.iter().any(|k| matches!(k, SqlExpr::IntLit { .. })) {
            self.constant_key = true;
        }
        if s.qualify.is_some() {
            self.qualify = true;
        }
        Ok(Rw::Same(SqlQuery::Select(Box::new(s))))
    }

    fn source(&mut self, s: SqlSource) -> Result<Rw<SqlSource>, Refusal> {
        match &s {
            SqlSource::Pivot { .. } => self.pivot = true,
            SqlSource::Join { kind: JoinKind::AsofLeft, .. } => self.as_of = true,
            SqlSource::Join { kind: JoinKind::Left, on: Some(on), .. } if holds_scalar_subquery(on) => {
                self.outer_join_subquery = true;
            }
            SqlSource::Join { kind: JoinKind::Left, on: Some(_), .. } => self.outer_join = true,
            SqlSource::Join { kind: JoinKind::LeftLateral | JoinKind::CrossLateral, .. } => self.lateral = true,
            SqlSource::Table { stored_types, .. }
                if stored_types.iter().any(|(_, t)| {
                    matches!(t, ColumnType::Plain { kind: ColumnKind::Other | ColumnKind::Distinct })
                }) =>
            {
                self.stored_reads = true;
            }
            _ => {}
        }
        Ok(Rw::Same(s))
    }

    fn expr(&mut self, e: SqlExpr) -> Result<Rw<SqlExpr>, Refusal> {
        match &e {
            SqlExpr::Call { r#fn: SqlFn::FloatPrint, .. } => self.float_print = true,
            SqlExpr::Call { r#fn: SqlFn::FloatCanon, .. } => self.float_canon = true,
            SqlExpr::Call { r#fn: SqlFn::Substring, .. } => self.substring = true,
            SqlExpr::Membership { .. }
            | SqlExpr::ReduceCollection { .. }
            | SqlExpr::CompactList { .. }
            | SqlExpr::CheckedOne { .. }
            | SqlExpr::Call { r#fn: SqlFn::Unnest | SqlFn::ListFilter, .. } => self.carrier = true,
            SqlExpr::Reducer(r) if matches!(r.r#fn, AggFn::QuantileCont | AggFn::QuantileDisc) => self.quantile = true,
            SqlExpr::Reducer(r) if r.r#fn == AggFn::Avg => self.avg = true,
            SqlExpr::WindowCall { r#fn, .. }
                if matches!(r#fn.as_ref(), lite_sql::SqlAgg::Reducer(r)
                    if matches!(r.r#fn, AggFn::QuantileCont | AggFn::QuantileDisc)) =>
            {
                self.quantile = true;
            }
            SqlExpr::FoldCall { .. } => self.fold = true,
            SqlExpr::CheckedDefects { .. } | SqlExpr::CheckedChildValue { .. } => self.checked_defects = true,
            SqlExpr::StarExcept { .. } => self.star_except = true,
            SqlExpr::Call {
                r#fn: SqlFn::Left | SqlFn::Right | SqlFn::RepeatStr | SqlFn::Lpad | SqlFn::Rpad | SqlFn::Chr,
                ..
            } => self.string_count = true,
            _ => {}
        }
        if let SqlExpr::WindowCall { r#fn, .. } = &e
            && matches!(r#fn.as_ref(), lite_sql::SqlAgg::Reducer(r)
                if matches!(r.r#fn, AggFn::Median | AggFn::Mode | AggFn::QuantileCont | AggFn::QuantileDisc))
        {
            self.ordered_set_window = true;
        }
        let _ = TypeFact::Unknown;
        Ok(Rw::Same(e))
    }
}

// ==================================================================
// The seam held to the renderer it dispatches to
// ==================================================================

/// The seam (`lite_dialect::dispatch`, the path `lite-ffi`'s `lite_dialect` takes) answering every op of the
/// dialect's statements exactly as a fresh dialect called directly answers it: the same JSON text (the SQL as a JSON
/// string, a statement in `SEAM.md`'s form), a statement read back from its JSON equal to the direct one (record for
/// record) and its binds and regions Java's record text, a refusal the same class and message. Each `query.render`
/// line is also asked for `query.renderInline`, and an engine-text dialect's for each of its aliases after the render
/// (`engine.renderedAlias` with the rendered query).
fn the_seam_answers_as_the_renderer_does(dialect: &'static str) {
    let tally = on_a_big_stack(move || seam_all(dialect));
    let mut report = String::new();
    let _ = writeln!(
        report,
        "seam: {} {dialect} ops (and {} inline renders, {} aliases after a render) dispatched, {} answered as the \
         renderer does, {} statements read back from JSON equal and Java's record text, {} refusals",
        tally.ops, tally.inline, tally.aliases, tally.same, tally.statements, tally.refusals
    );
    for (op, n) in &tally.by_op {
        let _ = writeln!(report, "  {op:<24} {n:>6}");
    }
    for d in tally.different.iter().take(SHOWN) {
        let _ = writeln!(report, "{d}");
    }
    eprintln!("{report}");
    assert!(tally.ops > 0, "the oracles hold no {dialect} statement");
    assert!(tally.different.is_empty(), "{} answers differ:\n{report}", tally.different.len());
    assert_eq!(tally.same, tally.ops + tally.inline + tally.aliases);
}

#[derive(Default)]
struct SeamTally {
    ops: usize,
    inline: usize,
    /// engine-text aliases asked after a render
    aliases: usize,
    same: usize,
    statements: usize,
    refusals: usize,
    by_op: BTreeMap<String, usize>,
    different: Vec<String>,
}

fn seam_all(dialect: &str) -> SeamTally {
    let (entries, _) = distinct(dialect);
    let mut tally = SeamTally::default();
    for e in entries {
        let doc = parse_with_depth(&e.line, MAX_DEPTH).unwrap();
        let doc = doc.as_obj().unwrap();
        let op = doc.str("op").unwrap();
        let config = doc.obj_or_null("config").unwrap();
        let mir = doc.get("mir").unwrap();
        let java = java_answer(doc.obj("answer").unwrap());
        tally.ops += 1;
        *tally.by_op.entry(op.to_owned()).or_default() += 1;
        let spec = spec(dialect, config);
        let raw = raw_mir(&e.line);
        seam_one(&e.at, &spec, op, (mir, raw), Some(&java), &mut tally);
        if op == "query.render" {
            tally.inline += 1;
            seam_one(&e.at, &spec, "query.renderInline", (mir, raw), None, &mut tally);
            if dialect.starts_with("EngineStyle") {
                rendered_aliases(&e.at, &spec, (mir, raw), &mut tally);
            }
        }
    }
    tally
}

/// An engine-text dialect's aliases after a render: each source alias of the query asked through the seam's
/// `engine.renderedAlias` with the query it rendered, held to Java's order of calls on one dialect object -- `render`,
/// then `renderedAlias` (the oracle's dialects are fresh, so the oracle holds no alias a render planned).
fn rendered_aliases(at: &str, spec: &str, input: (&Node, &str), tally: &mut SeamTally) {
    let (mir, raw) = input;
    let q = read_query(mir.as_obj().unwrap()).unwrap();
    let mut aliases = Vec::new();
    query_aliases(&q, &mut aliases);
    let d = open(spec).unwrap();
    if d.render(q).is_err() {
        return;
    }
    for alias in aliases {
        tally.aliases += 1;
        let args = format!("{{\"alias\":{},\"rendered\":{raw}}}", to_compact(&Node::Str(alias.clone())));
        let seam = dispatch(spec, "engine.renderedAlias", &args);
        let direct = engine(d.as_ref()).rendered_alias(&alias);
        match seam {
            Ok(json) if json == to_compact(&Node::Str(direct.clone())) => tally.same += 1,
            other => {
                tally.different.push(format!("{at} engine.renderedAlias {alias}: seam {other:?}, direct {direct}"))
            }
        }
    }
}

/// Every source alias of a query, its subqueries' too.
fn query_aliases(q: &SqlQuery, out: &mut Vec<String>) {
    match q {
        SqlQuery::Select(s) => source_aliases(&s.from, out),
        SqlQuery::Union(u) => u.branches.iter().for_each(|b| query_aliases(b, out)),
        SqlQuery::With(w) => {
            w.ctes.iter().for_each(|c| query_aliases(&c.query, out));
            query_aliases(&w.body, out);
        }
    }
}

fn source_aliases(s: &SqlSource, out: &mut Vec<String>) {
    match s {
        SqlSource::Join { left, right, .. } => {
            source_aliases(left, out);
            source_aliases(right, out);
        }
        SqlSource::Subselect { inner, alias, .. } => {
            out.push(alias.clone());
            query_aliases(inner, out);
        }
        SqlSource::Table { alias, .. } | SqlSource::VarSetPlaceholder { alias, .. } | SqlSource::Cte { alias, .. } => {
            out.push(alias.clone())
        }
        _ => {}
    }
}

/// The seam's argument key of an op's input.
fn input_key_of(op: &str) -> &'static str {
    match op {
        "fromEngineText" => "statement",
        "engine.collectionSplice" => "param",
        "engine.renderedAlias" => "alias",
        "engine.inTempTableSelect" => "table",
        "query.render"
        | "query.renderInline"
        | "query.renderStatement"
        | "engine.renderAsAliased"
        | "engine.selectList" => "query",
        "ddl.render" => "ddl",
        "dml.render" | "dml.renderStatement" => "dml",
        other => panic!("unknown op {other}"),
    }
}

/// One op through the seam and directly, its input the MirJson (read, and as the text Java wrote); `java` the JVM's
/// answer, to hold a statement's record text to.
fn seam_one(at: &str, spec: &str, op: &str, input: (&Node, &str), java: Option<&Answer>, tally: &mut SeamTally) {
    let (mir, raw) = input;
    let seam = dispatch(spec, op, &format!("{{\"{}\":{raw}}}", input_key_of(op)));
    let d = open(spec).unwrap();
    let obj = || mir.as_obj().unwrap();
    let direct: Result<Direct, Refusal> = match op {
        "query.render" => d.render(read_query(obj()).unwrap()).map(Direct::Text),
        "query.renderInline" => d.render_inline(read_query(obj()).unwrap()).map(Direct::Text),
        "query.renderStatement" => d.render_statement(read_query(obj()).unwrap()).map(Direct::Statement),
        "ddl.render" => d.render_ddl(&read_ddl(obj()).unwrap()).map(Direct::Text),
        "dml.render" => d.render_dml(&read_dml(obj()).unwrap()).map(Direct::Text),
        "dml.renderStatement" => d.render_dml_statement(&read_dml(obj()).unwrap()).map(Direct::Statement),
        "fromEngineText" => d.from_engine_text(mir.as_str().unwrap()).map(Direct::Text),
        "engine.renderAsAliased" => engine(d.as_ref()).render_as_aliased(read_query(obj()).unwrap()).map(Direct::Text),
        "engine.selectList" => {
            engine(d.as_ref()).select_list(read_query(obj()).unwrap()).map(|s| s.map_or(Direct::Null, Direct::Text))
        }
        "engine.collectionSplice" => {
            let param = read_expr(obj()).unwrap();
            engine(d.as_ref()).collection_splice(&PlanParam::of(&param).unwrap()).map(Direct::Text)
        }
        "engine.renderedAlias" => Ok(Direct::Text(engine(d.as_ref()).rendered_alias(mir.as_str().unwrap()))),
        "engine.inTempTableSelect" => Ok(Direct::Text(engine(d.as_ref()).in_temp_table_select(mir.as_str().unwrap()))),
        other => panic!("unknown op {other}"),
    };
    let differs = match (&seam, &direct) {
        (Err(s), Err(r)) => {
            tally.refusals += 1;
            (s.class != r.class || s.message != r.message).then(|| format!("refused {s} / {r}"))
        }
        (Ok(json), Ok(Direct::Text(sql))) => {
            (*json != to_compact(&Node::Str(sql.clone()))).then(|| format!("seam {json}\n    direct {sql:?}"))
        }
        (Ok(json), Ok(Direct::Null)) => (json != "null").then(|| format!("seam {json}\n    direct null")),
        (Ok(json), Ok(Direct::Statement(st))) => {
            let doc = parse_with_depth(json, MAX_DEPTH).unwrap_or_else(|e| panic!("{at}: the seam's JSON: {e}"));
            let back = read_statement(doc.as_obj().unwrap()).unwrap_or_else(|r| panic!("{at}: {r}"));
            let javas = match java {
                Some(Answer::Statement { sql, binds, regions }) => {
                    *sql == back.sql && *binds == list_text(&back.binds) && *regions == list_text(&back.regions)
                }
                _ => true,
            };
            if *json == to_compact(&statement_json(st)) && back == *st && javas {
                tally.statements += 1;
                None
            } else {
                Some(format!("seam {json}\n    direct {st:?}\n    java {java:?}"))
            }
        }
        (s, r) => Some(format!("seam {s:?}\n    direct {:?}", r.as_ref().map(|_| "an answer"))),
    };
    match differs {
        None => tally.same += 1,
        Some(d) => tally.different.push(format!("{at} {op}: {d}")),
    }
}

/// The direct renderer's answer.
enum Direct {
    Text(String),
    /// an engine-text dialect's select list of a query that is no select
    Null,
    Statement(RenderedStatement),
}

// ==================================================================
// The typing rules held to Java's own facts
// ==================================================================

/// Java's `SqlTyping` as `lite_dialect::typing` ports it, held to the facts Java's constructors stored: every node of
/// every distinct statement of the oracles (every dialect's: one tree), its fact recomputed from its children's and
/// compared with the one MirJson carries; a projection frame's outputs rebuilt from its projections, a union's labels
/// reconciled, a join's delivered columns, each compared with Java's. A leaf the builder supplies (a column, a lambda,
/// a parameter, a key cell) has no rule; an array literal or a field read whose children prove no type keeps the one
/// it was given.
#[test]
#[ignore = "reads the JVM's oracles named by LITE_ORACLES"]
fn typing_reproduces_javas_facts() {
    let facts = on_a_big_stack(check_all_facts);
    let mut report = String::new();
    let _ = writeln!(report, "typing: {} distinct statements, {} facts checked", facts.statements, facts.checked);
    for (kind, [checked, wrong]) in &facts.by_kind {
        let _ = writeln!(report, "  {kind:<28} {checked:>9} checked {wrong:>6} different");
    }
    for e in facts.examples.iter().take(SHOWN) {
        let _ = writeln!(report, "{e}");
    }
    eprintln!("{report}");
    let wrong: usize = facts.by_kind.values().map(|v| v[1]).sum();
    assert!(facts.checked > 0, "the oracles hold no statement");
    assert_eq!(wrong, 0, "{wrong} facts differ from Java's");
}

#[derive(Default)]
struct Facts {
    statements: usize,
    checked: usize,
    /// node kind -> [checked, different]
    by_kind: BTreeMap<String, [usize; 2]>,
    examples: Vec<String>,
}

impl Facts {
    fn check<T: PartialEq + std::fmt::Debug>(&mut self, at: &str, kind: &str, computed: T, stored: &T) {
        self.checked += 1;
        let tally = self.by_kind.entry(kind.to_owned()).or_default();
        tally[0] += 1;
        if computed != *stored {
            tally[1] += 1;
            if self.examples.len() < SHOWN {
                self.examples.push(format!("{at} {kind}:\n    java: {stored:?}\n    rust: {computed:?}"));
            }
        }
    }
}

/// The ops whose MirJson is a query or a DML statement (an engine-text op's query too).
fn holds_a_query(op: &str) -> bool {
    op.starts_with("query.") || op.starts_with("dml.") || op == "engine.renderAsAliased" || op == "engine.selectList"
}

fn check_all_facts() -> Facts {
    let keep = |line: &str| {
        let op = line.find(",\"op\":\"").map(|at| &line[at + 7..]).and_then(|rest| rest.split('"').next());
        op.is_some_and(holds_a_query)
    };
    let mut facts = Facts::default();
    // one tree whichever dialect rendered it: distinct by the MirJson alone
    let mut seen = HashSet::new();
    each_line(&keep, &mut |at, line| {
        if !seen.insert(hash(&[raw_mir(&line)])) {
            return;
        }
        let doc = parse_with_depth(&line, MAX_DEPTH).unwrap_or_else(|e| panic!("{at}: unparsable: {e}"));
        let doc = doc.as_obj().unwrap();
        let mir = doc.obj("mir").unwrap();
        let query = if doc.str("op").unwrap().starts_with("dml.") {
            read_dml(mir).ok().and_then(|d| dml_query(&d))
        } else {
            read_query(mir).ok()
        };
        if let Some(q) = query {
            facts.statements += 1;
            query_facts(&at, &q, &mut facts);
        }
    });
    facts
}

fn query_facts(at: &str, q: &SqlQuery, f: &mut Facts) {
    use lite_dialect::typing;
    match q {
        SqlQuery::Select(s) => {
            for p in &s.projections {
                if let Some(out) = &p.out {
                    let normalized = matches!(out.origin, Origin::Derived | Origin::PerKey);
                    f.check(at, "Projection.out origin", normalized, &true);
                }
                expr_facts(at, &p.expr, f);
            }
            source_facts(at, &s.from, f);
            s.r#where.iter().chain(&s.group_by).chain(&s.having).chain(&s.qualify).for_each(|e| expr_facts(at, e, f));
            s.order_by.iter().for_each(|k| expr_facts(at, &k.expr, f));
            if !s.projections.is_empty() {
                f.check(at, "SqlSelect.outputs", typing::outputs_from(s).ok(), &Some(s.outputs.clone()));
            } else if let (SqlSource::Join { .. }, Some(w)) = (&s.from, &s.r#where)
                && !s.outputs.is_empty()
            {
                let neutralized = typing::where_pad_neutralized(&s.from, w, s.outputs.clone());
                f.check(at, "SqlSelect.outputs (star)", neutralized, &s.outputs);
            }
        }
        SqlQuery::Union(u) => {
            u.branches.iter().for_each(|b| query_facts(at, b, f));
            let reconciled = typing::reconcile_union_labels(&u.branches, u.outputs.clone());
            f.check(at, "SqlUnion.outputs", reconciled, &u.outputs);
        }
        SqlQuery::With(w) => {
            w.ctes.iter().for_each(|c| query_facts(at, &c.query, f));
            query_facts(at, &w.body, f);
        }
    }
}

fn source_facts(at: &str, s: &SqlSource, f: &mut Facts) {
    use lite_dialect::typing;
    match s {
        SqlSource::Subselect { inner, .. } => query_facts(at, inner, f),
        SqlSource::Values { rows, .. } => rows.iter().flatten().for_each(|e| expr_facts(at, e, f)),
        SqlSource::Join { left, right, kind, on, outputs } => {
            source_facts(at, left, f);
            source_facts(at, right, f);
            on.iter().for_each(|e| expr_facts(at, e, f));
            f.check(at, "Join.outputs", typing::join_delivered(left, right, *kind), outputs);
        }
        SqlSource::Pivot { source, on, r#in, named, usings, .. } => {
            source_facts(at, source, f);
            on.iter().chain(r#in).chain(named).for_each(|e| expr_facts(at, e, f));
            usings.iter().for_each(|u| expr_facts(at, &SqlExpr::Reducer(u.agg.clone()), f));
        }
        _ => {}
    }
}

fn expr_facts(at: &str, e: &SqlExpr, f: &mut Facts) {
    use lite_dialect::typing::{self, scalar};
    // the subqueries a node holds, and a window function's arguments, which `children` leaves out
    match e {
        SqlExpr::Exists { subquery, .. }
        | SqlExpr::ScalarSubquery { subquery, .. }
        | SqlExpr::InSubquery { subquery, .. }
        | SqlExpr::Quantified { subquery, .. } => query_facts(at, subquery, f),
        SqlExpr::DeferredTdsString { inner, .. } => query_facts(at, &SqlQuery::Select(inner.clone()), f),
        SqlExpr::WindowCall { r#fn, .. } => match r#fn.as_ref() {
            lite_sql::SqlAgg::RankingFn { args, .. } | lite_sql::SqlAgg::ValueFn { args, .. } => {
                args.iter().for_each(|a| expr_facts(at, a, f))
            }
            lite_sql::SqlAgg::Reducer(_) => {}
        },
        _ => {}
    }
    for c in typing::children(e) {
        expr_facts(at, &c, f);
    }
    let computed = match e {
        SqlExpr::Call { r#fn, args, .. } => typing::call_type(*r#fn, args),
        SqlExpr::Case { whens, otherwise, .. } => typing::case_type(whens, otherwise.as_deref()),
        SqlExpr::Cast { value, target, .. } => typing::cast_type(value, target),
        SqlExpr::Membership { needle, collection, .. } => typing::membership_type(needle, collection),
        SqlExpr::ReduceCollection { reducer, collection, .. } => typing::reduce_collection_type(*reducer, collection),
        SqlExpr::Group { inner, .. } | SqlExpr::CompactList { list: inner, .. } => inner.r#type().clone(),
        SqlExpr::OrderedListAgg { value, .. } => typing::reducer_type(AggFn::List, value.r#type()),
        SqlExpr::ArrayLit { elements, .. } => match typing::array_lit_type(elements) {
            TypeFact::Unknown => return,
            proven => proven,
        },
        SqlExpr::StructLit { fields, .. } => typing::struct_lit_type(fields),
        SqlExpr::StructGet { source, field, .. } => match typing::struct_get_type(source, field) {
            TypeFact::Unknown => return,
            proven => proven,
        },
        SqlExpr::ScalarSubquery { subquery, .. } => typing::scalar_subquery_type(subquery),
        SqlExpr::CheckedOne { list, at_least_only, .. } => {
            if *at_least_only {
                list.r#type().clone()
            } else {
                typing::checked_one_type(list)
            }
        }
        SqlExpr::WindowCall { r#fn, .. } => typing::window_type(r#fn),
        SqlExpr::FoldCall { source, lambda, init, acc_is_list, .. } => {
            typing::fold_type(source, lambda, init, *acc_is_list)
        }
        SqlExpr::Reducer(r) => typing::reducer_fact(r),
        SqlExpr::StoredRead { column, .. } => column.r#type.clone(),
        SqlExpr::StringLit { .. }
        | SqlExpr::JsonKindLit { .. }
        | SqlExpr::TypeNameLit { .. }
        | SqlExpr::DeferredTdsString { .. } => scalar(Scalar::Varchar),
        SqlExpr::IntLit { .. } | SqlExpr::RowOrder { .. } => scalar(Scalar::Bigint),
        SqlExpr::FloatLit { value, .. } => typing::float_lit_type(value.text()),
        SqlExpr::DecimalLit { value, .. } => typing::decimal_lit_type(value),
        SqlExpr::BoolLit { .. } | SqlExpr::Exists { .. } | SqlExpr::InSubquery { .. } | SqlExpr::Quantified { .. } => {
            scalar(Scalar::Boolean)
        }
        SqlExpr::NullLit { .. } => TypeFact::Bottom,
        SqlExpr::DateLit { .. } => scalar(Scalar::Date),
        SqlExpr::TimestampLit { .. } => scalar(Scalar::Timestamp),
        SqlExpr::TimeLit { .. } => scalar(Scalar::Time),
        SqlExpr::JsonObject { .. } | SqlExpr::JsonArray { .. } | SqlExpr::JsonArrayAgg { .. } => scalar(Scalar::Json),
        SqlExpr::CheckedDefects { .. }
        | SqlExpr::CheckedChildValue { .. }
        | SqlExpr::Star { .. }
        | SqlExpr::StarExcept { .. }
        | SqlExpr::FormatLit { .. }
        | SqlExpr::DatePartLit { .. }
        | SqlExpr::TempTableInSplice { .. } => TypeFact::Unknown,
        // the leaves the builder supplies
        SqlExpr::Column(_) | SqlExpr::Lambda(_) | SqlExpr::PlanParam { .. } | SqlExpr::KeyCell { .. } => return,
    };
    let kind = match e {
        SqlExpr::Call { r#fn, .. } => format!("Call {fn}", fn = r#fn),
        other => lite_dialect::java::simple_name(other).to_owned(),
    };
    f.check(at, &kind, computed, e.r#type());
}

// ==================================================================
// java.time over the JDK's tzdb, held to Java's own answers
// ==================================================================

/// The engine text's zone shift held to the JDK (`LITE_ZONE_ORACLES`: `rust/oracles/tzdb`'s `zone-golden.tsv` and
/// `engine-text-zones.tsv`, its MANIFEST.tsv): every `O` line an instant's offset, standard offset and local date-time
/// in a zone (and, for a year of four digits, `timestampLit`'s shift of its UTC text there), every `L` line a local
/// date-time's valid offsets, transition and `ZonedDateTime.of` resolution, each as `lite_dialect::java_time` answers
/// it; and every line of Java's own `EngineStyleH2(false, zone)` render of a date-time constant, rendered by the seam's
/// dialect of that configuration. All exactly.
#[test]
#[ignore = "reads the JVM's zone oracles named by LITE_ZONE_ORACLES"]
fn the_zone_shift_answers_as_java_does() {
    let paths = std::env::var("LITE_ZONE_ORACLES")
        .expect("LITE_ZONE_ORACLES is unset: it names zone-golden.tsv and engine-text-zones.tsv");
    let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
    let mut different = Vec::new();
    for path in paths.split_whitespace() {
        let text = std::fs::read_to_string(path).unwrap_or_else(|e| panic!("cannot read {path}: {e}"));
        let engine = path.ends_with("engine-text-zones.tsv");
        for (n, line) in text.lines().enumerate().filter(|(_, l)| !l.starts_with('#')) {
            let fields: Vec<&str> = line.split('\t').collect();
            let at = format!("{}:{}", path.rsplit(['/', '\\']).next().unwrap_or(path), n + 1);
            let differs = if engine {
                *counts.entry("engine-text renders").or_default() += 1;
                engine_text_zone(&fields)
            } else if fields[0] == "O" {
                *counts.entry("O, instants").or_default() += 1;
                let (o, shifted) = zone_instant(&fields);
                *counts.entry("O, timestampLit shifts").or_default() += usize::from(shifted);
                o
            } else {
                *counts.entry("L, local date-times").or_default() += 1;
                zone_local(&fields)
            };
            if let Some(d) = differs {
                different.push(format!("{at}: {line}\n    rust: {d}"));
            }
        }
    }
    let data = lite_dialect::java_time::tzdb();
    eprintln!(
        "zones: java {}, tzdb {}, {} regions, {} rule sets; {counts:?}; {} different",
        data.java,
        data.version,
        data.region_ids().count(),
        data.rule_sets(),
        different.len()
    );
    for d in different.iter().take(SHOWN) {
        eprintln!("{d}");
    }
    assert!(counts.values().sum::<usize>() > 0, "the zone oracles hold no line");
    assert!(different.is_empty(), "{} zone answers differ from Java's", different.len());
}

/// The embedded tzdb is the build JDK's (`LITE_TZDB_GENERATED`: `//rust/oracles/tzdb:tzdb_data`'s `out/tzdb.bin`,
/// written on the build's JDK by every build): the committed `rust/oracles/tzdb/tzdb.bin` the dialect library embeds
/// is that output byte for byte, so a JDK bump whose tzdb changes the rules fails here until the data is regenerated.
#[test]
#[ignore = "reads the tzdb the build's JDK writes, named by LITE_TZDB_GENERATED"]
fn the_embedded_tzdb_is_the_build_jdks() {
    let path = std::env::var("LITE_TZDB_GENERATED")
        .expect("LITE_TZDB_GENERATED is unset: it names //rust/oracles/tzdb:tzdb_data's out/tzdb.bin");
    let generated = std::fs::read(&path).unwrap_or_else(|e| panic!("cannot read {path}: {e}"));
    let embedded = lite_dialect::java_time::tzdb_bytes();
    let data = lite_dialect::java_time::tzdb();
    assert!(
        generated == embedded,
        "the embedded tzdb (rust/oracles/tzdb/tzdb.bin: java {}, tzdb {}, {} bytes) is not what the build's JDK writes \
         ({path}, {} bytes): the JDK's time-zone rules changed. Regenerate the data and its oracles \
         (rust/oracles/tzdb/BUILD.bazel): bazel build //rust/oracles/tzdb:tzdb_data; cp \
         bazel-bin/rust/oracles/tzdb/out/tzdb.bin rust/oracles/tzdb/tzdb.bin; zstd -19 -f \
         bazel-bin/rust/oracles/tzdb/out/zone-golden.tsv -o rust/oracles/tzdb/zone-golden.tsv.zst; rerun \
         EngineTextZones.java as its comment says; update rust/oracles/tzdb/MANIFEST.tsv's versions and SHA-256s, and \
         the counts java_time.rs's tests pin",
        data.java,
        data.version,
        embedded.len(),
        generated.len()
    );
}

/// `O <zone> <epoch second> <offset> <standard offset> <local date-time>`: what differs, if anything, and whether the
/// shift of the instant's UTC text was checked too.
fn zone_instant(f: &[&str]) -> (Option<String>, bool) {
    use lite_dialect::java_time::{LocalDateTime, shift_to_zone, zone_of};
    let zone = zone_of(f[1]);
    let epoch: i64 = f[2].parse().unwrap();
    let offset = zone.offset_at(epoch);
    let standard = zone.standard_offset_at(epoch);
    let local = offset.and_then(|o| LocalDateTime::at_offset(epoch, o)).map(|l| l.to_java_string());
    let java = (Some(f[3].parse::<i32>().unwrap()), Some(f[4].parse::<i32>().unwrap()), Some(f[5].to_owned()));
    if (offset, standard, local.clone()) != java {
        return (Some(format!("{offset:?} {standard:?} {local:?}")), false);
    }
    // timestampLit's shift of the instant's UTC text (a year of four digits: the text the engine writes)
    let utc = LocalDateTime::of_epoch_second(epoch).unwrap();
    if !(0..=9999).contains(&utc.year) {
        return (None, false);
    }
    let iso =
        format!("{:04}-{:02}-{:02} {:02}:{:02}:{:02}", utc.year, utc.month, utc.day, utc.hour, utc.minute, utc.second);
    let mut expected = f[5].replace('T', " ");
    if expected.len() == 16 {
        expected.push_str(":00");
    }
    let shifted = shift_to_zone(&iso, f[1]);
    ((shifted.as_deref() != Some(expected.as_str())).then(|| format!("shift {iso}: {shifted:?}")), true)
}

/// `L <zone> <local date-time> <valid offsets> <transition epoch second or -> <resolved local> <its offset> <its epoch
/// second>`: what differs, if anything.
fn zone_local(f: &[&str]) -> Option<String> {
    use lite_dialect::java_time::{LocalDateTime, zone_of};
    let zone = zone_of(f[1]);
    let ldt = LocalDateTime::parse(f[2]).unwrap_or_else(|| panic!("an unreadable local date-time {}", f[2]));
    let valid: Vec<i32> = f[3].split(',').filter(|s| !s.is_empty()).map(|s| s.parse().unwrap()).collect();
    let transition = (f[4] != "-").then(|| f[4].parse::<i64>().unwrap());
    let resolved = (f[5].to_owned(), f[6].parse::<i32>().unwrap(), f[7].parse::<i64>().unwrap());
    let rust_valid = zone.valid_offsets(&ldt);
    let rust_transition = zone.transition(&ldt).map(|t| t.map(|t| t.epoch_second));
    let rust_resolved = zone.resolve(&ldt).map(|(l, o)| (l.to_java_string(), o, l.to_epoch_second(o)));
    (rust_valid.as_ref() != Some(&valid) || rust_transition != Some(transition) || rust_resolved != Some(resolved))
        .then(|| format!("{rust_valid:?} {rust_transition:?} {rust_resolved:?}"))
}

/// `<zone> <iso> <Java's render>`: the seam's EngineStyleH2 of that zone rendering `select TIMESTAMP'<iso>' as "t"`.
fn engine_text_zone(f: &[&str]) -> Option<String> {
    let mut spec = Obj::new();
    spec.insert("dialect", Node::Str("EngineStyleH2".to_owned()));
    spec.insert("quoteIdentifiers", Node::Bool(false));
    spec.insert("timeZone", Node::Str(f[0].to_owned()));
    let d = open(&to_compact(&Node::Obj(spec))).unwrap();
    let literal = SqlExpr::TimestampLit {
        iso: f[1].to_owned(),
        r#type: TypeFact::Typed { r#type: SqlType::Scalar(Scalar::Timestamp), nullable: false },
    };
    let select = SqlSelect {
        projections: vec![Projection { expr: literal, alias: Some("t".to_owned()), out: None }],
        distinct: false,
        from: SqlSource::Dual,
        r#where: None,
        group_by: Vec::new(),
        having: None,
        qualify: None,
        order_by: Vec::new(),
        limit: None,
        offset: None,
        outputs: Vec::new(),
    };
    let rendered = match d.render(SqlQuery::Select(Box::new(select))) {
        Ok(text) => text,
        Err(r) => format!("THROWS {}: {}", r.class, r.message),
    };
    (rendered != f[2]).then_some(rendered)
}
