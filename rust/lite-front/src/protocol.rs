// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! The protocol records' own rules the parser calls (core/'s `protocol.Protocol`, `Realization`,
//! `ConstraintDefinition`, `ValueSpecification.children`): an FQN split into the wire's package and name, a function's
//! signature-mangled path, a derived body's realization, a constraint's `$return` test.

use crate::cursor::{R, unquote_path_iae};
use crate::doors::JavaError;
use crate::jtypes::JStr;
use crate::records::{
    ColumnInstance, ConstraintDefinition, GraphFetchLiteralNode, Multiplicity, PFunction, PathLiteral, Realization,
    RealizationInline, RealizationRef, SourceInfo, TypeAnnotation, TypeExpression, ValueSpecification,
};
use crate::ustr::{last_index_of_str, u};

use ValueSpecification as VS;

/// `Protocol.splitFqn`: the wire's package and name, split at the last `::` outside a quoted segment, each half's
/// quoted segments unquoted and unescaped (a short unicode escape Java's `IllegalArgumentException`).
pub fn split_fqn(qualified_name: &[u16]) -> R<(Vec<u16>, Vec<u16>)> {
    let i = last_top_level_separator(qualified_name);
    let (pkg, name) = match i {
        None => (&[][..], qualified_name),
        Some(i) => (&qualified_name[..i], &qualified_name[i + 2..]),
    };
    Ok((unquote_path_iae(pkg)?, unquote_path_iae(name)?))
}

/// `Protocol.lastTopLevelSeparator`: the last `::` outside quoted segments (which honour backslash escapes).
fn last_top_level_separator(path: &[u16]) -> Option<usize> {
    let (q, bs, colon) = (u16::from(b'\''), u16::from(b'\\'), u16::from(b':'));
    let mut in_quote = false;
    let mut last = None;
    let mut i = 0;
    while i < path.len() {
        let c = path[i];
        if in_quote && c == bs {
            i += 1;
        } else if c == q {
            in_quote = !in_quote;
        } else if !in_quote && c == colon && i + 1 < path.len() && path[i + 1] == colon {
            last = Some(i);
            i += 1;
        }
        i += 1;
    }
    last
}

/// A record's `qualifiedName()`: `pkg::name`, or the name alone in the root package.
pub fn qualified_name(pkg: &[u16], name: &[u16]) -> Vec<u16> {
    if pkg.is_empty() {
        return name.to_vec();
    }
    let mut out = pkg.to_vec();
    out.extend(u("::"));
    out.extend_from_slice(name);
    out
}

/// `PFunction.mangledName`: the wire name, `f_Integer_1__String_MANY__Integer_1_`; else the message of the
/// `UnsupportedOperationException` Java's mangle throws (a type that is neither a name nor a generic, a multiplicity
/// parameter), the first in Java's order.
pub fn mangled_name(f: &PFunction) -> Result<Vec<u16>, String> {
    let mut m = f.name.0.clone();
    m.push(u16::from(b'_'));
    for (i, pd) in f.parameters.iter().enumerate() {
        if i > 0 {
            m.extend(u("__"));
        }
        m.extend(mangle_type(&pd.r#type)?);
        m.push(u16::from(b'_'));
        m.extend(mangle_mult(&pd.multiplicity)?);
    }
    m.extend(u(if f.parameters.is_empty() { "_" } else { "__" }));
    m.extend(mangle_type(&f.return_type)?);
    m.push(u16::from(b'_'));
    m.extend(mangle_mult(&f.return_multiplicity)?);
    m.push(u16::from(b'_'));
    Ok(m)
}

fn mangle_type(t: &TypeExpression) -> Result<Vec<u16>, String> {
    let full = match t {
        TypeExpression::TypeExpressionNameRef(n) => &n.name.0,
        TypeExpression::TypeExpressionGeneric(g) => &g.name.0,
        _ => {
            return Err(format!(
                "no mangle rule for parameter type {}",
                type_expression_simple_name(t)
            ));
        }
    };
    Ok(match last_index_of_str(full, "::") {
        None => full.clone(),
        Some(i) => full[i + 2..].to_vec(),
    })
}

fn mangle_mult(m: &Multiplicity) -> Result<Vec<u16>, String> {
    let Multiplicity::MultiplicityConcrete(c) = m else {
        return Err("no mangle rule for a multiplicity parameter".to_owned());
    };
    let lo = c.lower_bound.to_string();
    Ok(u(&match c.upper_bound {
        None if c.lower_bound == 0 => "MANY".to_owned(),
        None => format!("${lo}_MANY$"),
        Some(up) if up == c.lower_bound => lo,
        Some(up) => format!("${lo}_{up}$"),
    }))
}

/// A type expression's Java class's `getSimpleName()`.
pub fn type_expression_simple_name(t: &TypeExpression) -> &'static str {
    match t {
        TypeExpression::TypeExpressionNameRef(_) => "NameRef",
        TypeExpression::TypeExpressionGeneric(_) => "Generic",
        TypeExpression::TypeExpressionFunctionType(_) => "FunctionType",
        TypeExpression::TypeExpressionRelationType(_) => "RelationType",
        TypeExpression::TypeExpressionSchemaAlgebra(_) => "SchemaAlgebra",
    }
}

/// `Protocol.functionPath`: a function's name with its signature; its qualified name where the signature has no
/// engine name.
pub fn function_path(f: &PFunction) -> Vec<u16> {
    match mangled_name(f) {
        Err(_) => qualified_name(&f.pkg.0, &f.name.0),
        Ok(name) => qualified_name(&f.pkg.0, &name),
    }
}

/// `Realization.of(body)`: a body that is one element pointer binds a function; any other is inline.
pub fn realization_of(body: Vec<ValueSpecification>) -> Realization {
    if body.len() == 1
        && let VS::PackageableElementPtr(ptr) = &body[0]
    {
        let function_fqn = ptr.full_path.clone();
        let source = body.into_iter().next();
        return Realization::RealizationRef(Box::new(RealizationRef { function_fqn, source }));
    }
    Realization::RealizationInline(Box::new(RealizationInline { body }))
}

/// `ConstraintDefinition.mentionsReturn`: an inline body that reads `$return` anywhere among its children
/// (legend-pure's rule for a post-condition).
pub fn mentions_return(c: &ConstraintDefinition) -> bool {
    match &c.realization {
        Realization::RealizationInline(inl) => inl.body.iter().any(reads_return),
        Realization::RealizationRef(_) => false,
    }
}

/// Whether `v` or a node under it (`ValueSpecification.children`, transitively) is the variable `return`.
fn reads_return(v: &ValueSpecification) -> bool {
    match v {
        VS::Variable(var) => var.name == "return",
        VS::PathLiteral(pl) => pl.desugared.body.iter().any(reads_return),
        VS::GraphFetchLiteral(gf) => {
            nodes_read_return(&gf.sub_trees) || gf.sub_type_trees.iter().any(|st| nodes_read_return(&st.sub_trees))
        }
        VS::QuotedTreeCall(q) => q.original.parameters.iter().any(reads_return) || reads_return(&q.tree),
        VS::QuotedGrammarCall(q) => {
            q.original.parameters.iter().any(reads_return)
                || q.functions.iter().any(|f| f.body.iter().any(reads_return))
        }
        VS::TdsLiteral(tl) => tl.desugared.parameters.iter().any(reads_return),
        VS::AppliedFunction(af) => af.parameters.iter().any(reads_return),
        VS::AppliedProperty(ap) => reads_return(&ap.receiver),
        VS::LambdaFunction(lf) => lf.body.iter().any(reads_return),
        VS::NewInstance(ni) => ni.properties.iter().any(|kb| reads_return(&kb.expression.value)),
        VS::NewInstanceCast(nc) => reads_return(&nc.src),
        VS::PureCollection(pc) => pc.values.iter().any(reads_return),
        VS::ColumnInstance(ColumnInstance::ColSpecArray(ca)) => ca.col_specs.iter().any(|cs| col_spec_reads_return(cs)),
        VS::ColumnInstance(ColumnInstance::ColSpec(cs)) => col_spec_reads_return(cs),
        VS::CBoolean(_)
        | VS::CByteArray(_)
        | VS::CDate(_)
        | VS::CDecimal(_)
        | VS::CFloat(_)
        | VS::CInteger(_)
        | VS::CLatestDate(_)
        | VS::CString(_)
        | VS::CTime(_)
        | VS::EnumValue(_)
        | VS::PackageableElementPtr(_)
        | VS::TypeAnnotation(_)
        | VS::SqlIsland(_)
        | VS::GqlIsland(_) => false,
    }
}

fn col_spec_reads_return(cs: &crate::records::ColSpec) -> bool {
    cs.function1.as_ref().is_some_and(|f| f.body.iter().any(reads_return))
        || cs.function2.as_ref().is_some_and(|f| f.body.iter().any(reads_return))
        || cs.args.iter().any(reads_return)
}

fn nodes_read_return(nodes: &[Box<GraphFetchLiteralNode>]) -> bool {
    nodes
        .iter()
        .any(|n| n.parameters.iter().any(reads_return) || nodes_read_return(&n.sub_trees))
}

/// The first part of a graph-fetch argument legend-engine's grammar has no rule for, refused where the engine refuses
/// it; `None` for a literal, an enumeration value, a variable or a collection of them.
pub(crate) fn graph_fetch_argument_refused(p: &ValueSpecification, first_argument: (i32, i32)) -> Option<JavaError> {
    match p {
        VS::CDate(_)
        | VS::EnumValue(_)
        | VS::CString(_)
        | VS::CInteger(_)
        | VS::CBoolean(_)
        | VS::CFloat(_)
        | VS::Variable(_) => None,
        VS::PureCollection(c) => c
            .values
            .iter()
            .find_map(|v| graph_fetch_argument_refused(v, first_argument)),
        other => Some(graph_fetch_argument_refusal(other, first_argument)),
    }
}

const GRAPH_FETCH_ARGUMENT: &str =
    "a graph-fetch argument must be a literal, an enumeration value, a variable or a collection of them";

/// An argument the engine's grammar has no rule for (PARITY row 63), refused where the engine refuses it: a `-` or a
/// decimal its walker fails on, so its section catch-all answers; a prefix form at its first token; anything else at
/// the first token that cannot continue the literal, name or variable it starts with.
pub(crate) fn graph_fetch_argument_refusal(v: &ValueSpecification, first_argument: (i32, i32)) -> JavaError {
    use ValueSpecification as V;
    let message = JStr::from(GRAPH_FETCH_ARGUMENT);
    let start = node_span(v).map_or(first_argument, |at| {
        // a column specification's span starts past its `~`
        let tilde = i32::from(matches!(v, V::ColumnInstance(_)));
        (at.start_line, at.start_column - tilde)
    });
    let refused_at = |(line, column): (i32, i32)| JavaError::parse(message.clone(), line, column).as_refusal();
    match v {
        V::CDecimal(_) => JavaError::refused_at_section(message, start),
        // a sign before a literal; `2015-10-16` is a `minus` over a collection
        V::AppliedFunction(f)
            if f.function == "minus" && f.parameters.len() == 1 && !matches!(f.parameters[0], V::PureCollection(_)) =>
        {
            JavaError::refused_at_section(message, start)
        }
        V::AppliedFunction(f) if f.function == "not" || f.function == "new" => refused_at(start),
        V::AppliedFunction(_) | V::AppliedProperty(_) | V::PackageableElementPtr(_) | V::EnumValue(_) => {
            refused_at(first_primary_end(v).map_or(start, |(line, column)| (line, column + 1)))
        }
        _ => refused_at(start),
    }
}

/// Where the first primary of an expression ends: a left operand's, a receiver's, a call's name.
fn first_primary_end(v: &ValueSpecification) -> Option<(i32, i32)> {
    use ValueSpecification as V;
    match v {
        V::AppliedFunction(f) => {
            let at = f.pos.as_deref()?;
            // an operator's or an arrow's first operand starts no later than the span of its name
            match f.parameters.first().filter(|p| first_starts_by(p, at)) {
                Some(first) => first_primary_end(first),
                None => Some((at.start_line, at.start_column + f.function.0.len() as i32 - 1)),
            }
        }
        V::PureCollection(c) => first_primary_end(c.values.first()?),
        V::AppliedProperty(p) => first_primary_end(&p.receiver),
        other => node_span(other).map(|at| (at.end_line, at.end_column)),
    }
}

/// Whether an expression starts no later than `at`: a collection carrier by its first element.
fn first_starts_by(v: &ValueSpecification, at: &SourceInfo) -> bool {
    use ValueSpecification as V;
    let span = match v {
        V::PureCollection(c) => c.values.first().and_then(node_span),
        other => node_span(other),
    };
    span.is_some_and(|s| (s.start_line, s.start_column) <= (at.start_line, at.start_column))
}

/// A node's span.
fn node_span(v: &ValueSpecification) -> Option<&SourceInfo> {
    use ValueSpecification as V;
    match v {
        V::AppliedFunction(n) => n.pos.as_deref(),
        V::AppliedProperty(n) => n.pos.as_deref(),
        V::CBoolean(n) => n.pos.as_deref(),
        V::CDate(n) => n.pos.as_deref(),
        V::CDecimal(n) => n.pos.as_deref(),
        V::CFloat(n) => n.pos.as_deref(),
        V::CInteger(n) => n.pos.as_deref(),
        V::CString(n) => n.pos.as_deref(),
        V::CTime(n) => n.pos.as_deref(),
        V::EnumValue(n) => n.pos.as_deref(),
        V::LambdaFunction(n) => n.pos.as_deref(),
        V::PackageableElementPtr(n) => n.pos.as_deref(),
        V::PathLiteral(n) => n.pos.as_deref(),
        V::PureCollection(n) => n.pos.as_deref(),
        V::Variable(n) => n.pos.as_deref(),
        V::ColumnInstance(ColumnInstance::ColSpec(n)) => n.pos.as_deref(),
        V::ColumnInstance(ColumnInstance::ColSpecArray(n)) => n.pos.as_deref(),
        V::TypeAnnotation(TypeAnnotation::TypeAnnotationNamed(n)) => n.pos.as_deref(),
        V::TypeAnnotation(TypeAnnotation::TypeAnnotationRelationShape(n)) => n.pos.as_deref(),
        _ => None,
    }
}

/// `(line, column)` of island index `i`.
pub(crate) fn island_position(pl: &PathLiteral, lit: &SourceInfo, i: i32) -> (i32, i32) {
    island_place(&pl.line_starts, pl.literal_length, lit, i)
}

/// [`island_position`] of a literal `literal_length` long, its later lines starting at `starts`.
pub(crate) fn island_place(starts: &[i32], literal_length: i32, lit: &SourceInfo, i: i32) -> (i32, i32) {
    let offset = i + 1;
    let mut line = 0usize;
    while line < starts.len() && starts[line] <= offset {
        line += 1;
    }
    if line == 0 {
        (lit.start_line, lit.start_column + literal_length + i)
    } else {
        (lit.start_line + line as i32, offset - starts[line - 1] + 1)
    }
}
