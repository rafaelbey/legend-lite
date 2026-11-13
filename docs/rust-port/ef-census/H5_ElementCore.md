# H5: element core (F) -- PureModelContext, ModelIntegrity, GraphEdges, FunctionCompiler, TypeClassifier, ClassCompiler

All paths are under TREE/core/src/main/java/com/legend/ unless stated otherwise. "Strict" means the door
`Compiler.compileModel` -> `buildPositioned` -> `buildModel` -> `PureModelContext.from(..., wallSink=null, prior)`
(Compiler.java:75-118, 248-255). `compileErrors` (Compiler.java:904-930, called by server/PureV1Api.java:274,293 through
server/ModelInput.java:39,67) returns the FIRST element error alone, as its message with the element's `[line:col]` prefixed.
"Tolerant" means `Compiler.buildModule` (Compiler.java:532-539) with a `LinkedHashMap` wall sink. Its only non-test caller
is spec/src/test/java/com/legend/rcorpus/MinimalCorpus.java:296 (the corpus lanes). No product door reaches it.

## PureModelContext.java (858 lines), by family

| Family | Lines | Reached by | Reaches an answer | Idioms | Candidate |
|---|---|---|---|---|---|
| P1 Construction and the F gate: fields, ctors, `CheckedLayer`, `checkLayer`, `refuseDuplicates`, `from` | 38-190 | `Compiler.buildModel`:253 and `buildModule`:536 (5-arg `from`), `Compiler.boot()`:369 (`checkLayer`, which calls the 3-arg `from`:134), `Compiler.normalizeLayer`:279 (`refuseDuplicates`). Tests: testing/Phases.java:31, compiler/OneIndexTest.java:112. **Dead:** the public ctors :61 and :68 (no `new PureModelContext(` outside the file), the 4-arg `from` :169-173 | Yes. The integrity pass's first error goes to `compileErrors`, and the walls go to `buildModule`. `index.add(normalized.elements())` :184 is THE index G/H/I read. `retainLegacySurface` :187-188 feeds `findLegacyMapping` (P6) | **Identity:** `CheckedLayer.elements` is an `IdentityHashMap` set (:103-104; step 0 (b): 6,478,837 hits / 183,150 misses over 2,779 rows). `signatureKeys` `HashSet` :105, used by membership and `removeAll` only. The memo maps :83-86 (see the D5 table). `ConcurrentHashMap` for `derivedCache` :86 | PORT the gate's order (`index.add`, then integrity with `prior`). `CheckedLayer`: PORT as the one model-tier `IdSet` over the boot's `Arc`s (PHASE3_DESIGN §3, :522); see note P1. DON'T PORT the dead ctors and the 4-arg `from`. `refuseDuplicates`: RE-EXPRESS beside `ModelBuilder.duplicateElements` in lite-names (note MI1) |
| P2 Subtyping: `Reach`, `Walked`, `subtree`, `isSubtype`, `reach` | 198-266 | 50+ main call sites: compiler/spec/InferenceKernel (19), resolver/StackBuilder (5), ElementReferences (4), ChainNormalizer (4), AssociationJoins (3), testplan/JudgmentFacts (2), UserCallInliner, ReceiverOwnedFunctions, LiteralUnroll (2 each), planner/BodyPlanner, ClassLayouts:221, CypherIslands, ... `subtree` delegates to `KnowledgeLayer.subtree` | Yes. It decides type conformance (G), routing (H) and verdict facts (testplan). A rethrown failure is the query's error | Memo `Reach.of`, a `ConcurrentHashMap` in a derived fact (:204-207, :241, :264). **Exception as control flow:** `catch (LegendCompileException)` :257 (note P2). The `HashSet` at :246 is copied into a `LinkedHashSet` :263 and read with `contains` only (:228-229): safe | PORT, as its own function (not merged with `KnowledgeLayer.isSubtype`; see note P2). `Walked` becomes `(set, Option<Refusal>)` |
| P3 `derived` | 268-282 | 10 keys at HEAD: `Reach` (here), `LayoutMemo` (element/ClassLayouts.java:93,135), `OpSeeds` (planner/MetamodelSeeds.java:92), `Catalog` (compiler/spec/SqlIslands.java:99), `Labels`, `Ends`, `GraphEdges` (compiler/spec/CypherIslands.java:259,358,360,393,405,406,468), `Projects` (planner/PlanMaker.java:885), `SeedMemo`, `GraphKey` (planner/SystemRoutes.java:88,100) | Yes, through each fact (plans, SQL, seeds, the Cypher translation) | get-then-put `ConcurrentHashMap` :275-279, so a derivation can re-enter. The comment at :274 says "The derivation sees the BASE graph, never an overlay view", **but `derive.apply(this)` passes the overlay when it is asked first on one** (:277, the overlay shares `derivedCache` :578) | RE-EXPRESS as named fields (ruling 4): see the D5 table |
| P4 Element memos: `findClass`, `findClassDefinition`, `findEnum`, `findFunctionById`, `findFunction`, `findProperty`, `findFunctionDefinition(s)`, `findViewFunction` | 284-301, 371-440, 684-705, 729-733 | Every G/H/I/planner reader of the typed model (ModelContext's ~200 main importers, callers.txt); `findFunctionById` from compiler/spec/Overloads.java:1308 | Yes. These are the typed elements G types against | Memos `classCache`/`enumCache`/`functionCache` (`HashMap`, :292, :381, :411; never iterated). Java caches no failure, and `functionCache` also caches EMPTY lists for any FQN asked (bare names and misses: an open key set). `d.equals(tf.definition())` :396 is record (structural) equality, and here it equals identity, since `tf.definition()` is the instance `functionsAt` returned | PORT the answers with `OnceLock<Result<_, Refusal>>` cells for class, enum and function FQNs known at freeze (ruling 4). An FQN outside them answers empty, uncached. **Note P4** (two latent faults) |
| P5 Name universe and package index: `resolutionUniverse`, `elementFqns`, `isPackage`/`packages`, `functionFqns` | 309-369, 448-467, 828-833 | `resolutionUniverse`: Compiler.java:678 (`resolveNames`, by `contains` only: NameResolver.java:338,375,751,765), testplan/JudgmentFacts.java:117 (ITERATES). `elementFqns`: GraphEdges:126,131, CypherIslands:102,159, SqlIslands:177 (each wrapped in a `TreeSet`), compiler/spec/UserCallInliner.java:935, lineage/ScanRelations.java:2084, resolver/StoreResolver.java:659. `isPackage`: compiler/spec/Typer.java:1015. `functionFqns`: Compiler.java:868 (wrapped in a `TreeSet`) | Yes: name resolution, `Typer`'s package refs, and (by iteration order) a test plan's JSON and, on a mangling collision, SQL | **Hash order reaching output** (note P5): `HashSet` :330 (the `resolutionUniverse` iterator) and :346 (`elementFqns`). `packages` `HashSet` :467 is filled lazily at first ask (:450) and read with `contains` only. `functionFqns` `HashSet` :830 is sorted by its one caller | RE-EXPRESS: `resolutionUniverse` as a `contains` predicate plus an explicit ordered list for the one iterator; `packages` built at freeze (ruling 4). **ASK** on JudgmentFacts' order (note P5) |
| P6 Mapping fact readers: `findMapping`, `findLegacyMapping`, `unionMemberClasses`, `routedTargetClass`, `mixedUnionMembers`, `unionKeyThreads`, `mappingPoison`, `mappingSetPoison`, `mappingAssociationPoison` | 303-307, 474-537 | resolver/AssociationJoins, ElementReferences, ClassSources, CastReRoot, StackBuilder; compiler/spec/ImportDataFlow; normalizer/MappingFacts. `findLegacyMapping`: planner/MetamodelSeeds.java:731, lineage/ScanRelations.java:2277,2316 | Yes. The stamped `NormalizationFacts` feed H's routing and poisons, which are query errors | `getOrDefault(..., Map.of())` :495 is a lookup default, not a value fallback. LegacyReachbackCensusTest.java:107 pins this file at 4 `findLegacyMapping` sites (:304, :306, :607, :678) | PORT. It needs slice 1b's deferred `MappingDefinition` facts lookups (note "deferred") |
| P7 Execution overlay: `overlayRuntime`/`overlayConnection`, `findRuntime`, overlay ctor, `withExecutionOverlay`, `findConnection` | 55-59, 539-547, 566-614, 637-645 | `withExecutionOverlay`: test/ServiceTestRunner.java:434 (a main class that only tests reach: ServiceTestProvisionKeyTest, StressTool, CompileLatency, ...) and spec MinimalCorpus.java:302 (the corpus lanes). `findRuntime`/`findConnection` are ordinary lookups everywhere | Only on those harness paths: their two `IllegalArgumentException` texts (:598-601, :608-610) and the overlay's runtime and connection | The overlay shares the base's memo maps by reference (:575-578). The overlay's own `packages`, `declarations` and `implementations` start empty and are rebuilt | **ASK**: if the corpora run through the Rust CLI (PHASE3_DESIGN §2.5 step 5), this is a view struct `{Arc<Model>, runtime, connection}`. Otherwise it stays in the Java shim, DON'T PORT |
| P8 Platform tables: `declarations`, `implementations` | 616-635 | `implementations()`: TypedQuery, Compiler, planner/EnginePlans, BodyPlanner, lowering/SeedableLets, compiler/StatementInline, spec/Overloads, Typer, MapChecker, ... (15 files). `declarations()` has no caller outside the file | Yes. Every native dispatch reads the implementation table | Lazy fields (:618-623, :629-633), allowlisted in CodeShapeGuardrailTest.java:65. The input order is `Pure.all()` then `model.functions()` in ingest order (:620-621). Neither build throws: `ImplementationTable.build` collects `dangling`/`conflicts` lists (platform/ImplementationTable.java:63-245) | PORT. Build at freeze (Knowledge, ruling 4). It needs `Registrations` at freeze |
| P9 Metamodel registry: `TRACKED_CLASSIFIERS`, `tracksClassifier`, `classifierInstances` | 775-826 | `tracksClassifier`: resolver/ChainNormalizer, ElementReferences, testplan/BodyClass, ElementConsumption, AssertVerdicts. `classifierInstances`: planner/MetamodelSeeds, OpSeeds, lineage/ColumnLineageRows | Yes: the metamodel store's seeds (SQL rows) | `.distinct().sorted()` :825 is Java's `String` order (UTF-16 units, TRIAGE B5) | PORT, with the sort over UTF-16 units |
| P10 Store lookups and plain delegations: `findType`, `findTable`, `findTableMilestoning`/`milestoningWithIncludes`, `findView`, `viewMainTable`, `findDatabase`, `findJoinDefinition`, `findTableDefinition`, `findOwnTableDefinition`, `isDatabase`, `databases`, `findMeasure`, `findProfile`, `findData`, `findDataSpace`, `dataElements`, `isModelConnection`, `findAssociation*`, `isExecutionContextElement` | 192-196, 443-446, 469-472, 549-564, 647-682, 707-773, 835-856 | `findTableMilestoning`: resolver/TemporalFrame. `findTable`: setup/Ddl, testplan/EffectPlanner, spec/TableReferenceChecker. `isExecutionContextElement`: spec/Typer.java:1028. The rest are general | Yes (thin) | `milestoningWithIncludes` `seen` `HashSet` :746, membership only (an include-cycle guard). `findTable` is not memoized (`StoreCompiler.tableSchema` per call) | PORT as methods on `Model` (thin) |

### Notes, PureModelContext

**P1, `CheckedLayer`: semantic identity, as provenance.** `fresh = el -> !prior.contains(el)` (ModelIntegrity.java:57-58) skips
the boot layer's own instances, which joined the graph's index at the gate (:184, the boot's `sys.elements()` merged at
Compiler.java:483-488). A graph element at a boot FQN is a different instance and is checked. It is also reported as a
duplicate (`refuseDuplicates`, MI:63). `checkLayer` (:132-146) collects classes, functions (and their `FunctionId`s),
associations, enums, databases and mappings. It does NOT collect runtimes, services, data spaces, legacy mappings or
profiles, so any of those in the boot layer would be re-checked per graph (none observed; it would cost time only).

`without(adopted)` (:118-127) returns `this` when `adopted` is empty. Otherwise it returns a copy whose `signatureKeys`
omit the ids a graph adopted (Compiler.java:455-462: a graph function with a boot id takes the boot body through
`PlatformPure.adopt`, a new instance, so it is fresh and checked). Without the removal, `checkDuplicateSignatures`
(MI:345) would call the graph's adopted declaration "defined more than once". The copy also copies the identity set
(cost only).

The oracle covers both directions. Under-matching (boot elements treated as fresh) puts every boot function's key in
`priorKeys` and in `seen`, so every compile fails at MI:345. Over-matching (graph elements skipped) shows only on an
invalid graph model.

Skipping is not purely an optimisation. A graph association can inject a `<<graph::graph.from>>` end onto a BOOT class
(`GraphEdges.Index` is built over the whole index, MI:88-89), and the skipped boot class is never re-read. That is
contrived, but it is an answer difference from a full check.

Alternative to the `IdSet`: a boot-provenance flag per index slot, set when the boot's `NormalizedModel` elements are
added. It needs the layer to carry the boot slice of `normalized.elements()` (today a single merged list).

**P2, two subtypings (four walks), not one.**
- (a) `KnowledgeLayer.isSubtype`/`reach` (compiler/KnowledgeLayer.java:116-137; names tier; E's callers:
  normalizer/JoinChainEmission:295, MappingClosures:438, ImplicitInheritance:150,181, MappingNormalizer:2183,
  UnionSynthesis:458):
  - It reads declarations, native-first through `hierarchyClass`, where a PRIMITIVE is no class.
  - Heads come from `rawClassName`, and a structural head is skipped silently.
  - It has no `Nil` rule and never throws.
- (b) `PureModelContext.isSubtype` (:220-266; G and later):
  - It walks COMPILED classes (`findClass`). `classDef` is the same native-first lookup (TypeClassifier.java:76-78),
    but primitives are included, since the catalog declares their lattice nodes as classes.
  - `TypedClass.superClassFqns` comes from `TypeClassifier.headFqn`, which throws on a structural head, so the class
    fails to compile.
  - `Nil` is the bottom type (:228-229).
  - It rethrows the first compile failure on the walk, but only when the answer would be false (:234-236).
- (c) The `ModelContext` default `isSubtype` (ModelContext.java:387-450) is dead in main: `PureModelContext` is the only
  implementation and overrides it.
- (d) `ModelContext.isDeclaredSubtype` (:395-418) is live through compiler/spec/UserCallInliner.java:926.
  `UserCallInliner.declaredAncestors` (:931-959) is a fifth walk.

(a) and (b) disagree on primitives (`Integer <: Number`: false in (a), true in (b) by reading), on `Nil`, and on failures.
PORT them as two functions.

`rust/lite-model/src/subtyping.rs` ports (b) over declarations, but models only `headFqn`'s failure (:61-79). Java's
`catch` at :257 catches ANY `LegendCompileException` from `findClass`, for example `TypeClassifier.unknownType` for a
property typed by a missing class. In a strict build that cannot reach a fresh graph class, which integrity checked
(MI2). It can reach a tolerant build's walled-but-kept class (POISON-NOT-DROP, Compiler.java:517-531), and, not
established either way, a native catalog class whose declared types do not classify. So the Rust `find` must return
the element cell's cached `Refusal`, not `Option<&ClassDefinition>`, and the failure must be the class's whole compile
outcome. The rethrown object is the memoized exception instance, so the same message comes back each time.

**P4, two latent faults, by reading (not run).**
1. `findProperty` (:415-440) recurses into supers with no cycle guard. Strict builds refuse cycles (MI3), but a
   tolerant build keeps a walled cyclic class, and a miss there recurses until `StackOverflowError`.
2. An association end's multiplicity goes through `TypeClassifier.multiplicity` at :439 unchecked. MI's association
   check (MI:73-84) never calls `requireValidBounds`, so an end of `[2..1]` raises a raw
   `IllegalArgumentException("upper (1) must be >= lower (2)")` at G time.

**P5, hash order reaching output.**
- `JudgmentFacts.java:117` iterates `ctx.resolutionUniverse()` when a key's declared class is `Any`. The iterator is a
  `HashSet` built at :330. The order becomes `byClass`'s insertion order (a `LinkedHashMap`, JudgmentFacts.java:93,125),
  then `HostJudgment.Keys.byClass` (testexecutionplan/HostJudgment.java:49-53), then `HostJson.map`
  (testexecutionplan/HostJson.java:41-47,655), which keeps the order in the test plan's JSON (the frozen oracle's
  `test-plans-host`).
- No test source declares an `Any`-typed `<<equality.Key>>` (grep). Whether any frozen row reaches this branch is
  unknown: check `test-plans-host.tsv` for a `byClass` with many keys.
- Options:
  - (i) Sort the candidates (changes those rows if any exist).
  - (ii) Reproduce Java's `HashSet` order: `String.hashCode`, the table's capacity history, and the union with
    `platformFqns`. Feasible but ugly.
  - Owner's call.
- `elementFqns` (:346) reaches SQL only through resolver/StoreResolver.java:659 -> resolver/GenericTypeReflection.java:95 ->
  model/ClassMapping.java:72-80 `classOfWitnessPrefix`, the first match in iteration order. The mangling at
  ClassMapping.java:57-58 collides for FQNs that differ only in `::` vs `__` (`a::b__c` and `a__b::c` both give
  `stc_a__b__c___`). Then `HashSet` order picks the class name that is written into SQL (`typeValuePath`). Return an
  ordered set, and report that collision instead of first-match.
- Every other consumer is safe: wrapped in a `TreeSet`, `contains`-only, unique-match (ScanRelations:2084 returns null
  on two hits), or any-match over a `HashMap`'s values (UserCallInliner:868).

## The D5 mapping (PHASE3_DESIGN §2.5 and ruling 4)

PHASE3_DESIGN §2.1 lists "8 derived facts" including `SystemDatabase`. **It is stale at 7c438f7b3:** `SystemDatabase` is
gone (no main reference but javadoc: ExecuteOptions.java:20,79, MetamodelSeeds.java:17), and three keys were added. All
10 are pure functions of the model, so ruling 4's "SystemDatabase stays in Java on the shim's model object" has no
object left.

| Memo | Java | Knowledge / Work | Rust (ruled design) |
|---|---|---|---|
| `classCache` | `HashMap` :50, put :292. No failure is cached (a throw recomputes). Keys: class FQNs the classifier finds (model plus native catalog, primitives included) | Work, closed key set | `OnceLock<Result<TypedClass, Refusal>>` per class FQN known at freeze. A cached failure returns the same message Java recomputes |
| `enumCache` | `HashMap` :51, put :381. A `TypedEnum` is name plus values: no logic, no failure | Work by the ruling, though free | One cell per enum FQN, or built at freeze. A cell keeps the "cold element untouched" test (PHASE3_DESIGN §2.6) meaningful |
| `functionCache` | `HashMap` :52, put :411. It caches `List.of()` for ANY asked FQN (an open key set) and nothing on a throw (`compileAll` :91-93) | Work | A cell per function FQN known at freeze (model plus `Pure.nativeFunctionsAt` names). An unknown FQN answers empty without a cell: the same answer |
| `derivedCache` | `ConcurrentHashMap` :54,86, get-then-put :275-279. Shared with overlays | mixed | Named fields: `reach` = `Memo` (below). `layouts` = ClassLayouts' three `Memo`s. `op_seeds`, `catalog`, `labels`, `ends`, `graph_key` = one `OnceLock` each. `graph_edges` = `OnceLock<Result<GraphEdges, Refusal>>`, since `GraphEdges::of` can throw (`refuse`) in a tolerant build whose class was walled. `projects` (PlanMaker.java:907-909: an inner `ConcurrentHashMap` keyed by `platform/` or `declared/` plus the dialect's class name) and `seed_memo` (SystemRoutes.java:147-151: an inner map keyed by `DatabaseType`) are open-keyed: `Memo`, not a single `OnceLock` |
| `Reach.of` | `ConcurrentHashMap` inside derived :205-206, put :264. Keys: every FQN asked, including unknowns and primitives | Work, open | `Memo<Fqn, Arc<Walked>>` (a mutex-guarded map, computed unlocked, first writer wins). Its derivation calls `find_class` cells, never `reach` itself, so it cannot re-enter. `Walked` holds the cached `Refusal` |
| `packages` | `HashSet` :467, filled at the first `isPackage` (:450-462), `contains` only | Knowledge | Built at freeze (ruling 4) |
| `declarations`, `implementations` | lazy fields :43-44, :618-633. Neither throws | Knowledge | Built at freeze, or one `OnceLock` each. Freeze needs `Registrations` |
| `CheckedLayer` | Not a memo: a process-wide boot fact (`Compiler.BOOT`, a `ContentStore`, Compiler.java:293-373). An identity set plus a key set. `without` makes one per graph | Knowledge (boot) | `static OnceLock<Arc<Boot>>` holding an `IdSet` of the boot elements' `Arc`s plus `HashSet<FunctionId>` (membership only). `without` filters the key set per graph and need not copy the `IdSet`. The graph index must hold the boot's very `Arc`s, never clones |

## ModelIntegrity.java (639 lines), by family

The only caller is `PureModelContext` (:92 `check`, :153 `refuseDuplicates`). The class is package-private, and the
callers.txt "test" column is empty. **Its 3-arg `check` (:38-40) is dead.** On the strict door the FIRST failure in
`check`'s fixed order is the whole answer of `compileErrors`. The order is:
1. duplicates (`ModelBuilder.duplicateElements`, a `LinkedHashMap`)
2. classes, in ingest order (`ModelBuilder.classes()` = `elementOrder`, ModelBuilder.java:1255-1271)
3. inheritance cycles
4. functions
5. duplicate signatures
6. associations
7. GraphEdges, per non-native class
8. enums
9. databases
10. mappings
11. runtimes
12. services
13. data spaces

On the tolerant door the walls come in the same order (`LinkedHashMap`).

| Family | Lines | Reached by | Reaches an answer | Idioms | Candidate |
|---|---|---|---|---|---|
| MI1 Orchestration and walls: `check`, `refuseDuplicates`, `withElement` | 38-118, 212-247 | `PureModelContext` ctor :92 (every `from`); `refuseDuplicates` also through `PureModelContext.refuseDuplicates` <- Compiler.normalizeLayer:279 (BEFORE E, both layers) | Yes. The error (strict) or the wall map (tolerant). The wall key is `e.element()` when set, else the checked FQN (:238), and `Compiler.buildPositioned` prints that element's position | **Exception as control flow:** `catch (ModelException)` :234 (note MI1). `fresh` predicate over `CheckedLayer` identity :57-58 | PORT order and texts. RE-EXPRESS each check as `Result<(), Refusal>` and `with_element` as a fold |
| MI2 Class checks: `checkClass`, `realizedFqn`, `requireValidBounds` | 249-332, 418-420, 428-439 | MI1 :64-65 | Yes. Its messages: S91's "Invalid supertype ..." (:280-283), legend-pure's "Default values are supported only for mandatory fields ..." (:302-304), "Found duplicated property ..." (:289-292), "...: invalid multiplicity — <Bounded's message>" (:435-437), plus `FunctionCompiler`'s and `TypeClassifier`'s | **`catch (IllegalArgumentException)` :434** embeds `e.getMessage()`, which is `Multiplicity.Bounded`'s ctor text "upper (U) must be >= lower (L)" (element/type/Multiplicity.java:162-164; its "lower must be >= 0" at :160 cannot be reached, because protocol `Multiplicity.Concrete` refuses a negative lower first, protocol/Multiplicity.java:66-70). `HashSet propertyNames` :286, membership. `IllegalStateException` :278-279 escapes `withElement` (reachable only by an `extends ?` head, if the parser admits one) | PORT. RE-EXPRESS `requireValidBounds` as a direct bounds test that writes the same text (no catch) |
| MI3 Inheritance acyclic: `checkInheritanceAcyclic`, `walkSupers` | 454-494 | MI1 :66 | Yes: "Inheritance cycle: a -> b -> a" (:477-480), lite's words. The path order is the `LinkedHashSet path` insertion order. Which class reports depends on the ingest order and the `acyclic` memo | `catch (ModelException) { continue; }` :486, which produces nothing (a structural head is `checkClass`'s report). `HashSet acyclic` :464, membership | PORT. RE-EXPRESS :484-488 as `match head_fqn(sup) { Err(_) => continue, ... }` |
| MI4 Functions: `checkFunction`, `checkDuplicateSignatures` | 334-373 | MI1 :69-72 | Yes: "function 'f' is defined more than once with the same signature — calls would be ambiguous" (:349-351, lite's words), plus the classifier's errors | `HashSet<FunctionId> seen` :342, membership (iteration is over `model.functions()`, ingest order). `priorKeys` from `CheckedLayer` | PORT. **Gap:** no `requireValidBounds` on parameters or the return (compare MI2). See FunctionCompiler |
| MI5 Annotations and named references: associations (inline), enums and `checkEnum`, runtimes, services, data spaces, `requireAnnotations` x2, `profile`, `requireMapping`/`Runtime`/`Store` | 73-84, 92-98, 108-117, 120-210, 441-452 | MI1 | Yes: the engine's "Can't find ..." texts, "The store 'x' can't be found.", "Found duplicated value ..." | `HashSet seen` :443, membership. `rt.connectionBindings().entrySet()` :124 is a record `Map` (a `LinkedHashMap` view; lite-model keeps it as an ordered `Arc<[(K,V)]>`) | PORT |
| MI6 Mapping bindings: `checkMapping` | 375-416 | MI1 :101-102 | Yes: "mapping 'm' binds unknown class ...", "... binds to unknown function ...", "... must be realized by a function of the form (): Class[*]" | It iterates `md.classBindings()`/`associationBindings()` as lists only (declaration order). It does not need slice 1b's deferred `ClassBindings` index | PORT |
| MI7 Database refs: `checkDatabase`, `checkMilestoningColumnTypes`, `checkStoreRefs`, `checkColumnRef`, `hasColumn` | 496-637 | MI1 :99-100 | Yes: "The column ... can't be found in the table ...", "The table ... can't be found in the database ...", and the milestoning refusal (S26) | **A Java class name in an answer:** :553 writes `c.dataType().getClass().getSimpleName()`, i.e. "Other" or "Distinct", the simple names of `model.RelationalDataType` records. It calls `RelationalOperation.children()` :571 (not in TREE's lite-model) | PORT. RE-EXPRESS :553 as an explicit `"Other"`/`"Distinct"` per arm |

### Notes, ModelIntegrity

**MI1, `withElement` (:229-247).**
- Tolerant: `wallSink.putIfAbsent(e.element() ?? elementFqn, String.valueOf(e.getMessage()).split("\n")[0])`. The wall
  text is the first line of the message (Java `split`: TRIAGE C4). The first reason per key wins.
- Strict:
  - With `e.element() != null`, `e` is rethrown as is. Only `GraphEdges.refuse` sets it, to the DECLARING class, which
    may be a superclass of the class checked.
  - Otherwise a new `ModelException(e.phase(), e.getMessage(), elementFqn)` is thrown: the same text, element attached.
    `buildPositioned` prefixes the position.
- Only `ModelException` is caught. Others propagate unattributed, which is deliberate ("a genuine bug must fail the
  build", Compiler.java:529-530).

**refuseDuplicates (:220-227): names-tier work in F's file.** It reads only `ModelBuilder.duplicateElements()` (a
`LinkedHashMap`; the text "Duplicated element 'X'" is built in `ModelBuilder.internElement`, ModelBuilder.java:416-422).
It wraps each entry in `withElement`. It is called twice:
1. Before E, on each layer's own index (Compiler.java:279, so a duplicate's refusal is its own reason).
2. At the F gate (MI:63), for a graph element shadowing a boot FQN, recorded when the boot's elements joined the index
   in `from`:184.

Its home is `lite-names` (beside `duplicate_elements`), with the wall helper shared. `Compiler` is above every crate and
can call it either way. The answers are the same.

**Messages in lite's own words (no `SEMANTICS_REGISTER.md` row found by grep):**
- "Inheritance cycle: ..."
- "... is defined more than once with the same signature ..."
- "...: invalid multiplicity — ..."
- "... binds unknown class/association ..."
- "... binds to unknown function ..."
- "... must be realized by a function ..."
- TypeClassifier's "Unknown type: 'X' is not a known primitive, class, or enum" (:151-153)
- The headFqn texts (:305-310)

All of them reach `compileErrors`. Under AGENTS.md's upstream-fidelity tenet each is either upstream's text or a register
row. That is the owner's to rule, not this census's.

## GraphEdges.java (503 lines), by family

`public record`. Main callers: ModelIntegrity:88-91 (`sourceOf`, `Index.of`, `readOne`) and compiler/spec/CypherIslands.java:360,406,468
(`GraphEdges::of` as a derived fact). The test is GraphEdgesTest. CypherIslands is reached from a product door:
spec/Typer.java:218 -> SqlIslands.desugar :45-48 (language CYPHER) -> `CypherIslands.desugar`, and
spec/SpecCompiler.java:376-382 (Cypher updates). TRIAGE.md:22,78 puts the Cypher translator (`protocol/cypher`, 24.7K) in
a later wave.

| Family | Lines | Reached by | Reaches an answer | Idioms | Candidate |
|---|---|---|---|---|---|
| GE1 Records, `Source`, `Index`, `sourceOf` | 52-109, 147-171 | ModelIntegrity:88-89 (every F gate, every door) | Only as GE2's input | `Index.byOwner` `HashMap` :88, read by `getOrDefault` only (:103). Its lists are in association ingest order. `sourceOf.classes()` sorts (unused by `readOne`) | PORT the parts GE2 needs (`Source` as a trait or plain functions over the index) |
| GE2 The F-time rule: `readOne`, `EndRead`, `one`, `typeOf`, `STEREOTYPES`, `checkProfile`, `isMarked`, `lineage`, `is`, `typeMarked`, `toApplications`, `endKind`, `toOne`, `headName`, `spell`, `refuse` | 186-281, 283-502 | ModelIntegrity:90-91, for every fresh non-native class on every compile (strict and tolerant) | **Only for models that use `graph::graph` stereotypes** (`PlatformTypes.GRAPH_PROFILE`; `isProfile` is exact equality, PlatformTypes.java:443-451). Then a misdeclared edge is a `ModelException` naming the declaring class (`refuse` :499-502, element set). For every other class `readOne` returns null without throwing, so it is answer-free on valid models. The edge's `type` and ends are the Cypher island's answer (GE3) | `lineage` :415-435 is a BFS with `List.contains` (its own walk, a third copy beside `KnowledgeLayer.lineage`/`ancestorsAndSelf`). `LinkedHashSet seen` :195. Messages embed `List.toString` ("[a, b]") of the profile's stereotypes, its tags and `STEREOTYPES` (:396-398) | PORT, with F, since it runs on every door and its errors are `compileErrors`' (lite-only rule: "only lite checks the rule, since only lite runs the island", :49). **ASK alternative:** defer the whole reader to the Cypher wave with a ledger row (models with a misdeclared edge class then compile in Rust) |
| GE3 The Cypher view: `edgeAt`, `of(ctx)`, `read` | 111-145, 173-184 | `GraphEdges::of` only from CypherIslands (G) via `derived` | Yes, for a query with a Cypher island | `TreeSet` over `elementFqns` :126,131 (UTF-16 order) | DON'T PORT now: it goes with CypherIslands in the Cypher wave (about 50 lines once GE2 exists) |

### Notes, GraphEdges

- **F or G?** Both. The rule is checked at F, so a misdeclared edge is a model error even when no island runs. The
  derived view is the Cypher island's (G).
- **The two `Source`s differ:**
  - `sourceOf(model).findClass` = `model.findClass` (graph-first, :163).
  - `of(ctx).findClass` = `ctx.findClassDefinition` (native-first, :137).
  - Their association orders differ too: ingest order vs FQN-sorted.
  - So the order of ends and third links in an error text can differ between F time and Cypher time. F's comes first.
- **Stale javadoc:** :48 says "exactly the eight stereotypes", where `STEREOTYPES` has 15 and :384 says "fifteen".
- The default relationship type is `Protocol.splitFqn(cd.qualifiedName())[1]` (:364). `split_fqn` exists in
  rust/lite-front/src/protocol.rs:21.

## FunctionCompiler.java (164 lines), TypeClassifier.java (326), ClassCompiler.java (108)

All three are package-private (`final class`). Each is reached only through `PureModelContext`, `ModelIntegrity` and each
other (callers.txt; tests: ErrorShapeGuardrailTest pins `FunctionCompiler.java` at 1 total catch, :68; ArchitectureTest's
`PROTOCOL_DESUGAR_DEBT` pins `TypeClassifier` at 1 protocol node, ArchitectureTest.java:2492, which is :111's `new
TypeExpression.NameRef`). They hold a `ModelBuilder`, never a context (invariant 6).

| Class | Lines | Reached by | Reaches an answer | Idioms | Candidate |
|---|---|---|---|---|---|
| FunctionCompiler: `functionsAt`, `exists`, `compileAll`, `compile`, `requireFunction`, `requireShape`, `returnsBooleanOne`, `returnsClassMany` | 164 | `PureModelContext.findFunction` :410 (every overload set G resolves), ModelIntegrity MI2/MI6 (`requireFunction`/`requireShape`), ClassCompiler (ctor) | Yes. **The candidate ORDER** (catalog natives first, `Pure.nativeFunctionsAt`; then model versions whose `FunctionId` is not a catalog id, in ingest order; :48-57) is overload resolution's input. Typed signatures. "... binds to unknown function ..." (:129-130), "... must be realized by a function ..." (:141-142) | **`catch (RuntimeException)` :77** (TRIAGE A.4; note FC). `HashSet catalogIds` :49, membership. **Probe:** `DecisionProbe.INSTALLED` :58-60. A bare name answers empty (:35-40, L7 "resolve once") | PORT `functionsAt`/`compile`/`require*`/predicates. DON'T PORT :58-60 (PHASE3_CRATES §5 item 9). **ASK** on `compileAll`'s drop (note FC) |
| TypeClassifier: `findType`, `isClassFqn`, `isEnumFqn`, `classDef`, `enumDef`, `classify`, `requireArguments`, `requireTypeVariables`, `requireArity`, `arityMismatch`, `headFqn`, `multiplicity`, `op` | 326 | `PureModelContext.findType` :195 (every G type lookup), `findProperty` :438-439, ClassCompiler, FunctionCompiler, ModelIntegrity | Yes. Every type in F. **The `findType` order** :32-57: platform primitive, then primitive extension (`ModelBuilder.findPrimitiveExtension`), then the m3 `Enum` metaclass as an ENUM type (before the class test, which the catalog would also answer), then class, then enum. Error texts: "Unknown type ..." (:151-153), the engine's "Wrong type arguments count (N) for type: C<T|m>" (:288-295), "Wrong type variables count ..." and "Error for type: P(...), 'v' is not an instance of T" (:240-273), which print literals by Java's spelling: `String.valueOf(double)` (`Double.toString`, TRIAGE B1) :265 and `BigDecimal.toPlainString` :266 | `IllegalStateException`s :233-234, :269, :278-279 are internal invariants. Edge to `compiler.spec` at :47 (below). Deferred deps: `PlatformTypes.eraseTdsRow` :104 and `Multiplicity.from` :315 (PHASE3_CRATES §4 (b): "left for the slice that ports `builtin` and `model`") | PORT. RE-EXPRESS :47 as `lite_types` `ENUM`, and :111 as classifying the name directly (same answer, no protocol node) |
| ClassCompiler: `compile`, `realizedFqn` | 108 | `PureModelContext.findClass` :291 (lazy, per class) | Yes: `TypedClass` (supers, `superTypes` with arguments, stored and derived properties, constraints, the `isKey` flag through `EqualityKeys.isKeyStereotype` :55), the realizer FQNs through `SynthFqn.prop`/`constraint`/`constraintMsg` (:71, :83, :91) | :86 `"Warn".equalsIgnoreCase(level) ? WARN : ERROR`: any other spelling, null included, is ERROR. The parser takes any name or string as the level (rust/lite-front/src/element_parser.rs:766-769, 2047-2061), **so this is a default (invariant 4) unless the owner rules that ERROR is Pure's default**. `realizedFqn` :105-107 is a copy of ModelIntegrity:418-420 | PORT. **ASK** on :86 (keep the default, or refuse an unknown level by name). Share one `realized_fqn` with MI |

### Notes, the three compilers

**FC, `compileAll` (:70-95).**
- One overload failing to compile is DROPPED, and the rest are returned. If all fail, the first exception OBJECT is
  rethrown, so its class and message are the answer.
- The comment "A STRICT build never gets here — model integrity fails first" (:85) **is false by reading:**
  - `checkFunction` (MI:357-373) classifies parameter and return types but never validates bounds.
  - So a strict model with `function f(x: String[2..1]): ...` compiles.
  - `findFunction("f")` then throws a raw `IllegalArgumentException("upper (1) must be >= lower (2)")` from `Bounded`'s
    ctor through `TypeClassifier.multiplicity` :105/:116.
  - That is caught by `Compiler.compileAllBodies` :872 and walled with `e.getMessage()`, so `compileErrors` answers
    `["upper (1) must be >= lower (2)"]`.
  - With a healthy sibling overload, the broken one silently loses dispatch.
  - (Not run. LeniencyD6Test.java:69-70 covers only the class-property case.)
- In tolerant builds the drop also hides walled overloads ("tracked, task #56", :78-85).
- Options:
  - (a) Replicate it: `compile -> Result<TypedFunction, Refusal>`, drop `Err`s, return the first `Err` if none succeed.
    Only typed refusals are dropped and a Rust panic propagates, which satisfies TRIAGE A.4's "audited, not replicated"
    while keeping the answers.
  - (b) The poison sentinel (a broken overload scores and throws when it wins). It changes which overload wins and the
    error text.
  - (c) Close the integrity gap: `requireValidBounds` on function parameters, returns and association ends. A model's
    error then moves from a body wall to an element error with lite's "invalid multiplicity —" text.
  - (b) and (c) change answers, so they are the owner's to choose.

**The `compiler.spec` edge.** `TypeClassifier.java:47` reads `InferenceKernel.ENUM_METACLASS_FQN` =
`"meta::pure::metamodel::type::Enum"` (compiler/spec/InferenceKernel.java:70, used there at :170). The same string is
already `PlatformTypes.ENUM` (element/type/PlatformTypes.java:62) and `lite_types` `ENUM`
(rust/lite-types/src/platform.rs:34, ported in slice 1a). Point both uses at it and the element -> spec edge is gone.

**Names-tier edges (allowed, lite-names < lite-element):**
- `compiler.ModelBuilder`: PMC:3, MI:9, FC:4, TC:4, and through `knowledge()`, TC:77 and PMC:217 (`KnowledgeLayer`).
- `compiler.SynthFqn`: MI:10,321,326 and CC:9,71,83,91.
- `compiler.NameResolver.platformFqns()`: PMC:316.

Other layers reached: `platform` (PMC:42-44,620-631: `Registrations`, `DeclarationTable`, `ImplementationTable`),
`builtin` (`Pure`, `DecisionProbe`), `protocol`/`protocol.spec` (GE:14-19), `error`, `model` and `compiler.element.type`.
All of them are in core-layers.txt's `compiler` line (:89). There is no other edge outside it.

## Slice 1b's deferred model methods these classes call ("deferred", rust/lite-model/src/lib.rs:20-23)

- **`MappingDefinition` facts lookups** at PMC:488,495,504,511,520,527,535. These are `md.facts().unionMembers()`,
  `.routedTargetClasses()`, `.mixedUnions()`, `.unionKeyThreads()` and `.poisons().get(PoisonKey.ForClass/ForSet/ForAssociation)`.
  The record exists (records.rs:2348), but keyed lookups over its `Arc<[(K,V)]>` do not.
- **`RelationalOperation.children()`** at MI:571 (a default method, model/RelationalOperation.java:53). It is absent
  from TREE's lite-model. The live worktree has an untracked `relational_operation.rs`, which is not in TREE.
- **`DatabaseDefinition` lookups beyond its default schema**, reached through `ModelBuilder.findTableDefinition`/`findView`
  at MI:579,591 and PMC:710-727,855.
- **`FunctionDefinition.isSynthesized()`** (MI:362) is `synthesized_from.is_some()`. The field exists (records.rs:1841).
- **Not deferred:**
  - `ClassBindings.of`: MI only iterates the list.
  - `RuntimeDefinition.mappings()`/`connectionBindings()` (MI:121,124): record components, not the deferred helpers.
  - `FunctionId.of`: ported, lite-model/src/function_id.rs.

## Other surprises

- **Dead code in main:**
  - The public ctors PMC:61,68.
  - The 4-arg `PureModelContext.from` :169-173.
  - `ModelIntegrity.check` 3-arg :38-40.
  - The `ModelContext` default `isSubtype` (overridden by the only implementation).
- **Comments that contradict the code:**
  - PMC:274 says the overlay never sees a derivation. It does.
  - FC:85 says a strict build never drops an overload. It can (function bounds).
  - GE:48 says "eight". There are 15.
- **The PMC class javadoc :35** says "Not thread-safe (plain maps)", but `derivedCache` and `Reach` are
  `ConcurrentHashMap`s for re-entrancy, not threads (PHASE3_DESIGN §2.2).
