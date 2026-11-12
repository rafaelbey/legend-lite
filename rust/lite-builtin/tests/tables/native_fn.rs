// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! The native function families' section (Java's `NativeFnTables`): `NativeFn`, line for line as Java writes it --
//! every family's members and what an applied-name probe set they match, the instance answers, the indexes, every
//! `of(id)` and id predicate over every catalog id, `claims` and the by-name lookups, the constants, `OperatorRun`.

use std::collections::{BTreeMap, BTreeSet};

use lite_builtin::native_fn::{
    Calendar, Carrier, ContextOption, ContextOwner, DdlStatement, EXECUTE_ROW_OWNER, Effect, Family, FetchDbGrid,
    Frame, GroupAggregate, Handle, JavaRoutine, LiteDesugar, LiteralForm, LowererForm, Member, ObjectReference,
    OperatorRun, PlanWrapper, RelationQuantifier, ResolverForm, RowGetter, SubtypeForm, TDS_ROW_OWNER, TyperForm,
    Verdict, claims, index, index_by_id,
};
use lite_builtin::pure::{NativeId, catalog};
use lite_model::{FunctionId, JStr};

use super::support::{Out, b, join, json};

/// The overloads by their constants' names, then by their identities.
fn overloads(o: &[NativeId]) -> [String; 2] {
    [
        join(o.iter().map(|n| n.name())),
        join(o.iter().map(|n| n.id().qualified.lossy())),
    ]
}

fn name<M: Member>(m: Option<M>) -> String {
    m.map_or_else(|| "-".to_owned(), |m| m.name().to_owned())
}

/// The members of one family, what each matches of the probes, and the family's indexes; its rows held to
/// `Family::members()`.
struct FamilyLines {
    members: Vec<String>,
    index: Vec<String>,
}

fn family_lines<M: Member + PartialEq>(fam: Family, values: &'static [M], probes: &BTreeSet<String>) -> FamilyLines {
    let rows: Vec<_> = values.iter().map(|&m| (m.name(), m.fqn(), m.overloads())).collect();
    assert_eq!(fam.members(), rows, "Family::{fam:?}.members() is its enum's values");
    let mut out = Out::default();
    let f = fam.name().to_owned();
    out.line("nativefn.family", &[f.clone(), values.len().to_string()]);
    for (i, &m) in values.iter().enumerate() {
        let [names, ids] = overloads(m.overloads());
        out.line(
            "nativefn.member",
            &[
                f.clone(),
                i.to_string(),
                m.name().to_owned(),
                m.fqn().to_owned(),
                m.bare_name().to_owned(),
                names,
                ids,
            ],
        );
        out.line(
            "nativefn.matches",
            &[
                f.clone(),
                m.name().to_owned(),
                join(probes.iter().filter(|p| m.matches(p))),
            ],
        );
    }
    let members = out.lines;
    let mut out = Out::default();
    let by_fqn: BTreeMap<&str, M> = index(values).into_iter().collect();
    for (fqn, m) in by_fqn {
        out.line("nativefn.index", &[f.clone(), fqn.to_owned(), m.name().to_owned()]);
    }
    let by_id: BTreeMap<String, M> = index_by_id(values)
        .into_iter()
        .map(|(id, m)| (id.qualified.lossy(), m))
        .collect();
    out.line("nativefn.indexById.size", &[f.clone(), by_id.len().to_string()]);
    for (id, m) in by_id {
        out.line("nativefn.indexById", &[f.clone(), id, m.name().to_owned()]);
    }
    FamilyLines {
        members,
        index: out.lines,
    }
}

/// Every family's lines, in `families()`'s order.
fn families(probes: &BTreeSet<String>) -> Vec<FamilyLines> {
    Family::VALUES
        .iter()
        .map(|&fam| match fam {
            Family::Calendar => family_lines(fam, Calendar::VALUES, probes),
            Family::Verdict => family_lines(fam, Verdict::VALUES, probes),
            Family::RowGetter => family_lines(fam, RowGetter::VALUES, probes),
            Family::Frame => family_lines(fam, Frame::VALUES, probes),
            Family::LowererForm => family_lines(fam, LowererForm::VALUES, probes),
            Family::LiteralForm => family_lines(fam, LiteralForm::VALUES, probes),
            Family::ContextOption => family_lines(fam, ContextOption::VALUES, probes),
            Family::PlanWrapper => family_lines(fam, PlanWrapper::VALUES, probes),
            Family::RelationQuantifier => family_lines(fam, RelationQuantifier::VALUES, probes),
            Family::GroupAggregate => family_lines(fam, GroupAggregate::VALUES, probes),
            Family::ObjectReference => family_lines(fam, ObjectReference::VALUES, probes),
            Family::SubtypeForm => family_lines(fam, SubtypeForm::VALUES, probes),
            Family::ResolverForm => family_lines(fam, ResolverForm::VALUES, probes),
            Family::LiteDesugar => family_lines(fam, LiteDesugar::VALUES, probes),
            Family::TyperForm => family_lines(fam, TyperForm::VALUES, probes),
            Family::JavaRoutine => family_lines(fam, JavaRoutine::VALUES, probes),
            Family::Handle => family_lines(fam, Handle::VALUES, probes),
            Family::Effect => family_lines(fam, Effect::VALUES, probes),
            Family::Carrier => family_lines(fam, Carrier::VALUES, probes),
            Family::ContextOwner => family_lines(fam, ContextOwner::VALUES, probes),
            Family::DdlStatement => family_lines(fam, DdlStatement::VALUES, probes),
        })
        .collect()
}

/// The `of.columns` order: every family's `of(id)` (RowGetter and GroupAggregate have none), then `OperatorRun.of`.
const OF_COLUMNS: [&str; 20] = [
    "Calendar",
    "Verdict",
    "Frame",
    "LowererForm",
    "LiteralForm",
    "ContextOption",
    "PlanWrapper",
    "RelationQuantifier",
    "ObjectReference",
    "SubtypeForm",
    "ResolverForm",
    "LiteDesugar",
    "TyperForm",
    "JavaRoutine",
    "Handle",
    "Effect",
    "Carrier",
    "ContextOwner",
    "DdlStatement",
    "OperatorRun",
];

/// One id's line: every family's `of`, then `isBivariateMap` (not nullable), then the predicates.
fn by_id(out: &mut Out, section: &str, key: String, id: Option<&FunctionId>) {
    let mut fields = vec![
        key,
        name(Calendar::of(id)),
        name(Verdict::of(id)),
        name(Frame::of(id)),
        name(LowererForm::of(id)),
        name(LiteralForm::of(id)),
        name(ContextOption::of(id)),
        name(PlanWrapper::of(id)),
        name(RelationQuantifier::of(id)),
        name(ObjectReference::of(id)),
        name(SubtypeForm::of(id)),
        name(ResolverForm::of(id)),
        name(LiteDesugar::of(id)),
        name(TyperForm::of(id)),
        name(JavaRoutine::of(id)),
        name(Handle::of(id)),
        name(Effect::of(id)),
        name(Carrier::of(id)),
        name(ContextOwner::of(id)),
        name(DdlStatement::of(id)),
        OperatorRun::of(id).map_or_else(|| "-".to_owned(), |r| r.name().to_owned()),
    ];
    if let Some(id) = id {
        fields.push(b(LowererForm::is_bivariate_map(id)));
    }
    fields.push(b(Handle::is_execute(id)));
    fields.push(b(Handle::runs_a_frame(id)));
    fields.push(b(Handle::forces_at_value_position(id)));
    for n in 0..=4 {
        fields.push(Handle::orchestration_argument(id, n).to_string());
    }
    fields.push(b(Effect::is_db_effect(id)));
    fields.push(b(Effect::is_seed_sql_form(id)));
    fields.push(b(Effect::is_inert_diagnostic(id)));
    fields.push(b(Effect::reads_arguments_at_planning(id)));
    for k in 0..=3 {
        fields.push(b(Effect::argument_read_at_planning(id, k)));
    }
    fields.push(Carrier::fetch_db_grid(id).map_or_else(|| "-".to_owned(), |g| g.name().to_owned()));
    out.line(section, &fields);
}

pub fn dump(out: &mut Out) {
    out.line(
        "nativefn.families",
        &[Family::VALUES.iter().map(|f| f.name()).collect::<Vec<_>>().join(" ")],
    );
    // ---- the applied-name probes: every member's FQN and bare name, qualified elsewhere, and a near miss
    let mut probes = BTreeSet::new();
    for f in Family::VALUES {
        for (_, fqn, _) in f.members() {
            let bare = &fqn[fqn.rfind(':').map_or(0, |i| i + 1)..];
            probes.insert(fqn.to_owned());
            probes.insert(bare.to_owned());
            probes.insert(format!("x::{bare}"));
            probes.insert(format!("x{bare}"));
        }
    }
    // ---- the families: each member, what it matches, and its instance answers
    let fams = families(&probes);
    for f in &fams {
        out.lines.extend(f.members.iter().cloned());
    }
    for &r in ResolverForm::VALUES {
        out.line("nativefn.resolverForm", &[r.name().to_owned(), b(r.class_write())]);
    }
    for &r in JavaRoutine::VALUES {
        out.line(
            "nativefn.javaRoutine",
            &[
                r.name().to_owned(),
                r.implemented_derived().unwrap_or("-").to_owned(),
                r.implemented_member()
                    .map_or_else(|| "-".to_owned(), |c| json(&c, true)),
            ],
        );
    }
    for &g in RowGetter::VALUES {
        out.line(
            "nativefn.rowGetter",
            &[
                g.name().to_owned(),
                g.owner().to_owned(),
                g.property().to_owned(),
                b(g.typed_cell()),
                json(&g.member(), true),
                g.overloads().len().to_string(),
            ],
        );
    }
    out.line(
        "nativefn.fetchDbGrid",
        &[join(FetchDbGrid::VALUES.iter().map(|g| g.name()))],
    );
    // ---- the indexes of every family, in key order
    for f in &fams {
        out.lines.extend(f.index.iter().cloned());
    }
    // ---- every family's of(id) and every id predicate, over every catalog id, the null id and an unknown one
    out.line("nativefn.of.columns", &[OF_COLUMNS.join(" ")]);
    let cat = catalog();
    for n in NativeId::all() {
        by_id(out, "nativefn.of", n.id().qualified.lossy(), Some(n.id()));
    }
    by_id(out, "nativefn.of.null", "null".to_owned(), None);
    let unknown = FunctionId {
        qualified: JStr::from("meta::pure::functions::math::plus"),
    };
    by_id(out, "nativefn.of.unknown", unknown.qualified.lossy(), Some(&unknown));
    // ---- claims and the by-name lookups, over every catalog FQN, every member's spellings and the edge texts
    let mut texts = probes.clone();
    for d in cat.all() {
        texts.insert(d.qualified_name.lossy());
    }
    for c in cat.native_classes() {
        texts.insert(c.qualified_name.lossy());
    }
    for &g in RowGetter::VALUES {
        texts.insert(g.owner().to_owned());
        texts.insert(g.property().to_owned());
        texts.insert(format!("a::b::{}", g.property()));
        texts.insert(format!("{}$prop$", g.owner()));
        texts.insert(format!("{}$prop$x", g.fqn()));
    }
    for &r in JavaRoutine::VALUES {
        if let Some(d) = r.implemented_derived() {
            texts.insert(d.to_owned());
        }
    }
    for t in ["", "::", ":::get", "get::", "$prop$", "$prop$get", "x$prop$"] {
        texts.insert(t.to_owned());
    }
    for t in &texts {
        out.line(
            "nativefn.byName",
            &[
                t.clone(),
                b(claims(t)),
                name(RowGetter::of(Some(t))),
                name(RowGetter::of_lifted(Some(t))),
                b(RowGetter::is_owner(t)),
                name(JavaRoutine::of_derived(Some(t))),
            ],
        );
    }
    out.line(
        "nativefn.byName.null",
        &[
            name(RowGetter::of(None)),
            name(RowGetter::of_lifted(None)),
            name(JavaRoutine::of_derived(None)),
        ],
    );
    // ---- the string constants, by name
    out.line(
        "nativefn.string",
        &["EXECUTE_ROW_OWNER".to_owned(), EXECUTE_ROW_OWNER.to_owned()],
    );
    out.line(
        "nativefn.string",
        &["TDS_ROW_OWNER".to_owned(), TDS_ROW_OWNER.to_owned()],
    );
    // ---- the operator runs (no family: a classification the typer reads)
    for (i, &r) in OperatorRun::VALUES.iter().enumerate() {
        let [names, ids] = overloads(r.overloads());
        out.line(
            "nativefn.operatorRun",
            &[i.to_string(), r.name().to_owned(), r.fqn().to_owned(), names, ids],
        );
    }
}
