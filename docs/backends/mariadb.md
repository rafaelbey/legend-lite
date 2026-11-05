# MariaDB (and MySQL): how it behaves, measured

> Written in the 2026-11 docs cleanup from `docs/MARIADB_BACKEND.md` (the 2026-08 feasibility study, probed on
> MariaDB 11.4.5), which it replaces. Sections keep their old numbers; a gap is a section that was cut. Full
> original: `git show de2745351:docs/MARIADB_BACKEND.md`. The index of this folder is [README.md](README.md).

**Built, 2026-10-11.** The dialect is `MariaDb` (`core/.../sql/dialect/MariaDb.java`), and `MySql` beside it
where the two differ. Both run the five PCT suites on a real server Bazel unpacks from the vendor's own Linux
tarball (no Docker, no MariaDB4j): MariaDB 11.4.13 and MySQL 8.4.11, measured in [cloud.md](cloud.md) §13
(MariaDB), §14 (the system database), §16 (channel B) and §17 (MySQL). This file is the earlier probe study:
what MariaDB does, measured statement by statement against DuckDB. Its detailed SQL is in
[probes/frag-mariadb-scalar.md](probes/frag-mariadb-scalar.md) and
[probes/frag-mariadb-shapes.md](probes/frag-mariadb-shapes.md). Where the lane measured something newer, a note
says so.

**There is no golden-text half:** the engine's corpus has no MariaDB goldens, so **row equality is the only
contract**.

**Evidence standard.** The study executed **~1,150 probe statements against a real embedded MariaDB 11.4.5**
(MariaDB4j), every probe also run against DuckDB 1.5.0.0 as the reference value, through one shared harness.
**Value equality was checked, not just parse success.** Where documentation and execution disagreed, execution
won — **and it did, on the headline question**. Session as probed:

```
sql_mode = IGNORE_SPACE,STRICT_TRANS_TABLES,ERROR_FOR_DIVISION_BY_ZERO,
           NO_AUTO_CREATE_USER,NO_ENGINE_SUBSTITUTION
character_set_server = utf8mb4 · collation_server = utf8mb4_general_ci
time_zone = SYSTEM · max_recursive_iterations = 1000
```

**Note what is *not* in that `sql_mode`: `ANSI_QUOTES`, `PIPES_AS_CONCAT`, `NO_BACKSLASH_ESCAPES`,
`ONLY_FULL_GROUP_BY`.** The first three are correctness-critical for legend-lite (§4). The fourth is
widely documented as a MariaDB default and **is not one** — that claim is refuted by execution.

---

## 1. Verdict

**Achievable and strong — 97% reachable, zero impossible constructs in the entire shapes slice. But
it is gated behind a single session flag, and without that flag every query silently returns column
names as data.**

- **201 of 207 constructs reachable — 97%.** Only **6 D's**, all in the scalar slice.
- **H2's fatal wall does not transfer.** `LATERAL` really is absent — but **`JSON_TABLE` exists on
  MariaDB 11.4.5 and correlates**, which is the capability that matters. §3.
- **`ANSI_QUOTES` is the gate.** legend-lite quotes every identifier with `"`. Without the flag,
  `SELECT "name"` returns the *string* `'name'`. §4.
- **MariaDB reaches almost everything, but usually by rewrite rather than natively** (89 of the 207 constructs
  by a rewrite pass). It is the most *rewrite-hungry* backend measured, and correspondingly the one where
  a thin spelling table would be most misleading.
- **`ROUND` and `ROUND_HALF_UP` are both wrong, in opposite directions**, unless the argument type is
  forced. §5.

> Cut in the 2026-11 docs cleanup: the 2026-08 "NOT BUILT" banner and §2, the 207-construct capability-count
> table (the current per-dialect decisions are generated in `docs/dialect-capability/`). Full text:
> `git show de2745351:docs/MARIADB_BACKEND.md`.

---

## 3. `JSON_TABLE` exists, and it correlates

`LATERAL` is genuinely absent — every spelling is a parse error. But that is only half of H2's D1 wall
([h2.md](h2.md) §4), and it is the half that does not matter:

```sql
-- correlated explosion against a real table column: OK, SUM = 100
SELECT SUM(jt.v) FROM arrtab t, JSON_TABLE(t.arr, '$[*]' COLUMNS (v INT PATH '$')) jt;
```

Correlation works on a **column** and on an **arbitrary expression over the outer row**. This
contradicts the widespread claim that `JSON_TABLE` is MySQL-only: it landed in **MariaDB 10.6**
(MDEV-17399).

> **Record the version threshold.** MariaDB **10.5 and earlier really are H2-shaped** — no
> `JSON_TABLE`, no `LATERAL`, no correlated explosion. The minimum supported MariaDB is therefore
> **10.6**, and that is a hard floor, not a preference.

**So MariaDB's collection story is Postgres-shaped, not H2-shaped** — a different mechanism with the
same capability. The `LIST_*` family is reachable by rewrite.

`JSON_TABLE` is not a thin capability: it chains, nests, and takes `FOR ORDINALITY`, `NESTED PATH`,
`EXISTS PATH` and `ON EMPTY`. `LEFT JOIN JSON_TABLE(…) ON TRUE` preserves the empty-array row —
`LEFT_LATERAL` exactly.

**What `LATERAL`'s absence still costs, measured:** only **per-row top-N** and **per-row
multi-aggregate**, both of which rewrite to window or grouped-join forms with values equal to
DuckDB. Two independent fallbacks also execute: the `seq_M_to_N` SEQUENCE engine with a correlated
predicate, and a recursive-CTE `JSON_EXTRACT(arr, CONCAT('$[',n,']'))` index walk.

That second fallback is load-bearing beyond its size: **the index walk is what delivers `FoldCall`
and `LIST_REDUCE`, which `JSON_TABLE` alone cannot express.** Note the tension with §7 — it is also
the construction bounded by `max_recursive_iterations`.

Unlike SQLite, **MariaDB's recursive CTEs cannot be row-correlated** — which is exactly why
`LEVENSHTEIN` and `JARO_WINKLER` were D here but B on SQLite (§6).

> **Newer (the lane, 2026-10-11, MariaDB 11.4.13; cloud.md §13):** an outer reference inside a derived table or
> a recursive CTE fails ("Unknown column"), so a LATERAL subquery's rows are inlined as `JSON_TABLE` over those
> rows aggregated as JSON objects, and a fold, a `list_reduce` and an edit distance are a recursive CTE only over
> values that read no row, with a typed anchor (a CTE's column takes its first row's type: `''` is CHAR(0), `0`
> an INT). A table function's argument does correlate.

---

## 4. The gate — `ANSI_QUOTES`, and three more session flags

**This is the most severe finding of the four-backend probe study.**

| Setting | Default | What happens silently if left unset |
|---|---|---|
| **`ANSI_QUOTES`** | **off** | `SELECT "name" FROM emp` returns the **string `'name'`**, not `alice`. legend-lite quotes **every** identifier with `"`, so **every column reference becomes a constant.** A query returns a rectangle of column names. No error, anywhere. |
| **`PIPES_AS_CONCAT`** | off | `'a' \|\| 'b'` → **`0`**. `\|\|` is OR. Parses, returns a number, never raises. |
| **`NO_BACKSLASH_ESCAPES`** | off | `LENGTH('\\')` is 1, not 2 — **every regex pattern is corrupted at parse time.** |
| **collation** | `utf8mb4_general_ci` | case-**insensitive** *and* PAD SPACE: `'abc'='ABC'` **true**, `'abc'='abc '` **true**. `utf8mb4_bin` fixes case but **not** padding; only **`utf8mb4_nopad_bin`** matches DuckDB. |
| **`time_zone`** | `SYSTEM` | `UNIX_TIMESTAMP` is off by the host offset. |

Verified by mutating the session and re-probing, not by reading documentation.

> **The collation row is different in kind from the other four, and the difference is a trap.**
> `sql_mode` flags are genuinely session-scoped, but **session `collation_connection` does not
> override a column's declared collation.** Collation must be pinned at **server and DDL scope** —
> which means it is a property of how legend-lite *creates tables*, not of how it connects. That is
> the same defect class as H2's forked `charPadding` ([h2.md](h2.md) §7), reappearing on a second
> backend: string comparison semantics are a *storage* decision that the query layer cannot repair.

`CAST` is also non-validating here, as on SQLite: `CAST('abc' AS SIGNED)` → **`0`**, silently.

> These are not spelling and not capability — they are **configuration that must be owned and
> asserted**. MariaDB is the sharpest instance: one flag separates "works" from "uniformly wrong, silently."

> **Newer (the lane, 2026-10-11; cloud.md §13), what the dialect's session setup sets:**
> `ANSI_QUOTES,PIPES_AS_CONCAT,NO_BACKSLASH_ESCAPES` and strict mode, the zone UTC, utf8mb4 with its *binary*
> collation (case- and accent-sensitive comparison and LIKE), `group_concat_max_len` at 1 GB,
> `max_recursive_iterations` raised, 30 decimal digits for a division, and `condition_pushdown_for_derived` off
> (on, a JSON filter pushed into a UNION derived table returned no rows). The NO PAD collation was not used: the
> JSON functions answer in the binary collation, so a NO PAD one mixes with theirs ("Illegal mix of
> collations"). The binary collation still pads (`' ' = ''` is true), so an empty text is tested by its length
> (`CHAR_LENGTH`).

---

## 5. Silent value divergences

Every row executes cleanly and returns a wrong value (MariaDB 11.4.5 against DuckDB 1.5.0).

| | DuckDB | MariaDB |
|---|---|---|
| `ROUND(x)` on a **DOUBLE** | half away from zero | **banker's** |
| `ROUND(x)` on a **DECIMAL** | half away from zero | half-up |
| `REGEXP_REPLACE` | **first** match only | **all** matches — *same function name* |
| `TIMESTAMPDIFF('month', …)` | boundary crossings | **complete units** — 2→1, year 1→0, hour 2→1 |
| `DATEDIFF` | — | **argument order inverted** on top of the counting rule |
| `-8 >> 1` | `-4` | **`9223372036854775804`** — all bitwise ops evaluate **unsigned** |
| `LENGTH('héllo')` | characters | **bytes** |
| `GREATEST(a,NULL)`, `CONCAT(a,NULL)` | skips NULL | **propagates NULL** |
| `PI()` | full precision | **truncates to 7 digits** |
| `MAKEDATE(y, n)` | `make_date(y,m,d)` | **different function** — day-of-year, arity collision |
| `WEEK()` | ISO | needs **mode 3** for ISO |
| `DAYOFWEEK` / `WEEKDAY` | — | **two different bases, neither DuckDB's** |
| default NULL ordering | LAST/LAST | **FIRST on ASC, LAST on DESC — and `NULLS FIRST/LAST` is a parse error** |

**Both rounding functions are wrong and they fail in opposite directions.** The 2026-08 renderer wrote `ROUND`
as a half-even composite and `ROUND_HALF_UP` as bare SQL `ROUND` — on MariaDB the bare form is banker's for DOUBLE
and half-up for DECIMAL, so the argument type silently picks the mode. Both need an explicit cast to pin it. (The
lane, 2026-10-11: a double's ROUND is half to even, and so is a CAST of a double to an integer; DuckDB's half
away from zero is written out.)

---

## 5.1 Graph fetch — survives, with one mandatory cast

The nested object-graph envelope runs in **one statement**, three levels deep, using
`JSON_ARRAYAGG`/`JSON_OBJECT`. Verified: ordered aggregation; `COALESCE(…, JSON_ARRAY())` for empty
collections; NULL elements kept. **The expected double-encoding weak spot is absent** — nested JSON
stays JSON, and a `JSON`-typed column nests identically to DuckDB.

One real defect, and it is subtle:

> A **numeric correlated scalar subquery** inside a `JSON_OBJECT` **alongside a string sibling**
> serialises as a JSON **string**: `{"n":"2"}` where DuckDB gives `{"n":2}`. Cause is charset
> aggregation over the subquery's `binary` charset. **Fix: an explicit `CAST` on the numeric member.**

The same root cause makes the driver hand back **`byte[]`** for the envelope — the H2 precedent
([h2.md](h2.md) §3) — fixed by wrapping the envelope in `CAST(… AS CHAR)`.

Note this is a *type-dependent, sibling-dependent* serialisation bug: the numeric member is only
mis-typed when a string sibling is present. A graph-fetch test with a single scalar member would pass
and the defect would surface later on a wider object.

---

## 6. The six D's of the study — each with what was tried

| | Why |
|---|---|
| `SPLIT`, `REGEXP_EXTRACT_ALL` | **no ARRAY type.** This is the collection-carrier wall, not a spelling gap — under a JSON carrier both become B. |
| `LEVENSHTEIN`, `JARO_WINKLER` | no built-in. Recursive CTEs work in scalar subqueries but **cannot be row-correlated**, which is the primitive SQLite used to reclaim exactly these two as B. |
| `TYPEOF` | **downgradeable to C** — column types are statically known from `information_schema`. |
| `TIMEZONE` | a **deployment** gap, not an engine gap: `mysql.time_zone_name` is empty in the embedded image, so named zones return NULL. Numeric offsets work. Loading the tz tables fixes it. |

Only two are true capability walls; the rest are carrier, deployment, or downgradeable. (Today's generated
matrix, `docs/dialect-capability/`, shows `SPLIT`, `REGEXP_EXTRACT_ALL`, `LEVENSHTEIN` and `JARO_WINKLER` emulated
on `MariaDb` and `MySql`, under the JSON carrier; a named zone still needs the server's zone tables, one of the
lane's own pins in 2026-10.)

---

## 7. Limits, measured (MariaDB 11.4.5)

| Budget | H2 | **MariaDB 11.4.5** |
|---|---|---|
| `aliasLimit` | 256 | **64** for any DDL identifier (65 → *"Identifier name … is too long"*). SELECT aliases accept ≥257, so 64 binds. |
| `collectionThresholdLimit` | not set | **not set** — the IN→temp-table rewrite is unnecessary; `JSON_TABLE` over one JSON parameter is strictly better. Text bounded by `max_allowed_packet`. |
| `supportsFullOuterJoin` | false | **false** — UNION-of-LEFT-and-RIGHT rewrite, verified (B). |
| `supportsBooleanLiteral` | true | **true** — `TRUE`/`FALSE` parse, but read back as `Integer`. |
| `limitStyle` | `top N` / `offset…fetch` | **`LIMIT n OFFSET m`** — DuckDB-identical. No `TOP`. Offset-only needs `LIMIT 18446744073709551615 OFFSET n`. |
| `nullOrdering` | explicit | **implicit and inverted** — needs the `expr IS NULL` rewrite, not just a flag. |
| `max_recursive_iterations` | — | **1000, and it truncates silently.** A recursive-CTE rewrite that exceeds it returns *fewer rows*, not an error. **Budget row.** |

That last row deserves emphasis: with so many constructs reaching MariaDB by **rewrite**, and several of
those rewrites being recursive CTEs, a silent 1000-row ceiling is a correctness cliff, not a
performance note.

---

## 8. What the JDBC driver returns — the `normalize` rows

The largest normalize surface of any backend measured (nineteen rows; H2 four, Postgres four, SQLite eight).
Led by:

| | JDBC returns | canonical |
|---|---|---|
| `BOOLEAN` | **`Integer`** (`TINYINT(1)` alias) | `Boolean` |
| bitwise results | **`BigInteger`** (unsigned) | signed 64-bit |
| `SUM` | **`BigDecimal`** (DuckDB: `BigInteger`) | integer |
| `JSON` | `String` — `JSON` is a `LONGTEXT` alias, not a real type | JSON |

The full nineteen are in [probes/frag-mariadb-scalar.md](probes/frag-mariadb-scalar.md) §7 and
[probes/frag-mariadb-shapes.md](probes/frag-mariadb-shapes.md) §7.

---

## 9. MariaDB vs MySQL

The project targets both; they diverge on exactly the things this doc turns on.

| | MariaDB 11.4 | MySQL 8+ |
|---|---|---|
| `JSON_TABLE` | **yes** (10.6+) | yes |
| `LATERAL` | **no** | **yes** (8.0.14+) |
| `ONLY_FULL_GROUP_BY` default | **no** | **yes** |
| `JSON` type | `LONGTEXT` alias | real type with binary storage |
| `INTERSECT` / `EXCEPT` | yes (10.3+) | 8.0.31+ |

**MySQL is the strictly stronger target** (it has `LATERAL` and a real JSON type) but the stricter
default `sql_mode`. A dialect written against MariaDB should run on MySQL; the reverse is not true. (As built,
`MySql` is `MariaDb` where the two agree and its own hooks where they differ: JSON functions, regular expressions
(ICU on MySQL), `YEAR()`'s type, unsigned BIGINT literals, JSON text with a space after each `,` and `:`;
[cloud.md](cloud.md) §17.)

> Cut in the 2026-11 docs cleanup: §10 (standing MariaDB up with MariaDB4j 3.3.1 and its macOS ARM64 pcre2
> workaround; the lane now unpacks MariaDB's own Linux tarball, cloud.md §13) and §11 (the build order,
> 2026-08). Full text: `git show de2745351:docs/MARIADB_BACKEND.md`.

---

## 12. What NOT to do

- **Don't connect without `ANSI_QUOTES`** (§4). Every quoted identifier silently becomes a string
  literal. This is the single highest-severity configuration risk found across all four probed backends.
- **Don't support MariaDB < 10.6** (§3) — no `JSON_TABLE` means H2's wall, for real.
- **Don't trust `sql_mode` documentation** — `ONLY_FULL_GROUP_BY` is *not* a MariaDB default, and
  `JSON_TABLE` is *not* MySQL-only. Both claims were refuted by execution.
- **Don't reuse DuckDB's `REGEXP_REPLACE` semantics** — same name, first-match vs global (§5).
- **Don't build rewrites on recursive CTEs without bounding them** — 1000 iterations, truncated
  silently (§7).
- **Don't assume `LENGTH` or bitwise ops are portable** — bytes not characters, unsigned not signed.

Sources: `docs/MARIADB_BACKEND.md` (git history: docs/HISTORY.md); the lane's measurements in
`docs/CLOUD_BACKENDS_PCT_2026_10_11.md` §13 and §17 (now [cloud.md](cloud.md)); the generated matrix in
`docs/dialect-capability/`.
