// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

package com.legend.frontoracle;

import com.legend.json.Json;
import com.legend.parser.Dialect;
import com.legend.parser.ParseException;
import com.legend.parser.RustParsers;
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
 *
 * <p>With {@code --engine} first (step 5), the rest of the arguments go to {@link FrontEngineAnswers}: the engine-JSON
 * doors' answers for a question set ({@code --engine QUESTIONS OUT}, Java's; {@code --engine --shim QUESTIONS OUT},
 * the switched doors'), so {@code //core:front_show} and {@code //core:front_shim} answer any set, not only the
 * oracle's. The debug mode writes a question's engine forms too, when it is asked at an engine door.
 *
 * <p>With {@code --probes QUESTIONS} (step 6a, {@code //core:front_shim}), the parity harness's probes asked both ways
 * ({@link Probes}): what //parser-equivalence's gate tests asked of the Java parser's internals, against what they ask
 * the Rust parser since.
 */
public final class FrontAnswers {

    private FrontAnswers() {
    }

    public static void main(String[] args) throws Exception {
        if (args.length > 0 && args[0].equals("--engine")) {
            FrontEngineAnswers.main(java.util.Arrays.copyOfRange(args, 1, args.length));
            return;
        }
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
        if (args.length == 2 && args[0].equals("--probes")) {
            // Java's parser recurses through deep texts: the whole pass on one deep-stack thread, as --shim's
            int[] differ = new int[1];
            Exception[] failed = new Exception[1];
            Thread t = new Thread(null, () -> {
                try {
                    differ[0] = Probes.compare(Path.of(args[1]));
                } catch (Exception e) {
                    failed[0] = e;
                }
            }, "front-probes", 1L << 30);
            t.start();
            t.join();
            if (failed[0] != null) {
                throw failed[0];
            }
            if (differ[0] > 0) {
                throw new IllegalStateException(differ[0] + " probe answers differ between the Java and Rust parsers");
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
     *  written to {@code DIR/<door>.<dialect>.<sha>.java.txt} -- beside the Rust form the differential writes; and, for
     *  a question at an engine-JSON door (the model door at the engine's dialect, the lambda door), its two engine
     *  forms ({@link FrontEngineAnswers#forms}) to {@code DIR/<engine door>.<sha>.java.spans.txt} and
     *  {@code .java.bare.txt}, beside the engine differential's Rust forms. */
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
                        String engineDoor = door.equals("pmcd") && dialect.equals("LEGEND_ENGINE") ? "document"
                                : door.equals("lambda") ? "lambdaJson" : null;
                        if (engineDoor != null) {
                            String[] forms = FrontEngineAnswers.forms(answer);
                            Files.writeString(dir.resolve(engineDoor + "." + sha + ".java.spans.txt"), forms[0],
                                    StandardCharsets.UTF_8);
                            Files.writeString(dir.resolve(engineDoor + "." + sha + ".java.bare.txt"), forms[1],
                                    StandardCharsets.UTF_8);
                        }
                    }
                }
            }
        }
    }

    /**
     * THE PARITY HARNESS'S PROBES, BOTH WAYS (docs/rust-port/PHASE2.md §4 step 6a): what //parser-equivalence's gate
     * tests asked of the Java lexer's and parser's internals -- asked of them as the tests did, and of the Rust
     * library's probes ({@link RustParsers}' {@code sectionHeaders}, {@code elementSites}, {@code databaseSites},
     * {@code knownSections}), which the tests ask since -- over every text of a question set, each pair of answers
     * compared: every text's section headers; at every dialect a model text is asked at, the strict element sites (a
     * count, or the first error's class and message) and the Database sites (each one's name and JSON, or the first
     * error); at the engine's dialect, what {@code ParserEquivalence} reads (every element's JSON, in order) and what
     * {@code LegendLiteSectionParser} reads (each {@code ###Pure} section's code alone: its imports, each element's
     * path and JSON) -- the Java parser's own section walk, {@code PmcdParser.parseSections}, against the model
     * door's records; and the known sections against the registry's built-in half. Prints the counts and the first
     * mismatches; {@code bazel run //core:front_shim -- --probes QUESTIONS} fails on any. Retires with the Java parser.
     */
    static final class Probes {

        private static final int SHOWN = 20;

        private Probes() {
        }

        /** The comparison over {@code questions}: how many answers differ. */
        static int compare(Path questions) throws java.io.IOException {
            RustParsers rust = RustParsers.provider();
            java.util.Map<String, int[]> counts = new java.util.TreeMap<>();
            List<String> differ = new java.util.ArrayList<>();
            java.util.function.BiConsumer<String, Boolean> count = (probe, same) -> {
                int[] c = counts.computeIfAbsent(probe, k -> new int[3]);
                c[0]++;
                c[same ? 1 : 2]++;
            };
            try (BufferedReader in = Files.newBufferedReader(questions, StandardCharsets.UTF_8)) {
                for (String line = in.readLine(); line != null; line = in.readLine()) {
                    Json.Obj o = (Json.Obj) Json.parse(line);
                    String sha = ((Json.Str) o.get("sha")).value();
                    String text = ((Json.Str) o.get("text")).value();
                    compare("sectionHeaders", sha, javaHeaders(text), answer(() -> headers(rust.sectionHeaders(text))),
                            count, differ);
                    for (Json.Node a : ((Json.Arr) o.get("asks")).items()) {
                        List<Json.Node> pair = ((Json.Arr) a).items();
                        if (!((Json.Str) pair.get(0)).value().equals("pmcd")) {
                            continue;
                        }
                        Dialect dialect = Dialect.valueOf(((Json.Str) pair.get(1)).value());
                        String at = sha + " " + dialect.name();
                        compare("elementSites " + dialect.name(), at, javaElementSites(text, dialect),
                                answer(() -> "A " + rust.elementSites(text, dialect)), count, differ);
                        compare("databaseSites " + dialect.name(), at, javaDatabaseSites(text, dialect),
                                answer(() -> databases(rust.databaseSites(text, dialect))), count, differ);
                        if (dialect == Dialect.LEGEND_ENGINE) {
                            // ParserEquivalence's elements (their JSON, in order), the Java parser's section walk
                            // against the model door's records they are read from since
                            compare("sections: elements' JSON", at, javaSections(text, false),
                                    answer(() -> doorSections(text, false)), count, differ);
                            // LegendLiteSectionParser's: each ###Pure section's code alone, as the engine hands it
                            // over, read for its imports and each element's path and JSON
                            for (String code : pureSectionCodes(text)) {
                                compare("sections: a ###Pure section's imports, paths and JSON", at,
                                        javaSections(code, true), answer(() -> doorSections(code, true)), count,
                                        differ);
                            }
                        }
                    }
                }
            }
            java.util.Set<String> builtIn = new java.util.TreeSet<>(
                    com.legend.parser.SectionGrammarRegistry.all().keySet());
            for (RustParsers.Overlay overlay : com.legend.parser.SectionGrammarRegistry.overlays()) {
                builtIn.remove(overlay.name());
            }
            compare("knownSections", "-", String.join(",", builtIn),
                    String.join(",", new java.util.TreeSet<>(rust.knownSections())), count, differ);
            for (var e : counts.entrySet()) {
                System.out.printf(java.util.Locale.ROOT, "[front-probes] %s: %d asked, %d identical, %d differ%n",
                        e.getKey(), e.getValue()[0], e.getValue()[1], e.getValue()[2]);
            }
            differ.stream().limit(SHOWN).forEach(d -> System.out.println("[front-probes]   " + d));
            return differ.size();
        }

        private static void compare(String probe, String at, String java, String rust,
                java.util.function.BiConsumer<String, Boolean> count, List<String> differ) {
            boolean same = java.equals(rust);
            count.accept(probe, same);
            // what Java answered: an error, or how many headers, sites or Databases
            count.accept(probe + (java.startsWith("E ") ? " | Java errors" : " | Java answers"), same);
            if (!java.startsWith("E ")) {
                // (a sections answer with paths has its imports first)
                int n = probe.startsWith("elementSites") ? Integer.parseInt(java.substring(2))
                        : (int) java.chars().filter(ch -> ch == '\n').count() - (probe.contains("imports") ? 1 : 0);
                for (int i = 0; i < n; i++) {
                    count.accept(probe + " | sites in Java's answers", same);
                }
            }
            if (!same) {
                // the first line the two answers differ at
                String[] j = java.split("\n", -1);
                String[] r = rust.split("\n", -1);
                int k = 0;
                while (k < j.length && k < r.length && j[k].equals(r[k])) {
                    k++;
                }
                differ.add(probe + " " + at + " line " + k + ": Java " + head(k < j.length ? j[k] : "(none)")
                        + " | Rust " + head(k < r.length ? r[k] : "(none)"));
            }
        }

        private static String head(String s) {
            String one = s.replace('\n', '|');
            return one.length() > 200 ? one.substring(0, 200) + "..." : one;
        }

        /** An answer, or the error it threw as {@code E <class> <message>}. */
        private static String answer(java.util.function.Supplier<String> probe) {
            try {
                return probe.get();
            } catch (Throwable t) {
                return "E " + t.getClass().getName() + " " + t.getMessage();
            }
        }

        /** {@code Sectionize.plan}'s: the Java lexer's headers. */
        private static String javaHeaders(String text) {
            return answer(() -> {
                StringBuilder b = new StringBuilder("A");
                for (com.legend.lexer.TokenStream.SectionHeader h
                        : com.legend.lexer.Lexer.tokenize(text).sectionHeaders()) {
                    b.append('\n').append(h.name()).append('@').append(h.contentStartOffset());
                }
                return b.toString();
            });
        }

        private static String headers(List<RustParsers.SectionHeader> headers) {
            StringBuilder b = new StringBuilder("A");
            for (RustParsers.SectionHeader h : headers) {
                b.append('\n').append(h.name()).append('@').append(h.contentStartOffset());
            }
            return b.toString();
        }

        /** {@code RejectionParityTest.parseLegendEngine}'s second half, as it was: Java's element-at path. */
        private static String javaElementSites(String text, Dialect dialect) {
            return answer(() -> {
                var ts = com.legend.lexer.Lexer.tokenize(text);
                int parsed = 0;
                for (com.legend.lexer.TokenType marker : new com.legend.lexer.TokenType[]{
                        com.legend.lexer.TokenType.CLASS, com.legend.lexer.TokenType.ENUM,
                        com.legend.lexer.TokenType.PROFILE, com.legend.lexer.TokenType.ASSOCIATION,
                        com.legend.lexer.TokenType.FUNCTION}) {
                    for (int i : com.legend.parser.ElementParser.topLevelIndexes(ts, marker)) {
                        var p = com.legend.parser.ElementParser.at(ts, i, dialect);
                        switch (marker) {
                            case CLASS -> p.parseClassDefinition(false);
                            case ENUM -> p.parseEnumDefinition();
                            case PROFILE -> p.parseProfileDefinition();
                            case ASSOCIATION -> p.parseAssociationDefinition();
                            default -> p.parseFunctionProtocol();
                        }
                        parsed++;
                    }
                }
                return "A " + parsed;
            });
        }

        /** {@code ViewFilterParityTest}'s site scan, as it was: Java's lexer, Database parser and emitter. */
        private static String javaDatabaseSites(String text, Dialect dialect) {
            return answer(() -> {
                var ts = com.legend.lexer.Lexer.tokenize(text);
                StringBuilder b = new StringBuilder("A");
                for (int i = 0; i < ts.count(); i++) {
                    if (ts.type(i) != com.legend.lexer.TokenType.DATABASE) {
                        continue;
                    }
                    if (i > 0 && ts.type(i - 1) != com.legend.lexer.TokenType.PAREN_CLOSE
                            && ts.type(i - 1) != com.legend.lexer.TokenType.BRACE_CLOSE
                            && ts.type(i - 1) != com.legend.lexer.TokenType.SEMI_COLON) {
                        continue;
                    }
                    var db = com.legend.parser.DatabaseProtocolParser.parse(ts, i, dialect);
                    b.append('\n').append(db.qualifiedName()).append('\t')
                            .append(com.legend.protocol.ProtocolEmitter.emitElement(db));
                }
                return b.toString();
            });
        }

        /** {@code PmcdParser.parseSections}, the Java parser's walk at the engine's dialect, as ParserEquivalence and
         *  LegendLiteSectionParser read it: each element's JSON, in order; with {@code paths}, the sections' imports
         *  first and each element's path before its JSON. */
        private static String javaSections(String text, boolean paths) {
            return answer(() -> {
                StringBuilder imports = new StringBuilder("A");
                StringBuilder elements = new StringBuilder();
                if (paths) {
                    imports.append("\nimports");
                }
                for (com.legend.parser.PmcdParser.DocSection s : com.legend.parser.PmcdParser.parseSections(text)) {
                    s.imports().forEach(i -> imports.append(' ').append(i));
                    for (com.legend.parser.PmcdParser.DocElement e : s.elements()) {
                        elements.append('\n').append(paths ? e.path() + "\t" : "").append(e.json());
                    }
                }
                return (paths ? imports : new StringBuilder("A")).append(elements).toString();
            });
        }

        /** The same read off the model door's records ({@code PmcdParser.parseModel}), as the two read them since: each
         *  element but the section index; the index's imports, in section order; a path, {@code Protocol.pathOf}. */
        private static String doorSections(String text, boolean paths) {
            StringBuilder imports = new StringBuilder("A");
            StringBuilder elements = new StringBuilder();
            if (paths) {
                imports.append("\nimports");
            }
            for (com.legend.protocol.Protocol.Element e : com.legend.parser.PmcdParser.parseModel(text).elements()) {
                if (e instanceof com.legend.protocol.Protocol.PSectionIndex index) {
                    for (com.legend.protocol.Protocol.PSection s : index.sections()) {
                        s.imports().forEach(i -> imports.append(' ').append(i));
                    }
                } else {
                    elements.append('\n').append(paths ? com.legend.protocol.Protocol.pathOf(e) + "\t" : "")
                            .append(com.legend.protocol.ProtocolEmitter.emitElement(e));
                }
            }
            return (paths ? imports : new StringBuilder("A")).append(elements).toString();
        }

        /** Each {@code ###Pure} section's code alone -- the leading headerless one included -- as the engine's walker
         *  hands a section to the parser LegendLiteSectionParser stands in for: from its content's start to the next
         *  header's {@code ###}. */
        private static List<String> pureSectionCodes(String text) {
            List<String> codes = new java.util.ArrayList<>();
            String name = "Pure";
            int start = 0;
            for (com.legend.lexer.TokenStream.SectionHeader h
                    : com.legend.lexer.Lexer.tokenize(text).sectionHeaders()) {
                if (name.equals("Pure")) {
                    codes.add(text.substring(start, Math.max(start, h.nameOffset() - 3)));
                }
                name = h.name();
                start = h.contentStartOffset();
            }
            if (name.equals("Pure")) {
                codes.add(text.substring(Math.min(start, text.length())));
            }
            return codes;
        }

        private static String databases(List<RustParsers.DatabaseSite> sites) {
            StringBuilder b = new StringBuilder("A");
            for (RustParsers.DatabaseSite s : sites) {
                b.append('\n').append(s.qualifiedName()).append('\t').append(s.json());
            }
            return b.toString();
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
