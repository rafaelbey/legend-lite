// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

package com.legend.ffi;

import com.legend.base.Nullable;
import com.legend.json.Json;
import java.math.BigDecimal;
import java.math.BigInteger;
import java.util.ArrayList;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.function.Function;

/**
 * The lossless record codec's leaves, read (docs/rust-port/PHASE2.md §3): what {@link FrontRecords}' generated
 * readers call. The form is the writer's (lite-front's codec.rs, the JVM's RecordCodec): a field is always present,
 * {@code null} only where the component is nullable; a {@code double}, a {@code BigDecimal} and a {@code BigInteger}
 * are strings in Java's spelling, read back to the same value; a leaf in an open slot is {@code {"@":class,"$":value}}.
 * A form that is not the writer's is refused, never guessed at (AGENTS.md invariant 4).
 */
final class FrontRead {

    private FrontRead() {
    }

    static Json.Obj object(Json.Node n) {
        if (n instanceof Json.Obj o) {
            return o;
        }
        throw new IllegalArgumentException("the codec wrote a record as an object, not " + n);
    }

    /** A record's component: always written, so a missing one is a defect of the writer. */
    static Json.Node field(Json.Obj o, String name) {
        Json.Node n = o.get(name);
        if (n == null) {
            throw new IllegalArgumentException("the codec wrote no component " + name + " in " + o);
        }
        return n;
    }

    /** A record's component read as {@code read} reads it, a refusal naming where ({@code Record.component}), so a
     *  refusal deep in a tree names its path. */
    static <T> T component(Json.Obj o, String where, Function<Json.Node, T> read) {
        try {
            return read.apply(field(o, where.substring(where.lastIndexOf('.') + 1)));
        } catch (IllegalArgumentException e) {
            throw new IllegalArgumentException(where + ": " + e.getMessage(), e);
        }
    }

    /** A nullable component: {@code null} where the codec wrote null, else read as {@link #component} reads it. */
    static <T> @Nullable T nullableComponent(Json.Obj o, String where, Function<Json.Node, T> read) {
        if (field(o, where.substring(where.lastIndexOf('.') + 1)) instanceof Json.Null) {
            return null;
        }
        return component(o, where, read);
    }

    /** The record's tag where its slot is a sealed parent's. */
    static String tag(Json.Node n) {
        return string(field(object(n), "@"));
    }

    static <T> @Nullable T nullable(Json.Node n, Function<Json.Node, T> read) {
        return n instanceof Json.Null ? null : read.apply(n);
    }

    static String string(Json.Node n) {
        if (n instanceof Json.Str s) {
            return s.value();
        }
        throw new IllegalArgumentException("the codec wrote a string, not " + n);
    }

    static Json.Num num(Json.Node n) {
        if (n instanceof Json.Num x && x.isInteger()) {
            return x;
        }
        throw new IllegalArgumentException("the codec wrote an integer, not " + n);
    }

    static int integer(Json.Node n) {
        return Math.toIntExact(num(n).longValue());
    }

    static long longValue(Json.Node n) {
        return num(n).longValue();
    }

    static boolean bool(Json.Node n) {
        if (n instanceof Json.Bool b) {
            return b.value();
        }
        throw new IllegalArgumentException("the codec wrote a boolean, not " + n);
    }

    /** A {@code double}: the string of its {@code Double.toString}, which parses back to the same double. */
    static double doubleValue(Json.Node n) {
        return Double.parseDouble(string(n));
    }

    static BigDecimal decimal(Json.Node n) {
        return new BigDecimal(string(n));
    }

    /** A leaf in a slot declared {@code Number}. */
    static Number number(Json.Node n) {
        if (value(n) instanceof Number x) {
            return x;
        }
        throw new IllegalArgumentException("the codec wrote a number in a Number slot, not " + n);
    }

    /** A leaf in an open slot ({@code Object}): its class, then its value as that class. */
    static Object value(Json.Node n) {
        Json.Obj o = object(n);
        Json.Node v = field(o, "$");
        String cls = string(field(o, "@"));
        return switch (cls) {
            case "java.lang.String" -> string(v);
            case "java.lang.Long" -> longValue(v);
            case "java.lang.Integer" -> integer(v);
            case "java.lang.Boolean" -> bool(v);
            case "java.lang.Double" -> doubleValue(v);
            case "java.math.BigInteger" -> new BigInteger(string(v));
            case "java.math.BigDecimal" -> decimal(v);
            default -> throw new IllegalArgumentException("the codec wrote no leaf of class " + cls);
        };
    }

    static <T> List<T> list(Json.Node n, Function<Json.Node, T> read) {
        if (!(n instanceof Json.Arr a)) {
            throw new IllegalArgumentException("the codec wrote a list as an array, not " + n);
        }
        List<T> out = new ArrayList<>(a.items().size());
        for (Json.Node x : a.items()) {
            out.add(read.apply(x));
        }
        return out;
    }

    /** A list whose elements may be null ({@code List<@Nullable X>} in the record's source). */
    static <T> List<@Nullable T> nullableList(Json.Node n, Function<Json.Node, T> read) {
        if (!(n instanceof Json.Arr a)) {
            throw new IllegalArgumentException("the codec wrote a list as an array, not " + n);
        }
        List<@Nullable T> out = new ArrayList<>(a.items().size());
        for (Json.Node x : a.items()) {
            out.add(x instanceof Json.Null ? null : read.apply(x));
        }
        return out;
    }

    /** A string-keyed map, in the order the codec wrote it (the records' maps are insertion-ordered). */
    static <V> LinkedHashMap<String, V> map(Json.Node n, Function<Json.Node, V> read) {
        LinkedHashMap<String, V> out = new LinkedHashMap<>();
        for (Map.Entry<String, Json.Node> e : object(n).fields().entrySet()) {
            out.put(e.getKey(), read.apply(e.getValue()));
        }
        return out;
    }

    /** A map entry: the codec's {@code [key, value]} pair. */
    static <K, V> Map.Entry<K, V> entry(Json.Node n, Function<Json.Node, K> key, Function<Json.Node, V> value) {
        if (n instanceof Json.Arr a && a.items().size() == 2) {
            return Map.entry(key.apply(a.items().get(0)), value.apply(a.items().get(1)));
        }
        throw new IllegalArgumentException("the codec wrote a map entry as a pair, not " + n);
    }
}
