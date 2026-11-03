// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! The DuckDB dialect held to Java's own answers: every DuckDB statement of the golden dump (`LITE_MIR_DUMP`, JSON
//! lines of `{dialect, fresh, op, kind, mir, answer}`), rendered by a fresh `DuckDb` and compared with what the JVM's
//! fresh `DuckDb` answered -- the SQL text exactly, a refusal by its class and message, and (for now) a statement's
//! binds and regions by whether they are empty.
//!
//! The test asserts that at least the floor (`tests/duckdb-floor.txt`, or the file `DUCKDB_FLOOR` names) of the
//! statements are identical; the passes' port ratchets it up. It prints the counts by op and by MIR kind, the first
//! mismatches, and each mismatch's likely cause: the first stubbed pass, in the order the passes run, whose trigger
//! the statement holds (`rust/lite-dialect/PARITY.md` is that classification, read by hand). With
//! `LITE_DIALECT_MISMATCHES` set, every mismatch is written to the file it names.

use lite_dialect::rewriter::{SqlRewriter, rewrite};
use lite_dialect::statement::list_text;
use lite_dialect::{DuckDb, Refusal, RenderedStatement, SqlDialect};
use lite_json::{Node, Obj, parse_with_depth};
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
        let _ = writeln!(
            out,
            "\nmismatches by likely cause (the stubbed pass whose signature the first difference carries):"
        );
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
/// stead, or the text at the first difference), else the first stubbed pass, in the order the passes run, whose
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

/// The stubbed passes' triggers a statement holds, found by the shared walk.
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
        fn expr(&mut self, e: SqlExpr) -> Result<SqlExpr, Refusal> {
            self.0 |= matches!(e, SqlExpr::ScalarSubquery { .. });
            Ok(e)
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

    fn select(&mut self, s: SqlSelect) -> Result<SqlQuery, Refusal> {
        if s.group_by.iter().any(|k| matches!(k, SqlExpr::IntLit { .. })) {
            self.constant_key = true;
        }
        Ok(SqlQuery::Select(Box::new(s)))
    }

    fn source(&mut self, s: SqlSource) -> Result<SqlSource, Refusal> {
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
        Ok(s)
    }

    fn expr(&mut self, e: SqlExpr) -> Result<SqlExpr, Refusal> {
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
        Ok(e)
    }
}
