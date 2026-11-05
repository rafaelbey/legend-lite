# SQLite: how it behaves, measured

> Written in the 2026-11 docs cleanup from `docs/SQLITE_BACKEND.md` (the 2026-08 feasibility study, probed on
> SQLite 3.47.1 through `sqlite-jdbc` 3.47.1.0), which it replaces. Sections keep their old numbers; a gap is a
> section that was cut. Full original: `git show de2745351:docs/SQLITE_BACKEND.md`. The index of this folder is
> [README.md](README.md).

**Built, 2026-10-11.** The dialect is `SQLite` (`core/.../sql/dialect/SQLite.java`), measured on the five PCT
suites in-process and in memory: [cloud.md](cloud.md) §12 (the lane, its own limits, and what differs from
DuckDB with the dialect's answer to each), §16 (channel B). This file is the earlier probe study: what SQLite
does, measured statement by statement against DuckDB. Its detailed SQL is in the [probes/](probes/) write-ups
(`frag-strings`, `frag-math`, `frag-temporal`, `frag-collections`, `frag-shapes`, `frag-lateral`). Where the lane
measured something newer, a note says so.

**There is no golden-text half:** the engine's corpus has no SQLite goldens, so **row equality is the only
contract**.

**Evidence standard.** The study executed **2,400+ probe statements against a real
`org.xerial:sqlite-jdbc:3.47.1.0`** (SQLite 3.47.1), every probe also run against DuckDB 1.5.0.0 as the reference
value, through one shared harness so the outputs are comparable cell-by-cell. **Value equality was checked, not
just parse success** — most findings below are statements that *execute fine* and return the *wrong answer*.
Where documentation and execution disagreed, execution won. Claims sourced any other way are marked.

---

## 1. Verdict

**Achievable, and materially easier than H2 — but it carries the worst silent-corruption surface of
any backend measured.**

- **238 of 267 constructs are reachable — 89%**, against H2's measured 80%. SQLite is the *weaker*
  engine and the *better* target, because its gaps are honest errors while H2's were structural.
- **H2's fatal wall does not exist here.** H2's D1 ([h2.md](h2.md) §4) — no `LATERAL`, no correlated table
  function — is a fact about H2 alone. SQLite has **correlated row explosion** via `json_each`,
  verified in every position including inside a scalar subquery. The entire `LIST_*` family,
  `UNNEST`, `VARIANT_ELEMENTS` and `RANGE_FN` are reachable by rewrite. §3.
- **The real cost is that SQLite has no types**, only storage-class affinity. `CAST` cannot fail.
  This produces wrong rows *silently*, with no error, on the plainest possible paths. §4 is the
  operative section of this document.
- **The driver is not stock SQLite.** `sqlite-jdbc` 3.47.1.0 compiles in `SQLITE_ENABLE_MATH_FUNCTIONS`
  *and* registers `extension-functions.c` — 21 non-stock functions. That is a dependency on
  driver-private behaviour and mostly a trap. §5.
- **Graph fetch works, in one statement, byte-for-byte identical to DuckDB.** §6.
- **Temporal columns must be declared `TEXT`.** One line; removes an entire corruption class. §4.1.

> Cut in the 2026-11 docs cleanup: the 2026-08 "NOT BUILT" banner and §2, the 267-construct capability-count
> table (the current per-dialect decisions are generated in `docs/dialect-capability/`). Two of its observations,
> kept: not one temporal construct survived at the DuckDB spelling (the slice's most concentrated dialect cost),
> and no aggregate was a D (all 11 window functions, ROWS/RANGE/GROUPS, named windows and `EXCLUDE` work; the
> window wall is the *frame*, §7). Full text: `git show de2745351:docs/SQLITE_BACKEND.md`.

---

## 3. Correlated explosion exists — H2's D1 does not transfer

`FROM t, json_each(t.arr)` accepts a **row-correlated argument**. Verified in every position:
comma-join, `CROSS JOIN`, `LEFT JOIN … ON 1=1`, inside a scalar subquery, and with the argument as an
arbitrary expression over one *or several* outer rows.

```sql
-- correlated json_each inside a scalar subquery — the exact shape H2 cannot express
SELECT (SELECT sum(value) FROM json_each(e.arr)) FROM (SELECT '[10,20,30]' AS arr) e;   --> 60
```

The load-bearing case for graph fetch is that **the outer row must survive an empty collection**:
inner join gives 3 rows, `LEFT JOIN json_each(...) ON 1=1` gives 6 — the three employees with empty
project lists are preserved. So `CROSS_LATERAL` and `LEFT_LATERAL` are **A/C, not D**.

The full round trip — build a carrier column by aggregation, correlate-explode, filter, re-aggregate
— returns the identical value on DuckDB, SQLite and Postgres.

**The only real constraints are value-level, never placement-level:** the argument must be valid
JSON, and `json_each(NULL)` safely yields zero rows. SQLite lacks only the `AS t(c1,c2)` column-alias
list.

**Consequence for sequencing:** H2's collection-carrier redesign ([h2.md](h2.md) §4.1) was **not a
precondition for SQLite**.

> **As built (cloud.md §12):** no array, struct or map type, so lists are JSON1 text (`json_array`,
> `json_object`); a list operation is a correlated subquery over `json_each(xs)`, and a fold a correlated
> `WITH RECURSIVE` (both measured to correlate inside a scalar subquery); a nested value that leaves a JSON
> function through a column is put back with `json()`.

---

## 4. The operative section — SQLite has no types, and `CAST` cannot fail

Every row below **executes without error** and returns a wrong value.

| Expression | DuckDB | **SQLite** | Why it matters |
|---|---|---|---|
| `CAST('2020-01-15' AS DATE)` | `LocalDate:2020-01-15` | **`Integer:2020`** | leading digits parsed, month+day **discarded** |
| `CAST('abc' AS INTEGER)` | error | **`0`** | no cast ever fails |
| `CAST('{"a":1}' AS JSON)` | JSON | **`0`** | |
| `CAST(1 AS DECIMAL)/CAST(3 AS DECIMAL)` | `0.333…` | **`0`** | every DECIMAL is an IEEE double; this one is integer division |
| `7/2` | `3.5` | **`3`** | integer division |
| `sal % 2` (sal=100.5) | `0.5` | **`0.0`** | `%` truncates float operands |
| `greatest(sal, 100)` where `sal IS NULL` | `100` | **`NULL`** | SQLite's `max`/`min` **propagate** NULL; these are the clamps on `SUBSTRING` |
| `round(1.005, 2)` | `1.01` | **`1.0`** | no exact decimal type |
| default NULL sort order | LAST/LAST | **FIRST/LAST** | `dense_rank` sums 9 vs 9 vs 11 across the three |

### 4.1 The one-line fix that removes a whole class

`DATE` / `TIMESTAMP` / `DATETIME` columns carry **NUMERIC affinity** in SQLite. Inserting `'2021'`
stores the integer 2021. Declaring the column `TEXT` instead makes ISO-8601 text sort, compare and
round-trip correctly — **including BC dates**, verified.

> **Temporal columns on the SQLite target are `TEXT`, not `DATE`.** As built (cloud.md §12): every cast is
> decided by the dialect, a boolean is 1 or 0, and a date and a date-time are their ISO text.

### 4.2 What this means for the contract

The project's tenet is *loud walls over wrong rows*. SQLite's design is the opposite: it never
raises, it coerces. So **the wall has to be built in the lowering**, because the backend will not
build one. Two consequences:

- `normalize` (the dialect's decoding of what the driver returns) is not optional on SQLite — it is
  load-bearing for correctness, not cosmetics. §8.
- Any construct whose correctness depends on the backend rejecting bad input must be re-checked
  here. `CAST` is the whole family.

---

## 5. The driver is not stock SQLite

`pragma_function_list` returned **170 functions, 21 of them non-stock** — `sqlite-jdbc` bundles
`extension-functions.c`. Also compiled in: `SQLITE_ENABLE_MATH_FUNCTIONS` (so `sin`/`ln`/`pow`/`sqrt`
are A, not D — only `cbrt` is absent), the full JSON1 surface, the 3.45+ `jsonb_*` family, native
`median()`, and in-aggregate `ORDER BY` (3.44+).

**The 21 extension functions are mostly traps.** Only `reverse` matches DuckDB semantically:

| | trap |
|---|---|
| `padl` | 2-arg, space-only, **never truncates** |
| `charindex` | **arguments reversed** vs `strpos` |
| `rightstr(s,-2)` | fails `SQLITE_NOMEM` |
| `replicate(s,-1)` | throws where `repeat` returns `''` |
| `sign`, `cot` | come from the **driver's** table, not SQLite core — stock `sqlite3` lacks both |

**Recommendation: build on none of them but `reverse`.** Pure-SQL fallbacks were verified for `sign`
and `cot`. Binding the dialect to a driver-private function table would make legend-lite's
correctness depend on which SQLite build the deployment happens to link — exactly the class of
hidden coupling `AGENTS.md` forbids.

The census also **proves absence**, which is the more valuable half: a `COUNT(*)` over
`{regexp, md5, sha1, sha256, base64, uuid, levenshtein, split_part, ascii, chr, lpad, left, right,
repeat, starts_with, greatest}` returned **0**, and `load_extension` fails. **SQLite has no regex
engine at all** — 5 of the 11 string D's.

> **As built (cloud.md §12):** the lane's driver is the same xerial build (`@sqlite_jdbc`, SQLite 3.47.1 with the
> math functions and the driver's statistics extension). REGEXP is application-defined and none is registered
> (the database executes, never the host), so a literal pattern is compiled at render time to its automaton's
> transition table, as `VALUES` rows, and a recursive query walks it over the text (`SqliteRegex`); a pattern
> that is not a literal is a wall. The statistics extension's aggregates (stdev, variance, median, mode) are not
> window functions, so a windowed variance, deviation, covariance or correlation is computed from window sums.

---

## 6. Graph fetch — works, and is byte-identical to DuckDB

The nested object-graph envelope runs in **one statement**, three levels deep, using
`json_group_array`/`json_object`. Verified: nested JSON stays JSON rather than becoming a quoted
string; ordered aggregation with `DESC`; empty collection produces `[]` not NULL.

**SQLite's 3-level output is byte-for-byte identical to DuckDB's.** Both **keep** nulls by default —
the opposite of H2's `ABSENT ON NULL`, which needed an explicit override.

> **Emit `json_*`, never `jsonb_*`, for envelopes.** SQLite's `json` reads back as `String`; `jsonb`
> reads back as **`byte[]`** — the exact failure mode [h2.md](h2.md) §3 recorded for H2.

---

## 7. Where SQLite actually stops

| | Gap | Note |
|---|---|---|
| **S1** | **No `LATERAL`** in any spelling | but `json_each` correlation covers the need (§3); ASOF is B via a correlated scalar subquery, value-verified |
| **S2** | **No `INTERVAL` token at all** | the window *frame* wall. B-form verified: `ORDER BY julianday(hired) RANGE BETWEEN 1 PRECEDING` |
| **S3** | **No regex engine** | 5 string D's; `load_extension` fails |
| **S4** | **No ARRAY type**, no composite/struct, no wide numerics | **10 of SQLite's 11 shape-D's are this one thing** |
| **S5** | No exact decimal | `Decimal` at scale ≥1 is a genuine D (`round(1.005,2)`) |
| **S6** | `INTERSECT ALL` / `EXCEPT ALL` | syntax error at `ALL`; plain forms are A |
| **S7** | `IGNORE NULLS`, aggregate `ORDER BY` inside `OVER`, `DISTINCT` in a windowed aggregate | refused identically by Postgres — the only D-class window rows on either |

`QUALIFY` is absent, and `QualifyToSubselect` already closes it.

**Hard limits for the capability budget:** **64 tables per join**, **500 compound terms**. (The lane met a third:
a statement is at most 1,000,000 bytes, `SQLITE_TOOBIG`; cloud.md §12.)

### 7.1 Two soft D's worth reclaiming

SQLite supports **correlated recursive CTEs inside scalar subqueries** — the primitive H2 lacks.
That was used to build and verify **byte-exact base64 encode/decode** and a **full Levenshtein DP**
(6/6 matching DuckDB). Both are **B, not D**. Where a function is missing but the semantics are
computable, SQLite can usually get there; the D's above are the cases where the *type system*, not
the function set, is the obstacle. (As built: base64 is a recursive walk over the text's hex bytes three at a
time, `unhex` back; Levenshtein a walk over the edit-distance table a row at a time as a JSON array; regular
expressions, S3, the same way (§5's note). The 2026-10 lane pinned the digests (MD5, SHA-1, SHA-256) and Jaro-Winkler
as missing; today's generated matrix shows all four emulated on `SQLite`, `docs/dialect-capability/`.)

---

## 8. What the JDBC driver returns — the `normalize` families

SQLite needs twice what H2 needed. This is the specification:

| `SqlType` | JDBC returns | canonical |
|---|---|---|
| `BOOLEAN` | **`Integer`** 1/0 (every predicate) | `Boolean` |
| `DATE`, `TIMESTAMP` | `String` | date/timestamp |
| `JSON` | `String` (but `jsonb` → `byte[]`) | JSON |
| `DECIMAL` | **`Double`** — scale is gone | `BigDecimal` |
| `HUGEINT` | `Double` / clamped `Long` | 128-bit |
| `COUNT(*)`, `SUM(int)` | `Integer` | `Long` |
| `LENGTH`, `STRPOS` | `Integer` (DuckDB: `Long`) | `Long` |
| per-row type instability | a column can return different Java types row to row | — |

That last row has no H2 analogue and is the reason normalize must be driven by the **declared SQL
type**, never by `instanceof` on the returned object. (The lane adds: the driver names a column by its first
value's class, `NUMERIC` for a NULL; cloud.md §12.)

> Cut in the 2026-11 docs cleanup: §9, the build order (2026-08). Full text:
> `git show de2745351:docs/SQLITE_BACKEND.md`.

---

## 10. What NOT to do

- **Don't build on the driver's extension functions** (§5) except `reverse`. It couples correctness
  to which SQLite build is linked.
- **Don't declare temporal columns `DATE`.** NUMERIC affinity silently destroys the value (§4.1).
- **Don't emit `jsonb_*` in graph-fetch envelopes** — `byte[]` read-back, the H2 failure mode (§6).
- **Don't treat `CAST` as a validating operation.** It never fails; it coerces (§4).
- **Don't assume a missing function is a D.** Correlated recursive CTEs reclaimed base64 and
  Levenshtein (§7.1).
- **Don't port H2's collection-carrier conclusion here.** Different wall, different remedy (§3).

## 11. The plan path on SQLite answers differently (open, 2026-10-16)

When the plan tests first ran on every in-process database (the dialect program's Phase 6, defect 11), SQLite's
plan path differed from its direct path in 31 cases: a list parameter's plan is refused by name ("binds no array":
SQLite binds no array parameter); a Decimal parameter answers `2.500000000000000000` in CSV and `"2.50"`, a string,
in JSON, where the direct path answers `2.50` and `2.5`; a Date or DateTime parameter answers with nine fraction
digits in JSON. Each case is registered with its exact answer or refusal in
`core/src/test/resources/plan-maker/differences.tsv` (at most 31 rows, `PlanMakerTest.MAX_DIFFERENCES`); no lane
runs SQLite's plans.

Sources: `docs/SQLITE_BACKEND.md` (git history: docs/HISTORY.md); the lane's measurements in
`docs/CLOUD_BACKENDS_PCT_2026_10_11.md` §12 (now [cloud.md](cloud.md)); the generated matrix in
`docs/dialect-capability/`; §11 from `docs/DIALECT_LEAK_CENSUS_AND_PLAN_2026_10_12.md` §3 (defect 11) and
`core/src/test/resources/plan-maker/differences.tsv`.
