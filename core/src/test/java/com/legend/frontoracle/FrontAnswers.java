// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

package com.legend.frontoracle;

import com.legend.json.Json;
import com.legend.parser.Dialect;
import com.legend.parser.ParseException;
import java.io.BufferedReader;
import java.io.BufferedWriter;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.security.MessageDigest;
import java.util.HexFormat;
import java.util.List;

/**
 * JAVA'S ANSWERS, HASHED (Rust port phase 2, steps 3 and 4: docs/rust-port/PHASE2.md §3, the differential): every
 * question of the front oracle's set asked of the Java parser's door ({@link FrontOracle#ask}) and its answer in one
 * canonical form ({@link #canonical}) hashed -- what lite-front's doors are held to, question by question
 * ({@code //rust/lite-front:front_parser_differential_test}), live while the Java parser exists. The form is a Java
 * string hashed as its UTF-16 units (little-endian), so a lone surrogate in an error's message hashes as itself.
 *
 * <pre>a java_run: FrontAnswers QUESTIONS_JSONL OUT_TSV  (door, dialect, sha, the form's sha, A or E)</pre>
 *
 * <p>With {@code --shim} first ({@code //core:front_shim}), the same questions asked through the shim's doors
 * ({@link FrontOracle#askShim}: Rust's answers, read back into records, the model door's overlays called and spliced in
 * Java), hashed in the same form: the shim end to end, compared line by line with Java's file.
 */
public final class FrontAnswers {

    private FrontAnswers() {
    }

    public static void main(String[] args) throws Exception {
        if (args.length == 3 && args[0].equals("--shim")) {
            // the shim's reader and the codec recurse through deep records: the whole pass on one deep-stack thread,
            // which keeps one worker of the shim's for every call
            Exception[] failed = new Exception[1];
            Thread t = new Thread(null, () -> {
                try {
                    answerAll(Path.of(args[1]), Path.of(args[2]), true);
                } catch (Exception e) {
                    failed[0] = e;
                }
            }, "front-shim", 1L << 30);
            t.start();
            t.join();
            if (failed[0] != null) {
                throw failed[0];
            }
            return;
        }
        if (args.length > 2) {
            show(Path.of(args[0]), Path.of(args[1]), List.of(args).subList(2, args.length));
            return;
        }
        answerAll(Path.of(args[0]), Path.of(args[1]), false);
    }

    /** Every question of {@code questions} answered -- by Java's doors, or with {@code shim} the shim's -- and its
     *  answer's form hashed, a line of {@code answers} each. */
    private static void answerAll(Path questions, Path answers, boolean shim) throws Exception {
        MessageDigest md = MessageDigest.getInstance("SHA-256");
        RecordCodec codec = new RecordCodec();
        try (BufferedReader in = Files.newBufferedReader(questions, StandardCharsets.UTF_8);
                BufferedWriter out = Files.newBufferedWriter(answers, StandardCharsets.UTF_8)) {
            for (String line = in.readLine(); line != null; line = in.readLine()) {
                Json.Obj o = (Json.Obj) Json.parse(line);
                String sha = ((Json.Str) o.get("sha")).value();
                String text = ((Json.Str) o.get("text")).value();
                for (Json.Node a : ((Json.Arr) o.get("asks")).items()) {
                    List<Json.Node> pair = ((Json.Arr) a).items();
                    String door = ((Json.Str) pair.get(0)).value();
                    Dialect dialect = Dialect.valueOf(((Json.Str) pair.get(1)).value());
                    FrontOracle.Question q = new FrontOracle.Question(door, dialect, text);
                    FrontOracle.Answer answer = shim ? FrontOracle.askShim(q, codec)
                            : FrontOracle.onBigStack(() -> FrontOracle.ask(q, codec));
                    String form = canonical(answer);
                    String hash = HexFormat.of().formatHex(md.digest(form.getBytes(StandardCharsets.UTF_16LE)));
                    out.append(door).append('\t').append(dialect.name()).append('\t').append(sha).append('\t')
                            .append(hash).append('\t').append(answer.error() == null ? "A" : "E").append('\n');
                }
            }
        }
    }

    /** The debug mode ({@code //core:front_show}): Java's form for each named question ({@code door:dialect:sha}),
     *  written to {@code DIR/<door>.<dialect>.<sha>.java.txt} -- beside the Rust form the differential writes. */
    private static void show(Path questions, Path dir, List<String> args) throws Exception {
        java.util.Set<String> wanted = new java.util.HashSet<>();
        for (String a : args) {
            if (a.chars().filter(ch -> ch == ':').count() == 2) {
                wanted.add(a);
            }
        }
        Files.createDirectories(dir);
        RecordCodec codec = new RecordCodec();
        try (BufferedReader in = Files.newBufferedReader(questions, StandardCharsets.UTF_8)) {
            for (String line = in.readLine(); line != null; line = in.readLine()) {
                Json.Obj o = (Json.Obj) Json.parse(line);
                String sha = ((Json.Str) o.get("sha")).value();
                String text = ((Json.Str) o.get("text")).value();
                for (Json.Node a : ((Json.Arr) o.get("asks")).items()) {
                    List<Json.Node> pair = ((Json.Arr) a).items();
                    String door = ((Json.Str) pair.get(0)).value();
                    String dialect = ((Json.Str) pair.get(1)).value();
                    if (wanted.contains(door + ":" + dialect + ":" + sha)) {
                        FrontOracle.Answer answer = FrontOracle.onBigStack(() -> FrontOracle.ask(
                                new FrontOracle.Question(door, Dialect.valueOf(dialect), text), codec));
                        Files.writeString(dir.resolve(door + "." + dialect + "." + sha + ".java.txt"),
                                canonical(answer), StandardCharsets.UTF_8);
                        Files.writeString(dir.resolve(door + "." + dialect + "." + sha + ".text.txt"), text,
                                StandardCharsets.UTF_8);
                    }
                }
            }
        }
    }

    /** An answer as text: {@code A} and the codec's JSON; or {@code E}, the error's class, its line and column (a
     *  {@code ParseException}'s; empty for another class) and its message ({@code null} when it has none), tab
     *  separated. The engine JSON is not part of it (step 5's). */
    static String canonical(FrontOracle.Answer a) {
        if (a.error() == null) {
            return "A\t" + a.records();
        }
        Throwable e = a.error();
        String position = e instanceof ParseException pe ? pe.line() + "\t" + pe.column() : "\t";
        return "E\t" + e.getClass().getName() + "\t" + position + "\t"
                + (e.getMessage() == null ? "null" : e.getMessage());
    }
}
