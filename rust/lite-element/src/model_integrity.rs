// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! `ModelIntegrity` (ModelIntegrity.java at 18feff3fe): THE eager reference-safety pass (F.a with F.b), run once,
//! whole-model, when the model is built -- every reference an element makes (type names, realizing functions,
//! mapping bindings, association ends, annotations, named stores, mappings, runtimes, connections) exists and, for a
//! behaviour hat, has the right shape. Knowledge only: classification and lookups, no typed element made.
//!
//! **The order is the answer** (EF_CENSUS §2.2: the first failure is `compileErrors`' whole answer; the tolerant
//! door gets the walls in the same order). [`check`] runs the 13 checks of MI:53-122 in Java's order, each a
//! `Result<(), Thrown>`:
//! 1. duplicates (`ModelIndex::refuse_duplicates`, lite-names', MI:224's body);
//! 2. classes, in ingest order (`check_class`);
//! 3. inheritance cycles (`check_inheritance_acyclic`);
//! 4. functions, each under its id (`check_function`);
//! 5. duplicate signatures (`check_duplicate_signatures`, the prior layer's signature keys counted);
//! 6. associations (their ends' types, arguments and bounds, annotations);
//! 7. `GraphEdges` (`graph_edges::read_one` on each fresh non-native class);
//! 8. enums (`check_enum`, annotations);
//! 9. databases (`check_database`);
//! 10. mappings (`check_mapping`);
//! 11. runtimes (`check_runtime`);
//! 12. services (`check_service`);
//! 13. data spaces (their execution contexts' mappings and runtimes).
//!
//! `withElement` (MI:234) is [`with_element`], a fold over a tolerant wall sink: a `ModelException` is caught
//! (nothing else is); with a sink, its first line is recorded (`put_if_absent`) under the exception's element, else
//! the checked FQN, and the pass goes on; without one it is returned, attributed to the checked FQN when it names no
//! element. `fresh` (MI:57) is the prior layer's identity test ([`CheckedLayer`]'s `contains_*`): a runtime, a
//! service or a data space is never in the layer (Java's `checkLayer` adds none), so each is always fresh.
//!
//! The pass classifies types only to check them: in a scratch arena of its own (a `QueryArena` local to [`check`]),
//! never the model's. RE-EXPRESSED (EF_CENSUS §4; its lines are 7c438f7b3's, in parentheses): MI:444's (:434)
//! `catch (IllegalArgumentException)` is a direct bounds test with the same text ([`require_valid_bounds`]);
//! MI:513's (:486) `catch (ModelException) { continue; }` a match on `TypeClassifier::head_fqn`; MI:580's (:553)
//! `getClass().getSimpleName()` the literal names `Other` and `Distinct`. DON'T PORT (EF_CENSUS §3): the 3-argument
//! `check` (MI:38, no caller). The texts in lite's own words (census ASK 12) are ported as they are; their register
//! rows are the slice's step 6.
//!
//! Ported 2026-11-15 (helper C of the element slice), line for line: each check's order of tests and its texts are
//! Java's; Java's inline lambdas for an association, an enum and a data space (MI:73, :96, :116) are the private
//! functions [`check_association`], [`check_enum_element`] and [`check_data_space`].

use std::collections::HashSet;
use std::sync::Arc;

use lite_base::QueryArena;
use lite_error::Phase;
use lite_front::records as p;
use lite_model::linked::LinkedMap;
use lite_model::{
    AssociationDefinition, AssociationDefinitionAssociationEndDefinition, ClassDefinition, DataSpaceDefinition,
    DatabaseDefinition, DatabaseDefinitionTableDefinition, EnumDefinition, Function, FunctionId, JStr,
    MappingDefinition, ProfileDefinition, RelationalDataType, RelationalOperation, RelationalOperationColumnRef,
    RuntimeDefinition, ServiceDefinition, StereotypeApplication, TaggedValue, cat,
};
use lite_names::{ModelIndex, Thrown, synth_fqn};
use lite_types::{Bounded, Type, Var};

use crate::class_compiler::realized_fqn;
use crate::function_compiler::FunctionCompiler;
use crate::graph_edges;
use crate::model::CheckedLayer;
use crate::type_classifier::TypeClassifier;

/// `check(model, classifier, functions, wallSink, prior)` (:53): the 13 checks in their order (the module's docs);
/// strict (`wall_sink` none) the first refusal, attributed; tolerant every failing element's first line in the sink
/// and `Ok`. A raw Java exception a check lets escape (a bound's, an internal `IllegalStateException`) is returned
/// as it is, in both modes, as Java's escapes the pass.
pub fn check(
    model: &ModelIndex,
    classifier: &TypeClassifier<'_>,
    functions: &FunctionCompiler<'_>,
    wall_sink: Option<&mut LinkedMap<JStr, JStr>>,
    prior: Option<&CheckedLayer>,
) -> Result<(), Thrown> {
    let mut wall_sink = wall_sink;
    // the pass's own arena: a type classified only to be checked is dropped with it
    let scratch = QueryArena::new();
    // `fresh` (:57): not the prior layer's very element (by identity); every element without a prior layer
    let fresh_class = |cd: &Arc<ClassDefinition>| prior.is_none_or(|l| !l.contains_class(cd));
    // D6b: element-identity first (:63)
    model.refuse_duplicates(wall_sink.as_deref_mut())?;
    // :64
    for cd in model.classes().filter(|cd| fresh_class(cd)) {
        let work = check_class(cd, classifier, functions, model, &scratch);
        with_element(&cd.qualified_name, work, wall_sink.as_deref_mut())?;
    }
    // :66
    check_inheritance_acyclic(model, classifier, wall_sink.as_deref_mut(), prior)?;
    // a function's wall is under its KEY, its id (:69)
    for f in model
        .functions()
        .filter(|f| prior.is_none_or(|l| !l.contains_function(f)))
    {
        let key = FunctionId::of(f);
        let work = check_function(f, classifier, model, &scratch);
        with_element(&key.qualified, work, wall_sink.as_deref_mut())?;
    }
    // :71
    check_duplicate_signatures(model, wall_sink.as_deref_mut(), prior)?;
    // :73
    for a in model
        .associations()
        .filter(|a| prior.is_none_or(|l| !l.contains_association(a)))
    {
        let work = check_association(a, classifier, model, &scratch);
        with_element(&a.qualified_name, work, wall_sink.as_deref_mut())?;
    }
    // the property-graph profile's rule over the edge classes, one wall per class (:92)
    let graph_source = graph_edges::source_of(model);
    let graph_index = graph_edges::Index::of(&graph_source);
    for cd in model.classes().filter(|cd| fresh_class(cd)).filter(|cd| !cd.is_native) {
        let work = graph_edges::read_one(&graph_source, &graph_index, cd).map(|_| ());
        with_element(&cd.qualified_name, work, wall_sink.as_deref_mut())?;
    }
    // :96
    for ed in model.enums().filter(|ed| prior.is_none_or(|l| !l.contains_enum(ed))) {
        let work = check_enum_element(ed, model);
        with_element(&ed.qualified_name, work, wall_sink.as_deref_mut())?;
    }
    // :103
    for db in model
        .databases()
        .filter(|db| prior.is_none_or(|l| !l.contains_database(db)))
    {
        let work = check_database(db, model);
        with_element(&db.qualified_name, work, wall_sink.as_deref_mut())?;
    }
    // :105
    for md in model
        .mappings()
        .filter(|md| prior.is_none_or(|l| !l.contains_mapping(md)))
    {
        let work = check_mapping(md, model, classifier, functions);
        with_element(&md.qualified_name, work, wall_sink.as_deref_mut())?;
    }
    // the elements that only NAME others (:107); a runtime, a service, a data space is never the prior layer's (its
    // `checkLayer` adds none), so each is fresh
    for rt in model.runtimes() {
        let work = check_runtime(rt, model);
        with_element(&rt.qualified_name, work, wall_sink.as_deref_mut())?;
    }
    for sv in model.services() {
        let work = check_service(sv, model);
        with_element(&sv.qualified_name, work, wall_sink.as_deref_mut())?;
    }
    for ds in model.data_spaces() {
        let work = check_data_space(ds, model);
        with_element(&ds.qualified_name, work, wall_sink.as_deref_mut())?;
    }
    Ok(())
}

/// `check`'s association lambda (:73-88): both ends' types classified and their arguments counted, both ends' bounds
/// (18feff3fe's, :78), then the annotations -- the association's, each end's, each derived property's.
fn check_association(
    a: &AssociationDefinition,
    classifier: &TypeClassifier<'_>,
    model: &ModelIndex,
    scratch: &QueryArena,
) -> Result<(), Thrown> {
    classifier.classify(scratch, &a.property1.target_class, &[])?;
    classifier.classify(scratch, &a.property2.target_class, &[])?;
    classifier.require_arguments(&a.property1.target_class, &[])?;
    classifier.require_arguments(&a.property2.target_class, &[])?;
    require_valid_bounds(&a.property1.multiplicity, &association_end_site(&a.property1, a))?;
    require_valid_bounds(&a.property2.multiplicity, &association_end_site(&a.property2, a))?;
    require_annotations(&a.stereotypes, &a.tagged_values, model)?;
    require_annotations(&a.property1.stereotypes, &a.property1.tagged_values, model)?;
    require_annotations(&a.property2.stereotypes, &a.property2.tagged_values, model)?;
    for dp in a.derived_properties.iter() {
        require_derived_annotations(dp, model)?;
    }
    Ok(())
}

/// `check`'s enum lambda (:96-102): its values ([`check_enum`]), its annotations, each value's.
fn check_enum_element(ed: &EnumDefinition, model: &ModelIndex) -> Result<(), Thrown> {
    check_enum(ed)?;
    require_annotations(&ed.stereotypes, &ed.tagged_values, model)?;
    for va in ed.value_annotations.iter() {
        require_annotations(&va.stereotypes, &va.tagged_values, model)?;
    }
    Ok(())
}

/// `check`'s data space lambda (:116-121): each execution context's mapping and default runtime exist.
fn check_data_space(ds: &DataSpaceDefinition, model: &ModelIndex) -> Result<(), Thrown> {
    for ec in ds.execution_contexts.iter() {
        require_mapping(ec.mapping.as_ref(), model)?;
        require_runtime(ec.default_runtime.as_ref(), model)?;
    }
    Ok(())
}

/// `requireValidBounds(m, site)` (:440, RE-EXPRESSED): Java converts `m` (`TypeClassifier.multiplicity`) and catches
/// its `IllegalArgumentException` as "<site>: invalid multiplicity — <its message>" (a `ModelException`, MODEL);
/// here each refusal the conversion can raise is tested directly, with the same text: a concrete multiplicity's
/// "upper (U) must be >= lower (L)" and "lower must be >= 0, got L", and a variable's "multiplicity parameter name
/// must be non-empty" (the last two believed unreachable from the parser, which refuses a negative lower bound and an
/// empty name first: ported as Java has them all the same). The test is the type family's own constructors' (lite-types'
/// `Bounded::new`, `Var::new`: `Multiplicity::from`'s), so the texts are theirs; no type is made.
pub fn require_valid_bounds(m: &p::Multiplicity, site: &JStr) -> Result<(), Thrown> {
    let checked = match m {
        p::Multiplicity::MultiplicityConcrete(c) => Bounded::new(c.lower_bound, c.upper_bound).map(|_| ()),
        p::Multiplicity::MultiplicityParameter(v) => Var::new(&v.name.lossy()).map(|_| ()),
    };
    match checked {
        Ok(()) => Ok(()),
        // the catch (:444) takes an IllegalArgumentException alone
        Err(e) if e.thrown == lite_types::Thrown::IllegalArgument => Err(model_exception(cat!(
            site,
            ": invalid multiplicity — ",
            e.message.as_str()
        ))),
        Err(e) => Err(Thrown::from_type_error(e)),
    }
}

/// `requireValidBounds(m, site, element)` (:454; 18feff3fe's addition, PMC:464's caller): [`require_valid_bounds`]
/// outside the pass, where no `with_element` attributes the refusal: attributed to `element`, as the pass's wall
/// would be.
pub fn require_valid_bounds_at(m: &p::Multiplicity, site: &JStr, element: &JStr) -> Result<(), Thrown> {
    require_valid_bounds(m, site).map_err(|t| match t {
        // `new ModelException(e.phase(), e.getMessage(), element)` (:458): the same phase and message
        Thrown::Compile(mut e) if is_model_exception(&e) => {
            e.element = Some(element.0.clone());
            Thrown::Compile(e)
        }
        other => other,
    })
}

/// `associationEndSite(end, association)` (:463; 18feff3fe's addition): an association end's site in the bounds
/// check's words, `property 'p' of association a::A`.
pub fn association_end_site(
    end: &AssociationDefinitionAssociationEndDefinition,
    association: &AssociationDefinition,
) -> JStr {
    cat!(
        "property '",
        &end.property_name,
        "' of association ",
        &association.qualified_name
    )
}

/// `withElement(elementFqn, work, wallSink)` (:234): the fold (the module's docs). `work` is the check's outcome.
/// Only a `ModelException` is caught (Java's class is final): with a sink, its first line (`String.valueOf(message)
/// .split("\n")[0]`) is put if absent under its own element, else `element_fqn`, and the pass goes on; without one it
/// is returned, given `element_fqn` when it names no element (`new ModelException(phase, message, elementFqn)`).
/// Anything else is returned as it is, in both modes.
fn with_element(
    element_fqn: &JStr,
    work: Result<(), Thrown>,
    wall_sink: Option<&mut LinkedMap<JStr, JStr>>,
) -> Result<(), Thrown> {
    let e = match work {
        Ok(()) => return Ok(()),
        Err(Thrown::Compile(e)) if is_model_exception(&e) => e,
        Err(other) => return Err(other),
    };
    if let Some(sink) = wall_sink {
        // POISON-NOT-DROP: record the first reason and keep the element
        let key = e.element.clone().map_or_else(|| element_fqn.clone(), JStr);
        let line = Thrown::Compile(e).first_line();
        sink.put_if_absent(key, line);
        return Ok(());
    }
    if e.element.is_some() {
        return Err(Thrown::Compile(e));
    }
    let mut attributed = e;
    attributed.element = Some(element_fqn.0.clone());
    Err(Thrown::Compile(attributed))
}

/// Whether a `com.legend.error` throw is a `ModelException` (the class `withElement` and the attributed bounds check
/// catch).
fn is_model_exception(e: &lite_error::CompileError) -> bool {
    e.kind == lite_error::ErrorKind::Model
}

/// `new ModelException(Phase.MODEL, message)`: every refusal of the pass's own.
fn model_exception(message: JStr) -> Thrown {
    Thrown::model(Phase::Model, message, None)
}

/// `checkRuntime(rt, model)` (:124): its mappings, its connection bindings' stores and connections exist.
fn check_runtime(rt: &RuntimeDefinition, model: &ModelIndex) -> Result<(), Thrown> {
    for mapping in rt.mappings.iter() {
        require_mapping(Some(mapping), model)?;
    }
    for (store, connections) in rt.connection_bindings.iter() {
        require_store(Some(store), model)?;
        for connection in connections.iter() {
            if model.find_connection(connection).is_none()
                && !model.is_model_connection(connection)
                && !model.has_element(connection)
            {
                return Err(model_exception(cat!("Can't find connection '", connection, "'")));
            }
        }
    }
    Ok(())
}

/// `checkService(sv, model)` (:140): its mapping and runtime, and each keyed execution's, exist.
fn check_service(sv: &ServiceDefinition, model: &ModelIndex) -> Result<(), Thrown> {
    require_mapping(sv.mapping_ref.as_ref(), model)?;
    require_runtime(sv.runtime_ref.as_ref(), model)?;
    if let Some(multi) = &sv.multi_execution {
        for kx in multi.executions.iter() {
            require_mapping(kx.mapping.as_ref(), model)?;
            require_runtime(kx.runtime.as_ref(), model)?;
        }
    }
    Ok(())
}

/// `requireAnnotations(stereotypes, taggedValues, model)` (:157): each names a profile the model has and one that
/// profile declares (legend-engine's `getProfile`/`getStereotype`/`getTag` words).
fn require_annotations(
    stereotypes: &[Arc<StereotypeApplication>],
    tagged_values: &[Arc<TaggedValue>],
    model: &ModelIndex,
) -> Result<(), Thrown> {
    for st in stereotypes {
        if !profile(&st.profile_name, model)?
            .stereotypes
            .contains(&st.stereotype_name)
        {
            return Err(model_exception(cat!(
                "Can't find stereotype '",
                &st.stereotype_name,
                "' in profile '",
                &st.profile_name,
                "'"
            )));
        }
    }
    for tv in tagged_values {
        if !profile(&tv.profile_name, model)?.tags.contains(&tv.tag_name) {
            return Err(model_exception(cat!(
                "Can't find tag '",
                &tv.tag_name,
                "' in profile '",
                &tv.profile_name,
                "'"
            )));
        }
    }
    Ok(())
}

/// `requireAnnotations(dp, model)` (:174): a derived property's annotations (protocol records), checked as any.
fn require_derived_annotations(dp: &p::DerivedPropertyDefinition, model: &ModelIndex) -> Result<(), Thrown> {
    let stereotypes: Vec<Arc<StereotypeApplication>> = dp
        .stereotypes
        .iter()
        .map(|st| {
            Arc::new(StereotypeApplication {
                profile_name: st.profile.clone(),
                stereotype_name: st.value.clone(),
            })
        })
        .collect();
    let tagged_values: Vec<Arc<TaggedValue>> = dp
        .tagged_values
        .iter()
        .map(|tv| {
            Arc::new(TaggedValue {
                profile_name: tv.tag.profile.clone(),
                tag_name: tv.tag.value.clone(),
                value: tv.value.clone(),
            })
        })
        .collect();
    require_annotations(&stereotypes, &tagged_values, model)
}

/// `profile(fqn, model)` (:186): the profile, or "Can't find the profile '<fqn>'".
fn profile<'i>(fqn: &JStr, model: &'i ModelIndex) -> Result<&'i Arc<ProfileDefinition>, Thrown> {
    model
        .find_profile(fqn)
        .ok_or_else(|| model_exception(cat!("Can't find the profile '", fqn, "'")))
}

/// `requireMapping(mapping, model)` (:192): a named mapping exists (none: a reference not written).
fn require_mapping(mapping: Option<&JStr>, model: &ModelIndex) -> Result<(), Thrown> {
    if let Some(mapping) = mapping
        && model.find_mapping(mapping).is_none()
    {
        return Err(model_exception(cat!("Can't find mapping '", mapping, "'")));
    }
    Ok(())
}

/// `requireRuntime(runtime, model)` (:200): a named runtime exists (none: not written, or inline).
fn require_runtime(runtime: Option<&JStr>, model: &ModelIndex) -> Result<(), Thrown> {
    if let Some(runtime) = runtime
        && model.find_runtime(runtime).is_none()
    {
        return Err(model_exception(cat!("Can't find runtime '", runtime, "'")));
    }
    Ok(())
}

/// `requireStore(store, model)` (:208): the model store, a database or another store element exists.
fn require_store(store: Option<&JStr>, model: &ModelIndex) -> Result<(), Thrown> {
    if let Some(store) = store
        && !store.eq_str("ModelStore")
        && model.find_database(store).is_none()
        && !model.has_element(store)
    {
        return Err(model_exception(cat!("The store '", store, "' can't be found.")));
    }
    Ok(())
}

/// `checkClass(cd, classifier, functions, model)` (:260): annotations; supertypes (classified, their arguments
/// where written, a class: "Invalid supertype"); stored properties (no duplicate name, classified, arguments, bounds,
/// a default only when mandatory); derived properties (annotations, types, bounds, the realizing function exists);
/// constraints (the realizing function exists and returns `Boolean[1]`). Types in `scratch`.
fn check_class(
    cd: &ClassDefinition,
    classifier: &TypeClassifier<'_>,
    functions: &FunctionCompiler<'_>,
    model: &ModelIndex,
    scratch: &QueryArena,
) -> Result<(), Thrown> {
    let type_params: &[JStr] = &cd.type_params;
    let qn = &cd.qualified_name;
    require_annotations(&cd.stereotypes, &cd.tagged_values, model)?;
    for pd in cd.properties.iter() {
        require_annotations(&pd.stereotypes, &pd.tagged_values, model)?;
    }
    for sup in cd.super_classes.iter() {
        // a supertype is a type that exists (its head and each argument: classify) and a class, in legend-engine's
        // words (:268); its arguments counted where written (lite's dialect), a bare one's arity not (:275)
        classifier.classify(scratch, sup, type_params)?;
        if let p::TypeExpression::TypeExpressionGeneric(_) = sup {
            classifier.require_arguments(sup, type_params)?;
        }
        let sup_fqn = TypeClassifier::head_fqn(sup)?;
        // its kind, not its catalog entry; a type parameter is no class either (:280)
        if type_params.contains(sup_fqn)
            || !matches!(
                classifier
                    .find_type(scratch, sup_fqn)
                    .ok_or_else(|| Thrown::illegal_state(cat!(sup_fqn, " classified as a type but has none")))?,
                Type::ClassType(_)
            )
        {
            return Err(model_exception(cat!(
                "Invalid supertype: '",
                simple_name(qn)?,
                "' cannot extend '",
                sup_fqn,
                "' as it is not a class."
            )));
        }
    }
    let mut property_names: HashSet<JStr> = HashSet::new();
    for pd in cd.properties.iter() {
        if !property_names.insert(pd.name.clone()) {
            return Err(model_exception(cat!(
                "Found duplicated property '",
                &pd.name,
                "' in class '",
                qn,
                "'"
            )));
        }
        classifier.classify(scratch, &pd.r#type, type_params)?;
        classifier.require_arguments(&pd.r#type, type_params)?;
        require_valid_bounds(&pd.multiplicity, &cat!("property '", &pd.name, "' of ", qn))?;
        if pd.has_default
            && let p::Multiplicity::MultiplicityConcrete(m) = &pd.multiplicity
            && m.lower_bound == 0
        {
            // legend-pure's PropertyValidator, in its words (:304)
            return Err(model_exception(cat!(
                "Default values are supported only for mandatory fields, and property '",
                &pd.name,
                "' is optional."
            )));
        }
    }
    for dp in cd.derived_properties.iter() {
        require_derived_annotations(dp, model)?;
        classifier.classify(scratch, &dp.r#type, type_params)?;
        classifier.require_arguments(&dp.r#type, type_params)?;
        require_valid_bounds(&dp.multiplicity, &cat!("derived property '", &dp.name, "' of ", qn))?;
        for pp in dp.parameters.iter() {
            classifier.classify(scratch, &pp.r#type, type_params)?;
            classifier.require_arguments(&pp.r#type, type_params)?;
            require_valid_bounds(
                &pp.multiplicity,
                &cat!(
                    "parameter '",
                    &pp.name,
                    "' of derived property '",
                    &dp.name,
                    "' of ",
                    qn
                ),
            )?;
        }
        // the lifted name is made whether or not the realization is a reference (:325: Java evaluates it first)
        let fqn = realized_fqn(&dp.realization, synth_fqn::prop(qn, &dp.name)?);
        functions.require_function(&fqn, &cat!("derived property '", &dp.name, "' of ", qn))?;
    }
    for con in cd.constraints.iter() {
        let fqn = realized_fqn(&con.realization, synth_fqn::constraint(qn, &con.name)?);
        let site = cat!("constraint '", &con.name, "' of ", qn);
        functions.require_function(&fqn, &site)?;
        functions.require_shape(
            &fqn,
            FunctionCompiler::returns_boolean_one,
            &site,
            "returning Boolean[1]",
        )?;
    }
    Ok(())
}

/// `Protocol.splitFqn(fqn)[1]` (:286): the name half of an FQN, split at its last `::` outside a quoted segment,
/// both halves unquoted (the package's first, as Java builds the array). Its only refusal is a quoted segment's short
/// unicode escape, Java's raw `IllegalArgumentException` from `Escapes.unescapeJavaLike` ("Less than 4 hex digits in
/// unicode escape"), which escapes the pass; lite-front's `split_fqn` gives it as the parser's refusal, so it is
/// re-made here as Java's (believed unreachable: the FQN was split and unquoted once already, when the class was read).
fn simple_name(fqn: &JStr) -> Result<JStr, Thrown> {
    lite_front::protocol::split_fqn(&fqn.0)
        .map(|(_, name)| JStr(name))
        .map_err(|_| Thrown::illegal_argument(JStr::from(lite_front::escapes::SHORT_UNICODE_ESCAPE)))
}

/// `checkDuplicateSignatures(model, wallSink, fresh, priorKeys)` (:343): a fresh function whose id a prior function
/// or an earlier fresh one has, walled under its id ("... defined more than once with the same signature ...").
fn check_duplicate_signatures(
    model: &ModelIndex,
    wall_sink: Option<&mut LinkedMap<JStr, JStr>>,
    prior: Option<&CheckedLayer>,
) -> Result<(), Thrown> {
    let mut wall_sink = wall_sink;
    let no_keys: HashSet<FunctionId> = HashSet::new();
    let prior_keys = prior.map_or(&no_keys, |l| l.signature_keys());
    let mut seen: HashSet<FunctionId> = HashSet::new();
    for f in model
        .functions()
        .filter(|f| prior.is_none_or(|l| !l.contains_function(f)))
    {
        let key = FunctionId::of(f);
        // a key the prior layer has is never added to `seen` (Java's `||` short-circuits)
        if prior_keys.contains(&key) || !seen.insert(key.clone()) {
            let work = Err(model_exception(cat!(
                "function '",
                f.qualified_name(),
                "' is defined more than once with the same signature — calls would be ambiguous"
            )));
            with_element(&key.qualified, work, wall_sink.as_deref_mut())?;
        }
    }
    Ok(())
}

/// `checkFunction(f, classifier, model)` (:362): annotations; each parameter and the return classified, their
/// arguments counted unless the function is synthesized, their bounds valid. Types in `scratch`.
fn check_function(
    f: &Function,
    classifier: &TypeClassifier<'_>,
    model: &ModelIndex,
    scratch: &QueryArena,
) -> Result<(), Thrown> {
    require_annotations(f.stereotypes(), f.tagged_values(), model)?;
    // a declaration's types name their classes' arguments; lite's own synthesized signatures name a mapped class bare
    let declared = !matches!(f, Function::FunctionDefinition(fd) if fd.is_synthesized());
    let type_params: &[JStr] = f.type_parameters();
    for pp in f.parameters().iter() {
        classifier.classify(scratch, &pp.r#type, type_params)?;
        if declared {
            classifier.require_arguments(&pp.r#type, type_params)?;
        }
        require_valid_bounds(
            &pp.multiplicity,
            &cat!("parameter '", &pp.name, "' of function ", f.qualified_name()),
        )?;
    }
    classifier.classify(scratch, f.return_type(), type_params)?;
    if declared {
        classifier.require_arguments(f.return_type(), type_params)?;
    }
    require_valid_bounds(
        f.return_multiplicity(),
        &cat!("return of function ", f.qualified_name()),
    )
}

/// `checkMapping(md, model, classifier, functions)` (:391): includes' substituted stores exist; each class binding
/// names a class realized by a `(): Class[*]` function; each association binding an association realized by a
/// `(source, target): Boolean[1]` predicate.
fn check_mapping(
    md: &MappingDefinition,
    model: &ModelIndex,
    classifier: &TypeClassifier<'_>,
    functions: &FunctionCompiler<'_>,
) -> Result<(), Thrown> {
    let qn = &md.qualified_name;
    // an include's store substitutions name stores; the included mapping itself is the normalizer's to find (:393)
    for inc in md.includes.iter() {
        for sub in inc.substitutions.iter() {
            require_store(Some(&sub.original_store), model)?;
            require_store(Some(&sub.replacement_store), model)?;
        }
    }
    for cb in md.class_bindings.iter() {
        if !classifier.is_class_fqn(cb.class_fqn()) {
            return Err(model_exception(cat!(
                "mapping '",
                qn,
                "' binds unknown class '",
                cb.class_fqn(),
                "'"
            )));
        }
        let site = cat!("mapping '", qn, "' class binding for '", cb.class_fqn(), "'");
        functions.require_function(cb.function_fqn(), &site)?;
        functions.require_shape(
            cb.function_fqn(),
            |f| f.parameters().is_empty() && functions.returns_class_many(f),
            &site,
            "of the form (): Class[*]",
        )?;
    }
    for ab in md.association_bindings.iter() {
        if model.find_association(&ab.association_fqn).is_none() {
            return Err(model_exception(cat!(
                "mapping '",
                qn,
                "' binds unknown association '",
                &ab.association_fqn,
                "'"
            )));
        }
        let site = cat!("mapping '", qn, "' association binding for '", &ab.association_fqn, "'");
        functions.require_function(&ab.predicate_function_fqn, &site)?;
        functions.require_shape(
            &ab.predicate_function_fqn,
            |f| f.parameters().len() == 2 && FunctionCompiler::returns_boolean_one(f),
            &site,
            "of the form (source, target): Boolean[1]",
        )?;
    }
    Ok(())
}

/// `checkEnum(ed)` (:469): no value twice ("Found duplicated value 'X' in enumeration 'E'").
fn check_enum(ed: &EnumDefinition) -> Result<(), Thrown> {
    let mut seen: HashSet<&JStr> = HashSet::new();
    for v in ed.values.iter() {
        if !seen.insert(v) {
            return Err(model_exception(cat!(
                "Found duplicated value '",
                v,
                "' in enumeration '",
                &ed.qualified_name,
                "'"
            )));
        }
    }
    Ok(())
}

/// `checkInheritanceAcyclic(model, classifier, wallSink, fresh)` (:485): every fresh class's supers walked
/// (`walk_supers`), a cycle "Inheritance cycle: a -> b -> a", one wall per class. A prior layer cannot reach a graph
/// class (it was checked alone), so every cycle through a graph class starts at one; the classes found acyclic are
/// shared by the walks, each walk's path its own.
fn check_inheritance_acyclic(
    model: &ModelIndex,
    classifier: &TypeClassifier<'_>,
    wall_sink: Option<&mut LinkedMap<JStr, JStr>>,
    prior: Option<&CheckedLayer>,
) -> Result<(), Thrown> {
    let mut wall_sink = wall_sink;
    let mut acyclic: HashSet<JStr> = HashSet::new();
    for cd in model.classes().filter(|cd| prior.is_none_or(|l| !l.contains_class(cd))) {
        let work = walk_supers(cd, classifier, &mut LinkedMap::new(), &mut acyclic);
        with_element(&cd.qualified_name, work, wall_sink.as_deref_mut())?;
    }
    Ok(())
}

/// `walkSupers(cd, classifier, path, acyclic)` (:497): depth first along `path` (insertion-ordered); a malformed head
/// is skipped (RE-EXPRESSED MI:513, the census's :486), an unknown super ends the walk there. A refusal leaves `path`
/// and `acyclic` as Java's throw does (the class neither removed from the one nor added to the other).
fn walk_supers(
    cd: &ClassDefinition,
    classifier: &TypeClassifier<'_>,
    path: &mut LinkedMap<JStr, ()>,
    acyclic: &mut HashSet<JStr>,
) -> Result<(), Thrown> {
    let fqn = &cd.qualified_name;
    if acyclic.contains(fqn) {
        return Ok(());
    }
    if !path.put_if_absent(fqn.clone(), ()) {
        let mut message = cat!("Inheritance cycle: ");
        for (i, (step, ())) in path.iter().enumerate() {
            if i > 0 {
                message.0.extend(" -> ".encode_utf16());
            }
            message.0.extend_from_slice(&step.0);
        }
        return Err(model_exception(cat!(message, " -> ", fqn)));
    }
    for sup in cd.super_classes.iter() {
        let sup_fqn = match TypeClassifier::head_fqn(sup) {
            Ok(sup_fqn) => sup_fqn,
            // malformed head: checkClass's classify reports it (:514, a ModelException alone)
            Err(e) if e.is_model() => continue,
            Err(e) => return Err(e),
        };
        if let Some(sc) = classifier.class_def(sup_fqn) {
            walk_supers(sc, classifier, path, acyclic)?;
        }
    }
    path.remove(fqn);
    acyclic.insert(fqn.clone());
    Ok(())
}

/// `checkDatabase(db, model)` (:533): includes exist; joins', filters' and multigrain filters' column references
/// resolve; milestoning columns are dates.
fn check_database(db: &DatabaseDefinition, model: &ModelIndex) -> Result<(), Thrown> {
    for include in db.includes.iter() {
        if model.find_database(include).is_none() {
            return Err(model_exception(cat!("Can't find database '", include, "'")));
        }
    }
    for j in db.joins.iter() {
        check_store_refs(db, model, &j.operation, &cat!("join '", &j.name, "'"))?;
    }
    for f in db.filters.iter() {
        check_store_refs(db, model, &f.condition, &cat!("filter '", &f.name, "'"))?;
    }
    for f in db.multi_grain_filters.iter() {
        check_store_refs(db, model, &f.condition, &cat!("multigrain filter '", &f.name, "'"))?;
    }
    for td in db.tables.iter() {
        check_milestoning_column_types(db, td)?;
    }
    Ok(())
}

/// `checkMilestoningColumnTypes(db, td)` (:561): a milestoning column declared OTHER or DISTINCT refused, by the
/// literal type names (RE-EXPRESSED MI:580's, the census's :553, `getSimpleName`).
fn check_milestoning_column_types(
    db: &DatabaseDefinition,
    td: &DatabaseDefinitionTableDefinition,
) -> Result<(), Thrown> {
    if td.milestoning.is_none() {
        return Ok(());
    }
    for mc in milestoning_columns(td).into_iter().flatten() {
        for c in td.columns.iter() {
            let declared = match &c.data_type {
                RelationalDataType::RelationalDataTypeOther(_) => Some("Other"),
                RelationalDataType::RelationalDataTypeDistinct(_) => Some("Distinct"),
                _ => None,
            };
            if c.name == *mc
                && let Some(declared) = declared
            {
                return Err(model_exception(cat!(
                    "The milestoning column '",
                    mc,
                    "' of table '",
                    &td.name,
                    "' in database '",
                    &db.qualified_name,
                    "' is declared ",
                    declared,
                    ": a milestoning date must be a DATE or TIMESTAMP column"
                )));
            }
        }
    }
    Ok(())
}

/// The six milestoning columns MI:569 and MI:653 list, in their order (business `from`, `thru`, `snapshotDate`,
/// processing `in`, `out`, `snapshotDate`), each none when not declared (Java's `null`, which equals no column name).
fn milestoning_columns(td: &DatabaseDefinitionTableDefinition) -> [Option<&JStr>; 6] {
    let m = td.milestoning.as_deref();
    let b = m.and_then(|m| m.business.as_deref());
    let pr = m.and_then(|m| m.processing.as_deref());
    [
        b.and_then(|b| b.from.as_ref()),
        b.and_then(|b| b.thru.as_ref()),
        b.and_then(|b| b.snapshot_date.as_ref()),
        pr.and_then(|pr| pr.r#in.as_ref()),
        pr.and_then(|pr| pr.out.as_ref()),
        pr.and_then(|pr| pr.snapshot_date.as_ref()),
    ]
}

/// `checkStoreRefs(db, model, op, site)` (:587): each column reference of the operation not qualified by a foreign
/// database, through its children.
fn check_store_refs(
    db: &DatabaseDefinition,
    model: &ModelIndex,
    op: &RelationalOperation,
    site: &JStr,
) -> Result<(), Thrown> {
    if let RelationalOperation::RelationalOperationColumnRef(cr) = op
        // an explicit FOREIGN database qualifier resolves through lookups this pass doesn't own: skipped (:591)
        && cr.database_name.as_ref().is_none_or(|d| *d == db.qualified_name)
    {
        check_column_ref(db, model, cr, site)?;
    }
    for child in op.children() {
        check_store_refs(db, model, child, site)?;
    }
    Ok(())
}

/// `checkColumnRef(db, model, cr, site)` (:603): the table (include closure) has the column, else the view does,
/// else the table is unknown (each in its words).
fn check_column_ref(
    db: &DatabaseDefinition,
    model: &ModelIndex,
    cr: &RelationalOperationColumnRef,
    site: &JStr,
) -> Result<(), Thrown> {
    if let Some(table) = model.find_table_definition(&db.qualified_name, &cr.table) {
        if !has_column(table, &cr.column) {
            return Err(model_exception(cat!(
                "The column '",
                &cr.column,
                "' can't be found in the table '",
                &table.name,
                "' (",
                site,
                " of database '",
                &db.qualified_name,
                "')"
            )));
        }
        return Ok(());
    }
    if let Some(view) = model.find_view(&db.qualified_name, &cr.table) {
        let hit = view.column_mappings.iter().any(|cm| cm.name == cr.column);
        if !hit {
            return Err(model_exception(cat!(
                "The column '",
                &cr.column,
                "' can't be found in the view '",
                &view.name,
                "' (",
                site,
                " of database '",
                &db.qualified_name,
                "')"
            )));
        }
        return Ok(());
    }
    Err(model_exception(cat!(
        "The table '",
        &cr.table,
        "' can't be found in the database '",
        &db.qualified_name,
        "' (",
        site,
        ")"
    )))
}

/// `hasColumn(td, name)` (:642): a declared column, or a milestoning-declared temporal column, by bare name.
fn has_column(td: &DatabaseDefinitionTableDefinition, name: &JStr) -> bool {
    if td.columns.iter().any(|c| c.name == *name) {
        return true;
    }
    milestoning_columns(td).into_iter().flatten().any(|mc| mc == name)
}

#[cfg(test)]
mod tests {
    //! The pass's refusals, each text asserted whole: the Java tests' cases (ElementRefusalsTest, LeniencyD6Test,
    //! ReferenceIntegrityTest: the F-time ones) and each check's own. A model text is parsed, resolved and indexed as
    //! lite-normalizer's test support does, with no boot layer (no prelude: profiles are declared in the text) and no
    //! Phase E (a canonical mapping is built by hand and added). A test whose path reaches a fill not yet in this
    //! worktree (the classifier's `classify`, `require_arguments`, `head_fqn`, `find_type`: helper A; the function
    //! compiler: helper B; `graph_edges::Index::of`, `read_one`: helper D, which every whole pass past its first failing
    //! class reaches) is ignored, by name.

    use super::*;
    use lite_error::ErrorKind;
    use lite_front::doors::Dialect;
    use lite_model::linked::{empty, list};
    use lite_model::{MappingDefinitionAssociationBinding, MappingDefinitionClassBindingPure, PackageableElement};

    fn j(s: &str) -> JStr {
        JStr::from(s)
    }

    /// A model text parsed (lite's dialect), its names resolved and indexed: the graph's own elements as F's index
    /// holds them before E's products join it.
    fn index(text: &str) -> ModelIndex {
        let units: Vec<u16> = text.encode_utf16().collect();
        let parsed = lite_builtin::door::parse(&units, Dialect::LegendLite).expect("the model parses");
        let resolved = lite_names::resolver::resolve(&parsed).expect("the model's names resolve");
        ModelIndex::from(&resolved).expect("the model indexes")
    }

    /// The whole pass over `idx`, strict.
    fn strict(idx: &ModelIndex) -> Result<(), Thrown> {
        let classifier = TypeClassifier::new(idx);
        let functions = FunctionCompiler::new(idx, classifier);
        check(idx, &classifier, &functions, None, None)
    }

    /// The whole pass over `idx`, tolerant: its walls in order.
    fn tolerant(idx: &ModelIndex) -> Vec<(String, String)> {
        let classifier = TypeClassifier::new(idx);
        let functions = FunctionCompiler::new(idx, classifier);
        let mut walls = LinkedMap::new();
        check(idx, &classifier, &functions, Some(&mut walls), None).expect("a tolerant pass walls");
        walls.iter().map(|(k, v)| (k.lossy(), v.lossy())).collect()
    }

    /// A `ModelException` (MODEL)'s message and element.
    fn refusal(r: Result<(), Thrown>) -> (String, Option<String>) {
        match r {
            Err(Thrown::Compile(e)) => {
                assert_eq!(e.kind, ErrorKind::Model, "{e:?}");
                assert_eq!(e.phase, Some(Phase::Model), "{e:?}");
                let message = String::from_utf16(&e.message.expect("a message")).expect("UTF-16");
                (message, e.element.map(|el| String::from_utf16(&el).expect("UTF-16")))
            }
            other => panic!("expected a ModelException, got {other:?}"),
        }
    }

    /// A `ModelException` (MODEL)'s message, naming no element.
    fn refused(r: Result<(), Thrown>) -> String {
        let (message, element) = refusal(r);
        assert_eq!(element, None, "{message}");
        message
    }

    fn concrete(lower: i32, upper: Option<i32>) -> p::Multiplicity {
        p::Multiplicity::MultiplicityConcrete(Box::new(p::MultiplicityConcrete {
            lower_bound: lower,
            upper_bound: upper,
        }))
    }

    fn variable(name: &str) -> p::Multiplicity {
        p::Multiplicity::MultiplicityParameter(Box::new(p::MultiplicityParameter { name: j(name) }))
    }

    /// `upper (1) must be >= lower (2)`, a `[2..1]`'s text.
    const BOUND: &str = "upper (1) must be >= lower (2)";

    /// ReferenceIntegrityTest's BASE (`###Pure` dropped: the text's first section).
    const BASE: &str = "Profile a::P\n{\n  stereotypes: [s];\n  tags: [t];\n}\nClass a::C\n{\n  x: Integer[1];\n  name: \
                        String[1];\n}\nEnum a::E\n{\n  A, B\n}\n";

    const DB: &str = "###Relational\nDatabase a::Db\n(\n  Table T (X INTEGER)\n)\n";

    // ---- withElement (:234)

    #[test]
    fn a_strict_refusal_naming_no_element_is_given_the_checked_one() {
        let r = with_element(&j("a::A"), Err(model_exception(j("bad\nits detail"))), None);
        assert_eq!(refusal(r), ("bad\nits detail".to_owned(), Some("a::A".to_owned())));
    }

    #[test]
    fn a_strict_refusal_naming_its_element_keeps_it() {
        let r = with_element(
            &j("a::A"),
            Err(Thrown::model(Phase::Model, j("bad"), Some(&j("a::Super")))),
            None,
        );
        assert_eq!(refusal(r), ("bad".to_owned(), Some("a::Super".to_owned())));
        // the phase the exception has, kept (the normalizer's phase, as a check could raise it)
        let normalize = Thrown::model(Phase::Normalize, j("e"), None);
        match with_element(&j("a::A"), Err(normalize), None) {
            Err(Thrown::Compile(e)) => assert_eq!((e.phase, e.element), (Some(Phase::Normalize), Some(j("a::A").0))),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_tolerant_refusal_walls_its_first_line_and_the_first_reason_stays() {
        let mut walls = LinkedMap::new();
        with_element(&j("a::A"), Err(model_exception(j("first\nmore"))), Some(&mut walls)).unwrap();
        with_element(&j("a::A"), Err(model_exception(j("second"))), Some(&mut walls)).unwrap();
        with_element(
            &j("a::A"),
            Err(Thrown::model(Phase::Model, j("named"), Some(&j("a::Super")))),
            Some(&mut walls),
        )
        .unwrap();
        with_element(&j("a::B"), Ok(()), Some(&mut walls)).unwrap();
        let got: Vec<(String, String)> = walls.iter().map(|(k, v)| (k.lossy(), v.lossy())).collect();
        assert_eq!(
            got,
            vec![
                ("a::A".to_owned(), "first".to_owned()),
                ("a::Super".to_owned(), "named".to_owned())
            ]
        );
    }

    #[test]
    fn only_a_model_exception_is_caught_in_either_mode() {
        for raw in [
            Thrown::illegal_argument(j(BOUND)),
            Thrown::illegal_state(j("T classified as a type but has none")),
            Thrown::resolution(j("not a ModelException")),
        ] {
            assert_eq!(with_element(&j("a::A"), Err(raw.clone()), None), Err(raw.clone()));
            let mut walls = LinkedMap::new();
            assert_eq!(with_element(&j("a::A"), Err(raw.clone()), Some(&mut walls)), Err(raw));
            assert!(walls.is_empty());
        }
    }

    // ---- requireValidBounds (:440, :454), associationEndSite (:463)

    #[test]
    fn an_invalid_bound_is_refused_in_the_conversions_words() {
        let site = j("property 'x' of m::A");
        for (m, text) in [
            (concrete(2, Some(1)), BOUND),
            (concrete(-1, Some(1)), "lower must be >= 0, got -1"),
            // the lower bound is tested first
            (concrete(-1, Some(-3)), "lower must be >= 0, got -1"),
            (variable(""), "multiplicity parameter name must be non-empty"),
        ] {
            assert_eq!(
                refused(require_valid_bounds(&m, &site)),
                format!("property 'x' of m::A: invalid multiplicity — {text}")
            );
            // the direct test's text is the conversion's, which Java caught (MI:443)
            let converted = TypeClassifier::multiplicity(&QueryArena::new(), &m).expect_err("refused");
            assert_eq!(converted, Thrown::illegal_argument(j(text)));
        }
    }

    #[test]
    fn a_valid_bound_passes() {
        for m in [
            concrete(1, Some(1)),
            concrete(0, Some(1)),
            concrete(0, None),
            concrete(1, None),
            concrete(3, Some(7)),
            variable("m"),
        ] {
            assert_eq!(require_valid_bounds(&m, &j("site")), Ok(()));
            assert_eq!(require_valid_bounds_at(&m, &j("site"), &j("a::A")), Ok(()));
        }
    }

    #[test]
    fn the_attributed_bounds_check_names_its_element() {
        let idx = index(
            "Class w::A { name: String[1]; }\nClass w::B { name: String[1]; }\nAssociation w::AB { a: w::A[2..1]; b: \
             w::B[*]; }\n",
        );
        let a = idx.find_association(&j("w::AB")).expect("the association");
        let site = association_end_site(&a.property1, a);
        assert_eq!(site.lossy(), "property 'a' of association w::AB");
        assert_eq!(
            refusal(require_valid_bounds_at(
                &a.property1.multiplicity,
                &site,
                &a.qualified_name
            )),
            (
                format!("property 'a' of association w::AB: invalid multiplicity — {BOUND}"),
                Some("w::AB".to_owned())
            )
        );
        assert_eq!(
            require_valid_bounds_at(
                &a.property2.multiplicity,
                &association_end_site(&a.property2, a),
                &a.qualified_name
            ),
            Ok(())
        );
    }

    // ---- checkEnum (:469)

    #[test]
    fn an_enum_value_twice_is_refused() {
        let idx = index("Enum m::E { A, A }\nEnum m::F { A, B }\n");
        assert_eq!(
            refused(check_enum(idx.find_enum(&j("m::E")).unwrap())),
            "Found duplicated value 'A' in enumeration 'm::E'"
        );
        assert_eq!(check_enum(idx.find_enum(&j("m::F")).unwrap()), Ok(()));
    }

    // ---- requireAnnotations (:157, :174), profile (:186)

    fn class_annotations(text: &str, class: &str) -> Result<(), Thrown> {
        let idx = index(&format!("{BASE}{text}"));
        let cd = idx.find_class(&j(class)).expect("the class");
        require_annotations(&cd.stereotypes, &cd.tagged_values, &idx)
    }

    #[test]
    fn a_stereotypes_profile_exists_and_declares_it() {
        assert_eq!(
            refused(class_annotations("Class <<a::NoProfile.s>> a::B\n{\n}\n", "a::B")),
            "Can't find the profile 'a::NoProfile'"
        );
        assert_eq!(
            refused(class_annotations("Class <<a::P.nope>> a::B\n{\n}\n", "a::B")),
            "Can't find stereotype 'nope' in profile 'a::P'"
        );
        // a tag, not a stereotype
        assert_eq!(
            refused(class_annotations("Class <<a::P.t>> a::B\n{\n}\n", "a::B")),
            "Can't find stereotype 't' in profile 'a::P'"
        );
        let idx = index(&format!(
            "{BASE}Class a::B\n{{\n  <<a::NoProfile.s>> p: String[1];\n}}\n"
        ));
        let pd = &idx.find_class(&j("a::B")).unwrap().properties[0];
        assert_eq!(
            refused(require_annotations(&pd.stereotypes, &pd.tagged_values, &idx)),
            "Can't find the profile 'a::NoProfile'"
        );
        let idx = index(&format!(
            "{BASE}function <<a::NoProfile.s>> a::f(): String[1]\n{{\n  'x'\n}}\n"
        ));
        let f = &idx.find_function(&j("a::f"))[0];
        assert_eq!(
            refused(require_annotations(f.stereotypes(), f.tagged_values(), &idx)),
            "Can't find the profile 'a::NoProfile'"
        );
        let idx = index(&format!(
            "{BASE}Enum <<a::NoProfile.s>> a::F\n{{\n  X\n}}\nEnum a::G\n{{\n  <<a::P.nope>> X\n}}\n"
        ));
        assert_eq!(
            refused(check_enum_element(idx.find_enum(&j("a::F")).unwrap(), &idx)),
            "Can't find the profile 'a::NoProfile'"
        );
        assert_eq!(
            refused(check_enum_element(idx.find_enum(&j("a::G")).unwrap(), &idx)),
            "Can't find stereotype 'nope' in profile 'a::P'"
        );
    }

    #[test]
    fn a_tags_profile_exists_and_declares_it() {
        assert_eq!(
            refused(class_annotations("Class {a::NoProfile.t = 'v'} a::B\n{\n}\n", "a::B")),
            "Can't find the profile 'a::NoProfile'"
        );
        assert_eq!(
            refused(class_annotations("Class {a::P.nope = 'v'} a::B\n{\n}\n", "a::B")),
            "Can't find tag 'nope' in profile 'a::P'"
        );
        let idx = index(&format!(
            "{BASE}function {{a::P.nope = 'v'}} a::f(): String[1]\n{{\n  'x'\n}}\n"
        ));
        let f = &idx.find_function(&j("a::f"))[0];
        assert_eq!(
            refused(require_annotations(f.stereotypes(), f.tagged_values(), &idx)),
            "Can't find tag 'nope' in profile 'a::P'"
        );
        // the stereotypes are checked before the tags
        assert_eq!(
            refused(class_annotations(
                "Class <<a::P.nope>> {a::NoProfile.t = 'v'} a::B\n{\n}\n",
                "a::B"
            )),
            "Can't find stereotype 'nope' in profile 'a::P'"
        );
        assert_eq!(
            class_annotations("Class <<a::P.s>> {a::P.t = 'v'} a::B\n{\n}\n", "a::B"),
            Ok(())
        );
    }

    #[test]
    fn an_associations_and_a_derived_propertys_annotations_are_checked() {
        let idx = index(&format!(
            "{BASE}Association <<a::NoProfile.s>> a::AB\n{{\n  c: a::C[1];\n  d: a::C[*];\n}}\n\
             Association a::CD\n{{\n  {{a::P.nope = 'v'}} c: a::C[1];\n  d: a::C[*];\n}}\n"
        ));
        let ab = idx.find_association(&j("a::AB")).unwrap();
        assert_eq!(
            refused(require_annotations(&ab.stereotypes, &ab.tagged_values, &idx)),
            "Can't find the profile 'a::NoProfile'"
        );
        let cd = idx.find_association(&j("a::CD")).unwrap();
        assert_eq!(
            refused(require_annotations(
                &cd.property1.stereotypes,
                &cd.property1.tagged_values,
                &idx
            )),
            "Can't find tag 'nope' in profile 'a::P'"
        );
        let idx = index(&format!(
            "{BASE}Class a::B\n{{\n  <<a::NoProfile.s>> d() {{1}}: Integer[1];\n}}\n\
             Class a::D\n{{\n  {{a::P.nope = 'v'}} d() {{1}}: Integer[1];\n}}\n\
             Class a::F\n{{\n  <<a::P.s>> {{a::P.t = 'v'}} d() {{1}}: Integer[1];\n}}\n"
        ));
        let derived = |class: &str| idx.find_class(&j(class)).unwrap().derived_properties[0].clone();
        assert_eq!(
            refused(require_derived_annotations(&derived("a::B"), &idx)),
            "Can't find the profile 'a::NoProfile'"
        );
        assert_eq!(
            refused(require_derived_annotations(&derived("a::D"), &idx)),
            "Can't find tag 'nope' in profile 'a::P'"
        );
        assert_eq!(require_derived_annotations(&derived("a::F"), &idx), Ok(()));
    }

    // ---- the elements that only name others: runtimes (:124), services (:140), data spaces (:116)

    /// A canonical mapping (E's product) at `fqn`, empty, added to `idx`.
    fn with_mapping(mut idx: ModelIndex, fqn: &str) -> ModelIndex {
        let md = MappingDefinition::common(j(fqn), empty(), empty(), empty(), empty());
        idx.add(&[PackageableElement::MappingDefinition(Arc::new(md))])
            .expect("the mapping indexes");
        idx
    }

    fn runtime(idx: &ModelIndex, fqn: &str) -> Result<(), Thrown> {
        check_runtime(idx.find_runtime(&j(fqn)).expect("the runtime"), idx)
    }

    #[test]
    fn a_runtimes_mapping_is_a_mapping() {
        let idx = index("###Runtime\nRuntime a::R\n{\n  mappings:\n  [\n    a::NoMapping\n  ];\n}\n");
        assert_eq!(refused(runtime(&idx, "a::R")), "Can't find mapping 'a::NoMapping'");
    }

    #[test]
    fn the_runtimes_store_is_a_store() {
        let idx = index(
            "###Runtime\nRuntime a::R\n{\n  mappings:\n  [\n  ];\n  connections:\n  [\n    a::NoDb:\n    [\n      c1: \
             a::Cn\n    ]\n  ];\n}\n",
        );
        assert_eq!(refused(runtime(&idx, "a::R")), "The store 'a::NoDb' can't be found.");
    }

    #[test]
    fn a_runtimes_connection_is_a_connection_and_the_model_store_is_a_store() {
        let idx = index(&format!(
            "{DB}###Runtime\nRuntime a::R\n{{\n  mappings:\n  [\n  ];\n  connections:\n  [\n    a::Db:\n    [\n      \
             c1: a::NoCn\n    ]\n  ];\n}}\n"
        ));
        assert_eq!(refused(runtime(&idx, "a::R")), "Can't find connection 'a::NoCn'");
        let idx = index(&format!(
            "{DB}###Runtime\nRuntime a::R\n{{\n  mappings:\n  [\n  ];\n  connections:\n  [\n    ModelStore:\n    [\n  \
             ]\n  ];\n}}\n"
        ));
        assert_eq!(runtime(&idx, "a::R"), Ok(()));
    }

    #[test]
    fn the_elements_that_name_others_pass_when_they_exist() {
        let idx = with_mapping(
            index(&format!(
                "{DB}###Connection\nRelationalDatabaseConnection a::Cn\n{{\n  store: a::Db;\n  type: H2;\n  \
                 specification: LocalH2\n  {{\n  }};\n  auth: DefaultH2;\n}}\n###Runtime\nRuntime a::R\n{{\n  \
                 mappings:\n  [\n    a::M\n  ];\n  connections:\n  [\n    a::Db:\n    [\n      c1: a::Cn\n    ]\n  \
                 ];\n}}\n"
            )),
            "a::M",
        );
        assert_eq!(runtime(&idx, "a::R"), Ok(()));
    }

    #[test]
    fn a_services_mapping_and_runtime_exist() {
        let text = "###Service\nService a::S\n{\n  pattern: '/x';\n  documentation: '';\n  autoActivateUpdates: \
                    true;\n  execution: Single\n  {\n    query: |a::C.all()->project([x|$x.x], ['x']);\n    mapping: \
                    a::NoMapping;\n    runtime: a::NoRuntime;\n  }\n}\n";
        let idx = index(&format!("{BASE}{text}"));
        let sv = idx.find_service(&j("a::S")).expect("the service");
        assert_eq!(refused(check_service(sv, &idx)), "Can't find mapping 'a::NoMapping'");
        // the mapping found, the runtime is next
        let idx = with_mapping(
            index(&format!("{BASE}{}", text.replace("a::NoMapping", "a::M"))),
            "a::M",
        );
        let sv = idx.find_service(&j("a::S")).expect("the service");
        assert_eq!(refused(check_service(sv, &idx)), "Can't find runtime 'a::NoRuntime'");
    }

    #[test]
    fn a_data_spaces_mapping_and_runtime_exist() {
        let text = "###DataSpace\nDataSpace a::Ds\n{\n  executionContexts:\n  [\n    {\n      name: 'c';\n      \
                    mapping: a::NoMapping;\n      defaultRuntime: a::NoRuntime;\n    }\n  ];\n  \
                    defaultExecutionContext: 'c';\n}\n";
        let idx = index(text);
        let ds = idx.find_data_space(&j("a::Ds")).expect("the data space");
        assert_eq!(refused(check_data_space(ds, &idx)), "Can't find mapping 'a::NoMapping'");
        let idx = with_mapping(index(&text.replace("a::NoMapping", "a::M")), "a::M");
        let ds = idx.find_data_space(&j("a::Ds")).expect("the data space");
        assert_eq!(refused(check_data_space(ds, &idx)), "Can't find runtime 'a::NoRuntime'");
    }

    // ---- databases (:533-664)

    fn database(text: &str, fqn: &str) -> Result<(), Thrown> {
        let idx = index(text);
        check_database(idx.find_database(&j(fqn)).expect("the database"), &idx)
    }

    #[test]
    fn a_databases_include_is_a_database() {
        assert_eq!(
            refused(database(
                "###Relational\nDatabase a::Db\n(\n  include a::NoDb\n  Table T (X INTEGER)\n)\n",
                "a::Db"
            )),
            "Can't find database 'a::NoDb'"
        );
    }

    #[test]
    fn a_ghost_join_column_or_table_is_refused() {
        assert_eq!(
            refused(database(
                "###Relational\nDatabase s::DB ( Table T (ID INTEGER)\n  Join bad(T.ID = T.GHOST) )\n",
                "s::DB"
            )),
            "The column 'GHOST' can't be found in the table 'T' (join 'bad' of database 's::DB')"
        );
        assert_eq!(
            refused(database(
                "###Relational\nDatabase s::DB ( Table T (ID INTEGER)\n  Join bad(T.ID = NOPE.ID) )\n",
                "s::DB"
            )),
            "The table 'NOPE' can't be found in the database 's::DB' (join 'bad')"
        );
        assert_eq!(
            refused(database(
                "###Relational\nDatabase s::DB ( Table T (ID INTEGER)\n  Filter f(T.ID = T.GHOST) )\n",
                "s::DB"
            )),
            "The column 'GHOST' can't be found in the table 'T' (filter 'f' of database 's::DB')"
        );
        // a column reached through the include closure is found; a good join passes
        assert_eq!(
            database(
                "###Relational\nDatabase s::Base ( Table U (ID INTEGER) )\nDatabase s::DB ( include s::Base\n  Table \
                 T (ID INTEGER)\n  Join ok(T.ID = U.ID) )\n",
                "s::DB"
            ),
            Ok(())
        );
    }

    #[test]
    fn a_views_columns_are_its_column_mappings() {
        let text = "###Relational\nDatabase s::DB ( Table T (ID INTEGER, NAME VARCHAR(10))\n  View V ( id: T.ID )\n  \
                    Join j(T.ID = V.id) )\n";
        assert_eq!(database(text, "s::DB"), Ok(()));
        // by the mapping's name, as written (a view has no column `ID`, nor `NAME`: its one mapping is `id`)
        for column in ["ID", "NAME"] {
            assert_eq!(
                refused(database(&text.replace("V.id)", &format!("V.{column})")), "s::DB")),
                format!("The column '{column}' can't be found in the view 'V' (join 'j' of database 's::DB')")
            );
        }
    }

    #[test]
    fn a_milestoning_column_counts_as_a_column_and_must_be_a_date() {
        let milestoned = "###Relational\nDatabase s::DB ( Table T ( milestoning( business(BUS_FROM = from_z, BUS_THRU \
                          = thru_z) ) ID INTEGER, from_z DATE, thru_z DATE )\n  Join j(T.ID = T.thru_z) )\n";
        assert_eq!(database(milestoned, "s::DB"), Ok(()));
        assert_eq!(
            refused(database(&milestoned.replace("thru_z DATE", "thru_z OTHER"), "s::DB")),
            "The milestoning column 'thru_z' of table 'T' in database 's::DB' is declared Other: a milestoning date \
             must be a DATE or TIMESTAMP column"
        );
        // DISTINCT has no spelling in the grammar (only the protocol's JSON reads one): the table's column made one
        let idx = index(milestoned);
        let db = idx.find_database(&j("s::DB")).unwrap();
        let td = &db.tables[0];
        let columns = td
            .columns
            .iter()
            .map(|c| {
                if !c.name.eq_str("from_z") {
                    return c.clone();
                }
                Arc::new(lite_model::DatabaseDefinitionColumnDefinition {
                    data_type: RelationalDataType::RelationalDataTypeDistinct(Arc::new(
                        lite_model::RelationalDataTypeDistinct {},
                    )),
                    ..(**c).clone()
                })
            })
            .collect();
        let distinct = DatabaseDefinitionTableDefinition {
            columns: list(columns),
            ..(**td).clone()
        };
        assert_eq!(
            refused(check_milestoning_column_types(db, &distinct)),
            "The milestoning column 'from_z' of table 'T' in database 's::DB' is declared Distinct: a milestoning \
             date must be a DATE or TIMESTAMP column"
        );
        // a column no milestoning names may be either
        assert_eq!(database(&milestoned.replace("ID INTEGER", "ID OTHER"), "s::DB"), Ok(()));
    }

    // ---- mappings (:391): the refusals before the function compiler is asked

    #[test]
    fn a_mapping_binds_known_classes_and_associations_and_names_stores() {
        let mut idx = index(&format!("{BASE}{DB}"));
        let bad_class = MappingDefinition::common(
            j("a::M1"),
            empty(),
            list(vec![
                lite_model::MappingDefinitionClassBinding::MappingDefinitionClassBindingPure(Arc::new(
                    MappingDefinitionClassBindingPure {
                        class_fqn: j("a::NoClass"),
                        set_id: None,
                        extends_set_id: None,
                        root: true,
                        function_fqn: j("a::M1$class$a::NoClass"),
                        primary_key_columns: empty(),
                    },
                )),
            ]),
            empty(),
            empty(),
        );
        let bad_association = MappingDefinition::common(
            j("a::M2"),
            empty(),
            empty(),
            list(vec![Arc::new(MappingDefinitionAssociationBinding {
                association_fqn: j("a::NoAssoc"),
                predicate_function_fqn: j("a::M2$assoc$a::NoAssoc"),
            })]),
            empty(),
        );
        let bad_store = MappingDefinition::common(
            j("a::M3"),
            list(vec![Arc::new(lite_model::MappingInclude {
                mapping_path: j("a::M1"),
                substitutions: list(vec![Arc::new(lite_model::MappingIncludeStoreSubstitution {
                    original_store: j("a::NoDb"),
                    replacement_store: j("a::Db"),
                })]),
            })]),
            empty(),
            empty(),
            empty(),
        );
        idx.add(&[
            PackageableElement::MappingDefinition(Arc::new(bad_class)),
            PackageableElement::MappingDefinition(Arc::new(bad_association)),
            PackageableElement::MappingDefinition(Arc::new(bad_store)),
        ])
        .unwrap();
        let classifier = TypeClassifier::new(&idx);
        let functions = FunctionCompiler::new(&idx, classifier);
        let mapping = |fqn: &str| check_mapping(idx.find_mapping(&j(fqn)).unwrap(), &idx, &classifier, &functions);
        assert_eq!(
            refused(mapping("a::M1")),
            "mapping 'a::M1' binds unknown class 'a::NoClass'"
        );
        assert_eq!(
            refused(mapping("a::M2")),
            "mapping 'a::M2' binds unknown association 'a::NoAssoc'"
        );
        assert_eq!(refused(mapping("a::M3")), "The store 'a::NoDb' can't be found.");
    }

    // ---- checkDuplicateSignatures (:343)

    #[test]
    fn a_signature_defined_twice_is_walled_under_its_id() {
        let idx = index(
            "function w::f(x: String[1]): String[1]\n{\n  $x\n}\nfunction w::f(y: String[1]): String[1]\n{\n  \
             $y\n}\nfunction w::f(x: Integer[1]): String[1]\n{\n  'n'\n}\n",
        );
        let text = "function 'w::f' is defined more than once with the same signature — calls would be ambiguous";
        assert_eq!(
            refusal(check_duplicate_signatures(&idx, None, None)),
            (text.to_owned(), Some("w::f_String_1__String_1_".to_owned()))
        );
        let mut walls = LinkedMap::new();
        check_duplicate_signatures(&idx, Some(&mut walls), None).unwrap();
        let got: Vec<(String, String)> = walls.iter().map(|(k, v)| (k.lossy(), v.lossy())).collect();
        assert_eq!(got, vec![("w::f_String_1__String_1_".to_owned(), text.to_owned())]);
    }

    // ---- the whole pass, where it stops before any fill it does not have

    #[test]
    fn a_duplicated_element_is_the_first_refusal() {
        // LeniencyD6Test: the shared element namespace, across kinds too (lite-names' refuseDuplicates)
        for text in [
            "Class m::A { x: String[1]; }\nClass m::A { y: Integer[1]; }\n",
            "Class m::A { x: String[1]; }\nEnum m::A { V }\n",
        ] {
            assert_eq!(
                refusal(strict(&index(text))),
                ("Duplicated element 'm::A'".to_owned(), Some("m::A".to_owned()))
            );
        }
    }

    #[test]
    fn a_classs_annotations_are_its_first_check() {
        let profile = "Profile a::P\n{\n  stereotypes: [s];\n  tags: [t];\n}\n";
        assert_eq!(
            refusal(strict(&index(&format!(
                "{profile}Class <<a::NoProfile.s>> a::B\n{{\n}}\n"
            )))),
            (
                "Can't find the profile 'a::NoProfile'".to_owned(),
                Some("a::B".to_owned())
            )
        );
        // a property's annotation is checked before the class's supertypes and properties are classified
        assert_eq!(
            refusal(strict(&index(&format!(
                "{profile}Class a::B extends a::Missing\n{{\n  {{a::P.nope = 'v'}} p: a::Missing[1];\n}}\n"
            )))),
            (
                "Can't find tag 'nope' in profile 'a::P'".to_owned(),
                Some("a::B".to_owned())
            )
        );
    }

    // ---- the whole pass, past the fills (ignored until they land)

    #[test]
    #[ignore = "needs helper A's fill (classify, find_type, head_fqn) and helper D's (graph_edges::Index::of, read_one)"]
    fn a_tolerant_build_walls_every_class_of_a_cycle() {
        // ElementRefusalsTest.findPropertyOnAWalledCyclicClassIsRefusedByName: the walls
        let idx = index(
            "Class w::A extends w::B { a: String[1]; }\nClass w::B extends w::A { b: String[1]; }\nClass w::C extends \
             w::A { c: String[1]; }\n",
        );
        let walls = tolerant(&idx);
        let expected = [
            ("w::A", "Inheritance cycle: w::A -> w::B -> w::A"),
            ("w::B", "Inheritance cycle: w::B -> w::A -> w::B"),
            ("w::C", "Inheritance cycle: w::C -> w::A -> w::B -> w::A"),
        ];
        assert_eq!(walls, expected.map(|(k, v)| (k.to_owned(), v.to_owned())).to_vec());
        // LeniencyD6Test: strict, the first
        assert_eq!(
            refusal(strict(&index(
                "Class m::A extends m::B {}\nClass m::B extends m::A {}\n"
            ))),
            (
                "Inheritance cycle: m::A -> m::B -> m::A".to_owned(),
                Some("m::A".to_owned())
            )
        );
    }

    #[test]
    #[ignore = "needs helper A's fill (classify, find_type, head_fqn) and helper D's (graph_edges::Index::of, read_one)"]
    fn a_diamond_is_no_cycle() {
        let idx = index(
            "Class w::Top { t: String[1]; }\nClass w::L extends w::Top { }\nClass w::R extends w::Top { }\nClass w::D \
             extends w::L, w::R { }\n",
        );
        assert_eq!(strict(&idx), Ok(()));
        assert_eq!(tolerant(&idx), vec![]);
    }

    #[test]
    #[ignore = "needs helper A's fill (classify, require_arguments) and helper D's (graph_edges::Index::of, read_one)"]
    fn inverse_bounds_are_refused_at_every_signature_site() {
        // ElementRefusalsTest.inverseBoundsAreRefusedAtEverySignatureSite
        let parameter = index("function w::f(x: String[2..1]): String[1]\n{\n  'a'\n}\n");
        assert_eq!(
            refusal(strict(&parameter)),
            (
                format!("parameter 'x' of function w::f: invalid multiplicity — {BOUND}"),
                Some("w::f_String_$2_1$__String_1_".to_owned())
            )
        );
        let ret = index("function w::g(x: String[1]): String[2..1]\n{\n  $x\n}\n");
        assert_eq!(
            refusal(strict(&ret)),
            (
                format!("return of function w::g: invalid multiplicity — {BOUND}"),
                Some("w::g_String_1__String_$2_1$_".to_owned())
            )
        );
        let end = index(
            "Class w::A { name: String[1]; }\nClass w::B { name: String[1]; }\nAssociation w::AB { a: w::A[2..1]; b: \
             w::B[*]; }\n",
        );
        assert_eq!(
            refusal(strict(&end)),
            (
                format!("property 'a' of association w::AB: invalid multiplicity — {BOUND}"),
                Some("w::AB".to_owned())
            )
        );
        assert_eq!(
            refusal(strict(&index("Class w::C { p: String[2..1]; }\n"))),
            (
                format!("property 'p' of w::C: invalid multiplicity — {BOUND}"),
                Some("w::C".to_owned())
            )
        );
        assert_eq!(
            tolerant(&parameter),
            vec![(
                "w::f_String_$2_1$__String_1_".to_owned(),
                format!("parameter 'x' of function w::f: invalid multiplicity — {BOUND}")
            )]
        );
        assert_eq!(
            tolerant(&end),
            vec![(
                "w::AB".to_owned(),
                format!("property 'a' of association w::AB: invalid multiplicity — {BOUND}")
            )]
        );
    }

    #[test]
    #[ignore = "needs helper A's fill (classify, require_arguments, find_type, head_fqn) and helper D's \
                (graph_edges::Index::of, read_one)"]
    fn a_classs_own_refusals() {
        // LeniencyD6Test
        assert_eq!(
            refusal(strict(&index("Class m::A { x: String[1]; x: Integer[1]; }\n"))),
            (
                "Found duplicated property 'x' in class 'm::A'".to_owned(),
                Some("m::A".to_owned())
            )
        );
        // ReferenceIntegrityTest.anOptionalPropertyHasNoDefault
        assert_eq!(
            refusal(strict(&index(&format!(
                "{BASE}Class a::B\n{{\n  x: Integer[0..1] = 5;\n}}\n"
            )))),
            (
                "Default values are supported only for mandatory fields, and property 'x' is optional.".to_owned(),
                Some("a::B".to_owned())
            )
        );
        // ReferenceIntegrityTest.aSupertypeThatIsNotAClassIsRefused
        for (text, message) in [
            (
                "Class a::B extends a::E\n{\n}\n",
                "Invalid supertype: 'B' cannot extend 'a::E' as it is not a class.",
            ),
            (
                "Class a::B extends String\n{\n}\n",
                "Invalid supertype: 'B' cannot extend 'meta::pure::metamodel::type::String' as it is not a class.",
            ),
            (
                "Class a::B<T> extends T\n{\n}\n",
                "Invalid supertype: 'B' cannot extend 'T' as it is not a class.",
            ),
        ] {
            assert_eq!(
                refusal(strict(&index(&format!("{BASE}{text}")))),
                (message.to_owned(), Some("a::B".to_owned()))
            );
        }
        // ReferenceIntegrityTest.aSupertypeThatDoesNotExistIsRefused (classify's text)
        let (message, element) = refusal(strict(&index(&format!(
            "{BASE}Class a::B extends a::Missing\n{{\n}}\n"
        ))));
        assert!(message.contains("Unknown type: 'a::Missing'"), "{message}");
        assert_eq!(element, Some("a::B".to_owned()));
        // ReferenceIntegrityTest.aClassMayExtendAnotherClassAndAny
        assert_eq!(
            strict(&index(&format!(
                "{BASE}Class a::B extends a::C\n{{\n}}\nClass a::D extends Any\n{{\n}}\n"
            ))),
            Ok(())
        );
    }

    #[test]
    #[ignore = "needs helper A's fill (classify, require_arguments, find_type, head_fqn) and helper D's \
                (graph_edges::Index::of, read_one)"]
    fn a_declared_types_arguments_are_counted() {
        // ReferenceIntegrityTest.aSupertypesArgumentsAreItsParametersWhereWritten and
        // aGenericClassNamedWithoutItsArgumentsIsRefused: the classifier's texts, reached through the pass
        let generic = "Class a::G<T>\n{\n  v: T[1];\n}\n";
        for (text, fragment) in [
            (
                format!("{generic}Class a::B extends a::G<String, Integer>\n{{\n}}\n"),
                "Wrong type arguments count (2) for type: G<T>",
            ),
            (
                format!("{generic}Class a::B extends a::G<a::Missing>\n{{\n}}\n"),
                "Unknown type: 'a::Missing'",
            ),
            (
                "Class a::B extends a::C<String>\n{\n}\n".to_owned(),
                "Wrong type arguments count (1) for type: C",
            ),
            (
                format!("{generic}Class a::B\n{{\n  g: a::G[1];\n}}\n"),
                "Wrong type arguments count (0) for type: G<T>",
            ),
            (
                "Class a::B\n{\n  p: Pair<String>[1];\n}\n".to_owned(),
                "Wrong type arguments count (1) for type: Pair<U,V>",
            ),
            (
                "Class a::B\n{\n  g: a::Missing<String>[1];\n}\n".to_owned(),
                "Unknown type: 'a::Missing'",
            ),
        ] {
            let (message, element) = refusal(strict(&index(&format!("{BASE}{text}"))));
            assert!(message.contains(fragment), "{text}: {message}");
            assert_eq!(element, Some("a::B".to_owned()), "{text}");
        }
        for text in [
            format!("{generic}Class a::B extends a::G<String>\n{{\n}}\n"),
            "Class a::B extends Pair<String, Integer>\n{\n}\n".to_owned(),
            format!("{generic}Class a::B\n{{\n  g: a::G<String>[1];\n}}\n"),
            // the engine's backward compatibility: a written `Result` is Result<Any|1..*>
            "###Pure\nimport meta::pure::mapping::*;\nfunction a::f(r: Result[1]): Boolean[1]\n{\n  true\n}\n"
                .to_owned(),
        ] {
            assert_eq!(strict(&index(&format!("{BASE}{text}"))), Ok(()), "{text}");
        }
    }

    #[test]
    #[ignore = "needs helper A's fill (classify, require_arguments, find_type, head_fqn) and helper D's \
                (graph_edges::Index::of, read_one)"]
    fn the_checks_run_in_their_order() {
        // a bad class, a bad enum, a bad database, a bad runtime: strict gives the class's refusal, tolerant walls
        // each in the pass's order (classes, ..., enums, databases, ..., runtimes), whatever the text's order
        let idx = index(
            "###Runtime\nRuntime a::R\n{\n  mappings:\n  [\n    a::NoMapping\n  ];\n}\n###Relational\nDatabase a::Db \
             (\n  include a::NoDb\n  Table T (X INTEGER)\n)\n###Pure\nEnum a::E { A, A }\nClass a::C { x: String[1]; \
             x: Integer[1]; }\n",
        );
        assert_eq!(
            refusal(strict(&idx)),
            (
                "Found duplicated property 'x' in class 'a::C'".to_owned(),
                Some("a::C".to_owned())
            )
        );
        let expected = [
            ("a::C", "Found duplicated property 'x' in class 'a::C'"),
            ("a::E", "Found duplicated value 'A' in enumeration 'a::E'"),
            ("a::Db", "Can't find database 'a::NoDb'"),
            ("a::R", "Can't find mapping 'a::NoMapping'"),
        ];
        assert_eq!(
            tolerant(&idx),
            expected.map(|(k, v)| (k.to_owned(), v.to_owned())).to_vec()
        );
    }

    #[test]
    #[ignore = "needs helper A's fill (classify, require_arguments) and helper D's (graph_edges::Index::of, read_one)"]
    fn the_reference_integrity_cases_the_pass_refuses() {
        // ReferenceIntegrityTest's F-time cases, each its whole pass's first refusal
        let cases = [
            (
                "###Runtime\nRuntime a::R\n{\n  mappings:\n  [\n    a::NoMapping\n  ];\n}\n",
                "Can't find mapping 'a::NoMapping'",
            ),
            (
                "###Relational\nDatabase a::Db\n(\n  include a::NoDb\n  Table T (X INTEGER)\n)\n",
                "Can't find database 'a::NoDb'",
            ),
            (
                "Class <<a::P.nope>> a::B\n{\n}\n",
                "Can't find stereotype 'nope' in profile 'a::P'",
            ),
            (
                "Class a::B\n{\n  {a::P.nope = 'v'} p: String[1];\n}\n",
                "Can't find tag 'nope' in profile 'a::P'",
            ),
            (
                "function {a::P.nope = 'v'} a::f(): String[1]\n{\n  'x'\n}\n",
                "Can't find tag 'nope' in profile 'a::P'",
            ),
            (
                "Enum a::F\n{\n  <<a::P.nope>> X\n}\n",
                "Can't find stereotype 'nope' in profile 'a::P'",
            ),
            (
                "Association <<a::NoProfile.s>> a::AB\n{\n  c: a::C[1];\n  d: a::C[*];\n}\n",
                "Can't find the profile 'a::NoProfile'",
            ),
        ];
        for (text, message) in cases {
            let (got, _) = refusal(strict(&index(&format!("{BASE}{text}"))));
            assert_eq!(got, message, "{text}");
        }
        assert_eq!(strict(&index(BASE)), Ok(()));
    }
}
