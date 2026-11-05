# Backend capability fragment — STRING / REGEX / HASHING / ENCODING (41 `SqlFn` constants)

**Targets: SQLite 3.47.1 (`org.xerial:sqlite-jdbc` 3.47.1.0) and PostgreSQL 18.4 (zonky embedded).
Reference: DuckDB 1.5.0.0.**

**Evidence standard.** Every row below was produced by **executing 542 probe statements** against real
jars/servers via the shared harness (`probe/Probe.java`) — 364 in the main sweep plus 178 across five
iteration rounds. Where a first-choice spelling failed, alternate spellings and pure-SQL constructions
were authored and re-run until either a construction returned DuckDB's value or the option space was
exhausted. Documentation was used only to generate candidates; **execution decided every bucket**.

Raw outputs: `probe/out-{duckdb,sqlite,postgres}-strings.tsv` plus `-cen2`, `-ext`, `-r3`…`-r6`.

---

## 0. The finding that changes the SQLite column: the driver ships extension-functions

`org.xerial:sqlite-jdbc:3.47.1.0` does **not** register a stock SQLite function set. A full census
(`SELECT name FROM pragma_function_list`, paged so nothing was lost to truncation) returned
**170 distinct functions**, of which **21 are not in any stock SQLite build**:

```
atn2  charindex  cot  coth  difference  leftstr  lower_quartile  median  mode
padc  padl  padr  proper  replicate  reverse  rightstr  square  stdev
strfilter  upper_quartile  variance
```

That is exactly the function list of **`extension-functions.c`** (Liam Healy's contributed SQLite
extension), compiled into the driver's bundled native library. `sqlite_compileoption_get()` confirms
a custom build (`JDBC_EXTENSIONS`, …). This is **driver-specific behaviour**, not SQLite behaviour:
swapping to another SQLite JDBC driver, or to the `sqlite3` CLI, removes all 21.

In this slice it directly supplies `reverse` (bucket A for `REVERSE_STRING`) and supplies useful but
**semantically non-equivalent** candidates for `LEFT`/`RIGHT`/`LPAD`/`RPAD`/`REPEAT_STR`/`STRPOS`.
Measured deltas — each one a trap:

| Driver extra | Looks like | Actually |
|---|---|---|
| `padl(s,n)` / `padr(s,n)` | `lpad`/`rpad` | **2-arg only, pads with SPACES only, and never truncates.** `padl('abcdef',3)`→`'abcdef'`; DuckDB `lpad('abcdef',3,'*')`→`'abc'` |
| `leftstr(s,n)` | `left` | matches for `n>=0`; `leftstr('abcdef',-2)`→`''`, DuckDB `left(…,-2)`→`'abcd'` |
| `rightstr(s,n)` | `right` | matches for `n>=0`; `rightstr('abcdef',-2)` **fails `SQLITE_NOMEM`**, DuckDB→`'cdef'` |
| `replicate(s,n)` | `repeat` | matches for `n>=0`; `replicate('ab',-1)` **errors "domain error"**, DuckDB `repeat('ab',-1)`→`''` |
| `charindex(needle,hay)` | `strpos` | **argument order reversed**, and `charindex('','abcdef')`→`0` where DuckDB `strpos('abcdef','')`→`1` |
| `proper(s)` | `ucFirst` | title-cases **every** word and is ASCII-only |
| `difference(a,b)` | `levenshtein` | soundex distance 0–4, unrelated metric |

**Recommendation: do not build the SQLite dialect on these.** `reverse` is the only one whose
semantics match DuckDB exactly. The rest are traps that pass a smoke test and diverge at an edge.

Equally load-bearing, the census proves **absence**: this build has **no** `regexp`, `regexp_*`, `md5`,
`sha1`, `sha256`, `sha3`, `base64`, `uuid`, `levenshtein`, `editdist3`, `split_part`, `ascii`, `chr`,
`lpad`, `rpad`, `left`, `right`, `repeat`, `starts_with`, `ends_with`, `initcap`, `greatest`, `least`,
`hash` (probe `X.rx4` — a `COUNT(*)` over that exact name list returned **0**). `load_extension()` is
present but fails, so the loadable-extension escape hatch is closed too.

**One genuine capability, and it carries the whole B column:** SQLite supports **correlated recursive
CTEs inside a scalar subquery** — `(WITH RECURSIVE r(…) AS (… substr(e.name,i,1) …) SELECT …) FROM emp e`
executes and returns per-row answers. That is the primitive H2 lacks (`H2_BACKEND.md` D1). It is what
makes base64 and Levenshtein constructible rather than impossible.

---

## 1. The capability map

Buckets per `docs/H2_BACKEND.md` §2: **A** native (DuckDB's own spelling, same value) · **C** rendering
override (different name/syntax) · **B** rewrite pass (composite expression / different shape) ·
**D** impossible. `⚠` marks a bucket whose value diverges from DuckDB at an edge — see §2.

| `SqlFn` | DuckDB spelling | SQLite | SQLite spelling / construction | PG | Postgres spelling / construction |
|---|---|---|---|---|---|
| CONCAT | `concat(…)` | **A** | `concat(…)` — NULL-skipping identical | **A** | `concat(…)` |
| LENGTH | `length(s)` | **A** | `length(s)` | **A** | `length(s)` |
| UPPER | `upper(s)` | **A**⚠ | `upper(s)` — **ASCII-only case mapping** | **A**⚠ | `upper(s)` |
| LOWER | `lower(s)` | **A**⚠ | `lower(s)` — **ASCII-only** | **A** | `lower(s)` |
| SUBSTRING | `substr(s,a[,b])` | **A** | `substr(s,a[,b])` — matched on all 13 edge probes | **B**⚠ | `substr` exists but no negative-start wrap and negative length **throws**: `CASE WHEN a<0 THEN right(s,-a) ELSE substr(s,a) END` |
| STRPOS | `strpos(h,n)` | **C** | `instr(h,n)` — same order, 1-based, 0 = miss, `''`→1 | **A** | `strpos(h,n)` |
| STARTS_WITH | `starts_with(s,p)` | **B** | `instr(s,p)=1` (matches incl. `p=''` and NULL) | **A** | `starts_with(s,p)` |
| ENDS_WITH | `ends_with(s,p)` | **B** | `substr(s,length(s)-length(p)+1)=p` | **B** | **PG has no `ends_with`**: `right(s,length(p))=p` |
| MATCHES | `regexp_matches(s,p)` → BOOL | **D** | no regex engine of any kind | **C**⚠ | `s ~ p` — **`regexp_matches` exists on PG but is a set-returning `text[]`, see §2.1** |
| LEFT | `left(s,n)` | **B** | `substr(s,1,n)` for `n>=0`; full semantics `CASE WHEN n<0 THEN substr(s,1,max(length(s)+n,0)) ELSE substr(s,1,n) END` | **A** | `left(s,n)` (negative `n` matches) |
| RIGHT | `right(s,n)` | **B** | `CASE WHEN n<=0 THEN '' ELSE substr(s,-n) END` — the guard is required, `substr(s,-0)` returns the **whole string** | **A** | `right(s,n)` |
| LPAD | `lpad(s,n,p)` | **B** | `substr(substr(replace(hex(zeroblob(n)),'00',p),1,max(0,n-length(s)))||s,1,n)` — matched DuckDB incl. truncation and multi-char pad | **A** | `lpad(s,n,p)` |
| RPAD | `rpad(s,n,p)` | **B** | `substr(s||replace(hex(zeroblob(n)),'00',p),1,n)` | **A** | `rpad(s,n,p)` |
| TRIM | `trim(s[,cs])` | **A** | `trim(s[,cs])` — multi-char set identical | **A** | `trim(s[,cs])` (PG 18 accepts the 2-arg function form) |
| LTRIM | `ltrim(s[,cs])` | **A** | `ltrim(s[,cs])` | **A** | `ltrim(s[,cs])` |
| RTRIM | `rtrim(s[,cs])` | **A** | `rtrim(s[,cs])` | **A** | `rtrim(s[,cs])` |
| REPLACE | `replace(s,f,t)` | **A** | `replace(s,f,t)` — incl. `f=''` no-op and NULL | **A** | `replace(s,f,t)` |
| SPLIT | `string_split(s,d)` → LIST | **D** | **no ARRAY type.** `json_group_array` yields a JSON *string* no `LIST_*` can consume | **C** | `string_to_array(s,d)` → `text[]`, incl. empty elements |
| SPLIT_PART | `split_part(s,d,n)` | **B** | delimiter-padded peel, **`n` must be a literal**: `substr(T,1,instr(T,',')-1)` where `T` is `substr(…,instr(…,d)+1)` applied `n-1` times over `s||replicate(d,n)` | **A**⚠ | `split_part(s,d,n)` |
| REVERSE_STRING | `reverse(s)` | **A** | `reverse(s)` — **driver extra**, UTF-8 aware, identical | **A** | `reverse(s)` |
| ASCII_CODE | `ascii(s)` | **C**⚠ | `unicode(s)` | **A** | `ascii(s)` |
| CHR | `chr(n)` | **C** | `char(n)` — identical incl. `n=0` and astral planes | **A**⚠ | `chr(n)` |
| UC_FIRST | `upper(substr(x,1,1))\|\|substr(x,2)` | **A**⚠ | same composite (inherits ASCII-only `upper`) | **A** | same composite |
| LC_FIRST | `lower(substr(x,1,1))\|\|substr(x,2)` | **A**⚠ | same composite | **A** | same composite |
| ENCODE_BASE64 | `to_base64(CAST(x AS BLOB))` | **B** | recursive-CTE encoder over `hex(CAST(x AS BLOB))` — **verified byte-exact** vs DuckDB on 5 real rows + 5 literals (§3) | **B**⚠ | `replace(encode(x::bytea,'base64'), E'\n','')` — **the `replace` is mandatory**, see §2.2 |
| DECODE_BASE64 | `CAST(from_base64(x) AS VARCHAR)` | **B** | recursive-CTE decoder → `CAST(unhex(…) AS TEXT)` — verified incl. UTF-8 | **B**⚠ | `convert_from(decode(x,'base64'),'UTF8')` — a plain `::text` cast yields `\x68656c6c6f` |
| LEVENSHTEIN | `levenshtein(a,b)` | **B** | recursive-CTE DP carrying the matrix as a fixed-width string — matched DuckDB on 6/6 cases (§3) | **C**⚠ | `levenshtein(a,b)` **after `CREATE EXTENSION fuzzystrmatch`** — a per-database DDL prerequisite |
| JARO_WINKLER | `jaro_winkler_similarity(a,b)` | **D** | see §4 | **D** | see §4 |
| GUID | `uuid()` | **B** | `lower(hex(randomblob(4)))\|\|'-'\|\|…` — 36-char RFC-4122-v4-shaped, verified `length(…)=36` | **C** | `gen_random_uuid()` (core PG 13+) or `uuidv4()` (PG 18) |
| FORMAT | `printf(fmt,…)` | **A**⚠ | `printf(fmt,…)` — full C specifier set, `%s %d %.2f %%` all identical | **B**⚠ | `format()` supports only `%s %I %L %% %n$s`. **`%d` and `%.2f` throw.** Rewrite specifiers: `%d`→`%s`, `%.Nf`→`to_char(x,'FM990.99')` |
| HASH | `hash(x)` → UBIGINT | **D** | no hash function of any kind | **D** | `hashtext`/`hashtextextended` are different algorithms (§4) |
| MD5 | `md5(s)` | **D** | absent (§4) | **A** | `md5(s)` — identical hex |
| SHA1 | `sha1(s)` | **D** | absent (§4) | **B** | `encode(digest(s,'sha1'),'hex')` **after `CREATE EXTENSION pgcrypto`**; core PG has no `sha1` |
| SHA256 | `sha256(s)` | **D** | absent (§4) | **B**⚠ | `encode(sha256(s::bytea),'hex')` — bare `sha256()` returns **bytea**, not hex text |
| REPEAT_STR | `repeat(s,n)` | **B** | `replace(hex(zeroblob(n)),'00',s)` — matches at `n=0` and `n<0` where the driver's `replicate` throws | **A** | `repeat(s,n)` |
| REGEXP_EXTRACT | `regexp_extract(s,p[,g])` | **D** | — | **B**⚠ | group 0: `coalesce(regexp_substr(s,p),'')`; group n: `coalesce(regexp_substr(s,p,1,1,'',n),'')`. **The `coalesce` is mandatory** (§2.3) |
| REGEXP_EXTRACT_ALL | `regexp_extract_all(s,p)` | **D** | — | **B** | `ARRAY(SELECT (regexp_matches(s,p,'g'))[1])` — identical incl. empty result |
| REGEXP_REPLACE | `regexp_replace(s,p,r[,f])` | **D** | — | **A** | `regexp_replace(…)` — identical on 6/6 incl. default-first-only, `'g'`, `'gi'`, `\2\1` backrefs |
| REGEXP_FULL_MATCH | `regexp_full_match(s,p)` | **D** | — | **B** | `s ~ ('^(' \|\| p \|\| ')$')` or `regexp_like(s,'^('\|\|p\|\|')$')` |
| GREATEST | `greatest(…)` | **B**⚠ | `coalesce(max(coalesce(a,b),coalesce(b,a)),a,b)` — **SQLite's `max()` returns NULL if ANY arg is NULL** (§2.4) | **A**⚠ | `greatest(…)` — NULL-skipping identical |
| LEAST | `least(…)` | **B**⚠ | `coalesce(min(coalesce(a,b),coalesce(b,a)),a,b)` | **A** | `least(…)` |

### Bucket totals — this slice, 41 constructs

| | SQLite | Postgres |
|---|---|---|
| **A** native | **13** (32%) | **25** (61%) |
| **C** rendering override | **3** (7%) | **4** (10%) |
| **B** rewrite pass | **14** (34%) | **10** (24%) |
| **D** impossible | **11** (27%) | **2** (5%) |

SQLite reachable (A+C+B) = **30/41 (73%)**. Postgres reachable = **39/41 (95%)**.

---

## 2. Silent value divergence — OK on the target, **different value** than DuckDB

This is the defect class that produces wrong rows with no error anywhere. Every entry was measured.

### 2.1 `regexp_matches` on Postgres changes the CARDINALITY of the result — worst in the slice

`MATCHES` renders as `regexp_matches(s, p)`. That spelling **exists on Postgres and parses**, but it
is a *set-returning function returning `text[]`*, not a boolean predicate. In a select list it
cross-joins the row against its match set, so **non-matching rows vanish**:

| probe | statement | DuckDB | Postgres |
|---|---|---|---|
| `R6.srf2` | `SELECT count(*) FROM (SELECT e.id, regexp_matches(e.name,'a') FROM emp e) t` | **5 rows** | **3 rows** |
| `R6.srf4` | same with `'[0-9]'` (matches nothing) | **5 rows** | **0 rows** |
| `MATCHES.c1` | `SELECT regexp_matches('abc123','[0-9]+')` | `Boolean:true` | `Array:[123]` |
| `MATCHES.c2` | `SELECT regexp_matches('abcdef','[0-9]+')` | `Boolean:false` | `<norows>` |

A `matches()` in a projection silently deletes rows. Must render `s ~ p`.

### 2.2 `~` means the opposite thing on the two backends

DuckDB's `~` is `regexp_full_match`; Postgres's `~` is a **partial** match. Same operator, inverted
semantics, no error either way.

| probe | statement | DuckDB | Postgres |
|---|---|---|---|
| `R6.tilde2` | `'abc123' ~ 'bc1'` | **false** | **true** |
| `R6.tilde3` | `'abc123' !~ 'bc1'` | **true** | **false** |
| `MATCHES.c3` | `'abc123' ~ '[0-9]+'` | **false** | **true** |

So on PG, `MATCHES` → `s ~ p` and `REGEXP_FULL_MATCH` → `s ~ '^('||p||')$'`; on DuckDB the *same*
`~` already means full match. A shared "`~`" spelling row would be wrong on one of them.

### 2.3 Postgres `encode(…, 'base64')` line-wraps at 76 characters

| probe | input | DuckDB `to_base64` | Postgres `encode(…,'base64')` |
|---|---|---|---|
| `ENCODE_BASE64.c6/c7` | `repeat('a',100)` | `YWFh…YQ==` (one 136-char line) | same, **with a `\n` after char 76** |

Any payload over 57 input bytes diverges. `ENCODE_BASE64.c8` (`replace(…, chr(10), '')`) reproduces
DuckDB byte-for-byte. The `replace` is not optional.

### 2.4 SQLite `max()`/`min()` are NULL-propagating; `GREATEST`/`LEAST` are NULL-skipping

| probe | statement | DuckDB | Postgres | SQLite |
|---|---|---|---|---|
| `GREATEST.c2` | `greatest(1,NULL,3)` / `max(1,NULL,3)` | **3** | **3** | **NULL** |
| `LEAST.c2` | `least(1,NULL,3)` / `min(1,NULL,3)` | **1** | **1** | **NULL** |
| `GREATEST.c11/12` | `greatest(e.sal,100.0)` on `eve` (sal NULL) | **100.0** | **100.0** | **NULL** |

`greatest`/`least` are emitted by `Lowerer.java:2087, :2811` and `Scalars.java:1047, 1068, 1140, 1217,
1246` — including as **clamps on `SUBSTRING` start/length**, so a NULL leaking through turns a whole
substring into NULL. Verified fix (`R3.gr3`/`gr4`/`le1`/`le2`/`gr5`):
`coalesce(max(coalesce(a,b),coalesce(b,a)), a, b)`, which also returns NULL when *all* args are NULL.

### 2.5 Postgres `substr` has no negative-start wrap, and negative length throws

| probe | statement | DuckDB | SQLite | Postgres |
|---|---|---|---|---|
| `SUBSTRING.c13` | `substr('abcdef',-3)` | **`def`** | `def` | **`abcdef`** |
| `SUBSTRING.c4` | `substr('abcdef',-3,2)` | **`de`** | `de` | **`''`** |
| `SUBSTRING.c7` | `substr('abcdef',2,-1)` | `a` | `a` | **ERROR** `negative substring length not allowed` |
| `RIGHT.c2` | `substr('abcdef',-3)` as `right` | `def` | `def` | **`abcdef`** |

Anywhere the lowering emits a negative or computed-possibly-negative start, PG silently returns the
whole string. `R3.sub1` verifies the rewrite `CASE WHEN a<0 THEN right(s,-a) ELSE substr(s,a) END`.

### 2.6 SQLite `upper`/`lower` are ASCII-only

| probe | statement | DuckDB | Postgres | SQLite |
|---|---|---|---|---|
| `UPPER.c2` | `upper('héllo')` | `HÉLLO` | `HÉLLO` | **`HéLLO`** |
| `LOWER.c2` | `lower('HÉLLO')` | `héllo` | `héllo` | **`hÉllo`** |
| `LOWER.c3` | `lower('İ')` | `i` | `i` | **`İ`** |
| `UC_FIRST.c6` | `upper(substr('école',1,1))\|\|substr('école',2)` | `École` | `École` | **`école`** |
| `LC_FIRST.c2` | on `'ÉCOLE'` | `éCOLE` | `éCOLE` | **`ÉCOLE`** |

`sqlite_compileoption_used('ENABLE_ICU')` = **0**, so there is no in-SQL remedy. This propagates into
`UC_FIRST`/`LC_FIRST` and into any case-insensitive comparison built on `upper()`.

Minor, in the other direction: `upper('ß')` — DuckDB `ẞ` (U+1E9E), Postgres and SQLite both `ß`.

### 2.7 `LIKE` is case-INSENSITIVE on SQLite; and `LIKE`-based prefix/suffix tests are unsound anyway

| probe | statement | DuckDB | Postgres | SQLite |
|---|---|---|---|---|
| `STARTS_WITH.c4` | `'ABCdef' LIKE 'abc%'` | false | false | **true** |
| `R3.ew6` | `'abcdef' LIKE '%'\|\|'d_f'` | **true** | **true** | **true** |
| `R3.ew7` | `right('abcdef',length('D_F'))='D_F'` | **false** | **false** | — |

Two separate defects. (a) SQLite's `LIKE` folds ASCII case; `PRAGMA case_sensitive_like=ON` fixes it
(verified, `R3.ci1`/`ci2`) but is **connection-scoped session state** — it belongs in the `before`
list of the widened dialect seam (`H2_BACKEND.md` §8), not in an expression. (b) On *every* backend,
`s LIKE '%'||p` is wrong when `p` contains `%` or `_`. Use `right(s,length(p))=p`.

### 2.8 Regexp extract returns `''` on DuckDB and `NULL` everywhere else

| probe | statement | DuckDB | Postgres |
|---|---|---|---|
| `REGEXP_EXTRACT.c7` | `regexp_extract('abcdef','[0-9]+')` | **`''`** (empty string) | — |
| `REGEXP_EXTRACT.c8` | `regexp_substr('abcdef','[0-9]+')` | — | **NULL** |
| `REGEXP_EXTRACT.c10` | `substring('abcdef' from '[0-9]+')` | — | **NULL** |

Empty-string vs NULL flips every downstream `IS NULL`, `coalesce`, `length` and `concat`.
`R3.re1` confirms `coalesce(regexp_substr(…),'')` restores DuckDB's value.

**And the default group differs:** `regexp_extract(s,p)` with no group index returns the **whole
match** (group 0), but the natural PG translation `(regexp_match(s,p))[1]` returns the **first capture
group**:

| probe | statement | value |
|---|---|---|
| `REGEXP_EXTRACT.c13` | DuckDB `regexp_extract('abc123','([a-z]+)([0-9]+)')` | **`abc123`** |
| `REGEXP_EXTRACT.c4` | PG `(regexp_match('abc123','([a-z]+)([0-9]+)'))[1]` | **`abc`** |
| `R3.re2` | PG `regexp_substr('abc123','([a-z]+)([0-9]+)')` | **`abc123`** ✓ |

`regexp_substr` is the correct group-0 spelling; `regexp_match[…]` is not.

### 2.9 `levenshtein` is byte-based on DuckDB and character-based on Postgres

| probe | statement | DuckDB | Postgres |
|---|---|---|---|
| `R6.lev2` | `levenshtein('héllo','hello')` | **2** | **1** |
| `R6.lev1` | `levenshtein('kitten','sitting')` | 3 | 3 |

DuckDB counts the two UTF-8 bytes of `é` as two edits. Identical spelling, identical ASCII answers,
divergent on any non-ASCII input.

### 2.10 `FORMAT`: DuckDB's `format` is not Postgres's `format`, and NULL handling differs

DuckDB has **both** `printf` (C specifiers) and `format` (`{}` placeholders). `Spellings.DUCKDB` maps
`FORMAT`→`printf`, which is correct — but the neighbouring name is a live trap:

| probe | statement | DuckDB | SQLite | Postgres |
|---|---|---|---|---|
| `FORMAT.c2` | `format('%s-%s','a','1')` | **`%s-%s`** (literal!) | `a-1` | `a-1` |
| `FORMAT.c5` | `format('%.2f',3.14159)` | **`%.2f`** (literal!) | `3.14` | ERROR |
| `FORMAT.c3` | `format('%s-%d','a',1)` | **`%s-%d`** | `a-1` | ERROR `unrecognized format() type specifier "d"` |
| `FORMAT.c10` | `printf('%s',NULL)` | **NULL** | **`''`** | — |
| `R6.f1` | `format('%s',NULL)` | NULL | — | **`''`** |
| `FORMAT.c9` | `printf('%1$s-%2$s','a','b')` | **`%1$s-%2$s`** | **NULL** | `a-b` |

Three distinct answers to the same statement on three backends, all `OK`. Postgres additionally
supports **only** `%s %I %L %% %n$s` — `%d` and `%.2f` throw, so `FORMAT` on PG is a specifier-rewrite
pass (bucket B), not a spelling row.

### 2.11 Assorted edges, all measured

| probe | statement | DuckDB | SQLite | Postgres |
|---|---|---|---|---|
| `ASCII_CODE.c5/c6` | `ascii('')` / `unicode('')` | **0** | **NULL** | **0** |
| `CHR.c5` | `chr(0)` | `U+0000` | `U+0000` | **ERROR** `null character not permitted` |
| `RIGHT.c5/c6` | `right('abc',0)` vs `substr('abc',-0)` | `''` | **`abc`** | **`abc`** |
| `SPLIT_PART.c4` | `split_part('a,b,c',',',0)` | **`''`** | — | **ERROR** `field position must not be zero` |
| `R3.sp2` | `split_part('a,b,c','',2)` | **`,`** | — | **`''`** |
| `REPEAT_STR.c5` | `repeat('ab',-1)` | `''` | `replicate` **ERRORS** | `''` |
| `X.rightstr4` | `rightstr('abcdef',-2)` | (`right`→`cdef`) | **`SQLITE_NOMEM`** | — |
| `GREATEST.c7` | `greatest(1.5,2)` | `BigDecimal:2.0` | — | `BigDecimal:2` (scale 0) |
| `DECODE_BASE64.c3` | `CAST(decode('aGVsbG8=','base64') AS text)` | — | — | **`\x68656c6c6f`** not `hello` |
| `R3.ci2` (LIKE, no pragma) | `'ABCdef' LIKE 'abc%'` | false | **true** | false |

---

## 3. The two SQLite constructions worth keeping — verified byte-exact

Both rest on §0's correlated-recursive-CTE capability and both were validated against DuckDB's own
answer, not against my expectation.

**Base64 encode** — walks `hex(CAST(x AS BLOB))` in 3-byte (6 hex-digit) groups, folds each to a
24-bit integer with `instr('0123456789ABCDEF', …)`, indexes the alphabet with `>>`/`&`, and pads:

| probe | input | SQLite construction | DuckDB `to_base64` |
|---|---|---|---|
| `R4.enc1` | `'hello'` | `aGVsbG8=` | `aGVsbG8=` |
| `R4.enc2/3` | `'hi'` / `'a'` | `aGk=` / `YQ==` | same |
| `R4.enc4` | `'héllo'` | `aMOpbGxv` | same |
| `R4.enc5` | 36 chars | `YWJjZGVmZ2hpamtsbW5vcHFyc3R1dnd4eXowMTIzNDU2Nzg5` | same |
| `R5.b64all` | all 5 `emp.name` rows | `YWxpY2U=\|Ym9i\|Y2Fyb2w=\|ZGF2ZQ==\|ZXZl` | **identical** |

Decode (`R4.dec1-4`) round-trips all four including UTF-8. **Two guards are required and are not in
the expression as probed:** empty input yields `AA==` (DuckDB `''`) and NULL input yields `AA==`
(DuckDB NULL) — wrap in `CASE WHEN x IS NULL OR x='' THEN … END`.

**Levenshtein** — a recursive CTE that appends one DP cell per iteration, carrying the whole matrix as
a fixed-width 4-char-per-cell string and indexing it with `substr`:

| probe | args | SQLite construction | DuckDB `levenshtein` |
|---|---|---|---|
| `R5.lev1` | `kitten`,`sitting` | **3** | 3 |
| `R5.lev2` | `abc`,`abc` | **0** | 0 |
| `R5.lev3/4` | `abc`,`''` / `''`,`xyz` | **3** / **3** | 3 / 3 |
| `R5.lev5` | `flaw`,`lawn` | **2** | 2 |
| `R5.lev6` | `emp.name`(correlated),`alice` | **5** | 5 |

6/6. Cost is `O(m·n)` recursion steps per row and the expression text is ~1.5 KB — a real performance
ceiling to measure, not a correctness objection (same posture as `H2_BACKEND.md` §3).

---

## 4. Every `D` — what was tried and why it is impossible

**SQLite (11).**

- **MATCHES, REGEXP_EXTRACT, REGEXP_EXTRACT_ALL, REGEXP_REPLACE, REGEXP_FULL_MATCH** — tried
  `x REGEXP p`, `regexp(p,x)`, `regexp_like`, `regexp_matches`, `regexp_instr`, `regexp_count`,
  `regexp_substr`, `regexp_replace`: all `no such function`. `SELECT count(*) FROM pragma_function_list
  WHERE name='regexp'` = **0** — the xerial driver registers no REGEXP hook, so even the `REGEXP`
  *operator* (which SQLite parses but dispatches to a user function) fails. `load_extension()` errors,
  closing the loadable-extension route. `GLOB` exists but is shell globbing, not regex: no character
  classes, alternation, quantifiers or capture groups. Nothing in SQL can implement a regex engine.
- **SPLIT** — SQLite has **no ARRAY type**. `json_array`/`json_group_array` work (`R3.split1/3`
  produced `["a","b","c"]`) but yield a **`text`** value (`typeof` = `text`), not a list; no `LIST_*`
  `SqlFn` can consume it. This is the collection-carrier wall of `H2_BACKEND.md` §4.1, not a spelling
  gap — it is fixed by the collection redesign or not at all.
- **MD5, SHA1, SHA256** — tried `md5`, `sha1`, `sha256`, `sha3`, `sha3_query`, `hex(md5(…))`; none
  exist (census: 0 hits). SQLite *does* have `& | << >> ~`, `hex`, `unhex` and correlated recursive
  CTEs, so a bit-level implementation is not formally impossible — but MD5 is 64 rounds of 32-bit
  modular arithmetic over a length-padded message, and SHA-256 is 64 rounds over a 64-entry schedule,
  per row, with no 32-bit type (SQLite integers are signed 64-bit, so every operation needs an
  explicit `& 0xFFFFFFFF`). Probed the primitives (`R4.bit1-4`: `1<<31`→2147483648,
  `(0xFFFFFFFF+1)&0xFFFFFFFF`→0, both correct). Rejected as a construction: it is a several-thousand-
  character expression per call site with no test oracle short of reimplementing the RFC. **D in
  practice**; if it is ever needed the honest route is a JDBC-side codec, not SQL.
- **HASH** — no hash primitive of any kind, and `SqlFn.HASH` (`Lowerer.java:1074`) needs a *stable*
  64-bit value, so no ad-hoc substitute is admissible.
- **JARO_WINKLER** — tried `jaro_winkler_similarity`, `jaro_winkler`, `jarowinkler`,
  `jaro_similarity`, `similarity`: absent. The driver's `difference()` is soundex distance 0–4
  (returned **3** for `kitten`/`sitting` vs DuckDB's **0.746**) — a different metric, not a
  substitute. Unlike Levenshtein, Jaro-Winkler needs a match-window scan *plus* a transposition count
  over the matched subsequences — two dependent per-character passes, i.e. nested correlated
  recursion. Not attempted beyond scaffolding; recorded as D rather than claimed as B.

**Postgres (2).**

- **HASH** — `hash()` does not exist. `hashtext('abc')` returns `Integer:-785388649` and
  `hashtextextended('abc',0)` returns `Long:-6747756470228489321`; DuckDB's `hash('abc')` returns
  `BigInteger:1924864467101078684`. Different algorithms, different widths (PG's is signed, DuckDB's
  is UBIGINT), and both PG functions are undocumented internals whose values are explicitly not
  stable across versions. No spelling reproduces DuckDB's value, so any query whose *output* contains
  a hash diverges. (If `HASH` is only ever used for grouping/bucketing and never surfaced, this
  downgrades to C — check the call site at `Lowerer.java:1074`.)
- **JARO_WINKLER** — not in core, not in `fuzzystrmatch` (which ships `levenshtein`,
  `levenshtein_less_equal`, `soundex`, `difference`, `metaphone`, `dmetaphone`, `daitch_mokotoff` —
  all probed), not in `pg_trgm`. `similarity('kitten','sitting')` = **0.0714** vs DuckDB **0.746** —
  trigram overlap, an unrelated metric. Constructible only as a full recursive implementation, same
  objection as SQLite.

---

## 5. Returned Java type divergence — candidate `SqlDialect.normalize` rows

`SqlDialect.normalize` has zero overrides today (`H2_BACKEND.md` §8). These are the rows this slice
requires. The **boolean** row is the urgent one: SQLite has no boolean type, so every predicate this
slice produces comes back as `Integer` 1/0.

| Expression class | DuckDB | SQLite | Postgres | Note |
|---|---|---|---|---|
| **Predicates** (`STARTS_WITH`, `ENDS_WITH`, `MATCHES`, `REGEXP_FULL_MATCH`, all comparisons) | `Boolean` | **`Integer` 1/0** | `Boolean` | probes `R3.t2/t3`, `STARTS_WITH.*`, `ENDS_WITH.*` — affects *every* boolean in the slice |
| `LENGTH`, `STRPOS`, `LEVENSHTEIN` | **`Long`** | `Integer` | `Integer` | `R3.t1/t4/t5`, `R6.t1/t2/t4` — DuckDB returns BIGINT, both targets INTEGER |
| `SHA256` (`sha256(x::bytea)`) | `String` (hex) | — | **`byte[]`** | `R6.t5` — the H2-JSON-as-`byte[]` precedent exactly; `encode(…,'hex')` avoids it |
| `DECODE_BASE64` raw | `DuckDBBlobResult` | `byte[]` | `byte[]` | `DECODE_BASE64.c4`, `X.hex2` — all three need an explicit text cast |
| `GUID` | `UUID` | **`String`** | `UUID` | `GUID.c1/c2`, `R3.guid1` — SQLite's construction yields text |
| `SPLIT` | `Array` | **n/a** (no array type) | `Array` | `SPLIT.c1/c2` |
| `GREATEST`/`LEAST` on mixed numerics | `BigDecimal:2.0` | `Double` | `BigDecimal:2` | `GREATEST.c7`, `R6.t8` — **scale differs**, so `toString` comparison fails |
| `ASCII_CODE` | `Integer` | `Integer` | `Integer` | agrees (the one integer that does) |
| SQLite `random()` | `Double` [0,1) | **`Long`** (signed 64-bit) | `Double` | `GUID.c9` — adjacent to this slice, flagged for whoever owns it |

---

## 6. Two things that are not spellings and need a home

1. **`PRAGMA case_sensitive_like=ON`** must run on every SQLite connection or `LIKE` folds ASCII case
   (§2.7). It is session state, so it is a `before` statement in the widened seam
   (`H2_BACKEND.md` §8) — exactly the "sequencing, not computation" carve-out that section allows.
2. **`CREATE EXTENSION fuzzystrmatch` and `CREATE EXTENSION pgcrypto`** are prerequisites for
   `LEVENSHTEIN` and `SHA1` on Postgres. Verified by execution: on a **fresh** database
   `levenshtein('kitten','sitting')` fails with `function levenshtein(...) does not exist`
   (`R6.lev0`), and succeeds only after the `CREATE EXTENSION` (`R6.ext` → `R6.lev1`). These are
   **database-level DDL requiring elevated rights on the target**, not something a `before` statement
   on a read-only connection can supply. Either declare them as deployment prerequisites or treat
   `LEVENSHTEIN`/`SHA1` as D on unprivileged Postgres.
