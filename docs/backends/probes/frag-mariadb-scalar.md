# MariaDB backend — SCALAR FUNCTION surface (strings/regex/encoding · math/bitwise/arith/null · temporal)

> **Evidence standard.** 586 probe statements executed against **real MariaDB 11.4.5** (MariaDB4j
> embedded) and against **DuckDB 1.5.0.0** as the reference value for every probe, on the identical
> fixture. Probe file `probe/probes-mdb-scalar.tsv`; outputs `probe/out-mariadb-scalar.tsv` and
> `probe/out-duckdb-scalar.tsv`. Where documentation and execution disagreed, execution won —
> several documented claims (`ONLY_FULL_GROUP_BY` on by default; `ROUND` half-up; MariaDB lacks
> `JSON_TABLE`) were **refuted** by execution and are corrected below.
>
> Scope: 111 `SqlFn` constants. Buckets per `docs/H2_BACKEND.md` §2 — **A** native (DuckDB's own
> spelling, same value) · **C** rendering override · **B** rewrite/composite · **D** impossible.

---

## 1. Verdict and bucket totals

| | Strings/regex/encoding | Math/bitwise/arith/null | Temporal | **Total** | |
|---|---|---|---|---|---|
| **A** native | 13 | 29 | 5 | **47** | 42% |
| **C** rendering only | 9 | 11 | 2 | **22** | 20% |
| **B** rewrite pass | 15 | 10 | 11 | **36** | 32% |
| **D** impossible | 4 | 1 | 1 | **6** | 5% |
| | 41 | 51 | 19 | **111** | |

**95% of the scalar surface is reachable.** But the headline is not the D count — it is that
**MariaDB's failure mode is silent wrong answers, not errors.** 153 of 586 probes returned `OK` on
both backends with *different values*. Six constructs (`||`, `LENGTH`, `MAKEDATE`, `FORMAT`,
`REGEXP_REPLACE`, `ROUND`) parse identically and return the wrong answer. That is a larger, more
dangerous surface than H2's, whose gaps mostly announce themselves as `Function not found`.

---

## 2. Session configuration legend-lite MUST pin (analogue of H2_BACKEND.md §7)

**Measured default session** (`SELECT @@sql_mode` etc. on a stock MariaDB 11.4.5):

```
sql_mode                  = IGNORE_SPACE,STRICT_TRANS_TABLES,ERROR_FOR_DIVISION_BY_ZERO,
                            NO_AUTO_CREATE_USER,NO_ENGINE_SUBSTITUTION
time_zone                 = SYSTEM      (system_time_zone = EDT on this host)
character_set_connection  = utf8mb4
collation_connection      = utf8mb4_general_ci
lower_case_table_names    = 2
div_precision_increment   = 4
lc_time_names             = en_US
```

`ONLY_FULL_GROUP_BY` is **NOT** in MariaDB 11.4's default (probe `a2.onlyfullgb` → 0). That is a
MySQL 5.7+ default, not a MariaDB one — see §6.

### 2.1 The five settings that are correctness-load-bearing

Every row below was **verified by mutating the session and re-probing** (probe ids `z.*`).

| Setting | Default | Must pin to | Why — measured |
|---|---|---|---|
| `sql_mode` += **`ANSI_QUOTES`** | off | **on** | `SELECT "name" FROM emp` returns the **string `'name'`**, not alice's name (`cfg.dq3`). legend-lite's `AnsiSqlRenderer.ident()` quotes every identifier with `"` — **every quoted column reference in the engine would silently return a constant**. With `ANSI_QUOTES` set, `z.dq.after` returns `alice`. This is the single highest-severity finding in the slice, and it is a **rendering-wide** decision: MariaDB is the only backend in this matrix where `"x"` is a string. |
| `sql_mode` += **`PIPES_AS_CONCAT`** | off | **on** | `'a' \|\| 'b'` returns **`0`** (Integer) — `\|\|` is logical OR (`cfg.pipes1`). It parses, returns a number, and never errors. After the pin, `z.pipes.after` → `'ab'`. Note the residue: even then `\|\|` **propagates NULL** (`z.pipes.null` → 1) whereas legend-lite's `CONCAT` node is null-**skipping**, so `UC_FIRST`/`LC_FIRST` still cannot use `\|\|` (§4). |
| `sql_mode` += **`NO_BACKSLASH_ESCAPES`** | off | **on** | MariaDB interprets backslash escapes inside string literals: `LENGTH('\\')` is **1** vs DuckDB's 2; `LENGTH('\n')` is 1 vs 2; `LENGTH('\d')` is 1 vs 2 (`b2.bslash.*`). Every regex pattern and every literal containing `\` is corrupted at parse time. After the pin, `z.bslash.after` → 2. Alternative: double every backslash in the literal renderer — but that is a second escaping regime to keep in sync, so pin the mode. |
| **`collation_connection`** | `utf8mb4_general_ci` | **`utf8mb4_nopad_bin`** | Two independent divergences ride on the collation. (a) *Case*: `'abc' = 'ABC'` → **1** (`cfg.strcmpcase`), `INSTR('ABC','bc')` → **2** vs DuckDB 0, `'ABC' REGEXP 'abc'` → **1**, `GREATEST('a','B')` → `'B'` vs DuckDB `'a'`, `MIN` over `{'B','a'}` → `'a'` vs `'B'`. (b) *Trailing space*: `'abc' = 'abc '` → **1**. `utf8mb4_bin` fixes case but **not** padding (`z.pad.bin` → 1 — MariaDB's `_bin` is PAD SPACE). Only **`utf8mb4_nopad_bin`** fixes both (`z.pad.nopad` → 0, `z.strcmp.bin` → 0, `z.greatest.bin` → `'a'`). |
| **`time_zone`** | `SYSTEM` | **`'+00:00'`** | `UNIX_TIMESTAMP(TIMESTAMP '2020-01-15 00:00:00')` → **1579064400** vs DuckDB's 1579046400 — a 5-hour error that varies with the host (`t.ext.epoch.c1`). After `SET time_zone='+00:00'`, `z.epoch.after` → 1579046400. Same for `FROM_UNIXTIME`, `NOW()`. This mirrors H2_BACKEND.md §7's `timeZone='GMT'` pin. |

Also worth pinning / knowing:

- **`div_precision_increment` = 4** truncates `/` results: `1/3` → `0.3333` (BigDecimal), losing 12
  digits vs DuckDB's `0.3333333333333333`. Raising it to 30 restores the digits (`z.div.prec`) but
  the type stays `BigDecimal`. **Do not rely on it** — render `DIVIDE` with a `1e0` multiplier
  instead (§4), which fixes both value and type in one move.
- **`STRICT_TRANS_TABLES`** (default, keep). It **does** reject `'0000-00-00'` on INSERT
  (`e.zerodate.ins` → `Incorrect date value`), but it does **not** affect SELECT-context casts:
  `CAST('0000-00-00' AS DATE)` returns **NULL** silently (`t.zerodate.c1`), and still does with
  `NO_ZERO_DATE` set (`z.zerodate.after`). Zero dates are therefore a **read-path** hazard on
  pre-existing data, not a write-path one.
- **`ERROR_FOR_DIVISION_BY_ZERO`** (default, on) does **not** apply to SELECT: `1/0` → **NULL**
  (`ar.div.zero`, `z.div.after`) where DuckDB gives `Infinity`. `MOD(7,0)` → NULL on both.
- **`IGNORE_SPACE`** is in the default. It makes built-in function names reserved words. Decide
  explicitly; legend-lite's identifier quoting makes it harmless, but leaving `sql_mode` inherited
  rather than assigned is what makes it a surprise.
- **`lower_case_table_names` = 2** on macOS (0 on Linux). Table-name case sensitivity is therefore
  **host-dependent**, not database-dependent — a portability trap for the DDL/fixture layer.
- **`lc_time_names` = `en_US`** — pin it, or `DAYNAME`/`MONTHNAME` change language (§4 temporal).

**Recommended pin, one statement on every connection:**

```sql
SET SESSION sql_mode = 'ANSI_QUOTES,PIPES_AS_CONCAT,NO_BACKSLASH_ESCAPES,STRICT_TRANS_TABLES,'
                       'ERROR_FOR_DIVISION_BY_ZERO,NO_ENGINE_SUBSTITUTION',
    collation_connection = 'utf8mb4_nopad_bin',
    time_zone = '+00:00',
    lc_time_names = 'en_US';
```

This is exactly the `before`-statement capability H2_BACKEND.md §8 argues the dialect seam must
grow. MariaDB is a second, independent motivation for it.

---

## 3. Silent value divergence — exhaustive, with both values

**The defect class that matters.** Each row parses on *both* backends and returns a different
answer. Sorted by blast radius.

| # | Construct | DuckDB | MariaDB 11.4.5 | Probe | Note |
|---|---|---|---|---|---|
| 1 | `SELECT "name" FROM emp` | `alice` | **`name`** (the literal) | `cfg.dq3` | Every quoted identifier. Fixed by `ANSI_QUOTES`. |
| 2 | `'a' \|\| 'b'` | `'ab'` | **`0`** (Integer) | `cfg.pipes1` | `\|\|` is OR. `1 \|\| 0` → `1`. Never errors. |
| 3 | `'abc' = 'ABC'` | `false` | **`1`** | `cfg.strcmpcase` | ci collation. Also `INSTR`, `REGEXP`, `LIKE`, `GREATEST`, `MIN/MAX`, `ORDER BY`, `DISTINCT`, joins. |
| 4 | `'abc' = 'abc '` | `false` | **`1`** | `cfg.strcmppad` | PAD SPACE. Survives `utf8mb4_bin`; needs `_nopad_bin`. |
| 5 | `ROUND(2.5e0)` — DOUBLE | `3.0` (half-up) | **`2.0`** (half-even) | `rd.dbl.25` | MariaDB `ROUND` is **banker's on approximate types, half-up on exact types**: `ROUND(2.5)`→3 but `ROUND(2.5e0)`→2; `ROUND(-2.5e0)`→-2; `ROUND(0.125e0,2)`→0.12 vs `ROUND(0.125,2)`→0.13. Confirmed on a real DOUBLE column: `ROUND(CAST(sal AS DOUBLE))` on sal=100.5 → **100** vs DuckDB's `ROUND` → 101 (`d.re.dblcol`). **Both of legend-lite's rounding SqlFns are wrong unless the argument type is forced** — see §4. |
| 6 | `REGEXP_REPLACE('a1b2','[0-9]','#')` | `'a#b2'` (**first only**) | **`'a#b#'`** (**global**) | `r.repl.all.*` | Same function name, opposite scope. DuckDB needs a `'g'` flag for global; MariaDB has **no flags argument at all** (`regexp_replace` is 3-arg only, `b2.regrepl.g.duck`). |
| 7 | `MAKEDATE(2020, 5)` | `make_date` is (y,m,d) | **`2020-01-05`** — (year, **dayofyear**) | `t.mkdate.collide` | Colliding name, different arity *and* meaning. `make_date(2020,5,15)` → `2020-05-15`; `MAKEDATE(2020,5,15)` errors, but a 2-arg call silently succeeds with a wrong date. |
| 8 | `FORMAT('%s-%d','a',5)` | `printf` → `'a-5'` | **`'0'`** | `s.fmt.c1` | MariaDB `FORMAT(x, d)` is a *numeric* formatter: `FORMAT(1234.5678,2)` → `'1,234.57'`. `SFORMAT('{}-{}','a',5)` → `'a-5'` is the real analogue, with fmtlib `{}` syntax, not printf `%s`. |
| 9 | `LENGTH('héllo')` | `5` (chars) | **`6`** (bytes) | `s.len.multi` | `CHAR_LENGTH` → 5. Silent for ASCII, wrong for everything else. |
| 10 | `DATEDIFF(a, b)` | `date_diff('day', start, end)` | **inverted**: `DATEDIFF(end,start)` | `t.datediff.c1/c2` | `DATEDIFF('2020-01-15','2020-03-01')` → **-46**; `date_diff('day','2020-01-15','2020-03-01')` → 46. `TIMESTAMPDIFF(unit, a, b)` has the **same** order as DuckDB — but see #11. |
| 11 | `TIMESTAMPDIFF` unit counting | boundary crossings | **complete units** | `t.datediff.mon/yr/hr.*` | `month` 2020-01-15→2020-03-01: DuckDB **2**, MariaDB **1**. `year` 2019-12-31→2020-01-01: DuckDB **1**, MariaDB **0**. `hour` 01:59→03:00: DuckDB **2**, MariaDB **1**. `minute` 01:00:30→01:01:00: DuckDB **1**, MariaDB **0**. `day` and `week` happen to agree on the samples probed. **The argument order is right and the answer is still wrong** — the nastiest shape. |
| 12 | `DAYOFWEEK` base | `date_part('dayofweek')` = 0=Sun..6=Sat → **3** for Wed | `DAYOFWEEK()` = 1=Sun..7=Sat → **4**; `WEEKDAY()` = 0=Mon..6=Sun → **2** | `t.ext.dow.*` | Three different bases across two functions on one backend. `WEEKDAY(x)+1` reproduces DuckDB's `isodow`. |
| 13 | `WEEK` numbering | ISO week | **mode 0** (Sunday-start, may be week 0) | `t.ext.week*` | 2020-01-01: DuckDB **1**, `WEEK(d)` **0**. 2021-01-03: DuckDB **53**, `WEEK(d)` **1**. `WEEK(d, 3)` (== `WEEKOFYEAR`) reproduces ISO: 1 and 53. MariaDB has 8 modes; **only mode 3 is ISO**. |
| 14 | `1/3` | `Double 0.3333333333333333` | **`BigDecimal 0.3333`** | `ar.div.type` | `div_precision_increment=4`. `(1.0 * a)/b` — legend-lite's exact DuckDB rendering — stays DECIMAL: `ar.div.duck` → `3.50000`. `(1e0 * a)/b` gives `Double 3.5` exactly (`e.div.castdbl`). |
| 15 | `-8 >> 1` | `-4` | **`9223372036854775804`** | `bt.negshr`, `f.shr.neg.c1` | **All bitwise ops evaluate in UNSIGNED BIGINT.** `~5` → `18446744073709551610` (DuckDB `-6`); `-1 << 1` → `18446744073709551614` (DuckDB *errors*); `-8 ^ 12` → `18446744073709551604`. `CAST(… AS SIGNED)` rescues `&`, `\|`, `^`, `~`, `<<` — but **not** `>>`, which is a logical shift. |
| 16 | `GREATEST(1,NULL,3)` | `3` (skips NULL) | **`NULL`** | `nl.greatest.null` | Same for `LEAST`. |
| 17 | `CONCAT('a',NULL,'b')` | `'ab'` (skips NULL) | **`NULL`** | `a2.concat.null` | legend-lite's `AnsiSqlRenderer` comments explicitly rely on the null-skipping shape for LEFT-JOIN misses. `CONCAT_WS('', …)` restores it. |
| 18 | `PI()` | `3.141592653589793` | **`3.141593`** | `mt.pi` | Truncated to 7 significant digits *at the JDBC boundary*. `PI()+0e0`, `PI()*1.0e0` and `ACOS(-1)` all return full precision. `RADIANS(180)` is already full precision. |
| 19 | `ASCII('é')` | `233` (codepoint) | **`195`** (first UTF-8 byte) | `s.ascii.multi` | `ORD('é')` → **50089** (the byte sequence read as a number) — also not a codepoint. |
| 20 | `chr(233)` | `'é'` | `CHAR(233 USING utf8mb4)` → **NULL**; bare `CHAR(233)` → `byte[]` | `s.chr.*` | |
| 21 | `split_part('a,b,c',',',9)` | `''` | `SUBSTRING_INDEX` idiom → **`'c'`** | `s.splitpart.oob.*` | Out-of-range index clamps to the last field instead of returning empty. Needs an explicit field-count guard. |
| 22 | `LEFT('abcdef',-1)` | `'abcde'` (drop from end) | **`''`** | `s.left.neg` | |
| 23 | `SUBSTRING('abcdef',0,2)` | `'a'` | **`''`** | `s.substr.zero` | |
| 24 | `DATE '2020-01-15' + 1` | `2020-01-16` | **`20200116`** (Long) | `t.datemath` | Date+integer is numeric addition on the `YYYYMMDD` representation. Only `+ INTERVAL 1 DAY` is a date add. |
| 25 | `date_part('millisecond', ts)` | `1234` (seconds×1000 + ms) | `MICROSECOND(ts) DIV 1000` → **`234`** | `t.ext.ms.*` | DuckDB's `millisecond`/`microsecond` parts **include the seconds**. Construction: `SECOND(x)*1000 + MICROSECOND(x) DIV 1000` → 1234 ✓. |
| 26 | `LN(0)` / `LN(-1)` / `SQRT(-1)` | **error** | **NULL** | `mt.ln.*` | Domain errors become NULLs. Also `CAST('abc' AS SIGNED)` → **0** and `CAST('12abc' AS SIGNED)` → **12** where DuckDB raises (`nl.parseint.bad`, `f.parseint.strict`) — `PARSE_INT`'s failure contract is inverted. `STR_TO_DATE('zzz', …)` → NULL vs DuckDB's parse error. |
| 27 | `'ABC' REGEXP 'abc'` | `false` | **`1`** | `r.matches.case` | Regex case sensitivity is **collation-driven**, not pattern-driven. `(?i)` also works, so a pattern can be case-insensitive two different ways. |
| 28 | regex replacement backrefs | `'\2\1'` → `'ba'` | `'\2\1'` → **`'21'`**; `'\\2\\1'` → `'ba'` | `r.repl.backref.*` | Interacts with #— `NO_BACKSLASH_ESCAPES` (§2). `'$2$1'` works on neither. |
| 29 | `UNIX_TIMESTAMP` / `FROM_UNIXTIME` | UTC epoch | **local-time epoch** | `t.ext.epoch.c1` | 1579064400 vs 1579046400 under `time_zone=SYSTEM`. Host-dependent. |
| 30 | `x + INTERVAL 2 YEAR` on a DATE | returns `TIMESTAMP` | returns **`DATE`** | `t.addint.c1` | Type, not value — but it changes downstream `EXTRACT(HOUR …)` and JDBC read-back. |
| 31 | `CONVERT_TZ(ts,'UTC','America/New_York')` | `timezone()` works | **NULL** | `t.tz.c1` | `mysql.time_zone_name` has **0 rows** — the tz tables are not populated by `mariadb-install-db`. Numeric offsets work (`t.tz.c2`). |
| 32 | `12 ^ 10` | `6.19e10` (`^` is **power**) | **`6`** (`^` is **bitwise XOR**) | `bt.xor.c1` | The trap runs in the other direction too: a `^` written for MariaDB means exponentiation on DuckDB. |
| 33 | `DATE_FORMAT` vs `strftime` masks | `%M`=minute, `%S`=second, `%B`=month name | **`%M`=month name**, `%S`=second, `%b`=abbrev month | `d.strf.M.*`, `d.strf.B.*` | `%Y %m %d %H %j %p %%` agree; `%M` silently produces `'May'` where `'45'` was meant. |

**Not divergent (verified, worth recording):** `MOD`/`REM` sign behaviour matches exactly for
`MOD(-7,3)`=-1, `MOD(7,-3)`=1, `-7 % 3`=-1, `MOD(-7.5,3)`=-1.5 and legend-lite's always-positive
`MOD(MOD(a,b)+b,b)` composite → 2 on both. `MD5`/`SHA1` byte-identical. `DAYNAME`/`MONTHNAME`
identical under `lc_time_names=en_US`. `IN` with NULL members → NULL on both. `LPAD`/`RPAD`
including the over-length truncation case. `//` is a **syntax error** on MariaDB, not a comment —
so H2's `H5.2` silent-`INT_DIVIDE` bug does **not** reproduce here; it fails loudly.

---

## 4. Per-`SqlFn` bucket and construction

### 4.1 Strings / regex / encoding (41)

| SqlFn | DuckDB spelling | Bkt | MariaDB spelling / construction |
|---|---|---|---|
| CONCAT | `concat(…)` | **B** | `CONCAT_WS('', …)` — `CONCAT` NULL-propagates (§3 #17) |
| LENGTH | `length` | **C** | `CHAR_LENGTH` (§3 #9) |
| UPPER | `upper` | A | `UPPER` |
| LOWER | `lower` | A | `LOWER` |
| SUBSTRING | `substr` | A | `SUBSTRING` (start=0 edge, §3 #23) |
| STRPOS | `strpos(hay,ndl)` | **C** | `LOCATE(ndl,hay)` (**args inverted**) or `INSTR(hay,ndl)` (same order). Collation-sensitive |
| STARTS_WITH | `starts_with` | **B** | `LEFT(s,CHAR_LENGTH(p)) = p` |
| ENDS_WITH | `ends_with` | **B** | `RIGHT(s,CHAR_LENGTH(p)) = p` |
| MATCHES | `regexp_matches` | **C** | `s REGEXP p` (operator) or `REGEXP_INSTR(s,p) > 0` |
| LEFT | `left` | A | `LEFT` (negative-n edge, §3 #22) |
| RIGHT | `right` | A | `RIGHT` |
| LPAD | `lpad` | A | `LPAD` |
| RPAD | `rpad` | A | `RPAD` |
| TRIM | `trim(s[,c])` | **C** | 1-arg `TRIM(s)`; 2-arg → `TRIM(BOTH c FROM s)` |
| LTRIM | `ltrim(s[,c])` | **C** | 1-arg `LTRIM(s)`; 2-arg → `TRIM(LEADING c FROM s)` (2-arg `ltrim` errors) |
| RTRIM | `rtrim(s[,c])` | **C** | 1-arg `RTRIM(s)`; 2-arg → `TRIM(TRAILING c FROM s)` |
| REPLACE | `replace` | A | `REPLACE` |
| SPLIT | `string_split` | **D** | see §5 |
| SPLIT_PART | `split_part` | **B** | `SUBSTRING_INDEX(SUBSTRING_INDEX(s,d,n),d,-1)` + field-count guard (§3 #21) |
| REVERSE_STRING | `reverse` | A | `REVERSE` |
| ASCII_CODE | `ascii` | **B** | `CAST(CONV(HEX(CONVERT(s USING utf32)),16,10) AS SIGNED)` — `ASCII`/`ORD` are both wrong (§3 #19) |
| CHR | `chr` | **B** | `CONVERT(UNHEX(LPAD(CONV(n,10,16),8,'0')) USING utf32)` |
| UC_FIRST | `upper(substr(s,1,1)) \|\| substr(s,2)` | **B** | `CONCAT(UPPER(SUBSTRING(s,1,1)), SUBSTRING(s,2))` — the `\|\|` form returns **0** |
| LC_FIRST | as above w/ `lower` | **B** | `CONCAT(LOWER(SUBSTRING(s,1,1)), SUBSTRING(s,2))` |
| ENCODE_BASE64 | `to_base64(CAST(x AS BLOB))` | **C** | `TO_BASE64(x)` — the `BLOB` cast is a syntax error |
| DECODE_BASE64 | `CAST(from_base64(x) AS VARCHAR)` | **C** | `CAST(FROM_BASE64(x) AS CHAR)` — bare `FROM_BASE64` returns `byte[]` |
| LEVENSHTEIN | `levenshtein` | **D** | see §5 |
| JARO_WINKLER | `jaro_winkler_similarity` | **D** | see §5 |
| GUID | `uuid()` | A | `UUID()` — **v1** (time+MAC) not v4; both read back as `java.util.UUID` |
| FORMAT | `printf('%s',…)` | **B** | `SFORMAT('{}',…)` + printf→fmtlib mask translation (§3 #8) |
| HASH | `hash(x)` | **B** | `CAST(CONV(SUBSTRING(MD5(x),1,16),16,10) AS SIGNED)` — a *different* 64-bit value; portable only if `HASH` is required to be stable per-backend, not across |
| MD5 | `md5` | A | `MD5` |
| SHA1 | `sha1` | A | `SHA1` |
| SHA256 | `sha256` | **C** | `SHA2(x, 256)` |
| REPEAT_STR | `repeat` | A | `REPEAT` |
| REGEXP_EXTRACT | `regexp_extract(s,p[,g])` | **B** | no-group → `REGEXP_SUBSTR(s,p)` (C). group *n* → `REGEXP_REPLACE(s, CONCAT('^.*?',p,'.*$'), '\\n')` — MariaDB's `REGEXP_SUBSTR` is **2-arg only** (`f.regexpsubstr.args`) |
| REGEXP_EXTRACT_ALL | `regexp_extract_all` | **D** | see §5 |
| REGEXP_REPLACE | `regexp_replace` | **B** | same name — **global vs first-only** (§3 #6) + backref doubling (§3 #28). First-only needs a `REGEXP_INSTR`-split composite (verified: `b2.regrepl.first.mdb` → `'a#b2'`) |
| REGEXP_FULL_MATCH | `regexp_full_match` | **B** | `s REGEXP CONCAT('^(?:',p,')$')` — verified true/false on `'abc'`/`'abcd'` vs `'a.c'` |
| GREATEST | `greatest` | **B** | `GREATEST(COALESCE(a,SENTINEL),…)` — NULL propagates (§3 #16) |
| LEAST | `least` | **B** | as above |

**Regex engine.** MariaDB 10.0.5+ uses **PCRE2**; DuckDB uses **RE2**. MariaDB is a strict superset
of the syntax: lookahead `foo(?=bar)` and in-pattern backreferences `(ab)\1` **work on MariaDB and
error on DuckDB** (`r.lookahead.*`, `r.backrefpat.*`); POSIX classes work on both. Patterns
authored against DuckDB will therefore always compile on MariaDB — the risk is the reverse, plus
the case-sensitivity difference (§3 #27). `REGEXP_INSTR` exists (2-arg only); `REGEXP_SUBSTR_ALL`
does not exist at all.

### 4.2 Math / bitwise / arithmetic / null (51)

| SqlFn | DuckDB | Bkt | MariaDB |
|---|---|---|---|
| PLUS · MINUS · TIMES · NEGATE · ABS | infix / `abs` | A | identical |
| DIVIDE | `((1.0 * a) / b)` | **C** | `((1e0 * a) / b)` — `1.0` yields a truncated DECIMAL (§3 #14) |
| MOD | `MOD(MOD(a,b)+b, b)` | A | identical, value-identical |
| REM | `MOD(a,b)` | A | identical |
| IS_NULL · IS_NOT_NULL · IN · COALESCE | | A | identical |
| PARSE_INT | `CAST(x AS BIGINT)` | **C** | `CAST(x AS SIGNED)` — `BIGINT` is not a cast target. Bad input → **0/partial**, not an error (§3 #26) |
| SQRT · EXP · LN · LOG10 · POW · SIN · COS · TAN · ASIN · ACOS · ATAN · ATAN2 · COT · RADIANS · DEGREES | | A | identical (`ATAN(y,x)` is also accepted). Domain errors → NULL (§3 #26) |
| CBRT | `cbrt` | **B** | `SIGN(x)*POWER(ABS(x), 1e0/3e0)` — `POWER(-8, 1/3)` **errors** on MariaDB (DuckDB: NaN) |
| PI | `pi()` | **C** | `PI() + 0e0` (§3 #18) |
| SINH · COSH · TANH | `sinh`/`cosh`/`tanh` | **B** | `(EXP(x)-EXP(-x))/2`, `(EXP(x)+EXP(-x))/2`, `(EXP(2*x)-1)/(EXP(2*x)+1)` — all bit-exact vs DuckDB |
| CEILING | `CAST(ceil(x) AS BIGINT)` | **C** | `CAST(CEIL(x) AS SIGNED)` |
| FLOOR | `CAST(floor(x) AS BIGINT)` | **C** | `CAST(FLOOR(x) AS SIGNED)` |
| FLOOR_RAW | `floor(x)` | A | `FLOOR(x)` (type differs — §7) |
| **ROUND** (banker's) | `ROUND_EVEN(x,n)` | **C** | **`ROUND(CAST(x AS DOUBLE), n)`** — verified against `ROUND_EVEN` at 0.5/1.5/2.5/3.5/−0.5/−2.5 and at scale 2 on 0.125/0.135, and on a real column |
| **ROUND_HALF_UP** | `ROUND(x,n)` | **C** | **`ROUND(CAST(x AS DECIMAL(38,10)), n)`** — verified 2.5→3, 0.125→0.13 |
| SIGN | `CAST(sign(x) AS BIGINT)` | **C** | `CAST(SIGN(x) AS SIGNED)` |
| XOR (logical) | `(x AND NOT y) OR (NOT x AND y)` | A | the composite works verbatim; native `x XOR y` also available (C alternative) |
| BIT_AND / BIT_OR / BIT_XOR / BIT_SHIFT_LEFT | `&` `\|` `xor()` `<<` | **B** | `CAST((a & b) AS SIGNED)`, `CAST((a \| b) AS SIGNED)`, `CAST((a ^ b) AS SIGNED)`, `CAST((a << b) AS SIGNED)` — **the cast is mandatory** (§3 #15). Note `BIT_AND`/`BIT_OR`/`BIT_XOR` are **aggregate** function names in MariaDB; a 2-arg call is a syntax error (`e.bitagg.2arg`) |
| BIT_SHIFT_RIGHT | `>>` | **B** | `CAST(FLOOR(a / POWER(2,b)) AS SIGNED)` — `>>` is a **logical** shift; `CAST((-8 >> 1) AS SIGNED)` still returns 9223372036854775804. Verified: construction → −4 ✓, and 16 for `256>>4` ✓ |
| BIT_NOT | `xor(x,-1)` | **C** | `CAST(~x AS SIGNED)` → −6 ✓ |
| INT_DIVIDE | `(a // b)` | **C** | `a DIV b` — `//` is a **syntax error** (loud). Truncates toward zero on both (−7→−3). `DIV 0` → NULL |
| IS_DISTINCT | `a IS DISTINCT FROM b` | **C** | `NOT (a <=> b)` — exact on all three NULL combinations |
| ERROR | `error(msg)` | **B** | `ExtractValue('<a/>', CONCAT('~', msg))` → raises `XPATH syntax error: '~<msg>'`. The message carries, prefixed with `~`. `SIGNAL SQLSTATE '45000' SET MESSAGE_TEXT=…` works but is a **statement**, not an expression |
| CURRENT_USER_FN | `current_user` | A | `CURRENT_USER` / `CURRENT_USER()` — value is `user@host`, not a bare name |
| TYPEOF | `typeof(x)` | **D** | see §5 |

### 4.3 Temporal (19)

| SqlFn | DuckDB | Bkt | MariaDB |
|---|---|---|---|
| TODAY | `current_date` | A | `CURRENT_DATE` / `CURDATE()` |
| NOW | `now()` | A | `NOW()` — **second** precision; `NOW(6)` for microseconds |
| DATE_TRUNC_DAY | `CAST(x AS DATE)` | A | identical; `DATE(x)` equivalent |
| EXTRACT | `date_part('u', x)` | **B** | `EXTRACT(<U> FROM x)` for year/month/day/hour/minute/second/quarter/dayofyear (C-level). Constructions needed for: `dayofweek` → `DAYOFWEEK(x)-1`; `isodow` → `WEEKDAY(x)+1`; `week` → **`WEEK(x, 3)`**; `isoyear` → `YEARWEEK(x,3) DIV 100`; `epoch` → §below; `millisecond` → `SECOND(x)*1000 + MICROSECOND(x) DIV 1000`; `microsecond` → `SECOND(x)*1000000 + MICROSECOND(x)` |
| DATE_TRUNC | `date_trunc('u', x)` | **B** | no `DATE_TRUNC` at all. `TIMESTAMP(DATE_FORMAT(x, mask))` with mask per unit — verified for year `'%Y-01-01 00:00:00'`, month `'%Y-%m-01 00:00:00'`, hour `'%Y-%m-%d %H:00:00'`, minute `'…%H:%i:00'`, second `'…%H:%i:%s'`, millisecond `'…%H:%i:%s.%f'`; week → `TIMESTAMP(DATE_FORMAT(x,'%Y-%m-%d 00:00:00')) - INTERVAL WEEKDAY(x) DAY` (Monday-aligned, matches DuckDB); quarter → year-truncated `+ INTERVAL (QUARTER(x)-1) QUARTER`; decade → `TIMESTAMP(CONCAT(YEAR(x) DIV 10*10,'-01-01 00:00:00'))`. **All 9 units reproduced value-exactly.** |
| MAKE_DATE | `make_date(y,m,d)` | **B** | `DATE(CONCAT_WS('-',y,m,d))` or `STR_TO_DATE(CONCAT_WS('-',y,m,d),'%Y-%m-%d')`. **Never `MAKEDATE`** (§3 #7) |
| MAKE_TIMESTAMP | `make_timestamp(…)` | **B** | `STR_TO_DATE(CONCAT_WS(' ',CONCAT_WS('-',y,m,d),CONCAT_WS(':',h,mi,s)),'%Y-%m-%d %H:%i:%s')` |
| ADD_INTERVAL | `x + to_years(n)` | **C** | `x + INTERVAL n YEAR` / `DATE_ADD(x, INTERVAL n <U>)` — accepts an **expression** amount and negatives. Returns DATE for a DATE input (§3 #30) |
| DATE_DIFF | `date_diff('u',a,b)` | **B** | arg order matches `TIMESTAMPDIFF(<U>,a,b)` but the **counting rule differs** (§3 #11). Correct form: truncate both endpoints to the unit (as DATE_TRUNC above) then `TIMESTAMPDIFF` — verified `hour` 01:59→03:00 → 2 ✓. `month`/`year` are cheaper closed forms: `(Y(b)*12+M(b))-(Y(a)*12+M(a))` → 2 ✓ and `YEAR(b)-YEAR(a)` → 1 ✓. `day` → `DATEDIFF(b,a)` (note the swap) → 46 ✓. `week` matches `TIMESTAMPDIFF(WEEK,…)` natively |
| TIME_BUCKET | `time_bucket(…)` | **B** | `TIMESTAMP '1970-01-01 00:00:00' + INTERVAL (TIMESTAMPDIFF(SECOND,TIMESTAMP '1970-01-01 00:00:00',x) DIV width)*width SECOND` → `2020-05-14 00:00:00`, exactly DuckDB. Week buckets take the `1969-12-29` origin the same way |
| TIMEZONE | `timezone(zone, ts)` | **D** | see §5 |
| EPOCH_SECONDS | `epoch(ts)` | **B** | **`TIMESTAMPDIFF(SECOND, TIMESTAMP '1970-01-01 00:00:00', ts)`** — timezone-independent, matches DuckDB at any `time_zone`. `UNIX_TIMESTAMP` is tz-dependent (§3 #29) |
| EPOCH_MS | `epoch_ms(ts)` | **B** | `TIMESTAMPDIFF(MICROSECOND, TIMESTAMP '1970-01-01 00:00:00', ts) DIV 1000` → 1579046400123 ✓ |
| FROM_EPOCH_SECONDS | `to_timestamp(n)` | **B** | `TIMESTAMP '1970-01-01 00:00:00' + INTERVAL n SECOND` ✓ |
| FROM_EPOCH_MS | `epoch_ms(CAST(n AS BIGINT))` | **B** | `TIMESTAMP '1970-01-01 00:00:00' + INTERVAL n*1000 MICROSECOND` ✓ |
| DAYNAME | `dayname` | A | `DAYNAME` — pin `lc_time_names` |
| MONTHNAME | `monthname` | A | `MONTHNAME` — pin `lc_time_names` |
| STRFTIME | `strftime(x,fmt)` | **B** | `DATE_FORMAT(x, fmt')` with a **translated mask** (§3 #33) |
| STRPTIME | `strptime(s,fmt)` | **C** | `STR_TO_DATE(s, fmt')` — same mask-translation caveat; returns `DATE` for a date-only mask (wrap in `CAST(… AS DATETIME)`); bad input → NULL, not an error |

---

## 5. The six D's — what was tried

| SqlFn | Candidates attempted | Why D |
|---|---|---|
| **SPLIT** | `string_split`, `SPLIT`, `JSON_ARRAY(SUBSTRING_INDEX(…))`, `CONCAT('["',REPLACE(s,d,'","'),'"]')` | MariaDB has **no ARRAY type**. The last candidate returns the right *text* (`["a","b","c"]`, byte-identical to DuckDB's JSON rendering) but as a `String`, not an `Array` — a carrier change, not a `SPLIT`. Resolvable only by the collection-carrier redesign (H2_BACKEND.md §4.1); `JSON_TABLE` gives rows, not a value. |
| **REGEXP_EXTRACT_ALL** | `regexp_extract_all`, `REGEXP_SUBSTR_ALL`, `REGEXP_REPLACE(s,'[^0-9]+',',')` | Same no-ARRAY wall; `REGEXP_SUBSTR_ALL` does not exist; the replace idiom degenerates for general patterns. |
| **LEVENSHTEIN** | `levenshtein`, `LEVENSHTEIN`, `EDIT_DISTANCE`, `SOUNDEX` (different function), scalar-subquery recursive CTE | No built-in on any MariaDB. A recursive CTE **does** work inside a scalar subquery (`e.cte.inscalar` → 5) but **cannot be row-correlated**: `(WITH RECURSIVE … WHERE i < CHAR_LENGTH(e.name))` → `Unknown column 'e.name' in 'WHERE'` (`e.lev.corr`). `JSON_TABLE` accepts a correlated argument but offers no recursion, so the DP matrix is unreachable. |
| **JARO_WINKLER** | `jaro_winkler_similarity`, `JARO_WINKLER_SIMILARITY`, `JARO_WINKLER` | Absent; same recursion wall. |
| **TYPEOF** | `typeof`, `COLUMN_TYPE`, `JSON_TYPE(CAST(x AS JSON))`, `JSON_TYPE(JSON_EXTRACT(JSON_ARRAY(x),'$[0]'))`, `information_schema.columns` | No scalar type-of expression. `JSON_TYPE` works only on JSON values and uses a **different vocabulary** — `'INTEGER'`/`'STRING'`/`'ARRAY'` vs DuckDB `'INTEGER'`/`'VARCHAR'`. `information_schema` returns lowercase SQL type names but is a *table*, not an expression, and only for real columns. **Downgradeable to C exactly as H2_BACKEND.md §4's D6 argues**: the column type is statically known from the catalog, so the runtime dispatch in `Fold.jsonDateWrap` can be resolved at lowering. |
| **TIMEZONE** | `timezone(z,ts)`, `CONVERT_TZ(ts,'UTC',z)`, `CONVERT_TZ(ts,'SYSTEM','+00:00')`, `CONVERT_TZ(ts,'+00:00',z)` | `CONVERT_TZ` exists and works for **numeric offsets** (`'+00:00'`→`'-04:00'` ✓, `'SYSTEM'`→`'+00:00'` ✓) but returns **NULL for every named zone** because `mysql.time_zone_name` is **empty** (`t.tz.count` → 0). `mariadb-install-db` does not populate the tz tables; `mysql_tzinfo_to_sql` is a separate operational step. **D as-shipped, C on a provisioned server** — this is a deployment prerequisite legend-lite must declare, not a code fix. |

---

## 6. MariaDB vs MySQL — where the bucket would change

Both are plausible targets, so every construction above should be read with these deltas.

| | MariaDB 11.4 (measured) | MySQL 8 | Impact |
|---|---|---|---|
| **`ONLY_FULL_GROUP_BY`** | **not** in the default sql_mode (measured: `a2.onlyfullgb` → 0) | **is** in the default | A query legal on MariaDB can fail on MySQL. Pin sql_mode explicitly — do not inherit. |
| `REGEXP_SUBSTR` / `REGEXP_INSTR` arity | **2-arg only** (`f.regexpsubstr.args`, `f.regexpinstr.args`) | 5- and 6-arg forms with `pos`, `occurrence`, `match_type`, `return_option` | `REGEXP_EXTRACT` with a capture group is **C on MySQL, B on MariaDB**. |
| `REGEXP_REPLACE` flags | 3-arg only, always global | 6-arg with `match_type` | The first-vs-global fix (§3 #6) is cheaper on MySQL. |
| `SFORMAT` | **present** (fmtlib `{}`) | **absent** | `FORMAT` is B on MariaDB and **D-or-worse on MySQL** (no printf analogue at all). |
| `JSON_TABLE` | **present since 10.6** — verified working here, including with a **row-correlated** table-column argument | present since 8.0.4 | Contrary to the common claim, this is **not** a divergence on modern MariaDB. Both backends have correlated row explosion; both lack it below 10.6 / 8.0.4. |
| `utf8mb4_bin` padding | **PAD SPACE** (`z.pad.bin` → `'abc'='abc '` is 1) | NO PAD | The `_nopad_bin` pin (§2) is a MariaDB-specific necessity. |
| `_nopad_` collations | present (`utf8mb4_nopad_bin` verified) | not available under that name | The collation pin needs a per-flavour value. |
| `SEQUENCE` / `RETURNING` / `EXCEPT ALL` | MariaDB-only | — | Outside this slice, but a same-dialect assumption will break. |
| Default collation | `utf8mb4_general_ci` | `utf8mb4_0900_ai_ci` | Different ci rules; the pin makes it moot. |

---

## 7. Returned-Java-type divergence — candidate `SqlDialect.normalize` rows

`SqlDialect.normalize` has zero overrides today (H2_BACKEND.md §8). These are MariaDB's rows, each
observed in `out-mariadb-scalar.tsv` against DuckDB's cell for the same probe.

| Expression class | DuckDB | MariaDB | Probes |
|---|---|---|---|
| **Boolean-valued expressions** (`=`, `<`, `IS NULL`, `IN`, `TRUE`, `AND`/`OR`/`XOR`) | `Boolean` | **`Integer`** (1/0) | `cfg.bool1/2`, `nl.isnull`, `nl.in`, `bt.xor.log.c1` — pervasive; the single highest-frequency row |
| **Bitwise results** (`&`, `\|`, `^`, `<<`, `>>`, `~`) | `Integer` | **`BigInteger`** (UNSIGNED BIGINT) | `bt.and`, `bt.or`, `bt.shl`, `bt.not.c1` — `CAST(… AS SIGNED)` also fixes the *type* to `Long` |
| **Aggregate bitwise** (`BIT_AND(col)`) | `Integer` | **`BigInteger`** | `e.bitagg.and` |
| **`/` division** | `Double` | **`BigDecimal`** (scale = `div_precision_increment`) | `ar.div.plain`, `ar.div.duck` — fixed by the `1e0` rendering |
| **`LENGTH`/`CHAR_LENGTH`/`STRPOS`/`INSTR`** | `Long` | **`Integer`** | `s.len.duck`, `s.strpos.c2` |
| **`EXTRACT`/`YEAR`/`MONTH`/`DAYOFWEEK`/`WEEK`** | `Long` | **`Integer`** | `t.ext.year.c1` and every sibling |
| **`SIGN`** | `Byte` | **`Integer`** | `mt.sign` |
| **`FLOOR`/`CEIL` on DECIMAL** (`FLOOR_RAW`) | `BigDecimal` | **`Integer`** | `mt.floorraw`, `mt.ceiling` |
| **`DATE` values** | `LocalDate` | **`java.sql.Date`** | `t.datelit`, `t.hiredcol`, `t.today.c1` |
| **`now()` / `CURRENT_TIMESTAMP`** | `OffsetDateTime` | **`java.sql.Timestamp`** (no offset; second precision unless `NOW(6)`) | `t.now.duck/c1` |
| **`to_timestamp` / epoch→ts** | `OffsetDateTime` | **`Timestamp`** | `t.fromepochs.*` |
| **`date + INTERVAL n YEAR`** | `Timestamp` | **`Date`** (input type preserved) | `t.addint.c1` |
| **`STR_TO_DATE` with a date-only mask** | `strptime` → `Timestamp` | **`Date`** | `t.strptime.c1` |
| **`FROM_BASE64`** | `DuckDBBlobResult` | **`byte[]`** | `s.b64dec.c1` — H2's JSON→`byte[]` precedent |
| **`CHAR(n)`** (no `USING`) | `String` | **`byte[]`** | `s.chr.c1` |
| **`date_part('epoch')`** | `Double` | `TIMESTAMPDIFF` → **`Long`** | `t.ext.epoch.duck` vs `d.epoch.tzfree` |
| **`CONV(...)`** (hash/codepoint idioms) | — | **`String`** (or `Double` if arithmetic is applied) — needs `CAST(… AS SIGNED)` → `Long` | `rt.hash.c2`, `d.ascii.final`, `f.hash.signed` |
| **`SUM` over a recursive CTE** | `BigInteger`/`Long` | **`BigDecimal`** | reported by the coordinator; consistent with MariaDB's DECIMAL-widening of integer aggregates |
| **zero date** | error | **`<null>`** via the connector | `t.zerodate.*` — the connector's `zeroDateTimeBehavior` decides; pin it rather than inherit |

---

## 8. Recommended ordering for a MariaDB dialect

1. **Pin the session** (§2) — one `SET SESSION` on every connection, through the `before`-statement
   seam H2_BACKEND.md §8 proposes. Nothing else is trustworthy until `ANSI_QUOTES`,
   `PIPES_AS_CONCAT`, `NO_BACKSLASH_ESCAPES`, `utf8mb4_nopad_bin` and `time_zone='+00:00'` are set;
   four of the top five silent divergences vanish with it.
2. **Author `TypeNames.MARIADB` before `Spellings.MARIADB`.** `CAST(x AS BIGINT)` is a *syntax
   error* — it breaks `CEILING`, `FLOOR`, `SIGN`, `PARSE_INT` and the bitwise family before any
   spelling row is consulted. `SIGNED` / `DECIMAL` / `CHAR` / `DATETIME` are the targets.
3. **The two rounding rows.** `ROUND` → `ROUND(CAST(x AS DOUBLE), n)`; `ROUND_HALF_UP` →
   `ROUND(CAST(x AS DECIMAL(38,10)), n)`. Both are one-line `Spellings`-adjacent rules and both are
   silent wrong answers if omitted — and they fail in *opposite* directions, so a single "MariaDB
   ROUND is X" belief cannot be right.
4. **The bitwise `CAST(… AS SIGNED)` wrapper**, plus the `>>` arithmetic-shift construction.
5. **The temporal rewrite pass** — `DATE_TRUNC`, `DATE_DIFF`, the epoch family and `TIME_BUCKET` all
   route through two primitives (`TIMESTAMP(DATE_FORMAT(x,mask))` and
   `TIMESTAMPDIFF(u, TIMESTAMP '1970-01-01 00:00:00', x)`); build those two first and 11 of the 19
   temporal SqlFns fall out.
6. **Register the six D's** in the declared-gap registry (H2_BACKEND.md §9), with `TIMEZONE` marked
   as a *deployment* gap (load the tz tables) rather than a capability gap, and `TYPEOF` marked as
   downgradeable to C via static catalog types.
