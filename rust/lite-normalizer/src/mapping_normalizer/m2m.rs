// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! MappingNormalizer family 8 (EF_CENSUS §1.1): M2M (Pure) sets -- `getAll(Src) -> filter? -> map(src | ^T(...))`.
//! MN:1168's text is in the plan oracle (a poison suffix). `m2mPropertyValue` passes the raw expression through on
//! three misses and strips an uncited `AllVersions` suffix (ASK 12): port as Java has them.

use std::sync::Arc;

use lite_error::Phase;
use lite_front::records as p;
use lite_model::linked::{empty, list};
use lite_model::names::{default_set_id, is_default_set_id};
use lite_model::text::ends_with;
use lite_model::{ClassDefinition, ClassMappingPure, ClassMappingPurePropertyBinding, JStr, cat};
use lite_names::{ModelIndex, calls};

use super::{Fields, build_new_instance};
use crate::error::Thrown;
use crate::m2m_route_guards;
use crate::mapping_ledger::MappingLedger;
use crate::resolved_mapping::ResolvedMapping;

/// The synthetic milestoned sweep spelling `<base>AllVersions`.
const ALL_VERSIONS: &str = "AllVersions";

/// `MappingNormalizer.synthM2M` (:1155): a Pure set's body.
pub fn synth_m2m(
    md: &ResolvedMapping<'_>,
    pcm: &Arc<ClassMappingPure>,
    model: &ModelIndex,
    ledger: &MappingLedger,
) -> Result<p::ValueSpecification, Thrown> {
    // Source: SourceClass.all() -- emitted as getAll(SourceClass)
    let Some(src_fqn) = &pcm.source_class else {
        // a Pure mapping with no ~src is legal to WRITE and to compile, but with no source there is no extent to
        // iterate: refuse where the meaning runs out, not where the syntax does
        return Err(Thrown::model(
            Phase::Normalize,
            cat!(
                "Pure class mapping for '",
                &pcm.class_name,
                "' in mapping '",
                md.qualified_name(),
                "' declares no ~src, so it has no source extent to map from \u{2014} such a mapping can be compiled \
                 and analysed but cannot produce rows"
            ),
            None,
        ));
    };
    let mut source = applied(calls::platform(
        &JStr::from("getAll"),
        vec![p::ValueSpecification::PackageableElementPtr(Arc::new(
            p::PackageableElementPtr {
                full_path: src_fqn.clone(),
                pos: None,
            },
        ))],
    ));
    if let Some(filter) = &pcm.filter {
        source = applied(calls::platform(
            &JStr::from("filter"),
            vec![
                source,
                p::ValueSpecification::LambdaFunction(Arc::new(p::LambdaFunction {
                    parameters: list(vec![variable("src")]),
                    body: list(vec![filter.clone()]),
                    pos: None,
                })),
            ],
        ));
    }
    // Terminal: map(src | ^Class(...))
    let src_bind = variable("src");
    let mut fields = Fields::new();
    // MissProbe.knownMiss: the target class absent -> `tgt` none (the raw expressions pass through)
    let tgt: Option<Arc<ClassDefinition>> = model.knowledge().hierarchy_class(Some(&pcm.class_name)).cloned();
    for pb in pcm.property_bindings.iter() {
        // Audit 21a: the parsed mappingLine heads are honored or poisoned by DESIGN -- never dropped. A local (+prop)
        // is checked FIRST so a name collision with a real/inherited/association property can never retarget it
        if pb.local {
            // mapping-LOCAL property (the XStore assoc-key idiom): an extra isLocal binding column; a collision with a
            // declared property stays the audit 21a poison (M2mRouteGuards.localField)
            let collides = model
                .knowledge()
                .property_type(tgt.as_deref(), &pb.property_name)?
                .is_some();
            fields.put(
                pb.property_name.clone(),
                m2m_route_guards::local_field(pb, tgt.as_ref(), md, model, collides)?,
            );
            continue;
        }
        if pb.explode {
            return Err(Thrown::model(
                Phase::Normalize,
                cat!(
                    "M2M explosion '",
                    &pb.property_name,
                    "*' is a roadmap feature (index-aligned zip fan-out \u{2014} one target instance per source \
                     element); mapping=",
                    md.qualified_name()
                ),
                None,
            ));
        }
        if let Some(enum_mapping_id) = &pb.enum_mapping_id {
            // parsed and RECORDED -- dropping the transformer would read raw source values as enum names, silently
            // wrong
            return Err(Thrown::model(
                Phase::Normalize,
                cat!(
                    "M2M enum transformer 'EnumerationMapping ",
                    enum_mapping_id,
                    "' on '",
                    &pb.property_name,
                    "' is a roadmap feature (source-value decode on the M2M read); mapping=",
                    md.qualified_name()
                ),
                None,
            ));
        }
        let declared =
            |b: &JStr| -> Result<bool, Thrown> { Ok(model.knowledge().property_type(tgt.as_deref(), b)?.is_some()) };
        let key_name = m2m_route_guards::m2m_binding_key(pb, tgt.as_ref(), model, &declared)?;
        fields.put(
            key_name,
            p::KeyExpression {
                value: m2m_property_value(pb, tgt.as_ref(), pcm.source_class.as_ref(), md, model, ledger)?,
                is_add: false,
                is_local: false,
            },
        );
    }
    let terminal = build_new_instance(Some(&pcm.class_name), &fields)?;
    Ok(applied(calls::platform(
        &JStr::from("map"),
        vec![
            source,
            p::ValueSpecification::LambdaFunction(Arc::new(p::LambdaFunction {
                parameters: list(vec![src_bind]),
                body: list(vec![terminal]),
                pos: None,
            })),
        ],
    )))
}

/// `MappingNormalizer.m2mValueClass`: the class an M2M transform returns where it can be read without a typer --
/// `$src` the ~src class, a property path from it each property's declared class, `^C(...)` C; else none (S395).
pub fn m2m_value_class(
    v: &p::ValueSpecification,
    src_class: Option<&JStr>,
    model: &ModelIndex,
) -> Result<Option<JStr>, Thrown> {
    Ok(match v {
        p::ValueSpecification::Variable(var) if var.name == "src" => src_class.cloned(),
        p::ValueSpecification::NewInstance(ni) => Some(ni.class_name.clone()),
        // `^C(...)` on the wire: new(<C>, NewInstance), the instance its second argument
        p::ValueSpecification::AppliedFunction(af) if af.parameters.len() == 2 => match &af.parameters[1] {
            p::ValueSpecification::NewInstance(ni) => Some(ni.class_name.clone()),
            _ => None,
        },
        p::ValueSpecification::AppliedProperty(ap) => {
            let receiver = m2m_value_class(&ap.receiver, src_class, model)?;
            let knowledge = model.knowledge();
            let cd = receiver.as_ref().and_then(|r| knowledge.hierarchy_class(Some(r)));
            let t = match cd {
                Some(cd) => knowledge.property_type(Some(cd), &ap.property)?,
                None => None,
            };
            match t {
                Some(p::TypeExpression::TypeExpressionNameRef(nr))
                    if knowledge.hierarchy_class(Some(&nr.name)).is_some() =>
                {
                    Some(nr.name.clone())
                }
                _ => None,
            }
        }
        _ => None,
    })
}

/// `MappingNormalizer.m2mPropertyValue` (:1229): one binding's value -- the expression, or a `NewInstanceCast` to a
/// mapped class-typed property (loud for an unmapped one).
pub fn m2m_property_value(
    pb: &Arc<ClassMappingPurePropertyBinding>,
    tgt: Option<&Arc<ClassDefinition>>,
    src_class: Option<&JStr>,
    md: &ResolvedMapping<'_>,
    model: &ModelIndex,
    ledger: &MappingLedger,
) -> Result<p::ValueSpecification, Thrown> {
    // the three misses pass the raw expression through, and the `AllVersions` strip is uncited (ASK 12): as Java has
    // them
    let Some(tgt) = tgt else {
        return Ok(pb.expression.clone());
    };
    let mut prop_type = model.knowledge().property_type(Some(tgt), &pb.property_name)?;
    if prop_type.is_none() && ends_with(&pb.property_name.0, ALL_VERSIONS) {
        let cut = pb.property_name.len() - ALL_VERSIONS.len();
        prop_type = model
            .knowledge()
            .property_type(Some(tgt), &JStr::from(&pb.property_name.0[..cut]))?;
    }
    let Some(p::TypeExpression::TypeExpressionNameRef(nr)) = prop_type else {
        return Ok(pb.expression.clone());
    };
    let inner_fqn = &nr.name;
    if model.knowledge().hierarchy_class(Some(inner_fqn)).is_none() {
        return Ok(pb.expression.clone());
    }
    if !ledger.is_mapped(inner_fqn) {
        // a class-typed property whose class has no set: the engine's pass-through when the line names the class's
        // default id and the transform's type conforms, else "Can't find class mapping '<id>'"; a transform this reads
        // no type of keeps lite's refusal (since 2026-11-15, the normalizer's second batch, item 2, S395)
        let target = match &pb.target_set_id {
            Some(id) if !id.is_empty() => id.clone(),
            _ => default_set_id(inner_fqn),
        };
        let value_class = m2m_value_class(&pb.expression, src_class, model)?;
        // (a line's id naming another class's set -- a subclass's -- keeps lite's refusal below)
        if (value_class.is_some() || !is_default_set_id(&target, inner_fqn)) && md.set(Some(&target))?.is_none() {
            if let Some(vc) = &value_class
                && is_default_set_id(&target, inner_fqn)
                && (vc == inner_fqn || model.knowledge().is_subtype(vc, inner_fqn))
            {
                return Ok(pb.expression.clone());
            }
            return Err(Thrown::model(
                Phase::Mapping,
                cat!("Can't find class mapping '", &target, "'"),
                None,
            ));
        }
        return Err(Thrown::model(
            Phase::Normalize,
            cat!(
                "M2M class-typed property '",
                &pb.property_name,
                "' on '",
                &tgt.qualified_name,
                "' targets unmapped class '",
                inner_fqn,
                "'; map '",
                inner_fqn,
                "' or use Embedded. Mapping=",
                md.qualified_name()
            ),
            None,
        ));
    }
    // audit 21a heads honored: the line's [targetSetId] route rides the cast -- the whole-$src graph child dispatches
    // by it; the cast DEFERS the child to the graph, so nothing recurses here
    Ok(p::ValueSpecification::NewInstanceCast(Arc::new(p::NewInstanceCast {
        class_name: inner_fqn.clone(),
        type_arguments: empty(),
        src: pb.expression.clone(),
        target_set_id: pb.target_set_id.clone(),
    })))
}

/// A built call as a value.
fn applied(af: p::AppliedFunction) -> p::ValueSpecification {
    p::ValueSpecification::AppliedFunction(Arc::new(af))
}

/// `new Variable(name)`.
fn variable(name: &str) -> Arc<p::Variable> {
    Arc::new(p::Variable {
        name: JStr::from(name),
        r#type: None,
        multiplicity: None,
        pos: None,
    })
}

#[cfg(test)]
mod tests {
    use std::collections::{HashMap, HashSet};
    use std::rc::Rc;

    use super::*;
    use crate::lifted_views::LiftedViews;
    use crate::mapping_closures::MappingClosures;
    use crate::test_support::{index, mapping, model};
    use lite_model::linked::LinkedMap;
    use lite_model::text::j;

    const M2M: &str = r#"
Class model::Person { name: String[1]; firm: model::Firm[1]; boss: model::Boss[0..1]; }
Class model::Firm { name: String[1]; }
Class model::Boss { name: String[1]; }
Class model::RawPerson { name: String[1]; }

###Mapping
Mapping my::M
(
  *model::Person: Pure { ~src model::RawPerson name: $src.name }
  *model::Firm: Pure { ~src model::RawPerson name: $src.name }
)
"#;

    fn resolved<'a>(
        md: Arc<lite_model::LegacyMappingDefinition>,
        closures: &'a MappingClosures<'a>,
        views: &'a LiftedViews<'a>,
    ) -> ResolvedMapping<'a> {
        let surface = closures.surface(&md);
        let closure = closures.closure(&surface.qualified_name);
        ResolvedMapping::new(
            surface.clone(),
            surface,
            Rc::new(HashMap::new()),
            Rc::new(LinkedMap::new()),
            closure,
            views,
        )
    }

    fn src_name() -> p::ValueSpecification {
        p::ValueSpecification::CString(Arc::new(p::CString {
            value: j("x"),
            pos: None,
            multi_line: false,
        }))
    }

    fn binding(name: &str) -> ClassMappingPurePropertyBinding {
        ClassMappingPurePropertyBinding {
            property_name: j(name),
            expression: src_name(),
            source_set_id: None,
            target_set_id: Some(j("f")),
            explode: false,
            local: false,
            enum_mapping_id: None,
        }
    }

    fn pure(source: Option<&str>, pbs: Vec<ClassMappingPurePropertyBinding>) -> Arc<ClassMappingPure> {
        Arc::new(ClassMappingPure {
            class_name: j("model::Person"),
            set_id: None,
            extends_set_id: None,
            root: true,
            source_class: source.map(j),
            filter: None,
            property_bindings: list(pbs.into_iter().map(Arc::new).collect()),
        })
    }

    #[test]
    fn a_set_with_no_src_an_explosion_and_an_enum_transformer_are_refused() {
        let parsed = model(M2M);
        let idx = index(&parsed);
        let closures = MappingClosures::new(&idx);
        let views = LiftedViews::new(&parsed, &idx);
        let md = resolved(mapping(&parsed, "my::M"), &closures, &views);
        let ledger = MappingLedger::new(HashSet::new());
        let no_src = synth_m2m(&md, &pure(None, vec![]), &idx, &ledger).unwrap_err();
        assert!(no_src.is_model());
        assert_eq!(no_src.phase(), Some(Phase::Normalize));
        assert_eq!(
            no_src.message(),
            Some(j(
                "Pure class mapping for 'model::Person' in mapping 'my::M' declares no ~src, so it has no source \
                 extent to map from \u{2014} such a mapping can be compiled and analysed but cannot produce rows"
            ))
        );
        // MN:1176's text, the one the frozen plan oracle carries as a poison suffix
        let exploded = ClassMappingPurePropertyBinding {
            explode: true,
            ..binding("tradeId")
        };
        assert_eq!(
            synth_m2m(&md, &pure(Some("model::RawPerson"), vec![exploded]), &idx, &ledger)
                .unwrap_err()
                .message(),
            Some(j(
                "M2M explosion 'tradeId*' is a roadmap feature (index-aligned zip fan-out \u{2014} one target \
                 instance per source element); mapping=my::M"
            ))
        );
        let decoded = ClassMappingPurePropertyBinding {
            enum_mapping_id: Some(j("e1")),
            ..binding("name")
        };
        assert_eq!(
            synth_m2m(&md, &pure(Some("model::RawPerson"), vec![decoded]), &idx, &ledger)
                .unwrap_err()
                .message(),
            Some(j(
                "M2M enum transformer 'EnumerationMapping e1' on 'name' is a roadmap feature (source-value decode on \
                 the M2M read); mapping=my::M"
            ))
        );
    }

    #[test]
    fn a_class_typed_value_casts_to_a_mapped_class_and_refuses_an_unmapped_one() {
        let parsed = model(M2M);
        let idx = index(&parsed);
        let closures = MappingClosures::new(&idx);
        let views = LiftedViews::new(&parsed, &idx);
        let md = resolved(mapping(&parsed, "my::M"), &closures, &views);
        let ledger = MappingLedger::new(HashSet::from([j("model::Person"), j("model::Firm")]));
        let tgt = idx.knowledge().hierarchy_class(Some(&j("model::Person"))).cloned();
        let cast = |name: &str| m2m_property_value(&Arc::new(binding(name)), tgt.as_ref(), None, &md, &idx, &ledger);
        assert_eq!(
            cast("firm"),
            Ok(p::ValueSpecification::NewInstanceCast(Arc::new(p::NewInstanceCast {
                class_name: j("model::Firm"),
                type_arguments: empty(),
                src: src_name(),
                target_set_id: Some(j("f")),
            })))
        );
        // the uncited AllVersions strip (ASK 12)
        assert!(matches!(
            cast("firmAllVersions"),
            Ok(p::ValueSpecification::NewInstanceCast(_))
        ));
        // the three misses pass the raw expression through: a primitive, an undeclared name, no target class
        assert_eq!(cast("name"), Ok(src_name()));
        assert_eq!(cast("nope"), Ok(src_name()));
        assert_eq!(
            m2m_property_value(&Arc::new(binding("firm")), None, None, &md, &idx, &ledger),
            Ok(src_name())
        );
        // an unmapped class named by a line's set id that is not its default: the engine's refusal (since 2026-11-15,
        // the normalizer's second batch, item 2, S395; it was lite's "M2M class-typed property 'boss' on
        // 'model::Person' targets unmapped class 'model::Boss'; map 'model::Boss' or use Embedded. Mapping=my::M")
        assert_eq!(
            cast("boss").unwrap_err().message(),
            Some(j("Can't find class mapping 'f'"))
        );
    }
}
