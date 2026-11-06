// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

package com.legend.parser;

import java.util.LinkedHashMap;
import java.util.Map;
import java.util.Optional;

/**
 * THE OVERLAY REGISTRY (GRAMMAR_EXTENSIBILITY.md / PARSER_DROP_IN_PLAN.md Phase M): the {@code ###} sections a
 * third-party grammar claims through the {@code com.legend.spi.SectionGrammar} SPI, found by {@code ServiceLoader} --
 * {@code ###Name} &rarr; the grammar that owns it. The built-in sections are the parser's own (the Rust library's,
 * {@link RustParsers#knownSections}); the model door is told which overlays are registered ({@link #overlays}), hands
 * back each overlay section it meets, and {@link PmcdParser#parse} calls the grammar this registry holds for it. An
 * unknown section is never skipped in silence: the engine's and lite's dialects refuse it by name, the platform's
 * records it ({@code PmcdParser.Parsed#unclaimed()}).
 *
 * <p>Until 2026-11-06 the registry held a built-in half too, a stand-in or a Java grammar for every section the Java
 * parser read; it went with that parser (docs/rust-port/PHASE2.md §4 step 6), and the Rust parser decides its own
 * sections. An overlay registered under a built-in section's name is the Rust walk's to route (the engine's own
 * shadowing rule).
 */
public final class SectionGrammarRegistry {

    private SectionGrammarRegistry() {
    }

    /** The ServiceLoader's grammars, loaded once, in order: before {@link #REGISTRY}, whose {@link #build} reads them. */
    private static final java.util.List<com.legend.spi.SectionGrammar> LOADED = java.util.ServiceLoader
            .load(com.legend.spi.SectionGrammar.class).stream().map(java.util.ServiceLoader.Provider::get).toList();

    private static final Map<String, com.legend.spi.SectionGrammar> REGISTRY =
            build();

    private static Map<String, com.legend.spi.SectionGrammar> build() {
        Map<String, com.legend.spi.SectionGrammar> m = new LinkedHashMap<>();
        // in the ServiceLoader's order: a later grammar of the same name wins
        for (com.legend.spi.SectionGrammar g : LOADED) {
            m.put(g.name(), g);
        }
        return m;
    }

    /** The overlay grammars the ServiceLoader registered, in order (a later one of the same name wins): what the Rust
     *  model door is told is registered beside its own grammars (docs/rust-port/PHASE2.md, "The model door's
     *  overlays"). */
    public static java.util.List<RustParsers.Overlay> overlays() {
        java.util.List<RustParsers.Overlay> out = new java.util.ArrayList<>();
        for (com.legend.spi.SectionGrammar g : LOADED) {
            out.add(new RustParsers.Overlay(g.name(), g.lexable()));
        }
        return java.util.List.copyOf(out);
    }

    /** The overlay grammar registered for {@code sectionName}, if any. */
    public static Optional<com.legend.spi.SectionGrammar> lookup(String sectionName) {
        return Optional.ofNullable(REGISTRY.get(sectionName));
    }
}
