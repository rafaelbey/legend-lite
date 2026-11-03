# Rust port — iteration-1 roadmap (2026-11-02)

The plan that acts on [`TRIAGE.md`](TRIAGE.md). **Iteration 1 removes GraalVM from `libcompiler` and replaces the
whole planning path with Rust.** Java remains afterwards — the executor, the HTTP shell, the test runners, and the
warehouse/sdlc/depot/query-store modules ("What Java remains", below).

## The seams

The port works inward from both ends of the pipeline, over serialized forms that already exist.

- **The back seam — the SQL tree.** `MirJson.query(SqlQuery)` (`executionplan/MirJson.java:71`) and
  `MirJson.readQuery` (`:788`). The dialect's passes run inside `render` (`AnsiSqlRenderer.java:68,129`), so the seam
  is the dialect's entry points: `render(SqlQuery)`, `renderStatement(SqlQuery)`, `render(SqlDdl)`, `render(SqlDml)`,
  `renderStatement(SqlDml)` and `forServer(version)`. `SqlDdl` and `SqlDml` have no JSON yet → MirJson is extended.
- **The front seam — protocol JSON.** The parser's output *is* `Protocol.Element` (`ElementParser.parseSingle:150`),
  and `ModelReader.read`/`readElement` (`protocol/ModelReader.java:36,129`) rebuild identical records: the user's
  Rule A (S89), held corpus-wide by `JsonModelDoorTest` and `ModelReaderParityTest`.
- **The final seam — plan JSON.** `PlanJson` (`legend-lite-plan` v19; write `:98`, read `:354`): Rust plans and the
  Java executor, already walled off from planning (`ArchitectureTest.execReachesNoPlanning`), runs the plan.

## The shim

`//core/ffi` (`com.legend.ffi`): `java.lang.foreign` downcalls in the shape of
`warehouse/.../server/duck/Duck.java` (`SymbolLookup.libraryLookup` and a table of `downcallHandle`s), with
`--enable-native-access=ALL-UNNAMED` added once in the core test macro under `tools/java`. The calling convention is
today's libcompiler's: UTF-8 C strings in and out, the caller frees with `lite_free`, `lite_unfreed` counts.

**The error contract.** Rust answers `ERR\n<kind>\n<phase>\n<line:col>\n<message>`. On the wire (Boundary,
PureV1Api) the kinds are Rust's own. Inside the JVM the shim rethrows the Java class a test expects
(`ParseException`, `TypeInferenceException`, …): about 500 `assertThrows` and 1,181 `getMessage` reads depend on it,
so **message text is a spec surface for iteration 1**. That is the shim's job, not a Java-ism in Rust; it goes with
the tests.

**"Untouched"** means the test sources (`*/src/test/**`, `python/tests`, `studio/test`, `datacube/test`) do not change.
BUILD files, `jvm_flags` and runfiles may. Every exception is a row of `rust/RETIRED_TESTS.md`, dated and with its
reason, per AGENTS.md's rule for moving a pin or a ledger.

## Phases

### Phase 0 — groundwork (no Java deleted)

Steps 1–6 done 2026-11-02; the measurements are [`PHASE0.md`](PHASE0.md). Two deviations from the list below,
both deliberate: step 5 made the crates the work needed now (`lite-ffi`, `lite-javacompat`), not six empty ones --
`lite-json`, `lite-sql`, `lite-front` and `lite-core` arrive with phases 1–3; and `crate_universe` waits for the
first crates.io dependency (none yet).

1. The baseline: `bazel test //...` on the pristine tree, recorded per target as `rust/BASELINE.tsv`.
2. The JVM's answers that are build outputs today, committed: `//wasm:jvm_answers`, `//wasm:zone_jvm`,
   `//parser-equivalence:tab_round_trip`, and `//python:bindings_test`'s answers — otherwise the differentials
   would compare Rust with Rust.
3. Java-compatibility golden sets in `rust/oracles/java-compat/`, generated on the JVM while there is one:
   `Double.toString`, `BigDecimal` (`toString`, `toPlainString`, `equals`), `split`, `Character.isXxx`, `ZoneId`'s
   short IDs, `LocalDateTime.toString`.
4. The SQL-tree round trip proved: for every query a test renders, `render(readQuery(query(q))) == render(q)` byte
   for byte; MirJson extended to `SqlDdl` and `SqlDml`.
5. A Cargo workspace `rust/`, wired into Bazel with `rules_rust` (`crate_universe`). Crates: `lite-javacompat`,
   `lite-json`, `lite-sql`, `lite-front`, `lite-core`, `lite-ffi` (the `cdylib`).
6. The `//core/ffi` shim and the error contract, with one trivial round-trip entry point.

### Phase 1 — the back end

Design, measurements and order: [`PHASE1.md`](PHASE1.md). It adds to the list below: libcompiler's image calls the
Rust library too (measured to work with `invokeExact`), and DuckDB switches alone first.

`sql/dialect` and its 38 rewrite passes → `lite-sql`. Java's dialect entry points become the shim; Java's
`sql/dialect` is deleted (the `sql` records stay: the Java Lowerer still builds them). The TeaVM planner targets
(`wasm/BUILD.bazel:73,214`) and every TeaVM target that compiles core's planning code are deleted. **Keep the
generator of the golden sets:** `wasm/src/main/java/conformance/*` and `//wasm:conformance_jvm` produce
`rust/oracles/jvm/conformance_jvm.txt.zst`; move them under `rust/oracles/` as a JVM-only target before `//wasm` goes,
or the golden set can no longer be regenerated. The differential is `rust/tools/mirjson-roundtrip-probe.patch` with
its second render made by Rust, compared with Java's first render on a fresh dialect (PHASE0.md §4).
**Gates:** the baseline; the SQL tests that build a tree and render it (untouched); `corpus_duckdb`, `corpus_h2`,
`sql-parity`.

### Phase 2 — the front end

The lexer, the parser, the protocol emitter, `json` and the Postgres-SQL→Pure translator → `lite-front`. Java's
`ElementParser`, `SpecParser` and `PmcdParser` entry points become the shim: Rust emits protocol JSON, Java's
`ModelReader` reads it into the same records. The 36 lexer token tests are retired.
**Gates:** parser-equivalence (22,725 elements byte-identical), `ModelReaderParityTest`, `sql-parity` (2,398
Postgres answers).

### Phase 3 — the middle

About 130K lines → `lite-core`: normalizer, compiler, builtin, resolver, lowering, plan, planner, the executionplan
writer, platform, values, lineage, `Compiler` and `TypedQuery`. The node-ID design (D1) and the `Arc` decision (D5)
are settled before any code. `Compiler.compileModel`, `query`, `compileErrors` and `TypedQuery.plan` become the shim
over opaque Rust model handles, answering plan JSON to the Java executor. The Java middle and `ModelReader` are
deleted, and the ~580–770 test methods that inspect its internal objects are retired. This phase has no Java
checkpoint inside it, so its checkpoints are Rust's: per-package unit tests, and the corpora through a Rust CLI
before the switch.
**Gates:** the end-to-end tests (about 2.1–4K methods, through `Lean.run` and `Execution.run`); PCT channel B and the
spec corpus through about 8 shim functions for `driver`.

### Phase 4 — libcompiler in Rust

`rust_shared_library` exporting the compiler's `lite_*` entries: the Boundary, the PureV1Api routes, `Folded`'s
encoding. The warehouse, SDLC, depot and saved-queries entries are dropped; `python/legend_lite/_library.py` loses the
GraalVM isolate calls. `//native`'s `native_image` is deleted; GraalVM then remains only in the warehouse's image.
The library keeps today's platform floor and names: macOS 14 (`native/BUILD.bazel` passes `-mmacosx-version-min=14.0`;
rustc's default deployment target is far older), the `libcompiler` install name and soname.
**Gates:** `//python:bindings_test` against the frozen JVM answers (the dropped entries' tests ledgered); the
`wasm/corpus` answers through the native library; `lite_unfreed == 0`.

### The guard tests

35 classes and 133 methods (`ArchitectureTest`, `JdbcSurfaceCensusTest`, …) name Java classes, so they **fail**,
rather than go vacuous, when a package is deleted. Each phase retires or re-expresses the ones that reference its
deleted packages; the Bazel-query guards (`CoreClosureTest`, `CoreLayeringTest`, `ArrowClosureTest`) carry over as
checks of the crate graph.

## What Java remains after iteration 1

`exec` and the record layer it is walled to — `base`, `json`, `model`, `executionplan`, the `sql` records,
`values`, `result`, `cache`, `diagnostics`, `error` (AGENTS.md's THE WALL, `tools/deps/core-layers.txt`), kept in Java
beside their Rust ports — and the `server` HTTP shell, the `driver` and `judge` test runners, the test harnesses, and
warehouse, sdlc-server, depot-server and query-store.

**Later waves:** the Rust `wasm32` planner brings the browser back (wasm-bindgen, and a `load()` compatible with the
TeaVM loader's call sites, `engine-client/.../planner-worker.ts:16-26` and others); `exec` in Rust (the `duckdb`,
`postgres`, `rusqlite` and `mongodb` crates; H2 needs a decision); the HTTP server, `driver`, `judge`; the SAP IQ and
Cypher translators; warehouse, SDLC, depot and query-store with their TeaVM pages and the warehouse's GraalVM image.

## Verification, every phase

1. `bazel test //...` against `rust/BASELINE.tsv`: every baseline pass still passes, or is a row of
   `rust/RETIRED_TESTS.md`.
2. `git diff --stat <phase-start>.. -- '*/src/test/**' python/tests studio/test datacube/test` lists only ledgered
   files.
3. The oracles stay green: parser-equivalence, `sql-parity`, `corpus_duckdb`, `corpus_h2`, the frozen wasm and Python
   answers, the Java-compatibility golden sets (`cargo test`).
4. `cargo test` and `cargo clippy` clean; the Rust Bazel targets build on macOS and Linux.
