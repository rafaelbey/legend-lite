# DuckDB parity: the renderer without its passes (2026-11-03)

Step 1 of the DuckDB slice (docs/rust-port/PHASE1.md §4, step 3): the shared renderer (`AnsiSqlRenderer` as a trait),
the writer, DuckDB's own rendering, and the pass pipeline with every pass a named stub (`src/passes.rs`). This file
measures how far that gets on the golden dump, and what each remaining difference is waiting for.

Measured by `tests/duckdb_differential.rs` over `//rust/oracles:mir_dump` (`mir/mir-dump.jsonl`): every line whose
dialect is `DuckDb`, rendered by a fresh `DuckDb` and compared with the fresh JVM `DuckDb`'s answer. The SQL text is
compared exactly, a refusal by its Java class and message, and a statement's binds and regions (for now) by whether
they are empty.

```
cd rust
LITE_MIR_DUMP=<the dump> cargo test --release -p lite-dialect --test duckdb_differential -- --ignored --nocapture
```

## The numbers

**15,602 of 16,154 statements identical (96.6 %)**; the floor (`tests/duckdb-floor.txt`) is 15,602.

| op | identical | different | refused (Java rendered) | accepted (Java refused) |
|---|---:|---:|---:|---:|
| `ddl.render` | 1,346 | 0 | 0 | 0 |
| `dml.render` | 758 | 15 | 0 | 0 |
| `dml.renderStatement` | 758 | 15 | 0 | 0 |
| `query.render` | 949 | 65 | 9 | 0 |
| `query.renderStatement` | 11,791 | 384 | 64 | 0 |

- **Refusals**: all 10 of Java's DuckDB refusals are reproduced exactly, class and message: the 8
  `IllegalStateException`s of a `CREATE TEMPORARY TABLE ... AS` that binds a parameter asked for as text, and the 2
  `DialectCapability` refusals of a plan parameter in a DML row, each message embedding Java's `Bind[...]` record text.
  Rust never renders what Java refused. It refuses 73 statements Java rendered: 72 folds and one checked-defects
  envelope, both pass work (below).
- **Binds and regions**: of the identical statements, the 72 that bind a parameter or write a per-key region carry
  Java's record text exactly (`Bind[parameter=age, binding=One[nullType=BIGINT, hole=null]]`, `Region[start=21, end=49,
  lead=, , ...]`), UTF-16 positions included -- stricter than the emptiness the test asserts today.
- **By kind**: every DDL statement, `InsertValues`, `InsertFromTable`, `InsertSelect`, `Update` and `DeleteWhere` is
  identical. The differences are in `SqlSelect` (184), `SqlWith` (335), `SqlUnion` (3), `CreateTempTableAs` (26) and
  `Merge` (4), all in queries the passes rewrite.

## The 552 differences, by cause

Each difference is assigned to the pass whose signature the **first** difference carries (the refusal Rust writes in
Java's place, or the text at the first differing byte), else to the first stubbed pass, in pipeline order, whose
trigger the statement holds (`cause` in the test). Every one of the 552 is explained; none is a defect of the ported
renderer. A statement that needs two passes is counted once, under the one whose difference comes first, so porting a
pass can uncover a second difference in a few of them.

| cause | statements | where | what Java's text has that Rust's lacks |
|---|---:|---|---|
| `FloatCanonText.Expansion`, then `SubstringClamp` over it | 315 | 300 `renderStatement SqlWith`, 15 `render SqlSelect` | the start of the canon's two `substr` calls clamped: `greatest(CASE WHEN strpos(...) ... END, 1)` |
| `UnqualifyPivotArgs` | 75 | `SqlSelect` 59, `SqlWith` 16 | a PIVOT's USING arguments unqualified: `USING SUM(qty)` where Rust writes `SUM(t4.qty)` |
| `FoldToListReduce` | 72 | `SqlSelect` 55, `SqlWith` 17 | `list_reduce(...)`, where Rust refuses: `fold reached a dialect without a fold encoding` |
| `SubstringClamp` | 36 | all 30 DML differences (`CreateTempTableAs` 26, `Merge` 4) and 6 queries | a literal start clamped (`substr(t0.CODE, 1, 2)` for `0`), a computed one wrapped (`greatest(strpos(...) + 1, 1)`) |
| `VariantEgress` | 28 | root `SqlSelect` 25, `SqlUnion` 3 | a root star over a stored Variant column spelled out: `SELECT t0.ID, CAST(t0.V AS JSON) AS V, ...` for `SELECT *` |
| `QuantileOrder` | 18 | `renderStatement` | a percentile's order spelled: `list_extract(list_sort(LIST(v)), greatest(CAST(CAST(ceil(...` for `QUANTILE_DISC(v, 0.6)` |
| `StoredReads` | 7 | `render SqlSelect` 6, `renderStatement` 1 | an `OTHER`/`DISTINCT` column read as text: `CAST(t0.ADDR AS VARCHAR) AS addr` |
| `CheckedDefectsToLists` | 1 | `renderStatement SqlSelect` | the defects envelope as list lambdas, where Rust refuses: `nested checked defects reached a dialect without list lambdas` |

Notes on the causes:

- **The Float canon (315)**. The renderer's `FLOAT_CANON` arm (`floatCanonByText`) is ported and writes the very tree
  `FloatCanonText.Expansion` builds on DuckDB (`bindsFloatText` is false), so the canon's own text is right. What is
  missing is the order: Java expands the call in the *first* pass, so `SubstringClamp`, five passes later, clamps the
  two `SUBSTRING` calls of the expansion's fixed-point unfold (every byte before the first `greatest(` is Java's).
  The expansion and the clamp have to land together for these 315: the clamp alone never sees the canon's calls. `FLOAT_PRINT` (`floatPrintByText`) is the same, but its tree has no
  `SUBSTRING`, and no difference is attributed to it.
- **The DML differences (30)** are the clamp in the statement's query: `CREATE TEMPORARY TABLE ... AS` and `MERGE ...
  USING (...)` run the dialect's passes over their query (`passed`), as a query's render does.
- **`VariantEgress` and `StoredReads` overlap**: a star over a table holding both an `OTHER` column and a stored Variant
  (line 3088 of the dump: `SELECT t0.ID, t0.NAME, CAST(t0.ADDR AS VARCHAR) AS ADDR, ..., CAST(t0.META AS JSON) AS META`)
  needs both.
- **No first difference is attributed to `CarrierStrategies`, `ConstantKeysAsExpressions` or
  `OuterJoinSubqueryFilters`**, nor to `FloatPrintText.Expansion`: on this dump their rewrites either do not fire or
  produce what the renderer already writes. A difference of theirs may still surface behind one of the above.

## What the next steps port, by payoff

1. `SubstringClamp` with `FloatCanonText.Expansion` (and `FloatPrintText.Expansion`, the same shape): 351.
2. `UnqualifyPivotArgs`: 75.
3. `FoldToListReduce`: 72 (all of today's wrong refusals but one).
4. `VariantEgress` and `StoredReads` (with Java's `Stars`, which both spell a star with): 35.
5. `QuantileOrder`: 18.
6. `CheckedDefectsToLists`: 1.
7. `CarrierStrategies`, `ConstantKeysAsExpressions`, `OuterJoinSubqueryFilters`: no measured difference; ported for
   fidelity, held by the probe's differential.

The shared walk (`src/rewriter.rs`, Java's `SqlRewriter`) is ported; a pass overrides its hooks. Java rebuilds a node
whose child changed through its record constructor, which recomputes the node's type fact (`SqlTyping.callType`,
`caseType`, `castType`, ...) and a join's outputs; the walk here keeps a node's fact as it was, so a pass whose output
reaches a type-reading arm (`roundHalfEven`'s decimal test, a struct field's `Bottom` test, `storedVariant`) will need
the matching part of `SqlTyping`.

## What Java does that this does not reproduce exactly

None of these reaches the dump; each is a refusal's wording or an input the corpora do not hold.

- A `ClassCastException`'s message names the classes as `com.legend.sql.SqlExpr$X` without the JVM's module suffix
  (`... are in unnamed module of loader 'app'`).
- A message that embeds a whole MIR node (`part()`'s "a date call's first argument is its part, not <node>", a pass's
  stored-read column) writes the node's simple name, not Java's full record `toString`.
- An index past the end is refused in `ArrayList`'s words (`Index 1 out of bounds for length 1`); Java's two-element
  `List.of` words it `Index: 1 Size: 1`.
- `DuckDb.structGet`'s positional-field test is ASCII digits; Java's `Character.isDigit` takes every Unicode digit.
- `ddlIdentifier`'s `toUpperCase(Locale.ROOT)` is Rust's `to_uppercase`: the same special casing, but each from its own
  Unicode version.
- `sourceUrl`'s `Path.of(URI.create(url))` is a hand-written POSIX reading (the path's escapes decoded, repeated and
  trailing slashes dropped); its refusals of an authority, a query, a fragment or an opaque URI keep Java's
  `IllegalArgumentException` and wording, but not `URI.create`'s syntax errors.
- `RawSqlBoundary.translated`'s regular expressions are hand-written matchers, Java's semantics (ASCII `(?i)`, `\w`,
  `\s` and `\b`); a `CREATE TABLE ... (` at the very end of the text, which Java refuses with a
  `StringIndexOutOfBoundsException`, is written as it came.
- A `SqlExpr.Tagged`/`TaggedList` reaching the renderer (Java's `tagged` hook and its refusal): the Rust MIR has no such
  node; MirJson refuses it before a dialect sees it.
- Not ported, as no DuckDB render reaches them: `starsAsSourceColumns` (MariaDB, MySQL and BigQuery's), and DuckDB's
  non-rendering surface (its catalog types and rules, host operations, session setup, scripts, `copySelectList`).

## Stack

The deepest statement of the dump needs between 1 and 2 MB of stack in a debug build for its parse, read and render
together, and less than 1 MB in release (`LITE_DIALECT_STACK_MB` sets the test thread's; it defaults to 256 MB). The
test runs in 9.5 s in debug and 1.3 s in release.
