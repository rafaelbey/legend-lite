// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! legend-lite's compiler as a C library (docs/rust-port/ROADMAP.md, "The shim"): the exports the JVM's shim
//! (`com.legend.ffi.Lite`) calls today and Python's bindings call once `libcompiler` is Rust (phase 4).
//!
//! The calling convention is today's libcompiler's, less GraalVM's isolate: every argument is a NUL-terminated UTF-8
//! C string, every answer a newly allocated one the caller frees with [`lite_free`], and [`lite_unfreed`] counts the
//! answers not yet freed, so a host can check it frees every one. An answer is folded the way `planner.Folded` folds
//! the boundary's: `OK\n<answer>`, or `ERR\n<kind>\n<phase>\n<line:col>\n<message>` -- the kind Rust's own (never a
//! Java class name), the phase and the position `-` where they do not apply, the message the rest of the text.
//!
//! No export lets a panic out: an unwind that reached the `extern "C"` boundary would abort the host process, so every
//! one is caught and answered as an `INTERNAL` refusal.
//!
//! Phase 0 exported only the contract itself -- a version, an echo, and two probes that refuse and panic on purpose --
//! so the shim and its test could hold the whole round trip before any compiler code landed. Phase 1 adds
//! [`lite_dialect`], the dialects' seam (`rust/lite-dialect/SEAM.md`): one op of a dialect, its answer JSON, its
//! refusal's kind folded from the Java class the dialect threw.

use std::any::Any;
use std::cell::RefCell;
use std::ffi::{CStr, CString, c_char};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::mpsc::{self, Sender};

/// The answers handed out and not yet given back to [`lite_free`].
static UNFREED: AtomicI64 = AtomicI64::new(0);

/// What an export answers, before it is folded into text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Answer {
    Ok(String),
    Err(Refusal),
}

/// A refusal: what the compiler declines, and where.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refusal {
    /// Rust's own kind: `PARSER`, `COMPILATION`, `INVALID_ARGUMENT`, `INTERNAL`, ...
    pub kind: String,
    /// The compiler phase (`PARSE`, `TYPE`, ...), when there is one.
    pub phase: Option<String>,
    /// The source position, 1-based line and column, when there is one.
    pub position: Option<(u32, u32)>,
    pub message: String,
}

impl Refusal {
    pub fn new(kind: &str, message: impl Into<String>) -> Refusal {
        Refusal {
            kind: kind.to_owned(),
            phase: None,
            position: None,
            message: message.into(),
        }
    }

    fn invalid_argument(message: impl Into<String>) -> Refusal {
        Refusal::new("INVALID_ARGUMENT", message)
    }
}

/// The answer as text: `OK\n<answer>` or `ERR\n<kind>\n<phase>\n<line:col>\n<message>`.
pub fn fold(answer: &Answer) -> String {
    match answer {
        Answer::Ok(text) => format!("OK\n{text}"),
        Answer::Err(r) => {
            let phase = r.phase.as_deref().unwrap_or("-");
            let position = match r.position {
                Some((line, column)) => format!("{line}:{column}"),
                None => "-".to_owned(),
            };
            format!("ERR\n{}\n{phase}\n{position}\n{}", r.kind, r.message)
        }
    }
}

/// Runs an export's body, catching a panic, and hands its folded answer to the caller.
fn respond(body: impl FnOnce() -> Answer) -> *mut c_char {
    let answer = match catch_unwind(AssertUnwindSafe(body)) {
        Ok(answer) => answer,
        Err(payload) => Answer::Err(Refusal::new("INTERNAL", panic_message(payload.as_ref()))),
    };
    into_c(fold(&answer))
}

/// A panic's message, when it carried one.
fn panic_message(payload: &(dyn Any + Send)) -> String {
    payload
        .downcast_ref::<&str>()
        .map(|s| (*s).to_owned())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "a panic with no message".to_owned())
}

/// The stack a dialect call needs: a MIR nests thousands deep (the golden dump's deepest statement needs more than
/// 1 MB in a debug build), and the thread the host calls from may have 1 MB (a JVM thread's default).
pub const DIALECT_STACK_BYTES: usize = 256 << 20;

/// The stack of a calling thread's worker ([`on_big_stack`]): the deeper of the calls it runs, a dialect's
/// ([`DIALECT_STACK_BYTES`]) and a parser door's (`lite_front::doors::PARSE_STACK`). Reserved, not committed.
const WORKER_STACK_BYTES: usize = if DIALECT_STACK_BYTES > lite_front::doors::PARSE_STACK {
    DIALECT_STACK_BYTES
} else {
    lite_front::doors::PARSE_STACK
};

/// One call for a worker: it runs the body and sends the answer back itself.
type Job = Box<dyn FnOnce() + Send>;

thread_local! {
    /// The calling thread's worker, made on its first call: the sender of its jobs, dropped when the calling thread
    /// ends, which ends the worker.
    static WORKER: RefCell<Option<Sender<Job>>> = const { RefCell::new(None) };
}

/// A worker: a thread of [`WORKER_STACK_BYTES`] that runs its jobs in order until their sender is dropped.
fn worker() -> Result<Sender<Job>, String> {
    let (jobs, inbox) = mpsc::channel::<Job>();
    std::thread::Builder::new()
        .name("lite-worker".to_owned())
        .stack_size(WORKER_STACK_BYTES)
        .spawn(move || inbox.into_iter().for_each(|job| job()))
        .map_err(|e| format!("the lite-worker thread did not start: {e}"))?;
    Ok(jobs)
}

/// `body` on the calling thread's worker, a thread of its own with [`WORKER_STACK_BYTES`] of stack, made on the
/// calling thread's first call and kept for its next ones (a thread per call cost about 35 µs: docs/rust-port/
/// PHASE2.md, step 3), waited for before this returns: its answer, or the message of its panic (caught there, so the
/// worker outlives it) or of the worker's failure to start. A stack overflow is no panic -- it aborts the host -- so
/// the stack is sized for the deepest input. What a call commits of the stack stays committed while its worker lives.
fn on_big_stack<T: Send + 'static>(body: impl FnOnce() -> T + Send + 'static) -> Result<T, String> {
    let (reply, answer) = mpsc::sync_channel(1);
    let mut job: Job = Box::new(move || {
        // the caller waits on this answer, so a failed send has no one to tell
        let _ = reply.send(catch_unwind(AssertUnwindSafe(body)).map_err(|payload| panic_message(payload.as_ref())));
    });
    WORKER.with(|slot| -> Result<(), String> {
        let mut slot = slot.borrow_mut();
        // a worker that has ended hands the job back, and a new one takes it
        loop {
            let jobs = match slot.take() {
                Some(jobs) => jobs,
                None => worker()?,
            };
            match jobs.send(job) {
                Ok(()) => {
                    *slot = Some(jobs);
                    return Ok(());
                }
                Err(mpsc::SendError(back)) => job = back,
            }
        }
    })?;
    answer
        .recv()
        .map_err(|_| "the lite-worker thread ended without an answer".to_owned())?
}

/// A dialect's refusal at the seam: its kind folded from the Java class it carries (`SEAM.md`, "Refusals"), the class
/// prefixed to the message where the kind is `INTERNAL`.
fn dialect_refusal(r: &lite_dialect::Refusal) -> Refusal {
    Refusal::new(r.kind(), r.seam_message())
}

/// One op of a dialect (`rust/lite-dialect/SEAM.md`): `spec` names the dialect (`{"dialect":"DuckDb","server":...,
/// "engineScanOrder":...}`), `op` the method, `args` its arguments as JSON. Answers `OK\n<answer as JSON>`, or
/// `ERR\n<kind>\n-\n-\n<message>`. Each call runs on the calling thread's worker, a thread with a big stack
/// ([`on_big_stack`]), waited for before it returns; a panic there is an `INTERNAL` refusal.
///
/// # Safety
/// `spec`, `op` and `args` are null or NUL-terminated strings the caller keeps alive for the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn lite_dialect(spec: *const c_char, op: *const c_char, args: *const c_char) -> *mut c_char {
    respond(|| {
        let read = || -> Result<(&str, &str, &str), Refusal> {
            Ok((unsafe { text(spec, "spec") }?, unsafe { text(op, "op") }?, unsafe {
                text(args, "args")
            }?))
        };
        let (spec, op, args) = match read() {
            Ok(texts) => texts,
            Err(r) => return Answer::Err(r),
        };
        let (spec, op, args) = (spec.to_owned(), op.to_owned(), args.to_owned());
        match on_big_stack(move || lite_dialect::dispatch(&spec, &op, &args)) {
            Ok(Ok(json)) => Answer::Ok(json),
            Ok(Err(r)) => Answer::Err(dialect_refusal(&r)),
            Err(panic) => Answer::Err(Refusal::new("INTERNAL", panic)),
        }
    })
}

/// One parser door (docs/rust-port/PHASE2.md §3; com.legend.ffi.RustParserProvider): `door` is `spec`, `lambda` or
/// `block`, `dialect` the Java name of the parse's level, `text` the text as a JSON string literal (every surrogate a
/// `\uXXXX` escape, so a lone one survives the UTF-8 C string). Answers `OK\nA\n<records>` -- the lossless codec's
/// JSON of the door's records -- or `OK\nE\n<error>`, the error Java's parser raises as JSON (`class`, `message`, and
/// a `ParseException`'s `line` and `column`, its message without the `[line:col] ` prefix its constructor writes):
/// the parser's own answer either way; `ERR` is the call's failure. On a thread of the parser's own stack size.
///
/// `door` may instead name one of the parity harness's probes (`lite_front::probes`, docs/rust-port/PHASE2.md §4 step
/// 6a: what `//parser-equivalence`'s gate tests asked of the Java parser's internals), answered `A\n<json>` or
/// `E\n<error>` the same way: `sectionHeaders` (`[[name, contentStartOffset], ...]`), `elementSites` (how many
/// top-level declarations parsed where they stand), `databaseSites` (`[[qualifiedName, the element's JSON], ...]`,
/// the JSON as a string) and `knownSections` (`[name, ...]`; its text and dialect are not read); and the prelude
/// generator's (step 6b), `declarationExtents` (`[[kind, start, end], ...]`: each top-level declaration's head and its
/// extent in UTF-16 units).
///
/// # Safety
/// `door`, `dialect` and `text` are null or NUL-terminated strings the caller keeps alive for the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn lite_front(door: *const c_char, dialect: *const c_char, text: *const c_char) -> *mut c_char {
    respond(|| {
        let read = || -> Result<(&str, &str, &str), Refusal> {
            Ok((
                unsafe { self::text(door, "door") }?,
                unsafe { self::text(dialect, "dialect") }?,
                unsafe { self::text(text, "text") }?,
            ))
        };
        let (door, dialect, text) = match read() {
            Ok(texts) => texts,
            Err(r) => return Answer::Err(r),
        };
        let Some(dialect) = lite_front::doors::Dialect::from_java(dialect) else {
            return Answer::Err(Refusal::invalid_argument(format!("no dialect {dialect}")));
        };
        let units = match lite_front::wire::parse(text, 1) {
            Ok(lite_front::wire::Json::Str(units)) => units,
            _ => return Answer::Err(Refusal::invalid_argument("the text is not one JSON string")),
        };
        // at least the parser's own stack (lite_front::doors::PARSE_STACK), the one its differential parses on: the
        // deepest question it holds to Java parses there
        let door = door.to_owned();
        match on_big_stack(move || front_answer(&door, dialect, &units)) {
            Ok(Ok(answer)) => Answer::Ok(answer),
            Ok(Err(r)) => Answer::Err(r),
            Err(panic) => Answer::Err(Refusal::new("INTERNAL", panic)),
        }
    })
}

/// The model door (`PmcdParser.parse`; docs/rust-port/PHASE2.md, "The model door's overlays"): `dialect` the Java
/// name of the parse's level; `overlays` the shim's registered overlay grammars, in registration order, as JSON
/// (`[["Toy",false],...]`: each one's section name and `lexable()`); `text` the text as a JSON string literal.
/// Answers `OK\nA\n<sections>\n<records>` or `OK\nE\n<sections>\n<error>`: the overlay sections the walk met, in
/// section order, each `[name, start, end, line, element, section]` (`lite_front::model_parser::OverlaySection`: the
/// grammar's text `text[start, end)` and the line it starts on, then where its elements go -- before the records'
/// `model.elements[element]`, their paths into the section index's `sections[section]`); then the records in the
/// codec's form, or the error as [`lite_front`] writes it, which an overlay grammar's own exception pre-empts. On the
/// calling thread's worker, as [`lite_front`].
///
/// # Safety
/// `dialect`, `overlays` and `text` are null or NUL-terminated strings the caller keeps alive for the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn lite_front_model(
    dialect: *const c_char,
    overlays: *const c_char,
    text: *const c_char,
) -> *mut c_char {
    respond(|| {
        let read = || -> Result<(&str, &str, &str), Refusal> {
            Ok((
                unsafe { self::text(dialect, "dialect") }?,
                unsafe { self::text(overlays, "overlays") }?,
                unsafe { self::text(text, "text") }?,
            ))
        };
        let (dialect, overlays, text) = match read() {
            Ok(texts) => texts,
            Err(r) => return Answer::Err(r),
        };
        let Some(dialect) = lite_front::doors::Dialect::from_java(dialect) else {
            return Answer::Err(Refusal::invalid_argument(format!("no dialect {dialect}")));
        };
        let grammars = match overlay_grammars(overlays) {
            Some(grammars) => grammars,
            None => {
                return Answer::Err(Refusal::invalid_argument(
                    "the overlays are not a JSON array of [name, lexable]",
                ));
            }
        };
        let units = match lite_front::wire::parse(text, 1) {
            Ok(lite_front::wire::Json::Str(units)) => units,
            _ => return Answer::Err(Refusal::invalid_argument("the text is not one JSON string")),
        };
        match on_big_stack(move || model_answer(dialect, &grammars, &units)) {
            Ok(answer) => Answer::Ok(answer),
            Err(panic) => Answer::Err(Refusal::new("INTERNAL", panic)),
        }
    })
}

/// The engine-JSON doors (docs/rust-port/PHASE2.md §4 step 5): `door` is `document` (`PmcdParser.parseDocument`, a
/// model text at the engine's dialect) or `lambdaJson` (`SpecParser.lambdaJson`); `spans` is `true` or `false`, the
/// doors' `returnSourceInformation`; `overlays` the shim's registered overlay grammars as [`lite_front_model`] takes
/// them (the document door's; the lambda door reads none); `text` the text as a JSON string literal. The Rust parser
/// parses and the Rust emitter writes legend-engine's JSON, in one call. Answers:
/// - `OK\nA\n<json>`: the JSON;
/// - `OK\nU\n<the JSON as a JSON string literal>` when the JSON holds a lone surrogate, which UTF-8 cannot carry (every
///   surrogate escaped; the common answer pays nothing for the rare one);
/// - `OK\nE\n<error>`: the parser's error or the emitter's refusal, as [`lite_front`] writes it;
/// - `OK\nO\n`: the document's walk met an overlay section -- the overlay path, where Java answers (its emitter over
///   the records `PmcdParser.parse` splices). At the engine's dialect no walk meets one.
///
/// On the calling thread's worker, as [`lite_front`].
///
/// # Safety
/// `door`, `spans`, `overlays` and `text` are null or NUL-terminated strings the caller keeps alive for the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn lite_front_engine(
    door: *const c_char,
    spans: *const c_char,
    overlays: *const c_char,
    text: *const c_char,
) -> *mut c_char {
    respond(|| {
        let read = || -> Result<(&str, &str, &str, &str), Refusal> {
            Ok((
                unsafe { self::text(door, "door") }?,
                unsafe { self::text(spans, "spans") }?,
                unsafe { self::text(overlays, "overlays") }?,
                unsafe { self::text(text, "text") }?,
            ))
        };
        let (door, spans, overlays, text) = match read() {
            Ok(texts) => texts,
            Err(r) => return Answer::Err(r),
        };
        let spans = match spans {
            "true" => true,
            "false" => false,
            _ => {
                return Answer::Err(Refusal::invalid_argument(format!(
                    "spans is {spans}, not true or false"
                )));
            }
        };
        if door != "document" && door != "lambdaJson" {
            return Answer::Err(Refusal::invalid_argument(format!("no engine door {door}")));
        }
        let grammars = match overlay_grammars(overlays) {
            Some(grammars) => grammars,
            None => {
                return Answer::Err(Refusal::invalid_argument(
                    "the overlays are not a JSON array of [name, lexable]",
                ));
            }
        };
        let units = match lite_front::wire::parse(text, 1) {
            Ok(lite_front::wire::Json::Str(units)) => units,
            _ => return Answer::Err(Refusal::invalid_argument("the text is not one JSON string")),
        };
        let door = door.to_owned();
        match on_big_stack(move || engine_answer(&door, spans, &grammars, &units)) {
            Ok(answer) => Answer::Ok(answer),
            Err(panic) => Answer::Err(Refusal::new("INTERNAL", panic)),
        }
    })
}

/// An engine door's answer as [`lite_front_engine`] folds it: `A\n<json>`, `U\n<json string>`, `E\n<error>` or `O\n`.
fn engine_answer(door: &str, spans: bool, grammars: &[(Vec<u16>, bool)], text: &[u16]) -> String {
    let overlays: Vec<lite_front::model_parser::OverlayGrammar<'_>> = grammars
        .iter()
        .map(|(name, lexable)| lite_front::model_parser::OverlayGrammar {
            name,
            lexable: *lexable,
        })
        .collect();
    let answer = match door {
        "document" => lite_front::doors::document_with_overlays(text, spans, &overlays),
        _ => Some(lite_front::doors::lambda_json(text, spans)),
    };
    match answer {
        None => "O\n".to_owned(),
        Some(Ok(json)) => match String::from_utf16(&json) {
            Ok(utf8) => {
                let mut out = String::with_capacity(utf8.len() + 2);
                out.push_str("A\n");
                out.push_str(&utf8);
                out
            }
            Err(_) => {
                // a lone surrogate: the JSON as a JSON string literal, each surrogate a \uXXXX escape
                let mut w = lite_front::codec::Writer::new();
                w.raw("U\n");
                w.string(&lite_front::jtypes::JStr(json));
                w.finish()
            }
        },
        Some(Err(e)) => format!("E\n{}", front_error(&e)),
    }
}

/// `[["Toy",false],...]` as the overlay grammars' names and `lexable()`s.
fn overlay_grammars(json: &str) -> Option<Vec<(Vec<u16>, bool)>> {
    let parsed = lite_front::wire::parse(json, 3).ok()?;
    parsed
        .as_array()?
        .iter()
        .map(|g| match g.as_array()? {
            [name, lite_front::wire::Json::Bool(lexable)] => Some((name.as_units()?.to_vec(), *lexable)),
            _ => None,
        })
        .collect()
}

/// The model door's answer as [`lite_front_model`] folds it: `A\n<sections>\n<records>` or `E\n<sections>\n<error>`.
fn model_answer(dialect: lite_front::doors::Dialect, grammars: &[(Vec<u16>, bool)], text: &[u16]) -> String {
    use lite_front::codec;
    let overlays: Vec<lite_front::model_parser::OverlayGrammar<'_>> = grammars
        .iter()
        .map(|(name, lexable)| lite_front::model_parser::OverlayGrammar {
            name,
            lexable: *lexable,
        })
        .collect();
    let answer = lite_front::doors::pmcd(text, dialect, &overlays);
    let mut w = codec::Writer::new();
    w.raw("[");
    for (i, o) in answer.overlays.iter().enumerate() {
        if i > 0 {
            w.raw(",");
        }
        w.raw("[");
        w.string(&lite_front::jtypes::JStr(o.name.clone()));
        w.raw(&format!(
            ",{},{},{},{},{}]",
            o.start_offset, o.end_offset, o.start_line, o.element_index, o.section_index
        ));
    }
    w.raw("]");
    let sections = w.finish();
    match answer.parsed {
        // the door's declared PmcdParser.Parsed fixes the class: no tag
        Ok(parsed) => format!("A\n{sections}\n{}", codec::write(&parsed, true)),
        Err(e) => format!("E\n{sections}\n{}", front_error(&e)),
    }
}

/// A door's answer as `lite_front` folds it: `A\n<records>` or `E\n<error>`.
fn front_answer(door: &str, dialect: lite_front::doors::Dialect, text: &[u16]) -> Result<String, Refusal> {
    use lite_front::codec::{self, Codec};
    use lite_front::jtypes::JStr;
    use lite_front::probes;
    let written = match door {
        // the parity harness's probes (lite_front::probes; docs/rust-port/PHASE2.md §4 step 6a): what
        // //parser-equivalence's gate tests asked of the Java parser's internals
        "sectionHeaders" => {
            // [[name, contentStartOffset], ...]
            let mut w = codec::Writer::new();
            w.raw("[");
            for (i, (name, offset)) in probes::section_headers(text).into_iter().enumerate() {
                w.raw(if i > 0 { ",[" } else { "[" });
                w.string(&JStr(name));
                w.raw(&format!(",{offset}]"));
            }
            w.raw("]");
            Ok(w.finish())
        }
        "elementSites" => probes::element_sites(text, dialect).map(|parsed| parsed.to_string()),
        "databaseSites" => probes::database_sites(text, dialect).map(|sites| {
            // [[qualifiedName, the element's JSON as a string], ...]
            let mut w = codec::Writer::new();
            w.raw("[");
            for (i, (name, json)) in sites.into_iter().enumerate() {
                w.raw(if i > 0 { ",[" } else { "[" });
                w.string(&JStr(name));
                w.raw(",");
                w.string(&JStr(json));
                w.raw("]");
            }
            w.raw("]");
            w.finish()
        }),
        // the prelude generator's (step 6b): [[kind, start, end], ...]
        "declarationExtents" => probes::declaration_extents(text, dialect).map(|extents| {
            let mut w = codec::Writer::new();
            w.raw("[");
            for (i, d) in extents.into_iter().enumerate() {
                w.raw(if i > 0 { ",[" } else { "[" });
                w.string(&JStr::from(d.kind));
                w.raw(&format!(",{},{}]", d.start, d.end));
            }
            w.raw("]");
            w.finish()
        }),
        "knownSections" => {
            // [name, ...]: the text and the dialect are not read
            let mut w = codec::Writer::new();
            w.raw("[");
            for (i, name) in probes::known_sections().into_iter().enumerate() {
                if i > 0 {
                    w.raw(",");
                }
                w.string(&JStr::from(name));
            }
            w.raw("]");
            Ok(w.finish())
        }
        "spec" => lite_front::doors::spec(text, dialect).map(|v| codec::write(&v, false)),
        "lambda" => lite_front::doors::lambda(text).map(|v| codec::write(&v, true)),
        "block" => lite_front::doors::block(text, dialect).map(|vs| {
            // a block's elements are tagged, as the door's declared List<ValueSpecification> writes them
            let mut w = codec::Writer::new();
            w.raw("[");
            for (i, v) in vs.iter().enumerate() {
                if i > 0 {
                    w.raw(",");
                }
                v.write(&mut w, false);
            }
            w.raw("]");
            w.finish()
        }),
        _ => return Err(Refusal::invalid_argument(format!("no parser door {door}"))),
    };
    Ok(match written {
        Ok(json) => format!("A\n{json}"),
        Err(e) => format!("E\n{}", front_error(&e)),
    })
}

/// The error Java's parser raises, as JSON for the shim to rethrow: a `ParseException`'s message without the
/// `[line:col] ` prefix (its constructor writes it again from the line and column).
fn front_error(e: &lite_front::doors::JavaError) -> String {
    let mut w = lite_front::codec::Writer::new();
    w.raw("{\"class\":");
    w.string(&lite_front::jtypes::JStr::from(e.class.as_str()));
    w.raw(",\"message\":");
    match &e.message {
        None => w.raw("null"),
        Some(m) => {
            let mut units = m.units();
            if let Some((line, column)) = e.position
                && line > 0
            {
                let prefix: Vec<u16> = format!("[{line}:{column}] ").encode_utf16().collect();
                if units.starts_with(&prefix) {
                    units = &units[prefix.len()..];
                }
            }
            w.string(&lite_front::jtypes::JStr(units.to_vec()));
        }
    }
    if let Some((line, column)) = e.position {
        w.raw(&format!(",\"line\":{line},\"column\":{column}"));
    }
    w.raw("}");
    w.finish()
}

/// A C string the caller owns. Text holding a NUL cannot be one, so such an answer is refused instead.
fn into_c(text: String) -> *mut c_char {
    let c = CString::new(text).unwrap_or_else(|_| {
        CString::new(fold(&Answer::Err(Refusal::new(
            "INTERNAL",
            "an answer held a NUL character",
        ))))
        .expect("a refusal's text holds no NUL")
    });
    UNFREED.fetch_add(1, Ordering::SeqCst);
    c.into_raw()
}

/// An argument as text: refused when it is null or not UTF-8.
///
/// # Safety
/// `arg` is null or a NUL-terminated string the caller keeps alive for the call.
unsafe fn text<'a>(arg: *const c_char, name: &str) -> Result<&'a str, Refusal> {
    if arg.is_null() {
        return Err(Refusal::invalid_argument(format!("{name} is null")));
    }
    // SAFETY: non-null, and NUL-terminated and alive for the call by the caller's contract
    let bytes = unsafe { CStr::from_ptr(arg) };
    bytes
        .to_str()
        .map_err(|e| Refusal::invalid_argument(format!("{name} is not UTF-8: {e}")))
}

/// This library's version: `OK\n<version>`.
#[unsafe(no_mangle)]
pub extern "C" fn lite_version() -> *mut c_char {
    respond(|| Answer::Ok(env!("CARGO_PKG_VERSION").to_owned()))
}

/// The round trip: `OK\n<text>` for the text given.
///
/// # Safety
/// `text` is null or a NUL-terminated string the caller keeps alive for the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn lite_echo(text: *const c_char) -> *mut c_char {
    respond(|| match unsafe { self::text(text, "text") } {
        Ok(t) => Answer::Ok(t.to_owned()),
        Err(r) => Answer::Err(r),
    })
}

/// Phase 0's probe of the refusal path: refuses with the kind, phase and message given, at `line:column` when both
/// are positive.
///
/// # Safety
/// `kind`, `phase` and `message` are null or NUL-terminated strings the caller keeps alive for the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn lite_probe_refusal(
    kind: *const c_char,
    phase: *const c_char,
    line: u32,
    column: u32,
    message: *const c_char,
) -> *mut c_char {
    respond(|| {
        let read = || -> Result<Refusal, Refusal> {
            let kind = unsafe { text(kind, "kind") }?;
            let phase = unsafe { text(phase, "phase") }?;
            let message = unsafe { text(message, "message") }?;
            Ok(Refusal {
                kind: kind.to_owned(),
                phase: (!phase.is_empty()).then(|| phase.to_owned()),
                position: (line > 0 && column > 0).then_some((line, column)),
                message: message.to_owned(),
            })
        };
        Answer::Err(read().unwrap_or_else(|r| r))
    })
}

/// Phase 0's probe of the panic path: panics with the message given, which the export answers as `INTERNAL`.
///
/// # Safety
/// `message` is null or a NUL-terminated string the caller keeps alive for the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn lite_probe_panic(message: *const c_char) -> *mut c_char {
    respond(|| {
        let message = unsafe { text(message, "message") }
            .map(str::to_owned)
            .unwrap_or_default();
        panic!("{message}")
    })
}

/// Gives an answer back. Null is ignored.
///
/// # Safety
/// `answer` is null or a pointer an export of this library returned, not yet freed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn lite_free(answer: *mut c_char) {
    if !answer.is_null() {
        // SAFETY: an export's answer, made by CString::into_raw and freed once by the caller's contract
        drop(unsafe { CString::from_raw(answer) });
        UNFREED.fetch_sub(1, Ordering::SeqCst);
    }
}

/// The answers handed out and not yet freed.
#[unsafe(no_mangle)]
pub extern "C" fn lite_unfreed() -> i64 {
    UNFREED.load(Ordering::SeqCst)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, MutexGuard};

    /// One test at a time: [`UNFREED`] is the process's, and the tests run on parallel threads.
    fn serial() -> MutexGuard<'static, ()> {
        static LOCK: Mutex<()> = Mutex::new(());
        LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// An export's answer as text, freed.
    fn take(answer: *mut c_char) -> String {
        let text = unsafe { CStr::from_ptr(answer) }.to_str().expect("UTF-8").to_owned();
        unsafe { lite_free(answer) };
        text
    }

    fn c(s: &str) -> CString {
        CString::new(s).expect("no NUL")
    }

    #[test]
    fn folds_answers_and_refusals() {
        let _serial = serial();
        assert_eq!(fold(&Answer::Ok("a\nb".into())), "OK\na\nb");
        let refusal = Refusal {
            kind: "PARSER".into(),
            phase: Some("PARSE".into()),
            position: Some((3, 14)),
            message: "expected ')'\nnear here".into(),
        };
        assert_eq!(
            fold(&Answer::Err(refusal)),
            "ERR\nPARSER\nPARSE\n3:14\nexpected ')'\nnear here"
        );
        assert_eq!(
            fold(&Answer::Err(Refusal::new("INTERNAL", "boom"))),
            "ERR\nINTERNAL\n-\n-\nboom"
        );
    }

    #[test]
    fn echoes_text_and_refuses_what_is_not_text() {
        let _serial = serial();
        let s = c("héllo, \u{1F600}\nworld");
        assert_eq!(take(unsafe { lite_echo(s.as_ptr()) }), "OK\nhéllo, \u{1F600}\nworld");
        assert_eq!(
            take(unsafe { lite_echo(std::ptr::null()) }),
            "ERR\nINVALID_ARGUMENT\n-\n-\ntext is null"
        );
        let bad = [0xffu8, 0xfe, 0];
        let answer = take(unsafe { lite_echo(bad.as_ptr().cast()) });
        assert!(
            answer.starts_with("ERR\nINVALID_ARGUMENT\n-\n-\ntext is not UTF-8"),
            "{answer}"
        );
    }

    #[test]
    fn refuses_and_catches_panics() {
        let _serial = serial();
        let (kind, phase, message) = (c("COMPILATION"), c("TYPE"), c("no such class"));
        let answer = take(unsafe { lite_probe_refusal(kind.as_ptr(), phase.as_ptr(), 2, 5, message.as_ptr()) });
        assert_eq!(answer, "ERR\nCOMPILATION\nTYPE\n2:5\nno such class");
        let message = c("on purpose");
        assert_eq!(
            take(unsafe { lite_probe_panic(message.as_ptr()) }),
            "ERR\nINTERNAL\n-\n-\non purpose"
        );
    }

    #[test]
    fn counts_what_is_not_freed() {
        let _serial = serial();
        let before = lite_unfreed();
        let answer = lite_version();
        assert_eq!(lite_unfreed(), before + 1);
        assert_eq!(take(answer), format!("OK\n{}", env!("CARGO_PKG_VERSION")));
        assert_eq!(lite_unfreed(), before);
        unsafe { lite_free(std::ptr::null_mut()) };
        assert_eq!(lite_unfreed(), before);
    }

    // ---- lite_dialect (rust/lite-dialect/SEAM.md)

    const DUCKDB: &str = r#"{"dialect":"DuckDb","server":null,"engineScanOrder":false}"#;

    /// A select over raw SQL, as the golden dump holds one.
    const RAW_SELECT: &str = r#"{"query":{"_type":"select","projections":[],"distinct":false,"from":{"_type":"rawSql","sql":"select 3 as V","alias":"_p","outputs":[],"writtenFor":"DuckDB"},"where":null,"groupBy":[],"having":null,"qualify":null,"orderBy":[],"limit":0,"offset":null,"outputs":[]}}"#;

    /// A select of one plan parameter.
    const PARAM_SELECT: &str = r##"{"query":{"_type":"select","projections":[{"expr":{"_type":"planParam","type":{"_type":"typed","sqlType":{"_type":"scalar","name":"BIGINT"},"nullable":false},"name":"#count","kind":"OTHER","optional":false,"enumMapFn":null,"byValue":null},"alias":"value","out":{"name":"value","type":{"_type":"scalar","name":"BIGINT"},"nullable":false,"origin":"DERIVED"}}],"distinct":false,"from":{"_type":"dual"},"where":null,"groupBy":[],"having":null,"qualify":null,"orderBy":[],"limit":null,"offset":null,"outputs":[{"name":"value","type":{"_type":"scalar","name":"BIGINT"},"nullable":false,"origin":"DERIVED"}]}}"##;

    /// `lite_dialect`'s answer as text, freed.
    fn dialect(spec: &str, op: &str, args: &str) -> String {
        let (spec, op, args) = (c(spec), c(op), c(args));
        take(unsafe { lite_dialect(spec.as_ptr(), op.as_ptr(), args.as_ptr()) })
    }

    #[test]
    fn a_dialect_renders_a_query_and_a_statement() {
        let _serial = serial();
        let before = lite_unfreed();
        assert_eq!(
            dialect(DUCKDB, "query.render", RAW_SELECT),
            "OK\n\"SELECT *\\nFROM (select 3 as V) AS _p\\nLIMIT 0\""
        );
        assert_eq!(
            dialect(DUCKDB, "query.renderStatement", PARAM_SELECT),
            "OK\n{\"sql\":\"SELECT ? AS value\",\"binds\":[{\"parameter\":\"#count\",\"binding\":{\"_type\":\"one\",\
             \"nullType\":\"BIGINT\",\"hole\":null}}],\"regions\":[]}"
        );
        assert_eq!(
            dialect(DUCKDB, "physicalName", r#"{"name":"order"}"#),
            "OK\n\"\\\"order\\\"\""
        );
        assert_eq!(lite_unfreed(), before);
    }

    #[test]
    fn a_dialect_answers_its_facts() {
        let _serial = serial();
        assert_eq!(
            dialect(DUCKDB, "facts", "{}"),
            "OK\n{\"temporaryTables\":true,\"loadsRowsInBulk\":true,\"relational\":true,\"inProcess\":true,\
             \"dropsStagedTablesWithTheirTransaction\":false,\"needsStaticPivot\":false,\"serverVersions\":null,\
             \"inMemoryPath\":\":memory:\",\"scriptAbort\":\"ROLLBACK\",\"sessionSetup\":[\"SET TimeZone='UTC'\"],\
             \"keysSnapshot\":\"TRANSACTION\",\"resultColumnLimit\":null}"
        );
    }

    #[test]
    fn a_dialects_refusal_folds_its_java_class_into_a_kind() {
        let _serial = serial();
        assert_eq!(
            dialect(r#"{"dialect":"Oracle"}"#, "facts", "{}"),
            "ERR\nILLEGAL_ARGUMENT\n-\n-\nunknown dialect 'Oracle': Rust answers DuckDb, H2, H2Modern, EngineStyleH2, \
             EngineStyleDB2, EngineStyleComposite, Postgres, SQLite, Trino, Databricks, ClickHouse, BigQuery, \
             Snowflake, MariaDb, MySql, MongoDb"
        );
        let geometry = r#"{"column":{"name":"g","dataType":"GEOMETRY","logicalType":"GEOMETRY","precision":null,"scale":null,"notNull":false}}"#;
        assert_eq!(
            dialect(DUCKDB, "catalogType", geometry),
            "ERR\nDIALECT_CAPABILITY\n-\n-\na column of DuckDB type 'GEOMETRY' cannot be declared in a Pure Database \
             (a spatial value: no Pure Database type holds it)"
        );
        assert_eq!(
            dialect(DUCKDB, "query.render", PARAM_SELECT),
            "ERR\nILLEGAL_STATE\n-\n-\na statement with bound parameters [Bind[parameter=#count, binding=One[nullType=\
             BIGINT, hole=null]]] was asked for as text: render it as a statement"
        );
        let (spec, args) = (c(DUCKDB), c("{}"));
        assert_eq!(
            take(unsafe { lite_dialect(spec.as_ptr(), std::ptr::null(), args.as_ptr()) }),
            "ERR\nINVALID_ARGUMENT\n-\n-\nop is null"
        );
        // a class the seam names no kind for is INTERNAL, the class kept in the message
        let r = dialect_refusal(&lite_dialect::Refusal {
            class: lite_dialect::Refusal::NUMBER_FORMAT,
            message: "For input string: \"x\"".to_owned(),
        });
        assert_eq!(
            fold(&Answer::Err(r)),
            "ERR\nINTERNAL\n-\n-\njava.lang.NumberFormatException: For input string: \"x\""
        );
    }

    #[test]
    fn a_panic_on_the_worker_is_an_answer() {
        let _serial = serial();
        assert_eq!(on_big_stack(|| 7), Ok(7));
        assert_eq!(
            on_big_stack(|| -> i32 { panic!("on purpose") }),
            Err("on purpose".to_owned())
        );
        assert_eq!(
            on_big_stack(|| -> i32 { std::panic::panic_any(3) }),
            Err("a panic with no message".to_owned())
        );
        // the worker outlived the panics
        assert_eq!(on_big_stack(|| 8), Ok(8));
    }

    #[test]
    fn a_calling_thread_keeps_its_worker() {
        let _serial = serial();
        let here = on_big_stack(|| std::thread::current().id()).unwrap();
        assert_ne!(
            here,
            std::thread::current().id(),
            "the body ran on a worker, not the caller"
        );
        assert_eq!(
            on_big_stack(|| std::thread::current().id()),
            Ok(here),
            "the next call reuses it"
        );
        let elsewhere = std::thread::spawn(|| on_big_stack(|| std::thread::current().id()).unwrap())
            .join()
            .unwrap();
        assert_ne!(elsewhere, here, "another calling thread has a worker of its own");
    }

    #[test]
    fn the_model_door_hands_back_its_overlay_sections() {
        let _serial = serial();
        let source = "Class my::A { a: String[1]; }\n\n###Toy\nToy my::toys::T1;\n";
        let (start, end) = (source.find("Toy my").unwrap(), source.len());
        let text = format!("\"{}\"", source.replace('\n', "\\n"));
        let model = |dialect: &str, overlays: &str| {
            take(unsafe { lite_front_model(c(dialect).as_ptr(), c(overlays).as_ptr(), c(&text).as_ptr()) })
        };
        let lite = model("LEGEND_LITE", r#"[["Toy",false]]"#);
        let mut lines = lite.splitn(4, '\n');
        assert_eq!((lines.next(), lines.next()), (Some("OK"), Some("A")));
        // the class's records, then the section index; the toy grammar's elements go before element 1
        assert_eq!(lines.next(), Some(format!("[[\"Toy\",{start},{end},4,1,1]]").as_str()));
        assert!(lines.next().unwrap().starts_with('{'), "the records follow");
        // the engine's dialect knows no overlay: the walk refuses the section, having met no overlay before it
        let engine = model("LEGEND_ENGINE", "[]");
        assert!(engine.starts_with("OK\nE\n[]\n{"), "{engine}");
        assert!(engine.contains("is not a known section parser"), "{engine}");
        assert!(model("LEGEND_LITE", r#"[["Toy"]]"#).starts_with("ERR\n"));
    }

    #[test]
    fn the_engine_doors_answer_json_errors_and_lone_surrogates() {
        let _serial = serial();
        let engine = |door: &str, spans: &str, text: &str| {
            let quoted = format!(
                "\"{}\"",
                text.replace('\\', "\\\\").replace('\n', "\\n").replace('"', "\\\"")
            );
            take(unsafe {
                lite_front_engine(
                    c(door).as_ptr(),
                    c(spans).as_ptr(),
                    c("[]").as_ptr(),
                    c(&quoted).as_ptr(),
                )
            })
        };
        assert_eq!(
            engine("lambdaJson", "false", "x|$x"),
            "OK\nA\n{\"_type\":\"lambda\",\"body\":[{\"_type\":\"var\",\"name\":\"x\"}],\"parameters\":[{\"_type\":\
             \"var\",\"name\":\"x\"}]}"
        );
        assert!(engine("lambdaJson", "true", "x|$x").contains("\"sourceInformation\":{"));
        let document = engine("document", "false", "Class a::B {}");
        assert!(
            document.starts_with("OK\nA\n{\"_type\":\"data\",\"elements\":[{\"_type\":\"class\""),
            "{document}"
        );
        assert!(!document.contains("sourceInformation"), "{document}");
        // a parse error, as lite_front writes it
        let error = engine("document", "true", "Class a::B {");
        assert!(
            error.starts_with("OK\nE\n{\"class\":\"com.legend.parser.ParseException\""),
            "{error}"
        );
        // the emitter's refusal of a model the parser accepts (the oracle's one: a graph-fetch argument it has no
        // rule for), a placed parse error since 2026-11-09 (ruling 4 of docs/rust-port/PHASE2.md section 6)
        let refused = engine("document", "false", "function f::g():Any[*]{#{a::B{c(f())}}#}");
        assert!(
            refused.starts_with("OK\nE\n{\"class\":\"com.legend.parser.ParseException\""),
            "{refused}"
        );
        // a lone surrogate in a string literal: the JSON crosses as a JSON string, its surrogate escaped
        let surrogate = engine("lambdaJson", "false", "|'a\\uD800b'");
        assert!(
            surrogate.starts_with("OK\nU\n\"{\\\"_type\\\":\\\"lambda\\\""),
            "{surrogate}"
        );
        assert!(surrogate.contains("a\\ud800b"), "{surrogate}");
        assert!(engine("document", "maybe", "Class a::B {}").starts_with("ERR\nINVALID_ARGUMENT"));
        assert!(engine("pmcd", "true", "Class a::B {}").starts_with("ERR\nINVALID_ARGUMENT"));
    }

    #[test]
    fn the_parity_probes_answer_through_the_front_door() {
        let _serial = serial();
        let front = |door: &str, dialect: &str, text: &str| {
            let quoted = format!(
                "\"{}\"",
                text.replace('\\', "\\\\").replace('\n', "\\n").replace('"', "\\\"")
            );
            take(unsafe { lite_front(c(door).as_ptr(), c(dialect).as_ptr(), c(&quoted).as_ptr()) })
        };
        assert_eq!(
            front(
                "sectionHeaders",
                "LEGEND_ENGINE",
                "Class a::A {}\n###Relational\nDatabase a::D ()\n"
            ),
            "OK\nA\n[[\"Relational\",28]]"
        );
        assert_eq!(front("sectionHeaders", "LEGEND_ENGINE", "Class a::A {}"), "OK\nA\n[]");
        assert_eq!(
            front("elementSites", "LEGEND_ENGINE", "Class a::A {}\nEnum a::E { X }"),
            "OK\nA\n2"
        );
        let refused = front("elementSites", "LEGEND_ENGINE", "Class a::A<T> {}");
        assert!(
            refused.starts_with("OK\nE\n{\"class\":\"com.legend.parser.ParseException\""),
            "{refused}"
        );
        assert_eq!(front("elementSites", "LEGEND_LITE", "Class a::A<T> {}"), "OK\nA\n1");
        let databases = front("databaseSites", "LEGEND_ENGINE", "###Relational\nDatabase a::D ( )");
        assert!(
            databases.starts_with("OK\nA\n[[\"a::D\",\"{\\\"_type\\\":\\\"relational\\\""),
            "{databases}"
        );
        assert_eq!(
            front(
                "declarationExtents",
                "LEGEND_PLATFORM",
                "Enum a::E { X }\nnative function a::n(): String[1];"
            ),
            "OK\nA\n[[\"Enum\",0,15],[\"native function\",16,50]]"
        );
        let unterminated = front(
            "declarationExtents",
            "LEGEND_PLATFORM",
            "native function a::n(): String[1]",
        );
        assert!(
            unterminated.starts_with("OK\nE\n{\"class\":\"java.lang.IllegalStateException\""),
            "{unterminated}"
        );
        let known = front("knownSections", "LEGEND_ENGINE", "");
        assert!(known.starts_with("OK\nA\n[\"Pure\","), "{known}");
        assert_eq!(known.matches(',').count(), 24, "{known}");
    }

    #[test]
    fn a_deep_tree_renders_when_called_from_a_small_stack() {
        let _serial = serial();
        // 2045 NOTs over a typed literal: the MIR at MirJson's depth limit (4096), far deeper than a 1 MB stack takes
        // in a debug build
        let typed = r#"{"_type":"typed","sqlType":{"_type":"scalar","name":"BOOLEAN"},"nullable":true}"#;
        let nots = 2045;
        let mut e = format!(r#"{{"_type":"boolLit","type":{typed},"value":true}}"#);
        for _ in 0..nots {
            e = format!(r#"{{"_type":"call","type":{typed},"fn":"NOT","args":[{e}]}}"#);
        }
        let args = format!(
            r#"{{"query":{{"_type":"select","projections":[{{"expr":{e},"alias":"x","out":null}}],"distinct":false,"from":{{"_type":"dual"}},"where":null,"groupBy":[],"having":null,"qualify":null,"orderBy":[],"limit":null,"offset":null,"outputs":[]}}}}"#
        );
        // the caller's thread has a JVM thread's default stack
        let answer = std::thread::Builder::new()
            .stack_size(1 << 20)
            .spawn(move || dialect(DUCKDB, "query.render", &args))
            .expect("a thread")
            .join()
            .expect("no panic");
        assert!(
            answer.starts_with("OK\n\"SELECT "),
            "{}",
            &answer[..answer.len().min(200)]
        );
        assert_eq!(answer.matches("NOT").count(), nots);
    }
}
