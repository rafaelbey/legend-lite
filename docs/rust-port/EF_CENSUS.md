# The E/F census: what of the normalizer and the element compiler to port (2026-11-12)

*Where things are.* The helpers' evidence is in [`ef-census/`](ef-census) beside this file. The caller matrix, its
script and the extracted tree (`efcensus/callers.txt`, `efcensus/scripts/`, `efcensus/tree/`) stayed outside git, in
`~/.local/legend-lite/port-scratch/efcensus/` on the machine that made them; the tree is `git show 7c438f7b3:<path>`.
The owner ruled every ASK below on 2026-11-12: `PHASE3_SHIM_DESIGN.md` §10 rulings 27-31.

**What this is.** A read-only census of the Java that phase 3's next slice ports: the normalizer (step E,
`core/src/main/java/com/legend/normalizer`, 29 files, 11,372 lines) and the element compiler (step F,
`core/src/main/java/com/legend/compiler/element`, 24 files, 5,177 lines, plus `compiler/element/type`, 1,861 lines,
already ported as `rust/lite-types`). It covers every class and every large method family, and gives a recommendation
for each: PORT, RE-EXPRESS, DON'T PORT or ASK. No file in any worktree was changed.

**How it was measured.** Everything was read at commit `7c438f7b3`, the worktree's HEAD when the census began,
extracted to `efcensus/tree/` (core, spec, pct, wasm, warehouse, sdlc-server, query-store, datacube, docs, rust, tools).
Callers were measured by reference, not by name:

- A class-level matrix (`efcensus/callers.txt`; comments stripped, word match), pruned by access. Every normalizer class
  but `ModelNormalizer`, `MappingNormalizer` and `DynaFnArms` is package-private, so its out-of-package "test" hits are
  noise.
- Each cited caller was then verified by its import or qualified name, at the line.

Six read-only helpers each read one share of the code to a fixed brief (`ef-census/HELPER_BRIEF.md`). Their full evidence
is in `ef-census/H1..H6_*.md`, with every file:line citation. This report merges them. The headline claims were
re-checked by hand: the dead code, the 10 derived keys, `buildModule`'s callers, the `AssociationSynthesis:202` closure
shape, `JudgmentFacts:117`, `Calls`' dependencies and `core-layers.txt`. Nothing was run, so every "never fires" claim
rests on reading, or on step 0's frozen counts where it cites them.

**Short names.** MN = `MappingNormalizer`, JCE = `JoinChainEmission`, US = `UnionSynthesis`, AS =
`AssociationSynthesis`, ROT = `RelOpTranslator`, VR = `ViewRelation`, XS = `XStorePureEnds`, MJN = `ModelJoinNesting`,
RR = `RelationReads`, DC = `DeclaredCoercions`, LV = `LiftedViews`, PMC = `PureModelContext`, MI = `ModelIntegrity`,
GE = `GraphEdges`, FC = `FunctionCompiler`, TC = `TypeClassifier`, CC = `ClassCompiler`.

## 0. Headline

1. **E is almost all answer-bearing; port it nearly whole.** Every normalizer class is on the production path:
   - The chain is `Compiler.normalizeLayer:280` → `ModelNormalizer.normalize`, reached by `compileModel`,
     `compileErrors`, `buildModule`, `buildModel` and the boot layer.
   - Its output is the synthesized functions' bodies, which G types and H and I lower to SQL. It also produces the
     binding tables, the `NormalizationFacts` the resolver reads (poison texts appear inside plans: the frozen plan
     oracle holds MN:1176's text) and the model's errors.
   - About 360 lines need not move:
     - Dead methods and records, about 180 lines in MN, US, AS, RR, XS and DC.
     - `DynaFnArms`, which is test-only.
     - `MissProbe`, a census funnel that becomes `Option`.
     - About 70 lines of dangling javadoc.

   The rest is PORT, with a dozen RE-EXPRESS simplifications that keep the answers (§4).

   **Tolerant mode is ported too**, though it reaches no product door. This is the wall sink, the poisons,
   `withElement`, first-line vs full-message walls and `strictErrors`, at about 15 sites. Its only non-test caller is
   `buildModule` from the corpus harness, but the plan oracle's corpus rows were planned through `buildModule` and
   carry its poison texts, so it is answer-bearing for the oracle the planner is held to.

2. **About a third of F is not F's.**
   - Five classes in `compiler/element` have no F caller. Port them with their first caller instead:
     `ClassLayouts` and most of `EqualityKeys` (I and the planner), `RelationalOpRows` (H and the planner), `Temporal`
     (G, H, lineage and the planner), and `StoreCompiler.declaredType` (G).
   - Three more are E's: `RelationalTypeInference`, `ViewSignatures` and `MilestoningStrategy`.
   - Of `ModelContext`'s 26 default methods, 25 bodies never run, because `PureModelContext` overrides them. The 26th,
     `isDeclaredSubtype`, is G½'s.
   - `TypedNominal` and `TypedElement` are used by nothing.

   F's slice proper is about **3,200 lines**: `PureModelContext` as a concrete `Model`, `ModelIntegrity`, GraphEdges'
   F-time rule, the three element compilers and the `Typed*` records (§2).

3. **Two crate-split edges matter before E starts** (§6):
   - E builds every call through `compiler/spec/Calls` (10 files, about 120 uses). `Calls` is G's by package but
     names-tier by what it reaches.
   - The names slice deferred `KnowledgeLayer.columnKind` because it calls F's `RelationalTypeInference`.

   Both resolve by moving code into `lite-names` (ASK 1, ASK 2).

4. **Nothing frozen holds E's or F's own answers.**
   - The plan oracle sits downstream of G½ to I.
   - The model oracle is parse-only.
   - The census searched the in-git oracle texts for 23 of MN's about 46 error texts and found one (MN:1176, via a
     poison suffix). It found none of about 15 pre-pass texts. The database judge's test plans, archived outside git,
     were not searched.
   - Several identity and hash paths are blind: LV `walls` and `expanding` (0 hits), `ViewSignatures.expanding` (0/137),
     `MappingPrePass.selfEdge` (11 self-edges, no real chain), `MappingClosures.first` (0/2),
     `findAssociationAtClass` (0/0).

   The slice needs a live differential of E's `NormalizedModel` and F's answers, as slice 1b and the names slice made
   for theirs (§10).

5. **Hash order reaches an answer at three sites TRIAGE does not list** (§9.2):
   - `MappingPrePass:231/237`: the text of the "Circular M2M" error.
   - `PureModelContext:330` → `testplan/JudgmentFacts:117`: a test plan's `byClass` order.
   - `PureModelContext:346` → `ClassMapping.classOfWitnessPrefix`: the first match, on a mangling collision.

   `IndexMap` reproduces none of these: Java's order there is a `HashSet`'s.

6. **PHASE3_DESIGN §2.1 is stale.** `SystemDatabase` is gone, so ruling 4's "stays in Java" has no object. There are 10
   derived keys, not 8, and two of them (`Projects`, `SeedMemo`) are open-keyed. `MappingClosures`' memo is written and
   read only inside E, so it is not a post-freeze write (§9.3).

7. **Java has two (really four) subtype relations that answer differently:**
   - `KnowledgeLayer.isSubtype`, over declarations: no primitives, no `Nil`, never throws.
   - `PureModelContext.isSubtype`, over compiled classes: primitives included, `Nil` is the bottom, it rethrows a
     failure.
   - `ModelContext.isDeclaredSubtype`, used by G½.
   - `UserCallInliner.declaredAncestors`.

   The names brief's "one subtyping in Rust" cannot hold without changing answers (ASK 16).

## 1. The normalizer (step E), class by class

**Reach, once for the package.** The production entry is `Compiler.normalizeLayer:280` → `ModelNormalizer.normalize`
(public). It is reached by `compileModel` and `compileErrors` (strict: the first error is the answer), by `buildModel`,
by the boot layer (`Compiler.boot`, once per process) and by `buildModule` (tolerant: a wall map). **`buildModule`'s only
caller outside `Compiler` is the corpus harness** (`spec/.../rcorpus/MinimalCorpus.java:296`; 14 core tests also call
it). So E's tolerant-mode behaviour (walls, poisons) reaches the corpus lanes and their frozen plan rows, and no product
door.

Every class but `ModelNormalizer`, `MappingNormalizer` and `DynaFnArms` is package-private. Everything runs inside one
`normalize` call on one thread: E has no static mutable state and no `catch (RuntimeException)`.

**"Answer" here means one of:**
- the synthesized function bodies (G types them, H and I lower them: SQL);
- the binding table (`MappingDefinition`, which the resolver reads);
- `NormalizationFacts` (the resolver reads them; a poison text shows up inside a plan);
- an error or wall text (`compileErrors`, `buildModule`).

| Class | Lines | Reached by | Reaches an answer | Idioms needing a Rust choice | Recommendation |
|---|---|---|---|---|---|
| `MappingNormalizer` | 2,833 | `ModelNormalizer.normalize:128` (the 4-argument `normalize`); its helpers are called back by 10 sibling classes. The public 2- and 3-argument overloads are test-only | Yes: every legacy and clean-sheet mapping's lifted functions, its binding table, its facts, its walls and poisons | Seven catches turn failures into poisons or walls, each embedding `e.getMessage()` verbatim. `withElement` turns `NotImplementedException` into `ModelException(NORMALIZE)`. `inferMainTableQuiet` swallows an exception to answer null. AND/OR operands are ordered by `toString()` (compare-only). 9 `MissProbe` sites. No hash order reaches an answer | **PORT**, 19 families (§1.1). DON'T PORT about 150 lines; RE-EXPRESS 3 sites; several ASKs |
| `JoinChainEmission` | 1,065 | MN `synthTableBackedParts` (passes 1-3); `ViewRelation.viewRelationExpr` | Yes: the `joinSlot`, `legacyNavigate` and `route` steps and their slot names (`exchange_nav_name` etc. reach SQL) | `catch (ModelException)` :876 used as a boolean. `p.unionRoutes.remove` mutates the frame mid-emission. `equalsIgnoreCase` uses Java's fold. `nullTolerant`'s `default -> false` :1041 | **PORT**, 6 families (§1.2). RE-EXPRESS :876 and the three "first Relational set with this id" lookups. ASK `nullTolerant` |
| `UnionSynthesis` | 1,020 | MN `synthesizeClassMapping`, `synthTableBackedParts`; the pre-pass (`ImplicitInheritance`, `MappingPrePass`) | Yes: stack bodies (member order is the SQL UNION order), key threads (`ID_0`, `id__pk_*` reach SQL), route classification, poisons | `mixedUnions.put` runs before a throw (the fact must survive the error). The route ordinal is read only for its sign. The function-naming rule is spelled twice (US:79 and MN:294/787) | **PORT**, 7 families (§1.3). DON'T PORT US-7 (about 52 dead lines). RE-EXPRESS the ordinal and one `getOrDefault`. ASK on an unregistered "Declared divergence" |
| `AssociationSynthesis` | 795 | MN `normalizeMapping:267` (the injection pre-pass), `:418` (per association mapping) | Yes: appended Join PMs and hoisted sets (body order), the association predicate functions, withheld reasons → poisons | `m == md` :202 (step 0 (b) row, 68/66). The answer text holds `getClass().getSimpleName()` (:411, :434). `resolveAssociation` re-resolves names after D (:376-390). Hash sets are membership-only | **PORT**, 4 families (§1.4). DON'T PORT AS-4 (27 dead lines). RE-EXPRESS :202. ASK three items |
| `RelOpTranslator` | 753 | 23 call sites in 7 normalizer classes | Yes: every legacy property expression, filter, groupBy key, view column, join condition and association predicate, as Pure | The arm order, the guard fall-through and the left-to-right evaluation order decide which error surfaces. A raw ISE is reachable from user text (:610, with a Java class name in it; :720). `shaped == args` :751. Its `PipelineView` seam. `DynaFnRegistryTest` reads its source | **PORT line for line**, 4 families (§1.5). RE-EXPRESS the dispatch split, `PipelineView` and :751. ASK the raw ISEs and the test's form |
| `ModelNormalizer` | 495 | `Compiler.normalizeLayer:280` | Yes: the `NormalizedModel` element list in E.1-E.6 order, every lifted function (`$prop$`, `$constraint$`, `$constraintMsg$`, `$default$`, PRE/POST, `$query`), and E.6's second name-resolution pass (`NameResolver.resolveQueryIn`). E.4's ModelException text escapes even a tolerant build | `d == dp` :220 (step 0 (c), 9/23): an internal pipeline-order assertion. The `universe` HashSet is membership-only. `receiverType` duplicates `DerivedProps.receiverType` | **PORT**. RE-EXPRESS :220 as a typed step or `same` (never a derived `==`) and `receiverType` as the one in `lite-names` |
| `ViewRelation` | 457 | `LiftedViews.body:100` (the view body); JCE, MN, AS, XS for the frame rule and the main source | Yes: the lifted view functions' bodies; view-backed sets' PMs re-pointed at the view's columns | 5 `MissProbe` sites ("is this a view?"). `md` is always null in `viewRelationExpr` (LV:101). `equalsIgnoreCase` | **PORT**. Drop the always-null `md` parameter |
| `MappingClosures` | 451 | `MappingPrePass`, `MappingValidation`, `AssociationSynthesis`, `ResolvedMapping` | Yes: include-order facts (visible sets, roots, unions, enum mappings, pair entries), JSON-connection identity sets, "Can't find mapping/association" | Memoized on `KnowledgeLayer.derived` (a plain HashMap), written and read only inside E. `first.get(id) != cm` :297 (step 0 `first`, 0/2). The walkers' rule differences are documented as "the current semantics" | **PORT** the walkers line for line. RE-EXPRESS the memo as E-local and drop `first` |
| `StoreSubstitutionRewrite` | 404 | `MappingClosures.sets:225`; MN `normalize:162` (`resolveAllStores`); `MappingPrePass:108` | Yes: included sets re-pointed at the replacement store; `MappingDefinition.resolvedStores` (seed rows); "Duplicated mapping include", "mapping include cycle" | `collectDatabases` abuses a recording `AbstractMap` as a visitor. PARK-26. `qualifyStoreRefs`' `current` test is always true (stale comment). Its refusals run outside every catch | **PORT**. RE-EXPRESS `collectDatabases` (a mapper) and `qualifyStoreRefs` (drop dead `current`/`storeFqn`). ASK PARK-26. Measure whether `qualifyStoreRefs` ever rewrites |
| `GroupBySynthesis` | 310 | MN `synthTableBackedParts:1744/1751/1764`; `ViewRelation` | Yes: `groupByComputedKeys` bodies; key names `k0__PRODUCT_ID` reach SQL | Two copies of the key-naming loop. Properties are withheld where the engine accepts the mapping (no register row) | **PORT**. RE-EXPRESS the two key-name copies as one. ASK (low) for a register row |
| `XStorePureEnds` | 304 | MN `synthesizeXStoreMapping:955/966`, `synthesizeModelJoinMapping:1062`; `ModelJoinNesting:94` | Yes: property-space XStore predicates, `legacyLocalProperty` markers; PARK-1 wall | `thatRow == srcRow` (a local, structural is fine). `XEnd.localTypes` is never read. PARK-1's rule is token-identical to MN:1015-1025 | **PORT** `xstoreEndOf` and `renameReads`. RE-EXPRESS `synthesize` together with MN:971-1045 as one function (ASK 8) |
| `MappingPrePass` | 275 | MN `normalize:171` | Yes: each mapping's `ResolvedMapping` (surface → M2M cycles → declared keys → extends → implicit inheritance → store refs → implicit ops); walls (first line); per-set invalid reasons → poisons | **HashMap `pureByTarget` :231 iterated at :237: the "Circular M2M" text depends on Java's bucket order.** `next != pcm` :259 (step 0 `selfEdge`, 11/0) | **PORT**. RE-EXPRESS :259 as `sourceClass == className`. ASK the M2M text's order |
| `ResolvedMapping` | 244 | Built by `MappingPrePass`; read by 16 normalizer classes | Yes: everything synthesis asks of a mapping | HashMaps used for lookup only. `closure()`'s `seen` is redundant | **PORT** as a struct. RE-EXPRESS `closure()` as `[md] ++ closure.mappings()` |
| `DeclaredCoercions` | 228 | MN `translatePmToField:1995/2004/2036` | Yes: `castAsDeclared` / `typeAsDeclared` / `parse*` around each mapped value → SQL and result typing | Its `ledger` parameters are unused. The javadoc names a deleted class (`RequiredNullableCensus`). `declaredAssertion` does not see through views, `coerceColumnToDeclared` does | **PORT**. Drop the unused parameters. ASK the view asymmetry and the Boolean divergence's missing register row |
| `ImplicitInheritance` | 227 | `MappingPrePass.prePass:105,112` | Yes: inherited PMs merged into child sets; implicit `Inheritance` ops; "Can't find association" | HashMaps iterated at :43 and :123, but the order reaches nothing (a size-1 test; a membership set) | **PORT** |
| `ModelJoinNesting` | 197 | MN `synthesizeModelJoinMapping:1082/1090` | Yes: ModelJoin association functions' composite pipelines and nested slot reads | `Set<String[]>` :57 dedups nothing (arrays hash by identity). Reachable raw NPE :118 (a nested hop to a Pure-mapped end). `Map.of` throws if the two lambda parameters share a name. `findAssociationAtClass` is a step 0 (c) row at 0/0 | **PORT**. RE-EXPRESS `hops` as a `Vec` of pairs. ASK the NPE |
| `RelationReads` | 192 | MN `synthesizeXStoreMapping:1006` (`xstore`), MN:1097 and MJN:115 (7-argument `rewrite`) | Yes: XStore/ModelJoin predicate lambdas; `Col.bindSrc` inlining; derived-property β to depth 16 | The 6-argument `rewrite` has no caller. Its `model == null` branches are dead. `bindSrc` and `substVars` ignore lambda shadowing | **PORT**. DON'T PORT the 6-argument overload and the null branches |
| `LiftedViews` | 171 | `ModelNormalizer:126-127`; `body` from MN, JCE, VR (7 sites) | Yes: each view's `<db>$view$<name>` function (its body and declared type); walls with the **full** message and no element (every other E wall is first line, attributed) | **Four identity collections** :50-57 (step 0 (b): `owners` 112/0, `functions` 393/112, `walls` 0/112, `expanding` 0/112). A memoized failure rethrows the same exception object. Re-entrant get-then-put | **PORT** with `IdMap`/`IdSet`: equal views in two schemas are two views. The Rust index must hand out the same `Arc` from `findView` as from `viewsOf`. ASK the wall shape (ASK 10) |
| `M2mRouteGuards` | 158 | MN:1170, :1192; `MappingValidation:111` | Yes: M2M binding keys (the `AllVersions` rule) and refusals | 1 `MissProbe` (falls through to a loud refusal) | **PORT** |
| `Pipeline` | 129 | MN, JCE, `ViewRelation.viewRelationExpr:118` (`forView`), GBS, US | Yes, indirectly: the frame whose ordered maps decide slot names and which row a column binds | Three membership-only HashSets. An anonymous `PipelineView` adapter built per call. `ledgerOrNull` plus a throwing getter | **RE-EXPRESS** as a plain struct with insertion-ordered maps, `Option<&MappingLedger>` and a trait implementation (no answer can move: the HashSets are add/contains only, and every iterated map keeps insertion order) |
| `MappingValidation` | 114 | `MappingPrePass.run:65` | Yes: duplicate set ids, union members, PM names, M2M routes (texts) | A `catch (ModelException)` keeps the exception per set id (the full message becomes a poison). `default -> {}` | **PORT** (spell the two no-op arms) |
| `MappingFacts` | 114 | `MappingLedger.facts:88` | Yes: `unionMembers`, `routedTargetClasses` → `PureModelContext:488/495` → the resolver | Matches by the **raw** `setId()`, not the effective id. `unionMembers` keeps the last match. The javadoc: "Each rule is the F-side reader's, verbatim" | **PORT line for line** (raw id and last-wins included); see ASK 13 |
| `MappingLedger` | 90 | Per mapping, MN:183 | Yes: poisons (a second reason appended with `"; "`), `strictErrors` (the first is thrown in strict mode), `mixedUnions`, `unionKeyThreads`, `operationMembers` | ML:74 copies a HashSet into a LinkedHashSet (hash order, read by membership only) | **PORT**. RE-EXPRESS `mappedInClosure` as a plain set |
| `SetKeyFacts` | 70 | `MappingPrePass:93`; MN:327/392; `AggregateViewLift:44` | Yes: `DeclaredKeys` on Relational bindings → `MetamodelSeeds` → system-store rows | none | **PORT** |
| `AggregateViewLift` | 67 | MN `normalizeMapping:385/395` | Yes: aggregate view functions and bindings; `AggregateViewFacts` → resolver routing | Runs outside the per-class try (a view set's error walls the whole mapping). `DeclaredKeys` look always NONE for view sets | **PORT**. ASK (low) whether NONE is meant |
| `MissProbe` | 56 | 13 normalizer classes (funnel); `neverFired` at 5+ sites | Only as the branch each `null` takes; `neverFired`'s ISE text "F7.8: empty answer at <site> ..." escapes raw (an internal error) | A census funnel: `orElse(null)` and `null` behind names so that `FallbackLedgerTest` can count them | **RE-EXPRESS, don't port the class**: `Option` at each of about 30 funnel sites (each site's `None` branch is the spec); `neverFired` becomes an internal error with the same text. The funnel count retires with the Java (a `RETIRED_TESTS.md` row) |
| `DynaFnArms` | 56 | **No production caller.** Only `spec/.../generators/DynaFnRegistryTest.java`, which also reads `RelOpTranslator.java`'s source text | No | A public constant set for a source-text test | **DON'T PORT** as product code. The test's invariants need a Rust form (ASK 14) |
| `MilestoningFacts` | 30 | JCE:898 (sole caller) | Yes: a temporal target keeps the view expansion | A fold over `knowledge().lineage`. It duplicates the question `Temporal.strategyOf` answers over compiled classes | **PORT** with JCE |
| `package-info` | 67 | -- | -- | Its "idempotent" note is withdrawn; the description of E.1-E.4 is current | -- |

### 1.1 `MappingNormalizer`'s families (H1 has the evidence)

| # | Family | Lines | Reached by | Answer | Idioms | Recommendation |
|---|---|---|---|---|---|---|
| 1 | Entry and per-mapping wall: `normalize` ×3, `withElement` | 122-256 | 4-argument: `ModelNormalizer:128`. 2- and 3-argument: tests only | The element list (mappings, then every lifted function); walls (first line); strict: the ledger's first error with its full message | Catches :198 and :214 embed the message (first line via Java's `split`). `withElement` converts NIE → ModelException. The `MappingResolutionException` arm :247 is dead | PORT the 4-argument path and `withElement`. DON'T PORT the public overloads or the MRE arm |
| 2 | Legacy mapping → binding table: `normalizeMapping`, stamps | 258-456 | `normalize:187` | Lifted functions, `ClassBinding`s (their order feeds `position`), association bindings, `resolvedStores`, facts, poisons | Fault-isolation catches per set (:345), per class (:364) and per association (:419; NIE into `strictErrors` too, against its comment). `mixedUnionRooted` :280 is written and never read | PORT line for line (catch placement, error order). Drop `mixedUnionRooted` and the bare block :300-353 |
| 3 | Clean-sheet door: `cleanSheetToCanonical` and the inline lifts | 458-687 | `normalize:213` | Lifted inline functions, the binding table | The 5-argument `MappingDefinition` ctor: enum mappings are not flattened with includes (asymmetric with the legacy path) | PORT |
| 4 | XStore direction check: `canonicalizeEqualOperands`, `rootedAt` | 711-761 | MN:1016/1018; `XStorePureEnds:222/225` | Only the agree/disagree verdict (PARK-1's text on disagreement) | Operands ordered by record `toString()` (compare-only) | RE-EXPRESS with a derived structural order (same verdicts) |
| 5 | Class-synthesis dispatch | 769-798 | MN:319/363; AVL:36 | The lifted function and its name (`SynthFqn`) | an exhaustive switch | PORT |
| 6 | Relation (`~func`) sets | 800-928 | dispatch; JCE:566; XS:95 | `<fn>() -> map(row \| ^C(...))` | `Col.isEmbedded` :864 and `Col.bindSrc` :888 (deferred model methods). Raw NPE paths :889-890, :907 | PORT |
| 7 | XStore/ModelJoin predicates (column space) | 930-1121 | AS:400/405 ← MN:418 | `legacyAssocPredicate(...)` bodies; PARK-1 wall | Raw `NoSuchElementException` :943, :1058 (only after D left a name unresolved). `Map.of` :1088 | PORT; merge with XS (ASK 8) |
| 8 | M2M (Pure) sets: `synthM2M`, `m2mPropertyValue` | 1124-1230 | dispatch | `getAll(Src) -> filter? -> map(...)`; MN:1176's text **is in the plan oracle** | The raw expression passes through on three misses (:1205-1213). An uncited `AllVersions` suffix strip :1207-1210 | PORT; flag in ASK 12 |
| 9 | Dead collectors: `collectColumnsOfTable`, `collectTargetColumns` | 1238-1294 | nothing (self-recursive only) | No | -- | **DON'T PORT** |
| 10 | Relational dispatch and main-table inference | 1296-1519 | dispatch; US:544; 5 sibling classes | JSON / view / table path; the `RelationalSource` stamp (`aggregationAwareMain` :1382) | `inferMainTableQuiet` :1441-1448 is a catch used as a probe | PORT. RE-EXPRESS the probe as `Result<Option<_>>` with `Err → None` at the quiet caller |
| 11 | JSON-source sets | 1521-1569 | `synthRelational:1308` | `sourceUrl(url) -> map(row \| ^C(p=to(get(...))))` | -- | PORT |
| 12 | View-backed sets | 1571-1619 | `synthRelational:1330` | The view frame as the pipeline source | LV's identity memo (`md.views().body(view)`) | PORT |
| 13 | Table-backed pipeline | 1623-1907 | `synthRelational:1332`; self | The main row pipeline: hops, union routes, filter, groupBy, distinct, `^C(...)` | Ordered iteration only. An unknown filter is skipped at :1720 (loud later at :2240). `~primaryKey` is a deliberate no-op | PORT |
| 14 | PM → constructor field; embedded / otherwise / inline | 1958-2207 | :1834; recursive | Each `^C(p = ...)` binding | `inlineStack`'s order printed in a cycle error | PORT |
| 15 | `~filter` | 2209-2296 | :1739 | `filter(row \| cond)` | -- | PORT |
| 16 | Closure main-table lookups | 2298-2353 | JCE, AS, VR | The table an association anchors on | `mainTableOrNull` skips an ambiguous root set and returns a non-root set's table :2322-2333 | PORT; flag in ASK 12 |
| 17 | Enumeration decode | 2355-2471 | `translatePmToField`; `putRelationCols` | A nested `if(equal(...))` → SQL CASE | An inline ISE "NEVER fired" :2395, then a dead null check. An anonymous reference prints `EnumerationMapping 'null'` :2460. An id lookup takes the first match while the anonymous path is loud on two | PORT; flag :2460 in ASK 13 |
| 18 | Alias scope, view refs in joins, join target | 2473-2691 | MN; JCE, AS, US, VR, GBS | Which row a bare column reads (the first alias per table wins); view-column substitution | `seen` membership sets | PORT |
| 19 | Instance construction, `trustOne`, booleanize | 2693-2833 | M2M, relation, JSON, table, embedded | `new(@C, ^C(...))`, `trustOne`, `to(get, @Prim)`, `'true'/'false'` as Boolean literals | Lite-own emissions with no register row | PORT; ASK 12 |
| -- | Dangling javadoc and comments | about 70 lines | -- | No | -- | DON'T PORT |

### 1.2 `JoinChainEmission`'s families (H2)

| Family | Lines | Reached by | Answer | Idioms | Recommendation |
|---|---|---|---|---|---|
| JCE-1 Pass-1 structural hoisting | 45-217, 591-601 | MN `synthTableBackedParts:1702/1762` | Which hops hoist, in PM order → slot names → SQL; collision texts | Embedded / Otherwise / Inline handle an owner miss three different ways; dead null checks after `orElseThrow` | PORT (drop the dead checks) |
| JCE-2 Chain emission (`emitJoinChain`, `perArmHops`, `routeList`, ...) | 219-455, 471-589, 810-903, 1045-1062 | JCE-1; MN passes 2-3; `ViewRelation` | The body's `joinSlot` / `legacyNavigate` / `route(...)` steps | `catch` :874-878 used as a boolean (the message is discarded, the branch reaches SQL); `unionRoutes.remove` :299 | PORT line for line. RE-EXPRESS :876 as `mainTableOrNull(..).is_none()` (that null is the throw's only cause) |
| JCE-3 Slot naming and lookup | 457-469, 603-690 | JCE-2; MN; ROT via `PipelineView.slotFor` | The `<prop>_nav`, `_` loop and `__<n>` names | `equalsIgnoreCase` (Java's per-character fold); first mint wins | PORT |
| JCE-4 Class-typed target predicates | 692-730 | JCE-1/2; MN | Navigate-or-physical choice | A pinned shadow walker (`ShadowWalkerCensusTest`) | PORT |
| JCE-5 JoinNav collectors | 732-809 | MN, VR | The collection order = the pass-2 emission order = the slot names | The third copy of the "first Relational with this id" lookup | PORT; RE-EXPRESS the three lookups as one helper |
| JCE-6 INNER mapping-filter source; `nullTolerant` | 905-1043 | MN:1667 | `project(filter(...))` (engine `getRelationalElementWithInnerJoin`) | `default -> false` :1041 silently classes a new op as null-rejecting; the comment says "Real INNER emission is follow-up" | PORT; ASK 12 |

### 1.3 `UnionSynthesis`'s families (H2)

| Family | Lines | Reached by | Answer | Idioms | Recommendation |
|---|---|---|---|---|---|
| US-1 Pins and member-function names | 44-136 | JCE `routeList`; US | The FQN each arm calls | The second spelling of the naming rule (with MN:294/787: they must agree); `closure().get(0)` fallback shape :71, :120 | PORT both spellings; ASK the unregistered "Declared divergence" :101-102 |
| US-2 Route classification | 157-402 | MN:1689; the pre-pass | `unionRoutes`, dropped routed properties + poison | The ordinal is read only for its sign; `getOrDefault` default never taken :311 | PORT; RE-EXPRESS the ordinal as a 3-variant tag and the call as `get` |
| US-3 Operation synthesis | 138-155, 404-555, 579-697 | MN:782/783 | The `m1() -> concatenate(m2())` body; member order; `operationMembers`; `mixedUnions` | Write-then-throw (:440-441); `bySetId` built twice | PORT |
| US-4 Key threads | 557-577, 755-787, 894-924, 939-1018 | `synthUnion`, `synthInheritance` | `<col>_<n>`, `<col>__pk_<table>` → SQL | `replaceAll("[^A-Za-z0-9_]","_")` works per code point; a view-backed member gets no key thread | PORT; ASK (low) |
| US-5 Per-arm chain helpers | 789-891 | JCE | The `nl__<prop>__inb__<join>` mid aliases | No text evidence in git (the plan ids cover the shape) | PORT |
| US-6 `prePassedClosure` | 926-937 | ML | engine R1 "mapped" | -- | PORT |
| US-7 `MEMBER_WITNESS`, `FILTER_FORM`, `Thread`, `ScanSource` | 701-753 | nothing (checked against every module) | No | -- | **DON'T PORT** (the resolver still needs `ClassMapping.memberWitness()` itself) |

### 1.4 `AssociationSynthesis`'s families (H3)

| Family | Lines | Reached by | Answer | Idioms | Recommendation |
|---|---|---|---|---|---|
| AS-1 Multi-hop / routed injection pre-pass | 44-322 | MN:267 | Appended Join PMs, hoisted owner sets, root flag | `m == md` :202; `withClassMappings` :219; `withInjectedPMs` drops an embedded block's `~primaryKey` :308-309 | PORT. RE-EXPRESS :202 as "skip index 0" (`closure = [md] ++ Closure.mappings()`, and `mappings()` seeds `seen` with md's FQN, so only index 0 can match: this contradicts step 0's (b) reason but not its counts, 68 calls / 66 includes). ASK on `~primaryKey` |
| AS-2 End helpers, `resolveAssociation` | 340-391 | MN:430/439; II; MC | The association binding's FQN, the poison key | `resolveAssociation` :380-390: import wildcards plus a "same-package fallback" after D (`FallbackLedgerTest` calls this "Phase-D debt ... FIXLIST P7-3") | PORT the end helpers; ASK on :380-390 |
| AS-3 Predicate synthesis | 325-338, 393-547, 578-784 | MN:418 | `(a,b) -> Boolean` predicate functions; withheld reasons; walls | `getClass().getSimpleName()` in texts :411, :434; an unknown XStore/ModelJoin association gets "kind X not supported" instead of "references unknown association" | PORT (every text); ASK 13 on the wrong text |
| AS-4 The 8-argument `buildAssocPredicateBody`, `anchorNameOf` | 549-566, 786-794 | nothing (each calls only the other) | No | -- | **DON'T PORT** |

### 1.5 `RelOpTranslator`'s families (H3)

| Family | Lines | Reached by | Answer | Idioms | Recommendation |
|---|---|---|---|---|---|
| ROT-1 Dispatch (`translate`, `translateTail`, `translateArgs`, `toOne*`) | 46-55, 117-394, 396-613 | 23 sites | Every legacy expression as Pure; wall texts | Guards fall through to the generic arm; ISE :720 and :610 reachable from user text (:610's holds `op.getClass()`); a double `trustOne(trustOne(col))` (port it exactly) | **PORT line for line**. RE-EXPRESS the split (it exists only for a method-length guard) as one match in the same arm order. ASK 11 |
| ROT-2 `PipelineView`, `joinNavigation`, `rebaseToTable` | 56-91, 615-677 | `Pipeline.view()`, `NONE` | `$row.<alias>.<col>` reads | `NONE.slotFor` is unreachable | RE-EXPRESS `PipelineView` as `Option<&Pipeline>`; PORT the rest |
| ROT-3 Helpers (`collectTablesIn`, `columnRead`, `literalToValueSpec`, `comparisonFn`) | 93-115, 679-703 | JCE, MN, AS | Column reads, comparisons | `literalToValueSpec`'s text embeds `value.getClass().getName()` (needs Java's class names per literal kind) | PORT |
| ROT-4 Residual-name resolution | 705-752 | GBS, VR, ROT | Each non-armed dynafunction's spelling and candidates (`Calls.among`) | `shaped == args` :751 | PORT. RE-EXPRESS :751 as the boolean it tests |

## 2. The element compiler (step F), class by class

**Reach, once for the package.** F's entry is `PureModelContext.from`, called by:
- `Compiler.buildModel:253` (strict: `compileModel`, `compileErrors`, every product door through a compiled model);
- `buildModule:536` (tolerant: the corpus harness only, as for E);
- `Compiler.boot:369` (`checkLayer`).

After `from` returns, every later layer reads the model through `ModelContext`: 91 main files in G, H, I, the planner,
testplan, lineage, setup and the server. `PureModelContext` is the **only** implementation (no other `implements
ModelContext` in any module; `NoEagerUserClassLoadsTest`'s proxy forwards to a real one).

**"Is it F's?"** In the "Reached by" column, "not F's" means no caller in E or F; such a class is ported by the slice of
its first caller.

| Class | Lines | Reached by | Reaches an answer | Idioms needing a Rust choice | Recommendation |
|---|---|---|---|---|---|
| `PureModelContext` | 858 | `Compiler` (`from`, `checkLayer`, `refuseDuplicates`); then every reader of the typed model | Yes: the typed elements G types against, the index G, H and I read, the integrity pass's error or walls, the stamped mapping facts, the platform tables | `CheckedLayer` identity set :104 (step 0 (b), 6,478,837/183,150): semantic, provenance. Element memos `classCache`, `enumCache`, `functionCache` (HashMaps, never iterated; no failure cached). `derivedCache` (`ConcurrentHashMap`, get-then-put, re-entrant; 10 keys). `Reach` memo with `catch (LegendCompileException)` :257. **`resolutionUniverse`'s HashSet iterated by testplan** (§9.2) | **PORT** as the concrete `Model` (§2.1). DON'T PORT the two public ctors :61/:68 and the 4-argument `from` (dead). RE-EXPRESS `derived` as named fields, `packages`/`declarations`/`implementations` built at freeze, `resolutionUniverse` as a predicate plus an ordered list. `refuseDuplicates` belongs in `lite-names`. ASK 5 (the overlay), ASK 6 (hash order) |
| `ModelIntegrity` | 639 | `PureModelContext` only (ctor :92, `refuseDuplicates` :153) | Yes: **the first failure, in a fixed order of 13 checks, is `compileErrors`' whole answer**; the tolerant door gets the walls in the same order | `withElement` :234 catches `ModelException` and adds the element (wall = first line; strict = rethrow with the element). `catch (IllegalArgumentException)` :434 embeds `Bounded`'s text. `catch ... continue` :486. :553 writes `getClass().getSimpleName()` ("Other", "Distinct") into a message. Several messages are in lite's own words with no register row | **PORT** the order and the texts, each check as `Result<(), Refusal>` and `withElement` as a fold. RE-EXPRESS :434 (a direct bounds test, same text), :486 (a match), :553 (literal names). DON'T PORT the 3-argument `check` (dead). ASK 12 (the unregistered texts) |
| `GraphEdges` | 503 | GE1/GE2: MI:88-91 on **every** compile, for each fresh non-native class. GE3: `compiler/spec/CypherIslands` (a derived fact; reached from a product door through `SqlIslands`, language CYPHER) | GE2: a `ModelException` naming the declaring class, but only for models that use the `graph::graph` stereotypes; answer-free otherwise. GE3: the Cypher island's edges | Its own lineage walk (a third copy). `Index.byOwner` HashMap is lookup-only. Messages embed `List.toString`. Two `Source`s with different lookup and order (F-time vs Cypher-time) | **PORT** GE1 and GE2 with F (ASK 4 offers deferring them). **DON'T PORT** GE3 now: it goes with the Cypher wave |
| `ModelContext` | 452 | Interface; 26 default methods, 25 overridden by `PureModelContext` | Through `PureModelContext` | Its `derived(Class<T>, Function)` is an open `Class<?>`-keyed memo. The `isSubtype` default (:387-451) is dead. `isDeclaredSubtype` (:391-418) is the **one default that runs**, from `UserCallInliner:926` (G½) | **RE-EXPRESS** as `Model`'s inherent methods (no trait: one implementation). DON'T PORT the 25 overridden default bodies. PORT `isDeclaredSubtype` with G½ (a distinct relation: ASK 16) |
| `RelationalOpRows` | 344 | **Not F's.** H: `resolver/ConstructedInstances:93`, `ElementReferences:220`. Planner: `OpSeeds:54`, `MetamodelSeeds:469-635` | Yes: the metamodel store's `relational_elements` and `data_types` rows (ids `"tbl:" + db + ...`) → seed steps → metamodel query answers | `String.valueOf(Long/Double)` needs Java's double spelling (lite-javacompat). `findDatabase(..).orElse(scopeDb)` | **PORT with its first caller** (H or the planner; ASK 3), not in F's slice |
| `TypeClassifier` | 326 | PMC `findType`, `findProperty`; CC, FC, MI | Yes: every type in F. The `findType` order: primitive, primitive extension, the `Enum` metaclass, class, enum. Texts: "Unknown type ...", the engine's "Wrong type arguments count ...", the literal spellings (`Double.toString`, `toPlainString`) | **Edge to `compiler.spec`** :47 (`InferenceKernel.ENUM_METACLASS_FQN`). It builds a protocol node at :111 (pinned by `ArchitectureTest`'s `PROTOCOL_DESUGAR_DEBT`). Needs `Multiplicity.from` :315 and `PlatformTypes.eraseTdsRow` :104 (left out of `lite-types`) | **PORT**. RE-EXPRESS :47 as `PlatformTypes.ENUM` (the same string, already `lite_types::ENUM`: the element → spec edge goes) and :111 as classifying the name directly |
| `RelationalTypeInference` | 289 | **E's, in production**: `KnowledgeLayer.columnKind:406` ← DC:147, US:968/980; and `ViewSignatures:136` ← LV:99. Also `RelationalOpRows:306` (H, planner) | Yes: a column's kind → a coercion and a key thread's kind; a view function's declared columns | `str.length()` counts UTF-16 units (→ `VARCHAR(n)`). `orElse(db)` falls back to the scope (documented, the engine's rule) | **PORT line for line, into `lite-names`** (it reads only `StoreLookups` and the model). That finishes the names slice's deferred `columnKind` and deletes the names → element edge (ASK 2) |
| `ClassLayouts` | 277 | **Not F's.** I and the planner: `TypedQuery:173`, `Compiler:835`, `BodyPlanner:189`, `EnginePlans:170`, `SeedableLets:38` | Yes: a class value's SQL struct fields (order, `__id`, `__type`, `__canon`) | `LayoutMemo`: three `ConcurrentHashMap<Type, ...>`, open-keyed, re-entrant (D5's `layouts`). Reads `sql.SqlExpr.SYNTHETIC_ID`/`_TYPE` (not yet in `lite-sql`). Its `substitute` duplicates EqualityKeys' | **PORT with I** (`lite-lowering`); the memo storage per the D5 ruling. Add the two constants to `lite-sql` then |
| `EqualityKeys` | 245 | `isKeyStereotype`: **F** (CC:55). `fqnOf`, `resolve`: I, the planner, testplan. `declaresKeys`: testplan only | Yes: the `equalityKey` flag (F); in-SQL key arms and canons (I); test-plan key trees | The cycle guard is a **string** of `Type`'s derived text :140. `collect` never returns false (:147-149, :240 are unreachable) | PORT `isKeyStereotype` with F. Port the rest with I and testplan (ASK 3). RE-EXPRESS the guard as a structural key and `collect` as returning nothing |
| `FunctionCompiler` | 164 | PMC `findFunction:410` (every overload set); MI; CC | Yes: **the candidate order** (catalog natives first, then model versions in ingest order) is overload resolution's input; typed signatures; texts | **`catch (RuntimeException)` :77** (TRIAGE A.4): a broken overload is dropped silently; the first failure is rethrown if all fail. Its comment ":85 a STRICT build never gets here" is false by reading (function bounds are never validated). `DecisionProbe.INSTALLED` :58-60 | **PORT**. DON'T PORT :58-60 (the probe, as slice 1b ruled). ASK 7 on :77 |
| `ViewSignatures` | 148 | **E's**: LV:99 only | Yes: the lifted view function's declared return type; on a cycle, a wall "reads its own columns (cyclic view-on-view chain)" | **Identity set** :51 (step 0 (b) `expanding` 0/137: oracle-blind). Fallbacks: an unknown column → `Any[0..1]` :102, no inferred type → `Any` :137 (documented as the engine's) | **PORT** with E, into `lite-names` beside `RelationalTypeInference` (or `lite-normalizer`; ASK 2), with `IdSet` |
| `Temporal` | 141 | **Not F's.** `strategyOf`, `generatedMember`: G (Typer, GraphFetchChecker) and H. `isGeneratedDate*`: H, lineage. `anyTemporalGetAll`: the planner (`Compiler:825`, `EnginePlans:160`, `ProductStatements:140`) | Yes: milestoning types, temporal SQL, the engine exists-join form | `anyTemporalGetAll` walks `TypedSpec`: **the element → spec edge** | **PORT with G, H and the planner** (ASK 3); `anyTemporalGetAll` must not land in `lite-element` |
| `TypedFunction` | 129 | F builds it (FC:110); also G (`SignatureApart:67`) and H (`ObjectReferenceArms:69`); read by 55 files | Yes: every call's callee, signature, body | Record equality over all ten fields **is used** (`TypedNativeCall:42`, `TypedUserCall:73`), and **identity is used too** (`SpecCompiler:48`'s `IdentityHashMap`, step 0 (a), which relies on F's cache handing out one instance per overload). The 8- and 9-argument ctors and a null `id` are test-only | **PORT**: `Arc<TypedFunction>` handed out once per overload by the function cell, derived `PartialEq`/`Hash` over every field (not by `id`: `SignatureApart` makes a renamed copy with the same id). RE-EXPRESS the ctors (`definition`, `id` non-optional) |
| `StoreCompiler` | 120 | `tableSchema`: **F** (PMC:739). `columnType`: F and E (ViewSignatures:141). `declaredType`: **G** (`TableReferenceChecker:135`), setup's DDL, testplan | Yes: a table's relation type; DDL text; a scan's stored types in the plan | `declaredType` spells SQL ("VARCHAR", "DECIMAL", ...) carried as `Sized{kind: String}` into the plan JSON | PORT `tableSchema` with F; `columnType` moves with ViewSignatures; `declaredType` goes with G (its strings as they are; an enum would change the plan seam: not this slice's question) |
| `ClassCompiler` | 108 | PMC `findClass:291` (lazily, per class) | Yes: `TypedClass` (supers, properties, constraints, the `isKey` flag, realizer FQNs) | :86 maps any enforcement level but "Warn" (null included) to ERROR: a default. `realizedFqn` duplicates MI:418 | **PORT** (one shared `realized_fqn`). ASK 12 on :86 |
| `Property` | 103 | F builds it (CC); G, H, the planner, testplan read it. callers.txt's model/protocol hits are word-match noise | Yes | derived equality; no map key | **PORT** |
| `MilestoningStrategy` | 51 | E (`MilestoningFacts:23`), G, H, `Temporal` | Yes | `default -> null` :34 for an unknown `temporal.*` stereotype (a leniency, "matching the historical set-membership tolerance") | **PORT** with E (it is reached first by E); ASK 12 lists the default |
| `TypedConstraint` | 49 | F (CC:84); H reads it (`GraphEmission:599-613`) | Yes | `externalId` is always empty and never read; `of` has no main caller | **PORT**; RE-EXPRESS without `externalId`/`of` |
| `TypedClass` / `TypedEnum` / `TypedParameter` | 47 / 28 / 23 | F; G; H | Yes | `null → List.of()` in the ctor; derived equality | **PORT** |
| `TypedNominal` / `TypedElement` | 27 / 21 | **Nothing**: only their `permits` and `implements` lines | No | sealed markers | **DON'T PORT** |
| `package-info` | 85 | -- | -- | It states the contract: FQN references, lazy structure, F never triggers G | -- |
| `type/` (`Type`, `PlatformTypes`, `Multiplicity`, `ExprType`) | 1,861 | -- | -- | -- | **Ported** (`lite-types`, slice 1a). F needs two of the operations it left out: `Multiplicity.from` (TC:315; G has 13 more sites) and `PlatformTypes.eraseTdsRow` (TC:104). Add both to `lite-types` now: `lite-front`, `lite-builtin` and `lite-model` exist. `handleRowClass` (the planner and H) and `isStatementOnly` (the names slice's query pass) are not F's. Subtyping is not a type operation (ASK 16) |

### 2.1 `PureModelContext`'s families (H5 has the evidence)

| Family | Lines | Reached by | Answer | Idioms | Recommendation |
|---|---|---|---|---|---|
| P1 Construction and the F gate: `from`, `CheckedLayer`, `checkLayer`, `refuseDuplicates` | 38-190 | `Compiler.buildModel:253`, `buildModule:536`, `boot:369`, `normalizeLayer:279` | The index G, H and I read (`index.add(normalized.elements())` :184); integrity's error or walls | `CheckedLayer` identity set (semantic: the boot's own instances are skipped; a graph element at a boot FQN is checked). `without(adopted)` drops the ids a graph adopted. The public ctors :61/:68 and the 4-argument `from` are dead | PORT the gate's order. `CheckedLayer` becomes the boot's `IdSet` over its `Arc`s, so **the graph index must hold the boot's very `Arc`s, never clones** (the alternative: a boot-provenance flag per index slot). DON'T PORT the dead entries |
| P2 Subtyping: `Reach`, `Walked`, `isSubtype`, `subtree` | 198-266 | 50+ sites in G, H, testplan, the planner | Type conformance, routing, verdict facts; a rethrown failure is the query's error | `Reach` memo (open keys); `catch (LegendCompileException)` :257 caches the failure inside `Walked` and rethrows it only when the answer would be false | PORT as its own relation (not merged with `KnowledgeLayer.isSubtype`: ASK 16). `Walked` becomes `(set, Option<Refusal>)`. The Rust class lookup must return the cell's cached `Refusal`, not an `Option` |
| P3 `derived` | 268-282 | 10 keys (§9.3) | Through each fact | Get-then-put; re-entrant. **Its comment :274 says an overlay never sees a derivation; it does** (:277 passes the overlay when it asks first) | RE-EXPRESS as named fields (ruling 4). Only `Reach` (and the field set) is F's slice; the other 9 facts are G's, I's and the planner's |
| P4 Element memos: `findClass`, `findEnum`, `findFunction(ById)`, `findProperty`, ... | 284-301, 371-440, 684-705, 729-733 | every reader of the typed model | The typed elements | Nothing caches a failure. `functionCache` caches an empty list for any asked FQN (open keys). `findProperty` recurses into supers with no cycle guard (a tolerant build keeps a walled cyclic class: StackOverflowError) | PORT with `OnceLock<Result<_, Refusal>>` cells per FQN known at freeze; an unknown FQN answers empty with no cell. ASK 11 on `findProperty`'s cycle |
| P5 Name universe: `resolutionUniverse`, `elementFqns`, `isPackage`, `functionFqns` | 309-369, 448-467, 828-833 | `Compiler:678` (contains only), **testplan/JudgmentFacts:117 (iterates)**, G, H, lineage | Name resolution; **a test plan's `byClass` order**; on a mangling collision, SQL | HashSets :330 and :346 are iterated by outside readers; `packages` is filled lazily | RE-EXPRESS: a `contains` predicate plus an explicit ordered list; `packages` built at freeze. ASK 6 |
| P6 Mapping fact readers (`findLegacyMapping`, `unionMemberClasses`, `routedTargetClass`, `mixedUnionMembers`, `unionKeyThreads`, `mapping*Poison`) | 303-307, 474-537 | H (`AssociationJoins`, `ClassSources`, `StackBuilder`, ...), G (`ImportDataFlow`), the planner, lineage | E's facts become H's routing and poison errors | Keyed lookups over `NormalizationFacts` (in Rust an ordered `Arc<[(K,V)]>`, with no keyed lookup yet) | PORT; needs the deferred `NormalizationFacts` lookups (§8) |
| P7 Execution overlay: `withExecutionOverlay`, `overlayRuntime`/`Connection` | 55-59, 539-547, 566-614, 637-645 | `test/ServiceTestRunner:434` (reached only by tests and tools) and the corpus harness (`MinimalCorpus:302`) | Only on those harness paths | Shares the base's memo maps by reference | ASK 5 |
| P8 Platform tables: `declarations`, `implementations` | 616-635 | `implementations()`: 15 files; `declarations()`: none outside the file | Every native dispatch reads the implementation table | Lazy fields; neither build throws | PORT; build at freeze (Knowledge, ruling 4) |
| P9 Metamodel registry: `tracksClassifier`, `classifierInstances` | 775-826 | H, testplan, the planner, lineage | The metamodel store's seeds | `.sorted()` uses Java's `String` order (UTF-16 units, TRIAGE B5) | PORT with a UTF-16 sort |
| P10 Store lookups and delegations | 192-196, 443-472, 549-564, 647-682, 707-773, 835-856 | everywhere | Thin | `milestoningWithIncludes`' `seen` (membership) | PORT as `Model` methods |

### 2.2 `ModelIntegrity`'s families (H5)

The fixed check order is part of the answer: duplicates, classes (ingest order), inheritance cycles, functions,
duplicate signatures, associations, GraphEdges, enums, databases, mappings, runtimes, services, data spaces.

| Family | Lines | Answer | Idioms | Recommendation |
|---|---|---|---|---|
| MI1 Orchestration and walls: `check`, `refuseDuplicates`, `withElement` | 38-118, 212-247 | The strict error, or the wall map (keyed by `e.element()` else the checked FQN) | Exceptions as control flow (:234); the `CheckedLayer` freshness predicate | PORT; `refuseDuplicates` belongs in `lite-names` beside `duplicate_elements` |
| MI2 Classes: `checkClass`, `requireValidBounds` | 249-332, 418-439 | S91's "Invalid supertype", legend-pure's default-value text, "Found duplicated property", "invalid multiplicity — upper (U) must be >= lower (L)" | `catch (IllegalArgumentException)` :434 | PORT; RE-EXPRESS :434 as a direct test with the same text |
| MI3 Inheritance acyclic | 454-494 | "Inheritance cycle: a -> b -> a" (lite's words) | `catch ... continue` :486 | PORT; RE-EXPRESS :486 |
| MI4 Functions, duplicate signatures | 334-373 | "... defined more than once with the same signature ..." | `priorKeys` from `CheckedLayer`. **Gap: no bounds check on parameters or returns** (ASK 7) | PORT |
| MI5 Annotations, enums, runtimes, services, data spaces, named references | 73-84, 92-98, 108-210, 441-452 | The engine's "Can't find ..." texts | `connectionBindings` iterated in record order | PORT |
| MI6 Mapping bindings | 375-416 | "binds unknown class", "binds to unknown function", "must be realized by ..." | Lists only (it does not need the deferred `ClassBindings` index) | PORT |
| MI7 Database refs and milestoning (S26) | 496-637 | "The column ... can't be found ...", "The table ...", the milestoning refusal | `getClass().getSimpleName()` :553; needs `RelationalOperation.children()` :571 | PORT; RE-EXPRESS :553 as literal names |

### 2.3 `GraphEdges`' families (H5)

| Family | Lines | Answer | Recommendation |
|---|---|---|---|
| GE1 Records, `Source`, `Index`, `sourceOf` | 52-109, 147-171 | Only as GE2's input | PORT what GE2 needs |
| GE2 The F-time rule (`readOne`, `checkProfile`, `refuse`, ...) | 186-502 | A `ModelException` for a misdeclared edge class (only with `graph::graph` stereotypes); lite-only ("only lite checks the rule, since only lite runs the island", :49). Its javadoc :48 says "eight" stereotypes; there are 15 | PORT with F, or defer (ASK 4) |
| GE3 The Cypher view (`of`, `read`, `edgeAt`) | 111-145, 173-184 | The Cypher island's edges | DON'T PORT now (the Cypher wave) |

## 3. DON'T PORT, with the evidence

Each entry is dead (no caller in any module, checked), test-only, a probe, or superseded. The "Replaced by" column says
what takes its place.

| What | Lines | Evidence | Replaced by |
|---|---|---|---|
| MN's public 2- and 3-argument `normalize` | about 16 | Callers: `ModelBuilderTest:262,290` and `MappingNormalizerTest:1605,2695,2703` only. They rebuild `LiftedViews` a second time | nothing (Rust tests call the one entry) |
| MN `withElement`'s `MappingResolutionException` arm (:247) | 3 | No layer E may reach throws one (grep of base, builtin, compiler, model, platform, protocol, normalizer) | nothing |
| MN `collectColumnsOfTable`, `collectTargetColumns` (1238-1294) | 57 | Self-recursive only; referenced nowhere else in core or spec | nothing |
| MN `mixedUnionRooted` (:280, :287) | 2 | Written, never read | nothing |
| MN's dangling javadoc and comments (a truncated "ENGINE ROUTER PARITY", the docs of deleted methods, misplaced docs) | about 70 | No code | nothing |
| US-7: `MEMBER_WITNESS`, `FILTER_FORM`, `record Thread`, `record ScanSource` (701-753) | about 52 | No reference outside their definitions in any module. The `$member` columns in SQL come from the resolver (`GraphEmission:3028`, `Substitution:2275`, `CorrelatedSubselects:2164`) | nothing; `ClassMapping.memberWitness()` is still needed by H |
| AS-4: the 8-argument `buildAssocPredicateBody`, `anchorNameOf` (549-566, 786-794) | 27 | Each called only by the other | nothing |
| RR's 6-argument `rewrite` (49-56) and the `model == null` branches (:114, :116, :142) | about 11 | No caller; both live callers pass a model | nothing |
| `XEnd.localTypes`; the sentinel `"<relational>"` (XS) | about 5 | Written, never read | nothing |
| DC's unused `ledger` parameters; its javadoc's `RequiredNullableCensus` hook | about 6 | Never read; the class no longer exists | nothing |
| `DynaFnArms` | 56 | No production reader; only `DynaFnRegistryTest` (spec) | A Rust form of the test's invariants (ASK 14) |
| `MissProbe` (as a class) | 56 | A funnel so a source-text test can count `orElse(null)` sites | `Option` at each site; `neverFired` becomes an internal error with the same text (§4) |
| PMC's public ctors :61/:68 and its 4-argument `from` :169-173 | about 20 | No `new PureModelContext(` outside the file; no caller of the 4-argument `from` | nothing |
| MI's 3-argument `check` :38-40 | 3 | No caller | nothing |
| FC's `DecisionProbe.INSTALLED` hook :58-60 | 3 | A `ServiceLoader` measurement probe, a no-op unless `LL_SHADOW` is set (slice 1b already declined to port `DecisionProbe`) | nothing |
| GE3, the Cypher view (`of`, `read`, `edgeAt`) | about 47 | Reached only by `CypherIslands` (G) through `derived` | The Cypher wave ports it with the island (TRIAGE: Cypher is later) |
| `ModelContext`'s 25 overridden default bodies, including the `isSubtype` default and its private walk (:387-389, :420-451) | about 150 of 452 | `PureModelContext`, the only implementation, overrides each one | `Model`'s inherent methods (the signatures survive as methods) |
| `TypedNominal`, `TypedElement` | 48 | No switch, parameter or field of either type in any module | nothing (each record is its own struct) |
| `TypedFunction`'s 8- and 9-argument ctors and its null-`id` path :66-99 | about 25 | Test-only | non-optional `definition` and `id` |
| `TypedConstraint.externalId`, `TypedConstraint.of` | about 6 | `externalId` is always empty and never read; `of` has no main caller | nothing |

**Total: about 660 lines** (about 360 in E, about 300 in F). The guard tests that read E's sources as text retire with
the Java and need `rust/RETIRED_TESTS.md` rows, not ports:
- `FallbackLedgerTest` (the funnel per file, the never-fired floor);
- `ParkedWorkLedgerTest`'s PARK-1 and PARK-26 anchors (restate them against the Rust sites);
- `LegacyReachbackCensusTest`'s E and F rows;
- `CodeShapeGuardrailTest`'s allowlists (`Pipeline.expr`, `MappingClosures`' lazy fields, `PureModelContext.declarations`);
- `ShadowWalkerCensusTest` (`hasMappedSubclass`, `collectInheritanceMembers`);
- `ErrorShapeGuardrailTest` (FC's one catch);
- `ArchitectureTest`'s `PROTOCOL_DESUGAR_DEBT` (TC:111).

**Ported by another slice, not this one** (no E or F caller; the ASK 3 placement):

| Code | Lines | Ported by |
|---|---|---|
| `ClassLayouts` | 277 | I |
| `EqualityKeys` beyond `isKeyStereotype` | about 170 | I, the planner, testplan |
| `RelationalOpRows` | 344 | H or the planner |
| `Temporal` | 141 | G, H, lineage, the planner |
| `StoreCompiler.declaredType` | about 36 | G |
| `ModelContext.isDeclaredSubtype` | about 28 | G½ |

That is **about 1,000 lines**.

## 4. RE-EXPRESS: same answers, simpler structure

Each says what replaces what, and why no answer can move. "Oracle-blind" marks a site no frozen oracle exercises, where
only the argument (and the differential of §10) protects it.

| Site | Simpler structure | Why the answers cannot change |
|---|---|---|
| `Pipeline` (whole class) | A plain struct: insertion-ordered maps for `aliasToTargetTable`, `pathToSlot`, `navSlotByProp`, `navSlotOwner`, `unionRoutes`, `aliasToTargetColumns`, `inlineStack`; any set for the three HashSets; `Option<&MappingLedger>`; `PipelineView` as a trait implementation | The HashSets are only added to and tested. The adapter's five methods are pure reads. Every iterated map keeps insertion order |
| ROT `PipelineView` | `Option<&Pipeline>` (None = today's `NONE`) | Exactly two implementations; `hasSlots()` is constantly true on `Pipeline`; `NONE`'s only throwing method is unreachable (`joinNavigation` checks `hasSlots` first) |
| ROT `translate` + `translateTail` | One match in the same arm order | `translateTail` is reached only from `translate`'s default arm; the split exists for `CodeShapeGuardrailTest`'s 250-line method limit |
| ROT:751 `shaped == args` | The boolean it tests (`args.len() >= 2 && is_variadic_run(fqns)`) | The same predicate as :732 |
| MN `canonicalizeEqualOperands` (:736) | Order AND/OR operands by a derived structural order (or the codec's canonical text) instead of Java record `toString()` | Compare-only: the canonical form is never emitted (MN:1011-1015). Any total order consistent with equality gives the same agree/disagree verdict. *Hedge:* Java's `toString` order is consistent with equality only if no two distinct operands print alike. A record's text names its type and every component, so that is answer-preserving in practice, not by proof |
| MN `inferMainTableQuiet` (catch → null) and JCE:876 (catch → keep the view expansion) | `infer_main_table() -> Result<Option<T>, ModelException>`, the quiet caller mapping `Err → None`; JCE tests `main_table_or_null(..).is_none()` | Every exception the probe swallows maps to `None`. JCE:876's catch has exactly one cause (`mainTableDefOf`'s throw when `mainTableOrNull` is null); its message is discarded |
| AS:202 `m == md` | Iterate the included part only (skip index 0) | `closure = [md] ++ MappingClosures.closure(fqn).mappings()`, and `mappings()` seeds `seen` with md's own FQN (MappingClosures:156-157), so only index 0 can be `md`. Step 0's 68 hits / 66 misses = 68 calls, 66 includes. *Its class differs from the census's (b) reason, not from its counts* |
| MappingPrePass:259 `next != pcm` | `pcm.sourceClass != pcm.className` | `pureByTarget` holds one object per class and every `pcm` walked comes from it. 11/0 in step 0 = 11 self-edges and **no real chain in any lane (oracle-blind)** |
| MappingClosures:297 `first.get(id) != cm` | Drop the `first` map | `walkIds` visits each mapping once, surfaces are memoized, and no producer shares a `ClassMapping` between two mappings' surfaces, so the test is always true where it is reached (step 0: 0 same, 2 different). **Never a derived `==`**: it would suppress a genuine duplicate Java reports. Oracle-blind |
| ModelNormalizer:220 `d == dp` | A typed step (E takes the adopted model from `lite-names`), or `same` | An internal pipeline-order assertion; it never fires in production (`Compiler.normalizeLayer` adopts first). **Never a derived `==`** (an equal un-adopted property would pass) |
| `MappingClosures`' memo (on `KnowledgeLayer.derived`) | A value local to the `normalize` call, borrowing the index, with unsync `OnceCell` fields. Keep it lazy | Written and read only inside E: every caller is package-private normalizer code (it is not one of D5's post-freeze writes, §9.3). Laziness is answer-bearing: `sets()` throws "Can't find mapping" for the mapping that asks, and the tolerant catch is per mapping |
| `ResolvedMapping.closure()` | `[md] ++ closure.mappings()` | Its `seen` set re-does `mappings()`' dedup |
| MJN:57 `Set<String[]> hops` | A `Vec<(String, String)>` | Arrays hash by identity, so the set dedups nothing; the real dedup is :66-67 |
| XS `synthesize` and MN `synthesizeXStoreMapping:971-1045` | One function, parameterized by the read rewrite and the relation arguments | They differ only there and in where the empty-lines check sits (with no lines both throw the identical text); orientation, the comparison and all four wall texts are identical (subject to ASK 8) |
| GBS `groupedKeyColumnNames` + `applyGroupBy` | One loop returning the names | They differ only where `applyGroupBy` throws, and `applyGroupBy` runs first on the same set (MN:1744 before :1751) |
| JCE's three "first Relational set with this id" lookups (JCE:132-139, :758-771; US:231-240) | One helper | The same first-match-and-break semantics |
| US route ordinal; US:311 `getOrDefault` | A 3-variant tag {Member, Root, PinnedSingle}; `get` | No reader reads the number, only its sign (US:397, :826); `recordOwner` keys every property, so the default is never taken |
| `MappingLedger.mappedInClosure` | A plain set | Only `contains` reads it (its "ordered" copy keeps hash order anyway) |
| SSR `collectDatabases` | The rewrite takes a store mapper; collecting = running it with a recording mapper | The same traversal, the same keys |
| SSR `qualifyStoreRefs`' `current` and `storeFqn` | Drop them | `findDatabase` is exact-only now, so `current` is always null and `storeFqn` is the identity. The comment "rewrite ONLY the SHADOWED cases" is stale. Whether it ever rewrites in production is unmeasured: D already qualifies store refs, so it may be DON'T PORT once measured |
| DC unused `ledger` parameters; VR's always-null `md` | Drop them | Never read; LV:101 always passes null |
| `MissProbe` funnel | `Option` at each site; `neverFired` as an internal error with its text | Each site's `null` branch is kept as the `None` branch |
| `ModelNormalizer.receiverType` | `DerivedProps.receiverType` (lite-names) | An identical body |
| MI:434 `catch (IllegalArgumentException)` | Test the bounds directly and write "<owner>: invalid multiplicity — upper (U) must be >= lower (L)" | The catch exists only to read `Bounded`'s ctor message; its other message ("lower must be >= 0") is unreachable (the protocol refuses a negative lower first) |
| MI:486 `catch (ModelException) { continue; }` | `match head_fqn(sup) { Err(_) => continue, ... }` | It produces nothing; a structural head is `checkClass`'s report |
| MI:553 `getClass().getSimpleName()` | Literal "Other" / "Distinct" per arm | The text Java writes |
| TC:47 `InferenceKernel.ENUM_METACLASS_FQN` | `PlatformTypes.ENUM` / `lite_types::ENUM` | The same string `"meta::pure::metamodel::type::Enum"`; removes the element → spec edge |
| TC:111 (builds a protocol `NameRef` to classify) | Classify the name directly | The same answer; `ArchitectureTest` pins it as desugar debt |
| PMC `derived` (an open `Class<?>`-keyed map) | Named fields: `OnceLock` per closed fact, a `Memo` per open one | Ruling 4 already; every derived fact is a pure function of the model |
| PMC `packages`, `declarations`, `implementations` | Built at freeze | Knowledge; neither table build throws (they collect `dangling` and `conflicts` lists) |
| PMC `resolutionUniverse` | A `contains` predicate plus an explicit ordered list for the one iterator | Every other reader uses `contains` (the order question is ASK 6) |
| PMC `Walked` with its rethrown exception | `(set, Option<Refusal>)` from the class cell's cached outcome | Java rethrows the memoized exception object, so the same message comes back each time |
| `ModelContext` (an interface with one implementation) | `Model`'s inherent methods | One implementation |
| EqualityKeys:140 string cycle guard | A structural key `(fqn, Vec<(name, Type)>)` | `Type`'s text is its derived record text, so text equality is structural equality (barring a `]` or `,` inside an FQN) |
| `EqualityKeys.collect`'s boolean | Return nothing | It never returns false (:147-149 and :240 are unreachable; the "False = poisoned" javadoc is stale) |
| `TypedConstraint` without `externalId`/`of`; `TypedFunction` with non-optional `definition`/`id` | -- | Nothing reads them; the optional forms are test-only |

## 5. The ASKs: choices only the owner can make

Each is a question, its options, what each costs, and the census's leaning. They are ordered by when the slice meets them.

**ASK 1. Where does `Calls` (the one builder of a call after resolution) live?** E builds every synthesized call
through `compiler/spec/Calls`:
- 10 of its files, about 120 uses (`platform` about 90, `exact` about 45, `infixRun`, `among`, `infixRunAmong`).
- `Calls` is 117 lines and reaches only `NameResolver.builtCallNames`, `CoreFn` and protocol records: names-tier by
  what it reaches, G's by package.
- The names slice's live crate has no `Calls` and no `builtCallNames` as of this census.

| Option | Cost |
|---|---|
| (a) Re-home `Calls` (and `builtCallNames`) in `lite-names` | About 120 lines in the names slice (or in E's slice, first). PHASE3_CRATES §2 already lists `Calls` as the names → spec edge `StatementInline` and `LiteralMapUnroll` take, so this one move fixes both |
| (b) `lite-normalizer` depends on `lite-spec` | E's slice cannot finish before G's crate exists, which inverts PHASE3_DESIGN §3's order (E, then F, then G) |
| (c) A copy in `lite-normalizer` | Two builders, against "THE ONE PLACE A CALL IS BUILT" (its `ArchitectureTest` rule) |

*Leaning: (a).*

**ASK 2. Where do `RelationalTypeInference`, `ViewSignatures` and `StoreCompiler.columnType` live?**
- All three are E's, not F's: `KnowledgeLayer.columnKind:406` and `LiftedViews:99` call them.
- They need only `StoreLookups`, `lite-types`, `lite-model` and `lite-error`.
- The names slice deferred `columnKind` ("Left for F's slice: `columnKind`, whose computed-view-column arm asks F's
  `RelationalTypeInference`", rust/lite-names/src/knowledge.rs:20).

| Option | Cost |
|---|---|
| (a) Move them to `lite-names` | Finishes `columnKind`; deletes the names → element edge; amends PHASE3_CRATES §2's map (the owner ratifies) |
| (b) Keep them in `lite-element` | `columnKind` cannot live in `lite-names`; `KnowledgeLayer` splits across two crates |

`MilestoningStrategy` (51 lines, reached first by E) goes wherever (a) or (b) puts E's helpers. *Leaning: (a).*

**ASK 3. Port the misfiled element classes with their first caller rather than in F's slice?**

| Code | First caller |
|---|---|
| `ClassLayouts` | I |
| `EqualityKeys.resolve` and `fqnOf` | I and the planner |
| `EqualityKeys.declaresKeys` | testplan |
| `RelationalOpRows` | H and the planner |
| `Temporal` | G, H, lineage, the planner |
| `StoreCompiler.declaredType` | G |
| `ModelContext.isDeclaredSubtype` | G½ |
| GE3 | the Cypher wave |

| Option | Cost |
|---|---|
| (a) With the first caller | About 1,000 lines leave F's slice. F stays small and cannot reach G. `ClassLayouts`' memo becomes a `Model` field that lowering fills (D5's `layouts`) |
| (b) In `lite-element` now, as Java's package has them | `Temporal.anyTemporalGetAll` cannot compile there (it walks `TypedSpec`: PHASE3_CRATES §2's element → spec edge). `lite-element` would carry code no F caller uses, with no differential until its callers exist |

*Leaning: (a).*

**ASK 4. GraphEdges' F-time rule (GE1 and GE2, about 450 lines).** It runs on every compile. It refuses a misdeclared
edge class, but only in models that use the `graph::graph` stereotypes, and it is a lite-only rule ("only lite checks
the rule, since only lite runs the island", GE:49). The island it serves (Cypher) is a later wave (TRIAGE).

| Option | Cost |
|---|---|
| (a) Port with F | About 450 lines no product query in iteration 1 exercises beyond the check |
| (b) Defer the whole reader to the Cypher wave, with a ledger row | Until then a model with a misdeclared edge class compiles in Rust and is refused in Java: an answer difference in `compileErrors`, on models no oracle row has (unmeasured) |

*Leaning: (a) if the Cypher islands are ported in iteration 1, (b) if they are not.*

**ASK 5. The execution overlay (`withExecutionOverlay`, about 70 lines).** Its only callers are
`test/ServiceTestRunner:434` and the corpus harness (`MinimalCorpus:302`). It shares the base's memo maps.

| Option | Cost |
|---|---|
| (a) Port it as a view struct `{Arc<Model>, runtime, connection}` | Needed if the corpora run through the Rust CLI (PHASE3_DESIGN §2.5 step 5) |
| (b) Keep it in the Java shim | DON'T PORT; the corpus lanes then keep a Java model for the overlay |

**ASK 6. Hash order that reaches an answer.** Java's order at all three sites below is a `HashMap`'s or `HashSet`'s, and
`IndexMap` gives insertion order, which is not that.

| Site | Path to the answer | Oracle |
|---|---|---|
| (a) `MappingPrePass:231/237` | The "Circular M2M ~src chain detected ...: A -> B -> A" text spells the chain from the first root in `HashMap.values()` order | Blind (step 0's 11/0: no real chain in any lane); only `contains("Circular M2M")` in `MappingNormalizerTest:4588` |
| (b) `PureModelContext:330` | `testplan/JudgmentFacts:117` iterates `resolutionUniverse()` when a key slot is declared `Any`; the order becomes a test plan's `byClass` JSON (`HostJson:655`) | Unknown: no test source declares an `Any` key; check `test-plans-host` |
| (c) `PureModelContext:346` | `elementFqns` → `StoreResolver:659` → `ClassMapping.classOfWitnessPrefix`'s first match; matters only when two FQNs mangle alike (`a::b__c` vs `a__b::c` → `stc_a__b__c___`), when the order picks the class name written into SQL | Unknown |

| Option | Cost |
|---|---|
| (i) Emulate Java's order (`String.hashCode`, the table's growth, the union with `platformFqns`) | Exact, and ugly |
| (ii) A declared order (declaration or sorted) plus a `SEMANTICS_REGISTER` row | Moves the text or plan wherever reached |
| (iii), for (c) only | Refuse the collision by name instead of picking |

Only (a) is in E/F's slice; (b) is testplan's and (c) H's. *Leaning: (ii) for (a) and (b), (iii) for (c).*

**ASK 7. `FunctionCompiler:77`'s `catch (RuntimeException)`, which drops a broken overload.**
- When all overloads fail it rethrows the first exception; otherwise the broken one silently loses dispatch.
- Its comment says a strict build never gets there. By reading it can: `checkFunction` never validates multiplicity
  bounds, so `f(x: String[2..1])` compiles. Then `findFunction` throws a raw IAE that `compileAllBodies` walls as
  "upper (1) must be >= lower (2)". Association ends have the same gap (PMC:439).

| Option | Cost |
|---|---|
| (a) Replicate as a typed `Result` drop: Rust drops `Err` refusals; a panic propagates | Same answers; satisfies TRIAGE A.4's "audited, not replicated" |
| (b) A poison sentinel (a broken overload scores, then throws if it wins) | Changes which overload wins and the text |
| (c) Close the integrity gap: `requireValidBounds` on function parameters, returns and association ends | The error moves from a body wall to an element error with lite's "invalid multiplicity —" text |

*Leaning: (a) for the port, plus (c) as a Java-and-Rust fix the owner rules, in the manner of slice 1b's ten.*

**ASK 8. PARK-1's duplicate.**
- The XStore direction rule is "implemented TWICE verbatim" (PARKED_WORK_LEDGER:45-46): MN:1015-1025 and XS:221-233
  are token-identical.
- `ParkedWorkLedgerTest`'s anchor pins it to exactly those two Java files.
- The ledger's own acceptance asks for "ONE implementation".

| Option | Cost |
|---|---|
| (a) One Rust function, with the anchor restated against it (a `RETIRED_TESTS.md` row for the Java anchor) | Answer-identical by H3's comparison |
| (b) Two Rust sites, mirroring the anchor | Keeps a known duplicate |

The per-end redesign stays parked either way. *Leaning: (a).*

**ASK 9. PARK-26: a mapping included under two spellings is refused as an include cycle** (StoreSubstitutionRewrite:
`pending` counts list entries, and the decrement fires once per mapping).

| Option | Cost |
|---|---|
| (a) Port the wrong refusal exactly | Same answer |
| (b) Refuse it as "Duplicated mapping include" by the resolved name | Changes a refusal text |
| (c) Accept it | Changes a refusal into an accept |

A Rust in-degree count over a set would silently choose (c), so the choice must be deliberate. The ledger: the engine's
rule "is not in the repository's upstream notes". *Leaning: (a) until the engine's rule is read.*

**ASK 10. Tolerant-mode wall shapes.** These matter only to `buildModule`, whose sole non-test caller is the corpus
harness, and through it the corpus rows of the frozen plan oracle.

| | Quirk | Site |
|---|---|---|
| (a) | View walls record the **full** message with no element attribution; every other E wall records the first line, attributed | LV:155 |
| (b) | Refusals that escape a tolerant build instead of walling: `resolveAllStores`' "Duplicated mapping include" and "mapping include cycle" | MN:161, outside every catch |
| | (same) E.4's untyped-parameter error and E.3c's ISE | ModelNormalizer |
| | (same) `AggregateViewLift.lift`'s failure, which walls the whole mapping, not the set | outside the per-class try |
| (c) | **Orphan lifted functions**: a mapping walled after some of its functions were appended stays out, but those functions stay in the model | -- |
| (d) | An association `NotImplementedException` goes to `strictErrors` too, contrary to its own comment "TOLERANT (module) builds only". (2026-11-15: not a tolerant-mode shape alone -- the normalizer batch's fix, `47d13b73f` (d), changed what a STRICT build answers, so it was engine-observable, though tagged LITE-ONLY; the normalizer's second batch, item 4 (e), `e8ac002d0` and `2505f9d1d`, asked legend-engine site by site and refuses in its words where it refuses at compile, PHASE3.md) | MN:425-428 |

| Option | Cost |
|---|---|
| (i) Port all four exactly | No answer moves |
| (ii) Fix them in Java and Rust together, then re-freeze | Changes some corpus walls; whether any frozen corpus row moves is unmeasured |

*Leaning: (i) now, (ii) as a listed follow-up.*

**ASK 11. Raw Java exceptions a user's model can reach.** Rust cannot reproduce an NPE or a JDK message. "Behaviour that
merely leaked out of the JVM is dropped" (TRIAGE's rule).

| Site | What happens |
|---|---|
| ROT:610 | An ISE whose text embeds `op.getClass()` (a lambda argument to a non-UNSUPPORTED dynafunction) |
| ROT:720 | An ISE "declared TRANSLATED but no translator arm rewrote it" (a guard falls through) |
| MJN:118 | An NPE: a nested ModelJoin hop to a Pure-mapped end; the top-level path walls the same case |
| MN:943, :1058 | `NoSuchElementException` (only past an unresolved association name) |
| MN:1088, MJN:59, MJN:117 | `Map.of`/`Set.of` IAE if a ModelJoin lambda's two parameters share a name (whether the parser admits that is unverified) |
| PMC:415-440 | `findProperty`'s unbounded recursion on a tolerant build's walled cyclic class (StackOverflowError) |
| PMC:439 | An association end `[2..1]` raises a raw IAE at G time |

| Option | Cost |
|---|---|
| (a) A Rust internal error (a panic caught at the FFI) carrying the same text where one exists | Answer-identical except for class names |
| (b) Wall each as `NotImplementedException`/`ModelException` with a text modelled on its sibling | A crash becomes a refusal |
| (c) Fix the Java first, then port | As (b), decided in Java first |

*Leaning: (c), as one list.*

**ASK 12. Divergences and defaults the code documents with no `SEMANTICS_REGISTER` row.** AGENTS.md: "a divergence that
is not a row is a defect". Per item: port as is and add a row, or change it (an answer moves).

| Where | What the code says or does |
|---|---|
| US:101-102 | "Declared divergence": a pin the queried mapping cannot see still navigates "where the engine fails" |
| GBS:158-185 | Withholds Join PMs and per-row PMs outside the keys, where "the engine's compiler ACCEPTS the mapping" |
| VR:209-224 | "H2-LENIENT": an unmatched view column becomes a `first()` witness |
| AS:447-454 | "First-PM-wins is RETAINED ... two directions with genuinely NON-equivalent joins would still take the first silently" |
| DC:47-51 | "DELIBERATE divergence (audit 19 F6)": a declared-Boolean string errors in SQL where the engine's `Boolean.parseBoolean` yields false |
| ROT:357-362 | `splitPart` index shift; "Empty tokens still differ" |
| ROT:266-271 | `dayOfWeekNumber`'s Sunday-based conform |
| MN family 19 | `booleanizeCaseLiterals` ("engine keeps the strings in SQL"), the `trustOne` wrapping, the JSON `to(get)` fan-out (lite-own emissions) |
| MN:1205-1213 | `m2mPropertyValue` passes the raw expression through on three misses; an uncited `AllVersions` strip |
| MN:2322-2333 | `mainTableOrNull`: an ambiguous root set is skipped and a non-root set's table returned |
| JCE:1041 | `nullTolerant`'s `default -> false` (it licenses LEFT+WHERE ≡ INNER; "Real INNER emission is follow-up") |
| CC:86 | Any enforcement level but "Warn" (null included) becomes ERROR |
| `MilestoningStrategy:34` | `default -> null` for an unknown `temporal.*` stereotype |
| `ViewSignatures:102` | An unknown column becomes `Any[0..1]` |
| LV `spell` | Collapses any multiplicity but [1] to [0..1] |
| ModelIntegrity, TC | Lite-worded messages: "Inheritance cycle: ...", "defined more than once with the same signature", "invalid multiplicity —", "binds unknown class/association", "binds to unknown function", "must be realized by", "Unknown type: ..." |

*Leaning: port as is; the owner rules a row or a change per item, as with slice 1b's list.*

**ASK 13. Java behaviours that look wrong** (the slice 1b precedent: confirm each, then fix it in Java and Rust together,
or port it as is).

| Site | What it does |
|---|---|
| AS:308-309 | Rebuilding an Embedded block with an injected association entry **drops its `~primaryKey`** |
| AS:397-412 | An unknown XStore or ModelJoin association gets "Association mapping kind ... not supported" instead of "references unknown association" |
| VR:451-454 | `pmThroughFrame`'s default passes `EnumeratedExpression` and `OtherwiseEmbedded` through unrewritten: a view-backed set using either reads root-table columns outside the frame |
| DC | `declaredAssertion` does not see through views; `coerceColumnToDeclared` does |
| MF:42, :98 | Matches members by the **raw** `setId()` (null when the user wrote no brackets) where every other site uses the effective id; `unionMembers` keeps the last match |
| MN:2460 | An anonymous enumeration reference prints "EnumerationMapping 'null' value ..." |
| MN:2386-2389 | An id lookup takes the first match across includes, where the anonymous path is loud on two |
| LV | A Database declared twice (tolerant only) emits each view's function twice |
| SSR:318-319 | The cycle message lists every mapping never made ready, not only the cycle's members |
| AVL | Aggregate view sets' `DeclaredKeys` look always NONE (the pre-pass never captures them) |

**ASK 14. `DynaFnRegistryTest` in Rust.** The test asserts that the translator's arm set (read from `RelOpTranslator.java`'s
source by the regex `DynaFn\.X`) equals `DynaFnArms.ARMS`, that TRANSLATED ⊆ ARMS ⊆ TRANSLATED ∪ PURE, and that
`LANDINGS` ∪ SHIM rows = `Pure.ENGINE_VOCAB_SHIMS`.

| Option | Cost |
|---|---|
| (a) Retarget the source-text check at the one Rust translator file (`DynaFn::X`) | The file must name every armed member literally and no other |
| (b) A structural form: the arm guard as one exhaustive `fn arm(DynaFn) -> Option<..>` from which ARMS is computed | A `RETIRED_TESTS.md` row for the Java test |

Either way, ARMS and LANDINGS live in Rust (`lite-normalizer` or `lite-builtin`). *Leaning: (b).*

**ASK 15. The name-resolution fallbacks E still carries** (`FallbackLedgerTest` calls them "Phase-D debt ... FIXLIST
P7-3, which is not scheduled work"):
- AS `resolveAssociation`'s import wildcards and "same-package fallback" (:376-390);
- MN's `.orElse(am.associationName())` (:432, :441);
- SSR `qualifyStoreRefs` (likely never rewrites now).

| Option | Cost |
|---|---|
| (a) Port them verbatim | The debt moves to Rust |
| (b) Measure first (count the inputs that reach them past D over the lanes), and delete what is unreached, with D (the names slice) total for mapping bodies | One measurement; answer-identical where unreached |

*Leaning: (b), since the names slice is in flight.*

**ASK 16. How many subtype relations does Rust keep?** Java has four, and they answer differently:

| Relation | Over | Primitives | `Nil` | On a failure |
|---|---|---|---|---|
| `KnowledgeLayer.isSubtype` (E, names-tier) | declarations through `hierarchyClass` | none | no rule | never throws |
| `PureModelContext.isSubtype` (F's, used by G and H) | compiled classes | included (`Integer <: Number` true) | the bottom | rethrows the walk's first failure when the answer would be false |
| `ModelContext.isDeclaredSubtype` (G½) | `findClassDefinition` | includes their catalog nodes | -- | -- |
| `UserCallInliner.declaredAncestors` | -- | -- | -- | -- |

- The names brief's "one subtyping" rule (item 5) was conditional: *if* `PureModelContext.isSubtype` delegates to
  `KnowledgeLayer`'s kernel. It does not: it walks compiled classes on its own. The names slice's own `knowledge.rs`
  already says the two are different relations. So this is a confirmation, not a conflict with a ruling.
- Slice 1b's `lite_model::subtyping` ports PMC's walk over declarations, but models only `headFqn`'s failure.

| Option | Cost |
|---|---|
| (a) Keep the relations as distinct, named functions, each ported line for line, with `lite_model::subtyping` folded into whichever it is | Same answers |
| (b) One relation | Changes answers wherever they disagree (primitives, `Nil`, failures) |

*Leaning: (a), and say so in PHASE3_CRATES.*

**ASK 17. The verification for E and F** (§10): approve a live differential of E's `NormalizedModel` and of F's answers
before the slice lands, as slice 1b and the names slice did for theirs. *Leaning: yes.*

## 6. Crate-split edges (PHASE3_CRATES §2; `tools/deps/core-layers.txt`)

`core-layers.txt` lets `normalizer` reach `compiler` (all three parts in Rust), `compiler_element_type`, `builtin`,
`platform`, `model`, `protocol` and `error`. It lets `compiler` reach `sql` and `values`, but not `setup`: element's one
`com.legend.setup` mention, `ModelContext:155`, is javadoc. Every Java edge below is within those lines. The question is
the Rust order: names < element < spec.

| Edge | Where | What crosses | Resolution |
|---|---|---|---|
| **normalizer → spec** | `Calls` in MN, JCE, US, GBS, AS, ROT, VR, XS, MJN, RR, DC (about 120 uses) | The one call builder | Re-home `Calls` in `lite-names` (ASK 1); otherwise E waits for `lite-spec` |
| normalizer → element | LV:99 `ViewSignatures.of`; `MilestoningFacts:7` `MilestoningStrategy` | E's helpers living in F's package | Move them with ASK 2; then E reaches only names and types |
| normalizer → types | JCE:630 `PlatformTypes.ROWS_MARKER`; VR:90-97; LV:98, :123-140; ModelNormalizer:488 | `lite-types` | allowed |
| normalizer → names | `ModelBuilder` and `knowledge()` everywhere; `SynthFqn`; `ResolvedNames`; `KnowledgeLayer.canonicalTable`; `RelationalKinds`; `DerivedProps`; `NameResolver.resolveQueryIn` and `platformFqns` (E.6) | The index and name resolution | allowed; E.6 is a second resolution pass E runs itself |
| **names → element** | `KnowledgeLayer.columnKind:406` → `RelationalTypeInference` | A computed view column's kind | Gone if ASK 2 (a) |
| names → element, names → spec | `KnowledgeLayer`, `StatementInline`, `LiteralMapUnroll` (PHASE3_CRATES §2's rows) | `ModelContext`, `Calls`, `SourceSubst` | The names slice's own; `Calls` is ASK 1 |
| **element → spec** | `TypeClassifier:47` | `InferenceKernel.ENUM_METACLASS_FQN` | Use `PlatformTypes.ENUM` (the same string, already in `lite-types`): RE-EXPRESS, the edge goes |
| **element → spec** | `Temporal.anyTemporalGetAll` :78-109 | Walks `TypedSpec`/`TypedGetAll` | `Temporal` is not F's: the planner ports it (ASK 3) |
| element → names | `ModelBuilder` (PMC, MI, FC, TC), `SynthFqn` (MI, CC), `StoreLookups` (ModelContext, RTI, ViewSignatures), `NameResolver.platformFqns` (PMC:316), `KnowledgeLayer` (via `knowledge()`) | The index | allowed. `refuseDuplicates` (PMC/MI) is names-tier work in F's files: move it beside `duplicate_elements` |
| element → sql | `ClassLayouts:31,37` (`SqlExpr.SYNTHETIC_ID`/`_TYPE`); `StoreCompiler.declaredType` (`SqlDdl.ColumnType`) | Constants and a DDL type | allowed. Both go with their first callers (I, G); `lite-sql` gains the two constants when I ports `ClassLayouts` |
| element → platform, builtin | PMC (`Registrations`, `DeclarationTable`, `ImplementationTable`), FC (`Pure.nativeFunctionsAt`, `DecisionProbe`) | Platform tables | allowed (`DecisionProbe` is not ported) |
| element → spec (G uses F's types) | `SpecCompiler:48`'s identity memo keyed by `TypedFunction` | One instance per overload | Not an edge, but a contract: F's function cell must hand out one `Arc` per overload |

**Where E/F code lands, if the leanings hold:**

| Crate | Code |
|---|---|
| `lite-names` | Gains `Calls`, `RelationalTypeInference`, `ViewSignatures`, `StoreCompiler.columnType`, `MilestoningStrategy` (or `lite-normalizer`) and `refuseDuplicates` |
| `lite-normalizer` | E, about 11,000 lines |
| `lite-element` | About 3,200 lines: `Model` (PMC as a concrete type with ModelContext's signatures), `ModelIntegrity`, GE1/GE2, `TypeClassifier`, `FunctionCompiler`, `ClassCompiler`, the `Typed*` records, `Property`, `StoreCompiler.tableSchema`, `EqualityKeys.isKeyStereotype` |
| `lite-types` | Gains `Multiplicity.from` and `PlatformTypes.eraseTdsRow` |
| Later crates | Everything else in `compiler/element` waits for its caller's slice |

The `split compiler names element spec` line then holds with no exceptions.

## 7. What the Java's own docs call a known defect, shim, probe, ledger, census or schedule

| Item | Where | Status |
|---|---|---|
| PARK-1: XStore per-end predicates walled ("has direction-specific conditions"); the rule "implemented TWICE verbatim ... neither copy has a test" | MN:1019-1023, XS:227-231; `ParkedWorkLedgerTest:45-49`; PARKED_WORK_LEDGER:20-54 | A parked divergence; ASK 8 |
| PARK-26: an include under two spellings refused as a cycle | SSR:268-311; `ParkedWorkLedgerTest:173-177`; PARKED_WORK_LEDGER:644-668 | A known defect; ASK 9 |
| The `MissProbe` funnel, "F7.8 VERDICT funnel", census of 2026-09-15; `neverFired` "this default never fired on the 2026-09-15 B5 census" | `MissProbe.java`; `FallbackLedgerTest` (31 funnel sites in 11 files, plus the funnel's own, never-fired floor 5); MN:2395 and JCE:140, US:686 inline variants | A census and ledger; RE-EXPRESS (§4) |
| "the lenient NAME-RESOLUTION fallbacks Phase E still carries ... Phase-D debt ... FIXLIST P7-3, which is not scheduled work" | `FallbackLedgerTest:39-45`; AS:376-390; MN:432, :441; SSR `qualifyStoreRefs` | ASK 15 |
| `inferMainTableQuiet` "as a PROBE: null on ambiguity instead of loud" | MN:1440 | RE-EXPRESS (§4) |
| `DecisionProbe.INSTALLED` (a `ServiceLoader` measurement hook, `LL_SHADOW`) | FC:58-60 | DON'T PORT |
| "tracked, task #56": walled overloads hidden by the drop; "A STRICT build never gets here" (false by reading) | FC:78-85 | ASK 7 |
| "Real INNER emission is follow-up"; "the next rung must gate that threading" | JCE:942-947, :921-925 | A known gap; ported as is |
| "the typed-local read is an open leg of route A" | XS:276-280 | A known gap |
| "~primaryKey ... currently a no-op"; "roadmap feature"; "H3-pending wall downstream"; "await the H5 set-ID dispatch story" | MN:1775-1776, :306-309/:1177/:1187/:2562/:2641, :1784, :270-271 | Roadmap walls (answer text) |
| "Declared divergence" (no register row) | US:101-102 | ASK 12 |
| "DELIBERATE divergence (audit 19 F6)" | DC:47-51 | ASK 12 |
| "H2-LENIENT" | VR:209-224 | ASK 12 |
| "an inheritance CYCLE ... which the frontend still accepts" (DEEP_AUDIT §5b) | ModelContext:420-425 (the dead default walk) | Moot in Rust (MI3 refuses cycles in strict builds) |
| "the walk keeps a delegating shim for the constructed-instance tests" | RTI:22-24 | **Stale**: `MetamodelWalk` is gone |
| `RequiredNullableCensus` hook | DC:27-30 | **Stale**: the class is gone |
| Shadow walkers pinned by `ShadowWalkerCensusTest` | JCE `hasMappedSubclass`; US `collectInheritanceMembers` | A census; retires with the Java |
| `findLegacyMapping` reach-backs pinned by `LegacyReachbackCensusTest` | MappingClosures (2: :95, :375), PMC (4), ModelContext (1) | A census; retires with the Java |
| `CodeShapeGuardrailTest` allowlists: "normalizer emission frame: Pipeline IS the frame object"; MappingClosures' lazy fields "a memo of a pure derivation"; PMC `declarations`/`implementations` | `CodeShapeGuardrailTest:65-70, :355` | A ledger; retires with the Java |
| `ArchitectureTest.PROTOCOL_DESUGAR_DEBT` pins TC's one protocol node | TC:111; `ArchitectureTest:2492` | RE-EXPRESS removes it |
| `ErrorShapeGuardrailTest` pins FC's one catch | `ErrorShapeGuardrailTest:68` | Retires |
| Stale comments that contradict the code | PMC:274 (the overlay does see a derivation), PMC:35 ("not thread-safe": its concurrent maps are for re-entrancy), GE:48 ("eight" stereotypes; there are 15), US:840 (`liftMidSteps` no longer exists), PL:52-57 and JCE:412-416 (the "member-suffixed `FirmID_1`" names are made by key threads, not routes), ModelNormalizer:82-83 (the order "is free") | Do not port the comments |
| Nothing in E or F is "scheduled for deletion" by its own docs | -- | The probe package (`probe`, scheduled) is not E/F |

## 8. The model methods slice 1b deferred, and which E/F class calls each

`rust/lite-model/src/lib.rs:20-23` lists them. By call site:

| Deferred item | E/F callers | Not E/F (the resolver's and others') |
|---|---|---|
| `MappingDefinition`'s canonical ctor: `ClassBindings.of` (byClass, bySetId, byFunction, the `position` `IdentityHashMap`), `AssociationBindings.of` | **E constructs it**: MN:448 (6-argument, with facts), MN:532 (5-argument, clean-sheet); `ClassBinding.Relational`/`Operation`/`Pure` built at MN:322-404, :505-513 and AVL:39; `AssociationBinding` at MN:438, :524, :527 | -- |
| `ClassBindings` lookups `ofClass`/`withSetId`/`realizedBy`/`ofClasses` (and `position`'s sort), `associationBinding`, `classBindingsWithIncludes`, `withIncludes` | **None in E or F.** MI iterates the lists only | The resolver (ClassSources, StackBuilder, ChainNormalizer, GraphEmission, AssociationJoins, ElementReferences, CastReRoot, ...), lineage (`ColumnLineageRows`), the translators. `position` (step 0: 23,989/0) is H's: RE-EXPRESS it as the binding's index in the `Arc<[ClassBinding]>`, with the per-class lists holding indices |
| `NormalizationFacts` (the copying ctor; keyed lookups over its maps) | Built by `MappingLedger:86` (with `MappingFacts`). Read by key at PMC:488, :495, :504, :511, :520, :527, :535 (F) | -- |
| `ClassMapping` witness-column names: `memberWitness`, `subTypeColumn(Prefix)`, `isSubTypeColumn`, `classOfWitnessPrefix`, `primaryKeyBinding`, `isPrimaryKeyBinding`, `foreignKeyBinding`, `witnessPrefixOf` | Only `memberWitness`, at US:701, in a **dead** field (US-7). None needed by E or F | All the resolver's (GraphEmission, Substitution, StackBuilder, CorrelatedSubselects, ClassSources, ChainNormalizer, ForeignKeyIdentity, GenericTypeReflection, SyntheticHeads, ElementReferences) |
| `ClassMapping.Relational.aggregationAwareMain` | MN:1382 | `AggregationAwareRouting` (H) |
| `ClassMapping.RelationFunction.Col.isEmbedded` | MN:864 | -- |
| `ClassMapping.RelationFunction.Col.bindSrc` | MN:888, RR:131 | -- (it needs a `map_children` over the `Arc` source tree; neither lite-model nor lite-front has one) |
| `LegacyMappingDefinition.withClassMappings` | AS:219, `ImplicitInheritance:97,195`, `MappingClosures:127`, `MappingPrePass:152`, SSR:396 | `ModelBuilder.from` (JSON-connection cross-bake, per its javadoc) |
| `RuntimeDefinition`'s helpers (`widen`, `deepCopy`: the convenience ctors) | **None.** E and F read record components only (`MappingClosures:100-101`, MI:121, :124; PMC/ModelContext name the type) | -- |
| `ClassMapping.PropertyBinding.withExpression` | none | `NameResolver` (D) |

**Also needed by E/F and not yet in Rust** (not on slice 1b's list):

| Item | E/F callers |
|---|---|
| `FunctionDefinition.withSynthesizedFrom` / `isSynthesized` | ModelNormalizer (6 sites), LV:106, AS:544, XS:258, MI:362 |
| `RelationalOperation`'s default methods `children`, `mapChildren`, `navigatesJoin`, `tables` | ROT, VR, MI:571. The live names worktree has an untracked `relational_operation.rs` |
| `ValueSpecification.children`/`mapChildren` and `AppliedFunction.withParameters` (protocol, `lite-front`) | XS, RR, MJN, GBS |
| `DatabaseDefinition` lookups beyond the default schema | MI:579, :591; PMC:710-727, :855 |
| `KeyThread`'s 4-argument ctor | US:967 |
| `PoisonKey` | US:392 |
| `ClassMapping.Relational`'s 13-argument rebuilds | US:506, :567 |
| `PropertyMapping.Embedded`'s 2-argument ctor (the one that drops `~primaryKey`) | AS:308 |

## 9. The idioms, gathered

### 9.1 Identity (step 0's rows for E and F, and the verdict here)

| Site | Step 0 (hit/miss) | Verdict |
|---|---|---|
| LV `owners` :49, `functions` :52, `walls` :54, `expanding` :56 | 112/0, 393/112, **0/112**, **0/112** | Semantic: two equal views in two schemas are two views. PORT with `IdMap`/`IdSet`; the Rust index must hand out one `Arc` per view from both `findView` and `viewsOf`. `walls` and `expanding` are oracle-blind |
| `ViewSignatures.expanding` :51 | **0/137** | Semantic; PORT with `IdSet`; oracle-blind |
| PMC `CheckedLayer` :104 | 6,478,837/183,150 | Semantic (boot provenance). PORT as the boot's `IdSet`; the graph index must hold the boot's very `Arc`s. Both directions of a mistake are caught (under-matching fails every compile) |
| `MappingClosures.first` :297 | **0/2** | Dead (the same-object arm never fires). RE-EXPRESS: drop it, never a derived `==` |
| `MappingPrePass.selfEdge` :259 | 11/0 | ≡ `sourceClass == className`. RE-EXPRESS. Real chains are oracle-blind |
| `AssociationSynthesis.self` :202 | 68/66 | ≡ "index 0". RE-EXPRESS |
| `ModelNormalizer.adopted` :220 | 9/23 | An internal assertion. RE-EXPRESS as a typed step or `same` |
| `ModelBuilder.findAssociationAtClass` (reached from MJN:71) | **0/0** | The names slice's; MJN's nested path is oracle-blind |
| `MappingDefinition.position` (built by E's ctor) | 23,989/0 | H's lookup; RE-EXPRESS by index (§8) |
| `TypedFunction` as `SpecCompiler.memo`'s key (G's site, F's contract) | 3,031,740/245,638 | F's function cell must return one `Arc` per overload; equality stays derived over all fields |
| Local comparisons: MN:1003 / XS:202 (`thatRow == srcRow`), ROT:751, ModelNormalizer:177 (rebuild detector) | not counted | Accidental: structural or a bool |

### 9.2 Hash order (TRIAGE C1)

Every non-Linked `HashMap`/`HashSet` in E and F was checked.

TRIAGE's normalizer row counts "7 HashMap iterations". This census finds five iterations or copies of a hash-ordered
collection in E, and one of them reaches an answer:
- `MappingPrePass:237` (reaches the "Circular M2M" text);
- `ImplicitInheritance:43` and `:123`;
- the hash-ordered copies `ModelNormalizer:159/164` and `MappingLedger:74`.

F has two more, both iterated by readers outside F: `PureModelContext:330` and `:346`.

The outcome:
- **Three iterations reach an answer** (ASK 6): `MappingPrePass:231/237`, `PureModelContext:330` (through testplan) and
  `PureModelContext:346` (through H).
- Two iterate with no visible effect: `ImplicitInheritance:40/43` (a size-1 test) and `:121/123` (into a membership
  set).
- `MappingLedger:74` copies hash order into a `LinkedHashSet` that only `contains` reads.
- `FunctionCompiler:49`, `ModelIntegrity:286/342/443/464`, `GraphEdges:88`, `EqualityKeys:87`, `Temporal:119`, the
  `Pipeline` sets and every `seen` set are membership- or lookup-only.
- Every map whose order reaches output is already a `LinkedHashMap`/`LinkedHashSet` or a list. Rust needs
  insertion-ordered maps there (the slot maps, the scope maps whose `keySet()` is printed in ROT's wall texts,
  `NormalizationFacts`' inputs, `byClass`/`bySet` in AS-1).
- `NormalizationFacts`' maps become **spec in order** in Rust (an ordered `Arc<[(K,V)]>` with derived `PartialEq`),
  where Java's `Map.equals` is order-blind. A differential of E (§10) compares them in insertion order, which is
  stricter than `MappedInClosureTest:131`.

### 9.3 Memos and D5 (PHASE3_DESIGN §2; ruling 4)

| Memo | Today | In Rust, ruled |
|---|---|---|
| `classCache`, `enumCache`, `functionCache` | HashMaps, no failure cached; `functionCache` caches an empty list for any FQN asked | `OnceLock<Result<_, Refusal>>` per FQN known at freeze; an unknown FQN answers empty with no cell (the same answer). Caching a failure returns the message Java would recompute |
| `derivedCache` | **10 keys at this commit, not 8.** `Reach`, `LayoutMemo`, `OpSeeds`, `Catalog`, `Labels`, `Ends`, `GraphEdges`, `Projects`, `SeedMemo`, `GraphKey`. **`SystemDatabase` is gone**, so ruling 4's "SystemDatabase stays in Java on the shim's model object" has nothing left to keep | Named fields. Open-keyed: `Reach`, `LayoutMemo` (3 maps), `Projects` (keyed by platform/declared and the dialect's class), `SeedMemo` (keyed by `DatabaseType`): each a `Memo`. The rest one `OnceLock` each (`GraphEdges` as a cached `Result`). **Only `Reach` is F's slice**; the others arrive with G, I and the planner |
| `packages`, `declarations`, `implementations` | Lazy | Built at freeze |
| `CheckedLayer` | The process-wide boot fact (`Compiler.BOOT`) | `static OnceLock<Arc<Boot>>` |
| `MappingClosures` (on `KnowledgeLayer.derived`) | Listed in §2.1 as written after `from`; **by reading it is written and read only inside E** | E-local (§4), not a model field |
| `LiftedViews.functions`/`walls` | Per normalize call | E-local `IdMap`s |
| The overlay | Shares the base's memo maps by reference | ASK 5 |

No E or F code runs on two threads. The two `ConcurrentHashMap`s exist for re-entrancy (a derivation that reads another),
not threads, so the Rust memos must compute outside any lock, as §2.5 says.

### 9.4 Exceptions as control flow, and `catch (RuntimeException)`

- **The one `catch (RuntimeException)` in E/F**: `FunctionCompiler:77` (ASK 7).
- **The poison and wall machinery is the answer.** It must be ported exactly: which catch, which kind goes to
  `strictErrors`, `withElement`'s NIE → ModelException conversion, first-line vs full message, element attribution, the
  `"; "` append, declaration order. The sites:
  - MN:198, :214, :241-254, :345, :364, :419;
  - `MappingPrePass:73`, `MappingValidation:67`;
  - LV:109 (a memoized failure, rethrowing the same object) and LV:149;
  - MI:234 (`withElement`).
- **Probes by exception** become `Result` → `Option` (§4): MN:1445, JCE:876, PMC:257, MI:434, MI:486.
- Java's `split("\n")[0]` (the first line of a wall) drops trailing empties (TRIAGE C4). On a message of only newlines
  it throws; that is unreachable in practice, and Rust's `split().next()` differs only there.
- **Java class names in answer texts**:
  - AS:411, :434 (`getSimpleName`);
  - MI:553 (`getSimpleName`);
  - ROT:610 (`getClass()`, raw ISE);
  - ROT:685-686 (`value.getClass().getName()`, a literal's Java FQN).

  Rust must spell each per variant, or the owner drops it (ASK 11).

## 10. Oracle coverage, and what the slice needs to be verified

What is frozen today, and what it can see of E and F:

| Oracle | What it sees of E and F |
|---|---|
| `rust/oracles/plan` (51,337 rows, plan JSON by hash) | Only downstream of G½ to I: a `NormalizedModel` reordered but equivalent could pass it, and a wrong one surfaces only as a plan diff far from its cause. It does hold E's poison texts inside plans (MN:1176's "M2M explosion ... roadmap feature" text) and the corpus rows that depend on tolerant mode |
| `rust/oracles/identity` | Counts at the identity sites. 0-hit sites prove nothing: LV `walls`/`expanding`, `ViewSignatures.expanding`, `MappingClosures.first`, `findAssociationAtClass`, and the real `~src` chains behind `selfEdge` |
| `rust/oracles/model` | Parse only (the text → `ParsedModel`), not E or F |

E's and F's own error texts are in no oracle found. Of MN's about 46 texts, 23 were searched and one found (via a poison
suffix, inside a plan); none of about 15 pre-pass texts was found (H1, H4). The database judge's archived test plans
were not searched. The same goes for
ModelIntegrity's 13-check order and texts and every `buildModule` wall. Today only Java unit tests' `contains(...)`
checks guard them.

**Recommended (ASK 17), modelled on slice 1b and the names slice:**

1. **E's differential.** For every model text of the front oracle (the `pmcd` asks at each dialect, strict and
   tolerant), the canonical text of the `NormalizedModel` E hands to F. Slice 1b's generated codec already covers it:
   "190 types reached from `ParsedModel`, `NormalizedModel`, ...". Include, in order:
   - every lifted function;
   - the binding tables;
   - `NormalizationFacts` in insertion order;
   - `resolvedStores`;
   - the walls map.

   Java's hashes frozen under `rust/oracles/normalizer/`, two runs byte-identical.
2. **F's differential.** On the same models:
   - `compileErrors`' answer (strict: the first error);
   - the tolerant walls;
   - for every element FQN, the typed element's canonical text (`TypedClass` with properties, constraints and the key
     flag; `TypedFunction` signatures and the overload candidate order; `TypedEnum`; `findType`'s classification);
   - `isSubtype` over the class pairs;
   - the declaration and implementation tables.
3. **Targeted cases for the oracle-blind sites**, made while Java exists:
   - a view-on-view cycle, a walled view reached from a mapping, and two equal views in two schemas;
   - a real `~src` chain and an M2M cycle (with ASK 6(a) ruled);
   - a duplicate set id across includes;
   - a nested ModelJoin hop;
   - a `[2..1]` function parameter (ASK 7).

## Appendix: the evidence files

- `ef-census/H1_MappingNormalizer.md`: MN's 19 families, every catch, poison and text.
- `ef-census/H2_JoinUnion.md`: JCE, US, `Pipeline`, GBS, AVL, `SetKeyFacts`, `MappingLedger`, `MappingFacts`;
  the names that reach SQL and their oracle evidence.
- `ef-census/H3_AssocRelOp.md`: AS, ROT, VR, XS, MJN, RR, DC; `DynaFnRegistryTest`'s needs; PARK-1's two sites
  compared.
- `ef-census/H4_Frame.md`: `ModelNormalizer`, LV, `MappingPrePass`, `MappingClosures`, `ResolvedMapping`,
  `ImplicitInheritance`, SSR, `MappingValidation`, `M2mRouteGuards`.
- `ef-census/H5_ElementCore.md`: PMC, MI, GE, FC, TC, CC; the D5 table; the four subtypings.
- `ef-census/H6_ElementHelpers.md`: `ModelContext`, `ClassLayouts`, `EqualityKeys`, `RelationalOpRows`, RTI,
  `ViewSignatures`, `Temporal`, `StoreCompiler`, the `Typed*` records, `type/`.
- `efcensus/callers.txt`, `efcensus/scripts/callers.py`: the class-level caller matrix (a name match, pruned in the
  parts).
- `efcensus/tree/`: the extracted tree at `7c438f7b3`.
