# Rust port — phase 3: the middle (2026-11-09)

Phase 3 of [`ROADMAP.md`](ROADMAP.md): the normalizer, compiler, resolver, lowering and planner move to Rust. The design
of its two hard questions and the owner's rulings are [`PHASE3_DESIGN.md`](PHASE3_DESIGN.md). This file says what has been
done, in the order of the design's §3.

## Order

0. **While Java exists: the identity hit-count oracle, and the `MirJson` dump for the corpora.**

   **Done 2026-11-09, no production Java changed.** A probe
   ([`rust/tools/identity-hits-probe.patch`](../../rust/tools/identity-hits-probe.patch), in the form of phase 0's: applied
   to a clean tree, run, reverted) counted, per question, the hits and misses of every identity table of the design's §1.1
   and the outcomes of its identity comparisons, and took the `MirJson` of every lowering. Frozen under
   [`rust/oracles/identity/`](../../rust/oracles/identity) with what each column means in its `MANIFEST.tsv`.

   - **The probe.** `com.legend.base.IdentityProbe` (reachable from every library that has a site), 108 edits to core:
     each site passes through a call that returns the value the site computed, so nothing is rewritten (`computeIfAbsent`,
     `merge`, `compute` stay as they are; where a hit cannot be read off the call it is counted beside it, by a `containsKey`
     on an `IdentityHashMap`, which calls no `equals`, or by the map's size). The patch being applied is the switch; there is
     no flag. `git apply`, `bazel test ... --nozip_undeclared_test_outputs`, read `identity-hits-*.tsv` and
     `identity-mir-*.jsonl` from each test's outputs and from each corpus pass's scratch directory
     (`bazel-bin/spec/judge_*_tmp`). `-Dlegend.identity.run=N` in `spec/corpus.bzl` is a no-op that makes the disk cache run
     a corpus pass again.

   - **A row is one question.** Its key is (kind, model, query, doors, use). A thread-local scope is entered at `TypedQuery`'s
     public methods, at `Execution.run` and `Execution.runProgram` (so `BodyDriver` counts into its test body's row), at
     `Compiler.lowerResolved`, `programFacts` and `compileAllBodies`, and a nested question stays in the outer one. Kinds:
     `B` the boot layer, `M` a model compile, `Q` a query, `L` below. The model id is a text's SHA-256 (`t:`) where the door
     has text, in `rust/oracles/front`'s form, so the texts are read from `front/questions.jsonl.zst`: every model and 31,427
     of the 31,508 query rows with text ids are there, the rest being `compileAllBodies`' constant and 31 newer texts. Where
     no text exists the id is what the door has: the source list (`s:`, the corpus and the stress suites), the runner's name
     for the question (`test:`, `fixture:`, `setup:`, `service:`), and, for 2,550 rows, the resolved spec's `toString` (`vs:`,
     stable, but no Rust runner can make it: those rows need a name before they are used). The doors called, the runtime, the
     dialect class and the judge pass are in the key, because the same body is another question on another pass.

   - **Model-tier tables.** A table named `model.*` (§1.1's sites on model records) can fire lazily inside a query, and on a
     shared context the first query to touch a class pays for it. So such a count joins the query's row only when that
     context served exactly that query (`use` = `solo`, 807 rows), and otherwise goes to one `L` row of the context, keyed by
     the hash of the queries that ran on it (8 rows: the corpus's four passes, the stress suite's two, two others). `Compiler.BOOT`'s one
     computation is its own row whichever test pays: 53 JVMs, 1 distinct `B` row.

   - **What was counted**, summed over the frozen rows (a row counts a question once however often a suite asks it):

     | Class | Table or site | Hit / true | Miss / false | Rows |
     |---|---|---:|---:|---:|
     | (a) memo | `SpecCompiler.memo` | 1,582,730 | 240,191 | 34,507 |
     | (a) memo | `Anchors.memo` | 134,771 | 5,305,268 | 48,778 |
     | (a) memo | `Anchors.spaceMemo` | 26,351 | 434,887 | 48,987 |
     | (a) memo | `ClassSources.routedSources` | 12,354 | 7,207 | 4,907 |
     | (a) memo | `ClassSources.perRoutes` | 12,354 | 7,207 | 4,907 |
     | (a) memo | `ContextReading.scoped` | 4,018,844 | 667,100 | 14,254 |
     | (a) memo | `ContextReading.timeZone.seen` | 3,834 | 2,247,049 | 19,647 |
     | (a′) side table | `whereZones[not-in-rust]` (ruling 3: marked, free to count) | 9,428 | 5,700 | 6,399 |
     | (a′) side table | `DateSplit.ctxStrips` | 36 | 3,768 | 36 |
     | (a′) side table | `DateSplit.dateRenames` | 226 | 3,542 | 36 |
     | (a′) side table | `StackBuilder.leaves` | 1,220 | 53,420 | 5,775 |
     | (a′) side table | `StoreResolver.aggReads` | 1,558 | 1,086,703 | 24,599 |
     | (a′) side table | `InnerDemand.inQueryReads` | 220 | 2,094 | 591 |
     | (a′) side table | `Lowerer.calCtx` | 882 | 20,882 | 7,532 |
     | (b) query tier | `TypedUserCall.equals.origin` | 0 | 0 | 0 |
     | (b) query tier | `Evaluation.equals.origin` | 0 | 804 | 12 |
     | (b) query tier | `Evaluation.Site.equals.origin` | 746,151 | 0 | 1,317 |
     | (b) query tier | `CallInstantiation.equals.frame` | 3,334 | 0 | 104 |
     | (b) query tier | `CallInstantiation.equals.origin` | 3,334 | 0 | 104 |
     | (b) query tier | `UserCallInliner.callInstantiations` | 3,334 | 1,266,951 | 13,472 |
     | (b) query tier | `SourceSubst.untrackedLetNodes` (read in `Evaluation.untrack`) | 4,400 | 2,452 | 237 |
     | (b) query tier | `ContextReading.lambdaBound` | 714,195 | 2,271,789 | 11,172 |
     | (b) query tier | `SyntheticHeads.valuesLambdas` | 144 | 143,771 | 23,239 |
     | (b) model tier | `model.ModelBuilder.indexed.*` (15 of its 16 arms fired) | 61,458 | 7,905 | 16,589 |
     | (b) model tier | `model.PureModelContext.CheckedLayer` | 6,385,510 | 173,890 | 2,728 |
     | (b) model tier | `model.DatabaseDefinition.defaultSchemaTables` | 141,942 | 23,336 | 3,487 |
     | (b) model tier | `model.DatabaseDefinition.defaultSchemaViews` | 163 | 31,007 | 199 |
     | (b) model tier | `model.TableIndex.inSchemas` | 2 | 1 | 3 |
     | (b) model tier | `model.LiftedViews.owners` | 112 | 0 | 40 |
     | (b) model tier | `model.LiftedViews.functions` | 393 | 112 | 40 |
     | (b) model tier | `model.LiftedViews.walls` | 0 | 112 | 40 |
     | (b) model tier | `model.LiftedViews.expanding` | 0 | 112 | 40 |
     | (b) model tier | `model.ViewSignatures.expanding` | 0 | 137 | 40 |
     | (b) model tier | `model.MappingDefinition.position` (per key sorted: the comparator's count is the JDK's) | 15,356 | 0 | 165 |
     | (b) model tier | `model.MappingClosures.first` | 0 | 2 | 2 |
     | (b) model tier | `model.MappingPrePass.selfEdge` | 11 | 0 | 2 |
     | (b) model tier | `model.AssociationSynthesis.self` | 66 | 64 | 19 |
     | (b) model tier | `model.ScanRelations.pm` | 24 | 24 | 4 |
     | (c) query tier | `UserCallInliner.reducedTerms` | 2,436,879 | 15,330,022 | 49,041 |
     | (c) query tier | `UserCallInliner.countReads` | 19 | 143 | 9 |
     | (c) query tier | `Typer.pretyped` | 2,594 | 101,565 | 176 |
     | (c) query tier | `Typer.withPretyped.shadowed` | 48 | 362 | 176 |
     | (c) query tier | `Overloads.pretyped` (registered or not: this table is built here and read as `Typer.pretyped`) | 410 | 812 | 244 |
     | (c) query tier | `InnerDemand.lambdas.seen` | 2,734 | 3,417 | 527 |
     | (c) model tier | `model.NameResolver.resolvedViews` | 7 | 105 | 40 |
     | (c) model tier | `model.ModelNormalizer.adopted` | 9 | 23 | 3 |
     | (c) model tier | `model.ModelBuilder.findAssociationAtClass` | 0 | 0 | 0 |
     | (e), for §1.1 | `ObjectSites.nodes` | 0 | 0 | 0 |

     Left out on purpose: class (d); class (e) but for `ObjectSites`; `protocol/sql/SqlToPure`'s five (phase 3 or the
     translators' wave, as §1.1 says); `judge/InstanceIds` (stays Java).

   - **What ran.** Everything through `Lean.run` or `Execution.run`, and more. `ObjectIdentityTest` asks 45 cases over one
     model text through `Compiler.query`, in `//core:core_tests_root`; the corpus lanes enumerate 2,613 tests, 108
     fixtures and a setup on each database (`MinimalCorpus`, `PureTestRunner`, named `test:` and `fixture:`). Frozen rows by
     set (a question two sets ask is one row, so these overlap):

     | Set | Rows |
     |---|---:|
     | `//spec:corpus_duckdb`, each of its host and database passes | 5,306 |
     | `//spec:corpus_h2`, each pass | 5,306 |
     | `//core:stress_suites` / `stress_suites_h2` | 9,443 / 4,723 |
     | `//core:core_tests_integration` / `_h2` / `_sqlite` | 2,976 / 2,970 / 2,937 |
     | `//core:iq_parity_test`, `cypher_parity_test`, `cypher_parity_native_test`, `sql_parity_test` | 8,503, 5,897, 5,917, 2,130 |
     | PCT: DuckDB's five lanes, `pct_h2`, channel B's six | 2,512, 491, 1,731 |
     | The rest of `//core/...` | 2,364 |
     | **All of it, distinct** | **66,994** (B 1, L 8, M 2,741, Q 64,244; 2,752 models) |

     *Left out, and why.* The six Postgres lanes (`iq_parity_postgres_test`, `cypher_parity_postgres_test`,
     `sql_parity_postgres_test`, `postgres_arm_test`, `sql_island_postgres_test`, `store_writes_postgres_test`) ran and
     passed, but their model text carries the server's port, so the model's id moves every run: no stable key, and their
     questions are the non-Postgres parity lanes' (with the model id set aside, their counts agreed between the runs too). The
     same goes for 96 keys of `core_tests_integration` (30 on each of its three lanes) and `core_tests_server` (6): a model
     holding a temporary path. PCT on
     SQLite, Postgres and the cloud lanes, and anything outside `//core`, `//spec` and `//pct`, were not run: the brief did not
     ask for them.

   - **Stable.** Two complete runs, A and B, of the same patched tree (the corpus passes forced to run again by the flag, the
     tests by `--nocache_test_results`), 147,657 rows each over 53 JVMs. Within a run no key has two vectors. Across the runs,
     of the 66,994 keys in both, **no count differs**. Only the Postgres-and-temp-path keys above (16,775) exist in one run
     and not the other, and those are not frozen. 72 lowering ids moved (one per row): each a corpus aggregation-aware test
     whose lowered `VALUES` holds the host's fresh `executionTraceID` UUID, 18 tests on each of the 4 passes; those ids are
     frozen as `~moved`, and the counts are. The first attempt had three keys with two vectors, from a label applied too
     widely (a test's name on the setups its first run prepared), fixed by naming the setups' own questions `setup:`; the two
     runs above are of the fixed patch. Nothing had to be run serially: a scope belongs to the thread that asks its question,
     and every shared memo is per query, boot's row or the `L` row.

   - **Counting changes nothing.** The patch also holds a tap at the JDBC seam (`base/SqlTap`, the seam's four sends: length
     and hash of every statement in order). A control run applied the tap alone (`git apply
     --include='*JdbcStatements.java' --include='*SqlTap.java'`). Against the probe's run, over 5,731,686 statements in 48
     processes, 42 processes' statements are identical in order, one (`core_tests_exec`) identical in another order (as it is
     between A and B), and the other five differ at exactly the positions where A differs from B (the UUID above, a temporary
     path). `//core:update_ladder_*` (the SQL pins) and the corpus rosters' `update_rcorpus_*` stayed green with the patch.
     The only test the patch fails is `//core:guardrails` (the probe's static collections, `model` and `lowering` reaching
     `base`'s new class, reflection), and the tap alone passes it.

   - **Frozen**: [`rust/oracles/identity/`](../../rust/oracles/identity): `hits.tsv.zst` 1.3 MB (20.6 MB of text),
     `mir-ids.tsv.zst` 2.4 MB, `mir-store.tsv.zst` 5.1 MB (816 MB of text), each with its SHA-256 in `MANIFEST.tsv`. Small
     enough that nothing went to the archive outside git.

   - **`MirJson`: what was missing, and what is frozen.** `rust/oracles/mir/*` (phase 0 step 4 and phase 1) and
     `//parser-equivalence:round_trip_proof` hold the seams, not the questions: a dialect's input and answer deduplicated
     by content, with no way back to the model and query, and the protocol JSON's round trip. "The corpora through a Rust CLI"
     needs, for a question, the trees Java lowered, so the probe takes `MirJson.query` (and `MirJson.dml`) of each
     `Lowerer.lower` and `lowerStoreWrite`, per row, in order: 146,594 lowerings, 39,266 distinct. `mir-ids.tsv.zst` is
     the question-to-ids map, `mir-store.tsv.zst` the text of each id. 2,774 lowerings have no text: `MirJson` refuses a
     tagged value or list (2,770 lists, 4 values; phase 1 notes the values) by design, and they are the row's
     `!IllegalStateException:...` entry, to be compared by the same refusal. The dump is of the lowering's own answer, before
     `PlanMaker`'s `EnumValueTables.apply` and `BodyDriver`'s post-processing.

   - **What §1.1 did not say, or said too strongly.**
     1. The (a′) column heading "the lookup must hit" is too strong for these tables: most lookups miss by design
        (`StoreResolver.aggReads` 1,086,703 of 1,088,261, `StackBuilder.leaves` 53,420 of 54,640, `Lowerer.calCtx`,
        `InnerDemand.inQueryReads`, `DateSplit.*`), the node simply having no such fact. §1.1's prose is right ("a miss changes
        behaviour") and that is the hazard: a lost entry looks like an absent fact.
     2. `SpecCompiler.memo` is not purely class (a): it is also `compileReachable`'s visited set, its cycle break
        (`SpecCompiler.java:135-147`). The port needs the identity, not only the speed.
     3. `TypedUserCall`'s `origin ==` was not reached by any question here (one unit test reaches it). `Evaluation.equals`'s
        was reached 804 times, all false. The one that works is `Evaluation.Site.equals` (746,151, all true): `ObjectSites.sites`
        keys. `ObjectSites.nodes` is confirmed dead on every set: no lookup, scoped or not.
     4. `ClassSources.routedSources` and `perRoutes` count the same in all 4,907 rows, so on these sets a routes list seen
        again always finds its inner key: one memo in effect.
     5. Never fired: `ModelBuilder.findAssociationAtClass`'s `!=` (it needs two injected ends), the `indexed` arm for a
        `LegacyMappingDefinition`. `TableIndex.inSchemas` fired 3 times. These sites' behaviour is not in the oracle.

   - **For the Rust side.** A Rust counter per name in the table, at the same sites, and a CLI that asks the question a row
     names. A row reproduces when the CLI compiles the model and asks exactly the doors of its `doors` column on a context with
     its `use`; the corpus rows need the sources and the test FQN, the `vs:` rows a name first. `whereZones` has no Rust table
     (ruling 3): its counts are for reading only.

   **Step 0 redone on the merged tree: done 2026-11-11, no production Java changed** ([`PHASE3_SHIM_DESIGN.md`](PHASE3_SHIM_DESIGN.md)
   §10, rulings 1 and 8; §5's steps 0 and 1). The first freeze above was made on the old tree, whose `driver` and `judge` `main`
   has since deleted; this redoes it at `1ea8fbf45` (core and `base` as of the merge of `main` at `c4a39e06d`, `9c08abbc7`) and
   adds the plan oracle. Everything above stands as the record of the first freeze; where the two differ, this entry is the
   oracle. The probe was applied to a clean tree, run and reverted with `git apply -R` of the same patch, never across a
   commit; the control likewise.

   - **The probe, retargeted** ([`rust/tools/identity-hits-probe.patch`](../../rust/tools/identity-hits-probe.patch), replaced:
     124 edits to 40 files of core, `spec/corpus.bzl`'s run flag, and `base`'s `IdentityProbe` and `SqlTap`). The first
     freeze's edits were re-checked by their exact text against the merged sources; 99 applied unchanged, and what moved with
     `main`:
     1. *The doors.* `TypedQuery.lower` is the product statement road now (`ProductStatements`) and keeps its door;
        `TypedQuery.resolve` is gone; `executionPlan` and `projectPlan` meet in `TypedQuery.planned`, the product query's one
        planning entry, whose door is `executionPlan@<runtime>[,<features>]` or `projectPlan@<runtime>`. `Execution.run` is one
        statement over that and carries no door (the first freeze's `run@<runtime>,<establish>`, which nested `executionPlan`).
        A test body's planning is `TestPlanner.plan` (`Execution.runProgram` with `BodyDriver`, the first freeze's door, is gone):
        `testPlan@<runtime>,<judge>,<rules>[,edge][,<features>]`, the judge `HOST`, `DATABASE` or `BOTH`, the rules `LITE` or
        `COMPILED_PURE_COMPAT`, `edge` when the compatibility edge was handed; `TestPlanner.planAhead`'s is `testPlanAhead@...`.
        The first freeze's dialect class is dropped from the name: it is a function of the model and the runtime, both in the
        key. `TypedQuery.withBody`'s copy keeps the query's scope (the lift and the unread lets are the same question).
     2. *The runners' names.* `PureTestRunner` plans a package's setup inside `runFixture` (`runSetup`'s `planBody`) and an
        on-demand fixture in `provideFixtures` (`planProgram`); both are labelled `fixture:` there.
     3. *The lowering's MirJson hook.* `Lowerer.lowerStoreWrite` gained `main`'s `RunScope` line; the hook is unchanged.
     4. *The SQL tap.* Nine of the JDBC seam's sends are tapped now, not four: `main` added typed rows and reported rows
        (stamped as a query is), a statement's columns, a prepared statement's description, a file read and a streamed
        insert. The recovery statements (a connector's constants) and the bulk loader's rows are not.
     5. *The new sites* (below) and *the plan oracle*: `IdentityProbe.plan` and `testPlan` record each plan's JSON, two new
        columns of a row.

   - **The sites on the merged tree.** The first freeze's javac census (every reference comparison and identity collection,
     attributed over core, `base`, `json`, `arrow_ipc`, 0 errors) run again at `1ea8fbf45` and matched to the old one by
     (file, enclosing method, text). Of §1.1's 33 identity collections all 33 stand, 13 at a moved line (`SpecCompiler` 46→48,
     `Typer` 108/118→112/122, `Overloads` 361→365, `UserCallInliner` 114→116, `NameResolver` 1388→1395, `ClassSources`
     320→333, `StoreResolver` 2216→2251, `Lowerer` 1166/2042→1206/2082, `ObjectSites` 16→24; the census's lines, of which
     §1.1 cites a few one off). Of its classified comparisons 29 stand where they were, 31 moved a
     few lines (`ModelBuilder`'s sixteen `indexed` arms 380-402→382-404 and `findAssociationAtClass` 786→798,
     `UserCallInliner` 136/268→138/283, the (e) sites of `VerdictSql`, `KeyedPivot`, `Lowerer`, `Fold`, `RelOpTranslator`,
     `FoldChecker`, `InferenceKernel`), and 3 went: `resolver/RawGridSchema` is gone on `main` (a raw grid is a keyed plan
     step now), with its 8 comparisons, 3 of them (e). The ten dialect shims' `d == rust` went with `main`'s `forServer`. In
     scope now: 36 identity collections and 438 reference comparisons (the old scope's 424 and `testplan`'s 14); `setup` and
     `testdatagen` have none. Added, each classified as §1.1 does:

     | Site (merged tree) | What | Class | Counted as |
     |---|---|---|---|
     | `testplan/TestKinds.java:76,78`, read at `TestDoor.java:163` (`byStep`) | an effect or seed step (an `ExecutionPlan.Node`) → its ledger texts, re-keyed by the step's place | (b): plan nodes are pure data, so two equal effect steps are equal records and must stay two (a map by content would merge them and `byStep`'s size check would refuse the body). The miss is loud, not silent. `testplan` is re-expressed (ruling 6): keying by the step's index, as `TestKinds.frameRuns` already does, removes the identity | `TestKinds.effects`, `TestKinds.seeds` (the reads); `TestKinds.effects.put`, `.seeds.put` (a step put twice: never) |
     | `testplan/BodyClass.java:231`, read at `:236,238` | a frame's query lambdas, by instance: the store scan descends into these alone | (b), as `SyntheticHeads.valuesLambdas`: an equal lambda handed elsewhere "runs nowhere" | `BodyClass.frameQueries` |
     | `testplan/SqlTextVerdicts.java:176,182,396,404` | the SQL-text collector's sentinel texts (`new String(...)`, "compared by identity, never a text a program spells") | (b): a token, as `Evaluation`'s `new Object()` origin | `SqlTextVerdicts.token`, `.goldenToken`, `.batchToken` |
     | `planner/ProgramFrames.java:235` (`pastes`) | how many times a frame's chain stands in the body, by reference: "a pasted read places the frame's chain itself" | (c) | `ProgramFrames.pastes` |
     | `testplan/TestPlanner.java:52,181`, `BodyClass.java:259,297,713`, `BodyDriver.java:122` (`withPlan`), `HostAsserts.java:355`, `compiler/spec/RelationCountAsserts.java:45,57`, `ResultEnvelopeSplice.java:669`, `planner/PlanAllocations.java:288`, `EnginePlans.java:105`, `sql/SqlRewriter.java:108` | hand back the original if nothing changed | (d), 13 | not counted, as before |
     | `sql/dialect/QueryBlocks.java:37,38,41,48` | the alias's query blocks | (d), 4: a dialect helper, in Rust since the merge's replay (`query_blocks::each`) | not counted |
     | `testplan/HostAsserts.java:350` (`program == env`), `SqlTextVerdicts.java:209` (`at == env`), `DatabaseJudge.java:309` (`fe == fa`, two `Type.Primitive` constants under `Type`), `lowering/Lowerer.java:2503` (`parent.children().get(0) != n`) | which did I pick, which child am I | (e), 4. The first two rely on `withPlan`'s (d) handing back `this`; the third is the database judge's (ruling 4) | not counted |

   - **What was counted**, summed over the frozen rows, beside the first freeze (a row counts a question once):

     | Class | Table or site | Hit / true | Miss / false | Rows | First freeze |
     |---|---|---:|---:|---:|---|
     | (a) memo | `SpecCompiler.memo` | 3,031,740 | 245,638 | 37,564 | 1,582,730 / 240,191 / 34,507 |
     | (a) memo | `Anchors.memo` | 152,740 | 5,235,155 | 52,376 | 134,771 / 5,305,268 / 48,778 |
     | (a) memo | `Anchors.spaceMemo` | 31,613 | 472,818 | 52,589 | 26,351 / 434,887 / 48,987 |
     | (a) memo | `ClassSources.routedSources`, `perRoutes` (each) | 14,859 | 8,824 | 4,966 | 12,354 / 7,207 / 4,907 |
     | (a) memo | `ContextReading.scoped` | 1,786,100 | 379,866 | 14,428 | 4,018,844 / 667,100 / 14,254 |
     | (a) memo | `ContextReading.timeZone.seen` | 3,974 | 1,504,862 | 21,468 | 3,834 / 2,247,049 / 19,647 |
     | (a′) side table | `whereZones[not-in-rust]` | 11,730 | 6,643 | 6,290 | 9,428 / 5,700 / 6,399 |
     | (a′) side table | `DateSplit.ctxStrips` | 56 | 5,800 | 36 | 36 / 3,768 / 36 |
     | (a′) side table | `DateSplit.dateRenames` | 346 | 5,454 | 36 | 226 / 3,542 / 36 |
     | (a′) side table | `StackBuilder.leaves` | 2,333 | 73,452 | 5,881 | 1,220 / 53,420 / 5,775 |
     | (a′) side table | `StoreResolver.aggReads` | 2,211 | 1,193,754 | 27,255 | 1,558 / 1,086,703 / 24,599 |
     | (a′) side table | `InnerDemand.inQueryReads` | 328 | 2,844 | 687 | 220 / 2,094 / 591 |
     | (a′) side table | `Lowerer.calCtx` | 1,326 | 24,091 | 7,746 | 882 / 20,882 / 7,532 |
     | (b) query tier | `TypedUserCall.equals.origin` | 0 | 0 | 0 | the same |
     | (b) query tier | `Evaluation.equals.origin` | 0 | 4,424 | 240 | 0 / 804 / 12 |
     | (b) query tier | `Evaluation.Site.equals.origin` | 749,188 | 0 | 1,260 | 746,151 / 0 / 1,317 |
     | (b) query tier | `CallInstantiation.equals.frame`, `.origin` (each) | 1,280 | 0 | 84 | 3,334 / 0 / 104 |
     | (b) query tier | `UserCallInliner.callInstantiations` | 1,280 | 1,319,305 | 13,528 | 3,334 / 1,266,951 / 13,472 |
     | (b) query tier | `SourceSubst.untrackedLetNodes` | 4,400 | 2,452 | 237 | the same |
     | (b) query tier | `ContextReading.lambdaBound` | 768,414 | 1,016,177 | 12,769 | 714,195 / 2,271,789 / 11,172 |
     | (b) query tier | `SyntheticHeads.valuesLambdas` | 208 | 172,736 | 25,949 | 144 / 143,771 / 23,239 |
     | (b) query tier, **was (e)** | `ObjectSites.nodes` (below) | 822 | 232 | 17 | 0 / 0 / 0 |
     | (b) query tier, **new** | `TestKinds.effects` | 2,281 | 30,036 | 17,606 | |
     | (b) query tier, **new** | `TestKinds.seeds` | 11,664 | 20,653 | 17,606 | |
     | (b) query tier, **new** | `TestKinds.effects.put`, `.seeds.put` | 0 | 2,283; 11,913 | 864; 11,913 | |
     | (b) query tier, **new** | `BodyClass.frameQueries` | 138,766 | 213,534 | 22,972 | |
     | (b) query tier, **new** | `SqlTextVerdicts.token`, `.goldenToken`, `.batchToken` | 336; 4; 94 | 10,546; 10,878; 0 | 6,588; 6,588; 94 | |
     | (b) model tier | `model.ModelBuilder.indexed.*` (15 of its 16 arms fired, as before) | 73,173 | 11,500 | 16,791 | 61,458 / 7,905 / 16,589 |
     | (b) model tier | `model.PureModelContext.CheckedLayer` | 6,478,837 | 183,150 | 2,779 | 6,385,510 / 173,890 / 2,728 |
     | (b) model tier | `model.DatabaseDefinition.defaultSchemaTables` | 238,543 | 34,183 | 3,733 | 141,942 / 23,336 / 3,487 |
     | (b) model tier | `model.DatabaseDefinition.defaultSchemaViews` | 307 | 76,425 | 356 | 163 / 31,007 / 199 |
     | (b) model tier | `model.MappingDefinition.position` | 23,989 | 0 | 242 | 15,356 / 0 / 165 |
     | (b) model tier | `model.AssociationSynthesis.self` | 68 | 66 | 19 | 66 / 64 / 19 |
     | (b) model tier | `model.TableIndex.inSchemas`, `LiftedViews.*`, `ViewSignatures.expanding`, `MappingClosures.first`, `MappingPrePass.selfEdge`, `ScanRelations.pm` | | | | the same, each |
     | (c) query tier | `UserCallInliner.reducedTerms` | 2,769,826 | 17,027,637 | 52,673 | 2,436,879 / 15,330,022 / 49,041 |
     | (c) query tier | `UserCallInliner.countReads` | 25 | 263 | 9 | 19 / 143 / 9 |
     | (c) query tier | `Typer.pretyped`, `Typer.withPretyped.shadowed`, `Overloads.pretyped` | | | | the same, each |
     | (c) query tier | `InnerDemand.lambdas.seen` | 3,558 | 4,632 | 531 | 2,734 / 3,417 / 527 |
     | (c) query tier, **new** | `ProgramFrames.pastes` | 88 | 1,782 | 89 | |
     | (c) model tier | `model.NameResolver.resolvedViews`, `ModelNormalizer.adopted`, `ModelBuilder.findAssociationAtClass` | | | | the same, each |

   - **What ran.** The first freeze's targets, on the merged tree: `//core/...` (121 test targets now: `main` added the
     `*_judge_database` and `*_judge_both` variants of thirteen lanes), `//spec:corpus_duckdb` and `corpus_h2` (each a host and
     a database pass, 2,613 tests, 108 fixtures and 113 setups), PCT's DuckDB five, `pct_h2` and channel B's six: 145 test
     targets and the four corpus passes, each run at most four actions and two tests at a time (`--jobs=4
     --local_test_jobs=2`, PHASE1.md §6), `--nocache_test_results` and the corpus flag making every one run again. Each run
     took 23 to 29 minutes. Two targets fail with the probe, both rules over the source and neither over what is computed:
     `//core:guardrails`, as in the first freeze (the probe's own shape), and `//core:census`'s `HarnessDisciplineTest` (the test door's name sorts its feature list: a "sort site" in
     `TestPlanner`). Frozen rows by set (a question two sets ask is one row):

     | Set | Rows |
     |---|---:|
     | `//spec:corpus_duckdb`, each of its host and database passes | 5,418 |
     | `//spec:corpus_h2`, each pass | 5,418 |
     | `//core:stress_suites` / `stress_suites_h2` | 12,147 / 6,077 |
     | `//core:core_tests_integration` / `_h2` / `_sqlite` / `_judge_database` / `_judge_both` | 2,984 / 2,979 / 2,955 / 2,984 / 2,984 |
     | `//core:iq_parity_test`, `cypher_parity_test`, `cypher_parity_native_test`, `sql_parity_test` | 8,503, 5,897, 5,917, 2,130 |
     | PCT: DuckDB's five lanes, `pct_h2`, channel B's six | 2,495, 491, 1,730 |
     | The rest of `//core/...` (50 lanes, 22 of them judge variants) | 4,241 |
     | **All of it, distinct** | **73,411** (B 1, L 24, M 2,792, Q 70,594; 2,803 models) |

     Query ids: `t:` 31,615, `test:` 15,536, `service:` 18,249, `vs:` 4,535, `fixture:` 432, `setup:` 227. *Left out, and
     why:* as before, the Postgres lanes (`iq_parity_postgres_test`, `cypher_parity_postgres_test`, `sql_parity_postgres_test`,
     `postgres_arm_test`, `sql_island_postgres_test`, `store_writes_postgres_test`, and `main`'s `relation_to_one_postgres_test`)
     and the 30 keys of each `core_tests_integration` lane and 6 of each `core_tests_server` lane whose model holds a temporary
     path: 16,851 keys in each run, none in both, and with the model id set aside their counts agree between the runs, lane
     by lane. `//core:ladder_report` (a build action: the cache answered it in both runs, so its rows are the compile check's
     alone) is not frozen. `TestPlanner.planAhead` was reached by no lane in these sets, so the oracle holds no plan made
     ahead. The texts of `t:` ids are read from `rust/oracles/front/questions.jsonl.zst`: 31,438 of the 31,615 `t:` query
     rows (the other 177 are `compileAllBodies`' constant id, 50 rows, and 127 rows of texts newer than that oracle) and
     35,652 of the 37,112 rows with a `t:` model (50 of 2,664 distinct model texts are newer).

   - **Stable.** Two complete runs, A and B, of the same patched tree, 180,219 rows each, 90,262 keys each. Within a run no key
     has two answers (counts, MirJson ids, plan ids). Of the 73,411 keys in both, **no count differs, no lowering id moved and no
     plan id moved**. The first freeze's 72 moved lowering ids are gone: they were a corpus aggregation-aware test's `VALUES`
     holding the host's fresh `executionTraceID`, read back by the body driver after a statement ran; a test body is planned
     before it runs now. Every plan text's id was checked against its text (SHA-256 of its UTF-16 units), 59,552 texts in each
     run.

   - **Counting changes nothing.** A control run C applied only the SQL tap and a plan tap at the plan oracle's three sites
     ([`rust/tools/identity-control-tap.patch`](../../rust/tools/identity-control-tap.patch): no counting, no scope; each plan's
     masked id written in order beside the statements). Against A, over 6,986,471 statements in 72 processes: 6,969,583 in
     the same order, one process (`core_tests_exec_judge_database`) the same 531 statements with two adjacent ones swapped,
     and three (`core_tests_integration`, `_judge_database`, `_judge_both`) differing at 15 statements each, every one of which
     differs between A and B too (a temporary path). The swap is the lane's own: the control alone, run four times on that
     lane, sends the two in one order three times and the other once. The plans, per process as a multiset: 58 of 72 the
     same; 13 differ only where A and B differ too (the Postgres lanes' port, the temporary paths); `ladder_report` has plans
     only in C (the cache answered it in A). The control fails only `//core:guardrails`, by its plan tap's reflection (rule
     F1.11). `//core:update_ladder_*` (the SQL pins), the corpus rosters' `update_rcorpus_*` and both corpus verdicts stayed
     green with the probe.

   - **What moved against the first freeze, and why.** Matched one to one by key with the doors normalized (`run@R,E` and
     `executionPlan@R`; `runProgram@R,D,J` and `testPlan@R,J`): 49,774 rows, of which 33,576 count exactly as before and
     16,198 differently. By question:
     1. *Test bodies* (`testPlan` rows, 11,128): the body is planned whole once by `testplan`, where `BodyDriver` walked it and
        re-ran the inliner, resolver and lowering per statement and per assert side (PHASE3_SHIM_DESIGN.md §3.3). So the
        context reader is asked far less (`ContextReading.scoped` 3.0M/0.46M → 0.05M/0.08M on those rows, `lambdaBound`'s
        misses 1.7M → 22K, `timeZone.seen`'s 1.26M → 0.21M), while the inliner and the memo see the whole program
        (`reducedTerms` and `SpecCompiler.memo` grow; `SpecCompiler.memo`'s hits 1.43M → 2.81M), and the new tables appear.
     2. *`programFacts`* (4,752 rows, the corpus's): only `SpecCompiler.memo` moves (misses 39,202 → 33,828).
     3. *Product queries* (`executionPlan`, 313 of 29,772 matched): only `SpecCompiler.memo` (hits 706 → 1,454: the product
        statement road asks it again for a body it has typed).
     4. *Models*: 5 `buildModel` rows whose boot-layer check moved (`CheckedLayer` 11,955/18,060 → 0/70).
     Unmatched: 17,139 old rows, 23,567 new; 14,266 of the old find their partner when only the model id differs (`main`
     changed the stress suites' and PCT's sources, so their `s:` ids moved), 1,014 when only the doors do. The new rows are
     mostly questions `main`'s tests added (the judge lanes, `core_tests_testplan`, the stress suites' 2,704 more rows).

   - **What the first freeze said that no longer holds.**
     1. *`ObjectSites.nodes` is alive* (the first freeze's finding 3, §1.1's class (e)). `main` folded the test road's per-node
        ids (`judge/InstanceIds`, gone) into `ObjectSites`: "a construction the compiler builds after the inliner keys by its
        node". 822 hits and 232 misses in 17 rows: PCT channel B's four lanes and `core_tests_testexec` under all three
        judges. It numbers objects (`o1`, `o2`, ... or the test road's `i1`, ...) by node instance, so two equal constructions
        are two objects and the numbering order reaches the SQL: class (b), and the port needs it.
     2. `judge/InstanceIds` "stays Java" falls away with it (PHASE3_SHIM_DESIGN.md §10, question 13).
     3. The 72 `~moved` lowering ids, and with them the need for a mask (above).
     4. Findings 1, 2, 4 and 5 still hold on the merged tree: the (a′) lookups still miss by design; `ClassSources`' two
        levels still count alike in every row (4,966); `TypedUserCall`'s `origin ==` and `findAssociationAtClass` are still
        never reached, nor the `indexed` arm for a `LegacyMappingDefinition`.

   - **The plan oracle (new;** [`rust/oracles/plan/`](../../rust/oracles/plan), its `MANIFEST.tsv`**).** For every question
     the lanes enumerate, the plan Java made: a product query's `ExecutionPlan` where `TypedQuery.planned` makes it
     (`PlanJson`, v31) and a test body's `TestPlan` where `TestPlanner.plan` makes it (`TestPlanJson`, v11 over the product's
     v31), in each row in order. A plan's id is the SHA-256 of its JSON's UTF-16 units after masking run-specific values.
     *Masking:* the one candidate, the execution-trace comment's UUID (`testplan/ActivityComments` reserves one at planning
     under a verdict batch), is masked to the zero UUID; it applied **0 times** in both runs, and a search of run A's every plan
     and MirJson text for any UUID found none. The temporary paths and Postgres ports some plans hold are in their model's text
     too, so those keys are not frozen at all. Nothing else differed, so every frozen id is the hash of exactly what Java's
     writer wrote. *Sizes:* 51,337 rows with a plan: 25,740 with an execution plan (25,741 plans; 46 through `projectPlan`),
     25,597 with a test plan. Test plans by judge: `HOST` 17,651 rows (12,318 under lite's rules, 5,333 under the
     compatibility rules), and, **marked** as the database judge's in their doors, `DATABASE` 5,800 and `BOTH` 2,146 (ruling 4:
     not ported at first; frozen because the same runs made them). Distinct texts: 17,044 execution plans (424 million
     characters), 16,247 host test plans (30.2 billion: each stress-suite plan carries its 500 KB test data CSV several times
     over, about 2.5 MB a plan, and compresses about 900 to 1), 6,850 test plans only the database judge's rows reach (4.0
     billion).

   - **Frozen.** [`rust/oracles/identity/`](../../rust/oracles/identity), replaced (its MANIFEST says what moved):
     `hits.tsv.zst` 1.4 MB (25.8 MB of text, 73,411 rows, 69 tables), `mir-ids.tsv.zst` 2.5 MB (51,606 rows, 167,833
     lowerings, 40,613 distinct, 2,546 refusals: 2,538 tagged lists, 8 tagged values), `mir-store.tsv.zst` 5.3 MB (876
     million characters). [`rust/oracles/plan/`](../../rust/oracles/plan), new: `plan-ids.tsv.zst` 2.4 MB (every plan's id,
     both judges') and `execution-plans.tsv.zst` 3.5 MB (every execution plan's text). The test plans' texts are archived
     outside git, as the front oracle's full answers are, by the MANIFEST's rule (a text store in git when it is under 10 MB
     compressed and not the database judge's): `~/.local/legend-lite/oracle-archive/plan-test-plans-host.tsv.zst` (65 MB) and
     `plan-test-plans-database-judge.tsv.zst` (8 MB), each with its SHA-256 in the MANIFEST and in `plan-test-plans.sha256`
     beside it, each checked by decompressing it. Every file was checked against its MANIFEST row, every MirJson and plan id
     against a text, and every execution plan's text re-hashed to its id.

   - **For the Rust side, the plans.** A Rust planner reproduces a row when its writer's JSON for the question, masked so,
     hashes to the row's ids in order: the product doors' plans at switch 1, `testplan`'s at switch 2 (§5 of
     PHASE3_SHIM_DESIGN.md), the host judge's rows first. The ids are in git, so the comparison needs no archive; the archive
     is for reading a mismatch.

   - **The host judge's test plans in git, deduplicated (2026-11-11,** PHASE3_SHIM_DESIGN.md §10 ruling 18**).** Measured
     first: of the archive's 30.2 billion bytes, one 516,123-byte CSV (the stress suites' `testDataSetupCsv`) stands 31,494
     times in 10,498 plans (16.3 GB) and SQL strings take 4.5 GB more, but deduplicating whole JSON strings still leaves 4.9
     GB of plan text even from 64 bytes: whole sub-objects repeat across a suite's plans (the session, the seed steps, MIR
     trees). So the grain is the whole JSON value: a string, array or object of at least 8,192 bytes that occurs twice or
     more, nested ones too, is stored once under its key (its text's id, the plan's rule), and its place holds a reference,
     U+0001, the key's 64 hex digits, U+0001, a character Java's compact JSON writer never writes and the archive never
     holds. 26,551 values; 154 MB of plan text and 91 MB of values left, in independent 2 MiB zstd frames so that one plan
     reads only its own frames (65 to 83 ms a plan from a fresh reader, a median 5.6 MB decompressed). The threshold is the
     measured smallest (the four files' bytes: 256 17.1 MB, 1,024 11.8, 4,096 10.3, 8,192 10.1, 16,384 10.3). In
     [`rust/oracles/plan/`](../../rust/oracles/plan): `test-plans-host.tsv.zst` 6.0 MB, `test-plan-values.tsv.zst` 3.9 MB,
     `test-plan-values-index.tsv.zst` 0.2 MB, `test-plan-frames.tsv` 13 KB, 10.1 MB together; the format, the writer, the
     reader and the check are [`rust/tools/test-plan-texts.py`](../../rust/tools/test-plan-texts.py) (a tool beside the
     probes, not a probe: reading the oracle needs it). **Lossless:** all 16,247 plans rebuilt, each hashing to its id,
     every value to its key, the rebuilt stream's SHA-256 the archive's (`17af882a…`), the ids exactly the host rows' test
     plan ids; the writer run twice wrote the same bytes. A host mismatch is read from git now (`test-plan-texts.py read`);
     the archive stays, as the source, and the database judge's plans stay in theirs only.

1. **`identity`, `Type`/`ExprType`; the `ValueSpecification` family to `Arc`.**

   - **The source tree's records are `Arc` (done 2026-11-11,** the design's §6 ruling 1**).** The generator
     (`FrontCodegen`, `//core:front_records_rs`) holds every record that is a `ValueSpecification` node -- the sealed
     parent's 25 records and `ColumnInstance`'s 2 and `TypeAnnotation`'s 4, 31 -- as `Arc<R>` wherever a slot or a
     sealed parent holds it, and their 19 list components as `Arc<[T]>`. Every other record stays `Box`, its lists
     `Vec`: a record a node holds that is not itself one (`KeyExpression`, `NewInstance.KeyBinding`,
     `GraphFetchLiteral.Node` and `SubTypeNode`, `PathLiteral.Segment` and its arguments, `RelationShape.Column`, the
     GraphQL island's records, `SourceInfo`, `TypeExpression`, `Multiplicity`, a stereotype), and an element's list of
     value specifications (a function's body). Forced outside the family: `PClassMappingFunction` and
     `PFunctionAssociationMapping`, whose `function` and `bodyLambda` are family records (`Option<Arc<_>>`). A map on a
     node is refused by the generator until its shape is ruled (none exists). The Java reader (`FrontRecords`) did not
     change.
   - **The parser.** A list is built as a `Vec` and frozen into its `Arc<[T]>` (`nodes.rs`' helpers keep their `Vec`
     parameters). The service grammar's signed-literal fold is rewritten as Java's `withChildren`: the same node or
     list back when nothing changed, a new one with the variant's losses when something did. A quoted form's `original`
     aliases its call, as Java's does; an assertion's lambda is rebuilt with its span extended. **One `Arc::get_mut`**:
     `parse_parenthesised` marks the call it has just parsed `grouped` (its `)` comes after it is built; nothing else
     holds it yet: the one node the parser clones and keeps while it reads on, `parse_key_value`'s at the engine's
     dialect, was built before, never the one returned here. A shared one would fail the `expect`, loudly, never
     silently). `Arc::unwrap_or_clone` moves the parts of a node just built (a move, as the count is one) in
     `parse_lambda`, a property over an element pointer read as an enum value, and `signed_literal`.
   - **No answer moved.** The three front differentials as before, every answer byte-pinned: the lexer 52,143 of
     52,143; the four doors 110,044 identical and 552 listed of 110,596; `document` and `lambdaJson` 29,848 and 216 of
     30,064; 0 differ. `//core:front_shim` the same two counts, 0 differ. A mutation fuzz (a scratch harness, one seed:
     about 12,000 of the oracle's texts, each and three mutations of it, at every door: 275,040 answers) gave every
     answer's form the same SHA-256 before and after, no panic.
   - **The parser's speed, natively** (release, no FFI, no JVM: a scratch harness over `lite_front::doors`, the oracle's
     every ask on one thread of `PARSE_STACK`, the records released inside the time; before and after alternated twice).
     Totals in ms, before → after. The model doors, the last of three passes (load 3.5 falling to 1.5):

     | | round 1 | round 2 |
     |---|---|---|
     | model texts, parse (`pmcd`, 29,188 a dialect): lite's | 1,812 → 1,846 | 1,788 → 1,800 |
     | the platform's | 1,793 → 1,833 | 1,771 → 1,788 |
     | the engine's (with the emitter's judge) | 2,410 → 2,433 | 2,381 → 2,350 |
     | parse and codec, lite's (the shim's model door) | 2,537 → 2,568 | 2,504 → 2,501 |
     | `document` (parse and emit) | 2,225 → 2,225 | 2,199 → 2,162 |

     The expression doors, whose totals are small, the least of ten passes (load 4.4-4.6):

     | | round 1 | round 2 |
     |---|---|---|
     | `spec`, every dialect (22,151) | 43.4 → 48.4 | 43.9 → 48.1 |
     | `lambda` (876) | 11.0 → 13.1 | 11.1 → 13.0 |
     | `lambdaJson` (parse and emit) | 12.9 → 15.0 | 13.0 → 14.8 |

     So the model texts' parse is 0.7-2.2% slower at lite's and the platform's dialects (p50 the same to 0.0003 ms, p95
     1.7-2.6% higher) and -1.3% to +1.0% at the engine's, the rounds differing by 1-3%; an expression's parse is 10-20%
     slower, about 0.2 µs a `spec` query and 2.4 µs a lambda. **Allocations**, counted in one pass: `spec` 48.2 → 51.3 an ask, `lambda` 303.5 →
     339.1, `pmcd` at lite's 1,544 → 1,596, at the engine's 1,693 → 1,687. A list frozen from its `Vec` is a second
     allocation and a copy, and an empty `Vec` frozen is one where it was none (`Arc::default()` allocates nothing); the
     counts' atomics are the rest. Not changed here, and not measured: freezing an empty list as `Arc::default()` (one
     shared empty list, as Java's `List.of()` is) and building a short list straight into its `Arc<[T]>` (both done the
     same day, "The flip's cost won back", below).
   - **Gates.** `bazel test //... --keep_going --local_test_jobs=2` against `rust/BASELINE.tsv`: 567 targets, 186
     executed; every target the baseline passes passing but two that timed out while the machine was loaded (load
     13-15, 3.5 GB of swap in use); gone the 3 ledgered; new 18, the port's, passing. The two,
     `//core:stress_suites_h2` (3,702 s) and `//parser-equivalence:parser_parity` (3,209 s, its 45 tests passing), pass
     on a quieter run of the same inputs (689 s, 190.5 s). `parser_parity`'s counts as at `bcf60ed8f`: 422 pins, line
     agreement 415 of 421, column exact 337; the sentinel 2,217 matched of 2,217, lenient 0; the composer sweep's 6,755
     sources, 0 mismatched; own corpus 3,025, 0 diffs. `:round_trip_proof` and `//core:front_reader_test` pass. `cargo
     test --workspace`, clippy `-D warnings` and `fmt --check` clean.
   - **Slice 1a: the foundations, `identity` and `Type` (done 2026-11-11;** [`PHASE3_CRATES.md`](PHASE3_CRATES.md)**).** No
     Java production code changed, and nothing calls the new crates yet.
     - *The crates.* `PHASE3_CRATES.md` maps phase 3's crates: one per layer of `tools/deps/core-layers.txt` that phase 3
       ports, `door` merged into `builtin` (an exact merge), `compiler` split into names, element and spec in that order
       so that F cannot reach G (four Java edges against the order are listed for their slices). Made now: `lite-base`
       (the layer `base`), `lite-types` (`compiler_element_type`) and `lite-guards` (tests only). `lite-guards` holds
       `rust/lite-guards/crate-layers.txt` exactly against every crate's Cargo.toml and, under Bazel, each library's
       genquery, every edge of it within the layers file (a crate may reach less than its Java layer, never more), and
       the sources to the arena ruling's two rules: `unsafe` only in `lite-base/src/arena.rs` and `lite-ffi`, every
       other crate root `#![forbid(unsafe_code)]` (added to `lite-javacompat`, `lite-json`, `lite-sql`, `lite-dialect`;
       `lite-front`'s waited on its own work, and was added when its exemption was lifted, below), and a pointer turned
       into an address only in `lite-base/src/identity.rs`.
       `lite-sql` gained Java's `SqlSource.Pivot.SEPARATOR`, `presented` and `SqlExpr.TDS_NULL_CELL`, which the type
       layer reads from the `sql` layer.
     - *The allocator* (`lite-base/src/arena.rs`, hand-written, no crates.io dependency): 298 lines, 203 of code. A bump
       through chunks that never move (4 KiB doubling to 1 MiB, each 16-aligned; a value past a quarter of the next chunk
       or aligned past 16 gets a chunk of its own; every value at least one byte, so two `alloc`s of a zero-sized record
       are two objects, and every empty slice is the one `&[]`, as Java's `List.of()` is one object); the sealed `Alloc`
       trait (`alloc<T: Copy>`, `alloc_slice`, `alloc_str`, `alloc_from_iter`, which refuses an iterator that lies about
       its length); `QueryArena`, `Send` and not `Sync` (both held at compile time), with its keep pool (`keep`, for a
       `Box` SQL tree or an `Arc` source node, dropped with the arena); `ModelArena`, `Send` and `Sync`, a mutex on the
       bump state and the value written outside it. Its `unsafe`: 10 blocks, 2 `unsafe impl Send` and the keep pool's one
       `unsafe fn`, with 12 `SAFETY:` comments (`clippy::undocumented_unsafe_blocks` denied). 21 tests, 12 of the arena
       (alignment to 4,096, zero-sized values, empty slices, copies, an iterator that allocates and two that lie, a 16 MB
       value, chunk growth over 6.4 MB, the keep pool's 1,001 drops, a query arena moved to another thread, eight threads
       on one model arena) and 9 of identity. **Miri was not run**: the pinned toolchain (1.99.1, stable) has no `miri`
       component. The module avoids the shapes stacked borrows refuse: chunks kept as `alloc`'s raw pointers, the keep
       pool's boxes leaked to raw pointers and rebuilt only to free them.
     - *identity* (`lite-base/src/identity.rs`): `same` over `Identity` (an arena record, slice or text; an `Arc` node or
       list), `Addr` (no order, no text, no number), `IdMap` and `IdSet` (lookup, insert, remove, no iteration; an `Arc`
       key kept alive), `Origin<'q>` (of a source node, which the query's keep pool then holds, or fresh: a byte of the
       arena). The design's §1.5 item 3: equal but not same, same after a copy, a `Filter` rebuilt with equal children
       not same, a copied list not the list.
     - *`Type<'a>`* (`lite-types`): Java's family as one `Copy` family with a lifetime, a leaf inline and a structural
       type a reference to its arena record; covariant, proved by a function that compiles only so and by the static
       late-bound schema beside a query's types; at most three words. Every operation of the layer that needs no later
       layer is ported (PHASE3_CRATES.md §4 lists them); `Multiplicity.from`, `PlatformTypes.eraseTdsRow`,
       `handleRowClass` and `isStatementOnly` wait for the layers they read (`protocol`'s records, `builtin`, `model`),
       and subtyping is the model's in Java (`ModelContext.isSubtype`), so it is not invented here.
     - *The differential.* `rust/oracles/types/TypeOracle.java`, run on core's built jars, froze Java's answers in
       `type-facts.tsv` (MANIFEST row; two runs byte-identical): 26,429 lines over 80 ops -- 356 types (every primitive,
       every `PlatformTypes` constant as a class, every `byFqn` name, the carriers with zero, one and two arguments,
       function, relation and schema-algebra types), 17 multiplicities, 20 decimals (Java's `int` wrapping included), 536
       strings; every type against every type for equality; every refusal by its exception and message.
       `//rust/lite-types:type_differential_test` builds each type in both arenas and holds every line: **26,429 of
       26,429 identical**, each op's count the oracle header's.
     - *Gates.* `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings` and `cargo fmt --check`
       clean; the four new targets (`//rust/lite-base:lite_base_test`, `//rust/lite-types:lite_types_test`,
       `:type_differential_test`, `//rust/lite-guards:guards_test`) in `//gates:core`. `bazel test //... --keep_going`
       at `000520ef1`: 571 targets, 569 passed and 2 skipped (the baseline's two); of `rust/BASELINE.tsv`'s 550 passing
       targets every one passes but the three gone with their code and ledgered in `rust/RETIRED_TESTS.md`
       (`//core:core_tests_ide`, `//core:core_tests_lexer`, `//docs:update_dialect_capability_test`).
   - **The flip's cost won back, and `lite-front` under the source guards (done 2026-11-11,** the owner's ruling 20,
     [`PHASE3_SHIM_DESIGN.md`](PHASE3_SHIM_DESIGN.md) §10; `4cb1a8df5`, `2cd2b2a7e`**).** Every empty list of a node is the one shared empty list, `Arc::<[T]>::default()`
     (std's one static, no allocation), as Java's `List.of()` is one object; a list whose length is known is built
     straight into its `Arc<[T]>`, one allocation and no `Vec` first: `nodes::IntoList` (a `Vec`, an array, a list
     already frozen) and `nodes::list_of` (an iterator of known length), the helpers taking an array where the parser
     wrote `vec![..]`, and a call's receiver and arguments, an arithmetic run's operands, `.all()`'s class and dates, a
     range, a table reference and a grammar call's parameters built so. The service grammar's signed-literal fold makes
     a list only at its first changed value (the flip had it copy every list it visited). **No answer moved:** the
     mutation fuzz's 275,040 forms the same SHA-256 before and after (and the flip's own), every frozen question as
     before (the difference list's 768 asks exactly), the front differentials and `//core:front_shim` as before.
     **The speed**, natively (the flip's harness; three builds alternated: before the flip, the flip, now; load 2.3-2.7),
     total ms, before the flip / the flip / now:

     | | round 1 | round 2 | round 3 |
     |---|---|---|---|
     | `spec`, every dialect, least of ten passes | 42.3 / 44.4 / 42.7 | 42.7 / 44.2 / 43.1 | 42.1 / 43.9 / 42.6 |
     | `lambda` | 10.70 / 11.49 / 10.90 | 10.68 / 11.44 / 10.94 | 10.57 / 11.34 / 10.79 |
     | `lambdaJson` (parse and emit) | 12.62 / 13.39 / 12.69 | 12.66 / 13.31 / 12.77 | 12.42 / 13.20 / 12.59 |
     | `pmcd` at lite's dialect, the third of three passes | 1,743 / 1,798 / 1,764 | 1,777 / 1,820 / 1,795 | |
     | the platform's | 1,724 / 1,786 / 1,755 | 1,761 / 1,810 / 1,789 | |
     | the engine's | 2,344 / 2,365 / 2,327 | 2,383 / 2,386 / 2,373 | |
     | parse and codec, lite's (the shim's model door) | 2,457 / 2,488 / 2,463 | 2,496 / 2,525 / 2,493 | |
     | `document` (parse and emit) | 2,157 / 2,163 / 2,145 | 2,188 / 2,193 / 2,170 | |

     So of the flip's cost on this run (`spec` +3.5-4.9%, `lambda` +7.1-7.4%, `lambdaJson` +5.1-6.3%, the model texts
     +2.4-3.6% at lite's and the platform's dialects), about three quarters is won back: `spec` +0.9-1.3% over the
     unflipped parser, `lambda` +1.9-2.4%, `lambdaJson` +0.6-1.4%, the model texts +1.0-1.8%, the engine's dialect,
     the codec and `document` -0.8% to +0.3%. (The flip's own run, at load 4.4-4.6, measured its expression doors 10-20%
     slower; the same flipped build is 4-7% slower here.) **Allocations an ask**, one pass, before the flip / the flip /
     now: `spec` at lite's dialect 48.2 / 51.3 / 48.7, the platform's 27.2 / 29.1 / 27.8, the engine's 38.0 / 40.1 /
     39.3; `lambda` 303.5 / 339.1 / 306.5; `block` 31.6 / 34.4 / 32.4; `pmcd` at lite's 1,544 / 1,596 / 1,557, the
     platform's 1,553 / 1,605 / 1,566, the engine's 1,693 / 1,687 / 1,647. What remains is a list of unknown length
     (a call's arguments, a lambda's body, a collection's values), built as a `Vec` and frozen: two allocations where
     the unflipped parser made one. **The guards:** `lite-front`'s exemption from the arena ruling's source rules is
     lifted -- its `sources` filegroup handed to `//rust/lite-guards:guards_test`, its root `#![forbid(unsafe_code)]`
     (it holds no `unsafe` and no address conversion), the dated `CARGO_ONLY` entry removed, the list now empty.

2. **The model records, the platform's declarations and the text's door (slice 1b, done 2026-11-12;**
   [`PHASE3_CRATES.md`](PHASE3_CRATES.md) §6**).** No Java production code changed but the generators' second output;
   nothing calls the new crates yet. `ModelIndex`, its freeze and name resolution are the names slice's (step 2's rest).

   - *The crates.* `lite-error` (layer `error`, 257 lines: `LegendCompileException`'s phases, as the code has them --
     `RENDER` among them -- and the package's eleven classes as one record by kind), `lite-model` (`model`, reaching
     `lite-base`, `lite-front`, `lite-javacompat`; 4,632 generated lines and 4,310 by hand), `lite-builtin` (`builtin`
     and `door`, reaching `lite-error`, `lite-front`, `lite-model`; 3,317 generated and 4,918 by hand, 1,072 of tests),
     `lite-platform` (`platform`, reaching `lite-base`, `lite-builtin`, `lite-error`, `lite-model`; 2,450, and 945 of
     tests). `lite-values` is not made: nothing in `model`, `door`, `builtin` or `platform` reads `com.legend.values`
     (its records are `lite-front`'s). Each in `rust/lite-guards/crate-layers.txt` with its exact dependencies, the
     workspace, Bazel and `//gates:core`; each root `#![forbid(unsafe_code)]`.
   - *The model's records are generated* from the Java's by a third mode of `FrontCodegen` (`//core:model_records_rs`,
     held by `//rust/lite-model:update_generated_test` in `//:generated`): 190 types reached from `ParsedModel`,
     `NormalizedModel`, `Function`, `FunctionId` and `ClassMember` (153 records, 30 sealed parents, 7 enums, each enum
     with Java's `values`, `name`, `ordinal`, `valueOf`), each component an `Option` exactly where its source says
     `@Nullable`, with the lossless codec's writer, so both sides write the differential's canonical text from one
     schema. The model tier is `Arc` (PHASE3_DESIGN.md §6 ruling 1): a model record held as `Arc<R>`, a list
     `Arc<[T]>`, a map `Arc<[(K, V)]>` in Java's iteration order (every model map is a `LinkedHashMap`'s view: the
     differential's Java side checks it). **No model record writes its own `equals`** (searched: only `FunctionId` and
     `PoisonKey` write a `toString`), so every one derives `PartialEq`, and `Eq` and `Hash` where every component has
     them (126 of 190). A protocol record the model holds is `lite-front`'s.
   - *By hand in `lite-model`:* the converters, line for line -- `ModelFromProtocol` (the section index's imports, the
     spans, a parsed model's offsets), `FromProtocol`, `MappingFromProtocol`, `RelOpFromProtocol`,
     `MongoDeclarations` -- each throw Java's class and message (a converter's refusal named, as the door catches it by
     class); `FunctionId` and `SignatureMangle`; `ImportScope`'s builder; `ParsedModel`'s keys and positions;
     `RelationalIdentifier`, `JoinType`, `LogicalOp`, `ComparisonOp`, `SynthHat`, `SetId`, `DerivedPropertyNames`;
     `DatabaseDefinition`'s default-schema reads by identity (`lite_base::IdSet`, the schemas' and the flat lists sharing
     their records as Java's do). Left for the slice that first calls them: the methods the normalizer and the
     compiler call on `MappingDefinition`, `ClassMapping`'s witness-column names, `LegacyMappingDefinition`'s and
     `RuntimeDefinition`'s helpers, `RelationFunction.Col.bindSrc`.
   - *The text's door and the live differential.* `lite_builtin::door::parse` is `ModelFromText.parse`.
     `core/src/test/java/com/legend/frontoracle/ModelAnswers.java` asks Java's door every model text of the front
     oracle at every dialect it is asked at -- 87,564 asks over 29,188 texts; 74,292 answered (490,785 elements of 23
     kinds, 1,679,438 model records in sealed slots alone), 13,272 errors (`ParseException` 13,200,
     `IllegalStateException` 39, `UnsupportedOperationException` 33) -- and hashes each answer's canonical text (the front oracle's form: `A` and the record codec's JSON of the `ParsedModel`, every
     record and component, or `E`, the class, line, column and message). `//rust/lite-builtin:model_differential_test`
     holds Rust's door to every hash: **87,564 of 87,564 identical, on the first run**, with no list of differences.
     Java's hashes are frozen in [`rust/oracles/model/`](../../rust/oracles/model) (`answers.tsv.zst`, 1.4 MB, cut as
     the front oracle's; two Java runs byte-identical); the 87,564 texts (2.9 GB) are archived outside git
     (`model-texts-java.jsonl.zst`, 55 MB, its SHA-256 checked by decompressing it).
   - *The generators retargeted.* The two that write `builtin`'s Java tables gain a Rust output from the same inputs,
     so the two cannot drift at an upstream bump: `NativesGenerator --rust` reads `Pure.java` as `//spec:gen_natives`
     writes it and writes `rust/lite-builtin/src/natives.rs` (`//spec:gen_natives_rs`: the 891 constants' signature
     texts in the file's load-bearing order, each constant by its Java name as a `NativeId` -- so a constant the
     catalog lacks does not compile, as in Java -- and the 513 overload groups); `DynaFnGenerator --rust` writes
     `dyna_fn.rs` from the engine's registries (`//spec:gen_dynafn_rs`, 232 names). Both held by
     `//rust/lite-builtin:update_generated` in `//:generated`. The resources (`prelude.pure`, `engine-handlers.tsv`)
     are read by Rust as the same committed files (`include_str!`); `CoreImports.java` is the compiler's, the names
     slice's.
   - *By hand in `lite-builtin`:* `Pure` (the catalog parsed through the door at the platform's dialect on first read,
     its indexes, the lite vocabulary, the type FQN constants, the native classes and enumerations, the walled
     natives), `DynaFnDecisions` (its 191 decisions) with `DynaFn`'s methods, `EngineHandlers`, `Prelude`, `NativeFn`
     (the 21 families and `OperatorRun`), `SystemMetamodel` (its source written by the ported builders, byte for byte),
     `Subsumed`, `TdsLegacy`. Not ported: `DecisionProbe`, the untangle's measurement probe (a `ServiceLoader` hook, a
     no-op unless `LL_SHADOW` is set). `lite-platform` ports the whole package (`CoreFn`, `Feature`, `WalledBodies`,
     `DeclarationTable`, `Implementation`, `Registrations`, `ImplementationTable`, `PlatformPure`), with
     `ImplementationTableTest`'s, `DeclarationTableTest`'s and `FormOwnershipTest`'s cases as unit tests.
   - *The tables, held the same way.* [`rust/oracles/builtin/BuiltinAnswers.java`](../../rust/oracles/builtin) (a
     source program beside the frozen files, as the type oracle's is) dumps every table of `builtin` and `platform` one
     fact per line -- a record as the codec's JSON, a set Java keeps hashed written sorted -- and
     `//rust/lite-builtin:builtin_tables_test` and `//rust/lite-platform:platform_tables_test` write the same lines
     from Rust: **8,722 of 8,722** (the catalog with the dynafunctions, the handlers, the prelude, `Subsumed` and
     `TdsLegacy` 4,079; the families 2,741; the system metamodel 1,902) **and 5,028 of 5,028** (the platform: the
     declaration table over the catalog's, the prelude's and the system metamodel's functions, 1,078 ids; the
     implementation table over it and the registrations the platform holds; the Java tests' tables). Frozen with their
     MANIFEST, two runs byte-identical.
   - *Subtyping, held to the live Java (ruling 22).* `lite_model::subtyping` walks `PureModelContext.isSubtype`'s
     reach over class declarations; `lite_builtin::lattice` holds the boot layer's. `SubtypeProbe.java` asks the live
     Java's compiled boot model every ordered pair of its 509 class FQNs (with `Nil` and two absent names: 259,081
     pairs); `tests/subtype_live.rs` (ignored, run by hand: not frozen) holds the lattice to it. **The 366 children
     whose walk meets only FQN-spelled supertypes agree on every pair (186,294); the other 143 meet a supertype the
     prelude spells by its simple name under its section's imports** (`EngineRuntime extends Runtime`), which only
     name resolution answers -- held apart, not guessed, for the names slice.
   - *Gates.* `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings` and `cargo fmt --check`
     clean. Bazel `test //... --keep_going` at `b73168539`: 581 targets, 579 passed, 2 skipped; of
     `rust/BASELINE.tsv`'s 550 passing targets every one passes but the three gone with their code and ledgered
     (`//core:core_tests_ide`, `//core:core_tests_lexer`, `//docs:update_dialect_capability_test`); 32 targets new
     since the baseline, the port's, all passing, ten of them this slice's. `ledger_check.sh c4a39e06d`: 80 rows ok,
     none missing.
   - *The ten fixed (the owner's ruling of 2026-11-12 on this slice's report: fix every Java behaviour that looked
     wrong, in Java and in Rust).* Each confirmed first -- what Java did, what it should do, and where "should" comes
     from -- then one commit per item, Java and Rust together, the tables re-dumped from Java (two runs
     byte-identical) and Rust held to them. (1) `PlatformPure.adopt` keeps the declaration's pre- and
     post-conditions; measured first, inert: no adopted declaration carries one (the boot layer adopts none; upstream's
     43 declarations at the platform's names carry none), so S94 and S89 cover it; a platform version with a condition
     is refused (`f206e51fa`, +70 table lines). (2) The six "dangling" FQNs each exist upstream at exactly that name
     (tds.pure, validation.pure, eval.pure, dbExtension.pure, extension.pure): they dangle only in the dump's table
     over the boot declarations, and spec's `ImplementationTableTest` holds the whole table to none -- no Java change,
     the dump says what its 16 are (`fd638cdce`). (3) `families()` lists `RowGetter` once (`f8d74a601`). (4)
     `OperatorRun`'s id map refuses an overload in two runs, `indexById`'s rule (`f2fefe45a`). (5) `RowGetter.of`
     reads applied spellings only, pinned for every getter (`013a80c24`). (6) The system source spells Pure's `'\n'`
     and has no blank line before the Database's `)`: five line breaks fewer, every value the same, positions alone
     moved; every path by which a system position can reach a frozen text was traced and none is taken (a raise's
     position, a plan's scope id, a function body's content id, the stores' key), the plan, identity and MIR oracles
     not regenerated (`5f88f8bb8`; builtin 8,722 -> 8,717 lines, platform 105 hashes). (7)
     `duplicates()` reports each id once (`d64f9a4c6`, one line). (8) A `JavaRoutine`'s class member is an owner and a
     property together (`e1092f457`). (9) `GroupAggregate` needs no `of`, held to its two readers (`fa7b21d88`). (10)
     `isBivariateMap` takes a resolved callee's id, never null (`793b86949`). Items 5, 8, 9 and 10 are invariants named
     and pinned, not wider types; 2 to 5 and 8 to 10 move no table line. The tables now: builtin 8,717 of 8,717,
     platform 5,098 of 5,098. Gates: `cargo test --workspace`, clippy `-D warnings` and `fmt --check` clean; Bazel
     `test //... --keep_going` on the tree of `5f88f8bb8` (item 6 not yet committed, as the stop condition asks) but
     for one comment line of `rust/oracles/builtin/MANIFEST.tsv` corrected after the run, which no target reads:
     581 targets, 579 passed, 2 skipped, every baseline target passing but the three ledgered;
     `ledger_check.sh c4a39e06d`: 81 rows ok, none missing (the governance test's two new cases ledgered).

