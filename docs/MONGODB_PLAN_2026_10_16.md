# MongoDB as a first-class database of legend-lite: the study and the plan (2026-10-16)

**Status: decided (2026-10-16): the user ruled on every decision of §4 (D1 to D14) and asked for a viable v0. v0 was
built 2026-10-16 to 2026-10-17 (§11, its as-built record, cut in the 2026-11 docs cleanup); §0 describes the tree
before v0. Kept as spec input for the port: the decisions, the design, the PCT lane and the risks.** The ask (the user, 2026-10-16): learn what backends legend-lite supports
today (all SQL), then plan MongoDB as a first-class database, PCT lanes included, built "the pristine way": the way the
dialect consolidation program (the ninth line, `docs/DIALECT_LEAK_CENSUS_AND_PLAN_2026_10_12.md`) lays out the end
state, so that Mongo is done right the first time rather than consolidated later. That program names MongoDB as out of
its own scope ("MongoDB, Elasticsearch and SingleStore are new targets, not consolidation", its §0) and the backend
order already places it next after the corpus-on-every-database work (`docs/CLOUD_BACKENDS_PCT_2026_10_11.md` §2 item 4,
§11, §20; `docs/IN_FLIGHT.md`, the eighth line's "Next").

§0 is the answer; §4 the decisions; §5 the design; §6 the PCT lane; §9 the risks and the measurements that settle
them; §10 the sources. §1–§3, §7, §8 and §11 were cut in the 2026-11 docs cleanup.

---

## 0. Start here

1. **What exists.** Eleven executing databases, every one SQL over JDBC: DuckDB (the platform's own engine and the PCT
   reference lane), H2, Postgres, SQLite, MariaDB, MySQL, Trino, ClickHouse, Snowflake, Databricks and BigQuery, each a
   class in `core/src/main/java/com/legend/sql/dialect/` chosen by one registry, `database/Databases.dialect(type)`;
   plus three engine-text printers (`EngineStyleH2`, `EngineStyleDB2`, `EngineStyleComposite`) that write legend-engine's
   golden SQL text and execute nothing. The `DatabaseType` enum (`model/ConnectionDefinition.java:54`) is legend-engine's
   21 values plus lite's SQLite, MariaDB and MySQL (register row S48); the 13 others (DB2, SqlServer, Oracle, ...) are
   refused by name in every switch. The database enters the pipeline at three seams: plan side, `Databases.dialect`
   (phase J); execution side, `exec/Sessions` and the JDBC runners (phase K); the test harness, `PctBackend.Lane`.
   Every JDBC use is funnelled to `exec`, `server`, `testdatagen` and the root by `ArchitectureTest
   .javaSqlIsFunnelledToTheCharteredSeam`.
2. **The answer in one paragraph.** MongoDB is a **render target of the one compiler over the same `SqlQuery` IR**: a
   class beside the SQL dialects that decides, for every IR node, function and aggregate, what the aggregation pipeline
   spells, emulates or refuses by name; a **connector beside JDBC** that opens a `MongoClient`, runs the rendered
   command with its bound values and carries the one text the database wrote; and a **test backend beside the embedded
   servers** that starts a pinned `mongod` Bazel unpacked, as `EmbeddedClickHouse` starts ClickHouse. That is exactly the
   dialect program's acceptance test (§4 there: "a new database is three classes and one line in each of three
   registries"), and exactly what `docs/CLOUD_BACKENDS_PCT_2026_10_11.md` §11 already says of the non-SQL stores:
   "beside `SqlDialect`, not inside it: the Lowerer's output (the `SqlQuery` IR) stays the input, and a new target
   translates it into the store's own query language. The executor gains a second chartered seam beside JDBC." The MIR
   does not change (AGENTS.md invariant 3a: "new dialect = one class implementing `render(SqlQuery)`"); the Lowerer does
   not change; the PCT harness gains a lane the way ClickHouse's was gained. The execution tenet holds unchanged: the
   store executes, the pipeline builds the wire text, Java carries a string, and what Mongo cannot express is a
   `DialectCapability` wall, never a value computed in Java.
3. **The one structural dependency, and the ask to make now.** The dialect program's Phase 5 (the connector interface,
   `DatabaseConnector` and `Connectors.of(type)`) and Phase 6 (`TestBackend`) are not built yet (Phases 0 to 4 are on
   main: neema2/legend-lite#39, #45, #49). Today a session IS a `java.sql.Connection` (`Sessions.Session.connection()`,
   `PlanSessions`, `PlanRunner`, `QueryService.execute(..., Connection, ...)`). If Phase 5 is designed JDBC-only, Mongo
   has to fork the executor and the harness, which is the consolidation debt this plan exists to avoid. **The ask to the
   ninth line (§8.1): a connector's session is a sealed type over transports, the connector owns `check`, `version`,
   `run` and the bulk load, and `RenderedStatement` carries the language its text is in.** Mongo is then Phase 5's and
   Phase 6's first non-JDBC instance, built with the dialect session or right after it.
4. **The language is the engine's own.** legend-engine's `###MongoDB` store (collections with a JSON schema),
   `MongoDBConnection` and the `MongoDB` class mapping already parse, compose and round-trip in lite byte for byte
   (`parser/section/MongoDBSectionGrammar.java`, `protocol/Protocol.PMongoDatabase`, `PMongoDbConnection`,
   `PClassMappingMongoDb`), carried opaque in the model and never compiled or run. First-class means those compile,
   resolve, lower, render and execute; and the Relation surface (`#>{db.Collection}#`, `project`, `filter`, `groupBy`,
   ...) over a collection, which is what PCT and DataCube use, comes first (decision D3).
5. **Order.** After the eighth line's corpus-on-every-database harness, as the user ordered (§20 there), and with the
   dialect program's Phase 5 and 6: the seam first (one pure landing, measured by every existing lane's SQL dump staying
   byte-identical), then the server and the lane scaffold, then the target in two halves (scalar suites, then the
   relation suites), then the store and the mapping, then the corpus. Six landings, about 12k to 16k lines (§7).
6. **The rules this line keeps**, inherited from the Postgres, IQ and Cypher lines and the dialect program: zero FAIL,
   zero BUG, zero ERROR, a refusal by name beats a possibly wrong answer; Java never computes a value; every per-database
   decision in Mongo's own files (the owner guard holds it there); every pin dated with its reason; every landing
   announced in `IN_FLIGHT` first and recorded in `docs/GATES.md`.

---

> Cut in the 2026-11 docs cleanup: §1–§3, lite's backends as of 2026-10-16, the dialect program's end state and where Mongo fits, and what the tree then held for Mongo. Full text: `git show de2745351:docs/MONGODB_PLAN_2026_10_16.md`.

---

## 4. Decisions for the user

**Every decision below was ruled by the user on 2026-10-16 (the "Decided" line under each heading); the options are kept as the record of what was weighed.** Each decision below says what is being decided, every option worth taking with its pros, cons, cost and complexity,
the recommendation and why, and what reversing it later would cost. The scale: **cost** S is under 300 lines in one
line's own files; M is 300 to 1,500 lines, or one landing another line must co-sign; L is 1,500 to 6,000 lines; XL is a
program of its own. **Complexity** names what makes the option hard, not how long it takes. **Reversal** is the price of
changing the decision after it has landed.

| # | decision | recommendation | cost | reversal |
|---|---|---|---|---|
| D1 | the compiler's shape | a target over the `SqlQuery` IR | L (the target itself) | XL: a second lowering |
| D2 | the seam's type | `MongoDb implements SqlDialect` whole; no split now (decided) | S | M: the split, if the refusal list grows |
| D3 | the model surface first | the Relation surface, the class mapping on it | M | none: order only |
| D4 | how the registries name Mongo | `DatabaseType.MongoDB`, a lite extension | S to M | M: a second enum re-keys every registry |
| D5 | parameters | the command's `let` variables | S | S |
| D6 | typing a collection | its declared JSON schema, and only that | S to M | S: a sugar grammar can be added |
| D7 | the null model | SQL's, emulated; nullability stamps as the optimisation | M | unbounded pins if not |
| D8 | the lane's server | a pinned Community `mongod`, Bazel-downloaded | S | S |
| D9 | the version floor | pin 8.0; floor 7.0 if PR0 finds nothing 8.0-only | S | S |
| D10 | the wire text | the pipeline writes it; per-row carriage for large results | S + M | a tenet exception if not |
| D11 | server-side JavaScript | never | none | none |
| D12 | where the connector lives | `core/src/main/mongodb`, found by `ServiceLoader` | S | M |
| D13 | the metamodel store on Mongo | refused by name first; seeded collections in the burn-down | S, then M | none |
| D14 | the line and its order | start everything now, under three constraints (decided) | coordination | none |

### D1. The compiler's shape: a target over the IR, or a lowering of Mongo's own

**Decided (the user, 2026-10-16): a target over the `SqlQuery` IR.**

**What is decided.** Whether Mongo's pipeline is produced from the Lowerer's output (the `SqlQuery` IR, phase I),
by a class that decides each IR node as the SQL dialects do, or from the typed Pure (phase G½/H) by a lowering written
for Mongo, as legend-engine's `pureToMongoQuery` is.

**Option A, a target over the IR (recommended).** One class in `sql/dialect/`, `MongoDb`, with its own `passes()`
(IR rewrites into shapes it can spell: joins as lookups, subselects as sub-pipelines, static pivots through
`PivotAsGroupBy`) and the exhaustive switches over `SqlFn` and `SqlAgg.Fn`. Inside the class the pipeline may be built
as stage records first and written once (a dialect-private intermediate, as the carrier pass already builds its own
nodes), which keeps each stage testable; the MIR itself does not change.
- Pros: invariant 3a holds ("new dialect = one class; MIR does not change"); the Lowerer, the resolver, the mappings,
  the folds, the windows and the carriers are reused whole; the vocabulary guard, the capability matrix and the reach
  measurement apply with no edit; the wire wraps (`csvWire`, `jsonWire`, `tdsText`) are IR compositions, so they are
  rendered by the target like any select; PCT's storeless path reaches it; Elasticsearch later takes the same seam.
- Cons: the IR is SQL-shaped (one select with FROM, JOIN, WHERE, GROUP BY; subselects as the one nesting) and a
  pipeline is linear, so joins, correlated subqueries and exists tests are emulations (`$lookup` with `let`, then
  `$unwind` or `$size`); the IR's meanings are the platform's written contract (the dialect program's D3: DuckDB's
  typing rules, three-valued nulls) and Mongo emulates them (D7); a few IR constructs are refusals by name (a
  recursive CTE beyond `$graphLookup`'s shape, raw SQL text, the catalog grid); pipelines are more text per shape than
  SQL, so the class is the largest dialect.
- Cost: L for the class (4,000 to 6,000 lines in PR3, about 3,000 in PR4); nothing outside it. Complexity: the shape
  passes and the null emulation; architecturally nothing new.

**Option B, a lowering of Mongo's own from the typed Pure.** `TypedSpec` to a Mongo IR to text, beside the Lowerer.
- Pros: the pipeline is built in its own shape from the start; for the engine's narrow subset (filters on root
  fields, graph fetch) it is the shorter path; no fight with SQL semantics inside the translator.
- Cons: a second compiler: the resolver's and Lowerer's structural work (joins through mappings, inlining, folds,
  windows, carriers, the wire wraps) is done twice and drifts; it is invisible to the vocabulary guard, the matrix and
  the reach measurement; it ports the engine's Pure program to Java (Charter C6.1); every later IR feature is built
  twice; PCT's reach cannot measure it.
- Cost: XL (10,000 to 15,000 lines plus its own guards). Complexity: high, and permanent.

**Option C, translate rendered SQL text to a pipeline.** Rejected: text parsing of the platform's own SQL is the leak
the dialect program spent Phase 1 removing, and a SQL parser for one database is a program of its own.

**Reversal.** From A to B is a second lowering (XL). From B to A is a rewrite. A is also the one the program's §11
already names.

### D2. The seam's type: implement `SqlDialect` whole, or split a `QueryTarget` above it

**Decided (the user, 2026-10-16): **Mongo implements `SqlDialect` whole; no split now.** A re-read of the interface showed nearly every member has a Mongo meaning once "SQL text" is read as "the text the connector runs": DDL is a `create` with a validator, DML an `insertMany`, `sessionSetup` and `sessionTable` commands, `raw` a user's pipeline run as written, `storedName` a field path, `catalogType` the collection's validator. The members with no meaning are `fromEngineText` and `copySelectList` (already refusals on most SQL dialects) and three JDBC cell conventions a non-JDBC runner never calls. No `Language` field either: the plan's target carries `DatabaseType.MongoDB`, and the connector registered for that type runs its text. The split is reconsidered only if the refusal list grows past that. Atlas SQL (the `mongosql` dialect, Atlas-only) and the deprecated BI Connector are not reusable: SQL text in, Atlas endpoints only, a compiler of its own; their existence is evidence that MQL emulates SQL well.**

**What is decided.** What type a non-SQL target has, and what the plan records and the runner switch on.

**Option A, split (recommended, with the dialect program's Phase 5).** `QueryTarget` holds what every target has
(`renderStatement`, `render` as text, `passes`, the vocabulary switches, `sessionSetup`, `raw`, `forServer`,
`needsStaticPivot`, `wireTypeName`, `typingWireName`, `deliversOwn`, `normalize`); `SqlDialect extends QueryTarget`
keeps the SQL-only members (DDL, DML, `fromEngineText`, `physicalName`/`storedName`, the catalog, `copySelectList`,
`hostOperations`, `script`). `Databases.target(type)` answers a `QueryTarget`; `Databases.dialect(type)` answers a
`SqlDialect` and refuses a non-SQL type by name. `RenderedStatement` gains `Language` (`SQL`, `MONGO_COMMAND`), carried
into `ExecutionPlan.Sql`, and `PlanRunner` switches on it with no default.
- Pros: the types say the truth (a non-SQL target cannot be asked for DDL or engine text, by compile error, not by
  refusal arm); the plan record names its language; javac's exhaustiveness covers the runner's dispatch; no SQL-only
  member leaks (`sessionSetup` strings would otherwise be run as SQL); Elasticsearch reuses it; it is §11's own
  sentence, "beside `SqlDialect`, not inside it".
- Cons: an interface edit under the dialect program's ownership, touching every caller's declared type
  (`Compiler.dialectOf`, `PlanMaker`, `StatementExecutor`, `Execution`, `Executor.execute`, the wasm `Boundary`): a
  pure move the program must co-sign and prove by its lane dumps; it ties PR1 to Phase 5's timing.
- Cost: M (600 to 1,000 lines moved or re-typed, one co-signed landing). Complexity: low in code; the coordination is
  the work.

**Option B, `MongoDb implements SqlDialect`, refusing every SQL-only member by name.**
- Pros: no interface edit; lands without Phase 5; the smallest first cut; the capability matrix reads it as it reads
  any class in the package.
- Cons: about a dozen refusal arms that exist only because the type is wrong; `ExecutionPlan.Sql.statement` and
  `RenderedStatement.sql` carry a command under a SQL name; `sessionSetup()` must answer empty or the executor runs
  its strings as SQL; the runner and the PCT bridge still need a non-JDBC session, so the execution-side edit happens
  anyway; it is the identity-in-disguise shape (`rawH2IsNative`, `rawTarget`) the program removed in Phase 1c.
- Cost: S now (about 100 lines), M later to split regardless. Complexity: none now, debt later.

**Option C, a separate `DocumentTarget` interface and registry, no supertype.**
- Pros: `SqlDialect` untouched; Mongo wholly apart.
- Cons: every consumer (the plan maker, the runner, the PCT bridge, DataCube's planner boundary) grows a second
  branch; `Target.type()` must say which registry answers; Elasticsearch adds a third branch everywhere.
- Cost: M, growing with every consumer. Not recommended.

**Reversal.** B to A is the same edit as A, done later against more code. A is the one to make once.

### D3. The model surface first: the Relation surface, or legend-engine's class mapping

**Decided (the user, 2026-10-16): **the Relation surface first, under one rule: the document is a Variant, and typed columns are views on it.** A collection with no schema is `(_id, document: Variant)`, the shape the IR already has for `sourceUrl` and the Variant navigation the typer does for jsonb; a declared schema adds typed top-level columns, nested objects staying Variant; a class mapping types what it maps, a to-one nested object as an embedded property mapping into the same document at a deeper path, a nested array as an explosion (the lateral join `flatten` uses); `graphFetch` over a Mongo-mapped class is a `$project` of the subdocument. The engine's `~binding` schema says the same thing in the engine's words and is read when the class mapping lands.**

**What is decided.** Which way a user's query reaches a collection first.

**Option A, the Relation surface first (recommended).** A `###MongoDB` collection with a JSON schema is a relation:
`#>{db.Collection}#`, then `project`, `filter`, `groupBy`, `join`, `extend`, windows, the wire. The engine's
`MongoDB` class mapping (`~mainCollection`, `~filter`, `~binding`) is then a relation class mapping over that relation,
each property the same-named field.
- Pros: it is the IR path, so PCT, DataCube, Query and the corpus's relation tests use it unchanged; the normaliser
  already turns a legacy class mapping into synthesised functions, so the class mapping is a mapping of the relation,
  not new typing; the engine's drop-in surface follows in the same landing (PR5).
- Cons: a table reference over a `###MongoDB` store is a lite extension (the engine has none), a register row; the
  engine's surface (`getAll`, `filter`, `graphFetch`, `serialize` over the class mapping) comes a landing later.
- Cost: M (PR5, about 2,500 lines, both surfaces). Complexity: the schema-to-columns rule (D6) and the class mapping's
  reading of `~filter` and `~binding`.

**Option B, the engine's class-mapping surface first.**
- Pros: drop-in parity with the engine sooner; the engine's own Mongo test cases judge it.
- Cons: the engine's subset is narrow (root-field filters, graph fetch; no relation operations), and it still needs
  the IR target underneath to execute anything, so PR3 and PR4 are not saved; graph fetch's JSON wire is PR4's
  `jsonWire`; everything PCT measures is deferred.
- Cost: the same total in a worse order.

**Reversal.** None: this is order, not exclusion; both land in PR5.

### D4. How the registries name Mongo: `DatabaseType.MongoDB`, a wider engine enum, or no enum

**Decided (the user, 2026-10-16): `DatabaseType.MongoDB`, a declared lite extension, as SQLite, MariaDB and MySQL are.**

**What is decided.** The key every registry, `Target`, `ExecutionPlan.Target.Database`, `StorelessRuntime`,
`PctBackend.Lane` and the owner guard switch on.

**Option A, `DatabaseType.MongoDB` as a declared lite extension (recommended).** S48's precedent (SQLite, MariaDB,
MySQL): accepted by the model, refused on the drop-in surface (`LITE_DATABASE_TYPES`), declared only by a
`MongoDBConnection`, which `FromProtocol` reads into a `ConnectionDefinition(MongoDB, MongoServers(urls, database),
auth)` with one new `ConnectionSpecification` variant.
- Pros: javac forces an arm in every exhaustive switch over the type (`Databases` ×4, `Sessions` ×5,
  `StorelessRuntime`, `EngineText`, the grammar): every database decision is asked "and Mongo?" and answers by name;
  nothing re-keys; the owner guard already lists the name.
- Cons: the enum's javadoc ("the engine's `DatabaseType` verbatim plus lite's extensions") stretches to a store the
  engine does not call a database type; a `RelationalDatabaseConnection { type: MongoDB }` must be refused with a test;
  one register row.
- Cost: S to M (about 300 lines, mostly switch arms). Complexity: none.

**Option B, a wider enum (`Engine = DatabaseType ∪ {MongoDB}`) threaded through every registry and record.**
- Pros: `DatabaseType` stays the engine's relational enum untouched; "relational type" and "execution engine" are
  distinct notions.
- Cons: `Target.type()`, `ExecutionPlan.Target.Database.Platform`, `Databases`, `Sessions`, `StorelessRuntime`,
  `PctBackend`, `DatabaseOwners` and the plan's JSON all re-key; two enums kept in step; the register row is needed
  anyway.
- Cost: M to L (800 lines or more of re-keying, every plan fixture regenerated). Complexity: breadth.

**Option C, no enum entry: a connection kind read by class (`instanceof MongoConnectionDefinition`).**
- Cons: exhaustiveness is lost, which is the program's whole method; `DialectOwnerExclusivityTest` forbids deciding a
  database by a class test. Rejected.

**Reversal.** A to B is B's cost later; B to A is cheap. Start with A.

### D5. Parameters: the command's `let` variables, or literals in the text

**Decided (the user, 2026-10-16): **the SQL pattern exactly**: placeholders in the text, typed slots in order, the driver binds the values. The marker's spelling is PR0's measurement, between a `let` variable the pipeline references and a marker document the connector replaces in place before sending; the in-place form renders a comparison with a field as a plain `$match` (index use), takes a `$limit`, and needs no 5.0.**

**What is decided.** How a plan's declared parameters reach the server.

**Option A, `let` variables (recommended).** The writer writes `"$$p<n>"` where the SQL writer writes `?`, in the same
bind order; the connector builds `let: {p0: v0, ...}` from the plan's slots; a list binds as one BSON array; no
`TypeHole` (a BSON value carries its type).
- Pros: no value in text (the plan program's P3); one command text for every value; the same bind-order contract as
  `?`; typed values end to end.
- Cons: a variable is visible to `$match` only through `$expr`, and whether the planner uses an index for a
  comparison inside `$expr` against a `let` variable is a PR0 measurement (a cost on stored collections, none on
  PCT's literal relations); stage options (`$limit`, `$skip`) take constants, so a parameter used as a limit is refused
  by name (today's IR carries `limit` and `offset` as plan-time constants, so nothing is lost); `let` on `aggregate`
  needs 5.0 (D9).
- Cost: S (the writer's bind arm; the connector's `let`). Complexity: none.

**Option B, values written into the command text as Extended JSON literals at run time.**
- Pros: no `$expr` for a constant comparison; nothing a stage option cannot take.
- Cons: the plan's text changes with each value (no reuse, no stable plan to review); it is the value-in-text form P3
  rules out, and the execution-plan boundary program decided "a command document's values" for exactly this store.
- Cost: S. Rejected on the rule, not the cost.

**Reversal.** S either way; the connector and one writer arm.

### D6. Typing a collection: its declared JSON schema, a lite column grammar, or inference

**Decided (the user, 2026-10-16): **the document is a Variant**; a declared JSON schema types the top-level columns; a class mapping types what it maps; no sampling, ever.**

**What is decided.** Where a collection's columns and their Pure types come from.

**Option A, the declared `jsonSchema`, and only that (recommended).** Top-level `properties` by `bsonType`: string to
String, int and long to Integer, double to Float, decimal to Decimal, bool to Boolean, date to DateTime, objectId to
String (its hex), object and array to Variant; a property not in `required` is nullable; `number` (either int or
double) and a property with no `bsonType` are refused by name; a collection with no schema is refused by name;
`additionalProperties` adds nothing. DataCube's catalog reads the server's own validator (`listCollections`, the same
JSON schema shape) through `catalogType`.
- Pros: no guessing (invariant 4; Charter C2.2); the engine's own DSL requires the schema, so the model stays the
  engine's; the catalog and the model agree by construction.
- Cons: a collection with no validator cannot be read until someone declares it; `jsonSchema` is verbose; BSON to
  Pure is lossy at the edges (an objectId is text to Pure, a date has milliseconds, both register rows).
- Cost: S to M (about 400 lines: the schema reader, the type table, the rows). Complexity: the edges.

**Option B, a lite column grammar (`Collection X ( id: objectId; name: string; ... )`) beside the schema.**
- Pros: concise; typed exactly, as a `###Relational` table is.
- Cons: a lite-only grammar the engine refuses (the drop-in model breaks); it duplicates the schema's role; parser
  parity and the composer need an arm; a register row.
- Cost: M. A later sugar if users ask; not first.

**Option C, infer columns from documents (`$sample`, `$type`).** Rejected: deciding types from values (C2.2), and
nondeterministic.

**Option D, flatten nested properties into dotted columns instead of Variant.** Later, as a `flatten` native over the
Variant, once a cube needs it: dotted names collide with the quoting rules, and arrays cannot flatten.

**Reversal.** S: B can be added over A without moving A.

### D7. The null model: SQL's semantics emulated, or Mongo's own

**Decided (the user, 2026-10-16): **SQL's semantics.** A declared column exists for every document, so there is never a missing column; an absent field reads as NULL, as Postgres's jsonb reads an absent key; a field of the wrong type is a loud error on read; BSON's total comparisons are emulated as SQL's three-valued ones where an operand may be null, which the IR's nullability stamps say.**

**What is decided.** What a comparison, a boolean and a sort do when a field is `null` or missing. BSON distinguishes
missing from `null`, compares totally (`null` sorts first, then numbers, then strings, by BSON type order), and a
comparison with `null` is `true` or `false`, never unknown.

**Option A, SQL's three-valued semantics, emulated (recommended).** A missing field reads as `null` at the scan
(`$ifNull`); a comparison with a `null` operand answers `null` (`$cond` over `$type`); `AND` and `OR` are three-valued;
`=` never matches `null`; `ORDER BY` follows the IR's `SortKey` nulls placement through a computed key. The
capability matrix records each as emulated.
- Pros: PCT's null tests and every cross-database answer agree; it is the dialect program's D3 ("keep the meanings as
  the platform's written contract; every dialect already emulates them").
- Cons: verbose pipelines and slower comparisons (an emulated comparison cannot use an index); missing and `null`
  are conflated (a register row); every expression arm pays the tax.
- Cost: M (about 500 lines of expression helpers in PR3, then a tax per arm). Complexity: getting every operator's
  null case right; PCT measures it.

**Option B, Mongo's own semantics.**
- Pros: the shortest translation; native index use.
- Cons: wrong answers against Pure's semantics (`filter(x | $x.a < 5)` with `a` missing: SQL yields no row, `$expr`
  yields the row, since missing reads as `null` and `null < 5` in BSON order); every PCT null case pins forever.
- Cost: S now, unbounded pins.

**Option C, A where a value may be null, direct translation where the IR's nullability stamp says it cannot be.** The
IR carries nullability (`OutputCol.nullable`, the join pad truths, the census's zero "label lies" pins), so a comparison
of two non-null operands needs no guard.
- Pros: cheaper pipelines on declared-required fields and literal relations; semantics kept.
- Cons: relies on the stamps, which are guarded but were made for typing, not for this; more arms.
- Cost: M, in the burn-down.

**Recommendation.** A first, C as the optimisation once the lanes are green. Reversal from A to C is additive.

### D8. The lane's server: a pinned `mongod`, Docker, FerretDB, or Atlas

**Decided (the user, 2026-10-16): **a pinned Community `mongod`, Bazel-downloaded, for the lane** (flapdoodle's and `mongodb-memory-server`'s model, done by the build and checksummed); users bring their own server, as for every other database.**

**What is decided.** What the PCT lane runs against, and the licence posture.

**Option A, MongoDB Community `mongod`, pinned and downloaded by Bazel (recommended).** The ClickHouse rule's shape
(`tools/clickhouse/clickhouse.bzl`): version and sha256 per platform, one binary kept, started as the test JVM's child
on a free loopback port (`EmbeddedMongoDb`), Linux x86_64 first. Only the Apache-2.0 Java driver is on a classpath.
- Pros: the real reference, so every measurement is of MongoDB; the same mechanism as the Postgres, MariaDB, MySQL,
  Trino and ClickHouse lanes (no Docker, CI-able, `target_compatible_with` where no build is pinned); this container
  reaches the download (HTTP 200 measured).
- Cons: the server is SSPL-1.0, not Apache-2.0 as ClickHouse's binary is: nothing of it enters the repository or a
  shipped artifact, and running it in a test is not offering it as a service, but the posture is the user's to
  confirm; the tarball is larger than ClickHouse's; macOS and arm64 pins are later work.
- Cost: S (about 300 lines). Complexity: none; the licence question is a decision, not code.

**Option B, the `mongo` Docker image.**
- Pros: the simplest launch; every architecture.
- Cons: the lanes are Docker-free by design (CI runners, the Bazel sandbox, the five existing server lanes); a second
  mechanism to keep.
- Cost: S, and a pattern break.

**Option C, FerretDB (Apache-2.0, the wire protocol over Postgres or SQLite).**
- Pros: licence-clean; could run over the embedded Postgres already in the tree.
- Cons: it is not MongoDB: its aggregation support is a subset, so the lane would pin FerretDB's gaps as Mongo's and
  pass things Mongo refuses. The wrong oracle.

**Option D, MongoDB Atlas, as the three cloud lanes.**
- Pros: nothing to download; the real server.
- Cons: credentials, a running cost, a network round trip per query; the user's rule is "the open ones run locally".
  A second lane later, if a user runs Atlas.

**Reversal.** S: the launcher is one class; the lane does not care which server starts it.

### D9. The version floor

**Decided (the user, 2026-10-16): pin 8.0 for the lane; floor 7.0 unless PR0 finds an 8.0-only need.**

**What is decided.** The oldest server the connector accepts, and the server the lane pins.

**Option A, pin 8.0 LTS for the lane; floor 7.0 unless PR0 finds an 8.0-only need (recommended).** What the plan uses
(the manual, re-checked at the pin): `let` on `aggregate` and `$setWindowFields`, `$dateAdd`/`$dateDiff`/`$dateTrunc`
(5.0); `$documents` at the database level (5.1); `$sortArray` (5.2); PCRE2 (6.1); the bitwise operators (6.3);
`$median` and `$percentile` (7.0). Nothing listed needs 8.0; 8.0 is the current long-term release and the one to
measure on.
- Pros: every operator the plan names is available; one measured server; users on 7.0 are not refused.
- Cons: users on 6.0 and 5.0 servers are refused by name until a per-feature floor exists.
- Cost: S (a version check in the connector, as `PlanRunner.check` does for H2). Complexity: none.

**Option B, per-feature floors through `forServer(version)`** (the hook H2 already uses): a 6.0 server refuses only the
arms it lacks (`$median`, `$percentile`, the bitwise operators).
- Pros: the widest reach.
- Cons: each floor is a measurement on that version, so a second pinned server per floor; more arms.
- Cost: M per supported version. Later, when a user needs it.

**Reversal.** S either way.

### D10. The wire text: written by the pipeline, or formatted by the connector

**Decided (the user, 2026-10-16): **stream by default**: one document per row, the connector writes the punctuation, exactly as the JDBC runner does over a `ResultSet`; the whole-text form for PCT's TDS text and small results; the connector sets `allowDiskUse`.**

**What is decided.** Who turns a result into the CSV, JSON or PCT TDS text a consumer receives.

**Option A, the pipeline writes the whole text, as SQL does (recommended for PCT and whole results).** The IR wraps
(`csvWire`, `jsonWire`, `tdsText`) are compositions over the lowered query, so the target renders them like any select:
each cell to text (`$toString`, `$dateToString`, the Float and Decimal forms emulated where `$toString` differs from
`Scalars.floatRepr`), rows joined in order, the whole as one string in one document's one field, carried by the
connector.
- Pros: Charter C1.2 and C2.5 (Java carries; ruled representations render in the database); byte-identical judging
  with every SQL lane; the census sees one text column as on every lane; nothing new is written.
- Cons: a whole-text result past the 16 MB BSON document limit fails in the server (a named wall; SQL has no such
  cap); the ordered `$push` is a PR0 measurement; the text forms need emulation work; string building in MQL is
  slower than in SQL engines.
- Cost: S to reuse, M for the text forms (in PR3). Complexity: the forms.

**Option B, the connector reads documents and formats the text in Java.**
- Pros: no 16 MB cap (a cursor streams); faster; Java's formatting is easy.
- Cons: it renders ruled representations in Java (C2.5) and reshapes results (C2.3); it is the host formatter the
  eviction program deleted (`formatAsTds`, E1), and `JavaEvalLedgerTest` would need a new carriage site with a
  written justification; a second wire renderer to keep in step with the IR's.
- Cost: M, and a tenet exception. Not recommended.

**Option C, per-row carriage: the pipeline writes each row's text, the connector joins rows with separators.** The
runner already does this for `JSON_PER_ROW` ("the array's brackets and commas are the runner's"); a per-row CSV and
TDS wrap is a small IR composition (about 100 lines) beside `jsonWireRows`.
- Pros: no cap per result (the cap is per row); the ruled representations stay in the database; streaming for the
  product.
- Cons: the whole-text and per-row forms must agree byte for byte (one test per format).
- Cost: S.

**Recommendation.** A for PCT and small results (the simplest judge), C as the product's streaming form; together they
are the two forms the runner already distinguishes. Reversal: none, they coexist.

### D11. Server-side JavaScript: never

**Decided (the user, 2026-10-16): never. The JavaScript in question would be the dialect's own, authored and maintained in Java.**

**What is decided.** Whether the target may ever write `$function`, `$accumulator` or `$where`, or use `mapReduce`.

**Option A, never (recommended).**
- Pros: no program written by the dialect (Charter C6.1), no security surface (hardened servers and Atlas disable
  scripting), no JavaScript judged by nobody; the refusals it causes are few and honest: the digests (MD5, SHA), the
  string distances (Levenshtein, Jaro-Winkler, which BigQuery also refuses), and any walk needing recursion inside an
  expression.
- Cons: those refusals are permanent until Mongo adds operators.
- Cost: none.

**Option B, `$function` behind a feature flag, its JavaScript as data in the dialect, for the few.**
- Pros: three or four more PCT passes.
- Cons: a program in text, disabled on many servers, outside every guard; one flag that makes a lane's verdict depend
  on the server's configuration.
- Cost: S, and a rule broken. Not recommended.

### D12. Where the connector lives: a `core/src/main/mongodb` source set, a module of its own, or in core

**Decided (the user, 2026-10-16): **`core/src/main/mongodb`, bundled with `//core:drivers`**: the same place the driver-specific JDBC code (`core/src/main/duckdb`) sits; found through `ServiceLoader`.**

**What is decided.** Where the one class that imports the driver (`com.mongodb`, `org.bson`) compiles.

**Option A, `core/src/main/mongodb/`, found by `ServiceLoader` (recommended).** The `core/src/main/duckdb` precedent:
`DuckDbAppenderLoad` and `DuckDbCells` compile beside the DuckDB driver as `//core:duckdb_load`, core compiles against
no driver, and `BulkLoad`/`DriverCells` find them at run time. Phase 5's `Connectors.of(type)` names the Mongo
connector the same way (a `DatabaseConnector` service), or directly if that registry lives beside the drivers.
- Pros: core still compiles against no driver (the browser planner, `//core:plan_side`, never sees one); the owner
  guard gains one path beside `core/src/main/duckdb/`; the funnel rule names the packages and nothing in
  `core/src/main/java` can import them.
- Cons: a service lookup rather than a direct call; a missing jar is a run-time refusal by name, not a compile error.
- Cost: S (one library target, one service file). Complexity: none.

**Option B, a module of its own (`//mongodb`) depending on `//core`.**
- Pros: the cleanest separation.
- Cons: core's registry cannot name a class in a module above it, so the service lookup is needed anyway; a module
  needs its own guards, lanes and BUILD plumbing.
- Cost: M. No gain over A.

**Option C, the driver as a compile dependency of core.**
- Cons: breaks "core compiles against no driver"; the wasm and native builds would have to exclude it. Rejected.

**Reversal.** M: moving a class between source sets and re-pointing the service.

### D13. The metamodel store on Mongo: refused by name, or seeded as collections

**Decided (the user, 2026-10-16): refused by name now. Follow-up, recorded: a document-shaped metamodel store (one document per class, its properties nested), which fits Mongo better than row tables.**

**What is decided.** What a query that reads the platform's system tables (`meta::...` metamodel reads, `getAll`'s
`testBasic`) does on a Mongo runtime. On a server engine today, `SystemDatabase.routeFor` seeds every metamodel table
once per graph as session-private temporary tables on a system connection, through `SqlDialect.sessionTable`.

**Option A, refused by name in the first landings (recommended first).** The few PCT tests that read it are pinned
with that reason, as they were on each server lane before its system connection existed.
- Pros: nothing to build before the lanes measure; the refusal is honest and counted.
- Cons: a handful of pins; DataCube's model-reading features do not work on a Mongo runtime until B.
- Cost: S.

**Option B, seeded as collections (the burn-down).** `metamodel_<table>` collections in a session-private database
named by the session and dropped with it; the target renders `sessionTable`'s `CreateTable` and `InsertValues` records
as `create` and `insertMany` commands; the reads go through the same table renames.
- Pros: the feature on Mongo as on every server; the IR's DDL and DML records are the input, so nothing new is
  carried.
- Cons: the seed is the metamodel's size per graph (the ClickHouse session's limits show what that costs on a slow
  server); the session-private database's lifetime must be measured (the ClickHouse HTTP session's hour-long idle
  tail is the cautionary case).
- Cost: M (about 400 lines and a measurement).

**Option C, route metamodel reads to the platform's own DuckDB while the data query runs on Mongo.** Rejected: a
cross-engine query ("one database per query is supported", `Compiler.executesOn`), and a join across them would be
Java's.

**Reversal.** None: A then B is the normal burn-down order.

### D14. The line and its order: when the Mongo work starts, and what it waits for

**Decided (the user, 2026-10-16): **start everything now**, with three constraints that keep it from being a fork: (1) Mongo executes only through `PlanRunner`; the session type gains a sealed variant and the runner one arm; no reader is added to the legacy `ResultSet` executor; (2) PCT channel A on Mongo takes the plan path, which gains a PCT TDS output (the database owner's files, announced); (3) the dialect class in `sql/dialect/` is announced in the ninth line's section and merged between its phases. The seam then reaches Phase 5 with a real second transport in hand.**

**What is decided.** Whether Mongo is a line of its own, what it owns, and how it is sequenced against the dialect
program's Phase 5 and 6 and the eighth line's corpus harness.

**Option A, a twelfth line; start the parts that need no seam now; land the seam with Phase 5 (recommended).** PR0
(the measurement), PR2's server and launcher, and PR3's translator class (unit-tested through `renderStatement` and
run against the embedded server from a dialect test, as `ClickHouseDialectTest` runs its own) need neither Phase 5 nor
the PCT lane; PR1 (the seam) lands with Phase 5, co-signed; the PCT lanes follow; PR6 waits for the corpus harness.
The line owns `sql/dialect/MongoDb*.java`, `core/src/main/mongodb/`, `testing/.../EmbeddedMongoDb.java`,
`tools/mongodb/`, the Mongo suites and lanes, and the Mongo arms of the registries; every edit in `sql/dialect/` is
announced in the ninth line's section and merged between its phases.
- Pros: no idle wait on another line; Phase 5's interface is designed with a real non-JDBC connector in hand, not an
  imagined one; the seam lands once; two sessions never restructure `sql/dialect/` at the same time.
- Cons: the translator's first judge is its own tests until the lane exists; PR1's timing is the ninth line's.
- Cost: coordination. Complexity: the handshake in `IN_FLIGHT`.

**Option B, start everything now with D2's option B (implement `SqlDialect` whole) and a Mongo session type in the
harness only; the program consolidates later.**
- Pros: the earliest PCT numbers.
- Cons: the executor and the harness fork for one database; the consolidation is a landing of its own later, against
  more code; the ninth line's Phase 5 then has two shapes to reconcile.
- Cost: S now, M later, plus the risk this plan exists to avoid.

**Option C, wait for Phase 5, Phase 6 and the corpus harness, then start.**
- Pros: every interface exists first.
- Cons: Phase 5 and 6 are unscheduled; Phase 5 designed without a non-JDBC member in hand is the risk of §9.1; nothing
  of Mongo is measured meanwhile.
- Cost: time.

**Reversal.** None: order only.

---

## 5. The design

### 5.1 The query target: `sql/dialect/MongoDb.java`

One class, with helpers beside it in the same package (`MongoDbStages`, `MongoDbExpressions`, `MongoDbWire`), every
one an owner by its class name. It writes **Extended JSON (canonical mode)** into the one writer: the text of an
`aggregate` command document, with `"$$p<n>"` at each bind. The command is text because the plan is text
(`ExecutionPlan.Sql.statement`), and Extended JSON is lossless for every BSON type; the connector parses it with the
driver's own `BsonDocument.parse`.

**The IR to the pipeline**, shape by shape (the `passes()` first, then the writer):

| IR | pipeline |
|---|---|
| `SqlSource.Table` (a collection) | the command's `aggregate: "<collection>"`; a scan projects every declared column with `$ifNull` for a missing field (D7), `_id` only when declared |
| `Values`, `Dual` | `aggregate: 1` with `$documents: [ {...}, ... ]` (`Dual` is `$documents: [{}]`): PCT's storeless expressions need no collection and no loading |
| `SqlSelect.where` | `$match: { $expr: <expr> }` |
| `projections` | `$project` (computed columns as `$addFields` first where a later projection reads an earlier one, as `Fold` already isolates) |
| `groupBy`, aggregates, `having` | `$group: { _id: {k: ...}, agg: {...} }`, then `$project` to the output names, then `$match` for `having` |
| `orderBy`, `limit`, `offset` | `$sort` (over computed keys for NULLS FIRST/LAST and the IR's collation), `$skip`, `$limit` |
| `distinct` | `$group` on every output |
| `Join` INNER, LEFT | `$lookup` with `let` and a `pipeline` (the ON condition as `$expr`), then `$unwind` (`preserveNullAndEmptyArrays` for LEFT); RIGHT as the mirrored LEFT; FULL as LEFT `$unionWith` the unmatched right (`$lookup` + `$match` on an empty array); CROSS as `$lookup` with no condition; `CROSS_LATERAL` and `LEFT_LATERAL` as `$lookup` whose pipeline reads the left row's `let` variables; `ASOF_LEFT` through the shared `AsOfAsPick` pass |
| `Subselect`, `Cte` | a sub-pipeline inside `$lookup` (over `$documents` or the collection), or inlined where it is the only source; a recursive `SqlWith` is `$graphLookup` only where the shape is a plain reachability walk, else refused by name (the Cypher line's recursion native decides its own rule per dialect) |
| `SqlUnion` | `$unionWith` (ALL), then `$group` for DISTINCT |
| `Pivot` static | the shared `PivotAsGroupBy` pass; a dynamic pivot through `needsStaticPivot()` (the two-phase staticization) |
| `WindowCall`, `Frame` | `$setWindowFields: { partitionBy, sortBy, output: { x: { <fn>, window: { documents: [a, b] | range: [a, b] } } } }`; ranking functions as `$rank`, `$denseRank`, `$documentNumber`; `lag`/`lead` as `$shift`; `ntile`, `percent_rank`, `cume_dist` emulated from counts |
| `Exists`, `InSubquery`, `ScalarSubquery` | `$lookup` with the subquery's pipeline, then `$size`/`$arrayElemAt` in `$expr` |
| `Lambda`, `FoldCall`, lists | native: `$map`, `$filter`, `$reduce`, `$concatArrays`, `$slice`, `$sortArray`, `$zip`, `$range`, `$arrayElemAt`, `$size`, `$setUnion`, ...; structs and maps as embedded documents (`$getField`, `$setField`, `$objectToArray`, `$arrayToObject`, `$mergeObjects`); `CAPS` says lists, structs and maps are native, as DuckDB's does |
| `Cast` | `$convert` with the BSON type the Pure type names (`scalarTypeName`'s switch), `onError` refused (an error is an error) |
| `DateLit`, `TimestampLit`, date parts and formats | `{"$date": ...}` literals, `$dateAdd`, `$dateDiff`, `$dateTrunc`, `$dateToString` with the format the `formatText` switch spells, `$year`.. `$isoWeek`, `$dayOfWeek` |
| `RawSql` (written for Mongo) | the text is a pipeline array written by the user, run as written (D2); the engine's H2 text refused by name |
| `Catalog` | `listCollections` is a command, not a stage: refused as a grid in the first landings (Postgres refuses it too today); DataCube's catalog goes through `catalogType` (D6) |
| the wires (`csvWire`, `jsonWire`, `tdsText`) | the same composition, in `$project`/`$setWindowFields`/`$group`/`$reduce` (D10) |

**The vocabulary**: `call` is one switch over `SqlFn` (206), `reducer` one over `SqlAgg.Fn` (21), each arm one
expression: spelled (`$abs`, `$ceil`, `$floor`, `$round`, `$trunc`, `$sqrt`, `$pow`, `$exp`, `$ln`, `$log10`, `$mod`,
the trigonometric operators, `$concat`, `$toUpper`, `$toLower`, `$strLenCP`, `$substrCP`, `$indexOfCP`, `$split`,
`$trim`/`$ltrim`/`$rtrim`, `$regexMatch`, `$regexFind`, `$regexFindAll`, `$replaceOne`/`$replaceAll`, `$toInt`,
`$toLong`, `$toDouble`, `$toDecimal`, `$toString`, `$toDate`, `$toBool`, `$type`, `$cond`, `$switch`, `$ifNull`,
`$in`, `$size`, `$rand`, ...), emulated (integer division and `MOD` over the sign rules, `GREATEST`/`LEAST` with null
handling, `REPEAT`, `LPAD`/`RPAD`, `REVERSE` over `$reduce`, `SPLIT_PART`, `STRING_AGG` as `$push` then `$reduce` with
`$concat`, `MODE` as a nested `$group`, `VAR_SAMP`/`VAR_POP` from `$stdDevSamp`/`$stdDevPop` squared, `CORR`/`COVAR`
from sums, `ARG_MAX`/`ARG_MIN` as `$top`/`$bottom`, `QUANTILE` as `$percentile`, `MEDIAN` as `$median`), or refused by
name (`MD5`, `SHA1`, `SHA256`, Levenshtein, Jaro-Winkler and every other walk that needs recursion in an expression;
`HUGEINT` arithmetic past a Long; a date before year 1 in `$dateToString`, measured). Every window function is written
by `windowCall`, every lowering marker refused, exactly as the other tables do (`DialectVocabularyTest`'s
`ONE_EXPRESSION_TABLES`).

**Raising an error** (Pure's `assert`, a `raise` the SQL dialects spell through `raiseEnvelope`): MQL has no `$error`;
the measured candidate is a conversion that fails naming its input (`$toInt` of the message text), whose message the
connector's `RaisedErrors` reads back, as the SQL dialects' envelopes are read. Measured in PR0, refused by name until
then.

**What it reads back**: `wireTypeName` folds BSON type names (`int`, `long`, `double`, `decimal`, `string`, `bool`,
`date`, `objectId`, `object`, `array`, `null`) into the wire vocabulary; `deliversOwn` says a Variant arrives as a
document and a Decimal as Decimal128. There is no `normalize` of JDBC cells: the connector hands the one text through.

### 5.2 The connector: `core/src/main/mongodb/com/legend/exec/MongoDbConnector.java`

Its own source set with the driver on its compile classpath, as `core/src/main/duckdb` holds `DuckDbAppenderLoad` and
`DuckDbCells` beside the DuckDB driver and core compiles against no driver; found through `ServiceLoader` (the
`BulkLoad`/`DriverCells` precedent, `core/BUILD.bazel:329`), or named directly by Phase 5's `Connectors.of(type)` if that
registry lives beside the drivers. It owns:

- `open(ConnectionDefinition)`: a `MongoClient` from the `MongoServers` specification (hosts, the database) and the
  authentication (user and password from the vault reference the model names, as the cloud lanes read theirs: the
  platform holds no credential, C3b);
- `check(session, type)` and `version(session)` from `buildInfo`/`hello` (the version floor, D9);
- `run(RenderedStatement, values)`: `BsonDocument.parse` of the command text, `let` from the slots, `runCommand` on the
  session's database, the one text field of the one result document returned as the plan's text (or the per-row
  documents for `JSON_PER_ROW`), the cursor exhausted;
- the bulk load (`SetupStep.Rows` → `insertMany` of documents the dialect's seed render spells, every cell typed by
  BSON: the loader "spells no SQL and types no value" rule holds because the dialect wrote the documents);
- `raisedMessage` (`RaisedErrors`): the raised text read out of a command error, as MariaDB's is read out of a session
  variable;
- the system database (`SystemDatabase.routeFor` on a server engine): the metamodel tables seeded once per graph as
  collections `metamodel_<table>` in a session-private database (named by the session, dropped with it), read through the
  same table renames; or, for the first landings, metamodel reads refused by name on Mongo (the few PCT tests that read
  it, `getAll`'s `testBasic`, pinned with that reason).

Nothing in `core/src/main/java` imports `com.mongodb` or `org.bson`; `ArchitectureTest.JDBC_AND_DRIVERS` gains both so
the funnel rule sees them, and `DialectBoundaryTest`'s driver census names the driver.

### 5.3 The test backend: `EmbeddedMongoDb`, `tools/mongodb/`, the lane

- `tools/mongodb/mongodb.bzl`: the pinned Community tarball per platform (version, sha256), one binary kept (`mongod`;
  `mongosh` is not needed), a `MONGODB_ROOT` marker, the hub `select` over `//tools/platforms` and
  `EMBEDDED_MONGODB_COMPATIBLE`, the ClickHouse rule's shape line for line (`tools/clickhouse/clickhouse.bzl`). Linux
  x86_64 first; macOS arm64 and Linux arm64 are pins to add when a runner needs them. The module extension in
  `MODULE.bazel` beside `embedded_clickhouse`. The cloud-session distdir script covers the tarball if the proxy refuses
  Bazel's downloader (`docs/CLOUD_SESSIONS.md`).
- `testing/src/main/java/com/legend/testing/EmbeddedMongoDb.java`: started once per JVM on a free loopback port with
  `--dbpath` in the test's temporary directory, `--bind_ip 127.0.0.1`, a user created at start when the lane's
  connection declares one (the engine's `MongoDBConnection` requires an `authentication` block, measured in PR0: the
  grammar may need a lite `# None {}#` arm, a register row if so), stopped at exit, restarted if it dies, its log's tail
  in the failure, as `EmbeddedClickHouse` does.
- The lane: `PctBackend.Lane.MONGODB` (`LEGENDLITE_PCT_BACKEND=mongodb`; its `open()` answers the connector's session,
  which is why the lane enum's return type moves with Phase 5), the storeless runtime for Mongo
  (`StorelessRuntime.onServer` writing a `###MongoDB Database storeless::Store ( )` and a `MongoDBConnection` with the
  server's URL and database `pct`), the five suite classes `Test_LegendLite_MongoDB_<Suite>Functions_PCT` with every
  failure pinned at the first measurement, `PctCensusGate`'s ceilings for the lane, `ChannelBLane`'s pins, and in
  `pct/BUILD.bazel` five `junit_test`s plus a suite and the channel B target, `manual`, with the driver jars as their
  runtime deps and the server as `data`; `//gates:heavy` lists them. The driver's three jars in
  `tools/deps/jars_table.bzl` (`users = ["pct"]` until the server opens Mongo, then core's connector library).
- Phase 6's `MongoDbBackend` (when the interface exists): `openWorkspace()` is a database per package, `isolateFixture()`
  a copy of the fixture's collections (`$out` into the workspace, or a re-seed), `referee()` none (the H2 referee judges
  SQL text; a Mongo lane judges rows only, the `OTHER_LANE` shape the corpus already has for Postgres), `seed` through
  the connector's bulk load.

### 5.4 The model side

- **The store** (`###MongoDB`): a `PMongoDatabase` becomes a store the compiler knows (today it is "carried"): its
  collections are tables with the columns D6 derives, their declared types the BSON types (`storedTypes`, so the
  dialect can read an `objectId` as text and a `date` as DateTime). `include` and `Join` are parsed and dropped by the
  engine itself; lite does the same until a case needs them.
- **The table reference** `#>{db.Collection}#` resolves as it does for a relational store (`TableReferenceChecker`), the
  scan a `SqlSource.Table` whose `name` is the collection, `quoting` bare; a collection or field name with `$` or `.`
  is refused by name.
- **The class mapping** (`*Class: MongoDB { ~mainCollection [db] Coll ~filter ... ~binding ... }`): a relation class
  mapping over the collection's relation, each property the same-named field (nested paths as Variant reads), the
  filter the named one, the binding checked for the class and otherwise unread; `MappingFromProtocol.classMapping` stops
  returning null for it. This is the engine's drop-in surface; parity with the engine's own Mongo test cases in the
  pinned 4.145.0 tree is PR6's.
- **The connection**: `MongoDBConnection` → `ConnectionDefinition(MongoDB, MongoServers, auth)` (D4); the server's
  `ConnectionResolver` opens it through the connector; `EngineText` refuses Mongo by name (legend-engine's Mongo plans
  are not SQL text; their plan nodes are out of scope, `EXECUTION_PLAN_BOUNDARY` §1).
- **Data**: `###Data` rows and `SetupStep.Rows` seed a collection through the bulk load; the `###Relational` CSV seed
  forms do not apply.
- **The register** (`docs/SEMANTICS_REGISTER.md`) gains a row for each deliberate difference: `DatabaseType.MongoDB` as
  a lite extension; a `StrictDate` stored as a BSON date at midnight UTC (BSON has no date-only type); Decimal at
  Decimal128's 34 digits (DuckDB's 38); integers as 64-bit only; a missing field read as null (D7); strings measured in
  code points (`$strLenCP`), which is Pure's rule; the regular-expression dialect (PCRE2 against DuckDB's RE2); the
  Float's text where `$toString` differs from `Scalars.floatRepr` (measured).

---

## 6. The PCT lane on Mongo

- **Channel A** (`PctExecuteNative`): the expression and its injected model execute on the lane's storeless runtime.
  Today the bridge hands a `java.sql.Connection` to `QueryService.execute` and the result comes back through the
  legacy executor (`ExecutionResult.TdsText` under `ExecuteOptions.TDS_TEXT`, named `PCT_RENDER` until 2026-10-17). On Mongo the bridge hands the connector's session,
  and the result comes through the plan path: `TypedQuery.executionPlan` gains a `TDS_TEXT` output (the plan emits
  `Render.tdsText`'s text, the E1 precedent; landed 2026-10-17, §11.2) and `PlanRunner.run` carries it. That is the database owner's step 4 ("the
  callers switch") applied to one caller first (§8.2); it also means the Mongo lane never touches the 24 `ResultSet`
  sites of `Executor`.
- **Channel B** (`ChannelB`): lite's own compiler runs the PCT sources on the lane's database; nothing Mongo-specific
  beyond the session.
- **The census** (`SqlTypeCensus`, `PctCensusGate`): the wire is one text, as it is on every lane under the TDS text option;
  the lane's ceilings are measured on its own target.
- **What to expect** (from the shape of the other lanes, to be measured, never promised): lists, structs, maps and
  Variant are native, so the H2-class refusals (48) do not appear; dates carry milliseconds (microsecond cases pin,
  as on MariaDB); Decimal128 and 64-bit integers pin the big-number cases (`testBigFloatAbs`, `AdjustBy*BigNumber`,
  as they do on ClickHouse); the digests and the string-distance walks refuse (2 on BigQuery, the same here);
  regular-expression dialect differences pin a few (SQLite's 10); the platform's own 34 (instance identity, error
  positions, the 0-based `indexOf`) are the DuckDB lane's. Other lanes first measured between 40 (Snowflake, Trino) and
  83 (BigQuery) own pins and burned down to 4 to 42; a Mongo lane is likely to land in that range and burn down the same
  way. The number is the first run's to tell.
- **The capability matrix** gains a Mongo column on regeneration (`bazel run //docs:update_dialect_capability`), and
  `reach.tsv` the lane's reach: that page is the acceptance artifact for "every function decided, by name".

---

> Cut in the 2026-11 docs cleanup: §7, the six planned landings, and §8, the asks to the other lines (2026-10-16). Full text: `git show de2745351:docs/MONGODB_PLAN_2026_10_16.md`.

---

## 9. Risks, and the measurements that settle them (PR0)

1. **The seam is designed JDBC-only** (the one structural risk): Mongo would fork the executor and the harness, the
   debt this plan exists to avoid. Settled by §8.1, before PR1.
2. **Row order of the wire's `$push`**: the manual documents `$setWindowFields` output as ordered by `sortBy`; `$group`
   after `$sort` is commonly relied on and not guaranteed in so many words. Measured: 10,000 sorted rows pushed both
   ways, compared to the sorted order, repeated.
3. **The 16 MB document limit**: a result text past it fails in the server. Measured: the error's code and text, so the
   wall is named; `JSON_PER_ROW` (one document a row) is the form for large results, as streaming is today.
4. **Text forms**: `$toString` of a double (`1.0`, `1e21`, `0.1+0.2`), of a Decimal128 (scale kept?), of a date
   (`$dateToString` with milliseconds and the `+0000` zone text PCT expects), of a Variant (key order, spacing) against
   `Scalars.floatRepr`, `DateFmt` and the JSON key order the SQL dialects produce. Each difference is an emulation or a
   register row.
5. **Null and type order**: BSON's total order and missing-vs-null (D7) against the PCT null tests; measured on the
   collection suite's null cases.
6. **Decimal128** (34 digits, rounding mode) against the PCT decimal cases that DuckDB passes at 38.
7. **Regular expressions**: PCRE2 against the PCT patterns (`REGEXP_EXTRACT_ALL`, lookarounds, `\d` classes).
8. **The raise envelope** (§5.1): whether a failing `$convert` carries the message text whole in the command error.
9. **`$documents` with `let` at the database level** (`aggregate: 1`): measured, since the PCT's storeless expressions
   depend on it.
10. **The authentication block**: the engine's `MongoDBConnection` grammar has no "none" form; whether the lane creates a
    user at start or lite accepts a declared lite extension (`# None {}#`, a register row) is decided by what the
    grammar accepts (read in the pinned tree) and measured on the embedded server.
11. **The system database on Mongo** (the metamodel store as collections): the cost of seeding per graph on a server
    engine was measured for the SQL servers (the ClickHouse session's limits, §0 of the dialect plan); Mongo's
    session-private database is cheaper to drop but the seed is the same size. Refusal by name is the honest first
    answer if the measurement says so.
12. **Licence posture** (D8): the user's confirmation.
13. **Size**: a pipeline is more text per IR shape than SQL; PR3's estimate is the widest. The capability matrix keeps
    the count honest: every arm is decided, spelled or refused, and the pages show which.

**Measured, 2026-10-17 (the homework before the PCT lane, §11.2; MongoDB 8.0.17 from `@embedded_mongodb`, in
`//core:mongodb_plan_test`), each with where it is read back:**

- *3, the 16 MB limit* (`MongoDbPlanTest.theWholeTextWiresAreBoundedBySixteenMegabytes`): a CSV wire of 100 rows x
  100 KB (10,000,495 characters) passes in about 1.3 s; 200 rows x 100 KB fails in the server with error 16493
  (`Location16493`): `PlanExecutor error during aggregation :: caused by :: RCString too large. Requires
  size=16800896 < limit=16777216`. The wall is named; the runner carries the server's message. A second finding: the
  wire's row join is `$reduce` over `$concat`, quadratic in the text (2,000 rows of 100 B in 139 ms; 8,000 in
  1,829 ms), so a large whole-text result is slow before it is too big. `STREAMED_JSON` (a document a row) is the
  form for both; the whole-text wires are the PCT harness's (`TdsText`, one text) and the product's snapshot forms.
- *4, text forms* (`MongoDbPlanTest.theServersNumberTexts`, the raw operators then the target's spelling): `$toString`
  of a double is the shortest round-trip text (`0.1+0.2` is `0.30000000000000004`, as Pure prints it) but in
  scientific notation from 1e16 up (`1e+16`, `1e+20`; `1e15` is plain) and below 1e-4 (`1.5e-08`), and without a
  fraction when integral (`2.0` is `2`); a Decimal128's text is its own scientific form (`$convert` of 1e20 is
  `1.00000000000000E+20`, 15 significant digits; of 2.0 `2.00000000000000`; 1.5e-8 rounded at 18 `1.5000000000E-8`);
  `$convert` of `"+20"` to long is 20; `$type` of a double literal is `double`. The target's answer: a number's text is
  `$toString`'s bound once, and the platform's portable spelling (`FloatCanonText`: the exponent unfolded as text, a
  fraction digit kept) is written over it -- a double gets Pure's whole Float spelling, a decimal its exponent
  unfolded, a HUGEINT its digits alone -- in the cast to text and in the JSON wire's cells. After it: `2.0`,
  `100000000000000000000.0`, `0.000000015`, `0.30000000000000004`, as Pure and DuckDB spell them. Dates:
  `$dateToString` with `%L` carries the millisecond and the `+0000` text is literal; a Date spells `%Y-%m-%d`.
- *5, null and type order*: a select's `$sort` carries each key's null flag (`__nullN`, D7), so nulls place as the
  key's `NULLS FIRST/LAST` says, and an ordered aggregate's pushed values carry the flag too (`pushedWithKeys`); the
  differential's `extend_if_null`, `group_join_strings` and `sum_all_null` cases are EQUAL to DuckDB. BSON's order
  across types is not reached: every comparison is typed by the lowering.
- *8, the raise envelope*: a `$convert` to `int` of the message between the platform's U+001F sentinels, concatenated
  with an empty text that depends on the row (`UNFOLDED`): measured, the server's optimizer folds a constant
  conversion in a branch not taken and fails the pipeline before any row (`Failed to optimize pipeline`), so no raise
  may be a constant; with the row dependency the message arrives whole in the command error and the connector unwraps
  it into the platform's `DataError`.
- *9, `$documents` with `let` at the database level*: `aggregate: 1` with a `$documents` stage runs the storeless
  expressions (every scalar case of the differential); a `$lookup` under it takes `let` variables and a `from` where
  the inner reads a collection, none where it is literal.
- *10, the authentication block*: the engine's `MongoDBConnection` grammar has no "none" form; the lane makes the
  user (`EmbeddedMongoDb`: a root administrator through the localhost exception, then the user `pct` with `dbOwner`
  on its database) and the connection names it with a `SystemPropertiesSecret` password, so no lite extension of the
  grammar is needed.
- *2, the ordered `$push`*: not measured in bulk yet (the 10,000-row repetition); the differential's ordered
  aggregates are EQUAL on three rows. Open until the PCT lane's ordered cases run.
- *6, 7, 11, 12, 13*: open; 6 (Decimal128) is partly answered by item 4's texts (15 significant digits through
  `$convert` from a double; a Pure Decimal literal keeps its own digits).

---

## 10. Sources

Read in this tree at 59b547a (main, 2026-10-16), each cited above by path:

- `AGENTS.md` (the invariants: 3, 3a, 4, 7, 8; the execution and reference-checkout tenets);
  `docs/TENET_CHARTER.md` (Clauses 1, 2, 6); `docs/WORLD_MAP.md`.
- `docs/DIALECT_LEAK_CENSUS_AND_PLAN_2026_10_12.md` (§0 the handoff and the proof method; §2 the verdict; §4 the
  target; §5 the phases, 0 to 4 done; §6 decisions D1 to D11, P1b); `docs/dialect-capability/README.md`;
  `docs/IN_FLIGHT.md` (lines 3, 1183 to 1300 the eighth line, 1301 to 1482 the ninth line, 1689 the rules).
- `docs/CLOUD_BACKENDS_PCT_2026_10_11.md` (§2 the order, §11 the candidates and the non-SQL stores, §16 channel B,
  §19 ClickHouse, §20 the corpus on every database); `docs/CYPHER_PLAN_2026_10_15.md` and
  `docs/CYPHER_STUDY_2026_10_15.md` §0 (the method: a spec, an oracle, a ledger, refusals by name);
  `docs/EXECUTION_PLAN_BOUNDARY_2026_10_05.md` (P3, §10 E); `docs/PLAN_EXECUTION_SPLIT_AND_DATABASE_OWNER_2026_10_03.md`;
  `docs/STORE_TYPES_HOMEWORK_2026_10_02.md` (now in `docs/HISTORY.md`); `docs/SEMANTICS_REGISTER.md` (S43, S48, S63); `docs/GATES.md` (the
  lanes); `docs/CLOUD_SESSIONS.md`; `docs/PCT_AUDIT.md` §1 (now in `docs/HISTORY.md`).
- Code: `sql/dialect/SqlDialect.java`, `AnsiSqlRenderer.java` (`passes`), `SqlWriter.java`, `RenderedStatement.java`,
  `DialectCapability.java`; `sql/SqlSource.java`, `SqlSelect.java`, `SqlExpr.java`, `SqlFn.java`, `SqlAgg.java`;
  `database/Databases.java`, `Target.java`; `model/ConnectionDefinition.java`; `exec/Sessions.java`, `Executor.java`,
  `PlanRunner.java`, `BulkLoad.java`; `executionplan/ExecutionPlan.java`; `PlanMaker.java`; `Compiler.java`
  (`dialectOf`, `executesOn`); `lowering/WireRender.java`, `Render.java` (`tdsText`), `TdsTextWrap.java`;
  `test/StorelessRuntime.java`; `parser/section/MongoDBSectionGrammar.java`, `MappingProtocolParser.java`,
  `protocol/Protocol.java`, `ConnectionReader.java`, `model/FromProtocol.java`, `MappingFromProtocol.java`;
  `core/src/test/java/com/legend/ArchitectureTest.java` (`JDBC_AND_DRIVERS`, the funnel rules), `DatabaseOwners.java`;
  `pct/BUILD.bazel`, `pct/.../extension/PctBackend.java`, `PctExecuteNative.java`, `channelb/ChannelBLane.java`,
  `Test_LegendLite_ClickHouse_EssentialFunctions_PCT.java`, `PctCensusGate.java`;
  `testing/.../EmbeddedClickHouse.java`, `tools/clickhouse/clickhouse.bzl`, `tools/deps/jars_table.bzl`,
  `core/BUILD.bazel` (`drivers`, `duckdb_load`), `gates/BUILD.bazel`; `scripts/parser/fixtures/t2-mongodb.pure`.
- MongoDB facts (the manual, from memory, each marked "re-check at the pin" and settled by PR0): the version floors of
  §4 D9; `let` on `aggregate`; `$documents`; `$setWindowFields`; Decimal128; the 16 MB document limit; PCRE2; no
  digest or recursion operators in the expression language; `$function` as server-side JavaScript. Nothing from
  MongoDB's documentation or code enters the repository.

---

## Upstream facts found while building

What MongoDB 8.0 (the lane's pinned `mongod`, 8.0.17) and legend-pure answered while v0 was built (§11, cut), kept
here because §0–§10 do not say it. Each was measured on the lane and named in a differential case or a pin.

- **`$setWindowFields`.** A ranking operator takes a `sortBy` of exactly one field. A `"current"` bound is the current
  document alone, not SQL's RANGE peers. A range window needs a numeric field.
- **Operator pitfalls.** A literal array given to a one-argument operator (`$reverseArray`, `$size`) is read as the
  argument list. Converting any string to Boolean gives true. `$strLenCP` of null is a server error.
- **Missing is not null.** `$arrayElemAt` past the end returns nothing (missing), which `$eq` does not treat as null.
  `$pow` of two integers is an integer. `$addFields` merges a plain document into the field's existing document, so a
  document value must be wrapped in `$literal`.
- **No JSON in the pipeline.** MongoDB has no expression that writes a value as JSON and none that parses JSON. A
  document literal's fields are evaluated as expressions unless wrapped in `$literal`. The server's nesting limit is
  two hundred levels.
- **JSON null is BSON null.** A Variant holding JSON null is BSON null; MongoDB has no separate JSON null, so the cell
  is a null cell where Pure prints `'null'`.
- **Dates.** `$dateToString` fails for years past 9999; `$dateAdd` takes a bounded month count; there is no microsecond
  unit; `$convert` to a date rejects a single-digit month or day, which Pure reads.
- **Text.** There is no regex-replace operator (`$replaceAll` replaces literal text). `$toHashedIndexKey` exposes the
  database's own hash. In 8.0, `$convert` from text to `binData` reads UTF-8 and back writes base64.
- **legend-pure's `stringToTDS`** (the PCT's result reader) types an unquoted cell by its text, not by the header, and
  trims an unquoted cell's spaces; it reads an empty cell as null, so an empty string cannot cross; it builds no Date or
  Decimal through `map`.
- **legend-engine and legend-pure conventions.** `timeBucket` uses the engine's origins, 1970-01-01 (1969-12-29 for
  weeks), with months counted from 1970-01. The PCT runs under the engine's feature flag
  `CORRECT_SQL_SUBSTRING_INDEXING`. Pure's `mode` answers the smallest of the most frequent values. The PCT relation
  suite spells a TIMESTAMP cell with three millisecond digits (`T13:12:11.000+0000`); the newer reading in
  `docs/result-boundary-homework/README.md` §7 (2026-10-29) shows that is the PCT harness's own spelling
  (`TestTDS.java:209`), not the engine's product, which writes nine digits.

> Cut in the 2026-11 docs cleanup: §11, v0 as built (2026-10-16 to 2026-10-17: what landed, the homework before the PCT lane, the lane's first measurement and the burn-down); the four differences from SQL it kept (then register rows S72 to S75, ids since reused) were three defects of the target, fixed, and one item of the SQL side; the facts it measured are listed above. Full text: `git show de2745351:docs/MONGODB_PLAN_2026_10_16.md`.
