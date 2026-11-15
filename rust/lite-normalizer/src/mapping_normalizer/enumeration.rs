// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! MappingNormalizer family 17 (EF_CENSUS §1.1): the enumeration decode -- a nested `if(equal(...))` per source value
//! (an SQL CASE). An inline "NEVER fired" internal error (:2422) stays (as [`Thrown::never_fired`]'s sibling, its text
//! exactly); an anonymous reference prints `EnumerationMapping 'null'` (ASK 13); an id lookup takes the first match
//! across includes where the anonymous path is loud on two: port as Java has them.
//!
//! [`Thrown::never_fired`]: crate::error::Thrown::never_fired

use std::sync::Arc;

use lite_error::Phase;
use lite_front::records as p;
use lite_front::records::ValueSpecification as VS;
use lite_model::linked::empty;
use lite_model::names::set_id_of;
use lite_model::{
    EnumerationMapping, EnumerationMappingSourceValue as SourceValue, JStr, JValue, PropertyMappingEnumeratedColumn,
    cat,
};
use lite_names::{ModelIndex, calls};

use crate::error::Thrown;
use crate::pipeline::Pipeline;
use crate::rel_op_translator::{self, TableScope};
use crate::resolved_mapping::ResolvedMapping;

/// `MappingNormalizer.translateEnumeratedColumn` (:2386): the column read (`p` none: `PipelineView.NONE`), decoded.
#[allow(clippy::too_many_arguments)]
pub fn translate_enumerated_column(
    ec: &Arc<PropertyMappingEnumeratedColumn>,
    table_scope: &TableScope,
    default_table: &JStr,
    md: &ResolvedMapping<'_>,
    p: Option<&Pipeline<'_>>,
    owner_class_fqn: &JStr,
    model: &ModelIndex,
) -> Result<p::ValueSpecification, Thrown> {
    // `p == null ? PipelineView.NONE : p.view()`: `None` is NONE, a pipeline's view is the pipeline
    let col_read = rel_op_translator::column_read(&ec.table, &ec.column, table_scope, default_table, p)?;
    translate_enumerated_source(
        &ec.property_name,
        ec.enum_mapping_id.as_ref(),
        &col_read,
        md,
        owner_class_fqn,
        model,
    )
}

/// `MappingNormalizer.translateEnumeratedSource` (:2403): `source_read` decoded through the enumeration mapping
/// `enum_mapping_id` names (or the property's enum's one mapping).
pub fn translate_enumerated_source(
    property_name: &JStr,
    enum_mapping_id: Option<&JStr>,
    source_read: &p::ValueSpecification,
    md: &ResolvedMapping<'_>,
    owner_class_fqn: &JStr,
    model: &ModelIndex,
) -> Result<p::ValueSpecification, Thrown> {
    let mut em: Option<Arc<EnumerationMapping>> = None;
    let ems = md.enumeration_mappings_with_includes()?;
    if let Some(id) = enum_mapping_id {
        // engine getEnumerationMappingId: an anonymous enum mapping's IMPLICIT id is its enumeration FQN with :: -> _
        for cand in &ems {
            let cand_id = set_id_of(cand.mapping_id.as_ref(), &cand.enum_name);
            if *id == cand_id {
                em = Some(cand.clone());
                break;
            }
        }
    } else {
        let knowledge = model.knowledge();
        // Java's inline internal error (not a `MissProbe.neverFired`: its own text); the `owner == null ? null` after
        // it is dead
        let owner = knowledge.hierarchy_class(Some(owner_class_fqn)).ok_or_else(|| {
            Thrown::illegal_state(cat!(
                "F7.8: class unresolved at MappingNormalizer#8 (this default NEVER fired on the corpus census; a miss \
                 here is a real model gap): ",
                owner_class_fqn
            ))
        })?;
        let enum_fqn: Option<JStr> = match knowledge.property_type(Some(owner), property_name)? {
            Some(p::TypeExpression::TypeExpressionNameRef(nr)) => Some(nr.name.clone()),
            _ => None,
        };
        for cand in &ems {
            if Some(&cand.enum_name) == enum_fqn.as_ref() {
                if em.is_some() {
                    return Err(Thrown::model(
                        Phase::Normalize,
                        cat!(
                            "enum-mapped property '",
                            property_name,
                            "' uses an anonymous EnumerationMapping but '",
                            &cand.enum_name,
                            "' has more than one \u{2014} name the mapping id; mapping=",
                            md.qualified_name()
                        ),
                        None,
                    ));
                }
                em = Some(cand.clone());
            }
        }
    }
    let Some(em) = em else {
        let message = match enum_mapping_id {
            Some(id) => cat!(
                "enum-mapped property '",
                property_name,
                "' references unknown enum mapping '",
                id,
                "'; mapping=",
                md.qualified_name()
            ),
            None => cat!(
                "enum-mapped property '",
                property_name,
                "' uses an anonymous EnumerationMapping but no mapping for the property's enum type exists (or the \
                 property is not enum-typed); mapping=",
                md.qualified_name()
            ),
        };
        return Err(Thrown::model(Phase::Normalize, message, None));
    };
    let mut tail = VS::PureCollection(Arc::new(p::PureCollection {
        values: empty(),
        pos: None,
    }));
    let values = &em.value_mappings;
    // an entry naming a NON-EXISTENT enum value is a compile error (engine HelperMappingBuilder.processEnumMapping);
    // an enumeration the model has not: no check (`MissProbe.miss`)
    let known_values: Option<Arc<[JStr]>> = model.find_enum(&em.enum_name).map(|e| e.values.clone());
    for ev in values.iter().rev() {
        if let Some(known) = &known_values
            && !known.contains(&ev.enum_value)
        {
            return Err(Thrown::model(
                Phase::Normalize,
                cat!(
                    "EnumerationMapping '",
                    or_null(em.mapping_id.as_ref()),
                    "' maps value '",
                    &ev.enum_value,
                    "' which enumeration '",
                    &em.enum_name,
                    "' does not declare"
                ),
                None,
            ));
        }
        let mut disj: Option<VS> = None;
        // (Java's "Unhandled SourceValue" arm: the three variants are the whole sealed set, so no arm here)
        for sv in ev.source_values.iter() {
            let src_lit = match sv {
                SourceValue::EnumerationMappingSourceValueStringValue(s) => VS::CString(Arc::new(p::CString {
                    value: s.value.clone(),
                    pos: None,
                    multi_line: false,
                })),
                SourceValue::EnumerationMappingSourceValueIntegerValue(i2) => VS::CInteger(Arc::new(p::CInteger {
                    // `new CInteger(long)`
                    value: JValue::Long(i2.value),
                    pos: None,
                })),
                SourceValue::EnumerationMappingSourceValueEnumRef(er) => enum_value(&er.enum_path, &er.enum_value_name),
            };
            let eq = platform("equal", vec![source_read.clone(), src_lit]);
            disj = Some(match disj {
                None => eq,
                Some(d) => platform("or", vec![d, eq]),
            });
        }
        let Some(disj) = disj else {
            // Java prints the REFERENCE's id: `null` for an anonymous reference (ASK 13), kept
            return Err(Thrown::model(
                Phase::Normalize,
                cat!(
                    "EnumerationMapping '",
                    or_null(enum_mapping_id),
                    "' value '",
                    &ev.enum_value,
                    "' declares no source values; cannot build a match condition for property '",
                    property_name,
                    "'. Mapping=",
                    md.qualified_name()
                ),
                None,
            ));
        };
        let then = enum_value(&em.enum_name, &ev.enum_value);
        tail = platform("if", vec![disj, thunk(then), thunk(tail)]);
    }
    Ok(tail)
}

/// `new EnumValue(path, value)`: no spans, so the real `enumValue` node (`node` true).
fn enum_value(full_path: &JStr, value: &JStr) -> VS {
    VS::EnumValue(Arc::new(p::EnumValue {
        full_path: full_path.clone(),
        value: value.clone(),
        enumeration_pos: None,
        pos: None,
        node: true,
    }))
}

/// `new LambdaFunction(List.of(), List.of(body))`.
fn thunk(body: VS) -> VS {
    VS::LambdaFunction(Arc::new(p::LambdaFunction {
        parameters: empty(),
        body: vec![body].into(),
        pos: None,
    }))
}

/// `Calls.platform(name, args)` as a value.
fn platform(name: &str, args: Vec<VS>) -> VS {
    VS::AppliedFunction(Arc::new(calls::platform(&JStr::from(name), args)))
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
    use lite_model::linked::LinkedMap;
    use lite_model::text::j;

    const MODEL: &str = r#"
###Pure
Enum a::E
{
  A, B
}
Enum a::F
{
  X
}
Class a::P
{
  e: a::E[1];
  f: a::F[1];
  s: String[1];
}

###Mapping
Mapping a::Base
(
  a::E: EnumerationMapping EM
  {
    A: ['a', 1],
    B: 'b'
  }
  a::F: EnumerationMapping
  {
    X: 'x'
  }
)
Mapping a::M
(
  include a::Base
  a::E: EnumerationMapping EM2
  {
    A: 'aa'
  }
  a::F: EnumerationMapping FM
  {
    X: 'xx'
  }
)
"#;

    fn row_read() -> VS {
        VS::CString(Arc::new(p::CString {
            value: j("read"),
            pos: None,
            multi_line: false,
        }))
    }

    fn call_of(v: &VS) -> (JStr, Vec<VS>) {
        let VS::AppliedFunction(af) = v else { panic!("a call: {v:?}") };
        (af.function.clone(), af.parameters.to_vec())
    }

    /// The spelling `Calls.platform(bare, args)` gives the call.
    fn spelled(bare: &str, args: &[VS]) -> JStr {
        calls::platform(&j(bare), args.to_vec()).function
    }

    fn body(v: &VS) -> VS {
        let VS::LambdaFunction(l) = v else { panic!("a lambda: {v:?}") };
        l.body[0].clone()
    }

    #[test]
    fn a_decode_is_a_nested_if_ending_in_the_empty_collection() {
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
        let owner = j("a::P");
        // by id, across the include: if(equal(r,'a') or equal(r,1), {A}, {if(equal(r,'b'), {B}, {[]})})
        let d = translate_enumerated_source(&j("e"), Some(&j("EM")), &row_read(), &md, &owner, &idx).unwrap();
        let (f, ps) = call_of(&d);
        assert_eq!(f, spelled("if", &ps));
        let (or, ors) = call_of(&ps[0]);
        assert_eq!(or, spelled("or", &ors));
        let (_, second) = call_of(&ors[1]);
        assert_eq!(
            second[1],
            VS::CInteger(Arc::new(p::CInteger {
                value: JValue::Long(1),
                pos: None
            }))
        );
        assert_eq!(body(&ps[1]), enum_value(&j("a::E"), &j("A")));
        let (_, inner) = call_of(&body(&ps[2]));
        assert_eq!(body(&inner[1]), enum_value(&j("a::E"), &j("B")));
        assert_eq!(
            body(&inner[2]),
            VS::PureCollection(Arc::new(p::PureCollection {
                values: empty(),
                pos: None
            }))
        );
        // an anonymous mapping answers to its implicit id
        assert!(translate_enumerated_source(&j("f"), Some(&j("a_F")), &row_read(), &md, &owner, &idx).is_ok());
        // anonymous references: one mapping for the property's enum is loud on two, none on none
        let two = translate_enumerated_source(&j("e"), None, &row_read(), &md, &owner, &idx).unwrap_err();
        assert!(two.is_model());
        assert_eq!(
            two.message(),
            Some(j(
                "enum-mapped property 'e' uses an anonymous EnumerationMapping but 'a::E' has more than one \u{2014} \
                 name the mapping id; mapping=a::M"
            ))
        );
        let none = translate_enumerated_source(&j("s"), None, &row_read(), &md, &owner, &idx).unwrap_err();
        assert_eq!(
            none.message(),
            Some(j(
                "enum-mapped property 's' uses an anonymous EnumerationMapping but no mapping for the property's \
                 enum type exists (or the property is not enum-typed); mapping=a::M"
            ))
        );
        let unknown = translate_enumerated_source(&j("e"), Some(&j("Q")), &row_read(), &md, &owner, &idx).unwrap_err();
        assert_eq!(
            unknown.message(),
            Some(j("enum-mapped property 'e' references unknown enum mapping 'Q'; mapping=a::M"))
        );
        // the anonymous path's internal error, its own text
        let ise = translate_enumerated_source(&j("e"), None, &row_read(), &md, &j("a::Nope"), &idx).unwrap_err();
        assert_eq!(ise.java_class(), "java.lang.IllegalStateException");
        assert_eq!(
            ise.message(),
            Some(j(
                "F7.8: class unresolved at MappingNormalizer#8 (this default NEVER fired on the corpus census; a miss \
                 here is a real model gap): a::Nope"
            ))
        );
    }
}
