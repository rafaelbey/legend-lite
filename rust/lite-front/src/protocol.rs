// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! The protocol records' own rules the parser calls (core/'s `protocol.Protocol`, `Realization`,
//! `ConstraintDefinition`, `ValueSpecification.children`): an FQN split into the wire's package and name, a function's
//! signature-mangled path, a derived body's realization, a constraint's `$return` test.

use crate::cursor::{R, unquote_path_iae};
use crate::records::{
    ColumnInstance, ConstraintDefinition, GraphFetchLiteralNode, Multiplicity, PFunction, Realization,
    RealizationInline, RealizationRef, TypeExpression, ValueSpecification,
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

/// `PFunction.mangledName`: the wire name, `f_Integer_1__String_MANY__Integer_1_`; `None` where Java's mangle throws
/// `UnsupportedOperationException` (a type that is neither a name nor a generic, a multiplicity parameter).
fn mangled_name(f: &PFunction) -> Option<Vec<u16>> {
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
    Some(m)
}

fn mangle_type(t: &TypeExpression) -> Option<Vec<u16>> {
    let full = match t {
        TypeExpression::TypeExpressionNameRef(n) => &n.name.0,
        TypeExpression::TypeExpressionGeneric(g) => &g.name.0,
        _ => return None,
    };
    Some(match last_index_of_str(full, "::") {
        None => full.clone(),
        Some(i) => full[i + 2..].to_vec(),
    })
}

fn mangle_mult(m: &Multiplicity) -> Option<Vec<u16>> {
    let Multiplicity::MultiplicityConcrete(c) = m else {
        return None;
    };
    let lo = c.lower_bound.to_string();
    Some(u(&match c.upper_bound {
        None if c.lower_bound == 0 => "MANY".to_owned(),
        None => format!("${lo}_MANY$"),
        Some(up) if up == c.lower_bound => lo,
        Some(up) => format!("${lo}_{up}$"),
    }))
}

/// `Protocol.functionPath`: a function's name with its signature; its qualified name where the signature has no
/// engine name.
pub fn function_path(f: &PFunction) -> Vec<u16> {
    match mangled_name(f) {
        None => qualified_name(&f.pkg.0, &f.name.0),
        Some(name) => qualified_name(&f.pkg.0, &name),
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
    nodes.iter().any(|n| n.parameters.iter().any(reads_return) || nodes_read_return(&n.sub_trees))
}
