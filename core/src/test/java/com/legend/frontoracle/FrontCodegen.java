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
 * <pre>a java_run: FrontCodegen OUT_RS CORE_MAIN_DIR  (CORE_MAIN_DIR: core/src/main/java/com/legend)</pre>
 */
public final class FrontCodegen {

    /** A Rust type's spelling, and how the writer writes a value of it. */
    private record RustType(String spelling, String write) {
    }

    private final Path main;
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

    /** {@code rust OUT MAIN}: the Rust records and writer; {@code java OUT MAIN}: the Java reader. */
    public static void main(String[] args) throws Exception {
        FrontCodegen g = new FrontCodegen(Path.of(args[2]));
        g.discover(List.of(PmcdParser.Parsed.class, ValueSpecification.class, LambdaFunction.class));
        String out = switch (args[0]) {
            case "rust" -> g.rust();
            case "java" -> g.java();
            default -> throw new IllegalArgumentException("a mode is rust or java, not " + args[0]);
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
                // Java enum; the lossless codec's writer, byte for byte the JVM's (RecordCodec).

                #![allow(clippy::all, non_camel_case_types, unused_variables)]

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
     *  {@code Vec<Option<_>>}, each null element written {@code null}. */
    private RustType componentType(Class<?> owner, RecordComponent rc, String v) throws Exception {
        if (!nullableElements(owner).get(rc.getName())) {
            return type(rc.getGenericType(), v);
        }
        RustType e = type(listElement(rc), "x");
        return new RustType("Vec<Option<" + e.spelling() + ">>",
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

    /** A declared Java type in Rust, and the statement that writes {@code v} (an expression of a reference to it). */
    private RustType type(Type t, String v) {
        if (t instanceof ParameterizedType p) {
            Class<?> raw = (Class<?>) p.getRawType();
            Type[] a = p.getActualTypeArguments();
            if (raw == List.class) {
                RustType e = type(a[0], "x");
                return new RustType("Vec<" + e.spelling() + ">",
                        "{ w.raw(\"[\"); for (i, x) in " + v + ".iter().enumerate() { if i > 0 { w.raw(\",\"); } "
                                + e.write() + " } w.raw(\"]\"); }");
            }
            if (raw == Map.class || raw == LinkedHashMap.class) {
                RustType k = type(a[0], "k");
                RustType val = type(a[1], "x");
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
                RustType k = type(a[0], "ek");
                RustType val = type(a[1], "ev");
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
            return new RustType("Box<" + names.get(c) + ">", v + ".write(w, true);");
        }
        if (c.isSealed() || c.isEnum()) {
            return new RustType(names.get(c), v + ".write(w, true);");
        }
        throw new IllegalArgumentException("no Rust rule for " + c.getName());
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
            out.append("    ").append(sn).append("(").append(s.isRecord() ? "Box<" + sn + ">" : sn).append("),\n");
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
