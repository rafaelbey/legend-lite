// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! Java's `AnsiSqlRenderer` as a trait (docs/rust-port/TRIAGE.md D3): each protected hook is a method, its default
//! body the base's, written once as a free function in [`crate::ansi`] -- so a dialect's override that called
//! `super.x(...)` calls `ansi::x(self, ...)`. The base's `final` helpers are free functions of `ansi` only; its
//! `abstract` hooks are the methods without a default. Each hook keeps Java's name in snake case and its parameters
//! in Java's order; where Java passes a record that is a Rust enum variant, the hook takes the variant's fields (or,
//! for a source, the source node).
//!
//! Java's private instance state of the base -- `inlineMode`, the single-line mode a subquery is written in -- is
//! [`AnsiState`], which every dialect object holds (Java's field lived on the dialect object; it is restored around
//! each use, so it never outlives one render).

use crate::ansi;
use crate::refusal::Refusal;
use crate::rewriter::SqlRewriter;
use crate::statement::{RenderedStatement, TypeSpelling};
use crate::writer::SqlWriter;
use lite_sql::{
    AggFn, BigDecimal, Bound, Column, ColumnType, Cte, DateFmt, Frame, JoinKind, JsonArrayAggKey, JsonKind, Lambda,
    NameQuoting, OutputCol, ParamKind, Projection, Reducer, Scalar, SortKey, SqlAgg, SqlDdl, SqlDml, SqlExpr, SqlFn,
    SqlQuery, SqlSelect, SqlSource, SqlType, StructField, StructLitField, TypeFact, ValueKind, ValueTyping, When,
};
use std::borrow::Cow;
use std::cell::Cell;
use std::collections::HashSet;

/// Java's `Lexicon`: what a backend quotes and how -- its identifier quote character, and the lower-cased words that
/// force quoting of a plain-looking identifier.
#[derive(Debug)]
pub struct Lexicon {
    quote: char,
    reserved: HashSet<&'static str>,
}

impl Lexicon {
    pub fn new(quote: char, reserved: &[&'static str]) -> Lexicon {
        Lexicon { quote, reserved: reserved.iter().copied().collect() }
    }

    pub fn quote_char(&self) -> char {
        self.quote
    }

    /// The reserved words, in no particular order (Java's `Lexicon.reservedWords()`).
    pub fn reserved_words(&self) -> impl Iterator<Item = &'static str> + '_ {
        self.reserved.iter().copied()
    }

    /// Whether `lower` (a lower-cased word) is reserved.
    pub fn is_reserved(&self, lower: &str) -> bool {
        self.reserved.contains(lower)
    }
}

/// The base renderer's own instance state (Java's `AnsiSqlRenderer` private fields), held by each dialect object.
#[derive(Debug, Default)]
pub struct AnsiState {
    /// Java's `inlineMode`: when set, clause separators render as single spaces.
    pub(crate) inline: Cell<bool>,
}

/// A plan parameter (`SqlExpr::PlanParam`), its fields borrowed: what the placeholder hooks take.
#[derive(Debug, Clone, Copy)]
pub struct PlanParam<'a> {
    pub name: &'a str,
    pub kind: ParamKind,
    pub optional: bool,
    pub enum_map_fn: Option<&'a str>,
    pub r#type: &'a TypeFact,
    pub by_value: Option<&'a ValueTyping>,
}

impl<'a> PlanParam<'a> {
    /// The parameter `e` is, when it is one.
    pub fn of(e: &'a SqlExpr) -> Option<PlanParam<'a>> {
        match e {
            SqlExpr::PlanParam { name, kind, optional, enum_map_fn, r#type, by_value } => Some(PlanParam {
                name,
                kind: *kind,
                optional: *optional,
                enum_map_fn: enum_map_fn.as_deref(),
                r#type,
                by_value: by_value.as_ref(),
            }),
            _ => None,
        }
    }
}

/// The renderer every dialect is (Java's `AnsiSqlRenderer`): its hooks, each with the base's body as its default.
pub trait AnsiSqlRenderer: Sized {
    // ---- the dialect's own data

    /// The dialect's lexicon (Java's constructor argument, its class's `LEXICON`).
    fn lexicon(&self) -> &Lexicon;

    /// The base's instance state, held by the dialect object.
    fn ansi_state(&self) -> &AnsiState;

    /// Java's `getClass().getSimpleName()`, which some refusals name.
    fn simple_name(&self) -> &'static str;

    // ---- the entry points (Java's `SqlDialect` methods, each overridable)

    fn render(&self, query: SqlQuery) -> Result<String, Refusal> {
        ansi::render(self, query)
    }

    fn render_statement(&self, query: SqlQuery) -> Result<RenderedStatement, Refusal> {
        ansi::render_statement(self, query)
    }

    fn render_ddl(&self, ddl: &SqlDdl) -> Result<String, Refusal> {
        ansi::render_ddl(self, ddl)
    }

    fn render_dml(&self, dml: &SqlDml) -> Result<String, Refusal> {
        ansi::render_dml(self, dml)
    }

    fn render_dml_statement(&self, dml: &SqlDml) -> Result<RenderedStatement, Refusal> {
        ansi::render_dml_statement(self, dml)
    }

    /// The engine's H2 text translated for this dialect's database (Java's `SqlDialect.fromEngineText`, its name
    /// kept).
    #[allow(clippy::wrong_self_convention)]
    fn from_engine_text(&self, statement: &str) -> Result<String, Refusal>;

    // ---- the passes

    /// This dialect's MIR passes, run at render entry (the carrier strategy pass first).
    fn passes(&self) -> Vec<Box<dyn SqlRewriter>>;

    fn spells_float_canon(&self) -> bool {
        false
    }

    fn spells_float_print(&self) -> bool {
        false
    }

    fn binds_float_print(&self) -> bool {
        false
    }

    fn binds_float_text(&self) -> bool {
        false
    }

    /// Whether this dialect reads a column stored as `t` other than as the database holds it. A function of the type
    /// alone on every Java dialect (none reads its instance), so the `StoredReads` pass holds it as a plain function.
    fn reads_stored(t: &ColumnType) -> bool {
        ansi::reads_as_text(t)
    }

    fn stored_read(&self, w: &mut SqlWriter, column: &Column, stored: &ColumnType) -> Result<(), Refusal> {
        ansi::stored_read(self, w, column, stored)
    }

    fn huge_int_literal(&self, v: &BigDecimal) -> String {
        ansi::huge_int_literal(v)
    }

    // ---- queries and clause assembly

    fn query(&self, w: &mut SqlWriter, q: &SqlQuery, depth: usize) -> Result<(), Refusal> {
        ansi::query(self, w, q, depth)
    }

    fn select(&self, w: &mut SqlWriter, s: &SqlSelect, depth: usize) -> Result<(), Refusal> {
        ansi::select(self, w, s, depth)
    }

    fn source_url(&self, url: &str) -> Result<String, Refusal> {
        ansi::source_url(url)
    }

    fn supports_qualify(&self) -> bool {
        false
    }

    fn append_qualify(&self, w: &mut SqlWriter, s: &SqlSelect, depth: usize) -> Result<(), Refusal> {
        ansi::append_qualify(self, w, s, depth)
    }

    fn column_ref(&self, c: &Column) -> Result<String, Refusal> {
        ansi::column_ref(self, c)
    }

    fn projection(&self, w: &mut SqlWriter, p: &Projection) -> Result<(), Refusal> {
        ansi::projection(self, w, p)
    }

    fn projection_value(&self, w: &mut SqlWriter, p: &Projection) -> Result<(), Refusal> {
        ansi::projection_value(self, w, p)
    }

    fn projections(&self, w: &mut SqlWriter, ps: &[Projection]) -> Result<(), Refusal> {
        ansi::projections(self, w, ps)
    }

    fn per_key_name(&self, w: &mut SqlWriter, stem: &str) -> Result<(), Refusal> {
        ansi::per_key_name(self, w, stem)
    }

    fn implicit_label(&self, p: &Projection) -> Result<Option<String>, Refusal> {
        ansi::implicit_label(self, p)
    }

    fn sort_key(&self, w: &mut SqlWriter, k: &SortKey) -> Result<(), Refusal> {
        ansi::sort_key(self, w, k)
    }

    // ---- sources

    fn source(&self, w: &mut SqlWriter, src: &SqlSource, depth: usize) -> Result<(), Refusal> {
        ansi::source(self, w, src, depth)
    }

    fn cte_name(&self, name: &str) -> String {
        name.to_owned()
    }

    fn join_condition<'e>(&self, kind: JoinKind, on: &'e SqlExpr) -> Cow<'e, SqlExpr> {
        let _ = kind;
        Cow::Borrowed(on)
    }

    fn cte_as(&self, c: &Cte) -> String {
        let _ = c;
        " AS (".to_owned()
    }

    fn subselect_source(&self, w: &mut SqlWriter, inner: &SqlQuery, alias: &str, depth: usize) -> Result<(), Refusal> {
        ansi::subselect_source(self, w, inner, alias, depth)
    }

    /// A VALUES source (Java's `valuesSource(writer, v)`: its rows, column names, alias and declared outputs).
    fn values_source(
        &self,
        w: &mut SqlWriter,
        rows: &[Vec<SqlExpr>],
        columns: &[String],
        alias: &str,
        outputs: &[OutputCol],
    ) -> Result<(), Refusal> {
        let _ = outputs;
        ansi::values_source(self, w, rows, columns, alias)
    }

    /// A PIVOT source (`pivot` is the `SqlSource::Pivot` node).
    fn pivot_source(&self, w: &mut SqlWriter, pivot: &SqlSource, depth: usize) -> Result<(), Refusal> {
        let _ = (w, pivot, depth);
        Err(Refusal::capability("pivot reached a dialect without a PIVOT strategy"))
    }

    fn join_keyword(&self, kind: JoinKind) -> Result<String, Refusal> {
        ansi::join_keyword(self, kind)
    }

    fn as_of_join_clause(&self) -> Result<String, Refusal> {
        Err(Refusal::capability("asOfJoin reached a dialect without an AS-OF strategy"))
    }

    // ---- expressions

    fn checked_one(
        &self,
        w: &mut SqlWriter,
        list: &SqlExpr,
        scalar_carrier: bool,
        at_least_only: bool,
        parent_prec: i32,
    ) -> Result<(), Refusal> {
        ansi::checked_one(self, w, list, scalar_carrier, at_least_only, parent_prec)
    }

    fn compact_list(&self, w: &mut SqlWriter, list: &SqlExpr, parent_prec: i32) -> Result<(), Refusal> {
        ansi::compact_list(self, w, list, parent_prec)
    }

    fn expr(&self, w: &mut SqlWriter, e: &SqlExpr, parent_prec: i32) -> Result<(), Refusal> {
        ansi::expr(self, w, e, parent_prec)
    }

    /// The star exclusion's keyword (a dialect without one refuses: Postgres's).
    fn star_except_keyword(&self) -> Result<String, Refusal> {
        Ok("EXCLUDE".to_owned())
    }

    /// The stable row-order column (a dialect without one refuses: Postgres's).
    fn row_order_column(&self) -> Result<String, Refusal> {
        Ok("rowid".to_owned())
    }

    fn membership(&self, w: &mut SqlWriter, needle: &SqlExpr, collection: &SqlExpr) -> Result<(), Refusal> {
        ansi::membership(self, w, needle, collection)
    }

    fn reduce_collection(
        &self,
        w: &mut SqlWriter,
        reducer: AggFn,
        collection: &SqlExpr,
        extras: &[SqlExpr],
    ) -> Result<(), Refusal> {
        let _ = (w, collection, extras);
        Err(Refusal::capability(format!("collection reduction '{reducer}' reached a dialect without a list encoding")))
    }

    fn json_object(&self, w: &mut SqlWriter, kv: &[SqlExpr]) -> Result<(), Refusal> {
        ansi::json_object(self, w, kv)
    }

    fn json_array(&self, w: &mut SqlWriter, elements: &[SqlExpr]) -> Result<(), Refusal> {
        ansi::json_array(self, w, elements)
    }

    fn json_array_agg(
        &self,
        w: &mut SqlWriter,
        value: &SqlExpr,
        order_keys: &[JsonArrayAggKey],
    ) -> Result<(), Refusal> {
        ansi::json_array_agg(self, w, value, order_keys)
    }

    /// A function call: each dialect decides every `SqlFn` in its own exhaustive match. `r#type` is the call's own
    /// type fact (Java's `c.type()`), which some arms read.
    fn call(
        &self,
        w: &mut SqlWriter,
        r#fn: SqlFn,
        args: &[SqlExpr],
        r#type: &TypeFact,
        parent_prec: i32,
    ) -> Result<(), Refusal>;

    fn max_function_arguments(&self) -> usize {
        // Java's Integer.MAX_VALUE: no limit
        i32::MAX as usize
    }

    /// A JSON kind's name as this dialect's `JSON_TYPE` answers it.
    fn json_kind_name(&self, kind: JsonKind) -> Result<String, Refusal>;

    /// A type's name as this dialect's `TYPEOF` answers it.
    fn typeof_name(&self, scalar: Scalar) -> Result<String, Refusal>;

    fn hash_signed(&self, w: &mut SqlWriter, a: &[SqlExpr]) -> Result<(), Refusal> {
        let _ = (w, a);
        Err(Refusal::capability("signed 64-bit hashCode reached a dialect without a spelling"))
    }

    fn round_half_even(&self, w: &mut SqlWriter, a: &[SqlExpr]) -> Result<(), Refusal> {
        let _ = (w, a);
        Err(Refusal::capability("banker's ROUND reached a dialect without a spelling"))
    }

    fn bit_op(&self, w: &mut SqlWriter, r#fn: SqlFn, a: &[SqlExpr]) -> Result<(), Refusal> {
        let _ = (w, a);
        Err(Refusal::capability(format!("{fn} reached a dialect without bit-op support")))
    }

    fn variant_construct(&self, w: &mut SqlWriter, a: &[SqlExpr]) -> Result<(), Refusal> {
        let _ = (w, a);
        Err(Refusal::capability("toVariant reached a dialect without JSON support"))
    }

    /// A fold (`fold` is the `SqlExpr::FoldCall` node: Java's record, its type fact with it).
    fn fold_call(&self, w: &mut SqlWriter, fold: &SqlExpr) -> Result<(), Refusal> {
        let _ = (w, fold);
        Err(Refusal::capability("fold reached a dialect without a fold encoding"))
    }

    fn list_exists(&self, w: &mut SqlWriter, args: &[SqlExpr]) -> Result<(), Refusal> {
        let _ = (w, args);
        Err(Refusal::capability("collection exists reached a dialect without a list-predicate encoding"))
    }

    fn all_distinct(&self, w: &mut SqlWriter, args: &[SqlExpr]) -> Result<(), Refusal> {
        let _ = (w, args);
        Err(Refusal::capability("collection isDistinct reached a dialect without a list encoding"))
    }

    fn list_for_all(&self, w: &mut SqlWriter, args: &[SqlExpr]) -> Result<(), Refusal> {
        let _ = (w, args);
        Err(Refusal::capability("collection forAll reached a dialect without a list-predicate encoding"))
    }

    fn list_call(&self, w: &mut SqlWriter, r#fn: SqlFn, args: &[SqlExpr]) -> Result<(), Refusal> {
        let _ = (w, args);
        Err(Refusal::capability(format!("{fn} reached a dialect without a list encoding")))
    }

    fn split_part_call(&self, w: &mut SqlWriter, args: &[SqlExpr]) -> Result<(), Refusal> {
        let _ = (w, args);
        Err(Refusal::capability("PURE_SPLIT_PART reached a dialect without a spelling"))
    }

    fn unnest_projection(&self, w: &mut SqlWriter, args: &[SqlExpr]) -> Result<(), Refusal> {
        let _ = (w, args);
        Err(Refusal::capability("UNNEST reached a dialect without an unnest placement"))
    }

    fn variant_elements(&self, w: &mut SqlWriter, args: &[SqlExpr]) -> Result<(), Refusal> {
        let _ = (w, args);
        Err(Refusal::capability("variant navigation reached a dialect without JSON support"))
    }

    fn variant_get(&self, w: &mut SqlWriter, args: &[SqlExpr]) -> Result<(), Refusal> {
        ansi::variant_get(self, w, args)
    }

    fn struct_insert(&self, w: &mut SqlWriter, args: &[SqlExpr]) -> Result<(), Refusal> {
        let _ = (w, args);
        Err(Refusal::capability("struct_insert reached a dialect without struct support"))
    }

    fn lambda(&self, w: &mut SqlWriter, l: &Lambda) -> Result<(), Refusal> {
        let _ = (w, l);
        Err(Refusal::capability("a lambda reached a dialect without lambda support"))
    }

    fn variant_aware_cast(
        &self,
        w: &mut SqlWriter,
        value: &SqlExpr,
        target: &SqlType,
        conform: bool,
    ) -> Result<(), Refusal> {
        ansi::variant_aware_cast(self, w, value, target, conform)
    }

    // ---- window, aggregate, case

    /// A CASE; `r#type` is its own type fact (Java's `c.type()`).
    fn case_expr(
        &self,
        w: &mut SqlWriter,
        whens: &[When],
        otherwise: Option<&SqlExpr>,
        r#type: &TypeFact,
    ) -> Result<(), Refusal> {
        let _ = r#type;
        ansi::case_expr(self, w, whens, otherwise)
    }

    fn window_call(
        &self,
        w: &mut SqlWriter,
        r#fn: &SqlAgg,
        partition_by: &[SqlExpr],
        order_by: &[SortKey],
        frame: Option<&Frame>,
    ) -> Result<(), Refusal> {
        ansi::window_call(self, w, r#fn, partition_by, order_by, frame)
    }

    fn keyword(&self, keyword: &str) -> String {
        keyword.to_owned()
    }

    fn aggregate_name(&self, r#fn: AggFn) -> String {
        r#fn.as_str().to_owned()
    }

    /// A window frame bound (a dialect may refuse one it cannot spell: Postgres's interval units).
    fn bound(&self, b: &Bound) -> Result<String, Refusal> {
        Ok(ansi::bound(self, b))
    }

    /// Each dialect names every aggregate itself.
    fn reducer(&self, w: &mut SqlWriter, r: &Reducer) -> Result<(), Refusal>;

    /// A catalog grid's query (`catalog` is the `SqlSource::Catalog` node).
    fn catalog(&self, w: &mut SqlWriter, catalog: &SqlSource) -> Result<(), Refusal> {
        ansi::catalog(self, w, catalog)
    }

    fn agg_order_null_placement(&self, k: &SortKey) -> String {
        ansi::agg_order_null_placement(k)
    }

    // ---- lexical extension points

    /// A string literal (a dialect may refuse one its text type cannot hold: Postgres's NUL).
    fn string_lit(&self, value: &str) -> Result<String, Refusal> {
        Ok(ansi::string_lit(value))
    }

    fn bool_lit(&self, value: bool) -> String {
        if value { "TRUE" } else { "FALSE" }.to_owned()
    }

    /// A date literal (a dialect may refuse one its dates cannot hold: ClickHouse's years past 1900 to 2299).
    fn date_lit(&self, iso: &str) -> Result<String, Refusal> {
        Ok(format!("DATE '{iso}'"))
    }

    /// A date-time literal (a dialect may refuse one its dates cannot hold: ClickHouse's).
    fn timestamp_lit(&self, iso: &str) -> Result<String, Refusal> {
        Ok(format!("TIMESTAMP '{iso}'"))
    }

    fn time_lit(&self, iso: &str) -> Result<String, Refusal> {
        Ok(format!("TIME '{iso}'"))
    }

    fn array_lit(&self, w: &mut SqlWriter, elements: &[SqlExpr]) -> Result<(), Refusal> {
        let _ = (w, elements);
        Err(Refusal::capability("an array literal reached a dialect without array support"))
    }

    fn struct_lit(&self, w: &mut SqlWriter, fields: &[StructLitField]) -> Result<(), Refusal> {
        ansi::struct_lit(self, w, fields)
    }

    /// A struct's field read; `r#type` is the read's own type fact (Java's `g.type()`: not a function of its source
    /// alone, a builder may have given it).
    fn struct_get(&self, w: &mut SqlWriter, source: &SqlExpr, field: &str, r#type: &TypeFact) -> Result<(), Refusal> {
        let _ = r#type;
        ansi::struct_get(self, w, source, field)
    }

    /// A scalar type's name: each dialect decides every scalar, a name or a refusal.
    fn scalar_type_name(&self, s: Scalar) -> Result<String, Refusal>;

    fn array_type_name(&self, element: &str) -> String {
        format!("{element}[]")
    }

    fn map_type_name(&self, key: &str, value: &str) -> String {
        format!("MAP({key}, {value})")
    }

    /// A struct type's spelling, or its refusal where the database has none.
    fn struct_type_name(&self, fields: &[StructField]) -> Result<String, Refusal>;

    /// The date-format spelling: each dialect writes its own vocabulary.
    fn format_text(&self, parts: &[DateFmt]) -> Result<String, Refusal>;

    fn infix_pad(&self, r#fn: SqlFn) -> String {
        let _ = r#fn;
        " ".to_owned()
    }

    fn placeholder(&self, w: &mut SqlWriter, p: &PlanParam<'_>) -> Result<(), Refusal> {
        ansi::placeholder(self, w, p)
    }

    fn hole_type(&self, kind: ValueKind) -> Result<TypeSpelling, Refusal> {
        ansi::hole_type(kind)
    }

    fn table_name(&self, name: &str, quoting: NameQuoting) -> String {
        ansi::table_name(self, name, quoting)
    }

    /// A column's name at a reference (a dialect may refuse one it cannot spell: H2's origin-less column).
    fn column_name(&self, c: &Column) -> Result<String, Refusal> {
        Ok(ansi::column_name(self, c))
    }

    fn delimited(&self, name: &str) -> String {
        ansi::delimited(self, name)
    }

    fn star_except_name(&self, name: &str) -> String {
        self.delimited(name)
    }

    fn alias_ident(&self, name: &str) -> String {
        self.ident(name)
    }

    // ---- the store writes

    /// `dml` is a `SqlDml::InsertSelect`.
    fn insert_select(&self, dml: &SqlDml) -> Result<SqlWriter, Refusal> {
        let _ = dml;
        Err(ansi::unmeasured_write(self, "an INSERT of a query's rows"))
    }

    /// `dml` is a `SqlDml::Update`.
    fn update(&self, dml: &SqlDml) -> Result<SqlWriter, Refusal> {
        let _ = dml;
        Err(ansi::unmeasured_write(self, "an UPDATE"))
    }

    /// `dml` is a `SqlDml::DeleteWhere`.
    fn delete_where(&self, dml: &SqlDml) -> Result<SqlWriter, Refusal> {
        let _ = dml;
        Err(ansi::unmeasured_write(self, "a DELETE ... WHERE"))
    }

    /// `dml` is a `SqlDml::Merge`.
    fn merge(&self, dml: &SqlDml) -> Result<SqlWriter, Refusal> {
        let _ = dml;
        Err(ansi::unmeasured_write(self, "a MERGE"))
    }

    /// `dml` is a `SqlDml::CreateTempTableAs`.
    fn temp_table_as(&self, dml: &SqlDml) -> Result<SqlWriter, Refusal> {
        let _ = dml;
        Err(Refusal::capability(format!(
            "a temporary table from a query (CREATE TEMPORARY TABLE ... AS) on {}: no spelling measured on its \
             database (the test-data generator's sessions are H2, DuckDB and SQLite)",
            self.simple_name()
        )))
    }

    // ---- DDL

    fn stored_name(&self, name: &str, declared_quoted: bool) -> String {
        if declared_quoted { self.delimited(name) } else { self.physical_name(name) }
    }

    /// A store column's declared type as this database spells it in DDL.
    fn ddl_type(&self, t: &ColumnType) -> Result<String, Refusal>;

    fn ddl_identifier(&self, name: &str, declared_quoted: bool) -> String {
        if declared_quoted { self.delimited(name) } else { self.ident(name) }
    }

    fn insert_column_name(&self, name: &str, declared_quoted: bool) -> String {
        self.ddl_identifier(name, declared_quoted)
    }

    fn ddl_key_identifier(&self, name: &str, declared_quoted: bool) -> String {
        self.ddl_identifier(name, declared_quoted)
    }

    fn ddl_column_separator(&self) -> String {
        ", ".to_owned()
    }

    fn ddl_nullability(&self, not_null: bool) -> String {
        if not_null { " NOT NULL" } else { " NULL" }.to_owned()
    }

    fn physical_name(&self, name: &str) -> String {
        self.ident(name)
    }

    fn ident(&self, name: &str) -> String {
        ansi::ident(self, name)
    }

    fn nl(&self, w: &mut SqlWriter, depth: usize) {
        ansi::nl(self, w, depth)
    }

    fn float_literal(&self, v: f64) -> Result<String, Refusal> {
        crate::java::plain_float(v)
    }
}
