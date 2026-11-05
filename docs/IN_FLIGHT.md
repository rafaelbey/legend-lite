# In flight

This file says who is working, on what files, in what order, and the rules between sessions; each line's status lives
in its own plan (plan rule 0b.16).

## Now: the Rust port

The work is the Rust port, on the branch `rust-port`; its plan is `docs/rust-port/ROADMAP.md` there (iteration 1:
GraalVM out of `libcompiler`, the whole planning path in Rust; Java keeps the executor, the HTTP shell, the test
runners and the warehouse, SDLC, Depot and query-store modules). The port's session works on a branch of its own and
lands it by fast-forward (rule 2).

Beyond `rust/` and `docs/rust-port/`, the port edits three files that work on `main` edits too:
`docs/PARKED_WORK_LEDGER.md`, `docs/dialect-capability/*` (generated: regenerated, never edited by hand) and
`tools/deps/core-layers.txt`. An edit to any of them, on either branch, is announced here first, one line, before it
lands (rule 5).

## Open

- **PR #83, the planner-driver seam program** (`docs/PLANNER_DRIVER_SEAM_PROGRAM_2026_10_30.md`, planned 2026-10-30
  and not ratified: the user rules on its §6 first): its branch holds the decisions log's D78 to D108, unmerged
  (`docs/GATES.md`'s 2026-11-01 record; main's log ends at D77). Whether it lands is the user's call.

## The lines of work, 2026-09-29 to 2026-11-01

Each line announced its owned files and cross-area edits here, one at a time; the announcements and landings are cut
(the pointer at the end). Each line's status lives in its plan. The last state below is what this file recorded.
A plan marked "history" was removed from the tree: it is listed in `docs/HISTORY.md`.

| Line | Plan | Last state recorded here |
|---|---|---|
| The Bazel program, then the build rebuild and the self-contained bump (the 2026-10-04 order's first line) | `docs/BAZEL_FIRST_CLASS_WORKPLAN_2026_10_03.md` (its log `docs/BAZEL_EXECUTION_LOG.md`: history); `docs/REBUILD_PROGRAM_2026_10_06.md`; `docs/build-inventory/program/START_HERE.md` | The CI landing L1 landed 2026-10-07 (L1a `fd1b0ba77`, L1b `d126b47e1`, then L1c to L1e); Phase 3 (L2), Phase 3b (L3) and L7 by 2026-10-10. Stalled since: START_HERE §3 |
| The compiler line, session "Compiler Rewrite" (the compiler rebuild) | `docs/EXECUTION_PLAN_2026_09_26.md` (being folded into `docs/architecture/README.md`; history) | Parked 2026-10-04; resumed 2026-10-07 on evidence (D25); its last landings recorded here are of 2026-10-14 and 2026-10-15 (`d619c5cd6`, `9bc695885`, `487a0f460`: the three planner defects the tenth line found, a computed column reading the enclosing row, the compiler's two scale costs) |
| Second, 2026-09-29: DataCube against the warehouse | none named (branch `datacube-live-snap`) | Announced 2026-09-29: token refresh, GRANT checks, a Linux-only browser lane; no landing recorded here |
| Third, 2026-09-30: DataCube track A and the BI plan | `docs/BI_AND_ETL_PLAN_2026_09_29.md` | Cross-area core edits announced 2026-10-01 (catalog columns `NOT NULL`, two register rows); DataCube's catalog generators deleted 2026-10-08 by the sixth line |
| Fourth, 2026-09-30: the Query app | `docs/QUERY_APP_DESIGN_2026_09_30.md` | v1 landed on `main` 2026-10-01; one query store in a package of its own announced the same day |
| Fourth, 2026-10-03: one owner for every database decision (the database owner) | `docs/PLAN_EXECUTION_SPLIT_AND_DATABASE_OWNER_2026_10_03.md` (being folded into `docs/architecture/`; history); `docs/EXECUTION_PLAN_BOUNDARY_2026_10_05.md` | The execution plan's step 3, the runner, landed 2026-10-10 (`a2e879a8e`); the output layer (PARK-24) closed 2026-10-18 by the thirteenth line; the writes line announced write plans in its files 2026-10-20 |
| Fifth, 2026-10-03: Studio, SDLC-lite and Depot-lite | `docs/STUDIO_DESIGN_2026_10_02.md`; `docs/STUDIO_FULL_PLAN_2026_10_04.md`; `docs/PROTOCOL_PROGRAM_2026_10_05.md` | Legs 6 and 7 landed 2026-10-10, the protocol program's step 1 and part of step 2 on 2026-10-11; on 2026-10-21 the compiler's missing reference checks (what leg 8 had shortcut) were built on `reference-checks` and squashed onto `studio-program`, then an independent audit's fixes the same way; no later landing recorded here |
| Sixth, 2026-10-07: DataCube + Python | `docs/DATACUBE_MODEL_DESIGN_2026_10_10.md`; `docs/DATACUBE_ONE_API_PLAN_2026_10_21.md`; `docs/DATACUBE_BI_EXCEL_PARITY_2026_10_24.md` (history) | PR #47 merged 2026-10-18 (`c401fe3f`); PR #57 open 2026-10-18; the one API program from 2026-10-21; BI and Excel parity from 2026-10-24 (branch `claude/datacube-bi-parity`); the timeseries and Deephaven lines' shared files noted 2026-10-27 |
| Seventh, 2026-10-11: Postgres SQL in Pure, the `#SQL{...}#` island | `docs/LEGEND_SQL_STUDY_2026_10_11.md` | PRs #28 (2026-10-12), #35 (2026-10-13) and #41 (2026-10-15) merged; #46 closed unmerged 2026-10-16; the corpus program (SELECTs over the compatibility and real-world corpora) announced 2026-10-17 |
| Eighth, 2026-10-11: Databricks, Snowflake and BigQuery | `docs/CLOUD_BACKENDS_PCT_2026_10_11.md` | PR #34 landed 2026-10-13 (`96e49c0`); next was the corpus harness on any database |
| Ninth, 2026-10-13: the dialect consolidation program | `docs/DIALECT_LEAK_CENSUS_AND_PLAN_2026_10_12.md` (being folded into `docs/architecture/`; history) | Phases 2 to 4 landed 2026-10-16 (#45 `4fb892f`, #49 `59b547a`); Phase 5 announced 2026-10-16, Phase 6's part 2 on 2026-10-17; the D11 move done 2026-10-18 (#56) |
| Tenth, 2026-10-13: SAP IQ SQL in Pure, the `#IQ{...}#` island | `docs/IQ_SQL_STUDY_2026_10_13.md` | Landings 1 and 2 landed 2026-10-14 (#37 `75183d68e`, #40 `f2e7b553d`); landing 5's cross-area edits announced 2026-10-17 and reviewed 2026-10-18 |
| Eleventh, 2026-10-15: Cypher in Pure, the `#Cypher{...}#` island | `docs/CYPHER_STUDY_2026_10_15.md` | PR1a, PR1b, PR2 and the temporal slice landed on its branch 2026-10-15 and 2026-10-16 (PR #48); `main` merged into the branch 2026-10-16 |
| Twelfth, 2026-10-16: MongoDB as a first-class database | `docs/MONGODB_PLAN_2026_10_16.md` (§11.1 the record of what landed) | v0 landed on its branch 2026-10-17; the PCT lane on the embedded MongoDB, channel A through the plan path, pinned at its first measurement the same day (1,249 tests, 678 pass) |
| Thirteenth, 2026-10-17: everything executes through a plan, then the result boundary and the seams | `docs/PLAN_PATH_PROGRAM_2026_10_17.md` (being folded into `docs/PLAN_AND_RESULT_MODEL_2026_10_18.md`; history); `docs/PLAN_AND_RESULT_MODEL_2026_10_18.md`; `docs/RESULT_BOUNDARY_PROGRAM_2026_10_26.md`; `docs/PLANNER_DRIVER_SEAM_PROGRAM_2026_10_30.md` | The result boundary program merged as PR #80 on 2026-10-30 (`7bc0797d`); the java.sql revamp and the seam's steps A to C built 2026-10-30 (PR #82); the planner-driver seam planned 2026-10-30 (PR #83, above); the product path's speed-up 2026-11-01 (branch `claude/product-path-perf`, PR #85) |

**Parked:** the compiler rebuild (`docs/EXECUTION_PLAN_2026_09_26.md`, history once folded), superseded by the Rust
port; the build rebuild and the self-contained bump, stalled after 2026-10-10 (`docs/build-inventory/program/START_HERE.md`);
the server program (`docs/SERVER_PROGRAM_2026_09_26.md`, being folded into `docs/architecture/`; history); NLQ (the
untracked `nlq/` directory is not ours; leave it).

**Shared machine:** at most two heavy Bazel/JVM jobs at once; before a full `bazel test //...`, check that another
session is not running one (a browser test, `//studio:verify_test`, times out under two full chains).

## Rules between sessions

1. Never force-push; never bare `git stash` (the stash stack is shared by every worktree).
2. Before pushing, `git fetch origin`; `main` must fast-forward.
3. The gate before a push is `bazel test --lockfile_mode=error //gates:local`, then CI on every platform
   (`AGENTS.md`, "Pushing to main"); `//parser-equivalence:diagnostics` and `//parser-equivalence:diagnostics_reports`
   run only on their triggers (a pin bump, a parser/lexer/protocol change, a corpus manifest change).
4. A timing is a lane run alone with `--nocache_test_results`, load under 3 at the start, nothing else building.
5. If a second line of work starts again, this file becomes the handshake again: each side's owned area, cross-area edits
   announced one line each before they land, and dated status lines.

> Cut in the 2026-11 docs cleanup: the active lines' order of 2026-10-04 with every announcement and landing of the
> Bazel program, the build rebuild, Studio, the database owner and DataCube + Python, and the thirteen lines' sections
> with all their announcements and landings (2026-09-29 to 2026-11-01), including the compiler line's landings filed
> under the rules. Full text: `git show de2745351:docs/IN_FLIGHT.md`.
