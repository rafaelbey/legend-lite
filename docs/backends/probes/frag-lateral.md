# The linchpin: correlated row explosion on all four backends

**Probed directly (not delegated), `probes-lateral.tsv` → `out-<backend>-lateral.tsv`.**
Every row below is an executed statement against a real engine: DuckDB 1.5.0.0, SQLite 3.47.1.0,
PostgreSQL 18.4 (zonky embedded), MariaDB 11.4.5 (MariaDB4j embedded).

## Why this is the linchpin

`H2_BACKEND.md` §4 identifies **D1 — no `LATERAL`, no correlated table function** as H2's fatal
wall, the thing that kills `SqlFn.UNNEST` across 11 emission sites, `CROSS_LATERAL`, `LEFT_LATERAL`,
the 16-member `LIST_*` value family, `LIST_FLATTEN`, `RANGE_FN` and `VARIANT_ELEMENTS`. §4.1 then
concludes that reaching H2 requires re-lowering collections from array values to **relations** — a
Phase-H/I redesign of `Scalars.java` + `Fold.java` + `ListShapes.java`.

That conclusion is correct **for H2**. The question this probe answers is whether it generalises.

## Result — it does not. H2 is the outlier.

| Form | DuckDB | SQLite | Postgres | MariaDB |
|---|---|---|---|---|
| `t, unnest(t.arr)` (comma-correlated) | OK | — | **OK** | — |
| `LATERAL unnest(t.arr)` | OK | — | **OK** | — |
| `CROSS JOIN LATERAL` | OK | — | **OK** | — |
| `LEFT JOIN LATERAL … ON TRUE` | OK | — | **OK** | — |
| select-list `unnest(...)` | OK | — | **OK** | — |
| correlated table fn in scalar subquery | OK | **OK** | **OK** | — |
| `t, json_each(t.arr)` (correlated arg) | OK | **OK** | — | — |
| `JSON_TABLE(expr, '$[*]' COLUMNS …)` | — | — | **OK** | **OK** |
| **row-correlated explosion vs a REAL table** | **OK** | **OK** | **OK** | **OK** |
| recursive CTE | OK | OK | OK | OK |

**All four backends have correlated row explosion.** Each by a different mechanism:

- **Postgres** — full `LATERAL` in every position, plus `unnest`. Also has `JSON_TABLE` (17+).
- **SQLite** — no `LATERAL` and no array type, but `json_each` **accepts a row-correlated argument**.
  Verified end-to-end, not just parsed: `SELECT (SELECT sum(value) FROM json_each(e.arr)) FROM
  (SELECT '[10,20,30]' AS arr) e` → **60**. That is precisely the shape H2 cannot express.
  Against a real table: `SELECT d.id, j.value FROM dept d, json_each('[' || d.id || ',' || (d.id*10)
  || ']') j` → correct rows.
- **MariaDB 11.4.5** — **has `JSON_TABLE`**, contrary to the widespread claim that it is MySQL-only
  (it landed in MariaDB 10.6). Row-correlated: `SELECT d.id, jt.v FROM dept d,
  JSON_TABLE(CONCAT('[', d.id, ',', d.id*10, ']'), '$[*]' COLUMNS (v INT PATH '$')) jt` → correct.
  `LATERAL` itself is a separate question and MariaDB may genuinely lack it.
- **DuckDB** — the incumbent; list lambdas, no explosion needed.

## What this changes

1. **The §4.1 collection-carrier redesign is not a precondition for SQLite, Postgres or MariaDB.**
   It is a precondition for **H2 specifically**. Sequencing that treats "the carrier redesign" as the
   gate for all N backends would be scheduling H2's problem in front of three backends that do not
   have it.
2. **§4.1's stated rationale — "it is the right redesign for N backends generally, since Postgres,
   Snowflake and BigQuery each need a *different* one" — is half-confirmed.** The mechanisms genuinely
   are different per backend (LATERAL / json_each / JSON_TABLE). But "different mechanism" is a
   *dialect-assembly* difference, which is what `SqlFn.UNNEST`'s own comment already says placement is
   ("PLACEMENT (select-list vs LATERAL FROM) is dialect assembly"). It is not obviously a *carrier*
   difference. The carrier only has to change where the backend has no array type at all.
3. **The carrier question splits in two, and only one half is universal:**
   - *Does the backend have an array-shaped value?* Postgres yes (real `ARRAY`); DuckDB yes; SQLite
     no (JSON text only); MariaDB no (JSON text only); H2 has `ARRAY` but cannot explode it.
   - *Can the backend explode a correlated collection into rows?* **All four: yes.**
   So the redesign that actually generalises is **"collections must be able to lower to relations"**,
   not "collections must stop being values". Postgres can keep the value carrier and still explode.

## Incidental divergences already visible in this one probe file

| Probe | DuckDB | Postgres | MariaDB | Note |
|---|---|---|---|---|
| `sum` over a scalar-subquery explosion | `BigInteger:60` | `Long:60` | — | `normalize` row |
| `sum` over a recursive CTE | `BigInteger:3` | `Long:3` | **`BigDecimal:3`** | three drivers, three Java types for one SUM |
| `json_each` literal | `JsonNode:10` | — | — | DuckDB returns a JSON node; H2 returns `byte[]` |

The SUM row is the `SqlDialect.normalize` problem in miniature: **one semantic function, three Java
types, none of them wrong.** `H2_BACKEND.md` §8 predicted this would be where the deferred
`ValueCodec` step materialises. It is.

## Caveat on the SQLite result

`json_each` accepting a correlated argument is a **table-valued-function** capability, not `LATERAL`.
It worked in every position probed here, including inside a scalar subquery and against a real table
column. Before relying on it, the collections agent should confirm it holds for the specific shapes
`Scalars.java` emits — in particular multi-array zip (`Scalars.java:853-856`'s parallel unnest),
which has no obvious `json_each` equivalent since `json_each` takes exactly one document.
