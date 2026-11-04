# Dialect parity: DuckDB, H2, H2Modern, the engine-text dialects, Postgres, SQLite, Trino, Databricks, ClickHouse and
# BigQuery (2026-11-04)

Every dialect Rust answers, held to the JVM's own answers on the golden oracles (docs/rust-port/PHASE1.md §4): DuckDB
(step 3, its renderer and passes), the H2 family's execution dialects, H2 and H2Modern (step 4b), the engine-text
dialects, EngineStyleH2, EngineStyleDB2 and EngineStyleComposite (step 4c), Postgres (step 5b), SQLite (step 5d), Trino
and Databricks (step 6b), ClickHouse and BigQuery (step 6c).
Each is ported from
its Java class, over a walk that keeps Java's identity semantics and a port of the typing rules Java's constructors
apply. This file measures each on the oracles and says what of Java it does not reproduce exactly.
A `Character` predicate a dialect's Java calls (`isDigit`, `isLetterOrDigit`, `isWhitespace`) is the build JDK's own
class of the UTF-16 unit Java's `charAt` gives it (`lite_javacompat::character`, walked over all 1,114,112 code
points against `oracles/java-compat/extras/char-classes.tsv` by `//rust/lite-javacompat:char_classes_test`).

Measured by `tests/dialect_differential.rs` over `//rust/oracles:mir_dump` (`mir/mir-dump.jsonl`),
`//rust/oracles:h2_dump` (`mir/h2-dump.jsonl`), `//rust/oracles:dialects_dump` (`mir/dialects-dump.jsonl`, the ten Java
dialects left after the H2 family's switch), `//rust/oracles:sqlite_arms` (`mir/sqlite-arms.jsonl`, the SQLite arms no
lane reaches, rendered by Java's own SQLite), `//rust/oracles:dialect_arms` (`mir/dialect-arms.jsonl`, the eight dialects
left to port over inputs that reach every arm each decides, rendered by Java's own dialects) and
`mir/tagged-engine.jsonl` (the one engine-text query holding a tagged value, as the shim sends it), an input more than one holds (the same dialect, op, configuration and MirJson; an
engine-text dialect's default configuration and the first dump's none are one) counted once:

- `<dialect>_renders_as_java_does` (`duckdb_`, `h2_`, `h2modern_`, `enginestyleh2_`, `enginestyledb2_`,
  `enginestylecomposite_`, `postgres_`, `sqlite_`, `trino_`, `databricks_`, `clickhouse_`, `bigquery_`): every
  distinct input of the dialect, answered by a fresh Rust dialect of that
  name and configuration (made by the seam's `open`, as the JVM's shim makes it) and compared with the fresh JVM
  dialect's answer -- the SQL text exactly, a statement's binds and regions as Java's record text, a refusal by its Java
  class and message, an engine-text op's string (or a select list's null); at least the dialect's floor
  (`tests/<dialect>-floor.txt`) identical;
- `typing_reproduces_javas_facts`: every node of every distinct statement of the oracles (all dialects: one tree), its
  type fact recomputed by `src/typing.rs` from its children's and compared with the fact Java's constructor stored;
  every projection frame's outputs, union's labels and join's delivered columns likewise;
- `the_seam_answers_as_<dialect>s_renderer_does`: every op of the dialect's inputs answered through the seam
  (`SEAM.md`) exactly as a direct call answers it (an engine-text dialect's, each alias of each rendered query too, as
  `renderedAlias` after the render).

```
cd rust
LITE_ORACLES="<mir-dump.jsonl> <h2-dump.jsonl> $PWD/oracles/mir/tagged-engine.jsonl <dialects-dump.jsonl> <sqlite-arms.jsonl> <dialect-arms.jsonl>" \
  cargo test --release -p lite-dialect --test dialect_differential -- --ignored --nocapture
```

(`LITE_ORACLES` names the oracle files, separated by whitespace; `LITE_DIALECT_MISMATCHES=<dir>` writes each dialect's
every mismatch to `<dir>/<dialect>.txt`.)

# DuckDB

## The numbers

**16,154 of 16,154 statements identical (100 %)**; the floor (`tests/duckdb-floor.txt`) is 16,154. Step 1 (every pass
a stub) was 15,602.

| op | identical | different | refused (Java rendered) | accepted (Java refused) |
|---|---:|---:|---:|---:|
| `ddl.render` | 1,346 | 0 | 0 | 0 |
| `dml.render` | 773 | 0 | 0 | 0 |
| `dml.renderStatement` | 773 | 0 | 0 | 0 |
| `query.render` | 1,023 | 0 | 0 | 0 |
| `query.renderStatement` | 12,239 | 0 | 0 | 0 |

By MIR kind every one is identical: `SqlSelect` 11,183, `SqlWith` 2,056, `SqlUnion` 23, `CreateTable` 859,
`InsertValues` 840, `DropTable` 479, `InsertFromTable` 448, `CreateTempTableAs` 90, `InsertSelect` 88, `DeleteWhere` 38,
`Merge` 26, `Update` 16, `CreateSchema` 8. All 10 of Java's DuckDB refusals are reproduced, class and message, and Rust
refuses nothing Java rendered (step 1's 73 wrong refusals -- 72 folds, one checked-defects envelope -- are now Java's
`list_reduce` and list-lambda spellings). The 72 statements that bind or write a region carry Java's record text
exactly. (Since step 4b a statement is identical only with its binds and regions Java's record text; DuckDB's count did
not move.)

**Typing, over the first dump's every line: 1,494,436 facts of 26,095 statements, 0 different** -- every `Call` rule the
dump exercises (165 functions),
`Case` (24,368), `Cast` (35,557), the reducers and windows, the decimal and float literals, `ScalarSubquery`,
`StructGet`/`StructLit`, `FoldCall`, and 60,597 select outputs, 22,594 join outputs, 365 union labels, 8 star frames'
pad neutralizations, 192,143 projection slots' origin normalization.

The step 1 differences, by the pass that closed them: the Float canon's expansion with the substring clamp over it
(315), `UnqualifyPivotArgs` (75), `FoldToListReduce` (72), `SubstringClamp` alone (36, all 30 DML differences among
them), `VariantEgress` (28), `QuantileOrder` (18), `StoredReads` (7), `CheckedDefectsToLists` (1).

## What is ported

- **The passes** (`src/passes.rs`, `src/carrier.rs`, `src/scan_order.rs`), in Java's order (`ansi::render_passes`,
  `passes::duckdb_passes`): `FloatPrintText.Expansion`, `FloatCanonText.Expansion` (their binding forms too, which
  DuckDB does not use), `CarrierStrategies` (the whole class, over any `Caps`; identity on DuckDB's), `QuantileOrder`,
  `UnqualifyPivotArgs`, `FoldToListReduce`, `CheckedDefectsToLists`, `SubstringClamp`,
  `ConstantKeysAsExpressions` (both modes), `OuterJoinSubqueryFilters`, `StableScanOrder` with `ScanOrder` (under
  `DuckDb::engine_scan_order`), `VariantEgress`, `StoredReads`, and `Stars`. `ExplosionMarkers` and
  `FoldsAsRecursiveCtes` are not reached by any DuckDB pass (they are other dialects' passes) and wait for theirs.
- **The typing** (`src/typing.rs`, Java's `SqlTyping`): `callType` (every kind rule and nullability arm), `caseType`,
  `castType`, `membershipType`, `reduceCollectionType`, `reducerType`, `arrayLitType`, `structLitType`,
  `structGetType`, `scalarSubqueryType` with its one-row proof, `checkedOneType`, `foldType`, `windowType`,
  `decimalLitType` (over BigDecimal's text: precision, scale, `setScale(0)`, `bitLength`), the decimal and numeric
  promotions, `uniform`/`branchPromote`, `slotNullable`, `reconcileSlot`, `reconcileUnionLabels`,
  `wherePadNeutralized`; and `SqlExpr.children()`, `SqlAgg.aggregatesSelect`, `SqlQuery`/`SqlSource` `outputs()` and
  `alias()`, `Join.delivered`. Not ported: `taggedType`/`taggedListType` (no `Tagged` node in the Rust MIR),
  `subsumes` and `whereNullRejections` (no dialect calls them).
- **The constructors** (`src/build.rs`): every MIR record's canonical constructor as Java writes it, each computing its
  fact through `typing` -- `Call`, `Case`, `Cast`, `Group`, `ArrayLit` (with and without a given type), `StructGet`
  (likewise), `StructLit`, `Membership`, `ReduceCollection`, `OrderedListAgg`, `Reducer`, `ScalarSubquery`,
  `CheckedOne`, `CompactList`, `WindowCall`, `FoldCall`, the literals; `SqlSelect` (outputs rebuilt from the
  projections, a star frame's pads neutralized), `Projection` (an attached output normalized to `DERIVED`), `SqlUnion`
  (labels reconciled, two branches at least), `SqlWith`/`prepend`, `Join` (its ON kind-coupled, its columns
  delivered), and the `with*` copiers. The renderers' own built trees go through them too.

## Identity: where Java's `==` mattered

Java's walk (`SqlRewriter`) hands a node on as it is when no child changed (`==`) and rebuilds it through its record
constructor when one did; the constructor recomputes the node's fact, a select's outputs, a union's labels, a join's
columns. Rust has no identity, so every step answers `Rw::Same` (Java's `return e`) or `Rw::New` (anything else, an
equal copy included), and the walk rebuilds through `build` exactly where Java would. It matters in the output: a
rebuilt `ArrayLit` or `StructGet` loses the type it was given (the walk uses their one-type-less constructors), a
rebuilt `Lambda`'s fact becomes `UNKNOWN`, a rebuilt select's outputs adopt its projections' facts -- each pinned by a
unit test (`src/pass_tests.rs`). The passes' own identity tests, each reproduced:

- `SubstringClamp`: `clamped != st` -- a literal start of 1 or more leaves the call (and so its parents) as it came;
- `ConstantKeysAsExpressions`: `k != e`, the select rebuilt only when a key was cast;
- `OuterJoinSubqueryFilters`: `from == s.from()` through its join tree;
- `StoredReads`: a statement over no read table is returned as it came, a select whose stars reach no read table too;
- `VariantEgress`: `any ? withProjections(ps) : s`, root only; its union is always rebuilt (`new SqlUnion`), so its
  labels re-reconcile;
- `UnqualifyPivotArgs`: every pivot is rebuilt, so a select over one always recomputes its outputs;
- `FloatCanonText.Expansion` and `FloatPrintText.Expansion` (binding forms only, not DuckDB's): an equal call is looked
  up by record equality (a `HashMap`) and Java hands back the very node it built before, shared in the tree; Rust
  hands back a copy. The text is the same; only a later pass keyed by identity could tell, and none runs on DuckDB.
- `StableScanOrder` and `ScanOrder`: `stabilize` answers `Same` exactly where Java returns its input (its caller's
  `st == inner`), `walkFrom` where its source comes back (`l == left && r == right` for a join), the select hook where
  it returns `s`, `baseTableReducers` where its lazily copied list stays unmade; a firing is counted where the output
  differs from the input by `equals` (Rust's `PartialEq`). Its maps (`cteOrds`, `frames`, the frames needing order) are
  only looked up.
- `CarrierStrategies` (`src/carrier.rs`, every rule and helper over any `Caps`; on DuckDB's every hook answers `Same`):
  hooks answer `Same` exactly where Java returns its input; `substParam`, `remapAlias` and `literalNullTests` rebuild a
  node only where a child changed, through `carrier::with_children` (Java's `SqlExpr.withChildren`, which -- unlike
  the walk's constructors -- keeps an array literal's and a field read's given type and a lambda's own fact). Its JSON
  folds read literal JSON through `src/json.rs` (Java's `com.legend.sql.Json`: `parse`, `unescapeString`,
  `canonical`, `BigDecimal.toString`): an object keeps insertion order with a repeated key replaced in place, and that
  order reaches the folded text. `LIST_REDUCERS` is only looked up.

Map order reaching output: `StoredReads`' PIVOT refusal prints a table's read columns as Java's `HashMap` key set;
`java::hash_map_order` models `HashMap<String, _>` iteration (String.hashCode, the spread, the table doubling past
three quarters, a bucket's insertion order). Every other map or set of these passes is only looked up.

## What Java does that this does not reproduce exactly

None of these reaches the dump; each is a refusal's wording, an input the corpora do not hold, or a pass DuckDB does
not run.

- A `ClassCastException`'s message names the classes as `com.legend.sql.SqlExpr$X` without the JVM's module suffix
  (`... are in unnamed module of loader 'app'`); a reducer cast where it is no `SqlExpr` record is named
  `SqlExpr$Reducer` (Java's class is `com.legend.sql.SqlAgg$Reducer`), and a `DeferredTdsString` over a union names
  `SqlExpr$SqlUnion`. (The walk's pivot-USING cast names `com.legend.sql.SqlAgg$Reducer` as Java does, since step 6b.)
- A message that embeds a whole MIR node (`part()`'s "a date call's first argument is its part, not <node>", the
  walk's "a pass rewrote a stored read's column into <node>") writes the node's simple name, not Java's full record
  `toString`.
- An index past the end is refused in `ArrayList`'s words (`Index 1 out of bounds for length 1`); Java's two-element
  `List.of` words it `Index: 1 Size: 1`.
- The Float expansions' equal-call lookup compares a double by its text and value, so a NaN-valued call never matches
  an earlier one; Java's record equality (`Double.compare`) finds it.
- `java::hash_map_order` does not model a bucket of eight or more keys, which Java turns into a tree.
- `CarrierStrategies` (not run on DuckDB): a refusal that embeds a node (the FULL OUTER sort key's), and a pivot
  value's generated column name over a non-literal node, write a short name where Java writes the record's whole text
  (literal values print Java's exact text); `json.rs`'s refusals of a malformed number, a cut-off string (Java's
  `StringIndexOutOfBoundsException`, worded differently across JDKs) and an index past `outputs` are close to Java's
  words but not exact; a lone surrogate written `\uXXXX` becomes U+FFFD (a Rust string cannot hold it).
- `StableScanOrder`'s firing count is per pass object (Java's is a static `AtomicLong`), and `ScanOrder`'s one-argument
  `stabilize(q)` overload (no frames) is the two-argument one over an empty set; the dump never sets the flag, so the
  pass is held by its unit tests alone.
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
- Not ported, as no DuckDB render reaches it: `starsAsSourceColumns` (MariaDB, MySQL and BigQuery's). DuckDB's
  non-rendering surface -- the rest of `SqlDialect` (its catalog types and rules, host operations, session setup,
  scripts, `copySelectList`, the facts) -- is ported with the seam the JVM calls it through (`SEAM.md`, whose §6 says
  what of it is not exact).

# H2 and H2Modern

Step 4b (docs/rust-port/PHASE1.md §4): Java's `H2` and `H2Modern` (`src/h2.rs`) and every pass `H2.passes()` installs
(`src/h2_passes.rs`), each ported from its Java class.

## The numbers

**H2: 21,847 of 21,847 distinct statements identical (100 %); H2Modern: 394 of 394 (100 %).** The floors
(`tests/h2-floor.txt`, `tests/h2modern-floor.txt`) are 21,847 and 394. The oracles hold 29,379 H2 lines (21,453 of the
H2 dump, 7,926 of the first) and 763 H2Modern lines (386 and 377); an input both hold is one statement, and none has two
answers. The SQL, the binds and the regions are compared exactly: 59 H2 and 8 H2Modern statements bind a parameter
(type holes among them) and carry Java's record text. Every one of Java's refusals is reproduced, class and message --
H2's 939 (931 `DialectCapability`, 8 `IllegalStateException`), H2Modern's 19 (17 and 2) -- and Rust refuses nothing Java
rendered.

| H2 op | identical | different | refused (Java rendered) | accepted (Java refused) |
|---|---:|---:|---:|---:|
| `ddl.render` | 1,295 | 0 | 0 | 0 |
| `dml.render` | 820 | 0 | 0 | 0 |
| `dml.renderStatement` | 395 | 0 | 0 | 0 |
| `fromEngineText` | 2,190 | 0 | 0 | 0 |
| `query.render` | 1,813 | 0 | 0 | 0 |
| `query.renderStatement` | 15,334 | 0 | 0 | 0 |

| H2Modern op | identical | different | refused (Java rendered) | accepted (Java refused) |
|---|---:|---:|---:|---:|
| `dml.render` | 9 | 0 | 0 | 0 |
| `dml.renderStatement` | 9 | 0 | 0 | 0 |
| `query.render` | 17 | 0 | 0 | 0 |
| `query.renderStatement` | 359 | 0 | 0 | 0 |

By MIR kind every one is identical: H2's `SqlSelect` 13,544, `SqlWith` 3,565, `String` (the engine text) 2,190,
`InsertValues` 861, `DropTable` 664, `CreateTable` 618, `CreateTempTableAs` 187, `InsertSelect` 86, `DeleteWhere` 40,
`SqlUnion` 38, `Merge` 26, `Update` 14, `CreateSchema` 13, `DeleteAll` 1; H2Modern's `SqlSelect` 373,
`CreateTempTableAs` 6, `Update` 4, `SqlUnion` 3, `DeleteWhere`, `InsertSelect`, `InsertValues` and `Merge` 2 each.

The passes, measured by adding each in `H2.passes()`'s order (and, in brackets, the statements that differ with it alone
left out):

| passes | H2 identical | H2Modern identical |
|---|---:|---:|
| `CarrierStrategies` alone (the renderer and its hooks all ported) | 19,338 | 390 |
| + `ConstantLateralsAsRows` [4 and 4] | 19,342 | 394 |
| + `LateralExplodeToUnion` [16] | 19,358 | 394 |
| + `AggregatesDeliverDouble` (AVG) [104] | 19,462 | 394 |
| + `SourceSpelling` [2,385] | 21,847 | 394 |

The seam answers every op of them as a direct call does: H2's 21,847 ops and 1,813 inline renders (14,962 statements
read back from their JSON equal and with Java's record text, 1,103 refusals), H2Modern's 394 and 17 (352 statements, 20
refusals).

## What is ported

- **The dialect** (`src/h2.rs`): every one of `H2.java`'s 45 overrides -- the scalar and DDL type tables, the passes,
  `failingStatement`, the schema-qualified table name, the star's EXCEPT and its names, the `information_schema` catalog
  with H2's type names and codes, the local transactional staged table, the alias and reference rules (a derived label
  quoted always, a physical name as its DDL spelled it, an origin-less reference refused), the typed placeholders and
  type holes, `forServer` and the facts, native QUALIFY, the whole `call` table, the frame bounds, the JSON carrier's
  literals and aggregates, the java.time date format, the aggregates (MIN for `first()`, the WITHIN GROUP percentiles,
  `JSON_ARRAYAGG` lists), banker's ROUND, the JSON-parsing and boolean-text casts, splitPart, the JSON kind and TYPEOF
  names, the standard store writes -- and `H2Modern.java`'s 10: H2Modern is the same struct with `modern` set, its own
  overrides (`serverVersions` refused, `forServer`, `variantGet`, `structLit`, `structGet`, `call`'s LIST_LENGTH,
  `variantAwareCast`'s navigation cast) each branch on it, and its `scalarTypeName`, `formatText` and `reducer` are H2's
  (Java's are exhaustive switches delegating every constant to `super`). The refusals Java words with the literal "H2"
  (a TYPEOF, a placeholder, an origin-less column) keep it on H2Modern; the interface's defaults name the dialect
  (`getClass().getSimpleName()`), so H2Modern's say "H2Modern".
- **The passes** (`src/h2_passes.rs`), in `H2.passes()`'s order (`h2_passes`): `CarrierStrategies` under H2's caps (no
  native lists, no correlated explosion, no JSON carrier, no native FULL OUTER: `src/carrier.rs`, ported with DuckDB and
  first exercised here), `QualifyToSubselect` where QUALIFY is not native (never on H2; its plain form, `new
  QualifyToSubselect()`, ported and unit-tested; the by-outputs form is Postgres's), `ConstantLateralsAsRows`,
  `LateralExplodeToUnion`, `AggregatesDeliverDouble` over AVG, and `SourceSpelling` last. `render_passes` runs the Float
  expansions before them and `StoredReads` after, as for DuckDB.
- **The shared renderer**, three changes the port needed: `column_name`, `column_ref` and `implicit_label` may refuse
  (H2's origin-less and per-key columns), `SqlDialect::server_versions` may refuse (H2Modern's, written in the facts as
  `keysSnapshot` is), and the base's `variant_get`, `struct_lit` and `struct_get` refusals are `ansi`'s free functions
  (H2Modern's `super` calls).

## Identity

- `ConstantLateralsAsRows`: the select as it came unless a lateral over a constant list became its rows and every read
  of an exploded struct element is a field read; then `new SqlSelect` over it, a projection or sort key rebuilt only
  where its expression changed (Java's `x == p.expr()`), each expression through `mapChildren` (`carrier::map_children`
  and `with_children`, which keep an array literal's given type and a lambda's fact). `rewriteJoins` rebuilds a join
  only where its left side changed or its lateral became rows. Its explosions are a `LinkedHashMap` (an alias replaced
  in place), only looked up.
- `LateralExplodeToUnion`: the select as it came on every guard, `withFrom` otherwise.
- `AggregatesDeliverDouble`: each AVG reducer the walk's hook sees, cast to DOUBLE (`new SqlExpr.Cast`); a window's
  reducer is handed to no hook (`rewriteAgg`), so a windowed AVG stays as Java leaves it.
- `SourceSpelling`: a reference re-stamped (`Column.of(table, out)`, the output's type and origin) only where its origin
  is not its source's declared one (a null origin is not); every other node as it came. Its scope (alias to outputs,
  over every source of the statement but a join and Dual, the last one of an alias the walk meets kept, as
  `HashMap.put` keeps it) is only looked up.

## What Java does that this does not reproduce exactly

None of these reaches the oracles.

- `formatAsConcat` reads the template by characters where Java reads UTF-16 units: a `%` before a character outside the
  Basic Multilingual Plane is refused naming that character whole, where Java's message holds its high surrogate alone
  (a lone surrogate a Rust string cannot hold). The `split_part` arm's one-character separator is one UTF-16 unit, and
  `translate`'s lengths are code points, as Java's.
- `equalArm`'s `toLowerCase(Locale.ROOT)`, `h2Interval`'s `toUpperCase(Locale.ROOT)` and `execPart`'s reserved-word
  check are Rust's `to_lowercase` and `to_uppercase`: the same casing for every word they compare, each from its own
  Unicode version (only ASCII reaches the reserved-word check, past the plain-word pattern).
- The seam refuses `duckdb.tables` and `duckdb.readsStored` on H2 and H2Modern (`'<op>' is DuckDb's op, not H2's`, an
  `IllegalArgumentException`): ops of DuckDB's shim that no Java method of H2's answers.
- Java's frame interval `n * 7` and H2Modern's index `ix + 1` are `long` arithmetic, wrapping: so are Rust's.

# The engine-text dialects: EngineStyleH2, EngineStyleDB2, EngineStyleComposite

Step 4c (docs/rust-port/PHASE1.md §4): legend-engine's own SQL text, `EngineStyleH2.java`, `EngineStyleDB2.java` and
`EngineStyleComposite.java` (`src/engine_style.rs`), with Java's `EngineTextDialect` (`dialect::EngineTextDialect`).

## The numbers

**EngineStyleH2: 2,105 of 2,105 distinct inputs identical (100 %); EngineStyleDB2: 56 of 56; EngineStyleComposite: 24
of 24.** The floors (`tests/enginestyleh2-floor.txt`, `tests/enginestyledb2-floor.txt`,
`tests/enginestylecomposite-floor.txt`) are 2,105, 56 and 24. The oracles hold 2,132 EngineStyleH2 lines (2,092 of the
H2 dump over five configurations, 39 of the first, the tagged query's one), 61 EngineStyleDB2 lines (51 and 10) and 29
EngineStyleComposite lines (19 and 10). Every one of Java's refusals is reproduced, class and message --
EngineStyleH2's 18, DB2's 15, Composite's 15, all `DialectCapability` -- and Rust refuses nothing Java answered. No
engine-text statement binds a parameter (the plan text's parameters are FreeMarker templates in its text).

| EngineStyleH2 op | identical | different | refused (Java rendered) | accepted (Java refused) |
|---|---:|---:|---:|---:|
| `ddl.render` | 292 | 0 | 0 | 0 |
| `dml.render` | 141 | 0 | 0 | 0 |
| `dml.renderStatement` | 12 | 0 | 0 | 0 |
| `engine.collectionSplice` | 6 | 0 | 0 | 0 |
| `engine.inTempTableSelect` | 2 | 0 | 0 | 0 |
| `engine.renderAsAliased` | 9 | 0 | 0 | 0 |
| `engine.renderedAlias` | 1 | 0 | 0 | 0 |
| `engine.selectList` | 4 | 0 | 0 | 0 |
| `query.render` | 1,638 | 0 | 0 | 0 |

| EngineStyleH2 configuration (`quoteIdentifiers`, `timeZone`) | distinct inputs | identical | of them `query.render` |
|---|---:|---:|---:|
| `false`, `null` (Java's `new EngineStyleH2()`; the first dump's) | 1,961 | 1,961 | 1,503 |
| `false`, `"GMT"` | 129 | 129 | 121 |
| `false`, `"EST"` | 1 | 1 | 1 |
| `false`, `"US/Arizona"` | 5 | 5 | 5 |
| `true`, `null` | 6 | 6 | 5 |
| `true`, `"GMT"` | 3 | 3 | 3 |

| EngineStyleDB2 op | identical | | EngineStyleComposite op | identical |
|---|---:|---|---|---:|
| `dml.render` | 5 | | `dml.render` | 5 |
| `dml.renderStatement` | 5 | | `dml.renderStatement` | 5 |
| `engine.collectionSplice` | 2 | | `query.render` | 14 |
| `engine.inTempTableSelect` | 2 | | | |
| `query.render` | 42 | | | |

(DB2 and Composite are met with their default configuration alone; nothing is different, refused or accepted.)

By MIR kind every one is identical: EngineStyleH2's `SqlSelect` 1,628, `CreateTable` 148, `InsertValues` 143,
`DropTable` 132, `SqlUnion` 23, `CreateSchema` 6, `DropSchema` 6, `PlanParam` 6, `String` 3, and `CreateTempTableAs`,
`DeleteWhere`, `InsertSelect`, `Merge` and `Update` 2 each; DB2's `SqlSelect` 42 and Composite's 14, and each two of
the five store writes, DB2's `PlanParam` 2 and `String` 2. The tagged query (`sqlstring::testGreatestLeast`, its
`SqlExpr.Tagged` replaced by its comparable as the shim will send it) renders as Java rendered the original:
`greatest("root".quantity, 1, 3)`.

The seam answers every op of them as a direct call does: EngineStyleH2's 2,105 ops, 1,638 inline renders and 5,104
aliases asked after a render (`engine.renderedAlias` with the rendered query, held to `render` then `renderedAlias` on
one dialect object), 26 refusals; DB2's 56, 42 and 43, 20 refusals; Composite's 24, 14 and 11, 20 refusals.

The first measurement was already 100 %: nothing was found by the differential that the port had not reproduced.

## What is ported

- **EngineStyleH2** (`EngineStyle` with `Profile::H2`): every one of its overrides but `tagged` -- the one-line `query`
  (WITH and UNION inline), the `select` (H2's `top`, `offset .. fetch next`, QUALIFY refused), the select head (an
  exists-key frame's bare DISTINCT keys), the projection (a date constant as a plain string, a scalar-map column
  unaliased, a view frame's unquoted aliases, a pre-quoted alias quoted once), the sources (a table's parts quoted under
  `quoteIdentifiers`, a subselect's frame depth, the plan variable), the engine's alias plan (`planQuery`: groups by
  lowercased table, frame or variable name, a root scope's leftmost source `"root"` spending its group's index once,
  pre-order numbering, correlated EXISTS in the outer numbering), `wrapTdsJoinTop`, the expression forms (Float and
  Decimal literals, the correlated `exists (select 1 ...)`, mixed-operator arithmetic wrapped, every plan-parameter
  template and `GMTtoTZ`, the enum selector and the optional parameter's null-safe equality, the dotted struct
  placeholder, the quoted frame reads, flat AND/OR, the guarded IN, the temp-table splice, the collection templates, a
  singleton IN as equality), `engineDateDiff` (the elapsed-time and Sunday-week folds, the decode chain, the lowercase
  CASE), the whole `call` table and `listCall` (variadic greatest/least, the infix reduction chains, firstNotNull as
  coalesce, joinStrings' flat concat, `collectionSize`), the aggregates (`listagg ... within group`, the percentiles,
  the bare ANY_VALUE), the window's ordered `listagg`, the lowercase keywords and sort keys (no null placement), the
  per-dynafunction casts, membership, the date, timestamp (with the zone shift) and time literals, the scalar types, the
  DDL (`DdlSpelling.engineText`, the engine's column rule, raw key names, `,`), `storedName`, `insertColumnName`, the
  seed rows, `renderStatement`'s refusal, `fromEngineText`, the passes (`CarrierStrategies(H2.CAPS, false)` -- its
  literal reductions not folded, a mode first run here -- then `LateralExplodeToUnion`), and `EngineTextDialect`'s five
  methods. `DecodeShapes.flattenDecode` and `sourceExpr` are `src/decode_shapes.rs`.
- **EngineStyleDB2** (`Profile::Db2`): each of its twelve overrides is a branch where EngineStyleH2's method is written
  -- `expr`'s null-safe OR expansions, `whereSql`'s wrap, `nullSafeEq`, `holderArgs`' quoted boolean, `groupKey`,
  `groupBySeparator`, `infixPad`, `variantAwareCast` (`varchar(16000)`, the bare date-typed parses), and `call`
  (`db2_call`: infix `concat`, `trim`, `cbrt`, `position`, `substr`, the pads, `reverse`, `dayofyear`, `trim(left(..))`,
  `date(current date)`, `CHARACTER_LENGTH(x,CODEUNITS32)`, the `date(1)` truncations, the labeled intervals, `to_date`
  and `timestamp_format`), every other function `h2_call` (Java's `super.call`). Its `scalarTypeName`, `formatText` and
  `reducer` delegate every constant to EngineStyleH2's, so they are its.
- **EngineStyleComposite** (`Profile::Composite`): `composite_call` (plain `char_length`, the full `substring`), every
  other function `db2_call`; its `scalarTypeName`, `formatText` and `reducer` are DB2's, which are H2's.
- **`java.time`** (`src/java_time.rs`), what `timestampLit` uses: `LocalDateTime.parse` (strict ISO, case-insensitive,
  Java's sign rules for the year) and `toString`, `ZoneId.of(id, SHORT_IDS)` (offset IDs, the `UTC`/`GMT`/`UT`
  prefixes, region names), and `java.time.zone.ZoneRules` over the JDK's tzdb, embedded (below): `getOffset(Instant)`,
  `getStandardOffset`, `getValidOffsets`, `getTransition` and `ZonedDateTime.of`'s resolution of a local date-time (a
  gap moved later by its length, an overlap's earlier offset), each written from its JDK 25 source -- Java's own
  `Arrays.binarySearch` (which of equal local transitions it finds is Java's), `findYear`, the last rules'
  `ZoneOffsetTransitionRule.createTransition` for any year (`previousOrSame`/`nextOrSame`, the end-of-day flag, the
  UTC, wall and standard time definitions), and a `DateTimeException` as `None`.

## The JDK's time-zone database

`rust/oracles/tzdb` (its `BUILD.bazel` and `MANIFEST.tsv`): `TzdbData.java`, a build action on the build's JDK
(`//rust/oracles/tzdb:tzdb_data`, JDK 25.0.4+7, tzdb 2026b), writes every region `ZoneId.of(id, SHORT_IDS)` accepts --
604 regions over 345 distinct rule sets (links share theirs), and the 28 short IDs -- each rule set copied from
`ZoneRules`' own fields (the standard transitions have no public accessor; each array is checked against the public
answers): the standard transitions and offsets, the savings transitions and wall offsets, the last rules. Its compact
form (`tzdb.bin`: LEB128 varints, the transitions delta-encoded) is 145,799 bytes, committed with its SHA-256 and
embedded by `include_bytes!`, read once. The release `liblite_ffi` grows from 4,185,776 to 4,320,976 bytes (+135,200,
3.2 %; the hand-carried zone tables it replaces went).

**Held to java.time: 0 different** (`the_zone_shift_answers_as_java_does`, `LITE_ZONE_ORACLES`):

| oracle | lines | different |
|---|---:|---:|
| `zone-golden.tsv` `O`: an instant's offset, standard offset and local date-time (every ID at 63 instants of years 1, 1000, 1800-2100 and 9999; every rule set at each transition of 1800-2100 and of years 1, 1000, 5000 and 9999, a second before and at it) | 119,376 | 0 |
| the same instants' UTC text through `timestampLit`'s shift (`shift_to_zone`) | 119,376 | 0 |
| `zone-golden.tsv` `L`: a local date-time's valid offsets, its transition and `ZonedDateTime.of` (the same days at noon for every ID; around every transition a second before the gap or overlap, its start, its middle and its end) | 196,892 | 0 |
| `engine-text-zones.tsv`: Java's own `EngineStyleH2(false, zone)` rendering `select TIMESTAMP'<iso>' as "t"`, 30 zones (half-hour and 45-minute offsets, negative and skipped-day transitions, short IDs, offset IDs, unknown ones) by their transitions of 1950, 2019 and 2050 and timestampLit's reading's edges | 684 | 0 |

The `O` and `L` lines are generated with the data (`zone-golden.tsv.zst`, 1.46 MB); the renders run on core's built
jars (`EngineTextZones.java`, its command in its comment), the only way to reach Java's dialect from `rust/oracles`.
The first measurement was already exact.

## Identity and state

- The engine text runs two passes, both ported for H2 (`CarrierStrategies`, `LateralExplodeToUnion`): their identity is
  H2's (above). `CarrierStrategies` with `foldLiteralReductions` false leaves a reduction over a literal list as it
  came; the engine text spells it as its infix chain.
- `wrapTdsJoinTop` builds its select through `new SqlSelect` (`build::select_of`, the outputs recomputed from the
  projections, each projection's output normalized to DERIVED), as Java's.
- `planQuery` mutates the dialect's alias plan (`renames`, `subselects`, `rootConsumed`, `placeholders`, cleared at each
  plan): Rust keeps it in a `RefCell` on the dialect, cleared at the same point (after the passes and the wrap). Java's
  `subselects` keeps the frame itself; Rust keeps only what `quotedFrameRead` reads of it (a union frame's output names,
  a plain frame's projection aliases, or nothing), replaced as Java's `put` replaces. `frameDepth` and
  `anonDistinctDepth` are `Cell`s restored after the frame is written, a refusal included (Java's `finally`).
- `renderedAlias` reads the last plan, as Java's; the seam replays it from the query the caller rendered (`SEAM.md`
  §3b), and the differential holds every alias of every rendered query to the stateful answer.

## Java's behavior reproduced that looks like a bug

Each is Java's text or refusal as it is, reproduced and pinned by a unit test; none is fixed here.

- `timestampLit` under a connection's zone: `LocalDateTime.toString` drops a zero second, and `:00` is put back only
  when nothing follows the 19 characters read, so `2019-04-08 18:00:00.000` shifts to `2019-04-08 13:00.000` (the
  seconds lost before the fraction). The shift also swallows every `DateTimeException`: a constant that is no date-time,
  or a zone Java does not know, is written unshifted, silently. A year past 9999 written `+10000-...` would not have its
  `T` replaced (only position 10 is looked at).
- `engineDateDiff` reads the elapsed time's unit through `(int) u.value()`: a `long` divisor of `2^32 + 60000` folds to
  `datediff(minute, ...)`.
- DB2's interval: `-v` of `Long.MIN_VALUE` stays negative (`x - -9223372036854775808 DAYS`) and a week count's `v * 7`
  wraps.
- `joinedReduction` checks only that a COALESCE's first argument is the string aggregate over a literal list, not that
  its default is `''`: `COALESCE(<joined list>, 'x')` is written as the flat concat, the default dropped.
- The IN arm writes any two-argument IN as equality (`x = y`), the second argument any expression, not only a literal
  (its comment says "singleton literal in-list").
- `wrapTdsJoinTop` quotes an output name into the wrap's column as `"<name>"` without doubling a quote inside it.
- The seed text refuses an integer literal cell ("a IntLit cell reached the engine's seed text, which writes values
  only"): it writes numbers from decimal literals alone.

## What Java does that this does not reproduce exactly

None of these reaches the oracles.

- **The tzdb is the build JDK's.** The JVM shifts by its own JDK's tzdb; the Rust library by the build JDK's (JDK
  25.0.4, tzdb 2026b), embedded. A JVM of another tzdb version (a later JDK, or `tzupdater`) could shift a constant of a
  zone whose rules changed differently until the data is regenerated (`rust/oracles/tzdb/BUILD.bazel`). (`SHORT_IDS`
  differs across JDKs too: `EST` is America/Panama on JDK 25 and the fixed `-05:00` before it -- the same offset since
  1908.)
- A refusal that embeds the call's argument list (`strftime format has no engine-H2 formatdatetime spelling yet: [...]`,
  the `strptime` one, DB2's `date_trunc unit`, `interval shape` and `strptime` ones) writes each argument's simple name
  (`[Column[...], FormatLit[...]]`) where Java writes `List.toString()` of the records' whole text; likewise the
  unreachable `decode branch is not an equality: <node>`.
- `toLowerCase(Locale.ROOT)` (an alias group, a temp table's alias, the keywords, a type name) and the engine column
  rule's reserved-word check are Rust's `to_lowercase`: the same casing, each from its own Unicode version.
- `tagged` (Java's spelling of a `SqlExpr.Tagged` by its comparable) is not ported: the Rust MIR has no tagged node, and
  the JVM's shim replaces each by its comparable before sending a query (`tagged-engine.jsonl` holds the one the suite
  meets, its text Java's).
- The seam refuses `duckdb.*` on the engine-text dialects, as on H2, and `engine.*` on every other dialect: ops no Java
  method of theirs answers.

# Postgres

Step 5b (docs/rust-port/PHASE1.md §4): Java's `Postgres` and `PostgresHostOperations` (`src/postgres.rs`,
`src/host_ops.rs`), and the passes `Postgres.passes()` installs that no earlier dialect ran (`src/shared_passes.rs`).

## The numbers

**11,630 of 11,630 distinct inputs identical (100 %)**; the floor (`tests/postgres-floor.txt`) is 11,630. The oracles
hold 13,694 Postgres lines (11,613 of the dialects dump, 2,081 of the first); an input both hold is one statement, and
none has two answers. The SQL, the binds and the regions are compared exactly: 42 statements bind a parameter or write a
region and carry Java's record text. All 9 of Java's refusals are reproduced, class and message (8 `DialectCapability`
-- the four catalog grids, a PIVOT, three folds into a struct accumulator -- and one `IllegalStateException`, a bound
statement asked for as text), and Rust refuses nothing Java rendered. **The first measurement was already 100 %.**

| op | identical | different | refused (Java rendered) | accepted (Java refused) |
|---|---:|---:|---:|---:|
| `ddl.render` | 45 | 0 | 0 | 0 |
| `dml.render` | 17 | 0 | 0 | 0 |
| `dml.renderStatement` | 1,398 | 0 | 0 | 0 |
| `query.render` | 42 | 0 | 0 | 0 |
| `query.renderStatement` | 10,128 | 0 | 0 | 0 |

By MIR kind every one is identical: `SqlSelect` 10,070, `InsertSelect` 462, `Merge` 407, `CreateTempTableAs` 404,
`SqlUnion` 100, `DeleteWhere` 85, `Update` 37, `DropTable` 31, `InsertValues` 20, `CreateTable` 13, `CreateSchema` 1.

The oracle exercises nearly every arm of `call` (153 of the 181 `SqlFn`s; the maps, Levenshtein and Jaro-Winkler, the
exact Float remainder, printf, the list lambdas, folds and reductions, the regex family among them), 22 aggregates and
29 windowed functions. The 28 it holds none of -- `ALL_DISTINCT`, `LIST_DISTINCT`, `LIST_INIT`, the list Boolean
reductions, `REPEAT_VALUE`, `STRUCT_INSERT`, `JSON_ARRAY_LENGTH`, `JSON_PRETTY`, the epoch conversions, the day and
month names, `CONCAT_JOIN`, `FLOOR_RAW`, `IS_DISTINCT_FROM`, `NEGATE`, `ADD_INTERVAL_TEMPORAL`, every wall (`STRPTIME`,
`JSON_MERGE_PATCH`, `EPOCH_NS`, `FROM_EPOCH_NS`, `LIST_FLATTEN`, `LIST_ZIP`, `LIST_PRODUCT`, `TYPEOF`); `FLOAT_CANON`'s
arm, which the Float expansion run first leaves nothing to reach -- with the aggregates `ANY_VALUE`, `VARIANCE` and
`STDDEV` and the refusals of a shape Postgres cannot spell, are held to the Java by `src/postgres/tests.rs`, as
`fromEngineText` is (no oracle line).

The seam answers every op of them as a direct call does: 11,630 ops and 42 inline renders (11,523 statements read back
from their JSON equal and with Java's record text, 14 refusals).

## What is ported

- **The dialect** (`src/postgres.rs`): every one of `Postgres.java`'s overrides -- the lexicon (the documented reserved
  set), `maxFunctionArguments` (100), the scalar, struct, map and DDL type tables, the catalog (`CATALOG_TYPES` in
  Java's order, `CATALOG_RULES`), the session, the script and its abort, the host operations, `fromEngineText`, the keys
  snapshot and column limit, the static pivot, the passes, the materialized CTE, the whole `call` table (all 181
  `SqlFn`s, its walls among them) with its helpers (`maps`, `listCall`, `listConcat`, `listTransform`, `listReduce`,
  `range`, the predicates, `format`, `pattern`, the exact remainder, `roundHalfUp`, `timeBucket`, `dateTrunc`,
  `dateDiff`, the raise, `levenshtein`, `jaroWinkler`), `membership`, `reduceCollection`, the list literal,
  `unnestProjection`, `foldCall`, the stored reads as jsonb, the labels, `projection`'s three typed slots, the
  identifier rules (`ident`, `physicalName`, `bareNameReaches`, `ddlIdentifier`, `tableName`, `columnName`), the refused
  row order and star exclusion, `expr`'s array literal and ordered list aggregate, the type hole and the truncated
  timestamp literal, the NUL refusal, `variantAwareCast` (lists, maps, Variants, a value out of a variant, the Float
  text, the typed text), `variantGet`, `variantConstruct`, the struct literal and field read, `caseExpr`'s typed raise,
  `hashSigned`, `roundHalfEven`, `bitOp`, every aggregate, `windowCall`, the frame bounds, the JSON constructors, the
  date format, `jsonKindName`, `typeofName`, `deliversOwn` and the four store writes and the staged table. Java's
  `SqlWriter.Piece` is a shared closure (`postgres::P`): a piece written several times writes its parameters each time,
  and what Java evaluates when it makes a piece (a type's name, a string literal, an argument read eagerly) Rust
  evaluates then too.
- **The passes** (`src/shared_passes.rs`, where the dialects still to come find them): `StarExceptToColumns`,
  `WholePartitionOrderedSets`, `StringCountsAsInt`, `RootNumericTypes`, and `QualifyToSubselect`'s by-outputs form
  (`new QualifyToSubselect(true)`), moved there with its plain form and `AggregatesDeliverDouble` (re-exported by
  `h2_passes`). `CarrierStrategies` ran under Postgres's caps (no native lists, correlated explosion, the JSON carrier,
  FULL OUTER native) for the first time, unchanged; `SubstringClamp` and `ConstantKeysAsExpressions()` (the no-argument
  form, ported with DuckDB's) are `src/passes.rs`'.
- **`fromEngineText`**: `RawSqlBoundary.translated` (`src/raw_sql.rs`) with Postgres's column types --
  `(?i)\bDOUBLE\b(?!\s+PRECISION)` to `DOUBLE PRECISION` (`raw_sql::double_precision`), then `(?i)\bTINYINT\b` to
  `SMALLINT` (`raw_sql::replace_word`), each a hand-written matcher of Java's pattern -- and a bare name in lower case.
- **The shared renderer**, for the hooks Postgres reads a node's fact in or throws from: `call`, `case_expr` and
  `struct_get` take the node's type fact (Java's `c.type()`, `g.type()`; a field read's fact is not a function of its
  source), `fold_call` the `FoldCall` node; `string_lit` (the NUL refusal), `bound` (an unknown frame unit, an
  overflow), `row_order_column` and `star_except_keyword` may refuse. DuckDB, H2 and the engine text follow, their
  output unchanged.

## Identity

- `StarExceptToColumns`: the select as it came unless a projection is a star exclusion; then `withProjections`.
- `WholePartitionOrderedSets`: the select as it came when grouped, over a join or holding no whole-partition ordered
  set; a projection rebuilt (`new Projection`) only where its expression changed (Java's `e != p.expr()`), each
  expression through `withChildren` (`carrier::with_children`, which keeps an array literal's and a field read's given
  fact), the copy's alias `<from>_ops<i>` and one level down `_<child>`; a source no copy can be made of leaves the
  window.
- `StringCountsAsInt`: `new Call` only where a count was cast; every other call as the walk hands it on.
- `QualifyToSubselect` by outputs: its two expression walks are the rewriter's own (`rewrite_expr` with one hook), so a
  hidden column is minted in the walk's post-order; a projection is found by Java's `same` (a column by table and name,
  any other node by record equality).
- `RootNumericTypes`: root only (a union's branches and a WITH's body); a union always rebuilt, its labels
  re-reconciled; its `cast` map (a `HashMap` by equality, a repeated expression's last cast) only looked up.
- `AggregatesDeliverDouble` over Postgres's ten Float aggregates: each grouped one cast; a windowed one is the dialect's
  (`windowCall`).

## Java's behavior reproduced that looks like a bug

Each is Java's text or refusal as it is, reproduced; none is fixed here.

- `format` (printf unrolled): a precision on `%s` or `%d` (`%.3s`) is read and ignored, and the `0` flag of `%f`
  (`%08.2f`) pads with spaces -- only `%d` pads with zeros.
- A window frame's interval unit is looked up by its plural name alone (`YEARS`, ..., `SECONDS`; a week 7 days): a
  singular `DAY` is refused (`a DAY window frame offset has no Postgres interval qualifier`), where H2 takes either.
- `percentile` writes its value from the reducer's first argument and reads only the direction of its one order key,
  whatever that key's expression.
- `structGet`'s declared-field search does not stop at the first match: of two fields of one name the last decides the
  read's type.
- `LENGTH` over a text literal writes every argument the call holds (`length(<a>, <b>, ...)`); over anything else only
  the first.

## What Java does that this does not reproduce exactly

None of these reaches the oracles.

- `physicalName`'s, `bareNameReaches`' and `fromEngineText`'s `toLowerCase(Locale.ROOT)` and the frame unit's
  `toUpperCase(Locale.ROOT)` are Rust's `to_lowercase` and `to_uppercase`: the same casing, each from its own Unicode
  version.
- A lambda read as `(SqlExpr.Lambda) x` that is none is refused as a `ClassCastException` naming the simple classes
  (DuckDB's note above), and a lambda of too few parameters in `ArrayList`'s words (`Index 1 out of bounds for length
  1`; a `List.of` of two words it `Index: 1 Size: 1`).
- The seam refuses `duckdb.*` and `engine.*` on Postgres: ops no Java method of Postgres answers.

# SQLite

Step 5d (docs/rust-port/PHASE1.md §4): Java's `SQLite`, `SqliteRegex` and `SqliteDigests` (`src/sqlite.rs`,
`src/sqlite/`), and the two passes `SQLite.passes()` installs that no earlier dialect ran, in shared modules where
ClickHouse, MariaDB, Snowflake, Trino and BigQuery will find them: `ExplosionMarkers` (`src/explosion_markers.rs`) and
`CommonSubexpressions` (`src/common_subexpressions.rs`).

## The numbers

**2,529 of 2,529 distinct inputs identical (100 %)**; the floor (`tests/sqlite-floor.txt`) is 2,529. The oracles hold
4,162 SQLite lines of the two dumps (2,081 each; an input both hold is one: 2,158 distinct, none with two answers) and
371 of `mir/sqlite-arms.jsonl` (below). The SQL, the binds and the regions are compared exactly: 44 statements bind a
parameter or write a region and carry Java's record text. All 140 of Java's refusals are reproduced, class and message
(137 `DialectCapability`, 3 `IllegalStateException`), and Rust refuses nothing Java rendered. **The first measurement
was already 100 %**: 2,158 of 2,158 over the dumps, then every arm.

| op | identical | different | refused (Java rendered) | accepted (Java refused) |
|---|---:|---:|---:|---:|
| `ddl.render` | 95 | 0 | 0 | 0 |
| `dml.render` | 79 | 0 | 0 | 0 |
| `dml.renderStatement` | 78 | 0 | 0 | 0 |
| `query.render` | 46 | 0 | 0 | 0 |
| `query.renderStatement` | 2,231 | 0 | 0 | 0 |

By MIR kind every one is identical: `SqlSelect` 2,262, `InsertValues` 121, `DropTable` 46, `CreateTable` 45,
`CreateTempTableAs` 18, `SqlUnion` 12, `InsertSelect` 10, `CreateSchema` 3, `SqlWith` 3, `DeleteWhere`, `Merge` and
`Update` 2 each, `DeleteAll`, `DropSchema` and `InsertFromTable` 1 each.

**The arms the lanes do not reach, rendered by Java itself.** The dumps reach 138 of the 181 `SqlFn` arms on SQLite and
never the regular expressions' find path (only full matches), the maps, most casts, a quantified comparison, a VALUES
of no rows or a firing `CommonSubexpressions`. Rather than hold those to hand-derived texts, `rust/oracles/mir/
SqliteArms.java` builds 371 inputs with Java's own constructors, writes each as MirJson, reads it back and renders it
on a fresh `new SQLite()` (the tree the JVM's shim will send), on core's built jars and the build's JDK (25.0.4, its
command in its header): the other 43 `SqlFn` arms (with them all 181 are held; `FLOAT_CANON`'s input is expanded before
its arm, as on Postgres), `regexp_extract` with and without a group, `regexp_extract_all`, `regexp_replace` plain and
global with group references, the maps, the casts from twelve types to thirteen, `fromVariant`, the root's delivered
values (decimal, Float, big integer; a sort key moved to its expression; a union's and a WITH's), ANY and ALL, the
explosion markers' shapes (a CASE over three lists, a NULL list, an unqualified read, LEFT JOIN ON TRUE, a carried
list inlined, a star unwrapped, nested `__unnest<n>`), the lateral subqueries read as JSON rows, the windowed moments,
the interval RANGE frames and their refusals, the date-time formats, and `CommonSubexpressions` firing (`__cse0`
nested to `__cse2`, within a lambda, a raise left in place), 47 of them a refusal. `lite-sql` reads every one back to
Java's bytes (`//rust/lite-sql:mir_round_trip_test`). The first Rust measurement of them was 367 of 367.

The seam answers every op of them as a direct call does: 2,529 ops and 46 inline renders (2,189 statements read back
from their JSON equal and with Java's record text, 150 refusals).

## What is ported

- **The dialect** (`src/sqlite.rs`: the structure, the lexical hooks, the JSON helpers, the lists' building blocks, the
  Variant and the casts, the DDL and DML, its `SqlDialect`; `src/sqlite/functions.rs`: `calls`, `lists`, `maps`, the
  regular-expression, string and temporal helpers; `src/sqlite/aggregates.rs`: the signed hash, banker's ROUND, the bit
  operators, every reducer and the windows): every one of `SQLite.java`'s overrides -- the lexicon, the session's
  `PRAGMA case_sensitive_like`, the keys snapshot and the 2,000-column limit, `fromEngineText` refused, the scalar types
  as storage classes, the strftime format, the passes and `bindsFloatText`, the union's wrapped branches, the marker
  select over `json_each` positions (`__u<depth>`), the root's delivered values, the stars spelled over a marker, the
  lateral subqueries as JSON rows, the VALUES named by a select (`column<n>`), the NUL as `char(0)`, the ISO date and
  date-time texts, the REAL Float literal (`9e999` past the range, NaN refused), the JSON-held lists, structs, maps and
  Variants, the quantified comparison counted in three-valued logic, every `SqlFn` (the regular expressions as
  automaton walks, base64, Levenshtein and Jaro-Winkler as recursive walks, the digests in SQL), the large repeated
  operand bound once (`__b<n>`), every aggregate (the statistics extension's sample moments, the population ones from
  them, covariance and correlation from their sums, the quantiles over a sorted JSON array), the windows (a windowed
  moment as its sums, an interval RANGE frame over a date key's Julian day), the JSON constructors, the TEXT type
  holes, the schema-qualified table as one delimited name, the DDL and DML, `tempTableAs`, `jsonKindName`,
  `typeofName`, `wireTypeName` and `deliversOwn`.
- **`SqliteRegex`** (`src/sqlite/regex.rs`): the RE2 subset's parser (the leading flags, classes and their escapes,
  groups capturing or not, named groups, alternation, the repetitions and their lazy forms, the anchors, every refusal
  by name), the Thompson NFA (Java's state numbers, edge order and predicate order), each closure ascending (Java's
  `BitSet`), and the two walks, `matches` and `find`, their SQL text Java's.
- **`SqliteDigests`** (`src/sqlite/digests.rs`): MD5, SHA-1 and SHA-256 as SQLite's own SQL.
- **The passes**: `ExplosionMarkers`' predicates (`explodedRead`, `nullList`, `nullOverNullList`, `isMarker`,
  `isUnnest`, `markerColumn`) and its three passes, and `CommonSubexpressions`. `CarrierStrategies` ran under SQLite's
  caps (Postgres's: no native lists, correlated explosion, the JSON carrier, FULL OUTER native) unchanged, and
  `FloatCanonText.Expansion`'s binding form (`__fc<n>`, SQLite's `bindsFloatText`) first ran on a dialect here, as
  ported with DuckDB.

## Identity and state

- **The statement's state.** Java's SQLite keeps five pieces of state on the dialect object while it writes (`laterals`,
  `rowLaterals`, `roots`, `rootSelects`, `binds`); Rust keeps them in `RefCell`s and `Cell`s, pushed and popped around
  the same calls, a guard popping them on a refusal (Java's `finally`). The fresh names (`__b<n>`) start over at each
  outermost query render (the user's ruling, 2026-11-03; Java's `binds[0] = 0` when `roots` is empty); `tempTableAs`
  pushes the root but starts no names over, as Java's. A name is taken when its piece is made (`over`), so nested binds
  number in Java's writing order.
- **The root selects.** Java keys `rootSelects` by identity (an `IdentityHashMap` of the written root, a union's
  branches, a WITH's body). Its last pass, `CommonSubexpressions`, hands every select equal to one already rewritten
  that one's rewrite -- the same object -- so over the written tree Java's identity is structural equality, and Rust
  keeps the root selects by content. Measured by Java (`mir/sqlite-arms.jsonl`): a root select met again inside another
  branch's subquery is delivered there too; a pointer identity would have missed it.
- **`ExplosionMarkers`**: `StarOverExplosionUnwrapped` and `LateralUnnestAsMarker` answer `Same` on every source they do
  not rewrite, `SelectListUnnestAsLateral` on every select; a rewrite goes through `build::join` (the delivered columns),
  `build::select` (the outputs from the projections) and `build::projection` (an attached output normalized). Its
  `__unnest<n>` is counted by the pass object, made fresh for each render. Java's `carried` (a `LinkedHashMap`) keeps a
  repeated name's last list in place.
- **`CommonSubexpressions`**: Java first makes every equal subtree one object (`canonical`), then counts each node's
  uses, sizes, movability and names, and replaces a node, by identity. Rust counts the uses over the expression's
  hash-consed DAG (a node per distinct subtree: its own fields and its children's nodes), replaces every subtree equal
  to the bound one, and binds a lambda equal to one already bound with that one's answer -- Java's identity over the
  pooled tree. Java's selects are rewritten once per content (`rewritten`, a `HashMap` by `equals`): an equal select met
  again is handed the first one's rewrite, `Rw::New` (another object than its input, so its parent is rebuilt). A
  projection whose expression is too small to repeat a 40-node subtree (its written size under 80) is skipped before
  the DAG is built: nothing in it could be bound.

## Java's behavior reproduced that looks like a bug

Each is Java's text or refusal as it is, reproduced; none is fixed here.

- A root select with an explosion marker or a lateral subquery in scope is respelled (`starsSpelled`, a new select)
  before its identity is looked up, so its values are never delivered as its labels say: a decimal column is not
  written at its scale, a Float not cast to REAL, a big integer not raised past 64 bits (`mir/sqlite-arms.jsonl` holds
  one). For the same reason `rootSelects.put(over, TRUE)`, the marker select's own registration, has no effect.
- `regexp_replace` reads any decimal digit unit after a backslash as a group number (`Character.isDigit`, its unit
  less `'0'`): `\٣` is group 1587, refused as "no group 1587".
- `structGet`'s declared-field search takes the last field of a name, but never runs on a struct that holds the field:
  a field read of a typed struct is typed by its first field when it is built (`SqlTyping.structGetType`).
- `windowCall` refuses "a windowed MODE" after the ordered sets' check, which MODE meets first: that message is never
  written.
- An interval RANGE frame's offset is written as Java's double text of its days (`3.0 PRECEDING`, `1.5 FOLLOWING`).
- Through one dialect object, a `CREATE TEMPORARY TABLE ... AS` after a query render keeps counting the query's fresh
  names (`tempTableAs` starts none over); the seam makes a dialect per call, so no answer through it shows that, and no
  test of core, pct, spec or parser-equivalence names a `__b<n>`, `__cse<n>` or `__unnest<n>` (grepped).

## What Java does that this does not reproduce exactly

None of these reaches the oracles.

- **`CommonSubexpressions`' tie.** Of two repeated nodes of the same largest size, Java binds the first its
  `IdentityHashMap` iterates -- identity-hash order, which differs from run to run of the JVM; Rust binds the first met
  in a pre-order walk. (The arms oracle holds no tie, so its Java answers are stable.)
- `CommonSubexpressions`' `canonical` rebuilds a parent whose child was re-pooled, through `withChildren` (its fact
  recomputed); Rust keeps the tree as it came, whose facts are the recomputed ones wherever the typing holds (0
  different over the oracles).
- An unterminated named group (`(?P<n`, `(?<n`): Java reads the name to `indexOf('>') + 1`, the pattern's start, and
  parses the pattern again inside the group until its stack overflows (`StackOverflowError`, an `Error`); Rust refuses
  it as `java.lang.StackOverflowError` after 64 nested readings, an `INTERNAL` refusal at the seam.
- The pattern is read by UTF-16 units, as Java's; a refusal naming a lone surrogate unit (a flag, an escape) writes
  U+FFFD where Java writes the unit.
- `ddlType`'s `toUpperCase(Locale.ROOT)` is Rust's `to_uppercase`: the same casing, each from its own Unicode version.
- A non-finite Float literal: Java's `FloatLit` cannot hold one (its constructor types it through
  `BigDecimal.valueOf`); a MirJson `NaN` read in Rust is refused and an infinity written `9e999`, as `floatLiteral`
  would.
- The seam refuses `duckdb.*` and `engine.*` on SQLite: ops no Java method of SQLite answers.

# Trino

Step 6b (docs/rust-port/PHASE1.md §4): Java's `Trino` (`src/trino.rs`; `src/trino/functions.rs` the `call` and
`listCall` tables and their helpers, `src/trino/aggregates.rs` the hooks of the base, the aggregates and windows, the
frame intervals and the date format), and the two passes it installs that no earlier dialect ran, in
`src/shared_passes.rs` where Snowflake and ClickHouse will find them: `PivotAsGroupBy` and `AsOfAsPick`.

## The numbers

**2,630 of 2,630 distinct inputs identical (100 %)**; the floor (`tests/trino-floor.txt`) is 2,630. The oracles hold
2,650 Trino lines (2,598 of `mir/dialect-arms.jsonl`, 25 of the dialects dump, 27 of the first; 20 repeat an input,
none with two answers). The SQL, the binds and the regions are compared exactly: 25 statements bind a parameter or
write a region and carry Java's record text. All 350 of Java's refusals are reproduced, class and message (215
`DialectCapability`, 134 `IllegalStateException`, one `NullPointerException`: a quantified comparison by
IS_DISTINCT_FROM, below), and Rust refuses nothing Java rendered. The first measurement was 2,626 of 2,630: four
`regexp_replace` replacements whose group number Rust's port of `Character.digit` misread, fixed.

| op | identical | different | refused (Java rendered) | accepted (Java refused) |
|---|---:|---:|---:|---:|
| `ddl.render` | 47 | 0 | 0 | 0 |
| `dml.render` | 29 | 0 | 0 | 0 |
| `dml.renderStatement` | 29 | 0 | 0 | 0 |
| `fromEngineText` | 4 | 0 | 0 | 0 |
| `query.render` | 76 | 0 | 0 | 0 |
| `query.renderStatement` | 2,445 | 0 | 0 | 0 |

By MIR kind every one is identical: `SqlSelect` 2,512, `CreateTable` 38, `InsertValues` 22, `DeleteAll` 8,
`CreateTempTableAs`, `DeleteWhere` and `Update` 6 each, `DropTable` and `SqlUnion` 5 each, `InsertSelect`, `Merge`,
`SqlWith` and `String` (the engine text, every one refused) 4 each, `CreateSchema`, `DropSchema` and `InsertFromTable` 2
each.

The arms oracle reaches every one of the 181 `SqlFn` arms, every `SqlAgg.Fn` of the exhaustive `reducer`, every own
wall and every pass that changes a query (`CarrierStrategies` changes none where lists are native) -- measured by
`rust/tools/dialect-arms-coverage.py` (PHASE1.md, 6a). What it cannot hold is unit-tested from the Java
(`src/trino/tests.rs`): `listCall`'s LIST_REVERSE, RANGE_FN, REPEAT_VALUE and default arms and `bitOp`'s default
(their one caller passes none of them), a list lambda that is no lambda (Java's `ClassCastException`, dropped from the
oracle: below), the frame intervals' overflow, a replacement's non-ASCII group digit, the literals past a DECIMAL's
digits, the identifier rule and the pass list.

The seam answers every op of them as a direct call does: 2,630 ops and 76 inline renders (2,152 statements read back
from their JSON equal and with Java's record text, 355 refusals).

## What is ported

- **The dialect**: every one of `Trino.java`'s 60 overrides -- the lexicon (Trino's reserved keywords), the scalar,
  array, map, ROW and DDL type tables, the session (`SET TIME ZONE 'UTC'`), no keys snapshot and no column limit, the
  static pivot, no temporary table and the session table refused, `fromEngineText` refused, the passes, `OFFSET`
  before `LIMIT`, the root's ROW columns as their JSON (`rowsAsJson`), the explosion marker as one `UNNEST` (`WITH
  ORDINALITY` for a bare one, `CROSS JOIN` or `LEFT JOIN ... ON TRUE` for a lateral one, a lateral subquery as `LEFT
  JOIN LATERAL`), the refused star exclusion, row order, as-of clause and PIVOT, every identifier quoted, the timestamp
  literal's space, the explicit labels and the typed NULL or raise in a declared slot, the CASE's raise cast to its
  slot, the Float literal past 38 digits as `Double.toString` (never reached: below), `BIGINT` integer literals, a big
  decimal as `DOUBLE`, the ordered list aggregate, the typed list of rows and ROW literals, the field read (a field
  number as a subscript), the lambdas, every list call (an index lambda over `zip_with` and the positions, the moments
  as rows reduced), the list predicates, membership, the collection reductions, the fold as `reduce`, Pure's splitPart,
  `struct_insert` as a typed ROW, the JSON Variant (`toVariant`, navigation through a map or an array of JSON, the
  casts: ISO text to a zoned timestamp, a timestamp's `T`, a JSON member's text, a double's text), the whole `call`
  table with its helpers (the regular expressions' first match, the replacement's groups, the date parts, truncations,
  intervals and differences by the floored epoch, the time buckets, the zone's fixed offsets, `strftime` over
  `format_datetime` and the value's own six digits, Jaro-Winkler as nested `reduce`s), the signed hash, banker's ROUND
  unrolled, the bit operators, every aggregate (`arbitrary`, `listagg` or `array_join(array_agg)`, `max_by`, the
  covariance and correlation over DOUBLE, the quantiles and the mode over the group's sorted values and its
  `histogram`), the windows (an ordered set over the frame, AVG as DOUBLE, a windowed `listagg` ordered by the window
  or its values), the frame intervals (a week as its days), the JSON constructors, the Joda date format, the JSON kind
  and TYPEOF names, and `deliversOwn`.
- **The passes** (`src/shared_passes.rs`): `PivotAsGroupBy` (a static pivot of one ON key as a grouped CASE
  aggregation, a value's column `v__|__alias`, its slot `CarrierStrategies.generatedSlot`'s) and `AsOfAsPick` (an as-of
  join as a LEFT JOIN on its condition, picked by `QUALIFY k = MAX(k) OVER (PARTITION BY every left column) OR MAX(k)
  OVER (...) IS NULL`), the pick's steps one function (`as_of_pick`) that Databricks' own `asOf` shares with its own
  words. `CarrierStrategies` (identity under Trino's caps), `StarExceptToColumns`, the three `ExplosionMarkers` passes,
  `CheckedDefectsToLists`, `SubstringClamp`, `AggregatesDeliverDouble` over AVG, `QualifyToSubselect` by outputs,
  `ConstantKeysAsExpressions()` and `RootNumericTypes` were ported with the dialects before it.

## Identity

- `rowsAsJson`: the select as it came unless a ROW column is cast to its declared ROW and then to JSON; then `new
  SqlSelect` over the new projections (`build::select`, the outputs recomputed), each cast column `new Projection`
  (its output normalized). The marker select and the offset's `withLimit(null).withOffset(null)` go through the
  constructors too.
- `PivotAsGroupBy`: every source as it came (`Rw::Same`) but a static pivot of one ON key, which becomes a new
  `Subselect` over `SqlSelect.starOf(source).withProjections(...).withGroupBy(...)`.
- `AsOfAsPick`: every select as it came but one over an as-of join, `withFrom(new Join(..., LEFT, on))` then
  `withQualify(pick)`; the window and the pick built through their constructors (`build::window_call`, `build::call`),
  the extreme window one node twice (Java shares the object; nothing reads it by identity).

## Java's behavior reproduced that looks like a bug

Each is Java's text or refusal as it is, reproduced; none is fixed here.

- A quantified comparison by IS_DISTINCT_FROM (`x IS DISTINCT FROM ANY (subquery)`) is a `NullPointerException`,
  `quantified comparison must be an infix operator: IS_DISTINCT_FROM` (the base's infix lookup answers no operator; in
  the arms oracle). Databricks writes its own quantified comparison and refuses it by name.
- A list call whose lambda is no lambda (`LIST_TRANSFORM` or `LIST_FILTER` over a column, a shape no lowering builds;
  likewise `LIST_EXISTS` and `LIST_FOR_ALL`) is Java's `ClassCastException` from `(SqlExpr.Lambda) args.get(1)`, not a
  refusal by name; the arms oracle could not hold its message (below).
- `floatLiteral`'s fallback past a DECIMAL's 38 digits (`Double.toString`) can never fire: `plainFloat` writes plain
  digits only for a magnitude from 10^-6 to below 10^15, at most 17 significant digits -- never 39 places. Ported as
  written; unit-tested on the plain path.
- `hashSigned` of no argument writes `concat_ws(chr(31), )` (unit-tested).

## What Java does that this does not reproduce exactly

None of these reaches the oracles.

- **A `ClassCastException`'s message.** Java's names the JVM's class loader (`... (com.legend.sql.SqlExpr$Column and
  com.legend.sql.SqlExpr$Lambda are in unnamed module of loader 'app')`), so the arms oracle dropped those answers
  (PHASE1.md, 6a: LIST_TRANSFORM without a lambda on Trino and Databricks). Rust refuses with Java's class
  (`java.lang.ClassCastException`, `CLASS_CAST` at the seam) and Java's message less that clause: `class
  com.legend.sql.SqlExpr$Column cannot be cast to class com.legend.sql.SqlExpr$Lambda` (`src/trino/tests.rs`).
- `Character.digit` of a replacement's group digit is its place in its script's run of ten: the JDK's own (`isDigit`'s
  37 runs in the BMP each a multiple of ten long, read from `char-classes.tsv`, and Unicode assigns each run 0 to 9).
- The frame unit's `toUpperCase(Locale.ROOT)` is Rust's `to_uppercase`: the same casing, each from its own Unicode
  version.
- A lambda of too few parameters (a fold's) is refused in `ArrayList`'s words (`Index 1 out of bounds for length 1`).
- The seam refuses `duckdb.*` and `engine.*` on Trino: ops no Java method of Trino answers.

# Databricks

Step 6b (docs/rust-port/PHASE1.md §4): Java's `Databricks` (`src/databricks.rs`; `src/databricks/functions.rs` the
`call` and `listCall` tables and their helpers, `src/databricks/aggregates.rs` the bit operators, the aggregates and
windows, the frame bounds, the collected lists and the date format). Every pass it installs was ported before it.

## The numbers

**2,631 of 2,631 distinct inputs identical (100 %)**; the floor (`tests/databricks-floor.txt`) is 2,631. The oracles hold
2,652 Databricks lines (2,597 of `mir/dialect-arms.jsonl`, 26 of the dialects dump, 29 of the first; 21 repeat an
input, none with two answers). The SQL, the binds and the regions are compared exactly: 25 statements bind a parameter
or write a region and carry Java's record text. All 359 of Java's refusals are reproduced, class and message (229
`DialectCapability`, 130 `IllegalStateException`), and Rust refuses nothing Java rendered. **The first measurement was
already 100 %.**

| op | identical | different | refused (Java rendered) | accepted (Java refused) |
|---|---:|---:|---:|---:|
| `ddl.render` | 47 | 0 | 0 | 0 |
| `dml.render` | 30 | 0 | 0 | 0 |
| `dml.renderStatement` | 30 | 0 | 0 | 0 |
| `fromEngineText` | 4 | 0 | 0 | 0 |
| `query.render` | 76 | 0 | 0 | 0 |
| `query.renderStatement` | 2,444 | 0 | 0 | 0 |

By MIR kind every one is identical: `SqlSelect` 2,511, `CreateTable` 38, `InsertValues` 24, `DeleteAll` 8,
`CreateTempTableAs`, `DeleteWhere` and `Update` 6 each, `DropTable` and `SqlUnion` 5 each, `InsertSelect`, `Merge`,
`SqlWith` and `String` (the engine text, every one refused) 4 each, `CreateSchema`, `DropSchema` and `InsertFromTable` 2
each.

The arms oracle reaches every `SqlFn` arm, every `SqlAgg.Fn` of the exhaustive `reducer`, every own wall and every pass
that changes a query (PHASE1.md, 6a). What it cannot hold is unit-tested from the Java (`src/databricks/tests.rs`):
`listCall`'s unreached arms and `bitOp`'s default, the two answers that are Java's `ClassCastException` (a list lambda
that is no lambda; a pivot of AVG, below), the session's temporary view (no oracle line asks `sessionTable`), the frame
intervals and a RANGE offset in its key's type, the index lambda read 1-based, the split's `Pattern.quote`, the string
literal's escapes and NUL, the identifier rule and the pass list.

The seam answers every op of them as a direct call does: 2,631 ops and 76 inline renders (2,145 statements read back
from their JSON equal and with Java's record text, 364 refusals).

## What is ported

- **The dialect**: every one of `Databricks.java`'s 58 overrides -- the lexicon (backticks, ANSI mode's reserved
  words), the scalar (`STRING`, `VARIANT`, `INT`; TIME refused), array, map, STRUCT and DDL type tables, the session
  (`SET TIME ZONE 'UTC'`, `SET ANSI_MODE = true`), no keys snapshot and no column limit, the static pivot, the session
  table as a temporary view over its rows, `fromEngineText` refused, the passes, the as-of join's select (`asOf`: the
  shared pick with Databricks' own refusals), several unnests as one `inline(arrays_zip(...))`, native QUALIFY and
  `SELECT * EXCEPT`, the refused row order and as-of clause, the unparenthesized VALUES (a column of NULLs alone typed
  in its first row), the native PIVOT (a value's column named `v__|__agg`, several aggregates' aliases `_|__agg`),
  every identifier in backticks (a double-quoted declaration its inner name), the string literal's backslash escapes
  and NUL, the explicit labels, a list of no element type exploded as its declared type and the typed NULL or raise, the
  CASE's raise cast to its slot, `L` integer literals, the ordered list aggregate as a sorted `collect_list`, the
  quantified comparison's truth table, `array(...)`, `named_struct` (a composite member's NULL typed), the field read,
  the lambdas and the 1-based index (`oneBasedIndex`, `substitute`), every list call (the moments as rows reduced, the
  list's variance and deviation as DuckDB's own update), the list predicates, membership, the collection reductions,
  `explode`, the fold as `aggregate`, Pure's splitPart over the quoted split, `array_compact`, `struct_insert`, the
  VARIANT (`to_variant_object`, navigation through a map or an array of VARIANT, `parse_json`, `to_json`, a double's
  text in lower case), the whole `call` table with its helpers (the regular expressions' first match, the
  replacement's groups, the trims' standard form, `make_timestamp`, `strftime`'s year unsigned, the date parts,
  truncations, `timestampadd`, the differences by the floored epoch, the time buckets, `map_concat` keeping a key's last
  value, the JSON type by `schema_of_variant`, Jaro-Winkler as nested `aggregate`s), `xxhash64`, `bround`, the bit
  operators, every aggregate (`listagg`, the percentiles `WITHIN GROUP` with the discrete one cast, `max_by`), the
  windows with their own OVER clause (a ranking window ordered by 1, an INT argument, a RANGE offset in its key's
  type, the intervals as DAY TO SECOND or YEAR TO MONTH), the JSON constructors, the date format, and the JSON kind
  and TYPEOF names.
- **The shared renderer**: the `values_source` hook takes the source's declared outputs too (Java's `SqlSource.Values
  v`), which Databricks' typed NULL column reads; DuckDB's, H2's, the engine text's, Postgres's, SQLite's and Trino's
  output unchanged.

## Identity

- `asOf`: Java's static helper, called at render for a select over an as-of join; the pick built as `AsOfAsPick`
  builds it (`shared_passes::as_of_pick`), the select rendered again.
- `oneBasedIndex`: a lambda of two parameters becomes `new SqlExpr.Lambda([x, i__0], body)`, its body's every
  unqualified reference to `i` replaced by `i__0 + 1` (`substitute`: a node rebuilt through `withChildren` only where
  a child changed, `carrier::map_children`); a lambda of one parameter is itself.
- The walk's pivot USING: a reducer the passes rewrite into another node (`AggregatesDeliverDouble`'s cast of AVG) is
  Java's `ClassCastException` from `(SqlAgg.Reducer) rewriteExpr(u.agg())` (below).

## Java's behavior reproduced that looks like a bug

Each is Java's text or refusal as it is, reproduced; none is fixed here.

- A pivot whose aggregate is AVG (a shape lowering builds) is a `ClassCastException`: `AggregatesDeliverDouble` wraps
  the pivot's reducer in a cast to DOUBLE, which the walk reads back as a reducer (`(SqlAgg.Reducer)
  rewriteExpr(u.agg())`). Its message names the JVM's class loader, so the arms oracle dropped it (below).
- A list call whose lambda is no lambda (`LIST_TRANSFORM`, `LIST_FILTER`) is a `ClassCastException`, as on Trino.
- Several unnests side by side are checked for every clause but HAVING: a FROM-less select of unnests with a HAVING
  writes `SELECT inline(arrays_zip(...)) AS (...)`, its HAVING dropped (unit-tested).
- `LIST_EXISTS` and `LIST_FOR_ALL` write a lambda of the element and its index as it is (0-based, unlike `filter` and
  `transform`'s 1-based reading; Spark's `exists` and `forall` take the element alone, and Trino refuses it by name).
- A frame interval of a negative count writes each field signed (`INTERVAL '0 -1:00:00' DAY TO SECOND`), Java's
  `String.format("%02d")` of a negative number (unit-tested).

## What Java does that this does not reproduce exactly

None of these reaches the oracles.

- **A `ClassCastException`'s message**, as Trino's: Java's class and its message less the class-loader clause --
  `class com.legend.sql.SqlExpr$Column cannot be cast to class com.legend.sql.SqlExpr$Lambda`, and the pivot's `class
  com.legend.sql.SqlExpr$Cast cannot be cast to class com.legend.sql.SqlAgg$Reducer` (`src/databricks/tests.rs`).
- `Character.digit` of a replacement's group digit, and the frame unit's `toUpperCase(Locale.ROOT)`, as Trino's.
- A lambda of too few parameters (a fold's) is refused in `ArrayList`'s words.
- The seam refuses `duckdb.*` and `engine.*` on Databricks: ops no Java method of Databricks answers.

# ClickHouse

Step 6c (docs/rust-port/PHASE1.md §4): Java's `ClickHouse` (`src/clickhouse.rs`; `src/clickhouse/functions.rs` the
`call` and `listCall` tables, the list reductions, the several-unnests select, the variant hooks and the temporal arms,
`src/clickhouse/aggregates.rs` the aggregates and windows -- the collected lists, the RANGE frames by epoch milliseconds,
at the key's edge and fractional -- the bit operators, the JSON constructors, the date formats, the DDL and the wire
names), and the two passes only ClickHouse installs, in `src/clickhouse/passes_own.rs`: its nested
`ArrayJoinColumnsNamed` and `OuterJoinResidualsAsMatches`.

## The numbers

**2,643 of 2,643 distinct inputs identical (100 %)**; the floor (`tests/clickhouse-floor.txt`) is 2,643. The oracles
hold 2,672 ClickHouse lines (2,598 of `mir/dialect-arms.jsonl`, 37 of the dialects dump, 37 of the first; 29 repeat an
input, none with two answers). The SQL, the binds and the regions are compared exactly: 25 statements bind a parameter or
write a region and carry Java's record text. All 415 of Java's refusals are reproduced, class and message (291
`DialectCapability`, 123 `IllegalStateException`, one `NullPointerException`: a quantified comparison by
IS_DISTINCT_FROM, below), and Rust refuses nothing Java rendered. The first measurement was 2,607 of 2,643: the 36
ADD_INTERVAL statements, two slips in the date-range guard (a parenthesis, and the raise's message read as the raise
itself), fixed.

| op | identical | different | refused (Java rendered) | accepted (Java refused) |
|---|---:|---:|---:|---:|
| `ddl.render` | 48 | 0 | 0 | 0 |
| `dml.render` | 30 | 0 | 0 | 0 |
| `dml.renderStatement` | 30 | 0 | 0 | 0 |
| `fromEngineText` | 4 | 0 | 0 | 0 |
| `query.render` | 86 | 0 | 0 | 0 |
| `query.renderStatement` | 2,445 | 0 | 0 | 0 |

By MIR kind every one is identical: `SqlSelect` 2,522, `CreateTable` 39, `InsertValues` 24, `DeleteAll` 8,
`CreateTempTableAs`, `DeleteWhere` and `Update` 6 each, `DropTable` and `SqlUnion` 5 each, `InsertSelect`, `Merge`,
`SqlWith` and `String` (the engine text, every one refused) 4 each, `CreateSchema`, `DropSchema` and `InsertFromTable` 2
each.

The arms oracle reaches every one of the 181 `SqlFn` arms, every `SqlAgg.Fn` of the exhaustive `reducerIn`, every own
wall and every pass that changes a query (PHASE1.md, 6a: `CarrierStrategies` changes none where lists are native). What
it cannot hold is unit-tested from the Java (`src/clickhouse/tests.rs`): `listCall`'s LIST_REVERSE, RANGE_FN,
REPEAT_VALUE and default arms, `bitOp`'s and `temporal`'s defaults, `outsideDates`' units refused before it and
`jodaText`'s millisecond field (their one caller passes none of them), a list lambda that is no lambda (Java's
`ClassCastException`, dropped from the oracle: below), the date range and `Integer.parseInt`'s overflow, the literals,
the identifier rule, the per-render SETTINGS state, the wire names, the pass list, and `OuterJoinResidualsAsMatches`'
matches numbered per render and its refusal of a nested right side (below).

The seam answers every op of them as a direct call does: 2,643 ops and 86 inline renders (2,099 statements read back
from their JSON equal and with Java's record text, 430 refusals).

## What is ported

- **The dialect**: every one of `ClickHouse.java`'s 66 overrides -- the lexicon, the scalar (`Nullable(T)` throughout;
  TIME refused), array, map (its key never nullable), Tuple (a struct field its `Variant`) and DDL type tables, no
  session statements, no keys snapshot and no column limit, the static pivot, the session table (the default's two
  statements, refused when not temporary), `fromEngineText` refused, the passes, the root's SETTINGS clause (its two
  per-render counters below), the sources (a lateral marker as an `ARRAY JOIN` of its lists, several padded with NULL
  to the longest; an as-of join's lone inequality keyed by its operands' `cityHash64` times zero; an outer join reading
  one side re-keyed by `if(cond, 0, 1)`; a lateral filter as a join on its correlated conjuncts, each inner column read
  as the subquery's output; a null-safe join key as `isNotDistinctFrom`; any other lateral refused), native QUALIFY and
  `SELECT * EXCEPT`, a VALUES as the `values` table function (a Decimal column widened to its literals' scale), the
  refused row order and PIVOT, every identifier quoted, the string literal's escapes and NUL, the dates in range, the
  timestamp's six digits, the Float literal as an exact `Decimal(p, s)`, `toInt128` and `toInt64` literals, the
  explicit labels and the typed Boolean, NULL or raise in a declared slot, the CASE's raise cast (a list-typed one an
  `arrayResize`), the correlated subqueries (IN as its three-valued EXISTS, a quantified comparison answered over no
  rows first, a correlated EXISTS checked null-rejecting, an EXISTS of constants selecting the rows' columns), the
  ordered list aggregate, the empty typed list, the list literal's Decimal and Float64 unified, the Tuple literal, the
  field read (a struct field through `variantElement`), the lambdas, every list call, the list predicates, membership,
  the collection reductions (an interpolated quantile at a level that is no literal), `arrayJoin`, the zipped ARRAY
  JOIN of several unnests, the fold as `arrayFold`, Pure's splitPart, `struct_insert` as a typed Tuple, the Variant as
  its JSON text (a list read from a JSON null kept NULL, navigation, every cast), the whole `call` table with its
  helpers (the raise by a failed cast, the guarded remainders, the regular expressions, printf's conversions, the date
  parts, truncations, intervals guarded by ClickHouse's years, differences, time buckets and formats), the signed hash,
  banker's ROUND, the bit operators, every aggregate in its `OrNull` form (the sample statistics NULL under two values,
  the quantiles and the mode over the group's values, the collected lists sorted by their keys), the windows (`lag` and
  `lead` in frame, `cume_dist` as counts, a count or a rank an Int64, the RANGE frames), the JSON constructors, the
  formatDateTime and Joda formats, the JSON kind and TYPEOF names, `wireTypeName` and `deliversOwn`.
- **The passes** (`src/clickhouse/passes_own.rs`): `ArrayJoinColumnsNamed` (a star over a lateral explosion followed by
  its exploded columns, each labelled with its own name) and `OuterJoinResidualsAsMatches` (a LEFT join whose condition
  holds a both-sided conjunct that is no equality, as the left side LEFT-joined null-safely to its matches: each distinct
  tuple of the left columns the condition reads INNER-joined to the right side; its helper names `__oj<n>` numbered
  from 0 at each root). `CarrierStrategies`, `PivotAsGroupBy`, the explosion markers, `CheckedDefectsToLists`,
  `SubstringClamp`, `AggregatesDeliverDouble` over AVG, `ConstantKeysAsExpressions()` and `RootNumericTypes` were
  ported with the dialects before it.
- **The shared renderer**: the `date_lit`, `timestamp_lit` and `array_type_name` hooks may refuse (ClickHouse's dates
  outside 1900 to 2299; BigQuery's list of lists); every other dialect's output unchanged.

## Identity and state

- Java's two per-render counters, `nesting` (how deep the writer is in `query`: the root writes the SETTINGS clause) and
  `correlatedExists` (whether the root holds a correlated subquery, set by the subqueries it writes), are `Cell`s of the
  dialect object, reset where Java resets them: the flag at the root's entry, the depth decremented whether or not the
  query wrote. A subquery written inline is no root; nothing outlives a render.
- `ArrayJoinColumnsNamed`: every select as it came but one over a lateral marker, rebuilt through `new SqlSelect` (its
  outputs recomputed), each named column `new Projection` over `Column.derived`; the markers by alias in a
  `LinkedHashMap`'s order (a repeated alias keeps its place, its last projections).
- `OuterJoinResidualsAsMatches`: a join rebuilt only where its left side changed or it is matched (`Rw::Same`
  otherwise); the left columns the condition reads kept once each in their first order (Java's `LinkedHashMap` keyed by
  the column record), the condition's substitution keyed by expression equality (Java's `HashMap.get`), a node rebuilt
  through `withChildren` only where a child changed.
- `TablesRead` and `InnerAsOutput` are the rewriter's whole walk (subqueries included), as Java's `SqlRewriter`
  subclasses; `conjuncts` flattens ANDs (and, in the pass, groups) as each Java class does.

## Java's behavior reproduced that looks like a bug

Each is Java's text or refusal as it is, reproduced; none is fixed here.

- A quantified comparison by IS_DISTINCT_FROM is a `NullPointerException`, `quantified comparison must be an infix
  operator: IS_DISTINCT_FROM`, after ClickHouse has written its `(CASE WHEN (SELECT count() ...` (the base's infix
  lookup answers no operator; in the arms oracle).
- A list call whose lambda is no lambda (`LIST_TRANSFORM`, `LIST_FILTER`, `LIST_EXISTS`, `LIST_FOR_ALL` over a column, a
  shape no lowering builds) is Java's `ClassCastException`, not a refusal by name; the arms oracle could not hold its
  message (below).
- `OuterJoinResidualsAsMatches` reads `j.right().alias()` before it checks the right side is no join: a select whose FROM
  joins a nested join on its right is refused, whatever the join's kind, with `IllegalStateException`: `a nested join
  has no single alias — resolve per side` (unit-tested).
- `aliasesOf` asks a FROM-less select for its alias: an EXISTS, a one-column IN or a quantified comparison over a
  FROM-less subquery, and a lateral filter over one, is `IllegalStateException`: `a Dual (FROM-less) source has no alias
  — caller bug`.

## What Java does that this does not reproduce exactly

None of these reaches the oracles.

- **A `ClassCastException`'s message**, as Trino's and Databricks': Java's class (`CLASS_CAST` at the seam) and its
  message less the JVM's class-loader clause, `class com.legend.sql.SqlExpr$Column cannot be cast to class
  com.legend.sql.SqlExpr$Lambda` (`src/clickhouse/tests.rs`).
- The fractional RANGE frame's refusal names the window function by its record `toString`: exact for a ranking or value
  function of no argument (`RankingFn[fn=RANK, args=[]]`, the oracle's one); an argument or a reducer is written by its
  simple name (`Column[...]`, `orderBy=[...]`), where Java writes each record whole.
- The frame unit's `toUpperCase(Locale.ROOT)` is Rust's `to_uppercase`, as Trino's.
- The seam refuses `duckdb.*` and `engine.*` on ClickHouse: ops no Java method of ClickHouse answers.

# BigQuery

Step 6c (docs/rust-port/PHASE1.md §4): Java's `BigQuery` (`src/bigquery.rs`; `src/bigquery/functions.rs` the `call`
table and its helpers, the casts and texts of a value, the maps, the regular expressions and the temporal helpers,
`src/bigquery/aggregates.rs` the bit operators, the aggregates and windows -- the grouped quantiles, the analytic
PERCENTILE_* and the descending one, `bqOver` -- the DDL and its types), and the pass only BigQuery installs,
`FoldsAsRecursiveCtes`, in `src/bigquery/folds.rs`.

## The numbers

**2,643 of 2,643 distinct inputs identical (100 %)**; the floor (`tests/bigquery-floor.txt`) is 2,643. The oracles hold
2,673 BigQuery lines (2,599 of `mir/dialect-arms.jsonl`, 35 of the dialects dump, 39 of the first; 30 repeat an input,
none with two answers). The SQL, the binds and the regions are compared exactly: 24 statements bind a parameter or write
a region and carry Java's record text. All 444 of Java's refusals are reproduced, class and message (310
`DialectCapability`, 134 `IllegalStateException`), and Rust refuses nothing Java rendered. **The first measurement was
already 100 %.**

| op | identical | different | refused (Java rendered) | accepted (Java refused) |
|---|---:|---:|---:|---:|
| `ddl.render` | 48 | 0 | 0 | 0 |
| `dml.render` | 31 | 0 | 0 | 0 |
| `dml.renderStatement` | 31 | 0 | 0 | 0 |
| `fromEngineText` | 4 | 0 | 0 | 0 |
| `query.render` | 83 | 0 | 0 | 0 |
| `query.renderStatement` | 2,446 | 0 | 0 | 0 |

By MIR kind every one is identical: `SqlSelect` 2,520, `CreateTable` 39, `InsertValues` 26, `DeleteAll` 8,
`CreateTempTableAs`, `DeleteWhere` and `Update` 6 each, `DropTable` and `SqlUnion` 5 each, `InsertSelect`, `Merge`,
`SqlWith` and `String` (the engine text, every one refused) 4 each, `CreateSchema`, `DropSchema` and `InsertFromTable` 2
each.

The arms oracle reaches every one of the 181 `SqlFn` arms, every `SqlAgg.Fn` of the exhaustive `reducer`, every own
wall but one and every pass that changes a query (PHASE1.md, 6a). What it cannot hold is unit-tested from the Java
(`src/bigquery/tests.rs`): the NULL list of unknown element type (a `NullLit` is always bottom-typed, so no built
statement holds one; the wall is reached here with an untyped `NullLit` made by hand), `maps`' and `bitOp`'s defaults
(their one caller passes none), the closed fold hoisted (and the fold that reads its row, and over the NULL list), the
regular-expression rewrites (`onlyGroup`, `shiftGroups` with a non-ASCII digit, FORMAT's conversions), the literals and
identifiers, the frame offsets in a key's days and their overflow, the parameter types, the wire names and the pass
list.

The seam answers every op of them as a direct call does: 2,643 ops and 83 inline renders (2,063 statements read back
from their JSON equal and with Java's record text, 450 refusals).

## What is ported

- **The dialect**: every one of `BigQuery.java`'s 66 overrides -- the lexicon, the scalar (`INT64`, `FLOAT64`,
  `BIGNUMERIC`, `DATETIME` and `TIMESTAMP`, `JSON`), array (a list of lists refused), map (its entries,
  `ARRAY<STRUCT<key, value>>`), STRUCT and DDL type tables, no session statements, no keys snapshot, 10,000 result
  columns, the static pivot, `fromEngineText` refused, the bound Float text and print forms, the passes, native QUALIFY
  and `SELECT * EXCEPT`, the refused row order, as-of clause and PIVOT, `render` and `renderStatement` keeping the
  statement's labels, `UNION DISTINCT` and the parenthesized branches, the selects (a marker's reads over its
  positions, a FROM-less select with a filter over one row, an OFFSET without a LIMIT the largest one, a MODE counted
  in a select over its source, the markers in scope and the stars over them spelled, the root's NULL by shape typed as
  its label), the sources (a marker over `UNNEST(GENERATE_ARRAY(...))`, a lateral one joined `CROSS JOIN` or `LEFT JOIN
  ... ON TRUE`, any other lateral derived table an unnested array of structs), the VALUES as a typed array of structs,
  every identifier in backticks with its escapes, the string literal's escapes (`\xHH`, `\x3f`), the DATETIME literal's
  six digits, the BIGNUMERIC literals, the explicit labels, the raise in a declared slot (a DATETIME's through text),
  the typed NULL in a composite or decimal slot, the CASE mixing text cast to STRING, the list literal typed (a Float
  list of exact literals BIGNUMERIC), the STRUCT literal typed when declared, the field read, `struct_insert`, the
  quantified comparison counted, every list operation over `UNNEST(xs) WITH OFFSET` (lambda bodies with the 1-based
  index and the JSON element typed), membership, the collection reductions, Pure's splitPart, the JSON Variant
  (`TO_JSON`, `PARSE_JSON`, the subscripts, the member's text, a Variant as a list or a map, every cast through text,
  `floatText`), the whole `call` table with its helpers (the composite equalities by JSON text, the NULL-skipping
  GREATEST and LEAST, the remainders through BIGNUMERIC, DuckDB's roundings, the regular expressions with one group
  capturing and the first-match replace, printf's `%s` as text, the maps with their key checks, the date parts,
  truncations, intervals, differences, time buckets and formats), FARM_FINGERPRINT, banker's ROUND over a BIGNUMERIC,
  the bit operators (an arithmetic right shift), every aggregate (the grouped quantiles over the sorted values, the
  ordered string and list aggregates, MAX_BY and MIN_BY), the windows (AVG as FLOAT64, the analytic PERCENTILE_*, the
  descending quantile over the negated values, a RANGE frame over a date's days or a date-time's microseconds), the
  JSON constructors, the FORMAT_DATETIME codes, the type holes, the GoogleSQL DDL, `DELETE ... WHERE TRUE`, the JSON
  kind and TYPEOF names, `wireTypeName` and `deliversOwn`.
- **The pass** (`src/bigquery/folds.rs`): `FoldsAsRecursiveCtes` -- a closed fold (or `list_reduce`) as a recursive
  CTE `__fold<n>` prepended at the root, read as its last accumulator; over the NULL list its init; a fold inside
  another hoisted first. `CarrierStrategies` (under BigQuery's caps: lists not native, it rewrites), `SubstringClamp`,
  the three explosion-marker passes, `AggregatesDeliverDouble` over AVG, `ConstantKeysAsExpressions()` and
  `CommonSubexpressions` were ported with the dialects before it, as the bound expansions of a Float's text and print
  forms (`binds_float_text`, `binds_float_print`: BigQuery is the first Rust dialect to bind the print form).
- **The shared renderer**: `ansi::stars_as_source_columns` (Java's `starsAsSourceColumns`, which MariaDB and MySQL use
  too).

## Identity and state

- Java's three per-render stacks are `RefCell` stacks of the dialect object: `roots` (pushed by `render` and
  `renderStatement` with the query's outputs before the passes, popped after), `laterals` (each select's markers in
  scope, innermost last; a column's read looks from the innermost out, as Java's `ArrayDeque` iterates) and
  `modeCounts` (each select's counted MODEs). Each is pushed and popped where Java's is, whether or not the select
  wrote; what a scope holds is cloned out before it is written, so no borrow outlives a write.
- A counted MODE is found by Java's record equality: a grouped one is its reducer; a windowed one its window's
  function, partition, order and frame (its type fact is its function's, as the typing differential holds).
- `FoldsAsRecursiveCtes` keeps the reads it made in an identity set, which its closedness test consults; Rust keeps them
  by value: a read is `SELECT __acc FROM __fold<n> ORDER BY __i DESC LIMIT 1` over the pass's own CTE name, which no
  other node of a statement equals.
- A STRUCT literal's own type (Java's `typeOf(st)`) is recomputed from its fields (`typing::struct_lit_type`), as its
  constructor computed it.

## Java's behavior reproduced that looks like a bug

Each is Java's text or refusal as it is, reproduced; none is fixed here.

- `modesCounted` asks the select's FROM for its alias before its own check: a MODE over a join is
  `IllegalStateException`, `a nested join has no single alias — resolve per side` (in the arms oracle), and over a
  FROM-less select with no filter or grouping `a Dual (FROM-less) source has no alias — caller bug` (unit-tested), where
  `a MODE over a Join reached BigQuery` was meant.
- A DISTINCT MODE of one argument is refused as `a mode of 1 arguments reached BigQuery` (in the arms oracle).
- `rootNulls` reads the statement's labels for every select written at depth 0 -- a subquery written inline (an EXISTS,
  an IN, a scalar subquery) is one -- so such a subquery with as many columns as the root, one of them NULL by its
  shape, has that column cast to the root's label of its position.

## What Java does that this does not reproduce exactly

None of these reaches the oracles.

- The frame unit's `toUpperCase(Locale.ROOT)` is Rust's `to_uppercase`, as Trino's.
- The seam refuses `duckdb.*` and `engine.*` on BigQuery: ops no Java method of BigQuery answers.

# Typing

Over the oracles, every distinct statement (every dialect's, the ten dialects' of the dialects dump, the eight
dialects' of the arms oracle and the engine-text ops' queries included: one tree, counted once): **4,057,096 facts of
36,890 statements, 0 different** (4,045,363 of 34,069 before the arms oracle was read, step 6b; the same with
ClickHouse and BigQuery ported, step 6c: the oracles are unchanged).

# Tests

- `src/pass_tests.rs`: each pass's rewrite over a hand-built tree from its Java class's logic, with the identity cases
  (an untouched tree `Same`, a clamp's parent rebuilt with its fact recomputed and its untouched sibling not, the
  rebuilt `Lambda` and `ArrayLit` facts), refusals in Java's words, and the typing rules' own receipts (the decimal
  literal and arithmetic matrix, CASE and reducer nullability, the one-row proof, pad neutralization, `HashMap` order).
- `src/scan_order.rs` (9 tests): a positional cap over a join of scans keyed by each scan's `rowid` (one firing), a
  joined subselect adding no key, a user sort, a GROUP BY, DISTINCT, an aggregate, a single table or a frame name left
  alone with no firing, a cap wrapper ordering its inner, an order-sensitive `STRING_AGG` keyed over a scan, over a
  sorted frame (`__agg_ord0`, `__agg_key0`, their slots reconciled) and over a UNION ALL frame (`__agg_leg`,
  `__agg_legord`), a frame CTE threading its ordinals only when a reader needs order, and DuckDB's pipeline with
  `engine_scan_order` writing `ORDER BY t0.rowid NULLS LAST, t1.rowid NULLS LAST`.
- `src/carrier/tests.rs` (28 tests) and `src/json.rs` (4): `CarrierStrategies` under DuckDB's caps handing back the
  very input at every hook, and under H2's every rule -- the FULL OUTER emulation, each explode placement, ASOF, the
  static PIVOT as GROUP BY, the literal folds (a literal `LIST_SUM` cast to HUGEINT; a literal `LIST_PRODUCT` written
  `CAST(... AS DOUBLE)` because its leading `1.0` is a DECIMAL(2,1), as Java's), the collect fusions, the JSON literal
  navigation, membership -- each rewritten tree rendered without passes and held to the Java's logic; and Java's JSON
  reading and canonical text.
- `src/seam/tests.rs` (18 tests), `src/host_ops.rs` (6), `src/catalog_model.rs` (3), `src/wire_names.rs` (1): every op
  of the seam through `dispatch`, each answer held to the Java's logic -- the facts, the renders, `sessionTable`, the
  names, `raw` and the engine text, the wire names, the script, every catalog decision and refusal, the catalog
  question's text, the copy's select list, each host operation's statements and the browser reader's every refusal,
  `SqlDialect`'s defaults (on a dialect that overrides none), the records' JSON forms read back through their
  constructors, the refusal kinds, and a MIR exactly at MirJson's depth limit.
- `src/h2/tests.rs` (25 tests): H2's and H2Modern's hooks on hand-built nodes, from their Java -- the alias, reference
  and table-name rules and the refusals of an origin-less or per-key reference, the lexicon, every own spelling of
  `call` and its refusals by name, a boolean compared with text, the format template's concatenation and its three
  refusals, `split_part` (which the oracles hold none of) and `regexp_extract`, Pure's splitPart, `translate`, the
  aggregates (MIN for `first()` and its refusals naming the type, the WITHIN GROUP percentiles, the JSON list, the
  ordered NULL placement), banker's ROUND, the JSON carrier, the JSON-parsing and boolean-text casts, H2Modern's
  navigation and its navigation cast, the frame intervals (a week as 7 days), the date format, the placeholders and
  type holes, a projection labelled from its declared output, the DDL types, the staged temporary table, the catalog
  grid, and the pass list.
- `src/h2_passes/tests.rs` (9 tests): `QualifyToSubselect`'s wrap; `ConstantLateralsAsRows` over scalar elements, a NULL
  list (`WHERE FALSE`), struct elements read by field, and each case it leaves as it came; `LateralExplodeToUnion`'s
  keyed union, its null-dropping singleton and CROSS lateral, what it leaves alone, and its refusal of a list of no
  element type; `AggregatesDeliverDouble` (a windowed AVG not cast); `SourceSpelling`'s re-stamp and what it keeps.
- `src/seam/h2_tests.rs` (9 tests): every op of the seam for H2 and H2Modern (§3a of `SEAM.md`): the facts, `forServer`
  and a spec's `server`, the renders and a type hole's JSON, the DDL and a session table, the names, the engine text,
  the interface's defaults, `failingStatement`, the catalog and host refusals naming the dialect, and the DuckDB-only
  ops refused.
- `src/engine_style/tests.rs` (15 tests): the engine-text dialects' hooks on hand-built nodes, from their Java -- the
  alias plan (a self-join, a joined subselect numbered pre-order, a view frame, the DISTINCT root frame, an exists-key
  frame's bare keys, a plan variable, a correlated EXISTS), `renderedAlias` after a render and after `renderAsAliased`,
  the TDS join's wrap and its select list, a union's and a WITH's, the literals and references (`quoteIdentifiers`, the
  zone-shifted constants), every plan-parameter template, the enum selector and the optional parameter's null-safe
  equality (H2's and DB2's), the boolean text and every IN form, the date-difference and decode folds, H2's, DB2's and
  Composite's function spellings and their refusals (DB2's truncations and intervals the oracles hold none of), the
  list calls and joinStrings, the aggregates and windows, the clauses (top, fetch, the group keys, DB2's WHERE wrap),
  the casts, the DDL and the seed rows, and Java's bugs listed above.
- `src/java_time.rs` (6 tests) and `src/decode_shapes.rs` (1): `LocalDateTime`'s strict reading and its `toString`,
  the embedded tzdb's versions and counts, `ZoneId.of` with `SHORT_IDS` over every ID form (each measured on JDK 25),
  New York's gap and overlap and its last rules in 2500, Lord Howe's half hour, Dublin's rearguard summer, the shifts
  (Panama's local mean time, Arizona's 1967 summer), and the decode chain's branches and one source.
- `src/seam/engine_tests.rs` (6 tests): every op of the seam for the three (§3b of `SEAM.md`): the spec's
  configuration and its refusals, the facts, the renders and the refused statement, the DDL, seed rows and a session
  table, the names, the raw text and the interface's defaults naming each dialect, and the five engine ops (a null
  select list, a refused RAW splice, `renderedAlias` with and without the rendered query), refused on every other
  dialect.
- `src/postgres/tests.rs` (22 tests): Postgres's hooks on hand-built nodes, from its Java and the texts core's own
  Postgres tests pin -- the identifier rules (every name the query writes quoted, a store's bare name folded, a
  reserved or non-word name delimited, the stored table, the catalog name a bare declaration reaches), the references
  by origin and the refused row order and star exclusion, the NUL refusal, the truncated timestamp and the type hole,
  the typed list literal, the list calls the oracles hold none of and the empty collection, the scalar calls they hold
  none of, every wall and refusal by name, printf unrolled, the RE2 flags as ARE options, the exact remainder and the
  roundings, every aggregate and its shape refusals, the windowed Float cast and the ordered-set refusal, the frame
  units and their overflow, every cast branch (`NullVariantCastTest`'s typed NULL), the jsonb navigation and the
  struct read, the typed projections, the constant ORDER BY key (`PostgresDialectTest`), the DDL and the staged table
  (`TempTableAsTest`), the type tables and the date format, the stored reads (`StoredReadsTest`), `fromEngineText`,
  the catalog decisions (`PostgresCatalogTest`'s probe), the interface's answers, and the pass list.
- `src/shared_passes/tests.rs` (7 tests): `StarExceptToColumns`' spelled star and its identity;
  `WholePartitionOrderedSets`' correlated copy (the filter remapped, the partition matched null-safely), what it
  leaves alone, a nested ordered set one level down; `StringCountsAsInt`'s casts and what it leaves; the
  `AggregatesDeliverDouble` cast; `QualifyToSubselect` by outputs (a hidden window column, a star when nothing is
  hidden, the DISTINCT and subquery refusals) and its plain form; `RootNumericTypes`' casts, the unbounded Decimal, the
  DISTINCT sort key, a union's branches.
- `src/seam/postgres_tests.rs` (7 tests): every op of the seam for Postgres (§3c of `SEAM.md`): the facts and
  `forServer`, the renders and a finer date-time's type hole, the DDL, the DML and a session table, the names and the
  engine text, the interface's answers and refusals naming Postgres, the catalog decisions, the host operations (a
  query counted, everything else refused by name), and `readsStored` and `tableName` for every dialect.
- `src/sqlite/tests.rs` (7 tests), `src/sqlite/functions.rs` (1), `src/sqlite/regex.rs` (6) and `src/sqlite/digests.rs`
  (1): SQLite's hooks where the oracles cannot reach them -- a non-finite Float literal (no Java FloatLit holds one), the
  state of one dialect object (the fresh names started over at each query render, a DML render counting on, the state
  popped on a refusal), a root select known by its content, the pass list, the DDL's storage classes, a lambda's struct
  parameter read as JSON; printf's conversions as Java's regular expression finds them; the regular expressions' tables
  (a literal, a search's any-prefix state, classes and case folds, bounded and lazy repetitions, the multi-line anchors,
  a find's tracked groups) and every refusal by name, `Integer.parseInt`'s and the unterminated named group's among
  them; the digests' constants and padding.
- `src/explosion_markers/tests.rs` (5 tests), `src/common_subexpressions/tests.rs` (9): the markers' predicates and
  exploded reads, each pass's rewrite and every case it leaves as it came (`Rw::Same`), `__unnest<n>` counted by the
  pass object; a repeated 40-node subtree bound once, the largest first then within it, a lambda's body within itself
  and never above it, a raise, a fresh value and an aggregate's projection left in place, the walks' and subqueries'
  weights, a bind of its own shape movable, equal selects sharing one rewrite, and Rust's tie order.
- `src/seam/sqlite_tests.rs` (7 tests): every op of the seam for SQLite (§3d of `SEAM.md`): the facts and `forServer`, the
  renders and a date's TEXT type hole, the DDL and DML and a session table, the names and `tableName`, the engine text
  refused, the wire names and `deliversOwn`, the interface's defaults and refusals naming SQLite, the host operations
  refused, `readsStored`, and the DuckDB and engine-text ops refused.
- `src/trino/tests.rs` (7 tests): Trino's hooks where the oracles cannot reach them -- `listCall`'s LIST_REVERSE,
  RANGE_FN, REPEAT_VALUE and default arms, `bitOp`'s default, a list lambda that is no lambda (Java's
  `ClassCastException`) and the index lambda over `zip_with`, the frame intervals (a week as its days, the overflow,
  the refusal), a replacement's groups (`\1` to `$1`, an Arabic-Indic digit, a fullwidth nine raised to 10), the
  literals (BIGINT, the plain and the extreme Float, a decimal past 38 digits, a big integer, the timestamp's space),
  the identifier rule and the type names, and the pass list.
- `src/databricks/tests.rs` (9 tests): Databricks' hooks where the oracles cannot reach them -- `listCall`'s and
  `bitOp`'s unreached arms, the two `ClassCastException`s (a list lambda that is no lambda, a pivot of AVG) and a
  native pivot, several unnests as one `inline(arrays_zip(...))` (a HAVING dropped, a WHERE refused), the session's
  temporary view (with rows, without, not temporary, a bound parameter), the frame
  intervals and a RANGE offset in its key's type, a ranking window ordered by 1, the 1-based index, the string
  literal's escapes and NUL, the identifiers, the quoted split (`Pattern.quote`, a delimiter's `\E`), a non-ASCII
  group digit, the scalar walls and the pass list.
- `src/shared_passes/tests.rs` (2 more): `PivotAsGroupBy`'s grouped CASE aggregation, the pivots it leaves as they came,
  an integer value's column, a value of no text and an aggregate of no argument refused; `AsOfAsPick`'s pick by MAX and
  by MIN, any other join left as it came, the refusals of no inequality and of a select that filters or windows.
- `src/seam/trino_tests.rs` (7 tests) and `src/seam/databricks_tests.rs` (5): every op of the seam for Trino and
  Databricks (§3e and §3f of `SEAM.md`): the facts and `forServer`, the renders (OFFSET before LIMIT, the literals),
  the DDL and DML, the session table (refused on Trino, a temporary view on Databricks), the names and `tableName`, the
  engine text refused, the wire names and `deliversOwn`, the interface's defaults and refusals naming each, the host
  operations refused, `readsStored`, and the DuckDB and engine-text ops refused.
- `src/clickhouse/tests.rs` (10 tests): ClickHouse's hooks where the oracles cannot reach them -- `listCall`'s
  LIST_REVERSE, RANGE_FN, REPEAT_VALUE and default arms, `bitOp`'s and `temporal`'s defaults, `outsideDates`' units and
  `jodaText`'s millisecond fields, the descending key of no type, a list lambda that is no lambda (Java's
  `ClassCastException`, for all four list calls) and the index lambda over `arrayEnumerate`, the literals (the escapes
  and NUL, the dates in and out of range, `Integer.parseInt`'s overflow, the timestamp's six digits, the exact Decimal,
  the extreme Float and NaN, a decimal past 76 digits, an Int128), the identifier rule and the type names, the root's
  SETTINGS (a second render and an inline one), the wire names, the pass list, and `OuterJoinResidualsAsMatches`'
  matches numbered from 0 at each render and its refusal of a nested right side, and a FROM-less subquery's refused alias.
- `src/bigquery/tests.rs` (9 tests): BigQuery's hooks where the oracles cannot reach them -- the NULL list of unknown
  element type and the bottom-typed empty list, `maps`' and `bitOp`'s defaults and the arithmetic right shift,
  `FoldsAsRecursiveCtes` (a closed fold hoisted, a fold that reads its row left, a fold over the NULL list its init),
  `onlyGroup`, `shiftGroups` (a non-ASCII digit's number), FORMAT's conversions, the literals and identifiers, a RANGE
  frame over a date's days with its NULLs moved, an offset over the key refused and its overflow, a ROWS frame's keys,
  the type holes, the wire names, `deliversOwn`, the column limit, TYPEOF refused, the pass list, a MODE over a FROM-less
  select (Dual's refusal) and an inline subquery's NULL typed by the root's label.
- `src/seam/clickhouse_tests.rs` (5 tests) and `src/seam/bigquery_tests.rs` (5): every op of the seam for ClickHouse and
  BigQuery (§3g and §3h of `SEAM.md`): the facts and `forServer`, the renders (the SETTINGS clause; an OFFSET without a
  LIMIT), the DDL and DML (a key NOT ENFORCED, a DELETE's WHERE TRUE), the session table (the default's two statements,
  refused when not temporary), the names and `tableName`, the engine text refused, the wire names and `deliversOwn`, the
  interface's defaults and refusals naming each, the host operations refused, `readsStored`, and the DuckDB and
  engine-text ops refused.
- `tests/dialect_differential.rs`: the differentials above, one per dialect, the typing test, and the seam's, one per
  dialect: every op of the dialect's inputs (and each `query.render` asked inline, and for an engine-text dialect each
  of its aliases after the render) answered through `dispatch` exactly as a direct call answers it, each statement read
  back from its JSON equal to the direct one with Java's record text -- DuckDB's 16,154 ops and 1,023 inline renders
  (13,011 statements, 10 refusals), H2's 21,847 and 1,813 (14,962 statements, 1,103 refusals), H2Modern's 394 and 17
  (352 statements, 20 refusals), EngineStyleH2's 2,105, 1,638 and 5,104 aliases (7 statements, 26 refusals),
  EngineStyleDB2's 56, 42 and 43 (20 refusals), EngineStyleComposite's 24, 14 and 11 (20 refusals), Postgres's 11,630
  and 42 (11,523 statements, 14 refusals), SQLite's 2,529 and 46 (2,189 statements, 150 refusals), Trino's 2,630 and 76
  (2,152 statements, 355 refusals), Databricks' 2,631 and 76 (2,145 statements, 364 refusals), ClickHouse's 2,643 and 86
  (2,099 statements, 430 refusals), BigQuery's 2,643 and 83 (2,063 statements, 450 refusals): all the same; and the
  zone shift held to java.time (above).

# Stack

The deepest DuckDB statement needs between 1 and 2 MB of stack in a debug build for its parse, read, passes and render
together, the deepest H2 statement (and the typing walk over both oracles) between 2 and 4 MB, and every one less than 1
MB in release (`LITE_DIALECT_STACK_MB` sets the test thread's; it defaults to 256 MB). The twenty-five tests (and the two
zone tests) run in 57 s under Bazel (`//rust/lite-dialect:dialect_differential_test`, `size = medium`: 300 s; 47 s
before Postgres's two, 54 s before SQLite's and before Trino's and Databricks' four with the arms oracle, 53 s before
ClickHouse's and BigQuery's four) and 15 s in release; `//rust/lite-sql:mir_round_trip_test` in 70 s. A MIR at MirJson's depth limit (2,045 nested calls) needs between 32 and
64 MB in a debug build and more than 1 MB in release; the seam's C export runs each call on a 256 MB thread (`SEAM.md`
§1).
