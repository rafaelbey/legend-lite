// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! `M2mRouteGuards` (M2mRouteGuards.java at ad86fa895): an M2M binding's storage key (the `AllVersions` rule) and the
//! refusal of a set-routed binding root-routing would get wrong; a mapping-local property field.

use std::sync::Arc;

use lite_error::Phase;
use lite_front::records as p;
use lite_model::names::is_default_set_id;
use lite_model::{ClassDefinition, ClassMapping, ClassMappingPure, ClassMappingPurePropertyBinding, JStr, cat, text};
use lite_names::{MilestoningStrategy, ModelIndex};

use crate::error::Thrown;
use crate::resolved_mapping::ResolvedMapping;

/// `M2mRouteGuards.m2mBindingKey` (:28): the declared property name, or the base property of a synthetic
/// `<base>AllVersions` spelling; loud for an undeclared key. `declared` is Java's `Predicate<String>` (a property-type
/// lookup, fallible here).
pub fn m2m_binding_key(
    pb: &ClassMappingPurePropertyBinding,
    tgt: Option<&Arc<ClassDefinition>>,
    model: &ModelIndex,
    declared: &dyn Fn(&JStr) -> Result<bool, Thrown>,
) -> Result<JStr, Thrown> {
    let Some(tgt) = tgt else {
        return Ok(pb.property_name.clone());
    };
    if declared(&pb.property_name)? {
        return Ok(pb.property_name.clone());
    }
    // `pAllVersions` reads as p's binding only for a p whose type is a class carrying a temporal stereotype of its own,
    // the edge-point property legend-engine generates (since 2026-11-15, the normalizer's second batch, item 3, S389)
    let suffix = "AllVersions";
    if text::ends_with(&pb.property_name.0, suffix) {
        let base = JStr::from(&pb.property_name.0[..pb.property_name.len() - suffix.len()]);
        if declared(&base)? && own_temporal_type(model.knowledge().property_type(Some(tgt), &base)?.as_ref(), model) {
            return Ok(base);
        }
    }
    // a property the class has not, in the engine's words: the class and its generalizations by simple name, nearest
    // first (since 2026-11-15, S389; it was lite's own "M2M PropertyBinding 'p' is not declared on class 'T'")
    let mut order = String::new();
    for cd in model.knowledge().lineage(&tgt.qualified_name)? {
        order.push_str(&simple_name(&cd.qualified_name).to_string());
        order.push_str(", ");
    }
    Err(Thrown::model(
        Phase::Normalize,
        cat!(
            "Can't find property '",
            &pb.property_name,
            "' in [",
            &JStr::from(order.as_str()),
            "Any]"
        ),
        None,
    ))
}

/// `M2mRouteGuards.ownTemporalType`: whether `type` names a class carrying a temporal (milestoning) stereotype of its
/// own.
fn own_temporal_type(r#type: Option<&p::TypeExpression>, model: &ModelIndex) -> bool {
    let Some(p::TypeExpression::TypeExpressionNameRef(nr)) = r#type else {
        return false;
    };
    model.knowledge().hierarchy_class(Some(&nr.name)).is_some_and(|cd| {
        cd.stereotypes
            .iter()
            .any(|st| MilestoningStrategy::of_stereotype_or_null(&st.profile_name, &st.stereotype_name).is_some())
    })
}

/// A qualified name's last segment.
fn simple_name(fqn: &JStr) -> JStr {
    match text::last_index_of(&fqn.0, &JStr::from("::").0) {
        Some(i) => JStr::from(&fqn.0[i + 2..]),
        None => fqn.clone(),
    }
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
    if pb.source_set_id.is_none() && pb.target_set_id.is_none() {
        return Ok(());
    }
    if let Some(source) = &pb.source_set_id
        && !id_matches(pcm.set_id.as_ref(), &pcm.class_name, Some(source))
    {
        return Err(Thrown::model(
            Phase::Normalize,
            cat!(
                "M2M binding '",
                &pb.property_name,
                "[",
                source,
                ", ",
                or_null(pb.target_set_id.as_ref()),
                "]' names source set '",
                source,
                "' but is declared inside set '",
                or_null(pcm.set_id.as_ref()),
                "'; mapping=",
                md.qualified_name()
            ),
            None,
        ));
    }
    let knowledge = model.knowledge();
    let mut routed_class: Option<JStr> = None;
    if let Some(tgt) = tgt
        && let Some(p::TypeExpression::TypeExpressionNameRef(nr)) =
            knowledge.property_type(Some(tgt), &pb.property_name)?
        && knowledge.hierarchy_class(Some(&nr.name)).is_some()
    {
        routed_class = Some(nr.name.clone());
    }
    if let Some(routed_class) = &routed_class {
        let mut sets: Vec<ClassMapping> = Vec::new();
        for m in md.closure() {
            for cm in m.class_mappings.iter() {
                if cm.class_name() == routed_class {
                    sets.push(cm.clone());
                }
            }
        }
        // the routed class has no set: the line's default id is the engine's pass-through, decided with the
        // transform's type by m2m_property_value (S395); any other id is the engine's refusal (since 2026-11-15, the
        // normalizer's second batch, item 2)
        if sets.is_empty() {
            match &pb.target_set_id {
                None => return Ok(()),
                Some(id) if is_default_set_id(id, routed_class) => return Ok(()),
                // an id no set of the closure answers to; one naming another class's set keeps lite's refusal below
                Some(id) if md.set(Some(id))?.is_none() => {
                    return Err(Thrown::model(
                        Phase::Mapping,
                        cat!("Can't find class mapping '", id, "'"),
                        None,
                    ));
                }
                Some(_) => {}
            }
        }
        // a set id that matches nothing falls through to the refusal (`MissProbe.miss`)
        let routed = sets.iter().find(|cm| set_id_matches(cm, pb.target_set_id.as_ref()));
        // honored routes: the sole set, the root, or the WHOLE-SOURCE identity binding (`$src`), whose cast carries
        // the target set id
        if let Some(routed) = routed
            && (sets.len() == 1 || routed.root() || matches!(pb.expression, p::ValueSpecification::Variable(_)))
        {
            return Ok(());
        }
    }
    let of_class = match &routed_class {
        None => JStr::from(""),
        Some(c) => cat!(" of '", c, "'"),
    };
    Err(Thrown::model(
        Phase::Normalize,
        cat!(
            "M2M set-routed binding '",
            &pb.property_name,
            "[",
            or_null(pb.target_set_id.as_ref()),
            "]' targets a non-root set",
            of_class,
            "; per-set routing on the M2M side is a roadmap feature (root-routing would produce wrong rows); mapping=",
            md.qualified_name()
        ),
        None,
    ))
}

/// `M2mRouteGuards.setIdMatches` (:121): whether `cm` answers to `id` (its declared id, or exactly the engine-default
/// id of its class).
pub fn set_id_matches(cm: &ClassMapping, id: Option<&JStr>) -> bool {
    id_matches(cm.set_id(), cm.class_name(), id)
}

/// [`set_id_matches`] over a set's declared id and class (the source set of [`require_benign_route`] is a
/// `ClassMappingPure` record, not a `ClassMapping`).
fn id_matches(set_id: Option<&JStr>, class_name: &JStr, id: Option<&JStr>) -> bool {
    let Some(id) = id else {
        return false;
    };
    if let Some(declared) = set_id {
        return declared == id;
    }
    // audit 23: EXACTLY the engine-default id (the FQN with '_' for '::')
    is_default_set_id(id, class_name)
}

/// `M2mRouteGuards.localField` (:141): a mapping-local property's field (`isLocal`); loud when it collides with a
/// declared property.
pub fn local_field(
    pb: &ClassMappingPurePropertyBinding,
    tgt: Option<&Arc<ClassDefinition>>,
    md: &ResolvedMapping<'_>,
    _model: &ModelIndex,
    collides: bool,
) -> Result<p::KeyExpression, Thrown> {
    if let Some(tgt) = tgt
        && collides
    {
        return Err(Thrown::model(
            Phase::Normalize,
            cat!(
                "M2M local mapping property '+",
                &pb.property_name,
                "' collides with a declared property of '",
                &tgt.qualified_name,
                "' \u{2014} locals are distinct (engine) and the shared binding table cannot hold both; mapping=",
                md.qualified_name()
            ),
            None,
        ));
    }
    Ok(p::KeyExpression {
        value: pb.expression.clone(),
        is_add: false,
        is_local: true,
    })
}

/// Java's `"" + s` for a nullable string.
fn or_null(s: Option<&JStr>) -> JStr {
    s.cloned().unwrap_or_else(|| JStr::from("null"))
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::rc::Rc;

    use super::*;
    use crate::lifted_views::LiftedViews;
    use crate::mapping_closures::MappingClosures;
    use crate::test_support::{index, mapping, model};
    use lite_model::linked::{LinkedMap, empty};
    use lite_model::text::j;

    const MODEL: &str = r#"
###Pure
Class a::Firm
{
  name: String[1];
}
Class a::Person
{
  firm: a::Firm[1];
  name: String[1];
}

###Relational
Database a::db
(
  Table F (ID INTEGER PRIMARY KEY, NAME VARCHAR(20))
)

###Mapping
Mapping a::M
(
  *a::Firm[f1]: Relational
  {
    ~mainTable [a::db]F
    name: [a::db]F.NAME
  }
  a::Firm[f2]: Relational
  {
    ~mainTable [a::db]F
    name: [a::db]F.NAME
  }
)
"#;

    fn binding(
        prop: &str,
        src: Option<&str>,
        tgt: Option<&str>,
        expr: p::ValueSpecification,
    ) -> ClassMappingPurePropertyBinding {
        ClassMappingPurePropertyBinding {
            property_name: j(prop),
            expression: expr,
            source_set_id: src.map(j),
            target_set_id: tgt.map(j),
            explode: false,
            local: false,
            enum_mapping_id: None,
        }
    }

    fn src_var() -> p::ValueSpecification {
        p::ValueSpecification::Variable(Arc::new(p::Variable {
            name: j("src"),
            r#type: None,
            multiplicity: None,
            pos: None,
        }))
    }

    fn not_a_var() -> p::ValueSpecification {
        p::ValueSpecification::CString(Arc::new(p::CString {
            value: j("x"),
            pos: None,
            multi_line: false,
        }))
    }

    #[test]
    fn a_route_is_benign_on_the_root_or_an_identity_binding_and_loud_otherwise() {
        let parsed = model(MODEL);
        let idx = index(&parsed);
        let closures = MappingClosures::new(&idx);
        let views = LiftedViews::new(&parsed, &idx);
        let m = closures.surface(&mapping(&parsed, "a::M"));
        let md = ResolvedMapping::new(
            m.clone(),
            m.clone(),
            Rc::new(HashMap::new()),
            Rc::new(LinkedMap::new()),
            closures.closure(&m.qualified_name),
            &views,
        );
        let person = idx.find_class(&j("a::Person")).cloned();
        let pcm = ClassMappingPure {
            class_name: j("a::Person"),
            set_id: None,
            extends_set_id: None,
            root: false,
            source_class: None,
            filter: None,
            property_bindings: empty(),
        };
        let route = |pb: &ClassMappingPurePropertyBinding| require_benign_route(pb, &pcm, person.as_ref(), &md, &idx);
        assert!(route(&binding("firm", None, None, not_a_var())).is_ok());
        assert!(route(&binding("firm", None, Some("f1"), not_a_var())).is_ok());
        assert!(route(&binding("firm", Some("a_Person"), Some("f2"), src_var())).is_ok());
        let e = route(&binding("firm", None, Some("f2"), not_a_var())).unwrap_err();
        assert!(e.is_model());
        assert_eq!(
            e.message(),
            Some(j(
                "M2M set-routed binding 'firm[f2]' targets a non-root set of 'a::Firm'; per-set routing on the M2M \
                 side is a roadmap feature (root-routing would produce wrong rows); mapping=a::M"
            ))
        );
        let e = route(&binding("name", None, Some("f1"), not_a_var())).unwrap_err();
        assert_eq!(
            e.message(),
            Some(j(
                "M2M set-routed binding 'name[f1]' targets a non-root set; per-set routing on the M2M side is a \
                 roadmap feature (root-routing would produce wrong rows); mapping=a::M"
            ))
        );
        let e = route(&binding("firm", Some("q"), None, not_a_var())).unwrap_err();
        assert_eq!(
            e.message(),
            Some(j(
                "M2M binding 'firm[q, null]' names source set 'q' but is declared inside set 'null'; mapping=a::M"
            ))
        );
        // the binding key: the declared name, the base of an AllVersions spelling of a temporal class's property, else
        // the engine's refusal (since 2026-11-15, the normalizer's second batch, item 3, S389: `nameAllVersions` read as
        // `name`, a String, and an undeclared name was lite's "M2M PropertyBinding 'x' is not declared on class
        // 'a::Person'; mapping=a::M")
        let declared = |n: &JStr| Ok::<bool, Thrown>(n.eq_str("name"));
        let key = |prop: &str| {
            m2m_binding_key(
                &binding(prop, None, None, not_a_var()),
                person.as_ref(),
                &idx,
                &declared,
            )
        };
        assert_eq!(key("name"), Ok(j("name")));
        assert_eq!(
            key("nameAllVersions").unwrap_err().message(),
            Some(j("Can't find property 'nameAllVersions' in [Person, Any]"))
        );
        assert_eq!(
            key("x").unwrap_err().message(),
            Some(j("Can't find property 'x' in [Person, Any]"))
        );
        assert_eq!(
            m2m_binding_key(&binding("x", None, None, not_a_var()), None, &idx, &declared),
            Ok(j("x"))
        );
        // a local field, and its collision
        let pb = binding("prodId", None, None, not_a_var());
        let f = local_field(&pb, person.as_ref(), &md, &idx, false).unwrap();
        assert!(f.is_local && !f.is_add && f.value == not_a_var());
        assert_eq!(
            local_field(&pb, person.as_ref(), &md, &idx, true)
                .unwrap_err()
                .message(),
            Some(j(
                "M2M local mapping property '+prodId' collides with a declared property of 'a::Person' \u{2014} \
                 locals are distinct (engine) and the shared binding table cannot hold both; mapping=a::M"
            ))
        );
        assert!(local_field(&pb, None, &md, &idx, true).is_ok());
    }

    #[test]
    fn a_set_answers_to_its_declared_id_or_exactly_its_default() {
        let set = |id: Option<&str>| crate::test_support::cm(crate::test_support::relational("a::b::C", id, vec![]));
        assert!(set_id_matches(&set(Some("s")), Some(&j("s"))));
        assert!(!set_id_matches(&set(Some("s")), Some(&j("a_b_C"))));
        assert!(set_id_matches(&set(None), Some(&j("a_b_C"))));
        assert!(!set_id_matches(&set(None), Some(&j("C"))));
        assert!(!set_id_matches(&set(None), None));
    }
}
