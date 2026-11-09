// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

package com.legend.frontoracle;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertTrue;

import com.legend.ffi.FrontReader;
import com.legend.ffi.Lite;
import com.legend.json.Json;
import com.legend.parser.Dialect;
import com.legend.parser.PmcdParser;
import com.legend.parser.RustParsers;
import com.legend.parser.SectionGrammarRegistry;
import com.legend.protocol.spec.LambdaFunction;
import com.legend.protocol.spec.ValueSpecification;
import com.legend.testing.Runfile;
import java.io.BufferedReader;
import java.lang.reflect.Type;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.util.ArrayList;
import java.util.List;
import org.junit.jupiter.api.Test;

/**
 * THE READER IS LOSSLESS (Rust port phase 2, step 3: docs/rust-port/PHASE2.md §3): every answer the Rust library gives
 * the front oracle's questions, in the codec, read back by the generated reader ({@code FrontReader}, the shim's) and
 * written again by the codec ({@link RecordCodec}), is the same text -- the codec writes every component, spans
 * included, so the records read back are the records the library wrote. Until 2026-11-06 the answers read back were
 * the Java parser's; since step 6b deleted it they are the library's own, asked through the shim's downcalls
 * ({@code lite_front}, and {@code lite_front_model} with the registered overlay grammars, as the shim asks), and
 * every one of them is held to the Java parser's frozen answer by //rust/lite-front:front_parser_differential_test.
 */
class FrontReaderRoundTripTest {

    @Test
    void everyRustAnswerReadsBackToItself() throws Exception {
        RecordCodec codec = new RecordCodec();
        String overlays = overlays();
        int answers = 0;
        List<String> differ = new ArrayList<>();
        try (BufferedReader in = Files.newBufferedReader(Runfile.property("front.questions"),
                StandardCharsets.UTF_8)) {
            for (String line = in.readLine(); line != null; line = in.readLine()) {
                Json.Obj o = (Json.Obj) Json.parse(line);
                String sha = ((Json.Str) o.get("sha")).value();
                String text = ((Json.Str) o.get("text")).value();
                for (Json.Node a : ((Json.Arr) o.get("asks")).items()) {
                    List<Json.Node> pair = ((Json.Arr) a).items();
                    String door = ((Json.Str) pair.get(0)).value();
                    Dialect dialect = Dialect.valueOf(((Json.Str) pair.get(1)).value());
                    String json = records(door, dialect, text, overlays);
                    if (json == null) {
                        continue;
                    }
                    answers++;
                    Object back;
                    Type declared;
                    switch (door) {
                        case "spec" -> {
                            back = FrontReader.spec(json, 0);
                            declared = ValueSpecification.class;
                        }
                        case "lambda" -> {
                            back = FrontReader.lambda(json, 0);
                            declared = LambdaFunction.class;
                        }
                        case "block" -> {
                            back = FrontReader.block(json, 0);
                            declared = List.class;
                        }
                        case "pmcd" -> {
                            back = FrontReader.pmcd(json, 0);
                            declared = PmcdParser.Parsed.class;
                        }
                        default -> throw new IllegalArgumentException("no door " + door);
                    }
                    if (!codec.write(back, declared).equals(json)) {
                        differ.add(door + " " + dialect + " " + sha);
                    }
                }
            }
        }
        assertTrue(answers > 90_000, "the question set's answers were read: " + answers);
        assertEquals(List.of(), differ.subList(0, Math.min(20, differ.size())),
                differ.size() + " answers read back differently (the first 20 shown)");
    }

    /** The library's answer to one question, its records in the codec as it wrote them; {@code null} for an error. */
    private static String records(String door, Dialect dialect, String text, String overlays) {
        if (door.equals("pmcd")) {
            // A\n<the overlay sections, one line>\n<records>, or E\n<sections>\n<the error>
            String answer = Lite.shared().frontModel(dialect.name(), overlays, jsonString(text));
            return answer.startsWith("A\n") ? answer.substring(answer.indexOf('\n', 2) + 1) : null;
        }
        String answer = Lite.shared().front(door, dialect.name(), jsonString(text));
        return answer.startsWith("A\n") ? answer.substring(2) : null;
    }

    /** The registered overlay grammars as the library takes them, {@code [["Toy",false],...]}, as the shim passes
     *  them. */
    private static String overlays() {
        StringBuilder b = new StringBuilder("[");
        for (RustParsers.Overlay o : SectionGrammarRegistry.overlays()) {
            if (b.length() > 1) {
                b.append(',');
            }
            b.append('[').append(jsonString(o.name())).append(',').append(o.lexable()).append(']');
        }
        return b.append(']').toString();
    }

    /** {@code text} as a JSON string literal, as the shim writes it: {@code "} and {@code \}, the controls and every
     *  surrogate escaped, so a lone one survives the UTF-8 C string. */
    private static String jsonString(String text) {
        StringBuilder out = new StringBuilder(text.length() + 16).append('"');
        for (int i = 0; i < text.length(); i++) {
            char c = text.charAt(i);
            if (c == '"' || c == '\\') {
                out.append('\\').append(c);
            } else if (c < 0x20 || Character.isSurrogate(c)) {
                String hex = Integer.toHexString(c);
                out.append("\\u").append("0000", hex.length(), 4).append(hex);
            } else {
                out.append(c);
            }
        }
        return out.append('"').toString();
    }
}
