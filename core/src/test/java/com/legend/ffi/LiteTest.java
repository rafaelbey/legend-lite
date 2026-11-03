// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

package com.legend.ffi;

import com.legend.json.Json;
import com.legend.testing.Runfile;
import org.junit.jupiter.api.Test;

import java.lang.foreign.FunctionDescriptor;
import java.lang.foreign.MemoryLayout;
import java.lang.foreign.ValueLayout;
import java.lang.reflect.Field;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.List;
import java.util.Set;
import java.util.TreeSet;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertNull;
import static org.junit.jupiter.api.Assertions.assertThrows;

/**
 * The shim's contract with the Rust library (docs/rust-port/ROADMAP.md, phase 0 step 6), against the library Bazel
 * builds (//rust/lite-ffi:lite_ffi): text through it and back, refusals with their kind, phase and position, a panic
 * answered rather than a crash, and every answer freed.
 */
class LiteTest {

    private static final Lite LITE = Lite.load(library());

    /** The shared library among the target's outputs (on Windows its import library comes with it). */
    private static Path library() {
        List<Path> outputs = Runfile.envList("LEGEND_FFI_LIBRARY");
        return outputs.stream()
                .filter(p -> p.toString().endsWith(".dylib") || p.toString().endsWith(".so")
                        || p.toString().endsWith(".dll"))
                .findFirst()
                .orElseThrow(() -> new IllegalStateException("no shared library among " + outputs));
    }

    @Test
    void answersItsVersion() {
        assertEquals("0.1.0", LITE.version());
    }

    @Test
    void textGoesThroughTheLibraryAndBack() {
        for (String text : List.of("", "plain", "two\nlines\n", "héllo, wörld", "an astral 😀 smile",
                "x".repeat(1 << 20))) {
            assertEquals(text, LITE.echo(text));
        }
    }

    @Test
    void aRefusalCarriesKindPhasePositionAndMessage() {
        LiteRefusal r = assertThrows(LiteRefusal.class,
                () -> LITE.probeRefusal("COMPILATION", "TYPE", 2, 5, "no such class\non line two"));
        assertEquals("COMPILATION", r.kind());
        assertEquals("TYPE", r.phase());
        assertEquals(2, r.line());
        assertEquals(5, r.column());
        assertEquals("no such class\non line two", r.getMessage());
    }

    @Test
    void aRefusalWithoutPhaseOrPosition() {
        LiteRefusal r = assertThrows(LiteRefusal.class, () -> LITE.probeRefusal("PARSER", "", 0, 0, "unexpected"));
        assertEquals("PARSER", r.kind());
        assertNull(r.phase());
        assertEquals(0, r.line());
        assertEquals(0, r.column());
        assertEquals("unexpected", r.getMessage());
    }

    @Test
    void aPanicIsAnInternalRefusalNotACrash() {
        LiteRefusal r = assertThrows(LiteRefusal.class, () -> LITE.probePanic("on purpose"));
        assertEquals("INTERNAL", r.kind());
        assertEquals("on purpose", r.getMessage());
    }

    @Test
    void textHoldingNulIsRefusedBeforeTheCall() {
        assertThrows(IllegalArgumentException.class, () -> LITE.echo("a\0b"));
    }

    /** TRIAGE C2: a lone surrogate has no UTF-8; the JVM's encoder sends it as '?'. Pinned until the port decides. */
    @Test
    void aLoneSurrogateReachesTheLibraryAsAQuestionMark() {
        assertEquals("a?b", LITE.echo("a\uD800b"));
    }

    @Test
    void everyAnswerIsFreed() {
        long before = LITE.unfreed();
        LITE.version();
        LITE.echo("x");
        assertThrows(LiteRefusal.class, () -> LITE.probeRefusal("PARSER", "", 0, 0, "m"));
        assertThrows(LiteRefusal.class, () -> LITE.probePanic("p"));
        assertEquals(before, LITE.unfreed());
        assertEquals(0, LITE.unfreed());
    }

    /**
     * libcompiler's image declares every downcall shape this class makes (docs/rust-port/PHASE1.md §1): a shape
     * missing there fails the image's first call through it, not its build.
     */
    @Test
    void theNativeImageDeclaresEveryDowncall() throws Exception {
        Set<String> made = new TreeSet<>();
        for (Field f : Lite.class.getDeclaredFields()) {
            if (f.getType() == FunctionDescriptor.class) {
                FunctionDescriptor d = (FunctionDescriptor) f.get(null);
                made.add(d.returnLayout().map(LiteTest::metadataType).orElse("void")
                        + d.argumentLayouts().stream().map(LiteTest::metadataType).toList());
            }
        }
        Json.Obj metadata = (Json.Obj) Json.parse(Files.readString(Runfile.property("native.metadata")));
        Set<String> declared = new TreeSet<>();
        for (Json.Node n : metadata.getObj("foreign").getArr("downcalls").items()) {
            Json.Obj downcall = (Json.Obj) n;
            declared.add(downcall.getString("returnType") + downcall.getArr("parameterTypes").items().stream()
                    .map(p -> ((Json.Str) p).value()).toList());
        }
        assertEquals(made, declared, "native/src/main/resources/.../reachability-metadata.json's foreign downcalls");
    }

    /** A layout as the image's metadata names it. */
    private static String metadataType(MemoryLayout layout) {
        if (layout.equals(ValueLayout.ADDRESS)) {
            return "void*";
        }
        if (layout.equals(ValueLayout.JAVA_INT)) {
            return "jint";
        }
        if (layout.equals(ValueLayout.JAVA_LONG)) {
            return "jlong";
        }
        throw new IllegalStateException("a layout the metadata check does not name yet: " + layout);
    }
}
