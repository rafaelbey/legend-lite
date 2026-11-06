// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! The engine's operator-part builders (core/'s `parser.OperatorParts`, a port of legend-engine's
//! `DomainParseTreeWalker` `booleanPart`/`arithmeticPart`/`processOp`/`buildArithmeticOp`/`buildComparisonOp`/
//! `buildDivide`): pure functions of the parts the expression parser's combined loop reads. The engine's
//! mis-association after a comparison is reproduced (`EngineQuirks.RELATIONAL_ARITH_MISASSOCIATION`, always true):
//! `1 < 2 + 3 * 4` is `lessThan(1, times[Collection[plus[Collection[2,3]], 4]])`.

use crate::jtypes::JStr;
use crate::nodes::{Span, collection, infix, vs_fn};
use crate::records::{AppliedFunction, ValueSpecification};

/// `EngineQuirks.RELATIONAL_ARITH_MISASSOCIATION`: decided in writing to reproduce the engine (2026-08-06).
const RELATIONAL_ARITH_MISASSOCIATION: bool = true;

/// One grammar `arithmeticPart` context: a same-op run or a single comparison, with the context's span (first
/// operator through the last operand's last token), which the engine stamps on the built call and its collection.
pub struct ArithPart {
    pub function: &'static str,
    pub comparison: bool,
    pub operands: Vec<ValueSpecification>,
    pub ctx_span: Span,
}

/// One grammar `booleanPart` context: `(AND|OR) expression`, spanning the operator token.
pub struct BoolPart {
    pub function: &'static str,
    pub operand: ValueSpecification,
    pub op_span: Span,
}

/// The threaded accumulator: the built node and the operator that built (or kept) it.
struct Acc {
    node: AppliedFunction,
    op: &'static str,
}

/// `booleanPart`: `&&` after an `or` accumulator rewrites its last parameter, so `a || b && c` is
/// `or(a, and(b, c))`.
pub fn boolean_part(parts: Vec<BoolPart>, input: ValueSpecification) -> AppliedFunction {
    let mut acc: Option<Acc> = None;
    let mut input = Some(input);
    for p in parts {
        acc = Some(match acc {
            None => {
                let f = p.function;
                Acc { node: build_boolean(p, input.take().expect("the input is used once")), op: f }
            }
            Some(a) if a.op == "or" && p.function == "and" => {
                let AppliedFunction { function, mut parameters, referents, pos, .. } = a.node;
                let last = parameters.pop().expect("a boolean call has two parameters");
                let first = parameters.into_iter().next().expect("a boolean call has two parameters");
                let new_af = build_boolean(p, last);
                let mut node = infix(function, vec![first, vs_fn(new_af)], pos);
                node.referents = referents;
                Acc { node, op: a.op }
            }
            Some(a) => {
                let f = p.function;
                Acc { node: build_boolean(p, vs_fn(a.node)), op: f }
            }
        });
    }
    acc.expect("booleanPart on empty group").node
}

fn build_boolean(p: BoolPart, input: ValueSpecification) -> AppliedFunction {
    infix(JStr::from(p.function), vec![input, p.operand], p.op_span)
}

/// `arithmeticPart`: a comparison folds the whole accumulator; plus/minus/times/divide thread through `processOp`.
pub fn arithmetic_part(parts: Vec<ArithPart>, initial: ValueSpecification) -> AppliedFunction {
    let mut acc: Option<Acc> = None;
    let mut initial = Some(initial);
    for p in parts {
        if p.comparison {
            let input = match acc.take() {
                None => initial.take().expect("the initial value is used once"),
                Some(a) => vs_fn(a.node),
            };
            let f = p.function;
            acc = Some(Acc { node: build_comparison(p, input), op: f });
        } else {
            acc = Some(process_op(p, acc.take(), &mut initial));
        }
    }
    acc.expect("arithmeticPart on empty group").node
}

/// `processOp`: a null accumulator builds; a strictly lower-precedence one has its last parameter rewritten;
/// otherwise the accumulator is the left value.
fn process_op(p: ArithPart, acc: Option<Acc>, initial: &mut Option<ValueSpecification>) -> Acc {
    let Some(a) = acc else {
        let f = p.function;
        return Acc { node: build_arith(p, initial.take().expect("the initial value is used once")), op: f };
    };
    if is_strictly_lower_precedence(a.op, p.function) {
        let AppliedFunction { function, mut parameters, referents, pos, .. } = a.node;
        if a.op == "divide" || (RELATIONAL_ARITH_MISASSOCIATION && is_relational_comparison(a.op)) {
            let last = parameters.pop().expect("a binary call has two parameters");
            let first = parameters.into_iter().next().expect("a binary call has two parameters");
            let new_af = build_arith(p, last);
            let mut node = infix(function, vec![first, vs_fn(new_af)], pos);
            node.referents = referents;
            return Acc { node, op: a.op };
        }
        // a collection carrier (plus/minus/times): the last element rebuilt, re-collected with the CLAIMING
        // context's span
        let ctx_span = p.ctx_span.clone();
        let mut vals = match parameters.into_iter().next() {
            Some(ValueSpecification::PureCollection(c)) => c.values,
            _ => panic!("ClassCastException: an arithmetic run's parameter is its collection"),
        };
        let last = vals.pop().expect("an arithmetic run has operands");
        let new_af = build_arith(p, last);
        vals.push(vs_fn(new_af));
        let mut node = infix(function, vec![collection(vals, ctx_span)], pos);
        node.referents = referents;
        return Acc { node, op: a.op };
    }
    let f = p.function;
    Acc { node: build_arith(p, vs_fn(a.node)), op: f }
}

/// `buildArithmeticOp`/`buildDivide`: plus/minus/times wrap every operand in one collection; divide left-folds
/// pairwise; both stamp the context's span on every node built.
fn build_arith(p: ArithPart, initial: ValueSpecification) -> AppliedFunction {
    if p.function == "divide" {
        let mut af: Option<AppliedFunction> = None;
        let mut left = initial;
        for other in p.operands {
            let built = infix(JStr::from("divide"), vec![left, other], p.ctx_span.clone());
            left = vs_fn(built.clone());
            af = Some(built);
        }
        return af.expect("divide part without operands");
    }
    let mut vals = Vec::with_capacity(1 + p.operands.len());
    vals.push(initial);
    vals.extend(p.operands);
    infix(JStr::from(p.function), vec![collection(vals, p.ctx_span.clone())], p.ctx_span)
}

fn build_comparison(p: ArithPart, initial: ValueSpecification) -> AppliedFunction {
    let first = p.operands.into_iter().next().expect("a comparison has its right side");
    infix(JStr::from(p.function), vec![initial, first], p.ctx_span)
}

fn is_relational_comparison(f: &str) -> bool {
    matches!(f, "lessThan" | "lessThanEqual" | "greaterThan" | "greaterThanEqual")
}

fn is_additive(f: &str) -> bool {
    matches!(f, "plus" | "minus")
}

fn is_product(f: &str) -> bool {
    matches!(f, "times" | "divide")
}

/// `isStrictlyLowerPrecedence`: relational below additive and product; additive below product.
fn is_strictly_lower_precedence(op1: &str, op2: &str) -> bool {
    (is_relational_comparison(op1) && (is_additive(op2) || is_product(op2))) || (is_additive(op1) && is_product(op2))
}
