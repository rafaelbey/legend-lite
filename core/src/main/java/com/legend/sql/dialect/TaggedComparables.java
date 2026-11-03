// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

package com.legend.sql.dialect;

import com.legend.sql.SqlExpr;
import com.legend.sql.SqlQuery;

/**
 * legend-engine has no tagged values: its text of a mixed selection is the plain selection over the comparables
 * ({@link SqlExpr.Tagged#comparable}; the corpus's {@code sqlstring::testGreatestLeast}). The engine-text dialects send
 * the Rust library each query with every tagged value replaced by its comparable, since the plan's MirJson writes no
 * tagged value (it is resolved before a plan is written). Measured 2026-11-03 on the one engine-text query of the suite
 * that holds one: the replaced query's text is exactly the text Java's {@code EngineStyleH2.tagged} wrote
 * (docs/rust-port/PHASE1.md §4 step 4; rust/oracles/mir/tagged-engine.jsonl).
 */
final class TaggedComparables extends com.legend.sql.SqlRewriter {

    private TaggedComparables() {
    }

    /** {@code query} with each tagged value as its comparable; the query itself when it holds none. */
    static SqlQuery replaced(SqlQuery query) {
        return new TaggedComparables().rewrite(query);
    }

    @Override
    protected SqlExpr expr(SqlExpr e) {
        return e instanceof SqlExpr.Tagged t ? t.comparable() : e;
    }
}
