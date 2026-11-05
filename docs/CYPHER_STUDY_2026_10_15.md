# Cypher in Pure: the `#Cypher{...}#` island, Neo4j as the spec, and graph algorithms the database runs

**Status: the study of 2026-10-15, kept as spec input for the port. The line was then built, 2026-10-15 to 2026-10-20
(97 landing records in §12, cut in the 2026-11 docs cleanup); §4's decisions were written as proposals, and each
deliberate difference from Neo4j that was built is a row of `docs/SEMANTICS_REGISTER.md`.** This is the third study of its kind, after
`docs/LEGEND_SQL_STUDY_2026_10_11.md` (Postgres SQL, the `#SQL{...}#` island) and `docs/IQ_SQL_STUDY_2026_10_13.md`
(SAP IQ, the `#IQ{...}#` island). It answers the user's ask of 2026-10-15: do for Neo4j's Cypher what those two lines
did for Postgres and IQ, and go further, so that the graph algorithms Neo4j runs can run here, translated to SQL
where that is feasible. Read the two studies' method sections first (the Postgres study's §3.4, §8 and §9; the IQ
study's §2, §4 and §6): this line inherits their rules and reuses their harness.

§0 is the answer; §1 is what the two lines did, as the method; §2 is what Cypher is; §3 the spec sources, the corpus
and the oracles, in numbers; §4 the decisions; §5 the architecture; §6 recursion; §7 the algorithms; §8 the harness;
§9 the slices; §10 the risks; §11 the sources.

No reference checkout is used: legend-engine has no graph store and no Cypher reader (its non-relational stores are
MongoDB, Elasticsearch, the service store and flat data), and legend-pure has nothing of the kind. Like the IQ line,
this one is built from a spec and an oracle; there is nothing to port. The measurements below were taken on
2026-10-15 from a clone of `opencypher/openCypher` at `677cbafabb8c` (2026-03-20) and from Neo4j's published manuals
for the 2026.06 and 2026.07 releases.

---

## 0. Start here

1. **What the island is.** `#Cypher{...}#` makes a query written for Neo4j an ordinary Pure Relation expression:

   ```
   #Cypher{
     MATCH (p:Person)-[:firm]->(f:Firm)
     WHERE f.legalName STARTS WITH 'A'
     RETURN f.legalName AS firm, count(p) AS headcount
     ORDER BY headcount DESC LIMIT 10
   }#->filter(r | $r.headcount > 1)
   ```

   The translator reads Cypher and builds the Pure a person would write (`Person.all()->project(...)->groupBy(...)
   ->sort(...)->limit(10)`); the compiler types it once, like any query, and it runs on any backend legend-lite
   renders. It returns **Neo4j's answer**: three-valued `null`, Neo4j's ordering of mixed values, `1 = 1.0`, integer
   `/` that truncates, `count(*)` that counts rows.
2. **Why it fits better than SQL did.** Legend's class model is a property graph: labels are classes (a superclass is a
   label every subclass's node also carries), relationship types are association ends and to-one or to-many
   properties, node properties are properties. The Postgres line added "classes as tables" as a later phase; Cypher
   starts there, and the compiler already lowers `Person.all()->project(~[firm: p | $p.firm.legalName])` to joins
   through the mapping. A relationship with properties is an edge class, declared as one by a stereotype the
   compiler reads as it reads `<<equality.Key>>`; the graph a query sees is a data space, selected with Neo4j's own
   `USE` (§5.6). The pitch: graph queries over any warehouse lite renders, with no graph database.
3. **What is new, in two words: recursion and determinism.** Bounded paths (`*1..3`) unroll into a union of one-,
   two- and three-hop joins, which is "the unroll rearranges shapes" (`docs/TENET_CHARTER.md` C6.2) and needs nothing
   new. Unbounded paths, `shortestPath` and every iterative algorithm need the database to iterate. Every backend lite
   renders has `WITH RECURSIVE` in some form, and the dialect layer already emits one (`FoldsAsRecursiveCtes`, for
   BigQuery); what Pure lacks is a spelling of a fixpoint. That is one new native with one lowering rule per dialect
   (§6), and it touches C6.3 and C6.4 of the tenet charter, which name recursion as excluded or residue. **The user
   ruled on 2026-10-15 that recursion is required** for the graph queries and the algorithms, so the native is built
   (PR5, §6.3); what remains to agree is its shape. And "all of Neo4j's graph algorithms" has to mean "every
   deterministic one": about a third of the catalog is randomized or heuristic, and there is no answer to be right
   about (§7).
4. **The spec and the corpus are better than IQ's.** openCypher is Apache 2.0, with a published grammar (377
   productions) and a conformance kit, the TCK: 220 feature files, 1,615 scenarios, 3,880 with their examples
   expanded, each carrying its own expected rows or error, under Apache 2.0 (§3.1). It is on Maven Central as
   `org.opencypher:tck`, so it is test input exactly as the engine's parity YAML is. Its weakness is the function
   table: 17 of its mathematical feature files hold 6 scenarios between them; lite's own Neo4j-judged cases must fill
   that, as the Postgres line's `lite/*.yaml` did.
5. **The oracle is free and runs anywhere, and it is GPL.** Neo4j Community is GPLv3, so it is never a dependency. The
   IQ pattern fits: a local snap tool over the Apache 2.0 Python driver, answers committed, CI judges from the file
   with the STALE, UNSNAPPED and GONE guards. Unlike IQ, any machine with Docker or a JDK can add a case, and the Graph
   Data Science plugin loads in Community, so the same tool snaps the algorithms' answers (§3.3). LDBC Graphalytics,
   whose six algorithms are defined to be deterministic and come with reference outputs, is a second oracle that owes
   nothing to Neo4j (§3.4).
6. **The version switch (the user's nit of 2026-10-15).** Neo4j selects the language version with its own prefix
   inside the query, `CYPHER 5 MATCH ...` or `CYPHER 25 MATCH ...`. The island should honour exactly that and invent
   no tag syntax: one island type, `#Cypher{...}#`, the text inside it being what Neo4j accepts, the prefix
   included. For a read-only translator the two versions are one parser with a version flag (§2.1). **Decided by the
   user, 2026-10-15:** one island, the version in the query as Neo4j spells it, Cypher 25 the default (§4, C1, C9).
7. **The one-line rule of the line,** inherited: zero FAIL, zero BUG, zero ERROR, always. Where Neo4j's answer is not
   a function of the input (a tie `SHORTEST 1` breaks arbitrarily, a community a heuristic found), or where lite would
   have to compute a value in Java, the translator refuses by name and position.
8. **Size.** The Postgres and IQ readers are about 10k and 12k lines of main code. Cypher is a bigger grammar, plus
   lists, maps, nodes and paths as values, plus the native, plus the algorithm library: 20k to 25k lines over six to
   eight landings, each with its own cases, ledger, independent review and GATES entry (§9).

---

## 1. The method this line inherits

What the two lines did, as the rules a third one follows. The evidence is in their studies' §9.

- **One translator per language into one target.** `#SQL{}#` and `#IQ{}#` read text into a Pure Relation
  `ValueSpecification` that `compiler/spec/SqlIslands` hands to the Typer, which types it once; nothing is re-spelled
  as text and nothing is typed twice (invariant 1). The IQ line put Postgres's rules behind `InputDialect` and judged
  the move by the generated Pure of 2,225 cases, byte for byte.
- **The vendor is the spec; the oracle is real; CI judges from a file.** Postgres 16 runs embedded in CI and its
  answers are snapped (`sql-parity/postgres-answers.jsonl`), the Postgres lane checking the file against Postgres on
  every run. IQ cannot run here, so SQL Anywhere 17 is snapped locally by `tools/iq-oracle/snap.py` (1,142 lines of
  standard-library Python), and the lanes judge the committed answers with the STALE (an edited case is never judged
  by its old answer), UNSNAPPED and GONE guards.
- **Statuses that can only move with a dated entry.** PASS, FAIL (a wrong answer), BUG (answers what the oracle
  rejects), ERROR (errs where the oracle answers), REFUSED (by name), BOTH_ERROR, SKIP, per case per lane, in a ledger
  the test ratchets (REGRESSION, FIX DETECTED, STATUS MISMATCH).
- **A refusal by name beats a possibly wrong answer.** Each study's §9 lists what is refused and why, each reason
  measured on the oracle and on both lanes. The IQ line refused its riskiest provisional decisions outright until a real
  IQ can verify them.
- **Java never computes a value.** A missing semantic is a Pure spelling or a refusal; the IQ line's three natives
  turned out to be compositions of existing natives (its §9.8).
- **Randomized differentials and independent reviews.** The Postgres line's generator ran about 9,000 type-correct
  queries through both lanes against Postgres; every landing was reviewed by an agent that wrote none of it, each
  finding fixed and pinned by a case.
- **The ledgers at the last landings:** `sql-parity` 2,180 cases, PASS 1,527, 0 FAIL, 0 BUG, 0 ERROR; `iq-parity`
  about 1,600 cases, 0 FAIL, 0 BUG, 0 ERROR.

---

## 2. What Cypher is

### 2.1 Two versions, one prefix

Since Neo4j 2025.06 the server carries two language versions. **Cypher 5** is frozen as it was at 2025.06: bug fixes
and security updates only. **Cypher 25** is a clone of Cypher 5 that evolves with every monthly server release. A query
picks its version with Neo4j's own prefix, `CYPHER 5` or `CYPHER 25` before the first clause (other options may follow
it: `CYPHER 25 runtime=parallel`); otherwise the database's default applies: `db.query.default_language` in
`neo4j.conf`, `CREATE DATABASE ... SET DEFAULT LANGUAGE CYPHER 25`, or `ALTER DATABASE`. The defaults when nothing is
set: self-managed 2025.06 to 2026.01, Cypher 5; self-managed 2026.02 on, new installs write `CYPHER_25` into
`neo4j.conf` and a missing setting still means Cypher 5; Aura, Cypher 25 for new organisations.

What separates the two, as far as a **read-only translator** sees it (the 2025.06 to 2026.07 changelog):

| Cypher 25 adds | Cypher 25 removes | Changed |
|---|---|---|
| clauses `FILTER`, `LET`, `NEXT`, `WHEN`/`ELSE`; `UNION` and `UNION ALL` mixed inside braces; the `REPEATABLE ELEMENTS` and `DIFFERENT RELATIONSHIPS` match modes; `ACYCLIC` paths (2026.03); dynamic labels and types in `MATCH`; the `VECTOR` type and its functions; `allReduce()`, `format()`, `string.indexOf`, `string.join`, `string.regexReplace` (2026.05); GQL naming aliases (`collect_list`, `percentile_cont`, `zoned_datetime`, ...) | some identifier characters (a backticked composite part, unescaped `\u0085` and `$`); `SET n = node` (a write); `MERGE` reading another entity's property (a write); a few procedures | `WITH` no longer required between a read and a write clause (writes); `replace()` takes a limit; parameters may start with a digit; an imported variable in a `COLLECT`, `COUNT` or `EXISTS` subquery is a constant |

So for reads, Cypher 25 is Cypher 5 plus clauses and functions, minus two identifier spellings, with one scoping rule
changed. One reader with a version flag covers both; the flag decides which identifiers parse and whether the added
clauses are known. The island's spec is therefore **one pinned Neo4j release's Cypher 25** (§4, C9), with `CYPHER 5`
as the frozen profile.

**GQL.** ISO/IEC 39075:2024 is the standard Cypher 25 grows toward. Neo4j's conformance appendix (2026.06) claims
support for "the majority of mandatory GQL features", declares the open graph type (GG01), the mandatory property
types BOOLEAN, FLOAT (64-bit), INTEGER (64-bit) and STRING, and names its Unicode version by the JVM's. The standard's
text is not free; the openCypher grammar is published "in ISO WG3 BNF format" and is the free spelling of the same
language family. The island follows Neo4j's manual, not the ISO text, and says so.

### 2.2 The language a read-only translator meets

- **Clauses.** `MATCH`, `OPTIONAL MATCH`, `WHERE`, `WITH` (a pipeline boundary, with `DISTINCT`, `ORDER BY`, `SKIP`,
  `LIMIT` and `WHERE` of its own), `UNWIND`, `RETURN` (`*`, `DISTINCT`, aliases, implicit grouping), `ORDER BY`,
  `SKIP`, `LIMIT`, `UNION [ALL]`, `CALL { }` subqueries (with imported variables), `CALL proc() YIELD`, the Cypher 25
  `FILTER`, `LET`, `NEXT`, `WHEN`. Writes: `CREATE`, `MERGE`, `SET`, `REMOVE`, `DELETE`, `FOREACH`, `LOAD CSV`. `USE`,
  `SHOW`, administration.
- **Patterns.** Node patterns with label expressions (`:A`, `:A:B`, `:A&B`, `:A|B`, `:!A`, `:%`); relationship
  patterns with a type expression, a direction or none, a variable, properties; fixed-length paths; the legacy
  variable-length `-[:T*1..3]->` (lower bound default 1, unbounded `*` allowed); GQL quantified path patterns
  `((a)-[:T]->(b) WHERE ...){1,3}` and quantified relationships `-[:T]->{1,3}`, `+`, `{,10}`, with group variables
  that are lists outside the pattern; named paths `p = (...)`; the legacy `shortestPath()` and `allShortestPaths()`;
  the path selectors `ANY`, `ANY SHORTEST`, `SHORTEST k`, `SHORTEST k GROUPS`, `ALL SHORTEST`, `ALL`; the match modes
  `DIFFERENT RELATIONSHIPS` (the default: a relationship is traversed once per `MATCH`, a node may repeat) and
  `REPEATABLE ELEMENTS`; `ACYCLIC`. Pattern comprehensions `[(a)-->(b) | b.name]`; `EXISTS { }`, `COUNT { }`,
  `COLLECT { }` subqueries.
- **Values.** `null`, booleans, 64-bit integers, doubles, strings, lists (any mix), maps, nodes, relationships, paths,
  temporal values (`DATE`, `LOCAL TIME`, `ZONED TIME`, `LOCAL DATETIME`, `ZONED DATETIME`, `DURATION`), spatial
  points, vectors, UUIDs (2026). Lists are 0-based, a negative index counts from the end, an index out of range is
  `null`, a slice out of range is truncated, `range(a, b)` is inclusive at both ends.
- **Functions.** The 2026.07 manual lists 176 entries in 19 categories: aggregating 15, list 20, string 20, scalar 25,
  numeric 8, logarithmic 6, trigonometric 16, predicate 7, temporal instant 31, duration 6, format 1, spatial 3,
  vector 6, UUID 3, graph 4, database 1, security 2, LOAD CSV 2, user-defined (types only). Function names are
  case-insensitive (the TCK calls `cOuNt(` and `nOdEs(`); labels, types, property keys and variables are
  case-sensitive.
- **Null** is three-valued throughout: `AND`, `OR`, `XOR`, `NOT` by the SQL tables; `x = null` and `x <> null` are
  `null`; `IN` is `true` on a match, `null` on no match with a `null` element, `false` on no match otherwise, and a
  `null` left side is `null` for a non-empty list and `false` for an empty one; `[][0]`, `head([])`, a missing property
  and `1 + null` are `null`; a `WHERE` keeps only `true`.
- **Ordering and equality.** `ORDER BY` uses one global order of types, maps first, then nodes, relationships, lists,
  paths, vectors, points, the temporal types, strings, booleans, UUIDs and numbers last, `null` after everything in
  ascending order; strings compare "lexicographically" (the manual does not say by code point or UTF-16 unit:
  to measure); `1 = 1.0` is `true` (the TCK's `Comparison1` pins it); lists compare element by element and maps by
  key; every comparison with `NaN` is `false`; `<` on nodes, relationships or maps is an error; a duration or a UUID
  never orders.
- **Arithmetic** is Java's: `/` of two integers is an integer, `10 ^ 5` is the double `100000.0`, an integer result
  out of 64 bits is an error (the TCK's `IntegerOverflow`), and so is a double result out of range (the TCK's
  `FloatingPointOverflow`). Truncation direction and the sign of `%` are not stated in the manual: Java's, to
  measure.
- **Strings** are Unicode characters, never UTF-16 units (`size()` of a supplementary character is 1); `substring` is
  0-based, a negative or a null length is an error; `left` and `right` refuse a negative count; `btrim`, `ltrim`,
  `rtrim` take a character set; `split` takes a delimiter or a list of them; `toString` writes a double as `11.5` and a
  boolean as `true`; `=~` is a Java regular expression over the whole string.
- **Aggregation.** Non-aggregated `RETURN` or `WITH` items are the implicit group keys; `count(*)` counts rows,
  `count(x)` non-nulls; `avg` of integers is a double, `sum` of integers an integer, `sum` of nothing `0`, `avg` of
  nothing `null`; `collect` skips `null` and is `[]` over nothing; `min` and `max` rank mixed values by the global
  order; `percentileDisc` is the nearest value "by rounding", `percentileCont` interpolates; `stDev` of nothing is
  `null` since 2026.05 and was `0` before, which is the kind of change a pinned release freezes (C9).
- **Output names.** A returned expression without an alias is named by its text, as IQ names an unaliased item.

### 2.3 Where Cypher differs from SQL and from Pure

The rows the translator spells out from the first slice. Each is in our words from the manual or the TCK, and marked
where it is still to measure on the oracle.

| Behaviour | Cypher (Neo4j) | SQL (the Postgres island) | Pure | Id |
|---|---|---|---|---|
| A property no entity of the label has | `null` | a compile error (no such column) | a compile error | C4: a model has a schema; refused, not `null` |
| Numeric equality across kinds | `1 = 1.0` is `true` | `true` | `true` | agree |
| `=` across kinds | `1 = 'a'` is `false`, never `null` (to measure); `<` is an error for incomparable kinds | an error | an error | spelled per kind |
| `null = null` | `null` | `null` | `[] == []` is `true` | three-valued pairs, as `SqlToPure.Tx` |
| `IN` with `null` | the rule of §2.2 | the same | `in` is two-valued | the same spelling the Postgres island uses |
| Ordering of mixed kinds | one global order, `null` last ascending | an error | an error | a column of one kind orders; a mixed column is refused |
| Integer `/` | integer, Java's truncation | integer, toward zero | `Number`, a float on DuckDB | the Postgres island's exact quotient |
| Integer overflow | an error | an error | the database's | the Postgres island's checks only where a result can leave 64 bits |
| `^` | always a double | `^` of numerics is numeric | `pow` | `pow` then a double |
| String positions | 0-based `substring`, Unicode characters | 1-based, characters | `substring` 1-based in the relational spelling | the offset shifted by one |
| String order | lexicographic, basis unstated | the collation's | the database's | DuckDB and the embedded Postgres compare bytes, which is code-point order; equal to UTF-16 order inside the BMP; to measure beyond it |
| Case folding | Java's, all of Unicode | the collation's | the database's | DuckDB's `upper` folds all of Unicode; Postgres's in the C locale folds ASCII only: `toUpper` on the Postgres lane is refused for a value that may hold a non-ASCII letter until measured |
| Lists and maps as values | first-class | arrays and `jsonb` | collections, `Variant` | `Variant` (jsonb on Postgres, JSON on DuckDB), as the Postgres island reads `jsonb` |
| A node as a value | labels, properties, an id | a row | an instance | a `Variant` of labels and properties (C8); `id()` and `elementId()` refused |
| A path as a value | nodes and relationships in order | none | none | a `Variant` list (C8) |
| Output name of an unaliased item | the expression's text | `?column?` | the lambda's | the text, as IQ |
| `count(*)` of no rows | 0 | 0 | 0 | agree |
| Relationship uniqueness | once per `MATCH` | none | none | spelled as `r1 <> r2` per unrolled hop |
| Undirected pattern | each relationship matched in both directions | none | an association's two ends | a union of the two directions |
| Procedures (`CALL db.*`, `apoc.*`) | the server's | none | none | refused by name |
| Writes | `CREATE`, `MERGE`, `SET`, `DELETE`, `REMOVE` | none in lite yet | `write` is F1 of `docs/BI_AND_ETL_PLAN_2026_09_29.md`, not built | refused by name until F1 |

### 2.4 What each side cannot say

- **Cypher cannot say** a class's keys, a milestoned navigation, a mapping, a union of stores, an M2M transform, a
  typed function signature, a constraint. A model written for the island is written in Pure (or Python, or DataCube's
  "Make a class"); the island queries it.
- **Legend cannot say,** today, a property on a relationship (associations carry none: an edge class says it,
  §5.6), a label-less node, a dynamic label, a node of two unrelated classes at once. Each is a decision (§4, C3)
  or a refusal.

---

## 3. The spec sources, the corpus and the oracles, in numbers

### 3.1 openCypher: the grammar and the TCK

**License.** The openCypher project is Apache 2.0 (its `LICENSE`, its README's "The openCypher project is licensed
under the Apache license 2.0", the TCK README's "licensed with Apache license 2.0, which is inherited"). Each feature
file carries Neo4j Sweden AB's Apache header with an attribution notice: an implementation extended beyond the
consensus process "should not be described as Cypher or as openCypher" but as an implementation extension. The
island is named `#Cypher{...}#` after the language it reads; the documentation says "legend-lite's Cypher island",
never "a Cypher implementation".

**The grammar.** `grammar/openCypher.bnf`: 1,533 lines, 377 productions, in the ISO WG3 notation, the free spelling
of the language family GQL and Cypher 25 belong to. The IQ research had 244 productions. Neo4j's own Cypher 25 ANTLR
grammar lives in the GPL server tree and is not read.

**The TCK, measured** (`tck/features`, clone `677cbafabb8c`, 2026-03-20; the census scripts are in the session's
scratch, 60 lines of Python over the feature files):

| | feature files | scenarios | with examples expanded | expect an error | the query writes | read-only and expecting rows |
|---|---|---|---|---|---|---|
| `clauses/` | 93 | 827 | 1,251 | 120 | 288 | 457 |
| `expressions/` | 125 | 758 | 2,599 | 72 | 24 | 668 |
| `useCases/` | 2 | 30 | 30 | 0 | 0 | 30 |
| **all** | **220** | **1,615** | **3,880** | **188** | **312** | **1,155** |

Per family (scenarios; errors; writes; read-only expecting rows):

| family | scenarios | errors | writes | read-only | what it holds |
|---|---|---|---|---|---|
| clauses/match | 161 | 21 | 1 | 139 | nodes, relationships, fixed and variable-length patterns, named paths, optional match, the named graphs |
| clauses/match-where | 34 | 2 | 0 | 32 | filters, equi- and non-equi-joins, null predicates |
| clauses/return, return-orderby, return-skip-limit | 129 | 31 | 4 | 97 | projection, renaming, implicit grouping, ordering by type, skip and limit |
| clauses/with, with-orderBy, with-skip-limit, with-where | 156 | 11 | 1 | 144 | the pipeline; `with-orderBy` alone expands to 292 examples |
| clauses/union | 12 | 4 | 0 | 8 | |
| clauses/unwind | 14 | 0 | 2 | 12 | |
| clauses/call | 41 | 16 | 0 | 25 | procedures: refused by name (no procedures here) |
| clauses/create, merge, set, delete, remove | 280 | 35 | 280 | 0 | writes: refused by name until F1 |
| expressions/literals | 131 | 25 | 0 | 106 | every literal spelling; 25 syntax errors |
| expressions/quantifier | 100 | 4 | 0 | 96 | `all`, `any`, `none`, `single`: 604 expanded examples |
| expressions/list | 94 | 6 | 4 | 84 | indexing, slicing, comprehension, `IN`, `range` |
| expressions/temporal | 89 | 0 | 12 | 77 | 1,004 expanded examples: dates, times, zones, durations, truncation |
| expressions/graph | 48 | 8 | 6 | 34 | `labels`, `type`, `properties`, `keys`, `nodes`, `relationships` |
| expressions/precedence | 43 | 0 | 0 | 43 | |
| expressions/boolean, comparison, null | 85 | 5 | 0 | 80 | three-valued logic, comparison across types |
| expressions/pattern, path, existentialSubqueries | 53 | 8 | 2 | 45 | pattern comprehensions, path functions, `EXISTS { }` |
| expressions/aggregation | 27 | 3 | 0 | 24 | |
| expressions/map, conditional, typeConversion | 50 | 8 | 0 | 42 | |
| expressions/string | 32 | 0 | 0 | 32 | 14 files; thin |
| expressions/mathematical | 6 | 1 | 0 | 5 | **17 files, 6 scenarios**: 13 of the files are empty stubs (division, modulo, power, rounding, logarithms, trigonometry) |
| useCases | 30 | 0 | 0 | 30 | counting subgraph matches, triadic selection |

What the census says:
- **1,155 scenarios are the island's target** from the first slice: read-only, expecting rows. 188 expect an error
  (156 of them `SyntaxError` at compile time: the syntax ratchet, free). 312 write and are refused by name until F1
  gives lite writes; 41 call procedures and are refused by name.
- **723 scenarios set their graph up with `having executed: CREATE ...`**, and 19 use one of the two named graphs
  (binary trees of depth 4 with three relationship types). The harness must turn those `CREATE`s into tables (§8.2);
  the oracle does that for it.
- **The TCK is written for a schema.** Its property-naming convention assigns a type per name (`num` integer, `name`
  string, `price` float, `ok` boolean, `var` for the few mixed-type cases): only 7 feature files use `var`. A model
  inferred from each scenario's `CREATE`s therefore types almost every property, and the handful of mixed-type
  scenarios are refused by name.
- **Values in the expected rows** are written in a small grammar the judge must read: integers, decimals, `NaN`,
  `Inf`, single-quoted strings, `true`, `null`, `[...]`, `{k: v}`, `(:L1:L2 {p: 0})`, `[:T {p: 0}]`, and paths
  `<(:A)-[:T]->(:B)>`. Nodes and relationships compare by labels, type and properties, never by id. Rows are a set
  unless the `RETURN` carries `ORDER BY`; "in order" then means ordered by the first sort key only.
- **Functions used**, 65 distinct: `count` 143, `collect` 58, `reverse` 58, `all` 48, `any` 46, `size` 42, `sum` 32,
  `none` 31, `single` 27, `toString` 24, `labels` 23, `keys` 21, `type` 16, `toInteger` 15, `nodes` 12, `coalesce`
  10, `range` 10, `tail` 10, `max`, `min`, `properties` 9 each; the temporal constructors and `truncate`s about 150
  between them; `test.my.proc` 28 (procedures). The whole string and mathematical table is barely touched: **lite's
  own cases supply it** (§8.1), judged by Neo4j, as the Postgres line's `lite/functions.yaml` (139 cases) did.
- **`rand()`** appears in 116 expanded examples, all in the quantifier outlines, where it feeds a predicate's operand;
  those are volatile and are skipped, as `sql-parity`'s seven volatile cases are.

### 3.2 Neo4j's manuals: read, never copied

The Cypher manual, the GDS manual and the operations manual are the spec of what Neo4j answers. Their terms are
Neo4j's; the Bolt protocol documentation is CC BY-NC-SA 4.0 (its repository's `LICENSE.txt`). As with SAP's manuals
(the IQ study's I7): nothing from them is copied into this repository. Behaviour is written in our words with the
manual's page named; a case's query is ours; the review of each landing checks the diff for quoted text.

### 3.3 The oracle: Neo4j Community, local only

- **Neo4j Community Edition is GPLv3** (the server; Enterprise is closed and commercial). It cannot be a dependency of
  anything here, test-only included: no jar on any classpath, no `neo4j-harness`, no Testcontainers in CI (the hosted
  macOS and Windows runners have no Docker anyway: `tools/postgres/postgres.bzl`).
- **The Python driver is Apache 2.0** (`neo4j-python-driver`, `LICENSE.APACHE2.txt`), over Bolt. The snap tool is
  Python's standard library plus that driver, run isolated (`python3 -I`), as `tools/iq-oracle/snap.py` is.
- **The server runs locally**: the official image `neo4j:2026.07.1` (Community tags carry no suffix; `-enterprise`
  does) with `NEO4J_PLUGINS='["graph-data-science"]'`, which Neo4j documents for development, or the Community
  tarball on a JDK 21 for a machine without Docker. GDS is available in Community, capped at 4 cores.
- **CI judges from the committed answers.** `cypher-parity/neo4j-answers.jsonl`, one line per case, with the server
  version, the Cypher version the case ran under, the `sqlSha` over query, version and schema (STALE), and the
  dumped graph for scenarios that set one up (§8.2). `snap.sh --check` is the drift check a landing records in GATES.
- **A pinned release** (C9). Cypher 25 changes monthly (`stDev` of nothing, 2026.05). The answers carry the server
  version; a case re-snapped on a newer server that answers otherwise is a FIX DETECTED or a REGRESSION to adjudicate,
  as a drifted Postgres answer was on 2026-10-13.

### 3.4 A second oracle for the algorithms: LDBC Graphalytics

The LDBC Graphalytics benchmark (specification v1.0.5, CC-BY) defines six algorithms **to be deterministic** and ships
reference outputs with its datasets, validated by exact match (BFS), equivalence of partitions (WCC, CDLP: a two-way
mapping between the labels) or an epsilon match (PageRank, LCC, SSSP). Its definitions, in our words:

| Algorithm | Definition | Validation |
|---|---|---|
| BFS | each vertex's depth from the root; unreachable is `9223372036854775807` | exact |
| PageRank | `PR0(v) = 1/|V|`; `PRi(v) = (1-d)/|V| + d * Σ PR(u)/outdeg(u) over in-neighbours + (d/|V|) * Σ PR(w) over sink vertices`; a fixed number of iterations; doubles | epsilon |
| WCC | a component label per vertex, edges read as undirected | partitions equivalent |
| CDLP | `L0(v) = v`; each iteration every vertex takes the most frequent label among its in- and out-neighbours (a neighbour on both sides counts twice), the **smallest** label on a tie, its own label with no neighbours; synchronous; a fixed number of iterations | partitions equivalent |
| LCC | 0 for fewer than two neighbours, else the edges among the undirected neighbour set divided by `|N|(|N|-1)`, directions enforced inside | epsilon |
| SSSP | the weighted shortest distance from the root, non-negative double weights, unreachable is infinity | epsilon |

Why it matters: GDS's PageRank is the 1998 formula unnormalized (scores are not scaled to sum to 1 unless
`scaler: "L1Norm"` is asked), with `dampingFactor 0.85`, `maxIterations 20`, `tolerance 1e-7` and an early stop, and
its manual does not say how a dangling node's mass is handled; GDS's label propagation breaks ties "arbitrarily but
deterministically" by an order the manual does not state. Graphalytics states every such rule. So an algorithm here
is **one computation with the spec's constants as parameters** (initial value, teleport term, sink handling, tie rule,
iteration count or tolerance), judged against the oracle whose rule it was given: Neo4j for GDS's profile, the
reference outputs for Graphalytics's. Where GDS's rule is not stated and cannot be measured to a fixed rule, GDS's
profile of that algorithm is refused by name and Graphalytics's is offered.

### 3.5 Other tools, and why none is a dependency

- **Cypher for Apache Spark / Morpheus** (openCypher, Apache 2.0, archived): Cypher over DataFrames, graphs stored as
  one scan table per label and per relationship type, a fixed schema inferred or declared. The closest prior art to a
  relational, typed translation; its storage model is the model's own here (a class per label).
- **Graphiti** (2025) translates `MATCH` to inner joins and `OPTIONAL MATCH` to outer joins, syntax-directed; **Raqlet**
  (CIDR 2026) compiles Cypher and SQL/PGQ through a Datalog-like form to recursive SQL. Both confirm the shape of §5
  and §6; neither is a runtime for a JVM, TeaVM and GraalVM product.
- **Apache AGE** (Apache 2.0): openCypher inside Postgres, with its own `agtype`. A C extension built per platform;
  not loadable into the embedded Postgres the lanes run, and not Neo4j's answers.
- **DuckPGQ** (SQL/PGQ in DuckDB, a community extension): `MATCH` with `ANY SHORTEST` and bounded hops, a multi-source
  BFS operator. It targets DuckDB 1.4.4 and is documented as unavailable for 1.5.x, which `MODULE.bazel` pins (1.5.5),
  and as incomplete. Not a base; a possible render target for DuckDB later, as `QUALIFY` is for Snowflake.
- **Kùzu** (MIT, embeddable, Cypher): archived on 2025-10-10 after its acquisition; forks exist. Not an oracle.
- **DuckDB's `USING KEY`** recursion (since 1.3): a recursive CTE whose rows are keyed, so an iteration **replaces** a
  node's row instead of appending it. Built for exactly the iterative algorithms of §7; the DuckDB render of the
  recursion native uses it where the native's shape is keyed.

---

## 4. Decisions (proposed, for the user)

`C` for Cypher, distinct from the Postgres study's `D` and the IQ study's `I`.

- **C1. The island is `#Cypher{...}#`; the version is Neo4j's own prefix inside the text.** The user's nit of
  2026-10-15 asked whether the version should be a profile, a tagged value or a stereotype, or two island types
  `#Cypher5{...}#` and `#Cypher25{...}#`, rather than a change to the parser that upstream's parser would not read.
  The facts:
  - lite's lexer reads an island opening as `#`, then everything up to the first `{` or `#` (`lexer/Lexer.java`, the
    `ISLAND_OPEN` scan), so `#IQ:iq16-default{`, `#Cypher25{` and `#Cypher{` all lex alike today; the parser's
    `parseDsl` arm decides what a type means;
  - legend-engine's grammar refuses every island type it does not know, so `#IQ{`, `#IQ:x{`, `#Cypher{` and
    `#Cypher25{` are all equally foreign to upstream: the LEGEND_ENGINE surface refuses them, the own-corpus census
    declares them (`docs/OWN_CORPUS_DECISIONS.md` §14 for IQ). No choice among them is more upstream-compatible than
    another;
  - Neo4j itself puts the version **in the query**: `CYPHER 25 MATCH ...`.

  The proposal: one island type, no tag syntax, the text inside it exactly what Neo4j accepts, the `CYPHER 5` or
  `CYPHER 25` prefix honoured as Neo4j honours it. A query copied from Neo4j Browser runs unchanged. The default
  without a prefix is **Cypher 25** (what Aura's new organisations and 2026.02's new installs get, and where GQL's
  clauses live); `CYPHER 5` is the frozen profile. A stereotype or tagged value on the enclosing function is weaker:
  a bare query in Query, DataCube or Python has no function to carry it, and the island would not describe itself.
  `#Cypher5{...}#` and `#Cypher25{...}#` as sugar for the prefix are harmless and cost one more `parseDsl` arm each;
  they can be added if the user prefers the version visible in the tag. The wire shape is a `classInstance` of type
  `Cypher` with `{"cypher": <text>}`, as `IQ` carries `{"sql", "profile"}`.
  **Decided by the user, 2026-10-15: one island type, `#Cypher{...}#`, the version in the query as Neo4j spells it;
  no sugar types; no stereotype or tagged value for the version (the query carries it, so nothing in the model
  needs to).**
- **C2. The graph is the class model.** A label is a class the data space (or the query's mapping and runtime) can
  query; a node of class `Employee extends Person` carries both labels; a relationship type is an association end or
  a to-one or to-many property, by name; a node property is a property. The island therefore runs over any model
  that `Person.all()` runs over, on every backend, with no new store kind. Cypher over bare tables is "Make a class"
  first (DataCube's model line), never a parallel declaration.
- **C3. Relationship types, directions and edge properties.**
  - `(p:Person)-[:firm]->(f:Firm)` reads property `firm` of `Person`; `(f)-[:employees]->(p)` the other end;
    `(p)-[:PersonFirm]-(f)` the association by name, either direction. A type that names nothing on the left label is a
    compile error naming the label's properties.
  - **A relationship with properties is an edge class, declared as one** (§5.6, the full design; the user on
    2026-10-15: "let's not shortcut"). Pure's `Association` has exactly two properties and nothing else
    (`model/AssociationDefinition`), so the only Legend element that can hold a relationship's own properties, keys
    and mapping is a class: `Acting` with `actor: Actor[1]`, `movie: Movie[1]` and `role`. The class says it is an
    edge, and which ends are which, by an ordinary Pure `Profile` in a library project (`graph::graph`, with the
    four stereotypes `Edge`, `from`, `to` and `type`, and no tags), which the engine and Studio compile as any
    profile and which lite reads by name exactly as it reads `<<equality.Key>>` (`compiler/element/EqualityKeys`,
    `PlatformTypes.EQUALITY_PROFILE`): `Class <<graph.Edge>> demo::Acting` whose ends are the stereotyped
    properties `<<graph.from>> actor: Actor[1]` and `<<graph.to>> movie: Movie[1]`, declared on the class or
    injected by two associations (the decomposed association class, the idiomatic form), and whose type is a
    stereotyped derived property spelling one value of the model's `RelationshipType` enumeration: element
    references the engine checks, never strings.
    Then `(a:Actor)-[r:ACTED_IN]->(m:Movie) RETURN r.role` is Neo4j's own spelling, `r`'s identity is Pure's
    instance equality, its keys if it declares them and otherwise the row itself as its mapping's primary key
    realizes it, never a model property (C5), and the class maps to its bridge table like any class, which is what SQL/PGQ's "edge table with a source
    key and a destination key" is. Nothing is inferred from a class's shape (invariant 4); an undeclared class is a
    label. The graph's exposure and execution context are the data space's, selected with Neo4j's own `USE`
    (§5.6): no new element kind, and no lite-only model construct.
  - Multiple labels on one node pattern (`:Person:Employee`, `:Person&Employee`) is the most specific class of the
    set when the set is a chain of the model's inheritance, else no rows (`&`) or a union (`|`). `:!A` and `:%` and a
    label-less `(n)` over the whole model are a union of every queryable class with a common schema (id, labels,
    properties as a Variant): offered for small models, refused by name over a bound the slice measures.
  - A dynamic label (`:$(expr)`, Cypher 25) is refused: the model is the schema.
- **C4. The static rule.** A property no class of the pattern's label declares is a compile error with Neo4j's
  wording and the model's candidates, not `null` (a register row). A property typed differently on two classes a
  union reads is refused by name. This is where lite is stricter than Neo4j on purpose, and the TCK's naming
  convention (§3.1) makes it cheap.
- **C5. Identity is Pure's instance equality: the model's `<<equality.Key>>` where a class declares it, instance
  identity otherwise, realized by the mapping and never by a model property** (the user, 2026-10-15, twice: no
  physical key leaks into the model, Legend's developers "care about that a lot", with local mapping properties for
  what the class must not show; and `<<equality.Key>>` is the object world's identity, `~primaryKey` the relational
  mapping's). The two are layers, not alternatives:
  - **`<<equality.Key>>` is the model's identity**: when two instances are the same thing, by business key (a
    `Movie` is its `title` and `year`). It is what Pure's `==`, `equal()` and `removeDuplicates()` use on instances
    (D91, `lowering/InstanceEquality`'s keyed half, on every lane), and it holds across stores and across a union
    mapping that reads one entity from two tables.
  - **`~primaryKey` is the relational mapping's**: which columns make one row one object in that mapping,
    `~primaryKey(...)` where declared, else the main table's `PRIMARY KEY` columns (`ClassMapping.primaryKey`,
    `lineage/PkInference`), which the engine itself uses to merge rows into objects. It is physical, so it never
    enters the model; a physical attribute a mapping needs and the class must not show is a local mapping property
    (`+x: Type[1]`, `PropertyMapping.LocalProperty`).
  - **Pure's rule composes them**, and it is the graph's rule: a keyed class's node is its key, so two rows with one
    key are one node; a keyless class's node is the row itself, and the relational realization of "the row itself"
    is the mapping's primary key. So `DISTINCT`, `count(DISTINCT n)`, `collect(n)`, `n = m`, relationship
    uniqueness and path equality are spelled in the Pure the island builds as instance equality, `==` and
    `removeDuplicates()`, the Pure a person would write, and lowered per the class's keyedness.
  - **The dependency, stated:** the identity half of `InstanceEquality` (the minted `__id` from the mapping's
    primary key, for keyless classes) "stays verdict-lane-gated" today. C5 needs it on the product lane: the
    database owner's leg, announced in `IN_FLIGHT` before PR3 (the model slice), and until it lands a pattern that
    needs identity over a keyless class is refused by name, never approximated by comparing properties. A keyless
    class whose mapping has no primary key either is refused likewise.
  - **One difference from Neo4j, a register row:** Neo4j's identity is always physical, so where a table holds two
    rows with one business key, Neo4j shows two nodes and lite one, because the model said so.
  - `id()` and `elementId()` are refused: Neo4j's are numbers no model has, and the physical key is exactly what
    must not leak.
- **C6. The recursion native and the bounded-unroll-first rule.** A bounded path or quantifier (`*1..3`, `{1,3}`,
  `{,10}`) unrolls into a union of fixed-length joins: no new native, C6.2's "rearranges shapes". Unbounded `*`, `+`,
  `shortestPath`, the `SHORTEST` selectors over unbounded patterns, and every iterative algorithm need the native of
  §6. **Decided by the user, 2026-10-15: recursion is required** ("for these graph queries and algos for sure"), so
  the native is built, in PR5, as the world map's kind 1 (one lowering rule per dialect), and the charter's C6.3 and
  C6.4 gain a dated note saying so. Still to agree, in PR5's design: its name and shape (§6.3), the termination guard,
  and its owner (it lands in the database owner's files). Until PR5 the unbounded forms are refused by name; the
  first four slices need none of them.
- **C7. Algorithms: the three buckets of §7 and the refusal rule.** An algorithm whose answer is a function of the
  input is translated and judged; one that is deterministic but costly is bounded by a size the slice measures, as
  the IQ island bounds its Pure node count; one whose answer is not a function of the input (randomized, heuristic,
  order-dependent) is refused by name. A tie Neo4j breaks arbitrarily (`SHORTEST 1`, `ANY`) is refused where the tie
  can occur and answered where it cannot (`ALL SHORTEST`, `SHORTEST k GROUPS`, distances, counts).
- **C8. Result values.** A scalar column is a Relation column. A list, a map, a node, a relationship or a path is a
  `Variant` column (jsonb on Postgres, JSON on DuckDB; the Postgres island's S49 rule): a node as
  `{"labels": [...], "properties": {...}}` built in SQL (`SqlExpr.JsonObject`, which the lowering already emits for
  Variant egress), a path as a list of nodes and relationships. The TCK's judge reads them structurally (§8.3). A
  temporal value is a Pure date where Pure has one (`DATE`, `LOCAL DATETIME` as `DateTime`); zoned types, times and
  durations are refused by name until a spelling exists.
- **C9. The spec is one pinned Neo4j release**, its Cypher 25 the default and its Cypher 5 the profile, named in the
  answers file. The first candidate is the newest Community release at the first landing (2026.07.x today).
  Re-pinning is a landing of its own, as a Postgres bump is. **Decided by the user, 2026-10-15: the newest, most used
  feature set, Cypher 25, is the default**; a query says `CYPHER 5` to get the frozen one.
- **C10. Placement and contention.** A new line (the eleventh, if the numbering holds), announced in `IN_FLIGHT` with
  its files. New: `core/src/main/java/com/legend/protocol/cypher/` (lexer, parser, AST, `CypherToPure`, the function
  table), `compiler/spec/CypherIslands.java`, `core/src/test/resources/cypher-parity/`, `tools/neo4j-oracle/`, the
  tests. Edited: the ten island plumbing sites of §5.3. Not touched until the recursion ruling: `lowering/`,
  `sql/dialect/`, `builtin/Pure.java`. The translator is a **sibling** of `SqlToPure`, not an `InputDialect` of it:
  Cypher's AST is not `SqlAst`. It shares the Pure-building helpers `SqlToPure` made public and copies the
  three-valued `Tx` discipline as a pattern; extracting the shared half into one builder is a later pure move judged
  by byte-identical Pure, as the IQ line's PR1 was, and only if the duplication hurts.

Decided by the user on 2026-10-15: C1 (one island, the version in the query, no sugar), C3 (the edge class design of
§5.6 as it stands after the day's questions: four stereotypes from a library profile, the ends on stored, inherited,
association-injected or derived properties, the type an enumeration value, the data space as the graph, `USE` to
select it; "awesome, really like this plan"; the class form primary, §5.7; identity layered as C5 says), C6
(recursion is required: the native is built), C9 (Cypher 25 is the default; the spec is the newest Neo4j Community
release at each landing: "neo4j latest release"), the names (the profile `graph::graph` with `Edge`, `from`, `to`,
`type`, in the style of `meta::pure::profiles::equality`: "profile name you choose best"; the library project
`projects/graph`; the model's own `RelationshipType` enumeration), and **the start: the line begins on 2026-10-15
("let's start implementing"), announced in `IN_FLIGHT` as the eleventh line; the plan is
`docs/CYPHER_PLAN_2026_10_15.md`.** Open: the native's name, shape, guard and owner (C6, in PR5's design).

---

## 5. Architecture

```
#Cypher{...}#  ─▶ CypherLexer, CypherParser (protocol/cypher/) ─▶ CypherAst (spans) ─▶ CypherToPure ─┐
                                                                                                    ▼
Typer's island arm ─▶ compiler/spec/CypherIslands: types each label's class through the model,      a Relation
                     answers the translator's questions (a label's properties and their types,      ValueSpecification
                     an association's ends, a class's keys, the variables in scope)                  │
                                                                                                    ▼
                                              NameResolver ─▶ Typer types it once ─▶ G½ … K   (unchanged)
```

### 5.1 The reader: `protocol/cypher/`

- **Hand-written**, lexer plus Pratt parser, like `SqlParser` and `IqParser`: core compiles to TeaVM WebAssembly and
  GraalVM native image, and the protocol layer depends on nothing but the JDK, `values` and JSON
  (`ArchitectureTest.protocolIsTheBottomLayer`).
- **Grammar:** Cypher 25 as the pinned release's manual spells it, with the version flag; the openCypher BNF is the
  cross-check of the productions. Every clause parses, writes included, so that a write is refused by name after
  parsing with Neo4j's wording, not as a syntax error.
- **AST:** records with spans and the written case of every name; errors positioned in the enclosing Pure source as
  `SqlIslands.position` places them.
- **Printer:** a fixpoint, `parse(print(ast)) == ast`, over the TCK's queries.
- **The function table as data:** Neo4j name (case-insensitive) → the Pure spelling and the argument rule; an unknown
  name is a positioned error naming the nearest known one.

### 5.2 The translator: Cypher AST → Relation `ValueSpecification`

The target is the Relation API, entering at `Compiler.query` through the island arm, as the SQL islands do. The
clause mapping:

| Cypher | Relation API |
|---|---|
| `MATCH (n:Label)` | `Label.all()` as a relation of the class's columns (the rows of a `project` over every property the query reads; the scope keeps the class so later hops navigate) |
| `MATCH (a)-[:prop]->(b)` | a navigation `$a.prop`, which the mapping lowers to the join; for a to-many end a row per target, as `project` flattens |
| `MATCH (a)-[:T]-(b)` (undirected) | `concatenate` of the two directions |
| a second `MATCH` | `join` on the shared variables, INNER |
| `OPTIONAL MATCH` | `join` LEFT, its `WHERE` inside the join condition (Neo4j's rule: a `WHERE` on an `OPTIONAL MATCH` is part of the match) |
| `WHERE` | `filter`, three-valued, as `SqlToPure.Tx` spells it |
| `WITH` | a pipeline cut: `extend` + `select`; `WITH DISTINCT` adds `distinct`; its `ORDER BY`, `SKIP`, `LIMIT` sort and slice at that point |
| `UNWIND list AS x` | `lateral` + `flatten` over the list value, as the Postgres island reads `unnest` |
| `RETURN items` with aggregates | `groupBy` on the non-aggregated items, `extend` for expressions over the keys |
| `RETURN DISTINCT` | `distinct` |
| `ORDER BY`, `SKIP`, `LIMIT` | `sort`, `drop`, `limit` |
| `UNION`, `UNION ALL` | `concatenate->distinct`, `concatenate` |
| `EXISTS { }`, `COUNT { }`, `COLLECT { }` | `exists`, `size`, a list aggregate, over the subquery's relation correlated to the enclosing row (the Postgres island's subquery machinery) |
| `CALL { WITH x ... }` subquery | a correlated relation joined back, or refused by name where it reads nothing of the outer row and returns many rows (a cross product: feasible, measured) |
| `collect(x)` | the list aggregate (`SqlAgg.Fn.LIST`, which `Lowerer` already emits for lists of scalars); `collect(n)` a list of Variants (C8) |
| `[(a)-->(b) \| b.name]` | the list aggregate over a correlated navigation |
| `*1..3`, `{1,3}` | the union of the fixed-length unrolls, each with the uniqueness conjuncts |
| `*`, `shortestPath`, `SHORTEST k` | the recursion native (§6), after the ruling; refused before |
| `CASE`, `coalesce`, arithmetic, strings, lists | the Postgres island's spellings, Cypher's rules (§2.3) |
| `n` (a node) as a value | a Variant built from `labels` and the properties the class declares (C8) |
| `properties(n)`, `keys(n)`, `labels(n)`, `type(r)` | the Variant's parts, or constants the model knows at compile time (`labels(n)` of a node of one class is a spelled list: C6.2's "spelled tokens", no computation) |

The generated Pure should read like hand-written Pure, as the IQ showcase's does; the harness dumps it per case so that
a review reads it.

### 5.3 Plumbing (the cross-area edits a landing announces)

The island type sites, from the IQ island's own diff and a grep of `SqlIsland` today:

| File | Edit |
|---|---|
| `protocol/spec/CypherIsland.java` (new) | the record: text, span, version as written (null when the text carries none) |
| `protocol/spec/ValueSpecification.java` | the sealed permits list, `children()`, `withPos` (three sites) |
| `parser/SpecParser.java` | the `parseDsl` arm for `Cypher` (and the sugar types, if C1 wants them); the LEGEND_ENGINE surface keeps its refusal |
| `protocol/SpecIslandReader.java` | the `classInstance` type `Cypher` |
| `protocol/ProtocolEmitter.java` | two sites (the island with and without a span) |
| `protocol/PureComposer.java` | `#Cypher{` + text + `}#` |
| `compiler/NameResolver.java` | the island passes through, as `SqlIsland` does |
| `compiler/spec/Typer.java` | the island arm, one line, to `CypherIslands.desugar` |
| `compiler/spec/CypherIslands.java` (new) | the seam: the model's classes, properties, associations, keys and store types, the variables in scope; positions; the error kinds |
| guards | `CodeShapeGuardrailTest` (the reader's cursor fields), `PlatformNamesGuardrailTest` (keyword arms), `OwnDialectCensusTest`, `OwnCorpusConformanceTest`, `FixtureAdjudicationTest`, `corpus-manifest.tsv` (the island as a declared lite extension, LITE-DESIGN-cypher-island, `docs/OWN_CORPUS_DECISIONS.md` §15) |
| docs | `docs/SEMANTICS_REGISTER.md` rows (C4's property rule, string order, case folding, Variant egress, each refusal that is a difference from Neo4j), `docs/GATES.md`, `docs/IN_FLIGHT.md` |

Not touched by the first slices: `lowering/`, `sql/dialect/`, `builtin/`, the lexer (the island opening lexes already),
`SqlToPure.java` and `InputDialect.java` (the seventh and tenth lines' seam).

### 5.4 Constraints

- Invariant 7b: `protocol/**` depends only on protocol, values, JSON and the JDK.
- TeaVM: no JDK method it lacks; each landing builds `//sdlc-server:page` and `//wasm:planner`.
- `//parser-equivalence:diagnostics` on every landing that touches a parser, lexer or protocol file.
- Java never computes a value: `labels(n)` of a known class is a spelled constant, `size([1,2,3])` is the database's.

### 5.5 Bolt, for later: the wire

Bolt is Neo4j's binary client protocol, the counterpart of Postgres's frontend/backend protocol. Every Neo4j driver
(Java, Python, JavaScript, .NET, Go) and every tool built on them (Neo4j Browser, Bloom, the BI connectors, the
LangChain and LlamaIndex graph stores) speaks it over one TCP connection, port 7687 by default: a version handshake,
then messages (`HELLO`, `LOGON`, `RUN` with the query text and its parameters, `PULL`, `DISCARD`, `BEGIN`, `COMMIT`,
`ROLLBACK`, `RESET`, `GOODBYE`) and records streamed back, every value encoded in PackStream (integers, floats,
strings, lists, maps, and structures for nodes, relationships, paths and the temporal types). Other graph databases
implement it so that Neo4j's drivers work against them (Memgraph, Amazon Neptune).

A **Bolt server in lite** is the graph counterpart of the pgwire server the server program plans for SQL
(`docs/SERVER_PROGRAM_2026_09_26.md` W3; the Postgres study's D3: hand-written, JDK only): it would let any Neo4j
driver, and Neo4j Browser, connect to a legend-lite server as if it were a Neo4j, run the island's translation of each
`RUN` over the Legend model, and stream the rows back as Bolt records, nodes as PackStream structures. The protocol's
documentation is CC BY-NC-SA 4.0 (§3.2): read for behaviour, never copied, as pgwire is written from Postgres's
manual. The cheaper first wire is Neo4j's **HTTP Query API** (`POST /db/{name}/query/v2`, JSON in and out), which lite's
existing HTTP server can serve with no new transport. Both are after the island itself (§9, "Later").

### 5.6 Relationship properties: the edge class, the data space as the graph, `USE`

**The question.** Neo4j's relationships carry properties (`[:ACTED_IN {role: 'Neo'}]`), have their own identity,
and may run in parallel (two `ACTED_IN` between one actor and one movie). Legend's `Association` is two properties
and nothing else: no properties of its own, no keys, no mapping beyond the join. What is the right Legend element
for a relationship with properties, and where does the model say so?

**The candidates, and why four of them are wrong here:**

| Candidate | Verdict |
|---|---|
| Extend `Association` with properties (UML's association class in the metamodel) | a fork of Pure's metamodel: the engine's parser, protocol and compiler would refuse every such model; `parser-equivalence` would carry a permanent exception. Rejected |
| Read the relationship's properties off the mapping's join table (the bridge table's extra columns) | the wrong layer: the physical mapping would define the logical graph, the properties would be store-typed and per-store, and a model-to-model mapping would have none. Legend's tenet is that the model is the semantic layer. Rejected |
| Infer an edge from a class's shape (two to-one properties to keyed classes) | guessing: invariant 4. A class with two to-one ends is often not an edge (an `Address` with a `country` and a `region`). Rejected |
| A new element kind, `###Graph`, a GQL "graph type" or SQL/PGQ "property graph" over the model, naming node types, edge types, ends and directions | standards-shaped, but it duplicates the data space (which already is the consumer's view: the exposed elements, the default mapping and runtime), and it costs a section grammar (`DataSpaceSectionGrammar` is 727 lines) plus a model record, the protocol reader and emitter, the composer, `FromProtocol` and the census. Kept as a later option only if one data space must expose several graphs |
| **An edge class, declared by a stereotype from an ordinary `Profile` in a library project; the data space as the graph; `USE` to pick it** | **the proposal** |

**The design.**

1. **An edge is a class.** It is the only Legend element that can hold a relationship's properties (ordinary
   properties), its identity (its mapping's primary key, C5), its mapping (a class mapping over the bridge table, its two ends
   as to-one property mappings through joins: exactly SQL/PGQ's edge table with a source key and a destination key),
   its constraints, its documentation, its milestoning, and later its writes (`CREATE (a)-[:Acting {role}]->(m)` is
   a new instance of it). Nothing new is invented below the class.
2. **The class says it is an edge, with an ordinary Pure `Profile` that compiles everywhere** (the user,
   2026-10-15: "if we build it as a real profile that compiles in pure/engine then we just have to inject that
   profile"). The profile is a plain element in a library project, as `core_types::CtGovernance` already is in
   `projects/core-types/model.pure` for every project that depends on it:

   ```
   ###Pure
   Profile graph::graph
   {
     stereotypes: [Edge, from, to, type];
   }
   ```

   Four stereotypes (five since 2026-10-17: `Open`, a class of an open graph, §12.45) and no tags: nothing in the
   declaration is a string. A model that wants edge classes depends on
   that project (through the depot, as on any library) and writes the edge in either of Legend's two forms of a
   link. **The idiomatic form, an association class decomposed as Legend can say it** (the user, 2026-10-15: "should
   that be moved to association and only the property exist on the class?"): the class holds the relationship's
   own data and identity, and two associations attach it to its ends, which also gives the ends their inverse
   navigation (`$actor.actings.movie`) and draws the edge in a diagram as a box between two lines:

   ```
   import graph::*;
   Class <<graph.Edge>> demo::Acting
   {
     role: String[1];
     since: StrictDate[0..1];
   }
   Association demo::ActorActing { <<graph.from>> actor: demo::Actor[1]; actings: demo::Acting[*]; }
   Association demo::ActingMovie { <<graph.to>>   movie: demo::Movie[1]; actings: demo::Acting[*]; }
   ```

   **The compact form**, the ends as the class's own properties, with no inverse navigation:

   ```
   Class <<graph.Edge>> demo::Acting
   {
     <<graph.from>> actor: demo::Actor[1];
     <<graph.to>>   movie: demo::Movie[1];
     role: String[1];
   }
   ```

   **Why `from` and `to` remain in the association form** (the user asked, 2026-10-15): direction. A Pure
   association has none; it is two properties, traversed both ways by construction. A Neo4j relationship always
   has one, and `(a)-[r]->(m)`, `startNode(r)` and a self-typed `(p:Person)-[:KNOWS]->(q:Person)` depend on it. The
   marker and the two injected ends tell the compiler which two classes an edge joins, never which way. Labels in a
   pattern would disambiguate only sometimes; declaration order would make a reorder flip every relationship
   silently; one stereotype with the other by elimination is one explicit and one inferred; reading edges as
   undirected would deviate from Neo4j. So the two stereotypes carry the one bit an association cannot. Seen the
   other way: without them the edge class is a node with two relationship types, and the query writes two hops,
   `(a)-[:actings]->(x:Acting)-[:movie]->(m)`, each directed by its property name; `from` and `to` are exactly the
   declaration that collapses that shape into one directed relationship with properties. A plain association used as
   a relationship type needs no stereotype because the pattern names an end property (`[:firm]`, `[:employees]`),
   whose owner-to-target is the direction; an edge class's type names the class, which names no end.

   The rule is one for both: the edge class has exactly one to-one class-typed property carrying `<<graph.from>>`
   and one carrying `<<graph.to>>`, **wherever that property is declared**: on the class, inherited, or injected by an
   association (`ModelContext` already sees association-injected navigation uniformly as the class's properties).
   An association that touches an edge class is one of its ends, never a relationship type of its own (nor is its
   inverse `actings`); one whose edge-class-side property carries neither stereotype is an error, since an edge has
   two ends and no third link. The modeler chooses the form, as Legend always lets a modeler choose a property or an
   association; the island cannot tell the difference, and neither can the engine.

   **The ends are stereotyped properties, not strings naming properties** (the user, 2026-10-15: "any way we can do
   it more type-safe?"). That is how `<<equality.Key>>` is applied too: on the property, and read by
   `ClassCompiler` by the profile's full name and the stereotype's name (`ClassCompiler.java:58`). Every reference in
   the declaration is then an element reference the engine's own compiler checks: the profile exists, the
   stereotypes exist in it, the property exists and has the type and multiplicity written beside it. Nothing names a
   property by its spelling, so a renamed property cannot silently detach its end.

   **The relationship type is typed: an enumeration value, spelled in a stereotyped derived property** (the user,
   2026-10-15: "should type also be a typed type?", then "any way to use enums, inheritance, any combination?").
   A tagged value is a string in Pure; a stereotype is a typed marker but cannot carry a value; the one place a class
   can state a *typed constant* that the engine checks is a derived property whose body is a spelled value. So the
   model declares its catalog of relationship types as an enumeration, and each edge class names its type by one:

   ```
   Enum demo::RelationshipType { ACTED_IN, DIRECTED, KNOWS }

   Class <<graph.Edge>> demo::Acting
   {
     <<graph.type>> type() { demo::RelationshipType.ACTED_IN }: demo::RelationshipType[1];
     role: String[1];
   }
   ```

   The engine checks all of it: the enumeration and its value exist, the body's type is the declared return type,
   the stereotype exists in the profile. Lite reads it in phase F without typing the body (invariant 6): the derived
   property carrying `<<graph.type>>` must have a body that is exactly one spelled enumeration value (one
   `EnumValue` node of the protocol), which is what the charter's C6.2 lets the compiler read ("a spelled enum's
   name"); anything else is a compile error naming the property. The relationship type's name is the value's name:
   `(a:Actor)-[r:ACTED_IN]->(m:Movie)`, and `type(r)` is the value, written as its name on the wire, which is
   Neo4j's string. Without the derived property, the type is the class's simple name. Two edge classes may name
   the same type with different end pairs, as one Neo4j type may join several label pairs; `[:ACTED_IN]` is then
   their union and the endpoint labels choose.

   This is the typed alias the user asked for, and more: the enumeration is the graph's catalog of relationship
   types, an element a data space can expose, a DataCube filter can offer, and a query can compare against
   (`type(r) = RelationshipType.ACTED_IN` in Pure; `type(r) = 'ACTED_IN'` in Cypher). The same mechanism names an
   end the model does not own: an end property declared by another project's association, or inherited, cannot be
   stereotyped there, so the edge class declares `<<graph.from>> from() { $this.actor }: demo::Actor[1]`, a derived
   property whose body is exactly one navigation of `$this` to a to-one class-typed property; the engine types it,
   lite reads the spelled navigation. The rule stays one: the end is the property carrying the stereotype, stored or
   derived.

   **Other typed channels, considered and set aside.** A marker superclass (the user asked to see it, 2026-10-15):

   ```
   Class <<typemodifiers.abstract>> graph::Edge {}                   // the library project

   Class demo::Acting extends graph::Edge                            // the model
   {
     <<graph.from>> actor: demo::Actor[1];
     <<graph.to>>   movie: demo::Movie[1];
     <<graph.type>> type() { demo::RelationshipType.ACTED_IN }: demo::RelationshipType[1];
     role: String[1];
   }
   ```

   It replaces only the `Edge` marker: a superclass cannot type the ends (that needs a generic `Edge<From, To>`, which
   the engine refuses, and `from: Any[1]` cannot be narrowed by a subclass, since Pure has no covariant
   redeclaration) and cannot declare the type (Pure has no abstract member to override). It is as typed as the
   stereotype for that one thing, and would make "every relationship" a class (`graph::Edge.all()`) only once each
   model writes the superclass a union mapping over its edge classes, which the translator does for free as it does
   for a label-less node; and it enters the inheritance that mapping extension, equality keys, milestoning, `match`
   and graph fetch read. The stereotype marks without inheriting. A
   stored property with a default value (`type: RelationshipType[1] = RelationshipType.ACTED_IN;`) is typed but is a
   column to map, not a constant. A class constraint calling a library function with the two ends would need that
   function to exist in the engine as a Pure function and in lite as a known name, and the engine's refusal of
   generics means its signature could not check the ends' types. Each is recorded so that it is not re-proposed.

   The engine, Studio and the SDLC compile and show this as they show any stereotype and tagged value: the model is
   not lite-only. Lite alone gives it meaning: the compiler knows the profile by its full name
   (`PlatformTypes.GRAPH_PROFILE`, beside `EQUALITY_PROFILE`, which names the engine's `meta::pure::profiles::equality`
   the same way), checks in phase F that the element at that name is a `Profile` declaring exactly the stereotypes
   and the tag above (any other shape is a compile error naming the path), and reads the applications as
   `EqualityKeys` reads `<<equality.Key>>`.

   **The other type-safe route, and why not.** A generic superclass, `Class demo::Acting extends
   graph::Edge<demo::Actor, demo::Movie>`, would make the ends typed properties `from` and `to` inherited from the
   library and the end types generic arguments, with no convention at all beyond the superclass. Pure allows it,
   and lite compiles generics end to end (`docs/OWN_CORPUS_DECISIONS.md` §11). **The engine does not**: "Type and/or
   multiplicity parameters are not authorized in Legend Engine" (its §2), so such a model would be lite-only, the
   very thing the library profile avoids, and the ends would lose their domain names (`actor`, `movie`) for the
   library's (`from`, `to`). The stereotyped properties keep the names, the types and the engine.

   Three consequences, each deliberate:
   - **The prelude does not ship it.** A prelude copy beside the library's would be a duplicated element
     (`ModelIntegrity` refuses one), and it would make the model lite-only again. The library project is the one
     home; lite's `projects/graph` holds it and publishes it.
   - **The path stays out of `meta::`.** `meta::` is Pure's platform root; lite itself refuses library elements under
     `meta::pure::functions::` (`LibraryPlatformNamespaceGuardTest`), and whether the engine refuses a user element
     under `meta::` was not verified here and need not be: a plain path avoids the question.
   - **What the engine checks, and what only lite checks.** The engine checks every reference: the profile, the
     stereotypes, the properties they sit on, their types and multiplicities. Only lite checks the *rule*: exactly
     one `<<graph.from>>` and one `<<graph.to>>` property per edge class (own, inherited or association-injected),
     each `[1]` and typed by a class, on a class that carries `<<graph.Edge>>` (an end stereotype without the class
     marker, or the marker without both ends, is an error either way); no third link on an edge; an edge class never
     used as a label. A misdeclared edge compiles in the engine and fails here, in phase F, with a positioned error
     naming the class and its properties; since only lite runs the island, that is where the rule belongs, and
     Studio on lite shows it at compile time.
   - **One small model change for the association form.** The parser already reads an association end "as an
     ordinary wire property (annotations, span and all)" (`ElementParser.parseAssociationBody`), but the model record
     drops them: `AssociationDefinition.AssociationEndDefinition` holds the name, the target and the multiplicity
     only. It gains the end's stereotypes, which `ModelContext` then carries on the injected property as it carries
     a class's own. The protocol JSON already carries them (the engine's wire does), so the wire and the composer
     change nothing.

   The checks, each a compile error with Neo4j's and the model's words: exactly one property of the class (own,
   inherited or association-injected) carries `<<graph.from>>` and exactly one `<<graph.to>>`, each of multiplicity
   `[1]` (a Neo4j relationship always has both ends) and each typed by a class, which are the edge type's end node
   types; the relationship type is the `<<graph.type>>` derived property's spelled enumeration value, else the
   class's simple name; an edge class is never a label (a node pattern
   `(x:Acting)` is refused: "Acting is an edge type"); a subclass of an edge class is an edge type of its own,
   inheriting the ends; every other property, inherited ones included, is the relationship's property (`r.role`,
   `properties(r)`, `keys(r)`); a to-many property to a class is refused on an edge (a relationship has two ends),
   the associations' inverse ends excepted, which are the ends seen from the node.
3. **Identity.** The edge's identity is Pure's instance equality (C5): its `<<equality.Key>>` where the class
   declares one (an edge keyed by its two ends says one relationship per pair), else the row itself, realized by the
   mapping's primary key, the bridge table's `PRIMARY KEY` or the mapping's `~primaryKey(...)`, which the model
   never shows. `DISTINCT r`, `collect(r)`, `count(DISTINCT r)`, relationship uniqueness (`DIFFERENT RELATIONSHIPS`)
   and path equality use it. Parallel relationships are rows with distinct identities, as in Neo4j. A keyless edge
   whose mapping has no primary key is refused wherever identity is needed, as a node class is (C5).
4. **A relationship without properties stays an association or a property.** `(p:Person)-[:firm]->(f:Firm)` reads
   `Person.firm`; `(f)-[:employees]->(p)` the other end; `(p)-[:PersonFirm]-(f)` the association by name. Its
   identity is the pair of its ends' keys, `properties(r)` is `{}`, `type(r)` the name. Where Neo4j would hold two
   parallel relationships of one type between the same two nodes, an association holds one link: a
   `SEMANTICS_REGISTER` row, and the edge class is the complete representation whenever that matters.
5. **Direction.** For an edge class, from the `from` end to the `to` end. For a property, from its owner to its
   target. An undirected pattern `-[:T]-` is the union of the two orientations, each relationship once, as Neo4j
   matches it. Labels on the endpoints narrow each end to the label's class or a subclass.
6. **The graph is the data space.** Which classes and edges a query sees, and the mapping and runtime it runs on, are
   what a `DataSpace` already declares (its `elements`, its execution contexts). The island selects one with
   Neo4j's own clause, `USE demo::MovieSpace MATCH ...`; without `USE`, the island runs as any Pure query does, on
   the `from(mapping, runtime)` that follows it, over every class the mapping maps. This is the SQL study's own
   choice for the Postgres wire ("database = a project or data space", its §3.3), and it keeps every projection fact
   in a Legend element that exists.
7. **Names are the model's, and typed.** A relationship type is the edge class's `<<graph.type>>` enumeration value
   when it declares one, else the edge class's name, the association's name or the property's name; there is no
   renaming layer of strings. Pure has no type alias (the user asked, 2026-10-15): its element kinds are classes,
   associations, enumerations, profiles, functions, mappings and the runtime and tooling elements, and the one
   alias-shaped construct, `Primitive P extends Integer`, is for primitives; a subclass is not an alias (it
   narrows). The enumeration value above is the typed second name. A zero-argument function returning the class's
   instances, `function demo::ACTED_IN(): demo::Acting[*] { demo::Acting.all() }`, would be more than an alias, a
   typed view relationship type resolved as Legend SQL resolves `func('...')` sources and inlined by G½; noted as a
   later option of its own.
8. **Patterns over edge classes lower as joins.** `(a:Actor)-[r:Acting]->(m:Movie)` is `Acting.all()` joined to
   its `actor` and `movie` ends through the mapping, with the endpoint labels as filters; `r.role` is the row's
   column; `type(r)` a spelled constant; `startNode(r)` and `endNode(r)` the ends. A variable-length pattern over an
   edge class unrolls with two joins per hop. Nothing in the lowering is new: it is navigation.

**What it costs, and what it does not.** New: the library project `projects/graph` with the one profile, the
constant `PlatformTypes.GRAPH_PROFILE`, a phase-F reader beside `EqualityKeys` (`GraphEdges`: the validation and the
resolved ends per class), the stereotypes kept on association ends (above), the island's resolution of a
relationship type to an edge class, an association or a property, and the `USE` clause. Untouched: the parser
(stereotypes parse today, on class and association properties alike), the protocol, the mapping layer, the data
space, the prelude. **No lite-only model construct**: a model with edge classes compiles in the engine, in Studio
and here, byte for byte the same text; the own-corpus census needs no new family for it. The one convention is the
profile's name, which is the same kind of convention as `meta::pure::profiles::equality`, and it is checked, never
assumed. **No string anywhere in the declaration**: four stereotypes, an enumeration and its values, each an element
reference the engine checks.

**What this is not.** It is not a shortcut around the association: the association remains the right element for a
relationship with no properties, and the edge class is the right element for one with properties, which is how a
UML association class and a GQL edge type relate to a plain association. The two spellings coexist in one model and
one query.

### 5.7 The worked example: every feature in one model (the user's ask, 2026-10-15)

One model with the library profile, the enumeration, keyed and keyless nodes, a subclass label, both regular-edge
forms, the three property-edge shapes (the class form, a self-typed edge, ends through associations), the store,
the mapping, the runtime and the data space; then the queries, each with what it reads and the Pure the island
builds. The mapping has nothing new in it: edge classes on their bridge tables, every end a join, identity from the
tables' keys, the subclass by a filter, as `AssociationIntegrationTest` writes its own.

**The library project `projects/graph`, one element:**

```
###Pure
Profile graph::graph
{
  stereotypes: [Edge, from, to, type];
}
```

**The model:**

```
###Pure
import graph::*;
import demo::*;

Enum demo::RelationshipType { ACTED_IN, DIRECTED, KNOWS }

// ---- nodes ----
Class demo::Person { name: String[1]; born: Integer[0..1]; }          // keyless: a node is the row, realized by T_PERSON.ID (C5)
Class demo::Actor extends demo::Person { stageName: String[0..1]; }   // its nodes carry the labels Actor and Person
Class demo::Firm { legalName: String[1]; }
Class demo::Movie
{
  <<equality.Key>> title: String[1];                                  // keyed: a node is its business key (C5)
  <<equality.Key>> year: Integer[1];
  studio: Firm[0..1];                                                 // REGULAR EDGE, one way: a property, owner -> target
}

// ---- regular edge, both ways: ONE association, no properties ----
Association demo::Employment { employees: Person[*]; firm: Firm[0..1]; }

// ---- property edge, class form (the primary form): one element ----
Class <<graph.Edge>> demo::Acting
{
  <<graph.from>> actor: Actor[1];
  <<graph.to>>   movie: Movie[1];
  <<graph.type>> type() { RelationshipType.ACTED_IN }: RelationshipType[1];
  role: String[1];
}

// ---- property edge, self-typed: direction is knowable only from the model ----
Class <<graph.Edge>> demo::Knows
{
  <<graph.from>> knower: Person[1];
  <<graph.to>>   known: Person[1];
  <<graph.type>> type() { RelationshipType.KNOWS }: RelationshipType[1];
  since: StrictDate[1];
}

// ---- property edge, ends through associations: when Person and Movie should navigate to it in Pure ----
Class <<graph.Edge>> demo::Directing
{
  <<graph.type>> type() { RelationshipType.DIRECTED }: RelationshipType[1];
}
Association demo::DirectorDirecting { <<graph.from>> director: Person[1]; directings: Directing[*]; }
Association demo::DirectingMovie    { <<graph.to>>   movie: Movie[1];     directings: Directing[*]; }
```

**The store, mapping, runtime and data space:**

```
###Relational
Database demo::DB
(
  Table T_PERSON (ID INTEGER PRIMARY KEY, NAME VARCHAR(100), BORN INTEGER, FIRM_ID INTEGER, IS_ACTOR INTEGER, STAGE_NAME VARCHAR(100))
  Table T_FIRM (ID INTEGER PRIMARY KEY, LEGAL_NAME VARCHAR(100))
  Table T_MOVIE (ID INTEGER PRIMARY KEY, TITLE VARCHAR(200), YEAR INTEGER, STUDIO_ID INTEGER)
  Table T_ACTING (ID INTEGER PRIMARY KEY, ACTOR_ID INTEGER, MOVIE_ID INTEGER, ROLE VARCHAR(100))
  Table T_KNOWS (KNOWER_ID INTEGER PRIMARY KEY, KNOWN_ID INTEGER PRIMARY KEY, SINCE DATE)
  Table T_DIRECTING (ID INTEGER PRIMARY KEY, DIRECTOR_ID INTEGER, MOVIE_ID INTEGER)
  Filter ActorFilter(T_PERSON.IS_ACTOR = 1)
  Join PersonFirm(T_PERSON.FIRM_ID = T_FIRM.ID)
  Join MovieStudio(T_MOVIE.STUDIO_ID = T_FIRM.ID)
  Join ActingActor(T_ACTING.ACTOR_ID = T_PERSON.ID)
  Join ActingMovie(T_ACTING.MOVIE_ID = T_MOVIE.ID)
  Join KnowsKnower(T_KNOWS.KNOWER_ID = T_PERSON.ID)
  Join KnowsKnown(T_KNOWS.KNOWN_ID = T_PERSON.ID)
  Join DirectingDirector(T_DIRECTING.DIRECTOR_ID = T_PERSON.ID)
  Join DirectingMovie(T_DIRECTING.MOVIE_ID = T_MOVIE.ID)
)

###Mapping
import demo::*;
Mapping demo::Map
(
  Person: Relational { ~mainTable [demo::DB] T_PERSON  name: [demo::DB] T_PERSON.NAME, born: [demo::DB] T_PERSON.BORN }
  Actor: Relational  { ~filter [demo::DB] ActorFilter ~mainTable [demo::DB] T_PERSON
                       name: [demo::DB] T_PERSON.NAME, born: [demo::DB] T_PERSON.BORN, stageName: [demo::DB] T_PERSON.STAGE_NAME }
  Firm: Relational   { ~mainTable [demo::DB] T_FIRM  legalName: [demo::DB] T_FIRM.LEGAL_NAME }
  Movie: Relational  { ~mainTable [demo::DB] T_MOVIE  title: [demo::DB] T_MOVIE.TITLE, year: [demo::DB] T_MOVIE.YEAR,
                       studio: [demo::DB]@MovieStudio }
  Acting: Relational { ~primaryKey([demo::DB] T_ACTING.ID) ~mainTable [demo::DB] T_ACTING
                       role: [demo::DB] T_ACTING.ROLE, actor: [demo::DB]@ActingActor, movie: [demo::DB]@ActingMovie }
  Knows: Relational  { ~mainTable [demo::DB] T_KNOWS  since: [demo::DB] T_KNOWS.SINCE,
                       knower: [demo::DB]@KnowsKnower, known: [demo::DB]@KnowsKnown }
  Directing: Relational { ~mainTable [demo::DB] T_DIRECTING }
  demo::Employment: Relational { AssociationMapping ( employees: [demo::DB]@PersonFirm, firm: [demo::DB]@PersonFirm ) }
  demo::DirectorDirecting: Relational { AssociationMapping ( director: [demo::DB]@DirectingDirector, directings: [demo::DB]@DirectingDirector ) }
  demo::DirectingMovie: Relational { AssociationMapping ( movie: [demo::DB]@DirectingMovie, directings: [demo::DB]@DirectingMovie ) }
)

###Connection
RelationalDatabaseConnection demo::Conn { type: DuckDB; specification: DuckDB { }; auth: Test; }

###Runtime
Runtime demo::RT { mappings: [ demo::Map ]; connections: [ demo::DB: [ environment: demo::Conn ] ]; }

###DataSpace
DataSpace demo::MovieSpace
{
  executionContexts: [ { name: 'default'; mapping: demo::Map; defaultRuntime: demo::RT; } ];
  defaultExecutionContext: 'default';
  elements: [ demo ];
}
```

An `Actor` row is a `Person` row: both sets read `T_PERSON`, and a keyless node's identity is its table's key in
whichever set reads it, so `(n:Actor)` and `(n:Person)` meet the same node (the identity half of C5, which PR3
needs on the product lane).

**Regular edges.**

| Cypher | What it reads | The Pure the island builds |
|---|---|---|
| `USE demo::MovieSpace MATCH (p:Person)-[:firm]->(f:Firm) RETURN p.name, f.legalName` | the association's end `Person.firm`; `USE` picks the data space | `Person.all()->filter(p\|$p.firm->isNotEmpty())->project(~[name: p\|$p.name, legalName: p\|$p.firm.legalName])` |
| `MATCH (f:Firm)-[:employees]->(p:Person) RETURN f.legalName, count(p) AS headcount` | the other end, opposite direction, grouped: one row per firm with at least one employee | `Firm.all()->filter(f\|$f.employees->isNotEmpty())->project(~[legalName: f\|$f.legalName, headcount: f\|$f.employees->size()])` |
| `MATCH (p:Person)-[:Employment]-(f:Firm) RETURN p.name, f.legalName` | the association by name, either direction | the union of the two |
| `MATCH (m:Movie)-[:studio]->(f:Firm) RETURN m.title, f.legalName` | the property; owner to target | `Movie.all()->filter(...)->project(...)` |
| `MATCH (f:Firm)<-[:studio]-(m:Movie) RETURN f.legalName, m.title` | the same rows from the firm's side | the same Pure |
| `MATCH (f:Firm)-[:studio]->(m:Movie)` | nothing: `Firm` has no `studio` | no rows, and a note at compile time |
| `OPTIONAL MATCH (p:Person)-[:firm]->(f:Firm) RETURN p.name, f.legalName` | a left join; `null` where unemployed | `Person.all()->project(~[name: p\|$p.name, legalName: p\|$p.firm.legalName])` with the empty navigation kept |
| `MATCH (p)-[r:firm]->(f) RETURN type(r), properties(r)` | | `'firm'`, `{}` |

**Property edges.**

| Cypher | What it reads | The Pure the island builds |
|---|---|---|
| `MATCH (a:Actor)-[r:ACTED_IN]->(m:Movie) RETURN a.name, a.stageName, r.role, m.title` | the edge class, from `actor` to `movie`; the `Actor` label is the filtered set | `Acting.all()->project(~[name: x\|$x.actor.name, stageName: x\|$x.actor.stageName, role: x\|$x.role, title: x\|$x.movie.title])` |
| `MATCH (m:Movie)<-[r:ACTED_IN]-(a) WHERE r.role = 'Neo' RETURN type(r), properties(r), startNode(r).name` | the same rows from the movie's side | `type(r)` is `RelationshipType.ACTED_IN`, written `'ACTED_IN'`; `properties(r)` the Variant `{role: 'Neo'}`; `startNode(r)` the actor |
| `MATCH (d:Person)-[:DIRECTED]->(m:Movie) RETURN d.name, collect(m.title) AS titles` | the edge whose ends come through the associations; a list-valued aggregate | `Directing.all()->groupBy(~[name: x\|$x.director.name], ~[titles: x\|$x.movie.title: y\|<the list aggregate>])`: the registry has no `list` native today, so the aggregate's Pure spelling is PR2's to settle (it lowers to the `LIST` reducer the MIR already has) |
| `MATCH (a:Person)-[k:KNOWS]->(b:Person) RETURN a.name, b.name, k.since` | self-typed: `knower` to `known`, from the model, since the labels cannot tell | `Knows.all()->project(~[a: x\|$x.knower.name, b: x\|$x.known.name, since: x\|$x.since])` |
| `MATCH (a:Person)<-[k:KNOWS]-(b:Person) RETURN a.name, b.name` | reversed: `a` is the known | the same rows with the columns swapped |
| `MATCH (a:Person)-[k:KNOWS]-(b:Person) RETURN a.name, b.name` | undirected: both orientations, each relationship once | the union of the two orientations |
| `MATCH (a:Person {name: 'Ann'})-[:KNOWS*1..3]->(c) RETURN DISTINCT c.name` | one, two and three hops unrolled, with the relationship-uniqueness conjuncts | three navigations of `Knows` concatenated, `distinct` |
| `MATCH (n:Actor:Person) RETURN n.name` | both labels: the `Actor` set, whose rows are also `Person` rows | `Actor.all()->project(~[name: n\|$n.name])` |
| `MATCH (n:Person) RETURN count(DISTINCT n)` | keyless: each row one node, actors included, identity `T_PERSON.ID` | `Person.all()->removeDuplicates()->size()` |
| `MATCH (m:Movie) RETURN count(DISTINCT m)` | keyed: two rows with one `title` and `year` are one node | `Movie.all()->removeDuplicates()->size()`, lowered by the keys |
| `MATCH ()-[r:ACTED_IN]->() RETURN count(DISTINCT r)` | keyless edge: each row one relationship, identity `T_ACTING.ID` | `Acting.all()->removeDuplicates()->size()` |
| `MATCH (x:Acting) RETURN x` | | refused: "Acting is an edge type, not a label" |

**Versions, composition and refusals.**

| Cypher | What happens |
|---|---|
| `MATCH (p:Person) FILTER p.born > 1980 RETURN p.name` | Cypher 25, the default: `FILTER` is read |
| `CYPHER 5 MATCH (p:Person) WHERE p.born > 1980 RETURN p.name` | the frozen version, as Neo4j selects it |
| `#Cypher{ MATCH (a:Actor)-[:ACTED_IN]->(m:Movie) RETURN a.name AS actor, m.title AS title }#->filter(x \| $x.title->startsWith('The'))->from(demo::Map, demo::RT)` | the island composes with Pure; no `USE`, so the `from` supplies the context |
| `MATCH (p:Person) RETURN p.salary` | refused: no class of label `Person` declares `salary`; `Person` declares `name`, `born` (C4; a register row, Neo4j answers `null`) |
| `CALL db.labels()` | refused by name: no procedures |
| `MATCH (p:Person)-[:$($t)]->() RETURN p` | refused: a dynamic type; the model is the schema |
| `CREATE (:Person {name: 'Bob'})` | refused by name until F1 gives lite writes |
| `MATCH p = ANY SHORTEST (a:Person)-[:KNOWS]-+(b:Person) RETURN p` | refused where ties can occur; `ALL SHORTEST` and `SHORTEST 2 GROUPS` answer |
| `MATCH (a:Person)-[:KNOWS*]->(c) RETURN c.name` | refused by name until PR5's native lands |

**The class form is the primary form** of a property edge (the user's review of 2026-10-15: one element for one
relationship type, the shape a Neo4j user and a Legend link entity share); an end moves into an association only
when a model wants to navigate from a node to its edges in Pure or DataCube, as `Directing` shows. §5.6's earlier
wording that called the association form idiomatic is superseded by this section.

**Verified (2026-10-15, the user: "feel good about the Pure?").** The three code blocks above, concatenated as
one text, were compiled by legend-lite at `74d873d` through its every-error route
(`POST /api/lite/v1/compilation/compile`, `Compiler.compileErrors`: A to F, then every body), in this cloud
container after `tools/cloud-distdir.sh`: **`{"errors":[]}`** (re-run after the profile was renamed `graph::graph`:
the same). So the profile, the stereotypes on the class, on
stored properties, on derived properties and on association ends, the derived property's enumeration body, the
subclass with a filter over its superclass's table, the edge classes over their bridge tables, the association
mappings, the DuckDB connection, the runtime and the data space all parse and compile, bodies included.
Three negative controls, each a one-token edit of the same text:
- `RelationshipType.NOPE` in the derived property: refused, "in function 'demo::Acting$prop$type': enumeration
  demo::RelationshipType has no value 'NOPE'". The body is typed.
- `actor: Actorr[1]`: refused, "Unknown type: 'Actorr'". The ends are resolved.
- `<<graph.Edgee>>`, a stereotype the profile does not declare: **accepted, `{"errors":[]}`.** legend-lite does not
  check a stereotype or a profile reference against the profile today (no such check exists in `core/`, and
  `docs/LENIENCY_CATALOG.md` does not list the leniency); legend-engine refuses it ("Can't find stereotype"). Two
  consequences: the `GraphEdges` reader of PR3 validates the graph profile's references itself, so a misspelled
  `<<graph.from>>` is an error here as it is in the engine; and the general leniency is its own finding for the
  compiler line, recorded with this verification, since a model that compiles here with a misspelled stereotype
  fails in the engine.
**Verified against the real legend-engine too (2026-10-15).** The same text, through the repository's engine
runner (`bazel run //tools/engine-runner:testable -- example-model.pure`, legend-engine at the pinned 4.145.0 from
`@maven_runner`, parse and compile with no testables): **parse 432 ms, compile 4,686 ms, no error, exit 0** (re-run
after the profile was renamed `graph::graph`: parse 482 ms, compile 6,267 ms, no error). The
model with its edge classes, the profile, the stereotyped properties, derived properties and association ends, the
enumeration body, the mapping, the DuckDB connection, the runtime and the data space is engine-compatible, byte for
byte the same text, which is the claim of §5.6. The engine's answers to the controls, each an `EngineException`
with exit 1: `<<graph.Edgee>>`, "Can't find stereotype 'Edgee' in profile 'graph'"; `RelationshipType.NOPE`, "Can't
find enum value 'NOPE' in enumeration 'RelationshipType'". So the engine checks the profile's stereotypes and lite
does not: the asymmetry above, measured on both sides.

---

## 6. Recursion: what Pure lacks, what the databases have, what the native is

### 6.1 The need

| Construct | Needs iteration? |
|---|---|
| `*1..3`, `{1,3}`, `{,10}`, `shortestPath` with an upper bound | no: unroll to the bound (a `shortestPath` bounded at k is the first non-empty of the k unrolls, each a union) |
| `*`, `+`, `{1,}`, unbounded `shortestPath`, `SHORTEST k`, `ALL SHORTEST` | yes: breadth-first to a fixpoint |
| reachability (`EXISTS { (a)-[*]->(b) }`) | yes |
| WCC, SCC, BFS depths, SSSP, k-core, topological sort | yes: to a fixpoint |
| PageRank, ArticleRank, eigenvector, HITS, CDLP | yes: a fixed count of iterations, or to a tolerance |
| degree, triangles, LCC, node similarity, link prediction, modularity, conductance | no: joins and aggregates |

### 6.2 What the databases have

Every backend lite renders documents a recursive CTE: Postgres, DuckDB, H2, SQLite, MariaDB and MySQL, Snowflake,
BigQuery (top-level only, which `FoldsAsRecursiveCtes` already handles for the fold), Databricks, Trino (with a
depth limit) and ClickHouse. Each dialect's exact rules are measured in PR5, not taken from memory. The MIR already
carries the construct: `SqlWith(ctes, body, recursive)` and `SqlSource.Cte`; `AnsiSqlRenderer` writes
`WITH RECURSIVE`.

The rules differ, and they decide the native's lowering shape:
- **Postgres** iterates a working table: the non-recursive term, then the recursive term over the previous
  iteration's rows only, until an iteration adds no row; `UNION` drops rows seen before, `UNION ALL` keeps every
  row. The recursive self-reference may appear once, not inside a subquery, not on the nullable side of an outer
  join, not under `INTERSECT` or `EXCEPT`; no `ORDER BY`, `LIMIT` or `OFFSET` in the recursive term. The SQL standard
  forbids an aggregate over the recursive reference in the recursive term and Postgres follows it (to be measured
  on the embedded Postgres 16 in the first recursion slice, with the exact message). `SEARCH BREADTH FIRST` and
  `CYCLE ... SET ... USING path` are built in (Postgres 14+).
- **DuckDB** iterates likewise, and in the recursive term the CTE's name is the last iteration's rows while
  `recurring.<name>` is the union so far; its examples put joins, `NOT EXISTS`, a scalar subquery over `recurring`
  and a window function in the recursive term. **`USING KEY`** (since 1.3) keys the rows: an iteration's row with an
  existing key replaces that row rather than appending it, which is the shape of a label that converges or a score
  that is updated per node. No depth limit; termination is the query's.
- **BigQuery:** top-level only. **Trino:** `max_recursion_depth`. **SQLite, MariaDb:** standard, measured in the
  slice.

Two lowering shapes follow, chosen per dialect by the renderer, never by the translator:
1. **A row per node per iteration** (DuckDB with `USING KEY`, and every database for frontier algorithms that need no
   aggregate over the previous iteration: BFS, reachability, WCC by minimum label with `UNION`).
2. **One row per iteration carrying the whole state as an array** (Postgres and any database that forbids an
   aggregate over the recursive reference): the recursive term reads the previous row once, unnests its arrays in a
   lateral subquery that references no CTE, aggregates there, and re-packs. PageRank over `n` nodes is a row of `n`
   doubles per iteration. The database still computes; the SQL is larger and the size bound (C7) measures it.

### 6.3 The native

What Pure has: no fixpoint. Pure's `fold` is over a spelled list; C6.3 keeps recursion out of prelude bodies; C6.4
names "recursion over row-backed trees" as residue. What is proposed is not Java recursion and not a program: it is a
**native** in the world map's first kind, one Java lowering rule per dialect, like `lateral` or `asOfJoin`, under
`meta::legend::lite::` beside the 37 lite natives `docs/LITE_REVIEW_2026_09_11.md` reviews (`joinWithPrefix`,
`asOfJoinWithPrefix` are its LITE_SURFACE precedents). A first shape, for the user to shape:

```
native function meta::legend::lite::recurse<T>(
    base: Relation<T>[1],
    step: Function<{Relation<T>[1] -> Relation<T>[1]}>[1],   // the next rows from the last iteration's rows
    mode: RecursionMode[1],                                   // UNION (new rows only, a set) or UNION_ALL
    limit: Integer[0..1]                                      // iterations; none for "to a fixpoint"
): Relation<T>[1]
```

and a keyed variant `recurseByKey(base, step, ~key, limit)` whose rows replace by key (DuckDB's `USING KEY`
natively; the array shape elsewhere). The lowering: the base relation as the non-recursive term, the step's body with
its parameter bound to the CTE's name as the recursive term, the mode as `UNION` or `UNION ALL`, the limit as an
iteration counter column the step cannot see. The Typer types `step` once, with `T` the base's row type (the step must
return the same columns: a compile error otherwise).

**The user's ruling, 2026-10-15: recursion is required**, for the graph queries and the algorithms alike. So the
native is built, as kind 1 and not C6.4's residue (the charter gains a dated note at C6.3 and C6.4 when PR5 lands).
What PR5's design still settles: the name and the shape above; that a termination guard (`limit`) is required where
the database has no depth limit, so that no query can loop (a refusal without one over an unbounded pattern, or a
default the register names); which line owns it (the database owner's `sql/dialect/` and `lowering/` files, announced
in `IN_FLIGHT`). Until PR5: refused by name, and the first four slices (§9) need nothing of it.

---

## 7. The graph algorithms

### 7.1 The catalog

Neo4j GDS 2026.07's production tier lists about 60 algorithms (and GraphSAGE in beta): the full names are in §11's
reference. Cypher itself has `shortestPath`, `allShortestPaths` and the `SHORTEST` selectors. The question for each
is the same: **is the answer a function of the input?** If it is, lite can answer it exactly and a judge can say so.

### 7.2 Three buckets

| Bucket | Algorithms | How it runs | Judge |
|---|---|---|---|
| **1. Deterministic, set-based or fixed-iteration: translate** | degree centrality; triangle count and listing; local clustering coefficient; weakly connected components; strongly connected components; breadth-first search (depths); the shortest-path **distances** of Dijkstra, Bellman-Ford, delta-stepping and A* (one answer, however computed); all-pairs shortest paths; PageRank, ArticleRank, eigenvector centrality, HITS (power iteration, a fixed count or a tolerance); k-core decomposition; topological sort (as levels); longest path in a DAG; node similarity (Jaccard, overlap, cosine) and its filtered form; the six topological link-prediction functions (Adamic-Adar, common neighbours, preferential attachment, resource allocation, same community, total neighbours); the modularity metric; conductance; CDLP as Graphalytics defines it | joins and aggregates; the recursion native for the iterative ones | Neo4j GDS, snapped; floats within the parity harness's tolerance; partitions by equivalence; Graphalytics' reference outputs where its definition is the one asked |
| **2. Deterministic but costly or tie-dependent: translate with a bound, refuse the tie** | betweenness, closeness and harmonic centrality (all-pairs); articulation points and bridges; spanning tree and k-spanning tree (the weight sum answers; the edge set only when weights are distinct); Yen's k shortest paths (ordered enumeration; refused beyond a small k); clique counting; the **path** chosen among equal-cost shortest paths; `SHORTEST k` and `ANY` where ties can occur | the same, under a size bound the slice measures, as the IQ island bounds its Pure (20,000 nodes) | the same; a tie-dependent output is refused by name where the input can tie, answered where it cannot (`ALL SHORTEST`, `SHORTEST k GROUPS`) |
| **3. Randomized or heuristic: refuse by name** | Louvain, Leiden, label propagation as GDS runs it (ties by an unstated order), speaker-listener LPA, K1 coloring, modularity optimization, approximate max k-cut, random walk, k-means, HDBSCAN, FastRP, Node2Vec, HashGNN, GraphSAGE, CELF influence maximization, the Steiner trees (heuristic), max flow and min-cost max-flow (sequential augmenting; a later study if the value alone is wanted), k-nearest neighbours (approximate), scale properties and split relationships (preprocessing, not queries) | nothing | nothing: Neo4j's own answer is not reproducible across runs, so there is no parity to claim; the refusal names the algorithm and the reason |

The count: about 30 of the 60 are bucket 1, about 10 bucket 2, about 20 bucket 3. "All of Neo4j's graph algorithms"
means buckets 1 and 2, measured, and bucket 3 refused with its reason.

### 7.3 The spellings, by example

Each is a Pure program over the model (never a Java computation), with the native where marked:

- **Degree:** `groupBy(~[node], ~[degree: r | $r.other : y | $y->count()])` over the relationship's relation; in and
  out and undirected as Neo4j's `orientation`.
- **Triangles:** three navigations `a-b`, `b-c`, `c-a` with `a < b < c` on the key, counted; LCC as the triangle
  count over `deg * (deg - 1) / 2`, Graphalytics' directed rule enforced inside.
- **WCC:** the native, `UNION` mode: each node starts as its own label (its key), each step takes the minimum label of
  its neighbours; the fixpoint is the components. Judged by partition equivalence.
- **BFS depths and reachability:** the native, `UNION` mode, the frontier as the step; depth as the iteration.
- **SSSP:** the native, keyed: each step relaxes every edge from the last iteration's improved nodes and keeps the
  smaller distance per node (Bellman-Ford in rounds; Dijkstra's answer, not its order); judged with epsilon.
- **PageRank:** the native, keyed, `limit` the iteration count: a step reads every node's score, sums the
  contributions over in-edges, adds the teleport and (in the Graphalytics profile) the sinks' redistributed mass; a
  tolerance stop as a step that returns no changed row beyond the tolerance. The two profiles (GDS's unnormalized
  formula, Graphalytics' normalized one with sinks) are two parameter sets of one program.
- **CDLP (Graphalytics):** the native, keyed and synchronous: a step counts the previous labels over in- and
  out-neighbours and takes the smallest of the most frequent; a fixed count of iterations.
- **Node similarity:** the neighbour sets as list aggregates, Jaccard as the intersection's size over the union's,
  a self-join on candidate pairs; `topK` ties refused where they can occur.

### 7.4 What the judge compares

- A score column: within the harness's float tolerance (`SqlParity.outcome`'s rule for doubles), or Graphalytics'
  epsilon where that is the spec.
- A component or community id: never by value (GDS's ids are the in-memory graph's and "not guaranteed stable", its
  WCC page says); by **equivalence of partitions**, Graphalytics' rule, implemented in the harness as a two-way mapping
  check (test-side Java, a verdict: `docs/TENET_CHARTER.md` C2c's World 1).
- A path: by its nodes and relationships in order, as the TCK writes it.
- A distance: exact for unweighted, epsilon for weighted.

---

## 8. The harness

### 8.1 Files: `core/src/test/resources/cypher-parity/`

| File | Contents |
|---|---|
| `tck/` (or the Maven artifact `org.opencypher:tck`, as `@legend_engine_src//:sql_parity_tests` reads the engine's corpus) | the feature files, test input, Apache 2.0 |
| `lite/*.yaml` | lite's own cases, Neo4j-judged: the functions (strings, numbers, lists, maps, temporal), the model-shaped cases (associations, inheritance, keys, edge classes), the algorithms |
| `graphs/*.yaml` | the models and rows the cases run over: the TCK's two named graphs and a Legend-shaped one (persons, firms, addresses, an edge class) |
| `neo4j-answers.jsonl` | Neo4j's answer per case: columns, rows in the TCK value grammar, the server version, the Cypher version, the `sqlSha`; for a scenario with a `CREATE` setup, the dumped graph it produced |
| `ledger.tsv` | per case: the DuckDB status, the Postgres status, the judge (`tck`, `neo4j`, `graphalytics`) |
| `syntax-pins.tsv` | the 156 TCK compile-time syntax errors and lite's own, ratcheted: a case that errs stays an error |

### 8.2 The two things the harness must build

1. **A Gherkin reader for the TCK's subset.** `Scenario` and `Scenario Outline` with `Examples`, the steps `Given an
   empty graph`, `Given the <name> graph`, `Given any graph`, `And having executed:`, `And parameter values are:`,
   `When executing query:`, `Then the result should be, in any order / in order / (ignoring element order for lists)`,
   `Then the result should be empty`, `Then a <Type> should be raised at <phase>: <detail>`, `And the side effects
   should be:`, `And no side effects`. Test-side Java, 300 lines; `tck-api` is Scala and is not used.
2. **The graph a scenario sets up, as tables, and its model.** The scenario's `CREATE`s are run on the oracle, and the
   oracle's graph is dumped (`MATCH (n) RETURN labels(n), properties(n)`, the same for relationships) into the answers
   file; CI seeds the lanes' tables from the dump and never interprets a `CREATE`. The **model is inferred** from the
   dump: a class per label set (a node's label set is its class; a set that extends another is a subclass), a
   property per key typed by its values (Integer, Float, String, Boolean; a list as Variant), a relationship type per
   type and label pair as an association, with the keys the harness mints. A key typed two ways across a class's nodes
   makes the scenario REFUSED by name (the TCK's `var` cases). Nodes of no label are a class `_Node`. The inferred
   model is printed as Pure beside the case, so that a review reads it.

### 8.3 The judge

`CypherParity` reuses `SqlParity`'s grid, statuses, ratchet, ledger I/O and YAML reader, with its own `outcome()`: it
reads the TCK value grammar on both sides, compares rows as a set or in the first sort key's order, nodes and
relationships by labels, type and properties, lists in order (or ignoring order where the step says), maps by key,
doubles within the tolerance, `NaN` and the infinities by name. A Variant cell from lite is read as its JSON. Nodes
never compare by id (C5).

### 8.4 Lanes and guards

- `//core:cypher_parity_test` (DuckDB) and `//core:cypher_parity_postgres_test` (embedded Postgres 16), both in
  `//gates:core`; `//core:cypher_syntax_test` for the syntax ratchet.
- UNSNAPPED, STALE, GONE, as `iq-parity`; a case pinned to `CYPHER 5` or `CYPHER 25` is snapped under that version;
  a case with no prefix under the default (C1), and its answer records which.
- The version drift guard: an answer snapped on another server version than the pinned one is STALE.
- The partition judge for component ids (§7.4).
- The randomized check, as the Postgres line's: a generator of type-correct Cypher over the Legend-shaped graph
  (patterns of one to three hops, optional matches, `WITH` pipelines, aggregates, list expressions), each query run on
  Neo4j and on both lanes.

---

## 9. Milestones and slices

Each landing: `//gates:local`, the TeaVM builds, one full CI run, an independent review by an agent that wrote none
of it, a GATES entry, this document's §12 (the record of landings, to be added), `IN_FLIGHT`.

| Slice | Content | Cases | Needs |
|---|---|---|---|
| **PR1** the line | this study with the user's decisions; `tools/neo4j-oracle/` (`snap.py`, `snap.sh`, the Docker and tarball recipes, `--write`, `--check`); the island plumbing (§5.3); the reader (every clause parses; the version flag); `CypherToPure` for `MATCH` over labels with fixed-length patterns in both directions, `WHERE`, `WITH`, `RETURN`, `ORDER BY`, `SKIP`, `LIMIT`, `DISTINCT`, literals, comparisons, boolean logic, `CASE`, `coalesce`, the implicit grouping with `count`, `sum`, `avg`, `min`, `max`; the TCK reader; the inferred models; the two named graphs; the own-corpus declaration | TCK `clauses/match` (fixed-length), `match-where`, `return*`, `with*`, `expressions/literals`, `boolean`, `comparison`, `null`, `precedence`: about 650 read-only scenarios reachable; `lite/basics.yaml` | nothing outside the protocol layer, the Typer arm and the seam |
| **PR2** values | lists and maps as Variants, `UNWIND`, `collect`, `size`, `head`, `tail`, `last`, `range`, `reverse`, comprehension, `IN`, `all`/`any`/`none`/`single`; strings and numbers from Neo4j's table (lite's own cases, the TCK being thin there); `toInteger`, `toFloat`, `toString`, `toBoolean` | `expressions/list`, `quantifier`, `string`, `typeConversion`, `map`, `conditional`, `aggregation`; `lite/functions.yaml` | |
| **PR3** the model | `OPTIONAL MATCH`, `UNION`, `EXISTS`/`COUNT`/`COLLECT` subqueries, pattern comprehensions, nodes and relationships and paths as values (C8), `labels`, `type`, `properties`, `keys`, inheritance and multi-label patterns (C3), edge classes, keys and identity (C5), dates as Pure dates | `clauses/union`, `unwind`, `expressions/graph`, `pattern`, `path`, `existentialSubqueries`, `useCases`; `lite/model.yaml` | the edge stereotype (C3) |
| **PR4** bounded paths | `*m..n`, `{m,n}`, quantified path patterns with inline `WHERE` and group variables, named paths, bounded `shortestPath`, relationship uniqueness, `ALL SHORTEST` and `SHORTEST k GROUPS` within a bound | `clauses/match` (variable-length), `lite/paths.yaml`; the randomized check | nothing new: unrolling |
| **PR5** recursion | the native (§6.3), its lowering and dialect arms; unbounded `*` and `+`, unbounded `shortestPath` and selectors, reachability; termination guards | `lite/recursion.yaml`; measured on every dialect lite renders (rendered; run on DuckDB, Postgres, H2, SQLite) | the user's ruling (given 2026-10-15); the native's design agreed; the database owner's files |
| **PR6** algorithms 1 | bucket 1: degree, triangles, LCC, WCC, BFS, SSSP distances, PageRank (both profiles), CDLP, k-core, node similarity, link prediction, modularity; the partition judge; the GDS snap through `CALL gds.*.stream` on the oracle; the Graphalytics example graphs and reference outputs | `lite/algorithms.yaml`, `graphalytics/` | PR5 |
| **PR7** algorithms 2 | bucket 2 under size bounds; the refusals of bucket 3 by name with reasons | | |
| **Later** | a Bolt server, hand-written from the protocol's behaviour as pgwire is planned (its documentation is CC BY-NC-SA: read, never copied), so Neo4j Browser and every Bolt driver can query a Legend model; writes after F1; Cypher over bare tables through "Make a class"; SQL/PGQ `GRAPH_TABLE` as a second input spelling, and as a render target where a warehouse has it | | the server program's resumption; F1 |

**First slice: PR1 alone.** It ships Cypher over the class model inside any Pure query and proves the translator
against about 650 TCK scenarios before any value, path or algorithm work starts.

---

## 10. Risks

- **The recursion native's design.** The ruling is given (C6); the risk is now the native's shape: a step lambda the
  Typer types once against the base's row type, two lowering shapes per dialect (§6.2), a termination guard, and the
  size of the SQL it writes on the array-state shape. PR5 measures each on every dialect before anything unbounded
  is accepted; the first four slices ship without it.
- **Cypher is schema-optional; lite is not.** C4 refuses what Neo4j answers with `null`. Real-world queries over
  ad-hoc properties will meet that refusal first. The register row and the error's wording ("no class of label `A`
  declares `x`; `A` declares `y`, `z`") are the mitigation.
- **The oracle is GPL.** It never enters the repository; a license review of each landing's diff checks for Neo4j
  code and manual text, as the IQ reviews check for SAP's.
- **Cypher 25 moves monthly.** A pinned release (C9) and the version drift guard; a bump is a landing.
- **Value kinds Pure lacks.** Zoned times, durations, points, vectors: refused by name; the register names each.
  The TCK's 89 temporal scenarios (1,004 expanded) are mostly out of reach until Pure spells durations.
- **The size of the SQL.** Three-valued spellings per hop, unrolled unions, array-state recursion on Postgres: the
  IQ line met a fourfold growth per nested level; the harness measures the generated SQL's size per case from the
  first landing, and the size bound refuses beyond it.
- **Ownership and contention.** The island plumbing touches ten files other lines own (§5.3); the native touches
  the database owner's. Announced in `IN_FLIGHT` before each landing, as the tenth line's were.
- **Trademark.** "Cypher" is Neo4j's registered mark; the island is "legend-lite's Cypher island", its extensions
  "implementation extensions", per the openCypher attribution notice.

---

## 11. Sources

- **openCypher** (Apache 2.0): the repository `opencypher/openCypher` at `677cbafabb8c` (2026-03-20): `LICENSE`,
  `README.adoc`, `grammar/openCypher.bnf`, `tck/README.adoc`, `tck/index.adoc`, `tck/graphs/named-graphs.adoc`,
  `tck/features/**`; the Maven artifact `org.opencypher:tck` 2024.2 (2025-07-03); opencypher.org/resources (the
  Cypher 9 reference, the M23 ANTLR and EBNF grammars, the TCK API).
- **Neo4j Cypher manual** (2026.06 and 2026.07): "Select Cypher version"; "Deprecations, additions, removals and
  compatibility"; "Equality, ordering and comparison of value types"; "Working with null"; "Lists"; "Aggregating
  functions"; "String functions"; "Mathematical operators"; "Functions" (the catalog of 176); "Patterns" and
  "Variable-length patterns"; "Shortest paths"; "GQL conformance".
- **Neo4j Graph Data Science manual** (2026.07): "Algorithm references" (the tiers); "PageRank" (the defaults, the
  unnormalized scores); "Label Propagation" (ties "arbitrarily but deterministically"); "Weakly Connected
  Components" (ids "not guaranteed stable"); the licensing of GDS: the open sources GPLv3, the distributed library
  partly closed.
- **Neo4j operations manual** (2026.06): Docker images and tags (Community without a suffix), `NEO4J_PLUGINS`,
  GDS in Community.
- **Neo4j licensing:** the `neo4j/neo4j` README (Community under `community/` is GPLv3; Enterprise closed and
  commercial); the Python driver's `LICENSE.APACHE2.txt`; `neo4j/docs-bolt` `LICENSE.txt` (CC BY-NC-SA 4.0).
- **LDBC Graphalytics:** the specification v1.0.5 (arXiv 2011.15028, CC-BY): §2.3 the six algorithms, §2.4 the
  validation rules.
- **Databases:** PostgreSQL 16 "WITH queries" (the working table, `UNION` vs `UNION ALL`, `SEARCH` and `CYCLE`);
  DuckDB "WITH clause" (`recurring`, `USING KEY`, the 1.5.0 deprecation of `UNION` under `USING KEY`) and the
  2025-05-23 `USING KEY` post; the DuckDB community extension page for DuckPGQ (1.4.4; unavailable for 1.5.x).
- **Prior art:** openCypher's CAPS announcement (2017) and the Morpheus talks; Graphiti (arXiv 2504.03182); Raqlet
  (CIDR 2026, arXiv 2508.03978); Apache AGE (age.apache.org); the Kùzu archiving reports (2025-10).
- **This repository:** `docs/LEGEND_SQL_STUDY_2026_10_11.md`, `docs/IQ_SQL_STUDY_2026_10_13.md`,
  `docs/IQ_SQL_SHOWCASE.md`, `docs/TENET_CHARTER.md` (C2, C2b, C6), `docs/WORLD_MAP.md`,
  `docs/LITE_REVIEW_2026_09_11.md`, `docs/NATIVE_PROVENANCE_2026_09_11.md` (now in `docs/HISTORY.md`), `docs/OWN_CORPUS_DECISIONS.md` §14,
  `docs/BI_AND_ETL_PLAN_2026_09_29.md` (F1), `core/.../protocol/sql/InputDialect.java`,
  `core/.../compiler/spec/SqlIslands.java`, `core/.../sql/dialect/FoldsAsRecursiveCtes.java`,
  `core/.../sql/SqlWith.java`, `core/.../lowering/InstanceEquality.java`, `tools/iq-oracle/`,
  `core/src/test/java/com/legend/testcases/SqlParity.java`.

## Upstream facts found while building

What Neo4j answered while the line was built (§12, cut), kept here because §2 does not say it or says otherwise. Each
was measured on the local Neo4j 2026.09.0 oracle and snapped into a pinned case, cited by its file under
`core/src/test/resources/cypher-parity/` or by its TCK scenario. Where Neo4j and the TCK's text disagree, the oracle's
answer is the judge (§1). Lite's deliberate differences from these answers are register rows (S57 onward).

- **Comparing values of different kinds** (corrects §2.2 and the §2.3 row "`=` across kinds"). `=` across kinds is
  `false`, and `null` only when a side is `null`; `<` across kinds is `null`, not an error (`1 < 'a'`, `'1' < 1`,
  `0 > 'x'` and `NOT (0 > 'x')` are all `null`). Numbers order across integer and float, `[1] = [1.0]` is `true`, and
  `false < true`. (`lite/twoways.yaml`, `lite/rank.yaml`; TCK Comparison2/[1], [2], MatchWhere5/[4], WithWhere5/[4].)
- **Lists order element by element.** The first unequal pair decides, even when a later element is `null`
  (`['a', 1] < ['b', null]` is `true`). A `null` or unorderable pair before any decision makes the result `null`
  (`[1, 2] >= [1, null]`, `[1, 'a'] < [1, 2]`). If every pair is equal, the longer list is greater. A list inside a
  list orders the same way (`[[1]] < [[1, 0]]`); a list beside a number is `null`. (`lite/ordering.yaml`,
  `lists_inside_lists_ordered`.)
- **Maps order** (corrects §2.2, which calls `<` on maps an error). The map with fewer keys orders first; maps of one
  size compare their sorted keys, the first that differs deciding (`{a: 2, b: 0} < {a: 1, c: 5}` is `true`); maps of
  the same keys compare their values in key order. Another kind or a `null` makes the result `null`, and
  `{a: 1} <= {a: 1.0}` is `true`. (`maps_ordered_by_size_keys_and_values`, `maps_of_row_values_ordered`.)
- **Durations do not order, except as equal.** `<` and `>` are `null`; `<=` and `>=` are `true` when the two are equal
  and `null` otherwise (`duration({days: 1}) <= duration({hours: 24})` is `null`). (`durations_compared_by_each_operator`.)
- **`ORDER BY` with non-finite numbers**, ascending: `-Infinity`, the numbers, `Infinity`, `NaN`, then `null`.
  (`lite/nonfinite.yaml`; TCK ReturnOrderBy1/[11], [12], WithOrderBy1/[21], [22].)
- **Arithmetic** (settles §2.2's "to measure"). Integer `/` truncates toward zero (`-7 / 2` is `-3`) and integer `%`
  is the truncating remainder. A float `%` is Java's (`fmod`), exact and with the sign of the dividend:
  `0.3 % 0.1` is `0.09999999999999998` and `-4.0 % 2.0` is `-0.0`. Dividing by a float zero or a literal `0` gives
  `NaN`; an integer zero read from the row is the error `/ by zero`. `^` is left-associative and binds looser than
  unary minus (`-3 ^ 2` is `9`, `4 ^ 1 ^ 3` is `64`; the manual's reading was wrong). Strings order by code point.
  (§12.4's review; `lite/exprtail`; TCK Precedence2.)
- **Functions.** `round` rounds half up toward positive infinity (`round(-2.5)` is `-2.0`). `range(0, 1, -123)` is
  `[0]` (the TCK's text says empty). `single` is `true` only when exactly one element holds and none is unknown. In
  `[x IN l WHERE c]` a `null` condition drops the element. `percentileCont` over integers is an integer when it lands
  on an element and a float when it interpolates, and both percentiles are `null` over no values.
  `max([1, 2.0, 5, null, 3.2, 0.1])` is the integer `5`. `STARTS WITH`, `ENDS WITH` and `CONTAINS` with an operand
  that is not a string give `null` (TCK String8/[8], String9/[8], String10/[8]). `toInteger('foo')` is `null`.
- **Conversions that raise.** `toBoolean`, `toInteger` and `toFloat` raise a TypeError at run time on a node,
  relationship, path, list or map; so do `toBoolean` on a float and `toFloat` on a boolean. A literal of the wrong
  type is a compile-time mismatch. (TCK TypeConversion1/[5], 2/[8], 3/[6]; `lite/errors.yaml`.)
- **`toString` of composite values** is answered by Neo4j 2026.09, where the TCK expects an error: a list is
  `[1, a, [2, true]]` (strings unquoted), a map `{a: 1, b: x}`, a node its labels `(:A:B)`, a relationship `[:T]`, a
  path keeps its directions. (TCK TypeConversion4/[10]; `lite/exprtail.yaml`.)
- **Durations** have four parts: months, days, seconds and nanoseconds. A fraction cascades down
  (`duration({days: 1.5})` is `P1DT12H`; a month is 2,629,746 seconds), and a duration times a float or divided by a
  number follows Neo4j's `DurationValue.approximate`. `toString` writes zero as `PT0S`, years and months from the
  months by truncating division (`P-1Y-2M`), and the time part as one signed count (`PT-1M-0.001S`). (`lite/temporal.yaml`,
  `lite/temporaltail.yaml`; TCK Temporal8/[7].)
- **A date plus a duration** (corrects `docs/CYPHER_SHOWCASE.md` §16's "a date takes no seconds") adds the months, then
  the days plus the whole days in the seconds part, cut toward zero: 29 days and 33 hours move a date by 30 days.
  (TCK Temporal8/[1] #3; `a_date_moved_by_a_duration_of_more_than_a_day`.)
- **Zones.** A `time` orders by its wall time minus its offset (`00:30+01:00` before `23:30Z`). In a summer-time
  overlap Neo4j (java.time) takes the earlier instant (Stockholm's 02:30 on 2017-10-29 is `+02:00`; DuckDB and
  Postgres take the later); a wall time in a gap moves forward by the gap. `toString` of a value in a named zone
  writes the offset, then the zone's name in brackets. (`lite/nanozones.yaml`; TCK Temporal3/[3], [11], Temporal6/[7].)
- **Errors raised before any clause runs.** Semantic analysis checks the whole statement first: VariableTypeConflict,
  VariableAlreadyBound, an unknown function, `ParameterMissing`, InvalidClauseComposition (an update inside EXISTS),
  and the implicit-grouping errors (`AmbiguousAggregationExpression`; "Expression in WITH must be aliased (use AS)").
  An `ORDER BY` aggregate missing from the `WITH`'s items is accepted by 2026.09, where the TCK still expects
  `UndefinedVariable` (WithOrderBy4/[13], [14]). (`lite/varkinds.yaml`, `lite/errors.yaml`, `lite/open.yaml`.)
- **Reading.** `RETURN 017` is a syntax error; `-9223372036854775808` is a valid literal; `1<-1` means `1 < -1`; a
  parenthesised node is a pattern only when a map or a relationship follows it. (§12.4's review; TCK Literals5/[9], [10].)
- **Patterns.** An `OPTIONAL MATCH` that is the first clause and finds nothing returns one row of `null`s. An undirected
  pattern over a self-loop matches once. (`lite/optional.yaml`, `lite/cycles.yaml`; TCK Graph8/[4], [7].)
- **Cypher 25 narrows `CREATE`.** A property map may not reference an entity created in the same `CREATE`; Cypher 5
  accepts it, so the TCK's setups (openCypher 9 text) run under `CYPHER 5`. (TCK WithSkipLimit1/[1], With2/[1].)
- **legend-pure and legend-engine** (read, no file:line given): legend-pure's DateTime is an instant, a literal's offset
  normalised to UTC when parsed; zones exist only in formatting (`%t{[zone]pattern}`) and on the connection.
  legend-engine's relational lowering refuses StrictTime ("not supported yet"), and its `###Relational` grammar has no
  TIME column. Pure's `Duration` is one number and one `DurationUnit`.

> Cut in the 2026-11 docs cleanup: §12, the record of landings 12.1–12.97 (2026-10-15 to 2026-10-20); its divergences are register rows, its cases are pinned under `core/src/test/resources/cypher-parity/`, and the Neo4j facts it measured are listed above. Full text: `git show de2745351:docs/CYPHER_STUDY_2026_10_15.md`.
