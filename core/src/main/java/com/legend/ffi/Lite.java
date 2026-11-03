// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

package com.legend.ffi;

import java.lang.foreign.Arena;
import java.lang.foreign.FunctionDescriptor;
import java.lang.foreign.Linker;
import java.lang.foreign.MemorySegment;
import java.lang.foreign.SymbolLookup;
import java.lang.invoke.MethodHandleProxies;
import java.nio.file.Path;

import static java.lang.foreign.ValueLayout.ADDRESS;
import static java.lang.foreign.ValueLayout.JAVA_INT;
import static java.lang.foreign.ValueLayout.JAVA_LONG;

/**
 * The Rust compiler library from the JVM (docs/rust-port/ROADMAP.md, "The shim"): rust/lite-ffi's exports called
 * through java.lang.foreign, as the warehouse calls DuckDB's (warehouse/.../server/duck/Duck.java). An argument goes
 * as UTF-8 text; an answer comes back as UTF-8 text the library allocated, read and freed at once:
 * {@code OK\n<answer>} is returned as its answer, {@code ERR\n<kind>\n<phase>\n<line:col>\n<message>} thrown as a
 * {@link LiteRefusal}. Phase 0 binds the contract alone -- the version, the echo and the two probes; each later phase
 * moves core's entry points onto it.
 *
 * <p>Each export is a typed interface over its downcall ({@link MethodHandleProxies}), so a call is an ordinary Java
 * call that declares no {@code Throwable}. The JVM must allow native access ({@code --enable-native-access=ALL-UNNAMED}).
 */
public final class Lite {

    /** An export taking nothing, answering text. */
    public interface Nullary {
        MemorySegment call();
    }

    /** An export taking one text, answering text. */
    public interface Unary {
        MemorySegment call(MemorySegment text);
    }

    /** {@code lite_probe_refusal}'s shape. */
    public interface RefusalProbe {
        MemorySegment call(MemorySegment kind, MemorySegment phase, int line, int column, MemorySegment message);
    }

    /** {@code lite_free}'s shape. */
    public interface Free {
        void call(MemorySegment answer);
    }

    /** {@code lite_unfreed}'s shape. */
    public interface Count {
        long call();
    }

    private static final Linker LINKER = Linker.nativeLinker();

    private final Nullary version;
    private final Unary echo;
    private final RefusalProbe probeRefusal;
    private final Unary probePanic;
    private final Free free;
    private final Count unfreed;

    private Lite(SymbolLookup library) {
        version = export(library, Nullary.class, "lite_version", FunctionDescriptor.of(ADDRESS));
        echo = export(library, Unary.class, "lite_echo", FunctionDescriptor.of(ADDRESS, ADDRESS));
        probeRefusal = export(library, RefusalProbe.class, "lite_probe_refusal",
                FunctionDescriptor.of(ADDRESS, ADDRESS, ADDRESS, JAVA_INT, JAVA_INT, ADDRESS));
        probePanic = export(library, Unary.class, "lite_probe_panic", FunctionDescriptor.of(ADDRESS, ADDRESS));
        free = export(library, Free.class, "lite_free", FunctionDescriptor.ofVoid(ADDRESS));
        unfreed = export(library, Count.class, "lite_unfreed", FunctionDescriptor.of(JAVA_LONG));
    }

    /** The library at {@code path}, loaded for the life of the process. */
    public static Lite load(Path path) {
        return new Lite(SymbolLookup.libraryLookup(path, Arena.global()));
    }

    private static <T> T export(SymbolLookup library, Class<T> shape, String name, FunctionDescriptor descriptor) {
        MemorySegment symbol = library.find(name)
                .orElseThrow(() -> new IllegalStateException("the Rust library has no " + name));
        return MethodHandleProxies.asInterfaceInstance(shape, LINKER.downcallHandle(symbol, descriptor));
    }

    /** The library's version. */
    public String version() {
        return answer(take(version.call()));
    }

    /** {@code text}, through the library and back. */
    public String echo(String text) {
        try (Arena arena = Arena.ofConfined()) {
            return answer(take(echo.call(utf8(arena, text))));
        }
    }

    /** Phase 0's probe of the refusal path: always throws the {@link LiteRefusal} it describes. */
    public String probeRefusal(String kind, String phase, int line, int column, String message) {
        try (Arena arena = Arena.ofConfined()) {
            return answer(take(probeRefusal.call(utf8(arena, kind), utf8(arena, phase), line, column,
                    utf8(arena, message))));
        }
    }

    /** Phase 0's probe of the panic path: the library panics, and answers it as an {@code INTERNAL} refusal. */
    public String probePanic(String message) {
        try (Arena arena = Arena.ofConfined()) {
            return answer(take(probePanic.call(utf8(arena, message))));
        }
    }

    /** The answers the library handed out and were not yet freed: 0 between calls. */
    public long unfreed() {
        return unfreed.call();
    }

    /** {@code text} as a NUL-terminated UTF-8 string. Text holding a NUL cannot be one, and is refused. */
    private static MemorySegment utf8(Arena arena, String text) {
        if (text.indexOf('\0') >= 0) {
            throw new IllegalArgumentException("text for the Rust library holds a NUL character");
        }
        return arena.allocateFrom(text);
    }

    /** An answer's text, the answer freed. */
    private String take(MemorySegment answer) {
        if (answer.equals(MemorySegment.NULL)) {
            throw new IllegalStateException("the Rust library answered null");
        }
        String text = answer.reinterpret(Long.MAX_VALUE).getString(0);
        free.call(answer);
        return text;
    }

    /** A folded answer's text, or its refusal thrown. */
    static String answer(String folded) {
        if (folded.startsWith("OK\n")) {
            return folded.substring(3);
        }
        String[] parts = folded.split("\n", 5);
        if (!parts[0].equals("ERR") || parts.length < 5) {
            throw new IllegalStateException("not an answer of the Rust library: " + folded);
        }
        int line = 0;
        int column = 0;
        if (!parts[3].equals("-")) {
            int colon = parts[3].indexOf(':');
            line = Integer.parseInt(parts[3].substring(0, colon));
            column = Integer.parseInt(parts[3].substring(colon + 1));
        }
        throw new LiteRefusal(parts[1], parts[2].equals("-") ? null : parts[2], line, column, parts[4]);
    }
}
