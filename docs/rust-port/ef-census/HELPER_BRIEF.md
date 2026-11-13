# Helper brief: one part of the E/F census (read-only)

You are reading part of legend-lite's Java for a census that decides what the Rust port's next slice ports. Today is
2026-11-12. **Read-only**: change no file anywhere except your one output file named below. Never touch
/Users/cocobey73/Projects/legend-lite (another agent is committing there). Everything you need is in an extracted copy
of commit 7c438f7b3:

- TREE = /Users/cocobey73/.local/legend-lite/port-scratch/efcensus/tree   (core/src/main/java/com/legend/..., core/src/test,
  spec/, pct/, wasm/, warehouse/, docs/, rust/, AGENTS.md)
- Class-level caller matrix (a word-match over code with comments stripped; a NAME match, so prune it):
  /Users/cocobey73/.local/legend-lite/port-scratch/efcensus/callers.txt. A non-`public` class can only be reached
  from its own package: its "test" column entries outside the package are word-match noise unless the test is in the
  same package. Verify every caller you cite by import or qualified name.

Use Read and grep (Bash `grep -n`, `sed -n`) on TREE. Do not run git. Do not spawn agents.

## Context you need (read these first, briefly)
- TREE/AGENTS.md: the pipeline (E = `normalizer/ModelNormalizer.normalize`, F = `compiler/element/PureModelContext.from`),
  invariants 4 (no fallbacks), 5 (lazy user elements), 6 (F must not trigger G).
- The production path into E and F: `Compiler.normalizeLayer` (TREE/core/src/main/java/com/legend/Compiler.java ~line
  270) calls `ModelNormalizer.normalize(adopted, index, walls)`; it is reached by `Compiler.compileModel`,
  `buildModule`, `compileErrors`, `buildModel` and the process-wide boot layer (`Compiler.boot`). `PureModelContext.from`
  is called from `Compiler` likewise. So **everything reachable from `ModelNormalizer.normalize` or
  `PureModelContext.from` is production-reached**; a DON'T PORT for code on that path is wrong unless you show the path
  is never taken (e.g. a branch no input reaches, with evidence) or the code produces nothing any answer reads.
- What counts as "an answer": a plan (rust/oracles/plan), SQL, an error message (class + text, incl. a model wall's text
  that `compileErrors`/`buildModule` returns), a type, protocol JSON, a normalized model's content (the synthesized
  functions' bodies and names, binding tables, `NormalizationFacts`) that later stages read.
- The port's rules: Java's behaviour is the spec where it reaches an answer; the upstream-fidelity tenet (AGENTS.md); no
  fallbacks; the Rust planner is held to frozen oracles of Java's plans byte for byte.
- Identity (TREE/docs/rust-port/PHASE3_DESIGN.md §1.1 and §6 ruling 1): model tier is `Arc` with `lite_base::identity`
  (`same`, `IdMap`, `IdSet` -- no iteration API) where identity is semantic; derived structural equality where it is
  accidental. Step 0's census (TREE/docs/rust-port/PHASE3.md lines 9-400) classified the E/F identity sites; its rows
  for E and F, with frozen hit/miss counts over 73,411 questions:
  - (b) `model.LiftedViews.owners` 112/0, `.functions` 393/112, `.walls` 0/112, `.expanding` 0/112 (LiftedViews.java:50-57)
  - (b) `model.ViewSignatures.expanding` 0/137 (ViewSignatures.java:51)
  - (b) `model.PureModelContext.CheckedLayer` 6,478,837/183,150 (PureModelContext.java:104, the boot layer's elements by instance)
  - (b) `model.MappingClosures.first` 0/2 (MappingClosures.java:297, `first.get(id) != cm`: a diamond's one mapping vs a duplicate)
  - (b) `model.MappingPrePass.selfEdge` 11/0 (MappingPrePass.java:259, `next != pcm`)
  - (b) `model.AssociationSynthesis.self` 68/66 (AssociationSynthesis.java:202, `m == md`)
  - (c) `model.ModelNormalizer.adopted` 9/23 (ModelNormalizer.java:220, `d == dp`)
  - (b) model `MappingDefinition.position` 23,989/0 (model/MappingDefinition.java:174, built when E constructs a MappingDefinition)
  A site with 0 hits means the frozen oracle cannot catch a mistake there: say so.
- Hash order (TRIAGE C1, TREE/docs/rust-port/TRIAGE.md): Java's `HashMap`/`HashSet` iteration order is deterministic,
  Rust's is not. For each non-Linked hash collection in your classes, say whether it is ITERATED (for-each over
  keySet/values/entrySet, forEach, stream, toList, copy into a list) and whether that order can reach an answer. A
  membership-only `seen` set is safe; say so briefly.
- Other idioms to flag: mutable memo caches (and whether shared across threads: D5, PHASE3_DESIGN.md §2), exceptions as
  control flow (a `catch` that turns a failure into a poison/wall/skip -- quote what the catch produces: does the
  resulting text embed the exception's message or class name? then the text is part of the answer), `catch
  (RuntimeException)` (TRIAGE A.4: "audited, not replicated").
- Crate split (TREE/docs/rust-port/PHASE3_CRATES.md §2): `lite-names` (compiler root: ModelBuilder, NameResolver,
  KnowledgeLayer, SynthFqn, ResolvedNames, ...) < `lite-element` (F) < `lite-spec` (G, G½). `lite-normalizer` may reach
  all three. `compiler/spec/Calls` (the one builder of an `AppliedFunction` after resolution) lives in `compiler/spec`
  but depends only on `NameResolver`, `CoreFn` and protocol records. Record every edge your classes take to
  `compiler.spec.*`, `compiler.element.*` or a layer outside `tools/deps/core-layers.txt`'s `normalizer`/`compiler`
  lines (TREE/tools/deps/core-layers.txt).
- Model methods slice 1b deferred (TREE/rust/lite-model/src/lib.rs line 20): `MappingDefinition`'s methods and
  constructor index (`ClassBindings.of`, `position`, `NormalizationFacts`), `ClassMapping`'s witness-column names
  (`memberWitness`, `isEmbedded`, `aggregationAwareMain`, ...), `LegacyMappingDefinition.withClassMappings`,
  `RuntimeDefinition`'s helpers, `ClassMapping.RelationFunction.Col.bindSrc`. Name every one your classes call, with
  the line.
- Docs that call something a known defect, shim, probe, ledger, census, park, or "scheduled for deletion": quote the
  phrase and cite file:line. The guard tests over E's sources are FallbackLedgerTest (the `MissProbe` funnel per file),
  ParkedWorkLedgerTest (PARK-1 `MappingNormalizer`/`XStorePureEnds` "has direction-specific conditions", PARK-26
  `StoreSubstitutionRewrite` include spelled twice), LegacyReachbackCensusTest, CodeShapeGuardrailTest -- all under
  TREE/core/src/test/java/com/legend/.

## What to produce, for every class you are given (and for each large method family inside a big class)
1. What it does, in two sentences.
2. Who reaches it: production entry points (via `ModelNormalizer.normalize` / `PureModelContext.from` / another named
   main caller outside E/F -- e.g. the resolver, lowering, planner, server), tests only, probes/measurement only, or
   nothing (dead). Measure by callers (cite caller file:line), not by names. For a method family inside a big class,
   name the caller method(s).
3. Does its output reach an answer? Name the path (e.g. "the synthesized class function's body -> G types it -> H/I
   lower it -> SQL"; "a `ModelException` text -> `compileErrors`"; "a `NormalizationFacts` field -> resolver X").
4. Its Java idioms needing a deliberate Rust choice (identity -- and step 0's row, hash iteration reaching output,
   memo caches, exceptions as control flow, `catch (RuntimeException)`), each with file:line.
5. A CANDIDATE recommendation, one of: PORT (line for line: its behaviour is answer-bearing spec), RE-EXPRESS (same
   answers, simpler structure -- you MUST say what the simpler structure is and why it cannot change an answer),
   DON'T PORT (measurement, dead, test-only, or superseded -- say what replaces it, if anything), or ASK (a choice
   only the owner can make: spell the options and what each costs). The caller makes the final call; give evidence.
6. Lines (the file's `wc -l`; for a method family, its line range).

Also note anything surprising: code that looks like a fallback (invariant 4), a branch that looks unreachable,
duplicated logic with another class, a comment naming a defect.

## Output
Write your part to the output file named in your task (Markdown; a table per class or family with the six columns --
class/family, lines, reached by, reaches an answer, idioms, recommendation -- then a short notes paragraph per class
with the evidence: file:line citations). Be precise and terse; no preamble. Then end with a reply of at most 250 words:
the file path, your candidate DON'T PORT / RE-EXPRESS / ASK items in one line each, and anything the caller must check.
