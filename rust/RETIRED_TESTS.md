# Test sources the port changed

Iteration 1 runs the existing tests untouched against Rust (docs/rust-port/ROADMAP.md, "Untouched"). Every change to a
test source -- a test retired with the package it tested, a pinned assertion updated, a guard's ledger moved -- is a
row here, dated, with its reason. A change not listed here is a defect.

| date | test source | change | why |
|---|---|---|---|
| 2026-11-02 | `core/src/test/java/com/legend/ErrorShapeGuardrailTest.java` | `BROAD_CATCH_COUNTS` gains `Lite.java` at 1 (0 -> 1), with its dated comment | the shim's one downcall site, `Lite.guarded`: `MethodHandle.invokeExact` declares `Throwable`. The catch-free alternative (`MethodHandleProxies`) cannot be made inside libcompiler's native image -- measured, "no method in" every proxied interface -- so the warehouse's `invokeExact` shape it is (docs/rust-port/PHASE1.md §1) |
| 2026-11-03 | `tools/guards/CompileOnlyTest.java` | `SIGNATURE` and `SKIPS` no longer name the `wasm` tier (each with its dated comment); `ALLOWED`'s `wasm` entry kept | `//:wasm`'s two modules (`//wasm:planner`, `//sdlc-server:page`) are frozen files TeaVM last built (docs/rust-port/PHASE1.md §5): the tier registers no action, so it can show no TeaVM compile and its walk reaches no javac plugin. The Rust wasm32 build brings both back |
