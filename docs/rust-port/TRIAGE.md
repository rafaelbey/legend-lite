# Rust port — triage (2026-11-02)

What of legend-lite's Java ports to Rust **verbatim**, what ports **with considerations**, and what needs a
**redesign**. Measured on `rust-port` at `cf73e6ef7` (upstream `main`). The roadmap that acts on it is
[`ROADMAP.md`](ROADMAP.md).

## Why

`libcompiler.{so,dylib}` — the compiler as a native library, Python's engine — is built by GraalVM native-image
(`native/BUILD.bazel:54`), and the browser planner by TeaVM (`wasm/`). The goal is to drop Java and GraalVM as the
means of producing the binary by porting to Rust.

## Decisions

| Topic | Decision |
|---|---|
| Scope | Core compiler first. warehouse, sdlc-server, depot-server, query-store are later waves |
| Integration | A Java **FFI shim** (`java.lang.foreign`; the tests run on JDK 25, `.bazelrc:6`) over a Rust `cdylib` |
| Replacement | **Hard replace**: Java bodies are deleted as each Rust slice lands |
| Trust signal | The existing tests run **untouched** against Rust through the shim |
| 500 error answers | Rust-native error kinds; the 8 assertions that pin Java class names are updated |
| Query translators | Postgres-SQL→Pure in iteration 1; SAP IQ and Cypher later |
| Browser (TeaVM) | The TeaVM planner is **deleted** this iteration and comes back as Rust `wasm32` |
| Tests of deleted internals | **Retired with a ledger** entry (dated, per test class) |
| libcompiler's other entries | `lite_warehouse_*`, `lite_sdlc_handle`, `lite_depot_handle`, `lite_saved_*` are **dropped** for now |

## The rule

A Java behaviour is kept exactly **only where it is upstream spec**. legend-engine is itself Java, so its SQL
literals and protocol JSON carry Java's spellings and the corpora compare against them. Behaviour that merely leaked
out of the JVM is dropped.

## Verbatim — near-mechanical translation

| Package | LOC | Note |
|---|---|---|
| `model` | 8.1K | 170 records, 16 sealed interfaces → enums; 2 identity sites → indexes |
| `compiler` top level (`NameResolver`, `SymbolTable`, `ModelBuilder`) | 5.3K | string-keyed indexes |
| `sql` MIR records | 6.2K | sealed records → enums (identity: D1) |
| `sql/dialect` render **bodies** | ~44K | mechanical once the sharing redesign (D3) is in place |
| `executionplan`, `plan`, core `planner`, `platform`, `ide`, `setup`, `lineage`, `validation`, `error`, `door`, `diagnostics`, `database` | ~16K | records and printers; number spellings pinned (B1) |
| protocol records (`Protocol.java`, 297 records) | part of 27K | serde-shaped data; the emitter stays hand-written |
| `planner.Boundary` + `Folded` (`wasm/src/main/java/planner/`) | 0.7K | becomes the C-ABI layer |
| `server/PureV1Api` (the routes `Boundary.pureV1` reaches) | 1.1K | 15 routes, string in and out |

## With considerations — translatable, but a Java idiom needs a deliberate Rust choice

| Package | LOC | Consideration |
|---|---|---|
| `lexer` | 1.4K | UTF-16 vs code-point columns; Java's `Character.isXxx` classes (C3) |
| `parser` | 24.9K | UTF-16 offsets (545 `charAt`, 371 `substring`), lone surrogates, catch-as-backtrack (`SpecParser:2541`), `ServiceLoader` grammars |
| `normalizer` | 11.4K | identity maps (`LiftedViews:50-57`), 7 `HashMap` iterations |
| `compiler/element` | 6.9K | memo caches filled on lookup and shared across threads (D5) |
| `builtin` | 7.2K | 891 signatures and the 7.3K-line prelude parsed at class load → retarget the `spec/src/gen` generators to emit Rust |
| protocol emitter/reader + `spec/` | ~27K | ~100 hand-coded engine quirks; number spelling (B1) |
| `protocol/sql` (Postgres-SQL→Pure) | 14K | `LocalDate` arithmetic, a 38-default-method strategy interface, the checked `Bad` exception |
| `json` | 1K | ordered objects and raw number tokens; a custom writer (`serde_json` can parse only) |
| `values` | 1K | Pure years ±999,999,999, beyond `chrono` |
| `lowering` | 26.5K | closures over a mutable Lowerer (a borrow conflict → `Cell`), `static {}` rule tables → `LazyLock` |
| `result` + `arrow_ipc` | 3.4K | `CellText`'s `Double.toString`; the Arrow/zstd framer ports verbatim (no `arrow` crate: the `ArrowClosureTest` ruling) |
| `cache`, `queries`, top-level `Compiler`/`TypedQuery`/`HeldModels` | 3.4K | `synchronized` and single-flight stores → `Send + Sync`; static boot → `OnceLock` |

## Redesign

| Area | LOC | Why |
|---|---|---|
| D1 node identity (cross-cutting) | — | Identity is *semantic*: `TypedUserCall.equals` compares `origin ==`; about 300 identity-map sites (compiler 19, resolver 27, sql 23, lowering 6, normalizer 5). Nodes are re-wrapped (`withInfo`/`withOrigin`), so `Rc::ptr_eq` is not enough → **a NodeId stamped at construction**; identity maps become `HashMap<NodeId, _>` |
| D2 `compiler/spec` | 32K | D1, plus overloads chosen by try/catch with a typer snapshot rollback (`Overloads:693-701`, `Typer:118`) → a `Result` typer with explicit checkpoints |
| D3 dialect sharing | 44K | 16 classes under `AnsiSqlRenderer` (154 hooks, ~855 overrides, 153 `super.` calls) → a trait with default methods and free functions for `super`; MongoDb a separate implementation |
| D4 `resolver` | 36.7K | setter-injected services in a cycle (`StoreResolver:128-152`), shared mutable `letBindings`, probes by exception → one owning context struct |
| D5 shared model context | — | decide `Arc` + interior mutability once, before any code |
| D6 `spi` / `ServiceLoader` | — | an explicit registry (including the test grammars in `core/src/test/resources/META-INF/services`) |
| D7 the native adapter | 0.6K | GraalVM's isolate ABI (`graal_create_isolate`, `graal_attach_thread`) goes → a plain C ABI; `python/legend_lite/_library.py` changes |

## Not in iteration 1

`exec` and the drivers (H2 has **no Rust driver**), the `server` HTTP shell, the `driver` (12.4K) and `judge` (4.1K)
test runners, `testdatagen`, `probe` (already scheduled for deletion), `protocol/sql/iq` (27K), `protocol/cypher`
(24.7K), warehouse, sdlc-server, depot-server, query-store, and every TeaVM page.

## Edge cases — Java semantics the port must handle

**A. The error surface (JVM leaks: dropped)**

1. 500 answers spell the Java class name (`PureV1Api.named`, `server/PureV1Api.java:1122`) → Rust kinds. The pins
   updated: `PureV1ApiTest` (3), `python/tests/test_compiler.py` (1), `studio/test/engine.test.ts` (2),
   `datacube/test/saved-pages/conformance.ts` (2).
2. About 1,300 `throw`s used as control flow → `Result` and `?`; `catch_unwind` at every FFI export, as a safety
   net only.
3. `Folded` catches `StackOverflowError` → an explicit recursion-depth limit and a big-stack worker thread.
4. Five `catch (RuntimeException)` sites swallow bugs (`Overloads:1332`, `StaticFold:474`, `TdsChecker:275`,
   `FunctionCompiler:77`) → audited, not replicated.

**B. Spelling that is upstream spec (kept exactly)**

1. `Double.toString` (`1.0`, `1.0E10`) in SQL, plan JSON, protocol JSON and cell text (`AnsiSqlRenderer:2087`,
   `MirJson:319`, `ProtocolEmitter:2258`, `CellText:274`, …).
2. `BigDecimal`: scale-sensitive equality used as hash keys, exponent `toString`, 38 digits → the `bigdecimal` crate
   with custom `Eq`/`Hash`/`Display`.
3. java.time: `ZoneId` `SHORT_IDS` (`LiteralSpelling:529`, `EngineStyleH2:1027`), a pinned tzdata,
   `LocalDateTime.toString`.
4. Protocol JSON columns are UTF-16 units, error columns code points (`TokenStream:219` vs `:249`).
5. Insertion-ordered maps → `indexmap`; string sorts in UTF-16 order.

**C. Java library semantics that leak in by accident**

1. Java's `HashMap` iteration order is deterministic and Rust's is random (~63 sites) → `IndexMap` everywhere; the
   corpus flags any site where Java's bucket order reached pinned output (e.g. `ConnectionSectionGrammar:804`).
2. Lone surrogates from `\uXXXX` escapes (`parser/Escapes:59,80`, `json/Json:775,929`).
3. `Character.isLetterOrDigit/isWhitespace/isDigit` are not Rust's `char` classes; regex `\b` is ASCII in Java and
   Unicode in Rust.
4. `String.split` drops trailing empty strings (28 sites); 3 regexes are beyond the `regex` crate (`Postgres:220`
   lookahead, `SavedQueries:48` possessive, `AgreementArm:130` `\R`).
5. `CommonSubexpressions:93` breaks ties in identity-hash order, so Java's own output is already nondeterministic
   there (SQLite, BigQuery) → a deterministic rule.
6. Fresh-name counters on dialect objects are never reset per render (`SQLite` `binds`, `Snowflake` `fresh`,
   `CommonSubexpressions` `fresh`), so Java's SQL depends on what the dialect rendered before → the Rust renderer
   resets them per render ([`PHASE0.md`](PHASE0.md) §4: 28 of 5.6M renders).

## Sources

Measured by three read-only surveys of the tree (front end and protocol; middle and back end; runtime, hosts and
tests), with the counts and paths spot-checked against the code. Line numbers are as of `cf73e6ef7`.
