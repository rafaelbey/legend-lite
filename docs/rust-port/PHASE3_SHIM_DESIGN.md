# Rust port — phase 3: the Java that stays, and typed records under the arena (2026-11-09)

**Status: a draft for the owner to rule on (§9). No port code.** It answers `PHASE3_DESIGN.md` question 13 and §1.6 item 3.
Nothing here has been committed.

## 0. Read this first: `main` has moved, and it changes the question

The brief asks what the Java `driver` and `judge` read from a typed tree. This worktree (`8c7de71fc`) sits on `cf73e6ef7`,
`main` as of 2026-11-01. Remote `main` is now **`770acc55e`, 436 commits later**. Two pull requests there did, in Java,
most of what this design was asked to weigh:

- **#83** (merged 2026-11-06): every test body is planned whole and run as one plan; the statement-by-statement walk closed
  for core and PCT.
- **#92** (merged today): the walk deleted, and **the `driver` and `judge` packages deleted**. Planning a test body is
  `testplan`, running one is `testexec`, the records between them `testexecutionplan`. `TestExecution.runProgram` is literally
  `TestRunner.runOpening(TestPlanner.plan(typed, runtimeFqn, options.planning()), ...)`.

The port branch has neither. So `ROADMAP.md`'s "What Java remains: the `driver` and `judge`", ruling 4 on `SystemDatabase`, and
part of the identity oracle frozen at `6475958e0` describe a tree that `main` has left.

**How each side was measured.** HEAD (`8c7de71fc`): a throwaway program over javac's Tree API, every reference resolved by
the compiler (0 errors in core; 38 in `wasm`/`native`, from their absent GraalVM and TeaVM jars). `main` (`770acc55e`): I
was not to touch the shared repository, so I read it through GitHub's API. A tree listing, and file text scanned in memory
by a throwaway script for mentions of moved classes (so **`main`'s numbers are name mentions, not resolved references**; §8).
Its build file `tools/deps/core-layers.txt` gives the exact statements about which library can reach which.

"**Moved**" below means a class phase 3 takes to Rust; "**staying**" a class Java keeps.

## 1. The answer on one page

| | HEAD `8c7de71fc` (javac) | `main` `770acc55e` (name mentions) |
|---|---:|---:|
| Staying production code in `core` that names a moved class (tools: §3.4) | 4,100 references, 47 files | 191 mentions, 15 files |
| Of which reads a typed node, step by step | 2,832 (69%), `driver` 2,582, `testdatagen` 237 | **0** once `setup` moves (one mention, `setup/CsvSeed.java`, a planning-side library) |
| What the runner may compile against | `driver` reaches the whole compiler | **`exec`, `exec_program`, `testexec`, `test_plan`: no moved library, by the build** |
| Lines moved in phase 3 | 140,398 (with `setup`, `testdatagen`) | **160,697** (`testplan` 13,251; `planner` 7,390, was 1,885) |
| Plan formats at the seam | `legend-lite-plan` v19 | v31, and `legend-lite-test-plan` v11 |
| Test methods reading internals | 1,315 of 5,492 (roadmap: 580–770) | not resolved; 130 files, 1,435 `@Test` (HEAD by the same measure: 108, 1,303) |

**Recommendation: port from `main`, and let Java receive plan-shaped answers only.** A *plan-shaped answer* is one call
returning data: a plan as JSON, a type's description, a list of errors. No typed node crosses the FFI. On `main` that is what
already holds in Java: the planning half is the code that walks typed trees, and it moves to Rust with the compiler; the run
half reads data. It answers `PHASE3_DESIGN.md` §1.4's open worry: the arena ruling stands, and no node needs an id for Java.

It also gives phase 3 a checkpoint the roadmap says it does not have: **every test body's plan is data with a JSON form**,
so Rust's plan can be compared with Java's, byte for byte, per question, before any switch.

## 2. Scope

| | Packages | Lines |
|---|---|---|
| Moves, as the roadmap lists | `compiler` (with `element`, `spec`, `spec.typed`), `normalizer`, `resolver`, `lowering`, `plan`, `planner`, `builtin`, `platform`, `lineage`, `validation`, root `Compiler`, `TypedQuery`, `HeldModels`; `PlanJson`'s writer | `main` 145,270 |
| Moves, new on `main` (planning-side by `core-layers.txt`: only planners reach them) | `testplan` 13,251, `setup` 1,106, `testdatagen` 1,070 | 15,427 |
| Stays | `exec` 6,468, `testexec` 6,066, `testexecutionplan` 4,088, `server` 3,860, `test` 1,465, `edge` 224, the doors `Execution`, `TestExecution`, `ExecuteOptions`, the test harnesses, `pct/`, `spec/` | |
| Gone | `probe/Shadow.java` (`probe` is reached by nothing; TRIAGE: "already scheduled for deletion") | |

`protocol/sql`, its `iq` and `cypher` name **no** moved class at HEAD, so the roadmap's open question about a Java copy of the
translator is not affected by this design.

## 3. Part 1, measured at HEAD

### 3.1 Who names a moved class

Excludes `PlanJson`/`MirJson`/`ModelReader` readers (1,011, data formats that stay) and the translators.

| Staying package | Refs | Most named |
|---|---:|---|
| `driver` | 3,438 | `TypedSpec` 834, `TypedNativeCall` 317, `SpecCompiler` 170, `ExprType` 137 |
| `testdatagen` | 277 | `lineage.ScanRelations.Rel`, `TypedSpec` |
| `server` | 159 | `Compiler`, `TypedQuery`, `Type` |
| `judge` | 79 | the `Type` family (69 of them) |
| `probe`, `test`, `setup`, root | 60, 35, 27, 25 | constants; `Compiler`; `ModelContext`; the doors |
| `exec` | 0 | (2 to `PlanJson`) |

### 3.2 What the callers do with them

Citations are `core/src/main/java/com/legend/` at HEAD.

| What the caller does | Refs | Representative | One call returning data? |
|---|---:|---|---|
| Compile, type or ask a query | 83 | `Execution.java:109`, `server/PureV1Api.java:695` | yes |
| Get or read a plan | 181 | `driver/JudgePlans.java:71`, `server/PureV1Api.java:863` | yes |
| Read a type | 575 | `driver/KindClass.java:100`, `driver/AssertVerdicts.java:1524` | yes, as data |
| Read the model | 243 | `driver/PlanAllocations.java:330`, `setup/CsvSeed.java:310` | yes, a named question each |
| Read a platform constant | 183 | `driver/AssertVerdicts.java:413` | yes; Java constants |
| Catch a compiler exception | 3 | `driver/PlanAllocations.java:201` | the error contract |
| **Subtotal** | **1,268** | | |
| Carry a typed node (parameter, local, list) | 875 | `driver/HostJudge.java:805` | no |
| Read a typed node's field | 788 | `driver/ConnectionLets.java:39`, `driver/SqlTextVerdicts.java:798` | no |
| Test a node's kind (`instanceof`, pattern) | 296 | `driver/BodyDriver.java:1499` | no |
| Walk `children()` | 32 | `driver/BodyCompiler.java:196` | no |
| Call a typed-tree helper (`Lets.bound`, the context reader) | 129 | `driver/BodyDriver.java:725` | no |
| Run a phase, or take the compiler off the query | 712 | `driver/BodyCompiler.java:289`, `driver/BodyDriver.java:2094` | no |
| Build a typed node with `new` | **0** | | (they call the compiler's helpers) |
| **Subtotal: truly walks typed nodes** | **2,832** | | |

### 3.3 What the body driver needs from a typed body (HEAD)

Read closely: `Execution.runProgram`, `BodyCompiler` whole, `BodyDriver` to `:1700` and its index beyond.

1. **The statements as typed nodes, to classify each**: effect, generator, let of which kind, frame, staged rows, raw grid,
   assert, value (`BodyCompiler.java:71-176`).
2. **The phases, re-run per statement and per assert side** on sub-trees it holds: inliner, resolver, lowerer, renderer
   (`BodyDriver.prepareValue:343`, `resolver:577`, `engineSql:620`, `BodyCompiler.java:289-292`).
3. **Literal arguments read off native calls** to learn a mapping, a connection, a feature flag (`planToString:718`,
   `toSqlString:508`, `planConnOf:1131`).
4. **New trees**, always through compiler helpers (`StagedRows.boundTo`, `ExecuteChainAssembly.chain`).
5. **Live database answers during planning**. "A verdicts segment is planned only after the effects before it have run: two
   facts are read from the live session at plan time" (`BodyCompiler.java:50-52`); `gridReferee:1369`, `keyReader:2126`,
   `establishContexts:1925`.
6. **The judges read `Type`**, not nodes: 69 of `judge`'s 79.

Item 5 is why, at HEAD, moving the walkers to Rust would not be a simple move: Rust would have to call back into a Java
session in the middle of planning. It is exactly what #83 removed: the probes were deleted one by one ("probe 0"), a raw grid
and a dynamic pivot became keyed plan steps.

### 3.4 The same question on `main`

`TestPlanner.plan` plans "with no session opened and no statement sent", enforced by `base/RunScope`. It reads the typed
tree (`testplan/*`, 30 files). `PlanTypesAreDataTest` walks every type reachable from `ExecutionPlan` and `TestPlan` and
refuses a function, an exception, `Object` or a foreign type: "Guard 1 is at zero". Every plan passes
`TestPlanJson.requireRoundTrip` before it leaves the planner. Build edges, from `main`'s `core-layers.txt`:

```
exec      base cache diagnostics error execution_plan json result sql values
testexec  base diagnostics error exec exec_program execution_plan json result sql test_plan values
test_plan base error execution_plan json sql values
```

The whole of staying production that still names a moved class (all 191 mentions):

| File | Mentions | What it does | The shim needs |
|---|---:|---|---|
| `server/PureV1Api` | 50 | compile, type, `resultType`, `target`, `plan`, `executionPlan`; prints types through `UpstreamRelationType` | §6 |
| `server/CreateClass` | 23 | `compileModel`, `query(...).resultType()`, then reads the columns of a `Type` | a read-only Java `Type` |
| `server/ModelInput`, `ModelChanges`, `DiagramService`, `PureLspServer` | 16 | `compileModel`, `compileErrors` | §6 |
| `test/PureTestRunner`, `ServiceTestRunner` | 23 | `resolveQuery`, `programFacts`, `queryResolved`, `withExecutionOverlay` | §6 |
| `Execution`, `TestExecution`, `ExecuteOptions` | 24 | composes the doors | §6; `PlanningOptions` as JSON |
| `setup/*` | 15 | planning-side | moves |
| `probe/Shadow` | 40 | | deleted |

Outside `core`, the build-time tools name moved classes too: `spec/src/gen` 42 mentions (TRIAGE already retargets those
generators to Rust), `wasm/.../planner/Boundary` 33 (phase 4: TRIAGE, "becomes the C-ABI layer"), `datacube/tools/offer-facts` 25,
`engine-client/tools/typefacts` 12. One walks a typed tree:
`OfferFacts.java:148-151` (HEAD), a loop to find which native a call resolved to. It needs its own small answer or a frozen output.

**One thing on `main` is not data.** `PlanningOptions.edge` is a Java interface called *during* planning
(`testexecutionplan/Edge.java`: `effect(texts, queries)`, `seed(texts)`; each answers records to plan at that step). A Rust
planner should not call back. It can return each such step's ledger texts beside the plan, and the Java door then asks
the edge and sets `extensionsByStep`, which the record already has. The planner reads `options.stores()` too, for a load's
digest; that is data.

### 3.5 Tests

**At HEAD, resolved and followed through helper methods and field initializers.** 5,492 test methods in 511 classes under
`core`, `pct`, `spec`, `parser-equivalence`.

| The deepest thing a test method reaches | Methods |
|---|---:|
| No moved class (through `Execution.run`, `Lean`, or nothing of the compiler) | 3,515 |
| A compiler exception class alone (the shim rethrows it) | 186 |
| The doors alone (`compileModel`, `query`, `plan`, a `ModelContext` held) | 173 |
| Plan records (`QueryPlan`, `QueryParameters`) | 245 |
| A `Type` | 58 |
| **Internals** (191 classes): a phase directly 832, `resolver`/`lowering` 325, `builtin`/`platform` 63, the model's elements 55, typed nodes and nothing deeper 40 | **1,315** |

506 methods in 62 classes name a typed node at some level. **The roadmap's "~580–770 retired" is about half what the code says.**
If the shim keeps nine more `Compiler` doors (§6), retirement falls to 1,151. I could not reconstruct how 580–770 was counted.

**At `main`, by file.** 759 test files, 264 name a moved class; 130 files (1,435 `@Test`) name something deeper than
door, plan or type; 34 files (328 `@Test`) name the typed package, the largest `compiler/spec/TypeCheckerTest` 125 and
`SpecCompilerTest` 62. They are unit tests of Java internals and retire with them.

**The runners, on `main`.**

| Runner | What it calls | Fate |
|---|---|---|
| `Lean.run`, `service`, `established` | `Execution.run(text, ...)`, `Compiler.query(Compiler.compileModel(...))`, `Execution.runOnProjectDatabases` | unchanged |
| PCT channel A, `PctPlanRoute` | `compileModel`, `query`, `executionPlan`, `resultType` | unchanged, over the shim. `ValueBridge` reads `Type` and three `ClassLayouts.SYNTHETIC_*` constants |
| PCT channel B, `ChannelB` | `parseSources`, filters out native declarations in Java, `buildModel(pruned)`, `NameResolver.resolveQuery`, `TestExecution.runProgram` | needs §6 rows 3 and 9 |
| Corpus, `MinimalCorpus` | `parseSources`, `withShapes`, `buildModule`, `withExecutionOverlay`, `resolveQuery`, `hasStatementEffects`, `planBody`, `runBody` | needs §6 rows 3, 4, 9, 10 |

## 4. The options

| | A. Plan-shaped answers; the walkers are Rust | B. Read-only FFI handles to typed nodes | C. A serialized typed snapshot | D. Keep Java's compiler for the test path |
|---|---|---|---|---|
| In one line | `testplan` moves; Java gets `ExecutionPlan`/`TestPlan` JSON | Java keeps `testplan` and walks Rust's nodes through handles | Rust sends the typed body as JSON; Java rebuilds and walks it | Product path switches to Rust; test bodies still planned by Java |
| On `main` this is | what the code already does | a reversal | a reversal | a transition, not an end state |
| Cost here | Port 13,251 + 5,505 (`planner`'s growth) + 1,070 lines (+14%). Most of it is a mechanical port of pattern matches and helper calls | Keep `testplan`. Generated Java views for 128 kinds; a downcall per field; the inliner, resolver and lowerer as node-to-node FFI | Keep 143 typed classes in Java, plus a codec for about 450 fields, both ways, since `testplan` calls the phases with sub-trees | Keeps every line that `testplan` reaches: in effect the whole 145K, until it is ported |
| Arena, query handle | Ideal: no node leaves; a query handle answers about six questions; `'q` erased once (§1.6 item 7) | `'q` escapes into Java objects that Java's type system cannot tie to the handle's life; a use after release aborts the JVM. Every node handle must keep Java `==` | Identity does not survive JSON: `origin`, shared nodes and identity maps need stored ids, which ruling 1 ended | None; two compilers |
| Roadmap's checkpoints | **Adds one**: `TestPlan` and `ExecutionPlan` JSON per question, Rust against Java, byte-exact, before the switch | None | A typed snapshot could be diffed, but it is a new oracle for a thing nobody needs | **Adds one**: the product path green on Rust while the test lanes are still Java's |
| Build order | One new step, §5 | `testplan` not ported; a large FFI surface first | Two codecs first | Two switches |
| Verdict | **Recommended** | No | No | Only as the order of A's two switches |

## 5. The recommendation, and the build order

**Option A, from `main`, with the two switches of D.** The reason is the numbers: on `main`, zero staying production reads a typed
node, and the build keeps it so. Porting the seam work again, or undoing it to leave Java a window on an arena, would both cost
more than the 20,000 lines `testplan` adds.

**The order, replacing `PHASE3_DESIGN.md` §3:**

0. **New. Take `main`.** Base→`main` changed 997 files, base→HEAD 1,777, and only **75 both**. But 27 files the port
   deleted were changed on `main`, 18 of them main sources (17 under `sql/dialect`, and `Lexer.java`), and 40 of `sql/dialect`'s
   71 files changed there: that delta is to be replayed into `lite-dialect` and `lite-front`.
   Then **re-freeze what was measured on the old tree**: `rust/oracles/identity` (its scopes are `Execution.runProgram`
   and `BodyDriver`; both changed), the `MirJson` dumps, `rust/BASELINE.tsv`. `compiler/spec` changed in 69 of 194 files,
   `lowering` 25 of 78, `resolver` 6 of 59, `compiler/element` 6 of 29, `normalizer` 1 of 29.
1. **New. A plan oracle, while Java exists**: for every question in the corpora, Java's `ExecutionPlan` JSON (v31) and
   `TestPlan` JSON (v11), under `rust/oracles/`, with run-specific values masked (`executionTraceID` moved 72 lowering ids in
   the identity runs).
2. As before: `identity`, `Type`, the source tree to `Arc`, records, `ModelIndex`, name resolution, boot, `builtin`.
3. The normalizer, F. 4. `TypedSpec`, the typer, the inliner; **stop and measure**. 5. The resolver, lowering, `plan`, `planner`.
6. **New. `testplan`, `setup`, `testdatagen`** with `TestPlanJson`'s writer. The Rust CLI answers both plan oracles byte for byte.
7. **Switch 1**: the shim's product doors. `Lean.run`, `Execution.run`, `pure/v1` run on Rust; PCT channel B and the corpus
   still planned by Java. A lane flag, never a run-time fallback.
8. **Switch 2**: `TestExecution`'s planning. PCT, channel B, the corpus on Rust. Delete the Java middle; retire about 1,150–1,300
   test methods with ledger rows.

**What would change this.** If the owner keeps the port on the 2026-11-01 tree, A means redoing #83 and #92 in Rust; I would then
choose D and leave `driver` on Java's compiler for the whole of iteration 1. If the plan JSON turns out not to be byte-stable
between runs beyond masking (§8), the oracle of step 1 weakens, though the design does not. If `main`'s planning code keeps
changing at this rate (436 commits in eight days), the port needs a rule for where Java freezes (§9, question 2).

## 6. The shim functions (replacing the roadmap's "about 8")

In `lite_dialect`'s style: UTF-8 strings in, an answer or the `ERR` form out. A handle is an opaque `long`. "Mentions" are on
`main`, production + tests.

| # | Function | In | Out | Java it stands behind (mentions) |
|---|---|---|---|---|
| 1 | `lite_model_compile` | model text | model handle | `Compiler.compileModel(String)` (17 + 371, all overloads) |
| 2 | `lite_model_compile_data` | `PureModelContextData` JSON | model handle | `compileModel(PureModelContextData)`, `ModelInput.Data` |
| 3 | `lite_model_build` | sources JSON, kept (source, element) pairs or null, tolerant flag | model handle, walls JSON | `compileModel(List<ModelSource>)`, `buildModel`, `buildModule` (27 + 15 test). A Java `ParsedModel` that was filtered or assembled is sent as its sources and the pairs kept: the parser's codec runs Rust to Java only, over protocol records |
| 4 | `lite_model_overlay` | model, runtime JSON, connection JSON | model handle | `PureModelContext.withExecutionOverlay` |
| 5 | `lite_model_errors` | text or data JSON | JSON list | `compileErrors` (3 + 15) |
| 6 | `lite_model_all_bodies` | model | JSON map | `compileAllBodies` (test) |
| 7 | `lite_model_target` | model, runtime or null | target JSON | `executesOn` |
| 8 | `lite_model_release` | model | | a `Cleaner` (§7.6) |
| 9 | `lite_names_resolve` | model or null, statements JSON, imports JSON, mode | resolved spec JSON | `resolveQuery`, `resolveNames`, `NameResolver.resolveQuery*` (5 + 11) |
| 10 | `lite_program_facts` | model, resolved JSON | facts JSON | `programFacts`, `hasStatementEffects` |
| 11 | `lite_query_type` | model, text or resolved JSON | query handle | `Compiler.query`, `queryResolved` (28 + 202) |
| 12 | `lite_query_ask` | query, one of `result_type`, `parameters`, `target`, `writes` | JSON | `TypedQuery`'s questions |
| 13 | `lite_query_execution_plan` | query, runtime, features JSON, server version, project flag | plan JSON v31 | `executionPlan`, `projectPlan` (8 + 63) |
| 14 | `lite_query_test_plan` | query, runtime, `PlanningOptions` JSON, ahead flag | test plan JSON v11 or null, ledger texts per step | `TestPlanner.plan`, `planAhead`; the door applies the `Edge` |
| 15 | `lite_query_text_plan` | query, runtime | `QueryPlan` JSON | `TypedQuery.plan` (11 + 78), for `pure/v1` |
| 16 | `lite_query_release` | query | | a `Cleaner` |
| 17 | `lite_live_handles` | | count | the gate of ruling 6 |

Plus, Java-side and data only: a read-only `Type` family built from JSON (13 records; used by `PureV1Api`, `CreateClass`,
`ValueBridge`), thin `Compiler`, `TypedQuery`, `ModelContext` shells over the handles, and the constants tests name.
Rows 2 and 15 can go when `pure/v1` moves in phase 4. **Not shimmed, retired with their tests:** `TypedQuery.body`, `expression`,
`specs`, `lower`, `lowerer`, `resolve`, `withBody`; `Compiler.lowerResolved`; `ModelContext.find*` as used by tests.

## 7. Part 2: typed records under the arena

Measured at HEAD by javac, every instance field and record component. Five of `compiler/spec/typed`'s 100 files changed on
`main`: `TypedTds` gains a list of `Parameter` (two ints and a text), `TypedSerialize` a `boolean`, `FreeVars`, `Literals` and
`TypedSerializeGraph` are the others. No new group appears. **Set B below is pre-`testplan`:** Option A adds `testplan`'s own
per-query records (its `BodyDriver.ExecEnv`, what `HostAsserts` and `JudgePlans` hold while planning). Re-run on the merged tree.

### 7.1 The census

Three sets. **A**: everything reachable as a field from `TypedSpec`'s permitted records, 128 types, 434 fields. **T**: the rest of
the `typed` package, 15 types, 20 fields. **B**: the records in `compiler.spec` and `resolver` that hold a typed node, with what
the brief named (`ClassSource`, `Substitution`'s, `CallInstantiation`), 73 types, 292 fields. No field of any of them is
non-final. Three walker classes (`ContextReading`, `FreeVars`, `Substitution`) are not records and are left out.

| Java field type | A | T | B | All | Arena form |
|---|---:|---:|---:|---:|---|
| Primitive, enum, boxed | 85 | 0 | 19 | 104 | as is; `Integer` that may be null is `Option<i32>` |
| Reference to another node, `Optional` of one | 116 | 2 | 91 | 209 | `&'q T`, `Option<&'q T>`; a sealed root is a `Copy` enum of `&'q Record` |
| `String`, `Optional<String>` | 75 | 5 | 54 | 134 | `&'q str`, `Option<&'q str>` |
| The `Type` family | 86 | 0 | 23 | 109 | §7.3, needs a ruling |
| `List` | 52 | 1 | 27 | 80 | `&'q [T]` (53 nodes, 21 text, 3 integers, 1 column, 1 list of lists, 1 source node) |
| `Map` | 5 | 3 | 50 | 58 | an insertion-ordered slice, §7.2 |
| `Set` | 2 | 4 | 11 | 17 | same; `Set<Feature>` is a bitset |
| `BigDecimal`, `Number` | 5 | 0 | 0 | 5 | §7.2 |
| `Object` | 2 | 1 | 2 | 5 | `Origin`, §1.6 item 5 |
| Model tier (`TypedFunction`) | 2 | 0 | 8 | 10 | `&'q TypedFunction`, into a frozen `OnceLock` cell |
| Source tree, `SourceInfo` | 3 | 0 | 2 | 5 | `&'q` into an `Arc` the query keeps; `SourceInfo` a `Copy` struct |
| SQL tree (`SqlQuery`) | 1 | 0 | 1 | 2 | §7.2 |
| A function value | 0 | 4 | 4 | 8 | not nodes, §7.2 |
| **Total** | **434** | **20** | **292** | **746** | |

### 7.2 The fields that resist, and what to do

**A record needs `Copy` only if it is put in the arena.** Of B's 73, 55 are held by no other tier record: they are what a helper
returns (`NavigateChecker.Parts`, `FlattenOps.BelowSplit`). They can be ordinary Rust structs that borrow `&'q` nodes and own a
`Vec`. Arena-held are A, and the records the ruling names because they are aliased across passes.

| Field | Where (`compiler/spec/typed/` unless shown) | Problem | Proposal |
|---|---|---|---|
| `TypedFrameRef.plan` | `TypedFrameRef.java:28` | an owned `Box` SQL tree in a node | A small **keep pool** on the query context: `cx.keep(tree) -> &'q SqlQuery`; the pool owns and drops it. The same pool holds the `Arc`s below. A few lines in the arena's one `unsafe` module |
| `TypedTableReference.storedTypes` | `TypedTableReference.java:36` | a map of `SqlDdl.ColumnType` (`Plain(Kind)`, `Sized(String, int)`, `Scaled(...)`) | a `Copy` mirror with `&'q str`, or borrow it from the model's table, where the facts come from |
| `TypedCInteger.value` | `TypedCInteger.java:8` | `Number`: `Long` or `BigInteger` (`big()`, `:26`) | `enum IntLit<'q> { Long(i64), Big(&'q str) }` |
| `WindowFrame.Bound.{Preceding,Following}.n` | `WindowFrame.java:19,25` | `Number` | `lite-sql`'s `JavaNumber` (`mir.rs:522`) with text as `&'q str` |
| `TypedCDecimal.value`, `TypedCFloat.exact` | `TypedCDecimal.java:8`, `TypedCFloat.java:11` | `BigDecimal` | `{ negative, digits: &'q str, scale }`, the shape of `lite-front`'s `jtypes::BigDecimal` (`jtypes.rs:87`), with scale-sensitive equality (TRIAGE B2). `lite-sql` holds text (`mir.rs:626`): pick one at the first slice |
| `origin`, `frame` (5) | `TypedUserCall.java:32`, `Evaluation.java:27,105`, `spec/UserCallInliner.java:133` | `Object` | `Origin`, as ruled |
| `query`, `params`, `call`, `predLam`, `pos` (6, one a list) | `TypedCsvCensus.java:23`, `TypedTestDataGen.java:20`, `spec/GroupLambdaAggs.java:54`, `spec/NavigateChecker.java:444`, `TypedNativeCall.java:28`, `TypedUserCall.java:31` | `Arc` source nodes | `&'q`, kept alive by the pool |
| `callee` | `TypedNativeCall.java:27`, `TypedUserCall.java:30` | model tier; but **two sites make a per-query `TypedFunction`** (`spec/SignatureApart.java:67`, `resolver/ObjectReferenceArms.java:69`) | `&'q TypedFunction`; the two through the pool |
| Functions (8) | `ExecutionContext.java:306-312`, `resolver/StackBuilder.java:996-997`, `resolver/StoreResolver.java:3438`, `resolver/Substitution.java:108` | closures | none is a node: a reader, a builder, a scope. Stack values; `&dyn Fn` |
| Identity maps (3) | `resolver/StoreResolver.java:2142`, `resolver/Substitution.java:95,96` | `Map<TypedSpec, _>` | `IdMap`, owned by the resolver; the record borrows it |
| Written after construction | `resolver/StackBuilder.java:273`…`:1278` (`Col.perArm`, `Group.entries`, `Group.keyByRead`, `Embedded.tops`: 14 writes), `resolver/SyntheticHeads.java:574,1080` | mutated through an accessor | builders, never nodes: stack structs |

**Maps are not sorted slices.** The brief suggested one, but Java's are `LinkedHashMap` and order is seen: `ClassSource.bindings`
"declaration order is load-bearing" (`resolver/ClassSource.java:35`), and `TypedTableReference`'s constructor copies into one
(`:41`). The form is `&'q [(K, V)]` in insertion order, with a sorted index of positions beside it when the map is large
(`ClassSource.bindings` holds a class's properties and is looked up per property). Built once from a `Vec`, then frozen. Both are
`Copy`.

### 7.3 The one the ruling does not cover: the `Type` family

109 fields, one on nearly every node (`ExprType`). `Type` lives on **both** tiers: 6 model-tier records hold it (`TypedFunction`,
`TypedParameter`, `Property.Stored`, `Property.Derived`, `TypedClass`, `LayoutMemo`), and about 360 `new Type.*` sites make one, some in
static tables, some per query. `GenericType`, `FunctionType` and `RelationType` hold lists; `ClassLayouts`' memo is keyed by any
`Type` (`compiler/element/ClassLayouts.java:83-87`).

| | (a) Two families | (b) One `Arc` family, nodes hold `&'q Type` | (c) One `Copy` family, `Type<'a>` |
|---|---|---|---|
| Shape | a model `Type` (`Arc`) and a query `Type<'q>` | every `Type` an owned heap value, kept by the pool | `&'a str`, `&'a [Type<'a>]`; model values in a bump arena inside `Model`, query values in the query's |
| Crossing tiers | a conversion at each read | none | none: Rust shortens `&'m Type<'m>` to `&'q Type<'q>` by itself, because the type is covariant in its lifetime |
| Cost | `Type.java` (736 lines) and `PlatformTypes.java` (816) twice | a `malloc` for each per-query type, on the commonest field | one more erasure in the `unsafe` module; and the model's arena grows while queries hold references into it: a `Type` entering `LayoutMemo` on a miss takes a mutex on the bump pointer, over chunks that never move |

**Recommendation: (c).** One definition, no conversion and no refcount. If the owner would rather not have a second erased arena,
(b) is safe and simple, and a slower-but-contained choice.

### 7.4 Plainly: is the arena ruling workable as ruled?

**Yes.** 447 of the 746 fields (primitives, nodes, text; `Copy` as they stand) need nothing but the spelling above. 80 lists and 75
maps and sets are mechanical. What resists is 35 fields and one type family (109 fields), each with a named answer, and no resister
is a node needing to be changed after it is made: **no field of any tier record is non-final**. The ruling needs two additions: the
keep pool (§7.2) and the decision on `Type` (§7.3).

**What would change that.** A typed node that must outlive its query, such as a cross-query memo of typed bodies (today
`SpecCompiler.memo` is per query); Option B, which lets `'q` escape to Java; or a measurement of §1.5 item 5 showing the pool or
map index dominates.

### 7.5 A consequence for the frozen-model ruling

`withExecutionOverlay` (`compiler/element/PureModelContext`, used per test by `ServiceTestRunner` and `MinimalCorpus`) makes a
new context over an old one. `PHASE3_DESIGN.md` §2 has no overlay. It needs one: a `Model` layered on an `Arc<Model>`.

### 7.6 A consequence for handles

Java's `TypedQuery` and `ModelContext` have no `close`, and the tests must stay untouched: about 230 `Compiler.query` sites hold one.
So release is by `java.lang.ref.Cleaner`, at garbage collection; its action posts the release to `on_big_stack`, because the `Arc` tiers drop
recursively and a Cleaner thread has a default stack (ruling 6). Two effects. The gate of ruling 6, "live handles zero at the end",
needs a forced collection first. And native memory is invisible to the JVM's heap sizing, so the shim should count live arena bytes
and request a collection past a threshold, as NIO's direct buffers do. The largest tree in §1.6's sample (340,000 nodes, a model's and
not a query's) at its 200 or so bytes a node would hold about 70 MB until the collector ran.

## 8. What I could not establish

- **`main` is unresolved.** Name mentions, not javac. Against HEAD by both methods: 2,382 mentions against 4,100 references (mentions
  miss a field read off a value). A zero between libraries is the build's and is exact; a count is not. The census should be re-run
  on the merged tree.
- **I built and ran nothing.** Whether `TestPlan` JSON is byte-stable across runs beyond the `executionTraceID`, I did not test. Round
  trip is `main`'s claim (`requireRoundTrip`), and its CI, which I did not re-check.
- **`testplan`, read in part.** I read `TestPlanner`, the door, `Edge`, `PlanningOptions`, the type guard's head. "It opens no session" rests
  on `RunScope`, the build and the pull requests' text, not on reading 13,251 lines.
- **The cost of the merge.** File overlap only. The semantic size of 40 changed dialect files, and how much of the frozen conformance sets
  move, are unknown.
- **The test count** does not follow calls through an overridden helper, and counts a `@TestFactory` as one.
- **`TypedSerializeGraph` and the other changed `typed` files** were compared by their record headers only. `compiler/spec` records changed on
  `main` were not re-censused.
- **`Number`'s real classes** in `TypedCInteger` and `WindowFrame` come from the javadoc and `lite-sql`, not from enumerating every construction.
- **Whether every assembled model fits §6 row 3.** `ChannelB`'s filter and `withShapes` do from reading them (per-section imports come with the
  source); nothing was built, and others were not looked for. There is no Java-to-Rust form of `ParsedModel` today (`ffi/FrontRead`, `FrontRecords`).
- **`testplan`'s own records** are not in §7's census (they join it under Option A).
- **FFI call cost, plan JSON size** on the largest body: not measured.

## 9. Questions for the owner

Each with my recommendation first.

1. **Port phase 3 from `main`: merge `770acc55e` (or later) into the port before G½ and H. Yes.** Alternative: stay on the 2026-11-01 tree
   and keep `driver` and `judge` in Java, which means keeping Java's compiler through iteration 1 (Option D as an end state).
2. **Where Java freezes. Freeze the planning middle on `main` at the merge, by agreement, with a dated list of what may still land; port any later
   change as a delta with its own row.** Alternative: port against a moving `main`, merging every week. Which programs on `main` are still to come
   (`GOALS_OF_RECORD`, the data-in-the-plan slices L2 to L8) I could not weigh; you can.
3. **Plan-shaped answers only, no typed node over the FFI (Option A). Yes.** Alternatives: §4's B or C, for which I see no case on `main`.
4. **`testplan`, `setup` and `testdatagen` move in phase 3 (about 15,400 lines, and `planner`'s growth). Yes.** It takes phase 3 from about 140K
   to about 161K lines. Update the roadmap.
5. **A plan oracle, frozen before Java is deleted: `ExecutionPlan` and `TestPlan` JSON per question, and re-freeze the identity and MIR oracles
   after the merge. Yes.**
6. **Two switches, product path first, by lane flag. Yes.** Alternative: one switch at the end, as the roadmap's "no Java checkpoint" assumes.
7. **The `Edge` hook leaves planning: Rust returns each step's ledger texts, the Java door asks the edge and sets the records. Yes.** Alternative: move
   the compatibility edge's planning half to Rust.
8. **The §6 list of 17 functions replaces the roadmap's "about 8", with `Type` as a small read-only Java family built from JSON. Yes.**
9. **The retirement ledger is about 1,150 to 1,300 test methods, not 580 to 770, and is reviewed as one list before the deletion. Yes.** Alternative: keep
   more doors in the shim for tests alone (each buys back tens of methods, at the cost of a surface with no production caller).
10. **`Type` as one `Copy` family with a lifetime, the model's values in an arena inside `Model` (§7.3 c). Yes.** Alternative: (b).
11. **Add the keep pool to the arena module: `cx.keep`, for `Box` SQL trees and `Arc` source nodes. Yes.** Alternative: put `TypedFrameRef`'s frame in a side table by name.
12. **Only records a node holds, or that are aliased across passes, go in the arena; the rest are ordinary Rust structs that borrow. Yes.** Alternative: every per-query record
    in the arena, which makes the builder records (§7.2) awkward for nothing.
13. **Ruling 4's Java map for `SystemDatabase` falls away.** On `main` it is `exec/SystemDatabase.java`, "keyed by the graph's store key the plan names, never the compiled
    graph". Likewise `PHASE3.md`'s "`judge/InstanceIds` stays Java": no file of that name exists on `main`. **Recorded as withdrawn. Yes.**
14. **`Model` gains an overlay form for `withExecutionOverlay` (§7.5). Yes.** Alternative: rebuild a model per overlay, if measured cheap.
15. **Handles released by `Cleaner`, with a byte threshold and a forced collection before the live-handle gate (§7.6). Yes.** Alternative: add `close` and change the tests.

## 10. Rulings (the owner)

1. **Question 1, 2026-11-09: merge `main` before any porting.** Once the parser work of ruling 18 (PHASE2.md §6) lands, `main` is merged into this
   branch, its dialect and lexer changes are replayed into the Rust crates, the oracles and `rust/BASELINE.tsv` are re-frozen, and phase 3 ports from there.
2. **Question 2, 2026-11-10: the planning middle freezes on `main` at the merge.** From then on it takes only changes on a dated list the owner agrees;
   anything else that lands is ported into Rust as a delta with its own row. The runner, server and test-execution code can keep moving.

**Ruled 2026-11-10, after reviewing what from the test path is worth porting** (questions 3 to 15 of §9, and four the review added):

3. **The test path is this project's alone.** `testplan` and `testexec` exist to prove the compiler against PCT and the corpus; users will not run tests
   through them. Nothing is built for them beyond what those suites need.
4. **The Rust port has the host judge.** The database judge (`DatabaseJudge`, `DatabaseAsserts`, `VerdictBatch`, `VerdictSql`) is not ported at the start: its corpus
   passes and agreement mode stop at the second switch. It is ported later, as its own work, only if it is missed.
5. **SQL-text checks and their appeals are ported as `main` has them** (`SqlTextVerdicts`; the replay and comparison stay in Java).
6. **`testplan` is re-expressed, not transcribed**: written from what each piece must produce and judged by the plan oracle, `TestPlan` JSON byte for byte, and ported
   last. Measurement-only classes (`CanonicalDivergence`, `probe/Shadow`) are not ported.
7. **Questions 3, 4, 7, 8: finished answers only.** No typed node crosses the FFI. §6's 17 functions, a read-only Java `Type` built from JSON, the `Edge` hook out of
   planning; `testplan`, `setup` and `testdatagen` move in phase 3.
8. **Questions 5, 6: the plan oracle, frozen before Java is deleted, with the identity and MIR oracles re-frozen after the merge; two switches by lane flag**, the
   product path first.
9. **Question 10: `Type<'a>`, one `Copy` family with a lifetime**, the model's values in an arena inside `Model` (§7.3 c).
10. **Questions 11, 12: the keep pool is added; only records a node holds, or that passes share, go in the arena.**
11. **Questions 14, 15: handles are released by a `Cleaner`** with a byte threshold and a forced collection before the live-handle gate; **`Model` gains an overlay form.**
12. **Question 9: the retirement ledger, about 1,150 to 1,300 test methods, is reviewed by the owner as one list before any Java is deleted.**
13. **SDLC-lite and Depot-lite stay in Java and out of the port for now** (the owner's question, not §9's). The compiler port does not touch them; what Python's
    local home does without `lite_sdlc_handle` and `lite_depot_handle` is decided at phase 4.
14. **The freeze list is empty** (2026-11-10, ruling 2's list): no line changes `main`'s planning code (the compiler,
    normalizer, resolver, lowering, plan, planner, builtin, testplan) after the merge until its part of the port is done;
    the plan-as-data slices, the timeseries line and the corpus burn-down's compiler fixes wait, or land as an exception
    the owner reviews. The merge point is `main` as it stands when the merge is made (`c4a39e06d` on 2026-11-10).
    **Pinned 2026-11-10: merged at `c4a39e06d`** (`origin/main` as fetched that day, PR #94's merge; the merge commit
    `9c08abbc7`, its replay and re-freeze after it: docs/rust-port/PHASE2.md §4, "The merge of `main` at `c4a39e06d`").
    `main`'s planning code is frozen at that commit for the port.
15. **The Rust port's register rows take S300 to S499** (2026-11-10), beside the other lines' numbering blocks: at the
    merge, the port's S128 to S196 are renumbered into it, with every citation.
16. **The browser modules are re-frozen from `main`** (2026-11-11): `wasm/planner/` and `sdlc-server/page/` are built by
    TeaVM from pristine `main` at `c4a39e06d` and frozen, so they write `main`'s plan format (v31) and the tab's Output
    into a table works again; the list of inputs where the frozen module and the Rust parser differ is regenerated.
17. **The merge's own choices are ratified** (2026-11-11): own corpus 2,943 to 3,025 (a bigger corpus, not a parity
    change); the dialect oracles re-frozen by asking `main`'s dialects every frozen input again, with 1,065 inputs for arms
    none reached; the guard pins it moved, each a ledger row; `H2Modern`'s inputs re-asked of H2; the five removed docs
    `main` edited stay removed. The two allowlist entries for fields that no longer exist (`EngineStyleH2.frameDepth`,
    `anonDistinctDepth`) are removed.

Question 13 follows from ruling 1: `SystemDatabase` on `main` is `exec`'s and stays there, so `PHASE3_DESIGN.md` §6's item on it falls away, and so does `PHASE3.md`'s
note that `judge/InstanceIds` stays Java.
