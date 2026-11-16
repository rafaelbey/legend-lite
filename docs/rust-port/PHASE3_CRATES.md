# Rust port — phase 3's crates (2026-11-11)

**Status: the map phase 3 builds to, written with slice 1a (PHASE3.md, step 1).** The crates slices 1a and 1b fill exist
(`lite-base`, `lite-types` and the guards, `lite-guards`; `lite-error`, `lite-model`, `lite-builtin`, `lite-platform`, 2026-11-12;
`lite-names`, the names slice, 2026-11-12, §7; `lite-normalizer`, 2026-11-14, §9; `lite-element`, the element slice,
2026-11-15, §10);
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
| `compiler_element_type` | `lite-types` | `lite-base`, `lite-builtin`, `lite-model`, `lite-platform`, `lite-front`, `lite-sql` | `lite-sql`'s `SqlType`, `lite-javacompat`'s white space | **made** (slice 1a): reaches `lite-base`, `lite-sql`, `lite-javacompat`; since the element slice (2026-11-15, §10) also `lite-front` and `lite-builtin` (`Multiplicity::from`, `platform::erase_tds_row`) |
| `database` | `lite-database` | `lite-base`, `lite-error`, `lite-model`, `lite-dialect` | `lite-dialect` | a declared database type to its dialect |
| `compiler`, split (§2) | `lite-names`, `lite-element`, `lite-spec` | `compiler`'s line, and each part the parts before it | | D and the model index; F; G and G½. **`lite-names` made** (the names slice, 2026-11-12, §7): reaches `lite-base`, `lite-builtin`, `lite-error`, `lite-model`, `lite-platform`, `lite-types`; since the follow-up (2026-11-12, §8) it also holds `Calls`, `RelationalTypeInference`, `ViewSignatures`, `StoreCompiler.columnType` and `MilestoningStrategy`, with no new edge. **`lite-element` made** (the element slice, 2026-11-15, §10): reaches `lite-base`, `lite-builtin`, `lite-error`, `lite-front`, `lite-model`, `lite-names`, `lite-platform`, `lite-types`, and no spec crate |
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
   `KnowledgeLayer`) and name resolution (`NameResolver`, `BareNames`, `ResolvedNames`, `CoreImports`, `SynthFqn`) --
   and, by the owner's ruling of 2026-11-12 (§8), five helpers Java keeps in `spec` and `element` that reach only
   names-tier code and that E calls first: `spec/Calls`, `element/RelationalTypeInference`, `element/ViewSignatures`,
   `element/StoreCompiler.columnType` (the method alone) and `element/MilestoningStrategy`.
2. `compiler/element` → `lite-element`: F (`PureModelContext` as `Model`, the element compilers, `TypedClass` and kin).
3. `compiler/spec` → `lite-spec`: G and G½ (`SpecCompiler`, the typer, the inliner, the typed tree).

With `lite-element` not depending on `lite-spec`, F cannot call G: a compile error, not a convention (core/README.md
invariant 7, AGENTS.md invariant 6). Java's code has edges against this order today, each for its slice to re-home
(measured 2026-11-11 by reading the imports and qualified names):

| Edge | Where | What crosses |
|---|---|---|
| element → spec | `element/TypeClassifier.java:47` | `InferenceKernel.ENUM_METACLASS_FQN`, a constant: it belongs with `PlatformTypes` |
| element → spec | `element/Temporal.java:87-99` | helpers over `TypedSpec` (`TypedGetAll`): G's, living in F's package |
| names → element | `StatementInline.java` | `ModelContext` (F's). **`KnowledgeLayer.java`'s crossing is gone** (2026-11-12, §8): it was `RelationalTypeInference` alone (§7), now `lite-names`' own |
| names → spec | `StatementInline.java`, `LiteralMapUnroll.java` | `SourceSubst` (G½'s, living in the root). **`Calls` no longer crosses** (2026-11-12, §8): `StatementInline`'s `Calls.platform` is `lite-names`' own builder; `LiteralMapUnroll` never reached it |
| normalizer → spec, normalizer → element (the census's, 2026-11-12) | E's `Calls` (102 call sites in 11 files), `LiftedViews`' `ViewSignatures`, `MilestoningFacts`' `MilestoningStrategy` | **gone with §8**: E reaches only `lite-names` and `lite-types` for them |

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
| (b) needs a later layer | `Multiplicity.from` (the parser's protocol records: `lite-front`, whose records were being flipped); `PlatformTypes.eraseTdsRow` (`builtin`'s `NativeFn.RowGetter`); `handleRowClass`, `isStatementOnly` (`model`'s `FunctionId`, `builtin`'s `NativeFn`) | `Multiplicity.from` and `eraseTdsRow` ported by the element slice (2026-11-15, §10: `lite-types` gained its edges to `lite-front` and `lite-builtin`); `handleRowClass` and `isStatementOnly` left for their callers (the planner and H; the names slice's query pass) |
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
    differential met no such model. (Overtaken 2026-11-13 by the names fixes, item 7, ruling 29: Java's refusal names a
    type that names no class by its kind in words, and the port spells it the same; no `toString` in either.)
17. **The process-wide memos `BUILT` and `REFERENTS` are `Mutex<HashMap>`** (lookup, compute outside the lock, first writer
    wins, a trivial answer not kept), where PHASE3_DESIGN.md §2.5 item 7 said `RwLock`: they are written as often as read
    while a process warms. `KnowledgeLayer.derived` is a `TypeId`-keyed memo (its one caller is E's).

The names follow-up's (2026-11-12, §8), each the conservative path:

18. **`StoreCompiler` is split by method**: `columnType` is `lite-names`' (`store_compiler::column_type`); `tableSchema`
    waits for `lite-element` (F's, its one caller); `declaredType` (the DDL type, reaching `sql`) waits for its first
    caller (setup, testplan, G). Alternative: the whole class in `lite-names`, with a `lite-sql` edge it does not need yet.
19. **Each moved class is a module named for it** (`calls`, `relational_type_inference`, `view_signatures`,
    `store_compiler`, `milestoning`), not folded into the index or the kernel; `ViewSignatures.of` takes the arena its
    types live in (the type family's `&'a` records), and a column's name crosses as UTF-8 (`JStr::lossy`): a view column
    named with a lone surrogate would not survive, as everywhere the type family names a column (the differential met
    none).
20. **The type inference keeps Java's missing cycle guard**: a computed view column over a cyclic chain recurses until
    the stack is gone -- Java throws `StackOverflowError`, Rust aborts the process. Alternative: a depth guard with a
    named refusal (a divergence, a register row). The differential met no such model. (Overtaken 2026-11-13 by ruling 29
    and the names fixes, item 8: Java and Rust refuse the cycle by name, `ViewSignatures`' words, an identity set of
    the view columns being typed, no depth limit.)
21. **The Rust guard for "the one place a call is built" is a source scan** (§8), the parser's whole crate and the
    resolver's file exempt, `with_parameters` allowed, tests out of scope, the two files' counts pinned exactly.
    Alternative: a type-level guard (the record's constructor private to lite-front, the builder lite-front's), which
    moves `Calls` out of `lite-names` and against the owner's ruling.
22. **The differential's new questions are forms of their own** (`stores.tsv`, `calls.tsv`), so the models' and
    queries' forms frozen before are byte-identical; `columnType`, package-private in Java, is asked by reflection from
    `NameAnswers`; the call shapes are the callers' names at their arities (a site whose arguments are another call's at
    0 to 5), the names a site computes enumerated from the table it computes them from, and a sweep of every bare name
    the platform's tables know at 0 to 8.

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
| `KnowledgeLayer` | `lite-names` (`columnKind` since the follow-up, §8) | `builtin`, `model`, `protocol`, `error`, `Type.Primitive`; `columnKind`'s computed-view-column arm calls `RelationalTypeInference.infer`, F's package in Java, `lite-names`' since §8 |
| `SynthFqn`, `DerivedProps`, `RelationalKinds` | `lite-names` | `model`, `protocol` |
| `StatementInline` | waits for `lite-spec` | F's `ModelContext`, G½'s `SourceSubst` (its `Calls` is `lite-names`' since §8) |
| `LiteralMapUnroll` | waits for `lite-spec` | G½'s `SourceSubst` |

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

**Left for F and G:** `StatementInline`, `LiteralMapUnroll`, `Compiler.resolveQuery` (the front door that runs them and
`ValidateDesugar` after the resolution), `PureModelContext.resolutionUniverse`, `isSubtype` as F's method (its walk is
`lite_model::subtyping::is_subtype`, its lookup this crate's `Knowledge::class_def`), and the gate's `ModelBuilder.add`
of Phase E's products (`ModelIndex::add` is ported; E makes what it adds). (`KnowledgeLayer.columnKind`, listed here
first, is the follow-up's: §8.)

## 8. The names follow-up (2026-11-12)

The owner's rulings on the census of E and F (its asks 1 and 2, `port-scratch/efcensus/REPORT.md` §5): five classes
Java keeps in `compiler/spec` and `compiler/element` are `lite-names`', because they reach only names-tier code (the
store lookups, the type family, the model, the error crate, the resolver's built-call decision, the platform's forms)
and E is their first caller. PHASE3.md's names slice entry has the numbers.

| Java | Rust (`lite-names`) | Reaches | First callers |
|---|---|---|---|
| `compiler/spec/Calls` (117 lines) | `calls` (`platform`, `form`, `exact`, `carrier`, `infix_run`, `renamed`, `among`, `infix_run_among`, `like`) | `NameResolver.builtCallNames` (`resolver::built_call_names`, ported with the names slice), `platform.CoreFn`, the protocol records | 206 call sites: E 102 (11 files), G 98 (20), validation 5, `StatementInline` 1 |
| `compiler/element/RelationalTypeInference` (289) | `relational_type_inference::infer` | `StoreLookups`, the model | `KnowledgeLayer.columnKind`, `ViewSignatures`; H's `RelationalOpRows` later |
| `compiler/element/ViewSignatures` (148) | `view_signatures::of` (its types in an arena) | `StoreLookups`, the type family, the model, the error crate, the two above | E's `LiftedViews` |
| `compiler/element/StoreCompiler.columnType` (the method; the class's `tableSchema` waits for `lite-element`, `declaredType` for its first caller: it reaches `sql`) | `store_compiler::column_type` | the type family, the model | `ViewSignatures`; F's `tableSchema` later |
| `compiler/element/MilestoningStrategy` (51) | `milestoning::MilestoningStrategy` | `PlatformTypes.isProfile`, `TEMPORAL_PROFILE` | E's `MilestoningFacts`; G, H, `Temporal` later |
| `KnowledgeLayer.columnKind` (the names slice left it out) | `Knowledge::column_kind` | `RelationalTypeInference`, `RelationalKinds` | E's `DeclaredCoercions`, `UnionSynthesis` |

None reaches F: the brief's stop condition (a class that reaches F waits) met nothing. No crate edge moved
(`crate-layers.txt` unchanged: `lite-platform` and `lite-types` were `lite-names`' already). So §2's edge table loses
`KnowledgeLayer`'s names → element crossing and the `Calls` half of names → spec; `StatementInline` (F's
`ModelContext`, G½'s `SourceSubst`) and `LiteralMapUnroll` (`SourceSubst`) still wait for `lite-spec`.

**The one place a call is built, in Rust.** Java's `ArchitectureTest.callsAreBuiltOnlyByTheBuilder` names Java's class
and stays for Java. Rust's form is a source guard, as the address guard is (`ptr::eq` counts):
`rust/lite-guards/tests/guards.rs`' `calls_are_built_only_by_the_builder` scans every crate's production sources but
the parser's (lite-front builds calls before resolution: the parser and the protocol readers), and refuses a call
record (`AppliedFunction {`, a struct expression; a return type's `-> AppliedFunction {` is not one) anywhere but
`lite-names/src/calls.rs` (3, pinned exactly) and the resolver's `lite-names/src/resolver/values.rs` (1: the resolver
records, as Java's rule exempts `NameResolver`), and any use of the parser's call factories (`lite_front::nodes`,
`applied_full`, `operator_parts`) outside the parser's crate. `with_parameters` keeps the record and is allowed, as
Java's rule allows `withParameters`. Tests (`tests/`) are out of its scope, as Java's rule checks production classes
alone. Its detector has its own test (`a_call_construction_is_told_from_a_return_type`).

## 9. What the normalizer slice made (2026-11-14)

`lite-normalizer` (layer `normalizer`, tier 3, above `lite-names`; `rust/lite-guards/crate-layers.txt`: it reaches
`lite-base`, `lite-builtin`, `lite-error`, `lite-front`, `lite-model`, `lite-names`, `lite-platform`, `lite-types`, and
nothing of `element` or `spec`): step E, `ModelNormalizer.normalize` and its package, 15,351 lines of source and 8,054 of
unit tests (179), the differential 1,598. PHASE3.md's normalizer slice entry has the numbers.

| Java (`com.legend.normalizer`, at ad86fa895) | Rust (`lite-normalizer/src`) |
|---|---|
| `ModelNormalizer` | `model_normalizer.rs` (E.2-E.6; the entry is `lib.rs`' `normalize`) |
| `MappingNormalizer` (19 families) | `mapping_normalizer/`: `entry` (1, 2), `clean_sheet` (3), `xstore` (4, 7), `dispatch` (5), `relation_function` (6), `m2m` (8), `relational` (10-12), `table_backed` (13), `fields` (14), `filter` (15), `main_table` (16), `enumeration` (17), `alias_scope` (18), `instances` (19); family 9 is DON'T PORT |
| `JoinChainEmission`, `UnionSynthesis`, `AssociationSynthesis`, `RelOpTranslator`, `ViewRelation`, `GroupBySynthesis`, `XStorePureEnds`, `ModelJoinNesting`, `RelationReads`, `DeclaredCoercions`, `AggregateViewLift`, `M2mRouteGuards` | one file each, snake_case (`xstore_pure_ends::synthesize` is PARK-1's one function, MN:983-1059 with XS's `synthesize`) |
| `MappingPrePass`, `MappingClosures`, `ResolvedMapping`, `StoreSubstitutionRewrite`, `ImplicitInheritance`, `MappingValidation`, `LiftedViews`, `Pipeline` (with ROT's `PipelineView`), `MappingLedger`, `MappingFacts`, `SetKeyFacts`, `MilestoningFacts` | one file each |
| `DynaFnArms` (test-only) and `DynaFnRegistryTest`'s invariants | `dyna_fn.rs`: ONE exhaustive `match` (`arm`), ARMS and LANDINGS computed from it (ruling 31) |
| `MissProbe` | no module: `Option` at each funnel site, `neverFired` an internal `IllegalStateException` with its text (`error.rs`) |
| the throws and the catches (census §9.4) | `error.rs`: `Thrown` (`Compile`, `Java { class, message }`, and `Unported`, which no catch takes), the catch predicates by kind, `with_element`, `first_line` |

**Model methods it needed** (in `lite-model` and `lite-front`, never here; census §8): step 3's (`spec_children.rs`'s
`children`/`mapChildren`, `RelationalOperation`'s `mapChildren`, `Col.isEmbedded`/`bindSrc`, `aggregationAwareMain`,
`withClassMappings`, `withSynthesizedFrom`/`isSynthesized`, `MappingDefinition`'s canonical and common constructors
(the bindings' keyed lookups left to H), `KeyThread`'s four-argument form), and the fill's: lite-model's
`normalizer_methods.rs` (`PropertyMapping.propertyName`, `AssociationMapping.associationName`,
`AssociationPropertyMapping.propertyName`, `AssociationEndDefinition.targetClassFqn`, `FilterPointer.name`) and
`LinkedMap::remove`; lite-front's `normalizer_methods_e1.rs` (`ValueSpecification`'s Java `equals` as a key: the
records derive equality over every field, positions included, where Java's skips them -- PARK-1's agreement check needs
Java's) and `normalizer_methods_e2.rs` (`ConstraintDefinition.expression`). `MappingDefinition.position` is not built:
it is H's, an index (census §8).

**Identity** (census §9.1): `LiftedViews`' owners, functions, walls and expanding are `IdMap`/`IdSet` keyed by the
view's `Arc` (the index hands out one per view); `MappingClosures.first` dropped; `MappingPrePass`' self edge and AS's
`m == md` re-expressed (`sourceClass == className`, skip index 0). `ModelNormalizer:220`'s `d == dp` could not be
`same`: `lite-names`' adoption copies the definition, so the check compares the adopted property's statement nodes (by
value where a body has none) -- a clean fix is adoption sharing the `Arc`, or census §4's typed step (the owner's).

**Left for F** (`lite-element`): `PureModelContext` as the concrete `Model`, `ModelIntegrity`, GraphEdges' F-time
rule, the element compilers, the `Typed*` records; the keyed `NormalizationFacts` lookups F reads (PMC:488-535) and
`MappingDefinition.position` (H's); `ModelNormalizer`'s output is what F's `PureModelContext.from` takes, so F's
differential runs on this crate's answers.

## 10. What the element slice made (2026-11-15)

`lite-element` (layer `compiler/element`, the split's second part, after `lite-names`; `rust/lite-guards/crate-layers.txt`:
it reaches `lite-base`, `lite-builtin`, `lite-error`, `lite-front`, `lite-model`, `lite-names`, `lite-platform`,
`lite-types`, and no spec crate, so F calling G does not build): step F, `PureModelContext.from` and its package,
8,979 lines with 136 unit tests, the differential 1,175 and its support 2,112. PHASE3.md's element slice entry has the
numbers.

| Java (`com.legend.compiler.element`, at 18feff3fe) | Rust (`lite-element/src`) |
|---|---|
| `PureModelContext` and `ModelContext` (one implementation: no trait) | `model.rs`: `Model<'m>` (its inherent methods, P1-P10), `CheckedLayer`, `Walked`, `Overlay` (ruling 11) |
| `ModelIntegrity` | `model_integrity.rs` (its 13 checks in Java's order; `refuseDuplicates` is lite-names') |
| `GraphEdges` (GE1, GE2) | `graph_edges.rs` (GE3, the Cypher view, waits for the Cypher wave) |
| `TypeClassifier`, `FunctionCompiler`, `ClassCompiler` | `type_classifier.rs`, `function_compiler.rs`, `class_compiler.rs` |
| `StoreCompiler.tableSchema`, `EqualityKeys.isKeyStereotype` | `store_compiler.rs`, `equality_keys.rs` |
| `TypedClass`, `Property`, `TypedConstraint`, `TypedEnum`, `TypedParameter`, `TypedFunction` | `typed.rs` |

**The design** (each the conservative path where the choice is a person's; PHASE3.md's entry lists them for the owner):
the model borrows its `ModelArena` (`&'m`: the crate forbids `unsafe`, so it cannot own what its `Type<'m>`s point into;
the owning handle and its erasure are the FFI's) and owns its index; the classifier and the compilers are views made
per call; the element memos are `OnceLock<Result<_, Thrown>>` cells per FQN known at freeze, one `Arc` per overload;
`Reach` is a mutex map computed outside its lock; the declaration and implementation tables, the packages and the
universe's ordered list are built at freeze; `CheckedLayer` holds the boot's `Arc`s by identity, one `IdSet` per kind;
`Model` is `Send + Sync`, checked at compile time; the error type is lite-names' `Thrown` (a raw Java exception Java
lets escape is `Thrown::Java` with its class).

**Model methods it needed** (never in this crate; census §8): `lite-types`' `Multiplicity::from` and
`platform::erase_tds_row`; `lite-model`'s `NormalizationFacts` keyed lookups and `Function::java_equals` (Java's record
equality, the protocol records' positions left out, which `lite-platform`'s `DeclarationTable::of` reads too); the
`DatabaseDefinition` lookups beyond the default schema were `ModelIndex`'s already.

**The differential's boot.** `Compiler.boot()` and `normalizeWithSystem` (the planner's) are composed in the
differential's test support (`tests/support/boot.rs`), not this crate's API: the planner's slice moves them into its
crate. The codec records are FrontCodegen's element mode (`//core:element_records_rs`).

**Left for G** (`lite-spec`): `ModelContext.isDeclaredSubtype` (G½'s, a distinct relation: ASK 16), `Temporal`
(G, H, lineage, the planner), `StoreCompiler.declaredType`, `StatementInline` and `LiteralMapUnroll` (they reach F and
G½), `Compiler.resolveQuery`, the typer's slice's own first item (ruling 36, a lifted view's columns checked within a
family, in Java before G's oracle freezes); `ClassLayouts`, `EqualityKeys` beyond `isKeyStereotype` and
`RelationalOpRows` wait for I, testplan and H. G reads the model through `Model`'s methods: `find_class` hands out a
borrow, `find_function` the cell's `Arc`s (SpecCompiler's identity memo keys by them), `is_subtype` returns the walk's
cached failure. **The typed records' equality, for the owner:** `TypedFunction` derives `PartialEq` (and hashes by hand over a
subset), and its `definition` and `body` are protocol-holding records whose derived equality reads source positions,
where Java's record equality leaves them out (the protocol records' `equals`); `Property`'s stored default is the same.
Nothing in F compares two typed records by value (the differential's doubled `findFunctionById` lines were this, fixed
with `Function::java_equals`), but G's call equality (`TypedNativeCall`, `TypedUserCall`) and H's value-keyed sets will:
ported as the brief rules it (derived over every field), the position question G's slice decides first.
