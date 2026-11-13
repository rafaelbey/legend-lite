# H1 — `normalizer/MappingNormalizer.java` (2,833 lines, commit 7c438f7b3)

Paths below are relative to TREE/core/src/main/java/com/legend unless stated. "MN:n" = MappingNormalizer.java line n.

**Class shape.** `public final class`, private ctor (MN:122-124), all-static, no static mutable state (the only static
field is the immutable `PRIMITIVE_TYPE_NAMES = Set.of(...)`, MN:2740, membership-only). Public surface: the two
`normalize` overloads MN:135, MN:142 — **test-only** (ModelBuilderTest.java:262, :290; MappingNormalizerTest.java:1605,
:2695, :2703); callers.txt `main-out[0]`. Production enters through the package-private 4-arg `normalize` (MN:152) from
`ModelNormalizer.normalize` (ModelNormalizer.java:128), itself reached from `Compiler.normalizeLayer`
(Compiler.java:280) → `compileModel`/`buildModule`/`compileErrors`/`buildModel`/boot. Everything below except the dead
collectors is on that path.

**Who calls back into it (main, all in package `normalizer`, verified by qualified name):**
ModelNormalizer `normalize`(4-arg) :128 · MappingPrePass `withElement` :55 · AggregateViewLift `synthesizeClassMapping`
:36, `declaredPrimaryKeyColumns` :43, `relationalSourceOf` :46, `propertyPinsOf` :47 · AssociationSynthesis
`synthesizeModelJoinMapping` :400, `synthesizeXStoreMapping` :405, `nameRefOrNull` :344/347/360/363,
`containsTargetColumnRef` :604/674, `resolveViewRefsInJoin` :648, `determineTargetTable` :654, `requireNonViewTarget` :662,
`mainTableOrNull` :705, `inferMainTableQuiet` :783 · GroupBySynthesis `seedAliasScope` :138 · ImplicitInheritance
`inferMainTableQuiet` :59/213 · JoinChainEmission `hasMainTable` :291, `inferMainTableQuiet` :520, `determineTargetTable`
:563/832, `relationFunctionPipeline` :566, `resolveViewRefsInJoin` :830/901, `mainTableOf` :875, `seedAliasScope` :968 ·
RelOpTranslator `canonicalTable` :226 · UnionSynthesis `synthRelational` :544, `inferMainTableQuiet` :566/568,
`determineTargetTable` :861, `canonicalTable` :912/964/993/994 · ViewRelation `mainTableDefOf` :50, `canonicalTable`
:60/329/350/383, `seedAliasScope` :174 · XStorePureEnds `relationFunctionPipeline` :95, `canonicalizeEqualOperands`
:222/225. (lineage/ScanRelations' `mainTableOf` is its own method, not a caller.) Tests: MappingNormalizerTest also calls
`inferMainTable`, `determineTargetTable` (same package).

**Edges out (PHASE3_CRATES).** `compiler.spec.Calls` (lite-spec; MN:3; `Calls.platform` at 818, 1148, 1151, 1197,
1555, 1561, 1566, 1638, 1686, 1794, 1796, 1813, 2255, 2294, 2453, 2456, 2466, 2706, 2773; `Calls.exact` at 1028, 1103,
1541, 2132, 2786). Compiler root (lite-names): `ModelBuilder` throughout (`findFunction`, `findFunctionById`,
`findAssociation`, `findView`, `findFilter`, `findEnum`, `viewMainTable`, `knowledge().hierarchyClass/propertyType/
propertyDef/isSubtype`), `SynthFqn` 486, 522, 788, 790, 1038, 1113, `ResolvedNames` 722, 733, 734, 2781, 2811,
`KnowledgeLayer.canonicalTable` 1437. `builtin.Pure`/`Pure.Lite`, `builtin.NativeFn.RowGetter.GET` 2771,
`platform.CoreFn.NAVIGATE` 2781. **No `compiler.element.*` edge.** All within core-layers.txt:90's `normalizer` line.

**Deferred model methods (lite-model lib.rs:20) called here.** `ClassMapping.Relational.aggregationAwareMain()` MN:1382;
`RelationFunction.Col.isEmbedded()` MN:864; `RelationFunction.Col.bindSrc(...)` MN:888; `MappingDefinition` 6-arg ctor
MN:448 (→ `ClassBindings.of` → the `position` IdentityHashMap, MappingDefinition.java:174) and 5-arg ctor MN:532
(→ `NormalizationFacts.NONE`, empty `resolvedStores`); `ClassBinding.Relational` MN:322, 388, 505; `.Operation` MN:332,
397; `.Pure` MN:340, 404, 513; `DeclaredKeys.NONE` MN:328, 393, 508; `AssociationBinding` MN:438, 524, 527;
`RelationalSource.Table` MN:599, 1381, `.Json` MN:1363; `EnumColumn` MN:1377; `NormalizationFacts` via `ledger.facts(...)`
MN:455 (built at MappingLedger.java:86); `SetId.of` MN:2387. Not model methods but read: `ResolvedMapping.
enumerationMappingsWithIncludes` MN:453, 2381, `.withMapping` MN:267, `.closure/.relationalSets/.set/.invalidReason/
.views`. **`withClassMappings` is not called in this file** (it is reached via `AssociationSynthesis.
injectMultiHopAssociationPMs` at MN:267 → AssociationSynthesis.java:219).

**Step-0 identity rows this file feeds.** None is in this file. It constructs `MappingDefinition` (MN:448, 532) → row
`MappingDefinition.position` 23,989/0 (0 misses: the oracle never exercises an absent key). It calls `md.views().body(view)`
(MN:1616, view instance from `model.findView` MN:1327) → rows `LiftedViews.functions` 393/112, `.walls` 0/112,
`.expanding` 0/112 (0 hits: a view wall or view-expansion cycle reached from a mapping is invisible to the oracle). It runs
`MappingPrePass.run` (MN:170, row `selfEdge` 11/0) and `AssociationSynthesis` (MN:267/418, row `self` 68/66). Local
identity: `thatRow == srcRow` MN:1003 (two local `Variable`s with distinct names: accidental, structural is equal).

**Hash collections (TRIAGE C1).** MN:273 `HashMap mappingsPerClass` — `merge`/`get` only, not iterated: safe. MN:279
`HashSet unionRooted` — `contains` only: safe. MN:280 `HashSet mixedUnionRooted` — written MN:287, **never read: dead**.
MN:496 `HashSet seen` (clean-sheet chain cycle guard) — `add` only: safe. MN:2614 `HashSet seen` (view-chain walk) — `add`
only: safe. `Set.of` MN:2740 — `contains` only (Set.of iteration is per-JVM salted; never iterated): safe. Pipeline's
`classSlots`/`ambiguousTables`/`droppedRoutedProps` are `HashSet`s (Pipeline.java:20, 51, 62) used here membership-only
(MN:2482, 2486, 2494, 1831). Every **iterated** collection here is ordered: `aliasToTargetTable` (LinkedHashMap,
Pipeline.java:19) iterated MN:1810, 2481, 2485 → ColSpec order / scope binding → SQL; `LinkedHashSet` MN:1402, 1413, 1786,
2654 (MN:1786 → `select`/`distinct` column order → SQL; the others print into error texts as Java `Set.toString`
`[a, b]`). `Map.of` MN:1088-1089 is only `.get` (RelationReads/ModelJoinNesting: no iteration found) — safe, but `Map.of`
throws a raw `IllegalArgumentException` if the ModelJoin lambda's two params share a name (pairEndVars returns
`p0.name()`/`p1.name()`, ModelJoinNesting.java:150-162) — unverified whether the parser admits that.

**MissProbe funnel: 9 sites (= FallbackLedgerTest.java:61 pin).** `knownMiss`: MN:1158 (M2M target class absent → `tgt`
null → raw expressions, MN:1205), MN:2078 (owner absent → immediately loud ModelException MN:2079: equivalent to
`orElseThrow`), MN:2178 (inline owner absent → subtype check skipped), MN:2747 (class absent → no `trustOne`/`to`/booleanize
coercion at all). `miss`: MN:1328 (not a view → table-backed: the miss is the answer), MN:2431 (enum absent → value
validation skipped), MN:2536 (not a view → column ref kept), MN:2620 (not a view → chain stops), MN:2781 (call has no
resolved form → not navigate-exempt → may get `trustOne`). In Rust these are plain `Option`s; the per-site None behaviour is
the spec.

## Family table

| # | family (methods) | lines | reached by (caller methods) | reaches an answer? | idioms needing a Rust choice | candidate |
|---|---|---|---|---|---|---|
| 1 | Entry + per-mapping wall: `normalize`×3, `withElement` | 122-256 (135) | 4-arg: ModelNormalizer.normalize :128 (prod). 2/3-arg public: tests only. `withElement`: own loop MN:186/212 + MappingPrePass.run :55 | Yes: the `NormalizedModel` element list (legacy+clean-sheet mappings in element order, then every lifted fn, MN:228), `legacySurfaces`; a tolerant wall = `wallSink.putIfAbsent(e.element(), firstLine(msg))` MN:202-203, 218-219 → `buildModule`'s walls; strict = the ledger's FIRST `strictErrors` thrown MN:193-194 → `compileErrors` (strict: `compileModel`, Compiler.java:904-921) answers with that exception's FULL `getMessage()` | Exceptions as control flow: catches MN:198, 214 (wall text **embeds the exception message**, first line via Java `split("\n")[0]` — note `"\n".split` is empty in Java → AIOOBE; Rust `split().next()` differs on that edge); `withElement` MN:241-254 attaches the mapping FQN (message unchanged, cause dropped) and **converts `NotImplementedException` into `ModelException(NORMALIZE)`** (class changes). Its `MappingResolutionException` arm MN:247 is **dead**: no thrower in any layer E may reach (grep of base/builtin/compiler/model/platform/protocol/normalizer: none). `ModelException.getMessage()` is undecorated (LegendCompileException.java:38-42): no double prefix | PORT 4-arg + `withElement` semantics. DON'T PORT the public 2/3-arg overloads (test convenience; they rebuild `LiftedViews` a second time, MN:144-145 vs ModelNormalizer.java:126-127) or the MRE arm |
| 2 | Legacy mapping → binding table: `normalizeMapping`, `isOperation`, stamp helpers `propertyPinsOf`, `declaredPrimaryKeyColumns` | 258-456 (199), 763-767, 1909-1956 | normalize MN:187; stamps also AggregateViewLift :43/:47 | Yes: lifted class/set/assoc fns (bodies → G → H/I → SQL), `ClassBinding`s (order feeds `MappingDefinition.position`; read by resolver), `AssociationBinding`s, flattened enum mappings, `resolvedStores`, `NormalizationFacts` (poisons → `PureModelContext.mappingPoison` :518-536 → `ClassSources.java:714-717` MappingResolutionException text "(…reason…)"; **frozen plan oracle carries one: "… is not mapped in mapping '…crossMapping5' (M2M explosion 'tradeId*' is a roadmap feature …)"**, i.e. MN:1176's text) | Three fault-isolation catches (control flow): per-SET MN:345 (ModelException → `strictErrors`; poison `ForSet` with full message), per-CLASS MN:364 (ModelException → `strictErrors`; poison `ForClass` full message; NIE poisons only — "a ROADMAP gap defers in both", MN:366-369), per-ASSOCIATION MN:419 (**both** NIE and ModelException → `strictErrors`, MN:428; in strict mode the NIE is rethrown MN:194 and rewrapped by `withElement` as ModelException). Poison text = `String.valueOf(e.getMessage())` verbatim. Hash: see header (safe; `mixedUnionRooted` dead). Defaults: `getOrDefault` MN:188, 327, 392 (empty = none: semantic). MN:338 and MN:402 (`operationMembers`, default `List.of()`) are the ones that change a route: an Operation binding with no recorded members takes ClassSources' plain path instead of the stack builder (ClassSources.java:725-727); `.orElse(am.associationName())` MN:432, 441 — a name-resolution fallback (FallbackLedgerTest.java:40-45 calls these "Phase-D debt … FIXLIST P7-3") | PORT line for line (catch placement, order, which kind goes to `strictErrors`). Drop `mixedUnionRooted` and the bare block `{…}` MN:300-353 (RE-EXPRESS: no answer reads them) |
| 3 | Clean-sheet door: `cleanSheetToCanonical`, `refBody`, `inlineRootSource`, `userFunctionBody`, `liftClassInline`, `liftAssocInline` | 458-687 (230) | normalize MN:213 (prod: every `CleanSheetMappingDefinition`) | Yes: lifted inline fns (`(): Class[*]`, assoc `(A,B):Boolean[1]`), the binding table with stamped `RelationalSource.Table`; ModelException texts MN:548-551, 580-583, 602-607, 656-670 → walls | `HashSet seen` MN:496 (cycle guard, membership). Uses the 5-arg `MappingDefinition` (MN:532): no facts, no `resolvedStores`, `md.enumerationMappings()` **not** flattened with includes (legacy path flattens, MN:453) — asymmetry | PORT |
| 4 | XStore direction canonicalization: `canonicalizeEqualOperands`, `rootedAt` | 711-761 (51) | synthesizeXStoreMapping MN:1016/1018; XStorePureEnds :222/:225 | Only the verdict "directions agree?" (the canonical form is never emitted, MN:1011-1015); a disagreement = the PARK-1 NIE text (MN:1019-1023) → poison/strict error | **AND/OR operands ordered by Java record `toString()`** MN:736 — compare-only. Any total order consistent with equality gives the same verdict (canon(A op B)=canon(B op A) iff ties ⇒ equal; Java record toString includes the record name and every component, so equal-text ⇒ equal value in practice) | RE-EXPRESS: a derived structural `Ord` (or the codec's canonical text) instead of `toString`; same verdicts. Unify with XStorePureEnds' twin (PARK-1 says "implemented TWICE verbatim", PARKED_WORK_LEDGER.md:41-42) — but the anchor test pins 2 files (ParkedWorkLedgerTest.java:47-49): ASK before merging |
| 5 | Class-synthesis dispatch: `synthesizeClassMapping` | 769-798 (30) | normalizeMapping MN:319, 363; AggregateViewLift :36 | Yes: the lifted `FunctionDefinition` (name from `SynthFqn.mappingClass`/`mappingClassSet`, `Synthesized(CLASS,…)` hat) | none (exhaustive switch over sealed `ClassMapping`) | PORT |
| 6 | Relation(`~func`) sets: `synthRelationFunction`, `putRelationCols`, `relationFunctionPipeline` | 800-928 (129) | dispatch MN:784; `relationFunctionPipeline` also JoinChainEmission :566, XStorePureEnds :95 | Yes: `<fn body> -> map(row \| ^C(...))` → SQL; ModelException MN:849-877, NIE MN:918-925 texts (count in text) | Deferred model methods `Col.isEmbedded` MN:864, `Col.bindSrc` MN:888. Raw NPE paths MN:889-890, 907 (escape every catch: build fails). `findFunction` then `findFunctionById` MN:910-915 (two documented spellings, not a fallback) | PORT |
| 7 | XStore / ModelJoin association predicates (column space): `synthesizeXStoreMapping`, `synthesizeModelJoinMapping` | 930-1121 (192) | AssociationSynthesis.synthesizeAssociationMapping :400/:405 ← normalizeMapping MN:418 | Yes: `legacyAssocPredicate(a,b,srcRel,tgtRel,{s,t\|cond})` bodies → resolver → SQL; NIE/ModelException texts MN:972-995, 1019-1023 (PARK-1), 1065-1076 | PARK-1 wall MN:1019-1023 ("has direction-specific conditions"); set ids read from the **first** line only MN:949-954 (PARK-1's cost table). Raw `NoSuchElementException` MN:943, 1058: `findAssociation(name).orElseThrow()` escapes every catch. It is reachable only when NameResolver left the name unresolved (it resolves association-mapping names before E: NameResolver.java:1265-1294) and AssociationSynthesis.resolveAssociation's wildcard/same-package fallback (AssociationSynthesis.java:380-390) found it. That is an edge, not a routine path. `Map.of` MN:1088-1089 (see header). Identity MN:1003 (accidental) | PORT (PARK-1 behaviour is today's spec; the per-end redesign is the owner's parked item) |
| 8 | M2M (Pure) sets: `synthM2M`, `m2mPropertyValue` | 1124-1230 (107) | dispatch MN:780 | Yes: `getAll(Src)->filter?->map(src\|^T(...))`, `NewInstanceCast` route; ModelException texts MN:1141-1146, 1176-1190, 1215-1219 (MN:1176's text is in the frozen plan oracle) | `knownMiss` MN:1158. **Fallback-looking:** `m2mPropertyValue` returns `pb.expression()` verbatim on three misses (MN:1205 tgt null, 1211 not a NameRef, 1213 not a class); the `AllVersions` suffix strip MN:1207-1210 is an uncited milestoning name shim. No `toOne` wrap here by design (MN:2727-2729) | PORT (flag MN:1205-1213 and 1207-1210 for the owner) |
| 9 | Dead collectors: `collectColumnsOfTable`, `collectTargetColumns` | 1238-1294 (57) | **nothing**: only self-recursive references (grep of core main+test: MN only) | No | `default -> {}` arms | DON'T PORT (dead) |
| 10 | Relational dispatch + main-table inference: `synthRelational`, `resolvedMainTable`, `relationalSourceOf`, `inferMainTable`, `canonicalTable`, `inferMainTableQuiet`, `collectMainTables`, `collectExprTables` | 1296-1519 (224) | dispatch MN:781; `synthRelational` also UnionSynthesis :544; `relationalSourceOf` MN:329/394 + AggregateViewLift :46; `inferMainTableQuiet` 5 sibling classes (header); `canonicalTable` 3 sibling classes | Yes: chooses JSON / view / table path; `RelationalSource` stamp (`aggregationAwareMain` MN:1382, enum columns); error texts MN:1313-1317 (NIE), 1409-1411, 1416-1419 (Java Set.toString in text) | `orElseGet(MissProbe::miss)` MN:1328. **Exception as control flow:** `inferMainTableQuiet` MN:1441-1448 swallows `ModelException` → null (its javadoc: "as a PROBE: null on ambiguity instead of loud"). Raw ISE MN:1369 (documented unreachable). Exhaustive switches with deliberate empty arms (audit 15) | PORT; RE-EXPRESS the probe as `infer_main_table() -> Result<Option<TableRef>, ModelException>` with the quiet caller mapping `Err` → `None`. Answers stay identical: every `ModelException` the probe swallows maps to `None` (`KnowledgeLayer.canonicalTable` inside it is a pure prefix strip, KnowledgeLayer.java:346-348) |
| 11 | JSON-source sets: `synthJsonSourceMapping` | 1521-1569 (49) | synthRelational MN:1308 (sets cross-baked from a runtime's `JsonModelConnection` at MappingClosures.java:100-125) | Yes: `sourceUrl(url)->map(row\|^C(p=to(get($row.data,'p'),@T)…))` → SQL. "ours" per MN:1553-1554 | raw NPE MN:1542 (unreachable: guarded MN:1307) | PORT |
| 12 | View-backed sets: `synthViewBackedMapping` | 1571-1619 (49) | synthRelational MN:1330 | Yes: view frame (`LiftedViews.body`) as the pipeline source | Identity: `md.views().body(view)` MN:1616 keys `LiftedViews`' IdentityHashMaps by the index's view instance (step-0 rows above); `body` rethrows a stored wall instance (LiftedViews.java:81-84) → becomes this class's poison text | PORT |
| 13 | Table-backed pipeline: `RelationalParts`, `synthTableBackedMapping`, `synthTableBackedParts`, `collectMappedColumns`, `collectExprColumns` | 1623-1907 (285) | synthRelational MN:1332; synthViewBackedMapping MN:1618; self MN:1673 (INNER-filter recursion) | Yes: the main row pipeline: `tableReference` source, join hops (hand-off JoinChainEmission, MN:1667, 1702, 1711-1734, 1762), union routes (UnionSynthesis.classifyUnionRoutes MN:1689), `~filter`, `~groupBy` (GroupBySynthesis MN:1744-1765), `~distinct` (`select`+`distinct` / slot-carrying `distinct(~cols)`), the `^C(...)` terminal; NIE texts MN:1757-1760, 1818-1820 | Ordered iteration only (LinkedHashSet MN:1786; Linked `aliasToTargetTable` MN:1810) → column order in SQL. `findFilter(...).ifPresent` MN:1720 silently skips an unknown filter here (it is loud later, MN:2240-2244). `~primaryKey` deliberately a no-op MN:1769-1777. `default ->` MN:1905 is a generic child walk (fine) | PORT |
| 14 | Property mapping → ctor field; embedded / otherwise / inline: `CtorField`, `translatePmToField`, `materializeEmbedded`, `materializeOtherwiseEmbedded`, `materializeInlineEmbedded` | 1958-2207 (250) | synthTableBackedParts MN:1834; recursive (MN:2041, 2107, 2128, 2201) | Yes: each `^C(p = …)` binding (column reads, `DeclaredCoercions`, `RelOpTranslator.translate`, join slots, `otherwise(…)`, local-property `isLocal`) → SQL; ModelException/NIE texts MN:2080-2105, 2159-2198 | `knownMiss` MN:2078 (then loud), MN:2178 (silent skip of the subtype check). Cycle guard: `pipeline.inlineStack` (LinkedHashSet, Pipeline.java:37) add/remove in try/finally MN:2194-2206; its `toString` is printed in the cycle text MN:2198 (insertion order) | PORT |
| 15 | `~filter`: `applyFilter`, `applyDirectFilter`, `applyJoinMediatedFilter` | 2209-2296 (88) | synthTableBackedParts MN:1739 | Yes: `filter(row\|cond)` → SQL; ModelException texts MN:2241-2244, 2271-2274 | none beyond ordered maps | PORT |
| 16 | Closure main-table lookups: `hasMainTable`, `mainTableOrNull`, `mainTableDefOf`, `mainTableOf` | 2298-2353 (56) | JoinChainEmission :291, :875; AssociationSynthesis :705; ViewRelation :50; internal | Yes: the table an association predicate / navigation anchors on; ModelException MN:2343-2345 | **Fallback-looking:** `mainTableOrNull` uses `inferMainTableQuiet` (MN:2322): a ROOT set whose inference is ambiguous is skipped and the first non-root set's table returned (MN:2323-2333). The `model` params of `hasMainTable`/`mainTableDefOf`/`mainTableOf` are unused | PORT (flag MN:2322-2333 to the owner: root ambiguity silently yields a non-root table) |
| 17 | Enumeration decode: `translateEnumeratedColumn`, `translateEnumeratedSource` | 2355-2471 (117) | translatePmToField MN:1989, 2000, 2028; putRelationCols MN:894 | Yes: nested `if(equal(src,lit) or …, {EnumValue}, {…})` ending in `[]` → SQL CASE; ModelException texts MN:2402-2463 | `miss` MN:2431 (unknown enum → no value validation). Inline ISE "F7.8 … MappingNormalizer#8 … NEVER fired" MN:2395 (not a `MissProbe.neverFired`, so FallbackLedgerTest's floor does not count it) followed by a **dead** `owner == null ? null` MN:2396. Unreachable "Unhandled SourceValue" MN:2450-2451 (`SourceValue` is sealed, 3 variants: EnumerationMapping.java:72-75). Id lookup takes the **first** match across own+includes MN:2386-2389, while the anonymous path is loud on two (MN:2401-2407). Quirk: MN:2460 prints `enumMappingId`, which is `null` for an anonymous reference → text "EnumerationMapping 'null' value …" | PORT (keep the 'null' text, or ASK to fix it — it is an answer) |
| 18 | Alias scope, view refs in joins, join target: `seedAliasScope`, `resolveViewRefsInJoin`×2, `viewChainReaches`, `requireNonViewTarget`, `determineTargetTable`, `containsTargetColumnRef`, `nameRefOrNull` | 2473-2691 (219) + 700-702 | own pipeline (MN:1827, 2247, 2284); JoinChainEmission, AssociationSynthesis, UnionSynthesis, ViewRelation, GroupBySynthesis (header) | Yes: which row variable a table's bare columns read (first alias per table wins, ambiguous tables recorded), view-column substitution in join conditions, the join's target table; NIE texts MN:2557-2563, 2639-2643, 2664-2667 (LinkedHashSet printed), ModelException MN:2659, 2688 | `miss` MN:2536, 2620; `HashSet seen` MN:2614 (membership). `default -> op.mapChildren` MN:2599 (generic child walk, fine) | PORT |
| 19 | Instance construction: `buildNewInstance`, `simpleTypeName`, `PRIMITIVE_TYPE_NAMES`, `buildNewInstanceToOne`, `booleanizeCaseLiterals` | 2693-2833 (141) | synthM2M MN:1199; synthRelationFunction MN:820; putRelationCols MN:860, 882; JSON MN:1568; table MN:1640; embedded MN:2112 | Yes: `new(@C, ^C(...))` with `trustOne(...)` around `[1]` bindings (exempt: navigate/legacyNavigate/otherwise/new), `to(get, @Prim)` for untyped variant reads, `'true'/'false'` → Boolean literals → G → SQL | `knownMiss` MN:2747, `miss` MN:2781. Lite-own emissions with **no SEMANTICS_REGISTER row found** (grep "booleaniz/trust/toOne/to(get"): `booleanizeCaseLiterals` (javadoc: "engine keeps the strings in SQL" MN:2794-2795), `trustOne` wrapping, JSON `to(get)` fan-out; the `Pure.Lite` helpers are LITE_INVENTION_CENSUS.md:75 "INTERNAL DESUGAR IR" | PORT |
| — | Dangling text (no code): headers/javadocs MN:409-411 (truncated comment "ENGINE ROUTER PARITY … targets a"), 689-698, 704-705, 1232-1236, 1296-1298 (javadoc with no method), 1960-1970, 2298-2304, 2693-2700 (javadoc for a deleted translator), 2715-2734 (buildNewInstanceToOne's javadoc sits on `simpleTypeName`), 2828-2832 | ~70 | — | No | — | DON'T PORT |

## Notes (evidence)

**Entry / walls (rows 1-2).** Order of work in `normalize` (MN:152-230): `StoreSubstitutionRewrite.resolveAllStores`
MN:161-164, then `MappingPrePass.run` MN:170-171, then per element: legacy (skipped if the pre-pass walled it, MN:175-177),
clean-sheet, else passthrough; lifted fns appended last (MN:228). Strict policy MN:193-195: the first recorded strict error
of the mapping is thrown from inside `withElement`, so its class is `ModelException` (an association NIE is rewrapped,
MN:246-254) with the mapping FQN as element. Tolerant policy: the mapping is kept with its poisons; only an exception that
escapes the per-set/class/association catches walls (and **excludes**) the whole mapping. Such escapes include
`AggregateViewLift.lift` (MN:385 is outside the per-class try; AggregateViewLift.java:28-49 has no catch) and everything
before the loops (MN:267). **Surprising (caller must check):**
(a) `resolveAllStores` runs outside every catch (MN:161), and its "Duplicated mapping include" `ModelException`
(StoreSubstitutionRewrite.java:263-267, phase MODEL) is thrown nowhere else (grep), and `Compiler.buildModule`
(Compiler.java:532-539) has no try. So a tolerant module build **aborts** on a duplicated include instead of walling that
mapping. (b) When a mapping is walled after some of its functions were appended to the shared `lifted` list (clean-sheet
inline lift MN:487 then `inlineRootSource` throws MN:509-510; legacy class fn MN:382 then `AggregateViewLift.lift` throws), those
**orphan lifted functions stay** in the normalized model (MN:228). This happens only in tolerant builds (a strict build
throws). So an orphan is in `buildModule`'s context (Compiler.java:532-539), but not in `compileErrors`, which is strict
(Compiler.java:904-929 → `compileModel`). Whether anything runs eager G over a `buildModule` context and so surfaces an
orphan's body errors is unverified. PORT as-is or ASK.
(c) The comments disagree: MN:366-369 says a roadmap gap (NIE) "defers in both", but the association catch (MN:419-428)
puts NIE into `strictErrors` too, so a strict build fails on an association roadmap gap. Its comment (MN:425-427) says
"TOLERANT (module) builds only".
(d) In a strict build the per-class ModelException is recorded and the loop **continues** (MN:370-380). Later classes
still synthesize, and their exceptions can also be recorded. Only `strictErrors.get(0)` is thrown, in declaration order.
Port the order.

**Binding order (feeds `MappingDefinition.position`, step-0 row 23,989/0).** One loop in declaration order
(MN:293-408): a non-root set of a multi-set class binds through MN:321-344, every other set through MN:387-407, each right
after its function enters `lifted`. For an AggregationAware main set, the view sets' functions enter `lifted` **after**
the main function (MN:382, then 385). Their bindings enter `classBindings` **before** the main binding (MN:385, then
387). Both orders must be kept.

**Poison text reaches answers.** Three places produce the text. Per-class: `ledger.poison(ForClass, msg)` MN:379.
Per-set: MN:351, plus the pre-pass invalid reason MN:313-314 / MN:358. Per-association: MN:429-433. Every reason a
key gets is appended with "; " (MappingLedger.java:39-41). The text then flows `NormalizationFacts.poisons` →
`PureModelContext.mappingPoison/mappingSetPoison/mappingAssociationPoison` (PureModelContext.java:517-536) →
`ClassSources.java:710-717` / `AssociationJoins.java:1213`, as the parenthesised suffix of a
`MappingResolutionException`. Evidence: rust/oracles/plan (execution-plans + test-plans-host) holds the MN:1176
explosion text inside that suffix.

**Oracle coverage, plainly.** The file constructs about 50 throws with ~46 distinct texts (MN:245-2688), plus one
poison literal (MN:306-309). Grepping the frozen plan oracle for 23 of those texts found exactly one: MN:1176, via the
poison suffix. PARK-1's text, the main-table texts, the enum texts, the clean-sheet texts, the embedded/inline texts
and the join/view walls appear **0** times. `rust/oracles/model` is parse-only (B→ParsedModel, its MANIFEST), not E.
So I could see no frozen oracle for the wall texts this file feeds to `compileErrors`/`buildModule`. Any such text is
held only by Java's tests (MappingNormalizerTest etc.). The caller should confirm whether another oracle holds E's
walls.

**Guards over this file.** FallbackLedgerTest.java:61 pins 9 `MissProbe` sites (the count matches). ParkedWorkLedgerTest.java:45-49 pins PARK-1's
anchor text "has direction-specific conditions" to exactly `MappingNormalizer.java` and `XStorePureEnds.java`
(PARKED_WORK_LEDGER.md:20-54: "implemented TWICE verbatim … and neither copy has a test"; four engine fixtures cannot
normalize). LegacyReachbackCensusTest no longer registers this file (its 4 reaches moved to MappingClosures,
LegacyReachbackCensusTest.java:67-73). CodeShapeGuardrailTest names no row for it. Docs phrases in the file: "as a PROBE"
MN:1440; "roadmap feature" MN:306-309, 1177, 1187, 2562, 2641; "for now" MN:1022; "H3-pending wall downstream" MN:1784;
"await the H5 set-ID dispatch story" MN:270-271; "the deleted flattening fallback … ours alone and is gone" MN:1580-1581,
1612-1614; "~primaryKey … currently a no-op" MN:1775-1776; "this default NEVER fired on the corpus census" MN:2395.

**Memo caches / threads.** None in this file. All state is per call: `Pipeline`, `MappingLedger`, and local lists.
`catch (RuntimeException)`: none. Catches: MN:198, 214, 241, 246, 345, 364, 419, 1445; `finally` MN:2204.
