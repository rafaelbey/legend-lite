# History: documents removed from main

`main` carried about 40 MB of plans, audits, homework, censuses, session handoffs and logs that had done their job.
In November 2026, as the Rust port began, they were removed from the tree. Git keeps them. This file lists each
one with what it was and what came of it.

**Reading one.** Every path below was on `main` at `cf73e6ef7` (2026-11-01):

```sh
git show cf73e6ef7:docs/SOME_DOC.md                             # one file
git ls-tree -r --name-only cf73e6ef7 docs/type-audit-2026-08/   # what a folder held
git log --diff-filter=D -1 -- docs/SOME_DOC.md                  # the commit that removed it
```

A folder entry (ending in `/`) covers every file under it, except files still in the tree. A path cited in code
or in a doc that is no longer in the tree is listed here. The living documents are named in `AGENTS.md`
("Standing documents").

## Top-level docs, 2026-01 to 2026-08: the early plans, the burn-downs, the July and August audit rounds

- `docs/ADAPTER_NECESSITY_CENSUS.md` — 2026-08-27 necessity census of every ExecuteLegendLiteQuery/pct_adapter.pure arm (BOUNDARY, NAMED STOPGAP, DEAD); dead arms deleted in F10 slice 4, most PCT_AUDIT free wins found already executed.
- `docs/ADVERSARIAL_TENET_AUDIT_2026_08_18.md` — 2026-08-18 adversarial test of the claim 'Java orchestrates, the DB executes'; verdict accepted and the tenet recalibrated in AGENTS.md (guards catch drift, not adversaries); Tier 1-3 fixes landed, Tier 4 replaced by deleting HostEval/GridReads.
- `docs/ARCHITECTURE_AUDIT_2026_08.md` — 2026-08-05 twelve-agent audit of passing feature areas (11 of 12 'sound core, accreted edges'; lineage/TDG point solutions); fed the corpus burn-down index and remediation legs.
- `docs/ARCHITECTURE_REMEDIATION.md` — 2026-07-28 remediation plan from a 34-report architecture audit (phase boundaries, N-backend decoupling, concept ownership); T0-T3 landed (corpus 1,258 to 2,074).
- `docs/AUDIT_2026_07.md` — 2026-07 four-auditor core audit (parser, F, G, architecture) with an H/M/L ledger; Tier-1 fixes landed in July 2026.
- `docs/AUDIT_23_SPECIAL_CASING.md` — 2026-08 census of ~37k LOC of name/FQN/magic-string conditionals (engine-grounded, contract, overfit, wall, harness); overfit list burned in follow-up commits.
- `docs/AUDIT_FINDINGS_FOR_LEG1.md` — 2026-08-10 note proposing legend-pure's M3Parser as a second oracle, which corroborated 1,174 of 1,960 Leg-1 parser rows mechanically; fed DEEP_AUDIT_HANDOFF.
- `docs/AUDIT_PROGRAM.md` — 2026-07-30 plan for exhaustive null/try/if audits ('enumerations beat themes'); the null gate landed (NULL_GATE_VERIFICATION), the rest was overtaken by later rounds.
- `docs/BAZEL_DEPENDENCY_PROPOSAL.md` — Engine-era proposal for a Bazel-managed Legend monorepo with per-element artifacts and unused_inputs_list precision; superseded 2026-08-06, the real build is BAZEL_FIRST_CLASS_PLAN_2026_10_02.
- `docs/BAZEL_IMPLEMENTATION_PLAN.md` — Implementation checklist for BAZEL_DEPENDENCY_PROPOSAL (element serialization, lazy loading, LSP wiring) against the frozen engine tree; superseded 2026-08-06.
- `docs/BINDONCE_TYPING_HANDOFF.md` — 2026-08-31 handoff for bind-once typing of let-bound deferred expressions (~430 whole-test fallback rows), the next leg of the harness-deletion program.
- `docs/BURNDOWN.md` — Corpus burn-down taxonomy at 34d436d (2,223/2,721 pass, 479 failures bucketed by owner); superseded 2026-08-06.
- `docs/BURNDOWN_EXPLANATIONS.md` — 2026-08-06..18 per-test verdict ledger (IMPOSSIBLE, INFEASIBLE, ARTIFACT, HARNESS DEFECT, OPEN) for every non-passing row of RELATIONAL_CORPUS_ALL; replaced by the Bazel roster gates.
- `docs/BURNDOWN_ROADMAP.md` — 2026-07-29 priced roadmap for the remaining 442 corpus failures (2,096/2,538); superseded.
- `docs/CHANNELB_BURNDOWN_HANDOFF.md` — 2026-08-27 Channel B PCT burn charter and landing record: Essential 297 to 316/327, Grammar 133 to 135/137 in eight gated batches; remainder ledgered.
- `docs/COMPILER_SHORTCUT_AUDIT_2026_08_21.md` — 2026-08-21 audit of shortcuts in and around the compiler (proxy decisions, harness normalisation, chosen denominators); noted the pure-interpreted and engine-relational lanes disagree on 1 == 1.0 and on indexOf/substring bases.
- `docs/COMPILER_STAGE_AUDIT_2026_08.md` — 2026-08-06 audit of PMCD-to-PureModel compilation against live engine 4.133 and pure 5.88 oracles; section 7 found pure (most-specific FunctionMatch) and engine (first registered handler) disagree on overload resolution, later ruled as SEMANTICS_REGISTER S3 and S7.
- `docs/COMPILE_MODEL_AND_SIGNATURES.md` — Post-inversion plan to finish Phase F (model and function signatures) and outline Phase G, written against engine/com.gs.legend; executed then frozen.
- `docs/CORE_PHASE_F_TYPED_ELEMENTS.md` — v1 design of core Phase F typed elements (compiler/element); superseded by v2.
- `docs/CORE_PHASE_F_TYPED_ELEMENTS_V2.md` — v2 design of Phase F typed elements (ElementCompiler, ModelContext, FQN-string cross references); landed in core/compiler/element.
- `docs/CORPUS_BURNDOWN_INDEX.md` — 2026-08-05 working index of corpus cause groups per family with fix sites and confidence; superseded by the Bazel roster gates.
- `docs/CORPUS_STUDY_2026_08.md` — 2026-08-03 root-cause study of 356 non-passing corpus rows; superseded by the 484-row study.
- `docs/CORPUS_STUDY_2026_08_ALL.md` — 2026-08-05 23-agent study of all 484 non-passing corpus rows; section 4.1 recorded 31 engine goldens disproven on evidence and 4.3 the H2 row-order artifacts; basis of BURNDOWN_EXPLANATIONS.
- `docs/CORPUS_SWEEP_PERF.md` — 2026-08-26 perf investigation of the Maven corpus sweep (61.7% in module compilation); one fix landed at cafea293 (-21%), two proposed.
- `docs/CORPUS_TAXONOMY.md` — 2026-07/08 root-cause taxonomy of ~406 non-passing corpus tests; superseded by CORPUS_STUDY_2026_08 and _ALL.
- `docs/CORRECTNESS_REMEDIATION.md` — 2026-08-02 plan to fix wrong answers and scoreboard honesty (every bucket mislabeled, always flatteringly); tiers C0 onward executed.
- `docs/COVERAGE_CENSUS.md` — 2026-08-11 three-axis census of unexercised engine constructs (~53 protocol tags, ~35 keywords); probes never gated, superseded by GRAMMAR_COVERAGE_CENSUS and SurfaceCensusTest.
- `docs/CROSS_PROJECT_JOINS.md` — Engine-era comparison of mapping/database includes vs XStore model joins for cross-project joins, with a compile-time proposal; not built.
- `docs/CSV_DIFFERENTIAL.md` — 2026-08-17 probe of harness Java CSV render vs the platform toCSV lowering: 105 exact, 20 differing in 4 mechanisms; informed the F4.3 cutover.
- `docs/DEEP_AUDIT_CHARTER_2026_08.md` — 2026-08-14 brief for a four-dimension parallel deep audit for a fresh session.
- `docs/DEEP_AUDIT_HANDOFF.md` — 2026-08-12 parser deep-audit charter: ACCEPT parity 26,168 elements and 5,259 PMCD documents byte-identical, REJECT parity on 742 dialect rows, catalog total 1,459 rows.
- `docs/DESIGN_DEBT.md` — 2026-07-05 deferred MEDIUM/LOW lowering/sql/exec smells (exception as control flow, copy-paste pairs, boolean-mode parameters).
- `docs/E2E_BURNDOWN_2026_08_14.md` — 2026-08-14 classification of 276 non-passing core_relational tests into 11 buckets plus a survey of the engine's whole test estate; evidence in docs/burndown-2026-08-14/.
- `docs/E2E_DEEP_DIAGNOSIS_2026_08_15.md` — 2026-08-15 per-test mechanism, fix and refutation for all 276 failures (209 clusters); evidence in docs/e2e-diagnosis-2026-08-15/.
- `docs/EMBEDDED_UNION_NAV_2026_08_31.patch` — Verbatim 737-line diff of the reverted 2026-08-31 embedded-union navigation attempt (UnionSynthesis and three other files).
- `docs/EMBEDDED_UNION_NAV_HANDOFF_2026_08_31.md` — 2026-08-31 handoff after reverting the embedded-union navigation change-set (resolver-bug-4 tests); section 7 was the harness-deletion program blueprint.
- `docs/FOUNDATIONS_BASELINE.md` — 2026-08-16 F0.1 baseline of the foundations program (corpus 2,347/2,575 etc.) against which FOUNDATIONS_PLAN's expected reds were measured.
- `docs/FOUNDATIONS_DEEP_AUDIT_2026_08_17.md` — 2026-08-17 re-execution audit after the foundations program closed: PCT 1,110/1,110, corpus 2,337/2,575, deletion claims verified by grep.
- `docs/FQN_MIGRATION.md` — 2026-07 Wave 4c plan to key the native catalog by real package FQN per overload (32 multi-home bare names), ending silent capture of user functions.
- `docs/FULL_BURNDOWN_HANDOFF_2026_08_30.md` — 2026-08-30 state and work map for driving sql-exec declines and mismatches to zero (v7 dual channel agree 3,381 / disagree 9; corpus 2,356/2,575).
- `docs/FULL_RESIDUE_CENSUS_2026_08_30.md` — 2026-08-30 census of 417 decline/text-only/unable-to-exec residue rows over 305 tests, bucketed one by one.
- `docs/FUNCTION_REGISTRY.md` — 2026-03-20 proposal to store built-in signatures as Pure signature strings copied from upstream .pure files; realized as the verified native catalog.
- `docs/GRAMMAR_COMPATIBILITY_2026_08.md` — 2026-08-05 per-### section analysis of what 100% engine grammar compatibility needs; executed by the section program (24 of 25 sections registered).
- `docs/HARNESS_AUDIT_2026_08.md` — 2026-08-11 five-axis audit of the parser equivalence harness: comparison honest, subject not, instrument lacked a self-test.
- `docs/HARNESS_SIMPLIFICATION_PLAN.md` — 2026-08-11 plan to make the equivalence harness decision-free (verdict symmetry, document byte claim, self-test, judgments moved to data files); executed.
- `docs/HONEST_DEBT.md` — 2026-08-12 adversarial self-review ledger of caveats behind the parser parity numbers; items fixed or argued, compiler coverage handed to later programs.
- `docs/HOST_LOGIC_AUDIT_2026_08_20.md` — 2026-08-20 audit of every Java value-logic/formatting arm with world-2 consistency and emission discriminators; engine ruled the semantic oracle only, arms evicted or registered in JavaEvalLedgerTest.
- `docs/INVENTION_AUDIT_2026_08_14.md` — 2026-08-14 code-only diff of lite's declared surface against engine and pure sources: ungoverned meta::legend::lite natives and names found nowhere upstream; executed by deleting inventions and adding NativeCatalogGovernanceTest.
- `docs/LEG1_INNERJOIN_FAMILY.md` — 2026-07-26 census of classMappingFilterWithInnerJoin family errors: six mechanisms, the dominant one views as identity-carrying join targets.
- `docs/LEGEND_MAPPING_PATTERNS.md` — 2026-04 survey of legend mapping patterns and a plan to treat mappings as a thin DSL over Relation operators; superseded by the function-form normalizer design.
- `docs/M4_PRELAND_CHARTER.md` — 2026-08-25 demand census and pre-flight for the M4 re-land of the hetero-literal claim on the typed IR with zero compensations.
- `docs/MAPPING_FEATURE_CHECKLIST.md` — Engine-era single-source checklist of mapping features with Rosetta links; superseded 2026-08-06.
- `docs/MAPPING_STORE_GAP_ANALYSIS.md` — Engine-era mapping and store gap analysis toward 100% backward compatibility; superseded 2026-08-06.
- `docs/MAPPING_UNIFICATION_PLAN.md` — Engine-era four-pass (parse, type, resolve, generate) mapping unification plan; superseded 2026-08-06.
- `docs/MERGE_READINESS_AUDIT_2026_08.md` — 2026-08-07 adversarial review of the parser as a FINOS drop-in (drop-in built inside the real engine, differential over 7,219 sources).
- `docs/METAMODEL_MACHINERY_CENSUS.md` — 2026-08-30 census of every host-side metamodel evaluation door (statement-loop side doors, JavaEvalLedger registry).
- `docs/MODEL_TO_MODEL.md` — 2026-01 design for M2M transforms compiled entirely to SQL (database as runtime); M2M since built.
- `docs/NAV_ARM_CENSUS_4AD.md` — 2026-08-29 section 4AD step 1: named blast radius of arms compiling mapped navigation to EXISTS/correlated subqueries (1,017 tests, 4 arms).
- `docs/NAV_ARM_CENSUS_4AD_ROUND2.md` — 2026-08-29 round-2 arm-attribution census (fnlr value dispatch 30 tests, filter-equality fold 25, size-2 leaf 42) for the nav-routing batches.
- `docs/NAV_ROUTING_BATCH0_4AD.md` — 2026-08-29 batch 0: engine routing map read from pureToSQLQuery.pure plus golden shape survey; confirmed the two-form design (engine source as semantic, not implementation, spec).
- `docs/NAV_ROUTING_PLACEMENT_ADDENDUM_4AD.md` — 2026-08-29/30 corrective placement design and landing records (P0 bisect, P1 placement fix, P2 filter-position pad guard).
- `docs/NESTED_SCOPE_REGISTRIES.md` — 2026-07-26 design to build every nested substitution scope's registries from one factory (belowScope/nestedScope).
- `docs/NOT_IMPLEMENTABLE.md` — 2026-08-15 ledger of corpus tests unsatisfiable by a correct DuckDB implementation (positional at(N) asserts over unsorted H2 scans and similar).
- `docs/NULL_GATE_VERIFICATION.md` — 2026-07-31 verification of the null-gate commits a814ffa9..aa5df4f7 (NullAway with own Nullable annotations); G0-G3 executed.
- `docs/ORIGIN_ARCHITECTURE_AUDIT_2026_08.md` — 2026-08-29 audit of the identifier-origin slices: spine sound, JOIN/UNION stamping a construct-then-reconcile patch; remediation deleted three mechanisms.
- `docs/PARSER_ADVERSARIAL_AUDIT_2026_08.md` — 2026-08-12/14 adversarial parser audit; closing pass byte parity 6,489/6,489, walls 0, diffs 0.
- `docs/PARSER_AUDIT_2026_08_14.md` — 2026-08-14 audit of every parser disagreement over 3,180 engine files and 5,622 mutants; found 301 files (9.5%) crash the engine's own parser.
- `docs/PARSER_DEEP_AUDIT_2026_08.md` — 2026-08-12 deep audit of the parser; action list landed (infix flags, quote-aware FQN split, dialect-split floats, version-skew claims TSV).
- `docs/PARSER_DEEP_AUDIT_2026_08_14.md` — 2026-08-14 response to the parser audit: SurfaceCensusTest enrolled in gate 8, GQL escape and number bugs fixed.
- `docs/PARSER_DROP_IN.md` — 2026-08-04 feasibility study of lite's hand-written parser as a byte-identical drop-in for the engine's ANTLR parser (memory 1,281 vs ~39 bytes per character the strongest reason).
- `docs/PARSER_DROP_IN_STATUS.md` — 2026-08-06 drop-in status (22,725/22,725 byte-identical, 43/43 rejection pins); its branch framing was wrong, the work was on main.
- `docs/PARSER_IMPLEMENTATION_AUDIT_2026_08.md` — 2026-08-05 five-axis parser audit that found a 33% operator-precedence divergence invisible to every gate; fixed.
- `docs/PCT_AUDIT.md` — 2026-08-03 audit of PCT adapter compensations (each tested by deletion) and of a self-hosted PCT runner; free wins executed (see ADAPTER_NECESSITY_CENSUS).
- `docs/PCT_BURNDOWN.md` — 2026-07 PCT parity scoreboard (legend-pure 5.88.0 / engine 4.133.0 baseline) and slice protocol.
- `docs/PCT_EXPECTED_FAILURES.md` — 2026-07-30 ledger of 36 of 1,109 PCT expected failures, each justified by reference parity with the engine's own DuckDB adapter exclusions.
- `docs/PCT_HEADER_DIFFERENTIAL.md` — 2026-08-16 inventory of 138 wrong type/multiplicity patterns hidden by the PCT header overlay (Date as String dominant); overlay deleted in F5.3 Stage B.
- `docs/PERFORMANCE_AUDIT.md` — 2026-08-05 audit of Maven test-loop times, parallel-run kills and core algorithmic bottlenecks.
- `docs/PHASE4_REDESIGN_PROPOSAL.md` — 2026-08-19 decision that asserts are verdicts, always (the engine has no SQL translation for any assert); Channel B's SQL assert rule deleted.
- `docs/PHASE_5_6_IMPLEMENTATION_PLAN.md` — Engine-era Phase 5/6 end-to-end mapping implementation plan; superseded 2026-08-06.
- `docs/PHASE_B2_RESULT_VALUE.md` — 2026-07 design of the platform Result/execute() value to replace TestBody's host envelope machinery.
- `docs/PHASE_B_COMPILED_ELEMENTS.md` — Engine-era Phase B compiled-element serialization design (.legend files, Compiled* records, back-reference tiers); superseded 2026-08-06.
- `docs/PHASE_H_PLAN.md` — 2026-07-09 Phase H StoreResolver plan (a probe found synthesized mapping bodies had never been type-checked); superseded by PHASE_H2_H3_RESOLVER_PLAN.
- `docs/PHASE_K_EXECUTION.md` — 2026-07-05 Phase K execution design and the throwaway QueryService bridge that ran the engine's ~94-class suite as core's acceptance scoreboard.
- `docs/PIPELINE.md` — 2026-07-15 map of every compile pass (parse, resolve, normalize, F, G, inline, H resolve, I lower, J dialect, K execute) with what dies where.
- `docs/POSTGRES_BACKEND.md` — 2026-08 feasibility design for embedded Postgres as a backend (row equality the only contract); since built.
- `docs/PROTOCOL_MIGRATION_CENSUS.md` — 2026-08-09 ranked worklist: every construct parsed to a protocol record, transformed into the model, straight-to-model parsers deleted.
- `docs/QUERY_DEMAND_PRUNING.md` — Engine-era note proposing query-demand pruning from TypeChecker access maps (follow-up to Phase C.1); not pursued in core.
- `docs/ROSETTA_IMPLEMENTATION_CHECKLIST.md` — Engine-era filtered checklist of unimplemented mapping features linked to Rosetta docs; superseded 2026-08-06.
- `docs/RUNNABILITY_PLAN.md` — 2026-08 plan to make every core_relational test runnable (2,798 discovered); WALL_DEPTH post-mortem showed estimates missed ~4x.
- `docs/SECTION_PROGRAM_HANDOFF.md` — 2026-08-09 handoff: 24 of 25 engine sections claimed by registered section grammars on the protocol path; remaining wire ledger.
- `docs/SIMPLE_NAME_AUDIT.md` — Audit of simple-name matching where import-scoped FQN resolution is required (18 Databases named myDB); superseded by the NAME_RESOLUTION_BUG fix.
- `docs/SPAN_ORIGIN_CONSOLIDATION.md` — 2026-08-14 catalogue of seven span/offset mechanisms consolidated into SpanOrigin; recorded that expression-island kinds mirror the engine's closed set and the extension seam is the section level.
- `docs/STATE_AUDIT.md` — 2026-08-05 enumeration of 306 void methods, 476 records and 10 ThreadLocals with determinism probes.
- `docs/STRESS_TEST_BENCHMARKS.md` — Engine-era stress benchmarks at c0997b5 (1K-100K element models; parse plus build 71% of the 100K cold start).
- `docs/TDG_LANE_CHARTER.md` — 2026-08-30 charter for the ###Data/test-data-generation lane: 117 asserts in meta::relational::testDataGeneration::tests, censused by assert form.
- `docs/TEAM_DEPENDENCY_PROPOSAL.md` — Engine-era proposal for immutable compiled project artifacts, sparse checkout and diamond resolution; superseded 2026-08-06.
- `docs/TENET_AUDIT_2026_08_16.md` — 2026-08-16 eleven-auditor tenet audit: query compiler clean, violations at the egress boundary and harnesses; fed JAVA_EVICTION_PLAN.
- `docs/TENET_REMEDIATION.md` — 2026-07-30 eight-agent tenet conformance audit (VIOLATION/COMPENSATION/BOUNDARY/ORCHESTRATION rubric, 20 ranked violations); executed.
- `docs/TEXT_SURGERY_AUDIT_2026_08.md` — 2026-08-06 census of every regex and string-surgery site (drop-in, pipeline, measurement, engine) with an ArchUnit enforcement design.
- `docs/TYPE_E2E_AUDIT_2026_08.md` — 2026-08-26 seam-by-seam audit of types from Pure through typed IR, SQL, wire, decode and verdict (does consumption re-derive what construction knew).
- `docs/TYPE_SYSTEM_AUDIT_2026_08.md` — 2026-08 adversarial type-system audit (105 defects, 14 root causes): types computed rigorously but not enforced; native catalog diverged from upstream in 183 of 721 signatures, since verified verbatim.
- `docs/USER_FUNCTIONS.md` — Engine-era reference for user-defined Pure functions in lite (syntax, type and multiplicity checking, overloading, composition).
- `docs/V12_FUSION_SPIKE_2026_08_28.md` — 2026-08-28 spike hand-fusing four corpus tests' asserts into one SQL statement per test body (let as CTE, side-tagged UNION ALL evidence).
- `docs/V7_ARCH_AUDIT_2026_08_28.md` — 2026-08-28 audit of the V7 assert-to-database arc: skeleton architectural, verdict invariants held by convention in five places.
- `docs/V7_ASSERT_VERDICT_CENSUS.md` — 2026-08-28 phase-0 census for moving corpus asserts to SQL verdicts (0.26 ms per in-memory DuckDB query; seed replay dominates the sweep).
- `docs/V7_FLATCELLS_ATTEMPT_CENSUS.md` — Raw [v7]/[canon] output of the reverted V7 flat-cells leg (ec9f6fe8), kept for its 28-row diagnosis.
- `docs/V7_LAMBDA_AUDIT_2026_08_28.md` — 2026-08-28 audit of the m3-true function-value classifier slice (130 TypedLambda mint sites, one body-typed mint).
- `docs/VERIFY_ARM_AUDIT_2026_08.md` — 2026-08-28 audit of every decision arm on the SQL-text assert verify path (policy, dispatch, compensation).
- `docs/WALL_DEPTH.txt` — 2026-07-22 per-test distinct error-message depth over the history of RELATIONAL_CORPUS.md, an unlock-estimation instrument.
- `docs/WHOLETEST_COMPILATION_CHARTER.md` — 2026-08-31 charter for compiling a whole multi-statement test function as one unit (harness-deletion item 1), flipped per test with a counted fallback.
- `docs/XSTORE_LEG.md` — 2026-07-26 plan for 24 cross-store tests as one SQL plan (JSON ModelStore roots, JSON-sourced M2M, mapping chains).
- `docs/audit-20a-overfit.md` — 2026-07-18 overfit audit: the not-rule COALESCE set was keyed at the wrong layer (engine processNot special-cases only equal/in; partition comes from pure's [0..1] overloads).
- `docs/audit-20b-wrongrows.md` — 2026-07-18 wrong-rows audit of the B2a/B2b result frame, groupBy stage 2 and null consistency: no wrong rows or vacuous passes.
- `docs/audit-20c-architecture.md` — 2026-07/08 architecture coherence audit of the B2, groupBy stage 2, null-semantics and auto-map range.
- `docs/audit-21a-parse-leniency.md` — 2026-07 audit of parse-unlock commits: the M2M prop[targetSetId] route was consumed and dropped with no record.
- `docs/audit-21b-resolver.md` — 2026-07-18 resolver audit: nav-route correlation silently dropped conjuncts below a join slot; two wrong-rows holes found.
- `docs/audit-22a-typing.md` — 2026-07-19 typing and inlining audit: non-strict lambdas over empty [0..1] manufactured values; real pure auto-maps everything that is not [1].
- `docs/audit-22b-wire.md` — 2026-07-19 audit of identity emissions and the wire comparator: toOne identity under-enforced, flat-cells comparator too loose.
- `docs/fallback-cleanup-todo.md` — Engine-era Phase C.2 list of fallback and silent-default sites to remove; superseded 2026-08-06.
- `docs/gate-additions.patch` — Unapplied 2026-08-16 patch adding gates to the Maven-era .github/workflows/gate.yml; superseded by the Bazel CI.
- `docs/golden-shape-survey-4AD.tsv` — 2026-08-29 golden SQL shape survey for the section 4AD nav-routing batch 0 (engine goldens classified by shape and consumption position).
- `docs/native-dispatch-llvm-style.md` — Engine-era proposal for an LLVM-style typed IR and codegen split for native dispatch; folded into pipeline-architecture.md.
- `docs/nav-arm-census-4AD-round2.txt` — Raw round-2 nav-arm attribution dump (2026-08-29) behind NAV_ARM_CENSUS_4AD_ROUND2.
- `docs/nav-arm-census-4AD-round3.txt` — Raw round-3 nav-arm attribution dump (2026-08-29) for the nav-routing batches.
- `docs/nav-arm-census-4AD.txt` — 4AD navigation-arm census dump (2026-08-29) behind the nav-routing design; its rule is in NAV_ROUTING_DESIGN.
- `docs/pipeline-invariants.md` — Engine-era end-to-end per-pass invariant catalogue (lazy/eager equivalence, resolved state in types, closed IRs) with rustc/Calcite analogies; never the live rule set, which is AGENTS.md's.

## Top-level docs, 2026-09 to early 2026-10: homework, charters and the September programs

- `docs/A1_HOMEWORK_2026_09_25.md` — Homework for compiler-rebuild step A1/#47 (overload choice by legend-pure's rule, candidates as declarations), 2026-09-25; corrected by PLAN_AUDIT_2026_09_26 and superseded by EXECUTION_PLAN_2026_09_26 (parked).
- `docs/AUDIT_BATCHES_116_118_2026_09_07.md` — Audit of harness-endgame batches 116-118 (2026-09-07): a silent option default and a magic inlining depth fixed; the harness was later rebuilt (two judges, block compiler).
- `docs/BATCH_120_FRAME_FACTS_HANDOFF_2026_09_07.md` — Batch 120 handoff (2026-09-07): execution-frame facts (SQL post-processors, time zone, execute options) ride the from; landed with rosters exact (DuckDB 2454 / H2 1866).
- `docs/BATCH_1_5_AUDIT_2026_09_11.md` — Audit (2026-09-11) re-measuring upstream-boundary batches 1-5 against the tree: HOLDS / DEFERRED / SHORTCUT verdicts; the deferrals were fixed in later batches.
- `docs/BATCH_7_HOMEWORK_2026_09_11.md` — Homework (2026-09-11) for batch 7: which 15 checkout-reading test files move from core to the new spec module; the spec module exists.
- `docs/BATCH_7_PLAN_2026_09_11.md` — Batch 7 plan (2026-09-11): the Pure test runner moved into the product, checkout-reading code into spec, then core-never-depends-on-upstream enforced; landed.
- `docs/BAZEL_EXECUTION_RUNBOOK.md` — Runbook (2026-10-04) for executing the Bazel first-class workplan batch by batch (sync, read, build, prove locally, independent audit, push); the program ended and its remaining work moved to REBUILD_PROGRAM_2026_10_06.
- `docs/BURN_BREAKDOWN_2026_09_05.md` — Per-test breakdown (2026-09-05) of the 168 corpus fallbacks after batch 72c into IMPL / TEXT / ENGINE / NAMED; the burn-down is paused and the rcorpus rosters are the live record.
- `docs/CODE_AS_DATA_HOMEWORK_2026_09_05.md` — Parked research (2026-09-05) on legend-engine's extension registry and running engine tree-walking programs as data, for five connection-equality tests; mechanism kept on branch wip/72c-extension-registry-read; superseded by D6 (an engine compiler entry is ours or a named wall).
- `docs/COLUMN_OWNER_LOWERING_LEG_2026_09_09.md` — Homework (2026-09-09, batch 164): Column.owner : Relation lowering for five lineage scanColumns tests (implicit inheritance union on Relation); landed.
- `docs/COMPILE_EVERYTHING_HOMEWORK_2026_09_09.md` — Homework (2026-09-09) on the 19 platform bodies still not compiling after the prelude-as-module program; reconciled 'closure follows declarations' with 'the platform registers what it references'; closed.
- `docs/CONNECTION_LEASE_DESIGN_2026_09_09.md` — Design (2026-09-09) of a JDBC connection lease for ConnectionResolver's unpooled-connection leak found on Windows CI; the Lease left the code 2026-10-30 (D70, the planner-driver seam's step A).
- `docs/CONNECTION_LIFECYCLE_HOMEWORK_2026_09_09.md` — Homework (2026-09-09) proving a leaked DuckDB JDBC connection behind QueryServiceDirectTest's Windows teardown failure; fixed by the connection lease (itself removed 2026-10-30).
- `docs/CONSTRUCT_COVERAGE.md` — Maven-era construct-coverage taxonomy of the core_relational corpus (denominator 2489, last written 2026-09-06); nothing regenerates it; live measurement is //spec:corpus_duckdb and //spec:corpus_h2.
- `docs/CORPUS_BURNDOWN_HANDOFF.md` — Start-here handoff of the corpus burn-down program (ledger 2398/2798 at 4dd28f24); paused 2026-09-29 for the compiler rebuild; AGENTS.md labels it History.
- `docs/CORPUS_FAIL_REVIEW_2026_09_12.md` — Row-by-row review (2026-09-12) of the 121-row DuckDB corpus fail roster into BURN / accepted-divergence / wall verdicts; the rosters in spec/src/test/resources/rcorpus carry the live reasons.
- `docs/CORPUS_ZERO_PROGRAM_2026_09_12.md` — Corpus-to-zero program (2026-09-12): every failure climbs FIX > HIJACK > ACCEPT > WALL with per-row homework; paused with the burn-down 2026-09-29.
- `docs/DATABASE_MODE_HOMEWORK_2026_09_18.md` — Homework and leg log (2026-09-18..20) for judging step 3, database mode (each assert a SQL predicate over canonical spellings, the verdict a row); built; see JUDGING_TWO_MODES' summary.
- `docs/DATACUBE_AUDIT_PASS2_FINDINGS_2026_09_26.md` — Catalogue of 417 adversarially verified DataCube TypeScript defects (P2-0..P2-416, 2026-09-26); fixed through legs A-C and later DataCube programs; P2-n ids in datacube/src comments resolve here.
- `docs/DATACUBE_PRODUCTION_AUDIT_2026_09_26.md` — DataCube TypeScript production-readiness audit (2026-09-26): pass-1 findings, the pass-2 themes and fix order; leg 1 (re-aggregation), P2-36 and the following legs landed 2026-09-27/28.
- `docs/DECLARATIONS_HOMEWORK_2026_09_04.md` — Homework (2026-09-04) on whether the platform must register every class a program names; decided as option S, recorded as a WORLD_MAP section 5 amendment.
- `docs/END_TO_END_PLAN_2026_09_08.md` — Plan v2 (2026-09-08) to finish the harness and burn the corpus roster to its honest floor; superseded 2026-09-29; AGENTS.md lists it as history.
- `docs/HAND_SHAPE_DIVERGENCE_2026_09_08.md` — Sweep (2026-09-08) of the 84 hand-declared classes in Pure.java against the spec (30 real divergences) and the plan to move them into the generated prelude; done by batch 167.
- `docs/HARNESS_AUDIT_2026_09_07.md` — Seven-reviewer adversarial audit of the test harness (2026-09-07); merged into END_TO_END_PLAN v2; the harness was rebuilt since (two judges, block compiler).
- `docs/HARNESS_FROM_SCRATCH_AUDIT_2026_09_06.md` — Audit (2026-09-06) of the minimum a from-scratch test harness needs (122 walk fallbacks at the time); fed the harness endgame.
- `docs/IMPORT_DATA_FLOW_DESIGN_2026_09_08.md` — Clean-sheet design (2026-09-08) of importDataFlow for testPksWithImportDataFlow (union members project pk_<p>_<ordinal>); landed as batch 146.
- `docs/JUDGE_INVENTORY_2026_09_17.md` — Inventory (2026-09-17) of every harness judge deciding pass or fail for numeric values; led to JUDGING_TWO_MODES and the numeric charter.
- `docs/LEDGER_GRANULAR_2026_09_06.md` — Granular per-test ledger (2026-09-06, after batch 105) of the 130 corpus fallbacks scored by the legacy walk; the walk channel was deleted.
- `docs/LEG2_STACK_AUDIT_2026_09_14.md` — Adversarial receipt audit (2026-09-14) of leg 2 'the stack' of legacy routes as composition; findings fixed (R-key, R-target, R-chain).
- `docs/LEGACY_MAPPING_REACHBACK_CENSUS.md` — Census (2026-08-30) of findLegacyMapping reach-backs past the normalizer; the enumeration-mapping table was baked onto the normalized mapping.
- `docs/MAPPING_NORMALIZER_AUDIT_2026_09_15.md` — Sixteen-part audit (2026-09-15) of the ~209-commit mapping-normalizer rewrite; fixes A2-A11 landed (GATES.md); design item P7 left unagreed (NORMALIZER_PACKAGE_RECUT).
- `docs/METAMODEL_AS_RELATIONS_HOMEWORK_2026_09_02.md` — Deep homework (2026-09-02) on representing the Pure metamodel as relations in the database; ruling recorded in WORLD_MAP section 2.
- `docs/MODEL_HOME_2026_09_28.md` — Survey and design options (2026-09-28) for where Pure models live and are versioned; superseded by STUDIO_DESIGN_2026_10_02 (SDLC-lite and Depot-lite).
- `docs/MULTIPLICITY_AUDIT_2026_08_20.md` — Deep audit (2026-08-20) of the multiplicity program ([0..1] accepted into [1], match, egress); its 2026-09-11 loosening (section 4a) was reverted per legend-pure's source.
- `docs/NATIVE_CLAIMS_CENSUS_2026_09_10.tsv` — Snapshot (2026-09-10) of the 175 registry-unclaimed Pure.java FQNs (tools/native-axes.py); superseded by the derived claim registry (batch 3).
- `docs/NATIVE_PROVENANCE_2026_09_11.md` — Provenance (2026-09-11) of the 752 upstream-named Pure.java overloads by source file at engine 4.138.2 / pure 5.92.0; native signatures are now generated from the pin.
- `docs/NORMALIZER_PACKAGE_RECUT_2026_09_16.md` — Handoff (2026-09-16) for re-cutting the normalizer package (audit FIXLIST P7); not agreed, never started.
- `docs/NUMERIC_ENVELOPE_CENSUS_2026_09_17.md` — Census (2026-09-17) of the four read-time 'Float means DOUBLE' decisions behind 22 last-digit stress rows; led to the numeric charter.
- `docs/ONE_PLATFORM_PLAN.md` — Master foundations plan (2026-08-18): one implementation of Pure semantics, the database computing; superseded 2026-09-29; AGENTS.md lists it as history.
- `docs/OPEN_REGISTER.md` — Per-item register of open work under PROGRAM_MAP (2026-08..09); superseded 2026-09-29; AGENTS.md lists it as history.
- `docs/OPTION2_HOMEWORK_2026_09_03.md` — Homework (2026-09-03) for 'the compiler compares, the database computes' over the debugPrint tests and toPostgresModel; ratified as WORLD_MAP section 4.
- `docs/OUTSTANDING.md` — Frozen Maven-era feature-keyed ledger of non-passing corpus tests (written by tmp/outstanding.py, no longer run); SkipCensusTest's comment pointing at its 'Declared platform gaps' should be repointed.
- `docs/PHASE3_DEMAND_CUT_HOMEWORK_2026_09_08.md` — Homework (2026-09-08) for cutting the prelude to platform vocabulary only (corpus-only engine classes enter the corpus graph by file); landed.
- `docs/PHASE5_SIZING_2026_09_08.md` — Sizing (2026-09-08) of 'code and metamodel as data' from the 108 DuckDB fails after batch 146; deferred.
- `docs/PLATFORM_FAIL_ADJUDICATION_2026_09_01.md` — Foundation probe (2026-09-01): every platform-fail divergence adjudicated against engine source; no silent corruption; three engine-defect rows recorded.
- `docs/PRELUDE_MODULE_CENSUS_2026_09_08.tsv` — Snapshot of the system prelude module census (2026-09-08) that fed SYSTEM_PRELUDE_DESIGN.
- `docs/PROGRAM_MAP.md` — Whole-program map (ratified 2026-08-23: four buckets, audit intake, the longer arc); superseded 2026-09-29; AGENTS.md lists it as history.
- `docs/REAL_PLAN_2026_09_25.md` — Plan (2026-09-25) from a compiler that checks names to one that holds references; superseded by EXECUTION_PLAN_2026_09_26 (parked); AGENTS.md lists it as history.
- `docs/REFEREE_IN_DATABASE_DESIGN_2026_09_07.md` — Design (2026-09-07) for the SQL referee judging golden SQL in the database instead of H2Verify; its code is parked in docs/parked/InDbVerdict.java.
- `docs/RELATIONAL_CORPUS.md` — Maven-era corpus scoreboard and adjudication ledger (last written 2026-09-06); nothing regenerates or reads it; live rosters are spec/src/test/resources/rcorpus; repoint its citations in AGENTS.md, core/README.md, GATES.md, AUDITS.md.
- `docs/RELATIONAL_CORPUS_ALL.md` — Maven-era scoreboard of the same corpus with ToFix / Ignore / ExcludeAlloy tests included (-Drcorpus.includeExcluded); last written 2026-09-06; not regenerated.
- `docs/SCOREBOARD.md` — Maven-era engine-suite scoreboard append-log; its writer tools/scoreboard.py (--record) parses engine/target surefire reports that no longer exist and is invoked by nothing (delete the script too; //scripts/corpus:scoreboard_gate is a different tool).
- `docs/SESSION_HANDOFF_2026_09_01.md` — Session handoff (2026-09-01) of the harness-deletion burn; superseded by SESSION_HANDOFF_2026_09_02.
- `docs/SPEC_BODY_CENSUS_2026_09_08.md` — First typing census (2026-09-08) over legend-pure's nine platform packages (481 typed / 643 failed, cut to 22 pinned rows within days).
- `docs/SQLTEXT_HOMEWORK_2026_09_03.md` — Homework (2026-09-03) on every SQL-text assert: 36 of 65 gated tests flip on rows verdicts, 170 text verdicts each given a reason.
- `docs/STORE_TYPES_HOMEWORK_2026_10_02.md` — Homework (2026-10-02) for reading store types Pure cannot name by their declared type at every reference (OTHER/DISTINCT as text, ARRAY/OBJECT as Variant, a StoredRead IR node per dialect); ruled and built; the rule is SEMANTICS_REGISTER S26.
- `docs/STRESS_CORPUS_BURNDOWN_HOMEWORK_2026_09_16.md` — Row-by-row homework (2026-09-16) on the 156 DuckDB / 211 H2 stress-corpus fails in eleven families; fixed or ledgered.
- `docs/TRANSLATOR_AUDIT_2026_09_15.md` — Audit (2026-09-15) of the property-mapping translator against engine source rule by rule (T1-T16 with receipts and judges); findings fixed in legs 3-5.
- `docs/TWO_DESIGN_LEGS_2026_09_07.md` — Research (2026-09-07) on single-shot test execution and code + metamodel as data; single-shot became the block compiler, the other leg was deferred.
- `docs/UPSTREAM_BOUNDARY_FACTS_2026_09_10.md` — Opening fact sheet (2026-09-10) for bumping legend-engine / legend-pure; superseded by the program and its homework (latest was then engine 4.145.0 / pure 5.99.0).
- `docs/UPSTREAM_BOUNDARY_HOMEWORK_2026_09_10.md` — Evidence base (2026-09-10) behind the upstream boundary program: every measurement and receipt; the self-contained bump now belongs to REBUILD_PROGRAM_2026_10_06.
- `docs/V7_ASSERT_VERDICT_CHARTER.md` — Charter (2026-08-28) routing corpus asserts through the dual-verdict path (DB-computed canonical bytes plus the host lattice as referee); superseded by JUDGING_TWO_MODES (2026-09-17).
- `docs/WALKS_AND_REACHBACKS_CENSUS_2026_09_13.md` — Census (2026-09-13) of shadow type-system walkers, recursion and reach-backs in the normalizer; triggered T4.1.
- `docs/WALK_ONLY_PLAN_2026_09_05.md` — Homework and plan (2026-09-05) for the 16 walk-only corpus fallbacks; the walk channel was deleted and five connection-equality tests parked.
- `docs/WAREHOUSE_W0_HOMEWORK_2026_09_26.md` — Measured W0 answers (2026-09-26): a Java wrapper as GraalVM native image is viable, the parser supports a strict authorizer, DuckLake for multi-process, Arrow options, a WASM binding; identity-as-variable later replaced (FFM homework).
- `docs/patches-table-alias-column-leg-2026-09-04.patch` — Saved patch (2026-09-04) of a table-alias column leg in SystemMetamodel.java, referenced only by SESSION_HANDOFF_2026_09_02.

## Top-level docs, 2026-10 onward: landed programs, reviews and run logs

- `docs/AUDITS.md` — Index and reading order of the audit rounds, 2026-07 to 2026-10 (parser, compiler stage, grammar, text surgery, architecture, type system, harness, build, PR #67); every audit it indexed is archived and listed here.
- `docs/BAZEL_EXECUTION_LOG.md` — Per-push log of the Bazel first-class program (batches 0 to 12, 2026-10-03 to 10-07, PRs #19 to #26); done, its remainder became the build rebuild Phase 8.
- `docs/BAZEL_FIRST_CLASS_PLAN_2026_10_02.md` — Audit and plan (2026-10-02) to make every build, test, generation and CI step a hermetic, declared Bazel target (seven phases, Bazel 10 readiness, script inventory, traceability table); executed through BAZEL_EXECUTION_LOG, remainder folded into REBUILD_PROGRAM Phase 8.
- `docs/CLOUD_BACKENDS.md` — Desk research (2026-07-31) on free Databricks and Snowflake tiers for a capability-probe suite; superseded by the user's own accounts (CLOUD_BACKENDS_PCT_2026_10_11).
- `docs/COMPILER_RIGHT_DESIGN_2026_10_07.md` — Revision 2 (2026-10-07) of the compiler done right: measurements of the Java compiler mapped to EXECUTION_PLAN items and the ruling D25 ("the evidence way") on where the parked rebuild resumes; superseded by the Rust port.
- `docs/CYPHER_TCK_TO_ZERO_PLAN_2026_10_17.md` — Holistic plan (2026-10-17, at 3093 of 3880) to pass the whole openCypher TCK in five phases and about 32 legs, with a blockers log; executed: about 3602 of 3880 on both lanes (PR #58, writes PR #67), rulings recorded as register rows (e.g. S66).
- `docs/DATACUBE_APP_PLAN_2026_10_02.md` — Plan (2026-10-02) for bazel run //datacube:app -- postgresql://... (the developer path, legs after Postgres P0/P1); done; the living user guide is DATACUBE_ON_POSTGRES.md.
- `docs/DATACUBE_BI_EXCEL_PARITY_2026_10_24.md` — 2026-10-24 proposal: Pure as the semantic layer against DAX/Power BI/Excel, with rulings D1-D6 in its section 7 never taken; archived unratified.
- `docs/DATACUBE_BI_GAP_ANALYSIS_2026_10_24.md` — Area-by-area gap analysis of DataCube against Power BI and Tableau at 819a2e81a (2026-10-24), with a recommended order; continued as DATACUBE_BI_EXCEL_PARITY's catalogue.
- `docs/DATACUBE_COMMANDS_PAGE_2026_10_18.md` — Command program stage 1, page half (2026-10-18): every page-structure edit a PageCommand applied by one pure function; landed; carried on by DATACUBE_ONE_API_PLAN.
- `docs/DATACUBE_COMMANDS_STAGE1_2026_10_18.md` — Command program stage 1, grid half (2026-10-18): every grid edit a GridCommand applied by one pure function; landed; carried on by DATACUBE_ONE_API_PLAN.
- `docs/DATACUBE_COMMAND_API_HOMEWORK_2026_10_17.md` — Read-only survey (2026-10-17) of every grid, page and Python action and the state model, with a staged command plan; stages 1-3 landed, then the one API program replaced it.
- `docs/DATACUBE_COVERAGE_2026_10_18.md` — Coverage report of DataCube's 137 Node test files (2026-10-18, command program stage 1a part 4); rerunnable with datacube/tools/coverage.mjs.
- `docs/DATACUBE_FLOW_MODE_REVIEW_2026_10_26.md` — Independent four-reviewer audit of the flow-mode branch (PR #75, 2026-10-26): blocking and should-fix findings fixed before the PR left draft.
- `docs/DATACUBE_GRAPH_EXPERIENCE_2026_10_24.md` — Brainstorm (2026-10-24, not agreed) of a DataCube-style visual editor and viewer for Cypher graph queries, extending DATACUBE_GRAPH_MODE's phases G0-G4.
- `docs/DATACUBE_GRAPH_MODE_2026_10_18.md` — Proposal to the Cypher line (2026-10-18, not agreed) for a DataCube graph mode in phases G0-G4, and the G0 spike built on main's #Cypher island (roadmap run phase 4).
- `docs/DATACUBE_LIVE_COMMANDS_2026_10_18.md` — Command program stage 3 (2026-10-18): commands as the live channel between Python and an open page instead of re-serving the whole document; landed; carried on by DATACUBE_ONE_API_PLAN.
- `docs/DATACUBE_LIVE_EDITING_2026_10_24.md` — D19, the page keeper's client side for live shared editing (2026-10-24, PR #71); four audit rounds still found defects and the user chose LIVE_EDITING_REDESIGN's middle version the same day.
- `docs/DATACUBE_ONE_COMMAND_API_AUDIT_2026_10_21.md` — Four read-only audits (2026-10-21) of every change that is not a command and every browser/Python divergence in DataCube; the evidence for DATACUBE_ONE_API_PLAN's register.
- `docs/DATACUBE_ONE_GRID_EDIT_PATH_2026_10_17.md` — One way to edit a grid, the screen's gridEdit path, for the screen and Python alike (2026-10-17); built in PR #47; its principle became the one API tenet.
- `docs/DATACUBE_PAGE_KEEPER_2026_10_20.md` — Homework and as-built steps 1-2 (2026-10-20) for one keeper per page: the host keeps the page document, every write versioned, a stale write re-applied as commands or refused; superseded in part by ONE_API S4 and LIVE_EDITING_REDESIGN.
- `docs/DATACUBE_PAGE_STATE_2026_10_18.md` — Command program stage 2 (2026-10-18): a page's one state (PageDraft) held by one owner with one history, page-level undo; landed; carried on by DATACUBE_ONE_API_PLAN.
- `docs/DATACUBE_PYTHON_PAGES_DESIGN_2026_10_09.md` — Design (2026-10-09) for building whole DataCube pages (grids, visuals, tabs) from Python; landed as 94ee44b08; superseded in part by DATACUBE_ONE_APP_DESIGN R1 and R2.
- `docs/DATACUBE_PYTHON_SHOW_DESIGN_2026_10_08.md` — Design (2026-10-08) of ll.show(df): DataCube on a Python dataframe, rows as Arrow or JSON, live vs snapped, the notebook cube and marimo; landed; superseded in part by DATACUBE_ONE_APP_DESIGN R1, R4, R5.
- `docs/DATACUBE_ROADMAP_RUN_2026_10_18.md` — Ledger of the 2026-10-18 one-session DataCube roadmap run (stacked phases: page commands, page state, live commands, graph mode), with skipped items; done.
- `docs/DATACUBE_UI_REVIEW_2026_10_24.md` — UI deep dive (2026-10-24): one tile header, the page menu grouped by purpose, one menu design language in two scopes, chart options as a closing popover, status bar, Markdown text box; mostly built by 2026-10-25, the rest listed in its Suggested order.
- `docs/MANIFEST_WORLD_HOMEWORK_2026_10_05.md` — Homework (2026-10-05) on whether per-program manifests make the prelude unnecessary and what everyone gets for free; answered by MANIFEST_WORLD_EXPERIMENTS (the default world decided).
- `docs/OUTPUT_LAYER_DESIGN_2026_10_10.md` — PARK-24's output-layer design (2026-10-10): an answer is Pure's value, its text the served API's; measured that interpreted legend-pure keeps a Float's decimal digits (3 * 1.1 is 3.3) where compiled legend-engine uses doubles, keeping interpreted Pure as the reference (NUMERIC_CHARTER Rule 2); closed 2026-10-18 by the plan path program's step 2.
- `docs/PR67_REVIEW_2026_10_22.md` — Two independent reviews of PR #67 (Cypher writes, store and class writes, #SQL DML, write plans; 2026-10-22 and 10-24), every finding probed on DuckDB, H2, Postgres and Neo4j: 14 fixed, 3 registered, 9 decisions for the user.
- `docs/PRIMITIVE_COVERAGE_2026_10_12.md` — Census (c9d88f1, 2026-10-12) of the 244 primitives the relational corpus sends against what PCT exercises on each database, with a covering subset; succeeded by the generated docs/dialect-capability/matrix.tsv.
- `docs/RESULT_BOUNDARY_START_HERE.md` — Session handoff (rewritten 2026-10-28) for the result boundary program: roadmap, briefs, lanes, traps, the user's rulings; every track done and landed as PR #80 (2026-10-30); open questions live as register rows S119-S127 and the decisions in docs/result-boundary-homework/DECISIONS_LOG.md.
- `docs/SESSION_HANDOFF_2026_09_02.md` — Frozen handoff (2026-09-02 to 09-08) of the harness-deletion burn: next-session plans to batch 105, the non-metamodel burn map, the metamodel program and the host-side evaluation register; superseded by END_TO_END_PLAN and then the compiler rebuild.
- `docs/STEP9_LANE_BRIEF_2026_10_19.md` — Brief (2026-10-19) for sibling sessions putting every PCT backend on the plan route, one container per database; plan path step 9 done.
- `docs/TIMESERIES_REVIEW_2026_10_25.md` — Four-reviewer audits of the timeseries tile (2026-10-25/26) and their fixes; section 9's unfixed follow-ups remain (N7, Q12, P6-P8, Python frame() defaults, a cube alone saves no grid it added).
- `docs/WEB_IMAGE_SPIKE_2026_10_10.md` — GraalVM Web Image spike for the tab's planner (2026-10-10): exact JDK library and nine times faster planning, but four times larger, experimental, Oracle-licensed and not reproducible; the tab stayed on TeaVM with a conformance test.
- `docs/WINDOWS_APP_DESIGN_2026_10_02.md` — Design (2026-10-02) for //datacube:app and //warehouse:serve on Windows x64; its launcher was replaced 2026-10-07 by the build rebuild's L1c (the warehouse knows nothing of Bazel).
- `docs/WRITES_FOR_STORE_BACKENDS_2026_10_22.md` — Handoff note (2026-10-22) answering five questions about the write design for a keyed-table backend (the Deephaven line); the design is WRITES_DESIGN.

## Evidence folders under docs/

- `docs/bazel-audit-2026-10-02/` — Seven slice audits, five spikes (Bazel test runner, runfiles and launchers, hermetic cc for the native image, pinned Chromium, stress generator), coverage checks and the per-script review behind BAZEL_FIRST_CLASS_PLAN_2026_10_02.md; its workplan landed and the rest moved into the build rebuild.
- `docs/build-inventory/` — Build inventory deep dive (2026-10-05/06): eight area reports and a 1,163-row inventory of every Bazel target (build_inventory.py), the evidence for BUILD_REBUILD_DESIGN_2026_10_05.md (decisions D1-D15); the design landed as the build rebuild's Phases 0-3b.
- `docs/build-inventory/generators/` — Generator dossiers G1-G6 (2026-10-05): every generator's inputs, outputs and axes, the evidence for docs/GENERATORS.md; fed Phase 1 (generator hygiene) and Phase 2 (DynaFn and engine handlers from upstream), both landed.
- `docs/build-inventory/manifest-world/` — WORLD_H and manifest-world experiments E1-E8 plus phase 2b/3/3b censuses (2026-10-05/09): upstream's module loading against lite's boot, measured; the default-world rule they decided is in MANIFEST_WORLD_EXPERIMENTS_2026_10_06.md section 1 and REBUILD_PROGRAM Phase 4 (parked); the harness served the parked Phases 4 and 6.
- `docs/build-inventory/program/COLD_READ_2026_10_07.md` — A fresh session's cold read of the build rebuild documents (2026-10-07); its gaps were fixed into START_HERE and the briefs the same day.
- `docs/build-inventory/program/COLD_READ_2026_10_09.md` — End-to-end re-read of the bump program for the user (2026-10-09); its rulings (L7 moved before L4, F2/F3) were applied to REBUILD_PROGRAM section 4 and START_HERE section 3.
- `docs/build-inventory/program/DEBTS_RESOLVE_AND_TYPE_ONCE.md` — Research behind PARK-5 (a call resolved once, fixed by L7 on 2026-10-10) and PARK-6 (an argument typed once, open): the Java places that retype; PARK-6's rule (legend-pure types each argument once, then matches) stays in PARKED_WORK_LEDGER.
- `docs/build-inventory/program/JUDGES_TARGET_DESIGN_2026_10_11.md` — 2026-10-11 design of a //gates:judges suite and the lane query in //gates:local; awaited a ruling, nothing built; archived unratified.
- `docs/build-inventory/program/PHASES_3B_6.md` — Execution brief for Phases 3b and 6 (2026-10-07): 3b landed 2026-10-10 (991df952b, PHASE_3B_LANDING); Phase 6 (admitting the manifest world) never started, parked with the build rebuild and overtaken by the Rust port's compiler.
- `docs/build-inventory/program/PHASES_4_5_7.md` — Brief for Phases 4 (default world generated from upstream), 5 (Pure.java as rows by id) and 7 (the self-contained bump), 2026-10-07: never started; parked with the build rebuild; the Rust port replaces the Java compiler these phases restructured.
- `docs/build-inventory/program/PHASE_3B_HOMEWORK_2026_10_09.md` — Phase 3b homework H1-H7 measured (manifest-world census, boot-layer twins, import scopes), 2026-10-09; fed the agreed 3b plan, which landed 2026-10-10.
- `docs/build-inventory/program/PHASE_3B_LANDING.md` — Phase 3b landing record (991df952b, 2026-10-10): schema views lifted once, per-element import scopes, the platform's own Pure as a row kind, legend-pure's qualified-property lookup; census load walls 37 to 27.
- `docs/build-inventory/program/PHASE_3_LANDING.md` — What was left before Phase 3 (one implementation table decides, by exact function id) landed; landed 2026-10-09 as 1e4a2bd40 (GATES 'Build rebuild Phase 3').
- `docs/build-inventory/program/TEAVM_UNMODIFIABLE_DESIGN_2026_10_11.md` — TeaVM 0.15.0's Collections.unmodifiableSet/Map scanned linearly; TeaVM's 0.16.0 fix was ported into third_party/teavm_classlib (tab boot 3,575 to 299 ms, a 200-class page 3,975 to 73 ms); landed 2026-10-11.
- `docs/build-inventory/program/evidence/` — Audits, judge and render-census comparisons, probes and measurements for each build rebuild landing and side-line landings (phases 0-3b, L7, phase 6 homework, dup-mapping, func-descriptor, iq-finds, scale-costs, correlated columns, TeaVM numbers and unmodifiable wrappers, the Web Image spike, leg 8), 2026-10-05 to 10-15; every landing is recorded in GATES.md.
- `docs/build-inventory/program/evidence/compiler/` — Flight Recorder profiles of the Java compiler (eager corpus compile, stress corpus, the eager residue, the D24 substitution census, jfr_*.py tooling), 2026-10-07/08, which sized the parked compiler plan's W0.8/W1 work; tools/metrics/README's pointer to the JFR tooling goes with them.
- `docs/build-inventory/upstream-only/` — R1-R3 (2026-10-05/07): how each upstream generator (natives, prelude, engine handlers, dynafn, imports, fixtures) could read only upstream inputs; evidence for UPSTREAM_ONLY_HOMEWORK_2026_10_05.md, applied in Phases 1-2.
- `docs/burndown-2026-08-14/` — Evidence for E2E_BURNDOWN_2026_08_14.md: a 276-row classification of the Maven-era corpus failures (2,299 of 2,575 passing) and its scripts; the input of the 2026-08-15 deep diagnosis.
- `docs/datacube-charts-homework-2026-10-09/` — The ECharts gallery read whole (377 examples, a chart-kind and feature census) for DATACUBE_CHARTS_DESIGN_2026_10_09.md.
- `docs/datacube-dashboards-homework-2026-09-28/` — Chart-library survey (bundle sizes, CSP render checks), a gridstack measurement and a tile-layout prototype for DATACUBE_DASHBOARDS_DESIGN_2026_09_28.md.
- `docs/datacube-headless-evidence-2026-10-11/` — Measurement for DATACUBE_HEADLESS_PYTHON_DESIGN_2026_10_11.md: DataCube's non-screen TypeScript run inside Python in quickjs-ng, V8 and dukpy (identical output; dukpy lacks JSON source-text access), the decision the design records.
- `docs/datacube-model-homework-2026-10-11/` — The tab's 200-class compile split into boot and page (2026-10-11): the boot dominated, which found TeaVM's unmodifiable-collection scan (TEAVM_UNMODIFIABLE_DESIGN, landed); fed DATACUBE_MODEL_DESIGN_2026_10_10.md section 11.
- `docs/datacube-python-show/` — The manual JupyterLab check of a notebook's cube (2026-10-08) and the fault it found; became //datacube:jupyterlab_test on 2026-10-09.
- `docs/datacube-types-census-2026-09-27/` — Census G1-G10 of every DataCube TypeScript site that declares, decides or derives a type fact, the evidence for DATACUBE_TYPES_TO_SERVER_2026_09_27.md (types moved to the server).
- `docs/dialect-leak-census-2026-10-12/` — Census guide, scanners and nine slice reports behind DIALECT_LEAK_CENSUS_AND_PLAN_2026_10_12.md; the plan is done (2026-10-18): the ledger is empty and DialectLeakRatchetTest bans a database's name outside its owners.
- `docs/e2e-diagnosis-2026-08-15/` — Machine artifacts behind E2E_DEEP_DIAGNOSIS_2026_08_15.md: 283 static diagnoses of the Maven-era corpus's non-passing tests in 209 fix clusters; the burn-down it planned was paused (CORPUS_BURNDOWN_HANDOFF is history).
- `docs/execution-plan-boundary-2026-10-05/` — Render census, plan shadow run and in-memory-database sharing probe that measured the Java planner while EXECUTION_PLAN_BOUNDARY_2026_10_05.md's steps landed (2026-10-05 to 10-10); the boundary shipped and the plan path program built on it.
- `docs/harness-audit-2026-09-07/` — Seven adversarial reviews (driver, referee, main-side residue, ambient state, guards, roster and floor, strength) behind HARNESS_AUDIT_2026_09_07.md of the rebuilt Java test harness.
- `docs/invention-audit-2026-08-14/` — Maven-era probes reproducing INVENTION_AUDIT_2026_08_14.md.
- `docs/mapping-normalizer-audit-2026-09-15/` — Sixteen findings reports and FIXLIST behind MAPPING_NORMALIZER_AUDIT_2026_09_15.md, an audit of the Java normalizer's stamped-facts and legacy-routes arc; its fixes landed per GATES.
- `docs/notes/` — First cut of the query-side union builder (2026-09-14), reverted the same day after 337 lost rows (LEGACY_ROUTES_AS_COMPOSITION section 9).
- `docs/one-platform/` — Two probes (a page call through query-store's planner, the apps on Python) behind ONE_PLATFORM_PROGRAM_2026_10_12.md.
- `docs/parked/` — Parked Maven-era work: batch-120 patch scripts, the DuckDB fail roster of batch 119 and an InDbVerdict prototype (REFEREE_IN_DATABASE_DESIGN_2026_09_07); never resumed.
- `docs/parser-audit-2026-08-14/` — Maven-era probes reproducing PARSER_AUDIT_2026_08_14.md.
- `docs/plan-audit-2026-09-26/` — Homework and audits of the compiler rebuild plan EXECUTION_PLAN_2026_09_26 (program audit, stage readings, H1, meta and tractability audits, cold read, W0.6 homework, step-3 design, H2/H4 designs, wrong-rows runs, D24 homework), 2026-09-26 to 10-08; folded into plan revisions H1-H4; the plan is parked since 2026-10-04 and the Rust port replaces the compiler.
- `docs/reference/` — Copies of lite's pre-port legacy PlanGenerator/SqlBuilder Java (package com.gs.legend, 2026-04), kept as reference during the rewrite; the code is in git history.
- `docs/type-audit-2026-08/` — Evidence base of TYPE_SYSTEM_AUDIT_2026_08.md: about 30 auditors, a fuzzer, a fallback census and a signature oracle over the Java type system (105 defects, 14 root causes); TRIAGE (2026-08-26) dispositioned every defect and REMEDIATION fed the later compiler plans.

## Outside docs/

- `.agents/workflows/no-fallbacks.md` — Agent-workflow restatement of NO FALLBACKS in engine-era terms (compileExpr, PureCompileException); the rule lives on as AGENTS.md invariant 4 and its don't-list.
- `datacube/docs/FEATURE_CENSUS.md` — First, sampled DataCube-vs-upstream feature census (2026-09-25) with its fix history; superseded as the gap list by datacube/docs/UPSTREAM_CENSUS.md, whose banner restates its two corrections.
- `experiments/` — Prototype workspaces and probe harnesses (2026-04..08), outside the build; their findings live in the warehouse/backend docs.
- `experiments/backend-probes/corpora/` — Probe-statement corpora (3,702 SQL probes, 2026-07-31/08-01) run on DuckDB 1.5.0, SQLite 3.47.1, Postgres 18.4, MariaDB 11.4.5, H2; conclusions in experiments/backend-probes/fragments/ and docs/*_BACKEND.md.
- `experiments/backend-probes/databricks/` — Databricks probe harness and raw outputs behind CLOUD_BACKENDS_PCT.
- `experiments/backend-probes/duckdb-census/` — DuckDB 1.5.0 function-catalog dump (2,941 rows) and its Catalog*.java/py census tooling (2026-08-01), behind docs/DUCKDB_FUNCTION_COVERAGE.md.
- `experiments/backend-probes/duckdb-syntax/` — Two zero-assertion DuckDB probes (UNNEST with unequal arrays, variant/JSON load shapes) moved out of the test tree by FOUNDATIONS_PLAN Phase 8 (2026-08-17).
- `experiments/backend-probes/harness/HARNESS.md` — Run sheet for the 2026-08 backend-probe harness shared by concurrent agents (session /private/tmp scratch path, Maven cp.txt, MariaDB4j fix).
- `experiments/backend-probes/harness/` — The backend-probe harness (Probe.java, pom.xml) that produced fragments/.
- `experiments/backend-probes/results/` — Raw per-backend probe results (12,188 rows, 2026-08-01) behind experiments/backend-probes/fragments/ and docs/*_BACKEND.md.
- `experiments/name-resolution-repro/` — Repro (FqnBug.java) of unimported names binding silently by FQN-suffix scan; fixed 2026-08-02 and pinned, see docs/NAME_RESOLUTION_BUG.md.
- `experiments/tree_artifact_test/` — 2026-04 standalone Bazel prototype showing unused_inputs_list does not track files inside tree artifacts; finding recorded in docs/BAZEL_DEPENDENCY_PROPOSAL.md (line ~340).
- `progress-c0954a.txt` — Session log (2026-04-24) of the association 'right-architecture' migration in engine/ (NavScope, 1372/2669 tests); engine/ since deleted.
- `progress.txt` — Session progress log (2026-04) of the typed-AST big-bang that replaced engine's TypeChecker sidecar map with typed HIR (Phase A); landed as core's TypedSpec. MappingNormalizerTest cites a section it does not contain.
- `progress/baseline-failures.txt` — Baseline (2026-04-28..30) of failing engine (com.gs.legend) tests during the typed-AST migration; module deleted.
- `progress/mr-rewrite-progress.md` — Abandoned progress notes on the engine MappingResolver rewrite (2026-05..08); superseded by core resolver/StoreResolver (phase H).
- `repro/` — 19 standalone write-ups of legend-engine defects found by the L0 corpus (2026-08), each restated as an F-numbered entry in docs/UPSTREAM_FINDINGS.md, which cites 16 by path; reproduce via scripts/corpus/probe_*.py. Two carry minimized models (settest.pure, dialect.pure).
- `tools/golden_shape_survey.py` — One-shot 2026-08-29 script behind golden-shape-survey-4AD.tsv and the nav-arm censuses.
- `tools/native-axes.py` — Script that classified upstream natives into docs/NATIVE_CLAIMS_CENSUS_2026_09_10.tsv (the claim-registry homework).
- `tools/scoreboard.py` — Script that parsed engine/target surefire reports into docs/SCOREBOARD.md; both gone (scripts/corpus/scoreboard.py is a different, live tool).
