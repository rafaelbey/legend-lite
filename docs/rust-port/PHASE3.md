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
