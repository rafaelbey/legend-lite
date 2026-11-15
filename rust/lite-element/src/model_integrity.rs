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

use lite_base::QueryArena;
use lite_front::records as p;
use lite_model::linked::LinkedMap;
use lite_model::{
    AssociationDefinition, AssociationDefinitionAssociationEndDefinition, ClassDefinition, DatabaseDefinition,
    DatabaseDefinitionTableDefinition, EnumDefinition, Function, JStr, MappingDefinition, ProfileDefinition,
    RelationalOperation, RelationalOperationColumnRef, RuntimeDefinition, ServiceDefinition, StereotypeApplication,
    TaggedValue, cat,
};
use lite_names::{ModelIndex, Thrown};

use crate::function_compiler::FunctionCompiler;
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
    unimplemented!("lite-element skeleton: ModelIntegrity.check")
}

/// `requireValidBounds(m, site)` (:440, RE-EXPRESSED): Java converts `m` (`TypeClassifier.multiplicity`) and catches
/// its `IllegalArgumentException` as "<site>: invalid multiplicity — <its message>" (a `ModelException`, MODEL);
/// here each refusal the conversion can raise is tested directly, with the same text: a concrete multiplicity's
/// "upper (U) must be >= lower (L)" and "lower must be >= 0, got L", and a variable's "multiplicity parameter name
/// must be non-empty" (the last two believed unreachable from the parser, which refuses a negative lower bound and an
/// empty name first: ported as Java has them all the same).
pub fn require_valid_bounds(m: &p::Multiplicity, site: &JStr) -> Result<(), Thrown> {
    unimplemented!("lite-element skeleton: ModelIntegrity.requireValidBounds")
}

/// `requireValidBounds(m, site, element)` (:454; 18feff3fe's addition, PMC:464's caller): [`require_valid_bounds`]
/// outside the pass, where no `with_element` attributes the refusal: attributed to `element`, as the pass's wall
/// would be.
pub fn require_valid_bounds_at(m: &p::Multiplicity, site: &JStr, element: &JStr) -> Result<(), Thrown> {
    unimplemented!("lite-element skeleton: ModelIntegrity.requireValidBounds (attributed)")
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
fn with_element(
    element_fqn: &JStr,
    work: Result<(), Thrown>,
    wall_sink: Option<&mut LinkedMap<JStr, JStr>>,
) -> Result<(), Thrown> {
    unimplemented!("lite-element skeleton: ModelIntegrity.withElement")
}

/// `checkRuntime(rt, model)` (:124): its mappings, its connection bindings' stores and connections exist.
fn check_runtime(rt: &RuntimeDefinition, model: &ModelIndex) -> Result<(), Thrown> {
    unimplemented!("lite-element skeleton: ModelIntegrity.checkRuntime")
}

/// `checkService(sv, model)` (:140): its mapping and runtime, and each keyed execution's, exist.
fn check_service(sv: &ServiceDefinition, model: &ModelIndex) -> Result<(), Thrown> {
    unimplemented!("lite-element skeleton: ModelIntegrity.checkService")
}

/// `requireAnnotations(stereotypes, taggedValues, model)` (:157): each names a profile the model has and one that
/// profile declares (legend-engine's `getProfile`/`getStereotype`/`getTag` words).
fn require_annotations(
    stereotypes: &[std::sync::Arc<StereotypeApplication>],
    tagged_values: &[std::sync::Arc<TaggedValue>],
    model: &ModelIndex,
) -> Result<(), Thrown> {
    unimplemented!("lite-element skeleton: ModelIntegrity.requireAnnotations")
}

/// `requireAnnotations(dp, model)` (:174): a derived property's annotations (protocol records), checked as any.
fn require_derived_annotations(dp: &p::DerivedPropertyDefinition, model: &ModelIndex) -> Result<(), Thrown> {
    unimplemented!("lite-element skeleton: ModelIntegrity.requireAnnotations (derived property)")
}

/// `profile(fqn, model)` (:186): the profile, or "Can't find the profile '<fqn>'".
fn profile<'i>(fqn: &JStr, model: &'i ModelIndex) -> Result<&'i std::sync::Arc<ProfileDefinition>, Thrown> {
    unimplemented!("lite-element skeleton: ModelIntegrity.profile")
}

/// `requireMapping(mapping, model)` (:192): a named mapping exists (none: a reference not written).
fn require_mapping(mapping: Option<&JStr>, model: &ModelIndex) -> Result<(), Thrown> {
    unimplemented!("lite-element skeleton: ModelIntegrity.requireMapping")
}

/// `requireRuntime(runtime, model)` (:200): a named runtime exists (none: not written, or inline).
fn require_runtime(runtime: Option<&JStr>, model: &ModelIndex) -> Result<(), Thrown> {
    unimplemented!("lite-element skeleton: ModelIntegrity.requireRuntime")
}

/// `requireStore(store, model)` (:208): the model store, a database or another store element exists.
fn require_store(store: Option<&JStr>, model: &ModelIndex) -> Result<(), Thrown> {
    unimplemented!("lite-element skeleton: ModelIntegrity.requireStore")
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
    unimplemented!("lite-element skeleton: ModelIntegrity.checkClass")
}

/// `checkDuplicateSignatures(model, wallSink, fresh, priorKeys)` (:343): a fresh function whose id a prior function
/// or an earlier fresh one has, walled under its id ("... defined more than once with the same signature ...").
fn check_duplicate_signatures(
    model: &ModelIndex,
    wall_sink: Option<&mut LinkedMap<JStr, JStr>>,
    prior: Option<&CheckedLayer>,
) -> Result<(), Thrown> {
    unimplemented!("lite-element skeleton: ModelIntegrity.checkDuplicateSignatures")
}

/// `checkFunction(f, classifier, model)` (:362): annotations; each parameter and the return classified, their
/// arguments counted unless the function is synthesized, their bounds valid. Types in `scratch`.
fn check_function(
    f: &Function,
    classifier: &TypeClassifier<'_>,
    model: &ModelIndex,
    scratch: &QueryArena,
) -> Result<(), Thrown> {
    unimplemented!("lite-element skeleton: ModelIntegrity.checkFunction")
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
    unimplemented!("lite-element skeleton: ModelIntegrity.checkMapping")
}

/// `checkEnum(ed)` (:469): no value twice ("Found duplicated value 'X' in enumeration 'E'").
fn check_enum(ed: &EnumDefinition) -> Result<(), Thrown> {
    unimplemented!("lite-element skeleton: ModelIntegrity.checkEnum")
}

/// `checkInheritanceAcyclic(model, classifier, wallSink, fresh)` (:485): every fresh class's supers walked
/// (`walk_supers`), a cycle "Inheritance cycle: a -> b -> a", one wall per class.
fn check_inheritance_acyclic(
    model: &ModelIndex,
    classifier: &TypeClassifier<'_>,
    wall_sink: Option<&mut LinkedMap<JStr, JStr>>,
    prior: Option<&CheckedLayer>,
) -> Result<(), Thrown> {
    unimplemented!("lite-element skeleton: ModelIntegrity.checkInheritanceAcyclic")
}

/// `walkSupers(cd, classifier, path, acyclic)` (:497): depth first along `path` (insertion-ordered); a malformed head
/// is skipped (RE-EXPRESSED MI:513, the census's :486), an unknown super ends the walk there.
fn walk_supers(
    cd: &ClassDefinition,
    classifier: &TypeClassifier<'_>,
    path: &mut LinkedMap<JStr, ()>,
    acyclic: &mut std::collections::HashSet<JStr>,
) -> Result<(), Thrown> {
    unimplemented!("lite-element skeleton: ModelIntegrity.walkSupers")
}

/// `checkDatabase(db, model)` (:533): includes exist; joins', filters' and multigrain filters' column references
/// resolve; milestoning columns are dates.
fn check_database(db: &DatabaseDefinition, model: &ModelIndex) -> Result<(), Thrown> {
    unimplemented!("lite-element skeleton: ModelIntegrity.checkDatabase")
}

/// `checkMilestoningColumnTypes(db, td)` (:561): a milestoning column declared OTHER or DISTINCT refused, by the
/// literal type names (RE-EXPRESSED MI:580's, the census's :553, `getSimpleName`).
fn check_milestoning_column_types(
    db: &DatabaseDefinition,
    td: &DatabaseDefinitionTableDefinition,
) -> Result<(), Thrown> {
    unimplemented!("lite-element skeleton: ModelIntegrity.checkMilestoningColumnTypes")
}

/// `checkStoreRefs(db, model, op, site)` (:587): each column reference of the operation not qualified by a foreign
/// database, through its children.
fn check_store_refs(
    db: &DatabaseDefinition,
    model: &ModelIndex,
    op: &RelationalOperation,
    site: &JStr,
) -> Result<(), Thrown> {
    unimplemented!("lite-element skeleton: ModelIntegrity.checkStoreRefs")
}

/// `checkColumnRef(db, model, cr, site)` (:603): the table (include closure) has the column, else the view does,
/// else the table is unknown (each in its words).
fn check_column_ref(
    db: &DatabaseDefinition,
    model: &ModelIndex,
    cr: &RelationalOperationColumnRef,
    site: &JStr,
) -> Result<(), Thrown> {
    unimplemented!("lite-element skeleton: ModelIntegrity.checkColumnRef")
}

/// `hasColumn(td, name)` (:642): a declared column, or a milestoning-declared temporal column, by bare name.
fn has_column(td: &DatabaseDefinitionTableDefinition, name: &JStr) -> bool {
    unimplemented!("lite-element skeleton: ModelIntegrity.hasColumn")
}
