// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! `AggregateViewLift` (AggregateViewLift.java at ad86fa895): an AggregationAware node's views compile as SETS
//! (non-root, their own ids), the main binding carrying their specification facts for the router. It runs outside
//! the per-class try (MN:383-386): a view set's error walls the whole mapping (ASK 10). The views' `DeclaredKeys` look
//! always NONE (the pre-pass never captures them; ASK, low).

use std::collections::HashMap;
use std::sync::Arc;

use lite_model::linked::{empty, list};
use lite_model::{
    ClassMapping, ClassMappingRelational, FunctionDefinition, JStr, MappingDefinitionClassBinding,
    MappingDefinitionClassBindingAggregateViewFacts, MappingDefinitionClassBindingDeclaredKeys as DeclaredKeys,
    MappingDefinitionClassBindingRelational,
};
use lite_names::ModelIndex;

use crate::error::{Thrown, require};
use crate::mapping_ledger::MappingLedger;
use crate::mapping_normalizer::{
    declared_primary_key_columns, property_pins_of, relational_source_of, synthesize_class_mapping,
};
use crate::resolved_mapping::ResolvedMapping;
use crate::set_key_facts;

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
    for view in require(agg_main.aggregation.as_ref(), None)?.views.iter() {
        let set = ClassMapping::ClassMappingRelational(view.set.clone());
        let view_fn = synthesize_class_mapping(md, &set, model, true, ledger)?;
        let function_fqn = view_fn.qualified_name.clone();
        // the function is lifted before its binding is built (the stamp below may still throw)
        lifted.push(Arc::new(view_fn));
        let binding = MappingDefinitionClassBindingRelational {
            class_fqn: view.set.class_name.clone(),
            set_id: view.set.set_id.clone(),
            extends_set_id: view.set.extends_set_id.clone(),
            root: false,
            function_fqn,
            primary_key_columns: list(declared_primary_key_columns(&set)),
            declared: declared_keys
                .get(&set_key_facts::set_key(&view.set))
                .cloned()
                .unwrap_or_else(DeclaredKeys::none),
            source: relational_source_of(&view.set)?,
            aggregate_views: empty(),
            property_pins: property_pins_of(&view.set).freeze_with(list),
        };
        class_bindings.push(MappingDefinitionClassBinding::MappingDefinitionClassBindingRelational(
            Arc::new(binding),
        ));
    }
    Ok(())
}

/// `AggregateViewLift.facts` (:53): the view facts an AggregationAware main binding carries (a view set with no id is
/// Java's `NullPointerException`, "view set id").
pub fn facts(
    main: &ClassMappingRelational,
) -> Result<Vec<Arc<MappingDefinitionClassBindingAggregateViewFacts>>, Thrown> {
    let Some(aggregation) = &main.aggregation else {
        return Ok(Vec::new());
    };
    let mut out = Vec::new();
    for v in aggregation.views.iter() {
        out.push(Arc::new(MappingDefinitionClassBindingAggregateViewFacts {
            set_id: require(v.set.set_id.clone(), Some("view set id"))?,
            can_aggregate: v.can_aggregate,
            group_by_functions: v.group_by_functions.clone(),
            aggregate_values: v.aggregate_values.clone(),
        }));
    }
    Ok(out)
}
