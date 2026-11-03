# Test sources the port changed

Iteration 1 runs the existing tests untouched against Rust (docs/rust-port/ROADMAP.md, "Untouched"). Every change to a
test source -- a test retired with the package it tested, a pinned assertion updated, a guard's ledger moved -- is a
row here, dated, with its reason. A change not listed here is a defect.

| date | test source | change | why |
|---|---|---|---|
| 2026-11-02 | `core/src/test/java/com/legend/ErrorShapeGuardrailTest.java` | `BROAD_CATCH_COUNTS` gains `Lite.java` at 1 (0 -> 1), with its dated comment | the shim's one downcall site, `Lite.guarded`: `MethodHandle.invokeExact` declares `Throwable`. The catch-free alternative (`MethodHandleProxies`) cannot be made inside libcompiler's native image -- measured, "no method in" every proxied interface -- so the warehouse's `invokeExact` shape it is (docs/rust-port/PHASE1.md §1) |
