// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

package com.legend.frontoracle;

import com.legend.testing.Runfile;
import java.io.IOException;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.security.MessageDigest;
import java.util.HashMap;
import java.util.HexFormat;
import java.util.Map;

/**
 * The front oracle's deliberate differences (2026-11-09; docs/rust-port/PHASE2.md section 6, ruling 4):
 * {@code rust/oracles/front/deliberate-differences.tsv}, the questions where the Rust doors answer differently from the
 * frozen Java answers on purpose, each with its new answer. {@code //core:front_shim} holds them as the Rust
 * differentials do: a listed question really differs from Java's frozen answer and answers exactly its new answer.
 */
final class Deliberate {

    /** A listed question's new answer. */
    record Expected(String cls, String line, String column, String messageSha) {

        /** Why {@code form} ({@code E\t<class>\t<line>\t<column>\t<message>}) is not this answer, or null when it is. */
        String wrong(String form, MessageDigest md) {
            String[] f = form.split("\t", 5);
            if (f.length == 5 && f[0].equals("E") && f[1].equals(cls) && f[2].equals(line) && f[3].equals(column)
                    && HexFormat.of().formatHex(md.digest(f[4].getBytes(StandardCharsets.UTF_16LE))).equals(messageSha)) {
                return null;
            }
            return "not its listed answer: " + form.substring(0, Math.min(160, form.length()));
        }
    }

    private final Map<String, Expected> expected = new HashMap<>();

    private Deliberate() {
    }

    static Deliberate load() throws IOException {
        Deliberate d = new Deliberate();
        for (String line : Files.readAllLines(Runfile.property("front.deliberate"), StandardCharsets.UTF_8)) {
            if (line.isEmpty() || line.startsWith("#")) {
                continue;
            }
            String[] f = line.split("\t", -1);
            if (f.length != 8 || d.expected.putIfAbsent(f[0] + "\t" + f[1] + "\t" + f[2],
                    new Expected(f[4], f[5], f[6], f[7])) != null) {
                throw new IllegalStateException("a malformed or repeated line: " + line);
            }
        }
        return d;
    }

    /** The new answer the list names for a question, or null. */
    Expected expected(String door, String dialect, String sha) {
        return expected.get(door + "\t" + dialect + "\t" + sha);
    }
}
