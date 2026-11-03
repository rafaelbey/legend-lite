// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! The MIR's queries, sources, expressions, aggregates, types and type facts: Java's records in `com.legend.sql`,
//! field for field and in their component order. A Java name that is a Rust keyword is its raw identifier
//! (`r#type`, `r#fn`, `r#where`, `r#in`); a Java `Map` is its entries in order; a nullable component is an `Option`.
//!
//! Where Java types a component narrowly -- `StoredRead.column` a `Column`, `FoldCall.lambda` a `Lambda`,
//! `Pivot.Using.agg` a `Reducer`, `DeferredTdsString.inner` a `SqlSelect` -- the record is a struct of its own, which
//! its variant wraps. `SqlExpr.Tagged` and `SqlExpr.TaggedList` have no variant: they are resolved before a plan is
//! written, and MirJson refuses them.

use crate::names::{
    AggFn, CatalogGrid, DateFmtPart, DatePart, FrameKind, JoinKind, JsonKind, NullOrder, Origin, ParamKind, Quantifier,
    Scalar, SqlFn, TdsForm, ValueKind,
};
use crate::stmt::ColumnType;
use lite_json::Error;

/// Java's `SqlQuery`.
#[derive(Debug, Clone, PartialEq)]
pub enum SqlQuery {
    Select(Box<SqlSelect>),
    Union(SqlUnion),
    With(SqlWith),
}

/// Java's `SqlSelect`: one query node, every clause in its slot.
#[derive(Debug, Clone, PartialEq)]
pub struct SqlSelect {
    pub projections: Vec<Projection>,
    pub distinct: bool,
    pub from: SqlSource,
    pub r#where: Option<SqlExpr>,
    pub group_by: Vec<SqlExpr>,
    pub having: Option<SqlExpr>,
    pub qualify: Option<SqlExpr>,
    pub order_by: Vec<SortKey>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
    pub outputs: Vec<OutputCol>,
}

/// Java's `SqlSelect.Projection`.
#[derive(Debug, Clone, PartialEq)]
pub struct Projection {
    pub expr: SqlExpr,
    pub alias: Option<String>,
    pub out: Option<OutputCol>,
}

/// Java's `SqlSelect.SortKey`.
#[derive(Debug, Clone, PartialEq)]
pub struct SortKey {
    pub expr: SqlExpr,
    pub ascending: bool,
    pub null_order: Option<NullOrder>,
    pub output_name: Option<String>,
}

/// Java's `SqlUnion`.
#[derive(Debug, Clone, PartialEq)]
pub struct SqlUnion {
    pub branches: Vec<SqlQuery>,
    pub all: bool,
    pub outputs: Vec<OutputCol>,
}

/// Java's `SqlWith`.
#[derive(Debug, Clone, PartialEq)]
pub struct SqlWith {
    pub ctes: Vec<Cte>,
    pub body: Box<SqlQuery>,
    pub recursive: bool,
}

/// Java's `SqlWith.Cte`.
#[derive(Debug, Clone, PartialEq)]
pub struct Cte {
    pub name: String,
    pub query: SqlQuery,
    pub materialized: bool,
}

/// Java's `OutputCol`.
#[derive(Debug, Clone, PartialEq)]
pub struct OutputCol {
    pub name: String,
    pub r#type: SqlType,
    pub nullable: bool,
    pub origin: Origin,
}

/// Java's `NameQuoting`: which parts of a stored table's name its store declared quoted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct NameQuoting {
    pub schema: bool,
    pub table: bool,
}

/// Java's `SqlSource`.
#[derive(Debug, Clone, PartialEq)]
pub enum SqlSource {
    Table {
        name: String,
        alias: String,
        outputs: Vec<OutputCol>,
        call: bool,
        stored_types: Vec<(String, ColumnType)>,
        quoting: NameQuoting,
    },
    Subselect {
        inner: Box<SqlQuery>,
        alias: String,
        frame_name: Option<String>,
    },
    Cte {
        name: String,
        alias: String,
        outputs: Vec<OutputCol>,
    },
    Join {
        left: Box<SqlSource>,
        right: Box<SqlSource>,
        kind: JoinKind,
        on: Option<Box<SqlExpr>>,
        outputs: Vec<OutputCol>,
    },
    Pivot {
        source: Box<SqlSource>,
        on: Vec<SqlExpr>,
        r#in: Vec<SqlExpr>,
        named: Vec<SqlExpr>,
        usings: Vec<PivotUsing>,
        alias: String,
        outputs: Vec<OutputCol>,
    },
    Values {
        rows: Vec<Vec<SqlExpr>>,
        columns: Vec<String>,
        alias: String,
        outputs: Vec<OutputCol>,
    },
    Dual,
    SourceUrl {
        url: String,
        alias: String,
        outputs: Vec<OutputCol>,
    },
    RawSql {
        sql: String,
        alias: String,
        outputs: Vec<OutputCol>,
        written_for: String,
    },
    Catalog {
        grid: CatalogGrid,
        schema_pattern: Option<String>,
        table_pattern: Option<String>,
        column_pattern: Option<String>,
        alias: String,
        outputs: Vec<OutputCol>,
    },
    VarSetPlaceholder {
        var_name: String,
        alias: String,
        outputs: Vec<OutputCol>,
    },
}

/// Java's `SqlSource.Pivot.Using`.
#[derive(Debug, Clone, PartialEq)]
pub struct PivotUsing {
    pub agg: Reducer,
    pub alias: String,
    pub r#type: Option<SqlType>,
}

/// Java's `SqlExpr`: every node carries its type fact, `r#type`.
#[derive(Debug, Clone, PartialEq)]
pub enum SqlExpr {
    RowOrder {
        table: Option<String>,
        r#type: TypeFact,
    },
    StoredRead {
        column: Column,
        stored: ColumnType,
        r#type: TypeFact,
    },
    ReduceCollection {
        reducer: AggFn,
        collection: Box<SqlExpr>,
        extras: Vec<SqlExpr>,
        r#type: TypeFact,
    },
    Membership {
        needle: Box<SqlExpr>,
        collection: Box<SqlExpr>,
        r#type: TypeFact,
    },
    Column(Column),
    StarExcept {
        table: Option<String>,
        except: Vec<String>,
        r#type: TypeFact,
    },
    Star {
        table: Option<String>,
        r#type: TypeFact,
    },
    StringLit {
        value: String,
        r#type: TypeFact,
    },
    IntLit {
        value: i64,
        r#type: TypeFact,
    },
    FloatLit {
        value: Double,
        r#type: TypeFact,
    },
    DecimalLit {
        value: BigDecimal,
        r#type: TypeFact,
    },
    BoolLit {
        value: bool,
        r#type: TypeFact,
    },
    NullLit {
        r#type: TypeFact,
    },
    DateLit {
        iso: String,
        r#type: TypeFact,
    },
    Group {
        inner: Box<SqlExpr>,
        r#type: TypeFact,
    },
    KeyCell {
        cell: i32,
        r#type: TypeFact,
    },
    PlanParam {
        name: String,
        kind: ParamKind,
        optional: bool,
        enum_map_fn: Option<String>,
        r#type: TypeFact,
        by_value: Option<ValueTyping>,
    },
    FormatLit {
        parts: Vec<DateFmt>,
        r#type: TypeFact,
    },
    DatePartLit {
        part: DatePart,
        r#type: TypeFact,
    },
    JsonKindLit {
        kind: JsonKind,
        r#type: TypeFact,
    },
    TypeNameLit {
        scalar: Scalar,
        r#type: TypeFact,
    },
    TimestampLit {
        iso: String,
        r#type: TypeFact,
    },
    TimeLit {
        iso: String,
        r#type: TypeFact,
    },
    OrderedListAgg {
        value: Box<SqlExpr>,
        order_by: Box<SqlExpr>,
        r#type: TypeFact,
    },
    ArrayLit {
        elements: Vec<SqlExpr>,
        r#type: TypeFact,
    },
    StructLit {
        fields: Vec<StructLitField>,
        r#type: TypeFact,
    },
    StructGet {
        source: Box<SqlExpr>,
        field: String,
        r#type: TypeFact,
    },
    Call {
        r#fn: SqlFn,
        args: Vec<SqlExpr>,
        r#type: TypeFact,
    },
    Case {
        whens: Vec<When>,
        otherwise: Option<Box<SqlExpr>>,
        r#type: TypeFact,
    },
    Exists {
        subquery: Box<SqlQuery>,
        r#type: TypeFact,
    },
    InSubquery {
        value: Box<SqlExpr>,
        subquery: Box<SqlQuery>,
        r#type: TypeFact,
    },
    CheckedDefects {
        own: Box<SqlExpr>,
        hoists: Vec<Hoist>,
        r#type: TypeFact,
    },
    CheckedChildValue {
        envelope: Box<SqlExpr>,
        to_many: bool,
        r#type: TypeFact,
    },
    Quantified {
        value: Box<SqlExpr>,
        comparison: SqlFn,
        quantifier: Quantifier,
        subquery: Box<SqlQuery>,
        r#type: TypeFact,
    },
    ScalarSubquery {
        subquery: Box<SqlQuery>,
        r#type: TypeFact,
    },
    CheckedOne {
        list: Box<SqlExpr>,
        scalar_carrier: bool,
        at_least_only: bool,
        r#type: TypeFact,
    },
    CompactList {
        list: Box<SqlExpr>,
        r#type: TypeFact,
    },
    DeferredTdsString {
        inner: Box<SqlSelect>,
        alias: String,
        id: i32,
        form: TdsForm,
        render_tds_null: bool,
        r#type: TypeFact,
    },
    TempTableInSplice {
        temp_table_name: String,
        r#type: TypeFact,
    },
    JsonObject {
        kv: Vec<SqlExpr>,
        r#type: TypeFact,
    },
    JsonArray {
        elements: Vec<SqlExpr>,
        r#type: TypeFact,
    },
    JsonArrayAgg {
        value: Box<SqlExpr>,
        order_keys: Vec<JsonArrayAggKey>,
        r#type: TypeFact,
    },
    WindowCall {
        r#fn: Box<SqlAgg>,
        partition_by: Vec<SqlExpr>,
        order_by: Vec<SortKey>,
        frame: Option<Frame>,
        r#type: TypeFact,
    },
    Lambda(Lambda),
    Cast {
        value: Box<SqlExpr>,
        target: SqlType,
        conform: bool,
        r#type: TypeFact,
    },
    FoldCall {
        source: Box<SqlExpr>,
        lambda: Lambda,
        init: Box<SqlExpr>,
        acc_is_list: bool,
        homogeneous: bool,
        r#type: TypeFact,
    },
    /// An aggregate in a grouped select's projections or HAVING (Java's `SqlAgg.Reducer` is a `SqlExpr` too).
    Reducer(Reducer),
}

impl SqlExpr {
    /// The node's type fact (Java's `type()`).
    pub fn r#type(&self) -> &TypeFact {
        match self {
            SqlExpr::Column(c) => &c.r#type,
            SqlExpr::Lambda(l) => &l.r#type,
            SqlExpr::Reducer(r) => &r.r#type,
            SqlExpr::RowOrder { r#type, .. }
            | SqlExpr::StoredRead { r#type, .. }
            | SqlExpr::ReduceCollection { r#type, .. }
            | SqlExpr::Membership { r#type, .. }
            | SqlExpr::StarExcept { r#type, .. }
            | SqlExpr::Star { r#type, .. }
            | SqlExpr::StringLit { r#type, .. }
            | SqlExpr::IntLit { r#type, .. }
            | SqlExpr::FloatLit { r#type, .. }
            | SqlExpr::DecimalLit { r#type, .. }
            | SqlExpr::BoolLit { r#type, .. }
            | SqlExpr::NullLit { r#type }
            | SqlExpr::DateLit { r#type, .. }
            | SqlExpr::Group { r#type, .. }
            | SqlExpr::KeyCell { r#type, .. }
            | SqlExpr::PlanParam { r#type, .. }
            | SqlExpr::FormatLit { r#type, .. }
            | SqlExpr::DatePartLit { r#type, .. }
            | SqlExpr::JsonKindLit { r#type, .. }
            | SqlExpr::TypeNameLit { r#type, .. }
            | SqlExpr::TimestampLit { r#type, .. }
            | SqlExpr::TimeLit { r#type, .. }
            | SqlExpr::OrderedListAgg { r#type, .. }
            | SqlExpr::ArrayLit { r#type, .. }
            | SqlExpr::StructLit { r#type, .. }
            | SqlExpr::StructGet { r#type, .. }
            | SqlExpr::Call { r#type, .. }
            | SqlExpr::Case { r#type, .. }
            | SqlExpr::Exists { r#type, .. }
            | SqlExpr::InSubquery { r#type, .. }
            | SqlExpr::CheckedDefects { r#type, .. }
            | SqlExpr::CheckedChildValue { r#type, .. }
            | SqlExpr::Quantified { r#type, .. }
            | SqlExpr::ScalarSubquery { r#type, .. }
            | SqlExpr::CheckedOne { r#type, .. }
            | SqlExpr::CompactList { r#type, .. }
            | SqlExpr::DeferredTdsString { r#type, .. }
            | SqlExpr::TempTableInSplice { r#type, .. }
            | SqlExpr::JsonObject { r#type, .. }
            | SqlExpr::JsonArray { r#type, .. }
            | SqlExpr::JsonArrayAgg { r#type, .. }
            | SqlExpr::WindowCall { r#type, .. }
            | SqlExpr::Cast { r#type, .. }
            | SqlExpr::FoldCall { r#type, .. } => r#type,
        }
    }
}

/// Java's `SqlExpr.Column`: `table` absent for an unqualified reference.
#[derive(Debug, Clone, PartialEq)]
pub struct Column {
    pub table: Option<String>,
    pub name: String,
    pub r#type: TypeFact,
    pub origin: Option<Origin>,
}

/// Java's `SqlExpr.Lambda`.
#[derive(Debug, Clone, PartialEq)]
pub struct Lambda {
    pub params: Vec<String>,
    pub body: Box<SqlExpr>,
    pub r#type: TypeFact,
}

/// Java's `SqlExpr.StructLit.Field`.
#[derive(Debug, Clone, PartialEq)]
pub struct StructLitField {
    pub name: String,
    pub value: SqlExpr,
    pub declared: Option<SqlType>,
}

/// Java's `SqlExpr.Case.When`.
#[derive(Debug, Clone, PartialEq)]
pub struct When {
    pub condition: SqlExpr,
    pub then: SqlExpr,
}

/// Java's `SqlExpr.CheckedDefects.Hoist`.
#[derive(Debug, Clone, PartialEq)]
pub struct Hoist {
    pub path: Vec<String>,
    pub envelope: SqlExpr,
    pub to_many: bool,
}

/// Java's `SqlExpr.JsonArrayAgg.Key`.
#[derive(Debug, Clone, PartialEq)]
pub struct JsonArrayAggKey {
    pub expr: SqlExpr,
    pub desc: bool,
}

/// Java's `SqlExpr.WindowCall.Frame`.
#[derive(Debug, Clone, PartialEq)]
pub struct Frame {
    pub kind: FrameKind,
    pub from: Bound,
    pub to: Bound,
}

/// Java's `SqlExpr.WindowCall.Frame.Bound`.
#[derive(Debug, Clone, PartialEq)]
pub enum Bound {
    UnboundedPreceding,
    Preceding { n: JavaNumber },
    CurrentRow,
    Following { n: JavaNumber },
    UnboundedFollowing,
    IntervalPreceding { n: i64, unit: String },
    IntervalFollowing { n: i64, unit: String },
}

/// A window bound's offset, the Java `Number` it is: its class is kept, so the round trip is exact.
#[derive(Debug, Clone, PartialEq)]
pub enum JavaNumber {
    Long(i64),
    Integer(i32),
    Double(Double),
    BigDecimal(BigDecimal),
}

/// Java's `ValueTyping`: the kinds a value-typed parameter's values take, and the kind an absent one is a null of.
#[derive(Debug, Clone, PartialEq)]
pub struct ValueTyping {
    pub kinds: Vec<ValueKind>,
    pub absent: ValueKind,
}

/// Java's `SqlAgg`: a reducer (grouped or windowed), or a window-only ranking or value function.
#[derive(Debug, Clone, PartialEq)]
pub enum SqlAgg {
    Reducer(Reducer),
    RankingFn { r#fn: AggFn, args: Vec<SqlExpr> },
    ValueFn { r#fn: AggFn, args: Vec<SqlExpr> },
}

/// Java's `SqlAgg.Reducer`.
#[derive(Debug, Clone, PartialEq)]
pub struct Reducer {
    pub r#fn: AggFn,
    pub args: Vec<SqlExpr>,
    pub distinct: bool,
    pub order_by: Vec<SortKey>,
    pub r#type: TypeFact,
}

/// Java's `SqlType`: the SQL layer's own type vocabulary.
#[derive(Debug, Clone, PartialEq)]
pub enum SqlType {
    Scalar(Scalar),
    Decimal { precision: i32, scale: i32 },
    Array { element: Box<SqlType> },
    Map { key: Box<SqlType>, value: Box<SqlType> },
    Struct { fields: Vec<StructField> },
    Tagged { slots: Vec<TaggedSlot> },
}

/// Java's `SqlType.Struct.Field`.
#[derive(Debug, Clone, PartialEq)]
pub struct StructField {
    pub name: String,
    pub r#type: SqlType,
}

/// Java's `SqlType.Tagged.Slot`.
#[derive(Debug, Clone, PartialEq)]
pub struct TaggedSlot {
    pub kind: String,
    pub r#type: SqlType,
}

/// Java's `TypeFact`: what an expression is known to produce.
#[derive(Debug, Clone, PartialEq)]
pub enum TypeFact {
    Typed { r#type: SqlType, nullable: bool },
    Bottom,
    Raises,
    Unknown,
}

/// Java's `DateFmt`: one element of a typed date format.
#[derive(Debug, Clone, PartialEq)]
pub enum DateFmt {
    Part(DateFmtPart),
    Text { s: String },
}

/// A Java `double` as MirJson carries it: the text `Double.toString` wrote, kept to be written back, and its value.
#[derive(Debug, Clone, PartialEq)]
pub struct Double {
    text: String,
    value: f64,
}

impl Double {
    /// `text` read as Java's `Double.parseDouble` reads the text `Double.toString` writes.
    pub fn parse(text: &str) -> Result<Double, Error> {
        match text.parse::<f64>() {
            Ok(value) => Ok(Double { text: text.to_owned(), value }),
            Err(_) => Err(Error(format!("not a Java double: '{text}'"))),
        }
    }

    pub fn value(&self) -> f64 {
        self.value
    }

    /// The text it was read from: `Double.toString`'s.
    pub fn text(&self) -> &str {
        &self.text
    }
}

/// A Java `BigDecimal` as MirJson carries it: the text `BigDecimal.toString` wrote, exact (never through a double).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigDecimal {
    text: String,
}

impl BigDecimal {
    /// `text` in the grammar of Java's `new BigDecimal(String)`: a sign, digits with at most one point, and an
    /// exponent.
    pub fn parse(text: &str) -> Result<BigDecimal, Error> {
        let bad = || Error(format!("not a Java BigDecimal: '{text}'"));
        let unsigned = text.strip_prefix(['+', '-']).unwrap_or(text);
        let (significand, exponent) = match unsigned.find(['e', 'E']) {
            Some(i) => (&unsigned[..i], Some(&unsigned[i + 1..])),
            None => (unsigned, None),
        };
        let (int, frac) = significand.split_once('.').unwrap_or((significand, ""));
        let digits = |s: &str| s.bytes().all(|b| b.is_ascii_digit());
        if int.len() + frac.len() == 0 || !digits(int) || !digits(frac) {
            return Err(bad());
        }
        if let Some(e) = exponent {
            let e = e.strip_prefix(['+', '-']).unwrap_or(e);
            if e.is_empty() || !digits(e) || e.parse::<i32>().is_err() {
                return Err(bad());
            }
        }
        Ok(BigDecimal { text: text.to_owned() })
    }

    /// The text it was read from: `BigDecimal.toString`'s.
    pub fn as_str(&self) -> &str {
        &self.text
    }
}
