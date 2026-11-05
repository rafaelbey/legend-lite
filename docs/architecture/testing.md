# Testing: how lite is judged

lite is a re-implementation, so it is judged against the system it re-implements. This document says what the
oracles are, how a test body is compiled and judged, and what "the verdict" of an assertion means. It is the design;
the commands, lanes and their current numbers are operations, and live elsewhere:

- [docs/GATES.md](../GATES.md): every gate, its Bazel lane and what it holds (read the first section; the rest is the
  dated log);
- [docs/RUNNING_THE_CORPUS.md](../RUNNING_THE_CORPUS.md): running the stress corpus and the project graph;
- [docs/TEST_CORPUS_MASTER_PLAN.md](../TEST_CORPUS_MASTER_PLAN.md): what a corpus must contain to catch wrong answers
  and performance cliffs;
- [docs/CANONICAL_FORM_SPEC.md](../CANONICAL_FORM_SPEC.md): the canonical spellings the database judge compares.

The rest of the architecture is [README.md](README.md), [frontend.md](frontend.md) and [backend.md](backend.md).

## 1. The rule

- **Upstream's behavior is the specification.** lite matches legend-pure's observable outcomes and legend-engine's
  rows. A deliberate difference is a row of [docs/SEMANTICS_REGISTER.md](../SEMANTICS_REGISTER.md); a difference
  that is not a row is a defect.
- **Upstream's own tests are the main oracle**, read from the pinned legend-pure and legend-engine release
  (`MODULE.bazel` pins the jars and the source archives by checksum; tests read them as declared inputs, so no gate
  can run against a missing or wrong checkout).
- **Rows are the contract.** Golden SQL text is advisory (§6 says exactly where text decides).
- **A shape a judge cannot compare fails, as unjudged, with the shape named.** Nothing falls back to another judge
  to fill the gap.

Sources: `docs/SEMANTICS_REGISTER.md` (header), `docs/GATES.md` ("Upstream"), `docs/CARRIER_REDESIGN.md` §0,
`docs/JUDGING_TWO_MODES_2026_09_17.md` §2–§3 (git history: docs/HISTORY.md).

## 2. The oracles

| oracle | what it checks | lane (`//gates:<lane>`; gate number) |
|---|---|---|
| **parser equivalence** | byte parity with legend-engine's parser; the round trip text → JSON → text → JSON over the corpus, lite's projects and upstream's showcase projects, in the JVM and in the WebAssembly build (held to the JVM's answers) | `parser_equivalence` (8) |
| **the relational corpus** | legend-engine's own relational test functions, run through lite on DuckDB and on H2. Each lane runs the corpus twice, once per judge (§3), and joins the two per assert; the fail rosters must equal the committed ones exactly | `corpus_duckdb` (4, 11), `corpus_h2` (5) |
| **PCT, channel A** | legend-engine's Pure Compatibility Testing: the engine's own function tests, run through lite by a Pure adapter, one target per suite, per database; each database's expected failures pinned by message | `pct_duckdb` (6), `pct_h2` (7), `pct_postgres` (7P); the other databases are manual |
| **PCT, channel B** | lite's own compiler runs the same PCT sources straight from the pinned trees; its verdicts are diffed against channel A's ledgers and the engine's published manifests | `pct_channel_b` (9); every other database in `heavy` |
| **the stress corpus** | about 4,700 executable services with seeded data. Every expectation is computed from the seed by an independent oracle, never read back from an engine, and the corpus itself is checked by running it on legend-engine. lite must hold a pass floor on DuckDB and on H2 | `stress` (10) |
| **SQL parity** | upstream's Postgres parity corpus (legend-engine's end-to-end cases at the pin) as `#SQL{...}#` islands, on DuckDB and on Postgres 16; each case's status is a row of `core/src/test/resources/sql-parity/ledger.tsv`. The `#IQ{...}#` and `#Cypher{...}#` islands have parity lanes of the same shape | `core` |
| **the corpus differential** | lite's SQL over the stress seed against the corpus oracle | `core` |

Before a push to `main`, `bazel test //gates:local` runs the light lanes; the heavy ones (the corpus, PCT, stress,
parser equivalence, channel B) run in CI. `gates/BUILD.bazel` is the one list of each lane's members.

Sources: `docs/GATES.md` (first section), `gates/BUILD.bazel`, `pct/BUILD.bazel` (gates 6, 7, 9),
`spec/BUILD.bazel` (gates 4, 5, 11), `docs/RUNNING_THE_CORPUS.md` (git history: docs/HISTORY.md).

## 3. Two judges, never mixed

An assertion in a test body (`assertEquals`, `assertSameElements`, `assertSize`, ...) is judged in one of two modes,
chosen for the whole run, never per assertion:

- **HOST**, the reference. One small comparator decides every equality in the host, over two values that already
  carry their kinds, by the engine's own rules: same kind then equal by value (Integer exact, Decimal by numeric
  compare, Double exact with one declared, counted leniency of two units in the last place for values the database
  computed through its C math library, strings, booleans and dates exact, enumerations by name); collections ordered
  or as multisets as the assertion says; JSON documents parsed, numbers by value within kind, null the same as
  missing; instances by key tree, else by construction identity. It never runs SQL to judge, never converts a value
  and never consults a wire type. It mirrors what the engine runs (its assert seam and service-test comparator are
  Java), so it can be proven against the engine's published manifests. It is the verdict of record.
- **DATABASE**, the product's goal. Every comparison is a SQL predicate over the canonical spelling of each side:
  six mutually disjoint spellings (a Float spells its Float form, a Decimal its scale-preserving form with its kind
  mark, an Integer bare, temporals prefixed, strings quoted, booleans), instances by key tree or construction
  identity. The assert functions are platform natives lowered to SQL, each a boolean over spellings that raises with
  the engine's message on failure, and the database returns the verdict as a row. It never judges in Java.

**The differential gate** runs both judges over the same corpus and requires the same verdict per assertion; a
disagreement is a bug in one of them, and host mode, being small and the engine's mirror, is the reference. Today
the corpus lanes run one pass per judge and join them per assert; a third run mode, BOTH, judges every
statement-root assert with both arms in one run, keeps the host's verdict and fails, naming both verdicts, when the
database's differs (today's Java `ExecuteOptions.JudgeMode` and `AgreementArm`).

**Both modes rest on one rule: a value's kind is decided in the query, never by a judge.** Every result column
carries the kind the model declared. At the outermost select of every plan, a column declared Float whose value the
database computed in another kind is cast to DOUBLE once, in SQL; Decimal stays the database's decimal; Integer stays
an integer. Inside the query the database's own arithmetic stands. Every failed attempt before this rule was a judge
deciding kind from the carrier.

Never mixed is enforced: one class decides host equality, no verdict path consults both modes, and the mode is a
property of the run carried on its options, never a JVM-wide setting.

Sources: `docs/JUDGING_TWO_MODES_2026_09_17.md` (§1–§5), current code `ExecuteOptions.java` (`JudgeMode`),
`driver/AgreementArm.java`, `spec/BUILD.bazel` (gates 4, 11) (git history: docs/HISTORY.md).

## 4. One artifact per test body

A test body compiles to **one artifact** before any of it runs:

- a pure body (no effects) compiles to one verdict statement: its `let`s become common table expressions in
  dependency order, its asserts become verdict rows, and one fused statement is sent per connection;
- a body with effects (raw SQL, DDL, data loads) compiles to one script: the effects in order, with a verdict
  segment wherever the body asserts between effects; each segment is sent as one script;
- a body whose last statement is a value ends in a value segment.

Statement-by-statement execution was removed, not kept beside the compiler: two paths would be two implementations
of Pure semantics. A REPL is a sequence of blocks over a session whose earlier `let`s are frames. The runner only
sends the artifact and maps a database error back to the `let` or assert it came from (a fragment map). The artifact
must reproduce the engine's verdicts: stop at the first failing assert, errors named.

The SQL-text referee of §6 is the test lane's judge, never the product's, and is outside this rule. Every stage of
this design removes a walk of the body or merges two, never adds one.

Sources: `docs/BLOCK_COMPILER_HOMEWORK_2026_09_21.md` (the ratified decisions, §13, §15, §20), current code
`driver/BodyCompiler.java` (git history: docs/HISTORY.md).

## 5. Lean verdicts

The target for the database judge: **one statement per test body**, in which the product SQL of every `let` appears
exactly once, unchanged from what a user would get, inside the thinnest assert wrapper that yields a verdict row.

Lean is measured by counting, never by intuition: statements sent, bytes, CTEs, subqueries, and copies of each
product query, compared rung by rung against a hand-written lean statement. A ladder of small tests over one table,
each adding one construct, pins every statement byte for byte beside its lean target (today's Java
`ladder/LeanSqlLadderTest`); a rung closes when the two match.

The general shape that came out of it: each side of an equality is a rows relation folded to one facts row (count,
first cell, aggregated text, null and tree counts), and the verdict row reads two one-row CTEs, with no scalar
subqueries. A side declared exactly one is spelled straight over its plan; a side declared optional or many goes
through a rows CTE. The two-ULP Float leniency exists only when a side is declared Float, a compile-time fact.

A measured fact behind the counting: DuckDB's prepare time is planning, about 65 µs per plan operator, not per byte.
A reshape that cut bytes by 11% made prepare slower and was reverted. Fewer subqueries and fewer references per
assert are the lever.

Sources: `docs/LEAN_VERDICT_LADDER_2026_09_20.md` (git history: docs/HISTORY.md).

## 6. SQL-text and plan-text asserts

Many upstream tests assert the SQL text the engine generates (`assertEquals(golden, toSQLString(...))`,
`assertSameSQL`), or the text of an execution plan. Golden text is the engine's H2-flavoured SQL; lite generates its
own, for the database it runs on. Identical text proves the emitter's spelling, not the answer.

**The verdict.** A text byte-equal to the golden is the assert's verdict, in both judges (the user's ruling of
2026-09-21, "option 1"). When the texts differ, **rows are the appeal**: the golden text is replayed on an oracle
database and its rows are compared with the rows of lite's own query. A row difference fails the test whatever the
text said. Where one side cannot produce rows (a foreign dialect with no reference database, a non-executable SQL
string by design), text stays the contract, and each such case is a counted, named decline.

This narrows the first charter's rule (2026-09-01), which made rows the verdict even when the text matched byte for
byte. A measurement of the rows-first order on 2026-10-03 found one verdict that text-first hides
(`testToSQLStringForTDSStringJoin` passes on text, its rows diverge). Moving the engine-text printers out of the
product and the judge to rows-first was then proposed and staged family by family (`toSQLString`, plan text,
test-data generation, ...), each family's numbers to be brought to the user before any product output changes.

How the referee works:

- **Detection is structural.** A text assert is found by navigating the typed tree by node kind and exact callee name,
  never by sniffing text. The producer's children are its inputs: the query lambda, the mapping, the dialect, the
  runtime.
- **Four artifacts:** lite's text and the golden text (by ordinary evaluation), lite's rows (the producer's own query
  run through the normal pipeline, even when the test never ran it), and the golden rows (the replay).
- **The referee is test-only.** The platform defines a small interface (today's `SqlReplayReferee`); the harness
  registers the oracle per run. Production registers nothing, and a SQL-text assert there fails loudly: there are no
  goldens outside tests.
- **Order is decided statically.** Rows compare in order exactly when lite's typed query is ordered (it ends in a
  sort); otherwise as multisets. Within an ordered result, rows with equal sort keys compare as multisets (tie
  groups), since both databases order ties arbitrarily. Where the keys cannot be derived, the multiset verdict stays,
  counted.
- **Plan text is a program.** A plan is allocations, conditionals (for example, more than 50 elements go to a temp
  table, else inline literals) and SQL templates, generated for parameterized queries with no values supplied. The
  replayer takes each conditional both ways, fills the template, runs it on the oracle and compares rows per branch
  with lite's run on the same values. Matching characters can never prove the temp-table branch works.
- **The comparison policy is a ratified table** (NULL tokens, integer exactness, temporal canonicalization, enum
  decoding, carrier unwrapping, the engine's own bookkeeping columns excluded by a pinned, shrink-only list, and the
  rest), each rule recorded as real, reducible, or to be fixed.

Sources: `docs/SQLTEXT_ROW_VERDICT_CHARTER.md` (§0, §2–§7), `docs/BLOCK_COMPILER_HOMEWORK_2026_09_21.md` §10,
`docs/PLAN_EXECUTION_SPLIT_AND_DATABASE_OWNER_2026_10_03.md` (C6), current code `driver/SqlTextVerdicts.java` (git
history: docs/HISTORY.md).
