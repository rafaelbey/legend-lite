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
//! port's does; a string literal's size is its UTF-16 length; a function name is matched exactly, as the engine's map
//! `get` matches it (until 2026-11-14 lowered as `toLowerCase(Locale.ROOT)` lowers it: S378, the names batch, item 3). A view column read through its expression is EXPANDING while its expression is typed: a chain of view
//! columns that reaches one of them again (a view-on-view cycle) is refused, a `ModelException` (MODEL) worded as
//! `ViewSignatures`' cycle refusal (2026-11-13, the names fixes, item 8, ruling 29: before, Java recursed until
//! `StackOverflowError` and the Rust process aborted). The set holds the view column records by identity, and a column
//! leaves it once typed: two reads of one column side by side are no cycle.

use std::sync::Arc;

use lite_base::IdSet;
use lite_error::Phase;
use lite_model::{
    DatabaseDefinition, DatabaseDefinitionViewDefinitionViewColumnMapping, JValue, RelationalDataType as T,
    RelationalDataTypeBigInt, RelationalDataTypeBit, RelationalDataTypeDate_, RelationalDataTypeDecimal,
    RelationalDataTypeDouble_, RelationalDataTypeFloat_, RelationalDataTypeInteger_, RelationalDataTypeNumeric,
    RelationalDataTypeOther, RelationalDataTypeTimestamp, RelationalDataTypeVarchar, RelationalOperation as Op, cat,
};

use crate::error::Thrown;
use crate::model_index::StoreLookups;

/// The view columns being typed, by identity.
type Expanding = IdSet<Arc<DatabaseDefinitionViewDefinitionViewColumnMapping>>;

fn bit() -> T {
    T::RelationalDataTypeBit(Arc::new(RelationalDataTypeBit {}))
}

fn integer() -> T {
    T::RelationalDataTypeInteger_(Arc::new(RelationalDataTypeInteger_ {}))
}

fn big_int() -> T {
    T::RelationalDataTypeBigInt(Arc::new(RelationalDataTypeBigInt {}))
}

fn double() -> T {
    T::RelationalDataTypeDouble_(Arc::new(RelationalDataTypeDouble_ {}))
}

fn float() -> T {
    T::RelationalDataTypeFloat_(Arc::new(RelationalDataTypeFloat_ {}))
}

fn other() -> T {
    T::RelationalDataTypeOther(Arc::new(RelationalDataTypeOther {}))
}

fn date() -> T {
    T::RelationalDataTypeDate_(Arc::new(RelationalDataTypeDate_ {}))
}

fn timestamp() -> T {
    T::RelationalDataTypeTimestamp(Arc::new(RelationalDataTypeTimestamp {}))
}

/// A CHAR or a VARCHAR: the kinds the engine's `getSize` and its size matches read (relationalExtension.pure:2005-2011).
fn is_char_or_varchar(t: &T) -> bool {
    matches!(t, T::RelationalDataTypeChar_(_) | T::RelationalDataTypeVarchar(_))
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
            // the name matched exactly, as the engine's map get matches it (relationalExtension.pure:114): each arm
            // spells the map's key, a name spelled otherwise has no rule; until 2026-11-14 lower-cased first (S378, the
            // names batch, item 3, ruling 34). A name that is not UTF-16 text matches no key
            let name = String::from_utf16(&f.name.0).unwrap_or_default();
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
                        // an argument with no type drops out, as the engine's map does; the safe type is the
                        // engine's fold (S378, 2026-11-13, ruling 32)
                        safe_type_of(types).or(first)
                    }
                }
                "distinct" => first(expanding)?,
                // sum by its first argument's raw kind, the engine's five rules (relationalExtension.pure:1717-1741):
                // TINYINT, SMALLINT and INTEGER an INTEGER, BIGINT a BIGINT, a DECIMAL or a NUMERIC its own kind with its
                // precision capped at 31, FLOAT and DOUBLE a DOUBLE; any other kind (REAL among them) matches no rule, no
                // type. Until 2026-11-13 its first argument's type, a FLOAT or REAL one DOUBLE (S378, ruling 34; the
                // follow-ups' first version read four rules, its fifth missed, and typed a FLOAT or DOUBLE nothing)
                "sum" => match first(expanding)? {
                    Some(
                        T::RelationalDataTypeTinyInt(_)
                        | T::RelationalDataTypeSmallInt(_)
                        | T::RelationalDataTypeInteger_(_),
                    ) => Some(integer()),
                    Some(T::RelationalDataTypeBigInt(_)) => Some(big_int()),
                    Some(at @ (T::RelationalDataTypeDecimal(_) | T::RelationalDataTypeNumeric(_))) => Some(capped(&at)),
                    Some(T::RelationalDataTypeFloat_(_) | T::RelationalDataTypeDouble_(_)) => Some(double()),
                    _ => None,
                },
                // average and divide are DOUBLE whatever their arguments, which the engine's one rule each does not read
                // (relationalExtension.pure:291-300, :693-702); until 2026-11-13 average kept its first argument's type,
                // a FLOAT or REAL one DOUBLE, and divide was the safe type over its arguments (S378, ruling 34)
                "average" | "divide" => Some(double()),
                // count is BIGINT at any arity (relationalExtension.pure:543-551); until 2026-11-13 INTEGER (S378,
                // ruling 32)
                "count" => Some(big_int()),
                // boolean dynafunctions spell BIT (engine H2). like has no rule in the engine's map, so no type; until
                // 2026-11-14 BIT, a rule only lite had (S378, the names batch, item 2, ruling 34)
                "and" | "or" | "not" | "equal" | "notEqual" | "in" | "isNull" | "isNotNull" | "greaterThan"
                | "lessThan" | "greaterThanEqual" | "lessThanEqual" | "isEmpty" | "isNotEmpty" | "startsWith"
                | "endsWith" | "contains" => Some(bit()),
                // sqlNull carries the engine's OTHER type
                "sqlNull" => Some(other()),
                // engine inferDynaFunctionReturnType: sqlTrue/sqlFalse are BIT literals (ledger cluster 56)
                "sqlTrue" | "sqlFalse" => Some(bit()),
                // substring, left, right, trim, ltrim and rtrim are a VARCHAR sized by their first argument's CHAR
                // or VARCHAR (relationalExtension.pure:1051-1059, :1121-1129, :1509-1517, :1547-1555, :1647-1655,
                // :1853-1861), another first argument no type; until 2026-11-13 they kept their first argument's
                // type (S378, ruling 32)
                "substring" | "left" | "right" | "trim" | "ltrim" | "rtrim" => match first(expanding)? {
                    Some(T::RelationalDataTypeVarchar(v)) => Some(varchar(v.size)),
                    Some(T::RelationalDataTypeChar_(c)) => Some(varchar(c.size)),
                    _ => None,
                },
                // toUpper and toLower keep their first argument's type (:1793-1801, :1843-1851); upper and lower have no
                // rule in the engine's map, so no type (until 2026-11-13 they kept their first argument's type: S378,
                // ruling 34)
                "toUpper" | "toLower" => first(expanding)?,
                // length and indexOf are INTEGER whatever their arguments (relationalExtension.pure:1061-1069,
                // :961-969); position, charindex and locate have no rule in the engine's map, so no type; until 2026-11-14
                // INTEGER, rules only lite had (S378, the names batch, item 2, ruling 34)
                "length" | "indexOf" => Some(integer()),
                // sub is its first argument's type when its first two have one raw kind, else no rule matches
                // (relationalExtension.pure:1637-1645): no type, never a default. Until 2026-11-13 the safe type over
                // all its arguments (S378, ruling 32; its INTEGER default went with the names fixes, item 10)
                "sub" => {
                    if f.args.len() < 2 {
                        None
                    } else {
                        let first = infer_in(&f.args[0], db, ctx, expanding)?;
                        let second = infer_in(&f.args[1], db, ctx, expanding)?;
                        match (first, second) {
                            (Some(a), Some(b)) if std::mem::discriminant(&a) == std::mem::discriminant(&b) => Some(a),
                            _ => None,
                        }
                    }
                }
                // joinStrings AGGREGATES: the engine assigns the fixed 4000 buffer size
                "joinStrings" => Some(varchar(4000)),
                // string concatenation SUMS the operand sizes (engine getSize over concat); an operand typed other than
                // CHAR or VARCHAR leaves it no type -- the engine's one rule asks every typed operand to be one
                // (relationalExtension.pure:463-471); an operand with no type drops out. Until 2026-11-13 such an
                // operand added 0 (the names fixes, item 10, ruling 28)
                // group_concat has no rule in the engine's map (no type: until 2026-11-13 it was typed as a concat,
                // S378, ruling 32)
                "concat" => {
                    let mut size: i32 = 0;
                    let mut other = false;
                    for arg in f.args.iter() {
                        match infer_in(arg, db, ctx, expanding)? {
                            Some(T::RelationalDataTypeVarchar(v)) => size = size.wrapping_add(v.size),
                            Some(T::RelationalDataTypeChar_(c)) => size = size.wrapping_add(c.size),
                            Some(_) => other = true,
                            None => {}
                        }
                    }
                    if other { None } else { Some(varchar(size)) }
                }
                // case(c1, v1, ..., else): the engine's safe type over the value branches and the last argument
                // (relationalExtension.pure:402-411: the branches at 1, 3, ... below 2 * floor((n - 1) / 2), then the
                // last). Where none is possible the engine ASSERTS, whatever failOnMatchFailure is: its inference fails
                // where the engine runs it, at plan generation (relationalMappingExecution.pure:205-208), never when the
                // database compiles. Lite runs it when the model compiles and when it seeds the metamodel, where a
                // failure would refuse or break what the engine compiles: no type, register row S378's form lite cannot
                // spell (2026-11-13, ruling 34; Java's comment says more)
                "case" => {
                    let mut types: Vec<Option<T>> = Vec::new();
                    let mut i = 1;
                    while i < f.args.len() {
                        types.push(infer_in(&f.args[i], db, ctx, expanding)?);
                        i += 2;
                    }
                    if f.args.len() % 2 == 1 {
                        types.push(infer_in(&f.args[f.args.len() - 1], db, ctx, expanding)?);
                    }
                    safe_type_of(types)
                }
                // the numeric operators, each by the engine's own rules (S378, 2026-11-13, ruling 34; until then the
                // safe type over every argument, a rule only lite had). plus over ONE argument widens it
                // (relationalExtension.pure:1408-1436: TINYINT and SMALLINT an INTEGER, INTEGER and BIGINT a BIGINT, a
                // DECIMAL or NUMERIC its own kind capped at precision 31, FLOAT and DOUBLE a DOUBLE, another kind no
                // rule); plus and minus over several are their FIRST argument's type when a safe type over all of them
                // is possible, else no rule (:1408-1436, :1209-1218); times is the safe type of its first two
                // arguments, the rest unread (:1773-1782). divide is DOUBLE, above.
                "plus" if f.args.len() == 1 => match infer_in(&f.args[0], db, ctx, expanding)? {
                    Some(T::RelationalDataTypeTinyInt(_) | T::RelationalDataTypeSmallInt(_)) => Some(integer()),
                    Some(T::RelationalDataTypeInteger_(_) | T::RelationalDataTypeBigInt(_)) => Some(big_int()),
                    Some(at @ (T::RelationalDataTypeDecimal(_) | T::RelationalDataTypeNumeric(_))) => Some(capped(&at)),
                    Some(T::RelationalDataTypeFloat_(_) | T::RelationalDataTypeDouble_(_)) => Some(double()),
                    _ => None,
                },
                "plus" | "minus" => first_when_safe(&f.args, db, ctx, expanding)?,
                "times" => {
                    if f.args.len() < 2 {
                        None
                    } else {
                        let t0 = infer_in(&f.args[0], db, ctx, expanding)?;
                        let t1 = infer_in(&f.args[1], db, ctx, expanding)?;
                        match (t0, t1) {
                            (Some(a), Some(b)) => safe_type_of([Some(a), Some(b)]),
                            _ => None,
                        }
                    }
                }
                // ---- the 30 rules the seeded Database texts use (2026-11-14, the names batch, item 1: register row
                // S378, ruling 35; until then no rule, no type), each legend-engine 4.145.0's map entry at the lines
                // cited, every arm's answer asked of the engine (Java's comment says how). Where the engine's inference
                // fails, no type (S378).
                //
                // One type whatever the arguments, which the rule never reads: bitAnd :321-329, bitShiftLeft :351-359,
                // bitXor :371-379, dayOfMonth :633-641, dayOfYear :663-671, mod :1219-1227 (and :1479-1487, the same),
                // monthNumber :1259-1267, sign :1587-1595, weekOfYear :1883-1891 an INTEGER; cbrt :415-423, log
                // :1101-1109, sin :1597-1605, sqrt :1687-1695 a DOUBLE; datePart :623-631, firstDayOfMonth :763-771,
                // firstDayOfQuarter :773-781, firstDayOfWeek :813-821, firstDayOfYear :823-831 a DATE; firstHourOfDay
                // :833-841, firstMinuteOfHour :843-851, firstSecondOfMinute :853-861 a TIMESTAMP; convertVarchar128
                // :503-511 a VARCHAR(128), md5 :1165-1173 a VARCHAR(32), sha256 :1577-1585 a VARCHAR(64)
                "bitAnd" | "bitShiftLeft" | "bitXor" | "dayOfMonth" | "dayOfYear" | "mod" | "monthNumber" | "sign"
                | "weekOfYear" => Some(integer()),
                "cbrt" | "log" | "sin" | "sqrt" => Some(double()),
                "datePart" | "firstDayOfMonth" | "firstDayOfQuarter" | "firstDayOfWeek" | "firstDayOfYear" => {
                    Some(date())
                }
                "firstHourOfDay" | "firstMinuteOfHour" | "firstSecondOfMinute" => Some(timestamp()),
                "convertVarchar128" => Some(varchar(128)),
                "md5" => Some(varchar(32)),
                "sha256" => Some(varchar(64)),
                // abs is its one argument's type, whatever its kind (:193-201); with none or several, no type
                "abs" => {
                    if f.args.len() == 1 {
                        infer_in(&f.args[0], db, ctx, expanding)?
                    } else {
                        None
                    }
                }
                // add's two rules over its first two arguments, the first that matches answering (:213-225): a safe
                // type that is a VARCHAR, a VARCHAR of the two sizes summed (a CHAR's or VARCHAR's alone: an OTHER fails
                // getSize, no type); another safe type, itself; none possible, no rule. Both typed, and two of them
                "add" => {
                    if f.args.len() < 2 {
                        None
                    } else {
                        let t0 = infer_in(&f.args[0], db, ctx, expanding)?;
                        let t1 = infer_in(&f.args[1], db, ctx, expanding)?;
                        match (t0, t1) {
                            (Some(a), Some(b)) => match safe_type_of([Some(a.clone()), Some(b.clone())]) {
                                Some(T::RelationalDataTypeVarchar(_)) => {
                                    if is_char_or_varchar(&a) && is_char_or_varchar(&b) {
                                        Some(varchar(size(&a).wrapping_add(size(&b))))
                                    } else {
                                        None
                                    }
                                }
                                safe => safe,
                            },
                            _ => None,
                        }
                    }
                }
                // coalesce is the safe type over every argument when one is possible, else no rule (:453-461): an
                // argument with no type fails the engine's inference (failOnMatchFailure true), no type -- not dropped,
                // as case's untyped branch is (S378, the owner's) -- and no argument fails its fold's head, no type
                "coalesce" => {
                    let mut types: Vec<Option<T>> = Vec::with_capacity(f.args.len());
                    for arg in f.args.iter() {
                        types.push(infer_in(arg, db, ctx, expanding)?);
                    }
                    if types.iter().any(Option::is_none) {
                        None
                    } else {
                        safe_type_of(types)
                    }
                }
                // if is the safe type of its second and third arguments when one is possible, else no rule (:941-949);
                // the condition and a fourth argument unread; both typed, and three arguments at least
                "if" => {
                    if f.args.len() < 3 {
                        None
                    } else {
                        let t1 = infer_in(&f.args[1], db, ctx, expanding)?;
                        let t2 = infer_in(&f.args[2], db, ctx, expanding)?;
                        match (t1, t2) {
                            (Some(a), Some(b)) => safe_type_of([Some(a), Some(b)]),
                            _ => None,
                        }
                    }
                }
                // lpad is a VARCHAR sized by its second argument when that is an integer literal (:1131-1139), its
                // first unread; another second argument, or none, fails the match, no type (lite's grammar reads every
                // integer constant as a Long: one past an int is past the engine's parser, which reads it with
                // Integer.parseInt, and past a VARCHAR's size, no type)
                "lpad" => match f.args.get(1) {
                    Some(Op::RelationalOperationLiteral(l)) => match &l.value {
                        JValue::Integer(i) => Some(varchar(*i)),
                        JValue::Long(g) => i32::try_from(*g).ok().map(varchar),
                        _ => None,
                    },
                    _ => None,
                },
                // replace is a VARCHAR of twice its first argument's CHAR or VARCHAR size (:1489-1497), Java's int
                // doubling; another first argument, or none, no type
                "replace" => match first(expanding)? {
                    Some(at) if is_char_or_varchar(&at) => Some(varchar(size(&at).wrapping_mul(2))),
                    _ => None,
                },
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
            // a float literal is FLOAT (relationalExtension.pure:148); until 2026-11-13 DOUBLE (S378, ruling 34)
            JValue::Double(_) => Some(float()),
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

/// `firstWhenSafe`: plus's and minus's rule over several arguments (relationalExtension.pure:1408-1436, :1209-1218): the
/// FIRST argument's type when there is more than one argument and a safe type over all of them is possible (an argument
/// with no type drops out, as `safe_type_of` drops it), else no rule matches (`None`).
fn first_when_safe<'a>(
    args: &[Op],
    db: Option<&'a DatabaseDefinition>,
    ctx: Option<&'a dyn StoreLookups>,
    expanding: &mut Expanding,
) -> Result<Option<T>, Thrown> {
    if args.len() <= 1 {
        return Ok(None);
    }
    let mut types: Vec<Option<T>> = Vec::with_capacity(args.len());
    for arg in args.iter() {
        types.push(infer_in(arg, db, ctx, expanding)?);
    }
    let first = types.first().cloned().flatten();
    Ok(if safe_type_of(types).is_some() { first } else { None })
}

/// `capped`: plus's and sum's DECIMAL and NUMERIC rules (relationalExtension.pure:1408-1436, :1717-1741): the kind kept,
/// its precision `min([31, precision])`, its scale as it is. Asked of a DECIMAL or NUMERIC alone, as Java's refuses
/// another.
fn capped(t: &T) -> T {
    match t {
        T::RelationalDataTypeDecimal(d) => decimal(d.precision.min(31), d.scale),
        T::RelationalDataTypeNumeric(n) => T::RelationalDataTypeNumeric(Arc::new(RelationalDataTypeNumeric {
            precision: n.precision.min(31),
            scale: n.scale,
        })),
        _ => unreachable!("a kind other than DECIMAL or NUMERIC asked for a capped precision"),
    }
}

// ---- the engine's safe type (S378, 2026-11-13, ruling 32): relationalExtension.pure:1926-2033, exactly

/// A kind's place in the engine's `safeTypeMap` (relationalExtension.pure:2014-2033); `None` for a kind it does not list
/// (REAL, BINARY, VARBINARY, TIME, TIMESTAMP_NS, DISTINCT, SEMISTRUCTURED, ARRAY, OBJECT), which has no partner, itself
/// included.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    TinyInt,
    SmallInt,
    Integer,
    BigInt,
    Float,
    Double,
    Numeric,
    Decimal,
    Char,
    Varchar,
    Date,
    Timestamp,
    Bit,
    Other,
}

fn kind(t: &T) -> Option<Kind> {
    Some(match t {
        T::RelationalDataTypeTinyInt(_) => Kind::TinyInt,
        T::RelationalDataTypeSmallInt(_) => Kind::SmallInt,
        T::RelationalDataTypeInteger_(_) => Kind::Integer,
        T::RelationalDataTypeBigInt(_) => Kind::BigInt,
        T::RelationalDataTypeFloat_(_) => Kind::Float,
        T::RelationalDataTypeDouble_(_) => Kind::Double,
        T::RelationalDataTypeNumeric(_) => Kind::Numeric,
        T::RelationalDataTypeDecimal(_) => Kind::Decimal,
        T::RelationalDataTypeChar_(_) => Kind::Char,
        T::RelationalDataTypeVarchar(_) => Kind::Varchar,
        T::RelationalDataTypeDate_(_) => Kind::Date,
        T::RelationalDataTypeTimestamp(_) => Kind::Timestamp,
        T::RelationalDataTypeBit(_) => Kind::Bit,
        T::RelationalDataTypeOther(_) => Kind::Other,
        _ => return None,
    })
}

/// `safeTypeMap->get(k)`: its safe partners, in the map's order.
fn partners(k: Kind) -> &'static [Kind] {
    use Kind::*;
    match k {
        TinyInt => &[TinyInt, SmallInt, Integer, Float, BigInt, Double, Numeric, Decimal],
        SmallInt => &[SmallInt, Integer, Float, BigInt, Double, Numeric, Decimal],
        Integer => &[Integer, Float, BigInt, Double, Numeric, Decimal],
        BigInt => &[BigInt, Double, Numeric, Decimal],
        Float => &[Float, Double, Numeric, Decimal],
        Double => &[Double, Numeric, Decimal],
        Numeric => &[Numeric, Decimal],
        Decimal => &[Decimal, Numeric],
        Char | Varchar => &[Char, Varchar],
        Date => &[Date],
        Timestamp => &[Timestamp],
        Bit => &[Bit],
        Other => &[
            TinyInt, SmallInt, Integer, Float, BigInt, Double, Numeric, Decimal, Char, Varchar, Date, Timestamp, Bit,
            Other,
        ],
    }
}

/// `safeTypeMap->get(t1).values->contains(t2)`, by raw kind.
fn lists(t1: &T, t2: &T) -> bool {
    match (kind(t1), kind(t2)) {
        (Some(a), Some(b)) => partners(a).contains(&b),
        _ => false,
    }
}

/// The engine's `isSafeTypePossible(type1, type2)` (:1954-1965).
fn is_safe_type_possible(t1: &T, t2: &T) -> bool {
    lists(t1, t2) || lists(t2, t1)
}

/// The engine's `getSafeType(type1, type2)` (:1967-1985), asked only where a safe type is possible.
fn get_safe_type(t1: &T, t2: &T) -> T {
    let safe = if lists(t1, t2) { t2 } else { t1 };
    match safe {
        T::RelationalDataTypeChar_(_) | T::RelationalDataTypeVarchar(_) => varchar(size(t1).max(size(t2))),
        T::RelationalDataTypeNumeric(_) | T::RelationalDataTypeDecimal(_) => {
            let scale = scale(t1).max(scale(t2));
            decimal(integer_digits(t1).max(integer_digits(t2)).wrapping_add(scale), scale)
        }
        other => other.clone(),
    }
}

/// The engine's list fold (:1926-1951): from the first type, each next one against the accumulated safe type
/// (`getSafeType(next, acc)`); `None` when a step has none. A `None` type drops out first, as the engine's `map` drops
/// an empty answer; no types, `None`.
fn safe_type_of(types: impl IntoIterator<Item = Option<T>>) -> Option<T> {
    let mut acc: Option<T> = None;
    for t in types.into_iter().flatten() {
        acc = Some(match acc {
            None => t,
            Some(a) if is_safe_type_possible(&t, &a) => get_safe_type(&t, &a),
            Some(_) => return None,
        });
    }
    acc
}

/// A CHAR's or VARCHAR's size, an OTHER's 0 (getSafeType's own rule); no other kind meets a CHAR or VARCHAR safe type
/// (only CHAR, VARCHAR and OTHER list them), as Java's `size` refuses.
fn size(t: &T) -> i32 {
    match t {
        T::RelationalDataTypeChar_(c) => c.size,
        T::RelationalDataTypeVarchar(v) => v.size,
        T::RelationalDataTypeOther(_) => 0,
        _ => unreachable!("a kind other than CHAR, VARCHAR or OTHER met a CHAR or VARCHAR safe type"),
    }
}

fn scale(t: &T) -> i32 {
    match t {
        T::RelationalDataTypeNumeric(n) => n.scale,
        T::RelationalDataTypeDecimal(d) => d.scale,
        _ => 0,
    }
}

fn integer_digits(t: &T) -> i32 {
    match t {
        T::RelationalDataTypeNumeric(n) => n.precision.wrapping_sub(n.scale),
        T::RelationalDataTypeDecimal(d) => d.precision.wrapping_sub(d.scale),
        _ => 0,
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
