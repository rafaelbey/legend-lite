// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! The resolver's walkers over the model's records (2026-11-12): every packageable element, its mappings, its
//! database, its runtime and service. `None` is Java's `==` input; a rebuilt record is built as Java builds it.

use std::sync::Arc;

use lite_base::IdMap;
use lite_model::linked::LinkedMap;
use lite_model::protocol as p;
use lite_model::*;

use super::values::{
    arc_list, constraint, derived_property, fqn_list, name, opt_name, or, realization, resolve_type, resolve_vs,
    stereotypes, tagged_values, type_list, vs_list,
};
use super::{Rs, Scope, resolve_name};
use crate::error::Thrown;

type View = Arc<DatabaseDefinitionViewDefinition>;

/// `resolveElement(el, scope)`: each kind by name (a new kind is a compile error here until it is handled).
pub(super) fn resolve_element(el: &PackageableElement, scope: &Scope<'_>) -> Rs<PackageableElement> {
    use PackageableElement as E;
    Ok(match el {
        // opaque overlay elements carry extension-owned payloads; a data element is data
        E::OpaqueElementDefinition(_) | E::DataDefinition(_) => None,
        E::PrimitiveExtensionDefinition(pe) => name(&pe.base_type_name, scope)?.map(|base| {
            // Java's two-argument PrimitiveExtensionDefinition: the type variables are not carried
            E::PrimitiveExtensionDefinition(Arc::new(PrimitiveExtensionDefinition {
                qualified_name: pe.qualified_name.clone(),
                base_type_name: base,
                type_variables: Arc::default(),
            }))
        }),
        E::ClassDefinition(cd) => class(cd, scope)?.map(E::ClassDefinition),
        E::AssociationDefinition(ad) => association(ad, scope)?.map(E::AssociationDefinition),
        E::FunctionDefinition(fd) => function(fd, scope)?.map(E::FunctionDefinition),
        E::NativeFunctionDefinition(nfd) => native_function(nfd, scope)?.map(E::NativeFunctionDefinition),
        E::LegacyMappingDefinition(md) => legacy_mapping(md, scope)?.map(E::LegacyMappingDefinition),
        E::DatabaseDefinition(db) => database(db, scope)?.map(E::DatabaseDefinition),
        E::RuntimeDefinition(rd) => runtime(rd, scope)?.map(E::RuntimeDefinition),
        E::ServiceDefinition(sd) => service(sd, scope)?.map(E::ServiceDefinition),
        E::ExecutionEnvironmentDefinition(ee) => keyed_executions(&ee.executions, scope)?.map(|ks| {
            E::ExecutionEnvironmentDefinition(Arc::new(ExecutionEnvironmentDefinition {
                qualified_name: ee.qualified_name.clone(),
                executions: ks,
            }))
        }),
        E::ConnectionDefinition(cd) => opt_name(&cd.store_name, scope)?.map(|store| {
            E::ConnectionDefinition(Arc::new(ConnectionDefinition {
                qualified_name: cd.qualified_name.clone(),
                store_name: store,
                database_type: cd.database_type,
                specification: cd.specification.clone(),
                authentication: cd.authentication.clone(),
            }))
        }),
        E::DataSpaceDefinition(ds) => data_space(ds, scope)?.map(E::DataSpaceDefinition),
        E::ModelConnectionDefinition(mc) => name(&mc.class_name, scope)?.map(|cls| {
            E::ModelConnectionDefinition(Arc::new(ModelConnectionDefinition {
                qualified_name: mc.qualified_name.clone(),
                kind: mc.kind,
                class_name: cls,
                url: mc.url.clone(),
            }))
        }),
        E::ModelChainConnectionDefinition(mcc) => {
            // every mapping resolved; rebuilt when any changed
            let mut resolved: Vec<JStr> = Vec::with_capacity(mcc.mappings.len());
            let mut changed = false;
            for m in mcc.mappings.iter() {
                let r = resolve_name(m, scope)?;
                changed |= &r != m;
                resolved.push(r);
            }
            changed.then(|| {
                E::ModelChainConnectionDefinition(Arc::new(ModelChainConnectionDefinition {
                    qualified_name: mcc.qualified_name.clone(),
                    mappings: linked::list(resolved),
                }))
            })
        }
        E::EnumDefinition(ed) => enumeration(ed, scope)?.map(E::EnumDefinition),
        // profiles name nothing; a measure's conversions are numeric expressions over their own parameter
        E::ProfileDefinition(_) | E::MeasureDefinition(_) => None,
        E::PersistenceDefinition(pe) => opt_name(&pe.service, scope)?.map(|svc| {
            E::PersistenceDefinition(Arc::new(PersistenceDefinition {
                qualified_name: pe.qualified_name.clone(),
                doc: pe.doc.clone(),
                trigger_source: pe.trigger_source.clone(),
                service: svc,
                persister_source: pe.persister_source.clone(),
                service_output_targets_source: pe.service_output_targets_source.clone(),
                notifier_source: pe.notifier_source.clone(),
                tests_source: pe.tests_source.clone(),
            }))
        }),
        E::PersistenceContextDefinition(pce) => name(&pce.persistence, scope)?.map(|pr| {
            E::PersistenceContextDefinition(Arc::new(PersistenceContextDefinition {
                qualified_name: pce.qualified_name.clone(),
                persistence: pr,
                platform_source: pce.platform_source.clone(),
                service_parameters_source: pce.service_parameters_source.clone(),
                sink_connection_source: pce.sink_connection_source.clone(),
            }))
        }),
        // activator fields carry function-pointer signatures; generic keyed elements carry values as written
        E::FunctionActivatorDefinition(_) | E::GenericSectionElementDefinition(_) => None,
        E::CleanSheetMappingDefinition(md) => clean_sheet_mapping(md, scope)?.map(E::CleanSheetMappingDefinition),
        // a COMPILED mapping never re-resolves
        E::MappingDefinition(_) => None,
    })
}

// =====================================================================
// Class / property / association / function
// =====================================================================

fn class(cd: &ClassDefinition, outer: &Scope<'_>) -> Rs<Arc<ClassDefinition>> {
    // class type parameters shadow imports inside the body
    let scope = outer.with_type_params(&cd.type_params);
    let supers = type_list(&cd.super_classes, &scope)?;
    let properties = arc_list(&cd.properties, |pd| property(pd, &scope))?;
    let derived = arc_list(&cd.derived_properties, |dp| {
        Ok(derived_property(dp, &scope)?.map(Box::new))
    })?;
    let constraints = arc_list(&cd.constraints, |c| Ok(constraint(c, &scope)?.map(Box::new)))?;
    let st = stereotypes(&cd.stereotypes, &scope)?;
    let tv = tagged_values(&cd.tagged_values, &scope)?;
    if supers.is_none()
        && properties.is_none()
        && derived.is_none()
        && constraints.is_none()
        && st.is_none()
        && tv.is_none()
    {
        return Ok(None);
    }
    Ok(Some(Arc::new(ClassDefinition {
        qualified_name: cd.qualified_name.clone(),
        type_params: cd.type_params.clone(),
        type_variables: cd.type_variables.clone(),
        super_classes: or(&supers, &cd.super_classes),
        properties: or(&properties, &cd.properties),
        derived_properties: or(&derived, &cd.derived_properties),
        constraints: or(&constraints, &cd.constraints),
        stereotypes: or(&st, &cd.stereotypes),
        tagged_values: or(&tv, &cd.tagged_values),
        is_native: cd.is_native,
        multiplicity_params: cd.multiplicity_params.clone(),
    })))
}

fn property(pd: &ClassDefinitionPropertyDefinition, scope: &Scope<'_>) -> Rs<Arc<ClassDefinitionPropertyDefinition>> {
    let ty = resolve_type(&pd.r#type, scope)?;
    let st = stereotypes(&pd.stereotypes, scope)?;
    let tv = tagged_values(&pd.tagged_values, scope)?;
    // the declared default resolves in the class's own scope
    let dflt = match &pd.default_value {
        None => None,
        Some(d) => resolve_vs(d, scope)?,
    };
    if ty.is_none() && st.is_none() && tv.is_none() && dflt.is_none() {
        return Ok(None);
    }
    Ok(Some(Arc::new(ClassDefinitionPropertyDefinition {
        name: pd.name.clone(),
        r#type: or(&ty, &pd.r#type),
        multiplicity: pd.multiplicity.clone(),
        stereotypes: or(&st, &pd.stereotypes),
        tagged_values: or(&tv, &pd.tagged_values),
        has_default: pd.has_default,
        default_value: dflt.or_else(|| pd.default_value.clone()),
    })))
}

fn association(ad: &AssociationDefinition, scope: &Scope<'_>) -> Rs<Arc<AssociationDefinition>> {
    let p1 = association_end(&ad.property1, scope)?;
    let p2 = association_end(&ad.property2, scope)?;
    let derived = arc_list(&ad.derived_properties, |dp| {
        Ok(derived_property(dp, scope)?.map(Box::new))
    })?;
    let st = stereotypes(&ad.stereotypes, scope)?;
    let tv = tagged_values(&ad.tagged_values, scope)?;
    if p1.is_none() && p2.is_none() && derived.is_none() && st.is_none() && tv.is_none() {
        return Ok(None);
    }
    Ok(Some(Arc::new(AssociationDefinition {
        qualified_name: ad.qualified_name.clone(),
        property1: or(&p1, &ad.property1),
        property2: or(&p2, &ad.property2),
        derived_properties: or(&derived, &ad.derived_properties),
        stereotypes: or(&st, &ad.stereotypes),
        tagged_values: or(&tv, &ad.tagged_values),
    })))
}

fn association_end(
    end: &AssociationDefinitionAssociationEndDefinition,
    scope: &Scope<'_>,
) -> Rs<Arc<AssociationDefinitionAssociationEndDefinition>> {
    let target = resolve_type(&end.target_class, scope)?;
    let st = stereotypes(&end.stereotypes, scope)?;
    let tv = tagged_values(&end.tagged_values, scope)?;
    if target.is_none() && st.is_none() && tv.is_none() {
        return Ok(None);
    }
    Ok(Some(Arc::new(AssociationDefinitionAssociationEndDefinition {
        property_name: end.property_name.clone(),
        target_class: or(&target, &end.target_class),
        multiplicity: end.multiplicity.clone(),
        stereotypes: or(&st, &end.stereotypes),
        tagged_values: or(&tv, &end.tagged_values),
    })))
}

fn function_params(
    params: &Arc<[Arc<FunctionDefinitionParameterDefinition>]>,
    scope: &Scope<'_>,
) -> Rs<Arc<[Arc<FunctionDefinitionParameterDefinition>]>> {
    arc_list(params, |pd| {
        Ok(resolve_type(&pd.r#type, scope)?.map(|t| {
            Arc::new(FunctionDefinitionParameterDefinition {
                name: pd.name.clone(),
                r#type: t,
                multiplicity: pd.multiplicity.clone(),
            })
        }))
    })
}

fn function(fd: &FunctionDefinition, outer: &Scope<'_>) -> Rs<Arc<FunctionDefinition>> {
    // function type parameters shadow imports throughout the signature and body
    let scope = outer.with_type_params(&fd.type_parameters);
    let params = function_params(&fd.parameters, &scope)?;
    let ret = resolve_type(&fd.return_type, &scope)?;
    let body = vs_list(&fd.body, &scope)?;
    let st = stereotypes(&fd.stereotypes, &scope)?;
    let tv = tagged_values(&fd.tagged_values, &scope)?;
    // its constraints read its parameters (and a post-condition $return): resolved as a class's are
    let pre = arc_list(&fd.pre_constraints, |c| Ok(constraint(c, &scope)?.map(Box::new)))?;
    let post = arc_list(&fd.post_constraints, |c| Ok(constraint(c, &scope)?.map(Box::new)))?;
    if params.is_none()
        && ret.is_none()
        && body.is_none()
        && st.is_none()
        && tv.is_none()
        && pre.is_none()
        && post.is_none()
    {
        return Ok(None);
    }
    Ok(Some(Arc::new(FunctionDefinition {
        qualified_name: fd.qualified_name.clone(),
        type_parameters: fd.type_parameters.clone(),
        multiplicity_parameters: fd.multiplicity_parameters.clone(),
        parameters: or(&params, &fd.parameters),
        return_type: or(&ret, &fd.return_type),
        return_multiplicity: fd.return_multiplicity.clone(),
        body: or(&body, &fd.body),
        stereotypes: or(&st, &fd.stereotypes),
        tagged_values: or(&tv, &fd.tagged_values),
        synthesized_from: fd.synthesized_from.clone(),
        pre_constraints: or(&pre, &fd.pre_constraints),
        post_constraints: or(&post, &fd.post_constraints),
    })))
}

fn native_function(nfd: &NativeFunctionDefinition, outer: &Scope<'_>) -> Rs<Arc<NativeFunctionDefinition>> {
    let scope = outer.with_type_params(&nfd.type_parameters);
    let params = function_params(&nfd.parameters, &scope)?;
    let ret = resolve_type(&nfd.return_type, &scope)?;
    let st = stereotypes(&nfd.stereotypes, &scope)?;
    let tv = tagged_values(&nfd.tagged_values, &scope)?;
    if params.is_none() && ret.is_none() && st.is_none() && tv.is_none() {
        return Ok(None);
    }
    Ok(Some(Arc::new(NativeFunctionDefinition {
        qualified_name: nfd.qualified_name.clone(),
        type_parameters: nfd.type_parameters.clone(),
        multiplicity_parameters: nfd.multiplicity_parameters.clone(),
        parameters: or(&params, &nfd.parameters),
        return_type: or(&ret, &nfd.return_type),
        return_multiplicity: nfd.return_multiplicity.clone(),
        stereotypes: or(&st, &nfd.stereotypes),
        tagged_values: or(&tv, &nfd.tagged_values),
    })))
}

/// `resolveEnum(ed)`: its own and its values' annotations; always rebuilt, then compared with the input by value (an
/// equal rebuild is the input).
fn enumeration(ed: &Arc<EnumDefinition>, scope: &Scope<'_>) -> Rs<Arc<EnumDefinition>> {
    let mut values: Vec<Arc<EnumDefinitionValueAnnotations>> = Vec::with_capacity(ed.value_annotations.len());
    for va in ed.value_annotations.iter() {
        let st = stereotypes(&va.stereotypes, scope)?;
        let tv = tagged_values(&va.tagged_values, scope)?;
        values.push(Arc::new(EnumDefinitionValueAnnotations {
            value: va.value.clone(),
            stereotypes: or(&st, &va.stereotypes),
            tagged_values: or(&tv, &va.tagged_values),
        }));
    }
    let st = stereotypes(&ed.stereotypes, scope)?;
    let tv = tagged_values(&ed.tagged_values, scope)?;
    let resolved = EnumDefinition {
        qualified_name: ed.qualified_name.clone(),
        values: ed.values.clone(),
        stereotypes: or(&st, &ed.stereotypes),
        tagged_values: or(&tv, &ed.tagged_values),
        value_annotations: values.into(),
    };
    Ok(if resolved == **ed {
        None
    } else {
        Some(Arc::new(resolved))
    })
}

// =====================================================================
// Mappings
// =====================================================================

fn legacy_mapping(md: &LegacyMappingDefinition, scope: &Scope<'_>) -> Rs<Arc<LegacyMappingDefinition>> {
    let includes = mapping_includes(&md.includes, scope)?;
    let class_mappings = arc_list(&md.class_mappings, |cm| class_mapping(cm, scope))?;
    let assoc = arc_list(&md.association_mappings, |am| association_mapping(am, scope))?;
    let enums = enumeration_mappings(&md.enumeration_mappings, scope)?;
    if includes.is_none() && class_mappings.is_none() && assoc.is_none() && enums.is_none() {
        return Ok(None);
    }
    Ok(Some(Arc::new(LegacyMappingDefinition {
        qualified_name: md.qualified_name.clone(),
        includes: or(&includes, &md.includes),
        class_mappings: or(&class_mappings, &md.class_mappings),
        association_mappings: or(&assoc, &md.association_mappings),
        enumeration_mappings: or(&enums, &md.enumeration_mappings),
    })))
}

fn clean_sheet_mapping(md: &CleanSheetMappingDefinition, scope: &Scope<'_>) -> Rs<Arc<CleanSheetMappingDefinition>> {
    let includes = mapping_includes(&md.includes, scope)?;
    let class_bindings = arc_list(&md.class_bindings, |b| {
        let class_fqn = name(&b.class_fqn, scope)?;
        let r = realization(&b.realization, scope)?;
        if class_fqn.is_none() && r.is_none() {
            return Ok(None);
        }
        Ok(Some(Arc::new(CleanSheetMappingDefinitionClassBinding {
            class_fqn: or(&class_fqn, &b.class_fqn),
            kind: b.kind,
            set_id: b.set_id.clone(),
            extends_set_id: b.extends_set_id.clone(),
            root: b.root,
            realization: or(&r, &b.realization),
            primary_key_columns: b.primary_key_columns.clone(),
        })))
    })?;
    let assoc_bindings = arc_list(&md.association_bindings, |b| {
        let fqn = name(&b.association_fqn, scope)?;
        let r = realization(&b.realization, scope)?;
        if fqn.is_none() && r.is_none() {
            return Ok(None);
        }
        Ok(Some(Arc::new(CleanSheetMappingDefinitionAssociationBinding {
            association_fqn: or(&fqn, &b.association_fqn),
            realization: or(&r, &b.realization),
        })))
    })?;
    let enums = enumeration_mappings(&md.enumeration_mappings, scope)?;
    if includes.is_none() && class_bindings.is_none() && assoc_bindings.is_none() && enums.is_none() {
        return Ok(None);
    }
    Ok(Some(Arc::new(CleanSheetMappingDefinition {
        qualified_name: md.qualified_name.clone(),
        includes: or(&includes, &md.includes),
        class_bindings: or(&class_bindings, &md.class_bindings),
        association_bindings: or(&assoc_bindings, &md.association_bindings),
        enumeration_mappings: or(&enums, &md.enumeration_mappings),
    })))
}

fn enumeration_mappings(ms: &Arc<[Arc<EnumerationMapping>]>, scope: &Scope<'_>) -> Rs<Arc<[Arc<EnumerationMapping>]>> {
    arc_list(ms, |em| {
        let enum_name = name(&em.enum_name, scope)?;
        let values = arc_list(&em.value_mappings, |evm| {
            Ok(arc_list(&evm.source_values, |sv| {
                Ok(match sv {
                    EnumerationMappingSourceValue::EnumerationMappingSourceValueEnumRef(r) => {
                        name(&r.enum_path, scope)?.map(|path| {
                            EnumerationMappingSourceValue::EnumerationMappingSourceValueEnumRef(Arc::new(
                                EnumerationMappingSourceValueEnumRef {
                                    enum_path: path,
                                    enum_value_name: r.enum_value_name.clone(),
                                },
                            ))
                        })
                    }
                    _ => None,
                })
            })?
            .map(|sources| {
                Arc::new(EnumerationMappingEnumValueMapping {
                    enum_value: evm.enum_value.clone(),
                    source_values: sources,
                })
            }))
        })?;
        if enum_name.is_none() && values.is_none() {
            return Ok(None);
        }
        Ok(Some(Arc::new(EnumerationMapping {
            enum_name: or(&enum_name, &em.enum_name),
            mapping_id: em.mapping_id.clone(),
            value_mappings: or(&values, &em.value_mappings),
        })))
    })
}

fn mapping_includes(includes: &Arc<[Arc<MappingInclude>]>, scope: &Scope<'_>) -> Rs<Arc<[Arc<MappingInclude>]>> {
    arc_list(includes, |inc| {
        let path = name(&inc.mapping_path, scope)?;
        let subs = arc_list(&inc.substitutions, |sub| {
            let orig = name(&sub.original_store, scope)?;
            let repl = name(&sub.replacement_store, scope)?;
            if orig.is_none() && repl.is_none() {
                return Ok(None);
            }
            Ok(Some(Arc::new(MappingIncludeStoreSubstitution {
                original_store: or(&orig, &sub.original_store),
                replacement_store: or(&repl, &sub.replacement_store),
            })))
        })?;
        if path.is_none() && subs.is_none() {
            return Ok(None);
        }
        Ok(Some(Arc::new(MappingInclude {
            mapping_path: or(&path, &inc.mapping_path),
            substitutions: or(&subs, &inc.substitutions),
        })))
    })
}

/// `resolveAggregation(agg)`: every view resolved with the node -- always rebuilt (a relational class mapping holding
/// one is therefore always rebuilt too).
fn aggregation(
    agg: &ClassMappingAggregationAware,
    scope: &Scope<'_>,
) -> Result<Arc<ClassMappingAggregationAware>, Thrown> {
    let mut out: Vec<Arc<ClassMappingAggregateView>> = Vec::with_capacity(agg.views.len());
    for v in agg.views.iter() {
        let mut gbs: Vec<p::ValueSpecification> = Vec::with_capacity(v.group_by_functions.len());
        for g in v.group_by_functions.iter() {
            gbs.push(resolve_vs(g, scope)?.unwrap_or_else(|| g.clone()));
        }
        let mut avs: Vec<Arc<ClassMappingAggregateValue>> = Vec::with_capacity(v.aggregate_values.len());
        for a in v.aggregate_values.iter() {
            let map_fn = resolve_vs(&a.map_fn, scope)?.unwrap_or_else(|| a.map_fn.clone());
            let aggregate_fn = resolve_vs(&a.aggregate_fn, scope)?.unwrap_or_else(|| a.aggregate_fn.clone());
            avs.push(Arc::new(ClassMappingAggregateValue { map_fn, aggregate_fn }));
        }
        let set = match class_mapping(&ClassMapping::ClassMappingRelational(v.set.clone()), scope)? {
            None => v.set.clone(),
            Some(ClassMapping::ClassMappingRelational(r)) => r,
            Some(_) => unreachable!("a relational set resolves to a relational set"),
        };
        out.push(Arc::new(ClassMappingAggregateView {
            index: v.index,
            can_aggregate: v.can_aggregate,
            group_by_functions: linked::list(gbs),
            aggregate_values: linked::list(avs),
            set,
        }));
    }
    Ok(Arc::new(ClassMappingAggregationAware {
        views: linked::list(out),
    }))
}

fn class_mapping(cm: &ClassMapping, scope: &Scope<'_>) -> Rs<ClassMapping> {
    use ClassMapping as C;
    Ok(match cm {
        C::ClassMappingRelational(r) => {
            let class_name = name(&r.class_name, scope)?;
            let main_table = match &r.main_table {
                None => None,
                Some(t) => name(&t.database, scope)?.map(|db| {
                    Arc::new(LegacyMappingDefinitionTableReference {
                        database: db,
                        table: t.table.clone(),
                    })
                }),
            };
            let filter = match &r.filter {
                None => None,
                Some(f) => filter_mapping(f, scope)?,
            };
            let group_by = rel_op_list(&r.group_by, scope)?;
            let primary_key = rel_op_list(&r.primary_key, scope)?;
            let props = property_mapping_list(&r.property_mappings, scope)?;
            let agg = match &r.aggregation {
                None => None,
                Some(a) => Some(aggregation(a, scope)?),
            };
            if class_name.is_none()
                && main_table.is_none()
                && filter.is_none()
                && group_by.is_none()
                && primary_key.is_none()
                && props.is_none()
                && agg.is_none()
            {
                None
            } else {
                Some(C::ClassMappingRelational(Arc::new(ClassMappingRelational {
                    class_name: or(&class_name, &r.class_name),
                    set_id: r.set_id.clone(),
                    extends_set_id: r.extends_set_id.clone(),
                    root: r.root,
                    main_table: main_table.or_else(|| r.main_table.clone()),
                    filter: filter.or_else(|| r.filter.clone()),
                    distinct: r.distinct,
                    group_by: or(&group_by, &r.group_by),
                    primary_key: or(&primary_key, &r.primary_key),
                    property_mappings: or(&props, &r.property_mappings),
                    source_url: r.source_url.clone(),
                    property_target_sets: r.property_target_sets.clone(),
                    aggregation: agg.or_else(|| r.aggregation.clone()),
                })))
            }
        }
        C::ClassMappingUnion(u) => name(&u.class_name, scope)?.map(|cn| {
            C::ClassMappingUnion(Arc::new(ClassMappingUnion {
                class_name: cn,
                set_id: u.set_id.clone(),
                extends_set_id: u.extends_set_id.clone(),
                root: u.root,
                member_set_ids: u.member_set_ids.clone(),
            }))
        }),
        C::ClassMappingInheritance(ih) => name(&ih.class_name, scope)?.map(|cn| {
            C::ClassMappingInheritance(Arc::new(ClassMappingInheritance {
                class_name: cn,
                set_id: ih.set_id.clone(),
                extends_set_id: ih.extends_set_id.clone(),
                root: ih.root,
            }))
        }),
        C::ClassMappingRelationFunction(rf) => {
            let class_name = name(&rf.class_name, scope)?;
            match &rf.func_ref {
                None => {
                    // ~src inline expression: the SOURCE resolves like any mapping expression
                    let src = match &rf.inline_source {
                        None => None,
                        Some(s) => resolve_vs(s, scope)?,
                    };
                    if class_name.is_none() && src.is_none() {
                        None
                    } else {
                        Some(C::ClassMappingRelationFunction(Arc::new(
                            ClassMappingRelationFunction {
                                class_name: or(&class_name, &rf.class_name),
                                set_id: rf.set_id.clone(),
                                extends_set_id: rf.extends_set_id.clone(),
                                root: rf.root,
                                func_ref: None,
                                inline_source: src.or_else(|| rf.inline_source.clone()),
                                columns: rf.columns.clone(),
                                primary_key: rf.primary_key.clone(),
                            },
                        )))
                    }
                }
                Some(f) => {
                    let func_ref = name(f, scope)?;
                    if class_name.is_none() && func_ref.is_none() {
                        None
                    } else {
                        // no inline source; the primary key rides the rebuild (since 2026-11-13: item 2)
                        Some(C::ClassMappingRelationFunction(Arc::new(
                            ClassMappingRelationFunction {
                                class_name: or(&class_name, &rf.class_name),
                                set_id: rf.set_id.clone(),
                                extends_set_id: rf.extends_set_id.clone(),
                                root: rf.root,
                                func_ref: Some(or(&func_ref, f)),
                                inline_source: None,
                                columns: rf.columns.clone(),
                                primary_key: rf.primary_key.clone(),
                            },
                        )))
                    }
                }
            }
        }
        C::ClassMappingPure(pm) => {
            let class_name = name(&pm.class_name, scope)?;
            let source_class = opt_name(&pm.source_class, scope)?;
            let filter = match &pm.filter {
                None => None,
                Some(f) => resolve_vs(f, scope)?,
            };
            let bindings = arc_list(&pm.property_bindings, |b| {
                Ok(resolve_vs(&b.expression, scope)?.map(|e| b.with_expression(e)))
            })?;
            if class_name.is_none() && source_class.is_none() && filter.is_none() && bindings.is_none() {
                None
            } else {
                Some(C::ClassMappingPure(Arc::new(ClassMappingPure {
                    class_name: or(&class_name, &pm.class_name),
                    set_id: pm.set_id.clone(),
                    extends_set_id: pm.extends_set_id.clone(),
                    root: pm.root,
                    source_class: source_class.unwrap_or_else(|| pm.source_class.clone()),
                    filter: filter.or_else(|| pm.filter.clone()),
                    property_bindings: or(&bindings, &pm.property_bindings),
                })))
            }
        }
    })
}

fn association_mapping(am: &AssociationMapping, scope: &Scope<'_>) -> Rs<AssociationMapping> {
    use AssociationMapping as A;
    Ok(match am {
        A::AssociationMappingRelational(r) => {
            let n = name(&r.association_name, scope)?;
            let props = arc_list(&r.property_mappings, |apm| {
                Ok(property_mapping(&apm.body, scope)?.map(|body| {
                    Arc::new(AssociationPropertyMapping {
                        source_set_id: apm.source_set_id.clone(),
                        target_set_id: apm.target_set_id.clone(),
                        body,
                    })
                }))
            })?;
            if n.is_none() && props.is_none() {
                None
            } else {
                Some(A::AssociationMappingRelational(Arc::new(
                    AssociationMappingRelational {
                        association_name: or(&n, &r.association_name),
                        property_mappings: or(&props, &r.property_mappings),
                    },
                )))
            }
        }
        A::AssociationMappingModelJoin(mj) => {
            let n = name(&mj.association_name, scope)?;
            let lam = super::values::lambda(&mj.lambda, scope)?;
            if n.is_none() && lam.is_none() {
                None
            } else {
                Some(A::AssociationMappingModelJoin(Arc::new(AssociationMappingModelJoin {
                    association_name: or(&n, &mj.association_name),
                    lambda: lam.unwrap_or_else(|| mj.lambda.clone()),
                })))
            }
        }
        A::AssociationMappingCross(x) => {
            let n = name(&x.association_name, scope)?;
            let props = arc_list(&x.property_mappings2, |xp| {
                Ok(resolve_vs(&xp.expression, scope)?.map(|e| {
                    Arc::new(AssociationMappingCrossXStoreProperty {
                        property_name: xp.property_name.clone(),
                        source_set_id: xp.source_set_id.clone(),
                        target_set_id: xp.target_set_id.clone(),
                        expression: e,
                    })
                }))
            })?;
            if n.is_none() && props.is_none() {
                None
            } else {
                Some(A::AssociationMappingCross(Arc::new(AssociationMappingCross {
                    association_name: or(&n, &x.association_name),
                    property_mappings2: or(&props, &x.property_mappings2),
                })))
            }
        }
    })
}

fn property_mapping_list(list: &Arc<[PropertyMapping]>, scope: &Scope<'_>) -> Rs<Arc<[PropertyMapping]>> {
    arc_list(list, |pm| property_mapping(pm, scope))
}

fn property_mapping(pm: &PropertyMapping, scope: &Scope<'_>) -> Rs<PropertyMapping> {
    use PropertyMapping as P;
    Ok(match pm {
        P::PropertyMappingColumn(c) => name(&c.database, scope)?.map(|db| {
            P::PropertyMappingColumn(Arc::new(PropertyMappingColumn {
                property_name: c.property_name.clone(),
                database: db,
                table: c.table.clone(),
                column: c.column.clone(),
            }))
        }),
        // ported as Java has it: an enumerated expression's relational operation is not walked
        P::PropertyMappingEnumeratedExpression(_) => None,
        P::PropertyMappingEnumeratedColumn(ec) => name(&ec.database, scope)?.map(|db| {
            P::PropertyMappingEnumeratedColumn(Arc::new(PropertyMappingEnumeratedColumn {
                property_name: ec.property_name.clone(),
                enum_mapping_id: ec.enum_mapping_id.clone(),
                database: db,
                table: ec.table.clone(),
                column: ec.column.clone(),
            }))
        }),
        P::PropertyMappingJoin(j) => {
            let db = name(&j.database, scope)?;
            let joins = join_chain(&j.joins, scope)?;
            if db.is_none() && joins.is_none() {
                None
            } else {
                Some(P::PropertyMappingJoin(Arc::new(PropertyMappingJoin {
                    property_name: j.property_name.clone(),
                    database: or(&db, &j.database),
                    joins: or(&joins, &j.joins),
                    target_set_id: j.target_set_id.clone(),
                })))
            }
        }
        P::PropertyMappingJoinTerminalColumn(jtc) => {
            let db = name(&jtc.database, scope)?;
            let joins = join_chain(&jtc.joins, scope)?;
            let term = rel_op(&jtc.terminal_column, scope)?;
            if db.is_none() && joins.is_none() && term.is_none() {
                None
            } else {
                Some(P::PropertyMappingJoinTerminalColumn(Arc::new(
                    PropertyMappingJoinTerminalColumn {
                        property_name: jtc.property_name.clone(),
                        database: or(&db, &jtc.database),
                        joins: or(&joins, &jtc.joins),
                        terminal_column: or(&term, &jtc.terminal_column),
                        enum_mapping_id: jtc.enum_mapping_id.clone(),
                        enum_mapped: jtc.enum_mapped,
                    },
                )))
            }
        }
        P::PropertyMappingExpression(e) => rel_op(&e.expression, scope)?.map(|expr| {
            P::PropertyMappingExpression(Arc::new(PropertyMappingExpression {
                property_name: e.property_name.clone(),
                expression: expr,
            }))
        }),
        P::PropertyMappingEmbedded(e) => {
            // its ~primaryKey's operations resolve as a class mapping's do, and ride the rebuild (since 2026-11-13: the
            // names fixes, item 2; the two-argument form dropped them)
            let subs = property_mapping_list(&e.property_mappings, scope)?;
            let pk = rel_op_list(&e.primary_key, scope)?;
            if subs.is_none() && pk.is_none() {
                None
            } else {
                Some(P::PropertyMappingEmbedded(Arc::new(PropertyMappingEmbedded {
                    property_name: e.property_name.clone(),
                    property_mappings: or(&subs, &e.property_mappings),
                    primary_key: or(&pk, &e.primary_key),
                })))
            }
        }
        // set ids are local; no FQN inside
        P::PropertyMappingInlineEmbedded(_) => None,
        P::PropertyMappingOtherwiseEmbedded(oe) => {
            let emb = property_mapping_list(&oe.embedded, scope)?;
            let fb = property_mapping(&oe.fallback, scope)?;
            if emb.is_none() && fb.is_none() {
                None
            } else {
                Some(P::PropertyMappingOtherwiseEmbedded(Arc::new(
                    PropertyMappingOtherwiseEmbedded {
                        property_name: oe.property_name.clone(),
                        embedded: or(&emb, &oe.embedded),
                        fallback_set_id: oe.fallback_set_id.clone(),
                        fallback: or(&fb, &oe.fallback),
                    },
                )))
            }
        }
        P::PropertyMappingLocalProperty(lp) => {
            let ty = resolve_type(&lp.r#type, scope)?;
            let body = property_mapping(&lp.body, scope)?;
            if ty.is_none() && body.is_none() {
                None
            } else {
                Some(P::PropertyMappingLocalProperty(Arc::new(
                    PropertyMappingLocalProperty {
                        property_name: lp.property_name.clone(),
                        r#type: or(&ty, &lp.r#type),
                        multiplicity: lp.multiplicity.clone(),
                        body: or(&body, &lp.body),
                    },
                )))
            }
        }
    })
}

// =====================================================================
// Database
// =====================================================================

fn database(db: &DatabaseDefinition, scope: &Scope<'_>) -> Rs<Arc<DatabaseDefinition>> {
    let includes = fqn_list(&db.includes, scope)?;
    // each view resolved ONCE: a schema view is the same record in its schema's list and the flat list, so the one it
    // becomes is the same in both (by identity: the flat list's lookup, never iterated)
    let mut resolved_views: IdMap<View, Option<View>> = IdMap::new();
    let schemas = arc_list(&db.schemas, |s| {
        let views = arc_list(&s.views, |v| {
            let r = view(v, scope)?;
            resolved_views.insert(v.clone(), r.clone());
            Ok(r)
        })?;
        Ok(views.map(|views| {
            Arc::new(DatabaseDefinitionSchemaDefinition {
                name: s.name.clone(),
                tables: s.tables.clone(),
                views,
                tabular_functions: s.tabular_functions.clone(),
            })
        }))
    })?;
    let views = arc_list(&db.views, |v| match resolved_views.get(v) {
        Some(shared) => Ok(shared.clone()),
        None => view(v, scope),
    })?;
    let joins = arc_list(&db.joins, |j| {
        Ok(rel_op(&j.operation, scope)?.map(|op| {
            Arc::new(DatabaseDefinitionJoinDefinition {
                name: j.name.clone(),
                operation: op,
            })
        }))
    })?;
    let filters = filter_definitions(&db.filters, scope)?;
    let multi_grain = filter_definitions(&db.multi_grain_filters, scope)?;
    if includes.is_none()
        && schemas.is_none()
        && views.is_none()
        && joins.is_none()
        && filters.is_none()
        && multi_grain.is_none()
    {
        return Ok(None);
    }
    Ok(Some(Arc::new(DatabaseDefinition {
        qualified_name: db.qualified_name.clone(),
        includes: or(&includes, &db.includes),
        schemas: or(&schemas, &db.schemas),
        tables: db.tables.clone(),
        views: or(&views, &db.views),
        tabular_functions: db.tabular_functions.clone(),
        joins: or(&joins, &db.joins),
        filters: or(&filters, &db.filters),
        multi_grain_filters: or(&multi_grain, &db.multi_grain_filters),
    })))
}

fn filter_definitions(
    fs: &Arc<[Arc<DatabaseDefinitionFilterDefinition>]>,
    scope: &Scope<'_>,
) -> Rs<Arc<[Arc<DatabaseDefinitionFilterDefinition>]>> {
    arc_list(fs, |f| {
        Ok(rel_op(&f.condition, scope)?.map(|cond| {
            Arc::new(DatabaseDefinitionFilterDefinition {
                name: f.name.clone(),
                condition: cond,
            })
        }))
    })
}

fn view(v: &DatabaseDefinitionViewDefinition, scope: &Scope<'_>) -> Rs<View> {
    let filter = match &v.filter {
        None => None,
        Some(f) => filter_mapping(f, scope)?,
    };
    let group_by = rel_op_list(&v.group_by_columns, scope)?;
    let cols = arc_list(&v.column_mappings, |c| {
        Ok(rel_op(&c.expression, scope)?.map(|expr| {
            Arc::new(DatabaseDefinitionViewDefinitionViewColumnMapping {
                name: c.name.clone(),
                target_set_id: c.target_set_id.clone(),
                expression: expr,
                primary_key: c.primary_key,
            })
        }))
    })?;
    if filter.is_none() && group_by.is_none() && cols.is_none() {
        return Ok(None);
    }
    Ok(Some(Arc::new(DatabaseDefinitionViewDefinition {
        name: v.name.clone(),
        filter: filter.or_else(|| v.filter.clone()),
        group_by_columns: or(&group_by, &v.group_by_columns),
        distinct: v.distinct,
        column_mappings: or(&cols, &v.column_mappings),
    })))
}

// =====================================================================
// Runtime / service / data space
// =====================================================================

fn runtime(rd: &RuntimeDefinition, scope: &Scope<'_>) -> Rs<Arc<RuntimeDefinition>> {
    let mappings = fqn_list(&rd.mappings, scope)?;
    // the bindings rebuilt as Java's LinkedHashMap; two keys that name one store keep both connection lists, in order,
    // under the first key's place (until 2026-11-13 the second replaced the first's: the names fixes, item 3, ruling 28)
    let mut bindings: LinkedMap<JStr, Arc<[JStr]>> = LinkedMap::new();
    let mut bindings_changed = false;
    for (store, conns) in rd.connection_bindings.iter() {
        let s = resolve_name(store, scope)?;
        let c = fqn_list(conns, scope)?;
        bindings_changed |= &s != store || c.is_some();
        let conns = c.unwrap_or_else(|| conns.clone());
        let merged: Arc<[JStr]> = match bindings.get(&s) {
            None => conns,
            Some(prior) => prior.iter().chain(conns.iter()).cloned().collect(),
        };
        bindings.put(s, merged);
    }
    let mut inline: Vec<PackageableElement> = Vec::with_capacity(rd.inline_connections.len());
    let mut inline_changed = false;
    for el in rd.inline_connections.iter() {
        match resolve_element(el, scope)? {
            Some(r) => {
                inline_changed = true;
                inline.push(r);
            }
            None => inline.push(el.clone()),
        }
    }
    // the connection ids' stores resolve whatever else changed (until 2026-11-13 only on a rebuild: item 3)
    let mut ids: LinkedMap<JStr, JStr> = LinkedMap::new();
    let mut ids_changed = false;
    for (id, store) in rd.connection_ids.iter() {
        let s = resolve_name(store, scope)?;
        ids_changed |= &s != store;
        ids.put(id.clone(), s);
    }
    if mappings.is_none() && !bindings_changed && !inline_changed && !ids_changed {
        return Ok(None);
    }
    Ok(Some(Arc::new(RuntimeDefinition {
        qualified_name: rd.qualified_name.clone(),
        mappings: or(&mappings, &rd.mappings),
        connection_bindings: bindings.freeze(),
        json_connections: rd.json_connections.clone(),
        inline_connections: linked::list(inline),
        connection_ids: ids.freeze(),
    })))
}

fn keyed_executions(
    ks: &Arc<[Arc<ServiceDefinitionKeyedExecution>]>,
    scope: &Scope<'_>,
) -> Rs<Arc<[Arc<ServiceDefinitionKeyedExecution>]>> {
    let mut out: Vec<Arc<ServiceDefinitionKeyedExecution>> = Vec::with_capacity(ks.len());
    let mut changed = false;
    for k in ks.iter() {
        let m = opt_name(&k.mapping, scope)?;
        let r = opt_name(&k.runtime, scope)?;
        changed |= m.is_some() || r.is_some();
        out.push(Arc::new(ServiceDefinitionKeyedExecution {
            key_value: k.key_value.clone(),
            mapping: m.unwrap_or_else(|| k.mapping.clone()),
            runtime: r.unwrap_or_else(|| k.runtime.clone()),
        }));
    }
    Ok(changed.then(|| linked::list(out)))
}

fn service(sd: &ServiceDefinition, scope: &Scope<'_>) -> Rs<Arc<ServiceDefinition>> {
    let body = resolve_vs(&sd.function_body, scope)?;
    let mapping_ref = opt_name(&sd.mapping_ref, scope)?;
    let runtime_ref = opt_name(&sd.runtime_ref, scope)?;
    let multi = match &sd.multi_execution {
        None => None,
        Some(m) => keyed_executions(&m.executions, scope)?.map(|ks| {
            Arc::new(ServiceDefinitionMultiExecution {
                key: m.key.clone(),
                executions: ks,
            })
        }),
    };
    if body.is_none() && mapping_ref.is_none() && runtime_ref.is_none() && multi.is_none() {
        return Ok(None);
    }
    Ok(Some(Arc::new(ServiceDefinition {
        qualified_name: sd.qualified_name.clone(),
        pattern: sd.pattern.clone(),
        function_body: or(&body, &sd.function_body),
        documentation: sd.documentation.clone(),
        mapping_ref: mapping_ref.unwrap_or_else(|| sd.mapping_ref.clone()),
        runtime_ref: runtime_ref.unwrap_or_else(|| sd.runtime_ref.clone()),
        test_suites: sd.test_suites.clone(),
        owners: sd.owners.clone(),
        auto_activate_updates: sd.auto_activate_updates,
        multi_execution: multi.or_else(|| sd.multi_execution.clone()),
        test: sd.test.clone(),
    })))
}

fn data_space(ds: &DataSpaceDefinition, scope: &Scope<'_>) -> Rs<Arc<DataSpaceDefinition>> {
    let mut changed = false;
    let mut contexts = Vec::with_capacity(ds.execution_contexts.len());
    for ctx in ds.execution_contexts.iter() {
        // a context may name a mapping PROVIDER instead of a mapping, and its default runtime is optional
        let m = opt_name(&ctx.mapping, scope)?;
        let r = opt_name(&ctx.default_runtime, scope)?;
        changed |= m.is_some() || r.is_some();
        contexts.push(Arc::new(DataSpaceDefinitionExecutionContext {
            name: ctx.name.clone(),
            title: ctx.title.clone(),
            description: ctx.description.clone(),
            mapping: m.unwrap_or_else(|| ctx.mapping.clone()),
            default_runtime: r.unwrap_or_else(|| ctx.default_runtime.clone()),
            test_data_source: ctx.test_data_source.clone(),
        }));
    }
    let mut executables = Vec::with_capacity(ds.executables.len());
    for e in ds.executables.iter() {
        // a function-pointer executable keeps its full signature text and is no resolvable FQN
        let ex = match &e.executable {
            Some(x) if !x.0.contains(&u16::from(b'(')) => {
                let r = resolve_name(x, scope)?;
                changed |= &r != x;
                Some(r)
            }
            other => other.clone(),
        };
        executables.push(Arc::new(DataSpaceDefinitionExecutable {
            id: e.id.clone(),
            title: e.title.clone(),
            description: e.description.clone(),
            executable: ex,
            query_source: e.query_source.clone(),
            execution_context_key: e.execution_context_key.clone(),
        }));
    }
    let mut diagrams = Vec::with_capacity(ds.diagrams.len());
    for g in ds.diagrams.iter() {
        let d = resolve_name(&g.diagram, scope)?;
        changed |= d != g.diagram;
        diagrams.push(Arc::new(DataSpaceDefinitionDiagram {
            title: g.title.clone(),
            description: g.description.clone(),
            diagram: d,
        }));
    }
    if !changed {
        return Ok(None);
    }
    Ok(Some(Arc::new(DataSpaceDefinition {
        qualified_name: ds.qualified_name.clone(),
        execution_contexts: linked::list(contexts),
        default_execution_context: ds.default_execution_context.clone(),
        title: ds.title.clone(),
        description: ds.description.clone(),
        executables: linked::list(executables),
        diagrams: linked::list(diagrams),
        support_info_source: ds.support_info_source.clone(),
        elements: ds.elements.clone(),
        stereotypes: ds.stereotypes.clone(),
    })))
}

// =====================================================================
// Shared relational walkers
// =====================================================================

fn filter_mapping(fm: &FilterMapping, scope: &Scope<'_>) -> Rs<FilterMapping> {
    Ok(match fm {
        FilterMapping::FilterMappingDirect(d) => filter_pointer(&d.filter, scope)?
            .map(|fp| FilterMapping::FilterMappingDirect(Arc::new(FilterMappingDirect { filter: fp }))),
        FilterMapping::FilterMappingJoinMediated(jm) => {
            let src = opt_name(&jm.source_db, scope)?;
            let joins = join_chain(&jm.joins, scope)?;
            let fp = filter_pointer(&jm.filter, scope)?;
            if src.is_none() && joins.is_none() && fp.is_none() {
                None
            } else {
                // joinType rides the rebuild
                Some(FilterMapping::FilterMappingJoinMediated(Arc::new(
                    FilterMappingJoinMediated {
                        source_db: src.unwrap_or_else(|| jm.source_db.clone()),
                        joins: or(&joins, &jm.joins),
                        filter: or(&fp, &jm.filter),
                        join_type: jm.join_type.clone(),
                    },
                )))
            }
        }
    })
}

fn filter_pointer(fp: &FilterPointer, scope: &Scope<'_>) -> Rs<FilterPointer> {
    Ok(match fp {
        FilterPointer::FilterPointerLocal(_) => None,
        FilterPointer::FilterPointerCross(c) => name(&c.db, scope)?.map(|db| {
            FilterPointer::FilterPointerCross(Arc::new(FilterPointerCross {
                db,
                name: c.name.clone(),
            }))
        }),
    })
}

fn join_chain(chain: &Arc<[Arc<JoinChainElement>]>, scope: &Scope<'_>) -> Rs<Arc<[Arc<JoinChainElement>]>> {
    arc_list(chain, |jce| {
        let Some(jdb) = &jce.database_name else {
            return Ok(None);
        };
        Ok(name(jdb, scope)?.map(|db| {
            Arc::new(JoinChainElement {
                join_name: jce.join_name.clone(),
                join_type: jce.join_type,
                database_name: Some(db),
                include_self: jce.include_self,
            })
        }))
    })
}

fn rel_op_list(ops: &Arc<[RelationalOperation]>, scope: &Scope<'_>) -> Rs<Arc<[RelationalOperation]>> {
    arc_list(ops, |o| rel_op(o, scope))
}

fn rel_op(op: &RelationalOperation, scope: &Scope<'_>) -> Rs<RelationalOperation> {
    use RelationalOperation as R;
    Ok(match op {
        R::RelationalOperationColumnRef(cr) => match &cr.database_name {
            None => None,
            Some(d) => name(d, scope)?.map(|db| {
                R::RelationalOperationColumnRef(Arc::new(RelationalOperationColumnRef {
                    database_name: Some(db),
                    table: cr.table.clone(),
                    column: cr.column.clone(),
                }))
            }),
        },
        R::RelationalOperationTargetColumnRef(_)
        | R::RelationalOperationLiteral(_)
        | R::RelationalOperationLambdaParam(_) => None,
        // relational lambdas: names resolve inside the body (always rebuilt)
        R::RelationalOperationLambda(lam) => {
            let body = rel_op(&lam.body, scope)?.unwrap_or_else(|| lam.body.clone());
            Some(R::RelationalOperationLambda(Arc::new(RelationalOperationLambda {
                parameters: lam.parameters.clone(),
                body,
            })))
        }
        // a DB-side function name, never a Pure FQN: only its arguments
        R::RelationalOperationFunctionCall(fc) => rel_op_list(&fc.args, scope)?.map(|args| {
            R::RelationalOperationFunctionCall(Arc::new(RelationalOperationFunctionCall {
                name: fc.name.clone(),
                args,
            }))
        }),
        R::RelationalOperationComparison(c) => {
            let l = rel_op(&c.left, scope)?;
            let r = rel_op(&c.right, scope)?;
            if l.is_none() && r.is_none() {
                None
            } else {
                Some(R::RelationalOperationComparison(Arc::new(
                    RelationalOperationComparison {
                        left: or(&l, &c.left),
                        op: c.op,
                        right: or(&r, &c.right),
                    },
                )))
            }
        }
        R::RelationalOperationBooleanOp(b) => {
            let l = rel_op(&b.left, scope)?;
            let r = rel_op(&b.right, scope)?;
            if l.is_none() && r.is_none() {
                None
            } else {
                Some(R::RelationalOperationBooleanOp(Arc::new(
                    RelationalOperationBooleanOp {
                        left: or(&l, &b.left),
                        op: b.op,
                        right: or(&r, &b.right),
                    },
                )))
            }
        }
        R::RelationalOperationIsNull(n) => rel_op(&n.operand, scope)?
            .map(|o| R::RelationalOperationIsNull(Arc::new(RelationalOperationIsNull { operand: o }))),
        R::RelationalOperationIsNotNull(n) => rel_op(&n.operand, scope)?
            .map(|o| R::RelationalOperationIsNotNull(Arc::new(RelationalOperationIsNotNull { operand: o }))),
        R::RelationalOperationGroup(g) => rel_op(&g.inner, scope)?
            .map(|i| R::RelationalOperationGroup(Arc::new(RelationalOperationGroup { inner: i }))),
        R::RelationalOperationArrayLiteral(al) => rel_op_list(&al.elements, scope)?
            .map(|els| R::RelationalOperationArrayLiteral(Arc::new(RelationalOperationArrayLiteral { elements: els }))),
        R::RelationalOperationJoinNavigation(jn) => {
            // databaseName is null for the LOCAL form (the contextual database binds later, in the normalizer)
            let db = opt_name(&jn.database_name, scope)?;
            let chain = join_chain(&jn.chain, scope)?;
            let term = match &jn.terminal {
                None => None,
                Some(t) => rel_op(t, scope)?,
            };
            if db.is_none() && chain.is_none() && term.is_none() {
                None
            } else {
                Some(R::RelationalOperationJoinNavigation(Arc::new(
                    RelationalOperationJoinNavigation {
                        database_name: db.unwrap_or_else(|| jn.database_name.clone()),
                        chain: or(&chain, &jn.chain),
                        terminal: term.or_else(|| jn.terminal.clone()),
                    },
                )))
            }
        }
    })
}
