# The dialect seam (2026-11-03)

How the JVM uses a Rust dialect (docs/rust-port/PHASE1.md §3, step 3): one C export, `lite_dialect`, answering one
method of Java's `SqlDialect` (and its `HostOperations`) per call, JSON in and JSON out. The JVM side is written against
this file; `src/seam.rs` (`lite_dialect::dispatch`) and `lite-ffi`'s `lite_dialect` implement it, and their tests hold
every op below (`src/seam/tests.rs`, `lite-ffi/src/lib.rs`, and `tests/duckdb_differential.rs`'s
`the_seam_answers_as_the_renderer_does` over every DuckDB render of the golden dump).

Only DuckDB is answered by Rust today. Every answer below is DuckDB's: `DuckDb.java`'s overrides, `AnsiSqlRenderer`'s
implementations and `SqlDialect`'s defaults, exactly as Java answers them.

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
- **Fresh per call.** The dialect is made from `spec` for each call and dropped after it: DuckDB carries no state from
  one render to the next. (This supersedes PHASE1.md §3's handle: no dialect state is held between calls.) The JVM
  may read `facts` once per dialect and cache it.

## 2. Arguments

### `spec`

```json
{"dialect": "DuckDb", "server": "<version>" | null, "engineScanOrder": true | false}
```

| field | | |
|---|---|---|
| `dialect` | required string | Java's simple class name. Only `DuckDb`; any other is refused. |
| `server` | string, null or missing | the server version a session reports: the dialect is taken `forServer(server)` when given. DuckDB's `forServer` keeps itself and its `serverVersions` is null, so DuckDB ignores it. |
| `engineScanOrder` | boolean or missing (false) | `Boolean.getBoolean("legend.exec.engineScanOrder")`, which Java's `DuckDb` reads at each render: the engine corpus runner's opt-in to `StableScanOrder`. The JVM passes its property's value. |

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
| `duckdb.tables` | `{}` | DuckDB's own tables, which the JVM's `com.legend.sql.dialect.DuckDb` still offers by name (`LEXICON`, `CATALOG_TYPES`, `CATALOG_REFUSED`): `{"lexicon": {"quoteChar": string, "reservedWords": [string] (sorted)}, "catalogTypes": {<canonical type>: CatalogType, ...} (Java's order), "catalogRefused": {<canonical type>: <reason>, ...} (Java's order)}` |
| `duckdb.readsStored` | `{"type": MIR column type}` | boolean: `readsStored(ColumnType)`, the `StoredReads` pass's question (a plain `OTHER` or `DISTINCT` column is read as text) |

### `facts`

`SqlDialect`'s answers that take no argument, in this order (DuckDB's shown):

```json
{"temporaryTables": true, "loadsRowsInBulk": true, "relational": true, "inProcess": true,
 "dropsStagedTablesWithTheirTransaction": false, "needsStaticPivot": false, "serverVersions": null,
 "inMemoryPath": ":memory:", "scriptAbort": "ROLLBACK", "sessionSetup": ["SET TimeZone='UTC'"],
 "keysSnapshot": "TRANSACTION", "resultColumnLimit": null}
```

- `serverVersions`: `[string]` or null; `inMemoryPath`, `scriptAbort`: string or null; `sessionSetup`: `[string]`.
- `keysSnapshot`: a `SqlDialect.KeysSnapshot` name (`TRANSACTION`, `REPEATABLE_READ`, `SERIALIZABLE`, `NONE`), or,
  where the dialect refuses it, `{"refused": {"kind": <kind>, "message": <message>}}` (§5's kind and message; the JVM
  throws it when `keysSnapshot()` is called, not when the facts are read).
- `resultColumnLimit`: an integer, null (no limit, or none known: DuckDB's), or `{"refused": ...}` likewise. Null and
  a refusal are different answers.

### Op refusals

Beyond MirJson's and `lite_json`'s words (§2), each op refuses as Java's method throws, class and message:

- the renders: whatever Java's renderer throws for the statement -- `DialectCapability` (a capability wall), and
  `IllegalStateException` for a statement that binds a parameter asked for as text (`a statement with bound parameters
  [Bind[...]] was asked for as text: render it as a statement`), among others. The golden dump's ten DuckDB refusals
  are all reproduced, class and message (`PARITY.md`).
- `sessionTable`: `sessionTable's table is a createTable, not <_type>`, `sessionTable's rows are an insertValues, not
  <_type>` (Java's signature takes those records; Rust's statements are enums, so the seam checks the variant).
- `script`, `failingStatement`: `field 'statements' is an array of strings`.
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

The dialects' code throws two classes not in the table, both `INTERNAL` with the class in the message:
`java.lang.NumberFormatException` (a NaN or infinite float written as a decimal, `new BigDecimal(double)`'s refusal;
a malformed JSON number in `CarrierStrategies`) and `java.lang.StringIndexOutOfBoundsException` (a JSON literal cut
off, in `CarrierStrategies`, which DuckDB does not run).

`lite-ffi` itself answers `INVALID_ARGUMENT` for an argument that is null (`spec is null`, `op is null`, `args is
null`) or not UTF-8 (`<name> is not UTF-8: ...`), and `INTERNAL` for a panic, a thread that cannot start (`the
dialect's thread did not start: ...`) or a refusal whose message holds a NUL (`an answer held a NUL character`; an `OK`
answer cannot hold one, as JSON writes it `\u0000`).

## 6. What stays on the JVM, and what is not exact

- `CatalogModel.database` (the Database writer over a dialect's `catalogType` and `bareNameReaches`) and
  `RawSqlBoundary`'s ledger (`Recorder`) are not dialect methods and stay Java; the seam answers the methods they call.
- `forServer` answers "this dialect" as `None` in Rust (Java's `return this`); a dialect that differs by server version
  would answer another dialect, which the seam takes for the call.
- `WireNames.fold` and `CatalogRules.typeOf` upper-case with Rust's `to_uppercase` where Java uses
  `toUpperCase(Locale.ROOT)`: the same special casing (`ß` is `SS`), each from its own Unicode version. Java's `trim`
  (every character at or below U+0020) is kept as is.
- `Boolean.parseBoolean` (a secret's `USE_SSL`) is ASCII case-insensitive `true`, as Java's `equalsIgnoreCase` is on
  that word; `extensionName`'s pattern is ASCII, as Java's.
- The renders' own differences from Java, none of which the golden dump reaches, are `PARITY.md`'s.
