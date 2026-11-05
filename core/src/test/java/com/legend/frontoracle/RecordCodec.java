// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

package com.legend.frontoracle;

import java.lang.reflect.GenericArrayType;
import java.lang.reflect.ParameterizedType;
import java.lang.reflect.RecordComponent;
import java.lang.reflect.Type;
import java.lang.reflect.WildcardType;
import java.math.BigDecimal;
import java.math.BigInteger;
import java.util.ArrayDeque;
import java.util.Deque;
import java.util.IdentityHashMap;
import java.util.List;
import java.util.Map;
import java.util.Optional;
import java.util.Set;
import java.util.TreeMap;
import java.util.TreeSet;

/**
 * THE LOSSLESS RECORD CODEC'S WRITER AND SCHEMA (Rust port phase 2, step 1: docs/rust-port/PHASE2.md §3), by
 * reflection -- test code, as the product bans reflection (ArchitectureTest.reflectionIsBannedInProduction): the
 * shim's reader is generated from the schema this writes.
 *
 * <p>The form: a record is a JSON object of its components in canonical-constructor order, nulls explicit, with an
 * {@code "@"} type tag first only where the declared type does not already fix the runtime class (a sealed slot, an
 * {@code Object} slot); a list is an array; a map with string keys an object in its iteration order, any other map an
 * array of {@code [key, value]} pairs, a lone map entry one such pair; an enum is its name; {@code double}, {@code BigDecimal} and {@code BigInteger}
 * are strings in Java's spelling, so no number loses a digit; a string escapes {@code "} and {@code \}, the control
 * characters, and every surrogate as {@code \\uXXXX}, so a lone one survives. Anything else has no rule and throws
 * (AGENTS.md invariant 4): the codec is complete or it fails.
 *
 * <p>It measures as it writes: the record types reached, the records written twice (the same object in two places,
 * which the shim would copy), and the runtime classes of the maps and sets met (a {@code HashMap}'s order is a parity
 * trap).
 */
final class RecordCodec {

    private static final String ROOT = "com.legend.";

    /** Tag of each record type reached. */
    final Set<String> reached = new TreeSet<>();
    /** Per record tag, how many times one object was met again within one answer. */
    final Map<String, Integer> shared = new TreeMap<>();
    /** The runtime classes of the maps and sets met. */
    final Set<String> containers = new TreeSet<>();

    private final IdentityHashMap<Object, Boolean> seen = new IdentityHashMap<>();

    /** One answer, written: {@code declared} is the type its door returns. */
    String write(Object value, Type declared) {
        seen.clear();
        StringBuilder out = new StringBuilder();
        value(out, value, declared);
        return out.toString();
    }

    private void value(StringBuilder out, Object o, Type declared) {
        if (o == null) {
            out.append("null");
            return;
        }
        Class<?> erased = erase(declared);
        boolean exact = erased == o.getClass();
        if (o instanceof String s) {
            leaf(out, exact, o, () -> string(out, s));
        } else if (o instanceof Boolean || o instanceof Integer || o instanceof Long || o instanceof Short
                || o instanceof Byte) {
            leaf(out, exact || erased.isPrimitive(), o, () -> out.append(o));
        } else if (o instanceof Character c) {
            leaf(out, exact || erased.isPrimitive(), o, () -> string(out, String.valueOf(c)));
        } else if (o instanceof Double || o instanceof Float || o instanceof BigDecimal || o instanceof BigInteger) {
            leaf(out, exact || erased.isPrimitive(), o, () -> string(out, o.toString()));
        } else if (o instanceof Enum<?> e) {
            leaf(out, erased == e.getDeclaringClass(), o, () -> string(out, e.name()));
        } else if (o instanceof Optional<?> opt) {
            containers.add(o.getClass().getName());
            value(out, opt.orElse(null), argument(declared, 0));
        } else if (o instanceof List<?> list) {
            Type element = argument(declared, 0);
            out.append('[');
            for (int i = 0; i < list.size(); i++) {
                if (i > 0) {
                    out.append(',');
                }
                value(out, list.get(i), element);
            }
            out.append(']');
        } else if (o instanceof Set<?> set) {
            containers.add(o.getClass().getName());
            Type element = argument(declared, 0);
            out.append('[');
            boolean first = true;
            for (Object x : set) {
                if (!first) {
                    out.append(',');
                }
                first = false;
                value(out, x, element);
            }
            out.append(']');
        } else if (o instanceof Map<?, ?> map) {
            containers.add(o.getClass().getName());
            Type key = argument(declared, 0);
            Type val = argument(declared, 1);
            boolean stringKeys = erase(key) == String.class;
            out.append(stringKeys ? '{' : '[');
            boolean first = true;
            for (Map.Entry<?, ?> e : map.entrySet()) {
                if (!first) {
                    out.append(',');
                }
                first = false;
                if (stringKeys) {
                    string(out, (String) e.getKey());
                    out.append(':');
                    value(out, e.getValue(), val);
                } else {
                    out.append('[');
                    value(out, e.getKey(), key);
                    out.append(',');
                    value(out, e.getValue(), val);
                    out.append(']');
                }
            }
            out.append(stringKeys ? '}' : ']');
        } else if (o instanceof Map.Entry<?, ?> entry) {
            containers.add("java.util.Map$Entry");
            out.append('[');
            value(out, entry.getKey(), argument(declared, 0));
            out.append(',');
            value(out, entry.getValue(), argument(declared, 1));
            out.append(']');
        } else if (o.getClass().isRecord()) {
            record(out, (Record) o, exact);
        } else {
            throw new IllegalArgumentException("the record codec has no rule for " + o.getClass().getName()
                    + " (declared " + declared.getTypeName() + ")");
        }
    }

    private void leaf(StringBuilder out, boolean exact, Object o, Runnable write) {
        if (exact) {
            write.run();
            return;
        }
        out.append("{\"@\":");
        string(out, tag(o.getClass()));
        out.append(",\"$\":");
        write.run();
        out.append('}');
    }

    private void record(StringBuilder out, Record r, boolean exact) {
        String tag = tag(r.getClass());
        reached.add(tag);
        if (seen.put(r, Boolean.TRUE) != null) {
            shared.merge(tag, 1, Integer::sum);
        }
        out.append('{');
        boolean first = true;
        if (!exact) {
            out.append("\"@\":");
            string(out, tag);
            first = false;
        }
        for (RecordComponent c : r.getClass().getRecordComponents()) {
            if (!first) {
                out.append(',');
            }
            first = false;
            string(out, c.getName());
            out.append(':');
            Object v;
            try {
                v = c.getAccessor().invoke(r);
            } catch (ReflectiveOperationException e) {
                throw new IllegalStateException("cannot read " + tag + "." + c.getName(), e);
            }
            value(out, v, c.getGenericType());
        }
        out.append('}');
    }

    /** A class's tag: its binary name without the {@code com.legend.} prefix. */
    static String tag(Class<?> c) {
        String n = c.getName();
        return n.startsWith(ROOT) ? n.substring(ROOT.length()) : n;
    }

    static Class<?> erase(Type t) {
        if (t instanceof Class<?> c) {
            return c;
        }
        if (t instanceof ParameterizedType p) {
            return erase(p.getRawType());
        }
        if (t instanceof WildcardType w) {
            return erase(w.getUpperBounds()[0]);
        }
        if (t instanceof GenericArrayType) {
            return Object[].class;
        }
        return Object.class;
    }

    private static Type argument(Type t, int i) {
        return t instanceof ParameterizedType p ? p.getActualTypeArguments()[i] : Object.class;
    }

    /** A JSON string: {@code "} and {@code \} escaped, the short escapes, other controls and every surrogate as
     *  {@code \\uXXXX}. */
    static void string(StringBuilder out, String s) {
        out.append('"');
        for (int i = 0; i < s.length(); i++) {
            char c = s.charAt(i);
            switch (c) {
                case '"' -> out.append("\\\"");
                case '\\' -> out.append("\\\\");
                case '\n' -> out.append("\\n");
                case '\r' -> out.append("\\r");
                case '\t' -> out.append("\\t");
                case '\b' -> out.append("\\b");
                case '\f' -> out.append("\\f");
                default -> {
                    if (c < 0x20 || Character.isSurrogate(c)) {
                        String hex = Integer.toHexString(c);
                        out.append("\\u").append("0000", hex.length(), 4).append(hex);
                    } else {
                        out.append(c);
                    }
                }
            }
        }
        out.append('"');
    }

    /**
     * The schema: every record type reachable from {@code roots} through record components and sealed interfaces'
     * permitted subclasses -- its ordered components and their declared types -- and every sealed parent with its
     * subtypes. A declared type that is neither a record, a sealed interface, nor a known leaf or container is listed
     * as open: the codec cannot enumerate it, so the Rust side cannot cover it.
     */
    static String schema(List<Type> roots) {
        Map<String, List<String[]>> records = new TreeMap<>();
        Map<String, List<String>> sealed = new TreeMap<>();
        Set<String> leaves = new TreeSet<>();
        Set<String> open = new TreeSet<>();
        Deque<Type> todo = new ArrayDeque<>(roots);
        Set<Type> done = new java.util.HashSet<>();
        while (!todo.isEmpty()) {
            Type t = todo.pop();
            if (!done.add(t)) {
                continue;
            }
            if (t instanceof ParameterizedType p) {
                todo.push(p.getRawType());
                for (Type a : p.getActualTypeArguments()) {
                    todo.push(a);
                }
                continue;
            }
            if (t instanceof WildcardType w) {
                for (Type b : w.getUpperBounds()) {
                    todo.push(b);
                }
                continue;
            }
            Class<?> c = erase(t);
            if (c.isRecord()) {
                List<String[]> comps = new java.util.ArrayList<>();
                for (RecordComponent rc : c.getRecordComponents()) {
                    comps.add(new String[] {rc.getName(), rc.getGenericType().getTypeName()});
                    todo.push(rc.getGenericType());
                }
                records.put(tag(c), comps);
            } else if (c.isSealed()) {
                List<String> subs = new java.util.ArrayList<>();
                for (Class<?> s : c.getPermittedSubclasses()) {
                    subs.add(tag(s));
                    todo.push(s);
                }
                sealed.put(tag(c), subs);
            } else if (c.isPrimitive() || c.isEnum() || c == String.class || c == Boolean.class
                    || Number.class.isAssignableFrom(c) || c == Character.class) {
                leaves.add(tag(c));
            } else if (c == List.class || c == Map.class || c == Set.class || c == Optional.class
                    || c == Map.Entry.class || c == java.util.LinkedHashMap.class) {
                leaves.add(c.getName());
            } else {
                open.add(c.getName());
            }
        }
        StringBuilder out = new StringBuilder("{\"records\":{");
        boolean first = true;
        for (Map.Entry<String, List<String[]>> e : records.entrySet()) {
            if (!first) {
                out.append(',');
            }
            first = false;
            out.append('\n');
            string(out, e.getKey());
            out.append(":[");
            for (int i = 0; i < e.getValue().size(); i++) {
                if (i > 0) {
                    out.append(',');
                }
                out.append('[');
                string(out, e.getValue().get(i)[0]);
                out.append(',');
                string(out, e.getValue().get(i)[1]);
                out.append(']');
            }
            out.append(']');
        }
        out.append("\n},\"sealed\":{");
        first = true;
        for (Map.Entry<String, List<String>> e : sealed.entrySet()) {
            if (!first) {
                out.append(',');
            }
            first = false;
            out.append('\n');
            string(out, e.getKey());
            out.append(':');
            stringArray(out, e.getValue());
        }
        out.append("\n},\"leaves\":");
        stringArray(out, List.copyOf(leaves));
        out.append(",\n\"open\":");
        stringArray(out, List.copyOf(open));
        out.append("}\n");
        return out.toString();
    }

    private static void stringArray(StringBuilder out, List<String> values) {
        out.append('[');
        for (int i = 0; i < values.size(); i++) {
            if (i > 0) {
                out.append(',');
            }
            string(out, values.get(i));
        }
        out.append(']');
    }
}
