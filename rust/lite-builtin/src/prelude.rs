// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! `Prelude`: the generated prelude module (core's `prelude.pure`, written by //spec:gen_prelude from the pinned
//! upstream and read here as the same committed file), parsed once at the platform's dialect; its element FQNs by kind,
//! in module order (the resolver's bare-name fallback reads them in that order).

use std::collections::HashSet;
use std::sync::{Arc, LazyLock};

use lite_front::doors::Dialect;
use lite_front::jtypes::JStr;
use lite_model::{ClassDefinition, EnumDefinition, Function, FunctionId, PackageableElement, ParsedModel};

/// The module's source text, the committed file the Java reads.
pub const SOURCE: &str = include_str!("../../../core/src/main/resources/com/legend/builtin/prelude.pure");

pub struct Prelude {
    parsed: ParsedModel,
    class_fqns: Vec<JStr>,
    enum_fqns: Vec<JStr>,
    function_fqns: Vec<JStr>,
    function_ids: Vec<FunctionId>,
    profile_fqns: Vec<JStr>,
    element_fqns: Vec<JStr>,
    element_fqn_set: HashSet<JStr>,
}

static PRELUDE: LazyLock<Prelude> = LazyLock::new(build);

/// The prelude (parsed on first read).
pub fn prelude() -> &'static Prelude {
    &PRELUDE
}

/// Each distinct value of `values`, in first-seen order (a `LinkedHashSet`).
fn ordered<T: Clone + Eq + std::hash::Hash>(values: impl Iterator<Item = T>) -> Vec<T> {
    let mut seen = HashSet::new();
    values.filter(|v| seen.insert(v.clone())).collect()
}

fn build() -> Prelude {
    let units: Vec<u16> = SOURCE.encode_utf16().collect();
    let parsed = crate::door::parse(&units, Dialect::LegendPlatform)
        .unwrap_or_else(|e| panic!("the prelude does not parse: {e:?}"));
    let els = || parsed.elements.iter();
    let class_fqns = ordered(
        els()
            .filter(|e| matches!(e, PackageableElement::ClassDefinition(_)))
            .map(|e| e.qualified_name().clone()),
    );
    let enum_fqns = ordered(
        els()
            .filter(|e| matches!(e, PackageableElement::EnumDefinition(_)))
            .map(|e| e.qualified_name().clone()),
    );
    let function_fqns = ordered(
        els()
            .filter(|e| matches!(e, PackageableElement::FunctionDefinition(_)))
            .map(|e| e.qualified_name().clone()),
    );
    let function_ids = ordered(els().filter_map(|e| match e {
        PackageableElement::FunctionDefinition(f) => Some(FunctionId::of(&Function::FunctionDefinition(f.clone()))),
        _ => None,
    }));
    let profile_fqns = ordered(
        els()
            .filter(|e| matches!(e, PackageableElement::ProfileDefinition(_)))
            .map(|e| e.qualified_name().clone()),
    );
    let element_fqns = ordered(els().map(|e| e.qualified_name().clone()));
    let element_fqn_set = element_fqns.iter().cloned().collect();
    Prelude {
        class_fqns,
        enum_fqns,
        function_fqns,
        function_ids,
        profile_fqns,
        element_fqns,
        element_fqn_set,
        parsed,
    }
}

impl Prelude {
    /// `Prelude.parsedModel()`.
    pub fn parsed_model(&self) -> &ParsedModel {
        &self.parsed
    }

    /// `Prelude.elements()`, in module order.
    pub fn elements(&self) -> &[PackageableElement] {
        &self.parsed.elements
    }

    /// `Prelude.elementFqns()`, in module order.
    pub fn element_fqns(&self) -> &[JStr] {
        &self.element_fqns
    }

    /// Whether the prelude declares `fqn`.
    pub fn declares(&self, fqn: &JStr) -> bool {
        self.element_fqn_set.contains(fqn)
    }

    /// `Prelude.classFqns()`, in module order.
    pub fn class_fqns(&self) -> &[JStr] {
        &self.class_fqns
    }

    /// `Prelude.functionFqns()`: the platform library's functions, in module order.
    pub fn function_fqns(&self) -> &[JStr] {
        &self.function_fqns
    }

    /// `Prelude.functionIds()`, in module order.
    pub fn function_ids(&self) -> &[FunctionId] {
        &self.function_ids
    }

    /// `Prelude.enumFqns()`, in module order.
    pub fn enum_fqns(&self) -> &[JStr] {
        &self.enum_fqns
    }

    /// `Prelude.profileFqns()`, in module order.
    pub fn profile_fqns(&self) -> &[JStr] {
        &self.profile_fqns
    }

    /// `Prelude.cls(fqn)`: the prelude's class at `fqn`, as parsed; another refused (Java's
    /// `IllegalArgumentException`).
    pub fn cls(&self, fqn: &JStr) -> Result<&Arc<ClassDefinition>, String> {
        self.parsed
            .elements
            .iter()
            .find_map(|e| match e {
                PackageableElement::ClassDefinition(c) if c.qualified_name == *fqn => Some(c),
                _ => None,
            })
            .ok_or_else(|| format!("not a generated class: {fqn}"))
    }

    /// `Prelude.enumOf(fqn)`: the prelude's enumeration at `fqn`; another refused.
    pub fn enum_of(&self, fqn: &JStr) -> Result<&Arc<EnumDefinition>, String> {
        self.parsed
            .elements
            .iter()
            .find_map(|e| match e {
                PackageableElement::EnumDefinition(d) if d.qualified_name == *fqn => Some(d),
                _ => None,
            })
            .ok_or_else(|| format!("not a generated enum: {fqn}"))
    }
}
