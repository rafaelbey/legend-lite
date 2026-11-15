// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! MappingNormalizer family 3 (EF_CENSUS §1.1): the clean-sheet door -- every binding of the pre-E surface
//! (`CleanSheetMappingDefinition`) becomes a canonical binding; inline bodies lambda-lift, ref bindings keep the user's
//! FQN, every relational binding stamps its source. The 5-argument `MappingDefinition` constructor (enumeration
//! mappings not flattened with includes, unlike the legacy path) is `MappingDefinition::common`.
//!
//! An inline class body is lifted (into `lifted`) BEFORE its source is stamped, so a stamp that walls the mapping in a
//! tolerant build leaves the lifted function behind (an orphan, ASK 10): as Java has it.

use std::collections::HashSet;
use std::sync::Arc;

use lite_error::Phase;
use lite_front::records as p;
use lite_model::linked::{empty, list};
use lite_model::{
    CleanSheetMappingDefinition, CleanSheetMappingDefinitionAssociationBinding,
    CleanSheetMappingDefinitionClassBinding, CleanSheetMappingDefinitionKind, Function, FunctionDefinition,
    FunctionDefinitionParameterDefinition, FunctionDefinitionSynthesized, JStr, MappingDefinition,
    MappingDefinitionAssociationBinding, MappingDefinitionClassBinding, MappingDefinitionClassBindingDeclaredKeys,
    MappingDefinitionClassBindingPure, MappingDefinitionClassBindingRelational, MappingDefinitionRelationalSource,
    MappingDefinitionRelationalSourceTable, SynthHat, cat,
};
use lite_names::{ModelIndex, synth_fqn};

use crate::error::Thrown;

/// `MappingNormalizer.cleanSheetToCanonical` (:482): the clean-sheet mapping's binding table; its inline lifts
/// appended to `lifted`.
pub fn clean_sheet_to_canonical(
    md: &Arc<CleanSheetMappingDefinition>,
    model: &ModelIndex,
    lifted: &mut Vec<Arc<FunctionDefinition>>,
) -> Result<MappingDefinition, Thrown> {
    let mut class_bindings: Vec<MappingDefinitionClassBinding> = Vec::with_capacity(md.class_bindings.len());
    for cb in md.class_bindings.iter() {
        let fn_fqn: JStr;
        let mut src_body: Option<Arc<[p::ValueSpecification]>> = None;
        match &cb.realization {
            p::Realization::RealizationInline(inl) => {
                fn_fqn = synth_fqn::mapping_class(&md.qualified_name, &cb.class_fqn)?;
                lifted.push(Arc::new(lift_class_inline(md, cb, inl, &fn_fqn)));
                src_body = Some(Arc::from(inl.body.as_slice()));
            }
            p::Realization::RealizationRef(r) => fn_fqn = r.function_fqn.clone(),
        }
        if matches!(cb.kind, CleanSheetMappingDefinitionKind::RELATIONAL) {
            // ref bindings stamp from the REFERENCED function's body (functions register at resolution time) --
            // construction-time derivation, fetched only for the relational kind
            let mut seen: HashSet<JStr> = HashSet::new();
            let body = match src_body {
                Some(body) => body,
                None => {
                    let body = ref_body(&fn_fqn, &md.qualified_name, &cb.class_fqn, model)?;
                    seen.insert(fn_fqn.clone());
                    body
                }
            };
            // a FUNCTION-FORM binding declares no key text (no ~distinct / ~groupBy / ~primaryKey): its key facts are
            // the body's -- not stamped here
            let source = inline_root_source(&body, model, &md.qualified_name, &cb.class_fqn, &mut seen)?;
            class_bindings.push(MappingDefinitionClassBinding::MappingDefinitionClassBindingRelational(
                Arc::new(MappingDefinitionClassBindingRelational {
                    class_fqn: cb.class_fqn.clone(),
                    set_id: cb.set_id.clone(),
                    extends_set_id: cb.extends_set_id.clone(),
                    root: cb.root,
                    function_fqn: fn_fqn,
                    primary_key_columns: cb.primary_key_columns.clone(),
                    declared: MappingDefinitionClassBindingDeclaredKeys::none(),
                    source,
                    aggregate_views: empty(),
                    property_pins: empty(),
                }),
            ));
        } else {
            class_bindings.push(MappingDefinitionClassBinding::MappingDefinitionClassBindingPure(
                Arc::new(MappingDefinitionClassBindingPure {
                    class_fqn: cb.class_fqn.clone(),
                    set_id: cb.set_id.clone(),
                    extends_set_id: cb.extends_set_id.clone(),
                    root: cb.root,
                    function_fqn: fn_fqn,
                    primary_key_columns: cb.primary_key_columns.clone(),
                }),
            ));
        }
    }
    let mut assoc_bindings: Vec<Arc<MappingDefinitionAssociationBinding>> =
        Vec::with_capacity(md.association_bindings.len());
    for ab in md.association_bindings.iter() {
        match &ab.realization {
            p::Realization::RealizationInline(inl) => {
                let fn_fqn = synth_fqn::mapping_assoc(&md.qualified_name, &ab.association_fqn)?;
                lifted.push(Arc::new(lift_assoc_inline(md, ab, inl, &fn_fqn, model)?));
                assoc_bindings.push(Arc::new(MappingDefinitionAssociationBinding {
                    association_fqn: ab.association_fqn.clone(),
                    predicate_function_fqn: fn_fqn,
                }));
            }
            p::Realization::RealizationRef(r) => {
                assoc_bindings.push(Arc::new(MappingDefinitionAssociationBinding {
                    association_fqn: ab.association_fqn.clone(),
                    predicate_function_fqn: r.function_fqn.clone(),
                }));
            }
        }
    }
    Ok(MappingDefinition::common(
        md.qualified_name.clone(),
        md.includes.clone(),
        list(class_bindings),
        list(assoc_bindings),
        md.enumeration_mappings.clone(),
    ))
}

/// `MappingNormalizer.refBody` (:546): a function-REF binding's realizing body, fetched for the stamp; loud when the
/// FQN is no user function with a body.
pub fn ref_body(
    fn_fqn: &JStr,
    mapping_fqn: &JStr,
    class_fqn: &JStr,
    model: &ModelIndex,
) -> Result<Arc<[p::ValueSpecification]>, Thrown> {
    for f in model.find_function(fn_fqn) {
        if let Function::FunctionDefinition(fd) = f {
            return Ok(fd.body.clone());
        }
    }
    Err(Thrown::model(
        Phase::Normalize,
        cat!(
            "clean-sheet binding for '",
            class_fqn,
            "' references '",
            fn_fqn,
            "' which is not a user function with a body; mapping=",
            mapping_fqn
        ),
        None,
    ))
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
    let mut v = if body.len() == 1 { Some(&body[0]) } else { None };
    while let Some(p::ValueSpecification::AppliedFunction(af)) = v {
        if af.function == "tableReference" {
            break;
        }
        if let Some(chained) = user_function_body(&af.function, model) {
            if !seen.insert(af.function.clone()) {
                return Err(Thrown::model(
                    Phase::Normalize,
                    cat!(
                        "clean-sheet relational binding for '",
                        class_fqn,
                        "' has a CYCLIC source-function chain at '",
                        &af.function,
                        "'; mapping=",
                        mapping_fqn
                    ),
                    None,
                ));
            }
            return inline_root_source(&chained, model, mapping_fqn, class_fqn, seen);
        }
        if af.parameters.is_empty() {
            break;
        }
        v = Some(&af.parameters[0]);
    }
    if let Some(p::ValueSpecification::AppliedFunction(tr)) = v
        && tr.function == "tableReference"
        && tr.parameters.len() == 2
        && let p::ValueSpecification::PackageableElementPtr(db) = &tr.parameters[0]
        && let p::ValueSpecification::CString(t) = &tr.parameters[1]
    {
        return Ok(
            MappingDefinitionRelationalSource::MappingDefinitionRelationalSourceTable(Arc::new(
                MappingDefinitionRelationalSourceTable {
                    database: db.full_path.clone(),
                    table: t.value.clone(),
                    aggregation_aware_main: false,
                    enum_columns: empty(),
                },
            )),
        );
    }
    Err(Thrown::model(
        Phase::Normalize,
        cat!(
            "clean-sheet relational binding for '",
            class_fqn,
            "' has no derivable physical source (the body's root is neither a store access nor a user-function \
             chain reaching one); declare the binding Pure or root it at #>{db.TABLE}#; mapping=",
            mapping_fqn
        ),
        None,
    ))
}

/// `MappingNormalizer.userFunctionBody` (:619): the registered user function's body for a call root, else none.
pub fn user_function_body(callee_fqn: &JStr, model: &ModelIndex) -> Option<Arc<[p::ValueSpecification]>> {
    for f in model.find_function(callee_fqn) {
        if let Function::FunctionDefinition(fd) = f {
            return Some(fd.body.clone());
        }
    }
    None
}

/// `MappingNormalizer.liftClassInline` (:635): an inline class binding lifted to `fn_fqn(): C[*]`.
pub fn lift_class_inline(
    md: &CleanSheetMappingDefinition,
    cb: &CleanSheetMappingDefinitionClassBinding,
    inl: &p::RealizationInline,
    fn_fqn: &JStr,
) -> FunctionDefinition {
    FunctionDefinition::user_written(
        fn_fqn.clone(),
        empty(),
        empty(),
        empty(),
        name_ref(cb.class_fqn.clone()),
        // Multiplicity.Concrete.ZERO_MANY
        p::Multiplicity::MultiplicityConcrete(Box::new(p::MultiplicityConcrete {
            lower_bound: 0,
            upper_bound: None,
        })),
        Arc::from(inl.body.as_slice()),
        empty(),
        empty(),
    )
    .with_synthesized_from(Arc::new(FunctionDefinitionSynthesized::new(
        SynthHat::CLASS,
        md.qualified_name.clone(),
        cb.class_fqn.clone(),
    )))
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
    let lam = match inl.body.as_slice() {
        [p::ValueSpecification::LambdaFunction(lam)] => lam,
        _ => {
            return Err(Thrown::model(
                Phase::Normalize,
                cat!(
                    "inline association predicate for '",
                    &ab.association_fqn,
                    "' must be a single (source, target) -> Boolean lambda; mapping=",
                    &md.qualified_name
                ),
                None,
            ));
        }
    };
    if lam.parameters.len() != 2 {
        return Err(Thrown::model(
            Phase::Normalize,
            cat!(
                "inline association predicate for '",
                &ab.association_fqn,
                "' must take exactly 2 parameters (source, target); got ",
                lam.parameters.len().to_string(),
                "; mapping=",
                &md.qualified_name
            ),
            None,
        ));
    }
    let ad = model.find_association(&ab.association_fqn).ok_or_else(|| {
        Thrown::model(
            Phase::Normalize,
            cat!(
                "inline association predicate references unknown association '",
                &ab.association_fqn,
                "'; mapping=",
                &md.qualified_name
            ),
            None,
        )
    })?;
    // `targetClassFqn()`'s throw is an IllegalStateException (lite-model answers its message)
    let class_a = ad.property1.target_class_fqn().map_err(Thrown::illegal_state)?;
    let class_b = ad.property2.target_class_fqn().map_err(Thrown::illegal_state)?;
    let p0 = &lam.parameters[0];
    let p1 = &lam.parameters[1];
    let param_a = Arc::new(FunctionDefinitionParameterDefinition {
        name: p0.name.clone(),
        r#type: name_ref(class_a.clone()),
        multiplicity: pure_one(),
    });
    let param_b = Arc::new(FunctionDefinitionParameterDefinition {
        name: p1.name.clone(),
        r#type: name_ref(class_b.clone()),
        multiplicity: pure_one(),
    });
    Ok(FunctionDefinition::user_written(
        fn_fqn.clone(),
        empty(),
        empty(),
        list(vec![param_a, param_b]),
        name_ref(JStr::from("meta::pure::metamodel::type::Boolean")),
        pure_one(),
        lam.body.clone(),
        empty(),
        empty(),
    )
    .with_synthesized_from(Arc::new(FunctionDefinitionSynthesized::new(
        SynthHat::ASSOC,
        md.qualified_name.clone(),
        ab.association_fqn.clone(),
    ))))
}

/// `new TypeExpression.NameRef(name)`.
fn name_ref(name: JStr) -> p::TypeExpression {
    p::TypeExpression::TypeExpressionNameRef(Box::new(p::TypeExpressionNameRef { name, pos: None }))
}

/// `Multiplicity.Concrete.PURE_ONE`.
fn pure_one() -> p::Multiplicity {
    p::Multiplicity::MultiplicityConcrete(Box::new(p::MultiplicityConcrete {
        lower_bound: 1,
        upper_bound: Some(1),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{index, model};
    use lite_model::text::j;
    use lite_model::{PackageableElement, ParsedModel};

    const DOOR: &str = r#"
Class model::Person { name: String[1]; firmId: Integer[1]; }
Class model::Firm { id: Integer[1]; }
Association model::Person_Firm { firm: model::Firm[1]; person: model::Person[1]; }
function acme::rows(): meta::pure::metamodel::relation::Relation<Any>[1]
{
  #>{acme::DB.T_PERSON}#
}
function acme::viaRows(): meta::pure::metamodel::relation::Relation<Any>[1]
{
  acme::rows()
}
function acme::loopA(): meta::pure::metamodel::relation::Relation<Any>[1]
{
  acme::loopB()
}
function acme::loopB(): meta::pure::metamodel::relation::Relation<Any>[1]
{
  acme::loopA()
}
function acme::noTable(): Integer[1]
{
  1
}

###Relational
Database acme::DB ( Table T_PERSON (NAME VARCHAR(50), FIRM_ID INTEGER) )

###Mapping
Mapping acme::M
(
  *model::Person: Relational { #>{acme::DB.T_PERSON}# -> map(r | ^model::Person(name = $r.NAME->meta::legend::lite::trustOne())) }
  model::Person_Firm: AssociationMapping { {p, f | $p.firmId == $f.id} }
)
Mapping acme::Refs
(
  *model::Person: Relational { acme::viaRows }
  *model::Firm: Pure { acme::noTable }
  model::Person_Firm: AssociationMapping { acme::noTable }
)
"#;

    fn clean_sheet(parsed: &ParsedModel, fqn: &str) -> Arc<CleanSheetMappingDefinition> {
        parsed
            .elements
            .iter()
            .find_map(|e| match e {
                PackageableElement::CleanSheetMappingDefinition(m) if m.qualified_name == j(fqn) => Some(m.clone()),
                _ => None,
            })
            .expect("the clean-sheet mapping")
    }

    fn table(db: &str, t: &str) -> MappingDefinitionRelationalSource {
        MappingDefinitionRelationalSource::MappingDefinitionRelationalSourceTable(Arc::new(
            MappingDefinitionRelationalSourceTable {
                database: j(db),
                table: j(t),
                aggregation_aware_main: false,
                enum_columns: empty(),
            },
        ))
    }

    fn name_of(t: &p::TypeExpression) -> JStr {
        match t {
            p::TypeExpression::TypeExpressionNameRef(nr) => nr.name.clone(),
            _ => panic!("a NameRef"),
        }
    }

    #[test]
    fn an_inline_binding_lifts_and_stamps_its_root_table() {
        let parsed = model(DOOR);
        let idx = index(&parsed);
        let mut lifted = Vec::new();
        let m = clean_sheet_to_canonical(&clean_sheet(&parsed, "acme::M"), &idx, &mut lifted).expect("the table");
        assert_eq!(lifted.len(), 2);
        let class_fn = &lifted[0];
        assert_eq!(class_fn.qualified_name, j("acme::M$class$model::Person"));
        assert!(class_fn.parameters.is_empty());
        assert_eq!(name_of(&class_fn.return_type), j("model::Person"));
        assert_eq!(
            class_fn.return_multiplicity,
            p::Multiplicity::MultiplicityConcrete(Box::new(p::MultiplicityConcrete {
                lower_bound: 0,
                upper_bound: None
            }))
        );
        let hat = class_fn.synthesized_from.as_ref().expect("synthesized");
        assert_eq!(
            (hat.hat, hat.owner_fqn.clone(), hat.member_name.clone()),
            (SynthHat::CLASS, j("acme::M"), j("model::Person"))
        );
        let MappingDefinitionClassBinding::MappingDefinitionClassBindingRelational(b) = &m.class_bindings[0] else {
            panic!("a Relational binding");
        };
        assert_eq!(b.function_fqn, j("acme::M$class$model::Person"));
        assert!(b.root);
        assert_eq!(b.source, table("acme::DB", "T_PERSON"));
        assert_eq!(b.declared, MappingDefinitionClassBindingDeclaredKeys::none());
        // the association's lambda: param names from the user's lambda, types from the ends, Boolean[1]
        let assoc_fn = &lifted[1];
        assert_eq!(assoc_fn.qualified_name, j("acme::M$assoc$model::Person_Firm"));
        let params: Vec<(JStr, JStr)> = assoc_fn
            .parameters
            .iter()
            .map(|p| (p.name.clone(), name_of(&p.r#type)))
            .collect();
        assert_eq!(params, vec![(j("p"), j("model::Firm")), (j("f"), j("model::Person"))]);
        assert_eq!(
            name_of(&assoc_fn.return_type),
            j("meta::pure::metamodel::type::Boolean")
        );
        assert_eq!(assoc_fn.synthesized_from.as_ref().map(|s| s.hat), Some(SynthHat::ASSOC));
        assert_eq!(
            m.association_bindings[0].predicate_function_fqn,
            j("acme::M$assoc$model::Person_Firm")
        );
        // the common form: no stores, no facts
        assert!(m.resolved_stores.is_empty());
    }

    #[test]
    fn a_ref_binding_stamps_through_its_function_chain() {
        let parsed = model(DOOR);
        let idx = index(&parsed);
        let mut lifted = Vec::new();
        let m = clean_sheet_to_canonical(&clean_sheet(&parsed, "acme::Refs"), &idx, &mut lifted).expect("the table");
        assert!(lifted.is_empty(), "a ref binding lifts nothing");
        let MappingDefinitionClassBinding::MappingDefinitionClassBindingRelational(b) = &m.class_bindings[0] else {
            panic!("a Relational binding");
        };
        assert_eq!(
            (b.function_fqn.clone(), b.source.clone()),
            (j("acme::viaRows"), table("acme::DB", "T_PERSON"))
        );
        // a Pure ref binding is not stamped (its function need not reach a table)
        assert!(
            matches!(&m.class_bindings[1], MappingDefinitionClassBinding::MappingDefinitionClassBindingPure(b)
            if b.function_fqn == j("acme::noTable") && b.root)
        );
        assert_eq!(m.association_bindings[0].predicate_function_fqn, j("acme::noTable"));
    }

    #[test]
    fn a_cycle_a_missing_function_and_an_underivable_source_are_loud() {
        let parsed = model(DOOR);
        let idx = index(&parsed);
        let (mf, cf) = (j("acme::X"), j("model::Person"));
        let body = ref_body(&j("acme::loopA"), &mf, &cf, &idx).expect("a body");
        let mut seen = HashSet::from([j("acme::loopA")]);
        let cyc = inline_root_source(&body, &idx, &mf, &cf, &mut seen).unwrap_err();
        assert!(cyc.is_model());
        assert_eq!(
            cyc.message(),
            Some(j(
                "clean-sheet relational binding for 'model::Person' has a CYCLIC source-function chain at \
                 'acme::loopA'; mapping=acme::X"
            ))
        );
        let missing = ref_body(&j("acme::nope"), &mf, &cf, &idx).unwrap_err();
        assert_eq!(
            missing.message(),
            Some(j(
                "clean-sheet binding for 'model::Person' references 'acme::nope' which is not a user function with \
                 a body; mapping=acme::X"
            ))
        );
        let one = ref_body(&j("acme::noTable"), &mf, &cf, &idx).expect("a body");
        let none = inline_root_source(&one, &idx, &mf, &cf, &mut HashSet::new()).unwrap_err();
        assert_eq!(none.phase(), Some(Phase::Normalize));
        assert_eq!(
            none.message(),
            Some(j(
                "clean-sheet relational binding for 'model::Person' has no derivable physical source (the body's \
                 root is neither a store access nor a user-function chain reaching one); declare the binding Pure or \
                 root it at #>{db.TABLE}#; mapping=acme::X"
            ))
        );
        assert!(user_function_body(&j("acme::nope"), &idx).is_none());
    }

    #[test]
    fn an_inline_association_must_be_one_two_parameter_lambda_over_a_known_association() {
        let parsed = model(DOOR);
        let idx = index(&parsed);
        let md = clean_sheet(&parsed, "acme::M");
        let ab = |fqn: &str| CleanSheetMappingDefinitionAssociationBinding {
            association_fqn: j(fqn),
            realization: p::Realization::RealizationInline(Box::new(p::RealizationInline { body: Vec::new() })),
        };
        let lam = |n: usize| {
            p::ValueSpecification::LambdaFunction(Arc::new(p::LambdaFunction {
                parameters: (0..n)
                    .map(|i| {
                        Arc::new(p::Variable {
                            name: JStr::from(format!("v{i}")),
                            r#type: None,
                            multiplicity: None,
                            pos: None,
                        })
                    })
                    .collect(),
                body: empty(),
                pos: None,
            }))
        };
        let fqn = j("acme::M$assoc$x");
        let not_lambda = p::RealizationInline { body: Vec::new() };
        assert_eq!(
            lift_assoc_inline(&md, &ab("model::Person_Firm"), &not_lambda, &fqn, &idx)
                .unwrap_err()
                .message(),
            Some(j(
                "inline association predicate for 'model::Person_Firm' must be a single (source, target) -> Boolean \
                 lambda; mapping=acme::M"
            ))
        );
        let three = p::RealizationInline { body: vec![lam(3)] };
        assert_eq!(
            lift_assoc_inline(&md, &ab("model::Person_Firm"), &three, &fqn, &idx)
                .unwrap_err()
                .message(),
            Some(j(
                "inline association predicate for 'model::Person_Firm' must take exactly 2 parameters (source, \
                 target); got 3; mapping=acme::M"
            ))
        );
        let two = p::RealizationInline { body: vec![lam(2)] };
        assert_eq!(
            lift_assoc_inline(&md, &ab("model::Nope"), &two, &fqn, &idx)
                .unwrap_err()
                .message(),
            Some(j(
                "inline association predicate references unknown association 'model::Nope'; mapping=acme::M"
            ))
        );
    }
}
