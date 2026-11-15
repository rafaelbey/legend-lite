// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! MappingNormalizer family 6 (EF_CENSUS §1.1): Relation (`~func`) sets -- `<fn>() -> map(row | ^C(...))`. Reads
//! lite-model's `ClassMappingRelationFunctionCol::is_embedded` and `ClassMappingRelationFunctionCol::bind_src`.

use std::sync::Arc;

use lite_error::Phase;
use lite_front::records as p;
use lite_model::linked::list;
use lite_model::{ClassMapping, ClassMappingRelationFunction, ClassMappingRelationFunctionCol, Function, JStr, cat};
use lite_names::{ModelIndex, calls};

use super::{Fields, build_new_instance_to_one, translate_enumerated_source};
use crate::error::{Thrown, require};
use crate::resolved_mapping::ResolvedMapping;

/// `MappingNormalizer.synthRelationFunction` (:817): a Relation set's body.
pub fn synth_relation_function(
    md: &ResolvedMapping<'_>,
    rf: &Arc<ClassMappingRelationFunction>,
    model: &ModelIndex,
) -> Result<p::ValueSpecification, Thrown> {
    let pipeline = relation_function_pipeline(rf, model)?;
    let row = Arc::new(p::Variable {
        name: JStr::from("rf_row"),
        r#type: None,
        multiplicity: None,
        pos: None,
    });
    let mut fields = Fields::new();
    put_relation_cols(&mut fields, &rf.columns, &row, &rf.class_name, md, model)?;
    let terminal = build_new_instance_to_one(Some(&rf.class_name), &fields, model)?;
    Ok(p::ValueSpecification::AppliedFunction(Arc::new(calls::platform(
        &JStr::from("map"),
        vec![
            pipeline,
            p::ValueSpecification::LambdaFunction(Arc::new(p::LambdaFunction {
                parameters: list(vec![row]),
                body: list(vec![terminal]),
                pos: None,
            })),
        ],
    ))))
}

/// `MappingNormalizer.putRelationCols` (:833): bindings for a Relation mapping's column list into `fields` (EMBEDDED
/// blocks and inline-embedded sibling sets nested, locals skipped).
pub fn put_relation_cols(
    fields: &mut Fields,
    cols: &[Arc<ClassMappingRelationFunctionCol>],
    row: &Arc<p::Variable>,
    owner_class_fqn: &JStr,
    md: &ResolvedMapping<'_>,
    model: &ModelIndex,
) -> Result<(), Thrown> {
    for c in cols {
        if c.local {
            continue;
        }
        if let Some(inline_set_id) = &c.inline_set_id {
            // INLINE-embedded (prop () Inline [set]): the SIBLING set's column list gives the sub-object's shape; its
            // bindings read THIS row (engine inline semantics: reuse the set's mapping shape in place)
            let mut sibling: Option<&Arc<ClassMappingRelationFunction>> = None;
            for cm in md.class_mappings().iter() {
                if let ClassMapping::ClassMappingRelationFunction(rf2) = cm
                    && *inline_set_id == ResolvedMapping::id_of(cm)
                {
                    sibling = Some(rf2);
                    break;
                }
            }
            let Some(sibling) = sibling else {
                return Err(Thrown::model(
                    Phase::Normalize,
                    cat!(
                        "inline-embedded property '",
                        &c.property,
                        "' names set '",
                        inline_set_id,
                        "' which is not a Relation set of mapping ",
                        md.qualified_name()
                    ),
                    None,
                ));
            };
            let mut inner = Fields::new();
            put_relation_cols(&mut inner, &sibling.columns, row, &sibling.class_name, md, model)?;
            fields.put(
                c.property.clone(),
                key(build_new_instance_to_one(Some(&sibling.class_name), &inner, model)?),
            );
            continue;
        }
        if c.is_embedded() {
            let owner = model
                .knowledge()
                .hierarchy_class(Some(owner_class_fqn))
                .ok_or_else(|| {
                    Thrown::model(
                        Phase::Normalize,
                        cat!(
                            "Relation mapping embedded property '",
                            &c.property,
                            "': unknown owner class '",
                            owner_class_fqn,
                            "'"
                        ),
                        None,
                    )
                })?;
            let t = model.knowledge().property_type(Some(owner), &c.property)?;
            let Some(p::TypeExpression::TypeExpressionNameRef(nr)) = t else {
                return Err(Thrown::model(
                    Phase::Normalize,
                    cat!(
                        "Relation mapping embedded property '",
                        &c.property,
                        "' of '",
                        owner_class_fqn,
                        "' has non-class type \u{2014} cannot embed"
                    ),
                    None,
                ));
            };
            let mut inner = Fields::new();
            put_relation_cols(&mut inner, &c.embedded, row, &nr.name, md, model)?;
            fields.put(
                c.property.clone(),
                key(build_new_instance_to_one(Some(&nr.name), &inner, model)?),
            );
            continue;
        }
        // a ROW-EXPRESSION binding (explicit-src form) rebinds $src to THIS row
        let mut read = match &c.expr {
            Some(expr) => {
                ClassMappingRelationFunctionCol::bind_src(expr, &p::ValueSpecification::Variable(row.clone()))
            }
            None => p::ValueSpecification::AppliedProperty(Arc::new(p::AppliedProperty {
                receiver: p::ValueSpecification::Variable(row.clone()),
                property: require(c.column.clone(), Some("column read on an embedded ~func col"))?,
                pos: None,
                owner_class: None,
            })),
        };
        if let Some(enum_mapping_id) = &c.enum_mapping_id {
            // enum-decoded column: the same source-value decode chain every other enum-mapped read synthesizes
            read = translate_enumerated_source(&c.property, Some(enum_mapping_id), &read, md, owner_class_fqn, model)?;
        }
        fields.put(c.property.clone(), key(read));
    }
    Ok(())
}

/// `MappingNormalizer.relationFunctionPipeline` (:908): the Relation set's source -- its inline source, or the one
/// zero-arg single-expression function `~func` names.
pub fn relation_function_pipeline(
    rf: &ClassMappingRelationFunction,
    model: &ModelIndex,
) -> Result<p::ValueSpecification, Thrown> {
    let Some(r#ref) = &rf.func_ref else {
        // ~src inline expression IS the pipeline -- no resolution step (engine #4941: _relationFunction holds the
        // lambda itself)
        return require(rf.inline_source.clone(), None);
    };
    let mut fns = model.find_function(r#ref);
    if fns.is_empty() {
        // the signature-id spelling f__Relation_1_ names one overload, registered under that exact id
        fns = model.find_function_by_id(r#ref);
    }
    let Some(Function::FunctionDefinition(f)) = fns.first().filter(|_| fns.len() == 1) else {
        return Err(Thrown::not_implemented(cat!(
            "Relation mapping for '",
            &rf.class_name,
            "': ~func '",
            r#ref,
            "' resolves to ",
            fns.len().to_string(),
            " function(s)"
        )));
    };
    if !f.parameters.is_empty() || f.body.len() != 1 {
        return Err(Thrown::not_implemented(cat!(
            "Relation mapping ~func '",
            &f.qualified_name,
            "' must be a zero-arg single-expression function"
        )));
    }
    Ok(f.body[0].clone())
}

/// `new KeyExpression(value, false, false)`.
fn key(value: p::ValueSpecification) -> p::KeyExpression {
    p::KeyExpression {
        value,
        is_add: false,
        is_local: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{index, model};
    use lite_model::linked::empty;
    use lite_model::text::j;

    const FUNCS: &str = r#"
###Pure
function a::rows(): meta::pure::metamodel::relation::Relation<Any>[1]
{
  #>{a::db.PERSON}#
}
function a::two(x: String[1]): meta::pure::metamodel::relation::Relation<Any>[1]
{
  #>{a::db.PERSON}#
}

###Relational
Database a::db
(
  Table PERSON (ID INTEGER PRIMARY KEY, NAME VARCHAR(200))
)
"#;

    fn rf(func: Option<&str>, inline: Option<p::ValueSpecification>) -> ClassMappingRelationFunction {
        ClassMappingRelationFunction {
            class_name: j("a::P"),
            set_id: None,
            extends_set_id: None,
            root: true,
            func_ref: func.map(j),
            inline_source: inline,
            columns: empty(),
            primary_key: empty(),
        }
    }

    #[test]
    fn the_pipeline_is_the_one_zero_arg_functions_body() {
        let parsed = model(FUNCS);
        let idx = index(&parsed);
        let body = relation_function_pipeline(&rf(Some("a::rows"), None), &idx).expect("the body");
        let f = match &idx.find_function(&j("a::rows"))[0] {
            Function::FunctionDefinition(f) => f.clone(),
            Function::NativeFunctionDefinition(_) => unreachable!("a user function"),
        };
        assert_eq!(body, f.body[0]);
    }

    #[test]
    fn a_missing_or_parameterized_func_is_a_roadmap_wall() {
        let parsed = model(FUNCS);
        let idx = index(&parsed);
        let none = relation_function_pipeline(&rf(Some("a::nope"), None), &idx).unwrap_err();
        assert!(none.is_not_implemented());
        assert_eq!(
            none.message(),
            Some(j(
                "Relation mapping for 'a::P': ~func 'a::nope' resolves to 0 function(s)"
            ))
        );
        let two = relation_function_pipeline(&rf(Some("a::two"), None), &idx).unwrap_err();
        assert_eq!(
            two.message(),
            Some(j(
                "Relation mapping ~func 'a::two' must be a zero-arg single-expression function"
            ))
        );
    }

    #[test]
    fn an_inline_source_is_the_pipeline_and_none_is_javas_npe() {
        let parsed = model(FUNCS);
        let idx = index(&parsed);
        let src = p::ValueSpecification::CString(Arc::new(p::CString {
            value: j("x"),
            pos: None,
            multi_line: false,
        }));
        assert_eq!(relation_function_pipeline(&rf(None, Some(src.clone())), &idx), Ok(src));
        assert_eq!(
            relation_function_pipeline(&rf(None, None), &idx),
            Err(Thrown::null_pointer(None))
        );
    }
}
