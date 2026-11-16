// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

package com.legend.frontoracle;

import com.legend.builtin.Prelude;
import com.legend.builtin.SystemMetamodel;
import com.legend.compiler.NameResolver;
import com.legend.compiler.element.Property;
import com.legend.compiler.element.PureModelContext;
import com.legend.compiler.element.TypedClass;
import com.legend.compiler.element.TypedConstraint;
import com.legend.compiler.element.TypedEnum;
import com.legend.compiler.element.TypedFunction;
import com.legend.compiler.element.TypedParameter;
import com.legend.compiler.element.type.Multiplicity;
import com.legend.compiler.element.type.Type;
import com.legend.door.ModelFromText;
import com.legend.error.LegendCompileException;
import com.legend.json.Json;
import com.legend.model.AssociationDefinition;
import com.legend.model.ClassDefinition;
import com.legend.model.DatabaseDefinition;
import com.legend.model.EnumDefinition;
import com.legend.model.Function;
import com.legend.model.FunctionId;
import com.legend.model.MappingDefinition;
import com.legend.model.NormalizedModel;
import com.legend.model.PackageableElement;
import com.legend.model.ParsedModel;
import com.legend.model.PoisonKey;
import com.legend.parser.Dialect;
import com.legend.platform.DeclarationTable;
import com.legend.platform.Implementation;
import com.legend.platform.ImplementationTable;
import com.legend.protocol.TypeExpression;
import java.io.BufferedReader;
import java.io.BufferedWriter;
import java.lang.reflect.InvocationTargetException;
import java.lang.reflect.Method;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.security.MessageDigest;
import java.util.ArrayList;
import java.util.HexFormat;
import java.util.IdentityHashMap;
import java.util.LinkedHashMap;
import java.util.LinkedHashSet;
import java.util.List;
import java.util.Map;
import java.util.Optional;
import java.util.Set;
import java.util.TreeMap;
import java.util.TreeSet;
import java.util.stream.Collectors;

/**
 * THE ELEMENT DIFFERENTIAL'S JAVA SIDE (docs/rust-port/PHASE3.md, the element slice, 2026-11-15): what step F builds
 * from E's {@code NormalizedModel}, asked of the live Java, for {@code rust/lite-element} to be held to byte for byte
 * ({@code tests/element_differential.rs}), then frozen under {@code rust/oracles/element/} before the Java goes.
 *
 * <p>Its entry is {@code PureModelContext.from}'s integrity result, not {@code Compiler.compileErrors}: that door goes
 * on to {@code compileAllBodies} (step G) whenever the model's elements compile ({@code Compiler.compileErrorsOf}), and
 * this tool drives D, E and F only, so a strict first error is D's, E's or F's. Every model text of the front oracle at
 * every dialect it is asked at ({@code ["pmcd", dialect]}) that the door parses, and the slice's cases
 * ({@code rust/oracles/element/cases.jsonl}), driven as {@code Compiler.buildModel} (strict) and
 * {@code Compiler.buildModule} (tolerant) drive them, minus G:
 * <ol>
 *   <li>{@code NameResolver.resolveAlongside(parsed, bootFqns, walls)} (D);</li>
 *   <li>{@code Compiler.normalizeWithSystem(resolved, walls)} (the prelude's shadows dropped, a system element's
 *       redefinition refused, the platform's own Pure adopted, E over the graph's own elements, the boot layer's
 *       prepared elements appended; read by reflection: it is the compile driver's, private);</li>
 *   <li>{@code PureModelContext.from(model, index, walls, boot.checked().without(adopted), registrations)} (F's gate
 *       and its integrity pass, the boot layer checked on its own once per process).</li>
 * </ol>
 * The canonical text:
 * <pre>
 *   S TAB strict       walls null throughout: the first error is D's, E's or F's
 *   T TAB tolerant     one walls map threaded through D, E and F, as buildModule threads it; {@code =S} when equal
 * </pre>
 * a run is an error {@code E TAB class TAB phase TAB element TAB message} (the message as thrown: the positions
 * {@code Compiler.compileModel(String)} prefixes are the text door's, not F's), or the built model's answer: the walls
 * when a tolerant run has any ({@code W TAB [[key,line],...]}, D's, E's and F's in the order they were met, F's in
 * {@code ModelIntegrity}'s order), then {@code A}, then one line per question about the graph's OWN elements (those the
 * boot layer did not bring: compared by identity), in their ingest order, every element's lazy cell forced, a cell
 * that refuses written as {@code R TAB class TAB first line} in its place:
 * <pre>
 *   C fqn ClassView         findClass: the typed class (its supertypes, properties, constraints, key flags, realizers)
 *   N fqn TypedEnum         findEnum
 *   F fqn [FunctionView..]  findFunction: each overload in candidate order; then " same" when a second ask hands out
 *                           the same instances (the function cell's one object per overload) and every
 *                           findFunctionById answer is one of them
 *   I id [FunctionView..]   findFunctionById, per function element
 *   P class name Property   findProperty over the names the class's declared reach and the graph's association ends
 *                           name, and one it has not
 *   Y name Type             findType for each type name the graph's declarations use, and each element FQN
 *   S child parent T|F      isSubtype over the graph's classes against its classes, Any and Nil (the first 48 of each)
 *   D id table-answers      the declaration table's entry and the implementation table's row for each function
 *   G mapping ...           the mapping fact readers (findMapping's stamped facts by key; findLegacyMapping)
 *   B db table RelationType Milestoning  findTable and findTableMilestoning for each table of the graph's databases
 *                           (their default schema); V db view [FunctionView] findViewFunction for each view
 *   E fqn T|F               isExecutionContextElement for each of the graph's elements
 *   K name T|F              resolutionUniverse().contains; Q name T|F isPackage
 *   U n [names..]           resolutionUniverse() iterated, SORTED (Java's String order), less the boot-only baseline
 *   X classifier n [..]     classifierInstances, less the baseline; L lists the tables' duplicates, dangling,
 *                           conflicts and member walls, less the baseline
 * </pre>
 * The record codec writes each value (a projection where Java's record holds what E already wrote:
 * {@code ClassView}, {@code FunctionView}). The boot layer's own elements are written once, beside the answers
 * ({@code OUT.boot.txt}: the empty text's model with every element forced, and the baselines).
 *
 * <p>F is handed the platform's registrations as the lowering assembles them but for its own (no lowering keys, no
 * feature overrides, no members: slice I's), rust/oracles/builtin's PlatformTables' set, which F reads for one answer,
 * {@code implementations()}, so the Rust side can build the same.
 *
 * <p>Lines: {@code sha TAB dialect TAB hash} per ask, the hash the SHA-256 of the form's UTF-16 units, little-endian.
 * {@code --texts DIR} also writes every form ({@code forms.jsonl}). The measurements (the error classes met, how many
 * asks reach each F family) are printed to stdout and written per ask to {@code OUT.tsv.tags}, never hashed.
 *
 * <pre>
 *   java ... ElementAnswers.java OUT.tsv QUESTIONS.jsonl [MORE.jsonl ...] [--texts DIR] [--limit N]
 * </pre>
 */
public final class ElementAnswers {

    private ElementAnswers() {
    }

    /** The typed class, written whole but for its constraints' external id (always empty; the census's RE-EXPRESS). */
    record ClassView(String qualifiedName, List<String> typeParameters, List<String> superClassFqns,
            List<Type> superTypes, List<Property> properties, List<ConstraintView> constraints, boolean isNative,
            List<String> multiplicityParameters) {
    }

    record ConstraintView(String name, TypedConstraint.EnforcementLevel level, String predicateFqn,
            Optional<String> messageFqn) {
    }

    /** A typed function's signature: its body and definition are E's, written by the normalizer's oracle, so the body
     *  is its presence and the definition its codec text's SHA-256. */
    record FunctionView(String qualifiedName, List<String> typeParameters, List<String> multiplicityParameters,
            List<TypedParameter> parameters, Type returnType, Multiplicity returnMultiplicity, boolean bodyPresent,
            boolean isNative, FunctionId id, String definition) {
    }

    private static final Set<String> BOOT_FQNS = bootFqns();

    private static Set<String> bootFqns() {
        Set<String> out = new java.util.HashSet<>(SystemMetamodel.elementFqns());
        out.addAll(Prelude.elementFqns());
        return out;
    }

    /** The platform's registrations as the lowering assembles them but for its own (no lowering keys, no feature
     *  overrides, no members: the lowering's, slice I's), the walled natives and each form's FQNs sorted -- the
     *  registrations rust/oracles/builtin's PlatformTables hands the implementation table, so the Rust side can build
     *  the same. F reads them for one answer only: {@code implementations()}. */
    private static final com.legend.platform.Registrations REGISTRATIONS = registrations();

    private static com.legend.platform.Registrations registrations() {
        Map<Class<? extends com.legend.builtin.NativeFn.Member>, List<com.legend.model.NativeFunctionDefinition>>
                families = new LinkedHashMap<>();
        for (List<? extends com.legend.builtin.NativeFn.Member> members
                : com.legend.builtin.NativeFn.families().values()) {
            for (com.legend.builtin.NativeFn.Member m : members) {
                Class<? extends com.legend.builtin.NativeFn.Member> family = ((Enum<?>) m).getDeclaringClass()
                        .asSubclass(com.legend.builtin.NativeFn.Member.class);
                families.computeIfAbsent(family, k -> new ArrayList<>()).addAll(m.overloads());
            }
        }
        Map<com.legend.platform.CoreFn, Set<String>> forms = new LinkedHashMap<>();
        for (com.legend.platform.CoreFn form : com.legend.platform.CoreFn.values()) {
            if (!form.ownedFqns().isEmpty()) {
                forms.put(form, new LinkedHashSet<>(new TreeSet<>(form.ownedFqns())));
            }
        }
        Map<String, String> walledNatives = new LinkedHashMap<>();
        for (String fqn : new TreeSet<>(com.legend.builtin.Pure.walledNativeFqns())) {
            walledNatives.put(fqn, java.util.Objects.requireNonNull(com.legend.builtin.Pure.walledNativeReason(fqn)));
        }
        Set<String> subsumed = new LinkedHashSet<>();
        for (com.legend.builtin.Subsumed s : com.legend.builtin.Subsumed.values()) {
            subsumed.add(s.fqn());
        }
        return new com.legend.platform.Registrations(com.legend.builtin.Pure.all(), Map.of(), Map.of(), families,
                forms, walledNatives, com.legend.platform.WalledBodies.reasons(), subsumed, Map.of(),
                com.legend.platform.PlatformPure.ids(), com.legend.platform.PlatformPure.upstreamBodies(),
                com.legend.platform.PlatformPure.refusedVersions());
    }

    private static final Method NORMALIZE_WITH_SYSTEM;
    private static final Method BOOT;

    static {
        try {
            NORMALIZE_WITH_SYSTEM = com.legend.Compiler.class.getDeclaredMethod("normalizeWithSystem",
                    ParsedModel.class, Map.class);
            NORMALIZE_WITH_SYSTEM.setAccessible(true);
            BOOT = com.legend.Compiler.class.getDeclaredMethod("boot");
            BOOT.setAccessible(true);
        } catch (NoSuchMethodException e) {
            throw new IllegalStateException(e);
        }
    }

    private static Object call(Method m, Object target, Object... args) {
        try {
            return m.invoke(target, args);
        } catch (InvocationTargetException e) {
            if (e.getCause() instanceof RuntimeException r) {
                throw r;
            }
            if (e.getCause() instanceof Error r) {
                throw r;
            }
            throw new IllegalStateException(e.getCause());
        } catch (IllegalAccessException e) {
            throw new IllegalStateException(e);
        }
    }

    private static Object component(Object record, String name) {
        try {
            Method m = record.getClass().getDeclaredMethod(name);
            m.setAccessible(true);
            return call(m, record);
        } catch (NoSuchMethodException e) {
            throw new IllegalStateException(e);
        }
    }

    /** The boot layer: its normalized model (whose elements are the boot's own objects) and its checked layer. */
    private static final Object BOOT_LAYER = call(BOOT, null);
    private static final NormalizedModel BOOT_MODEL = (NormalizedModel) component(BOOT_LAYER, "model");
    private static final PureModelContext.CheckedLayer BOOT_CHECKED =
            (PureModelContext.CheckedLayer) component(BOOT_LAYER, "checked");
    private static final Set<Object> BOOT_ELEMENTS = identitySet(BOOT_MODEL.elements());

    private static Set<Object> identitySet(List<? extends Object> xs) {
        Set<Object> out = java.util.Collections.newSetFromMap(new IdentityHashMap<>());
        out.addAll(xs);
        return out;
    }

    /** The empty text's answers that every model shares (the boot layer's): subtracted from each ask's lists. */
    private static Baseline baseline;

    private record Baseline(Set<String> universe, Map<String, Set<String>> classifiers, Map<String, Set<String>> lists) {
    }

    private static final List<String> CLASSIFIERS = List.of(
            "meta::pure::metamodel::type::Class", "meta::pure::metamodel::type::Enumeration",
            "meta::pure::metamodel::relationship::Association", "meta::pure::mapping::Mapping",
            "meta::relational::metamodel::Database", "meta::pure::metamodel::type::Any");

    public static void main(String[] args) throws Exception {
        Path out = Path.of(args[0]);
        List<Path> questions = new ArrayList<>();
        Path texts = null;
        long limit = Long.MAX_VALUE;
        for (int i = 1; i < args.length; i++) {
            switch (args[i]) {
                case "--texts" -> texts = Path.of(args[++i]);
                case "--limit" -> limit = Long.parseLong(args[++i]);
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
        }, "element-answers", 1L << 30);
        th.start();
        th.join();
        if (failed[0] != null) {
            throw failed[0];
        }
    }

    private static final TreeMap<String, Long> STATS = new TreeMap<>();
    private static final Map<String, Set<String>> ASK_TAGS = new TreeMap<>();
    private static final String[] CURRENT = new String[1];

    private static void run(Path out, List<Path> questions, Path texts, long limit) throws Exception {
        MessageDigest md = MessageDigest.getInstance("SHA-256");
        RecordCodec codec = new RecordCodec();
        Files.writeString(Path.of(out + ".boot.txt"), boot(codec), StandardCharsets.UTF_8);
        Files.writeString(Path.of(out + ".boot-normalized.tsv"), bootNormalized(md), StandardCharsets.UTF_8);
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
        System.out.printf("[element-answers] %d asks answered%n", asks);
        for (Map.Entry<String, Long> e : STATS.entrySet()) {
            System.out.printf("[element-answers] %s\t%d%n", e.getKey(), e.getValue());
        }
        System.out.printf("[element-answers] record types reached %d: %s%n", codec.reached.size(), codec.reached);
        System.out.printf("[element-answers] containers %s%n", codec.containers);
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

    /** The context D, E and F build, strict ({@code walls} null) or tolerant. */
    private record Built(PureModelContext ctx, NormalizedModel model) {
    }

    private static Built build(ParsedModel parsed, Map<String, String> walls) {
        ParsedModel resolved = NameResolver.resolveAlongside(parsed, BOOT_FQNS, walls);
        Object layer = call(NORMALIZE_WITH_SYSTEM, null, resolved, walls);
        NormalizedModel nm = (NormalizedModel) component(layer, "model");
        com.legend.compiler.ModelBuilder index = (com.legend.compiler.ModelBuilder) component(layer, "index");
        @SuppressWarnings("unchecked")
        Set<FunctionId> adopted = (Set<FunctionId>) component(layer, "adopted");
        PureModelContext ctx = PureModelContext.from(nm, index, walls, BOOT_CHECKED.without(adopted), REGISTRATIONS);
        return new Built(ctx, nm);
    }

    private static String run(ParsedModel parsed, Map<String, String> walls, RecordCodec codec, String mode) {
        measureBounds(parsed, mode);
        Built built;
        try {
            built = build(parsed, walls);
        } catch (RuntimeException | StackOverflowError e) {
            count(mode + ":error:" + e.getClass().getName());
            tag(mode, "error:" + e.getClass().getName());
            if (e instanceof LegendCompileException lce && lce.element() != null) {
                tag(mode, "error-at:" + kindOf(parsed, lce.element()));
            }
            raw(mode, e);
            return error(e);
        }
        StringBuilder out = new StringBuilder();
        if (walls != null && !walls.isEmpty()) {
            out.append("W\t").append(walls(walls)).append('\n');
            tag(mode, "walls");
            for (String key : walls.keySet()) {
                tag(mode, "wall-at:" + kindOf(parsed, key));
            }
        }
        out.append('A');
        List<PackageableElement> own = new ArrayList<>();
        for (PackageableElement el : built.model().elements()) {
            if (!BOOT_ELEMENTS.contains(el)) {
                own.add(el);
            }
        }
        answer(out, built.ctx(), own, codec, mode, baseline);
        return out.toString();
    }

    /** The bounds the integrity pass checks (census ASK 7 (c), ruling 30): a multiplicity whose upper is below its
     *  lower, on a function's parameter or return, an association's end, a class's property (the existing check, the
     *  control), counted per ask off the input (never hashed). */
    private static void measureBounds(ParsedModel parsed, String mode) {
        for (PackageableElement el : parsed.elements()) {
            switch (el) {
                case Function f -> {
                    for (var p : f.parameters()) {
                        if (invalid(p.multiplicity())) {
                            tag(mode, "reach:1b function parameter bounds");
                        }
                    }
                    if (invalid(f.returnMultiplicity())) {
                        tag(mode, "reach:1b function return bounds");
                    }
                }
                case AssociationDefinition a -> {
                    if (invalid(a.property1().multiplicity()) || invalid(a.property2().multiplicity())) {
                        tag(mode, "reach:1b association end bounds");
                    }
                }
                case ClassDefinition cd -> {
                    for (ClassDefinition.PropertyDefinition pd : cd.properties()) {
                        if (invalid(pd.multiplicity())) {
                            tag(mode, "reach:1b class property bounds (control)");
                        }
                    }
                }
                default -> {
                }
            }
        }
    }

    private static boolean invalid(com.legend.protocol.Multiplicity m) {
        return m instanceof com.legend.protocol.Multiplicity.Concrete c && c.upperBound() != null
                && c.upperBound() < c.lowerBound();
    }

    /** The kind of the parsed element a refusal or a wall names (its FQN, or a function's id), for the measurements:
     *  which of ModelIntegrity's checks an ask reaches (never hashed). */
    private static String kindOf(ParsedModel parsed, String key) {
        for (PackageableElement el : parsed.elements()) {
            if (el.qualifiedName().equals(key)
                    || (el instanceof Function f && FunctionId.of(f).qualified().equals(key))) {
                return el.getClass().getSimpleName();
            }
        }
        return "(not a parsed element: lifted or boot)";
    }

    /** A raw Java exception a user's input reaches (ruling 29's second policy): named for the log. */
    private static void raw(String mode, Throwable e) {
        if (!e.getClass().getName().startsWith("com.legend.error.")) {
            StackTraceElement top = e.getStackTrace().length == 0 ? null : e.getStackTrace()[0];
            System.out.printf("[element-answers] RAW %s %s %s at %s: %s%n", CURRENT[0], mode,
                    e.getClass().getName(), top, firstLine(e));
        }
    }

    /** The boot layer's own answers and the baselines, from the empty text's model. */
    private static String boot(RecordCodec codec) {
        Built built = build(ModelFromText.parse("", Dialect.LEGEND_LITE), null);
        PureModelContext ctx = built.ctx();
        Set<String> universe = new java.util.HashSet<>(ctx.resolutionUniverse());
        Map<String, Set<String>> classifiers = new LinkedHashMap<>();
        for (String c : CLASSIFIERS) {
            List<String> xs = ctx.classifierInstances(c);
            classifiers.put(c, xs == null ? Set.of() : new java.util.HashSet<>(xs));
        }
        Map<String, Set<String>> lists = new LinkedHashMap<>();
        for (Map.Entry<String, List<String>> e : tableLists(ctx).entrySet()) {
            lists.put(e.getKey(), new java.util.HashSet<>(e.getValue()));
        }
        StringBuilder out = new StringBuilder("A");
        answer(out, ctx, built.model().elements(), codec, "B", null);
        List<String> sortedUniverse = new ArrayList<>(universe);
        java.util.Collections.sort(sortedUniverse);
        out.append("\nU\t").append(sortedUniverse.size()).append('\t').append(strings(sortedUniverse));
        for (String c : CLASSIFIERS) {
            List<String> xs = ctx.classifierInstances(c);
            out.append("\nX\t").append(c).append('\t').append(xs == null ? "null" : xs.size() + "\t" + strings(xs));
        }
        for (Map.Entry<String, List<String>> e : tableLists(ctx).entrySet()) {
            out.append("\nL\t").append(e.getKey()).append('\t').append(e.getValue().size()).append('\t')
                    .append(strings(e.getValue()));
        }
        baseline = new Baseline(universe, classifiers, lists);
        return out.append('\n').toString();
    }

    /** The boot layer as {@code Compiler.boot()} normalized it (the system metamodel and the prelude, resolved, the
     *  platform's own Pure adopted, E run over them): one line per element, in order, {@code index TAB fqn TAB hash}
     *  (the SHA-256 of its record-codec text's UTF-16 units), then {@code imports}, {@code legacySurfaces} and the
     *  whole {@code NormalizedModel}'s -- what the Rust boot is held to before F's answers can be. */
    private static String bootNormalized(MessageDigest md) {
        RecordCodec c = new RecordCodec();
        StringBuilder out = new StringBuilder();
        List<PackageableElement> els = BOOT_MODEL.elements();
        for (int i = 0; i < els.size(); i++) {
            out.append(i).append('\t').append(els.get(i).qualifiedName()).append('\t')
                    .append(hash(md, c.write(els.get(i), PackageableElement.class))).append('\n');
        }
        out.append("imports\t-\t").append(hash(md, c.write(BOOT_MODEL.imports(), com.legend.model.ImportScope.class)))
                .append('\n');
        out.append("legacySurfaces\t").append(BOOT_MODEL.legacySurfaces().size()).append('\t')
                .append(hash(md, c.write(new ArrayList<>(BOOT_MODEL.legacySurfaces().values()), legacyListType())))
                .append('\n');
        out.append("NormalizedModel\t").append(els.size()).append('\t')
                .append(hash(md, c.write(BOOT_MODEL, NormalizedModel.class))).append('\n');
        return out.toString();
    }

    /** {@code List<LegacyMappingDefinition>}, so the codec writes the surfaces untagged. */
    private static java.lang.reflect.Type legacyListType() {
        try {
            return ElementAnswers.class.getDeclaredMethod("legacyListCarrier").getGenericReturnType();
        } catch (NoSuchMethodException e) {
            throw new IllegalStateException(e);
        }
    }

    @SuppressWarnings("unused")
    private static List<com.legend.model.LegacyMappingDefinition> legacyListCarrier() {
        return List.of();
    }

    private static Map<String, List<String>> tableLists(PureModelContext ctx) {
        Map<String, List<String>> out = new LinkedHashMap<>();
        out.put("duplicates", ctx.declarations().duplicates());
        out.put("dangling", ctx.implementations().dangling());
        out.put("conflicts", ctx.implementations().conflicts());
        out.put("memberWalls", ctx.implementations().memberWalls());
        return out;
    }

    // =================================================================
    // The answer: the graph's own elements, in ingest order
    // =================================================================

    private static final int PAIRS = 48;
    private static final int NAMES = 32;
    private static final String ABSENT = "__absent__";

    private static void answer(StringBuilder out, PureModelContext ctx, List<PackageableElement> own,
            RecordCodec codec, String mode, Baseline base) {
        Set<String> tags = new TreeSet<>();
        List<ClassDefinition> classes = new ArrayList<>();
        List<Function> functions = new ArrayList<>();
        Set<String> functionFqns = new LinkedHashSet<>();
        List<AssociationDefinition> associations = new ArrayList<>();
        List<MappingDefinition> mappings = new ArrayList<>();
        List<DatabaseDefinition> databases = new ArrayList<>();
        Set<String> typeNames = new LinkedHashSet<>();
        for (PackageableElement el : own) {
            typeNames.add(el.qualifiedName());
            switch (el) {
                case ClassDefinition cd -> {
                    classes.add(cd);
                    tags.add("input:class");
                    out.append("\nC\t").append(cd.qualifiedName()).append('\t')
                            .append(cell(() -> classView(ctx.findClass(cd.qualifiedName()).orElse(null), codec), tags,
                                    "class"));
                    for (TypeExpression sup : cd.superClasses()) {
                        typeNames(sup, typeNames);
                    }
                    for (ClassDefinition.PropertyDefinition pd : cd.properties()) {
                        typeNames(pd.type(), typeNames);
                        if (pd.stereotypes().stream().anyMatch(st -> st.profileName().endsWith("::graph")
                                || st.profileName().equals("graph"))) {
                            tags.add("input:GE2 graph stereotype");
                        }
                    }
                    if (cd.stereotypes().stream().anyMatch(st -> st.profileName().endsWith("::graph"))) {
                        tags.add("input:GE2 graph stereotype");
                    }
                    for (var dp : cd.derivedProperties()) {
                        typeNames(dp.type(), typeNames);
                        for (var p : dp.parameters()) {
                            typeNames(p.type(), typeNames);
                        }
                    }
                    if (!cd.superClasses().isEmpty()) {
                        tags.add("input:class with supertypes");
                    }
                    if (!cd.constraints().isEmpty()) {
                        tags.add("input:class constraint");
                    }
                    if (!cd.derivedProperties().isEmpty()) {
                        tags.add("input:class derived property");
                    }
                }
                case EnumDefinition ed -> {
                    tags.add("input:enum");
                    out.append("\nN\t").append(ed.qualifiedName()).append('\t')
                            .append(cell(() -> {
                                TypedEnum te = ctx.findEnum(ed.qualifiedName()).orElse(null);
                                return te == null ? "none" : codec.write(te, TypedEnum.class);
                            }, tags, "enum"));
                }
                case Function f -> {
                    functions.add(f);
                    tags.add("input:function");
                    for (var p : f.parameters()) {
                        typeNames(p.type(), typeNames);
                    }
                    typeNames(f.returnType(), typeNames);
                    if (functionFqns.add(f.qualifiedName())) {
                        out.append("\nF\t").append(f.qualifiedName()).append('\t')
                                .append(cell(() -> functionList(ctx, f.qualifiedName(), codec), tags, "function"));
                    }
                    String id = FunctionId.of(f).qualified();
                    out.append("\nI\t").append(id).append('\t').append(cell(() -> {
                        List<String> views = new ArrayList<>();
                        for (TypedFunction tf : ctx.findFunctionById(id)) {
                            views.add(functionView(tf, codec));
                        }
                        return "[" + String.join(",", views) + "]";
                    }, tags, "function by id"));
                }
                case AssociationDefinition a -> {
                    associations.add(a);
                    tags.add("input:association");
                    typeNames(a.property1().targetClass(), typeNames);
                    typeNames(a.property2().targetClass(), typeNames);
                }
                case MappingDefinition m -> {
                    mappings.add(m);
                    tags.add("input:mapping");
                }
                case DatabaseDefinition db -> {
                    databases.add(db);
                    tags.add("input:database");
                }
                default -> tags.add("input:" + el.getClass().getSimpleName());
            }
        }
        if (base == null) {
            // the boot dump: the elements' cells alone
            return;
        }
        // P4: findProperty over the names each class's declared reach and the associations name, and one absent
        for (ClassDefinition cd : classes) {
            Set<String> names = new LinkedHashSet<>();
            declaredNames(ctx, cd, names, new java.util.HashSet<>());
            for (AssociationDefinition a : associations) {
                names.add(a.property1().propertyName());
                names.add(a.property2().propertyName());
            }
            List<String> asked = new ArrayList<>(names).subList(0, Math.min(NAMES, names.size()));
            List<String> all = new ArrayList<>(asked);
            all.add(ABSENT);
            for (String n : all) {
                out.append("\nP\t").append(cd.qualifiedName()).append('\t').append(n).append('\t')
                        .append(cell(() -> {
                            Property p = ctx.findProperty(cd.qualifiedName(), n).orElse(null);
                            return p == null ? "none" : codec.write(p, Property.class);
                        }, tags, "property"));
            }
        }
        // findType
        for (String n : typeNames) {
            out.append("\nY\t").append(n).append('\t').append(cell(() -> {
                Type t = ctx.findType(n).orElse(null);
                return t == null ? "none" : codec.write(t, Type.class);
            }, tags, "type"));
        }
        // P2: isSubtype
        List<String> children = new ArrayList<>();
        for (ClassDefinition cd : classes) {
            children.add(cd.qualifiedName());
        }
        List<String> parents = new ArrayList<>(children.subList(0, Math.min(PAIRS, children.size())));
        parents.add("meta::pure::metamodel::type::Any");
        parents.add("meta::pure::metamodel::type::Nil");
        if (classes.size() > PAIRS) {
            tags.add("bound:isSubtype pairs");
        }
        for (String c : children.subList(0, Math.min(PAIRS, children.size()))) {
            for (String p : parents) {
                out.append("\nS\t").append(c).append('\t').append(p).append('\t')
                        .append(cell(() -> ctx.isSubtype(c, p) ? "T" : "F", tags, "isSubtype"));
            }
        }
        // P8: the declaration and implementation tables
        DeclarationTable decl = ctx.declarations();
        ImplementationTable impl = ctx.implementations();
        Set<String> fqnsAsked = new LinkedHashSet<>();
        for (Function f : functions) {
            FunctionId id = FunctionId.of(f);
            Function d = decl.get(id);
            String entry = d == null ? "null" : d == f ? "self" : "other\t" + FunctionId.of(d).qualified();
            Implementation row = impl.of(id);
            out.append("\nD\t").append(id.qualified()).append('\t').append(entry).append('\t')
                    .append(row == null ? "null" : String.join("\t", row(row))).append('\t')
                    .append(impl.runsByRule(f)).append('\t').append(String.join("\t", row(impl.rowOf(f))));
            if (fqnsAsked.add(f.qualifiedName())) {
                out.append("\nD@\t").append(f.qualifiedName()).append('\t')
                        .append(decl.at(f.qualifiedName()).stream().map(x -> FunctionId.of(x).qualified())
                                .collect(Collectors.joining(",")));
            }
        }
        for (Map.Entry<String, List<String>> e : tableLists(ctx).entrySet()) {
            List<String> extra = new ArrayList<>();
            for (String s : e.getValue()) {
                if (!base.lists().get(e.getKey()).contains(s)) {
                    extra.add(s);
                }
            }
            out.append("\nL\t").append(e.getKey()).append('\t').append(e.getValue().size()).append('\t')
                    .append(strings(extra));
        }
        // P6: the mapping fact readers
        for (MappingDefinition m : mappings) {
            mappingFacts(out, ctx, m, codec, tags);
        }
        // P10: findTable and findTableMilestoning over the graph's databases' tables, findViewFunction over their views
        // (their default schema), and isExecutionContextElement over the graph's elements
        for (DatabaseDefinition db : databases) {
            for (var td : db.tables()) {
                out.append("\nB\t").append(db.qualifiedName()).append('\t').append(td.name()).append('\t')
                        .append(cell(() -> {
                            Type.RelationType rt = ctx.findTable(db.qualifiedName(), td.name()).orElse(null);
                            return rt == null ? "none" : codec.write(rt, Type.RelationType.class);
                        }, tags, "table"))
                        .append('\t').append(cell(() -> codec.write(
                                ctx.findTableMilestoning(db.qualifiedName(), td.name()).orElse(null),
                                DatabaseDefinition.TableDefinition.Milestoning.class), tags, "table milestoning"));
            }
            for (var vd : db.views()) {
                out.append("\nV\t").append(db.qualifiedName()).append('\t').append(vd.name()).append('\t')
                        .append(cell(() -> {
                            TypedFunction tf = ctx.findViewFunction(db.qualifiedName(), vd.name()).orElse(null);
                            return tf == null ? "none" : functionView(tf, codec);
                        }, tags, "view function"));
            }
        }
        for (PackageableElement el : own) {
            out.append("\nE\t").append(el.qualifiedName()).append('\t')
                    .append(ctx.isExecutionContextElement(el.qualifiedName()) ? 'T' : 'F');
        }
        // P5: the name universe
        Set<String> asked = new LinkedHashSet<>(typeNames);
        for (Function f : functions) {
            asked.add(FunctionId.of(f).qualified());
        }
        asked.add("absent::Name");
        Set<String> universe = ctx.resolutionUniverse();
        for (String n : asked) {
            out.append("\nK\t").append(n).append('\t').append(universe.contains(n) ? 'T' : 'F');
        }
        Set<String> packages = new LinkedHashSet<>();
        for (PackageableElement el : own) {
            String fqn = el.qualifiedName();
            for (int i = fqn.indexOf("::"); i >= 0; i = fqn.indexOf("::", i + 2)) {
                packages.add(fqn.substring(0, i));
            }
            packages.add(fqn);
        }
        packages.add("absent");
        for (String p : packages) {
            out.append("\nQ\t").append(p).append('\t').append(ctx.isPackage(p) ? 'T' : 'F');
        }
        List<String> sorted = new ArrayList<>(universe);
        java.util.Collections.sort(sorted);
        List<String> extra = new ArrayList<>();
        for (String s : sorted) {
            if (!base.universe().contains(s)) {
                extra.add(s);
            }
        }
        out.append("\nU\t").append(sorted.size()).append('\t').append(strings(extra));
        // P9: the metamodel registry
        for (String c : CLASSIFIERS) {
            List<String> xs = ctx.classifierInstances(c);
            out.append("\nX\t").append(c).append('\t').append(ctx.tracksClassifier(c) ? 'T' : 'F').append('\t');
            if (xs == null) {
                out.append("null");
                continue;
            }
            List<String> more = new ArrayList<>();
            for (String s : xs) {
                if (!base.classifiers().get(c).contains(s)) {
                    more.add(s);
                }
            }
            out.append(xs.size()).append('\t').append(strings(more));
        }
        for (String t : tags) {
            count(mode + ":" + t);
            tag(mode, t);
        }
    }

    /** The property names a class's declared reach names: its own, then each supertype's, depth-first, each once. */
    private static void declaredNames(PureModelContext ctx, ClassDefinition cd, Set<String> names, Set<String> seen) {
        if (!seen.add(cd.qualifiedName())) {
            return;
        }
        for (ClassDefinition.PropertyDefinition pd : cd.properties()) {
            names.add(pd.name());
        }
        for (var dp : cd.derivedProperties()) {
            names.add(dp.name());
        }
        for (TypeExpression sup : cd.superClasses()) {
            String head = sup instanceof TypeExpression.NameRef nr ? nr.name()
                    : sup instanceof TypeExpression.Generic g ? g.name() : null;
            if (head != null) {
                ctx.findClassDefinition(head).ifPresent(sc -> declaredNames(ctx, sc, names, seen));
            }
        }
    }

    private static void mappingFacts(StringBuilder out, PureModelContext ctx, MappingDefinition m, RecordCodec codec,
            Set<String> tags) {
        String fqn = m.qualifiedName();
        MappingDefinition.NormalizationFacts f = m.facts();
        Set<String> classes = new LinkedHashSet<>();
        Set<List<String>> sets = new LinkedHashSet<>();
        for (MappingDefinition.ClassBinding cb : m.classBindings()) {
            classes.add(cb.classFqn());
            if (cb.setId() != null) {
                sets.add(List.of(cb.classFqn(), cb.setId()));
            }
        }
        classes.addAll(f.unionMembers().keySet());
        classes.addAll(f.mixedUnions().keySet());
        classes.addAll(f.unionKeyThreads().keySet());
        classes.addAll(f.routedTargetClasses().keySet());
        Set<String> associations = new LinkedHashSet<>();
        for (MappingDefinition.AssociationBinding ab : m.associationBindings()) {
            associations.add(ab.associationFqn());
        }
        for (PoisonKey k : f.poisons().keySet()) {
            switch (k) {
                case PoisonKey.ForClass c -> classes.add(c.classFqn());
                case PoisonKey.ForSet s -> sets.add(List.of(s.classFqn(), s.setId()));
                case PoisonKey.ForAssociation a -> associations.add(a.associationFqn());
            }
        }
        if (!f.poisons().isEmpty() || !f.unionMembers().isEmpty() || !f.mixedUnions().isEmpty()
                || !f.unionKeyThreads().isEmpty() || !f.routedTargetClasses().isEmpty()) {
            tags.add("answer:P6 facts");
        }
        out.append("\nG\t").append(fqn).append("\tlegacy\t").append(ctx.findLegacyMapping(fqn).isPresent() ? 'T' : 'F');
        classes.add(ABSENT);
        for (String c : classes) {
            out.append("\nG\t").append(fqn).append("\tclass\t").append(c)
                    .append('\t').append(nullableStrings(ctx.unionMemberClasses(fqn, c)))
                    .append('\t').append(nullableStrings(ctx.mixedUnionMembers(fqn, c)))
                    .append('\t').append(codec.write(ctx.unionKeyThreads(fqn, c), keyThreadsType()))
                    .append('\t').append(codec.write(ctx.mappingPoison(fqn, c).orElse(null), String.class));
            Map<String, String> routed = f.routedTargetClasses().getOrDefault(c, Map.of());
            List<String> props = new ArrayList<>(routed.keySet());
            props.add(ABSENT);
            for (String p : props) {
                out.append("\nG\t").append(fqn).append("\troute\t").append(c).append('\t').append(p).append('\t')
                        .append(codec.write(ctx.routedTargetClass(fqn, c, p), String.class));
            }
        }
        sets.add(List.of(ABSENT, ABSENT));
        for (List<String> s : sets) {
            out.append("\nG\t").append(fqn).append("\tset\t").append(s.get(0)).append('\t').append(s.get(1))
                    .append('\t').append(codec.write(ctx.mappingSetPoison(fqn, s.get(0), s.get(1)).orElse(null),
                            String.class));
        }
        associations.add(ABSENT);
        for (String a : associations) {
            out.append("\nG\t").append(fqn).append("\tassociation\t").append(a).append('\t')
                    .append(codec.write(ctx.mappingAssociationPoison(fqn, a).orElse(null), String.class));
        }
    }

    /** {@code List<KeyThread>}, so the codec writes the threads untagged. */
    private static java.lang.reflect.Type keyThreadsType() {
        try {
            return ElementAnswers.class.getDeclaredMethod("keyThreadsCarrier").getGenericReturnType();
        } catch (NoSuchMethodException e) {
            throw new IllegalStateException(e);
        }
    }

    @SuppressWarnings("unused")
    private static List<com.legend.model.KeyThread> keyThreadsCarrier() {
        return List.of();
    }

    private static String classView(TypedClass tc, RecordCodec codec) {
        if (tc == null) {
            return "none";
        }
        List<ConstraintView> cs = new ArrayList<>();
        for (TypedConstraint c : tc.constraints()) {
            cs.add(new ConstraintView(c.name(), c.level(), c.predicateFqn(), c.messageFqn()));
        }
        return codec.write(new ClassView(tc.qualifiedName(), tc.typeParameters(), tc.superClassFqns(), tc.superTypes(),
                tc.properties(), cs, tc.isNative(), tc.multiplicityParameters()), ClassView.class);
    }

    private static final RecordCodec DEFINITIONS = new RecordCodec();
    private static final MessageDigest DEFINITION_SHA = sha256();

    private static MessageDigest sha256() {
        try {
            return MessageDigest.getInstance("SHA-256");
        } catch (java.security.NoSuchAlgorithmException e) {
            throw new IllegalStateException(e);
        }
    }

    private static String functionView(TypedFunction tf, RecordCodec codec) {
        String definition = tf.definition() == null ? "null"
                : hash(DEFINITION_SHA, DEFINITIONS.write(tf.definition(), Function.class));
        return codec.write(new FunctionView(tf.qualifiedName(), tf.typeParameters(), tf.multiplicityParameters(),
                tf.parameters(), tf.returnType(), tf.returnMultiplicity(), tf.body().isPresent(), tf.isNative(),
                tf.id(), definition), FunctionView.class);
    }

    private static String functionList(PureModelContext ctx, String fqn, RecordCodec codec) {
        List<TypedFunction> first = ctx.findFunction(fqn);
        List<String> views = new ArrayList<>();
        for (TypedFunction tf : first) {
            views.add(functionView(tf, codec));
        }
        List<TypedFunction> again = ctx.findFunction(fqn);
        boolean same = again.size() == first.size();
        for (int i = 0; same && i < first.size(); i++) {
            same = again.get(i) == first.get(i);
        }
        for (TypedFunction tf : first) {
            for (TypedFunction byId : ctx.findFunctionById(tf.id().qualified())) {
                if (first.stream().noneMatch(x -> x == byId) && byId.qualifiedName().equals(fqn)) {
                    same = false;
                }
            }
        }
        return "[" + String.join(",", views) + "]" + (same ? " same" : " apart");
    }

    private interface Cell {
        String get();
    }

    /** A lazy cell forced: its answer, or its refusal's class and first line in its place. */
    private static String cell(Cell c, Set<String> tags, String what) {
        try {
            String s = c.get();
            tags.add("cell:" + what);
            return s;
        } catch (RuntimeException | StackOverflowError e) {
            tags.add("refused:" + what + ":" + e.getClass().getName());
            raw("cell " + what, e);
            return "R\t" + e.getClass().getName() + "\t" + firstLine(e);
        }
    }

    private static String firstLine(Throwable e) {
        return String.valueOf(e.getMessage()).split("\n")[0];
    }

    private static void typeNames(TypeExpression te, Set<String> out) {
        switch (te) {
            case TypeExpression.NameRef nr -> out.add(nr.name());
            case TypeExpression.Generic g -> {
                out.add(g.name());
                for (TypeExpression a : g.arguments()) {
                    typeNames(a, out);
                }
            }
            case TypeExpression.FunctionType ft -> {
                for (TypeExpression.TypedParameter tp : ft.parameters()) {
                    typeNames(tp.type(), out);
                }
                typeNames(ft.result().type(), out);
            }
            case TypeExpression.RelationType rt -> {
                for (TypeExpression.Column c : rt.columns()) {
                    typeNames(c.type(), out);
                }
            }
            case TypeExpression.SchemaAlgebra sa -> {
                typeNames(sa.left(), out);
                typeNames(sa.right(), out);
            }
        }
    }

    /** An implementation-table row, as rust/oracles/builtin's PlatformTables writes one. */
    private static List<String> row(Implementation r) {
        return switch (r) {
            case Implementation.Form f -> List.of("Form", f.form().name(), names(f.alsoLowered()),
                    families(f.alsoFamilies()));
            case Implementation.Intrinsic n -> List.of("Intrinsic", names(n.positions()), names(n.featureOverrides()),
                    families(n.families()));
            case Implementation.Body b -> List.of("Body");
            case Implementation.PlatformPure p -> List.of("PlatformPure");
            case Implementation.Unimplemented u -> List.of("Unimplemented");
            case Implementation.Refused x -> List.of("Refused", x.reason().name(), x.why());
        };
    }

    private static String names(Set<? extends Enum<?>> s) {
        return s.stream().map(Enum::name).collect(Collectors.joining(","));
    }

    private static String families(Set<Class<? extends com.legend.builtin.NativeFn.Member>> s) {
        return s.stream().map(Class::getSimpleName).collect(Collectors.joining(","));
    }

    private static String nullableStrings(List<String> xs) {
        return xs == null ? "null" : strings(xs);
    }

    private static String strings(List<String> xs) {
        StringBuilder sb = new StringBuilder("[");
        for (int i = 0; i < xs.size(); i++) {
            if (i > 0) {
                sb.append(',');
            }
            RecordCodec.string(sb, xs.get(i));
        }
        return sb.append(']').toString();
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

    private static void count(String key) {
        STATS.merge(key, 1L, Long::sum);
    }

    private static void tag(String mode, String tag) {
        ASK_TAGS.computeIfAbsent(mode, k -> new TreeSet<>()).add(tag);
    }
}
