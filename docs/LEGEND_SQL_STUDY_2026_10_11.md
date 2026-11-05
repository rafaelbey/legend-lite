# Legend SQL ("AlloySQL"): what upstream built, how to port it, and SQL/dbt as modeling surfaces

**Status: research and brainstorm (2026-10-11), with the user's first decisions recorded in §8, kept as spec input
for the port. The `#SQL{...}#` island was then built, 2026-10-11 to 2026-10-13, and run over the PostgreSQL regression
suite on 2026-10-17 (§9's build records, cut in the 2026-11 docs cleanup; the facts they measured are listed after
§8).** It answers the user's question of 2026-10-11: study the Postgres-compatible SQL front end
of legend-engine/legend-pure deeply, port it to legend-lite, and consider (a) SQL as a modeling surface beside
Pure grammar and Python, and (b) a dbt-compatible front end ("Postgres-compatible for queries, dbt-compatible
for modeling, Postgres dialect throughout"). §0 is the answer; §1–§2 are the evidence; §3–§6 are the design;
§7–§8 are the plan and the open decisions.

Reference checkout used: `finos/legend-engine` at `e60ab67a` (2026-10-10). Paths below are relative to that
repository unless they start with `core/`, `docs/`, `python/`, `warehouse/`. "AlloySQL" does not occur in either
upstream repository. Alloy is Legend's internal name, and the thing the user means is legend-engine's
`legend-engine-xts-sql` module family plus its grammar in `legend-engine-xt-relationalStore-postgresSql`.
**legend-pure has no SQL front end at all.** Its only SQL code is the relational store's natives
(`legend-pure-store-relational`).

---

## 0. The answer: yes, and it is one project, not three

1. **The three ideas share one core.** Each is a **Postgres-SQL → Pure Relation translator** plus a shell:
   - *Query front end* = translator + Postgres wire server + a catalog.
   - *dbt front end* = translator (every dbt model body is a SELECT) + a project loader (YAML, a Jinja subset) +
     materializations (writes).
   - *SQL modeling* = translator (view and function bodies) + a DDL reader (CREATE TABLE/TYPE/DOMAIN/VIEW/FUNCTION
     → model elements).

   Build the translator once, well, and the rest are thin layers.

2. **Port the spec and the tests, not the code.** Upstream has three parts:
   - A **CrateDB grammar**. About 80% of it is dead surface that is parsed and then rejected.
   - An **8.2k-line translator written in Pure**. It builds Pure graphs by hand, prints them to text and recompiles
     them. It keeps a TDS branch and a Relation branch for almost every operation.
   - A **Crate-derived wire server**. It starts a Docker `postgres:10.5` to answer catalog queries and calls the
     engine over HTTP.

   legend-lite's tenets rule out running or porting engine Pure: the world map, C6, and the reference-checkout
   tenet. The code's quality argues the same way. What is worth taking:
   - the source conventions (`tb`/`func`/`service`/`csv`, the `tb__pkg` schema mangling);
   - the function table, as a spec;
   - above all, the **test corpora**:
     - 1,703 parity cases with a real-Postgres oracle at the pinned 4.145.0 (2,340 in a later upstream);
     - 230 SQL→Pure transpile pairs;
     - about 1,060 reverse-PCT triples;
     - 83 parser round-trip cases.

3. **Target `protocol.spec.ValueSpecification` (Relation API), entering at `Compiler.query(ctx, vs)`; do not
   target the SQL IR.** Legend sources (functions, services, classes through mappings) are resolved in G and H,
   which the IR is downstream of. The real compiler then types the query **once**. Upstream instead re-derives
   the schema at 22 sites and re-compiles through text.

4. **Postgres is the spec** (the user, 2026-10-11: "stick as close as possible to native Postgres for
   everything"). This covers the wire protocol, the grammar, names, the catalog **and results**:
   - The translator emits Pure whose results equal Postgres's on every backend.
   - The parity corpus's real-Postgres oracle defines "done".
   - Where matching Postgres would need Java to compute values, the gap is a `docs/SEMANTICS_REGISTER.md` row,
     never a silent default (§3.4). Postgres's many date input formats on DuckDB are one such case.

   Upstream does not hold itself to this (§1.3).

5. **SQL is the right modeling surface for the relational layer and the wrong one for the object layer.**
   - It fits stores, relation-valued functions, enums and constrained primitives.
   - It cannot say classes with inheritance, to-many properties, M2M mappings, milestoned navigation or graph
     fetch.

   dbt's YAML fills exactly that gap: columns, contracts, constraints, `semantic_models` (entities, dimensions,
   measures) and the open `meta:` key. So "dbt-shaped project, Postgres-dialect SQL" is coherent, and it maps
   almost one to one onto Legend (§5.1).

6. **The dbt angle has a real gap to fill.**
   - **What dbt v2 offers:** it is a Rust rewrite that "produces and validates a logical plan for every query"
     before running anything.
   - **Where it falls short:** strict type-checking needs `dbt login`, the type checker ("Level 2 SQL
     comprehension") is proprietary, and **Postgres is not a v2 adapter**.
   - **What legend-lite would add:** it would compile dbt-shaped Postgres projects with full typing, column
     lineage, a real semantic layer (classes and associations) and multi-backend rendering. dbt has none of
     these, and they come from legend-lite's own tenets. Sources: §10.

7. **The cost is real, and so is the contention.** Two things are missing in the compiler:
   - writes: `TypedWrite` to a store throws at `core/.../lowering/Lowerer.java:3062`;
   - `intersect`/`except` natives and recursive CTEs.

   There is also contention. pgwire is the paused server program's leg W3, and "Make a class" belongs to the
   DataCube line. A first useful slice is smaller: SQL over HTTP and in `#SQL{}#` islands, with no wire protocol
   (§7, phase 1).

---

## 1. What upstream built (the study)

### 1.1 Module map and sizes

| Part | Where | Size (lines) |
|---|---|---|
| Grammar (ANTLR4, **CrateDB's `SqlBase`**, Crate.io Apache header) | `legend-engine-xts-relationalStore/.../legend-engine-xt-relationalStore-postgresSql/...-grammar/src/main/antlr4/.../SqlBase{Lexer,Parser}.g4` | 497 + 868 |
| Parse tree → AST visitor | `...-grammar/.../from/SqlVisitor.java` | 2,327 (about 700 are `unsupported` stubs) |
| AST → SQL text | `...-grammar/.../to/SQLGrammarComposer.java` | 746 |
| Postgres date-string parser | `...-grammar/.../from/PostgresDateParser.java` | 485 |
| AST metamodel (Pure; Java POJOs generated from it) | `...-postgresSqlModel-pure/.../metamodel.pure` | 626 (81 classes, 15 enums) |
| SQL → Pure translator (**Pure**) | `legend-engine-xts-sql/legend-engine-xt-sql-transformation/legend-engine-xt-sql-pure/.../binding/fromPure/` | 8,159 (`fromPure.pure` alone 6,347) |
| Source providers (`service`, `func`, `relationalStore`) | `legend-engine-xts-sql/.../legend-engine-xt-sql-providers/*` | 1,334 |
| HTTP API (`/sql/v1/...`) | `legend-engine-xts-sql/legend-engine-xt-sql-http-api` | 2,034 |
| `#SQL{...}#` expression island | `legend-engine-xts-sql/legend-engine-xt-sql-expression/*` | 1,148 |
| Postgres wire server (Crate-derived pgwire, Netty) | `legend-engine-xts-sql/legend-engine-xt-sql-postgres-server` | 15,506 main + 5,557 test |
| reverse-PCT corpus | `legend-engine-xts-sql/.../legend-engine-xt-sql-reversePCT` | 15,244 Pure |
| e2e parity corpus | `legend-engine-xts-sql/legend-engine-xt-sql-e2e-tests/src/test/resources/parity-tests/` (4.145.0) | 73 YAML files, **1,703 cases** at the pin (2,340 in a later upstream) |

### 1.2 Grammar and AST

**Surface.** The grammar has 41 statement alternatives: transactions, SET, SHOW, DDL, DML, GRANT, cursors, and
CrateDB-only constructs (BLOB TABLE, SNAPSHOT, KILL, OPTIMIZE, `SET LICENSE`). **Only the query alternative
produces an AST.** Every other alternative throws `Unsupported Operation` (119 call sites, `SqlVisitor.java:2323`).

**Supported in queries:**
- CTEs, including `RECURSIVE` (kept as a flag).
- UNION, INTERSECT and EXCEPT [ALL].
- CROSS, INNER, LEFT, RIGHT, FULL and NATURAL joins, with ON or USING, plus `LATERAL`.
- Scalar, IN, EXISTS and ANY/ALL subqueries.
- Windows: inline, named `WINDOW w AS`, ROWS/RANGE frames, `FILTER (WHERE)`, `WITHIN GROUP`, ordered
  aggregates.
- HAVING; `ORDER BY ... NULLS FIRST/LAST`; LIMIT, OFFSET and FETCH FIRST.
- VALUES.
- Table functions with `name => value` arguments.
- `CAST`, `::` and `type 'lit'`.
- INTERVAL, arrays and slices.
- `$n` and `?` parameters.
- `$$`/`$tag$` strings.
- Postgres regex operators (`~ ~* !~`) and JSON operators (`-> ->> #> @>`...).

**Absent:**
- GROUPING SETS, ROLLUP and CUBE (not even tokens).
- QUALIFY, DISTINCT ON, SIMILAR TO, GROUPS frames, `E''` strings, `TRY_CAST`, `AT TIME ZONE`.
- Multi-statement input.

**Quirks that must not be ported:**
- Unquoted identifiers **keep their case**; Postgres folds them to lowercase (`SqlVisitor.java:902`).
- INTERSECT binds with the same precedence as UNION (`:1010`); in Postgres it binds tighter.
- Every decimal literal becomes a `DoubleLiteral`.
- **Silent drops:**
  - a parenthesised query loses its trailing ORDER BY/LIMIT (`:1009`, `:1039-1052`);
  - interval signs, qualifiers and unknown units are dropped (the regex at `:43` uses a character class instead of
    alternation);
  - `WITH ORDINALITY` and `IGNORE NULLS` are dropped.
- A column named `user` parses as `CURRENT_USER`.
- The AST has **no source positions**, and no record of whether an identifier was quoted.

**The composer is not a fixpoint.** Its 83 round-trip tests compare text case-insensitively, and the lowercased
input is checked too. Bugs found by reading the code:
- `FULL JOIN` renders as `null JOIN`.
- `count(DISTINCT x) FILTER (...)` loses both the DISTINCT and the filter.
- `'` is not re-escaped.
- `NOT (a OR b)` renders as `not a OR b`.
- Arrays print in CrateDB's `[...]` syntax.

### 1.3 The translator (SQL AST → Pure)

**Pipeline**:
1. Java parses the SQL, and the generated `ProtocolToMetamodelTranslator` turns the AST into Pure instances
   (`.../http-api/.../SQLExecutor.java:452`).
2. Java `QueryRealiaser` gives every relation a unique alias.
3. Pure `sqlToPure` (`fromPure.pure:194-256`) builds `SimpleFunctionExpression` graphs by hand
   (`compileUtils.pure:67-111`, `TDSBuilder` `:5894-6328`).
4. In relation mode the result is **printed to Pure text together with every dependent element, wrapped in a
   container function, and recompiled** (`compileUtils.pure:247-283`).
5. Then the plan, then `PlanExecutor`.

**Target shape: two branches.**
- Legacy TDS (`project/restrict/olapGroupBy`, `$row.getInteger('X')`), or the Relation API (`~col`,
  `extend(over(...))`).
- The choice is made by the sources (`rootContext`, `:5540-5546`); mixing the two throws.
- A `$relation` flag is tested at 116 sites. Several features are Relation-only: intersect/except, subqueries,
  LATERAL, VALUES, recursive CTEs, ordered/filtered aggregates.
- **A port targets the Relation API only, which roughly halves the logic.**

Example (`testTranspile.pure`, `testGroupBy`):

```sql
SELECT count(Integer) AS "count" FROM service."/service/service1" GROUP BY String
```

```pure
...->groupBy(~[String], ~[count:{x|$x.Integer}:{y|$y->count()}])->select(~[count])
```

**Sources.** Every FROM item becomes `(type, args)`:

| SQL | Meaning | Provider |
|---|---|---|
| `service('/pattern', coordinates=>'g:a:v', p=>1)` or `service."/pattern"` | a Legend service (single or multi-execution) | Java `LegendServiceSQLSourceProvider` |
| `func('pkg::f__Relation_1_', p=>v)` | a function returning a Relation/TDS | Java `FunctionSQLSourceProvider`, Pure dynamic source |
| `relationalStore(connection=>.., store=>.., schema=>.., table=>..)` | a raw table plus a synthesised runtime | Java |
| `tb('pkg::DB.schema.table')` | `#>{pkg::DB.schema.table}#` | Pure dynamic source |
| `csv('a,b\n1,2')` | an inline `#TDS{}#` | Pure dynamic source |
| `unnest(arr)`, `var('name')` | flatten, or a Pure variable in scope (for `#SQL{}#`) | table-function processors |
| `x` (one part) | a CTE | — |
| `a.b` (two parts) | rewritten to `a('b')` | — |

There is no class, data space or data product source; Legend's object layer is not addressable from Legend SQL.

**Catalog exposure:**
- `sql/v1/schema/getSchema` publishes only Database tables, as `tb` relations.
- The schema name is mangled as `tb__<pkg>`, with `_` → `__` and `::` → `_`.

**Expressions:**
- `processExpression` handles 31 node kinds.
- `functionProcessors()` has about 110 named Postgres functions: 17 aggregate, 29 math, 33 string, 6 date,
  11 window, plus conditional, collection, JSON and `to_char` with its own token engine. Each one picks a Pure
  overload from its argument types. Unknown names fail.
- Types come from a hand-maintained `QueryColumn{name,type,multiplicity}` environment, re-derived by `clone()`
  (`:5832-5865`, "resolve schema is expensive").
- A contextual "expected type" slot coerces literals. In `DateCol > '2023-01-01'`, the string becomes a date
  through the **Java native `parsePostgresDate` at translation time**. Under legend-lite's C2 that is a host
  computation, and a port must not do it (§3.4).
- **NULL semantics:**
  - Comparisons choose `[0..1]`-aware overloads.
  - `NOT IN (subquery)` is spelled out three-valued.
  - `=` keeps SQL three-valued logic only through a **plan-level flag**, `LEGACY_SQL_NULL_UNSAFE_EQUALS`
    (`fromPure.pure:326`). This is upstream conceding that SQL `=` is not Pure `==`.

**Explicitly unsupported (throws):**
- NATURAL JOIN; LEFT, RIGHT or FULL LATERAL.
- Interval frame bounds.
- Multi-column IN.
- Duplicate output names.
- WITH across runtimes.
- WITH RECURSIVE with UNION, or with ORDER BY/LIMIT.
- DDL, DML and SET.

The parity corpus quantifies the gap. Its per-case `expected_rel_status` columns read **1,106 PASS / 1,039
ERROR / 88 BUG / 45 FAIL / 62 SKIP of 2,340** for the Relation path in a later upstream; at the pinned 4.145.0,
**1,026 PASS / 518 ERROR / 61 BUG / 36 FAIL / 62 SKIP of 1,703** (§9, cut in the 2026-11 docs cleanup, compared legend-lite's).

**The `#SQL{...}#` island** (`xt-sql-expression`) makes SQL a Pure *expression*: `#SQL{select a from tb('p::DB.t')}#->filter(x|$x.a==1)`.
- Lambda variables in scope are visible through `var('name')`.
- legend-lite already parses the island (`core/.../parser/SpecParser.java:3140`) and refuses it at the Typer
  (`core/.../compiler/spec/Typer.java:180`).
- Once a translator exists, that refusal becomes a call to it.

### 1.4 The wire server

- **Inherited from CrateDB:**
  - **Protocol:** Netty, protocol v3, simple and extended query (Parse, Bind, Describe, Execute, Sync, Close,
    portals). No COPY.
  - **Authentication:** cleartext, GSS/Kerberos with credential delegation, or none. No MD5 or SCRAM, and **no
    TLS** (always answers `N`).
  - **Cancel:** BackendKeyData is commented out, so a client's cancel only closes a connection.
  - **Transaction status:** ReadyForQuery always reports `I`.
  - **Errors:** every error is SQLSTATE `XX000`, and a 20-frame Java stack trace goes back to the client in the
    `W` field.
- **Dispatch:**
  - The raw ANTLR tree classifies each statement:
    - BEGIN and SET are no-ops ("TODO: Handle set queries");
    - `SHOW TRANSACTION ISOLATION LEVEL` gets a constant answer;
    - a query with no table, or one naming `information_schema` or `pg_*`, is sent to the metadata database;
    - everything else is a Legend query.
- **Catalog:** the server **starts a Docker `postgres:10.5`** (`.../protocol/sql/SQLManager.java:110`) with a
  90-connection Hikari pool.
  - Per session it creates a `metadata_<n>` schema, copies `pg_class`/`pg_attribute`, and inserts Legend's
    tables and columns.
  - Only the five *qualified* `pg_catalog.pg_*` names are rewritten. Unqualified `pg_class`, `pg_type` and
    `information_schema.*` read the container's real catalog.
- **Legend execution:**
  - **Remote:** the SQL text goes over HTTP to `/api/sql/v1/execution/executeQueryString`, or, for databases
    named `projects|g:a:v|...`, as `#SQL{q}#->from(runtime)` to `pure/v1/execution/execute`. The TDS JSON comes
    back stream-parsed.
  - **Describe:** it does a second full compile.
  - **Parameters:** bind parameters are **unsupported** on Legend queries (`setObject` throws).
- **Types:**
  - Decimal, Number and Numeric are sent as `float8`.
  - Timestamps are carried as epoch milliseconds.
  - Unknown types fall back to `varchar`.
- **Tests:**
  - pgjdbc only. There are no psql `\d`, DBeaver, Tableau or Power BI probe tests, and the README says "More
    details to follow".
  - The e2e suite uses Testcontainers `postgres:16-alpine` as the oracle and ratchets per-case statuses.

**What would be portable as plumbing (about 7–8k lines, Apache-2.0, keep the Crate NOTICE).** It is not used:
D3 decided on a hand-written protocol.
- the decoder, messages, type codecs, `Session`/`Portal`, the per-connection future chain, and `DelayableWriteChannel`.

**What must be replaced:**
- the HTTP bridge, the Docker catalog, the type table, the `projects|` database convention, and Kerberos
  `Subject.doAs`.

### 1.5 The test estates, and how legend-lite may use them

Under the reference-checkout tenet these files are **test input**, never runtime.

| Corpus | Content | Use |
|---|---|---|
| `parity-tests/**/*.yaml` + `schema.yaml` | 1,703 SQL cases at the pin (2,340 later) over a seeded schema (persons, departments, orders…), categorised (functions 882, operators 638, structural 590, format_tokens 80, compositions 59, window_frames 44, predicates 37), oracle = real Postgres | **The conformance target.** Commit the oracle's answers as fixtures generated once against PG16, so CI needs no Docker, and ratchet statuses as upstream does |
| `testTranspile.pure` | 230 SQL → expected Pure pairs (TDS and/or Relation) | Translator unit tests: the Relation half, normalised to legend-lite's canonical Pure text |
| reverse-PCT | about 1,060 `(PCT id, Pure expr, SQL)` triples, self-contained through `csv()`, plus 259 expected errors | Ties the function table to the PCT lane: each SQL string must agree with the PCT expectation of its Pure twin |
| `TestSQLRoundTrip.java` | 83 parse/compose cases | Parser round trip, rebased onto a real AST fixpoint |
| `parsePostgresDate.pure` | 27 date-format cases | Spec for which literal spellings to accept (§3.4) |

---

## 2. What legend-lite has today (the seams)

- **No SQL text parser of any kind.** `docs/LEGEND_SQL_VISION.md`'s "N-in readers" were never built:
  - its writers (dialects) and its two prerequisites (`SqlType`, `ArchitectureTest.sqlLayerIsStandalone`) are done;
  - its parser core and its "SQL → Pure writer" are not.

  The closest relatives:
  - `normalizer/RelOpTranslator` turns `###Relational` dynaFunction trees, which are SQL-ish, into Pure.
  - `parser/GqlParser` (720 lines) is a hand-written parser for a foreign grammar.
  - `sql/dialect/RawSqlBoundary` is a regex H2→backend shim. Its own javadoc names the parser leg as its
    replacement.
- **No pgwire.** The paused `docs/SERVER_PROGRAM_2026_09_26.md` plans it as leg W3 (decision 5, pgwire before
  Flight SQL), but for the *warehouse*, a DuckDB SQL passthrough, not for Legend SQL.
- **Entry points a front end needs, which exist:**
  - **Queries:** `Compiler.query(ModelContext, protocol.spec.ValueSpecification)` (`core/.../Compiler.java:559`),
    "a query that arrives ALREADY PARSED… nothing is re-spelled as text". `TypedQuery.plan`/`.resultType` then
    give Describe its columns without a second compile.
  - **Models:** `Protocol` records → `model/FromProtocol` → `ParsedModel` → `Compiler.buildModel`
    (`Compiler.java:221`). `FromProtocol` says "everything legend-lite compiles comes through here".
  - `protocol.spec` records (`AppliedFunction`, `LambdaFunction`, `ColSpec`, `AppliedFunction.tableReference`
    for `#>{}#`) are the construction surface. `pure-protocol/src/relation.ts` is a working Relation builder
    that is byte-identical to the parser's output, and the best model for a SQL→Relation emitter.
- **Relation natives** cover what a SELECT needs: filter, extend, groupBy, aggregate, join, asOfJoin, lateral,
  concatenate, distinct, select, rename, sort, limit/drop/slice, over and frames, the ranking/value windows,
  `exists`/`in`, quantified comparisons. **Missing:** intersect, except, unpivot, recursion.
- **Writes are absent:**
  - `Lowerer.java:3062` "TypedWrite to a store destination is not yet implemented";
  - `SqlDdl` has only Create/Drop Table/Schema;
  - `SqlDml` has no MERGE or UPDATE.

  `docs/BI_AND_ETL_PLAN_2026_09_29.md` F1 and Track C (C1 materialize, C2 pipelines of Pure functions, C4
  incremental, C5 compiled data-quality checks) plan exactly what dbt materializations need.
- **Modeling:**
  - **Parse-only today:** `PersistenceDefinition` (nothing executes it), `MeasureDefinition` (units, not
    metrics), `DataDefinition`, `PrimitiveExtensionDefinition`.
  - **Working:** the relation class mapping `ClassMapping.RelationFunction` (`~func`), which is what "a SQL view
    becomes a class" lowers to.
  - **Planned:** `docs/DATACUBE_MODEL_DESIGN_2026_10_10.md` plans "Make a class" (upstream's
    `promoteRelationToModel`) and Python's `grid.make_class`, `page.relate`.
- **Python** (`python/legend_lite`) works on Pure text and protocol JSON through the native library. **There is
  no Pythonic class/mapping/store API yet**, and PMCD JSON is not compiled directly: `protocol/ModelReader` feeds
  only the composer, and `PureV1Api` refuses non-text contexts.
- **Overlay grammars are opaque** (`parser/OverlayElementSink`, pinned by
  `ArchitectureTest.opaqueCarrierIsLockedToTheOverlaySeam`). A SQL modeling section that should *compile* must
  therefore be built in, or must emit `Protocol` records outside the grammar SPI.

---

## 3. The query front end: port design

```
Postgres SQL text ─┐
#SQL{...}# island ─┼─▶ sql lexer + parser ─▶ syntax AST ─▶ translator ─▶ protocol.spec.ValueSpecification
pgwire Q/P         ┘    (hand-written)       (records,       (scopes,      (Relation API calls)
/api/sql/v1/*                                 spans)          functions)          │
                                                                                  ▼
                                    Compiler.query ─▶ G ─▶ G½ ─▶ H ─▶ I ─▶ J ─▶ K   (unchanged)
```

### 3.1 Parser and AST

- **Hand-written** (lexer plus Pratt/recursive descent), like `GqlParser` and the Pure lexer. Core compiles to
  TeaVM WebAssembly and GraalVM native-image, and an ANTLR runtime, Calcite or JSqlParser is a poor fit for both.
- **Grammar:**
  - the **Postgres** query subset, with Postgres precedence (INTERSECT tighter than UNION);
  - session statements (SET, SHOW, BEGIN/COMMIT/ROLLBACK, DISCARD) as typed nodes for the wire layer;
  - a DDL subset only when §4 needs it;
  - spec: the Postgres manual, not Crate's grammar.
- **AST:** about 60 immutable records with **source spans** and a **quoted-identifier flag**. Errors carry
  positions so the LSP can underline SQL.
- **Placement (D2: a new package, not `com.legend.sql`; settled by the architecture tests in step 1):
  `com.legend.protocol.sql`**, the reader and the translator together. Three rules decided it:
  - the compiler layer never touches `com.legend.parser` (`ParserBoundaryArchTest`, no exceptions), so the Typer
    cannot call a translator in the parser;
  - only the parser and the protocol packages build calls before name resolution
    (`ArchitectureTest.callsAreBuiltOnlyByTheBuilder`, `protocolNodesAreConstructedOnlyByTheParserAndTheNormalizer`);
  - `com.legend.protocol` is the bottom layer and depends only on the JDK, `values` and JSON
    (`ArchitectureTest.protocolIsTheBottomLayer`), so the protocol package cannot depend on a separate `pg` library.

  So the reader (lexer, parser, AST) and the translator sit in the protocol layer, where every other pre-resolution
  front end already is, and depend on nothing but the JDK. The compiler's one seam is `compiler/spec/SqlIslands`.
  The future wire server reads SQL through the same package.
  Later, `docs/LEGEND_SQL_VISION.md`'s "standalone N-in" reader can add a syntax-AST → IR step for raw-SQL
  transpiling (`RawSqlBoundary`'s replacement).
- **Printer:** a true fixpoint, checked as `parse(print(ast)) == ast` over a property corpus and the 83 upstream
  cases.

### 3.2 Translator (SQL AST → Relation `ValueSpecification`)

- **Relation API only. No TDS branch.**
- **Scopes:**
  - each FROM item contributes a column environment;
  - correlated subqueries see outer scopes;
  - CTEs become `let` bindings.

  The environment of a source comes from **the compiler**: the relation type of the source sub-expression,
  computed by the same routine that `compilation/lambdaRelationType` serves. The translator does not keep a
  parallel type system; the final query is typed once by G (invariant 1).
- **Clause mapping:**

  | SQL clause | Relation API |
  |---|---|
  | FROM / JOIN | `join(..., JoinKind, {l,r|...})` |
  | WHERE | `filter` |
  | SELECT expressions | `extend` + `select`/`rename` |
  | GROUP BY / HAVING | `groupBy` + `filter` |
  | window | `extend(over(...))` |
  | ORDER BY | `sort` |
  | LIMIT / OFFSET | `limit` / `drop` / `slice` |
  | DISTINCT | `distinct` |
  | UNION ALL | `concatenate` |
  | UNION | `concatenate->distinct` |
  | EXISTS / IN | `exists` / `in` |

  `intersect` and `except` need new natives (or a desugaring into `exists`, registered).
- **Function table:**
  - **data, not code:** Postgres name → Pure native, plus the argument and type rule that chooses the overload;
  - **spec:** upstream's about 110 processors;
  - **verification:** every entry's Pure side against the registry's signatures, in the "spec by VERIFICATION"
    spirit of the reference-checkout tenet;
  - an unknown function is a positioned compile error.
- **Expected outputs:**
  - Upstream's join column renaming (`col_<alias>`), HAVING matched by expression text and `_1` suffixes are
    artifacts of its typing. The port drops them.
  - The emitted Relation text should read like hand-written Pure. This is the "adoption weapon" of
    `LEGEND_SQL_VISION.md`: SQL in, idiomatic Pure out.

### 3.3 Sources (what a table name means)

Upstream's conventions are kept so that existing queries run: `tb`, `func`, `service`, `csv`, `var`,
`relationalStore`, CTEs, and `a.b` → `a('b')`. The `tb__pkg` mangling is accepted on input. **The proposal
adds** a *catalog-native* naming, so that BI tools browsing `information_schema` see something sensible:

| Postgres | Legend |
|---|---|
| database (startup `database=`) | a project or **data space**: its default mapping and runtime supply execution context |
| schema | a package, spelled verbatim as a quoted identifier (`"my::app"`; `::` is legal inside quotes, so there is no mangling). The session's `search_path` (native) resolves unqualified names, and it defaults to the data space's package |
| table | a zero-parameter Relation function, a Database table, a service, or a **class** (§3.6) |
| table function `f(...)` | a parameterised function, a service with parameters, or a milestoned class |
| foreign keys in `pg_constraint` | **associations**, so BI tools auto-join along the semantic model |

Classes as tables is the feature upstream lacks, and it is where Legend beats a raw warehouse: the SQL user
queries the *logical* model. §3.6 is the proposed projection.

### 3.4 Semantics: Postgres is the spec

**Postgres is the spec (decided 2026-10-11).**
- The translator emits Pure whose results equal Postgres's, whichever backend runs it.
- Where Pure has no native for a Postgres behaviour, the fix is a new native (one SQL lowering rule each), never
  Java computing a value.
- Until that native exists, the gap is a register row.

| Topic | Postgres (the spec) | Upstream | Plan |
|---|---|---|---|
| Identifiers | unquoted ones fold to lowercase; quoted ones are exact; names over 63 bytes are truncated | case preserved, exact match | **Recommended (D1, below): Postgres's rule first, then a case-insensitive match.** A quoted name matches exactly. An unquoted one matches the name equal to its folding when there is one (Postgres's rule), else the ONE name equal to it ignoring case; two such names are an error asking for quotes. Every query Postgres accepts resolves exactly as in Postgres, and `select firstName` finds the model's `firstName` unquoted. Output names keep the case they are written in. A quoted name in the wrong case gets Postgres's hint ("perhaps you meant \"firstName\""). Implemented in step 1 (register S43) |
| Output column names | an alias as written (folded); a column reference keeps its name; a function call is named after the function (`count`); anything else is `?column?`; **duplicates are allowed** | derived from the expression text; duplicates throw | **Postgres rules.** A Relation needs unique column names, so duplicates get internal names while the wire's RowDescription carries the Postgres names. A reference to an ambiguous name raises Postgres's `column reference "x" is ambiguous` |
| `=` with NULL | three-valued | plan flag `LEGACY_SQL_NULL_UNSAFE_EQUALS` | Postgres three-valued logic, explicit in the translation |
| Numeric literals | exact `numeric` | `DoubleLiteral` | exact Decimal, sent as OID `numeric` |
| `'2023-01-01'` against a date column | an implicit cast, with Postgres's many input formats | parsed in Java (`parsePostgresDate`) | **The database executes the cast** (`docs/TENET_CHARTER.md` C2.1/C2.2; register S2), and `now`/`today` become the current-date natives. Non-Postgres backends do not accept every Postgres input format (`'January 8, 1999'`), so the translator accepts the formats all backends agree on (ISO 8601) and refuses the rest with a positioned error. **A register row: the one place where "as close as possible" stops**, because Java does not compute |
| INTERSECT precedence | tighter than UNION | equal | Postgres |
| NULL order | NULLS LAST for ASC, NULLS FIRST for DESC | — | Postgres, spelled out per dialect |
| Integer `/` | truncates toward zero | — | Postgres. DuckDB's `/` returns a double, so the translation must say so |
| Unknown result type over the wire | n/a | falls back to `varchar` | compile error |

### 3.5 HTTP, island, wire

- **HTTP.** Upstream's `/api/sql/v1/execution/{execute,lambda,plan,schema,parseToMetamodel}`,
  `/sql/v1/schema/getSchema` and `/sql/v1/grammar/*`, with upstream's shapes, following the ruling "lite serves
  upstream's APIs and nothing of its own" (`docs/UPSTREAM_ENDPOINTS_DESIGN_2026_09_27.md`). `lambda` returning the
  generated Pure is the cheapest demo of the whole thing.
- **Island.** The `Typer` arm for `SqlIsland` calls the translator, so `#SQL{}#` composes with Pure.
- **pgwire: hand-written, no dependencies (D3, decided).**
  - **Transport:** plain `java.nio` sockets with one virtual thread per connection. The repository's Java 21
    language level already allows this, and the HTTP server likewise uses only the JDK's `com.sun.net.httpserver`.
  - **TLS:** the JDK's `SSLEngine`. **SCRAM-SHA-256:** `javax.crypto` (HMAC-SHA-256, PBKDF2).
  - **Spec:** protocol 3.0, written from the Postgres manual's "Frontend/Backend Protocol" chapter, not from
    Crate's code.
  - **Scope:** one module serves the Legend handler (translate). When the server program resumes, it also serves
    the warehouse's W3 (DuckDB passthrough).

  Requirements:
  - real BackendKeyData and cancel;
  - transaction status;
  - TLS and SCRAM;
  - honoured result format codes;
  - real SQLSTATEs and no stack traces;
  - a bounded executor;
  - `numeric` for Decimal.

  The **catalog** is a set of `pg_catalog`/`information_schema` views generated from the `ModelContext` and
  **executed by DuckDB**, never a Docker Postgres. The model's metadata is seeded as rows and the catalog is a set
  of views over them. That is the shape of `docs/WORLD_MAP.md` §3's one narrow exception (queries over the
  platform's own system tables), and it keeps "the DATABASE executes". SET and SHOW keep real session state; a parameter
  that is not honoured is an error or a named register row, not a no-op.

  Probe suites must cover psql `\d`, pgjdbc metadata, DBeaver, Tableau, Power BI/ODBC and psycopg. Upstream tests
  only pgjdbc.
- **libpg_query, Postgres's own parser as a C library: a test oracle, not a runtime dependency.**

  **What it is.** pganalyze extracts the real Postgres grammar (`gram.y`, `scan.l`) into a library. It is
  BSD-3-Clause, tracks Postgres 18, and gives JSON parse trees with source locations, a scanner and
  fingerprinting. It builds on Windows.

  **Why not at runtime:**
  - Core runs on the JVM, in the browser through TeaVM, and as a GraalVM native image. A C library reaches only
    the JVM, through JNI or FFM, and needs a build per platform.
  - Its output is Postgres's raw parse tree, hundreds of node types, which would still need translating into our
    AST. It would save the grammar, the easy part, and not the translator.
  - The DuckDB C library fits the warehouse because the warehouse *is* DuckDB. Core is not Postgres.

  **Why as a test oracle**, in a test-only Bazel target like the reference lane (`tools/reference`):
  1. **Acceptance:** every corpus statement is accepted or rejected by both parsers alike.
  2. **Printer fidelity:** `pgquery(print(parse(sql))) == pgquery(sql)` with locations stripped. Postgres's own
     parser certifies that our printer prints the same query.
  3. **Fuzzing.**

  The *semantic* oracle is a real Postgres, run once to generate the parity fixtures (§1.5).

### 3.6 Classes as tables: the relational projection of the model

**The rule:** a table per class the data space can query, and *identity* decides how references between classes
appear. This is the standard object-to-relational projection (an ORM's entities, embeddables and element
collections). It is also exactly what dbt's `semantic_models` need, entities with primary and foreign keys, so
the same rule exports a Legend model as dbt semantic models and imports them back (§5).

1. **Identity comes from the model.** It is the class's `<<equality.Key>>` properties, which legend-lite already
   resolves (D91; `compiler/element/ClassCompiler.java:59`, `lowering/InstanceEquality.java`).
   - A class with keys is an **entity**; a class without keys is a **value**.
2. **Tables.** Each class with a root class mapping in the data space's mapping is one table: schema = its
   package, name = its class name. Postgres allows tables without a primary key, so a mapped value gets a keyless
   table.
3. **Columns:**
   - single-valued primitive and enum properties, inherited ones included;
   - multi-valued primitive properties as Postgres arrays (`text[]`, `int8[]`);
   - qualified properties without parameters as computed columns;
   - the key properties declared `PRIMARY KEY` in `pg_constraint`;
   - enumerations as Postgres `ENUM` types in `pg_type`.
4. **A to-one property to an entity** becomes a foreign-key column per key property of the target, named by its
   path (`"firm.id"`) and declared in `pg_constraint`. So `JOIN "Firm" f ON f.id = p."firm.id"` works, and BI
   tools draw the join themselves.
   - The column's values are computed through Legend's own property/association mapping. The key join is
     therefore correct even when the mapping's join is not key equality.
5. **A to-one property to a value** has the value's columns flattened into the owner as path columns
   (`"address.city"`), recursively.
   - A cycle among values cannot be flattened. It is a compile error naming the cycle, never a silent cut.
6. **A to-many property:**
   - **To an entity whose inverse end is to-one:** no column. The other table carries the FK (the ordinary
     one-to-many).
   - **A many-to-many association between entities:** a bridge table named after the association
     (`"Person_Firm"`), with both FKs.
   - **To values** (`addresses: Address[*]`): a child table `"Person.addresses"` with the owner's key and the
     value's flattened columns.
   - **From a keyless owner:** nothing can link the rows, so it is not exposed (a register row).
7. **Inheritance.** Every mapped class is its own table, with its inherited columns. A superclass table also
   returns its subclasses' rows, as Postgres table inheritance does without `ONLY` and as Legend's `getAll()`
   does.
8. **Milestoned classes** become table functions taking their temporal parameters:
   `"Person"(business_date => DATE '2024-01-01')`. Named arguments are native Postgres syntax.

**Why not embed everything:**
- Cycles (Person → Firm → employees → Person).
- Duplicated data, and very wide tables.
- BI tools model star schemas through keys.

**Why not foreign keys everywhere:**
- Values have no key.
- A synthesised surrogate key (a row number, a hash) is not stable across queries, so a join between two
  queries' results would silently mismatch.

**Alternative for values: Postgres composite types.** That means `CREATE TYPE "Address" AS (...)`, a column
`address "Address"` read as `(address).city`, and arrays of composites for to-many.
- **For:** structurally more "native Postgres".
- **Against:** most BI tools and ODBC drivers cannot read composite or array columns.
- **Proposal:** flattened path columns by default, and the composite form later as a per-connection option.

**Open sub-decisions:**
- **Identity source:** `<<equality.Key>>` only (proposed: one source of truth, in the model), or also a mapping's
  primary key when a class declares none. "Make a class" can propose keys from a table's primary key, so models
  get keys without a second rule.
- **Qualified properties with parameters:** table functions, or not exposed.

---

## 4. SQL as a modeling surface (brainstorm)

Postgres DDL already names most Legend relational concepts:

| Postgres DDL | Legend element | legend-lite today |
|---|---|---|
| `CREATE SCHEMA s` | package (and Database schema) | yes |
| `CREATE TABLE t (... NOT NULL, PRIMARY KEY, REFERENCES)` | `Database` table + `Join` per FK | yes (`DatabaseDefinition`; `CatalogModel` already builds Databases from catalog rows) |
| `CREATE TYPE e AS ENUM (...)` | `Enumeration` | yes |
| `CREATE DOMAIN d AS text CHECK (...)` | primitive extension with constraints | parse-only (`PrimitiveExtensionDefinition`) |
| `CREATE VIEW v AS SELECT ...` | `function v(): Relation<(...)>[1] { <translated> }` | yes, given §3 |
| `CREATE FUNCTION f(p int) RETURNS TABLE(...) LANGUAGE sql AS $$...$$` | parameterised Relation function (and a service) | yes, given §3 |
| `CREATE MATERIALIZED VIEW` | function + materialization (`Persistence`) | parse-only / F1 |
| `COMMENT ON ...` | `doc` tagged values | yes |
| `GRANT` | entitlements | out of scope (warehouse grants) |

**Where SQL runs out: the object layer.** Postgres can say none of the following without abuse:
- class inheritance (`INHERITS` is not it);
- to-many properties (arrays of composites are not it);
- qualified properties with parameters;
- cross-object constraints;
- M2M mappings; union, merge or operation mappings;
- milestoned navigation;
- graph fetch.

A SQL surface for classes needs a side channel, and dbt's YAML is the one people already write (§5).

**How it enters the compiler.** A reader turns DDL into `Protocol` records → `FromProtocol` → `ParsedModel`. In
practice that is `.sql` files in a project, or a built-in `###SQL` section (an overlay grammar would be opaque).

**Round trip.** Every model prints as Pure through the existing emitter. A SQL-authored model can therefore be
"ejected" to Pure, and a Pure model's stores and relation functions can be printed back as DDL. That makes SQL a
*view* of the one model, not a second model.

**Python is the same shape.** A Pythonic API (`Class`, `Database`, `relate`) builds the same records, through PMCD
JSON and the native library. All three non-Pure surfaces share one prerequisite: compiling `Protocol` records
that did not come from Pure text. In Java this is already possible (`Compiler.buildModel`). Over JSON it is not
(`ModelReader` → `FromProtocol` is not wired into compile).

---

## 5. A dbt-compatible front end (brainstorm)

### 5.1 dbt ↔ Legend

| dbt | Legend | legend-lite today |
|---|---|---|
| `source()` (`sources.yml`: database/schema/tables/columns) | `Database` tables | yes |
| model `.sql` (a SELECT) | Relation function, body translated by §3 | needs §3 |
| `ref('m')` | a call to function `m()` | yes (G½ inlines it) |
| `materialized: view` / `ephemeral` | the function itself, inlined (Legend's default) | yes |
| `materialized: table` | `write` → CTAS | **missing** (F1) |
| `materialized: incremental` (append, merge, delete+insert; `unique_key`) | `Persistence`: AppendOnly / NontemporalDelta | parse-only |
| `snapshot` (SCD2, `dbt_valid_from/to`) | Persistence UnitemporalDelta + **processing milestoning** | parse-only / milestoning on tables yes |
| `columns:` + `data_type` + `contract: {enforced: true}` | the function's declared `Relation<(...)>` return type | yes, and **always** enforced |
| `constraints` (not_null, unique, PK, FK, check) | multiplicity `[1]`, Database PK/Join, class constraints | partial |
| data tests (unique, not_null, accepted_values, relationships, singular) | constraints/assertions compiled to SQL (Track C5) | planned |
| unit tests (given/expect) | function tests with test data | deferred (`docs/DEFERRED_TEST_EXECUTION.md`) |
| seeds (CSV) | `Data` elements / `csv()` | parse-level |
| `semantic_models` (entities, dimensions, measures) | **Class + Association + `~func` mapping** ("Make a class") | mapping yes; promotion planned |
| metrics | qualified properties / DataCube measures | partial |
| exposures | services, data spaces, DataCube pages | yes |
| macros (Jinja) | Pure functions, typed rather than text-templated | conceptual mismatch (§5.3) |
| packages (`dbt_utils`) | Pure libraries | per-macro mapping |
| Mesh cross-project `ref`, groups, `access` | project dependencies (depot), package visibility | partial |
| `profiles.yml` targets | `Runtime` + `Connection` | yes |
| `vars` | function/service parameters | yes |

The fit is close because the two tools model the same thing: dbt *adds* a semantic layer after the SQL, while
Legend *starts* from one (`docs/BI_AND_ETL_PLAN_2026_09_29.md:25`, "for us the model is the semantic layer").

### 5.2 Compatibility levels (each one is useful alone)

1. **L1, read: ingest `manifest.json`.**
   - Real dbt (`dbt compile`, v1 or v2: both write manifest **v12**) does the Jinja and dependency work.
   - legend-lite reads `nodes[*].compiled_code`, `depends_on`, `columns`, `contract`, `semantic_models`, and
     produces a Legend model.
   - **Gives:** typed compilation of every model, column lineage, a semantic layer, DataCube on top, and
     execution on any legend-lite backend.
   - **Needs:** no Jinja and no YAML parser in legend-lite.
   - **Limit:** compiled SQL is in the project adapter's dialect, so this means dbt-postgres projects (and
     DuckDB-dialect ones as a near neighbour).
2. **L2, author: read dbt project files natively.** `dbt_project.yml`, `models/**/*.sql`, `*.yml`.
   - **A Jinja *subset*:** `ref`, `source`, `config`, `var`, `this`, `is_incremental()`, plus a mapped list of
     common `dbt_utils` macros.
   - **Anything else** is a positioned compile error (invariant 4).
   - **Mixing:** projects may hold `.pure` and `.sql` side by side, and the LSP underlines SQL.
   - **Needs:** a YAML reader.
3. **L3, run: materializations.**
   - A `build` walks the DAG and emits CTAS / INSERT…SELECT / MERGE per dialect, and runs tests as SQL assertion
     queries.
   - **Needs:** F1 writes, plus `Persistence` given meaning.
4. **L4, be a dbt target.** Real dbt with dbt-postgres runs against legend-lite's pgwire:
   - `CREATE VIEW` registers a function in a workspace;
   - `CREATE TABLE AS` materializes it;
   - dbt-postgres's `pg_namespace`/`information_schema` introspection and rename-swap dance must also work.

   This is the most drop-in level and has the longest compatibility tail. Defer it.

The reverse direction is cheap once §4's printer exists: **emit a dbt project from a Legend model** (stores →
sources, relation functions → models, classes → semantic models). It is an adoption path for Legend users who
deploy with dbt.

### 5.3 The honest frictions

- **Jinja is text templating; Legend is typed functions.** Macros that generate SQL text can only be supported by
  *running* them (L1, through dbt itself) or by mapping known ones. Arbitrary macros are out at L2. Most model
  files use only `ref`/`source`/`config`, so the subset covers the common case. Measure it on real projects
  before promising anything.
- **Hooks** (`pre_hook`/`post_hook`), `run-operation` and `grants` execute arbitrary SQL. Refuse them, or pass
  them through as named residue, never silently.
- **Dialect lock.** dbt models are written in one warehouse's SQL, so "Postgres dialect" must be the *declared*
  input. The win is that legend-lite then renders it for DuckDB, Postgres, Snowflake or H2: write once, run on
  several backends, which dbt cannot do.
- **dbt's semantics are the warehouse's.** A dbt user expects Postgres results. §3.4's rows apply in full, and
  the parity corpus is the evidence.

---

## 6. One model, many surfaces

```
 Pure text    ─(parser)──────────┐
 Python       ─(PMCD JSON)───────┤
 SQL DDL      ─(sql reader)──────┼─▶ Protocol records ─▶ FromProtocol ─▶ ParsedModel ─▶ D–F (the model)
 dbt project  ─(manifest/loader)─┘

 Pure query   ─(SpecParser)──────┐
 Postgres SQL ─(translator)──────┼─▶ protocol.spec.ValueSpecification ─▶ Compiler.query ─▶ G … K
 #SQL{}#      ─(translator)──────┤
 DataCube / pure-protocol JSON ──┘
```

**Invariant for every surface:** it produces the same records the Pure parser produces, and **prints back as
Pure**. Pure text stays the canonical, reviewable form. No surface carries semantics the others cannot express;
anything SQL or dbt cannot say is written in Pure, or in Python once that exists.

---

## 7. A phased plan (if the user starts it)

Sizes are rough Java line counts, for comparison with upstream (§1.1).

| Phase | Content | Gate | Size |
|---|---|---|---|
| **0. Decide** | §8's open decisions; register rows (§3.4); an `IN_FLIGHT` slot (the DataCube line owns "Make a class"; the database-owner line owns `sql/dialect`/`lowering`) | — | doc |
| **1. SQL over HTTP and islands** | parser + AST + printer; translator for `tb`/`func`/`csv`/CTE sources; function table; `/api/sql/v1/execution/*` + `getSchema`; the `#SQL{}#` Typer arm | 83 round-trip cases; the **libpg_query lane** (acceptance and printer fidelity); the transpile pairs (Relation half); **parity corpus ratchet** (DuckDB execution against committed Postgres oracle answers) | 8–11k |
| **2. Wire** | the hand-written pgwire module (later shared with the warehouse's W3); a DuckDB-executed catalog generated from the model; session statements | probe suites per client (psql, pgjdbc, DBeaver, Tableau, ODBC, psycopg) | 6–9k |
| **3. Legend sources** | `service`, data space as database, classes as tables (§3.6), associations as FKs, bind parameters | parity corpus re-run through classes | 2–4k |
| **4. SQL/dbt modeling, read** | DDL reader → `Protocol`; dbt **L1** (manifest ingest); YAML contracts → declared return types; `semantic_models` → "Make a class" (shared with DataCube) | sample dbt projects compile; Pure print-back round trip | 3–5k |
| **5. dbt author and run** | **L2** (YAML reader, Jinja subset, LSP); **L3** (needs BI-plan F1 writes, `Persistence`, tests as assertions) | `jaffle_shop`-class projects build on DuckDB and Postgres | 5–8k + F1 |
| **6. Optional** | **L4** (dbt-postgres against pgwire); Legend → dbt export | dbt-postgres's own integration tests | — |

**First slice:** phase 1 alone. It ships Postgres SQL against Legend models over HTTP and inside Pure, and it
proves the translator against the corpus's oracle cases (1,703 at the pin) before any wire or dbt work starts.

---

## 8. Decisions

**Decided by the user, 2026-10-11:**

- **D1. Identifiers.** The user's first answer was native Postgres. Asked for an unbiased view, I recommend
  Postgres's rule first, then a unique case-insensitive match (§3.4, register S43):
  - **Native Postgres:** zero deviation, but every camelCase Legend name needs quotes (`"firstName"`), and an alias
    `AS totalAmount` comes out as `totalamount`, which a Pure caller then cannot read as `$x.totalAmount`.
  - **Exact, case-sensitive (upstream):** rejects queries Postgres accepts (`select NAME from t` with a column
    `name`).
  - **Fully case-insensitive (DuckDB's rule):** forgiving, but a model with `name` and `Name` makes valid Postgres
    ambiguous.
  - **snake_case aliases:** two names for every element.
  - **The recommendation:** keeps every valid Postgres query's meaning, and adds only what Postgres would refuse.
    Its one visible difference is that output names keep their written case.

  Implemented in step 1, pending the user's ruling.
- **D2. A new package** (§3.1): it is `com.legend.protocol.sql`, which the architecture tests require.
- **D3. A hand-written wire protocol with no dependencies.** libpg_query only as a test oracle (§3.5).
- **Overall: as close as possible to native Postgres in everything** (§0 point 4, §3.4).

**Open:**

- **D4. Whether to start now.** Several sessions work in this repository at once. `docs/IN_FLIGHT.md` lists the
  active lines in priority order: the build rebuild, Studio, the database owner, DataCube with Python. Each line
  announces the files it will touch so that sessions do not collide. The question is whether this becomes a new
  line now, beside the others, or waits.
  - Phase 1 is mostly new files under `com.legend.protocol.sql`. Its touch points elsewhere are small:
    - the `Typer`'s `SqlIsland` arm, in `compiler/spec` (the Bazel line's hold on it ended when its reference-lane
      fix landed);
    - route registration in `server/LegendHttpServer`;
    - new natives (`intersect`, `except`) in `builtin/`.
  - **Proposal:** a new line, announced in `IN_FLIGHT`, starting with phase 1.
- **D5. dbt target level:** L1+L2 (proposed), or the L4 drop-in?
- **D6. Classes as tables:** §3.6 is the proposal, and its two sub-decisions (identity source; qualified
  properties with parameters) are open.

---

## Upstream facts found while building

What Postgres 16 and legend-engine answered while the island was built (§9, cut), kept here because §1–§8 do not say
it. Each was measured on both lanes against Postgres, or read in Postgres's source or its regression suite
(`REL_16_STABLE` at `8e928803020e`); lite's deliberate differences are register rows (S46 to S49 among them).

- **Three-valued predicates.** A NULL boolean `IS TRUE` is false. `ANY (subquery)` over no rows is FALSE and `ALL` is
  TRUE. `NOT IN` over a set holding a NULL is never TRUE. `x BETWEEN lo AND hi` is `x >= lo AND x <= hi`, so a NULL
  bound can still give FALSE. `x IN (a, b…)` is the OR of `x = e`: TRUE where found, else NULL if any element is NULL.
- **Typing NULL and untyped constants.** A NULL operand takes the other operand's type, and a NULL argument the
  function's preferred type; `NULL + NULL` and `-NULL` raise "operator is not unique". In CASE, COALESCE, GREATEST and
  LEAST an untyped constant takes the others' type, numbers widen, and integer with text raises "COALESCE types integer
  and text cannot be matched"; a CASE whose results are all NULL is text. `VALUES` is a UNION ALL of one-row SELECTs
  naming `column1..N`, typed together, where a written UNION ALL is typed a pair at a time; under
  `select_common_type` only a bare NULL in a plain SELECT branch takes the other side's type (any other NULL is
  already text), and an integer or numeric beside a double becomes a double. (108 statements probed.)
- **Rounding and exact numbers.** Numeric rounds half away from zero (`round(x)`, `round(x, n)`, numeric-to-integer
  casts); float8 rounds half to even (`round(float8)` is rint, so `round('2.5'::float8)` is 2, and so do float8 casts
  to integer); a timestamp's fraction past microseconds rounds half to even. Numeric division is exact
  (`floor(0.3/0.1)` is 3; a double gives 2.9999999999999996). Pure's `round` is half to even by its spec.
- **Integer widths.** `sum(int4)` is bigint and `sum(int8)` numeric, so `sum(b)/2` is 3.5 for a bigint column.
  Smallint arithmetic stays smallint and raises past 32767; a constant is int4 where it fits; int2 with int2 is int2,
  otherwise the wider type. An integer cast raises only on the row that overflows.
- **Windows.** The default frame is RANGE up to the current row's last peer. `lag`/`lead`'s default applies only when
  the offset row is outside the partition, never when its value is NULL. In a RANGE frame with offsets a NULL-keyed row
  sits in its NULL peers' frame. `agg(x) FILTER (WHERE c) OVER` is the aggregate of `CASE WHEN c THEN x END`.
  DISTINCT or ORDER BY inside a window call, FILTER on a ranking function and two WINDOW definitions of one name are
  errors.
- **Functions** (ported from `varlena.c`, `oracle_compat.c`, `float.c`, `numeric.c`, `datetime.c`, and measured).
  `concat` writes a boolean `t`/`f` and skips NULL; `||` writes `true`/`false`. `strpos` is 1-based, 0 when absent.
  The trim functions strip only U+0020. `ceil`, `floor`, `sign` and `round(x)` keep numeric for numeric input, else
  double; `extract` returns numeric and `date_part` double. `log(1,8)` raises and `cot(0)` is Infinity. Boolean has no
  `min` or `max`. `mode()` takes the first value on a tie; `percentile_disc` picks `ceil(p*N)` computed in float8. Pure's
  `indexOf` is 0-based and its `splitPart` drops empty fields.
- **jsonb.** `->` with an integer counts from 0, negative from the end, NULL past either end; `->>` of a jsonb null is
  SQL NULL. `#>`, `#>>` and subscripts follow `jsonb_get_element`: a text element names a member, or an array element
  if it spells an integer by strtol's rules. jsonb keeps the last duplicate key and numeric's spelling (`1.50`).
  Element and key functions on a scalar raise "cannot extract elements from a scalar".
- **Dates.** Under the default DateStyle (ISO, MDY) Postgres reads month names, m/d/y, two-digit years (`69` is 2069,
  `70` is 1970), `19990108`, `J2451187`, AM/PM, `epoch`, `24:00` and second 60, and ignores a numeric zone for date and
  timestamp. Date ± integer is a date; date − date is a day count; date + interval is a timestamp, adding months
  (clamped to the month's end), then days, then time. (10,000 generated spellings read both ways.)
- **Output names** follow `FigureColname`: a cast's type or `case` names an item only when what it holds has no name of
  its own (`'1'::float8::int2` is `int2`, `CASE … ELSE b END` is `b`, `(SELECT max(x) …)` is `max`), with the
  grammar's type names (`int4`, `float8`, `bpchar`).
- **Upstream's parity judge** (legend-engine 4.145.0's `parity-tests`) seeds its tables as its `SchemaManager` does
  (with the Postgres setup `pgcrypto` and `test_seq`), sorts both results unless the SQL says ORDER BY, compares with
  `ResultComparator` (names without regard to case, floats within a tolerance), and records REGRESSION, FIX DETECTED
  and STATUS MISMATCH. Postgres's float functions come from the platform's C library, so macOS and Windows differ from
  Linux by up to 1e-12 of the larger value.

> Cut in the 2026-11 docs cleanup: §9, the build records (step 1 and steps 2 to 5, semi-structured, the basics, the long tail, 2026-10-11 to 2026-10-13, and the PostgreSQL regression-suite corpus program of 2026-10-17); its divergences are register rows, its cases are pinned in the island's parity corpus, and the facts it measured are listed above. Full text: `git show de2745351:docs/LEGEND_SQL_STUDY_2026_10_11.md`.

## 10. Sources

- **Upstream code:** `finos/legend-engine` `e60ab67a`, sparse checkout of `legend-engine-xts-sql/`,
  `legend-engine-xts-relationalStore/.../legend-engine-xt-relationalStore-postgresSql/`, and `docs/`.
  `finos/legend-pure` was searched for SQL front-end code and has none.
- **dbt v2** (Rust engine, static analysis, adapters, manifest v12):
  https://docs.getdbt.com/docs/dbt-versions/core-upgrade/upgrading-to-v2. Read 2026-10-11; check it again
  before relying on the adapter list.
- **dbt Fusion components and licenses** (2025-05-28; Level 2 SQL comprehension is proprietary; the authoring
  layer and artifact schemas are Apache 2.0): https://docs.getdbt.com/blog/dbt-fusion-engine-components
- **Manifest schemas:** https://schemas.getdbt.com/
