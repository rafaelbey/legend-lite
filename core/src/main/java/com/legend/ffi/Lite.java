// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

package com.legend.ffi;

import java.lang.foreign.Arena;
import java.lang.foreign.FunctionDescriptor;
import java.lang.foreign.Linker;
import java.lang.foreign.MemorySegment;
import java.lang.foreign.SymbolLookup;
import java.lang.invoke.MethodHandle;
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
 * <p>The handles are called with {@code invokeExact}, which runs in a GraalVM native image too: libcompiler calls this
 * class from inside its image (//native:compiler), where a {@code MethodHandleProxies} interface cannot be made
 * (measured 2026-11-02: "no method in" every proxied interface). {@code invokeExact} declares {@code Throwable}, so
 * every call goes through {@link #guarded}, the one place it is caught. The JVM, and the image, must allow native
 * access ({@code --enable-native-access=ALL-UNNAMED}); an image also declares each descriptor below as a downcall
 * (native/src/main/resources/META-INF/native-image/com.legend/native/reachability-metadata.json).
 */
public final class Lite {

    private static final Linker LINKER = Linker.nativeLinker();

    static final FunctionDescriptor NULLARY = FunctionDescriptor.of(ADDRESS);
    static final FunctionDescriptor UNARY = FunctionDescriptor.of(ADDRESS, ADDRESS);
    static final FunctionDescriptor REFUSAL_PROBE =
            FunctionDescriptor.of(ADDRESS, ADDRESS, ADDRESS, JAVA_INT, JAVA_INT, ADDRESS);
    static final FunctionDescriptor FREE = FunctionDescriptor.ofVoid(ADDRESS);
    static final FunctionDescriptor COUNT = FunctionDescriptor.of(JAVA_LONG);

    private final MethodHandle version;
    private final MethodHandle echo;
    private final MethodHandle probeRefusal;
    private final MethodHandle probePanic;
    private final MethodHandle free;
    private final MethodHandle unfreed;

    private Lite(SymbolLookup library) {
        version = downcall(library, "lite_version", NULLARY);
        echo = downcall(library, "lite_echo", UNARY);
        probeRefusal = downcall(library, "lite_probe_refusal", REFUSAL_PROBE);
        probePanic = downcall(library, "lite_probe_panic", UNARY);
        free = downcall(library, "lite_free", FREE);
        unfreed = downcall(library, "lite_unfreed", COUNT);
    }

    /** The library at {@code path}, loaded for the life of the process. */
    public static Lite load(Path path) {
        return new Lite(SymbolLookup.libraryLookup(path, Arena.global()));
    }

    private static MethodHandle downcall(SymbolLookup library, String name, FunctionDescriptor descriptor) {
        MemorySegment symbol = library.find(name)
                .orElseThrow(() -> new IllegalStateException("the Rust library has no " + name));
        return LINKER.downcallHandle(symbol, descriptor);
    }

    /** The library's version. */
    public String version() {
        return answer(take(guarded(() -> (MemorySegment) version.invokeExact())));
    }

    /** {@code text}, through the library and back. */
    public String echo(String text) {
        try (Arena arena = Arena.ofConfined()) {
            MemorySegment t = utf8(arena, text);
            return answer(take(guarded(() -> (MemorySegment) echo.invokeExact(t))));
        }
    }

    /** Phase 0's probe of the refusal path: always throws the {@link LiteRefusal} it describes. */
    public String probeRefusal(String kind, String phase, int line, int column, String message) {
        try (Arena arena = Arena.ofConfined()) {
            MemorySegment k = utf8(arena, kind);
            MemorySegment p = utf8(arena, phase);
            MemorySegment m = utf8(arena, message);
            return answer(take(guarded(() -> (MemorySegment) probeRefusal.invokeExact(k, p, line, column, m))));
        }
    }

    /** Phase 0's probe of the panic path: the library panics, and answers it as an {@code INTERNAL} refusal. */
    public String probePanic(String message) {
        try (Arena arena = Arena.ofConfined()) {
            MemorySegment m = utf8(arena, message);
            return answer(take(guarded(() -> (MemorySegment) probePanic.invokeExact(m))));
        }
    }

    /** The answers the library handed out and were not yet freed: 0 between calls. */
    public long unfreed() {
        return guarded(() -> (long) unfreed.invokeExact());
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
        // a void downcall through the one guarded site; the value it hands back is unused
        guarded(() -> {
            free.invokeExact(answer);
            return answer;
        });
        return text;
    }

    /** A downcall: {@code invokeExact}, which declares {@code Throwable}. */
    @FunctionalInterface
    private interface Downcall<T> {
        T call() throws Throwable;
    }

    /**
     * Runs a downcall. The one broad catch of this class (ErrorShapeGuardrailTest's pin): {@code invokeExact} declares
     * {@code Throwable}, and a downcall raises none of its own -- a RuntimeException or an Error (a link failure, the
     * heap) is rethrown unchanged, anything else wrapped; never swallowed, never a value.
     */
    private static <T> T guarded(Downcall<T> downcall) {
        try {
            return downcall.call();
        } catch (Throwable t) {
            if (t instanceof RuntimeException r) {
                throw r;
            }
            if (t instanceof Error e) {
                throw e;
            }
            throw new IllegalStateException("calling the Rust library", t);
        }
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
