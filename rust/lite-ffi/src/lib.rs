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
use std::ffi::{CStr, CString, c_char};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicI64, Ordering};

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
        Refusal { kind: kind.to_owned(), phase: None, position: None, message: message.into() }
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

/// The stack a dialect call runs on: reserved, not committed, so it costs only what the call touches. A MIR nests
/// thousands deep (the golden dump's deepest statement needs more than 1 MB in a debug build), and the thread the
/// host calls from may have 1 MB (a JVM thread's default).
pub const DIALECT_STACK_BYTES: usize = 256 << 20;

/// `body` on a thread of its own with [`DIALECT_STACK_BYTES`] of stack, joined before this returns: its answer, or the
/// message of its panic (or of the thread's failure to start).
fn on_dialect_thread<T: Send>(body: impl FnOnce() -> T + Send) -> Result<T, String> {
    std::thread::scope(|scope| {
        let thread = std::thread::Builder::new()
            .name("lite-dialect".to_owned())
            .stack_size(DIALECT_STACK_BYTES)
            .spawn_scoped(scope, body)
            .map_err(|e| format!("the dialect's thread did not start: {e}"))?;
        // joined here, so the scope has no unjoined panic to rethrow
        thread.join().map_err(|payload| panic_message(payload.as_ref()))
    })
}

/// A dialect's refusal at the seam: its kind folded from the Java class it carries (`SEAM.md`, "Refusals"), the class
/// prefixed to the message where the kind is `INTERNAL`.
fn dialect_refusal(r: &lite_dialect::Refusal) -> Refusal {
    Refusal::new(r.kind(), r.seam_message())
}

/// One op of a dialect (`rust/lite-dialect/SEAM.md`): `spec` names the dialect (`{"dialect":"DuckDb","server":...,
/// "engineScanOrder":...}`), `op` the method, `args` its arguments as JSON. Answers `OK\n<answer as JSON>`, or
/// `ERR\n<kind>\n-\n-\n<message>`. Each call runs on a fresh thread with a big stack ([`DIALECT_STACK_BYTES`]), joined
/// before it returns; a panic there is an `INTERNAL` refusal.
///
/// # Safety
/// `spec`, `op` and `args` are null or NUL-terminated strings the caller keeps alive for the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn lite_dialect(spec: *const c_char, op: *const c_char, args: *const c_char) -> *mut c_char {
    respond(|| {
        let read = || -> Result<(&str, &str, &str), Refusal> {
            Ok((unsafe { text(spec, "spec") }?, unsafe { text(op, "op") }?, unsafe { text(args, "args") }?))
        };
        let (spec, op, args) = match read() {
            Ok(texts) => texts,
            Err(r) => return Answer::Err(r),
        };
        match on_dialect_thread(|| lite_dialect::dispatch(spec, op, args)) {
            Ok(Ok(json)) => Answer::Ok(json),
            Ok(Err(r)) => Answer::Err(dialect_refusal(&r)),
            Err(panic) => Answer::Err(Refusal::new("INTERNAL", panic)),
        }
    })
}

/// A C string the caller owns. Text holding a NUL cannot be one, so such an answer is refused instead.
fn into_c(text: String) -> *mut c_char {
    let c = CString::new(text).unwrap_or_else(|_| {
        CString::new(fold(&Answer::Err(Refusal::new("INTERNAL", "an answer held a NUL character"))))
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
    bytes.to_str().map_err(|e| Refusal::invalid_argument(format!("{name} is not UTF-8: {e}")))
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
        let message = unsafe { text(message, "message") }.map(str::to_owned).unwrap_or_default();
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
        assert_eq!(fold(&Answer::Err(refusal)), "ERR\nPARSER\nPARSE\n3:14\nexpected ')'\nnear here");
        assert_eq!(fold(&Answer::Err(Refusal::new("INTERNAL", "boom"))), "ERR\nINTERNAL\n-\n-\nboom");
    }

    #[test]
    fn echoes_text_and_refuses_what_is_not_text() {
        let _serial = serial();
        let s = c("héllo, \u{1F600}\nworld");
        assert_eq!(take(unsafe { lite_echo(s.as_ptr()) }), "OK\nhéllo, \u{1F600}\nworld");
        assert_eq!(take(unsafe { lite_echo(std::ptr::null()) }), "ERR\nINVALID_ARGUMENT\n-\n-\ntext is null");
        let bad = [0xffu8, 0xfe, 0];
        let answer = take(unsafe { lite_echo(bad.as_ptr().cast()) });
        assert!(answer.starts_with("ERR\nINVALID_ARGUMENT\n-\n-\ntext is not UTF-8"), "{answer}");
    }

    #[test]
    fn refuses_and_catches_panics() {
        let _serial = serial();
        let (kind, phase, message) = (c("COMPILATION"), c("TYPE"), c("no such class"));
        let answer = take(unsafe { lite_probe_refusal(kind.as_ptr(), phase.as_ptr(), 2, 5, message.as_ptr()) });
        assert_eq!(answer, "ERR\nCOMPILATION\nTYPE\n2:5\nno such class");
        let message = c("on purpose");
        assert_eq!(take(unsafe { lite_probe_panic(message.as_ptr()) }), "ERR\nINTERNAL\n-\n-\non purpose");
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
        assert_eq!(dialect(DUCKDB, "physicalName", r#"{"name":"order"}"#), "OK\n\"\\\"order\\\"\"");
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
    fn a_panic_on_the_dialects_thread_is_an_answer() {
        let _serial = serial();
        assert_eq!(on_dialect_thread(|| 7), Ok(7));
        assert_eq!(on_dialect_thread(|| -> i32 { panic!("on purpose") }), Err("on purpose".to_owned()));
        assert_eq!(
            on_dialect_thread(|| -> i32 { std::panic::panic_any(3) }),
            Err("a panic with no message".to_owned())
        );
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
        assert!(answer.starts_with("OK\n\"SELECT "), "{}", &answer[..answer.len().min(200)]);
        assert_eq!(answer.matches("NOT").count(), nots);
    }
}
