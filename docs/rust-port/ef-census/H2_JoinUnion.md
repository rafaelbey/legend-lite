# H2 — JoinChainEmission, UnionSynthesis, Pipeline, GroupBySynthesis, AggregateViewLift, SetKeyFacts, MappingLedger, MappingFacts (commit 7c438f7b3)

Paths relative to TREE/core/src/main/java/com/legend/normalizer unless stated. JCE = JoinChainEmission.java, US =
UnionSynthesis.java, PL = Pipeline.java, GBS = GroupBySynthesis.java, AVL = AggregateViewLift.java, SKF = SetKeyFacts.java,
ML = MappingLedger.java, MF = MappingFacts.java, MN = MappingNormalizer.java. Every line count is `wc -l`.

## Said once, true of every row below

**Reach.** All eight are package-private (`final class`, no `public`), so only `com.legend.normalizer` reaches them;
callers.txt's out-of-package "test" entries for Pipeline (StressTest*, EnumIntegrationTest, M2M2RTabularTest) are
word-match noise, MappingNormalizerTest:3907/:4964 mention "Pipeline" in a string/comment only. No test calls a method of
JCE, US, PL, GBS, AVL, SKF or MF directly; MappedInClosureTest:35 calls `MappingLedger.mappedInClosure` (same package).
FallbackLedgerTest, ShadowWalkerCensusTest, CodeShapeGuardrailTest, LegacyReachbackCensusTest read these files as TEXT.
Production entry: `ModelNormalizer.normalize` → `MappingNormalizer.normalize` (MN:152) → (a) `MappingPrePass.run`
(pre-pass, MN:170) and (b) per mapping `normalizeMapping` (MN:186 → MN:258) → `synthesizeClassMapping` (MN:768) →
`synthRelational`/`synthTableBackedParts` (MN:1650) or `UnionSynthesis.synthUnion`/`synthInheritance` (MN:782-783); and
(c) the E.5 view lift `LiftedViews.body` (LiftedViews:100) → `ViewRelation.viewRelationExpr` (ViewRelation:87). All
production; nothing here is test- or probe-only except the dead members named under US-7.

**Error text is answer text.** A `NotImplementedException`/`ModelException` thrown inside one class's synthesis is caught
at MN:345 (non-root set → `PoisonKey.ForSet`) or MN:364 (→ `PoisonKey.ForClass`) and stored verbatim,
`ledger.poison(key, String.valueOf(e.getMessage()))` (MN:350, :379) → `NormalizationFacts.poisons` →
`PureModelContext:520-535` → the resolver's "0-binder" error; a `ModelException` is also queued on
`ledger.strictErrors` (MN:348, :371) and thrown by a strict build (MN:192-195). A throw OUTSIDE those try blocks (the
pre-pass; `AggregateViewLift.lift` at MN:385, after the try of MN:362-380) goes through `withElement` (MN:238-256: a
`NotImplementedException` is re-wrapped as a `ModelException` with the element FQN) → `wallSink` (first line of the
message, MN:203; `compileErrors`/`buildModule`) or thrown (strict). An `IllegalStateException` (MissProbe.neverFired, the
"F7.8" texts at JCE:140, US:686, `Pipeline.ledger()` PL:91) is caught nowhere in E and escapes the build raw. So every
message string in these files, and the order of `poison` calls (ML:40 appends a second reason with `"; "`), is spec.

**No `catch (RuntimeException)`** in any of the eight. One `catch` in total (JCE:876), one `finally` (US:242).

**Identity.** No reference-equality site in any of the eight (the only `==`/`!=` on objects are enum constants: JCE:1017-1027,
GBS:243). Step 0 (PHASE3.md) has no row for them. They REACH step-0 rows: `p.views.body(..)`/`md.views().body(..)` at
JCE:408, :437, :476, :568 is `LiftedViews.body`, keyed by `ViewDefinition` instance in `IdentityHashMap`s (LiftedViews:50-57;
(b) `.functions` 393/112, `.walls` 0/112, `.expanding` 0/112 — the wall-replay and cycle paths have 0 hits, so the frozen oracle cannot catch a mistake there). The
instance passed is `model.findView(..).orElseThrow()`, the index's own record, so `same` on it is the Rust rule.
`US.collectInheritanceMembers`'s `LinkedHashSet<ClassMapping> chosen` (US:588) dedups by record `equals`; the members
come from `rootByClass` (one object per class), so structural equality equals identity there (accidental structural).

**Hash collections (TRIAGE C1).** Every non-Linked one in the eight, all membership-only — none iterated, none reaches an
answer:
| site | collection | use | verdict |
|---|---|---|---|
| PL:20 `classSlots` | HashSet | `add` JCE:434; `contains` MN:2482, :2486 | safe |
| PL:51 `ambiguousTables` | HashSet | `add` MN:2494; `contains` RelOpTranslator:105, :228 via `PipelineView.ambiguousTables()`; never printed (`ambiguousTableRef(table, column)` takes neither the set) | safe |
| PL:62 `droppedRoutedProps` | HashSet | `add` US:391; `contains` JCE:50, MN:1831 | safe |
| GBS:51 `claimed`, GBS:148 `claimedKeys` | HashSet<Integer> | `contains`/`add` | safe |
| US:485 `seenSets` | HashSet | extends-chain cycle guard | safe |
| US:657 `seen` | HashSet | BFS visited | safe |
| ML:74 `out` | HashSet copied into `LinkedHashSet` (ML:80) | the copy CARRIES HASH ORDER (a Linked wrapper over a hash iteration); read only by `isMapped` (`contains`, ML:68-70); MappedInClosureTest compares with `Set.equals` | safe; misleading idiom (RE-EXPRESS: a plain set) |
Every other map/set in the eight is a `LinkedHashMap`/`LinkedHashSet`/`List`. Iteration that DOES reach answers is over
insertion-ordered maps: `p.aliasToTargetTable` (iterated by `seedAliasScope` MN:2481/:2485 — the FIRST alias of a table
wins `scope.putIfAbsent`, MN:2497 — and by MN:1810), `routedByProp` (US:308), `byTable`/`sharedKeys` (US:918, :973), the
inheritance member list (see US-3), `propsByOwner` (MF:76), `p.inlineStack` (printed in MN:2198's error text).
Reached memo caches (not in these files, single-threaded — no `parallel`/executor anywhere in E): `ModelBuilder.legacySetsById`
(ModelBuilder:149, lazily built HashMap, lookup only; via US `pinOwners`), `ModelBuilder.directSubclasses` (ModelBuilder:689,
lazily built HashMap, lookup only; via `knowledge().subtree`), `LiftedViews.functions/walls` (above).

**NormalizationFacts (ML:85-89).** The canonical constructor (model/MappingDefinition.java:77-97) copies each map into a
`LinkedHashMap`. Inputs: `poisons` ML:35, `mixedUnions` ML:49, `unionKeyThreads` ML:55 (LinkedHashMap each), `unionMembers`
MF:32 and `routedTargetClasses` MF:64 + inner MF:80 (LinkedHashMap each). **No input map is hash-ordered.** Production
readers are point lookups only (`PureModelContext:488, :495, :504, :511, :520, :527, :535`). But the Rust record
(rust/lite-model/src/records.rs:2346-2354) holds each map as an ordered `Arc<[(K, V)]>` with DERIVED `PartialEq`, and the
lossless codec writes them in that order: if a normalized-model differential is ever run, insertion order (the order of
`poison`/`put` calls during synthesis, MF's surface order) is spec. Java's `Map.equals` is order-insensitive, so
MappedInClosureTest:131's `assertEquals(ma.facts(), mb.facts())` across element orders would be STRICTER in Rust.

**Names that reach a synthesized body, and SQL.** No static or global counter in any of the eight; every minted name is
a function of (property / join / column names, the main table's columns, the slots already in `p.aliasToTargetTable` at
that moment, an index). Pipeline frames are per call (MN:1685, JCE:928, ViewRelation:118); the ledger per mapping (MN:183).
| rule | site | reaches | oracle evidence (rust/oracles/plan texts in git) |
|---|---|---|---|
| class-nav slot = prop; `prop_nav` if prop `equalsIgnoreCase` a main-table column (schema-stripped name, JCE:616-627) or is `columns`/`rows` (JCE:629-630); then `_` appended while the alias is taken (JCE:637-639); memo `navSlotByProp` | JCE:610-642 | `legacyNavigate(~slot ..)`, `$row.slot` (MN:2013-2016), SQL column-alias prefix | `exchange_nav_name`, `system_nav_name`, `columns_nav_name`, `tables_columns_nav_name` in test-plans-host; the `_` loop: no text evidence (0 hits of `_nav_`) |
| physical slot = `"__"`-join of the prefix path; `base__2`, `__3`.. on a name clash (local `n` from 2) | JCE:654-664 | `joinSlot(~slot ..)`, `$row.slot` | no text evidence of a collision suffix |
| per-arm inbound mid = `nl__<prop>__inb__<join>` | US:875 | joinSlot inside a route's rows | 0 text hits of `nl__`/`__inb__` in the in-git texts (execution-plans, test-plans-host, test-plan-values); the database judge's texts are archived outside git, and plan-ids keys for this shape exist (`testUnionOfViewsWithFilterInQualifiedPropertyAndNonOverlappingJoinSequnece` ×4, `...SetsV4` ×12), so the frozen ids cover it IF the alias reaches the plan JSON |
| union key thread = `<col>_<memberOrdinal>` | US:967 | `NormalizationFacts.unionKeyThreads` → StackBuilder:912, ImportDataFlow:51, CastReRoot:88 → SQL | `AS ID_0`, `AS ID_1` (hundreds) |
| shared table key = `<col>__pk_<table with [^A-Za-z0-9_]→_>` | US:1016-1018 | same | `id__pk_metamodel_value_specifications`, `id__pk_metamodel_plan_nodes`, `id__pk_metamodel_activities` |
| groupBy key = claiming PM's property name, else `k<i>__<col>` / `k<i>` | GBS:47-65, :141-188 | `groupByComputedKeys` ColSpecs → SQL | `k0__PRODUCT_ID`, `k1__PRODUCT_ID` |
| member function FQN = `SynthFqn.mappingClass` if `root() \|\| setsOfClass <= 1` in the DEFINING mapping, else `mappingClassSet` | US:75-83 | the stack's and each route's call (US:148, JCE:582) → the resolver binds it | (decides which function; no text) |
Answer-bearing sequencing: `rcm.propertyMappings()` order; Pass 1 (structural, MN:1697-1704; grouped Join PMs deferred) →
Pass 2 (nested JoinNavs: PM bodies, groupBy keys, Direct filter, MN:1705-1728) → Pass 3 (join-mediated filter chain,
MN:1729-1735) → the deferred grouped navigates (MN:1745-1766); `mintNavSlotAlias`/`uniqueSlotName` read the map as of
their call, so an earlier hop can push a later one's name. `p.unionRoutes.remove(prop)` at JCE:299 mutates the frame
mid-emission (later `perArmHops`/routed emission for that prop see no routes). The comments PL:52-57 and JCE:412-416
("each target-side member-suffixed (`FirmID_1`)") are STALE: `routeList` suffixes nothing and the route ordinal is read
only for its sign (US-2); the `_N` names lite emits come from key threads (US:967).

**Edges (PHASE3_CRATES §2; all within core-layers' `normalizer` line).** `compiler.spec.Calls` (lite-spec, but itself
only NameResolver + protocol): JCE:398, 409, 426, 431, 438, 445, 477, 544, 569, 582, 585, 972, 999; US:148, 149;
GBS:213, 222. `Calls.platform` calls `NameResolver.builtCallNames` (Calls.java:33-36), so E stamps lite-names' bare-name
referents into every synthesized `getAll`/`tableReference`/`filter`/`project`/`concatenate`/dynafn call — a lite-names
edge at E time. `compiler.SynthFqn` (lite-names): US:8, :80, :81 only. `compiler.element.*`: JCE:630
`compiler.element.type.PlatformTypes.ROWS_MARKER` (lite-types) only. `ModelBuilder` + `knowledge()` (lite-names): JCE, US,
AVL. builtin: `Pure.Lite.LEGACY_NAVIGATE/JOIN_SLOT/ROUTE` (JCE), `GROUP_BY_COMPUTED_KEYS` (GBS:222), `DynaFn`
(JCE:1015-1027, GBS:242-243), `Pure.nativeFunctionsAt` (GBS:247), `Pure.allNativeClasses` (US:644).

**Model methods slice 1b deferred (rust/lite-model/src/lib.rs:20), called here.** `ClassMapping.memberWitness()` US:701
(dead field); `MappingDefinition.NormalizationFacts` copying ctor ML:86; `ClassBinding.Relational` ctor (copies
`propertyPins`) AVL:39; `ClassBinding.AggregateViewFacts` ctor AVL:60; `ClassBinding.DeclaredKeys` ctor SKF:34 and
`DeclaredKeys.NONE` AVL:45; `KeyThread` 4-arg secondary ctor US:967 and canonical US:979; `ClassMapping.Relational` 13-arg
rebuilds US:506, :567; `PropertyMapping.Join` 4-arg US:211; `RelationalOperation.ColumnRef`/`JoinNavigation` GBS:273, :279;
`PoisonKey.ForClass` US:392; `SetId.of` SKF:28 (made in slice 1b). Protocol (lite-front) helpers: `AppliedFunction.withParameters`
GBS:102, :122 (US:747 dead); `ValueSpecification.mapChildren` GBS:127. NOT called from these eight:
`LegacyMappingDefinition.withClassMappings`, `RuntimeDefinition` helpers, `RelationFunction.Col.bindSrc`,
`isEmbedded`/`aggregationAwareMain`, `MappingDefinition.position`/`ClassBindings.of` (reached when MN builds the record).

**MissProbe funnel (FallbackLedgerTest:59, :66 pin 4 each).** JCE: :100 (Embedded block's owner miss → its sub-PMs are not
hoisted), :695 and :716 (owner miss → "not class-typed"), :978 (main relation not a view → the table branch). US: :277
(owner miss → no descent), :311 (owner miss → no union target: every route root/pinned), :773 (no physical table → no
shared key), :1001 (view-backed member → no key columns). Each is `Optional → null → a branch`: Rust `Option` with the same
branch; the funnel is a Java source-text count with no Rust analogue unless lite-guards adds one. Loud never-fired:
JCE#1 :72, JCE#3 :1057 (`neverFired`), JCE#2 :140 and US#3 :686 (inline ISE text); each followed by a DEAD null check
(JCE:73, :141, :1058; US:687).

## JoinChainEmission.java (1,065 lines) — six families

| family | lines | reached by (caller methods) | reaches an answer | idioms | recommendation |
|---|---|---|---|---|---|
| JCE-1 Pass-1 structural hoisting: `emitHopsForStructuralPm`, `emitOtherwiseEmbeddedHop`, `recordNavSlotOwner` | 45-217, 591-601 (184) | MN `synthTableBackedParts` :1702 (Pass 1), :1762 (deferred grouped Join PMs); self-recursive through LocalProperty/Embedded/Otherwise/Inline | which hops hoist, in PM order → slot names (table above) → body → SQL; collision/unsupported texts (:83-88, :111-117, :156-160, :185-188, :197-199, :203-205) → poison/wall | `default ->` over sealed `PropertyMapping` (:168); asymmetric owner-miss handling: Embedded `knownMiss` silently skips (:100), Otherwise `neverFired` ISE (:72), Inline raw ISE (:140); dead `== null` at :73, :141; Inline recursion has no cycle guard of its own — an Inline cycle is walled first by the pre-pass (`collectRoutedJoins`' guard US:223 via ImplicitInheritance:164, MappingPrePass:112), and per class again by `classifyUnionRoutes` (MN:1689, before :1702); JCE-5's Inline arm (:758-771) leans on the same guards | PORT (drop the dead null arms: `orElseThrow` makes them unreachable) |
| JCE-2 chain emission: `emitJoinChain` ×2, `perArmHops`, `relationRef`, `routeList`, `HopTarget`/`hopTarget`, `plainClassViewCond`, `targetColumnNames` | 219-455, 471-589, 810-903, 1045-1062 (468) | JCE-1; MN `synthTableBackedParts` :1724 (Pass 2), :1731 (Pass 3); ViewRelation `viewRelationExpr` :124, :157, :166 (view pipeline, `md` may be null); JCE `innerFilteredSource` :929 | the synthesized body itself: `joinSlot(..)`/`legacyNavigate(..)` steps with their `{s,t\|cond}` lambdas, the routed `route(<memberFn>(), rows, cond)` list, view targets as the view's relation; → G types it → H/I → SQL. "Join 'X' not found in db ..." texts (:356-359, :532-535) | the catch at :874-878 (see note); `p.unionRoutes.remove` mid-emission (:299); dedup keyed on `List<String>` path (structural, fine); reaches LiftedViews identity memo (:408, :437, :476, :568); `MilestoningFacts.isTemporal` (:898, its sole caller); two `orElseThrow()` with no message (:408, :437, :856) | PORT line for line; RE-EXPRESS the :876 catch as `mainTableOrNull(md, target).is_none() → None` (same answers, see note) |
| JCE-3 slot naming and lookup: `navSlotFor`, `mintNavSlotAlias`, `uniqueSlotName`, `joinPath`, `slotFor` | 457-469, 603-690 (98) | JCE-2 (:320, :351); MN `translatePmToField` :2013-2019; MN `applyJoinMediatedFilter` :2280; JCE `innerFilteredSource` :955; `Pipeline.view().slotFor` (PL:106) → RelOpTranslator :630 | slot names (table above); misses are loud `NotImplementedException` texts (:465-466, :685-687) | `String.equalsIgnoreCase` (:627) — Java's per-char upper/lower fold, not ASCII-only; schema strip `lastIndexOf('.')` (:616-617); memo `navSlotByProp` (first mint wins per frame) | PORT |
| JCE-4 class-typed target predicates: `classTypedTargetIfMapped`, `classTypedTarget`, `hasMappedSubclass` | 692-730 (39) | JCE-1 guards (:79, :107, :152), `recordNavSlotOwner` :598, JCE-2 :286; MN `translatePmToField` :2010; MN `materializeEmbedded` :2099 | navigate-vs-physical decision → body shape → SQL; ledger `isMapped` (closure-local) | `hasMappedSubclass` is a ShadowWalkerCensusTest-pinned "SUBTYPE" shadow walker (ShadowWalkerCensusTest:49); `anyMatch` over `subtree` (order-free boolean); 2 funnel sites | PORT |
| JCE-5 JoinNav collectors: `JoinNavSpec`, `collectJoinNavigationsInPms` ×2, `collectJoinNavigations` | 732-809 (78) | MN `synthTableBackedParts` :1711, :1713, :1721; ViewRelation `viewRelationExpr` :122, :154, :164; JCE `plainClassViewCond` :867 | collection order = Pass-2 emission order = slot names | exhaustive sealed switches, no default (good); Inline lookup (:758-771) is the 3rd copy of "first Relational in `md.classMappings()` with `idOf == id`" (also JCE:132-139, US:231-240) | PORT (RE-EXPRESS the three lookups as one helper: same first-match/break semantics) |
| JCE-6 (INNER) mapping-filter source: `innerFilteredSource`, `nullTolerant` | 905-1043 (139) | MN `synthTableBackedParts` :1667 (filter `JoinMediated` with an explicit join type), which re-enters itself with the filter removed | `project(filter(<main + chain>, cond), [every base column])` → SQL (engine `getRelationalElementWithInnerJoin`; oracle keys testInnerJoinClassMappingFilter* ×4 variants); loud texts :949-953, :1028-1031, :985-988, :937-941 | `nullTolerant`'s `default -> false` (:1041): a new/unlisted op (JoinNavigation, ArrayLiteral, Lambda, Literal) is silently classified null-REJECTING — the verdict that licenses LEFT+WHERE ≡ INNER; comment :942-947 "Every corpus (INNER) condition today is null-rejecting ... Real INNER emission is follow-up" (a known gap); comment :921-925 "NOTE (leg 4 ...) the next rung must gate that threading" (known gap); funnel :978 | PORT; ASK whether `nullTolerant`'s default should be enumerated (invariant 4) — changing it changes which mappings wall |

**Notes, JCE.**
- *The catch at JCE:876.* `MappingNormalizer.mainTableOf` (MN:2349) = `mainTableDefOf(..).table()`; `mainTableDefOf`
  (MN:2337-2346) throws exactly one `ModelException`, "No ~mainTable for class '..' in mapping=.. (required to synthesize
  AssociationMapping)", when `mainTableOrNull` (MN:2317-2334) is null — no Relational set of the target class in the
  closure has a declared or inferred main table. The catch returns `null` from `plainClassViewCond`: the caller keeps the
  VIEW EXPANSION (`viewTarget` stays set; the navigate's target rows are `p.views.body(view)`, JCE:407-411) instead of the
  physical-column condition. The message is discarded (not answer text); the BRANCH reaches SQL. Two layers of
  exception-as-boolean: `mainTableOrNull` itself calls `inferMainTableQuiet` (MN:1441-1448, `catch (ModelException) →
  null`). RE-EXPRESS: test `mainTableOrNull(md, targetClassFqn) == null` — identical outcomes because that is the throw's
  only condition and nothing else under `mainTableOf` throws `ModelException`.
- *`MilestoningFacts` (MilestoningFacts.java, 30 lines).* Sole caller JCE:898 (grep over core/src: no other). `isTemporal`
  walks `knowledge().lineage(cls)` stereotypes through `MilestoningStrategy.ofStereotypeOrNull`; a temporal target keeps
  the view expansion (one of the four "sweep-proven" gates at JCE:887-899, all answer-bearing). PORT with JCE-2.
- *Routed set retarget (JCE:289-300):* a single route entry to a strict subclass of a target class with no main table
  retargets the navigate and drops the union route — engine "the one routed set IS the rows" (javadoc JCE:260-267).
- *`routeList` (JCE:495-589):* `pin == null` cannot reach it (US-2 drops any property with an unresolved pin), so its
  "not Relational" throw (:515) fires only for a non-Relational, non-~func set. Per-arm branch (`inArm`, :538-555): no text
  evidence in git (0 `nl__` hits), keys exist (see the names table).
- Surprising: JCE:616-625 strips the schema off a `SCHEMA.TABLE` main table and asks `knowledge().table(mainDb, TABLE)`, so
  the `_nav` collision test of a schema-qualified main table consults the default-schema table of that name (or none).
- Oracle coverage present: Otherwise-embedded (plan-ids keys `otherwiseTestComplexExpression`, `CB_Otherwise_suite`),
  union chained joins (`testUnionWithChainedJoinsAcross4SetsV5`, `...SetsV4`), INNER filters (above). No TEXT evidence in
  git (the ids may still cover them; the caller can confirm against the archived database-judge texts): per-arm aliases
  (keys exist, above), the `_nav_` loop, the `__<n>` collision suffix. 0 hits (truly uncovered): LiftedViews wall replay
  and cycle (step 0), the F7.8 ISEs (never fired on the census by their own text).

## UnionSynthesis.java (1,020 lines) — seven families

| family | lines | reached by (caller methods) | reaches an answer | idioms | recommendation |
|---|---|---|---|---|---|
| US-1 pins and member-function names: `UnionRoute`, `PINNED_SINGLE`, `memberFunction` ×2, `PinTarget`, `resolvePin`, `pinOwners` | 44-136 (93) | JCE `routeList` :506, :583; US `stackBody` :148; US `classifyUnionRoutes` :338, :341 | the FQN each route/stack arm calls (SynthFqn, table above) → the resolver binds it → SQL; poison text naming ambiguous pin owners (:343-346) | the root-or-sole rule (:79) is a 2nd spelling of `synthesizeClassMapping`'s naming (MN:294 `mappingsPerClass > 1 && !root` over the RESOLVED md → `setDiscriminated` → MN:787-790) — must agree or a call names a function that does not exist; fallback-shaped `closure().get(0)` when no closure record matches by class+id (:71, :120) — plausibly the pre-pass's implicit sets, which no raw record lists; reaches `ModelBuilder.legacySetsById` memo | PORT both naming rules line for line; ASK: the "Declared divergence" at :101-102 (a pin the queried mapping cannot see still navigates "where the engine fails") has no `SEMANTICS_REGISTER` row (grep: none) — per AGENTS.md a divergence that is not a row is a defect |
| US-2 route classification: `pmIdentity`, `collectRoutedJoins` ×2, `recordOwner`, `embeddedOwner`, `classifyUnionRoutes` | 157-402 (246) | MN `synthTableBackedParts` :1689; ImplicitInheritance `implicitOpsForRoutedTargets` :164 (pre-pass, MappingPrePass:112); MappingPrePass `resolveExtends` :194, :197 (`pmIdentity`) | `p.unionRoutes` (→ JCE-2's routed navigate), `p.droppedRoutedProps` + poison "property 'p' routes to <reason>; the property is dropped from this synthesis" (:392-394, keyed `ForClass`); extends-merge identity `name + ' ' + targetSetId`; ModelException texts: "Cycle materializing Inline embedded set .. the splice chain [a, b] returns to it" (:224-228, prints the LinkedHashSet), "routed under two owners" (:267-271) | `UnionRoute.targetOrdinal` is read only for its sign (`== -1` :397, `>= 0` :826; nowhere else in core); `getOrDefault(prop, rcm.className())` (:311) default unreachable (`recordOwner` keys every prop); `default ->` (:249); the throws of `collectRoutedJoins` fire FIRST in the pre-pass (ImplicitInheritance:164 walks every Relational set) → whole-mapping wall, so the same throws inside `classifyUnionRoutes` are shadowed for those inputs; ImplicitInheritance:160-172 duplicates :304-315's owner/target prelude | PORT; RE-EXPRESS the ordinal as a 3-variant tag {Member, Root, PinnedSingle} (no reader reads the number) and `getOrDefault` as `get` (default never taken) |
| US-3 operation synthesis: `stackBody`, `synthUnion`, `synthInheritance`, `inheritanceMembers`, `collectInheritanceMembers` | 138-155, 404-555, 579-697 (323) | MN `synthesizeClassMapping` :782, :783; US `classifyUnionRoutes` :326 (`inheritanceMembers`) | the operation's body `m1() -> concatenate(m2()) ..` (member order = SQL UNION order and the `_N` key ordinals); `ledger.operationMembers` → `ClassBinding.Operation.memberSetIds` (MN:338, :402, `getOrDefault(.., List.of())`); `ledger.mixedUnions` → `NormalizationFacts` → PureModelContext:504 → resolver arms; many loud texts (:417-419, :441-445, :449-453, :459-463, :534-537, :604-615) | write-then-throw: `mixedUnions.put` (:440) then `NotImplementedException` (:441) — the fact must survive the error (control flow: "the throw lands on the poison ledger; the resolver route recognizes the registry before the ledger surfaces"); size-1 inheritance recurses into `MappingNormalizer.synthRelational` (:544); member order = `model.classes()` ingest order ∩ `subtree`, then `Pure.allNativeClasses()` (:637-648), leaves, per-leaf BFS up `superClasses()` order (:654-696); `collectInheritanceMembers` is a ShadowWalkerCensusTest-pinned walker (2 call sites, ShadowWalkerCensusTest:47); `bySetId` built twice identically (:425-429, :595-599); raw ISE (:686) + dead `cd != null` (:687) | PORT |
| US-4 key threads: `recordKeyThreadsOf`, `TABLE_KEY_SUFFIX`, `tableKey`, `ownSharedKeys`, `recordKeyThreads`, `memberPrimaryKey`, `sharedKeyName` | 557-577, 755-787, 894-924, 939-1018 (~150) | US `synthUnion` :513, `synthInheritance` :553 | `ledger.unionKeyThreads` → `NormalizationFacts` → StackBuilder:912, ImportDataFlow:51, CastReRoot:88 → SQL column names (table above) | `sharedKeyName`'s `replaceAll("[^A-Za-z0-9_]", "_")` (:1017) is per CODE POINT (Java regex) — Rust `chars()` matches, a UTF-16-unit loop would not; `columnKind` lookups; a view-backed member yields NO key thread (:774, :1002 funnel) — quiet; no register row found | PORT; ASK (low): is a view-backed union member's missing key thread engine-exact (engine `resolvePrimaryKey`)? |
| US-5 per-arm chain helpers: `uniformChainedRoutes`, `memberJoins`, `inboundArmSteps`, `LiftMidStep` | 789-891 (~100) | JCE `perArmHops` :234-235, JCE `routeList` :500-501, :540 | shared-prefix vs per-arm decision (one predicate for emitter and key collector, "never allowed to drift" :388-389); per-arm mid joins inside the route's rows; alias `nl__..__inb__..`; "Join .. not found" text :857-860 | none special; javadoc :840 links a method `liftMidSteps` that no longer exists | PORT; text-blind in git (0 `nl__` hits) but plan-ids keys exist for the shape (`testUnionOfViewsWithFilterInQualifiedPropertyAndNonOverlappingJoinSequnece`, `...SetsV4`): the id check covers it if the alias reaches the plan JSON — confirm against the archived texts before writing new tests |
| US-6 `prePassedClosure` | 926-937 (12) | ML `mappedInClosure` :75 | the closure records whose classes are "mapped" (engine R1) | none | PORT (with ML) |
| US-7 dead members: `MEMBER_WITNESS`, `FILTER_FORM`, `record Thread`, `record ScanSource` (`of`, `slotName`, `merged`) | 701, 704-707, 709-710, 712-753 (~52) | nothing: grep over TREE (all .java, every module) finds no reference outside their own definitions; `ScanSource.of` is called only by `ScanSource.merged`, which nothing calls | nothing (`MEMBER_WITNESS` evaluates `ClassMapping.memberWitness()` at class init and is never read; the `$member` columns in the oracle's SQL come from the resolver: GraphEmission:3028, Substitution:2275, CorrelatedSubselects:2164) | — | DON'T PORT (replaced by nothing; `ClassMapping.memberWitness()` itself is still needed by the resolver slice) |

**Notes, US.** `memberFunction(md, member)` (:62-72) and `resolvePin` (:103-124) both scan `md.closure()` × each record's
`classMappings()` per call (no index); fine for answers. `classifyUnionRoutes`'s poison for an unresolved pin drops the
whole PROPERTY but is keyed `PoisonKey.ForClass(rcm.className())` (:392). `pinOwners` resolves model-wide through
`ModelBuilder.legacySetsWithId` (ModelBuilder:1305-1318), whose per-mapping lists keep ingest/own order (the ambiguous
poison text lists owners in that order).

## Single-row classes

| class | lines | reached by | reaches an answer | idioms | recommendation |
|---|---|---|---|---|---|
| Pipeline | 129 | ctor: MN `synthTableBackedParts` :1685, JCE `innerFilteredSource` :928; `forView`: ViewRelation `viewRelationExpr` :118; fields read/written by JCE, US :391/:400, MN (:1787-1810, :1831, :2013-2022, :2194-2205, :2285, :2478-2497), GBS `applyGroupBy` :138/:193/:210 via `seedAliasScope`/`view()` | the frame whose maps decide slot names and scope binding (above): `aliasToTargetTable` ITERATED (first alias per table binds); `inlineStack` printed in MN:2198's error | mutable frame (`expr` allowlisted: CodeShapeGuardrailTest:355 "normalizer emission frame: Pipeline IS the frame object"); 3 membership-only HashSets; `ledgerOrNull` + `ledger()` ISE (PL:89-95, unreachable: view pipelines never call it); `view()` builds an anonymous adapter per call (PL:98-123); `targetHasColumn` uses `String.equalsIgnoreCase` (PL:116) — Java's per-char fold, as at JCE:627 | RE-EXPRESS: a plain struct; insertion-ordered maps for `aliasToTargetTable`, `pathToSlot`, `navSlotByProp`, `navSlotOwner`, `unionRoutes`, `aliasToTargetColumns`, `inlineStack`; any set for the three HashSets; `Option<&MappingLedger>` (or a view/mapping frame split); `PipelineView` as a trait impl — cannot change an answer: the HashSets are only `add`/`contains`, the adapter's five methods are pure reads, and every iterated map keeps insertion order |
| GroupBySynthesis | 310 | MN `synthTableBackedParts` :1744 (`applyGroupBy`), :1751 (`groupedKeyColumnNames`), :1764 (`renameGroupedNavCond`); ViewRelation :190 (`isGroupReducer`), :204 (`groupByOpsMatch`) | `groupByComputedKeys(source, keys, aggs)` body → SQL (key names `k<i>__<col>`, aggregate reducer `Calls.platform(dynaFnName)`), the grouped navigate's renamed condition; texts :91-93, :113-117, :163-168, :203-207 | two copies of the key-naming/claim loop (`groupedKeyColumnNames` :43-76 re-derives `applyGroupBy` :140-188; a 3rd in ViewRelation:190-210); `isGroupReducer` reads the DynaFn registry + native catalog (:238-255); structural `Objects.equals` on RelationalOperation (:302); silent WITHHOLD of Join PMs and of per-row PMs outside the keys (:158-160, :176-185: "the engine's compiler ACCEPTS the mapping ... only the query that reads the property fails, on the database") — lite fails earlier with a not-mapped wall, no register row found | PORT; RE-EXPRESS the two key-name copies as one (return names from `applyGroupBy`): they differ only where `applyGroupBy` throws (`pmAsRelationalOp == null`, :162-169 vs `continue` :57-59), and `applyGroupBy` runs first on the same `rcm` (MN:1744 < :1751), so no answer changes; ASK (low): the withhold divergence as a register row |
| AggregateViewLift | 67 | MN `normalizeMapping` :385 (`lift`), :395 (`facts`) | each aggregate view's lifted function (`synthesizeClassMapping(.., setDiscriminated=true, ..)`, AVL:36) + its non-root `ClassBinding.Relational`; `AggregateViewFacts` on the main binding → AggregationAwareRouting (resolver) → which set answers → SQL | `lift` runs OUTSIDE the per-class try (MN:385 vs :362-380): a view set's synthesis error walls the WHOLE mapping (withElement), not a poison, and `lifted` already holds the main function (MN:382); `declaredKeys.getOrDefault(setKey(view.set()), NONE)` (AVL:44-45) for sets the pre-pass appears never to capture (MappingPrePass:90-95 scans `surface.classMappings()`, where aggregate view sets are not listed — they live under `aggregation().views()`): NONE on every input unless a top-level set shares the view set's effective id | PORT; ASK (low): confirm the view set's DeclaredKeys (→ MetamodelSeeds:306-434) is meant to be NONE |
| SetKeyFacts | 70 | MappingPrePass `prePass` :93; MN `normalizeMapping` :327, :392; AVL `lift` :44 | `DeclaredKeys` (distinct, groupBy/primaryKey column names, mapped columns, own property names) on every Relational binding → planner MetamodelSeeds:306-307, :419, :434 → system-store rows → metamodel query answers | none (ColumnRef-only filtering is deliberate: "other shapes: no key fact") | PORT |
| MappingLedger | 90 | ctor + `mappedInClosure` MN `normalize` :183-184; `poison` from US :392, MN :305/:313/:350/:358/:379/:429, AssociationSynthesis :332; `isMapped` JCE :202/:708/:729, MN :1214; `facts` MN :455 | `NormalizationFacts` (see above); `strictErrors` → the strict build's first error (MN:192-195); `operationMembers` → `ClassBinding.Operation` (not part of `facts`) | `poison` merge appends `"; "` (ML:40, answer text, call order matters); ML:74 HashSet→LinkedHashSet copy (hash order, membership-only) | PORT; RE-EXPRESS `mappedInClosure` as a plain set (only `contains` reads it) |
| MappingFacts | 114 | ML `facts` :88 | `unionMembers`, `routedTargetClasses` → PureModelContext:488, :495 (the chain-position cast rule: `unionMemberClasses`, `routedTargetClass`) → resolver → SQL | matches members by the RAW `setId()` (MF:42, :98), not the effective id (`SetId.of`/`ResolvedMapping.idOf`) every other site uses — a set whose `setId()` is null here (ClassMapping:134-136 "null when the user wrote ClassName without brackets"; NameResolver:1191 keeps it as parsed; the JSON identity sets, MappingClosures:115) cannot match, where `SetId.of` would; whether the parser's protocol record already carries the default id was NOT verified; `unionMembers` keeps the LAST class with a matching id (no `break`, MF:41-45); `routedTargetClass` returns at the first owner set with routes (MF:107-109) and null on disagreement — javadoc MF:21-23 "Each rule is the F-side reader's, verbatim" | PORT line for line (raw id and last-wins included; they are the frozen F-side rule) |

**Notes, single rows.** Pipeline: no counter field; frames are never shared across mappings or threads. GroupBySynthesis
`renameGroupedNavCond` (GBS:83-103) requires the just-emitted step to be a 4-argument `legacyNavigate`; a ROUTED grouped
Join PM (3-argument navigate with a route list, JCE:426-429) or a deduped re-emission throws "did not emit a single
navigate step" — a wall, not a crash. MappingLedger: `facts()` passes its live maps; the record constructor snapshots them.
