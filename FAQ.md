# Frequently Asked Questions

## General

### What is Legend Lite?

Legend Lite is a **clean-sheet implementation** of the [FINOS Legend](https://legend.finos.org/) platform's Pure language and engine — the open-source data management platform originally created by Goldman Sachs — designed and written anew, taking [legend-pure](https://github.com/finos/legend-pure) and [legend-engine](https://github.com/finos/legend-engine) as its specification (see [`NOTICE`](NOTICE)). It reads the same Pure modeling language, compiles every query to SQL, and the database executes it. Around the compiler sit Studio, Query and DataCube rebuilt light, a DuckDB warehouse, and SDLC-lite and Depot-lite ([`README.md`](README.md), "Modules").

### How is Legend Lite different from the original Legend Engine?

The comparison is in [`README.md`](README.md), "What is Legend?". In short: the database computes every value
(legend-engine runs some work in memory in Java); the parser is hand-written, not ANTLR; `//core:core` compiles
against the JDK alone; and 16 dialect classes cover DuckDB, H2, Postgres, SQLite, MariaDB, MySQL, Trino,
ClickHouse, Snowflake, Databricks, BigQuery and MongoDB, plus three that print legend-engine's own SQL text.

> Cut in the 2026-11 docs cleanup: the comparison table (Maven-era sizes, build times and database list). Full
> text: `git show de2745351:FAQ.md`.

### Is Legend Lite a fork of Legend Engine?

No. Legend Lite is a **clean-sheet implementation**: its compiler and engine were written anew, not forked from legend-pure or legend-engine. It takes them as its specification — their grammar, typing rules and behaviour — and tests itself against their test suites and recorded answers. Some upstream material is included, under its Apache 2.0 license, and [`NOTICE`](NOTICE) lists it: the platform's Pure declarations, copied verbatim into `prelude.pure`; function signatures and registry names extracted from upstream by the generators in `spec/`; some test fixtures; and, for the apps, legend-studio's colour tokens, icons and Pure editor grammar. It reads the same Pure syntax but compiles it through a completely different pipeline built from scratch.

### What is the Rust port?

The Java implementation is the running system, and it is being ported to Rust on branch `rust-port`. The plan is
[`docs/rust-port/ROADMAP.md`](docs/rust-port/ROADMAP.md); the crates are under `rust/`
([`rust/README.md`](rust/README.md)). Iteration 1 replaces the whole planning path with Rust and removes GraalVM
from the compiler's native library. Java keeps the executor, the HTTP shell, the test runners, and the
warehouse, SDLC, depot and query-store modules.

The port works inward from both ends of the pipeline, over serialized forms that already exist: the SQL tree
(`MirJson`), the protocol JSON a parse produces, and the plan JSON the Java executor runs. Phase 1 (the
dialects) is under way: since 2026-11-03 DuckDB, H2 and the three engine-text dialects render in Rust behind
Java shims ([`docs/rust-port/PHASE1.md`](docs/rust-port/PHASE1.md) §4). A phase lands only when every test target
that passed on the pristine tree (`rust/BASELINE.tsv`) still passes, or is a dated row of `rust/RETIRED_TESTS.md`.

### What does "100% SQL push-down" mean?

The database computes every value of a query's answer; the runtime only streams the answer's typed rows to a
writer. Operations like `filter`, `groupBy`, `sort`, `if/else`, `fold`, struct construction, graph fetch and
string functions all compile to SQL constructs. Even a model-to-model query, which legend-engine runs in memory
in Java, runs on an in-process DuckDB with its data inline in the SQL (register row S27 of
[`docs/SEMANTICS_REGISTER.md`](docs/SEMANTICS_REGISTER.md)).

For example, this Pure (`RelationalMappingCompositionTest`, "Multi-operation chains"):

```pure
model::Person.all()
  ->filter({p|$p.name != 'Eve'})
  ->project(~[name:p|$p.name])
  ->sort(~name->ascending())
  ->limit(2)
```

renders on DuckDB as this statement (from the golden dump of Java's renders, `rust/oracles/mir/mir-dump.jsonl.zst`):

```sql
SELECT t0.NAME AS name
FROM T_PERSON AS t0
WHERE (t0.NAME IS DISTINCT FROM 'Eve')
ORDER BY t0.NAME NULLS LAST
LIMIT 2
```

---

## Architecture & Design

### Why Java records and sealed interfaces, and what carries over to Rust?

Every AST and IR node is an immutable record, and every variant family has a sealed root (`ValueSpecification`,
`TypedSpec`, `Type`, `SqlQuery`, `SqlExpr`, …). Switches over a sealed root list every variant with no
`default ->`, so javac fails the build when a variant is added and some dispatch site does not handle it
([`core/README.md`](core/README.md), invariants 3–5).

What carries over is the discipline, not the Java: closed variant families with exhaustive dispatch, plain
immutable data, and no stringly-typed calls. In the port the SQL tree is the `lite-sql` crate, which reads every
statement of the golden dump from its JSON (`MirJson`) and writes it back to the bytes Java wrote; the dialects
are `lite-dialect`, with the shared renderer as a trait ([`rust/README.md`](rust/README.md),
[`docs/rust-port/PHASE1.md`](docs/rust-port/PHASE1.md) §4).

> Cut in the 2026-11 docs cleanup: the Java-specific detail of this answer (record and sealed-interface
> benefits, and an illustrative switch). Full text: `git show de2745351:FAQ.md`.

### Why a hand-written parser instead of ANTLR?

**legend-lite's parser uses no ANTLR**, and no product closure holds it: the ANTLR runtime appears only inside the
upstream legend-engine jars that the parser-equivalence tests and the engine runner load as the oracle. Both
halves are hand-written recursive descent in `core/parser/`:
`ElementParser` for definitions (classes, mappings, databases, runtimes) and
`SpecParser` for expressions (value specifications, lambda bodies,
applications); `PmcdParser` walks a text's sections.

Upstream legend-engine uses ANTLR for both. The measured reasons for not
following it (the parser drop-in study's §0, `docs/PARSER_DROP_IN.md` in `docs/HISTORY.md`):

1. **Memory** — 1,281 bytes allocated per source character vs ~39. That is
   3.6 GB to parse 2.8 MB of source, plus ~31 MB retained just to warm the
   parser. The strongest reason by a distance.
2. **Generated code** — 156 `.g4` files, 14,568 generated lines, 46% of the
   grammar jar's bytes.
3. **Build** — an `antlr4-maven-plugin` bound to `generate-sources` in 41 modules.
4. **Currency** — upstream is pinned to ANTLR 4.8-1 (2020) and can only move
   in lockstep with all generated code.
5. **Debuggability** — stepping recursive descent versus an ATN simulator.
6. Speed — ~54× on parse, and explicitly the *weakest* of the six. There is no
   latency gate; the project is justified at 0% speedup.

The parser is a byte-for-byte drop-in candidate for upstream's: 22,725/22,725
elements currently emit identical protocol JSON, checked against legend-engine
running live in-process (gate 8).

### How does type information flow to the backend?

**Through the tree, not beside it.** Phase G (`compiler/spec/SpecCompiler`,
`Typer`, `InferenceKernel`) consumes the untyped `protocol.spec.ValueSpecification`
and produces a *different*, typed tree: `compiler.spec.typed.TypedSpec`, a
sealed hierarchy (64 direct variants, two of them sealed families of their own: the relation operations and the
store writes) carrying type, multiplicity, relation columns
and resolved callee signatures on the nodes themselves.

> **The legacy engine used a side table** — a `Map<ValueSpecification, TypeInfo>`
> threaded alongside the AST. Core deliberately does not: `core/README.md`
> invariant 8 forbids mutable sidecar state across passes. Each phase takes
> input and returns output. If you find yourself wanting an
> `IdentityHashMap<TypedSpec, ?>` spanning a phase boundary, that is the smell
> the invariant names.

### What is the dialect abstraction layer?

The Lowerer (`lowering/Lowerer`, phase I) never emits raw SQL. It builds a
sealed, dialect-free MIR under `sql/` — `SqlQuery`, `SqlSource`, `SqlExpr`,
`SqlAgg` — which the dialect then renders through a single entry point,
`SqlDialect.render(SqlQuery)`.

Crucially, **there is no `FunctionCall(String name, args)` catch-all in the
MIR**. Every operation is its own typed record, so adding a Pure native means
adding a variant *and* a render arm — and because render methods are switch
expressions with no `default ->`, javac fails the build until you do. That is
`AGENTS.md` invariant 3a.

This means adding a new database dialect only requires implementing the rendering layer — the compilation logic is shared. What each dialect does with every function and aggregate is generated in [`docs/dialect-capability/`](docs/dialect-capability/README.md).

---

## SQL Generation

The SQL below is DuckDB's, taken from the golden dump of Java's renders (`rust/oracles/mir/mir-dump.jsonl.zst`);
each query is from a core test.

> Cut in the 2026-11 docs cleanup: "Why `EXISTS` instead of `INNER JOIN` for to-many filters?" and "Should
> `WHERE EXISTS` come before or after the `JOIN`?" (2026-08). They said every to-many filter renders as `EXISTS`;
> the tree now joins a filter that reads the to-many values (legend-engine's row semantics). Full text:
> `git show de2745351:FAQ.md`.

### How does a filter through a to-many association render?

It depends on what the filter asks of the to-many values.

A filter that **reads the values** joins them, one row per matching child, as legend-engine does. For
`Person.all()->filter({p|$p.addresses.city == 'New York'})->project(~[name:p|$p.name])`:

```sql
SELECT t0.NAME AS name
FROM T_PERSON AS t0
LEFT OUTER JOIN T_ADDRESS AS t1 ON t0.ID = t1.PERSON_ID
WHERE t1.CITY = 'New York'
```

So a person with two matching addresses appears twice (`RelationalMappingIntegrationTest.testNoRowExplosion`
and `RelationalMappingCompositionTest.filterToManyNoExplosion`: "one row per matching child; explicit exists()
keeps the semi-join", the engine's row semantics from the corpus's `testInNegated` golden).

An **emptiness test** — `exists`, `isEmpty`, `isNotEmpty`, `forAll` — keeps each base row once, so it stays a
semi-join, `EXISTS (SELECT 1 FROM … WHERE … AND <correlation>)` (the dump's `exists()` renders sit inside larger
statements; this is their shape). The two kinds, an emptiness call versus a predicate read over the values, are
named in `resolver/InnerDemand`'s javadoc and pinned by the two `…NoExplosion` tests above.

### Why `LEFT OUTER JOIN` for to-many projections?

When projecting through a to-many association, we intentionally want row multiplication. For
`Person.all()->project(~[name:p|$p.name, street:p|$p.addresses.street])`:

```sql
SELECT t0.NAME AS name, t1.STREET AS street
FROM T_PERSON AS t0
LEFT OUTER JOIN T_ADDRESS AS t1 ON t0.ID = t1.PERSON_ID
```

**Why LEFT (not INNER)?** Preserves people with no addresses (NULL for street). This matches Pure's `[*]` multiplicity — "zero or more."

**Why JOIN (not EXISTS)?** We need actual data for the `SELECT` clause, not just an existence check.

### How does graph fetch compile?

The database builds the JSON. For `m::Firm.all()->sortBy(f|$f.legal)->graphFetch(#{m::Firm{legal, staff{name}}}#)`
(`ResolveSerializeTest.toManyChild`):

```sql
SELECT CAST(json_object('legal', t1.LEGAL, 'staff', (SELECT coalesce(json_group_array(json_object('name', t2.NAME)), '[]') AS result FROM P AS t2 WHERE t2.FID = t1.ID)) AS VARCHAR) AS result
FROM (
  SELECT *
  FROM F AS t0
  ORDER BY t0.LEGAL NULLS LAST
) AS t1
```

Each object is a `json_object`; a to-many branch is a correlated subquery that aggregates its objects with
`json_group_array`, and `coalesce(…, '[]')` makes an empty collection the empty array, never NULL. The plan
answers one object per row, and the JSON rows writer joins them into the array as the rows stream
(`StreamingIntegrationTest`, since 2026-10-27). Other dialects spell the same tree in their own JSON vocabulary
(`JSON_OBJECT` and `JSON_ARRAYAGG` on MariaDB, for one).

---

## Results & Serialization

### What result formats are supported?

A query's answer is its typed rows, streamed in batches by the plan's runner and written at the edge by a writer
(`core/src/main/java/com/legend/result`): `CsvWriter` (RFC 4180) and `JsonRowsWriter` (lite's own outputs, each cell in
Pure's print form); `PureV1Writer` (legend-engine's `pure/v1` execute answer, each cell as the engine's token);
`TdsObjectsWriter` (legend-engine's `PURE_TDSOBJECT`, the service test runner's format); and `ArrowWriter` (one
Arrow IPC stream, framed by `//arrow_ipc` with no Arrow library; `execute?serializationFormat=ARROW_IPC`).
The database computes every value; a writer only chooses its text.

### How do I add an output format?

Implement `com.legend.result.ResultSink` (`columns`, `batch`, `end`): the plan's columns arrive first, with their Pure
type, multiplicity and SQL slot, then the rows in batches of typed cells (`Batch.get(row, column)`). Spell each cell
from its column's facts alone (`CellText` holds Pure's print forms and the engine's tokens). A format the product
ships is a variant of `com.legend.result.Output` (a closed set: the result library makes the writer from the plan, so
the run calls no factory of the caller's), added beside `JsonRows` and `Csv`; a writer of your own runs as the caller's
receiver, `Execution.run(..., new Output.Rows(new YourWriter(out)))`, told the answer's shape (`ResultSink.answers`)
before its columns.

---

## Build & Development

### How do I build and test?

Bazel is the only build. The commands, and the local gate to run before a push to `main`
(`bazel test --lockfile_mode=error //gates:local`), are in [`README.md`](README.md), "Build & Test"; every lane
and what it holds is in [`docs/GATES.md`](docs/GATES.md).

### How do I run a single test?

Each core test package is its own target, and `--test_filter` takes a regex over `fully.qualified.Class#method`
(`tools/junit/JUnitMain.java`; a filter that matches nothing fails the run):

```bash
bazel test //core:core_tests_lexer --test_filter=LexerTest
```

> Cut in the 2026-11 docs cleanup: the NLQ section (an LLM pipeline whose `nlq/` module no longer exists) and the
> Maven-era build section (`mvn` commands, build times, a structure block listing the deleted `engine/` and
> `nlq/`). Full text: `git show de2745351:FAQ.md`.

---

## Compatibility

### Which Pure functions are supported?

What each dialect does with every function and aggregate of the SQL IR — spelled, emulated or refused — is
generated from the dialects' own tables: [`docs/dialect-capability/`](docs/dialect-capability/README.md)
(`functions.md`, `aggregates.md`). How much of the platform passes is measured by legend-engine's own PCT suites
([`docs/GATES.md`](docs/GATES.md)).

> Cut in the 2026-11 docs cleanup: the hand-maintained function list (2026-08). Full text:
> `git show de2745351:FAQ.md`.

### What happens with a function that has no SQL translation?

It fails loudly; there is no silent fallback to in-memory processing.

- A function the compiler does not know is a `TypeInferenceException` ("unknown function …"), a
  `LegendCompileException` whose `phase()` names the stage.
- A known function the target dialect cannot spell is a `DialectCapability` at render, the dialect's named
  capability wall (an `IllegalStateException`, distinct from a wrong answer).
- A feature not built yet is a `NotImplementedException`.

### Does Legend Lite support Model-to-Model (M2M) transforms?

Yes. A model-to-model query executes on the platform's in-process DuckDB, its data inline in the SQL, where
legend-engine runs it in memory in Java (register row S27 of
[docs/SEMANTICS_REGISTER.md](docs/SEMANTICS_REGISTER.md)). The original design note
(`docs/MODEL_TO_MODEL.md`) is in [docs/HISTORY.md](docs/HISTORY.md).

---

## Contributing

Have a question not answered here? Open an issue on [GitHub](https://github.com/neema2/legend-lite)!
