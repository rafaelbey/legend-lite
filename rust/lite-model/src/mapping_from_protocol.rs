// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! `MappingFromProtocol`: `PMapping` to the model -- a legacy DSL body to the surface tree the normalizer rewrites
//! (`LegacyMappingDefinition`), a clean-sheet body to `CleanSheetMappingDefinition`; never both in one mapping. The
//! property-mapping dispatch splits the wire's one operation per line by its shape, arm for arm as Java does; what is
//! not ported refuses loudly (`Thrown::Mapping`), and a class mapping whose database the source never names (the
//! pre-`~mainTable` shape, `MissingDatabase`) is skipped, as Java skips it.

use std::sync::Arc;

use lite_front::jtypes::{JStr, JValue};
use lite_front::records as p;

use crate::error::{Thrown, illegal_argument, illegal_state, null_pointer};
use crate::linked::{LinkedMap, list};
use crate::names::{is_default_set_id, join_type_from_identifier};
use crate::records::*;
use crate::rel_op_from_protocol as relop;
use crate::text::{is, last_index_of};

fn mapping_shape(message: JStr) -> Thrown {
    Thrown::Mapping(message)
}

/// `toMappingElement`: all legacy or all clean sheet; a mix refused.
pub fn to_mapping_element(m: &p::PMapping) -> Result<PackageableElement, Thrown> {
    let qn = JStr(lite_front::protocol::qualified_name(&m.pkg.0, &m.name.0));
    let has_clean_sheet = m
        .class_mappings
        .iter()
        .any(|cm| matches!(cm, p::PClassMapping::PClassMappingFunction(_)))
        || m.association_mappings
            .iter()
            .any(|am| matches!(am, p::PAssociationMapping::PFunctionAssociationMapping(_)));
    let has_legacy = m
        .class_mappings
        .iter()
        .any(|cm| !matches!(cm, p::PClassMapping::PClassMappingFunction(_)))
        || m.association_mappings
            .iter()
            .any(|am| !matches!(am, p::PAssociationMapping::PFunctionAssociationMapping(_)));
    if has_legacy && has_clean_sheet {
        return Err(mapping_shape(crate::cat!(
            "Mapping '",
            qn,
            "' mixes legacy DSL bodies with function-form bindings; a mapping must be all-legacy or all-clean-sheet",
            " (convert the whole mapping)"
        )));
    }
    Ok(if has_clean_sheet {
        PackageableElement::CleanSheetMappingDefinition(Arc::new(to_clean_sheet_definition(m, qn)?))
    } else {
        PackageableElement::LegacyMappingDefinition(Arc::new(to_mapping_definition(m, qn)?))
    })
}

fn to_clean_sheet_definition(m: &p::PMapping, qn: JStr) -> Result<CleanSheetMappingDefinition, Thrown> {
    let mut bindings = Vec::with_capacity(m.class_mappings.len());
    for cm in &m.class_mappings {
        let p::PClassMapping::PClassMappingFunction(f) = cm else {
            unreachable!("a clean-sheet mapping holds function-form bindings alone (to_mapping_element)");
        };
        bindings.push(Arc::new(CleanSheetMappingDefinitionClassBinding {
            class_fqn: f.class_name.clone(),
            kind: if is(&f.kind.0, "PURE") {
                CleanSheetMappingDefinitionKind::PURE
            } else {
                CleanSheetMappingDefinitionKind::RELATIONAL
            },
            set_id: f.id.clone(),
            extends_set_id: f.extends_class_mapping_id.clone(),
            root: f.root,
            realization: realization_of(f.function.as_ref(), f.body_lambda.as_ref())?,
            primary_key_columns: list(Vec::new()),
        }));
    }
    let mut enums = Vec::with_capacity(m.enumeration_mappings.len());
    for em in &m.enumeration_mappings {
        enums.push(Arc::new(enumeration_mapping(em)?));
    }
    let mut assoc_bindings = Vec::with_capacity(m.association_mappings.len());
    for am in &m.association_mappings {
        let p::PAssociationMapping::PFunctionAssociationMapping(fa) = am else {
            unreachable!("a clean-sheet mapping holds function-form bindings alone (to_mapping_element)");
        };
        assoc_bindings.push(Arc::new(CleanSheetMappingDefinitionAssociationBinding {
            association_fqn: fa.association.path.clone(),
            realization: realization_of(fa.function.as_ref(), fa.body_lambda.as_ref())?,
        }));
    }
    Ok(CleanSheetMappingDefinition {
        qualified_name: qn,
        includes: includes_of(m)?,
        class_bindings: list(bindings),
        association_bindings: list(assoc_bindings),
        enumeration_mappings: list(enums),
    })
}

/// The wire's mutually exclusive pair as the model's `Realization`: a pointer binds a function, else the lambda's
/// body is inline.
fn realization_of(
    f: Option<&Arc<p::PackageableElementPtr>>,
    lambda: Option<&Arc<p::LambdaFunction>>,
) -> Result<p::Realization, Thrown> {
    Ok(match f {
        Some(f) => p::Realization::RealizationRef(Box::new(p::RealizationRef {
            function_fqn: f.full_path.clone(),
            source: Some(p::ValueSpecification::PackageableElementPtr(f.clone())),
        })),
        None => {
            let lambda = lambda.ok_or_else(|| null_pointer(None))?;
            p::Realization::RealizationInline(Box::new(p::RealizationInline {
                body: lambda.body.to_vec(),
            }))
        }
    })
}

/// The includes, a data space's skipped (its compile semantic needs data space resolution); every substitution pair
/// kept.
fn includes_of(m: &p::PMapping) -> Result<Arc<[Arc<MappingInclude>]>, Thrown> {
    let mut includes = Vec::new();
    for inc in &m.included_mappings {
        if inc.included_data_space.is_some() {
            continue;
        }
        let subs = inc
            .substitutions
            .iter()
            .map(|s| {
                Arc::new(MappingIncludeStoreSubstitution {
                    original_store: s.source_database_path.clone(),
                    replacement_store: s.target_database_path.clone(),
                })
            })
            .collect();
        let Some(path) = inc.included_mapping.clone() else {
            return Err(null_pointer(None));
        };
        includes.push(Arc::new(MappingInclude {
            mapping_path: path,
            substitutions: list(subs),
        }));
    }
    Ok(list(includes))
}

/// `toMappingDefinition`: the legacy surface tree.
pub fn to_mapping_definition(m: &p::PMapping, qn: JStr) -> Result<LegacyMappingDefinition, Thrown> {
    let includes = includes_of(m)?;
    let mut class_mappings = Vec::with_capacity(m.class_mappings.len());
    for cm in &m.class_mappings {
        if let Some(built) = class_mapping(cm)? {
            class_mappings.push(built);
        }
    }
    let mut associations = Vec::with_capacity(m.association_mappings.len());
    for am in &m.association_mappings {
        associations.push(association_mapping(am)?);
    }
    let mut enums = Vec::with_capacity(m.enumeration_mappings.len());
    for em in &m.enumeration_mappings {
        enums.push(Arc::new(enumeration_mapping(em)?));
    }
    Ok(LegacyMappingDefinition {
        qualified_name: qn,
        includes,
        class_mappings: list(class_mappings),
        association_mappings: list(associations),
        enumeration_mappings: list(enums),
    })
}

/// A class mapping's model form, `None` where it is deliberately not modelled (a foreign store's, a merge, an
/// operation that is neither inheritance nor a union, a relational one with no database).
fn class_mapping(cm: &p::PClassMapping) -> Result<Option<ClassMapping>, Thrown> {
    match cm {
        p::PClassMapping::PServiceStoreClassMapping(_) | p::PClassMapping::PClassMappingMongoDb(_) => Ok(None),
        p::PClassMapping::PClassMappingRel(rel) => relational(rel),
        p::PClassMapping::PClassMappingFunction(_) => Err(mapping_shape(JStr::from(
            "a function-form binding has no legacy surface-tree shape; it belongs to MappingDefinition (use toMappingElement)",
        ))),
        p::PClassMapping::PClassMappingPure(pure) => Ok(Some(pure_instance(pure, pure.id.clone())?)),
        p::PClassMapping::PClassMappingOperation(op) => operation(op),
        p::PClassMapping::PClassMappingRelation(rel) => Ok(Some(relation_function(rel)?)),
        p::PClassMapping::PClassMappingAggregationAware(agg) => aggregation_aware(agg),
        p::PClassMapping::PClassMappingMergeOperation(_) => Ok(None),
    }
}

/// The AggregationAware element is one node: the main mapping (the class's root set) carrying the aggregate views; a
/// written id that is exactly the engine's derivation from the class is an absence.
fn aggregation_aware(agg: &p::PClassMappingAggregationAware) -> Result<Option<ClassMapping>, Thrown> {
    let agg_id = if is_default_set_id(&agg.id, &agg.class_name) {
        None
    } else {
        Some(agg.id.clone())
    };
    let main = class_mapping(&agg.main_set_implementation)?;
    if let Some(ClassMapping::ClassMappingRelational(r)) = &main {
        let mut views = Vec::with_capacity(agg.aggregate_set_implementations.len());
        for asi in &agg.aggregate_set_implementations {
            let vcm = class_mapping(&asi.set_implementation)?;
            let Some(ClassMapping::ClassMappingRelational(v)) = vcm else {
                return Err(mapping_shape(crate::cat!(
                    "AggregationAware view #",
                    asi.index.to_string(),
                    " of ",
                    agg.class_name,
                    " is not a Relational mapping"
                )));
            };
            let avs = asi
                .aggregate_values
                .iter()
                .map(|av| {
                    Arc::new(ClassMappingAggregateValue {
                        map_fn: av.map_fn.clone(),
                        aggregate_fn: av.aggregate_fn.clone(),
                    })
                })
                .collect();
            views.push(Arc::new(ClassMappingAggregateView {
                index: asi.index,
                can_aggregate: asi.can_aggregate,
                group_by_functions: list(asi.group_by_functions.clone()),
                aggregate_values: list(avs),
                set: Arc::new(ClassMappingRelational {
                    class_name: v.class_name.clone(),
                    set_id: v.set_id.clone(),
                    extends_set_id: v.extends_set_id.clone(),
                    root: false,
                    main_table: v.main_table.clone(),
                    filter: v.filter.clone(),
                    distinct: v.distinct,
                    group_by: v.group_by.clone(),
                    primary_key: v.primary_key.clone(),
                    property_mappings: v.property_mappings.clone(),
                    source_url: v.source_url.clone(),
                    property_target_sets: v.property_target_sets.clone(),
                    aggregation: None,
                }),
            }));
        }
        return Ok(Some(ClassMapping::ClassMappingRelational(Arc::new(
            ClassMappingRelational {
                class_name: r.class_name.clone(),
                set_id: agg_id,
                extends_set_id: r.extends_set_id.clone(),
                root: agg.root,
                main_table: r.main_table.clone(),
                filter: r.filter.clone(),
                distinct: r.distinct,
                group_by: r.group_by.clone(),
                primary_key: r.primary_key.clone(),
                property_mappings: r.property_mappings.clone(),
                source_url: r.source_url.clone(),
                property_target_sets: r.property_target_sets.clone(),
                aggregation: Some(Arc::new(ClassMappingAggregationAware { views: list(views) })),
            },
        ))));
    }
    if let p::PClassMapping::PClassMappingPure(pm) = &agg.main_set_implementation {
        // a Pure main serves as it is, with the outer element's set id
        return Ok(Some(pure_instance(pm, agg_id)?));
    }
    if main.is_none() {
        return Err(mapping_shape(JStr::from(
            "AggregationAware mapping has no ~mainMapping",
        )));
    }
    Ok(main)
}

const OP_INHERITANCE: &str = "INHERITANCE";
const OP_STORE_UNION: &str = "STORE_UNION";
const OP_ROUTER_UNION: &str = "ROUTER_UNION";

fn operation(op: &p::PClassMappingOperation) -> Result<Option<ClassMapping>, Thrown> {
    let f: &[u16] = op.operation.as_ref().map_or(&[], |s| &s.0);
    if is(f, OP_INHERITANCE) {
        return Ok(Some(ClassMapping::ClassMappingInheritance(Arc::new(
            ClassMappingInheritance {
                class_name: op.class_name.clone(),
                set_id: op.id.clone(),
                extends_set_id: op.extends_class_mapping_id.clone(),
                root: op.root,
            },
        ))));
    }
    if !is(f, OP_STORE_UNION) && !is(f, OP_ROUTER_UNION) {
        return Ok(None);
    }
    if op.parameters.is_empty() {
        return Err(mapping_shape(crate::cat!(
            "Operation union for '",
            op.class_name,
            "' names no member sets"
        )));
    }
    Ok(Some(ClassMapping::ClassMappingUnion(Arc::new(ClassMappingUnion {
        class_name: op.class_name.clone(),
        set_id: op.id.clone(),
        extends_set_id: op.extends_class_mapping_id.clone(),
        root: op.root,
        member_set_ids: list(op.parameters.clone()),
    }))))
}

/// A Pure instance mapping; `owner_id` is the set id the model carries, which a route echoing it is not.
fn pure_instance(pure: &p::PClassMappingPure, owner_id: Option<JStr>) -> Result<ClassMapping, Thrown> {
    let mut bindings = Vec::with_capacity(pure.property_mappings.len());
    for pm in &pure.property_mappings {
        let source = if pm.source == owner_id { None } else { pm.source.clone() };
        bindings.push(Arc::new(ClassMappingPurePropertyBinding {
            property_name: pm.property.clone(),
            expression: single(&pm.transform, "pure property transform")?,
            source_set_id: blank_to_null(source),
            target_set_id: blank_to_null(pm.target.clone()),
            explode: pm.explode_property,
            local: pm.local_mapping_property.is_some(),
            enum_mapping_id: pm.enum_mapping_id.clone(),
        }));
    }
    let filter = match &pure.filter {
        None => None,
        Some(f) => Some(single(f, "pure class-mapping filter")?),
    };
    if pure.src_class.is_none() && filter.is_some() {
        return Err(illegal_argument(JStr::from(
            "a ~filter needs a ~src: the filter reads $src, so the mapping must declare its type",
        )));
    }
    Ok(ClassMapping::ClassMappingPure(Arc::new(ClassMappingPure {
        class_name: pure.class_name.clone(),
        set_id: owner_id,
        extends_set_id: pure.extends_class_mapping_id.clone(),
        root: pure.root,
        source_class: pure.src_class.clone(),
        filter,
        property_bindings: list(bindings),
    })))
}

fn relation_function(rel: &p::PClassMappingRelation) -> Result<ClassMapping, Thrown> {
    let mut cols = Vec::with_capacity(rel.property_mappings.len());
    for pm in &rel.property_mappings {
        cols.push(relation_col(pm));
    }
    let func_ref = match &rel.relation_function {
        Some(f) => Some(f.clone()),
        None => rel
            .source_lambda
            .as_ref()
            .ok_or_else(|| null_pointer(None))?
            .function
            .clone(),
    };
    let Some(func_ref) = func_ref else {
        // ~src <expression>: carried inline
        let expr = rel
            .source_lambda
            .as_ref()
            .ok_or_else(|| null_pointer(None))?
            .expr
            .clone()
            .ok_or_else(|| null_pointer(Some("a Relation mapping source needs a function or an expression")))?;
        return relation_mapping(rel, None, Some(expr), cols);
    };
    let sig = func_ref.0.iter().position(|&c| c == u16::from(b'('));
    if let Some(sig) = sig {
        refuse_descriptor_types(
            &func_ref,
            &func_ref.0[sig..],
            rel.relation_function_source_information.as_deref(),
        )?;
    }
    let name = match sig {
        None => func_ref,
        Some(sig) => JStr(func_ref.0[..sig].to_vec()),
    };
    relation_mapping(rel, Some(name), None, cols)
}

/// `new ClassMapping.RelationFunction(...)`: exactly one of a function and an inline source.
fn relation_mapping(
    rel: &p::PClassMappingRelation,
    func_ref: Option<JStr>,
    inline_source: Option<p::ValueSpecification>,
    cols: Vec<Arc<ClassMappingRelationFunctionCol>>,
) -> Result<ClassMapping, Thrown> {
    if func_ref.is_none() == inline_source.is_none() {
        return Err(illegal_state(JStr::from(
            "a Relation mapping needs exactly one of funcRef/inlineSource",
        )));
    }
    Ok(ClassMapping::ClassMappingRelationFunction(Arc::new(
        ClassMappingRelationFunction {
            class_name: rel.class_name.clone(),
            set_id: rel.id.clone(),
            extends_set_id: None,
            root: rel.root,
            func_ref,
            inline_source,
            columns: list(cols),
            primary_key: list(rel.primary_key.clone()),
        },
    )))
}

/// The engine's rule for a function descriptor's types (`FunctionDescriptor.validateType`): each parameter type and
/// the return type a simple name, never a package path and never a function type; a type's arguments not read.
pub fn refuse_descriptor_types(descriptor: &JStr, signature: &[u16], at: Option<&p::SourceInfo>) -> Result<(), Thrown> {
    // white space removed outside quotes, as the engine's lexer drops it
    let mut compact: Vec<u16> = Vec::with_capacity(signature.len());
    let mut quoted = false;
    for &c in signature {
        if c == u16::from(b'\'') {
            quoted = !quoted;
        }
        if quoted || !lite_javacompat::character::is_whitespace(c) {
            compact.push(c);
        }
    }
    let s = compact;
    let mut types: Vec<&[u16]> = Vec::new();
    let mut depth = 0i32;
    let mut start = 1usize;
    let mut i = 1usize;
    let ch = |k: usize| s[k];
    while i < s.len() {
        let c = ch(i);
        if c == u16::from(b'-') && i + 1 < s.len() && ch(i + 1) == u16::from(b'>') {
            i += 2;
            continue;
        }
        if c == u16::from(b'(') || c == u16::from(b'<') || c == u16::from(b'{') || c == u16::from(b'[') {
            depth += 1;
        } else if c == u16::from(b'>')
            || c == u16::from(b'}')
            || c == u16::from(b']')
            || (c == u16::from(b')') && depth > 0)
        {
            // a closing bracket, or a `)` inside one (Java's two arms, one effect)
            depth -= 1;
        } else if c == u16::from(b')') {
            break;
        } else if c == u16::from(b',') && depth == 0 {
            types.push(&s[start..i]);
            start = i + 1;
        }
        i += 1;
    }
    if i > start {
        types.push(&s[start..i]);
    }
    if i + 1 < s.len() && ch(i + 1) == u16::from(b':') {
        types.push(&s[i + 2..]);
    }
    for type_and_mult in types {
        let mut ty = java_strip(type_and_mult);
        let bracket = [u16::from(b'[')];
        if let Some(mult) = last_index_of(ty, &bracket)
            && mult > 0
            && ty.last() == Some(&u16::from(b']'))
        {
            ty = &ty[..mult];
        }
        let head = match ty.iter().position(|&c| c == u16::from(b'<')) {
            None => ty,
            Some(a) => &ty[..a],
        };
        let sep: Vec<u16> = "::".encode_utf16().collect();
        let problem = if head.first() == Some(&u16::from(b'{')) {
            Some("Unsupported type: ")
        } else if crate::text::index_of(head, &sep).is_some() {
            Some("Package path not allowed in types: ")
        } else {
            None
        };
        if let Some(problem) = problem {
            let place = match at {
                None => String::new(),
                Some(w) => format!(" at {}:{}", w.start_line, w.start_column),
            };
            return Err(mapping_shape(crate::cat!(
                "~func '",
                descriptor,
                "'",
                place,
                ": ",
                problem,
                ty,
                " (a function descriptor spells its types by simple name, as Legend Engine reads it)"
            )));
        }
    }
    Ok(())
}

/// `String.strip()`: the leading and trailing white space by `Character.isWhitespace`.
fn java_strip(s: &[u16]) -> &[u16] {
    let ws = |c: u16| lite_javacompat::character::is_whitespace(c);
    let mut start = 0;
    let mut end = s.len();
    while start < end && ws(s[start]) {
        start += 1;
    }
    while end > start && ws(s[end - 1]) {
        end -= 1;
    }
    &s[start..end]
}

fn relation_col(pm: &p::PRelationFnPropertyMapping) -> Arc<ClassMappingRelationFunctionCol> {
    let nested = match &pm.nested {
        None => Vec::new(),
        Some(n) => n.iter().map(|sub| relation_col(sub)).collect(),
    };
    Arc::new(ClassMappingRelationFunctionCol {
        property: pm.property.clone(),
        column: pm.column.clone(),
        local: pm.local_mapping_property.is_some(),
        enum_mapping_id: pm.enum_mapping_id.clone(),
        embedded: list(nested),
        inline_set_id: pm.inline_set_id.clone(),
        expr: pm.expr.clone(),
    })
}

fn association_mapping(am: &p::PAssociationMapping) -> Result<AssociationMapping, Thrown> {
    match am {
        p::PAssociationMapping::PRelAssociationMapping(rel) => {
            let mut pms = Vec::with_capacity(rel.property_mappings.len());
            for pm in &rel.property_mappings {
                // an association end names both sets or neither
                let both = pm.source.is_some() && pm.target.is_some();
                let src = if both { pm.source.clone() } else { None };
                let tgt = if both { pm.target.clone() } else { None };
                pms.push(Arc::new(AssociationPropertyMapping {
                    source_set_id: src,
                    target_set_id: tgt,
                    body: body_of(&pm.property, &pm.relational_operation, None, None, None)?,
                }));
            }
            Ok(AssociationMapping::AssociationMappingRelational(Arc::new(
                AssociationMappingRelational {
                    association_name: rel.association.path.clone(),
                    property_mappings: list(pms),
                },
            )))
        }
        p::PAssociationMapping::PXStoreAssociationMapping(xs) => {
            let mut pms = Vec::with_capacity(xs.property_mappings.len());
            for pm in &xs.property_mappings {
                pms.push(Arc::new(AssociationMappingCrossXStoreProperty {
                    property_name: pm.property.clone(),
                    source_set_id: blank_to_null(Some(pm.source.clone())),
                    target_set_id: blank_to_null(Some(pm.target.clone())),
                    expression: single(&pm.cross_expression, "xstore cross expression")?,
                }));
            }
            Ok(AssociationMapping::AssociationMappingCross(Arc::new(
                AssociationMappingCross {
                    association_name: xs.association.path.clone(),
                    property_mappings2: list(pms),
                },
            )))
        }
        p::PAssociationMapping::PModelJoinAssociationMapping(mj) => {
            let p::ValueSpecification::LambdaFunction(lf) = &mj.join_condition else {
                return Err(mapping_shape(JStr::from("ModelJoin condition is not a lambda")));
            };
            Ok(AssociationMapping::AssociationMappingModelJoin(Arc::new(
                AssociationMappingModelJoin {
                    association_name: mj.association.path.clone(),
                    lambda: lf.clone(),
                },
            )))
        }
        p::PAssociationMapping::PFunctionAssociationMapping(_) => Err(mapping_shape(JStr::from(
            "association mapping kind PFunctionAssociationMapping is not ported yet",
        ))),
    }
}

fn enumeration_mapping(em: &p::PEnumerationMapping) -> Result<EnumerationMapping, Thrown> {
    let mut values = Vec::with_capacity(em.enum_value_mappings.len());
    for v in &em.enum_value_mappings {
        let sources: Vec<EnumerationMappingSourceValue> = v.source_values.iter().map(|sv| source_value(sv)).collect();
        if sources.is_empty() {
            return Err(illegal_argument(JStr::from(
                "EnumValueMapping requires at least one source value",
            )));
        }
        values.push(Arc::new(EnumerationMappingEnumValueMapping {
            enum_value: v.enum_value.clone(),
            source_values: list(sources),
        }));
    }
    Ok(EnumerationMapping {
        enum_name: em.enumeration.path.clone(),
        mapping_id: em.id.clone(),
        value_mappings: list(values),
    })
}

fn source_value(sv: &p::PEnumSourceValue) -> EnumerationMappingSourceValue {
    if let Some(enumeration) = &sv.enumeration {
        return EnumerationMappingSourceValue::EnumerationMappingSourceValueEnumRef(Arc::new(
            EnumerationMappingSourceValueEnumRef {
                enum_path: enumeration.clone(),
                enum_value_name: string_value_of(&sv.value),
            },
        ));
    }
    if let Some(n) = long_value(&sv.value) {
        return EnumerationMappingSourceValue::EnumerationMappingSourceValueIntegerValue(Arc::new(
            EnumerationMappingSourceValueIntegerValue { value: n },
        ));
    }
    EnumerationMappingSourceValue::EnumerationMappingSourceValueStringValue(Arc::new(
        EnumerationMappingSourceValueStringValue {
            value: string_value_of(&sv.value),
        },
    ))
}

/// `String.valueOf(value)` of an open slot's leaf.
pub fn string_value_of(v: &JValue) -> JStr {
    match v {
        JValue::String(s) => s.clone(),
        JValue::Long(n) => JStr::from(n.to_string()),
        JValue::Integer(n) => JStr::from(n.to_string()),
        JValue::Boolean(b) => JStr::from(b.to_string()),
        JValue::Double(d) => JStr::from(lite_javacompat::double_to_string(*d)),
        JValue::BigInteger(s) => JStr::from(s.as_str()),
        JValue::BigDecimal(d) => JStr::from(d.to_java_string()),
    }
}

/// `((Number) value).longValue()` when the leaf is a number, `None` when it is not one.
fn long_value(v: &JValue) -> Option<i64> {
    match v {
        JValue::Long(n) => Some(*n),
        JValue::Integer(n) => Some(i64::from(*n)),
        // a double's longValue saturates and takes NaN to 0, as Rust's cast does
        JValue::Double(d) => Some(*d as i64),
        JValue::BigInteger(s) => Some(big_integer_low_bits(s)),
        JValue::BigDecimal(d) => Some(big_integer_low_bits(&integer_part(d))),
        JValue::String(_) | JValue::Boolean(_) => None,
    }
}

/// `BigDecimal.toBigInteger()`'s decimal: the unscaled digits with the fraction dropped (a negative scale's zeros
/// appended), signed.
fn integer_part(d: &lite_front::jtypes::BigDecimal) -> String {
    let digits = &d.digits;
    let whole = if d.scale >= 0 {
        let keep = digits.len().saturating_sub(d.scale as usize);
        if keep == 0 {
            "0".to_owned()
        } else {
            digits[..keep].to_owned()
        }
    } else {
        format!("{digits}{}", "0".repeat(d.scale.unsigned_abs() as usize))
    };
    if d.negative && whole != "0" {
        format!("-{whole}")
    } else {
        whole
    }
}

/// `BigInteger.longValue()`: the low 64 bits of the two's complement value of a signed decimal.
fn big_integer_low_bits(decimal: &str) -> i64 {
    let (negative, digits) = match decimal.strip_prefix('-') {
        Some(d) => (true, d),
        None => (false, decimal),
    };
    let mut acc: u64 = 0;
    for c in digits.bytes() {
        acc = acc.wrapping_mul(10).wrapping_add(u64::from(c - b'0'));
    }
    let v = acc as i64;
    if negative { v.wrapping_neg() } else { v }
}

/// The wire wraps a single expression in a list; the model holds the one.
fn single(vs: &[p::ValueSpecification], what: &str) -> Result<p::ValueSpecification, Thrown> {
    if vs.len() != 1 {
        return Err(mapping_shape(crate::cat!(
            "a ",
            what,
            " must have exactly one expression, got an empty body"
        )));
    }
    Ok(vs[0].clone())
}

fn relational(rel: &p::PClassMappingRel) -> Result<Option<ClassMapping>, Thrown> {
    match relational_inner(rel) {
        Err(Thrown::MissingDatabase(_)) => Ok(None),
        other => other.map(Some),
    }
}

fn relational_inner(rel: &p::PClassMappingRel) -> Result<ClassMapping, Thrown> {
    let main_table = match &rel.main_table {
        None => None,
        Some(t) => Some(Arc::new(LegacyMappingDefinitionTableReference {
            database: require(t.database.clone(), "main table database")?,
            table: table_name(t),
        })),
    };
    let main_db = main_table.as_ref().map(|t| t.database.clone());
    let filter = match &rel.filter {
        None => None,
        Some(f) => Some(filter_mapping(f)?),
    };
    let mut group_by = Vec::with_capacity(rel.group_by.len());
    for op in &rel.group_by {
        group_by.push(relop::op(op, None)?);
    }
    let mut primary_key = Vec::with_capacity(rel.primary_key.len());
    for op in &rel.primary_key {
        primary_key.push(relop::op(op, None)?);
    }
    let mut pms = Vec::with_capacity(rel.property_mappings.len());
    let mut target_sets: LinkedMap<JStr, JStr> = LinkedMap::new();
    for pm in &rel.property_mappings {
        pms.push(property_mapping(pm, main_db.as_ref(), &mut target_sets)?);
    }
    Ok(ClassMapping::ClassMappingRelational(Arc::new(ClassMappingRelational {
        class_name: rel.class_name.clone(),
        set_id: rel.id.clone(),
        extends_set_id: rel.extends_class_mapping_id.clone(),
        root: rel.root,
        main_table,
        filter,
        distinct: rel.distinct,
        group_by: list(group_by),
        primary_key: list(primary_key),
        property_mappings: list(pms),
        source_url: None,
        property_target_sets: target_sets.freeze(),
        aggregation: None,
    })))
}

/// `~filter [DB] F` and `~filter [DB] @J | [DB] F`: the leading join type hangs on the filter, a later one on its hop.
fn filter_mapping(f: &p::PFilterMapping) -> Result<FilterMapping, Thrown> {
    let ptr = FilterPointer::FilterPointerCross(Arc::new(FilterPointerCross {
        db: f.db.clone(),
        name: f.name.clone(),
    }));
    if f.joins.is_empty() {
        return Ok(FilterMapping::FilterMappingDirect(Arc::new(FilterMappingDirect {
            filter: ptr,
        })));
    }
    let hops = &f.joins;
    let filter_join_type = hops[0].join_type.clone();
    let mut chain = Vec::with_capacity(hops.len());
    for (i, hop) in hops.iter().enumerate() {
        let hop_type = if i == 0 { None } else { hop.join_type.as_ref() };
        chain.push(Arc::new(JoinChainElement {
            join_name: hop.name.clone(),
            join_type: match hop_type {
                None => None,
                Some(t) => Some(join_type_from_identifier(t)?),
            },
            database_name: hop.db.clone(),
            include_self: false,
        }));
    }
    let source = require(hops[0].db.clone(), "join-mediated filter source database")?;
    crate::from_protocol::join_mediated(Some(source), chain, ptr, filter_join_type)
}

/// The wire splits schema from table; the model carries the name as written (`schema.table` for a named schema),
/// quotes off.
fn table_name(t: &p::PTablePtr) -> JStr {
    let table = unquote(&t.table);
    if is(&t.schema.0, "default") {
        table
    } else {
        crate::cat!(unquote(&t.schema), ".", table)
    }
}

fn unquote(s: &JStr) -> JStr {
    let u = &s.0;
    if u.len() >= 2 && u[0] == u16::from(b'"') && u[u.len() - 1] == u16::from(b'"') {
        JStr(u[1..u.len() - 1].to_vec())
    } else {
        s.clone()
    }
}

/// The database a property line resolves in, read off the protocol node (the model elides a self-reference).
fn protocol_db(op: &p::PRelOp) -> Option<JStr> {
    match op {
        p::PRelOp::PColumnRef(c) => c.table.database.clone(),
        p::PRelOp::PElemtWithJoins(j) if !j.joins.is_empty() => j.joins[0].db.clone(),
        _ => None,
    }
}

fn property_mapping(
    pm: &p::PPropertyMapping,
    main_db: Option<&JStr>,
    target_sets: &mut LinkedMap<JStr, JStr>,
) -> Result<PropertyMapping, Thrown> {
    match pm {
        p::PPropertyMapping::PInlineEmbeddedPropertyMapping(inl) => Ok(PropertyMapping::PropertyMappingInlineEmbedded(
            Arc::new(PropertyMappingInlineEmbedded {
                property_name: inl.property.clone(),
                set_id: inl.set_implementation_id.clone(),
            }),
        )),
        p::PPropertyMapping::PEmbeddedPropertyMapping(emb) => {
            let mut subs = Vec::with_capacity(emb.property_mappings.len());
            for sub in &emb.property_mappings {
                subs.push(property_mapping(sub, main_db, target_sets)?);
            }
            let mut pk = Vec::with_capacity(emb.primary_key.len());
            for op in &emb.primary_key {
                pk.push(relop::op(op, None)?);
            }
            Ok(PropertyMapping::PropertyMappingEmbedded(Arc::new(
                PropertyMappingEmbedded {
                    property_name: emb.property.clone(),
                    property_mappings: list(subs),
                    primary_key: list(pk),
                },
            )))
        }
        p::PPropertyMapping::POtherwiseEmbeddedPropertyMapping(oth) => {
            let mut subs = Vec::with_capacity(oth.property_mappings.len());
            for sub in &oth.property_mappings {
                subs.push(property_mapping(sub, main_db, target_sets)?);
            }
            let fallback = body_of(&oth.property, &oth.otherwise_op, main_db, None, None)?;
            Ok(PropertyMapping::PropertyMappingOtherwiseEmbedded(Arc::new(
                PropertyMappingOtherwiseEmbedded {
                    property_name: oth.property.clone(),
                    embedded: list(subs),
                    fallback_set_id: oth.otherwise_target.clone(),
                    fallback,
                },
            )))
        }
        p::PPropertyMapping::PRelPropertyMapping(rel) => {
            if let Some(lp) = &rel.local_mapping_property {
                return Ok(PropertyMapping::PropertyMappingLocalProperty(Arc::new(
                    PropertyMappingLocalProperty {
                        property_name: rel.property.clone(),
                        r#type: p::TypeExpression::TypeExpressionNameRef(Box::new(p::TypeExpressionNameRef {
                            name: lp.r#type.clone(),
                            pos: None,
                        })),
                        multiplicity: p::Multiplicity::MultiplicityConcrete(Box::new(p::MultiplicityConcrete {
                            lower_bound: lp.lower_bound as i32,
                            upper_bound: lp.upper_bound.map(|u| u as i32),
                        })),
                        body: body_of(
                            &rel.property,
                            &rel.relational_operation,
                            main_db,
                            rel.enum_mapping_id.as_ref(),
                            rel.target.as_ref(),
                        )?,
                    },
                )));
            }
            if let Some(t) = &rel.target {
                target_sets.put(rel.property.clone(), t.clone());
            }
            body_of(
                &rel.property,
                &rel.relational_operation,
                main_db,
                rel.enum_mapping_id.as_ref(),
                rel.target.as_ref(),
            )
        }
    }
}

/// The dispatch: one wire operation to one of five model variants by its shape -- a join navigation to an instance or
/// a terminal column, a column (enumerated or not), any other expression (enumerated or not).
fn body_of(
    property: &JStr,
    raw_op: &p::PRelOp,
    main_db: Option<&JStr>,
    enum_mapping_id: Option<&JStr>,
    target_set_id: Option<&JStr>,
) -> Result<PropertyMapping, Thrown> {
    let db = protocol_db(raw_op).or_else(|| main_db.cloned());
    let op = relop::op(raw_op, db.as_ref())?;
    match &op {
        RelationalOperation::RelationalOperationJoinNavigation(jn) => {
            let chain_db = jn.chain.first().and_then(|c| c.database_name.clone());
            let nav_db = require(
                db.clone().or_else(|| jn.database_name.clone()).or(chain_db),
                "a join navigation requires a database — either a ~mainTable directive or an explicit [DB] qualifier",
            )?;
            match &jn.terminal {
                None => {
                    if enum_mapping_id.is_some() {
                        return Err(mapping_shape(JStr::from(
                            "EnumerationMapping on a join property mapping requires a terminal column",
                        )));
                    }
                    if jn.chain.is_empty() {
                        return Err(illegal_argument(JStr::from(
                            "Join property mapping must have at least one join hop",
                        )));
                    }
                    Ok(PropertyMapping::PropertyMappingJoin(Arc::new(PropertyMappingJoin {
                        property_name: property.clone(),
                        database: nav_db,
                        joins: jn.chain.clone(),
                        target_set_id: target_set_id.cloned(),
                    })))
                }
                Some(terminal) => {
                    if jn.chain.is_empty() {
                        return Err(illegal_argument(JStr::from(
                            "Join-with-terminal property mapping must have at least one join hop",
                        )));
                    }
                    Ok(PropertyMapping::PropertyMappingJoinTerminalColumn(Arc::new(
                        PropertyMappingJoinTerminalColumn {
                            property_name: property.clone(),
                            database: nav_db,
                            joins: jn.chain.clone(),
                            terminal_column: terminal.clone(),
                            enum_mapping_id: enum_mapping_id.cloned(),
                            enum_mapped: enum_mapping_id.is_some(),
                        },
                    )))
                }
            }
        }
        RelationalOperation::RelationalOperationColumnRef(cr) => {
            let col_db = require(
                db.clone(),
                "a column reference requires a database — either a ~mainTable directive or an explicit [DB] qualifier",
            )?;
            Ok(match enum_mapping_id {
                None => PropertyMapping::PropertyMappingColumn(Arc::new(PropertyMappingColumn {
                    property_name: property.clone(),
                    database: col_db,
                    table: cr.table.clone(),
                    column: cr.column.clone(),
                })),
                Some(e) => {
                    PropertyMapping::PropertyMappingEnumeratedColumn(Arc::new(PropertyMappingEnumeratedColumn {
                        property_name: property.clone(),
                        enum_mapping_id: Some(e.clone()),
                        database: col_db,
                        table: cr.table.clone(),
                        column: cr.column.clone(),
                    }))
                }
            })
        }
        _ => Ok(match enum_mapping_id {
            None => PropertyMapping::PropertyMappingExpression(Arc::new(PropertyMappingExpression {
                property_name: property.clone(),
                expression: op.clone(),
            })),
            Some(e) => {
                PropertyMapping::PropertyMappingEnumeratedExpression(Arc::new(PropertyMappingEnumeratedExpression {
                    property_name: property.clone(),
                    enum_mapping_id: Some(e.clone()),
                    expression: op.clone(),
                }))
            }
        }),
    }
}

fn blank_to_null(s: Option<JStr>) -> Option<JStr> {
    s.filter(|s| !s.is_empty())
}

/// Refusals name the missing source directive: a missing database is `MissingDatabase` (the class mapping skipped),
/// anything else an unsupported shape.
fn require(v: Option<JStr>, what: &str) -> Result<JStr, Thrown> {
    v.ok_or_else(|| {
        if what.contains("requires a database") {
            Thrown::MissingDatabase(JStr::from(what))
        } else {
            mapping_shape(JStr::from(what))
        }
    })
}
