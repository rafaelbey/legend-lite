# Relational Feature & Architecture Map — legend-engine vs legend-lite

**Status: the engine-side survey of legend-engine's `core_relational`, kept as spec input for the port.** Written
2026-07 (all three survey waves merged 2026-07-23). For every feature area it records the engine's semantic model, with
source refs, and the behavioral contracts the engine's tests pin. Lite's deliberate differences are rows of
`docs/SEMANTICS_REGISTER.md`.

> Cut in the 2026-11 docs cleanup: lite's side of the map as of 2026-07: the verdict legend (SOUND/PATCHWORK/MISSING/DEFERRED) and the purpose note of 2026-07-04; each later cut site names what it held. Full text: `git show de2745351:docs/RELATIONAL_FEATURE_MAP.md`.

Engine source base (local checkout, all refs relative unless noted):
`legend-engine/legend-engine-xts-relationalStore/legend-engine-xt-relationalStore-generation/legend-engine-xt-relationalStore-pure/legend-engine-xt-relationalStore-core-pure/src/main/resources/core_relational/relational/`

---

## 0. The two architectures at a glance

### 0.1 Engine (legend-engine core_relational)

One 11k-line translator (`pureToSQLQuery/pureToSQLQuery.pure`) plus satellite files.
The semantic heart is ~18k lines of ~82k total:

| File | Lines | Owns |
|---|---|---|
| pureToSQLQuery/pureToSQLQuery.pure | 11,097 | the whole query translator: per-op processing, join trees, isolation, qualifiers, PMs |
| relationalExtension.pure | 2,022 | dyna-function → SQL registry (the scalar function catalog) |
| pureToSQLQuery_union.pure | 1,384 | union (Operation set) processing |
| testDataGeneration.pure | 1,366 | test data generation from mappings |
| dbExtension.pure + per-DB files | 1,308+ | dialect abstraction (DDL/DML/type mapping per DB) |
| relationalGraphFetch.pure | ~1,000 | graphFetch → SQL batching/temp-table model |
| milestoning/milestoning.pure | 938 | temporal context calculus |
| executionPlan/executionPlan.pure | ~900 | plan-node generation (allocation, TDS result, activities) |

Key architectural facts (established §A/§B of the H2/H3 plan, reconfirmed this survey):
- Per-column **select threads** carrying join trees, merged by **join-name identity** (`mergeSQLQueryData`); leanness comes from merge-by-identity, not from a pruning pass.
- **Isolation strategies** (MoveFilterInOnClause / BuildCorrelatedSubQuery / MoveFilterOnTop) decide nesting prophylactically; there is no post-hoc flattener.
- **No FK-shortcut, no join cancellation, no distinct-removal** — leanness is join dedup + prophylactic flatness only.
- Post-processor pipeline: CTE hoisting, pushFiltersDownToJoins, removeUnionOrJoins (Snowflake), replaceAliasName, prependSQLComments.
- One **milestoning context per query cursor**, rebuilt at explicit-date hops, propagated through same-strategy temporal parents, cleared at non-temporal hops.


> Cut in the 2026-11 docs cleanup: §0.2–0.3, lite's pipeline and layers (2026-07) and the corpus status per family at HEAD 15cead69. Full text: `git show de2745351:docs/RELATIONAL_FEATURE_MAP.md`.

---

## 1. Feature taxonomy (engine directory structure = the feature list)

Survey waves; every area gets FULL depth:

- **Wave 1 (known-divergent):** query-translator core + isolation; milestoning; unions; views + embedded/inline/otherwise.
- **Wave 2:** graphFetch (H4); qualifiers/derived properties + auto-map; dyna-functions + dialect architecture; mapping metamodel (incl. inheritance/extends, modelJoins, multigrain, merge, include).
- **Wave 3:** aggregationAware + calendarAggregation; execution plans/TDS/result contract; postprocessors; tooling (lineage, testDataGeneration, mutation, autogeneration, constraints, functionActivation).

Sections 2+ below are filled per wave.

---

## 2. Query-translator core (joins, isolation, per-op processing)

### 2.1 Engine model (pureToSQLQuery.pure, 11,097 lines; all L-refs to that file)

**The machine.** Entry `toSQLQuery` (L215): `processQuery → applyPostProcessingForTempTableAsDriver
→ manageDeepMapClassColumns → .select → pushSavedFilteringOperation → pushExtraFilteringOperation
→ orderImmediateChildNodeByJoinAliasDependencies`. Three SELECT shapes: `SelectSQLQuery`
(base; columns, data = RootJoinTreeNode, filteringOperation, **savedFilteringOperation**
(node-keyed deferred filters), **extraFilteringOperation** (merge-later, deduped),
leftSideOfFilter, groupBy/orderBy/having/qualify/pivot/distinct/fromRow/toRow/CTEs),
`TdsSelectSqlQuery` (adds `paths`: TDS col → PathInformation(type, relationalType, PM,
doc)), `ViewSelectSQLQuery` (View + expanded select, L5642).

**Join tree.** `RootJoinTreeNode` (driver) + `JoinTreeNode` (alias, join, joinName,
joinType, lateral). Joins store BOTH directional alias pairs `[(src,tgt),(tgt,src)]`
(L2381, L9883) — target resolution (`otherTable`/`findTarget`) depends on it. **Node
identity across rewrites is by re-search, not reference**: `findNode`/`findNodeByChild`
(L5979–6014) re-locate "the same" node by join-name match → child index → `__iso` prefix
heuristics; `replaceTreeNodeAndUpdatePointersInSwc` (L1675) re-points all five live
pointers atomically; `validate` (L479) asserts pointer liveness after nearly every rewrite.

**Carriers.** `SelectWithCursor` (L67): parent (for exists/in scope resolution), select,
currentTreeNode ("where we are"; navigation joins hang off it),
positionBeforeLastApplyJoinTreeNode (exists/in cut point), savedRoot, milestoningContext.
`State` (L82): immutable, threaded; ~30 flags — the load-bearing ones: **`inFilter`** (the
master switch — every leaf writes to filteringOperation vs columns on it, ~80 sites),
`inProject`/`inProjectFunctions`/`processingProjectionThread` (steer isolation strategy),
`shouldIsolate`/`shouldIsolateNestedFilter`/`filterChainDepth`, `qualifierBase`/
`functionReferenceScope` (qualifier machinery), `functionExpressionStack` (detects
"last operation", restrict-distinct adjacency, paginated position). `FunctionParamScope`
(L128): lambda/let var → the SWC recorded at lambda entry.

**Per-op processing** (each handler `(fe, PM[*], SWC, vars, State, JoinType, nodeId, aggFromMap, …) → element`):
- getAll `processGetAll` (L5440): per-set dispatch (Root → processRelationalMappingSpecification
  L5556; RelationFunction; Cross → placeholder; Operation(union) → buildUnion). Builds root
  node, resolves all datatype columns, injects `pk_<n>` **only when addPk && no groupBy &&
  not distinct** (L5580–5596), applies mapping ~filter/groupBy, milestoning at L5631.
  `inProject` suppresses fetch-all-properties (L5451).
- filter `processFilter` (L5824): processes left (filterChainDepth++), rootSelect carries
  only applicable saved filters, binds lambda param, predicate with inFilter=true, merge,
  collapse WHERE to single AND, then channel decision (L5897–5914) and conditional
  `manageIsolation` (L5943).
- project `processProject` (L7975): asserts DataType returns; source with inProject=true;
  `removeColumnsAndJoins`; `addPkForAggregation`; each projection fn = own thread with
  `processingProjectionThread=true`, isolated via manageIsolation (L8018), then merged;
  emits TdsSelectSqlQuery with paths.
- groupBy `processGroupBy` (L6710): noSubSelect decision (L6721 — no existing
  groupBy/pivot/distinct/orderBy-outside-groupcols); flat or wrapped-subselect emission;
  always through `isolateNonTerminalGroupByQueryWithEmptyGroupingColumns` (L851 — empty
  grouping cols + non-terminal ⇒ wrap, keyed on functionExpressionStack size > 1).
- sortBy (L3349): lambda arm computes a `generated_order_key` column, merges, moves the
  expression to orderBy and drops the temp column.
- take/slice/drop/paginated (L3517/L3423/L4245/L3480): set fromRow/toRow then
  `isolateSubSelectIfNotLastOperation` (L3465 — wrap iff a following TDS op exists on the
  functionExpressionStack). paginated asserts sorted-first and last-position.
- restrict `processTdsRestrict` (L7374): restrict-then-distinct over a project-of-basic-cols
  triggers `processTdsRestrictOptimized` — rewrites into a narrower project, CUTTING joins
  for unprojected columns; else subselect only if un-projected groupBy/sortBy cols or
  distinct (L7464).
- TDS/relation join (L7511): both sides materialized as aliases, dup-column assert, ON
  lambda → Operation (boolean literal coerced to 1=1/1=0), one JoinTreeNode, re-alias all
  columns to the joined alias.
- pivot (L6795): always subselect-wrapped; dynamic pivot with >1 agg = UnionAll-of-groupBys;
  forces `select *` (result columns data-dependent).
- window: three arrival paths (legacy TDS WindowColumn; relation `_Window`; first/last/
  offset/nth inside map, L3980) → `Window(partition, sortBy, frame)`; distinct isolates
  first (L3701); isolate-if-not-last unless in extend (L3744).
- exists/isEmpty (L6016/L4432): isEmpty branches on raw type — DataType → null/size check,
  Class → exists. Two emission forms: `buildExistsPredicate` (correlated EXISTS, when alias
  wraps Select/Union with single child, L6149) else `buildExistsAsJoinWithNullCheck`
  (LEFT JOIN + IS [NOT] NULL, L6165).
- in/contains (L6351): literal collection → in/equal with enum source-value mapping;
  property-collection → EXISTS subquery (L6531).
- concatenate (L2776): flattens nested; no-data ⇒ top-level Union; else per-arm subselects
  grafted as a Union node; `alignJoinAndPkColumnsForUnion` (L2978) NULL-pads to a shared
  column list.
- CTE: leading `let`s become CommonTableExpressions (L2646–2732).

**Join & navigation.** Every projection fn / filter operand / property is its OWN select
thread (columns + the join path it needs); threads combine via `mergeSQLQueryData` (L9424)
/ `merge` (L9301): match children **by join.name equality** → absorb + remap aliases both
directions; no match → reprocess (unique re-alias) + append. `applyJoinInTree` (L9543)
grafts join chains; explode-in-ON routes to lateral-flatten emission (L9643); joinName
collision at siblings ⇒ suffix with `buildUniqueName` of the operation.
`orderImmediateChildNodeByJoinAliasDependencies` (L365) topologically sorts sibling joins
by alias cross-reference. Navigation default LEFT_OUTER (L1510/1518); INNER only from
INNER mapping-filter subselect wrap (L1765) or user TDS join kind. Distinct/groupBy target
classes re-materialize as subselects (L1785). `buildUniqueName` (L8527) is the canonical
structural fingerprint for join naming AND filter/column dedup (MD5-truncated >256 chars —
correctness, not cosmetics).

**Isolation model** (the part we never modeled). Driven entirely by
savedFilteringOperation + INNER joins. `manageIsolation` (L8186) fires iff
`shouldIsolate && (tree-has-children && containsInnerJoin || savedFilters nonempty)`.
`isolateSubJoins` (L8346): `findBestNodeToIsolate` (L8449 — deepest node common to all
root→leaf threads referencing the filter's aliases; backs off from INNER leaves;
reconciles with leftSideOfFilter), then strategy (L8377):
- projection thread: immediate-child found → **MoveFilterInOnClause**, else
  **BuildCorrelatedSubQuery**;
- filter context: leaf + shouldIsolateNestedFilter (or inIfTrueFalseStmt) →
  MoveFilterInOnClause; else **MoveFilterOnTop**;
- OVERRIDE: MoveFilterOnTop upgrades to BuildCorrelatedSubQuery whenever the root
  containsInnerJoin (L8393).
MoveFilterOnTop (L1138): filters → top-level extraFilteringOperation + join rename
(un-shares the join). MoveFilterInOnClause (L1098): fold each saved filter into the ON of
the node carrying its alias. BuildCorrelatedSubQuery (L1170): wrap found node in nested
SELECT, partition filters inside/outside, pull needed columns up with unique renaming,
re-point join at the `csq` subselect. Five OTHER nesting triggers: isolate-if-not-last
(take/limit/slice/drop/window), pivot always, empty-groupcol groupBy non-terminal,
distinct-source project/window/rowCount (`moveSelectQueryToSubSelect` L8600), filter over
aggregated/window TDS (WHERE vs HAVING vs QUALIFY routing, L3782–3799).

**Qualifier/derived-property model.** `processQualifiedPropertyFunctionExpression` (L952):
process left side, detect aggregation in body (pre-add PKs, L982), then
`processQualifiedProperty` (L1048): bind qualifier params into FunctionParamScope, process
the single body expression with **shouldIsolate=false** (filters accumulate as saved; the
CALLER isolates), set parent/positionBefore so enclosing exists/in cuts correctly. `$this`
= qualifierBase with filter cleared (L435). Param values re-process against the state
recorded at qualifier entry (L463) — how `$f.employeesByAddress($f.address)` binds.
Auto-map `processMap` (L3978): isolate source, PKs if body aggregates, dispatch body
(property/qualifier/lambda/path).

**Key invariants** (full list in agent archive; the load-bearing ones):
- inFilter dichotomy at every leaf; `or`/`not` move extraFilters into filter first (L5334, L3267).
- Enum translation is POSITION-dependent: filter/comparison → source values (in-list)
  (L8800–8850); projection (+flag) → inlined `case` (L5679); `if`-predicate arg disables
  enum pushdown (L5320); enum-returning `if` requires one transformer across branches (L4567).
- `==` requires both sides [0..1] — else "use exists" failure (L8675).
- pk_ names are matched downstream BY NAME PATTERN (self-join L4364, temp-table L8133).
- `__iso_`/`csq`/`_ecq`/`gen_` alias prefixes are semantic (merge/find/self-join key off them).
- Column-name comparisons must be quote-insensitive (L7359–7369).
- `extractColumnTypeDefaultToInt` (L1376): inference failure defaults the column type to
  Integer (legacy-compat, load-bearing).
- Dispatch: `getSupportedFunctions` (L10650–11097) + context-guarded
  `getContextBasedSupportedFunctions` (L10348, checked first) — the full inventory: TDS ops,
  ~40 string fns, ~50 math, ~40 date/time incl. calendar aggs, hash/regexp, ~40 variant
  fns, window family with frames, special modes (import-data-flow FK materialization,
  temp-table-as-driver, CTEs, model joins, cross-store placeholders).

> Cut in the 2026-11 docs cleanup: §2.2–2.3, lite's side and verdict for the query-translator core, and §2a, the general mapping→SQL path compared end to end with its verdict (2026-07). Full text: `git show de2745351:docs/RELATIONAL_FEATURE_MAP.md`.

## 2b. Multi-hop navigation: the chain algebra

The recurring pain point, given its own verdict. Multi-hop = the composition of hops
(`$x.a.b.c`, filtered hops, to-many mid-chain, aggregates over chains, class-flatten
hops, qualifier hops) — historically our densest source of bespoke blocks.

### 2b.1 The engine's coherence — a 4-mechanism algebra (model coherent, machinery fragile)

1. **One hop function + cursor invariant**: every hop folds through
   processProperty → processPropertyMapping → doJoinToClass; EVERY feature (embedded,
   otherwise, union route, milestoned, view, distinct target) is an arm inside that
   per-hop dispatch, and every arm restores the same invariant — the cursor points at
   the hop's target frame. Chains compose across features because arms compose under
   one invariant.
2. **One sharing mechanism**: merge-by-join-name over per-column threads (prefix sharing).
3. **One cardinality mechanism**: filter channels + isolation chooser (to-many-in-filter
   → EXISTS; aggregate-over-chain → PK injection + grouped rejoin; below-hop filters →
   saved then placed by strategy).
4. **One target rule**: distinct/groupBy/complex targets re-materialize as subselects.

The engine's fragility is in HOW the invariant is maintained (live-pointer re-search,
`__iso` prefixes, qualifier re-entry through recorded state) — and its own test.ToFix
wrong-goldens cluster exactly at multi-hop × qualifier × isolation (P:188, ADV:237).
Even the reference strains at that intersection.

> Cut in the 2026-11 docs cleanup: §2b.2–2b.3, lite's multi-hop state and its proposed `Hop` algebra (2026-07). Full text: `git show de2745351:docs/RELATIONAL_FEATURE_MAP.md`.

## 3. Milestoning (temporal)

### 3.1 Engine model (milestoning.pure "M", PSQL = pureToSQLQuery.pure)

**The context object** — `TemporalMilestoningContext` (M L51–91), ONE per query cursor
(`SelectWithCursor.milestoningContext`):
- `processingDate` / `businessDate` : **RelationalOperationElement** [0..1] — resolved
  dates held as SQL elements (Literal, TableAliasColumn, or DynaFunction), NOT Pure dates.
  Dedup/equality by rendered form (`buildUniqueName`), never object identity.
- `currentMilestoningStrategy`: BusinessTemporal | ProcessingTemporal | BiTemporal
  (BiTemporal expands to [Business, Processing], M L820).
- `currentProcessingState`: an **8-value hop classifier** (M L34–43):
  MILESTONED_ALL_FUNC, MILESTONED_CLASS_PROPERTY, MILESTONED_CLASS_PROPERTY_NO_ARG,
  NON_MILESTONED_CLASS_PROPERTY, DATATYPE_PROPERTY, ALL_VERSIONS, ALL_VERSIONS_IN_RANGE,
  ALL_FOR_EACH_DATE. The pivotal predicate `currentProcessingStateIsMilestonedClassProperty`
  (M L86) = {MILESTONED_CLASS_PROPERTY, _NO_ARG, ALL_VERSIONS_IN_RANGE} — note
  ALL_VERSIONS is NOT in it. This gates intermediate-join filters.
- `startDate`/`endDate` for allVersionsInRange only.

**Context lifecycle.**
- **Seeded at root** per all() arity (M L830–844): 0 dates → allVersions; 1 →
  MILESTONED_ALL_FUNC; 2 → range (single-strategy) or bitemporal both-slots;
  getAllForEachDate special-cased.
- **Rebuilt at explicit-date hops**: `getMilestoningContextForQualifiedProperty`
  (M L846–868) — a generated-milestoned QP with date args builds a NEW context from its
  return-type class's stereotype; a no-arg milestoned QP INHERITS the incoming date
  (fails loudly outside a milestoned context, M L862); a NON-milestoned QP passes the
  incoming context through unchanged (M L855).
- **Explicit beats propagated**: an explicit `classification(%2015-10-17)` keeps its date
  while the root keeps its own — no override in either direction (goldens :282/:316).
  Two different dates on one chain = **two cloned table aliases** (`…_0`, `…_1`) each with
  its own filter — never two contexts (:290/:334).
- **Cleared at non-temporal hops** — but only for non-generated-milestoning properties
  (PSQL:2036–2041): temporal→non-temporal→temporal means the second temporal class must
  re-supply a date or use an edge-point property (`AllVersions`), which keep flowing.
- After each QP hop, `currentProcessingState` is reset to [] on the surviving context
  (M L552–556); final choice (PSQL:1002–1004): generated-milestoned QP → propagate
  realized columns; temporal return type → keep child context; else clear.

**Table-level model.** Milestoning blocks on tables: `business(BUS_FROM=from_z,
BUS_THRU=thru_z [,THRU_IS_INCLUSIVE][,INFINITY_DATE])`, `processing(in_z/out_z/
OUT_IS_INCLUSIVE/INFINITY_DATE)`, snapshot variants (`BUS_SNAPSHOT_DATE=col`).
Bitemporal = both blocks. Class-stereotype ↔ table-block matcher
`milestoningCanSupportTemporalStrategy` (M L743): business-temporal class may map to
BusinessMilestoning OR BusinessSnapshotMilestoning tables; Union supports a strategy via
member-table fold.

**Predicate builders** (each a distinct convention — do not conflate):
- default (exclusive thru): `from <= d AND thru > d`; inclusive flag flips to
  `from < d AND thru >= d` (M L403–410); flag consistency asserted across union members.
- range (allVersionsInRange): `from <= END AND thru > START` — a DIFFERENT pair (M L412).
- snapshot: EQUALITY `snapshotDate = d`, with datePart coercion for DATE col vs timestamp
  (M L387–401); `%latest` illegal for snapshot and for ranges.
- `%latest`: `thru = INFINITY_DATE`; requires declared INFINITY_DATE (hard fail M L608),
  identical across union members (M L612).
- allVersions: NO filter at all; `k_businessDate` = the inclusive boundary column.
- union: per-member **coalesce across member slots OR isNull(coalesce(all))** admitting
  non-milestoned members (M L349–385) — matches §4's union NULL discipline.
- bitemporal: 4-predicate AND (in_z/out_z + from_z/thru_z), k_processingDate then
  k_businessDate.

**Injection sites** (the processing algorithm):
1. Root scan: `applyMilestoningTypeFilters` (PSQL:5652) — guarded by `!allVersions &&
   !forEachDate && relationalElementIsMilestoned`; walks the tree
   (`applyMilestoningFilters` M L106–184: WHERE on SelectSQLQuery, **ON-clause AND-fold on
   JoinTreeNode**); appends synthetic `k_*Date` columns (only when class objects are
   returned, not TDS); resets state.
2. Navigation joins: filter lands in the LEFT JOIN ON of the milestoned target.
3. **Intermediate pass-through joins**: `getAppliedJoinMilestoningFilters` (M L542–550) —
   filtered when union-chained, or milestoned/non-milestoned-class-property state WITH
   children, or datatype-property terminus. Inside `applyJoinInTreeDeep` (PSQL:9567):
   isolation into a subselect when no inner join present, else pushed into the inner
   subselect's WHERE (goldens :175/:186/:195).
4. exists/in: predicate appears BOTH inside the correlated subquery and on the outer root.
5. Milestoned `==` equality is its OWN code path (PSQL:8684, M L230–261), incl. bitemporal
   top-level-`and` suppression — not "navigate then compare".

**Non-literal dates** — `resolveMilestoningDateParams` (M L638–681) dispatches:
variables (inScopeVars → Literal or plan VarPlaceHolder); **row-read dates**
(`$o.orderDate` → re-processed into a TableAliasColumn: `from_z <= "root".orderDate`);
function calls (reactivate if constant-foldable, else DynaFunction:
`from_z <= dateadd(day,-1, t.settlementDate)`); `$this.*Date` (re-entrant
TemporalMilestoningThisContext rewriting, M L45–66/683–704); getAllForEachDate calendar
column (`from_z <= calendar.calendar_date`). Router wrappers must be bypassed before
inspection.

**Full trap list, invariants, and 15 golden contracts:** archived in the agent report;
the load-bearing ones are reproduced above.

> Cut in the 2026-11 docs cleanup: §3.2–3.3, lite's side and verdict for milestoning, with its redesign leg (2026-07). Full text: `git show de2745351:docs/RELATIONAL_FEATURE_MAP.md`.

## 4. Unions (Operation set implementations)

### 4.1 Engine model (pureToSQLQuery_union.pure "U", union branches of pureToSQLQuery.pure "P")

**The model.** A union class mapping = `OperationSetImplementation` over N member sets;
member ordinal `i` = `resolveOperation` order and is load-bearing everywhere. At query
time a union is a **UNION ALL subselect** with a strict column discipline:

- **Driver getAll** (P L5463–5476): union → `buildUnion`, wrapped in a root alias literally
  named **`unionBase`**; outer select re-projects `u_type` + arm-0's value columns.
  Single surviving member ⇒ NO union node at all (degeneration, P L5464).
- **buildUnion** (U L174–274), the arm factory. Per arm: (1) member getAll (Root
  relational or `~func` relation-function member); (2) **PK slot synthesis**
  `managePrimaryKeys` (U L136): every arm carries the full cross-product of member PK
  slots `pk_<k>_<i>` — own member's real keys, siblings NULL-filled; (3) milestone
  columns suffixed `from_z_<i>`/`thru_z_<i>`, NULL-filled in siblings; (4) FK columns
  `fk_<i>` when navigating; (5) **value columns aligned to `allColumnsName`** — the
  union's common vocabulary; missing ⇒ `SQLNull` alias; (6) `u_type` discriminator
  prepended (driver context only), using the **deduplicated** member index while column
  suffixes use the RAW ordinal (self-union trap).
- **Value columns are merged un-suffixed** under `getUnionPropertyName` (P L1807 —
  concatenated unique names, e.g. `PersonSet1lastName_s1_PersonSet2lastName_s2`);
  **key/FK/milestone columns are per-member suffixed** — suffixing is what keeps split
  keys from cross-matching.

**Navigation.** Master routine `buildSQLQueryOutManySetImplementations` (U L408–514):
- FROM a union: per-member FK columns injected into each arm
  (`addMissingColumnToUnion` U L41–108, own FK populated, siblings NULL);
- INTO a union: target members normalized; >1 ⇒ target `buildUnion` aliased
  `unionAlias`/`unionalias_N`; operation-set as a nav target is REJECTED (U L483);
- join-back **OR-combined across member routes**
  (`simplyMergeJoinTreeNodeUsingOrOperation` U L584–631, emitting a tagged `UnionOrJoin`
  dyna): `on (uB.FirmID_0 = ua.ID_0 or uB.FirmID_1 = ua.ID_1)`. Union-to-union
  OR-combines the cartesian of legitimate routes; set-qualified PMs `prop[srcSet,tgtSet]`
  pin which routes are legitimate; extends-member override rule drops a PM whose source
  set is a super-set of another candidate's (P L1839–1847).
- **Merge-vs-push decision** (U L828–847): member join CHAINS that are not
  ordered-subsets of each other cannot be OR-merged — they are **pushed inside the union
  arms** (`pushChainedJoinsIntoSourceUnion`/`...TargetUnion` U L692–788): each arm applies
  its own residual joins internally, intermediate FK columns (`fk1_1`, `fk1_2`)
  materialize per-arm NULL-filled in siblings, and missing columns are hoisted up
  (U L790–826). The V1–V5 goldens exhaustively pin every source×target cardinality combo
  of this decision.
- Members with NO property mapping for the navigated property get a synthesized
  **null-join PM** (P L2011); its presence flips `avoidModeledProperties`, making FK
  columns RAW table column names (naming-consistency rule, U L388).

**Key invariants.**
- NULL discipline: only one slot per side non-null per row makes the OR-join safe; any
  coalesced fold MUST add the `... is null` disjunct (milestone guard:
  `(coalesce(from_z_0,from_z_1) <= d and coalesce(thru_z_0,thru_z_1) > d) or coalesce(all) is null`);
  the removeUnionOrJoins rewrite uses explicit `nullSafeEquals`.
- Member `~filter` stays per-arm WHERE inside the UNION ALL leg; never hoisted.
- **Aggregation over a union re-injects REAL per-member PK columns**
  (`addPkForAggregationInUnion` U L633, from P L3899) — grouping on the NULL-filled
  slots would collapse foreign-member rows.
- `-1` sentinel = single-set side, do NOT suffix (U L318/326).
- Nested materialization must re-expose every suffixed FK/PK/milestone column a parent
  join still needs — buildUnion's "project the entire raw column list" branch (U L222–231)
  exists solely for this (== our U4 raw-key-loss bug, named exactly).
- removeUnionOrJoins postprocessor (U L889–1385) = optional Snowflake-gated bridge-table
  rewrite, byte-identical results contract; the feature ships without it.

### 4.1a Behavioral contracts to port (goldens)
V1–V5 chained-join family (2/3/4-set unions, milestoned, push-vs-merge in every
combination); threeway union with overlapping FK/PK names; union self-join (manager);
chained-join + filter isolation (`unionalias_3` wrap); chained union aggregation
(group by pk_0_0, pk_0_1 + 4-way OR); `~filter` member golden; single-property/self-union;
union graphFetch root-level + milestoned (wave-2 crossover).

> Cut in the 2026-11 docs cleanup: §4.2–4.3, lite's side and verdict for unions, with the U4 redesign (2026-07). Full text: `git show de2745351:docs/RELATIONAL_FEATURE_MAP.md`.

## 5. Views

### 5.1 Engine model (REL = legend-pure relational.pure grammar; PSQL = pureToSQLQuery.pure)

**Dual identity.** `View` (REL L114–119) extends BOTH `NamedRelation` (queryable, has
name/columns/schema) AND `RelationalMappingSpecification` (columnMappings + ~filter/
~distinct/~groupBy + mainTableAlias) — and `RootRelationalInstanceSetImplementation`
extends the SAME RelationalMappingSpecification, so **a view and a class root mapping are
processed by the same code path** (`processRelationalMappingSpecification` PSQL:5556);
the getters getGroupBy/getFilter/getDistinct (PSQL:10242–10270) are polymorphic over both.

**The two outcomes, chosen by HOW the view is reached:**
1. Reached AS A RELATION (class mainTable, join endpoint, mainRelation) →
   `processRelation` (PSQL:5642): the view's spec is fully processed into an inner select
   and wrapped in **`ViewSelectSQLQuery` — which extends TABLE, not Select** (REL:270):
   a named, identity-carrying subselect with name+schema so all join/alias/PK machinery
   treats it like a physical table. **A view NEVER flattens into the outer query.**
2. A single view COLUMN referenced in an identity-sensitive expression (join condition,
   PK matching, self-join) → `findTableForColumnInAlias` (PSQL:10091–10108) recurses
   through columnMappings to the base physical table+column. Substitution exists ONLY for
   identity resolution, never for emission.

**Rules.**
- `~distinct` sets select.distinct + forces ALL declared columns materialized;
  `~groupBy` → select.groupBy; either SUPPRESSES synthetic pk_N columns (PSQL:5580).
  Views emit NO synthetic pks in any case (`viewSpecificationPrimaryKey` → [] PSQL:5703);
  view identity for self-join/graphFetch comes from the declared `View.primaryKey`,
  failing LOUDLY if absent (PSQL:4354).
- `~filter` with an INNER join tree is HOISTED into the view body pre-isolation
  (getRelationalElementWithInnerJoin PSQL:5535); LEFT_OUTER filter joins applied normally.
- View-on-view = natural recursion → nested ViewSelectSQLQuery; mainTable/findMainTable
  recurse to the base physical table.
- Two views over the same physical table = two separate ViewSelectSQLQuery instances with
  distinct aliases (alias = viewname+nodeId, PSQL:9853), disambiguated by schema when
  names collide — join correctly with no collapse (testViewToViewToUnion golden).
- Outer property predicates get DUPLICATED — pushed inside the view subselect AND
  repeated on the outer view column (testViewSimpleFilter golden) — a product of
  saved/extra filter isolation, deliberate.
- Inner view column aliases are UNQUOTED, outer class columns quoted
  (shouldQuoteColumnAliases PSQL:5707: View→false, Root→true).
- Milestoning applies inside the view body (isFromView flag, PSQL:5631/5658).
- View identity in tree matching is by object reference ($v.view == elem, PSQL:1550).

> Cut in the 2026-11 docs cleanup: §5.2–5.3, lite's side and verdict for views (2026-07). Full text: `git show de2745351:docs/RELATIONAL_FEATURE_MAP.md`.

## 6a. Views/embedded shared machinery note

The engine builds mainTable/mainRelation/getFilter/getDistinct/getGroupBy/resolvePrimaryKey
ONCE over the RelationalMappingSpecification + InstanceSetImplementation abstractions —
views, root mappings, embedded (via setMappingOwner), and unions (member fold) all answer
through the same resolvers.

> Cut in the 2026-11 docs cleanup: the note's second half, lite's equivalent seams and where the verdicts should land (2026-07). Full text: `git show de2745351:docs/RELATIONAL_FEATURE_MAP.md`.

## 6. Embedded / inline / otherwise mappings

### 6.1 Engine model (MAP = platform_dsl_mapping grammar; RMAP = relationalMapping.pure; PMI = functions_PropertyMappingsImplementation.pure; HELP = helperFunctions.pure)

**The type structure.** An embedded set-impl is SIMULTANEOUSLY a PropertyMapping and an
InstanceSetImplementation (MAP:80) — it maps the parent's property AND carries its own
propertyMappings. `setMappingOwner` always points to the owning ROOT set-impl (RMAP:52) —
PK/mainTable/mainRelation for ANY embedded (however nested) resolve through the root's
physical table (HELP:459–488). Three kinds:
- **Embedded** = same-row sub-instance, NO join, arbitrarily deep; navigation returns the
  source operation untouched (PSQL:1649), leaves become parent-row columns.
- **Inline** (`( ) Inline[targetId]`) = embedded delegating to another class mapping's
  property mappings — COPIED with owner/sourceSetImplementationId rebound
  (mappingExtension.pure:266; sharing by reference corrupts join reprocessing). Same-row;
  a join appears only if a BORROWED mapping itself joins. Supports subtype and non-root
  target ids; asserts exactly one root target (PMI:50).
- **Otherwise** (`( leaf:col ) Otherwise([id]:@join)`) = partial embedded + fallback PM
  (typically a join). **Per-leaf dispatch is name-driven**:
  `propertyMappingsByPropertyName(Otherwise) = body ∪ otherwisePropertyMapping` (PMI:84) —
  leaf in body → same-row column; absent → fallback join. Both coexist in ONE select.
  Deep traversal past an otherwise leaf forces the fallback join then chains into the
  target mapping's own joins; temporal targets re-enter with `milestoningUseOtherwise=true`
  (PSQL:713–718). GraphFetch flow sets `inGetterFlow`/`milestoningUseOtherwise` before
  descending (relationalGraphFetch.pure:775/805) — without these flags otherwise-dispatch
  silently doesn't happen in graphFetch.

Match order matters: Otherwise before Inline before plain Embedded (PSQL:1641–1650).
Enum pushdown works through embedded leaves; otherwise+union supported (owner chase
PSQL:1987).

> Cut in the 2026-11 docs cleanup: §6.2–6.3, lite's side and verdict for embedded, inline and otherwise mappings (2026-07). Full text: `git show de2745351:docs/RELATIONAL_FEATURE_MAP.md`.

## 7. GraphFetch (graph output)

### 7.1 Engine model (relationalGraphFetch.pure "GF", 1087 lines)

**NOT one big join.** The engine compiles a graphFetch tree into a TREE OF EXECUTION
NODES — one independent `SELECT DISTINCT` per class-level tree node, stitched by the
EXECUTOR, with the parent's PK set as the driver:
- Root node: SQL projecting root `pk_*` + to-one primitive properties (only to-one plain
  Properties inline — qualified properties NEVER inline, they always become child nodes).
- Each complex child: separate query whose FROM is the parent-PK temp table
  (`${temp_table_node_N}` placeholder; real temp table only on H2/Snowflake, VALUES
  inlining elsewhere) INNER JOIN main table LEFT JOIN property tables — projecting
  `parent_key_gen_*` (stitch keys) + own `pk_*` + primitives, with AND-of-`pk_* is not
  null` to suppress phantom children (GF:496–499).
- Executor groups child rows under parents by parent_key_gen; multiplicity from the tree
  decides object vs array. Temp table carries ONLY pk columns (GF:387).
- Cross-store children = root-like query on the target store driven by
  `parent_cross_key_*`; strict XStore constraints (single-property traversal,
  primitive-only keys, Varchar(4000) key type).

**Invariants:** PK definition REQUIRED (GF:663); store ~groupBy/~distinct FORBIDDEN in
graphFetch (GF:664–665) yet every generated query is `distinct=true` (identity dedup in
SQL); unions unsupported in this path (GF:950; union graphFetch is separate/newer);
aggregating qualifier children get GROUP BY over non-result columns, non-agg get
isNotNull; inner-join mapping ~filter switches the driver to a subquery (`_gftm`).

**Serialization contracts** (the part ANY implementation must honor):
- to-one absent → `null`; to-many absent → `[]`.
- JSON keys are semantically loaded: qualifier signature (`"name()"`,
  `"nameWithTitle('Mr')"`), milestone date (`"product(2015-10-16)"`), tree alias
  overrides (`'fn':name` → `"fn"`).
- primitives: date `"2003-07-19"`, timestamp with nanos, booleans, nulls.
- `graphFetchChecked` → `{"defects":[],"value":{...}}` envelope; defect records carry
  id/externalId/message/enforcementLevel/ruleType/ruleDefinerPath/path; checked and
  enableConstraints change NO SQL — executor-side.
- subtype subtrees: rows not of the subtype simply omit those properties; alloy config
  adds `@type` + base64 object references encoding PKs.
- batchSize only from `graphFetch(tree, size)` overloads.

> Cut in the 2026-11 docs cleanup: §7.2, lite's side and verdict for graph fetch (2026-07). Full text: `git show de2745351:docs/RELATIONAL_FEATURE_MAP.md`.

## 8. Qualified/derived properties + auto-map

### 8.1 Engine contracts (test-derived; P/Q/ADV/MAP/ATM/TREE = engine test files, IMPL = pureToSQLQuery.pure)

**The governing invariant — TRANSPARENCY:** a qualified property is indistinguishable from
manually inlining its body (pinned by consistency tests modulo alias numbering). The body
is position-INDEPENDENT; the surrounding SHAPE is position-DEPENDENT:
- project/filter, body touches owning table only → verbatim inline, no join, no subquery.
- through association → the join emits once; body binds to the joined alias.
- parameterized: scalar/enum args → SQL literals (enum via EnumerationMapping source
  value, NOT the enum name); root-correlated args ($f.legalName, $this.date) → correlated
  column refs folded into the join ON, operand order preserved (never normalized).
- **aggregate anywhere in a QP/map body** → `addPkForAggregation` grouped-subquery-rejoin:
  `left outer join (select PK, agg(...) as aggCol ... group by PK) on root.PK` —
  post-aggregate arithmetic stays OUTER referencing aggCol; hard-fails without a PK
  (IMPL:3855). The isAggregationFunction whitelist surprisingly includes plus/stringPlus/
  joinStrings.
- isEmpty/isNotEmpty on to-1 complex → `select distinct PK` existence subquery + CASE on
  null-ness (branch order flips with the predicate).
- chaining a QP off an already-joined table isolates the type-filter into a subselect
  (can't share the ON); single-hop keeps it in the ON-clause. Forced-isolation debug
  goldens (FQ) pin all three strategies on one query.
- mapping ~filter ANDs into the QP-generated join ON.
- null-safe equality `a=b or (a is null and b is null)` fires by column NULLABILITY, not
  syntax.
- toOne()/first()/at() NEVER emit LIMIT — multiplicity assertions that only steer
  isolation. (Our cycle-86 navLeafSubquery LIMIT-1 wrong-value bug violated exactly this.)
- Single-expression rule: multi-statement QP bodies rejected (IMPL:1060).

**Auto-map:** implicit `.prop` over to-many ≡ explicit ->map (pinned identical SQL).
Project position → LEFT JOIN row explosion, no dedup; filter position → distinct-FK
semijoin + is-not-null; aggregation → grouped subquery keyed on the PK of the MAP SOURCE
set (per-employee vs per-firm vs global distinguished only by what encloses the map);
duplicate navigations reuse one alias.

**Paths (#/Class/prop!alias#):** path ≡ lambda for the same navigation; segments fold
left-to-right, QP segments (with args) allowed mid-path, navigation continues past a QP
result; `!alias` names the TDS column; aliasing suppressed in filter position.

**Trap:** engine `test.ToFix` goldens assert current-but-WRONG behavior (P:136, P:188,
ADV:237) — never port those as contracts.

> Cut in the 2026-11 docs cleanup: §8.2, lite's side and verdict for qualified and derived properties (2026-07). Full text: `git show de2745351:docs/RELATIONAL_FEATURE_MAP.md`.

## 9. Dyna-functions & dialect architecture

### 9.1 Engine model (REL = relationalExtension.pure; DBX = dbExtension.pure; DEF = extensionDefaults.pure; H2/DB2 = dialect files)

**TWO independent registries keyed by the same dyna names** (never conflate):
1. **Type-inference registry** (REL:192–1887): dyna name → ordered list of
   (matcher-guard, type-producer) pairs; first matching guard wins; numeric promotion via
   the safe-type lattice (TinyInt→…→Decimal, Varchar size=max, precision capped at 31);
   uninferrable string size defaults 1024.
2. **Rendering registry** (DEF:180–295 ANSI + per-DB): dyna name → `ToSql{format,
   transform, contextAwareTransform, parametersWithinWhenClause}`. Per-DB composition is
   `default->groupBy(name)->putAll(dbSpecific->groupBy(name))` — **whole-template
   replacement per name, no partial override**. Dispatch filters by GenerationState
   (Select/Where × withinWhenClause) and asserts exactly one survivor.
   The legal-name universe is the `DynaFunctionRegistry` enum (DBX:1080–1308, 300+ names).

**DbExtension** = a record of function slots (literalProcessor, dynaFuncDispatch,
selectSQLQueryProcessor, joinProcessor, identifierProcessor, dataTypeToSqlText, limits…)
with `default::` fallbacks per slot. Literals: ANSI quotes booleans ('true'); H2 unquoted
TRUE + `CAST(%s AS FLOAT)` float literals + DATE'…'/TIMESTAMP'…'; date literals default
GMT. Identifier quoting: global flag OR reserved word OR embedded space; FreeMarker
`${…}` never quoted. LIMIT dialects diverge structurally (limit N / offset-fetch /
fetch-first / top), with limit arithmetic pre-computed. H2 emulates FULL OUTER as
UNION ALL of LEFT+RIGHT with a `_lhsJoinValid_` sentinel.

**Load-bearing per-function semantics** (the agent archive has the full ~300-name
catalog; these are the correctness-critical ones):
- 1-BASED string indexing everywhere (substring, indexOf→LOCATE with swapped args).
- `mod` is SIGN-NORMALIZED `mod(mod(a,b)+b,b)`; `rem` is raw mod. Distinct functions.
- division/average force float: `((1.0*a)/b)`, `avg(1.0*x)`.
- `round` defaults 2nd arg 0; projection wraps `cast(round(x) as bigint)`;
  ceiling/floor project through `cast(cast(... as numeric) as bigint)`.
- null-safe equality ONLY in filter position on two nullable columns (DBX:926–948);
  `not(equal)` / `not(in)` add OR-is-null arms depending on which side is a literal.
- `if`/`case` args render within-when-clause; `coalesce` n-ary; `concat` null semantics
  and rendering differ per DB (DB2 infix `concat`).
- Filter contract: the projected expression string is REPEATED VERBATIM in WHERE — same
  rendering in both positions.
- adjust/dateDiff unit maps (WEEKS→7·DAYS on DB2); firstDayOf* families are date
  arithmetic, not calendar-table lookups.

> Cut in the 2026-11 docs cleanup: §9.2, lite's side and verdict for dyna-functions and dialects (2026-07). Full text: `git show de2745351:docs/RELATIONAL_FEATURE_MAP.md`.

## 10. Mapping metamodel: inheritance/extends, includes, merge, multigrain, modelJoins, cross-store, associations

### 10.1 Engine model (P2S = pureToSQLQuery.pure; MJ = modelJoins.pure; tests under $E/tests/mapping/)

- **Inheritance/subtypes** — two mechanisms: (a) Operation set-impl with the
  `inheritance_...` operation fanning an abstract-class query to concrete subtype
  mappings (mixed-type results); (b) `subType(@T)` navigation cast mid-query. Unmatched
  subtype columns project TDSNull, rows never dropped. Embedded mappings can live inside
  per-subtype mappings on ONE table. `root==true` (`*` in grammar) marks the primary
  mapping among several for one class. Known engine gap (ToFix): qualified property from
  an unmapped superclass.
- **Extends** — `X[child] extends [parent]` inherits mainTable, PK, ~filter/~groupBy/
  ~distinct, and property mappings; overrides by property name; transitive chains; store
  substitution composes. **The PK-resolution matrix** (testExtendsForPrimaryKey:238–258)
  is the authoritative 4×4 truth table for child×parent {groupBy, distinct, userPK,
  nothing} — counterintuitive corners (child userPK loses to parent distinct → [id,aName]).
- **Includes** — transitive; `include M[src->dst]` store substitution is per-include-edge;
  `resolveStore` on a non-substituting mapping silently maps foreign→local (not an
  error); class-mapping ids must be unique across the include closure.
- **Merge** — union of set-impls + multi-hop join expressions inside one PM
  (case/and/or over `@J1 > @J2 > @J3` chains). Missing union branch column ⇒
  `Literal('__SQLNULL__')` (never dropped); same table via two join paths ⇒ two aliases,
  NEVER deduped by table name.
- **Multigrain** — MultiGrainFilter (discriminator ~filter per grain on one table) with
  THE suppression rule (P2S:1758–1771): root grain filter → WHERE; joined grain via
  non-PK column → discriminator pushed into ON; joined grain via a **single-column PK
  equi-join** (`isSimpleJoinToPk` P2S:1792) → filter SUPPRESSED (grain already pinned).
  distinct/groupBy short-circuit filter handling entirely, before the multigrain check.
- **ModelJoins** — association bound by a Pure lambda over the two ends instead of a DB
  @Join. Operator whitelist {equal,not,and,or,comparisons}; only end-var navigation +
  literals. TWO implementations: static localization (MJ:39–143, → synthetic
  RelationalAssociationImplementation + Join in a synthetic DB) and the newer
  union/milestoning-aware per-branch runtime compilation (P2S:2208–2304: placeholder
  `ON 1=1` join, `_mj_src`/`_mj_tgt` scope binding, conditional target
  materialization-as-subselect when the target has filter/groupBy/distinct or nested
  navigation). Goldens: condition compiles into the ON (incl. lower(), concat, arithmetic,
  IN — the IN predicate is pushed BOTH into the target subselect WHERE and the ON).
- **Cross-store** — CrossSetImplementation → synthesized root set over a placeholder
  table (`Gen_Cross_DB`), primitive cross columns as propertyPlaceHolder columns filled
  at execution.
- **Associations** — `AssociationMapping(end1[srcId,tgtId]: @J, end2[tgtId,srcId]: @J)`;
  default ids only legal when each end class has exactly ONE mapping; self-association =
  two class-mapping ids for the same class; multi-join chains `@J1 > @J2` are
  direction-specific (reverse spells the reverse chain); intermediate tables need PKs.

Provenance note: the core set-impl metamodel classes live UPSTREAM in legend-pure (this
matches [[verify-signatures-against-real-legend-pure]] — legend-pure checkout is part of
the spec base).

> Cut in the 2026-11 docs cleanup: §10.2, lite's side and verdict for the mapping metamodel (2026-07). Full text: `git show de2745351:docs/RELATIONAL_FEATURE_MAP.md`.

## 11. AggregationAware + calendar aggregation

### 11.1 Engine model — aggregationAware (AA = core/store/aggregationAware/aggregationAware.pure — NOTE: the engine lives in legend-engine-core, NOT under core_relational; $E has only tests)

A pre-aggregation REWRITE that runs before store planning:
- Mapping = `~mainMapping` (fallback) + ordered `Views`, each `~aggregateMapping`
  (relational set over the agg table) + `~modelOperation{~canAggregate,
  ~groupByFunctions, ~aggregateValues(~mapFn/~aggregateFn over the magic $mapped var)}`.
- **Only a top-level object `groupBy` triggers table switching** (AA:109); getAll/
  filter/project reprocess but bind to the main table. The collection under the groupBy
  must be exactly getAll(+filter) — anything else falls to main.
- Matching: query dimensions/measures compiled to order-insensitive **ProjectPaths**
  (qualified properties INLINED first; numeric aggregates canonicalised
  Integer/Float→Number so sum(Float) matches sum(Number)); `groupByMatch` = every query
  dim rewritable against view dims AND (canAggregate OR query grain == view grain);
  `aggValueMatch` = each measure matches a view spec (mapFn AND aggregateFn separately —
  max never matches a sum-only view) or is reachable as a dimension. **First matching
  view wins (mapping order significant); no match → main table, never an error.**
- `canAggregate=false` NARROWS a view to exact-grain queries; doesn't disable it.
- Observability: AggregationAwareActivity carries the rewritten query string (the goldens
  assert on it).

### 11.2 Engine model — calendar aggregation (CF/CP/CS = calendarFunctions/PureToSQL/Store)

34 grammar-stub functions `f(inputDate, calendarType, endDate, inputValue)` resolved ONLY
by the relational store:
- **A precomputed calendar DIMENSION table** per region (`NY_Calendar`, 24 columns:
  fiscal ordinals, offsets, prior-week dates, previous-period columns…). All window
  logic = table lookup on TWO LEFT OUTER self-joins: `calendar_0` keyed on the row's
  inputDate, `calendar_1` keyed on the endDate expression. Emission =
  `sum(case when <conditions over the two calendar rows> then value[/normFactor] else null end)`.
- Families: current (cw/reportEndDay), to-date (wtd/mtd/qtd/ytd — `<=` on a fiscal
  ordinal), previous-to-date (+prev-year rollover via precomputed columns, NOT
  arithmetic), previous-whole-period, rolling nX (date-range, adjusted-date aware),
  means (divide inside the case by 4/12/52 or a calendar ratio; pywa null outside
  first-5-weeks). p12mtd is the ONLY function using SQL date arithmetic.
- Fiscal week starts Saturday; weekend rollback differs per family (cw −1/0, pw −2/−1,
  _fm variants ignore weekends).
- Distinct endDates ⇒ distinct calendar_end aliases shared across sibling aggregates;
  union-mapped classes take a different join-splice path (unionBase form).
- The 27 date-range goldens ([min,max] of included dates per function @ 2022-11-16
  anchor) are the perfect behavioral spec — they pin windows without pinning SQL.

> Cut in the 2026-11 docs cleanup: §11.3, lite's side and verdict for aggregation-aware and calendar aggregation (2026-07). Full text: `git show de2745351:docs/RELATIONAL_FEATURE_MAP.md`.

## 12. Execution plans, TDS & result contract

### 12.1 Engine model (EP = executionPlan.pure; RME = relationalMappingExecution.pure; SC = storeContract.pure)

**Plan node taxonomy**: `SQLExecutionNode` (sql string + `resultColumns` = (label,
dataType-or-"") pairs + connection + RelationMetadata) wrapped by one of FOUR
instantiation nodes (Tds/Class/RelationData/DataType — constraint-enforced result
types); `CreateAndPopulateTempTableExecutionNode` for big in-lists (threshold
DB-specific: H2=50, DB2=32767, plus Stream inputs — chosen at RUNTIME by a
FreeMarkerConditionalExecutionNode); `RelationalBlockExecutionNode` (transactional,
whole-subtree scan for temp-table nodes upgrades the wrapper); plan variables via
Allocation/Constant/FunctionParametersValidation nodes.

**TDS result contract** (the part that matters to any implementation):
- TDSColumn = name (user's project alias) + Pure type (Primitive|Enumeration|
  Variant/Map/List ONLY — else fail) + **offset** (positional, load-bearing) +
  **sourceDataType** (DB-side type; TWO conflicting pureTypeToDataType overloads:
  result-inference uses String→Varchar(8192)/Integer→BigInt; the generic map uses
  Varchar(1024)/Integer) + enumMappingId/enumMapping.
- Computed/derived columns legitimately have NO dataType (printed "") — tests assert
  this; do not invent types.
- **Enum decode is either result-side (enumMapping populated) OR SQL-side
  (PUSH_DOWN_ENUM_TRANSFORM clears it) — never both.** Enum source values quoted iff
  String-typed (numeric enum sources unquoted).
- Nulls: TDS rows carry TDSNull instances (outer-join padding); real CSV null = EMPTY
  field ('null' string is a test-harness display token only). Booleans: Bit sourceType;
  boolean-valued SQL literals render 'true'/'false' strings.
- Date/time: DateTime CONSTANTS fold the connection timezone into the literal at plan
  time; VARIABLES defer to the GMTtoTZ FreeMarker function. StrictDate→Date,
  Date/DateTime→Timestamp.
- FreeMarker support-function library (renderCollection, optionalVarPlaceHolder
  selector, per-enum enumMap_<path> functions; empty enum param ⇒ `0 = 1` predicate).

**Routing**: StoreMappingRoutedValueSpecification wrappers carry .sets/.mapping/
.propertyMapping; traversal through user functions is NOT routed; multi-expression
lambdas produce multiple SQLs.

> Cut in the 2026-11 docs cleanup: §12.2, lite's side and verdict for execution plans and the result contract (2026-07). Full text: `git show de2745351:docs/RELATIONAL_FEATURE_MAP.md`.

## 13. Postprocessors

### 13.1 Engine model (PP = postProcessor.pure; DPP = defaultPostProcessor.pure; FPD = pushFiltersDownToJoin.pure; RA = reAliasQuery.pure)

**Two pipeline phases** (plan-gen `postProcessSQLQuery` and execution `postProcessQuery` —
different processor sets), built on two transform engines: `transform` (cached memo,
identity-preserving — CTE dedup, mapper rewrites) vs `transformNonCached` (rebuilds join
aliases/target wiring — re-alias, push-down). Empty-data select short-circuits everything.

**Plan pipeline** = 5 ordered macro-processors: processInOperation (large IN → temp table,
DB threshold) → processObjectReferenceIn → defaultProcessors [inner 5-pass: **CTE hoist →
pushFiltersDownToJoins → removeUnionOrJoins → replaceAliasName → prependSQLComments**,
then connection sqlQueryPostProcessors] → connectionAware → dbSpecific (aliasLimit trim +
pre/finally SQL).

**pushFiltersDownToJoins** (the semantically rich pass): strictly **ADDITIVE** — outer
WHERE always preserved, predicates COPIED onto join ONs (rewritten across join-key
equality, both directions) and into pushable subselects. Pushable = binary comparisons/
startsWith/endsWith/contains/in with constant/parameter value side + null-tests; bails
subtree-wide on ANY right/full outer join; subselect push blocked by window columns,
LIMIT rows, >1 filtering/having op, or unknown node types (whitelist walk); group-by
column → inner WHERE, non-group under groupBy → HAVING; aggregate columns never pushed.
Duplicated predicates in one ON are legitimate golden behavior.

**replaceAliasName**: case-insensitive basename grouping → deterministic `<name>_<i>`;
reserved aliases root/unionBase/subselect never renamed; hard-asserts every alias mapped.
**CTE hoist**: lift all nested commonTableExpressions to root, dedup by name.
**prependSQLComments**: `-- "executionTraceID" : "${execID}"` tag only.
**trimColumnName**: aliasLimit truncation to `<prefix>_<i>` (DB2 128).

**Extension model**: connection-level `sqlQueryPostProcessors` (+ConnectionAware variant)
+ parameterized `PostProcessorWithParameter` triad — built-ins: Mapper (table/schema
qualifier rewrites, temp tables exempt, last-match-wins), RelationalMapper (db-level),
ExtractSubQueriesAsCTEs (level-indexed `subquery_cte_<level>_<idx>`, row-identical
contract). Context-based hooks (FunctionActivator: timestamp casts, FreeMarker→
table-function param rewrites). removeUnionOrJoins gated Snowflake-or-flag.

> Cut in the 2026-11 docs cleanup: §13.2, lite's side and verdict for postprocessors (2026-07). Full text: `git show de2745351:docs/RELATIONAL_FEATURE_MAP.md`.

## 14. Tooling features: lineage, testDataGeneration, constraints, mutation, autogeneration, contract

### 14.1 Engine feature models (files under $E/lineage, /testDataGeneration, /validation, /mutation, /autogeneration, /mft, /contract, /extensions)

- **Lineage** — static analyses over the property-path tree: `scanColumns` → flat
  `ColumnWithContext[*]` (column + enclosing-SQL-node-class context; view columns
  expanded via columnMappings); `scanRelations` → a `RelationTree` of tables/views +
  joins (static form off the mapping; a runtime form reads relations off generated SQL
  clusters — that variant needs the whole pipeline). scanRelations is a PREREQUISITE for
  testDataGeneration.
- **TestDataGeneration** (the XL feature) — from query + mapping + seed row identifiers:
  scanRelations tree → per-relation column set (PKs ∪ non-nullable ∪ temporal dates ∪
  referenced) → root SELECT top-20 with OR-of-AND identifier filter (+milestoning
  filter) against a LIVE DB → per child join, materialize parent rows into
  `testDataGen_Temp_*` temp tables and join-pull child rows → dedup → CSV bundle
  (`schema\nname\ncsv` blocks). No identifiers and no default-PK flag → LOUD fail with
  copy-pasteable example seed code. Views/unions/milestoning/plan-generation are staged
  extensions.
- **Constraint validation** — THIN GLUE: each class constraint becomes
  `filter(x|not(body))->project(CONSTRAINT_ID, ENFORCEMENT_LEVEL, MESSAGE [+extras])`,
  concatenated, run through the ORDINARY relational path (constraint bodies with exists/
  aggregates/isDistinct compile like any query). Driver-table PK added to violations.
  ~150 lines of AST synthesis; no new SQL machinery.
- **Mutation** — plan-only: save/upsert (`RelationalSaveNode` + UpsertSQLQuery; only
  single-root, direct-column mappings; graph tree must contain ALL PKs; processing-
  temporal in/out sentinel injection) and relation write (INSERT…SELECT same-connection;
  temp-table staging cross-connection).
- **Autogeneration** — DB → Pure model reverse-generation (table→class, join→association
  with PK-based multiplicities, column→property with nullability→multiplicity,
  Bit→Boolean case-mapping). Self-contained graph transform; no execution deps.
- **MFT** — harness wiring; one datum: `rowExplosion` is the declared unsupported
  mapping feature.
- **StoreContract/extensions** — the SPI checklist (planExecution/supports/
  shouldStopRouting/resolveStore/executeStoreQuery/connectionEquality/…) + the
  RelationalExtension plug-points (buildUniqueName, milestoning filter handlers,
  post-processors, dialect node converters). For us: the interface inventory documents
  what a full store integration means; our QueryService/Compiler seam is the equivalent.

> Cut in the 2026-11 docs cleanup: §14.2, lite's verdicts for the tooling features (2026-07). Full text: `git show de2745351:docs/RELATIONAL_FEATURE_MAP.md`.

---

> Cut in the 2026-11 docs cleanup: §15, the synthesis: lite's coherent-architecture plan (shared foundation, legs 1–5, checklist burns, deferrals and their order), 2026-07. Full text: `git show de2745351:docs/RELATIONAL_FEATURE_MAP.md`.
