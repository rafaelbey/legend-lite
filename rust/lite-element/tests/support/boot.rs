// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! The planner's half of what step F is handed (2026-11-15, the element slice, step 5): `Compiler.boot()`,
//! `bootFqns()`, `withoutPreludeShadows` and `normalizeWithSystem` (Compiler.java:303-500 at 18feff3fe), composed over
//! lite-names, lite-normalizer, lite-platform and lite-builtin, line for line. They are the compile driver's, not
//! lite-element's API, so they live here, in the differential's test support, until the planner's slice ports them.
//!
//! The boot is split in two, as the differential reads it: [`boot_normalized`] (the system metamodel and the prelude,
//! resolved, the platform's own Pure adopted, `normalizeLayer`: no element compiler) and [`boot_checked`]
//! (`PureModelContext.checkLayer` over it, lite-element's). Each is made once per process and its outcome cached, a
//! failure or a panic as its message, so every ask that needs it meets the same failure again (Java's boot cannot fail
//! but by a platform bug: a refusal here is a defect either way).

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, LazyLock, Mutex, OnceLock};

use lite_builtin::prelude::prelude;
use lite_builtin::system_metamodel;
use lite_element::{CheckedLayer, Model, Thrown};
use lite_model::linked::{LinkedMap, empty};

use lite_model::{
    Function, FunctionDefinition, FunctionId, ImportScope, JStr, LegacyMappingDefinition, NormalizedModel,
    PackageableElement, ParsedModel,
};
use lite_names::model_index::ModelIndex;
use lite_names::{knowledge, resolver};
use lite_platform::{AdoptError, platform_pure};

use super::registrations::platform_registrations;

/// `FunctionId.of(fd)`.
pub fn id_of(fd: &Arc<FunctionDefinition>) -> FunctionId {
    FunctionId::of(&Function::FunctionDefinition(fd.clone()))
}

/// lite-normalizer's throw as lite-names' (the two carry the same classes: a `com.legend.error` class or a JDK one).
pub fn from_normalizer(e: lite_normalizer::Thrown) -> Thrown {
    match e {
        lite_normalizer::Thrown::Compile(c) => Thrown::Compile(c),
        lite_normalizer::Thrown::Java { class, message } => Thrown::Java { class, message },
    }
}

/// `PlatformPure.adopt`'s throw, as Java throws it: a `ModelException`, or a raw `IllegalArgumentException`.
fn from_adopt(e: AdoptError) -> Thrown {
    match e {
        AdoptError::Model(c) => Thrown::Compile(c),
        AdoptError::IllegalArgument(x) => Thrown::illegal_argument(x.message),
    }
}

/// A rebuilt `ParsedModel`: `elements` with every side component of `like` (Java's eight-argument constructor; the
/// element spans ride the rebuild since 2026-11-13, ruling 32).
fn rebuilt(elements: Vec<PackageableElement>, like: &ParsedModel) -> ParsedModel {
    ParsedModel {
        elements: elements.into(),
        imports: like.imports.clone(),
        source: like.source.clone(),
        element_offsets: like.element_offsets.clone(),
        element_imports: like.element_imports.clone(),
        element_sources: like.element_sources.clone(),
        unclaimed_sections: like.unclaimed_sections.clone(),
        element_spans: like.element_spans.clone(),
    }
}

/// `Compiler.normalizeLayer(resolved, walls)` (Compiler.java:272): the association qualified properties adopted into
/// their owners, THE index, `refuseDuplicates`, then E over it; the model and the index E read.
pub fn normalize_layer(
    resolved: &ParsedModel,
    mut walls: Option<&mut LinkedMap<JStr, JStr>>,
) -> Result<(NormalizedModel, ModelIndex), Thrown> {
    let adopted = knowledge::adopt_association_qualified_properties(resolved, walls.as_deref_mut())?;
    let index = ModelIndex::from(&adopted)?;
    index.refuse_duplicates(walls.as_deref_mut())?;
    let model = lite_normalizer::normalize(&adopted, &index, walls).map_err(from_normalizer)?;
    Ok((model, index))
}

/// The boot layer as `Compiler.boot()` normalizes it, before its check: the model, its own index (which the check
/// reads, once, and the graphs never do) and its functions by id (`Boot.functionsById`, the body a graph's
/// declaration with that id takes).
pub struct BootNormalized {
    pub model: NormalizedModel,
    /// taken by [`boot_checked`]'s one `checkLayer`
    index: Mutex<Option<ModelIndex>>,
    pub functions_by_id: HashMap<FunctionId, Arc<FunctionDefinition>>,
}

/// `Compiler.boot()`'s body up to `checkLayer` (Compiler.java:303-367): the prelude, refused if it redefines a system
/// element; the system metamodel's elements then the prelude's, in a `ParsedModel` made by Java's six-argument
/// constructor (no source, the empty scope, the prelude's offsets, imports and sources, no unclaimed sections, no
/// spans); resolved; each prelude function with a system version's id takes that version's body (`PlatformPure.adopt`),
/// the system's own declaration of it dropped; then `normalizeLayer`, strict.
fn normalize_boot() -> Result<BootNormalized, Thrown> {
    let pre = prelude().parsed_model();
    system_metamodel::require_no_system_element_redefined(pre).map_err(Thrown::Compile)?;
    let system = system_metamodel::elements();
    let mut elements: Vec<PackageableElement> = system.to_vec();
    elements.extend(pre.elements.iter().cloned());
    let boot = ParsedModel {
        elements: elements.into(),
        imports: ImportScope::empty(),
        source: None,
        element_offsets: pre.element_offsets.clone(),
        element_imports: pre.element_imports.clone(),
        element_sources: pre.element_sources.clone(),
        unclaimed_sections: empty(),
        element_spans: empty(),
    };
    let resolved = resolver::resolve(&boot)?;
    let system_count = system.len();
    // the system's own functions by id (a HashMap: the last of an id wins, as Java's put)
    let mut own: HashMap<FunctionId, Arc<FunctionDefinition>> = HashMap::new();
    for el in &resolved.elements[..system_count] {
        if let PackageableElement::FunctionDefinition(fd) = el {
            own.insert(id_of(fd), fd.clone());
        }
    }
    let mut adopted: HashSet<FunctionId> = HashSet::new();
    let mut merged: Vec<PackageableElement> = Vec::with_capacity(resolved.elements.len());
    for el in &resolved.elements[system_count..] {
        if let PackageableElement::FunctionDefinition(fd) = el {
            let id = id_of(fd);
            if let Some(version) = own.get(&id) {
                merged.push(PackageableElement::FunctionDefinition(
                    platform_pure::adopt(fd, version).map_err(from_adopt)?,
                ));
                adopted.insert(id);
                continue;
            }
        }
        merged.push(el.clone());
    }
    let mut all: Vec<PackageableElement> = Vec::with_capacity(resolved.elements.len());
    for el in &resolved.elements[..system_count] {
        let adopted_here = matches!(el, PackageableElement::FunctionDefinition(fd) if adopted.contains(&id_of(fd)));
        if !adopted_here {
            all.push(el.clone());
        }
    }
    all.extend(merged);
    let (model, index) = normalize_layer(&rebuilt(all, &resolved), None)?;
    // Boot.functionsById: a LinkedHashMap read by get alone; the last definition of an id wins
    let mut functions_by_id = HashMap::new();
    for el in model.elements.iter() {
        if let PackageableElement::FunctionDefinition(fd) = el {
            functions_by_id.insert(id_of(fd), fd.clone());
        }
    }
    Ok(BootNormalized {
        model,
        index: Mutex::new(Some(index)),
        functions_by_id,
    })
}

/// A panic's message (a stub's `unimplemented!` among them).
pub fn panic_text(payload: &(dyn std::any::Any + Send)) -> String {
    payload
        .downcast_ref::<String>()
        .cloned()
        .or_else(|| payload.downcast_ref::<&'static str>().map(|s| (*s).to_owned()))
        .unwrap_or_else(|| "a panic with no message".to_owned())
}

/// How a cached step ended: its value, a throw (written as the differential writes an error), or a panic's message.
pub enum Outcome<T> {
    Done(T),
    Threw(Thrown),
    Panicked(String),
}

/// `f`, its panic caught, on a parse-sized stack (the boot resolves and normalizes the whole prelude).
fn caught<T: Send>(f: impl FnOnce() -> Result<T, Thrown> + Send) -> Outcome<T> {
    lite_front::doors::on_parse_stack(|| match std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)) {
        Ok(Ok(v)) => Outcome::Done(v),
        Ok(Err(e)) => Outcome::Threw(e),
        Err(payload) => Outcome::Panicked(panic_text(&*payload)),
    })
}

static BOOT_NORMALIZED: OnceLock<Outcome<BootNormalized>> = OnceLock::new();
static BOOT_CHECKED: OnceLock<Outcome<CheckedLayer>> = OnceLock::new();

/// The boot layer normalized (milestone 1's subject), made once per process.
pub fn boot_normalized() -> &'static Outcome<BootNormalized> {
    BOOT_NORMALIZED.get_or_init(|| caught(normalize_boot))
}

/// The boot layer normalized, or the failure re-raised here as a panic: a boot that does not normalize is a defect.
pub fn boot_model() -> &'static BootNormalized {
    match boot_normalized() {
        Outcome::Done(b) => b,
        Outcome::Threw(e) => panic!("the boot layer does not normalize: {e:?}"),
        Outcome::Panicked(m) => panic!("{m}"),
    }
}

/// `Boot.checked` (Compiler.java:369): `PureModelContext.checkLayer(model, index, registrations)` over the normalized
/// boot, once per process. Java hands it the lowering's registrations (`PlatformRegistrations.current()`); Rust has no
/// lowering yet, so it is handed the registrations the differential hands every model (rust/oracles/builtin's
/// PlatformTables' set): the checked layer is the boot's elements by identity and its ids, which no registration
/// reaches.
pub fn boot_checked() -> &'static CheckedLayer {
    let outcome = BOOT_CHECKED.get_or_init(|| {
        let boot = boot_model();
        let index = boot
            .index
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .take()
            .expect("the boot's index is checked once");
        let registrations = platform_registrations();
        caught(move || Model::check_layer(&boot.model, index, &registrations))
    });
    match outcome {
        Outcome::Done(c) => c,
        Outcome::Threw(e) => panic!("the boot layer does not check: {e:?}"),
        Outcome::Panicked(m) => panic!("{m}"),
    }
}

/// `Compiler.bootFqns()` (Compiler.java:381), ElementAnswers' `BOOT_FQNS`: the system metamodel's element names and the
/// prelude's.
pub fn boot_fqns() -> &'static HashSet<JStr> {
    static FQNS: LazyLock<HashSet<JStr>> = LazyLock::new(|| {
        let mut out: HashSet<JStr> = system_metamodel::element_fqns().iter().cloned().collect();
        out.extend(prelude().element_fqns().iter().cloned());
        out
    });
    &FQNS
}

/// The prelude's names a graph copy yields to (`Prelude.classFqns()`, `enumFqns()`, `profileFqns()`, `functionIds()`),
/// as sets.
struct Shadows {
    classes: HashSet<JStr>,
    enums: HashSet<JStr>,
    profiles: HashSet<JStr>,
    functions: HashSet<FunctionId>,
}

static SHADOWS: LazyLock<Shadows> = LazyLock::new(|| {
    let p = prelude();
    Shadows {
        classes: p.class_fqns().iter().cloned().collect(),
        enums: p.enum_fqns().iter().cloned().collect(),
        profiles: p.profile_fqns().iter().cloned().collect(),
        functions: p.function_ids().iter().cloned().collect(),
    }
});

/// `Compiler.withoutPreludeShadows(parsed)` (Compiler.java:404): a graph class, enum or profile at a prelude FQN, or a
/// function with a prelude function's id, is dropped (the prelude wins); the model itself when nothing is.
pub fn without_prelude_shadows(parsed: &ParsedModel) -> ParsedModel {
    let s = &*SHADOWS;
    let kept: Vec<PackageableElement> = parsed
        .elements
        .iter()
        .filter(|el| {
            let shadow = match el {
                PackageableElement::ClassDefinition(c) => s.classes.contains(&c.qualified_name),
                PackageableElement::EnumDefinition(e) => s.enums.contains(&e.qualified_name),
                PackageableElement::ProfileDefinition(p) => s.profiles.contains(&p.qualified_name),
                PackageableElement::FunctionDefinition(fd) => s.functions.contains(&id_of(fd)),
                _ => false,
            };
            !shadow
        })
        .cloned()
        .collect();
    if kept.len() == parsed.elements.len() {
        parsed.clone()
    } else {
        rebuilt(kept, parsed)
    }
}

/// `Compiler.union(a, b)` (Compiler.java:493): `a` when `b` is empty, else a `LinkedHashMap` of `a` with `b` put in
/// (a key of both keeps `a`'s place and takes `b`'s value).
fn union(
    a: &Arc<[(JStr, Arc<LegacyMappingDefinition>)]>,
    b: &Arc<[(JStr, Arc<LegacyMappingDefinition>)]>,
) -> Arc<[(JStr, Arc<LegacyMappingDefinition>)]> {
    if b.is_empty() {
        return a.clone();
    }
    let mut out: LinkedMap<JStr, Arc<LegacyMappingDefinition>> = LinkedMap::new();
    for (k, v) in a.iter().chain(b.iter()) {
        out.put(k.clone(), v.clone());
    }
    out.freeze()
}

/// `Compiler.Layer` as `normalizeWithSystem` returns it: the model F is handed, the index E read (the graph's own),
/// the boot ids the graph declares itself with the platform's body, and how many of the model's elements are the
/// graph's own (they come first; the rest are the boot's very elements).
pub struct Layer {
    pub model: NormalizedModel,
    pub index: ModelIndex,
    pub adopted: HashSet<FunctionId>,
    pub own: usize,
}

/// `Compiler.normalizeWithSystem(resolved, walls)` (Compiler.java:443): the prelude's shadows dropped, a system
/// element's redefinition refused, each graph function with a boot version's id adopted (its declaration with the
/// boot's body; in a tolerant build a refusal walls the id, first reason kept, and the declaration stays as loaded),
/// E over the graph's own elements, then the boot's elements but the adopted ids' appended, the legacy surfaces the
/// union of both.
pub fn normalize_with_system(
    resolved: &ParsedModel,
    mut walls: Option<&mut LinkedMap<JStr, JStr>>,
) -> Result<Layer, Thrown> {
    let boot = boot_model();
    let graph = without_prelude_shadows(resolved);
    system_metamodel::require_no_system_element_redefined(&graph).map_err(Thrown::Compile)?;
    let mut adopted: HashSet<FunctionId> = HashSet::new();
    let mut own: Vec<PackageableElement> = Vec::with_capacity(graph.elements.len());
    for el in graph.elements.iter() {
        if let PackageableElement::FunctionDefinition(fd) = el {
            let id = id_of(fd);
            if let Some(version) = boot.functions_by_id.get(&id) {
                match platform_pure::adopt(fd, version) {
                    Ok(merged) => {
                        own.push(PackageableElement::FunctionDefinition(merged));
                        adopted.insert(id);
                        continue;
                    }
                    // POISON-NOT-DROP: the declaration stays as loaded, walled under its id
                    Err(AdoptError::Model(e)) => match walls.as_deref_mut() {
                        None => return Err(Thrown::Compile(e)),
                        Some(w) => {
                            let line = Thrown::Compile(e).first_line();
                            w.put_if_absent(id.qualified.clone(), line);
                        }
                    },
                    // a raw IllegalArgumentException: no catch takes it
                    Err(e) => return Err(from_adopt(e)),
                }
            }
        }
        own.push(el.clone());
    }
    let input = if adopted.is_empty() {
        graph
    } else {
        rebuilt(own, &graph)
    };
    let (user, index) = normalize_layer(&input, walls)?;
    let mut elements: Vec<PackageableElement> = user.elements.to_vec();
    let own_count = elements.len();
    for el in boot.model.elements.iter() {
        let adopted_here = matches!(el, PackageableElement::FunctionDefinition(fd) if adopted.contains(&id_of(fd)));
        if !adopted_here {
            elements.push(el.clone());
        }
    }
    let model = NormalizedModel {
        elements: elements.into(),
        imports: user.imports.clone(),
        legacy_surfaces: union(&user.legacy_surfaces, &boot.model.legacy_surfaces),
    };
    Ok(Layer {
        model,
        index,
        adopted,
        own: own_count,
    })
}
