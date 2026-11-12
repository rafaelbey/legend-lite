// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! `RelOpFromProtocol`: a protocol relational operation (`PRelOp`, the engine's flat wire shape, every operator a
//! `dynaFunc`) to the model's `RelationalOperation`, whose comparisons, connectives and null tests are typed variants
//! re-derived from the closed vocabulary of names below; anything else stays a `FunctionCall`, as it always has.

use std::sync::Arc;

use lite_front::jtypes::JStr;
use lite_front::records as p;

use crate::error::{Thrown, illegal_argument};
use crate::linked::list;
use crate::names::bare;
use crate::records::{
    ComparisonOp, JoinChainElement, LogicalOp, RelationalOperation as R, RelationalOperationArrayLiteral,
    RelationalOperationBooleanOp, RelationalOperationColumnRef, RelationalOperationComparison,
    RelationalOperationFunctionCall, RelationalOperationGroup, RelationalOperationIsNotNull, RelationalOperationIsNull,
    RelationalOperationJoinNavigation, RelationalOperationLambda, RelationalOperationLambdaParam,
    RelationalOperationLiteral, RelationalOperationTargetColumnRef,
};
use crate::text::is;

/// `RelOpFromProtocol.op(p, enclosingDb)`: `enclosing_db` is the database whose body the operation appears in; a
/// reference to it is null in the model, as the source wrote it.
pub fn op(p: &p::PRelOp, enclosing_db: Option<&JStr>) -> Result<R, Thrown> {
    Ok(match p {
        p::PRelOp::PColumnRef(c) => column_ref(c, enclosing_db),
        p::PRelOp::PRelLiteral(l) => {
            R::RelationalOperationLiteral(Arc::new(RelationalOperationLiteral { value: l.value.clone() }))
        }
        p::PRelOp::PRelLiteralList(l) => {
            let mut elements = Vec::with_capacity(l.values.len());
            for v in &l.values {
                elements.push(op(v, enclosing_db)?);
            }
            R::RelationalOperationArrayLiteral(Arc::new(RelationalOperationArrayLiteral {
                elements: list(elements),
            }))
        }
        p::PRelOp::PElemtWithJoins(j) => join_navigation(j, enclosing_db)?,
        p::PRelOp::PDynaFunc(f) => dyna_func(f, enclosing_db)?,
        p::PRelOp::PRelLambda(l) => R::RelationalOperationLambda(Arc::new(RelationalOperationLambda {
            parameters: list(l.parameter_names.clone()),
            body: op(&l.body, enclosing_db)?,
        })),
        p::PRelOp::PLambdaParam(lp) => {
            R::RelationalOperationLambdaParam(Arc::new(RelationalOperationLambdaParam { name: lp.name.clone() }))
        }
    })
}

/// `{target}.COL`: the sentinel table of a self-join's far side; everything else a plain column, its table as written
/// (`schema.table` for a named schema), quotes off.
fn column_ref(c: &p::PColumnRef, enclosing_db: Option<&JStr>) -> R {
    let t = &c.table;
    if is(&t.table.0, "{target}") || is(&c.table_alias.0, "target") {
        return R::RelationalOperationTargetColumnRef(Arc::new(RelationalOperationTargetColumnRef {
            column: c.column.clone(),
        }));
    }
    let db = match &t.database {
        Some(d) if Some(d) == enclosing_db => None,
        other => other.clone(),
    };
    let table = if is(&t.schema.0, "default") {
        bare(&t.table)
    } else {
        crate::cat!(bare(&t.schema), ".", bare(&t.table))
    };
    R::RelationalOperationColumnRef(Arc::new(RelationalOperationColumnRef {
        database_name: db,
        table,
        column: bare(&c.column),
    }))
}

/// A join navigation: the join type a hop carries qualifies that hop; the terminal is read against the navigation's
/// own root database, and a navigation rooted in the enclosing database is written bare.
fn join_navigation(j: &p::PElemtWithJoins, enclosing_db: Option<&JStr>) -> Result<R, Thrown> {
    let mut chain = Vec::with_capacity(j.joins.len());
    let mut db: Option<JStr> = None;
    for ptr in &j.joins {
        if db.is_none() {
            db = ptr.db.clone();
        }
        chain.push(Arc::new(JoinChainElement {
            join_name: ptr.name.clone(),
            join_type: match &ptr.join_type {
                None => None,
                Some(t) => Some(crate::names::join_type_from_identifier(t)?),
            },
            database_name: ptr.db.clone(),
            include_self: false,
        }));
    }
    let nav_db = if db.is_some() {
        db.clone()
    } else {
        enclosing_db.cloned()
    };
    if db.is_some() && db.as_ref() == enclosing_db {
        db = None;
    }
    let terminal = match &j.relational_element {
        None => None,
        Some(e) => Some(op(e, nav_db.as_ref())?),
    };
    navigation(db, chain, terminal)
}

/// `new RelationalOperation.JoinNavigation(db, chain, terminal)`: a chain is never empty.
pub fn navigation(
    database_name: Option<JStr>,
    chain: Vec<Arc<JoinChainElement>>,
    terminal: Option<R>,
) -> Result<R, Thrown> {
    if chain.is_empty() {
        return Err(illegal_argument(JStr::from("Join chain cannot be empty")));
    }
    Ok(R::RelationalOperationJoinNavigation(Arc::new(
        RelationalOperationJoinNavigation {
            database_name,
            chain: list(chain),
            terminal,
        },
    )))
}

/// The closed operator vocabulary; any other name is a function call.
fn dyna_func(f: &p::PDynaFunc, enclosing_db: Option<&JStr>) -> Result<R, Thrown> {
    let mut args = Vec::with_capacity(f.parameters.len());
    for a in &f.parameters {
        args.push(op(a, enclosing_db)?);
    }
    let name = &f.func_name.0;
    if let Some(cmp) = comparison(name)
        && args.len() == 2
    {
        let right = args.pop().expect("two");
        let left = args.pop().expect("two");
        return Ok(R::RelationalOperationComparison(Arc::new(
            RelationalOperationComparison { left, op: cmp, right },
        )));
    }
    if (is(name, "and") || is(name, "or")) && args.len() >= 2 {
        let lop = if is(name, "and") { LogicalOp::AND } else { LogicalOp::OR };
        // the wire flattens a same-operator run into one n-ary dynaFunc; the model is binary and right-associative
        let mut acc = args.pop().expect("at least two");
        while let Some(left) = args.pop() {
            acc = R::RelationalOperationBooleanOp(Arc::new(RelationalOperationBooleanOp {
                left,
                op: lop,
                right: acc,
            }));
        }
        return Ok(acc);
    }
    if args.len() == 1 {
        if is(name, "isNull") {
            let operand = args.pop().expect("one");
            return Ok(R::RelationalOperationIsNull(Arc::new(RelationalOperationIsNull {
                operand,
            })));
        }
        if is(name, "isNotNull") {
            let operand = args.pop().expect("one");
            return Ok(R::RelationalOperationIsNotNull(Arc::new(
                RelationalOperationIsNotNull { operand },
            )));
        }
        if is(name, "group") {
            let inner = args.pop().expect("one");
            return Ok(R::RelationalOperationGroup(Arc::new(RelationalOperationGroup {
                inner,
            })));
        }
    }
    Ok(R::RelationalOperationFunctionCall(Arc::new(
        RelationalOperationFunctionCall {
            name: f.func_name.clone(),
            args: list(args),
        },
    )))
}

fn comparison(name: &[u16]) -> Option<ComparisonOp> {
    if is(name, "equal") {
        Some(ComparisonOp::EQ)
    } else if is(name, "notEqual") || is(name, "notEqualAnsi") {
        Some(ComparisonOp::NEQ)
    } else if is(name, "lessThan") {
        Some(ComparisonOp::LT)
    } else if is(name, "lessThanEqual") {
        Some(ComparisonOp::LTE)
    } else if is(name, "greaterThan") {
        Some(ComparisonOp::GT)
    } else if is(name, "greaterThanEqual") {
        Some(ComparisonOp::GTE)
    } else {
        None
    }
}
