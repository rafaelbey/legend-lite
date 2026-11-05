# Backend probe write-ups

> Written in the 2026-11 docs cleanup from `experiments/backend-probes/README.md`, which it replaces; the
> write-ups beside this file were moved here from `experiments/backend-probes/fragments/` unchanged. Full original:
> `git show de2745351:experiments/backend-probes/README.md`.

The statement-level evidence behind [../sqlite.md](../sqlite.md), [../mariadb.md](../mariadb.md),
[../postgres.md](../postgres.md) and [../duckdb.md](../duckdb.md) (and behind `BACKEND_PORTABILITY.md`, folded in
the same cleanup; docs/HISTORY.md). Those files assert capability buckets and silent-divergence findings on the
strength of **executed probes, not documentation**. These write-ups make that claim auditable: each covers one
slice of the SQL IR, with the per-function table and **the exact SQL that was proven to work** for every
construct reached by a rewrite. They are more detailed than the per-database files: read the slice you are
implementing before writing a dialect.

**Measured 2026-07-31 / 2026-08-01.** 3,702 probe statements · 12,188 result rows · 5 engines.

## The write-ups

| File | Slice | Engines |
|---|---|---|
| [frag-lateral.md](frag-lateral.md) | correlated row explosion, the linchpin: H2 is the only one of the four that lacks it | DuckDB, SQLite, Postgres, MariaDB |
| [frag-collections.md](frag-collections.md) | collections, maps, variant, lambdas, `UNNEST`; the carrier verdict (Postgres arrays are rectangular); the graph-fetch JSON envelope | DuckDB, SQLite, Postgres |
| [frag-strings.md](frag-strings.md) | strings, regular expressions, hashing, encoding (41 functions); the SQLite driver's extension functions | DuckDB, SQLite, Postgres |
| [frag-math.md](frag-math.md) | math, bitwise, arithmetic, comparison, null (60 constructs); banker's rounding; raising an error | DuckDB, SQLite, Postgres |
| [frag-temporal.md](frag-temporal.md) | temporal functions and types; SQLite has no date type; the date-format parts per dialect | DuckDB, SQLite, Postgres |
| [frag-shapes.md](frag-shapes.md) | query shapes: IR expressions, sources and joins, types, aggregates and windows; the JDBC read-back (`normalize`) table | DuckDB, SQLite, Postgres |
| [frag-mariadb-scalar.md](frag-mariadb-scalar.md) | MariaDB's scalar surface (strings, math, temporal: 111 functions); the session flags it must pin | DuckDB, MariaDB |
| [frag-mariadb-shapes.md](frag-mariadb-shapes.md) | MariaDB's collections, JSON, variant and query shapes; `JSON_TABLE`; the graph-fetch envelope | DuckDB, MariaDB |

## Engines probed

| Backend | Version | How |
|---|---|---|
| DuckDB | 1.5.0.0 | `org.duckdb:duckdb_jdbc`: the **reference value for every probe** |
| SQLite | 3.47.1 | `org.xerial:sqlite-jdbc:3.47.1.0` |
| PostgreSQL | 18.4 | `io.zonky.test:embedded-postgres:2.2.2`: a real server, ~7 s boot, no Docker |
| MariaDB | 11.4.5 | `ch.vorburger.mariaDB4j:mariaDB4j:3.3.1`: a real server, ~7 s boot, no Docker |
| H2 | 2.4.240 | cross-check only; H2's own study is [../h2.md](../h2.md) |

No Docker, no external service, no root. Both embedded servers downloaded a real binary and ran it. (The PCT lanes
that came later run other versions: [../README.md](../README.md).)

## The method

Every probe runs on **every** backend, so results are comparable cell by cell. The runner recorded the first cell
of the first row as `JavaType:value`, or the error message. Two consequences, both deliberate:

- **Values are compared, not just parse success.** Nearly every finding that matters is a statement that executes
  cleanly and returns the *wrong answer*: `regexp_matches` deleting rows on Postgres, `CAST('2020-01-15' AS DATE)`
  → `Integer:2020` on SQLite, `SELECT "name"` returning the string `'name'` on MariaDB. A sweep that only checks for
  exceptions finds none of them.
- **The Java type is recorded.** `SUM` over an integer column returns `BigInteger` on DuckDB, `Long` or `BigDecimal`
  on Postgres, `BigDecimal` on MariaDB and `Integer` on SQLite. That table *is* the specification of each
  dialect's `normalize` (the decoding of what the driver returns).

**Buckets**, as the write-ups use them: **A** native (DuckDB's own spelling, the same value) · **C** a rendering
override only · **B** a rewrite pass · **D** structurally impossible. A `D` was recorded only after several
candidate spellings and constructions were tried and failed; the write-ups record what was attempted.

## Reading them today

- **They cite `H2_BACKEND.md` by section** ("§2" for the buckets, "§4" D1, "§4.1" the carrier, "§4.2" `CREATE
  ALIAS`, "§7" the connection, "§8" the capability budget, "§9" the declared-gap registry, "§10" the outcomes). That
  study is now [../h2.md](../h2.md), with the same section numbers.
- **They name raw files** (`probe/probes-*.tsv`, `probe/out-<backend>-*.tsv`, `scratchpad/probe/...`) and the
  harness (`probe/Probe.java`). The corpora, the raw results and the harness were archived in part 1 of the cleanup
  (docs/HISTORY.md: `experiments/backend-probes/corpora/`, `results/`, `harness/`).
- **They describe the Java renderer of 2026-08** (`Lowerer.java`, `Scalars.java` line numbers, "zero `normalize`
  overrides today"): the SQL and the measured values are what lasts. Since then each database got its own dialect
  and PCT lane ([../cloud.md](../cloud.md), [../postgres.md](../postgres.md)), and the current per-dialect
  decisions are generated in `docs/dialect-capability/`.

## Caveats

- **Denominators differ between studies**: 267 constructs for SQLite and Postgres, 207 for MariaDB, 256 for H2.
  The partitions overlap imperfectly. Percentages are comparable in spirit; **the counts are not subtractable.**
- **Results are point-in-time**, tied to the exact driver and engine versions above. `sqlite-jdbc` in particular
  is *not* stock SQLite: it compiles in `SQLITE_ENABLE_MATH_FUNCTIONS` and registers ~21 `extension-functions.c`
  functions, so a different SQLite build will not reproduce every row.
- **Session configuration changes the answers.** The MariaDB results were taken at the default `sql_mode`;
  `ANSI_QUOTES` alone changes whether quoted identifiers work at all ([../mariadb.md](../mariadb.md) §4).
- **No cloud database is here.** The 2026-07-31 desk research on cloud free tiers (`CLOUD_BACKENDS.md`) had no
  execution behind it, and its open question, whether JDBC works on a Databricks account, is settled: Databricks,
  Snowflake and BigQuery were measured on real accounts ([../cloud.md](../cloud.md) §8–§10).

> Cut in the 2026-11 docs cleanup: the "What is here" rows for `harness/`, `corpora/`, `results/`,
> `duckdb-census/` and `databricks/` (archived; docs/HISTORY.md), the standalone-Maven re-run instructions, and the
> Databricks connectivity test (2026-08). Full text: `git show de2745351:experiments/backend-probes/README.md`.

Sources: `experiments/backend-probes/README.md` (git history: docs/HISTORY.md).
