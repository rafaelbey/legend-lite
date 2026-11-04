// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! Snowflake's spelled texts (Snowflake.java, `jaroWinkler` and "a Float's text" to `nonFinite`): a Float's shortest
//! digits that read back as the same double, computed in SQL over the double's exact decimal, and the three texts
//! made from them -- DuckDB's cast to text, Pure's literal spelling (`FLOAT_CANON`) and Pure's print form
//! (`FLOAT_PRINT`) -- and DuckDB's Jaro-Winkler similarity as three REDUCEs. Each is text Java builds when the piece is
//! made, its fresh names drawn then, the value written once.

use super::Snowflake;
use crate::pieces::{P, pc};
use crate::refusal::Refusal;
use crate::renderer::AnsiSqlRenderer;
use crate::writer::SqlWriter;
use lite_sql::SqlExpr;

/// `key` of the OBJECT `state`, cast to `type` (Java's `fieldOf`).
fn field_of(state: &str, key: &str, t: &str) -> String {
    format!("CAST(GET({state}, '{key}') AS {t})")
}

/// The OBJECT `state` with `key` set to `value` (Java's `withField`).
fn with_field(state: &str, key: &str, value: &str) -> String {
    format!("OBJECT_INSERT({state}, '{key}', {value}, TRUE)")
}

/// One step of [`Snowflake::shortest_digits`]: `body` over the OBJECT of `fields`, bound as `param` (Java's `step`).
fn step(param: &str, fields: &str, body: &str) -> String {
    format!("GET(TRANSFORM(ARRAY_CONSTRUCT(OBJECT_CONSTRUCT_KEEP_NULL({fields})), {param} OBJECT -> {body}), 0)")
}

/// The decimal exponent of the first digit of a positive DECFLOAT's text (Java's `exponentOf`).
fn exponent_of(text: &str) -> String {
    format!(
        "CASE WHEN CONTAINS({text}, 'e') THEN TO_NUMBER(SPLIT_PART({text}, 'e', 2)) + LENGTH(SPLIT_PART(SPLIT_PART(\
         {text}, 'e', 1), '.', 1)) - 1 WHEN SPLIT_PART({text}, '.', 1) <> '0' THEN LENGTH(SPLIT_PART({text}, '.', 1)) \
         - 1 ELSE -1 - LENGTH(REGEXP_SUBSTR(SPLIT_PART({text}, '.', 2), '^0*')) END"
    )
}

/// Whether `x` is NaN or an infinity (Java's `nonFinite`).
fn non_finite(x: &str) -> String {
    format!("{x} IN ('NaN'::FLOAT, 'inf'::FLOAT, '-inf'::FLOAT)")
}

/// Java's name of the non-finite double `x` (Java's `javaNonFinite`).
fn java_non_finite(x: &str) -> String {
    format!("CASE WHEN {x} = 'NaN'::FLOAT THEN 'NaN' WHEN {x} > 0 THEN 'Infinity' ELSE '-Infinity' END")
}

/// A Float's shortest digits as [`Snowflake::shortest_digits`] reads them (Java's `Digits`): the value, its sign, its
/// significant digits and the exponent of the first.
struct Digits {
    value: String,
    sign: String,
    digits: String,
    exponent: String,
}

impl Digits {
    /// The digits placed by their exponent, unsigned (Java's `unfolded`).
    fn unfolded(&self) -> String {
        let (e, d) = (&self.exponent, &self.digits);
        format!(
            "CASE WHEN {e} >= 0 THEN RPAD({d}, {e} + 1, '0') || '.' || IFF(LENGTH({d}) > {e} + 1, SUBSTR({d}, \
             GREATEST({e} + 2, 1)), '0') ELSE '0.' || REPEAT('0', GREATEST(-{e} - 1, 0)) || {d} END"
        )
    }
}

impl Snowflake {
    /// A Float's text as DuckDB's cast writes it (Java's `floatText`): the shortest digits, plain where the exponent
    /// is from -4 to 15, else with its exponent; NaN and the infinities Snowflake's own.
    pub(super) fn float_text(&self, w: &mut SqlWriter, v: &SqlExpr) -> Result<(), Refusal> {
        let p = self.shortest_digits(v, |d| {
            format!(
                "CASE WHEN {} THEN CAST({} AS VARCHAR) WHEN {} = 0 THEN '0.0' WHEN {} BETWEEN -4 AND 15 THEN {} || {} \
                 ELSE {} || LEFT({}, 1) || IFF(LENGTH({}) > 1, '.' || SUBSTR({}, 2), '') || 'e' || TO_VARCHAR({}) END",
                non_finite(&d.value),
                d.value,
                d.value,
                d.exponent,
                d.sign,
                d.unfolded(),
                d.sign,
                d.digits,
                d.digits,
                d.digits,
                d.exponent
            )
        });
        w.piece(&*p)?;
        Ok(())
    }

    /// Pure's literal spelling of a Float (Java's `floatCanon`): the digits placed by their exponent at every
    /// magnitude, every zero `0.0`; NaN and the infinities the database's text with `.0`.
    pub(super) fn float_canon(&self, w: &mut SqlWriter, v: &SqlExpr) -> Result<(), Refusal> {
        let p = self.shortest_digits(v, |d| {
            format!(
                "CASE WHEN {} THEN CAST({} AS VARCHAR) || '.0' WHEN {} = 0 THEN '0.0' ELSE {} || {} END",
                non_finite(&d.value),
                d.value,
                d.value,
                d.sign,
                d.unfolded()
            )
        });
        w.piece(&*p)?;
        Ok(())
    }

    /// Pure's print form of a Float (Java's `floatPrint`): the text where it is plain; past it, a whole value's exact
    /// digits under 10^38 with `.0`; a value from 10^-17 below 10^-4 at eighteen places trimmed; else the text with
    /// its exponent. NaN and the infinities are Java's names.
    pub(super) fn float_print(&self, w: &mut SqlWriter, v: &SqlExpr) -> Result<(), Refusal> {
        let p = self.shortest_digits(v, |d| {
            let x = &d.value;
            let fraction_free = format!("{x} = FLOOR({x}) AND ABS({x}) < 1e38");
            let in_range = format!("ABS({x}) >= 1e-17 AND ABS({x}) < 9.007199254740992e15");
            let fixed = format!("RTRIM(CAST(CAST(IFF({in_range}, {x}, NULL) AS NUMBER(38, 18)) AS VARCHAR), '0')");
            format!(
                "CASE WHEN {} THEN {} WHEN {x} = 0 THEN '0.0' WHEN {} BETWEEN -4 AND 15 THEN {} || {} WHEN \
                 {fraction_free} THEN CAST(CAST(IFF({fraction_free}, {x}, NULL) AS NUMBER(38, 0)) AS VARCHAR) || \
                 '.0' WHEN {in_range} THEN IFF(ENDSWITH({fixed}, '.'), {fixed} || '0', {fixed}) ELSE {} || LEFT({}, \
                 1) || IFF(LENGTH({}) > 1, '.' || SUBSTR({}, 2), '') || 'e' || TO_VARCHAR({}) END",
                non_finite(x),
                java_non_finite(x),
                d.exponent,
                d.sign,
                d.unfolded(),
                d.sign,
                d.digits,
                d.digits,
                d.digits,
                d.exponent
            )
        });
        w.piece(&*p)?;
        Ok(())
    }

    /// `result` over a Float's shortest digits, the value written once (Java's `shortestDigits`): each step a
    /// one-element TRANSFORM whose OBJECT carries what the next reads.
    fn shortest_digits<'a>(&'a self, v: &'a SqlExpr, result: impl Fn(&Digits) -> String) -> P<'a> {
        let z = self.delimited(&self.fresh_name("f"));
        let a = self.delimited(&self.fresh_name("f"));
        let b = self.delimited(&self.fresh_name("f"));
        let c = self.delimited(&self.fresh_name("f"));
        let d = self.delimited(&self.fresh_name("f"));
        let value = field_of(&z, "v", "DOUBLE");
        let stand = format!("IFF(ABS({value}) >= 'inf'::FLOAT OR {value} = 0, CAST(1 AS DOUBLE), {value})");
        let k = format!("(29 - FLOOR(LOG(2, ABS({stand}))))");
        let scaled = format!(
            "{stand} * POWER(CAST(2 AS DOUBLE), FLOOR({k} / 2)) * POWER(CAST(2 AS DOUBLE), {k} - FLOOR({k} / 2))"
        );
        let decimal = format!(
            "TO_VARCHAR(CAST(CAST({scaled} AS NUMBER(38, 28)) AS DECFLOAT) * POWER(CAST(2 AS DECFLOAT), -{k}))"
        );
        let exact = field_of(&a, "d", "VARCHAR");
        let exponent = format!(
            "CASE WHEN CAST({exact} AS DECFLOAT) = 0 THEN 0 ELSE {} END",
            exponent_of(&format!("LTRIM({exact}, '-')"))
        );
        let mantissa = format!(
            "TO_VARCHAR(CAST(CAST({exact} AS DECFLOAT) * CAST('1e' || TO_VARCHAR(-({exponent})) AS DECFLOAT) AS \
             NUMBER(38, 37)))"
        );
        let mut rounded = String::from("CASE");
        for places in 14..=15 {
            let round =
                format!("ROUND(CAST({} AS NUMBER(38, 37)), {places}, 'HALF_TO_EVEN')", field_of(&b, "m", "VARCHAR"));
            rounded.push_str(&format!(
                " WHEN TRY_TO_DOUBLE(TO_VARCHAR({round}) || 'e' || TO_VARCHAR({})) = TRY_TO_DOUBLE({}) THEN {round}",
                field_of(&b, "e", "INTEGER"),
                field_of(&b, "d", "VARCHAR")
            ));
        }
        rounded.push_str(&format!(
            " ELSE ROUND(CAST({} AS NUMBER(38, 37)), 16, 'HALF_TO_EVEN') END",
            field_of(&b, "m", "VARCHAR")
        ));
        let shortest = field_of(&c, "t", "VARCHAR");
        let last = step(
            &d,
            &format!(
                "'v', {}, 's', {}, 'g', RTRIM(REPLACE({shortest}, '.', ''), '0'), 'x', {} + LENGTH(SPLIT_PART(\
                 {shortest}, '.', 1)) - 1",
                field_of(&c, "v", "DOUBLE"),
                field_of(&c, "s", "VARCHAR"),
                field_of(&c, "e", "INTEGER")
            ),
            &result(&Digits {
                value: field_of(&d, "v", "DOUBLE"),
                sign: field_of(&d, "s", "VARCHAR"),
                digits: field_of(&d, "g", "VARCHAR"),
                exponent: field_of(&d, "x", "INTEGER"),
            }),
        );
        let second = step(
            &c,
            &format!(
                "'v', {}, 's', IFF(LEFT({}, 1) = '-', '-', ''), 'e', {}, 't', LTRIM(TO_VARCHAR({rounded}), '-')",
                field_of(&b, "v", "DOUBLE"),
                field_of(&b, "d", "VARCHAR"),
                field_of(&b, "e", "INTEGER")
            ),
            &last,
        );
        let first = step(
            &b,
            &format!("'v', {}, 'd', {exact}, 'e', {exponent}, 'm', {mantissa}", field_of(&a, "v", "DOUBLE")),
            &second,
        );
        let bound = step(&a, &format!("'v', {value}, 'd', {decimal}"), &first);
        pc(move |w| {
            w.append("CAST(GET(TRANSFORM(ARRAY_CONSTRUCT(OBJECT_CONSTRUCT_KEEP_NULL('v', ").expr(self, v, 0)?;
            w.append(&format!(")), {z} OBJECT -> {bound}), 0) AS VARCHAR)"));
            Ok(())
        })
    }

    /// Jaro-Winkler similarity, exactly DuckDB's, as three REDUCEs over OBJECT states (Java's `jaroWinkler`).
    pub(super) fn jaro_winkler<'a>(&'a self, p: &'a SqlExpr, t: &'a SqlExpr) -> P<'a> {
        let o = self.delimited(&self.fresh_name("o"));
        let s = self.delimited(&self.fresh_name("s"));
        let u = self.delimited(&self.fresh_name("u"));
        let v = self.delimited(&self.fresh_name("v"));
        let q = self.delimited(&self.fresh_name("q"));
        let j = self.delimited(&self.fresh_name("j"));
        let i = self.delimited(&self.fresh_name("i"));
        let op = field_of(&o, "p", "STRING");
        let ot = field_of(&o, "t", "STRING");
        // t's j-th character takes the first unmatched equal character of p in its window
        let b = field_of(&s, "b", "INTEGER");
        let take = with_field(
            &with_field(
                &with_field(&u, "k", "1"),
                "f",
                &format!("INSERT({}, {i}, 1, '1')", field_of(&u, "f", "STRING")),
            ),
            "tm",
            &format!("{} || {}", field_of(&u, "tm", "STRING"), field_of(&u, "c", "STRING")),
        );
        let window = format!(
            "REDUCE(ARRAY_GENERATE_RANGE(GREATEST(1, {j} - {b}), {j} + {b} + 1), {}, ({u} OBJECT, {i} INTEGER) -> \
             IFF({} = 0 AND SUBSTR({}, {i}, 1) = '0' AND SUBSTR({}, {i}, 1) = {}, {take}, {u}))",
            with_field(&with_field(&s, "c", &format!("SUBSTR({}, {j}, 1)", field_of(&s, "t", "STRING"))), "k", "0"),
            field_of(&u, "k", "INTEGER"),
            field_of(&u, "f", "STRING"),
            field_of(&u, "p", "STRING"),
            field_of(&u, "c", "STRING")
        );
        let matched = format!(
            "REDUCE(ARRAY_GENERATE_RANGE(1, LENGTH({ot}) + 1), OBJECT_CONSTRUCT_KEEP_NULL('p', {op}, 't', {ot}, 'b', \
             GREATEST(0, FLOOR(GREATEST(LENGTH({op}), LENGTH({ot})) / 2) - 1), 'f', REPEAT('0', LENGTH({op})), 'tm', \
             ''), ({s} OBJECT, {j} INTEGER) -> {window})"
        );
        // along p: 'n' matched characters so far, 'x' of them unequal to t's matched character in the same place
        let n = field_of(&v, "n", "INTEGER");
        let counted = format!(
            "REDUCE(ARRAY_GENERATE_RANGE(1, LENGTH({op}) + 1), {}, ({v} OBJECT, {i} INTEGER) -> IFF(SUBSTR({}, {i}, \
             1) = '1', {}, {v}))",
            with_field(&with_field(&matched, "n", "0"), "x", "0"),
            field_of(&v, "f", "STRING"),
            with_field(
                &with_field(&v, "n", &format!("{n} + 1")),
                "x",
                &format!(
                    "{} + IFF(SUBSTR({}, {i}, 1) = SUBSTR({}, {n} + 1, 1), 0, 1)",
                    field_of(&v, "x", "INTEGER"),
                    field_of(&v, "p", "STRING"),
                    field_of(&v, "tm", "STRING")
                )
            )
        );
        let qp = field_of(&q, "p", "STRING");
        let qt = field_of(&q, "t", "STRING");
        let m = format!("CAST({} AS DOUBLE)", field_of(&q, "n", "INTEGER"));
        let jaro = format!(
            "(({m} / CAST(LENGTH({qp}) AS DOUBLE) + {m} / CAST(LENGTH({qt}) AS DOUBLE) + ({m} - CAST(FLOOR({} / 2) AS \
             DOUBLE)) / {m}) / CAST(3 AS DOUBLE))",
            field_of(&q, "x", "INTEGER")
        );
        let prefix: Vec<String> = (1..=4)
            .map(|k| {
                format!("IFF({k} <= LEAST(LENGTH({qp}), LENGTH({qt})) AND LEFT({qp}, {k}) = LEFT({qt}, {k}), 1, 0)")
            })
            .collect();
        let score = format!(
            "IFF({qp} IS NULL OR {qt} IS NULL, NULL, IFF({} = 0, CAST(0 AS DOUBLE), IFF({jaro} > CAST(0.7 AS DOUBLE), \
             {jaro} + CAST({} AS DOUBLE) * CAST(0.1 AS DOUBLE) * (CAST(1 AS DOUBLE) - {jaro}), {jaro})))",
            field_of(&q, "n", "INTEGER"),
            prefix.join(" + ")
        );
        let body = format!("GET(TRANSFORM(ARRAY_CONSTRUCT({counted}), {q} OBJECT -> {score}), 0)");
        pc(move |w| {
            w.append("CAST(GET(TRANSFORM(ARRAY_CONSTRUCT(OBJECT_CONSTRUCT_KEEP_NULL('p', ").expr(self, p, 0)?;
            w.append(", 't', ").expr(self, t, 0)?.append(&format!(")), {o} OBJECT -> {body}), 0) AS DOUBLE)"));
            Ok(())
        })
    }
}
