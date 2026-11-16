// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

package com.legend.frontoracle;

import com.legend.parser.PmcdParser;
import com.legend.protocol.spec.LambdaFunction;
import com.legend.protocol.spec.ValueSpecification;
import java.lang.reflect.ParameterizedType;
import java.lang.reflect.RecordComponent;
import java.lang.reflect.Type;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayDeque;
import java.util.ArrayList;
import java.util.Deque;
import java.util.HashMap;
import java.util.HashSet;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.Set;
import java.util.TreeMap;
import java.util.regex.Matcher;
import java.util.regex.Pattern;

/**
 * THE RECORDS IN RUST (Rust port phase 2, step 3: docs/rust-port/PHASE2.md §3): the protocol records the parser's
 * doors return, read by reflection (test code: the product bans it) and written as Rust -- a struct per record, its
 * components in canonical order; an enum per sealed parent over its permitted subtypes; an enum per Java enum -- with
 * the writer of the lossless codec ({@link RecordCodec}'s form, byte for byte: a type tag only where the declared type
 * does not fix the runtime class). A component is {@code Option} exactly where its source says
 * {@code @Nullable}: the protocol and the parser are NullAway-checked, so an unannotated component is never null. The
 * annotation has CLASS retention, so it is read from the records' source, which the generator is given.
 *
 * <p>The source tree is {@code Arc} (2026-11-11; docs/rust-port/PHASE3_DESIGN.md §6, ruling 1): a record that is a
 * {@link ValueSpecification} node ({@link #family}) is held as {@code Arc<R>} wherever a slot or a sealed parent holds
 * it, and its list components are {@code Arc<[T]>}, so that phase 3's rewrites alias an unchanged node or list as
 * Java's do and compare it by identity. Every other record is held as {@code Box<R>}, its lists {@code Vec<T>}. The
 * writers read through references, so the codec's bytes are the same either way.
 *
 * <pre>a java_run: FrontCodegen OUT_RS CORE_MAIN_DIR  (CORE_MAIN_DIR: core/src/main/java/com/legend)</pre>
 */
public final class FrontCodegen {

    /** A Rust type's spelling, and how the writer writes a value of it. */
    private record RustType(String spelling, String write) {
    }

    private final Path main;
    /** The test sources' {@code com/legend} directory (the element mode's: ElementAnswers' records are test code). */
    private Path test;
    private final Map<Class<?>, String> names = new HashMap<>();
    private final Map<String, Class<?>> byName = new HashMap<>();
    private final Map<Class<?>, Map<String, Boolean>> nullable = new HashMap<>();
    /** Per record, the list components whose ELEMENTS may be null ({@code List<@Nullable X>} in the source). */
    private final Map<Class<?>, Map<String, Boolean>> nullableElements = new HashMap<>();
    private final Map<Path, String> sources = new HashMap<>();
    private final List<Class<?>> records = new ArrayList<>();
    private final List<Class<?>> sealedTypes = new ArrayList<>();
    private final List<Class<?>> enums = new ArrayList<>();

    private FrontCodegen(Path main) {
        this.main = main;
    }

    /** {@code rust OUT MAIN}: the Rust records and writer; {@code java OUT MAIN}: the Java reader; {@code model OUT
     *  MAIN}: the model's records in Rust (2026-11-12, {@link #model}); {@code element OUT MAIN TEST}: the element
     *  differential's mirrors of ElementAnswers' records in Rust (2026-11-15, {@link #element}; TEST the test
     *  sources' {@code com/legend} directory, where ElementAnswers is). */
    public static void main(String[] args) throws Exception {
        FrontCodegen g = new FrontCodegen(Path.of(args[2]));
        g.discover(List.of(PmcdParser.Parsed.class, ValueSpecification.class, LambdaFunction.class));
        String out = switch (args[0]) {
            case "rust" -> g.rust();
            case "java" -> g.java();
            case "model" -> g.model();
            case "element" -> g.element(Path.of(args[3]));
            default -> throw new IllegalArgumentException("a mode is rust, java, model or element, not " + args[0]);
        };
        Files.writeString(Path.of(args[1]), out, StandardCharsets.UTF_8);
    }

    // ------------------------------------------------------------------ the classes reached

    private void discover(List<Class<?>> roots) {
        List<Class<?>> sealed = sealedTypes;
        Deque<Type> todo = new ArrayDeque<>(roots);
        Set<Class<?>> done = new HashSet<>();
        while (!todo.isEmpty()) {
            Type t = todo.pop();
            if (t instanceof ParameterizedType p) {
                for (Type a : p.getActualTypeArguments()) {
                    todo.push(a);
                }
                continue;
            }
            if (!(t instanceof Class<?> c) || !done.add(c)) {
                continue;
            }
            if (c.isRecord()) {
                records.add(c);
                for (RecordComponent rc : c.getRecordComponents()) {
                    todo.push(rc.getGenericType());
                }
            } else if (c.isSealed()) {
                sealed.add(c);
                for (Class<?> s : c.getPermittedSubclasses()) {
                    todo.push(s);
                }
            } else if (c.isEnum()) {
                enums.add(c);
            }
        }
        for (List<Class<?>> l : List.of(records, sealed, enums)) {
            l.sort(java.util.Comparator.comparing(RecordCodec::tag));
            for (Class<?> c : l) {
                String n = rustName(c);
                Class<?> before = byName.putIfAbsent(n, c);
                if (before != null) {
                    throw new IllegalStateException("two classes map to the Rust name " + n + ": " + before + ", " + c);
                }
                names.put(c, n);
            }
        }
    }

    private String rust() throws Exception {
        StringBuilder out = new StringBuilder();
        out.append("""
                // GENERATED by //core:front_records_rs (core/src/test/java/com/legend/frontoracle/FrontCodegen.java)
                // from the protocol records the parser's doors return -- do not edit; regenerate with
                // `bazel run //rust/lite-front:update_generated`. A struct per record, its components in canonical
                // order (`Option` exactly where the record's source says @Nullable); an enum per sealed parent and per
                // Java enum; the lossless codec's writer, byte for byte the JVM's (RecordCodec). A ValueSpecification
                // node is held as `Arc`, its lists `Arc<[T]>` (the source tree, docs/rust-port/PHASE3_DESIGN.md §6
                // ruling 1); every other record as `Box`, its lists `Vec`.

                #![allow(clippy::all, non_camel_case_types, unused_variables)]

                use std::sync::Arc;

                use crate::codec::{Codec, Writer};
                use crate::jtypes::{BigDecimal, JStr, JValue};

                """);
        for (Class<?> c : records) {
            record(out, c);
        }
        for (Class<?> c : sealedTypes) {
            sealed(out, c);
        }
        for (Class<?> c : enums) {
            enumeration(out, c);
        }
        return out.toString();
    }

    // ------------------------------------------------------------------ the Java reader

    /**
     * The codec's reader in Java (com.legend.ffi.FrontRecords), a streaming one: the answer's text pulled through
     * {@code FrontRead.Cursor} in the order the writers write it, no tree built. A method per record calling its
     * canonical constructor with each component read as its declared type, in canonical order -- Java evaluates the
     * arguments left to right, which is the order the codec writes the keys ({@code null} where the codec wrote null,
     * only where the component is {@code @Nullable}); {@code tagged} when a sealed parent's reader has read the
     * record's brace and tag, so its first key follows a comma. A method per sealed parent reading the tag, which the
     * codec writes first, and dispatching on it over every record beneath it (the codec tags the record, never a
     * parent); the leaves through {@code FrontRead}. No reflection: the product bans it
     * (ArchitectureTest.reflectionIsBannedInProduction), so the reader is generated, never reflective.
     */
    private String java() throws Exception {
        StringBuilder out = new StringBuilder();
        out.append("""
                // GENERATED by //core:front_records_java (core/src/test/java/com/legend/frontoracle/FrontCodegen.java)
                // from the protocol records the parser's doors return -- do not edit; regenerate with
                // `bazel run //core:update_generated`.

                package com.legend.ffi;

                /**
                 * The lossless record codec's reader (docs/rust-port/PHASE2.md §3): the Rust parser's answer read into
                 * the protocol records the Java parser builds, streamed -- the answer's text pulled through a
                 * {@link FrontRead.Cursor} in the order the writers write it, no tree of it built. A reader per record,
                 * its canonical constructor fed each component as its declared type, each key in canonical order (a key
                 * missing or out of order refused); a reader per sealed parent, by the record's tag, written first; the
                 * leaves through {@link FrontRead}. Generated (FrontCodegen), never reflective.
                 */
                final class FrontRecords {

                    private FrontRecords() {
                    }

                """);
        for (Class<?> c : records) {
            if (READER_OMITS.contains(c)) {
                continue;
            }
            if (!java.lang.reflect.Modifier.isPublic(c.getModifiers())) {
                throw new IllegalStateException(c.getName() + " is not public: the reader cannot build it");
            }
            Map<String, Boolean> nulls = nullability(c);
            String name = names.get(c);
            out.append("    static ").append(c.getCanonicalName()).append(" read").append(name)
                    .append("(FrontRead.Cursor r, boolean tagged) {\n");
            out.append("        if (!tagged) {\n            FrontRead.open(r);\n        }\n");
            out.append("        return FrontRead.close(r, \"").append(name).append("\", new ")
                    .append(c.getCanonicalName()).append("(");
            RecordComponent[] comps = c.getRecordComponents();
            if (comps.length == 0) {
                out.append("));\n    }\n\n");
                continue;
            }
            out.append("\n");
            for (int i = 0; i < comps.length; i++) {
                RecordComponent rc = comps[i];
                // each component read through FrontRead.component, so a refusal names its record and component
                String helper = nulls.get(rc.getName()) ? "nullableComponent" : "component";
                String value = javaRead(rc.getGenericType(), "c", 0);
                if (nullableElements(c).get(rc.getName())) {
                    value = "FrontRead.nullableList(c, x0 -> " + javaRead(listElement(rc), "x0", 1) + ")";
                    // the witness says the elements are nullable: NullAway infers the type argument without it
                    // a type annotation on a qualified name sits before its simple name (java.lang.@A String)
                    String element = ((Class<?>) listElement(rc)).getCanonicalName();
                    int dot = element.lastIndexOf('.');
                    helper = "<java.util.List<" + element.substring(0, dot + 1) + "@com.legend.base.Nullable "
                            + element.substring(dot + 1) + ">>" + helper;
                }
                helper = "FrontRead." + helper;
                // the key as the writers write it: after a comma, but for the first after an untagged brace
                String key = jsonLiteral(rc.getName()) + ":";
                String keyRead = i == 0
                        ? "tagged ? " + javaString("," + key) + " : " + javaString(key)
                        : javaString("," + key);
                String read = helper + "(r, \"" + name + "." + rc.getName() + "\", " + keyRead + ", c -> " + value
                        + ")";
                out.append("                ").append(read).append(i + 1 < comps.length ? ",\n" : "));\n");
            }
            out.append("    }\n\n");
        }
        for (Class<?> c : sealedTypes) {
            out.append("    static ").append(c.getCanonicalName()).append(" read").append(names.get(c))
                    .append("(FrontRead.Cursor r) {\n");
            out.append("        String tag = FrontRead.tag(r);\n        return switch (tag) {\n");
            for (Class<?> leaf : leaves(c)) {
                out.append("            case \"").append(RecordCodec.tag(leaf)).append("\" -> read")
                        .append(names.get(leaf)).append("(r, true);\n");
            }
            out.append("            default -> throw new IllegalArgumentException(\"no ")
                    .append(RecordCodec.tag(c)).append(" is tagged \" + tag);\n        };\n    }\n\n");
        }
        out.append("}\n");
        return out.toString();
    }

    /**
     * The records the reader never builds: the overlay seam's opaque record, which only {@code OverlayElementSink}
     * mints (AGENTS.md invariant 12, ArchitectureTest.opaqueRecordIsLockedToTheOverlaySeam). Rust never answers one:
     * it hands an overlay section back raw and Java's overlay grammar mints its record (docs/rust-port/PHASE2.md §3).
     */
    private static final Set<Class<?>> READER_OMITS = Set.of(com.legend.protocol.Protocol.POpaqueElement.class);

    /** The records beneath a sealed parent, through nested sealed parents, in tag order. */
    private static List<Class<?>> leaves(Class<?> sealed) {
        Map<String, Class<?>> out = new TreeMap<>();
        Deque<Class<?>> todo = new ArrayDeque<>(List.of(sealed.getPermittedSubclasses()));
        while (!todo.isEmpty()) {
            Class<?> c = todo.pop();
            if (READER_OMITS.contains(c)) {
                continue;
            }
            if (c.isRecord()) {
                out.put(RecordCodec.tag(c), c);
            } else if (c.isSealed()) {
                todo.addAll(List.of(c.getPermittedSubclasses()));
            } else {
                throw new IllegalStateException(c.getName() + " under " + sealed.getName() + " is neither");
            }
        }
        return List.copyOf(out.values());
    }

    /** The Java expression reading a node {@code n} (an expression) as declared type {@code t}. */
    private String javaRead(Type t, String n, int depth) {
        // each nested lambda's parameter its own name: Java refuses one that shadows an enclosing lambda's
        String x = "x" + depth;
        String k = "k" + depth;
        String v = "v" + depth;
        if (t instanceof ParameterizedType p) {
            Class<?> raw = (Class<?>) p.getRawType();
            Type[] a = p.getActualTypeArguments();
            if (raw == List.class) {
                return "FrontRead.list(" + n + ", " + x + " -> " + javaRead(a[0], x, depth + 1) + ")";
            }
            if ((raw == Map.class || raw == LinkedHashMap.class) && a[0] == String.class) {
                return "FrontRead.map(" + n + ", " + v + " -> " + javaRead(a[1], v, depth + 1) + ")";
            }
            if (raw == Map.Entry.class) {
                return "FrontRead.entry(" + n + ", " + k + " -> " + javaRead(a[0], k, depth + 1) + ", " + v + " -> "
                        + javaRead(a[1], v, depth + 1) + ")";
            }
            throw new IllegalArgumentException("no Java reader rule for " + t.getTypeName());
        }
        Class<?> c = (Class<?>) t;
        if (c == int.class || c == Integer.class) {
            return "FrontRead.integer(" + n + ")";
        }
        if (c == long.class || c == Long.class) {
            return "FrontRead.longValue(" + n + ")";
        }
        if (c == boolean.class || c == Boolean.class) {
            return "FrontRead.bool(" + n + ")";
        }
        if (c == double.class || c == Double.class) {
            return "FrontRead.doubleValue(" + n + ")";
        }
        if (c == String.class) {
            return "FrontRead.string(" + n + ")";
        }
        if (c == java.math.BigDecimal.class) {
            return "FrontRead.decimal(" + n + ")";
        }
        if (c == Number.class) {
            return "FrontRead.number(" + n + ")";
        }
        if (c == Object.class) {
            return "FrontRead.value(" + n + ")";
        }
        if (c.isRecord()) {
            // a slot declared as the record fixes its class: the codec writes no tag
            return "read" + names.get(c) + "(" + n + ", false)";
        }
        if (c.isSealed()) {
            return "read" + names.get(c) + "(" + n + ")";
        }
        if (c.isEnum()) {
            return c.getCanonicalName() + ".valueOf(FrontRead.string(" + n + "))";
        }
        throw new IllegalArgumentException("no Java reader rule for " + c.getName());
    }

    /** {@code protocol.Protocol$PClass} is {@code PClass}; another nested name joins its parts
     *  ({@code TypeExpression$NameRef}: {@code TypeExpressionNameRef}). */
    private static String rustName(Class<?> c) {
        String tag = RecordCodec.tag(c);
        String binary = tag.substring(tag.lastIndexOf('.') + 1);
        String[] parts = binary.split("\\$");
        if (tag.startsWith("protocol.Protocol$")) {
            return String.join("", java.util.Arrays.copyOfRange(parts, 1, parts.length));
        }
        return String.join("", parts);
    }

    // ------------------------------------------------------------------ records

    private void record(StringBuilder out, Class<?> c) throws Exception {
        String name = names.get(c);
        Map<String, Boolean> nulls = nullability(c);
        RecordComponent[] comps = c.getRecordComponents();
        out.append("/// `").append(c.getName()).append("`\n");
        out.append("#[derive(Clone, Debug, PartialEq)]\npub struct ").append(name).append(" {\n");
        List<RustType> types = new ArrayList<>();
        for (RecordComponent rc : comps) {
            RustType t = componentType(c, rc, "self." + field(rc.getName()));
            boolean n = nulls.get(rc.getName());
            if (n) {
                t = new RustType("Option<" + t.spelling() + ">", null);
            }
            types.add(t);
            out.append("    pub ").append(field(rc.getName())).append(": ").append(t.spelling()).append(",\n");
        }
        out.append("}\n\n");
        // the writer: {"@":tag, ...} where the declared type does not fix the class (exact false)
        out.append("impl Codec for ").append(name).append(" {\n");
        out.append("    fn write(&self, w: &mut Writer, exact: bool) {\n");
        out.append("        w.raw(\"{\");\n");
        String tagLit = jsonLiteral(RecordCodec.tag(c));
        if (comps.length == 0) {
            out.append("        if !exact { w.raw(\"\\\"@\\\":\"); w.raw(").append(rustString(tagLit)).append("); }\n");
        } else {
            out.append("        if !exact { w.raw(\"\\\"@\\\":\"); w.raw(").append(rustString(tagLit))
                    .append("); w.raw(\",\"); }\n");
        }
        for (int i = 0; i < comps.length; i++) {
            RecordComponent rc = comps[i];
            String key = (i > 0 ? "," : "") + jsonLiteral(rc.getName()) + ":";
            out.append("        w.raw(").append(rustString(key)).append(");\n");
            String access = "self." + field(rc.getName());
            if (nulls.get(rc.getName())) {
                RustType inner = componentType(c, rc, "v");
                out.append("        match &").append(access).append(" { None => w.raw(\"null\"), Some(v) => { ")
                        .append(inner.write()).append(" } }\n");
            } else {
                RustType t = componentType(c, rc, "v");
                out.append("        { let v = &").append(access).append("; ").append(t.write()).append(" }\n");
            }
        }
        out.append("        w.raw(\"}\");\n    }\n}\n\n");
    }

    /** A component's Rust type: its declared type's, but a list whose elements its source says may be null is a
     *  {@code Vec<Option<_>>} ({@code Arc<[Option<_>]>} on a {@link #family} record), each null element written
     *  {@code null}. */
    private RustType componentType(Class<?> owner, RecordComponent rc, String v) throws Exception {
        boolean shared = family(owner);
        if (!nullableElements(owner).get(rc.getName())) {
            return type(rc.getGenericType(), v, shared);
        }
        RustType e = type(listElement(rc), "x", shared);
        return new RustType(list(shared, "Option<" + e.spelling() + ">"),
                "{ w.raw(\"[\"); for (i, x) in " + v + ".iter().enumerate() { if i > 0 { w.raw(\",\"); } "
                        + "match x { None => w.raw(\"null\"), Some(x) => { " + e.write() + " } } } w.raw(\"]\"); }");
    }

    /** The element type of a list component (element nullability is only a list's). */
    private static Type listElement(RecordComponent rc) {
        if (rc.getGenericType() instanceof ParameterizedType p && p.getRawType() == List.class) {
            return p.getActualTypeArguments()[0];
        }
        throw new IllegalStateException(rc + ": only a list's elements may be marked @Nullable");
    }

    /**
     * The source tree's records (docs/rust-port/PHASE3_DESIGN.md §6, ruling 1): every record that is a
     * {@link ValueSpecification} node -- the sealed parent's records, and those of its nested parents
     * ({@code ColumnInstance}, {@code TypeAnnotation}). Held as {@code Arc<R>}, their lists {@code Arc<[T]>}. A record
     * a node holds that is not itself a node ({@code KeyExpression}, {@code GraphFetchLiteral.Node}, a GraphQL
     * island's records, {@code SourceInfo}, ...) stays {@code Box}, its lists {@code Vec}.
     */
    private static boolean family(Class<?> c) {
        return c.isRecord() && ValueSpecification.class.isAssignableFrom(c);
    }

    /** A list of {@code element}: {@code Arc<[_]>} on a {@link #family} record, else {@code Vec<_>}. */
    private static String list(boolean shared, String element) {
        return shared ? "Arc<[" + element + "]>" : "Vec<" + element + ">";
    }

    /**
     * A declared Java type in Rust, and the statement that writes {@code v} (an expression of a reference to it).
     * {@code shared}: the type is a component of a {@link #family} record, whose lists are {@code Arc<[T]>}.
     */
    private RustType type(Type t, String v, boolean shared) {
        if (t instanceof ParameterizedType p) {
            Class<?> raw = (Class<?>) p.getRawType();
            Type[] a = p.getActualTypeArguments();
            if (raw == List.class) {
                RustType e = type(a[0], "x", shared);
                return new RustType(list(shared, e.spelling()),
                        "{ w.raw(\"[\"); for (i, x) in " + v + ".iter().enumerate() { if i > 0 { w.raw(\",\"); } "
                                + e.write() + " } w.raw(\"]\"); }");
            }
            if (shared) {
                // a node's map or entry has no shape ruled yet: a decision, not a silent Vec
                throw new IllegalStateException("a ValueSpecification node holds a " + t.getTypeName()
                        + ": rule its shared shape (docs/rust-port/PHASE3_DESIGN.md §6, ruling 1)");
            }
            if (raw == Map.class || raw == LinkedHashMap.class) {
                RustType k = type(a[0], "k", false);
                RustType val = type(a[1], "x", false);
                if (a[0] == String.class) {
                    return new RustType("Vec<(" + k.spelling() + ", " + val.spelling() + ")>",
                            "{ w.raw(\"{\"); for (i, (k, x)) in " + v + ".iter().enumerate() { if i > 0 { w.raw(\",\"); } "
                                    + "w.string(k); w.raw(\":\"); " + val.write() + " } w.raw(\"}\"); }");
                }
                return new RustType("Vec<(" + k.spelling() + ", " + val.spelling() + ")>",
                        "{ w.raw(\"[\"); for (i, (k, x)) in " + v + ".iter().enumerate() { if i > 0 { w.raw(\",\"); } "
                                + "w.raw(\"[\"); " + k.write() + " w.raw(\",\"); " + val.write() + " w.raw(\"]\"); } "
                                + "w.raw(\"]\"); }");
            }
            if (raw == Map.Entry.class) {
                RustType k = type(a[0], "ek", false);
                RustType val = type(a[1], "ev", false);
                return new RustType("(" + k.spelling() + ", " + val.spelling() + ")",
                        "{ let ek = &" + v + ".0; let ev = &" + v + ".1; w.raw(\"[\"); " + k.write() + " w.raw(\",\"); "
                                + val.write() + " w.raw(\"]\"); }");
            }
            throw new IllegalArgumentException("no Rust rule for " + t.getTypeName());
        }
        Class<?> c = (Class<?>) t;
        if (c == int.class || c == Integer.class) {
            return new RustType("i32", "w.int(i64::from(*" + v + "));");
        }
        if (c == long.class || c == Long.class) {
            return new RustType("i64", "w.int(*" + v + ");");
        }
        if (c == boolean.class || c == Boolean.class) {
            return new RustType("bool", "w.boolean(*" + v + ");");
        }
        if (c == double.class || c == Double.class) {
            return new RustType("f64", "w.double(*" + v + ");");
        }
        if (c == String.class) {
            return new RustType("JStr", "w.string(" + v + ");");
        }
        if (c == java.math.BigDecimal.class) {
            return new RustType("BigDecimal", "w.decimal(" + v + ");");
        }
        if (c == Number.class || c == Object.class) {
            // a leaf in an open slot: always tagged by its runtime class
            return new RustType("JValue", "w.value(" + v + ");");
        }
        if (c.isRecord()) {
            return new RustType(held(c), v + ".write(w, true);");
        }
        if (c.isSealed() || c.isEnum()) {
            return new RustType(names.get(c), v + ".write(w, true);");
        }
        throw new IllegalArgumentException("no Rust rule for " + c.getName());
    }

    /** A record as a slot or a sealed parent holds it: {@code Arc<R>} for a {@link #family} record, else
     *  {@code Box<R>}. */
    private String held(Class<?> record) {
        return (family(record) ? "Arc<" : "Box<") + names.get(record) + ">";
    }

    private static final Set<String> KEYWORDS = Set.of("as", "async", "await", "box", "break", "const", "continue",
            "crate", "dyn", "else", "enum", "extern", "false", "fn", "for", "if", "impl", "in", "let", "loop", "match",
            "mod", "move", "mut", "pub", "ref", "return", "self", "static", "struct", "super", "trait", "true", "type",
            "unsafe", "use", "where", "while", "yield", "abstract", "become", "do", "final", "macro", "override",
            "priv", "try", "typeof", "unsized", "virtual", "gen");

    /** A component's Rust field: snake case, a keyword raw ({@code r#type}). */
    private static String field(String javaName) {
        String snake = javaName.replaceAll("([a-z0-9])([A-Z])", "$1_$2").toLowerCase(java.util.Locale.ROOT);
        return KEYWORDS.contains(snake) ? "r#" + snake : snake;
    }

    // ------------------------------------------------------------------ sealed parents and enums

    private void sealed(StringBuilder out, Class<?> c) {
        String name = names.get(c);
        out.append("/// `").append(c.getName()).append("` (sealed)\n");
        out.append("#[derive(Clone, Debug, PartialEq)]\npub enum ").append(name).append(" {\n");
        for (Class<?> s : c.getPermittedSubclasses()) {
            String sn = names.get(s);
            out.append("    ").append(sn).append("(").append(s.isRecord() ? held(s) : sn).append("),\n");
        }
        out.append("}\n\n");
        // a value in a slot declared as this parent is never exact: each record writes its tag
        out.append("impl Codec for ").append(name).append(" {\n");
        out.append("    fn write(&self, w: &mut Writer, _exact: bool) {\n        match self {\n");
        for (Class<?> s : c.getPermittedSubclasses()) {
            out.append("            ").append(name).append("::").append(names.get(s)).append("(x) => x.write(w, false),\n");
        }
        out.append("        }\n    }\n}\n\n");
    }

    private void enumeration(StringBuilder out, Class<?> c) {
        String name = names.get(c);
        out.append("/// `").append(c.getName()).append("` (enum)\n");
        out.append("#[derive(Clone, Copy, Debug, PartialEq, Eq)]\npub enum ").append(name).append(" {\n");
        for (Object k : c.getEnumConstants()) {
            out.append("    ").append(((Enum<?>) k).name()).append(",\n");
        }
        out.append("}\n\n");
        out.append("impl Codec for ").append(name).append(" {\n");
        out.append("    fn write(&self, w: &mut Writer, _exact: bool) {\n        w.raw(match self {\n");
        for (Object k : c.getEnumConstants()) {
            String n = ((Enum<?>) k).name();
            out.append("            ").append(name).append("::").append(n).append(" => ")
                    .append(rustString(jsonLiteral(n))).append(",\n");
        }
        out.append("        });\n    }\n}\n\n");
    }

    // ------------------------------------------------------------------ nullability, from the source

    /** Each component of {@code c}: whether its record header in the source annotates it {@code @Nullable}. */
    private Map<String, Boolean> nullability(Class<?> c) throws Exception {
        Map<String, Boolean> known = nullable.get(c);
        if (known != null) {
            return known;
        }
        Class<?> top = c;
        while (top.getEnclosingClass() != null) {
            top = top.getEnclosingClass();
        }
        String rel = top.getName().substring("com.legend.".length()).replace('.', '/') + ".java";
        Path file = main.resolve(rel);
        if (!Files.exists(file) && test != null) {
            // a record of the test sources (the element mode's ElementAnswers)
            file = test.resolve(rel);
        }
        String src = sources.computeIfAbsent(file, f -> {
            try {
                return Files.readString(f, StandardCharsets.UTF_8);
            } catch (java.io.IOException e) {
                throw new java.io.UncheckedIOException(e);
            }
        });
        String simple = c.getSimpleName();
        Matcher m = Pattern.compile("\\brecord\\s+" + Pattern.quote(simple) + "\\s*(<[^>]*>)?\\s*\\(").matcher(src);
        if (!m.find()) {
            throw new IllegalStateException("no record header for " + c.getName() + " in " + file);
        }
        int open = m.end() - 1;
        if (m.find()) {
            throw new IllegalStateException("two record headers named " + simple + " in " + file);
        }
        List<String> params = parameters(src, open);
        RecordComponent[] comps = c.getRecordComponents();
        if (params.size() != comps.length) {
            throw new IllegalStateException(c.getName() + ": the header lists " + params.size()
                    + " components, reflection " + comps.length);
        }
        Map<String, Boolean> result = new TreeMap<>();
        Map<String, Boolean> elements = new TreeMap<>();
        for (int i = 0; i < comps.length; i++) {
            String p = params.get(i).strip();
            String[] words = p.split("[^A-Za-z0-9_$]+");
            String last = words[words.length - 1];
            if (!last.equals(comps[i].getName())) {
                throw new IllegalStateException(c.getName() + ": header component " + i + " is '" + p
                        + "', reflection's " + comps[i].getName());
            }
            int angle = p.indexOf('<');
            String head = angle < 0 ? p : p.substring(0, angle);
            String inside = angle < 0 ? "" : p.substring(angle);
            result.put(last, head.contains("Nullable"));
            elements.put(last, inside.contains("Nullable"));
        }
        nullable.put(c, result);
        nullableElements.put(c, elements);
        return result;
    }

    /** Each list component of {@code c}: whether its source marks the ELEMENT type {@code @Nullable}. */
    private Map<String, Boolean> nullableElements(Class<?> c) throws Exception {
        nullability(c);
        return nullableElements.get(c);
    }

    /** The comma-separated parameters of the header whose '(' is at {@code open}, comments dropped. */
    private static List<String> parameters(String src, int open) {
        List<String> out = new ArrayList<>();
        int depth = 0;
        int angle = 0;
        StringBuilder cur = new StringBuilder();
        for (int i = open + 1; i < src.length(); i++) {
            char ch = src.charAt(i);
            if (ch == '/' && i + 1 < src.length() && src.charAt(i + 1) == '/') {
                while (i < src.length() && src.charAt(i) != '\n') {
                    i++;
                }
                continue;
            }
            if (ch == '/' && i + 1 < src.length() && src.charAt(i + 1) == '*') {
                i = src.indexOf("*/", i + 2) + 1;
                continue;
            }
            if (ch == '(') {
                depth++;
            } else if (ch == ')') {
                if (depth == 0) {
                    if (!cur.toString().isBlank()) {
                        out.add(cur.toString());
                    }
                    return out;
                }
                depth--;
            } else if (ch == '<') {
                angle++;
            } else if (ch == '>') {
                angle--;
            } else if (ch == ',' && depth == 0 && angle == 0) {
                out.add(cur.toString());
                cur.setLength(0);
                continue;
            }
            cur.append(ch);
        }
        throw new IllegalStateException("unterminated record header");
    }

    // ------------------------------------------------------------------ the model's records (model mode)

    /*
     * THE MODEL'S RECORDS IN RUST (2026-11-12, docs/rust-port/PHASE3.md slice 1b; PHASE3_DESIGN.md §6 ruling 1): the
     * records of com.legend.model the compiler's model is made of -- every record, sealed interface and enum reached
     * from ParsedModel, NormalizedModel, Function, FunctionId and ClassMember -- for rust/lite-model, with the same
     * codec's writer, so the model differential's canonical text (RecordCodec's form) is written by both sides from
     * one schema. The model tier is Arc: a model record is held as Arc<R> wherever a slot or a sealed parent holds it,
     * a list as Arc<[T]>, a map as Arc<[(K, V)]> in its iteration order (every map the model's records hold is a
     * LinkedHashMap's view or a one-entry Map.of). A protocol record the model holds is spelled as rust/lite-front
     * holds it (p::, an Arc for a ValueSpecification node, a Box otherwise); the walk stops there. Equality is the
     * record's: no model record writes its own equals (2026-11-12, searched), so every one derives PartialEq, and Eq and
     * Hash where every component has them.
     */

    private final List<Class<?>> modelRecords = new ArrayList<>();
    private final List<Class<?>> modelSealed = new ArrayList<>();
    private final List<Class<?>> modelEnums = new ArrayList<>();
    private final Map<Class<?>, String> modelNames = new HashMap<>();
    private final Map<Class<?>, Boolean> modelHashable = new HashMap<>();

    private static boolean isModel(Class<?> c) {
        return c.getName().startsWith("com.legend.model.");
    }

    private String model() throws Exception {
        discoverModel(List.of(com.legend.model.ParsedModel.class, com.legend.model.NormalizedModel.class,
                com.legend.model.Function.class, com.legend.model.FunctionId.class,
                com.legend.model.ClassMember.class));
        hashability();
        StringBuilder out = new StringBuilder();
        out.append("""
                // GENERATED by //core:model_records_rs (core/src/test/java/com/legend/frontoracle/FrontCodegen.java, its
                // model mode) from the model's records (com.legend.model) -- do not edit; regenerate with
                // `bazel run //rust/lite-model:update_generated`. A struct per record, its components in canonical order
                // (`Option` exactly where the record's source says @Nullable); an enum per sealed parent and per Java
                // enum; the lossless codec's writer, byte for byte the JVM's (RecordCodec). The model tier is `Arc`
                // (docs/rust-port/PHASE3_DESIGN.md §6 ruling 1): a model record held as `Arc<R>`, a list `Arc<[T]>`, a map
                // `Arc<[(K, V)]>` in its iteration order; a protocol record as rust/lite-front holds it.

                #![allow(clippy::all, non_camel_case_types, unused_variables, unused_imports)]

                use std::sync::Arc;

                use lite_front::codec::{Codec, Writer};
                use lite_front::jtypes::{BigDecimal, JStr, JValue};
                use lite_front::records as p;

                """);
        for (Class<?> c : modelRecords) {
            modelRecord(out, c);
        }
        for (Class<?> c : modelSealed) {
            modelSealedType(out, c);
        }
        for (Class<?> c : modelEnums) {
            modelEnumeration(out, c);
        }
        return out.toString();
    }

    private void discoverModel(List<Class<?>> roots) {
        Deque<Type> todo = new ArrayDeque<>(roots);
        Set<Class<?>> done = new HashSet<>();
        while (!todo.isEmpty()) {
            Type t = todo.pop();
            if (t instanceof ParameterizedType p) {
                for (Type a : p.getActualTypeArguments()) {
                    todo.push(a);
                }
                continue;
            }
            if (!(t instanceof Class<?> c) || !done.add(c)) {
                continue;
            }
            if (!isModel(c)) {
                if ((c.isRecord() || c.isSealed() || c.isEnum()) && !names.containsKey(c)) {
                    throw new IllegalStateException("the model holds " + c.getName()
                            + ", which the protocol's records do not reach: rust/lite-front has no type for it");
                }
                continue;
            }
            if (c.isRecord()) {
                modelRecords.add(c);
                for (RecordComponent rc : c.getRecordComponents()) {
                    todo.push(rc.getGenericType());
                }
            } else if (c.isSealed()) {
                modelSealed.add(c);
                for (Class<?> s : c.getPermittedSubclasses()) {
                    todo.push(s);
                }
            } else if (c.isEnum()) {
                modelEnums.add(c);
            } else {
                throw new IllegalStateException("the model holds " + c.getName()
                        + ", neither a record, a sealed interface nor an enum");
            }
        }
        Map<String, Class<?>> byModelName = new HashMap<>();
        for (List<Class<?>> l : List.of(modelRecords, modelSealed, modelEnums)) {
            l.sort(java.util.Comparator.comparing(RecordCodec::tag));
            for (Class<?> c : l) {
                String n = rustName(c);
                Class<?> before = byModelName.putIfAbsent(n, c);
                if (before != null) {
                    throw new IllegalStateException("two classes map to the Rust name " + n + ": " + before + ", " + c);
                }
                modelNames.put(c, n);
            }
        }
    }

    /** Which model types can derive {@code Eq} and {@code Hash}: every component a leaf that has them, or a model
     *  type that does (a fixpoint, the types being recursive); a protocol type never does (lite-front derives
     *  {@code PartialEq} alone), nor a {@code double} or an open slot. */
    private void hashability() {
        for (Class<?> c : modelNames.keySet()) {
            modelHashable.put(c, true);
        }
        boolean changed = true;
        while (changed) {
            changed = false;
            for (Class<?> c : modelNames.keySet()) {
                if (!modelHashable.get(c)) {
                    continue;
                }
                boolean ok = true;
                if (c.isRecord()) {
                    for (RecordComponent rc : c.getRecordComponents()) {
                        ok &= hashable(rc.getGenericType());
                    }
                } else if (c.isSealed()) {
                    for (Class<?> s : c.getPermittedSubclasses()) {
                        ok &= hashable(s);
                    }
                }
                if (!ok) {
                    modelHashable.put(c, false);
                    changed = true;
                }
            }
        }
    }

    private boolean hashable(Type t) {
        if (t instanceof ParameterizedType p) {
            boolean ok = true;
            for (Type a : p.getActualTypeArguments()) {
                ok &= hashable(a);
            }
            return ok;
        }
        Class<?> c = (Class<?>) t;
        if (c == int.class || c == Integer.class || c == long.class || c == Long.class || c == boolean.class
                || c == Boolean.class || c == String.class || c == java.math.BigDecimal.class) {
            return true;
        }
        if (isModel(c)) {
            return modelHashable.get(c);
        }
        return false;
    }

    private void modelRecord(StringBuilder out, Class<?> c) throws Exception {
        String name = modelNames.get(c);
        Map<String, Boolean> nulls = nullability(c);
        RecordComponent[] comps = c.getRecordComponents();
        out.append("/// `").append(c.getName()).append("`\n");
        out.append(modelHashable.get(c) ? "#[derive(Clone, Debug, PartialEq, Eq, Hash)]\n" : "#[derive(Clone, Debug, PartialEq)]\n");
        out.append("pub struct ").append(name).append(" {\n");
        for (RecordComponent rc : comps) {
            RustType t = modelComponentType(c, rc, "v");
            String spelling = nulls.get(rc.getName()) ? "Option<" + t.spelling() + ">" : t.spelling();
            out.append("    pub ").append(field(rc.getName())).append(": ").append(spelling).append(",\n");
        }
        out.append("}\n\n");
        out.append("impl Codec for ").append(name).append(" {\n");
        out.append("    fn write(&self, w: &mut Writer, exact: bool) {\n");
        out.append("        w.raw(\"{\");\n");
        String tagLit = jsonLiteral(RecordCodec.tag(c));
        if (comps.length == 0) {
            out.append("        if !exact { w.raw(\"\\\"@\\\":\"); w.raw(").append(rustString(tagLit)).append("); }\n");
        } else {
            out.append("        if !exact { w.raw(\"\\\"@\\\":\"); w.raw(").append(rustString(tagLit))
                    .append("); w.raw(\",\"); }\n");
        }
        for (int i = 0; i < comps.length; i++) {
            RecordComponent rc = comps[i];
            String key = (i > 0 ? "," : "") + jsonLiteral(rc.getName()) + ":";
            out.append("        w.raw(").append(rustString(key)).append(");\n");
            String access = "self." + field(rc.getName());
            RustType t = modelComponentType(c, rc, "v");
            if (nulls.get(rc.getName())) {
                out.append("        match &").append(access).append(" { None => w.raw(\"null\"), Some(v) => { ")
                        .append(t.write()).append(" } }\n");
            } else {
                out.append("        { let v = &").append(access).append("; ").append(t.write()).append(" }\n");
            }
        }
        out.append("        w.raw(\"}\");\n    }\n}\n\n");
    }

    /** A model record's component in Rust: its declared type's ({@link #modelType}), a list whose elements its
     *  source says may be null an {@code Arc<[Option<_>]>}. */
    private RustType modelComponentType(Class<?> owner, RecordComponent rc, String v) throws Exception {
        if (!nullableElements(owner).get(rc.getName())) {
            return modelType(rc.getGenericType(), v);
        }
        RustType e = modelType(listElement(rc), "x");
        return new RustType("Arc<[Option<" + e.spelling() + ">]>",
                "{ w.raw(\"[\"); for (i, x) in " + v + ".iter().enumerate() { if i > 0 { w.raw(\",\"); } "
                        + "match x { None => w.raw(\"null\"), Some(x) => { " + e.write() + " } } } w.raw(\"]\"); }");
    }

    /** A declared Java type of a model record's component in Rust, and the statement that writes {@code v}. */
    private RustType modelType(Type t, String v) {
        if (t instanceof ParameterizedType p) {
            Class<?> raw = (Class<?>) p.getRawType();
            Type[] a = p.getActualTypeArguments();
            if (raw == List.class) {
                RustType e = modelType(a[0], "x");
                return new RustType("Arc<[" + e.spelling() + "]>",
                        "{ w.raw(\"[\"); for (i, x) in " + v + ".iter().enumerate() { if i > 0 { w.raw(\",\"); } "
                                + e.write() + " } w.raw(\"]\"); }");
            }
            if (raw == Map.class || raw == LinkedHashMap.class) {
                RustType k = modelType(a[0], "k");
                RustType val = modelType(a[1], "x");
                String spelling = "Arc<[(" + k.spelling() + ", " + val.spelling() + ")]>";
                if (a[0] == String.class) {
                    return new RustType(spelling,
                            "{ w.raw(\"{\"); for (i, (k, x)) in " + v + ".iter().enumerate() { if i > 0 { w.raw(\",\"); } "
                                    + "w.string(k); w.raw(\":\"); " + val.write() + " } w.raw(\"}\"); }");
                }
                return new RustType(spelling,
                        "{ w.raw(\"[\"); for (i, (k, x)) in " + v + ".iter().enumerate() { if i > 0 { w.raw(\",\"); } "
                                + "w.raw(\"[\"); " + k.write() + " w.raw(\",\"); " + val.write() + " w.raw(\"]\"); } "
                                + "w.raw(\"]\"); }");
            }
            throw new IllegalArgumentException("no Rust rule for a model component of " + t.getTypeName());
        }
        Class<?> c = (Class<?>) t;
        if (c == int.class || c == Integer.class) {
            return new RustType("i32", "w.int(i64::from(*" + v + "));");
        }
        if (c == long.class || c == Long.class) {
            return new RustType("i64", "w.int(*" + v + ");");
        }
        if (c == boolean.class || c == Boolean.class) {
            return new RustType("bool", "w.boolean(*" + v + ");");
        }
        if (c == double.class || c == Double.class) {
            return new RustType("f64", "w.double(*" + v + ");");
        }
        if (c == String.class) {
            return new RustType("JStr", "w.string(" + v + ");");
        }
        if (c == java.math.BigDecimal.class) {
            return new RustType("BigDecimal", "w.decimal(" + v + ");");
        }
        if (c == Number.class || c == Object.class) {
            return new RustType("JValue", "w.value(" + v + ");");
        }
        if (isModel(c)) {
            if (c.isRecord()) {
                return new RustType("Arc<" + modelNames.get(c) + ">", v + ".write(w, true);");
            }
            if (c.isSealed() || c.isEnum()) {
                return new RustType(modelNames.get(c), v + ".write(w, true);");
            }
        } else if (names.containsKey(c)) {
            if (c.isRecord()) {
                // a derived property definition is SHARED (2026-11-15, the normalizer batch, item 10): the knowledge
                // layer's association adoption appends the very definition to its owner and the normalizer checks it by
                // identity (ModelNormalizer.requireQualifiedPropertiesAdopted's `d == dp`), so it is an Arc, as a
                // ValueSpecification node is
                boolean shared = family(c) || c == com.legend.protocol.DerivedPropertyDefinition.class;
                return new RustType((shared ? "Arc<p::" : "Box<p::") + names.get(c) + ">", v + ".write(w, true);");
            }
            return new RustType("p::" + names.get(c), v + ".write(w, true);");
        }
        throw new IllegalArgumentException("no Rust rule for a model component of " + c.getName());
    }

    private void modelSealedType(StringBuilder out, Class<?> c) {
        String name = modelNames.get(c);
        out.append("/// `").append(c.getName()).append("` (sealed)\n");
        out.append(modelHashable.get(c) ? "#[derive(Clone, Debug, PartialEq, Eq, Hash)]\n" : "#[derive(Clone, Debug, PartialEq)]\n");
        out.append("pub enum ").append(name).append(" {\n");
        for (Class<?> s : c.getPermittedSubclasses()) {
            String sn = modelNames.get(s);
            out.append("    ").append(sn).append("(").append(s.isRecord() ? "Arc<" + sn + ">" : sn).append("),\n");
        }
        out.append("}\n\n");
        out.append("impl Codec for ").append(name).append(" {\n");
        out.append("    fn write(&self, w: &mut Writer, _exact: bool) {\n        match self {\n");
        for (Class<?> s : c.getPermittedSubclasses()) {
            out.append("            ").append(name).append("::").append(modelNames.get(s)).append("(x) => x.write(w, false),\n");
        }
        out.append("        }\n    }\n}\n\n");
    }

    private void modelEnumeration(StringBuilder out, Class<?> c) {
        String name = modelNames.get(c);
        out.append("/// `").append(c.getName()).append("` (enum)\n");
        out.append("#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]\npub enum ").append(name).append(" {\n");
        for (Object k : c.getEnumConstants()) {
            out.append("    ").append(((Enum<?>) k).name()).append(",\n");
        }
        out.append("}\n\n");
        out.append("impl Codec for ").append(name).append(" {\n");
        out.append("    fn write(&self, w: &mut Writer, _exact: bool) {\n        w.raw(match self {\n");
        for (Object k : c.getEnumConstants()) {
            String n = ((Enum<?>) k).name();
            out.append("            ").append(name).append("::").append(n).append(" => ")
                    .append(rustString(jsonLiteral(n))).append(",\n");
        }
        out.append("        });\n    }\n}\n\n");
        // Java's values(), name(), ordinal() and valueOf(name), which the model's code calls
        Object[] constants = c.getEnumConstants();
        out.append("impl ").append(name).append(" {\n");
        out.append("    /// Java's `values()`, in declaration order.\n");
        out.append("    pub const VALUES: [").append(name).append("; ").append(constants.length).append("] = [");
        for (int i = 0; i < constants.length; i++) {
            out.append(i > 0 ? ", " : "").append(name).append("::").append(((Enum<?>) constants[i]).name());
        }
        out.append("];\n\n");
        out.append("    /// Java's `name()`.\n    pub fn name(self) -> &'static str {\n        match self {\n");
        for (Object k : constants) {
            String n = ((Enum<?>) k).name();
            out.append("            ").append(name).append("::").append(n).append(" => ").append(rustString(n))
                    .append(",\n");
        }
        out.append("        }\n    }\n\n");
        out.append("    /// Java's `ordinal()`.\n    pub fn ordinal(self) -> i32 {\n        self as i32\n    }\n\n");
        out.append("    /// Java's `valueOf(name)`: `None` where it throws.\n");
        out.append("    pub fn value_of(name: &[u16]) -> Option<").append(name).append("> {\n");
        out.append("        ").append(name).append("::VALUES.into_iter().find(|v| v.name().encode_utf16().eq(name.iter().copied()))\n");
        out.append("    }\n}\n\n");
    }

    // ------------------------------------------------------------------ ElementAnswers' records (element mode)

    /*
     * THE ELEMENT DIFFERENTIAL'S RECORDS IN RUST (2026-11-15, docs/rust-port/PHASE3.md, the element slice, step 5): Rust
     * mirrors of what ElementAnswers writes with the record codec -- its projections (ClassView, ConstraintView,
     * FunctionView) and the records they hold that are neither the model's nor the protocol's: compiler.element's
     * Property (Stored, Derived), TypedEnum, TypedParameter and TypedConstraint.EnforcementLevel, and the
     * compiler.element.type family (Type and its records and enums, Param, Column, Op; Multiplicity's) -- with the same
     * codec's writer, so rust/lite-element's differential writes ElementAnswers' text byte for byte from one schema
     * (rust/lite-element/tests/support/oracle_records.rs). A model record they hold is spelled as rust/lite-model holds
     * it (lite_model::, an Arc), a protocol record as rust/lite-front does (p::); the walk stops there. A mirror record
     * is held as Box<R>, a list as Vec<T>, an Optional as Option<T>. An enum honours its slot's exactness, as the codec
     * does (RecordCodec.value's leaf): its bare name where the slot declares the enum, {"@":tag,"$":name} where it does
     * not -- Type.Primitive in a Type slot. The classes are found by name, never linked: ElementAnswers is test code
     * (core_tests_lib, which depends on this generator's library) and compiler.element is not among this library's deps,
     * so the rule's run supplies both on its classpath.
     */

    private final List<Class<?>> elementRecords = new ArrayList<>();
    private final List<Class<?>> elementSealed = new ArrayList<>();
    private final List<Class<?>> elementEnums = new ArrayList<>();
    private final Map<Class<?>, String> elementNames = new HashMap<>();

    /** The element mode's roots, by binary name. */
    private static final List<String> ELEMENT_ROOTS = List.of(
            "com.legend.frontoracle.ElementAnswers$ClassView",
            "com.legend.frontoracle.ElementAnswers$ConstraintView",
            "com.legend.frontoracle.ElementAnswers$FunctionView",
            "com.legend.compiler.element.Property",
            "com.legend.compiler.element.TypedEnum",
            "com.legend.compiler.element.TypedParameter",
            "com.legend.compiler.element.type.Type",
            "com.legend.compiler.element.type.Multiplicity");

    private String element(Path testSources) throws Exception {
        this.test = testSources;
        // the model's Rust names, which a mirror spells a model record by
        discoverModel(List.of(com.legend.model.ParsedModel.class, com.legend.model.NormalizedModel.class,
                com.legend.model.Function.class, com.legend.model.FunctionId.class,
                com.legend.model.ClassMember.class));
        List<Type> roots = new ArrayList<>();
        for (String n : ELEMENT_ROOTS) {
            // initialize false: a projection's class is read, its outer class (ElementAnswers, which boots the
            // compiler in its static initializer) never run
            roots.add(Class.forName(n, false, FrontCodegen.class.getClassLoader()));
        }
        discoverElement(roots);
        StringBuilder out = new StringBuilder();
        out.append("""
                // GENERATED by //core:element_records_rs (core/src/test/java/com/legend/frontoracle/FrontCodegen.java, its
                // element mode) from ElementAnswers' projections and the compiler.element records they hold -- do not edit;
                // regenerate with `bazel run //rust/lite-element:update_generated`. A struct per record, its components in
                // canonical order (`Option` exactly where the record's source says @Nullable, and for an Optional); an enum
                // per sealed parent and per Java enum; the lossless codec's writer, byte for byte the JVM's (RecordCodec),
                // an enum written as its slot declares it (a bare name, or tagged where a sealed parent holds it). A mirror
                // record is held as `Box<R>`, a list as `Vec<T>`; a model record as rust/lite-model holds it, a protocol
                // record as rust/lite-front does.

                #![allow(clippy::all, non_camel_case_types, unused_variables, unused_imports, dead_code)]

                use std::sync::Arc;

                use lite_front::codec::{Codec, Writer};
                use lite_front::jtypes::{BigDecimal, JStr, JValue};
                use lite_front::records as p;

                """);
        for (Class<?> c : elementRecords) {
            elementRecord(out, c);
        }
        for (Class<?> c : elementSealed) {
            elementSealedType(out, c);
        }
        for (Class<?> c : elementEnums) {
            elementEnumeration(out, c);
        }
        return out.toString();
    }

    private void discoverElement(List<Type> roots) {
        Deque<Type> todo = new ArrayDeque<>(roots);
        Set<Class<?>> done = new HashSet<>();
        while (!todo.isEmpty()) {
            Type t = todo.pop();
            if (t instanceof ParameterizedType p) {
                for (Type a : p.getActualTypeArguments()) {
                    todo.push(a);
                }
                continue;
            }
            if (!(t instanceof Class<?> c) || !done.add(c)) {
                continue;
            }
            if (isModel(c)) {
                if ((c.isRecord() || c.isSealed() || c.isEnum()) && !modelNames.containsKey(c)) {
                    throw new IllegalStateException("ElementAnswers holds " + c.getName()
                            + ", which the model's records do not reach: rust/lite-model has no type for it");
                }
                continue;
            }
            if (names.containsKey(c)) {
                // a protocol record: rust/lite-front's
                continue;
            }
            if (!c.getName().startsWith("com.legend.")) {
                // a leaf or a container: the writer's rules (elementType)
                continue;
            }
            if (c.isRecord()) {
                elementRecords.add(c);
                for (RecordComponent rc : c.getRecordComponents()) {
                    todo.push(rc.getGenericType());
                }
            } else if (c.isSealed()) {
                elementSealed.add(c);
                for (Class<?> s : c.getPermittedSubclasses()) {
                    todo.push(s);
                }
            } else if (c.isEnum()) {
                elementEnums.add(c);
            } else {
                throw new IllegalStateException("ElementAnswers holds " + c.getName()
                        + ", neither a record, a sealed interface nor an enum");
            }
        }
        Map<String, Class<?>> byElementName = new HashMap<>();
        for (List<Class<?>> l : List.of(elementRecords, elementSealed, elementEnums)) {
            l.sort(java.util.Comparator.comparing(RecordCodec::tag));
            for (Class<?> c : l) {
                String n = elementName(c);
                Class<?> before = byElementName.putIfAbsent(n, c);
                if (before != null) {
                    throw new IllegalStateException("two classes map to the Rust name " + n + ": " + before + ", " + c);
                }
                elementNames.put(c, n);
            }
        }
    }

    /** A mirror's Rust name: ElementAnswers' projections by their own ({@code ClassView}), every other as
     *  {@link #rustName} names it ({@code Type$Primitive}: {@code TypePrimitive}). */
    private static String elementName(Class<?> c) {
        String tag = RecordCodec.tag(c);
        String projections = "frontoracle.ElementAnswers$";
        return tag.startsWith(projections) ? tag.substring(projections.length()).replace("$", "") : rustName(c);
    }

    private void elementRecord(StringBuilder out, Class<?> c) throws Exception {
        String name = elementNames.get(c);
        Map<String, Boolean> nulls = nullability(c);
        RecordComponent[] comps = c.getRecordComponents();
        out.append("/// `").append(c.getName()).append("`\n");
        out.append("#[derive(Clone, Debug, PartialEq)]\npub struct ").append(name).append(" {\n");
        for (RecordComponent rc : comps) {
            if (nullableElements(c).get(rc.getName())) {
                throw new IllegalStateException(c.getName() + "." + rc.getName()
                        + ": a list of nullable elements has no element-mode rule");
            }
            RustType t = elementType(rc.getGenericType(), "v");
            String spelling = nulls.get(rc.getName()) ? "Option<" + t.spelling() + ">" : t.spelling();
            out.append("    pub ").append(field(rc.getName())).append(": ").append(spelling).append(",\n");
        }
        out.append("}\n\n");
        out.append("impl Codec for ").append(name).append(" {\n");
        out.append("    fn write(&self, w: &mut Writer, exact: bool) {\n");
        out.append("        w.raw(\"{\");\n");
        String tagLit = jsonLiteral(RecordCodec.tag(c));
        if (comps.length == 0) {
            out.append("        if !exact { w.raw(\"\\\"@\\\":\"); w.raw(").append(rustString(tagLit)).append("); }\n");
        } else {
            out.append("        if !exact { w.raw(\"\\\"@\\\":\"); w.raw(").append(rustString(tagLit))
                    .append("); w.raw(\",\"); }\n");
        }
        for (int i = 0; i < comps.length; i++) {
            RecordComponent rc = comps[i];
            String key = (i > 0 ? "," : "") + jsonLiteral(rc.getName()) + ":";
            out.append("        w.raw(").append(rustString(key)).append(");\n");
            String access = "self." + field(rc.getName());
            RustType t = elementType(rc.getGenericType(), "v");
            if (nulls.get(rc.getName())) {
                out.append("        match &").append(access).append(" { None => w.raw(\"null\"), Some(v) => { ")
                        .append(t.write()).append(" } }\n");
            } else {
                out.append("        { let v = &").append(access).append("; ").append(t.write()).append(" }\n");
            }
        }
        out.append("        w.raw(\"}\");\n    }\n}\n\n");
    }

    /** A declared Java type of a mirror's component in Rust, and the statement that writes {@code v} (an expression of
     *  a reference to it). */
    private RustType elementType(Type t, String v) {
        if (t instanceof ParameterizedType p) {
            Class<?> raw = (Class<?>) p.getRawType();
            Type[] a = p.getActualTypeArguments();
            if (raw == List.class) {
                RustType e = elementType(a[0], "x");
                return new RustType("Vec<" + e.spelling() + ">",
                        "{ w.raw(\"[\"); for (i, x) in " + v + ".iter().enumerate() { if i > 0 { w.raw(\",\"); } "
                                + e.write() + " } w.raw(\"]\"); }");
            }
            if (raw == java.util.Optional.class) {
                // RecordCodec: the value as its argument's type, or null
                RustType e = elementType(a[0], "o");
                return new RustType("Option<" + e.spelling() + ">",
                        "match " + v + " { None => w.raw(\"null\"), Some(o) => { " + e.write() + " } }");
            }
            throw new IllegalArgumentException("no element-mode rule for " + t.getTypeName());
        }
        Class<?> c = (Class<?>) t;
        if (c == int.class || c == Integer.class) {
            return new RustType("i32", "w.int(i64::from(*" + v + "));");
        }
        if (c == long.class || c == Long.class) {
            return new RustType("i64", "w.int(*" + v + ");");
        }
        if (c == boolean.class || c == Boolean.class) {
            return new RustType("bool", "w.boolean(*" + v + ");");
        }
        if (c == String.class) {
            return new RustType("JStr", "w.string(" + v + ");");
        }
        if (isModel(c)) {
            if (c.isRecord()) {
                return new RustType("Arc<lite_model::" + modelNames.get(c) + ">", v + ".write(w, true);");
            }
            return new RustType("lite_model::" + modelNames.get(c), v + ".write(w, true);");
        }
        if (names.containsKey(c)) {
            if (c.isRecord()) {
                boolean shared = family(c) || c == com.legend.protocol.DerivedPropertyDefinition.class;
                return new RustType((shared ? "Arc<p::" : "Box<p::") + names.get(c) + ">", v + ".write(w, true);");
            }
            return new RustType("p::" + names.get(c), v + ".write(w, true);");
        }
        if (elementNames.containsKey(c)) {
            if (c.isRecord()) {
                return new RustType("Box<" + elementNames.get(c) + ">", v + ".write(w, true);");
            }
            return new RustType(elementNames.get(c), v + ".write(w, true);");
        }
        throw new IllegalArgumentException("no element-mode rule for " + c.getName());
    }

    private void elementSealedType(StringBuilder out, Class<?> c) {
        String name = elementNames.get(c);
        out.append("/// `").append(c.getName()).append("` (sealed)\n");
        out.append("#[derive(Clone, Debug, PartialEq)]\npub enum ").append(name).append(" {\n");
        for (Class<?> s : c.getPermittedSubclasses()) {
            String sn = elementNames.get(s);
            out.append("    ").append(sn).append("(").append(s.isRecord() ? "Box<" + sn + ">" : sn).append("),\n");
        }
        out.append("}\n\n");
        // a value in a slot declared as this parent is never exact: each record writes its tag, each enum its tagged leaf
        out.append("impl Codec for ").append(name).append(" {\n");
        out.append("    fn write(&self, w: &mut Writer, _exact: bool) {\n        match self {\n");
        for (Class<?> s : c.getPermittedSubclasses()) {
            out.append("            ").append(name).append("::").append(elementNames.get(s))
                    .append("(x) => x.write(w, false),\n");
        }
        out.append("        }\n    }\n}\n\n");
    }

    private void elementEnumeration(StringBuilder out, Class<?> c) {
        String name = elementNames.get(c);
        Object[] constants = c.getEnumConstants();
        out.append("/// `").append(c.getName()).append("` (enum)\n");
        out.append("#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]\npub enum ").append(name).append(" {\n");
        for (Object k : constants) {
            out.append("    ").append(((Enum<?>) k).name()).append(",\n");
        }
        out.append("}\n\n");
        // RecordCodec's leaf: the name where the slot declares the enum, else {"@":tag,"$":name}
        out.append("impl Codec for ").append(name).append(" {\n");
        out.append("    fn write(&self, w: &mut Writer, exact: bool) {\n");
        out.append("        let name = match self {\n");
        for (Object k : constants) {
            String n = ((Enum<?>) k).name();
            out.append("            ").append(name).append("::").append(n).append(" => ")
                    .append(rustString(jsonLiteral(n))).append(",\n");
        }
        out.append("        };\n");
        out.append("        if exact { w.raw(name); return; }\n");
        out.append("        w.raw(\"{\\\"@\\\":\"); w.raw(").append(rustString(jsonLiteral(RecordCodec.tag(c))))
                .append("); w.raw(\",\\\"$\\\":\"); w.raw(name); w.raw(\"}\");\n");
        out.append("    }\n}\n\n");
        out.append("impl ").append(name).append(" {\n");
        out.append("    /// Java's `values()`, in declaration order.\n");
        out.append("    pub const VALUES: [").append(name).append("; ").append(constants.length).append("] = [");
        for (int i = 0; i < constants.length; i++) {
            out.append(i > 0 ? ", " : "").append(name).append("::").append(((Enum<?>) constants[i]).name());
        }
        out.append("];\n\n");
        out.append("    /// Java's `name()`.\n    pub fn name(self) -> &'static str {\n        match self {\n");
        for (Object k : constants) {
            String n = ((Enum<?>) k).name();
            out.append("            ").append(name).append("::").append(n).append(" => ").append(rustString(n))
                    .append(",\n");
        }
        out.append("        }\n    }\n\n");
        out.append("    /// Java's `valueOf(name)`: `None` where it throws.\n");
        out.append("    pub fn value_of(name: &str) -> Option<").append(name).append("> {\n");
        out.append("        ").append(name).append("::VALUES.into_iter().find(|v| v.name() == name)\n");
        out.append("    }\n}\n\n");
    }

    // ------------------------------------------------------------------ literals

    private static String jsonLiteral(String s) {
        StringBuilder b = new StringBuilder();
        RecordCodec.string(b, s);
        return b.toString();
    }

    /** A Rust string literal of {@code s}. */
    private static String rustString(String s) {
        return "\"" + s.replace("\\", "\\\\").replace("\"", "\\\"") + "\"";
    }

    /** A Java string literal of {@code s}, which holds only printable ASCII (a JSON key: a component's name). */
    private static String javaString(String s) {
        for (int i = 0; i < s.length(); i++) {
            if (s.charAt(i) < 0x20 || s.charAt(i) > 0x7e) {
                throw new IllegalStateException("a key the generator does not spell in Java: " + s);
            }
        }
        return "\"" + s.replace("\\", "\\\\").replace("\"", "\\\"") + "\"";
    }
}
