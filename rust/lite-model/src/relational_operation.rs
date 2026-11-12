// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! `RelationalOperation`'s reads (2026-11-12, the names slice, docs/rust-port/PHASE3.md): the structural recursion's
//! children, the pre-order walk every reader of a relational expression shares, and the two facts the model index
//! reads off a view's columns (`navigatesJoin`, `tables`). A join navigation's hop chain is metadata, not a subtree:
//! only its terminal is beneath it.

use std::sync::Arc;

use lite_front::jtypes::JStr;

use crate::records::{ClassMappingPurePropertyBinding, RelationalOperation as Op};

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
