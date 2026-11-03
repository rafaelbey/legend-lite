package com.legend.nativelib;

import java.nio.file.Path;
import java.util.HashMap;
import java.util.Map;

import org.graalvm.nativeimage.IsolateThread;
import org.graalvm.nativeimage.c.function.CEntryPoint;
import org.graalvm.nativeimage.c.type.CCharPointer;

import com.legend.depot.Depot;
import com.legend.sdlc.Sdlc;
import com.legend.sdlc.grammar.CoreGrammar;
import com.legend.sdlc.server.GitStorage;

import planner.Folded;

/**
 * PYTHON'S MODEL HOME (docs/ONE_PLATFORM_PROGRAM_2026_10_12.md B3): the SDLC's and the Depot's rules -- the very classes
 * the page runs compiled to WebAssembly ({@code SdlcPage}) and the model home's server runs on the JVM
 * ({@code SdlcServer}) -- over a git repository in this user's folder ({@link GitStorage}: the server's keys on disk, so
 * {@code git --git-dir} reads it and {@code git push} sends it anywhere). The third build of one SDLC, held to the same
 * conformance suite ({@code sdlc-client/test/conformance.ts}).
 *
 * <p>One {@link Sdlc} and one {@link Depot} per repository, made the first time it is asked of, its user this machine's
 * own ({@code local}, "Local User": the page's, as the browser's in-page SDLC names it; there is no sign-in). Each call
 * is one request under its API root -- {@code target} the path after {@code /sdlc/api} or {@code /depot/api} with its
 * query -- answered {@code "OK\n<status>\n<body>"} (the body empty for none), or a failure as the library answers one
 * ({@link Folded}). Each call is made under the repository's own lock ({@link GitStorage#locked}: this process's threads
 * and every other process that opens it, the model home's server among them; docs/ONE_PLATFORM_PROGRAM_2026_10_12.md
 * G11): the rules are single-threaded, and {@link GitStorage} keeps nothing in memory between calls.
 */
final class ModelHome {
    private static final Map<String, GitStorage> REPOSITORIES = new HashMap<>();
    private static final Map<String, Sdlc> SDLCS = new HashMap<>();
    private static final Map<String, Depot> DEPOTS = new HashMap<>();

    // as Compiler's: the SDLC's grammar (//sdlc-server/grammar) reaches the planner and its dialects, which call the
    // Rust library, and a host may call here first
    static {
        RustLibrary.besideThisLibrary();
    }

    private ModelHome() {
    }

    /** The repository in {@code folder}, made the first time it is asked of. */
    private static synchronized GitStorage repository(String folder) {
        return REPOSITORIES.computeIfAbsent(folder, f -> new GitStorage(Path.of(f)));
    }

    /** The repository's SDLC, made the first time (the repository made too, when it is not yet). */
    private static synchronized Sdlc sdlc(String folder) {
        return SDLCS.computeIfAbsent(folder, f -> new Sdlc(repository(f), "local", "Local User",
                new CoreGrammar(), System::currentTimeMillis).backendType("git"));
    }

    /** Depot-lite over the repository's own versions, as the server mounts it beside its SDLC. */
    private static synchronized Depot depot(String folder) {
        Sdlc sdlc = sdlc(folder);
        return DEPOTS.computeIfAbsent(folder, f -> new Depot(sdlc.artifacts(), System::currentTimeMillis));
    }

    /** One SDLC request ({@code Sdlc.handle}): {@code body} the request's text, empty for none. */
    @CEntryPoint(name = "lite_sdlc_handle")
    static CCharPointer sdlcHandle(IsolateThread thread, CCharPointer folder, CCharPointer method, CCharPointer target,
            CCharPointer body) {
        String a;
        try {
            String f = Compiler.text(folder), m = Compiler.text(method), t = Compiler.text(target), b = Compiler.text(body);
            a = Folded.of(() -> {
                Sdlc.Response r = repository(f).locked(() -> sdlc(f).handle(m, t, b.isEmpty() ? null : b));
                return r.status() + "\n" + (r.body() == null ? "" : r.body());
            });
        } catch (Throwable failure) {
            a = Folded.failure(failure);
        }
        return Compiler.answer(a);
    }

    /** One Depot request ({@code Depot.handle}), answered as {@link #sdlcHandle}'s. */
    @CEntryPoint(name = "lite_depot_handle")
    static CCharPointer depotHandle(IsolateThread thread, CCharPointer folder, CCharPointer method, CCharPointer target,
            CCharPointer body) {
        String a;
        try {
            String f = Compiler.text(folder), m = Compiler.text(method), t = Compiler.text(target), b = Compiler.text(body);
            a = Folded.of(() -> {
                Depot.Response r = repository(f).locked(() -> depot(f).handle(m, t, b.isEmpty() ? null : b));
                return r.status() + "\n" + (r.body() == null ? "" : r.body());
            });
        } catch (Throwable failure) {
            a = Folded.failure(failure);
        }
        return Compiler.answer(a);
    }
}
