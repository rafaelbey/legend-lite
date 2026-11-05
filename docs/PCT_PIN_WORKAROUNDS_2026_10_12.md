# PCT pins: what each database cannot do, and the workarounds (2026-10-12)

**The question (the user, 2026-10-12):** *drill into ClickHouse PCT to understand what pieces it could not do, and,
as for BigQuery and the others, whether there are good workarounds for them.*

**How to read a pin.** A database's PCT ledger pins a test that fails there, together with its message. Some pins are
the **platform's own**: the DuckDB reference lane pins the same test, so the failure is legend-lite's on every
database (instance identity, a model's class instances, big date arithmetic, error-message text, ...). Those are not
the database's gaps, and no dialect work removes them. A database's **own** pins are the rest.

## Own pins per database (measured at c03ac6b)

| database | pins | of which the platform's (also on DuckDB) | **own** |
|---|---:|---:|---:|
| Postgres | 109 → **43** | 34 | 75 → **9** (burned down, below) |
| H2 (Relation suite only) | 26 | 1 | **25** |
| MySQL | 54 | 34 | **20** |
| MariaDB | 51 | 34 | **17** |
| SQLite | 49 | 34 | **15** |
| ClickHouse | 65 → **38** | 34 | 31 → **4** (burned down, below) |
| BigQuery | 42 | 34 | **8** |
| Trino | 40 | 34 | **6** |
| Databricks | 35 | 34 | **1** |
| Snowflake | 35 | 34 | **1** |

DuckDB itself pins 34. The ClickHouse and Postgres rows were re-measured after their burn-downs, later on 2026-10-12. The counts come from the `one("<test>", "<message>")` rows in
`pct/src/test/java/org/finos/legend/lite/pct/Test_LegendLite_<Db>_<Suite>Functions_PCT.java`, where DuckDB's ledger is
`Test_LegendLite_<Suite>Functions_PCT.java`.

## ClickHouse: its 31 own pins, and a workaround for each

**Outcome (later on 2026-10-12): 27 of the 31 fixed, and 4 kept pinned.** Each fix is in `ClickHouse.java` except two
connection settings, and is recorded in `docs/backends/cloud.md` §19.1. The table below is the plan as
probed. Where the fix took another shape than the probe, its row says so.

What ClickHouse cannot do is in `docs/backends/cloud.md` §19, measured during the burn-down. Each
workaround below was **probed on the pinned server binary** (ClickHouse 25.8.2.29, through `clickhouse local`). A
probe proves the SQL shape works; it does not yet prove the test passes. That is the dialect's work, measured by the
lane.

| pins | what ClickHouse cannot do | the workaround, probed | effort | outcome |
|---:|---|---|---|---|
| 6 | **A Tuple field of unknown type**, from an unset property: a Tuple's fields are typed, and `Nullable(Tuple)` is refused (probed: "cannot be inside Nullable type"). Tests: `testEqVarIdentity`, `testEqualVarIdentity`, `testContainsNonPrimitive`, `testMapInstance`, `testCoalesceInPreFilter`, `testAssignNewInstance` | Type the unknown field as **`Dynamic`**. Probed: `tuple(1, CAST(NULL AS Dynamic))` is `Tuple(UInt8, Dynamic)`, its NULL field reads as NULL, and `Dynamic = Dynamic` compares. **To check:** that the JDBC driver (0.9.6) decodes a `Dynamic` field. | medium | fixed, all 6: the field's DECLARED type (`StructLit.Field.declared`), an unset list as the empty list, two instances compared as one-element arrays (a Tuple's `=` answers NULL on a NULL field), and a field that is itself a struct carried as its `Variant(Tuple(...))`, NULL where the instance is absent and read through `variantElement` (the user's ask, measured). `testEqualNonPrimitive` now fails as on DuckDB |
| 4 | **An empty list of no type** (`Nullable(Nothing)`), which nothing can zip or project into. Tests: `testZip*ListEmpty` ×3, `testSimpleProjectWithEmpty` | An empty list is a **typed empty array**, never a NULL. Probed: `arrayZip([], [])` and `arrayZip(CAST([] AS Array(Nullable(Int64))), ...)` both answer `[]`. The NULL comes from the dialect's emulation, and the failing SQL needs a `dump-sql` look. | small | fixed: the slot's type for the empty list, at the projection's unnest and the ARRAY JOIN |
| 2 | **An as-of join with no equality**: ClickHouse needs one besides the inequality. Tests: `testSimpleAsOfJoin`, `..._MultipleExpressions` | A **constant key on both sides**: `0 AS k` in each, `ON l.k = r.k AND l.t >= r.t`. Probed: the right answer. A second alternative is the window pick Databricks uses (`AsOfAsPick`). | small | fixed, by another key: a constant is folded away and refused again, so each side's operand hashed times zero |
| 1 | **An outer join whose ON reads both sides outside an equality**: refused under `join_use_nulls`, even with `allow_experimental_join_condition` (probed). Test: `testNestedJoinArithmeticComparisonExpression` | Join **without** `join_use_nulls` (probed: the condition is accepted), plus a marker column `1 AS __m` on the optional side. A missing row reads `__m = 0`, and each of its columns becomes `if(__m = 1, col, NULL)`. Scope: per statement. | medium | kept pinned |
| 1 | **A join condition on one side alone** ("cannot determine join keys"). Test: `TestJoin_CurrentUserId` | For an inner join, a **CROSS JOIN with the condition in WHERE** (probed). For an outer join, the condition moves into that side's subquery. | small | fixed: for an outer join, the condition as a key (`if(cond, 0, 1)` against the other side's hash times zero) |
| 1 | **A correlated subquery as IN's argument**: refused even with `allow_experimental_correlated_subqueries` (probed). Test: `testIn_CorrelatedSubQuery` | Rewrite `x IN (correlated subquery)` as **`EXISTS (... AND col = x)`**. Probed: a correlated `EXISTS` runs under `allow_experimental_correlated_subqueries = 1`. Caution: experimental on 25.8. | small–medium | fixed as three-valued IN (`CASE` over three EXISTS), only where the filter is null-rejecting (the experimental feature answers false for an outer NULL, measured) |
| 4 | **No LATERAL join.** Tests: `testLateralJoin*` ×4 | BigQuery's shape: a **correlated scalar subquery collecting `groupArray(tuple(...))`, then `ARRAY JOIN`**, or `LEFT ARRAY JOIN` for an outer lateral. Probed: right rows under `allow_experimental_correlated_subqueries = 1`. Caution: experimental. | medium | fixed by decorrelation instead: a lateral subquery that only filters becomes an INNER or LEFT join with its correlated conjuncts in the ON; any other lateral is still refused |
| 4 | **A RANGE frame's offset must be a non-negative integer**; `2.1` is refused. Tests: `testRange_WithNumbers_*` ×2 in over and ×2 in reduce | **Scale the key and the offsets to integers**: order by `toInt64(x * 10^s)`, with the offset `2.1 → 21` at scale 1. Probed on a Decimal key: right sums. This is exact for a Decimal key (its scale is in its type); a TDS Float literal is already an exact Decimal on ClickHouse (§19). For a Float key it is approximate, so refuse it there. | medium | fixed without scaling: the frame computed over the partition's `groupArray(tuple(arg, key))`, filtered by the offsets and reduced by the aggregate |
| 1 | **A quantile's level must be a constant.** Test: `testPercentile` | **`arraySort(groupArray(x))` and linear interpolation** at `(n - 1) * level`. Probed: 3.25, the same as `quantileExactInclusive(0.25)`. | small | fixed as probed |
| 1 | **A statement over `max_query_size`** (256 KiB), refused before its own SETTINGS clause is read. Test: `testFormatFloat` | Set **`max_query_size` on the connection** (a JDBC/HTTP parameter), not in the statement. | small | fixed as probed, with the AST limits too |
| 1 | **No session over HTTP**, so no session-private table and nowhere for the system database. Test: getAll `testBasic` | ClickHouse's HTTP interface keeps a session by **`session_id`**, and temporary tables live in it. A session-private scratch database is the alternative. **To check:** how the 0.9.6 driver passes `session_id`. | medium | fixed: the system connection's `clickhouse_setting_session_id` (the driver passes it as an HTTP parameter), a TEMPORARY table in that session |
| 2 | **A list is never NULL** (`Nullable(Array)` is refused), so a JSON `null` read as a list becomes `[]`. Tests: `testVariantColumn_distinct_removeDuplicates`, `testVariantColumn_slice` | Carry a nullable list as **`Variant(Array(T))`**. Probed: it holds a NULL (`isNull` is 1). Every list function then needs `variantElement`, so this is a wide change for two tests. | hard | fixed another way: a list read from a JSON value is NULL exactly where the value is JSON's null, so where it is written back (`toVariant`) through NULL-strict list functions, it is NULL there; a raising branch is kept. The `Variant(Array(T))` carrier was not needed: ClickHouse's list functions refuse a Variant (measured) |
| 3 | **Dates only from 1900 to 2299.** `Date32` and `DateTime64` clamp silently, so the dialect refuses rather than answer for another date. Tests: `testBetween_{Date,DateTime,StrictDate}` with the year 1032 | **None cheap.** A date would have to be carried as a number (`toDaysSinceYearZero` exists) or as text on every path. Keep them pinned: the refusal is honest. | — | kept pinned |

**Summary for ClickHouse:**

- **The plan:** 9 small and 17 medium, probed shapes that should move 26 of the 31; 5 to stay pinned.
- **The outcome:** 27 fixed: the 26 planned less `testNestedJoinArithmeticComparisonExpression` (no per-join
  `join_use_nulls`), plus the two nullable-list pins. An absent instance is a struct field's `Variant(Tuple(...))`,
  which holds NULL (the user's ask, after a first pass refused it by name).
- **One experimental setting:** `allow_experimental_correlated_subqueries = 1`, for the IN rewrite's correlated
  EXISTS, written only into the statements that hold it. It answers false for an outer row whose correlated column
  is NULL (measured), so the rewrite is limited to null-rejecting filters. `Dynamic` was not needed.
- **4 stay pinned:** the dates (3) and the outer join's residual (1).
- **None of the 34 platform pins is ClickHouse's work.**

## Postgres: its 75 own pins (2026-10-12)

**Outcome: 66 fixed, 9 kept pinned** (48 in the first pass, then the 12 jsonb ones, then 6 more, each at the user's
ask). Each Postgres fix is in `Postgres.java` and was probed on the embedded Postgres 16.15 first; the typing gap was
the shared lowering's, fixed there for the ordered aggregate and re-measured on every lane. Both channels are green; channel B's floors rose to
329, 132, 465, 204 and 91.

| pins | what Postgres could not do | the answer | outcome |
|---:|---|---|---|
| 13 | **printf**: `format()` has `%s`, `%I` and `%L` only | A literal format unrolled into `\|\|` pieces, Snowflake's shape: `%s` the dialect's own text of the value, `%d` padded after its sign, `%.Nf` rounded half-even on the value's shortest decimal text, as Pure rounds (it was `round(numeric, N)`, half away from zero, until PR #34's review), a negative that rounds to zero keeping its `-`. Padding is `lpad` to `greatest(width, length)`, because Postgres' `lpad` cuts a longer text (measured). DuckDB's `%g` date part is `to_char`'s `MS` | fixed |
| 21 | **The empty collection as a list** (a NULL of no type): Postgres types every array, so `unnest(NULL)` or a reduction over it is refused ("could not determine polymorphic type") | Each call over it is spelled as DuckDB answers over a NULL list: a reduction, an element or a rearrangement is NULL, its explosion no row, a concatenation its other lists, `exists`/`forAll` the empty answer. A projection whose expression is the empty collection is cast to its column's declared type | fixed |
| 2 | **No `max(boolean)`** | A Boolean list's max is `bool_or`, its min `bool_and` | fixed |
| 7 | **A struct came back as the driver's jsonb object** (zip's pairs, a new instance) | `normalize` decodes its JSON text into the platform's map, fields in declared order | fixed. Seven more tests then fail as on DuckDB (instance identity); they were already counted among the platform's pins, and only their pinned message changed |
| 4 | **splitPart** | `(array_remove(string_to_array(s, d), ''))[i]`: the non-empty tokens, 1-based, NULL past either end | fixed |
| 1 | **hashCode** | The first 64 bits of the MD5 of the values' JSON text, read signed through `bit(64)` (MariaDB's construction) | fixed |
| 12 | **Maps, a fold into a list, a list reduction, a list aggregate** | A map is its entries in order, a `jsonb[]` of `{"key", "value"}` (BigQuery's shape, which the executor already decodes to a map), with DuckDB's semantics: entries keep their order, a NULL or repeated key raises, a concatenation keeps each key's first place with the last value, an extraction is a list. A fold or a reduction into a list of scalars is the recursive walk over a native array; the LIST aggregate is `array_agg`. Found on the way: the dialect's raise (a STABLE cast) fired at PLAN time under `unnest(...)`, whose row estimate pre-evaluates its argument; it now carries an empty volatile term | fixed |
| 4 | **Levenshtein, Jaro-Winkler**: no core function. `fuzzystrmatch` has a Levenshtein of at most 255 characters; no extension has a Jaro-Winkler | Recursive walks in plain SQL, no extension: Levenshtein is SQLite's edit-distance table a row at a time, with arrays; Jaro-Winkler is Snowflake's algorithm as a walk along the second text. Both equal DuckDB's on the 1,585 ASCII pairs Snowflake's was measured on (Jaro-Winkler bit for bit). On other text they count characters, as the reference and SQLite, MariaDB and Snowflake do, where DuckDB counts bytes (2 for `ä€x` and `a€x`, measured) | fixed |
| 1 | **SHA-1**: no core `sha1` in any release (checked: 18.6, the current one, and the devel docs of 2026-10-10 list md5, sha224 to sha512, crc32 and crc32c) | `pgcrypto`'s `digest(.., 'sha1')`, which the bundled Postgres ships | kept: the dialect requires no extension |
| 1 | **An IR typing gap**: `testMixedSortNoComparator`'s ordered list aggregate was typed VARCHAR whatever it aggregated | In the shared lowering (AGENTS.md invariant 1), every lane re-measured: it is typed as the LIST reducer types it (a list of its value's type), its columns stamped from the subselect's projections, and Postgres spells it `array_agg(v ORDER BY k)` | fixed |
| 2 | **A comparator dedup's CASE** (two `removeDuplicates` tests) is typed Unknown: on a list-less backend the shared carrier pass strips the LITERAL label from the list branch but not from the reduction's stamps | An identity transform on the list branch made Postgres pass, but it is a new list emission in the lowering, which the carrier-purity ratchet bans (`CarrierPurityRatchetTest`: lower to a semantic node, CARRIER_REDESIGN.md tenet #1). The fix is a semantic dedup node with strategy rules | kept: reverted, for the carrier lane |
| 3 | **jsonb reorders an object's keys** (shorter first), where Pure keeps them as written | A Variant as `json` (its text as written), jsonb only where it is compared, grouped or merged; printed minified | kept: Variants stay jsonb (the user's call, 2026-10-12): `json` has no equality, so the switch would reverse the dialect's documented choice and touch every Variant path for these 3 |
| 2 | **Postgres's own limits**: no NUL character in text, a timestamp range | None | kept |
| 2 | `testPairCollectionToString`: `to_jsonb` of a quoted literal, which has no type yet. `testConcatenateTypeInference`: a Variant cell (instances of two subclasses) came back as the driver's jsonb object | The literal cast to text first: fixed. The cell: delivering it as text in the dialect's `normalize` passed, but broke main's Postgres parity harness, which reads the driver's object (2026-10-12, at the merge of the `#SQL{...}#` line). The executor already turns a driver's JSON object into text through that driver's `DriverCells` (DuckDB's `DuckDbCells`); Postgres has none. A `PostgresCells` is the fix, in the plan's P5 connector | literal fixed; the cell kept, for P5 |

**Found in passing:** Snowflake's printf unroll pads with `LPAD`, which also cuts a longer text. A `%3d` of 12345
would likely print `123` there. Not yet checked on Snowflake.

## The other databases

H2 has the most own pins (25), then MySQL (20), MariaDB (17) and SQLite (15); Postgres has 9 after its burn-down.
BigQuery (8) and Trino (6) were burned down hardest, and Databricks and Snowflake have 1 each.

## What PR #34's independent review found beyond PCT (2026-10-13)

Four reviewers, none of whom wrote the code, read the shared code, the Postgres and ClickHouse dialects, and these
docs. They ran each shape on the lanes' own servers (Postgres 16.15, ClickHouse 25.8.2.29). PCT passes on every
lane, so what they found is what PCT's inputs do not reach.

**Fixed in the PR:**
- ClickHouse: an absent nested instance read as a present one with NULL fields (its struct field is now its
  Variant, unwrapped only to read a field of it); a correlated EXISTS ran under 25.8's NULL bug, its setting being on
  by default (it now runs only under a null-rejecting filter, and every other statement turns the setting off);
  a fractional RANGE frame used bare aggregate names (COUNT a UInt64, STDDEV without its OrNull form, MEDIAN
  approximate); a sample deviation or variance of one value was inf, DuckDB's NULL; an as-of join's hash key was
  taken from an inequality of one side.
- Postgres: printf's `%.Nf` rounded ties away from zero, and lost digits past 15 (`%.17f` of 0.1 + 0.2); a raise
  carried a volatile term everywhere, which stopped the planner flattening subqueries and aggregating in parallel
  (now only on a list or map raise, which unnest can take); a list of maps built an array of arrays; a jsonb number
  past a Long crashed the decode, and a jsonb null struct cell reached the caller as the driver's object.
- Shared: the type census accepted a map delivered as an array on every database (now BigQuery and Postgres only);
  the typed ordered list aggregate made SQLite, MariaDB, MySQL and BigQuery write it twice.

**Queued as their own tasks:** zero padding (`%0Nd`) cuts a long number on every database (the lowering's LPAD);
Postgres's Levenshtein is O(n·m²) on long text (12.6 s at 600 characters) and Jaro-Winkler slow past 1,000; a
Postgres struct's date-time, NaN and map fields stay raw (the executor's unwrap would type them).

**Kept, known, not fixed:**
- ClickHouse: a fractional RANGE frame holds its partition's pairs on every row, so its memory is the partition's
  size squared (a large partition fails loudly); a column-valued quantile level outside [0, 1] answers instead of
  raising; a session's limits, recorded in the plan's P5.
- Postgres: `splitPart` with a NULL delimiter splits into characters, and a negative part is NULL (DuckDB counts
  from the end; Pure raises); a precision on `%s` or `%d` is ignored (Pure raises); `hashCode` hashes each value's
  text, so `'10'` and `10` collide; a Variant cast to a `Map<Integer, _>` keeps its keys as text; `MAP_CONCAT` of NULL
  maps is `{}` (DuckDB's NULL); Jaro-Winkler is DuckDB's, which differs from commons-text on some pairs and on two
  empty strings; a supplementary character is one character (the reference counts UTF-16 units). None of these has
  a row in `docs/SEMANTICS_REGISTER.md` yet.
