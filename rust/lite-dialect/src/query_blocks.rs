// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! Java's `QueryBlocks` (`core/src/main/java/com/legend/sql/dialect/QueryBlocks.java`, main's 2026-11-05; merged into
//! the port 2026-11-10): A STATEMENT'S QUERY BLOCKS, the scope of its aliases. A source alias is visible in the block
//! that defines it -- a common table expression's query, the statement's body, a top-level union's branch -- and in
//! the blocks nested inside that one, never in a sibling block. A pass that maps alias names to their sources maps them
//! per block: the lowering keeps aliases unique within one lowered statement, but a statement COMPOSED of several (the
//! database judge's fused verdict statement: each assert's rows a block, each lowered on its own, each counting its
//! aliases from `t0`) holds the same alias in two blocks, and a statement-wide map resolved one block's `"t3"` to the
//! other's (H2's corpus: `Column "t3.bookId" not found`).

use crate::refusal::Refusal;
use lite_sql::{Cte, SqlQuery, SqlUnion, SqlWith};

/// `q` with `per_block` applied to each of its top-level blocks: each common table expression's query and the body, a
/// union's branches each, recursively (Java's `QueryBlocks.each`).
pub fn each(
    q: SqlQuery,
    per_block: &mut dyn FnMut(SqlQuery) -> Result<SqlQuery, Refusal>,
) -> Result<SqlQuery, Refusal> {
    match q {
        SqlQuery::With(w) => {
            let mut ctes = Vec::with_capacity(w.ctes.len());
            for c in w.ctes {
                ctes.push(Cte {
                    query: each(c.query, per_block)?,
                    ..c
                });
            }
            let body = Box::new(each(*w.body, per_block)?);
            Ok(SqlQuery::With(SqlWith {
                ctes,
                body,
                recursive: w.recursive,
            }))
        }
        SqlQuery::Union(u) => {
            let mut branches = Vec::with_capacity(u.branches.len());
            for b in u.branches {
                branches.push(each(b, per_block)?);
            }
            Ok(SqlQuery::Union(SqlUnion { branches, ..u }))
        }
        select => per_block(select),
    }
}
