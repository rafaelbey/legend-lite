// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

package com.legend.frontoracle;

import com.legend.json.Json;
import com.legend.parser.Dialect;
import com.legend.parser.PmcdParser;
import com.legend.parser.SpecParser;
import java.io.BufferedReader;
import java.lang.management.ManagementFactory;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.Arrays;
import java.util.List;
import java.util.Map;
import java.util.TreeMap;

/**
 * THE PARSE-TIME RECEIPT (Rust port phase 2, step 1: docs/rust-port/PHASE2.md §4): the front oracle's question set
 * asked of the parser's doors for several passes on one warm JVM, the last pass timed per door and dialect -- before
 * the switch the Java parser, after it the shim, so the two receipts compare the same calls. Also the bootstrap's
 * first touch of {@code builtin.Pure}, whose initialiser parses the platform's prelude. {@code --java} asks the Java
 * parser's twins ({@code parseInJava} and its kin) instead, so both receipts can be taken at one load, while the Java
 * parser exists (docs/rust-port/PHASE2.md §4 step 6 deletes it).
 *
 * <p>{@code --engine} (step 5) times the two engine-JSON doors instead, over the oracle's questions at them -- each
 * model text at the engine's dialect ({@code document}) and each lambda ({@code lambdaJson}), each asked twice, with
 * its source information ({@code spans}) and without it ({@code bare}): the switched doors
 * ({@code PmcdParser.parseDocument}, {@code SpecParser.lambdaJson}, the shim), or with {@code --java} Java's parser and
 * emitter exactly as the doors were before the switch ({@code ProtocolEmitter.emit(parseInJava(text,
 * LEGEND_ENGINE).model())}, {@code ProtocolEmitter.emitLambda(parseLambdaInJava(text))}, each cut by
 * {@code SourceInformation.withoutSpans} for the bare form).
 *
 * <pre>bazel run //core:front_timing -- QUESTIONS_JSONL [PASSES] [RAISED_OUT] [--java] [--engine]</pre>
 */
public final class FrontTiming {

    private FrontTiming() {
    }

    private record Ask(String door, Dialect dialect, String sha, String text) {
    }

    public static void main(String[] argv) throws Exception {
        boolean java = Arrays.asList(argv).contains("--java");
        boolean engine = Arrays.asList(argv).contains("--engine");
        String[] args = Arrays.stream(argv).filter(s -> !s.equals("--java") && !s.equals("--engine"))
                .toArray(String[]::new);
        double load = ManagementFactory.getOperatingSystemMXBean().getSystemLoadAverage();
        long b0 = System.nanoTime();
        Class.forName("com.legend.builtin.Pure");
        long bootstrap = System.nanoTime() - b0;

        Path questions = Path.of(args[0]);
        int passes = args.length > 1 ? Integer.parseInt(args[1]) : 3;
        List<Ask> asks = new ArrayList<>();
        try (BufferedReader in = Files.newBufferedReader(questions, StandardCharsets.UTF_8)) {
            for (String line = in.readLine(); line != null; line = in.readLine()) {
                Json.Obj o = (Json.Obj) Json.parse(line);
                String text = ((Json.Str) o.get("text")).value();
                String sha = ((Json.Str) o.get("sha")).value();
                for (Json.Node a : ((Json.Arr) o.get("asks")).items()) {
                    List<Json.Node> pair = ((Json.Arr) a).items();
                    String door = ((Json.Str) pair.get(0)).value();
                    Dialect dialect = Dialect.valueOf(((Json.Str) pair.get(1)).value());
                    if (!engine) {
                        asks.add(new Ask(door, dialect, sha, text));
                    } else if (door.equals("pmcd") && dialect == Dialect.LEGEND_ENGINE || door.equals("lambda")) {
                        String engineDoor = door.equals("pmcd") ? "document" : "lambdaJson";
                        asks.add(new Ask(engineDoor + " spans", dialect, sha, text));
                        asks.add(new Ask(engineDoor + " bare", dialect, sha, text));
                    }
                }
            }
        }
        Map<String, List<Long>> times = new TreeMap<>();
        java.util.Set<String> raised = new java.util.TreeSet<>();
        int failed = 0;
        for (int pass = 1; pass <= passes; pass++) {
            boolean last = pass == passes;
            failed = 0;
            raised.clear();
            for (Ask a : asks) {
                long t0 = System.nanoTime();
                try {
                    if (engine) {
                        engineDoor(a, java);
                    } else if (java) {
                        switch (a.door()) {
                            case "pmcd" -> PmcdParser.parseInJava(a.text(), a.dialect());
                            case "spec" -> SpecParser.parseInJava(a.text(), a.dialect());
                            case "lambda" -> SpecParser.parseLambdaInJava(a.text());
                            case "block" -> SpecParser.parseCodeBlockInJava(a.text(), a.dialect());
                            default -> throw new IllegalArgumentException("no door " + a.door());
                        }
                    } else {
                        switch (a.door()) {
                            case "pmcd" -> PmcdParser.parse(a.text(), a.dialect());
                            case "spec" -> SpecParser.parse(a.text(), a.dialect());
                            case "lambda" -> SpecParser.parseLambda(a.text());
                            case "block" -> SpecParser.parseCodeBlock(a.text(), a.dialect());
                            default -> throw new IllegalArgumentException("no door " + a.door());
                        }
                    }
                } catch (RuntimeException | StackOverflowError e) {
                    failed++;
                    raised.add(a.door() + "\t" + a.dialect() + "\t" + a.sha() + "\t" + e.getClass().getName());
                }
                if (last) {
                    times.computeIfAbsent(a.door() + " " + a.dialect(), k -> new ArrayList<>())
                            .add(System.nanoTime() - t0);
                }
            }
        }
        System.out.printf("[front-timing] %s%s; load average %.2f at start, %.2f at the end; %d asks, %d passes, %d"
                + " raised (last pass)%n", engine ? "the engine-JSON doors, " : "",
                java ? "the Java parser" : "the doors (the shim)", load,
                ManagementFactory.getOperatingSystemMXBean().getSystemLoadAverage(), asks.size(), passes, failed);
        System.out.printf("[front-timing] builtin.Pure first touch (the prelude's parse and build) %.1f ms%n",
                bootstrap / 1e6);
        long total = 0;
        for (Map.Entry<String, List<Long>> e : times.entrySet()) {
            long[] ns = e.getValue().stream().mapToLong(Long::longValue).sorted().toArray();
            long sum = Arrays.stream(ns).sum();
            total += sum;
            System.out.printf("[front-timing] %-22s n=%6d p50=%8.3f p95=%8.3f p99=%8.3f max=%9.3f sum=%10.1f ms%n",
                    e.getKey(), ns.length, at(ns, 0.50), at(ns, 0.95), at(ns, 0.99), ns[ns.length - 1] / 1e6,
                    sum / 1e6);
        }
        System.out.printf("[front-timing] total %.1f ms%n", total / 1e6);
        if (args.length > 2) {
            // the last pass's raising questions, to compare with the oracle's (a parse that answers differently on a
            // second asking is not a function of its text)
            Files.write(Path.of(args[2]), raised, StandardCharsets.UTF_8);
        }
    }

    /** One engine-JSON door's call: {@code document} or {@code lambdaJson}, its {@code spans} or {@code bare} form;
     *  through the switched doors, or ({@code java}) as Java's parser and emitter answered it before the switch. */
    private static String engineDoor(Ask a, boolean java) {
        boolean spans = a.door().endsWith(" spans");
        boolean document = a.door().startsWith("document");
        if (!java) {
            return document ? PmcdParser.parseDocument(a.text(), spans) : SpecParser.lambdaJson(a.text(), spans);
        }
        String json = document
                ? com.legend.protocol.ProtocolEmitter.emit(PmcdParser.parseInJava(a.text(), Dialect.LEGEND_ENGINE)
                        .model())
                : com.legend.protocol.ProtocolEmitter.emitLambda(SpecParser.parseLambdaInJava(a.text()));
        return spans ? json : com.legend.protocol.SourceInformation.withoutSpans(json);
    }

    private static double at(long[] sorted, double q) {
        return sorted[Math.min(sorted.length - 1, (int) Math.floor(q * sorted.length))] / 1e6;
    }
}
