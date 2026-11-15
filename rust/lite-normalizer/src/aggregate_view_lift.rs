// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! `AggregateViewLift` (AggregateViewLift.java at ad86fa895): an AggregationAware node's views compile as SETS
//! (non-root, their own ids), the main binding carrying their specification facts for the router. It runs outside
//! the per-class try (MN:383-386): a view set's error walls the whole mapping (ASK 10). The views' `DeclaredKeys` look
//! always NONE (the pre-pass never captures them; ASK, low).

use std::collections::HashMap;
use std::sync::Arc;

use lite_model::{
    ClassMappingRelational, FunctionDefinition, JStr, MappingDefinitionClassBinding,
    MappingDefinitionClassBindingAggregateViewFacts, MappingDefinitionClassBindingDeclaredKeys as DeclaredKeys,
};
use lite_names::ModelIndex;

use crate::error::Thrown;
use crate::mapping_ledger::MappingLedger;
use crate::resolved_mapping::ResolvedMapping;

/// `AggregateViewLift.lift` (:28): each view set's function appended to `lifted`, its binding to `class_bindings`.
#[allow(clippy::too_many_arguments)]
pub fn lift(
    md: &ResolvedMapping<'_>,
    agg_main: &ClassMappingRelational,
    model: &ModelIndex,
    lifted: &mut Vec<Arc<FunctionDefinition>>,
    class_bindings: &mut Vec<MappingDefinitionClassBinding>,
    declared_keys: &HashMap<JStr, Arc<DeclaredKeys>>,
    ledger: &mut MappingLedger,
) -> Result<(), Thrown> {
    Err(Thrown::Unported("AggregateViewLift.lift"))
}

/// `AggregateViewLift.facts` (:53): the view facts an AggregationAware main binding carries (a view set with no id is
/// Java's `NullPointerException`, "view set id").
pub fn facts(
    main: &ClassMappingRelational,
) -> Result<Vec<Arc<MappingDefinitionClassBindingAggregateViewFacts>>, Thrown> {
    Err(Thrown::Unported("AggregateViewLift.facts"))
}
