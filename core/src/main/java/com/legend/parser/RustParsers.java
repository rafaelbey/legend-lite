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
