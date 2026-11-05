# Every generator: what it is, when it runs, and the bump (2026-10-05)

Status: AGREED SHAPE (the user, 2026-10-05); what was built of it is section 6, and the build rebuild that carried it
stalled after 2026-10-10 (`docs/REBUILD_PROGRAM_2026_10_06.md`). On 2026-10-06 the full design of the rebuild's
Phases 3 to 5 replaced section 2's "basic split now" for items 5 to 8. The evidence, a dossier per generator (G1 to
G6, the 57 generators' list), was `docs/build-inventory/generators/` (git history: docs/HISTORY.md).

> Cut in the 2026-11 docs cleanup: the status and update notes of 2026-10-06 and 2026-10-08 and the evidence folder's
> description. Full text: `git show de2745351:docs/GENERATORS.md`.

## 1. The rules

1. **A generator is placed on two axes.**
   - **What it is** (by where its inputs come from): upstream, ours, a measurement of our engine, a build output, or
     dead.
   - **When it runs:** automatically, or on demand (`manual`).
2. **Upstream-derived content is generated from the pinned checkouts alone, and only the bump generates it.** This is
   the compiler design's tenets 4 and 5 (`docs/architecture/frontend.md` §2; the design itself is in `docs/HISTORY.md`): "nothing about upstream is typed by hand", and "everything the
   platform decides for itself is a registration, in one registry". Our decisions never go into an upstream record
   and are never an upstream generator's input.
3. **The bump is self-contained.** Every upstream generator and report runs only inside `bazel run //tools/bump`.
   Nothing in the everyday build, the local gate or CI runs them, and nothing there needs the upstream archives
   except tests that use them (PCT, parser-equivalence).
4. **A generated file is generated whole.** Nobody edits it. Hand-written content lives in its own files, and what
   people edit are inputs (native membership, DynaFn resolutions, the Lite extensions).
5. **Our own generators run when our code changes**, never in the bump.

## 2. Upstream: the bump's records and reports

All are committed and regenerated only by `//:update_upstream` inside the bump. Each must be deterministic: two runs,
same bytes.

| # | Generator | Writes | What it is | Reads | Status |
|---|---|---|---|---|---|
| 1 | `//parser-equivalence:gen_fixtures` | engine-grammar-fixtures.jsonl | every grammar example from upstream's own test jars (parity corpus tier C6); read by gate 8, the manifest, the roster, the censuses | upstream only | clean; narrow (it reaches all of core today) |
| 2 | `//parser-equivalence:gen_manifest` | corpus-manifest.tsv | a sha256 of every upstream corpus source; a record read as a diff | upstream only | clean; kept as a bump record (the user) |
| 3 | `//tools/engine-runner:vocab` | vocab.tsv | every literal token upstream's grammar lexers know | upstream jars only | clean; kept as a bump record (the user); leaves keyword_coverage's tool data |
| 4 | `//spec:gen_imports` | CORE_IMPORTS | the default import list in the engine's order, from `CompileContext.META_IMPORTS` | upstream, plus its host file | split: moves to its own generated file, then clean |
| 5 | `//spec:gen_natives` | Pure.java membership signatures | upstream's exact signature text for the natives we implement | upstream, our membership list, our parser | split now; full catalog later |
| 6 | `//spec:gen_dynafn` | DynaFn.java rows | upstream's dynafunctions: name, dialects, inference | upstream, plus our resolutions read back from the file | split now; registry rows later |
| 7 | `//spec:gen_engine_handlers` | engine-handlers.tsv | upstream's handler surface (bare-name resolution for engine input) | upstream, our committed Pure.java and prelude | upstream-only later |
| 8 | `//spec:gen_prelude` | prelude.pure | the platform declarations loaded at boot | upstream, a scan of our core Java, Pure.java's hand shapes, the claims | mostly ours today; shrinks to "what Java needs at boot" in the compiler rebuild |
| 9 | `//tools/reference:ref_imports` | (report) | each source's import groups, as legend-pure's compiler uses them | upstream jars only | becomes a committed bump report; prove determinism |
| 10 | `//parser-equivalence:pmcd_reachability_census` | (report) | which upstream protocol classes are reachable from a model | upstream jars, plus our roster | commit the upstream half (reachability); the "uncovered" worklist becomes an on-demand report |
| 11 | `//spec:native_declarations` | (report) | upstream declarations of natives | upstream, plus our membership list | widen to every upstream native (the catalog); then commit |

**Items 5 to 8 (option (a), the user's decision): the basic split now.**
- Pure.java keeps its hand-written parts: the Lite extensions, legend-lite's own natives, the hand code. The
  membership signatures and overload groups move to their own fully generated class.
- DynaFn's resolutions move to a hand-owned decisions file; DynaFn.java is generated whole.
- CORE_IMPORTS gets its own generated file.
- The full upstream catalog, the one registry and the prelude shrink are compiler-rebuild work (W2.1, "the one
  registry"). Until then, items 5, 6 and 8 keep one hand-owned input each, and they regenerate in the bump.

Research behind the split (git history):
- Nobody has hand-edited a generated part. Pure.java's generated text changed only in the 4 commits that introduced
  its generator (2026-09-11).
- The hand edits are legend-lite's extensions (9 commits on Lite), DynaFn resolution decisions (1 commit, 10
  decisions), and ordinary resolver development (NameResolver.java, 93 commits).
- The problem is placement: every hand edit is an input to a generator that splices into the same file.

**Moves with a bump by itself (no bump step):** `//pct:adapter_par` (our adapter compiled against upstream; fix its
clock-stamped jar), and `//tools/reference:ref_dump`.

## 3. The bump: `bazel run //tools/bump -- <engine release>`

| Step | Today | Agreed |
|---|---|---|
| 1. Decide | release published on Central; pure release, tag commits and managed versions read from it | unchanged |
| 2. Move | rewrite release.MODULE.bazel's pins; re-pin maven_upstream and maven_runner | unchanged |
| 3. Regenerate | `//:update_generated`, ONCE: all 55 writers, ours and the ratchets, ladder and roster included. It re-blesses expected results silently, and one run is not a fixed point. | `//:update_upstream`: the upstream records only (items 1 to 8), checked to be a fixed point (a second run changes nothing) |
| 4. Reports | none (people must remember items 9 to 11) | items 9 to 11, committed |
| 5. Seal | none | write `upstream.seal`: the release, plus the sha256 of every bump record and report |
| 6. Check | `bazel test //...` (after step 3 already re-blessed things) | `bazel test //...`, nothing re-blessed first: every expected result that moved is a red diff, listed together |
| 7. Judge | a person re-pins ratchets and commits | a person reads the record diffs and reports, re-blesses each moved expected result on purpose with its reason, and commits |

**Every day, the seal check:**
- One test reads the committed bump records and `upstream.seal`, and fails if any record's hash differs. It takes
  milliseconds and needs no upstream and no generator.
- It catches a hand edit to a generated record, or one changed outside a bump. Its message says "generated by the
  bump; run `bazel run //tools/bump`".
- Whether a record is current with upstream can only change when upstream does, which is only ever a bump.
- So the bump generators never run in the everyday gate, even on a cold cache.

## 4. Ours: committed, generated from our own code (they run when that code changes)

| Generator | Writes | True trigger | Narrow to |
|---|---|---|---|
| `//scripts/corpus:gen_dense` | stress 59, 60, 64 | the dense builder, hand-written stress files, linked projects | its 14 modules; drop queries.pure (never read) |
| `//scripts/corpus:gen_stress` | stress 92 to 98 | build.py, queries.pure, stress files, gen_dense output | its 27 modules |
| `//core:stress_layout` | stress-layout.json | core/stress.bzl | (written at analysis time) |
| `//datacube:offer_facts` | offer-facts.ts (shipped) | our compiler, DataCube's query builder | the `//core:planner` closure |
| `//datacube:test_imports` | test_imports.bzl | import lines of DataCube's src and test | the files it reads, not 122 npm files |
| `//engine-client:lite_facts` | lite-facts.ts (shipped) | `compiler/element/type` | `//core:compiler_element_type` |
| `//engine-client:lite_facts_py` | _type_facts.py (the wheel's) | `compiler/element/type` | `//core:compiler_element_type` |
| `//legend-art:icons_gen` | icons.ts (shipped) | our `icons.mjs` table; rarely the react-icons pin | (not the bump) |
| `//warehouse:reachability_metadata` | the native image's metadata | Duck.java, AuthenticatedUser.java, the services table | a small library on `:server_lib` |

These stay in `//:update_generated` (run by people), with their diff tests in the everyday gate.

## 5. The rest

**Measurements of our engine (SET ASIDE by the user, 2026-10-05; unchanged for now):**
- the corpus judges (6): `//spec:judge_host_duckdb`, `judge_database_duckdb`, `judge_host_h2`, `judge_database_h2`,
  `judge_host_warehouse`, `judge_database_warehouse`;
- `//core:ladder_report`;
- the spec, parser-equivalence and PCT ratchets;
- `//fixtures/saved-queries:gen` (its records are ours; its row counts are a test);
- `//spec:reference_lane_report`;
- the coverage measurements `//parser-equivalence:gen_roster` and `//scripts/parser:keyword_coverage`;
- on demand: `corpus_census`, `grammar_keyword_census`, `eager_corpus_compile`, and `gen_own_corpus_draft` (it
  drafts gate 8's expected-differences ledger).

**Build outputs, never committed (testonly where only tests read them):**
- for the product: `//warehouse:duckdb_library` and `postgres_extension` (they rerun only on their pins), and
  `//datacube:dist` (replaced by D10's shared site rule; broken on this branch, fix `dab833263` on the held
  `bazel/exec`);
- for tests: `gen_differential` (drop the 7 stress files it never reads), `stress_index`, `offer_queries`,
  `cube_queries`, `cube_jvm_answers`, `jvm_answers`, `zone_jvm` (narrowed to `LiteralSpelling`), `ref_dump`, `pins`.

**Our tools, on demand (`manual`):**
- `//spec:native_membership_draft` (with `//core:draft_native_membership`): drafts our membership list;
- `//datacube:link_dictionary_next`: cuts the next share-link version.

**Dead (delete):**
- `//parser-equivalence:migration_sizing`: the parsers it measured were deleted (`b23f68757`);
- `//spec:eager_corpus_compile_world2`;
- `//spec:gen_claims` with `core_next` and its writer, if D2 retires native-claims.tsv. Nothing reads that file:
  ClaimRegistryTest names its path and never opens it.

## 6. The work, in order: what was built

- **Built in the rebuild's Phase 1** (PR #26, `96bf6ae5d`, 2026-10-06): steps 1 to 3 (the dead deleted, the on-demand
  tools and reports `manual`, the build outputs `testonly`, every generator of ours narrowed to what it reads), and the
  small upstream generators made upstream-only: the fixtures, the manifest, vocab, CORE_IMPORTS in its own file
  generated from `CompileContext` alone, the reachability census split into its committed upstream half and an
  on-demand worklist. `ref_imports` was deleted rather than committed.
- **Built in Phase 2** (`ff70aef01`): DynaFn and the engine handlers generated whole from upstream, our decisions joined
  when the classes load.
- **Step 4's split** for Pure.java was replaced by the full design: Phase 3 (one table decides, by exact function id)
  landed 2026-10-09; Phase 5 (Pure.java as rows keyed by function id) never started.
- **Never started:** the rest of step 5 (`native_declarations` widened to every upstream native), step 6, the
  self-contained bump (Phase 7: there is no `//:update_upstream` and no `upstream.seal`; today's `bazel run //tools/bump`
  is section 3's "Today" column), and step 7's wiring guard.

D2 was ruled on 2026-10-06: native-claims.tsv, `core_next` and `gen_claims` retire in Phase 5
(`docs/REBUILD_PROGRAM_2026_10_06.md` §2, decision 5). The measurement group stays set aside.

> Cut in the 2026-11 docs cleanup: the seven-step work list of 2026-10-05. Full text: `git show de2745351:docs/GENERATORS.md`.

## 7. The default world: what a program without a manifest gets

A user's program (Studio, DataCube, Query, the server) has no manifest. What it boots on is decided by upstream, not
by a list of ours. Decided on 2026-10-06 by measurement, the way of making it ruled by the user on 2026-10-10; not
built (the rebuild's Phase 4 never started, so today's `prelude.pure` is still `PreludeGenerator`'s).

The default world is:
1. **Upstream's own core, whole:** legend-pure `platform*` plus the engine's `core_functions_*`, the modules
   `PureRuntime.loadAndCompileCore` compiles. Tests are stripped by upstream's own markers only: test stereotypes and
   `::tests::` packages. `::test::` packages hold upstream's test infrastructure, which ordinary code references, and
   stay.
2. **Upstream's own query surface:** every function the engine's core compiler and relational extension register as a
   handler (`Handlers.java`, `CoreCompilerExtension`, `RelationalCompilerExtension`: 455 functions) and every class
   those compiler modules instantiate (156).
3. **Closed** over what those reference: through declarations always, and through bodies. The experiments followed
   only the bodies of functions whose upstream body runs; following declarations alone changed every corpus lane,
   since some upstream bodies need functions only a body reaches. The user's ruling of 2026-10-10 (D4-1 (a)) follows
   every body, so the closure never reads what we implement.
4. **Built in:** the 87 m3 metamodel declarations upstream has only in `m3.pure`'s bootstrap form, and lite's stub.

The only input of ours is a module-level choice: the core and relational compiler modules count, kept in the bump's
configuration beside the pins. There is no list of names, no pruning, no Java scan. Per the 2026-10-10 ruling the
bump's generator runs legend-pure's own compiler from the pinned jars over that module choice, so the world is a
function of the pins and the module choice alone; the compiled boot form is a build step. Functions the platform owns
(the implementation table's rows, the language forms, the system metamodel's own upstream-named elements) are filtered
out of upstream's world at boot; without that filter the boot failed or calls bound to upstream's declarations
instead of our forms.

**Measured** (seven experiments, each swapping `prelude.pure` on the classpath of an unmodified build; cold starts):

| World | Elements | JVM boot | Browser first answer | Projects' body walls | Corpus, 6 passes | PCT, 2,737 cases |
|---|---|---|---|---|---|---|
| the prelude of 2026-10-06 | 641 | 350 ms | 1,500 ms | 146 | baseline | baseline |
| upstream's core, its query surface, closed through runnable bodies | 929 | 424 ms | 2,088 ms | 0 | identical | identical |

- The 146 walls were one function called 146 times, `meta::pure::functions::lang::orElse`: in upstream's handler
  table, missing from the hand-made prelude.
- The cost: +74 ms JVM boot, +590 ms (+39%) to the browser's first answer, +0.33 MB of WebAssembly.
- Pure.java against upstream: of 837 overloads, 794 match an upstream signature id exactly, none diverge, and the 43
  others are all legend-lite's own `meta::legend::lite`. Rows keyed by function id can replace Pure.java's signature
  text one for one.
- The corpus on its real manifest (experiment 8: the 38 repositories' closure, 655 more files) keeps every roster
  and register on all six passes; each ledger gains one line (an already-failing test now reaches its assertion and
  fails there), which needs one register entry with its reason.

Sources: `docs/MANIFEST_WORLD_EXPERIMENTS_2026_10_06.md` §1 to §4 and §6; `docs/REBUILD_PROGRAM_2026_10_06.md` §1 and
§2, decision 6 (git history: docs/HISTORY.md).
