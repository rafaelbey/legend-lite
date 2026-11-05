# SAP IQ SQL in Pure: the `#IQ{...}#` island, the research behind it, and the plan

**Status: study and plan (2026-10-13), with the user's decisions recorded in §4, kept as spec input for the port. The
line was built 2026-10-13 to 2026-10-14 (§9's landing records, cut in the 2026-11 docs cleanup; the SQL Anywhere and IQ
facts they measured are listed after §8).** This is the counterpart of `docs/LEGEND_SQL_STUDY_2026_10_11.md`, which did the same for Postgres and
the `#SQL{...}#` island. Read that study's §3.4, §8 and §9 first: this line follows its method (the dialect is the
spec, a refusal by name beats a possibly wrong answer, answers snapped from the real database and ratcheted per case)
and reuses its translator.

§0 is the summary; §1–§3 are the ground (what IQ SQL is, the oracle, the research); §4 is the decisions; §5–§6 are
the design; §7 is the plan; §8 the risks; §9 the record of landings.

---

## 0. Start here

1. **What the island is.** `#IQ{...}#` makes a query written for SAP IQ 16.2 an ordinary Pure Relation expression:

   ```
   #IQ{SELECT TOP 3 dept, COUNT(*) AS n
       FROM tb('e2e::IQ.emp')
       WHERE salary / 1000 >= 50
       GROUP BY dept
       ORDER BY n DESC}#->filter(r | $r.n > 1)
   ```

   The translator reads IQ's SQL and builds the Pure a person would write (filter, groupBy, sort, limit over
   `#>{e2e::IQ.emp}#`). The compiler types it once, like any query, and it runs on any backend legend-lite supports.
   It returns **IQ's answer**: here `salary / 1000` truncates toward zero when `salary` is an integer column (D-N3),
   and a NULL sort key would sort as the lowest value.
   `#IQ:<profile>{...}#` names the connection profile the SQL was written for (§4, I2).
   **A worked example:** `docs/IQ_SQL_SHOWCASE.md` follows one non-trivial query end to end -- SQL Anywhere's answer,
   lite's identical one, the Pure it desugars to, and the SQL it plans for Snowflake and DuckDB (pinned by a test).
2. **Status.** The research is done and the decisions are taken (§4). The first landing (§7, PR1 to PR3) is announced
   in `IN_FLIGHT`, with its cross-area edits listed there, and built on the branch (§9.1).
3. **Where the research lives: the user's private repository `neema2/iq-sql-research`. None of it is copied here.**
   - Its rules quote SAP's manuals sentence by sentence. It also holds the TPC-H full disclosure reports run on Sybase
     IQ (their queries fall under the TPC EULA) and about 4,000 queries collected from public GitHub repositories,
     which belong to their authors.
   - This repository holds only what we wrote: behaviour in our own words, the research's ids, and page numbers.
     Nothing in it is SAP's text, a manual's example, a TPC query or a corpus query.
4. **How to cite the research** (the ids are stable):

   | Id | What it names | Example |
   |---|---|---|
   | GRAMMAR `L`, `E`, `C`, `F`, `T`, `Q`, `S`, `R`, `G`, `X`, `K` + number | one of the 244 grammar productions (lexical, expressions, conditions, functions, types, query expressions, SELECT, FROM, GROUP BY, statements, client syntax) | GRAMMAR L28 (parameter markers) |
   | `D-<group><n>` | one of SEMANTICS §5's 68 provisional decisions | D-N3 (integer division) |
   | `VP-…` | a probe in the verification pack for real IQ | VP-N1-01 |
   | `IQ p.N`, `IQ-prog p.N`, `SA p.N` | a page of the IQ 16.2 SQL Reference, the IQ 16.0 SP04 Programming guide, the SA 17 SQL Reference | IQ p.187 |
   | SEMANTICS §n, TYPING §n, OPTIONS §n | a section of the research's spec | SEMANTICS §2 |

5. **The one-line rule of the line:** zero FAIL, zero BUG, as in the Postgres line. Where IQ's answer is not known
   for certain, or legend-lite cannot compute it without Java computing a value, the translator refuses by name and
   position.

---

## 1. What IQ SQL is

### 1.1 Two engines behind one parser

SAP IQ 16.2 is a column-store engine behind a SQL Anywhere front end. The IQ server embeds a SQL Anywhere server that
holds the catalog and parses every statement (IQ p.187, p.1565, p.1739). A statement that reads an IQ-store table is
then executed by the IQ engine; anything else is executed by SQL Anywhere itself. So IQ has **one parser and two sets
of execution rules**:

- **The parser is shared.** Keywords, precedence, literal typing and string escapes never depend on which engine runs
  the statement (SEMANTICS §2.3). Everything SQL Anywhere 17 accepts is IQ syntax too.
- **The answers can differ.** The same expression over an IQ table and over a constant can give different values or
  types (§1.3). Community reports describe the same split from the outside (SEMANTICS §2.1).

### 1.2 Which engine runs a query, and why it decides the oracle

| Query | Engine | `engine` flag |
|---|---|---|
| No FROM; FROM only `SYS.DUMMY`, `sa_rowgenerator(...)`, constant derived tables, catalog tables or SA procedures | the SA front end | `sa` |
| FROM reads any IQ-store table, including `iq_dummy` (a one-row IQ table) and IQ temporary tables; its subqueries and derived tables too | the IQ engine | `iq` |
| IQ tables mixed with catalog tables | IQ (D-E1) | `iq` |

In legend-lite, a `tb('pkg::DB.schema.table')` source is a user table, which in IQ would live in the IQ store. So an
island over `tb()` sources is `engine=iq`, and an island over nothing, DUMMY or `sa_rowgenerator` is `engine=sa`
(§4, I3).

**This split decides the oracle.** IQ itself cannot be run here (§2.1), but its front end can: SQL Anywhere 17 is the
very program that answers IQ's `engine=sa` queries. So SA 17 is a true oracle for `engine=sa`, and for `engine=iq`
only where the rule does not differ between the engines (§2.4). Everything else rests on the IQ documentation, read
under a policy the user chose (§4, I4), and stays provisional until a real IQ server answers the verification pack.

### 1.3 Where the engines differ

The rows the translator must key on `engine` (SEMANTICS §2.2, §5). Each is in our own words; the id names the
decision that settles it provisionally.

| Behaviour | IQ engine (`iq`) | SA front end (`sa`) | Id |
|---|---|---|---|
| DATEDIFF in hours, minutes, seconds and finer | whole units elapsed, truncated | unit boundaries crossed (hours and seconds derived from finer boundaries) | IQ p.264; D-T5 |
| One-argument DAYS, MONTHS, WEEKS… | counted from 1 January of year 0 | counted from 29 February of year 0, 59 days later | D-T4 |
| CHAR(n) values | blank padded to n; the pad counts in lengths and concatenation, not in comparisons | CHAR behaves as VARCHAR | D-S1 |
| A date or time with missing parts | year 1900, day 1900-01-01 | the current year or date | D-T8 |
| Decimal result types | the research's formulas at precision 126, scale 38 | the same formulas, capped by the database's PRECISION and SCALE | D-N1 |
| A quotient of decimal constants | at least six fractional digits | the formula's scale (two digits for `1.0/3.0`) | D-N2 |
| `x = NULL` under ANSINULL off | IS NULL everywhere, CASE included | only in comparisons standing directly in WHERE, HAVING or ON | D-L1 |
| DATEADD, and a date plus or minus a number | a DATE stays a DATE | always a TIMESTAMP | D-T2, D-T12 |
| CONVERT to text without a style | the session's date and time format options | a fixed default style | D-C6 |
| COUNT's type | unsigned 64-bit | 32-bit INTEGER | D-F15 |
| CEIL, CEILING, FLOOR | the argument's type | DOUBLE | D-F2 |
| Several LIST calls ordered differently in one block | the first order applies to all | an error | D-A1 |
| ROUND of a tie | an IQ-only option picks away from zero (the default) or to even | always away from zero | IQ p.448 |

### 1.4 What makes it a dialect of its own

IQ SQL is not ANSI SQL with a few extensions: only 14% of real hand-written IQ and SA queries use nothing beyond an
ANSI core (§3.3). The differences from Postgres that the translator spells out from the first slice:

- **Names** match without regard to case, quoted or not, folding only A–Z. `[x]` and `` `x` `` are identifiers; `"x"`
  is an identifier or a string, as the `quoted_identifier` option says, and TDS logins turn it off (GRAMMAR L10–L12,
  L22).
- **Data compares by the database's settings.** An IQ database compares text case-sensitively and in byte order by
  default (every upper-case letter before every lower-case one); SA's default database compares without case. Case
  folding in UPPER, LOWER and case-insensitive comparison touches A–Z only, never `é`.
- **Trailing blanks.** Under blank padding, `=`, `<`, IN, DISTINCT and GROUP BY ignore them; LIKE does not (D-S5).
- **NULL is the lowest value:** first in ascending order, last in descending. `NULLS FIRST/LAST` is rejected (D-W1).
- **`+` is overloaded.** Two numbers add, two strings concatenate, a string and a number do decimal arithmetic. A
  single NULL operand of `||` or of string `+` counts as the empty string; two NULLs give NULL (D-S3).
- **Operators.** `^` is exclusive or, not a power. The bitwise operators bind tighter than `*`; unary minus binds
  looser than them (D-N4).
- **Integers.** `/` of two integers truncates toward zero and keeps an integer type; `%` takes the dividend's sign
  (D-N3). Overflow is an error, never a wider type, even for small literals: `32767` is a SMALLINT (TYPING §2–§3).
- **Literals.** Integers are typed by magnitude (SMALLINT, INTEGER, then NUMERIC), decimals by their digits without
  trailing zeros, E-notation as NUMERIC (D-N5). `DATE '…'` and `INTERVAL` literals are rejected; a string converts
  where it is used.
- **Comparisons** of a string with a number compare both as NUMERIC, not DOUBLE; a string with a date compares as a
  TIMESTAMP (SEMANTICS §3.4).
- **`IF c THEN a ELSE b ENDIF`** is NULL when `c` is unknown, ELSE or not.
- **Row limits.** `TOP n [START AT m]` (m counts from 1) and `FIRST`. A literal `TOP 0` is an error; an expression
  that is 0 gives no rows (D-Q1). `LIMIT` exists only when an option enables it (GRAMMAR S6).
- **T-SQL forms** are everyday IQ: `alias = expr`, `'string'` aliases, ISNULL, `CONVERT(type, value [, style])`, and
  `*=`/`=*` outer joins.
- **Joins.** NATURAL JOIN does not merge the shared column; `JOIN … USING` is rejected; KEY JOIN joins through the
  declared foreign keys.
- **No boolean values.** A predicate is not a value, and `TRUE` is a column name.
- **Backslash escapes** in strings are on by default and off on TDS logins (the `escape_character` option).

---

## 2. The oracle: SQL Anywhere 17 on this Mac

### 2.1 The install, and how it is reached

- **No IQ.** IQ 16.2 has no macOS build. The user ruled out the Linux trial for development: checks against real IQ
  wait for a licence, and the research's verification pack (166 probes) is ready for that day.
- **SQL Anywhere 17.0.11.7254 SP1** is installed at `/Applications/SQLAnywhere17`. Its binaries are x86_64 and run
  under Rosetta on this arm64 Mac.
- **No JVM driver.** Both SA JDBC jars load native x86_64 libraries, which an arm64 JVM cannot load; the only bundled
  JVM is an x86_64 Java 8. So Bazel's test JVMs cannot reach SA at all.
- **The working path:** Python, through `ctypes` over `libdbcapi_r.dylib`, run as
  `arch -x86_64 /Library/Developer/CommandLineTools/usr/bin/python3 -I` after sourcing
  `System/bin64/sa_config.sh`. The research's runner (`sarun.py`, `runprobes.sh`) is ported to
  `tools/iq-oracle/snap.py`. It starts its own server on a private port, restarts it after a crash, and skips a list
  of known crashers (§2.5).

### 2.2 Two databases, five profiles

`snap.py` creates both databases in a temporary directory and seeds both from `iq-parity/schema.yaml`:

| Database | Created as | Stands for |
|---|---|---|
| `lite_iq.db` | `dbinit -c -b -z ISO_BINENG`, then PUBLIC `precision` 126 and `scale` 38 | an IQ database's catalog store: case-sensitive, blank padding, ISO-8859-1 byte order, IQ's decimal limits |
| `lite_sa.db` | `dbinit` defaults | a plain SQL Anywhere database: case-insensitive, no padding, UTF8BIN, precision 30, scale 6 |

The five profiles (OPTIONS §9) map onto them. A profile is creation settings plus session options; the session options
are set with `SET TEMPORARY OPTION` before each case:

| Profile | Database | Session options on top of the defaults |
|---|---|---|
| `iq16-default` | `lite_iq` | none: a connection at the database's defaults (Embedded SQL, dbcapi) |
| `iq16-odbc` | `lite_iq` | the date and time formats the IQ ODBC driver and Interactive SQL set (six fractional digits in timestamps, none in times) |
| `iq16-tds` | `lite_iq` | what a TDS login (jConnect, Open Client) sets: ANSINULL off, QUOTED_IDENTIFIER off, DATE_ORDER MDY, ESCAPE_CHARACTER off |
| `sqla17-default` | `lite_sa` | none |
| `sqla17-tds` | `lite_sa` | as `iq16-tds` |

A caveat the research records (SEMANTICS §4.7): TDS and ODBC behaviour was reproduced by setting the options, never
over a real TDS or ODBC connection.

IQ island, landing 4 (WP1), 2026-10-14: three more profiles, `iq17-default`, `iq17-odbc` and `iq17-tds`, are the IQ
base with the database properties of the real SAP IQ 17.1 the island is now judged against (the user's HANA Cloud data
lake: CaseSensitive On, BlankPadding On, UTF8BIN, UTF-8, precision 126, scale 38). `tools/iq-oracle/snap.sh --oracle
iq` snaps their answers from that server, and every case run under an `iq16-*` profile also runs under the matching
`iq17-*` one. The `iq16-*` profiles stay judged by SQL Anywhere (ISO_BINENG and ISO-8859-1, which that server cannot
be), not verified on a real IQ 16. Where lite follows SQL Anywhere and IQ differs, the `iq17-*` run is listed, with its
cause, in `iq-parity/iq17-divergences.tsv`, which only shrinks. `iq17-tds` too sets the TDS login's options on a
CmdSeq connection (dbcapi cannot log in over TDS).

### 2.3 Snapped answers; CI judges from the file

- **The file:** `core/src/test/resources/iq-parity/iq-answers.jsonl`, one line per case and profile: the column names
  and SA types, the cells or the error (SQLCODE, SQLSTATE), the SA build, the database, and a `sqlSha` over the SQL,
  the profile and the schema.
- **CI judges from the committed file.** Neither lane talks to SA; they run the island on DuckDB and Postgres and
  compare with the snapped answer, by `SqlParity`'s rules.
- **The local drift check** replaces the Postgres line's live lane. `snap.py --check` asks SA again and fails on any
  difference; `--write` snaps new or changed cases. Each landing runs `--check` once and records it in its GATES
  entry. Between checks, the STALE guard (§6.3) stops an edited case from being judged by an old answer.
- **Not snapped:** volatile values (the clock, RAND, NEWID, the server's name and version), as in `sql-parity`.

### 2.4 What SA can judge, and what it cannot

| Case | Judge |
|---|---|
| `engine=sa` (no FROM, DUMMY, `sa_rowgenerator`) | SA's answer on the profile's database (`sa:lite_iq` or `sa:lite_sa`), except the defects of §2.5 |
| `engine=iq`, where the rule is engine-neutral: no row of §1.3, no per-engine decision, not in SEMANTICS §4 | SA's answer; the engine-neutrality guard (§6.3) proves the case is neutral |
| `engine=iq` under a per-engine decision the line has adopted | the decision: the case names `decision: D-xx` and carries its `expect:` |
| IQ-only functions and syntax (LAG, LEAD, NTILE, the PERCENTILE and DATEFLOOR families, LPAD, RPAD…), IQ-only options, IQ-store-only rules, restrictions only the IQ engine enforces (D-R1), the 92 rules where SA and the documentation disagree unexplained | a decision with `expect:`, or a refusal by name |

The seeded tables are SA tables, so SA executes them with front-end rules. That is exactly why an `engine=iq` case
over them is only judged where the rules agree.

### 2.5 SA 17 defects the tests must avoid

Found by the research (SEMANTICS §7.1), each with a minimal reproduction. None is ever reproduced, for either engine:

- **Server crashes:** FIRST_VALUE in its aggregate form with an ORDER BY inside the call; the smallest INTEGER `%` −1
  over INTEGER columns. Both are in the known-crashers list; no case may contain them.
- **ROLLUP, CUBE and GROUPING SETS** where two grouping columns are the same constant: the rolled-up column keeps its
  value instead of NULL.
- **`*=` over a constant derived table** keeps the constant for unmatched rows instead of NULL.
- **Timestamps from 7911-01-01 on** lose their hours and minutes, a constant's (`'7911-01-01 10:30'` reads as
  7911-01-01 00:00) and DATEADD's result of any part (`9999-12-31 22:30` plus an hour is `9999-12-31 00:00`,
  `2026-01-01 10:30` plus 6000 years `8026-01-01 00:00`); there a DATEADD past 9999-12-31 below a day answers rather
  than raising -158, and one of an hour onto 7911-01-01 raises it. The schema holds none; the island refuses such a
  constant with a time, and a DATEADD that may reach those years with a time of day (IQ island, 2026-10-14: review
  4's M2, §9.11).
- **Also:** a decimal beyond its declared precision prints as `?` instead of an overflow error; `f(constant) IS NULL`
  is folded without evaluating `f`; DATEADD in milliseconds drops microseconds; STDDEV loses all precision for large
  equal values; RAND collapses when grouped in a derived table.
- **Constant expressions are typed from their value** (`ABS` of a BIGINT constant is a one-digit NUMERIC). Types are
  therefore judged over the schema's typed columns, never over constants (TYPING, method caveat).

---

## 3. The research, in numbers

### 3.1 Sources

- **Read in full:** the SAP IQ 16.2 SQL Reference (2,174 pages) and the SQL Anywhere 17 SQL Reference (1,972 pages).
  The IQ 16.0 SP04 Programming guide was read for its OLAP appendix and client chapters.
- **Coverage audit:** all 4,204 outline headings of the three were walked (1,384, 1,745 and 1,075); no relevant one is
  left uncovered.
- **Measured:** every claim that could be probed was run on SA 17 against both test databases. "Measurements override
  documents" is the research's rule.

### 3.2 The spec

| Part | Size | Status |
|---|---|---|
| Rules from the manuals | 3,480 | 1,672 verified on SA (26 after correcting the reader); 283 where the manual gives no value and SA's is recorded; 837 documentation only (212 IQ-only features, 36 IQ-store rules, 13 options SA lacks, 576 not probe-able); 92 where SA and the manual disagree unexplained, all settled by decisions; 60 IQ restrictions SA does not enforce; 38 manual errors; 4 that crash SA |
| Grammar | 244 productions | each with its source and a policy: accept, flag, warn, reject, gated by an option, or accept-and-translate (client layers only) |
| Syntax cases | 11,304 | 8,551 accept, 2,447 reject, 179 flag, 127 gated by an option; each with SA's measured outcome |
| Reserved words | 255 | each measured as a bare alias on SA 17 |
| Options | 385 (IQ ∪ SA) | tiers 17 / 18 / 31 / 319: modelled in every profile, modelled when reached, pinned and refused otherwise, ignored |
| Typing | 22,969 cases | operators, comparisons, casts, functions, aggregates and unification over 24 types on both databases (85,716 probe runs); the derived formulas reproduce 13,320 of 13,322 measured result types |
| Decisions | 68 | 46 follow the IQ manual, 8 follow SA 17, 14 follow corroborating evidence where the manual contradicts itself; 49 apply to `engine=iq` only |
| Verification pack | 166 probes | 132 confirm or overturn a decision; 34 cover IQ-engine rules and community reports. Ready to run on real IQ |

### 3.3 Real-world SQL

- **The corpus:** 4,046 distinct SELECTs from 121 public repositories; 3,208 of them hand-written (89 repositories),
  without templated SQL, test fixtures and generated DDL guards. Contexts: 2,759 bare statements, 1,133 parenthesised
  queries inside procedural code, 84 INSERT … SELECT, 68 CREATE VIEW.
- **SA 17 parses 95.3% of it.** Of 171 syntax errors, 30 are genuine dialect issues (ODBC `{oj}` escapes, IQ-only LAG
  and LEAD, legacy reserved words as names, user domains, `= *` with a space). No real query needed a form the grammar
  lacks.
- **The most common constructs** (share of hand-written queries): owner-qualified tables 41.8%, SELECT … INTO 23.0%,
  `:host` variables 16.2%, `@variables` 14.7%, SA catalog tables 13.0%, comma joins 11.9%, quoted identifiers 11.8%.
- **Coverage by milestone** (a query is covered when every construct in it is in that milestone or an earlier one):

  | Milestone | Hand-written queries | Repository-weighted |
  |---|---|---|
  | M1, the core dialect SELECT | **86.3%** | 83.6% |
  | M2, the Watcom and T-SQL library, KEY JOIN, `*=`, ODBC escapes | **97.7%** | 96.3% |
  | M3, windows, CTEs, rare clauses | 100% | 100% |

  An ANSI-only core covers **14.0%** (33.0% with owner-qualified names). M1 without the catalog track covers 68.6%:
  20.3% of queries read catalog tables, `sp_iq*` or `sa_*` procedures, or DUMMY.

### 3.4 Other tools

- **No existing tool implements IQ or SQL Anywhere semantics.** sqlglot has no Sybase dialect, and its T-SQL dialect
  gives wrong answers for IQ input in at least nine reproduced places. The one open-source SA → Postgres converter is
  syntax-level, GPL, with known semantic errors. Calcite renders `START AT` off by one. There is no Trino, Spark,
  sqlparser-rs, ZetaSQL or JSqlParser dialect.
- **T-SQL answers are not IQ answers.** Of 4,196 Babelfish test SELECTs that both SQL Server and SA accept, 62.5% give
  the same value on SA. Such suites are query corpora to re-answer on SA, never expected outputs.
- **Probes from other projects:** 892 claims harvested from about 35 projects (jOOQ's Sybase dialect the richest): on
  SA's defaults 629 confirmed, 231 contradicted. A red team wrote 969 cases in other dialects' habits; SA accepted 664.

So the translator is built from the spec and the oracle, as the Postgres one was built from Postgres's manual and
source; there is nothing to port.

---

## 4. Decisions

**Decided by the user, 2026-10-13** (`I` for island, distinct from the research's `D-` ids):

- **I1. The island: `#IQ{...}#`, and `#IQ:<profile>{...}#`.** The default profile is `iq16-default`. The LEGEND_ENGINE
  surface keeps its refusal of unknown island types; upstream has no IQ island.
- **I2. A closed profile set:** `iq16-default`, `iq16-odbc`, `iq16-tds`, `sqla17-default`, `sqla17-tds` (§2.2), and
  (landing 4, WP1) `iq17-default`, `iq17-odbc`, `iq17-tds`, judged by a real IQ 17.1. Any other name is refused by
  name. A profile is fixed for the island; per-island option overrides are not offered.
- **I3. `engine=auto`.** An island over `tb()` sources is `engine=iq`; one with no FROM, or over DUMMY or
  `sa_rowgenerator`, is `engine=sa`; a mix is `iq` (D-E1). **Only the typing and function layer reads the engine,
  never the parser**, because IQ's parser is shared (§1.1).
- **I4. Split by engine, with provisional decisions.** Queries on IQ tables follow the IQ documentation; constant and
  catalog-only queries follow SA 17; where the manual contradicts itself, the side independent evidence supports wins;
  where it is silent, SA's value holds for both engines (D-P1). Every result of the policy is provisional until the
  verification pack runs on real IQ. Within it:
  - **Adopted,** low risk: for example D-N3 (integer division and overflow), D-Q1 (literal TOP 0), D-S3 (NULL || NULL
    is NULL), D-L1 (ANSINULL off rewrites everywhere under `engine=iq`).
  - **Refused by name until verified,** the research's highest risks: **D-N1** (IQ-engine decimal types), **D-N6**
    (narrowing a DOUBLE to NUMERIC truncates), **D-S5** (LIKE over padded CHAR), **D-C6** (CONVERT without a style),
    **D-T2/D-T12** (DATEADD and date ± n keep DATE), **D-F1** (ROUND of a DOUBLE typed NUMERIC). The refusal is
    UNSUPPORTED and its message names the decision.
  - The rest of the research's ten highest risks (D-R1, D-C7, D-N2, D-C1/D-C2) are reached in later slices; each is
    put to the same test when its slice comes. **A refusal by name is always better than a possibly wrong answer.**
  - **D-E2, provisional (the lead, 2026-10-14; the second review's B2).** The research has no rule for the order in
    which a search condition's operands are evaluated, and its D-E1 is the engine of a mixed statement, so this one
    takes the next number. When an operand of AND or OR in a WHERE can raise a run-time error (an integer result out
    of its type, a zero divisor), whether it is computed for a row that another operand already decides is a matter
    of the plan, in IQ and in lite's databases alike: SQL Anywhere computed such an operand for rows another operand
    excluded, except when an equality filter ran first, while DuckDB and Postgres skip it. So lite may answer where
    IQ raises (or raise where IQ answers); where no row raises, the two answers never differ. Ordinary WHERE
    arithmetic is not refused. A parity case whose WHERE puts such an operand beside AND or OR is not judged (`skip:
    plan-dependent run-time error (D-E2)`) or is rewritten, and the harness fails a judged one (§6.3). AND and OR
    inside a CASE's or an IF's condition, and the arguments of COALESCE, ISNULL, CASE and IF, are lazy in SQL
    Anywhere as in lite: outside the decision. (2026-10-14, the third review's M4: a HAVING is such a search condition
    too, and a SUM or an AVG an operand that can raise; the harness reads both.)
  - **Refused by name after the second review (2026-10-14):** a select-list alias read inside an ORDER BY expression
    (IQ reads the alias there; a bare alias or a position is read as before); an operation that can raise (a result
    that may leave its type, a divisor that may be zero) in the select list or ORDER BY of a statement with TOP,
    FIRST or START AT (IQ computes the select list only for the rows it keeps; lite's databases may compute it for
    the rows the limit drops); TOP, FIRST or START AT over a FROM clause without ORDER BY (the rows kept are the
    plan's choice); an expression whose checked operations would write out more than a bound of Pure nodes (each
    repeats its operands: about fourfold per nested level, twentyfold per quotient). A constant result out of its
    type's range is IQ's error at translation only when every operand is a constant IQ folds and none is NULL;
    otherwise the check runs where the value is computed. A BIGINT result is never checked by lite: the database
    raises its own overflow first.
  - **D-E3, provisional (M1c, 2026-10-14; for the lead).** SQL Anywhere checks a SUM's (and an AVG's) running total
    against the result type in the order its plan reads the rows (measured: 2147483647, 1, -5 raises -158 in one order
    and answers 2147483643 in another), and IQ's order is its own plan's too. The island raises IQ's -158 wherever some
    order could overflow: the sum of the positive values above the type's largest, or of the negative values below its
    smallest. It never answers where SQL Anywhere raises; where only some orders overflow it raises and SQL Anywhere may
    answer. A parity case in that zone is not judged (`skip: plan-dependent run-time error (D-E3)`). (2026-10-14, the
    third review's B1: the check is computed where the sum is read, and SQL Anywhere checks every group in the
    aggregation, so a checked SUM or AVG read only for some groups -- inside IF or CASE, an argument of COALESCE,
    ISNULL, IFNULL, NULLIF or ARGN, under AND, OR, NOT, a truth test, BETWEEN or IN in HAVING, under a row limit, in a
    subquery -- is refused by name; read everywhere, it raises as before.)
  - **D-T13, provisional (the lead, 2026-10-14; the third review's M3).** lite's clock is its database session's, in
    UTC (DuckDB's, and the embedded Postgres's under `SET TimeZone='UTC'`); IQ's is the server's local time. A query
    that shows the clock, or compares it with data or a constant near the hours they differ by, answers otherwise
    wherever the server is not on UTC; parity cases compare clock readings only with each other (or far from any
    edge). SQL Anywhere reads GETDATE() apart from a statement's other readings (measured: `GETDATE() = CURRENT
    TIMESTAMP` is mostly false, `GETDATE() < NOW()` mostly true), where lite reads one clock per statement: a
    statement reading GETDATE() beside another reading of the clock is refused by name; NOW(), CURRENT TIMESTAMP,
    TODAY() and CURRENT DATE are one statement time in both.
- **I5. The oracle is local; CI judges the committed answers** (§2.3), guarded so that the file cannot silently fall
  behind the cases (§6.3).
- **I6. One translator.** `SqlToPure` gains an input-dialect seam and Postgres's behaviour moves behind it unchanged;
  the IQ reader is new (§5). No fork.
- **I7. Nothing from SAP's manuals in this repository** (§0, point 3), and no TPC or corpus query text.

**Open:**

- **O1. The natives the island needs** (§5.5), each announced to the database owner (the ninth line owns
  `sql/dialect/`) before it lands, and refused by name until then:
  - an **ASCII-only case fold** for UPPER, LOWER and case-insensitive keys (Pure's `toUpper`/`toLower` render the
    database's Unicode functions);
  - an **ASCII-only rtrim** for blank padding (DuckDB's `rtrim` strips every Unicode space; the Postgres study's §9.4
    left the same question to the user);
  - **exact decimal division** at the formula's scale, truncating (`divide(x, y, n)` goes through a double on DuckDB).
- **O2. Owner-qualified names** (41.8% of real queries). What `dba.t` means without `tb()`: the schema `dba` of a
  Database the island names once, a package, or a profile setting. Until decided, a bare owner-qualified name is
  refused by name. **Drafted by M1e (IQ island, 2026-10-14; provisional, for the lead; §9.7):** the model is the
  island's catalog. An IQ owner is a schema of the model's Databases, matched as IQ matches names; the tables declared
  outside any schema are the island's user's, `DBA` (the user IQ and SQL Anywhere create a database with, the oracle's),
  as are an unqualified name's and T-SQL's `db..t`; a database's and a server's qualifiers are read and ignored, as IQ
  ignores them. An owner that is no schema (nor `DBA`) is IQ's -140, an owner's table not there IQ's -141; a name two
  Databases hold, an unqualified one `DBA` does not own (IQ would search its catalog too) and IQ's own users' tables
  (its catalog: `SYS`, `dbo`, `PUBLIC`, ...) are refused by name; `tb('...')` always names one table.
- **O3. Host variables and parameters.** `@var`, `:name` and `?` as Pure variables in scope (the Postgres island's
  `var('name')` is the precedent). Open: how a positional `?` binds, and the research's ruling that a column wins over
  a variable of the same name (GRAMMAR L27). **Drafted by M1e (IQ island, 2026-10-14; provisional, for the lead;
  §9.7):** `@name` and `:name` read the enclosing Pure query's variable `$name` in scope, its name as written (Pure's
  names are case-sensitive), typed by its Pure type (an Integer as IQ's BIGINT, a Float a DOUBLE, a `Decimal(p,s)` that
  NUMERIC, a String a VARCHAR, a StrictDate a DATE, a DateTime a TIMESTAMP; nullable when it may hold none); a column of
  `@name`'s name in scope is read first (GRAMMAR L27; measured with `tsql_variables` Off, and refused by name under a
  TDS profile, where a client binds `@name` as a placeholder that reads its value: §9.15); none is IQ's -143 (`@name`)
  or -188 (`:name`). A positional `?`
  (and `:?`) names nothing in scope: refused by name. An unaliased item over a variable is refused (the oracle can only
  run the value written in, which SQL Anywhere names otherwise).
- **O4. Error text.** Recommended: an error is judged by being an error, as in `sql-parity`; the snapped file keeps
  SQLCODE and SQLSTATE, and lite's messages are its own words naming IQ's code where there is one.

---

## 5. Architecture

```
Typer's island arm ─▶ compiler/spec/SqlIslands: picks the dialect, types each FROM source, passes store types
                              │
                              ▼
#IQ{...}#  ─▶ IqLexer, IqParser    (protocol/sql/iq/) ─┐
#SQL{...}# ─▶ SqlLexer, SqlParser  (protocol/sql/)    ─┴─▶ SqlAst ─▶ SqlToPure + InputDialect
                                                                    (PostgresDialect | IqDialect)
                                                                           │ a Relation ValueSpecification
                                                                           ▼
                                             NameResolver ─▶ Typer types it once ─▶ G … K   (unchanged)
```

### 5.1 One translator, an input-dialect seam

`SqlToPure` (6,224 lines at 987c37e18) already holds the hard, dialect-free machinery: scopes and bindings, joins,
set operations and WITH, plain and grouped selects, windows, three-valued logic, subqueries, BETWEEN, IN and IS
DISTINCT, the exact integer quotient, and the Pure builders. PR1 moves only what is Postgres's own behind an
`InputDialect` interface, into `PostgresDialect`:

| Behind `InputDialect` | Postgres today | IQ |
|---|---|---|
| reading the text | `SqlLexer`, `SqlParser` | `IqLexer`, `IqParser` |
| names: fold, resolve, hint | fold unquoted to lower case, then a unique case-insensitive match (D1 of the Postgres study) | ASCII case-insensitive, quoted or not; two candidates are an ambiguity error |
| default null order | NULLS LAST ascending | NULL lowest: first ascending |
| a select item's default name | `?column?` | the expression's text (so the AST keeps each item's source span) |
| literal and operator typing; unification | Postgres's | IQ's lattice and formulas (TYPING §1–§8) |
| the function catalog, casts, LIKE | Postgres's | IQ's, per engine |
| messages | Postgres's | our own words with IQ's codes (O4) |

- **Interface shape:** `parse(sql, profile)`, `resolve(ident, names, what)`, `nullsFirst(desc)`,
  `defaultName(expr, text)`, `typeOf(pureType, storeType)`, `literal(expr)`, `binary(op, l, r, pos)`,
  `unify(what, types)`, `function(call, args)`, `cast(t, typeName)`, `like(...)`, `message(problem, args)`.
- **Types:** the translator's `Tx` gains an opaque, nullable type detail. Postgres leaves it null; IQ puts an
  `IqType` there (family, integer width, unsigned, precision and scale, CHAR length).
- **Errors:** the `SqlTranslationException` call sites stay where they are; only the text comes from the dialect.
- **The judge of PR1:** both `sql-parity` ledgers unchanged, and the generated Pure of every parity case identical
  before and after (a one-off dump through `PureComposer`). `SqlToPureTest`, `SqlParserTest` and
  `PostgresDateTimeTest` stay green.

### 5.2 The IQ reader: `protocol/sql/iq/`

- **`IqLexer`** reads under the profile's options: `[x]` and backtick identifiers; `"x"` per `quoted_identifier`;
  backslash escapes per `escape_character`; nested comments and `//`; money and binary literals refused by name;
  glued tokens warned (GRAMMAR L1–L31).
- **`IqParser`** produces the shared `SqlAst`. `TOP`, `FIRST` and `START AT` become limit and offset.
  `IF c THEN a ELSE b ENDIF` becomes `CASE WHEN c THEN a WHEN NOT c THEN b END`, so an unknown condition gives NULL.
  T-SQL `alias = expr` and string aliases are aliases. Forms from other dialects are rejected with a message naming
  the IQ spelling (GRAMMAR §13).
- **`IqDialect`, `IqTypes`, `IqFunctions`, `IqProfile`;** later `IqDateTime` (IQ's date spellings, DATE_ORDER and
  NEAREST_CENTURY), the counterpart of `PostgresDateTime`.

### 5.3 Plumbing (the cross-area edits announced in `IN_FLIGHT`)

- **`protocol/spec/SqlIsland`** gains the island's language and a nullable profile, rather than a new record, so
  `ValueSpecification`, `Typer` and `NameResolver` stay untouched.
- **`parser/SpecParser`** gains `case "IQ"` and the `IQ:<profile>` form. The lexer needs no change: an island's tag
  is everything between `#` and `{`.
- **The wire and the printers:** a `classInstance` of type `IQ` with the value `{sql, profile}`, in
  `SpecIslandReader`, `ProtocolEmitter` and `PureComposer`.
- **`compiler/spec/SqlIslands`** picks the dialect by language, keeps each language's opening length for positions,
  and passes the store's column types (§5.4).
- **Docs:** a LITE-DESIGN family for the island in `docs/OWN_CORPUS_DECISIONS.md`, and `docs/SEMANTICS_REGISTER.md`
  rows for what lite cannot represent (§8).

### 5.4 Store types through the seam

IQ's answers depend on declared types the Pure type does not carry: an INT and a SMALLINT are both Integer, a
NUMERIC(10,2) and a NUMERIC(38,0) both Decimal, and a CHAR(10) pads where a VARCHAR(10) does not. The compiler knows
them: `TypedTableReference.storedTypes()` holds each column's declared store type. `SqlIslands` spells it as text and
passes it in the translator's `Column` callback, beside the Pure type. Text, because the protocol layer may not import
`com.legend.sql` (`ArchitectureTest.protocolIsTheBottomLayer`). A derived table or subquery loses the store type; IQ's
rules then apply to the Pure type's widest reading, or the translator refuses where the width would change the
answer.

### 5.5 Where natives are needed: Java never computes a value

As in the Postgres line (`docs/TENET_CHARTER.md` C2), a missing semantic is a new Pure native or a refusal by name,
never Java computing a value:

| IQ behaviour | How |
|---|---|
| ASCII-only case fold; CASE IGNORE keys | a new native, or a composition of `replace` (O1) |
| blank padding: comparisons ignore trailing spaces, only U+0020 | an ASCII rtrim (O1) |
| exact decimal `/` at the formula's scale | a lowering fix for the database owner (O1); until then decimal `/` is refused |
| checked SMALLINT and INT overflow, `/` and `%` by zero | built from existing natives, as `SqlToPure`'s range checks already are |
| CHAR(n) padding under `engine=iq` | `rpad` to n, from the store type (D-S1) |
| byte-order collation | already: DuckDB and the embedded Postgres (`--no-locale`) compare bytes |
| unsigned integers, TIME, BINARY | refused by name |

### 5.6 Constraints

- **Invariant 7b:** `protocol/**` depends only on protocol, values, JSON and the JDK.
- **TeaVM** compiles core to WebAssembly: no JDK method it lacks (`String.isBlank`, `Character.toString(int)`). Each
  landing builds `//sdlc-server:page` and `//wasm:planner`.
- **Guards** move only with a dated justification naming this line: `CodeShapeGuardrailTest` (the new lexer's and
  parser's cursor fields), `PlatformNamesGuardrailTest` (keyword arms; prefer `Set` and `Map` lookups).
- **`//parser-equivalence:diagnostics`** runs on every landing that touches a parser, lexer or protocol file; its
  `parser_parity` and `round_trip_proof` stay unchanged.

---

## 6. The harness

### 6.1 Files: `core/src/test/resources/iq-parity/`

| File | Contents |
|---|---|
| `schema.yaml` | the tables, in IQ types, seeded with edge values (NULLs, trailing blanks, mixed case, range limits). Typed columns, not constants, so SA's constant folding (§2.5) cannot change a type |
| `lite/*.yaml` | the cases: `id`, `sql`, optional `profile`, `engine`, `skip`; a provisional case adds `decision: D-xx` and `expect:` |
| `iq-answers.jsonl` | SA's answer per case and profile (§2.3) |
| `ledger.tsv` | per case: the DuckDB status, the Postgres status, and the judge (`sa:lite_iq`, `sa:lite_sa` or `D-xx`) |
| `syntax-cases.jsonl` | derived from the research: id, probe SQL, options, policy, SA's outcome. No rule text, no notes, and no probe that reproduces a manual's example. The derivation (re-run 2026-10-14) keeps only the research's own probe families and drops a case whose note or rule marks a documentation example, whose probe shares a run of six words with the manuals' text, or holds a name or a string of twelve characters or more that the manuals print |

### 6.2 The ledger and the lanes

- **`testcases/IqParity.java`** reuses `SqlParity`'s grid, comparison, statuses, ratchet, ledger I/O and YAML reader.
  It has its own `outcome()` (`SqlParity` writes `#SQL{` itself) and its own FROM rewrite, through `IqLexer`.
- **Statuses** as in `sql-parity`: PASS, FAIL (a wrong answer), BUG (answers what the judge rejects), ERROR (errs where
  the judge answers), REFUSED (by name), BOTH_ERROR, SKIP. A changed status fails the run (REGRESSION, FIX DETECTED,
  STATUS MISMATCH); `-Diq.parity.update` rewrites the ledger.
- **The lanes:** `//core:iq_parity_test` (DuckDB) and `//core:iq_parity_postgres_test` (embedded Postgres), both in
  `//gates:core`. The Postgres lane runs the translated Pure on Postgres; unlike `sql-parity`'s, it has no drift step,
  since its oracle is not Postgres.
- **The target:** 0 FAIL, 0 BUG, 0 ERROR in both lanes, always.

### 6.3 Guards

- **UNSNAPPED:** a case and profile with no answer in the file fails.
- **STALE:** an answer whose `sqlSha` does not match the case's SQL, profile and schema fails, so an edited case is
  never judged by its old answer.
- **GONE:** an answer whose case no longer exists fails.
- **Engine neutrality:** a case judged by `sa` with `engine=auto` over `tb()` sources is translated under both engines.
  If the two Pure texts differ, the rule is engine-dependent, and the case must either name its decision or pin
  `engine: sa`. This is the mechanical form of §2.4.
- **Crashers:** `snap.py` refuses to send a case matching the known-crashers list.
- **Evaluation order (D-E2, §4):** a judged case whose WHERE puts arithmetic or NULLIF over a column beside AND or OR
  fails the lane: its answer depends on IQ's plan, so it is skipped or rewritten.
- **An `engine: iq` pin needs a decision:** a case SQL Anywhere judges must translate alike under both engines' rules,
  which a pin to the IQ engine alone would hide.
- **Determinism (since review 7, §9.16):** a case whose answer may be one plan's choice fails the lane
  (`IqParity.determinism`): a TOP, FIRST or START AT without ORDER BY, or whose ORDER BY reaches no column unique in
  its source (each table's `id`, `sa_rowgenerator`'s `row_num`, a grouped query's keys), and a LIST without its own
  ORDER BY but of a GROUP BY key or a constant. Lite answers such a query with its database's choice, which is right
  for a user (any answer IQ could return is), but a case judged so would pin one plan. A case that tests a refusal or
  an error opts out with its reason (`nondeterministic:`) and must then not PASS.
- **The judge** compares a both-error's SQLCODE when legend-lite's error names IQ's (`(IQ: SQLCODE n, ...)`): another
  code than SQL Anywhere's is a FAIL. It compares a NUMERIC column's scale with each of lite's decimal cells.

### 6.4 The syntax-case ratchet

`IqSyntaxCasesTest` reads `syntax-cases.jsonl` and parses each probe under its options. The parser's verdict is
compared with the policy: accept parses; reject is a syntax error (or the named post-parse error); flag parses with a
warning under `engine=iq`; gated follows its option. The count of agreeing cases is ratcheted: it only goes up, and a
case that agreed and stops agreeing fails. Parsing runs ahead of translation: a form may parse long before it
translates, and until it translates it is refused by name.

---

## 7. Milestones and slices

The milestones are the research's (BUILD_PRIORITY), ordered by real use; the slices are this line's landings. Each
landing: `//gates:local`, the TeaVM builds, CI, an independent review, a GATES entry, this document's §9, `IN_FLIGHT`.

| Milestone | Content | Real queries covered (cumulative) |
|---|---|---|
| **M0** foundation | lexer, names, the option vector, profiles | a prerequisite |
| **M1** core dialect SELECT | 50 constructs (below) | 86.3% |
| **M2** Watcom and T-SQL library | LIST, DATEADD/DATEDIFF/DATEPART per engine, KEY JOIN, `*=`, CONVERT styles, ROUND/TRUNCNUM, CHARINDEX, ODBC escapes | 97.7% |
| **M3** windows, CTEs, rare clauses | OVER and the WINDOW clause, IQ's LAG/LEAD, WITH, LIKE `[set]`, NATURAL JOIN, APPLY | 100% |
| **M4** statement contexts | parse and refuse with a message: INTO, SET, DECLARE, procedural wrappers, DML | the contexts real queries arrive in |
| **M5** completeness | ROLLUP/CUBE/GROUPING SETS, INTERSECT/EXCEPT, PIVOT, MEDIAN and the statistics, hints, §13 lenient rewrites | no corpus use; the case set |

**The slices** (each construct with its count in the 3,208 hand-written queries):

| Slice | Milestone | Content |
|---|---|---|
| **PR1** seam | — | `InputDialect`, `PostgresDialect`: a pure move, judged as §5.1 says. No IQ behaviour |
| **PR2** pipeline | M0 | `#IQ{SELECT … FROM tb(…) WHERE … ORDER BY …}#` under `iq16-default`; `IqLexer` (the M0 productions), an `IqParser` skeleton, `IqDialect` (names, null order, messages), `IqProfile`; the plumbing (§5.3); the harness with about 40 cases; unit tests and a protocol round trip. Reaches `SELECT *` 323, ORDER BY 390 (by ordinal 41), quoted identifiers 378, `[bracket]` 36 |
| **PR3** rest of M0 | M0 | `alias = expr` 45, string aliases 39; TOP 73, FIRST 33, TOP … START AT (literal TOP 0 an error, PERCENT refused); IF … ENDIF 62; ISNULL 116 with IQ's unification; `+` 75 (string with number refused until PR4); `\|\|` 46; integer `/` 35 and `%` (literal widths, result widths per TYPING §3, checked ranges, truncation; decimal `/` refused); store types; `IqSyntaxCasesTest`; `lite/m0.yaml`, about 120 cases |
| **PR4** M1a | M1 | comparisons with IQ's conversions (string with number as NUMERIC, string with date as TIMESTAMP); BETWEEN 73, IN, CASE 146, IS [NOT] TRUE/FALSE/UNKNOWN; LIKE 125 without `[set]`; EXISTS 79; DUMMY 75 and `sa_rowgenerator` (a range, flattened, as `generate_series` is for Postgres); `lite/rules.yaml`, generated from the research's rule probes, `engine=sa` |
| **PR5** M1b | M1 | comma joins 381, outer joins 105 and CROSS JOIN 7, derived tables 204, subqueries 410, IN (subquery) 59, UNION 112 with IQ's union typing; NATURAL JOIN without merging |
| **PR6** M1c | M1 | GROUP BY 278 (names and ordinals), HAVING 61, DISTINCT 141; COUNT, SUM, AVG, MIN, MAX with IQ's types |
| **PR7** M1d | M1 | COALESCE 133, UPPER/LOWER 70 (after the native), CAST 69, CONVERT 63 without a style, SUBSTRING 62 (under `ansi_substring`), the TRIM family 62, LEFT/RIGHT 37, LENGTH and LEN 30, CHAR and ASCII 25, REPLACE 7; GETDATE 54 and NOW, compared with CURRENT TIMESTAMP |
| **PR8** M1e | M1 | owner-qualified tables 1,342 (after O2); `@var` 472, `:host` 520 and `?` 26 as Pure variables (after O3); the `iq16-odbc` and `iq16-tds` profiles; `IqDateTime` |
| **Then** | — | the natives (O1), announced to the ninth line; M2; M3; M4's messages; M5 |

**Caveats on the shares:**
- They are the research's, for a statement-level translator. An island is a value, so SELECT … INTO (23.0% of the
  queries, most of them PowerBuilder host variables) is refused by name inside one; the island's own share is lower
  until a statement front end exists (M4).
- The catalog track (SA catalog tables, `sp_iq*` and `sa_*` procedures, 20.3% with DUMMY) is not scheduled. In
  legend-lite it would be catalog views generated from the model, as the Postgres study proposes for `pg_catalog`
  (its §3.5). Until then, catalog names are refused by name.
- Re-measure after each milestone with the research's coverage script; its milestone lists are the contract.

---

## 8. Risks

- **SA is not the IQ engine.** It judges `engine=sa` cases and engine-neutral rules only. Every IQ-engine rule rests
  on the manuals and a provisional decision; any of them may be overturned by real IQ. Mitigation: the riskiest are
  refused (I4), each adopted one names its decision in the case, and the verification pack is ready.
- **The oracle is local.** CI never sees SA. Mitigation: the STALE, UNSNAPPED and GONE guards, and a recorded
  `--check` per landing. A machine without SA cannot add a case; that is by design.
- **Ownership and contention.** `protocol/sql` is the seventh line's and `sql/dialect` the ninth's. PR1 rewrites the
  heart of the seventh line's translator while its stress-corpus fixes are in flight
  (`claude/loving-euler-gjta59`). Mitigation: PR1 rebases after them, is a pure move judged by identical Pure, and the
  seventh line may carry the seam itself (`IN_FLIGHT`).
- **What lite cannot represent:**
  - Decimal is 38 digits on every backend; IQ's is 126. A wider value is the database's overflow error, never a
    wrong answer (as in the Postgres line's §9.3).
  - No unsigned integers, no TIME, no BINARY: refused by name.
  - An IQ database's text is ISO-8859-1; lite's is Unicode. Byte order agrees over Latin-1 letters but not beyond.
  - The store type is lost through derived tables and subqueries (§5.4).

  Each becomes a `SEMANTICS_REGISTER` row.
- **The size of the SQL.** Range checks and three-valued spellings add CASEs to every integer operation. The stress
  corpus already met a 96 KB `+` chain. Mitigation: check only what can overflow (the store type bounds the operand),
  and measure the generated SQL's size in the harness.
- **Statement-level SQL in an expression island** (INTO, variables, procedural wrappers) lowers the real-world share
  (§7). The island refuses it with a message; the statement front end is M4.
- **Copyright.** The research is full of SAP's text. Mitigation: I7; behaviour in our own words, ids and page numbers
  only; the syntax-case derivation drops any probe that matches a manual's text (§6.1); every landing's review checks
  the diff for quoted text.
- **TeaVM and native image.** New code in core must compile to WebAssembly and native image; the island's code paths
  run in the browser too.

---

## Upstream facts found while building

What SQL Anywhere 17 (and, for options, the user's real IQ 17.1) answered while the line was built (§9, cut), kept
here because §1–§8 do not say it or say otherwise. Each was measured on the oracle (§2) and snapped into a case under
`core/src/test/resources/iq-parity/`. "SA's front end" is the SQL Anywhere engine of §1.1; lite's deliberate
differences are register rows (S50 onward).

- **NUMERIC division** (SA's front end). The type follows TYPING §4, (p1+p2+1, s1+p2−s2+1), then the profile's cap. The
  value is the exact quotient cut to ceil((p+1)/2) base-100 digits from its first nonzero digit, then rounded half
  away from zero at s; so it is truncated when p is odd, s even and the quotient fills all p−s whole digits
  (NUMERIC(1,0) `5/3` is 1.66, `2/3` is .67). SA rounds only where it shows the value: as an operand, a CAST, a
  comparison or a GROUP BY, HAVING or ORDER BY key it reads the cut value unrounded (`(10.0/3)*3` is 10.00 and
  `10.0/3 = 3.33` is false). (Measured over 3,400 quotients.)
- **Aggregate types** (SA's front end). SUM of SMALLINT or INTEGER is INTEGER, of BIGINT BIGINT, of BIT NUMERIC(18,0),
  of DOUBLE DOUBLE, of NUMERIC(p,s) NUMERIC(p+8,s). AVG of NUMERIC(p,s) is SUM divided by COUNT under the division
  rule, at NUMERIC(p+19,s+11); AVG of an integer is NUMERIC(49,21) (129 AVGs measured). MIN and MAX keep the
  argument's type. MEDIAN of SMALLINT or INTEGER keeps the type and floors the mean of the middle values (−1 and −4 give
  −3); MEDIAN of BIT, a date or NULL is -156. LIST skips NULL and `''` but keeps blanks, its default delimiter is `,`,
  and over no rows it is `''`.
- **`ansinull` Off makes some comparisons two-valued.** In a search condition (WHERE, HAVING, ON, a subquery's WHERE),
  `x = @v`, `<>` and `!=` give FALSE for a NULL x when x is a column, a constant, arithmetic, `||` or a sign over those,
  a CAST or CONVERT of one, or a derived column defined by one; so `NOT (x = @v)` keeps the NULL rows. They stay
  three-valued when x is a function's value, an aggregate in HAVING or a scalar subquery, and `<`, `<=`, `>`, `>=`,
  IN, NOT IN, BETWEEN, LIKE and any comparison outside a search condition always are. A literal `= NULL` becomes
  IS NULL in ON, HAVING, a comma join's WHERE and a subquery's or derived table's WHERE, in both engines; a NULL that
  is not a literal (`CAST(NULL AS INT)`, `NULL + 1`) stays unknown. (About 70 forms, under iq16-tds and sqla17-tds.)
- **GROUP BY and names** (corrects §7's "GROUP BY (names and ordinals)"). An integer in GROUP BY is a constant, never a
  position (`SELECT k, COUNT(*) … GROUP BY 1` is -149), and a constant beside another key is dropped. A select-list
  alias is read before a column of the same name in WHERE, ON, later items, GROUP BY, HAVING and inside ORDER BY
  expressions; a qualified name is never an alias; in a subquery the order is its own columns, then the enclosing
  query's aliases, then its columns. An ungrouped column is -149 (-854 in ORDER BY).
- **An unaliased item's name is SA's rendering, not the text written** (corrects §5.1's "the expression's text").
  Columns are qualified by the correlation name (`id+1` over `t a` is `a.id+1`); outer parentheses and a leading `+`
  are dropped; `+ - * /` are written tight; `%` becomes `remainder(a,b)` and `!=` `<>`; constants are written by value
  (`1.50` is `1.5`, `0.5` is `.5`, `1e3` is `1000.0`); keywords are lower case and `COUNT(*)` is `COUNT()`; the name
  is cut at 255 bytes. (138 probe statements and about 2,900 generated items.)
- **Where the IQ engine allows a subquery.** A scalar subquery only as a whole select item or a WHERE comparison's
  operand; EXISTS, IN and ANY only as WHERE predicates (not inside CASE, IF, a function or arithmetic, not in ORDER BY,
  ON or a derived table's select list); no set operation inside a subquery. SA's front end accepts all of these.
  (GRAMMAR E15, appendix B1.)
- **Dates read from strings** go by DATE_ORDER (`03/04/05` is 2003-04-05 under YMD, 2005-03-04 under MDY). A leading
  four-digit year reads under every order; `yyyymmdd`, `yymmdd` and month names read; IQ takes any prefix of a month
  name (`Ma` is March). A two-digit year goes by NEAREST_CENTURY 50 (49 is 2049, 50 is 1950). The fraction may follow
  `.`, `,` or `:` and is cut past six places; `24:00:00` is the next day's midnight. A string compared with a DATE
  matches only at midnight. (1,218 strings, orders and types.)
- **Strings.** TRIM, LTRIM and RTRIM drop only the space and 0x09–0x0D, in UTF-8 databases too. SUBSTRING with
  `ansi_substring` On counts virtual positions below 1 (`('mnopqrst', -3, 5)` is `'m'`) and a negative length is
  -1090; with it Off a negative start counts from the end. LEFT and RIGHT of a negative count are `''`. A search
  string of 255 characters or more gives NULL. ASCII and CHAR are NULL outside 0..255.
- **Conversions.** A string compared with a NUMERIC(p,s) column is converted to that NUMERIC, rounded to s places
  (against NUMERIC(10,2), `'0.005'` is 0.01); against an integer column it is read at the database's PRECISION and
  SCALE. A too-long string cast to CHAR is -638 unless the rest is spaces; a too-long integer cast to text is -157.
- **Setting options** (SA 17 and IQ 17.1). On and Off take any case, never 1 or `'yes'`; anything else is -201. An
  integer option reads leading blanks, at most one `+`, then digits (`' +60'` is 60, `''` is 0, `'60.0'` is -201): a
  fraction is -201 on both SA 17 and IQ 17.1, which refutes the research's D-Q2 ("IQ cuts a fraction"). The first level
  set wins: connection temporary, user, temporary PUBLIC, PUBLIC. On IQ 17.1, `round_to_even` is -202, `time_zone`
  -1924, and `SET TRANSACTION ISOLATION LEVEL READ UNCOMMITTED` is accepted where SA raises -676.
- **More SA 17 defects** (beside §2.5's). DATEADD wraps a huge count instead of raising (DAY 1,500,000,000 gives
  2598-05-12). MEDIAN computes the midpoint in INTEGER and wraps. `ESCAPE '%'` (or `_`, `[`) is read as the wildcard,
  so `'%%' ESCAPE '%'` matches every string. ROUND of a DOUBLE rounds its decimal digits (`ROUND(2.675e0, 2)` is
  2.68). In SA's front end SQRT and LN are NULL outside their domain, and with `divide_by_zero_error` Off a division
  by zero is NULL.

> Cut in the 2026-11 docs cleanup: §9, the steps landed 9.1–9.17 (PR1 to PR8, the natives, the merges, reviews 3 to 8 and the settings slice, 2026-10-13 to 2026-10-14); its divergences are register rows (S50 onward), its cases are pinned under `core/src/test/resources/iq-parity/`, and the facts it measured are listed above. Full text: `git show de2745351:docs/IQ_SQL_STUDY_2026_10_13.md`.
