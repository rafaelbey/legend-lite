# H4 Frame -- E's entry, the mapping pre-pass and the include closures (9 classes, 2,539 lines)

All paths under TREE/core/src/main/java/com/legend/ unless noted; line numbers at 7c438f7b3. "Prod" = reached from
`Compiler.normalizeLayer:280` -> `ModelNormalizer.normalize` (compileModel, buildModule, compileErrors, buildModel, boot).
Every class here but `ModelNormalizer` is package-private and its callers are all in `normalizer/` (callers.txt
main-out[0]; verified by grep), so everything below runs inside one `ModelNormalizer.normalize` call, on one thread.

**Oracle coverage (applies to every error text below).** The frozen oracles record plans, MIR ids and identity counts,
not refusal texts (plan/MANIFEST.tsv: plan-ids rows are those "with an execution plan" or "with a test plan"; identity's
hits are counts). I grepped the decompressed `plan/execution-plans`, `plan/test-plans-host`, `model/answers` and
`identity/mir-store` for every E error text in these nine classes ("Circular M2M ~src chain", "mapping include cycle",
"Duplicated class mappings", "expands through itself", "Duplicated mapping include", "Can't find mapping",
"Circular 'extends' chain", "only Relational extends", "targets a non-root set", "must be typed (name", "names source
set", "collides with a declared property", "Can't find class mapping", "Can't find association", ...): 0 hits. So no
frozen oracle holds any of these texts or any `buildModule` wall from E; the judges are Java unit tests, mostly
`contains(...)` checks (IncludeRulesTest, ReferenceIntegrityTest, MappingNormalizerTest, MappedInClosureTest,
MappingClosuresTest, MetamodelStoreSubstitutionTest).

## 1. ModelNormalizer.java (495; public; the E entry)

| Family | Lines | Reached by | Reaches an answer | Idioms | Candidate |
|---|---|---|---|---|---|
| `normalize` (entry, composes E.1-E.6) | 94-142 | Prod: Compiler.java:280. Tests: testing/Phases.java:26, OneIndexTest:91,110 (+ModelNormalizerTest, ClassSourceTest, PhaseHCensusTest) | yes: the NormalizedModel's element list (structural elements, MappingNormalizer's lifts, then E.2, E.3, E.3b, E.3c, E.4, E.5 in that order) is what F ingests | none; order is a List | PORT (keep the append order) |
| `resolveSynthesized` (E.6, second name-resolution pass) | 144-189 | `normalize`:140 | yes: every synthesized body as G will type it (bare names -> FQNs / referents) -> G -> H/I -> SQL; type errors -> compileErrors | HashSet `universe` :159, copied to LinkedHashSet :164 -- membership only (`Scope.knownFqns().contains`, NameResolver.java:338,375,751,765,786): safe. Identity `resolved != stmt` :177 is a change detector (rebuild-or-keep), answer-neutral | PORT; the :177 test may be an always-rebuild or ptr_eq, either is answer-neutral |
| `requireQualifiedPropertiesAdopted` (the `adopted` check) | 191-228 | `normalize`:120 | no: throws IllegalStateException (internal "normalize before the knowledge layer") only on a pipeline-order bug; prod always adopts first (Compiler.java:275) | identity `d == dp` :220 = step 0 (c) `ModelNormalizer.adopted` 9/23 (never threw); `MissProbe.neverFired("ModelNormalizer#1")` :217; `owner == null` :218-219 is dead (an Optional never holds null) | RE-EXPRESS: a typed step (E takes the names crate's adopted model) or `Arc::ptr_eq` if kept; never a derived `==` (it would let an equal un-adopted property pass). Cannot change an answer: never fires on the prod path |
| `liftDerivedProperties` (E.2) | 230-255 | `normalize`:130 | yes: `<owner>$prop$<name>` via `DerivedProps.lift` (compiler root); F's `Property.Derived` names it; G types it at navigation; its FQN is quoted in G's errors (SpecCompiler.java:114-128 "keep their names, which the corpora's expectations quote") | none | PORT |
| `receiverType` | 257-271 | `liftConstraints`:287 | via E.3 | none | RE-EXPRESS: call `DerivedProps.receiverType` (compiler/DerivedProps.java:67-76, identical body) |
| `liftConstraints` + `synthConstraintFunction` + `synthConstraintMsgFunction` (E.3) | 273-299, 385-430 | `normalize`:131 | yes: `<owner>$constraint$<name>` and `$constraintMsg$`, named by F (ClassCompiler.java:83,91; ModelIntegrity.java:326); typed by `compileAllBodies` -> compileErrors | none | PORT |
| `liftFunctionConstraints` (E.3c) | 301-351 | `normalize`:133 | yes: the PRE/POST predicate FQN (`FunctionId...qualified().replace('$','%')` :318, `SynthFqn.condition`) and `Synthesized(PRE/POST, fd.qualifiedName(), c.name())`, whose `describe()` is G's error location (SpecCompiler.java:120-122) | throws IllegalStateException :324 on a non-inline condition (internal) | PORT |
| `liftDefaults` (E.3b) | 353-383 | `normalize`:132 | yes: `<owner>$default$<prop>`; SpecCompiler.java:208 reads the DEFAULT hat; `describe()` in errors | none | PORT |
| `liftServiceQueries` + `synthServiceQuery` (E.4) | 432-494 | `normalize`:134 | yes, narrowly: no reader names `<svc>$query` (grep: only SynthFqn/ModelNormalizer); it is typed by `compileAllBodies` (Compiler.java:868-886) -> compileErrors; the service test runner reads `sd.functionBody()` itself (test/ServiceTestRunner.java). The untyped-parameter ModelException text (:470-474, Phase NORMALIZE, embeds `SynthFqn.query(...)`) is an answer | the ModelException has no element and is not caught: it escapes even a tolerant `buildModule` | PORT |
| E.5 views (delegates to LiftedViews) | 123-127, 135 | `normalize` | see LiftedViews | -- | (LiftedViews) |

Notes. Edges out of E: `com.legend.compiler.{KnowledgeLayer, ModelBuilder, SynthFqn, NameResolver, DerivedProps}` (names
tier, `lite-names`), `compiler.element.type.PlatformTypes.ANY` :488 (`lite-types`); no `compiler.element` (F) or
`compiler.spec` edge. `resolveSynthesized` calls `NameResolver.resolveQueryIn` :176 and `NameResolver.platformFqns()`
:160 -- a names-tier call from E, allowed for `lite-normalizer`. Model methods called: `FunctionId.of(fd).qualified()`
:318 (ported, lite-model `function_id`), `FunctionDefinition.withSynthesizedFrom` :336,345,379,408,428,492 and
`isSynthesized` :311 (not in lite-model yet), `ImportScope.Builder().build()` :165, `ParsedModel.elementImports()` :171.
Surprises: (a) a key-space mismatch at :171-172: `elementImports` is keyed by `ParsedModel.keyOf` (a function's
FunctionId, model/ParsedModel.java:178), but E.3c's `ownerFqn` is `fd.qualifiedName()` (:337), so a PRE/POST body
always resolves under `none`; harmless today because D already resolved conditions under the function's own scope
(NameResolver.java:947-948) and resolution is idempotent -- port the lookup as written or key by the id, not both. (b)
the rebuild at :180-183 uses the 10-argument constructor, which drops pre/post constraints; synthesized functions have
none, so it is harmless. (c) The class javadoc (:82-83) says the sub-slices' order "is free"; I found no answer that
depends on it alone (compileAllBodies sorts by FQN, Compiler.java:868), but the element order is the NormalizedModel's
and is cheap to keep. (d) E.4's ModelException and E.3c's IllegalStateException are not walled under a wall sink,
unlike mapping and view failures.

## 2. LiftedViews.java (171; E.5)

| Family | Lines | Reached by | Reaches an answer | Idioms | Candidate |
|---|---|---|---|---|---|
| ctor (index the views) | 59-71 | Prod: ModelNormalizer.java:126. Also MappingNormalizer.java:144 (the public 2-/3-arg `normalize`, called only by MappingNormalizerTest and ModelBuilderTest); tests IncludeRulesTest:87, MappedInClosureTest:31 | via `order` (declaration order of views: parsed databases' order, then `ModelBuilder.viewsOf` order) | IdentityHashMap `owners` :49 (step 0 112/0) | PORT with `IdMap` |
| `body` (memoized lift, cycle guard, memoized wall) | 73-115 | `liftAll`:148; MappingNormalizer.java:1616; JoinChainEmission.java:408,437,476,568; ViewRelation.java:64,111 (view-on-view, re-entrant) | yes: the view's relation body (ViewRelation.viewRelationExpr) is the `<db>$view$<spelling>` function's body and every mapping expansion site's source -> G -> H/I -> SQL; the declared return type `spell(signature)` is checked against it | IdentityHashMap `functions` :52 (393/112), `walls` :54 (0/112), identity set `expanding` :56 (0/112); re-entrant get-then-put memo; `catch (ModelException \| NotImplementedException)` :109 stores and rethrows the SAME exception object (memoized failure, "the same exception at every site") | PORT with `IdMap`/`IdSet` (no iteration needed: `functions()` iterates `order`); the memo as `IdMap<View, Result<FunctionDefinition, Error>>`, computed outside any borrow (re-entrant) |
| `spell` | 117-141 | `body`:104 | yes: the lifted view function's return type `Relation<(col: T[m], ...)>` | `default -> c.type().typeName()` :128 (a class FQN); multiplicity collapses anything not [1] to [0..1] :131-133 (fallback-shaped; view columns are [1] or [0..1]) | PORT |
| `liftAll` | 143-158 | ModelNormalizer.java:127, MappingNormalizer.java:145 | yes: strict: the view's exception escapes (compileErrors text = its message); tolerant: `wallSink.putIfAbsent(owner.fqn(), String.valueOf(e.getMessage()))` :155 -- a buildModule wall keyed by the lifted FQN | catch :149 | PORT |
| `functions` | 160-170 | ModelNormalizer.java:135 | yes: the E.5 elements, declaration order, walled views absent | none | PORT |

Notes. Edges: `compiler.element.ViewSignatures.of` :99 is the one F (`lite-element`) edge in these nine classes;
`compiler.element.type.{Type, Multiplicity, PlatformTypes}` :98,123-140 are `lite-types`. Identity is semantic: "two
schemas may declare a view of one name with different bodies" (:34-35); with structural keys, two structurally equal
views in two schemas (or databases) would share one `owners` entry, so one view gets the other's lift/FQN and
`functions()` emits one function twice. Step 0: `walls` 0 hits and `expanding` 0 hits -- the oracle cannot catch a
mistake in the memoized-wall path or the cycle guard (no corpus view walls or cycles). A RE-EXPRESS keyed by
`ViewLift.fqn()` (unique per view: `SynthFqn.view(ownerDb, spelling)`, ModelBuilder.java:1213-1216) is possible, but
every `body(ViewDefinition)` caller (7 sites above) would need the lift in hand; the Rust index must also hand out the
same `Arc` from `findView` (the callers' source) as from `viewsOf` (the ctor's), or the `IdMap` misses and the
"view 'x' is not declared by any database of this model" ModelException (:87-88, never reached: owners 112/0) fires.
Catch texts: :109 produces nothing new (stores and rethrows); :149 embeds the exception's FULL message, not its class
name. Surprise: every other wall in E records the FIRST LINE (`split("\n")[0]`: MappingPrePass.java:78,
MappingNormalizer.java:203,219, Compiler.java:470) and gets element attribution through `MappingNormalizer.withElement`;
view walls get neither (`liftAll` runs before MappingNormalizer, ModelNormalizer.java:127). Answer-bearing through
`buildModule`'s walls: port as is or ASK to unify. A view cycle A->B->A walls both views with the one exception naming
the first in `order` (A): deterministic. Tolerant-only edge (unverified): a Database declared twice makes the ctor add
its views to `order` twice (both iterations ask `viewsOf(sameFqn)`), so `functions()` would emit each lift twice.

## 3. MappingPrePass.java (275)

| Family | Lines | Reached by | Reaches an answer | Idioms | Candidate |
|---|---|---|---|---|---|
| `run` (per-mapping pre-pass + validation, strict/tolerant policy) | 42-82 | Prod: MappingNormalizer.java:171. Tests: IncludeRulesTest:86, MappedInClosureTest:31 | yes: the `ResolvedMapping` per mapping (what synthesis reads); strict: the first invalid set's ModelException (declaration order) :66-67; tolerant: walls and per-set poison reasons | `catch (ModelException)` :73 -> `wallSink.putIfAbsent(e.element(), first line of message)`; LinkedHashMap `pre`, `reasons` | PORT |
| `prePass` (the construction order) | 84-113 | `run`:57 | yes: surface -> M2M cycles -> declared keys -> extends -> implicit inheritance -> store-ref qualification -> implicit ops; each step's output feeds synthesis | HashMap `declaredKeys` :90 -- read only by `getOrDefault` (MappingNormalizer.java:327,392; AggregateViewLift.java:44): safe | PORT (the five `ResolvedMapping` rebuilds can be one builder: answer-neutral) |
| `resolveExtends` + `flattenExtends` | 120-223 | `prePass`:104 | yes: flattened Relational sets (merged PMs by `UnionSynthesis.pmIdentity`, inherit-if-absent table attributes) -> synthesis -> SQL; errors: "Circular 'extends' chain ..." / "... but no class mapping with set id ..." (ModelException NORMALIZE), "only Relational extends is supported" (NotImplementedException, re-wrapped by `withElement` to ModelException Phase NORMALIZE) | HashMap `bySetId` :134 -- `get` only: safe; LinkedHashSet `chain`, LinkedHashMap `merged` | PORT |
| `detectM2MCycles` + `walkM2MChain` | 225-263 | `prePass`:87 | yes: "Circular M2M ~src chain detected in mapping 'M': A -> B -> A" (:247-250) -> wall / compileErrors | **HashMap `pureByTarget` :231 ITERATED `.values()` :237; the first root that reaches a cycle spells the chain, so the text depends on Java's bucket order (A->B->A vs B->A->B)**; identity `next != pcm` :259 = step 0 `MappingPrePass.selfEdge` 11/0 | ASK (see notes); the identity RE-EXPRESS below |
| `kindOf` | 265-274 | `resolveExtends`:147 | yes: message text | exhaustive switch | PORT |

Notes. Catch :73: re-throws when strict or when `e.element()` is null; else walls `e.element()` with
`String.valueOf(e.getMessage()).split("\n")[0]` -- the message's first line, never the class name. Because
`MappingNormalizer.withElement` (MappingNormalizer.java:238-256) attaches the mapping FQN only when the exception has no
element, a ModelException carrying ANOTHER element's FQN walls that element and the mapping is dropped from `pre`
without a wall of its own (read, not measured). NotImplementedException/MappingResolutionException become
ModelException Phase NORMALIZE in `withElement`; genuine bugs (ISE, NPE) propagate. Tolerant reasons :70 record the
FULL message per set id (`r.withInvalid`), read by `ResolvedMapping.invalidReason` -> MappingNormalizer.java:311,356
`ledger.poison(...)` -> raised when a query uses the set (answer text). `split("\n")[0]` throws on a message of only
newlines (TRIAGE C4: Java drops trailing empties); unreachable in practice.
Hash order (the one C1 site in these nine classes): Java's text is deterministic but unpinned (only
`MappingNormalizerTest:4588` `contains("Circular M2M")`; no oracle row). Options: (1) emulate Java's String-keyed
HashMap bucket order -- exact, costs a JDK-hash emulation for one message; (2) iterate in declaration order
(`md.classMappings()`, last-wins per class kept) and register the divergence -- simpler, a SEMANTICS_REGISTER row the
owner rules. Also worth the owner's eye: this refusal is lite's own (no upstream source cited at the site).
Identity :259: `pureByTarget.put(className, pcm)` keeps one object per class (last wins :234) and every `pcm` walked
comes from that map, so `next != pcm` <=> `!pcm.sourceClass().equals(pcm.className())` (null source: both say "stop").
RE-EXPRESS as that string test: answer-identical. The probe counts `same("model.MappingPrePass.selfEdge", next == pcm)`
(rust/tools/identity-hits-probe.patch:917), so 11 true / 0 false means 11 self-edges and NO real chain (`A ~src B` with
B mapped) in any lane: the recursive arm and the cycle text are oracle-blind. Note the last-wins: an earlier Pure set
of the same class is never a root and never reachable as `next`, so its chain is never checked.
`LegacyMappingDefinition.withClassMappings` :152 (deferred in lite-model).

## 4. MappingClosures.java (451; the include-order facts)

| Family | Lines | Reached by | Reaches an answer | Idioms | Candidate |
|---|---|---|---|---|---|
| `of` + memo fields + `missingMapping` | 49-68 | MappingPrePass.java:86,103; MappingValidation.java:51; AssociationSynthesis.java:67 | via the facts below; `missingMapping` = "Can't find mapping 'x'" (Phase MAPPING, engine's words) -> wall / compileErrors (ReferenceIntegrityTest:186-188) | memo on `KnowledgeLayer.derived` (KnowledgeLayer.java:56,64-71: plain `HashMap<Class<?>,Object>`, get-then-put); HashMaps `surfaces` :52, `closures` :53 get/put only | RE-EXPRESS the memo's home (see notes) |
| `surface` + `withJsonIdentitySets` | 81-128 | MappingPrePass.java:86; `surfaceOf`:95 (every walker) | yes: the JSON-connection identity sets (`ClassMapping.Relational`, root, `sourceUrl = jmc.url()`) become synthesized class functions reading the URL -> plans | reads `model.runtimes()` (ingest order) and only the record components `rd.jsonConnections()`, `rd.mappings()` :100-101, `jmc.className()`, `jmc.url()` -- no RuntimeDefinition helper (it has none public); `withClassMappings` :127 | PORT |
| `Closure.mappings` / `walkMappings` | 151-177 | ResolvedMapping.java:126 (`closure()`), AssociationSynthesis.java:67 | yes: the include order every closure walk reads (M2M routes, union pins, association hoisting) | HashSet `seen` :157 membership; unresolved include skipped silently (`ifPresent`) | PORT |
| `relationalSets` | 179-195 | ResolvedMapping.java:149 <- MappingNormalizer.java:2320, JoinChainEmission.java:895 | yes: candidate sets for routing | HashMap :184, `getOrDefault` only: safe | PORT |
| `sets` / `walkSets` | 197-229 | ResolvedMapping.java:121,156,161 (`set`, `includedSets`, `visibleSets`) | yes: set-by-id resolution through includes, store substitutions applied (`StoreSubstitutionRewrite.apply` :225); later include overrides; throws "Can't find mapping" | HashSet `seen` :205 (keyed by the include's spelling = FQN, lookups are exact) | PORT |
| `union` / `inheritance` / `walkOps` | 231-269 | ResolvedMapping.java:176,186 | yes: the operation set for a class (last found wins, R1) | no `seen`: a diamond is walked twice (same objects, same answer); a cycle is impossible (refused earlier by `resolveAllStores`); `union("")` :253 is a "build both" hack | PORT (one builder for both maps is answer-neutral) |
| `duplicateIds` / `walkIds` | 271-307 | MappingValidation.java:52 | yes: "Duplicated class mappings found with ID [...] in mapping 'M'" (Phase MODEL) | identity `first.get(id) != cm` :297 = step 0 `MappingClosures.first` 0/2; HashSets `seen` :279, `own` :293 membership | RE-EXPRESS (drop `first`), see notes |
| `roots` / `walkRoots` / `ownRoots` | 309-351 | ResolvedMapping.java:70,71 | yes: root set per class (deeper first, nearer overrides; `*` or the sole set) -> `isRootOrSole`, union member selection (UnionSynthesis.java:627) | HashSet `seen` :316; LinkedHashMaps | PORT |
| `enumerationMappings` / `walkEnums` | 353-387 | ResolvedMapping.java:210 <- MappingNormalizer.java:453,2381 | yes: enum mappings in scope -> enum decoding in SQL | HashSet `seen` :359; its own include-path rule (in-package first, :372-378) | PORT |
| `pairEntries` / `walkPairs` / `ownPairs` | 389-449 | ResolvedMapping.java:218-219 <- UnionSynthesis.java:471 | yes: per-pair association Join PMs (target set stamped) -> navigation SQL; "Can't find association 'x'" | HashMap memo `pairEntries` :141 get/put; HashSet `seen` :397 | PORT |

Notes. **Memo and D5.** `of` memoizes on the index's `KnowledgeLayer.derived` -- its only user (grep:
`knowledge().derived` appears only at MappingClosures.java:67; the other `.derived(` calls are `ModelContext`'s). Inner
memos: `surfaces` by authored FQN, `closures` by mapping FQN, each Closure's seven lazy fields
(CodeShapeGuardrailTest.java:68-70 allowlists `MappingClosures.mappings/sets/unions/inheritances/roots/enums/
relationalSets` as "a memo of a pure derivation") and `pairEntries` per class. Every write and read happens inside E:
callers are package-private normalizer code, all run from `ModelNormalizer.normalize`. The index is made per
`normalizeLayer` (Compiler.java:276); the boot layer's is "checked and then discarded" (Compiler.java:356-357); the
graph's outlives E into F (Compiler.java:280 -> `PureModelContext.from`) but nothing reads the memo after E. So it is
not a post-freeze write and not D5-relevant (PHASE3_DESIGN.md §2.1 :379 lists it in the "written after
`PureModelContext.from`" table; by reading, it is written only before `from`). RE-EXPRESS: an E-local value owned by
the normalize call (borrowing the index), with unsync `OnceCell` fields -- not a named `OnceLock` on the frozen model
(§2.5 item 4). Answer-neutral: a memo of pure functions of (index, runtimes) with no reader after E. Keep the facts
LAZY: `sets()`/`duplicateIds()`/`enumerationMappings()` throw `missingMapping` for the mapping that asks, and the
tolerant catch is per mapping (MappingPrePass.java:73); an eager build would throw from the wrong place. No walker
re-enters `MappingClosures` (surface/ownPairs call only the index), so the cells cannot re-enter themselves; compute
outside any map borrow.
**Walker rules are answers.** The class javadoc (:41-45): "the walkers differed from one another ... those differences
are the current semantics" (visible sets: later include wins; operation sets: last found; roots: includes first, then
own). The closures are computed over SURFACES (authored + JSON identity sets), never over the included mappings'
pre-passed records (no extends flattening, no implicit ops of the included mapping): PORT line for line.
**Identity :297.** `walkIds` visits each mapping once (`seen` by the exact name `surfaceOf` looks up, ModelBuilder.java:
900-902 is exact), surfaces are memoized per mapping, and no producer I found shares a `ClassMapping` object between two
mappings' surfaces (JSON identity sets are made fresh per surface, :113). So whenever `prev != null &&
!prev.equals(mappingFqn)`, `first.get(id) != cm` is always true and the `first` map is dead; the probe counts
`same("model.MappingClosures.first", first.get(id) == cm)` (rust/tools/identity-hits-probe.patch:904), so 0 true / 2
false agrees: the "same object" arm never fired, and both evaluations were genuine duplicates reported. RE-EXPRESS: drop `first` (or keep `Arc::ptr_eq`). Never a derived `PartialEq`: it would
SUPPRESS a genuine duplicate when two mappings declare structurally equal sets under one id, which Java reports. The
oracle cannot catch either mistake (0 hits; and no dup-id text in any oracle). PHASE3_DESIGN.md:108's reason ("a
diamond's one mapping vs a genuine duplicate") is already handled by `seen`.
**Include-path rules.** Three spellings of "resolve an include path": exact (`surfaceOf`, every walker),
in-package-first (`walkEnums` :372-378), exact-then-in-package (`StoreSubstitutionRewrite.includeFqn`). Since
`sets()` throws "Can't find mapping" for a non-exact path in the pre-pass (via `ImplicitInheritance.apply` ->
`visibleSets`), the in-package branches look unreachable for any mapping that survives the pre-pass, and D already
qualifies include paths (NameResolver.java:1119-1125); unmeasured.
Census pins: LegacyReachbackCensusTest.java:74 pins exactly 2 `findLegacyMapping` reaches here (:95, :375).
Model methods: `withClassMappings` :127; `RuntimeDefinition` record components only (:100-101); `SetId.of` via
`ResolvedMapping.idOf` :222,295 (ported: lite-model names.rs `set_id_of_class_mapping`).

## 5. ResolvedMapping.java (244)

| Family | Lines | Reached by | Reaches an answer | Idioms | Candidate |
|---|---|---|---|---|---|
| record + ctor + `withMapping`/`withInvalid` | 33-90 | built at MappingPrePass.java:102 (5 rebuilds :104-112, :71); read by 16 normalizer classes (callers.txt) | yes: everything synthesis asks of a mapping | HashMaps `ownByClass` :62, `ownById` :63: `get`/`getOrDefault` only (:120,144,171,181): safe; `roots` LinkedHashMap | PORT (a struct) |
| faces + `idOf` | 97-111 | everywhere in E | yes | `idOf` = `SetId.of(cm)` (ported) | PORT |
| `set`, `includedSets`, `visibleSets` | 114-122, 153-166 | UnionSynthesis, JoinChainEmission, ImplicitInheritance, MappingPrePass, MappingValidation, ... | yes: set resolution (own first, then the closure's) | none | PORT |
| `closure()` | 124-136 | M2mRouteGuards.java:84; AssociationSynthesis.java:710; XStorePureEnds.java:78; UnionSynthesis.java:64,71,112,120,932; MappingNormalizer.java:2150 | yes: own record then includes' surfaces | HashSet `seen` :128 membership, and redundant (`Closure.mappings()` already dedupes with this FQN seeded, MappingClosures.java:157-158) | RE-EXPRESS: `[md] ++ closure.mappings()`, answer-identical |
| `relationalSets`, `unionOf`, `inheritanceOf`, `roots`, `isRootOrSole`, `enumerationMappingsWithIncludes`, `pairEntries` | 138-222 | AssociationSynthesis.java:102-137,487-488; UnionSynthesis.java:317-359,471; JoinChainEmission.java:895; MappingNormalizer.java:453,2320,2381 | yes (routing, unions, enums, association navigation -> SQL) | none beyond the closure's | PORT |
| `memberOrdinal` | 224-243 | UnionSynthesis.java:351 | yes: a union member's ordinal (route classification) | HashSet `seen` :233 (extends-cycle guard) | PORT |

Notes. A pure data/lookup class over MappingClosures; no catch, no identity. `invalidReason` :92 carries
MappingValidation's full message into `ledger.poison` (MappingNormalizer.java:311,356).

## 6. ImplicitInheritance.java (227)

| Family | Lines | Reached by | Reaches an answer | Idioms | Candidate |
|---|---|---|---|---|---|
| `apply` + `nearestMappedAncestor` (implicit same-extent inheritance) | 27-98, 198-225 | MappingPrePass.java:105 | yes: inherited unqualified PMs merged into a child set -> its synthesized function -> SQL; its `visibleSets()` call is also where a missing include first throws "Can't find mapping" | HashMap `bySetId` :40 ITERATED `.values()` :43 into HashMap `byClass` :42 lists -- order reaches nothing: the only consumer keeps `cands.size()==1` and returns null on >1 (:217-222); HashSet `own` :70 membership | PORT (IndexMap or HashMap: either is answer-identical here) |
| `implicitOpsForRoutedTargets` | 100-196 | MappingPrePass.java:112 | yes: appended `ClassMapping.Inheritance(cls, null, null, false)` ops, in `implied` order (LinkedHashSet: association mappings, then class mappings) -> union/route synthesis; "Can't find association 'x'" (Phase MAPPING) :133-136 | HashMap `bySetId` :121 ITERATED `.values()` :123 only into HashSet `mappedClasses` :122 (membership): safe; HashMap `ownerByProp` :163 `getOrDefault` only: safe; MissProbe :170 (owner class absent -> skip; FallbackLedgerTest.java:58 count 1) | PORT |

Notes. `withClassMappings` :97, :195. Knowledge calls (names tier): `isSubtype`, `hierarchyClass`, `propertyType`,
`ancestorsAndSelf` (nearest-first order, :205, answer-bearing: the nearest qualifying ancestor wins). Cross-normalizer:
`MappingNormalizer.inferMainTableQuiet` :59,213, `UnionSynthesis.collectRoutedJoins` :164,
`AssociationSynthesis.resolveAssociation/associationTargetClass` :133,145. The former never-fired probe at :133 became
the user's error on 2026-10-21 (FallbackLedgerTest.java:80-82; ReferenceIntegrityTest.java:194-201).

## 7. StoreSubstitutionRewrite.java (404)

| Family | Lines | Reached by | Reaches an answer | Idioms | Candidate |
|---|---|---|---|---|---|
| `apply` / `applyWith` / `views` / `db` / `filter` / `pointer` / `chain` / `pm` / `op` / `applyAssoc` (the exhaustive store-ref rewrite) | 35-201 | `apply`: MappingClosures.java:225; `applyWith`/`applyAssoc`: `collectDatabases`, `qualifyStoreRefs` | yes: included sets re-pointed to the replacement store -> synthesized functions -> SQL against that store | `Collectors.toMap(..., (a,b) -> b)` :40-43 is a HashMap, read only by `getOrDefault` (:82): safe; last-wins on a repeated `originalStore` (silent); exhaustive switches, no default | PORT |
| `collectDatabases` | 203-231 | `qualifyStoreRefs`:368 | indirectly (feeds `qualifyStoreRefs`) | a recording `AbstractMap` whose `getOrDefault` records every key (:211-224): the rewrite used as a visitor | RE-EXPRESS: the rewrite takes a store mapper (`FnMut(&str) -> String`); collecting = running it with a recording mapper. Same traversal, same keys |
| `resolveAllStores` + `includeFqn` + `storeFqn` (the engine's `Mapping.resolveStore`, Kahn over the include graph) | 233-349 | Prod: MappingNormalizer.java:162 | yes: `MappingDefinition.resolvedStores` -> planner/MetamodelSeeds.java:173-185 (store-resolution seed rows, sorted by TreeMap) -> plans and metamodel queries; errors: "Duplicated mapping include 'x' in mapping 'M'" (MODEL, element M) :264-266, "mapping include cycle: [..] -- a mapping cannot include itself, ..." (NORMALIZE, element = first) :320-323 | HashSet `listed` :258 membership; LinkedHashMaps elsewhere; the `ready` order only orders `out`, which is read by `getOrDefault` (answer-neutral); PARK-26 lives here | PORT the composition; ASK on PARK-26 (notes) |
| `qualifyStoreRefs` (import-scope store-ref qualification) | 351-403 | MappingPrePass.java:108 | yes when it rewrites (a store ref -> SQL); probably never in prod (notes) | MissProbe :382 (FallbackLedgerTest.java:65 count 1); `withClassMappings` :396 | RE-EXPRESS (drop the dead `current` test), and measure reach |

Notes. **PARK-26** (ParkedWorkLedgerTest.java:173-177; docs/PARKED_WORK_LEDGER.md:644-668): "a mapping included under
two spellings is refused as an include cycle" -- `include w::A` and `include A` in package `w` resolve (via
`includeFqn`, :268-271) to one name, "counted twice and satisfied once" (`pending` :274 counts list entries, the
decrement :305-311 fires once per finished mapping), so the mapping is refused with the wrong reason. The anchor regex
is `if \(fqn != null\) \{\s+incs\.add\(fqn\);` (:269-270). A Rust re-expression with de-duplicated in-degrees would
silently fix it (accept), changing an answer. ASK: (1) PORT the defect (same wrong refusal); (2) fix as "Duplicated
mapping include" by resolved name; (3) fix by accepting. The ledger: "The engine's rule for the pair is not in the
repository's upstream notes"; acceptance is a test in IncludeRulesTest.
**Dead branch in `qualifyStoreRefs`.** `:371` skips any `r` with `model.hasDatabaseExact(r)`; `findDatabase` is now
exact-only and uses the same lookup (ModelBuilder.java:871-885 vs :1359-1361), so `current` (:380-382) is always null,
the MissProbe fires on every bare ref that reaches it, and `!cand.equals(current)` (:386) is always true. The comment
"rewrite ONLY the SHADOWED cases ... When they agree the raw spelling stays" (:374-379) and the javadoc's "lenient
simple-name fallback" (:358-359) describe behaviour that no longer exists: today it qualifies unconditionally whenever a
wildcard import names a registered database (first wildcard wins, :383-390). Likewise `storeFqn` (:346-349) is the
identity on every input (exact lookup returns the database whose name is the input). Reach: D already resolves every
mapping store ref with the same import tiers (NameResolver.java:1178-1181, 1327-1349, 1596-1646), and its universe
holds every declared database, so a bare ref surviving D should have no wildcard candidate; the possible exception is a
mapping D walled in a tolerant build and passed on unresolved (unverified). DON'T PORT only with a measurement (count
non-empty `q` over the lanes); until then PORT without `current`/`storeFqn` (answer-identical by the argument above).
Both refusals escape a tolerant build: `resolveAllStores` is called at MappingNormalizer.java:161-164, outside the
per-mapping try, so "Duplicated mapping include" and "mapping include cycle" throw out of `buildModule` instead of
walling the mapping (the same asymmetry as E.4's service error and the view walls); this is also why no closure walker
can meet a cycle. Also: the cycle message's list (:318-319) holds every mapping never made ready -- the cycle's members AND every
mapping that includes one -- so it over-reports "the mappings in it". Two include-path rules disagree (exact here plus
in-package; exact in MappingClosures). Phase labels to port exactly: MODEL (:264), NORMALIZE (:320).

## 8. MappingValidation.java (114)

| Family | Lines | Reached by | Reaches an answer | Idioms | Candidate |
|---|---|---|---|---|---|
| `run` | 34-72 | MappingPrePass.java:65 | yes: dup-id ModelException thrown (MODEL, element = mapping) in both builds :53-57; per-set invalid map (declaration order) -> strict: the first is thrown (MappingPrePass.java:66-67); tolerant: per-set poison text | `catch (ModelException)` :67 -> `invalid.put(idOf(cm), e)` (the exception kept, keyed by set id, never by object -- :38-42 records the identity-key bug this fixed); `default -> { }` :65 (Inheritance, RelationFunction unvalidated) | PORT (spell the two no-op arms explicitly in Rust) |
| `validateUnionMembers` | 74-87 | `run`:64 | yes: "Can't find class mapping 'x' in mapping 'M'" (MAPPING) | HashSet `ids` :77 membership: safe | PORT |
| `validatePmNames` | 89-104 | `run`:62 | yes: "PropertyMapping 'p' references property not declared on class 'C'; mapping=M" (MODEL) | `MissProbe.knownMiss` :93 -> `if (cd == null) return` :94: a set of an unknown class skips PM-name validation (censused miss; FallbackLedgerTest.java:62 count 2) | PORT |
| `validateM2mRoutes` | 106-113 | `run`:63 | yes (via M2mRouteGuards) | `MissProbe.knownMiss` :109 | PORT |

Notes. Catch :67 produces no text itself; the text is the ModelException's message, later the full message as a
poison reason (MappingPrePass.java:70) or, strict, the exception itself (first line in a wall). No class name.
`closure.duplicateIds()` :52 is the only `duplicateIds` caller.

## 9. M2mRouteGuards.java (158)

| Family | Lines | Reached by | Reaches an answer | Idioms | Candidate |
|---|---|---|---|---|---|
| `m2mBindingKey` | 22-46 | MappingNormalizer.java:1192 | yes: the M2M binding's storage key (the `<base>AllVersions` -> base rule) in the binding table; "M2M PropertyBinding 'p' is not declared on class 'C'; mapping=M" (NORMALIZE) | `tgt == null` -> the name unchecked :32 | PORT |
| `requireBenignRoute` + `setIdMatches` | 51-133 | MappingValidation.java:111 | yes: refusals "M2M binding 'p[s, t]' names source set ..." :70-74, "M2M set-routed binding 'p[t]' targets a non-root set ..." :107-113 (NORMALIZE) | MissProbe :96 (a miss falls through to the loud refusal; FallbackLedgerTest.java:60 count 1); `setIdMatches` re-implements `SetId.of(cm).equals(id)` except for an empty declared id | PORT |
| `localField` | 135-157 | MappingNormalizer.java:1170 | yes: the local property's `KeyExpression(expr, false, true)` binding column; collision refusal (NORMALIZE) | none | PORT |

Notes. Reads `md.closure()` :84 (surfaces, un-substituted; only class names, set ids and `root` are read, so
substitution is irrelevant). Knowledge calls `propertyType`, `hierarchyClass` (names tier). No hash collection, no
identity, no catch.

## Cross-class summary

- **Identity (step 0 rows).** LiftedViews `owners` 112/0, `functions` 393/112, `walls` 0/112, `expanding` 0/112 ->
  PORT with `IdMap`/`IdSet` (semantic; 0-hit tables are oracle-blind). MappingClosures `first` 0/2 -> RE-EXPRESS (dead;
  never derived `==`). MappingPrePass `selfEdge` 11/0 (11 self-edges, no real chain: the recursive arm is oracle-blind)
  -> RE-EXPRESS as `sourceClass == className`. ModelNormalizer
  `adopted` 9/23 -> RE-EXPRESS (internal assertion). Not in step 0, answer-neutral: ModelNormalizer.java:177.
- **Hash collections.** Iterated with order reaching an answer: MappingPrePass.java:231/237 only (error text).
  Iterated, order reaches nothing: ImplicitInheritance.java:40/43, :121/123. Lookup or membership only: ImplicitInheritance
  :42,70,122,163; MappingClosures :52,53,141,157,184,205,279,293,316,359,397; MappingPrePass :90,134; ResolvedMapping
  :62,63,128,233; MappingValidation :77; StoreSubstitutionRewrite :40,258; ModelNormalizer :159.
- **Catches.** LiftedViews :109 (memoize + rethrow), :149 (wall = FULL message, no attribution); MappingPrePass :73
  (wall = first line, element-attributed); MappingValidation :67 (per-set exception -> full message as poison). None
  embeds a class name; none is `catch (RuntimeException)`.
- **Edges outside the normalizer.** F (`lite-element`): LiftedViews.java:99 `ViewSignatures.of` only. `lite-types`:
  LiftedViews.java:98,123-140; ModelNormalizer.java:488. Names tier: ModelBuilder, KnowledgeLayer, NameResolver
  (ModelNormalizer.java:160,176), SynthFqn, DerivedProps. No `compiler.spec` edge. All within core-layers.txt:90.
- **Deferred lite-model methods called.** `LegacyMappingDefinition.withClassMappings`: MappingClosures.java:127,
  MappingPrePass.java:152, ImplicitInheritance.java:97,195, StoreSubstitutionRewrite.java:396.
  `FunctionDefinition.withSynthesizedFrom`/`isSynthesized` (ModelNormalizer, LiftedViews.java:106) are not ported either.
  `RuntimeDefinition`: record components only (MappingClosures.java:100-101). `MappingDefinition.ClassBinding.DeclaredKeys`
  is a type only here (MappingPrePass.java:90); no `ClassBindings.of`, `position`, `NormalizationFacts`, witness-column
  name or `Col.bindSrc` call in these nine classes.
- **Guards over these sources.** FallbackLedgerTest.java:56-67 (ImplicitInheritance 1, M2mRouteGuards 1,
  MappingValidation 2, StoreSubstitutionRewrite 1; ModelNormalizer's `neverFired` counts toward NEVER_FIRED_FLOOR 5);
  ParkedWorkLedgerTest PARK-26; LegacyReachbackCensusTest.java:74 (MappingClosures 2); CodeShapeGuardrailTest.java:68-70
  (MappingClosures' lazy fields).
