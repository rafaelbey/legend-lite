# Running the corpus

Two bodies of work live here and they run differently.

| | what it is | how it is checked | where |
| --- | --- | --- | --- |
| the **corpus** | ~4690 executable services with seeded data | run against legend-engine, every answer compared to an independently computed expectation | `core/src/test/resources/stress/` |
| the **project graph** | 56 Legend projects that depend on each other | compiled only — no data, no runtimes, no services | `projects/` |

Some of the 56 are also pulled into the corpus and executed; see
[the boundary](#the-boundary) below. The corpus is also legend-lite's own gate 10: see
[running it through legend-lite](#running-it-through-legend-lite).

> Cut in the 2026-11 docs cleanup: the Maven-era prerequisites (a JDK path, `mvn` builds of core and the engine
> runner, a machine-specific `cp.txt`), the two Python commands and the measured cost of an engine run (about 0.6 s
> per service plus about 9 s per JVM, so about an hour at 4,290 testables), 2026-09. Full text:
> `git show de2745351:docs/RUNNING_THE_CORPUS.md`.

## Building and running it (Bazel)

Everything runs through Bazel from the repository root.

| what | command |
| --- | --- |
| regenerate the generated stress files | `bazel run //core:update_stress_corpus` |
| the corpus through legend-lite on DuckDB, shared sessions (gate 10) | `bazel test //core:stress_suites` |
| the same suites on H2, a fresh seeded session per test (the engine's shape) | `bazel test //core:stress_suites_h2` |
| both (the `stress` lane) | `bazel test //gates:stress` |
| the corpus's own ratchets: density, executed, stacking, the engine-surface and function inventories (in `//gates:checks`) | `bazel test //scripts/corpus:density_gate //scripts/corpus:executed_gate //scripts/corpus:stacking_gate //scripts/corpus:scoreboard_gate //scripts/corpus:functions_gate` |
| the cross-engine differential over the corpus (seconds) | `bazel test //core:corpus_differential_test` |
| one service through legend-lite, a session policy, row dumps | `bazel run //core:stress_tool -- [--backend NAME] [--sessions fresh\|shared] [--only NAME] [--data F.pure,...] [--rows DIR] [--out DIR]` |
| files or services through legend-engine | `bazel run //tools/engine-runner:testable -- <file.pure>... [--testable=<fqn>]... [--dump=<dir>]` |

Eleven files under `stress/` are **generator outputs** and must never be hand-edited, because the next build
overwrites them. They are listed once, in `core/stress.bzl`'s `STRESS_GENERATED`: `59`, `60` and `64` from
`scripts/corpus/dense_build.py`, `92` to `98` from `scripts/corpus/build.py` (two of them `94`), among them
`94-fanout-services.pure` (every generated service and its expectation) and `93-testdata.pure` (the `###Data`
element, all seeded rows). `bazel test //:generated` fails when a committed copy differs from what the generators
write, naming the update command. Everything else under `stress/` is hand-written and safe to edit. If you want to
change what is generated, change the generator in `scripts/corpus/`, not its output.

`build.py` also runs every consistency check before it writes anything — a class whose
mapping the runtime cannot resolve, a duplicate set id, a seeded value too wide for its
column, a service that sorts on an unprojected alias. It prints them all and writes nothing
if any fires.

Gate 10's floors are `MIN_PASS` ratchets inside `StressServiceSuitesTest` (DuckDB) and
`StressServiceSuitesH2Test` (H2): raised, never lowered.

**The engine side.** `scripts/corpus/run.py` runs the whole corpus through legend-engine and adjudicates every
service against the quarantine (below). It, `scripts/projects/check.py` and `scripts/projects/loadtime.py` still
read the Maven-era classpath file `tools/engine-runner/cp.txt`, which the Bazel build does not write; until they are
ported, the engine runs through `//tools/engine-runner:testable` (`tools/engine-runner/README.md`), with the linked
project files passed first (below).

## The two generated layers

The services fall into two layers, and the split is deliberate.

**Bare projections**, one per subtype set, ~1830 of them. A wrong `~filter` fails by returning
the WHOLE table rather than by erroring, and a bare projection over the subtype is the only
thing that makes that visible. These are the control.

**Stacked queries** from `scripts/corpus/deepstack.py`, ~2440 of them: four-hop chains
filtered on a NAVIGATED column, `groupBy` on a joined key, `graphFetch` three and four levels
deep, subtype roots navigating inherited edges, qualified properties taking arguments.

A stacked query is only interpretable if a simple one over the same rows passes, which is why
the bare ones were kept rather than upgraded. The same split already pays around F6
(`aggregates.py` against `tomany.py`) and F28 (`CB_NotEqualsNull`).

One pair is a true INVARIANCE. `DSDeep_X` filters on a model path before `project`, where the
predicate can be pushed into the WHERE beside the join; `DSPost_X` applies the identical
predicate to the projected ALIAS afterwards. All 400 pairs carry identical expectations,
each computed independently from the seed -- so if the engine ever places one of them wrongly,
exactly one side of the pair fails and names itself.

## Reading a failure

`run.py` classifies every service, and the classes mean different things:

| verdict | meaning |
| --- | --- |
| `PASS` | the engine's answer matched the oracle's |
| `KNOWN-FAIL <Fnn>` | quarantined against a recorded finding in `docs/UPSTREAM_FINDINGS.md` |
| `REGRESSION` | it failed and is not quarantined — **this is the one to care about** |
| `ERROR` | it threw rather than returning a wrong answer |
| `FIXED <Fnn>` | a quarantined service now passes; remove it from `quarantine.py` |
| `MISSING` | it reported nothing at all |

A wall of `MISSING` almost always means **one** service killed the JVM its batch ran in.
`run.py` prints `FATAL AT INIT <name>` above them when it can detect this. A service that
throws during test-suite *initialisation* cannot be quarantined — quarantine excuses a
failure, and this one never reports one — so it has to be removed from the corpus and its
defect pinned with a probe under `repro/` instead.

## Running one service

The full suite is too slow for a debugging loop. Through legend-lite, `bazel run //core:stress_tool -- --only NAME`.
Through legend-engine, name the services to run, with the linked project files ahead of the stress files:

```
bazel run //tools/engine-runner:testable -- <linked project .pure files> core/src/test/resources/stress/*.pure \
  --testable=stress::PL2_PillarTenorBand
```

(Paths are relative to where you type the command; `scripts/corpus/model.py`'s `linked_files()` lists the linked
projects' files in order.)

Two things that are easy to get wrong and fail anonymously:

- **the linked project files must come first.** A `.pure` file with no `###Section` header
  inherits whatever section the previous file left open, so order is load-bearing. Omit them
  entirely and the corpus fails with a bare `Unexpected token` naming no file.
- **`perf.TestableMain` runs only what is NAMED.** Passing the files with no `--testable=`
  compiles everything and runs nothing, reporting `0 total` rather than an error.

`--dump=<dir>` writes the expected and actual JSON per suite, which is the only way to see a
diff the console truncates.

## The project graph

```
bazel test //projects:tests      # each project alone with its declared closure, the whole graph, the contract's file checks
```

The scripts behind the questions below (`scripts/projects/check.py`, `loadtime.py`) still read the Maven-era
`tools/engine-runner/cp.txt` (see above).

`check.py` asks two different questions. **Alone**: does a project compile with exactly its
declared dependency closure — a project needing an undeclared one fails here, which is the
defect the graph exists to find. **Together**: does the whole graph compile at once, which is
where set-id and filter-name collisions appear, both being global namespaces in Legend.

It also checks that a project is *substantively* what it was meant to be — the size band from
`scripts/projects/spec.py` and the presence of a `MANIFEST.md`. A three-file placeholder
compiles perfectly, and without that check the graph looks complete when it is not.

`projects/CONTRACT.md` is binding on anything added to `projects/`. Its "learned the hard
way" list is eleven items long and every one of them cost a project a failed compile.

## The boundary

`projects/` is compile-only by design, which proves a cross-project reference *compiles* and
nothing more — and several recorded findings compile perfectly and fail at execution. So a
slice of the graph is pulled into the executable corpus:

```python
# core/stress.bzl (scripts/corpus/model.py reads it through stress-layout.json)
LINKED_PROJECTS = ["core-types", "core-tenor", "core-fx", "core-ratings", "core-instrument",
                   "core-calendar", "core-units", "core-account", "core-geo", "fee-core", "index-core"]
```

**Order is load-bearing: dependencies before dependents.** `fee-core` needs `core-types` and
`core-tenor` parsed before it, and `core-types` is in the list only because fee-core needs it — it
exports no store and no mapping at all.

Adding a project to that list is not enough on its own. It also needs:

1. **rows** in `scripts/corpus/seed.py` for each of its tables. Leave one unseeded and the
   corpus's expansion ring will invent placeholder rows for it, which surface as
   width-violation errors rather than as "nobody seeded this".
2. `include <project>::Store` in `store::DB` — **first in the body**, which is a grammar
   requirement, not tidiness.
3. `include <project>::Mapping` in `projectlink::ProjectLinkMapping` — likewise first.
4. an association in `8111-projectlink.pure` if the corpus is to navigate into it, with the
   set ids named on **both** ends. The project side names its ids explicitly; the corpus side
   mostly does not, so the two halves of one association follow opposite conventions and
   neither is guessable from the class name.

Two things that a project brings and the corpus does not have, which cost a build each:

- **a class declared over several lines**, with stereotypes and tagged values above its name,
  and **a qualified property whose body wraps**. The reader is line-based and folds both, but
  only because it RAISED rather than skipping them — an unparsed class has no properties, no
  mapping and no service, and nothing would have said so.
- **arithmetic follows the COLUMN type, not the property type.** A `DECIMAL` column adds and
  subtracts exactly -- `108.7500 - 107.9000` is `0.85` in the database and
  `0.8499999999999943` in binary -- while a `DOUBLE` column computes in binary and a 4-byte
  `FLOAT` computes narrowed. The oracle carries a tag per width (`flat.F32`, `flat.F64`);
  a value that loses its tag silently gets the wrong arithmetic.
- **a graphFetch ENFORCES multiplicity where a projection does not.** A `[1]` property whose
  column holds a NULL returns the null happily in a TDS projection and fails a tree with
  "Property of multiplicity [1] can not be null". The seed nulls every nullable column on
  purpose, so a generator building trees has to CHOOSE a null-free leaf rather than take the
  first one and reject the tree.
- **a wrapped `Join` condition.** A two-column join written over two lines is valid grammar
  and the reader now folds it on parenthesis balance. A comment in fee-core says a wrapped
  condition "is not parsed" -- that was true of the READER, not of Legend, and core-account
  wraps all four of its two-column joins.
- **a decimal column.** Seed it with a FLOAT, not a quoted string. The emitter writes
  `repr()` and the oracle's exact-decimal path goes through `Decimal(str(v))`, so every digit
  survives either way -- but a string is COMPARED as a string, and
  `$this.factorToBase == 1.0` is false for `"1.00000000"`. The engine returns decimals padded
  to the column's scale; that is not a mismatch, because `EqualToJson` compares numbers
  numerically and only the value has to agree.
- **a value containing a comma.** Allowed: the data element is CSV and RFC4180 quoting works,
  including `""` for an embedded quote. A NEWLINE is still refused, because the element is a
  concatenation of one Pure string literal per row and quoting cannot save that.
- **an enum over a string column.** A generator that picks a filter column by the column's
  type will pick it and emit `> ' '`, which fails to compile and takes the whole file with
  it. Choose by the DECLARED property type.
- **a mapping set that `extends` another.** It inherits the parent's main table, its column
  mappings AND their transformers — copying the columns without the enum transformers read
  `'ZC'` off the row where the engine returns `ZERO`, and only a subtype whose enum property
  is declared on an ANCESTOR set shows it. It does NOT inherit the parent's `~filter`; that
  is F56, and it is the engine's behaviour rather than the reader's choice.

A project whose value is its SUBTYPES rather than its navigation will generate almost
nothing on its own: `stacks.py` wants two distinct navigation targets and `graphs.py` wants
to-one branches, and a wide flat master table has neither. Register it in
`taxonomy.py`'s `TAXONOMIES` and `IDENT` instead — one entry per LEVEL, since each level is
told apart by a different column — and one service per subtype falls out.

## What the reader refuses, and what it records

The reader RAISES on a form it cannot model, and records what it can see but not resolve in
`c.unparsed`. Generators skip the latter, so a gap costs COVERAGE and never correctness -- a
service using an unresolved property fails with a reason rather than returning a wrong answer.

Five entries remain, all in single-construct mapping fixtures. That number was eleven until a
one-character asymmetry was found: a nested embedded block closes with a bare `)` and its
parent with `),`, and the generic closing-punctuation skip swallowed the bare one. So
`ownership ( ... taxResidence ( ... ) )` popped once instead of twice and stayed open, and
every property declared AFTER it -- a sibling embedded block and six outbound joins -- was
attributed to the inner class and resolved against nothing. One level of nesting worked; a
sibling after it did not, and `core_account::Account` read as having no navigation at all.

## The rule the whole thing rests on

**Every expectation is computed from the seed by `scripts/corpus/oracle.py`, never read back
from the engine.** An expectation copied from the engine's output asserts that the engine
agrees with itself, which is always true and worth nothing.

This is why the oracle refuses an unknown function rather than guessing at it, why it raises
on an aggregate it does not implement rather than returning null, and why a cross-project
function had to be reimplemented here from the project's own source. If you add a construct
the oracle cannot evaluate, teach the oracle — do not relax the assertion.

## Running it through legend-lite

On 2026-09-16 the corpus moved onto main from its own branch, where it was 4,742 services (4,729 with test suites)
over a 20-domain financial model, seeded by one `###Data` element, every expectation computed by the oracle above,
and run against legend-engine 4.138 with 0 unexpected and 28 engine defects quarantined (`docs/UPSTREAM_FINDINGS.md`).
legend-lite runs the same suites through its own product runner (today's `ServiceTestRunner`), following the
engine's testable rules from its sources:

- **Provisioning.** A suite's `data: [ connections: [ id: ... ] ]` names the runtime's store for that id; a
  `Reference` names the `###Data` body; `Relational` CSV becomes DDL and inserts typed from the parsed store. Other
  data kinds are skipped loudly.
- **Sessions: the engine's way first, the optimization second.** A suite runs on a test runtime whose connection
  carries the suite's data, loaded when the connection is established, as the engine does; the runner itself runs no
  SQL. `FRESH_PER_TEST` (a fresh session per test, seeded on establishment, the engine's fresh database per run) is
  the baseline; `SHARED` (one seeded session per distinct provisioning for read-only tests, a private one for a body
  with effects) is the measured optimization. Gate 10 runs DuckDB shared and H2 fresh.
- **Execution.** Parameters bind as `let` variables ahead of the body, then the one production entry runs against
  the service's runtime.
- **Judgment.** `EqualToJson` by the engine's own JSON rules (its `JsonNodeComparator`
  `NULL_MISSING_EQUIVALENT_AND_UNORDERED_ARRAYS`, with exact decimals): null equals missing, arrays are unordered at
  every level, `1 == 1.0`. Nothing "smarter than the engine" is hidden: DEFAULT tabular serialization,
  `EqualTo`/`Relation` assertions, multi-execution services and ExternalFormat or ModelStore data are skipped
  loudly, never faked, and every failure is listed by name. Where the engine is wrong and the oracle right (F6, F50,
  F51, F54), lite is expected to pass.

Measured on the first full run (2026-09-16): model parse and build 6.3 s, every test's execution 8.5 s, the whole
test 20.7 s, against about an hour through legend-engine. The cost that matters is the model build, paid once.

Sources: `docs/STRESS_CORPUS_THROUGH_LITE_2026_09_16.md` (git history: docs/HISTORY.md).
