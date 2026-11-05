# Legend Lite

A **clean-sheet re-implementation** of the [FINOS Legend](https://legend.finos.org/) platform: the Pure language
and engine of [legend-pure](https://github.com/finos/legend-pure) and
[legend-engine](https://github.com/finos/legend-engine), designed and written anew with those projects as its
specification, and the apps of [legend-studio](https://github.com/finos/legend-studio) (credited in
[`NOTICE`](NOTICE); Apache-2.0, [`LICENSE`](LICENSE)). Legend Lite compiles Pure models and queries to SQL, and
the database executes them: the database computes every value, and the runtime only streams the answer's typed
rows to a writer.

Around the compiler sit browser apps rebuilt light (Studio, Query and DataCube), a DuckDB warehouse behind an
HTTP SQL API, and SDLC-lite and Depot-lite, which serve legend-sdlc's and legend-depot's APIs. The apps speak
upstream's own APIs (Studio adds lite's text routes to the SDLC's), so a client written for legend-engine runs
against legend-lite unchanged.

## Status: Java today, Rust under way

The **Java implementation is the running system**. It is being **ported to Rust** on branch `rust-port`. The
plan is [`docs/rust-port/ROADMAP.md`](docs/rust-port/ROADMAP.md), and the crates are under `rust/`
([`rust/README.md`](rust/README.md)).

- **Iteration 1 replaces the planning path with Rust** and removes GraalVM from the compiler's native library.
  Java keeps the executor, the HTTP shell, the test runners, and the warehouse, SDLC, depot and query-store
  modules.
- **Where it stands (2026-11-04).** Phase 0, the groundwork, was done on 2026-11-02
  ([`PHASE0.md`](docs/rust-port/PHASE0.md)). Phase 1, the dialects, is under way: since 2026-11-03 DuckDB,
  H2 (both profiles) and the three dialects that print legend-engine's SQL text render in Rust, behind Java shims
  that keep their class names. The other ten dialects are still Java
  ([`PHASE1.md`](docs/rust-port/PHASE1.md) §4). The browser planner is frozen as the module TeaVM last built
  (`wasm/planner/`, `sdlc-server/page/`) until the Rust WebAssembly build replaces it (§5).
- **How a phase lands.** Every test target that passed on the pristine tree (`rust/BASELINE.tsv`) still passes,
  or is a dated row of `rust/RETIRED_TESTS.md`.

**Start here:** [`AGENTS.md`](AGENTS.md) for the invariants and the working rules,
[`docs/architecture/README.md`](docs/architecture/README.md) for the design,
[`core/README.md`](core/README.md) for the per-package spec, and [`docs/GATES.md`](docs/GATES.md) for what must
be green. The full map is [below](#documentation-map).

---

## Architecture

The compiler is `core/src/main/java/com/legend/`. Five classes in its root package are its doors. `Compiler`,
`TypedQuery` and `HeldModels` plan: phases A–J, with no database needed. `Execution` and `ExecuteOptions` run a
plan: phase K. Every phase is the same method its own unit tests exercise; there is no orchestrator-only code
path.

```
text                                                             [FRONTEND]
  A  lexer/            Lexer.tokenize                 text → TokenStream
  B  parser/, door/    PmcdParser.parse, ModelFromText tokens → protocol records → ParsedModel
  C  parser/           SpecParser.parse               tokens → ValueSpecification
  D  compiler/         NameResolver.resolve           simple name → FQN
  E  normalizer/       ModelNormalizer.normalize      ParsedModel → NormalizedModel
  F  compiler/element/ PureModelContext.from          → TypedElement + ModelContext
  G  compiler/spec/    SpecCompiler                   spec + model → TypedSpec
  G½ compiler/spec/    UserCallInliner.inlineBody     β-inline user calls
                                                                 [MIDEND]
  H  resolver/         StoreResolver.resolve          logical → physical TypedSpec
  I  lowering/         Lowerer.lower                  TypedSpec → sql.SqlQuery
                                                                 [BACKEND]
  J  sql/dialect/      SqlDialect.render              SqlQuery → SQL string
                                                                 [RUNTIME]
  K  exec/             PlanRunner.stream              a plan + a session → typed rows to a sink
```

**Phases A–F are model loading**, once per source. **G–K are query execution**, once per query. `ModelContext`
is the boundary: built once, reused across queries.

Two things worth knowing before reading the code:

- **The parser is hand-written recursive descent.** legend-lite's build has no ANTLR: the ANTLR runtime appears
  only inside the upstream legend-engine jars that the parser-equivalence tests and the engine runner load as
  the oracle. The parser is a byte-for-byte drop-in candidate for legend-engine's ANTLR parser: 22,725/22,725
  elements emit identical protocol JSON (gate 8; the parser drop-in study is in `docs/HISTORY.md`).
- **Errors carry a phase, not a letter.** `error/LegendCompileException.Phase` is
  `PARSE, RESOLVE, NORMALIZE, MODEL, TYPE, MAPPING, LOWER, RENDER, EXECUTE`.

### Architectural invariants

The full list, with honest per-rule enforcement status, is in [`AGENTS.md`](AGENTS.md); the ones the Rust port
keeps are in [`docs/architecture/README.md`](docs/architecture/README.md). The four that matter most:

1. **The frontend does ALL typing** — single source of truth. If a type is
   missing downstream, fix the frontend.
2. **The Lowerer does NO type inference** — it reads typed HIR for structure
   and never infers. No SQL syntax, no SQL function names, no dialect import.
3. **The dialect owns ALL SQL rendering.** Core has exactly one entry point:
   `SqlDialect.render(SqlQuery)`. The MIR under `sql/` is sealed, pure data —
   no `toSql()`, no `String` field encoding a SQL operation.
4. **No fallbacks, no defaulting** — fail loudly. Every default branch is a
   hidden bug.

---

## What is Legend?

[Legend](https://legend.finos.org/) is an open-source data management platform created by Goldman Sachs and donated to FINOS. Define your data model once in Pure, map it to databases, query it with type-safe semantics.

Legend Lite reimplements it with every query pushed down to the database.

| Dimension | Legend Engine (FINOS) | Legend Lite |
|-----------|----------------------|-------------|
| **Execution** | Mixed: some SQL, some in-memory Java | the database computes every value; a model-to-model query runs on an in-process DuckDB, its data inline in the SQL (register row S27 of [`docs/SEMANTICS_REGISTER.md`](docs/SEMANTICS_REGISTER.md)) |
| **Parser** | ANTLR4 (156 `.g4`, 14,568 generated lines) | hand-written recursive descent |
| **Dependencies** | Hundreds of JARs | `//core:core` compiles against the JDK alone; at run time it is given the DuckDB, H2, Postgres, SQLite and MongoDB drivers (`tools/deps/CoreClosureTest.java`) |
| **Java version** | Java 11 | Java 21 language level, built and run on JDK 25 (`.bazelrc`) |
| **Databases** | Postgres, Databricks, Snowflake, etc. | 16 dialect classes (counted 2026-11-04): DuckDB, H2 (2.1/2.2 and 2.3+), Postgres, SQLite, MariaDB, MySQL, Trino, ClickHouse, Snowflake, Databricks, BigQuery, MongoDB, and three that print legend-engine's own SQL text |

What each dialect does with every function and aggregate is generated from the dialects' own tables:
[`docs/dialect-capability/README.md`](docs/dialect-capability/README.md) (with `functions.md` and
`aggregates.md`). Which Pure is covered, and how well, is measured by the gates: legend-engine's PCT suites, the
relational corpus and parser equivalence ([`docs/GATES.md`](docs/GATES.md)).

> Cut in the 2026-11 docs cleanup: the hand-maintained Feature Set tables (Pure language support, Relation API
> operations, aggregate, scalar and window functions, graph fetch, association navigation, M2M; 2026-08).
> Full text: `git show de2745351:README.md`.

---

## Modules

Every top-level directory, one line each.

| Directory | What it is |
|---|---|
| `core/` | The compiler, planner, executor and HTTP server shell, in Java ([`core/README.md`](core/README.md)). `src/main/java` held 976 Java files and 353,551 lines on 2026-11-04 (`find`, `wc -l`). |
| `rust/` | The Rust port's crates (`lite-ffi`, `lite-javacompat`, `lite-json`, `lite-sql`, `lite-dialect`) and the frozen JVM answers they are judged against ([`rust/README.md`](rust/README.md)). |
| `base/` | The null annotations every Java module carries. Depends on nothing. |
| `json/` | legend-lite's strict JSON codec (`com.legend.json.Json`), used by core and the warehouse. |
| `arrow_ipc/` | The Arrow IPC framer that the warehouse's SQL API and core's Arrow writer share. No Arrow library. |
| `testing/` | Test support shared by every module's tests: runfiles, and the embedded Postgres, MariaDB, MySQL, Trino, ClickHouse and MongoDB. |
| `spec/` | The tests that read the upstream sources as the spec: the generators' byte parity and the relational corpus lanes. |
| `pct/` | legend-engine's PCT suites (its own function tests, in Pure) run through legend-lite. |
| `parser-equivalence/` | legend-lite's parser and legend-engine's over the same input, compared byte for byte. |
| `projects/` | The Legend model projects, each compiled alone with its declared dependencies ([`projects/CONTRACT.md`](projects/CONTRACT.md)). |
| `fixtures/` | The saved-query fixture that Query writes and DataCube reads. |
| `studio/` | Legend Studio rebuilt light: write models as text, compiled as you type; save, review, commit and release. Runs with no server ([`studio/README.md`](studio/README.md)). |
| `query/` | Legend Query rebuilt on legend-lite: find data, build a query, run it, save it ([`query/README.md`](query/README.md)). |
| `datacube/` | DataCube, the browser app over legend-lite's planner (TypeScript). |
| `engine-client/` | Where a query runs, for every app: one `QueryEngine` interface, a server's or the page's own ([`engine-client/README.md`](engine-client/README.md)). |
| `pure-protocol/` | Pure queries as legend-engine's V1 lambda JSON, in plain TypeScript. |
| `legend-art/` | The look the apps share (colour tokens, icons, type), modelled on upstream's `@finos/legend-art`. |
| `model-changes/` | Making a class, an association or a data space in any project (TypeScript). |
| `query-store/` | Upstream's saved queries (`/api/pure/v1/query`): one client, and the store answered in the page. |
| `sdlc-server/`, `sdlc-client/` | SDLC-lite: legend-sdlc's REST API, its rules written once in Java and run on a server over git or in the page as WebAssembly; and the TypeScript client. |
| `depot-server/`, `depot-client/` | Depot-lite: legend-depot's read API, the same way; and its client. |
| `warehouse/` | The warehouse: a DuckDB server behind an HTTP SQL API, on the JVM and as a GraalVM native image. |
| `native/` | The compiler as a native shared library (GraalVM's `libcompiler`), for Python. ROADMAP phase 4 replaces it with Rust. |
| `python/` | `legend_lite`: the compiler in Python, over the native library. |
| `wasm/` | The planner compiled to WebAssembly by TeaVM, which DataCube, Query and Studio plan on in the browser. Frozen since 2026-11-03 until the Rust build replaces it (PHASE1.md §5). |
| `site/` | Every app from one origin, as a folder for any static host. |
| `gates/` | The CI lanes as test suites, and `//gates:local`. |
| `tools/` | The build's own tooling: Bazel macros, guards, dependency pins and guards, generators, `bump`, the oracles. |
| `third_party/` | Build files for the upstream source archives, and the patches Bazel applies to third-party modules. |
| `scripts/` | Python behind the stress corpus, the parser fixtures and the project checks. |
| `experiments/` | `backend-probes/`, the raw evidence behind the backend docs (moving to `docs/backends/probes/`). |
| `docs/` | Design and process documents ([map below](#documentation-map)). |

> Cut in the 2026-11 docs cleanup: the Package Structure block (2026-08-06 file and line counts per core package)
> and the two notes on the frozen legacy `engine/` module, which was deleted on 2026-08-11. Full text:
> `git show de2745351:README.md`.

---

## Quick Start

### Prerequisites

- **[Bazelisk](https://github.com/bazelbuild/bazelisk)** (as `bazel`) — it runs the
  Bazel release `.bazelversion` pins. Bazel fetches the JDK, every jar
  (MODULE.bazel, pinned by lock files) and the legend-engine / legend-pure release
  sources the tests read as the spec (pinned by sha256).
- **For the native warehouse, the platform's C toolchain on macOS and Windows** (on Linux Bazel fetches
  LLVM, a C library and zlib; its linker needs only the host's `libxml2`): on macOS Apple's Command Line Tools (`xcode-select
  --install`), on Windows Visual Studio 2022 Build Tools 17.6+ (below). A native build checks for it
  first and names what is missing (`tools/cc/host_cc.bzl`).
- An IDE: IntelliJ with the Bazel plugin opens the BUILD files as the project.

**On Windows**, four more things, once:

- **Developer Mode** on (Settings → System → For developers). Bazel makes real symlinks for
  the tests' runfiles (`.bazelrc`), which Windows allows only in Developer Mode or as admin.
- **[Git for Windows](https://git-scm.com/download/win)** at its default path. Bazel runs its
  bash (`.bazelrc` names `C:/Program Files/Git/usr/bin/bash.exe`). If your Git is elsewhere, or
  you use MSYS2's bash, repeat those two `.bazelrc` lines with your path in
  `%USERPROFILE%\.bazelrc`. The `.bazelrc` lines take precedence over finding MSYS2 on its own,
  so without that the first shell action fails, naming the missing `bash.exe`.
- **Bazelisk** as `bazel`: `winget install Bazel.Bazelisk` installs it as `bazelisk`, then, in
  a new PowerShell,
  `New-Item -ItemType SymbolicLink -Path "$env:LOCALAPPDATA\Microsoft\WinGet\Links\bazel.exe" -Target (Get-Command bazelisk).Source`.
- **Visual Studio 2022 Build Tools**, "Desktop development with C++": `bazel build //...` builds the
  native warehouse, which links with MSVC. Install it before Bazel first runs, or run
  `bazel fetch --configure --force` once after, since Bazel keeps the C++ toolchain it found first.

Bazel keeps its outputs in `C:\bzl` on Windows (`.bazelrc`, `startup:windows --output_user_root`), not under
your profile: the default path is long enough to push some test files past Windows' path limit. It needs
permission to create `C:\bzl` once.

Line endings need nothing in a fresh clone: `.gitattributes` keeps text files LF in the working
tree, whatever `core.autocrlf` says, and normalizes an editor's CRLF to LF on commit (the corpus and
the generated files are compared byte for byte). A clone made with `core.autocrlf=true` before
`.gitattributes` existed keeps its CRLF files until they are checked out once more.

### Build & Test

Bazel is the only build. Every gate is one test target, and every CI lane is a suite of them in
[`gates/BUILD.bazel`](gates/BUILD.bazel).

```bash
bazel build //...                                 # everything
bazel test //core:core_tests                      # the compiler suite (NullAway runs on every compile)
bazel test --lockfile_mode=error //gates:local    # the local gate: run it before a push to main
bazel test //gates:core                           # one CI lane (core, checks, corpus_duckdb, pct_duckdb, ...)
bazel test //...                                  # every test, and every generated file checked against its generator
bazel run //:update_generated                     # regenerate every generated file from the pinned upstream release
bazel run //tools/bump -- 4.146.0                 # move upstream to another release (docs/GATES.md)
bazel test //rust/... //core:ffi_test             # the Rust crates and the JVM's shim over them
```

`//gates:local` holds the light lanes (`core`, `checks`, the warehouse's JVM tests, `datacube`, `ui`, `sdlc`). The
heavy lanes (the corpus, PCT, the stress corpus, parser equivalence, Channel B, the native image's tests) run in
CI only, and the manual tests are `//gates:heavy`. CI runs `bazel test //gates:<lane>` per lane on Linux, macOS
and Windows. The rules for pushing to `main` are `AGENTS.md`'s "Pushing to main".

The lanes and what each holds: [`docs/GATES.md`](docs/GATES.md). The upstream sources are declared inputs, so no
test skips for a missing checkout or runs against the wrong one. Test logs and outputs land in
`bazel-testlogs/<package>/<target>/`. More of the gates:

```bash
bazel test //spec:judge_lanes                     # the corpus on DuckDB and H2 under both judges
bazel test //pct:pct_duckdb                       # the engine's own PCT suites against legend-lite
bazel test //parser-equivalence:parser_parity     # byte-equivalence vs legend-engine's parser
```

### Run the Server

```bash
bazel run //core:server                  # LSP, query execution, SQL, diagrams
bazel build //core:server_deploy.jar     # the one self-contained jar to ship
```

The server starts on **port 8080** (or `$PORT`). Point any legend-engine client (DataCube, for one) at
`http://localhost:8080`. It listens on the loopback interface only and serves pages from loopback origins (any
port); set `LEGEND_LITE_ALLOWED_ORIGINS` (comma-separated exact origins) to serve another page, and
`LEGEND_LITE_BIND` to listen on another address. A request whose `Origin` is not allowed is refused with 403.

The deploy jar carries the Rust dialect library its build made, so the jar is the build platform's
([`PHASE1.md`](docs/rust-port/PHASE1.md) §4).

---

## HTTP API

legend-lite serves legend-engine's own `pure/v1` API: the same requests and the same answers,
so a client written for legend-engine runs against legend-lite unchanged
(`datacube/docs/ENGINE_API_CONTRACT.md`; parity tests in `core/src/test/java/com/legend/server/`).

| Method | Endpoint | legend-engine's call |
|--------|----------|----------------------|
| `POST` | `/api/pure/v1/grammar/grammarToJson/lambda` | a query's text to its lambda JSON |
| `POST` | `/api/pure/v1/grammar/grammarToJson/model` | a model's text to its PMCD JSON |
| `POST` | `/api/pure/v1/grammar/jsonToGrammar/lambda` (and `/lambda/batch`) | a lambda's JSON back to text |
| `POST` | `/api/pure/v1/grammar/jsonToGrammar/model` | a model's JSON back to text |
| `POST` | `/api/pure/v1/compilation/compile` | a model compiled, its errors |
| `POST` | `/api/pure/v1/compilation/lambdaRelationType` | a query's result columns and their types |
| `POST` | `/api/pure/v1/compilation/lambdaReturnType` | a query's return type |
| `POST` | `/api/pure/v1/execution/generatePlan` | the query's execution plan, carrying its SQL |
| `POST` | `/api/pure/v1/execution/execute` | the query run: its columns, the SQL it ran, its rows (`?serializationFormat=ARROW_IPC`: an Arrow IPC stream) |
| `GET` `POST` `PUT` `DELETE` | `/api/pure/v1/query/...` | the query store's saved queries; under `/dataCube`, DataCube's saved pages |
| `GET` | `/api/server/v1/currentUser` | the current user |

legend-lite's own routes beside them:

| Method | Endpoint | What |
|--------|----------|------|
| `POST` | `/api/lite/v1/compilation/compile` | every compile error of a model |
| `POST` | `/api/lite/v1/model/createClass`, `/createAssociation`, `/createDataSpace` | a class, an association or a data space made in a project, answered as the net file changes in the SDLC's shape (`server/ModelChanges`) |
| `POST` | `/lsp` | LSP JSON-RPC: diagnostics, completions, hover |
| `POST` | `/engine/diagram` | a class diagram from a Pure model |
| `GET` | `/health` | health check |

The model travels as text (`{"_type": "text", "code": ...}`); the runtime rides the query
(`->from(runtime)`) or the request (`"runtime": {"_type": "runtimePointer", "runtime": ...}`).

---

## The Pure Language

Pure is Legend's modeling and query language. Define **what your data looks like** (classes), **where it lives** (databases + mappings), and **how to access it** (runtimes). Query with type-safe expressions that compile to SQL.

### Classes & Associations

```pure
Class model::Person {
    firstName: String[1];
    lastName:  String[1];
    age:       Integer[1];
}

Class model::Address {
    street: String[1];
    city:   String[1];
}

Association model::Person_Address {
    person:    Person[1];
    addresses: Address[*];
}
```

Multiplicities: `[1]` required, `[0..1]` optional, `[*]` collection.
Types: `String`, `Integer`, `Float`, `Decimal`, `Boolean`, `Date`, `DateTime`, `StrictDate`.

### Databases & Mappings

```pure
Database store::DB (
    Table T_PERSON (ID INTEGER PRIMARY KEY, FIRST_NAME VARCHAR(100), LAST_NAME VARCHAR(100), AGE INTEGER)
    Table T_ADDRESS (ID INTEGER PRIMARY KEY, PERSON_ID INTEGER, STREET VARCHAR(200), CITY VARCHAR(100))
    Join Person_Address(T_PERSON.ID = T_ADDRESS.PERSON_ID)
)

Mapping model::M (
    Person: Relational {
        ~mainTable [store::DB] T_PERSON
        firstName: [store::DB] T_PERSON.FIRST_NAME,
        lastName:  [store::DB] T_PERSON.LAST_NAME,
        age:       [store::DB] T_PERSON.AGE
    }
    Address: Relational {
        ~mainTable [store::DB] T_ADDRESS
        street: [store::DB] T_ADDRESS.STREET,
        city:   [store::DB] T_ADDRESS.CITY
    }
    model::Person_Address: AssociationMapping (
        person:    [store::DB]@Person_Address,
        addresses: [store::DB]@Person_Address
    )
)
```

### Model-to-Model Mappings

```pure
Class model::RawPerson { firstName: String[1]; lastName: String[1]; age: Integer[1]; isActive: Boolean[1]; }
Class model::Person    { fullName: String[1]; ageGroup: String[1]; }

Mapping model::M2M (
    Person: Pure {
        ~src RawPerson
        ~filter $src.isActive == true
        fullName: $src.firstName + ' ' + $src.lastName,
        ageGroup: if($src.age < 18, |'Minor', |if($src.age < 65, |'Adult', |'Senior'))
    }
)
```

Chained M2M: `PersonSummary →(Pure) Person →(Pure) RawPerson →(Relational) T_PERSON` — resolves to a single SQL statement.

### Runtimes & Connections

```pure
RelationalDatabaseConnection store::Conn { type: DuckDB; specification: InMemory { }; auth: NoAuth { }; }
Runtime test::RT { mappings: [model::M]; connections: [store::DB: [environment: store::Conn]]; }
```

### Queries

```pure
// Tabular
Person.all()->filter({p|$p.age > 30})->project(~[name: p|$p.firstName, age: p|$p.age])

// Association navigation
Person.all()->filter({p|$p.addresses.city == 'NYC'})->project(~[name: p|$p.firstName])

// GroupBy
Person.all()->project(~[dept: p|$p.dept.name, sal: p|$p.salary])
  ->groupBy(~[dept], ~[avg: x|$x.sal : y|$y->average()])

// Graph fetch — N-level nested JSON
Person.all()
  ->graphFetch(#{ Person { name, firm { legalName }, addresses { city, country { name } } } }#)
  ->serialize(#{ Person { name, firm { legalName }, addresses { city, country { name } } } }#)
```

Each of these compiles to one SQL statement. An association becomes a join. A filter that reads a to-many
association's values joins it, one row per matching child, as legend-engine does; an emptiness test (`exists`,
`isEmpty`, `isNotEmpty`, `forAll`) is an `EXISTS` semi-join. A graph fetch's JSON is built by the database
(`json_object` and `json_group_array` on DuckDB, a to-many branch as a correlated subquery). M2M becomes inline
expressions. [`FAQ.md`](FAQ.md) shows the SQL.

---

## Documentation map

| Doc | What it holds |
|---|---|
| [`AGENTS.md`](AGENTS.md) | The rules for AI assistants working here, and the invariants with their enforcement status |
| [`docs/rust-port/ROADMAP.md`](docs/rust-port/ROADMAP.md), [`rust/README.md`](rust/README.md) | The Rust port: its phases and seams; its crates and oracles |
| [`docs/architecture/README.md`](docs/architecture/README.md) | The design overview, the invariants the Rust port keeps, the known structural gaps; with [`frontend.md`](docs/architecture/frontend.md) (text → typed model), [`backend.md`](docs/architecture/backend.md) (typed spec → SQL → plan) and [`testing.md`](docs/architecture/testing.md) (how lite is judged) |
| [`core/README.md`](core/README.md) | core's layout, per-package contracts and its own numbered invariants |
| [`docs/SEMANTICS_REGISTER.md`](docs/SEMANTICS_REGISTER.md) | Every deliberate difference from legend-pure and legend-engine, one row each |
| [`docs/UPSTREAM_BEHAVIOR.md`](docs/UPSTREAM_BEHAVIOR.md) | How legend-engine and legend-pure behave, with source citations |
| [`docs/backends/`](docs/backends/README.md) | Each database backend: what it supports, how, and the probe evidence |
| [`docs/DATACUBE_ARCHITECTURE.md`](docs/DATACUBE_ARCHITECTURE.md) | DataCube's architecture |
| [`docs/dialect-capability/`](docs/dialect-capability/README.md) | Generated: what each dialect does with every function and aggregate |
| [`docs/GATES.md`](docs/GATES.md) | The gates and lanes, and how to read a result |
| [`FAQ.md`](FAQ.md) | Design questions, answered |
| [`docs/HISTORY.md`](docs/HISTORY.md) | The index of removed plans, audits and logs (read any with `git show`) |

> Cut in the 2026-11 docs cleanup: the Project Stats table (derived 2026-08-06 from Maven surefire reports,
> with an `engine` row). Full text: `git show de2745351:README.md`. Today's numbers: `docs/GATES.md` and
> `rust/BASELINE.tsv`.

---

## License

Apache License 2.0 ([`LICENSE`](LICENSE); SPDX: `Apache-2.0`). What legend-lite takes from FINOS Legend's legend-pure, legend-engine and legend-studio, and the notices it carries, are in [`NOTICE`](NOTICE); the third-party software its products redistribute, with their licenses, is in [`THIRD_PARTY_NOTICES.md`](THIRD_PARTY_NOTICES.md).
