// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

package com.legend.frontoracle;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertTrue;

import com.legend.ffi.FrontReader;
import com.legend.json.Json;
import com.legend.parser.Dialect;
import com.legend.parser.PmcdParser;
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
 * THE READER IS LOSSLESS (Rust port phase 2, step 3: docs/rust-port/PHASE2.md §3): every answer the Java parser gives
 * the front oracle's questions, written in the codec, read back by the generated reader ({@code FrontReader}, the
 * shim's) and written again, is the same text -- the codec writes every component, spans included, so the records
 * read back are the records written. Held before any door switches to Rust, so the shim's reader is never the
 * question.
 */
class FrontReaderRoundTripTest {

    @Test
    void everyJavaAnswerReadsBackToItself() throws Exception {
        RecordCodec codec = new RecordCodec();
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
                    FrontOracle.Answer answer = FrontOracle.onBigStack(
                            () -> FrontOracle.ask(new FrontOracle.Question(door, dialect, text), codec));
                    if (answer.error() != null) {
                        continue;
                    }
                    String json = answer.records();
                    // the overlay seam's record never crosses from Rust (Java's overlay grammar mints it, invariant
                    // 12), so the reader builds none: the one answer holding one (the toy grammar's) is not its
                    if (json.contains("\"protocol.Protocol$POpaqueElement\"")) {
                        continue;
                    }
                    answers++;
                    Object back;
                    Type declared;
                    switch (door) {
                        case "spec" -> {
                            back = FrontReader.spec(json);
                            declared = ValueSpecification.class;
                        }
                        case "lambda" -> {
                            back = FrontReader.lambda(json);
                            declared = LambdaFunction.class;
                        }
                        case "block" -> {
                            back = FrontReader.block(json);
                            declared = List.class;
                        }
                        case "pmcd" -> {
                            back = FrontReader.pmcd(json);
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
}
