// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! `NativeFn`: the native function families -- the catalog's natives grouped by the code that implements them, each
//! family a closed type whose members carry their FQN and catalog overloads ([`NativeId`]s, so a constant the catalog
//! does not have does not compile).
//!
//! SKELETON (2026-11-12, slice 1b): the shared API the platform builds against; the families' members are filled in by
//! the port of `NativeFn.java`.

use crate::pure::NativeId;

/// `NativeFn.Member`: one member of a family -- the FQN it implements and every catalog overload of it.
pub trait Member: Copy + std::fmt::Debug + Send + Sync + 'static {
    /// The member's Java name (`name()`).
    fn name(self) -> &'static str;

    /// The FQN the member implements.
    fn fqn(self) -> &'static str;

    /// Every catalog overload of it, in order.
    fn overloads(self) -> &'static [NativeId];

    /// The bare function name (`assertEquals`), for messages: the FQN after its last `:`.
    fn bare_name(self) -> &'static str {
        let f = self.fqn();
        &f[f.rfind(':').map_or(0, |i| i + 1)..]
    }

    /// Whether an applied name (bare, or qualified) spells this member.
    fn matches(self, applied: &str) -> bool {
        applied == self.fqn()
            || applied == self.bare_name()
            || applied
                .strip_suffix(self.bare_name())
                .is_some_and(|head| head.ends_with("::"))
    }
}

/// The families, in `NativeFn.families()`'s order (Java puts `RowGetter` twice; a `LinkedHashMap` keeps its first
/// place).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Family {
    Calendar,
    Verdict,
    RowGetter,
    Frame,
    LowererForm,
    LiteralForm,
    ContextOption,
    PlanWrapper,
    RelationQuantifier,
    GroupAggregate,
    ObjectReference,
    SubtypeForm,
    ResolverForm,
    LiteDesugar,
    TyperForm,
    JavaRoutine,
    Handle,
    Effect,
    Carrier,
    ContextOwner,
    DdlStatement,
}

impl Family {
    /// Every family, in `families()`'s order.
    pub const VALUES: [Family; 21] = [
        Family::Calendar,
        Family::Verdict,
        Family::RowGetter,
        Family::Frame,
        Family::LowererForm,
        Family::LiteralForm,
        Family::ContextOption,
        Family::PlanWrapper,
        Family::RelationQuantifier,
        Family::GroupAggregate,
        Family::ObjectReference,
        Family::SubtypeForm,
        Family::ResolverForm,
        Family::LiteDesugar,
        Family::TyperForm,
        Family::JavaRoutine,
        Family::Handle,
        Family::Effect,
        Family::Carrier,
        Family::ContextOwner,
        Family::DdlStatement,
    ];

    /// The family's name, as `families()` keys it (the Java enum's simple name).
    pub fn name(self) -> &'static str {
        match self {
            Family::Calendar => "Calendar",
            Family::Verdict => "Verdict",
            Family::RowGetter => "RowGetter",
            Family::Frame => "Frame",
            Family::LowererForm => "LowererForm",
            Family::LiteralForm => "LiteralForm",
            Family::ContextOption => "ContextOption",
            Family::PlanWrapper => "PlanWrapper",
            Family::RelationQuantifier => "RelationQuantifier",
            Family::GroupAggregate => "GroupAggregate",
            Family::ObjectReference => "ObjectReference",
            Family::SubtypeForm => "SubtypeForm",
            Family::ResolverForm => "ResolverForm",
            Family::LiteDesugar => "LiteDesugar",
            Family::TyperForm => "TyperForm",
            Family::JavaRoutine => "JavaRoutine",
            Family::Handle => "Handle",
            Family::Effect => "Effect",
            Family::Carrier => "Carrier",
            Family::ContextOwner => "ContextOwner",
            Family::DdlStatement => "DdlStatement",
        }
    }

    /// Each member of the family, in its enum's order: its Java name, its FQN, its catalog overloads.
    pub fn members(self) -> Vec<(&'static str, &'static str, &'static [NativeId])> {
        // SKELETON: filled in by the port of NativeFn.java
        Vec::new()
    }
}

/// `NativeFn.claims(fqn)`: whether the FQN is a member of any family.
pub fn claims(fqn: &str) -> bool {
    Family::VALUES
        .iter()
        .any(|f| f.members().iter().any(|(_, m, _)| *m == fqn))
}
