# H6: compiler/element helpers (ModelContext, the F-package helpers, the Typed* records)

All paths under TREE/core/src/main/java/com/legend/ unless prefixed. "PMC" = compiler/element/PureModelContext.java, the
sole implementation of ModelContext (`public final class PureModelContext implements ModelContext`, PMC:38; a grep of
core, spec, pct, wasm, datacube for `implements ... ModelContext` finds no other; NoEagerUserClassLoadsTest.java:72's
`Proxy` forwards every call to a real PMC, :94).

**The short answer to "is it F's at all?"**: of the helpers, only `StoreCompiler.tableSchema`/`columnType`,
`EqualityKeys.isKeyStereotype` and the Typed* records are called by F. `RelationalTypeInference`, `ViewSignatures` and
`MilestoningStrategy.ofStereotypeOrNull` are E's (production via `ModelNormalizer.normalize`). `ClassLayouts`,
`EqualityKeys.resolve`, `RelationalOpRows`, `Temporal` and `StoreCompiler.declaredType` have no caller in E, F or (but
for Temporal and declaredType) G: their first callers are G, H, I and the planner.

---

## ModelContext.java (452 lines) -- the interface, by family

| Family | Lines | Reached by | Reaches an answer | Idioms | Recommendation |
|---|---|---|---|---|---|
| A. Abstract lookups (`findClass`, `derived`, `findEnum`, `findFunction`, `findFunctionById`, `findMapping`, `findMeasure`, `isPackage`, `findProfile`, `findAssociationOf/End`, `findAssociation`, `findRuntime`, `findConnection`, `isModelConnection`, `databases`, `findType`, `findProperty`, `findTable`, `findView` x2, `viewMainTable`, `findViewFunction`, `isExecutionContextElement`, `subtree`, `declarations`, `implementations`) | 31-40, 79-139, 218-278, 365, 378-385 | Every later layer (callers.txt: 91 main files; G, H, I, planner, testplan, lineage, setup, server). Implemented only by PMC | Yes: every type, plan and error downstream reads the model through these | `derived(Class<T>, Function)` (:40) is an open `Class<?>`-keyed memo on the model (PMC:269-278, get-then-put, `ConcurrentHashMap`; D5) | PORT with PMC as methods of a concrete `Model` (no trait: one implementation). `derived` RE-EXPRESS as one named `OnceLock` per fact (PHASE3_DESIGN.md §2.5 item 4, already ruled) |
| B. Default raw-model passthroughs (`findClassDefinition`, `findLegacyMapping`, `findData`, `findDataSpace`, `dataElements`, `findTableDefinition`, `findDatabase`, `findJoinDefinition`, `findFunctionDefinition(s)`, `findTableMilestoning`) | 66-76, 144-158, 318-357 | Overridden by PMC: 298-301, 304-307, 550, 556, 562, 853-856, 841-844, 847-850, 698, 686, 744. The default bodies (`Optional.empty()`, `Stream.empty()`) never run | The PMC overrides do; the defaults do not | Defaults answer "absent" -- a fallback shape (invariant 4) -- but unreachable | Default bodies: DON'T PORT (dead: sole implementation overrides each). Methods: PORT with PMC |
| C. Default E-fact passthroughs (`mappingPoison`, `mappingSetPoison`, `mappingAssociationPoison`, `unionMemberClasses`, `routedTargetClass`, `mixedUnionMembers`, `unionKeyThreads`) | 160-211 | Overridden: PMC:518, 524, 532, 485, 492, 501, 508. Callers: resolver (StoreResolver etc.), compiler/spec (unionKeyThreads) | Yes: E's poison reasons become H's error text; union members and key threads shape H's SQL | Defaults return `null`/empty: dead | As B |
| D. Default universes (`elementFqns`, `resolutionUniverse`, `functionFqns`, `classifierInstances`, `tracksClassifier`, `isDatabase`) | 47-58, 280-310 | Overridden: PMC:345, 315, 829, 793, 788, 836. `resolutionUniverse` callers: Compiler.java:678 (`resolveNames`, contains only), testplan/JudgmentFacts.java:117 (ITERATED) | Yes, see hash note | :55 `new HashSet<>(elementFqns())` then a `LinkedHashSet` copy: the copy takes the hash order. Dead (PMC:315 overrides). But the live override's iterator (PMC:329-333) is also a `HashSet` and IS iterated: JudgmentFacts.java:117 -> `byClass` `LinkedHashMap` (:93) -> `HostJudgment.Keys.byClass` -> testexecutionplan/HostJson.java:655 `"byClass"` = **hash order written into a test plan** when a key slot is declared `Any` and >= 2 keyed classes are found. A C1 site TRIAGE does not name (TRIAGE.md:109-110 counts ~63 and cites one); its `IndexMap` remedy gives insertion order, which is not Java's `HashSet` order, so the test plan JSON would move | Default bodies DON'T PORT. The Rust `resolution_universe` must have a deterministic iteration order (or none: the names slice reads only `contains`); the JudgmentFacts site must sort or take declaration order -- an ASK for testplan's slice (Java's order is a HashSet's, not reproducible) |
| E1. `isSubtype` default + private walk | 387-389, 420-451 | Dead: PMC:221 overrides `isSubtype(String,String)` (reach memo `Walked`, `ConcurrentHashMap` PMC:205-206); the private 3-arg walk is reached only from the overridden default | No | :388 `HashSet` visited, membership only | DON'T PORT (F's live relation is PMC:221-238, the slice ports that) |
| E2. `isDeclaredSubtype` | 391-418 | **The only default body that runs in production**: compiler/spec/UserCallInliner.java:926 (G½, `declaredSubtype`) | Yes: G½'s inlining choice | :396 `HashSet` visited, membership only. Supertype head extraction (:411-412) duplicates `protocol.TypeExpression.rawClassName` (TypeExpression.java:252-258) | PORT line for line as a `Model` method. Note: a THIRD subtype relation beside PMC:221 (compiled, Nil bottom) and KnowledgeLayer.isSubtype (KnowledgeLayer.java:116, ported in lite-names). It walks `findClassDefinition` = `knowledge().classDef` (PMC:299 -> TypeClassifier.java:77 -> KnowledgeLayer.java:82), which INCLUDES primitives' catalog nodes, where KnowledgeLayer.isSubtype uses `hierarchyClass` (KnowledgeLayer.java:91-97, primitives excluded). So it is not KnowledgeLayer.isSubtype: do not merge without asking |

**Notes.** Edges: `compiler.StoreLookups` (:2, names), `compiler.NameResolver.platformFqns()` (:56, names; dead default),
`platform.DeclarationTable/ImplementationTable` (:382-385, allowed by `compiler`'s line), `protocol.TypeExpression`
(:410-412). The `com.legend.setup.ProjectData` mention (:155) is javadoc, not an edge. No `compiler.spec` edge. No call
to any slice-1b-deferred model method (`findRuntime` returns `RuntimeDefinition`, :139, a type only). Comments: :9-11
"engine parity with `com.gs.legend.model.ModelContext`" (legacy); :420-425 "an inheritance CYCLE ... which the frontend
still accepts" (a known defect, DEEP_AUDIT §5b); the javadoc at :179-180 belongs to `mixedUnionMembers` and :367-373 to
`isSubtype` (both misplaced). Guard: LegacyReachbackCensusTest.java:96 pins one `findLegacyMapping` in this file (retires
with Java). 26 `default` methods; 25 overridden by PMC (every one but `isDeclaredSubtype`), so 25 default bodies
never run.

---

## ClassLayouts.java (277 lines)

| Class/family | Lines | Reached by | Reaches an answer | Idioms | Recommendation |
|---|---|---|---|---|---|
| `layoutOf`, `objectLayoutOf`, `layoutOfUncached`, `plainLayoutOf`, `layout`, `collect`, `substitute`, the carrier tests | 51-144, 168-276 | NOT E/F/G (no caller in compiler/element, compiler/spec, normalizer). Production: TypedQuery.java:173 (`lowerer`, reached from planner/ProductStatements.java:368 and planner/PlanMaker.java:599, the product plan's statements), Compiler.java:835 (`lowerResolved`), planner/BodyPlanner.java:189-190, planner/EnginePlans.java:170, lowering/SeedableLets.java:38 | Yes: a class value's SQL struct fields (order, types, `__id`/`__type`/`__canon`) -> the Lowerer's struct shapes -> SQL | `LayoutMemo` (:82-89): three `ConcurrentHashMap<Type, Optional<List<Column>>>`, open key (any `Type`, derived record equality), on the model via `ctx.derived` (:93, :135); get-then-put (:95-101, :136-143) because derivations nest (`objectLayoutOf` -> `layoutOf(...,true)` :140; `collect` -> `isSubtype` -> PMC's `Walked` memo). Membership only, never iterated: no hash order. Throws `IllegalStateException` (:117 "declares a property named __id", :251 "inherits conflicting declarations of property") -- swallowed as control flow where SeedableLets.java:42 `catch (RuntimeException notScalar)` wraps a Lowerer | PORT line for line, by the slice of its first caller, I (lowering), placed in `lite-lowering` (its deps -- `findClass`, `isSubtype`, `Type`, `PlatformTypes`, `EqualityKeys.fqnOf`, the sql constants -- are all within lowering's line). The memo's storage stays on `Model` as data (`Memo<Type, Option<Arc<[Column]>>>`, PHASE3_DESIGN.md §2.5's `layouts: LayoutMemos`) or goes per query (option C: the corpus 69 s -> 83 s cost is the reason it exists, :80; per-query recovery unmeasured) -- the D5 choice already recommended |
| `SYNTHETIC_ID`, `SYNTHETIC_TYPE`, `SYNTHETIC_CANON`, `syntheticOnlyLayout`, `objectSyntheticOnlyLayout` | 31-46, 146-166 | lowering (Lowerer.java:3528-3529, MixedEncoding.java:757, 810-943, CanonicalRenderSql.java:222-976, ConstructionCanon.java:22,37, InstanceEquality.java:349) | Yes: field names in SQL | `SYNTHETIC_ID/TYPE` are `sql.SqlExpr`'s (SqlExpr.java:38-43); lite-sql at TREE has only `TDS_NULL_CELL` (rust/lite-sql/src/mir.rs:117) | PORT with I; add `SYNTHETIC_ID`/`SYNTHETIC_TYPE` to lite-sql as slice 1a added `TDS_NULL_CELL`; `__canon` stays ClassLayouts' |

**Notes.** Fallback-looking: :172-174 a malformed parameterization returns empty ("let the caller stay loud"); :227
`findClass(superFqn).ifPresent` skips an unknown super silently; :271 `getOrDefault` leaves an unbound type variable in
place. :250 compares `Type` by derived `equals`. Misplaced javadoc: :56-73 is `layoutOf(…, withIdentity)`'s, sitting
above `LayoutMemo`; :194-210 stack three methods' docs. `substitute` (:269-276) is duplicated verbatim in
EqualityKeys.java:177-185. Tests: ClassLayoutsDiamondTest, LowererLetScopeTest; pct ValueBridge (test extension).

---

## EqualityKeys.java (245 lines)

| Family | Lines | Reached by | Reaches an answer | Idioms | Recommendation |
|---|---|---|---|---|---|
| `isKeyStereotype` | 72-77 | **F**: ClassCompiler.java:55 (stamps `Property.Stored.equalityKey`); also :99 here | Yes: the stored flag every equality reads | none | PORT in F |
| `fqnOf` | 58-70 | ClassLayouts:110 (I); lowering InstanceEquality.java:373,385, CanonicalRenderSql.java:87,144,461; testplan (HostJudge, JudgmentFacts, AssertVerdicts) | Yes (dispatch) | none; a pure function of `Type` | PORT; a `Type` helper -- could sit in lite-types |
| `resolve` (+ `typeArgsOf`, `substitute`, `collect`, record `EqualityKeys`/`Key`) | 33-56, 112-244 | Planner: TypedQuery.java:175, Compiler.java:837, planner/BodyPlanner.java:197 (`withInstanceKeys`); the record type is lowering's input (lowering/InstanceEquality.java:98-294, ConstructionCanon.java:34,59 take `Function<Type, EqualityKeys>`); testplan JudgmentFacts.java:90,92,123, AssertVerdicts.java:361-362 | Yes: in-SQL `eq`/`equal` key arms and canons -> SQL; key trees -> test plan (HostJson) | :140 cycle guard is the STRING `classFqn + typeArgs` (a `LinkedHashMap.toString` over record-derived `Type.toString`; Type overrides no `toString`), membership only (`LinkedHashSet` :115, :130). `seenNames`/`seenClasses` `LinkedHashSet`s, membership only | PORT with I/planner (record visible to lite-lowering). RE-EXPRESS the guard as a structural key `(fqn, Vec<(name, Type)>)`: Type's text is its derived record text, so text equality coincides with structural equality (barring a `]`/`,` inside an FQN), no answer moves. RE-EXPRESS `collect` as returning nothing: it never returns false (true at :201, :205, :243; :240 only propagates), so :147-149 and :240 are unreachable and the javadoc :195-196 "False = poisoned" is stale |
| `declaresKeys` | 79-110 | testplan only: JudgmentFacts.java:120 | Test plan key trees | :87 `HashSet` visited, membership only | PORT with testplan's slice (ported last, re-expressed) |

**Notes.** Fallback-looking: :164-166 a malformed or unknown parameterization -> no type args; :203-205 "unknown super
-- contributes nothing"; :233-234 an unsubstituted type variable -> slot class `Any`. :229-230 `findClass(declared)`
loads (compiles) the slot's class lazily. Edges: `protocol.TypeExpression.rawClassName` (:104), `model.ClassDefinition`,
`model.StereotypeApplication`. Test: SqlCanonConformanceTest. VerdictChannelRegisterTest.java:89 names it in a comment.

---

## RelationalOpRows.java (344 lines)

| Class | Lines | Reached by | Reaches an answer | Idioms | Recommendation |
|---|---|---|---|---|---|
| Row factories (`tableId`, `viewId`, `columnId`, `tableRow`, `viewRow`, `columnRow`, `aliasRow`), the builder (`dataType`, `kindOf`, `node`, `splitTable`) | 27-343 | NOT E/F/G. H: resolver/ConstructedInstances.java:93 (`new RelationalOpRows(ctx)`), resolver/ElementReferences.java:220 (`tableId`); planner: planner/OpSeeds.java:54, planner/MetamodelSeeds.java:469-635 | Yes: the metamodel store's `relational_elements` and `data_types` rows (ids like `"tbl:" + db + "|" + schema + "|" + name`, :51-61) -> the plan's seed steps -> SQL data -> metamodel query answers | :258 `String.valueOf(l.value())` spells a `Long`/`Double` literal (RelationalOperation.java:231-233) in Java's text (`1.0E10`): needs lite-javacompat's double spelling. :232, :300 `findDatabase(..).orElse(scopeDb)` (fallback to scope). :250-255 `IllegalStateException` whose text embeds `lam.parameters()` (a `List.toString`). `typeIds` `LinkedHashSet` (:134), membership. No hash collection | PORT line for line with H (first caller), placed in `lite-resolver` (planner reaches resolver) or `lite-names` (it needs only `StoreLookups.findDatabase` + RelationalTypeInference, not `ModelContext`) |

**Notes.** Calls `LogicalOp.keyword()` (:279; ported, rust/lite-model/src/names.rs:110). No slice-1b-deferred model
method. :304 `Group` arm "unreachable" (handled at :213). Doc :248-249 "relational lambdas ... refused loudly here ...
(4.145.0 construct; a lowering leg)": a named gap.

---

## RelationalTypeInference.java (289 lines)

| Class | Lines | Reached by | Reaches an answer | Idioms | Recommendation |
|---|---|---|---|---|---|
| `infer`, `safe`, `decimalOf`, `columnType`, `INT_WIDENING` | 31-288 | **E**, production: compiler/KnowledgeLayer.java:406 (`columnKind`'s computed-view-column arm) <- normalizer/DeclaredCoercions.java:147, normalizer/UnionSynthesis.java:968,980 (<- MappingNormalizer); and ViewSignatures.java:136 <- normalizer/LiftedViews.java:99. Also RelationalOpRows.java:306 (H, planner) | Yes: a column's kind -> a synthesized read's coercion (DeclaredCoercions), a union key thread's kind (`KeyThread`, UnionSynthesis:967-982 -> `unionKeyThreads`), a view function's declared columns; and the seeds' `data_types` rows | :150 `str.length()` = UTF-16 units -> `VARCHAR(n)`; :47 `toLowerCase(Locale.ROOT)`; :264 `ctx.findDatabase(opDb).orElse(db)` and :139-143 (documented :254-256: an unqualified legacy `[db]` falls to the scope); :128, :154, :165 `null` = "no inferable type" (spec'd as the engine's `failOnMatchFailure=false`, :26-29); :230 null where the engine asserts. No hash, no exception | PORT line for line, **into lite-names** (move out of compiler/element): it reads only `StoreLookups` (StoreLookups.java:17-31; ModelBuilder implements it, ModelBuilder.java:97; lite-names has the trait, rust/lite-names/src/model_index.rs:1195-1199) and the model. That finishes `KnowledgeLayer.columnKind`, which the names slice deferred ("Left for F's slice: `columnKind`, whose computed-view-column arm asks F's `RelationalTypeInference`", rust/lite-names/src/knowledge.rs:20-21; lib.rs:19), and deletes PHASE3_CRATES.md §2's names -> element edge |

**Notes.** Stale doc :22-24 ("the walk keeps a delegating shim for the constructed-instance tests") -- MetamodelWalk is
"GONE" (LegacyReachbackCensusTest.java comment, "group F burn, 2026-09-02"). :226-230 names "an earlier fallback"
removed. No tests in core/src/test reference it.

---

## ViewSignatures.java (148 lines)

| Class | Lines | Reached by | Reaches an answer | Idioms | Recommendation |
|---|---|---|---|---|---|
| `of`, `column`, `inferred`, `pureType`, `any` | 44-147 | **E** only: normalizer/LiftedViews.java:99 (`ViewSignatures.of(model, …)`, `model` a `ModelBuilder`, LiftedViews.java:48) | Yes: the lifted view function's declared return type (spelled by LiftedViews.spell, :121-139) -> G checks the body against it -> types and plans; on a cycle, a model wall's text | :51 identity set (`newSetFromMap(IdentityHashMap)`): step 0 row (b) `model.ViewSignatures.expanding` **0/137** -- the oracle cannot catch a mistake here. :57-61 `ModelException(Phase.MODEL, "view '…' of '…' reads its own columns (cyclic view-on-view chain)")`: this is the wall that fires on a view cycle (ViewSignatures.of runs at LiftedViews:98-99 before `viewRelationExpr` and recurses on itself, :96), caught at LiftedViews.java:109 into `walls` -> `compileErrors`. LiftedViews' own NORMALIZE-phase cycle text (:90-93) guards a different recursion. Both 0 hits: reproduce phase and text by reading | PORT line for line with E, placed in lite-names beside RelationalTypeInference (it needs `StoreLookups`, lite-types, lite-error, and `StoreCompiler.columnType`, which moves with it) or in lite-normalizer. Identity set -> `lite_base::identity::IdSet` (two equal views in two schemas are two views) |

**Notes.** Fallback-looking: :102 a column reference to an unknown table/column -> `Any[0..1]` (invariant 4); :137 no
inferred type -> `Any` (documented :35-37 as the engine's own rule having none).

---

## Temporal.java (141 lines) -- which methods are F's? None.

| Method | Lines | Reached by | Reaches an answer | Idioms | Recommendation |
|---|---|---|---|---|---|
| `strategyOf` | 111-140 | G: compiler/spec/Typer.java:617, :1463, GraphFetchChecker.java:175; H: resolver/GraphEmission.java:1236,1242, StoreResolver.java:2569, TemporalFrame.java:2008, RelationalRootForm.java:102; testplan/BodyClass.java:1093. No E/F caller | Yes: milestoning typing, temporal filters in SQL | :119 `HashSet` seen, membership only; BFS order from `superClassFqns` lists (deterministic). :137 `findClass` loads the class; an unknown one is skipped | PORT with G (lite-spec, or lite-element as a `Model` helper) |
| `generatedMember` | 10-53 | G: Typer.java:1487, GraphFetchChecker.java:187 | Yes: types of `businessDate`, `milestoning`, its members | `strat == PROCESSING` enum identity (fine); BITEMPORAL -> `BusinessDateMilestoning` (:33-35) | PORT with G |
| `isGeneratedDateProperty` / `isGeneratedDateName` | 58-76 | H: RelationalRootForm.java:111 / lineage: ScanRelations.java:1402, 2693 | Yes | none | PORT with H / lineage |
| `anyTemporalGetAll` | 78-109 | Planner: Compiler.java:825 (`lowerResolved`), planner/EnginePlans.java:160, planner/ProductStatements.java:140 | Yes: false -> `Lowerer.withEngineExistsJoinForm()` -> SQL shape | Walks `TypedSpec`/`TypedGetAll` (:87, :98-99): **the element -> spec edge** of PHASE3_CRATES.md §2 | PORT with the planner (lite-planner, or lite-spec); must not land in lite-element |

**Notes.** E answers "is temporal" separately: normalizer/MilestoningFacts.java:20-29 folds over
`knowledge().lineage` (declarations), while `strategyOf` walks compiled classes (`findClass`). Two implementations of
one question. Misplaced javadoc :58-64 (two methods' docs stacked).

## MilestoningStrategy.java (51 lines)

| Class | Lines | Reached by | Reaches an answer | Idioms | Recommendation |
|---|---|---|---|---|---|
| enum, `ofStereotypeOrNull`, `has`, `of` | 11-50 | **E**: normalizer/MilestoningFacts.java:23 (<- JoinChainEmission.java:898); G: Typer; H: GraphEmission, RelationalRootForm, StoreResolver, TemporalContext, TemporalFrame; Temporal | Yes: E's join-chain emission; G/H temporal behaviour | :34 `default -> null` for an unknown `temporal.*` stereotype ("matching the historical set-membership tolerance", :20-21): a leniency (invariant 4) | PORT with E (lite-names or lite-element; E reaches both) |

---

## StoreCompiler.java (120 lines)

| Method | Lines | Reached by | Reaches an answer | Idioms | Recommendation |
|---|---|---|---|---|---|
| `tableSchema` (package-private) | 29-42 | **F**: PMC:739 (`findTable`) | Yes: a table's relation type -> G | none | PORT in F |
| `columnType` (package-private) | 44-82 | F (`tableSchema`) and E (ViewSignatures.java:141) | Yes | exhaustive switch, no default | PORT; moves with ViewSignatures if that goes to lite-names (lite-types + lite-model only) |
| `declaredType` | 84-119 | **G**: compiler/spec/TableReferenceChecker.java:135 (-> `TypedTableReference.storedTypes` -> the lowering stamps a scan's stored types); setup/Ddl.java:87, :516; testplan/EffectPlanner.java:143. No F caller | Yes: DDL text and a scan's stored types in the plan | **It spells SQL**: `Sized("VARCHAR"/"CHAR"/"BINARY"/"VARBINARY", n)`, `Scaled("DECIMAL"/"NUMERIC", p, s)` (:108-113) are written verbatim by every dialect (sql/dialect/DdlSpelling.java:21-28) and carried as `ColumnType::Sized { kind: String }` in lite-sql (rust/lite-sql/src/stmt.rs:14-15, read.rs:769-773: the plan JSON) | PORT with G (its strings as is). Making `kind` an enum would change lite-sql and the plan seam: ASK the owner, not this slice |

**Notes.** Unused `import com.legend.builtin.Pure` (:3). Edge to `sql.SqlDdl` (allowed: compiler reaches sql). Test:
StoreCompilerTypesTest.

---

## The Typed* records

| Class | Lines | Reached by | Reaches an answer | Idioms | Recommendation |
|---|---|---|---|---|---|
| TypedFunction | 129 | **F** builds it: FunctionCompiler.java:110 (10-arg form, `definition` and `FunctionId.of(f)` always set); also SignatureApart.java:67 (G), ObjectReferenceArms.java:69 (H). Read by 55 main files | Yes: every call's callee, signature, body | Record-derived `equals`/`hashCode` over all ten fields, body (`ValueSpecification`s) and `definition` included, **used**: TypedNativeCall.java:42,49 and TypedUserCall.java:73 compare/hash `callee`. Identity **also used**: SpecCompiler.java:48 `IdentityHashMap<TypedFunction, CompiledFunction>` (step 0 class (a), 1,582,730/240,191), relying on F's cache returning one instance per overload. `id()` throws `IllegalStateException` when null (:66-73) | PORT: `Arc<TypedFunction>` from the cache with `lite_base::identity` for SpecCompiler's memo, AND derived `PartialEq`/`Hash` over every field. Equality by `id` alone is WRONG: SignatureApart.java:67-72 builds a renamed copy with the same `id` and `definition`. RE-EXPRESS the constructors: the 8- and 9-arg forms (:81-99) and a null `id` are test-only (no main caller), so `definition`/`id` can be non-optional (SignatureApart:72's null check then goes) |
| Property (Stored, Derived) | 103 | F builds (ClassCompiler.java:51-77); G, H, planner, testplan read. The model/builtin/protocol hits in callers.txt are word-match noise: model/ClassDefinition.java:125-127 is the text "Property name cannot be null", protocol/cypher/CypherToPure.java:17 is `AppliedProperty`; no file under model/, builtin/ or protocol/ names `compiler.element` | Yes | derived equality; no map or set keyed by it in main | PORT |
| TypedClass | 47 | F (ClassCompiler.java:95-97); G, H, planner | Yes | null -> `List.of()` in the canonical ctor; derived equality, no map key | PORT |
| TypedEnum, TypedParameter | 28, 23 | F; G (LiteralUnroll), H | Yes | none | PORT |
| TypedConstraint | 49 | F (ClassCompiler.java:84-93); H reads `name`, `predicateFqn`, `messageFqn`, `level` (resolver/GraphEmission.java:599-613) | Yes (constraint checks in graph fetch) | `externalId` is always `Optional.empty()` (ClassCompiler.java:85) and never read (the `.externalId()` hits in main are protocol's `ConstraintDefinition`/`DomainComposer`/`ProtocolEmitter`); `TypedConstraint.of` (:45) has no main caller. ClassCompiler.java:86-88 maps any level but `"Warn"` (case-insensitive, null included) to ERROR: a default in F (ClassCompiler's, not this record's) | PORT; RE-EXPRESS without `externalId` and `of` (nothing reads them, so no answer moves) |
| TypedNominal, TypedElement | 27, 21 | Nothing: no switch, parameter or field of either type in core, spec, pct, wasm (only the `permits`/`implements` lines) | No | sealed markers | DON'T PORT (nothing replaces them; each record is its own struct) |

No main `Map`/`Set` is keyed by TypedClass, Property, TypedEnum, TypedParameter, TypedConstraint or EqualityKeys (grep of
`Map<…`/`Set<…` declarations); SpecCompiler.java:48 is the one TypedFunction-keyed map.

---

## compiler/element/type (Type 759, PlatformTypes 816, Multiplicity 243, ExprType 21)

Ported as rust/lite-types (slice 1a, PHASE3_CRATES.md §4). Of the five left: **F needs two** -- `Multiplicity.from`
(F: TypeClassifier.java:315; G: 13 sites in Typer, Overloads, MatchChecker, TypeAnnotations, EvalChecker, SpecCompiler)
and `PlatformTypes.eraseTdsRow` (F: TypeClassifier.java:104; G: TypeAnnotations.java:160); both can now join lite-types
(lite-front, lite-builtin, lite-model exist). `handleRowClass` is not F's (planner/PlanAllocations.java:394,
resolver/ElementReferences.java:287, resolver/StoreResolver.java:1544, testplan/BodyClass.java:693); `isStatementOnly` is
not F's (compiler/StatementInline.java:293, the query door's pass the names slice deferred). Subtyping: F's relation is
PMC:221-238 with its `Walked` reach memo (PMC:205-206), not `ModelContext.isSubtype`'s default (dead, above);
`ModelContext.isDeclaredSubtype` (G½) and `KnowledgeLayer.isSubtype` (E, already in lite-names) are the other two.
