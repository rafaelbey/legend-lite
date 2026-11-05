# The World Map — what is Java, what is Pure, what is input

> Ratified with the user 2026-09-03 after the tier-1 recursion work and the
> option-2 homework (`docs/OPTION2_HOMEWORK_2026_09_03.md`, history: `docs/HISTORY.md`). This is the
> conceptual map every design decision about "Java vs Pure" is made on. The
> adjudication clause is `docs/TENET_CHARTER.md` Clause 6; `AGENTS.md` points
> here. Change this document only as a reviewed design decision.

## 1. What legend-lite is

A compiler from Pure to SQL, plus an orchestrator that drives a database. The
platform's own semantics are Java. The platform has **no runtime dependency on
legend-pure or legend-engine**: their `.pure` files are specification and test
input — read, verified against, never loaded, never executed.

Two tenets already govern this and are unchanged:

* **Java orchestrates, the database executes.** The query compiler executes no
  values (`docs/TENET_CHARTER.md` C1–C5).
* **Reference checkouts are spec, never runtime** (`AGENTS.md`).

What the corpus burn-down (2026-09) exposed is that "reimplement whatever we
need from legend-pure/engine in Java" was the right rule for two kinds of Pure
code and the wrong rule for a third. This document names the three kinds and
the rules that follow.

## 2. The three kinds of Pure code

| kind | examples | where its MEANING lives | how the platform treats it |
|---|---|---|---|
| **1. Natives** | `toLower`, `filter`, `newMap`, `groupBy`, `at` | Java, as ONE SQL lowering rule per native (what `toLower` means is SQL `lower`) | never a Pure body, never a Java evaluation |
| **2. Platform semantics** | routing, mapping resolution, milestoning, aggregation-aware rewrites, post-processors, plan generation, SQL generation itself (`pureToSqlQuery.pure`, `sqlQueryToString`) | Java, designed from the engine's Pure as SPEC | never run; the engine's implementation is read, not ported line by line |
| **3. Programs** | a user's M2M transform, a test helper, `wrapH2Boolean`, `toPostgresModel`, `getNames()->at(0)` | the program's own text | INPUT to the compiler, regardless of who wrote it |

Kind 3 is the new one. Engine developers authored some of these programs, but
they are indistinguishable from user code: they use natives and platform
semantics to build values. **A program is never ported to Java.** `MetamodelWalk`
(905 lines) was `toPostgresModel` ported to Java — the anti-pattern the old
rule failed to name — and is deleted by the toPostgresModel leg.

### The deletion test (kind 2 vs kind 3)

*If this function vanished, would any ordinary user query stop compiling to
SQL?*

* Yes → platform semantics (kind 2). Own it in Java, from the spec.
* No → a program (kind 3). Compile it.

`pureToSqlQuery` vanishing means no SQL: kind 2. `toPostgresModel` vanishing
breaks only its own tests: kind 3. Facts the compiler itself produces about a
model or a plan (primary-key inference, lineage scans, activities, plan nodes)
are kind 2 — Java-stamped, exposed as ROWS in the system store
(`docs/METAMODEL_AS_RELATIONS_HOMEWORK_2026_09_02.md`, history: `docs/HISTORY.md`); programs over those rows
are kind 3.

## 3. The prelude — the one thing the platform ships that looks like Pure

The platform ships **declarations only**: class and enum shapes (with their
`<<equality.Key>>` and defaults) and native function signatures — the
language's type surface. They are copied from the spec with receipts and pinned
by tests (`NativeFunctionTest`, the native catalog, signature verification
against the real `.pure`). They contain no bodies. This is not "Pure code in
the platform"; it is the surface the compiler accepts.

**One narrow exception: Pure QUERIES over the platform's own system tables.**
The metamodel navigation functions and the enumeration-mapping lookups in
`SystemMetamodel` are views over seeded rows, compiled by the same compiler as
user queries. The test: no control flow on a computed value, no recursion, no
effects — if a proposed body needs an `if` on a computed value, it is a program,
not a view, and does not belong in the platform.

## 4. The compiler compares; the database computes

The unroll (`UserCallInliner` + `LiteralUnroll`) lets programs over spelled
literals compile: a recursive function applied to a literal tree unrolls while
its argument strictly descends; `match`/`if`/`map`/`filter` act on the literal
before their bodies are rewritten. What keeps this from being an interpreter is
one rule:

**STRUCTURAL (the compiler may decide — the answer is visible in the text):**
which `match` arm a literal's class picks; a spelled field; an unspelled field
is the class's default/empty; `instanceOf`/`cast` on a literal's class; list
shape over spelled lists (`at`, `first`, `last`, `tail`, `init`, `slice`,
`size`, `isEmpty`, `reverse`, `concatenate`, `zip`, `fold`, `map`, `filter`
applied per element); identity of two spelled scalars of the SAME kind
(`'a' == 'a'`, `x in ['case','if']`, enum == enum, spelled-integer compare such
as `size() == 1`); a spelled enum's name; `newMap`/`get`/`pair`/`groupBy`/
`keyValues` over spelled pairs; `enumValues`; `assert(true, …)` is a no-op;
short-circuit `and`/`or` on a spelled boolean.

**COMPUTED (the database's — a SQL scalar in the value):** anything that
produces a NEW value: `toLower`, `toUpper`, `trim`, `startsWith`, `endsWith`,
`contains`, `replace`, string `+`, arithmetic, `joinStrings`, `format`,
`toString` on numbers and dates, cross-kind equality (`1 == 1.0`).

A decision that hangs on a computed value stays undecided: both outcomes are
carried and the database picks. The residual forms, cheapest first:

| residual | example | SQL |
|---|---|---|
| scalar | `if(x->startsWith('"'), \|…, \|x)` | `CASE` of VARCHAR |
| list | `if(schema == 'default', \|[], \|[schema])->concatenate(name)` | `CASE` of VARCHAR[] |
| shape | `if(cond, \|^DynaFunction(castBoolean…), \|$d)` — same class both sides | `CASE` of two structs (same layout ⇒ same type; polymorphic fields are JSON) |
| conditional membership | `[e1,e2]->filter(x \| computed(x))->isNotEmpty()` | each element kept under its own condition; list SQL over the guarded list |

Recursion terminates regardless of which branch runs: the descent measure is the
literal argument's size.

**Guard:** the fold set of `LiteralUnroll` is pinned to compare-only operations
(`LiteralUnrollLedgerTest`); a Java fold that produces a new scalar value is a
tenet violation, not an optimization.

## 5. Rules of the road

1. **Engine and pure checkouts are spec.** Never loaded, never executed.
   Signatures and class shapes verified against them with receipts.
2. **The platform ships a prelude of declarations only.** Shapes with keys and
   defaults, native signatures. No bodies. Pinned so they cannot drift.
   *Amended 2026-09-04 (option S, `docs/DECLARATIONS_HOMEWORK_2026_09_04.md`, history: `docs/HISTORY.md`;
   amended again 2026-09-08, §8):
   behavior is curated, shapes are data.* Native signatures are REGISTERED
   (we own their meaning). Library SHAPES are GENERATED from the spec files
   by demand (`core/src/main/java/com/legend/builtin/Prelude.java`,
   `PreludeGeneratorTest`) — the corpus's type positions, the platform's own
   Java names and the admitted program libraries (`Corpus.LIBRARY_FILES`)
   decide what is pulled, with the closure of what those declarations name;
   constraints are not loaded. By hand stay only: the m3 bootstrap (m3.pure
   is a graph file — `tools/m3shape.py` is the receipt), the primitives, the
   platform carriers, and the system-store-coupled shapes named in
   `Pure.java`. Trigger for the next step (verbatim `.pure` files loaded per
   build instead of a generated Java file): a shipped legend-lite without
   the checkouts, or a second module needing its own prelude selection.
3. **Platform Pure is limited to views over the system tables.** No control
   flow on computed values, no recursion, compiled by the same compiler.
4. **Every native means one SQL lowering rule.** A native that Java evaluates
   instead of lowers is a bug — inside the unroll too.
5. **A program that walls is compiler work.** A missing structural fold, a
   missing residual form, a missing lowering, a loading rule that did not admit
   the file. Never Java that knows the program's name (the frozen
   string-dispatch count, `ObservabilityGuardrailTest`, guards the last part).
6. **Residue is named, one line each, and never a port.** Recursion over
   row-backed trees (tier 2: a recursive CTE, when a second witness pays for
   it), effects, IO, reflection over the live graph.

## 6. The decision procedure when a corpus test walls

1. Is the missing thing a **native**? → one Java lowering rule to SQL.
2. Is it **platform semantics** (the deletion test says yes)? → design it in
   Java from the spec; never run or port the engine's Pure.
3. Is it a **program**? → it is input. Admit its file to the model (the corpus
   loader's job), and fix the compiler: fold, residual, lowering.
4. Is it a **declaration** (class, enum, native signature)? → the prelude, with
   its keys and defaults, with a receipt against the real `.pure`.
5. Is it **residue** (rule 6)? → name it; do not port it.

## 7. Applied to the work in flight (2026-09-03)

* debugPrint (9) and toPostgresModel (21) are programs. The natives that shadow
  `newState`/`convertElement`/`convertSelectSqlQuery`, the native
  `ModelConversionState` class, `MetamodelWalk`'s conversion arms,
  `MetamodelSteps` and `StatementExecutor.constructNode/constructOp` are
  deleted by the toPostgresModel leg.
* The ≈ 60 SQL-node and relational-metamodel classes and enums the program
  references go into the prelude WITH their equality keys (the engine files
  carry 143 + 22 keys; ours carried 4).
* The nine Java string folds added on 2026-09-03 violate rule 4; they are
  replaced by the conditional-membership and shape residuals and deleted.
* `testConvertJoinTreeNode` and `testConvertSelectSQLQuery` recurse over the
  mapping's join-tree ROWS: rule-6 residue (tier 2) until a second witness.

## 8. Amendment 2026-09-08 — the system prelude, two lists, diagnostics

Decided with the user after batch 147 (the placement rule and the prelude as a
module, which this section now holds whole: §8a and §8b). Two rules join §5, and
rule 2 is amended:

7. **A native signature without a lowering rule may not exist.** A spec
   `native function` is `Pure.java` + one SQL lowering, or a NAMED row in the
   permanent lowering list (reflection over the live graph, run-time function
   values, effects, IO) that `NativeCatalogGovernanceTest` pins shrink-only.
   Registering a signature "so engine code can type" is the violation batch 147
   made visible (about thirty phantom natives); each becomes a generated body,
   a real lowering, or is deleted.
8. **An engine file is admitted as a program library only if every function in
   it passes the deletion test as a program.** A file mixing programs with the
   engine's internals is not admitted whole; tests whose SUBJECT is the
   internals are walls. No laziness, no partial evaluation of a record, no
   phantom vocabulary to slip past a wall.

Rule 2 amended: **the prelude is system code that looks and acts like user
code.** Shapes AND their derived-property bodies AND the spec's Pure-bodied
functions are GENERATED from the spec with receipts, compiled by the same
compiler as a user query (`SystemMetamodel` is already this for the platform's
own views). The spec draws the native/program line: `native function` → rule 7;
`function` with a body / a derived property → a program. The Java `toString`
arms for `Pair` and `List` in `lowering/Scalars.java` are ports of such bodies
and are deleted when the bodies land.

Compile is three stages (load, type, lower-and-run); a failure belongs to
exactly one. The typing work list trends to zero (missing vocabulary, typer
gaps); the lowering list is the permanent boundary. Diagnostics report the
lowering list at compile time as rows on the compile artifact (WARNING = typed
but reaches a native with no SQL meaning), non-blocking.

### 8a. Where things go: one question places everything

*If this vanished, would an ordinary user query stop compiling to SQL on our
platform?* Yes → platform semantics, designed from the spec. No → a program,
compiled like user code. The spec already draws the native/program line:
`native function` means "the runtime implements this" (for lite, a platform
signature plus one SQL lowering, or a named wall); a `function` with a body, and
a class's derived property, mean "written in Pure" (for lite, a program). The
platform and the programs it runs touch only through data: the platform hands
programs facts (rows in the system tables, query results), programs hand the
platform code to compile; the platform never exposes a function it does not
implement, and a program never calls the platform's internals.

| what it is | where it lives |
|---|---|
| a built-in the spec marks `native function` (`toLower`, `filter`, `at`) | a platform signature plus exactly one SQL lowering rule; a native with no SQL meaning is a named wall (rule 7) |
| a class or enum shape, a derived property, a spec `function` with a body (`Pair.toString`, `paginated`, `union`) | the generated prelude (§8b), compiled like user code; never a host-language arm |
| the few shapes the platform's code is coupled to (the m3 bootstrap, primitives, carriers, system-store classes) | declared by hand, each with a receipt naming the code that needs it |
| how a query becomes SQL: routing, mapping resolution, plan generation, SQL rendering, milestoning, verdicts | the platform's code, designed from the spec, never a port of engine Pure |
| reads over the platform's own facts (`$mapping.classMappings`, plan-node closure) | Pure views over the system tables (today's `SystemMetamodel`): no branching on computed values, no recursion |
| everything else written in Pure: tests, helpers, engine-authored tree walks (`toPostgresModel`) | stays Pure, loaded from files, compiled by the same compiler |
| code that needs the engine's internals to produce a value | a wall, named and counted: no scaffolding, no phantom natives, no partial evaluation |

Two consequences. A file is admitted as a program library only if every function
in it is a program (rule 8; a file mixing programs with engine internals, such as
`relational/extensions/extension.pure`, is not admitted whole, and the tests whose
subject is the internals are walls). Overloads are Pure semantics and always
honoured (`x->toString()` runs the class's own `toString()` first); what decides
`Pair.toString` is where the body comes from: generated from the spec, never
loaded from a checkout at run time. The corrections this made (batch 147): the
engine's plug-in registry (`relationalExtensions()` and its `connectionEquality`
closures) is engine machinery, not code as data; reading an engine record's
fields "by need" is rejected as a mechanism (it partially executes engine
internals, field by field, with no principle for the next field); and the
natives once registered only so that engine code would type (`relationalExtensions`,
`setUpDataSQLs*`, `createDbConfig`, `moduleExtension`) each become a generated
body, a real lowering, or are deleted (§10 has the kind that took `createDbConfig`).

### 8b. The prelude is a module, under five tenets

Decided 2026-09-08 (batch 151; the user: "why do we have two different ways to do
derived properties? why doesn't everything go through the user pipeline?"). The
generated prelude is a Pure SOURCE (today
`core/src/main/resources/com/legend/builtin/prelude.pure`, written by
`bazel run //:update_generated` and diff-tested by the build): one `###Pure`
section per spec file with that file's imports, each declaration copied VERBATIM
from the spec, constraints, stereotypes, tagged values, derived properties and
defaults included. It joins every graph as the boot layer beside the system
metamodel: resolved under its imports, normalized (a derived property lifts to a
function, as a user class's does), cached once per process. One exception to the
verbatim copy: legend-pure's `m3.pure` is written in the M3 instance syntax, so the
generator reads it structurally and prints each class and enumeration as a
declaration, refusing loudly what it cannot read. The alternative, a
protocol-to-Pure printer over the old static catalog, was rejected: it kept two
lift paths and added a mechanism where the module removes one.

The tenets (ratified with the user 2026-09-08):

- **T1 — The prelude is what exists before any program is written:** legend-pure's
  platform packages, plus the platform's own vocabulary, plus the closure of what
  those declarations name. Vocabulary is decided by USE, never by provenance: a
  class is prelude material when the platform's code constructs it, reads it or
  names it in a native signature, whichever checkout declares it; a mention (a
  comment, a dispatch string) is not use.
- **T2 — The graph is everything a program declares or brings in as a library, by
  file.** Engine modules are programs (rule 8); "a corpus test names it" is never a
  reason to be in the prelude.
- **T3 — A prelude declaration names only prelude or catalog types.** A graph class
  it names is platform vocabulary too and moves to the prelude.
- **T4 — A name declared on both sides is a modeling error, not a contest.** The
  prelude wins, with a receipt list. (Planned to burn to zero; measured when demand
  was cut to T1, it is instead a stable list of vocabulary classes the graph's own
  files also declare, pinned shrink-only.)
- **T5 — The module is a closed library the boot layer checks:** every type a
  declaration names resolves inside prelude ∪ catalog, and everything the spec
  declares on a class is carried and lifted as for a user class.

**Closure follows declarations, never bodies** (the user's ruling, 2026-09-08):
supertypes, property types and derived-property signatures are closed over; what a
body names resolves where the body is used, against everything loaded then.
Following bodies would pull the engine's world in through the back door. So "the
prelude's bodies type" is a measured fact (the spec body census, shrink-only), not
a generator rule.

**Newer than the tenets** (the claim registry, 2026-09-10, §10): the prelude also
carries every upstream function the platform does not implement: upstream's Pure
body for a bodied one, a respelled `native function` for a native one, so "unknown
function" is never the answer for a function upstream declares. The 2026-09-08
homework's "the prelude never carries functions" is superseded. As of 2026-11 the
file holds 485 classes, 23 enumerations, 86 functions with bodies and 67 respelled
natives (a snapshot: every regeneration may move it).

Sources: `docs/SYSTEM_PRELUDE_DESIGN_2026_09_08.md`, `docs/PRELUDE_MODULE_HOMEWORK_2026_09_08.md`, `docs/CLAIM_REGISTRY_DESIGN_2026_09_10.md` (git history: docs/HISTORY.md).

## 9. Upstream: one release, read four ways

Upstream (legend-pure and legend-engine) is the spec. Two facts govern how lite
touches it (the upstream boundary program, 2026-09-10).

**"Native" means two unrelated things.** Upstream's `native function` is an
implementation detail: "this body is Java, not Pure" (in the roots read then, 286
native against 14,182 with Pure bodies: the Pure platform is 98% Pure). Lite's
platform signature is a semantic claim: "the platform has a lowering for this
function". The keyword `native` in lite's signatures is a spelling inherited from
an earlier port; read it as "platform-lowered". The two axes are orthogonal:

| | lite lowers it | lite does not (the prelude carries it) |
|---|---|---|
| **upstream native** (Java body) | a platform signature, its text verified against upstream's | a respelled `native function`: resolves, type-checks, fails at lowering with "not implemented" |
| **upstream bodied** (Pure body) | a platform signature: lite overrides the Pure body with SQL, and the prelude leaves the body out | upstream's body, compiled like user code: where most of the language lives |

Membership in the implemented surface follows the second meaning only. Nothing
about it follows from whether upstream happened to write a function in Java.

**The four ways the repository touches upstream:**

| mode | what it is | examples | the rule |
|---|---|---|---|
| spec text | the pinned checkouts, read as files | the prelude generator, the spec census, the relational corpus, PCT channel B, the parser corpus | one pinned release tag; read only from the upstream-facing modules (`spec`, `pct`, `parser-equivalence`); a moved file is loud |
| oracle jars | upstream's own code, run beside lite's for a differential verdict | the reference parser, the PCT framework, the protocol differential | the same release as the source: jars exist only at release tags, which is why the source pins a tag |
| copied facts | upstream declarations inside the platform | platform signatures, the core import group (its order is semantic), platform type spellings | generated from the pinned release and asserted byte-equal, or deleted in favour of a live comparison; membership decisions stay lite's |
| calibrated ratchets | numbers that are functions of upstream content | PCT expected failures, discovery pins, rosters, census rows | re-pinned at every bump, each row with a reason |

Spec text, oracle jars and ratchets verify behaviour. Copied facts are
transcription, which no behaviour test verifies: a stale copy gives a confidently
wrong answer that every behavioural test agrees with. Hence the rule that every
copied fact is generated and byte-checked.

**What the boundary keeps true.** Upstream is ONE release, named in one file (today
the PINS block of `release.MODULE.bazel`: legend-engine 4.145.0, and legend-pure
5.99.0 derived from that release's own pom; each source pinned at its release tag).
The platform has ZERO upstream dependencies, and every upstream fact inside it is
GENERATED from the pinned release and checked byte-equal on every build (today
`bazel run //:update_generated` writes them and `bazel test` fails on a
difference). The implemented surface is exactly the claimed functions (§10), and the
prelude carries everything else upstream declares. Everything that reads upstream
lives outside the platform. A bump is then one command
(`bazel run //tools/bump -- <engine release>`): it rewrites the pins, regenerates and
runs every gate, and a person reviews three things: the generated files' diffs (the
upstream change, made legible), the ratchet moves with their reasons, and the
ledger adjudications. A bump can no longer silently change a signature under lite,
move a file lite reads, change the wire format, add a function lite does not know
(the prelude carries it), or leave two referees on different releases.

Sources: `docs/UPSTREAM_BOUNDARY_PROGRAM.md` §0, §1, §1b, §5 (git history: docs/HISTORY.md).

## 10. What the platform implements: derived from dispatch, never a hand table

The implemented surface is a computed fact, never a hand-maintained list (the claim
registry, 2026-09-10; the user, of a hand table: "what is it actually giving us
besides another hand maintained mapping that could get out of sync with grep?"). A
function counts as implemented when something the compiler actually dispatches
from registers it.

- **A function may carry several claims, because it lowers differently by
  position:** `max` is a scalar rule over a collection and a reducer under
  `groupBy`; `first` is scalar, reducer and window function. "Exactly one claim per
  overload" was a false model; every registration is recorded, and the invariant is
  NONE UNCLAIMED, ratcheted shrink-only.
- **A closed family is a closed type** (the calendar functions, the asserts, the
  TDS row getters): an enum whose members carry their declarations and whose
  dispatch is an exhaustive switch with no default, so adding a member fails to
  compile until every switch handles it. The type is the set and the dispatch key,
  with nothing beside the code to keep in sync. Exhaustive types where a family is
  closed; a completeness test where the set is open (the string-keyed lowering
  tables).
- **A subsumed engine program is a third kind**, neither implemented nor a wall: an
  engine program reached only for its machinery (the engine's SQL-printer config,
  `createDbConfig`) whose value nothing consumes. The platform does not declare it
  (the corpus's own declaration types the call), no platform source names it, it is
  cited against the pinned checkout, and the list only shrinks; the inliner stops at
  it as an opaque typed value.
- **The surface is committed and reviewed as a diff:** today `native-claims.tsv` (one
  row per overload: its kinds, owners and other sites), regenerated by the build and
  checked byte-equal.

**Its successor, the implementation table** (the untangle, 2026-09-24): one
declaration per function id (upstream's own signature id, compared whole), and each
declaration's implementation exactly one of a closed set of kinds. In today's code
(`com.legend.platform.Implementation`) they are six: a language form, an intrinsic
(the platform's own rule, possibly in several positions), upstream's body, the
platform's own Pure over its rows, unimplemented (an upstream native lite does not
run) and refused (with its reason). The user's standing rulings for it: no string
identity for a resolved function (an id is never cut, affixed or prefix-tested),
and no compiler reference to PCT or to any category of function ("that just means
we are hacking around our generic capability"). The claims ledger retires once the
table is the surface (its own header says so).

**Lite's own functions** (`meta::legend::lite::`, reviewed one by one 2026-09-11)
are each one of three kinds: ENGINE (a name from the engine's relational grammar or
dynafunction registry with no Pure signature, where the translator lands), IR (an
intermediate form only lite's normalizer or resolver emits, never user-reachable),
or SURFACE (a user-reachable lite-dialect feature: `navigate`, `joinWithPrefix`,
`asOfJoinWithPrefix`, `sourceUrl` at the review). Only SURFACE functions are
user-visible deliberate divergences, so only they need a semantics-register row. A
name that is neither engine vocabulary nor needed as IR is deleted: `avg` (the
engine spells it only as SQL output for `average`), a one-argument `hash`, four
`sub` overloads with no producer.

Sources: `docs/CLAIM_REGISTRY_DESIGN_2026_09_10.md`, `docs/UPSTREAM_BOUNDARY_PROGRAM.md` §3 D, `docs/LITE_REVIEW_2026_09_11.md` (git history: docs/HISTORY.md).

