// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

package com.legend.frontoracle;

import com.legend.json.Json;
import com.legend.parser.Dialect;
import com.legend.parser.ParseException;
import com.legend.parser.PmcdParser;
import com.legend.parser.SpecParser;
import com.legend.protocol.spec.LambdaFunction;
import com.legend.protocol.spec.ValueSpecification;
import com.legend.testing.Runfile;
import java.io.BufferedReader;
import java.lang.reflect.Type;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.security.MessageDigest;
import java.util.ArrayList;
import java.util.HexFormat;
import java.util.List;

/**
 * THE SHIM END TO END, AGAINST JAVA'S FROZEN ANSWERS (Rust port phase 2: docs/rust-port/PHASE2.md §3, the
 * differential; a manual tool, {@code bazel run //core:front_shim}): every question of the front oracle's set asked
 * through the parser's switched doors -- the Rust library, the crossing, the generated reader, and the model door's
 * overlay grammars called and spliced in Java ({@link PmcdParser#parse}) -- and its answer's canonical form
 * ({@link #canonical}) hashed, then compared, line for line, with the Java parser's answer frozen before it was
 * deleted ({@code rust/oracles/front/answers.tsv.zst}, MANIFEST.tsv: {@code door, dialect, the form's sha, A or E}
 * per ask, in the questions file's order). The Rust differentials hold the library's own answers to the same hashes;
 * this holds what the shim's callers receive. The form is a Java string hashed as its UTF-16 units (little-endian),
 * so a lone surrogate in an error's message hashes as itself.
 *
 * <p>Until 2026-11-06 the same class wrote Java's answers ({@code //core:front_answers}, the file frozen since), Java's
 * form of a named question ({@code //core:front_show}) and the parity probes both ways; those modes went with the Java
 * parser (step 6b). {@code --engine} hands over to {@link FrontEngineAnswers}, the engine-JSON doors' pass.
 *
 * <pre>bazel run //core:front_shim [-- --engine]   (the question set and the frozen file from the runfiles)</pre>
 */
public final class FrontAnswers {

    private FrontAnswers() {
    }

    /** A question: the door, the dialect, the text. */
    record Question(String door, Dialect dialect, String text) {
    }

    /** A door's answer: its records in the codec, or the error it raised. */
    record Answer(String records, Throwable error) {
    }

    public static void main(String[] args) throws Exception {
        boolean engine = List.of(args).contains("--engine");
        Path questions = Runfile.property("front.questions");
        Path frozen = Runfile.property(engine ? "front.engine" : "front.answers");
        // the shim's reader and the codec recurse through deep records: the whole pass on one deep-stack thread,
        // which keeps one worker of the shim's for every call
        long[] differ = new long[1];
        Exception[] failed = new Exception[1];
        Thread t = new Thread(null, () -> {
            try {
                differ[0] = engine ? FrontEngineAnswers.compare(questions, frozen) : compare(questions, frozen);
            } catch (Exception e) {
                failed[0] = e;
            }
        }, "front-shim", 1L << 30);
        t.start();
        t.join();
        if (failed[0] != null) {
            throw failed[0];
        }
        if (differ[0] > 0) {
            throw new IllegalStateException(differ[0] + " answers differ from Java's frozen ones");
        }
    }

    /** Every ask of {@code questions} through the switched doors, its form's hash compared with the frozen line
     *  of {@code frozen} in lockstep; how many differ. Prints the counts and the first differences. */
    private static long compare(Path questions, Path frozen) throws Exception {
        MessageDigest md = MessageDigest.getInstance("SHA-256");
        RecordCodec codec = new RecordCodec();
        long asks = 0;
        List<String> differ = new ArrayList<>();
        try (BufferedReader in = Files.newBufferedReader(questions, StandardCharsets.UTF_8);
                BufferedReader java = Files.newBufferedReader(frozen, StandardCharsets.UTF_8)) {
            for (String line = in.readLine(); line != null; line = in.readLine()) {
                Json.Obj o = (Json.Obj) Json.parse(line);
                String sha = ((Json.Str) o.get("sha")).value();
                String text = ((Json.Str) o.get("text")).value();
                for (Json.Node a : ((Json.Arr) o.get("asks")).items()) {
                    List<Json.Node> pair = ((Json.Arr) a).items();
                    String door = ((Json.Str) pair.get(0)).value();
                    Dialect dialect = Dialect.valueOf(((Json.Str) pair.get(1)).value());
                    Answer answer = ask(new Question(door, dialect, text), codec);
                    String hash = HexFormat.of().formatHex(md.digest(canonical(answer)
                            .getBytes(StandardCharsets.UTF_16LE)));
                    String mine = door + "\t" + dialect.name() + "\t" + hash + "\t"
                            + (answer.error() == null ? "A" : "E");
                    String theirs = java.readLine();
                    if (theirs == null) {
                        throw new IllegalStateException("the frozen file ends before the question set's asks: "
                                + frozen);
                    }
                    if (!theirs.startsWith(door + "\t" + dialect.name() + "\t")) {
                        throw new IllegalStateException("the frozen file is out of step with the questions at "
                                + sha + " (" + door + " " + dialect + "): " + theirs);
                    }
                    asks++;
                    if (!mine.equals(theirs)) {
                        differ.add(door + " " + dialect.name() + " " + sha);
                    }
                }
            }
            if (java.readLine() != null) {
                throw new IllegalStateException("the frozen file holds more lines than the question set's asks: "
                        + frozen);
            }
        }
        System.out.printf("[front-shim] the four doors: %d asks, %d identical to Java's frozen answers, %d differ%n",
                asks, asks - differ.size(), differ.size());
        differ.stream().limit(20).forEach(d -> System.out.println("[front-shim]   differs: " + d));
        return differ.size();
    }

    /** {@code q} asked through the shim's doors (Rust's answer read back into records; the model door's overlay
     *  grammars called and spliced in Java): the records in the codec, or the error raised. */
    static Answer ask(Question q, RecordCodec codec) {
        Object value;
        Type declared;
        try {
            switch (q.door()) {
                case "pmcd" -> {
                    value = PmcdParser.parse(q.text(), q.dialect());
                    declared = PmcdParser.Parsed.class;
                }
                case "spec" -> {
                    value = SpecParser.parse(q.text(), q.dialect());
                    declared = ValueSpecification.class;
                }
                case "lambda" -> {
                    value = SpecParser.parseLambda(q.text());
                    declared = LambdaFunction.class;
                }
                case "block" -> {
                    value = SpecParser.parseCodeBlock(q.text(), q.dialect());
                    declared = List.class;
                }
                default -> throw new IllegalArgumentException("no door " + q.door());
            }
        } catch (RuntimeException e) {
            return new Answer(null, e);
        }
        return new Answer(codec.write(value, declared), null);
    }

    /** An answer as text: {@code A} and the codec's JSON; or {@code E}, the error's class, its line and column (a
     *  {@code ParseException}'s; empty for another class) and its message ({@code null} when it has none), tab
     *  separated -- the form the frozen file's hashes are of. */
    static String canonical(Answer a) {
        if (a.error() == null) {
            return "A\t" + a.records();
        }
        Throwable e = a.error();
        String position = e instanceof ParseException pe ? pe.line() + "\t" + pe.column() : "\t";
        return "E\t" + e.getClass().getName() + "\t" + position + "\t"
                + (e.getMessage() == null ? "null" : e.getMessage());
    }
}
