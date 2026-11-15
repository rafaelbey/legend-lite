// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! `DeclaredCoercions` (DeclaredCoercions.java at ad86fa895): `castAsDeclared` / `typeAsDeclared` / `parse*` around
//! each mapped value, as its declared property type asks (SQL and result typing). RE-EXPRESSED (census §4): the
//! unused `ledger` parameters are dropped, and the javadoc's `RequiredNullableCensus` hook (a deleted class) is not
//! ported. `declaredAssertion` does not see through views where `coerceColumnToDeclared` does, and a declared-Boolean
//! string errors in SQL where the engine's `Boolean.parseBoolean` yields false ("DELIBERATE divergence", :47-51): ASK
//! 12 and 13's, ported as Java has them.
//!
//! `MissProbe.knownMiss` / `orElseGet(MissProbe::miss)` (:80, :114) are the `Option`s themselves (census §4). Java's
//! `db == null` tests (:111, :146) cannot hold: the records hold `database` non-null (`PropertyMapping.Column`'s and
//! `JoinTerminalColumn`'s, lite-model's non-optional fields).

use lite_builtin::pure::lite;
use lite_front::records as p;
use lite_model::text::{contains, starts_with};
use lite_model::{JStr, PropertyMappingColumn, PropertyMappingJoinTerminalColumn, RelationalOperation};
use lite_names::ModelIndex;

use crate::error::Thrown;
use crate::rel_op_translator::{exact, platform, type_named_j};

/// The platform primitive package a declared type may be spelled in full with.
const PRIMITIVE_PACKAGE: &str = "meta::pure::metamodel::type::";

/// The declared kinds an expression property mapping's value casts to (:56).
const NUMERIC_OR_TEMPORAL: [&str; 8] = [
    "Float",
    "Integer",
    "Decimal",
    "Number",
    "DateTime",
    "StrictDate",
    "Date",
    "Boolean",
];

/// The numeric kinds (:177).
const NUMERIC: [&str; 4] = ["Float", "Decimal", "Integer", "Number"];

/// `DeclaredCoercions.coerceToDeclaredNumeric` (:53): a numeric value conformed to its declared number type.
pub fn coerce_to_declared_numeric(
    value: &p::ValueSpecification,
    prop_name: &JStr,
    owner_class_fqn: Option<&JStr>,
    model: &ModelIndex,
) -> Result<p::ValueSpecification, Thrown> {
    let simple = declared_platform_kind(prop_name, owner_class_fqn, model)?;
    let Some(simple) = simple.filter(|s| NUMERIC_OR_TEMPORAL.iter().any(|k| s == k)) else {
        return Ok(value.clone());
    };
    // a WIRE coercion (the engine runtime converts on the wire and its SQL/plan text never spells it) --
    // castAsDeclared casts at execution, reads bare in the engine-text funnel
    Ok(exact(lite::CAST_AS_DECLARED, vec![value.clone(), type_named_j(simple)]))
}

/// `DeclaredCoercions.declaredPlatformKind` (:75): the property's declared platform type's kind, else none -- the bare
/// spelling (no user class of that name) or the full platform FQN, never a suffix match.
pub fn declared_platform_kind(
    prop_name: &JStr,
    owner_class_fqn: Option<&JStr>,
    model: &ModelIndex,
) -> Result<Option<JStr>, Thrown> {
    let Some(owner_class_fqn) = owner_class_fqn else {
        return Ok(None);
    };
    let knowledge = model.knowledge();
    let t = match knowledge.hierarchy_class(Some(owner_class_fqn)) {
        None => None,
        Some(owner) => knowledge.property_type(Some(&**owner), prop_name)?,
    };
    let Some(p::TypeExpression::TypeExpressionNameRef(nr)) = t else {
        return Ok(None);
    };
    let name = &nr.name;
    if !contains(&name.0, "::") && knowledge.hierarchy_class(Some(name)).is_none() {
        return Ok(Some(name.clone()));
    }
    if starts_with(&name.0, PRIMITIVE_PACKAGE) {
        return Ok(Some(JStr::from(&name.0[PRIMITIVE_PACKAGE.len()..])));
    }
    Ok(None)
}

/// `DeclaredCoercions.declaredAssertion` (:99, its `ledger` dropped): a join-terminal column's read asserted as its
/// declared type (`typeAsDeclared`, no SQL) when the terminal column's physical kind differs. The column is a TABLE's
/// (`knowledge().column`, no view): the asymmetry with [`coerce_column_to_declared`] is Java's (module note).
pub fn declared_assertion(
    read: &p::ValueSpecification,
    jtc: &PropertyMappingJoinTerminalColumn,
    owner_class_fqn: &JStr,
    model: &ModelIndex,
) -> Result<p::ValueSpecification, Thrown> {
    let declared = declared_platform_kind(&jtc.property_name, Some(owner_class_fqn), model)?;
    let (Some(declared), RelationalOperation::RelationalOperationColumnRef(cr)) = (declared, &jtc.terminal_column)
    else {
        return Ok(read.clone());
    };
    let db = cr.database_name.as_ref().unwrap_or(&jtc.database);
    // the terminal column's kind resolves THROUGH a view, as coerce_column_to_declared's does (2026-11-15, the
    // normalizer batch, item 2, ASK 13)
    let col_kind = model.knowledge().column_kind(db, &cr.table, &cr.column)?;
    match col_kind {
        Some(k) if declared != k => Ok(exact(
            lite::TYPE_AS_DECLARED,
            vec![read.clone(), type_named_j(declared)],
        )),
        _ => Ok(read.clone()),
    }
}

/// `DeclaredCoercions.coerceColumnToDeclared` (:133, its `ledger` dropped): a column's read conformed to its declared
/// type (through a view's column to its physical one) -- a wire cast for String/Boolean, `cast` for DateTime over
/// StrictDate, the type-only assertion for numeric over numeric, a parse for numeric over String; anything else
/// bare (the type checker stays the loud arbiter).
pub fn coerce_column_to_declared(
    read: &p::ValueSpecification,
    col: &PropertyMappingColumn,
    owner_class_fqn: &JStr,
    model: &ModelIndex,
) -> Result<p::ValueSpecification, Thrown> {
    let Some(declared) = declared_platform_kind(&col.property_name, Some(owner_class_fqn), model)? else {
        return Ok(read.clone());
    };
    // the column's kind resolves THROUGH a view (its column expression's inferred SQL type)
    let Some(col_kind) = model.knowledge().column_kind(&col.database, &col.table, &col.column)? else {
        return Ok(read.clone());
    };
    if declared == col_kind {
        return Ok(read.clone());
    }
    // String/Boolean-declared over a mismatched column is a WIRE coercion -- castAsDeclared casts at execution, the
    // engine-text funnel reads it bare
    if declared == "String" || declared == "Boolean" {
        return Ok(exact(
            lite::CAST_AS_DECLARED,
            vec![read.clone(), type_named_j(declared)],
        ));
    }
    let cast = declared == "DateTime" && col_kind == "StrictDate";
    if cast {
        return Ok(platform("cast", vec![read.clone(), type_named_j(declared)]));
    }
    let declared_numeric = NUMERIC.iter().any(|k| declared == *k);
    if declared_numeric && NUMERIC.contains(&col_kind) {
        // NUMERIC CHARTER Rules 1 + 2: a numeric property over a numeric column is a TYPE assertion, never a
        // read-time cast (the engine's numeric transformers are the identity)
        return Ok(exact(
            lite::TYPE_AS_DECLARED,
            vec![read.clone(), type_named_j(declared)],
        ));
    }
    // a NUMERIC property over a VARCHAR column parses by EMISSION
    if declared_numeric && col_kind == "String" {
        let parse_fn = if declared == "Integer" {
            Some("parseInteger")
        } else if declared == "Float" || declared == "Number" {
            Some("parseFloat")
        } else if declared == "Decimal" {
            Some("parseDecimal")
        } else {
            None
        };
        if let Some(parse_fn) = parse_fn {
            // parse natives are strict String[1]; the column read is SQL-lane [0..1] -- the toOne trust wrap
            return Ok(platform(parse_fn, vec![exact(lite::TRUST_ONE, vec![read.clone()])]));
        }
    }
    Ok(read.clone())
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use lite_model::linked::empty;
    use lite_model::text::j;
    use lite_model::{JValue, RelationalOperationColumnRef, RelationalOperationLiteral};

    use super::*;
    use crate::test_support::{index, model};

    const MODEL: &str = r#"
###Pure
Class a::Q
{
  n: String[1];
}
Class a::P
{
  s: String[1];
  f: Float[1];
  i: Integer[1];
  d: Decimal[1];
  n: Number[1];
  dt: DateTime[1];
  sd: StrictDate[1];
  b: Boolean[1];
  q: a::Q[1];
}

###Relational
Database a::db
(
  Table T (ID INTEGER PRIMARY KEY, NAME VARCHAR(20), AMT DECIMAL(10,2), DT DATE, TS TIMESTAMP, FLAG BIT)
  View V (VID: T.ID, VNAME: T.NAME)
)
"#;

    fn read() -> p::ValueSpecification {
        p::ValueSpecification::Variable(Arc::new(p::Variable {
            name: j("r"),
            r#type: None,
            multiplicity: None,
            pos: None,
        }))
    }

    /// `f(a, b)` with a platform function by its last segment, `$r`, `@T`.
    fn show(v: &p::ValueSpecification) -> String {
        match v {
            p::ValueSpecification::AppliedFunction(f) => {
                let name = f.function.lossy();
                let name = match name.strip_prefix("meta::pure::functions::") {
                    Some(rest) => rest.rsplit("::").next().unwrap_or(rest).to_owned(),
                    None => name,
                };
                let args: Vec<String> = f.parameters.iter().map(show).collect();
                format!("{name}({})", args.join(", "))
            }
            p::ValueSpecification::Variable(v) => format!("${}", v.name),
            p::ValueSpecification::TypeAnnotation(p::TypeAnnotation::TypeAnnotationNamed(n)) => match &n.r#type {
                p::TypeExpression::TypeExpressionNameRef(nr) => format!("@{}", nr.name),
                other => format!("@{other:?}"),
            },
            other => format!("{other:?}"),
        }
    }

    fn column(prop: &str, table: &str, col: &str) -> PropertyMappingColumn {
        PropertyMappingColumn {
            property_name: j(prop),
            database: j("a::db"),
            table: j(table),
            column: j(col),
        }
    }

    fn terminal(prop: &str, terminal_column: RelationalOperation) -> PropertyMappingJoinTerminalColumn {
        PropertyMappingJoinTerminalColumn {
            property_name: j(prop),
            database: j("a::db"),
            joins: empty(),
            terminal_column,
            enum_mapping_id: None,
            enum_mapped: false,
        }
    }

    fn col_ref(table: &str, col: &str) -> RelationalOperation {
        RelationalOperation::RelationalOperationColumnRef(Arc::new(RelationalOperationColumnRef {
            database_name: None,
            table: j(table),
            column: j(col),
        }))
    }

    #[test]
    fn a_declared_kind_is_a_platform_primitive_never_a_class() {
        let parsed = model(MODEL);
        let idx = index(&parsed);
        let kind = |prop: &str, owner: Option<&str>| {
            declared_platform_kind(&j(prop), owner.map(j).as_ref(), &idx)
                .unwrap()
                .map(|k| k.lossy())
        };
        assert_eq!(kind("s", Some("a::P")).as_deref(), Some("String"));
        assert_eq!(kind("dt", Some("a::P")).as_deref(), Some("DateTime"));
        assert_eq!(kind("q", Some("a::P")), None);
        assert_eq!(kind("nope", Some("a::P")), None);
        assert_eq!(kind("s", Some("a::Nope")), None);
        assert_eq!(kind("s", None), None);
    }

    #[test]
    fn an_expressions_value_casts_to_a_numeric_temporal_or_boolean_declaration() {
        let parsed = model(MODEL);
        let idx = index(&parsed);
        let coerce = |prop: &str| show(&coerce_to_declared_numeric(&read(), &j(prop), Some(&j("a::P")), &idx).unwrap());
        assert_eq!(coerce("f"), "meta::legend::lite::castAsDeclared($r, @Float)");
        assert_eq!(coerce("sd"), "meta::legend::lite::castAsDeclared($r, @StrictDate)");
        assert_eq!(coerce("b"), "meta::legend::lite::castAsDeclared($r, @Boolean)");
        assert_eq!(coerce("s"), "$r");
        assert_eq!(coerce("q"), "$r");
        assert_eq!(
            show(&coerce_to_declared_numeric(&read(), &j("f"), None, &idx).unwrap()),
            "$r"
        );
    }

    #[test]
    fn a_column_read_conforms_to_its_declared_kind_through_a_view() {
        let parsed = model(MODEL);
        let idx = index(&parsed);
        let coerce = |prop: &str, table: &str, col: &str| {
            show(&coerce_column_to_declared(&read(), &column(prop, table, col), &j("a::P"), &idx).unwrap())
        };
        // matching kinds, an unknown column, a class-typed property: bare
        assert_eq!(coerce("i", "T", "ID"), "$r");
        assert_eq!(coerce("s", "T", "NOPE"), "$r");
        assert_eq!(coerce("q", "T", "ID"), "$r");
        // String/Boolean: the wire cast
        assert_eq!(
            coerce("s", "T", "ID"),
            "meta::legend::lite::castAsDeclared($r, @String)"
        );
        assert_eq!(
            coerce("b", "T", "NAME"),
            "meta::legend::lite::castAsDeclared($r, @Boolean)"
        );
        // DATE widened to DateTime: a SQL cast; the other temporal mismatches bare
        assert_eq!(coerce("dt", "T", "DT"), "cast($r, @DateTime)");
        assert_eq!(coerce("sd", "T", "TS"), "$r");
        // numeric over numeric: the type-only assertion
        assert_eq!(
            coerce("f", "T", "AMT"),
            "meta::legend::lite::typeAsDeclared($r, @Float)"
        );
        assert_eq!(
            coerce("n", "T", "ID"),
            "meta::legend::lite::typeAsDeclared($r, @Number)"
        );
        // numeric over String: a parse of the trusted read
        assert_eq!(
            coerce("i", "T", "NAME"),
            "parseInteger(meta::legend::lite::trustOne($r))"
        );
        assert_eq!(coerce("f", "T", "NAME"), "parseFloat(meta::legend::lite::trustOne($r))");
        assert_eq!(coerce("n", "T", "NAME"), "parseFloat(meta::legend::lite::trustOne($r))");
        assert_eq!(
            coerce("d", "T", "NAME"),
            "parseDecimal(meta::legend::lite::trustOne($r))"
        );
        // numeric over Boolean: bare
        assert_eq!(coerce("i", "T", "FLAG"), "$r");
        // through a view's column to its physical one
        assert_eq!(
            coerce("s", "V", "VID"),
            "meta::legend::lite::castAsDeclared($r, @String)"
        );
        assert_eq!(
            coerce("i", "V", "VNAME"),
            "parseInteger(meta::legend::lite::trustOne($r))"
        );
    }

    #[test]
    fn a_join_terminal_column_is_asserted_by_its_kind_through_a_view() {
        let parsed = model(MODEL);
        let idx = index(&parsed);
        let assert_of = |prop: &str, t: RelationalOperation| {
            show(&declared_assertion(&read(), &terminal(prop, t), &j("a::P"), &idx).unwrap())
        };
        assert_eq!(
            assert_of("s", col_ref("T", "ID")),
            "meta::legend::lite::typeAsDeclared($r, @String)"
        );
        assert_eq!(assert_of("i", col_ref("T", "ID")), "$r");
        assert_eq!(assert_of("q", col_ref("T", "ID")), "$r");
        assert_eq!(assert_of("s", col_ref("T", "NOPE")), "$r");
        // a view's column is seen through, as coerce_column_to_declared's (2026-11-15, the normalizer batch, item 2,
        // census ASK 13: it was not, the asymmetry ASK 12 named)
        assert_eq!(
            assert_of("s", col_ref("V", "VID")),
            "meta::legend::lite::typeAsDeclared($r, @String)"
        );
        // a terminal that is not a column reference
        let lit = RelationalOperation::RelationalOperationLiteral(Arc::new(RelationalOperationLiteral {
            value: JValue::Long(1),
        }));
        assert_eq!(assert_of("s", lit), "$r");
        // the column reference's own database, else the mapping's
        let elsewhere = RelationalOperation::RelationalOperationColumnRef(Arc::new(RelationalOperationColumnRef {
            database_name: Some(j("a::other")),
            table: j("T"),
            column: j("ID"),
        }));
        assert_eq!(assert_of("s", elsewhere), "$r");
    }
}
