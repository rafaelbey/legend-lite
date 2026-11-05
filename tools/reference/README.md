# The reference differential: what the real Pure compiler resolved, call by call

**Since 2026-09-29 this is a gated lane** (execution plan W1.1): `bazel test //spec:reference_lane` (manual, about
8 GB, about 45 s once the reference dump is cached). The reference side is the build output
`//tools/reference:ref_dump`, legend-pure's compiler run from the pinned `@maven_upstream` jars (4.145.0 / 5.99.0), so
the jar-versus-pin skew and the source-drift file of the earlier hand-run tooling are history. `ReferenceLaneTest` types our side, joins in Java
(`ReferenceJoin`, a port of the Python join this folder held), and compares the whole report (coverage, buckets, every disagreement class) to
`spec/src/test/resources/reference-lane/core_relational.txt`; every class needs a reason in `reasons.tsv` there. The
earlier hand-run tooling's join (`join.py`) and its source-drift script were deleted on
2026-10-18 (the dialect program's D6: no target ran them, and `ReferenceJoin` is the join); git keeps them.

`RefResolutions.java` compiles every module on the classpath with the real Pure compiler (the
interpreted runtime inside legend-engine's shaded server jar, over the jar's own `.pure` sources
and manifests), then writes one row per function-call expression in every function body under
the given source-id prefixes: source id, line, column, the spelling written, the resolved
function's declared name and its signature id, and the enclosing function's.

It is the oracle for name binding and overload choice: our compiler's answer for the same call
is right when it equals this, and every difference is either our bug or an upstream fact we did
not know. `spec/.../OurResolutionsDump` produces our side in the same shape
(`bazel run //spec:our_resolutions -- core_relational [--out FILE]`; by default the dump lands in
`our-resolutions-core_relational.txt` where it is run). Since 2026-09-26 (execution plan step 1) the join is CALL BY CALL, by the
call-name token's (source, line, column): `ReferenceJoin`'s javadoc has the rules, and the source-drift script
marked the sources whose text differs between the jar's version and our pinned trees (241 of
1,218: a position there means nothing), and the docstring names the two spellings that need
their own rule (operator runs, property reads).

> Cut in the 2026-11 docs cleanup: the hand-run instructions (a WebStorm JDK over the 4.138.5 shaded engine jar) and the receipts of the positional join of 2026-09-26 and of the superseded name-keyed join of 2026-09-25. Full text: `git show de2745351:tools/reference/README.md`.

## The reference's overload rule (legend-pure `FunctionExpressionMatcher` / `FunctionMatch`, pinned tree)

For every candidate with the right parameter count, a `FunctionMatch` holds one `GenericTypeMatch`
and one `MultiplicityMatch` per parameter (the argument's type against the parameter's, the
argument's multiplicity against the parameter's). Candidates are ordered by comparing, left to
right, ALL the type matches first and only then ALL the multiplicity matches; the smallest wins;
several smallest is a compilation error, "Too many matches", listing them. Our kernel ranks
differently (it does not prefer the closer multiplicity or the closer numeric type), which is the
whole of the 799 overload disagreements the name-keyed join of 2026-09-25 found. Reproducing this rule — the two match orderings
included — is task #47, the first thing done before step A (`docs/A1_HOMEWORK_2026_09_25.md`, now in `docs/HISTORY.md`,
had the orderings written out and the change list); the binder's acceptance test is this
differential at zero.

## The implicit import group (measured once, 2026-09-25)

`RefImports.java` (removed 2026-10-06, once its finding was recorded; it is in git history) ran the same way as
`RefResolutions.java` and printed, for every source the
reference compiled (2,108 on 2026-09-25), the packages its import group makes visible. Twenty-nine
packages appear in every source: the implicit imports the Pure parser adds to each section. Our
generated `CoreImports.SEQUENCE` is those 29 plus three the newer engine added. So a bare
name in Pure source (the corpus included) resolves by the section's imports and this group alone;
the engine's handler surface is a rule for engine input, not for Pure source. The list was in the
homework note `docs/A1_HOMEWORK_2026_09_25.md` §1.2 (now in `docs/HISTORY.md`). What keeps it true now: `CoreImports.java` is generated from the
engine's own list (`//spec:gen_imports`, from `CompileContext.META_IMPORTS`), and `CoreImportsParityTest` holds it
there.

## Upstream facts found while building

- **Bootstrap order.** legend-pure's runtime must run `loadAndCompileCore()` before `loadAndCompileSystem()`: the M3
  bootstrap creates the Root package the system compile walks (found running `RefResolutions.java`, 2026-09-25).
- **The overload rule, by example** (the 2026-09-25 join over 10,161 function bodies): where both fit, the reference
  picks `isEmpty`'s `[0..1]` overload over its `[*]` one, `max(Integer[1..*])` over `max(Integer[*])`,
  `average(Integer[*])` over `average(Number[*])`, and `between(DateTime…)` over `between(Date…)`: the overload whose
  multiplicity and numeric type are closest to the argument's.
