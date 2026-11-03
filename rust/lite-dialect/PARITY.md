# DuckDB parity: the renderer and its passes (2026-11-03)

Step 2 of the DuckDB slice (docs/rust-port/PHASE1.md §4, step 3): every pass DuckDB's render runs, ported from its Java
class, over a walk that keeps Java's identity semantics and a port of the typing rules Java's constructors apply. This
file measures it on the golden dump and says what of Java it does not reproduce exactly.

Measured by `tests/duckdb_differential.rs` over `//rust/oracles:mir_dump` (`mir/mir-dump.jsonl`):

- `duckdb_renders_as_java_does`: every line whose dialect is `DuckDb`, rendered by a fresh `DuckDb` and compared with
  the fresh JVM `DuckDb`'s answer -- the SQL text exactly, a refusal by its Java class and message, a statement's binds
  and regions by whether they are empty (and, where they are not, by Java's record text, counted);
- `typing_reproduces_javas_facts`: every node of every statement of the dump (all dialects: one tree), its type fact
  recomputed by `src/typing.rs` from its children's and compared with the fact Java's constructor stored; every
  projection frame's outputs, union's labels and join's delivered columns likewise.

```
cd rust
LITE_MIR_DUMP=<the dump> cargo test --release -p lite-dialect --test duckdb_differential -- --ignored --nocapture
```

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
exactly.

**Typing: 1,494,436 facts of 26,095 statements, 0 different** -- every `Call` rule the dump exercises (165 functions),
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
- Not ported, as no DuckDB render reaches them: `starsAsSourceColumns` (MariaDB, MySQL and BigQuery's), and DuckDB's
  non-rendering surface (its catalog types and rules, host operations, session setup, scripts, `copySelectList`).

## Tests

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
- `tests/duckdb_differential.rs`: the two dump differentials above.

## Stack

The deepest statement of the dump needs between 1 and 2 MB of stack in a debug build for its parse, read, passes and
render together, and less than 1 MB in release (`LITE_DIALECT_STACK_MB` sets the test thread's; it defaults to 256 MB).
Both differentials run in 16 s in debug and 2.2 s in release.
