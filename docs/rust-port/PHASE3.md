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
   - *The frozen oracles re-measured after the ten (2026-11-12).* Item 6's tracing was then measured: step 0's probe
     (`rust/tools/identity-hits-probe.patch`, applying cleanly with no offsets) run once over the freeze's lanes at
     `d6c3f18cf`, in the verify worktree, `--nocache_test_results`, 27 min 55 s, the two probe-caused failures of the
     freeze and no other. **Of the 73,411 frozen keys, all 73,411 answered and equal, none moved**: 67,752 rows of
     counts, 167,833 MIR ids, 25,741 execution plan ids and 25,597 test plan ids, every id re-hashed to its text, the
     mask applied 0 times, every lane's key count run A's. The keys run R adds are the 16,851 the freeze left out by
     design (the Postgres lanes, the temporary-path models) and 24 of `//core:ladder_report`, which ran because its
     inputs changed (2 of its 26 keys are frozen, and equal). So nothing between the freeze at `1ea8fbf45` and
     `d6c3f18cf` -- the flip, the parser's last rulings, slice 1b and the ten -- moved a Java answer; no re-freeze.


   **The names slice: the model index and name resolution (done 2026-11-12;** [`PHASE3_CRATES.md`](PHASE3_CRATES.md)
   §7**).** Step D as the crate `lite-names` (`compiler/names`, the split's first part). No Java production code changed
   but the import generator's second output; nothing calls the new crate yet.

   - *Placement* (PHASE3_CRATES.md §7's table, measured by imports and qualified names): `NameResolver`, `BareNames`,
     `ResolvedNames`, `CoreImports`, `SynthFqn`, `ModelBuilder`, `SymbolTable`, `TableIndex`, `StoreLookups`,
     `RelationalKinds`, `DerivedProps` and `KnowledgeLayer` are D's (they reach `builtin`, `platform`, `model`,
     `protocol`, `error` and `compiler_element_type`'s `Type.Primitive`); `KnowledgeLayer.columnKind` (F's
     `RelationalTypeInference`), `StatementInline` (F's `ModelContext`, G's `Calls`, `SourceSubst`) and
     `LiteralMapUnroll` (G's `SourceSubst`) wait for their slices, and with them `Compiler.resolveQuery`, the query front
     door that runs them after the resolution. `DecisionProbe`'s hooks are no-ops unless `LL_SHADOW` is set: not ported.
     The guard took the line `split compiler names element spec` as it stood (it checks that a crate's part is declared,
     never that every part has a crate), so no guard changed.
   - *The crate* (`rust/lite-names`, 5,039 lines of source, 44 generated, 330 of unit tests; 950 of differential and
     live tests): the resolver (`resolver/`: the tiers, the call decision, every element and value-specification
     walker), the index (`model_index.rs`: registration, the lazy indexes, the include-closure store lookups, the view's
     main table and lift, the legacy sets by id), the kernel (`knowledge.rs`), the boot lattice (`lattice.rs`, moved from
     lite-builtin), `bare_names.rs`, `resolved_names.rs`, `synth_fqn.rs`, `derived_props.rs`, `relational_kinds.rs`.
     The walkers answer `None` for Java's `==` input and rebuild as Java's constructors rebuild, the convenience
     constructors' dropped components included (the list below). The records' own methods the resolver calls are where
     Java has them: the protocol records' in `lite-front/src/record_methods.rs`, the model's in
     `lite-model/src/relational_operation.rs`.
   - *The generator retargeted.* `ImportsGenerator <CompileContext.java> <out> --rust` writes
     `rust/lite-names/src/core_imports.rs` from the same `CompileContext.META_IMPORTS` (`//spec:gen_imports_rs`), held by
     `//rust/lite-names:update_generated` in `//:generated`.
   - *Identity, as step 0's census classified each D site:* `model.ModelBuilder.indexed.*` (b) is `same` on the slot's
     `Arc`; `ModelBuilder.findAssociationAtClass` (c) `same`; `model.NameResolver.resolvedViews` (c) an `IdMap` keyed by
     the schema view's `Arc`; `model.TableIndex.inSchemas` (b) an `IdSet`; `DatabaseDefinition`'s default-schema reads
     slice 1b's. Two facts the next slice relies on (the census of E and F, 2026-11-12) are pinned by tests: the index
     hands out the database's own view record from `findView`, `viewsOf` and every spelling (E's `LiftedViews` keys
     identity maps by it: `a_view_is_the_databases_own_record_from_every_lookup`), and holds the very records it is
     given, the boot layer's among them, a batch added again adding nothing (F's `CheckedLayer` recognizes them by
     identity: `the_index_holds_the_boot_layers_own_records`).
   - *Java's hash order* reaches no answer of D: every `HashMap` of `ModelBuilder`, `KnowledgeLayer` and `NameResolver`
     is only looked up, and the maps whose order is seen (`duplicateElements`, the primitive extensions, a legacy
     mapping's classes, a runtime's rebuilt bindings) are insertion-ordered in both. One order outside D was found:
     `PureModelContext.resolutionUniverse`'s iterator, a `HashSet`'s, which `testplan/JudgmentFacts` iterates (F's and
     the test planner's; listed for the owner).
   - *The live differential* (`//rust/lite-names:names_differential_test` against core's
     `frontoracle/NameAnswers.java`). **Models**: every model text of the front oracle at every dialect it is asked at
     that the door parses -- 74,292 asks over 26,324 texts -- each one canonical text of six resolutions and an index:
     `B` `resolveAlongside(parsed, bootFqns, null)` (the strict compile's), `T` the same tolerant, `P` and `Q` the bare
     `resolve` strict and tolerant, `K` and `L` the association adoption strict and tolerant, `X` the index of `L`'s model
     asked every question E and F ask it (by reading their calls into `ModelBuilder` and `KnowledgeLayer`), every record
     named by its place. 74,280 strict resolutions answered and 12 refused. **Queries**: every (model, query) pair of the
     identity oracle whose texts the front oracle holds -- 31,297 pairs, 20,857 query texts, 2,321 model texts -- each
     resolved `N` with no model (the sectionless scope: shim function 9's null model), `M` against the model's universe
     with no imports, `I` with the model's union imports. **Every text identical to Java's: 74,292 of 74,292 and 31,297
     of 31,297**, with no list of differences.
   - *The model's universe for a query* is the step-D index (the adopted model's elements with the boot layer's resolved
     elements added, asked as `PureModelContext.resolutionUniverse` asks), not the compiled model's, which also holds
     Phase E's products. Measured in the same Java run against the live compiled model's universe
     (`Compiler.compileModel(text).resolutionUniverse()`): **31,297 of 31,297 pairs answer the same**, every model
     compiling.
   - *Subtyping.* `PureModelContext.isSubtype` does not delegate to `KnowledgeLayer`: it walks the compiled classes on
     its own (`Nil` the bottom, a primitive's lattice walked, a failing class the question's failure) and asks the kernel
     for the lookup alone; `KnowledgeLayer.isSubtype` is E's other relation (raw heads, a primitive no class, no bottom).
     Java keeps two relations; Rust keeps both as answers over one walk (`lite_model::subtyping::reach`, in F's shape or
     E's), the kernel calling it. The boot lattice moved to `lite-names` (it resolves the boot layer's names first, as
     `Compiler.boot()` does), and the live check (`tests/subtype_live.rs` against `SubtypeProbe.java`, run by hand) now
     answers **all 509 classes, 259,081 pairs (1,898 subtype), identical**; slice 1b's 143 held apart are gone.
   - *Frozen*: [`rust/oracles/names/`](../../rust/oracles/names) -- `models.tsv.zst` 2.1 MB (74,292 lines: the text,
     the dialect, the canonical text's SHA-256) and `queries.tsv.zst` 1.6 MB (31,297 lines: the query, the model, the
     hash), each with its SHA-256 in `MANIFEST.tsv`; two Java runs byte-identical (A with the live universe check, 20 min
     43 s; B writing the texts, 19 min 4 s). The texts (4.3 billion characters of model forms, 7,476,103 index lines among
     them; 51 MB of query forms) are archived outside the repository,
     `~/.local/legend-lite/oracle-archive/names-model-texts-java.jsonl.zst` (53 MB) and `names-query-texts-java.jsonl.zst`
     (1.5 MB), their SHA-256 in `names-texts-java.sha256` beside them, each checked by decompressing it. Among the
     answers: 12 strict resolutions refused (an ambiguous name), 1,175 asks where the bare `resolve` answers otherwise
     than `resolveAlongside` (the boot layer's names), 116 adoptions and 15 refused, 32 indexes refused; `M` equals `N`
     on every pair and `I` differs from `M` on 183 -- a measurement of this corpus, whose `t:` queries spell their user
     elements fully qualified, not a property of the resolver (a bare user name under a core-import package would
     differ). Under Bazel the differential takes about 4 minutes (237 s, optimized): the largest Rust test in
     `//gates:core` by far (the model differential's is about 25 s), within `large`'s 900 s; whether it belongs in the
     local gate or in CI alone is the owner's.
   - *Not held to a Java answer by the differential* (each a composition of pieces it does hold): `built_call_names`
     (G's `Calls` asks it), `resolve_names` (`Compiler.resolveNames`' wrapper), `resolve_with_universe` (the
     lower-level `resolve(model, knownFqns)`, no prelude), `ModelIndex::derived`, `knowledge::canonical_table`,
     `ModelIndex::retain_legacy_surface`; the unit tests (`src/tests.rs`, 12) hold `built_call_names`, `resolve_names`,
     `derived` and `canonical_table` to what their pieces answer.
   - *Gates.* `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings` and `cargo fmt --check`
     clean. Bazel `test //... --keep_going --local_test_jobs=2` on the tree of `76a191f11` (the crate, the oracle, the
     tool; the docs beside it are no test's input, G17): 584 targets, 582 passed, 2 skipped; of `rust/BASELINE.tsv`'s
     550 passing targets every one passes but the three gone with their code and ledgered (`//core:core_tests_ide`,
     `//core:core_tests_lexer`, `//docs:update_dialect_capability_test`), no status moved; 35 targets new since the
     baseline, the port's, all passing, three of them this slice's (`//rust/lite-names:lite_names_test`,
     `:names_differential_test`, `:update_generated_test`), and `//core:core_tests_lib` compiling `NameAnswers.java`.
     `lite_names_test` and the guards passed again on `5278bce01` (the compositions' unit tests, added during the run).
     `ledger_check.sh c4a39e06d`: 82 rows ok, none missing.
   - *Java behaviours that look wrong, ported as they are* (each a candidate for the owner, none changed):
     1. A resolved model drops its unclaimed sections and element spans when any element changed (`NameResolver.java`
        :257-259, the six-argument `ParsedModel`); the adoption drops the spans (`KnowledgeLayer.java`:478-480).
     2. The convenience constructors drop components on a rebuild: a name reference's, a relation column's, a
        parameter's, a lambda's, a variable's, a collection's, a type annotation's, an element pointer's and an applied
        property's position (`NameResolver.java` :645, :689, :889, :1987, :1968, :1937, :2052, :1860/:1875, :1903, the
        last also its owner class); an enum value's positions, so it becomes a node (:1879); a `ColSpec`'s position,
        declared type and multiplicity and annotations (:2043); a relation shape's spelled name and spans (:2060); a
        `NewInstance`'s multiplicity arguments (:1917); a constraint's external id, owner and position (:904); a
        function-reference realization's source (:1063); an embedded property mapping's primary key (:1363); a named
        relation-function mapping's primary key (:1224); a primitive extension's type variables (:409).
     3. A runtime's connection ids resolve only when something else in it changed (:1492-1496); two binding keys that
        resolve to one store merge into one entry (:1483).
     4. Not walked: an enumerated-expression property mapping's relational operation (:1331), a generic's type-variable
        values (:653), a quoted tree call's tree (:1838).
     5. A call position asks the call tiers for a qualified name the universe does not know (:337-349), though its
        comment says a qualified spelling matches no tier.
     6. A database registered twice keeps the first one's filter, join or view index when the second declares none
        (`ModelBuilder.java`:435-484); `findAssociationOf` climbs bare-name supertypes only (:784) where the
        direct-subclass index takes generic heads too; a join-only view's root reads the join in its database's own
        joins, not the include closure (:1198).
     7. `KnowledgeLayer.rawName` spells a non-name type by Java's record `toString` into a refusal's message (:526).
     8. `PureModelContext.resolutionUniverse` (F's) answers membership with the extensions and the function ids, but
        iterates and counts without them, in a `HashSet`'s order (`PureModelContext.java`:315-341), and
        `testplan/JudgmentFacts.java`:117 iterates it: a hash order reaching a plan (TRIAGE C1), outside this slice.
   - *The owner's choices this slice made* (PHASE3_CRATES.md §5, items 12 to 17): the records' methods where Java has
     them (`lite-front`, `lite-model`); subtyping one walk with Java's two relations as its shapes, the boot lattice in
     `lite-names`; what waits for F and G; the query differential's step-D universe; a generic's type-variable values
     refused rather than spelled in `rawName`'s message; the process-wide memos as `Mutex<HashMap>`. And the
     differential's own shape: the canonical text's six resolutions (the bare `resolve` beside the compile's
     `resolveAlongside`), the index asked by labels of place, the query pairs from the identity oracle's `t:` keys with
     the runner's section imports not recoverable from a key (no imports, and the model's union imports, instead).
   - *Open for the owner* (raised by the census of E and F, 2026-11-12, `port-scratch/efcensus/REPORT.md`): whether G's
     `Calls` (the call builder, about 120 uses in E, reaching only names-tier code) and F's `RelationalTypeInference`,
     `ViewSignatures` and `columnType` move into `lite-names` (with them, `KnowledgeLayer.columnKind` would be D's); and
     whether `PureModelContext.refuseDuplicates` belongs beside `duplicate_elements`. Not decided here: the brief's
     placement stands until the owner rules. (The first two ruled the same day, yes, `MilestoningStrategy` with them:
     the follow-up below. `refuseDuplicates`, ruled with ruling 28, moved in the names fixes below.)
   - *The follow-up (2026-11-12, the owner's rulings on the census's asks 1 and 2;* [`PHASE3_CRATES.md`](PHASE3_CRATES.md)
     §8*).* Five classes Java keeps in `compiler/spec` and `compiler/element`, which reach only names-tier code and
     which E calls first, are `lite-names`'; no Java production code changed, nothing calls the new code yet but tests.
     1. *What moved*, line for line, Java's refusals and messages included: `Calls` → `calls` (149 lines; its nine
        builders; `builtCallNames` was ported with the slice), `RelationalTypeInference` → `relational_type_inference`
        (264; `int` arithmetic wrapping as Java's, a function name lowered as `toLowerCase(Locale.ROOT)` lowers it),
        `ViewSignatures` → `view_signatures` (214; its types in the caller's arena, the views being expanded an `IdSet`
        as Java's identity set), `StoreCompiler.columnType` → `store_compiler` (50; the class's `tableSchema` waits for
        `lite-element`, `declaredType` for its first caller), `MilestoningStrategy` → `milestoning` (92); and
        `KnowledgeLayer.columnKind` → `Knowledge::column_kind`, which the slice had left out. None reaches F (the brief's
        stop condition met nothing); no crate edge moved. Java's hash order reaches none of their answers: the sets they
        keep (`columnKind`'s `seen`, `ViewSignatures`' identity set, the built-call memo) are only asked.
     2. *The edges*: `KnowledgeLayer`'s names → element crossing is gone, and so is the `Calls` half of names → spec
        (`StatementInline`'s one `Calls.platform`); `StatementInline` (F's `ModelContext`, G½'s `SourceSubst`) and
        `LiteralMapUnroll` (`SourceSubst`; it never reached `Calls`) still wait for `lite-spec`. E's normalizer → spec
        and → element crossings (`Calls`' 102 sites in 11 files, `LiftedViews`' `ViewSignatures`, `MilestoningFacts`')
        are gone with them.
     3. *The guard*: `rust/lite-guards`' `calls_are_built_only_by_the_builder` (PHASE3_CRATES.md §8) -- outside the
        parser's crate, a call record is built only in `lite-names/src/calls.rs` (3, pinned exactly) and the resolver's
        `src/resolver/values.rs` (1), and nothing names the parser's call factories; Java's ArchitectureTest rule stays
        for Java.
     4. *The differential* (`NameAnswers.java` and `tests/names_differential.rs` extended, two forms of their own so
        the frozen ones are unchanged). **Stores**, per model ask over the same index `X` asks (74,292; 74,260 with an
        index): every class's stereotypes' milestoning strategy (16,826: 3,954 business, 1,213 processing, 1,215
        bitemporal, 10,444 none), every table column's Pure type (357,625; 36 refused, a decimal's scale past its
        precision), every view's signature as `LiftedViews` asks it (2,809; 24 refused, two columns of one name),
        every view column's inferred SQL type with the scope and the index, with no scope and with no lookups (11,308
        columns), and the column kind of every column of every table or view spelling over the include closure, one
        absent column included (1,485,730). No stack overflow met; no cyclic view in the corpus (the unit test holds
        the refusal). **Calls**: 14,454 shapes -- every name the builder's 206 call sites build (read from them: E 102
        in 11 files, G 98 in 20, validation 5, `StatementInline` 1), at the arities they use (0 to 5 where a site's
        arguments are another call's), the names a site computes enumerated from the table it computes them from (the
        parse functions, the comparison shims, the legacy ranks, the sort directions, the dynafunctions' rows), a bare
        name through `exact`, a full name through `platform`, a sweep of every bare name the platform's tables know
        (the catalog, the engine surface, the forms, the prelude, the dynafunctions and their shims) at 0 to 8, every
        form at 0 to 4, and originals through `like` and `renamed` with no record, their built record, another and
        both, plain and with every written form set (`platform` 6,504, `like` 3,380, `renamed` 2,452, `infixRun` 732,
        `among` and `infixRunAmong` 478 each, `form` 370, `exact` 58, `carrier` 2). **Every text identical to Java's
        on the first run: models 74,292 of 74,292, stores 74,292 of 74,292, queries 31,297 of 31,297, calls 14,454 of
        14,454**, with no list of differences (cargo, release: 270 s).
     5. *Re-frozen* ([`rust/oracles/names/`](../../rust/oracles/names), its MANIFEST): run A (19 min 16 s) and run B
        (`--texts`, 19 min 35 s) byte-identical on all four files. **The answers frozen before are unchanged**:
        `models.tsv` and `queries.tsv` hash again to `4421f233...60b3bc` and `b5f77c9f...83b5b`, their `.zst` files
        untouched, and run B's texts to the archived `f3b5ac9a...721b` and `a5419b2b...6bff6`. New: `stores.tsv.zst`
        (1.4 MB, `d60c472c...4e37` uncompressed) and `calls.tsv.zst` (0.4 MB, `3d84bb66...0f984`); their texts (103 MB
        and 8.5 MB) archived as `oracle-archive/names-stores-texts-java.jsonl.zst` (1.7 MB, `4aa71030...1557`) and
        `names-calls-texts-java.jsonl.zst` (114 KB, `816451c4...f0efb`), each checked by decompressing it.
     6. *Java behaviours that look wrong, ported as they are* (each a candidate for the owner, none changed): (a)
        `RelationalTypeInference` has no cycle guard: a computed view column over a cyclic view chain recurses until the
        stack is gone (`RelationalTypeInference.java`:257-288, entered from `KnowledgeLayer.java`:406 and
        `ViewSignatures.java`:136; `ViewSignatures` guards only its own column-reference arm, `columnKind` only its own);
        Java throws `StackOverflowError`, Rust aborts -- the corpus has no such view. (b) `safe` lets a decimal beat any
        other type, a `VARCHAR`, a date or a `BIT` included, though its comment says "DECIMAL beats int/double/float"
        (:201-206), and spells a lone `NUMERIC` operand as `DECIMAL` (:201-206). (c) `sub` with no inferable operand is
        `INTEGER` (:86-87), a default; a concatenation's operand of no string type adds 0 to its size, so
        `concat(INT, 'ab')` is `VARCHAR(2)` (:94-105). (d) `max`, `min`, `distinct` and the string transforms read their
        first argument alone (:50-52, :75-78). (e) `ViewSignatures` types a column found nowhere `Any[0..1]` (:102) and a
        column with no inferred type `Any` (:137) -- documented as the engine's rule; an aggregate is recognized at one
        argument only (:104), so a `count()` of none is `Integer[0..1]`. (f) `columnKind`'s walk guard is the string
        `db + "@" + table + "." + col` (`KnowledgeLayer.java`:382): two triples that concatenate alike are one. (g)
        `MilestoningStrategy` answers nothing for an unknown `temporal.*` stereotype (:34, "the historical
        set-membership tolerance").
     7. *The owner's choices this follow-up made* (PHASE3_CRATES.md §5, items 18 to 22): `StoreCompiler` split by
        method; a module per moved class, column names into the type family as UTF-8; Java's missing cycle guard kept;
        the guard a source scan; the differential's new questions as forms of their own, `columnType` asked by
        reflection, the call shapes as listed.
     8. *Gates*: `cargo test --workspace` (630 passed, 0 failed, 44 ignored), `cargo clippy --workspace --all-targets
        -- -D warnings` and `cargo fmt --check` clean. Bazel `test //... --keep_going --local_test_jobs=2` on the tree of
        `8e2e0304a` (the code, the oracle and the guard; the docs beside it are no test's input, G17): 584 targets, 577
        passed, 2 skipped, 5 timed out -- the machine slept mid-run (37,354 s of wall clock), and each of the five
        (`//tools/guards:inventory_test`, whose own log says its one test passed, `//parser-equivalence:parser_parity`,
        `//core:core_tests_resolver_judge_both`, `:core_tests_normalizer_judge_both`,
        `//rust/lite-names:names_differential_test`) passed when re-run alone with the same flags (249 s; the
        differential 248 s). So of `rust/BASELINE.tsv`'s 550 passing targets every one passes but the three gone with
        their code and ledgered (`//core:core_tests_ide`, `//core:core_tests_lexer`,
        `//docs:update_dialect_capability_test`); 35 targets new since the baseline, all passing, `//rust/lite-guards:guards_test`,
        `//rust/lite-names:lite_names_test` and `:names_differential_test` among them. `ledger_check.sh c4a39e06d`: 82
        rows ok, none missing.
   - *The names fixes (2026-11-13, the owner's rulings 25, 28 and 29 on the slice's and the follow-up's "Java behaviours
     that look wrong"; the rows, every measurement and the held-back items' before and after:
     `port-scratch/namesfix/ROWS.md`).* Each item confirmed first -- what Java did, what it should do, and where "should"
     comes from (legend-engine 4.145.0's or legend-pure 5.99.0's pinned sources read, the code's own contract, a test) --
     and its reach measured: an instrumented build (never committed) counted, per site, the inputs that reach it and those
     whose answer the fix changes, over the names differential's asks (74,292 model asks, 31,297 query pairs) and over the
     front oracle's 8,794 Database texts compiled and seeded (`MetamodelSeeds`, so `RelationalOpRows` over every mapping
     expression and view column). One commit per item or coupled pair, Java and Rust together, each with its Java and Rust
     tests; the names oracle re-frozen in a commit only where its answers moved (two Java runs byte-identical, the texts
     archived with their SHA-256), lite-names held to every frozen answer on the first run. Then step 0's probe over the
     freeze's lanes, as the re-check ran it: four commits moved frozen keys, and were reverted (the stop condition).
     1. *Fixed, no answer moved* (NameAnswers at their state answering the frozen files byte for byte; the final probe
        equal): items 8 and 13 (`b25d64101`: the type inference refuses a view-on-view cycle by name, `ViewSignatures`'
        words, an identity set of the view columns being typed -- `infer` and `column_kind` answer a `Result` in Rust; the
        column-kind walk keys on its triple, a collision a text cannot make and a protocol model can); item 7
        (`eee2e98b2`: a qualified property returning a type that names no class is refused by its kind in words, never a
        record's `toString`; the port's refusal of a generic's values and its `toString` spelling deleted); item 5
        (`03b791378`: a qualified call matches no call tier); item 3 (`59f7c6a9a`: a runtime's ids resolve whatever else
        changed; two keys naming one store keep both lists -- the engine refuses several connections per store, a check
        lite makes for no runtime); items 9, 10's `sub` and 11 (`2be83bf8d`: a decimal is safe with the integer and float
        kinds alone -- the item's "lone NUMERIC comes out DECIMAL" is the engine's own rule, unchanged; `sub` has no
        default; `max`/`min` over several take the engine's safe type; `distinct` and the string transforms read their
        first argument as the engine's do).
     2. *Ruling 28's placement*: `refuseDuplicates` (`e4771f8b3`: `ModelIndex::refuse_duplicates` beside
        `duplicate_elements`, held by a question of its own, `duplicates.tsv`: 123 of 74,292 asks refused, frozen from two
        byte-identical runs; Java's stays where it is). The names oracle ends as the follow-up froze it, with
        `duplicates.tsv` beside it (its MANIFEST says which re-freezes the reverts took back).
     3. *Held back by the stop condition, reverted* (each landed with its tests and, where its answers moved, its names
        re-freeze; the probe then moved frozen keys and a revert commit took it back; the owner's to land):
        - Item 10's concat (`c4baaca19`, reverted by `371aef52e`) and item 4 (`5d162ee91`, reverted by `8f4c8003c`)
          moved 914 test plan ids in the corpus lanes (`bin/spec/judge_{host,database}_{duckdb,h2}`): every corpus plan
          seeds the system metamodel, and its rows moved -- concat's `Varchar(2)` type row for
          `simpleRelationalMappingWithConcat`'s `Order` property 0 (a `Varchar(2)` and an `Integer`) gone and the
          `DynaFunction` row's type reference null; 12 `TableAliasColumn` rows of enumerated expressions (the enumeration
          tests' `employeeTestMapping*`, the model-to-model tests' `relationalMapping`) naming their database by its full
          path (`column:myDB|...` -> `column:meta::relational::tests::mapping::enumeration::model::store::myDB|...`); the
          system database's `graphKey` with them. Landing either is a re-freeze of the plan oracle. Measured, not read
          off the rows: the probe at the item 4 revert moved no test plan id.
        - Items 1 and 2 (`d90670102`, reverted by `eaa5c1a27`) moved no answer but renamed 2,823 frozen keys: 2,801 of the
          4,535 `vs:` query ids (a query with no text is named by the SHA-256 of its resolved spec's `toString`, and a
          rebuilt node now keeps its positions) and 22 of the 24 `L` set keys built from them; grouped by kind, model,
          door and use, all 194 groups' new keys answer as the absent ones did, as multisets (counts, MirJson ids, plan
          ids). Landing them is a re-key of the identity and plan oracles, or a probe key that no `toString` makes. With
          them goes their last part, held back before: a rebuilt primitive extension's type variables
          (`NameResolver.java`:409), which flips acceptance (`Primitive test::IntCap(x:Integer[1]) extends Integer` used
          as `test::IntCap(2)` is refused today, "Wrong type variables count (1)", and would still be refused for
          another reason, "'2' is not an instance of Integer": a type variable's own type is never resolved, a class's
          no more than a primitive's; `test::IntCap()` is accepted today and would be refused).
        - Item 6 (`bca1e2eb3`, reverted by `bf26e88e4`) moved no plan, MirJson or test plan id and no names answer, but two
          counts of the identity oracle: `CallInstantiation.equals.frame` 32/0 -> 32/1 (`toPostgresModel`'s
          `testConvertSelectSQLQuery`, the host judge on DuckDB) and 64/0 -> 64/1 (`testConvertJoinTreeNode`, the
          database judge on H2), reproducibly (two full probes) and in one lane per test, the other three as frozen.
          Bisected with the corpus lanes alone (each lane its own JVM): as frozen at `631d4e0bf`, `b25d64101` and
          `59f7c6a9a`, both moved at `bca1e2eb3`. A hypothesis the reproduction supports: the probe counts
          `UserCallInliner`'s `CallInstantiation.equals`, which a `HashMap` calls when two keys' hashes are equal, and the
          record's `hashCode` is made of `System.identityHashCode`s (`UserCallInliner.java`:132-146), so a change that
          hashes one more object by identity on the lane's thread before these tests changes which distinct keys
          collide. That pair of counters (`.frame`, `.origin`) measures identity-hash collisions, not planning: a Rust
          port cannot be held to it, and the owner may drop or mask it as `executionTraceID` is masked; landing item 6
          is that, or a re-freeze of the two counts.
     4. *Register rows, for the owner* (ruling 29's third policy): S376 (item 12: `ViewSignatures`' `Any`s and its
        one-argument aggregates), S377 (item 14: an unknown `temporal.*` stereotype is no strategy at E, refused at F in
        the engine's words), S378 (the inference's other differences from `getSafeType` found while confirming 9 to 11,
        and concat's, held back).
     5. *Guards*: the full suite found `//parser-equivalence:parser_parity` failing on the new Java test snippets
        (`bc493b435`). Item 7's case wrote a function type as a qualified property's return, grammar the engine's
        parser refuses, which three own-corpus guards keep to declared hosts; no guard was widened -- the case sets the
        function type on the parsed records instead. `own_corpus.matched` 3025 -> 3038: the standing fixes' snippets
        that join the own corpus and match the oracle (a `rust/RETIRED_TESTS.md` row). No other pin, ratchet or
        allowlist moved. `//rust/lite-names:names_differential_test` was `size = "large"` already (since
        `76a191f11`), in `//gates:core`; its BUILD comment says so now. PHASE3_CRATES.md §5 items 16 and 20 are marked
        overtaken.
     6. *The probe* (`rust/tools/identity-hits-probe.patch`, applying with hunks offset and no fuzz; the re-check's
        scripts, `--nocache_test_results`, the verify worktree; each full run 143 of 145 targets passing, the two
        probe-caused failures of the freeze, `//core:census` and `//core:guardrails`, and no other). At `c4baaca19`
        (every fix in): of the 73,411 frozen keys, 70,588 answered, 69,674 equal, 914 test plan ids moved and 2,823 keys
        absent. At `8f4c8003c` (items 4 and concat reverted): no plan, MirJson or test plan id moved, the same 2,823
        absent, and item 6's two counts. At `eaa5c1a27` (items 1 and 2 reverted too): all 73,411 answered, 73,409 equal,
        item 6's two counts. At `bf26e88e4` (item 6 reverted too), and again at `bc493b435` (item 7's test changed, the
        code otherwise the same; 22 min 34 s): **of the 73,411 frozen keys, all 73,411 answered and equal, none moved**
        -- 67,752 rows of counts, 167,833 MirJson ids, 25,741 execution plan ids and 25,597 test plan ids, every id
        re-hashed to its text, the mask applied 0 times, the lanes' table the re-check's line for line (the 16,875 keys the
        freeze left out by design). `check_manifest.py`: the identity and plan oracles' files all as their MANIFEST rows.
        Each run's 35-38 GB folder deleted; the verify worktree left clean.
     7. *Gates* (at `bc493b435`; the docs beside it are no test's input, G17): `cargo test --workspace` (637 passed,
        0 failed, 44 ignored), `cargo clippy --workspace --all-targets -- -D warnings` and `cargo fmt --check` clean.
        Bazel `test //... --keep_going --local_test_jobs=2` (1,121 s): 584 targets, 582 passed, 2 skipped, none failed
        or timed out; of `rust/BASELINE.tsv`'s passing targets every one passes but the three gone with their code and
        ledgered (`//core:core_tests_ide`, `//core:core_tests_lexer`, `//docs:update_dialect_capability_test`); the 35
        targets new since the baseline all pass. `ledger_check.sh c4a39e06d`: 84 rows ok, none missing.
   - *The names landings (2026-11-13, the owner's rulings 32 to 34, PHASE3_SHIM_DESIGN.md §10; every measurement and
     tool in `port-scratch/namesland/`).* What the names fixes' stop condition held back lands, in Java and Rust
     together, one commit per item, the names oracle re-frozen in each commit where its answers move (two byte-identical
     Java runs, the texts archived), lite-names held to every frozen answer; then the one re-freeze of the probe's
     oracles, every moved key attributed to its item by measurement.
     1. *A key Rust can compute* (`3f0bea95e`). The probe keyed a query with no text by the SHA-256 of the RESOLVED
        spec's Java `toString` (`vs:`), and a model given as records or as a client's PureModelContextData by theirs
        (`p:`, `protocol:`). The probe's v2 (`rust/tools/identity-hits-probe-v2.patch`, beside v1) keys them by the input
        AS GIVEN, before resolution, in the lossless record codec's form, the form lite-front's and lite-names' writers
        write byte for byte: `spec:` the resolve door's input (`NameResolver.resolveQuery`'s query,
        `resolveQueryIn`'s `[query, imports]`, `Compiler.resolveQuery`'s `[[statements], imports]`), `parsed:` a
        `ParsedModel`, `pmcd:` a PureModelContextData. The doors keep weak references compared by `==` (no identity hash
        asked), the keys computed at JVM exit. The protocol emitter's JSON, the first candidate, was measured beside it
        and lost: 193 rows refused (no rule for a new instance on a variable), 13 groups of old keys with differing
        answers collapsed. ONE run (K1, at `de483e694`, no fix applied, every lane): all 73,411 frozen keys answered by
        their old keys and equal; over its 180,217 rows the old -> new map is a bijection (4,665 components renamed, no
        split, no collapse); 4,659 frozen keys renamed (`vs:` 4,535, `p:` 40, `protocol:` 46 + 19, 22 of 24 `L` set
        keys), 68,752 kept; `vs:too-deep` 0 rows, frozen and in the run; all 8,762 no-text scopes came through a door.
        The identity and plan oracles re-keyed by the map (archived, `oracle-archive/identity-rekey-map-2026-11-13`,
        SHA-256 `5df32e62...`), held to K1 by the new keys: 73,411 of 73,411.
     2. *The identity-hash counters dropped* (`c467dc205`): `CallInstantiation.equals.frame` and `.origin` leave the
        frozen identity oracle as `executionTraceID` is masked -- a counts mask at freeze and at compare (84 rows, 69
        tables -> 67), the probe still writing them.
     3. *The held-back fixes, re-applied* (each a revert of its revert, its test cases back, the oracle files of a
        conflicted revert written from the runs, never from either side): item 4 (`05f5682e6`; models 18 lines), item
        10's concat (`6b98c3dfb`; stores 11 lines, the original's freeze confirmed byte for byte), items 1 and 2 with the
        root door's three span drops (`ec4889721`; models 55,336 and queries 7,422 lines, the names fixes' freezes
        confirmed; the Java case's unclaimed section now set on the records, since its `###Unknown` text grew an
        own-corpus leniency ledger), item 6 (`d46c99057`; no names answer moved).
     4. *A primitive extension's type variables, fully* (`c38aee9fb`): carried through the rebuild and every type
        variable's declared type resolved, a class's too; the engine's four cases (legend-pure `TestPrimitiveCompile`,
        `TestNewInstance`; legend-engine `CompileContext.newGenericType` :756-771) now answered as the engine answers
        them (three compile, `IntCap()` is refused in the engine's words). Models 21 lines (every one at the platform's
        dialect); the builtin and platform tables re-dumped from Java, byte for byte as frozen.
     5. *Item 3 measured* (`bed5107ce`): 30 of the names differential's asks (5 runtime-store pairs in 3 texts) bind
        several connections to one store under one key, none under several keys; so the merge stays, with register row
        S379 (the engine refuses both: `HelperRuntimeBuilder.java:197-205`), and `RuntimeDefinition`'s javadoc says
        what each does.
     6. *S376 and S378 fixed to the engine* (`4307039aa`, `a1c8d3dbb`; rustfmt `ea9c4759d`). S376: a view reading a
        column or table its database has not is refused in the engine's words, plain or nested (`Can't find column
        'c'`, `Can't find table 'T' in schema 'S' and database 'D'`), every such text already refused for another
        reason; aggregates typed at any arity; stores 36 lines. What remains in the row: a column no rule types is
        `Any[0..1]` (the engine has no type). S378: the safe type the engine's exactly, count BIGINT, the string
        transforms a sized VARCHAR, `sub`'s rule, no rule for `group_concat`; stores 266 lines (799 view columns, every
        one a count INTEGER -> BIGINT). What remains in the row: the operators', `sum`'s and `average`'s own rule shapes,
        `case`'s assertion, a float literal, `upper`/`lower` -- found while fixing, the owner's. S377 accepted as it is.
     7. *Guards* (`c67d9552e`): the full suite at `a1c8d3dbb` found three guards red, each on code these landings
        added; no guard was widened. `ErrorShapeGuardrailTest.defaultLiteralFallbacksOnlyShrink`: S378's size, scale and
        integer-digit helpers ended in `default -> 0`, now the engine's rules written out (a CHAR's or VARCHAR's size, an
        OTHER's 0, any other kind refused; `getDecimalScale`'s and `getDecimalIntegerPrecision`'s 0 for a kind no NUMERIC
        or DECIMAL), lite-names' `size` refusing the same kind. `CodeShapeGuardrailTest.classNamesAreNeverLogic`: `sub`'s
        rule compared `getClass()`; the raw kind is an enum from an exhaustive switch, the safe-type map keyed by it.
        `ParserBoundaryArchTest`: `NameResolverTest`'s cases for items 1, 2 and step 4 named the parser's `Dialect`; they
        parse through the test door (`Own.model`), as item 4's does (a `rust/RETIRED_TESTS.md` row). Measured unchanged:
        NameAnswers at that commit answers all five names files byte for byte (one run), the builtin and platform tables
        re-dumped as frozen, and F9 (below) answers the re-frozen oracles exactly. No pin, ratchet or allowlist moved in
        that commit; across the landings one ratchet rose, `own_corpus.matched` 3038 -> 3063 (item 4 +6, concat +1, items
        1 and 2 +4, item 6 +8, step 4 +5, S376 +1: each commit's new test elements joining the own corpus and matching
        the oracle, by `update_ratchets`, each a `rust/RETIRED_TESTS.md` row).
     8. *The one re-freeze, attributed* (`e2d9e87e8`): two complete probe runs at `a1c8d3dbb` (Z1 and Z2, every lane,
        143 of 145 targets passing each, the two probe-caused failures; Z2's corpus judge passes re-run as Z2c by a probe
        copy differing by one unused constant, since the action cache had answered them with Z1's outputs) agree on every
        key. Attributed by measurement, state against state (`rust/oracles/plan/attribution-2026-11-13.tsv`): item 4
        moved 914 corpus test plan ids (the system-metamodel seed's enumerated-expression columns naming their database
        in full; C4 against K1), item 10's concat the same 914 again (the seed's concat type row; C10 against C4), items 1
        and 2 nothing (F12, every lane: absent 0), item 6 nothing (C6; the six PCT channel-B lanes, P6all), S376 and S378
        nothing (C376; Z1 against C376 and against P4all), and step 4 the only move outside the corpus: PCT channel B
        walls a legend-pure source that does not compile, and `essential/lang/cast/cast.pure` (whose `a::P(8)` lite
        refused) now compiles, so its six lanes' models have new sources and new `s:` ids -- 1,719 keys renamed (each
        answering as before but one BOTH-judge test plan, which now seeds cast.pure's classes), the second wall round's 5
        model rows dropped, the first round's 5 model rows' counts moved (P4all against P6all; P4all equal to Z1 on those
        lanes, 1,725 keys). And 7 keys added, by the freeze's own rule (every key both runs answer alike is frozen, but
        the Postgres lanes', the temporary-path models' and `//core:ladder_report`'s): step 4's test case builds the
        engine's four primitive-extension cases through `buildModel`, 4 model rows and the three that compile's
        `compileAllBodies` rows, no MirJson or plan id -- placed by `//core:core_tests_compiler` under the probe at
        `d46c99057` (none), `c38aee9fb`, `4307039aa` and `c67d9552e` (all 7, equal). Nothing moved or was added that no
        item explains. Re-frozen: `identity/hits.tsv` 73,411 -> 73,413 rows (5 dropped, 7 added), `mir-ids.tsv` and
        `plan/plan-ids.tsv` re-keyed for the renamed rows; 784 new test plan ids (403 host, 381 database judge), the host
        store rebuilt by `rust/tools/test-plan-texts.py` from a new archive (twice byte for byte, checked: every plan its
        id, the archive's SHA-256, the host rows' ids exactly), both archives kept outside the repository with their
        SHA-256; no execution plan or MirJson id moved; `rust/oracles/mir` is render answers no item touches (its Bazel
        tests pass). *Confirmed*: F9, a third complete run at `c67d9552e`, against the files before the 7 were added:
        **all 73,406 answered and equal, none moved, none absent** (180,232 rows, 90,288 distinct keys, 143 of 145
        targets, the two probe-caused failures; its 16,882 run-only keys the 16,851 the freeze leaves out by design, 24
        of `//core:ladder_report` and the 7); `check_manifest.py`: every identity and plan file as its MANIFEST row. Every
        run's folder deleted; the verify worktree left clean.
     9. *Gates* (at `e2d9e87e8`; the docs beside it are no test's input, G17): `cargo test --workspace` (644 passed,
        0 failed, 44 ignored), `cargo clippy --workspace --all-targets -- -D warnings` and `cargo fmt --check` clean.
        Bazel `test //... --keep_going --local_test_jobs=2` (1,716 s): 584 targets, 582 passed, 2 skipped, none failed
        or timed out; of `rust/BASELINE.tsv`'s passing targets every one passes but the three gone with their code and
        ledgered (`//core:core_tests_ide`, `//core:core_tests_lexer`, `//docs:update_dialect_capability_test`); the 35
        targets new since the baseline all pass. `ledger_check.sh c4a39e06d`: 85 rows ok, none missing.
   - *The names follow-ups (2026-11-13 and 14, the owner's standing ruling 34, PHASE3_SHIM_DESIGN.md §10; the engine's sources
     this work's only spec; every measurement and tool in `port-scratch/namesfollow/`).* What the names landings left in
     register rows S376 and S378, in Java and Rust together, each item measured first (an instrumented build, never
     committed, or a check over the names differential's asks), the names oracle re-frozen in its commit where it moved
     (two byte-identical Java runs, the texts archived), lite-names held to every frozen answer; then the one attributed
     re-freeze of the probe's oracles.
     1. *S378's remainder* (`04dc555aa`, corrected by `c59296c31`). `plus` over one argument widens it (TINYINT and
        SMALLINT an INTEGER, INTEGER and BIGINT a BIGINT, a DECIMAL or NUMERIC its own kind capped at precision 31, FLOAT
        and DOUBLE a DOUBLE; relationalExtension.pure:1409-1437); `plus` and `minus` over several their first argument's
        type when a safe type over all of them is possible (:1409-1437, :1209-1217); `times` the safe type of its first
        two (:1773-1781); `divide` and `average` DOUBLE (:693-701, :291-299); `sum` by its first argument's kind, DECIMAL
        and NUMERIC capped at 31, FLOAT and DOUBLE a DOUBLE, no rule for REAL (:1717-1741); a float literal FLOAT (:148);
        `upper` and `lower` no rule. The pair rule (the safe type over every argument) is gone. `04dc555aa` read four of
        `sum`'s five rules (its read stopped at :1735) and typed a FLOAT or DOUBLE nothing; the corpus probe at that
        commit caught it -- the engine's own `testSumAggregationWithJoinInference` (testRelationalExtension.pure:186-192,
        `sum` over a FLOAT, expects DOUBLE) failed on the host judge, and `//spec:corpus_{duckdb,h2}_verdict` with it --
        and `c59296c31` corrected it, every other cited rule re-read through its closing bracket. `case` keeps no type where
        no safe type is possible: the engine asserts there (:401-413) whatever `failOnMatchFailure` is, but its inference
        runs at plan generation (relationalMappingExecution.pure:205-208, from :96, :101), never when the database
        compiles (HelperRelationalBuilder.java:469-517), and lite infers when the model compiles and when it seeds the
        metamodel, where a failing inference would refuse or break what the engine compiles -- the form lite cannot
        spell, S378. Reach (the corrected rule against the landings' state): over the 8,794 seeded Database texts, 1
        text's op type row (an `average` over an INTEGER now DOUBLE), no column kind, signature or compile outcome; over
        the names differential's 74,292 asks, 15 asks (a `sum` over a SMALLINT now INTEGER); `plus`, `minus`, `times`,
        `divide` reached and unchanged; `upper`, `lower`, a float literal unreached. Names oracle: stores.tsv 220 lines at
        `04dc555aa` (the misread's), 205 back at `c59296c31`, 15 (5 texts) from the landings' answers; MANIFEST entries 1
        and 3.
     2. *S376's join terminal* (`edc11a648`). The engine compiles a join navigation's terminal as any other operation
        once the navigation's joins resolve (HelperRelationalBuilder.java:1181-1186: `getJoin` first, :1255, :167-192,
        then every `TableAliasColumn` through `getRelation` and `getColumn`, :1170-1171; a `{target}` column a
        self-join's, unchecked, :1162-1168), in the database of its last join (RelationalParseTreeWalker.java
        :1024-1044): it refuses a missing terminal, in S376's words. `ViewSignatures.requireColumns` now walks the
        terminal in the navigation's database once every hop's join resolves (the store lookups gain `findJoin`; a
        missing join is left to the lift, refused first). Reach (ViewCheck3): 2,174 terminals over the 74,292 asks, 12
        references found nowhere (2 texts), every one where a join is missing too: nothing moves; one Java run at the
        commit answered all five names files as frozen. `own_corpus.matched` 3063 -> 3065 (the case's two databases).
     3. *Register rows* (`3b988d325`, `afefe2e8e`): S378 FIXED where lite now matches the engine, the row describing
        only what still differs -- (1) the form lite cannot spell (an argument no rule types fails the engine's
        inference, the rules' own `inferRelationalType($rop)` being `failOnMatchFailure` true, :120-123; `case`'s
        assertion), (2) found while fixing, the owner's (116 of the map's names lite has no rule for, `joinStrings`'s
        size, a Boolean or date literal, four rules only lite has, a name matched in lower case); S376 FIXED for the
        terminal, what remains no rule (`Any[0..1]`), and two wordings found (a missing join, a missing schema); S379's
        status kept by ruling 32's second branch, its kind an outcome (the coordinator's word, 2026-11-14).
     4. *The one re-freeze, attributed* (`6e502d466`; `rust/oracles/plan/attribution-2026-11-14.tsv`). The probe v2 over
        the corpus lanes at each commit (NF0b at f391b32d0, NF1 at 04dc555aa, NF2 at edc11a648, NF3 at c59296c31; a run
        whose code an earlier probe run already built takes a copy of the probe differing by one unused constant, the
        landings' Z2c way, so the action cache cannot answer its corpus judge passes -- NF0's first try collected nothing
        for that reason) and two complete runs at c59296c31 (NZ1, NZ2, each its own tagged copy; 143 of 145 targets
        passing each, the two probe-caused failures). The base answered as frozen (NF0b: 16,202 corpus keys, all equal).
        NZ1 and NZ2 agree on every key: all 73,413 frozen keys answered, none absent, 914 test plan ids moved and nothing
        else (no count, MirJson or execution plan id), nothing added (`//core:ladder_report`'s 24 and each run's 16,851
        temporary-path keys by design). The 914 are item 1's (NF3 against NF0b; NF2 against NF1: item 2 moved 0): every
        corpus plan seeds the system metamodel, and its two op type rows of legend-engine's sqlFunction testMapping --
        `asin(divide(int1, 2))`, `acos(divide(int1, 2))`, testSqlFunctionsInMapping.pure:787-788 -- moved INTEGER ->
        DOUBLE (the engine's `divide` rule where the pair rule took the safe type), the system database's graphKey with
        them: one plan's text diffed row by row, the frozen archive's against NZ1's. 04dc555aa alone moved the same 914
        differently and failed the corpus verdicts; never frozen. Re-frozen: `plan/plan-ids.tsv` (914 test plan ids;
        783 new ids, 403 host and 380 database judge), the host store's four files rebuilt from a new archive by
        `rust/tools/test-plan-texts.py` (twice byte for byte, checked: every plan its id, the archive's SHA-256, the host
        rows' ids exactly), both new archives kept outside git with their SHA-256; `identity/hits.tsv` and `mir-ids.tsv`
        re-derived byte for byte as frozen, not rewritten; `rust/oracles/mir` untouched (its Bazel tests pass).
        `check_manifest`: every identity and plan file as its MANIFEST row. Each run's folder deleted; the verify worktree
        left clean and detached at c59296c31.
     5. *Gates* (at `6e502d466`, the re-freeze; the docs beside it are no test's input, G17): `cargo test --workspace` (646
        passed, 0 failed, 44 ignored), `cargo clippy --workspace --all-targets -- -D warnings` and `cargo fmt --check`
        clean. Bazel `test //... --keep_going --local_test_jobs=2` (1,758 s): 584 targets, 582 passed, 2 skipped, none
        failed or timed out; of `rust/BASELINE.tsv`'s passing targets every one passes but the three gone with their code
        and ledgered (`//core:core_tests_ide`, `//core:core_tests_lexer`, `//docs:update_dialect_capability_test`); the 35
        targets new since the baseline all pass. `ledger_check.sh c4a39e06d`: 85 rows ok, none missing. Found, not
        fixed (the owner's, to be scheduled under ruling 34; in rows S376 and S378): an argument no rule types and a `case`
        with no safe type (the form lite cannot spell); 116 of the engine map's names lite has no rule for, `joinStrings`'s
        size, a Boolean or date literal, the four rules only lite has, a name matched in lower case; a missing join and a
        missing schema refused in lite's own words; a `{target}` terminal in a view refused by lite's lift.
   - *The names batch (2026-11-14, the owner's rulings 34 and 35, PHASE3_SHIM_DESIGN.md §10; the engine's sources this
     work's only spec, every rule's answers ASKED of the engine; every measurement and tool in `port-scratch/namesbatch/`).*
     What the follow-ups recorded in rows S376 and S378 and did not fix, in Java and Rust together, before the normalizer
     slice (E) freezes its oracle, so E freezes on the engine's answers. Each item measured first by an instrumented
     build riding its new code (every node and call of the inference compared with the commit before's; the view
     signature's switch and refusal instrumented where an item reaches them), the names oracle run at each item's code
     (all five files as frozen each time, nothing re-frozen), lite-names held by the same cases as Java. Each rule's
     every arm was asked of legend-engine 4.145.0 -- the pinned release's own generated `inferRelationalType`
     (failOnMatchFailure false and true), on the engine runner's class path after its own compiler, over one Database
     whose every view column is a case (123 cases; the engine's own 21 type-inference tests pass there first) -- and
     the same cases through the legend-lsp daemon (interpreted, a newer checkout whose rule texts are the pinned tree's):
     both answer all 123 alike. The cases are KnowledgeLayerTest's and tests.rs' `ENGINE_CASES`, one text and one table
     on both sides; a case the engine fails is no type here (S378).
     1. *The 30 rules the seeds use* (`b275440ae`; ruling 35): `abs`, `add`, `bitAnd`, `bitShiftLeft`, `bitXor`, `cbrt`,
        `coalesce`, `convertVarchar128`, `datePart`, `dayOfMonth`, `dayOfYear`, `firstDayOfMonth`,
        `firstDayOfQuarter`, `firstDayOfWeek`, `firstDayOfYear`, `firstHourOfDay`, `firstMinuteOfHour`,
        `firstSecondOfMinute`, `if`, `log`, `lpad`, `md5`, `mod`, `monthNumber`, `replace`, `sha256`, `sign`, `sin`,
        `sqrt`, `weekOfYear` (relationalExtension.pure:193-1891, each cited in the code) -- the follow-ups' list
        (seeds-M2.tsv), exactly 30, and the list by use too: once they type their arguments no other of the map's names
        is reached. Reach: 38 seeded texts' 913 op type rows from no type to a type; no column kind, view signature or
        compile outcome; no ask of the names differential. The follow-ups' 39th text reaches only a `coalesce` over an
        array, which the engine fails (`Not supported yet: ...LiteralList`): no type either way. The other 86 names are
        row S378's, ported when a text first uses one. `own_corpus.matched` 3065 -> 3066.
     2. *The four rules only lite had* (`4edba3458`): `like`, `position`, `charindex`, `locate` have no rule. Reach: 3
        seeded texts' 24 op type rows INTEGER -> no type.
     3. *The name matched exactly* (`96985abbb`), in the inference and in `ViewSignatures`' aggregate switch
        (`->get($d.name)`, :114); lite-names' `java_string` went with its last callers. Reach: none at either site (the
        switch reached in 223 seeded texts and 667 asks, never by a name spelled otherwise).
     4. *`joinStrings`* (`cc7169c8c`): the map's rule (:1031-1039), a VARCHAR of max(4000, its first argument's CHAR or
        VARCHAR size), no rule for another first argument. The brief cited inferRelationalType's `JoinStrings` arm
        (:173, the parts' sizes summed): that arm types the operation only the engine's query translation builds, never
        a parsed `joinStrings`; the engine's answers are the map rule's. Reach: none.
     5. *Boolean and date literals* (`ad3affee6`, record only): neither grammar writes one in a relational operation (the
        engine's `constant` is `STRING | INTEGER | FLOAT`, RelationalParserGrammar.g4:216; both parsers refuse such a
        text); none in the front oracle's 52,143 distinct texts, and no JSON text with a Boolean relational literal. No
        parser change (ruling 29). A Boolean through the JSON door would be typed nothing (the engine's BIT): an
        inference-only arm, the owner's.
     6. *The missing schema* (`bbcb4da95`): a view column read through a schema neither its database nor an included one
        declares is refused `Can't find schema 's' in database 'D'` (validateSchemaExists, HelperRelationalBuilder.java
        :348-354), the database's own name, asked of the engine. Raised by `ViewSignatures`, not the lift. Reach: none
        (its refusal reached in 12 seeded texts and 36 asks, every one through a schema that exists).
        `own_corpus.matched` 3066 -> 3070.
     7. *`case` with no safe type, an argument no rule types* (`a59b05abb`, no code change; ruling 35): no type when the
        model compiles; the engine's refusal of a query reading such a column, made when it plans it, is carried to the
        resolver and lowering ("Carried to the slices not yet begun", at this file's end).
     8. *Register rows* (`8e9e0718c`): S378 and S376 FIXED where lite now matches, what still differs exact.
     9. *The one re-freeze, attributed* (`b62d7d6e8`; `rust/oracles/plan/attribution-2026-11-14-names-batch.tsv`). The
        probe v2 over the corpus lanes at each commit (NB0 at badd55f2e, NB1 b275440ae, NB2 4edba3458, NB3 96985abbb, NB4
        cc7169c8c, NB6 bbcb4da95; each its own tagged copy of the probe, every one passing its 12 tests) and two complete
        runs at bbcb4da95 (NBZ1, NBZ2, each its own copy; 143 of 145 targets passing each, the two probe-caused
        failures). The base answered as frozen (NB0: 16,202 corpus keys). NBZ1 and NBZ2 agree on every key: all 73,413
        frozen keys answered, none absent, 914 test plan ids moved and nothing else, nothing added. The 914 are the
        corpus plans' system-metamodel seed, moved by item 1 (NB1 against NB0: the seed's op type rows for the 30 rules'
        nodes in legend-engine's test mappings, 17 in one plan's text diffed row by row) and again by item 2 (NB2
        against NB1: the propertyfunc mappings' `position` rows, 6); items 3, 4, 6: 0. Re-frozen: `plan/plan-ids.tsv`
        (914 ids; 783 new, 403 host and 380 database judge), the host store's four files rebuilt (twice, byte for byte,
        checked), both new archives kept outside git with their SHA-256; `identity/hits.tsv` and `mir-ids.tsv`
        re-derived byte for byte as frozen; `rust/oracles/mir` untouched. Each run's folder deleted.
     10. *Found, the owner's, E's to settle first* (row S376): since item 1 a view column using `datePart`,
        `firstDayOfMonth`, `firstDayOfWeek`, `firstDayOfYear`, or `coalesce`/`if` over mixed kinds, is refused when its
        lifted function is compiled (a query over the view, or `compileErrors`), where the engine compiles it: the
        signature now carries the engine's type (DATE a StrictDate; FLOAT, DECIMAL), the lifted body Pure's (Date,
        Number); `case`, `max`, `plus`, `minus` over mixed kinds were refused so before the batch (74 one-view models,
        the engine compiling every one). No view column in the texts we hold uses one. Widen the signature or relax the
        lift's check: the owner's choice, ruled 2026-11-14 (ruling 36: the check accepts one family's two types, for a
        lifted view's function only; carried to the typer's slice, at this file's end). Also E's, from S376: a missing
        join in lite's words, and a `{target}` terminal the lift refuses.
     11. *Gates* (at `b62d7d6e8`, the re-freeze; the docs beside it are no test's input): `cargo test --workspace` (651 passed, 0 failed, 44 ignored), `cargo clippy --workspace --all-targets -- -D warnings` and
        `cargo fmt --check` clean. Bazel `test //... --keep_going --local_test_jobs=2` (1,580 s): 584 targets, 582 passed, 2
        skipped, none failed or timed out; of `rust/BASELINE.tsv`'s passing targets every one passes but the three gone
        with their code and ledgered (`//core:core_tests_ide`, `//core:core_tests_lexer`,
        `//docs:update_dialect_capability_test`); the 35 targets new since the baseline all pass. The early gates after
        each item (core's compiler, integration, root, architecture, normalizer and planner suites, the guardrails and
        census, the ratchets, lite-names' targets and from item 3 every Rust target, the corpus verdict lanes and their
        roster tests) all passed, the own-corpus ratchet once moved with items 1 and 6.
        `ledger_check.sh c4a39e06d`: 85 rows ok, none missing.

   **The normalizer slice: step E as `lite-normalizer` (done 2026-11-14;** [`PHASE3_CRATES.md`](PHASE3_CRATES.md)
   §9**).** E's Java made a refusal wherever a user's text reached a raw JDK exception, its one hash order declared,
   its two unreached Phase-D fallbacks deleted, then frozen (D then E, 74,364 asks), then ported family by family and
   held to the freeze byte for byte. Every measurement and tool is in `port-scratch/normalizer/`.

   - *Step 1, the raw exceptions (ruling 29's second policy; one commit per site or group, each with a
     `RefusalsNotCrashesTest` case).* A discovery run of the oracle's tool at the base named every raw exception a
     text reaches in D then E: `d2eacfead` ROT:610 (a relational lambda or `$x` at the translator's default arm, an
     ISE naming the Java class) and `59a5331ce` ROT:720 (a translated dynafunction no arm rewrites) are
     `NotImplementedException`s by name now (poison-only in a strict build on the property-mapping path: a deferred
     refusal, as their UNSUPPORTED sibling); `cdb60ee0f` an inline-embedded property of an unknown class and
     `f25760b47` a join hop to a table its database does not declare (`JoinChainEmission#3`, 3 front-oracle texts) are
     the model's errors; `a32e83c36` an included mapping's unknown association (`AssociationSynthesis#1`, 14 texts,
     tolerant only) is "Can't find association 'x'"; `33b0c97f0` MN:943/:1058's bare `orElseThrow()` (an association
     D left unresolved in a tolerant build) are "XStore/ModelJoin mapping references unknown association"; `7afe52b16`
     MN:1088, MJN:59 and MJN:117-118 (a ModelJoin's two lambda parameters of one name, a nested hop to a Pure end) are
     refused before the `Map.of`s, MJN:59 pinned as an invariant. `FallbackLedgerTest`'s never-fired floor 5 -> 3 and
     the own-corpus ratchet 3070 -> 3134 (the 64 elements of the new test texts, 58 of RefusalsNotCrashesTest's 14 and
     the M2M case's 6; the parser unchanged, so no existing element moved), each with its dated comment and row. Left
     unchanged, names- and types-tier code E calls (the owner's): `Type.RelationType`'s duplicate column through
     `ViewSignatures` (6 texts, 18 asks a mode) and `KnowledgeLayer.superClass`'s ISE (4 texts, 11 asks a mode) -- the
     29 raw refusals the freeze holds; and `resolveSynthesized` throws D's `ResolutionException`, unattributed, out of
     a module build (a Pure set's `~src` an ambiguous name).
   - *Ruling 29's first policy* (`56fc09f1e`, register S380): `MappingPrePass`' M2M index is a `LinkedHashMap`, so the
     "Circular M2M ~src chain" text starts from the first-declared class (declaration order over a sort: the mapping's
     own order, no UTF-16 sort for Rust to copy); no frozen oracle held the text.
   - *Ruling 30's measurement and deletions* (`ad86fa895`). The tool over an uncommitted instrumented build, every ask
     strict and tolerant (asks reaching, S / T): AS `resolveAssociation`'s import wildcards tried 241 / 265, found 0 / 6
     (the 6 the slice's own case texts, step 1's tolerant crash); its same-package fallback tried 259 / 277, found 0 / 0;
     MN's poison-key `.orElse(am.associationName())` 207 / 218; MN's binding `.orElse` 0 / 0; SSR `qualifyStoreRefs`
     on 5,276 / 5,300 asks, an unqualified spelling no database has on 709 / 727, rewriting 0 / 0. Deleted, each a named
     refusal now (no text reaches either, so no frozen hash moved): the binding's `.orElse` ("Can't find association")
     and the rewrite ("Mapping references unknown database 'Db' (name resolution left it unqualified; the import 'p::*'
     names 'p::Db'); mapping=M"). Kept and ported verbatim, each with its count in a dated comment: the import
     wildcards, the same-package fallback (tried, never found: the owner's) and the poison key's `.orElse`.
   - *The oracle* (`5656ecd6f`, ruling 31; `rust/oracles/normalizer/`, its MANIFEST). `frontoracle/NormalizerAnswers`
     drives D then E only (`resolveAlongside`, the association adoption, `ModelBuilder.from`, `refuseDuplicates`,
     `ModelNormalizer.normalize`; never F) over every model text of the front oracle at every dialect asked and the
     slice's 24 case texts -- ten for census §10's oracle-blind sites (view-on-view cycles mapped and alone, a walled
     view reached from a mapping, two equal views in two schemas, a real `~src` chain, M2M cycles of three and of two in
     both orders, a duplicate set id across includes, a nested ModelJoin hop) and step 1's 14 -- strict (the first
     error) and tolerant (the walls, then the answer): slice 1b's codec over the `NormalizedModel` (every element, the
     binding tables, `NormalizationFacts` in insertion order, `resolvedStores`, the imports, the legacy surfaces).
     74,364 asks over 26,348 texts; strict 69,104 answered and 5,260 refused -- 188 before E (33 by D, 155 by the index
     or `refuseDuplicates`) and 5,072 by E (4,986 ModelException, 57 NotImplementedException, the 29 raw); tolerant 144
     refused (32 before E, 112 by E), 1,739 with walls E added. Two runs in parallel on one jar
     snapshot byte-identical (741 s, 743 s); every one of the 283 record types reached is in the Rust codec; each case's
     form shows what it was written for. The forms (3.4 GB) archived outside git
     (`oracle-archive/normalizer-forms-java-2026-11-14.jsonl.zst`, 36.6 MB, with the per-ask tags; SHA-256 in
     `normalizer-texts-java.sha256`, checked by decompressing). Reach, in texts (asks) of the front oracle, strict: a
     legacy mapping 7,379 (21,665), table-backed Relational sets 3,967, sets with no main table 1,094, M2M 1,324,
     relation functions 471, unions 251, inheritance 50, aggregation-aware 25, JSON-source sets 99 (278 asks: counted
     from the answers' `RelationalSource$Json` bindings, since the index bakes a runtime's JSON connection into the set
     and the input tags read what E was handed); ~filter 323, ~groupBy 42, ~distinct 72, extends 310, includes 707
     (store substitution 67), enumeration mappings 511; association mappings Relational 1,534, XStore 404, ModelJoin
     164; views 367 (lifted 266); clean-sheet mappings 40; derived properties 2,015, constraints 1,053, defaults 153,
     function PRE/POST 37, services 888; poisons 353, mixed unions 25, union key threads 94, routed targets 63, resolved
     stores 18; E walls (tolerant) 579. No text's lifted body calls `like`, `charindex`, `locate` or `COUNT`/`SUM`/`ABS`
     in capitals (the names batch's G refusals: 0 texts; the three that hang on argument types -- `mod` over floats,
     `md5` over an integer, `sub` over varchars -- were not counted).
   - *The crate* (PHASE3_CRATES.md §9). A skeleton first (every module, shared type and cross-file signature; the
     frame ported: the error type, `Pipeline`, `MappingLedger`, `MappingFacts`, `ResolvedMapping`, the memos, the
     `DynaFn` match), then eight helpers filled the families in parallel on disjoint files, each line for line against
     ad86fa895, and one wrote the differential; every branch cherry-picked here, their worktrees removed. Census §3's
     DON'T PORT rows are out, §4's RE-EXPRESS rows applied (the census's `Option<&MappingLedger>` is `&mut`: US:392
     writes a poison through the pipeline). Java class names in answer texts are spelled per variant: AS:418's
     association mapping kinds, AS:441's and JCE:195's and GBS:164's property-mapping kinds, `targetClassFqn`'s type
     expression kinds, US:613's set kinds, ROT's `literalToValueSpec` (`java.math.BigInteger`, `java.math.BigDecimal`),
     `dynaFnName`'s `DynaFn.<NAME>`.
   - *The differential* (`tests/normalizer_differential.rs`, `//rust/lite-normalizer:normalizer_differential_test`,
     large, in `//gates:core`): every ask through `lite-names` and `lite-normalizer`, held to the frozen hash, with
     equal / mismatch / unported buckets and a report against the Java forms. **74,364 of 74,364 asks identical to
     Java's** at the first full run after the merge, no list of differences (48 s, release, 4 threads); a corrupted
     frozen hash is caught. The cases alone run under plain `cargo test`.
   - *Register rows* S381-S392 (census ASK 12's E rows, ported exactly, the engine's answers read from the pinned
     sources, none run; each for the owner). Read against the code's comments, S382's (Join property mappings are
     navigated over the grouped relation, and the engine's SQL carries a per-row column for `.all()` and an isolated
     grouped set), S386's (the engine's H2 `splitPart` drops empty tokens too) and S387's (the old H2 path) claims did
     not hold as written, and S391 is wider than the census says.
   - *Guards*: `ParkedWorkLedgerTest`'s PARK-1 and PARK-26 anchors restated against the Rust sites
     (`rust/lite-guards` `the_parked_work_anchors_hold_in_the_normalizer`: PARK-1's wall words in
     `xstore_pure_ends.rs` alone, the one function; PARK-26's list count in `store_substitution_rewrite.rs`);
     `DynaFnRegistryTest`'s invariants are `dyna_fn.rs`' unit tests; rows in `rust/RETIRED_TESTS.md`, the Java tests
     unchanged and running. `FallbackLedgerTest`'s funnel, `LegacyReachbackCensusTest`'s E rows,
     `CodeShapeGuardrailTest`'s E allowlists and `ShadowWalkerCensusTest` read Java's sources and have no Rust form
     (the funnel is an `Option` per site in Rust); they retire with the Java.
   - *The probe, under the stop condition* (after the last Java change, at `3a5297c80`; nothing under `core`, `spec` or
     `pct` changed since, and of the Rust the JVM loads -- `lite-ffi` and its crates -- only `lite-front` did, by two
     added modules of `impl` blocks only `lite-normalizer` calls, `normalizer_methods_e1.rs` and `_e2.rs`): the v2 probe, a copy tagged NORM1 (`namesfollow/make_variant.py`), over the full freeze in the
     verify worktree (`namesland/full_at.sh`, KEEP unset; the Bazel wrapper retried once after a transient remote-cache
     error, the second invocation 145 of 145 targets, 143 passing, the two probe-caused failures `//core:guardrails` and
     `//core:census`). `compare2.py` with the counts mask against this worktree's frozen oracles: **all 73,413 frozen
     keys answered and equal, none moved (counts, MirJson, execution and test plans), none absent**; 16,903 new keys,
     the run's by design (the parity lanes' and the ladder report's temporary paths) and the 25 of step 1's new test
     texts (`core_tests_normalizer`'s RefusalsNotCrashesTest and M2M case: 14 module builds, 11 compiles). The run's
     38 GB folder deleted; the verify worktree clean and detached.
   - *Gates* (at `b3079159e`, the slice's docs commit -- run on `c3573afcf`, its tree byte for byte, before the fill's
     commits were replayed so that every commit builds; the docs are no test's input, G17): `cargo test --workspace` (872
     passed, 0 failed, 47 ignored), `cargo clippy --workspace --all-targets -- -D warnings` and `cargo fmt --check`
     clean. Bazel `test //... --keep_going --local_test_jobs=2` (1,838 s): 586 targets, 584 passed, 2 skipped, none
     failed or timed out; of `rust/BASELINE.tsv`'s passing targets every one passes but the three gone with their code
     and ledgered (`//core:core_tests_ide`, `//core:core_tests_lexer`, `//docs:update_dialect_capability_test`); the
     37 targets new since the baseline all pass, `//rust/lite-normalizer:lite_normalizer_test` and
     `:normalizer_differential_test` (38.2 s) among them. After the last edits (the view-body test whole again, the
     poison key's dated count; the MANIFEST's and this entry's refusal split corrected), the crate's, the guards' and
     the oracles' targets again: 3 of 3 passing (the differential 36.1 s). `ledger_check.sh c4a39e06d`: 88 rows ok,
     none missing.
   - *Java behaviours that look wrong beyond census ASK 13, ported as they are* (lines at ad86fa895; each the owner's):
     ROT:247-256 the hash arm has no arity guard -- `md5()` is a raw `ArrayIndexOutOfBoundsException` from a user's
     text (step 1's list missed it), `md5(a, b)` drops `b` -- and ROT:323-330 `convertDate()` with no argument the same;
     JCE:843 `determineTargetTable`'s text doubles `mapping=` (`mapping=mapping=a::M`, or `mapping=store=<db>`) through
     `hopTarget`; JCE:624-633 `mintNavSlotAlias` drops a main table's schema; JCE:192-213 and MN:2111-2118 a primitive
     property passes "non-class property type" (refused a step later, or built as `new(@String, ...)`); JCE:415-419 a
     walled view refuses a routed navigation that never reads it; JCE:567 vs :585 one route collects the written join
     and translates the declared one; MN:1695-1699 the INNER-filter rebuild drops `sourceUrl` and any source override;
     MN:2562-2602 a view-on-view column re-resolves against the mapping's database, not its reference's; MN:340/:404 a
     `~func` set binds as `ClassBinding.Pure` (no relational source); MN:539 the clean-sheet door does not flatten
     enumeration mappings with includes; US:467-512 synthUnion's pair-entry merge reaches no answer, only its errors;
     AS:87 the pre-pass looks an association up directly where synthesis resolves it through imports (an import-only
     Relational association mapping is refused first); XS:140 a Relational end's pipeline is the class's main source
     even for a non-root set; MN:1080 and MJN:85-93 a ModelJoin's ends and its nested ModelJoin chosen by class, last
     match, own mappings only; RR:131/:184 `bindSrc` and `substVars` ignore lambda shadowing; MPP:73-78 a pre-pass
     ModelException naming another element walls that element, not the mapping; MPP:237-240 a class's last Pure set
     replaces the earlier, whose chain is not checked; SSR:40-43 one store substituted twice, the last wins; SSR:137
     a substituted Embedded block loses its `~primaryKey`; ModelNormalizer:171-172 PRE/POST bodies resolve under no
     imports (their owner is the function's name, the imports are keyed by its id); M2mRouteGuards:121-133 an empty
     declared set id is not the default; DC/ROT:671 `default.T` is normalized only in the column-reference arm. Also
     found while reading the engine (the register helper): lite refuses an M2M class-typed property whose class has no
     set ("targets unmapped class", MN:1235-1241) where the engine accepts one whose transform returns the property's
     type (`ClassMappingThirdPassBuilder.java`:103-109) -- a divergence with no row.
   - *The owner's choices, taken conservatively*: the same-package fallback kept (tried, never found); ROT:610/720 as
     NotImplementedException (deferred, as the sibling); the two raw sites in names- and types-tier code unchanged;
     `Pipeline`'s ledger `&mut` (US:392); ModelNormalizer:220's identity by statement nodes (a shared `Arc` from the
     adoption would make it `same`); MN:742's operand order by Java's equality key, which cannot wall two lines by a
     position accident as Java's `toString` order could (census §4's hedge; no text differs); `Unported` kept as the
     port's marker; the differential's four threads under Bazel; whether the differential (48 s) stays in the local
     gate.
   - *The normalizer batch (2026-11-14/15; rulings 29, 30, 34 and the owner's ruling of 2026-11-15 on ruling 30's
     frozen-answer clause, ruling 37 (a)).* The fixes the owner ruled for E, made after the slice and before F freezes,
     in Java and Rust together, each tagged ENGINE-SOURCED (the engine asked: legend-engine 4.145.0 on the engine
     runner's class path, `port-scratch/normbatch/engine`) or LITE-ONLY, each measured first by one instrumented build
     of the base (`port-scratch/normbatch/reach`, every normalizer-oracle ask strict and tolerant, `runs/reach.tsv`),
     each re-freezing what it moved in its own commit from two byte-identical Java runs, every moved ask attributed
     (`rust/oracles/normalizer/attribution-2026-11-15-normalizer-batch.tsv`, the MANIFEST's block). Item 1 (`59787797c`,
     ENGINE-SOURCED, run): a VARCHAR's size is a 64-bit integer, the engine's Integer (`RelationalDataType.Varchar(long
     size)`, every sum, doubling and maximum in Java and Rust): asked of the engine, the VREPL view's RP6 column, a
     VARCHAR(2000000000) replaced, is VARCHAR(4000000000); `KnowledgeLayerTest` and lite-names' table pin it (they
     pinned the wrapped -294967296) with five VBIG cases past an int; reach 0, no oracle moved. Item 2, census ASK 13's
     ten: `fbe06d321` (b) an unknown XStore / ModelJoin association is "Can't find association 'x'" (run; it was a
     NotImplementedException naming the kind; E 218 asks / 75 texts; its follow-up `b77a6bf47` mends
     `LegendLiteGapTest`'s association-mapping case, which `fbe06d321` broke: it asserts what XStore and ModelJoin do
     over a declared association -- they compile -- and the engine's refusal of an unknown one); `6419dec51` (a) an
     embedded block keeps its `~primaryKey` when an association entry is injected (read) and (c) a view-backed set's
     EnumeratedExpression and OtherwiseEmbedded read through the view's frame (run; E unmoved); `3c3e8b6f2` (e) the
     mapping facts name a union member and a routed target by the effective set id, the first match (run; E 216 / 72);
     `2ea106085` (f, g) an enumeration mapping id taken twice across a closure is "Duplicated enumeration mappings found
     with ID 'x' in mapping 'M'" and an unknown value "Can't find enum value 'C' in enumeration 'E'" (run; E 72 / 24);
     `d25682e95` (d, h; LITE-ONLY, moving nothing) declaredAssertion reads a join terminal's kind through a view and a
     Database declared twice lifts its views once; `202b4c624` (i) an include cycle is named by its members alone (run:
     the engine's "Cycle: w::A -> w::B -> w::A"; its walk is hash order, so lite's is the declared order, S393; E 6 / 2,
     the order of two A -> C -> B triangles); `b55232609` (j; LITE-ONLY, landed under ruling 37 (a)) an AggregationAware
     set's aggregate view sets keep their own declared keys (E 35 / 15; the one tagged ask unmoved is a view lift that
     fails first). Item 3 (`202b4c624`, PARK-26, run): the engine refuses one mapping included under two spellings as
     "Duplicated mapping include 'w::A' in mapping 'w::B'", by the resolved name, which lite already answered for every
     shape tried; the duplicate check now compares resolved names in Java and Rust, PARK-26 closed
     (ParkedWorkLedgerTest, PARKED_WORK_LEDGER, lite-guards' anchors, dated); E unmoved. Item 5 (`59475c162`, S376,
     run): a view's own missing join is the engine's "Can't find join 'J' in database 'D'" (E 6 / 2); a `{target}`
     terminal in a view is not ported -- the engine compiles the view and plans no query over it (a NullPointerException
     at plan), so lite has nothing to match without walling a view a strict build keeps; written in S376, reach 0. Item
     6 (measured and proposed, nothing changed): every G-refused spelling in 0 texts (like, charindex, locate, capitals'
     COUNT/SUM/ABS; mod over floats, md5 over an integer, sub over varchars: the 11 mod, 9 hash and 11 sub texts are
     integers and strings); the engine's SQL per call on H2 and DuckDB asked (like/charindex/locate refused at plan on
     both; capitals refused on H2, planned lower-case on DuckDB; mod over floats mod / cast(fmod as integer); md5 over
     an integer the dialect's hash; sub over varchars a minus); the proposal per call, for G with ruling 36, is in
     `port-scratch/normbatch/item6.md`. Item 7 (`029a63c12`, ruling 29's second policy): md5() and convertDate() with no
     argument and md5(a, b) refused by name (run: the engine compiles them and refuses at plan, so they stay deferred);
     `KnowledgeLayer.superClass`'s raw ISE is the engine's "Can't find type 'X'" (run; 11 asks / 4 texts); a view
     declaring a column twice is refused by name (run: the engine compiles it and fails at plan, "Cannot cast a
     collection of size 2 to multiplicity [1]"; LITE-ONLY wording, landed under ruling 37 (a); 18 / 6);
     `resolveSynthesized`'s ResolutionException walled at its element in a tolerant build (LITE-ONLY, reach 0) -- the
     names oracle re-frozen with it (models.tsv 74,260 lines, the lineage of an absent key now the superclass refusal;
     stores.tsv 18). Item 8 (`aa487f539`, ROT:610, run): a lambda in a relational expression anywhere but an array
     lambda function's second argument, and a `$x` out of every lambda, are the model's errors (the engine refuses the
     model: "Error in 'w::M'", "The lambda parameter 'x' is not in scope"); array_filter's lambda, which the engine
     compiles, stays a deferred refusal; ROT:720 (isNull with two arguments) stays deferred, as the engine compiles it
     and refuses at plan (E 9 / 3, the slice's cases). Item 9 (`fef8fa43f`, ASK 15, run): `resolveAssociation`'s
     same-package fallback deleted, as the engine refuses an unimported sibling ("Can't find association 'P_F'"); E
     unmoved, since D's own-package tier resolves such a name first (D's divergence, listed for the owner). Item 10
     (`847a12e7c`, Rust only but a Java comment): GBS:342 compares a Double literal by Java's bits; the model holds a
     derived property definition by `Arc` (FrontCodegen's rule, records regenerated), lite-names' adoption shares it and
     ModelNormalizer:220 is identity (lite-base's `same`); US:797 dedups class mappings by their codec text, every
     component equals reads (a source position too, which no two sets of one model differ by alone); a Rust unit test
     pins GBS's (`a_double_literal_matches_by_javas_double_equals`), none US's, and no text reaches either;
     `Thrown::Unported` and the `unported!` macro removed (the differential's UNPORTED bucket stays, empty); ad86fa895's
     comment slip fixed ("association lookups", Java and Rust); both differentials equal. Item 11 (`527419708`, S394,
     LITE-ONLY): Java compares an XStore association's two lines modulo AND/OR commutation by record equality
     (`MappingNormalizer.sameModuloCommutation`) where it ordered the operands by `toString`, positions included -- the
     accident shown on hand-built conditions against the base's jars (`port-scratch/normbatch/i11check`); reach 28 / 10,
     E unmoved. Item 4 (`47d13b73f`, census ASK 10, LITE-ONLY, landed under ruling 37 (a)): (a) a view's wall its first
     line (none moved: every tagged view wall one line, so the split is unexercised); (b) resolveAllStores'
     duplicated-include and include-cycle refusals wall their mapping in a tolerant build (a cycle: its first member),
     E.4's untyped service parameter walls its service (a Rust unit test; no Java one), AggregateViewLift's failure
     poisons its class (functions and bindings withdrawn) where it walled the mapping; (c) a walled mapping's lifted
     functions leave with it (no text reaches this once (b) poisons the aggregate view failure, the one that did, and no
     test exercises it); (d) an association's NotImplementedException defers in a strict build too, as a class's does. E
     468 / 157: (b) 84 / 29, every tolerant ask tagged (the aggregate view's one ask among them, whose orphan goes with
     its class); (d) 384 / 128, each now answered strict -- the 243 tagged strict asks unmoved are refused first by
     another ModelException (read from the run's forms). E.3c's IllegalStateException still escapes (a pipeline-order
     assertion: a bug is never walled). Item 12 (`b1d8fe8e0`): S376 and S378 marked FIXED with their commits; S393 (the
     include cycle's order), S394 (the XStore lines' comparison) and S395 (the M2M unmapped class, read:
     `ClassMappingThirdPassBuilder.java` :96-115) written. Ruling 37 recorded (`503a98560`, docs only): §10's ruling,
     rows S381-S392 and S395 ruled, the second batch carried.
   - *The batch's re-freezes.* The normalizer oracle: 1,059 moves over the batch, 1,053 distinct asks over 358 texts (2b
     218, 2e 216, 2fg 72, 2i 6, 5a 6, 7 29, 8 9, 2j 35, 4 468), each in its item's commit, each ask one the reach run
     tagged with the item's site, none untagged (`rust/oracles/normalizer/attribution-2026-11-15-normalizer-batch.tsv`,
     the MANIFEST's block); items 1, 2ac, 2dh, 3, 9, 10 and 11 moved nothing (a confirming run each, or the differential
     for item 10). The names oracle: item 7's re-freeze (models.tsv 74,260 lines, stores.tsv 18; the MANIFEST's note).
     The probe: the corpus lanes (//spec:corpus_duckdb, //spec:corpus_h2) under the probe v2 at the base and after each
     item that moved a front-oracle answer -- NN0 aa13a72da, NN1 item 1, NN2 2b, NN3 2e, NN4 2fg, NN5 3 and 2i, NN6 5a,
     NN7 7, NN8 2j -- each 16,202 keys answered, all equal (items 2ac, 2dh, 8, 9, 10 and 11 moved no front-oracle
     answer, so nothing downstream of E could move); then two complete runs at the last code commit, 47d13b73f (NNZ3,
     NNZ4; each its own tagged probe copy; 143 of 145 targets passing each, the two probe-caused failures //core:census
     and //core:guardrails): of the 73,413 frozen keys all answered, all equal, none moved or absent, in each -- no
     count, MirJson id, execution plan id or test plan id moved, so rust/oracles/plan, identity and mir are not
     re-frozen and no attribution file is written. Keys answered by both runs that the oracle does not hold, outside the
     freeze's exclusions (24 //core:ladder_report, the Postgres lanes' and 168 integration and server test models'
     temporary-path keys a run): 66 -- 64 in the core_tests_normalizer lanes, 2 in the core_tests_integration lanes,
     each matched to its text by the probe's own model id (`port-scratch/normbatch/keymap`), none with a plan id: 26 of
     the slice's step 1 texts (RefusalsNotCrashesTest's 14: their 14 module builds and 11 compiles, new at the slice's
     own probe, and TRANSLATED_SHAPE's bodies compile), 3 the bodies compiles of three of those texts that item 4 (d)
     lets a strict build compile (the ModelJoin shapes), and 37 of texts this batch added (NormalizerBatchTest 20,
     RefusalsNotCrashesTest's new cases 14, IncludeRulesTest's PARK-26 text 1, LegendLiteGapTest's `b77a6bf47` texts 2).
     Against the slice's count, 16,941 new keys = its 16,903 + these 40 - the 2 bodies compiles of LAMBDA_ARGUMENT and
     LAMBDA_PARAMETER, which item 8 makes a strict build refuse. None is added to the oracle, which freezes the base's
     keys and this batch moved none (namesland/refreeze3.py would add them: the lead's). A first pair at the same commit
     (03:48-04:10) ran without the probe -- its LiftedViews hunk no longer applied over item 4's wall line -- and
     answered nothing; the probe copy's hunk context rebased (normbatch/tools/make_variant4.py), the pair re-run. Every
     run's output folder deleted.
   - *Gates* (at `503a98560`, the code of `47d13b73f` with the two docs commits): cargo test --workspace (874 passed, 0
     failed), clippy -D warnings and fmt --check clean (CARGO_TARGET_DIR in the batch's scratch, Rust 1.99.1); the full
     Bazel suite, //... --keep_going --local_test_jobs=2: 584 of 586 targets passed and 2 skipped
     (//tools/guards:classpath_test, //tools/guards:markdown_inputs_test), every target passing in `rust/BASELINE.tsv`
     passing but the three ledgered gone ones (//core:core_tests_ide, //core:core_tests_lexer,
     //docs:update_dialect_capability_test); `port-tools/ledger_check.sh c4a39e06d`: no MISSING. The early gates after
     each item (`port-scratch/normbatch/tools/early.sh`: the normalizer, compiler, integration (H2 too), architecture
     and guard tests, lite-normalizer's, lite-names' and lite-guards' targets with both differentials, the corpus
     verdict lanes, the parser-equivalence ratchet) green at every commit; `own_corpus.matched` 3134 -> 3187 over the
     batch, each move regenerated by `update_ratchets` with its RETIRED_TESTS row.
   - *The owner's choices, taken conservatively*: name resolution's (D) own-package tier resolves `include A` and an
     unimported sibling association's simple name where the engine refuses both ("Can't find mapping 'A'", "Can't find
     association 'P_F'": D's, unchanged; `AssocSimpleNameProbeTest` runs through it); a mapping's own missing join keeps
     lite's words (the engine's getJoin would say "Can't find join", as a view's now does); a view's `{target}` terminal
     not ported (S376); md5() / convertDate() with no argument, md5(a, b), isNull with two arguments and an array lambda
     function's lambda deferred (the engine compiles each and refuses at plan or plans it); a view declaring a column
     twice refused when the model compiles (the engine compiles it and fails at plan; landed under ruling 37 (a)); the
     include cycle's words lite's and its order the declared one (S393); an enumeration mapping id written quoted keeps
     its quotes in the engine (''1''), lite's parser strips them (the reach run's note, phase 2's grammar unchanged);
     item 11's Java compares modulo commutation, the verdict any equality-consistent order gives, rather than ordering
     by an equality key Java does not have; item 4's include cycle walls its first member only (the refusal's element),
     and its (d) makes a strict build compile what an association's NotImplementedException defers (384 asks, among them
     RefusalsNotCrashesTest's three ModelJoin shapes) -- whether any such shape is the model's error upstream is the
     second batch's question; E.3c's ISE escapes a tolerant build; US:797's codec equality reads a source position the
     records' equals leaves out (no two sets of one model differ by it alone). The slice's own choices this batch
     settled: the same-package fallback deleted (item 9); ROT:610 the model's error (item 8; ROT:720 kept deferred); the
     two raw sites in names- and types-tier code fixed (item 7); ModelNormalizer:220 identity by a shared `Arc` (item
     10); MN:742's Java order (item 11); `Unported` removed (item 10). Corrections of the batch's own commits:
     `b77a6bf47` mends the `LegendLiteGapTest` case `fbe06d321` broke; `847a12e7c` corrects ad86fa895's comment ("store
     lookups" -> association lookups); nothing corrects `59787797c`.

## Carried to the slices not yet begun

Work an earlier slice found and ruled for a later one; each item names the ruling and the register row, and is done
when that slice lands.

- **The normalizer's second batch (E, before F freezes; ruling 37).** In Java and Rust together, each confirmed against
  the pinned engine first (run where it is an outcome), under rulings 34 and 37 (a) -- a fix that moves a frozen answer
  lands with its attributed re-freeze, any behaviour the engine shares left and noted: the ~25 further behaviours the
  normalizer slice listed (its entry above, "Java behaviours that look wrong beyond census ASK 13"); the M2M
  unmapped-class divergence (MN:1235-1241, register row S395, written by the normalizer batch); and the register rows
  ruled to take the engine's form -- S382 (a ~groupBy set's Join property mappings navigated inside the grouped select,
  multi-hop included: the emission in E, its lowering's share carried to slice I, the wrong comment fixed), S384 (each
  direction by its own join, measured first), S387 (dayOfWeekNumber's week start matched exactly, the engine run on a
  Sunday and a Saturday), S388's NULL run (the engine asked of a NULL read for a [1] property; an answer that differs an
  engine-sourced fix), S389 (pAllVersions only for a temporal p), S390 (the engine's words in strict, the association
  walled in tolerant), S391 (a real INNER join, measured first); S386's wrong comment fixed with them. Rows S381, S383,
  S385, S386 and S392 are ruled divergences, kept.

- **The resolver (H) and lowering (I).** *A query reading a relational column whose type inference the engine fails is
  refused when it is planned, in the engine's words* (the owner's ruling 35, 2026-11-14, PHASE3_SHIM_DESIGN.md §10;
  register row S378). legend-engine runs `inferRelationalType` where a query asks it -- at plan generation,
  `getRelationalTypeFromRelationalPropertyMapping` (relationalMappingExecution.pure:205-208, a TDS column's source type,
  :96, :101), as `inferRelationalType(false)`: an operation no rule types answers nothing there, and the column falls
  back to its property's own type (`inferSourceDataType`, :96-102), but three failures refuse the query whatever the
  flag -- a relational `case` with no safe type over its branches asserts `Type inference not supported yet! Dyna
  function: case` (relationalExtension.pure:402-411); an argument no rule types asserts `Type inference not supported
  yet! Dyna function: <name>` (:111-118: the rules read their arguments with `inferRelationalType($rop)`, the flag
  true, :120-123); and a rule's own failure (a `toOne` over no type, an `at()` past the arguments, a match with no arm)
  raises the engine's execution error. It never runs it when the database compiles
  (HelperRelationalBuilder.java:469-517 types no view column). Lite types these columns when the model compiles and
  when it seeds the metamodel, where it keeps no type (ruling 35: lite compiles what the engine compiles); when H and I
  are ported, a query that reads such a column is refused there, in those words, and the frozen answers that move with
  it are re-frozen and attributed then.
- **The typer (G), first, in Java before its oracle freezes.** *A lifted view's columns are checked within a family*
  (the owner's ruling 36, 2026-11-14, PHASE3_SHIM_DESIGN.md §10; register row S376). `SpecCompiler`'s return
  conformance (the `declares return type ... but body returns ...` refusal) also accepts, for a lifted view's function
  only (the lift's synthesized function, never a user's), a column whose declared type and body type are of one family:
  numbers (Integer, Float, Decimal, Number) or dates (Date, StrictDate, DateTime); a query over the view reads the
  declared type, the engine's; a column whose types are of two families is still refused. Its cases are the names
  batch's 74 one-view models (`port-scratch/namesbatch/cl`, `gen_compile_cases2.py`, `cl.final.out`): the 14 refused
  there by this check compile (`datePart`, `firstDayOfMonth`, `firstDayOfWeek`, `firstDayOfYear`, and `coalesce`, `if`,
  `case`, `max`, `plus`, `minus` over mixed kinds), every other answer unchanged; zero reach in the texts we hold.
- **The resolver (H).** *`ConstructedInstances.ARG_FREE` made consistent with the names batch* (found by the names
  batch, 2026-11-14; register row S378; the names batch's items 2-4, ruling 34). The set of relational functions whose
  type the resolver takes not to depend on their arguments (`resolver/ConstructedInstances.java`, `ARG_FREE`, read at
  the constructed instance's `DynaFunction` arm) still matches a name lower-cased, where the inference now matches it
  exactly (item 3), and still lists `joinstrings`, whose type now reads its first argument's size (item 4), and `like`,
  `position`, `charindex`, `locate`, which no longer have a rule (item 2). Measure its reach, then make it the
  inference's: the exact name, and only the rules whose type reads no argument.
- **The normalizer's follow-up (E, in Java and Rust together, after the port; ruling 30).** ~~Census ASK 13's ten
  behaviours that look wrong, fixed; PARK-26 settled by the engine's answer, asked first; tolerant mode's four wall
  shapes (census ASK 10 (a)-(d)) fixed; the view lift's missing join in lite's words where the engine says `Can't find
  join ...` (S376).~~ Done by the normalizer batch, 2026-11-15 (the normalizer slice's
  entry above, "the normalizer batch"), each re-freezing `rust/oracles/normalizer` with its moved asks attributed. Left:
  a `{target}` terminal the lift refuses where the engine compiles it (S376) -- written down, not ported: the engine
  plans no query over such a view, the owner's; the relational spellings the lift emits that G refuses (`like`,
  `charindex`, `locate`, capitals' `COUNT`/`SUM`/`ABS`, `mod` over floats, `md5` over an integer, `sub` over varchars;
  0 texts the oracles hold), measured and proposed per call for the typer's slice (G, with ruling 36;
  `port-scratch/normbatch/item6.md`); ~~the register rows S381-S392 as the owner rules them~~ ruled (ruling 37), their
  fixes the normalizer's second batch's (above).
