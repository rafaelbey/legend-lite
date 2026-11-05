# Upstream behavior: how legend-pure and legend-engine actually behave

This file collects facts about legend-pure and legend-engine that were read in their source or measured, each with its
citation. It is written for the port. Where the Rust code must answer as upstream does, these are the rules it has to
reproduce. It holds facts, not decisions. Where lite deliberately answers differently, the difference is a row of
[`docs/SEMANTICS_REGISTER.md`](SEMANTICS_REGISTER.md), and this file points to the row instead of restating it.

## How to check a fact

- **Read the pinned sources.** Bazel fetches legend-pure 5.99.0 and legend-engine 4.145.0 as external repositories,
  at `$(bazel info output_base)/external/+http_archive+legend_{pure,engine}_src`. They are spec input only, never a
  runtime dependency.
- **Ask the engine.** The engine runner's jars are the engine, so no server is needed:
  `bazel build //tools/engine-runner:testable`, a scratch main compiled against its classpath, and a model over DuckDB
  or H2. [`docs/result-boundary-homework/README.md`](result-boundary-homework/README.md) §7 has the recipe step by step.
  Record each answer under `core/src/test/resources/upstream-api/` with its model and provenance.
- **Mind the versions.** The citations below are copied as their sources gave them. Some were read against older
  trees: the 2026-08 audits used local checkouts, and §4 was measured on legend-engine 4.135.0 and legend-pure 5.89.2.
  A line number can drift, so re-read the pinned tree before relying on one.

More upstream facts read from source, about the engine's JSON and CSV writers, a Float's text in compiled and
interpreted mode, a date's zone, NaN, an instance's text, a Pair and the Variant column, are in
[`docs/result-boundary-homework/README.md`](result-boundary-homework/README.md) §7. Other homes are listed in §6.

---

## 1. Printing values

### 1.1 legend-pure's `toString` and `toRepresentation`

Source: `platform/pure/essential/string/toString/toString.pure` (its PCT tests are the behavioral spec) and
`toRepresentation.pure`.

| Kind | `toString` form | Witness |
|---|---|---|
| Integer | `1`, `-1`, with no decoration | testIntegerToString |
| Float | `3.14`. Trailing zeros collapse to one (`17.000` → `17.0`); an integral float keeps `.0`. Never exponent notation (`1.3421e8` → `134210000.0`, `134.21e-10` → `0.000000013421`). A leading zero is enforced (`.01` → `0.01`) | testFloatToString\* (4 tests) |
| Boolean | `true` / `false` | testBooleanToString |
| String | verbatim and unquoted (`toString`); `toRepresentation` quotes it and escapes `\\` and `\n` | testStringToString / toRepresentation:80-84 |
| DateTime | `yyyy-MM-ddTHH:mm[:ss[.S+]]+0000`. GMT-normalised: a `-0500` input prints shifted to `+0000`. The subsecond precision is kept as written: `.0000`, `.000`, `.0` and none are four distinct prints. Minute precision keeps `T00:00+0000` | testSimpleDateToString, testDateTimeWithTimezoneToString, testDateTimeToString |
| Date (partial) | `2014`, `2014-01`, `2014-01-01`: the precision is kept, zero-padded | testDateToString |
| Decimal | `toString` is the bare number (its scale is open, below); `toRepresentation` is `toString` plus `D` | toRepresentation.pure:24 |
| List | `[a, b, c]`, nested `[[a, b], c]`: elements by `toString`, separated by `, ` | testListToString |
| Pair | `<a, b>` | testPairToString |
| Enum value | the bare member name, `CITY` (no enum prefix) | testEnumerationToString |
| Enum type / Class | the simple name, `STR_GeographicEntityType`, `STR_Person` | testEnumerationToString, testClassToString |
| Class instance | an `Anonymous_…`/`@_…` id (an identity, not a canonical form), or the class's own `toString()` qualified property | testPersonToString, testComplexClassToString |
| `toRepresentation` deltas | a String is quoted and escaped; a Date is prefixed `%`; a Decimal is suffixed `D`; everything else equals `toString` | toRepresentation.pure |

Open: how `toString` prints a Decimal's scale (`17.00d` → `17.00` or `17.0`?). No platform witness was found (§1.3).

The Float form is how the interpreted runtime holds a Float. Floats are `BigDecimal`-backed and canonicalised at
construction (`FloatCoreInstance.canonicalizeBigDecimal`): a plain string, trailing zeros stripped keeping one
decimal, an integral value given `.0`. A `BigDecimal` has no negative zero, so `0.0` and `-0.0` are one value.

Sources: `docs/CANONICAL_RENDER_HOMEWORK.md` (H1), `docs/VERDICT_RULE_AUDIT_2026_08_22.md` (git history: docs/HISTORY.md).

### 1.2 The grid form the relational tests assert: `toCSV()`

The engine's relational tests assert a grid by spelling `$result.values->toCSV()` against expected text. That Pure
function defines the grid's text. Source: `core_relational/relational/helperFunctions/helperFunctions.pure:208-250`
and `corefunctions/dateExtension.pure:384-391`.

| Rule | Form | Source |
|---|---|---|
| Frame | the header is the column names joined by `,`; rows are joined by `\n`; a trailing `\n` | toCSV:208-212 |
| TDSNull | `''` (an empty cell); `'TDSNull'` only under `renderTdsNull=true`, which is where that sentinel comes from | toCSVString:232 |
| Temporal with an hour | `yyyy-MM-dd HH:mm:ss`: a space separator, no subsecond, no zone (unlike the scalar `toString`'s `T…+0000`) | SimpleDateTimeFormat |
| Date only | `yyyy-MM-dd` | ISO8601DateFormat |
| Everything else | pure `toString()` (the §1.1 forms) plus CSV escaping | toCSVString:234 |
| CSV escaping | quoted if and only if it contains `,` `"` `\n` or `\r`; `"` doubles to `""` | escapeCSVString:245-250 |

So there are two temporal forms, one per channel. A test that asserts a scalar sees `toString` (`T`, the subsecond as
written, `+0000`); a test that asserts a grid through `toCSV` sees a space, seconds precision and no zone. The two never
mix. Floats and decimals in grid cells follow §1.1, so the open Decimal-scale question applies to both channels.

This `toCSV()` is a Pure function the tests call. The engine's CSV result format (`SerializationFormat.CSV`,
commons-csv over the driver's text) is something else, described in `docs/result-boundary-homework/README.md` §7.

Sources: `docs/CANONICAL_RENDER_HOMEWORK.md` (H2) (git history: docs/HISTORY.md).

### 1.3 Edge values: what upstream's tests pin

Measured in 2026-08 over legend-pure's essential tree and the engine's relational test tree:

- **No NaN, Infinity or -0.0 witness exists in either tree.** No upstream test pins how those print or compare. For
  NaN, see register S119 (legend-pure has none; the engine's relational JSON writes a NaN or infinite cell as `null`).
- **No witness prints a huge magnitude** such as `1e300`.
- **Decimal scale has value witnesses but no print witness.** `assertEq(3.8D, toDecimal(3.8))` and
  `assertEq(8D, toDecimal(8))` compare values; no test pins how `toString` writes a Decimal's scale.

Sources: `docs/CANONICAL_RENDER_HOMEWORK.md` (H6), `docs/VERDICT_RULE_AUDIT_2026_08_22.md` (git history: docs/HISTORY.md).

---

## 2. Cells and decoding: how a database value becomes a Pure value

### 2.1 The physical type decides, then a narrow conversion by declared type

The engine turns a database cell into a Pure value in two steps, and neither looks at the cell's text:

1. **Extraction goes by the result set's physical JDBC type**: an INTEGER becomes a Long, a VARCHAR a String
   (`ResultSetValueHandlers.java:143-157`).
2. **A narrow conversion goes by the declared column type.** TDS cell conversion handles Boolean (`$a == 'true' || $a
   == true`), Decimal (`toDecimal`) and enum (`transform`), and everything else is the identity
   (`relationalMappingExecution.pure`, `buildExecutionResultInTDS`, about :507-515).

Related rules:

| Fact | Where |
|---|---|
| `RelationalPropertyMapping.transform` handles enums only | legend-pure `platform_store_relational/functions.pure:218` |
| The execute lane's transformer table special-cases only Boolean and the Date family; a Decimal passes through as the identity, and `TEMPORARY_DATATYPE_TRANSFORMER` passes a `BigDecimal` unchanged | core `SetImplTransformers.java:32-48`, `buildTransformer` :86-105 |
| `TDSRow.values` is a plain property (`Any[*]`), so `rows.values` auto-maps and flattens | `tds.pure:76-79` |
| `assertSameElements` is `assertEquals` of the two sorted sides, so it uses `equal()` and is fully typed | `assertSameElements.pure:17-25` |

A declared type is therefore not what the wire carries. The engine's relational fixtures create some columns with
another kind than their `###Relational` declaration. The canonical case is the test id column, declared `INT` and
created `VARCHAR(200)` (`relationalSetUp.pure:1397` against `:107`). The engine never notices, so its goldens pin the
fixture's kinds. This is `docs/UPSTREAM_DEFECTS.md` U19.

Sources: `docs/F10_SLICE3_AUDIT.md`, `docs/VERDICT_DISAGREEMENT_BURN_2026_08_30.md` (git history: docs/HISTORY.md).

### 2.2 Decimals and timestamps are decoded before equality runs

The engine's strict equality (§3) never sees the database's text. It sees the value its decode made, and that decode
depends on the lane:

| Receipt | Fact | Where |
|---|---|---|
| R3 | The store lane decodes a DECIMAL as `getBigDecimal(i).doubleValue()`, which erases the scale: the cell becomes a double | `ResultSetValueHandlers.java:112-120` |
| R4 | The execute lane reads a DECIMAL raw with `getBigDecimal`, which keeps its scale | `RelationalResult.java:596-601` |
| R1 | H2 2.4.240 (measured): a `DECIMAL(18,6)` holding 1.234000 comes back from `getBigDecimal` at scale 6 (`1.234000`), and `.equals(new BigDecimal("1.234"))` is false | lite's `H2ScaleProbe` |
| R5 | Both transform paths read a timestamp through `PureDate/DateFunctions.fromSQLTimestamp`, which writes `String.format("%09d", getNanos())`: a transformed temporal value has exactly nine subsecond digits | `ResultSetValueHandlers.java:82-90`, `RelationalResult.java:561-570`, `DateWithSubsecond.java:57` |
| R6 | The execute lane's raw value path leaves a TDS timestamp a bare `java.sql.Timestamp`, printed by its own `toString`, not at nine digits | `RelationalResult.java:465-473` |

What follows from the receipts:

- **Temporal text is per result lane.** Transformed, object and graph values carry nine subsecond digits (R5); raw TDS
  cells carry the driver Timestamp's own print (R6).
- **The engine's passing dataType goldens compare scale-erased decimals.** With R3 and R8 (§2.1's transformer row),
  the store and interpreted lane those suites run in can only pass by comparing a DECIMAL cell as a number. Equality
  itself is scale-sensitive (§3.1), but the wire arrives erased before it runs. Both receipts hold, at different
  layers.
- **A port must decode as the engine decodes:** a DECIMAL scale-canonical and a TIMESTAMP at nine subsecond digits,
  chosen by the physical type of the result set (`ResultSetValueHandlers` keys on it). Canonicalising text after the
  fact is the wrong layer: it makes the text agree while the values still differ.
- **Literals keep their nine digits.** The engine's sources write DateTime literals with nine subsecond digits.
  Truncating them to six (lite's strftime `%f` round-trips did) changes the value. Several tests passed only because
  both sides truncated the same way.
- **A populated milestoning date is its written constant.** The engine projects the populated business date as the
  string constant it was written as (receipt: `%2015-10-16T00:00:01.000`), not through a TIMESTAMP round-trip that
  would drop written subseconds.

For the engine's JSON writer (`ValueTransformer` writes a `java.sql.Timestamp` as `yyyy-MM-dd'T'HH:mm:ss.nnnnnnnnnZ`)
and legend-pure's `fromSQLTimestamp` read at the pinned versions, see `docs/result-boundary-homework/README.md` §7 and
register S118.

Sources: `docs/VERDICT_DISAGREEMENT_BURN_2026_08_30.md` (§FINAL of 2026-08-31 supersedes its earlier "designed split"
disposition), `docs/F10_SLICE3_AUDIT.md` (git history: docs/HISTORY.md).

---

## 3. Equality

### 3.1 The interpreted runtime: `EqualityUtilities.equal`

The interpreted engine has one equality, read in source. `EqualityUtilities.equal` decides in this order:

1. **Identity.** The same object is equal.
2. **Two primitives** go to `eq` (below).
3. **Different classifiers** are not equal.
4. **Maps** compare key-wise by `mapEquals`.
5. **Class instances** compare their `equality.Key` properties recursively. Two distinct instances of a class without
   keys are not equal.

`EqualityUtilities.eq` for primitives requires the **same primitive type name** and then `getValue().equals(...)`.
Primitives that are not boxed (dates) compare by their name string. The consequences:

- **No equality across primitive kinds.** Integer against Float, Integer against Decimal and Float against Decimal are
  all false: the type names differ. (A test once cited for Integer-equals-Decimal, `testIntToDecimal`, compares two
  Decimals.)
- **Decimal equality is scale-sensitive** (`getValue().equals`). The engine's tests spell the exact scale SQL
  arithmetic produces and pass strictly in both engine lanes.
- **Dates compare components and the exact subsecond string** (`AbstractPureDate.java:31-51`: field-wise plus
  `Objects.equals(getSubsecond(), ...)`), so `.000000000` is not `.000000`.
- **Floats are exact decimals.** They are `BigDecimal`-backed and canonicalised at construction (§1.1), so `0.0` equals
  `-0.0` and no tolerance applies.
- **Collections compare ordered and element-wise** (`EqualityUtilities.equal(ListIterable…)`); `assertSameElements`
  sorts both sides first (§2.1).
- **An enum value is a singleton instance**, so within one enumeration its name is its identity.
- **`assertEq` on non-primitives is identity.**
- **Pair and List are classes with keys.** Pair's `first` and `second` and List's `values` carry `<<equality.Key>>` in
  source, so they compare by the instance rule.

Sources: `docs/VERDICT_RULE_AUDIT_2026_08_22.md`, `docs/VERDICT_DISAGREEMENT_BURN_2026_08_30.md` (R7) (git history:
docs/HISTORY.md).

### 3.2 Compiled legend-pure: `CompiledSupport`

Compiled mode has its own rules, and they differ from the interpreter's for numbers:

- **A Number against a non-Number is false.** After the number arms, the tail is plain `equals`, with no `toString`
  fallback (`CompiledSupport.java:1028-1104`).
- **Two Numbers compare by `equals`, then by their texts** (`CompiledSupport.eq(Number, Number)`, read at
  `CompiledSupport.java:969-1011`), and `equal()` sends numbers to the same `eq`. So `3 == 3D` is true in compiled
  mode, and `1 == 1.0` and `1.5 == 1.50D` are false. The 2026-08-30 reading recorded BigDecimal against Double as
  explicitly false. The newer reading in register S127 (2026-10-29) found `1.5 == 1.5D` true. S127 is the current
  record, and it also states lite's answer, which is pending the user's ruling.

Sources: `docs/F10_SLICE3_AUDIT.md`, `docs/VERDICT_DISAGREEMENT_BURN_2026_08_30.md` (R2), register S127 (git history:
docs/HISTORY.md).

---

## 4. How upstream tests backends: three tiers

Measured against legend-engine 4.135.0 and legend-pure 5.89.2. Upstream splits every database into three tiers. The
split is not about importance: it is about whether anyone can get a server.

| Tier | How a server is obtained | Databases | Runs in CI? |
|---|---|---|---|
| **I: executed, containerised** | testcontainers, public image | Postgres `postgres:16.10`, Oracle `gvenzl/oracle-xe:21-slim-faststart`, SqlServer `mcr.microsoft.com/mssql/server:2019-latest` (`ACCEPT_EULA=Y`), MemSQL `ghcr.io/singlestore-labs/singlestoredb-dev`, ClickHouse, Trino; H2 and DuckDB in-process; Spanner through its emulator `gcr.io/cloud-spanner-emulator/emulator` | yes, every build, unconditionally |
| **II: executed, credential-gated** | a real SaaS account | Snowflake, Databricks | skipped by default: the poms carry `<skip>true</skip>`, and a `pct-cloud-test` profile that reads AWS Secrets Manager turns them back on. CI activates it only on same-repo PRs and pushes, so fork PRs skip silently |
| **III: SQL text only** | none | DB2, Sybase, SybaseIQ, Hive, Presto, SparkSQL, Athena, Aurora, BigQuery, Redshift | not applicable: no database is involved |

The registries can be overridden (`-Dlegend.engine.testcontainer.registry=…`). Nothing is `@Ignore`d: gating is pom
profiles plus CI conditionals.

**Postgres is tier I, although upstream's own docs say otherwise.** `legend-engine/CLAUDE.md:41` lists Postgres among
the cloud stores that need the `pct-cloud-test` profile and secrets. The code is the authority:
`PostgresTestConnectionIntegration.java:49` is `new PostgreSQLContainer(DockerImageName.parse("postgres").withTag("16.10"))`,
the module's pom declares the testcontainers dependencies with no `<skip>`, and `postgres` is a group in
`.github/workflows/resources/modulesToTest.json`.

**Tier III is not a choice.** It is what a database gets when nobody can run one.

### 4.1 DB2 never touches a database

DB2 has about 30 tests upstream, and none of them runs SQL. Each asserts the text `toSQLString` produces:

```pure
let db2sql = toSQLString(|Person.all()->project(p|length($p.firstName), 'nameLength'),
                         simpleRelationalMapping, DatabaseType.DB2, ...);
assertEquals('select CHARACTER_LENGTH("root".FIRSTNAME,CODEUNITS32) as "nameLength" from personTable as "root"', $db2sql);
```

They are spread across `testToSQLString.pure`, `testSqlFunctionsInMapping.pure`, `testPostProcessor.pure`
(`testDb2ColumnRename`), `executionPlanTest.pure` and a few TDS and groupBy tests. So for tier III the SQL text is the
whole upstream contract: there are no execution semantics and no PCT baseline behind it.

- **DB2 cannot run in upstream Legend at all.** A repo-wide search for `com.ibm`, `jcc` or `jdbc:db2` finds three hits,
  all documentation URLs in comments. There is no JDBC dependency at any scope, so `DatabaseManager.fromString("DB2")`
  reaches `throw new RuntimeException(dbType + " not supported yet")`. IBM publishes a free Db2 Community Edition
  container, and upstream does not use it.
- **DB2 is structurally the lowest tier.** It has no `-pure`, `-protocol`, `-grammar`, `-execution`, `-connection`,
  `-PCT` or `-SDT` module. Its 196-line dialect is still inside `core_relational/.../sqlQueryToString/dbSpecific/db2/`,
  where every other vendor moved out to `dbExtension/`.
- **There is no DB2 PCT.** No `Test_Relational_DB2_PCT.java`, no `pct-manifests/relational-db2/`. The complete
  manifest set is h2, duckdb, postgres, snowflake, databricks, clickhouse, trino, oracle, memsql, sqlserver, spanner,
  deephaven, java, core-compiled and core-interpreted.
- *Inference, flagged as such in the source:* a dead private method
  `batchInsertRealizedRelationalResultToDB2TempTable(...)` and `SESSION.`-prefixed temp tables in the dialect suggest
  DB2 is executed in an internal fork. Nothing like it is in the open-source repos.

Sources: `docs/BACKEND_PORTABILITY.md` §7 (git history: docs/HISTORY.md).

---

## 5. What a client sees: relation types and the DataCube query store

### 5.1 `lambdaRelationType` answers precise primitives

legend-engine 4.145.0's `pure/v1/compilation/lambdaRelationType`, asked on a local engine (2026-09-27), answers
`{_type: relationType, columns: [{name, genericType: {rawType: {fullPath}, typeVariableValues}, multiplicity}]}`. A
store column's type is a **precise primitive**, with its size or precision and scale as type-variable values:

| Column or expression | legend-engine's type |
|---|---|
| VARCHAR(32) | `meta::pure::precisePrimitives::Varchar` [32] |
| INTEGER / BIGINT | `precisePrimitives::Int` / `BigInt` |
| DOUBLE | `precisePrimitives::Double` |
| DECIMAL(10,2) | `precisePrimitives::Numeric` [10, 2] |
| DATE | `StrictDate` |
| TIMESTAMP | `precisePrimitives::Timestamp` |
| BIT | `precisePrimitives::TinyInt` |
| SEMISTRUCTURED | `meta::pure::metamodel::variant::Variant` |
| `sum` of an Int | `Integer` [1] |
| `average` | `Float` [1] |
| `sum` of a Numeric (Decimal) column | `Number` [1] |
| a pivot cell `sum(if(...))` | `Float` [1] |

The computed rows of the table (`sum`, `average`, a pivot cell) are plain primitives, and `sum` over a
Numeric/Decimal column is typed `Number`, not Decimal. Lite's compiler answers plain primitives throughout (String, Integer, Float, `Decimal(10,2)`,
DateTime) and `Decimal` for that `sum`; neither difference is a register row yet (both are proposed to the user). The
BIT case is register row S23: the engine contradicts itself there, and lite reads BIT as Boolean.

Sources: `docs/DATACUBE_TYPED_VALUES_DESIGN_2026_09_27.md` (git history: docs/HISTORY.md).

### 5.2 The DataCube query store (`legend-engine-application-query`)

Read at legend-engine 4.145.0 and legend-studio `c5b2f2c78` (2026-09-28). Upstream stores saved cubes in MongoDB.

- **The record** is `DataCubeQuery {id, name, description, content, owner, createdAt, lastUpdatedAt, lastOpenAt}`;
  `content` is the cube's `DataCubeSpecification`. The client's twin is `PersistentDataCube`.
- **Search** is `POST query/dataCube/search` with a `QuerySearchSpecification {searchTermSpecification {searchTerm,
  exactMatchName, includeOwner}, limit, showCurrentUserQueriesOnly, sortByOption, …}`, where `sortByOption` is
  `SORT_BY_CREATE`, `SORT_BY_VIEW` or `SORT_BY_UPDATE`. It returns a light projection (`id name owner createdAt
  lastUpdatedAt lastOpenAt`, no content): the current user's cubes first, then the sort, then the limit (capped). The
  search term matches the id, the name or the owner.
- **Reads.** `GET dataCube/batch?queryIds=` takes at most 50 ids. `GET dataCube/{id}` stamps `lastOpenAt`.
- **Writes.** `POST dataCube`: the client supplies the id, a taken id is refused, and the owner is forced to the
  current user. `PUT dataCube/{id}`: changing the id is refused; only the owner may update, and an unowned cube is
  claimed. `DELETE dataCube/{id}`: owner only. The store also has `dataCube/events` and `dataCube/stats`.
- **Launching a cube** (`legend-application-data-cube`). The route `/:dataCubeId?` fetches a saved cube by id and loads
  its source. `?sourceData=<url-safe base64 of the raw source JSON>` builds a new cube on that source; it is the launch
  point other apps use, and it does not carry a whole cube.

Sources: `docs/DATACUBE_SAVE_SHARE_2026_09_28.md` (step 0) (git history: docs/HISTORY.md).

---

## 6. Where other upstream facts live

| Topic | Where |
|---|---|
| legend-pure's overload ranking (all type matches, then all multiplicity matches; smallest wins; a tie is "Too many matches") and the 29-package implicit import group | [`tools/reference/README.md`](../tools/reference/README.md) |
| What the engine's parser accepts and refuses, by grammar: field and section order, dates, multiplicities, per-grammar quirks | [`scripts/parser/PERMISSIVENESS.md`](../scripts/parser/PERMISSIVENESS.md), [`scripts/parser/HANDOFF.md`](../scripts/parser/HANDOFF.md) §6.2–6.5 |
| The engine's section-dispatch SPI | [`docs/GRAMMAR_EXTENSIBILITY.md`](GRAMMAR_EXTENSIBILITY.md) |
| `core_relational`'s engine model per feature: joins and isolation, milestoning, unions, views, embedded, graph fetch, dyna-functions, plans, postprocessors | [`docs/RELATIONAL_FEATURE_MAP.md`](RELATIONAL_FEATURE_MAP.md) |
| Model-to-model mappings: metamodel, connections, execution modes | [`docs/M2M_DEEP_DIVE.md`](M2M_DEEP_DIVE.md) |
| legend-engine and legend-pure defects found by lite | [`docs/UPSTREAM_FINDINGS.md`](UPSTREAM_FINDINGS.md), [`docs/UPSTREAM_DEFECTS.md`](UPSTREAM_DEFECTS.md) |
| Studio, SDLC and Depot: routes, lifecycle, engine calls, how the engine receives models | [`studio/docs/UPSTREAM_STUDIO_CENSUS.md`](../studio/docs/UPSTREAM_STUDIO_CENSUS.md) |
| The engine's SQL front end | [`docs/LEGEND_SQL_STUDY_2026_10_11.md`](LEGEND_SQL_STUDY_2026_10_11.md) §1 |
| The engine routes DataCube calls, with their inputs and outputs | [`datacube/docs/ENGINE_API_CONTRACT.md`](../datacube/docs/ENGINE_API_CONTRACT.md) |
| Every deliberate difference, with upstream's answer and its source | [`docs/SEMANTICS_REGISTER.md`](SEMANTICS_REGISTER.md) |
