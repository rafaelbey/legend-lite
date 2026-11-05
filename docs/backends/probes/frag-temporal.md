# TEMPORAL functions and date/time types — SQLite + Postgres capability probe

**Evidence standard.** 853 probe statements executed against real engines: duckdb-jdbc **1.5.0.0**
(reference value for every row), sqlite-jdbc **3.47.1.0**, PostgreSQL **18.4** (zonky embedded).
Files: `probe/probes-temporal{,2,3,4}.tsv`, `probe/out-<backend>-temporal{,2,3,4}.tsv`. Three
iteration rounds — every `D` below means "these candidates were run and here is why they failed".

---

## 0. THE DOMINANT FACT — SQLite has no date/time type

Not a rendering inconvenience. **The type does not exist, and SQLite's response to date-shaped input
it cannot handle is a silent wrong answer, not an error.** Every finding in §5 follows from this.

Three measurements make the point:

```
CAST('2020-01-15' AS DATE)        DuckDB LocalDate:2020-01-15   SQLite Integer:2020
CAST('2020-01-15' AS TIMESTAMP)   DuckDB Timestamp:2020-01-15…  SQLite Integer:2020
CAST('2020-01-15' AS DATETIME)    DuckDB Timestamp:2020-01-15…  SQLite Integer:2020
```

`DATE`/`TIMESTAMP`/`DATETIME`/`TIMESTAMPTZ` all resolve to SQLite **NUMERIC affinity**, and CAST to a
NUMERIC-affinity type parses the *leading numeric prefix* of the string. Every date cast the lowering
emits becomes the integer year. No error. (`I.sqlite.int.affinity{,3,4}`)

The same affinity bites on INSERT: `INSERT INTO t(d DATE) VALUES ('2021')` stores **INTEGER 2021**,
`typeof` = `'integer'` (`I.sel2.numtype/numval`). A year-precision Pure date literal written to a
DATE column stops being a date.

**What the bundled 3.47.1.0 build actually has** (all probed, none assumed):

| function | present | note |
|---|---|---|
| `date()`, `time()`, `datetime()`, `strftime()`, `julianday()` | yes | |
| `unixepoch()` | **yes** | incl. the `'subsec'` 2-arg form |
| `timediff()` | **yes** | returns TEXT `'+0001-05-15 00:00:00.000'`, not a number |
| `printf()` / `format()` | yes | both |

Modifiers present (`SQLITEMOD.*`): `±N days/months/years/hours/minutes/seconds`, `start of
day|month|year`, `weekday N`, `unixepoch`, `julianday`, `auto`, `localtime`, `utc`, `subsec`,
`subsecond`, **`floor`**, **`ceiling`**. Absent (return NULL, silently): `+N milliseconds`,
`+N microseconds`.

---

## 1. Bucket table — one row per construct

Reference = the spelling `AnsiSqlRenderer`/`Spellings.DUCKDB` emits today.

| # | Construct (DuckDB spelling) | SQLite | SQLite construction | PG | Postgres construction |
|---|---|---|---|---|---|
| 1 | **DATE** type | **B** | no type. TEXT ISO-8601 carrier + affinity-safe DDL (`TEXT`, not `DATE`) | **A** | native |
| 2 | **TIMESTAMP** type | **B** | TEXT ISO-8601 carrier, **millisecond ceiling** | **A** | native (µs) |
| 3 | **TIMESTAMPTZ** type | **D** | `datetime('…+02')` → **NULL**; no zone-aware value at all | **A** | native (+ normalize row, §3) |
| 4 | date literal `DATE '2020-01-15'` | **C** | bare `'2020-01-15'` (prefixed form is a syntax error) | **A** | native |
| 5 | timestamp literal | **C** | bare `'2021-01-03 10:20:30.123'` | **A** | native |
| 6 | INTERVAL literal + `d ± interval` | **D** | no interval type, no interval arithmetic — only modifier *strings* | **A** | native; `date-date` → int, `ts-ts` → interval |
| 7 | **EXTRACT** (`date_part('p',x)`) | **C** | `CAST(strftime('%Y',x) AS INTEGER)` for 14 parts; 4 parts **B** (arithmetic); rest **D** | **C** | `date_part` native but **11 part names missing** + type differs → DuckDB-exact needs **B**: `FLOOR(EXTRACT(p FROM x))::bigint` |
| 8 | **TODAY** (`current_date`) | **C** | `date('now','localtime')` — bare `current_date` is **UTC**, §2.1 | **A** | `current_date` |
| 9 | **NOW** (`now()`) | **C** | `datetime('now','localtime','subsec')` — ms only | **A** | `now()` |
| 10 | **DATE_TRUNC_DAY** (`CAST(x AS DATE)`) | **C** | `date(x)` — the CAST spelling silently yields the year integer | **A** | `CAST(x AS DATE)` |
| 11 | **DATE_TRUNC** (`date_trunc('u',x)`) | **B** | per unit: `strftime('%Y-%m-%d %H:00:00',x)` / `datetime(x,'start of month')` / `datetime(x,'-6 days','weekday 1','start of day')` | **C** | native for 12/15 units; **century + millennium disagree in VALUE** (§2.2) → **B**; `isoyear`/`yearweek`/`weekday` → **D** |
| 12 | **MAKE_DATE** (`make_date(y,m,d)`) | **B** | `date(printf('%04d-%02d-%02d',y,m,d))` | **A** | `make_date` native, identical value |
| 13 | **MAKE_TIMESTAMP** | **B** | `strftime('%Y-%m-%d %H:%M:%f', printf(…))`; sub-ms lost | **C** | `make_timestamp(y,mo,d,h,mi,s)` — the `CAST(… AS DOUBLE)` wrapper is a PG syntax error (`type "double" does not exist`); 1-arg micros form → **B** |
| 14 | **ADD_INTERVAL** (`x + to_days(n)`) | **C**/**D** | `date(x,'+n days')`; **month/year require `'floor'`** (§2.3); ms/µs **D** | **C** | `x + n * INTERVAL '1 day'`, `x + (n‖' days')::interval`, or `x + make_interval(days=>n)` — all three verified equal; `to_days()` does not exist |
| 15 | **DATE_DIFF** (`date_diff('u',a,b)`) | **B** | per unit, §2.4 | **B** | per unit boundary-count, §2.4 — **`age()` is wrong on 9/15 cases** |
| 16 | **TIME_BUCKET** | **B** | `datetime(((unixepoch(x)-org)/w)*w+org,'unixepoch')` — exact for hour/day/week; month via `date('1970-01-01','+N months')` | **C** | `date_bin(INTERVAL 'n u', x, origin)` for sub-month; **month/quarter/year rejected** (`timestamps cannot be binned into intervals containing months or years`) → **B** |
| 17 | **TIMEZONE** (`timezone(z,x)`) | **D** | only `'utc'`/`'localtime'`; a named zone returns **NULL**, silently | **A** | `timezone(z,x)` — identical spelling, identical value, identical error on a bad zone |
| 18 | **EPOCH_SECONDS** (`epoch(x)`) | **C** | `unixepoch(x)` (INTEGER; DuckDB returns DOUBLE) | **C** | `EXTRACT(epoch FROM x)` — `epoch()` does not exist |
| 19 | **EPOCH_MS** (`epoch_ms(ts)`) | **C** | `CAST(unixepoch(x,'subsec')*1000 AS INTEGER)` | **B** | `FLOOR(EXTRACT(epoch FROM x)*1000)::bigint` — exact incl. pre-1970 |
| 20 | **FROM_EPOCH_SECONDS** (`to_timestamp(n)`) | **C** | `datetime(n,'unixepoch')` | **A** | `to_timestamp(n)` — same spelling, same value, same TIMESTAMPTZ type |
| 21 | **FROM_EPOCH_MS** (`epoch_ms(BIGINT)`) | **C** | `strftime('%Y-%m-%d %H:%M:%f', n/1000.0, 'unixepoch')` | **B** | `to_timestamp(n/1000.0) AT TIME ZONE 'UTC'` (the bare `to_timestamp` form is **4h off**, §2.5) |
| 22 | **DAYNAME** (`dayname(x)`) | **B** | 7-arm `CASE strftime('%w',x)` — `%A` returns NULL | **C** | `to_char(x,'FMDay')` — **not** `'Day'` (§2.6) |
| 23 | **MONTHNAME** (`monthname(x)`) | **B** | 12-arm `CASE strftime('%m',x)` — `%B` returns NULL | **C** | `to_char(x,'FMMonth')` |
| 24 | **STRFTIME** (`strftime(x,fmt)`) | **C** | **arg order reversed**: `strftime(fmt,x)`; 5 `DateFmt.Part`s unspellable (§4) | **C** | `to_char(x,pattern)` — full `DateFmt.Part` coverage |
| 25 | **STRPTIME** (`strptime(s,fmt)`) | **C**/**D** | `datetime(s)` for ISO-8601 only; arbitrary formats **D** | **C** | `to_timestamp(s,pattern)::timestamp` (the `::timestamp` is load-bearing — §2.5) |

### Bucket totals

| | A native | C rendering | B rewrite | D impossible | total |
|---|---|---|---|---|---|
| **SQLite** | **0** | 13 | 9 | 3 | 25 |
| **Postgres** | **12** | 10 | 3 | 0 | 25 |

SQLite has **zero A rows in this slice** — not one temporal construct survives at the DuckDB
spelling. Postgres has zero D rows: everything is reachable, but 13 of 25 need a dialect row and
three of those (§2) are silent-wrong-answer risks if written naively.

---

## 2. SILENT VALUE DIVERGENCE — the highest-value section

Every row here **parses and returns a value on both backends**. Only the value is wrong.

### 2.1 `current_date` / `CURRENT_TIMESTAMP` on SQLite are **UTC**, not local

Caught live, mid-session, at the day boundary:

| probe | wall clock | DuckDB | Postgres | SQLite |
|---|---|---|---|---|
| `TODAY.currentdate` | 15:54 EDT | `2026-07-31` | `2026-07-31` | `2026-07-31` |
| `J.today.all` | **21:32 EDT** | `2026-07-31` | `2026-07-31` | **`2026-08-01`** |
| `NOW.currentts` | 15:54 EDT | `…15:54:15` | `…15:54:42` | **`…19:54:21`** |

SQLite's `CURRENT_TIMESTAMP` is UTC and second-precision TEXT. For any process not running in UTC,
`TODAY` is wrong for the last N hours of every day. Fix: `date('now','localtime')` → `2026-07-31` ✓
(`J.today.sqlite.local`).

### 2.2 `date_trunc('century'|'millennium')` — DuckDB and Postgres disagree

```
date_trunc('century',    TIMESTAMP '2021-01-03 10:20:30.123456')
    DuckDB → 2000-01-01 00:00:00     Postgres → 2001-01-01 00:00:00
date_trunc('millennium', …)
    DuckDB → 2000-01-01 00:00:00     Postgres → 2001-01-01 00:00:00
```

Postgres is calendrically correct (the 21st century begins 2001); DuckDB uses `year/100*100`. Same
function name, same unit string, different day. `decade` agrees (both `2020-01-01`).
Matching PG construction: `make_timestamp((EXTRACT(year FROM x)::int/100)*100,1,1,0,0,0)` → verified
`2000-01-01` (`I.cent.pg.calc`).

### 2.3 SQLite month/year arithmetic OVERFLOWS where DuckDB and Postgres CLAMP

| expression | DuckDB | Postgres | SQLite (naive) | SQLite `'floor'` |
|---|---|---|---|---|
| `2020-01-31 + 1 month` | `2020-02-29` | `2020-02-29` | **`2020-03-02`** | `2020-02-29` ✓ |
| `2020-02-29 + 1 year` | `2021-02-28` | `2021-02-28` | **`2021-03-01`** | `2021-02-28` ✓ |
| `2020-03-31 − 1 month` | `2020-02-29` | `2020-02-29` | **`2020-03-02`** | `2020-02-29` ✓ |
| `2020-01-31 + 3 months` | `2020-04-30` | `2020-04-30` | `2020-05-01` | `2020-04-30` ✓ |
| `2020-01-15 + 1 month` | `2020-02-15` | `2020-02-15` | `2020-02-15` | `2020-02-15` ✓ |

**Every `date(x,'+n month(s)|year(s)')` SQLite emits MUST carry the `'floor'` modifier.** Verified it
is a no-op on non-overflowing dates, so it is unconditional. (`I.sqlite.eom.*`, `I.sqlite.leap.floor`,
`I.sqlite.normal.floor`.) `'ceiling'` reproduces the naive overflow — do not use it.

### 2.4 `date_diff` — Postgres `age()` is wrong on 9 of 15 cases; boundary-count is exact on 15/15

DuckDB `date_diff(unit,a,b)` counts **partition-boundary crossings**, not elapsed time. `age()` measures
elapsed time. They are different functions.

| case | DuckDB | PG via `EXTRACT(u FROM age(b,a))` | PG boundary-count |
|---|---|---|---|
| `month` 2020-01-31 → 2020-02-01 | **1** | **0** ✗ | 1 ✓ |
| `month` 2020-01-15 → 2021-06-30 | **17** | **5** ✗ | 17 ✓ |
| `month` 2021-03-01 → 2020-01-01 | **−14** | **−2** ✗ | −14 ✓ |
| `month` 2020-02-01 → 2020-01-31 | **−1** | — | −1 ✓ |
| `year` 2020-12-31 → 2021-01-01 | **1** | **0** ✗ | 1 ✓ |
| `day` 2020-01-01 23:00 → 2020-01-02 01:00 | **1** | **0** ✗ | 1 ✓ |
| `day` 2020-01-01 → 2020-03-05 | **64** | **4** ✗ | 64 ✓ |
| `hour` 10:59 → 11:01 | **1** | **0** ✗ | 1 ✓ |
| `hour` 11:01 → 10:59 | **−1** | — | −1 ✓ |
| `second` …00.9 → …01.1 | **1** | **0.2** ✗ | 1 ✓ |
| `millisecond` .000000 → .123456 | **123** | **123.456** ✗ | 123 ✓ |
| `quarter` 2020-03-31 → 2020-04-01 | **1** | 1 | 1 ✓ |
| `week` 2021-01-03 → 2021-01-04 | **0** | 0 | 0 ✓ |

The exact Postgres constructions (`DATE_DIFF.*.pgb`, `I.wkdiff.*`, `J.*.pgb`):

```sql
month   (EXTRACT(year FROM b)::int - EXTRACT(year FROM a)::int)*12
      + (EXTRACT(month FROM b)::int - EXTRACT(month FROM a)::int)
year     EXTRACT(year FROM b)::int - EXTRACT(year FROM a)::int
quarter (EXTRACT(year FROM b)::int - EXTRACT(year FROM a)::int)*4
      + (EXTRACT(quarter FROM b)::int - EXTRACT(quarter FROM a)::int)
day      CAST(b AS date) - CAST(a AS date)
week    (CAST(b AS date) - CAST(a AS date)) / 7        -- integer division
hour     FLOOR(EXTRACT(epoch FROM (date_trunc('hour',b) - date_trunc('hour',a)))/3600)::bigint
second  (FLOOR(EXTRACT(epoch FROM b)) - FLOOR(EXTRACT(epoch FROM a)))::bigint
milli   (FLOOR(EXTRACT(epoch FROM b)*1000) - FLOOR(EXTRACT(epoch FROM a)*1000))::bigint
```

**`week` is the trap.** DuckDB's `date_diff('week',…)` is **not** boundary-counting and **not**
Monday-aligned — it is plain `(b−a)/7` truncated toward zero. Probed on 7 spans (Sun→Mon 0, Mon→Mon 1,
Sat→Sun 0, Fri→Fri 1, Mon→Sun 0, −7d −1, −6d 0). A Monday-aligned boundary count — the intuitive
construction, and the one I wrote first — returns **1** where DuckDB returns **0** on Sun→Mon.

Two naive Postgres constructions that look right and are not:
* `FLOOR(EXTRACT(epoch FROM (b-a))/86400)` for `day` → **0** where DuckDB gives **1**
  (`DATE_DIFF.d.ts.pgnaive`). Boundary crossing ≠ elapsed 24h.
* `EXTRACT(month FROM age(b,a))` drops the year component entirely.

SQLite constructions verified exact: month `(strftime('%Y',b)-strftime('%Y',a))*12 +
(strftime('%m',b)-strftime('%m',a))`; day `CAST(julianday(date(b))-julianday(date(a)) AS INTEGER)`
— **the `date()` wrapper is required**: `julianday(b)-julianday(a)` on the 23:00→01:00 pair gives
**0** (`DATE_DIFF.d.naive.sqlite`); second `unixepoch(b)-unixepoch(a)`. `timediff()` exists but
returns a formatted TEXT interval, not a count — useless for `DATE_DIFF`.

### 2.5 Timezone — TIMESTAMPTZ, `to_timestamp`, `to_timestamp(text,fmt)`

**The JVM here is `America/New_York` (EDT, −04:00 at probe time), and both JDBC drivers convert
`timestamptz` into the JVM zone.** The Postgres session `TimeZone` GUC does **not** affect what the
driver hands back as a Java object — only the server-side `::varchar` text:

| after | server text (`::varchar`) | `rs.getObject(1)` |
|---|---|---|
| `SET TIME ZONE 'America/New_York'` | `2021-01-03 05:20:30-05` | `Timestamp:2021-01-03 05:20:30.0` |
| `SET TIME ZONE 'GMT'` | `2021-01-03 10:20:30+00` | **`Timestamp:2021-01-03 05:20:30.0`** |

Same instant, but the Java object's *rendered wall clock* is the JVM's, forever, regardless of the
GUC. So **`timeZone='GMT'` on the connection is not sufficient**: a golden or expectation that
compares the printed form of a fetched `timestamptz` compares against the JVM zone.

`H2_BACKEND.md` §7 records that the corpus pins `timeZone='GMT'` and *"every temporal expectation in
the corpus depends on it."* **This JVM is not GMT.** Implications:

* Any expectation over a `TIMESTAMPTZ` round-trip is machine-dependent. CI in UTC and a developer
  laptop in `America/New_York` produce different printed values from the same database.
* The fix is not another GUC. Either (a) fetch through `rs.getObject(i, OffsetDateTime.class)` and
  normalize to UTC in `SqlDialect.normalize`, or (b) forbid `TIMESTAMPTZ` from crossing the JDBC
  boundary at all — render `AT TIME ZONE 'UTC'` so the wire type is a naive `TIMESTAMP`, which is
  zone-immune. `I.tz.utcrender.*` confirms `AT TIME ZONE 'UTC'` gives byte-identical
  `2021-01-03 10:20:30` on both backends.
* The harness should also pin `-Duser.timezone=GMT` (or `TZ=UTC`) — the probe run demonstrates that
  without it, `SQLite CURRENT_TIMESTAMP` vs `DuckDB now()` differ by exactly the JVM offset (§2.1),
  and the day flipped between two runs four hours apart.

Two constructions that silently shift by the session offset:

```
FROM_EPOCH_MS   DuckDB epoch_ms(1609669230123)          → 2021-01-03 10:20:30.123   (naive UTC)
                PG     to_timestamp(1609669230123/1000.0) → 2021-01-03 05:20:30.123 ✗ (−5h)
                PG     to_timestamp(…) AT TIME ZONE 'UTC'  → 2021-01-03 10:20:30.123 ✓
STRPTIME        DuckDB strptime('2021-01-03 10:20:30',…) → 2021-01-03 10:20:30       (TIMESTAMP)
                PG     to_timestamp('2021-01-03 10:20:30',…) → 2021-01-03 10:20:30-05 ✗ (TIMESTAMPTZ)
                PG     to_timestamp(…)::timestamp          → 2021-01-03 10:20:30     ✓
```

`date_trunc('day', tstz)` is likewise zone-dependent on *both* backends: with `TimeZone=America/New_York`
`2021-01-03 02:20:30+00` truncates to **Jan 2**; with `TimeZone=GMT` to **Jan 3** (`ZZ.tz.trunc.*`).

SQLite: `datetime(x,'America/New_York')` returns **NULL**, silently. `'localtime'` uses the *process*
zone, not a named one. There is no zone-aware value in SQLite — `TIMEZONE` is **D**.

### 2.6 `dayname` / `monthname` — Postgres blank-pads to 9 characters

```
to_char(DATE '2021-01-03','Day')   → '[Sunday   ]'   ✗   dayname()   → '[Sunday]'
to_char(DATE '2021-01-03','FMDay') → '[Sunday]'      ✓
to_char(DATE '2021-05-03','Month') → '[May       ]'  ✗   monthname() → '[May]'
to_char(DATE '2021-05-03','FMMonth')→ '[May]'        ✓
```

The `FM` prefix is mandatory. Without it every short weekday/month name carries trailing spaces and
every string comparison, hash and JSON leaf differs. Spellings match DuckDB exactly with `FM`
(`Sunday`, `January`, `May`, `Wednesday`). `lc_time` is `en_US.UTF-8`; `TMDay` (locale-aware) also
gave `Sunday` here, but it is locale-dependent by definition — **use `FMDay`, never `TMDay`**.

SQLite has **no** month/weekday names: `strftime('%A',…)` and `%B` both return **NULL**, and — worse —
**one unrecognized code NULLs the entire format string**: `strftime('%Y-%m-%d %A', x)` → `NULL`
(`I.sqlite.fmt.mixed`). DuckDB *errors* on an unknown code; SQLite returns NULL.

### 2.7 Fractional seconds — truncate vs round vs 3-digit ceiling

| input | DuckDB | Postgres | SQLite (`%f`) |
|---|---|---|---|
| `…30.123456` | `.123456` | `.123456` | `.123` |
| `…30.1234567` | `.123456` (trunc) | **`.123457`** (round) | `.123` |
| `…30.123456789` | `.123456` | **`.123457`** | `.123` |
| `…59.9999999` | `10:20:59.999999` | **`10:21:00`** | `10:20:59.999` |

Postgres **rounds** the 7th digit and can therefore roll the minute; DuckDB **truncates**. The repo
already carries a note about *"silently truncated 59.999999"* (`Scalars.java:2181`) — on Postgres the
same literal becomes the next minute.

**SQLite's ceiling is milliseconds, everywhere.** `%f` is `SS.SSS`; `unixepoch(x,'subsec')` yields
`1.609669230123E9`; `datetime(x,'subsec')` yields `.123`; the julianday round trip yields `.123`.
There is no microsecond representation. `DateFmt.Part.SUBSEC_MICRO` cannot be honoured.

### 2.8 BC years — a one-year off-by-one between DuckDB and Postgres

The cleanest demonstration is one expression:

```
make_date(-44, 3, 15)      DuckDB → '0045-03-15 (BC)'      Postgres → '0044-03-15 BC'
make_date(0, 1, 1)         DuckDB → '0001-01-01 (BC)'      Postgres → ERROR  date field value out of range
```

Julian day numbers, measured (`I.bc.*.jd`):

| literal | DuckDB JDN | Postgres JDN |
|---|---|---|
| `DATE '-0044-03-15'` | 1705063 | **parse error** |
| `DATE '0044-03-15 BC'` | **1737205** (= AD 44!) | 1705428 |
| `DATE '0043-03-15 BC'` | — | 1705793 |

Three separate defects:

1. **DuckDB silently ignores a ` BC` suffix.** `DATE '0044-03-15 BC'` parses as **AD 44**
   (JDN 1737205, `era`=1, `date_part('year')`=44). No error. This is a silent wrong answer in the
   *incumbent*.
2. **Postgres cannot parse an ISO negative-year literal.** `DATE '-0044-03-15'` →
   `ERROR: time zone displacement out of range`. `PureDateLiteral`'s documented BC form
   (`-44-03-15`, `PureDateLiteral.java:34`) has **no Postgres literal**; it must be rewritten to
   `'0043-03-15 BC'` — sign flip **and** decrement, because DuckDB/ISO have a year 0 and Postgres
   does not.
3. **Both JDBC drivers drop the BC era.** DuckDB `TIMESTAMP '-0044-03-15 10:20:30'` →
   `Timestamp:0045-03-15 10:20:30.0`; Postgres `TIMESTAMP '0044-03-15 10:20:30 BC'` →
   `Timestamp:0044-03-15 10:20:30.0`. `Executor.java:272`'s LocalDateTime carrier is required on
   Postgres too, and its `ldt.getYear() < 1` guard is the right test — but only if the value comes
   back as `LocalDateTime`; the `java.sql.Timestamp` path is already lossy.

SQLite agrees with **DuckDB's** astronomical numbering: `julianday('-0044-03-15')` = 1705062.5 (the
midnight of JDN 1705063), and `date(1705062.5)` round-trips to `-0044-03-15`.

### 2.9 Year > 9999 — pgjdbc silently corrupts DATE (not TIMESTAMP)

```
SELECT DATE '10000-01-01'                          PG server text: '10000-01-01'   ✓
                                                   PG date_part('year'): 10000.0   ✓
                                                   PG rs.getObject(1): Date:0000-01-01   ✗✗
SELECT DATE '99999-12-31'                          PG text '99999-12-31' ✓  object Date:9999-12-31 ✗
SELECT TIMESTAMP '10000-01-01 00:00:00'            PG object Timestamp:10000-01-01 00:00:00.0 ✓
```

The server is right; **the driver's `java.sql.Date` decoder mis-parses a 5-digit year**, and it is
DATE-specific — TIMESTAMP survives to 294276. DuckDB returns `LocalDate:+10000-01-01` correctly.
Any Pure model with a far-future sentinel date (`Lowerer.java:2209` uses `9999-12-31`, which is safe;
anything past it is not) reads back wrong on Postgres with no error.

Postgres also rejects **year zero**: `DATE '0000-01-01'` → `ERROR: date/time field value out of
range`. DuckDB accepts it. SQLite accepts `date('0000-01-01')` and returns `'0000-01-01'`.

SQLite's range ends hard at 9999: `date('10000-01-01')` → **NULL**, `date('9999-12-31','+1 day')` →
**NULL**, `date(5373484.5)` → **NULL**. Lower end `date(0.0)` → `-4713-11-24`. All silent.

### 2.10 EXTRACT part values that differ

`dow`/`isodow`/`week`/`isoyear`/`doy`/`quarter` **agree** between DuckDB and Postgres — the classic
off-by-one is *not* present here (both: Sun `dow`=0, `isodow`=7, Mon `dow`=1/`isodow`=1,
2021-01-03 `week`=53 ISO, `isoyear`=2020). Good news, and it was worth measuring.

The divergences are elsewhere:

| part | DuckDB `date_part` | Postgres `date_part` |
|---|---|---|
| `second` | `Long:30` (integer, truncated) | **`Double:30.123456`** |
| `millisecond` | `Long:30123` | **`Double:30123.456`** |
| `microsecond` | `Long:30123456` | `Double:3.0123456E7` (same value, `Double`) |
| everything else | `Long` | `Double` |
| `julian` | `Double:2459218.430904207` | `Double:…207` / `EXTRACT` → `BigDecimal:2459218.43090420666666666667` |

**`date_part('second', …)` is the sharpest one**: `30` vs `30.123456` from the same call. A model that
extracts seconds gets a different number on Postgres. `FLOOR(EXTRACT(p FROM x))::bigint` restores both
the value **and** the `Long` Java type on all 14 parts probed (`I.exint.*`). `FLOOR` and `TRUNC` agree
on the pre-1970 case tested (`I.exint.neg.*`).

Postgres **rejects** 11 part names DuckDB accepts: `era`, `timezone`, `timezone_hour`,
`timezone_minute` (on a naive timestamp), `dayofweek`, `dayofmonth`, `dayofyear`, `weekday`,
`weekofyear`, `yearweek`, and `date_trunc` additionally rejects `isoyear`/`yearweek`/`weekday`.
These are loud errors, not silent — a `C` (alias to the supported name) for `dayofweek`→`dow`,
`dayofmonth`→`day`, `dayofyear`→`doy`, `weekofyear`→`week`; a `B` for `yearweek`
(`isoyear*100 + week`) and `era` (`CASE WHEN year>0 THEN 1 ELSE 0 END`).

### 2.11 SQLite silently returns NULL where the others error

| input | DuckDB | Postgres | SQLite |
|---|---|---|---|
| `date('not-a-date')` | ERROR | ERROR | **NULL** |
| `date('2021-1-3')` (unpadded) | `2021-01-03` | `2021-01-03` | **NULL** |
| `datetime('2021-01')` | ERROR | ERROR | **NULL** |
| `datetime('2021-01-03 10:20:30+02')` | — | — | **NULL** |
| `strftime('%Q', x)` (bad code) | ERROR | — | **NULL** |
| `strftime(x, '%Y-%m-%d')` (args swapped) | OK (DuckDB accepts both orders) | — | **NULL** |
| `datetime(x, 'America/New_York')` | — | — | **NULL** |
| `datetime(x, '+1500 milliseconds')` | — | — | **NULL** |
| `date('9999-12-31','+1 day')` | `10000-01-01` | `10000-01-01` | **NULL** |

And the one that is worse than NULL: **`datetime('2021')` → `-4707-06-06 12:00:00`.** SQLite reads a
bare 4-digit string as a **Julian day number**, not a year. A year-precision Pure literal handed to
any SQLite date function produces a plausible-looking date in the 48th century BC.

### 2.12 Miscellaneous

* `TIME '10:20:30.123456'` → DuckDB `LocalTime:10:20:30.123456`, **Postgres `Time:10:20:30`** —
  pgjdbc's `java.sql.Time` drops fractional seconds.
* `TIME WITH TIME ZONE '10:20:30+02'` → DuckDB `OffsetTime:10:20:30+02:00`,
  Postgres `Time:03:20:30` (shifted into the JVM zone, offset discarded).
* `ORDER BY hired DESC LIMIT 1` → DuckDB/SQLite `2022-03-20`, **Postgres `<null>`** (PG defaults
  NULLS FIRST on DESC). Not strictly this slice, but it lands on a date column.
* `DATE 'infinity'` → DuckDB `LocalDate:+5881580-07-11`, Postgres `Date:8994-08-16` (driver garbage
  from the sentinel). Neither is usable; forbid the literal.
* SQLite `REAL` is 8-byte; Postgres `REAL` is `float4`. A Julian-day-as-REAL carrier would lose
  time-of-day on Postgres — `2459217.93` came back as `Float:2459218.0` (`I.sel2.c`). Relevant if
  §5's carrier question is ever answered with "numeric Julian day".

---

## 3. Returned Java type divergence — candidate `SqlDialect.normalize` rows

| SQL value | DuckDB | Postgres | SQLite |
|---|---|---|---|
| `DATE` literal / column | `java.time.LocalDate` | `java.sql.Date` | **`String`** |
| `DATE` year > 9999 | `LocalDate:+10000-01-01` | **`Date:0000-01-01`** (corrupt) | `<null>` |
| `DATE` BC | `LocalDate:-0044-03-15` | `Date:0044-03-15` (era dropped) | `String:-0044-03-15` |
| `TIMESTAMP` | `java.sql.Timestamp` | `java.sql.Timestamp` | **`String`** |
| `TIMESTAMP` BC | `Timestamp` (era dropped → +45) | `Timestamp` (era dropped) | `String` |
| **`TIMESTAMPTZ`** | **`java.time.OffsetDateTime`** | **`java.sql.Timestamp`** (zone erased, JVM-local wall clock) | `String` (raw text, uninterpretable) |
| `TIME` | `java.time.LocalTime` (µs kept) | `java.sql.Time` (**µs dropped**) | `String` |
| `TIMETZ` | `java.time.OffsetTime` | `java.sql.Time` (offset dropped) | — |
| `INTERVAL` | `String` (`"1 day"`) | **`org.postgresql.util.PGInterval`** | — |
| `date - date` | `Long` | `Integer` | `Integer`/`Double` (julianday) |
| `date_part(...)` | `Long` (BIGINT) | **`Double`** | `String` (strftime) → cast needed |
| `EXTRACT(... FROM ...)` | `Long` | **`BigDecimal`** (PG 14+ returns `numeric`) | n/a |
| `EXTRACT(epoch …)` | `Double` | `BigDecimal` | `Integer` (unixepoch) |
| `now()` | `OffsetDateTime` | `Timestamp` | `String` (**UTC**) |
| `LOCALTIMESTAMP` | `Timestamp` | `Timestamp` | — |
| `REAL` column | `Float` | `Float` (float4) | `Double` (8-byte) |

Concrete normalize rows to write:

1. **Postgres `TIMESTAMPTZ` → UTC-normalized carrier.** Highest priority. Either fetch as
   `OffsetDateTime` or render `AT TIME ZONE 'UTC'` so the wire type is naive `TIMESTAMP`. Today the
   value silently carries the JVM zone's wall clock.
2. **Postgres `date_part`/`EXTRACT` → `Long`.** `Double`/`BigDecimal` where DuckDB gives `Long`;
   handled at render time by `FLOOR(...)::bigint` rather than in the codec, which is cheaper.
3. **Postgres `DATE` with year > 9999.** Driver-level corruption; the only safe fix is rendering
   `CAST(d AS VARCHAR)` for out-of-range years, or fetching via `getObject(i, LocalDate.class)`.
   Must at minimum be a loud guard, not a silent wrong year.
4. **Postgres `TIME` → fractional seconds lost.** Fetch as `LocalTime`.
5. **Postgres `PGInterval`.** Nothing in the IR consumes an interval as a value today; if it ever
   does, this is a driver-specific class the executor would have to import — prefer never returning
   a bare interval.
6. **SQLite: everything is `String`.** See §5.
7. **BC era on both.** `Executor.fetch`'s `LocalDateTime` fallback (`Executor.java:272-290`) must be
   extended to Postgres and to `java.sql.Date` (currently only `java.sql.Timestamp` is guarded).

---

## 4. `DateFmt.Part` → dialect vocabulary (measured, all 37 codes probed)

| `DateFmt.Part` | DuckDB | SQLite 3.47.1.0 | Postgres `to_char` |
|---|---|---|---|
| `YEAR4` | `%Y` → `2021` | `%Y` ✓ | `YYYY` ✓ |
| `MONTH2` | `%m` | `%m` ✓ | `MM` ✓ |
| `DAY2` | `%d` | `%d` ✓ | `DD` ✓ |
| `HOUR2` | `%H` | `%H` ✓ | `HH24` ✓ |
| `MIN2` | `%M` | `%M` ✓ | `MI` ✓ |
| `SEC2` | `%S` | `%S` ✓ | `SS` ✓ |
| `SUBSEC_MICRO` | `%f` → `123456` | **`%f` → `30.123`** — different meaning *and* ms-only | `US` → `123456` ✓ |
| `SUBSEC_MIN` | `%g` → `123` | **`%g` → `20`** (SQLite `%g` = 2-digit ISO year!) | **no equivalent** — `MS`→`123` fixed-3; trailing-zero trim is host-side |
| `MONTH_ABBREV` | `%b` → `Jan` | **NULL** | `Mon` ✓ |
| `MONTH_NAME` | `%B` → `January` | **NULL** | `FMMonth` ✓ |
| `WEEKDAY_NAME` | `%A` → `Sunday` | **NULL** | `FMDay` ✓ |
| `HOUR12` | `%I` → `10` | `%I` ✓ | `HH12` ✓ |
| `HOUR12_NOPAD` | `%-I` → `10` | **NULL** | `FMHH12` ✓ |
| `AMPM` | `%p` → `AM` | `%p` ✓ | `AM` ✓ |

**`%f` and `%g` mean different things in DuckDB and SQLite.** Emitting DuckDB's format string to
SQLite gives `30.123` where `123456` was meant, and `20` where minimal-subseconds was meant — both
plausible-looking, neither flagged. This is the single most dangerous line in the table because
`DateFmt`'s whole design premise (`DateFmt.java:5-14`) is that each dialect spells parts in its own
vocabulary; the codes merely *look* portable.

Postgres covers every `DateFmt.Part` except `SUBSEC_MIN`, which has no `to_char` pattern (no
trailing-zero trimming) — a **B**: emit `US` and trim host-side, or emit `FF6`.
SQLite cannot spell 5 of 14 parts; any format containing one returns NULL for the whole string.

---

## 5. What SQLite's lack of a date type costs — verdict

**Verdict: not a rendering problem (C), not fatal (D). It is a CARRIER REDESIGN — bucket B — and it
is exactly the same shape of decision as §4.1's collection-carrier deferral in `H2_BACKEND.md`.**

The reasoning, grounded in the probes:

**It is not C, and this is provable.** A rendering override changes a *name or syntax* and preserves
the value. Here the value's *representation* changes: on DuckDB `hired` is a `LocalDate`, on SQLite
it is a `String`. The lowering emits `CAST(x AS DATE)` for `DATE_TRUNC_DAY`; on SQLite that returns
`Integer:2020`, not an error. It emits `date_part('year', x)`; SQLite has no such function. It emits
`x + to_days(1)`; SQLite has no `+` for dates at all. Not one of the 25 constructs survives at the
DuckDB spelling — **A = 0**. A dialect that only renames functions cannot get there, because the
thing being renamed does not have the same type on both sides.

**It is not D, and this is also provable.** 22 of 25 constructs were *constructed and returned the
DuckDB value*, including the hard ones: `DATE_TRUNC` for every unit including ISO week
(`datetime(x,'-6 days','weekday 1','start of day')` → `2020-12-28`, exact); `TIME_BUCKET` for hour,
day, week and month (`datetime(((unixepoch(x)+259200)/604800)*604800-259200,'unixepoch')` →
`2020-12-28`, exact, and `2021-01-04` for the following Tuesday); `DATE_DIFF` for month, year, day,
hour and second; `MAKE_DATE`; `DAYNAME`/`MONTHNAME` as CASE chains. SQLite's text dates even sort and
compare correctly, because ISO-8601 is lexicographically ordered — `max(hired)`, `min(hired)`,
`hired > '2020-01-01'` and BC comparison (`'-0044-03-15' < '0001-01-01'` → `lt`) all give the DuckDB
answer with no help at all.

**The three genuine D's are narrow and honest:** `TIMESTAMPTZ` (no zone-aware value exists; named
zones return NULL), `INTERVAL` as a *value* (only modifier strings, so an interval cannot be computed,
stored or passed), and sub-millisecond arithmetic (`'+1 microsecond'` → NULL). All three are declared-
gap-registry rows (`H2_BACKEND.md` §9), not walls.

**What the redesign actually is.** Three decisions, none of them a spelling table:

1. **Pin the carrier to TEXT ISO-8601, and pin it in the DDL.** `Ddl` must emit `TEXT` — *not*
   `DATE`, `TIMESTAMP` or `DATETIME` — for temporal columns on SQLite. Those type names carry NUMERIC
   affinity, which turns `'2021'` into the integer 2021 on INSERT and every `CAST(… AS DATE)` into a
   year integer on read. This is the single highest-value line in this fragment: it is one DDL change
   that removes an entire silent-corruption class.
2. **`SqlDialect.normalize` must parse.** Every temporal cell arrives as `String`. The dialect has to
   turn it back into `LocalDate`/`LocalDateTime` using the *declared* Pure type — which
   `Executor.java:18-19`'s "no-sniffing" contract already provides. This is the codec row
   `H2_BACKEND.md` §8 predicted would "materialize with the first real backend codec row"; SQLite is
   a second instance of it, and a stronger one than H2's `byte[]`-for-JSON.
3. **A guard rail, because SQLite's failure mode is NULL.** Eleven distinct malformed inputs return
   NULL instead of erroring (§2.11). Where DuckDB and Postgres fail loudly, SQLite produces a NULL
   row that flows to the result. The dialect needs either strict input validation at the lowering
   boundary or a post-condition — otherwise SQLite's test results are systematically *greener* than
   they should be, which is the failure mode `H2_BACKEND.md` §10 names ("don't create a soft
   divergence bucket").

**On the live partial-precision constraint.** Hour-precision and minute-precision DateTimes are
distinct *values* in this project. On DuckDB and Postgres they survive natively as
`2021-01-03 10:00:00` vs `2021-01-03 10:20:00` (`EDGE.partial.hour/min`) and the ISO-prefix string
carrier `Scalars.java:2069` already uses compares correctly. On SQLite the ISO-prefix carrier is
*more* natural, not less — it is already the storage form. The hazards are two: `datetime('2021')`
reading as a Julian day (§2.11), and NUMERIC affinity converting `'2021'` to an integer on insert
(§0). Decision 1 above fixes the second; the first argues for never handing a partial-precision
literal to a SQLite date function — pad it at the lowering boundary, which `Scalars.java:2591`'s
`partialComparable` already does for comparison.

**Cost, stated honestly.** ~9 B-bucket constructions to author and pin, one DDL policy change, one
codec, one guard rail, and a permanent millisecond ceiling on `TIMESTAMP` (`DateFmt.Part.SUBSEC_MICRO`
is unreachable). That is a phase of work, not a dialect afternoon — and it is the same work Postgres
needs at 3 rows instead of 9, so it is not throwaway.

---

## 6. Postgres — recommended dialect rows, ranked

1. `date_diff` → the per-unit boundary-count constructions in §2.4. **Never `age()`.**
   Week is `(b−a)/7`, never a Monday-aligned count.
2. `TIMESTAMPTZ` → render `AT TIME ZONE 'UTC'` at the JDBC boundary, or fetch `OffsetDateTime`.
   Pin `TZ=UTC` in the harness/CI as well as `timeZone='GMT'` on the connection.
3. `EXTRACT`/`date_part` → `FLOOR(EXTRACT(p FROM x))::bigint`; alias the 4 renameable parts, rewrite
   `yearweek` and `era`, declare the rest.
4. `DAYNAME`/`MONTHNAME` → `to_char(x,'FMDay'|'FMMonth')`. The `FM` is not cosmetic.
5. `date_trunc('century'|'millennium')` → `make_timestamp((year/100)*100,1,1,0,0,0)`.
6. `FROM_EPOCH_MS` → `to_timestamp(n/1000.0) AT TIME ZONE 'UTC'`; `EPOCH_MS` →
   `FLOOR(EXTRACT(epoch FROM x)*1000)::bigint`.
7. `STRPTIME` → `to_timestamp(s,pat)::timestamp` — the cast is load-bearing.
8. `MAKE_TIMESTAMP` → drop the `CAST(… AS DOUBLE)` wrapper (`type "double" does not exist`).
9. BC literals → rewrite ISO `-YYYY` to `(YYYY−1) BC`; reject year 0.
10. Guard DATE year > 9999 loudly rather than letting the driver return `0000-01-01`.
11. `TIME_BUCKET` → `date_bin` for sub-month units; arithmetic construction for month/quarter/year
    (`date_bin` rejects them).
