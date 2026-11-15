// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! `Pipeline` (Pipeline.java at ad86fa895): the emission frame -- the accumulated relation expression plus the aliases
//! of the hops already emitted. Each alias is a sub-row slot (`joinSlot`) or a class-instance slot (`legacyNavigate`).
//!
//! RE-EXPRESSED (EF_CENSUS §4, the `Pipeline` and ROT `PipelineView` rows), with the same answers:
//! - **A plain struct.** Every map whose order is iterated keeps insertion order ([`LinkedMap`], [`LinkedSet`] for
//!   `inlineStack`, whose order a cycle text prints); the three `HashSet`s (`classSlots`, `ambiguousTables`,
//!   `droppedRoutedProps`) are only added to and tested, so they are any set.
//! - **The ledger is `Option`**, `None` on a VIEW pipeline (Java's `ledgerOrNull`), with Java's throwing getter
//!   ([`Pipeline::ledger`]). The pipeline holds it EXCLUSIVELY (`&mut`) while it lives: UnionSynthesis:392 poisons
//!   through `p.ledger()`, and at both construction sites (MN:1712, JCE:936) the ledger is used only through the
//!   pipeline from then on. (The census's `Option<&MappingLedger>` could not take that write; a fill helper that needs
//!   the ledger beside a live pipeline asks the pipeline for it.)
//! - **`PipelineView` is `Option<&Pipeline>`** (ROT-2): `None` is Java's `PipelineView.NONE`, the five methods the
//!   [`PipelineView`] trait on it, `NONE`'s answers on `None`. `hasSlots()` is constantly true on a pipeline.

use std::collections::HashSet;
use std::sync::Arc;

use lite_front::records as p;
use lite_model::linked::LinkedMap;
use lite_model::{JStr, JoinChainElement};

use crate::error::Thrown;
use crate::jdk::{LinkedSet, equals_ignore_case};
use crate::join_chain_emission;
use crate::lifted_views::LiftedViews;
use crate::mapping_ledger::MappingLedger;
use crate::union_synthesis::UnionRoute;

/// The emission frame.
pub struct Pipeline<'p> {
    /// The accumulated relation expression (Java's `expr`).
    pub expr: p::ValueSpecification,
    /// slot alias -> the TARGET table (or view) the hop reaches.
    pub alias_to_target_table: LinkedMap<JStr, JStr>,
    /// The class-instance (`legacyNavigate`) slots.
    pub class_slots: HashSet<JStr>,
    /// The structural identity of each emitted physical sub-row hop: the ordered join names from the main table -> the
    /// slot name used. The dedup key (a chain `[A, B]` never collides with a join literally named `A__B`) and the
    /// lookup readers use to recover the slot name.
    pub path_to_slot: LinkedMap<Vec<JStr>, JStr>,
    /// Class-typed navigate slots: property name -> MINTED alias (differs when the property name collides with a
    /// physical main-table column).
    pub nav_slot_by_prop: LinkedMap<JStr, JStr>,
    /// The Inline embedded set ids being spliced right now, outermost first; a set already on the stack is loud.
    pub inline_stack: LinkedSet<JStr>,
    /// Nav-slot OWNER class (property name -> owning class FQN): the collision guards fire only across DIFFERENT
    /// owners.
    pub nav_slot_owner: LinkedMap<JStr, JStr>,
    /// Physical (non-class) target tables reached by MORE THAN ONE distinct sub-row slot and which are NOT the main
    /// table: a bare column ref to one is left unbound in the row scope and fails loudly.
    pub ambiguous_tables: HashSet<JStr>,
    /// Routed class-typed navigations: property -> per-PM route entries, classified from each PM's own
    /// `Join.targetSetId`; one navigate per property emits the OR over all entries.
    pub union_routes: LinkedMap<JStr, Vec<UnionRoute>>,
    /// Routed properties DROPPED from this synthesis (reason on the poison ledger): their PMs emit nothing.
    pub dropped_routed_props: HashSet<JStr>,
    /// The per-MAPPING ledger this synthesis records into; none on a VIEW pipeline.
    ledger: Option<&'p mut MappingLedger>,
    /// The lifted view bodies every hop onto a view reads (E.5).
    pub views: &'p LiftedViews<'p>,
    /// Column names of each hoisted chain slot's TARGET relation (table or view), beside `alias_to_target_table`.
    pub alias_to_target_columns: LinkedMap<JStr, LinkedSet<JStr>>,
}

impl<'p> Pipeline<'p> {
    /// `new Pipeline(expr, ledger, views)` (:73): a mapping synthesis's pipeline, recording into `ledger`.
    pub fn new(expr: p::ValueSpecification, ledger: &'p mut MappingLedger, views: &'p LiftedViews<'p>) -> Pipeline<'p> {
        Pipeline::with(expr, Some(ledger), views)
    }

    /// `Pipeline.forView(expr, views)` (:85): a VIEW's relation pipeline -- physical hops only, no ledger.
    pub fn for_view(expr: p::ValueSpecification, views: &'p LiftedViews<'p>) -> Pipeline<'p> {
        Pipeline::with(expr, None, views)
    }

    fn with(
        expr: p::ValueSpecification,
        ledger: Option<&'p mut MappingLedger>,
        views: &'p LiftedViews<'p>,
    ) -> Pipeline<'p> {
        Pipeline {
            expr,
            alias_to_target_table: LinkedMap::new(),
            class_slots: HashSet::new(),
            path_to_slot: LinkedMap::new(),
            nav_slot_by_prop: LinkedMap::new(),
            inline_stack: LinkedSet::new(),
            nav_slot_owner: LinkedMap::new(),
            ambiguous_tables: HashSet::new(),
            union_routes: LinkedMap::new(),
            dropped_routed_props: HashSet::new(),
            ledger,
            views,
            alias_to_target_columns: LinkedMap::new(),
        }
    }

    /// `ledger()` (:89): loud on a view pipeline.
    pub fn ledger(&self) -> Result<&MappingLedger, Thrown> {
        self.ledger.as_deref().ok_or_else(no_ledger)
    }

    /// `ledger()` for a write (UnionSynthesis:392's poison): loud on a view pipeline.
    pub fn ledger_mut(&mut self) -> Result<&mut MappingLedger, Thrown> {
        self.ledger.as_deref_mut().ok_or_else(no_ledger)
    }

    /// `view()` (:98): the translator-facing view of this pipeline (seam b).
    pub fn view(&self) -> Option<&Pipeline<'p>> {
        Some(self)
    }
}

fn no_ledger() -> Thrown {
    Thrown::illegal_state(JStr::from(
        "a view pipeline carries no mapping ledger (it emits physical hops only)",
    ))
}

/// `RelOpTranslator.PipelineView` (RelOpTranslator.java:57-91), on `Option<&Pipeline>`: `Some` is a pipeline's
/// `view()` (Pipeline.java:98-123), `None` is `PipelineView.NONE`.
pub trait PipelineView {
    /// `ambiguousTables().contains(table)` (its one use; `NONE`'s set is empty).
    fn is_ambiguous(&self, table: &JStr) -> bool;
    /// `hasSlots()`: true on a pipeline, false on `NONE`.
    fn has_slots(&self) -> bool;
    /// `slotFor(chain)`: `JoinChainEmission.slotFor(pipeline, chain)`; on `NONE` the unreachable internal error
    /// (the translator asks only when `has_slots`).
    fn slot_for(&self, chain: &[Arc<JoinChainElement>]) -> Result<JStr, Thrown>;
    /// `targetTable(alias)`: the slot's target table; none for no alias, an unknown alias, or `NONE`.
    fn target_table(&self, alias: Option<&JStr>) -> Option<&JStr>;
    /// `targetHasColumn(alias, column)`: whether the slot's target relation has `column`, by `equalsIgnoreCase`;
    /// false on `NONE`.
    fn target_has_column(&self, alias: Option<&JStr>, column: &JStr) -> bool;
}

impl PipelineView for Option<&Pipeline<'_>> {
    fn is_ambiguous(&self, table: &JStr) -> bool {
        self.is_some_and(|p| p.ambiguous_tables.contains(table))
    }

    fn has_slots(&self) -> bool {
        self.is_some()
    }

    fn slot_for(&self, chain: &[Arc<JoinChainElement>]) -> Result<JStr, Thrown> {
        match self {
            Some(p) => join_chain_emission::slot_for(p, chain),
            None => Err(Thrown::illegal_state(JStr::from("no pipeline slots in this context"))),
        }
    }

    fn target_table(&self, alias: Option<&JStr>) -> Option<&JStr> {
        let p = (*self)?;
        p.alias_to_target_table.get(alias?)
    }

    fn target_has_column(&self, alias: Option<&JStr>, column: &JStr) -> bool {
        let (Some(p), Some(alias)) = (*self, alias) else {
            return false;
        };
        p.alias_to_target_columns
            .get(alias)
            .is_some_and(|cols| cols.iter().any(|c| equals_ignore_case(&c.0, &column.0)))
    }
}
