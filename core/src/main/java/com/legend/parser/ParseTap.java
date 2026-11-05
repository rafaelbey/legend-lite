// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

package com.legend.parser;

import java.io.IOException;
import java.io.UncheckedIOException;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.nio.file.StandardOpenOption;

/**
 * THE PARSE TAP (Rust port phase 2, step 1: docs/rust-port/PHASE2.md §4): the parser's text doors record what they
 * are asked -- the door, the dialect, the text -- so the Java parser's every answer can be reproduced and frozen as the
 * oracle the Rust parser is held to. Off unless {@code LEGEND_PARSE_TAP} is set; then each process appends one JSON
 * line per call to {@code parse-tap-<pid>.jsonl} under the test's undeclared outputs (or under the directory the
 * variable names, outside a test). It records the question, never the answer: the oracle's generator re-asks it.
 * Deleted with the Java parser (step 6).
 *
 * <p>No state: the file is opened, appended and closed per call, under this class's lock, and duplicates are left
 * to the merge. Every surrogate is written as a {@code \\uXXXX} escape, so a lone one (an {@code Escapes} product)
 * survives the UTF-8 file.
 */
final class ParseTap {

    private static final boolean ON = System.getenv("LEGEND_PARSE_TAP") != null;

    private ParseTap() {
    }

    /** Record one call of a text door: {@code door} names it, {@code dialect} is its level. */
    static void record(String door, Dialect dialect, String text) {
        if (!ON) {
            return;
        }
        String dir = System.getenv("TEST_UNDECLARED_OUTPUTS_DIR");
        if (dir == null) {
            dir = System.getenv("LEGEND_PARSE_TAP");
        }
        StringBuilder line = new StringBuilder(text.length() + 64);
        line.append("{\"door\":");
        quote(line, door);
        line.append(",\"dialect\":");
        quote(line, dialect.name());
        line.append(",\"text\":");
        quote(line, text);
        line.append("}\n");
        Path file = Path.of(dir, "parse-tap-" + ProcessHandle.current().pid() + ".jsonl");
        synchronized (ParseTap.class) {
            try {
                Files.writeString(file, line, StandardCharsets.UTF_8,
                        StandardOpenOption.CREATE, StandardOpenOption.APPEND);
            } catch (IOException e) {
                throw new UncheckedIOException("the parse tap could not write " + file, e);
            }
        }
    }

    private static void quote(StringBuilder out, String s) {
        out.append('"');
        for (int i = 0; i < s.length(); i++) {
            char c = s.charAt(i);
            if (c == '"' || c == '\\') {
                out.append('\\').append(c);
            } else if (c < 0x20 || Character.isSurrogate(c)) {
                String hex = Integer.toHexString(c);
                out.append("\\u").append("0000", hex.length(), 4).append(hex);
            } else {
                out.append(c);
            }
        }
        out.append('"');
    }
}
