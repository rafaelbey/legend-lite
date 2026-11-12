// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

package com.legend.frontoracle;

import com.legend.door.ModelFromText;
import com.legend.json.Json;
import com.legend.model.ParsedModel;
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
import java.util.Set;
import java.util.TreeMap;

/**
 * THE MODEL DIFFERENTIAL'S JAVA SIDE (docs/rust-port/PHASE3.md, slice 1b, 2026-11-12): every model text of the front
 * oracle's question set ({@code rust/oracles/front/questions.jsonl.zst}, decompressed), at every dialect it is asked
 * at ({@code ["pmcd", dialect]}), through the Java door ({@link ModelFromText#parse}: the Rust parser through the
 * shim, read back into the protocol records, made the compiler's model by {@code ModelFromProtocol}), its answer's
 * canonical text ({@link #canonical}) hashed. Rust's door ({@code rust/lite-builtin}, {@code door::parse}) is held to
 * these hashes line for line ({@code rust/lite-builtin/tests/model_differential.rs}); the file is frozen under
 * {@code rust/oracles/model/} before the Java goes (the second switch).
 *
 * <p>The canonical text is the front oracle's form ({@link FrontAnswers#canonical}): {@code A} and the
 * {@link RecordCodec}'s JSON of the {@link ParsedModel} (every record, every component, in canonical-constructor order,
 * a map in its iteration order), or {@code E}, the error's class, its line and column (a {@code ParseException}'s),
 * and its message. No overlay grammar is registered: both sides parse with the built-in grammars alone.
 *
 * <p>A line per ask, in the questions file's order: {@code sha TAB dialect TAB hash TAB A|E}, the hash the SHA-256 of
 * the form's UTF-16 units, little-endian. {@code --texts FILE} also writes every form, one JSON line each
 * ({@code {"sha","dialect","form"}}), for the archive a mismatch is read from once Java is gone. The maps' runtime
 * classes are checked: a map whose order is a JVM's salt (an {@code ImmutableCollections.MapN} of two or more
 * entries, a {@code HashMap}) would make the text unstable, and fails the run.
 *
 * <pre>
 *   java ... core/src/test/java/com/legend/frontoracle/ModelAnswers.java QUESTIONS.jsonl OUT.tsv [--texts FILE]
 * </pre>
 */
public final class ModelAnswers {

    private ModelAnswers() {
    }

    /** The maps' runtime classes whose order is insertion order (or that hold at most one entry). */
    private static final Set<String> ORDERED = Set.of("java.util.LinkedHashMap",
            "java.util.Collections$UnmodifiableMap", "java.util.ImmutableCollections$Map1");

    public static void main(String[] args) throws Exception {
        Path questions = Path.of(args[0]);
        Path out = Path.of(args[1]);
        Path texts = args.length > 3 && args[2].equals("--texts") ? Path.of(args[3]) : null;
        Exception[] failed = new Exception[1];
        long[] counts = new long[3];
        // the reader and the codec recurse through deep records: the whole pass on one deep-stack thread
        Thread t = new Thread(null, () -> {
            try {
                run(questions, out, texts, counts);
            } catch (Exception e) {
                failed[0] = e;
            }
        }, "model-answers", 1L << 30);
        t.start();
        t.join();
        if (failed[0] != null) {
            throw failed[0];
        }
        System.out.printf("[model-answers] %d asks: %d answered, %d errors%n", counts[0], counts[1], counts[2]);
    }

    private static void run(Path questions, Path out, Path texts, long[] counts) throws Exception {
        MessageDigest md = MessageDigest.getInstance("SHA-256");
        RecordCodec codec = new RecordCodec();
        TreeMap<String, Long> errorClasses = new TreeMap<>();
        try (BufferedReader in = Files.newBufferedReader(questions, StandardCharsets.UTF_8);
                BufferedWriter w = Files.newBufferedWriter(out, StandardCharsets.UTF_8);
                BufferedWriter tw = texts == null ? null : Files.newBufferedWriter(texts, StandardCharsets.UTF_8)) {
            for (String line = in.readLine(); line != null; line = in.readLine()) {
                Json.Obj o = (Json.Obj) Json.parse(line);
                String sha = ((Json.Str) o.get("sha")).value();
                String text = ((Json.Str) o.get("text")).value();
                for (Json.Node a : ((Json.Arr) o.get("asks")).items()) {
                    List<Json.Node> pair = ((Json.Arr) a).items();
                    if (!((Json.Str) pair.get(0)).value().equals("pmcd")) {
                        continue;
                    }
                    Dialect dialect = Dialect.valueOf(((Json.Str) pair.get(1)).value());
                    String form = canonical(text, dialect, codec, errorClasses);
                    String hash = HexFormat.of().formatHex(md.digest(form.getBytes(StandardCharsets.UTF_16LE)));
                    boolean error = form.startsWith("E\t");
                    counts[0]++;
                    counts[error ? 2 : 1]++;
                    w.write(sha + "\t" + dialect.name() + "\t" + hash + "\t" + (error ? "E" : "A") + "\n");
                    if (tw != null) {
                        StringBuilder j = new StringBuilder("{\"sha\":\"").append(sha).append("\",\"dialect\":\"")
                                .append(dialect.name()).append("\",\"form\":");
                        RecordCodec.string(j, form);
                        tw.write(j.append("}\n").toString());
                    }
                }
            }
        }
        for (String c : codec.containers) {
            System.out.println("[model-answers] container met: " + c);
            if (!ORDERED.contains(c) && !c.equals("java.util.ImmutableCollections$MapN")) {
                throw new IllegalStateException("a map whose order is not insertion order: " + c);
            }
        }
        System.out.println("[model-answers] error classes: " + errorClasses);
    }

    /** {@code text} at {@code dialect} through the Java door: the canonical text of its model, or of its error. */
    static String canonical(String text, Dialect dialect, RecordCodec codec, TreeMap<String, Long> errorClasses) {
        ParsedModel model;
        try {
            model = ModelFromText.parse(text, dialect);
        } catch (RuntimeException e) {
            errorClasses.merge(e.getClass().getName(), 1L, Long::sum);
            String position = e instanceof ParseException pe ? pe.line() + "\t" + pe.column() : "\t";
            return "E\t" + e.getClass().getName() + "\t" + position + "\t"
                    + (e.getMessage() == null ? "null" : e.getMessage());
        }
        refuseSaltedMaps(model);
        return "A\t" + codec.write(model, ParsedModel.class);
    }

    /** A map of two or more entries whose iteration order is a JVM's salt ({@code Map.of}, {@code Map.copyOf}) would
     *  make the canonical text differ between runs: refused, wherever the model holds one. */
    private static void refuseSaltedMaps(Object o) {
        if (o == null) {
            return;
        }
        if (o instanceof java.util.Map<?, ?> m) {
            String c = m.getClass().getName();
            if ((c.equals("java.util.ImmutableCollections$MapN") && m.size() > 1) || c.equals("java.util.HashMap")) {
                throw new IllegalStateException("a model map of " + m.size() + " entries in a salted order: " + c);
            }
            for (var e : m.entrySet()) {
                refuseSaltedMaps(e.getKey());
                refuseSaltedMaps(e.getValue());
            }
        } else if (o instanceof java.util.Collection<?> c) {
            for (Object x : c) {
                refuseSaltedMaps(x);
            }
        } else if (o instanceof Record r) {
            for (var rc : r.getClass().getRecordComponents()) {
                try {
                    refuseSaltedMaps(rc.getAccessor().invoke(r));
                } catch (ReflectiveOperationException e) {
                    throw new IllegalStateException(e);
                }
            }
        }
    }
}
