# Start here: the build rebuild and the self-contained bump (for a session picking the program up)

**Status (2026-11): stalled; the Rust port is now the work.** The build rebuild stopped after its state of
2026-10-10. Landed: Phases 0 to 2, the CI landing L1 (2026-10-07), Phase 3 (L2, 2026-10-09), Phase 3b (L3) and L7
"resolve once" (both 2026-10-10). Never started: Phases 4 to 6 (L4 to L6; Phase 6's homework was measured on
2026-10-11, nothing of it coded), the bump (Phase 7, L8), the typer's order (L9) and Phase 8. The work now is the Rust
port (`docs/IN_FLIGHT.md`, "Now"). What stays true here: the program's goal and north star (§1), the way of working
and landing (§4), the exact checks (§5) and the traps (§6).

Written 2026-10-07, corrected the same day after a cold read (`COLD_READ_2026_10_07.md`, git history:
docs/HISTORY.md). Read this first, then the plan (`docs/REBUILD_PROGRAM_2026_10_06.md`). The plan says what and why;
the phase briefs that said exactly how were removed in the 2026-11 cleanup (`docs/HISTORY.md`).

Do not confuse this program, **the build rebuild**, with **the compiler rebuild**: the parked compiler plan
(`docs/EXECUTION_PLAN_2026_09_26.md`; git history once folded: docs/HISTORY.md). Do not follow that plan's §0
checklist: its worktree and branch belong to another line of work.

## 1. The program in brief

legend-lite reimplements FINOS legend-pure and legend-engine. The program has four parts, in this order, each built
on the one before. **The plan's §0 has the whole picture, in the user's own words; read it first.**
1. **A first-class Bazel build, starting with a fast compile of only the product.** Bazel knows every step, what it
   reads and what it makes; nothing outside Bazel (shell scripts, Java or Python programs, Node) drives the work;
   everything pinned, nothing read from the machine, the same on Linux, macOS and Windows. The first deliverable is
   done (Phase 0): a clean compile of all the Java we ship in 12.4 s, with a guard that the compile targets only
   compile. What is left of this part (CI's build lane, Node, scripts, checks) is Phase 8.
2. **The bump as a standalone piece** (Phases 1 to 7, the main body): moving to a new upstream release runs only when
   we move the pins and reads only upstream's files. Today, files mix upstream facts with our decisions, generators
   read our own code, and the compiler decides some calls by name. So this part restructures how the product boots and
   fixes the compiler: one implementation table keyed by function id, overloads picked exactly as legend-pure picks
   them, the default world generated from upstream alone, Pure.java as rows.
3. **Our own generators, separate from the bump** (Phases 1 and 7, then Phase 8): generators made from our own code run
   when that code changes; measurements of our engine that look like generators become tests.
4. **The tests, untangled, after the bump** (Phase 8): core, the relational corpus, the stress corpus, the PCT
   flavors, parser equivalence, the browser and UI tests. Each depends on exactly the code it exercises, each CI lane is
   a `//gates` suite, and none runs on Node.

After the bump lands: the compiler's debts (`docs/PARKED_WORK_LEDGER.md` PARK-5 to PARK-14), each fixed with a design
agreed first.

The north star, in the plan's section 1: three kinds of files, never mixed. **Upstream** (the pinned archives; only
the bump changes them), **ours** (hand-written decisions and code: the implementation table, legend-lite's own
`meta::legend::lite` declarations, the system metamodel), **generated** (made from upstream and one setting, the
module choice, only by the bump; committed and sealed). Never a generator reading our code, never a hand edit of a
generated file, never a signature we typed ourselves.

## 2. Where everything is

> Cut in the 2026-11 docs cleanup: the table of where each document, branch and landing was and whether it was
> pushed (2026-10-07 to 2026-10-10). Full text: `git show de2745351:docs/build-inventory/program/START_HERE.md`.

| What | Where |
|---|---|
| The pinned upstream sources | Bazel repositories `@legend_engine_src` and `@legend_pure_src` (`$(bazel info output_base)/external/+http_archive+legend_engine_src`, `...legend_pure_src`; use `find -L` inside them); release pins in `release.MODULE.bazel` (engine 4.145.0, pure 5.99.0) |

## 3. State, and the next action

> Cut in the 2026-11 docs cleanup: the dated state blocks of 2026-10-07, 2026-10-09 and 2026-10-10 (each landing
> with its commit and run, the briefs' table, the decisions of 2026-10-07, the open decisions waiting for the
> user). The program's status is at the top of this file. Full text: `git show de2745351:docs/build-inventory/program/START_HERE.md`.

## 4. How we work (the user's rules; each one was learned the hard way)

**Before code**
- Plan each phase with the user and get agreement before writing code. Explain plainly: short sentences, no jargon,
  labels explained or dropped, one settled answer.
- Find the root cause before fixing anything; agree the design when it is not obvious. Never fix a slowdown with a
  cache first: fix the algorithm, and add a cache only if still needed, keyed on content.
- Never invent a new mechanism when an existing one (the implementation table, modules, manifests, the generators,
  the ledger) does the job.
- No hacks, no workarounds, nothing pushed around quietly. A shortcut that must stay is a row in
  `docs/PARKED_WORK_LEDGER.md` (date, who decided, why, what we do today, cost, acceptance, an anchor test), and it
  leaves only by being fixed (or, where the row allows keeping a behavior, by a `docs/SEMANTICS_REGISTER.md` row that
  replaces it). If a guard test rejects a change, the guard is usually right: stop and rethink.
- Never write bare "native": "upstream native" is upstream's `native function` keyword (a Java body); "platform-lowered"
  is what our Pure.java declares.

**While coding**
- Core edits: the `docs/IN_FLIGHT.md` announcement lands on main first, listing every core file the change touches
  (standing authorization to push IN_FLIGHT updates to main). No worktree sits on main: make one for the edit,
  `git -C runs/build-rebuild worktree add --detach runs/inflight origin/main` (inside a worktree's ignored `runs/`),
  edit and commit there with `[skip ci]`, `git -C runs/build-rebuild/runs/inflight push origin HEAD:refs/heads/main`,
  then remove the worktree.
- Never hand-edit a generated file; regenerate it with its narrow writer: `bazel run //core:update_generated` (DynaFn.java,
  Pure.java, CoreImports.java, engine-handlers.tsv, native-claims.tsv, prelude.pure), or the phase's own writer. The
  root `bazel run //:update_generated` also rewrites the ratchets, the ladder and other measurements (`BUILD.bazel`):
  if you use it, read `git diff` and give every moved ratchet, golden or ladder line its own reason.
- Any moved pin, ceiling, ratchet or allowlist entry carries a dated justification naming the task.
- No local paths (home or temp directories) or binaries in anything committed. Temp files go in the worktree's
  `runs/`, never `/tmp`. Use `git -C <dir>` / `env -C <dir>`, never `cd <dir> &&`.
- Run only the targets the change touches during the work. At most two heavy Bazel jobs on the machine at once
  (`docs/IN_FLIGHT.md`, `gates/BUILD.bazel`'s header): check other sessions first (`ps` for Bazel servers and test
  JVMs); do not stack worktrees or agents that run Bazel.

**Landing (no PRs, since 2026-10-06)**

Since 2026-10-26 `AGENTS.md`'s "Pushing to main" governs every landing: no push to `main` without the user's word in
that session, and a landing of more than one change goes through a PR. The steps below are the build rebuild's own.
1. An audit by an independent agent (`auditor`; `auditor-max` when the user asks for maximum effort).
2. Fix what it finds; rerun the checks the fixes touch.
3. Write what moved and why, with the numbers, in the commit message; a moved pin, roster or ratchet also gets its
   line in `docs/GATES.md`'s "Recent changes" (since the 2026-11 cleanup; `docs/GATES.md`, "Recording a moved pin,
   roster or ratchet").
4. Anything the change cites must be on main (the program's documents are, since 2026-10-07). A docs-only commit to main
   carries `[skip ci]` when it holds any file that is not `.md` (CI skips only Markdown and `progress*.txt`).
5. Push IN_FLIGHT's entry to main if it is not there yet (above).
6. Rebase onto the latest `origin/main`, then the local gate once: `bazel test --lockfile_mode=error //gates:local`. The
   commit message says "local gate: //gates:local green" (AGENTS.md, "Pushing to main", rule 1).
7. The tip commit message ends with `[skip ci]`: it stops main from running CI a second time when the commit lands
   (CI runs on pushes to main only; the branch run below is the verdict). Push the branch.
8. Dispatch one full CI run on the branch: `gh workflow run gate.yml --ref <branch> -f gates= -f platforms=all`
   (`gates=` empty means every lane; a run took about 22 to 31 minutes after L1, against 40 to 57 before it:
   `docs/GATES.md`, "The CI lanes and why"). For a throwaway run of affected lanes only, name
   them in `-f gates=...`; a full run is needed when `MODULE.bazel`, a toolchain, `.bazelrc` or a workflow changes.
   When the change touches the parser, lexer, protocol, parser-equivalence or `MODULE.bazel`, also
   `gh workflow run diagnostics.yml --ref <branch>` (its own trigger is a push to main, which `[skip ci]` suppresses).
9. When it is green, push that exact commit to main: `git push origin "${SHA}:refs/heads/main"` (the braces are
   required in zsh). If main moved meanwhile: docs-only commits, rebase and push; code commits, rerun CI.
10. No CI runs for the push itself (`[skip ci]`). Check the next nightly run on main
    (`gh run list --workflow gate.yml --event schedule`; cron 06:23 UTC); if it is red because of your commit, revert it at once (AGENTS.md, "Pushing to main", rule 3).
11. Commit trailers as the session's instructions give them.

## 5. How to run each check (exact)

| Check | Command | What "pass" means |
|---|---|---|
| Local gate | `bazel test --lockfile_mode=error //gates:local` | every test passes (290 targets; it does not include PCT or the corpus passes) |
| Core unit tests | `bazel test //core:core_tests` (24 per-package targets) | all pass |
| Guards (incl. the ledger anchors, identity counts, error shapes) | `bazel test //core:guardrails //core:census` | all pass; a shrink-only count that grows is a real finding, not a pin to raise |
| Spec tests (implementation table, ratchets) | `bazel test //spec:spec_tests` | all pass |
| Generated files current | `bazel test //:generated` | every diff test passes |
| PCT (heavy, 4 GB each) | `bazel test //pct:pct_duckdb //pct:pct_h2 //pct:pct_postgres //pct:pct_channel_b` (add `--local_test_jobs=2` when the machine is shared) | identical to before the change (17 targets) |
| **The six corpus passes** | `bazel build //spec:judge_host_duckdb //spec:judge_database_duckdb //spec:judge_host_h2 //spec:judge_database_h2 //spec:judge_host_warehouse //spec:judge_database_warehouse` | every result file identical to a baseline built BEFORE the change (below) |
| **The reference lane** (manual, about 8 GB) | `bazel build //spec:reference_lane_report`, then `diff bazel-bin/spec/reference-lane/core_relational.txt spec/src/test/resources/reference-lane/core_relational.txt`; re-bless a deliberate move with `bazel run //spec:update_reference_lane`; `bazel test //spec:reference_lane` checks every disagreement class has a reason in `reasons.tsv` | AGREE not down, no new disagreement class without a reason; every moved line explained in the commit message, with its line in `docs/GATES.md`'s "Recent changes" |

**The corpus passes, done right.** `//gates:local`'s corpus checks only compare committed results; they do not rerun
the corpus. A compiler change must rerun the six passes and compare them with a baseline:
1. Before changing anything, build the six targets and copy each output directory (`bazel-bin/spec/judge_<pass>/`)
   aside, e.g. to `runs/homework/<phase>/judges_base/judge_<pass>/`.
2. After the change, build them again (check the outputs' times: a cached result is not a rerun).
3. Compare the RESULT files, not the logs: the rosters (`*-fail-roster.txt`, `*-skipped-roster.txt`), the registers
   (`*-engine-order-register.txt`, `*-unordered-register.txt`, `*-database-engine-order-register.txt`), the ledgers
   (`judge-host.tsv`, `judge-database.tsv`) and `verdict.txt`, 22 files in all. Compare their sorted lines without
   comment lines (`evidence/phase3/compare_judges.py <baseline dir> bazel-bin/spec` does it). Logs (`host.log`, `database.log`) differ by timings; ignore them. (The SQL the executor sends,
   seen only with `-Dlegend.diagnostics=dump-sql`, may renumber the reflection rows' function ids when the world
   gains functions; that is not in the result files.)
4. A database pass refuses to run after its host pass fails, so an empty database output means look at the host pass.
5. A difference is fixed, or explained to the user, before landing.

Running a hand-made corpus command after another Bazel command: rebuild the corpus targets first (cached), or the
execution root lacks the upstream trees ("legend-engine checkout not present").

> Cut in the 2026-11 docs cleanup: how world changes (Phases 4 and 6, never started) ran the manifest-world
> experiment harness. Full text: `git show de2745351:docs/build-inventory/program/START_HERE.md`.

## 6. Traps already hit (add to this list)

- The reference lane's census walk does not follow the execution root's symlinks: point `-Dlegend.engine.root` and
  `-Dlegend.pure.root` at the real directories (realpath) when running its programs by hand. Inside the upstream
  trees use `find -L` (a plain `find` finds nothing).
- `jfr print` keeps only 5 frames per stack by default; use `--stack-depth 200` to see who calls a hot method.
- macOS `strings` fails on class files (it reads `CAFEBABE` as a fat Mach-O); use `grep -a`.
- zsh: `echo ====` fails (`=` expansion); an unquoted `$VAR` holding several paths is one word; `"${SHA}:refs/..."`
  needs the braces, and `"$c:spec/x"` is read as a modifier too.
- Segmenting upstream Pure files: a doc string belongs to the element below it; keywords at a line start inside a doc
  string or block comment are prose; names come after every `<<stereotype>>` and `{tagged value}` (`docstart.py`).
- Upstream marks tests with test stereotypes and `::tests::` packages; `::test::` packages are upstream's test
  infrastructure, which ordinary code references.
- An ownership filter must cover the functions Pure.java implements, `CoreFn`'s forms, the system metamodel's own
  versions, `TdsLegacy`'s functions and the helpers forms recognize (`agg`, `col`): missing one breaks hundreds of
  tests.
- `bazel info` blocks while another build runs in the same workspace: run `bazel info output_base` once at session
  start, before any build, and keep the path for the session (never commit it).
- `e6_lanes.py` forces `-Xmx4g` on every pass (the DuckDB and warehouse passes really run with 1 GB) and names the
  configuration directory `darwin_arm64-fastbuild`: adjust both before trusting a heap or another platform.
- The warehouse corpus passes build the GraalVM server first: slow on a cold cache.
- A guard's growth is a finding: do not raise a shrink-only pin to make a change pass.
- **An action's command line run outside Bazel fails with `NoClassDefFoundError`** (2026-10-10, the eager-compile
  timing): Bazel 9 builds without the bytes, so jars that hit the disk cache are not in the execution root. Build the
  target with `--remote_download_all` first (`evidence/l7/eager_ab.sh`, git history: docs/HISTORY.md).
- **A tree swap for an A/B run keeps the branch's new files** (2026-10-10, L7 steps 3-4): `git checkout <ref> -- <dir>`
  never deletes, so main's side fails to build on a file only the branch has; delete the branch-only files after the
  main-side checkout and restore with `git checkout HEAD -- <dir>` (`evidence/l7/eager_ab.sh`, git history).
- **A cloud session's Bazel fails on `Error downloading [https://github.com/...] ... 403 Forbidden`** (2026-10-14):
  the session's proxy refuses GitHub's archive downloads (legend-engine, legend-pure, the bats helpers) for any
  repository not attached to it. Run `tools/cloud-distdir.sh` from the repository root once per container: it seeds a
  `--distdir` from Bazel's mirror or a git clone packed as GitHub packs it, and Bazel still checks every pinned
  checksum (`docs/CLOUD_SESSIONS.md`, with the cloud container's other traps).

## 7. Reading order

1. This file. 2. The plan: sections 0 (what the whole program is for) to 2 (the user's decisions). 3.
`docs/PARKED_WORK_LEDGER.md` (PARK-5 to PARK-14). 4. The brief of the next phase (removed with the other briefs:
`docs/HISTORY.md`). 5. `docs/IN_FLIGHT.md` on main, to see who else is working.
