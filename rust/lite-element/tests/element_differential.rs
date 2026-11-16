// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! The element differential (docs/rust-port/PHASE3.md, the element slice, 2026-11-15): what step F builds from E's
//! `NormalizedModel`, written by this crate exactly as core's `frontoracle/ElementAnswers.java` writes it over the live
//! Java (its header is the grammar; its code the spec), hashed and held ask by ask to the frozen answers
//! (rust/oracles/element/answers.tsv.zst; its MANIFEST.tsv says how they were made). Every model text of the front
//! oracle's questions, then of the slice's cases, at every dialect it is asked at (`["pmcd", dialect]`) that the door
//! parses -- a text that does not parse is skipped, as Java's `continue` -- run D (`resolve_alongside` beside the boot
//! layer), then the planner's `normalizeWithSystem` (tests/support/boot.rs), then `Model::from` with the boot's checked
//! layer less the graph's adopted ids and the platform's registrations, strict and tolerant:
//!
//! ```text
//! S TAB strict     no walls: the first error, D's, E's or F's
//! T TAB tolerant   one walls map threaded through D, E and F; `=S` when equal
//! ```
//!
//! a run an error `E TAB class TAB phase TAB element TAB message`, or the walls (`W TAB [[key,line],...]`, tolerant,
//! when any), `A`, and one line per question about the graph's own elements (tests/support/answers.rs). The hash is
//! the SHA-256 of the form's UTF-16 units, little-endian.
//!
//! **The boot first** (every ask depends on it): milestone 1, the boot layer as `Compiler.boot()` normalizes it
//! (rust/oracles/element/boot-normalized.tsv, element by element, then its imports', legacy surfaces' and whole model's
//! lines; D and E alone); milestone 2, the boot layer's own answers and the baselines every model shares (boot.txt,
//! the empty text's model with every boot element's cell forced).
//!
//! **Two buckets.** EQUAL and MISMATCH. A panic -- the skeleton's `unimplemented!` among them -- is a mismatch, counted
//! by its message, never an abort of the run.
//!
//! Inputs (env): `LITE_ELEMENT_QUESTIONS` (rust/oracles/front/questions.jsonl.zst, decompressed),
//! `LITE_ELEMENT_CASES` (rust/oracles/element/cases.jsonl), `LITE_ELEMENT_ANSWERS` (answers.tsv.zst, decompressed),
//! `LITE_ELEMENT_BOOT` (boot.txt.zst, decompressed), `LITE_ELEMENT_BOOT_NORMALIZED` (boot-normalized.tsv). Under plain
//! cargo the last four default to the repository's files beside this crate (the compressed ones decompressed with the
//! `zstd` command). Options: `LITE_ELEMENT_LIMIT=<n>` the first n asks; `LITE_ELEMENT_ONLY=<sha>` one text's asks;
//! `LITE_ELEMENT_THREADS=<n>` (default the machine's parallelism, at most 4; the answers are the same at any count).
//! Diagnosis (dev only, never in Bazel): `LITE_ELEMENT_FORMS=<the Java run's forms.jsonl>` (the archived
//! ~/.local/legend-lite/oracle-archive/element-forms-java-2026-11-15.jsonl.zst, decompressed: one JSON line per ask,
//! `{"sha","dialect","form"}`; streamed once into an index of byte offsets, cached at `LITE_ELEMENT_FORMS_INDEX` or
//! else `<report>/forms.idx`, stamped with the file's size and mtime) and `LITE_ELEMENT_REPORT=<dir>`: per mismatching
//! ask the Java and Rust forms and the first difference (its mode, its line's kind, both lines;
//! `LITE_ELEMENT_REPORT_FORMS=<n>` writes the first n asks' forms only), a summary grouping the mismatches by where
//! they first differ, every ask's verdict, and the boot's Rust texts (`boot.rust.txt`, `boot-normalized.rust.tsv`).

mod support;

use std::cell::Cell;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs::File;
use std::io::{BufRead, BufReader, BufWriter, Read, Seek, SeekFrom, Write};
use std::panic::AssertUnwindSafe;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc;
use std::time::Instant;

use lite_front::codec::{self, Codec, Writer};
use lite_front::doors::{Dialect, PARSE_STACK};
use lite_front::sha256::sha256_utf16le;
use lite_front::wire;
use lite_model::{NormalizedModel, ParsedModel};

use support::answers::{self, Baseline};
use support::boot::{Outcome, boot_normalized, panic_text};

/// How many mismatches a failure lists, and how many groups it prints.
const SHOWN: usize = 25;
const GROUPS_SHOWN: usize = 30;
/// Units of a line shown in a locator.
const LINE_SHOWN: usize = 400;

fn env(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| panic!("{name} names the file to read"))
}

fn env_opt(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|v| !v.is_empty())
}

fn units(s: &str) -> Vec<u16> {
    s.encode_utf16().collect()
}

/// The SHA-256 of a codec text's UTF-16 units, little-endian (ElementAnswers' `hash`).
fn hash(text: &str) -> String {
    sha256_utf16le(&units(text))
}

/// `u` for a one-line TSV cell: tabs, newlines and returns written as their escapes, cut at `LINE_SHOWN` units.
fn shown(u: &[u16]) -> String {
    let mut out = String::new();
    for c in char::decode_utf16(u.iter().copied().take(LINE_SHOWN)) {
        match c {
            Ok('\t') => out.push_str("\\t"),
            Ok('\n') => out.push_str("\\n"),
            Ok('\r') => out.push_str("\\r"),
            Ok(c) => out.push(c),
            Err(e) => out.push_str(&format!("\\u{:04x}", e.unpaired_surrogate())),
        }
    }
    if u.len() > LINE_SHOWN {
        out.push('…');
    }
    out
}

// =====================================================================
// The oracle's files
// =====================================================================

/// The repository's oracle file beside this crate (rust/oracles/element/), or the one `env` names.
fn oracle_path(env: &str, file: &str) -> String {
    env_opt(env).unwrap_or_else(|| {
        format!(
            "{}/../oracles/element/{file}",
            option_env!("CARGO_MANIFEST_DIR").unwrap_or(".")
        )
    })
}

/// A compressed oracle file's text: the decompressed file `env` names, or else the repository's `file`.zst beside this
/// crate decompressed with the `zstd` command.
fn oracle_text(env: &str, file: &str) -> String {
    if let Some(path) = env_opt(env) {
        return std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{env}={path}: {e}"));
    }
    let zst = format!(
        "{}/../oracles/element/{file}.zst",
        option_env!("CARGO_MANIFEST_DIR").unwrap_or(".")
    );
    let out = std::process::Command::new("zstd")
        .args(["-dc", "--long=27", &zst])
        .output()
        .unwrap_or_else(|e| panic!("no {env}, and `zstd -dc {zst}` did not run: {e}"));
    assert!(out.status.success(), "zstd -dc {zst} failed");
    String::from_utf8(out.stdout).unwrap_or_else(|e| panic!("{zst} is not UTF-8: {e}"))
}

struct FrozenLine {
    sha: String,
    dialect: String,
    hash: String,
}

fn read_frozen(text: &str) -> Vec<FrozenLine> {
    text.lines()
        .map(|line| {
            let f: Vec<&str> = line.split('\t').collect();
            assert_eq!(f.len(), 3, "a line of the frozen answers is sha, dialect, hash: {line}");
            FrozenLine {
                sha: f[0].to_owned(),
                dialect: f[1].to_owned(),
                hash: f[2].to_owned(),
            }
        })
        .collect()
}

// =====================================================================
// Milestone 1: the boot layer, normalized
// =====================================================================

/// `ElementAnswers.bootNormalized` (ElementAnswers.java:504): one line per element, in order (`index TAB fqn TAB` the SHA-256 of its record-codec
/// text, tagged: written as a `PackageableElement`), then `imports` (untagged), `legacySurfaces` (the values as a list,
/// each untagged) and the whole `NormalizedModel` (untagged).
fn boot_normalized_lines(model: &NormalizedModel) -> Vec<Vec<u16>> {
    let mut out = Vec::with_capacity(model.elements.len() + 3);
    for (i, el) in model.elements.iter().enumerate() {
        let mut line = units(&format!("{i}\t"));
        line.extend_from_slice(&el.qualified_name().0);
        line.extend(units(&format!("\t{}", hash(&codec::write(el, false)))));
        out.push(line);
    }
    out.push(units(&format!(
        "imports\t-\t{}",
        hash(&codec::write(&*model.imports, true))
    )));
    let mut w = Writer::new();
    w.raw("[");
    for (i, (_, x)) in model.legacy_surfaces.iter().enumerate() {
        if i > 0 {
            w.raw(",");
        }
        x.write(&mut w, true);
    }
    w.raw("]");
    out.push(units(&format!(
        "legacySurfaces\t{}\t{}",
        model.legacy_surfaces.len(),
        hash(&w.finish())
    )));
    out.push(units(&format!(
        "NormalizedModel\t{}\t{}",
        model.elements.len(),
        hash(&codec::write(model, true))
    )));
    out
}

/// Milestone 1's verdict: `None` when the boot's normalized lines are Java's, else what differs.
fn milestone_1(report: Option<&Path>) -> Option<String> {
    let path = oracle_path("LITE_ELEMENT_BOOT_NORMALIZED", "boot-normalized.tsv");
    let java: Vec<Vec<u16>> = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("the boot's normalized lines {path}: {e}"))
        .lines()
        .map(units)
        .collect();
    let boot = match boot_normalized() {
        Outcome::Done(b) => b,
        Outcome::Threw(e) => {
            return Some(format!(
                "the boot layer does not normalize: {}",
                String::from_utf16_lossy(&answers::error(e))
            ));
        }
        Outcome::Panicked(m) => return Some(format!("the boot layer panicked: {m}")),
    };
    let rust = boot_normalized_lines(&boot.model);
    if let Some(dir) = report {
        std::fs::create_dir_all(dir).unwrap();
        let text: Vec<String> = rust.iter().map(|l| String::from_utf16_lossy(l)).collect();
        std::fs::write(dir.join("boot-normalized.rust.tsv"), text.join("\n") + "\n").unwrap();
    }
    // index TAB fqn: a line less its hash
    let key = |l: &Vec<u16>| -> Vec<u16> {
        let tabs: Vec<usize> = l
            .iter()
            .enumerate()
            .filter(|(_, c)| **c == u16::from(b'\t'))
            .map(|(i, _)| i)
            .collect();
        l[..tabs.get(1).copied().unwrap_or(l.len())].to_vec()
    };
    let order_differs = java.len() != rust.len() || java.iter().zip(&rust).any(|(j, r)| key(j) != key(r));
    let differences: Vec<String> = java
        .iter()
        .zip(&rust)
        .enumerate()
        .filter(|(_, (j, r))| j != r)
        .map(|(i, (j, r))| format!("  line {i}: java {} | rust {}", shown(j), shown(r)))
        .collect();
    if !order_differs && differences.is_empty() {
        return None;
    }
    Some(format!(
        "the boot layer's normalized model is not Java's: {} java lines, {} rust lines{}, {} lines differ:\n{}",
        java.len(),
        rust.len(),
        if order_differs {
            " (the elements' order or names differ)"
        } else {
            ""
        },
        differences.len(),
        differences.iter().take(SHOWN).cloned().collect::<Vec<_>>().join("\n")
    ))
}

/// MILESTONE 1: the boot layer as `Compiler.boot()` normalizes it, equal to rust/oracles/element/boot-normalized.tsv
/// (ElementAnswers' `bootNormalized`). No element compiler: D and E alone.
#[test]
fn the_boot_layer_normalizes_as_javas() {
    let report = env_opt("LITE_ELEMENT_REPORT").map(PathBuf::from);
    if let Some(failure) = milestone_1(report.as_deref()) {
        panic!("{failure}");
    }
}

// =====================================================================
// Milestone 2: the boot layer's own answers, and the baselines
// =====================================================================

thread_local! {
    /// Whether this thread is inside [`guarded`]: the panic hook keeps a run's panic quiet there (its message is
    /// counted, never lost).
    static GUARDED: Cell<bool> = const { Cell::new(false) };
}

/// The panic hook, installed once: silent inside [`guarded`], the default hook for every other panic.
fn install_panic_hook() {
    static HOOK: std::sync::Once = std::sync::Once::new();
    HOOK.call_once(|| {
        let default = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            if !GUARDED.with(Cell::get) {
                default(info);
            }
        }));
    });
}

/// `f`, its panic caught (its message the error).
fn guarded<T>(f: impl FnOnce() -> T) -> Result<T, String> {
    let was = GUARDED.with(|g| g.replace(true));
    let r = std::panic::catch_unwind(AssertUnwindSafe(f));
    GUARDED.with(|g| g.set(was));
    r.map_err(|payload| panic_text(&*payload))
}

/// The boot dump and the baselines, made once per process (on a parse-sized stack), a panic as its message.
fn boot_answers() -> &'static Result<(Vec<u16>, Baseline), String> {
    static BOOT: OnceLock<Result<(Vec<u16>, Baseline), String>> = OnceLock::new();
    BOOT.get_or_init(|| {
        install_panic_hook();
        lite_front::doors::on_parse_stack(|| guarded(answers::boot_text))
    })
}

/// Milestone 2's verdict: `None` when the boot's answers are Java's boot.txt, else the first difference.
fn milestone_2(report: Option<&Path>) -> Option<String> {
    let java = oracle_text("LITE_ELEMENT_BOOT", "boot.txt");
    let rust = match boot_answers() {
        Ok((text, _)) => String::from_utf16_lossy(text),
        Err(m) => return Some(format!("the boot's answers panicked: {m}")),
    };
    if let Some(dir) = report {
        std::fs::create_dir_all(dir).unwrap();
        std::fs::write(dir.join("boot.rust.txt"), &rust).unwrap();
    }
    if rust == java {
        return None;
    }
    let (jl, rl): (Vec<&str>, Vec<&str>) = (java.lines().collect(), rust.lines().collect());
    let at = jl
        .iter()
        .zip(&rl)
        .position(|(j, r)| j != r)
        .unwrap_or(jl.len().min(rl.len()));
    Some(format!(
        "the boot's answers are not Java's boot.txt: {} java lines, {} rust lines; the first difference at line {at}:\n  \
         java {}\n  rust {}",
        jl.len(),
        rl.len(),
        jl.get(at).map_or("(none)".to_owned(), |l| shown(&units(l))),
        rl.get(at).map_or("(none)".to_owned(), |l| shown(&units(l)))
    ))
}

/// MILESTONE 2: the boot layer's own answers (the empty text's model, every boot element's cell forced) and the
/// baselines every model shares, equal to rust/oracles/element/boot.txt.zst (ElementAnswers' `boot`). Ignored while
/// the element compiler's families are the skeleton's `unimplemented!` (2026-11-15: five helpers fill them; the lead
/// removes this ignore when they land). `--ignored` (the Bazel target's) runs it.
#[test]
fn the_boot_answers_as_javas() {
    let report = env_opt("LITE_ELEMENT_REPORT").map(PathBuf::from);
    if let Some(failure) = milestone_2(report.as_deref()) {
        panic!("{failure}");
    }
}

// =====================================================================
// The asks
// =====================================================================

/// One mode's run, its panic caught: the line (`E`, walls and `A`, or `A`), or `!panic TAB message`.
struct RunLine {
    text: Vec<u16>,
    panic: Option<String>,
}

fn run_mode(parsed: &ParsedModel, tolerant: bool) -> RunLine {
    let outcome = match boot_answers() {
        Ok((_, base)) => guarded(|| answers::run(parsed, tolerant, base)),
        Err(m) => Err(m.clone()),
    };
    match outcome {
        Ok(text) => RunLine { text, panic: None },
        Err(m) => {
            let mut text = units("!panic\t");
            text.extend(units(&m));
            RunLine { text, panic: Some(m) }
        }
    }
}

type Key = (String, String);

/// The Java run's forms file and its index: `(sha, dialect)` -> (byte offset, length) of its line.
struct Forms {
    path: PathBuf,
    index: HashMap<Key, (u64, usize)>,
}

fn stamp(path: &Path) -> String {
    let meta = std::fs::metadata(path).unwrap_or_else(|e| panic!("the forms file {}: {e}", path.display()));
    let mtime = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_secs());
    format!("# forms {} {} {}", path.display(), meta.len(), mtime)
}

impl Forms {
    /// The index, read from `cache` when its stamp is the file's, else built by streaming the file once and cached.
    fn open(path: &Path, cache: &Path) -> Forms {
        let stamp = stamp(path);
        if let Ok(text) = std::fs::read_to_string(cache)
            && text.lines().next() == Some(stamp.as_str())
        {
            let mut index = HashMap::new();
            for line in text.lines().skip(1) {
                let f: Vec<&str> = line.split('\t').collect();
                assert_eq!(
                    f.len(),
                    4,
                    "a line of the forms index is sha, dialect, offset, length: {line}"
                );
                index.insert(
                    (f[0].to_owned(), f[1].to_owned()),
                    (f[2].parse().unwrap(), f[3].parse().unwrap()),
                );
            }
            println!(
                "[element-differential] forms index: {} asks, read from {}",
                index.len(),
                cache.display()
            );
            return Forms {
                path: path.to_owned(),
                index,
            };
        }
        let started = Instant::now();
        let mut reader = BufReader::with_capacity(8 << 20, File::open(path).unwrap());
        let mut out = BufWriter::new(File::create(cache).unwrap());
        writeln!(out, "{stamp}").unwrap();
        let mut index = HashMap::new();
        let mut buf: Vec<u8> = Vec::new();
        let mut offset = 0u64;
        const SHA_AT: usize = br#"{"sha":""#.len();
        const DIALECT_KEY: &[u8] = br#"","dialect":""#;
        loop {
            buf.clear();
            let n = reader.read_until(b'\n', &mut buf).unwrap();
            if n == 0 {
                break;
            }
            assert!(
                buf.starts_with(br#"{"sha":""#) && buf.len() > SHA_AT + 64 + DIALECT_KEY.len(),
                "a forms line starts with its sha at byte {offset}"
            );
            let sha = std::str::from_utf8(&buf[SHA_AT..SHA_AT + 64]).unwrap().to_owned();
            let rest = &buf[SHA_AT + 64..];
            assert!(
                rest.starts_with(DIALECT_KEY),
                "a forms line names its dialect second, at byte {offset}"
            );
            let rest = &rest[DIALECT_KEY.len()..];
            let end = rest.iter().position(|&b| b == b'"').unwrap();
            let dialect = std::str::from_utf8(&rest[..end]).unwrap().to_owned();
            writeln!(out, "{sha}\t{dialect}\t{offset}\t{n}").unwrap();
            index.insert((sha, dialect), (offset, n));
            offset += n as u64;
        }
        out.flush().unwrap();
        println!(
            "[element-differential] forms index: {} asks over {offset} bytes, built in {:.1} s and cached at {}",
            index.len(),
            started.elapsed().as_secs_f64(),
            cache.display()
        );
        Forms {
            path: path.to_owned(),
            index,
        }
    }

    /// One ask's Java form, read through `file` (each worker's own handle).
    fn form(&self, file: &mut Option<File>, key: &Key) -> Option<Vec<u16>> {
        let &(offset, len) = self.index.get(key)?;
        let f = file.get_or_insert_with(|| File::open(&self.path).unwrap());
        f.seek(SeekFrom::Start(offset)).unwrap();
        let mut bytes = vec![0u8; len];
        f.read_exact(&mut bytes).unwrap();
        let line = std::str::from_utf8(&bytes).expect("a forms line is UTF-8");
        let json = wire::parse(line.trim_end(), 8).expect("a forms line is JSON");
        Some(
            json.get("form")
                .and_then(wire::Json::as_units)
                .expect("a forms line has its form")
                .to_vec(),
        )
    }
}

/// Where a mismatch's first difference is: the mode (`S`, `T`), the kind of its line (`C`, `F`, `P`, ...; `A`, `W`, `E`
/// for a run's head), its group (what differs there), both lines.
struct Locator {
    mode: String,
    group: String,
    detail: String,
}

/// A form's lines, each with its mode: `S TAB strict`, then `T TAB tolerant` (`=S` expanded to the strict lines).
fn mode_lines(form: &[u16]) -> Vec<(char, Vec<u16>)> {
    let text: Vec<&[u16]> = form.split(|&c| c == u16::from(b'\n')).collect();
    let mut out = Vec::new();
    let mut strict: Vec<Vec<u16>> = Vec::new();
    let mut mode = 'S';
    for (i, line) in text.iter().enumerate() {
        let line = if i == 0 {
            line.strip_prefix(&units("S\t")[..]).unwrap_or(line)
        } else if let Some(rest) = line.strip_prefix(&units("T\t")[..])
            && mode == 'S'
        {
            mode = 'T';
            if rest == units("=S") {
                for l in &strict {
                    out.push(('T', l.clone()));
                }
                continue;
            }
            rest
        } else {
            line
        };
        if mode == 'S' {
            strict.push(line.to_vec());
        }
        out.push((mode, line.to_vec()));
    }
    out
}

/// A line's kind: its first field (`C`, `D@`, ...; `E` for an error, `!panic`), `A` alone.
fn line_kind(line: &[u16]) -> String {
    let end = line.iter().position(|&c| c == u16::from(b'\t')).unwrap_or(line.len());
    String::from_utf16_lossy(&line[..end])
}

/// Whether a line's answer is a cell's refusal (`... TAB R TAB class TAB first line`).
fn refused(line: &[u16]) -> bool {
    line.windows(3).any(|w| w == units("\tR\t").as_slice())
}

/// The first difference between Java's form and Rust's: in the strict lines first, then in the tolerant ones.
fn locate(java: &[u16], rust: &[u16]) -> Locator {
    let (j, r) = (mode_lines(java), mode_lines(rust));
    let of = |lines: &[(char, Vec<u16>)], m: char| -> Vec<Vec<u16>> {
        lines.iter().filter(|(x, _)| *x == m).map(|(_, l)| l.clone()).collect()
    };
    for m in ['S', 'T'] {
        let (jm, rm) = (of(&j, m), of(&r, m));
        if jm == rm {
            continue;
        }
        let at = jm
            .iter()
            .zip(&rm)
            .position(|(a, b)| a != b)
            .unwrap_or(jm.len().min(rm.len()));
        let (jl, rl) = (jm.get(at), rm.get(at));
        let group = match (jl, rl) {
            (_, Some(r)) if line_kind(r) == "!panic" => {
                format!(
                    "rust panicked: {}",
                    shown(&r[7..]).chars().take(160).collect::<String>()
                )
            }
            (Some(j), Some(r)) if line_kind(j) != line_kind(r) => {
                format!("{m} kind: java={} rust={}", line_kind(j), line_kind(r))
            }
            (Some(j), Some(r)) => format!(
                "{m} {}{}",
                line_kind(j),
                match (refused(j), refused(r)) {
                    (true, false) => " (java refuses, rust answers)",
                    (false, true) => " (rust refuses, java answers)",
                    (true, true) => " (both refuse, differently)",
                    (false, false) => "",
                }
            ),
            (Some(j), None) => format!("{m} java has more: {}", line_kind(j)),
            (None, Some(r)) => format!("{m} rust has more: {}", line_kind(r)),
            (None, None) => unreachable!("two unequal line lists differ at a line"),
        };
        return Locator {
            mode: m.to_string(),
            group,
            detail: format!(
                "line {at}: java {} ‖ rust {}",
                jl.map_or("(none)".to_owned(), |l| shown(l)),
                rl.map_or("(none)".to_owned(), |l| shown(l))
            ),
        };
    }
    Locator {
        mode: "-".to_owned(),
        group: "the forms are equal: the frozen hash is not this form's".to_owned(),
        detail: "-".to_owned(),
    }
}

/// A run's head, for a mismatch with no Java form to read: `A`, `W`, `E(<class>)`, `panic`.
fn head(line: &[u16]) -> String {
    match line_kind(line).as_str() {
        "E" => {
            let class: Vec<u16> = line[2..]
                .iter()
                .copied()
                .take_while(|&c| c != u16::from(b'\t'))
                .collect();
            let class = String::from_utf16_lossy(&class);
            format!("E({})", class.rsplit('.').next().unwrap_or(&class))
        }
        "!panic" => "panic".to_owned(),
        k if k.starts_with('A') => "A".to_owned(),
        k => k.to_owned(),
    }
}

/// What a run of the harness reads and how.
struct Opts<'a> {
    label: &'static str,
    /// The questions files, in order (the front oracle's, then the cases).
    texts: Vec<String>,
    /// The frozen lines these asks are held to, in order (`only` looks them up by key instead).
    frozen: &'a [FrozenLine],
    limit: usize,
    only: Option<String>,
    threads: usize,
    forms: Option<&'a Forms>,
    report: Option<PathBuf>,
    /// How many mismatches' forms the report writes (`LITE_ELEMENT_REPORT_FORMS`; every one by default).
    report_forms: usize,
}

/// One ask's outcome, as a worker hands it on.
struct AskOut {
    sha: String,
    dialect: String,
    /// Each mode's head (`A`, `W`, `E(..)`, `panic`).
    heads: [String; 2],
    /// The panic messages met, strict then tolerant.
    panics: Vec<String>,
    mismatch: Option<Locator>,
    /// For the report: the Java form (when the forms are given) and the Rust one.
    forms: Option<(Option<Vec<u16>>, Vec<u16>)>,
}

/// A worker's view of everything it needs, shared.
struct Shared<'a> {
    hashes: HashMap<Key, &'a str>,
    forms: Option<&'a Forms>,
    report: bool,
}

/// Every ask of one questions line, in order.
fn text_asks(line: &str, only: Option<&str>, sh: &Shared<'_>, java_file: &mut Option<File>) -> Vec<AskOut> {
    let q = wire::parse(line, 64).expect("a questions line is JSON");
    let sha = q
        .get("sha")
        .and_then(wire::Json::as_string)
        .expect("a questions line has its sha");
    if only.is_some_and(|o| o != sha) {
        return Vec::new();
    }
    let text = q
        .get("text")
        .and_then(wire::Json::as_units)
        .expect("a questions line has its text");
    let mut out = Vec::new();
    for a in q
        .get("asks")
        .and_then(wire::Json::as_array)
        .expect("a questions line has its asks")
    {
        let pair = a.as_array().expect("an ask is a pair");
        let (door, dialect_name) = (pair[0].as_string().unwrap(), pair[1].as_string().unwrap());
        if door != "pmcd" {
            continue;
        }
        let dialect = Dialect::from_java(&dialect_name).expect("a dialect");
        let Ok(parsed) = lite_builtin::door::parse(text, dialect) else {
            continue;
        };
        out.push(one_ask(&parsed, sha.clone(), dialect_name, sh, java_file));
    }
    out
}

fn one_ask(
    parsed: &ParsedModel,
    sha: String,
    dialect: String,
    sh: &Shared<'_>,
    java_file: &mut Option<File>,
) -> AskOut {
    let s = run_mode(parsed, false);
    let t = run_mode(parsed, true);
    let rust = answers::form(&s.text, &t.text);
    let key: Key = (sha, dialect);
    let mut out = AskOut {
        sha: key.0.clone(),
        dialect: key.1.clone(),
        heads: [head(&s.text), head(&t.text)],
        panics: s.panic.iter().chain(t.panic.iter()).cloned().collect(),
        mismatch: None,
        forms: None,
    };
    let frozen = sh.hashes.get(&key).copied();
    if frozen == Some(sha256_utf16le(&rust).as_str()) {
        return out;
    }
    let java = sh.forms.and_then(|f| f.form(java_file, &key));
    let mut locator = match (&java, frozen) {
        (_, None) => Locator {
            mode: "-".to_owned(),
            group: "not among the frozen answers".to_owned(),
            detail: "-".to_owned(),
        },
        (Some(java), Some(_)) => locate(java, &rust),
        (None, Some(_)) => Locator {
            mode: "?".to_owned(),
            group: match out.panics.first() {
                Some(m) => format!(
                    "rust panicked: {}",
                    m.lines().next().unwrap_or("").chars().take(160).collect::<String>()
                ),
                None => format!(
                    "(no Java form) rust S={} T={}",
                    out.heads[0],
                    if t.text == s.text { "=S" } else { &out.heads[1] }
                ),
            },
            detail: "-".to_owned(),
        },
    };
    if let (Some(java), Some(h)) = (&java, frozen)
        && sha256_utf16le(java) != h
    {
        locator.detail = format!("(the forms file's form is not the frozen answer's) {}", locator.detail);
    }
    out.mismatch = Some(locator);
    if sh.report {
        out.forms = Some((java, rust));
    }
    out
}

/// A mismatch group's asks: (all, in S, in T, an example).
type Group = (usize, usize, usize, String);

/// What the collector counts.
#[derive(Default)]
struct Tally {
    asked: usize,
    equal: usize,
    mismatch: usize,
    /// per mode (0 strict, 1 tolerant): the asks by their run's head
    heads: [BTreeMap<String, usize>; 2],
    /// a panic's first line -> the runs that met it
    panics: BTreeMap<String, usize>,
    /// group -> (asks, in S, in T, an example)
    groups: BTreeMap<String, Group>,
    shown_mismatches: Vec<String>,
    forms_written: usize,
}

/// The report's per-ask files.
struct Report {
    locators: BufWriter<File>,
    asks: BufWriter<File>,
}

fn write_units(path: &Path, u: &[u16]) {
    std::fs::write(path, String::from_utf16_lossy(u)).unwrap();
}

/// The harness: every ask in order, held to the frozen lines; the tally.
fn differential(opts: &Opts<'_>) -> Tally {
    install_panic_hook();
    let started = Instant::now();
    let files: Vec<String> = opts
        .texts
        .iter()
        .map(|path| std::fs::read_to_string(path).unwrap_or_else(|e| panic!("the questions {path}: {e}")))
        .collect();
    let lines: Vec<&str> = files.iter().flat_map(|f| f.lines()).filter(|l| !l.is_empty()).collect();
    let shared = Shared {
        hashes: opts
            .frozen
            .iter()
            .map(|l| ((l.sha.clone(), l.dialect.clone()), l.hash.as_str()))
            .collect(),
        forms: opts.forms,
        report: opts.report.is_some(),
    };
    let mut report = opts.report.as_ref().map(|dir| {
        std::fs::create_dir_all(dir.join("forms")).unwrap();
        let mut r = Report {
            locators: BufWriter::new(File::create(dir.join("locators.tsv")).unwrap()),
            asks: BufWriter::new(File::create(dir.join("asks.tsv")).unwrap()),
        };
        writeln!(r.locators, "# sha\tdialect\tmode\tgroup\tdetail").unwrap();
        writeln!(r.asks, "# sha\tdialect\tverdict\tstrict\ttolerant").unwrap();
        r
    });
    let next = AtomicUsize::new(0);
    let stop = AtomicBool::new(false);
    let mut tally = Tally::default();
    let (tx, rx) = mpsc::channel::<(usize, Vec<AskOut>)>();
    std::thread::scope(|scope| {
        for w in 0..opts.threads {
            let tx = tx.clone();
            let (lines, next, stop, shared) = (&lines, &next, &stop, &shared);
            let only = opts.only.as_deref();
            std::thread::Builder::new()
                .name(format!("element-differential-{w}"))
                .stack_size(PARSE_STACK)
                .spawn_scoped(scope, move || {
                    let mut java_file: Option<File> = None;
                    while !stop.load(Ordering::Relaxed) {
                        let i = next.fetch_add(1, Ordering::Relaxed);
                        let Some(line) = lines.get(i) else { break };
                        let asks = text_asks(line, only, shared, &mut java_file);
                        if tx.send((i, asks)).is_err() {
                            break;
                        }
                    }
                })
                .expect("a worker starts");
        }
        drop(tx);
        let mut pending: BTreeMap<usize, Vec<AskOut>> = BTreeMap::new();
        let mut want = 0usize;
        for (i, asks) in rx {
            pending.insert(i, asks);
            while let Some(asks) = pending.remove(&want) {
                want += 1;
                for a in asks {
                    if tally.asked >= opts.limit {
                        stop.store(true, Ordering::Relaxed);
                        break;
                    }
                    collect(&mut tally, opts, a, report.as_mut());
                }
            }
        }
    });
    if let Some(mut r) = report {
        r.locators.flush().unwrap();
        r.asks.flush().unwrap();
    }
    if let Some(sha) = &opts.only {
        assert!(
            tally.asked > 0,
            "{}: LITE_ELEMENT_ONLY={sha} names no text whose asks the door parses",
            opts.label
        );
    } else {
        assert_eq!(
            tally.asked,
            opts.frozen.len().min(opts.limit),
            "{}: the asks are not the frozen answers' lines",
            opts.label
        );
    }
    print_tally(opts, &tally, started.elapsed().as_secs_f64());
    if let Some(dir) = &opts.report {
        write_summary(dir, &tally);
    }
    tally
}

/// One ask, in order: held in step with the frozen lines, counted, reported.
fn collect(tally: &mut Tally, opts: &Opts<'_>, a: AskOut, report: Option<&mut Report>) {
    if opts.only.is_none() {
        let line = opts.frozen.get(tally.asked).unwrap_or_else(|| {
            panic!(
                "{}: the frozen answers hold fewer lines than the asks (at {} {})",
                opts.label, a.dialect, a.sha
            )
        });
        assert!(
            line.sha == a.sha && line.dialect == a.dialect,
            "{}: the frozen answers are out of step at ask {}: {} {} asked, {} {} frozen",
            opts.label,
            tally.asked,
            a.dialect,
            a.sha,
            line.dialect,
            line.sha
        );
    }
    tally.asked += 1;
    for (m, h) in a.heads.iter().enumerate() {
        *tally.heads[m].entry(h.clone()).or_default() += 1;
    }
    for p in &a.panics {
        let first = p.lines().next().unwrap_or("").chars().take(200).collect::<String>();
        *tally.panics.entry(first).or_default() += 1;
    }
    let who = format!("{} {}", a.dialect, a.sha);
    match &a.mismatch {
        None => tally.equal += 1,
        Some(l) => {
            tally.mismatch += 1;
            if tally.shown_mismatches.len() < SHOWN {
                tally.shown_mismatches.push(format!("{who} {} ({})", l.mode, l.group));
            }
            let g = tally
                .groups
                .entry(l.group.clone())
                .or_insert_with(|| (0, 0, 0, who.clone()));
            g.0 += 1;
            g.1 += usize::from(l.mode == "S");
            g.2 += usize::from(l.mode == "T");
        }
    }
    let Some(r) = report else { return };
    writeln!(
        r.asks,
        "{}\t{}\t{}\t{}\t{}",
        a.sha,
        a.dialect,
        if a.mismatch.is_some() { "mismatch" } else { "equal" },
        a.heads[0],
        a.heads[1]
    )
    .unwrap();
    if let Some(l) = &a.mismatch {
        writeln!(
            r.locators,
            "{}\t{}\t{}\t{}\t{}",
            a.sha, a.dialect, l.mode, l.group, l.detail
        )
        .unwrap();
        if let (Some(dir), Some((java, rust))) = (&opts.report, &a.forms)
            && tally.forms_written < opts.report_forms
        {
            tally.forms_written += 1;
            let base = dir.join("forms");
            if let Some(java) = java {
                write_units(&base.join(format!("{}.{}.java.txt", a.sha, a.dialect)), java);
            }
            write_units(&base.join(format!("{}.{}.rust.txt", a.sha, a.dialect)), rust);
        }
    }
}

/// The mismatch groups, largest first.
fn groups(t: &Tally) -> Vec<(&String, &Group)> {
    let mut rows: Vec<(&String, &Group)> = t.groups.iter().collect();
    rows.sort_by(|a, b| b.1.0.cmp(&a.1.0).then_with(|| a.0.cmp(b.0)));
    rows
}

fn print_tally(opts: &Opts<'_>, t: &Tally, seconds: f64) {
    let p = |s: String| println!("[element-differential] {s}");
    p(format!(
        "{}: {} asks ({} frozen) in {seconds:.1} s on {} threads: {} equal, {} mismatch",
        opts.label,
        t.asked,
        opts.frozen.len(),
        opts.threads,
        t.equal,
        t.mismatch
    ));
    for (m, name) in [(0, "strict"), (1, "tolerant")] {
        let row: Vec<String> = t.heads[m].iter().map(|(h, n)| format!("{h} {n}")).collect();
        p(format!("  {name} runs: {}", row.join(", ")));
    }
    if !t.panics.is_empty() {
        p(format!("  panics by message ({} messages; runs):", t.panics.len()));
        let mut rows: Vec<(&String, &usize)> = t.panics.iter().collect();
        rows.sort_by(|a, b| b.1.cmp(a.1).then_with(|| a.0.cmp(b.0)));
        for (m, n) in rows.into_iter().take(GROUPS_SHOWN) {
            p(format!("    {n:>6}  {m}"));
        }
    }
    if !t.groups.is_empty() {
        p("  mismatches by first difference (asks, in S, in T; an example):".to_owned());
        for (group, (n, s, tt, ex)) in groups(t).into_iter().take(GROUPS_SHOWN) {
            p(format!("    {n:>6}  {group}  (S {s}, T {tt}; {ex})"));
        }
    }
}

fn write_summary(dir: &Path, t: &Tally) {
    let mut f = BufWriter::new(File::create(dir.join("summary.tsv")).unwrap());
    writeln!(f, "# asks\tin S\tin T\tfirst difference\texample").unwrap();
    for (group, (n, s, tt, ex)) in groups(t) {
        writeln!(f, "{n}\t{s}\t{tt}\t{group}\t{ex}").unwrap();
    }
    let mut p = BufWriter::new(File::create(dir.join("panics.tsv")).unwrap());
    writeln!(p, "# runs\tpanic (first line)").unwrap();
    for (m, n) in &t.panics {
        writeln!(p, "{n}\t{m}").unwrap();
    }
}

/// The asks' verdict: `None` when every ask is equal, else what differs (bounded).
fn asks_verdict(label: &str, t: &Tally) -> Option<String> {
    if t.mismatch == 0 {
        return None;
    }
    let mut lines: Vec<String> = t.shown_mismatches.iter().map(|m| format!("  mismatch: {m}")).collect();
    for (group, (n, _, _, _)) in groups(t).into_iter().take(SHOWN) {
        lines.push(format!("  {n} asks: {group}"));
    }
    Some(format!(
        "{label}: of {} asks, {} equal, {} answer otherwise than Java's\n{}",
        t.asked,
        t.equal,
        t.mismatch,
        lines.join("\n")
    ))
}

fn report_forms() -> usize {
    env_opt("LITE_ELEMENT_REPORT_FORMS")
        .map_or(usize::MAX, |n| n.parse().expect("LITE_ELEMENT_REPORT_FORMS is a count"))
}

fn threads() -> usize {
    env_opt("LITE_ELEMENT_THREADS").map_or_else(
        || std::thread::available_parallelism().map_or(1, |n| n.get()).min(4),
        |n| n.parse::<usize>().expect("LITE_ELEMENT_THREADS is a count").max(1),
    )
}

/// The diagnosis inputs, by env: the Java forms (indexed once, the index cached) and the report directory.
fn diagnosis(sub: &str) -> (Option<Forms>, Option<PathBuf>) {
    let report = env_opt("LITE_ELEMENT_REPORT").map(|d| PathBuf::from(d).join(sub));
    let forms = env_opt("LITE_ELEMENT_FORMS").map(|path| {
        let cache = env_opt("LITE_ELEMENT_FORMS_INDEX")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                let dir = report
                    .as_ref()
                    .and_then(|r| r.parent().map(Path::to_owned))
                    .expect("LITE_ELEMENT_FORMS needs LITE_ELEMENT_FORMS_INDEX or LITE_ELEMENT_REPORT for its index");
                std::fs::create_dir_all(&dir).unwrap();
                dir.join("forms.idx")
            });
        Forms::open(Path::new(&path), &cache)
    });
    (forms, report)
}

/// THE DIFFERENTIAL: milestones 1 and 2, then every ask of the front oracle's questions and the slice's cases, held to
/// the frozen answers; every failure listed, then the test fails.
#[test]
#[ignore = "reads the front oracle's questions, the cases and the frozen answers (LITE_ELEMENT_*): \
            //rust/lite-element:element_differential_test runs it"]
fn what_step_f_builds_answers_as_javas() {
    let frozen = read_frozen(&oracle_text("LITE_ELEMENT_ANSWERS", "answers.tsv"));
    let (forms, report) = diagnosis("full");
    let mut failures: Vec<String> = Vec::new();
    failures.extend(milestone_1(report.as_deref()));
    failures.extend(milestone_2(report.as_deref()));
    let opts = Opts {
        label: "full",
        texts: vec![
            env("LITE_ELEMENT_QUESTIONS"),
            oracle_path("LITE_ELEMENT_CASES", "cases.jsonl"),
        ],
        frozen: &frozen,
        limit: env_opt("LITE_ELEMENT_LIMIT").map_or(usize::MAX, |n| n.parse().expect("LITE_ELEMENT_LIMIT is a count")),
        only: env_opt("LITE_ELEMENT_ONLY"),
        threads: threads(),
        forms: forms.as_ref(),
        report,
        report_forms: report_forms(),
    };
    let tally = differential(&opts);
    failures.extend(asks_verdict("the element differential", &tally));
    assert!(failures.is_empty(), "{}", failures.join("\n\n"));
}

/// The slice's own cases alone (rust/oracles/element/cases.jsonl), held to the frozen lines of their asks -- the
/// answers' last lines (asserted). It reads the decompressed answers at `LITE_ELEMENT_ANSWERS`, or else decompresses the
/// repository's answers.tsv.zst with the `zstd` command. Ignored while the element compiler's families are the
/// skeleton's `unimplemented!` (2026-11-15; the lead removes this ignore when they land, so the cases run under plain
/// `cargo test`, as the normalizer's do).
#[test]
fn the_slices_cases_answer_as_javas() {
    let cases = oracle_path("LITE_ELEMENT_CASES", "cases.jsonl");
    let frozen = read_frozen(&oracle_text("LITE_ELEMENT_ANSWERS", "answers.tsv"));
    // the cases' asks are the frozen answers' last lines (some case texts are front-oracle texts too, so their lines
    // are counted, not searched for): one per pmcd ask whose text the door parses
    let text = std::fs::read_to_string(&cases).unwrap_or_else(|e| panic!("the cases {cases}: {e}"));
    let mut shas: HashSet<String> = HashSet::new();
    let mut asks = 0usize;
    for line in text.lines().filter(|l| !l.is_empty()) {
        let q = wire::parse(line, 64).expect("a cases line is JSON");
        let sha = q
            .get("sha")
            .and_then(wire::Json::as_string)
            .expect("a cases line has its sha");
        let units = q
            .get("text")
            .and_then(wire::Json::as_units)
            .expect("a cases line has its text");
        for a in q
            .get("asks")
            .and_then(wire::Json::as_array)
            .expect("a cases line has its asks")
        {
            let pair = a.as_array().expect("an ask is a pair");
            let dialect = Dialect::from_java(&pair[1].as_string().unwrap()).expect("a dialect");
            if pair[0].as_string().unwrap() == "pmcd" && lite_builtin::door::parse(units, dialect).is_ok() {
                asks += 1;
            }
        }
        shas.insert(sha);
    }
    assert!(asks > 0 && asks <= frozen.len(), "the cases' asks are frozen");
    let from = frozen.len() - asks;
    assert!(
        frozen[from..].iter().all(|l| shas.contains(&l.sha)),
        "the cases' asks are the frozen answers' last lines"
    );
    let (forms, report) = diagnosis("cases");
    let opts = Opts {
        label: "cases",
        texts: vec![cases],
        frozen: &frozen[from..],
        limit: usize::MAX,
        only: None,
        threads: threads(),
        forms: forms.as_ref(),
        report,
        report_forms: report_forms(),
    };
    let tally = differential(&opts);
    if let Some(failure) = asks_verdict("the element cases", &tally) {
        panic!("{failure}");
    }
}

// =====================================================================
// The harness's own pieces (no oracle needed)
// =====================================================================

#[test]
fn a_runs_panic_is_caught_with_its_message() {
    install_panic_hook();
    let stub = guarded(|| -> u8 { unimplemented!("lite-element skeleton: PureModelContext.from") }).unwrap_err();
    assert_eq!(stub, "not implemented: lite-element skeleton: PureModelContext.from");
    assert_eq!(guarded(|| 7u8), Ok(7));
}

#[test]
fn the_locator_names_the_mode_the_kind_and_the_cause() {
    let java = units("S\tA\nC\ta::B\t{\"x\":1}\nY\ta::B\tnone\nT\t=S");
    let rust = units("S\tA\nC\ta::B\t{\"x\":2}\nY\ta::B\tnone\nT\t=S");
    let l = locate(&java, &rust);
    assert_eq!((l.mode.as_str(), l.group.as_str()), ("S", "S C"));
    // the tolerant line's walls, the =S expanded
    let walled = units("S\tA\nC\ta::B\t{\"x\":1}\nY\ta::B\tnone\nT\tW\t[[\"a::B\",\"one\"]]\nA\nC\ta::B\tR\tc\tm");
    let l = locate(&java, &walled);
    assert_eq!((l.mode.as_str(), l.group.as_str()), ("T", "T kind: java=A rust=W"));
    let refusing = units("S\tA\nC\ta::B\tR\tjava.lang.IllegalArgumentException\tupper\nY\ta::B\tnone\nT\t=S");
    assert_eq!(locate(&java, &refusing).group, "S C (rust refuses, java answers)");
    let panicked = units("S\t!panic\tnot implemented: x\nT\t!panic\tnot implemented: x");
    assert_eq!(locate(&java, &panicked).group, "rust panicked: not implemented: x");
    assert_eq!(
        locate(&java, &units("S\tA\nC\ta::B\t{\"x\":1}\nT\t=S")).group,
        "S java has more: Y"
    );
}

#[test]
fn a_heads_class_is_its_simple_name() {
    assert_eq!(
        head(&units("E\tcom.legend.error.ModelException\tMODEL\ta::B\tm")),
        "E(ModelException)"
    );
    assert_eq!(head(&units("A\nC\ta\tnone")), "A");
    assert_eq!(head(&units("W\t[]\nA")), "W");
    assert_eq!(head(&units("!panic\tx")), "panic");
}
