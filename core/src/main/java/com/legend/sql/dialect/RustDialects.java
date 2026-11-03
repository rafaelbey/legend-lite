// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

package com.legend.sql.dialect;

import com.legend.base.Nullable;
import com.legend.sql.SqlDdl;
import com.legend.sql.SqlExpr;
import com.legend.sql.SqlQuery;

import java.util.Map;
import java.util.ServiceLoader;

/**
 * The dialects the Rust library answers (docs/rust-port/PHASE1.md), as this package sees them: a dialect by its Java
 * name, and DuckDB's own tables. The SQL layer depends on nothing outside itself (ArchitectureTest, invariant 6a), so
 * the implementation -- {@code com.legend.ffi}'s, over the C library -- is found through the SPI seam,
 * {@link ServiceLoader}: whatever runs core carries //core:rust_dialect on its runtime classpath.
 */
public interface RustDialects {

    /** The dialect the Rust library answers for {@code name} (Java's simple class name: {@code DuckDb}, {@code H2}). */
    SqlDialect dialect(String name);

    /** The engine-text dialect the Rust library answers for {@code name} ({@code EngineStyleH2}, ...), made as Java's
     *  {@code new EngineStyleH2(quoteIdentifiers, timeZone)} is. */
    EngineText engineText(String name, boolean quoteIdentifiers, @Nullable String timeZone);

    /** The name of the dialect {@code name}'s {@code forServer(version)} takes ({@code H2} for a 2.1 or 2.2 server,
     *  {@code H2Modern} for any other; a dialect that keeps itself names itself). */
    String forServer(String name, String version);

    /** The dialect's quote character and reserved words. */
    Lexicon lexicon(String name);

    /** The canonical catalog types the dialect declares, and how, in the library's order. */
    Map<String, CatalogType> catalogTypes(String name);

    /** The canonical catalog types the dialect refuses, each with its reason. */
    Map<String, String> catalogRefused(String name);

    /** Whether the dialect reads a column stored as {@code type} through its stored read ({@link StoredReads}). */
    boolean readsStored(String name, SqlDdl.ColumnType type);

    /** A table's name as the dialect writes a query's read of it. */
    String tableName(String name, String table, com.legend.sql.NameQuoting quoting);

    /** The statements the library's {@link StableScanOrder} has changed in this process, every dialect's. */
    long scanOrderFirings();

    /**
     * An engine-text dialect as the Rust library answers it (rust/lite-dialect/SEAM.md §3b): a {@link SqlDialect} and
     * the engine text's own entry points. Not {@link EngineTextDialect}, which the engine-text shims implement over
     * it: that interface belongs to the paths that print the engine's text (ArchitectureTest), and the library's
     * provider is not one. {@link #renderedAlias} takes the query whose alias plan it reads -- the library holds no
     * state between calls -- which the shim remembers from its last render.
     */
    interface EngineText extends SqlDialect {

        /** The query as {@link #render} writes it, with the aliases it carries. */
        String renderAsAliased(SqlQuery query);

        /** The select list of the query's first select as {@link #render} writes it; null for a {@code with}. */
        @Nullable String selectList(SqlQuery query);

        /** A collection-valued plan parameter spliced as the engine's plan text writes it. */
        String collectionSplice(SqlExpr.PlanParam param);

        /** {@code alias}'s rename in the engine's alias plan of {@code rendered} (as {@link #render} planned it), or
         *  the alias itself when there is no such query or it plans no rename of it. */
        String renderedAlias(String alias, @Nullable SqlQuery rendered);

        /** The query that reads the in-collection temp table back. */
        String inTempTableSelect(String tempTable);
    }

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
