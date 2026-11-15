// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

package com.legend.frontoracle;

import com.legend.builtin.Prelude;
import com.legend.builtin.SystemMetamodel;
import com.legend.compiler.KnowledgeLayer;
import com.legend.compiler.ModelBuilder;
import com.legend.compiler.NameResolver;
import com.legend.compiler.element.PureModelContext;
import com.legend.door.ModelFromText;
import com.legend.error.LegendCompileException;
import com.legend.json.Json;
import com.legend.model.AssociationMapping;
import com.legend.model.ClassDefinition;
import com.legend.model.ClassMapping;
import com.legend.model.CleanSheetMappingDefinition;
import com.legend.model.DatabaseDefinition;
import com.legend.model.FunctionDefinition;
import com.legend.model.LegacyMappingDefinition;
import com.legend.model.MappingDefinition;
import com.legend.model.NormalizedModel;
import com.legend.model.PackageableElement;
import com.legend.model.ParsedModel;
import com.legend.model.PropertyMapping;
import com.legend.model.ServiceDefinition;
import com.legend.normalizer.ModelNormalizer;
import com.legend.parser.Dialect;
import java.io.BufferedReader;
import java.io.BufferedWriter;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.security.MessageDigest;
import java.util.ArrayList;
import java.util.HexFormat;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.Set;
import java.util.TreeMap;
import java.util.TreeSet;

/**
 * THE NORMALIZER DIFFERENTIAL'S JAVA SIDE (docs/rust-port/PHASE3.md, the normalizer slice, 2026-11-14): what step E
 * hands to F, asked of the live Java, for {@code rust/lite-normalizer} to be held to byte for byte
 * ({@code tests/normalizer_differential.rs}), then frozen under {@code rust/oracles/normalizer/} before the Java goes.
 *
 * <p>Every model text of the front oracle ({@code rust/oracles/front/questions.jsonl.zst}, decompressed) at every
 * dialect it is asked at ({@code ["pmcd", dialect]}) that the door parses, and every model text of the slice's own
 * targeted cases ({@code rust/oracles/normalizer/cases.jsonl}, the same line form), driven through step D and then
 * step E ONLY -- what {@code Compiler.normalizeLayer} runs on {@code NameResolver.resolveAlongside}'s answer, never F:
 * <ol>
 *   <li>{@code NameResolver.resolveAlongside(parsed, bootFqns, walls)} (the compile's D);</li>
 *   <li>{@code KnowledgeLayer.adoptAssociationQualifiedProperties(resolved, walls)};</li>
 *   <li>{@code ModelBuilder.from(adopted)} (the graph's one index);</li>
 *   <li>{@code PureModelContext.refuseDuplicates(index, walls)};</li>
 *   <li>{@code ModelNormalizer.normalize(adopted, index, walls)} (E).</li>
 * </ol>
 * {@code Compiler.normalizeWithSystem}'s steps around {@code normalizeLayer} (the prelude's shadows dropped, a system
 * element's redefinition refused, the platform's own Pure adopted, the boot layer's prepared elements appended) are the
 * compile driver's, not E's: left out, so a text's answer is E's (or D's) alone. The canonical text:
 * <pre>
 *   S TAB strict       walls null throughout: the first error is E's or D's
 *   T TAB tolerant     one walls map threaded through D and E, as buildModule threads it; {@code =S} when equal
 * </pre>
 * a run is {@code A TAB} the record codec's JSON of the {@code NormalizedModel} (every element in order -- the
 * structural ones as D handed them, the compiled mappings with their binding tables, facts and resolved stores, every
 * lifted function -- the imports, the legacy surfaces), a tolerant run with walls prefixed by them
 * ({@code [[key,line],...] TAB}), an error {@code E TAB class TAB phase TAB element TAB message}.
 *
 * <p>Lines: {@code sha TAB dialect TAB hash} per ask, the hash the SHA-256 of the form's UTF-16 units, little-endian.
 * {@code --texts DIR} also writes every form ({@code forms.jsonl}) for the archive. The run's measurements (the error
 * classes met, strict and tolerant; how many asks reach each E family, read off the input and the answer) are printed
 * to stdout, and written per ask to {@code OUT.tsv.tags} ({@code sha TAB dialect TAB mode TAB tag|tag...}, so a
 * family's reach is counted in texts too), never hashed.
 *
 * <pre>
 *   java ... NormalizerAnswers.java OUT.tsv QUESTIONS.jsonl [MORE.jsonl ...] [--texts DIR] [--limit N]
 * </pre>
 */
public final class NormalizerAnswers {

    private NormalizerAnswers() {
    }

    /** The boot layer's element names, as {@code Compiler.bootFqns()} unites them. */
    private static final Set<String> BOOT_FQNS = bootFqns();

    private static Set<String> bootFqns() {
        Set<String> out = new java.util.HashSet<>(SystemMetamodel.elementFqns());
        out.addAll(Prelude.elementFqns());
        return out;
    }

    public static void main(String[] args) throws Exception {
        Path out = Path.of(args[0]);
        List<Path> questions = new ArrayList<>();
        Path texts = null;
        long limit = Long.MAX_VALUE;
        for (int i = 1; i < args.length; i++) {
            switch (args[i]) {
                case "--texts" -> texts = Path.of(args[++i]);
                case "--limit" -> limit = Long.parseLong(args[++i]);
                case "--probe" -> probe = true;
                default -> questions.add(Path.of(args[i]));
            }
        }
        Path t = texts;
        long l = limit;
        Exception[] failed = new Exception[1];
        Thread th = new Thread(null, () -> {
            try {
                run(out, questions, t, l);
            } catch (Exception e) {
                failed[0] = e;
            }
        }, "normalizer-answers", 1L << 30);
        th.start();
        th.join();
        if (failed[0] != null) {
            throw failed[0];
        }
    }

    /** The error classes met, per mode ({@code S:<class>} / {@code T:<class>}), and how many asks reach each family. */
    private static final TreeMap<String, Long> STATS = new TreeMap<>();

    /** The ask's tags per mode, written to {@code OUT.tsv.tags} once the ask is answered. */
    private static final Map<String, Set<String>> ASK_TAGS = new TreeMap<>();

    /** The ask being answered ({@code sha dialect}), for the log. */
    private static final String[] CURRENT = new String[1];

    /** {@code --probe}: the measuring run's hook (ruling 30, 2026-11-14) -- an instrumented build's counters of the
     *  Phase-D fallbacks E carries ({@code com.legend.normalizer.FallbackProbe.HITS}, a class only that never-committed
     *  build has), read by reflection per ask and mode, each fallback a text reaches counted once per ask. */
    private static boolean probe;

    @SuppressWarnings("unchecked")
    private static Map<String, Long> probeHits() {
        try {
            return (Map<String, Long>) Class.forName("com.legend.normalizer.FallbackProbe").getField("HITS").get(null);
        } catch (ReflectiveOperationException e) {
            throw new IllegalStateException("--probe needs the instrumented build's FallbackProbe", e);
        }
    }

    /** The relational names the lift emits under their relational spelling, which the typer (step G) refuses when it
     *  compiles the lifted body where the pinned engine compiles the model (the names batch, 2026-11-14): counted in
     *  the lifted bodies, never hashed. */
    private static final List<String> G_REFUSED_NAMES = List.of("like", "charindex", "locate", "COUNT", "SUM", "ABS");

    private static void run(Path out, List<Path> questions, Path texts, long limit) throws Exception {
        MessageDigest md = MessageDigest.getInstance("SHA-256");
        RecordCodec codec = new RecordCodec();
        long asks = 0;
        if (texts != null) {
            Files.createDirectories(texts);
        }
        try (BufferedWriter w = Files.newBufferedWriter(out, StandardCharsets.UTF_8);
                BufferedWriter gw = Files.newBufferedWriter(Path.of(out + ".tags"), StandardCharsets.UTF_8);
                BufferedWriter tw = texts == null ? null
                        : Files.newBufferedWriter(texts.resolve("forms.jsonl"), StandardCharsets.UTF_8)) {
            for (Path q : questions) {
                try (BufferedReader in = Files.newBufferedReader(q, StandardCharsets.UTF_8)) {
                    for (String line = in.readLine(); line != null; line = in.readLine()) {
                        Json.Obj o = (Json.Obj) Json.parse(line);
                        String sha = ((Json.Str) o.get("sha")).value();
                        String text = ((Json.Str) o.get("text")).value();
                        for (Json.Node a : ((Json.Arr) o.get("asks")).items()) {
                            List<Json.Node> pair = ((Json.Arr) a).items();
                            if (!((Json.Str) pair.get(0)).value().equals("pmcd") || asks >= limit) {
                                continue;
                            }
                            Dialect dialect = Dialect.valueOf(((Json.Str) pair.get(1)).value());
                            ParsedModel parsed;
                            try {
                                parsed = ModelFromText.parse(text, dialect);
                            } catch (RuntimeException e) {
                                continue;
                            }
                            asks++;
                            CURRENT[0] = sha + " " + dialect.name();
                            String form = form(parsed, codec);
                            w.write(sha + "\t" + dialect.name() + "\t" + hash(md, form) + "\n");
                            for (Map.Entry<String, Set<String>> m : ASK_TAGS.entrySet()) {
                                gw.write(sha + "\t" + dialect.name() + "\t" + m.getKey() + "\t"
                                        + String.join("|", m.getValue()) + "\n");
                            }
                            ASK_TAGS.clear();
                            if (tw != null) {
                                StringBuilder j = new StringBuilder("{\"sha\":\"").append(sha)
                                        .append("\",\"dialect\":\"").append(dialect.name()).append("\",\"form\":");
                                RecordCodec.string(j, form);
                                tw.write(j.append("}\n").toString());
                            }
                        }
                    }
                }
            }
        }
        System.out.printf("[normalizer-answers] %d asks answered%n", asks);
        for (Map.Entry<String, Long> e : STATS.entrySet()) {
            System.out.printf("[normalizer-answers] %s\t%d%n", e.getKey(), e.getValue());
        }
        System.out.printf("[normalizer-answers] record types reached %d: %s%n", codec.reached.size(), codec.reached);
        System.out.printf("[normalizer-answers] containers %s%n", codec.containers);
    }

    private static String hash(MessageDigest md, String form) {
        return HexFormat.of().formatHex(md.digest(form.getBytes(StandardCharsets.UTF_16LE)));
    }

    /** One ask: {@code S} strict, {@code T} tolerant (or {@code =S}). */
    static String form(ParsedModel parsed, RecordCodec codec) {
        String s = run(parsed, null, codec, "S");
        Map<String, String> walls = new LinkedHashMap<>();
        String t = run(parsed, walls, codec, "T");
        return "S\t" + s + "\nT\t" + (t.equals(s) ? "=S" : t);
    }

    /** D then E, strict ({@code walls} null) or tolerant. */
    private static String run(ParsedModel parsed, Map<String, String> walls, RecordCodec codec, String mode) {
        ParsedModel adopted = null;
        int wallsBeforeE = 0;
        if (probe) {
            probeHits().clear();
        }
        try {
            ParsedModel resolved = NameResolver.resolveAlongside(parsed, BOOT_FQNS, walls);
            adopted = KnowledgeLayer.adoptAssociationQualifiedProperties(resolved, walls);
            ModelBuilder index = ModelBuilder.from(adopted);
            PureModelContext.refuseDuplicates(index, walls);
            wallsBeforeE = walls == null ? 0 : walls.size();
            measureInput(mode, adopted);
            NormalizedModel nm = ModelNormalizer.normalize(adopted, index, walls);
            measureOutput(mode, nm, walls, wallsBeforeE);
            String json = codec.write(nm, NormalizedModel.class);
            return walls == null || walls.isEmpty() ? "A\t" + json : walls(walls) + "\tA\t" + json;
        } catch (RuntimeException | StackOverflowError e) {
            count(mode + ":error:" + (adopted == null ? "D:" : "E:") + e.getClass().getName());
            tag(mode, "error:" + (adopted == null ? "D:" : "E:") + e.getClass().getName());
            if (!e.getClass().getName().startsWith("com.legend.error.")) {
                // a raw Java exception a user's input reaches (ruling 29's second policy): named for the log
                StackTraceElement top = e.getStackTrace().length == 0 ? null : e.getStackTrace()[0];
                System.out.printf("[normalizer-answers] RAW %s %s %s at %s: %s%n", CURRENT[0], mode,
                        e.getClass().getName(), top, String.valueOf(e.getMessage()).split("\n")[0]);
            }
            return error(e);
        } finally {
            if (probe) {
                for (String k : probeHits().keySet()) {
                    count(mode + ":fallback:" + k);
                    tag(mode, "fallback:" + k);
                    System.out.printf("[normalizer-answers] FALLBACK %s %s %s%n", CURRENT[0], mode, k);
                }
            }
        }
    }

    private static String walls(Map<String, String> walls) {
        StringBuilder sb = new StringBuilder("[");
        boolean first = true;
        for (Map.Entry<String, String> e : walls.entrySet()) {
            if (!first) {
                sb.append(',');
            }
            first = false;
            sb.append('[');
            RecordCodec.string(sb, e.getKey());
            sb.append(',');
            RecordCodec.string(sb, e.getValue());
            sb.append(']');
        }
        return sb.append(']').toString();
    }

    /** {@code E TAB class TAB phase TAB element TAB message}. */
    static String error(Throwable e) {
        String phase = e instanceof LegendCompileException lce ? lce.phase().name() : "-";
        String element = e instanceof LegendCompileException lce && lce.element() != null ? lce.element() : "-";
        return "E\t" + e.getClass().getName() + "\t" + phase + "\t" + element + "\t" + e.getMessage();
    }

    // =================================================================
    // Measurements (printed, never hashed): which E families an ask reaches
    // =================================================================

    private static void count(String key) {
        STATS.merge(key, 1L, Long::sum);
    }

    private static void tag(String mode, String tag) {
        ASK_TAGS.computeIfAbsent(mode, k -> new TreeSet<>()).add(tag);
    }

    /** The families an ask's input reaches, once per ask and mode (read off the adopted model E is handed). */
    private static void measureInput(String mode, ParsedModel adopted) {
        Set<String> tags = new TreeSet<>();
        tags.add("reach:E");
        for (PackageableElement el : adopted.elements()) {
            switch (el) {
                case ClassDefinition cd -> {
                    if (!cd.derivedProperties().isEmpty()) {
                        tags.add("input:E.2 derived property");
                    }
                    if (!cd.constraints().isEmpty()) {
                        tags.add("input:E.3 constraint");
                    }
                    if (cd.properties().stream().anyMatch(p -> p.defaultValue() != null)) {
                        tags.add("input:E.3b default value");
                    }
                }
                case FunctionDefinition fd -> {
                    if (!fd.preConstraints().isEmpty() || !fd.postConstraints().isEmpty()) {
                        tags.add("input:E.3c function constraint");
                    }
                }
                case ServiceDefinition sd -> tags.add("input:E.4 service");
                case DatabaseDefinition db -> {
                    if (!db.views().isEmpty() || db.schemas().stream().anyMatch(s -> !s.views().isEmpty())) {
                        tags.add("input:E.5 view");
                    }
                }
                case CleanSheetMappingDefinition cs -> tags.add("input:E.1 clean-sheet mapping");
                case LegacyMappingDefinition lm -> {
                    tags.add("input:E.1 legacy mapping");
                    if (!lm.includes().isEmpty()) {
                        tags.add("input:include");
                        if (lm.includes().stream().anyMatch(i -> !i.substitutions().isEmpty())) {
                            tags.add("input:include with store substitution");
                        }
                    }
                    if (!lm.enumerationMappings().isEmpty()) {
                        tags.add("input:enumeration mapping");
                    }
                    for (ClassMapping cm : lm.classMappings()) {
                        measureClassMapping(cm, tags);
                    }
                    for (AssociationMapping am : lm.associationMappings()) {
                        tags.add("input:association " + am.getClass().getSimpleName());
                    }
                }
                default -> {
                }
            }
        }
        for (String tag : tags) {
            count(mode + ":" + tag);
            tag(mode, tag);
        }
    }

    private static void measureClassMapping(ClassMapping cm, Set<String> tags) {
        switch (cm) {
            case ClassMapping.Relational r -> {
                tags.add(r.sourceUrl() != null ? "input:class Relational JSON"
                        : r.mainTable() == null ? "input:class Relational (no main table)"
                        : "input:class Relational table");
                if (r.filter() != null) {
                    tags.add("input:~filter");
                }
                if (!r.groupBy().isEmpty()) {
                    tags.add("input:~groupBy");
                }
                if (r.distinct()) {
                    tags.add("input:~distinct");
                }
                if (r.aggregation() != null) {
                    tags.add("input:class AggregationAware");
                }
                if (r.extendsSetId() != null) {
                    tags.add("input:extends");
                }
                for (PropertyMapping pm : r.propertyMappings()) {
                    tags.add("input:pm " + pm.getClass().getSimpleName());
                }
            }
            case ClassMapping.Pure p -> {
                tags.add("input:class Pure (M2M)");
                if (p.filter() != null) {
                    tags.add("input:M2M ~filter");
                }
            }
            default -> tags.add("input:class " + cm.getClass().getSimpleName());
        }
    }

    /** What the answer shows: the bindings' kinds, the lifted functions' hats, the facts, E's walls. */
    private static void measureOutput(String mode, NormalizedModel nm, Map<String, String> walls, int wallsBeforeE) {
        Set<String> tags = new TreeSet<>();
        for (PackageableElement el : nm.elements()) {
            if (el instanceof MappingDefinition m) {
                for (MappingDefinition.ClassBinding cb : m.classBindings()) {
                    tags.add("answer:binding " + cb.getClass().getSimpleName());
                }
                if (!m.associationBindings().isEmpty()) {
                    tags.add("answer:association binding");
                }
                if (!m.resolvedStores().isEmpty()) {
                    tags.add("answer:resolvedStores");
                }
                MappingDefinition.NormalizationFacts f = m.facts();
                if (!f.poisons().isEmpty()) {
                    tags.add("answer:facts poisons");
                }
                if (!f.mixedUnions().isEmpty()) {
                    tags.add("answer:facts mixedUnions");
                }
                if (!f.unionKeyThreads().isEmpty()) {
                    tags.add("answer:facts unionKeyThreads");
                }
                if (!f.unionMembers().isEmpty()) {
                    tags.add("answer:facts unionMembers");
                }
                if (!f.routedTargetClasses().isEmpty()) {
                    tags.add("answer:facts routedTargetClasses");
                }
            } else if (el instanceof FunctionDefinition fd && fd.synthesizedFrom() != null) {
                tags.add("answer:lifted " + fd.synthesizedFrom().hat().name());
                String body = new RecordCodec().write(fd, FunctionDefinition.class);
                for (String name : G_REFUSED_NAMES) {
                    if (body.contains("\"function\":\"" + name + "\"")) {
                        tags.add("answer:lifted body calls '" + name + "' (G refuses)");
                    }
                }
            }
        }
        if (!nm.legacySurfaces().isEmpty()) {
            tags.add("answer:legacy surfaces");
        }
        if (walls != null && walls.size() > wallsBeforeE) {
            tags.add("answer:E walls");
        }
        for (String tag : tags) {
            count(mode + ":" + tag);
            tag(mode, tag);
        }
    }
}
