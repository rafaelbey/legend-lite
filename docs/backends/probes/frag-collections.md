# Collections / maps / variant / lambdas / UNNEST — SQLite + Postgres capability probe

**Evidence:** 443 probe statements per backend, executed in 8 iterated batches against real
`duckdb 1.5.0.0`, `sqlite-jdbc 3.47.1.0` (SQLite 3.47.1) and **real PostgreSQL 18.4** (zonky embedded).
Raw output: `probe/all-{duckdb,sqlite,postgres}-collections.tsv` (per-batch: `b1-…`…`b8-…`).
Own-flavour success: duckdb 98/98, sqlite 126/128, postgres 135/145.
Documentation was used only to generate candidate spellings; **every claim below was executed**.

---

## 1. THE LATERAL VERDICT — lead with this

> **H2's D1 does not transfer. Both SQLite and Postgres have correlated explosion.**
> The whole `LIST_*` family, `UNNEST`, `VARIANT_ELEMENTS`, `RANGE_FN`, `CROSS_LATERAL` and
> `LEFT_LATERAL` are **reachable by rewrite on both targets**. Nothing in this slice is `D` on
> either backend for the reason H2 is `D`.

H2_BACKEND.md §4 D1 says: *"`LATERAL` is not a keyword … every correlated FROM form fails."* That is
a fact about H2 and **only** about H2. Measured on the two real targets:

| Correlated form | probe | Postgres 18.4 | SQLite 3.47.1 |
|---|---|---|---|
| `FROM t, LATERAL unnest(t.arr) u(v)` | `LAT.pg1` | **OK** `30` | n/a (no ARRAY) |
| `CROSS JOIN LATERAL unnest(t.arr)` | `LAT.pg2` | **OK** `30` | n/a |
| `LEFT JOIN LATERAL unnest(t.arr) ON TRUE` | `LAT.pg3` | **OK** `30` | n/a |
| implicit `FROM t, unnest(t.arr)` (LATERAL keyword omitted) | `LAT.pg4` | **OK** `30` | n/a |
| correlated TF inside a **scalar subquery** | `LAT.pg5/pg8` | **OK** `60` / `1` | — |
| correlated `generate_series(1, t.n)` | `LAT.pg9` | **OK** `3` | — |
| select-list `UNNEST` (`SELECT unnest(arr) FROM t`) | `LAT.pg7`,`SLU.pg1/2` | **OK** `10`, `3`, `10` | n/a |
| select-list **parallel** unnest, ragged | `SLU.pg3` | **OK** `3` (short arm NULL-padded) | n/a |
| row-correlated over the real 5-row `emp` | `LAT.x1` | **OK** `10` | — |
| **`FROM t, json_each(t.arr) j`** (correlated arg) | `LAT.sq1` | — | **OK** `30` |
| `CROSS JOIN json_each(t.arr)` | `LAT.sq2` | — | **OK** `30` |
| `LEFT JOIN json_each(t.arr) ON 1=1` | `LAT.sq3` | — | **OK** `30` |
| correlated `json_each` in a **scalar subquery** | `LAT.sq4/sq9` | — | **OK** `60` / `1` |
| correlated arg = **expression over the outer row** (`json_array(e.id,e.dept_id)`) | `LAT.sq7`,`CORX.sq2` | — | **OK** `10` |
| correlated arg = **string concat** over outer row | `CORX.sq1` | — | **OK** `10` |
| **two** outer tables feeding one TF | `CORX.sq4` / `CORX.pg2` | **OK** `30` | **OK** `30` |
| TF in a scalar subquery correlated to a real table column | `SSQ.pg` / `SSQ.sq` | **OK** `9` | **OK** `9` |

### SQLite specifically — exactly which positions are legal

The question was "SQLite table-valued functions accept correlated arguments in *some* positions;
establish exactly which." **Answer: every position probed.** `json_each(X)` accepts a correlated `X`
in the comma-join, in `CROSS JOIN`, in `LEFT JOIN … ON`, and inside a correlated scalar subquery, and
`X` may be an arbitrary expression over one or several outer tables. The only constraints found are
value-level, not placement-level:

- `json_each(e.name)` → `[SQLITE_ERROR] malformed JSON` (`CORX.sq3`). The argument must be valid
  JSON text — a **carrier-discipline** requirement, not a correlation limit.
- `json_each(NULL)` → **0 rows, no error** (`SCAL.sq2`). Null-safe.
- `json_each('5')` on a JSON scalar → **1 row** (`SCAL.sq1`). Postgres `jsonb_array_elements('5')`
  **errors** — `cannot extract elements from a scalar` (`SCAL.pg1`). Divergence, see §6.
- SQLite has **no `AS t(c1,c2)` column-alias list** (`SQJ.aliascols` ERR; `LAT.pg1` fails on
  `near "AS"`). Table-function columns are addressed by their fixed names
  (`key,value,type,atom,id,parent,fullkey,path` — all eight confirmed present, `SQJ.eachcols`).

### `LEFT LATERAL` semantics survive on both

The load-bearing case for graph fetch is *the outer row must survive an empty collection*. Verified
against the real fixture, on a genuine array **column** built by aggregation:

| | inner join | left join |
|---|---|---|
| Postgres `…, LATERAL unnest(t.arr)` vs `LEFT JOIN LATERAL … ON TRUE` | `EXPL.pg1` = **3** | `EXPL.pg2` = **6** |
| SQLite `…, json_each(t.arr)` vs `LEFT JOIN json_each(t.arr) ON 1=1` | `EXPL.sq1` = **3** | `EXPL.sq2` = **6** |
| DuckDB reference | `EXPL.dd1` = **3** | — |

6 = 3 exploded project rows + the 3 employees whose project list is empty, preserved.
`CROSS_LATERAL` and `LEFT_LATERAL` are therefore **A/C, not D**, on both targets.

### The full round trip works in situ

The decisive end-to-end shape — build an array **column** by aggregation, correlate-explode it,
filter it, re-aggregate back into the carrier, per row — returns the **identical value on all three
backends**:

```
COL.dd / COL.pg / COL.sq  →  "2||3||"      (list_filter(v -> v > 1) per employee)
RT.dd  / RT.pg  / RT.sq   →  "2||3||"      (explode → filter → re-aggregate round trip)
```

That is `LIST_FILTER` over a row-correlated collection, executed, matching DuckDB, on both targets.

---

## 2. THE CARRIER VERDICT (question B)

### What each backend actually has

| | Native array type | Array literal | JDBC read-back | Jagged `List<List<T>>` | JSON |
|---|---|---|---|---|---|
| DuckDB | `LIST` | `[1,2,3]` **and** `ARRAY[1,2,3]` | `java.sql.Array` | **yes** (`NEST.dd2` len=2) | `JSON` → `JsonNode` |
| **Postgres** | `int[]`, real | `ARRAY[1,2,3]` ✅ (`[1,2,3]` is a **syntax error**) | `java.sql.Array` — **same as DuckDB** | **NO** — see below | `json`/`jsonb` → `PGobject` |
| **SQLite** | **none at all** (`SQA.lit`/`SQA.brk` both ERR) | none | — | via JSON only | JSON1 **fully present**, `json`→`String`, `jsonb`→`byte[]` |

### SQLite JSON1 census — probed, not assumed

`sqlite_version()` = **3.47.1**. Every JSON1 function probed is **present and correct**:
`json_array`, `json_object`, `json_each` (8 columns), `json_tree`, **`json_group_array`**,
**`json_group_object`**, `json_extract`, `->`, `->>` (both path and integer-index forms),
`json_type` (bare and with path), `json_valid`, `json_quote`, `json_patch`, `json_insert`,
`json_set`, `json_remove`, `json_array_length`, `json_error_position`.
**The SQLite 3.45+ `jsonb_*` family is also compiled in**: `jsonb()`, `jsonb_array`,
`jsonb_group_array`, `jsonb_extract`, `jsonb_object` all returned correct values (`SQJ.jsonb*`).
Bonus finding: **`median()` is a native aggregate in this build** (`MEDIAN.sq`=3.0, `MEDE.sq`=1.5),
and **in-aggregate `ORDER BY` works** (`IAO.sq1` — SQLite 3.44+), so
`group_concat(name, ',' ORDER BY id DESC)` is legal.

### The Postgres surprise: arrays are RECTANGULAR, not jagged

This is the single most important carrier finding on Postgres and it is easy to miss, because
`array_agg` and `unnest` make Postgres arrays *look* like DuckDB lists until you nest them.

| probe | statement | result |
|---|---|---|
| `NEST.pg1` | `ARRAY[ARRAY[1,2],ARRAY[3]]` | **ERR** `multidimensional arrays must have array expressions with matching dimensions` |
| `NEST.pg8` | `array_agg(a)` over rows whose `a` differ in length | **ERR** `cannot accumulate arrays of different dimensionality` |
| `NEST.pg3` | `cardinality(ARRAY[ARRAY[1,2],ARRAY[3,4]])` | **4**, not 2 — `LIST_LENGTH` of a `List<List<T>>` is *wrong*, silently |
| `NEST.pg6` | `(ARRAY[ARRAY[1,2],ARRAY[3,4]])[1]` | **NULL** — `LIST_GET` of a sublist returns NULL, silently |
| `NEST.pg5` | `unnest(ARRAY[ARRAY[1,2],ARRAY[3,4]])` | **4 scalars** — `UNNEST` flattens *all* levels, not one |

A Pure `List<List<T>>` is ragged in general. **Postgres `ARRAY` cannot represent it**, and three of the
five failure modes above are *silent wrong answers*, not errors. `jsonb` handles it correctly
(`NEST.pg9/10/11`, `FLAT.pg` = `1,2,3`).

### Verdict

**One carrier serves all three — JSON — and it is NOT the redesign H2 needs. §4.1 is right about the
diagnosis and wrong about the remedy.**

H2_BACKEND.md §4.1 says the fix is to lower collections **to relations** — "a correlated derived
table joined on a synthetic key, or an ordinal-keyed side table … a Phase-H/I redesign of
`Scalars.java` + `Fold.java` + `ListShapes.java`" — and justifies it as *"the right redesign for N
backends generally, since Postgres, Snowflake and BigQuery each need a different one of
unnest-lateral / array functions / lambdas."*

**Agree** that `SqlType.Array` + DuckDB list lambdas is not portable, and that the collection carrier
(not the graph layer) is the axis of the problem.

**Disagree on three counts, with evidence:**

1. **The relational lowering is not needed for these two backends.** It is needed *because H2 has no
   correlated table function*. Postgres and SQLite both do (§1), so the collection can stay a
   **value** and be exploded at the point of use. `RT.pg`/`RT.sq` prove the value-carrier round trip
   executes and matches DuckDB. A relations-only redesign would be paying H2's price on backends that
   don't owe it.

2. **"Postgres needs array functions, SQLite needs json_each, DuckDB needs lambdas" is three
   carriers where one suffices.** The probe shows a *single* JSON carrier expressible on all three:
   `json_group_array` / `json_agg` to build, `json_each` / `jsonb_array_elements` / `unnest(…::json[])`
   to explode, JSON text to store. Under that carrier the graph-fetch envelope is byte-identical
   between DuckDB and SQLite (§5), and the `LIST_*` constructions are the *same shape* on both
   targets — explode-to-rows, do relational work, re-aggregate. That is one rewrite pass
   parameterised by three spellings, not three redesigns.

3. **Choosing Postgres's native `ARRAY` as the Postgres carrier is a trap, not an optimisation.**
   It looks free (identical `ARRAY[…]` literal, identical `java.sql.Array` read-back, richer function
   set than SQLite) and it is free *until* `List<List<T>>`, where it produces silent wrong answers
   (`NEST.pg3/pg5/pg6` above). If native arrays are used at all, the type system must be able to say
   "this collection is nested" and fall back to `jsonb` — i.e. **two carriers on one backend**, which
   is strictly worse than one.

**Recommended carrier:** JSON as the *universal* representation of `SqlType.Array`, with native
arrays permitted as a **flat-only, depth-1 optimisation** on Postgres and DuckDB — never as the
declared carrier. The relational lowering of §4.1 then stays what it should be: **H2's** compatibility
mode, entered only for a backend that lacks correlated explosion, rather than the shared architecture.

---

## 3. THE LAMBDA FAMILY — lambda-free constructions, proved

Neither target has lambda syntax. Confirmed rather than assumed: on Postgres `x -> x+1` parses as the
JSON operator and fails `operator does not exist: integer -> integer` (`LAM.pg1`); `list_filter(…, x -> …)`
fails `column "x" does not exist` (`LAM.pg2`). On SQLite `list_filter` is `no such function` and
`[1,2]` is a syntax error (`LAM.sq1`, `LAM.dd`).

All five constructions below **executed and matched the DuckDB value**.

| SqlFn | Postgres construction | SQLite construction |
|---|---|---|
| `LIST_FILTER` | `ARRAY(SELECT t.v FROM unnest(:a) WITH ORDINALITY t(v,o) WHERE :p ORDER BY t.o)` | `(SELECT json_group_array(j.value) FROM json_each(:a) j WHERE :p)` |
| `LIST_TRANSFORM` | `ARRAY(SELECT :f(t.v) FROM unnest(:a) WITH ORDINALITY t(v,o) ORDER BY t.o)` | `(SELECT json_group_array(v) FROM (SELECT :f(j.value) AS v FROM json_each(:a) j ORDER BY j.key))` |
| `LIST_EXISTS` | `EXISTS(SELECT 1 FROM unnest(:a) x WHERE :p)` | `EXISTS(SELECT 1 FROM json_each(:a) j WHERE :p)` |
| `LIST_FOR_ALL` | `NOT EXISTS(SELECT 1 FROM unnest(:a) x WHERE NOT (:p))` | `NOT EXISTS(SELECT 1 FROM json_each(:a) j WHERE NOT (:p))` |
| `LIST_REDUCE` / `FoldCall` | recursive CTE over `WITH ORDINALITY`, seed cast to `bigint` | recursive CTE over `json_each.key` |

### Empty-collection semantics — the classic silent bug, verified

`SqlFn.java` requires `exists([]) = false`, `forAll([]) = true`. Probed explicitly:

| case | DuckDB | Postgres | SQLite |
|---|---|---|---|
| `exists([10,20,30], x>25)` | `true` | `true` (`EXISTS.pg`) | `1` (`EXISTS.sq`) |
| `exists([10,20], x>25)` | `false` | `false` (`EXISTSF.pg`) | `0` |
| **`exists([], x>25)`** | `false` | **`false`** (`EXISTSE.pg`) | **`0`** (`EXISTSE.sq`) ✅ |
| `forAll([30,40], x>25)` | `true` | `true` | `1` |
| `forAll([10,40], x>25)` | `false` | `false` | `0` |
| **`forAll([], x>25)`** | `true` | **`true`** (`FORALLE.pg`) | **`1`** (`FORALLE.sq`) ✅ |

The `EXISTS` / `NOT EXISTS` encoding gets both boundary cases right **for free** — no `coalesce`
guard needed, unlike the DuckDB `list_bool_or`/`list_bool_and` encoding which returns NULL on an
empty list and needs `coalesce(…,false)` / `coalesce(…,true)` (see `EXISTSE.dd`/`FORALLE.dd`, which
required exactly those wrappers to produce the right answer).

Also verified: `LIST_FILTER` over `[]` yields the empty collection, not NULL —
`FILTERE.sq2` = `[]` (SQLite's `json_group_array` over zero rows returns `[]` **natively**, no
coalesce), `FILTERE.pg` = `''` (empty array). **Warning:** the SQLite `group_concat` spelling
(`FILTERE.sq`) returns **NULL** for an empty result — only the `json_group_array` spelling is
carrier-correct.

**NULL-element trap in `forAll`:** `NOT (NULL > 25)` is NULL, so the row is not emitted and
`forAll([10,NULL], x>25)` would wrongly return `true` if the false element were absent. Measured:
`FORALLN.pg`/`FORALLN.sq` = `false` (correct — the `10` supplies the counterexample), but
`FORALLN2` with `[30,NULL]` needs `WHERE (:p) IS NOT TRUE` to catch the NULL. Both spellings probed;
the `IS NOT TRUE` form is the safe one and must be what the rewrite emits.

### Order preservation — stress-tested

`ARRAY(SELECT … FROM unnest(…))` is not contractually order-preserving. Probed at 200 elements:
`ORD.pg1` (bare) = `true`, `ORD.pg2` (`WITH ORDINALITY … ORDER BY o`) = `true`;
`ORD.sq1` = `1`, `ORD.sq2/sq3` = `1`. Both work today, but **the rewrite must emit the explicit
ordinal form** (`WITH ORDINALITY` on Postgres, `ORDER BY j.key` on SQLite) — the bare form is a
planner-dependent accident, and `LIST_TRANSFORM` on a permuted input (`XFORM`, `[30,10,20]`) is the
test that catches it.

---

## 4. CONSTRUCT × BUCKET TABLE

`A` = DuckDB's spelling works and returns the same value · `C` = different name/syntax ·
`B` = composite expression / different query shape · `D` = no construction found.
`:a` = the collection carrier expression.

| Construct | SQLite | construction | Postgres | construction |
|---|---|---|---|---|
| **ARRAY type** | **D** | no array type at all; carrier ⇒ JSON text | **A*** | native `int[]`; ***but D for `List<List<T>>`*** — must fall back to `jsonb` |
| array literal | **C** | `json_array(…)` / `'[…]'` | **C** | `ARRAY[…]` (DuckDB's `[…]` is a syntax error) |
| `UNNEST` (FROM) | **C** | `json_each(:a) j` | **A** | `unnest(:a)` — same name |
| `UNNEST` (select-list) | **D** | not expressible; always FROM-placed | **A** | `SELECT unnest(:a)`; parallel form NULL-pads |
| `CROSS_LATERAL` | **C** | `t, json_each(…)` / `CROSS JOIN` | **A** | `CROSS JOIN LATERAL` |
| `LEFT_LATERAL` | **C** | `LEFT JOIN json_each(…) ON 1=1` | **A** | `LEFT JOIN LATERAL … ON TRUE` |
| `LIST_FILTER` | **B** | `json_each`+`json_group_array` | **B** | `ARRAY(SELECT…unnest…WHERE)` |
| `LIST_TRANSFORM` | **B** | idem, `ORDER BY j.key` | **B** | idem, `WITH ORDINALITY` |
| `LIST_EXISTS` | **B** | `EXISTS(SELECT 1 FROM json_each…)` | **B** | `EXISTS(SELECT 1 FROM unnest…)` |
| `LIST_FOR_ALL` | **B** | `NOT EXISTS(… WHERE (:p) IS NOT TRUE)` | **B** | same |
| `LIST_REDUCE` | **B** | recursive CTE on `json_each.key` | **B** | recursive CTE on `WITH ORDINALITY` |
| `LIST_CONCAT` | **B** | `UNION ALL` of two `json_each` | **C** | `:a \|\| :b` / `array_cat` |
| `LIST_CONTAINS` | **B** | `EXISTS(… WHERE j.value = :x)` | **C** | `:x = ANY(:a)` or `:a @> ARRAY[:x]` |
| `LIST_GET` | **C** | `json_extract(:a,'$[i-1]')` — **0-based, shift required** | **A** | `(:a)[i]` — 1-based, same as DuckDB |
| `LIST_POSITION` | **B** | `(SELECT min(j.key)+1 FROM json_each…)` | **C** | `array_position` |
| `LIST_LENGTH` | **C** | `json_array_length(:a)` | **C** | `cardinality(:a)` — **never `array_length`**, see §6 |
| `LIST_FLATTEN` | **B** | `json_each(:a) j, json_each(j.value) k` | **B** | **JSON carrier only** — `jsonb_array_elements` ×2; native arrays cannot |
| `LIST_ZIP` | **B** | `json_each a JOIN json_each b ON a.key=b.key` | **C** | `unnest(:a,:b) WITH ORDINALITY t(x,y,o)` |
| `LIST_DISTINCT` | **B** | `SELECT DISTINCT` + regroup | **B** | `ARRAY(SELECT DISTINCT …)` |
| `LIST_APPEND` | **C** | `json_insert(:a,'$[#]',:x)` | **C** | `array_append` |
| `LIST_SUM` | **B** | `(SELECT sum(value) FROM json_each…)` | **B** | `(SELECT sum(x) FROM unnest…)` |
| `LIST_MIN` / `LIST_MAX` | **B** | `min`/`max` over `json_each` | **B** | `min`/`max` over `unnest` |
| `LIST_AVG` | **B** | `avg` over `json_each` | **B** | `avg` over `unnest` — returns **`BigDecimal`**, not `Double` |
| `LIST_MEDIAN` | **C** | native `median()` (present in this build) | **B** | `percentile_cont(0.5) WITHIN GROUP` |
| `LIST_MODE` | **B** | `GROUP BY value ORDER BY count(*) DESC, value LIMIT 1` | **B** | `mode() WITHIN GROUP` |
| `LIST_AGG` | **B** | `group_concat(v,sep)` over ordered subquery | **B** | `string_agg(v,sep ORDER BY o)` |
| `LIST_SORT` / `_SORT_DESC` | **B** | ordered subquery + regroup | **B** | `ARRAY(SELECT … ORDER BY x [DESC])` |
| `LIST_REVERSE` | **B** | `ORDER BY j.key DESC` | **B** | `ORDER BY o DESC` |
| `LIST_SLICE` | **B** | `WHERE key BETWEEN a-1 AND b-1` | **C** | `(:a)[a:b]` — same as DuckDB |
| `LIST_TAIL` | **B** | `WHERE key >= 1` | **C** | `(:a)[2:]` |
| `LIST_INIT` | **B** | `WHERE key < len-1` | **C** | `(:a)[1:cardinality(:a)-1]` |
| `LIST_PRODUCT` | **B** | recursive CTE (**not** `exp(sum(ln))` — §6) | **B** | recursive CTE |
| `LIST_BOOL_AND` / `_OR` | **B** | `min`/`max` over `json_each` (0/1) | **B** | `bool_and`/`bool_or` |
| `RANGE_FN` | **B** | `WITH RECURSIVE c(n)` | **C** | `generate_series(a,b)` — correlatable (`RANGECOR.pg`=15) |
| `MAP_FROM_LISTS` | **B** | `json_group_object` over key/value `json_each` join | **B** | `jsonb_object_agg` over `unnest(k,v)` |
| `MAP_FROM_ENTRIES` | **B** | `json_group_object(extract k, extract v)` | **B** | `jsonb_object_agg(e->>'k', e->'v')` |
| `MAP_EMPTY` | **C** | `json_object()` | **C** | `'{}'::jsonb` |
| `MAP_EXTRACT` | **C** | `json_extract(:m,'$.k')` | **C** | `:m ->> 'k'` (**returns text**) |
| `MAP_KEYS` | **B** | `SELECT key FROM json_each(:m)` | **C** | `jsonb_object_keys` |
| `MAP_VALUES` | **B** | `SELECT value FROM json_each(:m)` | **B** | `jsonb_each` + agg |
| `MAP_CONCAT` | **C** | `json_patch(:a,:b)` | **C** | `:a \|\| :b` |
| `TO_VARIANT` | **C** | `json_quote(:x)` | **C** | `to_jsonb(:x)` |
| `VARIANT_ELEMENTS` | **C** | `json_each(:v,'$.path')` — **correlatable** | **C** | `jsonb_array_elements(:v)` — **correlatable** |
| `VARIANT_GET` | **C** | `json_extract(:v,'$.a.b')` | **C** | `:v #>> '{a,b}'` or `jsonb_path_query_first` |
| `JSON_TYPE` | **A/C** | `json_type` — same name, **lower-case value** (§6) | **C** | `jsonb_typeof` — lower-case value |
| `JSON_MERGE_PATCH` | **C** | `json_patch(:a,:b)` — RFC 7386, exact | **B** | `jsonb_strip_nulls(:a \|\| :b)` — *shallow only*, deep merge needs a recursive CTE |
| `SqlExpr.Lambda` | **D→B** | no lambda syntax; every use site reachable by §3 | **D→B** | same |
| `SqlExpr.FoldCall` | **B** | recursive CTE | **B** | recursive CTE |

### Bucket totals (50 constructs in this slice)

| | A native | C rendering | B rewrite | D impossible |
|---|---|---|---|---|
| **SQLite** | 1 (`JSON_TYPE` name only) | 14 | **33** | **2** (ARRAY type, select-list UNNEST placement) |
| **Postgres** | 6 | 17 | **26** | **1** (`List<List<T>>` in the native-array carrier — **0** if the carrier is JSON) |
| *(H2, from H2_BACKEND.md §4 for contrast)* | — | — | — | **~all of this slice** |

Under a **JSON carrier**, Postgres reaches **D = 0** for this slice and SQLite reaches **D = 1**
(select-list UNNEST placement, which is an *assembly* choice the lowerer makes, not a capability —
FROM-placement is always available).

---

## 5. THE GRAPH-FETCH JSON ENVELOPE (question D)

H2_BACKEND.md §3 proved the nested envelope runs in one statement on H2. It runs on both targets too.
**Every property checked passed on both.**

### SQLite — byte-for-byte identical to DuckDB

```sql
-- 3 levels, ordered, empty ⇒ [], one statement
SELECT (SELECT json_group_array(json_object(
          'name', d.name,
          'emps', json((SELECT coalesce(json_group_array(json_object(
                     'name', e.name,
                     'projects', json((SELECT coalesce(json_group_array(json_object('name', p.name)),'[]')
                                       FROM proj p WHERE p.emp_id = e.id))
                   )),'[]')
                   FROM (SELECT id,name,dept_id FROM emp ORDER BY id) e
                   WHERE e.dept_id = d.id))
        ))
        FROM (SELECT id,name FROM dept ORDER BY id) d);
```
`ENV3.sq` → `[{"name":"Eng","emps":[{"name":"alice","projects":[{"name":"apollo"},{"name":"gemini"}]},{"name":"bob","projects":[]}]},{"name":"Sales",…}]`
— **identical string to `ENV3.dd`**. Likewise `ENV1`, `ENV2`, `ENVE`, `ENVN`, `ENVNO`, `CHK`, `ORDA`.

Notes: the `json()` wrapper is *not actually required* — `ENV2.sq` without it produced correct nested
JSON, because SQLite propagates a JSON subtype from `json_group_array` into `json_object`. Emit it
anyway; it is free and makes the invariant explicit rather than subtype-dependent.

### Postgres

```sql
SELECT coalesce(json_agg(json_build_object(
         'name', d.name,
         'emps', (SELECT coalesce(json_agg(json_build_object(
                    'name', e.name,
                    'projects', (SELECT coalesce(json_agg(json_build_object('name', p.name) ORDER BY p.id),'[]'::json)
                                 FROM proj p WHERE p.emp_id = e.id)
                  ) ORDER BY e.id),'[]'::json)
                  FROM emp e WHERE e.dept_id = d.id)
       ) ORDER BY d.id),'[]'::json) FROM dept d;
```
`ENV3.pg` → semantically identical; **text differs by whitespace only** (see §6).

### Property matrix

| property | DuckDB | Postgres | SQLite |
|---|---|---|---|
| 3-level nesting, one statement | ✅ | ✅ `ENV3.pg` | ✅ `ENV3.sq` |
| nested JSON stays JSON (not a quoted string) | ✅ | ✅ | ✅ (subtype-propagating, `json()` optional) |
| ordered aggregation | `ORDER BY` in subquery | **in-aggregate** `json_agg(… ORDER BY …)`; also honours a subquery `ORDER BY` (`ORDB.pg`) | subquery `ORDER BY`; **in-agg `ORDER BY` also legal** (3.44+, `IAO.sq1`) |
| `DESC NULLS LAST` in the agg | ✅ | ✅ `ORDA.pg` | ✅ `ORDA.sq` |
| empty collection ⇒ `[]` not NULL | needs `coalesce` | needs `coalesce(…,'[]'::json)` (`ZERO.pg1` = NULL bare) | **`[]` natively**, no coalesce (`ZERO.sq`) |
| **NULL elements: default** | **kept** `[1,1,2,2,null]` | **kept** `[1, 1, 2, 2, null]` | **kept** `[1,1,2,2,null]` |
| NULL values in objects: default | kept `{"a":null,"b":1}` | kept | kept |
| null-drop **controllable?** | yes (`list_filter`) | yes — `WHERE x IS NOT NULL` (`ENVND.pg`) or `jsonb_strip_nulls` (`ENVNO.pg2` → `{"b": 1}`) | yes — `WHERE … IS NOT NULL` / `json_type(value) <> 'null'` (`ENVND.sq/sq2` → `[1,3]`) |
| `CheckedEnvelope` (static array, drop NULLs) | `[{"id":"c2"}]` | `[{"id":"c2"}]` (`CHK.pg`) | `[{"id":"c2"}]` (`CHK.sq`) |
| date / decimal / boolean leaves | `{"d":"2020-01-15","n":100.5,"b":true}` | identical semantics | `"b":1` — **no boolean type** |

**Both targets default to keeping NULLs, matching DuckDB.** This is the *opposite* of H2, which
defaults to `ABSENT ON NULL` and needs an explicit `NULL ON NULL` (§3 of the H2 doc). No override is
needed on Postgres or SQLite; null-dropping is opt-in via a `WHERE` clause, which is what
`CheckedEnvelope`'s `LIST_FILTER(ArrayLit, x -> x IS NOT NULL)` rewrite should emit on all three.

### JDBC read-back for a JSON column — `SqlDialect.normalize` rows

| backend | `json`-producing expression | Java type handed back |
|---|---|---|
| DuckDB | `json_object('a',1)` | `JsonNode` |
| **Postgres** | `json_build_object('a',1)` | **`org.postgresql.util.PGobject`** |
| **Postgres** | `jsonb_build_object('a',1)` | **`PGobject`** |
| Postgres | `…::text` | `String` |
| **SQLite** | `json_object('a',1)` | **`String`** |
| **SQLite** | `jsonb_object('a',1)` | **`byte[]`** — the H2 failure mode exactly |
| *(H2, per §3)* | `JSON_OBJECT(…)` | `byte[]` |

`Executor.java:118-119`'s `String.valueOf(rs.getObject(1))` yields a usable string for Postgres
`PGobject` (its `toString()` is the JSON text) and for SQLite `json` (already `String`), but yields
`[B@…` for SQLite **`jsonb`**. **Do not emit `jsonb_*` on SQLite for the envelope** — use `json_*`.
On Postgres, either `jsonb`/`json` works via `PGobject.toString()`, but an explicit `::text` cast is
the honest fix and removes the driver dependency.

---

## 6. SILENT VALUE DIVERGENCE

Statements that parse everywhere and return a *different* answer — the worst defect class.

| # | Divergence | DuckDB | Postgres | SQLite | Fix |
|---|---|---|---|---|---|
| 1 | **`array_length(arr,1)` on an empty array** (`LENE`) | `0` | **`NULL`** | — | `LIST_LENGTH` must render **`cardinality`**, never `array_length`. A `NULL` length silently poisons every downstream comparison. |
| 2 | **`array_agg` / `json_group_array` over a LEFT-JOIN miss** (`EMPC`) | `[NULL]`, len **1** | `{NULL}`, cardinality **1** | `[null]`, len **1** | *All three* are wrong for an empty Pure collection. Must emit `agg(x) FILTER (WHERE x IS NOT NULL)` and `coalesce(…, empty)`. Verified: `EMPC.pg5`=0, `EMPC.sq3`=`[]`. **SQLite supports `FILTER`.** |
| 3 | **`json_type` value case** (`JTYPE`) | `ARRAY` / `OBJECT` | `array` / `object` | `array` / `object` | `JSON_TYPE` needs a `normalize` row, or the lowering must case-fold. Pure code comparing to a literal will silently mismatch. |
| 4 | **`json_extract` index base** (`GET`) | 1-based `[…][2]` | 1-based `(…)[2]` | **0-based** `'$[1]'` | `LIST_GET`/`LIST_POSITION`/`LIST_SLICE` need an index shift on SQLite only. `LIST_POSITION` = `min(j.key)+1`. |
| 5 | **`LIST_PRODUCT` via `exp(sum(ln))`** (`PROD`) | `24.0` | `23.999999999999993` | `24.000000000000004` | **Reject the analytic form.** It is also undefined for zero/negative elements. Use the recursive CTE: `PROD.pg`/`PRODZ.pg` = `24.0`/`-0.0`, exact. |
| 6 | **`EXISTS` / boolean read-back** (`EXISTS`,`FORALL`,`CONTAINS`,`BOOLAND`) | `Boolean` | `Boolean` | **`Integer` 0/1** | SQLite has no boolean type. `normalize` row; also affects JSON leaves (`LEAF.sq` = `"b":1` vs `"b":true`). |
| 7 | **`avg` return type** (`AVGI`) | `Double 1.5` | **`BigDecimal 1.5000000000000000`** | `Double 1.5` | `LIST_AVG` needs `::double precision` on Postgres or a `normalize` row. |
| 8 | **JSON text whitespace** (`ENV*`,`TVR`) | `{"a":1,"b":"x"}` | `json`: `{"a" : 1, "b" : "x"}` · `jsonb`: `{"a": 1}` | `{"a":1,"b":"x"}` | Postgres `json_build_object` inserts ` : ` and `, `. **JSON envelopes are not byte-comparable against Postgres.** Compare parsed, or normalize. SQLite *is* byte-identical to DuckDB. |
| 9 | **`json_each.key` type** (`KEYT`) | `String` for both | — | **`Integer`** for arrays, `String` for objects | SQLite returns a typed key; DuckDB always `String`. Affects any ordinal arithmetic read back to Java. |
| 10 | **`json_each.value` type** (`VALT`) | `JsonNode` | `PGobject` (`jsonb_array_elements`) / `String` (`…_text`) | `Integer`/`String`/raw-JSON `String` by element type | SQLite auto-types leaf values and hands nested elements back as JSON text. |
| 11 | **JSON scalar exploded** (`SCAL`) | — | `jsonb_array_elements('5')` **ERRORS** | `json_each('5')` → **1 row** | Pure's `[x]`-vs-`x` collapse must not rely on the backend. Always wrap in `json_array`/`ARRAY[]`. |
| 12 | **`LIST_ZIP` result shape** (`ZIPS`) | struct list `[(1, a), (2, b)]` | `[[1, "a"], [2, "b"]]` | `[[1,"a"],[2,"b"]]` | DuckDB zips into STRUCTs; neither target has one. The JSON pair-array is the portable shape — but `s[1]`/`s[2]` field access becomes `$[0]`/`$[1]`. |
| 13 | **`percentile_disc` vs `_cont`** (`MEDE`) | `1.5` | `_cont`=`1.5` ✅ / `_disc`=`1` ✗ | `median()`=`1.5` ✅ | `LIST_MEDIAN` must render `percentile_cont`, not `percentile_disc`. |
| 14 | **`group_concat` over zero rows** (`FILTERE`,`ZERO.sq2`) | — | — | **`NULL`**, while `json_group_array` gives `[]` | Never use `group_concat` as the collection carrier — only `json_group_array`. |
| 15 | **Postgres nested-array silent wrongs** (`NEST`) | len 2, get→sublist | **`cardinality`→4, `arr[1]`→NULL, `unnest`→full flatten** | n/a | See §2. No error is raised; three separate wrong answers. Forbid `List<List<T>>` on the native-array carrier. |

**Not a divergence, worth recording:** `ARRAY[1,2,3]` is accepted by *both* DuckDB and Postgres, while
DuckDB's `[1,2,3]` is a Postgres syntax error (`DDA.lit`, `SLU.dd`). Changing the array-literal
rendering from `[…]` to `ARRAY[…]` is a one-line `Spellings`/renderer change that makes literals
portable DuckDB↔Postgres at zero cost to DuckDB.

---

## 7. WHAT THIS MEANS FOR `SqlFn.java`'s COMMENT

`SqlFn.java:26-27` hypothesises: *"DuckDB uses list lambdas; Postgres would use unnest subqueries;
SQLite json_each."*

**Verified — all three halves are correct as *spellings*.** But the comment frames them as three
independent encodings, and the probe shows they are **one encoding with three spellings**:
build-with-an-aggregate → explode-with-a-correlated-table-function → do relational work →
re-aggregate. `RT.dd`/`RT.pg`/`RT.sq` are the same query shape and return the same value. That is
what makes a single `SqlRewriter` pass viable instead of three dialect-specific lowerings, and it is
the practical form of the carrier verdict in §2.
