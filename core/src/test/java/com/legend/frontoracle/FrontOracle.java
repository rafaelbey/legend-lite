// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

package com.legend.frontoracle;

import com.legend.json.Json;
import com.legend.parser.Dialect;
import com.legend.parser.ParseException;
import com.legend.parser.PmcdParser;
import com.legend.parser.SpecParser;
import com.legend.protocol.ProtocolEmitter;
import com.legend.protocol.spec.LambdaFunction;
import com.legend.protocol.spec.ValueSpecification;
import java.io.BufferedWriter;
import java.io.IOException;
import java.lang.reflect.Type;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.security.MessageDigest;
import java.security.NoSuchAlgorithmException;
import java.util.ArrayList;
import java.util.HexFormat;
import java.util.List;
import java.util.Map;
import java.util.TreeMap;
import java.util.stream.Stream;

/**
 * THE FRONT ORACLE'S GENERATOR (Rust port phase 2, step 1: docs/rust-port/PHASE2.md §4): every question the parse
 * tap recorded ({@code com.legend.parser.ParseTap}: the door, the dialect, the text), deduplicated, asked of the Java
 * parser again, and its answer written in the lossless record codec ({@link RecordCodec}) -- or the error it raised,
 * by class, message, line and column, which the shim must rethrow exactly. Engine-dialect model parses and lambdas
 * carry the engine JSON the protocol emitter writes too (step 5's gate).
 *
 * <pre>bazel run //core:front_oracle -- OUT_DIR TAP_FILE_OR_DIR...</pre>
 *
 * Every model text is asked at every dialect, not only at the one its test used. Writes {@code questions.jsonl} (the
 * committed question set: each text once, by the SHA-256 of its UTF-16 units, with every door and dialect asked of
 * it), {@code answers.jsonl} (by door, dialect and SHA), {@code schema.json} (every record the doors can return) and
 * {@code report.txt} (the measurements).
 */
public final class FrontOracle {

    private FrontOracle() {
    }

    private record Question(String door, Dialect dialect, String text) {
    }

    public static void main(String[] args) throws Exception {
        if (args.length < 2) {
            throw new IllegalArgumentException("usage: front_oracle OUT_DIR TAP_FILE_OR_DIR...");
        }
        Path out = Path.of(args[0]);
        Files.createDirectories(out);
        // door, dialect, sha -> question (sorted: the files are reproducible)
        Map<String, Question> questions = new TreeMap<>();
        Map<String, String> texts = new TreeMap<>();
        long lines = 0;
        for (int i = 1; i < args.length; i++) {
            for (Path f : tapFiles(Path.of(args[i]))) {
                try (java.io.BufferedReader in = Files.newBufferedReader(f, StandardCharsets.UTF_8)) {
                    for (String line = in.readLine(); line != null; line = in.readLine()) {
                        if (line.isEmpty()) {
                            continue;
                        }
                        lines++;
                        Json.Obj o = (Json.Obj) Json.parse(line);
                        String door = ((Json.Str) o.get("door")).value();
                        Dialect dialect = Dialect.valueOf(((Json.Str) o.get("dialect")).value());
                        String text = ((Json.Str) o.get("text")).value();
                        String sha = sha(text);
                        texts.put(sha, text);
                        questions.put(door + "\t" + dialect + "\t" + sha, new Question(door, dialect, text));
                    }
                }
            }
        }

        // breadth: every model text at every dialect, not only the one its test used (the dialect decides what is
        // accepted and where an unknown section goes, so each is its own question)
        int recorded = questions.size();
        for (Question q : List.copyOf(questions.values())) {
            if (q.door().equals("pmcd")) {
                String sha = sha(q.text());
                for (Dialect d : Dialect.values()) {
                    questions.putIfAbsent("pmcd\t" + d + "\t" + sha, new Question("pmcd", d, q.text()));
                }
            }
        }
        // the committed question set: each text once, with every (door, dialect) asked of it -- an id repeated per
        // question would not compress (a SHA is 32 random bytes)
        Map<String, List<String[]>> asks = new TreeMap<>();
        for (String key : questions.keySet()) {
            String[] k = key.split("\t");
            asks.computeIfAbsent(k[2], s -> new ArrayList<>()).add(new String[] {k[0], k[1]});
        }
        try (BufferedWriter w = Files.newBufferedWriter(out.resolve("questions.jsonl"), StandardCharsets.UTF_8)) {
            for (Map.Entry<String, List<String[]>> e : asks.entrySet()) {
                StringBuilder line = new StringBuilder("{\"sha\":");
                RecordCodec.string(line, e.getKey());
                line.append(",\"asks\":[");
                for (int i = 0; i < e.getValue().size(); i++) {
                    if (i > 0) {
                        line.append(',');
                    }
                    line.append('[');
                    RecordCodec.string(line, e.getValue().get(i)[0]);
                    line.append(',');
                    RecordCodec.string(line, e.getValue().get(i)[1]);
                    line.append(']');
                }
                line.append("],\"text\":");
                RecordCodec.string(line, texts.get(e.getKey()));
                w.append(line).append("}\n");
            }
        }

        RecordCodec codec = new RecordCodec();
        Map<String, Integer> byDoor = new TreeMap<>();
        Map<String, Integer> errors = new TreeMap<>();
        long answerBytes = 0;
        try (BufferedWriter w = Files.newBufferedWriter(out.resolve("answers.jsonl"), StandardCharsets.UTF_8)) {
            for (Map.Entry<String, Question> e : questions.entrySet()) {
                Question q = e.getValue();
                byDoor.merge(q.door() + " " + q.dialect(), 1, Integer::sum);
                StringBuilder line = new StringBuilder("{\"door\":");
                RecordCodec.string(line, q.door());
                line.append(",\"dialect\":");
                RecordCodec.string(line, q.dialect().name());
                line.append(",\"sha\":");
                RecordCodec.string(line, e.getKey().substring(e.getKey().lastIndexOf('\t') + 1));
                Answer a = onBigStack(() -> ask(q, codec));
                if (a.error() != null) {
                    errors.merge(a.error().getClass().getName(), 1, Integer::sum);
                    line.append(",\"error\":{\"class\":");
                    RecordCodec.string(line, a.error().getClass().getName());
                    line.append(",\"message\":");
                    String m = a.error().getMessage();
                    if (m == null) {
                        line.append("null");
                    } else {
                        RecordCodec.string(line, m);
                    }
                    if (a.error() instanceof ParseException pe) {
                        line.append(",\"line\":").append(pe.line()).append(",\"column\":").append(pe.column());
                    }
                    line.append('}');
                } else {
                    line.append(",\"answer\":").append(a.records());
                    if (a.engine() != null) {
                        line.append(",\"engine\":");
                        RecordCodec.string(line, a.engine());
                    }
                    if (a.engineError() != null) {
                        errors.merge("engine JSON: " + a.engineError().getClass().getName(), 1, Integer::sum);
                        line.append(",\"engineError\":{\"class\":");
                        RecordCodec.string(line, a.engineError().getClass().getName());
                        line.append(",\"message\":");
                        String m = a.engineError().getMessage();
                        if (m == null) {
                            line.append("null");
                        } else {
                            RecordCodec.string(line, m);
                        }
                        line.append('}');
                    }
                }
                line.append("}\n");
                answerBytes += line.length();
                w.append(line);
            }
        }
        String schema = RecordCodec.schema(List.of(PmcdParser.Parsed.class, ValueSpecification.class,
                LambdaFunction.class));
        Files.writeString(out.resolve("schema.json"), schema, StandardCharsets.UTF_8);

        StringBuilder report = new StringBuilder();
        report.append("tap lines ").append(lines).append(", questions recorded ").append(recorded)
                .append(", with every model text at every dialect ").append(questions.size())
                .append(", texts ").append(texts.size()).append('\n');
        report.append("answers.jsonl ").append(answerBytes).append(" chars\n");
        report.append("by door and dialect ").append(byDoor).append('\n');
        report.append("errors by class ").append(errors).append('\n');
        report.append("record types reached ").append(codec.reached.size()).append('\n');
        java.util.Set<String> unreached = new java.util.TreeSet<>(
                ((Json.Obj) ((Json.Obj) Json.parse(schema)).get("records")).fields().keySet());
        unreached.removeAll(codec.reached);
        report.append("schema records no answer reached ").append(unreached).append('\n');
        report.append("records met twice in one answer, by type ").append(codec.shared).append('\n');
        report.append("map, set and optional classes met ").append(codec.containers).append('\n');
        Files.writeString(out.resolve("report.txt"), report, StandardCharsets.UTF_8);
        System.out.print(report);
    }

    private record Answer(String records, String engine, Throwable engineError, Throwable error) {
    }

    private static Answer ask(Question q, RecordCodec codec) {
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
        } catch (RuntimeException | StackOverflowError e) {
            return new Answer(null, null, null, e);
        }
        // the engine JSON, for the engine's dialect: the emitter's own answer, so a refusal of the EMITTER is its
        // own field -- the parse above succeeded (a model the emitter refuses is still a parse)
        String engine = null;
        Throwable engineError = null;
        try {
            if (q.door().equals("pmcd") && q.dialect() == Dialect.LEGEND_ENGINE) {
                engine = ProtocolEmitter.emit(((PmcdParser.Parsed) value).model());
            } else if (q.door().equals("lambda")) {
                engine = ProtocolEmitter.emitLambda((LambdaFunction) value);
            }
        } catch (RuntimeException | StackOverflowError e) {
            engineError = e;
        }
        return new Answer(codec.write(value, declared), engine, engineError, null);
    }

    /** A parse on a thread with a deep stack (TRIAGE A3): the deepest nesting is not this harness's limit. */
    private static Answer onBigStack(java.util.function.Supplier<Answer> work) throws InterruptedException {
        Answer[] result = new Answer[1];
        RuntimeException[] failed = new RuntimeException[1];
        Thread t = new Thread(null, () -> {
            try {
                result[0] = work.get();
            } catch (RuntimeException e) {
                failed[0] = e;
            }
        }, "front-oracle", 1L << 30);
        t.start();
        t.join();
        if (failed[0] != null) {
            throw failed[0];
        }
        return result[0];
    }

    private static List<Path> tapFiles(Path p) throws IOException {
        if (!Files.isDirectory(p)) {
            return List.of(p);
        }
        try (Stream<Path> s = Files.walk(p)) {
            List<Path> files = new ArrayList<>(s.filter(f -> f.getFileName().toString().startsWith("parse-tap-")
                    && f.getFileName().toString().endsWith(".jsonl")).sorted().toList());
            return files;
        }
    }

    /** A text's id: the SHA-256 of its UTF-16 units (little-endian), which a lone surrogate survives -- UTF-8 would
     *  write it as {@code ?} and two texts could share an id. */
    private static String sha(String text) throws NoSuchAlgorithmException {
        MessageDigest md = MessageDigest.getInstance("SHA-256");
        return HexFormat.of().formatHex(md.digest(text.getBytes(StandardCharsets.UTF_16LE)));
    }
}
