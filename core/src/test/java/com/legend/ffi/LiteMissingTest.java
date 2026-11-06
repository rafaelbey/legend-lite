// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

package com.legend.ffi;

import static org.junit.jupiter.api.Assertions.assertNotNull;
import static org.junit.jupiter.api.Assertions.assertThrows;
import static org.junit.jupiter.api.Assertions.assertTrue;

import org.junit.jupiter.api.Test;

/**
 * A Rust library that is not there says so at every call (Lite.shared): never an {@code ExceptionInInitializerError}
 * with no message, then a bare {@code NoClassDefFoundError} -- what a parse at class load would leave a user with
 * (docs/rust-port/PHASE2.md §3, the class-load trap). Its own JVM (//core:ffi_missing_test), the library named where
 * none is.
 */
class LiteMissingTest {

    @Test
    void aMissingLibraryIsTheSameClearFailureAtEveryCall() {
        for (int call = 0; call < 2; call++) {
            IllegalStateException e = assertThrows(IllegalStateException.class, Lite::shared);
            assertNotNull(e.getMessage(), "the failure says what failed");
            assertTrue(e.getMessage().contains("no-such-lite-library"), e.getMessage());
        }
    }
}
