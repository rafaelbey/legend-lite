// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

package com.legend.sql.dialect;

/**
 * THE ORDER RULING's test-lane pass (user, 2026-09-20): product SQL carries no ordering the query did not ask for; the
 * emulation of the engine's insertion order is a feature of the corpus lanes, switched on by their build flags (the
 * engine scan-order switch the Rust dialect's shim reads). Since 2026-11-04 every dialect is the Rust library's, and so
 * is the pass
 * (rust/lite-dialect/src/scan_order.rs, installed by DuckDB's pass list behind the switch): this class keeps only its
 * count, which the corpus runner attributes to the test that ran ({@code rcorpus/<lane>-engine-order-register.txt}).
 */
public final class StableScanOrder {

    private StableScanOrder() {
    }

    /** The statements the pass CHANGED in this process (an order the statement lacked was added): the library's
     *  count, every dialect's (rust/lite-dialect/SEAM.md, scanOrderFirings). */
    public static long firings() {
        return RustDialects.provider().scanOrderFirings();
    }
}
