# H3 — AssociationSynthesis, RelOpTranslator, ViewRelation, XStorePureEnds, ModelJoinNesting, RelationReads, DeclaredCoercions (commit 7c438f7b3)

Paths relative to TREE/core/src/main/java/com/legend unless stated. AS = normalizer/AssociationSynthesis.java (795),
ROT = RelOpTranslator.java (753), VR = ViewRelation.java (457), XS = XStorePureEnds.java (304), MJN =
ModelJoinNesting.java (197), RR = RelationReads.java (192), DC = DeclaredCoercions.java (228), MN = MappingNormalizer.java,
JCE = JoinChainEmission.java, LV = LiftedViews.java. All seven are package-private `final class`, all-static, no static
mutable state. Callers below were found by qualified-name grep over core/spec main+test and verified at the line; no
test calls any of them directly (the only test references are the source-text guards FallbackLedgerTest:56-67,
ParkedWorkLedgerTest:47-49, DynaFnRegistryTest:39).

**Production path.** Compiler.java:280 → ModelNormalizer.normalize → (a) ModelNormalizer:126-127 `new LiftedViews` +
`liftAll` → LV.body:76-115 → VR.viewRelationExpr; (b) ModelNormalizer:128 → MN.normalize(4-arg):152 → MN:187
`normalizeMapping`:258 → everything else here. So every family below except the dead ones is production-reached.

**How a throw becomes an answer (read once, applies to every family).**
- MN.withElement:238-256 turns ModelException / NotImplementedException / MappingResolutionException into a
  `ModelException(NORMALIZE, e.getMessage(), elementFqn)`; MN:198-203 puts its first line in the wall sink
  (`buildModule`/`compileErrors`) or rethrows (strict). **IllegalStateException and NPE are not caught** ("Genuine bugs
  (NPE, ISE) stay RAW", MN:249-251): they fail the whole compile.
- Inside association synthesis, MN:416-435 additionally catches NotImplemented|ModelException from
  `AS.synthesizeAssociationMapping` (which reaches XS, MJN, RR, ROT): `ledger.strictErrors.add(e)` (thrown at MN:192-194
  in a strict build) and `ledger.poison(ForAssociation(...), String.valueOf(e.getMessage()))` → `NormalizationFacts`
  poisons → `PureModelContext.mappingAssociationPoison` (compiler/element/PureModelContext.java:532-537) → the navigation
  wall's text. So the **message text** of every NotImplemented/ModelException in AS-3, XS, MJN, RR and ROT is an answer.
- LV.body:109-111 memoizes a view's ModelException/NotImplementedException (`walls`, identity-keyed) and LV.liftAll:149-156
  writes its full message to the wall sink. Step 0: `model.LiftedViews.walls` 0/112 — **no frozen evidence** that a
  re-thrown view wall (VR-3's throws) is right.

**Hash order, once for all seven.** Every non-Linked hash collection is membership-only: AS:91 `routedUnionGroups`, AS:182-184
`ownClasses`/`ownMapped`/`ownSetIds`, AS:200 `hoisted` (an `add`-as-seen), AS:227 `seen` (cycle guard), AS:262 `have`;
VR:95 `ones` (read only at VR:288 `contains`); ROT:47 `DYNA_HASH_TYPES` (`Map.of`, `containsKey`/`get`); DC:56, DC:177
`Set.of`; `Map.of`/`Set.of` in XS:212-213, RR:41-42, MJN:59/117-118. **Answer-bearing order** exists only in Linked
collections: AS's `byClass`/`bySet` (AS:79-80, iterated at AS:268, AS:292-293 → the order of appended property mappings
→ synthesized class-function bodies), and the scope maps whose `keySet()` is printed in wall text (ROT:112, ROT:234:
`available=[...]`; all 13 scope maps passed to ROT are LinkedHashMap — AS:677, GroupBySynthesis:136, JCE:388/550/571/957,
MN:1825/2021/2245/2282, VR:172, UnionSynthesis:866, ROT:637). `RelationalOperation.tables()` is a LinkedHashSet
(model/RelationalOperation.java:129-137); MN:2661 prints it. Rust: insertion-ordered maps (IndexMap) for all of these.

**Edges.** `compiler.spec.Calls` from all seven (AS:5, ROT:3, VR:3, XS:6, MJN:6, RR:6, DC:6; uses: ROT 24 `platform`,
8 `exact`, 5 `infixRun`, 1 `among`, 1 `infixRunAmong`; VR 8/2; DC 2/5; AS, XS, MJN, RR `exact` only). Compiler root
(lite-names): `ModelBuilder` (AS, VR, XS, MJN, RR, DC), `SynthFqn.mappingAssoc` (AS:538, XS:251), `RelationalKinds.pureKindOf`
(DC:115), `KnowledgeLayer` via `model.knowledge()` (AS:723,725; RR:115,118,144,146; DC:80,82,87,114,147) and via
`MappingClosures.of` → `KnowledgeLayer.derived` (AS:67; the one `derived` fact, a plain HashMap memo, PHASE3_DESIGN.md:379).
`compiler.element.type` (layer `compiler_element_type`, lite-types, allowed): VR:90,96-97 (`Type.RelationType`,
`Type.Column`, `Multiplicity.Bounded.ONE`). **No edge to `compiler.element.*` (F) from these seven**; their only F-adjacent
input is VR-3's `signature`, computed by LV:99 `compiler.element.ViewSignatures.of` (an E→F edge in LV, not here).
builtin: `Pure.Lite.*` constants (all), `Pure.isVariadicRun` (ROT:732), `DynaFn` (ROT:4).

**Model/protocol methods called that Rust lacks (slice 1b deferrals and kin).** `LegacyMappingDefinition.withClassMappings`
AS:219 (the only `withClassMappings` call in these files). `ClassMapping.RelationFunction.Col.bindSrc` RR:131
(model/ClassMapping.java:501-509). `FunctionDefinition.withSynthesizedFrom` AS:544, XS:258. Default methods of
`RelationalOperation`: `navigatesJoin()` ROT:94, `tables()` ROT:98, `mapChildren()` ROT:675, VR:311,327,395 (no
`map_children`/`tables`/`navigates_join` in rust/lite-model or lite-front, grep). `ValueSpecification.children()/mapChildren()`
XS:301, RR:177,190, MJN:192; `AppliedFunction.withParameters` XS:289, RR:162. Already ported: `SetId.defaultFor` (AS:291 →
rust/lite-model/src/names.rs:150 `default_set_id`), `SetId.of` (via ResolvedMapping.idOf). ModelBuilder (lite-names):
`findAssociation`, `importsOf(..).wildcards()`, `findJoin`, `findView`, `findFilter`, `viewMainTable`, `findAssociationOf`.

---

## AssociationSynthesis.java (795 lines) — four families

| Family | Lines | Reached by | Reaches an answer | Idioms | Recommendation |
|---|---|---|---|---|---|
| AS-1 multi-hop / routed injection pre-pass: `injectMultiHopAssociationPMs`, `lineageOf`, `withRoot`, `withInjectedPMs` | 44-322 | MN.normalizeMapping:267 (every legacy mapping) | The rewritten mapping's class mappings (appended Join PMs, hoisted included owner sets, root flag flipped at :214-215) → class-function synthesis → synthesized bodies → G/H/I → SQL; and the class-binding list (hoisted sets become bindings) | identity `m == md` :202 (step 0 `model.AssociationSynthesis.self` 68/66, rows 19); `byClass`/`bySet` LinkedHashMap iterated :268, :292-293 (order → PM order); `MissProbe.neverFired("AssociationSynthesis#1")` :82 (ISE, raw); `MappingClosures.of(model)` memo :67; `withClassMappings` :219; `withInjectedPMs` drops the embedded block's `~primaryKey` (:308-309 uses the 2-arg `Embedded` ctor) | **PORT**, with the identity **RE-EXPRESSED**: iterate only the included part (`MappingClosures...mappings()`), no prepend. Cannot change an answer: `Closure.mappings()` seeds `seen` with the mapping's own FQN (MappingClosures.java:156-157), so `closure[0]` is the only element equal to `md` by FQN or by instance; `m == md` ⇔ index 0 |
| AS-2 association-end helpers: `associationTargetClass`, `associationOwnerClass`, `resolveAssociation` | 340-391 | MN:430, :439 (poison key, binding FQN); ImplicitInheritance:133, :145; MappingClosures:434; AS internal :97, :114, :157, :333, :397, :414 | `resolveAssociation`'s FQN keys the AssociationBinding and the poison (MN:430-441, AS:332-335); owner/target class pick which set gets the injected PM | `resolveAssociation` :376-391 is a second name resolution after NameResolver already resolved the header (NameResolver.java:1268-1284): import wildcards :380-385 and "same-package fallback (an unimported sibling)" :386-390 | PORT the two end helpers. **ASK** for `resolveAssociation`'s :380-390: (a) port verbatim; (b) delete after a probe shows no input reaches it past D (then it is `model.findAssociation`). FallbackLedgerTest:39-45 names exactly this class of code: "the lenient NAME-RESOLUTION fallbacks Phase E still carries ... Those are Phase-D debt — making Phase D total for mapping bodies and deleting them is FIXLIST P7-3, which is not scheduled work" |
| AS-3 predicate synthesis: `recordWithheld`, `synthesizeAssociationMapping`, `EndAnchors`, `endAnchors`, `buildAssocPredicateBody`(11-arg), `anchorTableOf`, `endAnchor`, `anchorOfSet`, `mainTableOfSet` | 325-338, 393-547, 578-784 | MN:418 (per association mapping); `anchorTableOf` also from AS-1 :120-121 | The lifted `(a,b)->Boolean` function (name `SynthFqn.mappingAssoc`, body `legacyAssocPredicate(a,b,srcRel,tgtRel,{srcRow,tgtRow|cond})`, `Synthesized(ASSOC, mapping, assoc, firstProp)`) → resolver's association join → SQL; withheld reasons (:473-479, :489-492) → poison → navigation wall; walls :410-412, :415-417, :422-424, :432-437, :598-600, :633-635, :641-644, :666-672 → poison/strict error (MN:416-435) | `.orElseGet(MissProbe::miss)` :398; `getClass().getSimpleName()` in answer text :411 and :434 (Java record simple names — Rust must spell them); ISE "F7.8 ... AssociationSynthesis#1" :723 (raw) followed by a dead `owner == null` :724; LinkedHashSet :607 read only when size 1; the throw at :690-694 is unreachable by its own comment (":684 Unreachable"; :444 returns before) | **PORT** (every text is an answer). Drop the dead null check :724 and the unreachable :690-694 only if the owner agrees the ModelException text there is never an answer (it cannot be: :444 returns for ≥2 hops, :632 walls 0 hops) |
| AS-4 dead: `buildAssocPredicateBody` 8-arg overload, `anchorNameOf` | 549-566, 786-794 | nothing (each called only by the other; grep of core+spec main and test) | none | — | **DON'T PORT** (nothing replaces them) |

**Notes.** (1) Step 0 classed `AS:202` as (b) "a mapping in its own closure" (PHASE3_DESIGN.md:108); the code says it is
accidental: `closure` is built as `[md] ++ Closure.mappings()` (AS:65-67) and `mappings()` excludes the mapping's FQN, so
the comparison is "skip the element this method itself prepended". 68 hits = 68 calls reaching the hoist, 66 misses = 66
included mappings visited; the oracle covers it. (2) **Surprising: wrong text for an unknown XStore/ModelJoin association.**
AS:397-398 funnels a missing association to `null`; the ModelJoin/Cross arms then skip (`&& ad0 != null`, :399, :404) and
the call lands on :409-412 "Association mapping kind ModelJoin|Cross not supported" — the right text ("references unknown
association", :414-417) sits behind the Relational check, and the pre-pass's "Can't find association" exists only for
Relational mappings (ImplicitInheritance:133-136, MappingClosures:424-428). ASK whether to keep Java's text. (3) **Surprising:
`withInjectedPMs` :308-309 rebuilds an Embedded block with `new PropertyMapping.Embedded(name, sub)`, dropping
`emb.primaryKey()`** (model/PropertyMapping.java:246-248) whenever an association entry nests into it; Java is the spec,
but this looks like a defect — ASK. (4) Documented silent residual, no register row (grep of SEMANTICS_REGISTER.md):
AS:447-454 "First-PM-wins is RETAINED ... Residual: two directions with genuinely NON-equivalent joins would still take the
first silently." (5) `AS:82` uses `model.findAssociation` directly while ImplicitInheritance:133 uses `resolveAssociation`
for the same mapping; an import-only association would pass the pre-pass and die raw here (ISE). Unreached on the census
("never fired"). (6) `anchorTableOf` copies `md.closure()` into a fresh list (:709-710) — harmless.

---

## RelOpTranslator.java (753 lines) — four families

| Family | Lines | Reached by | Reaches an answer | Idioms | Recommendation |
|---|---|---|---|---|---|
| ROT-1 the dispatch — a legacy RelationalOperation tree → a Pure value expression, dynafunction by dynafunction (armed rewrites, else the generic name/candidates call): `DYNA_HASH_TYPES`, `dyna`, `translateArgs`, `toOne`/`toOneAll`/`strCast`, `pureTypeFor`, `ambiguousTableRef`, `translate`, `translateTail` | 46-55, 117-394 (`translate` 217-394), 396-613 | `translate`: JCE:392, :553, :576, :969; MN:1990, :2005, :2024, :2254, :2287; GroupBySynthesis:192, :209; UnionSynthesis:872; VR:176, :191, :216, :227, :236, :250; AS:682 | Every legacy property expression, `~filter`, `~groupBy` key, view column, join condition and association predicate becomes a Pure expression → G types it → H/I → SQL. Wall texts :110-112, :194-198, :209-214, :232-234, :240-242, :278-280 | `dyna` :53 `orElseGet(MissProbe::miss)` (null = not a dynafunction); arity/shape guards that **fall through** to the generic arm (e.g. `adjust` with a non-literal unit, `case` with even arity, `concat` with one arg) and then hit the ISE at :720 "declared TRANSLATED but no translator arm rewrote it" (raw, user-reachable); default arm :610-611 ISE text embeds `op.getClass()` — reached by `RelationalOperation.Lambda`/`LambdaParam` (model/RelationalOperation.java:42-43, 98-110; neither switch has an arm) when a call whose name is not UNSUPPORTED (a plain pure name, or a PURE/SHIM dyna) carries a lambda argument (an UNSUPPORTED name throws NotImplemented first, :557 before :558); `toUpperCase(Locale.ROOT)` :174, :308 (javacompat); left-to-right evaluation decides which error surfaces (:556-559 computes `dynaFnName` before translating args); the two-method split (:391 → :398) exists only for CodeShapeGuardrailTest's METHOD_LIMIT 250 (CodeShapeGuardrailTest:37) | **PORT line for line** (arm order, guards, fall-through, evaluation order, every text). **RE-EXPRESS** `translate`+`translateTail` as one match in the same arm order (same answers: `translateTail` is reached only from `translate`'s default arm). **ASK** on the two raw ISEs reachable from user text (:610 with a Java class name in it; :720): replicate as an internal error, or wall them as NotImplemented (changes answers) |
| ROT-2 `PipelineView` seam, `joinNavigation`, `rebaseToTable` | 56-91, 615-677 | `PipelineView`: implemented by `NONE` (:72-90) and `Pipeline.view()` (Pipeline.java:98-123) only; `joinNavigation` from ROT:606 | The JoinNavigation terminal's slot read `$row.<alias>.<col>` (rebased to the chain-end table per the engine's reprocessAliases, :641-658) in mapping and view bodies → SQL; wall :625-628 | `NONE.slotFor` ISE :84 is unreachable (`joinNavigation` checks `hasSlots()` first, :624); `ambiguousTables()` is Pipeline's HashSet (Pipeline.java:51), membership only; `targetHasColumn` iterates a set for an any-match with `equalsIgnoreCase` (Pipeline.java:115-117) | **RE-EXPRESS** `PipelineView` as `Option<&Pipeline>` (None = NONE: empty ambiguous set, no slots, null target, false). Cannot change an answer: exactly two implementations, `Pipeline.view().hasSlots()` is constant `true`, NONE's only throwing method is unreachable. PORT `joinNavigation`/`rebaseToTable` |
| ROT-3 helpers: `collectTablesIn`, `columnRead`, `literalToValueSpec`, `comparisonFn` | 93-115, 679-703 | `collectTablesIn`: JCE:370, :559; MN:2655; AS:608. `columnRead`: MN:1996, :2364. The other two: ROT-internal (:246, :576) | Column reads in bodies; join-target table choice (via callers); comparisons spelled `equal` / `not(equal)` / Lite `*Any` ordering shims → SQL | `literalToValueSpec` :685-686 text embeds `value.getClass().getName()` (a Java FQN; Rust's literal is a `JValue`, rust/lite-model/src/rel_op_from_protocol.rs:30-31 — needs the Java class-name spelling per variant); `Integer` arm :682 likely unreachable from the parser (unverified) | **PORT** |
| ROT-4 residual-name resolution: `dynaFnName`, `variadicRun`, `dynaFnCandidates`, `operatorCall` | 705-752 | `dynaFnName`: GroupBySynthesis:214, VR:198, ROT:557; the others ROT-internal | The call spelling + candidate FQNs of every non-armed dynafunction (`Calls.among`/`infixRunAmong`) → G's overload resolution → SQL; NotImplemented text :722-724 for UNSUPPORTED (→ wall/poison) | identity `shaped == args` :751 (the (e) site PHASE3.md:213 counts for this file); `DynaFn.resolution()` lookup per call | **PORT**; **RE-EXPRESS** :751 as a boolean (`args.len() >= 2 && Pure::is_variadic_run(fqns)`, the condition at :732) — cannot change an answer, it is the same predicate |

**DynaFnRegistryTest (spec/src/test/java/com/legend/generators/DynaFnRegistryTest.java) — what it needs from a Rust port.**
Its `armsAreDerivedFromTheTranslatorSource` (:137-148) reads `RelOpTranslator.java`'s SOURCE (`TRANSLATOR`, :39), collects
every `DynaFn\.([A-Z][A-Z_0-9]+)\b` and asserts the set equals `DynaFnArms.ARMS` (normalizer/DynaFnArms.java:35-46, 27
members; I checked they match today: MD5/SHA1/SHA256 are named only in `DYNA_HASH_TYPES` :47-48, the rest in `when`
guards). `resolutionsHold` (:51-93) needs TRANSLATED ⊆ ARMS and ARMS ⊆ TRANSLATED ∪ PURE (AND, OR, PARSE_DATE are the
armed PURE ones; rust/lite-builtin/src/dyna_fn_decisions.rs:53,172,174 agree). `engineVocabShimsAreDerivedFromTheRegistry`
(:150-161) needs `DynaFnArms.LANDINGS` (the five FORMAT landings) ∪ SHIM rows = `Pure.ENGINE_VOCAB_SHIMS`. When the Java
translator goes, the test's path dies. A Rust port needs: (1) an `ARMS` set and a `LANDINGS` map in Rust (lite-normalizer or
lite-builtin); (2) either the same source-text check retargeted at the one Rust translator file with regex `DynaFn::(...)`
— which requires the Rust file to name every armed member literally and **no other** `DynaFn::X` (a helper naming a member
outside an arm breaks it) — or a structural replacement (the arm guard as one exhaustive `fn arm(DynaFn) -> Option<..>`
from which ARMS is computed), with a rust/RETIRED_TESTS.md row for the Java test; (3) the LANDINGS/ENGINE_VOCAB_SHIMS
equality kept. Which of (2)'s two forms is the owner's call (ASK).

**Port-verbatim trap (ROT-1).** In a property context (`targetVarOrNull == null`) `translateArgs` already wraps a bare
`ColumnRef` argument in `TRUST_ONE` (:140-142), and the SUBSTRING, ADD, SUB and generic arms then pass that list through
`toOneAll` (:472, :484, :489, :558), which wraps every non-`PureCollection` again: a column operand there is
`trustOne(trustOne(col))`. POSITION (:500-506) and CONCAT (:418-419) wrap once, by other routes. The comments ("every arg
wraps in the 'position' toOne idiom") invite a dedupe; whether the double wrap survives into a frozen plan depends on the
lowering's erasure, not visible from here — port it exactly, do not normalize it.

**Notes.** (1) Code-documented divergences with no SEMANTICS_REGISTER row (grep: only S17 touches splitPart, and on
separators): ROT:357-362 splitPart index base shift "part - 1" and "(Empty tokens still differ: SQL keeps them, pure drops
them.)"; ROT:266-271 dayOfWeekNumber Sunday-based conform `mod(isodow,7)+1`. (2) Step 0 has no arm-level counts for this
file; which arms the frozen oracles exercise is unmeasured.

---

## ViewRelation.java (457 lines) — five families

| Family | Lines | Reached by | Reaches an answer | Idioms | Recommendation |
|---|---|---|---|---|---|
| VR-1 main source as a relation: `mainSourceRef`, `sourceRefFor` | 44-68 | `mainSourceRef`: XS:140; `sourceRefFor`: AS:528-529, VR:50 | The relation argument of `legacyAssocPredicate` (AS) and the XStore end pipeline (XS): a view's lifted body (`md.views().body(view)`) or `tableReference(db, table)` → SQL | `findView(..).orElseGet(MissProbe::miss)` :62 (the miss IS the answer: not a view); `md.views().body` keys LV by the `ViewDefinition` instance (step 0 `LiftedViews.owners` 112/0, `.functions` 393/112) | **PORT** |
| VR-2 `owner` (message suffix) | 78-82 | JCE:359, :835; MN:2563; VR:140, :152 | Wall text suffix `store=<db>` / `mapping=<fqn>` | — | **PORT** |
| VR-3 the view body: `viewRelationExpr`, `declaredOne` | 84-291 | LV.body:100 only (← LV.liftAll, ModelNormalizer:127; and every `views().body` read) | The lifted view function's body (`tableReference`/inner view → join slots → `filter` → `groupByComputedKeys`/`project` → `distinct`), TRUST_ONE on signature-[1] columns → G → SQL; walls :137-140, :148-152 → LV walls | `ones` HashSet :95 membership only; MissProbe :109; `md` is always `null` (LV:101 passes null), so `owner(md, db)` here is always `store=<db>` and JCE.emitJoinChain gets `md=null` (:125, :158, :167); `boolean[] claimed` first-match :202-208 | **PORT**; the `md` parameter can be dropped (always null — passing `None` is the same) |
| VR-4 `inlineViewRefs` | 293-312 | MN:2253 | A table-backed set's `~filter` that names a view's columns reads the view's expressions → SQL | MissProbe :301; `mapChildren` identity-preserving (model/RelationalOperation.java:140-155) | **PORT** |
| VR-5 the frame rule: `frameRewrite`, `frameRewriteIfView`, `declaredSpelling`, `throughFrame`, `pmThroughFrame` | 314-456 | `throughFrame`: MN:1617; `frameRewriteIfView`: MN:2252, :2288, JCE:970; `declaredSpelling`: JCE:367, :554, :577 | View-backed sets' PMs/keys/filters re-pointed at the view's declared columns → bodies → SQL; wall :358-360 | MissProbe :369, :383; `equalsIgnoreCase` :333, :335, :350, :352, :388 (Java's rule: lite-javacompat); `throughFrame` writes `sourceUrl = null` (:416) | **PORT** |

**Notes.** (1) **Surprising:** `pmThroughFrame`'s `default -> pm` (:451-454, "the other kinds carry no column of the root")
passes `EnumeratedExpression` (whose `expression` is a RelationalOperation, model/PropertyMapping.java:109-111) and
`OtherwiseEmbedded` (whose `embedded` sub-PMs can be Column PMs, :285-288) through unrewritten; a view-backed set using
either would read root-table columns outside the frame. No fixture checked. (2) VR:182-246 parallels GroupBySynthesis:160-224
(match keys first-unclaimed, aggregate `fn1`/`fn2` ColSpecs, `GROUP_BY_COMPUTED_KEYS`) with one deliberate difference: an
unmatched view column becomes a `first()` witness (VR:209-224, "H2-LENIENT"), where GroupBySynthesis withholds the property
(:176-184). Not a straight duplicate; keep both. The witness behaviour has no register row (grep for "first()-reduced",
"ANY_VALUE": only S18, which is about `first()` itself). (3) The five MissProbe sites are all "is this name a view?" — Rust:
`Option` matches.

---

## XStorePureEnds.java (304 lines)

| Class | Lines | Reached by | Reaches an answer | Idioms | Recommendation |
|---|---|---|---|---|---|
| `XEnd` + `xstoreEndOf` (end → set: RelationFunction, then Relational as a column view with a lossy flag, then Pure; each kind in closure order), `synthesize` (property-space XStore predicate), `renameReads` | 57-169, 171-261, 263-303 (304 total) | `xstoreEndOf`: MN:955-956 (XStore), MN:1062-1063 (ModelJoin), MJN:94 (nested hop); `synthesize`: MN:966 (Pure or lossy end) | The lifted association function `legacyAssocPredicate(a, b, 'setA', 'setB', {srcRow,tgtRow|cond})` with `legacyLocalProperty($row,'p')` markers → the resolver's association route → SQL; walls :164-168, :189-193, :216-219, :227-231 (PARK-1) → poison/strict | identity `thatRow == srcRow` :202 (two fresh local Variables; also MN:1003) → a bool; `XEnd.localTypes` written (:106, :128, :145) and **read nowhere** (grep `localTypes()`); kind precedence by three passes :83-163, not declaration order; the Relational branch's pipeline is the class's main source (:140 `mainSourceRef(md, classFqn)`) even when `setId` picks a non-root set; sentinel function name `"<relational>"` :143 never read; `renameReads`' explicit AppliedProperty/PureCollection arms (:292-297) drop `ownerClass`/`pos` where `mapChildren` keeps them (both excluded from equality) | **PORT** `xstoreEndOf` and `renameReads` (drop `localTypes`). **RE-EXPRESS** `synthesize` together with MN.synthesizeXStoreMapping:971-1045 as ONE function parameterized by (per-line read rewrite, relation args). Cannot change an answer: the two differ only in the read rewrite (`RelationReads.xstore` vs `renameReads`), the relation args (`endX.pipeline()` vs `CString(endX.setId())`) and where the empty-lines check sits (MN:971 before the loop vs XS:215 after it — with no lines the loop is empty and both throw the identical text); orientation, the canonicalize-and-compare, and all four wall texts are identical. **ASK** about PARK-1 (below) |

**Notes.** PARK-1 (docs/PARKED_WORK_LEDGER.md:20-54; anchor ParkedWorkLedgerTest:47-49, the regex "has direction-specific
conditions" over exactly `MappingNormalizer.java` and `XStorePureEnds.java`): "The rule is also implemented TWICE verbatim
(the column-space and property-space paths), and neither copy has a test" (:45-46); acceptance includes "ONE
implementation" (:48-50); "unifying the duplicate changes the count" (:53-54). The two sites compared: MN:1015-1025 and
XS:221-233 are token-identical (line breaks differ; XS qualifies `MappingNormalizer.canonicalizeEqualOperands`). The anchor is Java-file text
and dies with the Java files. ASK: (a) unify in the port and restate PARK-1's anchor against the Rust site; (b) keep two
Rust sites to mirror the anchor. Upstream: per-end predicates (PARKED_WORK_LEDGER:30-33) — the wall itself is a parked
divergence, so its text is an answer for the four corpus fixtures it lists (:38-43). The `localProps` marker route has an
open leg named in code: XS:276-280 "the typed-local read is an open leg of route A".

---

## ModelJoinNesting.java (197 lines)

| Class | Lines | Reached by | Reaches an answer | Idioms | Recommendation |
|---|---|---|---|---|---|
| `compose` (a ModelJoin condition reading `$end.assocProp.leaf`: the end's pipeline composed with the nested ModelJoin target through `Pure.Lite.JOIN_SLOT`, plus nested column maps), `pairEndVars` (lambda params ↔ association ends: exact type, else engine property-name rule), `collectNestedHops` | 42-142, 144-169, 172-195 (197 total) | MN:1082 (`pairEndVars`), MN:1090 (`compose`), both in MN.synthesizeModelJoinMapping ← AS:400 ← MN:418; `pairEndVars` also MJN:111 | The ModelJoin association function's relation args (composite pipelines) and, via `nestedCols`, RR's slot reads `$row.<prop>.<COL>` → resolver → SQL; walls :87-92, :102-103, :164-168 → poison/strict | `Set<String[]> hops = new LinkedHashSet<>()` :57 — arrays hash by identity, so it dedups nothing; the real dedup is :66-67 (`nestedCols...containsKey(prop)` → `continue`); `MissProbe.neverFired("ModelJoinNesting#1")` :72 (ISE, raw); `model.findAssociationOf` :71 → `ModelBuilder.findAssociationAtClass` (step 0 (c) row 0/0: **no frozen evidence**); last-match-wins loop :77-85 (no `break`) using `model.findAssociation` directly (not `resolveAssociation`), over this mapping's own association mappings only; `Set.of(aVar, bVar)` :59 and `Map.of` :117-118 throw IllegalArgumentException if the two names coincide | **PORT**; **RE-EXPRESS** `hops` as a `Vec<(String, String)>` (or an insertion-ordered set of pairs). Cannot change an answer: a repeated hop is skipped at :66-67 after the first is recorded, so structural dedup yields the same sequence of compositions. **ASK** on the NPE path (note) |

**Notes.** **Surprising, raw NPE reachable from a model:** a nested hop whose nested end class is mapped by a Pure (M2M)
set gets `XEnd(null, null, ...)` from XS:160-161; then `Map.of(nPair[0], rf0, nPair[1], nRf)` (:118) throws NPE on the null
value (and `List.of(nEnd.pipeline())` :124 would too). NPE escapes both MN:419 and MN.withElement → the compile crashes.
The top-level ModelJoin path walls this case (MN:1064-1070 "has a Pure-set end"); the nested path does not. Rust cannot
reproduce an NPE: ASK (a wall mirroring MN:1064-1070's text, or an internal error).

---

## RelationReads.java (192 lines)

| Class | Lines | Reached by | Reaches an answer | Idioms | Recommendation |
|---|---|---|---|---|---|
| `xstore`, `rewrite` (3 overloads), `substVars`: `$this.p`/`$that.p`/`$var.p` reads → column reads on the relation rows (TRUST_ONE when the property is declared `[1]`), expression-bound columns inlined (`Col.bindSrc`), zero-arg derived properties beta-inlined to depth 16, nested-hop slot reads | 36-44, 49-65, 71-180, 184-191 (192 total) | `xstore`: MN:1006; 7-arg `rewrite`: MN:1097, MJN:115. **The 6-arg `rewrite` (49-56) has no caller** | The `{srcRow,tgtRow|cond}` lambda of the XStore/ModelJoin association function → resolver → SQL; walls :90-94, :155-159 → poison/strict | `bindSrc` :131 (slice 1b deferral; substitutes `$src` through `mapChildren` with no lambda-shadowing stop, model/ClassMapping.java:501-509); `substVars` :184-191 likewise ignores shadowing; ISEs "F7.8 ... RelationReads#1/#2" :115, :144 (raw); because both live callers pass a non-null `model`, the `model == null` branches :114, :116, :142 are dead; depth cap `derivedDepth < 16` :142 (a self-referential derived property ends at the :155 wall); explicit AppliedProperty/PureCollection arms :166-172 drop `ownerClass`/`pos` vs `mapChildren` | **PORT** :36-44, :58-191; **DON'T PORT** the 6-arg overload :49-56 and the dead null branches (nothing replaces them: every caller has a model) |

---

## DeclaredCoercions.java (228 lines)

| Class | Lines | Reached by | Reaches an answer | Idioms | Recommendation |
|---|---|---|---|---|---|
| `coerceToDeclaredNumeric` (expression PM → `castAsDeclared` for numeric/temporal/Boolean declared kinds), `declaredPlatformKind`, `declaredAssertion` (join-terminal column → `typeAsDeclared` on a kind mismatch), `coerceColumnToDeclared` (column PM: `castAsDeclared` for String/Boolean, `cast(@DateTime)` over StrictDate, `typeAsDeclared` numeric-over-numeric, `parseInteger/Float/Decimal(toOne(..))` numeric-over-String, else bare) | 53-67, 75-94, 99-122, 133-227 (228 total) | MN.translatePmToField: :1995 (`coerceColumnToDeclared`), :2004 (`coerceToDeclaredNumeric`), :2036 (`declaredAssertion`) | The value expression of each mapped property in the synthesized class function → G types the Lite natives → lowering (castAsDeclared casts at execution, reads bare in the engine-text funnel; typeAsDeclared emits no SQL) → SQL, plan text and result typing | `MissProbe.knownMiss` :80, `orElseGet(MissProbe::miss)` :114; `Set.of` membership :56, :177; `RelationalKinds.pureKindOf` :115; the `ledger` parameters :101, :135 are **unused**, and the class javadoc's `RequiredNullableCensus` hook (:27-30) names a class that no longer exists (grep: only DC and MissProbe mention it) | **PORT**; drop the unused `ledger` params and the stale javadoc (RE-EXPRESS: no read, so no answer can change; the call sites' `pipeline.ledger()` (MN:1997, :2037) throws only on a view pipeline (Pipeline.java:63-68, :89-95), and the one view pipeline, VR:118 `vp`, flows only to JCE.emitJoinChain (VR:124, :157, :166), MN.seedAliasScope (VR:174) and `vp.view()` — never to MN's private `translatePmToField`) |

**Notes.** (1) **Asymmetry:** `declaredAssertion` reads the column with `knowledge().column` (:114), which excludes views
(KnowledgeLayer.java:355-357 `table()`: "Empty for a null spelling, an unknown database, or a view"), while `coerceColumnToDeclared` uses `columnKind` (:147),
which resolves through views (KnowledgeLayer.java:376-400). A join-terminal column on a view gets no assertion; a direct
column PM on a view does. Java is the spec; flag. (2) Code-documented divergence with no register row (grep
SEMANTICS_REGISTER.md for "parseBoolean", "castAsDeclared", "F6": none): DC:47-51 "DELIBERATE divergence (audit 19 F6): the
engine's runtime rule for declared-Boolean strings is Boolean.parseBoolean — 'Y' maps to FALSE, silently. Our SQL cast
ERRORS on such strings instead". Per the upstream-fidelity tenet a divergence that is not a row is a defect — caller item.
(3) `declaredPlatformKind` treats any bare, non-class name as a platform kind (:87-89); after D every user type is an FQN,
so this is the primitives.
