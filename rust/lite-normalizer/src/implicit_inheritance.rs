// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! `ImplicitInheritance` (ImplicitInheritance.java at ad86fa895): IMPLICIT same-extent inheritance (an ancestor's
//! unqualified PMs merged into a child set over the same main table) and implicit `Inheritance` operations for
//! unmapped routed targets; "Can't find association". Java iterates two `HashMap`s (:43, :123) with no visible effect
//! (a size-1 test; a membership set): iterate `visibleSets` in its own order.

use std::collections::HashMap;
use std::sync::Arc;

use lite_model::{
    ClassMappingRelational, JStr, LegacyMappingDefinition, LegacyMappingDefinitionTableReference as TableRef,
};
use lite_names::ModelIndex;

use crate::error::Thrown;
use crate::resolved_mapping::ResolvedMapping;

/// `ImplicitInheritance.apply` (:38): the mapping with each eligible child set's inherited PMs merged.
pub fn apply(resolved: &ResolvedMapping<'_>, model: &ModelIndex) -> Result<Arc<LegacyMappingDefinition>, Thrown> {
    Err(Thrown::Unported("ImplicitInheritance.apply"))
}

/// `ImplicitInheritance.implicitOpsForRoutedTargets` (:118): the mapping with an `Inheritance` operation appended for
/// each unmapped association end or routed property type with a routed strict-subclass set.
pub fn implicit_ops_for_routed_targets(
    r: &ResolvedMapping<'_>,
    model: &ModelIndex,
) -> Result<Arc<LegacyMappingDefinition>, Thrown> {
    Err(Thrown::Unported("ImplicitInheritance.implicitOpsForRoutedTargets"))
}

/// `ImplicitInheritance.nearestMappedAncestor` (:200): the NEAREST ancestor with exactly ONE Relational set in scope
/// over the child's main table; none otherwise (ambiguity included).
pub fn nearest_mapped_ancestor(
    child_main: &TableRef,
    child_class: &JStr,
    model: &ModelIndex,
    by_class: &HashMap<JStr, Vec<Arc<ClassMappingRelational>>>,
) -> Result<Option<Arc<ClassMappingRelational>>, Thrown> {
    Err(Thrown::Unported("ImplicitInheritance.nearestMappedAncestor"))
}
