// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

package com.legend.parser;

import com.legend.protocol.spec.LambdaFunction;
import com.legend.protocol.spec.ValueSpecification;

import java.util.List;
import java.util.Objects;

/**
 * THE EXPRESSION DOORS: Pure value specifications -- a query, a lambda, a code block -- read into the sealed
 * {@link ValueSpecification} records, at the {@link Dialect} the caller names (there is no default). The Rust
 * library answers every door (docs/rust-port/PHASE2.md §4 step 3, the switch: {@code rust/lite-front}'s
 * {@code spec_parser.rs} over its lexer, through {@link RustParsers}), its error rethrown here as the class Java's
 * parser raised -- a {@link ParseException} with its line and column. The Java parser it ports was deleted on
 * 2026-11-06 (step 6b); its answers to the front oracle's every question are frozen in
 * {@code rust/oracles/front/}, which {@code //rust/lite-front:front_parser_differential_test} holds the library to.
 *
 * <p>Each door consumes its whole text: anything left after the one expression, or after the code block's last
 * statement, is refused, never truncated.
 */
public final class SpecParser {

    private SpecParser() {
    }

    /** {@code source} as a single expression (one program line: an expression or a {@code let}) at
     *  {@code dialect}. */
    public static ValueSpecification parse(String source, Dialect dialect) {
        return RustParsers.provider().spec(Objects.requireNonNull(source, "source"), dialect);
    }

    /** A lambda's text as upstream {@code grammarToJson/lambda} reads it -- legend-engine's
     *  grammar, so its level: text that is not a lambda becomes a parameterless one spanning
     *  all its tokens, as legend-engine wraps it. */
    public static LambdaFunction parseLambda(String source) {
        return RustParsers.provider().lambda(Objects.requireNonNull(source, "source"));
    }

    /**
     * Text to JSON for a lambda (the conversion legend-engine's {@code grammarToJson/lambda} makes, its
     * {@code returnSourceInformation} the second argument): {@link #parseLambda}, emitted, without any source
     * information when it is off -- every span, the named ones included, as the engine's parser then records none.
     *
     * <p>The Rust library's parser and emitter in one call (docs/rust-port/PHASE2.md §4 step 5), held to the Java
     * parser's and emitter's frozen answer on every lambda of the front oracle
     * ({@code //rust/lite-front:front_engine_differential_test}).
     */
    public static String lambdaJson(String source, boolean sourceInformation) {
        return RustParsers.provider().lambdaJson(Objects.requireNonNull(source, "source"), sourceInformation);
    }

    /**
     * A multi-statement code block: a {@code ;}-separated sequence of statements, each one program line (an
     * expression or a {@code let}). The block's value is its last statement's; the parser returns the sequence
     * and decides nothing about it.
     */
    public static List<ValueSpecification> parseCodeBlock(String source, Dialect dialect) {
        return RustParsers.provider().block(source, dialect);
    }
}
