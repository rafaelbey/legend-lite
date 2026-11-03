# Dialect parity: DuckDB, H2 and H2Modern (2026-11-03)

Every dialect Rust answers, held to the JVM's own answers on the golden oracles (docs/rust-port/PHASE1.md §4): DuckDB
(step 3, its renderer and passes) and the H2 family's execution dialects, H2 and H2Modern (step 4b). Each is ported from
its Java class, over a walk that keeps Java's identity semantics and a port of the typing rules Java's constructors
apply. This file measures each on the oracles and says what of Java it does not reproduce exactly.

Measured by `tests/dialect_differential.rs` over `//rust/oracles:mir_dump` (`mir/mir-dump.jsonl`) and
`//rust/oracles:h2_dump` (`mir/h2-dump.jsonl`), an input both hold (the same dialect, op, configuration and MirJson)
counted once:

- `<dialect>_renders_as_java_does` (`duckdb_`, `h2_`, `h2modern_`): every distinct input of the dialect, answered by a
  fresh Rust dialect of that name (made by the seam's `open`, as the JVM's shim makes it) and compared with the fresh
  JVM dialect's answer -- the SQL text exactly, a statement's binds and regions as Java's record text, a refusal by its
  Java class and message; at least the dialect's floor (`tests/<dialect>-floor.txt`) identical;
- `typing_reproduces_javas_facts`: every node of every distinct statement of the oracles (all dialects: one tree), its
  type fact recomputed by `src/typing.rs` from its children's and compared with the fact Java's constructor stored;
  every projection frame's outputs, union's labels and join's delivered columns likewise;
- `the_seam_answers_as_<dialect>s_renderer_does`: every op of the dialect's inputs answered through the seam
  (`SEAM.md`) exactly as a direct call answers it.

```
cd rust
LITE_ORACLES="<mir-dump.jsonl> <h2-dump.jsonl>" cargo test --release -p lite-dialect --test dialect_differential -- \
  --ignored --nocapture
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
  (`... are in unnamed module of loader 'app'`); the walk's pivot-USING cast names `SqlExpr$SqlAgg$Reducer` where
  Java's is `com.legend.sql.SqlAgg$Reducer`, and a `DeferredTdsString` over a union names `SqlExpr$SqlUnion`.
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

# Typing

Over both oracles, every distinct statement (every dialect's, the engine-text ops' queries included: one tree, counted
once): **2,257,711 facts of 24,644 statements, 0 different.**

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
- `tests/dialect_differential.rs`: the differentials above, one per dialect, the typing test, and the seam's, one per
  dialect: every op of the dialect's inputs (and each `query.render` asked inline) answered through `dispatch` exactly
  as a direct call answers it, each statement read back from its JSON equal to the direct one with Java's record text --
  DuckDB's 16,154 ops and 1,023 inline renders (13,011 statements, 10 refusals), H2's 21,847 and 1,813 (14,962
  statements, 1,103 refusals), H2Modern's 394 and 17 (352 statements, 20 refusals): all the same.

# Stack

The deepest DuckDB statement needs between 1 and 2 MB of stack in a debug build for its parse, read, passes and render
together, the deepest H2 statement (and the typing walk over both oracles) between 2 and 4 MB, and every one less than
1 MB in release (`LITE_DIALECT_STACK_MB` sets the test thread's; it defaults to 256 MB). The seven tests run in
parallel in 58 s in debug (the H2 seam's, the longest, 57 s alone) and 8.7 s in release. A MIR at MirJson's
depth limit (2,045 nested calls) needs between 32 and 64 MB in a debug build and more than 1 MB in release; the seam's
C export runs each call on a 256 MB thread (`SEAM.md` §1).
