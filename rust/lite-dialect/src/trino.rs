// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! Java's `Trino` (`core/src/main/java/com/legend/sql/dialect/Trino.java`), the Trino 483 execution dialect (its memory
//! connector): the base renderer with every identifier quoted, every integer literal a BIGINT (a bare one is a 32-bit
//! INTEGER whose arithmetic raises at 2^31), `OFFSET` before `LIMIT`, typed `ARRAY(T)` lists with lambdas, `ROW`,
//! `MAP` and `JSON`, an explosion one `UNNEST` in FROM, a list's moments as its rows reduced, the quantiles and the
//! mode from the group's values sorted, banker's ROUND unrolled, and -- as its [`crate::SqlDialect`] -- no temporary
//! table, no keys snapshot and no column limit. Every `SqlFn` and aggregate is decided by an exhaustive match:
//! `src/trino/functions.rs` (`call` and its helpers) and `src/trino/aggregates.rs` (the hooks of the base, the
//! aggregates and windows, the JSON constructors and the date format).
//!
//! Java's `SqlWriter.Piece` is [`P`], a shared closure (as Postgres's): the helpers compose pieces exactly as Java's
//! lambdas do, and what Java evaluates when a piece is made (a type's name, a date part, a string literal) Rust
//! evaluates then too.

use crate::ansi::{self, cast_type_name};
use crate::dialect::KeysSnapshot;
use crate::explosion_markers::{self, is_marker_select, marker_column, null_list, unnest_list};
use crate::java::simple_name;
use crate::refusal::{Refusal, arg};
use crate::renderer::{AnsiSqlRenderer, AnsiState, Lexicon};
use crate::rewriter::SqlRewriter;
use crate::statement::RenderedStatement;
use crate::writer::SqlWriter;
use crate::{build, carrier, shared_passes};
use lite_sql::{
    AggFn, Bound, ColumnKind, ColumnType, DateFmt, Frame, JoinKind, JsonArrayAggKey, JsonKind, Lambda, NameQuoting,
    Origin, Projection, Reducer, Scalar, SortKey, SqlAgg, SqlDdl, SqlDml, SqlExpr, SqlFn, SqlQuery, SqlSelect,
    SqlSource, SqlType, StructField, StructLitField, TypeFact, When,
};
use std::borrow::Cow;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::LazyLock;

mod aggregates;
mod functions;

/// The Trino dialect: one per render, as Java's `new Trino()`. It carries no state across renders.
#[derive(Debug, Default)]
pub struct Trino {
    ansi: AnsiState,
}

impl Trino {
    /// Java's `new Trino()`.
    pub fn new() -> Trino {
        Trino::default()
    }
}

/// Trino's documented reserved keywords (Java's `LEXICON`'s words). Trino quotes every identifier, so the list decides
/// no spelling there.
pub const RESERVED: &[&str] = &[
    "alter",
    "and",
    "as",
    "auto",
    "between",
    "by",
    "case",
    "cast",
    "constraint",
    "create",
    "cross",
    "cube",
    "current_catalog",
    "current_date",
    "current_path",
    "current_role",
    "current_schema",
    "current_time",
    "current_timestamp",
    "current_user",
    "deallocate",
    "delete",
    "describe",
    "distinct",
    "drop",
    "else",
    "end",
    "escape",
    "except",
    "execute",
    "exists",
    "extract",
    "false",
    "for",
    "from",
    "full",
    "group",
    "grouping",
    "having",
    "in",
    "inner",
    "insert",
    "intersect",
    "into",
    "is",
    "join",
    "json_array",
    "json_exists",
    "json_object",
    "json_query",
    "json_table",
    "json_value",
    "left",
    "like",
    "listagg",
    "localtime",
    "localtimestamp",
    "natural",
    "normalize",
    "not",
    "null",
    "on",
    "or",
    "order",
    "outer",
    "prepare",
    "recursive",
    "right",
    "rollup",
    "select",
    "skip",
    "table",
    "then",
    "trim",
    "true",
    "uescape",
    "union",
    "unnest",
    "using",
    "values",
    "when",
    "where",
    "with",
];

/// Java's `Trino.LEXICON`: `"` and [`RESERVED`].
static LEXICON: LazyLock<Lexicon> = LazyLock::new(|| Lexicon::new('"', RESERVED));

/// Java's `Trino.CAPS`: typed native arrays with lambdas -- the strategies stand aside, as on Databricks; the dialect
/// writes the static pivot and the as-of join itself (Trino has neither); FULL OUTER is native.
pub const CAPS: carrier::Caps =
    carrier::Caps { native_lists: true, correlated_explode: true, json_carrier: true, native_full_outer: true };

// ==================================================================
// Pieces and the type facts the arms read
// ==================================================================

/// Java's `SqlWriter.Piece`: SQL that writes itself, shareable among the helpers that write it.
pub type P<'a> = Rc<dyn Fn(&mut SqlWriter) -> Result<(), Refusal> + 'a>;

fn pc<'a>(f: impl Fn(&mut SqlWriter) -> Result<(), Refusal> + 'a) -> P<'a> {
    Rc::new(f)
}

/// A piece of fixed text.
fn text<'a>(s: impl Into<String>) -> P<'a> {
    let s = s.into();
    pc(move |w| {
        w.append(&s);
        Ok(())
    })
}

/// Java's `typeOf(e)`: the type a node is typed as, or none.
fn type_of(e: &SqlExpr) -> Option<&SqlType> {
    fact_type(e.r#type())
}

fn fact_type(f: &TypeFact) -> Option<&SqlType> {
    match f {
        TypeFact::Typed { r#type, .. } => Some(r#type),
        _ => None,
    }
}

/// Java's `listElement(e)`: the element type of a list, or none for a value that is no list.
fn list_element(e: &SqlExpr) -> Option<&SqlType> {
    match type_of(e) {
        Some(SqlType::Array { element }) => Some(element),
        _ => None,
    }
}

fn is_scalar(e: &SqlExpr, s: Scalar) -> bool {
    matches!(type_of(e), Some(SqlType::Scalar(t)) if *t == s)
}

/// Java's `isJson(e)`.
fn is_json(e: &SqlExpr) -> bool {
    is_scalar(e, Scalar::Json)
}

/// Java's `isText(e)`.
fn is_text(e: &SqlExpr) -> bool {
    is_scalar(e, Scalar::Varchar)
}

fn is_struct(t: Option<&SqlType>) -> bool {
    matches!(t, Some(SqlType::Struct { .. }))
}

/// Java's `isRaise(e)`: a call of ERROR.
fn is_raise(e: &SqlExpr) -> bool {
    matches!(e, SqlExpr::Call { r#fn: SqlFn::Error, .. })
}

/// Whether both operands are ROWs (structs): Java's `rows(a)`.
fn rows(a: &[SqlExpr]) -> bool {
    a.len() == 2 && is_struct(type_of(&a[0])) && is_struct(type_of(&a[1]))
}

/// `x` as Java's `(SqlExpr.Lambda) x` reads it.
fn lambda_arg(x: &SqlExpr) -> Result<&Lambda, Refusal> {
    match x {
        SqlExpr::Lambda(l) => Ok(l),
        other => Err(Refusal::class_cast(simple_name(other), "Lambda")),
    }
}

/// Java's `SqlExpr.Column.param(name, collection)`: a reference typed as the collection's element (nullable), or an
/// untyped derived one when the collection is no typed list.
fn column_param(name: &str, collection: &SqlExpr) -> SqlExpr {
    match list_element(collection) {
        Some(element) => build::typed_column(None, name, element.clone(), true, Origin::Derived),
        None => build::derived(None, name),
    }
}

/// `params.get(i)`, refused past the end as Java's list read words it.
fn param_name(params: &[String], i: usize) -> Result<&str, Refusal> {
    params.get(i).map(String::as_str).ok_or_else(|| Refusal {
        class: Refusal::INDEX_OUT_OF_BOUNDS,
        message: format!("Index {i} out of bounds for length {}", params.len()),
    })
}

/// Java's `p.outputName()`: its alias, else a column's (or a stored read's) name.
fn output_name(p: &Projection) -> Option<String> {
    if let Some(alias) = &p.alias {
        return Some(alias.clone());
    }
    match &p.expr {
        SqlExpr::Column(c) => Some(c.name.clone()),
        SqlExpr::StoredRead { column, .. } => Some(column.name.clone()),
        _ => None,
    }
}

/// The statement's own ROW columns as their JSON text (Java's `rowsAsJson`): the driver hands a ROW as its own class,
/// which the platform reads through no interface, and JSON text the typed reader decodes by the column's slot. Each
/// is cast to its declared ROW first, whose named fields are an object's keys. The select as it came when none is.
fn rows_as_json(s: &SqlSelect) -> Result<Cow<'_, SqlSelect>, Refusal> {
    // a HashMap's put: a repeated name's last type
    let labels: HashMap<&str, &SqlType> = s.outputs.iter().map(|o| (o.name.as_str(), &o.r#type)).collect();
    let mut ps = Vec::with_capacity(s.projections.len());
    let mut changed = false;
    for p in &s.projections {
        let name = output_name(p);
        let label = match name.as_deref().and_then(|n| labels.get(n)) {
            Some(t) => Some(*t),
            None => type_of(&p.expr),
        };
        // a ROW itself (a struct carried as text, a metamodel element by its name, is already text)
        if let Some(st @ SqlType::Struct { .. }) = label
            && is_struct(type_of(&p.expr))
            && !matches!(p.expr, SqlExpr::StringLit { .. })
        {
            let cast = build::cast_conform(
                build::cast_conform(p.expr.clone(), st.clone(), false),
                SqlType::Scalar(Scalar::Json),
                false,
            );
            ps.push(build::projection(cast, name, p.out.clone())?);
            changed = true;
        } else {
            ps.push(p.clone());
        }
    }
    if !changed {
        return Ok(Cow::Borrowed(s));
    }
    Ok(Cow::Owned(build::select(SqlSelect { projections: ps, ..s.clone() })?))
}

// ==================================================================
// The dialect's own helpers (Java's private methods)
// ==================================================================

impl Trino {
    /// `e`, written at `prec`, as a piece.
    fn ex<'a>(&'a self, e: &'a SqlExpr, prec: i32) -> P<'a> {
        pc(move |w| {
            w.expr(self, e, prec)?;
            Ok(())
        })
    }

    /// `a`'s `i`-th argument, written at `prec`, as a piece: read when written (Java's `w -> w.expr(a.get(i), prec)`).
    fn arg_ex<'a>(&'a self, a: &'a [SqlExpr], i: usize, prec: i32) -> P<'a> {
        pc(move |w| {
            w.expr(self, arg(a, i)?, prec)?;
            Ok(())
        })
    }

    /// A list argument: the empty collection (a bare NULL, untyped) as an empty array, whose `unknown` elements coerce
    /// to any type (Trino refuses a bare NULL where an array is required). Java's `list(xs)`.
    fn list<'a>(&'a self, xs: &'a SqlExpr) -> P<'a> {
        if matches!(xs, SqlExpr::NullLit { .. }) {
            return text("CAST(NULL AS ARRAY(UNKNOWN))");
        }
        self.ex(xs, 0)
    }

    /// `xs` less its NULL elements (Java's `present(xs)`).
    fn present<'a>(&'a self, xs: &'a SqlExpr) -> P<'a> {
        let list = self.list(xs);
        pc(move |w| {
            w.append("filter(").piece(&*list)?.append(", \"__x\" -> \"__x\" IS NOT NULL)");
            Ok(())
        })
    }

    /// A lambda parameter as its references spell it (Java's `param(name)`).
    fn param(&self, name: &str) -> Result<String, Refusal> {
        self.column_ref(&lite_sql::Column {
            table: None,
            name: name.to_owned(),
            r#type: TypeFact::Unknown,
            origin: Some(Origin::Derived),
        })
    }

    /// `xs`'s 1-based positions, `[1, ..., n]` (`[]` for an empty list, NULL for a NULL one).
    fn positions<'a>(&'a self, xs: &'a SqlExpr) -> P<'a> {
        let list = self.list(xs);
        pc(move |w| {
            w.append("slice(sequence(1, cardinality(").piece(&*list)?.append(") + 1), 1, cardinality(");
            w.piece(&*list)?.append("))");
            Ok(())
        })
    }

    /// `(SELECT agg FROM UNNEST(xs) AS u(e))`: a list reduced as rows (a correlated scalar subquery); an empty or NULL
    /// list reduces over no rows (Java's `overRows`).
    fn over_rows<'a>(&'a self, xs: &'a SqlExpr, f: AggFn, extras: &[SqlExpr]) -> P<'a> {
        let e = if list_element(xs).is_none() { build::derived(None, "__e") } else { column_param("__e", xs) };
        let mut args = vec![e];
        args.extend(extras.iter().cloned());
        let r = build::reducer(f, args, false, Vec::new());
        let list = self.list(xs);
        pc(move |w| {
            w.append("(SELECT ");
            self.reducer(w, &r)?;
            w.append(" FROM UNNEST(").piece(&*list)?.append(") AS \"__v\"(");
            let e = self.param("__e")?;
            w.append(&e).append("))");
            Ok(())
        })
    }

    /// A value as JSON (Java's `toVariant`): text is a JSON string; a list, map or struct converts (a ROW of named
    /// fields is an object); a scalar casts.
    fn to_variant<'a>(&'a self, x: &'a SqlExpr) -> P<'a> {
        if is_json(x) {
            return self.ex(x, 0);
        }
        pc(move |w| {
            w.append("CAST(").expr(self, x, 0)?.append(" AS JSON)");
            Ok(())
        })
    }

    /// `UNNEST(xs, ys) AS alias(a, b)` (Java's `unnest`): a NULL list as an empty one of its element's type (no row
    /// either way), a list of rows each wrapped in a row of one field; `ordinal`: `WITH ORDINALITY`, the position a
    /// column `__ord`.
    fn unnest<'a>(&'a self, marker: &'a SqlSelect, alias: &'a str, ordinal: bool) -> P<'a> {
        pc(move |w| {
            w.append("UNNEST(");
            w.join(&marker.projections, ", ", |v, p| {
                let xs = unnest_list(&p.expr)?;
                let declared = match &p.out {
                    Some(out) => Some(&out.r#type),
                    None => type_of(&p.expr),
                };
                let element = if null_list(xs) || list_element(xs).is_none() { declared } else { list_element(xs) };
                let array: P<'_> = if null_list(xs) {
                    pc(move |a| {
                        match declared {
                            None => a.append("ARRAY[]"),
                            Some(d) => {
                                let t = self.array_type_name(&cast_type_name(self, d)?);
                                a.append(&format!("CAST(ARRAY[] AS {t})"))
                            }
                        };
                        Ok(())
                    })
                } else if list_element(xs).is_none()
                    && let Some(d) = declared
                {
                    // a list of no element type (Pure's []) as the explosion's declared one: its rows' fields read
                    pc(move |a| {
                        a.append("CAST(").expr(self, xs, 0)?;
                        let t = self.array_type_name(&cast_type_name(self, d)?);
                        a.append(&format!(" AS {t})"));
                        Ok(())
                    })
                } else {
                    // a list of no type at all is Trino's own ARRAY[], an array(unknown): no rows
                    self.ex(xs, 0)
                };
                if is_struct(element) {
                    // UNNEST spreads a ROW element over one column per field: each wrapped in a one-field row stays
                    // one column, the row itself
                    v.append("transform(").piece(&*array)?.append(", \"__e\" -> ROW(\"__e\"))");
                } else {
                    v.piece(&*array)?;
                }
                Ok(())
            })?;
            let a = self.alias_ident(alias);
            w.append(if ordinal { ") WITH ORDINALITY AS " } else { ") AS " }).append(&a).append("(");
            let columns =
                marker.projections.iter().map(|p| Ok(self.alias_ident(&marker_column(p)?))).collect::<Result<
                    Vec<_>,
                    Refusal,
                >>()?;
            w.append(&columns.join(", "));
            if ordinal {
                let ord = self.alias_ident("__ord");
                w.append(&format!(", {ord})"));
            } else {
                w.append(")");
            }
            Ok(())
        })
    }

    /// A struct literal as a typed ROW (Java's `structLit(writer, st, siblings)`): each field typed by its value, else
    /// the slot its builder declared, else a sibling row's field of that name.
    fn struct_lit_with(
        &self,
        w: &mut SqlWriter,
        fields: &[StructLitField],
        siblings: &HashMap<&str, &SqlType>,
    ) -> Result<(), Refusal> {
        w.append("CAST(ROW(").join(fields, ", ", |w, f| {
            w.expr(self, &f.value, 0)?;
            Ok(())
        })?;
        w.append(") AS ROW(");
        w.join(fields, ", ", |w, f| {
            let mut t = type_of(&f.value);
            if t.is_none() {
                // an absent optional property: the slot its builder declared
                t = f.declared.as_ref();
            }
            if t.is_none() {
                t = siblings.get(f.name.as_str()).copied();
            }
            let Some(t) = t else {
                return Err(Refusal::capability("a struct field of unknown type reached Trino: a ROW's fields are typed"));
            };
            let name = self.ident(&f.name);
            let ty = cast_type_name(self, t)?;
            w.append(&name).append(" ").append(&ty);
            Ok(())
        })?;
        w.append("))");
        Ok(())
    }

    /// A branch that IS the raise, cast to the CASE's own type (Java's `branch`).
    fn branch<'a>(&'a self, e: &'a SqlExpr, slot: Option<&SqlType>) -> Result<P<'a>, Refusal> {
        if is_raise(e)
            && let Some(slot) = slot
        {
            let t = cast_type_name(self, slot)?;
            return Ok(pc(move |w| {
                w.append("CAST(").expr(self, e, 0)?.append(" AS ").append(&t).append(")");
                Ok(())
            }));
        }
        Ok(self.ex(e, 0))
    }
}

/// A lambda over the element alone: Trino's take no index (Java's `elementOnly`).
fn element_only(l: &Lambda, f: SqlFn) -> Result<(), Refusal> {
    if l.params.len() > 1 {
        return Err(Refusal::capability(format!(
            "{f} with an element index reached Trino: its lambdas take the element alone"
        )));
    }
    Ok(())
}

// ==================================================================
// The renderer (Java's overrides of AnsiSqlRenderer)
// ==================================================================

impl AnsiSqlRenderer for Trino {
    fn lexicon(&self) -> &Lexicon {
        &LEXICON
    }

    fn ansi_state(&self) -> &AnsiState {
        &self.ansi
    }

    fn simple_name(&self) -> &'static str {
        "Trino"
    }

    /// The engine's H2 text is refused here: Trino has no translation of it.
    fn from_engine_text(&self, statement: &str) -> Result<String, Refusal> {
        let _ = statement;
        Err(Refusal::capability("the engine's H2 text reached Trino, which has no translation of it"))
    }

    /// Native lists (the carrier strategies stand aside); a static pivot as a grouped CASE aggregation and an as-of
    /// join as a window pick; the star exclusion expanded; every list explosion an UNNEST in FROM; the substring start
    /// clamp; averages delivered as DOUBLE; QUALIFY as a wrapping subselect; a constant GROUP BY or ORDER BY key as a
    /// typed expression; the root's DECIMAL(p,s) and HUGEINT columns cast to their declared types.
    fn passes(&self) -> Vec<Box<dyn SqlRewriter>> {
        vec![
            Box::new(carrier::CarrierStrategies::new(CAPS)),
            Box::new(shared_passes::PivotAsGroupBy),
            Box::new(shared_passes::AsOfAsPick),
            Box::new(shared_passes::StarExceptToColumns),
            Box::new(explosion_markers::LateralUnnestAsMarker),
            Box::new(explosion_markers::SelectListUnnestAsLateral::default()),
            Box::new(explosion_markers::StarOverExplosionUnwrapped),
            Box::new(crate::passes::CheckedDefectsToLists),
            Box::new(crate::passes::SubstringClamp),
            Box::new(shared_passes::AggregatesDeliverDouble::new(&[AggFn::Avg])),
            Box::new(shared_passes::QualifyToSubselect::by_outputs()),
            Box::new(crate::passes::ConstantKeysAsExpressions::default()),
            Box::new(shared_passes::RootNumericTypes),
        ]
    }

    /// `OFFSET n` before `LIMIT m`: Trino's order. A bare explosion MARKER (a FROM-less select of UNNESTs, the lists a
    /// query answers) reads its elements from one `UNNEST ... WITH ORDINALITY`, in their lists' order.
    fn select(&self, w: &mut SqlWriter, s: &SqlSelect, depth: usize) -> Result<(), Refusal> {
        if is_marker_select(s) {
            w.append("SELECT ").join(&s.projections, ", ", |w, p| {
                let column = self.alias_ident(&marker_column(p)?);
                // a ROW element as its JSON text (rowsAsJson); a struct carried as text (a metamodel element by its
                // name) is text already
                let xs = unnest_list(&p.expr)?;
                if depth == 0
                    && let Some(out) = &p.out
                    && let st @ SqlType::Struct { .. } = &out.r#type
                    && (list_element(xs).is_none() || is_struct(list_element(xs)))
                {
                    let t = cast_type_name(self, st)?;
                    w.append(&format!("CAST(CAST({column} AS {t}) AS JSON) AS {column}"));
                } else {
                    w.append(&column);
                }
                Ok(())
            })?;
            w.append(" FROM ");
            self.unnest(s, "__unnest", true)(w)?;
            self.nl(w, depth);
            let ord = self.alias_ident("__ord");
            w.append("ORDER BY ").append(&ord);
            return Ok(());
        }
        let s = if depth == 0 { rows_as_json(s)? } else { Cow::Borrowed(s) };
        let Some(offset) = s.offset else {
            return ansi::select(self, w, &s, depth);
        };
        let inner = build::with_offset(build::with_limit(s.as_ref().clone(), None)?, None)?;
        ansi::select(self, w, &inner, depth)?;
        self.nl(w, depth);
        w.append("OFFSET ").append_long(offset);
        if let Some(limit) = s.limit {
            self.nl(w, depth);
            w.append("LIMIT ").append_long(limit);
        }
        Ok(())
    }

    /// An explosion MARKER is one `UNNEST` of its lists, which Trino pads to the longest with NULL; a lateral one joins
    /// its row (`CROSS JOIN UNNEST`, or `LEFT JOIN UNNEST ... ON TRUE` keeping an empty list's row).
    fn source(&self, w: &mut SqlWriter, src: &SqlSource, depth: usize) -> Result<(), Refusal> {
        if let SqlSource::Join { left, right, kind: kind @ (JoinKind::CrossLateral | JoinKind::LeftLateral), on, .. } =
            src
            && let SqlSource::Subselect { inner, alias, .. } = right.as_ref()
            && let SqlQuery::Select(m) = inner.as_ref()
            && is_marker_select(m)
        {
            if let Some(on) = on
                && !matches!(on.as_ref(), SqlExpr::BoolLit { value: true, .. })
            {
                return Err(Refusal::capability("a lateral explosion with a join condition reached Trino"));
            }
            self.source(w, left, depth)?;
            self.nl(w, depth);
            let left_lateral = *kind == JoinKind::LeftLateral;
            w.append(if left_lateral { "LEFT JOIN " } else { "CROSS JOIN " });
            self.unnest(m, alias, false)(w)?;
            if left_lateral {
                w.append(" ON TRUE");
            }
            return Ok(());
        }
        if let SqlSource::Subselect { inner, alias, .. } = src
            && let SqlQuery::Select(m) = inner.as_ref()
            && is_marker_select(m)
        {
            return self.unnest(m, alias, false)(w);
        }
        if let SqlSource::Join { left, right, kind: JoinKind::LeftLateral, on: None, .. } = src {
            self.source(w, left, depth)?;
            self.nl(w, depth);
            w.append("LEFT JOIN LATERAL ");
            self.source(w, right, depth)?;
            w.append(" ON TRUE");
            return Ok(());
        }
        ansi::source(self, w, src, depth)
    }

    fn star_except_keyword(&self) -> Result<String, Refusal> {
        Err(Refusal::capability(
            "SELECT * EXCEPT reached Trino, which has no star exclusion; the column list is expanded upstream \
             (StarExceptToColumns)",
        ))
    }

    fn row_order_column(&self) -> Result<String, Refusal> {
        Err(Refusal::capability("row order reached Trino, which has no row-order column"))
    }

    fn as_of_join_clause(&self) -> Result<String, Refusal> {
        Err(Refusal::illegal_state("an as-of join reached the join clause: Trino writes it as a pick (AsOfAsPick)"))
    }

    fn pivot_source(&self, w: &mut SqlWriter, pivot: &SqlSource, depth: usize) -> Result<(), Refusal> {
        let _ = (w, pivot, depth);
        Err(Refusal::illegal_state(
            "a pivot reached Trino's writer: PivotAsGroupBy writes it as a grouped aggregation (a dynamic pivot is \
             discovered first)",
        ))
    }

    // ---- lexical: identifiers, literals, types

    /// Every identifier quoted; a double-quoted declaration (`"date"`) is already its own spelling.
    fn ident(&self, name: &str) -> String {
        if name.len() > 1
            && name.starts_with('"')
            && name.ends_with('"')
            && !name[1..name.len() - 1].replace("\"\"", "").contains('"')
        {
            return name.to_owned();
        }
        self.delimited(name)
    }

    fn array_type_name(&self, element: &str) -> String {
        format!("ARRAY({element})")
    }

    fn map_type_name(&self, key: &str, value: &str) -> String {
        format!("MAP({key}, {value})")
    }

    fn struct_type_name(&self, fields: &[StructField]) -> Result<String, Refusal> {
        let fields = ansi::struct_fields(self, fields)?;
        let mut sb = String::from("ROW(");
        for (i, (name, t)) in fields.iter().enumerate() {
            if i > 0 {
                sb.push_str(", ");
            }
            sb.push_str(name);
            sb.push(' ');
            sb.push_str(t);
        }
        sb.push(')');
        Ok(sb)
    }

    /// A timestamp literal keeps its digits; its date and time are apart by a space (a `T` is refused).
    fn timestamp_lit(&self, iso: &str) -> String {
        format!("TIMESTAMP '{}'", iso.replacen('T', " ", 1))
    }

    /// Alias-less projections label EXPLICITLY from the declared output: Trino labels an expression `_colN`.
    fn implicit_label(&self, p: &Projection) -> Result<Option<String>, Refusal> {
        match &p.out {
            Some(out) => Ok(Some(self.alias_ident(&out.name))),
            None => ansi::implicit_label(self, p),
        }
    }

    /// A value that can only be NULL (a NULL literal, a raise) is spelled in its declared slot's type: Trino types a
    /// bare NULL `unknown`.
    fn projection(&self, w: &mut SqlWriter, p: &Projection) -> Result<(), Refusal> {
        if let Some(out) = &p.out
            && (matches!(p.expr.r#type(), TypeFact::Bottom)
                || is_raise(&p.expr)
                || matches!(p.expr, SqlExpr::NullLit { .. }))
        {
            w.append("CAST(").expr(self, &p.expr, 0)?.append(" AS ");
            let t = cast_type_name(self, &out.r#type)?;
            w.append(&t).append(")");
            let label = match &p.alias {
                Some(alias) => Some(self.alias_ident(alias)),
                None => self.implicit_label(p)?,
            };
            if let Some(label) = label {
                w.append(" AS ").append(&label);
            }
            return Ok(());
        }
        ansi::projection(self, w, p)
    }

    /// A branch that IS the raise is cast to the CASE's own type: `fail` is `unknown`.
    fn case_expr(
        &self,
        w: &mut SqlWriter,
        whens: &[When],
        otherwise: Option<&SqlExpr>,
        r#type: &TypeFact,
    ) -> Result<(), Refusal> {
        let slot = fact_type(r#type);
        w.append("CASE");
        for wh in whens {
            w.append(" WHEN ").expr(self, &wh.condition, 0)?.append(" THEN ");
            let b = self.branch(&wh.then, slot)?;
            b(w)?;
        }
        if let Some(o) = otherwise {
            w.append(" ELSE ");
            let b = self.branch(o, slot)?;
            b(w)?;
        }
        w.append(" END");
        Ok(())
    }

    /// A Float literal is its digits, a DECIMAL; past a DECIMAL's 38 digits Trino refuses it: it is the double's
    /// shortest text instead.
    fn float_literal(&self, v: f64) -> Result<String, Refusal> {
        let plain = crate::java::plain_float(v)?;
        let digits = plain.replace(['-', '.'], "");
        Ok(if digits.trim_start_matches('0').len() > 38 { lite_javacompat::double_to_string(v) } else { plain })
    }

    /// An integer past a signed BIGINT: a bare one is refused; a DECIMAL's holds 38 digits.
    fn huge_int_literal(&self, v: &lite_sql::BigDecimal) -> String {
        format!("DECIMAL '{}'", crate::java::big_decimal_plain(v.as_str()))
    }

    fn expr(&self, w: &mut SqlWriter, e: &SqlExpr, parent_prec: i32) -> Result<(), Refusal> {
        match e {
            // a BIGINT: a bare literal is a 32-bit INTEGER, whose arithmetic raises at 2^31
            SqlExpr::IntLit { value, .. } => {
                w.append(&format!("BIGINT '{value}'"));
                Ok(())
            }
            // past a DECIMAL's 38 digits a decimal literal is refused; DuckDB reads one as a DOUBLE, and so does this
            SqlExpr::DecimalLit { value, .. } if crate::typing::big_decimal_precision(value) > 38 => {
                w.append(&format!("DOUBLE '{}'", crate::java::big_decimal_plain(value.as_str())));
                Ok(())
            }
            SqlExpr::OrderedListAgg { value, order_by, .. } => {
                w.append("array_agg(").expr(self, value, 0)?.append(" ORDER BY ").expr(self, order_by, 0)?;
                w.append(" NULLS LAST)");
                Ok(())
            }
            other => ansi::expr(self, w, other, parent_prec),
        }
    }

    // ---- lists: native arrays with lambdas

    /// A list of rows types each field by the first row whose field is typed: a missing property is a NULL of no
    /// type, and a ROW's fields are typed.
    fn array_lit(&self, w: &mut SqlWriter, elements: &[SqlExpr]) -> Result<(), Refusal> {
        if !elements.is_empty() && elements.iter().all(|e| matches!(e, SqlExpr::StructLit { .. })) {
            let mut types: HashMap<&str, &SqlType> = HashMap::new();
            for e in elements {
                if let SqlExpr::StructLit { fields, .. } = e {
                    for f in fields {
                        if let Some(t) = type_of(&f.value) {
                            types.entry(f.name.as_str()).or_insert(t);
                        }
                    }
                }
            }
            w.append("ARRAY[").join(elements, ", ", |w, e| {
                let SqlExpr::StructLit { fields, .. } = e else { unreachable!("every element is a struct literal") };
                self.struct_lit_with(w, fields, &types)
            })?;
            w.append("]");
            return Ok(());
        }
        w.append("ARRAY[").list(self, elements)?.append("]");
        Ok(())
    }

    fn struct_lit(&self, w: &mut SqlWriter, fields: &[StructLitField]) -> Result<(), Refusal> {
        self.struct_lit_with(w, fields, &HashMap::new())
    }

    /// A field by name; a field NUMBER (a zip's pair, read by position) a row subscript (`r[1]`).
    fn struct_get(&self, w: &mut SqlWriter, source: &SqlExpr, field: &str, r#type: &TypeFact) -> Result<(), Refusal> {
        let _ = r#type;
        if !field.is_empty() && field.bytes().all(|b| b.is_ascii_digit()) {
            w.append("(").expr(self, source, 8)?.append(&format!(")[{field}]"));
            return Ok(());
        }
        w.append("(").expr(self, source, 8)?.append(").");
        let f = self.ident(field);
        w.append(&f);
        Ok(())
    }

    fn lambda(&self, w: &mut SqlWriter, l: &Lambda) -> Result<(), Refusal> {
        let head = if l.params.len() == 1 {
            self.param(&l.params[0])?
        } else {
            let ps = l.params.iter().map(|p| self.param(p)).collect::<Result<Vec<_>, _>>()?;
            format!("({})", ps.join(", "))
        };
        w.append(&head).append(" -> ").expr(self, &l.body, 0)?;
        Ok(())
    }

    fn list_call(&self, w: &mut SqlWriter, f: SqlFn, args: &[SqlExpr]) -> Result<(), Refusal> {
        self.list_call_arm(w, f, args)
    }

    fn list_exists(&self, w: &mut SqlWriter, args: &[SqlExpr]) -> Result<(), Refusal> {
        let list = self.list(arg(args, 0)?);
        w.append("coalesce(any_match(").piece(&*list)?.append(", ");
        let l = arg(args, 1)?;
        element_only(lambda_arg(l)?, SqlFn::ListExists)?;
        w.expr(self, l, 0)?.append("), FALSE)");
        Ok(())
    }

    fn list_for_all(&self, w: &mut SqlWriter, args: &[SqlExpr]) -> Result<(), Refusal> {
        let list = self.list(arg(args, 0)?);
        w.append("coalesce(all_match(").piece(&*list)?.append(", ");
        let l = arg(args, 1)?;
        element_only(lambda_arg(l)?, SqlFn::ListForAll)?;
        w.expr(self, l, 0)?.append("), TRUE)");
        Ok(())
    }

    fn all_distinct(&self, w: &mut SqlWriter, args: &[SqlExpr]) -> Result<(), Refusal> {
        let list = self.list(arg(args, 0)?);
        w.append("coalesce(cardinality(array_distinct(").piece(&*list)?;
        let list = self.list(arg(args, 0)?);
        w.append(")) = cardinality(").piece(&*list)?.append("), TRUE)");
        Ok(())
    }

    fn membership(&self, w: &mut SqlWriter, needle: &SqlExpr, collection: &SqlExpr) -> Result<(), Refusal> {
        w.append("contains(").piece(&*self.list(collection))?.append(", ").expr(self, needle, 0)?.append(")");
        Ok(())
    }

    /// A list reduced: by an array function where one says it exactly, else as rows. NULL elements are skipped, as an
    /// aggregate skips them.
    fn reduce_collection(
        &self,
        w: &mut SqlWriter,
        reducer: AggFn,
        collection: &SqlExpr,
        extras: &[SqlExpr],
    ) -> Result<(), Refusal> {
        let present = self.present(collection);
        match reducer {
            // NULL over no present element, as string_agg over no row
            AggFn::StringAgg if extras.len() == 1 => {
                w.append("(CASE WHEN cardinality(").piece(&*present)?.append(") = 0 THEN NULL ELSE array_join(");
                w.append("transform(").piece(&*present)?.append(", \"__x\" -> CAST(\"__x\" AS VARCHAR)), ");
                w.expr(self, &extras[0], 0)?.append(") END)");
            }
            AggFn::Min => {
                w.append("array_min(").piece(&*present)?.append(")");
            }
            AggFn::Max => {
                w.append("array_max(").piece(&*present)?.append(")");
            }
            AggFn::Count => {
                w.append("cardinality(").piece(&*present)?.append(")");
            }
            _ => {
                w.piece(&*self.over_rows(collection, reducer, extras))?;
            }
        }
        Ok(())
    }

    fn unnest_projection(&self, w: &mut SqlWriter, args: &[SqlExpr]) -> Result<(), Refusal> {
        let _ = (w, args);
        Err(Refusal::capability(
            "an UNNEST in a select list reached Trino: it explodes only in FROM (ExplosionMarkers places it there; a \
             grouped select's is refused)",
        ))
    }

    /// Pure's fold, `(element, acc)`, as Trino's `reduce(xs, init, (acc, x) -> ..., acc -> acc)`: its accumulator may
    /// be any type, a list included; an empty or NULL list folds to the init.
    fn fold_call(&self, w: &mut SqlWriter, fold: &SqlExpr) -> Result<(), Refusal> {
        let SqlExpr::FoldCall { source, lambda, init, r#type, .. } = fold else {
            unreachable!("a fold")
        };
        let TypeFact::Typed { r#type: t, .. } = r#type else {
            return Err(Refusal::capability("a fold of unknown type reached Trino"));
        };
        let ty = cast_type_name(self, t)?;
        let elem = self.param(param_name(&lambda.params, 0)?)?;
        let acc = self.param(param_name(&lambda.params, 1)?)?;
        w.append("reduce(coalesce(").piece(&*self.list(source))?.append(", ARRAY[]), CAST(").expr(self, init, 0)?;
        w.append(&format!(" AS {ty}), ({acc}, {elem}) -> CAST(")).expr(self, &lambda.body, 0)?;
        w.append(&format!(" AS {ty}), {acc} -> {acc})"));
        Ok(())
    }

    /// Pure's splitPart: the non-empty tokens, 1-based, NULL past the end (split takes the delimiter as text).
    fn split_part_call(&self, w: &mut SqlWriter, a: &[SqlExpr]) -> Result<(), Refusal> {
        w.append("element_at(filter(split(").expr(self, arg(a, 0)?, 0)?.append(", ").expr(self, arg(a, 1)?, 0)?;
        w.append("), \"__t\" -> \"__t\" <> ''), ").expr(self, arg(a, 2)?, 0)?.append(")");
        Ok(())
    }

    fn struct_insert(&self, w: &mut SqlWriter, args: &[SqlExpr]) -> Result<(), Refusal> {
        let st = type_of(arg(args, 0)?);
        let (Some(SqlType::Struct { fields }), SqlExpr::StringLit { value: name, .. }) = (st, arg(args, 1)?) else {
            return Err(Refusal::capability("struct_insert over an untyped struct reached Trino"));
        };
        let Some(added) = type_of(arg(args, 2)?) else {
            return Err(Refusal::capability("struct_insert of a value of unknown type reached Trino"));
        };
        let kept: Vec<&StructField> = fields.iter().filter(|f| f.name != *name).collect();
        w.append("CAST(ROW(");
        for f in &kept {
            w.append("(").expr(self, &args[0], 8)?.append(").");
            let n = self.ident(&f.name);
            w.append(&n).append(", ");
        }
        w.expr(self, &args[2], 0)?.append(") AS ROW(");
        for f in &kept {
            let n = self.ident(&f.name);
            let t = cast_type_name(self, &f.r#type)?;
            w.append(&n).append(" ").append(&t).append(", ");
        }
        let n = self.ident(name);
        let t = cast_type_name(self, added)?;
        w.append(&n).append(" ").append(&t).append("))");
        Ok(())
    }

    // ---- variant (Trino's JSON)

    fn variant_construct(&self, w: &mut SqlWriter, a: &[SqlExpr]) -> Result<(), Refusal> {
        if a.len() != 1 {
            return Err(Refusal::capability(format!("toVariant of {} arguments reached Trino", a.len())));
        }
        self.to_variant(&a[0])(w)
    }

    /// A key (a member, or NULL) or a 0-based index (-1 the last, past the end NULL) of a JSON value: through a cast to
    /// a map or an array of JSON, NULL when it is not one.
    fn variant_get(&self, w: &mut SqlWriter, args: &[SqlExpr]) -> Result<(), Refusal> {
        let key = arg(args, 1)?;
        if let SqlExpr::StringLit { value, .. } = key
            && value == "$"
        {
            w.expr(self, arg(args, 0)?, 0)?;
            return Ok(());
        }
        if is_text(key) || matches!(key, SqlExpr::StringLit { .. }) {
            w.append("element_at(TRY_CAST(").expr(self, arg(args, 0)?, 0)?.append(" AS MAP(VARCHAR, JSON)), ");
            w.expr(self, key, 0)?.append(")");
            return Ok(());
        }
        w.append("element_at(TRY_CAST(").expr(self, arg(args, 0)?, 0)?.append(" AS ARRAY(JSON)), CASE WHEN ");
        w.expr(self, key, 0)?.append(" >= 0 THEN ").expr(self, key, 6)?.append(" + 1 ELSE ").expr(self, key, 0)?;
        w.append(" END)");
        Ok(())
    }

    fn variant_elements(&self, w: &mut SqlWriter, args: &[SqlExpr]) -> Result<(), Refusal> {
        let a0 = arg(args, 0)?;
        if list_element(a0).is_some() {
            w.expr(self, a0, 0)?;
            return Ok(());
        }
        w.append("TRY_CAST(").expr(self, a0, 0)?.append(" AS ARRAY(JSON))");
        Ok(())
    }

    fn variant_aware_cast(
        &self,
        w: &mut SqlWriter,
        value: &SqlExpr,
        target: &SqlType,
        conform: bool,
    ) -> Result<(), Refusal> {
        let target_is = |s: Scalar| matches!(target, SqlType::Scalar(t) if *t == s);
        if target_is(Scalar::TemporalText) || target_is(Scalar::DecimalText) {
            return ansi::variant_aware_cast(self, w, value, target, conform);
        }
        // a zoned timestamp from text: ISO 8601's T, Z, +hhmm offsets and one-digit months, which a cast refuses
        if target_is(Scalar::Timestamptz) && is_text(value) {
            w.append("from_iso8601_timestamp(").expr(self, value, 0)?.append(")");
            return Ok(());
        }
        // a timestamp from text: DuckDB's cast takes ISO 8601's T between the date and the time, Trino's only a space
        if target_is(Scalar::Timestamp) && is_text(value) {
            w.append("CAST(replace(").expr(self, value, 0)?.append(", 'T', ' ') AS ");
            let t = cast_type_name(self, target)?;
            w.append(&t).append(")");
            return Ok(());
        }
        if target_is(Scalar::Json) {
            if is_text(value) {
                w.append("json_parse(").expr(self, value, 0)?.append(")");
                return Ok(());
            }
            return self.to_variant(value)(w);
        }
        if is_json(value) {
            // a member's value as text is its text (a JSON string casts unquoted); the whole value as text is its JSON
            let member = matches!(value, SqlExpr::Call { r#fn: SqlFn::VariantGet, .. });
            if target_is(Scalar::Varchar) {
                if !member {
                    w.append("json_format(").expr(self, value, 0)?.append(")");
                    return Ok(());
                }
                w.append("(CASE WHEN is_json_scalar(").expr(self, value, 0)?.append(") THEN CAST(");
                w.expr(self, value, 0)?.append(" AS VARCHAR) ELSE json_format(").expr(self, value, 0)?.append(") END)");
                return Ok(());
            }
            if matches!(target, SqlType::Array { .. } | SqlType::Map { .. }) {
                w.append("TRY_CAST(").expr(self, value, 0)?.append(" AS ");
                let t = cast_type_name(self, target)?;
                w.append(&t).append(")");
                return Ok(());
            }
        }
        // a double's text: Trino's cast is always scientific; a double's JSON is Java's shortest text, its exponent
        // marked 'e' as DuckDB's
        if target_is(Scalar::Varchar) && (is_scalar(value, Scalar::Double) || matches!(value, SqlExpr::FloatLit { .. }))
        {
            w.append("replace(json_format(CAST(").expr(self, value, 0)?.append(" AS JSON)), 'E', 'e')");
            return Ok(());
        }
        ansi::variant_aware_cast(self, w, value, target, conform)
    }

    // ---- scalar functions: every SqlFn decided in `functions.rs`

    fn call(&self, w: &mut SqlWriter, f: SqlFn, a: &[SqlExpr], t: &TypeFact, parent_prec: i32) -> Result<(), Refusal> {
        self.call_arm(w, f, a, t, parent_prec)
    }

    // ---- the hooks of the base, the aggregates and windows: `aggregates.rs`

    fn hash_signed(&self, w: &mut SqlWriter, a: &[SqlExpr]) -> Result<(), Refusal> {
        self.hash_signed_arm(w, a)
    }

    fn round_half_even(&self, w: &mut SqlWriter, a: &[SqlExpr]) -> Result<(), Refusal> {
        self.round_half_even_arm(w, a)
    }

    fn bit_op(&self, w: &mut SqlWriter, f: SqlFn, a: &[SqlExpr]) -> Result<(), Refusal> {
        self.bit_op_arm(w, f, a)
    }

    fn reducer(&self, w: &mut SqlWriter, r: &Reducer) -> Result<(), Refusal> {
        self.reducer_arm(w, r)
    }

    fn window_call(
        &self,
        w: &mut SqlWriter,
        f: &SqlAgg,
        partition_by: &[SqlExpr],
        order_by: &[SortKey],
        frame: Option<&Frame>,
    ) -> Result<(), Refusal> {
        self.window_call_arm(w, f, partition_by, order_by, frame)
    }

    /// A frame bound: an interval offset a Trino interval literal (`INTERVAL '1' DAY`).
    fn bound(&self, b: &Bound) -> Result<String, Refusal> {
        match b {
            Bound::IntervalPreceding { n, unit } => Ok(format!("{} PRECEDING", aggregates::interval(*n, unit)?)),
            Bound::IntervalFollowing { n, unit } => Ok(format!("{} FOLLOWING", aggregates::interval(*n, unit)?)),
            other => Ok(ansi::bound(self, other)),
        }
    }

    fn json_object(&self, w: &mut SqlWriter, kv: &[SqlExpr]) -> Result<(), Refusal> {
        w.append("CAST(MAP(ARRAY[");
        for (i, k) in kv.iter().enumerate().step_by(2) {
            w.append(if i > 0 { ", " } else { "" }).expr(self, k, 0)?;
        }
        w.append("], ARRAY[");
        for (i, v) in kv.iter().enumerate().skip(1).step_by(2) {
            w.append(if i > 1 { ", " } else { "" }).piece(&*self.to_variant(v))?;
        }
        w.append("]) AS JSON)");
        Ok(())
    }

    fn json_array(&self, w: &mut SqlWriter, elements: &[SqlExpr]) -> Result<(), Refusal> {
        w.append("CAST(ARRAY[").join(elements, ", ", |w, e| self.to_variant(e)(w))?.append("] AS JSON)");
        Ok(())
    }

    fn json_array_agg(
        &self,
        w: &mut SqlWriter,
        value: &SqlExpr,
        order_keys: &[JsonArrayAggKey],
    ) -> Result<(), Refusal> {
        w.append("coalesce(CAST(array_agg(").piece(&*self.to_variant(value))?;
        if !order_keys.is_empty() {
            w.append(" ORDER BY ").join(order_keys, ", ", |w, k| {
                w.expr(self, &k.expr, 0)?.append(if k.desc { " DESC" } else { " ASC" }).append(" NULLS LAST");
                Ok(())
            })?;
        }
        w.append(") AS JSON), json_parse('[]'))");
        Ok(())
    }

    fn format_text(&self, parts: &[DateFmt]) -> Result<String, Refusal> {
        Ok(aggregates::format_text(parts))
    }

    /// This database's name for each scalar type, a refusal where it has none.
    fn scalar_type_name(&self, s: Scalar) -> Result<String, Refusal> {
        Ok(match s {
            Scalar::Boolean => "BOOLEAN",
            Scalar::Integer => "INTEGER",
            Scalar::Bigint => "BIGINT",
            Scalar::Hugeint => "DECIMAL(38,0)",
            Scalar::Double => "DOUBLE",
            Scalar::Varchar | Scalar::TemporalText => "VARCHAR",
            Scalar::Date => "DATE",
            Scalar::Timestamp => "TIMESTAMP(6)",
            Scalar::Timestamptz => "TIMESTAMP(6) WITH TIME ZONE",
            Scalar::Json => "JSON",
            Scalar::Time => "TIME(6)",
            Scalar::DecimalText => {
                return Err(Refusal::illegal_state(format!("{s} cast reached a dialect without {s} support")));
            }
        }
        .to_owned())
    }

    // ---- session tables and DDL

    /// Trino DDL: a nullable column says nothing (an explicit NULL is a syntax error).
    fn ddl_nullability(&self, not_null: bool) -> String {
        if not_null { " NOT NULL" } else { "" }.to_owned()
    }

    fn ddl_type(&self, t: &ColumnType) -> Result<String, Refusal> {
        Ok(match t {
            ColumnType::Plain { kind } => match kind {
                ColumnKind::Bigint => "BIGINT",
                ColumnKind::Smallint => "SMALLINT",
                ColumnKind::Tinyint => "TINYINT",
                ColumnKind::Integer => "INTEGER",
                ColumnKind::Float | ColumnKind::Double => "DOUBLE",
                ColumnKind::Real => "REAL",
                ColumnKind::Bit => "BOOLEAN",
                ColumnKind::Timestamp => "TIMESTAMP(6)",
                ColumnKind::Date => "DATE",
                ColumnKind::Time => "TIME(6)",
                ColumnKind::TimestampNs => "TIMESTAMP(9)",
                ColumnKind::Json => "JSON",
                ColumnKind::Varchar => "VARCHAR",
                ColumnKind::Other | ColumnKind::Distinct | ColumnKind::Array | ColumnKind::Object => {
                    return Err(Refusal::illegal_state(format!("no DDL spelling for declared column type {kind}")));
                }
            }
            .to_owned(),
            ColumnType::Sized { kind, size } => format!("{kind}({size})"),
            ColumnType::Scaled { kind, precision, scale } => format!("{kind}({precision}, {scale})"),
        })
    }

    // ---- values: what the driver hands back

    /// Its JSON_TYPE answers DuckDB's names (its emulation).
    fn json_kind_name(&self, kind: JsonKind) -> Result<String, Refusal> {
        Ok(match kind {
            JsonKind::Object => "OBJECT",
            JsonKind::Array => "ARRAY",
            JsonKind::Null => "NULL",
            JsonKind::String => "VARCHAR",
            JsonKind::Integer => "BIGINT",
            JsonKind::UnsignedInteger => "UBIGINT",
            JsonKind::Float => "DOUBLE",
            JsonKind::Boolean => "BOOLEAN",
        }
        .to_owned())
    }

    /// A DATE is `date` (Trino's typeof); no other type's name is compared here.
    fn typeof_name(&self, scalar: Scalar) -> Result<String, Refusal> {
        if scalar == Scalar::Date {
            return Ok("date".to_owned());
        }
        Err(Refusal::capability(format!("a TYPEOF compared with {scalar}'s name reached Trino")))
    }
}

#[cfg(test)]
mod tests;

// ==================================================================
// SqlDialect
// ==================================================================

impl crate::dialect::SqlDialect for Trino {
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
        <Trino as AnsiSqlRenderer>::reads_stored(t)
    }

    fn table_name_spelled(&self, name: &str, quoting: NameQuoting) -> String {
        AnsiSqlRenderer::table_name(self, name, quoting)
    }

    /// The platform's naive-UTC temporal contract.
    fn session_setup(&self) -> Vec<String> {
        vec!["SET TIME ZONE 'UTC'".to_owned()]
    }

    /// Trino has no temporary table: a plan's metamodel tables are its query's own rows.
    fn temporary_tables(&self) -> bool {
        false
    }

    /// Trino has no temporary table, so no table only one session sees: refused by name.
    fn session_table(&self, table: &SqlDdl, rows: Option<&SqlDml>) -> Result<Vec<String>, Refusal> {
        let _ = rows;
        let name = match table {
            SqlDdl::CreateTable { table, .. } => table.as_str(),
            _ => unreachable!("the seam hands a session table a createTable"),
        };
        Err(Refusal::capability(format!("a session's own table reached Trino, which has no temporary tables: {name}")))
    }

    /// No transaction across a session's statements for its connectors.
    fn keys_snapshot(&self) -> Result<KeysSnapshot, Refusal> {
        Ok(KeysSnapshot::None)
    }

    /// No column limit documented.
    fn result_column_limit(&self) -> Result<Option<i32>, Refusal> {
        Ok(None)
    }

    /// No native PIVOT: a static pivot is a grouped CASE aggregation, its values discovered first.
    fn needs_static_pivot(&self) -> bool {
        true
    }

    /// A statement's ROW column is delivered as its JSON text, decoded to the struct's map.
    fn delivers_own(&self, label: &SqlType, wire: &str) -> bool {
        matches!(label, SqlType::Struct { .. }) && wire == "JSON"
    }
}
