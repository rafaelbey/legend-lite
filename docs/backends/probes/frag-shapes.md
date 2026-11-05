# QUERY SHAPES — SqlExpr, sources/joins, types, aggregates (SQLite + Postgres)

**Evidence standard.** **462 probe statements**, each executed against all three of **sqlite-jdbc
3.47.1.0** (OK 327 / ERR 135), **PostgreSQL 18.4** (OK 354 / ERR 108, zonky embedded) and **DuckDB
1.5.0** (OK 395 / ERR 67) as the reference value for every row. Probe file
`scratchpad/probe/probes-shapes.tsv`; outputs `out-{duckdb,sqlite,postgres}-shapes.tsv`. Where
documentation and execution disagreed, execution won — twice materially (SQLite FULL OUTER JOIN;
the SQLite JDBC driver's `stdev`/`variance`/`median` UDFs).

Buckets: **A** native (DuckDB's own spelling, same value) · **C** rendering override · **B** rewrite
pass · **D** structurally impossible.

---

## 1. Verdict — the two walls

**SQLite's join wall is LATERAL, not FULL OUTER JOIN.** The bundled 3.47.1.0 executes
`RIGHT OUTER JOIN`, `FULL OUTER JOIN`, `NATURAL JOIN` and `USING(...)` and returns values identical
to DuckDB and Postgres (`JOIN.right.c1` = 5, `JOIN.full.c1` = 6 on all three). The 3.39 threshold is
cleared. `LATERAL` in every form is a syntax error, exactly the H2 D1 wall.

**SQLite's window wall is the frame, not the function.** All 11 window-only functions, ROWS/RANGE/
GROUPS, named `WINDOW` clauses, and `EXCLUDE CURRENT ROW` all work. `RANGE BETWEEN INTERVAL …` does
not exist in any spelling — SQLite has no `INTERVAL` token at all. Postgres clears the interval
frame but only with the **singular, quoted** unit.

**Postgres has no window wall of shape** — every frame kind including
`RANGE BETWEEN INTERVAL '1' DAY PRECEDING` runs. Its three window gaps are all *modifiers*:
`IGNORE NULLS`, aggregate `ORDER BY` inside `OVER`, and `DISTINCT` inside a windowed aggregate — all
three refused with explicit "not implemented for window functions" errors, and all three are the
same on SQLite. Those are the only D-class window rows on either target.

---

## 2. Sources and joins (14 constructs)

| Construct | SQLite | Postgres |
|---|---|---|
| `Table` | **A** | **A** |
| `Dual` (FROM-less SELECT) | **A** — clause omission | **A** — clause omission |
| `Subselect` | **A** — nesting ≥3 verified | **A** |
| `Values` | **C** — no `AS v(x,y)` alias list; wrap in a renaming SELECT over the implicit `column1…columnN`. 600-row VALUES OK | **A** — `(VALUES …) AS v(x,y)`; implicit names also `column1…` |
| `SourceUrl` `data:` | **C** — `SELECT value AS data FROM json_each('<literal>')` | **C** — `SELECT value AS data FROM json_array_elements(CAST('<literal>' AS JSON))` |
| `SourceUrl` `file:` | **D** — no file reader | **D** — `pg_read_file` is superuser-only; do not take that exit |
| `Pivot` (static, `in`-pinned) | **B** — conditional aggregation, **value-verified** (`PV.static.v2` = 190.75 on all three) | **B** — same |
| `Pivot` (dynamic, empty `in`) | **B**, 2-phase — a `SELECT DISTINCT` key probe, then a generated conditional aggregation. `group_concat(DISTINCT dept_id)` gives the key list in one statement | **B**, 2-phase — `string_agg(DISTINCT …)`; `crosstab` needs the `tablefunc` extension and is not installed |
| `Join.INNER` | **A** | **A** |
| `Join.LEFT` | **A** | **A** |
| `Join.RIGHT` | **A** — 3.39+, verified on 3.47.1 | **A** |
| `Join.FULL` | **A** — 3.39+, verified on 3.47.1 | **A** |
| `Join.CROSS` | **A** — both `CROSS JOIN` and comma | **A** |
| `Join.CROSS_LATERAL` | **D** — no `LATERAL` keyword, no correlated FROM item | **A** |
| `Join.LEFT_LATERAL` | **D** — same | **A** — `LEFT JOIN LATERAL (…) ON TRUE` |
| `Join.ASOF_LEFT` | **B** — correlated scalar subquery per projected column (`… ORDER BY key DESC LIMIT 1`); **value-verified** (`B7.asof.*.v` = 11 on all three) | **B** — `LEFT JOIN LATERAL (… ORDER BY key DESC LIMIT 1) ON TRUE`, value-verified |

**Carve-out on the SQLite D's, stated as H2_BACKEND §4 states it.** `CROSS_LATERAL`/`LEFT_LATERAL`
are D *for row explosion*. Where the lateral yields a fixed set of scalars per left row, a correlated
scalar subquery reproduces it exactly (that is the ASOF row above, and `FU.lateral.sqlite.b1`). The
uncorrelated case is an ordinary derived table. **The SQLite lateral wall is correlated *explosion*,
not all correlation** — the same sentence H2 earned.

**Bucket totals — sources/joins:** SQLite A 8 · C 2 · B 2 · D 2. Postgres A 11 · C 1 · B 2 · D 0.

### Set operations (not separately counted; they ride `SqlUnion`)

| | SQLite | Postgres |
|---|---|---|
| `UNION` / `UNION ALL` | A / A | A / A |
| `INTERSECT` | A | A |
| `INTERSECT ALL` | **D** — syntax error at `ALL` | A |
| `EXCEPT` | A | A |
| `EXCEPT ALL` | **D** — syntax error at `ALL` | A |
| `MINUS` | D (nobody has it, DuckDB included) | D |
| `ORDER BY` + `LIMIT` + `OFFSET` on the compound | A — value-verified | A |
| Parenthesized branch `(SELECT … LIMIT 2) UNION ALL (…)` | **C** — wrap each branch as `SELECT * FROM (…) a` | A |
| Compound arity | **500 terms max** (`too many terms in compound select`) | 1000+ OK |

---

## 3. Window functions and frames

| Feature | SQLite | Postgres |
|---|---|---|
| `ROW_NUMBER RANK DENSE_RANK PERCENT_RANK CUME_DIST NTILE` | A | A |
| `LAG LEAD FIRST_VALUE LAST_VALUE NTH_VALUE` (+ offset, + default) | A | A |
| `Bound.UnboundedPreceding / CurrentRow / UnboundedFollowing` | A | A |
| `Bound.Preceding(n)` / `Following(n)` — `ROWS` | A | A |
| `Bound.Preceding(n)` / `Following(n)` — `RANGE`, numeric ORDER BY | A | A |
| `Bound.Preceding(n)` / `Following(n)` — `RANGE`, **date** ORDER BY | (n/a — dates are TEXT) | **D→B** — `RANGE with offset PRECEDING/FOLLOWING is not supported for column type date`; rewrite the sort key to `hired - DATE '1970-01-01'` (**verified**) |
| `Bound.IntervalPreceding/IntervalFollowing` | **D→B** — no `INTERVAL` token in any spelling. Rewrite to a numeric `RANGE` over `julianday(hired)` (**verified**), day-granular only; months/years need a different key | **C** — `INTERVAL '1' DAY` ✔, `INTERVAL '1 day'` ✔, `INTERVAL '1 days'` ✔ · `INTERVAL 1 DAY` ✘, `INTERVAL '1' **DAYS**` ✘, `INTERVAL '1' **MONTHS**` ✘ |
| `Frame.Kind.ROWS` / `RANGE` / `GROUPS` | A / A / A | A / A / A |
| Named window (`WINDOW w AS (…)`, multiple refs) | A | A |
| `EXCLUDE CURRENT ROW` (not in the IR) | A | A |
| `IGNORE NULLS` on lag/lead/first_value | **D→B** | **D→B** — neither `f(x) IGNORE NULLS OVER` nor `f(x IGNORE NULLS)` parses |
| Aggregate `ORDER BY` inside `OVER` | **D→B** — `ORDER BY may not be used with window functions` | **D→B** — `aggregate ORDER BY is not implemented for window functions` |
| `DISTINCT` inside a windowed aggregate | **D→B** — `DISTINCT is not supported for window functions` | **D→B** — `DISTINCT is not implemented for window functions` |
| `FILTER (WHERE …)` on an aggregate | A | A |

**The interval-frame B forms, both verified value-equal to DuckDB:**

```sql
-- SQLite: RANGE BETWEEN INTERVAL '1' DAY PRECEDING AND CURRENT ROW
sum(id) OVER (ORDER BY julianday(hired) RANGE BETWEEN 1 PRECEDING AND CURRENT ROW)
-- Postgres, date sort key: same rewrite, epoch days
sum(id) OVER (ORDER BY hired - DATE '1970-01-01' RANGE BETWEEN 1 PRECEDING AND CURRENT ROW)
```

**The `IGNORE NULLS` B form**, which reproduced DuckDB's `lag(sal IGNORE NULLS)` **exactly** (391.5)
on both targets, where the cheap "sort nulls to the end" trick did **not** (311.5):

```sql
(SELECT e2.sal FROM emp e2 WHERE e2.id < e.id AND e2.sal IS NOT NULL ORDER BY e2.id DESC LIMIT 1)
```

`Windows.java` never emits `IGNORE NULLS` today, so this is a latent row, not a live one.

### QUALIFY — bucket B on both, and the pass already exists

`QUALIFY` is a syntax error on SQLite (`near "rn"`) and on Postgres 18.4 (`syntax error at or near
"rn"`). **`AnsiSqlRenderer.passes()` already installs `QualifyToSubselect` whenever
`supportsQualify()` is false**, and the subselect form it produces returns the same value on both
targets (`QUALIFY.c2` / `B7.qualify.pg` = 1). Nothing new is needed: both dialects inherit the ANSI
default and the row is closed. This is the *opposite* of H2, where native QUALIFY made the pass a
free fallback.

---

## 4. `SqlExpr` (29 subtypes)

`Call` is the `SqlFn` slice (other agents). `PlanParam` is engine-style-only vocabulary and is a
loud error in any executable dialect — n/a on both.

| Subtype | SQLite | Postgres |
|---|---|---|
| `Column` | A | A |
| `Star` (`*`, `t.*`) | A | A |
| `StarExcept` (`t.* EXCLUDE (c)`) | **B** — DuckDB-only syntax; emit the explicit projection list from `SqlSource.outputs()` (verified) | **B** — same. `EXCEPT (c)` is not a spelling anywhere, DuckDB included |
| `StringLit` | A | A |
| `IntLit` | A | A |
| `FloatLit` — renders `CAST(v AS DOUBLE)` hardcoded at `AnsiSqlRenderer.java:294` | A | **C** — `type "double" does not exist`; must be `DOUBLE PRECISION`. The hardcoded literal bypasses `castTypeName`/`TypeNames`, so it is a renderer fix, not a table row |
| `DecimalLit` (bare `1.005`) | **C** — types as REAL; see §6 | A |
| `BoolLit` | A — `TRUE`/`FALSE` parse (3.23+) | A |
| `NullLit` | A | A |
| `DateLit` (`DATE '…'`) | **C** — `no such column: DATE`; emit a bare `'2020-01-15'` string literal | A |
| `TimestampLit` | **C** — same | A |
| `FormatLit` | A | A |
| `ArrayLit` (`[1,2,3]`) | **D** — no array type; the JSON-text carrier is the collection-carrier redesign, not a spelling | **C** — `ARRAY[1,2,3]`; `'{1,2,3}'::int[]` also works |
| `OrderedListAgg` (`list(x ORDER BY y)`) | **C** — `json_group_array(x ORDER BY y)`, returns TEXT | **C** — `array_agg(x ORDER BY y)` |
| `StructLit` (`{'a':1,'b':'x'}`) | **D** | **B** — `ROW(1,'x')` parses but **loses field names** (driver returns `PGobject:(1,x)`). Named fields require a declared composite: `CREATE TYPE st_ab AS (a INTEGER, b VARCHAR)` then `CAST(ROW(…) AS st_ab)` — verified. **This is the §8 `before`-statement seam's first real customer** |
| `StructGet` | **D** | **C** positional — `(ROW(1,'x')).f1`; **B** named — `(CAST(r AS st_ab)).a` |
| `Call` | (SqlFn slice) | (SqlFn slice) |
| `Case` (searched and simple, no-ELSE → NULL) | A | A |
| `Exists` / `NOT EXISTS` | A | A |
| `ScalarSubquery` | A — correlated **4 deep** verified | A — 4 deep verified |
| `WindowCall` | **C** — see §3 | **C** — see §3 |
| `Lambda` | **D** | **D** |
| `Cast` | **C** — per-type routing plus a validity guard; see §6, the largest silent-corruption surface in this report | A |
| `FoldCall` | **B** — `WITH RECURSIVE` verified working | **B** — same |
| `JsonObject` (`json_object('k',v)`) | **A** — identical positional spelling to DuckDB | **C** — `json_build_object('k',v)`, or PG16+ `JSON_OBJECT('k': v)` |
| `JsonArrayAgg` | **C** — `coalesce(json_group_array(…), '[]')`, returns TEXT | **C** — `coalesce(json_agg(… ORDER BY …), CAST('[]' AS JSON))` |
| `PlanParam` | n/a | n/a |
| `Group` | A | A |
| `SqlAgg.Reducer` | A | A |

**Bucket totals — SqlExpr (27 scored, `Call`+`PlanParam` excluded):**
SQLite A 14 · C 7 · B 2 · D 4. Postgres A 16 · C 7 · B 3 · D 1.

---

## 5. Types — and the `SqlDialect.normalize` specification

### 5.1 DDL name and CAST spelling

| `SqlType` | SQLite DDL / CAST | Postgres DDL / CAST |
|---|---|---|
| `BOOLEAN` | `BOOLEAN` accepted (NUMERIC affinity) | `BOOLEAN` |
| `INTEGER` | `INTEGER` | `INTEGER` |
| `BIGINT` | `BIGINT` | `BIGINT` |
| `HUGEINT` | **no 128-bit type** — see 5.3 | **`NUMERIC` bare.** `NUMERIC(38,0)` **overflows** (int128 max needs 39 digits); `HUGEINT`/`INT128` do not exist |
| `DOUBLE` | `DOUBLE PRECISION` (`REAL`, `DOUBLE`, `FLOAT8` all alias) | **`DOUBLE PRECISION` or `FLOAT8` only.** `DOUBLE` is not a type; **`REAL` is 4-byte and returns `Float`** |
| `VARCHAR` | `VARCHAR(n)` / `TEXT` | `VARCHAR(n)` / `TEXT` |
| `DATE` | `DATE` accepted as a name, **stores TEXT**; no `DATE '…'` literal | `DATE` |
| `TIMESTAMP` | name accepted, stores TEXT | `TIMESTAMP` |
| `TIMESTAMPTZ` | name accepted, stores TEXT | `TIMESTAMPTZ` / `TIMESTAMP WITH TIME ZONE` |
| `JSON` | name accepted, stores TEXT; `json1` functions present | `JSON` and `JSONB` both |
| `Decimal(p,s)` | name accepted, **stores REAL** — `typeof(budget)` = `'real'` | `DECIMAL(p,s)` / `NUMERIC(p,s)` |
| `Array(T)` | **none** | `T[]`, `ARRAY[…]` |
| `Map(K,V)` | **none** | **none** — no `MAP`, no `hstore`; JSONB is the only carrier |
| `Struct` | **none** | `CREATE TYPE … AS (…)` + `ROW()`; no inline `STRUCT(…)` type |

### 5.2 Returned Java type — the `normalize` table (**the deliverable**)

Every cell measured, not inferred. **Bold = a `SqlDialect.normalize` override is required.**

| Value produced by | DuckDB 1.5 (incumbent) | SQLite 3.47.1 | Postgres 18.4 |
|---|---|---|---|
| `BOOLEAN` column | `Boolean` | **`Integer`** (0/1) | `Boolean` |
| `TRUE` literal, `a = b`, `IS NULL`, `IS NOT DISTINCT FROM` | `Boolean` | **`Integer`** | `Boolean` |
| `INTEGER` column | `Integer` | `Integer` | `Integer` |
| `BIGINT` column | `Long` | `Long` | `Long` |
| `HUGEINT` column | `BigInteger` | **`Double`** (precision destroyed) | (n/a) |
| `CAST(… AS HUGEINT)` | `BigInteger` | **`Long`, silently clamped to 2⁶³−1** | (type absent) |
| `NUMERIC` (unconstrained) | — | — | `BigDecimal` ✔ full int128 range |
| `DOUBLE PRECISION` column | `Double` | `Double` | `Double` |
| `REAL` cast | `Float` | **`Double`** | `Float` |
| `VARCHAR` column | `String` | `String` | `String` |
| `DATE` column | **`LocalDate`** | **`String`** (`'2020-01-15'`) | **`java.sql.Date`** |
| `TIMESTAMP` column | `java.sql.Timestamp` | **`String`** | `java.sql.Timestamp` |
| `TIMESTAMPTZ` column | **`OffsetDateTime`** | **`String`** | **`java.sql.Timestamp`** (offset dropped) |
| `JSON` column / `json_*()` | **`JsonNode`** | **`String`** | **`PGobject`** |
| `DECIMAL(p,s)` column | `BigDecimal` | **`Double`** | `BigDecimal` |
| `ARRAY` | `java.sql.Array` | (none) | `java.sql.Array` |
| `MAP` | `LinkedHashMap` | (none) | (none) |
| `STRUCT` / `ROW` | `DuckDBStruct` | (none) | **`PGobject`**, text `(1,x)`, field names lost |
| **`COUNT(*)`** | `Long` | **`Integer`** | `Long` |
| **`SUM(INTEGER)`** | `BigInteger` | **`Integer`** | **`Long`** |
| **`SUM(BIGINT)`** | `BigInteger` | **`Integer`** ⚠ overflow-throws | **`BigDecimal`** |
| `SUM(DOUBLE)` | `Double` | `Double` | `Double` |
| `SUM(DECIMAL)` | `BigDecimal` | **`Double`** | `BigDecimal` |
| **bare numeric literal `1.5`** | `BigDecimal` | **`Double`** | `BigDecimal` |
| `round(2.5)` | `BigDecimal` | **`Double`** | `BigDecimal` |
| `ROW_NUMBER()`/`RANK()` (through an outer `SUM`) | `BigInteger` | **`Integer`** | **`BigDecimal`** |
| `length(…)` | `Long` | `Integer` | `Integer` |

**Reading of the table.** SQLite needs normalize rows for **BOOLEAN, DATE, TIMESTAMP, TIMESTAMPTZ,
JSON, DECIMAL, HUGEINT and every integer aggregate** — eight families, versus H2's four. The
`Integer`-for-`Boolean` precedent H2 set is exactly reproduced. Postgres needs far fewer, but three
of its four are non-obvious: `java.sql.Date` where DuckDB gives `LocalDate`, `PGobject` for JSON
(H2's `byte[]` in a different costume), and `BigDecimal` where DuckDB gives `BigInteger` for
`SUM(BIGINT)`.

**`Executor.java:118-119`'s `String.valueOf(rs.getObject(1))` is wrong on both targets** for the
same reason it was wrong on H2: `PGobject` stringifies with the driver's own spacing
(`{"n" : "alice"}`, note the spaces) and SQLite's JSON is already a `String` with DuckDB's spacing.
Two backends, two different JSON texts for one logical value.

### 5.3 HUGEINT — neither target has it, and the fallbacks fail differently

| | Postgres | SQLite |
|---|---|---|
| Fallback | **bare `NUMERIC`/`DECIMAL`** — full ±2¹²⁷ range, exact, `BigDecimal` | **TEXT** — round-trips the digits exactly; **no arithmetic** |
| Wrong fallback | `NUMERIC(38,0)` → `numeric field overflow` on int128 max | `HUGEINT`/`DECIMAL(38,0)` names are *accepted* and then stored as REAL |
| Overflow at 2⁶³ | `bigint out of range` — **loud** | **`9223372036854775807 + 1` → `9.223372036854776E18` (Double), silent** |
| `SUM` past 2⁶³ | promotes to `NUMERIC` → exact `18446744073709551614` | `integer overflow` — **loud**, and it is the *only* loud SQLite numeric error found |
| `CAST(<int128 max> AS HUGEINT)` | type absent (loud) | **`Long:9223372036854775807` — silently clamped** |

**SQLite HUGEINT is a D, not a C.** A carrier that silently clamps to int64 on cast, silently
degrades to float on arithmetic, and silently loses precision on storage cannot be a rendering
override. Declare it in the §9 gap registry.

**Bucket totals — types:** SQLite A 4 · C 5 · B 0 · D 5. Postgres A 8 · C 5 · B 1 · D 0.

---

## 6. Silent value divergence — exhaustive, both values

This is the defect class that matters. Every row below **parses on both backends and returns a
different answer**, or returns an answer where DuckDB errors.

### 6.1 CAST failure behaviour — SQLite never fails

| Expression | DuckDB | SQLite | Postgres |
|---|---|---|---|
| `CAST('abc' AS INTEGER)` | error | **`0`** | error |
| `CAST('abc' AS BIGINT)` | error | **`0`** | error |
| `CAST('abc' AS DOUBLE PRECISION)` | error | **`0.0`** | error |
| `CAST('abc' AS DECIMAL(12,2))` | error | **`0`** | error |
| `CAST('abc' AS BOOLEAN)` | error | **`0`** | error |
| `CAST('not-a-date' AS DATE)` | error | **`0`** | error |
| `CAST('12abc' AS INTEGER)` | error | **`12`** — prefix parse | error |
| `CAST('1.9' AS INTEGER)` | **`2`** (rounds) | **`1`** (truncates) | **error** — three-way divergence |
| `CAST(999999999999 AS INTEGER)` | error | **`999999999999`** — no narrowing at all | error |
| `CAST('2020-01-15' AS DATE)` | `LocalDate:2020-01-15` | **`Integer:2020`** ☠ | `Date:2020-01-15` |
| `CAST('2020-01-15 10:20:30' AS TIMESTAMP)` | `Timestamp` | **`Integer:2020`** ☠ | `Timestamp` |
| `CAST('{"a":1}' AS JSON)` | `JsonNode` | **`Integer:0`** ☠ | `PGobject` |
| `CAST('t' AS BOOLEAN)` | `true` | **`0`** ☠ | `true` |
| `TRY_CAST(x AS T)` | `NULL` | **absent** | **absent** |

**SQLite's `CAST` is not a cast — it is a coercion to storage-class affinity that cannot fail.** The
four ☠ rows are the worst: a `Cast` node that DuckDB uses to *carry a type* silently produces a
number. Any `SqlExpr.Cast` to DATE/TIMESTAMP/JSON/BOOLEAN must be rewritten out on SQLite, never
rendered. Postgres matches DuckDB's error discipline everywhere except `CAST('1.9' AS INTEGER)`,
where DuckDB rounds and Postgres refuses.

### 6.2 NULL sort order — all three backends differ from each other

| | ASC default | DESC default |
|---|---|---|
| **DuckDB** | NULLS **LAST** | NULLS **LAST** |
| **SQLite** | NULLS **FIRST** | NULLS **LAST** |
| **Postgres** | NULLS **LAST** | NULLS **FIRST** |

Explicit `NULLS FIRST` / `NULLS LAST` is honoured on all three. **`nullOrdering: explicit` is
mandatory for both targets, and for a different reason on each.**

This is not theoretical. `dense_rank() OVER (ORDER BY dept_id)` summed over the fixture returns
**9 on DuckDB, 9 on Postgres, 11 on SQLite** — a wrong answer with no error, from one NULL row.
`GROUPS BETWEEN 1 PRECEDING` diverges the same way (38 / 38 / **41**). And the trap has teeth:
plain `rank()` over the same data returns **13 on all three** by coincidence, so a single-probe
check would have declared the family clean.

### 6.3 Arithmetic

| Expression | DuckDB | SQLite | Postgres |
|---|---|---|---|
| `7 / 2` | **`3.5`** | **`3`** | **`3`** |
| `CAST(7 AS INTEGER) / CAST(2 AS INTEGER)` | **`3.5`** | **`3`** | **`3`** |
| `1 / 0` | **`Infinity`** | **`NULL`** | **error** |
| `1.0 / 0.0` (doubles) | **`Infinity`** | **`NULL`** | **error** |
| `CAST(1 AS DECIMAL(20,10)) / CAST(3 AS DECIMAL(20,10))` | `0.3333333333333333` | **`Integer:0`** ☠ | `0.33333333333333333333` |
| `9223372036854775807 + 1` | error | **`9.223372036854776E18`** | error |
| `round(2.5)` / `round(3.5)` | `3` / `4` | `3` / `4` | `3` / `4` — half-up everywhere |

**`SqlFn.DIVIDE` is a silent wrong answer on both targets today.** DuckDB's `/` is true division;
SQLite's and Postgres's is integer division on integer operands. `FU.pgint.c1`/`c3` confirm the fix:
`CAST(a AS DOUBLE PRECISION) / b` returns 3.5 on all three. (Flagged here because it is a *shape*
consequence of the IR's untyped `/`; the `SqlFn` slice owns the spelling.)

The DECIMAL row is the most dangerous single cell in this report: SQLite gives `DECIMAL` NUMERIC
affinity, so both operands stay integers and `1/3` becomes **`0`**.

### 6.4 DECIMAL precision loss on SQLite

| | DuckDB | SQLite | Postgres |
|---|---|---|---|
| `CAST('1.005' AS DECIMAL(12,2))` | `1.01` | **`1.005`** — scale not applied | `1.01` |
| `DECIMAL(12,2)` column read back | `BigDecimal:2000.50` | **`Double:2000.5`** | `BigDecimal:2000.50` |
| `typeof(<decimal col>)` | `DECIMAL(12,2)` | **`'real'`** | (n/a) |
| `0.1 + 0.2` at `DECIMAL(30,20)` | `0.30000000000000000000` | **`0.30000000000000004`** ☠ | `0.30000000000000000000` |
| `DECIMAL(30,5)` holding 25 digits | exact | **`1.2345678901234567E19`** — 6 digits lost | exact |
| `sum(DECIMAL)` | `BigDecimal:3300.75` | `Double:3300.75` | `BigDecimal:3300.75` |

**Every SQLite DECIMAL is an IEEE double.** Money arithmetic is wrong past 15–17 significant digits
and wrong at the 17th decimal immediately. `CAST(x AS VARCHAR)` on a decimal also diverges:
`'2000.50'` (DuckDB/PG, scale preserved) vs **`'2000.5'`** (SQLite) — so the divergence escapes into
string outputs and JSON envelopes too. The only exact SQLite carrier is TEXT, which forfeits
arithmetic. **`Decimal` is a D on SQLite and must be registered as one.**

### 6.5 Empty and NULL aggregates — the one clean family

| | DuckDB | SQLite | Postgres |
|---|---|---|---|
| `sum(x)` over **no rows** | `NULL` | `NULL` | `NULL` |
| `count(*)` over no rows | `0` | `0` | `0` |
| `avg` / `min` / `string_agg` over no rows | `NULL` | `NULL` | `NULL` |
| `sum` over rows **containing** NULL | `391.5` | `391.5` | `391.5` |
| `count(col)` skipping NULL | `4` | `4` | `4` |
| `GROUP BY` / `DISTINCT` treat NULL as one group | `3` | `3` | `3` |

All three agree: **`SUM` of no rows is NULL, never 0.** No rewrite needed. (SQLite additionally
offers `total(x)` which returns `0.0` for the empty case — do **not** map `SUM` to it.)

### 6.6 Other

- `'a' || NULL` → `NULL` on all three. `''` is not NULL on any. Clean.
- `string_agg(DISTINCT name, ',')` → DuckDB `carol,eve,dave,alice,bob` (hash order),
  Postgres `alice,bob,carol,dave,eve` (sorted). **DuckDB's is the non-deterministic one.** SQLite
  refuses the 2-arg DISTINCT form entirely (`group_concat(DISTINCT name)`, comma only).
- `mode(dept_id)` exists on SQLite's JDBC driver and returned **`NULL`** where DuckDB returned `1`.
  Do not map `MODE` to it.
- **Postgres truncates identifiers to 63 bytes silently.** Two 71-character aliases differing only
  after byte 63 both create, then collide on reference (`column reference … is ambiguous`).
  Generated aliases longer than 63 chars are a live corruption risk.

---

## 7. Aggregates (36)

### 7.1 Reducers (25)

| | SQLite | Postgres |
|---|---|---|
| `SUM` `COUNT` `AVG` `MIN` `MAX` | **A** ×5 | **A** ×5 |
| `ANY_VALUE` | **C** — no `any_value`; SQLite's lenient GROUP BY makes a bare column legal (verified), or `min()` | **A** — PG 16+ |
| `STDDEV_SAMP` | **C** — `stdev(x)` (see caveat) | **A** |
| `STDDEV` | **C** — `stdev(x)` | **A** |
| `VAR_SAMP` | **C** — `variance(x)` | **A** |
| `VARIANCE` | **C** — `variance(x)` | **A** |
| `STDDEV_POP` | **B** — `sqrt(avg(x*x) - avg(x)*avg(x))`, **value-exact** (1506.0) | **A** |
| `VAR_POP` | **B** — `avg(x*x) - avg(x)*avg(x)`, **value-exact** (22695.0) | **A** |
| `MEDIAN` | **C** — `median(x)` present and **value-exact** (95.375) | **C** — `percentile_cont(0.5) WITHIN GROUP (ORDER BY x)` |
| `MODE` | **B** — `mode(x)` exists but returns NULL; use `GROUP BY x ORDER BY count(*) DESC LIMIT 1` (verified) | **C** — `mode() WITHIN GROUP (ORDER BY x)` |
| `BOOL_AND` | **B** — `min(CASE WHEN p THEN 1 ELSE 0 END)` | **A** |
| `BOOL_OR` | **B** — `max(CASE WHEN p THEN 1 ELSE 0 END)` | **A** |
| `QUANTILE_CONT` | **B** — `avg` over the ORDER BY/LIMIT/OFFSET window (verified 95.375) | **C** — `percentile_cont(q) WITHIN GROUP` |
| `QUANTILE_DISC` | **B** — ORDER BY/LIMIT/OFFSET | **C** — `percentile_disc(q) WITHIN GROUP` |
| `CORR` | **B** — algebraic, **value-exact** (−230.0) | **A** |
| `COVAR_POP` | **B** — `avg(xy) − avg(x)avg(y)`, value-equal to rounding | **A** |
| `COVAR_SAMP` | **B** — algebraic × n/(n−1) | **A** |
| `ARG_MAX` | **B** — `ORDER BY key DESC LIMIT 1` (verified `carol`) | **B** — `(array_agg(x ORDER BY y DESC))[1]` (verified) |
| `ARG_MIN` | **B** — same | **B** — same |
| `STRING_AGG` | **C** — `group_concat(x, sep)`; **DISTINCT only in the 1-arg form** | **A** |
| `LIST` | **C** — `json_group_array(x)` → **TEXT, not an array** | **C** — `array_agg(x)` |

`__WAVG__`, `__HASH_LIST__`, `__IS_DISTINCT__`, `__UNIQUE_VALUE_ONLY__` are composition markers in
`Aggregates.java` and never reach a renderer — not scored.

> **Caveat, recorded because it is a trap.** `stdev`, `variance`, `median` and `mode` are **not core
> SQLite functions.** They resolve on the bundled `sqlite-jdbc 3.47.1.0` (which registers extension
> UDFs on the connection) and will **not** resolve against a stock `sqlite3` CLI or a
> differently-built driver. Treat every one of them as **C-conditional**: keep the B-form (all
> verified value-exact above) as the portable rendering, or pin the driver explicitly. This is the
> same class of dependency as H2's 15 `CREATE ALIAS` UDFs (§4.2) — with the difference that here it
> is the *driver*, not our code, doing the installing.

### 7.2 Window-only (11)

`ROW_NUMBER` `RANK` `DENSE_RANK` `PERCENT_RANK` `CUME_DIST` `NTILE` `LAG` `LEAD` `FIRST_VALUE`
`LAST_VALUE` `NTH_VALUE` — **A on both targets**, all value-verified, including `LAG(x, n)` and
`LAG(x, n, default)`. The only divergence in the family is `DENSE_RANK`'s NULL-ordering result
(§6.2), which is a sort-order bug, not a function gap.

**Bucket totals — aggregates:**

| | SQLite | Postgres |
|---|---|---|
| A | 5 reducers + 11 window-only = **16** | 18 reducers + 11 window-only = **29** |
| C | `ANY_VALUE` `STDDEV_SAMP` `STDDEV` `VAR_SAMP` `VARIANCE` `MEDIAN` `STRING_AGG` `LIST` = **8** | `MEDIAN` `MODE` `QUANTILE_CONT` `QUANTILE_DISC` `LIST` = **5** |
| B | **12** | **2** (`ARG_MAX`, `ARG_MIN`) |
| D | **0** | **0** |
| | 36 | 36 |

If the driver-registered UDFs (`stdev`, `variance`, `median`) are refused as non-portable, SQLite
becomes **A 16 · C 4 · B 16 · D 0** — every displaced row has a verified value-exact B form, so no
D appears either way.

**Not one aggregate is a D on either target.** That is the single most encouraging line in this
report and it inverts the H2 expectation.

---

## 8. Capability budget (H2_BACKEND §8 shape)

| Budget | H2 (reference) | **SQLite 3.47.1** | **Postgres 18.4** |
|---|---|---|---|
| `aliasLimit` | 256 | **64 tables per join** (hard: `at most 64 tables in a join`); identifier length unbounded | identifier **63 bytes, silently truncated**; **1664** target-list entries |
| `collectionThresholdLimit` | not set | **not set** — 100 000-element `IN` list executes | **not set** — 100 000-element `IN` list executes |
| `supportsFullOuterJoin` | false | **true** (3.39+, verified on 3.47.1) | **true** |
| `supportsBooleanLiteral` | true | **true** — `TRUE`/`FALSE` parse; the *value* returns as `Integer` (normalize row, not a budget row) | **true** |
| `limitStyle` | `top N` / `offset…fetch` | **`LIMIT n OFFSET m` only.** `OFFSET` without `LIMIT` is a **syntax error** → emit `LIMIT -1 OFFSET m`. No `FETCH`, no `TOP`. `LIMIT m, n` accepted (reversed order) | **`LIMIT n OFFSET m`** *and* `OFFSET m ROWS FETCH NEXT n ROWS ONLY`; bare `OFFSET m` legal; **`LIMIT m, n` rejected** |
| `nullOrdering` | explicit | **explicit** — default ASC = NULLS **FIRST** | **explicit** — default DESC = NULLS **FIRST** |

### Rows the H2 budget does not have, and both targets need

| Budget | SQLite | Postgres |
|---|---|---|
| `supportsQualify` | false — `QualifyToSubselect` covers it | false — same |
| `supportsLateral` | **false** | true |
| `supportsIntersectAll` / `supportsExceptAll` | **false** | true |
| `maxCompoundSelectTerms` | **500** | unbounded (1000 verified) |
| `maxResultColumns` | **2000** | **1664** |
| `maxExpressionDepth` | ~1000 (`expression tree is too large`) | 2000+ |
| `supportsIgnoreNulls` | false | false |
| `supportsAggregateOrderByInWindow` | false | false |
| `supportsDistinctInWindow` | false | false |
| `supportsIntervalFrame` | **false** | true, **singular unit only** |
| `castFailureMode` | **`coerce`** ☠ | `error` |
| `divideIsInteger` | **true** | **true** |
| `divideByZero` | `null` | `error` |

`SQLite`'s 64-table join limit and 500-term compound limit are the two budget rows most likely to be
hit by generated SQL: a graph fetch over a wide class, and a `Values` source rendered as a
`UNION ALL` chain, respectively. Note that `Values` rendered as a real `VALUES` list is **not**
subject to the 500 limit (600 rows verified) — only the `UNION ALL` fallback is, which is exactly
the fallback `AnsiSqlRenderer.valuesSource`'s comment says SQLite would need. **Render `VALUES`
natively on SQLite and rename its `column1…columnN` outputs; do not emit the UNION chain.**

---

## 9. Bucket totals

| | SqlExpr (27 scored) | Sources/joins (14) | Types (14) | Aggregates (36) | **Total (91)** |
|---|---|---|---|---|---|
| **SQLite** A | 14 | 8 | 4 | 16 | **42** (46%) |
| **SQLite** C | 7 | 2 | 5 | 8 | **22** (24%) |
| **SQLite** B | 2 | 2 | 0 | 12 | **16** (18%) |
| **SQLite** D | 4 | 2 | 5 | 0 | **11** (12%) |
| **Postgres** A | 16 | 11 | 8 | 29 | **64** (70%) |
| **Postgres** C | 7 | 1 | 5 | 5 | **18** (20%) |
| **Postgres** B | 3 | 2 | 1 | 2 | **8** (9%) |
| **Postgres** D | 1 | 0 | 0 | 0 | **1** (1%) |

(91, not 93: `SqlExpr.Call` belongs to the `SqlFn` slice and `SqlExpr.PlanParam` is not executable on
any backend.)

**The 11 SQLite D's**, in full: `ArrayLit`, `StructLit`, `StructGet`, `Lambda` · `Join.CROSS_LATERAL`,
`Join.LEFT_LATERAL` · `HUGEINT`, `Decimal`, `Array`, `Map`, `Struct`. Ten of the eleven are one
thing — **SQLite has no composite or wide-numeric value at all**, only NULL/INTEGER/REAL/TEXT/BLOB.
That is the same finding H2_BACKEND §4.1 reached from a different direction: the wall is the
**collection/value carrier**, not the query shape. SQLite reaches it harder, because it also loses
`Decimal`.

**The 1 Postgres D** is `SqlExpr.Lambda`, and it is the same row H2 has (D3). Postgres executes 70%
natively and needs **no capability refusal outside the lambda family** — the highest native share of
any backend measured so far, DuckDB excluded. Compare H2's 54% A / 19% D over the same four groups.

**Read the D column, not the A column.** SQLite's 12% D looks survivable and is not: five of the
eleven are *type* rows, and a missing type is not a missing feature — it is a value that arrives
wrong. §6 is the operative section, not §9.
