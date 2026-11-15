// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! MappingNormalizer family 3 (EF_CENSUS §1.1): the clean-sheet door -- every binding of the pre-E surface
//! (`CleanSheetMappingDefinition`) becomes a canonical binding; inline bodies lambda-lift, ref bindings keep the user's
//! FQN, every relational binding stamps its source. The 5-argument `MappingDefinition` constructor (enumeration
//! mappings not flattened with includes, unlike the legacy path) is `MappingDefinition::common`.

use std::collections::HashSet;
use std::sync::Arc;

use lite_front::records as p;
use lite_model::{
    CleanSheetMappingDefinition, CleanSheetMappingDefinitionAssociationBinding,
    CleanSheetMappingDefinitionClassBinding, FunctionDefinition, JStr, MappingDefinition,
    MappingDefinitionRelationalSource,
};
use lite_names::ModelIndex;

use crate::error::Thrown;
use crate::unported;

/// `MappingNormalizer.cleanSheetToCanonical` (:482): the clean-sheet mapping's binding table; its inline lifts
/// appended to `lifted`.
pub fn clean_sheet_to_canonical(
    md: &Arc<CleanSheetMappingDefinition>,
    model: &ModelIndex,
    lifted: &mut Vec<Arc<FunctionDefinition>>,
) -> Result<MappingDefinition, Thrown> {
    Err(Thrown::Unported("MappingNormalizer.cleanSheetToCanonical"))
}

/// `MappingNormalizer.refBody` (:546): a function-REF binding's realizing body, fetched for the stamp; loud when the
/// FQN is no user function with a body.
pub fn ref_body(
    fn_fqn: &JStr,
    mapping_fqn: &JStr,
    class_fqn: &JStr,
    model: &ModelIndex,
) -> Result<Arc<[p::ValueSpecification]>, Thrown> {
    Err(Thrown::Unported("MappingNormalizer.refBody"))
}

/// `MappingNormalizer.inlineRootSource` (:574): the DOOR-1 stamp -- a clean-sheet relational binding's root table,
/// through a chain of user functions (`seen` its cycle guard); loud when none is derivable.
pub fn inline_root_source(
    body: &[p::ValueSpecification],
    model: &ModelIndex,
    mapping_fqn: &JStr,
    class_fqn: &JStr,
    seen: &mut HashSet<JStr>,
) -> Result<MappingDefinitionRelationalSource, Thrown> {
    Err(Thrown::Unported("MappingNormalizer.inlineRootSource"))
}

/// `MappingNormalizer.userFunctionBody` (:619): the registered user function's body for a call root, else none.
pub fn user_function_body(callee_fqn: &JStr, model: &ModelIndex) -> Option<Arc<[p::ValueSpecification]>> {
    // UNPORTED: MappingNormalizer.userFunctionBody
    unported!("MappingNormalizer.userFunctionBody")
}

/// `MappingNormalizer.liftClassInline` (:635): an inline class binding lifted to `fn_fqn(): C[*]`.
pub fn lift_class_inline(
    md: &CleanSheetMappingDefinition,
    cb: &CleanSheetMappingDefinitionClassBinding,
    inl: &p::RealizationInline,
    fn_fqn: &JStr,
) -> FunctionDefinition {
    // UNPORTED: MappingNormalizer.liftClassInline
    unported!("MappingNormalizer.liftClassInline")
}

/// `MappingNormalizer.liftAssocInline` (:656): an inline association predicate lifted to `fn_fqn(a: A[1], b: B[1]):
/// Boolean[1]`; loud unless it is one two-parameter lambda over a known association.
pub fn lift_assoc_inline(
    md: &CleanSheetMappingDefinition,
    ab: &CleanSheetMappingDefinitionAssociationBinding,
    inl: &p::RealizationInline,
    fn_fqn: &JStr,
    model: &ModelIndex,
) -> Result<FunctionDefinition, Thrown> {
    Err(Thrown::Unported("MappingNormalizer.liftAssocInline"))
}
