# Server and cloud backends, measured on the PCT lanes

> Written in the 2026-11 docs cleanup from `docs/CLOUD_BACKENDS_PCT_2026_10_11.md` (Databricks, Snowflake and
> BigQuery on real accounts, then SQLite, MariaDB, MySQL, Trino and ClickHouse on servers Bazel unpacks; measured
> 2026-10-11 to 2026-10-14), which it replaces. Sections keep their old numbers, because code and BUILD files cite
> them ("§13", "§19"); a gap is a section that was cut. Full original:
> `git show de2745351:docs/CLOUD_BACKENDS_PCT_2026_10_11.md`. The index of this folder, and how to run a lane, is
> [README.md](README.md).

Per database, this file holds how its lane runs, what the database does differently from DuckDB (measured, with
its version), and the dialect's answer to each difference. H2 is in [h2.md](h2.md), Postgres in
[postgres.md](postgres.md), and the earlier statement-level probe studies of SQLite and MariaDB in
[sqlite.md](sqlite.md) and [mariadb.md](mariadb.md). Pin counts are as measured on the dates given: a lane's current
pins are its `Test_LegendLite_<Lane>_<Suite>Functions_PCT` expected-failure lists, and the later burn-downs are in
`docs/PCT_PIN_WORKAROUNDS_2026_10_12.md`.

**The ask (the user, 2026-10-11):** add Snowflake, Databricks and BigQuery as backends "very similar to how we just
added Postgres", passing the PCT suites, tested for real on accounts whose credentials live in AWS. Then, separately:
query each one's HTTP SQL API straight from DataCube in the browser (§7).

## 1. The bar

Postgres on 2026-10-11 (the bar the clouds were held to): **1,249 tests, 112 pinned expected failures** (Essential 69, Grammar 11, Relation 4, Standard 18,
Unclassified 10), from 267 at first measurement, burned down in five tiers on 2026-10-02 (`docs/GATES.md`, gate 7P).
The rows left are deliberate (Float is a double), the database's own error wording, or named work.

The same bar here: every suite green with the cloud's OWN pinned list, each row pinned by the text it fails with,
burned down until what is left is deliberate or a real limit of that database, each with its reason (and a
`SEMANTICS_REGISTER.md` row where it is a deliberate difference). Literally zero is not reachable on any backend.

The reference, legend-engine 4.145.0's own manifests (pinned tree, `pct-manifests/relational-<db>/*_manifest.json`),
counted 2026-10-11:

| Backend | Pinned exclusions | Essential | Grammar | Relation | Standard | Unclassified | Variant | ScenarioQuant |
|---|---|---|---|---|---|---|---|---|
| Snowflake | 276 | 139 | 46 | 17 | 51 | 5 | 16 | 2 |
| Databricks | 461 | 147 | 46 | 165 | 68 | 4 | 29 | 2 |
| Postgres | 501 | 206 | 59 | 95 | 96 | 8 | 36 | 1 |
| DuckDB | 261 | 146 | 44 | 2 | 44 | 8 | 17 | 0 |

legend-engine has **no BigQuery PCT module at all** at 4.145.0.


> Cut in the 2026-11 docs cleanup: §2, the order the backends were taken in (2026-10-11: the three clouds; then SQLite, MariaDB/MySQL, Trino, ClickHouse, MongoDB, Elasticsearch and SingleStore) and an inventory of the AWS account's database credentials. Full text: `git show de2745351:docs/CLOUD_BACKENDS_PCT_2026_10_11.md`.

## 3. How the cloud lanes are wired

- **Drivers** (`tools/deps/jars_table.bzl`), each one self-contained jar, Apache-2.0, sha1 checked against Maven Central: `com.databricks:databricks-jdbc:3.4.3` (41 MB),
  `net.snowflake:snowflake-jdbc:4.4.0` (99 MB), `com.google.cloud:google-cloud-bigquery-jdbc:1.6.0:all` (68 MB, the
  shaded jar; the plain one declares 109 runtime dependencies). `jars.bzl` takes an optional classifier
  (`group:artifact:version:classifier`).
- **The lanes** (`pct/BUILD.bazel`): `//pct:pct_<cloud>_<suite>` for cloud in `databricks`, `snowflake`, `bigquery`
  and the five suites, and `//pct:pct_<cloud>` for all five. Tagged `manual` (out of `//...` and `//gates:local`),
  `external` (never cached) and `local` (no sandbox, no remote execution).
- **The plumbing** (`pct/.../extension/CloudLane.java`, `PctBackend`): `LEGENDLITE_PCT_BACKEND=<cloud>` selects the
  lane; one session per test JVM, held for the suite (a failed login fails every later test at once with its reason);
  the storeless runtime declares the cloud's coordinates and a credential REFERENCE, never the secret
  (`StorelessRuntime.declaring`).
- **The suites**: `Test_LegendLite_<Cloud>_<Suite>Functions_PCT`, fifteen classes, each with the cloud's own
  expected-failure list (the first run on each account filled them).

> Cut in the 2026-11 docs cleanup: the rest of §3, the branch's as-built status list (2026-10-11). Full text: `git show de2745351:docs/CLOUD_BACKENDS_PCT_2026_10_11.md`.

## 4. The properties file

`LEGENDLITE_PCT_CLOUD_PROPERTIES` names a Java properties file (outside the repository; the lane inherits the
variable). For each cloud `c`:

```properties
# the JDBC URL, credentials left out
c.url=jdbc:...
# each driver property the session opens with -- the credentials among them
c.property.<name>=<value>
# the coordinates the runtime declares (not secret)
databricks.hostname=...        databricks.httpPath=...
snowflake.account=...          snowflake.warehouse=...   snowflake.region=...   snowflake.database=...
bigquery.projectId=...         bigquery.defaultDataset=...
# the SYSTEM connection's own driver properties, added to the session's (§14; not secret)
snowflake.system.property.schema=INTEGRATION_SCHEMA1
bigquery.system.property.EnableSession=1
```

A missing file or key is refused by name. Examples of the URL and properties, to confirm on the account:

- Databricks: `databricks.url=jdbc:databricks://<host>:443/default;transportMode=http;ssl=1;AuthMech=3;httpPath=<path>`,
  `databricks.property.UID=token`, `databricks.property.PWD=<personal access token>`.
- Snowflake: `snowflake.url=jdbc:snowflake://<account>.snowflakecomputing.com/?warehouse=<wh>&db=<db>&role=<role>`,
  `snowflake.property.user=...`, then key-pair (`private_key_file`, `private_key_pwd`) or `password`.
- BigQuery: `bigquery.url=jdbc:bigquery://https://www.googleapis.com/bigquery/v2:443;ProjectId=<p>;OAuthType=0;OAuthPvtKeyPath=<key.json>;DefaultDataset=<d>`.

**Where it comes from:** the user's AWS account holds the three databases' credentials (as legend-engine's own PCT
does: `AWSVaultImplementation`, Secrets Manager, us-east-1, secrets `snowflake.INTEGRATION_USER1` and `databricks`
there -- the user's names and region are to be read, not assumed). The AWS key reaches a session as the environment
variables `DB_AWS_ACCESS_KEY_ID` and `DB_AWS_SECRET_ACCESS_KEY` (the session environment's settings; prefixed because
the environment already sets `AWS_ACCESS_KEY_ID` as a proxy placeholder) and CI as the repository secret
`AWS_DB_CREDS` (five `KEY=VALUE` lines; only the access key id and secret are used -- never the console password).
**The secrets, as measured (2026-10-11):** Secrets Manager in `us-east-1` (mirrored in `us-east-2`) holds legend-engine's
own PCT secrets, read by key exactly as its `*TestConnectionIntegration` classes read them (legend-engine 4.145.0):

| Cloud | Secret | Keys used | Coordinates (legend-engine's, not secret) |
|---|---|---|---|
| Databricks | `databricks` | `integration_test.apitoken` (a personal access token) | host `dbc-f0687849-717f.cloud.databricks.com`, httpPath `/sql/1.0/warehouses/c56852187940e5a3` |
| Snowflake | `snowflake.INTEGRATION_USER1` | `encrypted_private_key` (PKCS#8, stored without PEM armour), `private_key_encryption_password` | account `ki79827`, region `us-east-2` (aws), warehouse `INTEGRATION_WH1`, database `INTEGRATION_DB1`, role `INTEGRATION_ROLE1`, user `INTEGRATION_USER1` |
| BigQuery | `bigquery.integration-bq-sa1` | `service_account_json` (a service-account key) | project `legend-integration-testing` |

The fetch is a short script (in the session's scratchpad, not the repository): `GetSecretValue` signed with SigV4 by
`curl --aws-sigv4` (the key pair read from stdin, never the command line), each secret written into `~/.legendlite/` at
mode 0600 -- the properties file, the Snowflake key re-armoured as PEM (`snowflake.property.private_key_file` +
`private_key_file_pwd`), the BigQuery key file -- and nothing printed but key names. The Snowflake JDBC URL is
`jdbc:snowflake://ki79827.us-east-2.aws.snowflakecomputing.com/?warehouse=INTEGRATION_WH1&db=INTEGRATION_DB1&role=INTEGRATION_ROLE1`.

## 5. Running

```bash
LEGENDLITE_PCT_CLOUD_PROPERTIES=$HOME/.legendlite/cloud-pct.properties \
  bazel test //pct:pct_databricks_essential --test_output=errors
```

A run reports each failing test as an `Or add to expected failure: one("<test>", "<message>")` line, the rows to pin.

**Cost**, estimated, not measured: a full run of the five suites is a few thousand short statements -- roughly 20 to
40 minutes per cloud with one session. Snowflake X-Small: 1 credit an hour; Databricks 2X-Small serverless: a few
DBUs an hour; BigQuery bills 10 MB minimum per query, so a run is cents. Burning a cloud down takes dozens of runs.

## 6. A cloud session's own setup (not in the repository)

Measured 2026-10-11 in a Claude Code cloud container (4 cores, 15 GB, network access Full). The newer and fuller
guide to the build in a cloud session is `docs/CLOUD_SESSIONS.md`.

- Bazel: `bazelisk` from GitHub releases, with `BAZELISK_BASE_URL=https://github.com/bazelbuild/bazel/releases/download`
  (`releases.bazel.build` was refused before the network was opened).
- GitHub refuses `/archive/` downloads for any repository not attached to the session (a separate check from the
  network policy). Two kinds of repository need one; `~/.bazelrc` overrides them:
  - the bats toolchains, which `bazel_lib` and `aspect_bazel_lib` register and no Java target uses: an empty
    repository (`--override_repository=bazel_lib++toolchains+bats_toolchains=<empty dir>`, and the `aspect_` one);
  - `@legend_pure_src` and `@legend_engine_src`: `git clone --depth 1 --branch legend-pure-5.99.0` (and
    `legend-engine-4.145.0`), with `third_party/legend_*_src.BUILD` copied in as `BUILD.bazel` and an empty
    `REPO.bazel`, then `--override_repository=legend_pure_src=<dir>` (and `legend_engine_src`).
- The Postgres lane: the container runs as root and `initdb` refuses root, so run it as an unprivileged user, with
  `tools/postgres/as_unprivileged.sh` (`docs/GATES.md`, since 2026-10-11). The DuckDB lane passes
  (`//pct:pct_duckdb_essential`, 45 s).
- Maven Central rate-limits the shared address now and then (HTTP 429): retry.
- The cloud lanes' test JVMs need the container's egress proxy and its CA: Bazel's test environment drops
  `JAVA_TOOL_OPTIONS`, so without it Snowflake fails PKIX and Databricks cannot connect. In `~/.bazelrc`:
  `test --test_env=JAVA_TOOL_OPTIONS --test_env=HTTPS_PROXY --test_env=https_proxy --test_env=NO_PROXY --test_env=no_proxy`.
- Disk: one output base is about 15 GB of the container's ~39 GB allowance, so a second checkout with its own output
  base does not fit; one tree, one Bazel server. A first build of the PCT lanes takes about 6 minutes.
- Measured 2026-10-11 with those settings: Databricks and Snowflake log in (their drivers authenticate when the
  connection opens) and the Grammar lane fails at `Sessions.jdbcProduct`, as expected before a dialect; BigQuery's
  connection opens too (its driver may authenticate lazily: its first query proves it).

## 7. DataCube straight to the warehouse's HTTP SQL API (CORS)

The second experiment: DataCube in the browser sending the planner's SQL to the warehouse's own HTTP API, with no
server between.

| | Endpoint | CORS preflight |
|---|---|---|
| BigQuery | `POST https://bigquery.googleapis.com/bigquery/v2/projects/<p>/queries` | **Passes** (measured 2026-10-11: `OPTIONS` from `Origin: http://localhost:5173` answers 200 with that origin, methods including POST, headers `authorization,content-type`, max-age 3600) |
| Snowflake | `POST https://ki79827.us-east-2.aws.snowflakecomputing.com/api/v2/statements` | **Passes** (measured 2026-10-11: `OPTIONS` answers 204 echoing ANY origin -- `http://localhost:5173` and `https://claude.ai` alike -- methods `GET,POST`, headers `Authorization,Content-Type,User-Agent,Accept,X-Snowflake-Authorization-Token-Type`, max-age 86400) |
| Databricks | `POST https://dbc-f0687849-717f.cloud.databricks.com/api/2.0/sql/statements` | **Fails** (measured 2026-10-11: `OPTIONS` answers 401 `WWW-Authenticate: Bearer` with no `Access-Control-*` header, on every path tried; a simple GET answers 401 without one too). A browser cannot call the workspace's API directly: it needs a relay (the warehouse server, or a Databricks App on the workspace's own origin) |

**From a page (measured 2026-10-11):** a page served at `http://localhost:5173`, in the container's pinned Chromium
(headless, through the egress proxy), `fetch`es each endpoint with `SELECT 1` and a deliberately invalid bearer token:
Snowflake and BigQuery are REACHED (the preflight passes; the page reads each one's JSON 401 --
`{"code":"390303","message":"Invalid OAuth access token..."}` and `{"error":{"code":401,...}}`), Databricks is BLOCKED
by the browser (`TypeError: Failed to fetch`). A real query from the page needs a real short-lived credential in the
browser -- a Snowflake key-pair JWT (`X-Snowflake-Authorization-Token-Type: KEYPAIR_JWT`, minted from the
`snowflake.INTEGRATION_USER1` key) and a BigQuery OAuth access token (from the service account) -- which this session did
not mint into a page (the session's safety review stopped it as credential materialisation): **open, the user's call**.

Questions it settles: whether each preflight answers for the page's origin; which credential a browser can hold
(BigQuery: an OAuth access token; Snowflake: OAuth or a key-pair JWT; Databricks: a token); and the result format
the grid needs (each API returns JSON rows; Databricks and Snowflake can also return Arrow).

**Where it plugs in** (surveyed 2026-10-11): the page's database is `QueryEngine` (`engine-client/src/engine.ts`:
`execute(plan)` to a `ResultTable`, `run(sql)` to a `RawTable`); a live source is also a `RemoteSource`
(`engine-client/src/snap.ts`: `arrowChunks`, Arrow IPC, which snapping loads into DuckDB-WASM). `WarehouseEngine`
(`engine-client/src/warehouse.ts`) is the model to follow: its statement protocol (submit, poll, fetch chunks,
cancel, a bearer token, sign-in in the source picker, nothing stored) is already the shape of Databricks' statement
API. The planner takes the dialect from the table model's connection `type` (`wasm/.../planner/Boundary.java`), so the
browser path needs the same three dialects as the PCT lanes: **the core dialect work serves both**. Order: first a
raw-SQL probe from the page (`run(sql)`: CORS, the credential, the result format) on each account, then an engine
per cloud behind `QueryEngine` once its dialect plans. A JSON result is decoded to the exact `Scalar`s
(`engine-client/src/values.ts`) and to Arrow IPC for snapping. Constraints: `datacube/test/guardrails.test.ts` (no
runtime fallback between engines) and the bundle budget.

## 8. Databricks, measured (2026-10-11)

**Result:** all five suites pass on the account, each failure pinned by its message: 1,249 tests, 35 pinned (Grammar
9 of 137, Essential 25 of 345, Relation 1 of 469, Standard 0 of 205, Unclassified 0 of 93), the census divergence 0
in every suite (burned down from 185 failing on the first run). A run of the five is about 10.5 minutes with three
suites at a time.

What the 35 are: 30 are the platform's own, pinned on the DuckDB lane by the same cause (instance identity, fold name
resolution, error positions and messages, the 0-based `indexOf`, `testBigFloatAbs`, the class `toString`, the
variant-array `joinStrings`, the ExtendedInteger type, the harness's lost `$address`); the other 5 are Databricks'
own, each measured: a date-time past Spark's range overflows (4; the DuckDB lane pins the same tests, by its own
messages), and the driver gives a BC date-time back as the Timestamp's text with its sign lost (1, §15).

The account: a Databricks SQL warehouse (`dbsql_version` 2026.39); the JDBC driver (databricks-jdbc 3.4.3) reports the
product `SparkSQL` (`Sessions.jdbcProduct`). Every spelling in `core/.../sql/dialect/Databricks.java` was measured there
first with a scratch JDBC probe (statements over literals and `VALUES` only). What differs from DuckDB, and the dialect's
answer:

| Measured | Consequence |
|---|---|
| `'it''s'` is `its`: two adjacent literals concatenate; backslash escapes are read | a quote is written `\'`, a backslash `\\` |
| a bare integer literal is a 32-bit INT; `2147483647 + 1` wraps to `-2147483648` with ANSI mode off | every integer literal is a BIGINT, `1L`; a window function's offset stays an INT |
| the warehouse's ANSI mode is OFF by default: overflow wraps, `CAST('abc' AS BIGINT)` is NULL, `x/0` is NULL, `element_at` out of range is NULL | the session sets `ANSI_MODE = true` (each then raises, as DuckDB does) and `TIME ZONE 'UTC'` |
| `concat('a', NULL)` and `'a' \|\| NULL` are NULL | concat is `concat_ws('', ...)`, each argument as text |
| `regexp_extract` defaults to group 1 (and refuses a pattern without one); `split` takes a regex; `regexp_replace` replaces every match | group 0 is passed; the delimiter is `\Q..\E`-quoted; a first-match replace is `^(?s:(.*?))(?:p)` → `$1` + the replacement, its groups shifted by one |
| `round` is half-up, `bround` half-even | ROUND is `bround`, ROUND_HALF_UP `round` |
| `timestampdiff` counts whole units; DuckDB's date_diff counts boundaries | year/quarter/month by the calendar, day by `datediff` of dates, sub-day by the floored `unix_micros` |
| `extract(DOW ...)` is Sunday 1 | DuckDB's dow is `extract(DOW ...) - 1`; isodow is `DOW_ISO` |
| a lambda's index parameter is 0-based | the body reads `i + 1` (DuckDB's is 1-based) |
| a bare `NULL` where an array is required is refused (`VOID`) | the empty collection is `CAST(NULL AS ARRAY<VOID>)`; a list function over it answers `CAST(NULL AS <its type>)` |
| a parenthesized `(VALUES ...) AS t(c)` is a syntax error | `VALUES (..), (..) AS t(c)` |
| no quantified comparison (`x = ALL (subquery)`) | its SQL truth table over `EXISTS`, NULLs included |
| a RANGE frame's offset must have its ORDER BY key's type | `1L PRECEDING` over a BIGINT key |
| `map_concat` refuses a duplicate key (its dedup policy is not a warehouse setting) | each map less the keys a later one sets (`map_filter`), then concatenated |
| `CAST(struct AS VARIANT)` is refused; `to_variant_object` converts; a variant takes no `[...]` subscript; object keys come back sorted | variant navigation casts to `MAP<STRING, VARIANT>` / `ARRAY<VARIANT>` (`try_element_at`) |
| arrays, structs, maps and variants arrive over JDBC as JSON text; `getObject(i, LocalDateTime.class)` answers a String | the executor's JSON carrier parses lists; a struct's text is decoded by the dialect's `normalize`; the timestamp re-fetch keeps the driver's `Timestamp` when the driver answers another class |
| no as-of join, and a correlated scalar subquery in a join condition (the portable form's correlated MAX) is refused | an as-of join is a LEFT JOIN on its condition whose select keeps, per left row, the matches with the extreme right key: `QUALIFY k = MAX(k) OVER (PARTITION BY every left column) OR MAX(k) OVER (...) IS NULL` (MIN when bounded below; the portable form's rows, ties included; only under a select that filters, groups and windows nothing of its own) |
| no subquery inside a higher-order function's arguments (`UNSUPPORTED_SUBQUERY_EXPRESSION_CATEGORY.HIGHER_ORDER_FUNCTION`): the mixed sort's ordered subquery sat under the PCT root's NULL-dropping `filter` | a list's compaction is `array_compact` (no lambda; the same rows: a NULL list NULL, an empty one empty, a nested list's NULLs kept) |
| Spark's variance and standard deviation are another update than DuckDB's, equal in algebra but not in rounding (3.14's came back 3.1399999999999997) | a list's moments are DuckDB's own update in an `aggregate` whose finish is the result (40 values over 10 lists equal to DuckDB's, measured) |
| `QUALIFY`, `SELECT * EXCEPT`, native `PIVOT` (columns named by the `IN` aliases), `listagg ... WITHIN GROUP`, `median`, `mode`, `percentile_cont/disc ... WITHIN GROUP`, `max_by`, `max(boolean)`, `explode`, correlated `(SELECT agg FROM explode(t.xs))` | used as they are |

## 9. Snowflake, measured (2026-10-11)

**Result:** all five suites pass on the account, each failure pinned by its message: 1,250 tests, 35 pinned (Grammar
9 of 137, Essential 25 of 345, Relation 1 of 469, Standard 0 of 205, Unclassified 0 of 93), the census divergence 0
in every suite (burned down from 144 failing on the first run).

What the 35 are: 34 are the platform's own, pinned on the DuckDB lane by the same cause (instance identity, fold name
resolution, error positions and messages, the 0-based `indexOf`, `testBigFloatAbs`, the class `toString`, the
variant-array `joinStrings`, the ExtendedInteger type, the harness's lost `$address`, a date-time out of range); the
other 1 is Snowflake's own, measured: the driver's Timestamp drops a BC year's sign.

The account: warehouse `INTEGRATION_WH1`, database `INTEGRATION_DB1`, key-pair login; the JDBC driver
(snowflake-jdbc 4.4.0) reports the product `Snowflake` (`Sessions.jdbcProduct`), version 10.36. Every spelling in
`core/.../sql/dialect/Snowflake.java` was measured there first with the scratch JDBC probe. What differs from DuckDB,
and the dialect's answer:

| Measured | Consequence |
|---|---|
| an unquoted identifier folds to UPPER case | every identifier is quoted |
| an integer is a `NUMBER(p,0)` at the value's own precision, which the driver hands back as a `Long`; `17.0` and `1.0` are `NUMBER(2,0)` (a trailing zero's scale is dropped, so their text is `17`) | the census reads `NUMBER(p,0)` as delivering an integer label; a decimal cell read as a `Long` becomes a `BigDecimal` (`normalize`); a Float or Decimal literal with a trailing zero spells its type (`CAST(17.0 AS NUMBER(3,1))`) |
| a DOUBLE's text drops an integral value's `.0` (`-1`, not DuckDB's `-1.0`) | a Float cast to text appends it (`REGEXP_REPLACE(..., '^(-?[0-9]+)$', '\\1.0')`) |
| lists are semi-structured `ARRAY`s of `VARIANT`; `ARRAY_AGG` drops a NULL, `OBJECT_INSERT` drops a NULL value's key; an `OBJECT`'s keys compare in name order | elements are cast where read; NULLs kept as JSON null; a list of structs sorts as an `ARRAY` of its fields in declared order (minBy/maxBy, sort with a key) |
| a lambda reads no enclosing lambda's parameter; a correlated subquery over `FLATTEN` (scalar or lateral) is refused (`Unsupported subquery type`) | list statistics are `REDUCE`s: variance and standard deviation as DuckDB's own Welford update, mode as runs over the sorted elements (the earliest-seen on a tie, as DuckDB), median and `quantile_cont`; an explosion is a bare `LATERAL FLATTEN` over the positions, `SELECT *` over it spelled out; a `FILTER` or `TRANSFORM` whose lambda reads an enclosing lambda's names (the comparator dedup's) is a `REDUCE` whose accumulator carries them, its start written where they are in scope (an enclosing parameter is legal there, measured) |
| regular expressions are POSIX: no `(?:`, no lazy quantifier, no inline flags | flags become parameters; `(?:` a plain group where no numbered group follows; `regexpIndexOf` is `REGEXP_INSTR` of the original pattern (its lowering's lazy prefix cannot run here) |
| no printf | `format`'s `%s`, `%d` and `%.Nf` are unrolled (`%f` keeps Java's `-0.00`) |
| `LISTAGG`, `MEDIAN` and the percentiles take no ordered window; `WITHIN GROUP` is refused beside an `OVER`'s `ORDER BY`; an `ARRAY_AGG` over an ordered whole-partition frame keeps no order | a cumulative ordered string aggregate is `ARRAY_TO_STRING(ARRAY_AGG(..) OVER (...))`; a whole-partition one the partition's `LISTAGG ... WITHIN GROUP` |
| `FIRST_VALUE`/`LAST_VALUE`/`NTH_VALUE` refuse a cumulative frame, and default to the whole partition (not ANSI's) | first: the partition's first; last: the current row (ROWS) or the last of its peers (RANGE); nth: the partition's nth once the frame holds n rows |
| a frame needs an `ORDER BY`; `RANGE` `CURRENT ROW` to `CURRENT ROW` over a date or timestamp is refused | a whole-partition frame without one is no frame; the peers are the zero-interval frame |
| `ASOF JOIN` takes its inequality as a `MATCH_CONDITION`, apart from `ON` | the ON's conjuncts split |
| a `+HHMM` offset is "not recognized" (`+HH:MM` is) | text to an instant rewrites the offset |
| `getObject(i, LocalDateTime.class)` throws a plain `SQLException` | the executor keeps the `Timestamp` it already read (the BC sign loss is pinned) |
| a VARIANT column is a JSON carrier; DuckDB's JSON travels as VARCHAR, which the plan labels; `ARRAY_MIN(NULL)` and `GET(NULL, 0)` are VARIANT columns | a JSON root column under a VARCHAR label is `TO_JSON(..)`; an exploded NULL list's column takes its label's type, and a min, max or element of the NULL list is its label's NULL (the root's labels as planned, before the dialect's passes) |
| the session: `TIMEZONE = 'UTC'`, `WEEK_START = 1`, `TIMESTAMP_TYPE_MAPPING = 'TIMESTAMP_NTZ'`, `QUOTED_IDENTIFIERS_IGNORE_CASE = FALSE`, `JSON_INDENT = 0`, `STRICT_JSON_OUTPUT = TRUE` | set at session start (`sessionSetup`) |
| `QUALIFY`, `EXCLUDE`, `MAX_BY`/`MIN_BY`, `BOOLAND_AGG`, `REDUCE`/`TRANSFORM`/`FILTER`, `ARRAY_*`, `OBJECT_*`, `CONVERT_TIMEZONE`, `DATEADD`/`DATEDIFF` | used as they are (with DuckDB's boundary-count `date_diff` per unit) |

**Jaro-Winkler, written exactly (2026-10-11, the user's ruling: exact SQL where the backend has lambdas).** Snowflake's
`JAROWINKLER_SIMILARITY` is another function: an integer percentage, case-insensitive (96 for MARTHA/MARHTA, 100 for
abc/ABC, 0 for two empty strings), so dividing by 100 does not give DuckDB's fraction; Databricks has none. Both now
write DuckDB's own algorithm (rapidfuzz's) in SQL: the match window `max(|p|, |t|) / 2 - 1`, at least 0; each
character of `t` in turn takes the first unmatched equal character of `p` in its window; half the matches out of
order, ROUNDED DOWN, are the transpositions (rapidfuzz's integer division: 181 of 1,585 pairs differ with the
textbook's half); a Jaro score above 0.7 gains a tenth of its distance to 1 per character of a common prefix of up to
four; an empty string scores 0. The score is computed in DOUBLE in DuckDB's order of operations, so the bits agree:
measured on 1,585 pairs (alphabets of 2 to 8 letters, lengths 0 to 140, past the 64-character block of rapidfuzz's
bit-parallel matcher), every result equal to DuckDB's on both accounts. Snowflake's is three `REDUCE`s over `OBJECT`
states, each start carrying what the next lambda reads (a lambda reads no enclosing lambda's parameter); Databricks' is
`aggregate`s whose finish lambdas chain the passes. One difference remains, DuckDB's own: it compares UTF-8 bytes,
these compare characters, so a non-ASCII pair can differ (no test has one). SQLite, MariaDB, MySQL, Postgres and
BigQuery have no lambdas and keep their wall.

## 10. BigQuery, measured (2026-10-11)

**Result:** all five suites pass on the account, each failure pinned by its message: 1,250 tests, 42 pinned (Grammar
10 of 137, Essential 28 of 346, Relation 2 of 469, Standard 0 of 205, Unclassified 2 of 93), the census divergence 0
in every suite (burned down from 347 failing on the first run).

What the 42 are: 34 are the platform's own, pinned on the DuckDB lane by the same cause (as on Snowflake); the other
8 are BigQuery's own limits, each measured: no list of lists, so removeDuplicates' reduction has no accumulator (3:
"nested arrays are not supported"); a date-time past years 1 to 9999 (2); no Jaro-Winkler function (2); and a fold
that reads its row (1: WITH RECURSIVE is top-level only, and there are no lambdas).

**What BigQuery lacks, and how each gap was closed (2026-10-11).** Each row was a lambda-less or BigQuery-shaped gap,
closed as a SQL author would on BigQuery, each shape measured on the account first (40 pins; the last line is the
second fix the date format's row needed):

| Pins | Was | Now |
|---|---|---|
| 8 | no lateral derived table ("LATERAL join is not supported"; a correlated derived table: "Unrecognized name") | a lateral join is `[LEFT] JOIN UNNEST(ARRAY(SELECT AS STRUCT * FROM (q)))`: an UNNEST correlates, through nested derived tables too (`BigQuery.lateral`) |
| 6 | no mode | the group's (or partition's) counts per value in a wrapping select, `COUNT(v) OVER (PARTITION BY keys, v)` (a FLOAT64 key partitions by its JSON text: no PARTITION BY on FLOAT64), then `ARRAY_AGG(v ORDER BY count DESC, v LIMIT 1)` or `FIRST_VALUE` over the same order; a tie takes the least value (DuckDB's takes the first row scanned, which SQL cannot name) |
| 11 | no MAP type | a map is its entries in order, `ARRAY<STRUCT<key K, value V>>` (BigQuery's own way to load one); DuckDB's semantics measured and kept: a NULL or repeated key raises DuckDB's message, a concatenation keeps each key at its first place with the last map's value, an extraction is a list; a Variant object's keys come in BigQuery's order, sorted (its JSON keeps no document order, measured), where DuckDB keeps the document's; the executor decodes the entries to the map, the census reads ARRAY as delivering MAP |
| 6 | no lambdas and no correlated recursion to fold with | a fold that reads nothing of its row is a top-level `WITH RECURSIVE` expression, read as its last accumulator (`FoldsAsRecursiveCtes`, MIR's `SqlWith.recursive`); a fold over the NULL list is its init; list_reduce the same from its first element (removeDuplicates' three now stop at the nested-array limit; the fold that reads its row stays) |
| 2 | a filter's or map's lambda parameter left unbound (the portable carrier strategies', on Postgres, SQLite and MariaDB too) | a read qualified by the parameter (`p.first`) is a field of the substituted element (`CarrierStrategies.substParam`); the third such row, `testFilterInstance`, now fails as on DuckDB (instance identity, the platform's own) |
| 3 | an empty list of no element type | `[]` is a list of Nil's carrier, VARCHAR (the column its element fills is labelled so); a lambda body over it, which never runs, is a NULL of the body's own type |
| 1 | a statement past the maximum query length (a float's text canon, 2.9M characters) | a large repeated subexpression written once (`CommonSubexpressions`, SQLite's pass; its one-row derived table under a scalar subquery correlates here, measured): 24K |
| 1 | PERCENTILE_CONT over a window ascending only | a descending one is DuckDB's own interpolation over the ascending order of the negated values, at an exact BIGNUMERIC position (`BigQuery.descendingOver`) |
| 1 | no minimal-fraction date format | `%E3S`'s milliseconds after the second |
| 1 | a cast of a JSON lambda parameter | the parameter of a JSON array's lambda is typed JSON, so its cast reads JSON (`INT64(j)`) |
| (the same) | behind the date format, the format's time-zone shift inverted (`18:07+0500` for `08:07-0500`) | timezone() decides instant or naive by its operand's shape, as Snowflake does (`BigQuery.isInstant`): format's shift is timezone(zone, timezone('UTC', t)), whose inner call is typed naive but renders an instant |

**Found later (2026-10-13, the dialect program's defect 10):** BigQuery keeps no order between the branches of a
`UNION ALL`. An ordered collection of instances, whose literal list lowers to a `UNION ALL` of one-row selects with
no ordinal and no `ORDER BY`, comes back in either order (`collection::tests::filter::testFilterInstance` answered
`[i1, i2]` or `[i2, i1]` from run to run on channel B's Grammar suite).

The account: project `legend-integration-testing`, a service-account key; the JDBC driver (Google's
google-cloud-bigquery-jdbc 1.6.0, the shaded jar) reports the product `Google BigQuery` (`Sessions.jdbcProduct`).
`core/.../sql/dialect/BigQuery.java` was drafted against the account (its own probes, kept in the dialect's javadoc)
and burned down on the lanes. What differs from DuckDB, and the dialect's answer:

| Measured | Consequence |
|---|---|
| a string literal reads backslash escapes and refuses `''`; the driver counts every `?` as a parameter, inside a literal too | a quote is `\'`, a `?` is `\x3f` |
| `1.5` is a FLOAT64 (DuckDB's is a DECIMAL); a parameterized type cannot be cast to; a BIGNUMERIC drops a trailing zero's scale (`'3.0'` reads 3) | a decimal literal is `BIGNUMERIC '1.5'`; a decimal cast a rounded BIGNUMERIC; its text has its scale's digits (`FORMAT('%.Nf')`); a decimal read back is padded to its declared scale (`normalize`); the census reads BIGNUMERIC as delivering decimal, HUGEINT and Float labels |
| no VALUES clause, no bare UNION, no OFFSET without LIMIT, no lambdas, no lateral derived table, no MAP type | rows are `UNNEST(ARRAY<STRUCT<..>>[..])`; `UNION ALL`/`UNION DISTINCT`; every list operation a correlated subquery over `UNNEST(xs) WITH OFFSET` |
| no set-returning projection | an explosion is a marker (`ExplosionMarkers`, shared with Snowflake) written as `UNNEST(GENERATE_ARRAY(0, n - 1))` over its longest list, a lateral one `CROSS`/`LEFT JOIN UNNEST`, each exploded column `list[SAFE_OFFSET(position)]`, a root one ordered by its position |
| a FROM-less select takes no WHERE | it runs over one row |
| no quantified comparison, and no derived-column alias list | `x op ANY/ALL (q)` is its three-valued count over `UNNEST(ARRAY(q))` |
| no `=` on STRUCT or ARRAY | compared through `TO_JSON_STRING` (DuckDB's nested NULLs compare equal, as the texts do); a NULL operand is NULL |
| a CASE's branches need a common type (INT64 and STRING have none); a column's text label may not be its value's (a pivot's) | a text cast is always written, but on a string literal |
| `ROUND` of a FLOAT64 is exact (31.41499... is 31.41) | DuckDB's own algorithm: `ROUND(x * 10^n) / 10^n` (31.42) |
| `FORMAT`'s `%s` takes a STRING only | a `%s` argument is cast to text |
| `FARM_FINGERPRINT(NULL)` is NULL (DuckDB's `hash(NULL)` is a number, and Pure's `hashCode([])` is one) | `hashCode` fingerprints the value's text as JSON, whose NULL is `'null'` |
| a bare NULL is an INT64 (`COALESCE(NULL, NULL)`, `UNNEST(NULL)`) | at the root, a value NULL by its shape is its label's NULL; a NULL list of known element type is `CAST(NULL AS ARRAY<T>)` |
| the driver's type names: INT64, FLOAT64, DATETIME (naive), TIMESTAMP (an instant) | the census reads them (`SqlTypeCensus.dialectSpelling`) |
| `QUALIFY`, `SELECT * EXCEPT`, `PERCENTILE_CONT/DISC` (analytic), `ARRAY_AGG ... ORDER BY`, `STRING_AGG ... ORDER BY`, `IEEE_DIVIDE`, `SAFE_OFFSET` | used as they are |

## 11. Further backends: candidates

> Since written: Trino (§18), ClickHouse (§19) and MongoDB (`docs/MONGODB_PLAN_2026_10_16.md`) were built; the rest
> of this section stands as the 2026-10-11 candidate list.

The order for the open ones was decided on 2026-10-11 (SQLite, MariaDB/MySQL, Trino, ClickHouse, MongoDB,
Elasticsearch, then SingleStore); the rest of this list is not. legend-lite replaces
legend-engine, so the first list is the relational databases the engine supports (`DynaFn` names several: MemSQL,
Spanner, Trino), ordered by how much an existing dialect carries over and by what users run:

| Backend | Carried over from | How it is tested | Notes |
|---|---|---|---|
| Redshift | `Postgres` (Redshift forked Postgres 8) | the AWS account's Redshift Serverless workgroup (`default`, us-east-1; it has a running cost) | the cheapest to add: mostly walls where Redshift lacks newer Postgres features |
| Trino (Presto, Athena, Starburst) | `Databricks` (typed arrays, lambdas, maps) | the `trinodb/trino` image with the memory connector | one dialect covers Athena too |
| SQL Server | none | the `mssql/server` image (free Developer edition) | large enterprise use; the hardest of these: no boolean type, `TOP` and `OFFSET ... FETCH`, lists through JSON (`OPENJSON`) |
| Oracle | none | the `gvenzl/oracle-free` image (23ai) | `''` is NULL, a deep divergence that will pin many tests; BOOLEAN only since 23ai |
| SingleStore (MemSQL) | MySQL | its free developer image | follows the MySQL dialect |
| Spanner | `BigQuery` (GoogleSQL) | Google's Spanner emulator | |
| DB2, Sybase ASE/IQ | the engine-text printers (`EngineStyleDB2`) are text only, not an execution dialect | licences make a lane hard | low priority |

The images need Docker, which this cloud container has (`/usr/bin/docker`); CI would need it too, or the engines' own
packages.

**Stores that are not SQL.** legend-engine has both: a MongoDB store (class queries to aggregation pipelines) and an
Elasticsearch store (relation queries to the query DSL). In legend-lite they fit beside `SqlDialect`, not inside it:
the Lowerer's output (the `SqlQuery` IR) stays the input, and a new target translates it into the store's own query
language. The executor gains a second chartered seam beside JDBC (the store's own client). The execution tenet still
holds: the store executes, and what it cannot express is a `DialectCapability` wall, never a fallback computed in Java.

- **MongoDB: a good fit.** The aggregation pipeline covers most of the relation algebra: `$match` (filter), `$set`
  (extend), `$project` (select), `$group` (groupBy), `$sort`, `$limit`/`$skip`, `$lookup` (join), `$setWindowFields`
  (over), `$unionWith` (concatenate), and `$documents` for a literal relation (so the PCT's `#TDS` literals need no
  loading). Its expression language has lambdas (`$map`, `$filter`, `$reduce`), so lists are native. Nested
  documents suit Pure classes. Scalar PCT runs as `$documents: [{}]` then `$project`. Tested with the `mongo` image or
  an embedded `mongod`.
- **Elasticsearch / OpenSearch: narrower.** Filter, sort, page, and grouped aggregates map well (DataCube's
  dashboard shape). There is no general join, window or computed-list support, so most of the relation PCT would be
  walls. Two ways in: ES|QL, a pipe language that reads like a relation chain (`FROM | WHERE | EVAL | STATS ... BY |
  SORT | LIMIT`), or the query DSL. A literal relation must be loaded into an index first, since ES|QL's `ROW` is a
  single row. Licences: Elasticsearch server is under Elastic, SSPL or AGPL licences; OpenSearch is Apache-2.0 and
  has its own SQL and PPL.


## 12. SQLite, measured (2026-10-11)

The first of the open ones. No account: the product's own driver, in-process and in memory.

**Result:** all five suites pass on SQLite, each failure pinned by its message: 1,249 tests, 50 pinned (Grammar 13 of
137, Essential 27 of 345, Relation 2 of 469, Standard 2 of 205, Unclassified 6 of 93), the census divergence 0 in
every suite (no ceiling raised); 49 after §17's change (a Float's text written once: the mixed sort's statement,
1.6 MB, fits the 1 MB cap at 258 KB). Until this lane SQLite was a lexicon row over the base renderer; that row
failed 985 tests on its first run.

What the 50 are: 34 are the platform's own, pinned on the DuckDB lane by the same cause. The other 16 are SQLite's own
limits, each measured:

- no digest function, so no MD5, SHA-1 or SHA-256 (3);
- an integer past 64 bits (3), raised rather than delivered as a REAL;
- no hash function (2, as on Postgres);
- no session user (2);
- no Jaro-Winkler function (2, as on Postgres and the clouds);
- a date past the years 0000-9999 (2): one is raised, and the other is a five-digit year that compares as text;
- no time zone but UTC (1);
- the 1,000,000-byte statement cap (1): a mixed list's sort compares each Float by its text canon, and each capture
  group's regular-expression walk is written separately.

(Later: today's generated matrix, `docs/dialect-capability/`, shows MD5, SHA-1, SHA-256 and Jaro-Winkler emulated on
`SQLite`.)

The driver is xerial `sqlite-jdbc` 3.47.1.0 (`@sqlite_jdbc`, already a dependency; its own build of SQLite 3.47.1, with
the math functions and the driver's statistics extension). Each executed expression gets a fresh in-memory database
(`jdbc:sqlite::memory:`), and the session runs `PRAGMA case_sensitive_like = ON`.

The dialect is `core/.../sql/dialect/SQLite.java`, with an exhaustive decision per `SqlFn` and `SqlAgg.Fn`;
regular expressions are `SqliteRegex.java` and the statement-cap pass `CommonSubexpressions.java`.

What differs from DuckDB, and the dialect's answer:

| Measured | Consequence |
|---|---|
| a value has no declared type, only a storage class (INTEGER, REAL, TEXT, BLOB, NULL); a CAST to a type name casts by its affinity only (`CAST('2024-01-01' AS DATE)` is 2024); the driver names a column by its first value's class (`NUMERIC` for a NULL) | every cast is decided by the dialect: a boolean is 1 or 0, a date and a date-time are their ISO text; the census reads INTEGER as delivering Boolean and HUGEINT labels, TEXT as delivering temporal, JSON, composite and decimal ones, FLOAT as DOUBLE, and a NULL first value as claiming no type |
| no exact decimal | a decimal is a REAL; a root decimal column is its text at its scale (`printf('%.Nf')`), read back exact (`normalize`) |
| no array, struct or map type | JSON1 text (`json_array`, `json_object`); a list operation is a correlated subquery over `json_each(xs)`, and a fold is a correlated `WITH RECURSIVE` (both measured to correlate inside a scalar subquery); a nested value that leaves a JSON function through a column is put back with `json()` |
| no LATERAL, no VALUES alias list, no QUALIFY, no ORDER BY or LIMIT on a UNION branch, no quantified comparison | a lateral subquery's rows are `json_each` over those rows aggregated as JSON objects; the explosions are markers (`ExplosionMarkers`) joined to `json_each` over the longest list; VALUES columns are renamed through a subselect; QUALIFY is a wrapping subselect; a union branch is wrapped |
| no regular-expression function (REGEXP is application-defined, and none is registered: the database executes, never the host) | a literal pattern is compiled at render time to its automaton's transition table, as `VALUES` rows; a recursive query walks the table over the text (`SqliteRegex`: matches, extract, extract-all, replace); a pattern that is not a literal is a wall |
| no base64, no Levenshtein | recursive walks: base64 over the text's hex bytes three at a time (`unhex` back); Levenshtein over the edit-distance table, a row at a time as a JSON array |
| no RAISE outside a trigger | a bad JSON path's error carries its text (`json_extract('{}', <message>)`), so a message is raised as one; a quote travels as U+001D (`RaisedErrors.QUOTE_MARK`), since SQLite doubles it in the error |
| a statement is at most 1,000,000 bytes (`SQLITE_TOOBIG`) | a repeated large subexpression is written once, as a column of a one-row derived table (`CommonSubexpressions`); a repeating function's large operands are bound once; a float's shortest text reads its value once |
| integer arithmetic overflows to a REAL silently (`9223372036854775807 + 3`) | a big integer's root column raises past 64 bits |
| dates are texts of the years 0000-9999, and a date function answers NULL past them; a date with an unpadded month (`2014-2-27`) is NULL | adding an interval raises past them; a parsed or cast date's fields are padded first; a month past the month's end is clamped (`'floor'`, SQLite 3.46+) |
| LIKE is case-insensitive | `PRAGMA case_sensitive_like = ON`, the dialect's session setup |
| the statistics extension's aggregates (stdev, variance, median, mode) are not window functions | a windowed variance, deviation, covariance or correlation is computed from window sums |
| an aggregate inside a table-valued function's argument in a subquery is "misuse of aggregate function" | argMax and argMin are `json_group_array`'s own `ORDER BY` (SQLite 3.44+) |
| `acos`/`asin` outside [-1, 1] answer NULL (DuckDB raises) | raised |
| the platform's schema-qualified tables (an `ATTACH`ed database is held by its connection: "database already in use") | a schema's table is one table named `"schema.table"` |

The lanes: `//pct:pct_sqlite_<suite>` and `//pct:pct_sqlite`, run from a desk with `bazel test //pct:pct_sqlite`.
They are tagged `manual`, in no per-push lane: they sit in `//gates:heavy`, beside the three cloud lanes, which were
in no lane of `gates/BUILD.bazel` until now (CI's lane guard would have refused them). A per-push lane for SQLite is
the user's call.

> Cut in the 2026-11 docs cleanup: §12's file-by-file list of the work and its burn-down history (2026-10-11). Full text: `git show de2745351:docs/CLOUD_BACKENDS_PCT_2026_10_11.md`.

## 13. MariaDB, measured (2026-10-11)

The second of the open ones. No account and no Docker: a real MariaDB 11.4.13 server that Bazel unpacks
from MariaDB's own Linux tarball (`@embedded_mariadb`, `tools/mariadb/`, the server and its bootstrap scripts only),
started once per test JVM on a free loopback port with its data in the test's temporary directory
(`testing/.../EmbeddedMariaDb.java`). The driver is MariaDB Connector/J 3.5.10 (`@mariadb_jdbc`, LGPL, test-only).

**Result:** all five suites pass on MariaDB, each failure pinned by its message: 1,249 tests, 59 pinned (Essential 31
of 345, Grammar 10 of 137, Relation 13 of 469, Standard 3 of 205, Unclassified 2 of 93), the census divergence 0 in
every suite (no ceiling raised); 51 after §17's changes (a Float's text written once: the five statement-memory pins,
the mixed sort and four `format`s, whose statements now fit; and a windowed mode counted per value, 3).

What the 59 are: 34 are the platform's own, pinned on the DuckDB lane by the same cause (7 of them instance
identity). The other 25 are MariaDB's own limits, each measured:

- GROUP_CONCAT, JSON_ARRAYAGG and a mode are no window functions (14): a windowed string aggregate (11) or mode (3);
- a statement's memory (5): a Float's text canon written out in each of `format`'s arguments, or in each comparison
  of a mixed list's sort, passes the server's 2 GB session cap or its 16 MB statement text;
- no Jaro-Winkler function (2, as on SQLite, Postgres and the clouds);
- a date past the years 1000-9999 (2);
- no named time zone without the server's zone tables (1);
- a recursive CTE takes no outer reference (1): a fold over a row's list.

The dialect is `core/.../sql/dialect/MariaDb.java`, with an exhaustive decision per `SqlFn` and `SqlAgg.Fn`;
`MariaDB` and `MySQL` are lite extensions of `ConnectionDefinition.DatabaseType` (`docs/SEMANTICS_REGISTER.md` S48).

What differs from DuckDB, and the dialect's answer:

| Measured | Consequence |
|---|---|
| the session decides the dialect | the dialect's session setup: `ANSI_QUOTES,PIPES_AS_CONCAT,NO_BACKSLASH_ESCAPES` and strict mode, the zone UTC, utf8mb4 with its binary collation (case- and accent-sensitive comparison and LIKE), `group_concat_max_len` at 1 GB, `max_recursive_iterations` raised, 30 decimal digits for a division, and `condition_pushdown_for_derived` off (on, a JSON filter pushed into a UNION derived table returned no rows) |
| the binary collation still pads (`' ' = ''` is true); the JSON functions answer in that collation, so a NO PAD one mixes with theirs ("Illegal mix of collations") | an empty text is tested by its length (`CHAR_LENGTH`) |
| no array, struct or map type | JSON text: a list operation is a correlated subquery over `JSON_TABLE(xs, '$[*]' ...)` (a table function's argument correlates, measured) |
| an outer reference inside a derived table or a recursive CTE fails ("Unknown column") | a LATERAL subquery's rows are inlined as `JSON_TABLE` over those rows aggregated as JSON objects; the explosions are markers joined to `JSON_TABLE` over the list; a fold, a `list_reduce` and an edit distance are a recursive CTE only over values that read no row, with a typed anchor (a CTE's column takes its first row's type, and a literal's is too narrow to grow: `''` is CHAR(0), `0` an INT) |
| no LATERAL, FULL OUTER JOIN, QUALIFY, NULLS FIRST/LAST, FILTER, VALUES alias list | FULL OUTER is emulated (`Caps.MARIADB`), QUALIFY a wrapping subselect, a sort key's NULL order an `IS NULL` key, VALUES a UNION ALL of selects; `<=>` is the null-safe comparison |
| a boolean is 1 or 0; `1.5` is a DECIMAL and `1.5E0` a DOUBLE; a JSON boolean's `JSON_VALUE` is `'1'` or `'0'` | a Float literal carries an exponent; a root Float column is cast to DOUBLE; a JSON boolean is read as 1/0 |
| a SELECT raises no error of its own text: SIGNAL runs only in a stored program, a conversion is a warning, and an XPath error is raised when the statement is PREPARED, whether or not its branch runs | a raise sets the session variable `@legend_raise` and fails at run time on a fixed error (a scalar subquery of two rows); after a failure, `Sessions.raisedMessage` reads the variable and clears it (one read, registered in the SQL-text and accessor ratchets with their reasons) |
| the driver parses a backslash before a quote as an escape, whatever the session's mode ("unknown escape sequence", or a socket error) | a string literal with a backslash before a quote spells it `CHAR(92 USING utf8mb4)` |
| a DATE or DATETIME is of the years 1000-9999 | adding an interval raises past them |
| a double's ROUND is half to even; CAST of a double to an integer rounds half to even | DuckDB's half away from zero is written out |
| a JSON_TABLE column of an empty or NULL list, and `JSON_VALUE` or `REPEAT` of a NULL, are text or binary | an element read of a list that claims no type, and a root value that claims none, are cast to their label |
| a deeply nested correlated JSON_TABLE was planned past 11 GB (the OOM killer ended the server; a common-subexpression pass made it so) | no such pass; the server caps a session at 2 GB (`--max-session-mem-used`), so a statement that big is an error; the 16 MB statement text is kept (at 1 GB a 24 MB statement's parse went past 4 GB, and a grouped `JSON_ARRAYAGG ... LIMIT` answered NULL) |
| a windowed variance or standard deviation is a running recurrence over the partition's rows in the order the window's sort leaves them, which varies (the same statement's z-scores 1.5 in one run, 1.5000000000000002 in another) | an integer's is computed from its exact DECIMAL sums over the window, the same in any order |
| a test compares the database's user with the JVM's (`currentUserId()`), and a container runs as root | the server's own account is `legend`, made by the server's init file, never `root` |

The lanes run from a desk with `bazel test //pct:pct_mariadb`. Linux x86_64 only (the tarball's platform): on another
platform the lanes are incompatible, not failing (`EMBEDDED_MARIADB_COMPATIBLE`). **MySQL** followed (§17): its 8.4 server
tarball, a sibling dialect where the two differ (its JSON functions and regular expressions).

> Cut in the 2026-11 docs cleanup: §13's file-by-file list of the work and its burn-down history (2026-10-11). Full text: `git show de2745351:docs/CLOUD_BACKENDS_PCT_2026_10_11.md`.

## 14. The system database on a server engine (2026-10-11)

`Class.all()` (Grammar's `getAll::testBasic`) reads the platform's metamodel store, which lives in the graph's SYSTEM
DATABASE (`exec/SystemDatabase.java`, the user's ruling of 2026-09-02): the metamodel rows in a database of their own,
apart from every user connection, written once per graph, read-only after, of the same engine as the runtime (so each
lane runs the metamodel queries in its own SQL). On DuckDB, H2 and SQLite it is a private in-memory instance. A server
engine has none, so the test was pinned on every server lane ("no in-memory instance of database type ...").

**The design (the user's, 2026-10-11): the same thing over a remote database.**

- **A system connection, configured apart.** The host gives the platform a SYSTEM CONNECTION per engine
  (`ExecuteOptions.withSystemConnections`, a `Sessions.Opener` per `DatabaseType`): the platform holds no server
  credential, and nothing assumes the system connection is the runtime's server. A server engine with none is refused
  by name. The opened session is checked to be the runtime's engine (`Sessions.check`), since the query is planned in
  the runtime's dialect, and takes the dialect's session setup. The PCT harness gives it the lane's own server and
  credentials, as its choice (`PctBackend.systemConnections`, `CloudLane.openSystem`): a NEW session, plus the
  `c.system.property.*` driver properties (§4).
- **Seeded once per graph, every table, as on DuckDB.** All 34 tables, on the graph's first metamodel query. A
  per-table write is about 0.4-1 s on Snowflake and 1-3 s on Databricks (measured), so a graph's seed is under a
  minute on Snowflake; only `testBasic` pays it on the cloud lanes.
- **Session-private tables.** Each table is TEMPORARY, the system session's own: no other session sees it, two graphs
  or two runs never collide, and the server drops it when the session ends, a crash included -- the private
  instance's guarantees on a shared account (the Snowflake account is a shared integration database; nothing
  permanent is created in it). A temporary table takes no schema on most servers, so each is named
  `metamodel_<table>` (`Ddl.systemTable`) and the query reads it through its table renames (`SystemDatabase.Route`,
  applied by the existing MIR rename before render).

**Measured, per engine (what each server needed):**

| Engine | Needed |
|---|---|
| all servers | no key and no length on these read-only copies (their rows are the ones the in-process engines load under both): Databricks refuses a key on a temporary table; MariaDB and MySQL refuse a key past 3,072 bytes (the store's `VARCHAR(1024)` keys in utf8mb4) and a row whose declared lengths pass 65,535 bytes; Snowflake and BigQuery enforce no key |
| MariaDB, MySQL, BigQuery | each non-text cell CAST to its column's type (`RowLoad.casts`): MariaDB and MySQL take no `'true'` into a BOOLEAN, BigQuery no string into an INT64 or a BOOL; the dialect spells its own conversion |
| Snowflake | a temporary table needs the session's current schema, and the session has none: `snowflake.system.property.schema` (a schema the role may create tables in) |
| BigQuery | a temporary table exists only in a session: `bigquery.system.property.EnableSession=1` (the driver takes it as a property); `CREATE TEMP TABLE`, unqualified |
| Databricks | a temporary TABLE is not read inside an `EXISTS` (`UC_COMMAND_NOT_SUPPORTED`, "Data source v2"; a scalar or `IN` subquery reads it): each system table is a `TEMPORARY VIEW` over its rows instead (`SqlDialect.sessionTable`, one statement, 0.2 s), each column cast to its type in the projection; Spark DDL spells an unbounded VARCHAR `STRING`, a BIT `BOOLEAN`, and a nullable column with no `NULL` |
| MySQL | a temporary table cannot be read twice in one query ("Can't reopen table"): `testBasic` reads it once; a self-joining metamodel query would fail there by that message |

**Result:** `testBasic` passes on Snowflake, Databricks, BigQuery, MariaDB and MySQL (each lane's Grammar re-run, every
one green but MySQL's open list); its pin is gone from those lanes and from Postgres. **Postgres is not verified
here**: the embedded Postgres refuses to start as root, and this session runs as root, so its lane runs only in CI.

## 15. A BC date-time on Snowflake and Databricks: the drivers read it wrong

**Overtaken (2026-10-19/21):** both pins are gone. The plan route prints a date-time in the database, which keeps a
BC year's sign (Databricks spells it with `date_format`; see the comments in
`Test_LegendLite_{Databricks,Snowflake}_EssentialFunctions_PCT`). The design below was not built; the driver facts
stand.

**The one failure each cloud had beyond the platform's own** was `testAdjustByMinutesBigNumber`: a date-time in the
year -21457. The database computes it right; the JDBC driver reads it wrong (measured 2026-10-11):

| Read | Snowflake (snowflake-jdbc 4.4.0) | Databricks (databricks-jdbc 3.4.3) |
|---|---|---|
| the value as text, in SQL | `-21457-01-08 20:48:00` (`TO_VARCHAR`) | `-21457-01-08 20:48:00` (`CAST(.. AS STRING)`) |
| `getString` | `21458-06-30 17:39:38.000` | the year's sign dropped (`21458-01-08 20:48:00`) |
| `getTimestamp` | epoch -21457-01-18T17:39:38Z (ten days and three hours off) | the same text, sign dropped |
| `getObject(i, LocalDateTime.class)` | refused (a plain `SQLException`) | the `Timestamp`'s text |

So no read of a TIMESTAMP cell can recover it: the executor's BC-safe re-fetch (`Executor.fetch`, the `java.time`
carrier) has nothing faithful to re-fetch on these drivers. The value survives only as text written by the database.

**The design: on these two dialects, a result's TIMESTAMP column is delivered as its text**, which the executor
already decodes -- a temporal-labelled VARCHAR cell is the precision-faithful string convention and parses
(`Executor.unwrap`: `PureDateLiteral.parse`), the label unchanged. The text must be what the driver path would have
produced for an AD value, so nothing else moves: `PureDateLiteral.fromLocalDateTime` writes no fraction when the
nanoseconds are zero, else the fraction with its trailing zeros stripped. Measured, both warehouses write exactly that:

- Databricks: `CAST(ts AS STRING)` -- `2016-02-29 01:02:03`, `2016-02-29 01:02:03.456`, `-21457-01-08 20:48:00`;
- Snowflake: `RTRIM(RTRIM(TO_VARCHAR(ts, 'YYYY-MM-DD HH24:MI:SS.FF9'), '0'), '.')` -- the same three, and
  `0010-01-01 00:00:00` (a four-digit year kept).

> Cut in the 2026-11 docs cleanup: the rest of §15, where the design would have gone and what to check when built (2026-10-11). Full text: `git show de2745351:docs/CLOUD_BACKENDS_PCT_2026_10_11.md`.

## 16. Channel B on every lane (2026-10-11)

The user's ask: run each backend through channel B too. Channel B (gate 9) is the second referee: our own compiler
runs the PCT sources straight from the pinned legend-pure and legend-engine trees, the test's WHOLE body (its asserts
too) compiled and run in the database, where channel A runs the test in legend-pure's interpreter and sends only the
evaluated expression to us. Until now it ran on DuckDB only.

**What changed:** channel B runs on the lane's own database (`PctBackend`, now public: the same connection, storeless
runtime and system connection channel A's adapter uses), and each lane's verdicts are diffed against THAT lane's
channel A ledger (`Test_LegendLite_<Lane>_<Suite>Functions_PCT`) and, where legend-engine publishes one, its PCT
manifest for the same database (Postgres, H2, Snowflake, Databricks; none for SQLite, MariaDB, MySQL or BigQuery, so
there every wire bug is a true one). DuckDB keeps its own pins in each suite's test; every other lane is judged by
`channelb.ChannelBLane`, by its own measured numbers: PASS may only grow, WIRE-BUG (channel B fails where channel A
passes) and TRUE-WIRE-BUG (and the engine passes too) may only shrink, and the never-happens invariants hold (no label
lie, no dual-verdict disagreement). The targets are `//pct:pct_channel_b_<lane>_<suite>` and `//pct:pct_channel_b_<lane>`,
manual as their channel A lanes, all in `//gates:heavy`.

**Measured (2026-10-11), PASS / WIRE-BUG / TRUE-WIRE-BUG:**

| Lane | Essential (345) | Grammar (137) | Relation (469) | Standard (205) | Unclassified (93) |
|---|---|---|---|---|---|
| DuckDB (reference) | 336 / 0 / 0 | 137 / 0 / 0 | 469 / 0 / 0 | 205 / 0 / 0 | 93 / 0 / 0 |
| H2 | -- | -- | 429 / 14 / 12 | -- | -- |
| Postgres | 285 / 2 / 0 | 128 / 3 / 0 | 464 / 3 / 1 | 186 / 1 / 0 | 83 / 0 / 0 |
| SQLite | 333 / 1 / 1 | 133 / 0 / 0 | 456 / 12 / 12 | 203 / 0 / 0 | 87 / 0 / 0 |
| MariaDB | 330 / 2 / 2 | 135 / 0 / 0 | 440 / 17 / 17 | 204 / 1 / 1 | 91 / 0 / 0 |
| MySQL | 331 / 1 / 1 | 136 / 0 / 0 | 422 / 32 / 32 | 204 / 1 / 1 | 91 / 0 / 0 |
| Databricks | 334 / 2 / 1 | 135 / 2 / 0 | 459 / 10 / 10 | 205 / 0 / 0 | 93 / 0 / 0 |
| Snowflake | 334 / 1 / 0 | 133 / 2 / 0 | 467 / 2 / 2 | 204 / 1 / 0 | 93 / 0 / 0 |
| BigQuery | 330 / 2 / 2 | 135 / 0 / 0 | 455 / 13 / 13 | 205 / 0 / 0 | 91 / 0 / 0 |

(H2's channel A lane runs the Relation suite only, so channel B's does too. SQLite, MariaDB and MySQL as re-measured
with §17's changes.)

**What the disagreements are.** Every one is a test channel A passes because the interpreter evaluates part of its
body in Java, which channel B sends to the database:

- **A Float printed in the query (Postgres, found and fixed the same day):** Postgres writes a whole double as `41`
  where Pure and DuckDB write `41.0`; channel A never saw it (the interpreter re-reads `41` into a Float column and
  prints `41.0`). Postgres's text of a double now keeps its `.0`, as Snowflake's already did (`Postgres.variantAwareCast`):
  Standard's wire bugs 35 -> 1, Relation's 7 -> 3, and channel A's `testSquareRootError` passes (its message reads
  `-1.0` now; the pin is gone).
- **A NULL Float's text on SQLite (found and fixed the same day):** the shortest-digits text read a NULL through
  `printf`, which takes it for 0 and wrote `0.0`; the dual-verdict alarm caught it on 9 empty aggregates (`max`,
  `greatest`, `corr`, `covar*` over no values). A NULL's text is NULL now (`SQLite.SHORTEST`).
- **A regular expression with a non-literal pattern (SQLite 10, MariaDB 9, BigQuery 10):** the tests' own checks
  (`chunk(1)->sort()->joinStrings('')` over a group's text) build the pattern in the query; these dialects translate a
  pattern only when it is a literal. The likely fix: `chunk` with a literal size spells its pattern as one literal.
- **Lists in a test's own check on H2 (14):** H2 has no list type (§ the H2 lane).
- **A pivot's column order (SQLite, MariaDB, Snowflake, BigQuery: 2 each):** the columns come back in another order
  than DuckDB's.
- **Databricks' 10 Relation rows:** the warehouse fails them with no error detail ("no error details from server");
  not yet read statement by statement.
- **MariaDB's `testMixedSortNoComparator`:** its statement (9.8 MB, the Float text expression written out 5,868 times
  for a five-element sort and the assert's element-wise comparison) takes the server past 12 GB while planning, memory
  `--max-session-mem-used` does not count, and the kernel's OOM killer ends it. It failed every later test until the
  embedded MariaDB and MySQL learned to restart a server a statement killed (`EmbeddedMariaDb.shared`, said on stderr
  with the dead server's log), so the test fails alone. Several MariaDB lanes run at once can exceed the machine
  together: run them one at a time (`--local_test_jobs=1`). The statement's size was the bug, fixed the same day
  (§17: a Float's text written once; the statement is 36 KB and the test passes on both channels).

## 17. MySQL, measured (2026-10-11)

The third of the open ones: MySQL 8.4.11, the server Bazel unpacks from MySQL's own Linux tarball
(`@embedded_mysql`, `tools/mysql/`), started per test JVM as MariaDB's is (`testing/.../EmbeddedMySql.java`); the driver
is MariaDB Connector/J, which speaks to MySQL too. The dialect, `MySql`, is `MariaDb` where the two agree and its hooks
where they differ.

**Result:** all five suites pass on MySQL, each failure pinned by its message: 1,249 tests, 54 pinned (Essential 26 of
345, Grammar 10 of 137, Relation 16 of 469, Standard 0 of 205, Unclassified 2 of 93), the census divergence 0 in every
suite (no ceiling raised). 34 are the platform's own, pinned on the DuckDB lane by the same cause (7 of them instance
identity); 20 are MySQL's own limits, each measured:

- GROUP_CONCAT and an ordered JSON_ARRAYAGG are no window functions (11): a windowed string aggregate;
- a date past the years 1000-9999 (3);
- a JSON value is stored parsed, an object's keys sorted (3): the values are right, the printed key order is not;
- no Jaro-Winkler function (2, as on SQLite, MariaDB, Postgres and the clouds);
- no named time zone without the server's zone tables (1).

**The blocker, and the fix for every dialect.** The first full run never finished Essential: the PCT's
`testMixedSortNoComparator` (`[342, 5.0, -2.0, 171, 1]->sort()`) planned for 15 minutes and the suite timed out, so
274 of 345 tests never ran (on MariaDB the same statement took the server past 12 GB, §16). The lowering spelled a
Float's Pure text (the kind-faithful carrier's, `docs/F10_CARRIER_DESIGN.md`; git history: docs/HISTORY.md) by reading the value's text some fifty
times, cheap where a double's text is a CAST (DuckDB: the literal is 8 KB) and multiplied where it is a long expression
(MariaDB and MySQL cannot print a double as Pure does, 1.6 KB each: the bare literal 545 KB, the sort 6.5 MB). Spelling a
literal in Java would need Java to format a double (the parser keeps a Float as a double, not its text) and is what the
charter's Clause 4 refuses (`docs/TENET_CHARTER.md`), so the database still computes every spelling, written once:

- `SqlFn.FLOAT_CANON`: one call per value; the portable spelling moved unchanged to `dialect/FloatCanonText`, expanded
  by the first render pass, so every later pass sees the tree it saw before (DuckDB's SQL is byte-identical);
- MariaDB and MySQL spell it themselves, the value read once through their one-row JSON_TABLE (the literal 545 KB ->
  2.8 KB, the sort 6.5 MB -> 36 KB on MariaDB, 85 KB on MySQL); SQLite and BigQuery bind the double's text once in a
  one-row derived table (SQLite's sort 1.6 MB -> 258 KB, under its 1 MB cap; BigQuery's 436 KB -> 47 KB);
- `CommonSubexpressions` stays on SQLite and BigQuery only. It was turned on for every dialect the same day (8837a71)
  and reverted on 2026-10-12 (the user's call): it bought nothing measured beyond the Float fix above (MariaDB's sort
  36 KB, MySQL's 85 KB, either way), and it broke six Snowflake tests -- a binding is a one-row subquery around the
  WHOLE expression (SQL has no let), so an expression that reads its row became a correlated scalar subquery, which
  Snowflake refuses ("Unsupported subquery type cannot be evaluated") even when the bound node is a constant. Should a
  dialect ever need a constant bound, the uncorrelated form is a one-row table joined in the select's FROM.

> Cut in the 2026-11 docs cleanup: §17's re-measurement record after the revert (2026-10-12, 79ca7e9: every lane green at the numbers above). Full text: `git show de2745351:docs/CLOUD_BACKENDS_PCT_2026_10_11.md`.

**What else differs from MariaDB, and the dialect's answer:**

| Measured | Consequence |
|---|---|
| no aggregate as JSON_TABLE's argument ("Incorrect arguments to JSON_TABLE") | a grouped mode is counted per value, `COUNT(v) OVER (PARTITION BY keys, v)`, then the most counted value, the least on a tie (as BigQuery's); a windowed mode is counted so on MariaDB too (its 3 pins gone) |
| no PERCENTILE_CONT, PERCENTILE_DISC or MEDIAN | a whole partition's median or quantile from each row's rank and the partition's count, the values at the ranks around the position interpolated as the grouped one reads its sorted array (Standard: 0 pins) |
| `YEAR(d)` answers in MySQL's YEAR type, which the driver reads as a date | `CAST(YEAR(d) AS SIGNED)` |
| an integer past a signed BIGINT that fits an unsigned one is a BIGINT UNSIGNED, its arithmetic too | a HUGEINT literal is `CAST(n AS DECIMAL(38,0))` (the census's BIGINT-under-HUGEINT gone) |
| a negative DECIMAL that rounds to zero has no sign | `%.2f` of -0.000553 writes its `-` (`-0.00`, as Java's printf) |
| a JSON value's text puts a space after each `,` and `:` | where a JSON value becomes text, a string-aware replace drops exactly those (as Postgres's); inside JSON_ARRAY or JSON_OBJECT it stays JSON (channel B Relation 59 -> 32) |
| no JSON_COMPACT, ORDER BY or DISTINCT in JSON_ARRAYAGG, negative JSON index; ICU regular expressions | MariaDB's hooks, spelled as MySQL needs them (`MySql`) |

**Channel B on MySQL** (§16): 331 / 1 / 1, 136 / 0 / 0, 422 / 32 / 32, 204 / 1 / 1, 91 / 0 / 0. Relation's 32 are 21
window tests whose rows come back in another order (no ORDER BY, so SQL promises none, and channel A's comparison is
unordered), 9 regular expressions with a non-literal pattern (the `chunk` family), 2 pivots' column order.

The lanes run from a desk with `bazel test //pct:pct_mysql` (`--local_test_jobs` low: each suite starts its own server).
Linux x86_64 only, as MariaDB's.

## 18. Trino, measured (2026-10-12)

The fourth of the open ones: Trino 483, the server tarball Bazel unpacks (`@embedded_trino`,
`tools/trino/`: its `lib` and the memory connector's plugin, nothing else), started per test JVM on the JVM's own Java
25 as Trino's launcher would start it (`testing/.../EmbeddedTrino.java`: the node's environment, id, data directory
and address as system properties, the Vector API module, a 2 GB heap), with the memory connector as its one catalog;
the driver is Trino's own JDBC driver (test-only). The dialect, `Trino`, is a sibling over `AnsiSqlRenderer`, much of
it Databricks' shapes in Trino's spelling; one dialect covers Presto, Starburst and Athena.

**Result:** all five suites pass on Trino, each failure pinned by its message: 1,251 tests, 40 pinned (Essential 25
of 346, Grammar 14 of 138, Relation 1 of 469, Standard 0 of 205, Unclassified 0 of 93), the census divergence 0 in
every suite (no ceiling raised). 34 are the platform's own, pinned on the DuckDB lane by the same cause (7 of them
instance identity); 6 are Trino's own limits, each measured:

- a ROW's fields are typed, and an unset property is a NULL of no type (5): where a sibling row types the field it
  is typed so, but alone it has no type, and were it typed, a ROW holding a NULL compares NULL and `contains`
  refuses one, where Pure's equality of two such instances is true;
- no temporary table (1): no table only one session sees, so the system database (§14) has nowhere to live.

**What Trino does differently, and the dialect's answer** (every row measured on 483):

| Measured | Consequence |
|---|---|
| a bare integer literal is a 32-bit INTEGER, whose arithmetic raises at 2^31; past a BIGINT one is refused | `BIGINT 'n'`; a HUGEINT literal `DECIMAL 'n'`; a decimal literal past 38 digits `DOUBLE '...'` (DuckDB reads one as a DOUBLE) |
| a double's text is always scientific (`1.5E0`) | a double's text is its JSON's, `json_format(CAST(x AS JSON))` (Java's shortest, `1.5`, `2.0`, `1.3421E-8`; NULL stays NULL, where `format('%s', x)` writes `null`), its exponent marked `e` as DuckDB's, which every Float spelling unfolds |
| UNNEST spreads a ROW element over one column per field | each element wrapped in a row of one field; an untyped list (Pure's `[]`) cast to the explosion's declared type |
| lambdas take no index; `nullif` inside a lambda fails in the planner (`Bind cannot be cast to Lambda`) | `zip_with` over the positions; CASE for nullif; a zero divisor `CASE WHEN d = 0 THEN NULL ...` (DuckDB's NULL), which also stops the planner folding `MOD(5, 0)` into its own error in a branch not taken |
| no microsecond unit in `date_add` or `date_diff` | microseconds from the epoch's seconds and the text's fraction, back through `from_unixtime_nanos` |
| `corr` and `covar_*` answer a REAL over anything but DOUBLE; `corr([1, 2], [10, 20])` is 0.9999999999999999 | their arguments DOUBLE; the correlation DuckDB's: `covar_pop / (stddev_pop * stddev_pop)`, windowed too |
| no exact percentile or mode; any aggregate is a window function | the sorted `array_agg` read with interpolation, and `histogram`'s most counted value, grouped or over the frame |
| `listagg` needs a WITHIN GROUP order | a windowed one of no order of its own takes the window's |
| no Jaro-Winkler function | Databricks' exact construction (`reduce`, `sequence`, typed ROWs), DuckDB's bits |
| the driver hands a ROW as its own class (`io.trino.jdbc.Row`), which production code may not reflect on | a statement's ROW column is its JSON text (cast to its named ROW first: an anonymous row's keys are empty), decoded in `normalize`; the census registers STRUCT <- JSON as Trino's carrier |
| a statement that fails before its columns are known opens a ResultSet with none (a date 12,345,678,912 days on) | the census makes no claim on it (adjudicated; a statement that succeeds always reports its columns) |
| `with_timezone` takes only a naive timestamp; a cast refuses ISO 8601's `T`, `Z` and `-0500`; `EST` is no zone | a zoned value is its own instant; text through `from_iso8601_timestamp` (milliseconds); EST, MST and HST their fixed offsets |
| `codepoint('')` refuses the empty string; `repeat` of NULL joins to `''`; `xxhash64` of NULL is NULL and no VARCHAR cast takes an array | Pure's 0; NULL; a list's or row's text its JSON, no value the empty text |
| OFFSET before LIMIT; no QUALIFY, star exclusion, PIVOT or as-of join; a correlated subquery in a join's ON is refused | the order swapped; `QualifyToSubselect`, `StarExceptToColumns`, `PivotAsGroupBy` and `AsOfAsPick` (a LEFT JOIN with the extreme key picked by a window), the last two shared passes |

**Channel B on Trino** (§16, judged against the engine's own Trino manifest): 331 / 1 / 0, 131 / 0 / 0, 463 / 6 / 5,
205 / 0 / 0, 93 / 0 / 0. Essential's 1 wire bug is the engine's frontier; Relation's 5 are two sorts (Trino
discards a subquery's ORDER BY, which SQL allows: no order is promised without one at the top), a pivot's column
order, a struct field of no type, and Trino's own planner refusing an arithmetic chain
("SignatureBinder.iterativeSolve does not converge").

The lanes run from a desk with `bazel test //pct:pct_trino --local_test_jobs=1`: each suite starts its own server
with a 2 GB heap beside a test JVM, and two at once overran this 15 GB container (the kernel's OOM killer took a test
JVM, and its log simply stopped). The coordinator forgets a finished query at once (`query.min-expire-age`,
`query.max-history`): by default it keeps each one's info, its plan included, for 15 minutes, and a suite's
statements filled its heap until it stopped (measured: the server restarted mid-suite, the statement running then
failed). Measured on Linux x86_64; the server is Java's jars, so the one repository serves
every platform (unmeasured elsewhere). The lanes are `manual`, in `//gates:heavy`.

## 19. ClickHouse, measured (2026-10-12)

The fifth of the open ones: ClickHouse 25.8, a long-term release, its static build Bazel unpacks
(`@embedded_clickhouse`, `tools/clickhouse/`: the one self-contained binary, pinned for Linux x86_64 and skipped as
incompatible elsewhere), started per test JVM on free loopback ports with a configuration file of its own
(`testing/.../EmbeddedClickHouse.java`); the driver is ClickHouse's JDBC driver (test-only), which speaks HTTP. The
dialect, `ClickHouse`, is a sibling over `AnsiSqlRenderer` that changes nothing the other dialects render.

**Result:** all five suites pass on ClickHouse, each failure pinned by its message: 1,251 tests, 65 pinned (Essential
29 of 346, Grammar 14 of 138, Relation 18 of 469, Standard 4 of 205, Unclassified 0 of 93), the census divergence 0 in
every suite (no ceiling raised). 34 are the platform's own, pinned on the DuckDB lane by the same cause (7 of them
instance identity); 31 are ClickHouse's own limits, each measured:

- a Tuple's fields are typed and a Tuple is never NULL (10): an unset property is a NULL of no type, and an empty
  untyped list (`Nullable(Nothing)`) has no Tuple to zip or project into;
- a join's ON needs an equality between its two sides (9): an as-of join one besides its inequality, a condition on
  one side alone is refused, and an outer join's condition reading both sides outside an equality is refused under
  `join_use_nulls` (which SQL's NULLs for the missing side need); a correlated subquery is refused as IN's argument;
  there is no LATERAL join (the dialect refuses one by name; an explosion's lateral join is an ARRAY JOIN);
- a RANGE frame's offsets are non-negative integers (4): a Float offset is refused;
- its dates are 1900 to 2299 (3): `Date32` and `DateTime64` clamp a date past either end, silently, so the dialect
  refuses such a literal rather than answer for another date;
- a list is never NULL (2): a JSON null read as a list is the empty list, which `toVariant` writes as `[]` where Pure
  writes `null`;
- a quantile's level is a parameter (1), a constant: a level from a column is refused;
- a statement over the server's `max_query_size` (1), 256 KiB by default, is refused before its own SETTINGS clause is
  read (this one spells many Floats' text);
- no session (1): the driver's HTTP has none, so no table only one session sees, and the system database (§14) has
  nowhere to live.

**What ClickHouse does differently, and the dialect's answer** (every row measured on 25.8.2.29):

| Measured | Consequence |
|---|---|
| SQL's semantics are settings, off by default: an outer join's missing side is its type's default (0, `''`), a bare UNION is refused, an aggregate over no row is 0; the driver's HTTP has no session, so a `SET` does not outlive its statement | every statement carries its own `SETTINGS` (`join_use_nulls`, `union_default_mode`, `cast_keep_nullable`, `session_timezone`), and each aggregate its `OrNull` form |
| a value is NOT NULL unless its type is `Nullable(T)`; `Nullable(Array)` and `Nullable(Tuple)` are refused; `splitByString` and `extractAll` refuse a Nullable text | every cast names the nullable type; the empty collection is the empty array; a list made from text takes the text past NULL (NULL's list is empty, `split_part`'s answer NULL) |
| a literal is typed by its magnitude (`1` is UInt8, and its arithmetic wraps); `abs` of an Int64 is a UInt64 | an integer literal is `toInt64(1)`, a Float literal an exact Decimal (`1.5` alone is a Float64); an integer's `abs` back to Int64 by `accurateCast` (raising where it does not fit, as DuckDB's overflow) |
| no raise with a computed message (`throwIf` takes a constant); a CASE's branch, read through an outer `if`, ran on every row once the planner merged a subquery's expressions into its reader's; a constant `range` step is folded before its guard runs | a failed cast of the message's text raises it, verbatim in the server's message; `query_plan_merge_expressions = 0`; the step `materialize`d |
| no LATERAL; an ARRAY JOIN's column is read as `"t1"."x"` when named `"t1.x"`, but `*` does not include an ARRAY JOIN's columns | an explosion is an ARRAY JOIN (several lists padded to the longest), and a select over one names its columns after its star |
| a RANGE frame's offset is a number only, and none over a DateTime64; a RANGE frame of offsets reaches a NULL key from any value | an interval frame orders by the key's epoch milliseconds; a NULL key is ordered as its type's extreme on the NULLs' side (a value's frame reaches it only from within its offset of Int64's bound) |
| `formatDateTime` has no milliseconds and its `%l` pads; Joda's `SSS` drops the fraction's leading zeros (.001 prints `100`) | a millisecond field is the first three digits of `%f`, the format around it formatted on its own; Joda's letters only for an unpadded 12-hour clock |
| `toTimeZone` keeps the instant and changes only how it prints | the wall clock in the zone is `addSeconds(x, timeZoneOffset(toTimeZone(x, zone)))`, a naive UTC value (DuckDB's, Trino's) |
| `exp`, `log` and `tanh` are approximations; a Decimal modulo of mixed scales is wrong and `moduloOrZero` takes no Decimal | `pow(e(), x)`, a ratio of `log2`, the tanh identity; the operands at one scale, a zero divisor NULL |
| `toString` drops a Decimal's trailing zeros (3.0 is `'3'`) | `toDecimalString` at the value's own scale, read from its type name (a constant) |
| no JSON constructor; `toJSONString` answers SQL NULL for NULL; a JSON value's text keeps the spaces it was written with | an object, an array and an array aggregate written as JSON text, JSON's `null` for NULL |
| `negate` refuses a String or a DateTime, which an ordered collection's descending key needed | a descending key as a key ordered ascending: a date its number negated, a string its bytes negated then 1 (arrays compare element by element) |
| the driver hands a Tuple as an `Object[]` and an Int128 as a `BigInteger` | a Tuple decoded into its named fields in `normalize`; a dynamic pivot's integral key decoded from a `BigInteger` (`DynamicPivot`, the integral `BigDecimal` arm's twin) |

**Channel B on ClickHouse** (§16, judged against the engine's own ClickHouse manifest): 328 / 3 / 1, 131 / 0 / 0,
445 / 7 / 0, 201 / 0 / 0, 93 / 0 / 0. Essential's 1 true wire bug is a date past 2299, outside ClickHouse's dates;
Relation's 7, each the engine's frontier too, are 5 JSON values whose text keeps its spaces (ClickHouse stores a JSON
value's text as written, DuckDB re-writes it) and a pivot's column order twice. Channel B also found what channel A's
interpreter had hidden, fixed the same day: the descending keys above, the JSON constructors, and a list made from
Nullable text (Relation went 389 / 62 / 7 to 445 / 7 / 0).

The lanes were measured with `bazel test //pct:pct_clickhouse --local_test_jobs=1`, as Trino's. Measured on Linux
x86_64, the one platform with a pinned build. The lanes are `manual`, in `//gates:heavy`.

### 19.1 Its own pins burned down (2026-10-12, later the same day)

The user asked for each of ClickHouse's 31 own pins to be worked: why it fails, a workaround probed on the server, then
the fix (`docs/PCT_PIN_WORKAROUNDS_2026_10_12.md` has the probes). **65 pins become 42; ClickHouse's own 31 become
8** (then 38 and 4, below), and the other 34 are the platform's (pinned on DuckDB too). Channel B: 333 / 3 / 1, 136 / 0 / 0, 459 / 7 / 0,
202 / 0 / 0, 93 / 0 / 0. Every fix is in `ClickHouse.java` except two connection settings, and changes no other
dialect's SQL.

| Pins | What was refused | The dialect's answer, measured on 25.8 |
|---:|---|---|
| 2 | an as-of join with no equality | an equality that holds wherever the inequality can: each side's first inequality operand hashed, times zero (`cityHash64(a) * 0 = cityHash64(b) * 0`; a constant key is folded away and refused again) |
| 1 | a join condition on one side alone | for an outer join, the condition becomes a key: `if(cond, 0, 1) = coalesce(cityHash64(<a column of the other side>), 0) * 0` |
| 1 | a correlated subquery as IN's argument | SQL's three-valued IN by correlated EXISTS: true where `EXISTS (.. WHERE w AND c = v)`, else NULL where a row's `c` is NULL or `v` is NULL and a row is there, else false |
| 4 | LATERAL | a lateral subquery that only filters is decorrelated: its correlated conjuncts move into an INNER or LEFT join's ON, its columns read through the subquery's outputs; any other lateral is still refused by name |
| 4 | a RANGE frame with a Float offset | the frame computed over the partition's values: `arrayReduce(agg, arrayMap(.., arrayFilter(<key within the offsets>, groupArray(tuple(arg, key)) OVER (PARTITION BY ..))))`, for one sort key and a one-argument aggregate |
| 1 | a quantile whose level is a column | the partition's values sorted, interpolated at `(n - 1) * level` (a literal level is still `quantileExactInclusive`) |
| 1 | a statement over `max_query_size` | the limit and the AST limits raised on the connection (`clickhouse_setting_max_query_size`, `..._max_ast_elements`, `..._max_expanded_ast_elements`): a statement's own SETTINGS are read only after the limit has refused it |
| 1 | the system database (§14): no session | the system connection opens an HTTP session of its own (`clickhouse_setting_session_id`, a new id per connection, an hour idle at most), and the dialect's session table is a TEMPORARY table in it. Another session does not see it; over a connection with no session the table is gone with its request, and the read after it is refused as an unknown table |
| 4 | an empty list of no type, zipped or exploded | the empty list of the slot's type (`CAST([] AS Array(Tuple(..)))`), unnested or ARRAY-JOINed, where the result's or the exploded column's type is known |
| 4 | a struct field of no type (an unset property) | the field's declared type; an unset list is the empty list; two instances compare as one-element arrays (`[a] = [b]`: a Tuple's `=` answers NULL where a field is NULL on either side, an array's compares NULL-safe) |

One of these leans on `allow_experimental_correlated_subqueries = 1`: the IN rewrite's correlated EXISTS is
experimental on 25.8, and it answers **false for an outer row whose correlated column is NULL, whatever the filter**
(measured: a filter `o.v IS NULL` is false for a NULL `o.v`, and `i.k = o.k OR i.c = o.v` false where `o.v` is NULL).
So the setting is written only into a statement that holds the rewrite, and the rewrite is made only where the filter
is null-rejecting: each outer column a bare operand of a comparison, `v` too. Elsewhere a correlated subquery is
still refused by the server. The LATERAL fix decorrelates into a join, so it needs no setting; it takes a subquery
that projects its own columns only (an aggregate or a window would see the source before the filter). The two connection settings are the test harness's (the product has
no ClickHouse connector yet): the connector the dialect-leak plan's P5 builds carries both.

**Then 4 more, the same day, at the user's ask:**

- an absent instance (2, `testMapInstance`, `testAssignNewInstance`): a struct field that is itself a struct is its
  `Variant(Tuple(...))`, which holds NULL, read through `variantElement` (a NULL instance's fields read NULL). Measured
  first: a list may mix present and absent instances, array equality, `JSONExtract` and `toJSONString` all take it;
- a JSON null read as a list (2, `testVariantColumn_slice`, `..._distinct_removeDuplicates`): the list is NULL exactly
  where its JSON value is null, so where it is written back as JSON through NULL-strict list functions, it is NULL
  (a raising branch kept). `Variant(Array(T))` itself would not do: every list function refuses a Variant (measured).

Channel B then: 333 / 3 / 1, 137 / 0 / 0, 461 / 7 / 0, 202 / 0 / 0, 93 / 0 / 0.

**What stays pinned (4):**

- dates outside 1900 to 2299 (3): `Date32` and `DateTime64` clamp, silently;
- an outer join whose ON reads both sides outside an equality (1): refused under `join_use_nulls`, which the missing
  side's NULLs need for the whole statement.

`testEqualNonPrimitive` now fails as it does on DuckDB ("Assert failed"): it was already counted among the platform's
pins, and only its pinned message changed.

**Found later, outside PCT (2026-10-13, the dialect program's defect 9):** ClickHouse's `concat` is NULL when any
argument is NULL (`clickhouse local` 25.8: `concat('a', NULL, 'b')` is `\N`), where the flat concatenation's contract
skips NULLs (H2, DuckDB, Postgres and SQLite skip them; Trino emulates it), so an operand a left join missed made the
whole text NULL on ClickHouse only. No PCT lane reached it. Today's dialect writes each argument as
`coalesce(toString(x), '')` (`ClickHouse.java`, `asText`).

## 20. The coverage map, and the corpus on every database (2026-10-12)

**The map** (`docs/PRIMITIVE_COVERAGE_2026_10_12.md`, measured, since succeeded by the generated
`docs/dialect-capability/`; git history: docs/HISTORY.md; the recipe `tools/census/README.md` section 3): the
primitive census (`-Dlegend.diagnostics=primitives`) reads each plan the executor sends for the MIR primitives a
dialect must spell, and tallies them by outcome. The relational corpus carries 244 to success on DuckDB; 48 of them
PCT never exercises on any database (graph fetch's JSON constructors, checked graph fetch, a relation's row order,
`BOOL_AND`, `COUNT(DISTINCT)`, `NOW`, a model's raw SQL, a DISTINCT union, ...), and past those each database adds
0 to 4 (Postgres 4, SQLite 3, Trino and ClickHouse 1; H2 104, its relation suite alone). Many of the 48 come only
from a model (mappings, stores, graph fetch, constraints), so the gap closes with the corpus itself: **77 corpus
tests carry all 244** (a greedy cover, `primitive_cover.py`), and run on a database they exercise everything the
corpus needs there.

**So the next build is the corpus harness on any database** -- the 77 on every lane, then the whole corpus on the
clouds. The harness today is DuckDB's and H2's (`spec/.../rcorpus/MinimalCorpus.java`,
`DuckWorkspaces.java`), and fast for reasons that must survive the move (measured: 2,613 tests, about 35 s a pass,
about 4.3 executor round trips a test):

- one compile of the whole world, once per JVM;
- one long-lived database, a cheap workspace per test package (an attached in-memory catalog; about 313 packages),
  the package's tests sharing it in the engine's order;
- each setup's statements, and each effect segment, sent as ONE script; a test's assertions as ONE fused statement;
  bare literals answered without a round trip;
- the H2 referee in-process, consulted only when a SQL-text assert's text differs.

What it does not do yet, and what a cloud's 50-300 ms round trip makes decisive (the design, before code):

1. **Seed the shared fixture once per lane, not once per package.** About 307 of the 313 packages re-seed the same
   ~220-statement fixture today. A package that only reads it, which is most, reads the one seeded copy; a package
   that writes, or adds a setup of its own, gets a copy: Snowflake's zero-copy `CREATE DATABASE w CLONE fixture`,
   Postgres's `CREATE DATABASE w TEMPLATE fixture`, Databricks' `SHALLOW CLONE` per table, BigQuery's
   `CREATE TABLE ... CLONE` per table; the fixture keyed by a hash of its statements, so a second run reuses it.
2. **Packages in parallel**, one workspace each (the harness's per-test state moved off statics, or one JVM per
   shard): a warehouse serves many sessions at once, and the run's length becomes its longest shard.
3. **A session's setup once per connection**, not once per execution (`Execution.dialectOf` re-runs it, and reads
   the product's name and version, on every `executeResolved`).
4. **Every network call counted** (a counting connection: metadata calls, describes, commits, connection opens are
   uncounted today), so each lane's round trips are a number to drive down.
5. **No transactional DDL to lean on**: a writing test's isolation on a cloud is its own copy (or a fresh copy after
   it fails), not a rollback.

> Cut in the 2026-11 docs cleanup: §20's survey of where each harness piece lives in the Java test code, with line numbers at 7afb645 (2026-10-12). Full text: `git show de2745351:docs/CLOUD_BACKENDS_PCT_2026_10_11.md`.

Postgres first (local, a template database is native, and its PCT lane is green), then the 77 on every lane, then
the whole corpus on Snowflake, Databricks and BigQuery.

**Postgres, the first step (measured 2026-10-12).** `//spec:corpus_on_postgres` runs the corpus's host pass on
the embedded Postgres, a database per package (`spec/.../rcorpus/ServerWorkspaces.java`; `MinimalCorpus` names the
lane, `-Drcorpus.backend=postgres`; `-Drcorpus.tests=<file>` runs a listed subset; a lane is held to its own fail
roster alone, `MinimalCorpusTest.holdToLaneRoster`). The 77 covering tests: **52 pass, 25 fail**, every failure a
Postgres gap PCT never reached -- jsonb collections and variants (the dialect's leg P4), a polymorphic `unknown` a
literal leaves, jsonb delivered as the driver's `PGobject`, a `bit` column read as an integer -- which the next
slices burn down. Two things it took first:

- **The corpus's raw H2 text, for Postgres** (`RawSqlBoundary.Target`, chosen by `SqlDialect.rawTarget()`, the one
  call site unchanged): DuckDB's flavor with Postgres's type names (`DOUBLE PRECISION`, no `TINYINT`), its bare
  names quoted as written.
- **Folding a store's bare names on Postgres -- tried, then reverted the same day.**
  - *What was tried, for the 52 above.* A store's bare names were spelled in lower case (`addresstable.id`), the
    engine's unquoted convention.
  - *Why it was reverted.* The PR's CI (neema2/legend-lite#30) showed two tests that rely on Postgres keeping a
    store's names as declared and quoted (`"EMP"."firstName"`): `#SQL{...}#`'s Postgres fixture, which creates its
    table with quoted mixed-case names, and DataCube's `infer.test.ts`.
  - *Where it stands (2026-10-14).* Decided: fold (decision D8 of the dialect program, `docs/DIALECT_LEAK_CENSUS_AND_PLAN_2026_10_12.md`,
    Phase 1d; git history: docs/HISTORY.md), with the review's two prerequisites and the two kinds of test following the rule. The 77 re-measured
    on the branch: **53 pass** with folding (1447d42), **0** without it (821398a: every test stops in the shared
    setup, `relation "addresstable" does not exist`, the corpus's bare setup text against the quoted spelling). The
    24 left are Postgres gaps, not names: jsonb delivered as `PGobject` (7, the connector's cells, the plan's Phase
    5), collections and variants over jsonb (5, leg P4), nested checked defects (2), a `bit` column the setup text
    fills with an integer, a list of unknown type, `strptime`, four statements Postgres refuses (an ordering on json,
    a `UNION` of bigint and text, a varchar compared with an integer, a `${GMTtoTZ}` template left in the SQL) and
    three wrong or unjudged answers to trace (one count, one SQL-text row verdict, one referee fault).

> Cut in the 2026-11 docs cleanup: §21, Postgres's own pins burned down (2026-10-12, 109 pins to 43). Its lasting Postgres facts are in [postgres.md](postgres.md); the cause-by-cause table is in `docs/PCT_PIN_WORKAROUNDS_2026_10_12.md`. Full text: `git show de2745351:docs/CLOUD_BACKENDS_PCT_2026_10_11.md`.

Sources: `docs/CLOUD_BACKENDS_PCT_2026_10_11.md` (git history: docs/HISTORY.md); the BC date-time's later fix read in
the Databricks and Snowflake PCT ledgers; §10's and §19's later findings from
`docs/DIALECT_LEAK_CENSUS_AND_PLAN_2026_10_12.md` §3 (defects 10 and 9) and `ClickHouse.java`.
