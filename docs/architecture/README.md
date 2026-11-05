# legend-lite's architecture

This folder describes legend-lite in language-neutral terms: what each stage owns, what crosses each seam, and which
rules hold whatever language a stage is written in. It is written for a newcomer and for whoever ports or extends a
stage (today the Rust port, `docs/rust-port/ROADMAP.md`). Java class names appear only where they are today's
implementation and help you find it; `AGENTS.md` and `core/README.md` hold the current Java map.

| Document | What it covers |
|---|---|
| this page | what legend-lite is, the pipeline, the plan as the seam, the tenets, the invariants the port keeps, the known structural gaps, the compiler's target and its oracles |
| [`frontend.md`](frontend.md) | text to typed model: parsing, names, the canonical function form, elements, typing, inlining, and the upstream rules each stage reproduces |
| [`backend.md`](backend.md) | typed spec to SQL to plan to rows: store resolution, normalization of mappings, lowering, the SQL IR, dialects, the plan/execution split, carriers |
| [`testing.md`](testing.md) | how lite is judged: the corpus, PCT, the judges and their verdicts, the oracles |

## 1. What legend-lite is

legend-lite replaces FINOS legend-pure and legend-engine. It is a compiler from Pure to SQL plus an orchestrator that
drives a database. It accepts what upstream accepts (Pure model text in the engine's grammar, protocol JSON, the
engine's `pure/v1` HTTP requests) and answers what upstream answers: the same protocol JSON from its parser, the same
`pure/v1` responses, the same rows.

It has no runtime dependency on either project. The pinned checkouts of legend-pure and legend-engine are
**specification and test input only**: corpus test sources, PCT trees, parity fixtures, signature verification. They
never feed the thing doing the judging or the running: no resolution, typing or standard-library body may depend on a
file from them. The platform's standard library (`meta::pure::functions::*`) is lite's own: native signatures verified
verbatim against the real `.pure` sources ("spec by verification, never by loading"), and parsed twins of those
functions are suppressed (register row S10).

Today the planner and the executor are Java, and the browser plans with a WebAssembly build of the Java planner
(TeaVM), frozen on 2026-11-03 when core's dialects moved out of what TeaVM can compile. The Rust port's first
iteration replaces the whole planning path with Rust; Java keeps the executor, the HTTP shell, the test runners and the
warehouse, SDLC, depot and query-store modules. A Rust wasm32 build replaces the frozen browser planner later
(`docs/rust-port/ROADMAP.md`, `docs/rust-port/PHASE1.md` §5).

Sources: `AGENTS.md` (reference-checkout tenet), `docs/WORLD_MAP.md` §1, `docs/rust-port/ROADMAP.md`,
`docs/rust-port/PHASE1.md`.

## 2. The pipeline

One driver owns the order of the stages. Knowledge (lexing through element compilation) is eager and total; Work (typing a body
onward) is demand-driven (`docs/TENETS.md`). The planner's library has no execution dependency: planning never
touches a database ([`backend.md`](backend.md) §11.1, the rule `AGENTS.md` cites as C2a). Every stage is the same function its own unit tests exercise; there is no
orchestrator-only code path.

| | Stage | In → out | Today's Java |
|---|---|---|---|
| A | **Lex** | text → tokens | `lexer/Lexer.tokenize` |
| B | **Parse elements** | tokens → protocol records (legend-engine's protocol, byte-exact as JSON) → the parsed model, through the same door a JSON model takes | `parser/PmcdParser`, `ElementParser`; `door/ModelFromText` → `model/ModelFromProtocol` |
| C | **Parse expressions** | tokens → value specifications (expression trees, also protocol records) | `parser/SpecParser` |
| D | **Resolve names** | simple names → full names; every call records every full name it can mean | `compiler/NameResolver.resolve` |
| E | **Normalize** | parsed model → normalized model: every executable body becomes an ordinary function; structure only references it | `normalizer/ModelNormalizer.normalize` |
| F | **Compile elements** | normalized model → typed model (typed elements in a model context); no body is opened | `compiler/element/PureModelContext.from` |
| G | **Type expressions** | expression + typed model → typed spec: every node a type and a multiplicity, every call one declaration | `compiler/spec/SpecCompiler`, `Typer`, `InferenceKernel` |
| G½ | **Inline** | typed spec → typed spec with user calls β-inlined; runs on **every** execution path | `compiler/spec/UserCallInliner` |
| H | **Resolve store** | logical typed spec → physical typed spec (class queries become relational pipelines) | `resolver/StoreResolver` |
| I | **Lower** | typed spec → SQL IR (a closed, dialect-free tree) | `lowering/Lowerer` |
| J | **Render** | SQL IR → SQL text for one dialect, parameters in placeholder order | `sql/dialect/SqlDialect`; since 2026-11-03 DuckDB, H2 and the engine-text dialects are shims over the Rust `lite-dialect` library, the other ten Java |
| P | **Plan** | statements, slots, target and typed result columns → one plan record | `TypedQuery.plan(runtime)` (runs G½ to J) → `executionplan/ExecutionPlan` |
| K | **Execute** | plan + session → typed rows, streamed to a sink | `exec/PlanRunner.stream` |
| — | **Write results** | rows → response bytes; each cell spelled at the boundary, a writer decides nothing | `result/` writers |

A to G½ is the frontend ([`frontend.md`](frontend.md); the mechanics of normalizing a legacy mapping are in
[`backend.md`](backend.md) §4, where the store side of the model takes shape); H and I the midend; J the backend; P
and K the runtime ([`backend.md`](backend.md) §§5–11).

Every user-visible error carries one of eight phases: PARSE, RESOLVE, NORMALIZE, MODEL, TYPE, MAPPING, LOWER,
EXECUTE (there is no RENDER). The Rust port's error contract carries the same phase
(`ERR\n<kind>\n<phase>\n<line:col>\n<message>`).

The planner's doors: compile a model (strict: the first error aborts; or tolerant, every element error collected),
list a model's compile errors (every body typed), type a query once into a typed query, and plan that typed query for
a runtime. Execution's door runs a plan and hands its rows to a writer; a test body is compiled whole to one artifact
(its frames, verdict rows and prepared value statements) before any of it runs, and the body driver then sends it
and its judges read the verdicts ([`testing.md`](testing.md) §4; `AGENTS.md`, "Entry points"). Two modes stand (register row S1): compile-all
types the whole world and fails on any error; a user query is typed on demand and runs if everything it touches is
valid, even when another body is broken, where legend-engine rejects the whole model.

Sources: `AGENTS.md` (Pipeline, Entry points, Layer ownership), `core/README.md` (Pipeline), `docs/TENETS.md`,
`docs/rust-port/ROADMAP.md`, `docs/rust-port/PHASE1.md` §4, `docs/SEMANTICS_REGISTER.md` S1, `docs/EXECUTION_PLAN_2026_09_26.md` D10 (git history:
docs/HISTORY.md).

## 3. The plan: the seam between planning and execution

A plan is one structured record, and its JSON is one rendering of it. The record holds the parameters with their Pure
types and multiplicities, and a root that is typed rows, a text result or a sequence of nodes. Each SQL node holds the
statement as rendered once at planning, its parameter slots (each binding and, where a database types a placeholder
by its value, the type hole and the spelling table), its target (the database, the servers it is written for, the
session statements, the setup steps) and the typed SQL tree it was rendered from. Every result column carries three
facts decided at planning: its Pure type and multiplicity, its SQL slot, and the store's declared type. A reader needs
no model.

The rules of the record (ratified 2026-10-18, `docs/PLAN_AND_RESULT_MODEL_2026_10_18.md` §1):

- **One record** (P1). The engine-protocol plan `pure/v1` serves is a rendering of it, not a second plan.
- **The statement is rendered once, at planning** (P2). The runner holds no dialect and renders nothing.
- **The tree travels only when asked** (P3): the wire plan carries the statement text; the full rendering adds the
  tree. A serialized plan never carries its statement twice.
- **JSON, no binary format** (P4); a big plan is big because of SQL text.
- **A plan is reviewed whole before it runs** (P5): every slot, every null type and the target, before a session opens.
- **The plan is model-free and dialect-free to its reader** (P6): a runner, renderer or decoder consults the plan's own
  facts and nothing else. That is what makes decoding a cell a carrier conversion rather than an evaluation.

A renderer reads the record and computes nothing. The only rendering read back is lite's own JSON: format
`legend-lite-plan`, version 19 at the port's start (`PlanJson.VERSION` in today's code and the ROADMAP; the
2026-10-18 note's "format 6" is older). The reader refuses any other format or version. A round-trip gate proves the
JSON reads back to the same record and the same statement on every dialect.

The result is one model too: a **stream of rows** in the database's order, consumed once by one sink (S1 to S6 of the
same note). The runner accumulates nothing; order is the database's (a consumer that needs one asks the statement for
an `ORDER BY`); a decoded cell is the driver's value read by the plan's slot and Pure type, never computed;
materializing a table, a scalar or a graph happens only at the edge, in a test's, a judge's or the PCT adapter's
collector. The product's sinks write bytes as rows arrive.

**The seam in the port.** The Rust port works inward over three serialized forms that already exist: the SQL tree's
JSON (the back seam), the parser's protocol JSON (the front seam) and the plan JSON (the final seam). In its third
phase the Java compiler's doors become a shim over Rust: Rust plans and answers plan JSON, and the Java executor,
already walled off from planning, runs it (`docs/rust-port/ROADMAP.md`). The plan JSON is therefore the contract
between a Rust planner and a Java executor; any field a reader needs must be in the record.

Sources: `docs/PLAN_AND_RESULT_MODEL_2026_10_18.md` §§1–6, `docs/rust-port/ROADMAP.md` ("The seams", phase 3),
`core/src/main/java/com/legend/executionplan/PlanJson.java`.

## 4. The tenets

Five tenets govern every design decision; each has its own document. **Total Knowledge, demand-driven Work**: be eager
and total about knowledge (parsing, names, the manifest of what exists with its kind and signature) and lazy about work
(typing bodies, lowering, SQL, execution); an untyped body is inert, not unsafe, because using it compiles it
([`TENETS.md`](../TENETS.md)). **The host orchestrates, the database executes**: no host interpreter, the query compiler
computes no values, and the egress boundary only carries values it never derives, reshapes or decides types from
([`TENET_CHARTER.md`](../TENET_CHARTER.md), clauses 1 to 6). **Three kinds of Pure code**: natives (one SQL lowering
rule each), platform semantics (designed from the engine's Pure as specification) and programs (input to the
compiler, whoever wrote them, never ported); the compiler compares spelled tokens and rearranges shapes, the database
computes every new value ([`WORLD_MAP.md`](../WORLD_MAP.md)). **Reference checkouts are specification, never runtime**
(§1). **Upstream fidelity**: wherever possible lite does exactly what legend-pure and legend-engine do; upstream's answer
is read from the pinned sources or asked of the engine, never recalled; a divergence is posted to the user before it
lands and ledgered as a row of [`SEMANTICS_REGISTER.md`](../SEMANTICS_REGISTER.md); a divergence without a row, or a
row the user did not rule, is a defect (`AGENTS.md`).

Sources: `AGENTS.md` (the tenet banners), `docs/TENETS.md`, `docs/TENET_CHARTER.md`, `docs/WORLD_MAP.md`.

## 5. Invariants the port must keep

`AGENTS.md` and `core/README.md` each hold a numbered list of invariants, cited by number from the code and written in
Java terms. The two lists are separate; neither is renumbered. The rules below are the language-neutral ones: read
each at its source.

**From `AGENTS.md` "Invariants":**

| # | Rule |
|---|---|
| 1 | The frontend does all tree walking and typing. Element compiler and expression compiler together are the single source of truth for types: every expression and call gets a type, every overload resolves to one signature; a type missing downstream is a frontend bug. |
| 2 | Lowering does no type inference. It reads typed structure and annotations; it may dispatch on structure, never on types it works out itself; it names no SQL function and contains no SQL syntax. |
| 3 | The dialect owns all SQL rendering: type names, keywords, function names, quirks. Rendering dispatch is exhaustive; a variant a dialect cannot express is a refusal, still an explicit arm. |
| 3a | The SQL IR is closed and pure data: no node renders itself, no node knows a dialect, no string field encodes a SQL operation (one carve-out: a cast names a Pure type), no catch-all function-call node; lambdas are data. A new dialect is one renderer; the IR does not change. |
| 4 | No fallbacks, no defaulting. An unknown type fails; a missing binding for a resolved overload throws. |
| 5 | User elements load lazily: references are full names in typed wrappers; structural access goes through the model context, the only layer that triggers a load; long-lived fields hold names, never resolved elements. |
| 6 | Compiling elements never type-checks a body (F never triggers G). |
| 7 | Store-only nodes do not survive store resolution, and none reaches lowering. |

Invariant 8 (no dependency from the new tree on the deleted legacy module, no `util` package) is Java-era; its lasting
form is that layering is enforced by build targets (below).

**From `core/README.md` "Strong invariants":** 3 (every variant family is closed), 4 (data carriers are immutable
records), 5 (matches over a closed family are exhaustive, with no default arm; a sanctioned guard default throws),
6 (no generic function-call node in the SQL IR), 7 (= `AGENTS.md` 6), 8 (no mutable sidecar state across passes:
each step takes input and returns output; pass-local caches are labelled and confined), 9 and 10 (= `AGENTS.md` 7),
11 (typed elements reference other elements by full name, not live reference), 12 (= `AGENTS.md` 3a). Items 1 and 2
are Java package rules.

**And these, which bind any implementation:**

- **Planning never touches a database; the executor depends on no planning code.** The planner library has no
  execution dependency, and the executor's dependencies are the record layer (base, JSON, the model, plan, SQL, values
  and result records) and the switchboards (cache, diagnostics, error), pinned by the build (`AGENTS.md`, "THE WALL",
  D72; [`backend.md`](backend.md) §11.1, C2a). A model compiles once and a query is typed once, every question asked
  of that typed query ([`backend.md`](backend.md) §11.2, C2b).
- **Layering is enforced by the build.** Every stage is its own build target with only the dependencies it should
  have; a back-edge is a build error, not a test finding (`docs/EXECUTION_PLAN_2026_09_26.md` rule 0b.12; the
  2026-09-28 review counts it among what a strong team does).
- **Names are resolved once.** Every call carries the record of every full name it can mean; no later stage
  re-resolves a spelling ([`frontend.md`](frontend.md) §4.3).
- **The parser's output is upstream's protocol.** Protocol JSON is byte-identical to legend-engine's, and JSON read
  back rebuilds identical records (the front seam; [`frontend.md`](frontend.md) §3).
- **Nothing observable may depend on hash-iteration order.** No output, decision or message may depend on the
  iteration order of a collection that promises none. The evidence: the TeaVM conformance test found TeaVM's
  `HashMap` and `HashSet` iterating differently from the JVM's in 147 of 166 cases; the JVM randomises `Set.of`,
  `Map.of` and the other immutable collections on every start. The order census of 2026-10-10 traced all 1,192 such
  sites in lite's main Java: 713 only looked up, 431 iterated where nothing can see the order, 48 where the order
  escapes (3 to output, 3 to behaviour, 21 to a message's text, 21 to test tooling only), none unsure. The six
  output and behaviour sites sit in five places in today's Java: which simple name a result's type cell carries when
  two class names mint the same subtype-column prefix; which same-named class a bare class name in a diagram
  resolves to; the order the warehouse attaches its Postgres catalogs (two sites); the key order of one HTTP 500
  body; the order LSP diagnostics for several documents go out. An earlier incident (2026-10-08) made the resolver's lambda scope ids differ between two runs of
  unchanged code; `NoSaltedIterationOrderTest` now refuses salted immutable copies across core. A port must use
  insertion-ordered or sorted collections wherever an order can be seen. The census does not cover Rust, but the same
  rule binds it with more force: Rust's standard `HashMap` is seeded randomly per process, so a dependence shows
  between two runs on one machine, not only between two runtimes. (The census also did not cover the TypeScript and
  Python code, whose maps iterate in insertion order apart from integer-like object keys.)
- **The server serves legend-engine's APIs and only those.** legend-lite's HTTP surface for clients is exactly
  legend-engine's `pure/v1/...` APIs, same requests and same responses (the user, 2026-09-27). Lite's invented
  endpoints were deleted, not extended. Types reach clients over upstream's calls (`lambdaRelationType`, a plan's result
  columns), never over an API of lite's own. A model may travel as `PureModelContextText`, a valid upstream request.
- **The warehouse rulings** (the user, 2026-09-26; the server program that carries them is paused since 2026-09-29):
  entitlements live in the warehouse, never in legend-lite or DataCube, so every mode inherits them; every query runs
  as the viewing user's own identity, the token travelling from the browser through legend-lite to the warehouse, with
  no service accounts; what a role may see is a set of role-level views (rows are the view's `WHERE`, columns and
  masks its `SELECT`) over ACL tables joined in, and the server never rewrites a statement; the browser speaks one
  HTTP SQL API modelled in legend-lite, and vendor APIs are bound behind it on the server side; the direct browser
  path (the tab plans, the warehouse runs) comes before the server-side JDBC path. The warehouse speaks DuckDB SQL and
  emulates no other vendor's dialect.

Sources: `AGENTS.md` (Invariants, the execution tenet's "THE WALL"), `core/README.md` (Strong invariants),
`docs/build-inventory/program/evidence/order-census/README.md`,
`core/src/test/java/com/legend/sql/NoSaltedIterationOrderTest.java`, `docs/UPSTREAM_ENDPOINTS_DESIGN_2026_09_27.md`,
`docs/SERVER_PROGRAM_2026_09_26.md` §0 and §1a, `docs/EXECUTION_PLAN_2026_09_26.md` rule 0b.12 (git history:
docs/HISTORY.md).

## 6. Known structural gaps

An architecture review on 2026-09-28 (tree `6ab32198d`; fourteen readers, each reading one stage whole, headline
claims re-checked by hand) found that **the code works and is badly shaped**. It passed 2,472 of 2,613 relational
corpus tests on DuckDB, had a parser at byte parity with the engine and a catalog of 787 exact declarations, but it
was not built like a compiler. The gaps a port should not reproduce:

- **No symbol layer.** Every function, class, property, store, table, column, join and variable is a string, and each
  stage re-derives identity from spelling.
- **One tree type for several phases.** The same untyped tree is read by name resolution, normalization, typing,
  lineage and the executor; the same typed tree serves typing, inlining, store resolution and lowering, with
  phase-only nodes policed at run time (the store resolver's escapee walk, 11 phase-restricted nodes, 27 mode flags)
  rather than by distinct types.
- **No source positions after the parser.** None of the typer's 231 exceptions, none of the resolver's ~240 and none
  in the normalizer carried a position; failures were classified by grepping messages. Still true in this tree: the
  typer's exception takes a message only.
- **Mapping semantics as text.** The normalizer emitted untyped, text-shaped Pure that was resolved and typed later;
  the store resolver was a 36,000-line pass keyed by dotted strings and suffixes, with mutable threaded state.

What changed since: resolve-once (L7, 2026-10-10) made every call carry its record of full names and put every built
call through one builder, though the names are still strings ([`frontend.md`](frontend.md) §4.3). For product queries
the review's "executor as a second pipeline driver" is gone: a query is planned once and the plan runs (the old door
was deleted 2026-10-29). A test body is compiled whole to one artifact before any of it runs; the statement-by-statement
loop was deleted ([`testing.md`](testing.md) §4). That artifact is not yet a plan:
`docs/PLANNER_DRIVER_SEAM_PROGRAM_2026_10_30.md` (planned 2026-10-30, not ratified) proposes planning a test body once,
as one sequence the database executes.

What the review said to keep in any rewrite: follow legend-pure's matcher exactly, its oddities included, because the
product is judged by a differential against it; one SQL IR, never a second; layering enforced by build targets; probe
before switch, one variable at a time, deletions with the switch; the oracle decides. Its picture of an expert design:
a source layer with one column unit; a syntax tree with spans and no resolver fields; an eager world of interned
declarations with per-kind indexes, C3 linearizations and the core import group generated and drift-tested; a
distinct resolved tree where every name is a handle and every binder has an id; a typed tree with the reference's
matcher under its candidate loop and union-find meta-variables; mapping elaboration after declarations are typed;
store resolution as small passes with explicit identities; lowering from one immutable, duplicate-refusing table;
diagnostics as records in a sink, with a phase verifier; the reference at the same pinned release as a gated lane.

Sources: `docs/plan-audit-2026-09-26/architecture-review-2026-09-28.md` §§1–3, §7, §8 (git history:
docs/HISTORY.md); `core/src/main/java/com/legend/compiler/spec/TypeInferenceException.java`; `AGENTS.md` (Entry
points).

## 7. The compiler's target, and the oracles that judge it

The Java compiler rebuild (`docs/EXECUTION_PLAN_2026_09_26.md`, rev H4, 2026-09-29) was parked on 2026-10-04 and is
superseded by the Rust port. Its Java task list is history; its target and its rules are not.

**The target is parity of observable outcomes** (rule 0b.13, the user's ruling D15). Lite matches legend-pure's
outcomes (accept or reject and the error class, the chosen declaration where it changes lowering or the result type,
the result type and multiplicity) and legend-engine's rows. It never copies a reference's nondeterminism (hash-order
ties), its self-declared bugs or its internal representation (the automap `map` nodes, `extractEnumValue` strings).
The typer is lite's own inference engine, judged by those outcomes, with a written rule table and a deterministic tie
rule, not a copy of upstream's code. Every deliberate difference is a register row with its evidence. The program's
goals (D17): cleaner, more bulletproof, much less code, faster, cruft deleted.

**The shape is a conventional production compiler**: a front end built as rustc and Roslyn are, a back end built as
Calcite is. Each stage's output is a distinct type carrying its own identities:

| Stage | Output, and the identity it carries |
|---|---|
| lex, parse | syntax tree (protocol records, import scopes) with spans; wire trivia in a side record |
| world | declaration tables per kind, import groups per section, C3 linearizations and variance; function identity is the reference's own element name (its signature id), elements as typed full-name wrappers |
| resolve | a resolved tree: calls carry candidate declaration lists, dot access stays `Member(name)` until typing, every binder an id, every node an id |
| elements | typed declarations keyed by id |
| type | every node typed; every call one declaration with its recorded type and multiplicity arguments; every member its property id |
| mapping elaboration | per mapping set a typed relation expression; bindings keyed by a closed key type |
| inline, shape-evaluate | the typed tree; substitution by binder id with the recorded instantiation; no unification and no overload choice |
| store resolution | rewrites over the typed tree with explicit set, navigation and join identities |
| lower | the SQL IR, from one immutable rule table keyed by declaration |
| legalise, render | SQL text per dialect; each dialect declares its session settings |
| plan, run | a staged plan; results |

**The rules that outlive the plan:**

- **Strict** means: collect every diagnostic in one pass, poison the failed unit, fail the build on any error. Never
  abort on the first error; never accept bad input (rule 0b.9).
- **Every new stage gets its own IR type.** A phase boundary is proved by the type system, not checked at run time;
  every match over an IR family is exhaustive with no default (0b.10).
- **Every stage is its own build target** with only the dependencies it should have (0b.12).
- **Homework before code, from primary sources** (the pinned trees and the code, by file and line); **probe before
  switch**; **one variable at a time** (names change with the world held constant, and the reverse); **deletions land
  with the switch** (0b.1, 0b.2, 0b.5, 0b.4).
- **No string identity for a declaration; no caches for slowness before the algorithm is right** (0b.8). D10's
  per-model memo of demand-driven typing is semantics, not a cache.
- **Oracles are cited for what they can see** (0b.14): agreement between DuckDB and H2 is evidence for dialect work
  only; the reference lane is evidence for resolution and types only.

**What each oracle sees** (the plan's §1c, condensed; the lanes that run today and how they judge are in [`testing.md`](testing.md) §§2–3):

| Oracle | Sees | Cannot see |
|---|---|---|
| corpus rosters (DuckDB, H2) | the engine tests' expected values on their small fixtures | wrong rows on data the fixtures lack (orphans, NULLs, ties, milestone versions) |
| reference lane | legend-pure's resolution and types per call, on Pure source | engine input, store resolution, SQL, rows |
| engine-input differential | legend-engine's compile of engine-grammar input: accept or reject, chosen function, return type | rows |
| rejection corpus | programs legend-pure refuses, by error class | — |
| product-SQL snapshot | any change in emitted SQL | whether the old SQL was right |
| adversarial data, old against new | refactor-induced row changes on data built to separate right from wrong | a bug present in both |
| metamorphic tests (TLP, NoREC, PQS) | internal inconsistency of filter, project and count under NULL and three-valued logic | a consistent wrong answer |
| legend-engine execution | the engine's rows for the same model, query and data | engine bugs (registered, not copied) |
| PCT lanes | per-function value semantics | mapping and routing |
| DuckDB against H2 over one SQL tree | dialect legalisation differences | anything above rendering |
| soundness monitor | a result whose shape (columns, cardinality) contradicts the typer's type | — |

Several of these were planned, not built, when the plan was parked; the plan's catalogue (its W-items) and its
decisions D1 to D25, which register rows cite by number (S1 cites D10, S3 D13, S7 D15, S14 D6), are in git history.

Sources: `docs/EXECUTION_PLAN_2026_09_26.md` §0b, §1, §1c, §2 (git history: docs/HISTORY.md);
`docs/SEMANTICS_REGISTER.md` (its header and rows S1, S3, S7, S14).

## 8. Where to read next

1. [`frontend.md`](frontend.md): text to typed model, and the upstream rules for names and overloads.
2. [`backend.md`](backend.md): store resolution, lowering, the SQL IR, dialects, the plan and execution.
3. [`testing.md`](testing.md): how lite is judged.
4. [`docs/rust-port/ROADMAP.md`](../rust-port/ROADMAP.md): the port, its seams and phases, what Java remains.
5. [`docs/PLAN_AND_RESULT_MODEL_2026_10_18.md`](../PLAN_AND_RESULT_MODEL_2026_10_18.md): the plan and result model in
   full, with its measurements.
6. [`docs/SEMANTICS_REGISTER.md`](../SEMANTICS_REGISTER.md): every deliberate difference from upstream.
7. [`docs/UPSTREAM_BEHAVIOR.md`](../UPSTREAM_BEHAVIOR.md): how legend-pure and legend-engine behave, source-cited.
8. [`docs/TENETS.md`](../TENETS.md), [`docs/TENET_CHARTER.md`](../TENET_CHARTER.md),
   [`docs/WORLD_MAP.md`](../WORLD_MAP.md): the tenets in full.
9. `AGENTS.md` and `core/README.md`: today's Java map, the numbered invariants, the guard tests.
10. [`docs/GATES.md`](../GATES.md): the gate chain; read it before claiming anything is green.
11. [`docs/HISTORY.md`](../HISTORY.md): every document removed from the tree, and how to read it from git.
