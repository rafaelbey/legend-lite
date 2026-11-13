// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! THE RUST PORT'S GUARDS (docs/rust-port/PHASE3_CRATES.md §3, 2026-11-11).
//!
//! 1. The crate graph: every crate's dependencies are exactly its line of `rust/lite-guards/crate-layers.txt`, in its
//!    Cargo.toml (both builds) and in Bazel's own answer (a genquery per library, Bazel only), and every edge of that
//!    file is one tools/deps/core-layers.txt allows. Cargo alone checks the workspace's member list against it.
//! 2. The arena ruling's two source rules (PHASE3_DESIGN.md §1.6 item 6; §1.5 item 3): `unsafe` is written only in
//!    lite-base's `src/arena.rs` (and lite-ffi, the C library's boundary), every other crate root forbids it; and a
//!    pointer becomes an address (`as_ptr`, `{:p}`, `ptr::eq`, `Arc::ptr_eq`, `addr`) only in lite-base's
//!    `src/identity.rs`, whose maps and sets have no iteration.
//!
//! 3. The one place a call is built (2026-11-12, the names follow-up; Java's `ArchitectureTest.callsAreBuiltOnlyByTheBuilder`
//!    in Rust): outside the parser's crate (lite-front: the parser and the protocol readers build calls before
//!    resolution), a call record (`AppliedFunction`) is constructed only by lite-names' `Calls` (`src/calls.rs`) and by
//!    the resolver, which records a parsed call's names (`src/resolver/values.rs`, as Java's rule exempts
//!    `NameResolver`); nothing else names lite-front's call factories. Production sources only (`src/`), as Java's
//!    rule checks the production classes alone.
//!
//! Bazel hands the files (`LITE_GUARD_FILES`) and the genqueries (`LITE_CRATE_DEPS`); cargo reads the tree. A comment
//! line (`//`, `//!`, `///`) is prose and is not scanned.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

const POLICY: &str = "rust/lite-guards/crate-layers.txt";
const LAYERS: &str = "tools/deps/core-layers.txt";

/// The crates whose sources Bazel does not hand this test, each with why; cargo scans them still. None: lite-front's,
/// the one there was (its BUILD file the source-tree flip's, in flight), was lifted on 2026-11-11 with the flip landed,
/// its sources handed and its root forbidding unsafe code (docs/rust-port/PHASE3.md step 1).
const CARGO_ONLY: &[(&str, &str)] = &[];

/// Where a pointer may become an address besides lite-base's identity module, each with why.
const ADDRESS_ALLOWED: &[(&str, usize, &str)] = &[
    (
        "rust/lite-dialect/src/java_time.rs",
        1,
        "a test that two zone ids share one rule set (`ptr::eq`), not identity",
    ),
    // lite-ffi: the C library's boundary, a C string's pointer handed to an export (its whole crate, below)
];

const ADDRESS_TOKENS: &[&str] = &[
    "as_ptr",
    ":p}",
    "ptr::eq",
    "ptr_eq",
    "addr_eq",
    ".addr()",
    "expose_provenance",
];

/// The repository's files this test reads, by repository-relative path.
struct Tree {
    files: BTreeMap<String, String>,
    /// Bazel's answer per crate: its library's direct dependencies, as crates; none under cargo
    bazel_deps: Option<BTreeMap<String, BTreeSet<String>>>,
}

fn tree() -> Tree {
    match std::env::var("LITE_GUARD_FILES") {
        Ok(listed) => {
            let mut files = BTreeMap::new();
            for path in listed.split_whitespace() {
                files.insert(path.to_owned(), read(Path::new(path)));
            }
            let mut deps = BTreeMap::new();
            for query in std::env::var("LITE_CRATE_DEPS")
                .expect("LITE_CRATE_DEPS beside LITE_GUARD_FILES")
                .split_whitespace()
            {
                let name = query
                    .rsplit('/')
                    .next()
                    .unwrap()
                    .strip_prefix("deps_")
                    .expect("a deps_<crate> query");
                let labels: BTreeSet<String> = read(Path::new(query))
                    .lines()
                    .filter(|l| !l.is_empty())
                    .map(crate_of_label)
                    .collect();
                deps.insert(name.to_owned(), labels);
            }
            Tree {
                files,
                bazel_deps: Some(deps),
            }
        }
        Err(_) => {
            let root = Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../..")
                .canonicalize()
                .expect("the repository");
            let mut files = BTreeMap::new();
            for path in ["rust/Cargo.toml", POLICY, LAYERS] {
                files.insert(path.to_owned(), read(&root.join(path)));
            }
            for entry in std::fs::read_dir(root.join("rust")).expect("rust/") {
                let dir = entry.expect("an entry").path();
                if dir.join("Cargo.toml").is_file() {
                    let name = dir.file_name().unwrap().to_str().unwrap().to_owned();
                    files.insert(format!("rust/{name}/Cargo.toml"), read(&dir.join("Cargo.toml")));
                    for sub in ["src", "tests"] {
                        walk(&root, &dir.join(sub), &mut files);
                    }
                }
            }
            Tree {
                files,
                bazel_deps: None,
            }
        }
    }
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

fn walk(root: &Path, dir: &Path, files: &mut BTreeMap<String, String>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    let mut paths: Vec<PathBuf> = entries.map(|e| e.expect("an entry").path()).collect();
    paths.sort();
    for path in paths {
        if path.is_dir() {
            walk(root, &path, files);
        } else if path.extension().is_some_and(|e| e == "rs") {
            let relative = path.strip_prefix(root).unwrap().to_str().unwrap().replace('\\', "/");
            files.insert(relative, read(&path));
        }
    }
}

/// `//rust/lite-base:lite_base` (or `@@//rust/...`) is the crate `lite-base`: its package's directory.
fn crate_of_label(label: &str) -> String {
    let package = label
        .trim_start_matches('@')
        .trim_start_matches("//")
        .split(':')
        .next()
        .unwrap();
    package
        .strip_prefix("rust/")
        .unwrap_or_else(|| panic!("{label}: not a crate of rust/"))
        .to_owned()
}

/// One line of crate-layers.txt.
#[derive(Debug, Clone)]
struct Crate {
    layers: Vec<String>,
    phase: String,
    deps: BTreeSet<String>,
}

/// crate-layers.txt: its crates, and its splits (a layer to its parts, in order).
fn policy(text: &str) -> (BTreeMap<String, Crate>, BTreeMap<String, Vec<String>>) {
    let mut crates = BTreeMap::new();
    let mut splits = BTreeMap::new();
    for line in text.lines().filter(|l| !l.is_empty() && !l.starts_with('#')) {
        let fields: Vec<&str> = line.split('\t').collect();
        if fields[0] == "split" {
            splits.insert(fields[1].to_owned(), fields[2].split(' ').map(str::to_owned).collect());
            continue;
        }
        assert!(
            (3..=4).contains(&fields.len()),
            "{POLICY}: `{line}` is not crate, layers, phase[, deps]"
        );
        let deps = fields.get(3).map_or_else(BTreeSet::new, |d| {
            d.split(' ').filter(|x| !x.is_empty()).map(str::to_owned).collect()
        });
        let layers = fields[1].split(' ').map(str::to_owned).collect();
        let previous = crates.insert(
            fields[0].to_owned(),
            Crate {
                layers,
                phase: fields[2].to_owned(),
                deps,
            },
        );
        assert!(previous.is_none(), "{POLICY}: {} has two lines", fields[0]);
    }
    (crates, splits)
}

/// core-layers.txt: each layer to its direct dependencies; a name it lists only as a dependency is a layer with none.
fn layers(text: &str) -> BTreeMap<String, BTreeSet<String>> {
    let mut out: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for line in text.lines().filter(|l| !l.trim().is_empty() && !l.starts_with('#')) {
        let (name, deps) = line.split_once('\t').unwrap_or((line, ""));
        let deps: BTreeSet<String> = deps.split(' ').filter(|d| !d.is_empty()).map(str::to_owned).collect();
        for d in &deps {
            out.entry(d.clone()).or_default();
        }
        out.entry(name.trim().to_owned()).or_default().extend(deps);
    }
    out
}

/// A Cargo.toml's `[dependencies]`, by crate name.
fn cargo_deps(manifest: &str) -> BTreeSet<String> {
    let mut deps = BTreeSet::new();
    let mut inside = false;
    for line in manifest.lines().map(str::trim) {
        if line.starts_with('[') {
            inside = line == "[dependencies]";
        } else if inside && let Some((name, _)) = line.split_once('=') {
            deps.insert(name.trim().to_owned());
        }
    }
    deps
}

/// The workspace's members (rust/Cargo.toml).
fn members(manifest: &str) -> BTreeSet<String> {
    let start = manifest.find("members = [").expect("a members list") + "members = [".len();
    let end = start + manifest[start..].find(']').expect("the list's end");
    manifest[start..end]
        .split(',')
        .map(|m| m.trim().trim_matches('"'))
        .filter(|m| !m.is_empty())
        .map(str::to_owned)
        .collect()
}

/// Every edge the policy names that the layers do not allow.
fn disallowed_edges(
    crates: &BTreeMap<String, Crate>,
    splits: &BTreeMap<String, Vec<String>>,
    core: &BTreeMap<String, BTreeSet<String>>,
) -> Vec<String> {
    let mut problems = Vec::new();
    let known = |layer: &str| -> bool {
        match layer.split_once('/') {
            Some((l, part)) => {
                core.contains_key(l) && splits.get(l).is_some_and(|parts| parts.iter().any(|p| p == part))
            }
            None => core.contains_key(layer),
        }
    };
    for (name, c) in crates {
        for l in &c.layers {
            if l != "jdk" && l != "top" && !known(l) {
                problems.push(format!(
                    "{name} ports `{l}`, which is no layer of {LAYERS} (nor a declared part of one)"
                ));
            }
        }
        for dep in &c.deps {
            let Some(d) = crates.get(dep) else {
                problems.push(format!("{name} depends on {dep}, which has no line"));
                continue;
            };
            if d.layers == ["top"] {
                problems.push(format!("{name} depends on {dep}, which is `top`: nothing may"));
                continue;
            }
            if d.layers == ["jdk"] || c.layers == ["top"] {
                continue;
            }
            let allowed = c
                .layers
                .iter()
                .any(|lc| d.layers.iter().any(|ld| layer_reaches(lc, ld, splits, core)));
            if !allowed {
                problems.push(format!(
                    "{name} ({}) depends on {dep} ({}): no layer of the first lists a layer of the second in {LAYERS}",
                    c.layers.join(" "),
                    d.layers.join(" ")
                ));
            }
        }
    }
    problems
}

/// Whether layer `from` may reach layer `to`: the layers file lists it, or both are parts of one split layer and
/// `to` comes first.
fn layer_reaches(
    from: &str,
    to: &str,
    splits: &BTreeMap<String, Vec<String>>,
    core: &BTreeMap<String, BTreeSet<String>>,
) -> bool {
    let whole = |l: &str| l.split_once('/').map_or(l.to_owned(), |(w, _)| w.to_owned());
    let (wf, wt) = (whole(from), whole(to));
    if wf == wt {
        let (Some((_, pf)), Some((_, pt))) = (from.split_once('/'), to.split_once('/')) else {
            return false;
        };
        let order = &splits[&wf];
        let at = |p: &str| order.iter().position(|x| x == p);
        return matches!((at(pf), at(pt)), (Some(f), Some(t)) if t < f);
    }
    core.get(&wf).is_some_and(|deps| deps.contains(&wt))
}

#[test]
fn the_crate_graph_is_held_to_the_layers() {
    let tree = tree();
    let (crates, splits) = policy(&tree.files[POLICY]);
    let core = layers(&tree.files[LAYERS]);
    assert!(
        core.len() >= 40,
        "{LAYERS} has {} layers: the guard is not looking",
        core.len()
    );
    assert!(
        crates.len() >= 9,
        "{POLICY} has {} crates: the guard is not looking",
        crates.len()
    );
    let mut problems = disallowed_edges(&crates, &splits, &core);
    for (name, c) in &crates {
        if !["0", "1", "2", "3"].contains(&c.phase.as_str()) {
            problems.push(format!("{name}'s phase is `{}`", c.phase));
        }
        let manifest = format!("rust/{name}/Cargo.toml");
        match tree.files.get(&manifest) {
            Some(text) if cargo_deps(text) != c.deps => problems.push(format!(
                "{manifest} depends on {:?}; {POLICY} says {:?}",
                cargo_deps(text),
                c.deps
            )),
            Some(_) => {}
            None if tree.bazel_deps.is_some() && CARGO_ONLY.iter().any(|(n, _)| n == name) => {}
            None => problems.push(format!("{name} has a line and no {manifest}")),
        }
    }
    match &tree.bazel_deps {
        Some(bazel) => {
            for (name, deps) in bazel {
                match crates.get(name) {
                    Some(c) if &c.deps != deps => problems.push(format!(
                        "Bazel's {name} depends on {deps:?}; {POLICY} says {:?}",
                        c.deps
                    )),
                    Some(_) => {}
                    None => problems.push(format!(
                        "rust/lite-guards/BUILD.bazel queries {name}, which has no line"
                    )),
                }
            }
            // every crate with a library is queried: all but the `top` ones, and lite-ffi, the C library
            for (name, c) in &crates {
                let has_library = c.layers != ["top"] || name == "lite-ffi";
                if has_library && !bazel.contains_key(name) {
                    problems.push(format!(
                        "{name} has no deps_{name} query in rust/lite-guards/BUILD.bazel"
                    ));
                }
            }
        }
        None => {
            let members = members(&tree.files["rust/Cargo.toml"]);
            let listed: BTreeSet<String> = crates.keys().cloned().collect();
            if members != listed {
                problems.push(format!(
                    "rust/Cargo.toml's members are {members:?}; {POLICY} lists {listed:?}"
                ));
            }
        }
    }
    assert!(
        problems.is_empty(),
        "the crate graph is not the policy's:\n{}",
        problems.join("\n")
    );
}

#[test]
fn a_split_layers_parts_reach_only_the_parts_before_them() {
    // the rule F-not-G will stand on (PHASE3_CRATES.md §2), over a policy of its own
    let core = layers("base\t\ncompiler\tbase\nnormalizer\tbase compiler\n");
    let line = |layers: &str, deps: &str| Crate {
        layers: layers.split(' ').map(str::to_owned).collect(),
        phase: "3".to_owned(),
        deps: deps.split(' ').filter(|d| !d.is_empty()).map(str::to_owned).collect(),
    };
    let splits = BTreeMap::from([(
        "compiler".to_owned(),
        vec!["names".to_owned(), "element".to_owned(), "spec".to_owned()],
    )]);
    let mut crates = BTreeMap::from([
        ("b".to_owned(), line("base", "")),
        ("names".to_owned(), line("compiler/names", "b")),
        ("element".to_owned(), line("compiler/element", "names b")),
        ("spec".to_owned(), line("compiler/spec", "element names")),
        ("normalizer".to_owned(), line("normalizer", "spec names")),
    ]);
    assert_eq!(disallowed_edges(&crates, &splits, &core), Vec::<String>::new());
    crates.get_mut("element").unwrap().deps.insert("spec".to_owned());
    crates.get_mut("b").unwrap().deps.insert("names".to_owned());
    crates.insert("bogus".to_owned(), line("compiler/typer", ""));
    let problems = disallowed_edges(&crates, &splits, &core);
    assert_eq!(problems.len(), 3, "{problems:#?}");
    assert!(
        problems
            .iter()
            .any(|p| p.starts_with("element (compiler/element) depends on spec"))
    );
    assert!(problems.iter().any(|p| p.starts_with("b (base) depends on names")));
    assert!(problems.iter().any(|p| p.contains("`compiler/typer`")));
}

/// The crate a repository path is in, and the scanned lines of a source (comment lines left out).
fn crate_of_path(path: &str) -> Option<&str> {
    path.strip_prefix("rust/")?.split('/').next()
}

fn code_lines(text: &str) -> impl Iterator<Item = (usize, &str)> {
    text.lines()
        .enumerate()
        .filter(|(_, l)| !l.trim_start().starts_with("//"))
        .map(|(i, l)| (i + 1, l))
}

fn has_word(line: &str, word: &str) -> bool {
    let ident = |c: char| c.is_alphanumeric() || c == '_';
    line.match_indices(word).any(|(at, _)| {
        !line[..at].chars().next_back().is_some_and(ident) && !line[at + word.len()..].chars().next().is_some_and(ident)
    })
}

#[test]
fn unsafe_lives_in_the_arena_and_addresses_in_identity() {
    let tree = tree();
    let (crates, _) = policy(&tree.files[POLICY]);
    let mut problems = Vec::new();
    let mut scanned: BTreeSet<&str> = BTreeSet::new();
    let mut address_counts: BTreeMap<&str, usize> = BTreeMap::new();
    for (path, text) in tree.files.iter().filter(|(p, _)| p.ends_with(".rs")) {
        let Some(name) = crate_of_path(path) else { continue };
        scanned.insert(name);
        // lite-ffi is the C library's boundary, where both are its own; this file names what it looks for
        if name == "lite-ffi" || path == "rust/lite-guards/tests/guards.rs" {
            continue;
        }
        for (n, line) in code_lines(text) {
            if has_word(line, "unsafe") && path != "rust/lite-base/src/arena.rs" {
                problems.push(format!(
                    "{path}:{n}: `unsafe` outside lite-base's arena module: {}",
                    line.trim()
                ));
            }
            if path == "rust/lite-base/src/identity.rs" {
                continue;
            }
            for token in ADDRESS_TOKENS.iter().filter(|t| line.contains(**t)) {
                *address_counts.entry(path).or_default() += 1;
                if !ADDRESS_ALLOWED.iter().any(|(p, _, _)| p == path) {
                    problems.push(format!(
                        "{path}:{n}: `{token}` outside lite-base's identity module: {}",
                        line.trim()
                    ));
                }
            }
        }
    }
    for (path, most, why) in ADDRESS_ALLOWED {
        let seen = address_counts.get(path).copied().unwrap_or(0);
        if seen > *most {
            problems.push(format!(
                "{path}: {seen} address conversions where {most} are allowed ({why})"
            ));
        }
    }
    // every crate root forbids unsafe code but lite-ffi's and lite-base's, which denies it and allows its arena alone
    for name in crates.keys() {
        let Some(root) = tree.files.get(&format!("rust/{name}/src/lib.rs")) else {
            continue;
        };
        let code: Vec<&str> = code_lines(root).map(|(_, l)| l.trim()).collect();
        match name.as_str() {
            "lite-ffi" => {}
            "lite-base" => {
                let allows: Vec<usize> = (0..code.len())
                    .filter(|&i| code[i] == "#[allow(unsafe_code)]")
                    .collect();
                if !code.contains(&"#![deny(unsafe_code)]")
                    || !code.contains(&"#![deny(clippy::undocumented_unsafe_blocks)]")
                    || allows.len() != 1
                    || code.get(allows[0] + 1) != Some(&"pub mod arena;")
                {
                    problems.push("rust/lite-base/src/lib.rs: unsafe code denied, undocumented unsafe blocks denied, and one allow, on the arena module".to_owned());
                }
            }
            _ if CARGO_ONLY.iter().any(|(n, _)| n == name) => {}
            _ if !code.contains(&"#![forbid(unsafe_code)]") => {
                problems.push(format!("rust/{name}/src/lib.rs does not forbid unsafe code"))
            }
            _ => {}
        }
    }
    // identity's maps and sets have no iteration: the order of an address hash is never seen
    let identity = &tree.files["rust/lite-base/src/identity.rs"];
    for api in [
        "fn iter",
        "fn keys",
        "fn values",
        "fn drain",
        "fn into_iter",
        "fn for_each",
        "IntoIterator for",
        "fn retain",
    ] {
        if identity.contains(api) {
            problems.push(format!(
                "rust/lite-base/src/identity.rs: `{api}`: an identity map is never iterated"
            ));
        }
    }
    // the guard is looking at every crate (under Bazel, all but those it names)
    for name in crates.keys() {
        let handed = tree.bazel_deps.is_none() || !CARGO_ONLY.iter().any(|(n, _)| n == name);
        if handed && !scanned.contains(name.as_str()) {
            problems.push(format!(
                "{name}'s sources were not scanned: hand them to this test (its BUILD's sources)"
            ));
        }
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

/// Where a call record is constructed outside the parser's crate, each with its count -- exact, so the scan is seen to
/// look -- and why (2026-11-12, the names follow-up: the owner's ruling moved Java's `compiler/spec/Calls` into
/// lite-names). A new site is a call built past the builder: build it through `lite_names::calls` instead.
const CALL_BUILDERS: &[(&str, usize, &str)] = &[
    (
        "rust/lite-names/src/calls.rs",
        3,
        "the one builder of a call after the resolver (Java's `compiler/spec/Calls`)",
    ),
    (
        "rust/lite-names/src/resolver/values.rs",
        1,
        "the resolver records a parsed call's names (Java's `NameResolver`, which the rule exempts)",
    ),
];

/// lite-front's call factories (the parser's `nodes` and `operator_parts`), which build a call with no record.
const CALL_FACTORIES: &[&str] = &[
    "lite_front::nodes",
    "nodes::applied",
    "nodes::infix",
    "applied_full",
    "operator_parts",
];

/// The constructions of a call record a code line writes: each `AppliedFunction {` that is not a return type
/// (`-> p::AppliedFunction {`, a function's body brace) nor a declaration (`struct`, `impl`).
fn call_constructions(line: &str) -> usize {
    let ident = |c: char| c.is_alphanumeric() || c == '_';
    let trimmed = line.trim_start();
    if trimmed.starts_with("impl") || trimmed.starts_with("struct") || trimmed.starts_with("pub struct") {
        return 0;
    }
    line.match_indices("AppliedFunction")
        .filter(|(at, word)| {
            let before = &line[..*at];
            let after = line[at + word.len()..].trim_start();
            if before.chars().next_back().is_some_and(ident) || !after.starts_with('{') {
                return false;
            }
            // the path before the name (`p::`, `records::`) is part of it
            let head = before.trim_end_matches(|c: char| ident(c) || c == ':').trim_end();
            !head.ends_with("->")
        })
        .count()
}

#[test]
fn calls_are_built_only_by_the_builder() {
    let tree = tree();
    let mut problems = Vec::new();
    let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
    let mut scanned = 0usize;
    for (path, text) in tree.files.iter().filter(|(p, _)| p.ends_with(".rs")) {
        let Some(name) = crate_of_path(path) else { continue };
        // the parser's crate builds calls before resolution; production sources only
        if name == "lite-front" || !path.starts_with(&format!("rust/{name}/src/")) {
            continue;
        }
        scanned += 1;
        let allowed = CALL_BUILDERS.iter().any(|(p, _, _)| p == path);
        for (n, line) in code_lines(text) {
            let made = call_constructions(line);
            if made > 0 {
                *counts.entry(path).or_default() += made;
                if !allowed {
                    problems.push(format!(
                        "{path}:{n}: a call record built outside lite-names' `calls` (build it there): {}",
                        line.trim()
                    ));
                }
            }
            for token in CALL_FACTORIES.iter().filter(|t| line.contains(**t)) {
                problems.push(format!(
                    "{path}:{n}: `{token}`, the parser's call factory, outside the parser's crate: {}",
                    line.trim()
                ));
            }
        }
    }
    for (path, expected, why) in CALL_BUILDERS {
        let seen = counts.get(path).copied().unwrap_or(0);
        if seen != *expected {
            problems.push(format!(
                "{path}: {seen} call records built where {expected} are pinned ({why}): a moved pin takes a dated \
                 comment and a rust/RETIRED_TESTS.md row"
            ));
        }
    }
    assert!(
        scanned >= 50,
        "{scanned} production sources scanned: the guard is not looking"
    );
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

#[test]
fn a_call_construction_is_told_from_a_return_type() {
    assert_eq!(call_constructions("    p::AppliedFunction {"), 1);
    assert_eq!(call_constructions("    Ok(Some(p::AppliedFunction {"), 1);
    assert_eq!(
        call_constructions("let x = lite_front::records::AppliedFunction { function, .. };"),
        1
    );
    assert_eq!(call_constructions("AppliedFunction{"), 1);
    assert_eq!(
        call_constructions("pub fn platform(name: &JStr, args: Vec<V>) -> p::AppliedFunction {"),
        0
    );
    assert_eq!(call_constructions(") -> AppliedFunction {"), 0);
    assert_eq!(call_constructions("impl AppliedFunction {"), 0);
    assert_eq!(call_constructions("pub struct AppliedFunction {"), 0);
    assert_eq!(call_constructions("V::AppliedFunction(af) => {"), 0);
    assert_eq!(call_constructions("let f: AppliedFunctionX {"), 0);
    assert_eq!(call_constructions("fn f(af: &p::AppliedFunction) {"), 0);
}
