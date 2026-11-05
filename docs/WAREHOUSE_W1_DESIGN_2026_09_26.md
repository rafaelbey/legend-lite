# Warehouse W1: the core and the HTTP SQL API — design (2026-09-26)

**Built** (W1a–W1f, 2026-09-26 to 2026-10-05; §5), and since joined by W2's authorizer and catalog grants and by
Direct mode's CORS (`--allow-origin`, §7). The warehouse is the DuckDB server legend-lite's clients speak to over
HTTP: DataCube in the browser, the JDBC driver, the corpus harness. It began as leg W1 of the server program
(`docs/SERVER_PROGRAM_2026_09_26.md`, "the program" below; folded in the 2026-11 docs cleanup, git history:
docs/HISTORY.md), built on the measurements of `docs/WAREHOUSE_W0_HOMEWORK_2026_09_26.md` (removed; docs/HISTORY.md).
W1 ran every statement as its signed-in user and set that user's identity before every statement, so W2 added
checks to a path that already carried identity. The 2026-11 cleanup folded in the identity rule of
`docs/WAREHOUSE_FFM_HOMEWORK_2026_09_26.md` (§6) and the Direct-mode design of
`docs/WAREHOUSE_D1_DESIGN_2026_09_26.md` (§7). Postgres catalogs (`--postgres NAME=DSN`, `postgres_query`) are in
`docs/backends/postgres.md`; DuckDB's own measured traps in `docs/backends/duckdb.md`.

## 1. Where it lives

A new Bazel package **`//warehouse`** (Java package `com.legend.warehouse`;
"warehouse" is a working name):

| Target | What | Depends on |
|---|---|---|
| `:sqlapi` | the HTTP SQL API as Java records, their JSON codec, the **binding** interface (a sans-I/O state machine, program §2c), and the native binding | **java.base only**. It must compile into the WebAssembly module beside the planner (ruling 7), and a guardrail pins that |
| `:client` | the JVM driver for a binding (`java.net.http`), and a **`java.sql.Driver`** over it (`jdbc:warehouse:http://host:port/catalog`) | `:sqlapi`, java.net.http, java.sql |
| `:server_lib` / `:server` | the warehouse process: DuckDB's **C API through `java.lang.foreign`** (W1d), no JDBC class | `:sqlapi`, jdk.httpserver; DuckDB **1.5.5.1**'s native library (its own pinned jar, apart from core's DuckDB, which runs 1.5.6.0 as of 2026-11): `:duckdb_library`, out of the JDBC jar at build time, passed with `--duckdb-library` or found in the server's runfiles |
| `:tests` | conformance, cancel/timeout, concurrency, identity | all of the above |

Rules carried from core: no reflection (ArchUnit), NullAway, the
Windows portability guardrail. **DuckDB's library** is passed with
`--duckdb-library` (a native image ships it beside the binary), or found in
the server's runfiles when Bazel started it (`//warehouse:duckdb_library`).
Since 2026-10-04 (Bazel workplan P1-16) it is never extracted from the JDBC
jar into a temporary directory: a JVM started without it fails, naming the
flag.

## 2. The process

- **HTTP:** the JDK's built-in server (no dependencies; it compiles in a
  native image, W0 Q1), with virtual threads for requests.
- **Catalogs:** W1 uses one DuckDB database file per catalog, owned by
  this process. The interface is `Catalogs`, so DuckLake (W0 Q5) slots in
  when on-demand readers are built, without changing the API.
- **A statement's life:**
  1. auth;
  2. a fresh DuckDB connection that belongs to the principal:
     `system.main.authenticated_user()` answers with it, from outside SQL
     (W1d; program §3 0b; §6 below);
  3. `current_user` / `session_user` installed as temp macros over it;
  4. the statement, run on the **executor**;
  5. results into chunks;
  6. the connection closed.
- **Executor:** a concurrency limit (default: 1, then tuned; W0 Q4 shows
  one query already uses every core), a FIFO queue with a length cap, a
  per-statement timeout, and cancel through `duckdb_interrupt`.
- **Results:** held per statement until fetched or expired: in memory up
  to a size cap, then spilled to a temp directory. **JSON** chunks of a
  fixed row count; **Arrow** chunks of whole 2,048-row batches (W1d).
- **Query history:** each statement's id, user, SQL text, state, timings,
  rows and error, in a `system.query_history` table, which users will
  query through grants in W2.
- **Tokens (W1):** users with passwords in the server's config.
  `POST /sql/v1/login` returns a short-lived HMAC-signed token. OIDC
  verification against an identity provider replaces the password
  store in W2. The principal is read **only** from a verified token.

## 3. The API (JSON)

Every call carries `Authorization: Bearer <token>`, except `login`. All
bodies are JSON (`application/json`), except Arrow chunks.

```
POST /sql/v1/login            {"user": "u", "password": "p"}
  → 200 {"token": "…", "expiresAt": "2026-09-26T12:00:00Z", "principal": "u"}

POST /sql/v1/statements
  {"sql": "SELECT …", "catalog": "main", "parameters": [ {"type": "INTEGER", "value": 3} ],
   "timeoutMs": 60000, "waitMs": 2000, "resultFormat": "json", "rowsPerChunk": 10000}
  → 200 when done within waitMs: {"statementId": "…", "state": "succeeded", "result": {…meta…}, "firstChunk": {…}}
  → 202 otherwise:                {"statementId": "…", "state": "queued" | "running"}

GET  /sql/v1/statements/{id}
  → {"statementId", "state", "submittedAt", "startedAt"?, "finishedAt"?,
     "result"?: {"columns": [{"name", "type", "nullable"}], "rowCount", "chunkCount"},
     "error"?: {"code", "message", "position"?}}

GET  /sql/v1/statements/{id}/chunks/{n}
  → json:  {"index": n, "rows": [[…], …]}
  → arrow: application/vnd.apache.arrow.stream: a whole stream per chunk (W1d)

POST /sql/v1/statements/{id}/cancel   → {"statementId", "state": "cancelled"}

DELETE /sql/v1/statements/{id}        → {"statementId", "closed": true}: the result freed now

POST   /sql/v1/sessions       {"catalog": "main"}
  → {"sessionId", "catalog", "engine": "DuckDB", "engineVersion": "v1.5.5"}
DELETE /sql/v1/sessions/{id}

GET  /sql/v1/history?limit=100         → the caller's own statements, newest first:
     [{"statementId", "catalog", "sql", "state", "submittedAt", "finishedAt"?, "rowCount"?, "errorCode"?}]

GET  /sql/v1/catalogs                  → [{"name"}]
GET  /sql/v1/catalogs/{c}/objects      → [{"schema", "name", "kind": "table"|"view", "columns": [{"name", "type"}]}]
```

- **Sessions:** a statement with a `sessionId` runs on that session's own
  connection, after the session's earlier statements, so `USE`, `SET`, temp
  tables and transactions carry over. A session reports the engine behind it
  (name and version as that engine's driver gives them): the SQL it accepts
  is that engine's.
- **`describeOnly: true`:** the statement is prepared, never run, and the
  result carries its columns and no rows (a compiler asking what a query
  returns; Snowflake's query API has the same flag).
- **States:** `queued`, `running`, `succeeded`, `failed`, `cancelled`.
- **Error codes:** a closed set:
  - `AUTH_REQUIRED`, `AUTH_INVALID`, `FORBIDDEN` (W2);
  - `SQL_PARSE`, `SQL_BIND`, `SQL_EXECUTE`;
  - `TIMEOUT`, `CANCELLED`, `QUEUE_FULL`, `NOT_FOUND`, `TOO_LARGE`.
- **Types:** a closed list, one table mapping DuckDB type → API type →
  Arrow type → Pure primitive:
  - `BOOLEAN`, `TINYINT` … `BIGINT`, `HUGEINT`, `FLOAT`, `DOUBLE`,
    `DECIMAL(p,s)`;
  - `VARCHAR`, `BLOB`, `DATE`, `TIME`, `TIMESTAMP`,
    `TIMESTAMP WITH TIME ZONE`, `UUID`, `INTERVAL`;
  - `LIST`/`STRUCT`/`MAP`, as JSON.

  A type outside the list is an error, never a guess.
- **JSON values:** numbers as JSON numbers, except BIGINT/HUGEINT/DECIMAL,
  which are **strings** so no precision is lost in a JavaScript client.
  Dates and times use ISO-8601 strings; TIMESTAMP WITH TIME ZONE is the
  UTC instant (`…Z`), which a client shows in its own zone, as DuckDB's
  JDBC driver does (measured: its offset is the client JVM's). NaN, the infinities and **-0.0**
  travel as their names (JSON cannot hold them, and loses -0.0's sign).
  Blobs are base64; JSON is its text.
- **Column types are DuckDB's own type names**, passed through unchanged
  (`INTEGER[]`, `STRUCT(x INTEGER, y DECIMAL(2,1)[])`, `MAP(VARCHAR, INTEGER)`),
  read into a tree by `DuckType` at both ends.
- **Nested values (W1b):** a list is an array, a struct an object, a map an
  array of `[key, value]` pairs. A top-level nested cell is
  `{"value": …, "text": "…"}`, where `text` is DuckDB's own text for it
  (what its driver's `getString` gives), so no client re-derives DuckDB's
  quoting rules.

## 4. The binding (program §2c), in `:sqlapi`

```java
interface SqlApiBinding {
  HttpCall login(Credentials c);
  HttpCall submit(StatementRequest r, Token t);
  Step next(HttpCall sent, HttpResult got);   // Poll | Fetch | Done | Failed
  ResultChunk decode(HttpResult chunk);
  HttpCall cancel(String statementId, Token t);
}
```

`NativeBinding` speaks §3 directly. A driver loop (`java.net.http` in
`:client`; `fetch` from the WebAssembly module in D1) performs the
calls. Vendor bindings (V*) implement the same interface.


## 5. As built: the legs of W1, each proven before the next

> Cut in the 2026-11 docs cleanup: the leg plan and the per-leg build records W1a–W1f, "Results: freed when done"
> and the Linux arm64 note (2026-09-26 to 2026-10-05). Full text: `git show de2745351:docs/WAREHOUSE_W1_DESIGN_2026_09_26.md`. What each leg
> established, kept:

- **W1a, the API and the server** (2026-09-26): login, statements, poll, JSON chunks, cancel, executor, history,
  identity per statement. Found on the way: DuckDB's JDBC 1.5.5.1 loses binder and catalog errors on
  `Statement.execute` (1.4.4 and `PreparedStatement` report them), so the server prepared every statement, then ran
  it; a script travels whole (DuckDB's prepare runs a script's leading statements and prepares the last); with a
  concurrency of 1 and a queue of 1, the third slow statement is refused with `QUEUE_FULL` (HTTP 503).
- **W1b, the JDBC driver** `jdbc:warehouse:http://host:port/catalog`: its values are the same Java objects DuckDB's
  own driver returns, for every type (`LocalDate`, `java.sql.Timestamp`, `OffsetDateTime`, `BigInteger`,
  `BigDecimal` with its scale, `UUID`, `java.sql.Array`/`Struct`/`LinkedHashMap` for nested values). The proof is a
  differential, `WarehouseJdbcTest`: the same SQL through this driver over HTTP and through DuckDB's driver
  in-process, compared cell by cell. Named differences: a BLOB compares by bytes and JSON by text. A connection is
  a session, so autocommit off, commit and rollback work as on DuckDB.
- **W1c, the corpus through the warehouse:** `//spec:corpus_warehouse` (manual) runs the DuckDB corpus lane with
  every connection a session on a warehouse child process. Same verdicts as in process (host judge 2,472 pass /
  107 fail, database judge 2,474 / 107), ~52 s a pass against ~35 s, the cost of one HTTP round trip and JSON per
  statement. It needed two API facts (a session reports its engine, from which core picks its dialect;
  `describeOnly`) and one core fix: a JSON column is decoded when the *plan* types it JSON, not when the driver's
  object has DuckDB's class name.
- **W1d, the server on DuckDB's C API** (through Java's FFM, no JDBC class, no reflection, no new dependency;
  nanoarrow rejected by measurement: an ENUM, even nested, invalidates the database for every connection). Results
  come chunk by chunk through `duckdb_data_chunk_to_arrow`, one C call per 2,048 rows; scripts are split by
  DuckDB's own parser (`duckdb_extract_statements`); type names are spelled from the logical type (the C API has
  none); a nested cell's text is DuckDB's own cast (`duckdb_get_varchar`). The Arrow contract: each chunk a whole
  IPC stream of whole batches; UHUGEINT and ENUM as text, TIMESTAMP WITH TIME ZONE tagged UTC (DuckDB tags the
  session's zone), INTERVAL as month-day-nano, `T[n]` as FixedSizeList; default settings, not
  `arrow_lossless_conversion` (it makes HUGEINT and UUID opaque bytes). Proven: pyarrow reads every chunk and every
  value equals the JSON API's (`WarehouseArrowTest`, 35 columns of every type, 0 differences). Measured, 1M rows ×
  8 columns over HTTP: JSON 734–950 ms and 92.4 MB, **Arrow 76–135 ms and 62.3 MB**. Owed: JSON inside a nested
  value renders quoted in `getString` (the C API cannot build a JSON-typed value; pinned by
  `jsonInsideANestedValueIsTheOneNamedTextDifference`); a UHUGEINT nested in a list or struct travels as DuckDB
  writes it (decimal128, right up to 2^127).
- **W1e, the native image:** `//warehouse:server_native`, GraalVM `native-image` over the server's class path as
  Bazel supplies it, with `--link-at-build-time` (a class missing from the class path fails the build, never a
  `NoSuchMethodError` at startup);
  `//warehouse:tests_native` runs the whole suite against the binary over HTTP. Its reachability metadata (the FFM
  call shapes, the identity function's upcalls, the JDK's HTTP server and crypto providers) is generated, not
  recorded, since 2026-10-05. Measured: a 21.5 MB binary, start to listening in 161 ms, 60 MB idle; 1M rows × 8 as
  Arrow in 158–167 ms; as JSON 1.20–1.27 s once chunks were written straight to bytes (2.7–4.0 s before). Linux,
  macOS, Windows (2026-10-02; `docs/WINDOWS_APP_DESIGN_2026_10_02.md`, removed: docs/HISTORY.md) and Linux arm64
  (2026-10-04: a native-image segfault there was Docker Desktop's VM, not GraalVM).
- **W1f, the JDBC driver reads Arrow by default:** the API's default stays JSON (curl and scripts; Snowflake's and
  Databricks' defaults too), and our own clients ask for Arrow. One set of value rules, java.base only (`Columnar`,
  one column of one batch in Arrow's layout, and `ApiValues`, the API's JSON values from it): the server fills it
  from DuckDB's buffers, the driver from an Arrow chunk, so the two formats give a client the same values by
  construction. With `cellText: true` each batch carries DuckDB's text for its nested cells in the Arrow message's
  metadata (`legend.cell_text`); INTERVAL text follows DuckDB's rule, pinned against DuckDB's own cast over 486
  intervals. The differential runs on both formats.
- **Results freed when done, spilled past a budget:** `DELETE /sql/v1/statements/{id}` frees a result at once (our
  client sends it after its last chunk; the 10-minute retention is only a safety net); past a memory budget across
  every result (`--result-memory-mb`, 1 GiB by default) chunks go to files under the data directory as they are
  written; `GET /health` reports `results.inMemoryBytes` and `results.spilledBytes`.

**Not in W1 (2026-09-26):** grants, ACL views and the authorizer (W2, since built); the PostgreSQL wire protocol
(W3); DuckLake and on-demand instances; OIDC (W2); the static-pivot dialect rule (§7's next step).

## 6. Identity: the principal comes from outside SQL

**The rule.** Identity inside the warehouse cannot be a session variable. From their own SQL a user can set the
variable, or shadow the function that reads it, and read another user's rows. The principal comes from outside SQL:
a scalar function registered through DuckDB's C API, called **fully qualified** (`system.main.<fn>()`, today
`system.main.authenticated_user()`), whose answer the server decides. An unmapped connection **fails closed**.

**Measured** (2026-09-26, DuckDB v1.5.5 on an Apple M4, the JVM and the GraalVM native image alike):

| Attack, from Bob's own SQL | `SET VARIABLE` + `getvariable` (the first W1 design) | the C-API function |
|---|---|---|
| `SET VARIABLE app_user = 'alice'` | **spoofed** (Bob reads Alice's rows) | immune |
| `CREATE TEMP MACRO getvariable(x) AS 'alice'` | **spoofed** | — |
| `CREATE TEMP MACRO <fn>() AS 'alice'` | — | spoofed if the view calls it unqualified; **immune** as `system.main.<fn>()` |

**How it works.** The function is implemented in the host (an upcall). Its **bind** callback runs once per query on
the calling query's own client context (`duckdb_scalar_function_get_client_context` takes the BIND info; measured:
passing the execute info gives garbage), reads the connection id, and looks the principal up in the server's own
map; the principal becomes the function's bind data, and the execute callback writes it. A connection the server
never mapped fails the query at bind ("no principal for connection 5"). Correct under DuckDB's worker threads (100
concurrent 1M-row scans, every answer the caller's own), upcalls from worker threads included.

**The name.** DuckDB's `current_user`, `session_user` and `current_role` are internal macros answering `'duckdb'`:
registering a function under that name fails, `DROP MACRO` refuses ("Cannot drop internal catalog entry"), the
system catalog cannot be written, and even `current_user` is shadowed by a temp macro. So the identity function
has its own name and views call it fully qualified; the server installs `current_user()` / `session_user()` as
temp macros over it on each connection, a convenience that only ever shows users their own name (no view reads
it).

**Rules this adds to W2:** every identity reference in a view is fully qualified; end users get no DDL (no temp
macros, no temp views: a temp view named like a real one shadows it for that connection); the variable-based
identity is retired. Tests: `noStatementCanChangeWhoTheUserIs`, `anAclViewShowsEachUserTheirOwnRows`.

**The route's other measured facts** (the homework's §1–§4): through the C API, errors are message for message
DuckDB's JDBC driver's, plus a typed kind (`PARSER`, `CATALOG`, `BINDER`, `CONVERSION`, `CONSTRAINT`, …); write
counts come from `duckdb_rows_changed`; a prepared statement gives parameter types and column names before running
(the API's `describeOnly`); a syntax error anywhere in a script refuses the whole script before anything runs;
`duckdb_interrupt` from another thread stopped a huge query at 301 ms with `INTERRUPT`, the connection still
usable. In a GraalVM CE 25 native image an FFM call costs ~2.3 µs (~8 ns on the JVM), so the design makes few C
calls: one per 2,048-row chunk, then bulk copies (per-value loops over native memory are slow there too). For a
small statement DuckDB's own parse, plan and execute dominate (JDBC ~80 µs, FFM ~66 µs on the JVM, ~99 µs native).
The Arrow type decisions are W1d's (§5).

Sources: `docs/WAREHOUSE_FFM_HOMEWORK_2026_09_26.md` §2–§5 (git history: docs/HISTORY.md).

## 7. Direct mode: the tab plans, the warehouse runs

**DataCube's Direct mode** (the program's leg D1): the tab plans with legend-lite's planner in WebAssembly, the one
DataCube already uses; only the engine changes, from DuckDB-WASM in the tab to the warehouse over the HTTP SQL API,
**as the signed-in user**, results streamed as Arrow. There is **no server-side cell encoder**: the first design
proposed one, and the measurements below made it unnecessary.

**Built (2026-09-26) as the cube's Live/Snap button**, not a separate page:

| | Live | Snap |
|---|---|---|
| plans | legend-lite in WASM | the same planner |
| runs | **the warehouse**, as the signed-in user | **DuckDB-WASM in the tab** |
| data | current, entitlement-filtered by the server | a frozen copy of every source column of the rows the user may read, under the source's own name, so one model reads both |

- **The client** (`engine-client/src/warehouse.ts`, first `datacube/src/warehouse.ts`): `signIn`, `listObjects`,
  and `WarehouseEngine`, a `QueryEngine` that speaks the HTTP SQL API (submit, poll, fetch each Arrow chunk, close;
  cancel on abort) and returns the local plane's result through `tableFromIPC` and the existing `toResultTable`.
  Hand-rolled TypeScript (the user's decision, against the Java binding in WASM and an OpenAPI spec); its reference
  is `NativeBinding.java`, and both clients are held to the real server. **Trigger to revisit:** the first outside
  consumer or a third client of the API gets an OpenAPI spec; the first non-warehouse backend called from the
  browser moves client logic into the bindings in WASM.
- **Two runners, fixed at construction:** live (planner + warehouse) and local (planner + DuckDB-WASM); the active
  one follows the plane the user clicked. **No fallback:** a live failure is an error, never answered from a snap.
- **Snapping from a live plane:** rows counted on the warehouse (the 10M ceiling), the server's Arrow chunks loaded
  into DuckDB-WASM unconverted (`insertArrowFromIPCStream`, schema included). A snap copies every source column
  (selecting only the view's columns made a fresh cube plan `select(~[])`, which was refused).
- **The source:** "Or a warehouse" in the Data window (URL, user, password: the development sign-in; the password
  is sent once and only the token kept, in memory), then the tables the user may read; `inferModel` turns the
  catalog's columns into the model (with an optional schema, planned as `"sales"."v_orders"`). Production sign-in
  is single sign-on (OIDC): the warehouse verifies the provider's token; nothing else changes.

**Measured first (2026-09-26):**

- **Versions:** the page's DuckDB-WASM (`@duckdb/duckdb-wasm` 1.33.1-dev57) is DuckDB v1.5.4; the warehouse ran
  v1.5.5; Arrow JS is `apache-arrow` 17.0.0.
- **Values need no converter anywhere.** The same SQL run by DuckDB-WASM and by the warehouse (its Arrow chunks read
  by `tableFromIPC`), both handed to the page's `toResultTable`: identical Arrow schemas, cells and Pure types for
  every type probed (BOOLEAN, all integer widths, HUGEINT, UBIGINT, FLOAT, DOUBLE with NaN and infinities, DECIMAL
  (9,2)/(18,2)/(38,10), DATE before 1970, TIMESTAMP in µs, ns and s, with time zone and before 1970, TIME, INTERVAL,
  UUID, BLOB, VARCHAR, JSON, LIST, STRUCT, MAP, fixed arrays, a grouped aggregate).
- **Every SQL the cube corpus plans, run by a warehouse reader** (granted only `TRADES`) and by DuckDB-WASM over the
  same rows (44 cases, `//datacube:cube_jvm_answers`): 36 identical; 2 the same rows in another order (no outer
  ORDER BY, so SQL promises none: compare as multisets); 2 order-dependent (`ROWS` frames ordered by `year` alone,
  so tied years may frame in either order in either engine); **4 refused**: the pivots, because legend-lite emits a
  **dynamic** `PIVOT` (no value list), which DuckDB expands into more than one statement, so the reader check cannot
  see one SELECT.
- **Pivots on data with a NULL pivot key:** the dynamic `PIVOT … ON "year"` gives 5 rows and a reader may not run
  it; legend-lite's valued pivot (`pivot(~year, [values], ~agg)`: a static `IN` **plus a pre-filter**
  `WHERE list_contains([values], year)`, legend-engine's semantics) gives **3** (groups whose key is NULL are
  dropped); a static `ON "year" IN (every distinct value)` with no pre-filter gives **5, identical to dynamic**, and a
  reader may run it. Also found: the cube's `pivotValues` path wrote `pivot(~[col], [values], ~[aggs])`, which
  matches no overload upstream or in lite (upstream's valued form takes one column and one aggregate).
- **The protocol in the module:** the SQL-API binding compiled into the planner's module adds 44.5 KB raw, 14.8 KB
  gzipped (~1%) to a 4.45 MB (1.56 MB gzipped) module.
- **The browser and the warehouse:** the warehouse then answered no CORS preflight, so a page on another origin could
  not call it; it now names the origins it allows (`--allow-origin`, `WarehouseCorsTest`). Tokens live 1 hour (the
  server's default), with no refresh.
- **Sources:** the catalog call (`GET /sql/v1/catalogs/{c}/objects`, filtered by W2's grants) returns
  `{schema, name, kind, columns: [{name, type}]}` with DuckDB's type names, which is what `inferModel` already turns
  into a model for an uploaded file.

**Proof:** `//datacube:live_snap_test` (in `//...`): the Bazel-built native warehouse, the trades rows loaded by an
owner and granted to a reader; every cube case live as the reader, then snapped and run locally; ordered cases
compared row for row, unordered ones as multisets; the 4 dynamic pivots pinned as the only live refusals; the
catalog-to-model path; an ungranted table refused. Proven red by a planted one-row difference in the snap.

**Next (2026-09-26):** the static-pivot double query (learn the values with `SELECT DISTINCT`, then a static `IN`);
single sign-on; a token refresh before expiry.

Sources: `docs/WAREHOUSE_D1_DESIGN_2026_09_26.md` (git history: docs/HISTORY.md);
`warehouse/src/test/java/com/legend/warehouse/WarehouseCorsTest.java`.
