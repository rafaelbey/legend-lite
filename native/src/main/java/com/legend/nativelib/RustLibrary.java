package com.legend.nativelib;

import com.legend.ffi.Lite;

import java.nio.file.Files;
import java.nio.file.Path;

import org.graalvm.nativeimage.IsolateThread;
import org.graalvm.nativeimage.ProcessProperties;
import org.graalvm.nativeimage.c.function.CEntryPointLiteral;
import org.graalvm.nativeimage.c.function.CFunctionPointer;

/**
 * Where this library finds the Rust compiler library it calls (docs/rust-port/PHASE1.md §1): beside itself. A package
 * ships the two in one folder (the wheel's {@code _native/}), and the host that loads this library names this one
 * alone, so the Rust library's path is this library's own folder -- asked of GraalVM through the address of one of
 * this library's entry points, which names its file whatever the executable is (Python, mostly). Bazel's runs keep
 * them in different folders and find the Rust library in their runfiles ({@code Lite.locate}); an explicit
 * {@code -Dlegend.lite.library} is never overridden.
 *
 * <p>Initialized when the image is built ({@code --initialize-at-build-time}, //native:compiler): an entry point's
 * literal can only be made then.
 */
final class RustLibrary {

    private static final CEntryPointLiteral<CFunctionPointer> SELF =
            CEntryPointLiteral.create(Compiler.class, "unfreed", IsolateThread.class);

    private RustLibrary() {
    }

    /** Names the Rust library beside this one as the one to load, when it is there and nothing else is named. */
    static void besideThisLibrary() {
        if (System.getProperty("legend.lite.library") != null) {
            return;
        }
        String self = ProcessProperties.getObjectFile(SELF);
        if (self == null) {
            return;
        }
        Path beside = Path.of(self).resolveSibling(Lite.fileName());
        if (Files.isRegularFile(beside)) {
            System.setProperty("legend.lite.library", beside.toString());
        }
    }
}
