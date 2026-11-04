// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! How MariaDB and MySQL hold a list, a struct, a map and a Variant -- JSON text (Java's `json`, `jsonOf`,
//! `columnType`, `valueAt`, `scalarOfText`) -- and what reads them: the list operations as correlated subqueries over
//! `JSON_TABLE` (`elements`, `overElements`, `overRows`), the struct literal's and field's paths, the Variant's
//! construction and reads, the casts and texts of a value (`variantAwareCast`, `castTo`, `asText`, `textOf`, the
//! double's text laid out as DuckDB's), and the recursive walks: a fold, a list reduction, an edit distance, a
//! Jaro-Winkler similarity, a split (MariaDb.java, "JSON" to "Variant" and the list helpers).

use super::{
    MariaDb, P, closed_alone, fact_type, index_of, is_json, is_text, lambda_body, list_element, nested, offset_of, pc,
    text, type_of,
};
use crate::build;
use crate::pieces::Ex;
use crate::refusal::{Refusal, arg};
use crate::renderer::AnsiSqlRenderer;
use crate::writer::SqlWriter;
use lite_sql::{AggFn, Lambda, Scalar, SortKey, SqlExpr, SqlFn, SqlType};

impl MariaDb {
    // ---- how a JSON container holds a value

    /// Java's `json(x)`: a value as a JSON container holds it.
    pub(super) fn json<'a>(&'a self, x: &'a SqlExpr) -> P<'a> {
        let mut t = type_of(x);
        if t.is_none() && matches!(x, SqlExpr::ArrayLit { .. } | SqlExpr::StructLit { .. }) {
            return self.compact(self.ex(x, 0));
        }
        let boolean = SqlType::Scalar(Scalar::Boolean);
        if t.is_none() && matches!(x, SqlExpr::BoolLit { .. }) {
            t = Some(&boolean);
        }
        self.json_of(self.ex(x, 0), t)
    }

    /// Java's `jsonOf(v, t)`: a boolean as JSON true or false, a list, struct, map or Variant as its JSON, anything
    /// else as it is.
    pub(super) fn json_of<'a>(&self, v: P<'a>, t: Option<&SqlType>) -> P<'a> {
        if t == Some(&SqlType::Scalar(Scalar::Boolean)) {
            return pc(move |w| {
                w.append("JSON_EXTRACT(CASE WHEN ").piece(&*v)?.append(" THEN 'true' WHEN NOT (").piece(&*v)?;
                w.append(") THEN 'false' END, '$')");
                Ok(())
            });
        }
        if nested(t) {
            return self.compact(v);
        }
        v
    }

    /// Java's `columnType(t)`: the `JSON_TABLE` column type an element of type `t` is read as.
    pub(super) fn column_type(&self, t: Option<&SqlType>) -> String {
        const TEXT: &str = "LONGTEXT CHARACTER SET utf8mb4 COLLATE utf8mb4_bin";
        let s = match t {
            Some(SqlType::Decimal { precision, scale }) => {
                return format!("DECIMAL({},{})", (*precision).min(65), (*scale).min(38));
            }
            Some(SqlType::Scalar(s)) => *s,
            other => return if nested(other) { "JSON" } else { TEXT }.to_owned(),
        };
        match s {
            Scalar::Boolean => self.boolean_column(),
            Scalar::Integer | Scalar::Bigint => "BIGINT",
            Scalar::Hugeint => "DECIMAL(38,0)",
            Scalar::Double => "DOUBLE",
            Scalar::Date => "DATE",
            Scalar::Timestamp | Scalar::Timestamptz => "DATETIME(6)",
            Scalar::Time => "TIME(6)",
            Scalar::Json => "JSON",
            Scalar::Varchar | Scalar::TemporalText | Scalar::DecimalText => TEXT,
        }
        .to_owned()
    }

    /// Java's `valueAt(json, path, t)`: a JSON text's value at `path` as a value of type `t`.
    pub(super) fn value_at<'a>(&self, json: P<'a>, path: P<'a>, t: Option<&SqlType>) -> P<'a> {
        if matches!(t, Some(SqlType::Array { .. } | SqlType::Struct { .. } | SqlType::Map { .. })) {
            return self.json_member(json, path);
        }
        if nested(t) {
            return pc(move |w| {
                w.append("JSON_EXTRACT(").piece(&*json)?.append(", ").piece(&*path)?.append(")");
                Ok(())
            });
        }
        let text = self.json_text(json, path);
        self.scalar_of_text(text, t)
    }

    /// Java's `scalarOfText(text, t)`: a JSON scalar's text as a value of type `t`.
    pub(super) fn scalar_of_text<'a>(&self, text: P<'a>, t: Option<&SqlType>) -> P<'a> {
        if t == Some(&SqlType::Scalar(Scalar::Boolean)) {
            return pc(move |w| {
                w.append("(CASE ").piece(&*text)?;
                w.append(" WHEN '1' THEN TRUE WHEN 'true' THEN TRUE WHEN '0' THEN FALSE WHEN 'false' THEN FALSE END)");
                Ok(())
            });
        }
        let r#type = match t {
            Some(d @ SqlType::Decimal { .. }) => Some(self.column_type(Some(d))),
            Some(SqlType::Scalar(s)) if !matches!(s, Scalar::Varchar | Scalar::TemporalText | Scalar::DecimalText) => {
                Some(
                    match s {
                        Scalar::Integer | Scalar::Bigint => "SIGNED",
                        Scalar::Hugeint => "DECIMAL(38,0)",
                        Scalar::Double => "DOUBLE",
                        Scalar::Date => "DATE",
                        Scalar::Timestamp | Scalar::Timestamptz => "DATETIME(6)",
                        Scalar::Time => "TIME(6)",
                        Scalar::Boolean
                        | Scalar::Json
                        | Scalar::Varchar
                        | Scalar::TemporalText
                        | Scalar::DecimalText => "CHAR",
                    }
                    .to_owned(),
                )
            }
            _ => None,
        };
        match r#type {
            Some(r#type) => pc(move |w| {
                w.append("CAST(").piece(&*text)?.append(&format!(" AS {type})"));
                Ok(())
            }),
            None => text,
        }
    }

    /// Java's `elementAt(xs, index)`: the element of `xs` at a 0-based index (NULL past either end).
    pub(super) fn element_at<'a>(&'a self, xs: &'a SqlExpr, index: P<'a>) -> P<'a> {
        self.value_at(self.ex(xs, 0), self.index_path(index), list_element(xs))
    }

    // ---- lists: each operation a correlated subquery over JSON_TABLE(xs)

    /// Java's `param(name)`: a lambda parameter (or an element's or index's name) as its references spell it.
    pub(super) fn param(&self, name: &str) -> String {
        let c = lite_sql::Column {
            table: None,
            name: name.to_owned(),
            r#type: lite_sql::TypeFact::Unknown,
            origin: Some(lite_sql::Origin::Derived),
        };
        self.column_ref(&c).expect("an unqualified column's reference")
    }

    /// Java's `elements(xs, e, o, i)`: `JSON_TABLE(xs, '$[*]' COLUMNS (o FOR ORDINALITY [, i FOR ORDINALITY], e <type>
    /// PATH '$')) AS e__t`.
    pub(super) fn elements<'a>(&'a self, xs: &'a SqlExpr, e: &str, o: &str, i: Option<&str>) -> P<'a> {
        let r#type = self.column_type(list_element(xs));
        let index = i.map_or_else(String::new, |i| format!(", {} FOR ORDINALITY", self.param(i)));
        let spec =
            format!("'$[*]' COLUMNS ({} FOR ORDINALITY{index}, {} {type} PATH '$')", self.param(o), self.param(e));
        self.json_table(self.ex(xs, 0), spec, self.param(&format!("{e}__t")))
    }

    /// Java's `unlessNull(xs, body)`: `body`, or NULL for a NULL list.
    pub(super) fn unless_null<'a>(&'a self, xs: &'a SqlExpr, body: P<'a>) -> P<'a> {
        pc(move |w| {
            w.append("(CASE WHEN ").expr(self, xs, 4)?.append(" IS NULL THEN NULL ELSE ").piece(&*body)?;
            w.append(" END)");
            Ok(())
        })
    }

    /// Java's `lambdaOf(e, fn)`: a lambda with a parameter, else refused by name.
    pub(super) fn lambda_of<'e>(&self, e: &'e SqlExpr, f: SqlFn) -> Result<&'e Lambda, Refusal> {
        match e {
            SqlExpr::Lambda(l) if !l.params.is_empty() => Ok(l),
            _ => Err(Refusal::capability(format!("{f} without a lambda reached {}", self.product()))),
        }
    }

    /// Java's `overElements`: `(SELECT JSON_ARRAYAGG(select ORDER BY o) FROM elements [WHERE where])`, the empty
    /// array over no rows, NULL for a NULL list.
    pub(super) fn over_elements<'a>(
        &'a self,
        xs: &'a SqlExpr,
        e: String,
        o: String,
        i: Option<String>,
        select: P<'a>,
        r#where: Option<P<'a>>,
    ) -> P<'a> {
        let order = text(self.param(&o));
        let agg = self.array_agg(false, select, Some(order), false);
        self.unless_null(
            xs,
            pc(move |w| {
                w.append("coalesce((SELECT ").piece(&*agg)?.append(" FROM ");
                let elements = self.elements(xs, &e, &o, i.as_deref());
                w.piece(&*elements)?;
                if let Some(wh) = &r#where {
                    w.append(" WHERE ").piece(&**wh)?;
                }
                w.append("), JSON_ARRAY())");
                Ok(())
            }),
        )
    }

    /// Java's `overRows`: `(SELECT agg(e) FROM elements)`, a list reduced as rows; MODE its most frequent non-NULL
    /// element, the first to occur among ties.
    pub(super) fn over_rows<'a>(&'a self, xs: &'a SqlExpr, f: AggFn, extras: &[SqlExpr]) -> Result<P<'a>, Refusal> {
        let e = self.param("__e");
        let o = self.param("__o");
        if f == AggFn::Mode {
            if !extras.is_empty() {
                return Err(Refusal::capability(format!("a mode with arguments reached {}", self.product())));
            }
            return Ok(pc(move |w| {
                w.append(&format!("(SELECT {e} FROM "));
                let elements = self.elements(xs, "__e", "__o", None);
                w.piece(&*elements)?;
                w.append(&format!(" WHERE {e} IS NOT NULL GROUP BY {e} ORDER BY count(*) DESC, min({o}) LIMIT 1)"));
                Ok(())
            }));
        }
        let el = if list_element(xs).is_none() { build::derived(None, "__e") } else { column_param("__e", xs) };
        let mut args = vec![el];
        args.extend(extras.iter().cloned());
        let order: Vec<SortKey> = if matches!(f, AggFn::StringAgg | AggFn::List) {
            vec![build::sort_key(build::derived(None, "__o"), true, None, None)]
        } else {
            Vec::new()
        };
        let r = build::reducer(f, args, false, order);
        Ok(pc(move |w| {
            w.append("(SELECT ");
            self.reducer(w, &r)?;
            w.append(" FROM ");
            let elements = self.elements(xs, "__e", "__o", None);
            w.piece(&*elements)?.append(")");
            Ok(())
        }))
    }

    /// Java's `elementReduction`: a reduction whose value is one of the list's elements, cast to the call's own type
    /// over a list whose elements claim no type.
    pub(super) fn element_reduction<'a>(
        &'a self,
        a: &'a [SqlExpr],
        call_type: Option<&SqlType>,
        f: AggFn,
    ) -> Result<P<'a>, Refusal> {
        let xs = arg(a, 0)?;
        let r = self.over_rows(xs, f, &[])?;
        let r#type = if list_element(xs).is_none() { cast_target(self, call_type) } else { None };
        Ok(match r#type {
            None => r,
            Some(t) => pc(move |w| {
                w.append("CAST(").piece(&*r)?.append(&format!(" AS {t})"));
                Ok(())
            }),
        })
    }

    /// Java's `memberPath(name)`: a JSON path to a member by name, `$."name"`.
    pub(super) fn member_path(&self, name: &str) -> String {
        self.string_literal(&format!("$.\"{}\"", name.replace('\\', "\\\\").replace('"', "\\\"")))
    }

    /// Java's `structInsert`: DuckDB's struct_insert, the struct with the field set (a new one last).
    pub(super) fn struct_insert_arm(&self, w: &mut SqlWriter, args: &[SqlExpr]) -> Result<(), Refusal> {
        let SqlExpr::StringLit { value: name, .. } = arg(args, 1)? else {
            return Err(Refusal::capability(format!(
                "struct_insert of a field that is not a literal reached {}",
                self.product()
            )));
        };
        w.append("JSON_SET(").expr(self, arg(args, 0)?, 0)?.append(", ").append(&self.member_path(name)).append(", ");
        let json = self.json(arg(args, 2)?);
        w.piece(&*json)?.append(")");
        Ok(())
    }

    /// Java's `listPredicate`: exists or forAll over the elements, the empty list's answer when none.
    pub(super) fn list_predicate(
        &self,
        w: &mut SqlWriter,
        args: &[SqlExpr],
        agg: &str,
        empty: &str,
        f: SqlFn,
    ) -> Result<(), Refusal> {
        let l = self.lambda_of(arg(args, 1)?, f)?;
        let xs = arg(args, 0)?;
        let body = lambda_body(l, list_element(xs))?;
        w.append(&format!("coalesce((SELECT {agg}(CASE WHEN ")).expr(self, &body, 0)?;
        w.append(" THEN 1 WHEN NOT (").expr(self, &body, 0)?.append(") THEN 0 END) FROM ");
        let elements = self.elements(xs, &l.params[0], &offset_of(l), index_of(l));
        w.piece(&*elements)?.append(&format!("), {empty})"));
        Ok(())
    }

    /// Java's `allDistinct`: no duplicates iff the distinct count is the count; an empty or NULL list is distinct.
    pub(super) fn all_distinct_arm(&self, w: &mut SqlWriter, args: &[SqlExpr]) -> Result<(), Refusal> {
        let e = self.param("__e");
        w.append(&format!("coalesce((SELECT count(*) = count(DISTINCT {e}) FROM "));
        let elements = self.elements(arg(args, 0)?, "__e", "__o", None);
        w.piece(&*elements)?.append("), TRUE)");
        Ok(())
    }

    /// Java's `membership`: DuckDB's list_contains -- NULL for a NULL list or needle, else found or not; a nested value
    /// compares as its JSON text.
    pub(super) fn membership_arm(
        &self,
        w: &mut SqlWriter,
        needle: &SqlExpr,
        collection: &SqlExpr,
    ) -> Result<(), Refusal> {
        let el = list_element(collection);
        let e = self.param("__e");
        let needle_piece = if nested(el) { self.compact(self.ex(needle, 0)) } else { self.ex(needle, 5) };
        let element = if nested(el) { self.compact(text(e.clone())) } else { text(e) };
        w.append("(CASE WHEN ").expr(self, collection, 4)?.append(" IS NULL OR ").expr(self, needle, 4)?;
        w.append(" IS NULL THEN NULL ELSE EXISTS(SELECT 1 FROM ");
        let elements = self.elements(collection, "__e", "__eo", None);
        w.piece(&*elements)?.append(" WHERE ").piece(&*element)?.append(" = ").piece(&*needle_piece)?;
        w.append(") END)");
        Ok(())
    }

    // ---- folds and walks: recursive CTEs over values that read no row

    /// Java's `foldCall`: Pure's fold as a `WITH RECURSIVE` over the elements in order.
    pub(super) fn fold_arm(&self, w: &mut SqlWriter, fold: &SqlExpr) -> Result<(), Refusal> {
        let SqlExpr::FoldCall { source, lambda, init, r#type, .. } = fold else { unreachable!("a fold call") };
        if !self.correlates_derived()
            && (!closed_alone(source) || !closed_alone(init) || !closed_alone(&SqlExpr::Lambda(lambda.clone())))
        {
            return Err(Refusal::capability(format!(
                "a fold that reads a row reached {}, whose recursive CTE cannot read an outer column (measured: \
                 Unknown column)",
                self.product()
            )));
        }
        let Some(acc_type) = fact_type(r#type).or_else(|| type_of(init)) else {
            return Err(Refusal::capability(format!(
                "a fold whose accumulator claims no type reached {}, whose recursive CTE types its column by its \
                 first row",
                self.product()
            )));
        };
        let body = lambda_body(lambda, list_element(source))?;
        let init_json = self.json(init);
        let array = pc(move |v| {
            v.append("JSON_ARRAY(").piece(&*init_json)?.append(")");
            Ok(())
        });
        let table = self.json_table(
            array,
            format!("'$[*]' COLUMNS (\"a\" {} PATH '$')", self.column_type(Some(acc_type))),
            "\"__a\"".to_owned(),
        );
        let anchor = pc(move |w| {
            w.append("SELECT 0, \"__a\".\"a\" FROM ").piece(&*table)?;
            Ok(())
        });
        let x = arg_param(&lambda.params, 0)?;
        let acc = arg_param(&lambda.params, 1)?;
        let walk = self.accumulation(source, x.to_owned(), acc.to_owned(), anchor, body);
        w.piece(&*walk)?;
        Ok(())
    }

    /// Java's `listReduce`: DuckDB's list_reduce `(acc, x)`, the first element the accumulator's start.
    pub(super) fn list_reduce<'a>(
        &'a self,
        a: &'a [SqlExpr],
        call_type: Option<&'a SqlType>,
    ) -> Result<P<'a>, Refusal> {
        let l = match arg(a, 1)? {
            SqlExpr::Lambda(l) if l.params.len() == 2 => l,
            _ => return Err(self.wall(SqlFn::ListReduce, "a reduction lambda of another shape")),
        };
        let xs = arg(a, 0)?;
        if !self.correlates_derived() && (!closed_alone(xs) || !closed_alone(&SqlExpr::Lambda(l.clone()))) {
            return Err(self.wall(
                SqlFn::ListReduce,
                "a reduction that reads a row needs a recursive CTE over an outer column (measured: Unknown column)",
            ));
        }
        let el = list_element(xs);
        let Some(acc_type) = call_type.or(el) else {
            return Err(self.wall(SqlFn::ListReduce, "a reduction whose accumulator claims no type"));
        };
        let acc = &l.params[0];
        let x = &l.params[1];
        let swapped = build::lambda_of(vec![x.clone(), acc.clone()], l.body.as_ref().clone());
        let body = lambda_body(&swapped, el)?;
        let table = self.json_table(
            self.ex(xs, 0),
            format!("'$[0]' COLUMNS (\"a\" {} PATH '$')", self.column_type(Some(acc_type))),
            "\"__a\"".to_owned(),
        );
        let anchor = pc(move |w| {
            w.append("SELECT 1, \"__a\".\"a\" FROM ").piece(&*table)?;
            Ok(())
        });
        Ok(self.accumulation(xs, x.clone(), acc.clone(), anchor, body))
    }

    /// Java's `accumulation`: `(WITH RECURSIVE __f(__i, acc) AS (<anchor> UNION ALL <step>) <last acc>)`, the step
    /// joining the element at position `__i + 1` and applying `body`.
    fn accumulation<'a>(&'a self, xs: &'a SqlExpr, x: String, acc: String, anchor: P<'a>, body: SqlExpr) -> P<'a> {
        let spec = format!(
            "'$[*]' COLUMNS (\"__k\" FOR ORDINALITY, {} {} PATH '$')",
            self.param(&x),
            self.column_type(list_element(xs))
        );
        let acc_param = self.param(&acc);
        let table = self.json_table(self.ex(xs, 0), spec, "\"__x\"".to_owned());
        pc(move |w| {
            w.append("(WITH RECURSIVE \"__f\"(\"__i\", ").append(&acc_param).append(") AS (").piece(&*anchor)?;
            w.append(" UNION ALL SELECT \"__f\".\"__i\" + 1, ").expr(self, &body, 0)?.append(" FROM \"__f\" JOIN ");
            w.piece(&*table)?.append(" ON \"__x\".\"__k\" = \"__f\".\"__i\" + 1) SELECT ").append(&acc_param);
            w.append(" FROM \"__f\" ORDER BY \"__i\" DESC LIMIT 1)");
            Ok(())
        })
    }

    /// Java's `splitPartCall`: Pure's splitPart -- the non-empty tokens, 1-based, NULL past the end.
    pub(super) fn split_part_arm(&self, w: &mut SqlWriter, a: &[SqlExpr]) -> Result<(), Refusal> {
        let parts = self.split(arg(a, 0)?, arg(a, 1)?);
        let cols = format!(
            "'$[*]' COLUMNS (\"__o\" FOR ORDINALITY, \"__v\" {} PATH '$')",
            self.column_type(Some(&SqlType::Scalar(Scalar::Varchar)))
        );
        let n = arg(a, 2)?;
        w.append("(CASE WHEN ").expr(self, n, 4)?.append(" > 0 THEN (SELECT __t.\"__v\" FROM ");
        let t = self.json_table(parts.clone(), cols.clone(), "__t".to_owned());
        w.piece(&*t)?.append(" WHERE __t.\"__v\" <> '' AND (SELECT count(*) FROM ");
        let u = self.json_table(parts, cols, "__u".to_owned());
        w.piece(&*u)?.append(" WHERE __u.\"__v\" <> '' AND __u.\"__o\" < __t.\"__o\") = ").expr(self, n, 6)?;
        w.append(" - 1) END)");
        Ok(())
    }

    /// Java's `levenshtein`: the edit distance of two texts by characters, a recursive walk over the cells.
    pub(super) fn levenshtein<'a>(&'a self, x: &'a SqlExpr, y: &'a SqlExpr) -> Result<P<'a>, Refusal> {
        if !self.correlates_derived() && (!closed_alone(x) || !closed_alone(y)) {
            return Err(self.wall(
                SqlFn::Levenshtein,
                "an edit distance of texts that read a row needs a recursive CTE over an outer column (measured: \
                 Unknown column)",
            ));
        }
        let at = self.json_text_of("__lv.\"p\"", "CONCAT('$[', __lv.\"j\" + 1, ']')");
        let cell = format!(
            "LEAST({at} + 1, {} + 1, {} + (SUBSTRING(__lv.\"a\", __lv.\"i\", 1) <> SUBSTRING(__lv.\"b\", __lv.\"j\" + \
             1, 1)))",
            self.json_text_of("__lv.\"c\"", "CONCAT('$[', __lv.\"j\", ']')"),
            self.json_text_of("__lv.\"p\"", "CONCAT('$[', __lv.\"j\", ']')")
        );
        let more = "__lv.\"j\" < CHAR_LENGTH(__lv.\"b\")";
        let text_type = self.column_type(Some(&SqlType::Scalar(Scalar::Varchar)));
        let last = self.json_text_of("\"c\"", "CONCAT('$[', \"j\", ']')");
        let tail = format!(
            ", JSON_ARRAY(), JSON_ARRAY(0)), '$' COLUMNS (\"a\" {text_type} PATH '$[0]', \"b\" {text_type} PATH \
             '$[1]', \"p\" JSON PATH '$[2]', \"c\" JSON PATH '$[3]')) AS __s UNION ALL SELECT __lv.\"a\", \
             __lv.\"b\", CASE WHEN {more} THEN __lv.\"i\" ELSE __lv.\"i\" + 1 END, CASE WHEN {more} THEN \
             __lv.\"j\" + 1 ELSE 0 END, CASE WHEN {more} THEN __lv.\"p\" ELSE __lv.\"c\" END, CASE WHEN {more} \
             THEN JSON_ARRAY_APPEND(__lv.\"c\", '$', CASE WHEN __lv.\"i\" = 0 THEN __lv.\"j\" + 1 ELSE {cell} END) \
             ELSE JSON_ARRAY(__lv.\"i\" + 1) END FROM __lv WHERE NOT (__lv.\"j\" = CHAR_LENGTH(__lv.\"b\") AND \
             __lv.\"i\" = CHAR_LENGTH(__lv.\"a\"))) SELECT CAST({last} AS SIGNED) FROM __lv WHERE \"i\" = \
             CHAR_LENGTH(\"a\") AND \"j\" = CHAR_LENGTH(\"b\"))"
        );
        Ok(pc(move |w| {
            w.append(
                "(WITH RECURSIVE __lv(\"a\", \"b\", \"i\", \"j\", \"p\", \"c\") AS (SELECT __s.\"a\", __s.\"b\", 0, \
                 0, __s.\"p\", __s.\"c\" FROM JSON_TABLE(JSON_ARRAY(",
            );
            w.expr(self, x, 0)?.append(", ").expr(self, y, 0)?.append(&tail);
            Ok(())
        }))
    }

    /// Java's `jaroWinkler`: DuckDB's jaro_winkler_similarity, two recursive walks.
    pub(super) fn jaro_winkler<'a>(&'a self, x: &'a SqlExpr, y: &'a SqlExpr) -> Result<P<'a>, Refusal> {
        if !self.correlates_derived() && (!closed_alone(x) || !closed_alone(y)) {
            return Err(self.wall(
                SqlFn::JaroWinkler,
                "a Jaro-Winkler similarity of texts that read a row needs a recursive CTE over an outer column \
                 (measured: Unknown column)",
            ));
        }
        let text_type = self.column_type(Some(&SqlType::Scalar(Scalar::Varchar)));
        let hit = "SUBSTRING(__jw.\"f\", __jw.\"i\", 1) = '0' AND SUBSTRING(__jw.\"p\", __jw.\"i\", 1) = \
                   SUBSTRING(__jw.\"t\", __jw.\"j\", 1)";
        let next = format!("__jw.\"i\" > __jw.\"j\" + __jw.\"b\" OR __jw.\"i\" > CHAR_LENGTH(__jw.\"p\") OR ({hit})");
        let taken = format!("__jw.\"i\" <= __jw.\"j\" + __jw.\"b\" AND {hit}");
        let m = "CAST(CHAR_LENGTH(__jx.\"m\") AS DOUBLE)";
        let jaro = format!(
            "({m} / CAST(CHAR_LENGTH(__jx.\"p\") AS DOUBLE) + {m} / CAST(CHAR_LENGTH(__jx.\"t\") AS DOUBLE) + ({m} - \
             CAST(FLOOR(__jx.\"n\" / 2) AS DOUBLE)) / {m}) / CAST(3 AS DOUBLE)"
        );
        let mut prefix = String::new();
        for k in 1..=4 {
            prefix.push_str(if k == 1 { "" } else { " + " });
            prefix.push_str(&format!(
                "CASE WHEN {k} <= LEAST(CHAR_LENGTH(__jx.\"p\"), CHAR_LENGTH(__jx.\"t\")) AND LEFT(__jx.\"p\", {k}) = \
                 LEFT(__jx.\"t\", {k}) THEN 1 ELSE 0 END"
            ));
        }
        let window = "GREATEST(0, FLOOR(GREATEST(CHAR_LENGTH(__s.\"p\"), CHAR_LENGTH(__s.\"t\")) / 2) - 1)";
        let head = format!(
            "(WITH RECURSIVE __jw(\"p\", \"t\", \"b\", \"j\", \"i\", \"f\", \"m\") AS (SELECT __s.\"p\", __s.\"t\", \
             {window}, 1, GREATEST(1, 1 - {window}), CONCAT(__s.\"m\", REPEAT('0', CHAR_LENGTH(__s.\"p\"))), \
             __s.\"m\" FROM JSON_TABLE(JSON_ARRAY("
        );
        let tail = format!(
            ", ''), '$' COLUMNS (\"p\" {text_type} PATH '$[0]', \"t\" {text_type} PATH '$[1]', \"m\" {text_type} PATH \
             '$[2]')) AS __s UNION ALL SELECT __jw.\"p\", __jw.\"t\", __jw.\"b\", CASE WHEN {next} THEN __jw.\"j\" + 1 \
             ELSE __jw.\"j\" END, CASE WHEN {next} THEN GREATEST(1, __jw.\"j\" + 1 - __jw.\"b\") ELSE __jw.\"i\" + 1 \
             END, CASE WHEN {taken} THEN INSERT(__jw.\"f\", __jw.\"i\", 1, '1') ELSE __jw.\"f\" END, CASE WHEN \
             {taken} THEN CONCAT(__jw.\"m\", SUBSTRING(__jw.\"t\", __jw.\"j\", 1)) ELSE __jw.\"m\" END FROM __jw \
             WHERE __jw.\"j\" <= CHAR_LENGTH(__jw.\"t\")), __jx(\"p\", \"t\", \"f\", \"m\", \"k\", \"q\", \"n\") AS \
             (SELECT __jw.\"p\", __jw.\"t\", __jw.\"f\", __jw.\"m\", 1, 0, 0 FROM __jw WHERE __jw.\"j\" > \
             CHAR_LENGTH(__jw.\"t\") UNION ALL SELECT __jx.\"p\", __jx.\"t\", __jx.\"f\", __jx.\"m\", __jx.\"k\" + \
             1, CASE WHEN SUBSTRING(__jx.\"f\", __jx.\"k\", 1) = '1' THEN __jx.\"q\" + 1 ELSE __jx.\"q\" END, CASE \
             WHEN SUBSTRING(__jx.\"f\", __jx.\"k\", 1) = '1' AND SUBSTRING(__jx.\"p\", __jx.\"k\", 1) <> \
             SUBSTRING(__jx.\"m\", __jx.\"q\" + 1, 1) THEN __jx.\"n\" + 1 ELSE __jx.\"n\" END FROM __jx WHERE \
             __jx.\"k\" <= CHAR_LENGTH(__jx.\"p\")) SELECT CASE WHEN CHAR_LENGTH(__jx.\"m\") = 0 THEN CAST(0 AS \
             DOUBLE) WHEN {jaro} > CAST(0.7 AS DOUBLE) THEN {jaro} + ({prefix}) * CAST(0.1 AS DOUBLE) * (CAST(1 AS \
             DOUBLE) - {jaro}) ELSE {jaro} END FROM __jx WHERE __jx.\"k\" > CHAR_LENGTH(__jx.\"p\"))"
        );
        Ok(pc(move |w| {
            w.append(&head).expr(self, x, 0)?.append(", ").expr(self, y, 0)?.append(&tail);
            Ok(())
        }))
    }

    /// Java's `rows(n, alias)`: `JSON_TABLE` over `n` rows, its ordinal `alias."__n"`.
    pub(super) fn rows<'a>(&self, n: P<'a>, alias: &str) -> P<'a> {
        let json = pc(move |w| {
            w.append("CONCAT('[', REPEAT('0,', (").piece(&*n)?.append(") - 1), '0]')");
            Ok(())
        });
        self.json_table(json, "'$[*]' COLUMNS (\"__n\" FOR ORDINALITY)".to_owned(), alias.to_owned())
    }

    /// Java's `split(s, d)`: DuckDB's string_split as a JSON array of the parts.
    pub(super) fn split<'a>(&'a self, s: &'a SqlExpr, d: &'a SqlExpr) -> P<'a> {
        pc(move |w| {
            w.append("(CASE WHEN ").expr(self, s, 4)?.append(" IS NULL OR ").expr(self, d, 4)?;
            w.append(" IS NULL THEN NULL WHEN CHAR_LENGTH(").expr(self, s, 0)?;
            w.append(") = 0 THEN JSON_ARRAY() WHEN CHAR_LENGTH(").expr(self, d, 0)?.append(") = 0 THEN (SELECT ");
            let chars = pc(move |v| {
                v.append("SUBSTRING(").expr(self, s, 0)?.append(", __g.\"__n\", 1)");
                Ok(())
            });
            let agg = self.array_agg(false, chars, Some(text("__g.\"__n\"")), false);
            w.piece(&*agg)?.append(" FROM ");
            let length = pc(move |v| {
                v.append("CHAR_LENGTH(").expr(self, s, 0)?.append(")");
                Ok(())
            });
            let rows = self.rows(length, "__g");
            w.piece(&*rows)?.append(") ELSE ");
            let parts = pc(move |v| {
                v.append("CONCAT('[', REPLACE(JSON_QUOTE(").expr(self, s, 0)?.append("), SUBSTRING(JSON_QUOTE(");
                v.expr(self, d, 0)?.append("), 2, CHAR_LENGTH(JSON_QUOTE(").expr(self, d, 0)?;
                v.append(")) - 2), '\",\"'), ']')");
                Ok(())
            });
            let compact = self.compact(parts);
            w.piece(&*compact)?.append(" END)");
            Ok(())
        })
    }

    // ---- Variant: JSON text

    /// Java's `variantOf(x)`: a value as a Variant's JSON text.
    pub(super) fn variant_of<'a>(&'a self, x: &'a SqlExpr) -> P<'a> {
        let t = type_of(x);
        if t == Some(&SqlType::Scalar(Scalar::Json)) {
            return self.ex(x, 0);
        }
        if nested(t) || matches!(x, SqlExpr::ArrayLit { .. } | SqlExpr::StructLit { .. }) {
            return self.compact(self.ex(x, 0));
        }
        if t == Some(&SqlType::Scalar(Scalar::Boolean)) || matches!(x, SqlExpr::BoolLit { .. }) {
            return self.json_of(self.ex(x, 0), Some(&SqlType::Scalar(Scalar::Boolean)));
        }
        pc(move |w| {
            w.append("JSON_EXTRACT(JSON_ARRAY(").expr(self, x, 0)?.append("), '$[0]')");
            Ok(())
        })
    }

    /// Java's `variantGet`: a member by key, or a 0-based index, its JSON text.
    pub(super) fn variant_get_arm(&self, w: &mut SqlWriter, args: &[SqlExpr]) -> Result<(), Refusal> {
        let key = arg(args, 1)?;
        if let SqlExpr::StringLit { value, .. } = key
            && value == "$"
        {
            w.expr(self, arg(args, 0)?, 0)?;
            return Ok(());
        }
        if is_text(key) {
            w.append("JSON_EXTRACT(").expr(self, arg(args, 0)?, 0)?.append(", CONCAT('$.\"', REPLACE(");
            w.expr(self, key, 0)?.append(", '\"', '\\\"'), '\"'))");
            return Ok(());
        }
        w.append("JSON_EXTRACT(").expr(self, arg(args, 0)?, 0)?.append(", ");
        let path = self.index_path(self.ex(key, 0));
        w.piece(&*path)?.append(")");
        Ok(())
    }

    /// Java's `variantAwareCast`: each cast decided here.
    pub(super) fn cast_arm(&self, w: &mut SqlWriter, v: &SqlExpr, target: &SqlType) -> Result<(), Refusal> {
        if matches!(target, SqlType::Scalar(Scalar::TemporalText | Scalar::DecimalText)) {
            w.expr(self, v, 0)?;
            return Ok(());
        }
        if matches!(v, SqlExpr::NullLit { .. }) {
            w.append("NULL");
            return Ok(());
        }
        if *target == SqlType::Scalar(Scalar::Json) {
            if is_json(v) {
                w.expr(self, v, 0)?;
            } else if is_text(v) {
                let compact = self.compact(self.ex(v, 0));
                w.piece(&*compact)?;
            } else {
                let variant = self.variant_of(v);
                w.piece(&*variant)?;
            }
            return Ok(());
        }
        if is_json(v) {
            let read = self.variant_as(v, target);
            w.piece(&*read)?;
            return Ok(());
        }
        if let SqlType::Array { element } = target {
            let from = list_element(v);
            if from.is_none_or(|f| f == element.as_ref()) {
                w.expr(self, v, 0)?;
                return Ok(());
            }
            // element-wise
            let each = self.cast_to(text(self.param("__c")), from, element);
            let select = self.json_of(each, Some(element));
            let over = self.over_elements(v, "__c".to_owned(), "__co".to_owned(), None, select, None);
            w.piece(&*over)?;
            return Ok(());
        }
        if matches!(target, SqlType::Struct { .. } | SqlType::Map { .. }) {
            w.expr(self, v, 0)?;
            return Ok(());
        }
        if *target == SqlType::Scalar(Scalar::Varchar) {
            let t = self.as_text(v);
            w.piece(&*t)?;
            return Ok(());
        }
        let cast = self.cast_to(self.ex(v, 0), type_of(v), target);
        w.piece(&*cast)?;
        Ok(())
    }

    /// Java's `fromVariant(v, to)`: a Variant's value as `to`.
    fn variant_as<'a>(&'a self, v: &'a SqlExpr, to: &SqlType) -> P<'a> {
        // a member read as text is its scalar text (a string unquoted); a whole value as text is its JSON
        let member = matches!(v, SqlExpr::Call { r#fn: SqlFn::VariantGet, .. });
        if *to == SqlType::Scalar(Scalar::Varchar) && !member {
            return self.json_as_text(self.ex(v, 0));
        }
        if *to == SqlType::Scalar(Scalar::Varchar) {
            let as_text = self.json_as_text(self.ex(v, 0));
            return pc(move |w| {
                w.append("(CASE WHEN JSON_TYPE(").expr(self, v, 0)?.append(") = 'STRING' THEN JSON_UNQUOTE(");
                w.expr(self, v, 0)?.append(") ELSE ").piece(&*as_text)?.append(" END)");
                Ok(())
            });
        }
        if *to == SqlType::Scalar(Scalar::Boolean) {
            // a JSON boolean's JSON_VALUE is '1' or '0' (MySQL's 'true' or 'false')
            let text = self.json_text(self.ex(v, 0), text("'$'"));
            return pc(move |w| {
                w.append("(CASE JSON_TYPE(").expr(self, v, 0)?.append(") WHEN 'BOOLEAN' THEN ").piece(&*text)?;
                w.append(" IN ('1', 'true') WHEN 'STRING' THEN CASE lower(JSON_UNQUOTE(").expr(self, v, 0)?;
                w.append(")) WHEN 'true' THEN TRUE WHEN 'false' THEN FALSE END WHEN 'INTEGER' THEN ").piece(&*text)?;
                w.append(" <> 0 END)");
                Ok(())
            });
        }
        if let SqlType::Array { element } = to {
            let each = if **element == SqlType::Scalar(Scalar::Json) {
                text(self.param("__j"))
            } else {
                self.scalar_of_text(text(self.json_text_of(&self.param("__j"), "'$'")), Some(element))
            };
            let agg = self.array_agg(false, self.json_of(each, Some(element)), Some(text(self.param("__jo"))), false);
            let spec =
                format!("'$[*]' COLUMNS ({} FOR ORDINALITY, {} JSON PATH '$')", self.param("__jo"), self.param("__j"));
            let table = self.json_table(self.ex(v, 0), spec, "__jt".to_owned());
            return pc(move |w| {
                w.append("(CASE WHEN JSON_TYPE(").expr(self, v, 0)?.append(") = 'ARRAY' THEN coalesce((SELECT ");
                w.piece(&*agg)?.append(" FROM ").piece(&*table)?.append("), JSON_ARRAY()) END)");
                Ok(())
            });
        }
        if matches!(to, SqlType::Map { .. } | SqlType::Struct { .. }) {
            return self.ex(v, 0);
        }
        self.scalar_of_text(self.json_text(self.ex(v, 0), text("'$'")), Some(to))
    }

    /// Java's `castTo(value, from, to)`: `value` cast to `to`, each cast decided here.
    pub(super) fn cast_to<'a>(&self, value: P<'a>, from: Option<&SqlType>, to: &SqlType) -> P<'a> {
        if let SqlType::Decimal { .. } = to {
            let t = self.column_type(Some(to));
            return pc(move |w| {
                w.append("CAST(").piece(&*value)?.append(&format!(" AS {t})"));
                Ok(())
            });
        }
        let SqlType::Scalar(s) = to else {
            return value;
        };
        let cast = |t: &'static str| -> P<'a> {
            let value = value.clone();
            pc(move |w| {
                w.append("CAST(").piece(&*value)?.append(" AS ").append(t).append(")");
                Ok(())
            })
        };
        match s {
            Scalar::Varchar => self.text_of(value, from),
            Scalar::Boolean => {
                if matches!(from, Some(SqlType::Scalar(Scalar::Varchar | Scalar::Json))) {
                    pc(move |w| {
                        w.append("(CASE lower(").piece(&*value)?;
                        w.append(") WHEN 'true' THEN TRUE WHEN 'false' THEN FALSE END)");
                        Ok(())
                    })
                } else {
                    pc(move |w| {
                        w.append("(").piece(&*value)?.append(" <> 0)");
                        Ok(())
                    })
                }
            }
            // DuckDB rounds a double to an integer, half away from zero
            Scalar::Integer | Scalar::Bigint => {
                if from == Some(&SqlType::Scalar(Scalar::Double)) {
                    pc(move |w| {
                        w.append("CAST(sign(").piece(&*value)?.append(") * floor(abs(").piece(&*value)?;
                        w.append(") + 0.5) AS SIGNED)");
                        Ok(())
                    })
                } else {
                    cast("SIGNED")
                }
            }
            Scalar::Hugeint => cast("DECIMAL(38,0)"),
            Scalar::Double => cast("DOUBLE"),
            Scalar::Date => cast("DATE"),
            Scalar::Timestamp => {
                if from == Some(&SqlType::Scalar(Scalar::Varchar)) {
                    pc(move |w| {
                        w.append("CAST(REPLACE(").piece(&*value)?.append(", 'T', ' ') AS DATETIME(6))");
                        Ok(())
                    })
                } else {
                    cast("DATETIME(6)")
                }
            }
            // a zoned date-time is held as its naive UTC date-time: its offset applied at the cast
            Scalar::Timestamptz => {
                if from == Some(&SqlType::Scalar(Scalar::Varchar)) {
                    utc_of(value)
                } else {
                    cast("DATETIME(6)")
                }
            }
            Scalar::Json => pc(move |w| {
                w.append("JSON_EXTRACT(JSON_ARRAY(").piece(&*value)?.append("), '$[0]')");
                Ok(())
            }),
            Scalar::Time => cast("TIME(6)"),
            Scalar::TemporalText | Scalar::DecimalText => value,
        }
    }

    /// Java's `asText(e)`: a value as text, as DuckDB's CAST to VARCHAR prints it.
    pub(super) fn as_text<'a>(&'a self, e: &'a SqlExpr) -> P<'a> {
        if matches!(e, SqlExpr::StringLit { .. }) {
            return self.ex(e, 0);
        }
        let double = SqlType::Scalar(Scalar::Double);
        let boolean = SqlType::Scalar(Scalar::Boolean);
        let t = match e {
            SqlExpr::FloatLit { .. } => Some(&double),
            SqlExpr::BoolLit { .. } => Some(&boolean),
            _ => type_of(e),
        };
        self.text_of(self.ex(e, 0), t)
    }

    /// Java's `textOf(value, type)`: DuckDB's text of a value of `type`.
    pub(super) fn text_of<'a>(&self, value: P<'a>, r#type: Option<&SqlType>) -> P<'a> {
        if r#type == Some(&SqlType::Scalar(Scalar::Double)) {
            return self.float_layout(value, false);
        }
        if r#type == Some(&SqlType::Scalar(Scalar::Boolean)) {
            return pc(move |w| {
                w.append("(CASE WHEN ").piece(&*value)?.append(" THEN 'true' WHEN NOT (").piece(&*value)?;
                w.append(") THEN 'false' END)");
                Ok(())
            });
        }
        if matches!(r#type, Some(SqlType::Scalar(Scalar::Timestamp | Scalar::Timestamptz))) {
            return pc(move |w| {
                w.append("(CASE WHEN MICROSECOND(").piece(&*value)?.append(") = 0 THEN DATE_FORMAT(").piece(&*value)?;
                w.append(", '%Y-%m-%d %H:%i:%s') ELSE TRIM(TRAILING '0' FROM DATE_FORMAT(").piece(&*value)?;
                w.append(", '%Y-%m-%d %H:%i:%s.%f')) END)");
                Ok(())
            });
        }
        pc(move |w| {
            w.append("CAST(").piece(&*value)?.append(" AS CHAR)");
            Ok(())
        })
    }

    /// Java's `floatLayout(value, canon)`: a double's text as DuckDB prints it, or (with `canon`) Pure's literal
    /// spelling, the value bound once through a one-row JSON_TABLE.
    pub(super) fn float_layout<'a>(&self, value: P<'a>, canon: bool) -> P<'a> {
        // __f.t: |v| as MariaDB's text; __f.n: v < 0
        let t = "__f.\"t\"";
        let mant = format!("SUBSTRING_INDEX({t}, 'e', 1)");
        let ip = format!("SUBSTRING_INDEX({t}, '.', 1)");
        let fp = format!("CASE WHEN INSTR({t}, '.') > 0 THEN SUBSTRING_INDEX({t}, '.', -1) ELSE '' END");
        // the significant digits, and the decimal exponent of the first
        let digits = format!(
            "TRIM(TRAILING '0' FROM CASE WHEN INSTR({t}, 'e') > 0 THEN REPLACE({mant}, '.', '') ELSE TRIM(LEADING '0' \
             FROM CONCAT({ip}, {fp})) END)"
        );
        let exp = format!(
            "CASE WHEN INSTR({t}, 'e') > 0 THEN CAST(SUBSTRING_INDEX({t}, 'e', -1) AS SIGNED) WHEN {ip} <> '0' THEN \
             CHAR_LENGTH({ip}) - 1 ELSE -(CHAR_LENGTH({fp}) - CHAR_LENGTH(TRIM(LEADING '0' FROM {fp})) + 1) END"
        );
        let d = "__g.\"d\"";
        let x = "__g.\"x\"";
        let fixed = format!(
            "CASE WHEN {x} >= 0 THEN CONCAT(RPAD(LEFT({d}, {x} + 1), {x} + 1, '0'), '.', CASE WHEN CHAR_LENGTH({d}) > \
             {x} + 1 THEN SUBSTRING({d}, {x} + 2) ELSE '0' END) ELSE CONCAT('0.', REPEAT('0', -{x} - 1), {d}) END"
        );
        let sci = format!(
            "CONCAT(LEFT({d}, 1), CASE WHEN CHAR_LENGTH({d}) > 1 THEN CONCAT('.', SUBSTRING({d}, 2)) ELSE '' END, \
             'e', CASE WHEN {x} < 0 THEN '-' ELSE '+' END, LPAD(ABS({x}), 2, '0'))"
        );
        let zero = if canon { "'0.0'".to_owned() } else { "CASE WHEN __f.\"n\" THEN '-0.0' ELSE '0.0' END".to_owned() };
        let layout = if canon { fixed } else { format!("CASE WHEN {x} BETWEEN -4 AND 15 THEN {fixed} ELSE {sci} END") };
        let inner = self.json_table(
            text(format!("JSON_ARRAY({digits}, {exp})")),
            "'$' COLUMNS (\"d\" VARCHAR(64) PATH '$[0]', \"x\" INT PATH '$[1]')".to_owned(),
            "__g".to_owned(),
        );
        let held = pc(move |v| {
            v.append("JSON_ARRAY(CAST(ABS(").piece(&*value)?.append(") AS CHAR), ").piece(&*value)?.append(" < 0)");
            Ok(())
        });
        let outer = self.json_table(
            held,
            format!("'$' COLUMNS (\"t\" VARCHAR(64) PATH '$[0]', \"n\" {} PATH '$[1]')", self.boolean_column()),
            "__f".to_owned(),
        );
        pc(move |w| {
            w.append(&format!("(SELECT CASE WHEN __f.\"t\" IS NULL THEN NULL WHEN __f.\"t\" = '0' THEN {zero}"));
            w.append(&format!(" ELSE CONCAT(CASE WHEN __f.\"n\" THEN '-' ELSE '' END, (SELECT {layout} FROM "));
            w.piece(&*inner)?.append(")) END FROM ").piece(&*outer)?.append(")");
            Ok(())
        })
    }
}

/// Java's `SqlExpr.Column.param(name, collection)`.
pub(super) fn column_param(name: &str, collection: &SqlExpr) -> SqlExpr {
    match list_element(collection) {
        Some(element) => build::typed_column(None, name, element.clone(), true, lite_sql::Origin::Derived),
        None => build::derived(None, name),
    }
}

/// Java's `castTarget(t)`: the CAST target a value of type `t` is (none for none).
pub(super) fn cast_target(d: &MariaDb, t: Option<&SqlType>) -> Option<String> {
    match t {
        Some(dec @ SqlType::Decimal { .. }) => Some(d.column_type(Some(dec))),
        Some(SqlType::Scalar(s)) => Some(
            match s {
                Scalar::Boolean | Scalar::Integer | Scalar::Bigint => "SIGNED",
                Scalar::Hugeint => "DECIMAL(38,0)",
                Scalar::Double => "DOUBLE",
                Scalar::Date => "DATE",
                Scalar::Timestamp | Scalar::Timestamptz => "DATETIME(6)",
                Scalar::Time => "TIME(6)",
                Scalar::Varchar | Scalar::TemporalText | Scalar::DecimalText | Scalar::Json => "CHAR",
            }
            .to_owned(),
        ),
        _ => None,
    }
}

/// A lambda's `i`-th parameter (Java's `l.params().get(i)`).
fn arg_param(params: &[String], i: usize) -> Result<&str, Refusal> {
    params.get(i).map(String::as_str).ok_or_else(|| Refusal {
        class: Refusal::INDEX_OUT_OF_BOUNDS,
        message: format!("Index {i} out of bounds for length {}", params.len()),
    })
}

/// Java's `utcOf(text)`: a date-time text with an offset as its UTC date-time.
pub(super) fn utc_of<'a>(text: P<'a>) -> P<'a> {
    let t = pc(move |w| {
        w.append("REPLACE(TRIM(").piece(&*text)?.append("), 'T', ' ')");
        Ok(())
    });
    let len = {
        let t = t.clone();
        pc(move |w| {
            w.append("CHAR_LENGTH(").piece(&*t)?.append(")");
            Ok(())
        })
    };
    let shifted = |off: P<'a>, cut: i32| -> P<'a> {
        let t = t.clone();
        let len = len.clone();
        pc(move |w| {
            w.append("(CAST(LEFT(").piece(&*t)?.append(", ").piece(&*len)?;
            w.append(&format!(" - {cut}) AS DATETIME(6)) - INTERVAL"));
            w.append(" (CASE LEFT(").piece(&*off)?.append(", 1) WHEN '-' THEN -1 ELSE 1 END) * (CAST(SUBSTRING(");
            w.piece(&*off)?.append(", 2, 2) AS SIGNED) * 60 + CAST(RIGHT(").piece(&*off)?;
            w.append(", 2) AS SIGNED)) MINUTE)");
            Ok(())
        })
    };
    let off6 = {
        let t = t.clone();
        pc(move |w| {
            w.append("RIGHT(").piece(&*t)?.append(", 6)");
            Ok(())
        })
    };
    let off5 = {
        let t = t.clone();
        pc(move |w| {
            w.append("RIGHT(").piece(&*t)?.append(", 5)");
            Ok(())
        })
    };
    let six = shifted(off6, 6);
    let five = shifted(off5, 5);
    pc(move |w| {
        w.append("(CASE WHEN ").piece(&*t)?.append(" LIKE '%Z' THEN CAST(LEFT(").piece(&*t)?.append(", ");
        w.piece(&*len)?.append(" - 1) AS DATETIME(6)) WHEN ").piece(&*t)?;
        w.append(" REGEXP '[+-][0-9]{2}:[0-9]{2}$' THEN ").piece(&*six)?.append(" WHEN ").piece(&*len)?;
        w.append(" > 10 AND ").piece(&*t)?.append(" REGEXP '[0-9][+-][0-9]{4}$' THEN ").piece(&*five)?;
        w.append(" ELSE CAST(").piece(&*t)?.append(" AS DATETIME(6)) END)");
        Ok(())
    })
}
