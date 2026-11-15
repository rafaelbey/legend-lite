// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! MappingNormalizer family 5 (EF_CENSUS §1.1): the class-synthesis dispatch, one exhaustive match over the five
//! class-mapping kinds; the lifted function named by `SynthFqn` (`mappingClassSet` when set-discriminated).

use lite_model::{ClassMapping, FunctionDefinition};
use lite_names::ModelIndex;

use crate::error::Thrown;
use crate::mapping_ledger::MappingLedger;
use crate::resolved_mapping::ResolvedMapping;
use crate::unported;

/// `MappingNormalizer.isOperation` (:771): a union or inheritance operation.
pub fn is_operation(cm: &ClassMapping) -> bool {
    // UNPORTED: MappingNormalizer.isOperation
    unported!("MappingNormalizer.isOperation")
}

/// `MappingNormalizer.synthesizeClassMapping` (:775): the set's realizing function `() -> C[*]`.
pub fn synthesize_class_mapping(
    md: &ResolvedMapping<'_>,
    cm: &ClassMapping,
    model: &ModelIndex,
    set_discriminated: bool,
    ledger: &mut MappingLedger,
) -> Result<FunctionDefinition, Thrown> {
    Err(Thrown::Unported("MappingNormalizer.synthesizeClassMapping"))
}
