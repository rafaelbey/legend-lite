// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

package com.legend.frontoracle;

import com.legend.json.Json;
import com.legend.parser.Dialect;
import com.legend.parser.ParseException;
import com.legend.parser.PmcdParser;
import com.legend.parser.SpecParser;
import java.io.BufferedReader;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.security.MessageDigest;
import java.util.ArrayList;
import java.util.HexFormat;
import java.util.List;

/**
 * THE ENGINE-JSON DOORS END TO END, AGAINST JAVA'S FROZEN ANSWERS (Rust port phase 2, step 5: docs/rust-port/PHASE2.md
 * §4, "The engine JSON"; {@code bazel run //core:front_shim -- --engine}): the front oracle's questions at the two
 * doors whose answer is the engine's JSON -- {@code document}, a model text at the engine's dialect
 * ({@code PmcdParser.parseDocument}), and {@code lambdaJson}, a lambda ({@code SpecParser.lambdaJson}) -- asked through
 * the switched doors, each form its own call (with its source information, and without it), each form hashed and the
 * pair compared, line for line, with the Java parser's and emitter's answer frozen before they were deleted
 * ({@code rust/oracles/front/engine.tsv.zst}: {@code door, the spans form's sha, the bare form's sha, A or E} per
 * engine ask, in the questions file's order).
 *
 * <p>The form is {@code A\t<json>}; or {@code E}, the error's class, line, column and message as
 * {@link FrontAnswers#canonical} writes them -- the parser's error, or the emitter's refusal of a model it parsed. A
 * Java string hashed as its UTF-16 units, little-endian, each unit as it is: a lone surrogate the emitter writes raw
 * hashes as itself. Until 2026-11-06 the same class wrote Java's answers ({@code //core:front_engine_answers}, the file
 * frozen since); that mode went with the Java parser (step 6b).
 */
public final class FrontEngineAnswers {

    private FrontEngineAnswers() {
    }

    /** Every engine ask of {@code questions} through the switched doors, compared with {@code frozen} in lockstep;
     *  how many differ. Prints the counts and the first differences. */
    static long compare(Path questions, Path frozen) throws Exception {
        MessageDigest md = MessageDigest.getInstance("SHA-256");
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
                    String engineDoor = door.equals("pmcd") && dialect == Dialect.LEGEND_ENGINE ? "document"
                            : door.equals("lambda") ? "lambdaJson" : null;
                    if (engineDoor == null) {
                        continue;
                    }
                    String[] forms = shimForms(engineDoor, text);
                    String mine = engineDoor + "\t" + hash(md, forms[0]) + "\t" + hash(md, forms[1]) + "\t"
                            + forms[0].substring(0, 1);
                    String theirs = java.readLine();
                    if (theirs == null) {
                        throw new IllegalStateException("the frozen file ends before the question set's engine asks: "
                                + frozen);
                    }
                    if (!theirs.startsWith(engineDoor + "\t")) {
                        throw new IllegalStateException("the frozen file is out of step with the questions at "
                                + sha + " (" + engineDoor + "): " + theirs);
                    }
                    asks++;
                    if (!mine.equals(theirs)) {
                        differ.add(engineDoor + " " + sha);
                    }
                }
            }
            if (java.readLine() != null) {
                throw new IllegalStateException("the frozen file holds more lines than the question set's engine asks: "
                        + frozen);
            }
        }
        System.out.printf("[front-shim] the engine-JSON doors: %d asks, %d identical to Java's frozen answers, %d"
                + " differ%n", asks, asks - differ.size(), differ.size());
        differ.stream().limit(20).forEach(d -> System.out.println("[front-shim]   differs: " + d));
        return differ.size();
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
