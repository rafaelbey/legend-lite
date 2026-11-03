// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! Constructors for the trees a renderer builds and writes (Java's `SqlExpr.Call.of`, `new SqlExpr.Case(...)`, ...).
//! Java's constructors compute a node's type fact (`SqlTyping`); no spelling of a built tree reads one, so a
//! computed fact is `Unknown` here and a literal's is the constant Java stamps (`T_VARCHAR`, `T_BIGINT`, `BOTTOM`).

use lite_sql::{Column, Double, Lambda, Origin, Scalar, SqlExpr, SqlFn, SqlType, TypeFact, When};

fn typed(s: Scalar) -> TypeFact {
    TypeFact::Typed { r#type: SqlType::Scalar(s), nullable: false }
}

/// `SqlExpr.Call.of(fn, args...)`.
pub fn call(f: SqlFn, args: Vec<SqlExpr>) -> SqlExpr {
    SqlExpr::Call { r#fn: f, args, r#type: TypeFact::Unknown }
}

/// `new SqlExpr.StringLit(value)`.
pub fn string_lit(value: &str) -> SqlExpr {
    SqlExpr::StringLit { value: value.to_owned(), r#type: typed(Scalar::Varchar) }
}

/// `new SqlExpr.IntLit(value)`.
pub fn int_lit(value: i64) -> SqlExpr {
    SqlExpr::IntLit { value, r#type: typed(Scalar::Bigint) }
}

/// `new SqlExpr.FloatLit(value)`: the double as MirJson carries one, its `Double.toString` text.
pub fn float_lit(value: f64) -> SqlExpr {
    let value = Double::parse(&lite_javacompat::double_to_string(value)).expect("Java's text of a finite double");
    SqlExpr::FloatLit { value, r#type: TypeFact::Unknown }
}

/// `new SqlExpr.NullLit()`.
pub fn null_lit() -> SqlExpr {
    SqlExpr::NullLit { r#type: TypeFact::Bottom }
}

/// `new SqlExpr.Case(whens, otherwise)`.
pub fn case(whens: Vec<When>, otherwise: Option<SqlExpr>) -> SqlExpr {
    SqlExpr::Case { whens, otherwise: otherwise.map(Box::new), r#type: TypeFact::Unknown }
}

/// `new SqlExpr.Case.When(condition, then)`.
pub fn when(condition: SqlExpr, then: SqlExpr) -> When {
    When { condition, then }
}

/// `new SqlExpr.Cast(value, target)`.
pub fn cast(value: SqlExpr, target: SqlType) -> SqlExpr {
    SqlExpr::Cast { value: Box::new(value), target, conform: false, r#type: TypeFact::Unknown }
}

/// `new SqlExpr.Lambda(params, body)`.
pub fn lambda(params: &[&str], body: SqlExpr) -> SqlExpr {
    SqlExpr::Lambda(Lambda {
        params: params.iter().map(|p| (*p).to_owned()).collect(),
        body: Box::new(body),
        r#type: TypeFact::Unknown,
    })
}

/// `SqlExpr.Column.derived(table, name)`: an untyped reference to a name the query invented.
pub fn derived(table: Option<&str>, name: &str) -> SqlExpr {
    SqlExpr::Column(Column {
        table: table.map(str::to_owned),
        name: name.to_owned(),
        r#type: TypeFact::Unknown,
        origin: Some(Origin::Derived),
    })
}
