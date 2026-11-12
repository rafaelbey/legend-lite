// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! `Implementation`: what executes one declaration -- exactly one of six kinds. A declaration's implementation is a
//! fact about the declaration, keyed by its `FunctionId`; it never moves the declaration and never depends on how the
//! function's name is spelled.
//!
//! Ported from core's `com.legend.platform.Implementation` (2026-11-12, docs/rust-port/PHASE3.md slice 1b). Java's
//! records copy their sets into `LinkedHashSet`s (an `EnumSet` handed in iterates in its enum's order): a set here is a
//! list of distinct members in that order, compared as a set, as Java's records compare. The family classes Java's sets
//! hold (`Class<? extends NativeFn.Member>`) are [`Family`]s.

use lite_builtin::native_fn::Family;
use lite_model::JStr;

use crate::core_fn::CoreFn;
use crate::feature::Feature;

/// Where a registered lowering applies.
#[allow(non_camel_case_types)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Position {
    /// `Scalars.RULES`: a SQL expression.
    SCALAR,
    /// `Aggregates.REDUCERS`: a SQL aggregate.
    AGGREGATE,
    /// `Windows.FNS`: a window function.
    WINDOW,
    /// `Windows.AGGREGATES`: a window-only aggregate.
    WINDOW_AGGREGATE,
}

impl Position {
    /// Java's `values()`.
    pub const VALUES: [Position; 4] = [
        Position::SCALAR,
        Position::AGGREGATE,
        Position::WINDOW,
        Position::WINDOW_AGGREGATE,
    ];

    /// Java's `name()`.
    pub fn name(self) -> &'static str {
        match self {
            Position::SCALAR => "SCALAR",
            Position::AGGREGATE => "AGGREGATE",
            Position::WINDOW => "WINDOW",
            Position::WINDOW_AGGREGATE => "WINDOW_AGGREGATE",
        }
    }
}

/// Why a declaration is refused.
#[allow(non_camel_case_types)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Reason {
    /// The engine's machinery for a concern the platform serves itself (its SQL printer, its planner): WalledBodies.
    ENGINE_MACHINERY,
    /// A program whose value is never needed here: Subsumed.
    MOOT,
    /// A native the platform cannot implement (an effect with no database meaning), or a capability it does not model.
    CANNOT_IMPLEMENT,
    /// A version of a function the platform declares or implements in its own Pure, with no row of its own: its
    /// upstream body is the spec, never the platform's implementation.
    NO_ROW,
}

impl Reason {
    /// Java's `values()`.
    pub const VALUES: [Reason; 4] = [
        Reason::ENGINE_MACHINERY,
        Reason::MOOT,
        Reason::CANNOT_IMPLEMENT,
        Reason::NO_ROW,
    ];

    /// Java's `name()`.
    pub fn name(self) -> &'static str {
        match self {
            Reason::ENGINE_MACHINERY => "ENGINE_MACHINERY",
            Reason::MOOT => "MOOT",
            Reason::CANNOT_IMPLEMENT => "CANNOT_IMPLEMENT",
            Reason::NO_ROW => "NO_ROW",
        }
    }
}

/// A `java.lang.IllegalArgumentException` a caller can see: its message.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IllegalArgument {
    pub message: JStr,
}

impl IllegalArgument {
    /// The Java class.
    pub const JAVA_CLASS: &str = "java.lang.IllegalArgumentException";
}

/// The distinct members of `values`, in their first order (`new LinkedHashSet<>(values)`).
pub(crate) fn distinct<T: PartialEq + Copy>(values: &[T]) -> Vec<T> {
    let mut out: Vec<T> = Vec::with_capacity(values.len());
    for &v in values {
        if !out.contains(&v) {
            out.push(v);
        }
    }
    out
}

/// Java's `Set.equals`: the same members, in any order (both hold each member once).
fn same_set<T: PartialEq>(a: &[T], b: &[T]) -> bool {
    a.len() == b.len() && a.iter().all(|x| b.contains(x))
}

/// A language form: its own checker and tree node. A form may also carry lowering registrations it delegates its
/// generic shape to, and families that co-register the same function (recorded, so the co-ownership is visible).
#[derive(Clone, Debug, Eq)]
pub struct Form {
    form: CoreFn,
    also_lowered: Vec<Position>,
    also_families: Vec<Family>,
}

impl Form {
    /// `new Form(form, alsoLowered, alsoFamilies)`: each set copied, in its order.
    pub fn new(form: CoreFn, also_lowered: &[Position], also_families: &[Family]) -> Form {
        Form {
            form,
            also_lowered: distinct(also_lowered),
            also_families: distinct(also_families),
        }
    }

    pub fn form(&self) -> CoreFn {
        self.form
    }

    pub fn also_lowered(&self) -> &[Position] {
        &self.also_lowered
    }

    pub fn also_families(&self) -> &[Family] {
        &self.also_families
    }
}

impl PartialEq for Form {
    fn eq(&self, other: &Form) -> bool {
        self.form == other.form
            && same_set(&self.also_lowered, &other.also_lowered)
            && same_set(&self.also_families, &other.also_families)
    }
}

/// The platform's own implementation; any upstream body is not used. One function may lower in several positions --
/// one implementation. A feature flag may select another scalar rule for it.
#[derive(Clone, Debug, Eq)]
pub struct Intrinsic {
    positions: Vec<Position>,
    feature_overrides: Vec<Feature>,
    families: Vec<Family>,
}

impl Intrinsic {
    /// `new Intrinsic(positions, featureOverrides, families)`: each set copied, in its order; one with neither a
    /// position nor a family is refused, as Java's constructor refuses it.
    pub fn new(
        positions: &[Position],
        feature_overrides: &[Feature],
        families: &[Family],
    ) -> Result<Intrinsic, IllegalArgument> {
        if positions.is_empty() && families.is_empty() {
            return Err(IllegalArgument {
                message: JStr::from("an intrinsic registers at least one position or family"),
            });
        }
        Ok(Intrinsic {
            positions: distinct(positions),
            feature_overrides: distinct(feature_overrides),
            families: distinct(families),
        })
    }

    pub fn positions(&self) -> &[Position] {
        &self.positions
    }

    pub fn feature_overrides(&self) -> &[Feature] {
        &self.feature_overrides
    }

    pub fn families(&self) -> &[Family] {
        &self.families
    }
}

impl PartialEq for Intrinsic {
    fn eq(&self, other: &Intrinsic) -> bool {
        same_set(&self.positions, &other.positions)
            && same_set(&self.feature_overrides, &other.feature_overrides)
            && same_set(&self.families, &other.families)
    }
}

/// A body or native the platform deliberately does not run.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Refused {
    pub reason: Reason,
    pub why: JStr,
}

/// What executes one declaration.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Implementation {
    /// A language form.
    Form(Form),
    /// The platform's own implementation.
    Intrinsic(Intrinsic),
    /// Upstream's Pure body, compiled and inlined by the platform.
    Body,
    /// The platform's own Pure: the system metamodel's body implements this id over the platform's own rows. A
    /// program that declares the id keeps its declaration and takes that body; the platform's own declaration serves
    /// where no program declares the id. Compiled and inlined like a body.
    PlatformPure,
    /// Declared (an upstream native) with no implementation here.
    Unimplemented,
    /// A body or native the platform deliberately does not run.
    Refused(Refused),
}

impl Implementation {
    /// `Implementation.byRule(row)`: whether the row runs the declaration by the platform's own rule or form -- never
    /// by the declaration's body. None (undeclared) is not.
    pub fn by_rule(row: Option<&Implementation>) -> bool {
        matches!(row, Some(Implementation::Intrinsic(_) | Implementation::Form(_)))
    }

    /// The Java record's simple name.
    pub fn kind(&self) -> &'static str {
        match self {
            Implementation::Form(_) => "Form",
            Implementation::Intrinsic(_) => "Intrinsic",
            Implementation::Body => "Body",
            Implementation::PlatformPure => "PlatformPure",
            Implementation::Unimplemented => "Unimplemented",
            Implementation::Refused(_) => "Refused",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_intrinsic_registers_a_position_or_a_family() {
        assert_eq!(
            Intrinsic::new(&[], &[Feature::CORRECT_SQL_SUBSTRING_INDEXING], &[]),
            Err(IllegalArgument {
                message: JStr::from("an intrinsic registers at least one position or family")
            })
        );
        assert!(Intrinsic::new(&[], &[], &[Family::Verdict]).is_ok());
        assert!(Intrinsic::new(&[Position::SCALAR], &[], &[]).is_ok());
    }

    #[test]
    fn the_sets_compare_as_sets() {
        let a = Intrinsic::new(
            &[Position::AGGREGATE, Position::SCALAR],
            &[],
            &[Family::Verdict, Family::Calendar],
        );
        let b = Intrinsic::new(
            &[Position::SCALAR, Position::AGGREGATE, Position::SCALAR],
            &[],
            &[Family::Calendar, Family::Verdict],
        );
        assert_eq!(a, b);
        assert_eq!(b.unwrap().positions(), &[Position::SCALAR, Position::AGGREGATE]);
        assert_ne!(
            Form::new(CoreFn::MAP, &[Position::SCALAR], &[]),
            Form::new(CoreFn::MAP, &[Position::SCALAR, Position::WINDOW], &[])
        );
        assert!(Implementation::by_rule(Some(&Implementation::Form(Form::new(
            CoreFn::MAP,
            &[],
            &[]
        )))));
        assert!(!Implementation::by_rule(Some(&Implementation::PlatformPure)));
        assert!(!Implementation::by_rule(None));
    }
}
