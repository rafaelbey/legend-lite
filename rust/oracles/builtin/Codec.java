// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

import java.lang.reflect.GenericArrayType;
import java.lang.reflect.ParameterizedType;
import java.lang.reflect.RecordComponent;
import java.lang.reflect.Type;
import java.lang.reflect.WildcardType;
import java.math.BigDecimal;
import java.math.BigInteger;
import java.util.List;
import java.util.Map;
import java.util.Optional;
import java.util.Set;

/**
 * The lossless record codec's writer, the tables' copy (2026-11-12, docs/rust-port/PHASE3.md slice 1b): the form of
 * core/src/test/java/com/legend/frontoracle/RecordCodec.java, byte for byte -- a record a JSON object of its
 * components in canonical-constructor order, a {@code "@"} tag where the declared type does not fix the runtime class,
 * a list an array, a map with string keys an object in its iteration order (any other an array of pairs), an enum its
 * name, a {@code double} or a big number a string in Java's spelling, a string with every surrogate escaped -- which
 * rust/lite-front's and rust/lite-model's generated writers write too. A copy because that class is package-private
 * in core's test tree, and this oracle is a source program beside the frozen file, as rust/oracles/types' is.
 */
final class Codec {

    private Codec() {
    }

    /** {@code value} written, its declared type deciding the tags. */
    static String write(Object value, Type declared) {
        StringBuilder out = new StringBuilder();
        value(out, value, declared);
        return out.toString();
    }

    private static void value(StringBuilder out, Object o, Type declared) {
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
        } else if (o.getClass().isRecord()) {
            record(out, (Record) o, exact);
        } else {
            throw new IllegalArgumentException("the record codec has no rule for " + o.getClass().getName()
                    + " (declared " + declared.getTypeName() + ")");
        }
    }

    private static void leaf(StringBuilder out, boolean exact, Object o, Runnable write) {
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

    private static void record(StringBuilder out, Record r, boolean exact) {
        String tag = tag(r.getClass());
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
        return n.startsWith("com.legend.") ? n.substring("com.legend.".length()) : n;
    }

    private static Class<?> erase(Type t) {
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
}
