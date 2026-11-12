// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! `PlatformPure`: the platform's own Pure -- the system metamodel's function bodies implement their ids over the
//! platform's own rows. The row kind is `Implementation::PlatformPure`; this module is its registration (the ids), the
//! merge that gives an upstream declaration the platform's body ([`adopt`]), and the decisions on upstream's other
//! versions at those names. A version at one of those names with no decision is refused (`NO_ROW`), as a version at a
//! catalog native's name is.
//!
//! Ported from core's `com.legend.platform.PlatformPure` (2026-11-12, docs/rust-port/PHASE3.md slice 1b). Java's two
//! decision tables are `LinkedHashMap`s, in insertion order here ([`LinkedMap`]).

use std::sync::{Arc, LazyLock};

use lite_builtin::system_metamodel;
use lite_error::{CompileError, Phase};
use lite_model::linked::LinkedMap;
use lite_model::{Function, FunctionDefinition, FunctionId, JStr};

use crate::implementation::IllegalArgument;
use crate::walled_bodies::{Kind, Wall};

/// `ids()`: the ids the platform implements in its own Pure -- every function the system metamodel declares, in order.
pub fn ids() -> Vec<FunctionId> {
    system_metamodel::function_ids()
}

/// What [`adopt`] throws.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AdoptError {
    /// `IllegalArgumentException`: the two declarations are not one function.
    IllegalArgument(IllegalArgument),
    /// `ModelException` (phase MODEL, at the id): the two name a parameter differently.
    Model(CompileError),
}

/// `adopt(declaration, platform)`: upstream's declaration with the platform's body -- `declaration` keeps its name,
/// type parameters, parameters, return type, annotations and provenance; its body is `platform`'s. Both have the same
/// id by construction. The body binds the platform's parameter names, so the two must name their parameters alike; a
/// difference is an error naming both, never a silent rebinding.
///
/// The declaration keeps its pre- and post-conditions too: they are the declaration's, not the body's (2026-11-12, the
/// owner's ruling on slice 1b's report: Java's ten-argument constructor dropped them; both sides now keep them). The
/// platform's own versions declare none; one that did is refused (`IllegalArgumentException`), never dropped.
pub fn adopt(
    declaration: &Arc<FunctionDefinition>,
    platform: &Arc<FunctionDefinition>,
) -> Result<Arc<FunctionDefinition>, AdoptError> {
    let id = FunctionId::of(&Function::FunctionDefinition(declaration.clone()));
    let theirs_id = FunctionId::of(&Function::FunctionDefinition(platform.clone()));
    if id != theirs_id {
        return Err(AdoptError::IllegalArgument(IllegalArgument {
            message: cat!("not the same function: ", &id.qualified, " and ", &theirs_id.qualified),
        }));
    }
    if !platform.pre_constraints.is_empty() || !platform.post_constraints.is_empty() {
        return Err(AdoptError::IllegalArgument(IllegalArgument {
            message: cat!(
                "the platform's own Pure declares no conditions: ",
                &id.qualified,
                " declares ",
                platform.pre_constraints.len().to_string(),
                " pre- and ",
                platform.post_constraints.len().to_string(),
                " post-conditions"
            ),
        }));
    }
    for (i, p) in declaration.parameters.iter().enumerate() {
        let theirs = &p.name;
        let ours = &platform.parameters[i].name;
        if theirs != ours {
            let message = cat!(
                "'",
                &id.qualified,
                "' is the platform's own Pure, whose body binds its parameter ",
                (i + 1).to_string(),
                " as '",
                ours,
                "'; this declaration names it '",
                theirs,
                "' — the two must agree"
            );
            return Err(AdoptError::Model(CompileError::model(
                Phase::Model,
                message.0,
                Some(id.qualified.0.clone()),
            )));
        }
    }
    Ok(Arc::new(FunctionDefinition {
        qualified_name: declaration.qualified_name.clone(),
        type_parameters: declaration.type_parameters.clone(),
        multiplicity_parameters: declaration.multiplicity_parameters.clone(),
        parameters: declaration.parameters.clone(),
        return_type: declaration.return_type.clone(),
        return_multiplicity: declaration.return_multiplicity.clone(),
        body: platform.body.clone(),
        stereotypes: declaration.stereotypes.clone(),
        tagged_values: declaration.tagged_values.clone(),
        synthesized_from: declaration.synthesized_from.clone(),
        pre_constraints: declaration.pre_constraints.clone(),
        post_constraints: declaration.post_constraints.clone(),
    }))
}

static UPSTREAM_BODIES: LazyLock<LinkedMap<FunctionId, JStr>> = LazyLock::new(|| {
    let id = |s: &str| FunctionId::new(JStr::from(s));
    let mut t = LinkedMap::new();
    let mut put = |k: &str, why: &str| {
        t.put(id(k), JStr::from(why));
    };
    // platform_dsl_mapping/functions_PropertyMappingsImplementation.pure: the per-kind versions dispatch to, or read
    // through, the platform's propertyMappingsByPropertyName and allPropertyMappings
    put(
        "meta::pure::mapping::propertyMappingsByPropertyName_OtherwiseEmbeddedSetImplementation_1__String_1__PropertyMapping_MANY_",
        concat!(
            "an otherwise-embedded set's property mappings: the platform's allPropertyMappings plus the set's",
            " otherwisePropertyMapping, filtered by name"
        ),
    );
    put(
        "meta::pure::mapping::propertyMappingsByPropertyName_AggregationAwareSetImplementation_1__String_1__PropertyMapping_MANY_",
        "delegates to the aggregation-aware set's main set: the platform's version",
    );
    put(
        "meta::pure::mapping::propertyMappingsByPropertyName_EmbeddedSetImplementation_1__String_1__PropertyMapping_MANY_",
        "dispatches by the embedded set's kind to the platform's versions",
    );
    // core_relational/relational/helperFunctions/helperFunctions.pure
    put(
        "meta::relational::runtime::extractDBs_Mapping_MANY__Runtime_1__Database_MANY_",
        concat!(
            "the databases of some mappings and a runtime: the platform's extractDBs(Mapping) per mapping, plus",
            " the runtime's database connection stores"
        ),
    );
    put(
        "meta::relational::runtime::extractDBs_Mapping_1__Mapping_1__Database_MANY_",
        concat!(
            "upstream's walk of a mapping's includes (its private helper); only upstream's own extractDBs(Mapping)",
            " called it, and the platform's version replaces that body; its body runs for a program that",
            " names it"
        ),
    );
    put(
        "meta::relational::mapping::resolvePrimaryKey_RelationalInstanceSetImplementation_1__RelationalOperationElement_MANY_",
        "dispatches an embedded set to its owner and a root set to the platform's version",
    );
    put(
        "meta::relational::mapping::resolvePrimaryKey_RelationFunctionInstanceSetImplementation_1__TableAliasColumn_MANY_",
        "a relation-function set's keys, upstream's own (getRelationFunctionPkAsTableAliasColumns)",
    );
    put(
        "meta::relational::mapping::resolvePrimaryKey_InstanceSetImplementation_1__RelationalOperationElement_MANY_",
        "dispatches by the set's kind to the relational and relation-function versions",
    );
    t
});

static REFUSED_VERSIONS: LazyLock<LinkedMap<FunctionId, Wall>> = LazyLock::new(|| {
    // core_relational/relational/relationalExtension.pure: the two versions that translate the inferred type to one
    // database's spelling. inferRelationalType(rop, failOnMatchFailure) is not refused: core_relational's own mapping
    // execution calls it, so it is a platform version over the rows.
    let translation = Wall::new(
        Kind::ENGINE_MACHINERY,
        concat!(
            "the engine's translation of a relational type to one database's spelling",
            " (translateCoreTypeToDbSpecificType); the platform's dialects spell types. Its callers in",
            " core_relational are the engine's graph-fetch planner (relationalGraphFetch.pure) and its",
            " test-data generator (testDataGeneration.pure, testRunner.pure), machinery the platform",
            " serves itself"
        ),
    );
    let mut t = LinkedMap::new();
    for k in [
        "meta::relational::functions::typeInference::inferRelationalType_RelationalOperationElement_1__TranslationContext_1__DataType_$0_1$_",
        "meta::relational::functions::typeInference::inferRelationalType_RelationalOperationElement_1__Boolean_1__TranslationContext_1__DataType_$0_1$_",
    ] {
        t.put(FunctionId::new(JStr::from(k)), translation.clone());
    }
    t
});

/// `upstreamBodies()`: upstream's other versions at the platform's own names whose upstream body runs here, by id,
/// each with why, in insertion order.
pub fn upstream_bodies() -> &'static LinkedMap<FunctionId, JStr> {
    &UPSTREAM_BODIES
}

/// `refusedVersions()`: upstream's other versions at the platform's own names refused by decision, by id, each with
/// its wall, in insertion order.
pub fn refused_versions() -> &'static LinkedMap<FunctionId, Wall> {
    &REFUSED_VERSIONS
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{declare, on_stack};

    fn fd(source: &str) -> Arc<FunctionDefinition> {
        match declare(source).remove(0) {
            Function::FunctionDefinition(f) => f,
            Function::NativeFunctionDefinition(_) => panic!("a body"),
        }
    }

    #[test]
    fn a_declaration_takes_the_platforms_body_and_keeps_the_rest() {
        on_stack(|| {
            let theirs = fd("function <<doc.deprecated>> my::pkg::f(x:Integer[1]):Integer[1] { 1 }");
            let ours = fd("function my::pkg::f(x:Integer[1]):Integer[1] { $x }");
            let adopted = adopt(&theirs, &ours).expect("adopted");
            assert_eq!(adopted.body, ours.body);
            assert_eq!(adopted.stereotypes, theirs.stereotypes);
            assert_eq!(adopted.parameters, theirs.parameters);
        });
    }

    #[test]
    fn the_declarations_conditions_are_kept_and_the_platforms_refused() {
        // 2026-11-12, the owner's ruling (item 1): the conditions are the declaration's, not the body's
        on_stack(|| {
            let lite = |s: &str| match crate::testing::declare_at(s, lite_front::doors::Dialect::LegendLite).remove(0) {
                Function::FunctionDefinition(f) => f,
                Function::NativeFunctionDefinition(_) => panic!("a body"),
            };
            let theirs = lite("function my::pkg::f(x:Integer[1]):Integer[1]\n[\n  $x > 0,\n  $return > 0\n]\n{ 1 }");
            assert_eq!((theirs.pre_constraints.len(), theirs.post_constraints.len()), (1, 1));
            let ours = fd("function my::pkg::f(x:Integer[1]):Integer[1] { $x }");
            let adopted = adopt(&theirs, &ours).expect("adopted");
            assert_eq!(adopted.body, ours.body);
            assert_eq!(adopted.pre_constraints, theirs.pre_constraints);
            assert_eq!(adopted.post_constraints, theirs.post_constraints);
            assert_eq!(
                adopt(&ours, &theirs),
                Err(AdoptError::IllegalArgument(IllegalArgument {
                    message: JStr::from(
                        "the platform's own Pure declares no conditions: my::pkg::f_Integer_1__Integer_1_ declares 1 \
                         pre- and 1 post-conditions"
                    )
                }))
            );
        });
    }

    #[test]
    fn a_parameter_named_otherwise_is_the_models_error_naming_both() {
        on_stack(|| {
            let theirs = fd("function my::pkg::f(x:Integer[1], y:String[1]):Integer[1] { 1 }");
            let ours = fd("function my::pkg::f(x:Integer[1], z:String[1]):Integer[1] { $x }");
            let Err(AdoptError::Model(e)) = adopt(&theirs, &ours) else {
                panic!("a model error")
            };
            assert_eq!(e.phase, Some(Phase::Model));
            assert_eq!(
                e.element,
                Some(JStr::from("my::pkg::f_Integer_1__String_1__Integer_1_").0)
            );
            assert_eq!(
                e.message_lossy(),
                "'my::pkg::f_Integer_1__String_1__Integer_1_' is the platform's own Pure, whose body binds its \
                 parameter 2 as 'z'; this declaration names it 'y' — the two must agree"
            );
            let other = fd("function my::pkg::g(x:Integer[1]):Integer[1] { $x }");
            assert_eq!(
                adopt(&theirs, &other),
                Err(AdoptError::IllegalArgument(IllegalArgument {
                    message: JStr::from(
                        "not the same function: my::pkg::f_Integer_1__String_1__Integer_1_ and my::pkg::g_Integer_1__Integer_1_"
                    )
                }))
            );
        });
    }

    #[test]
    fn the_decisions_are_in_their_order() {
        assert_eq!(upstream_bodies().len(), 8);
        assert_eq!(refused_versions().len(), 2);
        assert!(refused_versions().iter().all(|(_, w)| w.kind == Kind::ENGINE_MACHINERY));
    }
}
