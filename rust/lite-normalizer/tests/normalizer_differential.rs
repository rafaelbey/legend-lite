// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! The normalizer differential (docs/rust-port/PHASE3.md, the normalizer slice, 2026-11-14): what step E hands to F,
//! written by this crate exactly as core's `frontoracle/NormalizerAnswers.java` writes it over the live Java, hashed
//! and held ask by ask to the frozen answers (rust/oracles/normalizer/answers.tsv.zst; its MANIFEST.tsv says how they
//! were made). Every model text of the front oracle's questions, then of the slice's cases, at every dialect it is
//! asked at (`["pmcd", dialect]`) that the door parses -- a text that does not parse is skipped, as Java's `continue`
//! -- run D then E (`resolve_alongside` beside the boot layer, the association adoption, the index, `refuse_duplicates`,
//! `lite_normalizer::normalize`), strict and tolerant:
//!
//! ```text
//! S TAB strict     no walls: the first error, D's or E's
//! T TAB tolerant   one walls map threaded through D and E; `=S` when equal
//! ```
//!
//! a run `A TAB` the record codec's JSON of the `NormalizedModel` (its declared type, so untagged), prefixed by the walls
//! (`[[key,line],...] TAB`) when a tolerant run has any; an error `E TAB class TAB phase TAB element TAB message`. The
//! hash is the SHA-256 of the form's UTF-16 units, little-endian.
//!
//! **Three buckets.** EQUAL, MISMATCH, and UNPORTED: an ask whose strict or tolerant run reaches a body the skeleton has
//! not ported yet (`Thrown::Unported(site)` at the top, the `unported!` panic of a body that cannot fail, or a wall or
//! message that carries one's text) is unported, counted per site, never a mismatch. Any other panic is a mismatch
//! (`!panic`), never an abort of the run.
//!
//! Inputs (env): `LITE_NORMALIZER_QUESTIONS` (rust/oracles/front/questions.jsonl.zst, decompressed),
//! `LITE_NORMALIZER_CASES` (rust/oracles/normalizer/cases.jsonl), `LITE_NORMALIZER_ANSWERS` (answers.tsv.zst,
//! decompressed). Options: `LITE_NORMALIZER_LIMIT=<n>` the first n asks; `LITE_NORMALIZER_ONLY=<sha>` one text's asks;
//! `LITE_NORMALIZER_THREADS=<n>` (default the machine's parallelism, at most 4; the answers are the same at any count).
//! Diagnosis (dev only, never in Bazel): `LITE_NORMALIZER_FORMS=<the Java run's forms.jsonl>` (one JSON line per ask,
//! `{"sha","dialect","form"}`; streamed once into an index of byte offsets, cached at `LITE_NORMALIZER_FORMS_INDEX` or
//! else `<report>/forms.idx`, stamped with the file's size and mtime) and `LITE_NORMALIZER_REPORT=<dir>`: per
//! mismatching ask the Java and Rust forms and a locator of the first difference (its mode, the JSON path to the first
//! differing value, the offset with 120 units of context each side; `LITE_NORMALIZER_REPORT_FORMS=<n>` writes the first
//! n asks' forms only); a summary grouping the mismatches by that path, indices erased, so causes cluster; every ask's
//! verdict; the unported histogram. With the forms, the half of an unported ask that was answered is held to Java's
//! half too (the strict D refusals, today). The commands: port-scratch's normalizer/fill/x/README.md.

use std::cell::Cell;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs::File;
use std::io::{BufRead, BufReader, BufWriter, Read, Seek, SeekFrom, Write};
use std::panic::AssertUnwindSafe;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc;
use std::time::Instant;

use lite_front::codec::{self, Writer};
use lite_front::doors::{Dialect, PARSE_STACK};
use lite_front::sha256::sha256_utf16le;
use lite_front::wire;
use lite_model::linked::LinkedMap;
use lite_model::{JStr, NormalizedModel, ParsedModel};
use lite_names::model_index::ModelIndex;
use lite_names::{knowledge, resolver};
use lite_normalizer::Thrown;

/// How many mismatches and unported asks a failure lists, and how many histogram rows it prints.
const SHOWN: usize = 25;
const HISTOGRAM_SHOWN: usize = 30;
/// Units of context each side of a first difference.
const CONTEXT: usize = 120;
/// The text an `unported!` body panics with, and the text of a `Thrown::Unported`'s message.
const UNPORTED_PANIC: &str = "UNPORTED: ";
const UNPORTED_MESSAGE: &str = "not ported yet: ";

fn env(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| panic!("{name} names the file to read"))
}

fn env_opt(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|v| !v.is_empty())
}

fn units(s: &str) -> Vec<u16> {
    s.encode_utf16().collect()
}

/// A form being written: UTF-16 units, as Java's `StringBuilder`.
#[derive(Default)]
struct F(Vec<u16>);

impl F {
    fn s(&mut self, s: &str) -> &mut F {
        self.0.extend(s.encode_utf16());
        self
    }
    fn u(&mut self, u: &[u16]) -> &mut F {
        self.0.extend_from_slice(u);
        self
    }
}

// =====================================================================
// One ask, as NormalizerAnswers.form writes it
// =====================================================================

/// `NormalizerAnswers.error`: `E TAB class TAB phase TAB element TAB message` (`-` for no phase or element, `null` for no
/// message, as Java's concatenation of `getMessage()`).
fn error(e: &Thrown) -> Vec<u16> {
    let mut f = F::default();
    f.s("E\t").s(e.java_class()).s("\t");
    f.s(e.phase().map_or("-", |p| p.name())).s("\t");
    match e.element() {
        Some(el) => f.u(&el.0),
        None => f.s("-"),
    };
    f.s("\t");
    match e.message() {
        Some(m) => f.u(&m.0),
        None => f.s("null"),
    };
    f.0
}

/// `NormalizerAnswers.walls`: `[[key,line],...]`, each string as the record codec writes one.
fn walls_json(walls: &LinkedMap<JStr, JStr>) -> Vec<u16> {
    let mut w = Writer::new();
    w.raw("[");
    for (i, (k, v)) in walls.iter().enumerate() {
        if i > 0 {
            w.raw(",");
        }
        w.raw("[");
        w.string(k);
        w.raw(",");
        w.string(v);
        w.raw("]");
    }
    w.raw("]");
    units(&w.finish())
}

/// `NormalizerAnswers.BOOT_FQNS`: the system metamodel's element names and the prelude's.
fn boot_fqns() -> HashSet<JStr> {
    let mut out: HashSet<JStr> = lite_builtin::system_metamodel::element_fqns().iter().cloned().collect();
    out.extend(lite_builtin::prelude::prelude().element_fqns().iter().cloned());
    out
}

/// D then E (`NormalizerAnswers.run`'s try): `walls` none (strict) or the tolerant map, one map through every step.
/// `reached_e` is set as E is called: an error before it is D's.
fn steps(
    parsed: &ParsedModel,
    boot: &HashSet<JStr>,
    mut walls: Option<&mut LinkedMap<JStr, JStr>>,
    reached_e: &mut bool,
) -> Result<NormalizedModel, Thrown> {
    let resolved = resolver::resolve_alongside(parsed, boot, walls.as_deref_mut())?;
    let adopted = knowledge::adopt_association_qualified_properties(&resolved, walls.as_deref_mut())?;
    let index = ModelIndex::from(&adopted)?;
    index.refuse_duplicates(walls.as_deref_mut())?;
    *reached_e = true;
    lite_normalizer::normalize(&adopted, &index, walls)
}

/// Where a run ended.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
enum Stage {
    /// An error before E (D's: the resolver, the adoption, the index, `refuse_duplicates`).
    RefusedInD,
    /// A `NormalizedModel`.
    Answered,
    /// An error E raised.
    RefusedInE,
    /// A panic that is not a stub's: a crash, always a mismatch.
    Panicked,
    /// A body not ported yet.
    Unported,
}

impl Stage {
    const ALL: [Stage; 5] = [
        Stage::RefusedInD,
        Stage::Answered,
        Stage::RefusedInE,
        Stage::Panicked,
        Stage::Unported,
    ];

    fn name(self) -> &'static str {
        match self {
            Stage::RefusedInD => "refused in D",
            Stage::Answered => "answered",
            Stage::RefusedInE => "refused in E",
            Stage::Panicked => "panicked",
            Stage::Unported => "unported",
        }
    }
}

/// One mode's run.
enum Run {
    /// The run's line of the form (`A`, walls and `A`, `E`, or a crash's `!panic`).
    Line { text: Vec<u16>, stage: Stage },
    /// A body not ported yet, by its site; `leaked` when it reached the answer's text (a catch took it) rather than the
    /// top.
    Unported { site: String, leaked: bool },
}

impl Run {
    fn stage(&self) -> Stage {
        match self {
            Run::Line { stage, .. } => *stage,
            Run::Unported { .. } => Stage::Unported,
        }
    }

    fn line(&self) -> Option<&[u16]> {
        match self {
            Run::Line { text, .. } => Some(text),
            Run::Unported { .. } => None,
        }
    }
}

thread_local! {
    /// Whether this thread is inside [`guarded`]: the panic hook keeps a stub's panic quiet there alone.
    static GUARDED: Cell<bool> = const { Cell::new(false) };
}

/// The panic hook, installed once: silent for a stub's `unported!` panic inside [`guarded`], the default hook for every
/// other panic (libtest's own, a crash in a run).
fn install_panic_hook() {
    static HOOK: std::sync::Once = std::sync::Once::new();
    HOOK.call_once(|| {
        let default = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            let quiet =
                GUARDED.with(Cell::get) && panic_text(info.payload()).is_some_and(|m| m.contains(UNPORTED_PANIC));
            if !quiet {
                default(info);
            }
        }));
    });
}

fn panic_text(payload: &(dyn std::any::Any + Send)) -> Option<&str> {
    payload
        .downcast_ref::<String>()
        .map(String::as_str)
        .or_else(|| payload.downcast_ref::<&'static str>().copied())
}

/// `f`, its panic caught.
fn guarded<T>(f: impl FnOnce() -> T) -> std::thread::Result<T> {
    GUARDED.with(|g| g.set(true));
    let r = std::panic::catch_unwind(AssertUnwindSafe(f));
    GUARDED.with(|g| g.set(false));
    r
}

/// The site after `marker` in `text` (`Class.method`, `Class#n`).
fn site_after(text: &str, marker: &str) -> Option<String> {
    let at = text.find(marker)? + marker.len();
    let site: String = text[at..]
        .chars()
        .take_while(|c| c.is_alphanumeric() || matches!(c, '.' | '_' | '#' | '$'))
        .collect();
    Some(if site.is_empty() { "?".to_owned() } else { site })
}

/// One mode of `NormalizerAnswers.run`: strict, or tolerant with its walls map.
fn run(parsed: &ParsedModel, boot: &HashSet<JStr>, tolerant: bool) -> Run {
    let mut walls: LinkedMap<JStr, JStr> = LinkedMap::new();
    let mut reached_e = false;
    let outcome = guarded(|| steps(parsed, boot, tolerant.then_some(&mut walls), &mut reached_e));
    let r = match outcome {
        Ok(Ok(nm)) => {
            let mut f = F::default();
            if !walls.is_empty() {
                f.u(&walls_json(&walls)).s("\t");
            }
            f.s("A\t").s(&codec::write(&nm, true));
            Run::Line {
                text: f.0,
                stage: Stage::Answered,
            }
        }
        Ok(Err(Thrown::Unported(site))) => Run::Unported {
            site: site.to_owned(),
            leaked: false,
        },
        Ok(Err(e)) => Run::Line {
            text: error(&e),
            stage: if reached_e {
                Stage::RefusedInE
            } else {
                Stage::RefusedInD
            },
        },
        Err(payload) => {
            let msg = panic_text(&*payload).unwrap_or("a panic with no message").to_owned();
            match site_after(&msg, UNPORTED_PANIC) {
                Some(site) => Run::Unported { site, leaked: false },
                None => {
                    let mut f = F::default();
                    f.s("!panic\t").s(&msg);
                    Run::Line {
                        text: f.0,
                        stage: Stage::Panicked,
                    }
                }
            }
        }
    };
    // a stub's throw a catch took into a wall or a message: unported still, never a mismatch
    if let Run::Line { text, .. } = &r {
        let shown = String::from_utf16_lossy(text);
        if let Some(site) = site_after(&shown, UNPORTED_MESSAGE)
            .or_else(|| shown.contains(lite_normalizer::error::UNPORTED).then(|| "?".to_owned()))
        {
            return Run::Unported { site, leaked: true };
        }
    }
    r
}

/// `NormalizerAnswers.form`: `S TAB strict NL T TAB tolerant`, the tolerant `=S` when equal.
fn form(s: &[u16], t: &[u16]) -> Vec<u16> {
    let mut f = F::default();
    f.s("S\t").u(s).s("\nT\t");
    if t == s {
        f.s("=S");
    } else {
        f.u(t);
    }
    f.0
}

// =====================================================================
// The Java forms (diagnosis): an index of byte offsets, cached
// =====================================================================

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
                "[normalizer-differential] forms index: {} asks, read from {}",
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
            "[normalizer-differential] forms index: {} asks over {offset} bytes, built in {:.1} s and cached at {}",
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

// =====================================================================
// The locator of a first difference
// =====================================================================

/// Where a mismatch's first difference is: the mode (`S`, `T`; `S-half`/`T-half` for the answered half of an unported
/// ask), the path to the differing value (a JSON path, `error.<field>`, `walls[i][j]`, or `-`), its group (the path with
/// its indices erased and each object's `@` class shown, or the kind of difference), and the context.
struct Locator {
    mode: String,
    path: String,
    group: String,
    detail: String,
}

fn first_diff(a: &[u16], b: &[u16]) -> Option<usize> {
    match a.iter().zip(b).position(|(x, y)| x != y) {
        Some(i) => Some(i),
        None if a.len() == b.len() => None,
        None => Some(a.len().min(b.len())),
    }
}

/// `u` for a one-line TSV cell: tabs, newlines and returns written as their escapes.
fn shown(u: &[u16]) -> String {
    let mut out = String::new();
    for c in char::decode_utf16(u.iter().copied()) {
        match c {
            Ok('\t') => out.push_str("\\t"),
            Ok('\n') => out.push_str("\\n"),
            Ok('\r') => out.push_str("\\r"),
            Ok(c) => out.push(c),
            Err(e) => out.push_str(&format!("\\u{:04x}", e.unpaired_surrogate())),
        }
    }
    out
}

/// `CONTEXT` units each side of `at`, the place marked `▶`.
fn context(u: &[u16], at: usize) -> String {
    let at = at.min(u.len());
    let from = at.saturating_sub(CONTEXT);
    let to = (at + CONTEXT).min(u.len());
    format!(
        "{}{}▶{}{}",
        if from > 0 { "…" } else { "" },
        shown(&u[from..at]),
        shown(&u[at..to]),
        if to < u.len() { "…" } else { "" }
    )
}

/// A line's kind: `A`, `walls+A`, `E(<class>)`, `panic`, `=S`.
fn kind(line: &[u16]) -> String {
    let s = |p: &str| line.starts_with(&units(p));
    if s("A\t") {
        "A".to_owned()
    } else if s("[") {
        "walls+A".to_owned()
    } else if s("E\t") {
        let class: Vec<u16> = line[2..]
            .iter()
            .copied()
            .take_while(|&c| c != u16::from(b'\t'))
            .collect();
        format!("E({})", short_class(&String::from_utf16_lossy(&class)))
    } else if s("!panic") {
        "panic".to_owned()
    } else if s("=S") {
        "=S".to_owned()
    } else {
        "?".to_owned()
    }
}

/// A class's name after its last `.` (`com.legend.error.ModelException` -> `ModelException`).
fn short_class(name: &str) -> &str {
    name.rsplit('.').next().unwrap_or(name)
}

enum Frame {
    Obj {
        key: Vec<u16>,
        tag: Option<Vec<u16>>,
        want_key: bool,
    },
    Arr {
        index: usize,
    },
}

/// The JSON string starting at unit `from` of `u` (its opening quote behind it), up to its closing quote.
fn string_at(u: &[u16], from: usize) -> String {
    let mut out = Vec::new();
    let mut esc = false;
    for &c in u.get(from..).unwrap_or(&[]) {
        if esc {
            esc = false;
        } else if c == u16::from(b'\\') {
            esc = true;
            continue;
        } else if c == u16::from(b'"') {
            break;
        }
        out.push(c);
    }
    String::from_utf16_lossy(&out)
}

/// The `@` class of the object that starts at `at` of `u` (after its comma, if any), short.
fn tag_after(u: &[u16], at: usize) -> Option<String> {
    let rest = u.get(at..)?;
    let rest = rest.strip_prefix(&[u16::from(b',')][..]).unwrap_or(rest);
    let open = units(r#"{"@":""#);
    rest.starts_with(&open)
        .then(|| short_class(&string_at(rest, open.len())).to_owned())
}

/// What a JSON difference at `at` is, past its path: two classes (a difference in an `@` tag, `tag_from` the tag's first
/// unit), or one side's container running on (`java has more: <Class>`).
fn json_difference(java: &[u16], rust: &[u16], at: usize, tag_from: Option<usize>) -> String {
    if let Some(from) = tag_from {
        return format!(
            " java={} rust={}",
            short_class(&string_at(java, from)),
            short_class(&string_at(rust, from))
        );
    }
    let close = |c: Option<&u16>| c.is_some_and(|&c| c == u16::from(b']') || c == u16::from(b'}'));
    let (j, r) = (java.get(at), rust.get(at));
    let more = |side: &str, u: &[u16]| match tag_after(u, at) {
        Some(tag) => format!(" ({side} has more: {tag})"),
        None => format!(" ({side} has more)"),
    };
    if close(r) && !close(j) {
        more("java", java)
    } else if close(j) && !close(r) {
        more("rust", rust)
    } else {
        String::new()
    }
}

/// The JSON path at unit `at` of two JSON texts equal before it, with its group: `elements[3].classBindings[0].function`
/// and `elements[]<MappingDefinition>.classBindings[]<...>.function`, the group saying what differs there (two classes,
/// a longer side).
fn json_path(java: &[u16], rust: &[u16], at: usize, root: &str) -> (String, String) {
    let json = java;
    #[derive(PartialEq)]
    enum Capture {
        None,
        Key,
        Tag,
    }
    let q = u16::from(b'"');
    let mut stack: Vec<Frame> = Vec::new();
    let (mut in_str, mut esc) = (false, false);
    let mut capture = Capture::None;
    let mut buf: Vec<u16> = Vec::new();
    let mut str_from = 0usize;
    for (i, &c) in json[..at.min(json.len())].iter().enumerate() {
        if in_str {
            if esc {
                esc = false;
                if capture != Capture::None {
                    buf.push(c);
                }
            } else if c == u16::from(b'\\') {
                esc = true;
            } else if c == q {
                in_str = false;
                if let Some(Frame::Obj { key, tag, .. }) = stack.last_mut() {
                    match capture {
                        Capture::Key => *key = std::mem::take(&mut buf),
                        Capture::Tag => *tag = Some(std::mem::take(&mut buf)),
                        Capture::None => {}
                    }
                }
                capture = Capture::None;
            } else if capture != Capture::None {
                buf.push(c);
            }
            continue;
        }
        match u8::try_from(c).unwrap_or(0) {
            b'"' => {
                in_str = true;
                str_from = i + 1;
                buf.clear();
                capture = match stack.last() {
                    Some(Frame::Obj { want_key: true, .. }) => Capture::Key,
                    Some(Frame::Obj { key, .. }) if *key == [u16::from(b'@')] => Capture::Tag,
                    _ => Capture::None,
                };
            }
            b'{' => stack.push(Frame::Obj {
                key: Vec::new(),
                tag: None,
                want_key: true,
            }),
            b'[' => stack.push(Frame::Arr { index: 0 }),
            b'}' | b']' => {
                stack.pop();
            }
            b':' => {
                if let Some(Frame::Obj { want_key, .. }) = stack.last_mut() {
                    *want_key = false;
                }
            }
            b',' => match stack.last_mut() {
                Some(Frame::Obj { want_key, .. }) => *want_key = true,
                Some(Frame::Arr { index }) => *index += 1,
                None => {}
            },
            _ => {}
        }
    }
    // a difference inside a key: the key read so far
    if in_str
        && capture == Capture::Key
        && let Some(Frame::Obj { key, .. }) = stack.last_mut()
    {
        *key = buf;
        key.extend(units("…"));
    }
    let (mut path, mut group) = (root.to_owned(), root.to_owned());
    for frame in &stack {
        match frame {
            Frame::Obj { key, tag, .. } => {
                if let Some(tag) = tag {
                    group.push_str(&format!("<{}>", short_class(&String::from_utf16_lossy(tag))));
                }
                if !key.is_empty() {
                    let key = String::from_utf16_lossy(key);
                    for p in [&mut path, &mut group] {
                        if !p.is_empty() {
                            p.push('.');
                        }
                        p.push_str(&key);
                    }
                }
            }
            Frame::Arr { index } => {
                path.push_str(&format!("[{index}]"));
                group.push_str("[]");
            }
        }
    }
    if path.is_empty() {
        path.push('-');
        group.push_str("(the JSON's start)");
    }
    let tag_from = (in_str && capture == Capture::Tag).then_some(str_from);
    group.push_str(&json_difference(java, rust, at, tag_from));
    (path, group)
}

/// The first raw tab of `line` (where a walls prefix ends: the walls' strings escape theirs).
fn first_tab(line: &[u16]) -> Option<usize> {
    line.iter().position(|&c| c == u16::from(b'\t'))
}

/// The path and group of a first difference at `d` between two lines of one mode (Java's may run on past its end).
fn classify(java: &[u16], rust: &[u16], d: usize) -> (String, String) {
    let (jk, rk) = (kind(java), kind(rust));
    if rk == "panic" {
        let msg: String = shown(rust).chars().skip("!panic\\t".len()).take(100).collect();
        return ("-".to_owned(), format!("rust panicked: {msg}"));
    }
    if jk != rk {
        return ("-".to_owned(), format!("kind: java={jk} rust={rk}"));
    }
    match jk.as_str() {
        "A" if d >= 2 => json_path(&java[2..], &rust[2..], d - 2, ""),
        "walls+A" => {
            let (wj, wr) = (
                first_tab(java).unwrap_or(java.len()),
                first_tab(rust).unwrap_or(rust.len()),
            );
            if wj == wr && d >= wj + 3 {
                let after = |u: &[u16]| -> Vec<u16> { u.get(wj + 3..).unwrap_or(&[]).to_vec() };
                json_path(&after(java), &after(rust), d - wj - 3, "")
            } else {
                json_path(&java[..wj], &rust[..wr], d.min(wj), "walls")
            }
        }
        _ if jk.starts_with("E(") => {
            let field = java[..d.min(java.len())]
                .iter()
                .filter(|&&c| c == u16::from(b'\t'))
                .count();
            match field {
                0 | 1 => ("error.class".to_owned(), format!("error.class: java={jk} rust={rk}")),
                2 => ("error.phase".to_owned(), format!("error.phase ({jk})")),
                3 => ("error.element".to_owned(), format!("error.element ({jk})")),
                _ => ("error.message".to_owned(), format!("error.message ({jk})")),
            }
        }
        _ => ("-".to_owned(), format!("kind {jk}")),
    }
}

/// The first difference between Java's form and the one Rust writes from `s` and `t`.
fn locate(java: &[u16], s: &[u16], t: &[u16]) -> Locator {
    let rust = form(s, t);
    let Some(d) = first_diff(java, &rust) else {
        return Locator {
            mode: "-".to_owned(),
            path: "-".to_owned(),
            group: "the forms are equal: the frozen hash is not this form's".to_owned(),
            detail: "-".to_owned(),
        };
    };
    let s_end = 2 + s.len();
    let nt = units("\nT\t");
    if d < 2 {
        return Locator {
            mode: "?".to_owned(),
            path: "-".to_owned(),
            group: "the form's start".to_owned(),
            detail: format!("at {d}: java {} ‖ rust {}", context(java, d), context(&rust, d)),
        };
    }
    if d <= s_end || java.get(s_end..s_end + 3) != Some(&nt[..]) {
        let (path, group) = classify(&java[2..], s, d - 2);
        return Locator {
            mode: "S".to_owned(),
            path,
            group,
            detail: format!("at {d}: java {} ‖ rust {}", context(java, d), context(&rust, d)),
        };
    }
    let eq_s = units("=S");
    let jt = &java[s_end + 3..];
    let jt = if jt == eq_s { s } else { jt };
    let d2 = first_diff(jt, t).unwrap_or(0);
    let (path, group) = classify(jt, t, d2);
    Locator {
        mode: "T".to_owned(),
        path,
        group,
        detail: format!(
            "at {d2} of the tolerant line (=S expanded): java {} ‖ rust {}",
            context(jt, d2),
            context(t, d2)
        ),
    }
}

/// The answered half of an unported ask held to Java's: `None` when equal, else its locator (`Some(None)`: Java's half
/// is `=S`, which the unported strict half cannot check).
fn half_check(java: &[u16], run: &Run, strict: bool) -> Option<Option<Locator>> {
    let line = run.line()?;
    if strict {
        let mut f = F::default();
        f.s("S\t").u(line).s("\nT\t");
        let d = first_diff(java, &f.0)?;
        if d >= f.0.len() {
            return None;
        }
        let (path, group) = classify(&java[2.min(java.len())..], line, d.saturating_sub(2));
        return Some(Some(Locator {
            mode: "S-half".to_owned(),
            path,
            group,
            detail: format!("at {d}: java {} ‖ rust {}", context(java, d), context(&f.0, d)),
        }));
    }
    let nt = units("\nT\t");
    let at = java.windows(3).position(|w| w == &nt[..])?;
    let jt = &java[at + 3..];
    if jt == units("=S") {
        return Some(None);
    }
    let d = first_diff(jt, line)?;
    let (path, group) = classify(jt, line, d);
    Some(Some(Locator {
        mode: "T-half".to_owned(),
        path,
        group,
        detail: format!("at {d}: java {} ‖ rust {}", context(jt, d), context(line, d)),
    }))
}

// =====================================================================
// The harness
// =====================================================================

struct FrozenLine {
    sha: String,
    dialect: String,
    hash: String,
}

fn read_frozen(path: &str) -> Vec<FrozenLine> {
    BufReader::new(File::open(path).unwrap_or_else(|e| panic!("the frozen answers {path}: {e}")))
        .lines()
        .map(|line| {
            let line = line.unwrap();
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
    /// How many mismatches' forms the report writes (`LITE_NORMALIZER_REPORT_FORMS`; every one by default).
    report_forms: usize,
}

/// The answered half of an unported ask, held to Java's.
#[derive(Default, Clone, Copy)]
struct HalfCounts {
    equal: usize,
    differ: usize,
    unchecked: usize,
}

/// One ask's outcome, as a worker hands it on.
struct AskOut {
    sha: String,
    dialect: String,
    s: Stage,
    t: Stage,
    /// The unported sites reached, with the modes that reached each (`S`, `T`) and whether one leaked into a text.
    sites: Vec<(String, bool, bool)>,
    leaked: bool,
    verdict: Verdict,
    /// The answered half of an unported ask, checked against the Java form: (strict?, its locator if it differs;
    /// `None` inside when uncheckable).
    half: Option<(bool, Option<Option<Locator>>)>,
    /// For the report: the Java form (when the forms are given) and the Rust one.
    forms: Option<(Option<Vec<u16>>, Vec<u16>)>,
}

enum Verdict {
    Equal,
    Mismatch(Locator),
    Unported,
}

/// A worker's view of everything it needs, shared.
struct Shared<'a> {
    boot: HashSet<JStr>,
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
    let s = run(parsed, &sh.boot, false);
    let t = run(parsed, &sh.boot, true);
    let key: Key = (sha, dialect);
    let mut sites: Vec<(String, bool, bool)> = Vec::new();
    let mut leaked = false;
    for (r, strict) in [(&s, true), (&t, false)] {
        if let Run::Unported { site, leaked: l } = r {
            leaked |= l;
            match sites.iter_mut().find(|(x, _, _)| x == site) {
                Some(entry) => {
                    entry.1 |= strict;
                    entry.2 |= !strict;
                }
                None => sites.push((site.clone(), strict, !strict)),
            }
        }
    }
    let mut out = AskOut {
        sha: key.0.clone(),
        dialect: key.1.clone(),
        s: s.stage(),
        t: t.stage(),
        sites,
        leaked,
        verdict: Verdict::Unported,
        half: None,
        forms: None,
    };
    let (Some(sl), Some(tl)) = (s.line(), t.line()) else {
        // unported: the answered half (if any) held to Java's half
        let strict = s.line().is_some();
        if let Some(forms) = sh.forms
            && (strict || t.line().is_some())
            && let Some(java) = forms.form(java_file, &key)
        {
            {
                let checked = half_check(&java, if strict { &s } else { &t }, strict);
                let differs = matches!(checked, Some(Some(_)));
                out.half = Some((strict, checked));
                if differs && sh.report {
                    let mut f = F::default();
                    f.s("S\t");
                    match s.line() {
                        Some(l) => f.u(l),
                        None => f.s("<unported>"),
                    };
                    f.s("\nT\t");
                    match t.line() {
                        Some(l) => f.u(l),
                        None => f.s("<unported>"),
                    };
                    out.forms = Some((Some(java), f.0));
                }
            }
        }
        return out;
    };
    let rust = form(sl, tl);
    let hash = sha256_utf16le(&rust);
    let frozen = sh.hashes.get(&key).copied();
    if frozen == Some(hash.as_str()) {
        out.verdict = Verdict::Equal;
        return out;
    }
    let java = sh.forms.and_then(|f| f.form(java_file, &key));
    let mut locator = match (&java, frozen) {
        (_, None) => Locator {
            mode: "-".to_owned(),
            path: "-".to_owned(),
            group: "not among the frozen answers".to_owned(),
            detail: "-".to_owned(),
        },
        (Some(java), Some(_)) => locate(java, sl, tl),
        (None, Some(_)) => Locator {
            mode: "?".to_owned(),
            path: "-".to_owned(),
            group: format!(
                "(no Java form) rust S={} T={}",
                kind(sl),
                if tl == sl { "=S".to_owned() } else { kind(tl) }
            ),
            detail: "-".to_owned(),
        },
    };
    if let (Some(java), Some(h)) = (&java, frozen)
        && sha256_utf16le(java) != h
    {
        locator.detail = format!("(the forms file's form is not the frozen answer's) {}", locator.detail);
    }
    out.verdict = Verdict::Mismatch(locator);
    if sh.report {
        out.forms = Some((java, rust));
    }
    out
}

/// An unported site's asks: (all, by the strict run, by the tolerant run).
type Reach = (usize, usize, usize);
/// A mismatch group's asks: (all, in S, in T, an example).
type Group = (usize, usize, usize, String);

/// What the collector counts.
#[derive(Default)]
struct Tally {
    asked: usize,
    equal: usize,
    mismatch: usize,
    unported: usize,
    leaked: usize,
    /// per mode (0 strict, 1 tolerant): the asks by where the run ended
    stages: [BTreeMap<Stage, usize>; 2],
    /// site -> (asks, by strict, by tolerant)
    sites: BTreeMap<String, Reach>,
    /// group -> (asks, in S, in T, an example)
    groups: BTreeMap<String, Group>,
    halves: [HalfCounts; 2],
    shown_mismatches: Vec<String>,
    shown_unported: Vec<String>,
    forms_written: usize,
}

/// The report's per-ask files: the locators of the mismatches (and of the differing answered halves), and every ask's
/// verdict with where each run ended.
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
        boot: boot_fqns(),
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
        writeln!(r.locators, "# sha\tdialect\tmode\tpath\tgroup\tdetail").unwrap();
        writeln!(r.asks, "# sha\tdialect\tverdict\tstrict\ttolerant\tunported sites").unwrap();
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
                .name(format!("normalizer-differential-{w}"))
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
    if opts.only.is_none() {
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
fn collect(tally: &mut Tally, opts: &Opts<'_>, a: AskOut, mut report: Option<&mut Report>) {
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
    *tally.stages[0].entry(a.s).or_default() += 1;
    *tally.stages[1].entry(a.t).or_default() += 1;
    let who = format!("{} {}", a.dialect, a.sha);
    let mut locator_line = None;
    match &a.verdict {
        Verdict::Equal => tally.equal += 1,
        Verdict::Unported => {
            tally.unported += 1;
            if a.leaked {
                tally.leaked += 1;
            }
            for (site, strict, tolerant) in &a.sites {
                let e = tally.sites.entry(site.clone()).or_default();
                e.0 += 1;
                e.1 += usize::from(*strict);
                e.2 += usize::from(*tolerant);
            }
            if tally.shown_unported.len() < SHOWN {
                let sites: Vec<&str> = a.sites.iter().map(|(s, _, _)| s.as_str()).collect();
                tally.shown_unported.push(format!("{who} at {}", sites.join(", ")));
            }
            if let Some((strict, checked)) = &a.half {
                let h = &mut tally.halves[usize::from(!strict)];
                match checked {
                    None => h.equal += 1,
                    Some(None) => h.unchecked += 1,
                    Some(Some(l)) => {
                        h.differ += 1;
                        locator_line = Some(l);
                    }
                }
            }
        }
        Verdict::Mismatch(l) => {
            tally.mismatch += 1;
            if tally.shown_mismatches.len() < SHOWN {
                tally
                    .shown_mismatches
                    .push(format!("{who} {} {} ({})", l.mode, l.path, l.group));
            }
            locator_line = Some(l);
        }
    }
    if let Some(r) = report.as_deref_mut() {
        let verdict = match &a.verdict {
            Verdict::Equal => "equal",
            Verdict::Mismatch(_) => "mismatch",
            Verdict::Unported => "unported",
        };
        let sites: Vec<&str> = a.sites.iter().map(|(s, _, _)| s.as_str()).collect();
        writeln!(
            r.asks,
            "{}\t{}\t{verdict}\t{}\t{}\t{}",
            a.sha,
            a.dialect,
            a.s.name(),
            a.t.name(),
            if sites.is_empty() {
                "-".to_owned()
            } else {
                sites.join(",")
            }
        )
        .unwrap();
    }
    if let Some(l) = locator_line {
        let g = tally
            .groups
            .entry(format!(
                "{}{}",
                if l.mode.ends_with("-half") { "half: " } else { "" },
                l.group
            ))
            .or_insert_with(|| (0, 0, 0, who.clone()));
        g.0 += 1;
        g.1 += usize::from(l.mode.starts_with('S'));
        g.2 += usize::from(l.mode.starts_with('T'));
        if let Some(r) = report {
            writeln!(
                r.locators,
                "{}\t{}\t{}\t{}\t{}\t{}",
                a.sha, a.dialect, l.mode, l.path, l.group, l.detail
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
}

fn print_tally(opts: &Opts<'_>, t: &Tally, seconds: f64) {
    let p = |s: String| println!("[normalizer-differential] {s}");
    p(format!(
        "{}: {} asks ({} frozen) in {seconds:.1} s on {} threads: {} equal, {} mismatch, {} unported{}",
        opts.label,
        t.asked,
        opts.frozen.len(),
        opts.threads,
        t.equal,
        t.mismatch,
        t.unported,
        if t.leaked > 0 {
            format!(
                " ({} leaked into a wall or a message: a catch took a stub's throw)",
                t.leaked
            )
        } else {
            String::new()
        }
    ));
    for (m, name) in [(0, "strict"), (1, "tolerant")] {
        let row: Vec<String> = Stage::ALL
            .iter()
            .filter_map(|s| t.stages[m].get(s).map(|n| format!("{} {n}", s.name())))
            .collect();
        p(format!("  {name} runs: {}", row.join(", ")));
    }
    if opts.forms.is_some() {
        for (m, name) in [(0, "strict"), (1, "tolerant")] {
            let h = t.halves[m];
            if h.equal + h.differ + h.unchecked > 0 {
                p(format!(
                    "  the {name} half answered beside an unported half, held to Java's: {} equal, {} differ, {} \
                     uncheckable (Java's =S)",
                    h.equal, h.differ, h.unchecked
                ));
            }
        }
    }
    if !t.sites.is_empty() {
        p(format!(
            "  unported asks by stub site ({} sites; asks, strict, tolerant):",
            t.sites.len()
        ));
        for (site, (n, s, tt)) in histogram(t).into_iter().take(HISTOGRAM_SHOWN) {
            p(format!("    {n:>6}  {site}  (S {s}, T {tt})"));
        }
    }
    if !t.groups.is_empty() {
        p("  mismatches by first difference (asks, in S, in T; an example):".to_owned());
        for (group, (n, s, tt, ex)) in groups(t).into_iter().take(HISTOGRAM_SHOWN) {
            p(format!("    {n:>6}  {group}  (S {s}, T {tt}; {ex})"));
        }
    }
}

/// The unported histogram, most hit first.
fn histogram(t: &Tally) -> Vec<(&String, Reach)> {
    let mut rows: Vec<(&String, Reach)> = t.sites.iter().map(|(k, v)| (k, *v)).collect();
    rows.sort_by(|a, b| b.1.0.cmp(&a.1.0).then_with(|| a.0.cmp(b.0)));
    rows
}

/// The mismatch groups, largest first.
fn groups(t: &Tally) -> Vec<(&String, &Group)> {
    let mut rows: Vec<(&String, &Group)> = t.groups.iter().collect();
    rows.sort_by(|a, b| b.1.0.cmp(&a.1.0).then_with(|| a.0.cmp(b.0)));
    rows
}

fn write_summary(dir: &Path, t: &Tally) {
    let mut f = BufWriter::new(File::create(dir.join("summary.tsv")).unwrap());
    writeln!(
        f,
        "# asks\tin S\tin T\tfirst difference (path, indices erased; or kind)\texample"
    )
    .unwrap();
    for (group, (n, s, tt, ex)) in groups(t) {
        writeln!(f, "{n}\t{s}\t{tt}\t{group}\t{ex}").unwrap();
    }
    let mut u = BufWriter::new(File::create(dir.join("unported.tsv")).unwrap());
    writeln!(u, "# asks\tstrict\ttolerant\tsite").unwrap();
    for (site, (n, s, tt)) in histogram(t) {
        writeln!(u, "{n}\t{s}\t{tt}\t{site}").unwrap();
    }
}

/// The failure, listing what differs and what is unported (bounded), with the counts.
fn verdict(label: &str, t: &Tally) {
    let mut lines = Vec::new();
    for m in &t.shown_mismatches {
        lines.push(format!("  mismatch: {m}"));
    }
    for u in &t.shown_unported {
        lines.push(format!("  unported: {u}"));
    }
    for (site, (n, _, _)) in histogram(t).into_iter().take(SHOWN) {
        lines.push(format!("  unported at {site}: {n} asks"));
    }
    let halves = t.halves[0].differ + t.halves[1].differ;
    assert!(
        t.mismatch == 0 && t.unported == 0 && halves == 0,
        "{label}: of {} asks, {} equal, {} answer otherwise than Java's, {} reach a body not ported yet{}\n{}",
        t.asked,
        t.equal,
        t.mismatch,
        t.unported,
        if halves > 0 {
            format!(", {halves} answered halves of unported asks differ")
        } else {
            String::new()
        },
        lines.join("\n")
    );
}

fn report_forms() -> usize {
    env_opt("LITE_NORMALIZER_REPORT_FORMS").map_or(usize::MAX, |n| {
        n.parse().expect("LITE_NORMALIZER_REPORT_FORMS is a count")
    })
}

fn threads() -> usize {
    env_opt("LITE_NORMALIZER_THREADS").map_or_else(
        || std::thread::available_parallelism().map_or(1, |n| n.get()).min(4),
        |n| n.parse::<usize>().expect("LITE_NORMALIZER_THREADS is a count").max(1),
    )
}

/// The diagnosis inputs, by env: the Java forms (indexed once, the index cached) and the report directory.
fn diagnosis(sub: &str) -> (Option<Forms>, Option<PathBuf>) {
    let report = env_opt("LITE_NORMALIZER_REPORT").map(|d| PathBuf::from(d).join(sub));
    let forms = env_opt("LITE_NORMALIZER_FORMS").map(|path| {
        let cache = env_opt("LITE_NORMALIZER_FORMS_INDEX")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                let dir = report.as_ref().and_then(|r| r.parent().map(Path::to_owned)).expect(
                    "LITE_NORMALIZER_FORMS needs LITE_NORMALIZER_FORMS_INDEX or LITE_NORMALIZER_REPORT for its index",
                );
                std::fs::create_dir_all(&dir).unwrap();
                dir.join("forms.idx")
            });
        Forms::open(Path::new(&path), &cache)
    });
    (forms, report)
}

#[test]
#[ignore = "reads the front oracle's questions, the cases and the frozen answers (LITE_NORMALIZER_*): \
            //rust/lite-normalizer:normalizer_differential_test runs it"]
fn what_step_e_hands_on_answers_as_javas() {
    let frozen = read_frozen(&env("LITE_NORMALIZER_ANSWERS"));
    let (forms, report) = diagnosis("full");
    let opts = Opts {
        label: "full",
        texts: vec![env("LITE_NORMALIZER_QUESTIONS"), env("LITE_NORMALIZER_CASES")],
        frozen: &frozen,
        limit: env_opt("LITE_NORMALIZER_LIMIT")
            .map_or(usize::MAX, |n| n.parse().expect("LITE_NORMALIZER_LIMIT is a count")),
        only: env_opt("LITE_NORMALIZER_ONLY"),
        threads: threads(),
        forms: forms.as_ref(),
        report,
        report_forms: report_forms(),
    };
    let tally = differential(&opts);
    verdict("the normalizer differential", &tally);
}

/// The slice's own cases alone (rust/oracles/normalizer/cases.jsonl), held to the frozen lines of their asks -- the
/// answers' last lines. Ignored while any case reaches a body not ported yet (2026-11-14: every case does, at
/// `ModelNormalizer.requireQualifiedPropertiesAdopted`); un-ignored, it reads the decompressed answers at
/// `LITE_NORMALIZER_ANSWERS`, or else decompresses the repository's answers.tsv.zst with the `zstd` command.
#[test]
#[ignore = "every case reaches a body not ported yet (2026-11-14); reads LITE_NORMALIZER_ANSWERS and the cases"]
fn the_slices_cases_answer_as_javas() {
    let manifest = option_env!("CARGO_MANIFEST_DIR").unwrap_or(".");
    let cases =
        env_opt("LITE_NORMALIZER_CASES").unwrap_or_else(|| format!("{manifest}/../oracles/normalizer/cases.jsonl"));
    let frozen = match env_opt("LITE_NORMALIZER_ANSWERS") {
        Some(path) => read_frozen(&path),
        None => {
            let zst = format!("{manifest}/../oracles/normalizer/answers.tsv.zst");
            let out = std::process::Command::new("zstd")
                .args(["-dc", &zst])
                .output()
                .unwrap_or_else(|e| panic!("no LITE_NORMALIZER_ANSWERS, and `zstd -dc {zst}` did not run: {e}"));
            assert!(out.status.success(), "zstd -dc {zst} failed");
            let dir = std::env::temp_dir().join(format!("lite-normalizer-answers-{}", std::process::id()));
            std::fs::create_dir_all(&dir).unwrap();
            let path = dir.join("answers.tsv");
            std::fs::write(&path, out.stdout).unwrap();
            let frozen = read_frozen(path.to_str().unwrap());
            std::fs::remove_dir_all(&dir).ok();
            frozen
        }
    };
    let shas: HashSet<String> = std::fs::read_to_string(&cases)
        .unwrap()
        .lines()
        .filter(|l| !l.is_empty())
        .map(|l| {
            wire::parse(l, 64)
                .unwrap()
                .get("sha")
                .and_then(wire::Json::as_string)
                .unwrap()
        })
        .collect();
    let from = frozen
        .iter()
        .position(|l| shas.contains(&l.sha))
        .expect("the cases' asks are frozen");
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
    verdict("the normalizer cases", &tally);
}

// =====================================================================
// The harness's own pieces (no oracle needed)
// =====================================================================

#[test]
fn a_stubs_panic_is_unported_at_its_site_and_any_other_is_a_crash() {
    install_panic_hook();
    let stub = guarded(|| -> u8 { unimplemented!("UNPORTED: MappingNormalizer.synthM2M") }).unwrap_err();
    assert_eq!(
        site_after(panic_text(&*stub).unwrap(), UNPORTED_PANIC).as_deref(),
        Some("MappingNormalizer.synthM2M")
    );
    let crash = guarded(|| -> u8 { panic!("index out of bounds") }).unwrap_err();
    assert_eq!(site_after(panic_text(&*crash).unwrap(), UNPORTED_PANIC), None);
    assert_eq!(
        site_after(
            "E\tlite.normalizer.Unported\t-\t-\tnot ported yet: LiftedViews#2",
            UNPORTED_MESSAGE
        )
        .as_deref(),
        Some("LiftedViews#2")
    );
}

#[test]
fn the_locator_names_the_mode_the_path_and_the_cause() {
    let java = units(
        "S\tA\t{\"elements\":[{\"@\":\"model.ClassDefinition\",\"qualifiedName\":\"a::B\"},{\"@\":\"model.\
         MappingDefinition\",\"classBindings\":[{\"@\":\"model.X\",\"function\":\"f\"}]}]}\nT\t=S",
    );
    let s = units(
        "A\t{\"elements\":[{\"@\":\"model.ClassDefinition\",\"qualifiedName\":\"a::B\"},{\"@\":\"model.\
         MappingDefinition\",\"classBindings\":[{\"@\":\"model.X\",\"function\":\"g\"}]}]}",
    );
    let l = locate(&java, &s, &s);
    assert_eq!(l.mode, "S");
    assert_eq!(l.path, "elements[1].classBindings[0].function");
    assert_eq!(l.group, "elements[]<MappingDefinition>.classBindings[]<X>.function");
    // two classes at one place
    let other = units("A\t{\"elements\":[{\"@\":\"model.LegacyMappingDefinition\"}]}");
    let java = units("S\tA\t{\"elements\":[{\"@\":\"model.MappingDefinition\"}]}\nT\t=S");
    let l = locate(&java, &other, &other);
    assert_eq!(
        l.group,
        "elements[].@ java=MappingDefinition rust=LegacyMappingDefinition"
    );
    // Java's list runs on; the tolerant line's walls; the =S expanded
    let short = units("A\t{\"elements\":[{\"@\":\"model.A\"}]}");
    let java = units("S\tA\t{\"elements\":[{\"@\":\"model.A\"},{\"@\":\"model.FunctionDefinition\"}]}\nT\t=S");
    assert_eq!(
        locate(&java, &short, &short).group,
        "elements[] (java has more: FunctionDefinition)"
    );
    let walled = units("[[\"a::M\",\"one\"]]\tA\t{\"elements\":[{\"@\":\"model.A\"}]}");
    let java = units("S\tA\t{\"elements\":[{\"@\":\"model.A\"}]}\nT\t[[\"a::M\",\"two\"]]\tA\t{\"elements\":[]}");
    let l = locate(&java, &short, &walled);
    assert_eq!((l.mode.as_str(), l.path.as_str()), ("T", "walls[0][1]"));
    // an error's message; a kind
    let e = units("E\tcom.legend.error.ModelException\tNORMALIZE\ta::M\tone");
    let java = units("S\tE\tcom.legend.error.ModelException\tNORMALIZE\ta::M\ttwo\nT\t=S");
    assert_eq!(locate(&java, &e, &e).group, "error.message (E(ModelException))");
    assert_eq!(
        locate(&java, &short, &short).group,
        "kind: java=E(ModelException) rust=A"
    );
}
