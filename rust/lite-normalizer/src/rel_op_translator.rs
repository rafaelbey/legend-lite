// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! `RelOpTranslator` (RelOpTranslator.java at ad86fa895, PORT LINE FOR LINE): every legacy relational expression
//! (property expressions, filters, groupBy keys, view columns, join conditions, association predicates) as Pure. The
//! arm order, the guards' fall-through to the generic arm and the left-to-right evaluation order decide which error
//! surfaces. EF_CENSUS §1.5's families: ROT-1 dispatch (`translate` with `translateTail`, `translateArgs`, `toOne*`,
//! :127-627), ROT-2 `PipelineView`, `joinNavigation`, `rebaseToTable` (:57-91, :632-691), ROT-3 helpers
//! (`collectTablesIn`, `columnRead`, `literalToValueSpec`, `comparisonFn`, :93-115, :693-717), ROT-4 residual-name
//! resolution (:726-773).
//!
//! RE-EXPRESSED (census §4, ruling 31):
//! - `translate` and `translateTail` are ONE match in the same arm order (the split existed for a method-length guard;
//!   `translateTail` is reached only from `translate`'s default arm) -- [`translate`] alone, no `translate_tail`.
//!   Java's every `FunctionCall` arm is contiguous (translate's eleven, then translateTail's twelve and the generic
//!   one), so they are one nested match in that order; `translateTail`'s `default` (a column, `{target}` column or
//!   literal reaching it, :623) has no Rust arm: the one match takes those kinds first.
//! - The dispatch's dynafunction arms are `crate::dyna_fn::arm`'s [`Arm`]: `translate` matches `arm(d)` EXHAUSTIVELY
//!   over `Arm`, every variant named once and no `_` (no `DynaFn::X` guard of its own: this file names no `DynaFn`
//!   member), which is `DynaFnRegistryTest`'s source-text invariant made structural. Each armed variant answers its
//!   rewrite, or none when its shape guard fails, and none falls to the generic arm (Java's guard fall-through).
//!   `DYNA_HASH_TYPES` (:47) is [`Arm::hash_type`](crate::dyna_fn::Arm::hash_type).
//! - `PipelineView` is `Option<&Pipeline>` (`crate::pipeline::PipelineView`); `NONE` is `None`.
//! - `operatorCall`'s `shaped == args` (:771) is the boolean it tests: `args.len() >= 2 && isVariadicRun(fqns)`.
//! - The raw internal errors a user's text could reach (ASK 11) are refusals in Java since ad86fa895 (ruling 29;
//!   :720's TRANSLATED arm a `NotImplementedException` now); `literalToValueSpec`'s text names the literal's Java class
//!   (`JValue::java_class`).
//!
//! Kept as Java has them (a raw JDK throw a user's text reaches): the hash arm has no arity guard (`md5()` reads
//! `args.get(0)` of the empty immutable list, :253) and `convertDate` takes zero arguments (`size() <= 2`, :324, then
//! `as.get(0)`, :329): both are `java.lang.ArrayIndexOutOfBoundsException: Index 0 out of bounds for length 0` (JDK 25's
//! `ImmutableCollections.ListN.get`, probed).

use std::sync::Arc;

use lite_builtin::dyna_fn::DynaFn;
use lite_builtin::dyna_fn_decisions::Resolution;
use lite_builtin::pure::lite;
use lite_error::Phase;
use lite_front::records as p;
use lite_model::linked::LinkedMap;
use lite_model::text::ascii_upper;
use lite_model::{
    ComparisonOp, JStr, JValue, LogicalOp, RelationalOperation, RelationalOperationColumnRef,
    RelationalOperationFunctionCall, RelationalOperationJoinNavigation, cat,
};
use lite_names::calls;
use lite_names::model_index::java_list;

use crate::dyna_fn::{Arm, arm};
use crate::error::{Thrown, require};
use crate::jdk::{LinkedSet, equals_ignore_case};
use crate::mapping_normalizer;
use crate::pipeline::{Pipeline, PipelineView};

/// The table scope a translation reads: table name -> the row expression its columns read off (Java's
/// `Map<String, ValueSpecification> tableScope`, a `LinkedHashMap` at every site; its `keySet()` is printed in a
/// wall text).
pub type TableScope = LinkedMap<JStr, p::ValueSpecification>;

/// The JDK class of an index past an immutable list's end (`ImmutableCollections.ListN.get`'s array read).
const ARRAY_INDEX_OUT_OF_BOUNDS: &str = "java.lang.ArrayIndexOutOfBoundsException";

/// `RelOpTranslator.dyna` (:52): the registry's entry for the call's name.
pub fn dyna(call: &RelationalOperationFunctionCall) -> Option<DynaFn> {
    // `DynaFn.of` by the exact spelling; a name that is not UTF-16-valid is no registry name (they are all ASCII)
    String::from_utf16(&call.name.0).ok().and_then(|name| DynaFn::of(&name))
}

/// `RelOpTranslator.collectTablesIn` (:93): the tables an expression reads, into `sink`; loud for a join navigation
/// inside it.
pub fn collect_tables_in(op: &RelationalOperation, sink: &mut LinkedSet<JStr>) -> Result<(), Thrown> {
    if op.navigates_join() {
        return Err(Thrown::model(
            Phase::Normalize,
            JStr::from("JoinNavigation inside expression"),
            None,
        ));
    }
    for t in op.tables() {
        sink.add(t);
    }
    Ok(())
}

/// `RelOpTranslator.columnRead` (:101): `<row>.column` for the table's row in scope (the default table's when the
/// table has none); loud for an ambiguous table or no row.
pub fn column_read(
    table: &JStr,
    column: &JStr,
    table_scope: &TableScope,
    default_table: &JStr,
    pipeline: Option<&Pipeline<'_>>,
) -> Result<p::ValueSpecification, Thrown> {
    let mut base = table_scope.get(table);
    if base.is_none() && pipeline.is_ambiguous(table) {
        return Err(ambiguous_table_ref(table, column));
    }
    if base.is_none() {
        base = table_scope.get(default_table);
    }
    let Some(base) = base else {
        return Err(Thrown::model(
            Phase::Normalize,
            cat!(
                "No row variable in scope for table '",
                table,
                "'; available=",
                key_set(table_scope)
            ),
            None,
        ));
    };
    Ok(property(base.clone(), column))
}

/// `RelOpTranslator.translateArgs` (:127): a call's arguments translated, left to right; outside a join condition (no
/// target var) a bare column operand is wrapped in `trustOne` (the dyna-wide multiplicity convention).
pub fn translate_args(
    call: &RelationalOperationFunctionCall,
    table_scope: &TableScope,
    target_var_or_null: Option<&p::ValueSpecification>,
    row_bind_or_null: Option<&Arc<p::Variable>>,
    pipeline: Option<&Pipeline<'_>>,
) -> Result<Vec<p::ValueSpecification>, Thrown> {
    let mut out = Vec::with_capacity(call.args.len());
    for a in call.args.iter() {
        let t = translate(a, table_scope, target_var_or_null, row_bind_or_null, pipeline)?;
        // JOIN CONDITIONS (a target var is bound) stay VERBATIM; only PROPERTY expressions get the scalar conform
        out.push(
            if target_var_or_null.is_none() && matches!(a, RelationalOperation::RelationalOperationColumnRef(_)) {
                exact(lite::TRUST_ONE, vec![t])
            } else {
                t
            },
        );
    }
    Ok(out)
}

/// `RelOpTranslator.toOne` (:150): `trustOne(v)`.
pub fn to_one(v: &p::ValueSpecification) -> p::ValueSpecification {
    exact(lite::TRUST_ONE, vec![v.clone()])
}

/// `RelOpTranslator.toOneAll` (:154): every operand `trustOne`d but a collection (an in-list is genuinely many).
pub fn to_one_all(vs: &[p::ValueSpecification]) -> Vec<p::ValueSpecification> {
    vs.iter()
        .map(|v| {
            if matches!(v, p::ValueSpecification::PureCollection(_)) {
                v.clone()
            } else {
                to_one(v)
            }
        })
        .collect()
}

/// `RelOpTranslator.strCast` (:164): `cast(v, @String)`, the SQL VARCHAR coercion.
pub fn str_cast(v: &p::ValueSpecification) -> p::ValueSpecification {
    platform("cast", vec![v.clone(), type_named("String")])
}

/// `RelOpTranslator.pureTypeFor` (:173): the Pure primitive an `extractFromSemiStructured` SQL type extracts to; loud
/// for another (SEMISTRUCTURED, an array suffix).
pub fn pure_type_for(sql_type: &JStr) -> Result<JStr, Thrown> {
    // `sqlType.toUpperCase(Locale.ROOT)` compared with ASCII words: an upper case that is not ASCII matches none
    let kind = match ascii_upper(&sql_type.0).as_deref() {
        Some("VARCHAR" | "CHAR" | "STRING") => "String",
        Some("INTEGER" | "INT" | "BIGINT" | "SMALLINT" | "TINYINT") => "Integer",
        Some("FLOAT" | "DOUBLE" | "REAL") => "Float",
        Some("DECIMAL" | "NUMERIC") => "Decimal",
        Some("BOOLEAN" | "BIT") => "Boolean",
        Some("DATE") => "StrictDate",
        Some("TIMESTAMP" | "DATETIME") => "DateTime",
        _ => {
            return Err(Thrown::model(
                Phase::Normalize,
                cat!(
                    "Unsupported SQL type '",
                    sql_type,
                    "' in extractFromSemiStructured (the engine's scalar list is",
                    " BOOLEAN, CHAR, VARCHAR, STRING, INTEGER, DECIMAL, FLOAT,",
                    " DATE, DATETIME, TIMESTAMP; SEMISTRUCTURED and []-suffixed",
                    " array element types are not realized here yet)"
                ),
                None,
            ));
        }
    };
    Ok(JStr::from(kind))
}

/// `RelOpTranslator.ambiguousTableRef` (:208): the refusal of a bare column ref to a table two sub-row slots reach.
pub fn ambiguous_table_ref(table: &JStr, column: &JStr) -> Thrown {
    Thrown::model(
        Phase::Normalize,
        cat!(
            "Ambiguous column reference '",
            table,
            ".",
            column,
            "': the join ",
            "chain reaches table '",
            table,
            "' through more than one path, so a ",
            "bare column reference cannot identify which sub-row is meant. Pin the ",
            "intended sub-row with a join-terminal column (e.g. @SomeJoin | ",
            table,
            ".",
            column,
            ")."
        ),
        None,
    )
}

/// `RelOpTranslator.translate` (:217, with `translateTail` :398 as its tail; module note): one relational expression
/// as Pure.
pub fn translate(
    op: &RelationalOperation,
    table_scope: &TableScope,
    target_var_or_null: Option<&p::ValueSpecification>,
    row_bind_or_null: Option<&Arc<p::Variable>>,
    pipeline: Option<&Pipeline<'_>>,
) -> Result<p::ValueSpecification, Thrown> {
    let tr = |o: &RelationalOperation| translate(o, table_scope, target_var_or_null, row_bind_or_null, pipeline);
    let args_of = |c: &RelationalOperationFunctionCall| {
        translate_args(c, table_scope, target_var_or_null, row_bind_or_null, pipeline)
    };
    Ok(match op {
        RelationalOperation::RelationalOperationColumnRef(r) => {
            // scope registers canonical names (MappingNormalizer owns the default-schema spelling rule)
            let ref_table = mapping_normalizer::canonical_table(&r.table);
            let path = table_scope.get(&ref_table);
            if path.is_none() && pipeline.is_ambiguous(&ref_table) {
                return Err(ambiguous_table_ref(&ref_table, &r.column));
            }
            let Some(path) = path else {
                return Err(Thrown::model(
                    Phase::Normalize,
                    cat!(
                        "ColumnRef references table '",
                        ref_table,
                        "' not in scope; available=",
                        key_set(table_scope)
                    ),
                    None,
                ));
            };
            property(path.clone(), &r.column)
        }
        RelationalOperation::RelationalOperationTargetColumnRef(t) => {
            let Some(target) = target_var_or_null else {
                return Err(Thrown::model(
                    Phase::Normalize,
                    cat!(
                        "TargetColumnRef {target}.",
                        t.column,
                        " outside a join condition context"
                    ),
                    None,
                ));
            };
            property(target.clone(), &t.column)
        }
        RelationalOperation::RelationalOperationLiteral(lit) => literal_to_value_spec(&lit.value)?,
        RelationalOperation::RelationalOperationFunctionCall(call) => {
            let args = &call.args[..];
            let n = args.len();
            // the armed rewrites, in Java's arm order (translate's :247-390, then translateTail's :404-544); none is
            // an arm whose guard did not match, or a name with no arm: the generic arm below
            let rewritten: Option<p::ValueSpecification> = match dyna(call).and_then(arm) {
                // DYNA_HASH_TYPES.containsKey (:248); no arity guard (module note). SQL-lane operand: hash(String[1],
                // ...) is strict -- the 'position' toOne idiom
                Some(a @ (Arm::Md5 | Arm::Sha1 | Arm::Sha256)) => {
                    let operand = to_one(&tr(at(args, 0)?)?);
                    let hash_type = require(a.hash_type(), None)?;
                    Some(platform(
                        "hash",
                        vec![
                            operand,
                            enum_value("meta::pure::functions::hash::HashType", JStr::from(hash_type)),
                        ],
                    ))
                }
                // the DYNA returns the day NAME (a string); pure's dayOfWeek returns the enum -- toString is the name
                Some(Arm::DayOfWeek) => {
                    if n == 1 {
                        Some(platform("toString", vec![platform("dayOfWeek", args_of(call)?)]))
                    } else {
                        None
                    }
                }
                // 1-arg = DAY_OF_WEEK (SUNDAY=1); the 2-arg form fixes the week start ('Monday' or 'Sunday', else the
                // engine asserts). The pure native lowers to isodow (Monday=1), so Sunday-based forms conform by
                // emission: mod(isodow, 7) + 1. The week start is checked before the operand is translated.
                Some(Arm::DayOfWeekNumber) => {
                    if n == 1 || n == 2 {
                        let mut week_start = JStr::from("Sunday");
                        if n == 2 {
                            let Some(ws) = string_literal(&args[1])
                                .filter(|ws| ignore_case_is(ws, "Monday") || ignore_case_is(ws, "Sunday"))
                            else {
                                return Err(Thrown::not_implemented(JStr::from(
                                    "dayOfWeekNumber requires 'Sunday' or 'Monday' as the week start (engine assert)",
                                )));
                            };
                            week_start = ws.clone();
                        }
                        // SQL-lane operand (the 'position' toOne idiom): dayOfWeekNumber's real signature is strict
                        // Date[1]
                        let iso = platform("dayOfWeekNumber", vec![to_one(&tr(&args[0])?)]);
                        Some(if ignore_case_is(&week_start, "Monday") {
                            iso
                        } else {
                            infix_run("plus", vec![platform("mod", vec![iso, cinteger(7)]), cinteger(1)])
                        })
                    } else {
                        None
                    }
                }
                // the dyna spells the DurationUnit as a string literal; operands are SQL-lane (the 'position' toOne
                // idiom)
                Some(Arm::Adjust) => {
                    if n == 3
                        && let Some(unit) = string_literal(&args[2])
                    {
                        let a0 = to_one(&tr(&args[0])?);
                        let a1 = to_one(&tr(&args[1])?);
                        Some(platform(
                            "adjust",
                            vec![
                                a0,
                                a1,
                                enum_value("meta::pure::functions::date::DurationUnit", to_upper_root(unit)),
                            ],
                        ))
                    } else {
                        None
                    }
                }
                Some(Arm::ConvertTimeZone) => {
                    if n == 3 {
                        Some(exact(lite::CONVERT_TIME_ZONE_FORMAT, args_of(call)?))
                    } else {
                        None
                    }
                }
                // FORMAT dynafunctions: parseDate/convertDate/convertDateTime/toTimestamp with a format string route
                // to the lite natives; convertDate without a format is the ISO spelling; convertVarchar128 is the
                // VARCHAR coercion.
                Some(Arm::ParseDate) => {
                    if n == 2 {
                        Some(exact(lite::PARSE_DATE_FORMAT, args_of(call)?))
                    } else {
                        None
                    }
                }
                // zero arguments pass `size() <= 2` (module note)
                Some(Arm::ConvertDate) => {
                    if n <= 2 {
                        let translated = args_of(call)?;
                        let operands = if translated.len() == 2 {
                            translated
                        } else {
                            vec![at(&translated, 0)?.clone(), cstring(JStr::from("yyyy-MM-dd"))]
                        };
                        Some(exact(lite::CONVERT_DATE_FORMAT, operands))
                    } else {
                        None
                    }
                }
                Some(Arm::ConvertDateTime | Arm::ToTimestamp) => {
                    if n == 2 {
                        Some(exact(lite::CONVERT_DATE_TIME_FORMAT, args_of(call)?))
                    } else {
                        None
                    }
                }
                Some(Arm::ConvertVarchar128) => {
                    if n == 1 {
                        Some(str_cast(&tr(&args[0])?))
                    } else {
                        None
                    }
                }
                // a string-typed part index conforms by EMISSION (cast(@Integer)); token/part are strict [1] (the
                // 'position' toOne idiom; a0 stays bare: pure's first param is [0..1]); SQL's split_part counts parts
                // from 1, pure's splitPart from 0 -- part - 1 (empty tokens still differ: SQL keeps them, pure drops
                // them)
                Some(Arm::SplitPart) => {
                    if n == 3 {
                        let a0 = tr(&args[0])?;
                        let a1 = tr(&args[1])?;
                        let a2 = tr(&args[2])?;
                        Some(platform(
                            "splitPart",
                            vec![
                                a0,
                                to_one(&a1),
                                infix_run(
                                    "minus",
                                    vec![platform("cast", vec![to_one(&a2), type_named("Integer")]), cinteger(1)],
                                ),
                            ],
                        ))
                    } else {
                        None
                    }
                }
                // case(c1, v1 [, c2, v2 ...], default) -- nested if(cond, {|then}, {|else}); the default translated
                // first, then the pairs from the last
                Some(Arm::Case) => {
                    if n >= 3 && n % 2 == 1 {
                        let mut tail = tr(&args[n - 1])?;
                        for i in (0..=n - 3).rev().step_by(2) {
                            let cond = tr(&args[i])?;
                            let then = tr(&args[i + 1])?;
                            tail = platform("if", vec![cond, thunk(then), thunk(tail)]);
                        }
                        Some(tail)
                    } else {
                        None
                    }
                }
                // translateTail (:404). The variadic 'concat' has no pure counterpart: string plus over each part's
                // cast(@String) (SQL concat COERCES its arguments), ONE run -- the parser's n-ary carrier
                Some(Arm::Concat) => {
                    if n >= 2 {
                        let mut parts = Vec::with_capacity(n);
                        for part in args {
                            parts.push(to_one(&str_cast(&tr(part)?)));
                        }
                        Some(if parts.len() == 1 {
                            parts.remove(0)
                        } else {
                            infix_run("plus", parts)
                        })
                    } else {
                        None
                    }
                }
                // extractFromSemiStructured(col, 'path', 'SQLTYPE'): to(get(col, 'path'), @Type); the SQL type is
                // read after both operands
                Some(Arm::ExtractFromSemiStructured) => {
                    if n == 3
                        && let Some(sql_type) = string_literal(&args[2])
                    {
                        let get = platform("get", vec![tr(&args[0])?, tr(&args[1])?]);
                        Some(platform("to", vec![get, type_named_j(pure_type_for(sql_type)?)]))
                    } else {
                        None
                    }
                }
                // the 'indexOf' DYNAFUNCTION is 1-based locate, as the pure-relational indexOf rule: VERBATIM
                Some(Arm::IndexOf) => {
                    if n == 2 {
                        Some(platform("indexOf", vec![tr(&args[0])?, tr(&args[1])?]))
                    } else {
                        None
                    }
                }
                // dyna 'substring' is SQL SUBSTRING, forwarded unchanged; every operand SQL-lane (a column operand is
                // trustOne(trustOne(col)), as Java's: translateArgs wraps it, toOneAll again)
                Some(Arm::Substring) => {
                    if n == 2 || n == 3 {
                        Some(platform("substring", to_one_all(&args_of(call)?)))
                    } else {
                        None
                    }
                }
                // dyna 'add'/'sub' are SQL ARITHMETIC: plus/minus, the operands trustOne'd
                Some(Arm::Add) => {
                    if n == 2 {
                        Some(infix_run("plus", to_one_all(&args_of(call)?)))
                    } else {
                        None
                    }
                }
                Some(Arm::Sub) => {
                    if n == 2 {
                        Some(infix_run("minus", to_one_all(&args_of(call)?)))
                    } else {
                        None
                    }
                }
                // SQL POSITION(needle, haystack) -- 1-based, arguments REVERSED vs pure's indexOf(haystack, needle);
                // the haystack (translated first) trustOne'd
                Some(Arm::Position) => {
                    if n == 2 {
                        let haystack = exact(lite::TRUST_ONE, vec![tr(&args[1])?]);
                        Some(platform("indexOf", vec![haystack, tr(&args[0])?]))
                    } else {
                        None
                    }
                }
                // isNull/isNotNull ARE isEmpty/isNotEmpty on [0..1] values; group(x) is parenthesization; if's
                // branches are THUNKS
                Some(Arm::IsNull) => {
                    if n == 1 {
                        Some(platform("isEmpty", vec![tr(&args[0])?]))
                    } else {
                        None
                    }
                }
                Some(Arm::IsNotNull) => {
                    if n == 1 {
                        Some(platform("isNotEmpty", vec![tr(&args[0])?]))
                    } else {
                        None
                    }
                }
                Some(Arm::Group) => {
                    if n == 1 {
                        Some(tr(&args[0])?)
                    } else {
                        None
                    }
                }
                Some(Arm::If) => {
                    if n == 3 {
                        let cond = tr(&args[0])?;
                        let then = thunk(tr(&args[1])?);
                        let otherwise = thunk(tr(&args[2])?);
                        Some(platform("if", vec![cond, then, otherwise]))
                    } else {
                        None
                    }
                }
                // the or/and DYNAs are VARIADIC; pure's boolean or/and are binary: fold left
                Some(Arm::Or | Arm::And) => {
                    if n > 2 {
                        let mut acc = tr(&args[0])?;
                        for a in &args[1..] {
                            acc = vs(calls::platform(&call.name, vec![acc, tr(a)?]));
                        }
                        Some(acc)
                    } else {
                        None
                    }
                }
                None => None,
            };
            match rewritten {
                Some(v) => v,
                // Wire-vocabulary passthrough (:556): the name resolved first (a refusal beats an operand's error),
                // then the operands, SQL-lane, each trustOne'd
                None => {
                    let name = dyna_fn_name(call)?;
                    let candidates = dyna_fn_candidates(call);
                    let operands = to_one_all(&args_of(call)?);
                    vs(operator_call(&name, &candidates, operands)?)
                }
            }
        }
        // a RELATIONAL comparison over a column has SQL null semantics BY DEFINITION -- conform by EMISSION (toOne);
        // join conditions stay verbatim
        RelationalOperation::RelationalOperationComparison(cmp) => {
            let side = |o: &RelationalOperation| -> Result<p::ValueSpecification, Thrown> {
                let t = tr(o)?;
                Ok(
                    if target_var_or_null.is_none() && matches!(o, RelationalOperation::RelationalOperationColumnRef(_))
                    {
                        exact(lite::TRUST_ONE, vec![t])
                    } else {
                        t
                    },
                )
            };
            let left = side(&cmp.left)?;
            let right = side(&cmp.right)?;
            let c = platform(comparison_fn(cmp.op), vec![left, right]);
            // NEQ emits not(equal(...)) -- real pure has no notEqual
            if cmp.op == ComparisonOp::NEQ {
                platform("not", vec![c])
            } else {
                c
            }
        }
        RelationalOperation::RelationalOperationBooleanOp(bo) => {
            let left = tr(&bo.left)?;
            let right = tr(&bo.right)?;
            platform(if bo.op == LogicalOp::AND { "and" } else { "or" }, vec![left, right])
        }
        // relational 'is (not) null' is REAL pure's isEmpty/isNotEmpty
        RelationalOperation::RelationalOperationIsNull(n) => platform("isEmpty", vec![tr(&n.operand)?]),
        RelationalOperation::RelationalOperationIsNotNull(n) => platform("isNotEmpty", vec![tr(&n.operand)?]),
        RelationalOperation::RelationalOperationGroup(g) => tr(&g.inner)?,
        RelationalOperation::RelationalOperationArrayLiteral(arr) => {
            let mut values = Vec::with_capacity(arr.elements.len());
            for e in arr.elements.iter() {
                values.push(tr(e)?);
            }
            collection(values)
        }
        RelationalOperation::RelationalOperationJoinNavigation(jn) => {
            join_navigation(jn, table_scope, target_var_or_null, row_bind_or_null, pipeline)?
        }
        // a lambda argument and its `$x` are parsed and carried; translating them is a leg of its own, refused by
        // name (ruling 29, the normalizer slice's step 1)
        RelationalOperation::RelationalOperationLambda(lam) => {
            let mut params: Vec<u16> = Vec::new();
            for (i, name) in lam.parameters.iter().enumerate() {
                if i > 0 {
                    params.extend(", ".encode_utf16());
                }
                params.extend_from_slice(&name.0);
            }
            return Err(Thrown::not_implemented(cat!(
                "a lambda argument (",
                params,
                " | ...) in a relational expression is not supported by this platform yet"
            )));
        }
        RelationalOperation::RelationalOperationLambdaParam(lp) => {
            return Err(Thrown::not_implemented(cat!(
                "a lambda parameter ($",
                lp.name,
                ") in a relational expression is not supported by this platform yet"
            )));
        }
    })
}

/// `RelOpTranslator.joinNavigation` (:632): `$row.<slot>.<col>` for a hoisted join navigation (an inner scope over
/// the slot's target table otherwise).
pub fn join_navigation(
    jn: &RelationalOperationJoinNavigation,
    table_scope: &TableScope,
    target_var_or_null: Option<&p::ValueSpecification>,
    row_bind_or_null: Option<&Arc<p::Variable>>,
    pipeline: Option<&Pipeline<'_>>,
) -> Result<p::ValueSpecification, Thrown> {
    let (Some(row_bind), true) = (row_bind_or_null, pipeline.has_slots()) else {
        return Err(Thrown::model(
            Phase::Normalize,
            cat!(
                "Nested JoinNavigation in scope without pipeline; ",
                "JoinNav inside association predicates or join ",
                "conditions is not supported."
            ),
            None,
        ));
    };
    // Java's `requireNonNull(pipeline.slotFor(..), "no pipeline slot for join chain")`: `slotFor` is loud itself and
    // never answers null (JoinChainEmission:689)
    let alias = pipeline.slot_for(&jn.chain)?;
    let sub_row = property(p::ValueSpecification::Variable(row_bind.clone()), &alias);
    let Some(jn_terminal) = &jn.terminal else {
        return Ok(sub_row);
    };
    let terminal_table: Option<JStr> = pipeline.target_table(Some(&alias)).cloned();
    let mut inner_scope = table_scope.clone();
    if let Some(tt) = &terminal_table {
        inner_scope.put(tt.clone(), sub_row);
    }
    // ENGINE (pureToSQLQuery.pure resolveJoinElement): the terminal's columns are re-resolved in the JOINED cursor --
    // a column the chain end declares moves to it; one it does not stays where it is spelled
    let terminal = match &terminal_table {
        None => jn_terminal.clone(),
        Some(tt) => rebase_to_table(jn_terminal, tt, &|c: &RelationalOperationColumnRef| {
            c.table != *tt && pipeline.target_has_column(Some(&alias), &c.column)
        }),
    };
    translate(&terminal, &inner_scope, target_var_or_null, row_bind_or_null, pipeline)
}

/// `RelOpTranslator.rebaseToTable` (:680): the column refs `moves` accepts re-pointed at `table`; nested join
/// navigations keep their own chains.
pub fn rebase_to_table(
    t: &RelationalOperation,
    table: &JStr,
    moves: &dyn Fn(&RelationalOperationColumnRef) -> bool,
) -> RelationalOperation {
    rebased(t, table, moves).unwrap_or_else(|| t.clone())
}

/// `rebaseToTable`'s answer as `mapChildren` reads it: none for "this very operation".
fn rebased(
    t: &RelationalOperation,
    table: &JStr,
    moves: &dyn Fn(&RelationalOperationColumnRef) -> bool,
) -> Option<RelationalOperation> {
    match t {
        RelationalOperation::RelationalOperationColumnRef(cr) => moves(cr).then(|| {
            RelationalOperation::RelationalOperationColumnRef(Arc::new(RelationalOperationColumnRef {
                database_name: cr.database_name.clone(),
                table: table.clone(),
                column: cr.column.clone(),
            }))
        }),
        RelationalOperation::RelationalOperationJoinNavigation(_) => None,
        _ => t.map_children(|x| rebased(x, table, moves)),
    }
}

/// `RelOpTranslator.literalToValueSpec` (:693): a literal as a Pure constant; loud for another kind ("Unsupported
/// literal type: <Java class>").
pub fn literal_to_value_spec(value: &JValue) -> Result<p::ValueSpecification, Thrown> {
    Ok(match value {
        JValue::String(s) => cstring(s.clone()),
        JValue::Long(l) => cinteger(*l),
        // `new CInteger((long) i)`: the Integer widened to a Long
        JValue::Integer(i) => cinteger(i64::from(*i)),
        JValue::Double(d) => p::ValueSpecification::CFloat(Arc::new(p::CFloat {
            value: *d,
            exact: None,
            pos: None,
        })),
        JValue::Boolean(b) => p::ValueSpecification::CBoolean(Arc::new(p::CBoolean { value: *b, pos: None })),
        // `value.getClass().getName()` (a literal is never null here: the model's `Literal.value` is non-optional)
        JValue::BigInteger(_) | JValue::BigDecimal(_) => {
            return Err(Thrown::model(
                Phase::Normalize,
                cat!("Unsupported literal type: ", value.java_class()),
                None,
            ));
        }
    })
}

/// `RelOpTranslator.comparisonFn` (:703): a comparison's function (`equal`, or the Any-typed Lite ordering shims).
pub fn comparison_fn(op: ComparisonOp) -> &'static str {
    match op {
        ComparisonOp::EQ => "equal",
        // wrapped in not(...) at the call site
        ComparisonOp::NEQ => "equal",
        ComparisonOp::LT => lite::LESS_THAN_ANY,
        ComparisonOp::LTE => lite::LESS_THAN_EQUAL_ANY,
        ComparisonOp::GT => lite::GREATER_THAN_ANY,
        ComparisonOp::GTE => lite::GREATER_THAN_EQUAL_ANY,
    }
}

/// `RelOpTranslator.dynaFnName` (:726): a non-armed dynafunction's spelling (PURE: the call's name; SHIM: its Lite
/// FQN); a TRANSLATED one in a shape no arm rewrites, or an UNSUPPORTED one, refused by name.
pub fn dyna_fn_name(call: &RelationalOperationFunctionCall) -> Result<JStr, Thrown> {
    let Some(d) = dyna(call) else {
        return Ok(call.name.clone());
    };
    match d.resolution() {
        Resolution::PURE => Ok(call.name.clone()),
        // `liteFqn()`'s IllegalStateException ("<name> is not a SHIM") for a SHIM naming no one Lite FQN
        Resolution::SHIM => d
            .lite_fqn()
            .map(JStr::from)
            .map_err(|e| Thrown::illegal_state(JStr::from(e))),
        // a TRANSLATED name in a shape no arm rewrites (an arm's guard did not match): refused by name, as the
        // UNSUPPORTED arm refuses one (ruling 29, the normalizer slice's step 1)
        Resolution::TRANSLATED => {
            let n = call.args.len();
            Err(Thrown::not_implemented(cat!(
                "engine dynafunction '",
                call.name,
                "' is not supported by this platform yet in this shape (",
                n.to_string(),
                if n == 1 { " argument" } else { " arguments" },
                "; DynaFn.",
                java_name(d),
                ": no translator arm rewrites it)"
            )))
        }
        Resolution::UNSUPPORTED => Err(Thrown::not_implemented(cat!(
            "engine dynafunction '",
            call.name,
            "' is not supported by this platform yet (DynaFn.",
            java_name(d),
            "; the engine's dialects that register it: docs/engine-dynafunctions.tsv)"
        ))),
    }
}

/// `RelOpTranslator.variadicRun` (:752): two or more arguments to a variadic-run name as one collection
/// (`Pure.isVariadicRun` is `lite_builtin::pure::catalog().is_variadic_run`, refusing a bare name). The name (`fn`) is
/// unread, as in Java.
pub fn variadic_run(
    _f: &JStr,
    fqns: &[JStr],
    args: Vec<p::ValueSpecification>,
) -> Result<Vec<p::ValueSpecification>, Thrown> {
    Ok(if is_run(args.len(), fqns)? {
        vec![collection(args)]
    } else {
        args
    })
}

/// `variadicRun`'s test: `args.size() >= 2 && Pure.isVariadicRun(fqns)`, the catalog asked only for two or more.
fn is_run(arity: usize, fqns: &[JStr]) -> Result<bool, Thrown> {
    if arity < 2 {
        return Ok(false);
    }
    // `Pure.nativeFunctionsAt`'s refusal of a bare name is an IllegalArgumentException
    lite_builtin::pure::catalog()
        .is_variadic_run(fqns)
        .map_err(|e| Thrown::illegal_argument(JStr::from(e)))
}

/// `RelOpTranslator.dynaFnCandidates` (:761): a PURE dynafunction's catalog FQNs, else none.
pub fn dyna_fn_candidates(call: &RelationalOperationFunctionCall) -> Vec<JStr> {
    match dyna(call) {
        Some(d) if d.resolution() == Resolution::PURE => d.fqns().iter().map(|f| JStr::from(f.as_str())).collect(),
        _ => Vec::new(),
    }
}

/// `RelOpTranslator.operatorCall` (:770): `Calls.among`, or `Calls.infixRunAmong` for a variadic run (module note on
/// `shaped == args`).
pub fn operator_call(
    f: &JStr,
    candidates: &[JStr],
    args: Vec<p::ValueSpecification>,
) -> Result<p::AppliedFunction, Thrown> {
    // `shaped == args` is false exactly when variadicRun reshaped: its own test; the run takes the arguments themselves
    // (infixRunAmong makes the one collection)
    Ok(if is_run(args.len(), candidates)? {
        calls::infix_run_among(f, candidates, args)
    } else {
        calls::among(f, candidates, args)
    })
}

/// `tableScope.keySet()` as Java prints it.
fn key_set(table_scope: &TableScope) -> JStr {
    let keys: Vec<JStr> = table_scope.iter().map(|(k, _)| k.clone()).collect();
    java_list(&keys)
}

/// Java's `Enum.name()` of a dynafunction: lite-builtin's generated enum spells each member as Java's constant, so its
/// derived `Debug` is the constant's name.
fn java_name(d: DynaFn) -> String {
    format!("{d:?}")
}

/// `list.get(i)` on an immutable list (the record's `args`, `translateArgs`' `toList()`): an index past its end is the
/// JDK's `ArrayIndexOutOfBoundsException` (module note).
fn at<T>(list: &[T], i: usize) -> Result<&T, Thrown> {
    list.get(i).ok_or_else(|| {
        Thrown::java(
            ARRAY_INDEX_OUT_OF_BOUNDS,
            Some(cat!(
                "Index ",
                i.to_string(),
                " out of bounds for length ",
                list.len().to_string()
            )),
        )
    })
}

/// `s.equalsIgnoreCase(lit)`.
fn ignore_case_is(s: &JStr, lit: &str) -> bool {
    let lit: Vec<u16> = lit.encode_utf16().collect();
    equals_ignore_case(&s.0, &lit)
}

/// `op instanceof Literal lit && lit.value() instanceof String s`: `s`.
fn string_literal(op: &RelationalOperation) -> Option<&JStr> {
    match op {
        RelationalOperation::RelationalOperationLiteral(lit) => match &lit.value {
            JValue::String(s) => Some(s),
            _ => None,
        },
        _ => None,
    }
}

/// `String.toUpperCase(Locale.ROOT)` over UTF-16 units: each code point's full upper-case mapping, a lone surrogate
/// kept (lite-front's `database_parser::to_upper_root`, crate-private there).
fn to_upper_root(s: &JStr) -> JStr {
    let mut out = Vec::with_capacity(s.len());
    for r in char::decode_utf16(s.0.iter().copied()) {
        match r {
            Ok(c) => {
                for up in c.to_uppercase() {
                    let mut buf = [0u16; 2];
                    out.extend_from_slice(up.encode_utf16(&mut buf));
                }
            }
            Err(e) => out.push(e.unpaired_surrogate()),
        }
    }
    JStr(out)
}

// The protocol nodes the translator makes (Java's position-free convenience constructors). Calls are built by
// lite-names' `calls` alone; these make the other nodes.

/// A built call as a value.
pub(crate) fn vs(f: p::AppliedFunction) -> p::ValueSpecification {
    p::ValueSpecification::AppliedFunction(Arc::new(f))
}

/// `Calls.platform(name, args)`.
pub(crate) fn platform(name: &str, args: Vec<p::ValueSpecification>) -> p::ValueSpecification {
    vs(calls::platform(&JStr::from(name), args))
}

/// `Calls.exact(fqn, args)`.
pub(crate) fn exact(fqn: &str, args: Vec<p::ValueSpecification>) -> p::ValueSpecification {
    vs(calls::exact(&JStr::from(fqn), args))
}

/// `Calls.infixRun(op, operands)`.
fn infix_run(op: &str, operands: Vec<p::ValueSpecification>) -> p::ValueSpecification {
    vs(calls::infix_run(&JStr::from(op), operands))
}

/// `new AppliedProperty(receiver, property)`: no position, no owner class.
fn property(receiver: p::ValueSpecification, name: &JStr) -> p::ValueSpecification {
    p::ValueSpecification::AppliedProperty(Arc::new(p::AppliedProperty {
        receiver,
        property: name.clone(),
        pos: None,
        owner_class: None,
    }))
}

/// `new CString(value)`: not multi-line.
fn cstring(value: JStr) -> p::ValueSpecification {
    p::ValueSpecification::CString(Arc::new(p::CString {
        value,
        pos: None,
        multi_line: false,
    }))
}

/// `new CInteger(long)`.
fn cinteger(value: i64) -> p::ValueSpecification {
    p::ValueSpecification::CInteger(Arc::new(p::CInteger {
        value: JValue::Long(value),
        pos: None,
    }))
}

/// `new EnumValue(fullPath, value)`: no spans, so (the parser's form) the real `enumValue` node.
fn enum_value(full_path: &str, value: JStr) -> p::ValueSpecification {
    p::ValueSpecification::EnumValue(Arc::new(p::EnumValue {
        full_path: JStr::from(full_path),
        value,
        enumeration_pos: None,
        pos: None,
        node: true,
    }))
}

/// `new LambdaFunction(List.of(), List.of(body))`: a thunk.
fn thunk(body: p::ValueSpecification) -> p::ValueSpecification {
    p::ValueSpecification::LambdaFunction(Arc::new(p::LambdaFunction {
        parameters: Arc::default(),
        body: Arc::from(vec![body]),
        pos: None,
    }))
}

/// `new PureCollection(values)`.
fn collection(values: Vec<p::ValueSpecification>) -> p::ValueSpecification {
    p::ValueSpecification::PureCollection(Arc::new(p::PureCollection {
        values: values.into(),
        pos: None,
    }))
}

/// `new TypeAnnotation.Named(new TypeExpression.NameRef(name))`.
pub(crate) fn type_named(name: &str) -> p::ValueSpecification {
    type_named_j(JStr::from(name))
}

/// [`type_named`] of a Java string.
pub(crate) fn type_named_j(name: JStr) -> p::ValueSpecification {
    p::ValueSpecification::TypeAnnotation(p::TypeAnnotation::TypeAnnotationNamed(Arc::new(
        p::TypeAnnotationNamed {
            r#type: p::TypeExpression::TypeExpressionNameRef(Box::new(p::TypeExpressionNameRef { name, pos: None })),
            pos: None,
        },
    )))
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use lite_model::text::j;
    use lite_model::{
        BigDecimal, RelationalOperationArrayLiteral, RelationalOperationBooleanOp, RelationalOperationComparison,
        RelationalOperationGroup, RelationalOperationIsNull, RelationalOperationLambda, RelationalOperationLambdaParam,
        RelationalOperationLiteral, RelationalOperationTargetColumnRef,
    };

    use super::*;
    use crate::lifted_views::LiftedViews;
    use crate::mapping_ledger::MappingLedger;
    use crate::test_support::{index, model};

    type Op = RelationalOperation;

    fn s(v: &str) -> Op {
        Op::RelationalOperationLiteral(Arc::new(RelationalOperationLiteral {
            value: JValue::String(j(v)),
        }))
    }

    fn int(v: i64) -> Op {
        Op::RelationalOperationLiteral(Arc::new(RelationalOperationLiteral { value: JValue::Long(v) }))
    }

    /// `{target}.c`: outside a join condition, a ModelException naming `c` -- the probe of evaluation order.
    fn tcol(c: &str) -> Op {
        Op::RelationalOperationTargetColumnRef(Arc::new(RelationalOperationTargetColumnRef { column: j(c) }))
    }

    fn col(table: &str, c: &str) -> Op {
        Op::RelationalOperationColumnRef(Arc::new(RelationalOperationColumnRef {
            database_name: None,
            table: j(table),
            column: j(c),
        }))
    }

    fn fc(name: &str, args: Vec<Op>) -> RelationalOperationFunctionCall {
        RelationalOperationFunctionCall {
            name: j(name),
            args: args.into(),
        }
    }

    fn call(name: &str, args: Vec<Op>) -> Op {
        Op::RelationalOperationFunctionCall(Arc::new(fc(name, args)))
    }

    fn cmp(left: Op, op: ComparisonOp, right: Op) -> Op {
        Op::RelationalOperationComparison(Arc::new(RelationalOperationComparison { left, op, right }))
    }

    fn row() -> p::ValueSpecification {
        p::ValueSpecification::Variable(Arc::new(variable("row")))
    }

    fn variable(name: &str) -> p::Variable {
        p::Variable {
            name: j(name),
            r#type: None,
            multiplicity: None,
            pos: None,
        }
    }

    /// Outside any pipeline and join condition.
    fn tr(op: &Op) -> Result<p::ValueSpecification, Thrown> {
        translate(op, &TableScope::new(), None, None, None)
    }

    /// A translation's tree as text: `f(a, b)` (an infix run `f!(..)`), `'s'`, `7`, `$v`, `r.p`, `P.V`, `{|b}`, `[..]`,
    /// `@T`; a platform function by its name's last segment.
    fn show(v: &p::ValueSpecification) -> String {
        let all = |vs: &[p::ValueSpecification]| vs.iter().map(show).collect::<Vec<_>>().join(", ");
        match v {
            p::ValueSpecification::AppliedFunction(f) => {
                // a platform function by its last segment (the builder records the one it names); the Lite natives
                // in full
                let name = f.function.lossy();
                let name = match name.strip_prefix("meta::pure::functions::") {
                    Some(rest) => rest.rsplit("::").next().unwrap_or(rest).to_owned(),
                    None => name,
                };
                format!("{name}{}({})", if f.infix { "!" } else { "" }, all(&f.parameters))
            }
            p::ValueSpecification::AppliedProperty(ap) => format!("{}.{}", show(&ap.receiver), ap.property),
            p::ValueSpecification::CString(c) => format!("'{}'", c.value),
            p::ValueSpecification::CInteger(c) => match &c.value {
                JValue::Long(l) => format!("{l}"),
                other => format!("{other:?}"),
            },
            p::ValueSpecification::CFloat(c) => format!("{}f", c.value),
            p::ValueSpecification::CBoolean(c) => format!("{}", c.value),
            p::ValueSpecification::Variable(v) => format!("${}", v.name),
            p::ValueSpecification::EnumValue(e) => {
                assert!(e.node, "a synthesized EnumValue is the real node");
                format!("{}.{}", e.full_path, e.value)
            }
            p::ValueSpecification::LambdaFunction(l) => {
                assert!(l.parameters.is_empty());
                format!("{{|{}}}", all(&l.body))
            }
            p::ValueSpecification::PureCollection(c) => format!("[{}]", all(&c.values)),
            p::ValueSpecification::TypeAnnotation(p::TypeAnnotation::TypeAnnotationNamed(n)) => match &n.r#type {
                p::TypeExpression::TypeExpressionNameRef(nr) => format!("@{}", nr.name),
                other => format!("@{other:?}"),
            },
            other => format!("{other:?}"),
        }
    }

    fn shown(op: &Op) -> String {
        show(&tr(op).expect("translates"))
    }

    fn message(e: &Thrown) -> String {
        e.message_or_null().lossy()
    }

    const T1: &str = "meta::legend::lite::trustOne";

    #[test]
    fn literals_are_pure_constants_and_a_big_number_names_its_java_class() {
        let lit = |v: JValue| show(&literal_to_value_spec(&v).unwrap());
        assert_eq!(lit(JValue::String(j("a"))), "'a'");
        assert_eq!(lit(JValue::Long(7)), "7");
        // `new CInteger((long) i)`: an Integer is widened
        let widened = literal_to_value_spec(&JValue::Integer(3)).unwrap();
        let p::ValueSpecification::CInteger(c) = &widened else {
            panic!("{widened:?}")
        };
        assert_eq!(c.value, JValue::Long(3));
        assert_eq!(lit(JValue::Double(1.5)), "1.5f");
        assert_eq!(lit(JValue::Boolean(true)), "true");
        let big = literal_to_value_spec(&JValue::BigInteger("123456789012345678901".to_owned())).unwrap_err();
        assert!(big.is_model());
        assert_eq!(big.phase(), Some(Phase::Normalize));
        assert_eq!(message(&big), "Unsupported literal type: java.math.BigInteger");
        let dec = literal_to_value_spec(&JValue::BigDecimal(BigDecimal::parse("1.25").unwrap())).unwrap_err();
        assert_eq!(message(&dec), "Unsupported literal type: java.math.BigDecimal");
    }

    #[test]
    fn comparisons_spell_equal_and_the_any_typed_ordering_shims() {
        assert_eq!(comparison_fn(ComparisonOp::EQ), "equal");
        assert_eq!(comparison_fn(ComparisonOp::NEQ), "equal");
        assert_eq!(comparison_fn(ComparisonOp::LT), "meta::legend::lite::lessThan");
        assert_eq!(comparison_fn(ComparisonOp::LTE), "meta::legend::lite::lessThanEqual");
        assert_eq!(comparison_fn(ComparisonOp::GT), "meta::legend::lite::greaterThan");
        assert_eq!(comparison_fn(ComparisonOp::GTE), "meta::legend::lite::greaterThanEqual");
        assert_eq!(shown(&cmp(s("a"), ComparisonOp::NEQ, int(1))), "not(equal('a', 1))");
        assert_eq!(
            shown(&cmp(s("a"), ComparisonOp::LT, int(1))),
            "meta::legend::lite::lessThan('a', 1)"
        );
        // left before right
        let e = tr(&cmp(tcol("L"), ComparisonOp::EQ, tcol("R"))).unwrap_err();
        assert!(message(&e).contains("{target}.L"), "{e:?}");
    }

    #[test]
    fn the_extracted_sql_type_is_the_engines_scalar_list_case_folded() {
        let kind = |t: &str| pure_type_for(&j(t)).unwrap().lossy();
        assert_eq!(kind("varchar"), "String");
        assert_eq!(kind("Int"), "Integer");
        assert_eq!(kind("double"), "Float");
        assert_eq!(kind("numeric"), "Decimal");
        assert_eq!(kind("bit"), "Boolean");
        assert_eq!(kind("date"), "StrictDate");
        assert_eq!(kind("datetime"), "DateTime");
        // Java's toUpperCase folds the ligature to FL
        assert_eq!(kind("\u{fb02}oat"), "Float");
        let e = pure_type_for(&j("semistructured")).unwrap_err();
        assert!(e.is_model());
        assert_eq!(
            message(&e),
            "Unsupported SQL type 'semistructured' in extractFromSemiStructured (the engine's scalar list is BOOLEAN, \
             CHAR, VARCHAR, STRING, INTEGER, DECIMAL, FLOAT, DATE, DATETIME, TIMESTAMP; SEMISTRUCTURED and []-suffixed \
             array element types are not realized here yet)"
        );
    }

    #[test]
    fn an_ambiguous_table_is_refused_with_guidance() {
        let e = ambiguous_table_ref(&j("T"), &j("C"));
        assert!(e.is_model());
        assert_eq!(e.phase(), Some(Phase::Normalize));
        assert_eq!(
            message(&e),
            "Ambiguous column reference 'T.C': the join chain reaches table 'T' through more than one path, so a bare \
             column reference cannot identify which sub-row is meant. Pin the intended sub-row with a join-terminal \
             column (e.g. @SomeJoin | T.C)."
        );
    }

    #[test]
    fn a_column_read_takes_its_tables_row_else_the_default_tables() {
        let mut scope = TableScope::new();
        scope.put(j("T"), row());
        scope.put(j("U"), p::ValueSpecification::Variable(Arc::new(variable("u"))));
        let read = |t: &str, d: &str, pl: Option<&Pipeline<'_>>| column_read(&j(t), &j("C"), &scope, &j(d), pl);
        assert_eq!(show(&read("U", "T", None).unwrap()), "$u.C");
        assert_eq!(show(&read("X", "T", None).unwrap()), "$row.C");
        let e = read("X", "Y", None).unwrap_err();
        assert!(e.is_model());
        assert_eq!(message(&e), "No row variable in scope for table 'X'; available=[T, U]");
        // a table two slots reach: loud before the default table is asked
        let parsed = model("###Pure\nClass a::A\n{\n  n: String[1];\n}\n");
        let idx = index(&parsed);
        let views = LiftedViews::new(&parsed, &idx);
        let mut ledger = MappingLedger::new(HashSet::new());
        let mut pipe = Pipeline::new(row(), &mut ledger, &views);
        pipe.ambiguous_tables.insert(j("X"));
        let e = read("X", "T", pipe.view()).unwrap_err();
        assert!(message(&e).starts_with("Ambiguous column reference 'X.C'"), "{e:?}");
        // a table in scope is never ambiguous
        pipe.ambiguous_tables.insert(j("T"));
        assert_eq!(show(&read("T", "T", pipe.view()).unwrap()), "$row.C");
    }

    #[test]
    fn a_target_column_reads_the_target_var_and_is_loud_outside_a_join_condition() {
        let target = p::ValueSpecification::Variable(Arc::new(variable("t")));
        let read = translate(&tcol("ID"), &TableScope::new(), Some(&target), None, None).unwrap();
        assert_eq!(show(&read), "$t.ID");
        let e = tr(&tcol("ID")).unwrap_err();
        assert!(e.is_model());
        assert_eq!(
            message(&e),
            "TargetColumnRef {target}.ID outside a join condition context"
        );
    }

    #[test]
    fn the_hash_arms_spell_the_hash_type_and_take_no_arity_guard() {
        assert_eq!(
            shown(&call("md5", vec![s("a")])),
            format!("hash({T1}('a'), meta::pure::functions::hash::HashType.MD5)")
        );
        // extra arguments are ignored, as Java's
        assert_eq!(
            shown(&call("sha256", vec![s("a"), s("b")])),
            format!("hash({T1}('a'), meta::pure::functions::hash::HashType.SHA256)")
        );
        // no argument: Java's `args.get(0)` of the empty immutable list
        let e = tr(&call("sha1", vec![])).unwrap_err();
        assert_eq!(e.java_class(), "java.lang.ArrayIndexOutOfBoundsException");
        assert_eq!(message(&e), "Index 0 out of bounds for length 0");
    }

    #[test]
    fn the_format_arms_land_on_the_lite_natives() {
        assert_eq!(
            shown(&call("convertDate", vec![s("x")])),
            "meta::legend::lite::convertDateFormat('x', 'yyyy-MM-dd')"
        );
        assert_eq!(
            shown(&call("convertDate", vec![s("x"), s("f")])),
            "meta::legend::lite::convertDateFormat('x', 'f')"
        );
        let e = tr(&call("convertDate", vec![])).unwrap_err();
        assert_eq!(e.java_class(), "java.lang.ArrayIndexOutOfBoundsException");
        assert_eq!(message(&e), "Index 0 out of bounds for length 0");
        assert_eq!(
            shown(&call("toTimestamp", vec![s("x"), s("f")])),
            "meta::legend::lite::convertDateTimeFormat('x', 'f')"
        );
        assert_eq!(
            shown(&call("parseDate", vec![s("x"), s("f")])),
            "meta::legend::lite::parseDateFormat('x', 'f')"
        );
        assert_eq!(
            shown(&call("convertTimeZone", vec![s("x"), s("z"), s("f")])),
            "meta::legend::lite::convertTimeZoneFormat('x', 'z', 'f')"
        );
        assert_eq!(shown(&call("convertVarchar128", vec![int(1)])), "cast(1, @String)");
    }

    #[test]
    fn day_of_week_number_takes_a_week_start_checked_before_its_operand() {
        assert_eq!(
            shown(&call("dayOfWeekNumber", vec![s("d")])),
            format!("plus!([mod(dayOfWeekNumber({T1}('d')), 7), 1])")
        );
        assert_eq!(
            shown(&call("dayOfWeekNumber", vec![s("d"), s("mONDAY")])),
            format!("dayOfWeekNumber({T1}('d'))")
        );
        let e = tr(&call("dayOfWeekNumber", vec![tcol("D"), s("Tuesday")])).unwrap_err();
        assert!(e.is_not_implemented());
        assert_eq!(
            message(&e),
            "dayOfWeekNumber requires 'Sunday' or 'Monday' as the week start (engine assert)"
        );
        assert!(
            tr(&call("dayOfWeekNumber", vec![tcol("D"), int(1)]))
                .unwrap_err()
                .is_not_implemented()
        );
        assert_eq!(shown(&call("dayOfWeek", vec![s("d")])), "toString(dayOfWeek('d'))");
    }

    #[test]
    fn a_guard_that_does_not_match_falls_to_the_generic_arm_and_its_refusal() {
        let e = tr(&call("concat", vec![tcol("A")])).unwrap_err();
        assert!(e.is_not_implemented(), "the name is refused before the operand: {e:?}");
        assert_eq!(
            message(&e),
            "engine dynafunction 'concat' is not supported by this platform yet in this shape (1 argument; \
             DynaFn.CONCAT: no translator arm rewrites it)"
        );
        let e = tr(&call("case", vec![s("c"), s("v")])).unwrap_err();
        assert_eq!(
            message(&e),
            "engine dynafunction 'case' is not supported by this platform yet in this shape (2 arguments; \
             DynaFn.CASE: no translator arm rewrites it)"
        );
        // adjust's unit must be a string literal
        let e = tr(&call("adjust", vec![s("d"), int(1), int(2)])).unwrap_err();
        assert!(message(&e).contains("(3 arguments; DynaFn.ADJUST:"), "{e:?}");
        let e = tr(&call("extractFromSemiStructured", vec![s("j"), s("p"), int(1)])).unwrap_err();
        assert!(
            message(&e).contains("(3 arguments; DynaFn.EXTRACT_FROM_SEMI_STRUCTURED:"),
            "{e:?}"
        );
        // an armed PURE name whose guard does not match is its pure self
        assert_eq!(
            shown(&call("and", vec![s("a"), s("b")])),
            format!("and({T1}('a'), {T1}('b'))")
        );
        let unsupported = tr(&call("array_append", vec![tcol("A")])).unwrap_err();
        assert!(unsupported.is_not_implemented());
        assert_eq!(
            message(&unsupported),
            "engine dynafunction 'array_append' is not supported by this platform yet (DynaFn.ARRAY_APPEND; the \
             engine's dialects that register it: docs/engine-dynafunctions.tsv)"
        );
    }

    #[test]
    fn the_generic_arm_trusts_every_operand_and_resolves_its_name() {
        // a name the engine registers as no dynafunction: itself, no record
        let plain = tr(&call("foo", vec![s("a"), int(1)])).unwrap();
        assert_eq!(show(&plain), format!("foo({T1}('a'), {T1}(1))"));
        let p::ValueSpecification::AppliedFunction(f) = &plain else {
            panic!()
        };
        assert!(f.referents.is_empty() && !f.infix);
        // a SHIM takes its Lite identity
        assert_eq!(
            dyna_fn_name(&fc("greaterThan", vec![int(1), int(2)])).unwrap(),
            j("meta::legend::lite::greaterThan")
        );
        assert!(dyna_fn_candidates(&fc("greaterThan", vec![])).is_empty());
        // a PURE variadic name with two operands is one run among its declarations
        let candidates = dyna_fn_candidates(&fc("plus", vec![]));
        assert!(!candidates.is_empty());
        let run = tr(&call("plus", vec![int(1), int(2)])).unwrap();
        assert_eq!(show(&run), format!("plus!([{T1}(1), {T1}(2)])"));
        let p::ValueSpecification::AppliedFunction(f) = &run else {
            panic!()
        };
        assert_eq!(&f.referents[..], &candidates[..]);
        // ... and with one, a plain call among them
        let one = tr(&call("plus", vec![int(1)])).unwrap();
        assert_eq!(show(&one), format!("plus({T1}(1))"));
        // a collection operand stays bare
        let arr = Op::RelationalOperationArrayLiteral(Arc::new(RelationalOperationArrayLiteral {
            elements: vec![int(1), int(2)].into(),
        }));
        assert_eq!(shown(&call("foo", vec![arr])), "foo([1, 2])");
    }

    #[test]
    fn a_variadic_run_asks_the_catalog_only_for_two_operands() {
        let bare = [j("plus")];
        let one = vec![tr(&int(1)).unwrap()];
        assert_eq!(variadic_run(&j("plus"), &bare, one.clone()).unwrap(), one);
        assert!(!operator_call(&j("plus"), &bare, one).unwrap().infix);
        let two = vec![tr(&int(1)).unwrap(), tr(&int(2)).unwrap()];
        let e = operator_call(&j("plus"), &bare, two.clone()).unwrap_err();
        assert_eq!(e.java_class(), "java.lang.IllegalArgumentException");
        assert!(
            message(&e).starts_with("'plus' is a bare name, not a declaration"),
            "{e:?}"
        );
        let plus = dyna_fn_candidates(&fc("plus", vec![]));
        let shaped = variadic_run(&j("plus"), &plus, two.clone()).unwrap();
        assert_eq!(shaped.len(), 1);
        assert_eq!(show(&shaped[0]), "[1, 2]");
        // the run takes the operands themselves: one collection, not two
        let f = operator_call(&j("plus"), &plus, two).unwrap();
        assert!(f.infix);
        assert_eq!(show(&vs(f)), "plus!([1, 2])");
    }

    #[test]
    fn evaluation_order_decides_which_error_surfaces() {
        let names = |op: Op| {
            let e = tr(&op).unwrap_err();
            assert!(e.is_model(), "{e:?}");
            message(&e)
        };
        // case: the default first
        assert!(names(call("case", vec![tcol("C"), tcol("V"), tcol("D")])).contains("{target}.D"));
        // position: the haystack (the second operand) first
        assert!(names(call("position", vec![tcol("N"), tcol("H")])).contains("{target}.H"));
        // extract: the operands before the SQL type
        assert!(
            names(call(
                "extractFromSemiStructured",
                vec![tcol("A"), s("p"), s("semistructured")]
            ))
            .contains("{target}.A")
        );
        assert!(
            names(call(
                "extractFromSemiStructured",
                vec![s("j"), s("p"), s("semistructured")]
            ))
            .starts_with("Unsupported SQL type")
        );
        // splitPart, concat, if, or: left to right
        assert!(names(call("splitPart", vec![tcol("A"), tcol("B"), tcol("C")])).contains("{target}.A"));
        assert!(names(call("concat", vec![tcol("A"), tcol("B")])).contains("{target}.A"));
        assert!(names(call("if", vec![tcol("A"), tcol("B"), tcol("C")])).contains("{target}.A"));
        assert!(names(call("or", vec![tcol("A"), tcol("B"), tcol("C")])).contains("{target}.A"));
    }

    #[test]
    fn the_rewriting_arms_spell_their_pure_forms() {
        assert_eq!(
            shown(&call("case", vec![s("c1"), s("v1"), s("c2"), s("v2"), s("d")])),
            "if('c1', {|'v1'}, {|if('c2', {|'v2'}, {|'d'})})"
        );
        assert_eq!(
            shown(&call("concat", vec![s("a"), int(1)])),
            format!("plus!([{T1}(cast('a', @String)), {T1}(cast(1, @String))])")
        );
        assert_eq!(
            shown(&call("splitPart", vec![s("a"), s(" "), s("2")])),
            format!("splitPart('a', {T1}(' '), minus!([cast({T1}('2'), @Integer), 1]))")
        );
        assert_eq!(
            shown(&call("extractFromSemiStructured", vec![s("j"), s("p"), s("varchar")])),
            "to(get('j', 'p'), @String)"
        );
        assert_eq!(
            shown(&call("adjust", vec![s("d"), int(1), s("days")])),
            format!("adjust({T1}('d'), {T1}(1), meta::pure::functions::date::DurationUnit.DAYS)")
        );
        assert_eq!(shown(&call("indexOf", vec![s("h"), s("n")])), "indexOf('h', 'n')");
        assert_eq!(
            shown(&call("position", vec![s("n"), s("h")])),
            format!("indexOf({T1}('h'), 'n')")
        );
        assert_eq!(
            shown(&call("substring", vec![s("a"), int(1), int(2)])),
            format!("substring({T1}('a'), {T1}(1), {T1}(2))")
        );
        assert_eq!(
            shown(&call("add", vec![int(1), int(2)])),
            format!("plus!([{T1}(1), {T1}(2)])")
        );
        assert_eq!(
            shown(&call("sub", vec![int(1), int(2)])),
            format!("minus!([{T1}(1), {T1}(2)])")
        );
        assert_eq!(shown(&call("isNull", vec![s("a")])), "isEmpty('a')");
        assert_eq!(shown(&call("isNotNull", vec![s("a")])), "isNotEmpty('a')");
        assert_eq!(shown(&call("group", vec![s("a")])), "'a'");
        assert_eq!(
            shown(&call("if", vec![s("c"), s("t"), s("e")])),
            "if('c', {|'t'}, {|'e'})"
        );
        assert_eq!(
            shown(&call("or", vec![s("a"), s("b"), s("c")])),
            "or(or('a', 'b'), 'c')"
        );
    }

    #[test]
    fn the_structural_kinds_translate_and_a_lambda_is_refused_by_name() {
        let both = Op::RelationalOperationBooleanOp(Arc::new(RelationalOperationBooleanOp {
            left: s("a"),
            op: LogicalOp::AND,
            right: s("b"),
        }));
        assert_eq!(shown(&both), "and('a', 'b')");
        let null = Op::RelationalOperationIsNull(Arc::new(RelationalOperationIsNull { operand: s("a") }));
        assert_eq!(shown(&null), "isEmpty('a')");
        let group = Op::RelationalOperationGroup(Arc::new(RelationalOperationGroup { inner: s("a") }));
        assert_eq!(shown(&group), "'a'");
        let lam = Op::RelationalOperationLambda(Arc::new(RelationalOperationLambda {
            parameters: vec![j("x"), j("y")].into(),
            body: s("a"),
        }));
        let e = tr(&call("foo", vec![lam])).unwrap_err();
        assert!(e.is_not_implemented());
        assert_eq!(
            message(&e),
            "a lambda argument (x, y | ...) in a relational expression is not supported by this platform yet"
        );
        let lp = Op::RelationalOperationLambdaParam(Arc::new(RelationalOperationLambdaParam { name: j("x") }));
        assert_eq!(
            message(&tr(&lp).unwrap_err()),
            "a lambda parameter ($x) in a relational expression is not supported by this platform yet"
        );
    }

    #[test]
    fn a_join_navigation_needs_a_row_and_a_pipeline() {
        let jn = RelationalOperationJoinNavigation {
            database_name: None,
            chain: Vec::new().into(),
            terminal: None,
        };
        let row_var = Arc::new(variable("row"));
        let text = "Nested JoinNavigation in scope without pipeline; JoinNav inside association predicates or join \
                    conditions is not supported.";
        let e = join_navigation(&jn, &TableScope::new(), None, Some(&row_var), None).unwrap_err();
        assert!(e.is_model());
        assert_eq!(message(&e), text);
        let parsed = model("###Pure\nClass a::A\n{\n  n: String[1];\n}\n");
        let idx = index(&parsed);
        let views = LiftedViews::new(&parsed, &idx);
        let mut ledger = MappingLedger::new(HashSet::new());
        let pipe = Pipeline::new(row(), &mut ledger, &views);
        let e = join_navigation(&jn, &TableScope::new(), None, None, pipe.view()).unwrap_err();
        assert_eq!(message(&e), text);
    }

    #[test]
    fn rebasing_moves_the_accepted_columns_and_keeps_nested_navigations() {
        let nested = Op::RelationalOperationJoinNavigation(Arc::new(RelationalOperationJoinNavigation {
            database_name: None,
            chain: Vec::new().into(),
            terminal: Some(col("U", "Z")),
        }));
        let e = call("f", vec![col("T", "A"), col("U", "B"), nested.clone()]);
        let moved = rebase_to_table(&e, &j("END"), &|c: &RelationalOperationColumnRef| c.table == "U");
        assert_eq!(moved, call("f", vec![col("T", "A"), col("END", "B"), nested]));
        // nothing accepted: the very operation
        let same = rebase_to_table(&e, &j("END"), &|_: &RelationalOperationColumnRef| false);
        let (Op::RelationalOperationFunctionCall(a), Op::RelationalOperationFunctionCall(b)) = (&e, &same) else {
            panic!()
        };
        assert!(lite_base::same(a, b));
    }

    #[test]
    fn the_tables_an_expression_reads_are_collected_in_order() {
        let mut sink = LinkedSet::new();
        sink.add(j("U"));
        collect_tables_in(&call("f", vec![col("T", "A"), col("U", "B"), col("T", "C")]), &mut sink).unwrap();
        assert_eq!(sink.to_vec(), vec![j("U"), j("T")]);
        let nav = Op::RelationalOperationJoinNavigation(Arc::new(RelationalOperationJoinNavigation {
            database_name: None,
            chain: Vec::new().into(),
            terminal: None,
        }));
        let e = collect_tables_in(&call("f", vec![nav]), &mut sink).unwrap_err();
        assert!(e.is_model());
        assert_eq!(message(&e), "JoinNavigation inside expression");
    }

    // A column reference goes through `MappingNormalizer.canonicalTable`, another helper's stub until the fill is
    // merged: these run with `--ignored` then.

    #[test]
    #[ignore = "needs MappingNormalizer.canonicalTable (mapping_normalizer/relational.rs)"]
    fn a_column_operand_of_the_trusting_arms_is_trusted_twice_as_javas() {
        let mut scope = TableScope::new();
        scope.put(j("T"), row());
        let t = |op: &Op| show(&translate(op, &scope, None, None, None).unwrap());
        assert_eq!(
            t(&call("substring", vec![col("T", "A"), int(1)])),
            format!("substring({T1}({T1}($row.A)), {T1}(1))")
        );
        assert_eq!(t(&call("foo", vec![col("T", "A")])), format!("foo({T1}({T1}($row.A)))"));
        // translateArgs alone wraps once; position and concat once by other routes
        assert_eq!(t(&call("indexOf", vec![col("T", "A"), s("n")])), "indexOf($row.A, 'n')");
        assert_eq!(
            t(&call("dayOfWeek", vec![col("T", "A")])),
            format!("toString(dayOfWeek({T1}($row.A)))")
        );
        assert_eq!(
            t(&call("position", vec![s("n"), col("T", "A")])),
            format!("indexOf({T1}($row.A), 'n')")
        );
        // a comparison's column side is trusted; a join condition's (a target var bound) is not
        assert_eq!(
            t(&cmp(col("default.T", "A"), ComparisonOp::EQ, int(1))),
            format!("equal({T1}($row.A), 1)")
        );
        let target = p::ValueSpecification::Variable(Arc::new(variable("t")));
        let jc = translate(
            &cmp(col("T", "A"), ComparisonOp::EQ, tcol("B")),
            &scope,
            Some(&target),
            None,
            None,
        )
        .unwrap();
        assert_eq!(show(&jc), "equal($row.A, $t.B)");
        // a grouped column is not a bare column operand
        let grouped = Op::RelationalOperationGroup(Arc::new(RelationalOperationGroup { inner: col("T", "A") }));
        assert_eq!(t(&call("foo", vec![grouped])), format!("foo({T1}($row.A))"));
    }

    #[test]
    #[ignore = "needs MappingNormalizer.canonicalTable (mapping_normalizer/relational.rs)"]
    fn a_column_of_a_table_not_in_scope_is_loud() {
        let mut scope = TableScope::new();
        scope.put(j("T"), row());
        scope.put(j("U"), row());
        let e = translate(&col("X", "A"), &scope, None, None, None).unwrap_err();
        assert!(e.is_model());
        assert_eq!(
            message(&e),
            "ColumnRef references table 'X' not in scope; available=[T, U]"
        );
        let parsed = model("###Pure\nClass a::A\n{\n  n: String[1];\n}\n");
        let idx = index(&parsed);
        let views = LiftedViews::new(&parsed, &idx);
        let mut ledger = MappingLedger::new(HashSet::new());
        let mut pipe = Pipeline::new(row(), &mut ledger, &views);
        pipe.ambiguous_tables.insert(j("X"));
        let e = translate(&col("default.X", "A"), &scope, None, None, pipe.view()).unwrap_err();
        assert!(message(&e).starts_with("Ambiguous column reference 'X.A'"), "{e:?}");
    }
}
