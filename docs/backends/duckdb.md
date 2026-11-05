# DuckDB: what it offers, and what legend-lite emits

> Written in the 2026-11 docs cleanup. It folds the conclusions of `docs/DUCKDB_FUNCTION_COVERAGE.md` (the
> question "how do we implement every DuckDB function?", measured on DuckDB 1.5.0 on 2026-08-01), which it
> replaces. The raw catalog dump (2,941 rows) and its census tooling were archived with
> `experiments/backend-probes/duckdb-census/` (docs/HISTORY.md). The index of this folder is
> [README.md](README.md).

DuckDB is the **reference database**: the product's default engine, the database every probe in [probes/](probes/)
compared against, and the reference PCT lane (`//pct:pct_duckdb`) whose pins are "the platform's own" on every
other lane. It is not one of the dialects of the generated capability matrix (`docs/dialect-capability/`), which
lists the others against it.

| Where | DuckDB version (2026-11) |
|---|---|
| the product (core's `duckdb_jdbc`) | 1.5.6.0 |
| the warehouse (its own `duckdb_jdbc`, the native library it calls through the C API) | 1.5.5.1 |
| DataCube's tab (DuckDB-WASM 1.33.1-dev57) | v1.5.4 (measured 2026-09-26) |
| the probe studies and the function census below | 1.5.0.0 |

Sources: `tools/deps/jars_table.bzl`; `docs/WAREHOUSE_D1_DESIGN_2026_09_26.md` H1 (git history: docs/HISTORY.md).

## 1. The answer: "every DuckDB function" is the wrong goal

The number that makes it wrong is not DuckDB's 938 function names. It is **vocabulary × N backends.** legend-lite's
SQL IR has a *semantic* function vocabulary (`SqlFn`, 164 constants on 2026-08-01), rendered by an exhaustive
switch in every dialect: adding a function without a spelling, an emulation or a named refusal in every dialect is
a compile error. That is the guarantee, and it is also the cost. Widening the vocabulary to DuckDB's documented
surface would have added ~295 constants, so ~1,770 new render arms across the six backends then planned (H2,
SQLite, Postgres, MariaDB, Snowflake, Databricks; there are more now). And H2's measured capability map said 26% of
the *portable* vocabulary (43 of 163) was already structurally impossible on H2 ([h2.md](h2.md)); a tier chosen
*because DuckDB has it* would approach a 100% impossibility rate there, each one a hand-written refusal.

**The rule that follows:** grow the function vocabulary by **Pure demand**, not by DuckDB's catalog. Each dialect
declares its gaps as typed capability walls (`DialectCapability`). A constant enters the portable core only when a
second backend can render it (`gcd`, `lcm`, `isnan`, `isinf`, `hex` would qualify at once; `damerau_levenshtein`
and `jaccard` probably never). **No raw function-name string may bypass the dialect's mapping.**

That last rule was being broken when the study ran: the aggregate and window paths carried a raw `String fn`
(`SqlAgg.Reducer`/`RankingFn`/`ValueFn`, `Windows.WindowFn.sqlName`) that the base renderer wrote verbatim, with
no dialect mapping, no exhaustiveness and no wall, so `QUANTILE_CONT` rendered as DuckDB's name on any dialect that
did not override it. AGENTS.md invariant 3a forbids it by name. **Closed since:** the aggregates are the typed enum
`SqlAgg.Fn`, which every dialect spells (its javadoc records the old channel as "an undeclared passthrough"), and
the generated matrix lists each dialect's decision per aggregate.

**The line to keep:** a *closed* set of part names that every dialect translates (an `EXTRACT` part, an interval
unit) is fine; an *open* set passed through verbatim is a passthrough.

Sources: `docs/DUCKDB_FUNCTION_COVERAGE.md` §1, §3.1, §4.1, §4.2, §6 (git history: docs/HISTORY.md);
`core/src/main/java/com/legend/sql/SqlAgg.java`.

## 2. What DuckDB offers (1.5.0, default extension set)

Every count was produced by a query against an in-process `duckdb_jdbc-1.5.0.0` database
(`SELECT version()` → `v1.5.0`).

| function_type | overloads | distinct names |
|---|---:|---:|
| `scalar` | 1,458 | 611 |
| `aggregate` | 1,177 | 88 |
| `macro` | 131 | 118 |
| `table` | 125 | 85 |
| `pragma` | 45 | 45 |
| `table_macro` | 4 | 4 |
| **total** | **2,940** | **938** |

- **Overloads are not functions.** Ten aggregate names (`min_by`, `max_by`, `argmax`, `argmin`, `arg_min`,
  `arg_max`, the `_nulls_last` and `_null` variants) account for 916 of the 1,177 aggregate overloads (78%), 92 or
  91 each; `+` has 45. A roadmap counted in overloads measures DuckDB's type matrix.
- 13 names are registered under more than one `function_type` (`range`, `repeat`, `generate_series` are scalar
  and table; `histogram` aggregate and table macro; `version` pragma and scalar).
- `internal` is `true` for all 2,940 rows on a fresh database: it partitions nothing. 2,064 rows (70.2%) carry a
  description.
- **938 is a floor.** 26 extensions are known and 5 loaded (`core_functions`, `icu`, `jemalloc`, `json`,
  `parquet`, all statically linked); `spatial`, `fts`, `inet`, `excel`, `vss`, `iceberg`, `delta` and the scanners
  each add functions on load. "Every DuckDB function" is a per-deployment number.

**How many would a Pure user want?** Filtering the 938 names cumulatively: drop table functions, table macros and
pragmas (sources and settings, not expressions) → 813; drop the 48 `pg_catalog` compatibility shims → 765; drop 19
`__` internals → 746; drop 30 operator-symbol names → 716; drop 133 undocumented `icu_*` internals → 583; require a
description, the catalog's own signal for user-facing → **459**. A stricter filter (every parameter and the return
type Pure-mappable) gives 271, but it drops the whole `list_`/`array_`/`map_`/`struct_` family, 33 of whose
functions lite already implements, so it is a lower bound only. 33 names lite legitimately emits (`row_number`,
`rank`, `lag`, `lead`, `json_object`, `to_json`, …) have an empty description, so **the honest target is ~490
names**.

Sources: `docs/DUCKDB_FUNCTION_COVERAGE.md` §2, §5 and its query ledger (git history: docs/HISTORY.md).

## 3. What legend-lite emitted (2026-08-01)

| Denominator | lite covered | % |
|---|---:|---:|
| all 938 catalog names | 163 | 17.4% |
| **the 459 documented, user-facing scalar and aggregate names** | **131** | **28.5%** |
| the 271 strictly typeable | 98 | 36.2% |

- Every name lite could emit (from the spelling table, the coded render arms, the aggregate and window lists: 164
  candidates) was looked up in `duckdb_functions()`: 163 are catalog functions; the one that is not is
  `coalesce`, which DuckDB implements in its parser.
- **The missing names are mostly not missing capability:** alias duplicates (`argmax`/`argmin`, `datediff`,
  `datepart`, `datesub`, `lcase`, `instr`, `editdist3`), calendar-part conveniences lite reaches through `EXTRACT`
  (`day`, `month`, `hour`, `dayofweek`, `isoyear`, `century`, `decade`, `millennium`, `era`, …), and a group of
  **60–80 genuinely new capabilities** (`damerau_levenshtein`, `jaccard`, `hamming`, `bar`, `hex`, `bin`, `gcd`,
  `lcm`, `isnan`, `isinf`, `nfc_normalize`, `approx_quantile`, `entropy`, `kurtosis`, `mad`). That last group is a
  burndown list, not an architecture problem.
- **The two vocabularies are not 1:1, and must not be forced to be.** 388 distinct Pure native names compressed onto
  164 `SqlFn` constants because the IR is semantic: `EXTRACT` carries the part as a literal and covers
  `year`/`month`/`dayOfWeek`/… as one constant. That compression is the vocabulary's whole value, and a generated
  1:1 registry would destroy it.
- **Lite had no expression-level escape hatch, by design (2026-08):** an unregistered Pure overload is a loud error naming
  its signature, an unknown function a loud error, and the IR has no raw-function node. Arbitrary SQL text runs only
  at the setup/statement boundary (`executeInDb`), never inside an expression.

Sources: `docs/DUCKDB_FUNCTION_COVERAGE.md` §3, §5.1 (git history: docs/HISTORY.md).

## 4. What a catalog can and cannot give a dialect

| Generable from `duckdb_functions()` | Not generable: the contract |
|---|---|
| the name; the arity, varargs included | **the Pure type mapping** (`BIGINT` → `Integer` or `Number`; 44 scalar overloads take `ANY`, 41 return `ANY[]`) |
| parameter and return types | **null semantics** (nothing says `GREATEST` skips NULLs; H2 and SQLite measured otherwise) |
| a description for 70% of rows | **the rounding contract** (Pure pins `ROUND` half-even; DuckDB's `round` is not, hence separate `ROUND_EVEN` and `ROUND_HALF_UP`) |
| one type-valid test call per overload | **empty-collection semantics** (`exists([])` is false, `forAll([])` true) |
| alias groups (70 description groups covering 172 names: 102 redundant aliases) | **the index base** (`SUBSTRING`/`STRPOS` 1-based in the IR) and **which alias is canonical** |

**The catalog generates the signature; a human writes the contract.** Every function bug recorded in the project
(banker's rounding, integer division written `//`, `GREATEST`'s NULL skipping) lived on the contract side. What
generation is worth doing: a **test corpus** (one call per overload) and a **drift gate** (every DuckDB name lite
emits still exists in `duckdb_functions()` with a compatible arity), since DuckDB renames and deprecates between
minor versions and nothing would notice otherwise.

**A DuckDB-only extension tier, if one is ever wanted** (recommended 2026-08-01, not built as far as the tree
shows): one IR node reading a registry row, one generic render arm on DuckDB, a pre-declared wall on every other
dialect, with the declared-gap registry's semantics ([h2.md](h2.md) §9): a call that raises its registered wall on
Postgres is a pass, one that raises a different error *or silently succeeds* a failure. Its contract sentence: an
extension-tier call may name any function in the registry; it may never be the reason a core function was not
added.

**Don't:** count overloads; treat 938 as a fixed target (pin the extension set); auto-generate contracts (938 rows
with the semantics field blank); use `internal` as a filter; let an extension tier absorb core work (the engine's
`PlatformRoutingStrategy` is the cautionary case: the escape hatch becomes the sink for everything the SQL layer
could not do).

Sources: `docs/DUCKDB_FUNCTION_COVERAGE.md` §4.3, §4.4, §6, §7 (git history: docs/HISTORY.md).

## 5. DuckDB behaviors the other dialects reproduce

DuckDB's answers are the reference the other files measure against. The ones that most often need writing out on
another database, each measured where cited:

| DuckDB does | Where it was measured |
|---|---|
| `round` of a double is half away from zero (and `round(x, n)` is `ROUND(x * 10^n) / 10^n`) | [postgres.md](postgres.md), [mariadb.md](mariadb.md) §5, cloud.md §10 (BigQuery), §13 (MariaDB) |
| `date_diff` counts unit-boundary crossings, not elapsed units | [postgres.md](postgres.md), mariadb.md §5, cloud.md §8, §9 |
| `regexp_replace` replaces the first match only; `~` is a full match | [postgres.md](postgres.md), mariadb.md §5, cloud.md §8 |
| `levenshtein` and `jaro_winkler` count UTF-8 bytes, not characters | [postgres.md](postgres.md), cloud.md §9 |
| `GREATEST`/`LEAST` and `concat` skip NULLs | [h2.md](h2.md) §2, mariadb.md §5, sqlite.md §4, cloud.md §8 |
| NULLs sort last both ascending and descending | [postgres.md](postgres.md), mariadb.md §5, sqlite.md §4 |
| `json_group_array` keeps NULL elements | [h2.md](h2.md) §3, sqlite.md §6 |
| `mode` takes the first row scanned on a tie | cloud.md §10 (BigQuery takes the least value instead) |
| `1/0` is `inf` | [postgres.md](postgres.md), [probes/frag-math.md](probes/frag-math.md) §4.4 |
| a list is jagged (`List<List<T>>` is native) | [postgres.md](postgres.md) (Postgres arrays are rectangular) |
| `date_trunc('century', …)` is `year/100*100` | [postgres.md](postgres.md) (Postgres's is calendrically correct) |

**Measured traps in DuckDB itself:**

- **The lambda arrow is deprecated.** DuckDB 1.5.5 warns *"Deprecated lambda arrow (->) … transition to the new
  lambda syntax, i.e., `lambda x: …`, before DuckDB's next release"* (2026-10-01). The DuckDB dialect still writes
  `x -> body` (`rust/lite-dialect/src/duckdb.rs`, its `lambda`).
- **Its JDBC driver 1.5.5.1 loses binder and catalog errors on `Statement.execute`** (a generic "Attempting to
  execute an unsuccessful or closed pending query result"); 1.4.4 and `PreparedStatement` on 1.5.5.1 report them.
  DuckDB's `prepareStatement` runs a script's leading statements and prepares the last.
- **The nanoarrow extension is unusable for a server:** an ENUM, even nested, invalidates the database for every
  connection; UHUGEINT's maximum comes back as −1. DuckDB's own Arrow conversion writes UHUGEINT as
  `decimal128(38,0)` (wrong above 2^127), an ENUM as a dictionary, and tags TIMESTAMP WITH TIME ZONE with the
  session's zone.
- `current_user`, `session_user` and `current_role` are internal macros answering `'duckdb'`; a function cannot be
  registered under those names, and a temporary macro of the same name shadows them.

These last three are the warehouse's findings, in `docs/WAREHOUSE_W1_DESIGN_2026_09_26.md`.

Sources: `docs/POSTGRES_DIALECT_HOMEWORK_2026_10_01.md` Q3; `docs/WAREHOUSE_W1_DESIGN_2026_09_26.md` (W1a);
`docs/WAREHOUSE_FFM_HOMEWORK_2026_09_26.md` §1, §3, §5; the files named in the table (git history:
docs/HISTORY.md).
