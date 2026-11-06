// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

package com.legend.equivalence;

import com.legend.model.ParsedModel;
import com.legend.parser.RustParsers;

import java.util.List;

/**
 * THE THREE SURFACES, each named ONCE for the whole parity harness —
 * this module's job is comparing the levels against the referee, and
 * these are the levels it compares.
 *
 * <p>Beside the text doors, what the gate tests ask of the parser below
 * them -- the section headers, the strict element sites, the Database
 * sites, the known sections -- is asked of the Rust parser
 * ({@link RustParsers}' probes, docs/rust-port/PHASE2.md §4 step 6a),
 * where it was asked of the Java lexer's and parser's internals.
 */
public final class Surfaces {

    private Surfaces() {
    }

    public static ParsedModel platform(String source) {
        return com.legend.door.ModelFromText.parse(source,
                com.legend.parser.Dialect.LEGEND_PLATFORM);
    }

    public static ParsedModel lite(String source) {
        return com.legend.door.ModelFromText.parse(source,
                com.legend.parser.Dialect.LEGEND_LITE);
    }

    public static ParsedModel engine(String source) {
        return com.legend.door.ModelFromText.parse(source,
                com.legend.parser.Dialect.LEGEND_ENGINE);
    }

    /** The strict surface's second half, engine level: every top-level
     *  declaration parsed where it stands in the whole text's tokens (so
     *  source positions stay file-absolute) -- protocol-only constructs
     *  like test suites are read there; the first one's error thrown. */
    public static void engineElementSites(String source) {
        RustParsers.provider().elementSites(source,
                com.legend.parser.Dialect.LEGEND_ENGINE);
    }

    /** Every {@code Database} element where a declaration can stand, engine
     *  level (byte parity's subject is the drop-in surface): each one's
     *  qualified name and its legend-engine JSON. */
    public static List<RustParsers.DatabaseSite> engineDatabases(String source) {
        return RustParsers.provider().databaseSites(source,
                com.legend.parser.Dialect.LEGEND_ENGINE);
    }

    /** The lexer's {@code ###Section} headers, in order: name and content offset. */
    public static List<RustParsers.SectionHeader> sectionHeaders(String source) {
        return RustParsers.provider().sectionHeaders(source);
    }

    /** The sections the parser reads with a grammar of its own (the overlays
     *  a ServiceLoader registers are {@code SectionGrammarRegistry.overlays()}). */
    public static List<String> knownSections() {
        return RustParsers.provider().knownSections();
    }
}
