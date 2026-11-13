// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! Name resolution (2026-11-12, the names slice: Java's `NameResolver`, pipeline step D): every simple name of a
//! model or a query rewritten to its FQN through the import tiers, an `ImportScope` and a universe of known FQNs.
//!
//! **The rule** (Java's, line for line): a qualified name, a type parameter in scope, an M3 primitive's reserved bare
//! name are themselves; else the section's wildcards over the universe (every match: several is an ambiguity in a type
//! position, overload candidates in a call); else the element's own package; else the core import group, first match;
//! else the name passes through, unresolved, for the downstream phase to refuse. A call position also unions the
//! platform's bare-name tiers ([`bare_names`](crate::bare_names)) and records the call's referents on the node.
//!
//! **Unchanged is the input.** Every walker answers `None` when nothing in its subtree changed (Java hands back the
//! very input, `==`, and decides every rebuild on that), and a rebuilt record is built as Java builds it -- including
//! the convenience constructors that drop a component (a rebuilt name reference, parameter, column, lambda, variable,
//! collection, property, enum value or element pointer loses its source position; a rebuilt `ColSpec` its type,
//! annotations and position; a rebuilt `NewInstance` its multiplicity arguments; a rebuilt constraint its external id,
//! owner and position; a rebuilt function-reference realization its source; a rebuilt embedded property mapping its
//! primary key; a rebuilt named relation function mapping its primary key; a rebuilt primitive extension its type
//! variables; a resolved model its unclaimed sections and element spans). Each is listed in docs/rust-port/PHASE3.md,
//! the names slice, as a Java behaviour ported as it is.
//!
//! **Identity**: a schema view is the same record in its schema's list and the database's flat list, and resolved once
//! for both (PHASE3.md step 0's census, `model.NameResolver.resolvedViews`, class (c)): an [`IdMap`](lite_base::IdMap).

mod elements;
mod values;

use std::collections::HashSet;
use std::sync::{Arc, LazyLock, Mutex};

use lite_builtin::pure::catalog;
use lite_error::Phase;
use lite_model::linked::LinkedMap;
use lite_model::protocol::{LambdaFunction, ValueSpecification};
use lite_model::{ImportScope, JStr, PackageableElement, ParsedModel, cat};
use lite_types::{PRECISE_PRIMITIVES, Primitive};

use crate::bare_names;
use crate::core_imports;
use crate::error::Thrown;
use crate::model_index::ModelIndex;

pub use values::resolve_vs as resolve_value;

/// A walker's answer: `None` when nothing changed (Java's `==` input), else the rebuilt value.
pub(crate) type Rs<T> = Result<Option<T>, Thrown>;

/// The universe of known FQNs a scope resolves against (Java's `Set<String> knownFqns`, asked only `contains`).
pub trait Universe: Sync {
    fn contains(&self, name: &JStr) -> bool;
}

impl Universe for HashSet<JStr> {
    fn contains(&self, name: &JStr) -> bool {
        HashSet::contains(self, name)
    }
}

/// A model's universe: its own element names and function ids, the names it is resolved alongside (the boot layer's),
/// and the platform's.
struct ModelUniverse<'a> {
    own: HashSet<JStr>,
    also: Option<&'a HashSet<JStr>>,
    platform: bool,
}

impl Universe for ModelUniverse<'_> {
    fn contains(&self, name: &JStr) -> bool {
        self.own.contains(name)
            || self.also.is_some_and(|a| a.contains(name))
            || (self.platform && PLATFORM.fqns.contains(name))
    }
}

/// A compiled model's resolution universe, over its index (Java's `PureModelContext.resolutionUniverse`, its
/// membership: the platform's names, the declared primitive extensions, every registered element, every registered
/// function's signature id). The method is F's; its answer is the index's, so the names layer serves it for a query
/// resolved against a model ([`resolve_query_in`]). Java's view also iterates (`elementFqns` and the platform's, a
/// hash order); the resolver never iterates it, and neither does this.
pub struct IndexUniverse<'m> {
    pub index: &'m ModelIndex,
}

impl Universe for IndexUniverse<'_> {
    fn contains(&self, name: &JStr) -> bool {
        PLATFORM.fqns.contains(name)
            || self.index.is_primitive_extension(name)
            || self.index.has_element(name)
            || self.index.has_function_id(name)
    }
}

/// The threaded context of resolution (Java's `NameResolver.Scope`): the import scope, the universe, the type
/// parameters in scope, the element's own package, whether the prelude spells the M3 primitives, and the profiles a
/// stereotype's or tag's name may mean (an element scope's; none in a query's).
#[derive(Clone)]
pub struct Scope<'u> {
    pub imports: Arc<ImportScope>,
    pub known: &'u dyn Universe,
    pub type_params: Arc<HashSet<JStr>>,
    pub own_package: Option<JStr>,
    pub prelude: bool,
    pub profile_fqns: &'u HashSet<JStr>,
}

static NO_NAMES: LazyLock<HashSet<JStr>> = LazyLock::new(HashSet::new);
static NO_TYPE_PARAMS: LazyLock<Arc<HashSet<JStr>>> = LazyLock::new(|| Arc::new(HashSet::new()));

impl<'u> Scope<'u> {
    /// `Scope.of(imports, known)`: no type parameters, no own package, no prelude.
    pub fn of(imports: Arc<ImportScope>, known: &'u dyn Universe) -> Scope<'u> {
        Scope {
            imports,
            known,
            type_params: NO_TYPE_PARAMS.clone(),
            own_package: None,
            prelude: false,
            profile_fqns: &NO_NAMES,
        }
    }

    /// `Scope.preludeOf(imports, known)`: the query entries' scope.
    pub fn prelude_of(imports: Arc<ImportScope>, known: &'u dyn Universe) -> Scope<'u> {
        Scope {
            prelude: true,
            ..Scope::of(imports, known)
        }
    }

    /// `withTypeParams(params)`: a child scope with `params` in scope (this scope itself when there are none).
    pub fn with_type_params(&self, params: &[JStr]) -> Scope<'u> {
        if params.is_empty() {
            return self.clone();
        }
        let mut merged: HashSet<JStr> = (*self.type_params).clone();
        merged.extend(params.iter().cloned());
        Scope {
            type_params: Arc::new(merged),
            ..self.clone()
        }
    }
}

/// The platform's name universe (Java's `NameResolver.PLATFORM_FQNS`): the native classes and enums, the prelude's
/// classes and enums, the precise primitives, every platform function's signature id and every user-resolvable
/// platform function's FQN. A constant of the process.
struct Platform {
    fqns: HashSet<JStr>,
}

static PLATFORM: LazyLock<Platform> = LazyLock::new(|| {
    let mut fqns: HashSet<JStr> = HashSet::new();
    let c = catalog();
    for cls in c.native_classes() {
        fqns.insert(cls.qualified_name.clone());
    }
    for e in c.native_enums() {
        fqns.insert(e.qualified_name.clone());
    }
    let prelude = lite_builtin::prelude::prelude();
    fqns.extend(prelude.class_fqns().iter().cloned());
    fqns.extend(prelude.enum_fqns().iter().cloned());
    for (fqn, _) in Primitive::by_fqn() {
        if fqn.starts_with(PRECISE_PRIMITIVES) {
            fqns.insert(JStr::from(*fqn));
        }
    }
    // a function's element name upstream IS its signature id: every declaration's id joins the universe
    for n in c.all() {
        fqns.insert(lite_model::function_id::mangle(
            &lite_model::Function::NativeFunctionDefinition(n.clone()),
        ));
    }
    for el in prelude.elements() {
        if let Some(f) = el.as_function() {
            fqns.insert(lite_model::function_id::mangle(&f));
        }
    }
    fqns.extend(c.user_resolvable_function_fqns().iter().cloned());
    for el in prelude.elements() {
        if let Some(f) = el.as_function() {
            fqns.insert(f.qualified_name().clone());
        }
    }
    Platform { fqns }
});

/// `platformFqns()`: the platform's name universe (membership).
pub fn platform_fqns() -> &'static HashSet<JStr> {
    &PLATFORM.fqns
}

/// The sectionless query's scope (Java's `QUERY_SCOPE`): no imports, the platform's universe, the prelude on.
static QUERY_SCOPE: LazyLock<Scope<'static>> =
    LazyLock::new(|| Scope::prelude_of(ImportScope::empty(), &PLATFORM.fqns));

/// The M3 primitive names, reserved bare names (`PRIMITIVE_TYPE_NAMES`).
const PRIMITIVE_TYPE_NAMES: &[&str] = &[
    "Number",
    "Integer",
    "Float",
    "Decimal",
    "String",
    "Boolean",
    "Byte",
    "Date",
    "StrictDate",
    "DateTime",
    "LatestDate",
    "StrictTime",
];

fn qualified(name: &JStr) -> bool {
    lite_model::text::contains(&name.0, "::")
}

fn under(pkg: &JStr, name: &JStr) -> JStr {
    cat!(pkg, "::", name)
}

fn under_str(pkg: &str, name: &JStr) -> JStr {
    cat!(pkg, "::", name)
}

// =====================================================================
// Public entry points
// =====================================================================

/// `resolve(parsed)`: the frontend's entry -- the user's imports, the platform prelude a fallback tier consulted
/// inside the tiers (never merged into an element's imports); strict: the first element whose resolution fails is a
/// `ModelException` (RESOLVE) naming its key.
pub fn resolve(parsed: &ParsedModel) -> Result<ParsedModel, Thrown> {
    resolve_model(parsed, None, None, true)
}

/// `resolve(parsed, walls)`: tolerant -- an element whose resolution fails is recorded in `walls` (its key, the first
/// line of the message) and kept as parsed (poison, not drop).
pub fn resolve_tolerant(parsed: &ParsedModel, walls: &mut LinkedMap<JStr, JStr>) -> Result<ParsedModel, Thrown> {
    resolve_model(parsed, None, Some(walls), true)
}

/// `resolveAlongside(parsed, alsoKnown, walls)`: [`resolve`] with `also_known` in the universe (the boot layer's
/// element names, which a graph's elements name by import as they name each other's).
pub fn resolve_alongside(
    parsed: &ParsedModel,
    also_known: &HashSet<JStr>,
    walls: Option<&mut LinkedMap<JStr, JStr>>,
) -> Result<ParsedModel, Thrown> {
    resolve_model(parsed, Some(also_known), walls, true)
}

/// `resolve(model, knownFqns[, walls])`: the lower-level entry -- the caller's universe exactly (no platform names
/// added) and no prelude.
pub fn resolve_with_universe(
    model: &ParsedModel,
    known: &dyn Universe,
    walls: Option<&mut LinkedMap<JStr, JStr>>,
) -> Result<ParsedModel, Thrown> {
    resolve_elements(model, known, walls, false)
}

fn resolve_model(
    parsed: &ParsedModel,
    also: Option<&HashSet<JStr>>,
    walls: Option<&mut LinkedMap<JStr, JStr>>,
    prelude: bool,
) -> Result<ParsedModel, Thrown> {
    // the declared element FQNs and function ids + the platform's: the wildcard-disambiguation universe
    let mut own: HashSet<JStr> = HashSet::new();
    for el in parsed.elements.iter() {
        own.insert(el.qualified_name().clone());
        if let Some(f) = el.as_function() {
            own.insert(lite_model::function_id::mangle(&f));
        }
    }
    let known = ModelUniverse {
        own,
        also,
        platform: true,
    };
    resolve_elements(parsed, &known, walls, prelude)
}

fn resolve_elements(
    model: &ParsedModel,
    known: &dyn Universe,
    mut walls: Option<&mut LinkedMap<JStr, JStr>>,
    prelude: bool,
) -> Result<ParsedModel, Thrown> {
    // SECTION-scoped resolution: each element in its own section's imports, else the union scope; its own package
    // visible bare
    let mut profiles: HashSet<JStr> = lite_builtin::prelude::prelude()
        .profile_fqns()
        .iter()
        .cloned()
        .collect();
    for el in model.elements.iter() {
        if let PackageableElement::ProfileDefinition(pd) = el {
            profiles.insert(pd.qualified_name.clone());
        }
    }
    let element_imports: std::collections::HashMap<&JStr, &Arc<ImportScope>> =
        model.element_imports.iter().map(|(k, v)| (k, v)).collect();
    let mut resolved: Vec<PackageableElement> = Vec::with_capacity(model.elements.len());
    let mut changed = false;
    let sep: Vec<u16> = "::".encode_utf16().collect();
    for el in model.elements.iter() {
        let element_key = ParsedModel::key_of(el);
        let own = element_imports.get(&element_key);
        let fqn0 = el.qualified_name();
        let own_pkg = match lite_model::text::last_index_of(&fqn0.0, &sep) {
            Some(cut) if cut > 0 => Some(JStr::from(&fqn0.0[..cut])),
            _ => None,
        };
        let scope = Scope {
            imports: own.map_or_else(|| model.imports.clone(), |s| (*s).clone()),
            known,
            type_params: NO_TYPE_PARAMS.clone(),
            own_package: own_pkg,
            prelude,
            profile_fqns: &profiles,
        };
        match elements::resolve_element(el, &scope) {
            Ok(Some(r)) => {
                resolved.push(r);
                changed = true;
            }
            Ok(None) => resolved.push(el.clone()),
            Err(e) if e.is_resolution() => match walls.as_deref_mut() {
                // POISON-NOT-DROP: the element stays, unresolved as parsed
                Some(w) => {
                    w.put_if_absent(element_key, e.first_line());
                    resolved.push(el.clone());
                }
                None => {
                    return Err(Thrown::Compile(lite_error::CompileError {
                        message: e.message().map(|m| m.0),
                        ..lite_error::CompileError::model(Phase::Resolve, Vec::new(), Some(element_key.0))
                    }));
                }
            },
            Err(e) => return Err(e),
        }
    }
    if !changed {
        return Ok(model.clone());
    }
    // Java's six-argument ParsedModel: the unclaimed sections and the element spans are not carried
    Ok(ParsedModel {
        elements: lite_model::linked::list(resolved),
        imports: model.imports.clone(),
        source: model.source.clone(),
        element_offsets: model.element_offsets.clone(),
        element_imports: model.element_imports.clone(),
        element_sources: model.element_sources.clone(),
        unclaimed_sections: Arc::default(),
        element_spans: Arc::default(),
    })
}

/// `resolve(vs, scope)`: an expression in isolation, in a caller's scope.
pub fn resolve_in_scope(vs: &ValueSpecification, scope: &Scope<'_>) -> Result<ValueSpecification, Thrown> {
    Ok(values::resolve_vs(vs, scope)?.unwrap_or_else(|| vs.clone()))
}

/// `resolveQuery(query)`: a standalone query in the sectionless scope -- the platform's names alone, the prelude on;
/// a user element needs its full path, and an unresolved bare name passes through to fail in the typer.
pub fn resolve_query(query: &ValueSpecification) -> Result<ValueSpecification, Thrown> {
    resolve_in_scope(query, &QUERY_SCOPE)
}

/// `resolveQueryIn(query, imports, universe)`: a query under a section's imports, the prelude on, over a ready
/// universe (a compiled model's: [`IndexUniverse`]).
pub fn resolve_query_in(
    query: &ValueSpecification,
    imports: Arc<ImportScope>,
    universe: &dyn Universe,
) -> Result<ValueSpecification, Thrown> {
    resolve_in_scope(query, &Scope::prelude_of(imports, universe))
}

/// `Compiler.resolveNames(statements, imports, ctx)`'s resolution: the statements as a parameterless lambda resolved
/// by [`resolve_query_in`], its body back.
pub fn resolve_names(
    statements: &[ValueSpecification],
    imports: Arc<ImportScope>,
    universe: &dyn Universe,
) -> Result<Vec<ValueSpecification>, Thrown> {
    let lambda = ValueSpecification::LambdaFunction(Arc::new(LambdaFunction {
        parameters: Arc::default(),
        body: statements.to_vec().into(),
        pos: None,
    }));
    match resolve_query_in(&lambda, imports, universe)? {
        ValueSpecification::LambdaFunction(l) => Ok(l.body.to_vec()),
        _ => unreachable!("a lambda resolves to a lambda"),
    }
}

// =====================================================================
// The tiers
// =====================================================================

/// `resolveName(name, scope)`: a type position -- several imported matches is an ambiguity (`ResolutionException`).
pub(crate) fn resolve_name(name: &JStr, scope: &Scope<'_>) -> Result<JStr, Thrown> {
    let mut matches = resolve_name_multi(name, scope);
    if matches.len() > 1 {
        return Err(Thrown::resolution(cat!(
            "ambiguous reference '",
            name,
            "' \u{2014} matches via imports: ",
            crate::model_index::java_list(&matches),
            ". Use a fully qualified name."
        )));
    }
    Ok(matches.swap_remove(0))
}

/// `resolveNameMulti(name, scope)`: one element is the name resolved (or the name itself when nothing matched), several
/// are the imported packages that define it.
fn resolve_name_multi(name: &JStr, scope: &Scope<'_>) -> Vec<JStr> {
    if name.is_empty() || scope.type_params.contains(name) || qualified(name) {
        return vec![name.clone()];
    }
    // M3 PRIMITIVES are reserved bare names: reachable by no import; the prelude spells the m3 FQN
    if PRIMITIVE_TYPE_NAMES.iter().any(|p| name.eq_str(p)) {
        return vec![if scope.prelude {
            cat!("meta::pure::metamodel::type::", name)
        } else {
            name.clone()
        }];
    }
    let mut matches: Vec<JStr> = Vec::new();
    for pkg in scope.imports.wildcards.iter() {
        let candidate = under(pkg, name);
        if scope.known.contains(&candidate) && !matches.contains(&candidate) {
            matches.push(candidate);
        }
    }
    if !matches.is_empty() {
        return matches;
    }
    // OWN PACKAGE: a tier below the imports, never a peer
    if let Some(own) = &scope.own_package {
        let candidate = under(own, name);
        if scope.known.contains(&candidate) {
            return vec![candidate];
        }
    }
    // CORE IMPORTS: first match in the group's order
    for pkg in core_imports::SEQUENCE {
        let candidate = under_str(pkg, name);
        if scope.known.contains(&candidate) {
            return vec![candidate];
        }
    }
    // NO FALLBACK TIER: unresolved, it fails downstream as the engine's "Can't find type"
    vec![name.clone()]
}

/// `resolveCallCandidates(name, scope)`: a call position's candidates -- every function the name denotes across the
/// wildcards, the own package and the whole core group; a known or type-parameter name is the type tiers'.
fn resolve_call_candidates(name: &JStr, scope: &Scope<'_>) -> Vec<JStr> {
    if name.is_empty() || scope.known.contains(name) || scope.type_params.contains(name) {
        return resolve_name_multi(name, scope);
    }
    let out = call_tier_matches(name, scope);
    if out.is_empty() {
        resolve_name_multi(name, scope)
    } else {
        out
    }
}

/// `callTierMatches(name, scope)`: the call tiers' matches for a bare name, empty when none. A qualified spelling
/// matches no tier: it is not prefixed with a package (until 2026-11-13 it was, `pkg::a::b::f` asked of the universe for
/// every tier -- the names fixes, item 5, ruling 28).
fn call_tier_matches(name: &JStr, scope: &Scope<'_>) -> Vec<JStr> {
    if qualified(name) {
        return Vec::new();
    }
    let mut out: Vec<JStr> = Vec::with_capacity(2);
    let add = |c: JStr, out: &mut Vec<JStr>| {
        if scope.known.contains(&c) && !out.contains(&c) {
            out.push(c);
        }
    };
    for pkg in scope.imports.wildcards.iter() {
        add(under(pkg, name), &mut out);
    }
    if let Some(own) = &scope.own_package {
        add(under(own, name), &mut out);
    }
    for pkg in core_imports::SEQUENCE {
        add(under_str(pkg, name), &mut out);
    }
    out
}

/// A call's names as the resolver decides them (Java's `NameResolver.CallNames`): the spelling it leaves, the record
/// (`AppliedFunction.referents`), and whether anything was found.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CallNames {
    pub function: JStr,
    pub referents: Vec<JStr>,
    pub found: bool,
}

/// `decide(name, matches, prelude, arity)`: THE DECISION for a call name -- several imported packages are candidates
/// the typer unions; the platform joins the union for a bare name the tiers captured; a bare call the tiers left bare
/// records the platform's names at its arity; a name the tiers qualified to one function keeps that name.
fn decide(name: &JStr, mut matches: Vec<JStr>, prelude: bool, arity: usize) -> CallNames {
    let captured = !(matches.len() == 1 && &matches[0] == name);
    let bare = !qualified(name);
    if captured && prelude && bare {
        let mut merged: Option<Vec<JStr>> = None;
        for (_, natives) in bare_names::catalog_tiered(name) {
            for nf in natives {
                let nfq = &nf.def().qualified_name;
                if !matches.contains(nfq) && merged.as_ref().is_none_or(|m| !m.contains(nfq)) {
                    merged.get_or_insert_with(|| matches.clone()).push(nfq.clone());
                }
            }
        }
        if let Some(m) = merged {
            matches = m;
        }
    }
    let nothing_found = matches.len() == 1 && &matches[0] == name;
    let platform = if prelude && bare && nothing_found {
        bare_names::referents(name, arity)
    } else {
        Vec::new()
    };
    let all = if platform.is_empty() { &matches } else { &platform };
    let function = if !nothing_found && all.len() == 1 {
        all[0].clone()
    } else {
        name.clone()
    };
    let referents = if nothing_found {
        platform.clone()
    } else if all.len() > 1 {
        all.clone()
    } else {
        Vec::new()
    };
    CallNames {
        function,
        referents,
        found: !nothing_found || !platform.is_empty(),
    }
}

static BUILT: LazyLock<Mutex<std::collections::HashMap<(JStr, usize), CallNames>>> =
    LazyLock::new(|| Mutex::new(std::collections::HashMap::new()));

/// `builtCallNames(name, arity)`: what the resolver decides for a bare name at `arity` in the sectionless scope, so a
/// call built after the resolver is the call the parser and the resolver would have made of the same spelling.
/// Memoized for the process (a trivial answer is not kept).
pub fn built_call_names(name: &JStr, arity: usize) -> CallNames {
    let key = (name.clone(), arity);
    if let Some(hit) = BUILT.lock().expect("built").get(&key) {
        return hit.clone();
    }
    let made = decide(name, resolve_call_candidates(name, &QUERY_SCOPE), true, arity);
    if made.found {
        BUILT.lock().expect("built").entry(key).or_insert_with(|| made.clone());
    }
    made
}

/// `qualifyIdByBase(id, scope)`: a bare signature-id reference whose exact id no tier knows, qualified under the
/// package of the first tier that knows its base name (cut from the right); the spelling itself when no cut names one.
fn qualify_id_by_base(id: JStr, scope: &Scope<'_>) -> JStr {
    let res =
        lite_model::function_id::resolve(&id, |base| call_tier_matches(&JStr::from(base), scope), |_: &JStr| None);
    let Some(first) = res.base.first() else {
        return id;
    };
    let sep: Vec<u16> = "::".encode_utf16().collect();
    // Java's first.substring(0, first.lastIndexOf("::") + 2): a tier's match always has a package
    let cut = lite_model::text::last_index_of(&first.0, &sep).map_or(1, |i| i + 2);
    cat!(&first.0[..cut], &id)
}

/// `resolveProfile(name, scope)`: a stereotype's or tag's profile -- among profiles first, through the same tiers (as
/// legend-engine's profile-typed lookup); a name no profile has resolves as any name does.
pub(crate) fn resolve_profile(name: &JStr, scope: &Scope<'_>) -> Result<JStr, Thrown> {
    if qualified(name) {
        return Ok(name.clone());
    }
    for pkg in scope.imports.wildcards.iter() {
        let c = under(pkg, name);
        if scope.profile_fqns.contains(&c) {
            return Ok(c);
        }
    }
    if let Some(own) = &scope.own_package {
        let c = under(own, name);
        if scope.profile_fqns.contains(&c) {
            return Ok(c);
        }
    }
    for pkg in core_imports::SEQUENCE {
        let c = under_str(pkg, name);
        if scope.profile_fqns.contains(&c) {
            return Ok(c);
        }
    }
    resolve_name(name, scope)
}
