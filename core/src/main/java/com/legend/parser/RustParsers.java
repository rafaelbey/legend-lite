// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

package com.legend.parser;

import com.legend.protocol.spec.LambdaFunction;
import com.legend.protocol.spec.ValueSpecification;
import java.util.List;
import java.util.ServiceLoader;

/**
 * The parser's doors the Rust library answers (docs/rust-port/PHASE2.md §3), as this package sees them: the records,
 * or the error Java's parser raises, rethrown as its class. The parser may depend on no ffi code (ArchitectureTest,
 * invariant 7c), so the implementation -- {@code com.legend.ffi}'s, over the C library -- is found through the SPI
 * seam, {@link ServiceLoader}, as the dialects' {@code RustDialects} is: whatever runs core carries
 * //core:rust_parser on its runtime classpath.
 */
public interface RustParsers {

    /** {@link SpecParser#parse(String, Dialect)}'s answer. */
    ValueSpecification spec(String text, Dialect dialect);

    /** {@link SpecParser#parseLambda(String)}'s answer. */
    LambdaFunction lambda(String text);

    /** {@link SpecParser#parseCodeBlock(String, Dialect)}'s answer. */
    List<ValueSpecification> block(String text, Dialect dialect);

    /**
     * {@link PmcdParser#parse(String, Dialect)}'s answer before its overlay grammars are called (docs/rust-port/
     * PHASE2.md, "The model door's overlays"): Rust's records, or the error to raise, and each overlay section the walk
     * met, whose grammar {@link PmcdParser} calls and whose elements it splices -- Rust never makes an opaque element.
     */
    ModelAnswer model(String text, Dialect dialect, List<Overlay> overlays);

    /**
     * {@link PmcdParser#parseDocument(String, boolean)}'s answer (docs/rust-port/PHASE2.md §4 step 5): the model text
     * parsed at the engine's dialect and emitted as legend-engine's JSON by Rust, without its source information when
     * {@code sourceInformation} is off; the parser's error or the emitter's refusal thrown as its class. {@code null}
     * when the walk met a section of a registered overlay grammar: the overlay path, which {@link PmcdParser} answers
     * itself (Rust never makes an opaque element).
     */
    @com.legend.base.Nullable String document(String text, boolean sourceInformation, List<Overlay> overlays);

    /** {@link SpecParser#lambdaJson(String, boolean)}'s answer: the lambda parsed and emitted by Rust, or the error. */
    String lambdaJson(String text, boolean sourceInformation);

    // THE PARITY HARNESS'S PROBES (docs/rust-port/PHASE2.md §4 step 6a): what //parser-equivalence's gate tests asked
    // of the Java lexer's and parser's internals, answered by the Rust library (rust/lite-front/src/probes.rs), so the
    // tests judge the Rust parser through them once the Java one is deleted. No product code calls them.

    /** The lexer's {@code ###Section} headers in {@code text}, in order: each one's name and content offset. */
    List<SectionHeader> sectionHeaders(String text);

    /**
     * The strict element sites: every top-level {@code Class}, {@code Enum}, {@code Profile}, {@code Association} and
     * {@code function} -- the kinds in that order, each kind's sites in token order -- parsed where it stands in the
     * whole text's tokens, at {@code dialect}, by its kind's reader; how many were parsed, or the first one's error
     * thrown as its class ({@link ParseException} with its line and column).
     */
    int elementSites(String text, Dialect dialect);

    /**
     * Every {@code Database} at the token stream's start or after a {@code )}, a closing brace or a {@code ;}, in order,
     * parsed where it stands at {@code dialect} and emitted as legend-engine's element JSON: each one's qualified name
     * and JSON; or the first error, thrown as its class.
     */
    List<DatabaseSite> databaseSites(String text, Dialect dialect);

    /** The sections the model parser reads with a grammar of its own (the registry's built-in half, not overlays). */
    List<String> knownSections();

    /**
     * THE PRELUDE GENERATOR'S PROBE (step 6b: spec/src/gen's {@code PreludeGenerator}, a build action, which cut each
     * declaration's verbatim text with the Java lexer and cursor before they were deleted): every declaration at brace
     * depth 0 and a declaration's position -- the stream's first token, or right after a {@code ;} or a closing brace,
     * a documentation literal before its head starting it -- whose head is {@code Class}, {@code Enum},
     * {@code Profile}, {@code function} or {@code native function}, in token order: its kind and its extent, from its
     * first token to the end of the last token its kind's reader consumed at {@code dialect} (a native's {@code ;}).
     * The first reader's error is thrown as its class.
     */
    List<DeclarationExtent> declarationExtents(String text, Dialect dialect);

    /** A declaration {@link #declarationExtents} found: its head ({@code Class}, {@code Enum}, {@code Profile},
     *  {@code function} or {@code native function}) and its text, {@code text.substring(start, end)}. */
    record DeclarationExtent(String kind, int start, int end) {
    }

    /** A {@code ###Section} header: its name, and the offset (a {@code char} index) its content starts at. */
    record SectionHeader(String name, int contentStartOffset) {
    }

    /** A {@code Database} site: the element's qualified name and its legend-engine JSON. */
    record DatabaseSite(String qualifiedName, String json) {
    }

    /** An overlay grammar the registry holds (its ServiceLoader half): its section's name and its {@code lexable()}. */
    record Overlay(String name, boolean lexable) {
    }

    /**
     * An overlay section the model door's walk met: its grammar's text {@code text[startOffset, endOffset)} and the
     * line it starts on; its elements go before the records' {@code model.elements[elementIndex]} (splicing the
     * sections last-first keeps each index right), their paths, each once, into the section index's
     * {@code sections[sectionIndex]}, which Rust leaves empty.
     */
    record OverlaySection(String name, int startOffset, int endOffset, int startLine, int elementIndex,
            int sectionIndex) {
    }

    /**
     * The model door's answer: the records, or the error the parser raises (not thrown: an overlay grammar called
     * first may throw its own), and the overlay sections the walk met -- all of them with the records, those before
     * the error with it.
     */
    record ModelAnswer(PmcdParser.@com.legend.base.Nullable Parsed parsed,
            @com.legend.base.Nullable RuntimeException raised, List<OverlaySection> overlays) {
    }

    /** The provider, found once. */
    static RustParsers provider() {
        return Found.PROVIDER;
    }

    /** The provider's holder: loaded on first use. */
    final class Found {
        private Found() {
        }

        static final RustParsers PROVIDER = ServiceLoader.load(RustParsers.class).findFirst()
                .orElseThrow(() -> new IllegalStateException("no Rust parser provider: //core:rust_parser is not on"
                        + " the runtime classpath (its META-INF/services names com.legend.ffi.RustParserProvider)"));
    }
}
