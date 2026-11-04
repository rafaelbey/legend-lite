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
   alone in a folder, no runfiles, Python planning a DuckDB query -- answered, 0 unfreed. The lookup runs when an entry
   class that can reach the dialects loads: `nativelib.Compiler` (through `//wasm:boundary`) and `ModelHome` (through
   `//sdlc-server/grammar`); by `bazel query`, the warehouse's, the saved queries' and the depot's entries reach no
   dialect -- what phase 4's entry-point drop needs to know.

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

   **H2 and the engine-style dialects** (`H2`, `H2Modern`; `EngineStyleH2`, `EngineStyleDB2`, `EngineStyleComposite`):
   4a an oracle, 4b H2 and H2Modern, 4c the engine-style three, 4d the switch.

   - **4a, done 2026-11-03: the oracle.** The first dump holds the engine-style dialects thinly (39, 10, 10 entries:
     DDL and DML only): its probe hooked `AnsiSqlRenderer`'s render, which `EngineStyleH2` overrides, and could keep
     nothing from the corpus lanes, which run as build actions. A second probe (`rust/tools/h2-dump-probe.patch`, never
     committed to the sources) rode the whole suite, the lanes' judge passes included (a `java_run` action's
     `java.io.tmpdir` is its declared scratch directory), and dumped the H2 family's every entry point:
     `rust/oracles/mir/h2-dump.jsonl.zst`, 24,001 entries, no input with two answers -- H2 21,453 (2,190 of them
     `fromEngineText`), H2Modern 386, EngineStyleH2 about 2,100 over five configurations (`quoteIdentifiers`, the plan
     text's time zone: none, GMT, EST, US/Arizona) with every `EngineTextDialect` method, DB2 51, Composite 19. An
     engine-text dialect is made with arguments, so each line carries its `config`, and the seam's spec must too.
   - **4b, done 2026-11-03: H2 and H2Modern in Rust** (`rust/lite-dialect/src/h2.rs`, `h2_passes.rs`;
     `rust/lite-dialect/PARITY.md`). Every input of both oracles renders as Java's -- H2 21,847 of 21,847 distinct
     inputs, H2Modern 394 of 394, the SQL, binds and regions exact and every refusal Java's class and message -- and
     DuckDB stays at 16,154; the typing rules hold over both oracles (2,257,711 facts, 0 different) and lite-sql reads
     back every MirJson text of both (26,887). H2Modern is H2's struct with a `modern` flag (its ten overrides branch on
     it). The passes `H2.passes()` installs are ported (`QualifyToSubselect`'s plain form, never installed on H2, and
     `ConstantLateralsAsRows`, `LateralExplodeToUnion`, `AggregatesDeliverDouble`, `SourceSpelling`), and
     `CarrierStrategies` ran under H2's caps for the first time, unchanged. The differential is one test file for every
     dialect (`tests/dialect_differential.rs`, `//rust/lite-dialect:dialect_differential_test`, a floor per dialect).
     The seam opens `H2` and `H2Modern` and answers a new op, `forServer` (the chosen dialect's name, for the shim to
     build: no Java code chooses a dialect by its class), and `facts` writes H2Modern's refused `serverVersions`
     (`rust/lite-dialect/SEAM.md` §3a). For the switch (4d): the shims of `H2` and `H2Modern` (`H2Modern extends
     H2` kept), `forServer` through the op, `serverVersions` rethrown from the facts, and the guards listed below.
   - **One engine-text query holds a `SqlExpr.Tagged`** (`sqlstring::testGreatestLeast`, met four times: the H2 and
     DuckDB lanes' two passes each), which MirJson refuses by design (a tagged value is resolved before a plan is
     written) and lite-sql has no variant for; `EngineStyleH2.tagged` writes the value's `comparable` alone.
     **Decided 2026-11-03, measured:** the engine-text shim replaces each `Tagged` by its comparable before it writes
     MirJson. A probe on the lane rendered both on fresh dialects: the substituted query's text equals Java's text of
     the original exactly, and MirJson writes it. `rust/oracles/mir/tagged-engine.jsonl` holds it (the substituted
     MIR, Java's answer for the original) for 4c's differential.
   - **4c, done 2026-11-03: the engine-text dialects in Rust** (`rust/lite-dialect/src/engine_style.rs`,
     `java_time.rs`, `decode_shapes.rs`; `rust/lite-dialect/PARITY.md`). Every input of the three oracles answers as
     Java's -- EngineStyleH2 2,105 of 2,105 distinct inputs over its five configurations (the tagged query among them),
     EngineStyleDB2 56 of 56, EngineStyleComposite 24 of 24: the text, every engine-text op and every refusal exact,
     the first measurement already 100 % -- and DuckDB, H2 and H2Modern stay at 16,154, 21,847 and 394; the typing
     holds over the three oracles (2,257,734 facts of 24,645 statements). The three are one struct with a profile:
     each of DB2's twelve overrides and Composite's `call` is a branch where EngineStyleH2's method is written, a
     `super.call` is `h2_call`. Java's `EngineTextDialect` is a trait the seam reaches through
     `SqlDialect::engine_text`. The seam opens the three with their configuration (`quoteIdentifiers`, `timeZone`;
     refused on any other dialect; Composite has only its default) and answers five `engine.*` ops (SEAM.md §3b).
     `renderedAlias`, the one stateful method (it reads the alias plan of the dialect object's last `render` or
     `selectList`), takes the query the caller rendered and replays its plan; the differential holds 5,158 aliases to
     the stateful answer. A date-time constant under a connection's zone is shifted exactly as Java's: **decided
     2026-11-03 (the user: reproduce Java, no refusal)**, the dialect library embeds the build JDK's whole tzdb --
     `rust/oracles/tzdb` (`TzdbData.java`, the build action `//rust/oracles/tzdb:tzdb_data` on remotejdk_25: JDK
     25.0.4+7, tzdb 2026b; 604 regions, 345 rule sets, the 28 short IDs; each rule set `ZoneRules`' own arrays), 145,799
     bytes in `tzdb.bin` (its SHA-256 in MANIFEST.tsv; the release `liblite_ffi` grows by 135,200 bytes, 3.2 %) -- and
     `java_time.rs` answers `ZoneRules`' `getOffset`, `getStandardOffset`, `getValidOffsets`, `getTransition` and
     `ZonedDateTime.of`'s resolution as the JDK 25 source does, for any year. Held to java.time with 0 different: 119,376
     instants (and their `timestampLit` shifts) and 196,892 local date-times (gaps and overlaps) over every ID and every
     rule set's transitions of 1800-2100 and years 1, 1000, 5000 and 9999 (`zone-golden.tsv.zst`), and 684 renders of
     Java's own `EngineStyleH2(false, zone)` over 30 zones (`engine-text-zones.tsv`, run on core's built jars). For the
     switch (4d): shims of `EngineStyleH2()`, `(boolean)`, `(boolean, String)`, `EngineStyleDB2()`, `(boolean, String)`
     and `EngineStyleComposite()` sending their arguments in the spec; each `Tagged` replaced by its comparable before
     MirJson; `selectList`'s JSON null as Java's null; the shim remembering the last query it passed to `render` or
     `selectList` (and none after `renderAsAliased`) for `renderedAlias`; a `PlanParam`'s MirJson for
     `collectionSplice`; and the tzdb regenerated whenever the build's JDK changes.
   - **What a shim must keep** (an inventory of every use outside the five files): production code only constructs
     them -- `H2()`, `H2Modern()`, `EngineStyleH2()`, `(boolean)`, `(boolean, String)`, `EngineStyleDB2()`,
     `(boolean, String)`, `EngineStyleComposite()` -- and calls `SqlDialect` / `EngineTextDialect` methods; no static
     of theirs is read outside them. Tests type a dialect as `H2` (`TempTableAsTest:80`), so `H2Modern extends H2`
     stays, and several pin a refusal's text holding the dialect's own simple name (`StoreWritesTest:202`,
     `TempTableAsTest:121`, `HostOperationsTest:122`, `TranslateTest:76,107`, `SqlWriterTest:84`,
     `CatalogGridTest:164`). The guards shape the shims: a shim declares `implements SqlDialect, EngineTextDialect`
     (`DialectOwnerExclusivityTest:194-224` finds dialects by that text); no `instanceof` / `X.class` /
     `getClass() ==` on a dialect in main code (`noDialectIsDecidedByItsClass`, `CodeShapeGuardrailTest:415`), so
     `forServer` and the DB2/Composite choice pass a name; `com.legend.ffi` may not name a dialect class
     (`DialectLeakRatchetTest:58`) nor depend on `EngineTextDialect` (`ArchitectureTest:269`).
   - **Guards that move at 4d:** `DialectVocabularyTest:101` (15 `AnsiSqlRenderer` dialects -> 10),
     `ParkedWorkLedgerTest:144` (PARK-18 pins a hook in `EngineStyleH2.java`; with `docs/PARKED_WORK_LEDGER.md`),
     `TestLaneOrderGuardrailTest:66` (`LateralExplodeToUnion.java` mints RowOrder), and the regenerated
     `docs/dialect-capability`. The H2 family is the last user of `ConstantLateralsAsRows`, `SourceSpelling`,
     `LateralExplodeToUnion` and `DuckDbSpellings`, which go with it; `QualifyToSubselect`, `AggregatesDeliverDouble`
     and `CarrierStrategies` stay for the other dialects.
   - **4d, done 2026-11-03: the switch.** `H2`, `H2Modern`, `EngineStyleH2`, `EngineStyleDB2` and
     `EngineStyleComposite` are shims over the Rust library, keeping their names, constructors and hierarchy
     (`H2Modern extends H2`; DB2 and Composite extend the engine text), each method one seam call through
     `RustDialects` (the SQL layer still depends on nothing outside itself). `H2.forServer` asks the library which
     profile a version takes (`forServer`) and makes `H2Modern` by name; an engine-text dialect is made with its
     configuration (`RustDialects.engineText`), replaces each tagged value by its comparable before the seam
     (`TaggedComparables`), and remembers the query it last rendered for `renderedAlias`. The Java renderers are gone,
     with `ConstantLateralsAsRows`, `SourceSpelling`, `LateralExplodeToUnion` and `DuckDbSpellings`, whose last users
     they were; ten dialects remain on `AnsiSqlRenderer`. Three guards moved with the code (`rust/RETIRED_TESTS.md`):
     `DialectVocabularyTest`'s floor, the RowOrder mint list, and PARK-18's anchor (re-pointed to
     `engine_style.rs`); `docs/dialect-capability` regenerated without the five columns.

     **The shipped server jar carries the library** (found by this switch's full run). `//core:server_deploy.jar`,
     "the one self-contained jar to ship", run as `java -jar` outside Bazel had no way to find `liblite_ffi` -- no
     property, no runfiles -- so since the DuckDB switch its first Rust dialect call failed; every test runs under
     Bazel, and `//fixtures/saved-queries:gen`, which starts the jar to record saved queries on H2, was the first to
     call one. `//core:ffi`'s jar now carries the library its build made (`//rust/lite-ffi:lite_ffi_jar_resource`),
     which `Lite.locate` writes out to a file of the process's own when neither the property nor the runfiles name one
     (as the DuckDB driver beside it does with its own), and the jar's manifest allows native access. The jar is now
     the build platform's (it was platform-independent Java; the DuckDB driver already bundles every platform's
     natives): a jar for several platforms would carry each platform's library, built where CI builds the wheels.

     The gates: the H2 and DuckDB corpus verdicts pass and all ten of their roster diffs regenerate byte for byte;
     `pct_h2` and PCT channel B on H2 pass; the frozen browser module's differential passes; `bazel test //...` against
     rust/BASELINE.tsv -- 517 pass, the same 2 skipped, every baseline target at its baseline status, the new targets
     the Rust crates' tests and `//core:ffi_test`.

   **Postgres** (step 5; the cloud dialects, MariaDB/MySQL, SQLite and MongoDB follow over the same oracle):

   - **5a, done 2026-11-03: the oracle.** `rust/oracles/mir/dialects-dump.jsonl.zst` (`//rust/oracles:dialects_dump`;
     its `MANIFEST.tsv` line): the ten Java dialects left after the H2 family's switch, rendered over `bazel test //...`
     by a probe never committed to the sources -- 13,918 entries, Postgres 11,613 of them, SQLite 2,081, the others 8 to
     39 each.
   - **5b, done 2026-11-03: Postgres in Rust** (`rust/lite-dialect/src/postgres.rs`, `src/shared_passes.rs`;
     `rust/lite-dialect/PARITY.md`, `SEAM.md` §3c). Every Postgres input of both oracles answers as Java's -- **11,630
     of 11,630** distinct inputs (11,613 dialects-dump lines, 2,081 of the first dump), the SQL, binds and regions exact
     and all 9 of Java's refusals by class and message, **the first measurement already 100 %** -- and the seam answers
     every op as a direct call does (11,630 ops, 42 inline renders); DuckDB, H2, H2Modern and the engine text stay at
     16,154, 21,847, 394, 2,105, 56 and 24. The typing holds over the three dumps (34,069 statements, 4,045,363 facts, 0
     different), and lite-sql reads back every MirJson text of the three (36,330; the new dump's ten dialects 9,443 of
     them). The oracle exercises 153 of the 181 `SqlFn` arms; the other 28 (but `FLOAT_CANON`'s, which the Float
     expansion leaves nothing to reach), the refusals and `fromEngineText` (no oracle line) are unit-tested from the
     Java (`src/postgres/tests.rs`, 22 tests). The passes Postgres installs that no
     earlier dialect ran (`StarExceptToColumns`, `WholePartitionOrderedSets`, `StringCountsAsInt`, `RootNumericTypes`,
     `QualifyToSubselect(true)`) are in `src/shared_passes.rs` with `QualifyToSubselect`'s plain form and
     `AggregatesDeliverDouble`, where the remaining dialects find them; `CarrierStrategies` ran under Postgres's caps
     unchanged. The renderer trait took what Postgres's overrides need: `call`, `case_expr` and `struct_get` see the
     node's type fact, `fold_call` the node, and `string_lit`, `bound`, `row_order_column` and `star_except_keyword` may
     refuse (DuckDB's, H2's and the engine text's output unchanged). Under Bazel the differential takes 54 s (47 s
     before) and the round trip 67 s, both `size = medium`.

     For the switch (5c): a `Postgres` shim (`new Postgres()`, `Databases.dialect(Postgres)`; `PostgresCatalogTest`
     tests `instanceof Postgres`) whose every method is one seam call, the facts read once; `hostOperations()` through
     the `host.*` ops (`PostgresHostOperations` moves); and two protected methods core's same-package tests call on a
     Postgres object, answered by two new ops every dialect answers -- `readsStored` (`StoredReadsTest:49`, `new
     StoredReads(new Postgres()::readsStored)`) and `tableName` (`TempTableAsTest:71`). No public static of Postgres's
     (`LEXICON`, `CAPS`, `CATALOG_TYPES`, `CATALOG_RULES`) is read outside `Postgres.java`, and Postgres keeps no state
     between renders. `StarExceptToColumns`, `WholePartitionOrderedSets`, `StringCountsAsInt`, `RootNumericTypes`,
     `QualifyToSubselect`, `AggregatesDeliverDouble`, `ConstantKeysAsExpressions` and `SubstringClamp` stay Java for
     the dialects still Java.
   - **5c, done 2026-11-03: Postgres switches.** `Postgres` is a shim over the Rust library -- its name, its
     constructor, and `readsStored` and `tableName`, which same-package tests call, through the seam's general
     `readsStored` and `tableName` ops (`RustDialects`). `PostgresHostOperations` and `StringCountsAsInt`, whose last
     user it was, are deleted. Two guards moved (`rust/RETIRED_TESTS.md`): `RawSqlLedgerTest`, whose ledger is empty (no
     Java caller of the raw translation is left), and `DialectVocabularyTest`'s floor (10 -> 9); `docs/dialect-capability`
     regenerated. `bazel test //...` against rust/BASELINE.tsv: 517 pass, the same 2 skipped, every baseline target at its
     baseline status (run with the counter reset below).
   - **Fresh-name counters reset per render (the user's ruling, 2026-11-03).** SQLite's `__b<n>`, Snowflake's `__f<n>`,
     `__s<n>`, ... and `CommonSubexpressions`' `__cse<n>` were numbered by counters that, in Java, could outlive a render
     (PHASE0.md §4): the text a dialect object wrote depended on the statements it had written before, and the seam
     makes a fresh dialect per call. Posted with both options -- keep Java's counters through the seam, or reset per
     render -- and the user ruled: reset per render, and asked whether Java could do the same. Measured: the
     `CommonSubexpressions` pass already reset (`renderPasses` builds a fresh one for each render); SQLite's and
     Snowflake's names are a statement's own ("unique within a statement", SQLite's comment), no caller pastes one
     render into another statement, and no test pins their numbers. So Java took the ruling first: SQLite and Snowflake
     start their names over at each outermost query render (their `roots` stack empty). The same full run as 5c held
     every baseline target, so the change is invisible to the suite, and their Rust ports reproduce Java exactly; the
     golden dumps, made on fresh dialects, hold as they are.
   - **5d, done 2026-11-03: SQLite in Rust** (`rust/lite-dialect/src/sqlite.rs`, `src/sqlite/` -- the dialect,
     `SqliteRegex`, `SqliteDigests` -- and, shared for the dialects still to come, `src/explosion_markers.rs` and
     `src/common_subexpressions.rs`; `rust/lite-dialect/PARITY.md`, `SEAM.md` §3d). Every SQLite input of the oracles
     answers as Java's -- **2,529 of 2,529** distinct inputs, the SQL, binds and regions exact and all 140 of Java's
     refusals by class and message, **the first measurement already 100 %** -- and the seam answers every op as a direct
     call does (2,529 ops, 46 inline renders); every other dialect stays at its floor. The dumps hold 2,158 distinct
     SQLite inputs, which reach 138 of the 181 `SqlFn` arms and never the regular expressions' find path, the maps or a
     firing `CommonSubexpressions`, so `rust/oracles/mir/SqliteArms.java` builds 371 more with Java's constructors,
     writes each as MirJson, reads it back and renders it on a fresh `new SQLite()` on core's built jars
     (`mir/sqlite-arms.jsonl.zst`, `//rust/oracles:sqlite_arms`, its MANIFEST row): every `SqlFn` arm is held now, and
     each is Java's own answer, not a hand-derived text (367 of 367 identical at the first Rust measurement). Two
     identity questions were settled by Java's answers: SQLite's root selects are Java's `IdentityHashMap`, but its last
     pass, `CommonSubexpressions`, hands every equal select one object, so a root select met again inside another branch
     is delivered there too -- Rust keeps them by content; and `CommonSubexpressions`' own identity over its pooled tree
     is equality (a hash-consed DAG in Rust). The per-render state (`laterals`, `rowLaterals`, `roots`, `rootSelects`,
     `binds`) lives in `RefCell`s for one call; `__b<n>`, `__cse<n>` and `__unnest<n>` start at 0 per render, as the
     ruling above made Java's. Not exact, none reached by an oracle (`PARITY.md`): `CommonSubexpressions`' tie between
     two largest repeated nodes (Java's identity-hash order, which varies between JVM runs; Rust's first met), and
     an unterminated named group in a regular expression (Java overflows its stack; Rust refuses as
     `java.lang.StackOverflowError`). `Character.isLetterOrDigit` and `isDigit` (and, across the dialects, `isDigit` in
     DuckDB's `structGet` and Postgres's `timestampLit`, `isWhitespace` in the JSON and raw-SQL readers) are the build
     JDK's own classes per UTF-16 unit, `lite_javacompat::character`, walked over every code point against
     `char-classes.tsv` (`//rust/lite-javacompat:char_classes_test`). Under Bazel the differential
     takes 54 s and the round trip 68 s (both unchanged, `size = medium`).

     For the switch (5e): a `SQLite` shim (`new SQLite()` in `Databases` and seven core tests, its simple name in their
     refusal assertions) whose every method is one seam call, the facts read once. No public static of SQLite's
     (`LEXICON`, `CAPS`) is read outside `SQLite.java`, and no test calls a non-interface method of a SQLite object, so
     no new op is needed; SQLite keeps no state the shim must carry between calls (its per-render state is a call's own;
     a JVM object that rendered a query and then a `CREATE TEMPORARY TABLE ... AS` kept counting `__b<n>` across the two
     in Java, a fresh seam dialect does not -- no test of core, pct, spec or parser-equivalence names a `__b<n>`,
     `__cse<n>` or `__unnest<n>`, grepped; `TempTableAsTest:130` renders one query three times on one object and holds
     the third to the first, which both keep: each is an outermost render). `SqliteArms.java` freezes Java's current
     answers: rerun it if `SQLite.java` changes before the switch. `SqliteRegex` and `SqliteDigests`
     go with `SQLite.java`; `ExplosionMarkers` and `CommonSubexpressions` stay Java for Snowflake, BigQuery, MariaDB,
     ClickHouse and Trino, so `CodeShapeGuardrailTest:278`'s `ExplosionMarkers.next` pin does not move. What reads
     `SQLite.java`'s source moves with it: `DialectVocabularyTest`'s floor of `AnsiSqlRenderer` dialects (9 -> 8; its
     switch finder names `SQLite.calls`, line 282) and the regenerated `docs/dialect-capability` (`CapabilityMatrix`,
     which loses SQLite's column as it lost DuckDB's); the name censuses (`DialectLeakRatchetTest:59`,
     `DialectBoundaryTest:185`) hold as long as the shim keeps the name `SQLite` and its constructions stay where they
     are. The switch's gates are the SQLite lanes: `//core:core_tests_integration_sqlite`, `//pct:pct_sqlite` and PCT
     channel B on SQLite.
   - **5e, done 2026-11-04: SQLite switches.** `SQLite` is a shim over the Rust library (its name and constructor;
     every method one seam call); `SqliteRegex` and `SqliteDigests` are deleted with it. `DialectVocabularyTest`'s floor
     moved (9 -> 8; `rust/RETIRED_TESTS.md`) and `docs/dialect-capability` was regenerated. `bazel test //...` against
     rust/BASELINE.tsv: every baseline target at its baseline status, 518 pass and the same 2 skipped -- the run went
     through a night's sleep, which timed out 80 targets (`//core:ffi_test`, a second's work, among them) and a browser
     wait; those 81 were rerun awake and all pass.

   **The eight left** (step 6: Snowflake, BigQuery, Databricks, Trino, ClickHouse, MariaDb, MySql, MongoDb):

   - **6a, done 2026-11-04: their arms oracle.** No lane runs them, so the whole-suite dump holds 8 to 39 lines of each.
     `rust/oracles/mir/DialectArms.java` builds 2,599 inputs with Java's constructors (SqliteArms' builders where they
     fit), writes each as MirJson, reads it back, and renders it on a fresh dialect of each class on core's built jars
     (JDK 25.0.4+7; its command in its header): `mir/dialect-arms.jsonl.zst` (`//rust/oracles:dialect_arms`, its
     MANIFEST row), 20,786 lines, about 2,600 per dialect, 3,023 of them refusals, the five render ops and
     `fromEngineText` (every one a refusal). MirJson refuses 2 inputs, by design (a `Tagged`, a `TaggedList`); 6 answers
     are no line, a `ClassCastException` whose message names the JVM's class loader -- Java's own defects, a failed cast
     inside the renderer: LIST_TRANSFORM without a lambda on Snowflake, Databricks, Trino and ClickHouse (a shape no
     lowering builds), and two shapes lowering does build, a Databricks pivot of AVG (`AggregatesDeliverDouble` wraps
     the pivot's reducer in a cast) and a MongoDB window over a grouped aggregate: each a defect for the user to rule
     on, a fix or a refusal by name.
     lite-sql reads every line's MirJson back to Java's bytes (`mir_round_trip_test` reads the file).

     Measured, not reasoned: `rust/tools/dialect-arms-coverage.py` instruments a copy of the eight dialects' sources
     (MongoDb with `MongoDbPipeline` and `MongoJson`; every switch's selector, every `DialectCapability` built and
     every pass's effect recorded per dialect, the lines kept), compiles it before core's jars, reruns the generator
     with the dump's 224 lines of the eight replayed (each answered as the dump says), and holds the instrumented
     oracle to the plain one byte for byte. What the two oracles reach together (a label of MySql's is MariaDb's
     where MySql delegates; MariaDb's methods MySql overrides without `super` are not MySql's):

     | dialect | `SqlFn` (`call`) | `SqlAgg.Fn` (exhaustive) | `SqlAgg.Fn` (others) | own walls | other enum labels | pattern labels | passes changing a query |
     |---|---|---|---|---|---|---|---|
     | Snowflake | 181/181 | 80/80 | -- | 64/65 | 168/174 | 42/42 | 9/10 |
     | BigQuery | 181/181 | 40/40 | -- | 69/70 | 148/150 | 39/39 | 12/12 |
     | Databricks | 181/181 | 40/40 | 11/11 | 35/35 | 171/176 | 23/23 | 9/10 |
     | Trino | 181/181 | 40/40 | 8/8 | 33/33 | 156/161 | 21/21 | 15/16 |
     | ClickHouse | 181/181 | 40/40 | 45/46 | 56/56 | 198/209 | 55/55 | 13/14 |
     | MariaDb | 181/181 | 40/40 | 6/6 | 59/61 | 229/250 | 28/28 | 11/11 |
     | MySql | 181/181 | 80/80 | 6/6 | 53/62 | 234/259 | 28/28 | 12/12 |
     | MongoDb | 179/181 | 79/80 | 23/23 | 74/80 | 153/178 | 148/158 | 3/4 |

     Every arm not reached is dead in Java or refused before it, each read in the source:
     - `CarrierStrategies` changes no query where `CAPS.nativeLists` holds (Snowflake, Databricks, Trino, ClickHouse,
       MongoDb): its three hooks return their input (`CarrierStrategies.java:95,354,790`). Their ports need not run it.
     - A helper's switch whose one caller passes only its labels: `bitOp`'s default on all eight; `listCall` on
       Databricks, Trino and ClickHouse (LIST_REVERSE, RANGE_FN, REPEAT_VALUE, which `call` spells itself, and default)
       and MongoDb (default); Snowflake's `list` and `format` defaults (its directive pattern admits s, d, f);
       BigQuery's and MariaDb's `maps`, MariaDb's `lists`; ClickHouse's `temporal` and inner `reduceCollection`
       defaults; MongoDb's `timeBucket`, `strftime`, `jsonText`, `accumulator`, `emptyInput` and the pipeline's two
       subquery defaults.
     - An earlier check takes it: Snowflake's `dateTrunc` NANOSECOND, DAY_OF_YEAR, ISO_DAY_OF_WEEK and its wall (`unit`
       refuses them first, :2355); ClickHouse's `outsideDates` QUARTER and the three (ADD_INTERVAL's unit wall, :2052)
       and `jodaText` SUBSEC_MIN (cut by `formatted`, :2776); MariaDb's `extract` YEAR (`yearOf`, :2720) and nine labels
       of its second switch (:2734), `scalarOfText` BOOLEAN, VARCHAR, TEMPORAL_TEXT, DECIMAL_TEXT (:1139, :1143);
       MongoDb's `reduceArray` COUNT (`$size`, :2313) and `scalarTypeName` BOOLEAN, TIMESTAMPTZ, JSON and its wall
       (`variantAwareCast` takes them first).
     - No caller: MongoDb's `ddlType` (21 labels, 2 walls: its `render(SqlDdl)` writes no column type), its
       `structTypeName` and depth-above-0 query walls (its `query` replaces the base's) and the pipeline's flat-join
       wall (`join`/`oneSource` refuse first); MariaDb's (and MySql's) `scalarTypeName` JSON, TEMPORAL_TEXT, TIME,
       DECIMAL_TEXT and `structTypeName` wall (no `castTypeName` entry left to MariaDb carries them).
     - No such value: BigQuery's NULL list of unknown element (a `NullLit` is always bottom) and MariaDb's non-finite
       float (a `FloatLit` cannot hold one).
     - MySql: MariaDb's seven walls behind `!correlatesDerived()` (MySql's is true, `MySql.java:365`).
     - MongoDb: `Tagged`/`TaggedList` (MirJson refuses them) and FLOAT_CANON/FLOAT_PRINT, expanded before every pass,
       which reach the other dialects' arms only in an `InsertValues` row (a DML row is written with no pass,
       `AnsiSqlRenderer.java:1778`) -- MongoDb's insert refuses a cell that is no literal.

     Off the ops (their decisions are the seam's facts, not a render's): `wireTypeName`, `typingWireName`,
     `deliversOwn`, `sessionTable`, `sessionSetup`, `keysSnapshot`, `resultColumnLimit`, `needsStaticPivot`,
     `temporaryTables` -- 4 labels on BigQuery, 15 on ClickHouse, 8 on MariaDb and MySql, one wall each on Databricks
     and Trino. Java's oddities the ports must reproduce, as the oracle holds them (each a defect for the user to rule
     on): MongoDb writes `{"_id": 0"t0": {}}` (no comma) for a source of no fields; a quantified comparison by
     IS_DISTINCT_FROM is a `NullPointerException` on five dialects; a collection's CORR, COVAR_SAMP or COVAR_POP over a
     list of structs is an `IndexOutOfBoundsException` on MariaDb and MySql, as is a window's NTH_VALUE of one argument
     on MongoDb. `DialectArms.java` freezes Java's current answers: rerun it (and the measure) if a dialect's source
     changes before its switch.
   - **6b, done 2026-11-04: Trino and Databricks in Rust** (`rust/lite-dialect/src/trino.rs`, `src/trino/`,
     `src/databricks.rs`, `src/databricks/`; `PivotAsGroupBy` and `AsOfAsPick` in `src/shared_passes.rs`, where
     Snowflake and ClickHouse will find the first; `PARITY.md`, `SEAM.md` §3e and §3f). Every input of the three oracles
     answers as Java's -- **Trino 2,630 of 2,630** distinct inputs (2,598 arms-oracle lines, 52 of the two dumps; Java's
     350 refusals by class and message, the quantified IS_DISTINCT_FROM's `NullPointerException` among them) and
     **Databricks 2,631 of 2,631** (2,597 and 55; 359 refusals), the SQL, binds and regions exact -- and the seam answers
     every op as a direct call does (2,706 and 2,707 ops with the inline renders). Databricks' first measurement was
     already 100 %, Trino's 2,626 (four `regexp_replace` group digits, a port slip, fixed). The differential now reads
     `//rust/oracles:dialect_arms` too: every other dialect stays at its floor, and the typing holds over 36,890
     statements, 4,057,096 facts, 0 different. Both dialects' every `@Override` is a renderer hook as before; one hook
     widened -- `values_source` takes the VALUES' declared outputs (Java's `SqlSource.Values v`), which Databricks' typed
     NULL column reads, the other dialects' output unchanged -- and the walk's pivot-USING cast now words Java's
     `ClassCastException` as Java does (`com.legend.sql.SqlAgg$Reducer`, less the class-loader clause). The six answers
     the arms oracle dropped (a list lambda that is no lambda, a Databricks pivot of AVG) are Java's
     `ClassCastException`, refused in Rust with Java's class (`CLASS_CAST` at the seam) and Java's message less the
     JVM's class-loader clause, unit-tested. What the oracles cannot reach is unit-tested from the Java
     (`src/trino/tests.rs` 7, `src/databricks/tests.rs` 9, `src/seam/trino_tests.rs` 7, `src/seam/databricks_tests.rs`
     5, 2 more in `src/shared_passes/tests.rs`): `listCall`'s and `bitOp`'s unreached arms, the frame intervals,
     Databricks' session table (a temporary view over its rows: no oracle line asks `sessionTable`). Under Bazel the
     differential takes 53 s (54 s before) and the round trip 65 s, both `size = medium`.

     For the switch: a `Trino` and a `Databricks` shim (`new Trino()`, `new Databricks()` in `Databases` and five core
     tests -- `TranslateTest`, `StoreWritesTest`, `CatalogGridTest`, `TempTableAsTest`, `WireTypeNamesTest`, and
     `SeedSpellingTest` for Databricks -- their simple names in refusal messages) whose every method is one seam call,
     the facts read once. No public static of either (`LEXICON`, `CAPS`) is read outside its class, no test calls a
     non-interface method of either, and neither keeps state between renders, so no new op is needed; Databricks'
     `sessionTable` is the seam's existing op. `AsOfAsPick` goes with `Trino.java`; `PivotAsGroupBy` stays Java for
     Snowflake and ClickHouse. What reads their sources moves with them: `DialectVocabularyTest`'s floor of
     `AnsiSqlRenderer` dialects (8 -> 6 after both), the regenerated `docs/dialect-capability`, and
     `PlatformNamesGuardrailTest`'s count of bare-name arms (Trino's and Databricks' date-part tables among them, a
     ceiling). The switch's gates are `//pct:pct_trino` (manual, in `//gates:heavy`: a Trino 483 coordinator the test JVM
     starts from the pinned `@embedded_trino`, no Docker) and `//pct:pct_databricks` (a cloud lane: a real warehouse,
     its credentials in `LEGENDLITE_PCT_CLOUD_PROPERTIES`).

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
