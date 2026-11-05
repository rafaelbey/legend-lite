# The result boundary program: a typed plan, writers at the edge, the text path deleted

**Status: RULED (revision 3, 2026-10-26) and LANDED: every track built and merged to `main` as PR #80 (`7bc0797d`,
2026-10-30).** The user ruled on
2026-10-26: the design (D1 to D5), the formatting-at-the-edge clause with its guards, lean JSON's numbers,
collecting into memory as a test helper only, and the order are accepted as written in revision 2 ("everything else
is okay"); **the database judge is never deleted** ("a crown jewel"); the Float rule and the Arrow question were
sent back for the expert version, which revision 3 gives (§8, items 3 and 4), and both were then accepted the same
day ("arrow look good"; "sounds great"). Revision 1 was written the same day at the
user's ask ("this is now becoming a huge blocker to our streaming/deephaven/real-time ambitions; how can we move this
forward sanely AND delete all our historical cruft"); revision 2 folded in the user's three questions that followed.
It is the execution plan for `docs/RESULTS_AND_RENDERERS_2026_10_22.md` (D1 to D6) and for the plan path program's
steps 7 to 11 (`docs/PLAN_AND_RESULT_MODEL_2026_10_18.md` §11), made one program. Kept here: the plan in plain words
(§0), the rules with the tests that keep them (§1), the shape (§2), the gates (§6) and the user's rulings (§8). The
session handoff that tracked the landing (`RESULT_BOUNDARY_START_HERE.md`) is history (`docs/HISTORY.md`); the
decisions made on the way are `docs/result-boundary-homework/DECISIONS_LOG.md`.

## 0. The plan in plain words

**The goal.** A query produces a table of typed values. The database computes every value. Our SQL never formats
anything for output. Whoever consumes the result formats it at the edge, their way: Studio and the other legend
clients get legend-engine's exact JSON, written in Java; the PCT and corpus harnesses get Pure values, no text; lite's
own clients get lossless typed data (Arrow, JSON, CSV); the browser formats its own grid. Every old path that did it
differently is deleted, not kept beside the new one. The product keeps zero third-party dependencies in the compiler,
the WebAssembly planner and the native image: no Arrow library enters the product (§2).

**What we have today.**

- The compiler makes a plan: the SQL plus, for each result column, its Pure type and multiplicity, the SQL type the
  database answers with, and the store column's declared type. This is right and stays.
- Two executors. The old one runs SQL and builds an in-memory result with its own decoding. The plan runner runs the
  plan and reads each cell by the plan's column types. Two decoders; one must go.
- On most routes the SQL itself is made to write the final text: Pure's TDS text for PCT, legend-engine's JSON rows
  for the served API, CSV. That is what turns one addition into a 48 KB statement on Snowflake for PCT and 6 MB for
  the served API, makes every new database re-derive Pure's spellings before one test passes, and is the wrong shape
  for anything that streams or ticks. This goes.
- Values SQL has no type for. A mixed `max` now comes back as a tag plus one typed column per kind (landed
  2026-10-23). Mixed lists still come back as text and must be typed the same way.
- A Float turned into a String inside a query (`toString`, `format`): the database writes it, as the shortest decimal
  that reads back as the same double. On Snowflake and BigQuery that costs a 5 KB expression because their own
  formatters are lossy (measured 2026-10-21 and 2026-10-24). This stays, because it is part of the query's meaning
  and nothing cheaper is lossless there; §8 item 3 says how to make it cheap where it can be.
- The warehouse server already writes Arrow IPC streams with no Arrow library: its own framer
  (`warehouse/.../duck/ArrowStreams.java`, `FlatBuilder`) over the buffers DuckDB builds, inside the native image,
  checked against pyarrow in its tests. The lean Arrow writer reuses that framing over our own batches; the batch
  itself stays our own representation, typed arrays behind one accessor (§2: why not Arrow's layout inside core).

**What we will do, in order.** Measure first. Make the plan answer typed rows and nothing else. Define the result
as a stream of batches of typed rows, our own typed arrays behind one accessor. Write legend-engine's JSON in Java and prove it byte
for byte, then delete the SQL that wrote it. Feed PCT and the corpus from typed rows, lane by lane, then delete the
SQL that wrote TDS text. Add the lossless writers, the Arrow one over the warehouse's framer. Type the mixed lists and
delete the text carrier. Delete the old executor and make "collect into memory" a test helper rather than a
production method. The database judge and the host judge both stay, reading typed rows.

**Is it correct?** Yes. The decision that matters, typed results with formatting at the edge, is what every engine
and every driver does, and the measurements of 2026-10-18 and 2026-10-22 show the alternative's cost on our own
lanes. The risks are known and each has a gate that finds it (§6).

**Is it tractable?** Yes. Seven to nine weeks of one session's work, the cloud lanes' turnaround included. Every
step lands on its own with its own gate and its own deletion; no step needs the ones after it; the Deephaven line
needs only steps 1 and 6. The risk is concentrated in step 4 (the lanes) and step 7 (test expectations that move
when the old decoder's quirks are corrected), and both are made visible by running old and new side by side before
switching.

## 1. The rules, each with the test that keeps it

AGENTS.md: guard files are the rules' mechanical form. A rule without a guard is a wish.

- **R1. The database computes every value; our code derives none.** Unchanged (the execution tenet; the charter's
  Clauses 1 to 6). Guard: `JavaEvalLedgerTest`, `ArchitectureTest.theInterpreterPerformsNoJdbc`.
- **R2. The plan describes types; it never spells values and never names an output format.** Every result column
  carries its three facts. A fact that only Pure knows (a Decimal's scale, a DateTime's written precision, a tagged
  winner's kinds) travels in the plan or in the data, never in a text. Guard: `PlanJsonTest` (format pin),
  `PlanMakerTest.aColumnCarriesItsThreeTypeFacts`, `testcases/PlanRoundTrip`; new: the planner makes no `TextResult`.
- **R3. Formatting at the edge is serialization, not computation** (ruled 2026-10-26). A writer spells the typed
  value it received, losslessly, in its consumer's layout. It does no arithmetic, no comparison, no conversion
  between kinds, no default for a null, no zone shift, and chooses no spelling by a Pure rule. The test: swap the
  writer and every value is unchanged. Guard, new: `ArchitectureTest.writersAreCarriageOnly` (the writer package
  depends on the plan's column facts and the typed batch, on no `compiler`, `lowering`, `sql` or `exec` class) and
  `WriterCarriageGuardTest` (a source scan for arithmetic and comparison on numbers and dates in that package, zero
  allowed; representation operations such as setting a scale without rounding are allowed and listed). A scan
  catches drift, not an adversary; that limit is the same the existing guards carry. The charter clause is D3's.
- **R4. One statement, one typed result, several writers.** The legacy writer makes legend-engine's bytes and is
  judged by bytes (`OutputMatrixTest`); the PCT and corpus harnesses take Pure values through `ValueBridge`; the lean
  writers are judged by the value round trip. Execution is never forked for a format (D6).
- **R5. A String the query itself builds is computed in the query, lossless, with one spelling.** The shortest
  round-trip decimal in Pure's layout: the database's own text where its cast writes shortest digits, the exact
  expression where it does not (Snowflake, BigQuery), a platform-owned SQL function only if measurement ever shows
  that expression on a hot path (§8 item 3). A Float printed as the last thing a result column does is not a String
  the query builds: it is carried as the Float and spelled by the writer, which is Pure's own implementation
  (§8 item 3, the edge rule). This is D7. Guard: the PCT suites; a dated register row for any deliberate difference.
- **R6. A value SQL has no type for rides as a typed encoding with a plan fact, never as a spelling.** The tagged
  winner is landed; mixed lists follow (step 6) as a list of tagged values, which is Arrow's list of a union, so the
  plan's vocabulary, the tagged carrier and the lean wire are one type system. Guard: `TaggedSelectionTest` and its
  successor for lists; a shrink-only census of `LITERAL` slots in the test corpus's plans, zero at step 6.
- **R7. Measure, run both, switch, delete, in that order, per lane.** Nothing is deleted until its replacement has
  produced the same bytes (legacy) or the same values (lean, PCT) on every lane and the gate pins it. No parallel
  path survives its switch. Guard: a GATES entry per switch; a shrink-only census of database-written result text,
  zero at step 4.
- **R8. A result is a stream of batches of typed rows** (ruled 2026-10-26). A batch is up to N rows, flushed when
  full, when the source has no next row ready, or at the end; a batch of one row is a batch. Memory is bounded by one
  batch. Collecting a whole result into memory is a writer like any other, used by tests and the judges, and never a
  production method. Guard: `TenetRatchetTest` (JDBC accessor sites at 21); the exec package's closed register; new:
  no production method in `Execution` returns a materialized result.
- **R9. Zero third-party dependencies stay zero in the compiler, the WebAssembly planner and the native image**
  (the user, 2026-10-26: "I don't want to compromise on that"). The Arrow format is written by our own framer; the
  Arrow and pyarrow libraries are test-only readers. Guard, new: a Bazel query test in `tools/guards` that
  `deps(//core:plan_side)`, `deps(//wasm:...)` and the native image's deps contain no `arrow` repository; the
  existing C2a pin (`ArchitectureTest.rootJavaSqlSurfaceIsPinned`: the planner is `java.sql`-free).
- **R10. Both judges stay** (the user, 2026-10-26; plan path R2). The host judge compares typed values in Java; the
  database judge composes both sides into one verdict statement and compares inside the database, over the
  platform's own canonical spellings (`FLOAT_CANON`, the tagged canon per slot), which are query-side constructs
  like `toString` and are never deleted. The two must agree. Guard: `VerdictChannelRegisterTest`, channel B's floors.

## 2. The shape, once

```
Pure query ──▶ PLAN: the statement + one column record per result column (type, multiplicity, slot, store)
                 │        no output format, no spelling, no writer named in it
                 ▼
            EXECUTE: the database computes everything that has meaning
                 │
                 ▼
            TYPED BATCHES: the session reads cells by the plan's facts into typed arrays, up to N rows, one accessor
                 │        (a row driver fills small batches; the warehouse's native DuckDB path keeps its own Arrow buffers)
     ┌───────────┼──────────────────────────┬──────────────────────────┬────────────────────────┐
     ▼           ▼                          ▼                          ▼                        ▼
 lean writers   legacy writer            ValueBridge              a collector              the tab's own reader
 Arrow IPC      legend-engine pure/v1    (PCT, corpus):           (tests, the judges;      DataCube on DuckDB in the
 (our framer),  JSON, byte for byte      Pure values, no text     never a production API)  browser, and on Deephaven
 JSON, CSV
```

The right-hand consumer is why the plan is the only contract that matters for real time: on the Deephaven route no
Java of ours runs between the plan and the grid. What it needs from this program is R2 and R6, nothing else.

**Row databases and batches (the user's worry, 2026-10-26).** Batches are not a plan concern and not two execution
modes; the batch size is a session setting. A row-at-a-time driver (Postgres, MySQL, H2, SQLite) produces small
batches and flushes on idle, so the first row is not delayed and a tick arrives as a batch of one. Moving a row's
cells into columns is a copy per cell, cheaper than today's typed read plus formatting per cell; the driver's own
wire cost is paid either way, and most row drivers fetch in chunks underneath. A one-value result pays microseconds
of fixed cost against a round trip of milliseconds. Choosing rows as the unit instead would de-vectorize every
columnar source and re-vectorize it at the Arrow writer: slowest exactly where the databases are fastest.

**Where the Arrow format lives, and where no library does (the user's question, 2026-10-26).** The compiler
(`//core:planner`, exported by `//core:plan_side`, the WebAssembly planner's only core input) has no execution
dependency and no Arrow; that is C2a, pinned today, and R9 adds the Bazel query guard. The execution side
(`//core:exec`, the JDBC drivers in `//core:drivers`, the writers) gains no Arrow library either. **The batch is our
own representation: one typed Java array per column (`long[]`, `double[]`, `String[]`, `BigDecimal[]`, the date and
nested kinds likewise) with a null mask, behind one accessor interface** (column count, row count, is-null, a typed
read per cell) that every writer uses, so no writer knows the representation. The user asked (2026-10-26) whether
the warehouse server's own Arrow layout should be that representation instead, and the honest answer is no, for
one decisive reason: in core there is no Arrow-native source to pass through. The warehouse gets DuckDB's Arrow
buffers through the foreign-function API, in the native image, a path of its own; core reads DuckDB through JDBC,
whose Arrow export needs the Arrow library's allocator. So an Arrow layout inside core would be built cell by cell
from JDBC rows (bit-packed validity, offsets, 128-bit decimals, nested children, by hand) only to be read cell by
cell again by the row-oriented writers that are the bulk of the consumers (legend-engine's JSON, `ValueBridge`, the
collector), and its one benefit, no copy at the Arrow writer, would be paid for on every other route. Typed arrays
are the natural form for those writers, trivial to build from a driver row, and the Arrow writer converts them in
one pass, which costs about what any formatting pass costs. What is shared with the warehouse is its IPC framing
(`FlatBuilder` and the schema and record-batch messages of `ArrowStreams`), not its buffers: the core writer builds
Arrow buffers from the arrays and hands them to the same framer. The plan's three facts per column become Arrow field
metadata at that writer. If a measured gap ever shows on a route that could hand over Arrow buffers as they are
(DuckDB through the foreign-function API in core, a cloud driver's Arrow results), an Arrow-backed batch implements
the same accessor and no writer changes. The Arrow Java library and pyarrow stay what they are today: readers in
tests (`arrow_matches_json`), which read back what our writer wrote. The native image carries exactly what it
carries today.

> Cut in the 2026-11 docs cleanup: §3 where things stood, §4 step 0's measurements (M1 to M8) and §5 the steps 1 to 8,
> each with what it built, ran both ways, switched and deleted, step 8's question parked for the user (the
> engine-style SQL-text dialects) and the list of what stays by name (2026-10-26 to 2026-10-29; landed as PR #80).
> Full text: `git show de2745351:docs/RESULT_BOUNDARY_PROGRAM_2026_10_26.md`.

## 6. Gates for the program

1. **The value round-trip gate** (M2): the judge of every lean writer and of the typed seam on every lane.
2. **`OutputMatrixTest` byte for byte**: the judge of the legacy writer; a deliberate difference is a register row.
3. **PCT and the corpus, test by test, every lane, no lane below its previous count**; the floors move only at step
   4's re-baseline, in the GATES entry that names it. **Both judges agree** on every lane (R10).
4. **Two shrink-only censuses:** database-written result text (M1) and `LITERAL` slots (R6), both zero by step 6.
5. **`writersAreCarriageOnly` and `WriterCarriageGuardTest`** from step 3 on; **R9's dependency query guard** from
   step 1 on.
6. **The size and time table** (M5) re-measured at steps 4 and 7: Snowflake's standard suite at or below 269 s; no
   PCT statement over 5 KB from output text; Snowflake's served statement for `|1.5 + 2.25` at 44 characters.
7. **No production method returns a materialized result** (R8), from step 7 on.

> Cut in the 2026-11 docs cleanup: §7 the order, dependencies and size estimate of the steps (2026-10-26). Full
> text: `git show de2745351:docs/RESULT_BOUNDARY_PROGRAM_2026_10_26.md`.

## 8. The rulings

Ruled 2026-10-26 ("everything else is okay"): **1** D1 to D5 as written in RESULTS_AND_RENDERERS §2; **2** D3's
charter clause (R3) with its two guards, the writers registered carriage sites in `JavaEvalLedgerTest`; **5** lean
JSON writes exact JSON numbers with the column types beside the rows, Arrow for any reader that needs exactness in
JavaScript; **6** collecting into memory is a test helper and the judges' writer, never a production method, and the
old executor goes at step 7; **7** the database judge is never deleted (R10; the engine-style dialects stay parked);
**8** the order of §7, mixed lists in parallel after step 1 if Deephaven is the priority.

Sent back for the expert version, given here, waiting on the user's yes:

**3. A Float, the version that is best on every side and makes the lean experience good. Ruled 2026-10-26
("sounds great"), the last of the eight.** Three places, and
the user's reminder (2026-10-26) that the legacy writer and the lean writer may hold different ideas of a Float's
text where that is better. The plan, in one table:

| Where a Float meets text | LEGACY (legend-engine's bytes) | LEAN | PCT (Pure's values) |
|---|---|---|---|
| A. In a result | its 64 bits; the writer spells `Double.toString` | its 64 bits; Arrow float64; JSON and CSV the shortest digits | its bits, a Pure Float |
| B. Printed as the last thing a column does | the bits carried; the writer spells `Double.toString`, which is Pure's own implementation | the same bits, the same spelling | the bits, spelled by the bridge with the same function: the reference, byte for byte |
| C. A String built inside the query (feeds a filter, a join, a group, a longer String) | one spelling, in SQL: shortest digits in Pure's layout; a native cast on nine engines, the exact-digits expression on Snowflake and BigQuery (a register row where legend-engine printed the database's lossy text, M7) | **the same SQL** (the recommendation); the alternative, the database's own native text at zero cost, is lossless on nine engines but engine-dependent in layout and lossy on Snowflake, so it is offered, not recommended | the same SQL as LEGACY: Pure's text |

Places A and B are where real code meets a Float's text and both cost nothing on any engine. Place C is the rare
one, and the only place where LEGACY and LEAN could differ; the recommendation keeps one SQL because the
difference buys nothing measurable and costs a String that changes meaning by database.

**"At the edge" is a property of a plan's root, not of the Pure function** (the user's question, 2026-10-26: does an
`assert` pushed to the database turn place B into place C?). Yes, and by design. The edge rule applies when the
plan's root projection is the printed Float and nothing in the statement consumes the String. An assertion run by
the host judge reads the two operands as the plan's results and compares them in Java, so the printed Float is at
the root there, carried as its bits, and the host judge spells it with Pure's own implementation. The same
assertion composed by the database judge into one verdict statement consumes the String inside the SQL (`expected
= actual`), so there it is place C: the database spells it, with the exact-digits expression on the two lossy
engines, as channel B does today. R10 then requires the two judges to agree, which makes every such test a
cross-check of the Java spelling against the SQL spelling on every lane: if either were wrong, the judges would
disagree and name the test. That is the database judge doing exactly what it is kept for. The cost is test-only and
already paid (channel B's statements carry the canon today); a product query whose root prints a Float pays
nothing. PCT's channel A is place B throughout: `ValueBridge` builds the String with Pure's own function and
legend-pure runs the assertion over it.

The detail of each place:
- *In a result.* A Float is its 64 bits, never a text: the DOUBLE slot, the driver's exact read, Arrow's float64.
  Nothing is lost, nothing is spelled, no database is involved. Where a lean writer must spell it (JSON, CSV), it
  writes the shortest decimal that reads back as the same double: 0.1 is `0.1`, a third is `0.3333333333333333`,
  never `0.10000000000000001` and never a lost digit. That routine is pinned (M7): the JDK's `Double.toString` from
  JDK 19 on, or our own shortest-digits routine (about 300 lines, no dependency) if the pinned JDK is older. Decimals
  print at their own scale; integers exactly; NaN and the infinities as named strings in JSON and natively in Arrow.
- *Inside a query* (`toString`, `format`, a String built from a Float). One spelling everywhere, so the same program
  gives the same String on every database: the shortest round-trip digits in Pure's layout. Nine engines write it
  with one cast and a layout fix (DuckDB, Postgres, H2, ClickHouse, Trino, MariaDB, MySQL, Databricks, MongoDB);
  Deephaven is Java and writes it natively. Snowflake and BigQuery cannot (measured: sixteen digits and a
  mis-rounded `%.17g`), so they compute the exact digits in SQL today, about 5 KB per Float printed, correct.
- *Making those two cheap, practically, with no code running inside the database that is not SQL* (the user,
  2026-10-26: no JavaScript in the database; `toString` inside a query is rare in real code). Four options, in the
  order they should be used:
  1. **The edge rule: a Float printed as the last thing a column does is carried as the Float and spelled by the
     writer.** When `toString` (or `format` of one Float) is the final operation on a result column and nothing in
     the query consumes the String, the plan keeps the DOUBLE slot and records the column's Pure type as String; the
     writers spell it with the shortest-digits routine. This is not an approximation: Pure's reference
     implementation of a Float's `toString` is Java's `Double.toString`, so the writer in Java is byte-identical to
     the reference by construction, and `ValueBridge` builds the PCT value the same way. Covers the common real
     case (a report column) at zero SQL cost on every engine. The database judge canons such a column as the String
     the reference would print (the exact-digits expression inside the judge, test-only).
  2. **Inside the query, the exact-digits expression stays, inline.** A String that feeds a filter, a join, a group
     or a longer String is computed where it is used, correct, about 5 KB per Float on two engines, zero elsewhere.
     Pay only when used; M6 and M8 say how rare that is.
  3. **A platform-owned SQL function, installed once, as the escalation if 2 is ever on a hot path.** The same
     expression as the body of a SQL function (`CREATE FUNCTION`, SQL language only, deterministic) in the
     platform's own schema, the way the platform already ships views over its own system tables (`docs/WORLD_MAP.md`);
     each statement then spells a Float as one call and the engine inlines it. Needs a schema and the privilege to
     create a function; a deployment that refuses DDL falls back to 2. Built only on M6's number.
  4. **A compile-time diagnostic** when a Float becomes a String inside a query on an engine that cannot print it
     natively, naming the cost. Cheap, informative, changes nothing.
- *Rejected and why:* JavaScript or any non-SQL function in the database (the user's ruling); a fifteen- or
  sixteen-digit standard (lossy by design on the lossless engines, and Snowflake's own fifteen is double-rounded);
  Snowflake's sixteen digits as a per-database exception (one database would answer differently, and it is not
  lossless); fixed seventeen digits (shows binary noise on ordinary values, which is why every modern runtime moved
  to shortest).

**4. Arrow: where the dependency would sit (nowhere), and what the batch is (our own). Ruled 2026-10-26 ("arrow
look good").** The compiler never sees
it: the planner has no execution dependency (C2a, pinned), and the WebAssembly planner takes only `//core:plan_side`.
The execution side does not take it either. The batch is our own representation, typed arrays with a null mask
behind one accessor interface (§2). The user asked whether the warehouse server's own Arrow layout should be the
representation instead; it was a suggestion, and the honest answer is no: core has no Arrow-native source to pass
through (the warehouse reaches DuckDB's buffers through the foreign-function API in the native image; core reads
DuckDB through JDBC, whose Arrow export needs the library), so an Arrow layout in core would be built by hand from
driver rows and read back cell by cell by the row-oriented writers, paying on every route for a saving on one.
Typed arrays are the natural form for those writers and the Arrow writer converts them in one pass. Shared with the
warehouse: its IPC framing (`FlatBuilder`, the schema and record-batch messages), not its buffers. The Arrow Java
library and pyarrow remain test-only readers. R9 pins it with a Bazel query guard over the planner, the WebAssembly
planner and the native image. Reversible: an Arrow-backed batch behind the same accessor, if a measured gap ever
shows on a route that can hand over Arrow buffers as they are; never in the compiler or the native image, never the
default.

**Ruled later (2026-10-27 and 2026-10-28; the order of work they set is the start page's §4, `RESULT_BOUNDARY_START_HERE.md`, now history:
`docs/HISTORY.md`):**

**9. The execution order and the audit (2026-10-27).** "Skip the guards - let's actually do the real work of making
the plan real and deleting everything else including the string spellings and old execution - keep going in phases,
do expert work, audit after each phase": phases 1 to 5. Then "we need to pause and do an audit": the audit
(`docs/result-boundary-homework/README.md` §1) and its three-stage plan, answered "keep it, and keep going": the
paused phase 5 commit kept and made green. The plan's other three questions, answered 2026-10-28: "any output
spelling changes need to come to me on case by case basis" (no spelling change of any output is accepted or
registered by a session: each is put to the user, alone, with its before and after, and the user's answer recorded);
"yes keep arrow writer at the boundary in this program (but no arrow dependencies in compiler!)" (the Arrow writer is
track A, at the boundary only; R9's guard keeps every Arrow artifact out of the compiler, the WebAssembly planner and
the native image); the writer guards: the user asked what they are for ("writer guards are for what exactly"; the
start page's S1.4 keeps the answer) and then approved them, "yes add this to the plan": R3's two guards
(`ArchitectureTest.writersAreCarriageOnly` and `WriterCarriageGuardTest`) are built in S1.4, right after the typed
precision fact, which supersedes 2026-10-27's "skip the guards" for them. The first spelling change put to the user
case by case, a negative zero ("what should we do about the negative zero? what is the right move", then "okay"):
lite's own outputs (LEAN JSON and CSV) write it `-0.0`, item 3's shortest decimal that reads back as the same double;
Pure's `toString` of a Float stays `0.0` for either sign, legend-pure's own (register S117; S114 until S1.0). Then "keep the fix, fix
the plan wording including adding that we need to fix 0.00010": `b944a42e` kept, and the trailing zero lite's own
outputs write after a small double's one digit (`1.0E-4` as `0.00010`, where legend-pure and item 3's shortest digits
give `0.0001`) ruled a defect, fixed in S1.4.

**10. A PCT relation crosses as legend-engine's own adapter reads it (2026-10-28, "Do the probe", then "Do it").**
The relation is streamed through the server's own LEGACY writer (`PureV1Api.engineWriter`) and read by a Pure copy of
the engine adapter's `getTDSResultFromProtocol` steps, then `stringToTDS`; the TDS text written in Java
(`PureTdsText`) is deleted. Chosen over the typed `TestTDSInterpreted.addColumn` route the audit had proposed: that
is not the reference's own crossing, its reader holds a date at the millisecond, and its classes sit in jars the
upstream pool does not list. So the PCT judges the JSON the server serves (three production defects found by it).

**12. Upstream fidelity (2026-10-29).** "wherever possible we should stick to upstream legend-pure and legend-engine
as possible - every answer should be 'can we do exactly as pure/engine does, or as close to as possible where too
expensive to be exact but with an explicit posting to me and a ledger for diverging' every decision should be settled
with that tenet." Recorded as AGENTS.md's upstream-fidelity tenet, the start page's first rule (§5) and the register's
header: upstream's answer, read from the pinned sources or asked of the engine runner, is the default; a divergence is
taken only where exactness is too expensive, posted to the user before it lands, and ledgered with the ruling. It
settles S1.4's two open questions (the start page's §4) and governs S1.5's classification of the 51 writer differences
and every track after.

**11. Our own core tests are the LEAN twin of PCT and the corpus (2026-10-28).** "i would prefer not to delete the
overlapping tests but use the exact same logic but with platform output that we think users would actually run/use
instead of backwards compat mode - so its almost the platform twin of PCT/core-relational". Replaces step 7's "test
helper with assertions unchanged": no test is deleted for overlapping PCT or the corpus; each value test keeps its
query and logic and is judged on lite's own LEAN output (lite's JSON rows first, Arrow when built), on every lane like
PCT. PCT and the corpus remain the LEGACY judges. Track T of the start page.

> Cut in the 2026-11 docs cleanup: §9 the dated status log, revision 1 to the seam-and-wall steps (2026-10-26 to
> 2026-10-30). Full text: `git show de2745351:docs/RESULT_BOUNDARY_PROGRAM_2026_10_26.md`.

