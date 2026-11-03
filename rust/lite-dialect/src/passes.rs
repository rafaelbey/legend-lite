// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! The MIR passes the DuckDB dialect runs, in one place: each a named stub that hands the statement back unchanged,
//! for the next step of the port to fill in with its Java body (`core/src/main/java/com/legend/sql/dialect/<Name>.java`).
//! The order they run in is [`crate::ansi::render_passes`]'s (Java's `AnsiSqlRenderer.renderPasses`): the Float
//! expansions, the dialect's `passes()` ([`crate::duckdb::DuckDb`]'s: Java's `DuckDb.passes`, line 675), then
//! `StoredReads`.
//!
//! [`rust/lite-dialect/PARITY.md`](../PARITY.md) measures what each stub costs on the golden dump.

use crate::refusal::Refusal;
use crate::rewriter::SqlRewriter;
use lite_sql::{ColumnType, SqlQuery};

/// A stub pass: its Java class's name, and the statement handed back as it came.
macro_rules! stub_pass {
    ($(#[$meta:meta])* $name:ident, $java:literal) => {
        $(#[$meta])*
        #[derive(Debug, Default)]
        pub struct $name;

        impl SqlRewriter for $name {
            fn name(&self) -> &'static str {
                $java
            }

            /// A stub: the statement as it came (the Java body is the next step's).
            fn rewrite_root(&mut self, q: SqlQuery) -> Result<SqlQuery, Refusal> {
                Ok(q)
            }
        }
    };
}

stub_pass!(
    /// The carrier strategy pass (Java's `CarrierStrategies`, over DuckDB's `CAPS` -- all four capabilities): semantic
    /// collection nodes become the dialect's emission. Runs first of the dialect's passes on every dialect.
    CarrierStrategies,
    "CarrierStrategies"
);
stub_pass!(
    /// DuckDB's quantile family takes no within-group order: the percentile reducer's order key spelled.
    QuantileOrder,
    "QuantileOrder"
);
stub_pass!(
    /// DuckDB's PIVOT forbids qualified column references inside ON and USING.
    UnqualifyPivotArgs,
    "UnqualifyPivotArgs"
);
stub_pass!(
    /// DuckDB's fold encoding: a `FoldCall` becomes a `LIST_REDUCE` call tree.
    FoldToListReduce,
    "FoldToListReduce"
);
stub_pass!(
    /// DuckDB's spelling of the checked-envelope nodes (`CheckedDefects`, `CheckedChildValue`) as list lambdas.
    CheckedDefectsToLists,
    "CheckedDefectsToLists"
);
stub_pass!(
    /// DuckDB's SUBSTRING start clamp.
    SubstringClamp,
    "SubstringClamp"
);
stub_pass!(
    /// The engine corpus's scan order made explicit: engine-corpus compatibility only, run when the JVM's
    /// `legend.exec.engineScanOrder` property is set (here, [`crate::duckdb::DuckDb::engine_scan_order`]).
    StableScanOrder,
    "StableScanOrder"
);
stub_pass!(
    /// A LEFT JOIN condition's scalar subquery of the null-supplying side filters that side before the join.
    OuterJoinSubqueryFilters,
    "OuterJoinSubqueryFilters"
);
stub_pass!(
    /// At the statement's root, a star over a source holding a stored Variant column is spelled into its columns.
    VariantEgress,
    "VariantEgress"
);

/// A constant GROUP BY (or ORDER BY) key rendered as a typed expression (Java's `ConstantKeysAsExpressions`);
/// DuckDB's runs over GROUP BY's integers alone (`ConstantKeysAsExpressions.groupByOnly()`).
#[derive(Debug, Default)]
pub struct ConstantKeysAsExpressions {
    /// Java's `orderToo`: every constant key of GROUP BY and ORDER BY, not GROUP BY's integers alone.
    pub order_too: bool,
}

impl ConstantKeysAsExpressions {
    /// Java's `groupByOnly()`: DuckDB's.
    pub fn group_by_only() -> ConstantKeysAsExpressions {
        ConstantKeysAsExpressions { order_too: false }
    }
}

impl SqlRewriter for ConstantKeysAsExpressions {
    fn name(&self) -> &'static str {
        "ConstantKeysAsExpressions"
    }

    fn rewrite_root(&mut self, q: SqlQuery) -> Result<SqlQuery, Refusal> {
        Ok(q)
    }
}

/// The first pass of a dialect that does not spell `FLOAT_PRINT` itself (Java's `FloatPrintText.Expansion`): each
/// call becomes [`crate::float_text::print_of`], so every later pass sees the expanded tree. A stub: the renderer's
/// `FLOAT_PRINT` arm writes the same tree when `bind` is unset, but the later passes do not see it.
#[derive(Debug)]
pub struct FloatPrintExpansion {
    /// Java's `bind`: the value and its text read from a one-row derived table, each written once.
    pub bind: bool,
    /// Java's `fresh`: the derived tables' alias counter (`__fpN`), per pass object.
    pub fresh: u32,
}

impl FloatPrintExpansion {
    pub fn new(bind: bool) -> FloatPrintExpansion {
        FloatPrintExpansion { bind, fresh: 0 }
    }
}

impl SqlRewriter for FloatPrintExpansion {
    fn name(&self) -> &'static str {
        "FloatPrintText.Expansion"
    }

    fn rewrite_root(&mut self, q: SqlQuery) -> Result<SqlQuery, Refusal> {
        Ok(q)
    }
}

/// The first pass of a dialect that does not spell `FLOAT_CANON` itself (Java's `FloatCanonText.Expansion`): each
/// call becomes [`crate::float_text::canon_of`]. A stub, as [`FloatPrintExpansion`] is.
#[derive(Debug)]
pub struct FloatCanonExpansion {
    /// Java's `bindText`: the double's text read from a one-row derived table, written once.
    pub bind_text: bool,
    /// Java's `fresh`: the derived tables' alias counter (`__fcN`), per pass object.
    pub fresh: u32,
}

impl FloatCanonExpansion {
    pub fn new(bind_text: bool) -> FloatCanonExpansion {
        FloatCanonExpansion { bind_text, fresh: 0 }
    }
}

impl SqlRewriter for FloatCanonExpansion {
    fn name(&self) -> &'static str {
        "FloatCanonText.Expansion"
    }

    fn rewrite_root(&mut self, q: SqlQuery) -> Result<SqlQuery, Refusal> {
        Ok(q)
    }
}

/// The stored reads (Java's `StoredReads`), registered last on every dialect: a table column whose declared store
/// type the dialect reads other than as held (`reads`) is read so at every reference.
#[derive(Debug)]
pub struct StoredReads {
    /// Java's `reads`: the dialect's `readsStored`.
    pub reads: fn(&ColumnType) -> bool,
}

impl StoredReads {
    pub fn new(reads: fn(&ColumnType) -> bool) -> StoredReads {
        StoredReads { reads }
    }
}

impl SqlRewriter for StoredReads {
    fn name(&self) -> &'static str {
        "StoredReads"
    }

    fn rewrite_root(&mut self, q: SqlQuery) -> Result<SqlQuery, Refusal> {
        Ok(q)
    }
}

/// DuckDB's `passes()` (Java's `DuckDb.passes`, line 675): carrier strategies first, then its structural rewrites,
/// `VariantEgress` last of them (`StoredReads` follows, in `render_passes`).
pub fn duckdb_passes(engine_scan_order: bool) -> Vec<Box<dyn SqlRewriter>> {
    let mut ps: Vec<Box<dyn SqlRewriter>> = vec![
        Box::new(CarrierStrategies),
        Box::new(QuantileOrder),
        Box::new(UnqualifyPivotArgs),
        Box::new(FoldToListReduce),
        Box::new(CheckedDefectsToLists),
        Box::new(SubstringClamp),
        Box::new(ConstantKeysAsExpressions::group_by_only()),
        // a LEFT JOIN condition's scalar subquery of the null-supplying side filters that side before the join
        Box::new(OuterJoinSubqueryFilters),
    ];
    if engine_scan_order {
        ps.push(Box::new(StableScanOrder));
    }
    // last of this dialect's passes, on the statement root only: a stored Variant leaving through a star read as JSON
    ps.push(Box::new(VariantEgress));
    ps
}
