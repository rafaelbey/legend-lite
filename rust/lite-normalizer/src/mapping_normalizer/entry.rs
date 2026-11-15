// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! MappingNormalizer families 1 and 2 (EF_CENSUS §1.1): the entry, the per-mapping wall (`withElement`), and the
//! legacy mapping's binding table (`normalizeMapping`, its per-set, per-class and per-association catches). The
//! catches are the answer (§9.4): port their placement, which kinds reach `strictErrors`, the first-line wall and
//! the element attribution exactly.

use std::sync::Arc;

use lite_model::linked::LinkedMap;
use lite_model::{FunctionDefinition, JStr, MappingDefinition, NormalizedModel, ParsedModel};
use lite_names::ModelIndex;

use crate::error::Thrown;
use crate::lifted_views::LiftedViews;
use crate::mapping_ledger::MappingLedger;
use crate::resolved_mapping::ResolvedMapping;

/// `MappingNormalizer.normalize(parsed, model, wallSink, views)` (:152): E.1 over every legacy and clean-sheet
/// mapping, element order. Store substitutions resolved once (`StoreSubstitutionRewrite.resolveAllStores`, outside
/// every catch), every mapping's pre-pass run to completion first (`MappingPrePass.run`, with the E-local
/// `MappingClosures` made here: `MappingClosures::new(model)`), then each mapping normalized under `withElement`, a
/// `ModelException` with an element walled (first line, `putIfAbsent`) when `wall_sink` is given; the lifted functions
/// appended after the structural elements.
pub fn normalize(
    parsed: &ParsedModel,
    model: &ModelIndex,
    wall_sink: Option<&mut LinkedMap<JStr, JStr>>,
    views: &LiftedViews<'_>,
) -> Result<NormalizedModel, Thrown> {
    Err(Thrown::Unported("MappingNormalizer.normalize"))
}

/// `MappingNormalizer.withElement(elementFqn, work)` (:238): attach the element's FQN to what escapes `work` -- ONE
/// wrap covers every throw inside a mapping's normalization ([`Thrown::with_element`]: a `ModelException` without an
/// element gets this one, a `NotImplementedException` becomes `ModelException(NORMALIZE)`, a bug stays raw).
pub fn with_element<T>(element_fqn: &JStr, work: impl FnOnce() -> Result<T, Thrown>) -> Result<T, Thrown> {
    work().map_err(|e| e.with_element(element_fqn))
}

/// `MappingNormalizer.normalizeMapping` (:258): the legacy mapping -> its canonical binding table (`ClassBinding`s in
/// declaration order, association bindings, flattened enumeration mappings, `resolved_stores`, the ledger's facts);
/// the lifted functions appended to `lifted`. `mixedUnionRooted` (:280) is not ported (written, never read).
pub fn normalize_mapping(
    pp: &ResolvedMapping<'_>,
    model: &ModelIndex,
    lifted: &mut Vec<Arc<FunctionDefinition>>,
    resolved_stores: &LinkedMap<JStr, JStr>,
    ledger: &mut MappingLedger,
) -> Result<MappingDefinition, Thrown> {
    Err(Thrown::Unported("MappingNormalizer.normalizeMapping"))
}
