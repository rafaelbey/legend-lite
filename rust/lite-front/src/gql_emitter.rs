// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! core/'s `com.legend.protocol.GqlEmitter`, ported (docs/rust-port/PHASE2.md §4 step 5): a GraphQL island's document
//! as the engine's GraphQL AST wire (`executableDocument` and its definitions, selections, values and type
//! references): fields alphabetical per node, a bare-selection operation without `type`, an alias with its colon, a
//! variable definition's name with its `$`.

use crate::emitter::{Buf, Em, str};
use crate::records::*;

/// `GqlEmitter.document`.
pub fn document(b: &mut Buf, doc: &GqlDocument) -> Em {
    b.a("{\"_type\":\"executableDocument\",\"definitions\":[");
    for (i, d) in doc.definitions.iter().enumerate() {
        if i > 0 {
            b.ch(b',');
        }
        definition(b, d);
    }
    b.a("]}");
    Ok(())
}

fn definition(b: &mut Buf, d: &GqlDefinition) {
    match d {
        GqlDefinition::GqlOperation(op) => {
            b.a("{\"_type\":\"operationDefinition\",\"directives\":");
            directives(b, &op.directives);
            if let Some(name) = &op.name {
                b.a(",\"name\":");
                str(b, name);
            }
            b.a(",\"selectionSet\":");
            selections(b, &op.selection_set);
            if let Some(t) = &op.r#type {
                b.a(",\"type\":");
                str(b, t);
            }
            b.a(",\"variables\":[");
            for (i, v) in op.variables.iter().enumerate() {
                if i > 0 {
                    b.ch(b',');
                }
                variable_def(b, v);
            }
            b.a("]}");
        }
        GqlDefinition::GqlFragment(f) => {
            b.a("{\"_type\":\"fragmentDefinition\",\"directives\":");
            directives(b, &f.directives);
            b.a(",\"name\":");
            str(b, &f.name);
            b.a(",\"selectionSet\":");
            selections(b, &f.selection_set);
            b.a(",\"typeCondition\":");
            str(b, &f.type_condition);
            b.ch(b'}');
        }
        GqlDefinition::GqlObjectType(t) => {
            b.a("{\"_type\":\"objectTypeDefinition\",\"_implements\":");
            strings(b, &t.implements_list);
            b.a(",\"directives\":[],\"fields\":");
            field_defs(b, &t.fields);
            b.a(",\"name\":");
            str(b, &t.name);
            b.ch(b'}');
        }
        GqlDefinition::GqlSchemaDef(s) => {
            b.a("{\"_type\":\"schemaDefinition\",\"directives\":");
            directives(b, &s.directives);
            b.a(",\"rootOperationTypeDefinitions\":[");
            for (i, r) in s.root_ops.iter().enumerate() {
                if i > 0 {
                    b.ch(b',');
                }
                // the operation type appended raw, unescaped
                b.a("{\"operationType\":\"");
                b.units(&r.operation_type.0);
                b.a("\",\"type\":");
                str(b, &r.r#type);
                b.ch(b'}');
            }
            b.a("]}");
        }
        GqlDefinition::GqlDirectiveDef(dd) => {
            b.a("{\"_type\":\"directiveDefinition\",\"argumentDefinitions\":");
            input_values(b, &dd.args);
            b.a(",\"executableLocation\":");
            strings(b, &dd.exec_locations);
            b.a(",\"name\":");
            str(b, &dd.name);
            b.a(",\"typeSystemLocation\":");
            strings(b, &dd.type_system_locations);
            b.ch(b'}');
        }
        GqlDefinition::GqlScalarType(st) => {
            b.a("{\"_type\":\"scalarTypeDefinition\",\"directives\":");
            directives(b, &st.directives);
            b.a(",\"name\":");
            str(b, &st.name);
            b.ch(b'}');
        }
        GqlDefinition::GqlInterfaceType(it) => {
            b.a("{\"_type\":\"interfaceTypeDefinition\",\"_implements\":");
            strings(b, &it.implements_list);
            b.a(",\"directives\":");
            directives(b, &it.directives);
            b.a(",\"fields\":");
            field_defs(b, &it.fields);
            b.a(",\"name\":");
            str(b, &it.name);
            b.ch(b'}');
        }
        GqlDefinition::GqlUnionType(ut) => {
            b.a("{\"_type\":\"unionTypeDefinition\",\"directives\":");
            directives(b, &ut.directives);
            b.a(",\"members\":");
            strings(b, &ut.members);
            b.a(",\"name\":");
            str(b, &ut.name);
            b.ch(b'}');
        }
        GqlDefinition::GqlEnumType(et) => {
            b.a("{\"_type\":\"enumTypeDefinition\",\"directives\":");
            directives(b, &et.directives);
            b.a(",\"name\":");
            str(b, &et.name);
            b.a(",\"values\":[");
            for (i, v) in et.values.iter().enumerate() {
                if i > 0 {
                    b.ch(b',');
                }
                b.a("{\"directives\":");
                directives(b, &v.directives);
                b.a(",\"value\":");
                str(b, &v.value);
                b.ch(b'}');
            }
            b.a("]}");
        }
        GqlDefinition::GqlInputObjectType(iot) => {
            b.a("{\"_type\":\"inputObjectTypeDefinition\",\"directives\":");
            directives(b, &iot.directives);
            b.a(",\"fields\":");
            input_values(b, &iot.fields);
            b.a(",\"name\":");
            str(b, &iot.name);
            b.ch(b'}');
        }
    }
}

fn strings(b: &mut Buf, xs: &[crate::jtypes::JStr]) {
    b.ch(b'[');
    for (i, x) in xs.iter().enumerate() {
        if i > 0 {
            b.ch(b',');
        }
        str(b, x);
    }
    b.ch(b']');
}

/// SDL field definitions: `argumentDefinitions, directives, name, type`, no `_type`.
fn field_defs(b: &mut Buf, fields: &[Box<GqlFieldDef>]) {
    b.ch(b'[');
    for (i, fd) in fields.iter().enumerate() {
        if i > 0 {
            b.ch(b',');
        }
        b.a("{\"argumentDefinitions\":");
        input_values(b, &fd.args);
        b.a(",\"directives\":");
        directives(b, &fd.directives);
        b.a(",\"name\":");
        str(b, &fd.name);
        b.a(",\"type\":");
        type_ref(b, &fd.r#type);
        b.ch(b'}');
    }
    b.ch(b']');
}

/// Input values: `[defaultValue,] directives, name, type`, no `_type`.
fn input_values(b: &mut Buf, ivs: &[Box<GqlInputValueDef>]) {
    b.ch(b'[');
    for (i, iv) in ivs.iter().enumerate() {
        if i > 0 {
            b.ch(b',');
        }
        b.ch(b'{');
        if let Some(dv) = &iv.default_value {
            b.a("\"defaultValue\":");
            value(b, dv);
            b.ch(b',');
        }
        b.a("\"directives\":");
        directives(b, &iv.directives);
        b.a(",\"name\":");
        str(b, &iv.name);
        b.a(",\"type\":");
        type_ref(b, &iv.r#type);
        b.ch(b'}');
    }
    b.ch(b']');
}

fn selections(b: &mut Buf, sels: &[GqlSelection]) {
    b.ch(b'[');
    for (i, s) in sels.iter().enumerate() {
        if i > 0 {
            b.ch(b',');
        }
        match s {
            GqlSelection::GqlField(f) => {
                b.a("{\"_type\":\"field\"");
                if let Some(alias) = &f.alias {
                    b.a(",\"alias\":");
                    str(b, alias);
                }
                b.a(",\"arguments\":");
                arguments(b, &f.arguments);
                b.a(",\"directives\":");
                directives(b, &f.directives);
                b.a(",\"name\":");
                str(b, &f.name);
                b.a(",\"selectionSet\":");
                selections(b, &f.selection_set);
                b.ch(b'}');
            }
            GqlSelection::GqlFragmentSpread(fs) => {
                b.a("{\"_type\":\"fragmentSpread\",\"directives\":");
                directives(b, &fs.directives);
                b.a(",\"name\":");
                str(b, &fs.name);
                b.ch(b'}');
            }
        }
    }
    b.ch(b']');
}

fn arguments(b: &mut Buf, args: &[Box<GqlArgument>]) {
    b.ch(b'[');
    for (i, a) in args.iter().enumerate() {
        if i > 0 {
            b.ch(b',');
        }
        b.a("{\"name\":");
        str(b, &a.name);
        b.a(",\"value\":");
        value(b, &a.value);
        b.ch(b'}');
    }
    b.ch(b']');
}

fn directives(b: &mut Buf, dirs: &[Box<GqlDirective>]) {
    b.ch(b'[');
    for (i, d) in dirs.iter().enumerate() {
        if i > 0 {
            b.ch(b',');
        }
        b.a("{\"arguments\":");
        arguments(b, &d.arguments);
        b.a(",\"name\":");
        str(b, &d.name);
        b.ch(b'}');
    }
    b.ch(b']');
}

fn variable_def(b: &mut Buf, v: &GqlVariableDef) {
    b.ch(b'{');
    if let Some(dv) = &v.default_value {
        b.a("\"defaultValue\":");
        value(b, dv);
        b.ch(b',');
    }
    b.a("\"directives\":[],\"name\":");
    str(b, &v.dollar_name);
    b.a(",\"type\":");
    type_ref(b, &v.r#type);
    b.ch(b'}');
}

fn type_ref(b: &mut Buf, t: &GqlTypeRef) {
    match t {
        GqlTypeRef::GqlNamedType(n) => {
            b.a("{\"_type\":\"namedTypeReference\",\"name\":");
            str(b, &n.name);
            b.a(",\"nullable\":");
            b.boolean(n.nullable);
            b.ch(b'}');
        }
        GqlTypeRef::GqlListType(l) => {
            b.a("{\"_type\":\"listTypeReference\",\"itemType\":");
            type_ref(b, &l.item_type);
            b.a(",\"nullable\":");
            b.boolean(l.nullable);
            b.ch(b'}');
        }
    }
}

fn value(b: &mut Buf, v: &GqlValue) {
    match v {
        GqlValue::GqlIntValue(i) => {
            b.a("{\"_type\":\"intValue\",\"value\":");
            b.int(i.value);
            b.ch(b'}');
        }
        GqlValue::GqlFloatValue(f) => {
            b.a("{\"_type\":\"floatValue\",\"value\":");
            b.a(&lite_javacompat::double_to_string(f.value));
            b.ch(b'}');
        }
        GqlValue::GqlStringValue(s) => {
            b.a("{\"_type\":\"stringValue\",\"value\":");
            str(b, &s.value);
            b.ch(b'}');
        }
        GqlValue::GqlBooleanValue(bo) => {
            b.a("{\"_type\":\"booleanValue\",\"value\":");
            b.boolean(bo.value);
            b.ch(b'}');
        }
        GqlValue::GqlNullValue(_) => b.a("{\"_type\":\"nullValue\"}"),
        GqlValue::GqlEnumValue(e) => {
            b.a("{\"_type\":\"enumValue\",\"value\":");
            str(b, &e.value);
            b.ch(b'}');
        }
        GqlValue::GqlVariableRef(vr) => {
            b.a("{\"_type\":\"variable\",\"name\":");
            str(b, &vr.name);
            b.ch(b'}');
        }
        GqlValue::GqlListValue(l) => {
            b.a("{\"_type\":\"listValue\",\"values\":[");
            for (i, x) in l.values.iter().enumerate() {
                if i > 0 {
                    b.ch(b',');
                }
                value(b, x);
            }
            b.a("]}");
        }
        GqlValue::GqlObjectValue(o) => {
            b.a("{\"_type\":\"objectValue\",\"fields\":");
            arguments(b, &o.fields);
            b.ch(b'}');
        }
    }
}
