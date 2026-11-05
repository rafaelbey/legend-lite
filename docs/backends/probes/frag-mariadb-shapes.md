# MariaDB — collections/JSON/variant and query shapes

> **Evidence standard.** Every claim below was produced by **executing probes against a real
> embedded MariaDB 11.4.5** (MariaDB4j, `probes-mdb-shapes{,2,3,4,5,6,7,8}.tsv`, 8 runs, ~560 probe
> statements), with DuckDB 1.5.0.0 run on the same probe files as the reference value. Where
> documentation and execution disagreed, execution won — **and it did, on the headline question.**
> Session as probed: `sql_mode=IGNORE_SPACE,STRICT_TRANS_TABLES,ERROR_FOR_DIVISION_BY_ZERO,
> NO_AUTO_CREATE_USER,NO_ENGINE_SUBSTITUTION` · `character_set_server=utf8mb4` ·
> `collation_server=utf8mb4_general_ci` · `time_zone=SYSTEM` · `max_recursive_iterations=1000`.
> **Note what is NOT in that sql_mode: `ONLY_FULL_GROUP_BY`, `ANSI_QUOTES`, `PIPES_AS_CONCAT`,
> `NO_BACKSLASH_ESCAPES`.** All four are legend-lite configuration decisions (§9).

---

## 1. Verdict — lead with this: **H2's D1 wall does not transfer to MariaDB**

`docs/H2_BACKEND.md` §4 names the wall as *"no LATERAL, no correlated table function. Unfixable."*
On MariaDB **half of that is true and the half that matters is false.**

| | H2 2.4.240 | MariaDB 11.4.5 |
|---|---|---|
| `LATERAL` keyword | **absent** | **absent** — every form is a parse error |
| Correlated row explosion | **absent — D1** | **PRESENT — `JSON_TABLE`** |
| Correlated table-function argument | **absent** | **PRESENT** |

**`JSON_TABLE` exists on MariaDB 11.4.5 and correlates.** This contradicts the widespread claim
that `JSON_TABLE` is MySQL-only. It landed in **MariaDB 10.6** (MDEV-17399) — record the version
threshold, because 10.5 and earlier really are H2-shaped.

```sql
-- correlated explosion against a real table column: OK, SUM = 100 (10+20+30+40)
SELECT SUM(jt.v) FROM arrtab t, JSON_TABLE(t.arr, '$[*]' COLUMNS (v INT PATH '$')) jt;
```

**So the answer to "is MariaDB's collection story closer to H2's or Postgres's" is: Postgres's.**
Not by the same mechanism, but with the same *capability*.

### 1.1 What `LATERAL`'s absence still costs — precisely

`JSON_TABLE` is a correlated table function, not a general lateral. Measured boundary:

| Form | Result |
|---|---|
| `t, JSON_TABLE(t.col, …)` | **OK** — correlated on a column |
| `t, JSON_TABLE(CONCAT('[',t.id,']'), …)` | **OK** — correlated on an expression |
| `t, JSON_TABLE((SELECT JSON_ARRAYAGG(…) FROM u WHERE u.k=t.k), …)` | **OK** — correlated on a *scalar subquery*. This is the general lateral-explosion escape hatch |
| `t LEFT JOIN JSON_TABLE(t.col, …) jt ON TRUE` | **OK** — preserves the empty-array row (`LEFT_LATERAL` exactly) |
| `t, JSON_TABLE(a.col, …) a, JSON_TABLE(a.v, …) b` | **OK** — chained, second correlates on the first |
| `t, LATERAL (SELECT … FROM u WHERE u.k=t.k ORDER BY … LIMIT 1)` | **ERR** — per-row top-N |
| `t, LATERAL (SELECT COUNT(*) c, SUM(x) s FROM u WHERE u.k=t.k)` | **ERR** — per-row multi-aggregate |
| `t, (SELECT t.id AS z) x` | **ERR** `Unknown table 't' in SELECT` |

The two `LATERAL` losses are both **bucket B with verified rewrites**:

- per-row top-N → `ROW_NUMBER() OVER (PARTITION BY k ORDER BY …)` + `WHERE rn=1` (`lt.topnrw`: 3 = DuckDB 3)
- per-row multi-aggregate → grouped `LEFT JOIN` (`lt.multiaggrw2`: 3 = DuckDB 3) or N correlated scalar subqueries (`lt.multiaggrw`: 1 = DuckDB 1)

**There is no probed construct in this slice for which MariaDB has no working form. Bucket D is
empty.**

### 1.2 Two independent explosion routes, both proven

Belt and braces — if `JSON_TABLE` is ever unavailable (a 10.5 target), two fallbacks execute:

**(a) `seq_M_to_N` — MariaDB's SEQUENCE storage engine, a numbers table with a correlated predicate.**
This is exactly the H2 §4-addendum "BOUNDARY" carve-out, and MariaDB has it as a first-class source:
```sql
SELECT SUM(CAST(JSON_EXTRACT(t.arr, CONCAT('$[', s.seq, ']')) AS SIGNED))
FROM arrtab t, seq_0_to_9 s WHERE s.seq < JSON_LENGTH(t.arr);      -- 100 ✓
```
Bounds must be literal (`seq_0_to_JSON_LENGTH(t.arr)` is a parse error), so this is *bounded*
explosion. `seq_1_to_10_step_3` works too.

**(b) Recursive CTE walking `JSON_EXTRACT(arr, CONCAT('$[', n, ']'))` by index** — the lambda-free,
`JSON_TABLE`-free explosion the brief asked about. **It works, unbounded, and correlated:**
```sql
WITH RECURSIVE ex AS (
  SELECT id, arr, 0 AS n FROM arrtab WHERE JSON_LENGTH(arr) > 0
  UNION ALL SELECT id, arr, n+1 FROM ex WHERE n+1 < JSON_LENGTH(arr))
SELECT GROUP_CONCAT(CONCAT(id,':',JSON_EXTRACT(arr, CONCAT('$[',n,']'))) ORDER BY id,n) FROM ex;
--> 1:10,1:20,1:30,2:40   ✓  (id=3's '[]' correctly contributes nothing)
```
This also gives `FoldCall` / `LIST_REDUCE` over an arbitrary lambda, which `JSON_TABLE` alone does
not — a fold is inherently sequential:
```sql
-- fold [1,2,3] with acc = acc*10 + x  -->  123 ✓ ; with init 100 and (+) --> 106 ✓
-- correlated over a table --> '1:60,2:40,3:0' ✓   (fd.fold / fd.foldinit / fd.foldcorr)
```
Guard: `max_recursive_iterations` defaults to **1000** on this build — a collection longer than
1000 silently truncates unless the session raises it. **That is a capability-budget row.**

---

## 2. The carrier verdict

> §4.1 of `H2_BACKEND.md`: *"Reaching H2 means lowering collections to **relations** … That is a
> Phase-H/I redesign of `Scalars.java` + `Fold.java` + `ListShapes.java`, not a dialect exercise.
> And it is the right redesign for N backends generally, since Postgres, Snowflake and BigQuery each
> need a *different* one."*

**That paragraph is confirmed, and MariaDB is a fourth distinct carrier — but a cheap one.**

| Backend | Carrier | Explosion mechanism |
|---|---|---|
| DuckDB | `LIST`/`ARRAY` value | list lambdas — no explosion needed |
| H2 | *none that works* | **D1: nothing** |
| Postgres | array value | `LATERAL` + `unnest` |
| **MariaDB** | **JSON text (`LONGTEXT` + `json_valid` CHECK)** | **`JSON_TABLE` (+ `seq_M_to_N`, recursive CTE)** |

Three things make MariaDB's carrier *materially better than H2's*, and they are the whole verdict:

1. **MariaDB has no `ARRAY` type — and it does not need one.** JSON text is a complete carrier
   because `JSON_TABLE` turns it back into rows. H2 has arrays *and* `UNNEST` and is still fatal,
   because it cannot correlate them. **The carrier that matters is not the value type; it is the
   round trip value → rows → value.** MariaDB closes that loop:
   `JSON_ARRAYAGG` (rows → value) and `JSON_TABLE` (value → rows) are exact inverses, both verified.
2. **The relational lowering §4.1 prescribes is the *only* lowering MariaDB needs** — there is no
   second, lambda-flavoured path to maintain. `filter`/`map`/`exists`/`forAll`/`sort`/`slice` all
   become `WHERE`/`SELECT`/`EXISTS`/`ORDER BY`/`BETWEEN` over `JSON_TABLE` rows, re-aggregated with
   `JSON_ARRAYAGG`. Every one is proven in §4 below with a value equal to DuckDB's.
3. **Therefore MariaDB does not force the §4.1 redesign — it *rewards* it.** H2 forces the question
   and then cannot answer it; MariaDB answers it. If the redesign is done for Postgres it lands on
   MariaDB for the cost of a spelling table plus one placement rule
   (`FROM … , JSON_TABLE(<corr expr>, '$[*]' COLUMNS (…)) alias`).

**Deferral recommendation unchanged, priority changed.** §12 defers the carrier redesign
deliberately. Keep the deferral, but note that MariaDB moves it from *"the work that unblocks the
last 19% on one backend"* to *"the work that unblocks three backends at once"*. It is now the
highest-leverage item in the multi-backend program, not a Phase-H curiosity.

**What MariaDB's carrier costs, honestly:** JSON text is not a typed array. `JSON_LENGTH`,
`JSON_EXTRACT` and `JSON_TABLE` re-parse the document on every call, so an N-element collection
touched K times is O(N·K) parses. DuckDB's `LIST` is a native columnar vector. This is a
**performance** ceiling to measure, not a correctness objection — the same sentence §3 writes about
nested correlated subqueries.

---

## 3. The graph-fetch JSON envelope — **it survives, with one mandatory rewrite**

The `Lowerer.java:627/:737` envelope shape runs in **one statement** on MariaDB and returns correct
3-level nested JSON. But there is a silent type divergence that must be fixed, and it is *not* the
double-encoding everyone expects.

### 3.1 Double-encoding: **NOT a problem.** The expected weak spot is absent.

Despite `JSON` being a `LONGTEXT` alias, MariaDB tracks JSON-ness through function results and
through `JSON`-declared columns. Measured, against DuckDB's identical behaviour:

| Probe | MariaDB | DuckDB | |
|---|---|---|---|
| `JSON_OBJECT('k', JSON_ARRAY(1,2))` | `{"k": [1, 2]}` | `{"k":[1,2]}` | nested, not quoted ✓ |
| `JSON_OBJECT('k', (SELECT JSON_ARRAYAGG(id) FROM emp))` | `{"k": [1,2,3,4,5]}` | — | ✓ |
| `JSON_OBJECT('k', doc)` — `doc` is a `JSON` column | `{"k": {"a":1,"b":[1,2,3]}}` | `{"k":{"a":1,…}}` | ✓ **identical** |
| `JSON_OBJECT('k', arr)` — `arr` is `VARCHAR` holding `'[10,20,30]'` | `{"k": "[10,20,30]"}` | `{"k":"[10,20,30]"}` | quoted on **both** — correct, not a divergence |
| `JSON_OBJECT('k', JSON_EXTRACT(arr,'$'))` | `{"k": [10, 20, 30]}` | `{"k":[10,20,30]}` | the explicit re-JSON-ify escape hatch ✓ |

`JSON_EXTRACT(x,'$')` and `JSON_QUERY(x,'$')` are the deterministic "this text IS json" markers if
one is ever needed. Key order preserved; duplicate keys preserved; unicode preserved; `"` in a key
escaped correctly — all byte-equivalent to DuckDB modulo whitespace.

### 3.2 **The real defect: a numeric correlated scalar subquery becomes a JSON *string***

This is the finding that matters, and it is exactly the shape a graph fetch produces (an object with
a string property and a correlated count).

```sql
SELECT JSON_OBJECT('a', s.name, 'n', (SELECT COUNT(*) FROM proj p WHERE p.emp_id=s.id))
FROM emp s WHERE s.id=1;
-->  {"a": "alice", "n": "2"}          -- WRONG: "2" is a JSON string
-->  DuckDB equivalent:  {"a":"alice","n":2}
```

**Mechanism, isolated by probe:** a scalar subquery's result has `CHARSET(...) = 'binary'`
(`z.subquerycharset`, `x.cs2`). When `JSON_OBJECT` mixes a `binary` numeric with a `utf8mb4` string,
charset aggregation coerces the numeric to text and it is serialized as a JSON string. Confirmed on
`COUNT(*)` and on `SUM(DECIMAL)` (`{"n": "3000.50"}`). It does **not** happen for a plain column
(`JSON_OBJECT('a', s.name, 'n', s.id)` → `{"n": 1}` ✓) nor when there is no string sibling.

**Fix — mandatory, verified in three forms:** wrap the numeric leaf.

| Rewrite | Result |
|---|---|
| `CAST(<subq> AS SIGNED)` / `CAST(<subq> AS DECIMAL(p,s))` | `{"n": 2}` / `{"n": 3000.50}` ✓ |
| `(<subq>) + 0` | `{"n": 2}` ✓ |
| `JSON_EXTRACT(JSON_ARRAY(<subq>), '$[0]')` | `{"n": 2}` ✓ |

**This is a `SqlRewriter` pass, bucket B:** *any numeric-typed `ScalarSubquery` in a `JsonObject`
value position gets an explicit `Cast` to its Phase-G type.* Without it, a Pure `Integer` property
comes back as a JSON string and the frontend mis-types it — silently, on every graph fetch that has
both a string property and a counted association.

### 3.3 The working envelope

Three-level nesting, ordered aggregation, empty collection → `[]`, numeric leaves correct, one
statement, no round trips:

```sql
SELECT CAST(COALESCE(JSON_ARRAYAGG(JSON_OBJECT(
    'name', d.name,
    'n',    CAST((SELECT COUNT(*) FROM emp e WHERE e.dept_id=d.id) AS SIGNED),
    'emps', COALESCE((SELECT JSON_ARRAYAGG(JSON_OBJECT(
                 'name', e.name,
                 'sal',  e.sal,
                 'np',   CAST((SELECT COUNT(*) FROM proj p WHERE p.emp_id=e.id) AS SIGNED)
               ) ORDER BY e.id) FROM emp e WHERE e.dept_id=d.id), JSON_ARRAY())
  ) ORDER BY d.id), JSON_ARRAY()) AS CHAR)
FROM dept d;
```
```json
[{"name": "Eng",   "n": 2, "emps": [{"name":"alice","sal":100.5,"np":2},
                                    {"name":"bob","sal":90.25,"np":0}]},
 {"name": "Sales", "n": 2, "emps": [{"name":"carol",…,"np":1},{"name":"dave",…,"np":0}]},
 {"name": "Empty", "n": 0, "emps": []}]
```

Four contract points, each verified separately:

| Contract | MariaDB | Note |
|---|---|---|
| empty collection → `[]` not NULL | `JSON_ARRAYAGG` over 0 rows returns **NULL** | `COALESCE(…, JSON_ARRAY())` **required at every level** — `ec.nullinner2` without it gives `{"k": null}` |
| NULL elements kept | `JSON_ARRAYAGG(sal)` → `[100.5,90.25,120.75,80,null]` | matches DuckDB's `json_group_array`; MariaDB has **no `ABSENT ON NULL`** — the opposite of H2's default |
| ordered aggregation | `JSON_ARRAYAGG(x ORDER BY k [DESC])` native | also `LIMIT n` inside the aggregate |
| nested stays JSON | ✓ §3.1 | no `JSON_QUERY` wrapper needed |

**`CheckedEnvelope` (§3's `LIST_FILTER(ARRAY[…], x -> x IS NOT NULL)`) — bucket B, but a *different*
B than H2's.** H2 got it free from `JSON_ARRAY(… ABSENT ON NULL)`. MariaDB keeps NULLs
(`[null, {"id":"c2"}]`), so the pass must instead emit either
`JSON_REMOVE(JSON_ARRAY(…), '$[i]', …)` for the statically-known NULL slots — `ge.absentnull4` →
`[{"id": "c2"}]` ✓ — or, cleaner, a `UNION ALL … WHERE v IS NOT NULL` re-aggregation
(`ge.absentnull2` → `[{"id": "c2"}]` ✓). **Note the ordering trap in `JSON_REMOVE`: paths must be
given highest-index-first**, since each removal reindexes.

### 3.4 What the JDBC driver hands back — **the H2 `byte[]` precedent repeats**

| Expression | Java type |
|---|---|
| `JSON_OBJECT('a',1)`, `JSON_OBJECT('a',<string col>)` | `String` |
| `JSON_ARRAYAGG(JSON_OBJECT('id', id))` | `String` |
| `SELECT doc FROM t` where `doc JSON` | `String` |
| `JSON_OBJECT('n', <numeric scalar subquery>)` | **`byte[]`** |
| `JSON_COMPACT(…)`, `CONCAT('a', <numeric subquery>)` | **`byte[]`** |
| `CAST(<anything> AS CHAR)`, `CONVERT(… USING utf8mb4)` | `String` |

Same rule as the value bug: **`CHARSET(result)='binary'` → the driver returns `byte[]`**, and
`Executor.java:118-119`'s `String.valueOf(rs.getObject(1))` would yield `[B@1f3a` — the exact H2
defect recorded in §3. **Fix: wrap the outermost envelope in `CAST(… AS CHAR)`** (verified
deterministic across every shape probed). This is a `SqlDialect.normalize` row *and* a rendering
rule.

---

## 4. Collections / JSON / variant — construct × bucket

`SqlFn` collection surface = 46 entries in this slice. Reference form is DuckDB's spelling.

### 4.1 `LIST_*` (30) — all via `JSON_TABLE` explosion + `JSON_ARRAYAGG` re-aggregation

| SqlFn | Bucket | MariaDB form | probe = DuckDB? |
|---|---|---|---|
| `LIST_CONTAINS` | **C** | `JSON_CONTAINS(arr, val)` | `1` ≡ `true` |
| `LIST_GET` | **C** | `JSON_EXTRACT(arr,'$[i]')` / `JSON_VALUE` | `20` ✓ |
| `LIST_LENGTH` | **C** | `JSON_LENGTH(arr)` | `3` ✓ |
| `LIST_CONCAT` | **C** | `JSON_MERGE_PRESERVE(a,b)` | `[1,2,3]` ✓ |
| `LIST_APPEND` | **C** | `JSON_ARRAY_APPEND(a,'$',v)` | `[1,2,3]` ✓ |
| `LIST_AGG` | **C** | `GROUP_CONCAT(x ORDER BY k SEPARATOR s)` | `alice-bob-…` ✓ |
| `LIST_FILTER` | **B** | `JSON_ARRAYAGG(v) FROM JSON_TABLE(…) WHERE <pred>` | `[20,30]` ✓ |
| `LIST_TRANSFORM` | **B** | `JSON_ARRAYAGG(f(v)) FROM JSON_TABLE(…)` | `[20,40,60]` ✓ |
| `LIST_EXISTS` | **B** | `EXISTS(SELECT 1 FROM JSON_TABLE(…) WHERE p)` | `1`; **`[]`→`0`** ✓ Pure semantics |
| `LIST_FOR_ALL` | **B** | `NOT EXISTS(… WHERE NOT p)` | `1`; **`[]`→`1`** ✓ Pure semantics |
| `LIST_FLATTEN` | **B** | `JSON_TABLE(… NESTED PATH '$[*]' COLUMNS(v …))` | `[1,2,3]` ✓ |
| `LIST_POSITION` | **B** | `MIN(o)-1 FROM JSON_TABLE(… o FOR ORDINALITY …) WHERE v=x` | `1` ✓ |
| `LIST_SUM/MIN/MAX/AVG` | **B** | `SUM/MIN/MAX/AVG(v) FROM JSON_TABLE(…)` | `60`/`10`/`20.0000` ✓ |
| `LIST_MEDIAN` | **B** | `MEDIAN(v) OVER ()` + `DISTINCT`, or the `ROW_NUMBER` construction | `95.375` ✓ |
| `LIST_MODE` | **B** | `GROUP BY v ORDER BY COUNT(*) DESC LIMIT 1` | `1` ✓ |
| `LIST_PRODUCT` | **B** | `EXP(SUM(LN(v)))` | ⚠ `23.999999999999993` — see §8 |
| `LIST_BOOL_AND/OR` | **B** | `MIN(v)`/`MAX(v)` over 0/1 | `0` ✓ |
| `LIST_DISTINCT` | **B** | `JSON_ARRAYAGG(DISTINCT v)` | `[1,2]` ✓ |
| `LIST_SORT` / `_SORT_DESC` | **B** | `JSON_ARRAYAGG(v ORDER BY v [DESC])` | `[1,2,3]`/`[3,2,1]` ✓ |
| `LIST_REVERSE` | **B** | `… ORDER BY o DESC` on `FOR ORDINALITY` | `[2,1,3]` ✓ |
| `LIST_SLICE` / `_TAIL` / `_INIT` | **B** | `WHERE o BETWEEN a AND b` on `FOR ORDINALITY` | `[2,3]` ✓ |
| `LIST_ZIP` | **B** | two `JSON_TABLE`s joined `ON a.o=b.o` | `[[1,"x"],[2,"y"]]` ✓ |
| `LIST_REDUCE` | **B** | recursive CTE (§1.2b) | `123` ✓ |
| `UNNEST` | **B** | `FROM …, JSON_TABLE(<expr>,'$[*]' COLUMNS(v … ))` | ✓ |
| `RANGE_FN` | **C**/B | `seq_M_to_N[_step_K]` (literal bounds); recursive CTE if dynamic | `[1,2,3]`, `[1,4,7,10]` ✓ |

**LIST_\* totals: A 0 · C 6 · B 24 · D 0.**

### 4.2 `MAP_*` (7) — JSON objects

| SqlFn | Bucket | Form | ✓ |
|---|---|---|---|
| `MAP_EMPTY` | **C** | `JSON_OBJECT()` → `{}` | ✓ |
| `MAP_EXTRACT` | **C** | `JSON_VALUE(m,'$.k')` / `JSON_EXTRACT` | ✓ |
| `MAP_KEYS` | **C** | `JSON_KEYS(m)` | `["a","b"]` ✓ |
| `MAP_CONCAT` | **C** | `JSON_MERGE_PATCH(a,b)` | ✓ |
| `MAP_FROM_LISTS` / `_FROM_ENTRIES` | **B** | `JSON_OBJECTAGG(k,v)` over the exploded pair source | `{"a":1,"b":2}` ✓ |
| `MAP_VALUES` | **B** | `JSON_ARRAYAGG(JSON_EXTRACT(m,CONCAT('$.',k)))` over `JSON_TABLE(JSON_KEYS(m),'$[*]')` | `[1,2]` ✓ |

**MAP totals: A 0 · C 4 · B 3 · D 0.** MariaDB has **no `MAP` type** (H2's D5) — but re-encoding as
JSON does *not* re-enter a wall here, because `JSON_TABLE` + `JSON_KEYS` gives key iteration.

### 4.3 Variant / JSON (9)

| SqlFn | Bucket | Form | Note |
|---|---|---|---|
| `JSON_MERGE_PATCH` | **A** | same name, RFC-7386 null-drop confirmed (`{"c":3,"b":2}`) | value ≡ DuckDB |
| `VARIANT_GET` | **C** | `JSON_EXTRACT(v, path)` | **`->` and `->>` DO NOT EXIST** — 4 spellings tried, all parse errors |
| variant→text | **C** | `JSON_UNQUOTE(JSON_EXTRACT(…))` | DuckDB's `->>` |
| `VARIANT_ELEMENTS` | **B** | `JSON_TABLE(v,'$[*]' COLUMNS(…))` | H2's permanent D — **solved here** |
| `TO_VARIANT` | **C** | `JSON_QUOTE(s)` for strings; `JSON_EXTRACT(JSON_ARRAY(x),'$[0]')` for numerics | no `to_json` |
| `JSON_TYPE` | **C**+normalize | `JSON_TYPE(x)` | ⚠ value differs: `'7'` → MariaDB `INTEGER`, DuckDB `UBIGINT`; string → `STRING` vs `VARCHAR` |
| `TYPEOF` | **B** | `INFORMATION_SCHEMA.COLUMNS.DATA_TYPE` (static) | H2's D6, same downgrade argument |
| `IS_DISTINCT` | **B** | `NOT (a <=> b)` | `IS DISTINCT FROM` is a parse error |
| `SourceUrl(data:)` | **B** | `JSON_TABLE('<literal>','$[*]' COLUMNS (data JSON PATH '$'))` | **H2's D4 — solved.** `file:` via `LOAD_FILE()` (returns NULL without `FILE` priv → 0 rows, no error) |

**Variant totals: A 1 · C 4 · B 4 · D 0.**

**Full JSON function census** (all OK unless noted): `JSON_ARRAY` `JSON_OBJECT` `JSON_ARRAYAGG`
`JSON_OBJECTAGG` `JSON_EXTRACT` `JSON_VALUE` `JSON_QUERY` `JSON_LENGTH` `JSON_TYPE` `JSON_CONTAINS`
`JSON_CONTAINS_PATH` `JSON_MERGE_PATCH` `JSON_MERGE_PRESERVE` `JSON_KEYS` `JSON_VALID` `JSON_QUOTE`
`JSON_UNQUOTE` `JSON_DEPTH` `JSON_SEARCH` `JSON_SET` `JSON_INSERT` `JSON_REPLACE` `JSON_REMOVE`
`JSON_ARRAY_APPEND` `JSON_ARRAY_INSERT` `JSON_COMPACT` `JSON_DETAILED` `JSON_PRETTY` `JSON_EXISTS`
`JSON_OVERLAPS` `JSON_EQUALS` `JSON_NORMALIZE` `JSON_TABLE`.
**Absent: `->`, `->>`, `CAST(x AS JSON)`.**

### 4.4 `JSON_TABLE` feature depth (all verified OK)

`FOR ORDINALITY` · `NESTED PATH … COLUMNS(…)` (incl. 2-level) · `EXISTS PATH` ·
`DEFAULT 'v' ON EMPTY` · `NULL ON ERROR` · `JSON`-typed output columns · `VARCHAR(n)`/`INT` columns ·
wildcard `$.*` object iteration · `$[*][*]` deep paths · scalar document `'5'` with `'$'` ·
inside a scalar subquery · inside a select-list subquery · two `JSON_TABLE`s on one row · chained ·
`GROUP BY` over exploded rows · `LEFT JOIN … ON TRUE`.
Edge: a **NULL** document yields 0 rows (use `COALESCE(doc,'[]')` + `LEFT JOIN` to preserve the row —
verified 1 row); a **malformed** document is a hard error, `Syntax error in JSON text in argument 1`.

---

## 5. Sources and joins

| `SqlSource.Join.Kind` | Bucket | Form |
|---|---|---|
| `INNER` | **A** | `JOIN … ON` — 4 ≡ DuckDB 4 |
| `LEFT` | **A** | `LEFT OUTER JOIN` — 5 ✓ |
| `RIGHT` | **A** | `RIGHT OUTER JOIN` — 5 ✓ |
| `CROSS` | **A** | `CROSS JOIN` — 15 ✓ |
| **`FULL`** | **B** | **no `FULL [OUTER] JOIN`** — both spellings parse errors. See below |
| `CROSS_LATERAL` | **B** | `, JSON_TABLE(<corr>, …)` for explosion; window/grouped-join rewrite otherwise |
| `LEFT_LATERAL` | **B** | `LEFT JOIN JSON_TABLE(COALESCE(<corr>,'[]'), …) ON TRUE` |
| `ASOF_LEFT` | **B** | no `ASOF` — needs a correlated `MAX(k) <= …` or `ROW_NUMBER` rewrite |

**FULL OUTER JOIN rewrite — proven multiset-equal to DuckDB's native `FULL`:**
```sql
SELECT … FROM a LEFT OUTER JOIN b ON <p>
UNION ALL
SELECT … FROM a RIGHT OUTER JOIN b ON <p> WHERE a.<notnullkey> IS NULL
```
`fo.rewrite` → `-/3,1/1,2/1,3/2,4/2,5/-` — **byte-identical to DuckDB's `FULL OUTER JOIN`**; row
count 6 ≡ 6. **Use `UNION ALL` + the anti-filter, not `UNION`**: plain `UNION` dedups and silently
drops legitimate duplicate rows. The anti-filter column must be `NOT NULL` on the left side (a
join key), or the rewrite loses rows whose left key is genuinely NULL.

| Other source | Bucket | Note |
|---|---|---|
| `Table` | **A** | |
| `Subselect` | **A** | ⚠ **`ORDER BY` inside a derived table is DROPPED** (`no.obdesc`: MariaDB `1,1,2,2,N`, DuckDB `2,2,1,1,N`). Ordering must live in the outermost query or in a window |
| `Dual` | **A** | `SELECT 1` works; `FROM DUAL` also works (DuckDB rejects `DUAL`) |
| `Values` | **C** | `(VALUES (1,2),(3,4)) AS t` **OK**; **`AS t(a,b)` is a parse error**. Columns are named `1`,`2`,… (`SELECT \`1\` FROM (VALUES (1,2)) AS t` → 1). Rewrite: `(SELECT 1 AS a, 2 AS b UNION ALL SELECT 3,4) AS t` ✓ |
| `Pivot` | **B** | no native `PIVOT`. Conditional aggregation `SUM(CASE WHEN k=v THEN x END)` → `190.75` ≡ DuckDB ✓; `SUM(IF(…))` also works. Dynamic form needs the `Executor` `hasPivot` path + an `INFORMATION_SCHEMA`/pre-query round trip for the value set — that is a `before`-statement, i.e. §8's widened seam |
| `SourceUrl` | **B** | §4.3 |

**Set operations** — every one matches DuckDB exactly:

| Op | MariaDB | ≡ DuckDB |
|---|---|---|
| `UNION` / `UNION ALL` | 5 / 8 | ✓ |
| `INTERSECT` / `INTERSECT ALL` | 3 / 3 | ✓ (MariaDB 10.3 / 10.5) |
| `EXCEPT` / `EXCEPT ALL` | 2 / 2 | ✓ |
| `MINUS` | parse error | DuckDB also rejects — not a divergence |
| trailing `ORDER BY` over the set | `1,2,3,4,5` | ✓ |
| `ORDER BY … LIMIT` over the set | 2 | ✓ |
| per-branch `(SELECT … LIMIT 2) UNION (…)` | 2 | ✓ |
| chained `UNION … EXCEPT` precedence | 4 | ✓ |

`SqlUnion` only models UNION/UNION ALL, so INTERSECT/EXCEPT are headroom, not requirements.

### 5.1 Other query shapes

| Shape | Bucket | MariaDB | ≡ DuckDB |
|---|---|---|---|
| FROM-less `SELECT 1` / `SELECT 1+1 AS a, 'x' AS b` | **A** | ✓ | ✓ |
| `EXISTS` / correlated `EXISTS` | **A** | 4 | ✓ |
| correlated scalar subqueries **3 deep**, and `EXISTS` nested 3 deep | **A** | 2 / 2 | ✓ ✓ |
| correlated scalar subquery **inside `JSON_OBJECT`** | **A** | ✓ (⚠ §3.2 cast) | ✓ |
| `CASE WHEN … ELSE`, no-`ELSE` (→NULL), simple `CASE x WHEN` | **A** | `a` / NULL / `a` | ✓ ✓ ✓ |
| `CAST` | **A**/⚠ | parses everywhere — but **fails silently**, §8 #8 | ✗ value |
| non-recursive CTE, multiple CTEs, CTE referenced twice | **A** | 5 / 3 / 5 | ✓ ✓ ✓ |
| `WITH RECURSIVE` | **A** | 55 | ✓ (bounded at 1000, §9) |
| `WITH RECURSIVE … CYCLE n RESTRICT` | **C** | 6 | DuckDB rejects — MariaDB extra |
| `LIMIT` / `LIMIT…OFFSET` / `LIMIT m,n` / `OFFSET…FETCH NEXT` / `FETCH FIRST` | **A**/C | 2 / 2 / 2 / 2 / 2 | `LIMIT m,n` is MariaDB-only; no `TOP` on either |
| `GROUP BY` ordinal / alias / `HAVING` on an alias | **A** | ✓ / ✓ / 1 | ✓ |
| `(a,b) IN ((1,1),(2,1))` row-constructor `IN` | **A** | 2 | ✓ |
| `(1,2) = (1,2)` row compare | **C** | `1` (Integer) | `true` (Boolean) |
| **`ROW(1,2)` as a value** | **B** | *"Operand should contain 1 column(s)"* — no composite value type | `DuckDBStruct`. Carrier = `JSON_OBJECT` (§7) |
| **`SELECT * EXCEPT (c)` / `EXCLUDE (c)`** | **B** | both parse errors | DuckDB has `EXCLUDE`. Replacement: **enumerate the columns** — `SqlSelect.outputs()` already carries them, so `StarExcept` lowers to an explicit projection list with no catalog round trip. `INFORMATION_SCHEMA.COLUMNS` (verified: `id,name,dept_id,sal,hired,mgr`) is the fallback when the star is untyped |
| `alias.*` qualified star | **A** | ✓ | ✓ |
| `NATURAL JOIN`, `JOIN … USING (c)` | **A** | 0 / 3 | ✓ ✓ |
| `STRAIGHT_JOIN` | **C** | 4 | DuckDB rejects — MariaDB extra |

**Sources/joins totals: A 7 · C 1 · B 6 · D 0** (8 `Join.Kind` + 6 `SqlSource` variants; the §5.1
shapes are counted under their owning `SqlExpr`/`SqlSelect` slice, not re-counted here).

---

## 6. Aggregates and window functions

| Construct | Bucket | Note |
|---|---|---|
| `ROW_NUMBER` `RANK` `DENSE_RANK` `NTILE` `CUME_DIST` `PERCENT_RANK` | **A** | ⚠ `RANK`'s *value* diverges — §8 (NULL ordering) |
| `LAG(x)` `LAG(x,n)` `LEAD` `FIRST_VALUE` `LAST_VALUE` `NTH_VALUE` | **A** | |
| **`LAG(x,n,default)`** (3-arg) | **B** | parse error. `COALESCE(LAG(x,n) OVER (…), default)` → `-1,-1,1,2,3` **≡ DuckDB exactly** |
| `NTH_VALUE(… FROM LAST)` | **B** | parse error (DuckDB rejects too) |
| **`IGNORE NULLS`** | **B** | parse error in every spelling. Rewrite: `LAST_VALUE(x) OVER (… ROWS BETWEEN UNBOUNDED PRECEDING AND CURRENT ROW)` over a NULL-filtered frame, or a filtered self-join |
| `ROWS` frames — `UNBOUNDED PRECEDING` / `n PRECEDING` / `CURRENT ROW` / `n FOLLOWING` / `UNBOUNDED FOLLOWING`, and the short `ROWS n PRECEDING` | **A** | 15/12/15/12/9 — **every value ≡ DuckDB** |
| `RANGE` frames, numeric offsets | **A** | 15/12 ✓ |
| **`RANGE … INTERVAL n UNIT PRECEDING/FOLLOWING`** | **B** | parse error; and even `RANGE 1 PRECEDING` over a `DATE` fails: *"Numeric datatype is required for RANGE-type frame"*. **Rewrite: project the temporal key to a number** — `ORDER BY TO_DAYS(hired) RANGE BETWEEN n PRECEDING …` (✓ 2 and 1) for DAY+ units, `UNIX_TIMESTAMP(ts)` with a second count for sub-day (✓ 1). Covers `Bound.IntervalPreceding`/`IntervalFollowing` |
| **`GROUPS`** frame | **B** | parse error. `Frame.Kind` only models ROWS/RANGE, so this is headroom |
| **frame `EXCLUDE`** | **B** | explicit *"Frame exclusion is not supported yet"*. Not in the IR |
| named windows (`WINDOW w AS (…)`), multiple refs | **A** | 15 / 7 ✓ |
| empty `OVER ()` | **A** | 15 ✓ |
| `DISTINCT` inside an aggregate (`COUNT(DISTINCT x)`) | **A** | 2 ✓; multi-arg `COUNT(DISTINCT a,b)` also OK |
| **`COUNT(DISTINCT …) OVER (…)`** | **B** | explicit *"doesn't yet support COUNT(DISTINCT) aggregate as window function"*. Rewrite: pre-aggregate subquery ✓. ⚠ the `DENSE_RANK asc + DENSE_RANK desc - 1` trick gives **3 vs DuckDB 5** — NULL ordering again; do not use it |
| aggregate `ORDER BY` inside the call | **A** | `GROUP_CONCAT(name ORDER BY id DESC)` → `eve,dave,…` ✓; `JSON_ARRAYAGG(x ORDER BY k)` ✓ |
| `STDDEV_SAMP` `STDDEV_POP` `VAR_SAMP` | **A** | 17.3955 / 15.065 / 302.6042 — **exact match** |
| `BIT_AND` `BIT_OR` `BIT_XOR` (aggregate) | **A** | 0/7/1 ✓ (returns `BigInteger`, DuckDB `Integer`) |
| `MEDIAN`, `PERCENTILE_CONT/DISC` | **B** | **window-only** — under `GROUP BY` they are parse errors. `… OVER ()` + `DISTINCT` → 95.375 ✓, or the `ROW_NUMBER`/`COUNT(*) OVER ()` construction → 95.375 ✓ |
| `ANY_VALUE` | **B** | **does not exist** on 11.4.5 (`FUNCTION probe.ANY_VALUE does not exist`). Use `MIN`/`MAX` |
| `GROUP BY … WITH ROLLUP` | **C** | ✓ 4 (DuckDB uses `GROUPING SETS`) |
| `GROUPING SETS` | **B** | parse error |
| **`QUALIFY`** | **B** | **absent, confirmed.** `QualifyToSubselect` (already in `passes()`) covers it — the rewrite returns `1` ≡ DuckDB. Unlike H2, this pass is a *requirement*, not a free fallback |

**Aggregates/windows totals: A 10 · C 1 · B 11 · D 0** (22 rows above).

---

## 7. Types — the `SqlDialect.normalize` specification

| `SqlType` | Has it? | MariaDB DDL | **JDBC Java type** | DuckDB returns | normalize needed |
|---|---|---|---|---|---|
| `BOOLEAN` (column) | alias | `BOOLEAN` → stored `TINYINT(1)` | **`Boolean`** | `Boolean` | no |
| `BOOLEAN` (**expression**) | — | `1=1`, `TRUE`, `NOT(…)`, `IS NULL` | **`Integer` (1/0)** | `Boolean` | **YES — the big one** |
| `INTEGER` | ✓ | `INTEGER`/`INT` | `Integer` | `Integer` | no |
| `BIGINT` | ✓ | `BIGINT` | `Long` | `Long` | no |
| `HUGEINT` | **no** | — (`HUGEINT`/`INT128` unknown) | fallback `DECIMAL(39,0)` → **`BigDecimal`** | `BigInteger` | **YES** |
| `DOUBLE` | ✓ | `DOUBLE PRECISION` | `Double` | `Double` | no |
| `VARCHAR` | ✓ | `VARCHAR(n)` (max 65535 bytes/row) / `TEXT` | `String` | `String` | no |
| `DATE` | ✓ | `DATE` | **`java.sql.Date`** | **`LocalDate`** | **YES** |
| `TIMESTAMP` | ✓ | **`DATETIME`** (see below) | `java.sql.Timestamp` | `Timestamp` | no |
| `TIMESTAMPTZ` | **no** | `TIMESTAMP` is UTC-normalized (≈tz-aware); `TIMESTAMP WITH TIME ZONE` is a parse error | `Timestamp` | `Timestamp`/`OffsetDateTime` | **YES** |
| `JSON` | **alias only** | `JSON` accepted → stored **`longtext`** + auto `CHECK (json_valid(...))` | **`String`**, or **`byte[]`** when `CHARSET(result)='binary'` | `JsonNode` | **YES** |
| `Decimal(p,s)` | ✓ | `DECIMAL(p,s)`, **max p=65, max s=38** | `BigDecimal` ✓ scale preserved | `BigDecimal` | no |
| `Array(T)` | **no** | `INTEGER ARRAY` / `INT[]` both parse errors | carrier = JSON text → `String` | `Array` | **YES** |
| `Map(K,V)` | **no** | `MAP(…)` parse error | carrier = JSON object text → `String` | `DuckDBMap` | **YES** |
| `Struct(fields)` | **no** | `STRUCT(…)` parse error; `ROW(1,2)` as a value → *"Operand should contain 1 column(s)"* | carrier = JSON object text → `String` | `DuckDBStruct` | **YES** |
| — | | `TINYINT` | **`Integer`** | `Byte` | **YES** |
| — | | `SUM(int)` | **`BigDecimal`** | `BigInteger` | **YES** |
| — | | `AVG(int)` | **`BigDecimal`** (scale 4: `3.0000`) | `Double` | **YES** |
| — | | `COUNT(*)` | `Long` | `Long` | no |
| — | | `BIT_OR`/`BIT_XOR` agg | `BigInteger` | `Integer` | **YES** |
| — | | `CEIL(2.1)` | `Integer` | `BigDecimal` | **YES** |

### 7.1 Notes the table cannot hold

- **`BOOLEAN` is `TINYINT(1)`** and the split is subtle: a *declared* `BOOLEAN` **column** comes back
  as `Boolean` (the driver reads the display width), but every boolean **expression** —
  `1=1`, `TRUE`, `NOT(…)`, `x IS NULL`, `MAX(x=1)` — comes back as **`Integer` 1/0**. Both are on the
  graph-fetch path. Inside JSON it is correct: `JSON_OBJECT('b',(1=1))` → `{"b": true}` ✓.
- **`JSON` costs exactly one thing: it is not a cast target.** `CAST(x AS JSON)` and
  `CONVERT(x, JSON)` are parse errors, so DuckDB's `CAST(v AS JSON[])` (`variantElements`) and
  `JSON '[]'` typed literals have no counterpart — use `JSON_EXTRACT(x,'$')` / `JSON_ARRAY()`.
  Storage-wise the alias is nearly free: the auto-generated `CHECK (json_valid(...))` **is enforced**
  (inserting `'not json'` fails with `CONSTRAINT 'jsontab.doc' failed`), so validity is not lost —
  only the in-memory binary representation and any type-directed indexing are.
- **`HUGEINT` fallback is `DECIMAL(39,0)`, and it is *wider* than DuckDB's INT128, not narrower.**
  `99999999999999999999999999999999999999 * 10` → `999999999999999999999999999999999999990` on
  MariaDB, while **DuckDB overflows** (`Out of Range Error: Overflow in multiplication of
  DECIMAL(38)`). Precision ceiling is 65 digits (`DECIMAL(66,0)` → *"Too big precision … Maximum is
  65"*). The semantic mismatch is not range but *kind*: `DECIMAL` division yields decimals and never
  wraps, where INT128 truncates and wraps. `BIGINT` itself **errors** on overflow
  (`BIGINT value is out of range`), matching DuckDB's loudness.
- **`TIMESTAMP` vs `DATETIME` — pick `DATETIME` for `SqlType.TIMESTAMP`.** Measured with a value
  written under `time_zone=SYSTEM` and read back:

  | session `time_zone` | `TIMESTAMP` col | `DATETIME` col |
  |---|---|---|
  | `+00:00` | `2020-06-15 16:00:00` | `2020-06-15 12:00:00` |
  | `+05:00` | `2020-01-15 20:20:30` | `2020-01-15 10:20:30` |

  `TIMESTAMP` is stored as UTC and re-rendered in the session zone — it is the closest thing MariaDB
  has to `TIMESTAMPTZ`, and it is **wrong for a naive `TIMESTAMP`**. `DATETIME` is zone-inert and is
  the correct carrier for `SqlType.TIMESTAMP`; `TIMESTAMP` is the correct carrier for
  `SqlType.TIMESTAMPTZ`. Either way **the session `time_zone` must be pinned**, exactly as §7 pins
  H2's `timeZone='GMT'`. `CONVERT_TZ` works with **offsets** (`'+00:00'→'+05:00'` ✓) but returns
  **NULL for named zones** (`'UTC'→'America/New_York'`) because the `mysql.time_zone*` tables are not
  loaded on this build — a silent NULL, not an error.

---

## 8. Silent value divergence — exhaustive, both values

Ordered by how badly each would corrupt a result. Every row is measured.

| # | Construct | **MariaDB** | **DuckDB** | Severity |
|---|---|---|---|---|
| 1 | `'a' \|\| 'b'` | **`0`** (`\|\|` is logical OR) | `'ab'` | **silent wrong answer** — the `//` class of `H5.2`. Fixed by `PIPES_AS_CONCAT` (verified → `'ab'`) or by rendering `CONCAT` |
| 2 | `SELECT "id" FROM emp` | **`'id'`** — a **string literal**, not the column | column value | **silent wrong answer.** `AnsiSqlRenderer` quotes identifiers with `"`. Fixed by `ANSI_QUOTES` (verified) or by using backticks |
| 3 | `JSON_OBJECT('a',s.name,'n',(SELECT COUNT(*)…))` | `{"a":"alice","n":**"2"**}` | `{"a":"alice","n":2}` | **silent wrong type in the graph payload** — §3.2 |
| 4 | `'a' = 'A'` | **`1`** (`utf8mb4_general_ci`) | `false` | **silent wrong answer.** Affects `=`, `IN`, `LIKE`, `DISTINCT`, `GROUP BY`, join keys |
| 5 | `'a ' = 'a'` | **`1`** (PAD SPACE) | `false` | **silent wrong answer.** This is H2's forked `charPadding = NEVER` (§7) arriving on a second backend |
| 6 | `'ABC' LIKE 'abc'` | **`1`** | `false` | as #4 |
| 7 | `ORDER BY x` with NULLs | **NULLS FIRST** asc / NULLS LAST desc | NULLS LAST both | changes `RANK`: `MAX(RANK() OVER (ORDER BY dept_id))` → **4** vs **5** |
| 8 | `CAST('abc' AS SIGNED)` | **`0`** + warning | **error** | **silent wrong answer.** Also `CAST('abc' AS DECIMAL)`→`0.00`, `CAST('12abc' AS SIGNED)`→`12`, `CAST('abc' AS DATE)`→`NULL`. `STRICT_TRANS_TABLES` does **not** change SELECT-position casts |
| 9 | `1/0` | **`NULL`** | `Infinity` | divergent |
| 10 | `7/2` | **`3.5000`** `BigDecimal` | `3.5` `Double` | `SqlFn.DIVIDE` "forces float" — **must render `CAST(a AS DOUBLE)/b`** (verified `0.3333333333333333` ✓) |
| 11 | `1/3` | **`0.3333`** (4 dp — `div_precision_increment`) | `0.3333333333333333` | as #10. `div_precision_increment` is settable (30 → 30 dp) but the CAST is the right fix |
| 12 | `ROUND(2.5)` on `DOUBLE` | **`2.0`** (half-even) | `3.0` | on `DECIMAL` both give `3` / `4` / `-3` — the divergence is **DOUBLE-only** |
| 13 | `GREATEST(1,NULL,3)` / `LEAST` | **`NULL`** | `3` / `1` | MariaDB is null-propagating; H2 skipped nulls — **three different answers across three backends** |
| 14 | `CONCAT('a',NULL)` | **`NULL`** | `'a'` | divergent |
| 15 | `SUBSTRING('hello',0,3)` | **`''`** | `'he'` | 0-index divergence |
| 16 | `SUM(int)` | `BigDecimal:15` | `BigInteger:15` | type-only |
| 17 | `AVG(int)` | `BigDecimal:3.0000` | `Double:3.0` | type + scale |
| 18 | `JSON_TYPE('7')` | `INTEGER` | `UBIGINT` | value-only |
| 19 | `EXP(SUM(LN(v)))` for `LIST_PRODUCT` | **`23.999999999999993`** | `24` | float-error divergence introduced by *our* rewrite — prefer a recursive-CTE product |
| 20 | `SELECT name, COUNT(*) FROM emp` | **`'alice'`** — no `ONLY_FULL_GROUP_BY` | error | **silent wrong answer**; fixed by the sql_mode (verified → error) |
| 21 | `ORDER BY` inside a derived table | **dropped** | honoured | ordering must be outermost |
| 22 | `LENGTH('a\nb')` | **`3`** (backslash escapes active) | `4` | string-literal rendering; fixed by `NO_BACKSLASH_ESCAPES` (verified → `4`) |
| 23 | `ORDER BY` on strings | `'a'` before `'B'` (ci) | `'B'` before `'a'` | as #4 |
| 24 | `JSON_OBJECT('n', DECIMAL 1.10)` | `{"n": 1.10}` | `{"n":1.1}` | textual only |
| 25 | driver type for a `binary`-charset string | **`byte[]`** | n/a | §3.4 — `[B@…` through `String.valueOf` |

**Collation is the deepest of these.** Session `collation_connection` is **not enough**: setting it
to `utf8mb4_bin` fixed literal-vs-literal (`'a'='A'` → 0) but **`WHERE name='ALICE'` still matched**
(1 row) because the *column's* collation wins by coercibility. The fix must be at DDL or server
scope — `CREATE TABLE cs (s VARCHAR(20) COLLATE utf8mb4_nopad_bin)` gives `'ALICE'`→0 rows **and**
`'a '='a'`→0 rows (both verified). **Decide this explicitly** — either pin
`collation_server=utf8mb4_nopad_bin` (or `utf8mb4_bin` if PAD SPACE is acceptable) on the connection
URL / server args, or emit `COLLATE` on every string column in `Ddl`. This is `H2_BACKEND.md` §7's
"H2 compatible does not mean stock H2" repeating verbatim on MariaDB, and it must not be discovered
as mysterious row divergence.

**Explicit `NULLS FIRST/LAST` is a parse error.** Verified rewrite:
`NULLS LAST` → `ORDER BY expr IS NULL, expr` · `NULLS FIRST` → `ORDER BY expr IS NOT NULL, expr`.
Applied to the window ORDER BY it restores value equality with DuckDB (`RANK` 4 → **5** ≡ 5).

---

## 9. Capability budget (`H2_BACKEND.md` §8 shape)

| Budget | H2's value | **MariaDB 11.4.5's value** |
|---|---|---|
| `aliasLimit` | 256 | **64** for any DDL identifier (65 → *"Identifier name … is too long"*). SELECT column/table aliases accepted at ≥257 without error, so 64 is the binding constraint |
| `collectionThresholdLimit` | not set | **not set** — the IN→temp-table rewrite is unnecessary; `JSON_TABLE` over a single JSON parameter is strictly better. Statement text is bounded by `max_allowed_packet` |
| `supportsFullOuterJoin` | false | **false** (§5 rewrite) |
| `supportsBooleanLiteral` | true | **true** — `TRUE`/`FALSE` parse, but read back as `Integer` |
| `limitStyle` | `top N` / `offset…fetch` | **`LIMIT n OFFSET m`** (native, DuckDB-identical). Also `LIMIT m, n` and `OFFSET m ROWS FETCH NEXT n ROWS ONLY`. **No `TOP`.** Offset-only needs `LIMIT 18446744073709551615 OFFSET n` |
| `nullOrdering` | explicit | **implicit-and-inverted** — NULLS FIRST on ASC, NULLS LAST on DESC, and `NULLS FIRST/LAST` **is a parse error**. Needs the `expr IS NULL` rewrite, not just a flag |
| *new* `maxRecursion` | — | **1000** (`max_recursive_iterations`) — bounds every recursive-CTE collection rewrite |
| *new* `decimalMaxPrecision` | — | **65** (scale ≤ 38) |
| *new* `requiredSqlMode` | `MODE=LEGACY` + NON_KEYWORDS | **`PIPES_AS_CONCAT,ANSI_QUOTES,NO_BACKSLASH_ESCAPES,ONLY_FULL_GROUP_BY,STRICT_TRANS_TABLES`** — all four of the first verified to fix a silent wrong answer |
| *new* `requiredCollation` | H2 `charPadding=NEVER` fork | **`utf8mb4_nopad_bin` at server/DDL scope** — session scope is insufficient (§8) |
| *new* `sessionTimeZone` | `'GMT'` | **must be pinned**; and `SqlType.TIMESTAMP` → `DATETIME`, not `TIMESTAMP` |

**Reserved words measured:** `over` requires quoting (`SELECT 1 AS over` → parse error); `rank` does
**not**. Quote char is **backtick** unless `ANSI_QUOTES` is set — under `ANSI_QUOTES`, `"` becomes an
identifier quote and `SELECT "abc"` correctly errors, so `Lexicon.MariaDB.quoteChar()` can stay `"`
**only if** the session mode is guaranteed. Given `H5.4` (dialect and connection never reconciled),
backticks are the safer default.

---

## 10. Bucket totals

| Group | A | C | B | D | Total |
|---|---|---|---|---|---|
| `LIST_*` + `UNNEST` + `RANGE_FN` (§4.1) | 0 | 6 | 24 | **0** | 30 |
| `MAP_*` (§4.2) | 0 | 4 | 3 | **0** | 7 |
| Variant / JSON / misc (§4.3) | 1 | 4 | 4 | **0** | 9 |
| Sources & joins (§5) | 7 | 1 | 6 | **0** | 14 |
| Aggregates & windows (§6) | 10 | 1 | 11 | **0** | 22 |
| Types (§7) | 6 | 3 | 5 | **0** | 14 |
| **Total (this slice)** | **24** | **19** | **53** | **0** | **96** |

**25% native · 20% rendering override · 55% rewrite pass · 0% impossible.**

Type buckets: **A** `INTEGER` `BIGINT` `DOUBLE` `VARCHAR` `DATE` `Decimal` · **C** `BOOLEAN`
(`TINYINT(1)` alias) `TIMESTAMP` (→`DATETIME`) `JSON` (→`LONGTEXT`) · **B** `HUGEINT` `TIMESTAMPTZ`
`Array` `Map` `Struct`.

Compare H2's whole-vocabulary shape (54% A, 25% C, 1% B, **19% D**). MariaDB inverts it: **the B
column carries the load and the D column is empty.** That is the difference between "a dialect plus
a redesign that may not be enough" and "a dialect plus the redesign §4.1 already prescribes."

Every bucket here was earned: no construct was assigned **D** without at least three candidate
spellings failing, and every **B** in this document has a probe with a value compared against DuckDB.

---

## 11. MariaDB vs MySQL — the divergence note

Established **by execution** on MariaDB 11.4.5 in this probe:

| Feature | MariaDB 11.4.5 (**executed**) | MySQL 8 (**documented, NOT executed here — verify before relying on it**) |
|---|---|---|
| **`JSON_TABLE`** | **PRESENT** (since 10.6) | present (since 8.0.4) |
| **`LATERAL`** | **ABSENT** — every form a parse error | present (8.0.14+) |
| `->` / `->>` | **ABSENT** — 4 spellings tried | present |
| `CAST(x AS JSON)` | **ABSENT** | present |
| `JSON` type | **`LONGTEXT` alias + `json_valid` CHECK** | native binary type |
| `INTERSECT` / `EXCEPT` | **present** (10.3/10.5) | present (8.0.31) |
| `SEQUENCE` engine (`seq_1_to_N`) | **present** | absent |
| `JSON_COMPACT` / `JSON_DETAILED` / `JSON_EQUALS` / `JSON_NORMALIZE` | **present** | absent |
| `JSON_ARRAYAGG(… ORDER BY …)` / `LIMIT` inside | **present** | not supported |
| `LAG(x,n,default)` 3-arg | **ABSENT** | present |
| `GROUPS` frame, frame `EXCLUDE` | **ABSENT** | absent |
| `COUNT(DISTINCT) OVER` | **ABSENT** (explicit message) | absent |
| `ANY_VALUE` | **ABSENT** | present |
| `FULL OUTER JOIN` | **ABSENT** | absent |

**The headline:** the two features usually cited to separate them go in *opposite* directions.
MySQL has `LATERAL` and MariaDB does not; both have `JSON_TABLE`. Since **`JSON_TABLE` is the one
that carries the collection story and `LATERAL` is the one with cheap window/grouped-join rewrites,
MariaDB is the *better* of the two for legend-lite's collection carrier** — and a single
`Spellings.MariaDB` cannot be reused for MySQL (`->`, `CAST … AS JSON`, `ANY_VALUE`,
3-arg `LAG`, and ordered `JSON_ARRAYAGG` all differ). **They are two dialects, not one.**

---

## 12. What to record in the sequencing plan

1. **The `SqlDialect.normalize` rows are now concrete** (§7) — `BOOLEAN` expression → `Integer`,
   `DATE` → `java.sql.Date`, JSON → `String`/**`byte[]`**, `SUM`/`AVG` → `BigDecimal`,
   `TINYINT` → `Integer`, array/map/struct → JSON text.
2. **Two new mandatory `SqlRewriter` passes**, both bucket B and both correctness-critical:
   (a) cast numeric `ScalarSubquery` in `JsonObject` value position (§3.2);
   (b) `NULLS FIRST/LAST` → `expr IS [NOT] NULL, expr` (§8 #7).
3. **`CAST(<envelope> AS CHAR)` on the outermost graph expression** — otherwise `byte[]`.
4. **Session/DDL configuration is a first-class decision** (§9), same shape as H2's §7 fork, and
   `PIPES_AS_CONCAT` / `ANSI_QUOTES` / collation each fix a *measured* silent wrong answer.
5. **`QualifyToSubselect` becomes load-bearing**, not a fallback (H2 had native `QUALIFY`).
6. **The §4.1 collection-carrier redesign should be re-prioritised.** Keep the deferral, but it now
   unblocks MariaDB, Postgres and Snowflake with one relational lowering — not just H2's last 19%.
