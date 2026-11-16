// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! The element differential's support (2026-11-15, the element slice, step 5): the planner's boot and
//! `normalizeWithSystem` ([`boot`]), the registrations F is handed ([`registrations`]), one ask's canonical text as
//! ElementAnswers writes it ([`answers`]), the generated mirrors of the records it writes ([`oracle_records`]) and the
//! hand-written conversion of lite-element's and lite-types' values into them ([`views`]).

pub mod answers;
pub mod boot;
#[rustfmt::skip] // generated, and held byte for byte to its generator: //rust/lite-element:update_generated_test
pub mod oracle_records;
pub mod registrations;
pub mod views;
