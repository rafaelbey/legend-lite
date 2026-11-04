# The dialect seam (2026-11-03)

How the JVM uses a Rust dialect (docs/rust-port/PHASE1.md §3, step 3): one C export, `lite_dialect`, answering one
method of Java's `SqlDialect` (and its `HostOperations`) per call, JSON in and JSON out. The JVM side is written against
this file; `src/seam.rs` (`lite_dialect::dispatch`) and `lite-ffi`'s `lite_dialect` implement it, and their tests hold
every op below (`src/seam/tests.rs`, `src/seam/h2_tests.rs`, `src/seam/engine_tests.rs`, `lite-ffi/src/lib.rs`, and
`tests/dialect_differential.rs`'s `the_seam_answers_as_<dialect>s_renderer_does` over every op of each dialect's inputs
in the golden oracles; `src/seam/postgres_tests.rs` for Postgres, `src/seam/sqlite_tests.rs` for SQLite).

Rust answers DuckDB, H2 and H2Modern (docs/rust-port/PHASE1.md §4, steps 3 and 4b), the engine-text dialects
`EngineStyleH2`, `EngineStyleDB2` and `EngineStyleComposite` (step 4c), Postgres (step 5b) and SQLite (step 5d). Every
answer below is DuckDB's -- `DuckDb.java`'s overrides, `AnsiSqlRenderer`'s implementations and `SqlDialect`'s defaults,
exactly as Java answers them -- unless §3a gives H2's and H2Modern's (`H2.java`'s and `H2Modern.java`'s overrides over
the same base), §3b the engine-text dialects' (theirs over the same base, and Java's `EngineTextDialect`), §3c
Postgres's (`Postgres.java`'s and `PostgresHostOperations.java`'s) or §3d SQLite's (`SQLite.java`'s, with
`SqliteRegex.java`, `SqliteDigests.java` and the passes it installs).

## 1. The call

```c
char *lite_dialect(const char *spec, const char *op, const char *args);  /* freed with lite_free */
```

Every argument is a NUL-terminated UTF-8 string. The answer is a new string the caller frees with `lite_free` (and
`lite_unfreed` counts), folded as every export of `liblite_ffi` folds:

```
OK\n<the answer, compact JSON>
ERR\n<kind>\n-\n-\n<message>
```

The phase and position fields are always `-`; the message is the rest of the text and may hold newlines.

- **Thread and stack.** Each call runs on a thread of its own with 256 MB of stack reserved (`DIALECT_STACK_BYTES`;
  reserved, not committed), joined before `lite_dialect` returns: the calling thread's stack (a JVM thread's 1 MB) is
  never used for the tree. Measured: a MIR at the depth limit (2,045 nested calls, 4,096 levels) needs between 32 and
  64 MB in a debug build and more than 1 MB in release. A stack overflow aborts the process (it is not a panic); the
  depth limit (§2) and that headroom are what keep it out of reach.
- **Panics.** A panic on the dialect's thread, or a thread that cannot start, is answered `ERR\nINTERNAL\n-\n-\n<the
  panic's message>`; nothing unwinds into the caller.
- **Fresh per call.** The dialect is made from `spec` for each call and dropped after it: DuckDB, the H2 family and
  Postgres carry no state from one render to the next, and SQLite's is a statement's own (its markers, its root's labels
  and selects, and its fresh names `__b<n>`, started over at each outermost render: the user's ruling, 2026-11-03). (This supersedes PHASE1.md §3's handle: no dialect state is held
  between calls.) The JVM may read `facts` once per dialect and cache it. An engine-text dialect's render leaves one
  piece of state on Java's dialect object -- the engine's alias plan of the query, which `renderedAlias` then reads --
  and the seam rebuilds it from the query the caller says it rendered (`engine.renderedAlias`'s `rendered`, §3b).

## 2. Arguments

### `spec`

```json
{"dialect": "DuckDb" | "H2" | "H2Modern" | "EngineStyleH2" | "EngineStyleDB2" | "EngineStyleComposite" | "Postgres"
             | "SQLite",
 "server": "<version>" | null, "engineScanOrder": true | false,
 "quoteIdentifiers": true | false, "timeZone": "<zone>" | null}
```

| field | | |
|---|---|---|
| `dialect` | required string | Java's simple class name: `DuckDb`, `H2`, `H2Modern`, `EngineStyleH2`, `EngineStyleDB2`, `EngineStyleComposite`, `Postgres` or `SQLite`; any other is refused (`unknown dialect '<name>': Rust answers DuckDb, H2, H2Modern, EngineStyleH2, EngineStyleDB2, EngineStyleComposite, Postgres, SQLite`). |
| `server` | string, null or missing | the server version a session reports: the dialect is taken `forServer(server)` when given. DuckDB's `forServer` keeps itself and its `serverVersions` is null, so DuckDB ignores it; H2 is taken as H2Modern for a version that starts with neither `2.1` nor `2.2` (§3a); H2Modern, the engine-text dialects, Postgres and SQLite keep themselves. |
| `engineScanOrder` | boolean or missing (false) | `Boolean.getBoolean("legend.exec.engineScanOrder")`, which Java's `DuckDb` reads at each render: the engine corpus runner's opt-in to `StableScanOrder`. The JVM passes its property's value. Read as a boolean for every dialect; only DuckDB reads it. |
| `quoteIdentifiers` | boolean or missing (false) | an engine-text dialect's constructor argument (`new EngineStyleH2(quoteIdentifiers, timeZone)`, `new EngineStyleDB2(...)`; the plan text's connection): every physical identifier double-quoted. |
| `timeZone` | string, null or missing (null) | the same constructors' second argument, the connection's zone: a DATETIME placeholder written in `GMTtoTZ`, a date-time constant shifted from GMT to the zone (`GMT` itself shifts none; §6). |

`quoteIdentifiers` and `timeZone` configure an engine-text dialect alone: on `DuckDb`, `H2`, `H2Modern`, `Postgres` or
`SQLite` either field, present even as its default, is refused (`IllegalArgumentException`: `quoteIdentifiers and timeZone
configure an engine-text dialect (EngineStyleH2, EngineStyleDB2), not <Dialect>`). `EngineStyleComposite` has only
Java's no-argument constructor: the fields may be absent or their defaults (`false`, `null`); any other value is refused
(`EngineStyleComposite has only its default configuration (Java's one constructor: quoteIdentifiers false, timeZone
null)`). The JVM's `new EngineStyleH2()` is `{"dialect":"EngineStyleH2"}` (or the fields at their defaults), `new
EngineStyleH2(q)` is `{"dialect":"EngineStyleH2","quoteIdentifiers":q}`, and `new EngineStyleH2(q, tz)` adds
`"timeZone":tz`.

### `op` and `args`

`op` is one of the ops of §3. `args` is a JSON object (for `facts`, `{}`; its fields are ignored). It is parsed with a
nesting limit of 4,097: the args object, and a MIR in it nested at most 4,096 deep (MirJson's own limit, `lite_sql::
MAX_DEPTH`).

A **MIR** is MirJson exactly as `com.legend.executionplan.MirJson` writes it: a query is `MirJson.query(q)`, a DDL
statement `MirJson.ddl(d)`, a DML statement `MirJson.dml(d)`, a type `MirJson.type(t)`. It is read with MirJson's
reader rules and refused in its words (`unknown query tag 'x'`, `unknown scalar type 'X'`, ...).

A field read as a **string** must be present and a JSON string; a **boolean** present and `true`/`false`; a
**nullable** field may be null or missing. JSON-level refusals are `lite_json`'s (Java's `Json`) words: `missing field
'name'`, `field 'name' is null, not a string` (`an object`, `an array`, `a string`, `a number`, `a boolean`, `null`),
`field 'x' is not an integer: 1.5`, and a parse error's `<what> at line L col C`.

### Refusal order

The spec (its JSON, its `dialect`, its fields), then the op (`unknown dialect op '<op>'`, before `args` is read), then
`args`' JSON (`the args of '<op>' are a JSON object` when it is not an object), then whatever the op itself refuses.

## 3. Ops

Each answer is one JSON value. "string" is a JSON string, `[string]` an array of them.

| op | args | answer (DuckDB) |
|---|---|---|
| `facts` | `{}` | the facts object (below) |
| `forServer` | `{"version": string}` | string: the simple name of the dialect a session of that server version is spelled in, `forServer(version)` (this dialect's own name where Java returns `this`); DuckDB's is `"DuckDb"` whatever the version. The JVM builds its own shim of the named dialect (no Java code chooses a dialect by its class) |
| `query.render` | `{"query": MIR query}` | string: `render(SqlQuery)` |
| `query.renderInline` | `{"query": MIR query}` | string: `renderInline(SqlQuery)` (one line) |
| `query.renderStatement` | `{"query": MIR query}` | STATEMENT: `renderStatement(SqlQuery)` |
| `ddl.render` | `{"ddl": MIR DDL}` | string: `render(SqlDdl)` |
| `dml.render` | `{"dml": MIR DML}` | string: `render(SqlDml)` |
| `dml.renderStatement` | `{"dml": MIR DML}` | STATEMENT: `renderStatement(SqlDml)` |
| `sessionTable` | `{"table": MIR createTable, "rows": MIR insertValues \| null}` | `[string]`: the table's DDL, then (when `rows` is given) its INSERT |
| `physicalName` | `{"name": string}` | string |
| `storedName` | `{"name": string, "declaredQuoted": boolean}` | string |
| `insertColumnName` | `{"name": string, "declaredQuoted": boolean}` | string |
| `raw` | `{"statement": string, "writtenFor": string \| null}` | string: `fromEngineText(statement)` when `writtenFor` is `"H2"` (`RawSqlBoundary.ENGINE_TEXT`), else the statement as written |
| `fromEngineText` | `{"statement": string}` | string: the engine's H2 text as DuckDB runs it (`RawSqlBoundary.translated`) |
| `wireTypeName` | `{"driverTypeName": string \| null}` | string, never null: `WireNames.fold` (`""` for null) |
| `typingWireName` | `{"driverTypeName": string \| null}` | string or null; DuckDB's is `wireTypeName`'s, always a string |
| `deliversOwn` | `{"label": MIR type, "wire": string}` | boolean; DuckDB's is `false` |
| `script` | `{"statements": [string]}` | string: `BEGIN TRANSACTION;\n<s1>;\n<s2>;\nCOMMIT;` (no statements: `BEGIN TRANSACTION;\n;\nCOMMIT;`) |
| `failingStatement` | `{"message": string, "statements": [string]}` | integer (0-based) or null (`OptionalInt.empty()`); DuckDB's is `null` |
| `catalogType` | `{"column": Column}` | CatalogType |
| `catalogColumnsQuery` | `{"schema": string, "table": string}` | string: DuckDB's catalog question, each name a string literal |
| `bareNameReaches` | `{"catalogName": string}` | boolean; DuckDB's is `true` |
| `copySelectList` | `{"conversions": [Conversion]}` | string: `*`, or `* REPLACE (<sql> AS "<column>", ...)` |
| `host.openDatabase` | `{"name": string}` | `[string]`: `ATTACH ':memory:' AS "<name>"` |
| `host.useDatabase` | `{"name": string}` | `[string]`: `USE "<name>"` |
| `host.closeDatabase` | `{"name": string}` | `[string]`: `DETACH DATABASE IF EXISTS "<name>"` |
| `host.statements` | `{"operation": HostOperation}` | `[string]`, in order; empty when the operation has nothing to do |
| `duckdb.tables` | `{}` | (DuckDB only) DuckDB's own tables, which the JVM's `com.legend.sql.dialect.DuckDb` still offers by name (`LEXICON`, `CATALOG_TYPES`, `CATALOG_REFUSED`): `{"lexicon": {"quoteChar": string, "reservedWords": [string] (sorted)}, "catalogTypes": {<canonical type>: CatalogType, ...} (Java's order), "catalogRefused": {<canonical type>: <reason>, ...} (Java's order)}` |
| `duckdb.readsStored` | `{"type": MIR column type}` | (DuckDB only) boolean: `readsStored(ColumnType)`, the `StoredReads` pass's question (a plain `OTHER` or `DISTINCT` column is read as text); `readsStored` answers the same for DuckDB |
| `readsStored` | `{"type": MIR column type}` | (every dialect) boolean: Java's protected `AnsiSqlRenderer.readsStored(ColumnType)`, the dialect's override or the base's rule (a plain `OTHER` or `DISTINCT` column read as text): what `new StoredReads(new Postgres()::readsStored)` asks in core's same-package tests. DuckDB's is the base's; the engine-text dialects' `false` (every column as stored); Postgres's §3c |
| `tableName` | `{"name": string, "quoting": {"schema": boolean, "table": boolean}}` | (every dialect) string: Java's protected `AnsiSqlRenderer.tableName(name, quoting)`, a stored table's name as a query reads it (`quoting` is `NameQuoting`, its components by name): what `new Postgres().tableName(...)` asks in `TempTableAsTest`. DuckDB's is the base's (a schema-qualified name each part delimited, else `delimited` when declared quoted, `ident` otherwise) |
| `scanOrderFirings` | `{}` | integer: the statements `StableScanOrder` has changed in this process, every dialect's (Java's `StableScanOrder.firings()`, which adds them to its own; the corpus runner's engine-order register) |
| `engine.renderAsAliased` | `{"query": MIR query}` | (engine-text dialects only, §3b) string: `renderAsAliased(SqlQuery)` |
| `engine.selectList` | `{"query": MIR query}` | (engine-text only) string, or null when the query is no select (`selectList(SqlQuery)`, `@Nullable`) |
| `engine.collectionSplice` | `{"param": MIR expression}` | (engine-text only) string: `collectionSplice(PlanParam)`; `param` is a `planParam` |
| `engine.renderedAlias` | `{"alias": string, "rendered": MIR query \| null}` | (engine-text only) string: `renderedAlias(alias)` on a dialect whose last render was `render(rendered)`; with no `rendered`, a dialect that rendered nothing (the alias itself) |
| `engine.inTempTableSelect` | `{"table": string}` | (engine-text only) string: `inTempTableSelect(table)` |

### `facts`

`SqlDialect`'s answers that take no argument, in this order (DuckDB's shown):

```json
{"temporaryTables": true, "loadsRowsInBulk": true, "relational": true, "inProcess": true,
 "dropsStagedTablesWithTheirTransaction": false, "needsStaticPivot": false, "serverVersions": null,
 "inMemoryPath": ":memory:", "scriptAbort": "ROLLBACK", "sessionSetup": ["SET TimeZone='UTC'"],
 "keysSnapshot": "TRANSACTION", "resultColumnLimit": null}
```

- `serverVersions`: `[string]`, null, or, where the dialect refuses it (H2Modern's), `{"refused": ...}` as for
  `keysSnapshot` below; `inMemoryPath`, `scriptAbort`: string or null; `sessionSetup`: `[string]`.
- `keysSnapshot`: a `SqlDialect.KeysSnapshot` name (`TRANSACTION`, `REPEATABLE_READ`, `SERIALIZABLE`, `NONE`), or,
  where the dialect refuses it, `{"refused": {"kind": <kind>, "message": <message>}}` (§5's kind and message; the JVM
  throws it when `keysSnapshot()` is called, not when the facts are read).
- `resultColumnLimit`: an integer, null (no limit, or none known: DuckDB's), or `{"refused": ...}` likewise. Null and
  a refusal are different answers.

### 3a. H2 and H2Modern

H2 and H2Modern answer every op above but `duckdb.tables` and `duckdb.readsStored`; where their answer differs from
DuckDB's it is:

| op | H2 | H2Modern |
|---|---|---|
| `facts` | `{"temporaryTables": true, "loadsRowsInBulk": false, "relational": true, "inProcess": true, "dropsStagedTablesWithTheirTransaction": true, "needsStaticPivot": true, "serverVersions": ["2.1", "2.2"], "inMemoryPath": null, "scriptAbort": null, "sessionSetup": [], "keysSnapshot": "REPEATABLE_READ", "resultColumnLimit": 16384}` | H2's, but `"serverVersions": {"refused": {"kind": "DIALECT_CAPABILITY", "message": "H2Modern: a plan is written for H2's servers [2.1, 2.2], and H2Modern is chosen for any other at run time"}}` |
| `forServer` | `"H2"` for a version that starts with `2.1` or `2.2` (Java's `startsWith`: `2.10` does), `"H2Modern"` for any other (the empty one too) | `"H2Modern"` |
| the renders | H2's: every alias and derived label quoted, a physical name as its DDL spelled it, a placeholder cast to its type (`TIMESTAMP(9)` for a date-time) or to a type hole of every kind it may take (`BIGINT`, `NUMERIC` with its digits, `DECFLOAT` with its digits, `DATE`, `TIMESTAMP(9)`), the H2 family's passes (`PARITY.md`) | H2's, and typed-JSON navigation: `(x)."key"`, `(x)[i+1]`, `JSON_OBJECT(...)` for a struct, `CARDINALITY(x)` for a list's length |
| `physicalName` | the name as written when pre-quoted (`"x"`), else bare when it is `[A-Za-z_][A-Za-z0-9_$]*` and not reserved (the engine's H2 words and `right`), else double-quoted | H2's |
| `storedName` | delimited when declared quoted, else `physicalName`'s | H2's |
| `insertColumnName` | delimited when declared quoted, else bare unless needed (`[A-Za-z_][A-Za-z0-9_]*`, not reserved; no `$`) | H2's |
| `raw`, `fromEngineText` | the engine's H2 text as it is (H2's own) | H2's |
| `wireTypeName`, `typingWireName`, `deliversOwn`, `script` | the interface's defaults: `WireNames.fold`, the same name, `false`, the statements joined by `;\n` with a `;` last (`;` for none) | H2's |
| `failingStatement` | the index of the first statement whose stripped text starts the stripped quote after `SQL statement: ` in the message (an empty statement starts every quote), or null when the message has no such marker or no statement starts it | H2's |
| `catalogType`, `catalogColumnsQuery`, `bareNameReaches`, a converting `copySelectList` | the interface's refusals, naming `H2` (`copySelectList` of no conversion: `"*"`) | the same, naming `H2Modern` |
| `host.*` | refused: `a project's own database on H2 is not built: only DuckDB makes one so far (docs/ONE_PLATFORM_PROGRAM_2026_10_12.md C5)`; `host.statements`: `a host's <Operation> on H2 is not built (docs/ONE_PLATFORM_PROGRAM_2026_10_12.md D5)` | the same, naming `H2Modern` |
| `duckdb.tables`, `duckdb.readsStored` | refused (`IllegalArgumentException`): `'<op>' is DuckDb's op, not H2's` | the same, naming `H2Modern` |

The refusals H2's Java words with the literal "H2" keep it on H2Modern: `a TYPEOF compared with <TYPE>'s name reached
H2`, `plan parameter '<name>' has no type and is not typed by its value, and H2 types a parameter when it prepares the
statement: not bound on H2`, `origin-less column reference reached the case-sensitive H2 renderer: <ref> — ...`.

### 3b. The engine-text dialects: EngineStyleH2, EngineStyleDB2, EngineStyleComposite

legend-engine's own SQL text (`EngineStyleH2.java`, `EngineStyleDB2.java`, `EngineStyleComposite.java`;
`src/engine_style.rs`), never executed: the `toSQLString` and plan goldens. The three answer every op of §3 but
`duckdb.*`, and the five `engine.*` ops (Java's `EngineTextDialect`), which every other dialect refuses. Where their
answer differs from DuckDB's it is:

| op | EngineStyleH2 | EngineStyleDB2 | EngineStyleComposite |
|---|---|---|---|
| `facts` | `SqlDialect`'s defaults: `{"temporaryTables": true, "loadsRowsInBulk": false, "relational": true, "inProcess": false, "dropsStagedTablesWithTheirTransaction": false, "needsStaticPivot": false, "serverVersions": null, "inMemoryPath": null, "scriptAbort": null, "sessionSetup": [], "keysSnapshot": {"refused": {"kind": "DIALECT_CAPABILITY", "message": "a dynamic pivot's probe plan reached EngineStyleH2, which says no snapshot for it"}}, "resultColumnLimit": {"refused": {"kind": "DIALECT_CAPABILITY", "message": "a dynamic pivot's probe plan reached EngineStyleH2, which says no column limit for it"}}}` | the same, naming `EngineStyleDB2` | the same, naming `EngineStyleComposite` |
| `forServer` | `"EngineStyleH2"` | `"EngineStyleDB2"` | `"EngineStyleComposite"` |
| `query.render`, `query.renderInline` | the engine's text on one line (`renderInline` is the same text): the passes (H2's carrier strategies with no literal reduction folded, the lateral explode as a union), a TDS join's bare star wrapped, the engine's alias plan (`"root"`, `<table>_<n>`), lowercase keywords, plan parameters as FreeMarker templates (`PARITY.md`) | H2's, but `concat` infix, DB2's function forms, the null-safe OR expansions, a WHERE conjunction over an OR-rooted conjunct wrapped, group keys as expressions joined by `,`, `+`/`-` tight, a boolean placeholder quoted | DB2's, but `char_length` and the full `substring` |
| `query.renderStatement` | refused (`DialectCapability`): `the legacy engine-text plan binds no parameters: its parameters are template variables in its text (render)` | the same | the same |
| `ddl.render` | the engine's `dataTypeToSqlText` types (`INT`, `OTHER`), its column rule (a reserved word of the engine's H2 list, a pre-quoted or a space-bearing name quoted), columns joined by `,`, key names raw | H2's | H2's |
| `dml.render`, `dml.renderStatement` | a seed row as `loadValuesToDbTableH2` writes it (`insert into S.T (a,b) values (1,'x');`: one row, a cell a value); the other writes the base's (a store write refused naming the dialect) | H2's, naming `EngineStyleDB2` | H2's, naming `EngineStyleComposite` |
| `sessionTable` | the interface's default over these renders | the same | the same |
| `physicalName` | the base's identifier rule over a lexicon of no reserved word: bare when `[A-Za-z_][A-Za-z0-9_]*`, a valid pre-quoted name as it is, else double-quoted | H2's | H2's |
| `storedName` | `physicalName`'s, declared quoted or not | H2's | H2's |
| `insertColumnName` | double-quoted when declared quoted; a name that is no plain identifier double-quoted with its quotes dropped; else the engine's column rule | H2's | H2's |
| `raw`, `fromEngineText` | the engine's H2 text as it is | the same | the same |
| `wireTypeName`, `typingWireName`, `deliversOwn`, `script`, `failingStatement` | the interface's defaults (`failingStatement`: null) | the same | the same |
| `catalogType`, `catalogColumnsQuery`, `bareNameReaches`, a converting `copySelectList`, `host.*` | the interface's refusals, naming `EngineStyleH2` | naming `EngineStyleDB2` | naming `EngineStyleComposite` |
| `duckdb.tables`, `duckdb.readsStored` | refused: `'<op>' is DuckDb's op, not EngineStyleH2's` | naming `EngineStyleDB2` | naming `EngineStyleComposite` |
| `engine.renderAsAliased` | the query as `render` writes it, with the aliases it carries | DB2's text | Composite's text |
| `engine.selectList` | the select list of the query's first select (a union's first branch, recursively) after the passes, the wrap and the alias plan; `null` for a `with` | DB2's | Composite's |
| `engine.collectionSplice` | `${renderCollection(<name>![] "," <prefix> <suffix> <escapes> "null")}`, the escape pair with a trailing space (`{"'" : "''" }`); a RAW parameter refused (`IllegalStateException`: `RAW plan params are bare splices, never collections`) | H2's, a BOOLEAN quoted (`"'" "'" {}`) | DB2's |
| `engine.renderedAlias` | the alias's rename in the engine's alias plan of `rendered` (as `render(rendered)` planned it: its passes, its wrap); the alias itself when `rendered` is absent or plans no rename of it | the same | the same |
| `engine.inTempTableSelect` | `select "<table, lower case>_0".ColumnForStoringInCollection as ColumnForStoringInCollection from <table> as "<table, lower case>_0"` | the same | the same |

`engine.renderedAlias` is the one stateful call of Java's `EngineTextDialect`: Java's `renderedAlias` reads the alias
plan its dialect object's last `render` (or `selectList`) made, and `renderAsAliased` clears it. The JVM's shim keeps
the last query it rendered through `query.render` (or asked `engine.selectList` of) and sends it as `rendered`; after an
`engine.renderAsAliased`, or before any render, it sends none. The plan is a function of the query alone (made after the
passes, which a render that succeeded ran without refusal), so the replay is Java's answer; the differential holds
every alias of every engine-text `query.render` input of the oracles to it (`tests/dialect_differential.rs`).

The refusals the engine text's Java words with "engine-H2" or "the engine text" keep their words on DB2 and Composite
(`QUALIFY has no engine-H2 golden spelling`, `plan: struct extraction has no engine-H2 spelling`, `a TYPEOF compared
with <TYPE>'s name reached the engine text`); those of `AnsiSqlRenderer` and `SqlDialect` name the dialect.

### 3c. Postgres

PostgreSQL 16+ (`Postgres.java`, `PostgresHostOperations.java`; `src/postgres.rs`). Postgres answers every op of §3 but
`duckdb.*` and `engine.*`; where its answer differs from DuckDB's it is:

| op | Postgres |
|---|---|
| `facts` | `{"temporaryTables": true, "loadsRowsInBulk": false, "relational": true, "inProcess": false, "dropsStagedTablesWithTheirTransaction": false, "needsStaticPivot": true, "serverVersions": null, "inMemoryPath": null, "scriptAbort": "ROLLBACK", "sessionSetup": ["SET TimeZone='UTC'"], "keysSnapshot": "REPEATABLE_READ", "resultColumnLimit": 1664}` |
| `forServer` | `"Postgres"` whatever the version |
| the renders | Postgres's (`PARITY.md`): every name the query writes delimited (an alias, a derived label), a store's bare name folded to lower case, an alias-less projection labelled from its declared output; lists as native arrays (a nested list, a struct or a map element held as jsonb), structs and Variants as jsonb, maps as a `JSONB[]` of `{"key", "value"}`; error() a TIMESTAMPTZ cast of the sentinel-wrapped message; a placeholder bound bare, a date-time's to a type hole (`TIMESTAMP`; a finer one `TIMESTAMP` of six digits, `fraction` 6); Postgres's passes (the portable carrier strategies with FULL OUTER native, the star exclusion spelled out, whole-partition ordered sets correlated, the substring clamp, string counts as INTEGER, the Float aggregates as DOUBLE, QUALIFY by outputs, constant keys typed, the root's numeric types stated) |
| `physicalName` | the name as written when pre-quoted (`"x"`, its interior a valid quoted body), else folded to lower case: bare when `[a-z_][a-z0-9_]*` and not one of Postgres's reserved words, else delimited |
| `storedName` | delimited when declared quoted, else `physicalName`'s |
| `insertColumnName` | delimited when declared quoted, else `physicalName`'s |
| `raw`, `fromEngineText` | the engine's H2 text in DuckDB's flavour (`RawSqlBoundary.translated`), each column type's `DOUBLE` (not already `DOUBLE PRECISION`) as `DOUBLE PRECISION` and `TINYINT` as `SMALLINT`, a bare column name in lower case (`toLowerCase(Locale.ROOT)`) |
| `wireTypeName`, `typingWireName`, `failingStatement`, `catalogColumnsQuery`, a converting `copySelectList` | the interface's defaults, the refusals naming `Postgres` |
| `deliversOwn` | `true` for a map label at the wire `ARRAY` (a map is carried as its entries' jsonb[]), else `false` |
| `script` | DuckDB's: `BEGIN TRANSACTION;\n<s1>;\n<s2>;\nCOMMIT;` |
| `catalogType` | Postgres's own catalog (`pg_catalog`'s type names, upper-cased; `CATALOG_RULES`, database `Postgres`): a number, a text, a date or a timestamp as itself (`int4` `INTEGER`, `bool` `BIT`, `text` `VARCHAR(4096)`, ...), `timestamptz` `COPY_CONVERTED` to `TIMESTAMP` (`CAST(timezone('UTC', %s) AS TIMESTAMP)`), json and jsonb `SEMISTRUCTURED`, `bytea` `LEFT_OUT`, every other built-in type and the kinds `ARRAY`, `ENUM`, `COMPOSITE`, `RANGE`, `USER-DEFINED` `OTHER`; a `numeric` with precision and scale `DECIMAL(p,s)`, without them `DOUBLE`; any other refused (`a column of Postgres type '<dataType>' cannot be declared in a Pure Database (<why>)`) |
| `bareNameReaches` | `true` when the catalog name is its own lower-case fold |
| `host.openDatabase`, `host.useDatabase`, `host.closeDatabase` | refused: `a project's own database on Postgres is not built: only DuckDB makes one so far (docs/ONE_PLATFORM_PROGRAM_2026_10_12.md C5)` |
| `host.statements` | a `count` of a `query` source: `["SELECT count(*) AS n FROM (<sql>) AS counted"]`; every other operation (a count of a relation or a file too) refused: `a host's <Operation> on Postgres is not built (docs/ONE_PLATFORM_PROGRAM_2026_10_12.md D5)` |
| `readsStored` | `true` for a plain `JSON` (read `CAST(x AS JSONB)`), `ARRAY` or `OBJECT` (read `to_jsonb(x)`), `OTHER` or `DISTINCT` (read as text); `false` for every other type |
| `tableName` | each part (a schema before the first `.`, at a place past the first character) a `storedName` by its `quoting` part: `legend_stage_rowsA` is `legend_stage_rowsa` |
| `duckdb.tables`, `duckdb.readsStored` | refused (`IllegalArgumentException`): `'<op>' is DuckDb's op, not Postgres's` |

Postgres's public statics (`LEXICON`, `CAPS`, `CATALOG_TYPES`, `CATALOG_RULES`) are read by no Java code outside
`Postgres.java` (main and test sources), so no op offers them; the two protected methods core's same-package tests ask
of a Postgres object are `readsStored` and `tableName` above.

### 3d. SQLite

SQLite 3.47 through the xerial driver (`SQLite.java`, `SqliteRegex.java`, `SqliteDigests.java`; `src/sqlite.rs`,
`src/sqlite/`). SQLite answers every op of §3 but `duckdb.*` and `engine.*`; where its answer differs from DuckDB's it
is:

| op | SQLite |
|---|---|
| `facts` | `{"temporaryTables": true, "loadsRowsInBulk": false, "relational": true, "inProcess": true, "dropsStagedTablesWithTheirTransaction": false, "needsStaticPivot": true, "serverVersions": null, "inMemoryPath": null, "scriptAbort": null, "sessionSetup": ["PRAGMA case_sensitive_like = ON"], "keysSnapshot": "TRANSACTION", "resultColumnLimit": 2000}` |
| `forServer` | `"SQLite"` whatever the version |
| the renders | SQLite's (`PARITY.md`): every value as its storage class holds it (a boolean 1 or 0, a date and a date-time their ISO text, a decimal a REAL and a root decimal its text at its scale, a big integer raised past 64 bits), lists, structs, maps and Variants as JSON text, each list operation a correlated subquery over `json_each`, a fold a correlated `WITH RECURSIVE`, an explosion a marker over `json_each` positions, a lateral subquery as JSON rows, a literal regular expression as its automaton's walk, the digests in SQL, a large repeated operand bound once (`__b<n>`) and a large repeated subexpression written once (`__cse<n>`); a date or date-time placeholder's type hole `TEXT`; SQLite's passes (the portable carrier strategies with FULL OUTER native, the star exclusion spelled out, the substring clamp, whole-partition ordered sets correlated, the three explosion-marker passes, QUALIFY by outputs, constant keys typed, common subexpressions) |
| `ddl.render` | a schema's table one delimited name (`"S.Trades"`), its columns by storage class (`INTEGER`, `REAL`, `TEXT`, a binary `BLOB`), the keys joined by `,`; a schema's creation and drop `SELECT 1;` |
| `dml.render`, `dml.renderStatement` | the base's spellings, a schema's table named as the DDL names it; the store writes (`InsertSelect`, `Update`, `DeleteWhere`, `Merge`) refused naming SQLite; a temporary table from a query in the standard spelling, its query a root |
| `physicalName`, `storedName`, `insertColumnName` | the base's identifier rule over SQLite's reserved words (`key`, `order`, ... delimited) |
| `raw`, `fromEngineText` | the engine's H2 text refused (`DialectCapability`): `the engine's H2 text reached SQLite, which has no translation of it`; any other text as written |
| `wireTypeName` | `WireNames.fold` of the driver's name, `FLOAT` read as `DOUBLE` and `NUMERIC` (a NULL first value's) as `NULL`; `typingWireName` the same |
| `deliversOwn` | `true` at the wire `NULL`; at `INTEGER` for a BOOLEAN or HUGEINT label; at `VARCHAR` for a DATE, TIMESTAMP, TIMESTAMPTZ, JSON, list, struct, map or decimal label; else `false` |
| `script`, `scriptAbort`, `failingStatement`, `catalogColumnsQuery`, `catalogType`, `bareNameReaches`, a converting `copySelectList`, `host.*` | the interface's defaults, the refusals naming `SQLite` |
| `readsStored` | the base's: a plain `OTHER` or `DISTINCT` read as text |
| `tableName` | a name with a schema (a `.` past its first character) one delimited name, whatever the quoting; else the base's |
| `duckdb.tables`, `duckdb.readsStored` | refused (`IllegalArgumentException`): `'<op>' is DuckDb's op, not SQLite's` |

SQLite's public statics (`LEXICON`, `CAPS`) are read by no Java code outside `SQLite.java` (main and test sources), and
no test calls a method of a SQLite object that is no `SqlDialect` method: the core tests that make one
(`TempTableAsTest`, `SchemaDdlTextTest`, `TranslateTest`, `StoreWritesTest`, `CatalogGridTest`, `HostOperationsTest`,
`WireTypeNamesTest`) ask it `render` (of a query, a DDL and a DML statement), `renderInline`, `hostOperations` and
`typingWireName` alone, and assert refusals naming its simple class name (`SQLite`), so no op beyond §3 is needed.

### Op refusals

Beyond MirJson's and `lite_json`'s words (§2), each op refuses as Java's method throws, class and message:

- the renders: whatever Java's renderer throws for the statement -- `DialectCapability` (a capability wall), and
  `IllegalStateException` for a statement that binds a parameter asked for as text (`a statement with bound parameters
  [Bind[...]] was asked for as text: render it as a statement`), among others. The golden dump's ten DuckDB refusals
  are all reproduced, class and message (`PARITY.md`).
- `sessionTable`: `sessionTable's table is a createTable, not <_type>`, `sessionTable's rows are an insertValues, not
  <_type>` (Java's signature takes those records; Rust's statements are enums, so the seam checks the variant).
- `script`, `failingStatement`: `field 'statements' is an array of strings`.
- `forServer`: `missing field 'version'`, `field 'version' is null, not a string`.
- `duckdb.tables`, `duckdb.readsStored` asked of another dialect: `'<op>' is DuckDb's op, not <Dialect>'s`.
- `readsStored`: `missing field 'type'`, and the column type reader's words. `tableName`: `missing field 'name'`,
  `missing field 'quoting'`, `missing field 'schema'` / `'table'`, `field 'schema' is ..., not a boolean`.
- `engine.*` asked of a dialect that is no engine-text dialect (`IllegalArgumentException`): `'<op>' is an engine-text
  dialect's op (EngineStyleH2, EngineStyleDB2, EngineStyleComposite), not <Dialect>'s`.
- `engine.collectionSplice`: `engine.collectionSplice's param is a planParam, not <_type>` (Java's signature takes a
  `PlanParam`), and the dialect's own refusal of a RAW one.
- `engine.renderedAlias`: `missing field 'alias'`, and whatever planning `rendered` refuses (its passes').
- `catalogType`: `DialectCapability` -- `a DECIMAL column whose catalog gives no precision and scale ('<dataType>')
  cannot be declared`; `a column of DuckDB type '<dataType>' cannot be declared in a Pure Database (<why>)`, `<why>`
  the refused type's recorded reason, `a type this dialect does not read`, or `its catalog names no canonical type`.
  A `precision` or `scale` past an int: `field 'precision' is not an int: <n>`.
- `host.statements`: `missing field 'operation'`, and the HostOperation reader's and DuckDB's refusals (§4).
- A dialect without these (not DuckDB) refuses as `SqlDialect`'s defaults do, naming itself: `catalogColumnsQuery`
  (`UnsupportedOperationException`: `the platform asks no catalog question of <Dialect>`), `catalogType`,
  `bareNameReaches`, a converting `copySelectList`, the host operations (`DialectCapability`).

## 4. The records' JSON

Each record is an object of its Java components by name, in declaration order; a nested record the same way; a Java
enum by its `name()`; an absent nullable component `null`, always written.

**STATEMENT** (`RenderedStatement`):

```json
{"sql": string,
 "binds": [{"parameter": string, "binding": BINDING}],
 "regions": [{"start": n, "end": n, "lead": string, "separator": string, "empty": string | null,
              "firstBind": n, "bindCount": n, "counters": [n]}]}
```

Every position (`start`, `end`, `counters`, a hole's `at`) is Java's: a UTF-16 index into `sql` (`String.length()`
counts), so the JVM uses them as they come. The JVM builds its records through their constructors, whose checks hold.

**BINDING**, by its record:

```json
{"_type": "one", "nullType": string, "hole": TYPEHOLE | null}
{"_type": "array", "elementSqlType": string}
{"_type": "key", "cell": n}
```

(The component is `hole`, as `Binding.One` names it -- not PlanJson's `typeHole`.)

**TYPEHOLE** (`RenderedStatement.TypeHole`), its `types` map keyed by `ValueKind` name in the kinds' declared order
(`INTEGER`, `DECIMAL`, `FLOATING`, `DATE`, `DATE_TIME`, `DATE_TIME_NANOS`; Java's `EnumMap`), each value a
`TypeSpelling`, its `fraction` always written:

```json
{"at": n, "types": {"INTEGER": {"name": "BIGINT", "digits": "NONE", "fraction": null}, ...}, "absent": "INTEGER"}
```

`digits` is `NONE`, `PRECISION` or `PRECISION_AND_SCALE`.

**Column** (`CatalogModel.Column`), read:

```json
{"name": string, "dataType": string, "logicalType": string | null, "precision": n | null, "scale": n | null,
 "notNull": boolean}
```

`logicalType`, `precision` and `scale` may also be missing.

**Conversion** (`CatalogModel.Conversion`), read: `{"column": string, "sql": string}`.

**CatalogType**, answered: `{"read": "AS_STORED" | "CONVERTED" | "COPY_CONVERTED" | "LEFT_OUT", "declared": string |
null, "conversion": string | null, "reason": string | null}`.

**HostOperation**, read: one operation object in the encoding the browser's planner already reads
(`wasm/src/main/java/planner/HostOperationsJson.java`; engine-client's `host-operations.ts` and Python's
`compiler.host_operations` write it). That reader takes an array; the seam takes one element of it, read by the same
rules and refused in the same words. Its tags and fields:

| `op` | fields | record |
|---|---|---|
| `relation` | `kind`, `target` NAME, `replace`?, `selectList`?, `replacing`? `[{"column","sql"}]`, `from` SOURCE | `MakeRelation` |
| `insert` | `target` NAME, `columns` `[{"target","from"}]`, `from` SOURCE | `Insert` |
| `drop` | `kind`, `target` NAME | `Drop` |
| `schema` | `database`?, `name` | `MakeSchema` |
| `dropSchema` | `database`?, `name` | `DropSchema` |
| `count` | `from` SOURCE | `Count` |
| `secret` | `name`, `scope`, `credentials` (an object of strings, in order) | `Secret` |
| `extensions` | `names` `[string]` | `Extensions` |
| `extensionRepository` | `url` | `ExtensionRepository` |
| `extensionDirectory` | `folder` | `ExtensionDirectory` |
| `fileSearchPath` | `folder` | `FileSearchPath` |
| `transaction` | `step`: `begin`, `commit` or `rollback` | `Transaction` |
| `nameTaken` | `name` | `NameTaken` |
| `relationExists` | `name` NAME | `RelationExists` |
| `tablesIn` | `database`?, `schema` | `TablesIn` |
| `schemaExists` | `database`?, `schema` | `SchemaExists` |
| `currentDatabase` | | `CurrentDatabase` |
| `platform` | | `Platform` |
| `icebergManifestLists` | `url` | `IcebergManifestLists` |
| `icebergManifests` | `lists` `[string]` | `IcebergManifests` |
| `icebergDataFiles` | `url` | `IcebergDataFiles` |

- `kind` is `table` or `view`. NAME is `{"database"?, "schema"?, "name", "declared"?}`; a declared name's schema
  `default` is no schema (`HostOperation.Name`'s constructor).
- SOURCE is the first of these keys present (present even when null): `{"relation": NAME}`, `{"file": {"at",
  "format": "csv" | "csvText" | "parquet" | "json"}}`, `{"remote": {"url", "format": "csv" | "parquet" | "iceberg"}}`,
  `{"query": string}`.
- `?` marks an optional field: a string one (`selectList`, `database`, `schema`) may be missing or null; a boolean one
  (`replace`, `declared`) missing is `false`; `replacing` missing is none.
- Its refusals (`IllegalArgumentException`): `an operation is a JSON object`, `an operation's <field> is a string`,
  `an operation's <field> is true or false`, `an operation names no <field>`, `an operation's <field> is a JSON
  object`, `a host's operation is not one this platform names: <op>`, `a relation's kind is table or view, not <k>`,
  `a source is a relation, a file, a remote file or a query`, `a file is read as csv, csvText, parquet or json, not
  <f>`, `a remote file is csv, parquet or iceberg, not <f>`, `a relation's replacing is an array`, `a conversion is a
  JSON object`, `rows put in name their columns, an array`, `a column is a JSON object`, `a credential is a string:
  <key>`, `an operation's <field> is an array of strings`, `a transaction's step is begin, commit or rollback, not <s>`.
- DuckDB's own (`IllegalArgumentException`): `a secret's credential is one of [ENDPOINT, KEY_ID, REGION, SECRET,
  SESSION_TOKEN, URL_STYLE, USE_SSL], not <key>`; `an extension's name is a word of lower-case letters, digits and
  '_', not <name>`. A secret with no credentials answers `[]`.

## 5. Refusals

A refusal crosses as `ERR\n<kind>\n-\n-\n<message>`. The kind is the seam's, from the Java class the dialect threw,
matched exactly (a subclass not listed is not its parent's kind):

| Java class | kind | the JVM throws |
|---|---|---|
| `com.legend.sql.dialect.DialectCapability` | `DIALECT_CAPABILITY` | `new DialectCapability(message)` |
| `java.lang.IllegalStateException` | `ILLEGAL_STATE` | `new IllegalStateException(message)` |
| `java.lang.IllegalArgumentException` | `ILLEGAL_ARGUMENT` | `new IllegalArgumentException(message)` |
| `java.lang.UnsupportedOperationException` | `UNSUPPORTED_OPERATION` | `new UnsupportedOperationException(message)` |
| `com.legend.error.NotImplementedException` | `NOT_IMPLEMENTED` | `new NotImplementedException(message)` |
| `java.lang.NullPointerException` | `NULL_POINTER` | `new NullPointerException(message)` |
| `java.lang.ClassCastException` | `CLASS_CAST` | `new ClassCastException(message)` |
| `java.lang.IndexOutOfBoundsException` | `INDEX_OUT_OF_BOUNDS` | `new IndexOutOfBoundsException(message)` |
| anything else | `INTERNAL` | the message is `<class>: <Java's message>` |

The dialects' code throws three classes not in the table, each `INTERNAL` with the class in the message:
`java.lang.NumberFormatException` (a NaN or infinite float written as a decimal, `new BigDecimal(double)`'s refusal;
a malformed JSON number in `CarrierStrategies`; Postgres's `Integer.parseInt` of a printf width or precision past an
int, `For input string: "<digits>"`), `java.lang.StringIndexOutOfBoundsException` (a JSON literal cut off, in
`CarrierStrategies`, which DuckDB does not run) and `java.lang.ArithmeticException` (`long overflow`: Postgres's
`Math.multiplyExact` of a week-long window frame offset past a long).

`lite-ffi` itself answers `INVALID_ARGUMENT` for an argument that is null (`spec is null`, `op is null`, `args is
null`) or not UTF-8 (`<name> is not UTF-8: ...`), and `INTERNAL` for a panic, a thread that cannot start (`the
dialect's thread did not start: ...`) or a refusal whose message holds a NUL (`an answer held a NUL character`; an `OK`
answer cannot hold one, as JSON writes it `\u0000`).

## 6. What stays on the JVM, and what is not exact

- `CatalogModel.database` (the Database writer over a dialect's `catalogType` and `bareNameReaches`) and
  `RawSqlBoundary`'s ledger (`Recorder`) are not dialect methods and stay Java; the seam answers the methods they call.
- SQLite's own: `PARITY.md` (an unterminated named group refused as `java.lang.StackOverflowError` where Java's stack
  overflows, `CommonSubexpressions`' tie order).
- `forServer` answers "this dialect" as `None` in Rust (Java's `return this`) and another dialect as that dialect (H2's
  H2Modern), which the seam takes for the call (a spec's `server`); the `forServer` op answers the chosen dialect's
  name, and the JVM makes its own shim of it.
- `WireNames.fold` and `CatalogRules.typeOf` upper-case with Rust's `to_uppercase` where Java uses
  `toUpperCase(Locale.ROOT)`: the same special casing (`ß` is `SS`), each from its own Unicode version. Java's `trim`
  (every character at or below U+0020) is kept as is.
- `Boolean.parseBoolean` (a secret's `USE_SSL`) is ASCII case-insensitive `true`, as Java's `equalsIgnoreCase` is on
  that word; `extensionName`'s pattern is ASCII, as Java's.
- Postgres's `physicalName`, `bareNameReaches` and `fromEngineText` fold with Rust's `to_lowercase` where Java uses
  `toLowerCase(Locale.ROOT)`: the same casing (a final sigma too), each from its own Unicode version.
- An engine-text dialect's `timeZone` shifts a date-time constant with Java's time-zone rules (`ZoneId.of(zone,
  ZoneId.SHORT_IDS)`): the Rust library embeds the build JDK's whole tzdb (JDK 25.0.4, tzdb 2026b; every region and
  short ID, each region's rules exactly as `ZoneRules` holds them: `rust/oracles/tzdb`, `PARITY.md`), and a zone Java
  does not know leaves the constant unshifted, as Java's caught `DateTimeException` does. The JVM's own tzdb is the
  JDK's: a JVM of another tzdb version would shift by its own rules, and the data is regenerated with the build's JDK
  (`rust/oracles/tzdb/BUILD.bazel` says how).
- The renders' own differences from Java, none of which the golden dump reaches, are `PARITY.md`'s.
