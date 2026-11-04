// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! Java's `MongoDb` (`core/src/main/java/com/legend/sql/dialect/MongoDb.java`), the MongoDB target
//! (docs/MONGODB_PLAN_2026_10_16.md): the one query compiler's render target whose text is an `aggregate` command -- a
//! pipeline in MongoDB's Extended JSON -- rather than SQL. It is a renderer like every target, overriding the writing
//! of a query ([`pipeline`], Java's `MongoDbPipeline`: the stages) and of an expression (MongoDB's aggregation
//! operators), keeping the base's compositions that are pure IR (the checked narrowing, the compaction). Every function
//! (`functions`) and every aggregate (`aggregates`) is a spelling, an emulation over MongoDB's operators, or a
//! refusal by name; a comparison is three-valued through a null guard ([`json::guarded`]); a plan parameter is a marker
//! document the connector replaces in place (`{"$legendParam": "?"}`), bound by its place among the markers; a raise is
//! a conversion that cannot succeed, its message between the platform's U+001F sentinels. [`json`] is Java's
//! `MongoJson`, `texts` the long compositions (a number's and a value's JSON text, the string distances, printf, the
//! date formats), [`java_text`] the records' `toString` the lowering-bug refusals embed.
//!
//! Java's per-render state -- `scopes` (each level of the pipeline: the lookup's variable aliases, the flat alias, the
//! reads of the markers its stages filled, the sole alias of the select being written) and `lambdaVariables` (the
//! lambda parameters in scope, each the pipeline variable it reads as) -- is the dialect object's own: two
//! [`RefCell`] stacks, reset by every query render as Java's `query` resets them, so a fresh dialect per call is
//! exact. Java's `SqlWriter.Piece` is [`Pc`]: a closure handed the dialect when it is written, so a read a stage
//! registers (an aggregate's, a subquery's, a window's) owns what it writes and outlives the stage that made it.

use crate::ansi;
use crate::carrier;
use crate::dialect::KeysSnapshot;
use crate::refusal::Refusal;
use crate::renderer::{AnsiSqlRenderer, AnsiState, Lexicon, PlanParam};
use crate::rewriter::SqlRewriter;
use crate::statement::{Bind, Binding, RenderedStatement};
use crate::writer::SqlWriter;
use lite_sql::{
    AggFn, Column, ColumnKind, ColumnType, DateFmt, Frame, JsonArrayAggKey, JsonKind, Lambda, Origin, Reducer, Scalar,
    SortKey, SqlAgg, SqlDdl, SqlDml, SqlExpr, SqlFn, SqlQuery, SqlType, StructField, StructLitField, TypeFact, When,
};
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::LazyLock;

mod aggregates;
mod functions;
pub mod java_text;
pub mod json;
pub mod pipeline;
mod texts;

#[cfg(test)]
mod tests;

pub use aggregates::GroupField;

/// Java's `SqlWriter.Piece` as MongoDB composes it: written where a helper writes it, as often as it does, handed the
/// dialect that writes its expressions. A piece a stage registers as a read owns its data (`Pc<'static>`).
pub type Pc<'a> = Rc<dyn Fn(&MongoDb, &mut SqlWriter) -> Result<(), Refusal> + 'a>;

/// A piece from a closure (Java's `w -> ...`).
pub fn pc<'a>(f: impl Fn(&MongoDb, &mut SqlWriter) -> Result<(), Refusal> + 'a) -> Pc<'a> {
    Rc::new(f)
}

/// A piece of fixed text (Java's `MongoJson.text`).
pub fn text<'a>(s: impl Into<String>) -> Pc<'a> {
    let s = s.into();
    pc(move |_, w| {
        w.append(&s);
        Ok(())
    })
}

/// A piece writing `e` (Java's `MongoJson.of`, `x -> x.expr(e, 0)`).
pub fn of(e: &SqlExpr) -> Pc<'_> {
    pc(move |d, w| {
        w.expr(d, e, 0)?;
        Ok(())
    })
}

// ==================================================================
// The dialect's tables
// ==================================================================

/// No quoting: a field or collection name is spelled as a JSON string, never delimited (Java's `LEXICON`).
static LEXICON: LazyLock<Lexicon> = LazyLock::new(|| Lexicon::new('"', &[]));

/// Lists are native BSON arrays; a correlated explosion, a JSON carrier and a native full join are not shaped.
pub const CAPS: carrier::Caps =
    carrier::Caps { native_lists: true, correlated_explode: false, json_carrier: false, native_full_outer: false };

/// The field a group stage computes an aggregate into, by its index (`__agg0`).
pub const AGGREGATE_MARKER: &str = "__agg";
/// The field a lookup stage computes a subquery into, by its index (`__sq0`).
pub const SUBQUERY_MARKER: &str = "__sq";
/// A window function's field, set by a `$setWindowFields` stage and read by the clauses after it.
pub const WINDOW_MARKER: &str = "__w";
/// A group key's read after the group stage (`_id.k0`).
const GROUP_KEY: &str = "_id.k";
/// A plan parameter's place in a command: the marker document the connector replaces by the value of the slot at its
/// place among the markers.
pub const MARKER: &str = "{\"$legendParam\": \"?\"}";

pub(crate) const RAISE_SENTINEL: &str = "\u{1f}";
pub(crate) const POSITION_DIVIDER: &str = "\u{1e}";

pub(crate) const G0: &str = "\"$$g0\"";
pub(crate) const G1: &str = "\"$$g1\"";
pub(crate) const G2: &str = "\"$$g2\"";
pub(crate) const THIS: &str = "\"$$this\"";
pub(crate) const VALUE: &str = "\"$$value\"";

/// An empty text that depends on the row (`$$ROOT`'s type, cut to nothing): concatenated into a raise so the server's
/// optimizer cannot fold the conversion of a constant message and raise before any row is read.
pub(crate) const UNFOLDED: &str = "{\"$substrCP\": [{\"$type\": \"$$ROOT\"}, 0, 0]}";

// ==================================================================
// Scope: how a column reference spells inside the pipeline
// ==================================================================

/// One level of the pipeline (Java's `Scope`): inside a lookup, the left row's aliases are `let` variables and the
/// right source's rows are flat; at the top, every alias is a field of the current row.
struct Scope {
    variable_aliases: Vec<String>,
    flat_alias: Option<String>,
    /// The reads of this level's markers (Java's `LinkedHashMap<String, Piece>`), each filled by its stage and read by
    /// the clauses after it.
    reads: Vec<(String, Pc<'static>)>,
    /// The one alias a select's row is wrapped under while its clauses are written.
    sole_alias: Option<String>,
}

impl Scope {
    fn new(variable_aliases: Vec<String>, flat_alias: Option<String>) -> Scope {
        Scope { variable_aliases, flat_alias, reads: Vec::new(), sole_alias: None }
    }
}

/// The MongoDB dialect: one per render, as Java's `new MongoDb()`.
#[derive(Default)]
pub struct MongoDb {
    ansi: AnsiState,
    /// Java's `scopes`: the pipeline's levels, the innermost (Java's `getFirst()`) last.
    scopes: RefCell<Vec<Scope>>,
    /// Java's `lambdaVariables`: each lambda parameter in scope and the variable it reads as, the innermost last.
    lambda_variables: RefCell<Vec<(String, String)>>,
}

impl std::fmt::Debug for MongoDb {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MongoDb").finish_non_exhaustive()
    }
}

/// Java's `ArrayDeque.getFirst()` on an empty deque.
fn no_scope() -> Refusal {
    Refusal { class: "java.util.NoSuchElementException", message: String::new() }
}

impl MongoDb {
    /// Java's `new MongoDb()`.
    pub fn new() -> MongoDb {
        MongoDb::default()
    }

    /// A lookup's pipeline: the row's aliases become its `let` variables; `flat` is the alias whose rows the pipeline
    /// sees flat (a join's right source), or none for a subquery's pipeline.
    pub(crate) fn push_lookup_scope(&self, variables: &[String], flat: Option<&str>) {
        self.scopes.borrow_mut().push(Scope::new(variables.to_vec(), flat.map(str::to_owned)));
    }

    pub(crate) fn pop_scope(&self) {
        self.scopes.borrow_mut().pop();
    }

    /// The alias a select's wrapped row reads unqualified columns under, or none once the row is flat.
    pub(crate) fn sole_alias(&self, alias: Option<String>) -> Result<(), Refusal> {
        let mut scopes = self.scopes.borrow_mut();
        scopes.last_mut().ok_or_else(no_scope)?.sole_alias = alias;
        Ok(())
    }

    pub(crate) fn aggregate_reads(&self, reads: Vec<(String, Pc<'static>)>) -> Result<(), Refusal> {
        self.replace_reads(AGGREGATE_MARKER, reads)
    }

    pub(crate) fn subquery_reads(&self, reads: Vec<(String, Pc<'static>)>) -> Result<(), Refusal> {
        self.replace_reads(SUBQUERY_MARKER, reads)
    }

    pub(crate) fn window_reads(&self, reads: Vec<(String, Pc<'static>)>) -> Result<(), Refusal> {
        self.replace_reads(WINDOW_MARKER, reads)
    }

    /// This level's reads of one marker kind replaced: a select's markers are its own, and a nested select's, written
    /// earlier under the same names, are spent.
    fn replace_reads(&self, marker: &str, reads: Vec<(String, Pc<'static>)>) -> Result<(), Refusal> {
        let mut scopes = self.scopes.borrow_mut();
        let scope = scopes.last_mut().ok_or_else(no_scope)?;
        scope.reads.retain(|(k, _)| !k.starts_with(marker));
        for (k, v) in reads {
            match scope.reads.iter_mut().find(|(key, _)| *key == k) {
                Some(slot) => slot.1 = v,
                None => scope.reads.push((k, v)),
            }
        }
        Ok(())
    }

    fn reset(&self) {
        let mut scopes = self.scopes.borrow_mut();
        scopes.clear();
        scopes.push(Scope::new(Vec::new(), None));
        self.lambda_variables.borrow_mut().clear();
    }

    /// The variable a lambda parameter `name` reads as, the innermost binding first.
    fn lambda_variable(&self, name: &str) -> Option<String> {
        self.lambda_variables.borrow().iter().rev().find(|(k, _)| k == name).map(|(_, v)| v.clone())
    }

    /// `body` written with the lambda's parameters bound to `variables`, in order (Java's `withLambda`).
    pub(crate) fn with_lambda(
        &self,
        w: &mut SqlWriter,
        l: &Lambda,
        variables: &[String],
        body: &Pc<'_>,
    ) -> Result<(), Refusal> {
        if l.params.len() != variables.len() {
            return Err(Refusal::illegal_state(format!(
                "lambda {} bound to {}",
                java_text::strings(&l.params),
                java_text::strings(variables)
            )));
        }
        {
            let mut vars = self.lambda_variables.borrow_mut();
            for (p, v) in l.params.iter().zip(variables) {
                vars.push((p.clone(), v.clone()));
            }
        }
        let written = body(self, w);
        {
            let mut vars = self.lambda_variables.borrow_mut();
            for _ in variables {
                vars.pop();
            }
        }
        written
    }

    // ==================================================================
    // Column references
    // ==================================================================

    /// A column reference as a path: an aggregate's marker reads the group field; a group key reads `_id`; a lambda's
    /// parameter is its variable (a field of it, the variable's field); inside a lookup the left row's aliases are
    /// variables and the right source's row is flat; otherwise the row holds each source under its alias.
    fn column(&self, w: &mut SqlWriter, c: &Column) -> Result<(), Refusal> {
        let name = c.name.as_str();
        let table = c.table.as_deref();
        let (variable_aliases, flat_alias, sole_alias) = {
            let scopes = self.scopes.borrow();
            let scope = scopes.last().ok_or_else(no_scope)?;
            (scope.variable_aliases.clone(), scope.flat_alias.clone(), scope.sole_alias.clone())
        };
        if c.origin == Some(Origin::PerKey) {
            return per_key_column(w, &variable_aliases, flat_alias.as_deref(), sole_alias.as_deref(), table, name);
        }
        let Some(table) = table else {
            if name.starts_with(AGGREGATE_MARKER)
                || name.starts_with(SUBQUERY_MARKER)
                || name.starts_with(WINDOW_MARKER)
            {
                let read = {
                    let scopes = self.scopes.borrow();
                    let scope = scopes.last().ok_or_else(no_scope)?;
                    scope.reads.iter().find(|(k, _)| k == name).map(|(_, p)| p.clone())
                };
                let Some(read) = read else {
                    return Err(Refusal::illegal_state(format!("marker {name} has no read in this scope")));
                };
                return read(self, w);
            }
            if name.starts_with(GROUP_KEY) {
                w.append(&json::string(&format!("${name}")));
                return Ok(());
            }
            if let Some(v) = self.lambda_variable(name) {
                w.append(&json::string(&format!("$${v}")));
                return Ok(());
            }
            json::key(name)?;
            if let Some(sole) = &sole_alias {
                json::key(sole)?;
                w.append(&json::path(&[sole, name]));
                return Ok(());
            }
            w.append(&json::path(&[name]));
            return Ok(());
        };
        json::key(name)?;
        if let Some(v) = self.lambda_variable(table) {
            // a lambda parameter's field (a struct element's, a pair's): the variable's field
            w.append(&json::string(&format!("$${v}.{name}")));
            return Ok(());
        }
        if variable_aliases.iter().any(|a| a == table) {
            w.append(&json::string(&format!("$${}.{name}", json::variable(table))));
            return Ok(());
        }
        if flat_alias.as_deref() == Some(table) {
            w.append(&json::path(&[name]));
            return Ok(());
        }
        json::key(table)?;
        w.append(&json::path(&[table, name]));
        Ok(())
    }
}

/// A per-key column of a dynamic pivot's probe plan (`_u0`) read in a copy of its per-key region:
/// `"$_pct0._pk3__u0"` for the fourth key, the runner writing the key's position after `_pk`.
fn per_key_column(
    w: &mut SqlWriter,
    variable_aliases: &[String],
    flat_alias: Option<&str>,
    sole_alias: Option<&str>,
    table: Option<&str>,
    name: &str,
) -> Result<(), Refusal> {
    json::key(name)?;
    let under = match table {
        Some(t) => {
            if Some(t) == flat_alias {
                None
            } else {
                Some(t)
            }
        }
        None => sole_alias,
    };
    if let Some(u) = under {
        json::key(u)?;
        if variable_aliases.iter().any(|a| a == u) {
            return Err(Refusal::capability(
                "a dynamic pivot's per-key column read inside a lookup reached MongoDb: no shape yet",
            ));
        }
    }
    let head = match under {
        None => "\"$".to_owned(),
        Some(u) => format!("\"${u}."),
    };
    w.append(&head).append("_pk").counter()?.append(&format!("_{name}\""));
    Ok(())
}

/// A parameter written where ONE value goes: a list parameter is bound only as a whole list.
fn one_value<'a>(p: &PlanParam<'a>) -> Result<PlanParam<'a>, Refusal> {
    if let TypeFact::Typed { r#type: SqlType::Array { .. }, .. } = p.r#type {
        return Err(Refusal::capability(format!(
            "list parameter '{}' is bound only as a whole list (in, contains); written where one value goes, it is \
             not bound",
            p.name
        )));
    }
    Ok(*p)
}

/// A list parameter's bind: the whole list, as one array of its element's type.
fn list_bind(p: &PlanParam<'_>) -> Result<Bind, Refusal> {
    if let TypeFact::Typed { r#type: SqlType::Array { element }, .. } = p.r#type
        && let SqlType::Scalar(element) = element.as_ref()
    {
        return Ok(Bind {
            parameter: p.name.to_owned(),
            binding: Binding::Array { element_sql_type: element.as_str().to_owned() },
        });
    }
    Err(Refusal::capability(format!(
        "plan parameter '{}' is not a list parameter (an array of a named element type): it is not bound as a whole \
         list",
        p.name
    )))
}

/// DATE or TIMESTAMP when `e` is typed so; none otherwise.
pub(crate) fn temporal(e: &SqlExpr) -> Option<Scalar> {
    match e.r#type() {
        TypeFact::Typed { r#type: SqlType::Scalar(s @ (Scalar::Date | Scalar::Timestamp)), .. } => Some(*s),
        _ => None,
    }
}

/// Whether `e` is typed as the scalar `s`.
pub(crate) fn typed_as(e: &SqlExpr, s: Scalar) -> bool {
    matches!(e.r#type(), TypeFact::Typed { r#type: SqlType::Scalar(t), .. } if *t == s)
}

/// A list of `Pc`s' JSON-free text join helper: the Java list `[a, b]` of the strings.
fn ddl_collection(schema: Option<&str>, table: &str) -> Result<String, Refusal> {
    if let Some(s) = schema
        && !s.is_empty()
        && s != "default"
    {
        return Err(Refusal::capability(format!(
            "a schema-qualified table ({s}.{table}) reached MongoDb: a collection has no schema"
        )));
    }
    if table.contains('.') {
        return Err(Refusal::capability(format!(
            "a schema-qualified table ({table}) reached MongoDb: a collection has no schema"
        )));
    }
    Ok(table.to_owned())
}

/// A row's cell as a VALUE in Extended JSON (an inserted document is data, not an expression): a literal, or null;
/// anything else is refused by name.
fn insert_value(w: &mut SqlWriter, cell: &SqlExpr) -> Result<(), Refusal> {
    match cell {
        SqlExpr::StringLit { value, .. } => {
            w.append(&json::string(value));
        }
        SqlExpr::IntLit { value, .. } => {
            w.append("{\"$numberLong\": \"").append_long(*value).append("\"}");
        }
        SqlExpr::FloatLit { value, .. } => {
            let text = crate::java::plain_float(value.value())?;
            w.append("{\"$numberDouble\": \"").append(&text).append("\"}");
        }
        SqlExpr::DecimalLit { value, .. } => {
            w.append("{\"$numberDecimal\": \"").append(&crate::java::big_decimal_plain(value.as_str())).append("\"}");
        }
        SqlExpr::BoolLit { value, .. } => {
            w.append(if *value { "true" } else { "false" });
        }
        SqlExpr::NullLit { .. } => {
            w.append("null");
        }
        SqlExpr::DateLit { iso, .. } | SqlExpr::TimestampLit { iso, .. } => {
            w.append(&json::date_literal(iso)?);
        }
        SqlExpr::Cast { value, .. } => return insert_value(w, value),
        other => {
            return Err(Refusal::capability(format!(
                "a row's cell is {}: a MongoDb insert holds values, not expressions",
                crate::java::simple_name(other)
            )));
        }
    }
    Ok(())
}

impl MongoDb {
    /// Rows in: the `insert` command, each row a document of values.
    fn insert(
        &self,
        schema: Option<&str>,
        table: &str,
        columns: &[String],
        rows: &[Vec<SqlExpr>],
    ) -> Result<String, Refusal> {
        if columns.is_empty() {
            return Err(Refusal::capability(
                "an insert naming no columns reached MongoDb: a document's fields are named",
            ));
        }
        let mut w = SqlWriter::new();
        let collection = ddl_collection(schema, table)?;
        w.append("{\"insert\": ").append(&json::string(&collection)).append(", \"documents\": [");
        for (r, row) in rows.iter().enumerate() {
            if row.len() != columns.len() {
                return Err(Refusal::illegal_state(format!(
                    "a row of {} values for {} columns",
                    row.len(),
                    columns.len()
                )));
            }
            w.append(if r > 0 { ", {" } else { "{" });
            for (c, cell) in row.iter().enumerate() {
                w.append(if c > 0 { ", " } else { "" }).append(&json::key(&columns[c])?).append(": ");
                insert_value(&mut w, cell)?;
            }
            w.append("}");
        }
        w.append("]}");
        w.into_text()
    }
}

// ==================================================================
// The renderer
// ==================================================================

impl AnsiSqlRenderer for MongoDb {
    fn lexicon(&self) -> &Lexicon {
        &LEXICON
    }

    fn ansi_state(&self) -> &AnsiState {
        &self.ansi
    }

    fn simple_name(&self) -> &'static str {
        "MongoDb"
    }

    /// The engine's H2 text has no translation into a pipeline.
    fn from_engine_text(&self, statement: &str) -> Result<String, Refusal> {
        let _ = statement;
        Err(Refusal::capability("the engine's H2 text reached MongoDb, which runs no SQL text"))
    }

    fn passes(&self) -> Vec<Box<dyn SqlRewriter>> {
        vec![Box::new(carrier::CarrierStrategies::new(CAPS))]
    }

    /// A collection's `_id` declared `OTHER` (an objectId) is read as its hex text.
    fn reads_stored(t: &ColumnType) -> bool {
        matches!(t, ColumnType::Plain { kind: ColumnKind::Other })
    }

    fn stored_read(&self, w: &mut SqlWriter, column: &Column, stored: &ColumnType) -> Result<(), Refusal> {
        let _ = stored;
        w.append("{\"$toString\": ").expr(self, &SqlExpr::Column(column.clone()), 0)?.append("}");
        Ok(())
    }

    // ---- the query: a pipeline

    fn query(&self, w: &mut SqlWriter, q: &SqlQuery, depth: usize) -> Result<(), Refusal> {
        if depth != 0 {
            return Err(Refusal::capability(
                "a nested query reached MongoDb as a subquery: only a relation's own pipeline is written",
            ));
        }
        self.reset();
        pipeline::command(self, w, q)
    }

    // ---- expressions

    fn expr(&self, w: &mut SqlWriter, e: &SqlExpr, parent_prec: i32) -> Result<(), Refusal> {
        match e {
            SqlExpr::Group { inner, .. } => {
                w.expr(self, inner, 0)?;
            }
            SqlExpr::TempTableInSplice { temp_table_name, .. } => {
                return Err(Refusal::illegal_state(format!(
                    "temp-table IN splice '{temp_table_name}' reached an executable dialect — plan-text vocabulary \
                     only"
                )));
            }
            SqlExpr::PlanParam { .. } => {
                let p = PlanParam::of(e).expect("a plan parameter");
                return self.placeholder(w, &one_value(&p)?);
            }
            // a dynamic pivot's probe plan: each copy of the per-key region binds its key's text
            SqlExpr::KeyCell { cell, .. } => {
                w.bind_key_as(MARKER, *cell)?;
            }
            SqlExpr::RowOrder { .. } => {
                return Err(Refusal::capability(
                    "a physical row order reached MongoDb: a collection's documents have no row order to read",
                ));
            }
            SqlExpr::Column(c) => return self.column(w, c),
            SqlExpr::StoredRead { column, stored, .. } => return self.stored_read(w, column, stored),
            SqlExpr::Star { .. } => {
                return Err(Refusal::illegal_state(
                    "a star reached the expression writer: the projection stage merges the sources it names",
                ));
            }
            SqlExpr::StarExcept { .. } => {
                return Err(Refusal::capability("a star with excluded columns reached MongoDb: no shape yet"));
            }
            SqlExpr::StringLit { value, .. } => json::literal(w, value),
            SqlExpr::FormatLit { parts, .. } => {
                let t = self.format_text(parts)?;
                json::literal(w, &t);
            }
            SqlExpr::DatePartLit { part, .. } => {
                return Err(Refusal::illegal_state(format!(
                    "a date part ({part}) reached the expression writer: the arm of its call spells it"
                )));
            }
            SqlExpr::JsonKindLit { kind, .. } => {
                let t = self.json_kind_name(*kind)?;
                json::literal(w, &t);
            }
            SqlExpr::TypeNameLit { scalar, .. } => {
                let t = self.typeof_name(*scalar)?;
                json::literal(w, &t);
            }
            SqlExpr::IntLit { value, .. } => {
                w.append("{\"$numberLong\": \"").append_long(*value).append("\"}");
            }
            SqlExpr::FloatLit { value, .. } => {
                w.append("{\"$numberDouble\": \"");
                let text = self.float_literal(value.value())?;
                w.append(&text).append("\"}");
            }
            SqlExpr::DecimalLit { value, .. } => {
                w.append("{\"$numberDecimal\": \"")
                    .append(&crate::java::big_decimal_plain(value.as_str()))
                    .append("\"}");
            }
            SqlExpr::BoolLit { value, .. } => {
                w.append(if *value { "true" } else { "false" });
            }
            SqlExpr::NullLit { .. } => {
                w.append("null");
            }
            SqlExpr::DateLit { iso, .. } | SqlExpr::TimestampLit { iso, .. } => {
                w.append(&json::date_literal(iso)?);
            }
            SqlExpr::TimeLit { .. } => {
                return Err(Refusal::capability("a time-of-day literal reached MongoDb, which has no time type"));
            }
            SqlExpr::OrderedListAgg { .. } => {
                return Err(Refusal::illegal_state(
                    "an ordered list aggregate reached the expression writer outside a group stage",
                ));
            }
            SqlExpr::ArrayLit { elements, .. } => return self.array_lit(w, elements),
            SqlExpr::StructLit { fields, .. } => return self.struct_lit(w, fields),
            SqlExpr::StructGet { source, field, r#type } => return self.struct_get(w, source, field, r#type),
            SqlExpr::Call { r#fn, args, r#type } => return self.call(w, *r#fn, args, r#type, parent_prec),
            SqlExpr::Case { whens, otherwise, r#type } => {
                return self.case_expr(w, whens, otherwise.as_deref(), r#type);
            }
            SqlExpr::Exists { .. } => {
                return Err(Refusal::capability(
                    "an EXISTS subquery reached MongoDb: a correlated subquery has no shape yet",
                ));
            }
            SqlExpr::InSubquery { .. } => {
                return Err(Refusal::capability(
                    "an IN subquery reached MongoDb: a subquery in expression position has no shape yet",
                ));
            }
            SqlExpr::CheckedDefects { .. } => {
                return Err(Refusal::capability("nested checked defects reached MongoDb: no shape yet"));
            }
            SqlExpr::CheckedChildValue { .. } => {
                return Err(Refusal::capability("a checked child's value reached MongoDb: no shape yet"));
            }
            SqlExpr::Quantified { .. } => {
                return Err(Refusal::illegal_state(
                    "a quantified comparison reached the expression writer outside the lookup stage that computes \
                     its rows (MongoDbPipeline.subqueries)",
                ));
            }
            SqlExpr::ScalarSubquery { .. } => {
                return Err(Refusal::capability("a scalar subquery reached MongoDb: no shape yet"));
            }
            SqlExpr::CheckedOne { list, scalar_carrier, at_least_only, .. } => {
                return self.checked_one(w, list, *scalar_carrier, *at_least_only, parent_prec);
            }
            SqlExpr::CompactList { list, .. } => return self.compact_list(w, list, parent_prec),
            SqlExpr::DeferredTdsString { id, .. } => {
                return Err(Refusal::illegal_state(format!(
                    "deferred relation-toString reached the renderer — the execution boundary must resolve the \
                     dynamic column list first (DeferredTdsString id {id})"
                )));
            }
            SqlExpr::WindowCall { r#fn, partition_by, order_by, frame, .. } => {
                return self.window_call(w, r#fn, partition_by, order_by, frame.as_ref());
            }
            SqlExpr::Lambda(l) => return self.lambda(w, l),
            SqlExpr::Cast { value, target: SqlType::Tagged { .. }, .. } => {
                w.expr(self, value, parent_prec)?;
            }
            SqlExpr::Cast { value, target, conform, .. } => return self.variant_aware_cast(w, value, target, *conform),
            SqlExpr::FoldCall { .. } => return self.fold_call(w, e),
            SqlExpr::JsonObject { kv, .. } => return self.json_object(w, kv),
            SqlExpr::JsonArray { elements, .. } => return self.json_array(w, elements),
            SqlExpr::JsonArrayAgg { value, order_keys, .. } => return self.json_array_agg(w, value, order_keys),
            SqlExpr::ReduceCollection { reducer, collection, extras, .. } => {
                return self.reduce_collection(w, *reducer, collection, extras);
            }
            SqlExpr::Membership { needle, collection, .. } => return self.membership(w, needle, collection),
            SqlExpr::Reducer(r) => return self.reducer(w, r),
        }
        Ok(())
    }

    /// A plan parameter: a marker the connector replaces in place by the bound value, by its place among the
    /// statement's markers.
    fn placeholder(&self, w: &mut SqlWriter, p: &PlanParam<'_>) -> Result<(), Refusal> {
        let bind = ansi::scalar_bind(p)?;
        w.bind_as(MARKER, bind);
        Ok(())
    }

    fn array_lit(&self, w: &mut SqlWriter, elements: &[SqlExpr]) -> Result<(), Refusal> {
        w.append("[").list(self, elements)?.append("]");
        Ok(())
    }

    /// A struct is a document: each field by its name.
    fn struct_lit(&self, w: &mut SqlWriter, fields: &[StructLitField]) -> Result<(), Refusal> {
        w.append("{");
        for (i, f) in fields.iter().enumerate() {
            if i > 0 {
                w.append(", ");
            }
            w.append(&json::key(&f.name)?).append(": ").expr(self, &f.value, 0)?;
        }
        w.append("}");
        Ok(())
    }

    fn struct_get(&self, w: &mut SqlWriter, source: &SqlExpr, field: &str, r#type: &TypeFact) -> Result<(), Refusal> {
        let _ = r#type;
        w.append("{\"$getField\": {\"field\": ").append(&json::key(field)?).append(", \"input\": ");
        w.expr(self, source, 0)?.append("}}");
        Ok(())
    }

    /// A CASE as `$switch`: a null condition takes no branch, as SQL's does; no ELSE answers null.
    fn case_expr(
        &self,
        w: &mut SqlWriter,
        whens: &[When],
        otherwise: Option<&SqlExpr>,
        r#type: &TypeFact,
    ) -> Result<(), Refusal> {
        let _ = r#type;
        w.append("{\"$switch\": {\"branches\": [");
        for (i, wh) in whens.iter().enumerate() {
            if i > 0 {
                w.append(", ");
            }
            w.append("{\"case\": ").expr(self, &wh.condition, 0)?.append(", \"then\": ");
            w.expr(self, &wh.then, 0)?.append("}");
        }
        w.append("], \"default\": ");
        match otherwise {
            None => {
                w.append("null");
            }
            Some(o) => {
                w.expr(self, o, 0)?;
            }
        }
        w.append("}}");
        Ok(())
    }

    fn window_call(
        &self,
        w: &mut SqlWriter,
        r#fn: &SqlAgg,
        partition_by: &[SqlExpr],
        order_by: &[SortKey],
        frame: Option<&Frame>,
    ) -> Result<(), Refusal> {
        let _ = (w, r#fn, partition_by, order_by, frame);
        Err(Refusal::illegal_state(
            "a window function reached the expression writer outside the $setWindowFields stage that computes it \
             (MongoDbPipeline.windows)",
        ))
    }

    fn lambda(&self, w: &mut SqlWriter, l: &Lambda) -> Result<(), Refusal> {
        let _ = (w, l);
        Err(Refusal::illegal_state("a lambda reached the expression writer outside the list operator that applies it"))
    }

    fn variant_aware_cast(
        &self,
        w: &mut SqlWriter,
        value: &SqlExpr,
        target: &SqlType,
        conform: bool,
    ) -> Result<(), Refusal> {
        let _ = conform;
        texts::variant_aware_cast(self, w, value, target)
    }

    /// A fold as `$reduce`, the lambda's Pure order (element, accumulator) read as `$$this` and `$$value`; the empty
    /// (null) collection folds to the initial value.
    fn fold_call(&self, w: &mut SqlWriter, fold: &SqlExpr) -> Result<(), Refusal> {
        let SqlExpr::FoldCall { source, lambda, init, .. } = fold else {
            return Err(Refusal::illegal_state("fold_call is handed a FoldCall"));
        };
        if lambda.params.len() != 2 {
            return Err(Refusal::illegal_state(format!(
                "a fold's lambda takes (element, accumulator): {}",
                java_text::strings(&lambda.params)
            )));
        }
        w.append("{\"$reduce\": {\"input\": {\"$ifNull\": [").expr(self, source, 0)?;
        w.append(", []]}, \"initialValue\": ").expr(self, init, 0)?.append(", \"in\": ");
        self.with_lambda(w, lambda, &["this".to_owned(), "value".to_owned()], &of(&lambda.body))?;
        w.append("}}");
        Ok(())
    }

    /// A JSON object: alternating literal keys and values, as a document.
    fn json_object(&self, w: &mut SqlWriter, kv: &[SqlExpr]) -> Result<(), Refusal> {
        w.append("{");
        let mut i = 0;
        while i + 1 < kv.len() {
            if i > 0 {
                w.append(", ");
            }
            let SqlExpr::StringLit { value: key, .. } = &kv[i] else {
                return Err(Refusal::capability(
                    "a JSON object with a computed key reached MongoDb: a document's field names are spelled",
                ));
            };
            w.append(&json::key(key)?).append(": ").expr(self, &kv[i + 1], 0)?;
            i += 2;
        }
        w.append("}");
        Ok(())
    }

    fn json_array(&self, w: &mut SqlWriter, elements: &[SqlExpr]) -> Result<(), Refusal> {
        w.append("[").list(self, elements)?.append("]");
        Ok(())
    }

    fn json_array_agg(
        &self,
        w: &mut SqlWriter,
        value: &SqlExpr,
        order_keys: &[JsonArrayAggKey],
    ) -> Result<(), Refusal> {
        let _ = (w, value, order_keys);
        Err(Refusal::illegal_state("a JSON array aggregate reached the expression writer outside a group stage"))
    }

    /// A collection value reduced by an aggregate: its non-null elements, as the group reads reduce.
    fn reduce_collection(
        &self,
        w: &mut SqlWriter,
        reducer: AggFn,
        collection: &SqlExpr,
        extras: &[SqlExpr],
    ) -> Result<(), Refusal> {
        let non_null = pc(move |d, x| aggregates::non_null(d, x, &of(collection)));
        aggregates::array_reduction(self, w, reducer, &non_null, extras, false)
    }

    /// `needle` in a collection value: null when the needle is null (SQL's answer); a list parameter is bound whole,
    /// as one array.
    fn membership(&self, w: &mut SqlWriter, needle: &SqlExpr, collection: &SqlExpr) -> Result<(), Refusal> {
        if let Some(p) = PlanParam::of(collection) {
            let body = pc(move |_, x| {
                let bind = list_bind(&p)?;
                x.append(&["{\"$in\": [", G0, ", "].concat()).bind_as(MARKER, bind).append("]}");
                Ok(())
            });
            return json::guarded(self, w, std::slice::from_ref(needle), &body);
        }
        json::guarded_op(self, w, "$in", &[needle.clone(), collection.clone()])
    }

    // ---- functions (src/mongodb/functions.rs)

    fn call(
        &self,
        w: &mut SqlWriter,
        r#fn: SqlFn,
        args: &[SqlExpr],
        r#type: &TypeFact,
        parent_prec: i32,
    ) -> Result<(), Refusal> {
        functions::call(self, w, r#fn, args, r#type, parent_prec)
    }

    /// `$round` rounds half to even, Pure's banker's rounding.
    fn round_half_even(&self, w: &mut SqlWriter, a: &[SqlExpr]) -> Result<(), Refusal> {
        functions::round_half_even(self, w, a)
    }

    fn bit_op(&self, w: &mut SqlWriter, r#fn: SqlFn, a: &[SqlExpr]) -> Result<(), Refusal> {
        functions::bit_op(self, w, r#fn, a)
    }

    /// The database's own hash: the hashed index key, a 64-bit integer.
    fn hash_signed(&self, w: &mut SqlWriter, a: &[SqlExpr]) -> Result<(), Refusal> {
        json::op1(self, w, "$toHashedIndexKey", crate::refusal::arg(a, 0)?)
    }

    fn split_part_call(&self, w: &mut SqlWriter, args: &[SqlExpr]) -> Result<(), Refusal> {
        functions::split_part(self, w, args, true)
    }

    /// `exists`: any element true, false over the empty (or null) list.
    fn list_exists(&self, w: &mut SqlWriter, args: &[SqlExpr]) -> Result<(), Refusal> {
        functions::quantified(self, w, args, "$anyElementTrue")
    }

    /// `forAll`: all elements true, true over the empty (or null) list.
    fn list_for_all(&self, w: &mut SqlWriter, args: &[SqlExpr]) -> Result<(), Refusal> {
        functions::quantified(self, w, args, "$allElementsTrue")
    }

    fn all_distinct(&self, w: &mut SqlWriter, args: &[SqlExpr]) -> Result<(), Refusal> {
        json::guarded(
            self,
            w,
            args,
            &text(["{\"$eq\": [{\"$size\": ", G0, "}, {\"$size\":", " {\"$setUnion\": [", G0, "]}}]}"].concat()),
        )
    }

    fn list_call(&self, w: &mut SqlWriter, r#fn: SqlFn, args: &[SqlExpr]) -> Result<(), Refusal> {
        functions::list_call(self, w, r#fn, args)
    }

    fn unnest_projection(&self, w: &mut SqlWriter, args: &[SqlExpr]) -> Result<(), Refusal> {
        let _ = (w, args);
        Err(Refusal::capability("an unnest reached MongoDb in expression position: an $unwind stage is not placed yet"))
    }

    /// `struct_insert(struct, name, value)`: the document with the field merged in.
    fn struct_insert(&self, w: &mut SqlWriter, args: &[SqlExpr]) -> Result<(), Refusal> {
        let SqlExpr::StringLit { value: name, .. } = crate::refusal::arg(args, 1)? else {
            return Err(Refusal::capability("a struct insert with a computed field name reached MongoDb"));
        };
        w.append("{\"$mergeObjects\": [").expr(self, crate::refusal::arg(args, 0)?, 0)?.append(", {");
        w.append(&json::key(name)?).append(": ").expr(self, crate::refusal::arg(args, 2)?, 0)?.append("}]}");
        Ok(())
    }

    /// A BSON value is the variant.
    fn variant_construct(&self, w: &mut SqlWriter, a: &[SqlExpr]) -> Result<(), Refusal> {
        w.expr(self, crate::refusal::arg(a, 0)?, 0)?;
        Ok(())
    }

    /// An array value's elements are the array itself.
    fn variant_elements(&self, w: &mut SqlWriter, args: &[SqlExpr]) -> Result<(), Refusal> {
        w.expr(self, crate::refusal::arg(args, 0)?, 0)?;
        Ok(())
    }

    fn variant_get(&self, w: &mut SqlWriter, args: &[SqlExpr]) -> Result<(), Refusal> {
        functions::variant_get(self, w, args)
    }

    // ---- aggregates (src/mongodb/aggregates.rs)

    fn reducer(&self, w: &mut SqlWriter, r: &Reducer) -> Result<(), Refusal> {
        aggregates::reducer(self, w, r)
    }

    // ---- names and types

    /// The BSON type name a cast converts to, a refusal where MongoDB has no such type.
    fn scalar_type_name(&self, s: Scalar) -> Result<String, Refusal> {
        Ok(match s {
            Scalar::Boolean => "bool",
            Scalar::Integer | Scalar::Bigint => "long",
            Scalar::Double => "double",
            Scalar::Varchar | Scalar::TemporalText => "string",
            Scalar::Date | Scalar::Timestamp => "date",
            // a 128-bit integer is a Decimal128 here: 34 exact digits, the one type past 64 bits
            Scalar::Hugeint => "decimal",
            Scalar::Timestamptz => {
                return Err(Refusal::capability(
                    "a cast to a zoned timestamp reached MongoDb, whose dates are UTC instants",
                ));
            }
            Scalar::Time => {
                return Err(Refusal::capability("a cast to a time of day reached MongoDb, which has no time type"));
            }
            Scalar::Json => return Err(Refusal::illegal_state("a cast to JSON is the value itself on MongoDb")),
            Scalar::DecimalText => return Err(Refusal::capability("a cast to a decimal's text reached MongoDb")),
        }
        .to_owned())
    }

    fn struct_type_name(&self, fields: &[StructField]) -> Result<String, Refusal> {
        let _ = fields;
        Err(Refusal::capability("a cast to a struct type reached MongoDb, whose documents are untyped"))
    }

    /// `$dateToString`'s codes: the date and time parts to the millisecond; a name or a 12-hour part has none.
    fn format_text(&self, parts: &[DateFmt]) -> Result<String, Refusal> {
        texts::format_text(parts)
    }

    /// The kind names `call`'s JSON_TYPE answers.
    fn json_kind_name(&self, kind: JsonKind) -> Result<String, Refusal> {
        Ok(match kind {
            JsonKind::Object => "OBJECT",
            JsonKind::Array => "ARRAY",
            JsonKind::Null => "NULL",
            JsonKind::String => "STRING",
            JsonKind::Integer | JsonKind::UnsignedInteger => "INTEGER",
            JsonKind::Float => "FLOAT",
            JsonKind::Boolean => "BOOLEAN",
        }
        .to_owned())
    }

    /// `$type`'s name of a date; no other type's name is compared.
    fn typeof_name(&self, scalar: Scalar) -> Result<String, Refusal> {
        if matches!(scalar, Scalar::Date | Scalar::Timestamp) {
            return Ok("date".to_owned());
        }
        Err(Refusal::capability(format!("a TYPEOF compared with {scalar}'s name reached MongoDb")))
    }

    /// A name is a JSON string: no quoting, no reserved words.
    fn ident(&self, name: &str) -> String {
        name.to_owned()
    }

    fn delimited(&self, name: &str) -> String {
        name.to_owned()
    }

    fn ddl_identifier(&self, name: &str, declared_quoted: bool) -> String {
        let _ = declared_quoted;
        name.to_owned()
    }

    /// A declared column's BSON type, for the store's own reading; a collection declares no columns to the server.
    fn ddl_type(&self, t: &ColumnType) -> Result<String, Refusal> {
        Ok(match t {
            ColumnType::Plain { kind } => match kind {
                ColumnKind::Bigint | ColumnKind::Integer | ColumnKind::Smallint | ColumnKind::Tinyint => "long",
                ColumnKind::Float | ColumnKind::Double | ColumnKind::Real => "double",
                ColumnKind::Bit => "bool",
                ColumnKind::Timestamp | ColumnKind::Date => "date",
                ColumnKind::Time => {
                    return Err(Refusal::capability("a TIME column: MongoDB has no time-of-day type"));
                }
                ColumnKind::TimestampNs => {
                    return Err(Refusal::capability("a TIMESTAMP_NS column: MongoDB's dates keep milliseconds"));
                }
                ColumnKind::Varchar => "string",
                ColumnKind::Json | ColumnKind::Object => "object",
                ColumnKind::Array => "array",
                ColumnKind::Other => "objectId",
                ColumnKind::Distinct => {
                    return Err(Refusal::illegal_state(format!("no DDL spelling for declared column type {kind}")));
                }
            },
            ColumnType::Sized { .. } => "string",
            ColumnType::Scaled { .. } => "decimal",
        }
        .to_owned())
    }

    // ---- DDL and DML: commands

    /// A table is a collection: the `create` and `drop` commands; a schema has no counterpart, nor has a temporary
    /// collection.
    fn render_ddl(&self, ddl: &SqlDdl) -> Result<String, Refusal> {
        match ddl {
            SqlDdl::CreateTable { schema, table, temporary, .. } => {
                if *temporary {
                    return Err(Refusal::capability(
                        "a temporary table reached MongoDb, which has no temporary collection",
                    ));
                }
                Ok(format!("{{\"create\": {}}}", json::string(&ddl_collection(schema.as_deref(), table)?)))
            }
            SqlDdl::DropTable { schema, table, .. } => {
                Ok(format!("{{\"drop\": {}}}", json::string(&ddl_collection(schema.as_deref(), table)?)))
            }
            SqlDdl::CreateSchema { schema, .. } | SqlDdl::DropSchema { schema, .. } => {
                Err(Refusal::capability(format!(
                    "a schema ({schema}) reached MongoDb: a collection's database is the connection's, and has no \
                     schema"
                )))
            }
        }
    }

    /// Rows in: the `insert` command; rows out: `delete` of every document. A copy between collections and a store
    /// write are not shaped.
    fn render_dml(&self, dml: &SqlDml) -> Result<String, Refusal> {
        match dml {
            SqlDml::InsertValues { schema, table, columns, rows, .. } => {
                self.insert(schema.as_deref(), table, columns, rows)
            }
            SqlDml::InsertFromTable { .. } => Err(Refusal::capability(
                "a copy from a table reached MongoDb: an insert from another collection is not shaped yet",
            )),
            SqlDml::DeleteAll { schema, table, .. } => Ok(format!(
                "{{\"delete\": {}, \"deletes\": [{{\"q\": {{}}, \"limit\": 0}}]}}",
                json::string(&ddl_collection(schema.as_deref(), table)?)
            )),
            SqlDml::CreateTempTableAs { .. } => self.temp_table_as(dml)?.into_text(),
            SqlDml::InsertSelect { .. } => Err(Refusal::capability(
                "an insert of a query's rows reached MongoDb: a store write is not shaped on MongoDb",
            )),
            SqlDml::Update { .. } => {
                Err(Refusal::capability("an update reached MongoDb: a store write is not shaped on MongoDb"))
            }
            SqlDml::DeleteWhere { .. } => Err(Refusal::capability(
                "a delete of a condition's rows reached MongoDb: a store write is not shaped on MongoDb",
            )),
            SqlDml::Merge { .. } => {
                Err(Refusal::capability("a merge reached MongoDb: a store write is not shaped on MongoDb"))
            }
        }
    }
}

// ==================================================================
// The dialect as its callers use it
// ==================================================================

impl crate::dialect::SqlDialect for MongoDb {
    fn dialect_name(&self) -> &'static str {
        AnsiSqlRenderer::simple_name(self)
    }

    fn render(&self, query: SqlQuery) -> Result<String, Refusal> {
        AnsiSqlRenderer::render(self, query)
    }

    fn render_inline(&self, query: SqlQuery) -> Result<String, Refusal> {
        ansi::render_inline(self, query)
    }

    fn render_statement(&self, query: SqlQuery) -> Result<RenderedStatement, Refusal> {
        AnsiSqlRenderer::render_statement(self, query)
    }

    fn render_ddl(&self, ddl: &SqlDdl) -> Result<String, Refusal> {
        AnsiSqlRenderer::render_ddl(self, ddl)
    }

    fn render_dml(&self, dml: &SqlDml) -> Result<String, Refusal> {
        AnsiSqlRenderer::render_dml(self, dml)
    }

    fn render_dml_statement(&self, dml: &SqlDml) -> Result<RenderedStatement, Refusal> {
        AnsiSqlRenderer::render_dml_statement(self, dml)
    }

    fn raw(&self, statement: &str, written_for: Option<&str>) -> Result<String, Refusal> {
        ansi::raw(self, statement, written_for)
    }

    fn from_engine_text(&self, statement: &str) -> Result<String, Refusal> {
        AnsiSqlRenderer::from_engine_text(self, statement)
    }

    fn stored_name(&self, name: &str, declared_quoted: bool) -> String {
        AnsiSqlRenderer::stored_name(self, name, declared_quoted)
    }

    fn physical_name(&self, name: &str) -> String {
        AnsiSqlRenderer::physical_name(self, name)
    }

    fn insert_column_name(&self, name: &str, declared_quoted: bool) -> String {
        AnsiSqlRenderer::insert_column_name(self, name, declared_quoted)
    }

    fn reads_stored(&self, t: &ColumnType) -> bool {
        <MongoDb as AnsiSqlRenderer>::reads_stored(t)
    }

    fn table_name_spelled(&self, name: &str, quoting: lite_sql::NameQuoting) -> String {
        AnsiSqlRenderer::table_name(self, name, quoting)
    }

    /// No: an aggregation pipeline, which a `MongoDBConnection` declares (D4).
    fn relational(&self) -> bool {
        false
    }

    /// MongoDB has no temporary collection: a plan's metamodel tables are its pipeline's own documents.
    fn temporary_tables(&self) -> bool {
        false
    }

    /// A dynamic pivot's probe plan reads the keys and then the body, two commands with no transaction across them.
    fn keys_snapshot(&self) -> Result<KeysSnapshot, Refusal> {
        Ok(KeysSnapshot::None)
    }

    /// No column count bounds a document; its 16 MB bounds the answer, refused by the server when exceeded.
    fn result_column_limit(&self) -> Result<Option<i32>, Refusal> {
        Ok(None)
    }
}
