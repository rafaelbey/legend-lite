// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! Java's `toString()` of a protocol type expression (2026-11-12), the one place a record's default spelling reaches
//! an answer of the names layer: `KnowledgeLayer.rawName` spells a supertype or a return type that is no name (a
//! function, relation or schema-algebra type) by its `toString()`, and an association qualified property's refusal
//! quotes it. A record's default `toString` is `Name[component=value, ...]`, a list `[a, b]`, a null `null`;
//! `Multiplicity` writes its own (`[1]`, `[0..1]`, `[*]`, `[1..*]`, `[m]`).
//!
//! A generic's type-variable values are value specifications, whose `toString` is not ported: a type holding one is
//! refused loudly (`IllegalStateException`), never spelled by a guess.

use lite_model::protocol as p;
use lite_model::{JStr, cat};

use crate::error::Thrown;

/// `t.toString()`.
pub fn type_expression(t: &p::TypeExpression) -> Result<JStr, Thrown> {
    let mut out = String::new();
    write_type(t, &mut out)?;
    // the names are UTF-16 in the record: rebuilt from units below where they are written
    Ok(JStr::from(out))
}

fn write_type(t: &p::TypeExpression, out: &mut String) -> Result<(), Thrown> {
    match t {
        p::TypeExpression::TypeExpressionNameRef(n) => {
            out.push_str("NameRef[name=");
            out.push_str(&n.name.lossy());
            out.push_str(", pos=");
            write_pos(n.pos.as_deref(), out);
            out.push(']');
        }
        p::TypeExpression::TypeExpressionGeneric(g) => {
            if !g.type_variable_values.is_empty() {
                return Err(Thrown::illegal_state(cat!(
                    "the names port does not spell a type variable value's Java toString (a Generic of ",
                    &g.name,
                    ")"
                )));
            }
            out.push_str("Generic[name=");
            out.push_str(&g.name.lossy());
            out.push_str(", arguments=[");
            for (i, a) in g.arguments.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                write_type(a, out)?;
            }
            out.push_str("], multiplicityArguments=[");
            for (i, m) in g.multiplicity_arguments.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                out.push_str(&m.lossy());
            }
            out.push_str("], typeVariableValues=[], pos=");
            write_pos(g.pos.as_deref(), out);
            out.push(']');
        }
        p::TypeExpression::TypeExpressionFunctionType(f) => {
            out.push_str("FunctionType[parameters=[");
            for (i, tp) in f.parameters.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                write_typed_parameter(tp, out)?;
            }
            out.push_str("], result=");
            write_typed_parameter(&f.result, out)?;
            out.push(']');
        }
        p::TypeExpression::TypeExpressionRelationType(r) => {
            out.push_str("RelationType[columns=[");
            for (i, c) in r.columns.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                out.push_str("Column[name=");
                out.push_str(&c.name.lossy());
                out.push_str(", type=");
                write_type(&c.r#type, out)?;
                out.push_str(", multiplicity=");
                out.push_str(&multiplicity(&c.multiplicity));
                out.push_str(", pos=");
                write_pos(c.pos.as_deref(), out);
                out.push(']');
            }
            out.push_str("]]");
        }
        p::TypeExpression::TypeExpressionSchemaAlgebra(s) => {
            out.push_str("SchemaAlgebra[left=");
            write_type(&s.left, out)?;
            out.push_str(", op=");
            out.push_str(match s.op {
                p::TypeExpressionOp::EQUAL => "EQUAL",
                p::TypeExpressionOp::UNION => "UNION",
                p::TypeExpressionOp::DIFFERENCE => "DIFFERENCE",
                p::TypeExpressionOp::SUBSET => "SUBSET",
            });
            out.push_str(", right=");
            write_type(&s.right, out)?;
            out.push(']');
        }
    }
    Ok(())
}

fn write_typed_parameter(tp: &p::TypeExpressionTypedParameter, out: &mut String) -> Result<(), Thrown> {
    out.push_str("TypedParameter[type=");
    write_type(&tp.r#type, out)?;
    out.push_str(", multiplicity=");
    out.push_str(&multiplicity(&tp.multiplicity));
    out.push(']');
    Ok(())
}

fn write_pos(pos: Option<&p::SourceInfo>, out: &mut String) {
    match pos {
        None => out.push_str("null"),
        Some(s) => {
            out.push_str("SourceInfo[sourceId=");
            out.push_str(&s.source_id.lossy());
            out.push_str(&format!(
                ", startLine={}, startColumn={}, endLine={}, endColumn={}]",
                s.start_line, s.start_column, s.end_line, s.end_column
            ));
        }
    }
}

/// `Multiplicity.toString()`.
pub fn multiplicity(m: &p::Multiplicity) -> String {
    match m {
        p::Multiplicity::MultiplicityConcrete(c) => match c.upper_bound {
            None if c.lower_bound == 0 => "[*]".to_owned(),
            None => format!("[{}..*]", c.lower_bound),
            Some(u) if u == c.lower_bound => format!("[{}]", c.lower_bound),
            Some(u) => format!("[{}..{}]", c.lower_bound, u),
        },
        p::Multiplicity::MultiplicityParameter(pm) => format!("[{}]", pm.name.lossy()),
    }
}
