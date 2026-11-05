# Writes: one write layer, two kinds of graph (design sprint, 2026-10-18)

**Status:** design agreed in conversation with the user on 2026-10-18 (§7 records each call); built: all seven legs
landed on the branch by 2026-10-20. Leg 3 (the spike, answered 2026-10-19) decided graph B: the class model over the
native store's own tables, contained work. §8 keeps what the legs decided (B's layout, write plans); their landing
records are cut.

**Asked for:** writes for the `#Cypher{...}#` island, designed so that the Postgres `#SQL{...}#` island's DML (and
GraphQL mutations later) land on the same layer, instead of each front end getting its own.

**Inputs:** three read-only surveys of 2026-10-18:
- lite's write machinery today;
- the TCK's remaining write cases;
- legend-pure 5.99.0 and legend-engine 4.145.0, read as spec.

§1 summarizes them. This builds on two earlier proposals:
- `docs/CYPHER_TCK_TO_ZERO_PLAN_2026_10_17.md` (history: `docs/HISTORY.md`): W4, decisions 2, 5 and 6, questions Q5
  and Q6.
- `docs/BI_AND_ETL_PLAN_2026_09_29.md`: F1, a `write` lowered to DML.

## 1. Where things stand

> Cut in the 2026-11 docs cleanup: lite's write machinery as it stood on 2026-10-18 (no lowering emitted DML, the
> plan side only read, every island refused writes). Full text: `git show de2745351:docs/WRITES_DESIGN_2026_10_18.md`.

### legend-engine's writes are narrow

- **`relation::write(rel, accessor): Integer[1]` appends:** `INSERT INTO table SELECT …`, columns by position,
  returning the rows written.
- **`mutation::save(values, tree, mapping, runtime)` is the only write through a mapping.** It is a single-object
  upsert over plain column mappings of one root mapping's main table, with every primary key in the graph-fetch
  tree. Joins, DynaFunctions and enumeration mappings are silently dropped.
  - Upsert SQL exists for H2 and MemSQL only.
  - It returns a message string, not a count.
  - There is no delete through a mapping.
- **GraphQL mutations** are rewritten to `internalize → graphFetch → save`, so they inherit all of that.
- **The SQL layer parses INSERT, UPDATE and DELETE and refuses them** ("Unsupported Operation").
- **Persistence** (`###Persistence`) is an ingest pipeline: a service's rows are merged into a target table. Its ingest
  modes are useful vocabulary for later: append-only, snapshot, delta with a delete indicator, uni- and bitemporal
  milestoning, deduplication.

Nothing upstream is a design to copy. Its signatures (`write`, `save`) are verified spec that lite keeps where it
defines the same thing.

> Cut in the 2026-11 docs cleanup: the TCK's write cases as counted on 2026-10-18 (245 refused, 33 skipped, by
> clause and shape). Full text: `git show de2745351:docs/WRITES_DESIGN_2026_10_18.md`.

## 2. The design in one picture

```
  #Cypher over a domain model   #Cypher over a native graph   GraphQL mutation (later)   #SQL INSERT/UPDATE/DELETE
             |                            |                          |                           |
             v                            |                          v                           |
   L2  class writes over a direct (1-1) mapping: property -> column, key, FK column or junction row  |
             |                            |                                                      |
             |                            v                                                      |
             |                L2' the native store: labels, properties, relationships as rows    |
             v                            v                                                      v
   L1  store writes: write / update / delete / merge over a table accessor (#>{db.schema.table}#)
       lowering emits SqlDml (InsertSelect, Update, DeleteWhere, Merge); each dialect renders it
                                          |
                                          v
   L0  execution: one transaction per statement, update counts as values, rollback on error
```

- **Each front end translates to Pure,** as every island does today, and the one compiler lowers it. No island
  writes SQL.
- **`#SQL` DML lands on L1 directly,** because the SQL island already reads physical tables.
- **Cypher has two kinds of graph,** and a query is always on exactly one (§3).

## 3. Two kinds of graph

### A. A graph over a domain model (what the island reads today)

A label is a class, a property is a property, a relationship type is an association or an edge class. Writes keep
the model's shape:
- `CREATE` of a declared class (the database mints the key);
- `SET` and `REMOVE` of declared properties;
- links and unlinks of declared associations;
- `DELETE` and `DETACH DELETE`;
- `MERGE`.

Everything outside the model is refused by name:
- a label that names no class;
- a property the class doesn't declare;
- **a label added to or removed from a node: on a domain model a node's label is its class**, and an instance does
  not change class;
- a write to a class whose mapping is not direct (§4, L2).

This is the Legend value: Cypher over your model, with `#SQL` and GraphQL writing the same model under the same rules.

### B. A native graph store (new): lite as a graph database on any SQL database

There is no domain model. The platform owns one fixed schema, created once, that holds any graph:

```
nodes       (id PK)
node_labels (node_id, label)          -- a node's labels are rows
node_props  (node_id, key, value)     -- or one JSON column per node: leg 3 decides
rels        (id PK, type, src_id, dst_id)
rel_props   (rel_id, key, value)
```

Every Cypher write is an insert or delete of rows in these tables:
- `CREATE (:Robot {name: 'x'})` is a `nodes` row, a `node_labels` row and a `node_props` row.
- `SET n:Manager` inserts a `node_labels` row; `REMOVE n:Manager` deletes it.
- `SET n.nickname = 'A'` inserts or updates a `node_props` row.
- A relationship is a `rels` row, with its properties in `rel_props`.

Nothing needs declaring and nothing changes class. **This is how Neo4j itself works:**
- label names, relationship types and property keys are tokens created on first use;
- a node's record carries its label set and a chain of key/value property records, each value carrying its own type;
- a relationship's record carries its type and its two ends;
- constraints (uniqueness, existence, property types) are optional, restrict later writes, and never declare a
  schema.

So B emulates Neo4j by doing what Neo4j does. It needs no schema and no inference.

### How a query picks its graph

By the graph it names. `USE` already names a DataSpace (W1):
- `USE my::People`, a DataSpace over a model, is **A**;
- `USE my::Social`, declared as a native graph (a new store element), is **B**.

### How this relates to open and closed graphs

They are different questions. The backing (A or B) decides what a node is made of. Open or closed decides what a name
outside the schema means, so it only applies to A:

| | Reading an unknown name | Writing an unknown name |
|---|---|---|
| A, closed | compile error | refused |
| A, open (`<<graph.Open>>`, W1) | no rows, or null | refused: nothing to hold it |
| B | no rows, or null | a new row |

In GQL's own terms, an open graph has no graph type and takes any insert: that is B. A domain-model graph is GQL's
closed graph, typed by the class model. `<<graph.Open>>` on A means "lenient reads".

## 4. The layers

### L1: store writes

**Four verbs over a store accessor,** each returning `Integer[1]`, the rows affected (the database's update count, a
value it produced; charter C1.2):

| Verb | Pure | SQL | Notes |
|---|---|---|---|
| append | `rel->write(#>{db.t}#)` | `INSERT INTO t (cols) SELECT …` | upstream's native, lowered for real; columns by name |
| update | `#>{db.t}#->update(r | pred, ~[c: r | expr, …])` | `UPDATE t SET c = expr … WHERE pred` | lite native |
| delete | `#>{db.t}#->delete(r | pred)` | `DELETE FROM t WHERE pred` | lite native; `true` is `DeleteAll` |
| merge | `rel->merge(#>{db.t}#, ~[keys])` | `MERGE`, or `INSERT … ON CONFLICT (keys) DO UPDATE`, per dialect | lite native; an upsert by key |

- **Natives.** The lite natives are `meta::legend::lite::` functions, each moving the native governance ceiling
  with a dated justification. `write` stays upstream's (`meta::pure::functions::relation`), signature verified.
- **The MIR: extend what is there, nothing beside it.** New records follow `SqlDml`'s table naming (schema, table,
  `NameQuoting`, quoted columns):
  - `InsertSelect(table…, columns, SqlQuery)`, the general form of `InsertFromTable`;
  - `Update(table…, alias, assignments, where)`;
  - `DeleteWhere(table…, alias, where)`, of which `DeleteAll` is the `true` case;
  - `Merge(table…, SqlQuery source, keys, updates, inserts)`.

  Each record is pure data with a render arm per dialect. An arm that can't express its statement throws; there is
  no `default ->`.
- **Rendering with parameters.** `renderStatement(SqlDml)` returns a `RenderedStatement` (text plus its `?` binds),
  as queries have. The seed callers keep the text form.
- **DDL.** `SqlDdl.Column` gains an identity or sequence column where a store declares one (keys, D4).
- **The pipeline.** A write lowers to a `LoweredStatement`: a query, or a DML step with the query it reads.
- **Dialects.** DuckDB, Postgres and H2 first: the lanes that judge. The others refuse by name until each is measured.

### L0: execution

- **`Execution.executeWrite`** runs a statement's DML steps and its final query on one connection, in one
  transaction. It commits on success, returning the result and the counts, and rolls back on any error. The
  transaction calls live in `com.legend.exec`.
- **A caller-owned transaction is joined, not committed.** When the connection has autocommit off, the executor works
  inside the caller's transaction. The harness uses this to roll every case back.
- **The "a writing statement reseeds the next" rule** applies to test-data sessions only. A product write is never
  wiped.
- **Execution plans keep reading only for now.** A write plan node is a later leg.

### L2: class writes over direct (1-1) mappings (graph A, GraphQL)

**The verbs take a class or an association as the target.**
- `rows->write(m::Person)`, the rows' columns named by `Person`'s properties.
- `m::Person.all()->filter(p | …)->update(~[name: p | 'B'])`.
- `m::Person.all()->filter(p | …)->delete()`.
- `rows->merge(m::Person, ~[keys])`.

An association is written as its extent of pairs: a `write` links, a `delete` unlinks.

**Only direct mappings are writable.** Inverting mappings in general is the view-update problem, so a write requires
a direct mapping. Phase F checks this (it is never assumed). A mapping is **direct** when all of these hold:
- one main table, with no `~filter`, `~distinct` or `~groupBy`;
- not a union or operation, and not model-to-model;
- every property is a plain column of that table, or a value or embedded class made of its columns;
- no other class mapping uses the table;
- the key (`~primaryKey`, else the table's PRIMARY KEY) is known;
- every NOT NULL column is a property, the key, or minted.

A direct class write is a store write with property names renamed to columns.

| A direct mapping has | A write becomes |
|---|---|
| a plain column property | that column |
| an embedded or value class | its columns, in the same row |
| a key | the row's identity; required for update, delete, merge and links |
| an association over a foreign-key column (a to-one end) | an `UPDATE` of that column, or setting it to null |
| an association over a junction table of the two keys | an insert or delete of a junction row |
| an edge class over a bridge table | an insert or delete of a bridge row |

- **Everything else is refused, with the reason** ("`C` is not writable: its mapping filters its table").
- **Writability is Knowledge.** Phase F records each class's write shape; the DML is Work, built on demand.
- **Two later extensions:**
  - a user-written inverse (a Pure function over L1) for a mapping that isn't direct;
  - upstream's `save`, defined over `merge` and verified against its signature.

**Keys are minted by the database** (C2.4): an identity or sequence column where the store declares one; otherwise
`COALESCE(MAX(key), 0) + ROW_NUMBER() OVER ()` inside the statement. The latter is registered as unsafe under
concurrent writers.

**Staging gives read-your-own-writes.** A write clause materializes its rows, minted keys included, into a session
temp table (`CreateTempTableAs` exists). It applies its DML from that table, and the clauses after it read the temp
table. So `CREATE (n {name: 'foo'}) RETURN n.name` returns the written row, and a later `MATCH` sees it.

### L2': the native store (graph B)

- **A platform-owned schema** (§3 B), created by the platform's DDL. Every write is a `write` or a `delete` of its
  rows, and every read filters them.
- **Property values carry their type, as in Neo4j.** They are stored as Variants, and comparison and ordering use the
  Variant machinery already built (W7, W11).
- **The open question (leg 3):** can B be a platform-owned class model that the existing translator already reads?
  For example `Node` with `labels: String[*]` and Variant properties, over those tables, with a platform-defined
  inverse for its writes.
  - If yes, B is a new resolution rule for labels and properties: contained work.
  - If no, B is a second Cypher backend: much bigger.

## 5. The front ends

### Cypher

Each clause runs once per row of the rows in hand, as Neo4j's do.

- **`CREATE`:** a `write` of the node (A: to its class, with a minted key; B: `nodes`, `node_labels`, `node_props`
  rows). A relationship is a link or a bridge row (A), or a `rels` row (B).
- **`SET` and `REMOVE` of properties:** an `update` of the property (A: its column; B: its `node_props` row).
  - `SET n = map` replaces the properties: those the map names are written, the others nulled.
  - `SET n += map` updates only the properties the map names.
  - Writing `null` removes the property.
- **`SET n:L` and `REMOVE n:L`:** B inserts or deletes the `node_labels` row. A refuses by name.
- **`DELETE`:** deletes the row. Neo4j's DeleteConnectedNode is raised by the database, through a guarded statement,
  while relationships remain. **`DETACH DELETE`** unlinks first, then deletes.
- **`MERGE`:** a read of the pattern, then a create of what the read lacks, in the same statement. `ON CREATE SET` and
  `ON MATCH SET` are updates scoped to the created or the matched rows. A MERGE over a null property raises
  MergeReadOwnWrites.
- **The 33 compile-time write errors** (VariableAlreadyBound, UndefinedVariable and others) come from the static
  checks the island already runs, extended to the write clauses' variables.
- **A temporal written as a property** (2026-10-20): a date, a local time or a local date-time in its column; on B, a
  zoned value as two slot rows, its instant and its offset (a zoned time's spelled `time:instant` and
  `time:offsetSeconds`), and a duration as four (its months, days, seconds and nanoseconds), as the store's value
  classes hold them on A. A list of temporals is the Variant of each element's text, and the model's property carries
  the graph profile's list mark (`listOfDate` ... `listOfDuration`, `projects/graph/model.pure`): JSON holds no
  temporal, so the text says what an element is and the mark says what the list holds. A list of dates, local times
  or local date-times holds the island's own text of each and is read back element by element; a list of zoned
  values or durations holds each one's whole text, its offset written, and is returned whole (an element of one read
  alone is refused by name: the island carries one zone for a list's elements, not one each).

### `#SQL` (L1)

- `INSERT INTO t [(cols)] VALUES | SELECT` becomes `write`.
- `UPDATE … WHERE` becomes `update`; `DELETE … WHERE` becomes `delete`.
- `INSERT … ON CONFLICT … DO UPDATE` becomes `merge`, and `DO NOTHING` a `write` of the rows no row of the table holds
  the key of (as landed, leg 4).
- `RETURNING` reads the staged rows back.
- **The judge:** the table's state after the statement, compared with Postgres 16's (the SQL parity harness's oracle).
- **Refused by name:** data-modifying CTEs and multi-statement scripts.

### GraphQL (later, L2)

A mutation is a `merge` or a `delete` over a class by key, its input object the row. That covers legend-engine's
`save`, and more. It needs the `#GQL{...}#` read side compiled first.

## 6. The judge

**The judge lands first, so a write case can never pass by doing nothing.**

- **Neo4j is the verdict only, never the schema.**
  - `snap.py` runs each case's query in an explicit transaction and commits it: Neo4j checks some writes only at
    commit (a node deleted with its relationships left raises then), and the next case empties the graph. For a query
    Neo4j's summary marks as writing, it dumps the graph after (each element the graph before held keeping its
    ordinal), then runs the control query over it. (The first draft rolled back; the commit-time check made it
    commit, measured on the DELETE scenarios, 2026-10-18.)
  - The answers file gains Neo4j's post-graph (`after`) and the control query's answer (`control`). The side effects
    are the diff of the two graphs, computed the TCK's way, and `snap.sh --check` holds that diff to every TCK
    side-effect table (all 248 agree, and the 2935 "no side effects").
  - Nothing in lite's model is taken from them.
- **The schema on A is lite's own inference,** from two sources it owns:
  - the pre-graph's data, as today;
  - the query's write clauses, typed by lite's own typer: every label, relationship type and property key a CREATE,
    SET or MERGE names, each property typed by the expression written into it (`{num: 1}` is an Integer; `{num: x}`
    takes the UNWIND element's type).

  Two fallback rules cover what that leaves open:
  - a property whose type the writes don't fix (only ever null, or several types) becomes a Variant column, as W7's
    two-typed properties are;
  - a relationship type the pre-graph lacks becomes a junction table (any number of links), or an edge class when the
    query writes its properties.
- **B needs no schema.**
- **Each case runs in a transaction the harness owns:** the statement through the island, then the post-graph read
  back through the island (the oracle's own dump queries), then the side effects (lite's diff of its two reads),
  then the control query if any. Then it rolls back, so no case leaks into the next. The graph before is read back
  once per seeded graph and must be Neo4j's dump, or the judge is broken there (a JUDGE problem fails the lane).
- **The verdict has four parts:**
  - the rows, as today;
  - the side-effect counts, lite's diff against Neo4j's (which is the TCK's table);
  - the post-graph, against Neo4j's, compared structurally;
  - the control queries' rows.
- **Write errors** are judged as BOTH_ERROR is today, but against the error raised, not a refusal.
- **Every write case runs on both kinds of graph,** each over DuckDB and Postgres:
  - **On A,** the cases that change a node's labels are decided as refused (a register row: "on a domain model a
    node's label is its class").
  - **On B,** all 245 run, with the 33 control queries.

## 7. The decisions (2026-10-18)

1. **D1. Layering:** L1 store writes under L2 class writes and the native store, every front end translating to Pure.
   *Agreed.*
2. **D2. Vocabulary:** upstream's `write` lowered for real, plus lite's `update`, `delete` and `merge`. The same verbs
   take a table accessor (L1) or a class or association (L2). *Proposed, unchallenged.*
3. **D3. Writable mappings: direct (1-1) only,** checked in phase F and refused by name otherwise. *The user's call.*
4. **D4. Keys minted by the database only:** an identity or sequence, else max plus row number inside the statement
   (registered as unsafe under concurrent writers). *Proposed, unchallenged.*
5. **D5. Two kinds of graph:** A (domain model, schema-first) and B (native store, schemaless). A query names its
   graph. *Agreed.*
6. **D6. Labels:** on A a label is the class, and label changes are refused by name. On B labels are rows. The
   label-set row move of the first draft is dropped. *Agreed.*
7. **D7. One transaction per statement,** writes staged through session temp tables, rollback on error, a caller's
   transaction joined. *Proposed, unchallenged.*
8. **D8. The judge first.** Neo4j is the verdict only. A's schema is lite's own inference (§6), B needs none. Per-case
   rollback, the four checks. *Agreed (the user: "we can't use Neo4j, have to do our own inference").*
9. **D9. Order** as §8.
10. **D10. Ownership:** this line builds L1's MIR and render arms in the database owner's files, announcing each in
    `docs/IN_FLIGHT.md`. *Proposed, unchallenged.*

## 8. Legs

> Cut in the 2026-11 docs cleanup: legs 1 (the judge) and 2 (L0 and L1, the store writes) as landed, 2026-10-18.
> Full text: `git show de2745351:docs/WRITES_DESIGN_2026_10_18.md`.

3. **The spike: the native store as a platform class model** (§4, L2'). It decides B's cost and its legs.
   **Answered 2026-10-19: yes, contained work.**
   - **How:** graph B is the domain realization's own class model, mapping and island, realized over the native
     store's fixed tables through views. The views are the platform's over its own system tables (the world map's
     tenet), and every one is a table the mapping already maps (`CypherGraph.Realization.NATIVE`,
     `CypherGraph.Native`).
   - **The layout, decided:** narrow rows, not one JSON column per node.
     - `nodes (id, labels)`: the label set as one canonical text (`:A:B:`), so a class's view is one equality.
     - `node_props (node_id, key, slot, v_int, v_float, v_str, v_bool, v_date, v_ts, v_time, v_json)`: one row
       per property slot (a scalar's slot `''`; a duration's, a zoned or a nanosecond value's, its parts). Each
       value sits in the column of its slot's type, with a scalar's JSON beside it for a property typed two ways.
     - `rels (id, type, src_id, dst_id)` and `rel_props`, alike.
     - Why: every Cypher write is then one of L1's four verbs over a row. A property set is a `merge` on (node, key,
       slot), a removal a `delete`, a label change an `update` of `labels`, a create an insert. A JSON column would
       need JSON-editing functions in every dialect.
   - **Every relationship type is a junction** (or an edge class's bridge) in the views. A foreign key holds one
     end, which writes would outgrow.
   > Cut in the 2026-11 docs cleanup: the spike's measurement on the TCK corpus and what it left for leg 6
   > (2026-10-19). Full text: `git show de2745351:docs/WRITES_DESIGN_2026_10_18.md`.

> Cut in the 2026-11 docs cleanup: legs 4 (`#SQL` DML), 5 (L2 and Cypher on graph A) and 6 (graph B and Cypher on
> it) as landed, 2026-10-20, with what each built and found. Full text:
> `git show de2745351:docs/WRITES_DESIGN_2026_10_18.md`.

7. **Write plans. Landed on the branch 2026-10-20** (GATES' hundredth landing), in the database owner's files under D10
   (`docs/IN_FLIGHT.md`). A program that writes plans as an `ExecutionPlan` the model-free runner executes, so a write
   runs where a query runs through a plan, every value bound. Before it, `TypedQuery.executionPlan` lowered a body as
   one query and refused a write in it by name, and the server's `pure/v1/execution/execute` ran the wire path, which
   refused one too: a Cypher write or `#SQL` DML ran in process (`Execution.execute`), not over HTTP.
   - **The dialect: DML as a statement.** `SqlDialect.renderStatement(SqlDml)`: the text and its `?` parameters in
     placeholder order, as `renderStatement(SqlQuery)` is. The store writes' and the staged table's render methods
     return the writer they wrote into (invariant 3); `render(SqlDml)` stays text, and a parameter in it is refused as
     a query's text refuses one. A dropped table's DDL is text (it holds no value). `StoreWritesTest` binds an UPDATE's
     value and predicate and a staged table's predicate on DuckDB and both H2 dialects (Postgres's placeholders
     listed).
   - **The plan records** (`ExecutionPlan`, `PlanJson` version 5):
     - `Update(sql, count)` is a statement run for the change it makes (a write into a store table, a staged table
       created or dropped). Its answer is the count the database reports. `count` is, where a later step reads that
       count, the name its slot binds it by (`#count`, which no declared parameter can be named).
     - `Check(sql)` is a query run for the error its database may raise (a Cypher write's guard: DeleteConnectedNode,
       DeletedEntityAccess). Its rows are read and dropped.
     - A `Sequence` holding an `Update` runs in one transaction of its session (D7): its changes all land or none do,
       and a caller's open transaction is joined. Its answer is its one result step's, passed on once the transaction
       has committed.
   - **The runner** (`PlanRunner`, `PlanSessions`):
     - A sequence's steps run on one session. Each statement's target must be the same, or the plan is refused by
       name. The session statements run once. Each change's count is kept for the slots after it, and the result's
       text is held until the commit. A raise the session holds surfaces as the execution's does (`RaisedErrors`).
     - Decision A's second rule: a plan that changes a database the runner shares runs on a database of its own,
       opened and set up from the target for that run (`Source.openToChange`), so the shared one is never changed. A
       caller's connection, a database the user names, or one a URL reaches keeps the change.
   - **The planner** (`PlanMaker.program`; `TypedQuery.resolve` and `lowerer`, the wire path's steps split out).
     A program that writes or stages is planned statement by statement, each with the lets before it, as the
     execution's body compiler takes them, every declared parameter a value its statement binds:
     - a staged let is an `Update` creating `legend_stage_s`; the let is bound to that table for the statements after
       (`StagedRows`, which now owns the table's name and its store for both paths), and an `Update` drops the table
       after the result;
     - any other let rides the lets of the statements after;
     - a store write (L1's, or L2's after the store resolver) is an `Update` of `Lowerer.lowerStoreWrite`;
     - any other statement before the last is a `Check`;
     - the last statement is the answer: a query's text in the form the caller asks for, or a write's count, bound
       after its `Update` into the one placeholder of a query of its own (`Lowerer.lowerPlanParam`). Either way the
       database writes the text.
   > Cut in the 2026-11 docs cleanup: leg 7's server route and the tests that judged it (2026-10-20). Full text:
   > `git show de2745351:docs/WRITES_DESIGN_2026_10_18.md`.

   - **Not yet:** a write whose answer is a count, over `execute` (a scalar's TDS answer is unprobed for reads too).
     Since 2026-10-20 it is refused by name before its plan runs, as a read's scalar is
     (`Execution.executeUpstream`; `WritePlansH2Test.executeRefusesACountAnswerBeforeItsWriteRuns`): before, its write
     landed and the answer then failed to serialize.
8. **Later:**
   - GraphQL mutations, once `#GQL{...}#` compiles;
   - Persistence-style ingest modes over `merge`.

> Cut in the 2026-11 docs cleanup: the order the legs ran in and the expected end state (2026-10-19 and 10-20).
> Full text: `git show de2745351:docs/WRITES_DESIGN_2026_10_18.md`.
