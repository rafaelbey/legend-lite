// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! `MappingLedger` (MappingLedger.java at ad86fa895, ported line for line): Phase E's per-MAPPING ledger -- what the
//! synthesis of ONE mapping learns and records on the way (poisons, mixed-kind unions, union key threads, the
//! operation sets' members, the per-element errors a strict build throws), stamped onto the compiled
//! `MappingDefinition` as its `NormalizationFacts` when the mapping's normalization completes.
//!
//! It rides the [`Pipeline`](crate::pipeline::Pipeline) so the deep emission sites reach it: the pipeline holds it
//! exclusively (`&mut`) while it lives, as UnionSynthesis:392 poisons through `p.ledger()`. RE-EXPRESSED (census
//! §4): `mappedInClosure` is a plain set (only `contains` reads it; Java's "ordered" copy kept hash order anyway).

use std::collections::HashSet;
use std::sync::Arc;

use lite_model::linked::{LinkedMap, list};
use lite_model::{JStr, KeyThread, LegacyMappingDefinition, MappingDefinitionNormalizationFacts, PoisonKey, cat};

use crate::error::Thrown;
use crate::mapping_facts;
use crate::resolved_mapping::ResolvedMapping;
use crate::union_synthesis;

/// The per-mapping ledger.
#[derive(Debug)]
pub struct MappingLedger {
    /// `PoisonKey` -> the reason the binding is withheld (loud at use). Written ONLY through [`MappingLedger::poison`]:
    /// every reason is kept (a second, different reason for one key appends with `"; "`).
    pub poisons: LinkedMap<PoisonKey, JStr>,
    /// The per-element errors a STRICT build surfaces, in element order (a `ModelException`, or an association's
    /// `NotImplementedException`: MN:345, :364, :419); a MODULE build keeps them as poisons only.
    pub strict_errors: Vec<Thrown>,
    /// class -> member set ids of a MIXED-KIND Operation union.
    pub mixed_unions: LinkedMap<JStr, Vec<JStr>>,
    /// operation set id -> the member set ids its function's body concatenates, in member order.
    pub operation_members: LinkedMap<JStr, Vec<JStr>>,
    /// class -> the primary-key threads of an Operation union's row.
    pub union_key_threads: LinkedMap<JStr, Vec<Arc<KeyThread>>>,
    /// The classes MAPPED for this mapping (engine R1: a set in the queried mapping's closure). Never a graph-wide
    /// fact.
    mapped_in_closure: HashSet<JStr>,
}

impl MappingLedger {
    /// `new MappingLedger(mappedInClosure)` (:63).
    pub fn new(mapped_in_closure: HashSet<JStr>) -> MappingLedger {
        MappingLedger {
            poisons: LinkedMap::new(),
            strict_errors: Vec::new(),
            mixed_unions: LinkedMap::new(),
            operation_members: LinkedMap::new(),
            union_key_threads: LinkedMap::new(),
            mapped_in_closure,
        }
    }

    /// `MappingLedger.poison(key, reason)` (:39): record `reason` against `key`; a prior reason is kept and this one appended
    /// (`a.equals(b) ? a : a + "; " + b`).
    pub fn poison(&mut self, key: PoisonKey, reason: JStr) {
        let merged = match self.poisons.get(&key) {
            None => reason,
            Some(a) if *a == reason => return,
            Some(a) => cat!(a, "; ", &reason),
        };
        self.poisons.put(key, merged);
    }

    /// `MappingLedger.isMapped(classFqn)` (:68): whether `class_fqn` has a set in this mapping's closure.
    pub fn is_mapped(&self, class_fqn: &JStr) -> bool {
        self.mapped_in_closure.contains(class_fqn)
    }

    /// `MappingLedger.mappedInClosure(md, resolved)` (:73): the mapped classes of `md`'s pre-passed closure.
    pub fn mapped_in_closure(
        md: &ResolvedMapping<'_>,
        resolved: &LinkedMap<JStr, ResolvedMapping<'_>>,
    ) -> HashSet<JStr> {
        let mut out = HashSet::new();
        for m in union_synthesis::pre_passed_closure(md, resolved) {
            for cm in m.class_mappings.iter() {
                out.insert(cm.class_name().clone());
            }
        }
        out
    }

    /// `MappingLedger.facts(surface)` (:85): the compiled mapping's facts -- what this synthesis recorded, plus the SURFACE facts
    /// Phase F reads off the artifact ([`mapping_facts`]), each map in its insertion order.
    pub fn facts(&self, surface: &LegacyMappingDefinition) -> MappingDefinitionNormalizationFacts {
        MappingDefinitionNormalizationFacts {
            poisons: list(self.poisons.iter().cloned().collect()),
            mixed_unions: list(
                self.mixed_unions
                    .iter()
                    .map(|(k, v)| (k.clone(), list(v.clone())))
                    .collect(),
            ),
            union_key_threads: list(
                self.union_key_threads
                    .iter()
                    .map(|(k, v)| (k.clone(), list(v.clone())))
                    .collect(),
            ),
            union_members: mapping_facts::union_members(surface).freeze_with(list),
            routed_target_classes: mapping_facts::routed_target_classes(surface).freeze_with(LinkedMap::freeze),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lite_model::PoisonKeyForClass;
    use lite_model::text::j;

    fn class_key(c: &str) -> PoisonKey {
        PoisonKey::PoisonKeyForClass(Arc::new(PoisonKeyForClass { class_fqn: j(c) }))
    }

    #[test]
    fn a_second_reason_appends_and_a_repeat_is_kept_once() {
        let mut l = MappingLedger::new(HashSet::from([j("a::P")]));
        l.poison(class_key("a::P"), j("first"));
        l.poison(class_key("a::P"), j("first"));
        l.poison(class_key("a::P"), j("second"));
        l.poison(class_key("a::Q"), j("other"));
        let rows: Vec<(PoisonKey, JStr)> = l.poisons.iter().cloned().collect();
        assert_eq!(
            rows,
            vec![(class_key("a::P"), j("first; second")), (class_key("a::Q"), j("other"))]
        );
        assert!(l.is_mapped(&j("a::P")) && !l.is_mapped(&j("a::Q")));
    }

    #[test]
    fn the_facts_keep_insertion_order() {
        let mut l = MappingLedger::new(HashSet::new());
        l.mixed_unions.put(j("a::B"), vec![j("b1")]);
        l.mixed_unions.put(j("a::A"), vec![j("a1"), j("a2")]);
        l.poison(class_key("a::Z"), j("z"));
        let md = crate::test_support::legacy("a::M", vec![]);
        let f = l.facts(&md);
        assert_eq!(
            f.mixed_unions.iter().map(|(k, _)| k.clone()).collect::<Vec<_>>(),
            vec![j("a::B"), j("a::A")]
        );
        assert_eq!(&*f.mixed_unions[1].1, &[j("a1"), j("a2")]);
        assert_eq!(f.poisons.len(), 1);
        assert!(f.union_members.is_empty() && f.routed_target_classes.is_empty() && f.union_key_threads.is_empty());
    }
}
