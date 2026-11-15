// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! What the normalizer throws, as Java throws it, and the predicates its catches test (EF_CENSUS §9.4: the poison and
//! wall machinery is the answer -- which catch takes what, which kind reaches `strictErrors`, first line or full
//! message, element attribution).
//!
//! - [`Thrown::Compile`]: a `com.legend.error` class with its phase, message and element (lite-error's
//!   [`CompileError`]): `ModelException`, `NotImplementedException` (and `WalledBodyException`, its subclass), the
//!   resolver's `ResolutionException` passed through from lite-names, and so on.
//! - [`Thrown::Java`]: a JDK exception by its class and message (`IllegalStateException`, the `NullPointerException`
//!   of an `Objects.requireNonNull`, the `NoSuchElementException` of a bare `orElseThrow()`).
//! - [`Thrown::Unported`]: a body the skeleton has not ported yet, by its Java site (`Class.method`). **It matches no
//!   catch predicate**: a per-set, per-class or per-association catch that took it would turn an unported body into a
//!   poison or a wall, so it always reaches the top. It goes when the last body is ported.
//!
//! Java's catches, and the predicate each is ported with: `catch (ModelException e)` is [`Thrown::is_model`];
//! `catch (NotImplementedException e)` is [`Thrown::is_not_implemented`] (a `WalledBodyException` too); `catch
//! (NotImplementedException | ModelException e)` is [`Thrown::is_model_or_not_implemented`]; a catch of any
//! `LegendCompileException` is [`Thrown::is_compile`]. A JDK exception is caught by none of them: E has no `catch
//! (RuntimeException)` (EF_CENSUS §1).

use lite_error::{CompileError, ErrorKind, Phase};
use lite_model::JStr;
use lite_model::cat;

pub const ILLEGAL_STATE: &str = "java.lang.IllegalStateException";
pub const ILLEGAL_ARGUMENT: &str = "java.lang.IllegalArgumentException";
pub const NULL_POINTER: &str = "java.lang.NullPointerException";
pub const NO_SUCH_ELEMENT: &str = "java.util.NoSuchElementException";
pub const NUMBER_FORMAT: &str = "java.lang.NumberFormatException";
/// The class an [`Thrown::Unported`] answers by: no Java class, so a differential shows it as the miss it is.
pub const UNPORTED: &str = "lite.normalizer.Unported";

/// A throw of the normalizer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Thrown {
    /// A `com.legend.error` class (a `LegendCompileException`, or a `NotImplementedException`, which has no phase).
    Compile(CompileError),
    /// A JDK exception: its class's binary name and its message.
    Java { class: &'static str, message: Option<JStr> },
    /// A body not ported yet, by its Java site (`Class.method`). Caught by nothing.
    Unported(&'static str),
}

impl Thrown {
    /// `new ModelException(phase, message[, elementFqn])`.
    pub fn model(phase: Phase, message: JStr, element: Option<&JStr>) -> Thrown {
        Thrown::Compile(CompileError::model(phase, message.0, element.map(|e| e.0.clone())))
    }

    /// `new NotImplementedException(message)`.
    pub fn not_implemented(message: JStr) -> Thrown {
        Thrown::Compile(CompileError::not_implemented(message.0))
    }

    /// `new IllegalStateException(message)`: an internal error (a pipeline-order bug, a model gap past D).
    pub fn illegal_state(message: JStr) -> Thrown {
        Thrown::java(ILLEGAL_STATE, Some(message))
    }

    /// `new IllegalArgumentException(message)`.
    pub fn illegal_argument(message: JStr) -> Thrown {
        Thrown::java(ILLEGAL_ARGUMENT, Some(message))
    }

    /// The `NullPointerException` of `Objects.requireNonNull(null[, message])`.
    pub fn null_pointer(message: Option<&str>) -> Thrown {
        Thrown::java(NULL_POINTER, message.map(JStr::from))
    }

    /// The `NoSuchElementException` of a bare `Optional.orElseThrow()` on an empty optional ("No value present").
    pub fn no_such_element() -> Thrown {
        Thrown::java(NO_SUCH_ELEMENT, Some(JStr::from("No value present")))
    }

    /// `MissProbe.neverFired(site)` (census §4: the funnel class is not ported; its loud arm is this internal error,
    /// its text exactly): an empty answer at a site that never fired on the 2026-09-15 census, a real model gap.
    pub fn never_fired(site: &str) -> Thrown {
        Thrown::illegal_state(cat!(
            "F7.8: empty answer at ",
            site,
            " (this default never fired on the 2026-09-15 B5 census; a miss here is a real model gap)"
        ))
    }

    /// A JDK exception by its class and message.
    pub fn java(class: &'static str, message: Option<JStr>) -> Thrown {
        Thrown::Java { class, message }
    }

    /// The thrown class's binary name ([`UNPORTED`] for a body not ported yet).
    pub fn java_class(&self) -> &'static str {
        match self {
            Thrown::Compile(e) => e.kind.java_class(),
            Thrown::Java { class, .. } => class,
            Thrown::Unported(_) => UNPORTED,
        }
    }

    /// `getMessage()` (an unported body: its site).
    pub fn message(&self) -> Option<JStr> {
        match self {
            Thrown::Compile(e) => e.message.clone().map(JStr),
            Thrown::Java { message, .. } => message.clone(),
            Thrown::Unported(site) => Some(cat!("not ported yet: ", *site)),
        }
    }

    /// A `LegendCompileException`'s `phase()`; none for anything else (a `NotImplementedException` has none).
    pub fn phase(&self) -> Option<Phase> {
        match self {
            Thrown::Compile(e) => e.phase,
            _ => None,
        }
    }

    /// A `LegendCompileException`'s `element()`: the FQN of the element it is about, when the thrower knew it.
    pub fn element(&self) -> Option<JStr> {
        match self {
            Thrown::Compile(e) => e.element.clone().map(JStr),
            _ => None,
        }
    }

    /// `catch (ModelException e)`.
    pub fn is_model(&self) -> bool {
        matches!(self, Thrown::Compile(e) if e.kind == ErrorKind::Model)
    }

    /// `catch (NotImplementedException e)`: a `WalledBodyException` too (its subclass).
    pub fn is_not_implemented(&self) -> bool {
        matches!(self, Thrown::Compile(e) if matches!(e.kind, ErrorKind::NotImplemented | ErrorKind::WalledBody))
    }

    /// `catch (NotImplementedException | ModelException e)`, the per-set, per-class and per-association catches and
    /// the lifted views' (MN:345, :364, :419; LiftedViews:109, :149).
    pub fn is_model_or_not_implemented(&self) -> bool {
        self.is_model() || self.is_not_implemented()
    }

    /// A catch of any `LegendCompileException` (a `NotImplementedException` is none: lite-error's
    /// `ErrorKind::is_compile_error`).
    pub fn is_compile(&self) -> bool {
        matches!(self, Thrown::Compile(e) if e.kind.is_compile_error())
    }

    /// Whether this is a body not ported yet.
    pub fn is_unported(&self) -> bool {
        matches!(self, Thrown::Unported(_))
    }

    /// `String.valueOf(e.getMessage())`: the message, or `"null"`.
    pub fn message_or_null(&self) -> JStr {
        self.message().unwrap_or_else(|| JStr::from("null"))
    }

    /// `String.valueOf(e.getMessage()).split("\n")[0]`: a wall's line (lite-names' rule, Java's `split`).
    pub fn first_line(&self) -> JStr {
        lite_names::error::first_line(&self.message_or_null())
    }

    /// `MappingNormalizer.withElement`'s conversion (MN:238-256; census §9.4): what escapes a mapping's normalization
    /// carries the element's FQN.
    /// - a `ModelException` that names an element passes unchanged; one that names none is rebuilt with `element_fqn`
    ///   (its phase and message kept);
    /// - a `NotImplementedException` (a deliberate wall) becomes `ModelException(NORMALIZE, message, element_fqn)`;
    /// - anything else (a JDK exception, a resolver's error, an unported body) stays raw: a genuine bug fails the
    ///   build, never walls an element away.
    ///
    /// Java's `MappingResolutionException` arm (MN:246-247) is not ported (census §3: nothing E reaches throws one),
    /// so one would pass unchanged here.
    pub fn with_element(self, element_fqn: &JStr) -> Thrown {
        match self {
            Thrown::Compile(e) if e.kind == ErrorKind::Model => {
                if e.element.is_some() {
                    Thrown::Compile(e)
                } else {
                    Thrown::Compile(CompileError {
                        element: Some(element_fqn.0.clone()),
                        ..e
                    })
                }
            }
            Thrown::Compile(e) if matches!(e.kind, ErrorKind::NotImplemented | ErrorKind::WalledBody) => {
                Thrown::Compile(CompileError {
                    kind: ErrorKind::Model,
                    phase: Some(Phase::Normalize),
                    message: e.message,
                    element: Some(element_fqn.0.clone()),
                    unjudged_reason: None,
                })
            }
            other => other,
        }
    }
}

impl From<lite_names::Thrown> for Thrown {
    /// What the names layer throws, unchanged in class, phase, message and element.
    fn from(e: lite_names::Thrown) -> Thrown {
        match e {
            lite_names::Thrown::Compile(c) => Thrown::Compile(c),
            lite_names::Thrown::Java { class, message } => Thrown::Java { class, message },
        }
    }
}

impl From<lite_types::TypeError> for Thrown {
    /// A type family's refusal, as the names layer throws it (`lite_names::Thrown::from_type_error`).
    fn from(e: lite_types::TypeError) -> Thrown {
        Thrown::from(lite_names::Thrown::from_type_error(e))
    }
}

/// `Objects.requireNonNull(v[, message])`: the value, or the `NullPointerException`.
pub fn require<T>(v: Option<T>, message: Option<&str>) -> Result<T, Thrown> {
    v.ok_or_else(|| Thrown::null_pointer(message))
}

/// A body the skeleton has not ported, in a function with no `Result` (one that cannot fail in Java): it panics with
/// its site, loudly; never a value. Each use is marked `// UNPORTED: Class.method` beside it.
// skeleton: remove when every body is ported
#[macro_export]
macro_rules! unported {
    ($site:literal) => {
        unimplemented!(concat!("UNPORTED: ", $site))
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    use lite_model::text::j;

    #[test]
    fn with_element_attributes_a_model_exception_and_keeps_its_phase() {
        let e = Thrown::model(Phase::Mapping, j("Can't find mapping 'a::M'"), None).with_element(&j("a::N"));
        assert!(e.is_model());
        assert_eq!(e.phase(), Some(Phase::Mapping));
        assert_eq!(e.element(), Some(j("a::N")));
        assert_eq!(e.message(), Some(j("Can't find mapping 'a::M'")));
        // an element already named stays
        let named = Thrown::model(Phase::Model, j("m"), Some(&j("x::Y"))).with_element(&j("a::N"));
        assert_eq!(named.element(), Some(j("x::Y")));
    }

    #[test]
    fn with_element_turns_a_not_implemented_into_a_normalize_model_exception() {
        let e = Thrown::not_implemented(j("roadmap")).with_element(&j("a::M"));
        assert!(e.is_model());
        assert_eq!(e.java_class(), "com.legend.error.ModelException");
        assert_eq!(e.phase(), Some(Phase::Normalize));
        assert_eq!(e.element(), Some(j("a::M")));
        assert_eq!(e.message(), Some(j("roadmap")));
        let walled = Thrown::Compile(CompileError::walled_body(j("w").0)).with_element(&j("a::M"));
        assert!(walled.is_model());
    }

    #[test]
    fn with_element_leaves_bugs_and_unported_bodies_raw() {
        let ise = Thrown::illegal_state(j("bug")).with_element(&j("a::M"));
        assert_eq!(ise, Thrown::illegal_state(j("bug")));
        let unported = Thrown::Unported("X.y").with_element(&j("a::M"));
        assert_eq!(unported, Thrown::Unported("X.y"));
    }

    #[test]
    fn unported_matches_no_catch() {
        let u = Thrown::Unported("MappingNormalizer.synthM2M");
        assert!(!u.is_model());
        assert!(!u.is_not_implemented());
        assert!(!u.is_model_or_not_implemented());
        assert!(!u.is_compile());
        assert!(u.is_unported());
        assert_eq!(u.java_class(), UNPORTED);
        assert_eq!(u.message(), Some(j("not ported yet: MappingNormalizer.synthM2M")));
    }

    #[test]
    fn the_catch_predicates_take_what_java_takes() {
        let nie = Thrown::not_implemented(j("n"));
        assert!(nie.is_not_implemented() && !nie.is_model() && !nie.is_compile());
        let me = Thrown::model(Phase::Normalize, j("m"), None);
        assert!(me.is_model() && me.is_compile() && !me.is_not_implemented());
        let re = Thrown::from(lite_names::Thrown::resolution(j("r")));
        assert!(re.is_compile() && !re.is_model());
        let npe = Thrown::null_pointer(Some("ledger"));
        assert!(!npe.is_compile() && !npe.is_model_or_not_implemented());
        assert_eq!(npe.java_class(), NULL_POINTER);
    }

    #[test]
    fn a_wall_line_is_javas_first_split_part() {
        assert_eq!(
            Thrown::model(Phase::Normalize, j("one\ntwo"), None).first_line(),
            j("one")
        );
        assert_eq!(
            Thrown::Java {
                class: ILLEGAL_STATE,
                message: None
            }
            .first_line(),
            j("null")
        );
        assert_eq!(
            require::<u8>(None, Some("view set id")),
            Err(Thrown::null_pointer(Some("view set id")))
        );
    }

    #[test]
    fn never_fired_is_the_funnels_text() {
        assert_eq!(
            Thrown::never_fired("ModelNormalizer#1").message(),
            Some(j(
                "F7.8: empty answer at ModelNormalizer#1 (this default never fired on the 2026-09-15 B5 census; a miss \
                 here is a real model gap)"
            ))
        );
    }
}
