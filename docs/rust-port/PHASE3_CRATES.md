# Rust port — phase 3's crates (2026-11-11)

**Status: the map phase 3 builds to, written with slice 1a (PHASE3.md, step 1).** The crates slices 1a and 1b fill exist
(`lite-base`, `lite-types` and the guards, `lite-guards`; `lite-error`, `lite-model`, `lite-builtin`, `lite-platform`, 2026-11-12;
`lite-names`, the names slice, 2026-11-12, §7);
the rest are named here and made by the slice that first fills each. The owner's ruling it follows: `PHASE3_DESIGN.md` §6 ruling 6 (`lite-core` split along
`tools/deps/core-layers.txt`, so that "F must not trigger G" is a compile error) and `PHASE3_SHIM_DESIGN.md` §10. Where a
boundary is a choice the layers file leaves open, it is listed in §5 for the owner.

## 1. The map

One crate per layer of `tools/deps/core-layers.txt` that phase 3 ports, but for one merge (`door` into `builtin`) and
one split (`compiler`, §2). A crate may reach what its Java layer's line lists, mapped to crates, and no more; it may
reach less (the layers are Java's; a Rust crate takes an edge when its code needs it). `rust/lite-guards/crate-layers.txt`
says, for each crate that exists, its layers and its exact dependencies, and `lite-guards` holds it (§3).

| Layer(s) | Crate | May reach (its layers' lines, mapped) | Reuses | Status |
|---|---|---|---|---|
| the JDK | `lite-javacompat` | -- | | phase 0; any crate may reach it |
| `base` | `lite-base` | -- | | **made**: the arenas, the keep pool, `identity` (slice 1a) |
| `json` | `lite-json` | (`//json`, below core) | | phase 1 |
| `sql` and `execution_plan`'s MirJson | `lite-sql` | `lite-json` | | phase 1 |
| `sql_dialect` and `rust_dialect`'s seam JSON | `lite-dialect` | `lite-sql`, `lite-json` | | phase 1 |
| `parser`, `protocol`'s records, `rust_parser`'s doors | `lite-front` | `lite-javacompat` (`error`, `spi`, `values` when needed) | | phase 2; its source tree `Arc` since 2026-11-11 |
| `error` | `lite-error` | `lite-base` | | **made** (slice 1b): the error contract, `LegendCompileException`'s phases; reaches nothing |
| `values` | `lite-values` | `lite-base` | | not made in slice 1b: nothing in `model`, `door`, `builtin` or `platform` reads `com.legend.values` (its records are `lite-front`'s) |
| `cache` | `lite-cache` | `lite-base` | `lite-front`'s `sha256` | the content `Hash` only; `HandleStore` is exec's and stays Java |
| `diagnostics` | `lite-diagnostics` | `lite-base` | | `testplan`'s verdict batch reads it |
| `model` | `lite-model` | `lite-base`, `lite-front` (protocol) | `lite-javacompat`'s white space | **made** (slice 1b): the model records (generated) and the converters from the protocol records; `ModelIndex` is the names slice's |
| `builtin` with `door` | `lite-builtin` | `lite-base`, `lite-error`, `lite-model`, `lite-front` | | `door` is 67 lines and an exact merge: every layer that reaches `door` reaches `builtin`, and `door`'s deps are within `builtin`'s. **Made** (slice 1b): reaches `lite-error`, `lite-front`, `lite-model` |
| `platform` | `lite-platform` | `lite-base`, `lite-builtin`, `lite-error`, `lite-model` | | **made** (slice 1b): reaches all four |
| `compiler_element_type` | `lite-types` | `lite-base`, `lite-builtin`, `lite-model`, `lite-platform`, `lite-front`, `lite-sql` | `lite-sql`'s `SqlType`, `lite-javacompat`'s white space | **made** (slice 1a): reaches `lite-base`, `lite-sql`, `lite-javacompat` today |
| `database` | `lite-database` | `lite-base`, `lite-error`, `lite-model`, `lite-dialect` | `lite-dialect` | a declared database type to its dialect |
| `compiler`, split (§2) | `lite-names`, `lite-element`, `lite-spec` | `compiler`'s line, and each part the parts before it | | D and the model index; F; G and G½. **`lite-names` made** (the names slice, 2026-11-12, §7): reaches `lite-base`, `lite-builtin`, `lite-error`, `lite-model`, `lite-platform`, `lite-types` |
| `normalizer` | `lite-normalizer` | its line (it reaches all three compiler parts) | | E |
| `lineage` | `lite-lineage` | its line | | |
| `validation` | `lite-validation` | its line | | |
| `lowering` | `lite-lowering` | its line | `lite-sql` | I |
| `execution_plan` (records, PlanJson's writer) | `lite-execution-plan` | `lite-base`, `lite-json`, `lite-sql` | `lite-sql`'s MirJson | the plan seam (v31) |
| `plan` | `lite-plan` | its line | | |
| `resolver` | `lite-resolver` | its line | | H |
| `setup` | `lite-setup` | its line | | |
| `test_plan` | `lite-test-plan` | its line | | TestPlanJson's writer (v11) |
| `testdatagen` | `lite-testdatagen` | its line | | |
| `planner` (with the root's `Compiler`, `TypedQuery`, `HeldModels`) | `lite-planner` | its line | | |
| `testplan` | `lite-testplan` | its line | | ported last, re-expressed (shim ruling 6) |
| not a layer | `lite-ffi` | any crate (`top`) | | the C library: the shim's 17 functions (shim design §6) |
| not a layer | `lite-guards` | -- (`top`) | | the guards of §3: tests only |

**Not ported** (they stay Java, by the build: `PHASE3_SHIM_DESIGN.md` §2, §10): `exec`, `exec_program`, `testexec`,
`result`, `edge`, `execution`, `test_execution`, `test`, `server_lib`, `saved_queries`, `arrow_ipc`; `pure_v1` waits for
phase 4; `ffi`, `rust_dialect` and `rust_parser` are Java's side of the FFI (`lite-ffi` is above every crate, where
Java's `ffi` is below its providers); `spi` is the parser's, which `lite-front` is; `probe` is deleted.

**Open, not this map's:** `protocol`'s translators (`protocol/sql`, `iq`, `cypher`) sit in the `protocol` layer, the
bottom; ROADMAP leaves whether the SQL→Pure translator moves in phase 3 to phase 3's decision (§5, item 4).

## 2. The compiler split

`compiler` is one layer and one Java library (the "compiler quartet": the root package, `element`, `spec`,
`spec.typed`), so the layers file cannot say "F does not reach G". The port splits it into three parts, in this order,
each reaching only the parts before it:

1. `compiler/names` → `lite-names`: the root package -- the model index (`ModelBuilder`, `SymbolTable`, `TableIndex`,
   `KnowledgeLayer`) and name resolution (`NameResolver`, `BareNames`, `ResolvedNames`, `CoreImports`, `SynthFqn`).
2. `compiler/element` → `lite-element`: F (`PureModelContext` as `Model`, the element compilers, `TypedClass` and kin).
3. `compiler/spec` → `lite-spec`: G and G½ (`SpecCompiler`, the typer, the inliner, the typed tree).

With `lite-element` not depending on `lite-spec`, F cannot call G: a compile error, not a convention (core/README.md
invariant 7, AGENTS.md invariant 6). Java's code has edges against this order today, each for its slice to re-home
(measured 2026-11-11 by reading the imports and qualified names):

| Edge | Where | What crosses |
|---|---|---|
| element → spec | `element/TypeClassifier.java:47` | `InferenceKernel.ENUM_METACLASS_FQN`, a constant: it belongs with `PlatformTypes` |
| element → spec | `element/Temporal.java:87-99` | helpers over `TypedSpec` (`TypedGetAll`): G's, living in F's package |
| names → element | `KnowledgeLayer.java`, `StatementInline.java` | `ModelContext` and the typed classes |
| names → spec | `StatementInline.java`, `LiteralMapUnroll.java` | `Calls`, `SourceSubst`: G½'s, living in the root |

`crate-layers.txt` gives a split with a line `split compiler names element spec`; `lite-guards` then allows
`compiler/spec` to reach `compiler/element` and `compiler/names`, and refuses the reverse (its test
`a_split_layers_parts_reach_only_the_parts_before_them`). **The line is in the file since the names slice (2026-11-12)**,
with `lite-names` (`compiler/names`) its first part: the guard checks that a part a crate names is declared, never that
every declared part has a crate, so the line stands before `lite-element` and `lite-spec` exist, unchanged.

## 3. How the graph is held

`rust/lite-guards` (tests only, `//rust/lite-guards:guards_test`, in `//gates:core`) holds the Rust graph to the layers
file as `//tools/deps:core_layering_test` holds Bazel's:

- **Exact, both ways, both builds.** Each crate's Cargo.toml `[dependencies]` are exactly its line of `crate-layers.txt`;
  under Bazel, so is each library's own answer (a genquery of `labels(deps, …)` per library, as `core/layers.bzl` makes
  them). An edge added or removed fails until the file says so. Cargo's run also holds the workspace's member list to
  the file's crates.
- **Allowed.** Every edge of the file is one the layers allow: some layer of the crate lists some layer of the
  dependency in `core-layers.txt`, or both are parts of a split in order; the JDK's spellings (`lite-javacompat`) are
  below every layer; `top` crates reach anything and nothing reaches them.
- **The sources** (PHASE3_DESIGN.md §1.6 item 6, §1.5 item 3): `unsafe` is written only in `lite-base/src/arena.rs` and
  `lite-ffi` (the C boundary), and every other crate root says `#![forbid(unsafe_code)]` (`lite-base`'s denies it and
  allows its arena module alone, with `clippy::undocumented_unsafe_blocks` denied); a pointer becomes an address
  (`as_ptr`, `{:p}`, `ptr::eq`, `Arc::ptr_eq`, `addr`, `addr_eq`, `expose_provenance`) only in
  `lite-base/src/identity.rs`, but for `lite-ffi`'s C strings and one test's `ptr::eq` in `lite-dialect/src/java_time.rs`
  (a count, so it cannot grow); `identity`'s maps and sets have no iteration method. Every crate's sources are handed
  to the test under Bazel too: `lite-front`'s were not until its exemption was lifted on 2026-11-11, the source-tree
  work landed (its `sources` filegroup and its root's `forbid`, PHASE3.md step 1).

## 4. What slice 1a made

`lite-base` (layer `base`): `arena` (`QueryArena` with its keep pool, `ModelArena`, the sealed `Alloc` trait) and
`identity` (`same`, `Identity`, `Addr`, `IdMap`, `IdSet`, `Origin`). `lite-types` (layer `compiler_element_type`): the
`Type<'a>` family. `lite-guards`. `lite-sql` gained Java's `SqlSource.Pivot.SEPARATOR` and `presented` and
`SqlExpr.TDS_NULL_CELL`, which the type layer reads from the `sql` layer in Java. PHASE3.md's slice 1a entry has the
numbers.

**The `compiler_element_type` layer, by what each operation needs** (the census behind `lite-types`' three edges):

| Bucket | Operations | In slice 1a |
|---|---|---|
| (a) a function of `Type`, strings and `SqlType` | `typeName` and every `text()`; `Primitive`'s table (`values`, `byFqn` in its order, `findByFqn`, `typeVariables`, `family`, `isNumeric`, `isTemporal`); `PrecisionDecimal` (its checks, `plus`, `minus`, `times`, `dividedBy`, `adjust`, `basePrimitive`); `relation`, `isRelation`, `classFqn`, `asClassType`, `relationSchema`, `schemaView`, `relationValued`, `requireRelationSchema`, `wireDecided`, `kindOfSqlType`; `RelationType`'s (`lateBound`, `erasedRow`, `isLateBound`, `isErasedRow`, `presentPivotName`, `pivotColumnType`, `trustedColumn`, its duplicate checks); every `PlatformTypes.is*` over a type or a string, `typeValuePath`, `functionTypeOf`, `lambdaType`, `concreteFunctionDefinitionType`, every constant and set; `Multiplicity`'s (`isMany`, `requireBounded`, `union`, `product`, `ofArgument`, the bounds' checks, `isUnbounded`, `isToOne`); `ExprType.one` | **ported**, held to Java's answers |
| (b) needs a later layer | `Multiplicity.from` (the parser's protocol records: `lite-front`, whose records were being flipped); `PlatformTypes.eraseTdsRow` (`builtin`'s `NativeFn.RowGetter`); `handleRowClass`, `isStatementOnly` (`model`'s `FunctionId`, `builtin`'s `NativeFn`) | left for the slice that ports `builtin` and `model` |
| (c) not a type's operation | subtyping: `ModelContext.isSubtype` walks a compiled model's classes (the primitive lattice is `Pure.java`'s `extends` chain; `Type.java` says it is "intentionally not re-encoded here") | not ported, not invented: F's slice ports it with the model, while Java still exists to be asked |

## 5. For the owner

1. **The compiler split** (§2): three parts, names < element < spec, and the four Java edges against it re-homed in
   their slices. Alternative: two parts (names with element), if F's slice finds the root and `element` too entangled.
2. **`door` merged into `builtin`** (exact, §1). Alternative: its own crate.
3. **The rule is "within", not "equal"**: a Rust crate may reach less than its Java layer (`lite-types` reaches three
   of six). The file is exact against Cargo and Bazel, as `core-layers.txt` is against Bazel.
4. **`protocol`'s translators**: where they go is phase 3's open question (ROADMAP), not settled here.
5. **Subtyping**: not on the type, so not in `lite-types`; frozen nowhere yet. If the owner wants the platform's
   lattice frozen now (`isSubtype` over the platform FQNs on the boot model), it is a few lines more in
   `rust/oracles/types/TypeOracle.java` and a model on its class path. (Overtaken 2026-11-12 by the owner's ruling 22:
   held to the live Java in slice 1b, not frozen -- item 11.)
6. **The model's records generated** (slice 1b): a third mode of `FrontCodegen`, held by its diff test, the records'
   methods written by hand beside them. Alternative: hand-written records, held only by the differential.
7. **A map on a model record is `Arc<[(K, V)]>` in Java's iteration order** (every model map is a `LinkedHashMap`'s view;
   lookups are scans or the builder's `LinkedMap`). Alternative: a hash map beside an order.
8. **`lite-values` is not made yet**: nothing ported so far reads `com.legend.values`.
9. **`DecisionProbe` is not ported** (a measurement probe, `ServiceLoader`-bound, a no-op unless `LL_SHADOW` is set).
10. **The tables' Java dump lives beside its frozen files** (`rust/oracles/builtin/`, a source program with its own copy
    of the record codec, as `rust/oracles/types/TypeOracle.java` does), never compiled by Bazel; the model differential's
    Java side is in core's test tree (`frontoracle/ModelAnswers.java`, beside the front oracle's, allowlisted in
    `ParserBoundaryArchTest` for naming a dialect, as they are).
11. **Subtyping (ruling 22)** agrees with the live Java on every class whose walk meets only FQN-spelled supertypes (366 of
    the boot model's 509); the 143 that meet a prelude supertype spelled by its simple name wait for name resolution
    (the names slice) and are not guessed. The live check is a test run by hand while Java exists, not frozen.
    (Closed 2026-11-12 by the names slice: all 509 agree, §7.)

The names slice's (2026-11-12, §7), each the conservative path where the choice is a person's:

12. **The protocol records' methods name resolution calls are `lite-front`'s** (`src/record_methods.rs`: `rawClassName`,
    the stereotypes' `withProfile`, `KeyExpression.withValue`, `AppliedFunction.withParameters`, `PathLiteral`'s
    `withChildren`, `GraphFetchLiteral`'s `arguments`, `withArguments`, `withClassNames`), as the model's are
    `lite-model`'s (`RelationalOperation`'s walk, `navigatesJoin`, `tables`; `PropertyBinding.withExpression`): where Java
    has them. Alternative: private helpers in `lite-names`.
13. **Subtyping is one walk with two shapes.** Java has two relations that answer differently: `KnowledgeLayer.isSubtype`
    (E's: raw heads, a primitive no class, no bottom) and `PureModelContext.isSubtype` (F's: compiled heads, `Nil` the
    bottom, a primitive's lattice walked, a failing class the question's failure), which does not delegate to the
    knowledge layer but asks it for the lookup alone. Rust keeps both relations, as answers, over one walk
    (`lite_model::subtyping::reach`, `Heads::Raw` or `Heads::Compiled`); the boot lattice moved to `lite-names`, which
    resolves the boot layer's names. Alternative: one relation, which would change one of Java's answers.
14. **What waits for F and G** (they reach F or G): `StatementInline`, `LiteralMapUnroll`, `KnowledgeLayer.columnKind`, the
    query front door `Compiler.resolveQuery` (it runs those and `ValidateDesugar` after the resolution), and
    `PureModelContext.resolutionUniverse` itself (F's; its answer, over the index, is `lite-names`' `IndexUniverse`).
15. **The query differential's universe is the step-D index plus the boot layer's resolved elements**, not the compiled
    model's (which also holds Phase E's products): `IndexUniverse` over `ModelIndex::from` of the adopted model with the
    boot layer added. The Java side measured it against the live compiled model's universe on every pair (§7).
16. **A generic type with type-variable values is not spelled by Java's `toString`** where `KnowledgeLayer.rawName` spells
    a non-name type into a refusal's message (an association qualified property returning a function, relation or
    schema type): the names layer refuses it loudly (`IllegalStateException`) rather than guess a value's spelling. The
    differential met no such model.
17. **The process-wide memos `BUILT` and `REFERENTS` are `Mutex<HashMap>`** (lookup, compute outside the lock, first writer
    wins, a trivial answer not kept), where PHASE3_DESIGN.md §2.5 item 7 said `RwLock`: they are written as often as read
    while a process warms. `KnowledgeLayer.derived` is a `TypeId`-keyed memo (its one caller is E's).

## 6. What slice 1b made (2026-11-12)

`lite-error` (layer `error`), `lite-model` (`model`), `lite-builtin` (`builtin` and `door`), `lite-platform`
(`platform`); PHASE3.md's slice 1b entry has the numbers. The shape each follows:

- **The model tier is `Arc`, generated.** The model's records are written by a mode of `FrontCodegen` from the Java's
  (`//core:model_records_rs`), as the protocol records are (`//core:front_records_rs`), so they cannot drift while
  Java exists and the canonical text both sides write comes from one schema. A record held as `Arc<R>`, a list
  `Arc<[T]>`, a map `Arc<[(K, V)]>` in Java's iteration order; equality derived (no model record writes its own).
- **The converters from the protocol records are `lite-model`'s**, as they are `com.legend.model`'s in Java; the text's
  door (67 lines) is `lite-builtin`'s, merged as §1 says.
- **The platform's generated tables are generated in Rust too**, by the generators that write the Java, from the same
  inputs (`//spec:gen_natives_rs`, `//spec:gen_dynafn_rs`); the resources are read as the same committed files.
- **The crates above the protocol layer reach its types through `lite-model`'s re-exports** (`JStr`, the protocol
  records, the codec): `platform`'s line in core-layers.txt lists no `protocol`.

## 7. What the names slice made (2026-11-12)

`lite-names` (layer `compiler/names`, the first part of §2's split): step D and the model index. PHASE3.md's names slice
entry has the numbers.

**Placement, measured by imports and qualified names** (each class of `com.legend.compiler`'s root package; "reaches"
counts every `com.legend.*` type a class names, imports and fully qualified references alike):

| Class | Crate | Why |
|---|---|---|
| `NameResolver` | `lite-names` | reaches `builtin` (`Pure`, `Prelude`, `DecisionProbe`'s no-op hooks), `model`, `protocol`, `error`, and `compiler_element_type`'s `Type.Primitive` (`lite-types`, a layer `compiler` lists): all below D |
| `BareNames`, `ResolvedNames` | `lite-names` | `builtin` (`Pure`, `EngineHandlers`, `Prelude`), `platform` (`CoreFn`), `protocol` |
| `CoreImports` | `lite-names` | generated (`//spec:gen_imports_rs`), reaches nothing |
| `ModelBuilder`, `SymbolTable`, `TableIndex`, `StoreLookups` | `lite-names` | `model`, `protocol`, `error`, `Type.Primitive` |
| `KnowledgeLayer` | `lite-names`, but for `columnKind` | `builtin`, `model`, `protocol`, `error`, `Type.Primitive`; `columnKind`'s computed-view-column arm calls F's `RelationalTypeInference.infer` -- the method waits for `lite-element` (only E calls it) |
| `SynthFqn`, `DerivedProps`, `RelationalKinds` | `lite-names` | `model`, `protocol` |
| `StatementInline` | waits for `lite-spec` | F's `ModelContext`, G's `Calls` and `SourceSubst` |
| `LiteralMapUnroll` | waits for `lite-spec` | G's `SourceSubst` |

So §2's edge table holds with one correction: `KnowledgeLayer` reaches F through `RelationalTypeInference` alone (its
other `compiler.element` references are `compiler.element.type`, `lite-types`, below the compiler); `StatementInline`
reaches F (`ModelContext`) and G (`Calls`, `SourceSubst`); `LiteralMapUnroll` reaches G (`SourceSubst`).

**What D hands on, and how it is held.** The live differential (`tests/names_differential.rs` against core's
`frontoracle/NameAnswers.java`, frozen as `rust/oracles/names/`): the resolved `ParsedModel` (strict and tolerant, the
compile's `resolveAlongside` and the bare `resolve`), the association adoption, and the index's answers to every
question E and F ask it (the calls into `ModelBuilder` and `KnowledgeLayer` from `normalizer` and `compiler/element`,
listed by reading them: the find methods, the iteration accessors, the include-closure store lookups, the view's main
table and lift, the legacy sets by id, the section scopes, the functions by id; the kernel's class lookup, subtyping,
ancestors, subtree, lineage, a property's type, definition, multiplicity and inline derivation), each record named by
its place in the model, never by a private field.

**Query resolution reads the index alone.** `NameResolver.resolveQuery` reads no model (the sectionless scope);
`resolveQueryIn`, through `Compiler.resolveNames`, reads a universe that `PureModelContext.resolutionUniverse` builds from
the index alone (`hasElement`, `hasFunctionId`, `primitiveExtensionFqns`, the platform's names). So shim function 9
(`lite_names_resolve`, "model or null") is served by `resolver::resolve_query` and `resolver::resolve_query_in` over an
`IndexUniverse`; the method that makes the universe from a compiled model is F's and waits.

**Left for F and G:** `StatementInline`, `LiteralMapUnroll`, `KnowledgeLayer.columnKind`, `Compiler.resolveQuery` (the
front door that runs them and `ValidateDesugar` after the resolution), `PureModelContext.resolutionUniverse`,
`isSubtype` as F's method (its walk is `lite_model::subtyping::is_subtype`, its lookup this crate's
`Knowledge::class_def`), and the gate's `ModelBuilder.add` of Phase E's products (`ModelIndex::add` is ported; E makes
what it adds).
