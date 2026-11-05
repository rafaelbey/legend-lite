# Backends: how each database behaves, measured

legend-lite plans one SQL IR and renders it per database, and the database computes every value: what it cannot
express is a named capability wall (`DialectCapability`), never a fallback computed in the host. So each dialect
rests on facts about its database: which spellings exist, which execute and return a *different value* than
DuckDB, what the driver hands back, which session settings change the answers. This folder collects those facts,
**measured, with the database and driver versions they were measured on**, as input for the dialects of the Rust
port and for anyone extending one.

Written in the 2026-11 docs cleanup from the backend studies and lane records it replaces (each file names its
source; git history: docs/HISTORY.md).

## The files

| Database | File | Measured on | Its PCT lane |
|---|---|---|---|
| DuckDB (the reference) | [duckdb.md](duckdb.md): its function catalog against what lite emits, and the vocabulary rule | 1.5.0 (census); the product runs 1.5.6.0, the warehouse 1.5.5.1 | `//pct:pct_duckdb` (gate 6, CI) |
| H2 | [h2.md](h2.md): no LATERAL, no JSON field access before 2.3, the engine's forked H2 | 2.1.214 (the product, the engine's goldens), 2.4.240 (the lane) | `//pct:pct_h2` (gate 7, CI; Relation suite only) |
| Postgres | [postgres.md](postgres.md): the product path through the warehouse, the jsonb carrier, measured divergences | 16.15 (the lane), 17 (the homework), 18.4 (the probe study) | `//pct:pct_postgres` (gate 7P, CI) |
| SQLite | [sqlite.md](sqlite.md): no types, `CAST` never fails, a non-stock driver, `json_each` | 3.47.1 (`sqlite-jdbc` 3.47.1.0) | `//pct:pct_sqlite` (manual) |
| MariaDB, MySQL | [mariadb.md](mariadb.md): the `ANSI_QUOTES` gate, `JSON_TABLE`, silent value divergences | 11.4.5 (the study), 11.4.13 and MySQL 8.4.11 (the lanes) | `//pct:pct_mariadb`, `//pct:pct_mysql` (manual) |
| Databricks, Snowflake, BigQuery, Trino, ClickHouse (and the SQLite, MariaDB and MySQL lanes) | [cloud.md](cloud.md): per database, how its lane runs, what it does differently from DuckDB, and the dialect's answer | real accounts (2026-10-11); Trino 483, ClickHouse 25.8.2.29 | `//pct:pct_<db>` (manual) |
| MongoDB | `docs/MONGODB_PLAN_2026_10_16.md`: MongoDB as a database of legend-lite, the study and the plan | MongoDB 8.0 (the lane; the floor is 7.0) | `//pct:pct_mongodb` (manual) |

Also here:

- [probes/](probes/): the statement-level probe write-ups (2026-07-31/08-01), one per slice of the IR, with the
  exact SQL proven on each database. Read the slice before implementing a dialect.
- `docs/PCT_PIN_WORKAROUNDS_2026_10_12.md`: what each database cannot do on PCT, and the workaround probed for each
  pin (ClickHouse's and Postgres's burn-downs, cause by cause).
- `docs/dialect-capability/`: **the capability matrix, generated** (below). Never edit it by hand.

## How the behavior was measured

Three methods, each with its own evidence. Where documentation and execution disagreed, execution won, every time.

**1. Probes (2026-07-31 to 2026-08-01).** About 3,700 statements, each run on every backend (DuckDB 1.5.0 as the
reference value, SQLite 3.47.1, Postgres 18.4, MariaDB 11.4.5, H2), comparing the first cell's value *and its Java
type*, not just whether it parsed. Each construct was bucketed: **A** native, **C** a rendering override, **B** a
rewrite pass, **D** impossible, a D only after several constructions were tried. The method and the write-ups are
in [probes/](probes/); the conclusions are the per-database files.

**2. The PCT lanes (2026-09 onward).** legend-engine's PCT suites (Essential, Grammar, Relation, Standard,
Unclassified: about 1,250 tests) run on each database through its own dialect. Each failing test is **pinned by
its message** in that lane's ledger (`Test_LegendLite_<Lane>_<Suite>Functions_PCT`): a pinned error is a pass, a
changed message is a failure, and an unexpected pass is a failure too, the engine's own declared-gap semantics
([h2.md](h2.md) §9). A pin the DuckDB reference lane shares is "the platform's own"; the rest are the database's
own limits, each with its reason. Every spelling in a dialect was measured on the database before it was trusted.
**Channel B** runs the same PCT sources through lite's own compiler, the whole test body in the database, and diffs
each lane's verdicts against that lane's channel A ledger and, where legend-engine publishes one, the engine's own
manifest for that database ([cloud.md](cloud.md) §16). A wire census on every suite checks that each column's
delivered type is the one the plan declared (divergence 0 on every lane but Postgres, whose by-design class, a
struct delivered as jsonb, has measured per-suite ceilings; `docs/GATES.md`, gate 7P).

**3. The capability matrix (generated).** `docs/dialect-capability/` lists what each concrete dialect does with
every function and aggregate of the IR (**S** spelled, **E** emulated, **R** refused by name, **–** never reached),
read from the dialects' own tables by `CapabilityMatrix`, plus which arms a PCT case reaches (`reach.tsv`, a
measurement). It replaced the hand-kept capability maps of the earlier studies. Regenerate it with
`bazel run //docs:update_dialect_capability`; re-measure the reach with
`bazel run //core:capability_reach -- docs/dialect-capability/reach.tsv <pct-cases.tsv>...` when the lowering, the
passes or the suites move. DuckDB and H2 are not among its dialects.

## Running a lane

Every lane is a Bazel test target in `pct/BUILD.bazel`, one per suite (`//pct:pct_<lane>_<suite>`, the suite in
lower case) and one for all five (`//pct:pct_<lane>`). `LEGENDLITE_PCT_BACKEND=<lane>` selects the database; the
targets set it.

| Lane | Database, how it runs | Notes |
|---|---|---|
| `duckdb` | in process | the reference; gate 6, in CI |
| `h2` | H2 2.4.240 in process (`@h2_modern`, never beside 2.1.214) | Relation suite only; gate 7, in CI |
| `postgres` | Postgres 16.15 from pinned binaries the test JVM starts (`@embedded_postgres`, no Docker) | gate 7P, in CI. In a root container run it as an unprivileged user: `tools/postgres/as_unprivileged.sh` (`docs/GATES.md`) |
| `sqlite` | in process and in memory, the product's own driver | manual, in `//gates:heavy` |
| `mariadb`, `mysql` | a real server per test JVM, unpacked by Bazel from the vendor's Linux tarball | manual, `//gates:heavy`; Linux x86_64 only (elsewhere incompatible, not failing); run with `--local_test_jobs=1`: each suite starts its own server |
| `trino` | Trino 483's server jars, the memory connector, a 2 GB heap | manual, `//gates:heavy`; `--local_test_jobs=1` (two at once overran a 15 GB container) |
| `clickhouse` | ClickHouse 25.8's static binary | manual, `//gates:heavy`; Linux x86_64; `--local_test_jobs=1` |
| `mongodb` | MongoDB 8.0's server, channel A through the plan path | manual, `//gates:heavy`; `docs/MONGODB_PLAN_2026_10_16.md` §6 |
| `databricks`, `snowflake`, `bigquery` | the real account, credentials from a properties file outside the repository | manual, `external` (never cached), `local`; costs money ([cloud.md](cloud.md) §4, §5) |

```bash
bazel test //pct:pct_sqlite --test_output=errors
bazel test //pct:pct_trino --local_test_jobs=1
LEGENDLITE_PCT_CLOUD_PROPERTIES=$HOME/.legendlite/cloud-pct.properties \
  bazel test //pct:pct_databricks_essential --test_output=errors
```

A run reports each new failure as an `Or add to expected failure: one("<test>", "<message>")` line: the row to pin,
once its reason is known. Channel B on a lane is `//pct:pct_channel_b_<lane>` (and `_<suite>`), manual like its
channel A lane; on DuckDB it is `//pct:pct_channel_b` (gate 9). For a cloud session (a proxy, root, a disk
allowance) see `docs/CLOUD_SESSIONS.md` and [cloud.md](cloud.md) §6.

**Driver versions pinned today** (`tools/deps/jars_table.bzl`): DuckDB JDBC 1.5.6.0 (the warehouse 1.5.5.1), H2
2.1.214 (the lane 2.4.240), pgjdbc 42.7.13, sqlite-jdbc 3.47.1.0, MariaDB Connector/J 3.5.10 (MariaDB and MySQL),
trino-jdbc 483, clickhouse-jdbc 0.10.0, databricks-jdbc 3.4.3, snowflake-jdbc 4.4.0, google-cloud-bigquery-jdbc
1.6.0, the MongoDB driver 5.13.0.

## What recurs across databases

The same few traps account for most of the measured differences; each file has the details.

| Trap | Where measured |
|---|---|
| **Rounding mode depends on the type.** A double rounds half-even on Postgres and MariaDB, half-up on H2; DuckDB rounds half away from zero; Pure pins `ROUND` half-even | [postgres.md](postgres.md), [mariadb.md](mariadb.md) §5, [h2.md](h2.md) §2, [cloud.md](cloud.md) §8, §10 |
| **Identifier case and quoting.** Postgres folds an unquoted name to lower case, Snowflake to upper; MariaDB reads `"name"` as a string without `ANSI_QUOTES`; H2 upper-cases unless told otherwise | postgres.md, cloud.md §9, mariadb.md §4, h2.md §7 |
| **The same name, another function.** `~` is a full match on DuckDB and a partial one on Postgres; `regexp_replace` replaces the first match on DuckDB and every match on MariaDB and Databricks; `regexp_matches` is a set-returning function on Postgres; `date_diff` counts boundaries on DuckDB and whole units elsewhere | postgres.md, mariadb.md §5, cloud.md §8 |
| **Casts that never fail, and integer literals that are too narrow.** SQLite and MariaDB coerce (`CAST('abc' AS INTEGER)` is 0); Databricks without ANSI mode answers NULL; a bare integer literal is 32-bit on Databricks and Trino, a UInt8 on ClickHouse | sqlite.md §4, mariadb.md §4, cloud.md §8, §18, §19 |
| **NULL handling.** Default NULL order differs on every database; `GREATEST` and `concat` skip NULLs on DuckDB and propagate them on SQLite, MariaDB, Databricks and ClickHouse; H2's `JSON_ARRAYAGG` drops NULLs by default | postgres.md, mariadb.md §5, sqlite.md §4, h2.md §3, cloud.md §8, §19 |
| **Session settings decide the semantics.** MariaDB's `sql_mode` and collation, Databricks' `ANSI_MODE`, Snowflake's session parameters, ClickHouse's per-statement `SETTINGS`, the time zone everywhere: set and asserted at connection open | mariadb.md §4, cloud.md §8, §9, §19, h2.md §7 |
| **No native list type, or a rectangular one.** SQLite, MariaDB, BigQuery's lack of a list of lists, Postgres's rectangular arrays: collections ride JSON, exploded by a correlated table function (`json_each`, `JSON_TABLE`, `jsonb_array_elements`, `UNNEST`), which H2 lacks entirely | sqlite.md §3, mariadb.md §3, postgres.md, h2.md §4, cloud.md §10 |
| **The driver reads a right value wrong.** pgjdbc's `java.sql.Date` past year 9999; Snowflake's and Databricks' drivers lose a BC year's sign; H2's and MariaDB's JSON arrive as `byte[]` | postgres.md, cloud.md §15, h2.md §3, mariadb.md §5.1 |
| **Statement size caps.** SQLite 1,000,000 bytes; ClickHouse `max_query_size` 256 KiB; BigQuery's maximum query length; MariaDB's session memory | cloud.md §10, §12, §13, §17, §19 |
| **Range limits on dates.** SQLite 0000–9999, MariaDB and MySQL 1000–9999, BigQuery 1–9999, ClickHouse 1900–2299 (clamped silently, so refused) | cloud.md §10, §12, §13, §17, §19 |

Sources: `docs/H2_BACKEND.md`, `docs/MARIADB_BACKEND.md`, `docs/SQLITE_BACKEND.md`,
`docs/CLOUD_BACKENDS_PCT_2026_10_11.md`, `docs/POSTGRES_DIALECT_HOMEWORK_2026_10_01.md`,
`docs/DUCKDB_FUNCTION_COVERAGE.md`, `experiments/backend-probes/README.md` (git history: docs/HISTORY.md);
`pct/BUILD.bazel`, `gates/BUILD.bazel`, `tools/deps/jars_table.bzl`, `docs/dialect-capability/README.md`.
