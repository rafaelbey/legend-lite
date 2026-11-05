# Test Corpus Master Plan — features, harnesses, combinations, benchmarks

**Date:** 2026-08-12. **Question:** what should a Legend test-and-benchmark corpus
contain, so that it detects wrong answers and performance cliffs rather than merely
proving things don't crash? **Method:** a measured performance audit of legend-engine
+ legend-pure, four grammar/doc surveys of the feature surface, a rebuild of the corpus
from scratch with an independent oracle, and mutation testing to prove the battery can
fail. **Answer:** below — a census-driven build queue with a known end, a high-order
combination strategy, and a benchmark methodology whose alarms are shape changes rather
than percentages.

Everything asserted here was verified against a running engine or against source at a
cited path. Where a claim is inferred rather than measured, it says so.

---

## 0. Why this document exists

The previous corpus reached **10,800 generated cases** and could not detect a wrong
answer. Its assertion was `rows > 0`. Auditing it found:

- `filter(age > 30)` seeded against ages `{21,22,23}` — **100% of those cases returned
  zero rows**; the predicate was arithmetically incapable of matching.
- **All 294 graph-fetch cases returned nothing**, so the entire graph-fetch execution
  path was unverified.
- Subtypes and associations were generated into 432 models each and **referenced by 0 of
  8,208 queries** — inert. `compile_ms` was identical at `subtype=0` and `subtype=3`.
- `r0_id` = row number = the joined PK, so **every join matched exactly once**; a
  completely wrong join still returned 3 rows and passed.
- ~75% of the 864 models were duplicates with respect to what the queries exercised.

Case count is not a quality metric — but that is an argument against *unasserted* scale,
not against scale. The target is **10,000+ cases and 1,000+ models**, every case
asserting a computed value (§5A). Everything in this plan follows from that distinction.

---

## 1. What the audit found — and what each finding implies

| Finding | Evidence | Implication for the corpus |
|---|---|---|
| **E-1** — lambda compilation is Θ(2ⁿ) in property-traversal depth | 2.00×/hop; 0.54 ms → 6,855 ms at 22 hops; controlled experiment: `[0..1]` → `[1]` collapses 6,855 ms to 0.56 ms (**12,240×**) | Depth must be a first-class swept axis (→24). Pairwise matrices generate depth 2, where it is invisible. |
| **E-2** — graph-fetch plan-gen is Θ(2ᵈ) in tree depth, plus a ~8 ms/node constant | 1.97×/level across M2M, relational and chained M2M-over-relational; **40.3 s at depth 24 on a 6-element, 0.6 KB model** | Two separate axes: tree depth *and* node count. Tiny models can be worst case, so do not scale model size. |
| **B-4** — `count()` on an empty to-many returns 1, not 0 | `COUNT(*)` over a LEFT OUTER JOIN counts the preserved outer row; 6 fixture rows wrong | Zero-cardinality data is mandatory. Silent wrong answers are the worst class. |
| **B-1** — graph-fetch tree parser NPEs on `%latest` | `GraphFetchTreeParserGrammar.g4:41` declares `LATEST_DATE`; the walker handles 2 of 4 alternatives and dereferences null | Error paths need tests. Grammar-vs-walker gaps are a defect *family*. |
| **B-2** — graph fetch over a union-mapped source throws a cast exception | `relationalGraphFetch.pure:358`; 504/504 matching cases; zero without a union | Needs three features together — no pair reproduces it. |
| **Dependency recompile tax** | Editing one class: 5.0 ms alone → **188.5 ms** against 10,082 dependency elements (38×), linear in total elements | Dependency scale is its own benchmark dimension. |
| **Plan-gen costs more than execution** | 8.9 ms vs 6.4 ms median over 748 executed cases | Benchmark plan generation as a first-class stage. |
| **Scaling is about query shape, not model size** | 32× more source → 13× parse, 3× compile. Package density, include-chain depth, inheritance depth: all flat | Do not spend budget generating bigger models. |

### Refuted — do not spend effort here

| Hypothesis | Disproof |
|---|---|
| Union mappings cause mᵏ router permutation blowup | 2→16 union sets: 23.6 → 41.0 ms. Sub-linear. |
| Oversized never-JIT-compiled methods are a bottleneck | Sizes are real (26,409 B confirmed by `javap`) but A/B on `-XX:-DontCompileHugeMethods` gives **+2.4%** — noise, at both simple and 80-column queries. Their cost is *allocation*, not interpretation. |
| JIT code cache is exhausted | 36% / 20% full after sustained load. |
| O(N²) package-children scan matters | 3,200 classes in one package compile in 165 ms. |
| Mapping include-graph traversal is exponential | A 64-deep include chain is *faster* than a flat mapping. |

---

> Cut in the 2026-11 docs cleanup: §2 the feature census as measured in 2026-08 (the protocol type roster, the grammar
> keyword roster, the semantic census and its worklist) and §2b its correction (the keyword census, 670 grammar
> keywords, is the better denominator than the protocol roster). Full text:
> `git show de2745351:docs/TEST_CORPUS_MASTER_PLAN.md`.

---

## 3. Feature inventory by DSL

Derived from four surveys of the `.g4` grammars, `docs/`, and working examples in the
legend-engine checkout. Use the census (§2) as the denominator; use this as the map of
what the constructs *are*.

### 3.1 Domain (`###Pure`)

Legend Engine accepts **6 element kinds only**: `Class`, `Association`, `Enum`,
`Profile`, `function`, `Measure`. `Primitive`, `native function`, top-level `^instance`
and `projects` are grammar-reachable but rejected by the walker with "Unsupported
syntax". **Type parameters `<T>` / `<|m>` parse and then hard-fail** in legend-engine
(valid in legend-pure only) — a generator must never emit them.

| Feature | Notes |
|---|---|
| Multiplicities | `[1] [0..1] [*] [1..*] [n..m] [n]`; bare identifier form is the multiplicity-parameter form, engine-rejected |
| Primitives | String, Integer, Float, Decimal, Boolean, Date, StrictDate, DateTime, StrictTime, Number, Any, Byte |
| Precise primitives | `Varchar(200)`, `Numeric(p,s)`, `TinyInt`…`UBigInt`, `Timestamp`, `Float4`, `Double` |
| Derived / qualified properties | zero-arg and parameterised; multi-statement bodies; return type **after** the body |
| Constraints | simple, named, and the full form with strict order `~owner? ~externalId? ~function ~enforcementLevel? ~message?` — only `~function` mandatory; `~enforcementLevel` is `Error` or `Warn` |
| Constraints on **functions** | grammar-reachable but **silently dropped by the walker** — do not generate |
| Inheritance | `extends`; multiple supertypes grammar-legal; queried via `->subType(@T)` (root level only) |
| Associations | simple, self, many-to-many; may carry stereotypes/tags/aggregation/defaults **and a qualified property** as a third member |
| Aggregation kinds | `(shared)`, `(none)`, `(composite)` — after stereotypes, before the name |
| Property defaults | literal, array, `^instance`, enum ref, graph ref — simple properties only |
| Enumerations | quoted values, digit-leading values (must be quoted), stereotypes/tags on enum and values |
| Profiles | stereotypes + tags; both lists optional and repeatable |
| Documentation | `'''…'''` triple-quoted, sugar for `doc.doc`; **no escape mechanism** |
| Milestoning stereotypes | `temporal.businesstemporal`, `.processingtemporal`, `.bitemporal` |
| Milestoned querying | `.all(d)`, `.all(d1,d2)`, `%latest`, `.allVersions()`, `.allVersionsInRange(a,b)`, property nav `$x.p(%latest)` |
| Measures & Units | canonical `*Unit` (exactly one), non-convertible measures, unit-typed properties `M~U`, unit literals `5 M~U` |
| Functions | params, mandatory return type+multiplicity, overloading, `<<access.*>>`, function descriptors |
| Lambdas / `let` / collections | `^` instance construction incl. nested and `+=` |
| Graph fetch trees | `#{ }#`, aliases, qualified properties with args, `->subType()` |
| Navigation paths | `#/a::B/prop#` |
| Casts | `@Type`, `->cast`, `->to`, `->toMany` |

**Core profiles available:** `doc`, `access`, `temporal`, `milestoning`, `equality`,
`typemodifiers`, `functionType`, `test`.

### 3.2 Mapping

Registered mapping-element parser names: **`Pure`, `Relational`, `Relation`,
`Operation`, `XStore`, `ModelJoin`, `AggregationAware`, `AggregateSpecification`,
`EnumerationMapping`, `MongoDB`, `ServiceStore`, `Deephaven`**.

| Group | Features |
|---|---|
| Container | `Mapping pkg::M ( )`; section imports; `include mapping X`; **include with store substitution** `include mapping X[dbInc->db]`; `include dataspace X` |
| Element | root flag `*`; set id `[id]`; `extends [superSetId]`; mapping element name (EnumerationMapping only) |
| Class-mapping kinds | Pure (M2M) with `~src`/`~filter`; Relational; Relation with `~func` or `~src` + `~primaryKey`; Operation (union / special_union / inheritance / merge with a validation lambda); XStore (`$this`/`$that`); ModelJoin; AggregationAware (`Views:[...]` + `~mainMapping`); ServiceStore; MongoDB |
| Relational knobs | **strict order** `~filter? ~distinct? ~groupBy? ~primaryKey? ~mainTable?` then property mappings; `~filter` may take a join sequence and an explicit `(INNER)`/`(OUTER)` qualifier; `~mainTable` may be schema-qualified or a **View** |
| `scope()` | `scope([db]table)( … )`, `scope([db])( … )`, scope on a View |
| Property mapping forms | simple column; dyna-function RHS; `[db]@join`; join chains `@a > (INNER) @b \| col`; cross-database chains; `prop[srcId,targetId]`; **embedded** `prop( … )` arbitrarily nested (may contain `~primaryKey`); **Otherwise** `) Otherwise ([id]: @join)`; **Inline** `prop() Inline[setId]`; `Binding pkg::b : rhs`; `EnumerationMapping id : rhs`; local property `+p: T[m]: rhs`; M2M explosion `prop*:` |
| Association mappings | Relational `AssociationMapping ( … )`; XStore; ModelJoin |
| Enumeration mappings | string / integer / enum-reference / quoted / unbracketed source values; named and unnamed |
| Tests | `testSuites` (modern) and legacy `MappingTests` — `MappingTests` must precede `testSuites` |

Ordering constraints a generator must respect: mapping body is
`include* → mappingElement* → MappingTests? → testSuites?`; property mappings are
comma-separated, class-mapping knobs are not.

### 3.3 Relational store

| Group | Features |
|---|---|
| Database | `include <db>`; typed `include <storeType> <db>`; stereotypes/tags/doc on Database, Schema, Table, Column, View |
| Schema/Table | quoted identifiers; duplicate schemas merge; empty tables legal; **reserved words usable unquoted as column names** |
| Column types (19) | CHAR(n), VARCHAR(n), NUMERIC(p,s), DECIMAL(p,s), FLOAT, DOUBLE, REAL, INT→INTEGER, BIGINT, SMALLINT, TINYINT, DATE, TIMESTAMP, BINARY(n), VARBINARY(n), BIT, ARRAY→OTHER, OTHER; plus JSON and SEMISTRUCTURED. **No BOOLEAN, no DISTINCT.** Arity is enforced at parse time. |
| Modifiers | `PRIMARY KEY` / `NOT NULL` — mutually exclusive; multi-column PK = repeated `PRIMARY KEY` |
| Views | **strict order** `~filter → ~groupBy → ~distinct → columns`; column `PRIMARY KEY`; columns via join or join chain; views on views; views joinable like tables |
| TabularFunction | columns only; no milestoning. **Stored procedures unsupported.** |
| Filters | `Filter F(cond)`; dyna-functions and `in(x,[...])`; on a view; **filters cannot reference joins** — that composition happens at the mapping `~filter` site |
| MultiGrainFilter | semantically distinct only in SQL generation |
| Joins | simple; composite; `{target}` self-join (no `[db]` prefix, no schema qualification); all 7 comparison operators; `is null` / `is not null`; and/or nesting with `group()`; dyna-functions on either side; `in()`; cross-database with `[db]` pointers; chained comparison `a=b=c`; semi-structured extraction |
| Join sequences | `@a > @b`, per-hop `(INNER)`/`(OUTER)`, per-hop `[db]`. **Only INNER and OUTER** are accepted. |
| Functional form | every infix operator has a dyna-function equivalent (`equal`, `and`, `isNull`, `group`, …) parsing to the identical AST — doubles the combination space for free |
| Milestoning | `business(BUS_FROM=, BUS_THRU=[, THRU_IS_INCLUSIVE=][, INFINITY_DATE=])`; `processing(PROCESSING_IN=, PROCESSING_OUT=[, OUT_IS_INCLUSIVE=][, INFINITY_DATE=])`; snapshot forms `BUS_SNAPSHOT_DATE` / `PROCESSING_SNAPSHOT_DATE`; bitemporal = both, comma-separated. **Knob order is rigid.** |
| Dyna-function vocabulary | 168 names available in join/filter/view operations |

**Connections & runtimes:** 21 `type:` values; 15 datasource specs (LocalH2 with
`testDataSetupSqls`/`testDataSetupCSV`, Static, EmbeddedH2, Snowflake, BigQuery,
Databricks, Redshift, Spanner, Trino, Athena, DuckDB, Oracle, Aurora, GlobalAurora,
MemSql); 11 auth strategies; `mode: local;` (mutually exclusive with
`specification:`/`auth:`); post-processors (`mapper`, `relationalMapper`,
`ExtractSubQueriesAsCTEsPostProcessor`); `queryGenerationConfigs`; `quoteIdentifiers`;
`queryTimeOutInSeconds`; `timezone`. Runtime: multi-mapping/store/connection,
`connectionStores:`, `SingleConnectionRuntime`, inline `#{ }#` connections.
**No runtime include/extends** — only section `import`.

### 3.4 Sections

25 `###` section headers are accepted: 4 built-in (`Pure`, `Mapping`, `Connection`,
`Runtime`) + 21 extension-registered (`Data`, `Relational`, `QueryPostProcessor`,
`Service`, `Persistence`, `Diagram`, `Text`, `DataSpace`, `ServiceStore`,
`ExternalFormat`, `FileGeneration`, `GenerationSpecification`, `DataQualityValidation`,
`HostedService`, `FunctionJar`, `Snowflake`, `MemSql`, `BigQuery`, `MongoDB`,
`Elasticsearch`, `Deephaven`).

### 3.5 Relation vs TDS

A **TDS** (`meta::pure::tds::TabularDataSet`) carries its schema as a runtime
`TDSColumn[*]`, referenced by string name. A **Relation**
(`meta::pure::metamodel::relation::Relation<T>`) carries it as a compile-time type
argument `Relation<(name:String[1], age:Integer[0..1])>`, with row-type arithmetic
checked by the compiler. `TDS<T> extends Relation<T>`.

Separate function libraries with confusable names: TDS `restrict` ≡ Relation `select`;
TDS `renameColumn` ≡ Relation `rename`; TDS `olapGroupBy` ≡ Relation
`extend(over(…), ~…)`. **`project` exists in both** — `->project([col(f,'x')])` is TDS,
`->project(~[x: f])` is Relation.

Column-spec forms: `~a`, `~'a space'`, `~a:Integer`, `~a:c|'ok'`, `~a:c|'ok':x|'YO'`
(map+reduce agg), `~a:{p,f,r|'ok'}` (3-arg window), `~[a,b]`, `~[first:x|$x.name]`.
Window: `over(~a)`, `over(~[a,b], !c->descending(), [])`.

`#>{db.table}#` (three-segment `#>{db.schema.table}#` also supported) yields a
`Relation<T>` typed from the physical table — **no Mapping required**, only
`->from(runtime)`. This is a distinct compile and plan path.

---

## 4. User test harnesses

Six `TestableRunnerExtension` implementations are registered. Two are verified working
against our fixture; four are not yet exercised.

| Harness | Shape | Status |
|---|---|---|
| **Mapping** | `testSuites: [ s: { function:; tests: [ { data:; asserts: } ] } ]` | verified — 23 cases passing |
| **Function** | second brace block after the body | verified — 1 case passing |
| **Service** | `testSuites` with `data: [ connections: [...] ]`, `parameters:`, `serializationFormat:` | not built |
| **Persistence** | `tests: [ t: { testBatches: [ b: { data: { connection: {…} } } ] } ]` | not built |
| **DataQualityRelationValidation** | `###DataQualityValidation` + `validationTree` | not built |
| **DataQualityRelationComparison** | relation comparison | not built |

### Platform test forms — a different category

The split is clean and worth stating, because choosing the wrong one is a category error:

| Form | `src/main/resources` | `src/test/resources` |
|---|---|---|
| `<<test.Test>>` + `executeInDb` | **518 files** | **0** |
| `testSuites` | **0** | **94 files** |

`<<test.Test>>` is the **platform** corpus — engine's own tests, living in registered
code repositories, seeded by `createTablesAndFillDb()` with `executeInDb(...)`, asserted
with `assertEquals`/`assertSize`/`assertSameElements`. This is what legend-lite's
`RelationalCorpusRunner` consumes, and its stereotypes (`ExcludeAlloy`, `ExcludeLazy`,
`ExcludeModular`, `AlloyOnly`) are engine-execution-mode concerns, meaningless in a user
project.

`testSuites` is the **user** framework. Our corpus models a user project, so `testSuites`
is the correct format. PCT (`<<PCT.test>>`) is a third thing again — cross-store
behavioural parity.

### Assertions

| Assertion | Form | Status |
|---|---|---|
| `EqualToJson` | `EqualToJson #{ expected: ExternalFormat #{ contentType; data; }#; }#` | used |
| `EqualTo` | `=> 'literal'` | not used |
| `Relation` | `=> Relation #{ id, firstName \n 1, John; }#` — tabular, readable, avoids JSON-parity risk | not used |
| `(XML)` | `=> (XML) '…'` | not used |
| `EqualToTDS` | complete grammar, **no Java implementation in either repo** | **never emit** |

Only `EqualToGrammarParser` and `EqualToJsonGrammarParser` are registered as assertion
parsers; the flat `=>` forms are handled by the function/service grammars.

**`EqualToJson` comparison semantics** (the parity target, `TestAssertionHelper`):

```java
new ObjectMapper()
  .enable(DeserializationFeature.USE_BIG_DECIMAL_FOR_FLOATS)
  .setNodeFactory(JsonNodeFactory.withExactBigDecimals(true));

JsonNodeComparator.NULL_MISSING_EQUIVALENT_AND_UNORDERED_ARRAYS.compare(expected, actual) == 0
```

`null ≡ missing`, arrays unordered, exact BigDecimals. The "actual" side is
`((StreamingResult) result).flush(result.getSerializer(SerializationFormat.RAW))`.

### Embedded data kinds

Seven registered; we use two.

| Kind | Syntax | Status |
|---|---|---|
| Relational CSV | `Relational #{ schema.table: 'csv'; }#` | used |
| Reference | `Reference #{ data::Element }#` | used |
| **ModelStore** | `ModelStore #{ my::Person: [ ^my::Person(…) ] }#` — **the M2M path** | not used |
| ExternalFormat | `ExternalFormat #{ contentType; data; }#` | not used |
| Relation elements | `Relation #{ cols \n rows; }#` | not used |
| ServiceStore stubs | request/response stubbing | not used |
| DataspaceTestData | `DataspaceTestData #{ … }#` | not used |

---

## 5. Combination strategy

### Why high-order, not pairwise

Pairwise answers "does A break B". **Every confirmed defect here is an accumulation
bug**, and pairwise would have missed all of them:

- **E-1** needs traversal depth ≥ 14. Pairwise generates depth 2.
- **E-2** needs graph-fetch depth ≥ 16.
- **B-2** needs chained M2M **and** union **and** graph fetch — any two pass.
- **B-4** needs an association **and** an aggregate **and** an entity with zero children
  — any two pass.

### The stacking model — six layers

A feature stack vector picks slots from each layer. Two slots per layer yields
**~12 simultaneously-active features** per model, which is the target density.

| Layer | Slots | Choices |
|---|---|---|
| **L1 domain** | 6 | inheritance depth · constraint set (5 knobs × enforcement level) · derived property · qualified property with params · association kind (self / m:n / cross-package) · temporal stereotype · enum · measure+unit · precise primitive · property default · aggregation kind |
| **L2 store** | 5 | column-type set · view (`~filter`/`~groupBy`/`~distinct`) · join kind (simple / composite / self-`{target}` / dyna-function / and-or nest / `in()` / cross-db) · join chain + INNER/OUTER · Filter / MultiGrainFilter · milestoning spec · TabularFunction |
| **L3 mapping** | 6 | class-mapping kind · `~filter ~distinct ~groupBy` · `scope()` · embedded → inline → otherwise · enum mapping · local property `+p:` · explosion `p*:` · association mapping · include + store substitution · `extends [id]` |
| **L4 query** | 5 | TDS vs Relation · traversal depth · graph-fetch depth/branching/`subType` · aggregate kind · milestoned access · parameters · string/math/date functions · window/olap |
| **L5 harness** | 3 | harness (6) · assertion (5) · embedded data (7) · serialization format |
| **L6 data** | 4 | cardinality (0 / 1 / many) · NULL placement · duplicates & ties · orphans · torture values · volume |

### Ten named dense scenarios

Each is realistic — these are shapes real enterprise models have — and each is chosen
because the *stack*, not any pair, stresses the engine.

| # | Scenario | Features | Why the stack is dangerous |
|---|---|---|---|
| S1 | **Temporal union graph fetch** | bitemporal + `Operation` union over 2 sets + association traversal + graph fetch depth 3 + `subType` + `%latest` + embedded PM + enum mapping + constraint + orphans (10) | Milestoning dates must propagate through union set resolution, into a nested tree, across a subtype boundary. B-2 already shows union × graph fetch failing; temporal is a second independent rewrite over the same tree. |
| S2 | **Self-referential hierarchy with derived aggregates** | self-association + recursive graph fetch + derived property over the association + qualified property with param + aggregate over an **empty** collection + inheritance + `subType` filter + `{target}` self-join + NULL roots (10) | Combines the E-1/E-2 recursive-walk shape with B-4's empty-aggregate bug and self-join aliasing. Manager chains are in every real model. |
| S3 | **Embedded / inline / otherwise chain** | embedded nested 3 deep + `Inline[setId]` + `Otherwise` + join chain + `scope()` + local property + enum transformer inside embedded + `~primaryKey` inside embedded + `extends [id]` (9) | Arbitrarily deep grammar-legal recursion — exactly the shape that produced both exponentials — plus set-id resolution at every level. |
| S4 | **Cross-store diamond** | M2M over relational (`ModelChainConnection`) + XStore + ModelJoin + union on the source side + graph fetch + orphans + two runtimes + `ModelStore` data (8) | Chained stores measured at 2× cost (one extra doubling). Cross-store joins that can *miss*, plus set resolution across the chain. |
| S5 | **Aggregation-aware temporal rollup** | AggregationAware (a mapping nested in a mapping) + `~groupBy` + milestoning + View with `~groupBy` + MultiGrainFilter + aggregate over association + `~filter` + enum grouping key (8) | Aggregate rewriting must choose between main mapping and aggregate view *under* a temporal predicate — two independent rewrite systems over one query. |
| S6 | **Relation paradigm stack** | `#>{db.table}#` + `~[col specs]` + `extend`/`over`/window + `Relation` class mapping with `~func` + join + sort + `Relation #{}#` assertion + precise primitives (8) | An entirely separate compile and plan path, never executed once. Its own PK inference, its own function library. |
| S7 | **Constraint enforcement under polymorphism** | constraints with all 5 knobs + both enforcement levels + inheritance + `subType` + violating data + derived property *inside* the constraint + association traversal inside the constraint + milestoned class (8) | Constraints are compiled lambdas evaluated per instance. We declare them and have **never once seen one fail**. |
| S8 | **Type torture** | all 19 column types + precise primitives + Measures/Units + property defaults + enum + SEMISTRUCTURED with `Binding` + quoted identifiers + reserved words as column names + aggregation kinds (9) | Type coercion end-to-end: DDL → seed → SQL gen → result → serialization → assertion. Every layer can silently widen or truncate. |
| S9 | **Wide dependency graph** | 10 projects, diamond dependencies + cross-project inheritance + cross-project mapping + cross-project association + include with store substitution + duplicate element paths + mixed SNAPSHOT/versioned pointers (7) | Compilation ordering across projects, plus the measured 38× recompile tax. |
| S10 | **Service over everything** | Service multi-execution + `parameters:` + `serializationFormat:` + testSuites + `connections:` data + a query stacking S1 (12+) | The highest-level artifact on the deepest stack — closest to a real deployed service. |

### Systematic generation above the hand-picked set

1. **Declare validity rules** — mutually exclusive slot values (`~func` vs `~src` on a
   Relation mapping; milestoning on an M2M target class; `%latest` inside a graph-fetch
   tree, which currently NPEs).
2. **Generate covering arrays over _slots_, not features.** Slot-level *pairwise*
   already yields ~12 active features per model, because each slot contributes one.
   This is the key move: slot-level pairwise buys feature-level high-order for free.
3. **Force density** — reject any vector with fewer than 10 active features; regenerate.
4. **Sweep depth on the survivors** — run the Tier-3 continua on each dense model. This
   is what turns a dense model into an exponential detector.

### Automatic minimization — non-optional

A 12-feature model that fails tells you almost nothing. Without shrinking, high-order
combination testing produces unactionable failures and gets abandoned. The generator
must ship with a **delta-debugger**:

1. On failure, remove one feature and re-run.
2. Still failing → keep the removal; passing → restore.
3. Iterate to a fixed point — the result is a **minimal failing feature set**.

This is how B-2 became "chained M2M + union + graph fetch" rather than "8 models failed",
and how E-1 reduced to a single character. **Budget the minimizer alongside the
generator.**

### Budget split

- **40%** dense scenarios (S1–S10 + slot-level covering arrays, ≥10 features each)
- **25%** depth sweeps **on top of** dense models — the only exponential detector
- **20%** negative & boundary (constraint violations, empty collections, error paths)
- **15%** pairwise breadth for genuinely independent axes

Sweeping depth on a bare two-class model finds E-1; sweeping it on S1 finds E-1 *and*
whatever interacts with it.

---

## 5A. Scale targets — and how assertions survive them

### The targets

| Dimension | Target | Notes |
|---|---|---|
| Dense test cases | **10,000+** | every one asserting a value; ≥10 active features each |
| Distinct models | **1,000+** | generated from the layer/slot vectors of §5, not hand-written |
| Dependency graphs | **64 projects deep/wide** | diamonds, cross-project inheritance/mapping/association |
| Elements per project | 10 → 5,000 | up to ~100k total elements in the largest fixture |
| Features exercised | the full 221-tag / ~35-keyword worklist | §2 is the denominator; "done" is a fact |

### The contradiction, resolved

§0 says case count is not a quality metric. That is not an argument against scale — it
is an argument against *unasserted* scale. The previous corpus failed because 10,800
cases shared one worthless assertion and ~75% were duplicates. **10,000 cases each
asserting a computed value, each with ≥10 features live, is the goal.** The constraint
is per-case quality, not case count.

### The hard problem: you cannot hand-write 10,000 expectations

The current oracle is hand-written per case. That scaled to 24. Three mechanisms scale
to 10,000, and the corpus needs all three because they cover different things.

#### (a) Generative oracle — a reference evaluator

A small relational interpreter in Python, evaluating the *generated* query against the
*generated* fixture data. Not a Legend implementation — just enough algebra to predict a
result:

```
project · filter · sort · limit · distinct · groupBy/aggregate
join (inner/left, with the LEFT-OUTER semantics Legend actually emits)
association traversal · self-join · enum code→name translation
milestoning predicate (business/processing/bitemporal, %latest)
nested object construction for graph-fetch shapes
```

This is bounded work — a few hundred lines — and it yields **exact** expectations for
any query the generator can emit within that algebra. It is the workhorse: most of the
10,000 come from here.

Its limit is honest: it can only predict what it models. Complex mapping constructs
(AggregationAware rewrite, XStore, embedded/otherwise resolution) are exactly the things
we do not want to reimplement, because a reimplementation would encode our *belief*
about the semantics rather than the truth.

#### (b) Metamorphic relations — assertions without knowing the answer

For everything the reference evaluator cannot predict, assert **properties that must
hold regardless of the answer**. These scale without bound: any generated query yields
several checkable relations, and no expectation needs computing.

| Relation | Property |
|---|---|
| Identity filter | `filter(x\|true)` ≡ `all()` |
| Filter monotonicity | `count(filter(p))` ≤ `count(all())` |
| Filter composition | `filter(p)->filter(q)` ≡ `filter(p && q)` |
| Partition | `count(filter(p))` + `count(filter(!p))` ≡ `count(all())` — **catches three-valued-logic bugs when p is NULL-sensitive** |
| Distinct idempotence | `distinct()->distinct()` ≡ `distinct()` |
| Sort permutation | `sort(c)` multiset ≡ unsorted multiset |
| Limit bound | `count(limit(n))` ≡ `min(n, count(all()))` |
| Projection arity | `count(project([a,b]))` ≡ `count(project([a]))` (no implicit dedup) |
| Aggregate decomposition | Σ over groupBy groups ≡ aggregate over all |
| Traversal containment | `all().assoc` ⊆ target class `all()` |
| Graph-fetch prefix | tree at depth *d* contains the tree at depth *d−1* as a prefix |
| Milestoning containment | `all(%latest)` ⊆ `allVersions()` |
| Union additivity | disjoint union set counts sum to the union count |
| Enum round-trip | filtering on the enum value ≡ filtering on its source code |
| **Mapping invariance** | the same query through two semantically equivalent mappings ≡ same rows |
| **Paradigm invariance** | the same query expressed as TDS and as Relation ≡ same rows |
| **Dialect invariance** | the same query on H2 vs DuckDB vs Postgres ≡ same rows |

The last three are the highest-value: they need no oracle at all, they exercise the
deepest machinery, and a violation is unambiguously an engine defect. **B-4 would have
been caught by the partition relation**: `count(orders)` summed over people must equal
total orders, and with `COUNT(*)` over a LEFT JOIN it does not.

#### (c) Differential — two engines, one corpus

Run the identical corpus through legend-engine and legend-lite, land both into tables,
`EXCEPT ALL` both directions. Catches divergence without any expectation. Its limit is
equally honest: it cannot see a bug both engines share.

### Coverage split across the three

| Mechanism | Share of the 10,000 | Covers |
|---|---|---|
| Generative oracle | ~60% | the core query algebra, exactly |
| Metamorphic relations | ~30% | everything the evaluator cannot predict — complex mappings, graph-fetch shapes, dialects |
| Differential | ~10% | cross-engine conformance; also a cross-check on the other two |

Every case still asserts something. None of the three is `rows > 0`.

### Generating 1,000+ models without generating 1,000 duplicates

The v1 failure mode was 864 models of which ~75% were interchangeable. Guards:

1. **Reject low-density vectors** — fewer than 10 active features, regenerate.
2. **Reject query-invisible variation.** If two models differ only in features no query
   in their battery touches, they are one model. v1 generated 432 models with subtypes
   that no query referenced; that check alone would have caught it.
3. **Hash the feature vector**, not the model text — dedupe before emitting.
4. **Require each model to move a census counter** (§2) or be justified as a
   depth/stress variant of one that does.

### The scale ladder

| Tier | Models | Cases | Purpose |
|---|---|---|---|
| Smoke | ~20 | ~200 | every harness, every assertion kind — runs per commit |
| Core | ~200 | ~2,000 | the census worklist + S1–S10 dense scenarios — runs per PR |
| Full | ~1,000 | ~10,000 | slot-level covering arrays + depth sweeps + metamorphic — nightly |
| Stress | ~50 | — | dependency graphs to 64 projects, volume to 1M rows — weekly |

Runtime is the constraint on the Full tier: at the measured ~15 ms plan-gen and ~6 ms
execution for a simple case, 10,000 cases is roughly 4 minutes of engine time plus
compilation. Model **compilation** dominates — so batch cases by model (compile once,
run its whole battery), which the current runner already does.

---

## 5B. Base corpus + enrichment layers — quality first, scale by variation

### The correction

§5A's covering arrays produce *structurally* valid models with synthetic shape:
`Person0`, `r0_id`, five identical classes in a chain. They exercise features but look
like nothing anyone would build, and the interesting defects live in the shapes real
models actually take — denormalized reporting tables, partitioned history, slowly
changing dimensions, deep instrument hierarchies, the same domain mapped several ways.

We do not have to author that from scratch. `core/src/test/resources/stress/` already
holds a realistic multi-domain financial-services corpus. It is the base.

### What the base already is

| | |
|---|---|
| Files / lines | 46 / 7,428 |
| Domains | 20 — products, refdata, counterparty, org, positions, trading, pnl, risk, settlement, ops, collateral, sales, regulatory, marketdata, funding, accounting, clearing, tax, research, prime |
| Classes | **200**, ~15 properties each, domain-meaningful names (`notional`, `settlementDate`, `executionVenue`, `slippage`, `marketImpact`) |
| Associations | **182**, including a dedicated cross-domain file for multi-hop navigation |
| Mappings | 21, composed through an include closure (`stress::AllMapping`) |
| Stores | 20 store files over one `store::DB` |
| Services | 12, already using the **Relation** paradigm (`->project(~[...])`) |
| Enums | 4 |

That is realistic **breadth and topology** — genuinely hard to author by hand, and already
done.

### What the base does not have — the enrichment surface

A feature scan across all 46 files returns **zero** occurrences of: milestoning
(`temporal.*`), `Operation` union mappings, M2M (`: Pure`), `Relation` class mappings,
XStore, ModelJoin, AggregationAware, `Otherwise`, `Inline[`, `EnumerationMapping`,
`~groupBy`, `~filter`, `View`, `{target}` self-joins, `testSuites`. Also no functions, no
profiles, no Measures/Units, and the class mappings carry no `~primaryKey`.

So the base is **broad and shallow** — exactly the right starting point. Every feature we
need to cover is an *enrichment* of a realistic model rather than a synthetic fixture.

Several enrichments make the model **more** realistic, not less:

| Base today | Enrichment | Why it is also more realistic |
|---|---|---|
| `status: String[1]`, `side`, `tradeType`, `currency` as `String` | typed enums + `EnumerationMapping` with source codes | the enums (`OrderStatus`, `OrderType`) already exist and are unused; real warehouses store codes, not labels |
| refdata / products / counterparty as current-state tables | business-temporal milestoning (SCD2) | reference data is versioned in every real firm |
| one `TRADE` table | partitioned trade history + `Operation` union | trade history is partitioned by year everywhere |
| flat column mappings | a denormalized reporting table with embedded / inline / otherwise mappings | every firm has a wide reporting table |
| no aggregates | `View` with `~groupBy` for positions-by-book, plus AggregationAware | standard warehouse rollup |
| `org` hierarchy without self-reference | `{target}` self-join for manager / parent legal entity | org charts are recursive |
| no constraints | `notional == quantity * price`, `settlementDate >= tradeDate` | actual trade-capture validations |
| no derived properties | `netAmount()`, `isSettled()`, `positionAsOf(date)` | the questions users actually ask |
| amounts as bare `Float` | Measures/Units for currency | multi-currency is the domain |
| services without tests | `testSuites` on the existing 12 services | they are already the realistic query battery |

### Enrichment layers, in build order

Each layer is an increment of ~150–250 assertion-bearing cases — the iteration unit.
Each is validated (compiles, executes, asserted, mutation-tested, census-checked) before
the next begins.

| Layer | Content |
|---|---|
| **L0 — data + assertions on the base** | Realistic seed data with skew, NULLs, orphans, zero-child entities, ties. Oracle + metamorphic assertions over the existing 12 services and a wider battery. Proves the base end-to-end before changing it. |
| **L1 — types & rules** | Enums for the string-typed code columns + EnumerationMappings; constraints; derived and qualified properties; Measures/Units for currency. |
| **L2 — mapping depth** | A second mapping of the same domains: denormalized with embedded/inline/otherwise. **This is what makes mapping invariance testable.** |
| **L3 — temporal** | SCD2 milestoning on refdata/products/counterparty; bitemporal on one; `%latest`, `allVersions`, `allVersionsInRange`. |
| **L4 — partitioning & rollup** | Partitioned trade history + `Operation` union; Views with `~groupBy`/`~filter`/`~distinct`; AggregationAware. |
| **L5 — cross-store** | An M2M canonical→reporting layer; `ModelChainConnection`; XStore and ModelJoin to a "external party system" store. |
| **L6 — harnesses** | `testSuites` on the services; Function tests; Persistence and DataQuality packs. |

### Why plural mappings is the key idea

Once L2 lands, every query in the battery yields a free assertion:

```
query(normalizedMapping) ≡ query(denormalizedMapping)
```

No oracle, no expectation to compute, and a violation is unambiguously an engine defect.
It exercises embedded/inline/otherwise resolution, join-tree construction and union
dispatch against a known-good reference — precisely the parts we refuse to reimplement in
the generative oracle. Same argument for L4's partitioned-union mapping and L5's M2M
layer: each is another equivalent view of the same domain.

### Quality bar for anything authored on top

- Names would pass review in a real project — no `C0`, no `r0`.
- Every constraint encodes a rule someone could state in a sentence.
- Every derived property answers a question a user would actually ask.
- Each enrichment shows at least one of: denormalization, partitioning, SCD2, or a
  natural-vs-surrogate key split.
- Data has realistic cardinality skew — not uniform fan-out.
- At least two mappings of the same domain exist, so invariance is testable.

### How this reaches 10,000

20 domains × ~4 mappings × a 40-query battery ≈ 3,200 high-realism cases before any
generation. The covering arrays of §5A then layer *on top of* the enriched base —
varying dialect, harness, assertion kind and depth against realistic models rather than
synthetic ones. That is how the count grows without diluting quality.

---

## 6. Corpus architecture

```
domain/            canonical data + invariant self-checks (import-time assertions)
  ├ core.py        Firm/Person/Address/Order/Product — the realistic topology
  ├ variants.py    milestoned, union, subtype, xstore, relation-paradigm fixtures
  └ torture.py     quotes, commas, unicode, empty strings, extreme numerics

oracle/            expectations computed independently from domain data
  ├ tds.py         project / filter / sort / distinct / groupBy semantics
  ├ graph.py       nested graph-fetch object shapes
  └ errors.py      expected diagnostics for negative cases

emit/              portable .pure emitters, one per harness
  ├ mapping.py     testSuites + ###Data + EqualToJson
  ├ function.py    second brace block + flat assertions (+ Relation form)
  ├ service.py     Service testSuites, parameters, serializationFormat
  └ store.py       shared model/store/mapping emission

run/               execution + comparison
  ├ TestableMain   Legend's own TestableRunner (the portable path)
  ├ AssertRunner   direct compile→plan→execute (perf instrumentation)
  ├ DiffRunner     DB-side EXCEPT ALL for volume + cross-engine
  └ compare.py     JSON comparison matching EqualToJson semantics exactly

census/            coverage measurement — the gate, not a claim
  ├ roster.py      re-run the three probes; diff covered-set before vs after
  └ gate.py        fail the build if a wave lands without moving the counters

bench/             performance harness (§8)
mutate/            defect injection proving the battery can fail
```

The shipped artifact is always `.pure`. **Python is a build-time oracle only** — nothing
Python runs at test time, which is what makes the corpus portable.

### Two comparison sinks, deliberately

| Use | Comparison | Why |
|---|---|---|
| User-test harnesses | Jackson tree compare, exact `EqualToJson` semantics | compatibility is the requirement |
| Volume + cross-engine differential | `EXCEPT ALL` both directions in the database | typed, exact, multiplicity-preserving, scales to millions of rows, no serialization-parity risk |

For the differential case, materialize the expected side using the **actual result's
column metadata**, not a hand-written DDL — otherwise type coercion (DECIMAL vs DOUBLE,
DATE vs TIMESTAMP) produces spurious diffs.

### Portability status

| Piece | legend-engine | legend-lite |
|---|---|---|
| Model, store, mapping `.pure` | ✅ | ✅ **proven** — `Compiler.compileModel` returns a `PureModelContext` |
| `###Data` seeding | ✅ | ❌ parsed, not materialized |
| `testSuites` execution | ✅ 23/23 | ❌ parsed and re-emitted, no assertion executor |

To close it in legend-lite: **~250–350 lines, one class, ~1 day.** The expensive parts
already exist — `exec/CsvSeed.sqls(csvBlocks, dbFqn, ModelContext)` already generates
DDL+INSERT typed from the parsed store, and
`Compiler.executeResolved(ValueSpecification, ModelContext, runtime, Connection)` already
executes a resolved value spec. Missing: resolve `Reference #{ }#` → `PDataElement` →
`PRelationalCsvData` → `CsvSeed` (~40 lines); the suite/test orchestrator (~150); JSON
comparison matching the semantics above (~50); a result type (~30). Note mapping
`testSuites` are **not carried into the compiled model** (`FromProtocol.java:560,564`
drops Service suites to `"<suites>"`) — read suites from the **parsed protocol**, execute
against the **compiled `ModelContext`**. `CsvSeed` expects blocks as
`schema\ntable\nheader\nrows`, separated by a line of dashes, so `PRelationalCsvTable`
(which stores schema/table/values separately) must be reassembled.

---

## 7. Dependency & scale stress

Measured: editing one class costs 5.0 ms alone, 188.5 ms against 10,082 dependency
elements — **linear in total elements**, because dependencies are merged into one flat
graph and recompiled in full. There is no incremental compilation, and the
developer-iteration path is excluded from every cache (workspace contexts never cached;
SNAPSHOT versions disqualify caching; inline `PureModelContextData` never cached).

### Generator parameters

| Parameter | Range | What it isolates |
|---|---|---|
| projects P | 1 → 64 | per-project fixed cost vs element cost |
| elements per project E | 10 → 5,000 | the linear term |
| dependency depth D | 1 → 10 | does chain depth cost beyond total size? |
| fan-out F | 1 → 8 | wide dependency sets |
| **diamond width W** | 0 → 8 | **is a shared dependency compiled once or W times?** |
| cross-ref density X | 0% → 50% | edges, not elements — prerequisite discovery scales with references |
| cross-project inheritance | on/off | forces ordering across projects |
| cross-project mapping | on/off | mapping in A, class from B, store in C |

### Questions it must answer

1. **Is a diamond dependency compiled once or repeatedly?** Highest-value unknown.
   Sweep W with total element count held constant.
2. **Does edge density cost more than element count?** Hold elements fixed, vary
   references per element.
3. **What does context _shape_ cost?** Send identical content as (a) inline PMCD,
   (b) a combination of pointers, (c) one versioned pointer, (d) the same with
   `-SNAPSHOT`. Only (c) should get sub-millisecond repeats — this measures the cache
   cliff directly.
4. **Does DSL breadth cost?** Same element count across 1 vs 10 `###` sections.
5. **Does parallel compilation help?** The shipped server passes no `ForkJoinPool`.

---

## 8. Performance testing

### Stages, measured separately

Bundling hides everything. E-1 was invisible until `parseLambda` / `buildLambda` /
`generateExecutionPlanAsPure` were timed apart — plan-gen was flat while compile doubled
per hop.

```
parse(model) → compile(model) → parse(lambda) → compile(lambda)
  → route+plangen+sqlgen → serialize plan → deserialize plan → execute → serialize result
```

### Metrics per run

- **Wall time** per stage — `System.nanoTime`, median of N, best-of for cliff detection
- **Cold vs warm** — first iteration in a fresh JVM reported separately; the ratio is
  itself the metric (measured 64×)
- **Allocation** — JFR `ObjectAllocationSample`; the 1.37 GB/s steady-state rate was more
  diagnostic than CPU time
- **Classes loaded & JIT time** — `ClassLoadingMXBean`, `CompilationMXBean`; 19,554
  classes / 9 s JIT for one query was the cold-start story
- **Code cache & metaspace** — `MemoryPoolMXBean`
- **Rows & bytes** — normalise per row for execution comparisons

### Sweeps that matter

| Axis | Range | Expected shape | Alarm |
|---|---|---|---|
| **property-traversal depth** | 1 → 24 | linear after E-1 fix | ratio > 1.2 per hop |
| **graph-fetch tree depth** | 0 → 24 | linear after E-2 fix | ratio > 1.2 per level |
| graph-fetch node count | 1 → ~1,400 | linear; watch ms/node (was ~8 ms) | ms/node rising |
| projection width | 1 → 80 cols | linear | super-linear |
| dependency elements | 0 → 100k | linear (flat after caching) | any super-linearity |
| result rows | 1 → 1M | linear in execution, flat in plan-gen | plan-gen scaling with rows |
| model elements | 10 → 10k | linear — already confirmed | regression only |

The first two are the **regression tripwires**.

### Regression detection

1. Record a baseline per (stage, axis, point) — median of 7, plus machine fingerprint
   and load average.
2. **Alarm on ratio, not absolute time.** Absolute ms varies with hardware; the growth
   ratio between adjacent sweep points does not.
3. **Fail the build on shape change** (linear → super-linear) rather than on a
   percentage regression, which is noisy.
4. **Track allocation separately.** Allocation regressions precede latency regressions
   and are far less noisy.

### Benchmarking discipline — non-negotiable

- **Never time on a contended machine.** Check load average before and after; record it.
  Several numbers in the audit are inflated because analysis agents ran concurrently —
  the ratios survived, the absolutes did not.
- **Measure with `System.nanoTime` around the call.** JFR is for *attribution*, not wall
  time.
- **Warm up, then measure.** Report cold separately.
- **A/B a hypothesis before acting on it.** Three plausible CRITICAL findings did not
  survive an A/B.

---

> Cut in the 2026-11 docs cleanup: §9 the build order in waves 0 to 5 (2026-08-12). Full text:
> `git show de2745351:docs/TEST_CORPUS_MASTER_PLAN.md`.

---

## 10. Invariants

1. **Every case asserts a value.** Never `rows > 0`. If an expectation cannot be
   computed, the case does not ship.
2. **Expectations come from the oracle, not the engine.** Recording engine output as
   "expected" bakes in bugs. Where semantics are genuinely uncertain, establish them
   empirically once, write down the finding, then derive.
3. **Data must discriminate.** Every predicate matches some rows and excludes others —
   enforced by import-time assertions on the fixture.
4. **Mutation-test the battery.** Inject defects into the *model*; any mutation nothing
   catches is a hole. Query-level mutations are circular and excluded.
5. **Quarantine, never weaken.** A failing case whose expectation is correct gets
   quarantined with a defect id and reported upstream — never "fixed" by changing the
   assertion.
6. **Track sole guards.** Mutation testing showed three defects caught by exactly *one*
   case each (enum mapping, self-join direction, milestoning infinity date). They must
   not be deleted or merged during cleanup.
7. **The artifact is `.pure`.** Python is build-time only.
8. **Name the dark set.** Every coverage number ships with what it could not see.

---

## 11. Traps — already paid for

- **Pure operator precedence:** `&&` binds tighter than `<`, so
  `a > 0 && a < 150` parses as `((a > 0) && a) < 150`. Parenthesise every comparison
  inside a boolean expression.
- **Milestoned tables need composite PKs** `(id, from_z)`. Raw-SQL seeding hides this;
  framework-generated DDL does not.
- **`%latest` requires `INFINITY_DATE`** on the milestoning spec, and cannot mix with a
  concrete date in one property call — `(%latest, %latest)` or two concrete dates.
- **Function testable ids are mangled descriptors** — `model::Fn__TabularDataSet_1_`,
  not `model::Fn`.
- **Graph-fetch trees reject `%latest`** (B-1) — use concrete dates until fixed.
- **`EqualToTDS` is orphaned** — complete grammar, no implementation anywhere.
- **Joins, filters and views need schema-qualified table refs** when tables live in a
  named schema.
- **The JVM does not exit after `PlanExecutor` use** — non-daemon threads; batch runners
  need an explicit `System.exit`.
- **Type parameters `<T>` parse then hard-fail** in legend-engine; `native function`,
  `Primitive`, top-level `^instance` and `projects` are grammar-reachable but unhandled.

---

## 12. Defects and performance findings for upstream (2026-08-12)

> Cut in the 2026-11 docs cleanup: the table of what was built and passing on 2026-08-12. Full text:
> `git show de2745351:docs/TEST_CORPUS_MASTER_PLAN.md`.

Open defects found and quarantined: **B-1** (graph-fetch parser NPE on `%latest`),
**B-2** (graph fetch over union → cast exception), **B-3** (property traversal into a
union-mapped class), **B-4** (`count()` on empty to-many returns 1).

Performance findings for upstream: **E-1** (compiler exponential, root-caused to
`HelperValueSpecificationBuilder.processProperty` — line 186 compiles the sub-expression,
line 315 hands the *raw* protocol node to `buildFunctionExpression("map", …)` which
compiles it again, and the first result is discarded on that branch), **E-2**
(graph-fetch plan-gen exponential, localized to
`platformBinding/typeInfo/typeInfo.pure` — 47.5% inclusive, with
`CompiledSupport.concatenate` at 30% and `addAllIterable` the top self-time frame).

---

## References

- `docs/COVERAGE_CENSUS.md` — the three-axis parse/protocol census (history: `docs/HISTORY.md`)
- `docs/CONSTRUCT_COVERAGE.md` — the semantic/execution census and burn order (history: `docs/HISTORY.md`)
- `docs/RELATIONAL_FEATURE_MAP.md` — full-depth core_relational feature survey
- Probes: `ZFullRosterCensusProbe`, `ZKeywordCoverageProbe`, `ZPmcdReachabilityProbe`,
  `PmcdEquivalenceTest`, `CorpusCensusTest`
