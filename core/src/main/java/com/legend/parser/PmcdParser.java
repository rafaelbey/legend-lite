// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

package com.legend.parser;

import com.legend.protocol.Protocol;
import com.legend.protocol.ProtocolEmitter;
import com.legend.protocol.SourceInfo;

import java.util.ArrayList;
import java.util.List;
import java.util.Objects;

/**
 * THE MODEL DOORS: a model's text in, its records out -- every element in section order, then the
 * {@code __internal__::SectionIndex} the engine appends, exactly as {@code PureGrammarParser.parseModel} builds it --
 * or the engine's full PMCD JSON ({@code {"_type":"data","elements":[...]}}), at the {@link Dialect} the caller names.
 *
 * <p>The Rust library answers every door (docs/rust-port/PHASE2.md §4 steps 4 and 5: {@code rust/lite-front}'s
 * {@code model_parser.rs}, the section walk, and its section grammars over its lexer, through {@link RustParsers}):
 * the engine's section walk and import scoping at every dialect, engine-strict throughout at the engine's, an
 * unregistered section refused in the engine's own words; section spans in the engine's buffer coordinates (it
 * parses {@code "\n###Pure\n" + code}, PureGrammarParser.DEFAULT_SECTION_BEGIN). Java calls the overlay grammars
 * back ({@link SectionGrammarRegistry}'s ServiceLoader half) and splices their opaque elements ({@link #parse}). The
 * Java parser it ports was deleted on 2026-11-06 (step 6b); its answers to the front oracle's every question are
 * frozen in {@code rust/oracles/front/}, which {@code //rust/lite-front}'s differentials hold the library to.
 */
public final class PmcdParser {

    private PmcdParser() {
    }

    /** THE level the drop-in surface ({@link #parseDocument}, {@link #parseModel}) exists for, named once. */
    private static final Dialect LEVEL = Dialect.LEGEND_ENGINE;

    /** One parsed element: the path the sectionIndex lists (functions use their MANGLED path, like the engine), the
     *  PROTOCOL type, and the parsed RECORD itself, the protocol program's hub. */
    public record DocElement(@com.legend.base.Nullable String path,
            Class<? extends Protocol.Element> kind, Protocol.Element element) {

        /** Its wire JSON, emitted from the record when asked -- never on the compiler's path, which takes the record
         *  (leg 8: one parser, and the compile does not pay for JSON it does not read). */
        public String json() {
            return ProtocolEmitter.emitElement(element);
        }
    }

    /**
     * A text parsed at a dialect: its records (every element in section order, then the section index), and the
     * sections no grammar claims, which the platform dialect records instead of refusing (the engine's and lite's
     * refuse them by name).
     */
    public record Parsed(Protocol.PureModelContextData model, List<UnclaimedSection> unclaimed) {
    }

    /** A {@code ###} section no registered grammar claims, recorded and counted (the platform dialect), never
     *  silently skipped. */
    public record UnclaimedSection(String name, int startOffset, int endOffset) {
    }

    /** One document section with its parsed elements and imports. */
    public record DocSection(String parserName, List<DocElement> elements,
                             List<String> imports, SourceInfo span) {
    }

    /** Parse {@code source} into the engine's full PMCD JSON: the records of {@link #parseModel}, emitted. */
    public static String parseDocument(String source) {
        return parseDocument(source, true);
    }

    /**
     * Text to JSON for a model (the conversion legend-engine's {@code grammarToJson/model} makes, its
     * {@code returnSourceInformation} the second argument): {@link #parseDocument}, without any source
     * information when it is off -- every span, the named ones ({@code classSourceInformation}, ...) included, as the
     * engine's parser then records none (its span builder answers null; CorpusSweepTest's claim 1c).
     *
     * <p>The Rust library's answer (docs/rust-port/PHASE2.md §4 step 5): its parser and its emitter, in one call --
     * held to the Java parser's and emitter's frozen answer, cut by {@code SourceInformation.withoutSpans}, on every
     * model text of the front oracle (//rust/lite-front:front_engine_differential_test). <b>The overlay path</b>: when
     * the walk meets a section of a registered overlay grammar, Rust answers nothing and this method answers as Java
     * does -- {@link ProtocolEmitter} over {@link #parse}'s records, which call the overlay grammars, let one's
     * exception win, and splice their opaque elements in element order, where the emitter refuses the first lite-only
     * one. Rust never makes an opaque element. At the engine's dialect the walk refuses an unknown section before any
     * overlay grammar is consulted, so no text takes this path today; it is Java's answer by construction, not a
     * fallback.
     */
    public static String parseDocument(String source, boolean sourceInformation) {
        Objects.requireNonNull(source, "source");
        String json = RustParsers.provider().document(source, sourceInformation, SectionGrammarRegistry.overlays());
        if (json != null) {
            return json;
        }
        String emitted = ProtocolEmitter.emit(parse(source, LEVEL).model());
        return sourceInformation ? emitted : com.legend.protocol.SourceInformation.withoutSpans(emitted);
    }

    /**
     * Parse {@code source} into the typed protocol records (text to records, the protocol program's parse
     * leg): every element in section order, then the section index the engine appends.
     */
    public static Protocol.PureModelContextData parseModel(String source) {
        return parse(source, LEVEL).model();
    }

    /**
     * THE ONE PARSER (docs/PROTOCOL_PROGRAM_2026_10_05.md, leg 8): {@code source} at {@code dialect} into its records --
     * the engine's section walk and import scoping for every dialect (imports bound to their section), the dialect
     * deciding only what is accepted: the engine's exactly; lite's adds its declared extensions ({@code Primitive});
     * the platform's (lite's own Pure: the prelude, the system metamodel, the test harness) accepts any declaration in
     * its built-in sections, native declarations, and records an unclaimed section where the others refuse it. The
     * compiler builds from these records through its one door ({@code ModelFromProtocol}).
     *
     * <p>The Rust parser's answer (docs/rust-port/PHASE2.md §4 step 4, held to the Java parser's frozen answer to every
     * question of the front oracle by //rust/lite-front:front_parser_differential_test), its error this package's:
     * each overlay grammar called here on the section Rust names, in section order, and its elements spliced where
     * Java's walk put them -- through {@link OverlayElementSink}, the one producer of an opaque element. An overlay
     * grammar's exception wins over the parser's own error, as in Java's walk, which called it first.
     */
    public static Parsed parse(String source, Dialect dialect) {
        Objects.requireNonNull(source, "source");
        RustParsers.ModelAnswer answer = RustParsers.provider().model(source, dialect,
                SectionGrammarRegistry.overlays());
        List<List<Protocol.Element>> called = new ArrayList<>();
        for (RustParsers.OverlaySection o : answer.overlays()) {
            com.legend.spi.SectionGrammar g = SectionGrammarRegistry.lookup(o.name()).orElseThrow(
                    () -> new IllegalStateException("the Rust parser met an overlay section '" + o.name()
                            + "' no grammar is registered for"));
            List<Protocol.Element> opaque = new ArrayList<>();
            g.parse(new com.legend.spi.SectionSource(o.name(), source.substring(o.startOffset(), o.endOffset()),
                    o.startOffset(), o.endOffset(), o.startLine()), new OverlayElementSink(o.name(), opaque));
            called.add(opaque);
        }
        RuntimeException raised = answer.raised();
        if (raised != null) {
            throw raised;
        }
        Parsed parsed = Objects.requireNonNull(answer.parsed(), "the Rust model door answered neither records nor"
                + " an error");
        if (called.isEmpty()) {
            return parsed;
        }
        List<Protocol.Element> elements = new ArrayList<>(parsed.model().elements());
        // the section index is the answer's last element; every overlay's elements go before it
        Protocol.PSectionIndex index = (Protocol.PSectionIndex) elements.remove(elements.size() - 1);
        List<Protocol.PSection> sections = new ArrayList<>(index.sections());
        // last-first, so each section's element index is still the one Rust gave
        for (int k = called.size() - 1; k >= 0; k--) {
            RustParsers.OverlaySection o = answer.overlays().get(k);
            List<Protocol.Element> opaque = called.get(k);
            Protocol.PSection s = sections.get(o.sectionIndex());
            List<@com.legend.base.Nullable String> paths = new ArrayList<>(s.elements());
            for (Protocol.Element e : opaque) {
                String path = Protocol.pathOf(e);
                if (!paths.contains(path)) {
                    paths.add(path);
                }
            }
            sections.set(o.sectionIndex(), new Protocol.PSection(s.importAware(), s.parserName(), paths, s.imports(),
                    s.sourceInformation()));
            elements.addAll(o.elementIndex(), opaque);
        }
        elements.add(new Protocol.PSectionIndex(index.pkg(), index.name(), sections));
        return new Parsed(new Protocol.PureModelContextData(elements, parsed.model().serializer(),
                parsed.model().origin()), parsed.unclaimed());
    }
}
