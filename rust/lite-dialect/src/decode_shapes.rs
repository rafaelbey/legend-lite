// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! Java's `DecodeShapes` (`core/src/main/java/com/legend/sql/DecodeShapes.java`), the part the engine text reads: the
//! structure of a literal-DECODE case chain (a mapping's enum decode: every branch `col = literal -> 'NAME'`, a null
//! terminal). Its other functions (`codesAndNames`, `sourceColumn`, `stripDecodes`) serve the lowering and the plan
//! text, which stay on the JVM.

use lite_sql::{SqlExpr, SqlFn, When};

/// The chain's branches, each a condition and its string literal, through its nested `otherwise`s; `None` when `e` is
/// no literal-decode case (Java's `Optional.empty()`).
pub fn flatten_decode(e: &SqlExpr) -> Option<Vec<&When>> {
    let mut out = Vec::new();
    let mut cur = e;
    while let SqlExpr::Case { whens, otherwise, .. } = cur {
        for w in whens {
            if !matches!(w.then, SqlExpr::StringLit { .. }) {
                return None;
            }
            out.push(w);
        }
        match otherwise {
            None => return if out.is_empty() { None } else { Some(out) },
            Some(o) => cur = o,
        }
    }
    (matches!(cur, SqlExpr::NullLit { .. }) && !out.is_empty()).then_some(out)
}

/// The one source expression every branch's condition compares (`src = literal`, or an OR of such equalities over the
/// same `src`); `None` otherwise. Two sources are one when they are equal records (Java's `equals`).
pub fn source_expr(e: &SqlExpr) -> Option<&SqlExpr> {
    let flat = flatten_decode(e)?;
    let mut src: Option<&SqlExpr> = None;
    for w in flat {
        let left = condition_source(&w.condition)?;
        match src {
            None => src = Some(left),
            Some(s) if s != left => return None,
            Some(_) => {}
        }
    }
    src
}

/// The shared left side of an equality, or of an OR-tree of equalities over the same left side.
fn condition_source(cond: &SqlExpr) -> Option<&SqlExpr> {
    match cond {
        SqlExpr::Call { r#fn: SqlFn::Equal, args, .. } if args.len() == 2 => Some(&args[0]),
        SqlExpr::Call { r#fn: SqlFn::Or, args, .. } => {
            let mut src: Option<&SqlExpr> = None;
            for arm in args {
                let a = condition_source(arm)?;
                if src.is_some_and(|s| s != a) {
                    return None;
                }
                src = Some(a);
            }
            src
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::build::{call, case, derived, int_lit, null_lit, string_lit, when};

    fn eq(col: &str, v: i64) -> SqlExpr {
        call(SqlFn::Equal, vec![derived(Some("t"), col), int_lit(v)])
    }

    #[test]
    fn a_decode_chain_flattens_through_its_otherwise_and_names_its_one_source() {
        // CASE WHEN t.c = 1 THEN 'A' ELSE CASE WHEN t.c = 2 OR t.c = 3 THEN 'B' ELSE NULL END END
        let inner = case(vec![when(call(SqlFn::Or, vec![eq("c", 2), eq("c", 3)]), string_lit("B"))], Some(null_lit()));
        let chain = case(vec![when(eq("c", 1), string_lit("A"))], Some(inner));
        assert_eq!(flatten_decode(&chain).map(|ws| ws.len()), Some(2));
        assert_eq!(source_expr(&chain), Some(&derived(Some("t"), "c")));
        // no otherwise: the branches it has
        let open = case(vec![when(eq("c", 1), string_lit("A"))], None);
        assert_eq!(flatten_decode(&open).map(|ws| ws.len()), Some(1));
        // a branch that is no string literal, a terminal that is no NULL, two sources: not a decode over one source
        assert!(flatten_decode(&case(vec![when(eq("c", 1), int_lit(1))], None)).is_none());
        assert!(flatten_decode(&case(vec![when(eq("c", 1), string_lit("A"))], Some(string_lit("Z")))).is_none());
        let two = case(vec![when(eq("c", 1), string_lit("A")), when(eq("d", 2), string_lit("B"))], None);
        assert!(flatten_decode(&two).is_some());
        assert_eq!(source_expr(&two), None);
        // any other expression is no chain
        assert!(flatten_decode(&eq("c", 1)).is_none());
        assert_eq!(source_expr(&null_lit()), None);
    }
}
