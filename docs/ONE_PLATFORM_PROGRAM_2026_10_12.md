# The rest of the program: twin hosts, one implementation, and legend-lite as a drop-in legend-engine

Agreed 2026-10-12. Rewritten 2026-10-13 as the runbook a fresh session builds the whole program from, end to end.
**Status (2026-11 docs cleanup):** by 2026-10-13 phases B to E (less B7) and part of G were recorded as built, PR #31
and PR #36 on main and the rest on branches; §1, §6 and §7 carry rulings of 2026-10-15. Kept here: the product's rules (§1), how
work is done (§2), the open phases as §5 last recorded them, the pull-request order (§6) and what is decided (§7).

The user, 2026-10-12 and 2026-10-13:
- "lets plot out the whole rest of the program completely so we can see every step";
- "we should try as hard as we can to use the same interfaces inside them - with only the 'storage' part different";
- "really why we want legacy mode is actually so that legend-lite can be a drop in replacement for legend-engine - and
  so legacy mode really has two parts the UI talking to real legend-engine and the UIs talking to a real legend-lite
  server over the EXACT same protocols/APIs";
- "the tenet is that typescript and python are twins - so both columns should be the same";
- "please write it in detail so that we can have a fresh session implement the whole thing end to end please expertly
  from the docs".

## 0. How to use this document

Read all of it before writing code, then the designs it cites for the step in hand:
- `docs/DATACUBE_ONE_APP_DESIGN_2026_10_11.md`: the one app, its services, R1 to R6 (§13), and PlanAndRun (§15);
- `docs/DATACUBE_MODEL_DESIGN_2026_10_10.md`: classes, associations, data spaces, dashboards on the model (§13, §15);
- `docs/DATACUBE_HEADLESS_PYTHON_DESIGN_2026_10_11.md`: DataCube's code in V8 inside Python;
- `AGENTS.md`, `docs/IN_FLIGHT.md` and `docs/GATES.md` on main: the architectural invariants, what other lines are
  doing, and how a landing is recorded.

Each step below has:
- **its goal**;
- **what is decided** (never reopened without the user);
- **the design**;
- **the files**;
- **its proof**;
- **its reviews**;
- **what to measure or ask first**.

Where a step says "ask the user", stop and ask, with a recommendation. Everywhere else, build.

## 1. The product's rules (the user's, each settled)

1. **Twin hosts.** The browser and Python give the same capabilities.
   - A feature's code comes with the app; a host gives only services (one-app design §12: "on Python, the work is
     services, never features"). A check in the app for one host's own pieces, rather than for a service, is a bug.
   - TypeScript and Python are twin client languages. A per-language library is designed once: the same functions,
     the same parameters, the same choice of backends (the host's own, in process, or a remote server). One shared test
     corpus holds the two alike. A two-column comparison whose columns differ in what is possible is a gap to close,
     not a design.
   - **A host plans in process** (the user, 2026-10-15). Python as a host -- its frames, its pages, DataCube on
     Python -- runs legend-lite's compiler in its own process. A remote engine is a client feature over the remote
     side's own models and data (DataCube's REST client in the browser app; a future Python client on `warehouse.py`'s
     pattern, a candidate, not built), never a mode of the host. So a Python host has one plane, and the remote
     planners G1 built are removed (G1, in the cut §5).
2. **One implementation, storage per host.** Logic that must run on more than one host exists once; only where it keeps
   its records and how a call reaches it differ.
   - Logic a server needs too is Java, built three ways: the JVM, the browser's WebAssembly module (TeaVM), and
     Python's native library (GraalVM). Examples: the compiler, the SDLC, the Depot, saved queries, the warehouse's
     binding.
   - Client glue over services is two twin libraries, TypeScript and Python, held by one corpus. Example: creating a
     class in a project.
   - DataCube's own semantics are DataCube's TypeScript. Python's pages run it headless, never a Python copy. Examples:
     what a grid's relation is, how a page exports.
   - A remote git (GitHub) is a push and pull of a local git store, never a `Storage` of its own.
3. **legend-lite is a drop-in legend-engine.**
   - Studio, Query and DataCube speak legend-engine's APIs exactly, and work against a real legend-engine and against
     a legend-lite server with the same answers.
   - A feature that needs legend-lite's own routes (Create a Class…) is offered only where those routes answer, as
     every action is (one-app design §3.4).
4. **Python's DuckDB reads only what it was given (R4).** The page sends Pure, never SQL.
   - Python's own planner, in process, writes every line of SQL Python runs (§1.1: a host plans in process; the user's
     ruling of 2026-10-15, which replaced the server planners of 2026-10-13).
   - Python plans only over a model every table of whose Databases is one Python serves: its frames, a page's files,
     and the page's projects over them (the user's option (c), `Frames.model_for`).
   - A remote file is read only from a remote URL, matched exactly against `<scheme>://`. An Iceberg table's data files
     must be remote too.
   - A query's own `sourceUrl('file:...')` is accepted (the user: "your python could just read that file anyway").
5. **Every grid is one PlanAndRun** (one-app design §15): the chosen planner's plan, the connection's runner, and the
   rows typed by the plan by the executor of the language they become (§15.3a). A Variant is its JSON text.
6. **One session on every host.** Each host runs the compiler's session setup (`Boundary.sessionSetup`: DuckDB in UTC)
   on its DuckDB as it sets it up.
7. **No hacks.** Find the cause and agree a design before code. A shortcut becomes an anchored row of
   `docs/PARKED_WORK_LEDGER.md`, never a quiet fix.
8. **The simplest right thing, done in our own pull request.** A fix that touches another line's files is built here and
   reviewed by that line, never handed off to wait behind its pull request.

## 2. How work is done here

**The worktree.**
- Work in the worktree, never `cd` elsewhere; temporary files go in its `runs/`.
- Never a bare `git stash`: the stash stack is shared across worktrees and sessions.
- Never switch branches or edit sources while a gate runs.
- The shell is zsh: `for x in $list` does not split a string, so use an array or `while read`.

**A step's life.**
1. **Branch** from main: one branch per pull request, its steps separate commits.
2. **Announce** a cross-area edit (core, or another line's files) in `docs/IN_FLIGHT.md` on main before building it: the
   files, and why. When it lands, record "files touched beyond the announced list".
3. **Build and prove locally:**
   - `bazel test --lockfile_mode=error //gates:local //datacube/... //python/...`, plus every package the change
     touches;
   - the lane query, which must be empty: `bazel query 'tests(//...) except tests(//gates:lanes) except
     tests(//gates:heavy)'`.
4. **Review:**
   - an independent audit agent (`auditor`), on the commits; every blocker fixed and every should-fix answered before
     the pull request;
   - Bazel changes by the Compiler Rewrite session;
   - a planner-boundary change (`Boundary`, `TabExports`, the native library's entry points, a plan's shape) by the
     Plan Gen / Exec Split session as well;
   - Studio's files (`studio/`, `query/`, `site/`, `sdlc-*`, `depot-*`, `query-store/`) by the Studio / SDLC / Depot
     session.
5. **Pull request** with the description's sections: what it is; What this closes; Proof; Windows; Independent review.
6. **Merge:** the full CI run must be green on every platform.
   - A job failed by a runner's filesystem error (a Node module tree, lost remote-cache inputs) is re-run once.
   - Merge is an admin merge, with the user's approval each time.
   - Main then runs its own CI. If the merged commits carry `[skip ci]`, GitHub skips the push run, so dispatch
     `gate.yml` on main by hand.
7. **After landing:** a GATES entry and the IN_FLIGHT "landed" line, a docs-only commit on main (no CI runs for
   Markdown).

**Never:**
- push code to main without asking;
- merge without the user's approval;
- reopen a decision of §1 or §7.

**Tests.**
- **Every test target is in a lane** of `gates/BUILD.bazel`.
- **A Python test never touches a real home:** `python/tests/main.py` points `LEGEND_LITE_HOME` at the test's folder.
- **The headless lane** (`//datacube:headless_test`) runs every DataCube test file in V8. One that needs the page's own
  APIs is left out with a reason (`_HEADLESS_LEFT_OUT`).
- **Each test's import list** (`datacube/test_imports.bzl`) is regenerated by `bazel run //datacube:update_generated`.
- **Pure text added to a test** needs `//parser-equivalence/...`.
- **A typing change** needs `//spec:reference_lane`.
- **A bundle that grows** raises its budget in `datacube/test/bundle-budget.test.ts`, with the measured size before and
  after and the reason.

**Every commit ends with:**

```
Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>
Claude-Session: <the session's link>
```

**Explaining to the user.**
- Use plain words.
- Give one settled recommendation, not a menu.
- Ask before a decision of the user's; never decide it quietly.

## Open phases (as §5 recorded them, 2026-10-15)

Phase L, the legacy-mode check of each UI against legend-engine 4.145.0 and against a legend-lite server (the
saved-query API first), is the Studio / SDLC / Depot session's; it waits on the user's OK and on MongoDB, and B5's
acceptance waits on it. B4's two orchestrations are to become one Java "edit a project" operation that also works
with a real legend-sdlc (the user, 2026-10-15), and the revision a page is pinned to is still owed. B7, a remote git,
and the model plan F1 to F5 are not built. Of the follow-ups, true streaming (G2, PARK-36), one DuckDB release
everywhere (G3), the optional G6 and G7, the upload's SQL in the dialect (G8), one result-typing table (G9) and
sign-in where a store is shared (G10) remain, as do a page's board exported headless (PARK-32) and H, publishing,
which is the user's to decide. Check `docs/PARKED_WORK_LEDGER.md` for what has moved since.

> Cut in the 2026-11 docs cleanup: §3, where things stood on 2026-10-13; §4, the logic that existed twice (measured 2026-10-12 and 2026-10-13); §5, the phases L and B to H with their designs and as-built notes (2026-10-12 to 2026-10-15). The step ids §6 and §7 name (B2, B4–B7, C1–C5, D, D5, E, F1–F5, G1–G12, H, L, L1) are defined there. The upstream contracts the phases relied on are in `datacube/docs/ENGINE_API_CONTRACT.md`, `docs/UPSTREAM_BEHAVIOR.md` §5 and `studio/docs/UPSTREAM_STUDIO_CENSUS.md`. Full text: `git show de2745351:docs/ONE_PLATFORM_PROGRAM_2026_10_12.md`.

## 6. The order: the pull requests

| # | Pull request | Steps | What you get |
|---|---|---|---|
| 1 | Python parity | G1 + G5 | Remote planners on Python's pages (removed 2026-10-15: a host plans in process, §1.1); Iceberg files on Python, every data file remote |
| 2 | The model actions as a generic Legend capability, and headless pages | B4 + D | Twin libraries, TypeScript and Python, for a class, an association or a data space in any project; Python's `grid.create_class()`, `page.create_association()`, `page.create_data_space()`, `page.export()` and `page.save()` |
| 3 | Stores on one implementation | B5 + B6 | Saved queries and saved pages each one set of Java rules in the browser, Python and the server; Python's pages get saved queries; L1's saved-query differential the acceptance test |
| 4 | One engine path | C1 + C2 + C4 | The tab answers legend-engine's execute API with Python's Java; Query's SQL button with no server; one planner protocol |
| 5 | Warehouse on Python | C3 | Warehouse tables on Python's pages, Live on the warehouse, Snap into Python's DuckDB |
| 6 | Studio and Query from Python | G11 + E | `ll.studio()` and `ll.query()` on Python's own SDLC, saved queries and engine |
| 7 | A project's data where its queries run, and no SQL text in TypeScript | C5 (PARK-35 + PARK-33) + D5 | Query and Studio on Python, and on legend-lite's server, run a project's queries on its own data; a saved query opens as a grid on Python's page; the tab and Python's host spell no SQL |
| 8+ | The model plan | F1 to F5 (B7 with F4) | Dashboards on classes, joins in prep, the model view, promote and publish, measures |

- Rows 1 to 3 are independent of each other, so each lands as soon as it is green.
- Row 6 needs rows 2 and 3.
- G2, G3, G4, G6, G7, G9 and G10 are taken where they block a row. L runs beside all of them.
- H comes last.

## 7. Decided: not to be reopened without the user

- One DataCube app on every host. Python's pages give services; Python's model home is an SDLC (one-app design R1 to
  R6).
- PlanAndRun for every grid. Each host's executor types its results by the plan.
- R4 as §1.4 states it, including option (c), the accepted `sourceUrl('file:...')`, remote-only files and Iceberg
  data files.
- Python plans in process, always (the user, 2026-10-15: DataCube on Python has no remote engines, and Python as a host
  is no client of one; §1.1). Running queries on a server's own models and data, if wanted, is a separate client on
  `warehouse.py`'s pattern, recorded as a candidate.
- The two DuckDB versions, accepted until G3. Python's one shared connection, a feature.
- Python's model home:
  - the place `Python` (the tab's `Browser`);
  - the SDLC user `local` / "Local User", as the browser's;
  - no Studio link until E.
- Row order and float last digits without a sort are not guaranteed: "users have to add their own sort if they want
  that guarantee like database query".
- The page's settings are kept by Python (B2). Each host's DuckDB runs the compiler's session setup.
- Iceberg on Python ships, with its data files remote (G5).
- The model actions are twin libraries, not DataCube code (B4).
- legend-lite is a drop-in legend-engine over the exact same APIs (L).
