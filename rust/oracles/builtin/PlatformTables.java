// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

import com.legend.builtin.NativeFn;
import com.legend.builtin.Prelude;
import com.legend.builtin.Pure;
import com.legend.builtin.Subsumed;
import com.legend.builtin.SystemMetamodel;
import com.legend.door.ModelFromText;
import com.legend.error.ModelException;
import com.legend.model.ClassMember;
import com.legend.model.Function;
import com.legend.model.FunctionDefinition;
import com.legend.model.FunctionId;
import com.legend.model.NativeFunctionDefinition;
import com.legend.model.PackageableElement;
import com.legend.model.SynthHat;
import com.legend.parser.Dialect;
import com.legend.platform.CoreFn;
import com.legend.platform.DeclarationTable;
import com.legend.platform.Feature;
import com.legend.platform.Implementation;
import com.legend.platform.ImplementationTable;
import com.legend.platform.PlatformPure;
import com.legend.platform.Registrations;
import com.legend.platform.WalledBodies;
import java.lang.reflect.Field;
import java.util.ArrayList;
import java.util.LinkedHashMap;
import java.util.LinkedHashSet;
import java.util.List;
import java.util.Map;
import java.util.Objects;
import java.util.Optional;
import java.util.Set;
import java.util.TreeMap;
import java.util.TreeSet;
import java.util.stream.Collectors;

/**
 * The platform's dump, the second file of {@link BuiltinAnswers} (2026-11-12, docs/rust-port/PHASE3.md slice 1b):
 * {@code com.legend.platform}. Rust writes the same lines ({@code rust/lite-platform/tests/tables/platform.rs}).
 *
 * <p>{@code CoreFn} (every member with its spellings and owned FQNs, the parse-name map, {@code owning} and {@code of}
 * over every catalog FQN, every spelling, every owned FQN, the internal desugar names and a few misses), {@code Feature},
 * {@code WalledBodies}, the {@code Implementation} enums, {@code PlatformPure} (its ids, decisions, and {@code adopt} over
 * every system function: with itself bodiless, with its last parameter renamed, with the next function), the platform's
 * declarations (the catalog, the prelude's functions, the system metamodel's) as a {@code DeclarationTable}, the
 * {@code ImplementationTable} over them from the registrations the platform and builtin hold (the lowering's keys,
 * feature overrides and members empty), the same over the catalog alone, and the tables of the platform tests' cases.
 *
 * <p>A row is written by hand (an {@code Implementation} holds family classes, which the record codec has no rule
 * for): its kind, then its fields, a set as its members' names joined by {@code ,} in its order (a family by its
 * simple name). A set Java keeps in a hash structure ({@code Set.of}, {@code Map.of}, a {@code HashMap}) is written
 * sorted, and fed to a table sorted, so that no answer is in a hash order; every map a case hands the builder is a
 * {@code LinkedHashMap} in the order written here.
 */
final class PlatformTables {

    private PlatformTables() {
    }

    private static final String UPPER_ID = "meta::pure::functions::string::toUpper_String_1__String_1_";
    private static final String LOWER_ID = "meta::pure::functions::string::toLower_String_1__String_1_";
    private static final String TRIM_ID = "meta::pure::functions::string::trim_String_1__String_1_";
    private static final String LENGTH_ID = "meta::pure::functions::string::length_String_1__Integer_1_";

    static void dump(BuiltinAnswers.Out out) throws Exception {
        coreFn(out);
        feature(out);
        walledBodies(out);
        implementationEnums(out);
        platformPure(out);
        List<Function> platform = platformDeclarations();
        DeclarationTable declarations = DeclarationTable.of(platform);
        declarations(out, "platform.declarations", null, platform, declarations);
        ImplementationTable table = ImplementationTable.build(declarations, platformRegistrations());
        // THE BOOT DECLARATIONS' TABLE (the catalog, the prelude, the system metamodel), not the platform's whole one:
        // its 16 dangling registrations name upstream declarations only a loaded module supplies -- the forms'
        // meta::pure::tds::distinct and extend (core/pure/tds/tds.pure), meta::relational::validation::validate
        // (validation.pure) and meta::pure::functions::relation::eval (core_functions_relation eval.pure), the subsumed
        // createDbConfig (dbExtension.pure) and relationalExtensions (core_relational extensions/extension.pure), and
        // the 8 upstream-body and 2 refused-version decisions' ids -- each read at that name in the pinned sources
        // (2026-11-12, the owner's ruling on slice 1b's report). spec's ImplementationTableTest builds the whole table
        // (the catalog, the pinned stdlib, every upstream declaration at a registered FQN) and holds it to none.
        table(out, "platform.table", null, table, declarations);
        outside(out, table);
        DeclarationTable catalog = DeclarationTable.of(Pure.all());
        out.line("platform.catalogTable.size", catalog.size(), Pure.all().size());
        table(out, "platform.catalogTable", null, ImplementationTable.build(catalog, platformRegistrations()), catalog);
        declarationCases(out);
        implementationCases(out);
    }

    // ---------------------------------------------------------------- the enums and constants

    private static String name(Optional<? extends Enum<?>> e) {
        return e.map(Enum::name).orElse("-");
    }

    private static void coreFn(BuiltinAnswers.Out out) throws Exception {
        Field spellings = CoreFn.class.getDeclaredField("parseNames");
        spellings.setAccessible(true);
        CoreFn[] fns = CoreFn.values();
        TreeSet<String> texts = new TreeSet<>();
        for (int i = 0; i < fns.length; i++) {
            CoreFn f = fns[i];
            String[] names = (String[]) spellings.get(f);
            out.line("platform.coreFn", i, f.name(), f.parseName(), String.join(" ", names),
                    f.ownedFqns().size(), BuiltinAnswers.Out.sorted(f.ownedFqns()));
            texts.addAll(List.of(names));
            texts.addAll(f.ownedFqns());
        }
        for (Map.Entry<String, CoreFn> e : new TreeMap<>(CoreFn.parseNames()).entrySet()) {
            out.line("platform.coreFn.parseNames", e.getKey(), e.getValue().name());
        }
        for (NativeFunctionDefinition d : Pure.all()) {
            texts.add(d.qualifiedName());
        }
        for (String d : Pure.INTERNAL_DESUGAR) {
            texts.add(d);
            texts.add(Pure.Lite.PKG + d);
        }
        texts.addAll(List.of("", "::", "Filter", "filter ", "x::filter", "meta::pure::functions::collection::Filter",
                "meta::pure::functions::collection::", "meta::pure::tds", "meta::legend::lite::", "new", "toUpper",
                "meta::pure::functions::string::toUpper"));
        for (String t : texts) {
            out.line("platform.coreFn.lookup", BuiltinAnswers.Out.str(t), name(CoreFn.owning(t)), name(CoreFn.of(t)));
        }
    }

    private static void feature(BuiltinAnswers.Out out) {
        Feature[] fs = Feature.values();
        for (int i = 0; i < fs.length; i++) {
            out.line("platform.feature", i, fs[i].name());
        }
        out.line("platform.feature.fqn", Feature.FQN);
    }

    private static void walledBodies(BuiltinAnswers.Out out) {
        WalledBodies.Kind[] kinds = WalledBodies.Kind.values();
        for (int i = 0; i < kinds.length; i++) {
            out.line("platform.walledBodies.kind", i, kinds[i].name());
        }
        int i = 0;
        for (Map.Entry<String, WalledBodies.Wall> e : WalledBodies.reasons().entrySet()) {
            out.line("platform.walledBodies", i++, e.getKey(), e.getValue().kind().name(), e.getValue().why());
        }
        TreeSet<String> asked = new TreeSet<>(WalledBodies.reasons().keySet());
        asked.addAll(List.of("", "my::pkg::nothing", "meta::relational::functions::sqlQueryToString::DbConfig$prop$",
                "meta::pure::tds::schema::SchemaState", "sqlQueryToString"));
        for (String fqn : asked) {
            String r = WalledBodies.reason(fqn);
            out.line("platform.walledBodies.reason", BuiltinAnswers.Out.str(fqn), r == null ? "null" : r);
        }
        out.line("platform.walledBodies.count", WalledBodies.count());
    }

    private static void implementationEnums(BuiltinAnswers.Out out) {
        Implementation.Position[] ps = Implementation.Position.values();
        for (int i = 0; i < ps.length; i++) {
            out.line("platform.implementation.position", i, ps[i].name());
        }
        Implementation.Reason[] rs = Implementation.Reason.values();
        for (int i = 0; i < rs.length; i++) {
            out.line("platform.implementation.reason", i, rs[i].name());
        }
    }

    // ---------------------------------------------------------------- the platform's own Pure

    private static FunctionDefinition withBody(FunctionDefinition f, List<FunctionDefinition.ParameterDefinition> ps,
            List<com.legend.protocol.spec.ValueSpecification> body) {
        return new FunctionDefinition(f.qualifiedName(), f.typeParameters(), f.multiplicityParameters(), ps,
                f.returnType(), f.returnMultiplicity(), body, f.stereotypes(), f.taggedValues(), f.synthesizedFrom(),
                f.preConstraints(), f.postConstraints());
    }

    private static void adopt(BuiltinAnswers.Out out, String kase, int k, FunctionDefinition declaration,
            FunctionDefinition platform) {
        try {
            FunctionDefinition r = PlatformPure.adopt(declaration, platform);
            out.line("platform.platformPure.adopt", kase, k, "returned",
                    BuiltinAnswers.Out.sha(out.json(r, FunctionDefinition.class)));
        } catch (IllegalArgumentException e) {
            out.line("platform.platformPure.adopt", kase, k, "IllegalArgumentException", e.getMessage());
        } catch (ModelException e) {
            out.line("platform.platformPure.adopt", kase, k, "ModelException", e.phase().name(), e.element(),
                    e.getMessage());
        }
    }

    private static void platformPure(BuiltinAnswers.Out out) {
        int i = 0;
        for (FunctionId id : PlatformPure.ids()) {
            out.line("platform.platformPure.id", i++, id.qualified());
        }
        i = 0;
        for (Map.Entry<FunctionId, String> e : PlatformPure.upstreamBodies().entrySet()) {
            out.line("platform.platformPure.upstreamBody", i++, e.getKey().qualified(), e.getValue());
        }
        i = 0;
        for (Map.Entry<FunctionId, WalledBodies.Wall> e : PlatformPure.refusedVersions().entrySet()) {
            out.line("platform.platformPure.refusedVersion", i++, e.getKey().qualified(), e.getValue().kind().name(),
                    e.getValue().why());
        }
        // adopt over every system function: itself without its body, its last parameter renamed, the next function
        List<FunctionDefinition> sys = new ArrayList<>(SystemMetamodel.functionsById().values());
        for (int k = 0; k < sys.size(); k++) {
            FunctionDefinition f = sys.get(k);
            adopt(out, "bodiless", k, withBody(f, f.parameters(), List.of()), f);
            if (!f.parameters().isEmpty()) {
                List<FunctionDefinition.ParameterDefinition> ps = new ArrayList<>(f.parameters());
                FunctionDefinition.ParameterDefinition last = ps.get(ps.size() - 1);
                ps.set(ps.size() - 1, new FunctionDefinition.ParameterDefinition(last.name() + "Renamed", last.type(),
                        last.multiplicity()));
                adopt(out, "renamed", k, withBody(f, ps, f.body()), f);
            }
            adopt(out, "other", k, f, sys.get((k + 1) % sys.size()));
        }
    }

    // ---------------------------------------------------------------- the declarations

    /** The platform's declarations: the catalog, the prelude's functions, the system metamodel's, in that order. */
    private static List<Function> platformDeclarations() {
        List<Function> all = new ArrayList<>(Pure.all());
        for (PackageableElement e : Prelude.parsedModel().elements()) {
            if (e instanceof Function f) {
                all.add(f);
            }
        }
        all.addAll(SystemMetamodel.functionsById().values());
        return all;
    }

    private static List<Object> fields(String key, Object... rest) {
        List<Object> f = new ArrayList<>();
        if (key != null) {
            f.add(key);
        }
        f.addAll(List.of(rest));
        return f;
    }

    private static void declarations(BuiltinAnswers.Out out, String section, String key, List<Function> input,
            DeclarationTable d) {
        out.line(section + ".size", fields(key, input.size(), d.size()).toArray());
        int i = 0;
        for (FunctionId id : d.ids()) {
            Function f = d.get(id);
            out.line(section + ".id", fields(key, i++, id.qualified(), f.getClass().getSimpleName(),
                    BuiltinAnswers.Out.sha(out.json(f, Function.class))).toArray());
        }
        out.line(section + ".duplicates", fields(key, d.duplicates().size(),
                BuiltinAnswers.Out.join(d.duplicates())).toArray());
        TreeSet<String> fqns = new TreeSet<>();
        for (Function f : input) {
            fqns.add(f.qualifiedName());
        }
        fqns.addAll(List.of("", "my::pkg::nothing", "toUpper"));
        for (String fqn : fqns) {
            List<String> ids = new ArrayList<>();
            for (Function f : d.at(fqn)) {
                ids.add(FunctionId.of(f).qualified());
            }
            out.line(section + ".at", fields(key, BuiltinAnswers.Out.str(fqn), ids.size(),
                    BuiltinAnswers.Out.join(ids)).toArray());
        }
        out.line(section + ".get", fields(key, "no::such::function_Integer_1__Integer_1_",
                d.get(new FunctionId("no::such::function_Integer_1__Integer_1_")) == null).toArray());
    }

    // ---------------------------------------------------------------- the implementations

    /** The platform's registrations, as PlatformRegistrations assembles them but for the lowering's: no lowering
     *  keys, no feature overrides, no members. The walled natives (a {@code Map.of}) and each form's FQNs (a
     *  {@code Set.of}) are handed sorted. */
    private static Registrations platformRegistrations() {
        Map<Class<? extends NativeFn.Member>, List<NativeFunctionDefinition>> families = new LinkedHashMap<>();
        for (List<? extends NativeFn.Member> members : NativeFn.families().values()) {
            for (NativeFn.Member m : members) {
                Class<? extends NativeFn.Member> family = ((Enum<?>) m).getDeclaringClass()
                        .asSubclass(NativeFn.Member.class);
                families.computeIfAbsent(family, k -> new ArrayList<>()).addAll(m.overloads());
            }
        }
        Map<CoreFn, Set<String>> forms = new LinkedHashMap<>();
        for (CoreFn form : CoreFn.values()) {
            if (!form.ownedFqns().isEmpty()) {
                forms.put(form, new LinkedHashSet<>(new TreeSet<>(form.ownedFqns())));
            }
        }
        Map<String, String> walledNatives = new LinkedHashMap<>();
        for (String fqn : new TreeSet<>(Pure.walledNativeFqns())) {
            walledNatives.put(fqn, Objects.requireNonNull(Pure.walledNativeReason(fqn)));
        }
        Set<String> subsumed = new LinkedHashSet<>();
        for (Subsumed s : Subsumed.values()) {
            subsumed.add(s.fqn());
        }
        return new Registrations(Pure.all(), Map.of(), Map.of(), families, forms, walledNatives,
                WalledBodies.reasons(), subsumed, Map.of(), PlatformPure.ids(), PlatformPure.upstreamBodies(),
                PlatformPure.refusedVersions());
    }

    private static String names(Set<? extends Enum<?>> s) {
        return s.stream().map(Enum::name).collect(Collectors.joining(","));
    }

    private static String families(Set<Class<? extends NativeFn.Member>> s) {
        return s.stream().map(Class::getSimpleName).collect(Collectors.joining(","));
    }

    /** A row: its kind, then its fields. */
    private static List<Object> row(Implementation r) {
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

    private static void table(BuiltinAnswers.Out out, String section, String key, ImplementationTable t,
            DeclarationTable d) {
        out.line(section + ".size", fields(key, t.rows().size()).toArray());
        int i = 0;
        for (Map.Entry<FunctionId, Implementation> e : t.rows().entrySet()) {
            List<Object> f = fields(key, i++, e.getKey().qualified(), t.runsByRule(d.get(e.getKey())),
                    t.of(e.getKey()) == e.getValue());
            f.addAll(row(e.getValue()));
            out.line(section + ".row", f.toArray());
        }
        list(out, section + ".dangling", key, t.dangling());
        list(out, section + ".conflicts", key, t.conflicts());
        list(out, section + ".memberWalls", key, t.memberWalls());
    }

    private static void list(BuiltinAnswers.Out out, String section, String key, List<String> values) {
        out.line(section + ".count", fields(key, values.size()).toArray());
        for (int i = 0; i < values.size(); i++) {
            out.line(section, fields(key, i, values.get(i)).toArray());
        }
    }

    /** rowOf and runsByRule for declarations outside the table, and for none. */
    private static void outside(BuiltinAnswers.Out out, ImplementationTable t) {
        FunctionDefinition fd = (FunctionDefinition) declare(
                "function my::pkg::outside(x:Integer[1]):Integer[1] { $x }").get(0);
        NativeFunctionDefinition upper = catalog(UPPER_ID);
        NativeFunctionDefinition nfd = new NativeFunctionDefinition("my::pkg::outsideNative", upper.typeParameters(),
                upper.multiplicityParameters(), upper.parameters(), upper.returnType(), upper.returnMultiplicity(),
                upper.stereotypes(), upper.taggedValues());
        for (Function f : List.of(fd, nfd)) {
            List<Object> fs = fields(f.getClass().getSimpleName(), FunctionId.of(f).qualified(),
                    t.of(FunctionId.of(f)) == null, t.runsByRule(f));
            fs.addAll(row(t.rowOf(f)));
            out.line("platform.table.outside", fs.toArray());
        }
        out.line("platform.table.outside", "null", t.runsByRule(null));
    }

    // ---------------------------------------------------------------- the tests' cases

    private static NativeFunctionDefinition catalog(String id) {
        return Objects.requireNonNull(Pure.nativeFunctionById(id), id);
    }

    /** The functions of a text at the engine's dialect (the tests' {@code Engine.model}). */
    private static List<Function> declare(String source) {
        List<Function> out = new ArrayList<>();
        for (PackageableElement e : ModelFromText.parse(source, Dialect.LEGEND_ENGINE).elements()) {
            if (e instanceof Function f) {
                out.add(f);
            }
        }
        return out;
    }

    private static List<Function> concat(List<? extends Function> a, List<? extends Function> b) {
        List<Function> out = new ArrayList<>(a);
        out.addAll(b);
        return out;
    }

    private static void declarationCases(BuiltinAnswers.Out out) {
        NativeFunctionDefinition upper = catalog(UPPER_ID);
        String bodied = "function meta::pure::functions::string::toUpper(source:String[1]):String[1] { $source }";
        String one = "function my::pkg::one(x:Integer[1]):Integer[1] { $x }";
        Map<String, List<Function>> cases = new LinkedHashMap<>();
        cases.put("nativeThenBodied", concat(List.of(upper), declare(bodied)));
        cases.put("bodiedThenNative", concat(declare(bodied), List.of(upper)));
        cases.put("nativeTwice", List.of(upper, upper));
        cases.put("overloads", declare("function my::pkg::twice(x:Integer[1]):Integer[1] { $x }\n"
                + "function my::pkg::twice(x:String[1]):String[1] { $x }"));
        cases.put("sameBodyTwice", concat(declare(one), declare(one)));
        cases.put("twoBodies", concat(concat(declare(one), declare("function my::pkg::one(x:Integer[1]):Integer[1] { 1 }")),
                declare("function my::pkg::one(x:Integer[1]):Integer[1] { 2 }")));
        cases.put("empty", List.of());
        for (Map.Entry<String, List<Function>> c : cases.entrySet()) {
            declarations(out, "platform.case.declarations", c.getKey(), c.getValue(), DeclarationTable.of(c.getValue()));
        }
    }

    private static Registrations registrations(List<NativeFunctionDefinition> catalog,
            Map<Implementation.Position, Set<FunctionId>> keys, Map<Feature, Set<FunctionId>> overrides,
            Map<Class<? extends NativeFn.Member>, List<NativeFunctionDefinition>> families,
            Map<CoreFn, Set<String>> forms, Map<String, String> walledNatives, Map<String, WalledBodies.Wall> walls,
            Set<String> subsumed, Map<Class<? extends NativeFn.Member>, Set<ClassMember>> members,
            Set<FunctionId> platformPure, Map<FunctionId, String> upstreamBodies,
            Map<FunctionId, WalledBodies.Wall> refusedVersions) {
        return new Registrations(catalog, keys, overrides, families, forms, walledNatives, walls, subsumed, members,
                platformPure, upstreamBodies, refusedVersions);
    }

    /** No registrations but the catalog's. */
    private static Registrations none(List<NativeFunctionDefinition> catalog) {
        return registrations(catalog, Map.of(), Map.of(), Map.of(), Map.of(), Map.of(), Map.of(), Set.of(), Map.of(),
                Set.of(), Map.of(), Map.of());
    }

    private static FunctionId id(Function f) {
        return FunctionId.of(f);
    }

    @SafeVarargs
    private static <T> Set<T> set(T... values) {
        return new LinkedHashSet<>(List.of(values));
    }

    private static <K, V> Map<K, V> map(Object... kvs) {
        Map<K, V> out = new LinkedHashMap<>();
        for (int i = 0; i < kvs.length; i += 2) {
            @SuppressWarnings("unchecked")
            K k = (K) kvs[i];
            @SuppressWarnings("unchecked")
            V v = (V) kvs[i + 1];
            out.put(k, v);
        }
        return out;
    }

    private static FunctionDefinition fd(String source) {
        return (FunctionDefinition) declare(source).get(0);
    }

    private static void implementationCases(BuiltinAnswers.Out out) {
        NativeFunctionDefinition upper = catalog(UPPER_ID);
        NativeFunctionDefinition lower = catalog(LOWER_ID);
        NativeFunctionDefinition trim = catalog(TRIM_ID);
        NativeFunctionDefinition length = catalog(LENGTH_ID);
        Map<String, Object[]> cases = new LinkedHashMap<>();

        // ImplementationTableTest's cases, each map in the order the test writes it
        cases.put("defaults", new Object[] {
            concat(List.of(upper), declare("function my::pkg::twice(x:Integer[1]):Integer[1] { $x }")),
            none(List.of(upper))});
        cases.put("loweringKey", new Object[] {List.of(upper),
            registrations(List.of(upper), map(Implementation.Position.SCALAR, set(id(upper))), Map.of(), Map.of(),
                    Map.of(), Map.of(), Map.of(), Set.of(), Map.of(), Set.of(), Map.of(), Map.of())});
        cases.put("danglingKey", new Object[] {List.of(upper),
            registrations(List.of(upper),
                    map(Implementation.Position.SCALAR, set(new FunctionId("no::such::function_String_1__String_1_"))),
                    Map.of(), Map.of(), Map.of(), Map.of(), Map.of(), Set.of(), Map.of(), Set.of(), Map.of(), Map.of())});
        cases.put("familyAndForm", new Object[] {List.of(upper, lower),
            registrations(List.of(upper, lower), Map.of(), Map.of(), map(NativeFn.Verdict.class, List.of(upper)),
                    map(CoreFn.FILTER, set(lower.qualifiedName())), Map.of(), Map.of(), Set.of(), Map.of(), Set.of(),
                    Map.of(), Map.of())});
        cases.put("formWithRule", new Object[] {List.of(upper),
            registrations(List.of(upper), map(Implementation.Position.SCALAR, set(id(upper))), Map.of(), Map.of(),
                    map(CoreFn.MAP, set(upper.qualifiedName())), Map.of(), Map.of(), Set.of(), Map.of(), Set.of(),
                    Map.of(), Map.of())});
        cases.put("refusalBesideImplementation", new Object[] {List.of(upper),
            registrations(List.of(upper), map(Implementation.Position.SCALAR, set(id(upper))), Map.of(), Map.of(),
                    Map.of(), map(upper.qualifiedName(), "an effect"), Map.of(), Set.of(), Map.of(), Set.of(),
                    Map.of(), Map.of())});
        cases.put("twoForms", new Object[] {List.of(upper),
            registrations(List.of(upper), Map.of(), Map.of(), Map.of(),
                    map(CoreFn.MAP, set(upper.qualifiedName()), CoreFn.FILTER, set(upper.qualifiedName())),
                    Map.of(), Map.of(), Set.of(), Map.of(), Set.of(), Map.of(), Map.of())});
        cases.put("refusalKinds", new Object[] {
            concat(List.of(upper), declare("function my::pkg::printer(x:Integer[1]):Integer[1] { $x }")),
            registrations(List.of(upper), Map.of(), Map.of(), Map.of(), Map.of(),
                    map(upper.qualifiedName(), "an effect"),
                    map("my::pkg::printer", new WalledBodies.Wall(WalledBodies.Kind.ENGINE_MACHINERY, "the printer"),
                            "my::pkg::Cls$prop$derived", new WalledBodies.Wall(WalledBodies.Kind.CANNOT_IMPLEMENT, "x")),
                    Set.of(), Map.of(), Set.of(), Map.of(), Map.of())});
        cases.put("subsumedDangling", new Object[] {List.of(upper),
            registrations(List.of(upper), Map.of(), Map.of(), Map.of(), Map.of(), Map.of(), Map.of(),
                    set("no::such::program"), Map.of(), Set.of(), Map.of(), Map.of())});

        // the members a family implements, matched by a lifted declaration's provenance
        FunctionDefinition derived = fd("function my::pkg::derived(x:Integer[1]):Integer[1] { $x }")
                .withSynthesizedFrom(new FunctionDefinition.Synthesized(SynthHat.PROP, "my::pkg::Cls", "derived"));
        FunctionDefinition other = fd("function my::pkg::other(x:Integer[1]):Integer[1] { $x }")
                .withSynthesizedFrom(new FunctionDefinition.Synthesized(SynthHat.PROP, "my::pkg::Cls", "other"));
        FunctionDefinition constraint = fd("function my::pkg::constraint(x:Integer[1]):Integer[1] { $x }")
                .withSynthesizedFrom(new FunctionDefinition.Synthesized(SynthHat.CONSTRAINT, "my::pkg::Cls", "derived"));
        FunctionDefinition plain = fd("function my::pkg::plain(x:Integer[1]):Integer[1] { $x }");
        cases.put("members", new Object[] {List.of(derived, other, constraint, plain),
            registrations(List.of(), Map.of(), Map.of(), Map.of(), Map.of(), Map.of(), Map.of(), Set.of(),
                    map(NativeFn.JavaRoutine.class, set(new ClassMember("my::pkg::Cls", "derived")),
                            NativeFn.RowGetter.class, set(new ClassMember("my::pkg::Cls", "derived"),
                                    new ClassMember("my::pkg::Cls", "other"))),
                    Set.of(), Map.of(), Map.of())});

        // every conflict and every dangling registration the builder reports
        FunctionDefinition own = fd("function my::pkg::own(x:Integer[1]):Integer[1] { $x }");
        FunctionDefinition ownOther = fd("function my::pkg::own(x:String[1]):String[1] { $x }");
        FunctionDefinition formed = fd("function my::pkg::formed(x:Integer[1]):Integer[1] { $x }");
        FunctionDefinition walled = fd("function my::pkg::walled(x:Integer[1]):Integer[1] { $x }");
        FunctionDefinition lowered = fd("function my::pkg::lowered(x:Integer[1]):Integer[1] { $x }");
        FunctionDefinition refusedVersion = fd("function my::pkg::refusedVersion(x:Integer[1]):Integer[1] { $x }");
        FunctionDefinition formRefused = fd("function my::pkg::formRefused(x:Integer[1]):Integer[1] { $x }");
        FunctionDefinition upperVersion = fd(
                "function meta::pure::functions::string::toUpper(source:String[1], n:Integer[1]):String[1] { $source }");
        FunctionDefinition lowerVersion = fd(
                "function meta::pure::functions::string::toLower(source:String[1], n:Integer[1]):String[1] { $source }");
        FunctionDefinition ownBody = fd("function my::pkg::ownBody(x:Integer[1]):Integer[1] { $x }");
        FunctionDefinition formOwn = fd("function my::pkg::formOwn(x:Integer[1]):Integer[1] { $x }");
        FunctionDefinition refusedBody = fd("function my::pkg::refusedBody(x:Integer[1]):Integer[1] { $x }");
        List<Function> ds = List.of(upper, lower, trim, own, ownOther, formed, walled, lowered, refusedVersion,
                formRefused, upperVersion, lowerVersion, ownBody, formOwn, refusedBody);
        cases.put("conflicts", new Object[] {ds,
            registrations(List.of(upper, lower, trim, length),
                    map(Implementation.Position.AGGREGATE, set(id(upper), id(lowered)),
                            Implementation.Position.SCALAR, set(id(upper), id(own),
                                    new FunctionId("no::such::scalar_Integer_1__Integer_1_"))),
                    map(Feature.CORRECT_SQL_SUBSTRING_INDEXING, set(id(upper),
                                    new FunctionId("no::such::feature_Integer_1__Integer_1_")),
                            Feature.LEGACY_SQL_NULL_UNSAFE_EQUALS, set(id(lower))),
                    map(NativeFn.Verdict.class, List.of(upper, trim, length), NativeFn.Calendar.class, List.of(upper)),
                    map(CoreFn.FILTER, set("my::pkg::formed", "my::pkg::nowhere", "my::pkg::formOwn"),
                            CoreFn.MAP, set("my::pkg::formRefused")),
                    map(trim.qualifiedName(), "an effect", "my::pkg::noNative", "none"),
                    map("my::pkg::walled", new WalledBodies.Wall(WalledBodies.Kind.ENGINE_MACHINERY, "the printer"),
                            "my::pkg::Cls$prop$m", new WalledBodies.Wall(WalledBodies.Kind.CANNOT_IMPLEMENT, "member")),
                    set("my::pkg::formRefused", "no::such::program", "my::pkg::refusedBody"),
                    Map.of(),
                    set(id(own), id(walled), id(upper), new FunctionId("no::such::pure_Integer_1__Integer_1_"),
                            id(ownBody), id(formOwn)),
                    map(id(lowerVersion), "decided", id(formed), "formed", id(lowered), "lowered", id(upper), "native",
                            new FunctionId("no::such::body_Integer_1__Integer_1_"), "none", id(ownBody), "own",
                            id(refusedBody), "refused"),
                    map(id(refusedVersion), new WalledBodies.Wall(WalledBodies.Kind.CANNOT_IMPLEMENT, "refused"),
                            new FunctionId("no::such::version_Integer_1__Integer_1_"),
                            new WalledBodies.Wall(WalledBodies.Kind.ENGINE_MACHINERY, "none")))});

        for (Map.Entry<String, Object[]> c : cases.entrySet()) {
            @SuppressWarnings("unchecked")
            List<Function> declared = (List<Function>) c.getValue()[0];
            Registrations r = (Registrations) c.getValue()[1];
            DeclarationTable d = DeclarationTable.of(declared);
            table(out, "platform.case", c.getKey(), ImplementationTable.build(d, r), d);
        }
    }
}
