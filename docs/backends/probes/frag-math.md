# MATH / BITWISE / ARITHMETIC / COMPARISON / NULL — SQLite and Postgres

> **Evidence standard.** 439 probe statements executed against real `org.xerial:sqlite-jdbc
> 3.47.1.0` and real PostgreSQL 18.4 (zonky embedded), every one cross-checked against
> `duckdb_jdbc 1.5.0.0` as the reference value. Probes: `probe/probes-math.tsv`; raw results:
> `probe/out-{duckdb,sqlite,postgres}-math.tsv`. Where documentation and execution disagreed,
> execution won. **60 `SqlFn` constructs** in scope.

---

## 0. Headline — the SQLite math-functions question is settled: THEY ARE PRESENT

**`org.xerial:sqlite-jdbc 3.47.1.0` ships a SQLite compiled with `SQLITE_ENABLE_MATH_FUNCTIONS`,
*and* it registers `extension-functions.c` on every connection.** Measured, not assumed:

| Source | Functions confirmed working |
|---|---|
| **SQLite core math build** | `sqrt ln log10 log2 log(b,x) exp pow power pi sin cos tan asin acos atan atan2 sinh cosh tanh acosh asinh atanh ceil ceiling floor trunc degrees radians mod` |
| **xerial's `extension-functions.c`** (proved by `proper padl strfilter difference charindex replicate coth atn2 variance median lower_quartile` all returning values) | adds **`sign` `cot` `square` `reverse` `stdev`** |
| **Absent everywhere on SQLite** | **`cbrt`** — the only math name in this slice with no SQLite spelling at all |

**This single fact moves 24 of my 60 constructs from a predicted D to A.** Without it,
`SQRT EXP LN LOG10 POW PI SIN COS TAN ASIN ACOS ATAN ATAN2 SINH COSH TANH CEILING FLOOR
COT RADIANS DEGREES SIGN` would all have been D.

⚠️ **Portability caveat that must be written into the dialect, not discovered later.**
`SIGN` and `COT` come from **`extension-functions.c`, which is the JDBC driver's doing, not
SQLite's**. Stock `sqlite3`, a different JDBC driver, or a native/mobile SQLite build will *not*
have them (`sign` and `cot` are absent from SQLite's documented math-function list). Both have
exact, verified pure-SQL fallbacks — `CASE WHEN x>0 THEN 1 WHEN x<0 THEN -1 ELSE 0 END` and
`1.0/tan(x)` — which returned byte-identical values. **Prefer the fallbacks; do not bind
legend-lite's SQLite dialect to a driver-private function table.**

---

## 1. Capability map — 60 constructs

Buckets per `docs/H2_BACKEND.md` §2: **A** native (DuckDB's own spelling, same value) · **C**
rendering override · **B** rewrite/composite · **D** impossible.

| `SqlFn` | SQLite | SQLite spelling / construction | Postgres | Postgres spelling / construction |
|---|---|---|---|---|
| `AND` `OR` `NOT` | **A** | infix | **A** | infix — but `NOT <int>` **errors** (strict boolean, §4.8) |
| `EQUAL` `NOT_EQUAL` `LESS` `LESS_EQUAL` `GREATER` `GREATER_EQUAL` | **A** | infix | **A** | infix |
| `PLUS` `MINUS` `TIMES` | **A** | infix | **A** | infix |
| `DIVIDE` | **A** | `((1.0 * a) / b)` — the renderer's forced-float form; → `Double` | **C** | `((CAST(1.0 AS double precision) * a) / b)`. The current `1.0` literal is **`numeric`** on PG → `BigDecimal` + 16-digit truncation |
| `MOD` (always-positive) | **C** | integer operands `((a % b) + b) % b`; float operands `mod(mod(a,b)+b,b)`. `MOD()` returns `REAL` even for ints | **B** | `CAST(mod(mod(CAST(a AS numeric),b)+b,b) AS double precision)` — **`mod(double,double)` does not exist on PG** |
| `REM` (keeps sign) | **C** | integer `a % b`; float **must** be `mod(a,b)` — `%` truncates floats (§4.3) | **B** | `CAST(mod(CAST(a AS numeric), CAST(b AS numeric)) AS double precision)` |
| `NEGATE` `ABS` | **A** | `-x`, `abs(x)` | **A** | `-x`, `abs(x)` |
| `IS_NULL` `IS_NOT_NULL` `IN` `COALESCE` | **A** | ANSI | **A** | ANSI |
| `PARSE_INT` | **B** | `CAST(x AS BIGINT)` *parses everything* — never raises (§4.5). Needs a GLOB validation guard + the §3 raise | **A** | `CAST(x AS BIGINT)` — raises correctly on junk and on overflow |
| `SQRT` `EXP` `LN` `LOG10` `POW` `PI` | **A** | `sqrt ln log10 power pi()` | **A** | same |
| `CBRT` | **B** | absent → `CASE WHEN x<0 THEN -pow(-x,1.0/3) ELSE pow(x,1.0/3) END` → `-3.0`/`3.0` exact | **A** | `cbrt(x)` |
| `SIN COS TAN ASIN ACOS ATAN ATAN2 SINH COSH TANH` | **A** | plain names | **A** | plain names |
| `CEILING` `FLOOR` | **A** | `CAST(ceil(x) AS BIGINT)` / `CAST(floor(x) AS BIGINT)` | **A** | same |
| `FLOOR_RAW` | **A** | `floor(x)` | **A** | `floor(x)` |
| `ROUND` (**HALF-EVEN**) | **B** at scale 0 · **D** at scale ≥1 | §2 construction — exact match to `ROUND_EVEN` for `0.5 1.5 2.5 3.5 -0.5 -2.5 -3.5 1.2`. Scale ≥1 **cannot** be made to agree (§4.2) | **B** | `round(x::double precision)` is **natively half-even** at scale 0 (**C**); scale ≥1 needs the §2 construction over `numeric`, which matches DuckDB exactly |
| `ROUND_HALF_UP` | **A** | `ROUND(x[,n])` — matches at scale 0; diverges at scale ≥1 on decimal literals (§4.2) | **C** | **`round(CAST(x AS numeric)[, n])`**. Bare `round(double)` is half-EVEN (wrong) and `round(double,int)` **does not exist** |
| `SIGN` | **A**\* | `CAST(sign(x) AS BIGINT)` — \*driver-supplied; portable form `CAST(CASE WHEN x>0 THEN 1 WHEN x<0 THEN -1 ELSE 0 END AS BIGINT)` | **A** | `CAST(sign(x) AS BIGINT)` — bare `sign()` returns `double` |
| `XOR` (boolean) | **A** | `(x AND NOT y) OR (NOT x AND y)` | **A** | same |
| `BIT_AND` `BIT_OR` | **A** | `&` `\|` — 64-bit | **A** | `&` `\|` — **32-bit unless operands are cast to BIGINT** |
| `BIT_XOR` | **B** | `xor()` absent → `((a \| b) - (a & b))` → `6` ✓ | **C** | `(a # b)` |
| `BIT_SHIFT_LEFT` `BIT_SHIFT_RIGHT` | **A** | `<<` `>>` — 64-bit, mathematically correct | **C** | `<<` `>>` **with both operands cast to BIGINT**; shift count is masked mod width (§4.6) |
| `BIT_NOT` | **C** | `~x` (`xor(x,-1)` absent). Safe at `MIN_LONG` — verified `9223372036854775807` | **C** | `~x` |
| `COT` | **A**\* | `cot(x)` — \*driver-supplied; fallback `1.0/tan(x)` exact | **A** | `cot(x)` |
| `RADIANS` `DEGREES` | **A** | plain | **A** | plain |
| `INT_DIVIDE` | **B** | `//` is a **syntax error** → `CAST(trunc((1.0*a)/b) AS BIGINT)` → `-3` ✓ | **B** | `//` is `operator does not exist` → `CAST(trunc(a/b) AS BIGINT)`, or `div(numeric,numeric)` |
| `IS_DISTINCT` | **A** | `a IS DISTINCT FROM b` — native | **A** | native |
| `ERROR` | **B** | `json_extract('{}', '<msg>')` → `bad JSON path: '<msg>'` — conditional **and** message-carrying (§3) | **B** | `CAST('<msg>' \|\| left(CAST(random() AS text),0) AS BIGINT)` (§3) |
| `CURRENT_USER_FN` | **D** | — | **A** | `current_user` **bare**; `current_user()` is a syntax error |
| `TYPEOF` | **D** | `typeof()` exists but returns **storage classes**, not types (§4.7) | **C** | `CAST(pg_typeof(x) AS text)` — strings differ from DuckDB's |

### Bucket totals

| | SQLite | Postgres |
|---|---:|---:|
| **A** native | **49** (82%) | **48** (80%) |
| **C** rendering override | **3** | **7** |
| **B** rewrite / composite | **6** | **5** |
| **D** impossible | **2** | **0** |
| | **60** | **60** |

**Postgres has zero D's in this slice.** SQLite has two, both soft (§5).

---

## 2. The banker's-`ROUND` construction that works on both

`ROUND_EVEN` exists **only** on DuckDB (`round_even` → `function does not exist` on both targets;
`roundeven` and `rint` exist nowhere, including Postgres). This composite reproduced DuckDB's
`ROUND_EVEN(x, 0)` **byte-for-byte on SQLite, Postgres and DuckDB**:

```sql
CASE WHEN x - floor(x) = 0.5 AND CAST(floor(x) AS BIGINT) % 2 <> 0
       THEN CAST(floor(x) AS BIGINT) + 1
     WHEN x - floor(x) = 0.5
       THEN CAST(floor(x) AS BIGINT)
     ELSE CAST(round(x) AS BIGINT) END
```

| x | 0.5 | 1.5 | 2.5 | 3.5 | −0.5 | −2.5 | −3.5 | 1.2 | `sal`=100.5 |
|---|---|---|---|---|---|---|---|---|---|
| DuckDB `ROUND_EVEN` | 0 | 2 | 2 | 4 | −0 | −2 | −4 | 1 | 100 |
| construction, all 3 backends | **0** | **2** | **2** | **4** | **0** | **−2** | **−4** | **1** | **100** |

**Write `% 2 <> 0`, never `% 2 = 1`.** SQL `%` keeps the dividend's sign, so `-3 % 2` is `-1`;
the `= 1` form silently mis-rounds every negative tie (measured: `-2.5 → -3`, should be `-2`).

**Scale ≥ 1 does not port to SQLite.** Pre-scaling by `10ⁿ` matches DuckDB on Postgres (`numeric`)
but not on SQLite, because SQLite parses `2.345` as binary `REAL`: `2.345*100` is
`234.50000000000003` there and exactly `234.500` on DuckDB/PG. Measured `round_even(2.345,2)`:
DuckDB `2.34`, construction on Postgres `2.34` ✓, construction on SQLite **`2.35`** ✗. **`ROUND`
at non-zero scale is a `D` on SQLite** and belongs in the declared-gap registry.

---

## 3. `ERROR` must raise — both targets can, neither the obvious way

`error('msg')` is DuckDB-only (`no such function` / `function error(unknown) does not exist`).

**Everything that "should" raise on SQLite silently does not:**

| Vector | SQLite result |
|---|---|
| `RAISE(ABORT,'boom')` | `RAISE() may only be used within a trigger-program` — **prepare-time parse error, not a value-position raise** |
| `1/0` | **`NULL`** |
| `CAST('boom' AS BIGINT)` | **`0`** |
| `sqrt(-1)` `ln(-1)` `ln(0)` `log10(0)` `asin(2)` `acos(2)` | **`NULL`** (DuckDB and Postgres all raise) |
| `9223372036854775807 + 1` | **`9.223372036854776E18`** — silently promotes to `REAL` |

**The one that works — `json_extract`:**

```sql
json_extract('{}', 'this is the Pure error message')
--> [SQLITE_ERROR] bad JSON path: 'this is the Pure error message'
```

Verified: fires only when reached (`CASE WHEN id>100 THEN … ELSE 'ok' END` → `ok`;
`CASE WHEN id>0 THEN …` → raises), and **carries the message text verbatim**. Also raises for
messages prefixed `$` or `!`, so a defensive prefix is safe. Runner-up: `json('<msg>{')` raises
conditionally but reports only `malformed JSON` — no message.

**Postgres — the naïve construction is wrong.** `CAST('boom' AS integer)` raises with the message
**but is constant-folded during parse analysis**, so it fires even from an unreachable `CASE` arm
(measured: `CASE WHEN 1=1 THEN 42 ELSE CAST('boom' AS BIGINT) END` → error). `1/0` is *not*
folded (same shape → `42`) but carries no message. The form that is both lazy and
message-carrying:

```sql
CAST('<msg>' || left(CAST(random() AS text), 0) AS BIGINT)
--> ERROR: invalid input syntax for type bigint: "<msg>"
```
`random()` is `VOLATILE`, which blocks folding; `left(…,0)` appends nothing. Verified: guard false
→ `42`, guard true → raises with the message.

---

## 4. SILENT VALUE DIVERGENCE — the highest-value section

Every row: **the statement parses on the target and returns a different value than DuckDB.**

### 4.1 `round(<DOUBLE column>)` on Postgres is banker's — DuckDB's is half-up

The single most dangerous row in this slice, because it fires on the plain path with no cast
anywhere in sight.

| Probe | DuckDB | SQLite | **Postgres** |
|---|---|---|---|
| `round(sal)`, `sal`=100.5 | `101.0` | `101.0` | **`100.0`** |
| `round(2.5::float8)` | `3.0` | `3.0` | **`2.0`** |
| `round(0.5::float8)` | `1.0` | `1.0` | **`0.0`** |
| `round(-2.5::float8)` | `-3.0` | `-3.0` | **`-2.0`** |
| `round(1.5::float8)` / `round(3.5::float8)` | `2.0` / `4.0` | `2.0` / `4.0` | `2.0` / `4.0` (agree — ties-to-even coincides) |

Postgres `round(numeric)` **is** half-up (`2.5→3`, `-2.5→-3`) and agrees with DuckDB.
**The mode depends entirely on the argument type, and only half the test values expose it.**
`ROUND_HALF_UP` must therefore render `round(CAST(x AS numeric)[, n])` on Postgres — verified to
reproduce DuckDB exactly for `2.5 3.5 -2.5 2.345 sal`.

Related **hard failure**, not a divergence: `round(<double>, <int>)` **does not exist** on
Postgres (`round(sal, 1)` → `function round(double precision, integer) does not exist`). Any
2-arg `ROUND_HALF_UP` over a `DOUBLE` column fails outright today.

*(H2 precedent: H2's `ROUND` was measured HALF_UP. Postgres is worse — it is **both**, chosen by
argument type.)*

### 4.2 `round(x, n)` at non-zero scale — SQLite disagrees on exact-decimal literals

| Probe | DuckDB | **SQLite** | Postgres |
|---|---|---|---|
| `round(2.355, 2)` | `2.36` | **`2.35`** | `2.36` |
| `round(1.005, 2)` | `1.01` | **`1.0`** | `1.01` |
| `round(2.345, 2)` | `2.35` | `2.35` | `2.35` (agree) |

Root cause: DuckDB and Postgres parse `1.005` as exact `DECIMAL`/`numeric`; SQLite parses it as
binary `REAL` (`1.00499999…`). Not fixable by spelling — it is a **type-system** difference.

### 4.3 Integer vs float division, and `%` on floats

| Probe | DuckDB | **SQLite** | **Postgres** |
|---|---|---|---|
| `5/2` | `2.5` | **`2`** | **`2`** |
| `id/2`, `id`=3 | `1.5` | **`1`** | **`1`** |
| `sal % 2`, `sal`=100.5 | `0.5` | **`0.0`** | **ERROR** (`operator % (double, double)` does not exist) |
| `5.5 % 2` | `1.5` | **`1.0`** | `1.5` |

SQLite's `%` **truncates both operands to integers**. Both targets do integer division for `/`
where DuckDB returns a float. The renderer's `((1.0 * a) / b)` already neutralises the `/` case —
**keep it** — but `REM` must stop rendering `%` for non-integer operands on SQLite.

*(H2 precedent H5.2: `INT_DIVIDE` rendered `//`, which is a comment in H2 and silently returned
the numerator. On SQLite and Postgres `//` is a **hard error** — no silent wrong answer. Better.)*

### 4.4 Division by zero — a three-way split

| Probe | DuckDB | SQLite | Postgres |
|---|---|---|---|
| `1/0` | `Infinity` | **`NULL`** | **ERROR: division by zero** |
| `7 % 0` / `mod(7,0)` | `NULL` | `NULL` | **ERROR: division by zero** |
| `pow(0,-1)`, `exp(1000)`, `pow(10,400)` | `Infinity` | `Infinity` | **ERROR: value out of range** |
| `sqrt(-1)` `ln(0)` `asin(2)` | ERROR | **`NULL`** | ERROR |

Three different answers to the same expression. Postgres raises where DuckDB yields `Infinity`;
SQLite yields `NULL` where DuckDB raises.

### 4.5 `PARSE_INT` on SQLite never fails — and clamps

`CAST(x AS BIGINT)` on SQLite (`PARSE_INT`'s rendering) is a silent-answer machine:

| input | DuckDB | **SQLite** | Postgres |
|---|---|---|---|
| `'9223372036854775807'` / `'-9223372036854775808'` | ✓ exact | ✓ exact | ✓ exact |
| `'9223372036854775808'` (overflow) | ERROR | **`9223372036854775807`** (clamped) | ERROR |
| `'-9223372036854775809'` | ERROR | **`-9223372036854775808`** (clamped) | ERROR |
| `'abc'` | ERROR | **`0`** | ERROR |
| `''` | ERROR | **`0`** | ERROR |
| `'12abc'` | ERROR | **`12`** | ERROR |
| `'1e3'` | `1000` | **`1`** | ERROR |
| `'3.7'` | `4` (**rounds**) | `3` (truncates) | ERROR |

Note DuckDB and Postgres also disagree with each other on `'3.7'` and `'1e3'` — PCT pins
`Long.MIN/MAX`, which all three honour, but nothing else is portable.

Integer **overflow** is the same story: `9223372036854775807 + 1` raises on DuckDB and Postgres,
and on SQLite silently becomes `9.223372036854776E18` (`REAL`). `CAST(1e19 AS BIGINT)` → SQLite
clamps to `Long.MAX`, both others raise. `2147483647 + 1` → both others raise `integer out of
range`; SQLite widens to `2147483648`.

### 4.6 Bitwise width and shift counts

| Probe | DuckDB | SQLite | **Postgres** |
|---|---|---|---|
| `1 << 40` (int literal) | ERROR `Left-shift out of range` | `1099511627776` ✓ | **`256`** (count masked mod 32) |
| `1::int << 32` | ERROR | `4294967296` | **`1`** |
| `1::int << 31` | ERROR | `2147483648` | **`-2147483648`** (sign overflow) |
| `1::bigint << 64` | ERROR | `0` | **`1`** (count masked mod 64) |
| `1::bigint << 100` | ERROR | `0` | **`68719476736`** |
| `16::bigint >> -1` | `0` | **`32`** (shifts the other way) | `0` |
| `-1::bigint >> 64` | `0` | `-1` | `-1` |

Three regimes: DuckDB raises, SQLite is 64-bit and mathematically saturating, Postgres masks the
shift count modulo the operand width and silently overflows. **Both operands must be cast to
`BIGINT` on Postgres** or `<<` is a 32-bit operation. Shifts of ≥ 64 cannot be made to agree.

### 4.7 `TYPEOF`

| value | DuckDB `typeof` | **SQLite `typeof`** | Postgres `pg_typeof` |
|---|---|---|---|
| `1` | `INTEGER` | `integer` | `integer` |
| `1.5` | `DECIMAL(2,1)` | **`real`** | `numeric` |
| `'a'` | `VARCHAR` | `text` | **`unknown`** (untyped literal!) |
| `TRUE` | `BOOLEAN` | **`integer`** | `boolean` |
| `NULL` | `"NULL"` | `null` | `unknown` |
| `hired` (DATE column) | `DATE` | **`text`** | `date` |
| `hired` where the row is NULL | `DATE` | **`null`** | `date` |
| `budget` (`DECIMAL(12,2)`) | `DECIMAL(12,2)` | **`integer`** | `numeric` |

SQLite reports **per-value storage classes**, so the answer *changes row to row* and a `DATE`
column reports `text`. That is exactly the dispatch `Fold.jsonDateWrap:612` needs, and it is a
wrong answer. Postgres's names are lowercase and differ from DuckDB's (`numeric`≠`DECIMAL(2,1)`),
so any string comparison must be dialect-aware. **`pg_typeof` also exists on DuckDB** and returns
DuckDB names lowercased — a tempting but non-portable shortcut.

### 4.8 Smaller, still silent

| Probe | DuckDB | SQLite | Postgres |
|---|---|---|---|
| `'1' = 1` | `true` | **`false`** (no cross-storage-class coercion) | `true` |
| `NOT 5` | `false` | `0` | **ERROR** `argument of NOT must be type boolean` |
| `coalesce(CAST(NULL AS INTEGER), 'a')` | ERROR | **`'a'`** | ERROR |
| `CAST(2.5 AS INTEGER)` | `3` (**rounds**) | **`2`** (truncates) | `3` (rounds) |
| `CAST(1.9 AS INTEGER)` | `2` | **`1`** | `2` |
| `CAST(7/2 AS BIGINT)` | `4` | `3` | `3` |
| `cot(0)` | ERROR | `Infinity` | `Infinity` |

`CAST(<float> AS INTEGER)` **truncates on SQLite and rounds on DuckDB/Postgres.** Any rewrite that
uses a bare integer cast as a floor is correct on SQLite and wrong on the other two — use
`trunc()`/`floor()` explicitly.

Three-valued logic (`NULL AND FALSE`→false, `NULL AND TRUE`→NULL, `NULL OR TRUE`→true),
`IN` with NULLs, `COALESCE`, and `IS DISTINCT FROM` (including `IS NOT DISTINCT FROM`) were
**identical on all three backends** — no divergence found.

---

## 5. The two SQLite `D`s — what was tried

- **`CURRENT_USER_FN` — D.** Tried `current_user`, `CURRENT_USER`, `current_user()`, `user`,
  `session_user`: all five → `no such column`. SQLite is an embedded library with no user concept
  and no session catalog to synthesise one from. The only route is a JVM-registered function,
  which is `CREATE ALIAS` by another name (`H2_BACKEND.md` §4.2 — do not take that exit). A
  constant folded in at render time is the honest alternative if a value is required.
- **`TYPEOF` — D for its actual use.** `typeof()` executes, so it is not a spelling gap; it
  returns SQLite storage classes, which cannot express `DATE` (a date column reports `text`) and
  change per row (a NULL date reports `null`). Tried `typeof`, `pg_typeof` (absent), and reading
  a value's declared type in an expression (no such construct). **This is `H2_BACKEND.md` §D6
  exactly, and downgrades the same way:** the column's declared type is statically known from
  `PRAGMA table_info` / the store declaration, so the runtime dispatch can be resolved at
  lowering. Soft D → C once that lowering exists.
- **`ROUND` at scale ≥ 1 — D on SQLite** (§4.2). No spelling and no construction can agree,
  because SQLite has no exact decimal type. Belongs in the declared-gap registry
  (`H2_BACKEND.md` §9), not in a burndown list.

---

## 6. Returned Java type divergence — candidate `SqlDialect.normalize` rows

`SqlDialect.normalize` has zero overrides today. These are measured, from the `JavaType:` prefix.

| Situation | DuckDB | SQLite | Postgres | Note |
|---|---|---|---|---|
| **Any boolean** — `TRUE`, `1=1`, `IS NULL`, `IN`, `IS DISTINCT FROM`, `x < y` | `Boolean` | **`Integer` (0/1)** | `Boolean` | **The single biggest normalize row in this slice.** Every comparison, null test and membership test on SQLite. Direct analogue of H2's BIT→Integer |
| `DECIMAL(12,2)` column (`budget`) | `BigDecimal:1000.00` | **`Integer:1000`** | `BigDecimal:1000.00` | SQLite has no DECIMAL — scale is gone, not just the Java type |
| `-budget` | `BigDecimal:-1000.00` | **`Integer:-1000`** | `BigDecimal:-1000.00` | |
| `((1.0 * a) / b)` (the `DIVIDE` rendering) | `Double` | `Double` | **`BigDecimal`** | `1.0` is `numeric` on PG. Fix by rendering `CAST(1.0 AS double precision)` → `Double` on all three |
| `MOD(...)` over integers (the `MOD` rendering) | `Integer` | **`Double`** | `Integer` | SQLite's `mod()` is `fmod` — always `REAL`. Render `%` for integer operands |
| `ceil/floor/trunc/round` bare (no cast) | `BigDecimal` | **`Double`** | `BigDecimal` | The renderer's `CAST(... AS BIGINT)` already normalises `CEILING`/`FLOOR`; `FLOOR_RAW` does not |
| `sign(x)` bare | **`Byte`** | `Integer` | **`Double`** | Three different types. The renderer's `CAST(... AS BIGINT)` fixes all three |
| `pg_typeof(x)` | `String` | n/a | **`PGobject`** | Must render `CAST(pg_typeof(x) AS text)` to get a `String` |
| `CAST(x AS BIGINT)` | `Long` | **`Integer`** when the value fits | `Long` | SQLite's driver picks the narrowest type per value |
| `coalesce(sal, -1)` on the NULL row | `Double:-1.0` | **`Integer:-1`** | `Double:-1.0` | **Per-row type instability** — the same column is `Double` on rows 1–4 and `Integer` on row 5. Confirmed via `typeof`: `real` vs `integer` |
| `-(-9223372036854775808)` | `BigInteger` | **`Double`** (precision lost) | `BigDecimal` | |
| `1e18 + 0.5` floor | `Double` | `Double` | **`BigDecimal`** | |

**The per-row row is the nasty one.** SQLite types values, not columns, so a normalizer keyed on
column metadata is not sufficient — it must coerce by the *declared* type of the projection, not
by what `ResultSet.getObject` happened to return for the first row.

---

## 7. Concrete dialect work items

1. **`Spellings.SQLITE`** — nothing to change for the 24 math names; they are DuckDB-identical.
   Add `CBRT` (composite) and remove `xor`.
2. **`Spellings.POSTGRES`** — `BIT_XOR` → `#`; `BIT_NOT` → `~`; `TYPEOF` → `CAST(pg_typeof(x) AS text)`.
3. **`roundHalfEven()` override on both** — the §2 construction; throw for scale ≥ 1 on SQLite.
4. **`ROUND_HALF_UP` on Postgres must wrap in `CAST(x AS numeric)`** — today's `ROUND(x[,n])`
   is a wrong answer for `DOUBLE` at scale 0 and a hard error at scale ≥ 1.
5. **`DIVIDE` should render `CAST(1.0 AS double precision)`, not `1.0`** — one character of
   intent, and it makes the returned Java type `Double` on all three.
6. **`MOD`/`REM` need operand-type dispatch**, not one spelling: `%` for integers, `mod()` for
   floats on SQLite; `numeric` round-trip on Postgres.
7. **`bitOp()` on Postgres must cast both operands to `BIGINT`** or `<<` is 32-bit.
8. **`INT_DIVIDE` must stop emitting `//`** for any non-DuckDB dialect — it is a hard error on
   both targets (unlike H2, where it silently returned the numerator, H5.2).
9. **Register the D's** (`CURRENT_USER_FN`, `TYPEOF`, `ROUND` scale ≥ 1 on SQLite) in the
   declared-gap registry per `H2_BACKEND.md` §9 rather than as skips.
