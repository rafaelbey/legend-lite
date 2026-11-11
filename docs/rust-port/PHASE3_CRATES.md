# Rust port — phase 3's crates (2026-11-11)

**Status: the map phase 3 builds to, written with slice 1a (PHASE3.md, step 1).** Only the crates slice 1a fills exist
(`lite-base`, `lite-types`, and the guards, `lite-guards`); the rest are named here and made by the slice that first
fills each. The owner's ruling it follows: `PHASE3_DESIGN.md` §6 ruling 6 (`lite-core` split along
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
| `error` | `lite-error` | `lite-base` | | the error contract, `LegendCompileException`'s phases |
| `values` | `lite-values` | `lite-base` | | |
| `cache` | `lite-cache` | `lite-base` | `lite-front`'s `sha256` | the content `Hash` only; `HandleStore` is exec's and stays Java |
| `diagnostics` | `lite-diagnostics` | `lite-base` | | `testplan`'s verdict batch reads it |
| `model` | `lite-model` | `lite-base`, `lite-front` (protocol) | | the model records, `ModelIndex` (step 2) |
| `builtin` with `door` | `lite-builtin` | `lite-base`, `lite-error`, `lite-model`, `lite-front` | | `door` is 67 lines and an exact merge: every layer that reaches `door` reaches `builtin`, and `door`'s deps are within `builtin`'s |
| `platform` | `lite-platform` | `lite-base`, `lite-builtin`, `lite-error`, `lite-model` | | |
| `compiler_element_type` | `lite-types` | `lite-base`, `lite-builtin`, `lite-model`, `lite-platform`, `lite-front`, `lite-sql` | `lite-sql`'s `SqlType`, `lite-javacompat`'s white space | **made** (slice 1a): reaches `lite-base`, `lite-sql`, `lite-javacompat` today |
| `database` | `lite-database` | `lite-base`, `lite-error`, `lite-model`, `lite-dialect` | `lite-dialect` | a declared database type to its dialect |
| `compiler`, split (§2) | `lite-names`, `lite-element`, `lite-spec` | `compiler`'s line, and each part the parts before it | | D and the model index; F; G and G½ |
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
`a_split_layers_parts_reach_only_the_parts_before_them`). No crate is split yet, so the file has no such line.

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
  (a count, so it cannot grow); `identity`'s maps and sets have no iteration method. Under Bazel `lite-front`'s sources
  are not handed to the test (its BUILD file belongs to the source-tree work in flight on 2026-11-11, dated in the
  test); cargo's run scans them.

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
   `rust/oracles/types/TypeOracle.java` and a model on its class path.
