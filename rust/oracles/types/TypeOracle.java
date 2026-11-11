// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

import com.legend.compiler.element.type.ExprType;
import com.legend.compiler.element.type.Multiplicity;
import com.legend.compiler.element.type.PlatformTypes;
import com.legend.compiler.element.type.Type;
import com.legend.sql.SqlType;
import java.io.IOException;
import java.lang.reflect.Field;
import java.lang.reflect.Modifier;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.Collection;
import java.util.List;
import java.util.Map;
import java.util.Optional;
import java.util.Set;
import java.util.TreeMap;
import java.util.TreeSet;
import java.util.function.Supplier;

/**
 * THE TYPE FAMILY'S ANSWERS, frozen before the Java is deleted (docs/rust-port/PHASE3.md, slice 1a, 2026-11-11): every
 * operation of core's {@code compiler_element_type} layer (core/src/main/java/com/legend/compiler/element/type/) that
 * needs no other layer than {@code sql}, asked of Java over a set of inputs, one answer per line, for
 * rust/lite-types' differential ({@code tests/type_differential.rs}). The platform types are all here: every
 * {@code Type.Primitive}, every FQN {@code Type.Primitive.byFqn()} knows (in its order), every constant of
 * {@code PlatformTypes} (each as a class, and each string to every string predicate), the carriers with zero, one and
 * two arguments; and every type against every type for equality.
 *
 * <p>Not asked, because they need a layer the Rust port has not reached (rust/README.md, docs/rust-port/PHASE3_CRATES.md):
 * {@code PlatformTypes.eraseTdsRow} ({@code builtin}'s {@code NativeFn.RowGetter}), {@code handleRowClass} and
 * {@code isStatementOnly} ({@code model}'s {@code FunctionId}, {@code builtin}'s {@code NativeFn}), and
 * {@code Multiplicity.from} (the parser's protocol records). Subtyping is not a {@code Type} operation:
 * {@code ModelContext.isSubtype} walks a compiled model's classes.
 *
 * <p>A line is {@code op TAB argument... TAB answer}, every field JSON: an argument is a sample's index, a JSON value
 * or a type; an answer is JSON, a refusal {@code {"throws":"<simple class name>","message":"..."}}. A type is
 * {@code ["P","INTEGER"]}, {@code ["D",p,s]}, {@code ["C",fqn]}, {@code ["E",fqn]}, {@code ["V",name]},
 * {@code ["G",raw,[types],[multiplicities]]}, {@code ["F",[[type,mult]...],[type,mult]]},
 * {@code ["R",[[name,type,mult]...],[[name,type,mult]...]]}, {@code ["S",left,"OP",right]}; a multiplicity
 * {@code [lower,upper]} (upper null when unbounded) or a variable's name as a string. Run on the tree's built jars
 * with the build's JDK, from the tree's root (the Rust library parses the platform's declarations when
 * {@code Type.Primitive} first loads):
 * <pre>
 *   bazel build //core:compiler_element_type //core:rust_parser //core:ffi //rust/lite-ffi:lite_ffi
 *   $(bazel info output_base)/external/rules_java++toolchains+remotejdk25_&lt;os&gt;_&lt;arch&gt;/bin/java \
 *     --enable-native-access=ALL-UNNAMED -Dlegend.lite.library=bazel-bin/rust/lite-ffi/liblite_ffi.dylib \
 *     -cp &lt;the runtime jars of //core:compiler_element_type&gt; \
 *     rust/oracles/types/TypeOracle.java rust/oracles/types/type-facts.tsv
 * </pre>
 * The runtime jars: {@code bazel cquery //core:compiler_element_type --output=starlark
 * '--starlark:expr=":".join([f.path for f in
 * providers(target)["@@rules_java+//java/private:java_info.bzl%JavaInfo"].transitive_runtime_jars.to_list()])'}.
 */
public final class TypeOracle {

    private TypeOracle() {
    }

    private static final List<String> LINES = new ArrayList<>();
    private static final Map<String, Integer> COUNTS = new TreeMap<>();
    private static final List<Type> TYPES = new ArrayList<>();
    private static final List<Multiplicity> MULTS = new ArrayList<>();
    private static final List<Type.PrecisionDecimal> DECIMALS = new ArrayList<>();
    private static final Set<String> STRINGS = new TreeSet<>();

    // ---- JSON, written by hand: Java's one escape table (quote, backslash, the named controls, every other control
    // as \\u00xx), everything else as is

    static String str(String s) {
        StringBuilder b = new StringBuilder("\"");
        for (int i = 0; i < s.length(); i++) {
            char c = s.charAt(i);
            switch (c) {
                case '"' -> b.append("\\\"");
                case '\\' -> b.append("\\\\");
                case '\n' -> b.append("\\n");
                case '\r' -> b.append("\\r");
                case '\t' -> b.append("\\t");
                case '\b' -> b.append("\\b");
                case '\f' -> b.append("\\f");
                default -> {
                    if (c < 0x20) {
                        b.append(String.format("\\u%04x", (int) c));
                    } else {
                        b.append(c);
                    }
                }
            }
        }
        return b.append('"').toString();
    }

    static String mult(Multiplicity m) {
        return switch (m) {
            case Multiplicity.Bounded b -> "[" + b.lower() + "," + (b.upper() == null ? "null" : b.upper()) + "]";
            case Multiplicity.Var v -> str(v.name());
        };
    }

    static String type(Type t) {
        return switch (t) {
            case Type.Primitive p -> "[\"P\"," + str(p.name()) + "]";
            case Type.PrecisionDecimal d -> "[\"D\"," + d.precision() + "," + d.scale() + "]";
            case Type.ClassType c -> "[\"C\"," + str(c.fqn()) + "]";
            case Type.EnumType e -> "[\"E\"," + str(e.fqn()) + "]";
            case Type.TypeVar v -> "[\"V\"," + str(v.name()) + "]";
            case Type.GenericType g -> "[\"G\"," + str(g.rawFqn()) + "," + list(g.arguments(), TypeOracle::type) + ","
                    + list(g.multArguments(), TypeOracle::mult) + "]";
            case Type.FunctionType f -> "[\"F\"," + list(f.params(), TypeOracle::param) + "," + param(f.result()) + "]";
            case Type.RelationType r -> "[\"R\"," + list(r.columns(), TypeOracle::column) + ","
                    + list(r.dynamicColumns(), TypeOracle::column) + "]";
            case Type.SchemaAlgebra s -> "[\"S\"," + type(s.left()) + "," + str(s.op().name()) + "," + type(s.right())
                    + "]";
        };
    }

    static String param(Type.Param p) {
        return "[" + type(p.type()) + "," + mult(p.multiplicity()) + "]";
    }

    static String column(Type.Column c) {
        return "[" + str(c.name()) + "," + type(c.type()) + "," + mult(c.multiplicity()) + "]";
    }

    static <T> String list(Collection<T> xs, java.util.function.Function<T, String> f) {
        StringBuilder b = new StringBuilder("[");
        for (T x : xs) {
            if (b.length() > 1) {
                b.append(',');
            }
            b.append(f.apply(x));
        }
        return b.append(']').toString();
    }

    /** A value as its answer's JSON. */
    static String value(Object v) {
        if (v == null) {
            return "null";
        }
        if (v instanceof Optional<?> o) {
            return o.isPresent() ? value(o.get()) : "null";
        }
        if (v instanceof Boolean || v instanceof Integer) {
            return v.toString();
        }
        if (v instanceof String s) {
            return str(s);
        }
        if (v instanceof Type t) {
            // Type.Primitive is an enum and a Type: a type
            return type(t);
        }
        if (v instanceof Multiplicity m) {
            return mult(m);
        }
        if (v instanceof Type.Column c) {
            return column(c);
        }
        if (v instanceof Type.Primitive.TypeVariable tv) {
            return "[" + str(tv.name()) + "," + str(tv.type().name()) + "]";
        }
        if (v instanceof Enum<?> e) {
            return str(e.name());
        }
        if (v instanceof Collection<?> c) {
            return list(c, TypeOracle::value);
        }
        throw new IllegalArgumentException("no answer form for " + v.getClass());
    }

    /** The answer of {@code f}: its value, or what it throws. */
    static String answer(Supplier<?> f) {
        try {
            return value(f.get());
        } catch (RuntimeException e) {
            return "{\"throws\":" + str(e.getClass().getSimpleName()) + ",\"message\":"
                    + str(String.valueOf(e.getMessage())) + "}";
        }
    }

    static void line(String op, String... fields) {
        LINES.add(op + "\t" + String.join("\t", fields));
        COUNTS.merge(op, 1, Integer::sum);
    }

    // ---- the samples

    static Multiplicity.Bounded b(int lower, Integer upper) {
        return new Multiplicity.Bounded(lower, upper);
    }

    static Type.Column col(String name, Type t, Multiplicity m) {
        return new Type.Column(name, t, m);
    }

    static Type.Param par(Type t, Multiplicity m) {
        return new Type.Param(t, m);
    }

    static Type.ClassType cls(String fqn) {
        return new Type.ClassType(fqn);
    }

    static Type.GenericType gen(String raw, Type... args) {
        return new Type.GenericType(raw, List.of(args));
    }

    static void samples() {
        MULTS.addAll(List.of(Multiplicity.Bounded.ONE, Multiplicity.Bounded.ZERO_ONE, Multiplicity.Bounded.ZERO_MANY,
                Multiplicity.Bounded.ONE_MANY, b(0, 0), b(2, 2), b(3, 7), b(2, null), b(0, 2), b(65536, 65536),
                b(0, Integer.MAX_VALUE), b(Integer.MAX_VALUE, Integer.MAX_VALUE), b(Integer.MAX_VALUE, null),
                b(46341, 46341), b(1, 2), new Multiplicity.Var("m"), new Multiplicity.Var("n")));

        // the decimals: the defaults, the clamp's both arms ((49,15) -> (38,6), (49,5) -> (38,5)), the division's
        // floor, and Java's int arithmetic past its range
        for (int[] ps : new int[][] {{38, 18}, {10, 2}, {0, 0}, {1, 0}, {1, 1}, {5, 5}, {38, 0}, {38, 38}, {18, 2},
                {24, 15}, {24, 5}, {30, 2}, {2, 1}, {12, 0}, {20, 10}, {1000, 500}, {Integer.MAX_VALUE, 0},
                {Integer.MAX_VALUE, 1}, {Integer.MAX_VALUE, Integer.MAX_VALUE}, {1073741824, 0}}) {
            DECIMALS.add(new Type.PrecisionDecimal(ps[0], ps[1]));
        }

        Type integer = Type.Primitive.INTEGER;
        Type string = Type.Primitive.STRING;
        Type any = cls(PlatformTypes.ANY);
        Type tv = new Type.TypeVar("T");
        Type.RelationType ab = new Type.RelationType(List.of(col("a", integer, Multiplicity.Bounded.ONE),
                col("b", string, Multiplicity.Bounded.ZERO_ONE)));
        Type.RelationType a = new Type.RelationType(List.of(col("a", integer, Multiplicity.Bounded.ONE)));
        Type.RelationType pivot = new Type.RelationType(List.of(col("name", string, Multiplicity.Bounded.ONE),
                col("'x__|__sum'", integer, Multiplicity.Bounded.ZERO_ONE)),
                List.of(col("sum", integer, Multiplicity.Bounded.ZERO_ONE),
                        col("cnt", Type.Primitive.FLOAT, Multiplicity.Bounded.ONE)));
        Type.FunctionType f0 = new Type.FunctionType(List.of(), par(integer, Multiplicity.Bounded.ONE));
        Type.FunctionType f2 = new Type.FunctionType(List.of(par(integer, Multiplicity.Bounded.ONE),
                par(string, Multiplicity.Bounded.ZERO_ONE)), par(Type.Primitive.BOOLEAN, Multiplicity.Bounded.ZERO_MANY));
        Type.FunctionType fm = new Type.FunctionType(List.of(par(tv, new Multiplicity.Var("m"))),
                par(tv, new Multiplicity.Var("m")));

        TYPES.addAll(List.of(Type.Primitive.values()));
        for (Type.PrecisionDecimal d : List.of(DECIMALS.get(0), DECIMALS.get(1), DECIMALS.get(2), DECIMALS.get(7),
                Type.PrecisionDecimal.DEFAULT_DECIMAL)) {
            TYPES.add(d);
        }
        // every PlatformTypes constant as a class (the type FQNs among them are the platform's nominal types), and the
        // FQNs Type.Primitive.byFqn() knows, and the names the predicates must not take by a suffix
        for (String fqn : stringConstants(PlatformTypes.class).values()) {
            TYPES.add(cls(fqn));
        }
        for (String fqn : Type.Primitive.byFqn().keySet()) {
            TYPES.add(cls(fqn));
        }
        for (String fqn : List.of("my::app::Person", "my::domain::List", "List", "Any", "", "a:::b", "::x", "x::",
                "meta::pure::metamodel::type::Type", "meta::pure::metamodel::type::Integer")) {
            TYPES.add(cls(fqn));
        }
        TYPES.add(cls(PlatformTypes.ANY));
        TYPES.addAll(List.of(new Type.EnumType("my::Color"), new Type.EnumType(PlatformTypes.ENUM),
                new Type.EnumType(PlatformTypes.ANY), new Type.TypeVar("T"), new Type.TypeVar("V"),
                new Type.TypeVar("?"), new Type.TypeVar("")));
        // the carriers: each relation carrier over a schema, a variable and two arguments; the value carriers with
        // zero, one and two arguments
        for (String raw : new TreeSet<>(PlatformTypes.RELATION_CARRIERS)) {
            TYPES.addAll(List.of(gen(raw, ab), gen(raw, tv), gen(raw, ab, a), gen(raw)));
        }
        TYPES.add(Type.relation(a));
        TYPES.add(Type.relation(a));
        for (String raw : List.of(PlatformTypes.LIST, PlatformTypes.PAIR, "meta::pure::functions::collection::Map",
                PlatformTypes.FUNCTION, PlatformTypes.FUNCTION_DEFINITION, PlatformTypes.LAMBDA_FUNCTION,
                PlatformTypes.CONCRETE_FUNCTION_DEFINITION, PlatformTypes.RESULT, PlatformTypes.VARIANT,
                PlatformTypes.COL_SPEC, PlatformTypes.COL_SPEC_ARRAY, PlatformTypes.FUNC_COL_SPEC,
                PlatformTypes.FUNC_COL_SPEC_ARRAY, PlatformTypes.AGG_COL_SPEC, PlatformTypes.AGG_COL_SPEC_ARRAY,
                PlatformTypes.TABULAR_DATA_SET, PlatformTypes.CLASS_METACLASS, PlatformTypes.WINDOW,
                "meta::pure::metamodel::function::property::Property", "my::app::Box", "my::domain::List", "Relation",
                "meta::pure::mapping::EnumerationMapping")) {
            TYPES.addAll(List.of(gen(raw), gen(raw, string), gen(raw, string, integer)));
        }
        TYPES.addAll(List.of(gen(PlatformTypes.FUNCTION, f2), gen(PlatformTypes.LAMBDA_FUNCTION, f0),
                gen(PlatformTypes.FUNCTION_DEFINITION, any), gen(PlatformTypes.FUNCTION, f2, f0),
                PlatformTypes.lambdaType(f2), PlatformTypes.concreteFunctionDefinitionType(fm),
                new Type.GenericType(PlatformTypes.RESULT, List.of(tv), List.of(new Multiplicity.Var("m"))),
                new Type.GenericType(PlatformTypes.RESULT, List.of(cls("X")), List.of(Multiplicity.Bounded.ONE)),
                new Type.GenericType("meta::pure::metamodel::function::property::Property",
                        List.of(cls(PlatformTypes.NIL), any), List.of(Multiplicity.Bounded.ZERO_MANY)),
                gen(PlatformTypes.CLASS_METACLASS, any), gen(PlatformTypes.COL_SPEC, a),
                gen(PlatformTypes.WINDOW, tv)));
        TYPES.addAll(List.of(f0, f2, fm, new Type.FunctionType(List.of(par(Type.relation(ab), Multiplicity.Bounded.ONE)),
                par(ab, Multiplicity.Bounded.ZERO_MANY))));
        TYPES.addAll(List.of(new Type.RelationType(List.of()), a, ab, pivot,
                new Type.RelationType(List.of(col("price", new Type.PrecisionDecimal(38, 2), Multiplicity.Bounded.ONE))),
                Type.RelationType.lateBound(), Type.RelationType.erasedRow(PlatformTypes.TDS_ROW),
                Type.RelationType.erasedRow("my::Row"), Type.RelationType.erasedRow(PlatformTypes.ANY),
                // the late-bound template written out again: equal to the grid's, so not an erased row
                new Type.RelationType(List.of(), List.of(col("*", any, Multiplicity.Bounded.ZERO_ONE))),
                new Type.RelationType(List.of(), List.of(col("*", any, Multiplicity.Bounded.ONE))),
                new Type.RelationType(List.of(), List.of(col("x", any, Multiplicity.Bounded.ZERO_ONE))),
                new Type.RelationType(List.of(col("a", integer, Multiplicity.Bounded.ONE)),
                        List.of(col("*", any, Multiplicity.Bounded.ZERO_ONE))),
                new Type.RelationType(List.of(), List.of(col("*", any, Multiplicity.Bounded.ZERO_ONE),
                        col("y", any, Multiplicity.Bounded.ZERO_ONE))),
                new Type.RelationType(List.of(col("t", Type.relation(a), Multiplicity.Bounded.ONE),
                        col("first name", string, new Multiplicity.Var("m")), col("tab\there", string,
                                Multiplicity.Bounded.ONE), col("été⊆", string, Multiplicity.Bounded.ONE))),
                a));
        Type v = new Type.TypeVar("V");
        Type z = new Type.TypeVar("Z");
        TYPES.addAll(List.of(new Type.SchemaAlgebra(tv, Type.Op.UNION, v),
                new Type.SchemaAlgebra(tv, Type.Op.DIFFERENCE, z), new Type.SchemaAlgebra(z, Type.Op.SUBSET, tv),
                new Type.SchemaAlgebra(z, Type.Op.EQUAL, new Type.TypeVar("K")),
                new Type.SchemaAlgebra(new Type.SchemaAlgebra(tv, Type.Op.UNION, v), Type.Op.DIFFERENCE, ab),
                new Type.SchemaAlgebra(a, Type.Op.UNION, gen(PlatformTypes.LIST, string))));

        // the strings every string predicate is asked: each constant, its simple name, and near misses
        for (String s : stringConstants(PlatformTypes.class).values()) {
            STRINGS.add(s);
            STRINGS.add(s.substring(s.lastIndexOf("::") + 2));
            STRINGS.add(s + "x");
        }
        STRINGS.addAll(Type.Primitive.byFqn().keySet());
        STRINGS.addAll(List.of("", "plus", "DatabaseConnection", "RelationalDatabaseConnection",
                "TestDatabaseConnection", "ModelChainConnection", "JsonModelConnection", "sqlQueryPostProcessors",
                "sqlQueryPostProcessorsConnectionAware", "queryPostProcessorsWithParameter", "meta::pure::functions::asserts::",
                "meta::pure::functions::asserts::assertEq", "meta::pure::functions::assertsX", "x__|__y", "'x__|__y'",
                "'x__|__y", "x__|__y'", "'", "''", "__|__", "a b", "é::⊆", "Integer", "String", "Decimal",
                "meta::pure::precisePrimitives::Int", "meta::pure::precisePrimitives::Varchar",
                "meta::pure::precisePrimitives::Numeric", "meta::pure::precisePrimitives::Bogus", "my::Row"));
    }

    /** Every public static final String field of {@code c}, by name. */
    static Map<String, String> stringConstants(Class<?> c) {
        Map<String, String> out = new TreeMap<>();
        for (Field f : c.getDeclaredFields()) {
            int m = f.getModifiers();
            if (Modifier.isPublic(m) && Modifier.isStatic(m) && Modifier.isFinal(m) && f.getType() == String.class) {
                try {
                    out.put(f.getName(), (String) f.get(null));
                } catch (IllegalAccessException e) {
                    throw new IllegalStateException(e);
                }
            }
        }
        return out;
    }

    static Multiplicity.Bounded bounded(int i) {
        return (Multiplicity.Bounded) MULTS.get(i);
    }

    public static void main(String[] args) throws IOException {
        if (args.length != 1) {
            throw new IllegalArgumentException("usage: TypeOracle <out.tsv>");
        }
        samples();

        // ---- the samples themselves
        for (int i = 0; i < TYPES.size(); i++) {
            line("type", Integer.toString(i), type(TYPES.get(i)));
        }
        for (int i = 0; i < MULTS.size(); i++) {
            line("mult", Integer.toString(i), mult(MULTS.get(i)));
        }
        for (int i = 0; i < DECIMALS.size(); i++) {
            line("decimal", Integer.toString(i), type(DECIMALS.get(i)));
        }

        // ---- the constants: every public static final String of PlatformTypes, every public static final Set of it
        // (sorted: Set.of's order is the JVM's), and the other classes' constants this layer names
        for (Map.Entry<String, String> e : stringConstants(PlatformTypes.class).entrySet()) {
            line("const", str("PlatformTypes." + e.getKey()), str(e.getValue()));
        }
        for (Field f : PlatformTypes.class.getDeclaredFields()) {
            int m = f.getModifiers();
            if (Modifier.isPublic(m) && Modifier.isStatic(m) && Modifier.isFinal(m) && f.getType() == Set.class) {
                try {
                    line("const", str("PlatformTypes." + f.getName()), value(new TreeSet<>((Set<?>) f.get(null))));
                } catch (IllegalAccessException e) {
                    throw new IllegalStateException(e);
                }
            }
        }
        line("const", str("Type.Primitive.PRECISE_PRIMITIVES"), str(Type.Primitive.PRECISE_PRIMITIVES));
        line("const", str("Type.RelationType.PIVOT_SEPARATOR"), str(Type.RelationType.PIVOT_SEPARATOR));
        line("const", str("Type.RelationType.LATE_BOUND_WILDCARD"), str(Type.RelationType.LATE_BOUND_WILDCARD));
        line("const", str("Type.PrecisionDecimal.DEFAULT_DECIMAL"), value(Type.PrecisionDecimal.DEFAULT_DECIMAL));
        line("const", str("Type.PrecisionDecimal.MAX_PRECISION"), value(Type.PrecisionDecimal.MAX_PRECISION));
        line("const", str("Type.PrecisionDecimal.MIN_ADJUSTED_SCALE"), value(Type.PrecisionDecimal.MIN_ADJUSTED_SCALE));
        line("const", str("Multiplicity.Bounded.ONE"), value(Multiplicity.Bounded.ONE));
        line("const", str("Multiplicity.Bounded.ZERO_ONE"), value(Multiplicity.Bounded.ZERO_ONE));
        line("const", str("Multiplicity.Bounded.ONE_MANY"), value(Multiplicity.Bounded.ONE_MANY));
        line("const", str("Multiplicity.Bounded.ZERO_MANY"), value(Multiplicity.Bounded.ZERO_MANY));
        for (Type.Op op : Type.Op.values()) {
            line("op.symbol", str(op.name()), str(op.symbol()));
        }
        for (Type.Primitive.Family fam : Type.Primitive.Family.values()) {
            line("family", Integer.toString(fam.ordinal()), str(fam.name()));
        }

        // ---- the primitives: the constants in order, each one's facts, and the FQN table in its order
        for (Type.Primitive p : Type.Primitive.values()) {
            String n = str(p.name());
            line("primitive", Integer.toString(p.ordinal()), n);
            line("primitive.qualifiedName", n, answer(p::qualifiedName));
            line("primitive.typeName", n, answer(p::typeName));
            line("primitive.family", n, answer(p::family));
            line("primitive.isNumeric", n, answer(p::isNumeric));
            line("primitive.isTemporal", n, answer(p::isTemporal));
        }
        int k = 0;
        for (Map.Entry<String, Type.Primitive> e : Type.Primitive.byFqn().entrySet()) {
            line("primitive.byFqn", Integer.toString(k++), "[" + str(e.getKey()) + "," + str(e.getValue().name()) + "]");
        }

        // ---- the string predicates
        for (String s : STRINGS) {
            String q = str(s);
            line("string.isTypeClassifier", q, answer(() -> PlatformTypes.isTypeClassifier(s)));
            line("string.isStoreNavFn", q, answer(() -> PlatformTypes.isStoreNavFn(s)));
            line("string.isRelationalConnectionClass", q, answer(() -> PlatformTypes.isRelationalConnectionClass(s)));
            line("string.relationalConnectionSimpleName", q,
                    answer(() -> PlatformTypes.relationalConnectionSimpleName(s)));
            line("string.isModelChainConnection", q, answer(() -> PlatformTypes.isModelChainConnection(s)));
            line("string.isJsonModelConnection", q, answer(() -> PlatformTypes.isJsonModelConnection(s)));
            line("string.isPlus", q, answer(() -> PlatformTypes.isPlus(s)));
            line("string.isVerdictFunction", q, answer(() -> PlatformTypes.isVerdictFunction(s)));
            line("string.isPostProcessorConfigProperty", q, answer(() -> PlatformTypes.isPostProcessorConfigProperty(s)));
            line("string.isProfile", q, str(PlatformTypes.EQUALITY_PROFILE),
                    answer(() -> PlatformTypes.isProfile(s, PlatformTypes.EQUALITY_PROFILE)));
            line("string.typeValuePath", q, answer(() -> PlatformTypes.typeValuePath(s)));
            line("string.findByFqn", q, answer(() -> Type.Primitive.findByFqn(s)));
            line("string.typeVariables", q, answer(() -> Type.Primitive.typeVariables(s)));
            line("string.presentPivotName", q, answer(() -> Type.RelationType.presentPivotName(s)));
            line("string.trustedColumn", q, answer(() -> Type.RelationType.trustedColumn(s)));
            line("string.erasedRow", q, answer(() -> Type.RelationType.erasedRow(s)));
        }

        // ---- every type: its printing and every unary operation; the kinds' own
        List<String> pivotNames = List.of("name", "'name'", "x__|__sum", "'x__|__sum'", "x__|__cnt", "'x__|__cnt'",
                "x__|__nope", "'x__|__nope'", "nope", "a", "'a'", "*", "__|__sum", "x__|__", "'x__|__cnt", "", "''",
                "a__|__b__|__sum", "'x__|__sum");
        for (int i = 0; i < TYPES.size(); i++) {
            Type t = TYPES.get(i);
            String id = Integer.toString(i);
            line("type.typeName", id, answer(t::typeName));
            line("type.isRelation", id, answer(() -> Type.isRelation(t)));
            line("type.classFqn", id, answer(() -> Type.classFqn(t)));
            line("type.asClassType", id, answer(() -> Type.asClassType(t)));
            line("type.relationSchema", id, answer(() -> Type.relationSchema(t)));
            line("type.schemaView", id, answer(() -> Type.schemaView(t)));
            line("type.requireRelationSchema", id, answer(() -> Type.requireRelationSchema(t)));
            line("type.wireDecided", id, answer(() -> Type.wireDecided(t)));
            line("type.isAny", id, answer(() -> PlatformTypes.isAny(t)));
            line("type.isNil", id, answer(() -> PlatformTypes.isNil(t)));
            line("type.isVariant", id, answer(() -> PlatformTypes.isVariant(t)));
            line("type.isJsonElement", id, answer(() -> PlatformTypes.isJsonElement(t)));
            line("type.isListCarrier", id, answer(() -> PlatformTypes.isListCarrier(t)));
            line("type.isPairCarrier", id, answer(() -> PlatformTypes.isPairCarrier(t)));
            line("type.isMapCarrier", id, answer(() -> PlatformTypes.isMapCarrier(t)));
            line("type.isValueCarrier", id, answer(() -> PlatformTypes.isValueCarrier(t)));
            line("type.isFunctionCarrier", id, answer(() -> PlatformTypes.isFunctionCarrier(t)));
            line("type.isTdsShaped", id, answer(() -> PlatformTypes.isTdsShaped(t)));
            line("type.isTdsType", id, answer(() -> PlatformTypes.isTdsType(t)));
            line("type.printsByOwnToString", id, answer(() -> PlatformTypes.printsByOwnToString(t)));
            line("type.typeValuePath", id, answer(() -> PlatformTypes.typeValuePath(t)));
            line("type.functionTypeOf", id, answer(() -> PlatformTypes.functionTypeOf(t)));
            for (int m = 0; m < MULTS.size(); m++) {
                int mi = m;
                line("type.relationValued", id, Integer.toString(m),
                        answer(() -> Type.relationValued(new ExprType(t, MULTS.get(mi)))));
            }
            StringBuilder eq = new StringBuilder();
            for (Type u : TYPES) {
                eq.append(t.equals(u) ? '1' : '0');
            }
            line("type.equals", id, str(eq.toString()));
            switch (t) {
                case Type.Primitive p -> line("primitive.ofType", id, str(p.name()));
                case Type.PrecisionDecimal d -> line("decimal.basePrimitive", id, answer(d::basePrimitive));
                case Type.GenericType g -> line("generic.isClassMetaclass", id, answer(() -> PlatformTypes.isClassMetaclass(g)));
                case Type.FunctionType f -> {
                    line("function.lambdaType", id, answer(() -> PlatformTypes.lambdaType(f)));
                    line("function.concreteFunctionDefinitionType", id,
                            answer(() -> PlatformTypes.concreteFunctionDefinitionType(f)));
                }
                case Type.RelationType r -> {
                    line("relation.relation", id, answer(() -> Type.relation(r)));
                    line("relation.isLateBound", id, answer(r::isLateBound));
                    line("relation.isErasedRow", id, answer(r::isErasedRow));
                    for (String n : pivotNames) {
                        line("relation.pivotColumnType", id, str(n), answer(() -> r.pivotColumnType(n)));
                    }
                }
                default -> {
                }
            }
        }

        // ---- the multiplicities: each one's facts, and every pair's union and product
        for (int i = 0; i < MULTS.size(); i++) {
            Multiplicity m = MULTS.get(i);
            String id = Integer.toString(i);
            line("mult.text", id, answer(m::text));
            line("mult.isMany", id, answer(m::isMany));
            line("mult.requireBounded", id, answer(() -> m.requireBounded("the oracle")));
            if (m instanceof Multiplicity.Bounded bm) {
                line("mult.isUnbounded", id, answer(bm::isUnbounded));
                line("mult.isToOne", id, answer(bm::isToOne));
            }
            for (int j = 0; j < MULTS.size(); j++) {
                Multiplicity n = MULTS.get(j);
                line("mult.union", id, Integer.toString(j), answer(() -> Multiplicity.union(m, n)));
                line("mult.product", id, Integer.toString(j), answer(() -> Multiplicity.product(m, n)));
            }
        }

        // ---- the decimals: every pair's arithmetic
        for (int i = 0; i < DECIMALS.size(); i++) {
            Type.PrecisionDecimal d = DECIMALS.get(i);
            String id = Integer.toString(i);
            line("decimal.typeName", id, answer(d::typeName));
            for (int j = 0; j < DECIMALS.size(); j++) {
                Type.PrecisionDecimal e = DECIMALS.get(j);
                String jd = Integer.toString(j);
                line("decimal.plus", id, jd, answer(() -> d.plus(e)));
                line("decimal.minus", id, jd, answer(() -> d.minus(e)));
                line("decimal.times", id, jd, answer(() -> d.times(e)));
                line("decimal.dividedBy", id, jd, answer(() -> d.dividedBy(e)));
            }
        }

        // ---- the constructors' refusals, and the multiplicity spellings
        for (int p = -2; p <= 4; p++) {
            for (int s = -2; s <= 5; s++) {
                int pp = p;
                int ss = s;
                line("new.decimal", "[" + p + "," + s + "]", answer(() -> new Type.PrecisionDecimal(pp, ss)));
            }
        }
        for (int[] ps : new int[][] {{Integer.MIN_VALUE, 0}, {Integer.MAX_VALUE, Integer.MAX_VALUE}, {0, Integer.MIN_VALUE},
                {Integer.MAX_VALUE, -1}, {39, 39}}) {
            line("new.decimal", "[" + ps[0] + "," + ps[1] + "]", answer(() -> new Type.PrecisionDecimal(ps[0], ps[1])));
        }
        for (int l = -1; l <= 3; l++) {
            for (Integer u : new Integer[] {null, -1, 0, 1, 2, 3}) {
                int ll = l;
                line("new.bounded", "[" + l + "," + (u == null ? "null" : u) + "]", answer(() -> b(ll, u)));
            }
        }
        line("new.bounded", "[" + Integer.MIN_VALUE + ",null]", answer(() -> b(Integer.MIN_VALUE, null)));
        for (String n : List.of("m", "", " ", "a b", "*")) {
            line("new.var", str(n), answer(() -> new Multiplicity.Var(n)));
        }
        Type integer = Type.Primitive.INTEGER;
        Type string = Type.Primitive.STRING;
        List<List<Type.Column>> dup = List.of(List.of(), List.of(col("a", integer, Multiplicity.Bounded.ONE)),
                List.of(col("a", integer, Multiplicity.Bounded.ONE), col("a", string, Multiplicity.Bounded.ONE)),
                List.of(col("a", integer, Multiplicity.Bounded.ONE), col("b", string, Multiplicity.Bounded.ONE),
                        col("a", integer, Multiplicity.Bounded.ONE)),
                List.of(col("b", integer, Multiplicity.Bounded.ONE), col("b", integer, Multiplicity.Bounded.ONE),
                        col("c", integer, Multiplicity.Bounded.ONE), col("c", integer, Multiplicity.Bounded.ONE)));
        for (List<Type.Column> cs : dup) {
            for (List<Type.Column> ds : dup) {
                line("new.relation", "[" + list(cs, TypeOracle::column) + "," + list(ds, TypeOracle::column) + "]",
                        answer(() -> new Type.RelationType(cs, ds)));
            }
        }
        for (String s : List.of("*", "1", "0", "0..1", "1..*", "0..*", "2..5", " 2 ", "\t3\n", "m", "n1", "0..", "..1",
                "1..0", "5..2", "01", "007..010", "99999999999", "2147483647", "2147483648", "0..99999999999", "", " ",
                " m ", " m", "m\u001f", "\u0085m", "　*", "1 .. 2", "1..2..3", "+1", "-1", "١",
                "*..*", "a*")) {
            line("ofArgument", str(s), answer(() -> Multiplicity.ofArgument(s)));
        }

        // ---- the SQL types a wire spells
        List<Map.Entry<String, SqlType>> sql = new ArrayList<>();
        for (SqlType.Scalar s : SqlType.Scalar.values()) {
            sql.add(Map.entry("scalar:" + s.name(), s));
        }
        sql.add(Map.entry("decimal:10,2", new SqlType.Decimal(10, 2)));
        sql.add(Map.entry("array:BIGINT", new SqlType.Array(SqlType.Scalar.BIGINT)));
        sql.add(Map.entry("map:VARCHAR,BIGINT", new SqlType.Map(SqlType.Scalar.VARCHAR, SqlType.Scalar.BIGINT)));
        sql.add(Map.entry("struct:a:BIGINT", new SqlType.Struct(List.of(new SqlType.Struct.Field("a", SqlType.Scalar.BIGINT)))));
        sql.add(Map.entry("tagged:INTEGER:BIGINT", new SqlType.Tagged(List.of(new SqlType.Tagged.Slot("INTEGER",
                SqlType.Scalar.BIGINT)))));
        for (Map.Entry<String, SqlType> e : sql) {
            line("kindOfSqlType", str(e.getKey()), answer(() -> Type.kindOfSqlType(e.getValue())));
        }

        StringBuilder out = new StringBuilder();
        out.append("# The type family's answers: rust/oracles/types/TypeOracle.java (its header says how it is run and what a line is).\n");
        out.append("# ").append(LINES.size()).append(" lines, ").append(TYPES.size()).append(" types, ")
                .append(MULTS.size()).append(" multiplicities, ").append(DECIMALS.size()).append(" decimals, ")
                .append(STRINGS.size()).append(" strings. Lines per op:\n");
        for (Map.Entry<String, Integer> e : COUNTS.entrySet()) {
            out.append("#\t").append(e.getKey()).append('\t').append(e.getValue()).append('\n');
        }
        for (String l : LINES) {
            out.append(l).append('\n');
        }
        Files.writeString(Path.of(args[0]), out.toString(), StandardCharsets.UTF_8);
        System.out.println(LINES.size() + " lines -> " + args[0]);
    }
}
