// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! The portable spellings of a Float's literal text (`FloatCanonText.of`) and its print form (`FloatPrintText.of`),
//! as SQL over the database's text of the double: the trees the renderer's `FLOAT_CANON` and `FLOAT_PRINT` arms
//! write (`AnsiSqlRenderer.floatCanonByText`, `floatPrintByText`). The first passes of a dialect that does not spell
//! the calls itself (`FloatCanonText.Expansion`, `FloatPrintText.Expansion`, [`crate::passes`]) build the same trees
//! before every other pass sees the statement; their bodies are the next step's.

use crate::build::{call, case, cast, float_lit, int_lit, null_lit, string_lit, when};
use lite_sql::{Scalar, SqlExpr, SqlFn, SqlType};

fn varchar() -> SqlType {
    SqlType::Scalar(Scalar::Varchar)
}

/// Pure's literal spelling of the double `v`, as SQL over its text (`FloatCanonText.of`).
pub fn canon_of(v: SqlExpr) -> SqlExpr {
    of_text(cast(v, varchar()))
}

/// The exponent unfolded to fixed point where the text `base` carries one, else the text as it is
/// (`FloatCanonText.fixedPoint`).
pub fn fixed_point(base: SqlExpr) -> SqlExpr {
    case(vec![when(call(SqlFn::Or, vec![has(&base, "e"), has(&base, "E")]), exponent_unfold(&base))], Some(base))
}

/// Pure's literal spelling of a double over `base`, the database's text of it (`FloatCanonText.ofText`).
pub fn of_text(base: SqlExpr) -> SqlExpr {
    let unfolded = exponent_unfold(&base);
    case(
        vec![
            // every zero is '0.0', detected textually
            when(call(SqlFn::RegexpFullMatch, vec![base.clone(), string_lit("-?0+(\\.0+)?")]), string_lit("0.0")),
            // both wire spellings of the exponent
            when(call(SqlFn::Or, vec![has(&base, "e"), has(&base, "E")]), unfolded),
            when(call(SqlFn::Not, vec![has(&base, ".")]), call(SqlFn::Concat, vec![base.clone(), string_lit(".0")])),
        ],
        Some(base),
    )
}

/// The complete textual exponent unfold: the mantissa's digits shifted by the exponent, as text.
fn exponent_unfold(base: &SqlExpr) -> SqlExpr {
    let sign = case(
        vec![when(call(SqlFn::StartsWith, vec![base.clone(), string_lit("-")]), string_lit("-"))],
        Some(string_lit("")),
    );
    let mant = call(SqlFn::RegexpExtract, vec![base.clone(), string_lit("-?([0-9]+(?:\\.[0-9]+)?)[eE]"), int_lit(1)]);
    let digits = call(SqlFn::Replace, vec![mant.clone(), string_lit("."), string_lit("")]);
    let dot_pos = call(SqlFn::Strpos, vec![mant.clone(), string_lit(".")]);
    let int_len = case(
        vec![when(call(SqlFn::Equal, vec![dot_pos.clone(), int_lit(0)]), call(SqlFn::Length, vec![mant]))],
        Some(call(SqlFn::Minus, vec![dot_pos, int_lit(1)])),
    );
    let exp_text = call(SqlFn::RegexpExtract, vec![base.clone(), string_lit("[eE]([+-]?[0-9]+)$"), int_lit(1)]);
    let exp = cast(
        case(vec![when(call(SqlFn::Equal, vec![exp_text.clone(), string_lit("")]), null_lit())], Some(exp_text)),
        SqlType::Scalar(Scalar::Integer),
    );
    let point_pos = call(SqlFn::Plus, vec![int_len, exp]);
    let d_len = call(SqlFn::Length, vec![digits.clone()]);
    let tiny = call(
        SqlFn::Concat,
        vec![
            call(SqlFn::Concat, vec![string_lit("0."), zeros(call(SqlFn::Minus, vec![int_lit(0), point_pos.clone()]))]),
            digits.clone(),
        ],
    );
    let huge = call(
        SqlFn::Concat,
        vec![
            call(
                SqlFn::Concat,
                vec![digits.clone(), zeros(call(SqlFn::Minus, vec![point_pos.clone(), d_len.clone()]))],
            ),
            string_lit(".0"),
        ],
    );
    let mid = call(
        SqlFn::Concat,
        vec![
            call(
                SqlFn::Concat,
                vec![call(SqlFn::Substring, vec![digits.clone(), int_lit(1), point_pos.clone()]), string_lit(".")],
            ),
            call(SqlFn::Substring, vec![digits, call(SqlFn::Plus, vec![point_pos.clone(), int_lit(1)])]),
        ],
    );
    let body = case(
        vec![
            when(call(SqlFn::LessEqual, vec![point_pos.clone(), int_lit(0)]), tiny),
            when(call(SqlFn::GreaterEqual, vec![point_pos, d_len]), huge),
        ],
        Some(mid),
    );
    call(SqlFn::Concat, vec![sign, body])
}

/// `n` zeros (RPAD over empty; a negative n yields '').
fn zeros(n: SqlExpr) -> SqlExpr {
    call(
        SqlFn::Rpad,
        vec![
            string_lit(""),
            cast(call(SqlFn::Greatest, vec![n, int_lit(0)]), SqlType::Scalar(Scalar::Integer)),
            string_lit("0"),
        ],
    )
}

fn has(text: &SqlExpr, needle: &str) -> SqlExpr {
    call(SqlFn::Greater, vec![call(SqlFn::Strpos, vec![text.clone(), string_lit(needle)]), int_lit(0)])
}

/// Pure's print form of the double `x`, as SQL over its text (`FloatPrintText.of`).
pub fn print_of(x: SqlExpr) -> SqlExpr {
    let s = cast(x.clone(), varchar());
    print(x, s)
}

/// Pure's print form of the double `x` whose text is `s` (`FloatPrintText.print`).
pub fn print(x: SqlExpr, s: SqlExpr) -> SqlExpr {
    let has_exp = call(
        SqlFn::Greater,
        vec![call(SqlFn::Strpos, vec![call(SqlFn::Lower, vec![s.clone()]), string_lit("e")]), int_lit(0)],
    );
    let fraction_free = call(
        SqlFn::And,
        vec![
            call(SqlFn::Equal, vec![x.clone(), call(SqlFn::FloorRaw, vec![x.clone()])]),
            call(SqlFn::Less, vec![call(SqlFn::Abs, vec![x.clone()]), float_lit(1e38)]),
        ],
    );
    let in_range = call(
        SqlFn::And,
        vec![
            call(SqlFn::GreaterEqual, vec![call(SqlFn::Abs, vec![x.clone()]), float_lit(1e-17)]),
            call(SqlFn::Less, vec![call(SqlFn::Abs, vec![x.clone()]), float_lit(9.007199254740992e15)]),
        ],
    );
    let int_plain = call(
        SqlFn::Concat,
        vec![
            cast(cast(only_where(fraction_free.clone(), x.clone()), SqlType::Scalar(Scalar::Hugeint)), varchar()),
            string_lit(".0"),
        ],
    );
    let plain = call(
        SqlFn::Rtrim,
        vec![
            cast(cast(only_where(in_range.clone(), x), SqlType::Decimal { precision: 38, scale: 18 }), varchar()),
            string_lit("0"),
        ],
    );
    let fixed = case(
        vec![when(
            call(SqlFn::EndsWith, vec![plain.clone(), string_lit(".")]),
            call(SqlFn::Concat, vec![plain.clone(), string_lit("0")]),
        )],
        Some(plain),
    );
    let bare = call(
        SqlFn::And,
        vec![
            call(SqlFn::Not, vec![has_exp.clone()]),
            call(SqlFn::Equal, vec![call(SqlFn::Strpos, vec![s.clone(), string_lit(".")]), int_lit(0)]),
        ],
    );
    let mut whens = non_finite(&s);
    whens.push(when(call(SqlFn::And, vec![has_exp.clone(), fraction_free]), int_plain));
    whens.push(when(call(SqlFn::And, vec![has_exp, in_range]), fixed));
    whens.push(when(bare, call(SqlFn::Concat, vec![s.clone(), string_lit(".0")])));
    case(whens, Some(s))
}

/// `x` where `guard` holds, else NULL.
fn only_where(guard: SqlExpr, x: SqlExpr) -> SqlExpr {
    case(vec![when(guard, x)], None)
}

/// The text of a double that is no number, in Pure's spelling.
fn non_finite(s: &SqlExpr) -> Vec<lite_sql::When> {
    let lower = call(SqlFn::Lower, vec![s.clone()]);
    vec![
        when(call(SqlFn::In, vec![lower.clone(), string_lit("nan"), string_lit("-nan")]), string_lit("NaN")),
        when(
            call(SqlFn::In, vec![lower.clone(), string_lit("inf"), string_lit("+inf"), string_lit("infinity")]),
            string_lit("Infinity"),
        ),
        when(call(SqlFn::In, vec![lower, string_lit("-inf"), string_lit("-infinity")]), string_lit("-Infinity")),
    ]
}
