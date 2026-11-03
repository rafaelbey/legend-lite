// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! legend-lite's SQL tree, the MIR, ported from `core/src/main/java/com/legend/sql` with its JSON codec,
//! `core/src/main/java/com/legend/executionplan/MirJson.java` (docs/rust-port/PHASE1.md): the JVM hands a dialect
//! each statement as MirJson text, and Rust reads it. What the port keeps exactly:
//!
//! - every record, field for field (`mir`, `stmt`), and every Java enum the MIR names, constant for constant, with
//!   its `name()` (`names`);
//! - MirJson's wire: its tags, its field names and their order, null written for an absent value, a long or an int
//!   as a JSON integer, and a double, a BigDecimal and a window bound's number as the text Java wrote -- so for any
//!   MirJson Java wrote, `write_x(&read_x(o)?)` is `o` again, byte for byte;
//! - its refusals: an unknown tag or constant is refused by name, worded as Java's `named` words it.
//!
//! A MirJson tree nests deep: parse it with `lite_json::parse_with_depth(text, MAX_DEPTH)`.

mod mir;
mod names;
mod read;
mod stmt;
mod write;

pub use lite_json::Error;
pub use mir::{
    BigDecimal, Bound, Column, Cte, DateFmt, Double, Frame, Hoist, JavaNumber, JsonArrayAggKey, Lambda, NameQuoting,
    OutputCol, PivotUsing, Projection, Reducer, SortKey, SqlAgg, SqlExpr, SqlQuery, SqlSelect, SqlSource, SqlType,
    SqlUnion, SqlWith, StructField, StructLitField, TaggedSlot, TypeFact, ValueTyping, When,
};
pub use names::{
    AggFn, CatalogGrid, ColumnKind, DateFmtPart, DatePart, FrameKind, JoinKind, JsonKind, NullOrder, Origin, ParamKind,
    Quantifier, Scalar, SqlFn, TdsForm, ValueKind,
};
pub use read::{
    EXPR_TAGS, read_agg, read_column_type, read_ddl, read_dml, read_expr, read_fact, read_query, read_source, read_type,
};
pub use stmt::{Assignment, ColumnType, DdlColumn, SqlDdl, SqlDml};
pub use write::{
    expr_tag, write_agg, write_column_type, write_ddl, write_dml, write_expr, write_fact, write_query, write_source,
    write_type,
};

/// The nesting a MirJson document is parsed to (Java's `PlanJson` reads a plan with the same limit).
pub const MAX_DEPTH: usize = 4096;

#[cfg(test)]
mod tests;
