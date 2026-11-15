// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! MappingNormalizer family 5 (EF_CENSUS §1.1): the class-synthesis dispatch, one exhaustive match over the five
//! class-mapping kinds; the lifted function named by `SynthFqn` (`mappingClassSet` when set-discriminated).

use std::sync::Arc;

use lite_front::records as p;
use lite_model::linked::{empty, list};
use lite_model::{ClassMapping, FunctionDefinition, FunctionDefinitionSynthesized, SynthHat};
use lite_names::{ModelIndex, synth_fqn};

use super::{synth_m2m, synth_relation_function, synth_relational};
use crate::error::Thrown;
use crate::mapping_ledger::MappingLedger;
use crate::resolved_mapping::ResolvedMapping;
use crate::union_synthesis;

/// `MappingNormalizer.isOperation` (:771): a union or inheritance operation.
pub fn is_operation(cm: &ClassMapping) -> bool {
    matches!(
        cm,
        ClassMapping::ClassMappingUnion(_) | ClassMapping::ClassMappingInheritance(_)
    )
}

/// `MappingNormalizer.synthesizeClassMapping` (:775): the set's realizing function `() -> C[*]`.
pub fn synthesize_class_mapping(
    md: &ResolvedMapping<'_>,
    cm: &ClassMapping,
    model: &ModelIndex,
    set_discriminated: bool,
    ledger: &mut MappingLedger,
) -> Result<FunctionDefinition, Thrown> {
    // prop[setId] routing is classified PER-PM (Join.targetSetId) inside synthTableBackedParts: no map-driven
    // pre-rewrite happens here
    let body = match cm {
        ClassMapping::ClassMappingPure(pcm) => synth_m2m(md, pcm, model, ledger)?,
        ClassMapping::ClassMappingRelational(rcm) => synth_relational(md, rcm, model, ledger)?,
        ClassMapping::ClassMappingUnion(u) => union_synthesis::synth_union(md, u, model, ledger)?,
        ClassMapping::ClassMappingInheritance(ih) => union_synthesis::synth_inheritance(md, ih, model, ledger)?,
        ClassMapping::ClassMappingRelationFunction(rf) => synth_relation_function(md, rf, model)?,
    };
    let fqn = if set_discriminated {
        synth_fqn::mapping_class_set(md.qualified_name(), cm.class_name(), &ResolvedMapping::id_of(cm))?
    } else {
        synth_fqn::mapping_class(md.qualified_name(), cm.class_name())?
    };
    Ok(FunctionDefinition::user_written(
        fqn,
        empty(),
        empty(),
        empty(),
        p::TypeExpression::TypeExpressionNameRef(Box::new(p::TypeExpressionNameRef {
            name: cm.class_name().clone(),
            pos: None,
        })),
        // Multiplicity.Concrete.ZERO_MANY
        p::Multiplicity::MultiplicityConcrete(Box::new(p::MultiplicityConcrete {
            lower_bound: 0,
            upper_bound: None,
        })),
        list(vec![body]),
        empty(),
        empty(),
    )
    .with_synthesized_from(Arc::new(FunctionDefinitionSynthesized::new(
        SynthHat::CLASS,
        md.qualified_name().clone(),
        cm.class_name().clone(),
    ))))
}

#[cfg(test)]
mod tests {
    use std::collections::{HashMap, HashSet};
    use std::rc::Rc;

    use super::*;
    use crate::lifted_views::LiftedViews;
    use crate::mapping_closures::MappingClosures;
    use crate::test_support::{index, mapping, model, union};
    use lite_model::linked::LinkedMap;
    use lite_model::text::j;
    use lite_model::{ClassMappingInheritance, ClassMappingPure};

    #[test]
    fn an_operation_is_a_union_or_an_inheritance() {
        assert!(is_operation(&union("a::P", None, &["p1"])));
        let ih = ClassMapping::ClassMappingInheritance(Arc::new(ClassMappingInheritance {
            class_name: j("a::P"),
            set_id: None,
            extends_set_id: None,
            root: false,
        }));
        assert!(is_operation(&ih));
        let r = crate::test_support::cm(crate::test_support::relational("a::P", None, vec![]));
        assert!(!is_operation(&r));
    }

    #[test]
    fn the_dispatch_lets_a_refusal_through_unrecorded() {
        let parsed = model("Class a::P { name: String[1]; }\n###Mapping\nMapping a::M ( )");
        let idx = index(&parsed);
        let closures = MappingClosures::new(&idx);
        let views = LiftedViews::new(&parsed, &idx);
        let md0 = mapping(&parsed, "a::M");
        let surface = closures.surface(&md0);
        let md = ResolvedMapping::new(
            surface.clone(),
            surface.clone(),
            Rc::new(HashMap::new()),
            Rc::new(LinkedMap::new()),
            closures.closure(&surface.qualified_name),
            &views,
        );
        let mut ledger = MappingLedger::new(HashSet::new());
        let no_src = ClassMapping::ClassMappingPure(Arc::new(ClassMappingPure {
            class_name: j("a::P"),
            set_id: None,
            extends_set_id: None,
            root: true,
            source_class: None,
            filter: None,
            property_bindings: empty(),
        }));
        let e = synthesize_class_mapping(&md, &no_src, &idx, false, &mut ledger).unwrap_err();
        assert!(e.is_model());
        assert_eq!(e.phase(), Some(lite_error::Phase::Normalize));
        assert_eq!(
            e.message(),
            Some(j(
                "Pure class mapping for 'a::P' in mapping 'a::M' declares no ~src, so it has no source extent to map \
                 from \u{2014} such a mapping can be compiled and analysed but cannot produce rows"
            ))
        );
        // nothing reached the ledger: the dispatch records nothing of its own, and a refusal escapes it unwalled
        assert!(ledger.poisons.is_empty() && ledger.strict_errors.is_empty());
    }
}
