// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! The DuckDB dialect held to Java's own answers: every DuckDB statement of the golden dump (`LITE_MIR_DUMP`, JSON
//! lines of `{dialect, fresh, op, kind, mir, answer}`), rendered by a fresh `DuckDb` and compared with what the JVM's
//! fresh `DuckDb` answered -- the SQL text exactly, a refusal by its class and message, and (for now) a statement's
//! binds and regions by whether they are empty.
//!
//! The test asserts that at least the floor (`tests/duckdb-floor.txt`, or the file `DUCKDB_FLOOR` names) of the
//! statements are identical (all of them, since every pass is ported). It prints the counts by op and by MIR kind, the
//! first mismatches, and each mismatch's likely cause: the pass whose signature its first difference carries, else the
//! first pass, in the order the passes run, whose trigger the statement holds (`rust/lite-dialect/PARITY.md`). With
//! `LITE_DIALECT_MISMATCHES` set, every mismatch is written to the file it names.
//!
//! A second test holds the typing rules (`lite_dialect::typing`, Java's `SqlTyping`) to the facts Java stored on every
//! node of every statement of the dump. A third holds the seam (`lite_dialect::dispatch`, `SEAM.md`) to the renderer:
//! every DuckDB render op of the dump answered through it exactly as a direct call answers it.

use lite_dialect::rewriter::{Rw, SqlRewriter, rewrite};
use lite_dialect::seam::{read_statement, statement_json};
use lite_dialect::statement::list_text;
use lite_dialect::{DuckDb, Refusal, RenderedStatement, SqlDialect, dispatch};
use lite_json::{Node, Obj, parse_with_depth, to_compact};
use lite_sql::{
    AggFn, ColumnKind, ColumnType, JoinKind, MAX_DEPTH, Origin, Projection, Scalar, SqlDml, SqlExpr, SqlFn, SqlQuery,
    SqlSelect, SqlSource, SqlType, TypeFact, read_ddl, read_dml, read_query,
};
use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::io::{BufRead, BufReader};

const SHOWN: usize = 30;

#[test]
#[ignore = "reads the JVM's MirJson dump named by LITE_MIR_DUMP"]
fn duckdb_renders_as_java_does() {
    let path = std::env::var("LITE_MIR_DUMP")
        .expect("LITE_MIR_DUMP is unset: it names the JVM's MirJson dump (JSON lines) this test reads");
    let floor = floor();
    // the renderer recurses with the tree, which nests thousands deep (LITE_DIALECT_STACK_MB measures how deep)
    let stack_mb: usize = std::env::var("LITE_DIALECT_STACK_MB").ok().and_then(|v| v.parse().ok()).unwrap_or(256);
    let run = std::thread::Builder::new().stack_size(stack_mb << 20).spawn(move || compare_all(&path)).unwrap();
    let tally = run.join().unwrap();
    let report = tally.report();
    eprintln!("{report}");
    if let Ok(out) = std::env::var("LITE_DIALECT_MISMATCHES") {
        std::fs::write(&out, tally.mismatches.iter().map(|m| m.full.as_str()).collect::<Vec<_>>().join("\n"))
            .unwrap_or_else(|e| panic!("cannot write {out}: {e}"));
    }
    assert!(tally.statements > 0, "the dump holds no DuckDb statement");
    assert!(
        tally.identical >= floor,
        "{} of {} DuckDb statements render as Java's, below the floor of {floor}",
        tally.identical,
        tally.statements
    );
}

/// The floor: `DUCKDB_FLOOR` names its file (Bazel), else the crate's `tests/duckdb-floor.txt` (cargo).
fn floor() -> usize {
    let path = std::env::var("DUCKDB_FLOOR").unwrap_or_else(|_| {
        format!("{}/tests/duckdb-floor.txt", option_env!("CARGO_MANIFEST_DIR").unwrap_or("rust/lite-dialect"))
    });
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("cannot read the floor {path}: {e}"));
    text.lines()
        .map(str::trim)
        .find(|l| !l.is_empty() && !l.starts_with('#'))
        .and_then(|l| l.parse().ok())
        .unwrap_or_else(|| panic!("{path} holds no floor (a count of identical statements)"))
}

#[derive(Default)]
struct Tally {
    statements: usize,
    identical: usize,
    /// binds and regions written exactly as Java's record text, of the statements whose SQL is identical
    exact_binds: usize,
    statements_with_binds: usize,
    /// (op or kind) -> [identical, different, refused wrongly, accepted wrongly]
    by_op: BTreeMap<String, [usize; 4]>,
    by_kind: BTreeMap<String, [usize; 4]>,
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
    Statement { sql: String, binds: String, regions: String },
    Throws { class: String, message: String },
}

const IDENTICAL: usize = 0;
const DIFFERENT: usize = 1;
const REFUSED_WRONGLY: usize = 2;
const ACCEPTED_WRONGLY: usize = 3;

impl Tally {
    fn report(&self) -> String {
        let mut out = String::new();
        let _ = writeln!(
            out,
            "DuckDb: {} statements, {} identical, {} not (SQL compared exactly; binds/regions by emptiness)",
            self.statements,
            self.identical,
            self.statements - self.identical
        );
        let _ = writeln!(
            out,
            "binds and regions as Java's record text: {} of the {} identical statements that bind or have a region",
            self.exact_binds, self.statements_with_binds
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

fn compare_all(path: &str) -> Tally {
    let file = std::fs::File::open(path).unwrap_or_else(|e| panic!("cannot open {path}: {e}"));
    let mut tally = Tally::default();
    for (i, line) in BufReader::new(file).lines().enumerate() {
        let line = line.unwrap_or_else(|e| panic!("{path}:{}: {e}", i + 1));
        // the dialect is the first field: skip the others' lines unparsed
        if !line.starts_with("{\"dialect\":\"DuckDb\"") {
            continue;
        }
        compare(i + 1, &line, &mut tally);
    }
    tally
}

fn compare(at: usize, line: &str, tally: &mut Tally) {
    let doc = parse_with_depth(line, MAX_DEPTH).unwrap_or_else(|e| panic!("line {at}: unparsable: {e}"));
    let doc = doc.as_obj().unwrap();
    let op = doc.str("op").unwrap().to_owned();
    let kind = doc.str("kind").unwrap().to_owned();
    let mir = doc.obj("mir").unwrap_or_else(|e| panic!("line {at}: {e}"));
    let java = java_answer(doc.obj("answer").unwrap_or_else(|e| panic!("line {at}: {e}")));
    let (rust, query) = render(&op, mir).unwrap_or_else(|e| panic!("line {at}: {op} {kind}: unreadable MIR: {e}"));
    tally.statements += 1;
    let outcome = match (&java, &rust) {
        (Answer::Throws { .. }, Answer::Throws { .. }) | (Answer::Text(_), Answer::Text(_)) => {
            if java == rust {
                IDENTICAL
            } else {
                DIFFERENT
            }
        }
        (
            Answer::Statement { sql: js, binds: jb, regions: jr },
            Answer::Statement { sql: rs, binds: rb, regions: rr },
        ) => {
            let same = js == rs && (jb == "[]") == (rb == "[]") && (jr == "[]") == (rr == "[]");
            if same && (jb != "[]" || jr != "[]") {
                tally.statements_with_binds += 1;
                if jb == rb && jr == rr {
                    tally.exact_binds += 1;
                }
            }
            if same { IDENTICAL } else { DIFFERENT }
        }
        (_, Answer::Throws { .. }) => REFUSED_WRONGLY,
        (Answer::Throws { .. }, _) => ACCEPTED_WRONGLY,
        _ => DIFFERENT,
    };
    tally.by_op.entry(op.clone()).or_default()[outcome] += 1;
    tally.by_kind.entry(kind.clone()).or_default()[outcome] += 1;
    if outcome == IDENTICAL {
        tally.identical += 1;
        return;
    }
    let cause = cause(query.as_ref(), &java, &rust);
    *tally.causes.entry(cause).or_default() += 1;
    let short = format!("line {at} {op} {kind} [{cause}]: {}", difference(&java, &rust, 100));
    let examples = tally.examples.entry(cause).or_default();
    if examples.len() < 2 {
        examples.push(short.clone());
    }
    let full = format!("line {at} {op} {kind} [{cause}]\n  java: {java:?}\n  rust: {rust:?}");
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
        Node::Obj(o) => Answer::Statement {
            sql: o.str("sql").unwrap().to_owned(),
            binds: o.str("binds").unwrap().to_owned(),
            regions: o.str("regions").unwrap().to_owned(),
        },
        other => panic!("an ok answer neither text nor a statement: {other:?}"),
    }
}

/// Rust's answer for the op, and the query the passes would have seen (a DML statement's own, or its expressions in
/// a select; none for DDL).
fn render(op: &str, mir: &Obj) -> Result<(Answer, Option<SqlQuery>), lite_sql::Error> {
    let d = DuckDb::new();
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
/// stead, or the text at the first difference), else the first pass, in the order the passes run, whose
/// trigger the statement holds.
fn cause(q: Option<&SqlQuery>, java: &Answer, rust: &Answer) -> &'static str {
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
    if before.contains("(PIVOT ") && rust_rest.split([' ', ')', ',', '(']).next().is_some_and(|tok| tok.contains('.')) {
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
    let order: [(bool, &'static str); 12] = [
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
    ];
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
        Ok(Rw::Same(SqlQuery::Select(Box::new(s))))
    }

    fn source(&mut self, s: SqlSource) -> Result<Rw<SqlSource>, Refusal> {
        match &s {
            SqlSource::Pivot { .. } => self.pivot = true,
            SqlSource::Join { kind: JoinKind::Left, on: Some(on), .. } if holds_scalar_subquery(on) => {
                self.outer_join_subquery = true;
            }
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
            SqlExpr::WindowCall { r#fn, .. }
                if matches!(r#fn.as_ref(), lite_sql::SqlAgg::Reducer(r)
                    if matches!(r.r#fn, AggFn::QuantileCont | AggFn::QuantileDisc)) =>
            {
                self.quantile = true;
            }
            SqlExpr::FoldCall { .. } => self.fold = true,
            SqlExpr::CheckedDefects { .. } | SqlExpr::CheckedChildValue { .. } => self.checked_defects = true,
            _ => {}
        }
        let _ = TypeFact::Unknown;
        Ok(Rw::Same(e))
    }
}

// ==================================================================
// The seam held to the renderer it dispatches to
// ==================================================================

/// The spec the JVM's DuckDB sends (`SEAM.md`).
const DUCKDB_SPEC: &str = r#"{"dialect":"DuckDb","server":null,"engineScanOrder":false}"#;

/// The seam (`lite_dialect::dispatch`, the path `lite-ffi`'s `lite_dialect` takes) answering every DuckDB render op of
/// the dump exactly as a fresh `DuckDb` called directly answers it: the same JSON text (the SQL as a JSON string, a
/// statement in `SEAM.md`'s form), a statement read back from its JSON equal to the direct one (record for record)
/// and its binds and regions Java's record text, a refusal the same class and message. Each `query.render` line is
/// also asked for `query.renderInline`.
#[test]
#[ignore = "reads the JVM's MirJson dump named by LITE_MIR_DUMP"]
fn the_seam_answers_as_the_renderer_does() {
    let path = std::env::var("LITE_MIR_DUMP")
        .expect("LITE_MIR_DUMP is unset: it names the JVM's MirJson dump (JSON lines) this test reads");
    let stack_mb: usize = std::env::var("LITE_DIALECT_STACK_MB").ok().and_then(|v| v.parse().ok()).unwrap_or(256);
    let run = std::thread::Builder::new().stack_size(stack_mb << 20).spawn(move || seam_all(&path)).unwrap();
    let tally = run.join().unwrap();
    let mut report = String::new();
    let _ = writeln!(
        report,
        "seam: {} DuckDb render ops (and {} inline renders) dispatched, {} answered as the renderer does, {} \
         statements read back from JSON equal and Java's record text, {} refusals",
        tally.ops, tally.inline, tally.same, tally.statements, tally.refusals
    );
    for (op, n) in &tally.by_op {
        let _ = writeln!(report, "  {op:<24} {n:>6}");
    }
    for d in tally.different.iter().take(SHOWN) {
        let _ = writeln!(report, "{d}");
    }
    eprintln!("{report}");
    assert!(tally.ops > 0, "the dump holds no DuckDb statement");
    assert!(tally.different.is_empty(), "{} answers differ:\n{report}", tally.different.len());
    assert_eq!(tally.same, tally.ops + tally.inline);
}

#[derive(Default)]
struct SeamTally {
    ops: usize,
    inline: usize,
    same: usize,
    statements: usize,
    refusals: usize,
    by_op: BTreeMap<String, usize>,
    different: Vec<String>,
}

fn seam_all(path: &str) -> SeamTally {
    let file = std::fs::File::open(path).unwrap_or_else(|e| panic!("cannot open {path}: {e}"));
    let mut tally = SeamTally::default();
    for (i, line) in BufReader::new(file).lines().enumerate() {
        let line = line.unwrap_or_else(|e| panic!("{path}:{}: {e}", i + 1));
        if !line.starts_with("{\"dialect\":\"DuckDb\"") {
            continue;
        }
        let doc = parse_with_depth(&line, MAX_DEPTH).unwrap_or_else(|e| panic!("line {}: unparsable: {e}", i + 1));
        let doc = doc.as_obj().unwrap();
        let op = doc.str("op").unwrap();
        let mir = doc.obj("mir").unwrap();
        let java = java_answer(doc.obj("answer").unwrap());
        tally.ops += 1;
        *tally.by_op.entry(op.to_owned()).or_default() += 1;
        seam_one(i + 1, op, mir, Some(&java), &mut tally);
        if op == "query.render" {
            tally.inline += 1;
            seam_one(i + 1, "query.renderInline", mir, None, &mut tally);
        }
    }
    tally
}

/// One op through the seam and directly; `java` the JVM's answer, to hold a statement's record text to.
fn seam_one(at: usize, op: &str, mir: &Obj, java: Option<&Answer>, tally: &mut SeamTally) {
    let key = op.split('.').next().unwrap();
    let mut args = Obj::new();
    args.insert(key, Node::Obj(mir.clone()));
    let seam = dispatch(DUCKDB_SPEC, op, &to_compact(&Node::Obj(args)));
    let d = DuckDb::new();
    let direct: Result<Direct, Refusal> = match op {
        "query.render" => SqlDialect::render(&d, read_query(mir).unwrap()).map(Direct::Text),
        "query.renderInline" => SqlDialect::render_inline(&d, read_query(mir).unwrap()).map(Direct::Text),
        "query.renderStatement" => SqlDialect::render_statement(&d, read_query(mir).unwrap()).map(Direct::Statement),
        "ddl.render" => SqlDialect::render_ddl(&d, &read_ddl(mir).unwrap()).map(Direct::Text),
        "dml.render" => SqlDialect::render_dml(&d, &read_dml(mir).unwrap()).map(Direct::Text),
        "dml.renderStatement" => SqlDialect::render_dml_statement(&d, &read_dml(mir).unwrap()).map(Direct::Statement),
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
        (Ok(json), Ok(Direct::Statement(st))) => {
            let doc = parse_with_depth(json, MAX_DEPTH).unwrap_or_else(|e| panic!("line {at}: the seam's JSON: {e}"));
            let back = read_statement(doc.as_obj().unwrap()).unwrap_or_else(|r| panic!("line {at}: {r}"));
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
        Some(d) => tally.different.push(format!("line {at} {op}: {d}")),
    }
}

/// The direct renderer's answer.
enum Direct {
    Text(String),
    Statement(RenderedStatement),
}

// ==================================================================
// The typing rules held to Java's own facts
// ==================================================================

/// Java's `SqlTyping` as `lite_dialect::typing` ports it, held to the facts Java's constructors stored: every node of
/// every statement of the dump (every dialect's: one tree), its fact recomputed from its children's and compared with
/// the one MirJson carries; a projection frame's outputs rebuilt from its projections, a union's labels reconciled, a
/// join's delivered columns, each compared with Java's. A leaf the builder supplies (a column, a lambda, a parameter,
/// a key cell) has no rule; an array literal or a field read whose children prove no type keeps the one it was given.
#[test]
#[ignore = "reads the JVM's MirJson dump named by LITE_MIR_DUMP"]
fn typing_reproduces_javas_facts() {
    let path = std::env::var("LITE_MIR_DUMP")
        .expect("LITE_MIR_DUMP is unset: it names the JVM's MirJson dump (JSON lines) this test reads");
    let stack_mb: usize = std::env::var("LITE_DIALECT_STACK_MB").ok().and_then(|v| v.parse().ok()).unwrap_or(256);
    let run = std::thread::Builder::new().stack_size(stack_mb << 20).spawn(move || check_all_facts(&path)).unwrap();
    let facts = run.join().unwrap();
    let mut report = String::new();
    let _ = writeln!(report, "typing: {} statements, {} facts checked", facts.statements, facts.checked);
    for (kind, [checked, wrong]) in &facts.by_kind {
        let _ = writeln!(report, "  {kind:<28} {checked:>9} checked {wrong:>6} different");
    }
    for e in facts.examples.iter().take(SHOWN) {
        let _ = writeln!(report, "{e}");
    }
    eprintln!("{report}");
    let wrong: usize = facts.by_kind.values().map(|v| v[1]).sum();
    assert!(facts.checked > 0, "the dump holds no statement");
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
    fn check<T: PartialEq + std::fmt::Debug>(&mut self, at: usize, kind: &str, computed: T, stored: &T) {
        self.checked += 1;
        let tally = self.by_kind.entry(kind.to_owned()).or_default();
        tally[0] += 1;
        if computed != *stored {
            tally[1] += 1;
            if self.examples.len() < SHOWN {
                self.examples.push(format!("line {at} {kind}:\n    java: {stored:?}\n    rust: {computed:?}"));
            }
        }
    }
}

fn check_all_facts(path: &str) -> Facts {
    let file = std::fs::File::open(path).unwrap_or_else(|e| panic!("cannot open {path}: {e}"));
    let mut facts = Facts::default();
    for (i, line) in BufReader::new(file).lines().enumerate() {
        let line = line.unwrap_or_else(|e| panic!("{path}:{}: {e}", i + 1));
        let doc = parse_with_depth(&line, MAX_DEPTH).unwrap_or_else(|e| panic!("line {}: unparsable: {e}", i + 1));
        let doc = doc.as_obj().unwrap();
        let op = doc.str("op").unwrap();
        let mir = doc.obj("mir").unwrap();
        let query = match op {
            "query.render" | "query.renderStatement" => read_query(mir).ok(),
            "dml.render" | "dml.renderStatement" => read_dml(mir).ok().and_then(|d| dml_query(&d)),
            _ => None,
        };
        if let Some(q) = query {
            facts.statements += 1;
            query_facts(i + 1, &q, &mut facts);
        }
    }
    facts
}

fn query_facts(at: usize, q: &SqlQuery, f: &mut Facts) {
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

fn source_facts(at: usize, s: &SqlSource, f: &mut Facts) {
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

fn expr_facts(at: usize, e: &SqlExpr, f: &mut Facts) {
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
