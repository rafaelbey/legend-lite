// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

package com.legend.frontoracle;

import com.legend.json.Json;
import com.legend.lexer.Lexer;
import com.legend.lexer.TokenStream;
import java.io.BufferedReader;
import java.io.BufferedWriter;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.security.MessageDigest;
import java.util.HexFormat;

/**
 * JAVA'S TOKENS, HASHED (Rust port phase 2, step 2: docs/rust-port/PHASE2.md §4): every text of the front oracle's
 * question set lexed by the Java lexer, its token stream written in one canonical form ({@link #canonical}) and that
 * form's SHA-256 recorded -- what lite-front's lexer is held to, question by question
 * ({@code //rust/lite-front:front_lexer_differential_test}). The form names every token's type, offsets and span
 * positions, every section header with its residue, and every opaque section the lexer skipped; it is a Java string
 * hashed as its UTF-16 units (little-endian), so a lone surrogate in a residue hashes as itself.
 *
 * <pre>a java_run: FrontTokens QUESTIONS_JSONL OUT_TSV  (one line per text: sha, the form's sha, the token count)</pre>
 */
public final class FrontTokens {

    private FrontTokens() {
    }

    public static void main(String[] args) throws Exception {
        MessageDigest md = MessageDigest.getInstance("SHA-256");
        try (BufferedReader in = Files.newBufferedReader(Path.of(args[0]), StandardCharsets.UTF_8);
                BufferedWriter out = Files.newBufferedWriter(Path.of(args[1]), StandardCharsets.UTF_8)) {
            for (String line = in.readLine(); line != null; line = in.readLine()) {
                Json.Obj o = (Json.Obj) Json.parse(line);
                String sha = ((Json.Str) o.get("sha")).value();
                TokenStream ts = Lexer.tokenize(((Json.Str) o.get("text")).value());
                String form = canonical(ts);
                String hash = HexFormat.of().formatHex(md.digest(form.getBytes(StandardCharsets.UTF_16LE)));
                out.append(sha).append('\t').append(hash).append('\t').append(Integer.toString(ts.count()))
                        .append('\n');
            }
        }
    }

    /** One token stream as text: a {@code N} line with the count, a {@code T} line per token (type, start, end, start
     *  line, start column, end line, end column), an {@code H} line per section header (name, name offset, content
     *  offset, the residue quoted or {@code -}, its line and column), an {@code S} line per skipped section. */
    static String canonical(TokenStream ts) {
        StringBuilder b = new StringBuilder();
        b.append("N ").append(ts.count()).append('\n');
        for (int i = 0; i < ts.count(); i++) {
            b.append("T ").append(ts.type(i).name()).append(' ').append(ts.start(i)).append(' ').append(ts.end(i))
                    .append(' ').append(ts.startLine(i)).append(' ').append(ts.startColumn(i))
                    .append(' ').append(ts.endLine(i)).append(' ').append(ts.endColumn(i)).append('\n');
        }
        for (TokenStream.SectionHeader h : ts.sectionHeaders()) {
            b.append("H ").append(h.name()).append(' ').append(h.nameOffset()).append(' ')
                    .append(h.contentStartOffset()).append(' ')
                    .append(h.residueToken() == null ? "-" : "'" + h.residueToken() + "'")
                    .append(' ').append(h.residueLine()).append(' ').append(h.residueCol()).append('\n');
        }
        for (TokenStream.SkippedSection s : ts.skippedSections()) {
            b.append("S ").append(s.name()).append(' ').append(s.nameOffset()).append(' ').append(s.startOffset())
                    .append(' ').append(s.endOffset()).append('\n');
        }
        return b.toString();
    }
}
