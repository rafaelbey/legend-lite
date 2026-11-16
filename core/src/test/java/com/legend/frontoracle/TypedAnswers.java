// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

package com.legend.frontoracle;

import com.legend.builtin.Prelude;
import com.legend.builtin.SystemMetamodel;
import com.legend.compiler.NameResolver;
import com.legend.compiler.element.PureModelContext;
import com.legend.compiler.element.TypedFunction;
import com.legend.compiler.spec.CompiledFunction;
import com.legend.compiler.spec.RelationGenerics;
import com.legend.compiler.spec.SpecCompiler;
import com.legend.compiler.spec.UserCallInliner;
import com.legend.compiler.spec.typed.TypedSpec;
import com.legend.door.ModelFromText;
import com.legend.error.LegendCompileException;
import com.legend.json.Json;
import com.legend.model.Function;
import com.legend.model.FunctionId;
import com.legend.model.NormalizedModel;
import com.legend.model.ParsedModel;
import com.legend.parser.Dialect;
import com.legend.protocol.spec.SqlIsland;
import com.legend.protocol.spec.ValueSpecification;
import java.io.BufferedReader;
import java.io.BufferedWriter;
import java.lang.reflect.InvocationTargetException;
import java.lang.reflect.Method;
import java.lang.reflect.ParameterizedType;
import java.lang.reflect.RecordComponent;
import java.lang.reflect.Type;
import java.math.BigDecimal;
import java.math.BigInteger;
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

/**
 * THE TYPER'S DIFFERENTIAL, ITS JAVA SIDE (docs/rust-port/PHASE3.md, the typer slice, stage 1b, 2026-11-16; the G
 * census's §14, the owner's ruling 42): what steps G (the typer) and G½ (the inliner) build, asked of the live Java,
 * for {@code rust/lite-spec} to be held to byte for byte, then frozen under {@code rust/oracles/typed/} before the Java
 * goes. ElementAnswers' sibling: the same D, E (with the boot layer) and F, then G.
 *
 * <p><b>Model asks</b>: every model text of the front oracle at every dialect it is asked at ({@code ["pmcd",
 * dialect]}) that the door parses, ElementAnswers' cases and G's own ({@code rust/oracles/typed/cases.jsonl}),
 * strict ({@code S}: walls null) and tolerant ({@code T}: one walls map threaded through D, E and F, as
 * {@code Compiler.buildModule} threads it; {@code =S} when equal). A run is the build's error
 * {@code E TAB class TAB phase TAB element TAB message}, or {@code W TAB walls} (tolerant, when any) and {@code A}, then
 * G over every body in {@code Compiler.compileAllBodies}' order -- {@code new TreeSet<>(ctx.functionFqns())}, each
 * fqn's overloads in candidate order, bodiless, boot (by id) and relation-generic (S112) bodies skipped, ONE
 * SpecCompiler for the model, so the fresh-name and memo state runs as the door's:
 * <pre>
 *   X TAB fqn TAB refusal          findFunction refused (the door walls the plain fqn)
 *   F TAB FunctionView             a body typed next: its signature (ElementAnswers' FunctionView)
 *   C TAB [typed statements]       its CompiledFunction's body, in the canonical text below
 *   R TAB class TAB phase TAB message   or its refusal, the full message (compileErrors' answer, not its first line)
 *   W TAB ["message",...]          then the compileErrors list: every refusal's message in that order
 * </pre>
 *
 * <p><b>Query asks</b>: every distinct (query, model) pair of the identity oracle whose model and query are both
 * {@code t:} texts of the front oracle (as NameAnswers takes them), and G's own cases' queries: the model built strict
 * at lite's dialect as above ({@code M TAB error} when it does not build), the query parsed by
 * {@code Compiler.parseQuery} ({@code P TAB error}), then {@code Compiler.query(ctx, parsed)} ({@code Q TAB refusal}
 * when its resolution refuses):
 * <pre>
 *   G TAB [typed statements]       TypedQuery.body()       (or R ...)
 *   Y TAB ExprType                 TypedQuery.resultType() (or R ...)
 *   A TAB [Declared]               TypedQuery.parameters() (or R ...)
 *   I TAB [typed statements]       new UserCallInliner(specs).inlineBody(body), no splice hook (ruling 42, ASK 13)
 *   L TAB {name: typed}            its queryLets()
 *   K TAB ProgramFacts             Compiler.programFacts(resolved, ctx) (or R ...)
 * </pre>
 * Each query ask is tagged island (the parsed query holds a {@code #SQL}, {@code #IQ} or {@code #Cypher} island: the
 * first met, pre-order) or island-free ({@code -}): the Rust differential marks the island asks "awaiting the islands"
 * until they are ported (ruling 42, ASK 9), never comparing them against nothing.
 *
 * <p><b>The canonical text</b> (§14.3; ruling 42, ASKs 6 and 7): the record codec's form ({@link RecordCodec}: a record
 * a JSON object of its components in order, an {@code "@"} tag where the declared type does not fix the class, a
 * {@code double}, {@code BigDecimal}, {@code BigInteger} or {@code Long} in a {@code Number} slot written with its
 * class, so a BigDecimal keeps its scale and a Long never reads as a BigInteger), with these rules:
 * <ol>
 *   <li>sharing: a typed node ({@code TypedSpec}) met more than once in one line is written whole at its first visit
 *       with a leading {@code "@id":"n<k>"} (k by first visit), and {@code {"@ref":"n<k>"}} after;</li>
 *   <li>origins ({@code Evaluation.origin}, {@code Evaluation.Site.origin}, {@code TypedUserCall.origin}): the source
 *       node's place in the ask's own source tree ({@code "p:s<i>/<component>:<index>..."}, first place wins; a body's
 *       statements or the resolved query), else {@code "v<k>"} for a source node outside it (a callee's body, a
 *       property's default, a substituted copy) and {@code "o<k>"} for a synthesized token, k by first meeting;</li>
 *   <li>callees: a {@code TypedFunction} is its {@code FunctionView} (ElementAnswers'), never its body;</li>
 *   <li>a map or a set is written sorted by its key (Java's String order): Java's equality is order-insensitive;</li>
 *   <li>source positions are dropped (a {@code SourceInfo} component is not written; ruling 42, ASK 7);</li>
 *   <li>a value of a class with no rule throws (AGENTS.md invariant 4): {@code TypedFrameRef} (minted only by the
 *       planner's splice hook, which G½'s oracle runs without) among them;</li>
 *   <li>relation types by value: a {@code Type.RelationType} equal (Java's record equality) to one met earlier in the
 *       line is {@code {"@rel":"r<k>"}}, its first visit leading with {@code "@relid":"r<k>"} (k by first visit) --
 *       a typed node's info repeats its relation's columns, and a Cypher island's CREATE chain wrote 1.27 billion
 *       characters for one query without it.</li>
 * </ol>
 *
 * <p>Lines: {@code sha TAB dialect TAB islands TAB hash} per model ask (islands: the languages of the islands its
 * functions' bodies hold, or {@code -}), {@code querySha TAB modelSha TAB island TAB hash} per query ask, the hash the
 * SHA-256 of the form's UTF-16 units, little-endian. {@code OUT.tags} holds the measurements per ask (never hashed):
 * the refusal classes met, the origin and sharing label counts, each phase's wall time in microseconds (a cheap timing
 * mode for the G census's §15), and the hit counters of an instrumented build (a class {@code com.legend.base.Hits},
 * present only in a measurement copy of the jars, read by reflection; never committed).
 *
 * <pre>
 *   java ... TypedAnswers.java OUT QUESTIONS.jsonl HITS.tsv [CASES.jsonl ...] [--texts DIR] [--limit N]
 *       [--models-only | --queries-only] [--shard I/N] [--keys FILE] [--check-door]
 * </pre>
 * {@code --check-door} also asks {@code Compiler.compileAllBodies} itself of every model run and tags whether its walls
 * are the replica's compileErrors list (a measurement: the replica's loop is the door's, line for line).
 * {@code --shard I/N} answers the model asks whose index is I mod N and the query asks whose model's SHA-256 falls in
 * bucket I (each line prefixed by its ask's index, for {@code merge}); {@code --keys FILE} only the asks it lists.
 */
public final class TypedAnswers {

    private TypedAnswers() {
    }

    /** The codec met a value it has no rule for: the run fails (never written as an answer). */
    static final class NoRule extends IllegalStateException {
        NoRule(String m) {
            super(m);
        }
    }

    // =================================================================
    // D, E and F as ElementAnswers drives them
    // =================================================================

    private static final Set<String> BOOT_FQNS = bootFqns();

    private static Set<String> bootFqns() {
        Set<String> out = new java.util.HashSet<>(SystemMetamodel.elementFqns());
        out.addAll(Prelude.elementFqns());
        return out;
    }

    /** ElementAnswers' registrations (rust/oracles/builtin's PlatformTables'), so the Rust side can build the same. */
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

    private static final Object BOOT_LAYER = call(BOOT, null);
    private static final PureModelContext.CheckedLayer BOOT_CHECKED =
            (PureModelContext.CheckedLayer) component(BOOT_LAYER, "checked");
    /** The boot layer's bodies, skipped by id as compileAllBodies skips them. */
    @SuppressWarnings("unchecked")
    private static final Set<FunctionId> BOOT_IDS =
            ((Map<FunctionId, ?>) component(BOOT_LAYER, "functionsById")).keySet();

    private static PureModelContext build(ParsedModel parsed, Map<String, String> walls) {
        ParsedModel resolved = NameResolver.resolveAlongside(parsed, BOOT_FQNS, walls);
        Object layer = call(NORMALIZE_WITH_SYSTEM, null, resolved, walls);
        NormalizedModel nm = (NormalizedModel) component(layer, "model");
        com.legend.compiler.ModelBuilder index = (com.legend.compiler.ModelBuilder) component(layer, "index");
        @SuppressWarnings("unchecked")
        Set<FunctionId> adopted = (Set<FunctionId>) component(layer, "adopted");
        return PureModelContext.from(nm, index, walls, BOOT_CHECKED.without(adopted),
                LIVE_REGISTRATIONS ? com.legend.lowering.PlatformRegistrations.current() : REGISTRATIONS);
    }

    /** {@code --live-registrations}: F handed the registrations Compiler.buildModel hands it (the lowering's own
     *  included), not ElementAnswers' -- a measurement that the oracle's answers do not depend on the difference. */
    private static boolean LIVE_REGISTRATIONS;

    // =================================================================
    // The asks
    // =================================================================

    /** {@code --check-door}: each model run's compileErrors list checked against Compiler.compileAllBodies' own. */
    private static boolean CHECK_DOOR;

    /** The measurements of the ask being answered (never hashed). */
    private static final Map<String, Long> TAGS = new TreeMap<>();
    private static final TreeMap<String, Long> STATS = new TreeMap<>();

    private static void tag(String t, long n) {
        TAGS.merge(t, n, Long::sum);
        STATS.merge(t.contains("=") ? t.substring(0, t.indexOf('=')) : t, n, Long::sum);
    }

    private static void tag(String t) {
        tag(t, 1);
    }

    private static long micros(long t0) {
        return (System.nanoTime() - t0) / 1000;
    }

    /** One model ask: strict, then tolerant ({@code =S} when equal). */
    static String modelForm(ParsedModel parsed) {
        String s = modelRun(parsed, null, "S");
        String t = modelRun(parsed, new LinkedHashMap<>(), "T");
        return "S\t" + s + "\nT\t" + (t.equals(s) ? "=S" : t);
    }

    private static String modelRun(ParsedModel parsed, Map<String, String> walls, String mode) {
        long t0 = System.nanoTime();
        PureModelContext ctx;
        try {
            ctx = build(parsed, walls);
        } catch (RuntimeException | StackOverflowError e) {
            tag(mode + ":build-error:" + e.getClass().getSimpleName());
            return error(e);
        } finally {
            tag(mode + ":us-def", micros(t0));
        }
        StringBuilder out = new StringBuilder();
        if (walls != null && !walls.isEmpty()) {
            out.append("W\t").append(walls(walls)).append('\n');
        }
        out.append('A');
        long t1 = System.nanoTime();
        resetHits();
        SpecCompiler specs = new SpecCompiler(ctx);
        List<String> errors = new ArrayList<>();
        int bodies = 0;
        for (String fqn : new TreeSet<>(ctx.functionFqns())) {
            List<TypedFunction> overloads;
            try {
                overloads = ctx.findFunction(fqn);
            } catch (RuntimeException e) {
                errors.add(String.valueOf(e.getMessage()));
                out.append("\nX\t").append(fqn).append('\t').append(refusal(e));
                tag(mode + ":find-refused");
                continue;
            }
            for (TypedFunction tf : overloads) {
                if (tf.body().isEmpty() || BOOT_IDS.contains(tf.id()) || RelationGenerics.of(tf)) {
                    continue;
                }
                bodies++;
                out.append("\nF\t").append(functionView(tf));
                try {
                    CompiledFunction cf = specs.compile(tf);
                    out.append("\nC\t").append(typed(cf.body(), places(tf.body().orElseThrow()), mode));
                } catch (NoRule e) {
                    throw e;
                } catch (RuntimeException e) {
                    errors.add(String.valueOf(e.getMessage()));
                    out.append("\nR\t").append(refusal(e));
                    tag(mode + ":body-refused:" + e.getClass().getSimpleName());
                } catch (StackOverflowError e) {
                    // the door does not catch it: compileErrors would throw; written so, and counted
                    out.append("\nR\t").append(refusal(e));
                    tag(mode + ":body-overflow");
                }
            }
        }
        out.append("\nW\t").append(strings(errors));
        if (CHECK_DOOR) {
            // the replica against the door: compileAllBodies' own walls, in their order (a measurement, never hashed)
            List<String> door = new ArrayList<>(com.legend.Compiler.compileAllBodies(ctx).values());
            tag(mode + (door.equals(errors) ? ":door-same" : ":door-DIFFERS"));
        }
        tag(mode + ":bodies", bodies);
        tag(mode + ":us-g", micros(t1));
        readHits(mode);
        return out.toString();
    }

    /** One query ask over a model built strict at lite's dialect (the model memoized per text: its F cells are true
     *  answers; every query gets its own SpecCompiler, as Compiler.query gives it). */
    static String queryForm(String queryText, Object model) {
        if (model instanceof RuntimeException e) {
            return "M\t" + error(e);
        }
        if (model instanceof StackOverflowError e) {
            return "M\t" + error(e);
        }
        PureModelContext ctx = (PureModelContext) model;
        ValueSpecification parsed;
        try {
            parsed = com.legend.Compiler.parseQuery(queryText);
        } catch (RuntimeException e) {
            return "P\t" + error(e);
        }
        StringBuilder out = new StringBuilder();
        resetHits();
        long t0 = System.nanoTime();
        com.legend.TypedQuery tq;
        try {
            tq = com.legend.Compiler.query(ctx, parsed);
        } catch (RuntimeException | StackOverflowError e) {
            tag("Q:resolve-refused:" + e.getClass().getSimpleName());
            return "Q\t" + refusal(e);
        }
        IdentityHashMap<Object, String> places = places(List.of(tq.resolved()));
        List<TypedSpec> body;
        try {
            body = tq.body();
        } catch (RuntimeException | StackOverflowError e) {
            tag("Q:type-refused:" + e.getClass().getSimpleName());
            tag("Q:us-g", micros(t0));
            readHits("Q");
            return out.append("G\t").append(refusal(e)).toString();
        }
        tag("Q:us-g", micros(t0));
        out.append("G\t").append(typed(body, places, "Q"));
        out.append("\nY\t");
        try {
            out.append(new TypedCodec(places).write(tq.resultType(),
                    com.legend.compiler.element.type.ExprType.class));
        } catch (NoRule e) {
            throw e;
        } catch (RuntimeException | StackOverflowError e) {
            out.append(refusal(e));
        }
        out.append("\nA\t");
        try {
            out.append(new TypedCodec(places).write(tq.parameters(), parametersType()));
        } catch (NoRule e) {
            throw e;
        } catch (RuntimeException | StackOverflowError e) {
            out.append(refusal(e));
        }
        long t1 = System.nanoTime();
        out.append("\nI\t");
        try {
            UserCallInliner inliner = new UserCallInliner(tq.specs());
            List<TypedSpec> inlined = inliner.inlineBody(body);
            out.append(typed(inlined, places, "I"));
            out.append("\nL\t").append(new TypedCodec(places).write(new LinkedHashMap<>(inliner.queryLets()),
                    letsType()));
        } catch (NoRule e) {
            throw e;
        } catch (RuntimeException | StackOverflowError e) {
            tag("I:refused:" + e.getClass().getSimpleName());
            out.append(refusal(e));
        }
        tag("I:us", micros(t1));
        long t2 = System.nanoTime();
        out.append("\nK\t");
        try {
            out.append(new TypedCodec(places).write(com.legend.Compiler.programFacts(tq.resolved(), ctx),
                    com.legend.planner.ProgramFacts.class));
        } catch (NoRule e) {
            throw e;
        } catch (RuntimeException | StackOverflowError e) {
            tag("K:refused:" + e.getClass().getSimpleName());
            out.append(refusal(e));
        }
        tag("K:us", micros(t2));
        readHits("Q");
        return out.toString();
    }

    /** A query's model: the built strict context, or what refused it. */
    static Object model(String text) {
        try {
            return build(ModelFromText.parse(text, Dialect.LEGEND_LITE), null);
        } catch (RuntimeException | StackOverflowError e) {
            return e;
        }
    }

    // carriers of the declared types the codec writes a door's answer as
    @SuppressWarnings("unused")
    private static List<com.legend.planner.QueryParameters.Declared> parametersCarrier() {
        return List.of();
    }

    @SuppressWarnings("unused")
    private static Map<String, TypedSpec> letsCarrier() {
        return Map.of();
    }

    @SuppressWarnings("unused")
    private static List<TypedSpec> statementsCarrier() {
        return List.of();
    }

    private static Type carrier(String name) {
        try {
            return TypedAnswers.class.getDeclaredMethod(name).getGenericReturnType();
        } catch (NoSuchMethodException e) {
            throw new IllegalStateException(e);
        }
    }

    private static Type parametersType() {
        return carrier("parametersCarrier");
    }

    private static Type letsType() {
        return carrier("letsCarrier");
    }

    private static String typed(List<TypedSpec> statements, IdentityHashMap<Object, String> places, String mode) {
        TypedCodec codec = new TypedCodec(places);
        String s = codec.write(statements, carrier("statementsCarrier"));
        tag(mode + ":shared", codec.shared);
        tag(mode + ":origin-place", codec.originPlace);
        tag(mode + ":origin-v", codec.originV);
        tag(mode + ":origin-o", codec.originO);
        return s;
    }

    // =================================================================
    // Places: every record of the ask's own source tree, by its path
    // =================================================================

    /** {@code s<i>} the i-th statement, then {@code /<j>} its j-th written component and {@code :<k>} a list's k-th
     *  element; first place wins. */
    static IdentityHashMap<Object, String> places(List<? extends ValueSpecification> statements) {
        IdentityHashMap<Object, String> out = new IdentityHashMap<>();
        for (int i = 0; i < statements.size(); i++) {
            place(statements.get(i), "s" + i, out);
        }
        return out;
    }

    private static void place(Object o, String path, IdentityHashMap<Object, String> out) {
        if (o == null) {
            return;
        }
        if (o instanceof List<?> l) {
            for (int k = 0; k < l.size(); k++) {
                place(l.get(k), path + ":" + k, out);
            }
            return;
        }
        if (o instanceof Optional<?> opt) {
            place(opt.orElse(null), path, out);
            return;
        }
        if (o instanceof Map<?, ?> m) {
            int k = 0;
            for (Object v : m.values()) {
                place(v, path + ":" + k++, out);
            }
            return;
        }
        if (!o.getClass().isRecord() || o instanceof com.legend.protocol.SourceInfo) {
            return;
        }
        if (out.putIfAbsent(o, path) != null) {
            return;
        }
        RecordComponent[] cs = o.getClass().getRecordComponents();
        for (int j = 0; j < cs.length; j++) {
            place(read(o, cs[j]), path + "/" + j, out);
        }
    }

    // =================================================================
    // The canonical text
    // =================================================================

    static final class TypedCodec {
        private final IdentityHashMap<Object, String> places;
        private final IdentityHashMap<Object, Integer> occurrences = new IdentityHashMap<>();
        private final IdentityHashMap<Object, String> ids = new IdentityHashMap<>();
        private final IdentityHashMap<Object, String> synth = new IdentityHashMap<>();
        /** Relation types by value (Java's record equality): how many times each is met, and the label of one met
         *  more than once. */
        private final Map<Object, Integer> relationCounts = new java.util.HashMap<>();
        private final Map<Object, String> relationIds = new java.util.HashMap<>();
        long shared;
        long originPlace;
        long originV;
        long originO;
        private int nextV;
        private int nextO;

        TypedCodec(IdentityHashMap<Object, String> places) {
            this.places = places;
        }

        String write(Object value, Type declared) {
            count(value);
            StringBuilder out = new StringBuilder();
            value(out, value, declared, false);
            return out.toString();
        }

        /** The sharing pass: every typed node's occurrences, each node's children walked once. */
        private void count(Object o) {
            if (o == null) {
                return;
            }
            if (o instanceof List<?> l) {
                for (Object x : l) {
                    count(x);
                }
            } else if (o instanceof Optional<?> opt) {
                count(opt.orElse(null));
            } else if (o instanceof Map<?, ?> m) {
                for (Object x : m.values()) {
                    count(x);
                }
            } else if (o instanceof Set<?> s) {
                for (Object x : s) {
                    count(x);
                }
            } else if (o.getClass().isRecord() && !(o instanceof TypedFunction)
                    && !(o instanceof com.legend.protocol.SourceInfo)) {
                if (o instanceof TypedSpec) {
                    Integer n = occurrences.get(o);
                    occurrences.put(o, n == null ? 1 : n + 1);
                    if (n != null) {
                        return;
                    }
                }
                if (o instanceof com.legend.compiler.element.type.Type.RelationType) {
                    Integer n = relationCounts.get(o);
                    relationCounts.put(o, n == null ? 1 : n + 1);
                    if (n != null) {
                        return;
                    }
                }
                for (RecordComponent c : o.getClass().getRecordComponents()) {
                    if (!isOrigin(c)) {
                        count(read(o, c));
                    }
                }
            }
        }

        private static boolean isOrigin(RecordComponent c) {
            return c.getName().equals("origin") && c.getType() == Object.class;
        }

        private void value(StringBuilder out, Object o, Type declared, boolean origin) {
            if (o == null) {
                out.append("null");
                return;
            }
            if (origin) {
                origin(out, o);
                return;
            }
            Class<?> erased = RecordCodec.erase(declared);
            boolean exact = erased == o.getClass();
            if (o instanceof String s) {
                leaf(out, exact, o, () -> RecordCodec.string(out, s));
            } else if (o instanceof Boolean || o instanceof Integer || o instanceof Long || o instanceof Short
                    || o instanceof Byte) {
                leaf(out, exact || erased.isPrimitive(), o, () -> out.append(o));
            } else if (o instanceof Character c) {
                leaf(out, exact || erased.isPrimitive(), o, () -> RecordCodec.string(out, String.valueOf(c)));
            } else if (o instanceof Double || o instanceof Float || o instanceof BigDecimal
                    || o instanceof BigInteger) {
                leaf(out, exact || erased.isPrimitive(), o, () -> RecordCodec.string(out, o.toString()));
            } else if (o instanceof Enum<?> e) {
                leaf(out, erased == e.getDeclaringClass(), o, () -> RecordCodec.string(out, e.name()));
            } else if (o instanceof Optional<?> opt) {
                value(out, opt.orElse(null), argument(declared, 0), false);
            } else if (o instanceof List<?> list) {
                Type element = argument(declared, 0);
                out.append('[');
                for (int i = 0; i < list.size(); i++) {
                    if (i > 0) {
                        out.append(',');
                    }
                    value(out, list.get(i), element, false);
                }
                out.append(']');
            } else if (o instanceof Set<?> set) {
                Type element = argument(declared, 0);
                out.append('[');
                boolean first = true;
                for (Object x : sorted(set)) {
                    if (!first) {
                        out.append(',');
                    }
                    first = false;
                    value(out, x, element, false);
                }
                out.append(']');
            } else if (o instanceof Map<?, ?> map) {
                Type key = argument(declared, 0);
                Type val = argument(declared, 1);
                boolean stringKeys = RecordCodec.erase(key) == String.class;
                out.append(stringKeys ? '{' : '[');
                boolean first = true;
                for (Object k : sorted(map.keySet())) {
                    if (!first) {
                        out.append(',');
                    }
                    first = false;
                    if (stringKeys) {
                        RecordCodec.string(out, (String) k);
                        out.append(':');
                        value(out, map.get(k), val, false);
                    } else {
                        out.append('[');
                        value(out, k, key, false);
                        out.append(',');
                        value(out, map.get(k), val, false);
                        out.append(']');
                    }
                }
                out.append(stringKeys ? '}' : ']');
            } else if (o instanceof TypedFunction tf) {
                out.append(functionView(tf));
            } else if (o instanceof com.legend.compiler.spec.typed.TypedFrameRef) {
                throw new NoRule("TypedAnswers has no rule for a TypedFrameRef (the planner's "
                        + "splice hook mints it; G½'s oracle runs without the hook)");
            } else if (o.getClass().isRecord()) {
                record(out, (Record) o, exact);
            } else {
                throw new NoRule("TypedAnswers has no rule for " + o.getClass().getName()
                        + " (declared " + declared.getTypeName() + ")");
            }
        }

        /** A map's keys or a set's elements in Java's String order of their spelling (each a String, an enum or a
         *  number; anything else has no rule). */
        private static List<Object> sorted(java.util.Collection<?> xs) {
            TreeMap<String, Object> out = new TreeMap<>();
            for (Object x : xs) {
                String k;
                if (x instanceof String s) {
                    k = s;
                } else if (x instanceof Enum<?> e) {
                    k = e.name();
                } else if (x instanceof Number || x instanceof Boolean) {
                    k = x.toString();
                } else {
                    throw new NoRule("TypedAnswers has no order for a key or element of "
                            + (x == null ? "null" : x.getClass().getName()));
                }
                if (out.put(k, x) != null) {
                    throw new NoRule("two keys of one spelling: " + k);
                }
            }
            return new ArrayList<>(out.values());
        }

        private void origin(StringBuilder out, Object o) {
            String p = places.get(o);
            if (p != null) {
                originPlace++;
                RecordCodec.string(out, "p:" + p);
                return;
            }
            String s = synth.get(o);
            if (s == null) {
                if (o instanceof ValueSpecification) {
                    s = "v" + nextV++;
                    originV++;
                } else if (o.getClass() == Object.class) {
                    s = "o" + nextO++;
                    originO++;
                } else {
                    throw new NoRule("TypedAnswers has no rule for an origin of "
                            + o.getClass().getName());
                }
                synth.put(o, s);
            }
            RecordCodec.string(out, s);
        }

        private void leaf(StringBuilder out, boolean exact, Object o, Runnable write) {
            if (exact) {
                write.run();
                return;
            }
            out.append("{\"@\":");
            RecordCodec.string(out, RecordCodec.tag(o.getClass()));
            out.append(",\"$\":");
            write.run();
            out.append('}');
        }

        private void record(StringBuilder out, Record r, boolean exact) {
            if (r instanceof com.legend.compiler.element.type.Type.RelationType
                    && relationCounts.getOrDefault(r, 0) > 1) {
                String id = relationIds.get(r);
                if (id != null) {
                    out.append("{\"@rel\":\"").append(id).append("\"}");
                    return;
                }
                id = "r" + relationIds.size();
                relationIds.put(r, id);
                out.append("{\"@relid\":\"").append(id).append('"');
                recordBody(out, r, exact, false);
                return;
            }
            if (r instanceof TypedSpec && occurrences.getOrDefault(r, 0) > 1) {
                String id = ids.get(r);
                if (id != null) {
                    out.append("{\"@ref\":\"").append(id).append("\"}");
                    return;
                }
                id = "n" + ids.size();
                ids.put(r, id);
                shared++;
                out.append("{\"@id\":\"").append(id).append('"');
                recordBody(out, r, exact, false);
                return;
            }
            out.append('{');
            recordBody(out, r, exact, true);
        }

        private void recordBody(StringBuilder out, Record r, boolean exact, boolean first) {
            if (!exact) {
                if (!first) {
                    out.append(',');
                }
                out.append("\"@\":");
                RecordCodec.string(out, RecordCodec.tag(r.getClass()));
                first = false;
            }
            for (RecordComponent c : r.getClass().getRecordComponents()) {
                if (c.getType() == com.legend.protocol.SourceInfo.class) {
                    continue;
                }
                if (!first) {
                    out.append(',');
                }
                first = false;
                RecordCodec.string(out, c.getName());
                out.append(':');
                value(out, read(r, c), c.getGenericType(), isOrigin(c));
            }
            out.append('}');
        }
    }

    private static Object read(Object r, RecordComponent c) {
        try {
            Method m = c.getAccessor();
            m.setAccessible(true);
            return m.invoke(r);
        } catch (ReflectiveOperationException e) {
            throw new IllegalStateException("cannot read " + r.getClass().getName() + "." + c.getName(), e);
        }
    }

    private static Type argument(Type t, int i) {
        return t instanceof ParameterizedType p ? p.getActualTypeArguments()[i] : Object.class;
    }

    // =================================================================
    // Small writers, as ElementAnswers'
    // =================================================================

    private static final RecordCodec DEFINITIONS = new RecordCodec();
    private static final RecordCodec VIEWS = new RecordCodec();

    /** ElementAnswers' FunctionView of a callee or a body's signature. */
    static String functionView(TypedFunction tf) {
        String definition = tf.definition() == null ? "null"
                : hash(DEFINITIONS.write(tf.definition(), Function.class));
        return VIEWS.write(new ElementAnswers.FunctionView(tf.qualifiedName(), tf.typeParameters(),
                tf.multiplicityParameters(), tf.parameters(), tf.returnType(), tf.returnMultiplicity(),
                tf.body().isPresent(), tf.isNative(), tf.id(), definition), ElementAnswers.FunctionView.class);
    }

    private static final MessageDigest SHA = sha256();

    private static MessageDigest sha256() {
        try {
            return MessageDigest.getInstance("SHA-256");
        } catch (java.security.NoSuchAlgorithmException e) {
            throw new IllegalStateException(e);
        }
    }

    static String hash(String form) {
        // in chunks: a form can pass a gigabyte, whose UTF-16 bytes no one array holds
        int i = 0;
        while (i < form.length()) {
            int end = Math.min(form.length(), i + (1 << 20));
            if (end < form.length() && Character.isHighSurrogate(form.charAt(end - 1))) {
                end++;   // a surrogate pair is never split between two chunks
            }
            SHA.update(form.substring(i, end).getBytes(StandardCharsets.UTF_16LE));
            i = end;
        }
        return HexFormat.of().formatHex(SHA.digest());
    }

    /** {@code E TAB class TAB phase TAB element TAB message}. */
    static String error(Throwable e) {
        String phase = e instanceof LegendCompileException lce ? lce.phase().name() : "-";
        String element = e instanceof LegendCompileException lce && lce.element() != null ? lce.element() : "-";
        return "E\t" + e.getClass().getName() + "\t" + phase + "\t" + element + "\t" + e.getMessage();
    }

    /** {@code R TAB class TAB phase TAB message}: a refusal, its full message. */
    static String refusal(Throwable e) {
        String phase = e instanceof LegendCompileException lce ? lce.phase().name() : "-";
        return "R\t" + e.getClass().getName() + "\t" + phase + "\t" + e.getMessage();
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

    // =================================================================
    // Islands
    // =================================================================

    /** The languages of the islands a tree holds, in the order met (pre-order). */
    static void islands(Object o, Set<String> out, IdentityHashMap<Object, Boolean> seen) {
        if (o == null) {
            return;
        }
        if (o instanceof SqlIsland si) {
            out.add(si.language());
            return;
        }
        if (o instanceof List<?> l) {
            for (Object x : l) {
                islands(x, out, seen);
            }
        } else if (o instanceof Optional<?> opt) {
            islands(opt.orElse(null), out, seen);
        } else if (o instanceof Map<?, ?> m) {
            for (Object x : m.values()) {
                islands(x, out, seen);
            }
        } else if (o instanceof Set<?> s) {
            for (Object x : s) {
                islands(x, out, seen);
            }
        } else if (o.getClass().isRecord() && seen.put(o, Boolean.TRUE) == null) {
            for (RecordComponent c : o.getClass().getRecordComponents()) {
                islands(read(o, c), out, seen);
            }
        }
    }

    private static String islandTag(Object tree) {
        Set<String> found = new LinkedHashSet<>();
        islands(tree, found, new IdentityHashMap<>());
        return found.isEmpty() ? "-" : String.join("+", found);
    }

    // =================================================================
    // The measurement copy's hit counters (absent from the product's jars)
    // =================================================================

    private static final Method HITS_RESET;
    private static final Method HITS_TAKE;

    static {
        Method reset = null;
        Method take = null;
        try {
            Class<?> h = Class.forName("com.legend.base.Hits");
            reset = h.getMethod("reset");
            take = h.getMethod("take");
        } catch (ReflectiveOperationException e) {
            // the product's jars: no counters
        }
        HITS_RESET = reset;
        HITS_TAKE = take;
    }

    private static void resetHits() {
        if (HITS_RESET != null) {
            call(HITS_RESET, null);
        }
    }

    @SuppressWarnings("unchecked")
    private static void readHits(String mode) {
        if (HITS_TAKE != null) {
            for (Map.Entry<String, Long> e : ((Map<String, Long>) call(HITS_TAKE, null)).entrySet()) {
                tag("hit:" + mode + ":" + e.getKey(), e.getValue());
            }
        }
    }

    // =================================================================
    // The run
    // =================================================================

    public static void main(String[] args) throws Exception {
        Throwable[] failed = new Throwable[1];
        Thread th = new Thread(null, () -> {
            try {
                run(args);
            } catch (Throwable e) {
                // an Error too (an OutOfMemoryError ended a shard silently, exit 0, until 2026-11-16)
                failed[0] = e;
            }
        }, "typed-answers", 1L << 30);
        th.start();
        th.join();
        if (failed[0] != null) {
            failed[0].printStackTrace();
            System.exit(1);
        }
    }

    private static void run(String[] args) throws Exception {
        Path out = Path.of(args[0]);
        Path questions = Path.of(args[1]);
        Path hits = Path.of(args[2]);
        List<Path> cases = new ArrayList<>();
        Path texts = null;
        long limit = Long.MAX_VALUE;
        boolean models = true;
        boolean queries = true;
        int shard = 0;
        int shards = 1;
        Set<String> keys = null;
        for (int i = 3; i < args.length; i++) {
            switch (args[i]) {
                case "--texts" -> texts = Path.of(args[++i]);
                case "--limit" -> limit = Long.parseLong(args[++i]);
                case "--check-door" -> CHECK_DOOR = true;
                case "--live-registrations" -> LIVE_REGISTRATIONS = true;
                case "--models-only" -> queries = false;
                case "--queries-only" -> models = false;
                case "--shard" -> {
                    String[] p = args[++i].split("/");
                    shard = Integer.parseInt(p[0]);
                    shards = Integer.parseInt(p[1]);
                }
                case "--keys" -> keys = new java.util.HashSet<>(Files.readAllLines(Path.of(args[++i])));
                default -> cases.add(Path.of(args[i]));
            }
        }
        if (texts != null) {
            Files.createDirectories(texts);
        }
        // every text by its sha (the front oracle's, then the cases'), and the model asks in order
        Map<String, String> textBySha = new LinkedHashMap<>();
        List<String[]> modelAsks = new ArrayList<>();
        List<String[]> caseQueries = new ArrayList<>();
        List<Path> all = new ArrayList<>();
        all.add(questions);
        all.addAll(cases);
        for (Path q : all) {
            try (BufferedReader in = Files.newBufferedReader(q, StandardCharsets.UTF_8)) {
                for (String line = in.readLine(); line != null; line = in.readLine()) {
                    Json.Obj o = (Json.Obj) Json.parse(line);
                    String sha = ((Json.Str) o.get("sha")).value();
                    String text = ((Json.Str) o.get("text")).value();
                    textBySha.putIfAbsent(sha, text);
                    for (Json.Node a : ((Json.Arr) o.get("asks")).items()) {
                        List<Json.Node> pair = ((Json.Arr) a).items();
                        if (((Json.Str) pair.get(0)).value().equals("pmcd")) {
                            modelAsks.add(new String[] {sha, ((Json.Str) pair.get(1)).value()});
                        }
                    }
                    if (o.has("queries") && o.get("queries") instanceof Json.Arr qs) {
                        for (Json.Node qn : qs.items()) {
                            String qt = ((Json.Str) qn).value();
                            String qsha = hash(qt);
                            textBySha.putIfAbsent(qsha, qt);
                            caseQueries.add(new String[] {qsha, sha});
                        }
                    }
                }
            }
        }
        try (BufferedWriter w = Files.newBufferedWriter(Path.of(out + ".models.tsv"), StandardCharsets.UTF_8);
                BufferedWriter gw = Files.newBufferedWriter(Path.of(out + ".models.tags"), StandardCharsets.UTF_8);
                BufferedWriter tw = texts == null ? null
                        : Files.newBufferedWriter(texts.resolve("models.jsonl"), StandardCharsets.UTF_8)) {
            long n = 0;
            for (int idx = 0; models && idx < modelAsks.size() && n < limit; idx++) {
                if (idx % shards != shard) {
                    continue;
                }
                String sha = modelAsks.get(idx)[0];
                String dialect = modelAsks.get(idx)[1];
                if (keys != null && !keys.contains(sha + "\t" + dialect)) {
                    continue;
                }
                ParsedModel parsed;
                try {
                    parsed = ModelFromText.parse(textBySha.get(sha), Dialect.valueOf(dialect));
                } catch (RuntimeException e) {
                    continue;
                }
                n++;
                TAGS.clear();
                String islands = islandTag(parsed);
                String form = modelForm(parsed);
                String prefix = shards > 1 ? idx + "\t" : "";
                w.write(prefix + sha + "\t" + dialect + "\t" + islands + "\t" + hash(form) + "\n");
                gw.write(prefix + sha + "\t" + dialect + "\t" + tags() + "\n");
                if (tw != null) {
                    StringBuilder j = new StringBuilder("{\"sha\":\"").append(sha).append("\",\"dialect\":\"")
                            .append(dialect).append("\",\"form\":");
                    RecordCodec.string(j, form);
                    tw.write(prefix + j.append("}\n"));
                }
            }
            System.out.printf("[typed-answers] %d model asks answered%n", n);
        }
        // the query pairs: the identity oracle's keys, model and query both t: texts of the front oracle, then the
        // cases' (query, model) pairs
        LinkedHashSet<List<String>> pairs = new LinkedHashSet<>();
        if (queries) {
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
            for (String[] cq : caseQueries) {
                pairs.add(List.of(cq[0], cq[1]));
            }
        }
        Map<String, Object> builtModels = new LinkedHashMap<>();
        try (BufferedWriter w = Files.newBufferedWriter(Path.of(out + ".queries.tsv"), StandardCharsets.UTF_8);
                BufferedWriter gw = Files.newBufferedWriter(Path.of(out + ".queries.tags"), StandardCharsets.UTF_8);
                BufferedWriter tw = texts == null ? null
                        : Files.newBufferedWriter(texts.resolve("queries.jsonl"), StandardCharsets.UTF_8)) {
            long n = 0;
            int idx = -1;
            String last = null;
            for (List<String> pair : pairs) {
                idx++;
                if (n >= limit) {
                    break;
                }
                String q = pair.get(0);
                String m = pair.get(1);
                if (shards > 1 && Math.floorMod(m.hashCode(), shards) != shard) {
                    continue;
                }
                if (keys != null && !keys.contains(q + "\t" + m)) {
                    continue;
                }
                n++;
                if (!m.equals(last)) {
                    builtModels.clear();
                    last = m;
                }
                TAGS.clear();
                long t0 = System.nanoTime();
                Object model = builtModels.computeIfAbsent(m, k -> model(textBySha.get(k)));
                tag("Q:us-model", micros(t0));
                String island;
                try {
                    island = islandTag(com.legend.Compiler.parseQuery(textBySha.get(q)));
                } catch (RuntimeException e) {
                    island = "-";
                }
                String form = queryForm(textBySha.get(q), model);
                if (form.length() > 50_000_000) {
                    StringBuilder sizes = new StringBuilder();
                    for (String section : form.split("\n")) {
                        sizes.append(section, 0, Math.min(2, section.length())).append(section.length()).append(' ');
                    }
                    System.err.println("[typed-answers] a form of " + form.length() + " characters: " + q + " " + m
                            + " sections " + sizes);
                    if (System.getProperty("typed.debug") != null) {
                        TreeMap<String, Long> tagsMet = new TreeMap<>();
                        int at = form.indexOf("\"@\":\"");
                        while (at >= 0) {
                            int end = form.indexOf('"', at + 5);
                            tagsMet.merge(form.substring(at + 5, end), 1L, Long::sum);
                            at = form.indexOf("\"@\":\"", end);
                        }
                        System.err.println("[typed-answers] tags " + tagsMet);
                        System.err.println("[typed-answers] refs " + (form.split("@ref", -1).length - 1) + " ids "
                                + (form.split("\"@id\"", -1).length - 1) + " callees "
                                + (form.split("\"qualifiedName\"", -1).length - 1));
                        Files.writeString(Path.of(System.getProperty("typed.debug")), form.substring(0, 400_000));
                    }
                }
                String prefix = shards > 1 ? idx + "\t" : "";
                w.write(prefix + q + "\t" + m + "\t" + island + "\t" + hash(form) + "\n");
                gw.write(prefix + q + "\t" + m + "\t" + tags() + "\n");
                if (tw != null) {
                    StringBuilder j = new StringBuilder("{\"query\":\"").append(q).append("\",\"model\":\"")
                            .append(m).append("\",\"form\":");
                    RecordCodec.string(j, form);
                    tw.write(prefix + j.append("}\n"));
                }
            }
            System.out.printf("[typed-answers] %d query asks answered (%d pairs)%n", n, pairs.size());
        }
        for (Map.Entry<String, Long> e : STATS.entrySet()) {
            if (!e.getKey().startsWith("hit:") && !e.getKey().contains(":us")) {
                System.out.printf("[typed-answers] %s\t%d%n", e.getKey(), e.getValue());
            }
        }
        System.out.printf("[typed-answers] record types reached by the views %s%n", VIEWS.reached.size());
    }

    private static String tags() {
        StringBuilder sb = new StringBuilder();
        for (Map.Entry<String, Long> e : TAGS.entrySet()) {
            if (sb.length() > 0) {
                sb.append('|');
            }
            sb.append(e.getKey()).append('=').append(e.getValue());
        }
        return sb.toString();
    }
}
