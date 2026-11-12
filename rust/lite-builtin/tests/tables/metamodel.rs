// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! The system metamodel's section (Java's `MetamodelTables`): `SystemMetamodel`, line for line as Java writes it --
//! the constants, the source (its SHA-256 and length, then each line), every element as parsed, the FQN sets in their
//! order, the functions by id, `isSystemElement` over every element FQN and a few that are not, and
//! `requireNoSystemElementRedefined` over small models.

use lite_builtin::system_metamodel as sm;
use lite_front::doors::Dialect;
use lite_front::jtypes::JStr;
use lite_model::PackageableElement;

use super::support::{self, Out, b, join, json, sha};

/// FQNs `isSystemElement` is asked of beyond the elements' own (Java's `NOT_ELEMENTS`).
const NOT_ELEMENTS: [&str; 9] = [
    "my::Foo",
    "",
    "meta::lite::metamodel",
    "MetamodelStore",
    "meta::lite::metamodel::metamodelstore",
    "meta::pure::mapping::Mapping",
    "meta::relational::metamodel::datatype::Varchar",
    " meta::lite::metamodel::MetamodelStore",
    "meta::lite::metamodel::MetamodelStore",
];

/// The models `requireNoSystemElementRedefined` is asked of: (name, text at lite's dialect) (Java's `MODELS`).
const MODELS: [(&str, &str); 9] = [
    ("empty", ""),
    ("none", "Class my::app::Person\n{\n    name: String[1];\n}\n"),
    (
        "systemClass",
        "Class my::app::Person\n{\n}\n\nClass meta::lite::metamodel::MappingVisibility\n{\n    includeRank: Integer[1];\n}\n",
    ),
    (
        "systemPlatformClass",
        "Class meta::relational::metamodel::datatype::Time\n{\n}\n",
    ),
    (
        "systemAssociation",
        "Association meta::lite::metamodel::AliasBaseTables\n{\n    base: my::A[1];\n    baseOf: my::B[*];\n}\n",
    ),
    (
        "systemStore",
        "###Relational\nDatabase meta::lite::metamodel::MetamodelStore\n(\n)\n",
    ),
    (
        "systemMapping",
        "###Mapping\nMapping meta::lite::metamodel::MetamodelMapping\n(\n)\n",
    ),
    (
        "systemFunction",
        "function meta::pure::mapping::classMappings(_this:meta::pure::mapping::Mapping[1]):meta::pure::mapping::SetImplementation[*]\n{\n    []\n}\n",
    ),
    (
        "twoSystem",
        "###Mapping\nMapping meta::lite::metamodel::MetamodelMapping\n(\n)\n\n###Pure\nClass meta::lite::metamodel::StoreResolution\n{\n}\n",
    ),
];

fn s(v: &str) -> String {
    v.to_owned()
}

pub fn dump(out: &mut Out) {
    // ---- the constants
    out.line("metamodel.const", &[s("STORE_FQN"), s(sm::STORE_FQN)]);
    out.line("metamodel.const", &[s("ORDINAL_COLUMN"), s(sm::ORDINAL_COLUMN)]);
    out.line("metamodel.const", &[s("MAPPING_FQN"), s(sm::MAPPING_FQN)]);
    // ---- the source: the whole text's hash and length (UTF-16 units), then each line
    let source = sm::source();
    let units: Vec<u16> = source.encode_utf16().collect();
    out.line("metamodel.source", &[sha(&units), units.len().to_string()]);
    for (i, l) in source.lines().enumerate() {
        out.line("metamodel.sourceLine", &[i.to_string(), support::str(&JStr::from(l))]);
    }
    // ---- the elements, as parsed (declared PackageableElement: tagged)
    let elements = sm::elements();
    for (i, e) in elements.iter().enumerate() {
        out.line(
            "metamodel.element",
            &[i.to_string(), e.qualified_name().lossy(), json(e, false)],
        );
    }
    // ---- the FQN sets, in their (insertion) order
    out.line(
        "metamodel.elementFqns",
        &[join(sm::element_fqns().iter().map(JStr::lossy))],
    );
    out.line(
        "metamodel.notFunctionFqns",
        &[join(sm::non_function_fqns().iter().map(JStr::lossy))],
    );
    // ---- the functions by id, in order: each id and the index of its definition among the elements (the last one,
    // as Java's loop finds it; Java asks identity, this the record's equality -- no two system functions are equal,
    // their ids differ -- as the source guards keep addresses out of the crates)
    for (n, (id, def)) in sm::functions_by_id().iter().enumerate() {
        let at = elements
            .iter()
            .rposition(|e| matches!(e, PackageableElement::FunctionDefinition(f) if **f == **def))
            .map_or(-1, |i| i as i64);
        out.line(
            "metamodel.functionById",
            &[n.to_string(), id.qualified.lossy(), at.to_string()],
        );
    }
    out.line(
        "metamodel.functionIds",
        &[join(sm::function_ids().iter().map(|i| i.qualified.lossy()))],
    );
    // ---- isSystemElement, over every element FQN and a few that are not
    let mut asked: Vec<String> = sm::element_fqns().iter().map(JStr::lossy).collect();
    asked.extend(NOT_ELEMENTS.iter().map(|v| s(v)));
    for fqn in &asked {
        out.line(
            "metamodel.isSystemElement",
            &[support::str(&JStr::from(fqn.as_str())), b(sm::is_system_element(fqn))],
        );
    }
    // ---- requireNoSystemElementRedefined, over small models
    for (name, text) in MODELS {
        let units: Vec<u16> = text.encode_utf16().collect();
        let parsed = lite_builtin::door::parse(&units, Dialect::LegendLite)
            .unwrap_or_else(|e| panic!("the model {name} does not parse: {e:?}"));
        // Java asks whether the model returned is the one handed (`==`); the signature ties the answer to its
        // argument, so the record's equality asks the same here
        match sm::require_no_system_element_redefined(&parsed) {
            Ok(back) => out.line(
                "metamodel.requireNoSystemElementRedefined",
                &[s(name), s("returned"), b(*back == parsed)],
            ),
            Err(e) => out.line(
                "metamodel.requireNoSystemElementRedefined",
                &[
                    s(name),
                    s(e.kind.java_class().rsplit('.').next().unwrap_or_default()),
                    s(e.phase.map_or("null", |p| p.name())),
                    e.element.as_deref().map_or_else(|| s("null"), String::from_utf16_lossy),
                    e.message_lossy(),
                ],
            ),
        }
    }
}
