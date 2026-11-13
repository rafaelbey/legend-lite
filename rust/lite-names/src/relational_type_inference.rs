// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! The SQL type of a relational operation element (2026-11-12, the names follow-up: Java's
//! `compiler/element/RelationalTypeInference`, moved into the names layer by the owner's ruling of 2026-11-12 -- its
//! readers in production are E's, `KnowledgeLayer.columnKind` and `ViewSignatures`, and H's `RelationalOpRows`): the
//! compiler's typing rule for mapping expressions and view column expressions (the engine's `inferRelationalType` and
//! `getSafeType`, relationalExtension.pure:111-190 and :1906-2010). `None` is Java's null: the element has no
//! inferable type (the engine's `failOnMatchFailure=false` branch).
//!
//! Ported line for line. Java's `int` arithmetic wraps (a decimal's digits, a concatenation's summed sizes), so the
//! port's does; a string literal's size is its UTF-16 length; a function name is lowered as `toLowerCase(Locale.ROOT)`
//! lowers it. A view column read through its expression is EXPANDING while its expression is typed: a chain of view
//! columns that reaches one of them again (a view-on-view cycle) is refused, a `ModelException` (MODEL) worded as
//! `ViewSignatures`' cycle refusal (2026-11-13, the names fixes, item 8, ruling 29: before, Java recursed until
//! `StackOverflowError` and the Rust process aborted). The set holds the view column records by identity, and a column
//! leaves it once typed: two reads of one column side by side are no cycle.

use std::sync::Arc;

use lite_base::IdSet;
use lite_error::Phase;
use lite_model::{
    DatabaseDefinition, DatabaseDefinitionViewDefinitionViewColumnMapping, JValue, RelationalDataType as T,
    RelationalDataTypeBit, RelationalDataTypeDecimal, RelationalDataTypeDouble_, RelationalDataTypeInteger_,
    RelationalDataTypeOther, RelationalDataTypeVarchar, RelationalOperation as Op, cat,
};

use crate::error::Thrown;
use crate::java_string;
use crate::model_index::StoreLookups;

/// The view columns being typed, by identity.
type Expanding = IdSet<Arc<DatabaseDefinitionViewDefinitionViewColumnMapping>>;

fn bit() -> T {
    T::RelationalDataTypeBit(Arc::new(RelationalDataTypeBit {}))
}

fn integer() -> T {
    T::RelationalDataTypeInteger_(Arc::new(RelationalDataTypeInteger_ {}))
}

fn double() -> T {
    T::RelationalDataTypeDouble_(Arc::new(RelationalDataTypeDouble_ {}))
}

fn other() -> T {
    T::RelationalDataTypeOther(Arc::new(RelationalDataTypeOther {}))
}

fn varchar(size: i32) -> T {
    T::RelationalDataTypeVarchar(Arc::new(RelationalDataTypeVarchar { size }))
}

fn decimal(precision: i32, scale: i32) -> T {
    T::RelationalDataTypeDecimal(Arc::new(RelationalDataTypeDecimal { precision, scale }))
}

/// `infer(op, db, ctx)`: the type of `op`; `db` scopes bare column references (a view's column expressions read their
/// own database), `ctx` resolves an op's own `[db]` qualifier (mapping expressions carry it). A view-on-view cycle is
/// refused (`Err`).
pub fn infer<'a>(
    op: &Op,
    db: Option<&'a DatabaseDefinition>,
    ctx: Option<&'a dyn StoreLookups>,
) -> Result<Option<T>, Thrown> {
    infer_in(op, db, ctx, &mut IdSet::new())
}

fn infer_in<'a>(
    op: &Op,
    db: Option<&'a DatabaseDefinition>,
    ctx: Option<&'a dyn StoreLookups>,
    expanding: &mut Expanding,
) -> Result<Option<T>, Thrown> {
    Ok(match op {
        Op::RelationalOperationColumnRef(c) => {
            column_type(db, ctx, c.database_name.as_ref(), &c.table, &c.column, expanding)?
        }
        Op::RelationalOperationFunctionCall(f) => {
            let lowered = java_string::to_lower_root(&f.name.0);
            let name = String::from_utf16(&lowered).unwrap_or_default();
            let first = |expanding: &mut Expanding| match f.args.first() {
                None => Ok(None),
                Some(a) => infer_in(a, db, ctx, expanding),
            };
            match name.as_str() {
                // aggregates carry their argument's type (engine inferDynaFunctionReturnType: max/min/sum/avg family);
                // max and min over SEVERAL arguments are the safe type over all of them when one exists, else the
                // first's -- the engine's two rules (relationalExtension.pure:1151-1197); until 2026-11-13 the first's
                // alone (the names fixes, item 11, ruling 28)
                "max" | "min" => {
                    let mut types: Vec<Option<T>> = Vec::with_capacity(f.args.len());
                    for arg in f.args.iter() {
                        types.push(infer_in(arg, db, ctx, expanding)?);
                    }
                    let first = types.first().cloned().flatten();
                    if types.len() <= 1 {
                        first
                    } else {
                        // an argument with no type drops out, as the engine's map does
                        let mut acc: Option<T> = None;
                        let mut possible = true;
                        for t in types.into_iter().flatten() {
                            match acc {
                                None => acc = Some(t),
                                Some(a) => {
                                    acc = safe(Some(a), Some(t));
                                    if acc.is_none() {
                                        possible = false;
                                        break;
                                    }
                                }
                            }
                        }
                        if possible && acc.is_some() { acc } else { first }
                    }
                }
                "distinct" => first(expanding)?,
                // sum/avg PROMOTE float-family to DOUBLE (engine inferDynaFunctionReturnType aggregation rule)
                "sum" | "average" => {
                    let at = first(expanding)?;
                    if matches!(at, Some(T::RelationalDataTypeFloat_(_) | T::RelationalDataTypeReal(_))) {
                        Some(double())
                    } else {
                        at
                    }
                }
                "count" => Some(integer()),
                // boolean dynafunctions spell BIT (engine H2)
                "and" | "or" | "not" | "equal" | "notequal" | "in" | "isnull" | "isnotnull" | "greaterthan"
                | "lessthan" | "greaterthanequal" | "lessthanequal" | "isempty" | "isnotempty" | "like"
                | "startswith" | "endswith" | "contains" => Some(bit()),
                // sqlNull carries the engine's OTHER type
                "sqlnull" => Some(other()),
                // engine inferDynaFunctionReturnType: sqlTrue/sqlFalse are BIT literals (ledger cluster 56)
                "sqltrue" | "sqlfalse" => Some(bit()),
                // string transforms keep their input's type
                "substring" | "left" | "right" | "trim" | "ltrim" | "rtrim" | "toupper" | "tolower" | "upper"
                | "lower" => first(expanding)?,
                "position" | "length" | "charindex" | "locate" | "indexof" => Some(integer()),
                // no typed operand, no type: never a default (until 2026-11-13 INTEGER -- the names fixes, item 10,
                // ruling 28; the engine's sub matches no rule then)
                "sub" => {
                    let mut acc: Option<T> = None;
                    for arg in f.args.iter() {
                        acc = safe(acc, infer_in(arg, db, ctx, expanding)?);
                    }
                    acc
                }
                // joinStrings AGGREGATES: the engine assigns the fixed 4000 buffer size
                "joinstrings" => Some(varchar(4000)),
                // string concatenation SUMS the operand sizes (engine getSize over concat)
                "concat" | "group_concat" => {
                    let mut size: i32 = 0;
                    for arg in f.args.iter() {
                        match infer_in(arg, db, ctx, expanding)? {
                            Some(T::RelationalDataTypeVarchar(v)) => size = size.wrapping_add(v.size),
                            Some(T::RelationalDataTypeChar_(c)) => size = size.wrapping_add(c.size),
                            _ => {}
                        }
                    }
                    Some(varchar(size))
                }
                // case(c1, v1, ..., else): the SAFE type over the value branches (engine getSafeType lattice)
                "case" => {
                    let mut acc: Option<T> = None;
                    let mut i = 1;
                    while i < f.args.len() {
                        acc = safe(acc, infer_in(&f.args[i], db, ctx, expanding)?);
                        i += 2;
                    }
                    if f.args.len() % 2 == 1 {
                        acc = safe(acc, infer_in(&f.args[f.args.len() - 1], db, ctx, expanding)?);
                    }
                    acc
                }
                // numeric operators widen positionally (engine math compatibility: DOUBLE beats INT; DECIMAL beats both)
                "plus" | "minus" | "times" | "divide" => {
                    let mut acc: Option<T> = None;
                    for arg in f.args.iter() {
                        acc = safe(acc, infer_in(arg, db, ctx, expanding)?);
                    }
                    acc
                }
                _ => None,
            }
        }
        Op::RelationalOperationJoinNavigation(j) => {
            let Some(terminal) = j.terminal.as_ref() else {
                return Ok(None);
            };
            // the nav's own [DB] qualifier scopes its TERMINAL (the terminal ColumnRef is stored db-relative --
            // adjudication ledger cluster 8: dropping it left the column untyped and concat inferred VARCHAR(n+0))
            let mut sub = db;
            if let (Some(name), Some(c)) = (&j.database_name, ctx)
                && let Some(jdb) = c.find_database(Some(name))
            {
                sub = Some(&**jdb);
            }
            infer_in(terminal, sub, ctx, expanding)?
        }
        Op::RelationalOperationGroup(g) => infer_in(&g.inner, db, ctx, expanding)?,
        Op::RelationalOperationLiteral(l) => match &l.value {
            JValue::String(s) => Some(varchar(s.0.len() as i32)),
            JValue::Integer(_) => Some(integer()),
            JValue::Long(_) => Some(integer()),
            JValue::Double(_) => Some(double()),
            JValue::Boolean(_) | JValue::BigInteger(_) | JValue::BigDecimal(_) => None,
        },
        // boolean expressions spell BIT (engine H2 boolean SQL type)
        Op::RelationalOperationComparison(_) => Some(bit()),
        Op::RelationalOperationBooleanOp(_) => Some(bit()),
        Op::RelationalOperationIsNull(_) => Some(bit()),
        Op::RelationalOperationIsNotNull(_) => Some(bit()),
        Op::RelationalOperationTargetColumnRef(_)
        | Op::RelationalOperationArrayLiteral(_)
        | Op::RelationalOperationLambda(_)
        | Op::RelationalOperationLambdaParam(_) => None,
    })
}

/// `safe(a, b)`: the engine's `getSafeType` pair rule (typeInference.pure): equal types keep; OTHER absorbs into the
/// other operand; DECIMAL beats the integer and float kinds as it is (never a string, a date or a bit: the engine's
/// `safeTypeMap` has no such pair -- until 2026-11-13 it beat any type, the names fixes, item 9); two decimals widen to
/// `DECIMAL(maxIntDigits + maxScale, maxScale)`; DOUBLE beats integers; two VARCHARs keep the larger size; two integer
/// kinds the wider; `None` operands pass the other side through; no safe type is `None` (never the first operand).
fn safe(a: Option<T>, b: Option<T>) -> Option<T> {
    let Some(a) = a else { return b };
    let Some(b) = b else { return Some(a) };
    if a == b {
        return Some(a);
    }
    // engine getSafeType: OTHER absorbs into the other operand -- ledger cluster 56
    if matches!(a, T::RelationalDataTypeOther(_)) {
        return Some(b);
    }
    if matches!(b, T::RelationalDataTypeOther(_)) {
        return Some(a);
    }
    match (decimal_of(&a), decimal_of(&b)) {
        (Some((ap, as_)), Some((bp, bs))) => {
            let int_digits = ap.wrapping_sub(as_).max(bp.wrapping_sub(bs));
            let scale = as_.max(bs);
            return Some(decimal(int_digits.wrapping_add(scale), scale));
        }
        (Some((ap, as_)), None) => return decimal_partner(&b).then(|| decimal(ap, as_)),
        (None, Some((bp, bs))) => return decimal_partner(&a).then(|| decimal(bp, bs)),
        (None, None) => {}
    }
    let float = |t: &T| {
        matches!(
            t,
            T::RelationalDataTypeDouble_(_) | T::RelationalDataTypeFloat_(_) | T::RelationalDataTypeReal(_)
        )
    };
    if float(&a) || float(&b) {
        return Some(double());
    }
    if let (T::RelationalDataTypeVarchar(va), T::RelationalDataTypeVarchar(vb)) = (&a, &b) {
        return Some(varchar(va.size.max(vb.size)));
    }
    match (int_widening(&a), int_widening(&b)) {
        (Some(ra), Some(rb)) => Some(if ra >= rb { a } else { b }),
        // NO safe type (the engine's isSafeTypePossible is false: an INT branch against a VARCHAR branch, a DATE
        // against a TIMESTAMP) -- the engine ASSERTS there; this platform types the element as nothing
        _ => None,
    }
}

/// The engine's integer widening order (safeTypeMap): `INT_WIDENING.indexOf(t.getClass())`.
fn int_widening(t: &T) -> Option<usize> {
    match t {
        T::RelationalDataTypeTinyInt(_) => Some(0),
        T::RelationalDataTypeSmallInt(_) => Some(1),
        T::RelationalDataTypeInteger_(_) => Some(2),
        T::RelationalDataTypeBigInt(_) => Some(3),
        _ => None,
    }
}

/// `DECIMAL_PARTNERS.contains(t.getClass())`: the kinds a DECIMAL (or NUMERIC) is safe with besides another decimal --
/// the engine's `safeTypeMap` lists Decimal and Numeric among the partners of TinyInt, SmallInt, Integer, BigInt, Float
/// and Double alone.
fn decimal_partner(t: &T) -> bool {
    matches!(
        t,
        T::RelationalDataTypeTinyInt(_)
            | T::RelationalDataTypeSmallInt(_)
            | T::RelationalDataTypeInteger_(_)
            | T::RelationalDataTypeBigInt(_)
            | T::RelationalDataTypeFloat_(_)
            | T::RelationalDataTypeDouble_(_)
    )
}

/// `decimalOf(t)`: a decimal's or numeric's precision and scale.
fn decimal_of(t: &T) -> Option<(i32, i32)> {
    match t {
        T::RelationalDataTypeDecimal(d) => Some((d.precision, d.scale)),
        T::RelationalDataTypeNumeric(n) => Some((n.precision, n.scale)),
        _ => None,
    }
}

/// `columnType(scope, ctx, opDb, table, column)`: the declared type of `table.column` -- in the op's OWN database when
/// its `[db]` names one, else the scoping database -- through the one table lookup (include closure, schema-qualified
/// spellings, exact names); a VIEW's column resolves THROUGH its column expression (view-on-view), the column EXPANDING
/// while it is typed (a cycle is refused). The op's own name is tried first as spelled: an UNQUALIFIED `[db]` resolves
/// to nothing and falls to the scope.
fn column_type<'a>(
    scope: Option<&'a DatabaseDefinition>,
    ctx: Option<&'a dyn StoreLookups>,
    op_db: Option<&lite_model::JStr>,
    table: &lite_model::JStr,
    column: &lite_model::JStr,
    expanding: &mut Expanding,
) -> Result<Option<T>, Thrown> {
    let mut db = scope;
    if let (Some(name), Some(c)) = (op_db, ctx) {
        db = c.find_database(Some(name)).map(|d| &**d).or(db);
    }
    let (Some(db), Some(ctx)) = (db, ctx) else {
        return Ok(None);
    };
    let db_fqn = &db.qualified_name;
    if let Some(td) = ctx.find_table_definition(db_fqn, table) {
        return Ok(td
            .columns
            .iter()
            .find(|c| &c.name == column)
            .map(|c| c.data_type.clone()));
    }
    if let Some(view) = ctx.find_view(db_fqn, table) {
        for cm in view.column_mappings.iter() {
            if &cm.name == column {
                if !expanding.insert(cm.clone()) {
                    return Err(Thrown::model(
                        Phase::Model,
                        cat!(
                            "view '",
                            &view.name,
                            "' of '",
                            db_fqn,
                            "' reads its own columns (cyclic view-on-view chain)"
                        ),
                        None,
                    ));
                }
                // Java's try/finally: the column leaves the set whatever the answer
                let out = infer_in(&cm.expression, Some(db), Some(ctx), expanding);
                expanding.remove(cm);
                return out;
            }
        }
    }
    Ok(None)
}
