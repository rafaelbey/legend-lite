// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! `MappingPrePass` (MappingPrePass.java at ad86fa895): every legacy mapping's RESOLVED record, element order --
//! surface (JSON identity sets) -> M2M `~src` cycles -> declared keys -> extends -> implicit inheritance -> store refs
//! -> implicit operation sets; then the validation (a strict build rejects the first invalid set IN DECLARATION ORDER,
//! a module build poisons them by set id). A mapping whose pre-pass fails is walled (first line, attributed) under a
//! tolerant build, thrown under a strict one.
//!
//! The M2M index is a `LinkedHashMap` in Java now (declaration order, register row S380: the cycle's text starts at
//! the first-declared class of the first chain that closes) -- a `LinkedMap` here. RE-EXPRESSED (census §4): :265's
//! `next != pcm` is `pcm.source_class != pcm.class_name` (the index holds one set per class, and every set walked
//! comes from it). The E-local include facts are a parameter here (where Java asks `MappingClosures.of(model)`).

use std::sync::Arc;

use lite_model::linked::LinkedMap;
use lite_model::{ClassMapping, ClassMappingPure, ClassMappingRelational, JStr, LegacyMappingDefinition, ParsedModel};
use lite_names::ModelIndex;

use crate::error::Thrown;
use crate::jdk::LinkedSet;
use crate::lifted_views::LiftedViews;
use crate::mapping_closures::MappingClosures;
use crate::resolved_mapping::ResolvedMapping;
use crate::unported;

/// `MappingPrePass.run` (:47): each legacy mapping's resolved record (absent when walled), element order.
pub fn run<'a>(
    parsed: &ParsedModel,
    model: &ModelIndex,
    closures: &'a MappingClosures<'a>,
    wall_sink: Option<&mut LinkedMap<JStr, JStr>>,
    views: &'a LiftedViews<'a>,
) -> Result<LinkedMap<JStr, ResolvedMapping<'a>>, Thrown> {
    Err(Thrown::Unported("MappingPrePass.run"))
}

/// `MappingPrePass.prePass` (:84): one mapping's construction, every step a rewrite of the record under construction.
pub fn pre_pass<'a>(
    authored: &Arc<LegacyMappingDefinition>,
    model: &ModelIndex,
    closures: &'a MappingClosures<'a>,
    views: &'a LiftedViews<'a>,
) -> Result<ResolvedMapping<'a>, Thrown> {
    Err(Thrown::Unported("MappingPrePass.prePass"))
}

/// `MappingPrePass.resolveExtends` (:126): `extends [parentSetId]` flattened into each Relational child (set ids
/// resolved within the mapping and its includes); loud for a Pure child.
pub fn resolve_extends(r: &ResolvedMapping<'_>, model: &ModelIndex) -> Result<Arc<LegacyMappingDefinition>, Thrown> {
    Err(Thrown::Unported("MappingPrePass.resolveExtends"))
}

/// `MappingPrePass.flattenExtends` (:160): one child's `extends` chain merged (parent PMs first, the child's by
/// `(name, targetSetId)` overriding; table attributes inherit-if-absent); loud for a cycle (`chain`'s order printed),
/// a missing parent or a non-Relational one.
pub fn flatten_extends(
    child: &ClassMappingRelational,
    by_set_id: &LinkedMap<JStr, ClassMapping>,
    chain: &mut LinkedSet<JStr>,
    md: &LegacyMappingDefinition,
) -> Result<Arc<ClassMappingRelational>, Thrown> {
    Err(Thrown::Unported("MappingPrePass.flattenExtends"))
}

/// `MappingPrePass.detectM2MCycles` (:229): loud for an M2M `~src` cycle (`A ~src B`, `B ~src A`, or longer).
pub fn detect_m2m_cycles(md: &LegacyMappingDefinition) -> Result<(), Thrown> {
    Err(Thrown::Unported("MappingPrePass.detectM2MCycles"))
}

/// `MappingPrePass.walkM2MChain` (:249): one chain from `pcm`; a self-sourced set is a leaf (module note).
pub fn walk_m2m_chain(
    pcm: &Arc<ClassMappingPure>,
    pure_by_target: &LinkedMap<JStr, Arc<ClassMappingPure>>,
    visiting: &mut LinkedSet<JStr>,
    md: &LegacyMappingDefinition,
) -> Result<(), Thrown> {
    Err(Thrown::Unported("MappingPrePass.walkM2MChain"))
}

/// `MappingPrePass.kindOf` (:272): a class mapping's kind, as its grammar names it (for a message).
pub fn kind_of(cm: &ClassMapping) -> &'static str {
    // UNPORTED: MappingPrePass.kindOf
    unported!("MappingPrePass.kindOf")
}
