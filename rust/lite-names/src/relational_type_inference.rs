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

fn varchar(size: i64) -> T {
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
                    } else if types.iter().any(Option::is_none) {
                        // an argument with no type leaves no type (the engine's failOnMatchFailure=true map fails on it,
                        // relationalExtension.pure:120-123; until 2026-11-17 it dropped out: PR #102's e2112a291)
                        None
                    } else {
                        // over numerics alone the database's promotion (2026-11-17, PR #102's 130f44eb9, its row S500);
                        // else the safe type, the engine's fold (S378, 2026-11-13, ruling 32), else the first's
                        let typed: Vec<T> = types.iter().flatten().cloned().collect();
                        promotion(&typed).or_else(|| safe_type_of(types)).or(first)
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
                    // a REAL too, as lite typed it until 2026-11-13 (2026-11-17, PR #102's 36cd4cd3c, its row S500)
                    Some(T::RelationalDataTypeFloat_(_) | T::RelationalDataTypeDouble_(_) | T::RelationalDataTypeReal(_)) => {
                        Some(double())
                    }
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
                    Some(T::RelationalDataTypeChar_(c)) => Some(varchar(i64::from(c.size))),
                    _ => None,
                },
                // toUpper and toLower keep their first argument's type (:1793-1801, :1843-1851); upper and lower have no
                // rule in the engine's map, so no type (until 2026-11-13 they kept their first argument's type: S378,
                // ruling 34)
                "toUpper" | "toLower" => first(expanding)?,
                // length and indexOf are INTEGER whatever their arguments (relationalExtension.pure:1061-1069,
                // :961-969); position, charindex and locate have no rule in the engine's map, so no type; until 2026-11-14
                // INTEGER, rules only lite had (S378, the names batch, item 2, ruling 34)
                // position INTEGER, as the database returns it (2026-11-17, PR #102's 4993f536a, its row S500)
                "length" | "indexOf" | "position" => Some(integer()),
                // sub is its first argument's type when its first two have one raw kind, else no rule matches
                // (relationalExtension.pure:1637-1645): no type, never a default. Until 2026-11-13 the safe type over
                // all its arguments (S378, ruling 32; its INTEGER default went with the names fixes, item 10)
                // sub over numerics the database's promotion, over others the engine's rule as their safe type
                // (`operator`; 2026-11-17, PR #102's rules, its row S500)
                "sub" => operator(Arith::Sub, &f.args, db, ctx, expanding)?,
                // joinStrings is a VARCHAR of the larger of 4000 and its first argument's CHAR or VARCHAR size; another
                // first argument, or none, no type (relationalExtension.pure:1031-1039: the map's rule types a parsed
                // joinStrings, a DynaFunction; Java's comment says why not the JoinStrings operation's arm, :173). Until
                // 2026-11-14 VARCHAR(4000) whatever its arguments (S378, the names batch, item 4, ruling 34)
                "joinStrings" => match first(expanding)? {
                    Some(at) if is_char_or_varchar(&at) => Some(varchar(size(&at).max(4000))),
                    _ => None,
                },
                // string concatenation SUMS the operand sizes (engine getSize over concat); an operand typed other than
                // CHAR or VARCHAR leaves it no type -- the engine's one rule asks every typed operand to be one
                // (relationalExtension.pure:463-471); an operand with no type drops out. Until 2026-11-13 such an
                // operand added 0 (the names fixes, item 10, ruling 28)
                // group_concat has no rule in the engine's map (no type: until 2026-11-13 it was typed as a concat,
                // S378, ruling 32)
                // since 2026-11-17 (PR #102's e2112a291 and 130f44eb9, its row S500): an operand with no type leaves no
                // type; a non-string operand is sized by its kind's widest text (`text_width`), one of no fixed width
                // leaves no type; an OTHER adds 0. Java's comment says more
                "concat" => match every_type(&f.args, db, ctx, expanding)? {
                    None => None,
                    Some(types) => {
                        let mut size: i64 = 0;
                        let mut fixed = true;
                        for t in types.iter() {
                            let w = if is_char_or_varchar(t) || matches!(t, T::RelationalDataTypeOther(_)) {
                                size_of(t)
                            } else {
                                i64::from(text_width(t))
                            };
                            if w < 0 {
                                fixed = false;
                                break;
                            }
                            size = size.wrapping_add(w);
                        }
                        if fixed { Some(varchar(size)) } else { None }
                    }
                },
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
                    // a branch with no type leaves no type; numeric branches alone the database's promotion
                    // (2026-11-17, PR #102's rules, its rows S500, S501)
                    if types.iter().any(Option::is_none) {
                        None
                    } else {
                        let typed: Vec<T> = types.iter().flatten().cloned().collect();
                        promotion(&typed).or_else(|| safe_type_of(types))
                    }
                }
                // the numeric operators, each by the engine's own rules (S378, 2026-11-13, ruling 34; until then the
                // safe type over every argument, a rule only lite had). plus over ONE argument widens it
                // (relationalExtension.pure:1408-1436: TINYINT and SMALLINT an INTEGER, INTEGER and BIGINT a BIGINT, a
                // DECIMAL or NUMERIC its own kind capped at precision 31, FLOAT and DOUBLE a DOUBLE, another kind no
                // rule); plus and minus over several are their FIRST argument's type when a safe type over all of them
                // is possible, else no rule (:1408-1436, :1209-1218); times is the safe type of its first two
                // arguments, the rest unread (:1773-1782). divide is DOUBLE, above.
                // since 2026-11-17 (the owner's ruling on register row S1006: PR #102's rules, 4993f536a, its rows S400
                // then S500): over numeric operands the database's promotion over every argument, view columns and
                // the metamodel's seeded rows alike (`operator`); plus over one argument keeps the engine's widening, a
                // REAL a DOUBLE as sum's
                "plus" if f.args.len() == 1 => match infer_in(&f.args[0], db, ctx, expanding)? {
                    Some(T::RelationalDataTypeTinyInt(_) | T::RelationalDataTypeSmallInt(_)) => Some(integer()),
                    Some(T::RelationalDataTypeInteger_(_) | T::RelationalDataTypeBigInt(_)) => Some(big_int()),
                    Some(at @ (T::RelationalDataTypeDecimal(_) | T::RelationalDataTypeNumeric(_))) => Some(capped(&at)),
                    Some(T::RelationalDataTypeFloat_(_) | T::RelationalDataTypeDouble_(_) | T::RelationalDataTypeReal(_)) => {
                        Some(double())
                    }
                    _ => None,
                },
                "plus" => operator(Arith::Plus, &f.args, db, ctx, expanding)?,
                "minus" => operator(Arith::Minus, &f.args, db, ctx, expanding)?,
                "times" => operator(Arith::Times, &f.args, db, ctx, expanding)?,
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
                        JValue::Integer(i) => Some(varchar(i64::from(*i))),
                        JValue::Long(g) => i32::try_from(*g).ok().map(|i| varchar(i64::from(i))),
                        _ => None,
                    },
                    _ => None,
                },
                // replace is a VARCHAR of twice its first argument's CHAR or VARCHAR size (:1489-1497); another first
                // argument, or none, no type. A size is an i64, as the engine's Integer, which does not wrap (2026-11-14,
                // the normalizer batch, item 1, ruling 34: until then Java's int doubling, a size past 2^30 wrapped, as
                // concat's and add's sums did; S378)
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
            JValue::String(s) => Some(varchar(s.0.len() as i64)),
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

/// A numeric operator's member (Java's `DynaFn` PLUS, MINUS, TIMES, SUB in `operator`).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Arith {
    Plus,
    Minus,
    Times,
    Sub,
}

/// `everyType(args)`: every argument's type, each typed; `None` when any has none (the engine's failOnMatchFailure=true
/// map fails there).
fn every_type<'a>(
    args: &[Op],
    db: Option<&'a DatabaseDefinition>,
    ctx: Option<&'a dyn StoreLookups>,
    expanding: &mut Expanding,
) -> Result<Option<Vec<T>>, Thrown> {
    let mut types: Vec<T> = Vec::with_capacity(args.len());
    let mut untyped = false;
    for arg in args.iter() {
        match infer_in(arg, db, ctx, expanding)? {
            Some(t) => types.push(t),
            None => untyped = true,
        }
    }
    Ok(if untyped { None } else { Some(types) })
}

/// `operator(op, args)`: plus, minus and times over several arguments (minus and times over one), sub over two (the
/// owner's ruling of 2026-11-17 on register row S1006: PR #102's rules, its rows S400 then S500; until then plus and
/// minus over several were their first argument's type where a safe type was possible, the engine's rule). An argument
/// with no type leaves no type; numeric operands alone are the database's `promotion`; any other is the engine's rule
/// as its safe type: plus and minus over several their safe type, times its first two's, sub over two of one raw kind
/// their safe type or the first's (sub takes two arguments, else no type). Java's comment says more.
fn operator<'a>(
    op: Arith,
    args: &[Op],
    db: Option<&'a DatabaseDefinition>,
    ctx: Option<&'a dyn StoreLookups>,
    expanding: &mut Expanding,
) -> Result<Option<T>, Thrown> {
    let Some(types) = every_type(args, db, ctx, expanding)? else {
        return Ok(None);
    };
    if op == Arith::Sub && types.len() != 2 {
        return Ok(None);
    }
    if let Some(numeric) = promotion(&types) {
        return Ok(Some(numeric));
    }
    let several = types.len() > 1;
    Ok(match op {
        Arith::Plus | Arith::Minus => {
            if several {
                safe_type_of(types.into_iter().map(Some))
            } else {
                None
            }
        }
        Arith::Times => {
            if several {
                safe_type_of(types.into_iter().take(2).map(Some))
            } else {
                None
            }
        }
        Arith::Sub => {
            if std::mem::discriminant(&types[0]) != std::mem::discriminant(&types[1]) {
                None
            } else {
                let first = types[0].clone();
                safe_type_of(types.into_iter().take(2).map(Some)).or(Some(first))
            }
        }
    })
}

/// `promotion(types)`: the database's numeric promotion over every type (PR #102's row S500), when every type is numeric
/// or an OTHER and one is numeric; `None` otherwise. An approximate operand makes a DOUBLE, but operands of one
/// approximate kind keep it; else a DECIMAL or NUMERIC operand as declared, two of them a DECIMAL of the larger integer
/// digits plus the larger scale; else the widest integer kind.
fn promotion(types: &[T]) -> Option<T> {
    let mut acc: Option<T> = None;
    for t in types.iter() {
        if matches!(t, T::RelationalDataTypeOther(_)) {
            continue;
        }
        if !is_numeric(t) {
            return None;
        }
        acc = Some(match acc {
            None => t.clone(),
            Some(a) => promote(&a, t),
        });
    }
    acc
}

fn is_numeric(t: &T) -> bool {
    matches!(
        t,
        T::RelationalDataTypeTinyInt(_)
            | T::RelationalDataTypeSmallInt(_)
            | T::RelationalDataTypeInteger_(_)
            | T::RelationalDataTypeBigInt(_)
            | T::RelationalDataTypeFloat_(_)
            | T::RelationalDataTypeDouble_(_)
            | T::RelationalDataTypeReal(_)
            | T::RelationalDataTypeDecimal(_)
            | T::RelationalDataTypeNumeric(_)
    )
}

fn approximate(t: &T) -> bool {
    matches!(t, T::RelationalDataTypeFloat_(_) | T::RelationalDataTypeDouble_(_) | T::RelationalDataTypeReal(_))
}

fn is_decimal_kind(t: &T) -> bool {
    matches!(t, T::RelationalDataTypeDecimal(_) | T::RelationalDataTypeNumeric(_))
}

/// Java's `INTEGER_WIDTH.indexOf(kind)`: TINYINT, SMALLINT, INTEGER, BIGINT at 0-3, any other kind -1.
fn integer_width(t: &T) -> i32 {
    match t {
        T::RelationalDataTypeTinyInt(_) => 0,
        T::RelationalDataTypeSmallInt(_) => 1,
        T::RelationalDataTypeInteger_(_) => 2,
        T::RelationalDataTypeBigInt(_) => 3,
        _ => -1,
    }
}

fn promote(a: &T, b: &T) -> T {
    if approximate(a) || approximate(b) {
        return if std::mem::discriminant(a) == std::mem::discriminant(b) { a.clone() } else { double() };
    }
    let da = is_decimal_kind(a);
    let dbx = is_decimal_kind(b);
    if da != dbx {
        return if da { a.clone() } else { b.clone() };
    }
    if da {
        let scale = scale(a).max(scale(b));
        return decimal(integer_digits(a).max(integer_digits(b)).wrapping_add(scale), scale);
    }
    if integer_width(a) >= integer_width(b) { a.clone() } else { b.clone() }
}

/// `textWidth(t)`: the widest text a database writes for a value of a kind of fixed width, or -1 (concat's non-string
/// operands; PR #102's 130f44eb9): TINYINT 4, SMALLINT 6, INTEGER 11, BIGINT 20, a DECIMAL or NUMERIC its precision plus
/// a sign and a point, BIT 5, DATE 10, TIMESTAMP 29.
fn text_width(t: &T) -> i32 {
    match t {
        T::RelationalDataTypeTinyInt(_) => 4,
        T::RelationalDataTypeSmallInt(_) => 6,
        T::RelationalDataTypeInteger_(_) => 11,
        T::RelationalDataTypeBigInt(_) => 20,
        T::RelationalDataTypeDecimal(_) | T::RelationalDataTypeNumeric(_) => {
            integer_digits(t).wrapping_add(scale(t)).wrapping_add(2)
        }
        T::RelationalDataTypeBit(_) => 5,
        T::RelationalDataTypeDate_(_) => 10,
        T::RelationalDataTypeTimestamp(_) => 29,
        _ => -1,
    }
}

/// A CHAR's, VARCHAR's or OTHER's size (concat's string operands), as `size` reads it.
fn size_of(t: &T) -> i64 {
    size(t)
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
/// (`getSafeType(next, acc)`); `None` when a step has none, or when a type is `None`; no types, `None`.
fn safe_type_of(types: impl IntoIterator<Item = Option<T>>) -> Option<T> {
    let mut acc: Option<T> = None;
    for t in types.into_iter() {
        // a `None` type leaves none: the engine's map over the failOnMatchFailure=true inferRelationalType fails on it
        // (relationalExtension.pure:120-123; until 2026-11-17 it dropped out, PR #102's e2112a291)
        let Some(t) = t else {
            return None;
        };
        acc = Some(match acc {
            None => t,
            Some(a) if is_safe_type_possible(&t, &a) => get_safe_type(&t, &a),
            Some(_) => return None,
        });
    }
    acc
}

/// A CHAR's or VARCHAR's size, an OTHER's 0 (getSafeType's own rule); no other kind meets a CHAR or VARCHAR safe type
/// (only CHAR, VARCHAR and OTHER list them), as Java's `size` refuses. An i64, the engine's Integer (getSize,
/// :2005-2011): every sum, product and maximum of sizes is taken in it (2026-11-14, the normalizer batch, item 1, ruling
/// 34; until then an i32, Java's int).
fn size(t: &T) -> i64 {
    match t {
        T::RelationalDataTypeChar_(c) => i64::from(c.size),
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
