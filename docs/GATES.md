# The gates: what must be green, and how to read a result

## THE GATES UNDER BAZEL (since 2026-09-22) — read this first

Every gate is a test target, and every CI lane is a suite of them in `gates/BUILD.bazel` (since 2026-10-07, the
build rebuild's first landing; the lane set and why: "The CI lanes and why", below). CI runs
`bazel test //gates:<lane>` per lane, in parallel, on Linux, macOS and Windows, and builds the product in its own
job (`.github/workflows/gate.yml`, `gates-run.yml`). The gate numbers (1 to 11, 7P) are the Maven era's, kept so older
records and code comments still read; that era's commands (`mvn`, `tools/allgates.sh`, `-Dx.generate=1`) no longer
exist, and its log, with the per-landing log that followed it, is cut from this file (the pointer is at the end).

**Before pushing to `main`: `bazel test --lockfile_mode=error //gates:local`** (`gates/BUILD.bazel`): a suite of the
light lanes (`core`, `checks`, the warehouse's JVM tests, `datacube`, `ui`, `sdlc`). The heavy lanes (the corpus,
PCT, the stress corpus, parser equivalence, Channel B, the native image's tests) run in CI only; the manual tests are
`//gates:heavy`. The push rule is `AGENTS.md`'s "Pushing to main".

| Lane (`//gates:<lane>`) | Gates | Members | What it holds |
|---|---|---|---|
| `product` (a `bazel build`, not a suite) | 2 | `//:java //:web //:wasm //:native //:sites //datacube:app`; then `bazel build --nobuild --config=bazel10 //...`; then A25; then the lane guard | the product compiles and nothing else (`//tools/guards:compile_only_test` holds the four compile targets to compiles and file plumbing); NullAway is a compile error; the jar pools are one version each by construction; everything still analyzes under Bazel 10's incompatible flags (G14, since 2026-10-04); every test of the repository is in some lane or in `heavy` (`bazel query 'tests(//...) except tests(//gates:lanes) except tests(//gates:heavy)'` is empty). Until 2026-10-07 this lane was `bazel build //...`, which also computed the six corpus passes and the 2 GB `//pct:ratchets` |
| `core` | 1, 3 | `//core:core_tests`, `//core:duckdb_load_test`, `//core:section_grammar_registry_test`, `//core:corpus_differential_test`, `//core:planner_on_java_base_test`, `//core:postgres_arm_test`, `//spec:spec_tests`, `//json:tests`, `//pure-protocol:twins_test`, `//tools/engine-runner:smoke_test` | the compiler suite (NullAway runs on every compile); with `//core:corpus_differential_test`, legend-lite's SQL over the stress seed against the corpus oracle (wired 2026-10-05, P3-18); spec parity: generators, census, manifest; the JSON and protocol libraries; the engine-runner binaries through their launchers (P3-26); the server's Postgres arm, one query against the embedded Postgres 16 |
| `checks` | — | `//:generated`, `//projects:tests`, `//tools/deps:all`, the five `//tools/guards:*_test` plus `workflows_test`, `//tools/junit:pins_test`, `//tools/junit:runner_test`, `//tools/java_run:pins_test`, `//tools/python:lock_matches_requirements`, `//tools/browser:revision_test`, `//tools/browser:proxy_args_test`, `//tools/bump:bump_test`, `//tools/js:lock_matches_package_json_test`, the five `//scripts/corpus:*_gate`, `//core:guardrails`, `//core:census`, `//pct:pct_discipline` | the guards (since 2026-10-04, Bazel workplan Phase 6): every file inventoried and in some package's `all_files`, every jar pool locked and strict, no Markdown test input, no classpath with one Maven coordinate at two versions; the build targets perform only compiles and file plumbing, and the guard's fixture tier proves it still catches a genrule, a java_run, one calling itself Javac, and a validation (since 2026-10-06); the workflows linted by the pinned actionlint (since 2026-10-07; it ran by `curl` before); Bump's rewrites against the real pins; every pnpm lock equal to its package.json; the pinned Chromium equal to every locked Playwright's revision (since 2026-10-04, P1-14); the proxy a test's environment names given to Chromium as its own flags (since 2026-10-25, the timeseries review's G6); the Python lock holding every pin of tools/python/requirements.in with hashes (offline; since 2026-10-04, P1-07); every committed generated file equal to its generator (`//:generated` holds each package's diff-test suite; a missing suite fails the build — since 2026-10-03, replacing a `bazel query` whose failure ran nothing); the model projects, each compiled alone with its declared closure, the graph, the contract (P3-23); the dependency guards; the corpus ratchets (P2-04); PCT's discipline guard (no comparison machinery in the module); the source checks |
| `corpus_duckdb` | 4, 11 | `//spec:corpus_duckdb` | the relational corpus on DuckDB: the host judge (the build action `:judge_host_duckdb`) and the database judge (`:judge_database_duckdb`), joined per assert to the host pass's ledger (one lane since 2026-09-23; both passes cached build actions since 2026-10-05, Bazel workplan P3-01, P2-15); the lane, a suite, is its verdict and the diff tests of what it measures (spec/corpus.bzl) |
| `corpus_h2` | 5 | `//spec:corpus_h2` | the relational corpus on H2, both judges and their per-assert join — :corpus_duckdb's shape (joined since 2026-09-23; `rcorpus/h2-database-untriaged-register.txt` lists what the first join found and has yet to be explained) |
| `pct_duckdb` | 6 | `//pct:pct_duckdb` | the five PCT suites on DuckDB, a suite of one target per PCT suite (`//pct:pct_duckdb_<suite>`, each judged on its own census traffic; Bazel workplan P3-09) |
| `pct_h2` | 7 | `//pct:pct_h2` | PCT relation on H2 2.4.240: 469 tests, H2's 26 expected failures (27 -> 26, 2026-10-02: the self-referencing exists) each pinned by message (`Test_LegendLite_H2_RelationFunctions_PCT`) |
| `pct_postgres` | 7P | `//pct:pct_postgres` | PCT, all five suites, on Postgres 16.15 (one target per suite, `//pct:pct_postgres_<suite>`; P3-09) started by the test JVM from pinned binaries (`@embedded_postgres`, no Docker): 1,249 tests, Postgres's expected failures each pinned by message (`Test_LegendLite_Postgres_*Functions_PCT`), grouped as wrong answers, Postgres's own errors, leg P4's collections and Variant, and named refusals: 267 first measured, 242 after tier 1 (multi-column pivots, the self-referencing exists, half-even rounding to a scale, calendar buckets, whole-partition median/mode/quantile, maxBy/minBy, pow, ordered removeDuplicates), 230 after tier 2 (Variant over jsonb: key and index access, toVariant, parse, typed reads), 145 after tier 3 (lists of scalars as native arrays), 129 after tier 4 (structs and non-scalar lists as jsonb, held flat); its census divergence ceilings measured per suite (Essential 13, Grammar 5, the others 0; P3-09; 12 and 4 until 2026-10-11, when two more tests reached the wire with a struct as jsonb, the by-design class: `PctCensusGate.maxWireDiverge`) |
| `pct_channel_b` | 9 | `//pct:pct_channel_b` | Channel B dual-verdict suites, one target per class (`//pct:pct_channel_b_<suite>`; P3-09) |
| (heavy) | 9 | `//pct:pct_channel_b_<lane>` | Channel B on each other database (h2, postgres, sqlite, mariadb, mysql, databricks, snowflake, bigquery, trino, clickhouse), judged against that lane's own channel A ledger (2026-10-11, `docs/backends/cloud.md` §16); manual, in `//gates:heavy` |
| `parser_equivalence` | 8 | `//parser-equivalence:parser_parity`, `//parser-equivalence:round_trip_proof`, `//wasm:round_trip_test` | byte parity with legend-engine's parser; text to JSON to text to JSON over the corpus, lite's projects and the upstream showcase projects, in the JVM and in the tab (the WebAssembly build held to the JVM's answers; the protocol program's leg 5) |
| `stress` | 10 | `//core:stress_suites`, `//core:stress_suites_h2` | the stress corpus, on DuckDB (shared sessions, MIN_PASS) and on H2 (a fresh session per test, the engine's shape, `StressServiceSuitesH2Test.MIN_PASS`, `StressServiceSuitesTest.MIN_PASS_H2` until 2026-10-16; enforced since 2026-10-05, Bazel workplan P3-12); its measuring knobs are `bazel run //core:stress_tool` |
| `warehouse` (also Linux arm64) | — | `//warehouse:tests`, `//warehouse:sqlapi_wasm_build_test`, `//warehouse:tests_native`, `//warehouse:postgres_live`, `//warehouse:postgres_live_native` | the warehouse's suite on the JVM, and its SQL API compiled to WebAssembly (P3-24); the suite against the native binary, on Linux, macOS and Windows x64 (since 2026-10-02) and Linux arm64 (since 2026-10-04, P1-13), linked on Linux by the hermetic LLVM toolchain (P1-09); the launchers and their test went with the build rebuild's L1c (2026-10-07: the server runs as built, nothing of Bazel in it; the app as a plain folder is `//datacube:verify_app_test`'s, in `datacube`); the two tests against a live Postgres, in a lane for the first time on 2026-10-07. On Windows the lane runs in Eastern time, not the runners' UTC, so a test that reads the machine's zone shows (since 2026-10-03; the warehouse's reference session) |
| `datacube` | — | `//datacube:tests`, `//datacube:verify_app_test`, `//datacube:package_test`, `//datacube:verify_smoke_test`, `//wasm:differential_test`, `//wasm:zone_test` | DataCube's suite and typecheck, with every cube case live on the native warehouse and snapped into DuckDB-WASM, the answers compared (`live_snap_test`, plus receipts, sign-in again, token refresh); the single-user app against Postgres 16 in the pinned Chromium, on every platform (`verify_app_test`, since 2026-10-04, Bazel workplan P1-14b: its own Postgres from the pinned binaries, no psql); the smoke harness as a test on the pinned Chromium (since 2026-10-04, P1-14); the planner compiled to WebAssembly (TeaVM) and held to the JVM by differentials — its own corpus, DataCube's serialised cubes, the timezone database. Since L1b (2026-10-07) the other harnesses (features, page, charts, cubes, real data, upload, remote, wasm-browser, stress) are tests too, in `//datacube:browser`, run on Linux only in CI by their `ci-linux-only` tag |
| `ui` | — | `//studio:tests`, `//query:tests`, `//query-store:wasm_test`, `//query-store:share_test`, `//query-store:lite_test` | Studio's and Query's suites and typechecks; the saved-query store's one suite against the page's own store and against legend-lite's server (started by the test), and a saved query's share link round trip. Since L1b their browser harnesses are tests too (`//studio:verify_test`, `//query:verify_test`, `//site:verify_test`; Linux only in CI) |
| `sdlc` | — | `//sdlc-server:git_repository_test`, `//sdlc-client:tests`, `//depot-client:tests` | the model home (`docs/STUDIO_DESIGN_2026_10_02.md`): the SDLC and Depot suites, each run on the page's WebAssembly module and on the server over git |
| `heavy` (manual; no per-push lane) | — | `//spec:reference_lane`, `//spec:update_reference_lane_test`, `//spec:corpus_warehouse_verdict`, `//spec:manifest_world_census`, `//parser-equivalence:diagnostics`, `//pct:update_ratchets_test`, `//tools/python:requirements.test`, `//core:mongodb_plan_test` (the MongoDB v0 end to end on the embedded MongoDB 8.0, 2026-10-17; Linux x86_64) | the reference lane (about 8 GB), the warehouse corpus passes (they build the GraalVM server), the manifest-world census, the parser-equivalence diagnostics battery, PCT's ratchet re-measure, the Python requirements test: a landing runs the ones it needs by hand (`docs/build-inventory/program/START_HERE.md` §5); no scheduled run exists (the build rebuild's Test-11, never built). The full list, the other databases' PCT and channel B lanes among it, is `gates/BUILD.bazel`'s `heavy` |

**The Postgres lanes in a root container** (2026-10-11). The embedded Postgres refuses root (`initdb` does), so a
session that runs as root (a cloud container) runs a Postgres test as an unprivileged user, outside the sandbox:
`chmod o+x /root` once (other users may pass through the Bazel output base's parent, not list it), then
`bazel test //pct:pct_postgres --strategy=TestRunner=standalone --run_under="$PWD/tools/postgres/as_unprivileged.sh ubuntu"`
(any existing non-root user). The wrapper hands the test's own directories to the user and drops root with `setpriv`.
Measured: all five Postgres suites green as `ubuntu`. CI runs as a non-root user and needs none of this.

**`//gates:local` in a root container behind a TLS-intercepting proxy** (2026-10-12, measured at 5b05af7 in a
Claude Code cloud container). Three things differ from CI, and each has its own remedy:

1. *Postgres refuses root*: the recipe above, over the whole gate (every test then runs as `ubuntu`). With it alone,
   334 of 347 tests passed; the 13 that failed were all DataCube, Query and site tests over DuckDB-WASM or the pinned
   Chromium, not one of them a Postgres test.
2. *The proxy re-signs TLS, and a Bazel test inherits none of the variables that trust it.* DuckDB-WASM downloads its
   `json` and `parquet` extensions from `extensions.duckdb.org` when a test first reads JSON or Parquet. In Node it
   fetches them on a worker thread and waits on an `Atomics` flag that only a successful fetch sets. Without
   `NODE_EXTRA_CA_CERTS`, Node 22 refuses the proxy's certificate (`SELF_SIGNED_CERT_IN_CHAIN`, measured with the
   toolchain's node). The flag is never set, so the test hangs until its TIMEOUT. Chromium reads its trust store from
   `$HOME/.pki/nssdb`, and a test's `HOME` is its `TEST_TMPDIR`, which has none. Pass the bundle and a home whose NSS
   store trusts the proxy's CA (made once with `certutil -A -t "C,," ...` as that user):
   `--test_env=NODE_EXTRA_CA_CERTS=/root/.ccr/ca-bundle.crt --test_env=HOME=/home/ubuntu`. With these, 11 of the 13
   passed. **Since 2026-10-17 neither flag is needed:** a Node test starts DuckDB with engine-client's
   `startNodeDuckDb`, which answers its extension requests from the pinned files `node_test` gives it, so no test asks
   the network (docs/CLOUD_SESSIONS.md); the full local set passed in the container without them (the Postgres tests
   as `ubuntu`, item 1).
3. *Two tests write into their working directory*, the runfiles tree, which root owns: `//datacube:upload_test` and
   `//engine-client:model_data_test` (DuckDB-WASM's `COPY ... TO 'nested.json'`, `'tmp_made.parquet'`). As `ubuntu`
   they fail with `EACCES`. Run those two as root, without `--run_under`: both passed.

So all 347 tests passed in this container. The two hazards are the tests' own: an extension fetched from the
network at run time, and a scratch file written outside `TEST_TMPDIR`. CI doesn't see them because it has direct
internet and owns its runfiles tree.

4. *A container whose own `~/.bazelrc` passes the proxy into tests* (2026-10-14, measured at beebe47; the cloud lanes'
   containers carry `test --test_env=HTTPS_PROXY --test_env=https_proxy ...` for their JVMs). Chromium on Linux reads
   `https_proxy`, so a page sends `extensions.duckdb.org` to the proxy, which resolves it: the browser guard's
   resolver rule (`--host-resolver-rules=MAP extensions.duckdb.org ~NOTFOUND`, `tools/browser/pinned-chromium.mjs`)
   never sees the name, and `//datacube:browser_guard_test` fails ("Download is starting"). Run that test with the
   proxy cleared, `--test_env=HTTPS_PROXY= --test_env=https_proxy=`: both launches then meet `ERR_NAME_NOT_RESOLVED`,
   and it passes. A guard the proxy can step around in such a container; CI passes no proxy.

Beside the gates, in `bazel test //...`:

- **Generated files** — `//:generated` (CI's checks lane), that is `//core:update_generated_*_test`,
  `//core:update_stress_corpus_*_test`, `//core:update_ladder_*_test`, `//docs:update_generated_test`,
  `//parser-equivalence:update_generated_*_test`, `//datacube:update_generated_*_test`,
  `//query:update_generated_test`, `//fixtures/saved-queries:update_generated_*_test`: each committed generated file
  (Pure.java's signatures, DynaFn.java, CoreImports.java, the protocol's reachable classes,
  prelude.pure, native-claims.tsv, the fixture snapshot, the corpus manifest,
  the stress corpus, the SQL ladder's current pins, the protocol roster, DataCube's lite-facts.ts,
  Query's icons.ts, the saved-query fixtures) equals its generator's output.
  Regenerate: `bazel run //:update_generated`.
- **Source checks** — `//core:guardrails` (tests whose subject is core's own
  code: size and layer guardrails, shrink-only ratchets, ledgers; `@Tag("guardrail")`)
  and `//core:census` (the ones that walk other modules too; `@Tag("census")`).
  Out of `//core:core_tests`, a suite of one target per test package with no data: a behaviour test reads only
  its classpath (Bazel workplan P3-05).
- **Dependency guards** — `//tools/deps:core_closure_test` (core compiles
  against no jar; the drivers are exactly the named five: H2, DuckDB, SQLite,
  Postgres and its annotations jar, since 2026-10-03), and
  `//tools/guards:classpath_test` (no runtime classpath holds one Maven coordinate at two versions; it replaced
  `:pools_are_disjoint`, 2026-10-04).

Suites and manual targets: `//spec:judge_lanes` (all four judge lanes, the full corpus on both backends),
`//parser-equivalence:diagnostics` (the asserting grammar-coverage census),
`//parser-equivalence:diagnostics_reports` (the measurements, as cached report actions),
`//parser-equivalence:parse_speed_benchmark` (`bazel run`), `//spec:manifest_world_census`, `//core:heavy`, and three `bazel run` targets that write a DRAFT for a person to finish, never a
generated file: `//docs:draft_own_corpus_ledger` (the own-corpus ledger), `//core:draft_native_membership`
(native-membership.tsv from Pure.java's constants) and `//datacube:cut_link_dictionary` (the share link's next
dictionary version, frozen by its hash once cut). `bazel build //spec:native_declarations` lists every upstream
declaration of a membership FQN.

**Upstream.** The legend-engine / legend-pure release is pinned in MODULE.bazel
(the jars, and the source archives by sha256); tests read the sources as declared
inputs, so no gate can run against a missing or wrong checkout.
`bazel run //tools/bump -- <release>` moves it: pins, repin, regenerate, every
gate — then the judgement half (re-pin each moved ratchet with a reason).

**Reading a result.** `bazel-testlogs/<package>/<target>/test.log`, and the
test's written reports under `test.outputs/`. A corpus lane's passes are build actions: each one's log, ledger,
verdict and measured rosters are in `bazel-bin/spec/judge_host_<lane>/` and `judge_database_<lane>/`; a moved roster
is re-blessed by `bazel run //spec:update_rcorpus_<lane>` (Bazel workplan P2-15). A `(cached) PASSED` is a real
pass: Bazel re-runs a test whenever any of its inputs changed.

## Recording a moved pin, roster or ratchet

The guard files are load-bearing (`AGENTS.md`, its opening note): a ceiling, floor, pin, ratchet constant, roster,
register, golden or allowlist entry moves only on purpose, with a dated reason that names the task or the incident,
never to make a build green. Several sites send you here for that reason: `MinimalCorpusTest`'s roster and register
messages ("Every change to the roster file carries a written reason in docs/GATES.md"), `spec/BUILD.bazel`'s
reference lane and `ReferenceLaneReport` ("the GATES entry saying which lines moved and why"). This file no longer
holds one entry per landing (it did until the 2026-11 cleanup; the pointer at the end). The reason now goes in three
places:

1. **Beside what moved.** The dated comment `AGENTS.md` asks for, on the constant or the row itself. Example,
   `RejectionParityTest.MIN_PINS`: "424 -> 420 on 2026-09-10 (upstream boundary batch 1): ... a denominator move,
   not a parity loss".
2. **In the commit message.** What moved (old and new value, or the rows added and removed), why, and what was run
   to show it. This is the full record. `spec/corpus.bzl`'s roster diff test already asks for it there ("give the
   reason in the commit").
3. **One line in "Recent changes", the last section of this file.** Newest first: the date, what moved and why in a
   sentence, and the commit (or the PR, until the commit is known). The list keeps the 20 most recent lines; drop the
   oldest when you add one, since git keeps it.

How each kind is re-blessed once the move is understood:

| What moved | How |
|---|---|
| A corpus roster or register | `bazel run //spec:update_rcorpus_<lane>` (above, "Reading a result") |
| The reference lane | `bazel run //spec:update_reference_lane` (`docs/build-inventory/program/START_HERE.md` §5) |
| PCT's, spec's or parser-equivalence's ratchets | `bazel run //pct:update_ratchets`, `//spec:update_ratchets`, `//parser-equivalence:update_ratchets` |
| A committed generated file | its package's writer, or `bazel run //:update_generated`; the root one also rewrites the ratchets, the ladder and other measurements, so read the diff and give each moved line its own reason (START_HERE §4) |
| A constant in a guard's source (an ArchUnit register, a ledger, a size pin, a shrink-only ceiling) | edit it with its dated comment |

A guard that grows is a finding, not a pin to raise (START_HERE §6). A register row that no longer differs is a moved
pin too: delete it, with its reason (`MinimalCorpusTest`).

Sources: `AGENTS.md`; `spec/src/test/java/com/legend/rcorpus/MinimalCorpusTest.java`; `spec/BUILD.bazel`;
`spec/corpus.bzl`; `docs/build-inventory/program/START_HERE.md` §4–§6.

## Running and reading the gates: what the log taught

Each of these was learned on a real run, between 2026-08 and 2026-11, and still holds under Bazel.

**A timing is a lane run alone.** A timed claim is one lane run on its own with `--nocache_test_results`, the
one-minute load under 3 at the start, nothing else building (the rule between sessions, `docs/IN_FLIGHT.md`). Any
other number is labelled contended. A lane's time inside a chain or the local gate is a scheduling artefact, never a
timing: on 2026-09-27 the lanes of a cold `//...` still ran two to three times their solo time after the memory fix
below (three to seven times before it), and those inflated figures were read as regressions twice. On a macOS desk,
keep the machine awake (`caffeinate -dims`; plain `-i` is not enough once the machine is in its sleep cycle). A lane
near 900 s with near-zero CPU means the machine slept; a first timing of 722 s measured 34 s awake. Treat an outlier
as suspect before treating it as data.

**Local scheduling by declared memory.** Bazel estimates a test's memory from its `size` (an `enormous` test is
assumed to need 800 MB), while our JVM lanes peak at 0.7 to 8.2 GB; with one test per core it ran up to ten large
JVMs at once on a 10-core, 32 GB desk (measured 2026-09-27). So every JVM test declares one number, `memory_mb`
(`tools/junit/defs.bzl`): it is both the JVM's `-Xmx` and the scheduler's `resources:memory:<MB>` tag. It is set
from the measured live peak plus half again, at least 512 MB, rounded up to 256 MB; a target sized otherwise says why
beside it, and each run prints its live peak ("[bazel] heap: live peak ..."). The desk's budget is `.bazelrc`'s
`test --local_resources=memory=HOST_RAM*.6`: 60% of RAM for tests, the rest for the Bazel server, the OS and anything
else on the desk. Bazel then packs tests by memory and fills the free cores with the cheap ones. Measured cold from
`bazel clean`, 2026-09-27, with the diagnostics battery also taken out of the chain: `//...` went from 10m37s to
7m11s. The 7 GB macOS runner runs one test JVM at a time instead (`build:ci-small --local_test_jobs=1`; `gate.yml` passes `low-memory: true` there).

**The shared machine.** At most two heavy Bazel or JVM jobs at once on one desk. Before a heavy run, check for other
sessions' Bazel servers and test JVMs (`ps`), and do not stack worktrees or agents that run Bazel. A browser test such
as `//studio:verify_test` times out under two full chains, and the local gate's misses on a loaded machine have been
timeouts that passed alone: rerun such a target alone before reading it as a failure. (`docs/IN_FLIGHT.md`;
`gates/BUILD.bazel`'s `local`; START_HERE §4.)

**One architecture's last ULP.** CI runs Linux and Windows on x86_64 and macOS on arm64 because a single-platform
pipeline cannot see this class of difference (`gate.yml`'s header). macOS is the reference configuration: the
rosters and goldens were minted on arm64. Two cases were found:
- DuckDB's own `percentile_cont`/`quantile_cont` answered one ULP apart from the same driver binary (2026-09-09, the
  first CI runs: 44.6 on arm64, 44.599999999999994 on x86_64, over 22, 27, 38, 44, 45 at p = 0.9). Its compiled
  interpolation takes an FMA contraction on arm64. Today's DuckDB dialect spells the interpolation itself in plain SQL
  arithmetic, which is bit-identical on both and the correctly rounded value (today's Java
  `core/src/main/java/com/legend/sql/dialect/QuantileOrder.java`).
- DuckDB's `cbrt` calls the C library's cube root, which differs by one ULP between the architectures (2026-09-17,
  the stress corpus's combination battery). H2 uses Java's fdlibm and agrees everywhere.

The judges count two ULP of the larger magnitude as equal for finite doubles; that policy is register row S15. It
pairs two cells only when one of them is spelled as a floating number: two integral spellings still compare as text
(2026-09-20, the epoch-millis lesson).

**What a sweep cannot see.** A corpus sweep cannot find a disagreement about a form the corpus never contains. So
`FixtureAdjudicationTest` is the one parser-equivalence tier pointed at our own fixtures rather than upstream's files;
that is how three leniencies once survived for months, pinned by our own tests. `AdversarialParityTest` covers the
other blind spot: byte parity says nothing where the reference refuses.

**A refactor's oracle.** When a change should move no answer, the corpus judge passes rebuilt on its jars write
reports identical to the last run's, so the roster checks are cache hits and only the verdicts rerun: zero roster
movement is the proof (the 2026-10-30 package moves). A compiler change instead compares the six passes with a
baseline built before the change (START_HERE §5, "The corpus passes, done right").

**The workflows.** A workflow whose own file is invalid never starts: GitHub runs zero jobs and names the failed run
after the file path (the first `gate.yml`, 2026-09-09: a job-level `if` that read `matrix`). The pinned actionlint
runs as `//tools/guards:workflows_test` in the `checks` lane; run it before pushing a workflow change, since CI cannot
lint a file it cannot start.

**What no CI run covers.** `//gates:heavy` is manual. The MariaDB and MySQL PCT lanes and their channel B twins run
nowhere unless run by hand (corrected 2026-10-29: earlier records said "the MariaDB lane (CI alone)"); the cloud lanes
need an account's credentials. A landing's record says which of them it did not measure.

**Windows is slower inside a lane.** On the Windows runner a JVM suite ran two to three times slower inside its lane
than alone: CPU contention beside the lane's other test JVMs, with no I/O cost (diagnosed 2026-10-15, PR #44, when
`//core:core_tests_integration` hit its 300 s limit there while its Linux time fell). A Windows timeout is first a
question about the lane's margin.

**In a cloud container** (root, a TLS proxy, a disk allowance: the recipes are `docs/CLOUD_SESSIONS.md` and this
file's head):
- `//python:engine_test` dies with a segmentation fault at the headless host's V8 before its body; CI judges it.
- A run that fills the disk can leave a cached failure that looks like a result. On 2026-11-01 the DuckDB database
  judge's pass, unable to unpack DuckDB's native library, was cached as UNMEASURED. Delete that pass's outputs and run
  it again.
- The whole local gate run as the unprivileged user once filled the disk allowance (386 failures, every one "No space
  left on device"). Run it as root, with only the Postgres targets unprivileged.

Sources: this file's log entries of 2026-08-10, 2026-08-11, 2026-09-09, 2026-09-17, 2026-09-20, 2026-09-27,
2026-10-15, 2026-10-28, 2026-10-29, 2026-10-30 and 2026-11-01 (git history, the pointer at the end); `.bazelrc`;
`tools/junit/defs.bzl`; `.github/workflows/gate.yml`; `QuantileOrder.java`; `docs/SEMANTICS_REGISTER.md` S15.

## The CI lanes and why

**What runs** (`.github/workflows/gate.yml`, `gates-run.yml`, `tools/ci/lane-flags.sh`, `gates/BUILD.bazel`):

| | |
|---|---|
| When | every push to `main` and every pull request, unless only `**/*.md` or `progress*.txt` changed (so a documentation-only PR gets no `gates green`: the owner merges it by bypass, `AGENTS.md`); every night on `main` at 06:23 UTC, which is main's own verdict for a PR merged with `[skip ci]` and refreshes the downloads cache that branch runs start from (a branch run can read main's cache, not the reverse); by dispatch, with `gates=` (lane keys), `platforms=` and `linux_only_tests=` |
| Where | Linux `ubuntu-latest`, macOS `macos-15` (arm64; `macos-14` until 2026-11-02, when GitHub retired it) and Windows `windows-2022` run every lane; Linux arm64 `ubuntu-24.04-arm` runs `warehouse` only. The platforms are independent jobs, so the wall clock is the slowest one |
| What | 15 lane keys: `product`, a `bazel build`, and `core`, `checks`, `corpus_duckdb`, `corpus_h2`, `pct_duckdb`, `pct_h2`, `pct_postgres`, `pct_channel_b`, `parser_equivalence`, `stress`, `warehouse`, `datacube`, `ui`, `sdlc`, each `bazel test //gates:<key>`. A lane's members are listed in `gates/BUILD.bazel` and nowhere else, so a new test goes into a suite, never into a script |
| The product job | the compile targets (`//:java //:web //:wasm //:native //:sites //datacube:app`); then everything analysed under Bazel 10's incompatible flags; then A25 (the native packages analysed for a platform none of them supports); then the lane guard: every test of the repository is in a lane or in `heavy` |
| Flags | `--config=ci` and the repository cache on every command; `--config=ci-small` (one test JVM at a time) on the 3-vCPU, 7 GB macOS runner; off Linux, `--test_tag_filters=-ci-linux-only --build_tests_only`; CI runs every test every time (`test:ci --nocache_test_results`) |
| The verdict | `gates green`, the one required check: it fails unless every platform's jobs succeeded, and does not run on a dispatch, which may pick a subset. Only a PR's superseded run is cancelled; every commit on `main` gets a verdict |
| Windows | the `warehouse`, `datacube` and `ui` lanes run in Eastern time, not UTC, so a test that reads the machine's zone shows |
| Beside it | `diagnostics.yml` runs `//parser-equivalence:diagnostics` when `MODULE.bazel`, `release.MODULE.bazel`, `parser-equivalence/` or core's parser, lexer or protocol packages change |

**Why it has this shape.** Measured on 2026-10-07, before and after the build rebuild's CI landing, L1:

- **Before L1 a full run took 40 to 57 minutes.** The build lane ran `bazel build //...`, which also computed the six
  corpus passes (the corpus lanes then computed them again) and the 2 GB `//pct:ratchets`: 29, 25 and 42 minutes on
  Linux, macOS and Windows. The browser lane ran its harnesses one at a time from a shell loop on fixed ports
  (23 minutes; `verify_features` alone 4m36s). Every Linux and Windows job fetched everything cold: a lane spent
  3 to 5 minutes on checkout, Bazel start, analysis and fetching before its first test. So the product job builds
  the product and nothing else (2.2 minutes of build on Linux in L1a's run); every lane is one `//gates` suite, with
  the same name, command and members on every platform; the harnesses are Bazel tests; the heavy and hand targets are
  `manual`; and the guard makes sure no test is left in no lane (there is no "misc").
- **GitHub keeps 10 GB of Actions cache per repository.** A cache of build outputs per lane crowded every other entry
  out: on 2026-10-07 the three entries left were all macOS's, and Linux and Windows started cold on every run. The
  cache now holds the downloads only, `~/.cache/bazel-repo/content_addressable/`, one entry per platform keyed
  `bazel-repo-<platform>=<hash of the pins>`. Three fixes, each found by a run: the key leaves out the runner image's
  version (GitHub rotates images from run to run, so such a key almost never matched twice); `=` separates the platform
  from the hash, so `linux` cannot prefix-match `linux-arm` (Linux once restored the arm64 downloads); and the product
  job saves right after the product build, before its analysis of `//...` fetches every test pool's jars (the Linux
  entry had been 19 GB). Saving the whole folder also saved Bazel 9's unpacked repositories (`contents/`, 13 of
  Linux's 16 GB), and a negated pattern in the cache's `path` did not leave them out (actions/cache matches entries,
  not trees), so the path names `content_addressable/` itself. After the fix the four entries were 2.99 GB (Linux),
  2.64 GB (Linux arm64), 1.02 GB (Windows) and 0.99 GB (macOS): 7.6 GB together. Only the product job saves (and the
  linux-arm lane, which has no product job): actions/cache never adds to an existing key.
- **GitHub runs at most five macOS jobs at once.** On macOS the wall clock is the queue, roughly the lanes' minutes
  added up and divided by five (in one run macOS's last job started 27 minutes after its first). Every extra macOS lane
  costs its floor of about two minutes, divided by five.
- **The browser harnesses run on Linux only in CI**, by their `ci-linux-only` tag, as a cost policy and not a
  platform limit. Run everywhere on 2026-10-07, all of them passed on macOS and Windows, at a cost of about seven
  minutes of wall clock, all of it macOS's `datacube` lane. A desk runs them on any platform; a dispatch with
  `linux_only_tests=everywhere` runs them everywhere.
- **The remote cache** (BuildBuddy, since L1e, 2026-10-07) serves every lane the build part: the native images, the
  WebAssembly planner, the bundles, the jars (450 to 815 remote cache hits per job), which each lane had built again
  on its own runner. The product job went from 14 / 8 / 14 to 12 / 3 / 9 minutes (Linux / macOS / Windows), the
  corpus lanes from 9 / 6 / 8 to 5 / 3 / 3, `checks` from 11 / 8 / 15 to 7 / 6 / 10. The test-bound lanes (PCT,
  stress, `datacube`) did not move, because every test runs every time: with a remote cache Bazel would otherwise
  serve cached test results. Only CI writes to the cache (`.bazelrc`'s `remote` config uploads nothing; CI's own rc
  turns uploads on); the event upload is fully asynchronous, so an outage can never turn a build red; without the
  secret (a fork, a desk) Bazel builds as before.
- **The result.** L1a's first run, cold, took 30.6 minutes; L1c's took 26 minutes to the first verdict; with the
  remote cache warm, 22 minutes (38 with the harnesses run everywhere).
- **The warehouse lane builds `//native:compiler` in a step of its own**, then the lane's targets, then the tests
  (2026-10-08): a test beside a native-image build timed out (`tests_native`), and two images at once risk a 7 GB
  runner's memory.

The rest of that brief (about 80 build items: Node, the generators outside the bump, tests by their true trigger, the
renames, the final audit) was parked with the build rebuild and is in git history.

Sources: `docs/build-inventory/program/PHASE_8.md` §8; `docs/build-inventory/program/evidence/phase8/CI_LANES_2026_10_07.md`
§1–§11 (git history: docs/HISTORY.md); `.github/workflows/gate.yml`, `gates-run.yml`; `tools/ci/lane-flags.sh`;
`.bazelrc`; `gates/BUILD.bazel`.

## Recent changes

The dated record of moved pins, rosters and ratchets (the rules: "Recording a moved pin, roster or ratchet", above).
Newest first, at most 20 lines. Seeded from the last entries of the cut log; "D<n>" is the decisions log of the
landing's program.

- 2026-11-09 — ruling 18 (`44012955b`): `rust/oracles/front/deliberate-differences.tsv` 9 -> 744 asks; `FROZEN_MODULE_DIFFERS` 1 -> 66 inputs, now `wasm/frozen_module_differs.json`; `ModelReaderParityTest` own 37739 -> 37648 and documents 13514 -> 13512, `JsonModelDoorTest` 7085 -> 7084, `RoundTripProofTest` 6905 -> 6904 and upstream 270 -> 271 (`relationMappingSetup.pure`, which the pinned engine refuses); `LENIENT_RESIDUE` 13 -> 6; `docs/refusal-allowlist.tsv` and `REFUSAL_ALLOW_TOTAL` 4 -> 3; `OwnDialectCensusTest` `ModelNormalizerTest.java` 0 -> 1.
- 2026-11-09 — `rust/oracles/front/deliberate-differences.tsv` 7 -> 9 asks (`1680d307` at `pmcd`/`LEGEND_ENGINE` and `document`,
  row 73: an `AwsGlue` island with its `;` left off, now at the engine's own place, ruling 16); no floor lowered, no other
  list or pin moved (docs/rust-port/PHASE2.md §6, rulings 15 and 16). Commit `91de44e54`. Branch `worktree-rust-port-phase0`.
- 2026-11-09 — `rust/oracles/front/deliberate-differences.tsv` 5 -> 7 asks (`00dc41c7` at `pmcd`/`LEGEND_ENGINE`, ruling 13;
  `672e812e` at `spec`/`LEGEND_ENGINE`, ruling 11, from `raw-left-for-user.tsv`, deleted with its readers); the front
  differentials fail on any `java.*` answer over the frozen questions, which stay Java's; no floor lowered
  (docs/rust-port/PHASE2.md §6, rulings 11 to 13). `OwnDialectCensusTest.EXTENSION_HOST_PINS`' `ProbeWireShapes.java` 1 -> 2:
  the instrument's probe the engine refuses is refused at `LEGEND_ENGINE` now. Commit `1c9920466`. Branch
  `worktree-rust-port-phase0`.
- 2026-11-09 — `rust/oracles/front/deliberate-differences.tsv` (5 asks: `109b58da`'s `pmcd` at three dialects and `document`,
  `00dc41c7`'s `document`) and `raw-left-for-user.tsv` (`672e812e`) are new: the front differentials and `//core:front_shim`
  hold identical + listed == total both ways, and fail on any other `java.*` answer over the frozen questions, which stay
  Java's; no floor lowered (docs/rust-port/PHASE2.md §6, ruling 4). Commit `f5175c9c3`. Branch `worktree-rust-port-phase0`.
- 2026-11-09 — `wasm/round_trip.mjs` gains `FROZEN_MODULE_DIFFERS`, one input (`TestProfile.java#52`): the JVM's parser refuses
  it with a placed parse error (docs/rust-port/PHASE2.md §6, ruling 4; PARITY row 9) and the frozen `//wasm:planner`, TeaVM's
  last build of the Java parser, keeps the raw answer. Held both ways; it goes with the Rust wasm32 build. Branch
  `worktree-rust-port-phase0`.
- 2026-11-09 — `ArchitectureTest.parseMachineryIsUsedOnlyWhereSanctioned` (6i) pins `PmcdParser` beside `SpecParser`
  (docs/rust-port/PHASE2.md §6, ruling 2: both text doors pinned to the parse-entry packages), with the three classes
  outside those packages that are parse entries by role, the only dependents the widened rule found, named:
  `door.ModelFromText`, `ffi.FrontReader`, `ffi.FrontRecords`. Branch `worktree-rust-port-phase0`.
- 2026-11-06 — The Rust port deletes the Java lexer and parser (phase 2 step 6b): `ArchitectureTest` retires 7a
  (`lexerDependsOnNothingButTheJdk`) and 6f (`lexerIsPrivateToTheParser`), its lists lose `lexer` and `ide`, 6i pins
  `SpecParser` alone and 7c's surface loses `lexer`; `DialectBoundaryTest.DATABASE_NAME_DECISIONS` empties (the
  grammars' 12 + 1 + 1 rows, deleted or re-measured 0); the parse tap's flag, `QuotedSpecParser`'s catch, the deleted
  cursors' allowlist rows, `DropInSurfaceTextRuleTest`'s source re-read rows, `ParserBoundaryArchTest`'s
  `ModelOrchestrator` and `FrontOracle`, `OwnDialectCensusTest`'s `LexerTest` host leave; `tools/deps/core-layers.txt`
  loses `lexer` and `ide`. With the retired tests' 46 own-corpus snippets, parser-equivalence's `own_corpus.matched`
  ratchet goes 3009 -> 2943 (a denominator move; diffs 0) and `OwnCorpusConformanceTest`'s ledger loses
  `PURE-DIALECT-diagram` (its one row was `LexerTest`'s). Branch `worktree-rust-port-phase0`.
- 2026-11-06 — `CodeShapeGuardrailTest.MUTABLE_FIELD_ALLOWLIST` gains `FrontRead.pos` and `FrontRead.depth`: the Rust
  port's shim reads the parser's answer through a pull cursor instead of a `Json` tree (which overflowed
  `//core:corpus_differential_test`'s 768 MB), and a cursor's position is its state, as `NonFiniteJson.at`'s and
  `Json.i`'s are. Branch `worktree-rust-port-phase0`.
- 2026-11-06 — The Rust port's model door switches (phase 2 step 4): `ParserBoundaryArchTest.DIALECT_CLASSES` gains
  the front oracle's `FrontEngineAnswers` (step 5's oracle, at the engine's dialect); `core`'s `not_layers` gains
  `core_next_rust_parser` (the claims generator's core carries the parser provider). Branch `worktree-rust-port-phase0`.
- 2026-11-05 — The Rust port's expression doors switch (phase 2 step 3): `ParserBoundaryArchTest.DIALECT_CLASSES` gains
  `RustParserProvider` and the front oracle's four classes (they pass on or re-ask a caller's dialect);
  `DatabaseOwners`' vocabulary gains the shim's reader (`ffi/FrontRe*`) and `frontoracle/` (they name every record
  as the protocol does); `tools/deps/core-layers.txt` gains `rust_parser`, `core`'s `not_layers` `front_codegen` (test
  code). Branch `worktree-rust-port-phase0`.
- 2026-11-05 — `ArchitectureTest.callsAreBuiltOnlyByTheBuilder` and `protocolNodesAreConstructedOnlyByTheParserAndTheNormalizer`
  exempt `com.legend.ffi.FrontRecords`, the generated reader that reads the Rust parser's answer into the records the
  Java parser builds (a protocol reader: it builds before resolution). Invariant 12 is untouched: the reader builds no
  opaque record. Branch `worktree-rust-port-phase0`.
- 2026-11-05 — `ObservabilityGuardrailTest.ENV_FLAGS` gains `LEGEND_PARSE_TAP`: the Rust port's phase 2 parse tap
  records the parser's questions for the front oracle; the parser may not reach a switchboard (invariant 7c), and the
  flag retires with the Java parser. Branch `worktree-rust-port-phase0`.
- 2026-11-05 — The Rust port's phase 1 (all sixteen dialects render in Rust behind Java shims): the guards that named
  Java `sql/dialect` classes moved with the code (allowlists, size pins, the scan-order owners, `CompileOnlyTest`'s
  TeaVM tier) and seven Java dialect test files retired, each a dated row of `rust/RETIRED_TESTS.md`; `//python:wheel_test`'s glibc
  ceiling now follows the tag, `manylinux_2_25` (the user's ruling, 2026-11-04). Branch `worktree-rust-port-phase0`.
- 2026-11-02 — The macOS lane moved from the `macos-14` image to `macos-15` (both arm64): GitHub retired `macos-14`
  on 2026-11-02 and had stopped assigning runners to it. PR #85.
- 2026-11-01 — The product path's overhead taken out, every output byte-identical; `TenetRatchetTest`'s ceiling 21 → 9,
  the ninth site named (the direct timestamp read; the user's yes; the ceiling had stayed at 21 after the legacy
  executor's sites went on 2026-10-27). Branch `claude/product-path-perf` from `efff0406`, PR #85.
- 2026-10-30 — Step C, the root dissolved (D73): the ledger's `ROOT_CLASSES` is the five doors, `PLANNER_CLASSES` (7)
  and `DRIVER_CLASSES` (26) new; two new rules (`theDriverHasOneCaller`, `plannerIsExecutionFree`); size pins
  AssertVerdicts 1258 → 1259 and BodyDriver 2313 → 2321 (the import lines the move needs). PR #82, `caa4bb8c2`.
- 2026-10-30 — The seam's step B2, the wall (D72): `ArchitectureTest.EXEC_PLANNING_REFERENCES` deleted,
  `execReachesNoPlanning` the rule; GridProbe's size pin 53 → 52; `EXEC_CLASSES` −3, `JUDGE_CLASSES` +2,
  `ROOT_CLASSES` +1. PR #82, `9f44d43c`.
- 2026-10-30 — The seam's step B1, the judge layer out of exec (D71): `JUDGE_CLASSES` (14) new, `EXEC_CLASSES` −16,
  `ROOT_CLASSES` +4; size pins GridProbe 48 → 53, DynamicPivot 113 → 118, Equality 447 → 448 (import lines). PR #82, `7a6c899e`.
- 2026-10-30 — The seam's step A, one way into the session seam (D70): the JDBC census register loses
  `PlanSessions`, `SystemDatabase`, `SessionState` and `ConnectionResolver` (18 rows); the static-state register
  loses `SessionState.OF`. PR #82, `fd060846`.
- 2026-10-30 — The java.sql revamp, step 3 (D69): the JDBC surface register as a table of classes and their acts; the
  funnel's exemption; the new rule `noJdbcTypeInAPublicSignatureOutsideTheSeam`. `dfbbcfa5`.

> Cut in the 2026-11 docs cleanup: the Maven-era chain and its log (2026-08-02 to 2026-09-22: `tools/allgates.sh`, the
> root-flag trap, the budget decisions, the batch chain logs, the hand-kept ratchet table, the CI leg, fork policy and
> build-once notes) and about 400 per-landing entries (2026-09-10 to 2026-11-01). Full text:
> `git show de2745351:docs/GATES.md`.
