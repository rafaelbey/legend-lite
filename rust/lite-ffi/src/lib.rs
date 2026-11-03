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
//! Phase 0 exports only the contract itself -- a version, an echo, and two probes that refuse and panic on purpose --
//! so the shim and its test can hold the whole round trip before any compiler code lands.

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
        Err(payload) => {
            let what = payload
                .downcast_ref::<&str>()
                .map(|s| (*s).to_owned())
                .or_else(|| payload.downcast_ref::<String>().cloned())
                .unwrap_or_else(|| "a panic with no message".to_owned());
            Answer::Err(Refusal::new("INTERNAL", what))
        }
    };
    into_c(fold(&answer))
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
}
