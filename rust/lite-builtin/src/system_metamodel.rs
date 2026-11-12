// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! `SystemMetamodel`: the system metamodel store -- the system Database, Mapping and the navigation functions over them,
//! fixed Pure source parsed once and injected into every model build, and the FQN constants that identify them.
//!
//! SKELETON (2026-11-12, slice 1b): the shared API the platform builds against; the source and its elements are filled
//! in by the port of `SystemMetamodel.java`.

use lite_model::{FunctionDefinition, FunctionId, PackageableElement};
use std::sync::Arc;

/// The system store element.
pub const STORE_FQN: &str = "meta::lite::metamodel::MetamodelStore";

/// The system store's order column.
pub const ORDINAL_COLUMN: &str = "ordinal";

/// The system mapping.
pub const MAPPING_FQN: &str = "meta::lite::metamodel::MetamodelMapping";

/// `SystemMetamodel.source()`: the system Pure source.
pub fn source() -> &'static str {
    // SKELETON
    ""
}

/// `SystemMetamodel.elements()`: the system elements as parsed (at lite's dialect), in source order.
pub fn elements() -> &'static [PackageableElement] {
    // SKELETON
    &[]
}

/// `SystemMetamodel.functionIds()`: the ids of the system metamodel's functions (the platform's own Pure), in order.
pub fn function_ids() -> Vec<FunctionId> {
    functions_by_id().into_iter().map(|(id, _)| id).collect()
}

/// `SystemMetamodel.functionsById()`: the system functions as parsed, by id, in order.
pub fn functions_by_id() -> Vec<(FunctionId, Arc<FunctionDefinition>)> {
    // SKELETON
    Vec::new()
}

/// `SystemMetamodel.isSystemElement(fqn)`: whether the FQN names a system element that is not a function.
pub fn is_system_element(fqn: &str) -> bool {
    elements()
        .iter()
        .any(|e| !matches!(e, PackageableElement::FunctionDefinition(_)) && e.qualified_name().lossy() == fqn)
}
