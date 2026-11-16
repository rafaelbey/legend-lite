// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! The element differential's support (2026-11-15, the element slice, step 5): the planner's boot and
//! `normalizeWithSystem` ([`boot`]), the registrations F is handed ([`registrations`]).

// milestone 1 reads the boot's normalized half alone; the asks (the next commit) read the rest
#[allow(dead_code)]
pub mod boot;
#[allow(dead_code)]
pub mod registrations;
