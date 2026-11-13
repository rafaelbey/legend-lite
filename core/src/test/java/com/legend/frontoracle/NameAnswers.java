// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

package com.legend.frontoracle;

import com.legend.Compiler;
import com.legend.builtin.Prelude;
import com.legend.builtin.SystemMetamodel;
import com.legend.compiler.DerivedProps;
import com.legend.compiler.KnowledgeLayer;
import com.legend.compiler.ModelBuilder;
import com.legend.compiler.NameResolver;
import com.legend.compiler.RelationalKinds;
import com.legend.door.ModelFromText;
import com.legend.error.LegendCompileException;
import com.legend.json.Json;
import com.legend.model.AssociationDefinition;
import com.legend.model.ClassDefinition;
import com.legend.model.ClassMapping;
import com.legend.model.DatabaseDefinition;
import com.legend.model.Function;
import com.legend.model.ImportScope;
import com.legend.model.LegacyMappingDefinition;
import com.legend.model.PackageableElement;
import com.legend.model.ParsedModel;
import com.legend.model.RuntimeDefinition;
import com.legend.model.SetId;
import com.legend.model.SignatureMangle;
import com.legend.parser.Dialect;
import com.legend.parser.SpecParser;
import com.legend.protocol.DerivedPropertyDefinition;
import com.legend.protocol.Multiplicity;
import com.legend.protocol.TypeExpression;
import com.legend.protocol.spec.ValueSpecification;
import java.io.BufferedReader;
import java.io.BufferedWriter;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.security.MessageDigest;
import java.util.AbstractSet;
import java.util.ArrayList;
import java.util.HexFormat;
import java.util.IdentityHashMap;
import java.util.Iterator;
import java.util.LinkedHashMap;
import java.util.LinkedHashSet;
import java.util.List;
import java.util.Map;
import java.util.Optional;
import java.util.Set;
import java.util.TreeMap;
import java.util.function.Supplier;

/**
 * THE NAMES DIFFERENTIAL'S JAVA SIDE (docs/rust-port/PHASE3.md, the names slice, 2026-11-12): what step D hands on,
 * asked of the live Java, for {@code rust/lite-names} to be held to byte for byte ({@code tests/names_differential.rs}),
 * then frozen under {@code rust/oracles/names/} before the Java goes.
 *
 * <p><b>Models</b>: every model text of the front oracle ({@code rust/oracles/front/questions.jsonl.zst}, decompressed)
 * at every dialect it is asked at ({@code ["pmcd", dialect]}) that the door parses. Its canonical text, sections in
 * order, a section equal to an earlier one written {@code =X}:
 * <ul>
 *   <li>{@code B} {@code NameResolver.resolveAlongside(parsed, bootFqns, null)} (the strict compile's),
 *       {@code T} the same tolerant (a walls map: the module build's), {@code P} {@code resolve(parsed)} strict and
 *       {@code Q} tolerant (the bare entries);</li>
 *   <li>{@code K} {@code KnowledgeLayer.adoptAssociationQualifiedProperties} strict over {@code B}'s model, {@code L}
 *       tolerant over {@code T}'s, its walls continuing {@code T}'s;</li>
 *   <li>{@code X} the index over {@code L}'s model ({@code ModelBuilder.from}) asked every question E and F ask it
 *       ({@link #index}), every record named by its place in the model (an identity label), never by a private field.</li>
 * </ul>
 * A model is {@code A} and the record codec's JSON ({@link RecordCodec}, the model differential's form), a tolerant one
 * its walls ({@code [[key,line],...]}) then its model; an error {@code E}, its class, phase, element and message.
 *
 * <p><b>Queries</b>: every distinct (model, query) pair of the identity oracle whose model and query are both {@code t:}
 * texts of the front oracle ({@code rust/oracles/identity/hits.tsv.zst}, decompressed): the query parsed at lite's
 * dialect, resolved {@code N} with no model ({@code resolveQuery}: the sectionless scope), {@code M} against the model
 * with no imports and {@code I} with the model's own union imports ({@code resolveQueryIn} over the model's universe:
 * the index of the model's {@code L} elements and the boot layer's resolved elements, asked as
 * {@code PureModelContext.resolutionUniverse} asks: the platform's names, the extensions, the elements, the function
 * ids). {@code --live} also compares {@code M} and {@code I} with the live compiled model's universe
 * ({@code Compiler.compileModel(text).resolutionUniverse()}) and counts what differs (a measurement, never frozen).
 *
 * <p><b>Stores</b> (the names follow-up, 2026-11-12: the owner's ruling moved {@code RelationalTypeInference},
 * {@code ViewSignatures}, {@code StoreCompiler.columnType} and {@code MilestoningStrategy} into the names layer, and
 * with them {@code KnowledgeLayer.columnKind}): per model ask, over the same index {@code X} asks ({@link #storesForm}),
 * a form of its own -- so the models' forms, frozen before, are unchanged: {@code m} every class's stereotypes'
 * milestoning strategy, {@code z} every table column's Pure type, {@code y} every view's signature (as
 * {@code LiftedViews} asks it: the lift's owner database), {@code r} every view column's inferred SQL type (with the
 * database's scope and the index, as {@code columnKind} and {@code ViewSignatures} ask; with no scope; with no lookups),
 * {@code n} the column kind of every column of every table or view spelling over the include closure (and one absent
 * column). {@code columnType} is package-private: asked by reflection.
 *
 * <p><b>Calls</b> (the follow-up's other half: {@code Calls} moved into the names layer): every builder of
 * {@code com.legend.compiler.spec.Calls} over a set of call shapes ({@link #callShapes}): every name its callers build
 * through it, read from them (2026-11-12), at the arities they use, and a sweep of every bare name the platform's tables
 * know at arities 0 to 8; each answer the record codec's JSON of the call.
 *
 * <p>Lines: {@code sha TAB dialect TAB hash TAB A|E} per model ask (a text the door refuses is skipped: the model
 * oracle has it), {@code querySha TAB modelSha TAB hash} per pair, {@code sha TAB dialect TAB hash} per model ask for
 * the stores, {@code shape... TAB hash} per call shape (its fields tab-separated, so the Rust side rebuilds the same
 * call from the line), the hash the SHA-256 of the form's UTF-16 units, little-endian. {@code --texts DIR} writes every
 * form too ({@code models.jsonl}, {@code queries.jsonl}, {@code stores.jsonl}, {@code calls.jsonl}) for the archive.
 *
 * <pre>
 *   java ... NameAnswers.java QUESTIONS.jsonl HITS.tsv MODELS-OUT.tsv QUERIES-OUT.tsv STORES-OUT.tsv CALLS-OUT.tsv
 *       [--texts DIR] [--limit N] [--live]
 * </pre>
 */
public final class NameAnswers {

    private NameAnswers() {
    }

    /** The boot layer's element names, as {@code Compiler.bootFqns()} unites them. */
    private static final Set<String> BOOT_FQNS = bootFqns();

    private static Set<String> bootFqns() {
        Set<String> out = new java.util.HashSet<>(SystemMetamodel.elementFqns());
        out.addAll(Prelude.elementFqns());
        return out;
    }

    public static void main(String[] args) throws Exception {
        Path questions = Path.of(args[0]);
        Path hits = Path.of(args[1]);
        Path modelsOut = Path.of(args[2]);
        Path queriesOut = Path.of(args[3]);
        Path storesOut = Path.of(args[4]);
        Path callsOut = Path.of(args[5]);
        Path texts = null;
        long limit = Long.MAX_VALUE;
        boolean live = false;
        for (int i = 6; i < args.length; i++) {
            switch (args[i]) {
                case "--texts" -> texts = Path.of(args[++i]);
                case "--limit" -> limit = Long.parseLong(args[++i]);
                case "--live" -> live = true;
                default -> throw new IllegalArgumentException("unknown option " + args[i]);
            }
        }
        Path t = texts;
        long l = limit;
        boolean lv = live;
        Exception[] failed = new Exception[1];
        Thread th = new Thread(null, () -> {
            try {
                run(questions, hits, modelsOut, queriesOut, storesOut, callsOut, t, l, lv);
            } catch (Exception e) {
                failed[0] = e;
            }
        }, "name-answers", 1L << 30);
        th.start();
        th.join();
        if (failed[0] != null) {
            throw failed[0];
        }
    }

    private static void run(Path questions, Path hits, Path modelsOut, Path queriesOut, Path storesOut, Path callsOut,
            Path texts, long limit, boolean live) throws Exception {
        MessageDigest md = MessageDigest.getInstance("SHA-256");
        RecordCodec codec = new RecordCodec();
        Map<String, String> textBySha = new LinkedHashMap<>();
        long asks = 0;
        long answered = 0;
        TreeMap<String, Long> kinds = new TreeMap<>();
        if (texts != null) {
            Files.createDirectories(texts);
        }
        try (BufferedReader in = Files.newBufferedReader(questions, StandardCharsets.UTF_8);
                BufferedWriter w = Files.newBufferedWriter(modelsOut, StandardCharsets.UTF_8);
                BufferedWriter sw = Files.newBufferedWriter(storesOut, StandardCharsets.UTF_8);
                BufferedWriter tw = texts == null ? null
                        : Files.newBufferedWriter(texts.resolve("models.jsonl"), StandardCharsets.UTF_8);
                BufferedWriter stw = texts == null ? null
                        : Files.newBufferedWriter(texts.resolve("stores.jsonl"), StandardCharsets.UTF_8)) {
            for (String line = in.readLine(); line != null; line = in.readLine()) {
                Json.Obj o = (Json.Obj) Json.parse(line);
                String sha = ((Json.Str) o.get("sha")).value();
                String text = ((Json.Str) o.get("text")).value();
                textBySha.put(sha, text);
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
                    String[] stores = new String[1];
                    String form = modelForm(parsed, codec, kinds, stores);
                    String hash = hash(md, form);
                    answered++;
                    w.write(sha + "\t" + dialect.name() + "\t" + hash + "\n");
                    sw.write(sha + "\t" + dialect.name() + "\t" + hash(md, stores[0]) + "\n");
                    if (tw != null) {
                        StringBuilder j = new StringBuilder("{\"sha\":\"").append(sha).append("\",\"dialect\":\"")
                                .append(dialect.name()).append("\",\"form\":");
                        RecordCodec.string(j, form);
                        tw.write(j.append("}\n").toString());
                        StringBuilder s = new StringBuilder("{\"sha\":\"").append(sha).append("\",\"dialect\":\"")
                                .append(dialect.name()).append("\",\"form\":");
                        RecordCodec.string(s, stores[0]);
                        stw.write(s.append("}\n").toString());
                    }
                }
            }
        }
        System.out.printf("[name-answers] %d model asks answered; sections %s; stores %s%n", answered, kinds,
                STORE_LINES);

        // the query pairs: the identity oracle's keys, model and query both t: texts of the front oracle
        LinkedHashSet<List<String>> pairs = new LinkedHashSet<>();
        try (BufferedReader in = Files.newBufferedReader(hits, StandardCharsets.UTF_8)) {
            for (String line = in.readLine(); line != null; line = in.readLine()) {
                if (line.startsWith("#")) {
                    continue;
                }
                String[] f = line.split("\t", -1);
                if (f.length < 3 || !f[1].startsWith("t:") || !f[2].startsWith("t:")) {
                    continue;
                }
                String m = f[1].substring(2);
                String q = f[2].substring(2);
                if (textBySha.containsKey(m) && textBySha.containsKey(q)) {
                    pairs.add(List.of(q, m));
                }
            }
        }
        Map<String, Optional<ModelUniverse>> universes = new LinkedHashMap<>();
        Map<String, Optional<Set<String>>> liveUniverses = new LinkedHashMap<>();
        long[] liveCounts = new long[4];
        long n = 0;
        try (BufferedWriter w = Files.newBufferedWriter(queriesOut, StandardCharsets.UTF_8);
                BufferedWriter tw = texts == null ? null
                        : Files.newBufferedWriter(texts.resolve("queries.jsonl"), StandardCharsets.UTF_8)) {
            for (List<String> pair : pairs) {
                if (n >= limit) {
                    break;
                }
                n++;
                String q = pair.get(0);
                String m = pair.get(1);
                Optional<ModelUniverse> u = universes.computeIfAbsent(m, k -> universe(textBySha.get(k)));
                String form = queryForm(textBySha.get(q), u, codec);
                w.write(q + "\t" + m + "\t" + hash(md, form) + "\n");
                if (tw != null) {
                    StringBuilder j = new StringBuilder("{\"query\":\"").append(q).append("\",\"model\":\"")
                            .append(m).append("\",\"form\":");
                    RecordCodec.string(j, form);
                    tw.write(j.append("}\n").toString());
                }
                if (live && u.isPresent()) {
                    Optional<Set<String>> lu = liveUniverses.computeIfAbsent(m, k -> {
                        try {
                            return Optional.of(Compiler.compileModel(textBySha.get(k)).resolutionUniverse());
                        } catch (RuntimeException e) {
                            return Optional.empty();
                        }
                    });
                    if (lu.isEmpty()) {
                        liveCounts[3]++;
                    } else {
                        String liveForm = queryForm(textBySha.get(q), Optional.of(new ModelUniverse(lu.get(),
                                u.get().imports)), codec);
                        liveCounts[liveForm.equals(form) ? 0 : 1]++;
                        if (!liveForm.equals(form) && liveCounts[1] <= 20) {
                            System.out.println("[name-answers] live universe differs: query " + q + " model " + m);
                        }
                    }
                }
            }
        }
        System.out.printf("[name-answers] %d query pairs (%d models)%n", n, universes.size());
        if (live) {
            System.out.printf("[name-answers] live universe: %d same, %d differ, %d models do not compile%n",
                    liveCounts[0], liveCounts[1], liveCounts[3]);
        }

        // the call shapes, each asked of its builder
        TreeMap<String, Long> builders = new TreeMap<>();
        try (BufferedWriter w = Files.newBufferedWriter(callsOut, StandardCharsets.UTF_8);
                BufferedWriter tw = texts == null ? null
                        : Files.newBufferedWriter(texts.resolve("calls.jsonl"), StandardCharsets.UTF_8)) {
            for (List<String> shape : callShapes()) {
                String key = String.join("\t", shape);
                String form = callForm(shape, codec);
                w.write(key + "\t" + hash(md, form) + "\n");
                builders.merge(shape.get(0), 1L, Long::sum);
                if (tw != null) {
                    StringBuilder j = new StringBuilder("{\"shape\":");
                    RecordCodec.string(j, key);
                    j.append(",\"form\":");
                    RecordCodec.string(j, form);
                    tw.write(j.append("}\n").toString());
                }
            }
        }
        System.out.printf("[name-answers] call shapes %s%n", builders);
    }

    private static String hash(MessageDigest md, String form) {
        return HexFormat.of().formatHex(md.digest(form.getBytes(StandardCharsets.UTF_16LE)));
    }

    // =================================================================
    // Models
    // =================================================================

    /** A model resolved, adopted and indexed: what step D hands on; {@code stores[0]} the stores form over the same
     *  index ({@code -} when there is none). */
    static String modelForm(ParsedModel parsed, RecordCodec codec, TreeMap<String, Long> kinds, String[] stores) {
        stores[0] = "-";
        StringBuilder f = new StringBuilder();
        ParsedModel[] b = new ParsedModel[1];
        String bText = strict(() -> NameResolver.resolveAlongside(parsed, BOOT_FQNS, null), codec, b);
        f.append("B\t").append(bText);
        Map<String, String> tWalls = new LinkedHashMap<>();
        ParsedModel[] t = new ParsedModel[1];
        String tText = tolerant(() -> NameResolver.resolveAlongside(parsed, BOOT_FQNS, tWalls), tWalls, codec, t);
        f.append("\nT\t").append(same(tText, "B", bText));
        ParsedModel[] ignored = new ParsedModel[1];
        String pText = strict(() -> NameResolver.resolve(parsed), codec, ignored);
        f.append("\nP\t").append(same(pText, "B", bText));
        Map<String, String> qWalls = new LinkedHashMap<>();
        String qText = tolerant(() -> NameResolver.resolve(parsed, qWalls), qWalls, codec, ignored);
        f.append("\nQ\t").append(same(qText, "T", tText));
        String kText = b[0] == null ? "-"
                : strict(() -> KnowledgeLayer.adoptAssociationQualifiedProperties(b[0], null), codec, ignored);
        f.append("\nK\t").append(same(kText, "B", bText));
        ParsedModel[] l = new ParsedModel[1];
        String lText;
        if (t[0] == null) {
            lText = "-";
        } else {
            Map<String, String> lWalls = new LinkedHashMap<>(tWalls);
            lText = tolerant(() -> KnowledgeLayer.adoptAssociationQualifiedProperties(t[0], lWalls), lWalls, codec, l);
        }
        f.append("\nL\t").append(same(lText, "T", tText));
        f.append("\nX");
        if (l[0] != null) {
            ModelBuilder ix;
            try {
                ix = ModelBuilder.from(l[0]);
            } catch (RuntimeException e) {
                ix = null;
                f.append('\t').append(error(e));
            }
            if (ix != null) {
                f.append('\n');
                index(f, l[0], ix, codec);
                stores[0] = storesForm(l[0], ix);
            }
        }
        kinds.merge(bText.startsWith("A") ? "B:A" : "B:E", 1L, Long::sum);
        kinds.merge(tText.startsWith("A") || tText.startsWith("[") ? "T:A" : "T:E", 1L, Long::sum);
        return f.toString();
    }

    private static String same(String text, String label, String earlier) {
        return text.equals(earlier) ? "=" + label : text;
    }

    private static String strict(Supplier<ParsedModel> s, RecordCodec codec, ParsedModel[] out) {
        try {
            ParsedModel m = s.get();
            out[0] = m;
            return "A\t" + codec.write(m, ParsedModel.class);
        } catch (RuntimeException e) {
            return error(e);
        }
    }

    private static String tolerant(Supplier<ParsedModel> s, Map<String, String> walls, RecordCodec codec,
            ParsedModel[] out) {
        try {
            ParsedModel m = s.get();
            out[0] = m;
            if (walls.isEmpty()) {
                return "A\t" + codec.write(m, ParsedModel.class);
            }
            return walls(walls) + "\tA\t" + codec.write(m, ParsedModel.class);
        } catch (RuntimeException e) {
            return error(e);
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
    static String error(RuntimeException e) {
        String phase = e instanceof LegendCompileException lce ? lce.phase().name() : "-";
        String element = e instanceof LegendCompileException lce && lce.element() != null ? lce.element() : "-";
        return "E\t" + e.getClass().getName() + "\t" + phase + "\t" + element + "\t" + e.getMessage();
    }

    private static String fail(RuntimeException e) {
        return "!" + e.getClass().getName() + ": " + e.getMessage();
    }

    // =================================================================
    // The index, asked every question E and F ask it
    // =================================================================

    /** Every record the model holds, by its place: an element {@code e<i>}, a runtime's inline connection
     *  {@code e<i>.c<j>}, an association's ends {@code e<i>.1}, {@code .2}, a database's tables, views, tabular
     *  functions ({@code .t}, {@code .v}, {@code .f}, a schema's {@code .s<j>.t<k>} ...), joins {@code .j}, filters
     *  {@code .g}, multi-grain filters {@code .m}, a legacy mapping's class mappings {@code .cm}. First place wins (a
     *  schema's table is the flat list's). A record the model does not hold (a native class) is {@code n}. */
    private static IdentityHashMap<Object, String> labels(ParsedModel m) {
        IdentityHashMap<Object, String> labels = new IdentityHashMap<>();
        List<PackageableElement> els = m.elements();
        for (int i = 0; i < els.size(); i++) {
            labels.putIfAbsent(els.get(i), "e" + i);
        }
        for (int i = 0; i < els.size(); i++) {
            String e = "e" + i;
            switch (els.get(i)) {
                case RuntimeDefinition rd -> {
                    for (int j = 0; j < rd.inlineConnections().size(); j++) {
                        labels.putIfAbsent(rd.inlineConnections().get(j), e + ".c" + j);
                    }
                }
                case AssociationDefinition ad -> {
                    labels.putIfAbsent(ad.property1(), e + ".1");
                    labels.putIfAbsent(ad.property2(), e + ".2");
                }
                case DatabaseDefinition db -> {
                    for (int j = 0; j < db.tables().size(); j++) {
                        labels.putIfAbsent(db.tables().get(j), e + ".t" + j);
                    }
                    for (int j = 0; j < db.views().size(); j++) {
                        labels.putIfAbsent(db.views().get(j), e + ".v" + j);
                    }
                    for (int j = 0; j < db.tabularFunctions().size(); j++) {
                        labels.putIfAbsent(db.tabularFunctions().get(j), e + ".f" + j);
                    }
                    for (int j = 0; j < db.schemas().size(); j++) {
                        DatabaseDefinition.SchemaDefinition s = db.schemas().get(j);
                        for (int k = 0; k < s.tables().size(); k++) {
                            labels.putIfAbsent(s.tables().get(k), e + ".s" + j + ".t" + k);
                        }
                        for (int k = 0; k < s.views().size(); k++) {
                            labels.putIfAbsent(s.views().get(k), e + ".s" + j + ".v" + k);
                        }
                        for (int k = 0; k < s.tabularFunctions().size(); k++) {
                            labels.putIfAbsent(s.tabularFunctions().get(k), e + ".s" + j + ".f" + k);
                        }
                    }
                    for (int j = 0; j < db.joins().size(); j++) {
                        labels.putIfAbsent(db.joins().get(j), e + ".j" + j);
                    }
                    for (int j = 0; j < db.filters().size(); j++) {
                        labels.putIfAbsent(db.filters().get(j), e + ".g" + j);
                    }
                    for (int j = 0; j < db.multiGrainFilters().size(); j++) {
                        labels.putIfAbsent(db.multiGrainFilters().get(j), e + ".m" + j);
                    }
                }
                case LegacyMappingDefinition lm -> {
                    for (int j = 0; j < lm.classMappings().size(); j++) {
                        labels.putIfAbsent(lm.classMappings().get(j), e + ".cm" + j);
                    }
                }
                default -> {
                }
            }
        }
        return labels;
    }

    private static String label(IdentityHashMap<Object, String> labels, Object o) {
        return o == null ? "-" : labels.getOrDefault(o, "n");
    }

    private static String labels(IdentityHashMap<Object, String> labels, java.util.stream.Stream<?> os) {
        return String.join(",", os.map(o -> label(labels, o)).toList());
    }

    private static String bool(boolean b) {
        return b ? "1" : "0";
    }

    static void index(StringBuilder f, ParsedModel m, ModelBuilder ix, RecordCodec codec) {
        IdentityHashMap<Object, String> labels = labels(m);
        for (Map.Entry<String, String> d : ix.duplicateElements().entrySet()) {
            f.append("d\t").append(d.getKey()).append('\t').append(d.getValue()).append('\n');
        }
        f.append("o\tclasses\t").append(labels(labels, ix.classes())).append('\n');
        f.append("o\tassociations\t").append(labels(labels, ix.associations())).append('\n');
        f.append("o\tfunctions\t").append(labels(labels, ix.functions())).append('\n');
        f.append("o\tdatabases\t").append(labels(labels, ix.databases())).append('\n');
        f.append("o\tdataElements\t").append(labels(labels, ix.dataElements())).append('\n');
        f.append("o\tmappings\t").append(labels(labels, ix.mappings())).append('\n');
        f.append("o\tlegacyMappings\t").append(labels(labels, ix.legacyMappings())).append('\n');
        f.append("o\tenums\t").append(labels(labels, ix.enums())).append('\n');
        f.append("o\tmeasures\t").append(labels(labels, ix.measures())).append('\n');
        f.append("o\truntimes\t").append(labels(labels, ix.runtimes())).append('\n');
        f.append("o\tservices\t").append(labels(labels, ix.services())).append('\n');
        f.append("o\tdataSpaces\t").append(labels(labels, ix.dataSpaces())).append('\n');
        f.append("o\tconnections\t").append(labels(labels, ix.connections())).append('\n');
        f.append("x\t").append(String.join(",", ix.primitiveExtensionFqns())).append('\n');

        KnowledgeLayer k = ix.knowledge();
        List<PackageableElement> els = m.elements();
        LinkedHashSet<String> keys = new LinkedHashSet<>();
        for (PackageableElement el : els) {
            keys.add(el.qualifiedName());
        }
        for (PackageableElement el : els) {
            if (el instanceof RuntimeDefinition rd) {
                rd.inlineConnections().forEach(c -> keys.add(c.qualifiedName()));
            }
        }
        for (PackageableElement el : els) {
            switch (el) {
                case ClassDefinition cd -> cd.superClasses().forEach(s -> {
                    String raw = TypeExpression.rawClassName(s);
                    if (raw != null) {
                        keys.add(raw);
                    }
                });
                case AssociationDefinition ad -> {
                    for (var end : List.of(ad.property1(), ad.property2())) {
                        String raw = TypeExpression.rawClassName(end.targetClass());
                        if (raw != null) {
                            keys.add(raw);
                        }
                    }
                }
                case DatabaseDefinition db -> keys.addAll(db.includes());
                default -> {
                }
            }
        }
        keys.add("absent::Name");
        keys.add("");
        for (String key : keys) {
            f.append("k\t").append(key);
            f.append('\t').append(label(labels, ix.findClass(key).orElse(null)));
            f.append('\t').append(label(labels, ix.findAssociation(key).orElse(null)));
            f.append('\t').append(label(labels, ix.findEnum(key).orElse(null)));
            f.append('\t').append(label(labels, ix.findProfile(key).orElse(null)));
            f.append('\t').append(label(labels, ix.findMeasure(key).orElse(null)));
            f.append('\t').append(label(labels, ix.findDatabase(key).orElse(null)));
            f.append('\t').append(label(labels, ix.findMapping(key).orElse(null)));
            f.append('\t').append(label(labels, ix.findLegacyMapping(key).orElse(null)));
            f.append('\t').append(label(labels, ix.findService(key).orElse(null)));
            f.append('\t').append(label(labels, ix.findData(key).orElse(null)));
            f.append('\t').append(label(labels, ix.findDataSpace(key).orElse(null)));
            f.append('\t').append(label(labels, ix.findRuntime(key).orElse(null)));
            f.append('\t').append(label(labels, ix.findConnection(key).orElse(null)));
            f.append('\t').append(bool(ix.isModelConnection(key)));
            f.append('\t').append(labels(labels, ix.findFunction(key).stream()));
            f.append('\t').append(bool(ix.hasElement(key)));
            f.append('\t').append(bool(ix.hasDatabaseExact(key)));
            f.append('\t').append(ix.findPrimitiveExtension(key).map(Enum::name).orElse("-"));
            f.append('\t').append(String.join(",", ix.primitiveTypeVariables(key).stream()
                    .map(com.legend.protocol.ParameterDefinition::name).toList()));
            f.append('\t').append(String.join(",", ix.directSubclasses(key)));
            f.append('\n');
            // the knowledge kernel
            Optional<ClassDefinition> cdef = k.classDef(key);
            f.append("c\t").append(key);
            f.append('\t').append(label(labels, cdef.orElse(null)));
            f.append('\t').append(label(labels, k.hierarchyClass(key).orElse(null)));
            f.append('\t').append(cdef.map(c -> String.join(",", k.superClassFqns(c))).orElse("-"));
            f.append('\t').append(attempt(() -> String.join(",", k.ancestorsAndSelf(key))));
            f.append('\t').append(String.join(",", k.subtree(key)));
            f.append('\t').append(String.join(",", k.directSubtypes(key)));
            f.append('\t').append(attempt(() -> labels(labels, k.lineage(key).stream())));
            f.append('\n');
            StringBuilder parents = new StringBuilder();
            for (String p : keys) {
                if (k.isSubtype(key, p)) {
                    if (!parents.isEmpty()) {
                        parents.append(',');
                    }
                    parents.append(p);
                }
            }
            f.append("s\t").append(key).append('\t').append(parents).append('\n');
            // properties over the hierarchy, for a class the calculus knows
            ClassDefinition hc = k.hierarchyClass(key).orElse(null);
            if (hc != null) {
                for (String name : probeNames(key, k, els)) {
                    f.append("p\t").append(key).append('\t').append(name);
                    f.append('\t').append(attempt(() -> json(codec, k.propertyType(hc, name),
                            TypeExpression.class)));
                    f.append('\t').append(attempt(() -> json(codec, k.propertyDef(hc, name),
                            ClassDefinition.PropertyDefinition.class)));
                    f.append('\t').append(attempt(() -> json(codec, k.propertyMultiplicity(hc, name),
                            Multiplicity.class)));
                    f.append('\t').append(attempt(() -> json(codec, k.derivedInline(hc, name),
                            DerivedPropertyDefinition.class)));
                    f.append('\t').append(label(labels, ix.findAssociationEnd(key, name).orElse(null)));
                    f.append('\t').append(attempt(() -> label(labels, ix.findAssociationOf(key, name).orElse(null))));
                    f.append('\n');
                }
            }
        }
        // the derived properties lifted (DerivedProps.lift: E's and F's one synthesis)
        for (int i = 0; i < els.size(); i++) {
            if (els.get(i) instanceof ClassDefinition cd) {
                for (DerivedPropertyDefinition dp : cd.derivedProperties()) {
                    f.append("l\te").append(i).append('\t').append(dp.name()).append('\t')
                            .append(attempt(() -> json(codec, DerivedProps.lift(cd, dp),
                                    com.legend.model.FunctionDefinition.class)))
                            .append('\n');
                }
            }
        }
        // the stores over the include closure
        LinkedHashSet<String> dbs = new LinkedHashSet<>();
        LinkedHashSet<String> spellings = new LinkedHashSet<>();
        LinkedHashSet<String> joins = new LinkedHashSet<>();
        LinkedHashSet<String> filters = new LinkedHashSet<>();
        for (PackageableElement el : els) {
            if (el instanceof DatabaseDefinition db) {
                dbs.add(db.qualifiedName());
                dbs.addAll(db.includes());
                db.tables().forEach(x -> {
                    spellings.add(x.name());
                    spellings.add("default." + x.name());
                });
                db.views().forEach(x -> spellings.add(x.name()));
                db.tabularFunctions().forEach(x -> spellings.add(x.name()));
                for (DatabaseDefinition.SchemaDefinition s : db.schemas()) {
                    s.tables().forEach(x -> spellings.add(s.name() + "." + x.name()));
                    s.views().forEach(x -> spellings.add(s.name() + "." + x.name()));
                    s.tabularFunctions().forEach(x -> spellings.add(s.name() + "." + x.name()));
                }
                db.joins().forEach(x -> joins.add(x.name()));
                db.filters().forEach(x -> filters.add(x.name()));
                db.multiGrainFilters().forEach(x -> filters.add(x.name()));
            }
        }
        dbs.add("absent::Db");
        spellings.add("absent_T");
        joins.add("absent_J");
        filters.add("absent_F");
        for (String db : dbs) {
            List<ModelBuilder.IndexedView> views = ix.viewsOf(db);
            f.append("v\t").append(db).append('\t').append(String.join(",", views.stream()
                    .map(iv -> label(labels, iv.definition()) + "@" + iv.lift().spelling()).toList())).append('\n');
            for (ModelBuilder.IndexedView iv : views) {
                f.append("w\t").append(db).append('\t').append(label(labels, iv.definition()))
                        .append('\t').append(attempt(() -> ix.viewMainTable(db, iv.definition())))
                        .append('\t').append(attempt(() -> iv.lift().fqn())).append('\n');
            }
            for (String sp : spellings) {
                Optional<DatabaseDefinition.TableDefinition> td = ix.findTableDefinition(db, sp);
                f.append("t\t").append(db).append('\t').append(sp);
                f.append('\t').append(label(labels, td.orElse(null)));
                f.append('\t').append(label(labels, ix.findOwnTableDefinition(db, sp).orElse(null)));
                f.append('\t').append(label(labels, ix.findView(db, sp).orElse(null)));
                f.append('\t').append(ix.viewLift(db, sp).map(vl -> vl.ownerDb() + "/" + vl.spelling()).orElse("-"));
                f.append('\t').append(label(labels, k.table(db, sp).orElse(null)));
                f.append('\n');
                if (td.isPresent()) {
                    List<DatabaseDefinition.ColumnDefinition> cols = td.get().columns();
                    for (DatabaseDefinition.ColumnDefinition c : cols) {
                        DatabaseDefinition.ColumnDefinition hit = k.column(db, sp, c.name()).orElse(null);
                        int at = -1;
                        for (int i = 0; i < cols.size(); i++) {
                            if (cols.get(i) == hit) {
                                at = i;
                                break;
                            }
                        }
                        f.append("u\t").append(db).append('\t').append(sp).append('\t').append(c.name())
                                .append('\t').append(at).append('\t')
                                .append(hit == null ? "-" : RelationalKinds.pureKindOf(hit.dataType())).append('\n');
                    }
                }
            }
            for (String j : joins) {
                f.append("j\t").append(db).append('\t').append(j).append('\t')
                        .append(label(labels, ix.findJoin(db, j).orElse(null))).append('\n');
            }
            for (String g : filters) {
                f.append("g\t").append(db).append('\t').append(g).append('\t')
                        .append(label(labels, ix.findFilter(db, g).orElse(null))).append('\n');
            }
        }
        // the legacy sets by effective id
        LinkedHashSet<String> setIds = new LinkedHashSet<>();
        for (PackageableElement el : els) {
            if (el instanceof LegacyMappingDefinition lm) {
                for (ClassMapping cm : lm.classMappings()) {
                    setIds.add(SetId.of(cm));
                }
            }
        }
        setIds.add("absent_set");
        for (String id : setIds) {
            f.append("q\t").append(id).append('\t').append(String.join(",", ix.legacySetsWithId(id).stream()
                    .map(s -> label(labels, s.defining()) + "/" + label(labels, s.set())).toList())).append('\n');
        }
        // the section scopes
        for (PackageableElement el : els) {
            String key = ParsedModel.keyOf(el);
            f.append("i\t").append(key).append('\t').append(String.join(",", ix.importsOf(key).wildcards()))
                    .append('\n');
        }
        // the functions by signature id
        LinkedHashSet<String> ids = new LinkedHashSet<>();
        for (PackageableElement el : els) {
            if (el instanceof Function fn) {
                ids.add(SignatureMangle.mangle(fn));
            }
        }
        ids.add("absent_id");
        for (String id : ids) {
            f.append("f\t").append(id).append('\t').append(labels(labels, ix.findFunctionById(id).stream()))
                    .append('\t').append(bool(ix.hasFunctionId(id))).append('\n');
        }
    }

    /** The property names asked of a class: every name its ancestors declare (stored, derived) or have injected by an
     *  association end, nearest first, then one no class has. */
    private static List<String> probeNames(String key, KnowledgeLayer k, List<PackageableElement> els) {
        LinkedHashSet<String> names = new LinkedHashSet<>();
        List<String> line;
        try {
            line = new ArrayList<>(k.ancestorsAndSelf(key));
        } catch (RuntimeException e) {
            line = List.of(key);
        }
        for (String a : line) {
            k.hierarchyClass(a).ifPresent(d -> {
                d.properties().forEach(p -> names.add(p.name()));
                d.derivedProperties().forEach(p -> names.add(p.name()));
            });
            for (PackageableElement el : els) {
                if (el instanceof AssociationDefinition ad) {
                    if (ad.property2().targetClass() instanceof TypeExpression.NameRef n && n.name().equals(a)) {
                        names.add(ad.property1().propertyName());
                    }
                    if (ad.property1().targetClass() instanceof TypeExpression.NameRef n && n.name().equals(a)) {
                        names.add(ad.property2().propertyName());
                    }
                }
            }
        }
        names.add("absent$prop");
        return List.copyOf(names);
    }

    /** A small record's JSON, by a codec of its own: the shared codec's identity table keeps the capacity of the
     *  largest model it wrote, and clearing it per small write made one model's index minutes long. */
    private static String json(RecordCodec unused, Object o, Class<?> declared) {
        return o == null ? "-" : new RecordCodec().write(o, declared);
    }

    private static String attempt(Supplier<String> s) {
        try {
            return s.get();
        } catch (RuntimeException e) {
            return fail(e);
        }
    }

    // =================================================================
    // The stores: the helpers E calls, moved into the names layer (2026-11-12)
    // =================================================================

    /** The stores form's lines, by kind, over the whole run (printed at the end). */
    private static final TreeMap<String, Long> STORE_LINES = new TreeMap<>();

    /** {@link #attempt}, a stack overflow written as one (the type inference has no cycle guard: a computed view
     *  column over a cyclic view chain recurses until the stack is gone; the Rust side cannot answer such a model, and
     *  its marker says which). */
    private static String attemptDeep(Supplier<String> s) {
        try {
            return s.get();
        } catch (RuntimeException e) {
            return fail(e);
        } catch (StackOverflowError e) {
            return "!java.lang.StackOverflowError";
        }
    }

    private static void storeLine(StringBuilder f, String kind) {
        f.append(kind);
        STORE_LINES.merge(kind, 1L, Long::sum);
    }

    /** {@code StoreCompiler.columnType}, package-private: by reflection (no production code changes). */
    private static final java.lang.reflect.Method COLUMN_TYPE = columnTypeMethod();

    private static java.lang.reflect.Method columnTypeMethod() {
        try {
            java.lang.reflect.Method m = com.legend.compiler.element.StoreCompiler.class.getDeclaredMethod(
                    "columnType", com.legend.model.RelationalDataType.class);
            m.setAccessible(true);
            return m;
        } catch (ReflectiveOperationException e) {
            throw new IllegalStateException(e);
        }
    }

    private static com.legend.compiler.element.type.Type columnType(com.legend.model.RelationalDataType dt) {
        try {
            return (com.legend.compiler.element.type.Type) COLUMN_TYPE.invoke(null, dt);
        } catch (java.lang.reflect.InvocationTargetException e) {
            if (e.getCause() instanceof RuntimeException re) {
                throw re;
            }
            throw new IllegalStateException(e.getCause());
        } catch (IllegalAccessException e) {
            throw new IllegalStateException(e);
        }
    }

    /** A type as its variant's letter and its {@code typeName()}. */
    static String typeText(com.legend.compiler.element.type.Type t) {
        String kind = switch (t) {
            case com.legend.compiler.element.type.Type.Primitive p -> "P";
            case com.legend.compiler.element.type.Type.PrecisionDecimal d -> "D";
            case com.legend.compiler.element.type.Type.ClassType c -> "C";
            case com.legend.compiler.element.type.Type.EnumType e -> "E";
            default -> "?";
        };
        return kind + ":" + t.typeName();
    }

    /** A view's signature: each column's {@code name}, its type's {@link #typeText} and its multiplicity's text. */
    static String signatureText(com.legend.compiler.element.type.Type.RelationType r) {
        StringBuilder sb = new StringBuilder();
        for (var c : r.columns()) {
            if (!sb.isEmpty()) {
                sb.append(',');
            }
            sb.append(c.name()).append('=').append(typeText(c.type())).append(c.multiplicity().text());
        }
        return sb.append('|').append(r.dynamicColumns().size()).toString();
    }

    private static String dataTypeText(com.legend.model.RelationalDataType dt) {
        return dt == null ? "-" : new RecordCodec().write(dt, com.legend.model.RelationalDataType.class);
    }

    /** What the store and class helpers E calls answer over the index {@code X} asks. */
    static String storesForm(ParsedModel m, ModelBuilder ix) {
        StringBuilder f = new StringBuilder();
        IdentityHashMap<Object, String> labels = labels(m);
        KnowledgeLayer k = ix.knowledge();
        List<PackageableElement> els = m.elements();
        // the milestoning strategy of every class's stereotypes
        for (int i = 0; i < els.size(); i++) {
            if (els.get(i) instanceof ClassDefinition cd) {
                for (int j = 0; j < cd.stereotypes().size(); j++) {
                    var st = cd.stereotypes().get(j);
                    com.legend.compiler.element.MilestoningStrategy s = com.legend.compiler.element.MilestoningStrategy
                            .ofStereotypeOrNull(st.profileName(), st.stereotypeName());
                    storeLine(f, "m");
                    f.append("\te").append(i).append('\t').append(j).append('\t').append(s == null ? "-"
                            : s.name() + "/" + bool(s.has(com.legend.compiler.element.MilestoningStrategy.Dimension
                                    .BUSINESS)) + bool(s.has(com.legend.compiler.element.MilestoningStrategy
                                            .Dimension.PROCESSING))).append('\n');
                }
            }
        }
        // every table column's Pure type
        for (PackageableElement el : els) {
            if (el instanceof DatabaseDefinition db) {
                List<DatabaseDefinition.TableDefinition> tables = new ArrayList<>(db.tables());
                for (DatabaseDefinition.SchemaDefinition s : db.schemas()) {
                    tables.addAll(s.tables());
                }
                for (DatabaseDefinition.TableDefinition td : tables) {
                    for (DatabaseDefinition.ColumnDefinition c : td.columns()) {
                        storeLine(f, "z");
                        f.append('\t').append(label(labels, td)).append('\t').append(c.name()).append('\t')
                                .append(attempt(() -> typeText(columnType(c.dataType())))).append('\n');
                    }
                }
            }
        }
        // the databases and spellings the index is asked (as index() gathers them)
        LinkedHashSet<String> dbs = new LinkedHashSet<>();
        LinkedHashSet<String> spellings = new LinkedHashSet<>();
        for (PackageableElement el : els) {
            if (el instanceof DatabaseDefinition db) {
                dbs.add(db.qualifiedName());
                dbs.addAll(db.includes());
                db.tables().forEach(x -> {
                    spellings.add(x.name());
                    spellings.add("default." + x.name());
                });
                db.views().forEach(x -> spellings.add(x.name()));
                db.tabularFunctions().forEach(x -> spellings.add(x.name()));
                for (DatabaseDefinition.SchemaDefinition s : db.schemas()) {
                    s.tables().forEach(x -> spellings.add(s.name() + "." + x.name()));
                    s.views().forEach(x -> spellings.add(s.name() + "." + x.name()));
                    s.tabularFunctions().forEach(x -> spellings.add(s.name() + "." + x.name()));
                }
            }
        }
        dbs.add("absent::Db");
        spellings.add("absent_T");
        for (String db : dbs) {
            DatabaseDefinition scope = ix.findDatabase(db).orElse(null);
            for (ModelBuilder.IndexedView iv : ix.viewsOf(db)) {
                DatabaseDefinition.ViewDefinition view = iv.definition();
                String vl = label(labels, view);
                storeLine(f, "y");
                f.append('\t').append(db).append('\t').append(vl).append('\t')
                        .append(attemptDeep(() -> signatureText(com.legend.compiler.element.ViewSignatures.of(ix,
                                iv.lift().ownerDb(), view)))).append('\n');
                List<DatabaseDefinition.ViewDefinition.ViewColumnMapping> cms = view.columnMappings();
                for (int c = 0; c < cms.size(); c++) {
                    com.legend.model.RelationalOperation expr = cms.get(c).expression();
                    storeLine(f, "r");
                    f.append('\t').append(db).append('\t').append(vl).append('\t').append(c)
                            .append('\t').append(attemptDeep(() -> dataTypeText(
                                    com.legend.compiler.element.RelationalTypeInference.infer(expr, scope, ix))))
                            .append('\t').append(attemptDeep(() -> dataTypeText(
                                    com.legend.compiler.element.RelationalTypeInference.infer(expr, null, ix))))
                            .append('\t').append(attemptDeep(() -> dataTypeText(
                                    com.legend.compiler.element.RelationalTypeInference.infer(expr, scope, null))))
                            .append('\n');
                }
            }
            for (String sp : spellings) {
                List<String> names = new ArrayList<>();
                Optional<DatabaseDefinition.TableDefinition> td = ix.findTableDefinition(db, sp);
                if (td.isPresent()) {
                    td.get().columns().forEach(c -> names.add(c.name()));
                } else {
                    ix.findView(db, sp).ifPresent(v -> v.columnMappings().forEach(c -> names.add(c.name())));
                }
                names.add("absent_C");
                for (String name : names) {
                    storeLine(f, "n");
                    f.append('\t').append(db).append('\t').append(sp).append('\t').append(name).append('\t')
                            .append(attemptDeep(() -> {
                                String kind = k.columnKind(db, sp, name);
                                return kind == null ? "-" : kind;
                            })).append('\n');
                }
            }
        }
        return f.toString();
    }

    // =================================================================
    // The call builder (2026-11-12)
    // =================================================================

    /** A position for an original call (its carry-through pinned). */
    private static final com.legend.protocol.SourceInfo ORIGINAL_POS =
            new com.legend.protocol.SourceInfo("src", 1, 2, 3, 4);

    /** {@code n} arguments: {@code CString}s {@code <prefix>0 ...}. */
    private static List<ValueSpecification> args(String prefix, int n) {
        List<ValueSpecification> out = new ArrayList<>();
        for (int i = 0; i < n; i++) {
            out.add(new com.legend.protocol.spec.CString(prefix + i));
        }
        return out;
    }

    private static List<String> refs(String joined) {
        return joined.isEmpty() ? List.of() : List.of(joined.split(",", -1));
    }

    /** An original call for {@code like} and {@code renamed}: its name, arity, record and written forms ({@code p} a
     *  position, {@code c} propertyCall, {@code g} grouped, {@code x} infix, {@code i} island, {@code f} an fControl,
     *  {@code o} an ownerClass). */
    private static com.legend.protocol.spec.AppliedFunction original(String fn, int n, String referents,
            String flags) {
        return new com.legend.protocol.spec.AppliedFunction(fn, args("a", n), refs(referents),
                flags.contains("p") ? ORIGINAL_POS : null, flags.contains("c"), flags.contains("g"),
                flags.contains("x"), flags.contains("i"), flags.contains("f") ? "fctl" : null,
                flags.contains("o") ? "my::Own" : null);
    }

    /** One shape asked of its builder: {@code A} and the call's JSON, or the error. */
    static String callForm(List<String> s, RecordCodec codec) {
        try {
            com.legend.protocol.spec.AppliedFunction af = switch (s.get(0)) {
                case "platform" -> com.legend.compiler.spec.Calls.platform(s.get(1),
                        args("a", Integer.parseInt(s.get(2))));
                case "form" -> com.legend.compiler.spec.Calls.form(com.legend.platform.CoreFn.valueOf(s.get(1)),
                        args("a", Integer.parseInt(s.get(2))));
                case "exact" -> com.legend.compiler.spec.Calls.exact(s.get(1), args("a", Integer.parseInt(s.get(2))));
                case "carrier" -> com.legend.compiler.spec.Calls.carrier(s.get(1),
                        args("a", Integer.parseInt(s.get(2))));
                case "infixRun" -> com.legend.compiler.spec.Calls.infixRun(s.get(1),
                        args("a", Integer.parseInt(s.get(2))));
                case "among" -> com.legend.compiler.spec.Calls.among(s.get(1), refs(s.get(2)),
                        args("a", Integer.parseInt(s.get(3))));
                case "infixRunAmong" -> com.legend.compiler.spec.Calls.infixRunAmong(s.get(1), refs(s.get(2)),
                        args("a", Integer.parseInt(s.get(3))));
                case "renamed" -> com.legend.compiler.spec.Calls.renamed(
                        original(s.get(1), Integer.parseInt(s.get(2)), s.get(3), s.get(4)), s.get(5),
                        args("b", Integer.parseInt(s.get(6))));
                case "like" -> com.legend.compiler.spec.Calls.like(
                        original(s.get(1), Integer.parseInt(s.get(2)), s.get(3), s.get(4)),
                        args("b", Integer.parseInt(s.get(5))));
                default -> throw new IllegalArgumentException("no builder " + s.get(0));
            };
            return "A\t" + codec.write(af, ValueSpecification.class);
        } catch (RuntimeException e) {
            return error(e);
        }
    }

    /** The names {@code Calls}' callers build through it, read from them (2026-11-12, every call site of core's main
     *  sources: 135 {@code platform}, 53 {@code exact}, 8 {@code infixRun}, 4 {@code form}, 3 {@code like}, 1
     *  {@code renamed}, {@code among} and {@code infixRunAmong}), with the arity each site builds at ({@code -1}: the
     *  site's arguments are another call's, asked at 0 to 5); a name a site computes is listed from the table it
     *  computes it from (the parse functions, the comparison shims, the legacy ranks, the sort directions). */
    private static final Object[][] CALLERS = {
        {"platform", "adjust", 3}, {"platform", "and", 2}, {"platform", "at", 2}, {"platform", "cast", 2},
        {"platform", "col", 2}, {"platform", "concatenate", 2}, {"platform", "count", 1},
        {"platform", "dayOfWeek", -1}, {"platform", "dayOfWeekNumber", 1}, {"platform", "distinct", 1},
        {"platform", "distinct", 2}, {"platform", "distinct", -1}, {"platform", "equal", 2},
        {"platform", "equal", -1}, {"platform", "execute", 5}, {"platform", "extend", 3},
        {"platform", "extend", -1}, {"platform", "filter", 2}, {"platform", "first", 1}, {"platform", "fold", -1},
        {"platform", "format", -1}, {"platform", "get", 2}, {"platform", "getAll", 1}, {"platform", "groupBy", 3},
        {"platform", "groupBy", 4}, {"platform", "groupBy", -1}, {"platform", "hash", 2}, {"platform", "if", 3},
        {"platform", "indexOf", 2}, {"platform", "isDistinct", 1}, {"platform", "isEmpty", 1},
        {"platform", "isEmpty", -1}, {"platform", "isNotEmpty", 1}, {"platform", "isNotEmpty", -1},
        {"platform", "join", 4}, {"platform", "join", -1}, {"platform", "joinStrings", 2},
        {"platform", "letFunction", 2}, {"platform", "map", 2}, {"platform", "mod", 2}, {"platform", "new", 2},
        {"platform", "not", 1}, {"platform", "not", -1}, {"platform", "or", 2}, {"platform", "over", 1},
        {"platform", "over", -1}, {"platform", "pair", 2}, {"platform", "project", 2}, {"platform", "project", 3},
        {"platform", "project", -1}, {"platform", "rename", 3}, {"platform", "restrict", 2},
        {"platform", "select", 2}, {"platform", "slice", 3}, {"platform", "splitPart", 3},
        {"platform", "substring", -1}, {"platform", "tableReference", 2}, {"platform", "to", 2},
        {"platform", "toOne", 1}, {"platform", "toOneMany", 1}, {"platform", "toString", 1},
        // DeclaredCoercions' parse functions, RelOpTranslator's comparison functions, TdsDesugars' legacy ranks,
        // SortChecker's directions, EvalChecker's user function
        {"platform", "parseInteger", 1}, {"platform", "parseFloat", 1}, {"platform", "parseDecimal", 1},
        {"platform", com.legend.builtin.Pure.Lite.LESS_THAN_ANY, 2},
        {"platform", com.legend.builtin.Pure.Lite.LESS_THAN_EQUAL_ANY, 2},
        {"platform", com.legend.builtin.Pure.Lite.GREATER_THAN_ANY, 2},
        {"platform", com.legend.builtin.Pure.Lite.GREATER_THAN_EQUAL_ANY, 2},
        {"platform", "rank", 3}, {"platform", "denseRank", 3}, {"platform", "rowNumber", 2},
        {"platform", "ascending", 1}, {"platform", "descending", 1}, {"platform", "my::pkg::f", -1},
        {"form", "ASC", 1}, {"form", "DESC", 1}, {"form", "RENAME", 3},
        {"infixRun", "plus", 2}, {"infixRun", "plus", -1}, {"infixRun", "minus", 2}, {"infixRun", "minus", -1},
        {"infixRun", "times", 2},
        {"exact", "meta::pure::functions::variant::convert::toVariant", 1},
        {"exact", com.legend.builtin.Pure.Lite.CAST_AS_DECLARED, 2},
        {"exact", com.legend.builtin.Pure.Lite.CONVERT_DATE_FORMAT, -1},
        {"exact", com.legend.builtin.Pure.Lite.CONVERT_DATE_TIME_FORMAT, -1},
        {"exact", com.legend.builtin.Pure.Lite.CONVERT_TIME_ZONE_FORMAT, -1},
        {"exact", com.legend.builtin.Pure.Lite.GROUP_BY_COMPUTED_KEYS, 3},
        {"exact", com.legend.builtin.Pure.Lite.GROUP_BY_OVER_INSTANCES, -1},
        {"exact", com.legend.builtin.Pure.Lite.INLINE_VIEW, 1},
        {"exact", com.legend.builtin.Pure.Lite.JOIN_SLOT, 3},
        {"exact", com.legend.builtin.Pure.Lite.LEGACY_ASSOC_PREDICATE, 5},
        {"exact", com.legend.builtin.Pure.Lite.LEGACY_LOCAL_PROPERTY, 2},
        {"exact", com.legend.builtin.Pure.Lite.LEGACY_NAVIGATE, 3},
        {"exact", com.legend.builtin.Pure.Lite.LEGACY_NAVIGATE, 4},
        {"exact", com.legend.builtin.Pure.Lite.OTHERWISE, 2},
        {"exact", com.legend.builtin.Pure.Lite.PARSE_DATE_FORMAT, -1},
        {"exact", com.legend.builtin.Pure.Lite.ROUTE, 3},
        {"exact", com.legend.builtin.Pure.Lite.SOURCE_URL, 1},
        {"exact", com.legend.builtin.Pure.Lite.TRUST_ONE, 1},
        {"exact", com.legend.builtin.Pure.Lite.TUPLE, -1},
        {"exact", com.legend.builtin.Pure.Lite.TYPE_AS_DECLARED, 2},
        {"exact", com.legend.builtin.Pure.SQL_NULL.qualifiedName(), 0},
        {"exact", "my::pkg::f", -1},
        // a bare spelling through exact: the rule is never consulted
        {"exact", "filter", 2},
        {"carrier", com.legend.protocol.spec.AppliedFunction.NEW, 2},
        {"carrier", com.legend.protocol.spec.AppliedFunction.LIST, 1},
    };

    /** Every call shape asked: the callers' ({@link #CALLERS}); then a sweep over every bare name the platform's tables
     *  know (the catalog natives' short names, the engine surface's handler names, the forms' spellings, the prelude's
     *  functions, the dynafunctions' names and their shims' lite names) through {@code platform} at arities 0 to 8 and
     *  through {@code infixRun} at two operands; every form at 0 to 4; every PURE dynafunction through {@code among}
     *  and {@code infixRunAmong} with its registry row; and originals of every caller's name through {@code like} and
     *  {@code renamed}, at their arity and the ones beside it, with no record, their built record, another, and both,
     *  plain and with every written form set. */
    static List<List<String>> callShapes() {
        LinkedHashSet<List<String>> out = new LinkedHashSet<>();
        LinkedHashSet<String> callerNames = new LinkedHashSet<>();
        for (Object[] c : CALLERS) {
            String builder = (String) c[0];
            String name = (String) c[1];
            int arity = (Integer) c[2];
            if (builder.equals("platform")) {
                callerNames.add(name);
            }
            for (int n = arity < 0 ? 0 : arity; n <= (arity < 0 ? 5 : arity); n++) {
                out.add(List.of(builder, name, Integer.toString(n)));
            }
        }
        java.util.TreeSet<String> names = new java.util.TreeSet<>();
        for (com.legend.model.NativeFunctionDefinition nf : com.legend.builtin.Pure.all()) {
            String q = nf.qualifiedName();
            names.add(q.substring(q.lastIndexOf("::") + 2));
        }
        names.addAll(com.legend.builtin.EngineHandlers.names());
        names.addAll(com.legend.platform.CoreFn.parseNames().keySet());
        for (PackageableElement el : Prelude.elements()) {
            if (el instanceof Function fn) {
                String q = fn.qualifiedName();
                names.add(q.substring(q.lastIndexOf("::") + 2));
            }
        }
        for (com.legend.builtin.DynaFn d : com.legend.builtin.DynaFn.values()) {
            names.add(d.dynaName());
            if (d.resolution() == com.legend.builtin.DynaFn.Resolution.SHIM) {
                names.add(d.liteFqn());
            }
        }
        for (String name : names) {
            for (int n = 0; n <= 8; n++) {
                out.add(List.of("platform", name, Integer.toString(n)));
            }
            out.add(List.of("infixRun", name, "2"));
        }
        for (com.legend.platform.CoreFn form : com.legend.platform.CoreFn.values()) {
            for (int n = 0; n <= 4; n++) {
                out.add(List.of("form", form.name(), Integer.toString(n)));
            }
        }
        for (com.legend.builtin.DynaFn d : com.legend.builtin.DynaFn.values()) {
            if (d.resolution() == com.legend.builtin.DynaFn.Resolution.PURE) {
                String row = String.join(",", d.fqns());
                for (int n = 1; n <= 3; n++) {
                    out.add(List.of("among", d.dynaName(), row, Integer.toString(n)));
                    out.add(List.of("infixRunAmong", d.dynaName(), row, Integer.toString(n)));
                }
            }
        }
        out.add(List.of("among", "plus", "", "2"));
        out.add(List.of("infixRunAmong", "plus", "", "2"));
        callerNames.add("my::pkg::f");
        callerNames.add("noSuchFunctionAnywhere");
        for (String fn : callerNames) {
            for (int n = 0; n <= 3; n++) {
                String built = String.join(",", com.legend.compiler.spec.Calls.platform(fn, args("a", n))
                        .referents());
                LinkedHashSet<String> records = new LinkedHashSet<>(List.of("", built, "other::pkg::" + fn,
                        built.isEmpty() ? "other::pkg::" + fn : built + ",other::pkg::" + fn));
                for (String record : records) {
                    for (String flags : List.of("", "pcgxifo")) {
                        for (int m = Math.max(0, n - 1); m <= n + 1; m++) {
                            out.add(List.of("like", fn, Integer.toString(n), record, flags, Integer.toString(m)));
                        }
                        out.add(List.of("renamed", fn, Integer.toString(n), record, flags, "ascending", "1"));
                        out.add(List.of("renamed", fn, Integer.toString(n), record, flags, fn,
                                Integer.toString(n + 1)));
                    }
                }
            }
        }
        return List.copyOf(out);
    }
    // =================================================================
    // Queries
    // =================================================================

    /** A model's universe for a query (the index's membership, as {@code PureModelContext.resolutionUniverse} asks it)
     *  and its union imports. */
    record ModelUniverse(Set<String> universe, ImportScope imports) {
    }

    private static Optional<ModelUniverse> universe(String text) {
        ParsedModel parsed;
        try {
            parsed = ModelFromText.parse(text, Dialect.LEGEND_LITE);
        } catch (RuntimeException e) {
            return Optional.empty();
        }
        try {
            Map<String, String> walls = new LinkedHashMap<>();
            ParsedModel resolved = NameResolver.resolveAlongside(parsed, BOOT_FQNS, walls);
            ParsedModel adopted = KnowledgeLayer.adoptAssociationQualifiedProperties(resolved, walls);
            ModelBuilder ix = ModelBuilder.from(adopted);
            ix.add(BootLayer.RESOLVED.elements());
            Set<String> platform = NameResolver.platformFqns();
            Set<String> extensions = ix.primitiveExtensionFqns();
            Set<String> u = new AbstractSet<>() {
                @Override
                public boolean contains(Object o) {
                    return o instanceof String s && (platform.contains(s) || extensions.contains(s)
                            || ix.hasElement(s) || ix.hasFunctionId(s));
                }

                @Override
                public Iterator<String> iterator() {
                    throw new UnsupportedOperationException("the resolver asks contains alone");
                }

                @Override
                public int size() {
                    throw new UnsupportedOperationException("the resolver asks contains alone");
                }
            };
            return Optional.of(new ModelUniverse(u, parsed.imports()));
        } catch (RuntimeException e) {
            return Optional.empty();
        }
    }

    /** The boot layer resolved, as {@code Compiler.boot()}'s first step resolves it. */
    private static final class BootLayer {
        static final ParsedModel RESOLVED = resolve();

        private static ParsedModel resolve() {
            ParsedModel pre = SystemMetamodel.requireNoSystemElementRedefined(Prelude.parsedModel());
            List<PackageableElement> elements = new ArrayList<>(SystemMetamodel.elements());
            elements.addAll(pre.elements());
            return NameResolver.resolve(new ParsedModel(elements, ImportScope.empty(), null, pre.elementOffsets(),
                    pre.elementImports(), pre.elementSources()));
        }
    }

    static String queryForm(String text, Optional<ModelUniverse> u, RecordCodec codec) {
        ValueSpecification q;
        try {
            q = SpecParser.parse(text, Dialect.LEGEND_LITE);
        } catch (RuntimeException e) {
            return "P\t" + error(e);
        }
        StringBuilder f = new StringBuilder();
        String n = query(() -> NameResolver.resolveQuery(q), codec);
        f.append("N\t").append(n);
        if (u.isEmpty()) {
            return f.append("\nM\t-").toString();
        }
        String mText = query(() -> NameResolver.resolveQueryIn(q, ImportScope.empty(), u.get().universe()), codec);
        f.append("\nM\t").append(same(mText, "N", n));
        String iText = query(() -> NameResolver.resolveQueryIn(q, u.get().imports(), u.get().universe()), codec);
        f.append("\nI\t").append(same(iText, "M", mText));
        return f.toString();
    }

    private static String query(Supplier<ValueSpecification> s, RecordCodec codec) {
        try {
            return "A\t" + codec.write(s.get(), ValueSpecification.class);
        } catch (RuntimeException e) {
            return error(e);
        }
    }
}
