// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

package com.legend.frontoracle;

import com.legend.json.Json;
import com.legend.parser.Dialect;
import com.legend.parser.ParseException;
import com.legend.parser.PmcdParser;
import com.legend.parser.SpecParser;
import com.legend.protocol.SourceInformation;
import java.io.BufferedReader;
import java.io.BufferedWriter;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.security.MessageDigest;
import java.util.HexFormat;
import java.util.List;

/**
 * JAVA'S ENGINE JSON, HASHED (Rust port phase 2, step 5: docs/rust-port/PHASE2.md §4, "The engine JSON"): the front
 * oracle's questions at the two doors whose answer is the engine's JSON, asked of Java's parser and emitter --
 * {@code document}, a model text at the engine's dialect ({@code PmcdParser.parseDocument}), and {@code lambdaJson},
 * a lambda ({@code SpecParser.lambdaJson}) -- each answer's form hashed twice: with its source information, and
 * without it ({@code SourceInformation.withoutSpans}, the doors' {@code returnSourceInformation=false}). What the Rust
 * emitter is held to, question by question, live while the Java emitter exists.
 *
 * <p>The form is {@code A\t<json>}; or {@code E}, the error's class, line, column and message as
 * {@link FrontAnswers#canonical} writes them -- the parser's error, or the emitter's refusal of a model it parsed. A
 * Java string hashed as its UTF-16 units, little-endian, each unit as it is: a lone surrogate the emitter writes raw
 * hashes as itself (2026-11-06, step 5: {@code getBytes(UTF_16LE)} wrote it as U+FFFD, so a lone surrogate and a real
 * U+FFFD hashed alike; no oracle answer holds one, so the file's bytes did not move).
 *
 * <p>With {@code --shim} first (step 5's switch: {@code //core:front_shim}), the same questions asked through the
 * switched doors -- {@code PmcdParser.parseDocument(text, sourceInformation)} and {@code SpecParser.lambdaJson},
 * each form its own call, Rust's parser and emitter and Rust's cut -- and hashed in the same form: the shim end to end,
 * compared line by line with Java's file.
 *
 * <pre>a java_run: FrontEngineAnswers [--shim] QUESTIONS_JSONL OUT_TSV  (door, sha, the spans form's sha, the no-spans
 * form's sha, A or E)</pre>
 */
public final class FrontEngineAnswers {

    private FrontEngineAnswers() {
    }

    public static void main(String[] argv) throws Exception {
        boolean shim = argv.length == 3 && argv[0].equals("--shim");
        String[] args = shim ? new String[]{argv[1], argv[2]} : argv;
        // the parser, the emitter and withoutSpans recurse through the deepest texts: the whole pass on one deep-stack
        // thread (TRIAGE A3), which keeps one worker of the shim's for every call
        Exception[] failed = new Exception[1];
        Thread t = new Thread(null, () -> {
            try {
                answerAll(Path.of(args[0]), Path.of(args[1]), shim);
            } catch (Exception e) {
                failed[0] = e;
            }
        }, "front-engine", 1L << 30);
        t.start();
        t.join();
        if (failed[0] != null) {
            throw failed[0];
        }
    }

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
                    String engineDoor = door.equals("pmcd") && dialect == Dialect.LEGEND_ENGINE ? "document"
                            : door.equals("lambda") ? "lambdaJson" : null;
                    if (engineDoor == null) {
                        continue;
                    }
                    FrontOracle.Question q = new FrontOracle.Question(door, dialect, text);
                    String[] forms = shim ? shimForms(engineDoor, text) : forms(FrontOracle.ask(q, codec));
                    out.append(engineDoor).append('\t').append(sha).append('\t').append(hash(md, forms[0]))
                            .append('\t').append(hash(md, forms[1])).append('\t').append(forms[0].substring(0, 1))
                            .append('\n');
                }
            }
        }
    }

    /** The answer's two forms: with its source information, and without it. */
    static String[] forms(FrontOracle.Answer a) {
        Throwable e = a.error() != null ? a.error() : a.engineError();
        if (e != null) {
            String form = error(e);
            return new String[]{form, form};
        }
        String json = java.util.Objects.requireNonNull(a.engine(), "a parsed answer at an engine door has its JSON");
        String bare;
        try {
            bare = "A\t" + SourceInformation.withoutSpans(json);
        } catch (RuntimeException | StackOverflowError x) {
            bare = error(x);
        }
        return new String[]{"A\t" + json, bare};
    }

    /** The two forms through the switched doors: each its own call, as a caller asks for one form. */
    static String[] shimForms(String engineDoor, String text) {
        return new String[]{shimForm(engineDoor, text, true), shimForm(engineDoor, text, false)};
    }

    private static String shimForm(String engineDoor, String text, boolean sourceInformation) {
        try {
            return "A\t" + (engineDoor.equals("document") ? PmcdParser.parseDocument(text, sourceInformation)
                    : SpecParser.lambdaJson(text, sourceInformation));
        } catch (RuntimeException e) {
            return error(e);
        }
    }

    private static String error(Throwable e) {
        String position = e instanceof ParseException pe ? pe.line() + "\t" + pe.column() : "\t";
        return "E\t" + e.getClass().getName() + "\t" + position + "\t"
                + (e.getMessage() == null ? "null" : e.getMessage());
    }

    /** The form's UTF-16 units, little-endian, each as it is (a lone surrogate too), hashed. */
    static String hash(MessageDigest md, String form) {
        byte[] bytes = new byte[form.length() * 2];
        for (int i = 0; i < form.length(); i++) {
            char c = form.charAt(i);
            bytes[2 * i] = (byte) c;
            bytes[2 * i + 1] = (byte) (c >>> 8);
        }
        return HexFormat.of().formatHex(md.digest(bytes));
    }
}
