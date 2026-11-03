// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

package com.legend.sql.dialect;

/**
 * H2 2.3+ -- since 2026-11-03 the Rust library's (docs/rust-port/PHASE1.md §4 step 4): {@link H2}'s dialect with the
 * typed-JSON navigation 2.3 added ({@code (x)."key"}, 1-based {@code (x)[i]}), chosen for a server that is not one of
 * H2's pinned versions ({@link H2#forServer}).
 */
public class H2Modern extends H2 {

    public H2Modern() {
        super("H2Modern");
    }
}
