// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! The platform's sections (Java's `PlatformTables`): `CoreFn`, `Feature`, `WalledBodies`, the `Implementation` enums,
//! `PlatformPure`, the platform's declarations and the implementation tables over them (the platform's registrations
//! as the lowering would assemble them but for its own keys, overrides and members), the catalog's table, and the
//! tables of the platform tests' cases -- line for line as Java writes them. Java's hash-ordered sources (the walled
//! natives' `Map.of`, each form's `Set.of`) are handed to a table sorted on both sides; every case's maps are handed in
//! the order written.

use std::collections::BTreeSet;
use std::sync::Arc;

use lite_builtin::native_fn::Family;
use lite_builtin::prelude::prelude;
use lite_builtin::pure::{INTERNAL_DESUGAR, NativeId, WALLED_NATIVES, catalog, lite};
use lite_builtin::subsumed::Subsumed;
use lite_builtin::system_metamodel;
use lite_front::doors::Dialect;
use lite_model::linked::{LinkedMap, empty};
use lite_model::{
    ClassMember, Function, FunctionDefinition, FunctionDefinitionParameterDefinition, FunctionDefinitionSynthesized,
    FunctionId, JStr, NativeFunctionDefinition, SynthHat,
};
use lite_platform::{
    AdoptError, CoreFn, DeclarationTable, Feature, Implementation, ImplementationTable, Kind, Position, Reason,
    Registrations, Wall, platform_pure, walled_bodies,
};

use super::support::{self, Out, b, join, json, sha, sorted};

/// A text as the codec writes a string.
fn quoted(s: &JStr) -> String {
    support::str(s)
}

const UPPER_ID: &str = "meta::pure::functions::string::toUpper_String_1__String_1_";
const LOWER_ID: &str = "meta::pure::functions::string::toLower_String_1__String_1_";
const TRIM_ID: &str = "meta::pure::functions::string::trim_String_1__String_1_";
const LENGTH_ID: &str = "meta::pure::functions::string::length_String_1__Integer_1_";

/// One line of fields, each written as Java's `String.valueOf` writes it.
macro_rules! put {
    ($out:expr, $section:expr $(, $f:expr)* $(,)?) => {
        $out.line($section, &[$($f.to_string()),*])
    };
}

pub fn dump(out: &mut Out) {
    core_fn(out);
    feature(out);
    walled(out);
    implementation_enums(out);
    platform_pure_section(out);
    let platform = platform_declarations();
    let declarations = DeclarationTable::of(platform.iter().cloned());
    declarations_section(out, "platform.declarations", None, &platform, &declarations);
    let table = ImplementationTable::build(&declarations, &platform_registrations());
    // The boot declarations' table, not the platform's whole one: its 16 dangling registrations are upstream
    // declarations only a loaded module supplies, each read at its name in the pinned sources (PlatformTables.java says
    // which; 2026-11-12, the owner's ruling). spec's ImplementationTableTest holds the whole table to none.
    table_section(out, "platform.table", None, &table, &declarations);
    outside(out, &table);
    let all: Vec<Function> = catalog().all().iter().map(native).collect();
    let catalog_table = DeclarationTable::of(all.iter().cloned());
    put!(out, "platform.catalogTable.size", catalog_table.size(), all.len());
    table_section(
        out,
        "platform.catalogTable",
        None,
        &ImplementationTable::build(&catalog_table, &platform_registrations()),
        &catalog_table,
    );
    declaration_cases(out);
    implementation_cases(out);
}

// ---------------------------------------------------------------- the enums and constants

fn name(f: Option<CoreFn>) -> String {
    f.map_or_else(|| "-".to_owned(), |f| f.name().to_owned())
}

fn core_fn(out: &mut Out) {
    let mut texts: BTreeSet<JStr> = BTreeSet::new();
    for (i, f) in CoreFn::VALUES.iter().enumerate() {
        put!(
            out,
            "platform.coreFn",
            i,
            f.name(),
            f.parse_name(),
            f.spellings().join(" "),
            f.owned_fqns().len(),
            sorted(f.owned_fqns().iter())
        );
        texts.extend(f.spellings().iter().map(|s| JStr::from(*s)));
        texts.extend(f.owned_fqns().iter().map(|s| JStr::from(*s)));
    }
    let mut names: Vec<(&str, CoreFn)> = CoreFn::parse_names().to_vec();
    names.sort_by_key(|(n, _)| *n);
    for (n, f) in names {
        put!(out, "platform.coreFn.parseNames", n, f.name());
    }
    for d in catalog().all() {
        texts.insert(d.qualified_name.clone());
    }
    for d in INTERNAL_DESUGAR {
        texts.insert(JStr::from(*d));
        texts.insert(JStr::from(format!("{}{d}", lite::PKG)));
    }
    for t in [
        "",
        "::",
        "Filter",
        "filter ",
        "x::filter",
        "meta::pure::functions::collection::Filter",
        "meta::pure::functions::collection::",
        "meta::pure::tds",
        "meta::legend::lite::",
        "new",
        "toUpper",
        "meta::pure::functions::string::toUpper",
    ] {
        texts.insert(JStr::from(t));
    }
    for t in &texts {
        let text = t.lossy();
        put!(
            out,
            "platform.coreFn.lookup",
            quoted(t),
            name(CoreFn::owning(&text)),
            name(CoreFn::of(&text))
        );
    }
}

fn feature(out: &mut Out) {
    for (i, f) in Feature::VALUES.iter().enumerate() {
        put!(out, "platform.feature", i, f.name());
    }
    put!(out, "platform.feature.fqn", Feature::FQN);
}

fn walled(out: &mut Out) {
    for (i, k) in Kind::VALUES.iter().enumerate() {
        put!(out, "platform.walledBodies.kind", i, k.name());
    }
    for (i, (fqn, wall)) in walled_bodies::reasons().iter().enumerate() {
        put!(out, "platform.walledBodies", i, fqn, wall.kind.name(), wall.why);
    }
    let mut asked: BTreeSet<JStr> = walled_bodies::reasons().iter().map(|(k, _)| k.clone()).collect();
    for t in [
        "",
        "my::pkg::nothing",
        "meta::relational::functions::sqlQueryToString::DbConfig$prop$",
        "meta::pure::tds::schema::SchemaState",
        "sqlQueryToString",
    ] {
        asked.insert(JStr::from(t));
    }
    for fqn in &asked {
        let r = walled_bodies::reason(&fqn.lossy());
        put!(
            out,
            "platform.walledBodies.reason",
            quoted(fqn),
            r.map_or_else(|| "null".to_owned(), JStr::lossy)
        );
    }
    put!(out, "platform.walledBodies.count", walled_bodies::count());
}

fn implementation_enums(out: &mut Out) {
    for (i, p) in Position::VALUES.iter().enumerate() {
        put!(out, "platform.implementation.position", i, p.name());
    }
    for (i, r) in Reason::VALUES.iter().enumerate() {
        put!(out, "platform.implementation.reason", i, r.name());
    }
}

// ---------------------------------------------------------------- the platform's own Pure

/// Java's twelve-argument constructor over `f`, with these parameters and this body.
fn with_body(
    f: &FunctionDefinition,
    parameters: Arc<[Arc<FunctionDefinitionParameterDefinition>]>,
    body: Arc<[lite_model::protocol::ValueSpecification]>,
) -> Arc<FunctionDefinition> {
    Arc::new(FunctionDefinition {
        parameters,
        body,
        ..f.clone()
    })
}

fn adopt(
    out: &mut Out,
    kase: &str,
    k: usize,
    declaration: &Arc<FunctionDefinition>,
    platform: &Arc<FunctionDefinition>,
) {
    match platform_pure::adopt(declaration, platform) {
        Ok(r) => put!(
            out,
            "platform.platformPure.adopt",
            kase,
            k,
            "returned",
            sha(&json(&*r, true).encode_utf16().collect::<Vec<u16>>())
        ),
        Err(AdoptError::IllegalArgument(e)) => put!(
            out,
            "platform.platformPure.adopt",
            kase,
            k,
            "IllegalArgumentException",
            e.message
        ),
        Err(AdoptError::Model(e)) => put!(
            out,
            "platform.platformPure.adopt",
            kase,
            k,
            "ModelException",
            e.phase.map_or("null", |p| p.name()),
            e.element
                .as_deref()
                .map_or_else(|| "null".to_owned(), String::from_utf16_lossy),
            e.message_lossy()
        ),
    }
}

fn platform_pure_section(out: &mut Out) {
    for (i, id) in platform_pure::ids().iter().enumerate() {
        put!(out, "platform.platformPure.id", i, id.qualified);
    }
    for (i, (id, why)) in platform_pure::upstream_bodies().iter().enumerate() {
        put!(out, "platform.platformPure.upstreamBody", i, id.qualified, why);
    }
    for (i, (id, wall)) in platform_pure::refused_versions().iter().enumerate() {
        put!(
            out,
            "platform.platformPure.refusedVersion",
            i,
            id.qualified,
            wall.kind.name(),
            wall.why
        );
    }
    // adopt over every system function: itself without its body, its last parameter renamed, the next function
    let sys: Vec<Arc<FunctionDefinition>> = system_metamodel::functions_by_id()
        .into_iter()
        .map(|(_, f)| f)
        .collect();
    for (k, f) in sys.iter().enumerate() {
        adopt(out, "bodiless", k, &with_body(f, f.parameters.clone(), empty()), f);
        if let Some(last) = f.parameters.last() {
            let mut ps: Vec<Arc<FunctionDefinitionParameterDefinition>> = f.parameters.to_vec();
            let renamed = Arc::new(FunctionDefinitionParameterDefinition {
                name: JStr([last.name.0.clone(), "Renamed".encode_utf16().collect()].concat()),
                r#type: last.r#type.clone(),
                multiplicity: last.multiplicity.clone(),
            });
            *ps.last_mut().expect("a parameter") = renamed;
            adopt(out, "renamed", k, &with_body(f, Arc::from(ps), f.body.clone()), f);
        }
        adopt(out, "other", k, f, &sys[(k + 1) % sys.len()]);
    }
}

// ---------------------------------------------------------------- the declarations

fn native(n: &Arc<NativeFunctionDefinition>) -> Function {
    Function::NativeFunctionDefinition(n.clone())
}

/// The platform's declarations: the catalog, the prelude's functions, the system metamodel's, in that order.
fn platform_declarations() -> Vec<Function> {
    let mut all: Vec<Function> = catalog().all().iter().map(native).collect();
    all.extend(prelude().elements().iter().filter_map(|e| e.as_function()));
    all.extend(
        system_metamodel::functions_by_id()
            .into_iter()
            .map(|(_, f)| Function::FunctionDefinition(f)),
    );
    all
}

/// A line's fields after its case's key, when it has one.
fn keyed(key: Option<&str>, rest: Vec<String>) -> Vec<String> {
    key.map(str::to_owned).into_iter().chain(rest).collect()
}

fn kind(f: &Function) -> &'static str {
    match f {
        Function::FunctionDefinition(_) => "FunctionDefinition",
        Function::NativeFunctionDefinition(_) => "NativeFunctionDefinition",
    }
}

fn declarations_section(out: &mut Out, section: &str, key: Option<&str>, input: &[Function], d: &DeclarationTable) {
    out.line(
        &format!("{section}.size"),
        &keyed(key, vec![input.len().to_string(), d.size().to_string()]),
    );
    for (i, (id, f)) in d.entries().iter().enumerate() {
        out.line(
            &format!("{section}.id"),
            &keyed(
                key,
                vec![
                    i.to_string(),
                    id.qualified.lossy(),
                    kind(f).to_owned(),
                    sha(&json(f, false).encode_utf16().collect::<Vec<u16>>()),
                ],
            ),
        );
    }
    out.line(
        &format!("{section}.duplicates"),
        &keyed(
            key,
            vec![
                d.duplicates().len().to_string(),
                join(d.duplicates().iter().map(JStr::lossy)),
            ],
        ),
    );
    let mut fqns: BTreeSet<JStr> = input.iter().map(|f| f.qualified_name().clone()).collect();
    for t in ["", "my::pkg::nothing", "toUpper"] {
        fqns.insert(JStr::from(t));
    }
    for fqn in &fqns {
        let ids: Vec<String> = d.at(fqn).iter().map(|f| FunctionId::of(f).qualified.lossy()).collect();
        out.line(
            &format!("{section}.at"),
            &keyed(key, vec![quoted(fqn), ids.len().to_string(), join(ids)]),
        );
    }
    let unknown = "no::such::function_Integer_1__Integer_1_";
    out.line(
        &format!("{section}.get"),
        &keyed(
            key,
            vec![
                unknown.to_owned(),
                b(d.get(&FunctionId::new(JStr::from(unknown))).is_none()),
            ],
        ),
    );
}

// ---------------------------------------------------------------- the implementations

/// The platform's registrations, as the lowering assembles them but for its own: no lowering keys, no feature
/// overrides, no members. The walled natives and each form's FQNs are handed sorted (Java's are hash-ordered).
fn platform_registrations() -> Registrations {
    let mut families: LinkedMap<Family, Vec<Arc<NativeFunctionDefinition>>> = LinkedMap::new();
    for f in Family::VALUES {
        for (_, _, overloads) in f.members() {
            families
                .get_or_insert_with(f, Vec::new)
                .extend(overloads.iter().map(|n| n.def().clone()));
        }
    }
    let mut forms: LinkedMap<CoreFn, Vec<JStr>> = LinkedMap::new();
    for f in CoreFn::VALUES {
        if !f.owned_fqns().is_empty() {
            let mut owned: Vec<&str> = f.owned_fqns().to_vec();
            owned.sort();
            forms.put(f, owned.into_iter().map(JStr::from).collect());
        }
    }
    let mut walled: Vec<(&str, &str)> = WALLED_NATIVES.to_vec();
    walled.sort();
    Registrations {
        catalog: catalog().all().to_vec(),
        families,
        forms,
        walled_natives: LinkedMap::of(walled.into_iter().map(|(f, r)| (JStr::from(f), JStr::from(r)))),
        walled_bodies: walled_bodies::reasons().clone(),
        subsumed: Subsumed::VALUES.iter().map(|s| JStr::from(s.fqn())).collect(),
        platform_pure: platform_pure::ids(),
        upstream_bodies: platform_pure::upstream_bodies().clone(),
        refused_versions: platform_pure::refused_versions().clone(),
        ..Registrations::default()
    }
}

fn names<T: Copy>(values: &[T], name: impl Fn(T) -> &'static str) -> String {
    values.iter().map(|&v| name(v)).collect::<Vec<_>>().join(",")
}

/// A row: its kind, then its fields.
fn row(r: &Implementation) -> Vec<String> {
    match r {
        Implementation::Form(f) => vec![
            "Form".to_owned(),
            f.form().name().to_owned(),
            names(f.also_lowered(), Position::name),
            names(f.also_families(), Family::name),
        ],
        Implementation::Intrinsic(n) => vec![
            "Intrinsic".to_owned(),
            names(n.positions(), Position::name),
            names(n.feature_overrides(), Feature::name),
            names(n.families(), Family::name),
        ],
        Implementation::Body | Implementation::PlatformPure | Implementation::Unimplemented => {
            vec![r.kind().to_owned()]
        }
        Implementation::Refused(x) => vec!["Refused".to_owned(), x.reason.name().to_owned(), x.why.lossy()],
    }
}

fn table_section(out: &mut Out, section: &str, key: Option<&str>, t: &ImplementationTable, d: &DeclarationTable) {
    out.line(
        &format!("{section}.size"),
        &keyed(key, vec![t.rows().len().to_string()]),
    );
    for (i, (id, r)) in t.rows().iter().enumerate() {
        let mut fields = vec![
            i.to_string(),
            id.qualified.lossy(),
            b(t.runs_by_rule(d.get(id))),
            // Java asks whether `of` answers the row itself (`==`); the record's equality asks the same here
            b(t.of(id) == Some(r)),
        ];
        fields.extend(row(r));
        out.line(&format!("{section}.row"), &keyed(key, fields));
    }
    list(out, &format!("{section}.dangling"), key, t.dangling());
    list(out, &format!("{section}.conflicts"), key, t.conflicts());
    list(out, &format!("{section}.memberWalls"), key, t.member_walls());
}

fn list(out: &mut Out, section: &str, key: Option<&str>, values: &[JStr]) {
    out.line(&format!("{section}.count"), &keyed(key, vec![values.len().to_string()]));
    for (i, v) in values.iter().enumerate() {
        out.line(section, &keyed(key, vec![i.to_string(), v.lossy()]));
    }
}

/// rowOf and runsByRule for declarations outside the table, and for none.
fn outside(out: &mut Out, t: &ImplementationTable) {
    let fd = declare("function my::pkg::outside(x:Integer[1]):Integer[1] { $x }").remove(0);
    let upper = def(UPPER_ID);
    let nfd = Function::NativeFunctionDefinition(Arc::new(NativeFunctionDefinition {
        qualified_name: JStr::from("my::pkg::outsideNative"),
        ..(*upper).clone()
    }));
    for f in [fd, nfd] {
        let id = FunctionId::of(&f);
        let mut fields = vec![
            kind(&f).to_owned(),
            id.qualified.lossy(),
            b(t.of(&id).is_none()),
            b(t.runs_by_rule(Some(&f))),
        ];
        fields.extend(row(t.row_of(&f)));
        out.line("platform.table.outside", &fields);
    }
    put!(out, "platform.table.outside", "null", b(t.runs_by_rule(None)));
}

// ---------------------------------------------------------------- the tests' cases

fn def(id: &str) -> Arc<NativeFunctionDefinition> {
    let n: NativeId = catalog()
        .native_function_by_id(&JStr::from(id))
        .unwrap_or_else(|| panic!("{id}"));
    n.def().clone()
}

/// The functions of a text at the engine's dialect (the tests' `Engine.model`).
fn declare(source: &str) -> Vec<Function> {
    let units: Vec<u16> = source.encode_utf16().collect();
    lite_builtin::door::parse(&units, Dialect::LegendEngine)
        .unwrap_or_else(|e| panic!("{source}: {e:?}"))
        .elements
        .iter()
        .filter_map(|e| e.as_function())
        .collect()
}

fn concat(a: Vec<Function>, b: Vec<Function>) -> Vec<Function> {
    a.into_iter().chain(b).collect()
}

fn declaration_cases(out: &mut Out) {
    let upper = native(&def(UPPER_ID));
    let bodied = "function meta::pure::functions::string::toUpper(source:String[1]):String[1] { $source }";
    let one = "function my::pkg::one(x:Integer[1]):Integer[1] { $x }";
    let cases: Vec<(&str, Vec<Function>)> = vec![
        ("nativeThenBodied", concat(vec![upper.clone()], declare(bodied))),
        ("bodiedThenNative", concat(declare(bodied), vec![upper.clone()])),
        ("nativeTwice", vec![upper.clone(), upper.clone()]),
        (
            "overloads",
            declare(
                "function my::pkg::twice(x:Integer[1]):Integer[1] { $x }\n\
                 function my::pkg::twice(x:String[1]):String[1] { $x }",
            ),
        ),
        ("sameBodyTwice", concat(declare(one), declare(one))),
        (
            "twoBodies",
            concat(
                concat(
                    declare(one),
                    declare("function my::pkg::one(x:Integer[1]):Integer[1] { 1 }"),
                ),
                declare("function my::pkg::one(x:Integer[1]):Integer[1] { 2 }"),
            ),
        ),
        ("empty", vec![]),
    ];
    for (key, declared) in cases {
        let d = DeclarationTable::of(declared.iter().cloned());
        declarations_section(out, "platform.case.declarations", Some(key), &declared, &d);
    }
}

fn id(f: &Function) -> FunctionId {
    FunctionId::of(f)
}

fn unknown(qualified: &str) -> FunctionId {
    FunctionId::new(JStr::from(qualified))
}

fn fqn(f: &Function) -> JStr {
    f.qualified_name().clone()
}

fn j(s: &str) -> JStr {
    JStr::from(s)
}

/// The one function of `source`.
fn fd(source: &str) -> Function {
    declare(source).remove(0)
}

/// `fd.withSynthesizedFrom(new Synthesized(hat, owner, member))`.
fn synthesized(f: Function, hat: SynthHat, owner: &str, member: &str) -> Function {
    let Function::FunctionDefinition(f) = f else {
        panic!("a body")
    };
    Function::FunctionDefinition(Arc::new(FunctionDefinition {
        synthesized_from: Some(Arc::new(FunctionDefinitionSynthesized {
            hat,
            owner_fqn: j(owner),
            member_name: j(member),
            forward_property: None,
        })),
        ..(*f).clone()
    }))
}

fn implementation_cases(out: &mut Out) {
    let (upper_def, lower_def, trim_def, length_def) = (def(UPPER_ID), def(LOWER_ID), def(TRIM_ID), def(LENGTH_ID));
    let (upper, lower, trim) = (native(&upper_def), native(&lower_def), native(&trim_def));
    let only = Registrations::of_catalog;

    // ImplementationTableTest's cases, each map in the order the test writes it
    let mut cases: Vec<(&str, Vec<Function>, Registrations)> = vec![(
        "defaults",
        concat(
            vec![upper.clone()],
            declare("function my::pkg::twice(x:Integer[1]):Integer[1] { $x }"),
        ),
        only(vec![upper_def.clone()]),
    )];
    cases.push((
        "loweringKey",
        vec![upper.clone()],
        Registrations {
            lowering_keys: LinkedMap::of([(Position::SCALAR, vec![id(&upper)])]),
            ..only(vec![upper_def.clone()])
        },
    ));
    cases.push((
        "danglingKey",
        vec![upper.clone()],
        Registrations {
            lowering_keys: LinkedMap::of([(
                Position::SCALAR,
                vec![unknown("no::such::function_String_1__String_1_")],
            )]),
            ..only(vec![upper_def.clone()])
        },
    ));
    cases.push((
        "familyAndForm",
        vec![upper.clone(), lower.clone()],
        Registrations {
            families: LinkedMap::of([(Family::Verdict, vec![upper_def.clone()])]),
            forms: LinkedMap::of([(CoreFn::FILTER, vec![fqn(&lower)])]),
            ..only(vec![upper_def.clone(), lower_def.clone()])
        },
    ));
    cases.push((
        "formWithRule",
        vec![upper.clone()],
        Registrations {
            lowering_keys: LinkedMap::of([(Position::SCALAR, vec![id(&upper)])]),
            forms: LinkedMap::of([(CoreFn::MAP, vec![fqn(&upper)])]),
            ..only(vec![upper_def.clone()])
        },
    ));
    cases.push((
        "refusalBesideImplementation",
        vec![upper.clone()],
        Registrations {
            lowering_keys: LinkedMap::of([(Position::SCALAR, vec![id(&upper)])]),
            walled_natives: LinkedMap::of([(fqn(&upper), j("an effect"))]),
            ..only(vec![upper_def.clone()])
        },
    ));
    cases.push((
        "twoForms",
        vec![upper.clone()],
        Registrations {
            forms: LinkedMap::of([(CoreFn::MAP, vec![fqn(&upper)]), (CoreFn::FILTER, vec![fqn(&upper)])]),
            ..only(vec![upper_def.clone()])
        },
    ));
    cases.push((
        "refusalKinds",
        concat(
            vec![upper.clone()],
            declare("function my::pkg::printer(x:Integer[1]):Integer[1] { $x }"),
        ),
        Registrations {
            walled_natives: LinkedMap::of([(fqn(&upper), j("an effect"))]),
            walled_bodies: LinkedMap::of([
                (j("my::pkg::printer"), Wall::new(Kind::ENGINE_MACHINERY, "the printer")),
                (j("my::pkg::Cls$prop$derived"), Wall::new(Kind::CANNOT_IMPLEMENT, "x")),
            ]),
            ..only(vec![upper_def.clone()])
        },
    ));
    cases.push((
        "subsumedDangling",
        vec![upper.clone()],
        Registrations {
            subsumed: vec![j("no::such::program")],
            ..only(vec![upper_def.clone()])
        },
    ));

    // the members a family implements, matched by a lifted declaration's provenance
    let derived = synthesized(
        fd("function my::pkg::derived(x:Integer[1]):Integer[1] { $x }"),
        SynthHat::PROP,
        "my::pkg::Cls",
        "derived",
    );
    let other = synthesized(
        fd("function my::pkg::other(x:Integer[1]):Integer[1] { $x }"),
        SynthHat::PROP,
        "my::pkg::Cls",
        "other",
    );
    let constraint = synthesized(
        fd("function my::pkg::constraint(x:Integer[1]):Integer[1] { $x }"),
        SynthHat::CONSTRAINT,
        "my::pkg::Cls",
        "derived",
    );
    let plain = fd("function my::pkg::plain(x:Integer[1]):Integer[1] { $x }");
    let member = |name: &str| ClassMember {
        owner: j("my::pkg::Cls"),
        name: j(name),
    };
    cases.push((
        "members",
        vec![derived, other, constraint, plain],
        Registrations {
            members: LinkedMap::of([
                (Family::JavaRoutine, vec![member("derived")]),
                (Family::RowGetter, vec![member("derived"), member("other")]),
            ]),
            ..only(vec![])
        },
    ));

    // every conflict and every dangling registration the builder reports
    let own = fd("function my::pkg::own(x:Integer[1]):Integer[1] { $x }");
    let own_other = fd("function my::pkg::own(x:String[1]):String[1] { $x }");
    let formed = fd("function my::pkg::formed(x:Integer[1]):Integer[1] { $x }");
    let walled = fd("function my::pkg::walled(x:Integer[1]):Integer[1] { $x }");
    let lowered = fd("function my::pkg::lowered(x:Integer[1]):Integer[1] { $x }");
    let refused_version = fd("function my::pkg::refusedVersion(x:Integer[1]):Integer[1] { $x }");
    let form_refused = fd("function my::pkg::formRefused(x:Integer[1]):Integer[1] { $x }");
    let upper_version =
        fd("function meta::pure::functions::string::toUpper(source:String[1], n:Integer[1]):String[1] { $source }");
    let lower_version =
        fd("function meta::pure::functions::string::toLower(source:String[1], n:Integer[1]):String[1] { $source }");
    let own_body = fd("function my::pkg::ownBody(x:Integer[1]):Integer[1] { $x }");
    let form_own = fd("function my::pkg::formOwn(x:Integer[1]):Integer[1] { $x }");
    let refused_body = fd("function my::pkg::refusedBody(x:Integer[1]):Integer[1] { $x }");
    let registrations = Registrations {
        catalog: vec![
            upper_def.clone(),
            lower_def.clone(),
            trim_def.clone(),
            length_def.clone(),
        ],
        lowering_keys: LinkedMap::of([
            (Position::AGGREGATE, vec![id(&upper), id(&lowered)]),
            (
                Position::SCALAR,
                vec![id(&upper), id(&own), unknown("no::such::scalar_Integer_1__Integer_1_")],
            ),
        ]),
        feature_overrides: LinkedMap::of([
            (
                Feature::CORRECT_SQL_SUBSTRING_INDEXING,
                vec![id(&upper), unknown("no::such::feature_Integer_1__Integer_1_")],
            ),
            (Feature::LEGACY_SQL_NULL_UNSAFE_EQUALS, vec![id(&lower)]),
        ]),
        families: LinkedMap::of([
            (
                Family::Verdict,
                vec![upper_def.clone(), trim_def.clone(), length_def.clone()],
            ),
            (Family::Calendar, vec![upper_def.clone()]),
        ]),
        forms: LinkedMap::of([
            (
                CoreFn::FILTER,
                vec![j("my::pkg::formed"), j("my::pkg::nowhere"), j("my::pkg::formOwn")],
            ),
            (CoreFn::MAP, vec![j("my::pkg::formRefused")]),
        ]),
        walled_natives: LinkedMap::of([(fqn(&trim), j("an effect")), (j("my::pkg::noNative"), j("none"))]),
        walled_bodies: LinkedMap::of([
            (j("my::pkg::walled"), Wall::new(Kind::ENGINE_MACHINERY, "the printer")),
            (j("my::pkg::Cls$prop$m"), Wall::new(Kind::CANNOT_IMPLEMENT, "member")),
        ]),
        subsumed: vec![
            j("my::pkg::formRefused"),
            j("no::such::program"),
            j("my::pkg::refusedBody"),
        ],
        members: LinkedMap::new(),
        platform_pure: vec![
            id(&own),
            id(&walled),
            id(&upper),
            unknown("no::such::pure_Integer_1__Integer_1_"),
            id(&own_body),
            id(&form_own),
        ],
        upstream_bodies: LinkedMap::of([
            (id(&lower_version), j("decided")),
            (id(&formed), j("formed")),
            (id(&lowered), j("lowered")),
            (id(&upper), j("native")),
            (unknown("no::such::body_Integer_1__Integer_1_"), j("none")),
            (id(&own_body), j("own")),
            (id(&refused_body), j("refused")),
        ]),
        refused_versions: LinkedMap::of([
            (id(&refused_version), Wall::new(Kind::CANNOT_IMPLEMENT, "refused")),
            (
                unknown("no::such::version_Integer_1__Integer_1_"),
                Wall::new(Kind::ENGINE_MACHINERY, "none"),
            ),
        ]),
    };
    let declared = vec![
        upper,
        lower,
        trim,
        own,
        own_other,
        formed,
        walled,
        lowered,
        refused_version,
        form_refused,
        upper_version,
        lower_version,
        own_body,
        form_own,
        refused_body,
    ];
    cases.push(("conflicts", declared, registrations));

    for (key, declared, registrations) in cases {
        let d = DeclarationTable::of(declared);
        table_section(
            out,
            "platform.case",
            Some(key),
            &ImplementationTable::build(&d, &registrations),
            &d,
        );
    }
}
