# The program: an amazing build with a self-contained upgrade bump (2026-10-06)

**Status (2026-11): stalled; the Rust port is now the work.** The program stopped after the state of 2026-10-10
(`docs/build-inventory/program/START_HERE.md`, its status line). Landed: Phases 0 to 2, the CI landing L1
(2026-10-07), Phase 3 (L2, 2026-10-09), Phase 3b (L3) and L7 "resolve once" (2026-10-10). Never started: Phases 4 to 6
(L4 to L6), the bump (Phase 7, L8), the typer's order (L9) and Phase 8 (L10 to L13). What stays here outlives the Java
build: what the program is for (§0), the north star (§1) and the user's decisions (§2). The landing rule below ("No
PRs") is superseded: since 2026-10-26 `AGENTS.md`'s "Pushing to main" governs.

Begun by the user, 2026-10-06: "we should do the whole thing here, in phases that make sense - let's get it fully fully
done". Each phase is proven locally first and lands on main from its own branch. **No PRs since 2026-10-06** (the
user: "I think we should not do PRs anymore so you can automatically push to main without my manual step"): an
audit, the local gate, one full CI run on the branch, then that exact commit pushed to main (Phases 0 and 1 landed as
PRs #25 and #26, before the change).

**A session picking this program up starts at `docs/build-inventory/program/START_HERE.md`**: the status, the
working rules, how to run every check, the traps. The execution briefs, one per remaining phase, were removed in the
2026-11 cleanup (`docs/HISTORY.md`); this file stays the plan.

This file is the program's one plan. The decisions and evidence behind it:
- `docs/BUILD_REBUILD_DESIGN_2026_10_05.md`: the build (decisions D1 to D15);
- `docs/GENERATORS.md`: every generator, its axes, the bump's records and the seal;
- `docs/UPSTREAM_ONLY_HOMEWORK_2026_10_05.md`: each upstream generator's inputs and its upstream-only design;
- `docs/MANIFEST_WORLD_HOMEWORK_2026_10_05.md` and `docs/MANIFEST_WORLD_EXPERIMENTS_2026_10_06.md`: the default world,
  measured (git history: docs/HISTORY.md; the decision is `docs/GENERATORS.md`, "The default world").

## 0. What the whole program is for

Written 2026-10-07 at the user's request ("add the actual overall goal of this program not just phase 3 ... so we have
holistic view of what we are trying to accomplish"). Each part quotes the user's own words, with the date they were
said. The program has four parts, in this order, each built on the one before; after the bump lands come the
compiler's debts (0.5).

### 0.1 A first-class Bazel build, starting with a fast compile of only the product (the base; first deliverable done)

**The goal.** An expert, principled Bazel build. Bazel knows every step: what it reads and what it makes, so Bazel
alone decides, deterministically, what runs and when. Nothing outside Bazel drives the work: no shell script, no Java
or Python program, no Node. As hermetic as Bazel allows: every tool and every piece of data pinned, nothing read from
the machine (no host JDK, no hard-coded paths, not even PATH), the same bytes on Linux, macOS and Windows. Its base is
a very fast compile of only the real product.
- The user, 2026-10-03: "the build needs to be principled and bazel first class - not a hodge podge of random scripts";
  "there are java code orchestrators like Repo.java that run outside of bazel?"; "at end of whole plan will we have
  deleted all non-bazel orchestration like calling out to shell scripts or java?"; and Windows must not break for the
  people who use it.
- The user, 2026-10-05: "our java build used to take 30 seconds for clean build now saying 5-10 min - we first have to
  fix the basic basic stuff - a single java build that makes sense and is fast"; "building all of our java stuff should
  be around 30 seconds so how do we carve up all the rest of everything correctly in the right way to get to a sane
  build? I don't want any guessing or sampling".

**What "the product" is.** The servers: core (the compiler and the execution pipeline), the warehouse, and the SDLC
server, which carries Depot's rules until Depot becomes its own server (design D12; D11 renames them core, db, sdlc,
depot). Then the web bundles (DataCube, Query, Studio), the planner compiled to WebAssembly (TeaVM), and the
warehouse's native image. Generators, tests, checks and measurements are not the product build.

**How the first deliverable was built (Phase 0, PR #25, merged 2026-10-06 as `ab4723ba2`).**
- The inventory first, no sampling: all 1,162 targets from Bazel's own graph, each with one verdict (compile, tool,
  generator, check, test by kind, wiring, dead), every claim citing a file and line (`docs/build-inventory/`, design
  §2).
- Named compile targets (design §4.1, §5a): `//:java` (every runtime jar of the servers, computed from their
  dependencies), `//:web` (8 esbuild bundles; no Node: Bazel calls the esbuild program directly), `//:wasm`,
  `//:native`, and `//:sites` (packaging).
- A guard keeps them honest: `//tools/guards:compile_only_test` fails if anything but compiles and file plumbing enters
  them (proven to fail on a generator put into `//:native`).
- The slow clean build's cause, found by measuring: the Maven plugin stamped the DuckDB jar's manifest and copied it
  (17.4 of the 19.7 s critical path). The product's jars moved to Bazel's own `http_jar` (pinned by URL and sha256),
  and stamping went off.
- Result (design §5a-§5c; macOS, clean, three runs): **a clean `//:java` in 12.4 s** (22.6-23.1 s before; the
  critical path is now our own code, the compiler's jar); a one-line comment edit to `Compiler.java` rebuilds in 0.44 s
  (one library: its interface did not change, so nothing above it recompiles); a clean `//:web` in 4.8 s.

**What is left of this part (Phase 8; `docs/build-inventory/program/PHASE_8.md`, git history: docs/HISTORY.md; its
CI part landed as L1 on 2026-10-07 and is described in `docs/GATES.md`, "The CI lanes and why"). Written 2026-10-07,
before L1:** "Build means compile" holds for
the compile targets, not yet for CI: CI's build lane runs `bazel build //...`, which also runs the corpus passes and
the 2 GB `//pct:ratchets`. Tests and some generators still run on Node. Scripts outside Bazel still drive work (the
SQL census's `lanes.sh` and `render.sh`, P3-25's corpus runner, scripts that read a host JDK: the brief's Short-22,
Short-25, Exec-2). Checks run as reports in every package. CI's cache barely works. The build's own carried shortcuts
are listed in the brief's §2.8.

### 0.2 The bump as a standalone piece (Phases 1 to 7: the main body)

**The goal.** Moving to a new upstream release (legend-engine and legend-pure) is one self-contained job: it runs only
when we move the pins, reads only upstream's files, regenerates everything made from upstream, seals it and runs the
tests; a person judges what changed. Nothing else ever regenerates an upstream-derived file, and no upstream
generator reads our code.
- The user, 2026-10-05: "we def need to keep the generated code only generated on upstream bump".
- The user, 2026-10-06: "I want the bump to be a self contained thing that only runs when need to bump"; "So how do
  we actually get to self contained bump for real?"; "We still need the java override list? And the generators still
  need our list as input"; "let's get it fully fully done so we have an amazing build with self contained upgrade
  bump".
- The user, 2026-10-07: "really want to get this bump self contained program done".

**Why it takes more than build work.** The upstream generators took our own lists as input (Pure.java's signature
catalog, the "override list"; the prelude generator's hand lists; `native-claims.tsv` with `core_next`), and the
compiler decided some calls by name, so the bump could not run from upstream alone. Getting there restructures how
the product boots and what it reads from generated upstream code, and fixes the compiler where it differs from
legend-pure:
- Phase 1 (done, PR #26, `96bf6ae5d`): generator hygiene; the small upstream generators made upstream-only.
- Phase 2 (done, `ff70aef01`): DynaFn and the engine handlers generated whole from upstream; our decisions joined when
  the classes load.
- Phase 2b (done): the end-state experiment (every upstream declaration in the world, no catalog).
- Phase 3 (landed 2026-10-09 as `1e4a2bd40`): one table decides, by exact function id; overloads ranked as
  legend-pure ranks them.
- Phase 3b (landed 2026-10-10 as `991df952b`): the compiler fixes the bump and users need (the boot layer's twin
  files, the mapping files, `routeFunction`, F-L1).
- Phase 6: the corpus loads its real manifest instead of our hand lists of files.
- Phase 4: the default world (what users boot on) generated from upstream alone, replacing `PreludeGenerator`; the
  system metamodel's result views leave startup.
- Phase 5: Pure.java becomes rows keyed by function id; the catalog, the membership list, `native-claims.tsv`,
  `core_next` and `gen_claims` go.
- Phase 7: `bazel run //tools/bump -- <release>` decides, moves the pins, regenerates, writes the reports and the
  seal, and runs the tests. **Acceptance: a real bump to the next legend-engine release, end to end, with no hand edit
  to any generated file.**

The target shape is §1, the north star (three kinds of files, never mixed). One open point bounds "only upstream's
files": a generator that reads upstream through our parser (Pure.java's generator today; the default world's
generator, the Phase 4 brief's D4-5) makes a parser change a real trigger too (design §4.2 group B), and the seal has
to say so.

### 0.3 Our own generators, separate from the bump (Phases 1 and 7, then Phase 8)

**The goal.** Generators that have nothing to do with the bump (made from our own code or from pinned data) run only
when what they read changes, as Bazel actions with exact inputs. Things that only look like generators, measurements
of our own engine (corpus results, ratchets, the ladder), become tests.
- The user, 2026-10-05: "we need to split the generation work into 'generators that are needed for version bump' vs
  'our generators that have nothing to do with version bump at all' vs 'tests that look like generators'"; "Why is
  relational corpus in generators at all?" and then "relational corpus should be tests"; "I don't think we should make
  update generated manual until we go through in detail exactly what all the generators are doing".

**Where.** `docs/GENERATORS.md` places each of the 57 generators on two axes (what it is, when it runs), with a
dossier each in `docs/build-inventory/generators/` (git history: docs/HISTORY.md); design §4.2 groups them A to F by what truly changes them. Phase 1
narrowed every one of ours to what it reads (done). Phase 7 makes `//:update_generated` ours only and `manual`, with
a guard that every writer and diff test belongs to a group. The rest is the Phase 8 brief: §2.2 (Gen-1 to Gen-9: the
JavaScript generators off Node, the offer-facts tool reading its own output, drafts `manual`) and §2.9 (the
measurement group, each carved by its true trigger).

### 0.4 The tests, untangled (after the bump)

**The goal.** Each family of tests depends only on the code it exercises, so it reruns only when that code changes.
Each CI lane is a Bazel suite in `//gates`, and a guard holds that every test is in some lane. No Node: the JavaScript
tests and the browser checks run in the pinned Chromium through a small client for Chrome's remote-control protocol
(CDP). Comparing a run with a previous run, or with a sibling run, is done the Bazel way, not by scripts.
- The user, 2026-10-07: "various different tests like core, rcorpus, stress corpus, different flavors of PCT, parser
  equivalence, UI/playwrite, etc etc etc that we also want to untangle after".
- The user, 2026-10-05: "what tests do we have that should run when"; "what is the bazel first class native way to do
  run by run test compares either to a previous run or to sibling runs? What would expert do".

**The families today** (2026-10-07, before L1; the lanes as `docs/GATES.md` on main listed them), and what tangles each (the Phase 8 brief's
item):

| Family | Targets | Tangled by |
|---|---|---|
| Core | `//core:core_tests` (gate 1), `//core:corpus_differential_test` | one `core_tests_lib` carrying the stress corpus and the linked projects (Test-1); heavy classes skipped by name, not by tag (Test-10) |
| Spec parity | `//spec:spec_tests` (gate 3) | one `spec_tests_lib` (Test-2) |
| The relational corpus ("rcorpus") | six passes, a host judge and a database judge on each of DuckDB (`//spec:corpus_duckdb`, gates 4 and 11), H2 (`//spec:corpus_h2`, gate 5) and the warehouse (`//spec:corpus_warehouse`, manual); the reference lane (`//spec:reference_lane`, manual) | the passes are build actions, so `bazel build //...` runs them (CI's build lane up to twice per platform); hand lists of files until Phase 6; the manual ones in no lane (Test-11) |
| The stress corpus | `//core:stress_suites`, `//core:stress_suites_h2` (gate 10); its generators `//scripts/corpus:gen_*` | one Python library for five gates (Gen-7); the engine-side runner P3-25 unfinished (Exec-2) |
| PCT, four flavors | `//pct:pct_duckdb` (five suites, gate 6), `//pct:pct_h2` (gate 7), `//pct:pct_postgres` (five suites, gate 7P), `//pct:pct_channel_b` (gate 9); `//pct:pct_discipline`; `//pct:ratchets` (manual, 2 GB) | Channel B builds the adapter archive it does not use (Test-4); CI's build lane builds `//pct:ratchets` |
| Parser equivalence | `//parser-equivalence:parser_parity` (gate 8); `:diagnostics` (manual) | `pe_tests_lib` depends on all of core (Test-3); test data no test reads (Short-6) |
| The model projects | 56 compile tests (`tools/legend`) | 56 JVMs on all of core (Test-5) |
| The app, WebAssembly, the warehouse | the app lane (`//datacube:tests`, `//datacube:verify_app_test`, `//wasm:all`, `//warehouse:tests`, `//query-store:lite_test`); the native lane (`//warehouse:tests_native`, `//warehouse:launcher_test`) | the native image inside `//gates:local` (Test-9); `postgres_live` in no lane (Test-8) |
| The UI and the browser | `//datacube:live_snap_test`, `//datacube:verify_smoke_test`; the Playwright harnesses (tagged `browser-ci`, run by `bazel run`, Linux only) | Node and Playwright throughout (Node-1 to Node-9); CI installs Playwright's own Chromium (Short-18) |
| Checks | `//:generated` and the guards (the checks lane) | a report in every package (Check-1 to Check-7) |

**Where and when.** The Phase 8 brief: §2.3 (Test-1 to Test-13), §2.5 (no Node), §2.6 (CI and caching), §2.9 (the
measurement group). It comes after the bump because the bump's phases change these tests: Test-1 waits for Phases 3b,
4 and 5 (they add core tests), Test-2 for Phases 6 and 5 (Phase 6 rewrites the corpus runner). **Not designed yet:**
the Bazel way to compare a run with a previous run or a sibling run. Today the corpus compares against committed
results through diff tests, while the SQL census (`tools/census/`) and the landing checks (START_HERE §5) compare two
commits by script and by hand. It needs a design agreed with the user.

### 0.5 After the bump lands: the compiler's debts

The shortcuts this program carries in the compiler are `docs/PARKED_WORK_LEDGER.md` rows PARK-5 to PARK-14 (§5). The
user, 2026-10-07: "we need a ledger of hacks that we need to come back a fix", a list "to fix correctly after we land
the bump". Two close inside the program (PARK-11 in Phase 4, PARK-12 in Phase 3b); PARK-5's timing is the user's open
decision.

### 0.6 Done means

- `bazel build //:java //:web` compiles and nothing else (the guard); a clean `//:java` in about 12 s; an edit that
  leaves a library's interface unchanged rebuilds that library alone. CI's build lane builds the compile targets, not
  `//...` (design §4.1).
- No work driven from outside Bazel: every generator, test and check is a Bazel target with exactly its inputs; each
  remaining script is a `bazel run` tool or gone; `bazel query` finds no Node toolchain.
- Nothing read from the machine; the same bytes on Linux, macOS and Windows.
- The bump: `bazel run //tools/bump -- <release>` from upstream alone; a real bump with no hand edit; the seal's test
  in every lane.
- Our generators run when our code changes; `//:update_generated` is ours only and `manual`; no "update everything"
  command re-blesses a measurement.
- Every test in a `//gates` lane, each depending on what it exercises, with a guard.
- The ledger's rows closed by agreed designs, their anchors gone.

## 1. The north star

**Three kinds of files, never mixed:**

| Kind | What it is | Who changes it |
|---|---|---|
| Upstream | the pinned legend-engine and legend-pure archives: every declaration and upstream body | only the bump |
| Ours | hand-written decisions and code: the implementation table (what we implement and how, keyed by function id), legend-lite's own declarations (`meta::legend::lite`), the system metamodel (its core views load always; its harness views join a program whose world has their classes) | people, when they decide something |
| Generated | made from upstream and one setting, only by the bump, committed and sealed: the default world (what `prelude.pure` becomes) and the other upstream tables (DynaFn, handlers, core imports, the bump's records and reports). The one setting is the module choice (the core and relational compiler modules count), kept in the bump's configuration beside the pins | only the bump |

**How a program gets its world:**
- **User programs** (Studio, DataCube, Query, the server) boot on the default world. The implementation table joins
  it at boot by function id: a row says "ours" (an intrinsic or a language form), no row means upstream's body runs.
- **Test programs** (the corpus, PCT) load their manifest from the pinned archive on top: each element once, the
  implementation table applied, platform-namespace functions from the default world only. Test input, never product.

**The bump:** move the pins, regenerate from upstream alone, write the seal, run the tests, judge the diffs. If
upstream renamed something we implement, a row stops matching and a test says so; a person updates the row.

**Never:** a generator reading our code, a hand edit of a generated file, a signature we typed ourselves.

**The build:** "build" means compile only, guarded (`compile_only_test`); every generator, test and check runs on its
true trigger; no Node anywhere; one shape per app.

## 2. Decisions (the user, 2026-10-06)

1. **The system metamodel stays in Pure** (the user: its model, mapping and store are the platform building on
   itself). What changes is where its **result views** load. Those are the parts added on 2026-09-03 (harness
   burn-down batches 18 and 20, under the 2026-09-02 ruling that the database, not Java, answers what a test reads):
   the tables, mapping and small classes that let Pure read the results of `executionPlan`, `execute`,
   `scanRelations` and `scanColumns` (plan nodes and connections, activities, lineage trees), and our versions of
   three upstream functions over those rows (`allNodes`, `routerExtensions`, `relationTreeAsString`).
   - **Everything they describe is declared upstream in two engine modules**, `core` and `core_relational`. So they
     **load in a program whose manifest includes those two modules** (the corpus's does) and never at boot. A user's
     program has no manifest and gets the default world, which holds none of those functions, so its world never needs
     the engine classes those views name, and no list of ours feeds the default-world generator.
   - **The classes stay upstream's**, used as they are; the views only say where their instances' data lives. They
     write no data at boot or ever: the rows ride each query (`PlanRows`, `LineageRows`, the activity rows).
   - **The three functions** exist upstream too. Which version runs is decided by the implementation table, by
     function id: each has a row saying "the platform's version", here our Pure over the rows. That needs one more
     kind of row, **the platform's own Pure**, which the system metamodel's other upstream-named functions
     (`classMappingById`, `mainTable`, …) also use. (2026-10-07: Phase 3 replaced the name rule with hiding by function id, `SystemMetamodel.shadows`.
     **2026-10-09, Phase 3b item 1b (3b-O1, option (b)): built.** `Implementation.PlatformPure` is the row; its registration
     is the system metamodel's function ids (`platform/PlatformPure.java`); a loaded or generated declaration with one of
     those ids keeps its declaration and takes the platform's body (`PlatformPure.adopt`, at the boot and at the graph
     merge); the "no row means refused" rule covers the names the platform implements at, so each of upstream's 14 other
     versions there carries a decision (its body runs, or a wall); `shadows` is deleted and PARK-12 closed.)
2. **Boot speed: profile and optimize the boot first**; the pre-built boot layer (Phase 4b, the parked compiler plan's
   W2.1 "generated at build time, not parsed at class load") is decided with measured numbers. (**Amended 2026-10-10,
   the user:** 4b is part of Phase 4, not optional: a compiler ships its standard library compiled. The numbers size
   it — what is pre-built and how — and no longer decide whether; see decision 6.)
3. **The product ships a generated copy of upstream bodies** (the default world). AGENTS.md and TENET_CHARTER C6.3
   are amended in Phase 4 to match WORLD_MAP rule 2 (amended 2026-09-08); so are WORLD_MAP's own §3 ("declarations only
   ... no bodies"), rule 1 ("never loaded") and rule 2's first text and 2026-09-04 amendment, which still contradict it
   (`docs/build-inventory/program/PHASES_4_5_7.md` §2.9, git history: docs/HISTORY.md), and AGENTS.md's guard name (`Runner.registerLibrarySource`
   is now `MinimalCorpus.refusePlatformNamespace`).
4. **Bazel changes are reviewed by an audit agent** before a push or PR (this session is the Bazel program).
5. **D2: native-claims.tsv, `core_next` and `gen_claims` retire** (Phase 5).
6. **The default world is made by upstream's own compiler, at bump time only** (the user, 2026-10-10; the cold read's
   F2 and F3, the Phase 4 brief's D4-1 and D4-5). The bump's generator runs legend-pure's compiler from the pinned
   jars (the reference dump's path, `tools/reference/RefResolutions.java`) over the module choice; it takes from
   the compiled graph each element's span (the text copied verbatim, tests stripped by upstream's markers), its
   imports and the references out of every seed function's signature and body; the closure **follows every body**
   (D4-1 (a)), never reading what we implement. The world is therefore a function of the pins and the module choice
   alone, sealed by them (group A); our parser reads it, proven by the parity lane; no diff test on our front end is
   needed for the world. Whether the generated text keeps upstream's imports (resolved at boot or pre-built) or is
   rewritten with full names from the same graph (the reference dump already reads every resolution with its
   position) is chosen on Phase 4's boot numbers; either way the **compiled boot form is a build step** (4b) whose
   declared inputs are the sealed world and our front end, regenerated by the build, never by hand.
7. **The typer's order is measured before the bump** (the user, 2026-10-10): before L8, count the reference lane's
   OVERLOAD rows that are typing-order cases (PARK-6 to PARK-10: arguments typed once, the function chosen on typed
   arguments, our tie-breaks) rather than the recorded type-only ones (S40, S41). More than a handful, and L9 moves
   before L8, because the bump's safety ("a changed upstream makes a test say so") rests on our picks matching
   legend-pure's where the corpus does not look.

> Cut in the 2026-11 docs cleanup: §3 the phases with their status, §4 the order of the landings L1 to L13 and what
> can move, §5 working on this program, §6 the order and cadence from Phase 3b on (2026-10-06 to 2026-10-10).
> Full text: `git show de2745351:docs/REBUILD_PROGRAM_2026_10_06.md`.
