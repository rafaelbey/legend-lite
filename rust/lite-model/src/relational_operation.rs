// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! `RelationalOperation`'s reads (2026-11-12, the names slice, docs/rust-port/PHASE3.md): the structural recursion's
//! children, the pre-order walk every reader of a relational expression shares, and the two facts the model index
//! reads off a view's columns (`navigatesJoin`, `tables`). A join navigation's hop chain is metadata, not a subtree:
//! only its terminal is beneath it. The recursion's other half, `withChildren` and `mapChildren` (2026-11-14, the
//! normalizer slice's model methods, docs/rust-port/EF_CENSUS.md §8).

use std::convert::Infallible;
use std::sync::Arc;

use lite_front::jtypes::JStr;

use crate::linked::list;
use crate::records::{
    ClassMappingPurePropertyBinding, RelationalOperation as Op, RelationalOperationArrayLiteral,
    RelationalOperationBooleanOp, RelationalOperationComparison, RelationalOperationFunctionCall,
    RelationalOperationGroup, RelationalOperationIsNotNull, RelationalOperationIsNull,
    RelationalOperationJoinNavigation, RelationalOperationLambda,
};

impl Op {
    /// `children()`: the direct operation children, in rebuild order.
    pub fn children(&self) -> Vec<&Op> {
        match self {
            Op::RelationalOperationColumnRef(_)
            | Op::RelationalOperationTargetColumnRef(_)
            | Op::RelationalOperationLiteral(_)
            | Op::RelationalOperationLambdaParam(_) => Vec::new(),
            Op::RelationalOperationFunctionCall(f) => f.args.iter().collect(),
            Op::RelationalOperationComparison(c) => vec![&c.left, &c.right],
            Op::RelationalOperationBooleanOp(b) => vec![&b.left, &b.right],
            Op::RelationalOperationIsNull(n) => vec![&n.operand],
            Op::RelationalOperationIsNotNull(n) => vec![&n.operand],
            Op::RelationalOperationGroup(g) => vec![&g.inner],
            Op::RelationalOperationArrayLiteral(a) => a.elements.iter().collect(),
            Op::RelationalOperationJoinNavigation(j) => j.terminal.iter().collect(),
            Op::RelationalOperationLambda(l) => vec![&l.body],
        }
    }

    /// `withChildren(cs)`: this node with its direct children replaced (`cs` in [`children`](Self::children)' order),
    /// every other component kept; a leaf, and a join navigation with no terminal, answer themselves (the very `Arc`).
    /// An index past `cs` is Java's `IndexOutOfBoundsException`, a panic here (a caller's bug: `map_children` never
    /// makes one).
    pub fn with_children(&self, cs: Vec<Op>) -> Op {
        let at = |i: usize| -> Op {
            cs.get(i)
                .unwrap_or_else(|| {
                    panic!(
                        "java.lang.IndexOutOfBoundsException: Index {i} out of bounds for length {}",
                        cs.len()
                    )
                })
                .clone()
        };
        match self {
            Op::RelationalOperationColumnRef(_)
            | Op::RelationalOperationTargetColumnRef(_)
            | Op::RelationalOperationLiteral(_) => self.clone(),
            Op::RelationalOperationFunctionCall(f) => {
                Op::RelationalOperationFunctionCall(Arc::new(RelationalOperationFunctionCall {
                    name: f.name.clone(),
                    args: list(cs),
                }))
            }
            Op::RelationalOperationComparison(c) => {
                Op::RelationalOperationComparison(Arc::new(RelationalOperationComparison {
                    left: at(0),
                    op: c.op,
                    right: at(1),
                }))
            }
            Op::RelationalOperationBooleanOp(b) => {
                Op::RelationalOperationBooleanOp(Arc::new(RelationalOperationBooleanOp {
                    left: at(0),
                    op: b.op,
                    right: at(1),
                }))
            }
            Op::RelationalOperationIsNull(_) => {
                Op::RelationalOperationIsNull(Arc::new(RelationalOperationIsNull { operand: at(0) }))
            }
            Op::RelationalOperationIsNotNull(_) => {
                Op::RelationalOperationIsNotNull(Arc::new(RelationalOperationIsNotNull { operand: at(0) }))
            }
            Op::RelationalOperationGroup(_) => {
                Op::RelationalOperationGroup(Arc::new(RelationalOperationGroup { inner: at(0) }))
            }
            Op::RelationalOperationArrayLiteral(_) => {
                Op::RelationalOperationArrayLiteral(Arc::new(RelationalOperationArrayLiteral { elements: list(cs) }))
            }
            Op::RelationalOperationJoinNavigation(j) => match &j.terminal {
                None => self.clone(),
                Some(_) => Op::RelationalOperationJoinNavigation(Arc::new(RelationalOperationJoinNavigation {
                    database_name: j.database_name.clone(),
                    chain: j.chain.clone(),
                    terminal: Some(at(0)),
                })),
            },
            Op::RelationalOperationLambda(l) => Op::RelationalOperationLambda(Arc::new(RelationalOperationLambda {
                parameters: l.parameters.clone(),
                body: at(0),
            })),
            Op::RelationalOperationLambdaParam(_) => self.clone(),
        }
    }

    /// `mapChildren(f)`, fallible: `f` over every direct child in order (the first `Err` returned at once, as Java's
    /// throw leaves the loop). Java answers the very object when `f` answered every child's very object (`r == c`); here
    /// `f` answers `None` for "this child, unchanged" and `Some(r)` for a different object, even an equal one, and this
    /// answers `Ok(None)` for Java's `this` (a leaf included) and `Ok(Some(rebuilt))` through
    /// [`with_children`](Self::with_children) otherwise, the unchanged children their very `Arc`s. (Said structurally,
    /// as `ValueSpecification::try_map_children` says it, whose crate reaches no identity; no rebuild here drops a
    /// component, so the two answers differ in identity alone.)
    pub fn try_map_children<E>(&self, mut f: impl FnMut(&Op) -> Result<Option<Op>, E>) -> Result<Option<Op>, E> {
        let cs = self.children();
        if cs.is_empty() {
            return Ok(None);
        }
        let mut rw = Vec::with_capacity(cs.len());
        let mut same = true;
        for c in cs {
            match f(c)? {
                None => rw.push(c.clone()),
                Some(r) => {
                    same = false;
                    rw.push(r);
                }
            }
        }
        Ok(if same { None } else { Some(self.with_children(rw)) })
    }

    /// `mapChildren(f)` for an `f` that cannot fail: [`try_map_children`](Self::try_map_children)'s answer.
    pub fn map_children(&self, mut f: impl FnMut(&Op) -> Option<Op>) -> Option<Op> {
        let Ok(answer) = self.try_map_children(|c| Ok::<_, Infallible>(f(c)));
        answer
    }

    /// `descendants()`: this operation and every operation beneath it, pre-order.
    pub fn descendants(&self) -> Vec<&Op> {
        let mut out = Vec::new();
        let mut stack = vec![self];
        while let Some(o) = stack.pop() {
            out.push(o);
            let cs = o.children();
            stack.extend(cs.into_iter().rev());
        }
        out
    }

    /// `navigatesJoin()`: whether a join is navigated anywhere in this expression.
    pub fn navigates_join(&self) -> bool {
        self.descendants()
            .into_iter()
            .any(|o| matches!(o, Op::RelationalOperationJoinNavigation(_)))
    }

    /// `tables()`: every table this expression's column references read, as spelled, once each, in reading order.
    pub fn tables(&self) -> Vec<JStr> {
        let mut out: Vec<JStr> = Vec::new();
        for o in self.descendants() {
            if let Op::RelationalOperationColumnRef(cr) = o
                && !out.contains(&cr.table)
            {
                out.push(cr.table.clone());
            }
        }
        out
    }
}

impl ClassMappingPurePropertyBinding {
    /// `withExpression(e)`: this binding with its expression replaced, every other component kept.
    pub fn with_expression(&self, e: lite_front::records::ValueSpecification) -> Arc<ClassMappingPurePropertyBinding> {
        Arc::new(ClassMappingPurePropertyBinding {
            property_name: self.property_name.clone(),
            expression: e,
            source_set_id: self.source_set_id.clone(),
            target_set_id: self.target_set_id.clone(),
            explode: self.explode,
            local: self.local,
            enum_mapping_id: self.enum_mapping_id.clone(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lite_front::codec;
    use lite_front::jtypes::JValue;

    use crate::records::{
        ComparisonOp, JoinChainElement, LogicalOp, RelationalOperationColumnRef, RelationalOperationLambdaParam,
        RelationalOperationLiteral,
    };

    fn col(table: &str, column: &str) -> Op {
        Op::RelationalOperationColumnRef(Arc::new(RelationalOperationColumnRef {
            database_name: Some(JStr::from("db::D")),
            table: JStr::from(table),
            column: JStr::from(column),
        }))
    }

    fn lit(n: i64) -> Op {
        Op::RelationalOperationLiteral(Arc::new(RelationalOperationLiteral { value: JValue::Long(n) }))
    }

    fn eq(l: Op, r: Op) -> Op {
        Op::RelationalOperationComparison(Arc::new(RelationalOperationComparison {
            left: l,
            op: ComparisonOp::EQ,
            right: r,
        }))
    }

    /// Every read of table `T` moved to `V`, every other node through `map_children`.
    fn rebase(o: &Op) -> Option<Op> {
        if let Op::RelationalOperationColumnRef(c) = o
            && c.table == "T"
        {
            return Some(Op::RelationalOperationColumnRef(Arc::new(
                RelationalOperationColumnRef {
                    table: JStr::from("V"),
                    ..(**c).clone()
                },
            )));
        }
        o.map_children(rebase)
    }

    fn json(o: &Op) -> String {
        codec::write(o, false)
    }

    #[test]
    fn a_rebuild_keeps_every_other_component() {
        let tree = Op::RelationalOperationBooleanOp(Arc::new(RelationalOperationBooleanOp {
            left: eq(col("T", "A"), lit(1)),
            op: LogicalOp::OR,
            right: Op::RelationalOperationFunctionCall(Arc::new(RelationalOperationFunctionCall {
                name: JStr::from("concat"),
                args: Arc::from(vec![col("U", "B"), col("T", "C")]),
            })),
        }));
        let after = rebase(&tree).expect("changed");
        assert_eq!(json(&after), json(&tree).replace("\"table\":\"T\"", "\"table\":\"V\""));
        assert_eq!(after.tables(), vec![JStr::from("V"), JStr::from("U")]);
    }

    #[test]
    fn an_unchanged_tree_answers_none() {
        let tree = Op::RelationalOperationGroup(Arc::new(RelationalOperationGroup {
            inner: Op::RelationalOperationIsNull(Arc::new(RelationalOperationIsNull { operand: col("U", "A") })),
        }));
        assert_eq!(rebase(&tree), None);
        assert_eq!(
            lit(1).map_children(|_| Some(lit(2))),
            None,
            "a leaf has no child to change"
        );
    }

    #[test]
    fn a_join_navigation_rebuilds_its_terminal_alone() {
        let hop = Arc::new(JoinChainElement {
            join_name: JStr::from("J"),
            join_type: None,
            database_name: None,
            include_self: false,
        });
        let bare = Op::RelationalOperationJoinNavigation(Arc::new(RelationalOperationJoinNavigation {
            database_name: Some(JStr::from("db::D")),
            chain: Arc::from(vec![hop.clone()]),
            terminal: None,
        }));
        assert_eq!(bare.map_children(|_| Some(lit(0))), None, "no terminal, no child");
        let nav = Op::RelationalOperationJoinNavigation(Arc::new(RelationalOperationJoinNavigation {
            terminal: Some(col("T", "A")),
            database_name: Some(JStr::from("db::D")),
            chain: Arc::from(vec![hop]),
        }));
        let after = rebase(&nav).expect("changed");
        assert_eq!(json(&after), json(&nav).replace("\"table\":\"T\"", "\"table\":\"V\""));
    }

    #[test]
    fn a_lambda_and_an_array_rebuild_and_the_first_refusal_is_answered() {
        let lambda = Op::RelationalOperationLambda(Arc::new(RelationalOperationLambda {
            parameters: Arc::from(vec![JStr::from("x")]),
            body: eq(
                Op::RelationalOperationLambdaParam(Arc::new(RelationalOperationLambdaParam { name: JStr::from("x") })),
                col("T", "A"),
            ),
        }));
        let after = rebase(&lambda).expect("changed");
        assert_eq!(
            json(&after),
            json(&lambda).replace("\"table\":\"T\"", "\"table\":\"V\"")
        );
        let array = Op::RelationalOperationArrayLiteral(Arc::new(RelationalOperationArrayLiteral {
            elements: Arc::from(vec![lit(1), col("T", "A"), lit(3)]),
        }));
        let mut seen = 0;
        let refused: Result<Option<Op>, &str> = array.try_map_children(|c| {
            seen += 1;
            match c {
                Op::RelationalOperationColumnRef(_) => Err("a column"),
                _ => Ok(None),
            }
        });
        assert_eq!((refused, seen), (Err("a column"), 2));
    }
}
