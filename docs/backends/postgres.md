# Postgres: how it behaves, measured

> Written in the 2026-11 docs cleanup. It folds the lasting conclusions of
> `docs/POSTGRES_DIALECT_HOMEWORK_2026_10_01.md` (the dialect homework and its first landings, 2026-10-01/02), and
> gathers the Postgres facts measured since on the PCT lane (`docs/CLOUD_BACKENDS_PCT_2026_10_11.md` §16, §20, §21;
> `docs/PCT_PIN_WORKAROUNDS_2026_10_12.md`). The earlier probe study, `docs/POSTGRES_BACKEND.md` (2026-08-06,
> probed on Postgres 18.4), was archived in part 1 of the cleanup (docs/HISTORY.md: "since built"); its "NOT BUILT"
> banner was stale by then. Its evidence survives in the probe write-ups in [probes/](probes/). The index of this
> folder is [README.md](README.md).

## Versions, and where Postgres runs today

| What | Version | Where |
|---|---|---|
| The dialect's floor | **Postgres 16** | `Postgres` (`core/.../sql/dialect/Postgres.java`), "version 16 and later" |
| The PCT lane (gate 7P) | 16.15, embedded from pinned binaries (`@embedded_postgres`, no Docker) | `//pct:pct_postgres`, one target per suite; in a root container it runs as an unprivileged user (`tools/postgres/as_unprivileged.sh`, `docs/GATES.md`): `initdb` refuses root |
| The JDBC driver | pgjdbc 42.7.13 | `tools/deps/jars_table.bzl` |
| The homework's probes | Postgres 17 (Docker), DuckDB 1.5.5 with `postgres_scanner` | 2026-10-01 |
| The probe study | Postgres 18.4 (zonky embedded) against DuckDB 1.5.0 | 2026-08, [probes/](probes/) |

**Why 16 is the floor** (the user, 2026-10-01): it is the version that has everything needed. It adds `ANY_VALUE`
and the standard `JSON_ARRAY`/`JSON_OBJECT`/`JSON_ARRAYAGG`. Nothing needed is 17+ only (`JSON_TABLE` is not
needed). The test lane runs on 16, so a newer feature fails there.

Sources: `docs/POSTGRES_DIALECT_HOMEWORK_2026_10_01.md` Q5, Q7; `docs/GATES.md` (gate 7P); `pct/BUILD.bazel`
(git history: docs/HISTORY.md).

## How planned SQL reaches Postgres

**The product path (decided 2026-10-01):** the browser plans (the WebAssembly planner), the warehouse server runs,
and Postgres executes, with DuckDB's `postgres` extension as the driver. No Postgres extension, no new server. A
query is `SELECT * FROM postgres_query('<catalog>', '<planner SQL>')` over a catalog the warehouse attached
(`ATTACH '…' AS pg (TYPE postgres, READ_ONLY)`) before it locked itself down. Rejected along the way: an Arrow
Flight SQL extension forked to HTTP and `omni_httpd` (both run inside Postgres: a crash restarts the cluster,
loading needs a restart, no TLS, unmaintained), raw libpq in the warehouse (it would rewrite the DuckDB-shaped Arrow
and JSON encoders), and the legend-lite server's JDBC path (the goal is server-free DataCube).

**What DuckDB actually sends** (Postgres `log_statement=all`):

```
BEGIN TRANSACTION ISOLATION LEVEL REPEATABLE READ READ ONLY
COPY (SELECT "c1", "c2" FROM (<planner SQL>) AS __unnamed_subquery ) TO STDOUT (FORMAT "binary")
COMMIT
```

Only the columns the outer query uses travel. Postgres does all the work; DuckDB only decodes rows.

**Measured on that path** (Apple M4; Postgres 17; a 100,001-row × 13-column table; the warehouse on the JVM and as
its native binary):

| Check | Result |
|---|---|
| the extension loads from beside the binary (no download) | yes, both builds |
| full table as Arrow | 0.15 s server-side, 0.46 s end to end (native) |
| full table as JSON | 3.5 s (the JSON encoder, not Postgres) |
| Arrow types (read by pyarrow) | `int64 int32 decimal128(18,6) bool date32 timestamp[us] timestamp[us,UTC] double list<string> string` |
| JSON values | BIGINT and DECIMAL as strings (no 2^53 loss), ISO dates, `timestamptz` in UTC with `Z` |
| the inner `ORDER BY`'s order | kept |
| Postgres's error text | reaches the client (`SQL_BIND`, Postgres's message and caret intact) |
| `ATTACH` after start | refused (the lockdown holds) |
| `statement_timeout` in the DSN | cancels in Postgres |
| a reader without a grant | refused: `no SELECT granted on table function main.main.postgres_query` |

**The JVM path** (pgjdbc), used by the PCT and corpus lanes, runs **the same dialect output**. Only `normalize`
differs: pgjdbc's objects on the JVM, DuckDB-decoded values on the product path. The dialect may not name driver
types (`java.sql.Date`, `PGobject`, `timestamptz`, `SUM(BIGINT)` as `BigDecimal` are decoded as plain Java
objects).

Sources: `docs/POSTGRES_DIALECT_HOMEWORK_2026_10_01.md` Q1, Q4 (git history: docs/HISTORY.md).

## One warehouse binary, two kinds of catalog

The same warehouse serves DuckDB and Postgres catalogs (the user, 2026-10-01): a catalog is DuckDB (`--catalog`)
or Postgres (`--postgres NAME=DSN`, repeatable; `--duckdb-extensions DIR`, in a native image the directory beside
the binary). One process with both, or separate processes with one each, is a deployment choice, not a code fork.

- **Entitlements.** DuckDB's authorizer cannot parse Postgres SQL, so on a Postgres catalog the server wraps the
  client's SQL in `postgres_query`, a **catalog-level grant** (`GRANT USAGE ON CATALOG c TO r`) replaces the
  per-statement check, and **the DSN's Postgres role is the data boundary**: it must be SELECT-only. DuckDB
  prepares the `COPY` as one statement, so the client's SQL cannot break out of it.
- **Cancel and timeout.** Statements are tagged `/* wh:<uuid> */`; a cancel or timeout runs `pg_cancel_backend`
  from a second connection, matching the tag with `strpos` and limited to the catalog's role. Measured (2026-10-02,
  JVM and native, Postgres 17): a `pg_sleep(20)` backend is gone 0.5 s after a cancel; a 1,500 ms timeout returns
  `TIMEOUT` at 1.53 s; 100,001 rows come back as Arrow in 0.22–0.24 s. A tag past `track_activity_query_size`
  cannot be found, so keep `statement_timeout` in the DSN too.
- **`describeOnly`** prepares the query without running it.
- **The catalog's objects** are read from DuckDB's catalog over the attach, with DuckDB's type names, leaving out
  `pg_catalog` and `information_schema`; a reader needs USAGE.
- **Open when written (2026-10-02):** Postgres errors surface as `SQL_BIND`; startup fails while Postgres is down;
  a `timestamptz` column is left out of DataCube's model (DuckDB's catalog mapping declares it TIMESTAMP only with
  a UTC conversion, and a read-only table cannot apply one; it is the usual Postgres timestamp type, so it was the
  next gap).

**DataCube on Postgres, end to end (2026-10-02):** the native warehouse over Postgres 17 with a 250,000-row
`sales.orders` and a view `sales.order_facts`, read through a SELECT-only role. Opening the view loads
250,000 × 12 in 311 ms; grouping by `country` takes 576 ms, with totals equal to `psql`'s; a reader is refused
before `GRANT USAGE ON CATALOG shop TO bob`, then lists and queries. No Snap for a Postgres source: its plan is
Postgres SQL.

The warehouse itself is in `docs/WAREHOUSE_W1_DESIGN_2026_09_26.md`.

Sources: `docs/POSTGRES_DIALECT_HOMEWORK_2026_10_01.md` Q7.5, "Status (2026-10-02)", "DataCube on Postgres, end
to end" (git history: docs/HISTORY.md).

## What Postgres does differently from DuckDB

Every row was measured. "Lane" rows are from the PCT lane on 16.15 (2026-10-12/13), "homework" rows from
Postgres 17 against DuckDB 1.5.5 (2026-10-01), "study" rows from Postgres 18.4 against DuckDB 1.5.0 (2026-08; the
write-up named in brackets).

| Expression or fact | DuckDB | Postgres | The dialect's answer | From |
|---|---|---|---|---|
| `7/2` | 3.5 | 3 | DIVIDE casts both operands; INT_DIVIDE `//` is a syntax error, so `div(a,b)` | homework |
| `1/0` | `inf` | ERROR | Pure's divide-by-zero contract decides which engine is wrong (to adjudicate by a PCT test) | homework |
| `round(2.5::double)`, `round(sal)` with 100.5 | 3, 101 | **2, 100** (half-even on a double) | ROUND_HALF_UP is `round(CAST(x AS numeric)[, n])` | homework; study [frag-math §4.1] |
| `'a_c' like 'a\_c'` | false | **true** | Postgres's default escape is `\`: render an explicit `ESCAPE` everywhere | homework |
| `'abc' ~ 'b'` | false (full match) | **true** (partial) | REGEXP_FULL_MATCH is the anchored `~ ('^(?:' \|\| p \|\| ')$')`; MATCHES is `~` | homework; study [frag-strings §2.2] |
| `regexp_matches(s, p)` | a boolean | a **set-returning** function: in a select list it drops non-matching rows | never inherit DuckDB's `MATCHES` spelling | study [frag-strings §2.1] |
| `ORDER BY x DESC` NULLs | last | **first** | the sort key always spells `NULLS FIRST/LAST` | homework |
| an unquoted `X` | kept | **folded to lowercase** | quote every identifier; a Database declares its tables and columns in their physical case | homework |
| `avg(int)` | double 1.5 | numeric 1.5000… | cast to the delivered type | homework |
| `date_trunc('month', date)` | timestamp | **timestamptz** | cast back to the declared type | homework |
| `date_trunc('century'\|'millennium', …)` of 2021 | 2000-01-01 | **2001-01-01** (calendrically correct) | `make_timestamp((EXTRACT(year FROM x)::int/100)*100,1,1,0,0,0)` | study [frag-temporal §2.2] |
| `date_diff` by unit | counts boundary crossings | `age()` measures elapsed time (wrong on 9 of 15 cases) | a boundary-count `date_diff` | study [frag-temporal §2.4]; homework (P1) |
| `to_char(d, 'Day')` / `'Month'` | — | blank-padded to 9 characters | the `FM` prefix | study [frag-temporal §2.6] |
| BC dates | `make_date(-44,3,15)` is `0045-03-15 (BC)` | `0044-03-15 BC`; year zero is refused | — | study [frag-temporal §2.8, §2.9] |
| a DATE past year 9999 | read right | the server is right, **pgjdbc's `java.sql.Date` mis-parses a 5-digit year** (TIMESTAMP survives) | — | study [frag-temporal §2.9] |
| `QUALIFY`, `GROUP BY ALL`, `EXCLUDE`, `[..]`, `{..}`, `//`, `list_transform` | ok | syntax errors | `QualifyToSubselect`; the column list expanded; `ARRAY[..]`; the JSON carrier | homework |
| `strftime` / `to_char` | only strftime | only to_char | the date format per dialect (`to_char`; DuckDB's `%g` part is `to_char`'s `MS`) | homework; lane |
| `encode(…, 'base64')` | one line | **a `\n` after every 76 characters** | `replace(…, chr(10), '')` | study [frag-strings §2.3] |
| `substr('abcdef', -3)` | `def` | **`abcdef`** (no negative-start wrap; a negative length raises) | `CASE WHEN a<0 THEN right(s,-a) ELSE substr(s,a) END` | study [frag-strings §2.5] |
| `levenshtein('héllo','hello')` | 2 (UTF-8 bytes) | 1 (characters) | — | study [frag-strings §2.9] |
| `ARRAY[ARRAY[1,2],ARRAY[3]]` | a ragged list | **refused**: arrays are rectangular; `cardinality` of a nested array counts every scalar and `unnest` flattens every level | nested lists are never native arrays (next section) | study [frag-collections §2] |
| `max(boolean)` | exists | **does not exist** | `bool_or` / `bool_and` | homework (DataCube run); lane |
| `format()` | printf | `%s`, `%I` and `%L` only | a literal format unrolled into `\|\|` pieces (Snowflake's shape); `lpad` to `greatest(width, length)`, since `lpad` cuts a longer text | lane |
| `unnest(NULL)`, a reduction over an untyped NULL | — | refused ("could not determine polymorphic type"): Postgres types every array | each call over the empty collection spelled as DuckDB answers over a NULL list | lane |
| a STABLE cast (the dialect's raise) under `unnest(...)` | — | **pre-evaluated at plan time** where the planner estimates rows, so an untaken raise fired | the raise carries an empty volatile term, only on a list or map raise (a volatile term everywhere stopped subquery flattening and parallel aggregation) | lane; PR #34 review |
| `sha1` | exists | **no core function in any release** (18.6 and the 2026-10-10 devel docs checked: md5, sha224 to sha512, crc32, crc32c) | kept pinned: `pgcrypto`'s `digest(.., 'sha1')` would need an extension, and the dialect requires none | lane |
| Levenshtein, Jaro-Winkler | built in | no core function; `fuzzystrmatch`'s Levenshtein stops at 255 characters; no extension has a Jaro-Winkler | recursive walks in plain SQL, equal to DuckDB's on 1,585 ASCII pairs (counting characters where DuckDB counts bytes) | lane |
| a JSON object's keys | as written | **`jsonb` reorders them** (shorter first); `json` keeps the text but has no equality | Variants stay `jsonb` (the user's call, 2026-10-12): 3 PCT pins kept | lane |
| the text of a whole double | `41.0` | `41` | the dialect's text of a double keeps its `.0` | lane (channel B, cloud.md §16) |
| a function's arguments | — | at most 100 (`FUNC_MAX_ARGS`, measured 2026-10-18 on a concatenation of a spelled list's texts) | a longer `concat` is a concat of concats of runs of arguments | `Postgres.java`, `AnsiSqlRenderer.concatOf` |
| NUL in text; a timestamp past Postgres's range | — | refused | kept pinned (Postgres's own limits) | lane |

Sources: `docs/POSTGRES_DIALECT_HOMEWORK_2026_10_01.md` Q3, "Status (2026-10-02)", "DataCube on Postgres";
`docs/CLOUD_BACKENDS_PCT_2026_10_11.md` §16, §21; `docs/PCT_PIN_WORKAROUNDS_2026_10_12.md`; the fragments named
(git history: docs/HISTORY.md).

## The collection carrier

**The ruling (2026-10-01):** the collection carrier is `jsonb`, not Postgres's native arrays, because a Pure
`List<List<T>>` is ragged and Postgres arrays are rectangular (three of the study's five nested-array probes were
silent wrong answers, not errors). Every lambda form of DuckDB's list family was measured as a correlated subquery
over `jsonb_array_elements(xs) WITH ORDINALITY t(e, o)`, elements cast to their Pure type (`e::numeric`,
`e #>> '{}'`), results rebuilt with `to_jsonb` and `jsonb_agg(… ORDER BY o)` (Postgres 17):

| DuckDB | Postgres over jsonb | measured |
|---|---|---|
| `list_transform` | `(SELECT coalesce(jsonb_agg(to_jsonb(f(e)) ORDER BY o), '[]') FROM jsonb_array_elements(xs) WITH ORDINALITY t(e,o))` | `[3,1,2]` → `[4,2,3]`; `[]` → `[]` |
| `list_filter` | same with `WHERE p(e)` | ✅ |
| exists / forAll | `EXISTS (… WHERE p)` / `NOT EXISTS (… WHERE NOT p)` | ✅, empty lists included |
| `list_reduce` / fold | correlated `WITH RECURSIVE f(i, acc)` stepping `xs->>i` | order-sensitive `acc*10+x` over `[3,1,2]` = 312 ✅ |
| sort, reverse | `ORDER BY e`, `ORDER BY o DESC` | ✅ |
| distinct (first occurrence) | `GROUP BY e ORDER BY min(o)` | `[3,1,3,2,1]` → `[3,1,2]` ✅ |
| slice / at / size | `WHERE o BETWEEN a AND b`, `xs->i`, `jsonb_array_length` | ✅ |
| concat / contains / zip | `\|\|`, `@>`, join on `o` | ✅ |
| nested `List<List<T>>` | elements are jsonb arrays themselves | sizes `[[1,2],[3]]` → `[2,1]` ✅ (native arrays fail here) |

The cost is SQL volume, not capability: one correlated subquery per list operation.

**As built, newer than the ruling** (gate 7P's tiers, 2026-10-02; the pin burn-down, 2026-10-12): a list of
scalars is a **native array** (the study's "flat-only, depth-1" allowance); structs and non-scalar lists are
`jsonb`, held flat; a map is its entries in order, a `jsonb[]` of `{"key", "value"}` (BigQuery's shape, which the
executor decodes to a map), with DuckDB's semantics (a NULL or repeated key raises; a concatenation keeps each
key's first place with the last value); a fold or a reduction into a list of scalars walks a native array; the LIST
aggregate is `array_agg`. The homework's open point, that these forms return `[]` on a NULL list, was settled by
spelling each call over the empty collection as DuckDB answers over a NULL list (the row above).

Sources: `docs/POSTGRES_DIALECT_HOMEWORK_2026_10_01.md` Q7.3 and its jsonb table; `docs/GATES.md` (gate 7P);
`docs/PCT_PIN_WORKAROUNDS_2026_10_12.md`; [probes/frag-collections.md](probes/frag-collections.md) §2 (git
history: docs/HISTORY.md).

## Rules the product path imposes on the dialect

| Constraint | Why | Rule |
|---|---|---|
| one SELECT, no trailing `;` | it is spliced into `COPY (SELECT … FROM (<sql>) …)` | the product path renders a query only; scripts, DDL and DML never travel it |
| output column names unique | the wrapper selects columns by name | every projection is labelled |
| an unconstrained `numeric` arrives as **DOUBLE** | `sum(numeric)` measured `DOUBLE`; with `::numeric(38,6)`, `DECIMAL(38,6)` | cast decimal aggregates to the declared `Decimal(p,s)` |
| `json`/`jsonb` arrive as `VARCHAR` | measured, both through `postgres_query` and attached tables | DataCube decides by the Pure type; no Arrow tag needed |
| session settings cannot run as statements | the planner has no connection | `TimeZone=UTC` and the like go in the ATTACH DSN (`options=-c TimeZone=UTC`); the dialect's session setup is the JVM lane's only |
| no second round trip | the browser planner has no connection | a dynamic pivot (keys found by a first query) is refused by name on the product path |

**The raise without a UDF:** `error()` is a sentinel-wrapped cast to TIMESTAMPTZ, which is STABLE and so is not
folded when the plan is written (the plan-time case under `unnest` is in the table above).

**First landing, measured (2026-10-02):** 26 of 28 query shapes returned the same rows as DuckDB through
`postgres_query`; the two misses were `mode()` tie-breaks and `stddev_samp` at about 1e-13 relative.

**Structure (decided 2026-10-01):** Postgres is a sibling dialect over the shared base, with its own name tables
seeded only from the base's portable rows, never inheriting a DuckDB-only one (DuckDB's `MATCHES` →
`regexp_matches` row is correct on DuckDB and deletes rows on Postgres). The base split ("the base as ANSI,
DuckDB as a layer") was ordered after Postgres, so that every arm Postgres overrides is a DuckDB arm by
measurement, not by reading.

Sources: `docs/POSTGRES_DIALECT_HOMEWORK_2026_10_01.md` Q1, Q2, Q7.4, "Status (2026-10-02)" (git history:
docs/HISTORY.md).

## The lanes

- **PCT, gate 7P** (`//pct:pct_postgres`): the five suites, each failure pinned by its message
  (`Test_LegendLite_Postgres_*Functions_PCT`). Pins: 267 at first measurement, 112 by 2026-10-11, **43 after the
  2026-10-12 burn-down, 9 of them Postgres's own** (`docs/PCT_PIN_WORKAROUNDS_2026_10_12.md` has the cause-by-cause
  table). Channel B on Postgres is judged against the engine's own Postgres manifest (cloud.md §16).
- **The corpus on Postgres** (`//spec:corpus_on_postgres`, cloud.md §20): a database per package; the 77 tests
  that cover every primitive the corpus sends: 53 pass with a store's bare names folded to lower case (the
  engine's unquoted convention; decision D8). The rest were Postgres gaps PCT never reached (jsonb delivered as
  the driver's `PGobject`, collections and variants over jsonb, a `bit` column the setup text fills with an
  integer, `strptime`, statements Postgres refuses).

Sources: `docs/GATES.md`; `docs/CLOUD_BACKENDS_PCT_2026_10_11.md` §1, §20, §21 (now [cloud.md](cloud.md));
`docs/PCT_PIN_WORKAROUNDS_2026_10_12.md` (git history: docs/HISTORY.md).
