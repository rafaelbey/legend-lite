// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! core/'s `com.legend.protocol.MappingEmitter`, ported (docs/rust-port/PHASE2.md §4 step 5): the `###Mapping`
//! element's wire -- its class mappings (relational, Pure, clean-sheet function, AggregationAware, operation and merge,
//! Relation, ServiceStore, MongoDB), association mappings, enumeration mappings, includes, test suites and legacy
//! tests -- and the embedded data values (`###Data` and the test data). Byte for byte Java's, its refusals in Java's
//! order (`emitter`'s module comment).

use crate::emitter::{
    Buf, Em, comma_each, element_ptr, illegal_state, instances_collection, join_ptr, lambda, npe, placeless,
    pointer, rel_op, src_info, str, strs_each, table_ptr, value_spec,
};
use crate::jtypes::{JStr, JValue};
use crate::records::*;

/// `MappingEmitter.mapping`: `_type:"mapping"`.
pub fn mapping(b: &mut Buf, m: &PMapping) -> Em {
    b.a("{\"_type\":\"mapping\",\"associationMappings\":[");
    comma_each(b, &m.association_mappings, association_mapping)?;
    b.a("],\"classMappings\":[");
    for (i, cm) in m.class_mappings.iter().enumerate() {
        if i > 0 {
            b.ch(b',');
        }
        match cm {
            PClassMapping::PServiceStoreClassMapping(ss) => service_store_class_mapping(b, ss)?,
            PClassMapping::PClassMappingMongoDb(mg) => mongo_db_class_mapping(b, mg),
            PClassMapping::PClassMappingRel(r) => rel_class_mapping(b, r)?,
            PClassMapping::PClassMappingPure(pu) => pure_class_mapping(b, pu)?,
            PClassMapping::PClassMappingFunction(fi) => function_class_mapping(b, fi)?,
            PClassMapping::PClassMappingAggregationAware(aa) => aggregation_aware(b, aa)?,
            PClassMapping::PClassMappingMergeOperation(mo) => {
                b.a("{\"_type\":\"mergeOperation\",\"class\":");
                str(b, &mo.class_name);
                b.a(",\"classSourceInformation\":");
                src_info(b, mo.class_source_information.as_deref());
                extends_id(b, mo.extends_class_mapping_id.as_ref());
                if let Some(id) = &mo.id {
                    b.a(",\"id\":");
                    str(b, id);
                }
                b.a(",\"operation\":\"MERGE\",\"parameters\":[");
                strs_each(b, &mo.parameters);
                b.a("],\"root\":");
                b.boolean(mo.root);
                b.a(",\"sourceInformation\":");
                src_info(b, mo.source_information.as_deref());
                b.a(",\"validationFunction\":{\"_type\":\"lambda\",\"body\":[");
                value_spec(b, &mo.validation_lambda)?;
                b.a("],\"parameters\":[]}}");
            }
            PClassMapping::PClassMappingRelation(rf) => relation_class_mapping(b, rf)?,
            PClassMapping::PClassMappingOperation(om) => {
                b.a("{\"_type\":\"operation\",\"class\":");
                str(b, &om.class_name);
                b.a(",\"classSourceInformation\":");
                src_info(b, om.class_source_information.as_deref());
                extends_id(b, om.extends_class_mapping_id.as_ref());
                if let Some(id) = &om.id {
                    b.a(",\"id\":");
                    str(b, id);
                }
                if let Some(op) = &om.operation {
                    // MERGE emits no discriminator
                    b.a(",\"operation\":");
                    str(b, op);
                }
                b.a(",\"parameters\":[");
                strs_each(b, &om.parameters);
                b.a("],\"root\":");
                b.boolean(om.root);
                b.a(",\"sourceInformation\":");
                src_info(b, om.source_information.as_deref());
                b.ch(b'}');
            }
        }
    }
    b.a("],\"enumerationMappings\":[");
    comma_each(b, &m.enumeration_mappings, |b, em| enumeration_mapping(b, em))?;
    b.a("],\"includedMappings\":[");
    comma_each(b, &m.included_mappings, |b, inc| mapping_include(b, inc))?;
    b.a("],\"name\":");
    str(b, &m.name);
    b.a(",\"package\":");
    str(b, &m.pkg);
    b.a(",\"sourceInformation\":");
    src_info(b, m.source_information.as_deref());
    if !m.test_suites.is_empty() {
        b.a(",\"testSuites\":[");
        comma_each(b, &m.test_suites, |b, ts| test_suite(b, ts))?;
        b.ch(b']');
    }
    b.a(",\"tests\":[");
    for (i, t) in m.tests.iter().enumerate() {
        if i > 0 {
            b.ch(b',');
        }
        b.a("{\"assert\":{\"_type\":\"expectedOutputMappingTestAssert\",\"expectedOutput\":");
        str(b, &t.expected_output);
        b.a(",\"sourceInformation\":");
        src_info(b, t.assert_source_information.as_deref());
        b.a("},\"inputData\":[");
        for (j, inp) in t.input_data.iter().enumerate() {
            if j > 0 {
                b.ch(b',');
            }
            if inp.relational {
                b.a("{\"_type\":\"relational\",\"data\":");
                str(b, &inp.data);
                b.a(",\"database\":");
                str(b, &inp.target_path);
            } else {
                b.a("{\"_type\":\"object\",\"data\":");
                str(b, &inp.data);
            }
            b.a(",\"inputType\":");
            str(b, &inp.input_type);
            if !inp.relational {
                b.a(",\"sourceClass\":");
                str(b, &inp.target_path);
            }
            b.a(",\"sourceInformation\":");
            src_info(b, inp.source_information.as_deref());
            b.ch(b'}');
        }
        b.a("],\"name\":");
        str(b, &t.name);
        b.a(",\"query\":");
        value_spec(b, &t.query)?;
        b.a(",\"sourceInformation\":");
        src_info(b, t.source_information.as_deref());
        b.ch(b'}');
    }
    b.a("]}");
    Ok(())
}

/// The `relation` class mapping (a Relation function's, or the `~src` row-source form).
fn relation_class_mapping(b: &mut Buf, rf: &PClassMappingRelation) -> Em {
    b.a("{\"_type\":\"relation\",\"class\":");
    str(b, &rf.class_name);
    if let Some(e) = &rf.extends_class_mapping_id {
        b.a(",\"extendsClassMappingId\":");
        str(b, e);
    }
    if let Some(id) = &rf.id {
        b.a(",\"id\":");
        str(b, id);
    }
    b.a(",\"primaryKey\":[");
    strs_each(b, &rf.primary_key);
    b.a("],\"propertyMappings\":[");
    for (j, pm) in rf.property_mappings.iter().enumerate() {
        if j > 0 {
            b.ch(b',');
        }
        if pm.nested.is_some() {
            relation_fn_embedded(b, pm)?;
            continue;
        }
        if let Some(set_id) = &pm.inline_set_id {
            // prop () Inline [set]
            b.a("{\"_type\":\"relationFunctionEmbeddedPropertyMapping\",\"id\":");
            str(b, set_id);
            b.a(",\"property\":{\"class\":");
            let Some(owner) = &pm.owner_class else {
                return Err(npe(None));
            };
            str(b, owner);
            b.a(",\"property\":");
            str(b, &pm.property);
            b.a(",\"sourceInformation\":");
            src_info(b, pm.property_source_information.as_deref());
            b.a("},\"propertyMappings\":[]");
            if let Some(s) = &pm.source {
                b.a(",\"source\":");
                str(b, s);
            }
            b.a(",\"sourceInformation\":");
            src_info(b, pm.source_information.as_deref());
            b.ch(b'}');
            continue;
        }
        relation_fn_pm(b, pm)?;
    }
    b.ch(b']');
    if let Some(f) = &rf.relation_function {
        b.a(",\"relationFunction\":{\"path\":");
        str(b, f);
        b.a(",\"sourceInformation\":");
        src_info(b, rf.relation_function_source_information.as_deref());
        b.a(",\"type\":\"FUNCTION\"}");
    }
    b.a(",\"root\":");
    b.boolean(rf.root);
    b.a(",\"sourceInformation\":");
    src_info(b, rf.source_information.as_deref());
    if let Some(sl) = &rf.source_lambda {
        // ~src row-source form: one func node over the bare name, the wrapper's span the parser's
        b.a(",\"sourceLambda\":{\"_type\":\"lambda\",\"body\":[");
        if let Some(expr) = &sl.expr {
            // ~src <expression>: the raw expression is the body
            value_spec(b, expr)?;
        } else {
            b.a("{\"_type\":\"func\",\"function\":");
            let Some(function) = &sl.function else {
                return Err(npe(None));
            };
            str(b, function);
            b.a(",\"parameters\":[],\"sourceInformation\":");
            src_info(b, sl.function_source_information.as_deref());
            b.ch(b'}');
        }
        b.a("],\"parameters\":[],\"sourceInformation\":");
        src_info(b, sl.source_information.as_deref());
        b.ch(b'}');
    }
    b.ch(b'}');
    Ok(())
}

/// `MappingEmitter.associationMapping`.
pub fn association_mapping(b: &mut Buf, am: &PAssociationMapping) -> Em {
    match am {
        // clean-sheet association binding: the pointer-or-lambda fork of functionInstance
        PAssociationMapping::PFunctionAssociationMapping(fa) => {
            b.a("{\"_type\":\"functionAssociation\",\"association\":");
            pointer(b, &fa.association);
            if let Some(l) = &fa.body_lambda {
                b.a(",\"bodyLambda\":");
                lambda(b, l)?;
            }
            if let Some(f) = &fa.function {
                b.a(",\"function\":");
                element_ptr(b, &f.full_path, f.pos.as_deref());
            }
            b.a(",\"sourceInformation\":");
            src_info(b, fa.source_information.as_deref());
            b.ch(b'}');
        }
        PAssociationMapping::PRelAssociationMapping(ra) => {
            b.a("{\"_type\":\"relational\",\"association\":");
            pointer(b, &ra.association);
            if let Some(id) = &ra.id {
                b.a(",\"id\":");
                str(b, id);
            }
            b.a(",\"propertyMappings\":[");
            for (j, pm) in ra.property_mappings.iter().enumerate() {
                if j > 0 {
                    b.ch(b',');
                }
                b.a("{\"_type\":\"relationalPropertyMapping\",\"property\":{");
                if let Some(owner) = &pm.owner_class {
                    b.a("\"class\":");
                    str(b, owner);
                    b.ch(b',');
                }
                b.a("\"property\":");
                str(b, &pm.property);
                b.a(",\"sourceInformation\":");
                src_info(b, pm.property_source_information.as_deref());
                b.a("},\"relationalOperation\":");
                rel_op(b, &pm.relational_operation, true);
                if let Some(s) = &pm.source {
                    b.a(",\"source\":");
                    str(b, s);
                }
                b.a(",\"sourceInformation\":");
                src_info(b, pm.source_information.as_deref());
                if let Some(t) = &pm.target {
                    b.a(",\"target\":");
                    str(b, t);
                }
                b.ch(b'}');
            }
            b.a("],\"sourceInformation\":");
            src_info(b, ra.source_information.as_deref());
            b.a(",\"stores\":[");
            strs_each(b, &ra.stores);
            b.a("]}");
        }
        PAssociationMapping::PXStoreAssociationMapping(xa) => {
            b.a("{\"_type\":\"xStore\",\"association\":");
            pointer(b, &xa.association);
            if let Some(id) = &xa.id {
                b.a(",\"id\":");
                str(b, id);
            }
            b.a(",\"propertyMappings\":[");
            for (j, pm) in xa.property_mappings.iter().enumerate() {
                if j > 0 {
                    b.ch(b',');
                }
                b.a("{\"_type\":\"xStorePropertyMapping\",\"crossExpression\":{\"_type\":\"lambda\",\"body\":[");
                comma_each(b, &pm.cross_expression, value_spec)?;
                b.a("],\"parameters\":[]},\"property\":{\"class\":");
                str(b, &pm.owner_class);
                b.a(",\"property\":");
                str(b, &pm.property);
                b.a(",\"sourceInformation\":");
                src_info(b, pm.property_source_information.as_deref());
                b.a("},\"source\":");
                str(b, &pm.source);
                b.a(",\"sourceInformation\":");
                src_info(b, pm.source_information.as_deref());
                b.a(",\"target\":");
                str(b, &pm.target);
                b.ch(b'}');
            }
            b.a("],\"sourceInformation\":");
            src_info(b, xa.source_information.as_deref());
            b.a(",\"stores\":[]}");
        }
        PAssociationMapping::PModelJoinAssociationMapping(mj) => {
            b.a("{\"_type\":\"modelJoin\",\"association\":");
            pointer(b, &mj.association);
            if let Some(id) = &mj.id {
                b.a(",\"id\":");
                str(b, id);
            }
            b.a(",\"joinCondition\":");
            value_spec(b, &mj.join_condition)?;
            b.a(",\"sourceInformation\":");
            src_info(b, mj.source_information.as_deref());
            b.a(",\"stores\":[]}");
        }
    }
    Ok(())
}

/// `_type:"MongoDB"` class mapping: no spans on this wire.
fn mongo_db_class_mapping(b: &mut Buf, mg: &PClassMappingMongoDb) {
    b.a("{\"_type\":\"MongoDB\"");
    if let Some(p) = &mg.binding_path {
        b.a(",\"bindingPath\":");
        str(b, p);
    }
    b.a(",\"class\":");
    str(b, &mg.class_name);
    extends_id(b, mg.extends_class_mapping_id.as_ref());
    if let Some(id) = &mg.id {
        b.a(",\"id\":");
        str(b, id);
    }
    if let Some(c) = &mg.main_collection_name {
        b.a(",\"mainCollectionName\":");
        str(b, c);
    }
    b.a(",\"root\":");
    b.boolean(mg.root);
    if let Some(p) = &mg.store_path {
        b.a(",\"storePath\":");
        str(b, p);
    }
    b.ch(b'}');
}

/// `extendsClassMappingId`, when the class mapping extends another.
fn extends_id(b: &mut Buf, extends_class_mapping_id: Option<&JStr>) {
    if let Some(e) = extends_class_mapping_id {
        b.a(",\"extendsClassMappingId\":");
        str(b, e);
    }
}

/// `_type:"serviceStore"` class mapping: dotted service pointers nest parent group objects; transforms wrap as
/// parameterless lambdas spanning their expression.
fn service_store_class_mapping(b: &mut Buf, ss: &PServiceStoreClassMapping) -> Em {
    b.a("{\"_type\":\"serviceStore\",\"class\":");
    str(b, &ss.class_name);
    b.a(",\"classSourceInformation\":");
    src_info(b, ss.class_span.as_deref());
    extends_id(b, ss.extends_class_mapping_id.as_ref());
    if let Some(id) = &ss.id {
        b.a(",\"id\":");
        str(b, id);
    }
    b.a(",\"localMappingProperties\":[");
    for (i, lp) in ss.local_props.iter().enumerate() {
        if i > 0 {
            b.ch(b',');
        }
        b.a("{\"multiplicity\":{\"lowerBound\":");
        b.int(i64::from(lp.lower_bound));
        if let Some(up) = lp.upper_bound {
            b.a(",\"upperBound\":");
            b.int(i64::from(up));
        }
        b.a("},\"name\":");
        str(b, &lp.name);
        b.a(",\"sourceInformation\":");
        src_info(b, lp.source_information.as_deref());
        b.a(",\"type\":");
        str(b, &lp.r#type);
        b.ch(b'}');
    }
    b.a("],\"root\":");
    b.boolean(ss.root);
    b.a(",\"servicesMapping\":[");
    for (i, sm) in ss.services.iter().enumerate() {
        if i > 0 {
            b.ch(b',');
        }
        b.ch(b'{');
        if let Some(po) = &sm.path_offset {
            b.a("\"pathOffset\":{\"path\":[");
            for (k, seg) in po.property_path.iter().enumerate() {
                if k > 0 {
                    b.ch(b',');
                }
                b.a("{\"_type\":\"propertyPath\",\"parameters\":[");
                // parameters().get(k): an IndexOutOfBoundsException where Java's list is shorter
                let Some(arguments) = po.parameters.get(k) else {
                    return Err(crate::doors::JavaError::of(
                        "java.lang.IndexOutOfBoundsException",
                        Some(&format!("Index {k} out of bounds for length {}", po.parameters.len())),
                    ));
                };
                comma_each(b, arguments, value_spec)?;
                b.a("],\"property\":");
                str(b, seg);
                b.ch(b'}');
            }
            b.a("],\"startType\":");
            str(b, &po.start_type);
            b.a("},");
        }
        if let Some(req) = &sm.request {
            b.a("\"requestBuildInfo\":{");
            let mut first = true;
            if let Some(body) = &req.body {
                b.a("\"requestBodyBuildInfo\":{\"sourceInformation\":");
                src_info(b, body.source_information.as_deref());
                b.a(",\"transform\":");
                transform_lambda(b, &body.transform, body.transform_span.as_deref())?;
                b.ch(b'}');
                first = false;
            }
            if let Some(params) = &req.parameters {
                if !first {
                    b.ch(b',');
                }
                b.a("\"requestParametersBuildInfo\":{\"parameterBuildInfoList\":[");
                for (k, e) in params.entries.iter().enumerate() {
                    if k > 0 {
                        b.ch(b',');
                    }
                    b.a("{\"serviceParameter\":");
                    str(b, &e.service_parameter);
                    b.a(",\"sourceInformation\":");
                    src_info(b, e.source_information.as_deref());
                    b.a(",\"transform\":");
                    transform_lambda(b, &e.transform, e.transform_span.as_deref())?;
                    b.ch(b'}');
                }
                b.ch(b']');
                if let Some(s) = &params.source_information {
                    b.a(",\"sourceInformation\":");
                    src_info(b, Some(s));
                }
                b.ch(b'}');
            }
            if let Some(s) = &req.source_information {
                b.a(",\"sourceInformation\":");
                src_info(b, Some(s));
            }
            b.a("},");
        }
        b.a("\"service\":");
        service_ptr(b, &sm.service);
        b.a(",\"sourceInformation\":");
        src_info(b, sm.source_information.as_deref());
        b.ch(b'}');
    }
    b.a("],\"sourceInformation\":");
    src_info(b, ss.source_information.as_deref());
    b.ch(b'}');
    Ok(())
}

/// The nested group/service pointer chain: the leading segments are groups (outermost first), the last the service.
fn service_ptr(b: &mut Buf, ptr: &PServicePtr) {
    let segs = &ptr.segments;
    let last = segs.len() - 1;
    b.ch(b'{');
    if last > 0 {
        b.a("\"parent\":");
        group_ptr(b, ptr, last - 1);
        b.ch(b',');
    }
    b.a("\"service\":");
    str(b, &segs[last].name);
    b.a(",\"serviceStore\":");
    str(b, &ptr.service_store);
    b.a(",\"sourceInformation\":");
    src_info(b, segs[last].source_information.as_deref());
    b.ch(b'}');
}

fn group_ptr(b: &mut Buf, ptr: &PServicePtr, idx: usize) {
    b.ch(b'{');
    if idx > 0 {
        b.a("\"parent\":");
        group_ptr(b, ptr, idx - 1);
        b.ch(b',');
    }
    b.a("\"serviceGroup\":");
    str(b, &ptr.segments[idx].name);
    b.a(",\"serviceStore\":");
    str(b, &ptr.service_store);
    b.a(",\"sourceInformation\":");
    src_info(b, ptr.segments[idx].source_information.as_deref());
    b.ch(b'}');
}

/// A transform expression as a parameterless lambda spanning the expression's own tokens.
fn transform_lambda(b: &mut Buf, expr: &ValueSpecification, span: Option<&SourceInfo>) -> Em {
    b.a("{\"_type\":\"lambda\",\"body\":[");
    value_spec(b, expr)?;
    b.a("],\"parameters\":[],\"sourceInformation\":");
    src_info(b, span);
    b.ch(b'}');
    Ok(())
}

/// `MappingEmitter.mappingInclude`: an included mapping (its one store substitution) or data space.
pub fn mapping_include(b: &mut Buf, inc: &PMappingInclude) -> Em {
    if let Some(ds) = &inc.included_data_space {
        b.a("{\"_type\":\"mappingIncludeDataSpace\",\"includedDataSpace\":");
        str(b, ds);
        b.a(",\"sourceInformation\":");
        src_info(b, inc.source_information.as_deref());
        b.ch(b'}');
        return Ok(());
    }
    // an include's store substitutions: the engine's JSON carries one pair and its parser keeps neither of several;
    // lite's dialect keeps them all (S89)
    let Some(included) = &inc.included_mapping else {
        return Err(npe(None));
    };
    placeless(inc.substitutions.len() <= 1, "an include's several store substitutions", &included.0)?;
    b.a("{\"_type\":\"mappingIncludeMapping\",\"includedMapping\":");
    str(b, included);
    if let Some(s) = &inc.source_database_path {
        b.a(",\"sourceDatabasePath\":");
        str(b, s);
    }
    b.a(",\"sourceInformation\":");
    src_info(b, inc.source_information.as_deref());
    if let Some(t) = &inc.target_database_path {
        b.a(",\"targetDatabasePath\":");
        str(b, t);
    }
    b.ch(b'}');
    Ok(())
}

/// `ClassCastException` of an unchecked `(String)` cast of a leaf of another class.
fn class_cast_to_string(v: &JValue) -> crate::doors::JavaError {
    let c = v.java_class();
    crate::doors::JavaError::of(
        "java.lang.ClassCastException",
        Some(&format!(
            "class {c} cannot be cast to class java.lang.String ({c} and java.lang.String are in module java.base of \
             loader 'bootstrap')"
        )),
    )
}

/// `MappingEmitter.enumerationMapping`.
pub fn enumeration_mapping(b: &mut Buf, em: &PEnumerationMapping) -> Em {
    b.a("{\"enumValueMappings\":[");
    for (j, vm) in em.enum_value_mappings.iter().enumerate() {
        if j > 0 {
            b.ch(b',');
        }
        b.a("{\"enumValue\":");
        str(b, &vm.enum_value);
        b.a(",\"sourceValues\":[");
        for (k, esv) in vm.source_values.iter().enumerate() {
            if k > 0 {
                b.ch(b',');
            }
            if let Some(enumeration) = &esv.enumeration {
                // [my::Other.bla]
                b.a("{\"_type\":\"enumSourceValue\",\"enumeration\":");
                str(b, enumeration);
                b.a(",\"value\":");
                let JValue::String(sv) = &esv.value else {
                    return Err(class_cast_to_string(&esv.value));
                };
                str(b, sv);
            } else if let JValue::String(sv) = &esv.value {
                b.a("{\"_type\":\"stringSourceValue\",\"value\":");
                str(b, sv);
            } else {
                b.a("{\"_type\":\"integerSourceValue\",\"value\":");
                crate::emitter::append_value(b, &esv.value);
            }
            b.ch(b'}');
        }
        b.a("]}");
    }
    b.a("],\"enumeration\":");
    pointer(b, &em.enumeration);
    if let Some(id) = &em.id {
        b.a(",\"id\":");
        str(b, id);
    }
    b.a(",\"sourceInformation\":");
    src_info(b, em.source_information.as_deref());
    b.ch(b'}');
    Ok(())
}

/// `MappingEmitter.relClassMapping`: `_type:"relational"`.
pub fn rel_class_mapping(b: &mut Buf, cm: &PClassMappingRel) -> Em {
    b.a("{\"_type\":\"relational\",\"class\":");
    str(b, &cm.class_name);
    b.a(",\"classSourceInformation\":");
    src_info(b, cm.class_source_information.as_deref());
    b.a(",\"distinct\":");
    b.boolean(cm.distinct);
    if let Some(e) = &cm.extends_class_mapping_id {
        b.a(",\"extendsClassMappingId\":");
        str(b, e);
    }
    if let Some(f) = &cm.filter {
        b.a(",\"filter\":");
        filter_mapping(b, f);
    }
    b.a(",\"groupBy\":[");
    for (i, g) in cm.group_by.iter().enumerate() {
        if i > 0 {
            b.ch(b',');
        }
        rel_op(b, g, true);
    }
    b.ch(b']');
    if let Some(id) = &cm.id {
        b.a(",\"id\":");
        str(b, id);
    }
    if let Some(t) = &cm.main_table {
        b.a(",\"mainTable\":");
        table_ptr(b, t);
    }
    b.a(",\"primaryKey\":[");
    for (i, k) in cm.primary_key.iter().enumerate() {
        if i > 0 {
            b.ch(b',');
        }
        rel_op(b, k, true);
    }
    b.a("],\"propertyMappings\":[");
    comma_each(b, &cm.property_mappings, rel_property_mapping)?;
    b.a("],\"root\":");
    b.boolean(cm.root);
    b.a(",\"sourceInformation\":");
    src_info(b, cm.source_information.as_deref());
    b.ch(b'}');
    Ok(())
}

/// `_type:"functionInstance"`: the clean-sheet function form, `function` and `bodyLambda` exclusive.
pub fn function_class_mapping(b: &mut Buf, cm: &PClassMappingFunction) -> Em {
    b.a("{\"_type\":\"functionInstance\"");
    if let Some(l) = &cm.body_lambda {
        b.a(",\"bodyLambda\":");
        lambda(b, l)?;
    }
    b.a(",\"class\":");
    str(b, &cm.class_name);
    b.a(",\"classSourceInformation\":");
    src_info(b, cm.class_source_information.as_deref());
    if let Some(e) = &cm.extends_class_mapping_id {
        b.a(",\"extendsClassMappingId\":");
        str(b, e);
    }
    if let Some(f) = &cm.function {
        b.a(",\"function\":");
        element_ptr(b, &f.full_path, f.pos.as_deref());
    }
    if let Some(id) = &cm.id {
        b.a(",\"id\":");
        str(b, id);
    }
    b.a(",\"kind\":");
    str(b, &cm.kind);
    b.a(",\"root\":");
    b.boolean(cm.root);
    b.a(",\"sourceInformation\":");
    src_info(b, cm.source_information.as_deref());
    b.ch(b'}');
    Ok(())
}

/// A local mapping property: `{"multiplicity":{...},"sourceInformation":...,"type":...}`.
fn local_prop(b: &mut Buf, lp: &PLocalProp) {
    b.a("{\"multiplicity\":{\"lowerBound\":");
    b.int(lp.lower_bound);
    if let Some(up) = lp.upper_bound {
        b.a(",\"upperBound\":");
        b.int(up);
    }
    b.a("},\"sourceInformation\":");
    src_info(b, lp.source_information.as_deref());
    b.a(",\"type\":");
    str(b, &lp.r#type);
    b.ch(b'}');
}

/// `_type:"pureInstance"`.
pub fn pure_class_mapping(b: &mut Buf, cm: &PClassMappingPure) -> Em {
    b.a("{\"_type\":\"pureInstance\",\"class\":");
    str(b, &cm.class_name);
    b.a(",\"classSourceInformation\":");
    src_info(b, cm.class_source_information.as_deref());
    if let Some(e) = &cm.extends_class_mapping_id {
        b.a(",\"extendsClassMappingId\":");
        str(b, e);
    }
    if let Some(fb) = &cm.filter {
        // ~filter lambda rides between the class span and id
        b.a(",\"filter\":{\"_type\":\"lambda\",\"body\":[");
        comma_each(b, fb, value_spec)?;
        b.a("],\"parameters\":[]}");
    }
    if let Some(id) = &cm.id {
        b.a(",\"id\":");
        str(b, id);
    }
    b.a(",\"propertyMappings\":[");
    for (i, pm) in cm.property_mappings.iter().enumerate() {
        if i > 0 {
            b.ch(b',');
        }
        b.a("{\"_type\":\"purePropertyMapping\",");
        if let Some(e) = &pm.enum_mapping_id {
            b.a("\"enumMappingId\":");
            str(b, e);
            b.ch(b',');
        }
        b.a("\"explodeProperty\":");
        b.boolean(pm.explode_property);
        if let Some(lp) = &pm.local_mapping_property {
            b.a(",\"localMappingProperty\":");
            local_prop(b, lp);
        }
        b.a(",\"property\":{");
        if let Some(owner) = &pm.owner_class {
            b.a("\"class\":");
            str(b, owner);
            b.ch(b',');
        }
        b.a("\"property\":");
        str(b, &pm.property);
        b.a(",\"sourceInformation\":");
        src_info(b, pm.property_source_information.as_deref());
        b.ch(b'}');
        if let Some(s) = &pm.source {
            b.a(",\"source\":");
            str(b, s);
        }
        b.a(",\"sourceInformation\":");
        src_info(b, pm.source_information.as_deref());
        if let Some(t) = &pm.target {
            b.a(",\"target\":");
            str(b, t);
        }
        b.a(",\"transform\":{\"_type\":\"lambda\",\"body\":[");
        comma_each(b, &pm.transform, value_spec)?;
        b.a("],\"parameters\":[]}}");
    }
    b.a("],\"root\":");
    b.boolean(cm.root);
    if cm.src_class.is_some()
        && let Some(s) = &cm.source_class_source_information
    {
        b.a(",\"sourceClassSourceInformation\":");
        src_info(b, Some(s));
    }
    b.a(",\"sourceInformation\":");
    src_info(b, cm.source_information.as_deref());
    if let Some(s) = &cm.src_class {
        b.a(",\"srcClass\":");
        str(b, s);
    }
    b.ch(b'}');
    Ok(())
}

/// `_type:"aggregationAware"`.
pub fn aggregation_aware(b: &mut Buf, aa: &PClassMappingAggregationAware) -> Em {
    b.a("{\"_type\":\"aggregationAware\",\"aggregateSetImplementations\":[");
    for (i, asi) in aa.aggregate_set_implementations.iter().enumerate() {
        if i > 0 {
            b.ch(b',');
        }
        b.a("{\"aggregateSpecification\":{\"aggregateValues\":[");
        for (j, av) in asi.aggregate_values.iter().enumerate() {
            if j > 0 {
                b.ch(b',');
            }
            b.a("{\"aggregateFn\":");
            lambda_envelope(b, &av.aggregate_fn)?;
            b.a(",\"mapFn\":");
            lambda_envelope(b, &av.map_fn)?;
            b.ch(b'}');
        }
        b.a("],\"canAggregate\":");
        b.boolean(asi.can_aggregate);
        b.a(",\"groupByFunctions\":[");
        for (j, g) in asi.group_by_functions.iter().enumerate() {
            if j > 0 {
                b.ch(b',');
            }
            b.a("{\"groupByFn\":");
            lambda_envelope(b, g)?;
            b.ch(b'}');
        }
        b.a("]},\"index\":");
        b.int(i64::from(asi.index));
        b.a(",\"setImplementation\":");
        nested_class_mapping(b, &asi.set_implementation)?;
        b.ch(b'}');
    }
    b.a("],\"class\":");
    str(b, &aa.class_name);
    extends_id(b, aa.extends_class_mapping_id.as_ref());
    b.a(",\"id\":");
    str(b, &aa.id);
    b.a(",\"mainSetImplementation\":");
    nested_class_mapping(b, &aa.main_set_implementation)?;
    if !aa.agg_property_mappings.is_empty() {
        // Pure main mappings copy their property mappings onto the aggregation-aware node
        b.a(",\"propertyMappings\":[");
        for (i, pm) in aa.agg_property_mappings.iter().enumerate() {
            if i > 0 {
                b.ch(b',');
            }
            b.a("{\"_type\":\"AggregationAwarePropertyMapping\",\"property\":{\"class\":");
            let Some(owner) = &pm.owner_class else {
                return Err(npe(None));
            };
            str(b, owner);
            b.a(",\"property\":");
            str(b, &pm.property);
            b.a(",\"sourceInformation\":");
            src_info(b, pm.property_source_information.as_deref());
            b.ch(b'}');
            if let Some(s) = &pm.source {
                b.a(",\"source\":");
                str(b, s);
            }
            b.a(",\"sourceInformation\":");
            src_info(b, pm.source_information.as_deref());
            if let Some(t) = &pm.target {
                b.a(",\"target\":");
                str(b, t);
            }
            b.ch(b'}');
        }
        b.ch(b']');
    }
    b.a(",\"root\":");
    b.boolean(aa.root);
    b.a(",\"sourceInformation\":");
    src_info(b, aa.source_information.as_deref());
    b.ch(b'}');
    Ok(())
}

/// `{_type:lambda, body:[expr], parameters:[]}` with no span: the aggregate-lambda envelope.
pub fn lambda_envelope(b: &mut Buf, expr: &ValueSpecification) -> Em {
    b.a("{\"_type\":\"lambda\",\"body\":[");
    value_spec(b, expr)?;
    b.a("],\"parameters\":[]}");
    Ok(())
}

/// `MappingEmitter.testSuite`: `_type:"mappingTestSuite"`.
pub fn test_suite(b: &mut Buf, ts: &PMappingTestSuite) -> Em {
    b.a("{\"_type\":\"mappingTestSuite\"");
    if let Some(doc) = &ts.doc {
        b.a(",\"doc\":");
        str(b, doc);
    }
    b.a(",\"func\":");
    value_spec(b, &ts.func)?;
    b.a(",\"id\":");
    str(b, &ts.id);
    b.a(",\"sourceInformation\":");
    src_info(b, ts.source_information.as_deref());
    b.a(",\"tests\":[");
    for (i, t) in ts.tests.iter().enumerate() {
        if i > 0 {
            b.ch(b',');
        }
        b.a("{\"_type\":\"mappingTest\",\"assertions\":[");
        for (j, a) in t.assertions.iter().enumerate() {
            if j > 0 {
                b.ch(b',');
            }
            match &a.expected {
                PAssertionValue::PExternalFormatData(ef) => {
                    b.a("{\"_type\":\"equalToJson\",\"expected\":");
                    external_format_data(b, ef)?;
                }
                PAssertionValue::PRelationElement(re) => {
                    b.a("{\"_type\":\"equalToRelation\",\"expected\":");
                    relation_element(b, re)?;
                }
                PAssertionValue::PEqualToValue(ev) => {
                    b.a("{\"_type\":\"equalTo\",\"expected\":");
                    value_spec(b, &ev.value)?;
                }
            }
            b.a(",\"id\":");
            str(b, &a.id);
            b.a(",\"sourceInformation\":");
            src_info(b, a.source_information.as_deref());
            b.ch(b'}');
        }
        b.ch(b']');
        if let Some(doc) = &t.doc {
            b.a(",\"doc\":");
            str(b, doc);
        }
        b.a(",\"id\":");
        str(b, &t.id);
        b.a(",\"sourceInformation\":");
        src_info(b, t.source_information.as_deref());
        b.a(",\"storeTestData\":[");
        for (j, sd) in t.store_test_data.iter().enumerate() {
            if j > 0 {
                b.ch(b',');
            }
            store_test_data(b, sd)?;
        }
        b.a("]}");
    }
    b.a("]}");
    Ok(())
}

fn store_test_data(b: &mut Buf, sd: &PStoreTestData) -> Em {
    if let Some(rels) = &sd.relation_elements {
        // Relation #{ schema.table: CSV...; }#
        b.a("{\"data\":{\"_type\":\"relationAccessor\",\"relationElements\":[");
        comma_each(b, rels, |b, re| relation_element(b, re))?;
        b.a("],\"sourceInformation\":");
        src_info(b, sd.relation_accessor_source_information.as_deref());
        b.a("},\"sourceInformation\":");
        src_info(b, sd.source_information.as_deref());
        b.a(",\"store\":");
        pointer(b, &sd.store);
        b.ch(b'}');
        return Ok(());
    }
    if let Some(embedded) = &sd.embedded {
        // a plain embedded-data value under the store pointer
        b.a("{\"data\":");
        embedded_data_value(b, embedded)?;
        b.a(",\"sourceInformation\":");
        src_info(b, sd.source_information.as_deref());
        b.a(",\"store\":");
        pointer(b, &sd.store);
        b.ch(b'}');
        return Ok(());
    }
    if let Some(de) = &sd.data_element {
        // Reference #{ path }#
        b.a("{\"data\":{\"_type\":\"reference\",\"dataElement\":");
        pointer(b, de);
        b.a(",\"sourceInformation\":");
        src_info(b, de.source_information.as_deref());
        b.a("},\"sourceInformation\":");
        src_info(b, sd.source_information.as_deref());
        b.a(",\"store\":");
        pointer(b, &sd.store);
        b.ch(b'}');
        return Ok(());
    }
    let Some(mdl) = &sd.model_data else {
        return Err(npe(None));
    };
    b.a("{\"data\":{\"_type\":\"modelStore\",\"modelData\":[");
    comma_each(b, mdl, model_data_entry)?;
    b.a("],\"sourceInformation\":");
    src_info(b, sd.model_store_source_information.as_deref());
    b.a("},\"sourceInformation\":");
    src_info(b, sd.source_information.as_deref());
    b.a(",\"store\":");
    pointer(b, &sd.store);
    b.ch(b'}');
    Ok(())
}

/// A class mapping's Java class (`Class.toString()`'s name).
fn class_mapping_class(cm: &PClassMapping) -> &'static str {
    match cm {
        PClassMapping::PClassMappingRel(_) => "com.legend.protocol.Protocol$PClassMappingRel",
        PClassMapping::PClassMappingPure(_) => "com.legend.protocol.Protocol$PClassMappingPure",
        PClassMapping::PClassMappingOperation(_) => "com.legend.protocol.Protocol$PClassMappingOperation",
        PClassMapping::PClassMappingRelation(_) => "com.legend.protocol.Protocol$PClassMappingRelation",
        PClassMapping::PClassMappingMergeOperation(_) => "com.legend.protocol.Protocol$PClassMappingMergeOperation",
        PClassMapping::PClassMappingAggregationAware(_) => {
            "com.legend.protocol.Protocol$PClassMappingAggregationAware"
        }
        PClassMapping::PClassMappingFunction(_) => "com.legend.protocol.Protocol$PClassMappingFunction",
        PClassMapping::PServiceStoreClassMapping(_) => "com.legend.protocol.Protocol$PServiceStoreClassMapping",
        PClassMapping::PClassMappingMongoDb(_) => "com.legend.protocol.Protocol$PClassMappingMongoDb",
    }
}

/// An aggregation-aware mapping's nested set implementation: relational, Pure or function.
pub fn nested_class_mapping(b: &mut Buf, cm: &PClassMapping) -> Em {
    match cm {
        PClassMapping::PClassMappingRel(r) => rel_class_mapping(b, r),
        PClassMapping::PClassMappingPure(pu) => pure_class_mapping(b, pu),
        PClassMapping::PClassMappingFunction(fi) => function_class_mapping(b, fi),
        other => Err(illegal_state(JStr::from(format!(
            "nested class-mapping kind unbuilt: class {}",
            class_mapping_class(other)
        )))),
    }
}

/// `prop ( street: COL, ... )` in a Relation mapping: embedded, its nested property mappings classless.
pub fn relation_fn_embedded(b: &mut Buf, pm: &PRelationFnPropertyMapping) -> Em {
    b.a("{\"_type\":\"relationFunctionEmbeddedPropertyMapping\",\"property\":{\"class\":");
    let Some(owner) = &pm.owner_class else {
        return Err(npe(None));
    };
    str(b, owner);
    b.a(",\"property\":");
    str(b, &pm.property);
    b.a(",\"sourceInformation\":");
    src_info(b, pm.property_source_information.as_deref());
    b.a("},\"propertyMappings\":[");
    let Some(nested) = &pm.nested else {
        return Err(npe(None));
    };
    comma_each(b, nested, |b, n| relation_fn_pm(b, n))?;
    b.ch(b']');
    if let Some(s) = &pm.source {
        b.a(",\"source\":");
        str(b, s);
    }
    b.a(",\"sourceInformation\":");
    src_info(b, pm.source_information.as_deref());
    b.ch(b'}');
    Ok(())
}

/// One `relationFunctionPropertyMapping`, shared by the outer arm and the embedded one.
pub fn relation_fn_pm(b: &mut Buf, pm: &PRelationFnPropertyMapping) -> Em {
    b.a("{\"_type\":\"relationFunctionPropertyMapping\"");
    if let Some(t) = &pm.binding_transformer {
        b.a(",\"bindingTransformer\":{\"binding\":");
        str(b, t);
        b.ch(b'}');
    }
    if let Some(c) = &pm.column {
        b.a(",\"column\":");
        str(b, c);
    }
    if let Some(e) = &pm.enum_mapping_id {
        b.a(",\"enumMappingId\":");
        str(b, e);
    }
    if let Some(lp) = &pm.local_mapping_property {
        b.a(",\"localMappingProperty\":");
        local_prop(b, lp);
    }
    b.a(",\"property\":{");
    if let Some(owner) = &pm.owner_class {
        b.a("\"class\":");
        str(b, owner);
        b.ch(b',');
    }
    b.a("\"property\":");
    str(b, &pm.property);
    b.a(",\"sourceInformation\":");
    src_info(b, pm.property_source_information.as_deref());
    b.ch(b'}');
    if let Some(s) = &pm.source {
        b.a(",\"source\":");
        str(b, s);
    }
    b.a(",\"sourceInformation\":");
    src_info(b, pm.source_information.as_deref());
    if let Some(expr) = &pm.expr {
        b.a(",\"valueFn\":{\"_type\":\"lambda\",\"body\":[");
        value_spec(b, expr)?;
        b.a("],\"parameters\":[],\"sourceInformation\":");
        src_info(b, pm.expr_lambda_source_information.as_deref());
        b.ch(b'}');
    }
    b.ch(b'}');
    Ok(())
}

/// `MappingEmitter.relationElement`: the bare columns/paths/rows shape, every cell a string.
pub fn relation_element(b: &mut Buf, re: &PRelationElement) -> Em {
    b.a("{\"columns\":[");
    strs_each(b, &re.columns);
    b.a("],\"paths\":[");
    strs_each(b, &re.paths);
    b.a("],\"rows\":[");
    for (i, row) in re.rows.iter().enumerate() {
        if i > 0 {
            b.ch(b',');
        }
        b.a("{\"values\":[");
        strs_each(b, row);
        b.a("]}");
    }
    b.a("],\"sourceInformation\":");
    src_info(b, re.source_information.as_deref());
    b.ch(b'}');
    Ok(())
}

/// One `path: <payload>` ModelStore entry: an embedded value or an instance collection.
fn model_data_entry(b: &mut Buf, md: &PModelData) -> Em {
    let (model, span) = match md {
        PModelData::PModelEmbeddedData(e) => {
            b.a("{\"_type\":\"modelEmbeddedData\",\"data\":");
            embedded_data_value(b, &e.data)?;
            (&e.model, e.source_information.as_deref())
        }
        PModelData::PModelInstanceData(i) => {
            b.a("{\"_type\":\"modelInstanceData\",\"instances\":");
            instances_collection(b, &i.instances)?;
            (&i.model, i.source_information.as_deref())
        }
    };
    b.a(",\"model\":");
    str(b, model);
    b.a(",\"sourceInformation\":");
    src_info(b, span);
    b.ch(b'}');
    Ok(())
}

/// A stub's parameter map, its entries as the record sorts them: `"<field>":{...},`.
fn stub_params(b: &mut Buf, field: &str, params: &[Box<PStubParam>]) {
    b.ch(b'"');
    b.a(field);
    b.a("\":{");
    for (i, p) in params.iter().enumerate() {
        if i > 0 {
            b.ch(b',');
        }
        str(b, &p.name);
        b.ch(b':');
        string_value_pattern(b, &p.pattern);
    }
    b.a("},");
}

fn string_value_pattern(b: &mut Buf, v: &PStringValuePattern) {
    b.a("{\"_type\":");
    str(b, &v.r#type);
    b.a(",\"expectedValue\":");
    str(b, &v.expected_value);
    b.ch(b'}');
}

/// `MappingEmitter.embeddedDataValue`.
pub fn embedded_data_value(b: &mut Buf, v: &PEmbeddedDataValue) -> Em {
    match v {
        PEmbeddedDataValue::PServiceStoreData(ss) => {
            b.a("{\"_type\":\"serviceStore\",\"serviceStubMappings\":[");
            for (i, st) in ss.stubs.iter().enumerate() {
                if i > 0 {
                    b.ch(b',');
                }
                b.a("{\"requestPattern\":{");
                if let Some(bp) = &st.body_patterns {
                    b.a("\"bodyPatterns\":[");
                    for (k, p) in bp.iter().enumerate() {
                        if k > 0 {
                            b.ch(b',');
                        }
                        string_value_pattern(b, p);
                    }
                    b.a("],");
                }
                if let Some(hp) = &st.header_params {
                    stub_params(b, "headerParams", hp);
                }
                b.a("\"method\":");
                str(b, &st.method);
                if let Some(qp) = &st.query_params {
                    b.ch(b',');
                    stub_params(b, "queryParams", qp);
                    b.u.pop(); // drop the trailing comma
                }
                b.a(",\"sourceInformation\":");
                src_info(b, st.request_span.as_deref());
                if let Some(u) = &st.url {
                    b.a(",\"url\":");
                    str(b, u);
                }
                if let Some(u) = &st.url_path {
                    b.a(",\"urlPath\":");
                    str(b, u);
                }
                b.a("},\"responseDefinition\":{\"body\":");
                external_format_data(b, &st.body)?;
                b.a(",\"sourceInformation\":");
                src_info(b, st.response_span.as_deref());
                b.a("},\"sourceInformation\":");
                src_info(b, st.source_information.as_deref());
                b.ch(b'}');
            }
            b.a("],\"sourceInformation\":");
            src_info(b, ss.source_information.as_deref());
            b.ch(b'}');
        }
        PEmbeddedDataValue::PExternalFormatData(ef) => external_format_data(b, ef)?,
        PEmbeddedDataValue::PModelStoreData(ms) => {
            b.a("{\"_type\":\"modelStore\",\"modelData\":[");
            comma_each(b, &ms.model_data, model_data_entry)?;
            b.a("],\"sourceInformation\":");
            src_info(b, ms.source_information.as_deref());
            b.ch(b'}');
        }
        PEmbeddedDataValue::PRelationData(rd) => {
            b.a("{\"_type\":\"relationAccessor\",\"relationElements\":[");
            comma_each(b, &rd.relation_elements, |b, re| relation_element(b, re))?;
            b.a("],\"sourceInformation\":");
            src_info(b, rd.source_information.as_deref());
            b.ch(b'}');
        }
        PEmbeddedDataValue::PRelationalCsvData(rc) => {
            b.a("{\"_type\":\"relationalCSVData\",\"sourceInformation\":");
            src_info(b, rc.source_information.as_deref());
            b.a(",\"tables\":[");
            for (i, t) in rc.tables.iter().enumerate() {
                if i > 0 {
                    b.ch(b',');
                }
                b.a("{\"schema\":");
                str(b, &t.schema);
                b.a(",\"sourceInformation\":");
                src_info(b, t.source_information.as_deref());
                b.a(",\"table\":");
                str(b, &t.table);
                if let Some(v) = &t.values {
                    b.a(",\"values\":");
                    str(b, v);
                }
                b.ch(b'}');
            }
            b.a("]}");
        }
        PEmbeddedDataValue::PDataReference(dr) => {
            b.a("{\"_type\":\"reference\",\"dataElement\":");
            pointer(b, &dr.data_element);
            b.a(",\"sourceInformation\":");
            src_info(b, dr.source_information.as_deref());
            b.ch(b'}');
        }
    }
    Ok(())
}

/// `MappingEmitter.externalFormatData`: `_type:"externalFormat"`.
pub fn external_format_data(b: &mut Buf, ef: &PExternalFormatData) -> Em {
    b.a("{\"_type\":\"externalFormat\",\"contentType\":");
    str(b, &ef.content_type);
    b.a(",\"data\":");
    str(b, &ef.data);
    b.a(",\"sourceInformation\":");
    src_info(b, ef.source_information.as_deref());
    b.ch(b'}');
    Ok(())
}

/// Older JSON's embedded set `class`, written back where it came from.
fn embedded_class(b: &mut Buf, embedded_class: Option<&JStr>) {
    if let Some(c) = embedded_class {
        b.a(",\"class\":");
        str(b, c);
    }
}

/// A relational operation's span (`PRelOp.sourceInformation()`).
fn rel_op_span(op: &PRelOp) -> Option<&SourceInfo> {
    match op {
        PRelOp::PDynaFunc(x) => x.source_information.as_deref(),
        PRelOp::PColumnRef(x) => x.source_information.as_deref(),
        PRelOp::PRelLiteral(x) => x.source_information.as_deref(),
        PRelOp::PRelLiteralList(x) => x.source_information.as_deref(),
        PRelOp::PElemtWithJoins(x) => x.source_information.as_deref(),
        PRelOp::PRelLambda(x) => x.source_information.as_deref(),
        PRelOp::PLambdaParam(x) => x.source_information.as_deref(),
    }
}

/// A property mapping's `property` object: `{["class":..,]"property":..,"sourceInformation":..}`.
fn property_ref(b: &mut Buf, owner: Option<&JStr>, property: &JStr, span: Option<&SourceInfo>) {
    b.a("\"property\":{");
    if let Some(o) = owner {
        b.a("\"class\":");
        str(b, o);
        b.ch(b',');
    }
    b.a("\"property\":");
    str(b, property);
    b.a(",\"sourceInformation\":");
    src_info(b, span);
    b.ch(b'}');
}

/// `MappingEmitter.relPropertyMapping`: a relational class mapping's property mappings.
pub fn rel_property_mapping(b: &mut Buf, pmi: &PPropertyMapping) -> Em {
    match pmi {
        PPropertyMapping::PRelPropertyMapping(pm) => {
            b.a("{\"_type\":\"relationalPropertyMapping\",");
            if let Some(t) = &pm.binding_transformer {
                b.a("\"bindingTransformer\":{\"binding\":");
                str(b, t);
                b.a("},");
            }
            if let Some(e) = &pm.enum_mapping_id {
                b.a("\"enumMappingId\":");
                str(b, e);
                b.ch(b',');
            }
            if let Some(lp) = &pm.local_mapping_property {
                b.a("\"localMappingProperty\":");
                local_prop(b, lp);
                b.ch(b',');
            }
            property_ref(b, pm.owner_class.as_ref(), &pm.property, pm.property_source_information.as_deref());
            b.a(",\"relationalOperation\":");
            rel_op(b, &pm.relational_operation, true);
            if let Some(s) = &pm.source {
                b.a(",\"source\":");
                str(b, s);
            }
            b.a(",\"sourceInformation\":");
            src_info(b, pm.source_information.as_deref());
            if let Some(t) = &pm.target {
                b.a(",\"target\":");
                str(b, t);
            }
            b.ch(b'}');
        }
        PPropertyMapping::PEmbeddedPropertyMapping(em) => {
            b.a("{\"_type\":\"embeddedPropertyMapping\",\"classMapping\":{\"_type\":\"embedded\"");
            embedded_class(b, em.embedded_class.as_ref());
            if let Some(id) = &em.id {
                b.a(",\"id\":");
                str(b, id);
            }
            b.a(",\"primaryKey\":[");
            for (i, k) in em.primary_key.iter().enumerate() {
                if i > 0 {
                    b.ch(b',');
                }
                rel_op(b, k, true);
            }
            b.a("],\"propertyMappings\":[");
            comma_each(b, &em.property_mappings, rel_property_mapping)?;
            b.a("],\"root\":false,\"sourceInformation\":");
            src_info(b, em.source_information.as_deref());
            b.ch(b'}');
            if let Some(id) = &em.id {
                b.a(",\"id\":");
                str(b, id);
            }
            b.ch(b',');
            property_ref(b, em.owner_class.as_ref(), &em.property, em.property_source_information.as_deref());
            b.a(",\"sourceInformation\":");
            src_info(b, em.source_information.as_deref());
            if let Some(id) = &em.id {
                b.a(",\"target\":");
                str(b, id);
            }
            b.ch(b'}');
        }
        PPropertyMapping::PInlineEmbeddedPropertyMapping(ie) => {
            b.a("{\"_type\":\"inlineEmbeddedPropertyMapping\"");
            if let Some(id) = &ie.id {
                b.a(",\"id\":");
                str(b, id);
            }
            b.ch(b',');
            property_ref(b, ie.owner_class.as_ref(), &ie.property, ie.property_source_information.as_deref());
            b.a(",\"setImplementationId\":");
            str(b, &ie.set_implementation_id);
            b.a(",\"sourceInformation\":");
            src_info(b, ie.source_information.as_deref());
            b.ch(b'}');
        }
        PPropertyMapping::POtherwiseEmbeddedPropertyMapping(oe) => {
            b.a("{\"_type\":\"otherwiseEmbeddedPropertyMapping\",\"classMapping\":{\"_type\":\"embedded\"");
            embedded_class(b, oe.embedded_class.as_ref());
            if let Some(id) = &oe.id {
                b.a(",\"id\":");
                str(b, id);
            }
            b.a(",\"primaryKey\":[");
            for (i, k) in oe.primary_key.iter().enumerate() {
                if i > 0 {
                    b.ch(b',');
                }
                rel_op(b, k, true);
            }
            b.a("],\"propertyMappings\":[");
            comma_each(b, &oe.property_mappings, rel_property_mapping)?;
            b.a("],\"root\":false,\"sourceInformation\":");
            src_info(b, oe.class_mapping_source_information.as_deref());
            b.a("},\"otherwisePropertyMapping\":{\"_type\":\"relationalPropertyMapping\",");
            property_ref(b, oe.owner_class.as_ref(), &oe.property, oe.property_source_information.as_deref());
            b.a(",\"relationalOperation\":");
            rel_op(b, &oe.otherwise_op, true);
            b.a(",\"sourceInformation\":");
            src_info(b, rel_op_span(&oe.otherwise_op));
            b.a(",\"target\":");
            str(b, &oe.otherwise_target);
            b.a("},");
            property_ref(b, oe.owner_class.as_ref(), &oe.property, oe.property_source_information.as_deref());
            b.a(",\"sourceInformation\":");
            src_info(b, oe.source_information.as_deref());
            b.ch(b'}');
        }
    }
    Ok(())
}

/// A relational class mapping's `~filter`: the filter pointer and the join chain reaching it.
pub fn filter_mapping(b: &mut Buf, f: &PFilterMapping) {
    b.a("{\"filter\":{\"db\":");
    str(b, &f.db);
    b.a(",\"name\":");
    str(b, &f.name);
    b.a("},\"joins\":[");
    for (i, j) in f.joins.iter().enumerate() {
        if i > 0 {
            b.ch(b',');
        }
        join_ptr(b, j);
    }
    b.a("],\"sourceInformation\":");
    src_info(b, f.source_information.as_deref());
    b.ch(b'}');
}
