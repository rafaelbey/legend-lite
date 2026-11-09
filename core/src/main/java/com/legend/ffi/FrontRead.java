// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

package com.legend.ffi;

import com.legend.base.Nullable;
import java.math.BigDecimal;
import java.math.BigInteger;
import java.util.ArrayList;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.function.Function;

/**
 * The lossless record codec's leaves, read (docs/rust-port/PHASE2.md §3): what {@link FrontRecords}' generated
 * readers call, pulling the codec's JSON text through a {@link Cursor} -- a position into the answer's
 * {@code String}, no tree. The form is the writer's (lite-front's codec.rs and records.rs, the JVM's RecordCodec),
 * read in the order they write it: a record is {@code {} its components in canonical order {@code }}, every one
 * written, {@code null} only where the component is nullable, an {@code "@"} tag first exactly where its slot is a
 * sealed parent's; no whitespace; a {@code double}, a {@code BigDecimal} and a {@code BigInteger} are strings in Java's
 * spelling, read back to the same value; a leaf in an open slot is {@code {"@":class,"$":value}}. A form that is not
 * the writer's -- a component missing or out of order, a key the record has not, text after the answer -- is refused,
 * never guessed at (AGENTS.md invariant 4), as an {@link IllegalArgumentException} naming where
 * ({@code Record.component}).
 */
final class FrontRead {

    /** The deepest nesting the reader accepts: the codec's deepest answer for a deep expression (the JSON tree's
     *  reader held the same, {@code Json.Config(1 << 16)}, before the reader streamed). */
    static final int MAX_DEPTH = 1 << 16;

    private FrontRead() {
    }

    /** A pull cursor over one answer: its text, the position read to, and the containers open there. */
    static final class Cursor {

        private final String text;
        private int pos;
        private int depth;

        /** The answer that starts at {@code start} of {@code text}: the shim's answer is read where it lies, no copy. */
        Cursor(String text, int start) {
            this.text = text;
            this.pos = start;
        }

        /** The answer read whole: nothing after its value. */
        void end() {
            if (pos != text.length()) {
                throw refusal("nothing after the answer");
            }
        }

        /** {@code literal} next, consumed; else nothing consumed. */
        private boolean take(String literal) {
            if (text.startsWith(literal, pos)) {
                pos += literal.length();
                return true;
            }
            return false;
        }

        /** {@code c} next, consumed; else nothing consumed. */
        private boolean take(char c) {
            if (pos < text.length() && text.charAt(pos) == c) {
                pos++;
                return true;
            }
            return false;
        }

        private void enter() {
            if (++depth > MAX_DEPTH) {
                throw new TooDeep("the codec's answer nests deeper than " + MAX_DEPTH + ", at " + pos, false);
            }
        }

        private void leave() {
            depth--;
        }

        /** The refusal of what is here: {@code the codec wrote <expected>, not <what is here>}. */
        IllegalArgumentException refusal(String expected) {
            return new IllegalArgumentException("the codec wrote " + expected + ", not " + here());
        }

        /** What is at the position: a few characters of it and the offset, or the answer's end. */
        private String here() {
            if (pos >= text.length()) {
                return "the answer's end (at " + pos + ")";
            }
            int to = Math.min(text.length(), pos + 32);
            return "'" + text.substring(pos, to) + (to < text.length() ? "..." : "") + "' at " + pos;
        }
    }

    // ------------------------------------------------------------------ records

    /** A record's opening brace, where its slot fixes its class (no tag). */
    static void open(Cursor r) {
        if (!r.take('{')) {
            throw r.refusal("a record as an object");
        }
        r.enter();
    }

    /** The record's tag where its slot is a sealed parent's: the brace and {@code "@"}, always first. */
    static String tag(Cursor r) {
        open(r);
        if (!r.take("\"@\":")) {
            throw r.refusal("a record's tag first");
        }
        return string(r);
    }

    /** A record's closing brace, after its last component: {@code value}, the record built. */
    static <T> T close(Cursor r, String record, T value) {
        if (!r.take('}')) {
            throw new IllegalArgumentException(record + ": " + r.refusal("the record's end").getMessage());
        }
        r.leave();
        return value;
    }

    /** A record's component, its key ({@code "name":}, after a comma unless it is the object's first) next: read
     *  as {@code read} reads it, a refusal naming where ({@code Record.component}), so a refusal deep in a tree
     *  names its path. */
    static <T> T component(Cursor r, String where, String key, Function<Cursor, T> read) {
        try {
            key(r, where, key);
            return read.apply(r);
        } catch (TooDeep e) {
            throw e.at(where);
        } catch (IllegalArgumentException e) {
            throw new IllegalArgumentException(where + ": " + e.getMessage(), e);
        }
    }

    /** A nullable component: {@code null} where the codec wrote null, else read as {@link #component} reads it. */
    static <T> @Nullable T nullableComponent(Cursor r, String where, String key, Function<Cursor, T> read) {
        try {
            key(r, where, key);
            return r.take("null") ? null : read.apply(r);
        } catch (TooDeep e) {
            throw e.at(where);
        } catch (IllegalArgumentException e) {
            throw new IllegalArgumentException(where + ": " + e.getMessage(), e);
        }
    }

    /**
     * The depth refusal, named by the innermost component it passes and by no other: its path is as deep as the
     * limit, and each level's name prefixed would make the message's building quadratic in the depth (the tree's
     * reader refused it before reading any record, so with no path at all).
     */
    private static final class TooDeep extends IllegalArgumentException {

        private static final long serialVersionUID = 1L;

        private final boolean named;

        TooDeep(String message, boolean named) {
            super(message);
            this.named = named;
        }

        TooDeep at(String where) {
            return named ? this : new TooDeep(where + ": " + getMessage(), true);
        }
    }

    /** Every component is written, in canonical order: the next key is this one's, or the answer is refused. */
    private static void key(Cursor r, String where, String key) {
        if (!r.take(key)) {
            throw r.refusal("component " + where.substring(where.lastIndexOf('.') + 1) + " next");
        }
    }

    // ------------------------------------------------------------------ leaves

    static String string(Cursor r) {
        String s = r.text;
        int n = s.length();
        int p = r.pos;
        if (p >= n || s.charAt(p) != '"') {
            throw r.refusal("a string");
        }
        int start = p + 1;
        for (int i = start; i < n; i++) {
            char c = s.charAt(i);
            if (c == '"') {
                r.pos = i + 1;
                return s.substring(start, i);
            }
            if (c == '\\') {
                return escaped(r, start, i);
            }
            if (c < 0x20) {
                r.pos = i;
                throw r.refusal("a control character escaped");
            }
        }
        throw r.refusal("a string with its closing quote");
    }

    /** A string with an escape at {@code at}: read as {@code Json.parse} reads the codec's strings -- the short
     *  escapes, and each {@code \\uXXXX} one UTF-16 unit, a lone surrogate kept (PHASE2 §3: UTF-16 inside, the
     *  escape at the edges). */
    private static String escaped(Cursor r, int start, int at) {
        String s = r.text;
        int n = s.length();
        StringBuilder out = new StringBuilder(at - start + 16).append(s, start, at);
        int i = at;
        while (i < n) {
            char c = s.charAt(i);
            if (c == '"') {
                r.pos = i + 1;
                return out.toString();
            }
            if (c == '\\') {
                if (i + 1 >= n) {
                    break;
                }
                char e = s.charAt(i + 1);
                switch (e) {
                    case '"' -> out.append('"');
                    case '\\' -> out.append('\\');
                    case '/' -> out.append('/');
                    case 'n' -> out.append('\n');
                    case 'r' -> out.append('\r');
                    case 't' -> out.append('\t');
                    case 'b' -> out.append('\b');
                    case 'f' -> out.append('\f');
                    case 'u' -> {
                        int unit = i + 6 <= n ? hex4(s, i + 2) : -1;
                        if (unit < 0) {
                            r.pos = i;
                            throw r.refusal("a \\u escape of four hexadecimal digits");
                        }
                        out.append((char) unit);
                        i += 4;
                    }
                    default -> {
                        r.pos = i;
                        throw r.refusal("an escape the codec writes");
                    }
                }
                i += 2;
            } else if (c < 0x20) {
                r.pos = i;
                throw r.refusal("a control character escaped");
            } else {
                out.append(c);
                i++;
            }
        }
        r.pos = n;
        throw r.refusal("a string with its closing quote");
    }

    /** The four hexadecimal digits at {@code at} as a UTF-16 unit, or -1. */
    private static int hex4(String s, int at) {
        int v = 0;
        for (int i = at; i < at + 4; i++) {
            int d = Character.digit(s.charAt(i), 16);
            if (d < 0) {
                return -1;
            }
            v = (v << 4) | d;
        }
        return v;
    }

    /** A JSON integer, as {@code Json.parse} reads one: {@code -?(0|[1-9][0-9]*)}, no fraction, no exponent, within
     *  a long. */
    private static long integerToken(Cursor r) {
        String s = r.text;
        int n = s.length();
        int start = r.pos;
        int p = start;
        boolean negative = p < n && s.charAt(p) == '-';
        if (negative) {
            p++;
        }
        if (p >= n || s.charAt(p) < '0' || s.charAt(p) > '9') {
            throw r.refusal("an integer");
        }
        int digits = p;
        if (s.charAt(p) == '0') {
            p++;
        } else {
            while (p < n && s.charAt(p) >= '0' && s.charAt(p) <= '9') {
                p++;
            }
        }
        if (p < n) {
            char c = s.charAt(p);
            if (c == '.' || c == 'e' || c == 'E' || (c >= '0' && c <= '9')) {
                throw r.refusal("an integer");
            }
        }
        long v;
        if (p - digits <= 18) {
            v = 0;
            for (int i = digits; i < p; i++) {
                v = v * 10 + (s.charAt(i) - '0');
            }
            if (negative) {
                v = -v;
            }
        } else {
            try {
                v = Long.parseLong(s.substring(start, p));
            } catch (NumberFormatException e) {
                throw r.refusal("an integer within a long");
            }
        }
        r.pos = p;
        return v;
    }

    static int integer(Cursor r) {
        return Math.toIntExact(integerToken(r));
    }

    static long longValue(Cursor r) {
        return integerToken(r);
    }

    static boolean bool(Cursor r) {
        if (r.take("true")) {
            return true;
        }
        if (r.take("false")) {
            return false;
        }
        throw r.refusal("a boolean");
    }

    /** A {@code double}: the string of its {@code Double.toString}, which parses back to the same double. */
    static double doubleValue(Cursor r) {
        return Double.parseDouble(string(r));
    }

    static BigDecimal decimal(Cursor r) {
        return new BigDecimal(string(r));
    }

    /** A leaf in a slot declared {@code Number}. */
    static Number number(Cursor r) {
        Object v = value(r);
        if (v instanceof Number x) {
            return x;
        }
        throw new IllegalArgumentException("the codec wrote a number in a Number slot, not the "
                + v.getClass().getName() + " " + v);
    }

    /** A leaf in an open slot ({@code Object}): its class, then its value as that class. */
    static Object value(Cursor r) {
        open(r);
        if (!r.take("\"@\":")) {
            throw r.refusal("a leaf's class first");
        }
        String cls = string(r);
        if (!r.take(",\"$\":")) {
            throw r.refusal("a leaf's value after its class");
        }
        Object v = switch (cls) {
            case "java.lang.String" -> string(r);
            case "java.lang.Long" -> longValue(r);
            case "java.lang.Integer" -> integer(r);
            case "java.lang.Boolean" -> bool(r);
            case "java.lang.Double" -> doubleValue(r);
            case "java.math.BigInteger" -> new BigInteger(string(r));
            case "java.math.BigDecimal" -> decimal(r);
            default -> throw new IllegalArgumentException("the codec wrote no leaf of class " + cls);
        };
        if (!r.take('}')) {
            throw r.refusal("a leaf's end");
        }
        r.leave();
        return v;
    }

    // ------------------------------------------------------------------ containers

    static <T> List<T> list(Cursor r, Function<Cursor, T> read) {
        openArray(r);
        if (r.take(']')) {
            r.leave();
            return new ArrayList<>(0);
        }
        T first = read.apply(r);
        if (r.take(']')) {
            r.leave();
            List<T> one = new ArrayList<>(1);
            one.add(first);
            return one;
        }
        ArrayList<T> out = new ArrayList<>();
        out.add(first);
        while (r.take(',')) {
            out.add(read.apply(r));
        }
        closeArray(r);
        // sized as the tree's reader sized it: a record keeps its list, so no growth's slack is kept with it
        out.trimToSize();
        return out;
    }

    /** A list whose elements may be null ({@code List<@Nullable X>} in the record's source). */
    static <T> List<@Nullable T> nullableList(Cursor r, Function<Cursor, T> read) {
        openArray(r);
        ArrayList<@Nullable T> out = new ArrayList<>(0);
        if (!r.take(']')) {
            do {
                out.add(r.take("null") ? null : read.apply(r));
            } while (r.take(','));
            closeArray(r);
            out.trimToSize();
        } else {
            r.leave();
        }
        return out;
    }

    /** A string-keyed map, in the order the codec wrote it (the records' maps are insertion-ordered). */
    static <V> LinkedHashMap<String, V> map(Cursor r, Function<Cursor, V> read) {
        if (!r.take('{')) {
            throw r.refusal("a map as an object");
        }
        r.enter();
        LinkedHashMap<String, V> out = new LinkedHashMap<>();
        if (r.take('}')) {
            r.leave();
            return out;
        }
        do {
            String key = string(r);
            if (!r.take(':')) {
                throw r.refusal("a map key's ':'");
            }
            out.put(key, read.apply(r));
        } while (r.take(','));
        if (!r.take('}')) {
            throw r.refusal("a map's ',' or '}'");
        }
        r.leave();
        return out;
    }

    /** A map entry: the codec's {@code [key, value]} pair. */
    static <K, V> Map.Entry<K, V> entry(Cursor r, Function<Cursor, K> key, Function<Cursor, V> value) {
        if (!r.take('[')) {
            throw r.refusal("a map entry as a pair");
        }
        r.enter();
        K k = key.apply(r);
        if (!r.take(',')) {
            throw r.refusal("a map entry as a pair");
        }
        V v = value.apply(r);
        if (!r.take(']')) {
            throw r.refusal("a map entry as a pair");
        }
        r.leave();
        return Map.entry(k, v);
    }

    private static void openArray(Cursor r) {
        if (!r.take('[')) {
            throw r.refusal("a list as an array");
        }
        r.enter();
    }

    private static void closeArray(Cursor r) {
        if (!r.take(']')) {
            throw r.refusal("a list's ',' or ']'");
        }
        r.leave();
    }
}
