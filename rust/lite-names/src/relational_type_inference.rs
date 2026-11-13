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
//! lowers it. **No cycle guard, as in Java**: a computed view column whose expression reaches its own view through a
//! column reference recurses until the stack is gone (Java's `StackOverflowError`; the Rust process aborts) -- listed for
//! the owner, not changed.

use std::sync::Arc;

use lite_model::{
    DatabaseDefinition, JValue, RelationalDataType as T, RelationalDataTypeBit, RelationalDataTypeDecimal,
    RelationalDataTypeDouble_, RelationalDataTypeInteger_, RelationalDataTypeOther, RelationalDataTypeVarchar,
    RelationalOperation as Op,
};

use crate::java_string;
use crate::model_index::StoreLookups;

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
/// own database), `ctx` resolves an op's own `[db]` qualifier (mapping expressions carry it).
pub fn infer<'a>(op: &Op, db: Option<&'a DatabaseDefinition>, ctx: Option<&'a dyn StoreLookups>) -> Option<T> {
    match op {
        Op::RelationalOperationColumnRef(c) => column_type(db, ctx, c.database_name.as_ref(), &c.table, &c.column),
        Op::RelationalOperationFunctionCall(f) => {
            let lowered = java_string::to_lower_root(&f.name.0);
            let name = String::from_utf16(&lowered).unwrap_or_default();
            let first = || f.args.first().and_then(|a| infer(a, db, ctx));
            match name.as_str() {
                // aggregates carry their argument's type (engine inferDynaFunctionReturnType: max/min/sum/avg family)
                "max" | "min" | "distinct" => first(),
                // sum/avg PROMOTE float-family to DOUBLE (engine inferDynaFunctionReturnType aggregation rule)
                "sum" | "average" => {
                    let at = first();
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
                | "lower" => first(),
                "position" | "length" | "charindex" | "locate" | "indexof" => Some(integer()),
                "sub" => {
                    let mut acc: Option<T> = None;
                    for arg in f.args.iter() {
                        acc = safe(acc, infer(arg, db, ctx));
                    }
                    Some(acc.unwrap_or_else(integer))
                }
                // joinStrings AGGREGATES: the engine assigns the fixed 4000 buffer size
                "joinstrings" => Some(varchar(4000)),
                // string concatenation SUMS the operand sizes (engine getSize over concat)
                "concat" | "group_concat" => {
                    let mut size: i32 = 0;
                    for arg in f.args.iter() {
                        match infer(arg, db, ctx) {
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
                        acc = safe(acc, infer(&f.args[i], db, ctx));
                        i += 2;
                    }
                    if f.args.len() % 2 == 1 {
                        acc = safe(acc, infer(&f.args[f.args.len() - 1], db, ctx));
                    }
                    acc
                }
                // numeric operators widen positionally (engine math compatibility: DOUBLE beats INT; DECIMAL beats both)
                "plus" | "minus" | "times" | "divide" => {
                    let mut acc: Option<T> = None;
                    for arg in f.args.iter() {
                        acc = safe(acc, infer(arg, db, ctx));
                    }
                    acc
                }
                _ => None,
            }
        }
        Op::RelationalOperationJoinNavigation(j) => {
            let terminal = j.terminal.as_ref()?;
            // the nav's own [DB] qualifier scopes its TERMINAL (the terminal ColumnRef is stored db-relative --
            // adjudication ledger cluster 8: dropping it left the column untyped and concat inferred VARCHAR(n+0))
            let mut sub = db;
            if let (Some(name), Some(c)) = (&j.database_name, ctx)
                && let Some(jdb) = c.find_database(Some(name))
            {
                sub = Some(&**jdb);
            }
            infer(terminal, sub, ctx)
        }
        Op::RelationalOperationGroup(g) => infer(&g.inner, db, ctx),
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
    }
}

/// `safe(a, b)`: the engine's `getSafeType` pair rule (typeInference.pure): equal types keep; OTHER absorbs into the
/// other operand; DECIMAL beats int, double and float as it is; two decimals widen to
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
        (Some((ap, as_)), None) => return Some(decimal(ap, as_)),
        (None, Some((bp, bs))) => return Some(decimal(bp, bs)),
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
/// spellings, exact names); a VIEW's column resolves THROUGH its column expression (view-on-view). The op's own name is
/// tried first as spelled: an UNQUALIFIED `[db]` resolves to nothing and falls to the scope.
fn column_type<'a>(
    scope: Option<&'a DatabaseDefinition>,
    ctx: Option<&'a dyn StoreLookups>,
    op_db: Option<&lite_model::JStr>,
    table: &lite_model::JStr,
    column: &lite_model::JStr,
) -> Option<T> {
    let mut db = scope;
    if let (Some(name), Some(c)) = (op_db, ctx) {
        db = c.find_database(Some(name)).map(|d| &**d).or(db);
    }
    let (Some(db), Some(ctx)) = (db, ctx) else {
        return None;
    };
    let db_fqn = &db.qualified_name;
    if let Some(td) = ctx.find_table_definition(db_fqn, table) {
        return td
            .columns
            .iter()
            .find(|c| &c.name == column)
            .map(|c| c.data_type.clone());
    }
    if let Some(view) = ctx.find_view(db_fqn, table) {
        for cm in view.column_mappings.iter() {
            if &cm.name == column {
                return infer(&cm.expression, Some(db), Some(ctx));
            }
        }
    }
    None
}
