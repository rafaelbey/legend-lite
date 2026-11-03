# Rust port — phase 1: the dialects (2026-11-02)

Phase 1 of [`ROADMAP.md`](ROADMAP.md): `sql/dialect` -- 44,195 lines, 16 dialects, the rewrite passes -- moves to Rust
and its Java is deleted. This file holds what was measured before any of it moved, and the design that follows.

## 1. libcompiler can call Rust from inside its image

`//native:compiler` compiles core with GraalVM, so once a dialect is a Rust call, libcompiler makes that call from
inside a native image. Measured with a throwaway entry point (`lite_rust_echo`) that called `com.legend.ffi.Lite` from
the image, through Python's `ctypes` exactly as the bindings load libcompiler:

- **`MethodHandleProxies` does not survive the image**: every call answered `IllegalArgumentException: no method in :
  com.legend.ffi.Lite$Nullary` -- the proxy's interface is resolved by runtime reflection the image does not carry.
- **`invokeExact` does**: `OK\nhéllo from inside the image`, multi-line and empty text, all through the Bazel-built
  `liblite_ffi.dylib`. The shim is now that shape (one guarded downcall site, pinned in `ErrorShapeGuardrailTest`;
  `rust/RETIRED_TESTS.md`).

The recipe, reverted after the measurement and re-applied when DuckDB switched (§4 step 3, 2026-11-03):
`--enable-native-access=ALL-UNNAMED` in the image's `extra_args`; `//core:ffi` among `//native:entry`'s deps; every
descriptor of `Lite` declared under `"foreign": {"downcalls": [...]}` in
`native/src/main/resources/META-INF/native-image/com.legend/native/reachability-metadata.json` (`void*` for an address,
`jint`, `jlong`, `void`); and `liblite_ffi` shipped beside libcompiler, in the Python wheel too. The descriptors and the
metadata must stay one list: a check that the JSON names every `Lite` descriptor comes with the switch.

## 2. What of `sql/dialect` stays Java

The package's types, by who uses them outside it:

| type | outside users | in phase 1 |
|---|---|---|
| `SqlDialect` | 62 references: planner, setup, driver, server, testdatagen, judge, `Compiler`, `Execution` | **stays**: the shim. A Rust-backed implementation behind `Databases.dialect(type)` |
| `RenderedStatement` (+ `Bind`, `Region`) | `planner/PlanMaker` | **stays**: a record the shim builds from Rust's answer |
| `DialectCapability` | `server/PureV1Api`, `driver/DatabaseJudge`, `judge/VerdictBatch` | **stays**: the exception a Rust refusal of that kind is rethrown as |
| `HostOperations` (+ `HostOperation`) | `setup/ProjectData` | **stays** as the interface; `DuckDbHostOperations`, `PostgresHostOperations` move |
| `DdlSpelling` | `setup/Ddl` | **stays** (vocabulary) |
| `RawSqlBoundary` | `ExecuteOptions`, `setup/Ddl` | **stays** (vocabulary) |
| `EngineTextDialect` | `driver` (EngineText, BodyDriver, PlanEnvelope), `testdatagen` | **stays**: the interface; `EngineStyleH2`'s implementation moves |
| `ExplosionMarkers` | none -- every reference is inside the package | moves |
| the 16 renderers, `AnsiSqlRenderer`, `SqlWriter`, the passes, the spellings | none | move, then deleted |

`Databases.dialect(type)` makes a new dialect per call; only `H2.forServer` differs (`H2Modern` for an unpinned
server). A caller keeps one dialect for several statements, and three of them carry counters across renders
(PHASE0.md §4), so the Rust side keeps per-dialect state behind each Java dialect object: Java's text, exactly,
until a decision to reset per render is posted and ledgered.

## 3. The FFI shape

One export, `lite_dialect(handle, op, json) -> folded answer`, dispatched in Rust, not one symbol per method: a dialect
is opened (`type`, server version) into a handle that holds its per-dialect state; its facts (`temporaryTables`,
`loadsRowsInBulk`, `relational`, `inProcess`, ...) come back once as a descriptor the Java side caches; `render`,
`renderStatement`, the DDL/DML forms and the helpers (`storedName`, `insertColumnName`, `raw`, `wireTypeName`,
`catalogType`, ...) are ops over `MirJson`. A refusal of kind `DIALECT_CAPABILITY` is rethrown as `DialectCapability`.

## 4. Order

1. Pre-deletion records -- **done 2026-11-02**: the receipt
   `docs/build-inventory/program/evidence/compiler/BASELINE_2026_11_02.md` (the machine was not quiet: load 10.4 at its
   start, recorded in the receipt; the render stage, phase 1's comparison, is p50 0.01 ms over the stress corpus's
   4,733 queries and 0.04 ms over the DataCube set's 69); the conformance families and their JVM run moved to
   `//rust/oracles/conformance`, regenerating the frozen snapshot byte for byte (its SHA-256 in `MANIFEST.tsv`), the
   TeaVM exports left in `//wasm` until it goes.
2. `lite-json` (a port of `json/`: insertion order, raw number tokens) and `lite-sql`'s MIR, mirroring `MirJson`'s tags,
   with its reader; a golden dump from the probe -- `(dialect, server, MirJson, Java's render)`, deduplicated and
   measured before deciding what is committed. **Done 2026-11-02**: the dump is 28,951 statements answered on fresh
   dialects (DuckDB 16,154; 138 refusals), 2.9 MB compressed (`rust/oracles/mir/`, decompressed for tests by the
   hermetic zstd); `lite-sql` reads every one into the MIR and writes it back to the bytes Java wrote
   (`//rust/lite-sql:mir_round_trip_test`). The deepest tree needs more than 1 MB of stack in a debug build (256 KB in
   release): the dialect calls run on a big-stack thread or behind a depth limit (TRIAGE A3). The DuckDB slice of step
   3 is measured at about 16K lines of Java (`AnsiSqlRenderer`, `SqlTyping`, `CarrierStrategies`, `DuckDb`, and about
   60 passes and helpers, without the other dialects).
3. The shared passes, `AnsiSqlRenderer` as a trait with defaults, the writer; DuckDB to byte parity on the dump and the
   probe's differential. Then **DuckDB alone switches**: `Databases.dialect(DUCKDB)` answers the Rust-backed dialect,
   `DuckDb.java` is deleted, the other fifteen stay Java. The TeaVM planner targets go in the same commit (core's
   planning code can no longer be compiled by TeaVM), and the libcompiler recipe of §1 is applied.

   **Done 2026-11-03.** DuckDB renders all 16,154 dump statements as Java did (rust/lite-dialect/PARITY.md), and
   `Databases.dialect(DUCKDB)` answers a shim: `com.legend.sql.dialect.DuckDb` delegates every `SqlDialect` method,
   through the `RustDialects` SPI (the SQL layer stays standalone; the provider, `com.legend.ffi.RustDialectProvider`,
   is found by `ServiceLoader`), to `RustDialect`, one `lite_dialect` call each over MirJson
   (rust/lite-dialect/SEAM.md). Its tables (`LEXICON`, `CATALOG_TYPES`, `CATALOG_REFUSED`) and `readsStored` are the
   seam's `duckdb.tables` and `duckdb.readsStored`; `StableScanOrder.firings()` adds the library's own count
   (`scanOrderFirings`), so the corpus lanes' engine-order registers came back byte for byte (872 and 980 tests, each
   with its firing count). The Java renderer and `DuckDbHostOperations` are deleted; the guards that pinned them
   follow the code (rust/RETIRED_TESTS.md). The TeaVM modules were frozen instead of deleted (§5).

   libcompiler takes §1's recipe: native access; the six downcall shapes declared, held to `Lite`'s descriptors by
   `//core:ffi_test` (the image registers five: `jlong()` and `void*()` are one native shape); and the Rust library
   found BESIDE it (`native/.../RustLibrary.java`: the address of one of its own entry points names its file,
   `ProcessProperties.getObjectFile`, an `--initialize-at-build-time` class holding the literal). The wheel ships the
   two side by side in `_native/` (`//python:wheel_rust_library`), and `//python:wheel_test` measures both and runs the
   installed wheel with nothing pointing anywhere. Bazel's runs carry `//rust/lite-ffi:lite_ffi` beside
   `//native:compiler` and find it in their runfiles. Probed by hand before the wiring: libcompiler and liblite_ffi
   alone in a folder, no runfiles, Python planning a DuckDB query -- answered, 0 unfreed.

   `bazel test //...` against rust/BASELINE.tsv: every baseline target keeps its status; the new targets are the Rust
   crates' tests and `//core:ffi_test`.

   Open, each small: a libcompiler without the Rust library beside it answers `ExceptionInInitializerError: null`
   (DuckDb's tables are read when the class loads) -- the wheel and Bazel never lack it, but the message should name
   the file; `docs/dialect-capability` lost DuckDB's column (the matrix is read from Java sources) until the Rust
   dialects generate theirs; the Windows wheel's Rust library imports, and its Linux glibc floor, are measured by the
   first CI run on those systems (macOS is measured here); `RustDialectProvider` asks for `duckdb.tables` three times
   at class load, where one would do.
4. Then by lane: H2 with the engine-style dialects (`corpus_h2`, `pct_h2`), Postgres, SQLite; the cloud dialects;
   MongoDB (and its pipeline) as an implementation of its own. `AnsiSqlRenderer` and the passes are deleted with the
   last.

## 5. The browser keeps a frozen planner (decided 2026-11-03)

Switching DuckDB to Rust takes core's dialects out of what TeaVM can compile. Deleting the TeaVM planner would have
broken 103 test targets (DataCube 71, Studio 8, Query 5, the Python wheel and notebook, engine-client, the site, guards)
and in-browser planning in DataCube, Query and Studio; `//sdlc-server:page` compiles core too. Because the Rust
dialects render exactly as Java's, the user chose to **freeze** instead: `//wasm:planner` and `//sdlc-server:page` are
the modules TeaVM last built (at `6f61be3f0`), committed under `wasm/planner/` and `sdlc-server/page/` and served by
filegroups of the same names. All 243 tests that reach them pass on the frozen files; `//wasm:differential_test` now
holds the frozen Java module to the live JVM's answers, a differential of its own once DuckDB renders in Rust.
`//:wasm` therefore registers no action, so `CompileOnlyTest` no longer expects a TeaVM compile or a javac plugin in
that tier (`rust/RETIRED_TESTS.md`). The Rust wasm32 build replaces both modules later.
