// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

package com.legend.sql.dialect;

import com.legend.sql.SqlDdl;

import java.util.Map;
import java.util.ServiceLoader;

/**
 * The dialects the Rust library answers (docs/rust-port/PHASE1.md), as this package sees them: a dialect by its Java
 * name, and DuckDB's own tables. The SQL layer depends on nothing outside itself (ArchitectureTest, invariant 6a), so
 * the implementation -- {@code com.legend.ffi}'s, over the C library -- is found through the SPI seam,
 * {@link ServiceLoader}: whatever runs core carries //core:rust_dialect on its runtime classpath.
 */
public interface RustDialects {

    /** The dialect the Rust library answers for {@code name} (Java's simple class name: {@code DuckDb}). */
    SqlDialect dialect(String name);

    /** The dialect's quote character and reserved words. */
    Lexicon lexicon(String name);

    /** The canonical catalog types the dialect declares, and how, in the library's order. */
    Map<String, CatalogType> catalogTypes(String name);

    /** The canonical catalog types the dialect refuses, each with its reason. */
    Map<String, String> catalogRefused(String name);

    /** Whether the dialect reads a column stored as {@code type} through its stored read ({@link StoredReads}). */
    boolean readsStored(String name, SqlDdl.ColumnType type);

    /** The statements the library's {@link StableScanOrder} has changed in this process, every dialect's. */
    long scanOrderFirings();

    /** The provider, found once. */
    static RustDialects provider() {
        return Found.PROVIDER;
    }

    /** The provider's holder: loaded on first use. */
    final class Found {
        private Found() {
        }

        static final RustDialects PROVIDER = ServiceLoader.load(RustDialects.class).findFirst()
                .orElseThrow(() -> new IllegalStateException("no Rust dialect provider: //core:rust_dialect is not on"
                        + " the runtime classpath (its META-INF/services names com.legend.ffi.RustDialectProvider)"));
    }
}
