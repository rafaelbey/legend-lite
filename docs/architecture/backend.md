# The backend: from a typed query to SQL and to a plan

This document covers what happens to a query after the frontend has typed it. Legacy mappings become functions
the compiler can type. Store resolution turns class queries into relational ones. The lowering builds a closed,
typed SQL IR. Each database's dialect writes that IR as SQL. Planning ends there, before any database is touched.

Neighbours, so nothing is said twice:

- [README.md](README.md): the overview, and the invariants the Rust port must keep.
- [frontend.md](frontend.md): text to typed model (lexing, parsing, name resolution, the element and expression
  compilers).
- [testing.md](testing.md): how all of this is judged.
- [docs/backends/](../backends/README.md): how each database behaves (types, spellings, quirks, probes).
- [docs/dialect-capability/README.md](../dialect-capability/README.md): the generated matrix of what each dialect
  spells, emulates or refuses, function by function.
- `AGENTS.md` and `core/README.md`: today's Java names, packages and the numbered invariants that code cites.

This document replaces `docs/pipeline-architecture.md`, which was written against the deleted `engine/` tree. Its
rules (a closed, dialect-free IR; all SQL in the dialect) are kept here; its migration plan for that tree is history.
Java classes are named only where they are today's implementation, and are marked as such.

## 1. Where the backend sits

| step | what it does | in → out |
|---|---|---|
| E | mapping normalization | the legacy mapping DSL → Pure functions, one per mapped set (§4) |
| G½ | user-call inlining | typed spec → typed spec with user function bodies spliced in |
| H | store resolution | logical typed spec (classes, properties, navigations) → physical typed spec (tables, columns, joins) (§5–§7) |
| I | lowering | typed spec → SQL IR (§3, §8, §9) |
| J | rendering | SQL IR → SQL text and its bound parameters, for one dialect (§10) |
| K | execution | a plan and a session → typed rows to a sink (§11) |

Steps A to G belong to [frontend.md](frontend.md), except mapping normalization (E), which is described here because
it is where the store side of the model takes shape. G½ runs on every execution path. Planning is everything up to J,
and it never touches a database (§11, C2a). The full step table, with today's packages and entry points, is in
`AGENTS.md` ("Pipeline — 11 steps, one driver").

A query's result shape (a relation, a graph of objects, a scalar, a collection) follows from the type at the plan's
root. It is decided once, there, and no per-operator lowering rule branches on it. Today's result model (one plan,
one result stream with sinks) is `docs/PLAN_AND_RESULT_MODEL_2026_10_18.md`.

Sources: `docs/pipeline-architecture.md` §1, `docs/PHASE_HIJ_LOWERING.md` ("Result shape"), `AGENTS.md` (git
history: docs/HISTORY.md).

## 2. Who owns what

Each layer is justified only if its boundary is sharp. The contract:

| layer | owns | forbidden |
|---|---|---|
| mapping normalizer (E) | legacy mapping DSL → synthesized functions, asking the compiled knowledge | mutating the model; keeping its own copy of the type system; running after the typer |
| store resolver (H) | logical → physical typed spec | re-running type checks; side tables beside the tree; letting a store-only node through |
| lowerer (I) | typed spec → SQL IR, through binding tables keyed by the resolved function | inferring types; naming SQL functions; SQL syntax; importing a dialect; any string field that encodes a SQL operation |
| dialect (J) | SQL IR → SQL text: every spelling and every shape; reading result values back | inferring types; rewriting the typed tree; consulting the model |
| runner (K) | a plan and a session → typed rows | everything above |

The full table, with the frontend layers and the judges, is `AGENTS.md` ("Layer ownership"). The lowerer reads
types from the typed tree's annotations; a missing type is a frontend bug, fixed in the frontend (`AGENTS.md`
invariants 1–2).

Sources: `docs/pipeline-architecture.md` §2, `AGENTS.md` (git history: docs/HISTORY.md).

## 3. The SQL IR

### 3.1 It is closed

The SQL IR is the target-independent intermediate form between the typed tree and SQL text. The analogy is a
compiler's: the typed tree is the source IR, the SQL IR is the backend IR, the dialect is code generation. Each
boundary is a typed surface, not a string gateway.

- **Every operation is its own record.** There is no `FunctionCall(name, args)` catch-all and no `Binary(left, op,
  right)` with a string operator. A new native is a new IR variant and a new render arm.
- **No string field encodes dispatch.** The single carve-out is a cast's target, which carries a *Pure* type name
  that the dialect maps to its own type name. Pure type names are not SQL.
- **The IR is pure data.** No IR type renders itself or references a dialect. Lambdas live in the IR as data; the IR
  never holds a Pure AST node.
- **Exhaustiveness is the test.** Every hierarchy is sealed, and each render method is a switch over it with no
  default arm, so the compiler fails the build when a variant has no arm. A dialect that cannot express a variant
  still has the arm, and the arm refuses by name.
- **Lowerings cannot leak SQL**, because the IR types have nowhere to put it.

`AGENTS.md` invariants 3 and 3a list today's sealed roots and mark which half of the rule a test enforces.

### 3.2 A node's type is part of the node

Every SQL IR expression carries its SQL type as an immutable component, computed once when the node is built:

- a composition's type comes from its function's typing rule over its children's stored types;
- a leaf (a column, a lambda parameter) is given its type by the builder, which always has it in hand (the
  projection it just built, the DDL, the source list's element type);
- an expression with no typing rule yet says so explicitly (`Unknown`); it is counted, never guessed. `Bottom` is the
  SQL NULL value and fits any nullable slot; `Raises` is an expression that never yields (an `error()`), and fits any
  slot.

There is no lazy memo and no side cache: a map of types kept beside the tree is mutable knowledge with its own
lifecycle, and the type is a property of the tree. This replaced a "judge" that walked finished trees to reconstruct
their types at consumption, which had structural blind spots (error branches, subselect boundaries, lambda
parameters: each a place where construction knowledge had been thrown away).

**Nullability is the second dimension of the same fact**, also computed at construction:

- a base column is non-null when its DDL says not-null or primary key, otherwise nullable (legend-engine's rule:
  unknown lowers to `[0..1]`);
- the padded side of a LEFT, RIGHT, FULL or ASOF join makes its columns nullable at the join's constructor;
- literals are non-null;
- a scalar call is nullable when any operand is, with probed exceptions (`IS NULL`, `EXISTS` and `COUNT` never;
  `COALESCE` only when all operands are; `CASE` when any branch is or the `ELSE` is missing);
- a reducer is nullable at its node (an empty input gives NULL) and is refined under `GROUP BY`, since SQL groups are
  non-empty; a scalar subquery over an ungrouped select that aggregates adds no nullability of its own, since such a
  select yields exactly one row.

`false` is a proof claim, so a NULL arriving under a non-null label is a compiler bug, pinned at zero on the lanes.
Pure multiplicity is untouched by this: `[1]` stays the Pure contract, and the SQL layer stops echoing it as a fact
about NULLs.

**Output columns** carry their name, type, nullability, and where their name was born: a physical name from the DDL
spells bare unless it needs quoting, a derived name the query invents is always quoted (the engine's convention). Result layers that need Pure types read them
from the typed root, never from the plan.

### 3.3 Model reasoning goes by the store; touching values goes by the wire

- **The store.** Typing, lowering and the emitted SQL come from the declarations, as legend-engine's own SQL
  generation does. A test fixture that contradicts the store is never adopted into the model or the plan.
- **The wire.** An output slot holds what its expression computes. The executor decodes what the driver returns.
  Only a judge, which writes SQL about values, reads the type the database reports, on its own copy of the plan,
  never written back.
- **Disagreements are counted, never tolerated by a tag.** Declared kind against slot (a `String` property over an
  `INT` column is legitimate) is one census; slot against driver metadata ("your test database does not match your
  store") is another.

An earlier design kept the declared type on the slot and marked mismatches "tolerated"; that tag was deleted on
2026-09-19.

### 3.4 Multiplicity stamps stay true

A Pure multiplicity stamp must stay true through every rewrite. At lowering, an expression's multiplicity must match
the provable shape of its SQL: a to-one stamp never lowers to a list, a many stamp never to a definite scalar, and no
multiplicity variable survives to lowering. A rewrite that changes a count does so with an explicit emission, never
by leaving a stale stamp. Consumers can then trust static types instead of sniffing runtime shapes, which is what the
check made possible to delete.

- **SQL NULL is the designed carrier for Pure `[]`**, in scalar and in list positions. A check must exclude it, or it
  counts the convention itself as a lie (the first run counted it 62,460 times).
- **`toOne` is split by provenance.** A user's `toOne` over a list-producing expression lowers to a checked extract:
  more than one element raises Pure's "Cannot cast a collection of size N to multiplicity [1]", one extracts, none
  flows as NULL. A `toOne` the resolver synthesizes to conform a type rides through, marked as such. A subquery that
  reads values is stamped `[*]` where it is produced.
- **A few (stamp, carrier) pairs are designed, not lies**, each an adjudicated row: a relation-typed node (the stamp
  describes the relation value, the SQL its row carrier); a constructed instance carried as a struct; a platform
  `List<T>` (one object, carried as a SQL array); a many-stamped property read whose SQL is scalar per row.

The check runs after lowering, always on, and throws on a provable mismatch (today's Java `StampCensus`).

Sources: `docs/pipeline-architecture.md` §3, `docs/TYPED_SQL_IR.md`, `docs/WIRE_SLOT_HOMEWORK_2026_09_19.md`,
`docs/STAMP_DISCIPLINE_PROGRAM.md`; current code `sql/TypeFact.java`, `sql/OutputCol.java` (git history:
docs/HISTORY.md).

## 4. Mapping normalization: mappings become functions

A legacy mapping (`###Mapping` with class mappings, property mappings, joins, unions) is normalized into Pure
functions before anything is typed. From there the mapping DSL is dead: the rest of the pipeline sees ordinary
functions and the facts stamped on them.

### 4.1 Knowledge first, then normalization

Knowledge (classes, properties, types, supertypes, enumerations, stores with their includes resolved) is compiled
first, once, into one immutable index, before mappings are normalized. The normalizer asks that compiled knowledge
the same fixed set of questions the compiler asks, with one implementation per question. It does not keep a shadow
type system, re-walk includes, or write into the model index.

The reorder loses no semantics, because normalization only adds to knowledge (the functions it synthesizes, the
compiled mappings); it never changes a class. Facts are computed once, in dependency order, and stamped on the
artifacts. Model validation that knowledge alone can answer (property-mapping names against class properties,
model-to-model route cycles) runs before normalization, with the same strict/tolerant split as the rest of the
model build: a strict build rejects the model, a module build poisons the element and goes on.

### 4.2 One function per set

Each mapped set becomes exactly one function, never one per queried mapping. A set's function reaches another class
only through `Class.all()`, and the store resolver answers that call under the mapping being queried. So a
navigation written in one mapping works unchanged when another mapping includes it: which set answers is decided at
query time, as the engine decides it.

Include semantics are legend-pure's and legend-engine's, read at the pin (`functions_Mapping.pure`,
`MappingValidator.java`). A class's root set is found by collecting the includes' answers first (recursively, in
include order), then the mapping's own, and the last root wins: the mapping's own root beats every include, and a
later include beats an earlier one. A class mapped in several included mappings is legal. Duplicate set ids across
the include closure, and duplicate includes, are rejected. A navigation's target set is resolved in the closure of
the mapping being executed, not the one that declared the property mapping.

### 4.3 Join property mappings are route lists; a union is a stack

A class-typed property mapped through a join becomes a navigation step whose routes each name the target set's own
function, the rows it reads, and the join condition as the author wrote it:
`legacyNavigate(~firm: [route(<target set's function>, <rows>, {s, t | <condition>}), ...])`. A single pinned
route is a list of one; a route through a middle table carries the middle table in its rows and its first hop as the
condition.

A union of a class's sets **is** a stack of the members' functions: `Operation { concatenate(p1(), p2()) }`. That is
what Pure's union means: a list of objects from both, each still belonging to the set it came from. One law governs
it:

> **Any operation on a stack is that operation on each arm, stacked.** Navigation, filtering, identity, graph fetch,
> a union inside a union.

`Operation` is only the binding's kind tag ("this class's function is a composition"). There is no union operator
users call, and no pre-built union object with helper columns. One builder makes every union (today's Java
`resolver/StackBuilder`): each arm projects the class's properties by its own bindings (a typed NULL where it has
none), and every arm keeps its own primary key, because identity needs the set (two sets may carry the same id for
different objects) and one of the join shapes joins on it (§7).

This replaced a design in which a union learned who navigated to it and published a key named after each navigator.
That meant an included mapping's union had to be regenerated for every mapping that added a navigator, and that a
navigating class's function depended on which mapping declared the union.

### 4.4 A view is a function

Each `View` of each `Database` is lifted at normalization into a zero-argument function `<db>::<view>(): Relation`
(`<db>::<schema>::<view>` for a view in a schema) whose body is the view's relation expression. Every consumer
resolves that function by name: the relation accessor `#>{db.view}#` types a call to it and the user-call inliner
inlines it; a class mapped on a view is a class mapped on a relation function; the resolver, the lineage and the
test-data generator read the same function. The raw view definition stays only as the schema record, as a table's
does. A view's main table is one store fact on the index (the engine's `findMainTableForView`).

Upstream's rule, read from source: a view is a mapping specification. The engine plans it with the same function
that plans a class mapping and emits it as an inline derived table aliased by the view's name; milestoning applies
inside the view's select from the outer query's context. The engine never creates a database view: a Legend view is
virtual by contract.

Sources: `docs/T4_1_KNOWLEDGE_BEFORE_NORMALIZATION_2026_09_13.md`, `docs/NORMALIZER_CLEAN_SHEET_HOMEWORK_2026_09_13.md`,
`docs/LEGACY_ROUTES_AS_COMPOSITION_2026_09_13.md` (§9, §11), `docs/VIEWS_COMPILED_ONCE_HOMEWORK_2026_09_22.md` (§5, §7,
§8f) (git history: docs/HISTORY.md).

## 5. Store resolution

### 5.1 A rewrite of the typed tree, not a side table

Store resolution is a pure typed-tree → typed-tree rewrite. Its input has class fetches, logical property names and
association navigations; its output has table references, physical columns and explicit joins, and nothing logical
remains. The resolution lives in the returned tree. No table of facts keyed by node identity is kept beside it: the
engine-era resolver did exactly that, and it bred a family of stale-stamp bugs. Lowering therefore reads everything
off the tree and infers nothing.

It is the same kind of pass as user-call inlining. Both expand an abstraction over a body: `Person.all()` and a call
to a user function are each replaced by a body spliced in with hygiene (fresh variable names, capture-avoiding
substitution). A class fetch is memoized per class and cycle-guarded, since a class may reach itself through a
self-join. Today these are two steps (G½ inlines user calls, H expands class fetches), and that kinship is why no class fetch, user call or other
store-only node may survive resolution (`core/README.md` invariants 9–10, `AGENTS.md` invariant 7).

Resolution scans before it rewrites. The demand scan over the whole operator chain completes first, the final row
types are known, and only then are bodies materialized and substituted, each node built with its correct type. There
is no restamping pass.

### 5.2 The mapping body is a binding table

A set's synthesized function ends in `map(row | ^Class(prop = expr, ...))`. That constructor survives resolution only
beneath a serialization boundary (a graph fetch, a serialize, a graph-shaped root). Every relational consumer
(project, filter, sort, group-by, a scalar `map`) reads it as a property → typed-expression binding table: the resolver
never builds an object (no JSON, no struct packing) that a downstream operator would immediately flatten. A user's
`->map(p | $p.name)` over instances is a one-column projection through the bindings.

Order: the mapping's `~filter`, then the user's filter, then the user's projection. Before the projection, `$p.prop`
resolves through the bindings; after it, columns are literal names.

### 5.3 Mapping-implied joins are access paths

- A join implied by the mapping (an association navigation, a `@Join` chain, an embedded mapping's fallback) is
  emitted only when a consumed expression reads through it. Demand is collected across the whole operator chain and
  joins are deduplicated by navigation path, so a filter and a projection that both read `$p.employer` share one
  join; longer paths chain off shorter ones.
- Joins that define the row set are exempt: those feeding a mapping `~filter` or a mapping `~groupBy` are demanded by
  the pipeline itself. A join the user wrote is never touched.
- Every navigation join is LEFT, with two exceptions: a mapping's INNER `~filter` makes the class source a filtered
  subselect, and a filtered navigation read in value position joins INNER (§6).
- An embedded binding is a column read on the parent's alias, with no join. An "otherwise" binding dispatches per
  leaf: an embedded leaf reads the parent; any other leaf takes the join.
- The lowering treats every join it receives as required; elision is the resolver's alone.

Upstream facts behind this (legend-engine `pureToSQLQuery.pure` at the pin): each projected column or filter operand
builds its own thread of joins, and threads merge joins by identity, so two properties through one association are
one join. The engine never drops a placed join, never removes a DISTINCT by key reasoning, and has no foreign-key
shortcut (reading `$p.firm.id` off the source's foreign key without joining). Such a shortcut is sound only for a
simple equality join to the target's sole primary key when the target has no `~filter`, `~distinct` or `~groupBy` (a
dangling key to a filtered-out row must give NULL, as the LEFT JOIN does). It is not built.

An enumeration compared in filter position is translated to its source values (`$p.status == Status.ACTIVE` becomes
`CODE = 'A'`, or `IN (...)`), as the engine does; in projection position the enumeration mapping stays a `CASE`.

### 5.4 Misses are loud

An unmapped class or property is an error naming the property, the class and the mapping. lite's engine-era resolver
and plan generator instead fell back silently to the property's name as a column; that fallback is not carried
forward. A lookup is always keyed by the receiving class, never a search of every store by property name (cross-class
name collisions would resolve arbitrarily). A cycle through an association target already being resolved
instantiates that target shallowly (its main pipeline and scalar bindings; deeper joins load on continued demand);
any other cycle throws with the cycle printed. A class query with no execution context is a loud error too, where
legend-engine answers an empty set: a forgotten `->from()` must not return `[]` (the one exception is the metamodel,
§5.5).

### 5.5 The metamodel lives in the database

`Class.all()` and the other reflection reads are ordinary mapped-class queries over system tables
(`metamodel.classes` and its siblings), seeded from the compiler's own registry and run through the normal store
path. The engine's one routing rule, "mapped goes to the store", then covers metamodel and user classes alike, with no
special case in dispatch.

- The system database and its mapping are fixed Pure source, appended to every model build, so the resolver sees
  ordinary parsed elements.
- A class query with no execution context whose every class is a tracked metamodel class takes the system mapping
  and runtime. Any other class query with no context keeps the loud error.
- A class value's identity is its fully qualified name; the simple name is its print form.
- The seed is a pure function of the active model and is written at the one place where execution setup runs.

Upstream: the Pure runtime has no metaclass list. Every compiled element is an instance with a classifier, one index
groups instances by classifier, and `X.all()` is one lookup, so `Class.all()` works because class definitions are the
instances whose classifier is `Class`. The engine's relational lane cannot run it (its DuckDB PCT manifest excludes
`getAll::testBasic`); the Java Pure runtime answers it. lite runs both halves through one database. A host-side
compile-time fold was rejected as a hedge.

Sources: `docs/PHASE_H2_H3_RESOLVER_PLAN.md`, `.windsurf/plans/mapping-resolver-as-rewrite.md` (North Star, "Unified
Inliner"), `docs/METAMODEL_STORE_HANDOFF.md`, current code `resolver/package-info.java` (git history:
docs/HISTORY.md).

## 6. Navigation: two forms, chosen by syntax

A mapped navigation compiles only to the engine's row algebra, in exactly one of two forms. The form is decided by
the expression's syntax alone, with no look at the head, the topology or the mapping:

| the consumption | the form | rows |
|---|---|---|
| not an explicit reducer call | **fanned join read**: a plain chain over the (possibly filtered) joined head; the surrounding computation is row-wise | one row per surviving joined row |
| an explicit reducer (`sum`, `max`, `count`, `joinStrings`, ..., and one-argument collection `plus`, which is `sum`) | **grouped join read**: a grouped subselect joined back by key (the engine's sub-aggregation shape) | one row per root |

Infix `a + b` is a scalar operator and always takes the fanned form. Downstream passes read this structure; they
classify nothing. A shape the materializer cannot build fails loudly; there is no fallback to a correlated scalar
subquery, which this design removed (it also computed wrong rows: a concatenation's NULL-skipping minted phantom rows
for firms that matched nothing).

- **Filtered reads fan in every position, filter position included.** An emptiness test (`isEmpty`, `isNotEmpty`,
  `exists`) stays a semi-join, by its meaning.
- **Placement of a navigation's own predicate is a construction-time fact of the join**: row-preserving or
  row-dropping. In value position the predicate stays in the joined target and the join is INNER. That is
  row-identical to the engine's LEFT JOIN plus top-level WHERE on every measured case, and correct on the case where
  the predicate is NULL-safe (there the engine's own placements differ in rows: they are row-equivalent only for
  NULL-rejecting predicates).
- **A chained head** (a navigation through a middle table) bundles the middle table and the target inside each
  filtered occurrence's own join subselect, as the engine does; sharing one middle join across occurrences
  cross-multiplies rows.
- **The graph-fetch lane** decorrelates the same way: a correlated reducer becomes a grouped subselect LEFT-joined on
  its keys, with a zero count through `COALESCE`. A shape that cannot be proven keeps the correlated form, counted.

The engine's own chooser is a tree-shape heuristic, not syntax-directed. The syntax rule is lite's, justified by the
measured row equality of the placements, not by a claim that the engine does the same.

Sources: `docs/NAV_ROUTING_DESIGN_4AD_SLICE1.md` (§1, §3, §7, §9) (git history: docs/HISTORY.md).

## 7. Navigating to or from a union

The law of §4.3 says one join per arm. The engine joins once above the union, and the measurements say why: on
DuckDB, for one union family, a single join on one merged key took 0.8 ms (hash join), a coalesced key 1.5 ms (hash
join), and an OR of per-arm equalities 8.9 ms (nested loop). So lowering a navigation whose source is a stack is a pass
with a few shapes, chosen only by the arms' typed conditions and the dialect:

| shape | when | what is emitted |
|---|---|---|
| 1. uniform | every arm's condition has the same shape | one join above the stack on one key column, each arm projecting its own read under that name |
| 2. non-uniform equalities | the conditions differ but each is an equality against the same source expression | one join with per-arm key columns, `coalesce` over them where the left side is shared, else an OR of the equalities |
| 3. push into the arm | arms are reached through chains of different lengths | each arm rooted at its chain's middle table, the rest of the chain joined inside the arm, the first hop's column projected as the arm's key, then shape 2 above |
| 4. bridge union | the dialect's choice (the engine's default on Snowflake) | a bridge `UNION ALL` of matching primary-key pairs, then two plain equalities; needs both sides' primary keys |
| 5. general | a condition that is not an equality | the law literally: the join inside each arm, results stacked (row-equal, slowest) |

A routed navigation into a union is the same pass seen from the other side. Shape 4 is not built in lite.

Upstream spells a union as `UNION ALL` of its sets, every leg carrying every set's primary-key columns (`pk_<i>_<j>`,
its own filled and the others NULL), and a join from a union as an OR over the per-set conditions. Its post-processor
`removeUnionOrJoins` (on by default for Snowflake, otherwise by the connection's `GenerationFeaturesConfig`) rewrites
that OR into the bridge of shape 4. On a hash-join engine the bridge is not faster: measured on DuckDB at 20k firms
and 400k people, the OR took 2.77 s, the coalesced key 3.8–5.0 ms, the bridge 10.5 ms (four hash joins). The corpus
rows that assert the bridge's marker column (`union_gen_source_pk_0`) are therefore treated as text contracts; their
rows pass on lite's form.

Pushing a join into the legs in general is wrong: an outer join pushed into each leg yields a NULL row per
non-matching leg, everything above the root would have to be repeated per leg, and the per-set conditions may differ.

Sources: `docs/LEGACY_ROUTES_AS_COMPOSITION_2026_09_13.md` §9 (2026-09-14, the newer source),
`docs/UNION_OR_JOIN_REMOVAL_DESIGN_2026_09_13.md` §1, §7, §11, §12 (git history: docs/HISTORY.md).

## 8. Lowering: lean, readable SQL

### 8.1 The tenet

Generate lean SQL that a person would write. One `SELECT` per fold-compatible run of operators; a subquery only when
an operator cannot fold into clauses already occupied; joins inlined. Flatness is the IR's natural state, not a
printer's afterthought.

### 8.2 One fold authority

Whether an operator folds into the current `SELECT` or isolates it is decided by one authority, not re-derived per
operator. The core rule follows SQL's fixed clause order (FROM, WHERE, GROUP BY, HAVING, window, QUALIFY, SELECT,
DISTINCT, ORDER BY, LIMIT): an operator folds when every clause it writes is still free at its position, and wraps
otherwise (the formalization Apache Calcite's `RelToSqlConverter` uses). Where Calcite would wrap, legend-engine
re-places, and so does lite: a filter over an aggregate goes to `HAVING`, a filter over a window column to `QUALIFY`,
and a chain of renames collapses. A window extend always isolates its source; a join's sides are isolated only when
they are not already table-shaped.

Leanness has two measured dimensions: **flatness** (the number of `SELECT`s and joins, pinned in golden tests in the
style of the engine's own flat-SQL tests) and **join elision**, which belongs to the resolver (§5.3), never to the
fold.

### 8.3 Dispatch

The lowering is an exhaustive switch over the typed tree. Scalar natives dispatch through binding tables keyed by the
resolved function's identity, never by name strings; a resolved overload with no binding throws (`AGENTS.md`
invariant 4: no fallbacks). Table aliases come from one deterministic counter (`t0`, `t1`, ...).

### 8.4 The standard for a lowering change

Each change to lowering is judged two ways:

1. **Nothing that passed stops passing.** The corpus rosters and the PCT lanes are the floor
   ([testing.md](testing.md)).
2. **The generated SQL stays readable.** Byte-identical SQL is *not* the standard: a change that inlines what was
   wrapped, or drops a redundant projection, is welcome. A previously inlined expression that now wraps in a
   subquery, or a join condition that splits across a CTE for no reason, is a regression and blocks the change.

Rows are the contract; golden SQL text is advisory.

The first lowering design's list of per-function SQL semantics (string index bases, numeric rules, and the like) is
not carried here: parts of it were reversed later. Today's rules are `docs/INDEXOF_SUBSTRING_LANE_CENSUS.md`,
`docs/NUMERIC_CHARTER_2026_09_17.md` and `docs/SEMANTICS_REGISTER.md`.

Sources: `docs/PHASE_HIJ_LOWERING.md` (Tenet, "The fold policy", "Lowering rules + dispatch"),
`docs/pipeline-architecture.md` §7 (git history: docs/HISTORY.md).

### 8.5 NULL semantics follow the oracle, use by use

NULL handling in the emitted SQL is decided by legend-engine's emissions, not by a blanket rule. A 2026-09 compiler
design proposed rendering `[0..1] == [0..1]` as `IS NOT DISTINCT FROM` everywhere and making an empty aggregate yield
Pure's result. The audit of that design rejected it for parity: the engine emits a bare `=` and a plain `SUM`, and
`IS NOT DISTINCT FROM` appears in no golden. The oracle decides, so the lowering keeps its per-use rules. In today's
code those are: the engine's null-safe equality for a user's equality over two optional operands; a plain `=` in a
mapping's own definition (its join conditions and correlation filters); and the engine's NULL compensation for
negated predicates (`processNotEqual`, `processNotIn`). These rules live in today's Java `lowering/NullSemantics`.

Sources: `docs/PLAN_AUDIT_2026_09_26.md` §2 ("The lowering design", "Null semantics vs parity", citing
`Scalars.java:83-130` and `NullSemantics.verbatim`), current code `lowering/NullSemantics.java` (git history:
docs/HISTORY.md).

## 9. Collections and carriers

### 9.1 Semantic nodes, rewritten per dialect

The lowering emits backend-neutral semantic nodes for collection constructs and never spells a backend idiom:

| node | meaning | DuckDB's native form, for comparison |
|---|---|---|
| collection source | a collection used as a row source | `UNNEST(...)`, an array literal scan |
| sub-aggregate | a per-group reduction of a to-many path | `list(...)`, ordered list aggregates |
| membership | `in()` over a collection value | `LIST_CONTAINS`, an array in `IN` |
| collection value | a list-typed intermediate | array literals, `LIST_GET`, `LIST_CONCAT` |

A strategy pass in the dialect layer, selected by the dialect's declared capabilities, rewrites each node before the
SQL is written. There are three strategy families:

1. **native lists** (DuckDB);
2. **one JSON carrier** for arrays (Postgres, SQLite, MariaDB): build with the `json_agg` family, explode with the
   `json_each` family; native arrays are banned as the carrier there, because jagged nested lists gave three measured
   silent wrong answers;
3. **relational fusion** (H2, the outlier with no correlated explosion): the engine's portable shapes, such as a
   grouped subselect joined by keys, literal `IN` or join membership, and `VALUES` or `UNION` literals.

Rules:

- **One compiler.** A construct never exists in two forms. Each dialect's rule for a node reproduces that dialect's
  emission, and the direct emission is deleted when the node lands.
- **The database computes.** A portable rule may change the SQL's shape; it never moves a value's computation to the
  host.
- **No rule means a typed wall.** A node a dialect has no rule for refuses with a named capability error, counted.
  It never falls back to host-side computation.
- A node upstream of the strategy pass that presupposes one backend's data model (list values and their functions,
  JSON-array composites) is a violation. A shrink-only guard counts them (today's `CarrierPurityRatchetTest`); the
  target is zero.

### 9.2 Values with no single SQL type

Some values have kinds no single SQL column type can hold: a mixed selection's winner (the `max` of mixed Integers and
Floats), a mixed list. They travel as **tagged values**: a tag column naming the winner's slot, and one typed column
per kind, so the kind survives as a type and never as a spelling. A mixed list is a list of tagged values.

The rule is the result boundary program's R6: a value SQL has no type for rides as a typed encoding with a plan
fact, never as a spelling. It replaced (2026-10-29) the earlier LITERAL carrier, in which a cell held Pure literal
spellings (`1`, `1.0`, `3.14D`, `'hello'`, `true`, `%2014-01-01`), disjoint by construction, so the spelling was the
kind tag. One rule of that design outlives it: the `JSON` type is reserved for genuine Variant values and is never
borrowed as an internal carrier. The borrowing it cured had made a Date and the string `'2014-01-01'` the same bytes,
so the engine's `equal('2014-01-01', %2014-01-01)` (false) could not be decided.

### 9.3 Parameters

A precompiled query is a typed plan, not a text template. Each parameter position carries its declared Pure type,
multiplicity and enumeration-ness; each dialect renders the plan with real bind placeholders, and execution binds
values. A collection parameter is the same semantic node with its value arriving at bind time. How the dialect writes
SQL and its parameters into one writer, in placeholder order, and the one designed hole the runner fills before it
prepares a statement, are `AGENTS.md` invariant 3 and `docs/EXECUTION_PLAN_BOUNDARY_2026_10_05.md` §9–§10.

Sources: `docs/CARRIER_REDESIGN.md` (§0–§2b), `docs/F10_CARRIER_DESIGN.md` (§1–§2),
`docs/RESULT_BOUNDARY_PROGRAM_2026_10_26.md` (R6, track L), current code `sql/SqlType.java` (git history:
docs/HISTORY.md).

## 10. The dialect layer: one owner per database

### 10.1 The dialect owns spelling, shape and reading back

The IR and the lowering carry meaning, in Pure's conventions. Each dialect owns everything a database decides:

- **lexical**: identifier quoting and case folding, literals, cast type names, function and operator spellings, limit
  syntax;
- **idioms**: JSON access, list operations, NULL-aware equality, modulo and rounding emulations;
- **structural capabilities**: `QUALIFY` natively or by wrapping, `PIVOT` natively or by `CASE` aggregates, ASOF
  natively or by emulation, where `UNNEST` goes, FULL and RIGHT joins; a construct that is impossible refuses loudly;
- **reading back**: the driver's type names (`wireTypeName`) and its values (`normalize`), since a driver may return
  a date as a string or a boolean as an integer.

The IR's *meanings* are the platform's written contract (typing rules, slice ends, date-format tokens), and every
dialect emulates them; only database strings were removed from the IR (date parts, JSON kinds and type names are typed
values now, and a join's kind no longer carries its keyword).

### 10.2 Adding a database is three units and three registry lines

Every database decision lives in one of three units of that database, and each unit kind has one exhaustive registry:

| unit | owns | registry |
|---|---|---|
| its SQL dialect (plan side, no driver) | all SQL text (queries, DDL, DML, session setup), names, keywords, capabilities, catalog queries, reading driver type names and values back | `Databases.dialect(type)` |
| its connector (execution side) | the driver's product name, the URL from a connection spec, the in-process private instance, bulk loading, cell decoding, session facts | `Connectors.of(type)` |
| its test backend (harness) | starting the server, a fresh workspace, fixture isolation, the referee's reader, the Pure connection block, roster names | `TestBackends.of(type)` |

A guard bans a database's name anywhere else (today's `DialectLeakRatchetTest` and `DialectOwnerExclusivityTest`).
The decisions that shaped it (the user's, 2026-10-13 to 2026-10-18):

- **Vocabulary is not a decision.** The `DatabaseType` enum, the connection grammar and the protocol name databases
  because the user's language does. They may parse and carry a database's name and never decide anything by it. An
  SQL input language (`#SQL{...}#`'s Postgres, `#IQ{...}#`'s IQ) is the owner of its own language under the same
  guard.
- **Copy or refuse.** A dialect that used to inherit DuckDB's spelling silently either gets the spelling copied in,
  where its PCT lanes prove it works, or refuses by name.
- **Raw SQL is in the language of the declared database.** It is translated only when it runs on another (an H2 test
  connection run on DuckDB) and passes untouched otherwise, as legend-engine passes `executeInDb` text.
- **The base renderer has no spellings.** Each concrete dialect is an exhaustive switch, families included, and the
  capability matrix is generated from the dialects' own tables
  ([docs/dialect-capability/README.md](../dialect-capability/README.md)).
- **Postgres folds a store's bare names to lower case**, as legend-engine does (it emits them unquoted).
- **legend-engine's table of which database supports which function** stays reference data, verified against the
  engine's source; lite's own answers come from each dialect's capabilities, so the two cannot disagree.

What each database actually does, and the probes behind it, is [docs/backends/](../backends/README.md). Which
dialects already render through the Rust port's library is in [README.md](README.md) §2.

Sources: `docs/PHASE_HIJ_LOWERING.md` ("Dialect architecture", 2026-07-05), `docs/DIALECT_LEAK_CENSUS_AND_PLAN_2026_10_12.md`
(§4, §6, Phase 2, Phase 4) (git history: docs/HISTORY.md).

### 10.3 Capability belongs to the dialect, not to a declaration

Whether a database can run a construct is not a column on a function's declaration. Capability is a property of the
dialect, and it can depend on shape (the audit's example: on H2, a lateral expansion works only over a literal
collection). The
2026-09-26 audit put it on the dialect as a `supports(function | structural node)` question, asked by one walk of the
SQL IR after lowering and before rendering. That gives a lowering error before any SQL is written, without pretending
capability is per declaration. As of this writing no such walk exists in `sql/` or `lowering/`: a refusal is still
thrown at render time, by name, from the dialect's arm (`DialectCapability`). Each dialect records its capabilities
in its own tables, which generate the capability matrix (§10.2).

Sources: `docs/PLAN_AUDIT_2026_09_26.md` §2 ("The lowering design", "Dialect capability is not a declaration column"),
current code `sql/dialect/DialectCapability.java` (git history: docs/HISTORY.md).

## 11. Planning and execution

### 11.1 Planning never touches a database (C2a)

The planner has no execution dependency, and the build enforces it, not class loading: the planner's library depends
on no execution library. A plan-only consumer (the browser's WebAssembly planner, `generatePlan`) depends on the plan
side alone (`//core:plan_side`). Execution reaches the planner only through its public API. `AGENTS.md` cites this
rule as **C2a**.

### 11.2 Compile once, type a query once (C2b)

The compiled model is compiled once. A query is parsed, name-resolved and typed once into one typed query, and every
question about it (its result type, its target, its expression, its lowering, its plan for a runtime) is asked of
that typed query. The entry points that took the model as text and recompiled it for each question were deleted
(2026-10-05).

Everything that is loaded compiles: no analysis catches a compile error and assumes an answer.

### 11.3 One owner for every database decision, keyed by the declared type

Given a database, what is done with it (its dialect, the session opening, the in-memory instance, its protocol facts)
is decided in one place, keyed by the runtime's **declared** connection type. That is upstream's own shape:
`dbExtension.pure` (`createDbConfig(connection.type)` → `loadDbExtension()`) for the SQL side and Java database
managers for connections. §10.2 is the same rule carried down to every unit. The decisions:

| id | decision |
|---|---|
| D1 | The dialect is the runtime's declared connection type. The session is only checked against it, never consulted to choose. |
| D2 | A query with no store runs on a declared runtime built the way legend-engine's PCT adapter builds one (`pct_relational.pure`: a database and a connection store around the test's connection, then the normal plan). |
| D3 | A runtime whose data lives in no database (file or inline JSON model connections, instances) runs on the platform's own in-process DuckDB: lite's counterpart of upstream's in-memory model-to-model execution (`docs/SEMANTICS_REGISTER.md` S27). It is applied by whoever chooses the session (the server's connection resolver); a direct caller's own connection is checked, never swapped. |
| D6 | Where a program reaches upstream's Pure compiler (`sqlQueryToString`, `pureToSqlQuery`, `typeInference`, `toPostgresModel`, `executionPlan`, ...), the entry is lite's: a native implemented in lite, or a named wall. Upstream's Pure internals are never typed or run, and an override is never a no-op. |
| D8 | A runtime that binds a database and file data runs in that database with the file data inline in its SQL; a dialect that cannot spell inline JSON refuses by name (proven on DuckDB, 2026-10-03). |

The plan is the boundary between the two sides; what a plan holds and how its result streams are
`docs/EXECUTION_PLAN_BOUNDARY_2026_10_05.md` and `docs/PLAN_AND_RESULT_MODEL_2026_10_18.md`.

Sources: `docs/PLAN_EXECUTION_SPLIT_AND_DATABASE_OWNER_2026_10_03.md` (§1, §2, §3.7, C2a, C2b, C3), `AGENTS.md`
("Pipeline") (git history: docs/HISTORY.md).
