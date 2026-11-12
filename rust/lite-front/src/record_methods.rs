// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! The protocol records' own methods that name resolution calls (2026-11-12, phase 3's names slice,
//! docs/rust-port/PHASE3.md): ported here, beside the records, because Java has them on the records
//! (`com.legend.protocol`'s `TypeExpression.rawClassName`, `Protocol.PStereotype.withProfile` and
//! `PTaggedValue.withProfile`, `KeyExpression.withValue`, `AppliedFunction.withParameters`, `PathLiteral`'s
//! `withChildren`, `GraphFetchLiteral.arguments`, `withArguments` and `withClassNames`). Each copies every component
//! it does not replace, as Java's does.

use std::sync::Arc;

use crate::jtypes::JStr;
use crate::records::{
    AppliedFunction, GraphFetchLiteral, GraphFetchLiteralNode, GraphFetchLiteralSubTypeNode, KeyExpression,
    LambdaFunction, PStereotype, PTag, PTaggedValue, PathLiteral, TypeExpression, ValueSpecification,
};

impl TypeExpression {
    /// `TypeExpression.rawClassName(t)`: a name's or a generic's head, `None` for any other form (never a refusal:
    /// `TypeClassifier.headFqn` is the reading that refuses).
    pub fn raw_class_name(&self) -> Option<&JStr> {
        match self {
            TypeExpression::TypeExpressionNameRef(n) => Some(&n.name),
            TypeExpression::TypeExpressionGeneric(g) => Some(&g.name),
            _ => None,
        }
    }
}

impl PStereotype {
    /// `withProfile(resolved)`: this stereotype with its profile replaced (the same value when it is unchanged).
    pub fn with_profile(&self, resolved: &JStr) -> PStereotype {
        PStereotype {
            profile: resolved.clone(),
            value: self.value.clone(),
            profile_source_information: self.profile_source_information.clone(),
            source_information: self.source_information.clone(),
        }
    }
}

impl PTaggedValue {
    /// `withProfile(resolved)`: this tagged value with its tag's profile replaced (the same value when unchanged).
    pub fn with_profile(&self, resolved: &JStr) -> PTaggedValue {
        PTaggedValue {
            tag: Box::new(PTag {
                profile: resolved.clone(),
                value: self.tag.value.clone(),
                profile_source_information: self.tag.profile_source_information.clone(),
                source_information: self.tag.source_information.clone(),
            }),
            value: self.value.clone(),
            multi_line: self.multi_line,
            source_information: self.source_information.clone(),
        }
    }
}

impl KeyExpression {
    /// `withValue(v)`.
    pub fn with_value(&self, value: ValueSpecification) -> KeyExpression {
        KeyExpression {
            value,
            is_add: self.is_add,
            is_local: self.is_local,
        }
    }
}

impl AppliedFunction {
    /// `withParameters(ps)`: every other component kept, the position and written details included.
    pub fn with_parameters(&self, parameters: Arc<[ValueSpecification]>) -> AppliedFunction {
        AppliedFunction {
            function: self.function.clone(),
            parameters,
            referents: self.referents.clone(),
            pos: self.pos.clone(),
            property_call: self.property_call,
            grouped: self.grouped,
            infix: self.infix,
            island: self.island,
            f_control: self.f_control.clone(),
            owner_class: self.owner_class.clone(),
        }
    }
}

impl PathLiteral {
    /// `withChildren([desugared])`: the literal with its desugared lambda replaced, every wire piece kept.
    pub fn with_desugared(&self, desugared: Arc<LambdaFunction>) -> PathLiteral {
        PathLiteral {
            start_type: self.start_type.clone(),
            segments: self.segments.clone(),
            desugared,
            alias: self.alias.clone(),
            has_dated_segment: self.has_dated_segment,
            pos: self.pos.clone(),
            literal_length: self.literal_length,
            line_starts: self.line_starts.clone(),
        }
    }
}

impl GraphFetchLiteral {
    /// `arguments()`: every node's parameters, the property trees' first, then each subtype tree's, depth first.
    pub fn arguments(&self) -> Vec<ValueSpecification> {
        let mut out = Vec::new();
        collect(&self.sub_trees, &mut out);
        for st in self.sub_type_trees.iter() {
            collect(&st.sub_trees, &mut out);
        }
        out
    }

    /// `withArguments(args)`: the tree with its nodes' parameters replaced, in [`arguments`](Self::arguments)' order;
    /// `Err` with Java's `IllegalArgumentException` message when the count differs.
    pub fn with_arguments(&self, args: &[ValueSpecification]) -> Result<GraphFetchLiteral, String> {
        let mut next = 0usize;
        let nodes = replace(&self.sub_trees, args, &mut next);
        let mut sub_types: Vec<Box<GraphFetchLiteralSubTypeNode>> = Vec::with_capacity(self.sub_type_trees.len());
        for st in self.sub_type_trees.iter() {
            sub_types.push(Box::new(GraphFetchLiteralSubTypeNode {
                sub_type_class: st.sub_type_class.clone(),
                pos: st.pos.clone(),
                sub_trees: replace(&st.sub_trees, args, &mut next),
            }));
        }
        if next != args.len() {
            return Err(format!(
                "a graph-fetch tree has {next} argument(s), given {}",
                args.len()
            ));
        }
        Ok(GraphFetchLiteral {
            class_name: self.class_name.clone(),
            sub_trees: nodes.into(),
            sub_type_trees: sub_types.into(),
            pos: self.pos.clone(),
        })
    }

    /// `withClassNames(rename)`: every class name renamed, in Java's order -- each subtype tree's class then its nodes'
    /// subtypes, then the root class, then the property trees' subtypes -- the first refusal returned.
    pub fn with_class_names<E>(
        &self,
        rename: &mut impl FnMut(&JStr) -> Result<JStr, E>,
    ) -> Result<GraphFetchLiteral, E> {
        let mut sub_types: Vec<Box<GraphFetchLiteralSubTypeNode>> = Vec::with_capacity(self.sub_type_trees.len());
        for st in self.sub_type_trees.iter() {
            let class = rename(&st.sub_type_class)?;
            sub_types.push(Box::new(GraphFetchLiteralSubTypeNode {
                sub_type_class: class,
                pos: st.pos.clone(),
                sub_trees: renamed(&st.sub_trees, rename)?,
            }));
        }
        let class_name = rename(&self.class_name)?;
        let sub_trees = renamed(&self.sub_trees, rename)?;
        Ok(GraphFetchLiteral {
            class_name,
            sub_trees: sub_trees.into(),
            sub_type_trees: sub_types.into(),
            pos: self.pos.clone(),
        })
    }
}

#[allow(clippy::vec_box)] // the record's own list type (generated: a node held as a Box)
fn renamed<E>(
    nodes: &[Box<GraphFetchLiteralNode>],
    rename: &mut impl FnMut(&JStr) -> Result<JStr, E>,
) -> Result<Vec<Box<GraphFetchLiteralNode>>, E> {
    let mut out = Vec::with_capacity(nodes.len());
    for n in nodes {
        let sub_type = match &n.sub_type {
            None => None,
            Some(s) => Some(rename(s)?),
        };
        out.push(Box::new(GraphFetchLiteralNode {
            property: n.property.clone(),
            pos: n.pos.clone(),
            parameters: n.parameters.clone(),
            alias: n.alias.clone(),
            sub_type,
            sub_trees: renamed(&n.sub_trees, rename)?,
        }));
    }
    Ok(out)
}

fn collect(nodes: &[Box<GraphFetchLiteralNode>], out: &mut Vec<ValueSpecification>) {
    for n in nodes {
        out.extend(n.parameters.iter().cloned());
        collect(&n.sub_trees, out);
    }
}

#[allow(clippy::vec_box)] // the record's own list type (generated: a node held as a Box)
fn replace(
    nodes: &[Box<GraphFetchLiteralNode>],
    args: &[ValueSpecification],
    next: &mut usize,
) -> Vec<Box<GraphFetchLiteralNode>> {
    let mut out = Vec::with_capacity(nodes.len());
    for n in nodes {
        let end = (*next + n.parameters.len()).min(args.len());
        let params = args[(*next).min(end)..end].to_vec();
        *next += n.parameters.len();
        out.push(Box::new(GraphFetchLiteralNode {
            property: n.property.clone(),
            pos: n.pos.clone(),
            parameters: params,
            alias: n.alias.clone(),
            sub_type: n.sub_type.clone(),
            sub_trees: replace(&n.sub_trees, args, next),
        }));
    }
    out
}
