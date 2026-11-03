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

The recipe, reverted after the measurement and re-applied when the first dialect switches:
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

1. Pre-deletion records: `bazel run //tools/metrics:baseline` on a quiet machine, the receipt beside
   `BASELINE_2026_10_08.md`; `wasm/src/main/java/conformance` and `:conformance_jvm` moved under `rust/oracles/`.
2. `lite-json` (a port of `json/`: insertion order, raw number tokens) and `lite-sql`'s MIR, mirroring `MirJson`'s tags,
   with its reader; a golden dump from the probe -- `(dialect, server, MirJson, Java's render)`, deduplicated and
   measured before deciding what is committed.
3. The shared passes, `AnsiSqlRenderer` as a trait with defaults, the writer; DuckDB to byte parity on the dump and the
   probe's differential. Then **DuckDB alone switches**: `Databases.dialect(DUCKDB)` answers the Rust-backed dialect,
   `DuckDb.java` is deleted, the other fifteen stay Java. The TeaVM planner targets go in the same commit (core's
   planning code can no longer be compiled by TeaVM), and the libcompiler recipe of §1 is applied.
4. Then by lane: H2 with the engine-style dialects (`corpus_h2`, `pct_h2`), Postgres, SQLite; the cloud dialects;
   MongoDB (and its pipeline) as an implementation of its own. `AnsiSqlRenderer` and the passes are deleted with the
   last.
