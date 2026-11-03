// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

package com.legend.sql.dialect;

import com.legend.base.Nullable;

/**
 * legend-engine's own SQL TEXT for DB2 -- since 2026-11-03 the Rust library's (docs/rust-port/PHASE1.md §4 step 4):
 * {@link EngineStyleH2}'s text with the DB2 goldens' spellings (concat infix, DB2's function forms, the null-safe
 * expansions, a boolean placeholder quoted). Every unpinned form inherits the H2 spelling -- an unmatched golden stays an
 * HONEST text FAIL, never a guessed respell.
 */
public class EngineStyleDB2 extends EngineStyleH2 {

    public EngineStyleDB2() {
        this(false, null);
    }

    public EngineStyleDB2(boolean quoteIdentifiers, @Nullable String timeZone) {
        this("EngineStyleDB2", quoteIdentifiers, timeZone);
    }

    /** The DB2-family text the Rust library names {@code name}: this one's, or Composite's. */
    EngineStyleDB2(String name, boolean quoteIdentifiers, @Nullable String timeZone) {
        super(name, quoteIdentifiers, timeZone);
    }
}
