# `core/` — the Legend Lite compiler and runtime

> **Read this before adding any file under `core/`.** This is the authoritative spec of core's layout and
> contracts. Its twelve [strong invariants](#strong-invariants) are cited by number from code: never renumber
> or remove one.

`core/` is legend-lite's compiler and the Java runtime around it. It parses Pure, compiles it, plans it to SQL,
renders the SQL for a database, runs the plan and writes the answer. It also holds the services, runtimes,
executor, JDBC plumbing and the HTTP server shell (`server/`). Tests run end to end inside this module.

It began as a [Strangler Fig](https://martinfowler.com/bliki/StranglerFigApplication.html) rewrite beside the
old `engine/` module (`com.gs.legend`). That module was deleted on 2026-08-11 (`AGENTS.md`), and `core/` is the
whole product.

**The Rust port.** On branch `rust-port` the planning path is moving to Rust
([`docs/rust-port/ROADMAP.md`](../docs/rust-port/ROADMAP.md); the crates are under [`rust/`](../rust/README.md)).
Iteration 1 replaces the planning path. The executor (`exec`) and the record layer it is walled to, the `server`
HTTP shell, and the `driver` and `judge` test runners stay Java. Phase 1 (the dialects) is under way: since
2026-11-03, `DuckDb`, `H2`, `H2Modern` and the three `EngineStyle*` dialects in `sql/dialect/` are Java shims
over the Rust library, reached through the `RustDialects` SPI that `ffi/` implements. The other ten dialects are
still Java on `AnsiSqlRenderer` ([`PHASE1.md`](../docs/rust-port/PHASE1.md) §4). This file describes the Java
tree as it stands. [Which invariants the port keeps](#which-invariants-the-rust-port-keeps) is below. The
language-neutral design is [`docs/architecture/README.md`](../docs/architecture/README.md).

## The wall — non-negotiable

**Nothing under `com.legend.*` may import anything under `com.gs.legend.*`** (the deleted engine module).
**Nothing in core, main or test, may import legend-engine's or legend-pure's Java (`org.finos.legend.*`).**

Two layers enforce this:

1. **Bazel.** `//core:core` reaches no external jar. It compiles against the JDK alone
   (`//tools/deps:core_closure_test`, `tools/deps/CoreClosureTest.java`). The database drivers are
   `//core:drivers`, a runtime choice: exactly DuckDB, H2, Postgres, SQLite and MongoDB's driver. Each core
   library's direct dependencies are pinned in `tools/deps/core-layers.txt`, and
   `//tools/deps:core_layering_test` compares that file with Bazel's own answer. The upstream jars are inputs of
   the tests and tools outside the product (`pct/`, `spec/`, `parser-equivalence/`, `tools/engine-runner/`).
2. **ArchUnit.** `ArchitectureTest.coreModuleHasNoDependenciesOnEngine` rejects any `com.legend..` →
   `com.gs.legend..` dependency. Nothing of `com.gs.legend` exists any more; the rule stays as the wall's pin.
   `ArchitectureTest.upstreamJavaNeverEntersCore` rejects any `org.finos.legend..` import in core's main or test
   classes.

The wall has no exceptions, no `@Suppress`, no shared utility classes and no generated bridges. **If `core/`
needs something, reimplement it in `core/`.** This is the cost we accept for getting it right.

## Pipeline

```
text                                                            [FRONTEND]
  ──▶  parser/       PmcdParser, SpecParser (the doors)          text → protocol records
                     (the Rust library behind them: rust/lite-front's lexer.rs, model_parser.rs, spec_parser.rs)
  ──▶  door/         ModelFromText (→ model/ModelFromProtocol)   records → parsed model
  ──▶  compiler/     NameResolver                                simple → FQN
  ──▶  normalizer/   ModelNormalizer                             Decl → Decl (mappings → fns)
  ──▶  compiler/     PureModelContext.from                       Decl → Def     (compiled model)
  ──▶  compiler/     SpecCompiler                                Spec + model → TypedSpec
                                                                [MIDEND]
  ──▶  resolver/     StoreResolver                               logical → physical TypedSpec
  ──▶  lowering/     Lowerer (+Fold/Scalars/Aggregates/Windows)  TypedSpec → sql.SqlQuery IR
                                                                [BACKEND]
  ──▶  sql/dialect/  SqlDialect.render(SqlQuery)                  SqlQuery → SQL string
                                                                [RUNTIME]
  ──▶  exec/         PlanRunner (+ result/ the writers)           a plan + a session → typed rows to a sink
```

| # | Step | Package | Driver | Output |
|---|---|---|---|---|
| 0 | bootstrap | `builtin/`, `platform/` | (static registry) | platform types + native sigs (`builtin.Pure`, the generated native signatures; `Prelude`, the platform's Pure declarations from `prelude.pure`) |
| A | lex | `rust/lite-front` | `lexer.rs` (behind the doors below; the Java `lexer/` deleted 2026-11-06) | tokens, never crossing the seam |
| B | parse elements | `parser/`, then `door/` | `PmcdParser` (the Rust library's section walk and grammars, `rust/lite-front`'s `model_parser.rs`; an overlay section's grammar called back in Java), then `door.ModelFromText` | `Protocol.PureModelContextData` records; the door (`model.ModelFromProtocol`, the same one a JSON model goes through) makes them `model.PackageableElement`s (`ClassDefinition`, `MappingDefinition`, …) wrapped in `ParsedModel` |
| C | parse specs | `parser/` | `SpecParser` (the Rust library's `spec_parser.rs`) | `protocol.spec.ValueSpecification` (`LambdaFunction`, `AppliedFunction`, `Variable`, `CString`, …) |
| D | resolve names | `compiler/` | `NameResolver.resolve` | (same shapes, FQN-rewritten) |
| E | normalize model | `normalizer/` | `ModelNormalizer.normalize` — `MappingNormalizer` is sub-slice E.1, not the entry | `NormalizedModel` (mappings desugared into `FunctionDefinition`s) |
| F | compile elements | `compiler/element/` | `PureModelContext.from` — there is no `ElementCompiler` class | `compiler.element.TypedElement` (`TypedNominal` = `TypedClass` \| `TypedEnum`; `TypedFunction`) + `ModelContext` |
| G | compile specs | `compiler/spec/` | `SpecCompiler` | `compiler.spec.typed.TypedSpec` (`TypedFilter`, `TypedProject`, …) |
| G½ | inline user calls | `compiler/spec/` | `UserCallInliner` | `TypedSpec` (β-inlined) |
| H | resolve store | `resolver/` | `StoreResolver` | `TypedSpec` (class queries → relational pipelines) |
| I | lower | `lowering/` | `Lowerer` | `sql.SqlQuery` IR (dialect-free) |
| J | render SQL | `sql/dialect/` | `SqlDialect.render`. Java on `AnsiSqlRenderer`: `Postgres`, `SQLite`, `MariaDb`→`MySql`, `Trino`, `ClickHouse`, `Snowflake`, `Databricks`, `BigQuery`, `MongoDb`. Shims over Rust: `DuckDb`, `H2`→`H2Modern`, `EngineStyleH2`→`EngineStyleDB2`→`EngineStyleComposite` | SQL string |
| K | execute | `exec/`, `result/` | `PlanRunner`, the writers | typed rows to a sink, each cell spelled at the boundary (the old door's `Executor`/`ExecutionResult` deleted 2026-10-27 and 2026-10-29) |

Wired by the root package's doors (`package-info.java`). `Compiler.compileModel` runs A–F once per model.
`Compiler.query` types a query against the compiled model (G, G½) and returns a `TypedQuery`.
`TypedQuery.plan(runtime)` gives the SQL in the runtime's dialect; `TypedQuery.executionPlan(runtime)` gives the
typed execution plan (H–J). `Execution.run` runs a plan on a session (K). `Compiler`, `TypedQuery` and
`HeldModels` are the plan side (`//core:planner`, which has no execution library); `Execution` and
`ExecuteOptions` are the execution side (`//core:driver`).

## Folder layout

Folder = package, 1:1. Single prefix `com.legend.`. Each package below is one Bazel library or part of one
(`tools/deps/core-layers.txt` lists the libraries and their direct dependencies). Counted on 2026-11-04:
976 `.java` files under `src/main/java`.

```
core/src/main/java/com/legend/
│
├── (the root)                         THE FIVE DOORS, and nothing else since D73: Compiler, TypedQuery, HeldModels
│                                      (the plan side) and Execution, ExecuteOptions (the execution side); the
│                                      product's API, the five classes named outside core. Execution.Source (D72):
│                                      where execution gets the session for the target the compiler decided.
│                                      package-info.java: the module's pipeline and invariant summary
│
├── builtin/                           phase 0: Pure (the generated native signatures), Prelude (the platform's
│                                        Pure declarations, src/main/resources/.../builtin/prelude.pure), NativeFn,
│                                        DynaFn, SystemMetamodel, EngineHandlers
├── platform/                          the platform's own Pure and its registrations (PlatformPure, Registrations,
│                                        DeclarationTable, ImplementationTable, WalledBodies)
│
├── parser/                            A,B,C. text → protocol records through the doors, and no compiler model
│   │                                    (ArchitectureTest 7c); the lexer and parser are the Rust library's
│   │                                    (rust/lite-front), the Java ones deleted 2026-11-06
│   ├── PmcdParser.java                      B door: a text → its Protocol.PureModelContextData (the engine's section walk)
│   ├── SpecParser.java                      C door: a text → protocol.spec.ValueSpecification
│   ├── RustParsers.java                     the doors' Rust provider, found by ServiceLoader (ffi/RustParserProvider)
│   ├── SectionGrammarRegistry.java          the overlay grammars (spi/SectionGrammar, by ServiceLoader), called back
│   └── Dialect.java, ParseException.java, OverlayElementSink.java
│
├── protocol/                          legend-engine's protocol as records (Protocol: PureModelContextData and its
│   │                                    Element records), the readers (ModelReader) and the emitters/composers
│   ├── spec/                                the value specifications (engine class names verbatim)
│   ├── sql/                                 the #SQL{}# island: Postgres SQL → Pure (SqlParser, SqlToPure);
│   │                                          iq/, the #IQ{}# island's
│   └── cypher/                              the #Cypher{}# island: Cypher → Pure (CypherParser, CypherToPure)
├── spi/                               the overlay seam: SectionGrammar, SectionSource, ElementSink
│
├── door/                              B → D. a text's records → the compiler's model (leg 8's last step, 2026-10-21)
│   └── ModelFromText.java                   PmcdParser's records through model/ModelFromProtocol, the door a JSON
│                                              model also goes through; the one place the parser and model meet
│
├── model/                             the compiler's parsed model: PackageableElement records, ParsedModel,
│                                        NormalizedModel, ImportScope, and ModelFromProtocol (records → model);
│                                        value specifications are protocol/spec's
│
├── normalizer/                        E. ModelNormalizer.normalize (entry); MappingNormalizer (sub-slice E.1:
│                                        mappings → synthesized FunctionDefinitions) and the other sub-slices
│
├── compiler/                          D, F, G. parsed → typed
│   ├── NameResolver.java                    D: simple name → FQN
│   ├── SymbolTable.java, ModelBuilder.java, …
│   ├── element/                             F: compiled elements and the model view
│   │   ├── PureModelContext.java                F driver (`from`); the model registry
│   │   ├── ModelContext.java                    the read-only model view (findClass, findEnum, findFunction, …)
│   │   ├── TypedElement.java                    sealed root: TypedNominal | TypedFunction
│   │   ├── TypedNominal.java                    sealed: TypedClass | TypedEnum
│   │   ├── TypedClass.java, TypedEnum.java, TypedFunction.java, TypedConstraint.java, TypedParameter.java
│   │   ├── ClassCompiler.java, FunctionCompiler.java, StoreCompiler.java, …
│   │   └── type/                                the type system: Type (sealed), ExprType, Multiplicity, PlatformTypes
│   └── spec/                                G: SpecCompiler (driver), Typer, InferenceKernel, UserCallInliner (G½),
│       │                                      and the per-native checkers, flat (FilterChecker, ProjectChecker,
│       │                                      ExtendChecker, GroupByChecker, … 42 *Checker classes)
│       └── typed/                               G output: TypedSpec (sealed root) and its variants, flat —
│                                                  TypedGetAll, TypedFilter, TypedProject, TypedExtend, TypedSort,
│                                                  TypedGroupBy, TypedJoin, TypedFlatten, TypedFrom, TypedSerialize,
│                                                  TypedVariable, TypedLambda, TypedNativeCall, TypedUserCall,
│                                                  TypedPropertyAccess, TypedNewInstance, TypedIf, TypedLet,
│                                                  TypedCast, …; TypedRelationOp and TypedStoreWrite are sealed
│                                                  families of their own
├── validation/                        the desugar of meta::relational::validation::validate
│
├── resolver/                          H. logical → physical TypedSpec
│   ├── StoreResolver.java                   driver: resolveObject + the post-condition no-escape walk
│   │                                          (invariant 9, StoreEscapees)
│   ├── ClassSource.java, ClassSources.java  mapping body → binding table
│   ├── Substitution.java                    β-substitution over bindings
│   ├── NavMaterializer.java                 navigate targets, recursive SubNav tree
│   ├── AssociationJoins.java                assoc ends → join material
│   ├── GraphEmission.java                   graph-fetch tree synthesis
│   ├── SyntheticHeads.java                  #fN/#dN join identities (JoinIdentity)
│   ├── TemporalContext.java, TemporalFrame.java   milestoning context propagation
│   └── Pipelines.java                       pipeline surgery helpers
│
├── lowering/                          I. TypedSpec → sql.SqlQuery IR
│   ├── Lowerer.java                         per-node dispatch; three authorities:
│   ├── Fold.java                            every fold-vs-isolate decision
│   ├── Scalars.java                         natives by resolved-overload key
│   ├── Aggregates.java + Windows.java       reducers / window functions
│   └── PureSql.java                         Pure type → SqlType boundary
│
├── sql/                               I output: the MIR — sealed, data-only records (SqlQuery, SqlSelect, SqlSource,
│   │                                    SqlExpr, SqlAgg, SqlType, SqlDdl, SqlDml, …); depends on nothing in
│   │                                    com.legend outside itself (invariant 12)
│   └── dialect/                         J. SqlDialect (the one entry) + AnsiSqlRenderer (the shared renderer), the
│                                          16 dialect classes, the rewrite passes, SqlWriter, DialectCapability;
│                                          RustDialects (the SPI the Rust-backed shims call)
├── ffi/                               the Rust library from the JVM (java.lang.foreign): Lite, RustDialect,
│                                        RustDialectProvider (the RustDialects implementation, found by ServiceLoader)
├── database/                          the plan-side owner of every per-database decision (Databases, Target)
│
├── lineage/                           column-level lineage and scanned relations
├── plan/                              the compile-only plan: QueryPlan (the rendered SQL, the root's typed contract,
│                                        the execution shape) and PlanText (legend-engine's plan text)
├── executionplan/                     the execution plan records (ExecutionPlan), their JSON (PlanJson) and the MIR's
│                                        JSON (MirJson) — the port's seams (docs/rust-port/ROADMAP.md)
├── planner/                           the plan machinery behind the plan-side doors (step C, D73, 2026-10-30):
│   ├── PlanMaker.java                       a typed query to its plan; MetamodelSeeds + OpSeeds the system store's
│   ├── QueryParameters.java                 rows; ProgramFacts; CrossStoreGuard. Part of //core:planner;
│   └── ...                                  depends on no execution class (ArchitectureTest.plannerIsExecutionFree)
├── setup/                             a connection's setup as text and rows (Ddl, CsvSeed, RowLoad, ProjectData)
│
├── exec/                              K. a plan + a session → typed rows to a sink (2026-10-29: the old
│   ├── PlanRunner.java                      executor, Executor.java, and its materialized result are gone)
│   ├── Sessions.java + SessionState.java    (the typed session seam: a session made here alone, its state its own,
│   │                                         D70; PlanSessions.java the sessions a plan's target names)
│   ├── Statements.java + JdbcStatements.java (the one place a statement reaches a database)
│   ├── JdbcDecode.java                      (a cell's typed value, read by the plan's column facts)
│   ├── WireTypes.java                       (the wire TABLE: JDBC's type codes to the SQL vocabulary, a SQL type's
│   │                                         Pure NAME; the runtime names a type, never holds one, D72)
│   ├── DatabaseConnector.java + Connectors   (each database's driver facts, its decode facts among them, D72)
│   └── *Connector.java                      (eleven JDBC connectors here; MongoDB's is in src/main/mongodb. exec
│                                             depends on NO planning library since D72, 2026-10-30: the wall,
│                                             ArchitectureTest.execReachesNoPlanning)
├── result/                            K's edge: ResultSink, Batch, CellText and the writers (CsvWriter,
│                                        JsonRowsWriter, PureV1Writer, TdsObjectsWriter, ArrowWriter)
│
├── judge/                             the verdict layer (D71, 2026-10-30; out of exec: planning-side work the
│   ├── PureAsserts.java + Equality.java     runtime must not carry): a test body's assertion judged over typed
│   ├── TdsCompare.java                      rows -- the host lattice, the grid compare, the canonical instruments
│   ├── CanonicalForm.java + ...Divergence   (CanonicalForm, CanonicalDivergence), the verdict batch, the SQL
│   ├── VerdictBatch.java                    replay referee, the censuses, the assert listener; reachable from the
│   ├── TypedSide.java                       verdict seam alone (the judges' collected side; ExecutionResult
│   │                                        until 2026-10-29)
│   └── Column.java + Row.java               (a judged side's column in the compiler's Type, and its row; exec's
│                                            until D72)
├── driver/                            the execution side's machinery behind the execution doors (step C, D73):
│   ├── BodyDriver.java + BodyCompiler       a test body as a sequence of plans (BodyDriver has one caller,
│   ├── HostJudge + DatabaseJudge + ...      Execution.runProgram); the judges' arms and their plans (JudgePlans,
│   ├── CsvLoad + SeedSqlForms + ...         AssertVerdicts, AgreementArm, SqlTextVerdicts); the seeds and loads;
│   ├── ColumnProbe + GridProbe + ...        the schema probes (D71: ColumnProbe, GridProbe, DynamicPivot,
│   └── SystemDatabase.java                  WireProbe); the graph's system database (D72). //core:driver
├── test/                              the Pure test runner (PureTestRunner, ServiceTestRunner) — product surface
├── testdatagen/                       meta::relational::testDataGeneration::generateTestData
│
├── server/                            the HTTP shell: LegendHttpServer, PureV1Api (legend-engine's pure/v1),
│                                        PureLspServer, DiagramService, ModelChanges
├── queries/                           the saved queries' and saved cubes' rules (SavedQueries, SavedCubes)
│
├── probe/                             Shadow, a probe of the platform untangle
├── cache/                             the one sanctioned cache: ContentStore, keyed by Hash (ArchitectureTest
│                                        cachesAreFunneledToContentAddressedStore)
└── values/  error/  diagnostics/      leaves: date/time literals; LegendCompileException and its kinds; the
                                         -Dlegend.diagnostics switchboard
```

Beside `src/main/java`: `src/main/duckdb`, `src/main/postgres` and `src/main/mongodb` hold the classes that ride
with a driver (DuckDB's appender load and cells, Postgres's cells, MongoDB's connector), so core itself compiles
against no jar. `src/main/ffi` registers `RustDialectProvider` for `ServiceLoader`. `src/main/resources` holds the
builtin tables (`prelude.pure`, `native-claims.tsv`, …).

## Naming conventions

| Convention | Example | Rationale |
|---|---|---|
| **Single top prefix** `com.legend.*` | `com.legend.parser.PmcdParser` | Distinct from `com.gs.legend.*` (the deleted engine); makes the wall unambiguous |
| **Folder = package**, 1:1 | folder `parser/` ↔ package `com.legend.parser` | One name per concept |
| **Noun packages** | `parser/`, `compiler/`, `resolver/`, `exec/`, `judge/` | Matches engine convention; matches Java idioms |
| **Element / Spec symmetry, everywhere** | `compiler/element/` + `compiler/spec/`; `TypedElement` + `TypedSpec` (the parser's spec records are `protocol/spec/`; F's driver is `PureModelContext`, there is no `ElementCompiler`; the Rust parser keeps the pair as `element_parser.rs` + `spec_parser.rs`) | "Element" = packageable element. "Spec" = value specification. Same pair end-to-end, no ad-hoc `Model`/`Query` mixing |
| **Pure data records** for every IR node | `record TypedFilter(...)` | No identity, free equality, free serialization |
| **Sealed roots** for every variant family | `sealed interface TypedSpec permits ...` | Compile-time exhaustiveness on every dispatch site |
| **Parser records = engine class names verbatim** | `LambdaFunction`, `AppliedFunction`, `CString` | Maximizes test portability against the engine corpus |
| **`Typed*` prefix** for everything `compiler/` produces | `TypedClass`, `TypedFunction`, `TypedFilter`, `TypedProject` | One uniform prefix for the typed world. Elements and specs both follow the rule — sealed roots `TypedElement` + `TypedSpec` |
| **No `FunctionCall(String, args)`** anywhere | each native is its own typed record | Forces enumeration; kills stringly-typed dispatch |
| **No `default ->`** in any sealed switch | every arm explicit, throw on unsupported | javac enforces variant coverage |
| **No `util/` package** | helpers live with the code that needs them | `util/` is a code smell; ArchUnit blocks it |
| **FQN strings, not live refs**, in long-lived fields | `superClassFqns: List<String>`, not `superClass: TypedClass` | Lazy loading; no transitive force-load |

## Strong invariants

**Enforcement is marked honestly.** `[ENFORCED]` = a test fails if you break
it. `[CONVENTION]` = the rule is real and expected, but **nothing checks it**.
Audited 2026-11-04: 5 of 12 are enforced, 1 half, 6 are convention.

> `ArchitectureTest` has **52 rules** (2026-11-04), nearly all of them dependency direction. It
> does not assert sealedness, record-ness or exhaustiveness. It
> also uses its own numbering ("6g", "7a-c") that does **not** map to this
> list. Do not merge the two numbering schemes. Cite its rules by name: line numbers drift.

1. **The wall.** No `com.legend..` → `com.gs.legend..` dependency.
   `[ENFORCED — ArchitectureTest.coreModuleHasNoDependenciesOnEngine]`; the upstream half
   (`org.finos.legend..`) by `ArchitectureTest.upstreamJavaNeverEntersCore`.
2. **No `util/` package.** `[ENFORCED — ArchitectureTest.coreModuleHasNoUtilPackage]`
3. **Sealed everywhere a hierarchy exists.** `PackageableElement`,
   `ValueSpecification`, `TypedElement`, `TypedSpec`, `Type`, `SqlQuery`,
   `SqlSource`, `SqlExpr`, `SqlAgg`, `SqlType`, `TypedSide` (the judges' side; `ExecutionResult` until 2026-10-29).
   `[CONVENTION]` — no sealed-or-final test exists.
   *(This entry previously named `Rel`, `ScalarOp`, `Dialect`, `NativeChecker`
   and `BindRule`, none of which exist in core.)*
4. **Records for all data carriers** under `model/`, `protocol/spec/`,
   `compiler/element/`, `compiler/spec/`, `sql/`, `plan/`. `[CONVENTION]` — no
   record-ness test exists.
5. **Sealed exhaustiveness over `default ->`** in `lowering/`, `sql/`,
   `resolver/`, `compiler/`. List every variant. Sanctioned exceptions:
   guarded-pattern switches need a coverage default (make it THROW), and
   best-effort rewrite walkers may pass unknown nodes through ONLY where a
   downstream loud wall is guaranteed. `[CONVENTION]` — enforced by javac only,
   and only when a sealed root gains a variant. No test scans for `default ->` itself;
   `ErrorShapeGuardrailTest.defaultLiteralFallbacksOnlyShrink` counts only `default -> "<literal>"`
   fallbacks (shrink-only), which is adjacent, not this rule.
6. **No `FunctionCall(String, args)` in the MIR.** `[CONVENTION]`
   *Corrected 2026-08-06:* this previously claimed "a grep test asserts no such
   record shape". **No such test exists, and the record does exist** —
   `model/RelationalOperation.FunctionCall` is the parsed `###Relational` dynaFunc
   node. That is legitimate; it is not MIR. The rule applies to `sql/` only.
7. **`F` (compile elements) MUST NOT trigger `G` (compile specs).** Function
   bodies stay as `ValueSpecification` inside `TypedFunction`, type-checked on
   demand. (Engine violates this in `buildPureFunctions`; we do not carry the
   violation forward.) `[CONVENTION]` — no test checks that F never types a body; `TypedFunction` has
   no compiled-body field (its javadoc). The adjacent behavioural guard is
   `architecture/NoEagerUserClassLoadsTest`, in core (rebuilt from the engine-era test): it hands
   `SpecCompiler.typeQueryBody` a fail-fast `ModelContext` proxy and fails if query typing looks up a
   user element the query does not reach.
8. **No mutable sidecar state across passes.** Each step takes input, returns
   output. No `IdentityHashMap<TypedSpec, ?>` threaded across phase
   boundaries; pass-local caches are fine, labelled and confined.
   `[CONVENTION]` — the nearest live rule is `ArchitectureTest.staticFieldsAreFinal` (with
   `staticCollectionStateIsImmutableOrRegistered`): every static field is final. It is adjacent, not the same.
9. **`TypedGetAll` and `TypedUserCall` MUST NOT survive `H` (resolver/).**
   `[ENFORCED — StoreResolver.assertNoStoreOnlyEscapees]` (the walk is `resolver/StoreEscapees`),
   called on every resolved statement, pinned by `StoreResolverTest`.
10. **No store-only node reaches the lowerer.** `TypedJoinSlot` / store
    navigates hitting `lowering/` are named "resolver bug" walls, not generic
    errors. `[ENFORCED — runtime]`
11. **`compiler/element/Typed*` reference other elements by FQN string, not
    live ref.** `superClassFqns: List<String>`, not `TypedClass`.
    `[ENFORCED — architecture/NoEagerTypeReferencesTest]`: it walks every class of core's product jars
    (`:product_jars`, with a floor of 498 classes) and fails on any field that holds a resolved `TypedClass`
    or `TypedEnum`, directly or inside a collection, map, `Optional`, array or wildcard bound. The only
    allowlisted fields are the registry's own, `PureModelContext#classCache` and `#enumCache`. (It is
    rebuilt for core; the engine-era original scanned `com.gs.legend` and was deleted with that module.)
12. **`sql/` is closed and pure data.** No `toSql()`, no dialect import, no
    `String` field encoding a SQL operation. `[HALF ENFORCED]` — the
    *dependency* half by `tools/deps/core-layers.txt` (the `sql` library depends on `base` alone;
    `//tools/deps:core_layering_test`), `ArchitectureTest.sqlLayerIsStandalone` and
    `sqlLayerIsFullyStandalone` (`com.legend.sql` depends only on itself and the JDK), and
    `compileSideLayersAreDialectBlind` (no compile-side layer sees `sql.dialect`). The
    "no `toSql()` / no SQL-encoding String field" half has no test.

### Which invariants the Rust port keeps

The port carries the contracts, not the Java shapes (`docs/rust-port/ROADMAP.md`; the language-neutral list is
[`docs/architecture/README.md`](../docs/architecture/README.md)).

- **Contracts the port keeps as they are:** 6 (no stringly-typed call in the MIR), 7 (F never triggers G),
  8 (no sidecar state across passes), 9 and 10 (store-only nodes end at the resolver), 12 (the MIR is closed,
  pure data; in Rust it is `lite-sql`, and the dialects render it).
- **Idioms with a Rust equivalent:** 3 (sealed roots: Rust enums are closed), 4 (records: plain data
  structs and enums), 5 (exhaustiveness: a `match` with no catch-all arm).
- **A contract whose representation is still open:** 11. No eager transitive load carries over; how Rust
  holds a reference to another element is the node-ID and `Arc` decisions (D1, D5) that ROADMAP phase 3
  settles before any code.
- **Java package layout:** 1 (the wall, as package names) and 2 (no `util/` package). So are the folder
  layout and the naming conventions above.

The guard tests below name Java classes, so they fail, rather than go vacuous, when a package is deleted. Each
phase retires or re-expresses the ones that reference its deleted packages, and the Bazel-query guards
(`CoreClosureTest`, `CoreLayeringTest`, `ArrowClosureTest`) carry over as checks of the crate graph
(ROADMAP, "The guard tests").

### The mechanical guards that do exist

| Test | Target | Enforces |
|---|---|---|
| `ArchitectureTest` | `//core:guardrails` | 49 rules, nearly all dependency direction (the wall and the upstream boundary, no `util/`, `sql/` standalone, package acyclicity, protocol at the bottom, exec reaches no planning, the compile side dialect-blind, caches funnelled, static fields final, …; "lexer JDK-only" retired with the Java lexer, 2026-11-06) |
| `CodeShapeGuardrailTest` | `//core:guardrails` | method ≤ 250 lines (the 3,500-line file limit was dropped on 2026-09-29); non-final instance fields allowlisted; shrink-only counts (dead private methods, class names as logic, …) |
| `ErrorShapeGuardrailTest` | `//core:guardrails` | no return/throw in `finally`; broad catches pinned per file; no control flow on exception text; `default -> "<literal>"` fallbacks only shrink |
| `ObservabilityGuardrailTest` | `//core:guardrails` | shrink-only ratchets on debug env flags, `System.err` prints, string-dispatch sites |
| `CarrierPurityRatchetTest` | `//core:guardrails` | 5 regex pins over `lowering/`+`resolver/`+`plan/` (list, array and UNNEST idioms built before the dialect) |
| `PlatformNamesGuardrailTest` | `//core:guardrails` | a Pure function or class name is spelled only in `PlatformTypes`; literal checks elsewhere only shrink |
| `NoEagerTypeReferencesTest` | `//core:guardrails` | invariant 11 |
| `NoEagerUserClassLoadsTest` | `//core:core_tests` (architecture package) | query typing looks up no user element the query does not reach (adjacent to invariant 7) |
| `TypedSpecChildrenTest`, `SpecChildrenContractsTest` | `//core:core_tests` | reflective `children()`/`withChildren()` totality |
| `CoreClosureTest`, `CoreLayeringTest` | `//tools/deps:core_closure_test`, `//tools/deps:core_layering_test` | core reaches no jar and the drivers are exactly the named ones; each core library's direct dependencies equal `tools/deps/core-layers.txt` |
| NullAway | every `legend_java_library` compile (`tools/java/defs.bzl`, `//tools/nullaway`) | a null-discipline finding is a compile error, unless the target opts out and its BUILD file says why |

## How to add a file under `core/`

1. Read this README.
2. Find the package in the layout above. **If your file doesn't fit any listed package, stop and discuss before inventing a new one.**
3. Records first; classes only for services (`SpecCompiler`, `StoreResolver`, ...) and per-step drivers.
4. Sealed root for any new variant family.
5. Run `bazel test //core:guardrails //core:core_tests` — `ArchitectureTest` (in `//core:guardrails`) must stay
   green. A new library also needs its line in `tools/deps/core-layers.txt` (`//tools/deps:core_layering_test`).

## Testing strategy

- **Unit tests** per package in `core/src/test/java/com/legend/<package>/`, one Bazel target per test package
  (`//core:core_tests_<package>`); `//core:core_tests` is their suite.
- **Pipeline tests** end to end through `Compiler.compileModel`, `Compiler.query` and `TypedQuery.plan` /
  `lower`, asserting SQL for golden Pure inputs.
- **Execution tests** end to end through `Execution.run`, asserting rows on the in-process databases;
  `//core:core_tests_lanes` runs the integration tests on SQLite and H2, each held to its lane's floor.
- **Parity with upstream** runs in its own modules, never in core: `pct/` (legend-engine's PCT suites,
  `//pct:pct_duckdb`, `//pct:pct_h2`, `//pct:pct_postgres`), `spec/` (the relational corpus, `//spec:corpus_duckdb`,
  `//spec:corpus_h2`) and `parser-equivalence/` (`//parser-equivalence:parser_parity`). Their upstream jars
  are test inputs there only. Which lane holds what: [`docs/GATES.md`](../docs/GATES.md); how lite is judged:
  [`docs/architecture/testing.md`](../docs/architecture/testing.md).

## Closed decisions (cited by number from code)

The open decisions of the parser build are all closed. Code still cites them by number:

- **D-1.** Pure bodies (derived properties, constraints, function and service bodies, M2M property RHS and
  `~filter`) are parsed eagerly into typed `ValueSpecification`s; no raw-`String` body field remains.
- **D-2.** Unknown top-level keys in `Runtime`, `RelationalDatabaseConnection` and `Service` bodies are a
  parse error naming the key. FINOS legend-engine's grammar rejects them too; only lite's old engine module
  skipped them.
- **D-3.** Test suites were held as raw text pending a test-suite grammar. A service's suites are now the typed
  protocol records, and that placeholder is retired (`model/ServiceDefinition`).
- **D-6.** A view's `~filter` takes all four grammar forms, as a sealed `FilterMapping` (`Direct` |
  `JoinMediated`) over a sealed `FilterPointer` (`Local` | `Cross`).
- **D-7.** A bare column identifier in a Database-context expression is rejected with "Missing table or alias
  for column 'X'", as legend-engine's `RelationalParseTreeWalker` rejects it.

> Cut in the 2026-11 docs cleanup: the open-decisions log D-1..D-7 (2026-07/08, with paths into the deleted
> engine module) and the Status / Phase B–C build record (2026-08-06: test counts, Maven wiring,
> `ModelOrchestrator` delegation). Full text: `git show de2745351:core/README.md`.
