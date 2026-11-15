// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! `M2mRouteGuards` (M2mRouteGuards.java at ad86fa895): an M2M binding's storage key (the `AllVersions` rule) and the
//! refusal of a set-routed binding root-routing would get wrong; a mapping-local property field.

use std::sync::Arc;

use lite_front::records as p;
use lite_model::{ClassDefinition, ClassMapping, ClassMappingPure, ClassMappingPurePropertyBinding, JStr};
use lite_names::ModelIndex;

use crate::error::Thrown;
use crate::resolved_mapping::ResolvedMapping;
use crate::unported;

/// `M2mRouteGuards.m2mBindingKey` (:28): the declared property name, or the base property of a synthetic
/// `<base>AllVersions` spelling; loud for an undeclared key. `declared` is Java's `Predicate<String>` (a property-type
/// lookup, fallible here).
pub fn m2m_binding_key(
    pb: &ClassMappingPurePropertyBinding,
    tgt: Option<&Arc<ClassDefinition>>,
    md: &ResolvedMapping<'_>,
    declared: &dyn Fn(&JStr) -> Result<bool, Thrown>,
) -> Result<JStr, Thrown> {
    Err(Thrown::Unported("M2mRouteGuards.m2mBindingKey"))
}

/// `M2mRouteGuards.requireBenignRoute` (:63): loud unless the binding's `[src, tgt]` route is the source set itself
/// and its target the routed class's sole set, its root, or a whole-source identity binding.
pub fn require_benign_route(
    pb: &ClassMappingPurePropertyBinding,
    pcm: &ClassMappingPure,
    tgt: Option<&Arc<ClassDefinition>>,
    md: &ResolvedMapping<'_>,
    model: &ModelIndex,
) -> Result<(), Thrown> {
    Err(Thrown::Unported("M2mRouteGuards.requireBenignRoute"))
}

/// `M2mRouteGuards.setIdMatches` (:121): whether `cm` answers to `id` (its declared id, or exactly the engine-default
/// id of its class).
pub fn set_id_matches(cm: &ClassMapping, id: Option<&JStr>) -> bool {
    // UNPORTED: M2mRouteGuards.setIdMatches
    unported!("M2mRouteGuards.setIdMatches")
}

/// `M2mRouteGuards.localField` (:141): a mapping-local property's field (`isLocal`); loud when it collides with a
/// declared property.
pub fn local_field(
    pb: &ClassMappingPurePropertyBinding,
    tgt: Option<&Arc<ClassDefinition>>,
    md: &ResolvedMapping<'_>,
    model: &ModelIndex,
    collides: bool,
) -> Result<p::KeyExpression, Thrown> {
    Err(Thrown::Unported("M2mRouteGuards.localField"))
}
