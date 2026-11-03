// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

package com.legend.sql.dialect;

/**
 * legend-engine's own SQL TEXT for a Composite store -- since 2026-11-03 the Rust library's
 * (docs/rust-port/PHASE1.md §4 step 4): the engine's DEFAULT spellings (native trim/pad/cbrt like DB2's 'common' goldens,
 * but WITHOUT DB2's own quirks: {@code CHARACTER_LENGTH(x,CODEUNITS32)} stays plain {@code char_length}). Divergent
 * goldens fail honestly.
 */
public final class EngineStyleComposite extends EngineStyleDB2 {

    public EngineStyleComposite() {
        super("EngineStyleComposite", false, null);
    }
}
