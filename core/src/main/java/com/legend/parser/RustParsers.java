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
