// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! The catalog's sections (Java's `PureTables`): `Pure`, `DynaFn` with its decisions, `EngineHandlers`, `Prelude`,
//! `Subsumed` and `TdsLegacy`, line for line as Java writes them.

use std::collections::BTreeSet;

use lite_builtin::dyna_fn::DynaFn;
use lite_builtin::dyna_fn_decisions::Resolution;
use lite_builtin::engine_handlers::handlers;
use lite_builtin::natives::OVERLOAD_GROUPS;
use lite_builtin::prelude::prelude;
use lite_builtin::pure::{
    ENGINE_VOCAB_SHIMS, INTERNAL_DESUGAR, LITE_SURFACE, NATIVE_CLASSES, NATIVE_ENUMS, NativeId, STRINGS,
    WALLED_NATIVES, catalog, is_to_one_call, lite, lite_surface_functions, walled_native_reason,
};
use lite_builtin::subsumed::{Subsumed, TdsLegacy};
use lite_front::jtypes::JStr;
use lite_front::records as p;

use super::support::{Out, b, join, json, sha, sorted};

pub fn dump(out: &mut Out) {
    let cat = catalog();
    // ---- the catalog
    for (i, def) in cat.all().iter().enumerate() {
        let n = NativeId(i as u16);
        out.line(
            "pure.native",
            &[
                i.to_string(),
                n.name().to_owned(),
                n.id().qualified.lossy(),
                json(&**def, true),
            ],
        );
    }
    for (i, c) in cat.native_classes().iter().enumerate() {
        out.line(
            "pure.class",
            &[i.to_string(), NATIVE_CLASSES[i].0.to_owned(), json(&**c, true)],
        );
    }
    for (i, e) in cat.native_enums().iter().enumerate() {
        out.line(
            "pure.enum",
            &[i.to_string(), NATIVE_ENUMS[i].0.to_owned(), json(&**e, true)],
        );
    }
    let mut groups: Vec<_> = OVERLOAD_GROUPS.to_vec();
    groups.sort_by_key(|(name, _)| *name);
    for (name, group) in groups {
        out.line(
            "pure.group",
            &[name.to_owned(), join(group.iter().map(|n| n.id().qualified.lossy()))],
        );
    }
    // ---- the constants
    let mut strings: Vec<_> = STRINGS.to_vec();
    strings.sort();
    for (name, value) in strings {
        out.line("pure.string", &[name.to_owned(), value.to_owned()]);
    }
    let mut lites: Vec<_> = lite::ALL.to_vec();
    lites.sort();
    for (name, value) in lites {
        out.line("pure.lite", &[name.to_owned(), value.to_owned()]);
    }
    out.line("pure.internalDesugar", &[join(INTERNAL_DESUGAR.iter())]);
    out.line("pure.engineVocabShims", &[join(ENGINE_VOCAB_SHIMS.iter())]);
    out.line("pure.liteSurface", &[sorted(LITE_SURFACE.iter())]);
    let mut surface = lite_surface_functions();
    surface.sort_by_key(|(name, _)| *name);
    for (name, group) in surface {
        out.line(
            "pure.liteSurfaceFunctions",
            &[name.to_owned(), join(group.iter().map(|n| n.id().qualified.lossy()))],
        );
    }
    out.line(
        "pure.liteInternalNatives",
        &[join(cat.lite_internal_natives().iter().map(JStr::lossy))],
    );
    for (i, fqn) in cat.user_resolvable_function_fqns().iter().enumerate() {
        out.line("pure.userResolvable", &[i.to_string(), fqn.lossy()]);
    }
    out.line(
        "pure.nativeClassFqns",
        &[sorted(cat.native_class_fqns().map(JStr::lossy))],
    );
    out.line(
        "pure.nativeEnumFqns",
        &[sorted(cat.native_enum_fqns().map(JStr::lossy))],
    );
    let walled: BTreeSet<&str> = WALLED_NATIVES.iter().map(|(f, _)| *f).collect();
    for fqn in walled {
        out.line(
            "pure.walled",
            &[fqn.to_owned(), walled_native_reason(fqn).unwrap().to_owned()],
        );
    }
    // ---- the indexes, asked of every FQN the catalog holds
    let fqns: BTreeSet<String> = cat.all().iter().map(|d| d.qualified_name.lossy()).collect();
    for fqn in &fqns {
        let j = JStr::from(fqn.as_str());
        let at = cat.native_functions_at(&j).unwrap();
        out.line(
            "pure.at",
            &[
                fqn.clone(),
                b(is_to_one_call(fqn)),
                b(cat.is_variadic_run(std::slice::from_ref(&j)).unwrap()),
                join(at.iter().map(|n| n.id().qualified.lossy())),
            ],
        );
    }
    let to_one = "meta::pure::functions::multiplicity::toOne";
    out.line("pure.isToOneCall", &[to_one.to_owned(), b(is_to_one_call(to_one))]);
    let bare = match cat.native_functions_at(&JStr::from("toOne")) {
        Ok(_) => "answered".to_owned(),
        Err(e) => e,
    };
    out.line("pure.bareName", &["toOne".to_owned(), bare]);
    let ids: BTreeSet<String> = (0..cat.all().len())
        .map(|i| NativeId(i as u16).id().qualified.lossy())
        .collect();
    for id in ids {
        let n = cat.native_function_by_id(&JStr::from(id.as_str())).unwrap();
        out.line("pure.byId", &[id, n.name().to_owned()]);
    }
    let mut supers: BTreeSet<String> = BTreeSet::new();
    for c in cat.native_classes() {
        supers.insert(c.qualified_name.lossy());
        for s in c.super_classes.iter() {
            if let p::TypeExpression::TypeExpressionNameRef(nr) = s {
                supers.insert(nr.name.lossy());
            }
        }
    }
    for fqn in supers {
        let j = JStr::from(fqn.as_str());
        out.line(
            "pure.directSubclasses",
            &[
                fqn,
                join(cat.direct_native_subclasses(&j).iter().map(JStr::lossy)),
                b(cat.find_native_class(&j).is_some()),
                b(cat.find_native_enum(&j).is_some()),
            ],
        );
    }
    for e in cat.native_enums() {
        out.line(
            "pure.findNativeEnum",
            &[
                e.qualified_name.lossy(),
                b(cat.find_native_enum(&e.qualified_name).is_some()),
            ],
        );
    }
    // ---- the engine's dynafunctions and the platform's decisions
    for d in DynaFn::values() {
        let lite = match d.lite_fqn() {
            Ok(f) => f.to_owned(),
            Err(e) => format!("!{e}"),
        };
        out.line(
            "dynafn",
            &[
                format!("{d:?}"),
                d.dyna_name().to_owned(),
                d.inference().name().to_owned(),
                d.resolution().name().to_owned(),
                d.fqns().join(","),
                lite,
                format!("{:?}", DynaFn::of(d.dyna_name()).unwrap()),
            ],
        );
    }
    for r in [
        Resolution::PURE,
        Resolution::SHIM,
        Resolution::TRANSLATED,
        Resolution::UNSUPPORTED,
    ] {
        out.line(
            "dynafn.withResolution",
            &[
                r.name().to_owned(),
                join(DynaFn::with_resolution(r).iter().map(|d| format!("{d:?}"))),
            ],
        );
    }
    // ---- the engine's handler surface
    let h = handlers();
    for name in h.names() {
        out.line(
            "handlers",
            &[
                name.to_owned(),
                join(h.ids_of(name).iter()),
                join(h.fqns_of(name).iter()),
            ],
        );
    }
    out.line("handlers.undeclared", &[join(h.undeclared_ids().iter())]);
    out.line("handlers.unmatchedSurface", &[join(h.unmatched_surface().iter())]);
    // ---- the prelude
    let pre = prelude();
    out.line(
        "prelude.source",
        &[sha(&lite_builtin::prelude::SOURCE.encode_utf16().collect::<Vec<u16>>())],
    );
    out.line(
        "prelude.model",
        &[sha(&json(pre.parsed_model(), true)
            .encode_utf16()
            .collect::<Vec<u16>>())],
    );
    out.line("prelude.elements", &[join(pre.element_fqns().iter().map(JStr::lossy))]);
    out.line("prelude.classes", &[join(pre.class_fqns().iter().map(JStr::lossy))]);
    out.line("prelude.enums", &[join(pre.enum_fqns().iter().map(JStr::lossy))]);
    out.line(
        "prelude.functions",
        &[join(pre.function_fqns().iter().map(JStr::lossy))],
    );
    out.line(
        "prelude.functionIds",
        &[join(pre.function_ids().iter().map(|i| i.qualified.lossy()))],
    );
    out.line("prelude.profiles", &[join(pre.profile_fqns().iter().map(JStr::lossy))]);
    // ---- the programs subsumed, and the legacy TDS vocabulary
    for s in Subsumed::VALUES {
        out.line(
            "subsumed",
            &[
                s.name().to_owned(),
                s.fqn().to_owned(),
                s.engine_file().to_owned(),
                s.from_line().to_string(),
                s.to_line().to_string(),
                s.reason().to_owned(),
                Subsumed::of(Some(s.fqn())).unwrap().name().to_owned(),
            ],
        );
    }
    for (t, name, fqn) in TdsLegacy::ROWS {
        out.line("tdsLegacy", &[name.to_owned(), fqn.to_owned(), t.bare().to_owned()]);
    }
}
