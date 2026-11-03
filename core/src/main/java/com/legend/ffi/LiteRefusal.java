// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

package com.legend.ffi;

import com.legend.base.Nullable;

/**
 * A refusal the Rust library answered ({@code ERR\n<kind>\n<phase>\n<line:col>\n<message>}, rust/lite-ffi): its kind
 * is Rust's own ({@code PARSER}, {@code COMPILATION}, {@code INVALID_ARGUMENT}, {@code INTERNAL}, ...), its phase and
 * position absent where they do not apply. The phases that move core's entry points onto the library rethrow it as the
 * exception their callers expect (docs/rust-port/ROADMAP.md, "The error contract").
 */
public final class LiteRefusal extends RuntimeException {

    private final String kind;
    private final @Nullable String phase;
    private final int line;
    private final int column;

    public LiteRefusal(String kind, @Nullable String phase, int line, int column, String message) {
        super(message);
        this.kind = kind;
        this.phase = phase;
        this.line = line;
        this.column = column;
    }

    public String kind() {
        return kind;
    }

    public @Nullable String phase() {
        return phase;
    }

    /** The 1-based line, or 0 when the refusal has no position. */
    public int line() {
        return line;
    }

    /** The 1-based column, or 0 when the refusal has no position. */
    public int column() {
        return column;
    }
}
