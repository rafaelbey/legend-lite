# The frontend: text to typed model

The frontend turns Pure text (or protocol JSON) into a typed model and typed expressions: stages A to G½ of the
pipeline in [`README.md`](README.md) §2. Store resolution (H) and everything after it are in
[`backend.md`](backend.md), which also holds the mechanics of normalizing a legacy mapping (its §4); this page holds
the canonical form that normalization produces (§5). This page is language-neutral: it says what each stage owns, what it hands on and which
upstream rule it reproduces. A Java name appears only as "today's" implementation. It replaces
`docs/frontend-architecture.md` (now in git history, `docs/HISTORY.md`), whose reasoning (the two compilers, the typed output contract, the frontend
invariants) it keeps.

Upstream sources are cited as the plan audit of 2026-09-26 read them in the pinned trees (legend-pure 5.99.0,
legend-engine 4.145.0). Short names: `FEP` = legend-pure `m3/compiler/postprocessing/processor/valuespecification/
FunctionExpressionProcessor.java`; `FEM` = `m3/compiler/postprocessing/functionmatch/FunctionExpressionMatcher.java`;
`IS` = `m3/navigation/importstub/ImportStub.java`; `CMP` = legend-engine's `legend-engine-language-pure-compiler/.../
compiler/toPureGraph`. The verbatim readings, with every line number, stay in
[`docs/plan-audit-2026-09-26/`](../plan-audit-2026-09-26/): `kernel-reading-2026-09-26.md` (the twelve matcher
methods), `reference-matching.md` (names and overloads in legend-pure) and `engine-resolution.md` (calls in
legend-engine).

## 1. Two compilers

legend-lite has two cooperating compilers, as Roslyn (declaration compiler) and Scala (namer, typer) do.

- **The element compiler** (outer) takes the packageable elements (classes, associations, enumerations, profiles,
  functions, mappings, stores, runtimes, connections, services) and builds the typed model: names resolved, the symbol
  table populated, class hierarchies and property types checked. Its output is a model context of typed elements.
- **The expression compiler** (inner, the typer) takes a value specification (an expression tree), the model context
  and the variable bindings in scope, and produces a typed spec: every node typed, every call bound to one
  declaration. It reads the model and never writes it.

The pair is recursive. Element compilation reaches an expression when a body is demanded (a mapping transform, a
derived property, a constraint, a function body, a service query); expression compilation loads elements through the
model context's lookups. The lazy-loading rules (`AGENTS.md` invariant 5) bound the recursion.

The split is the north-star tenet made into structure (`docs/TENETS.md`): the element compiler is Knowledge, eager and
total, so every reference, kind and existence error is found up front for the whole program; the expression compiler
is Work, demand-driven, and a body is typed only when something uses it. An untyped body is inert, never unsafe,
because using it types it. Two modes stand (the user's ruling D10, register row S1): compile-all types the whole world,
collects every diagnostic and fails on any; a user query types on demand and runs when everything it touches is valid,
even if another body is broken, where legend-engine rejects the whole model. A declaration header's reference is
Knowledge; an unknown name inside a body poisons only that body on user paths and is reported by compile-all.

Sources: `docs/frontend-architecture.md` §1, `docs/TENETS.md`, `docs/EXECUTION_PLAN_2026_09_26.md` D10,
`docs/SEMANTICS_REGISTER.md` S1 (git history: docs/HISTORY.md).

## 2. The stages and their contracts

```
text ─A─▶ tokens ─B,C─▶ protocol records ─door─▶ parsed model ─D─▶ names resolved ─E─▶ normalized model
                                                                                            │
                         typed spec ◀─G─ expression + typed model ◀─F──────────────────────┘
                             │
                             └─G½─▶ inlined typed spec ─▶ store resolution (backend.md)
```

| Stage | Takes → gives | Owns | Must not |
|---|---|---|---|
| A lexer (`rust/lite-front/src/lexer.rs`) | text → tokens | tokenizing | anything else; it depends on nothing but the language's standard library (and, in Rust, the JDK's character classes it reproduces, `lite-javacompat`) |
| B, C parser | tokens → protocol records: elements and value specifications | syntax | carry type information or make semantic decisions |
| the door | protocol records → the parsed model (the same door a JSON model takes) | the model's shape (one full name per element) | let upstream's wire shapes into the model |
| D name resolver | simple names → full names over the parsed model; each call's record of what it can mean | names | consult the compiled model; type anything |
| E normalizer | every executable body → an ordinary function; structure → references by full name | the canonical form | mutate its input; run after the typer |
| F element compiler | normalized definitions → typed elements in a model context | the typed model | type-check a body (F never triggers G) |
| G typer | expression + model → typed spec; overloads chosen; types and multiplicities computed | types | mutate the model; emit SQL IR |
| G½ inliner | typed spec → typed spec with user calls β-inlined; runs on every execution path | substitution | compute a value (and, in the target design, unify or choose an overload) |

**The output contract.** Every expression and call in a typed spec has a type and a multiplicity; every overload has
resolved to one declaration, held on the call node itself, not in a side table. A relational node carries its output
schema (the row struct) inside its type, so lowering reads it and never re-derives it; a schema-carrying value holds a
bare row struct, and the parametric `Relation<T>` form appears only in signatures, where unification binds it (the
practice of Calcite, Catalyst, Substrait and DataFusion). A relational operator gets its own typed node (filter,
project, join, ...); a scalar function is one generic call node tagged by its declaration, again the production
SQL-compiler split. A missing type downstream is a frontend bug, fixed in the frontend (`AGENTS.md` invariant 1).

**The principles behind the stages** (the 2026-09-25 compiler design; its stages and principles stood after the
2026-09-26 audit corrected its facts):

1. **Identity is the declaration.** After binding, nothing names a function, class or property by its spelling.
2. **Each stage owns one question and produces one immutable value.** Parse owns "what tree", bind "which
   declaration", type "which overload, what type". No stage re-asks another's question.
3. **A failure is reported at the stage that owns it, with the source span.** An unknown name is a bind error at its
   span; an ambiguous overload a type error naming the candidates.
4. **Everything known about upstream is generated from the pinned checkouts** and verified by a test that reads the
   same checkout; nothing about upstream is typed by hand.
5. **Everything lite decides for itself is a registration in one total registry**: what it implements, how, and what
   it refuses and why.
6. **The oracle is the arbiter.** A change that needs a special case to keep the rosters is wrong.
7. **Strictness**, as later ruled: collect every diagnostic in one pass, poison the failed unit, fail on any error;
   never accept bad input, never abort on the first error.

Principles 1 and 3 are not yet true of today's code ([`README.md`](README.md) §6).

Sources: `docs/frontend-architecture.md` §2, `AGENTS.md` (Layer ownership), `docs/PHASE_G_SPEC_COMPILER.md` §5 and
§9 (G-α, G-η), `docs/COMPILER_DESIGN_2026_09_25.md` §2, `docs/EXECUTION_PLAN_2026_09_26.md` rule 0b.9 (git history:
docs/HISTORY.md).

## 3. Parsing: protocol first, upstream's bytes, lite's types

**The governing rule.** If legend-engine models an element, lite models it: every built-in section is read into typed
protocol records whose JSON is byte-exact with legend-engine's, and the model is a transform of those records. There
is one parse and one grammar per construct: no second parser straight to the model, no section accepted without being
read, no opaque carrier for a built-in section. An opaque element exists only for a third-party grammar that lite
cannot open, and only the extension seam may make one (today's `OpaqueElementDefinition`, its construction guarded).
The migration that made this true deleted the old straight-to-model mapping and relational parsers; the compiler
consumes the transform's output only. Its lesson: structural equality over the corpus is necessary and nowhere near
sufficient. When the relational section switched, 538 of 561 databases were already identical while nine families
generated wrong SQL; the gates that generate and run SQL were the switchover's proof.

**The protocol is a serialization contract, not a design constraint.** Upstream's protocol is mutable public-field
classes, Jackson subtype annotations, nulls throughout and about twenty bespoke serializers. None of that shapes lite's
types:

| | upstream's protocol | lite's records |
|---|---|---|
| mutability | mutable public fields | immutable records |
| variance | Jackson subtype annotations | closed variant families |
| dispatch | runtime type-id lookup | exhaustive match, no default arm |
| unknown shape | silently absent or null | an error naming it |
| position | a mutable field | an immutable component, set at construction |

All knowledge of upstream's JSON shape lives in one place, the emitter: one exhaustive match over the closed family,
so a protocol type without an emit rule does not compile. The protocol is a clean-room reimplementation: no dependency
on legend-engine's protocol artifacts, which the equivalence harness uses at test scope only to produce reference
bytes. Hand-written emission is easier to make byte-identical than configuring one Jackson to match another.

**The model keeps one full name per element.** The protocol splits `package` and `name`; lite's model does not, and
splits only on the way out to the wire. The single full name is a structural defence against the simple-name lookup
bug of §4.2: no "simple name" accessor exists to invite `findClass(element.simpleName())`.

**The parity bar is a drop-in replacement.** The parser was built to replace legend-engine's grammar parser
completely and byte for byte, as stage one of replacing legend-engine seam by seam (parse to protocol, compile to a
model, plan, execute), each seam a clean-room boundary type proven by a differential harness. Upstream's own tests
compare JSON loosely; lite holds byte equality, so upstream's drift breaks lite where it does not break upstream (three
grammar or protocol changes landed upstream in the roughly three weeks lite's checkouts were behind), and the plan made checking every
upstream release a standing rule. Today the parser-equivalence gate holds 22,725 elements
byte-identical, and protocol JSON read back rebuilds records identical to the parser's (the user's Rule A, held
corpus-wide). The reason for the drop-in was never speed (54.5 times faster, the weakest of six reasons): memory
(32.9 times fewer bytes allocated per source character), generated code, build cost, a pinned ANTLR, debuggability.

**A parser always knows which dialect level it serves** (today's `parser.Dialect`; `docs/DIALECT_LEVELS.md`):
lite's own platform dialect (bootstrap and platform sources only), exact legend-engine (the drop-in surface, which
refuses the other two with the engine's own messages) and lite (the engine's grammar plus declared extensions such as
mapping-as-function, nothing undeclared). At the engine level lite reads exactly what the engine reads, including text
the engine's grammar accepts and then drops (register row S89). Upstream's oddities are reproduced, not fixed: the
engine's one-level "snatch the last parameter" arithmetic precedence (`1 < 2+3*4` parsed as `1 < (2+3)*4`) is
legend-pure's own rule (`AntlrContextToM3CoreInstance.java:1909-1939`), so reproducing it is fidelity.

Sources: `docs/PARSER_COMPLETENESS_PLAN.md` (governing rule, §1, §2, §R3), `docs/PARSER_DROP_IN_PLAN.md` §1, §2.1,
§4.1, §4.1.1, §6, §8, `docs/plan-audit-2026-09-26/architecture-review-2026-09-28.md` §6 (git history:
docs/HISTORY.md); `docs/rust-port/ROADMAP.md` (the front seam, phase 2 gates), `docs/DIALECT_LEVELS.md`,
`docs/SEMANTICS_REGISTER.md` S89.

### 3.1 Parser parity with legend-engine

Three parser decisions, closed during the parser build and still cited in the code as D-2, D-6 and D-7
(`core/README.md`), each settled by matching legend-engine:

- **D-2: an unknown key is an error.** An unknown top-level key in a `Runtime`, `RelationalDatabaseConnection` or
  `Service` body throws a parse error naming the key; the service and persistence section grammars refuse unknown keys
  the same way. legend-engine's ANTLR grammar makes an unknown key a syntax error (its runtime walker validates each
  required and optional field: `RuntimeParseTreeWalker.java:99`, as the decision log cited it). Only lite's deleted
  `engine/` module skipped unknown keys (it skipped to the next semicolon). Pinned by `ElementParserTest`
  (`runtimeRejectsUnknownTopLevelKey`, `connectionRejectsUnknownTopLevelKey`, `serviceRejectsUnknownTopLevelKey`,
  `singleConnectionRuntimeRejectsUnknownKey`).
- **D-6: a view's `~filter` is a closed type.** Lite models a view's filter as a sealed `FilterMapping`: `Direct`
  (one `FilterPointer`) for `~filter F` and `~filter [DB] F`; `JoinMediated` (the source database, the join chain, the
  target `FilterPointer`) for `~filter [DB1] @J1 > @J2 | [DB2] F`, one hop or several. `FilterPointer` is sealed in
  turn: `Local(name)`, which searches the enclosing database and its includes, or `Cross(db, name)`, which searches only
  the named database. legend-engine's protocol has the same information in a flat `FilterMapping{filter, joins}` over a
  `FilterPointer{db, name}` with a nullable `db`; the two encodings are isomorphic, and the sealed one makes a consumer
  handle both resolution paths. In the join-mediated form the source `[DB1]` and the target `[DB2]` are both required:
  the grammar's `viewFilterMappingJoin` ends in a database pointer (`RelationalParserGrammar.g4:143`, cited in the
  test). The model type would hold a local target, but lite's parser refuses `~filter [DB] @J | F`, a leniency that
  died with the legacy parser; every one of the 36 corpus uses writes the target `[DB]`. Pinned by `ElementParserTest`
  (`viewFilterDirectLocal`, `viewFilterDirectCross`, `viewFilterJoinMediatedRequiresTargetDbQualifier`,
  `viewFilterJoinMediatedMultiHop`, `viewFilterJoinMediatedCrossTarget`, `joinMediatedFilterRequiresSourceDbQualifier`,
  `filterMappingJoinMediatedRejectsEmptyJoins`).
- **D-7: a bare column in a database expression is refused.** A bare column identifier in a Database-context
  expression (a `Filter`, a `Join`, a `MultiGrainFilter`, a view filter) throws "Missing table or alias for column
  'X'" (today's `rust/lite-front/src/database_parser.rs:1326`; the Java `parser/DatabaseProtocolParser.java:1160` until
  2026-11-06), legend-engine's own words: its grammar accepts the bare
  identifier and its walker refuses it (`RelationalParseTreeWalker.generateTableAlias`, walker line 993 as cited in
  lite's parser). Neither implementation has an implicit-table column reference. Under a scope header (a table
  written before a block of columns, as a class mapping's table is) a bare identifier is a column of that header
  table, as the engine's scope-info grammar reads it. Pinned by `ElementParserTest.filterRejectsBareIdentifierMatchingEngine`.

Sources: `core/README.md` (closed decisions D-2, D-6, D-7) and its earlier decision log (`git show
de2745351:core/README.md`); `docs/PARSER_COMPLETENESS_PLAN.md` §R3 (git history: docs/HISTORY.md);
the parser, Rust's since the Java one was deleted on 2026-11-06 (docs/rust-port/PHASE2.md §4 step 6):
`rust/lite-front/src/database_parser.rs:1290-1326` (the scope header and the bare column),
`rust/lite-front/src/runtime_parser.rs:248` and `rust/lite-front/src/service_parser.rs:205` (an unknown key),
`rust/lite-front/src/persistence_parser.rs`, `rust/lite-front/src/connection_parser.rs`
(the Java `ElementParser.java`, `DatabaseProtocolParser.java:1133-1160`, `section/ServiceSectionGrammar.java` and
`section/PersistenceSectionGrammar.java` they port are in git history);
`core/src/main/java/com/legend/model/FilterMapping.java`, `core/src/main/java/com/legend/model/FilterPointer.java`,
`core/src/test/java/com/legend/parser/ElementParserTest.java`.

## 4. Names: upstream's rules, resolved once

### 4.1 What upstream does (read from source)

**Element references** (a class, enumeration, store, mapping named in a model or query; `IS:181-234`): primitive type
names and `Package` first; a qualified name is absolute; a bare name is looked up in **all import-group packages as
one set**, the core import group and the section's own imports together. More than one hit is an error ("X has been
found more than one time in the imports"); no hit falls back to a top-level (Root) element, else "X has not been
defined!". There is **no own-package tier** and no order between section and core imports.

**Function candidates** (`FEM:159-170`, `Imports.java:52-65`): a qualified call searches exactly that package (an
unknown package gives zero candidates, so "can't find a match", never "unknown package"); a bare call searches the
core imports, the section's imports and Root, **as a set**. The call's own package is not searched unless imported;
Root functions are visible everywhere; there is no precedence between packages, so two same-shape overloads in two
imported packages tie ("Too many matches"), never "nearest import wins".

**The core import group** is `system::imports::coreImport`, declared in `m3.pure:175-213`: **29 packages** (the m3
metamodel packages, `meta::pure::functions::*`, router, service, tds, tools, profiles). A section's import group holds
only its own `import x::*;` lines.

**Engine input** (model and query text in legend-engine's grammar, compiled by legend-engine's own compiler;
`CMP/CompileContext.java:544-635`, `CMP/Handlers.java`): the handler map is the whole function namespace, user
functions included (registered under `package::name` without signature). A qualified `meta::` spelling collapses to
its bare name when the registry knows the package; a bare name is tried as `<import>::<name>` over the engine's 32
`META_IMPORTS` and the section's imports, against the same map; the graph is never consulted for calls. Overloads are
chosen by hand-ordered dispatch tests, the first that passes wins, with no tie error. Every handler carries its
return-type inference (830 ids in `Handlers.java`, 61 more in extensions, eight named inference functions). Pure
source (the relational corpus, the platform's modules) is compiled by legend-pure and never takes this path.

**Lite's rule for engine input** (the user's ruling D13, register row S3): one rule, legend-pure's outcomes, over the
engine's namespace (its 32 imports and handler names). The engine's first-match order is registration order, not
semantics, so lite does not reproduce it; every disagreement is to be classified into the register.

### 4.2 What lite does today

**Elements** (today's `NameResolver`, per element, in its own section's imports): reserved primitive names first; then
the section's wildcard imports; then the element's own package; then the core import group, in the group's order,
first match; otherwise the name stays unresolved and fails downstream as the engine's "Can't find type" does. A
platform-wide fallback by simple name was deleted (the user, 2026-09-08: "bare names must fail like pure/engine").
**Functions**: a bare call's record (§4.3) holds the program's candidates from the same tiers (imports, own package,
core group) and the platform's names by one bare-name rule: legend-engine's handler surface, the core-import packages
and the form's own declarations. Program and platform candidates are a union, never tiered, because legend-pure has
no user/platform tiering for function matching.

This differs from §4.1 in ways that have no register row (reported in this cleanup): the own-package tier; tiers
where upstream has one set; the first match in the core group where upstream errors on two hits; and a core group of
32 packages where legend-pure's has 29 (the extra three, `metamodel::variant`, `metamodel::relation` and
`precisePrimitives`, are engine-only, so a bare call that resolves only through them compiles in lite and fails in
legend-pure, which the 2026-09-26 audit called lite's bug). The same audit found the engine's handler surface applied
to every bare call, Pure source included, with nothing recording which grammar produced a tree; which bare names
resolve only through it was not measured. The compiler plan's item for adopting upstream's
candidate rule for Pure source (W2.3b) did not land before the plan was parked. A bare element name that only an
own-package tier resolves shows up only in a compile-status differential, not in an overload one.

**The lasting rule from the name-resolution bug** (fixed 2026-08-02): an unimported element never binds. The bug
resolved an unqualified name by scanning the whole model for any full name ending in `::<name>` and bound a unique
hit, so a model that real Pure rejects compiled and emitted SQL against the wrong store. The fix made every lookup
after resolution exact-full-name only and stopped the platform's names from overwriting an explicit import. The test
suite was structurally blind to it: each corpus family compiled as an isolated module has one candidate per simple
name, so the scan was usually right by construction. Compiling the corpus as one model (every family registered
first, one strict parse, a per-test execution overlay instead of a recompile) is the forcing function that keeps the
bug class unrepresentable; it can land only after resolution is right, or every ambiguous reference fails at once.

### 4.3 Resolve once

**Every call's names are worked out once and recorded on the call; nothing downstream resolves a name.** (L7, landed
2026-10-10, closing parked item PARK-5.)

- A **parsed call** gets its record from the name resolver: every full name the call can mean, the program's
  candidates from its tiers and the platform's referents alike (today's `AppliedFunction.referents`).
- A **built call** (one the typer, a desugar, the normalizer, validation or lineage makes after resolution) is born
  with its record, through one builder, the only place after the resolver that may make a call from a name (today's
  `Calls`, guarded by `ArchitectureTest.callsAreBuiltOnlyByTheBuilder`). A built call is what the same call parsed
  would be, spelling included: the builder asks the resolver, under the sectionless-query scope (the platform's
  prelude alone: no imports, no own package), memoized per name and arity. A rebuilt call keeps the original's record.
  A built call names the platform's function by design; the one built call that must resolve in the user's namespace
  (a dynafunction with no registry row) keeps an empty record on purpose.
- A **call with a record is not resolved again**: a second pass over a synthesized body keeps its spelling and record
  (resolution is idempotent).
- **Readers read the record.** The typer collects candidates from it; a call whose record is empty after resolution
  is an unknown function, reported from the record. A checker that needs another form's declarations, or a form at an
  arity the call does not have, asks the resolver's answer for that name and arity. A wrong-arity call to a known name
  keeps its arity message.

Nothing about which names a call can mean changed: only when the question is answered (once) and where the answer
lives (on the call). Measured at landing: on the eager corpus compile, typing's median fell from 2,701 to 2,188 ms
(the name lookup had been 29% of typing time); the decision probe's candidate rows matched the old rule's except 48
calls in synthesized bodies, each explained (they keep their bare spelling with a one-name record); the typing census
was identical; none of the render census's 52,530 entries changed.

The record is still a list of full-name strings. The target the plans set is a distinct resolved tree in which every
call carries declaration handles, a dot access stays `Member(name)` until the typer knows the receiver's type, and
every binder has an id (`docs/EXECUTION_PLAN_2026_09_26.md` D12, §1, in git history); it is not built.

Sources: `docs/PLAN_AUDIT_2026_09_26.md` §1 rows 6–8 and the core-import paragraph,
`docs/NAME_RESOLUTION_BUG.md`, `docs/build-inventory/program/L7_RESOLVE_ONCE_DESIGN_2026_10_10.md` §§1–4, §10, §12,
§13, `docs/EXECUTION_PLAN_2026_09_26.md` D12, D13, W2.3b (git history: docs/HISTORY.md);
`docs/plan-audit-2026-09-26/reference-matching.md` findings 1–3, `kernel-reading-2026-09-26.md` items 28–29,
`engine-resolution.md` finding 1; `docs/SEMANTICS_REGISTER.md` S3; the 2026-10-10 L7 entry of `docs/GATES.md`
(`git show de2745351:docs/GATES.md`); `core/src/main/java/com/legend/compiler/NameResolver.java` (`resolveNameMulti`).

## 5. One canonical form: everything executable is a function

**Every executable body has one canonical form, a function definition.** Mapping transforms, derived properties,
constraints, service queries and user functions are all ordinary functions in the model's element list. Structural
elements are expression-free and reference behaviour by full name: a mapping is a binding table, a derived property a
signature plus the name of its body function. No value specification lives on a structural record after
normalization.

**Four doors converge on that form before typing:**

1. clean-sheet mapping text, where a class binds a function by name or an inline expression (a lite extension, the
   mapping-as-function family of the lite dialect);
2. the legacy mapping DSL, which the normalizer translates into functions plus a binding table, exactly what door 1
   produces for the equivalent text;
3. inline binding expressions, lambda-lifted into functions;
4. plain-function authoring for derived properties, constraints and service queries (a member binds a function by
   name).

**Faithfulness is a parse property; meaning is a normalize property.** Each surface grammar parses into records
faithful to its own syntax; normalization is the single desugaring point; after it exactly one representation exists,
and no later stage can tell which door the input came through. That is the shape of every desugaring compiler (GHC's
surface syntax lowered to Core, rustc's AST to HIR). What made upstream messy was never grammar count but leakage:
surface records flowing past the desugar boundary. The convergence property makes this testable: a legacy mapping and
its hand-written clean-sheet equivalent produce identical normalized models, modulo lifted names and provenance
(today's `LegacyCleanSheetConvergenceTest`).

**Lifted functions use the reserved `$` sigil**, which no user can write, so they never collide with authored names
in the one function index. One injective scheme names them, embedding the full owner or target name (a simple name
would collide when `a::Person` and `b::Person` are mapped in one mapping):

| Hat | Name |
|---|---|
| derived property | `<classFqn>$prop$<name>` |
| constraint | `<classFqn>$constraint$<name>` |
| service query | `<serviceFqn>$query` |
| class transform | `<mappingFqn>$class$<classFqn>[$<setId>]` |
| association predicate | `<mappingFqn>$assoc$<associationFqn>` |

A lifted function differs from a hand-written one only by its name and a provenance tag (what it was synthesized
from, for messages in the user's terms); nothing downstream may branch on either. A lifted body's nodes are the nodes
the parser built, so their positions can survive; the design also gave nodes the normalizer fabricates the span of the
construct they realize, which today's normalizer does not do ([`README.md`](README.md) §6).

**Bindings are the scope; the function index is global.** Every function, authored or lifted, lives in one index, so
several mappings of one class coexist without conflict, because nothing dispatches by name pattern. `Person.all()`
under mapping `M` resolves through `M`'s binding table to a function name, then to the one index; `$p.fullName`
through the class's derived-property binding; constraint checks enumerate the constraint bindings; a service reads its
query function's name. It is always a binding lookup, never a body walk or a name probe. The typer is therefore one
body checker for every kind of body: it needs no mapping-target context, no worklist of mapping bodies discovered while
typing and no side record of which properties a body touched; mapping realization is store resolution's job, read
off the binding table. How a legacy mapping's semantics become relational function bodies is in
[`backend.md`](backend.md) §4; the surface forms are specified in `docs/MAPPING_CLEAN_SHEET.md` and
`docs/MAPPING_LEGACY_TO_FUNCTION.md`, the two-layer thesis in `docs/STRUCTURE_VS_BEHAVIOR.md`.

Sources: `docs/CLEAN_SHEET_INVERSION.md` §1, §1.5, §3, §5, §6, `docs/PHASE_G_SPEC_COMPILER.md` §0, §7 (git history:
docs/HISTORY.md); `docs/DIALECT_LEVELS.md`; `core/src/main/java/com/legend/model/SynthHat.java`.

## 6. Elements: eager knowledge, lazy bodies

The element compiler builds the typed model from the normalized definitions and opens no body: a function body stays
an untyped expression inside its typed function and is typed on demand (`AGENTS.md` invariant 6; legend-engine
violates this when it builds its functions, and lite does not carry the violation forward, `core/README.md`
invariant 7). The cheap index of what exists (existence, kind, signature) is eager over the declared dependency
closure; bulky cross-project structure is materialized on demand through the model context, never by a transitive
load (`docs/TENETS.md`). A user type is a full name in a typed wrapper, not a loaded class; structural questions
(find a property, is-subtype, a superclass walk) go through the model context, the only layer that triggers a load;
long-lived fields hold full names (`AGENTS.md` invariant 5, `core/README.md` invariant 11).

The platform's standard library is lite's own natives, their signatures verified against upstream's `.pure` sources,
and parsed twins of `meta::pure::functions::*` are suppressed (register row S10). The platform ships declarations
only: class and enumeration shapes and native signatures, plus views over its own system tables
(`docs/WORLD_MAP.md` §3).

Sources: `docs/frontend-architecture.md` §4.2, `docs/PHASE_G_SPEC_COMPILER.md` §0 (git history: docs/HISTORY.md);
`AGENTS.md`, `core/README.md`, `docs/TENETS.md`, `docs/WORLD_MAP.md`, `docs/SEMANTICS_REGISTER.md` S10.

## 7. Typing

### 7.1 One bidirectional kernel

Function bodies are typed on demand by **one kernel with an expected-type slot**: type a node against `Check(type)` or
`Infer(fresh variable)`; inference is checking against a fresh meta-variable (as OCaml's `type_expect` and GHC's
`instSigma` do). Native and user calls differ only in the callee's **type scheme**:

- a **native** is a polymorphic scheme (`map<T,V>(T[*], {T->V[*]}): V[*]`): instantiate it with fresh type and
  multiplicity variables, check each argument against the instantiated parameters, and unification solves the return;
- a **user function** is a monomorphic scheme (user signatures have no free type variables): instantiation is a
  no-op, and the same rule checks the declared return.

One application rule covers both: instantiate, check the arguments, unify the return with what is expected. The one
genuine branch on mode is **lambda introduction**: a checked lambda unifies against the expected function type; an
inferred one invents a fresh variable per parameter. A lambda argument is typed after the parameters it flows into are
known, so the expected type flows into it.

A call is typed from its callee's **declared signature, never its body**, so recursion is never rejected and needs no
cycle guard (the textbook rule; legend-engine's rejection of cyclic call graphs exists only to protect its
recurse-into-bodies design). Typed bodies are memoized per model snapshot: the memo lives with the model context, so an
edit makes a new context and an empty memo. Element compilation never opens a body.

### 7.2 Choosing an overload: upstream's matcher

The product is judged against legend-pure, so the matcher's outcomes must be legend-pure's, oddities included
(`docs/PLAN_AUDIT_2026_09_26.md` §1 rows 1–4, in git history; `kernel-reading-2026-09-26.md` has every method):

- **The loop.** legend-pure collects every candidate that matches leniently (untyped lambda parameters and empty
  column types match anything), sorted. It loops over them in that order; for each, it binds type and multiplicity
  parameters from the typed arguments, types the lambda arguments against **that candidate's** signature, then
  re-ranks all candidates strictly and accepts this one only if it is still the best; otherwise it unbinds and tries
  the next (`FEP:121-265`, accept `:210-215`, retry `:223-227`, the lenient search `:387`). The winner is the first
  candidate in lenient order that is strict-best with the lambdas typed against it. A strict tie raises "Too many
  matches".
- **Type match order**: a simple match (its distance is the index in the C3 linearization, not a count of
  generalization steps) < non-concrete (a type parameter `T`, which ranks **below** every concrete match, `Any`
  included) < relation or function < bottom (the argument is `Nil`) < null (`TypeMatch.java`, `GenericTypeMatch.java`).
- **Multiplicity match order**: exact < non-concrete < by upper then lower distance. `[1..*]` does **not** accept
  `[*]`; a `*` upper bound against a bounded one is the maximum distance; a collection literal `[1,2,3]` has
  multiplicity exactly `[3]`, which is why the `[1..*]` overloads win (`MultiplicityMatch.java`,
  `InstanceValueProcessor.java:194-211`).
- **A type variable bound twice is merged** by least upper bound with the parameter's variance, never an error;
  multiplicities merge by subsumption (`TypeInferenceContext.java:330-440, 276-281`).
- **Zero candidates is not always immediate**: if every candidate failed only because a lambda could not be inferred
  yet, the call keeps its raw signature's return type and is reprocessed when the enclosing call binds more.
- **Member access is rewritten**: a dot on a receiver that is not exactly `[1]` becomes `map` over the receiver (to-one
  is strict: `[0..1]` automaps); an enumeration value becomes `extractEnumValue`; a milestoned property gets its dates.

Lite keeps the outcomes and not the representation (the user's ruling D15): its own inference engine, a written rule
table and a **deterministic** tie rule where legend-pure breaks ties within the lenient order by hash order (row S7);
it does not copy a merge mode legend-pure's own source calls a bug (row S8); member access and enumeration values keep
lite's own typed representation, with the same result type and multiplicity, rather than the `map` and
`extractEnumValue` rewrites (rows S5, S6). Two facts matter for anyone writing this kernel. First, a form's typing rule cannot be chosen before
its lambdas are typed, and the lambdas need the form's rule; the reference's loop resolves this by letting each
candidate supply the lambda's parameter types, so a form dispatches when any candidate has a form rule. Second, as of
the 2026-09-28 review lite's kernel still chose overloads by an additive score with tie-breaks, and its deferred path by
the first candidate that typed; the solver that adopts the loop (plan item W3.3) did not land before the plan was
parked. (The earlier design's additive scheme, exact 2, subtype 1, type variable 0, times ten plus a multiplicity
score, was the predecessor engine's algorithm and is superseded by this reading.)

### 7.3 Forms and special forms

A **form** is a function whose result type depends on its arguments in a way its signature cannot express (`project`,
`extend`, `groupBy`, `join` and their kin); upstream gives each a return-type inference keyed by its handler. Lite
classifies each native on two independent axes: its type rule (driven by its signature, which handles relations too
because the relational cases are in the kernel, or bespoke when the signature cannot express the output) and its output
node (the generic call node, or a distinct relational node that lowering must tell apart). The closed vocabulary of
language forms is one enumeration (today's `CoreFn`): a name is resolved to a form exactly once, every dispatch over it
is exhaustive, and the several hundred scalar and collection natives ride the generic signature-driven path.

**Special forms and the execution context are two kinds of knowledge kept apart.** A special form (`from`, `execute`,
`executionPlan`, `executeInDb`, the DDL natives) is a native whose arguments are programs or contexts, not values; its
implementation is a compilation rule, as `if` and `let` are special forms in any language, and one catalog names and
classifies them for every other component. The **execution context** (the mapping, the runtime, its connections'
setup data, model-chain mappings, connection flags such as identifier quoting, time zone and database type, JSON
sources) is evaluated **once, as an immutable value**, and bound onto the `from` (today's `ExecutionContext` on
`TypedFrom`). One reader knows the runtime's field names; every consumer reads a field; nothing re-derives the context
by walking the tree. Setups run once per statement, in tree order, deduplicated. The design came from a regression:
nineteen tree walks and 76 literal name checks had accumulated, and two corpus tests broke because one route forgot to
re-derive a context the others walked for ("are we building a general purpose pure runner or a super hard-coded test
runner?").

### 7.4 A TDS is a relation

The legacy TDS natives carry **upstream's exact signatures** (`TabularDataSet`, `TDSRow`, `Table`), while the checker
treats a TDS as a `Relation<T>` whose schema it knows; there is no ledger of respellings. Two rules, each in one place:

- **`TDSRow` erases where a type expression becomes a type** (signatures, class properties, annotations): it becomes
  the erased, late-bound row, and a concrete row struct conforms to it. Nothing downstream sees a nominal `TDSRow`.
- **A declared `TabularDataSet` result refines to the argument's actual relation** (or the relation a query lambda
  argument returns), substituted inside the output's shape, on the eager and the deferred call paths alike: it is the
  kernel's output rule, a refinement of the signature's output, never a bypass of its checks.

The cell getters (`getString`, ...) are, as upstream declares them, qualified properties of `TDSRow` and of the
result set's `Row` (`value`), implemented by the platform: a literal column name folds to the column read at type time
(typed by the schema when the row is concrete, by the getter's declared type when erased); a non-literal name stays a
call to the property, typed by its declaration. legend-pure's runtime carries a TDS's columns at run time; lite carries
them at type time; the emitted SQL is the same.

### 7.5 Numbers and precision

Inside a query the database's own types rule: literals render bare and arithmetic runs in the kind the database gives
it; at the boundary every value takes the kind the model declared; a Number-declared arithmetic result has one owner
for its kind (`docs/NUMERIC_CHARTER_2026_09_17.md`, rules 1, 2 and 2a; today's `NumberKinds.refine`; register row S25
for where legend-engine is stricter).

On the type itself, lite carries a Decimal's **precision and scale** (`Decimal(p,s)`, today's `Type.PrecisionDecimal`),
taken from store columns and from a literal's text (`1.50d` has scale 2; an unparameterized Decimal is `(38,18)`). They
are invisible to subtyping and overload resolution: a `Decimal(p,s)` subtypes and unifies as `Decimal`. The reason
given (2026-06): lite emits SQL, where `DECIMAL(p,s)` is mandatory, while upstream's Decimal is precisionless at the
language level because it computes in memory. The same design also planned integer width on the type and a precision
algebra for arithmetic; neither is in today's code: the width-carrying precise integer primitives alias to `Integer`,
and no caller of the precision formulas was found. Upstream does carry precision on legend-pure's precise primitives
(`Numeric(precision, scale)` extends `Decimal`; `Varchar`, `Int`, `Timestamp`), which legend-engine uses to name
relational columns in `lambdaRelationType`; lite answers those calls with its own names (`Decimal` with its precision
and scale as type-variable values, `Integer`, `DateTime`), a vocabulary gap recorded in the endpoints design and
allowed by name in its parity test. Neither difference has a register row (reported in this cleanup).

Sources: `docs/PHASE_G_SPEC_COMPILER.md` §2, §3.4, §4, §8, §9 (G-γ, G-ε, G-ζ), `docs/PLAN_AUDIT_2026_09_26.md` §1
rows 1–5 and §2 (B), `docs/COMPILER_DESIGN_2026_09_25.md` §0, `docs/EXECUTION_CONTEXT_DESIGN_2026_09_06.md`,
`docs/TDS_ERASURE_DESIGN_2026_09_11.md` §1, §4b, §7, `docs/UPSTREAM_ENDPOINTS_DESIGN_2026_09_27.md` (vocabulary),
`docs/EXECUTION_PLAN_2026_09_26.md` D15, `docs/plan-audit-2026-09-26/architecture-review-2026-09-28.md` §2 (git
history: docs/HISTORY.md); `docs/plan-audit-2026-09-26/kernel-reading-2026-09-26.md`, `reference-matching.md`
findings 4–6; `docs/NUMERIC_CHARTER_2026_09_17.md`; `docs/SEMANTICS_REGISTER.md` S5–S8, S25;
`core/src/main/java/com/legend/compiler/element/type/Type.java`, `core/src/main/java/com/legend/platform/CoreFn.java`,
`core/src/main/java/com/legend/plan/UpstreamRelationType.java`.

## 8. Inlining (G½)

After typing, user calls are β-inlined, on every execution path. The inliner **compares and never computes**
(`docs/WORLD_MAP.md` §4): over spelled literals it may decide what the text itself shows (which `match` arm a
literal's class picks, a spelled field, list shape over spelled lists, identity of two spelled scalars of the same
kind), and it unrolls a recursive function while its literal argument strictly descends; anything that produces a new
value (string functions, arithmetic, `toString`, cross-kind equality) stays in the query, and a decision that hangs on
a computed value carries both outcomes for the database to pick (a residual `CASE`). Plan-time evaluation may compute
only what fixes the shape of the SQL (column lists and names, which branch, which lambda body, type tokens), never a
value the database could compute (register row S2, where legend-engine pre-evaluates constants). The target stage
substitutes by binder identity with each call's recorded instantiation, and does no unification and no overload choice.

Sources: `docs/PLAN_AUDIT_2026_09_26.md` §2 (C), `docs/EXECUTION_PLAN_2026_09_26.md` §1 (git history:
docs/HISTORY.md); `docs/WORLD_MAP.md` §4; `docs/SEMANTICS_REGISTER.md` S2.

## 9. Frontend invariants

These mirror the backend's and hold in any implementation.

1. **The name resolver is model-free.** It works on the parsed tree, a precomputed set of full names and the import
   lists, never on the compiled model, so it runs before model construction with no circular dependency and is
   independent of loading order.
2. **The element compiler never compiles a body.** Bodies are typed when something demands them (a query runs, a
   service is invoked, an association is traversed): lazy loading extended from classes to bodies.
3. **The typer never mutates the model.** Every "what is the type of x" question goes through the typed elements; new
   entries appear only through the element compiler.
4. **Names are resolved once.** No stage after the resolver resolves a spelling; every call carries its record; every
   built call goes through the one builder (§4.3).
5. **An unimported element never binds.** Lookups after resolution are exact full names (§4.2).
6. **After normalization there is one representation.** No stage can tell which door a body came through (§5).
7. **No fallbacks at any stage.** Each stage fails loudly on missing input: no default arm in checker dispatch, no
   silent degradation of a type to `Any` (`AGENTS.md` invariant 4).
8. **No dispatch on spellings.** Dispatching on a type's or a function's name string is the frontend's version of the
   SQL IR's catch-all function call; the remedy is a closed enumeration or variant family, as the form vocabulary is.

(The old fourth invariant, that the store resolver only reads the normalized mapping, belongs to store resolution:
[`backend.md`](backend.md) §5.)

Sources: `docs/frontend-architecture.md` §4,
`docs/build-inventory/program/L7_RESOLVE_ONCE_DESIGN_2026_10_10.md`, `docs/NAME_RESOLUTION_BUG.md`,
`docs/CLEAN_SHEET_INVERSION.md` (git history: docs/HISTORY.md); `AGENTS.md`.

## 10. Vocabulary

| Term | Meaning here |
|---|---|
| frontend | lexing through inlining: source text to fully typed, inlined expressions (stages A to G½) |
| midend | stages on typed IR that produce no target code: store resolution and lowering |
| backend | the dialect: SQL IR to SQL text |
| element compiler | the outer compiler: packageable elements to typed elements in a model context (stages D to F, invoking the inner one on demand) |
| expression compiler, typer | the inner compiler: an expression and the model to a typed spec (stage G, reusing A to D for query text) |
| HIR, typed spec | the frontend's typed output: Pure semantics (lambdas, multiplicities, navigation, classes) |
| MIR, SQL IR | lowering's typed output: relational algebra in SQL terms, dialect-free (`backend.md`) |
| protocol records | the parser's output, shaped as legend-engine's protocol and written as its JSON |
| the door | the transform from protocol records to the parsed model, shared by text and JSON input |
| referents | a call's record: every full name it can mean, worked out once |
| form | a function whose result type its signature cannot express, typed by its own rule |
| special form | a native whose arguments are programs or contexts (`from`, `execute`, ...) |
| execution context | a `from`'s mapping, runtime, connections and flags, evaluated once as a value |
| lifted function | a function the normalizer made from a body, named with the `$` sigil |
| binding table | a mapping's canonical form: class and association bindings to function names |
| Knowledge, Work | the eager, total facts about what exists; the demand-driven production from them (`docs/TENETS.md`) |

Sources: `docs/frontend-architecture.md` §6 (git history: docs/HISTORY.md); this page.

## 11. Open gaps and divergences in the frontend

- **No positions after the parser** and identity carried as strings ([`README.md`](README.md) §6).
- **Name tiers** differ from legend-pure's one-set rule, and the core import group has 32 packages instead of 29
  (§4.2). No register row.
- **Overload choice** is not yet legend-pure's loop (§7.2).
- **Decimal precision on the type** and the type-name vocabulary of `pure/v1` type answers (§7.5). No register row.
- From the 2026-09-28 review, reported by a reader and not re-checked: import scopes keyed by element full name, first
  one wins, so overloads of one function declared in two sections all resolve with the first section's imports; span
  columns count UTF-16 units while error columns count code points, so they disagree after a character outside the
  basic plane.

Sources: `docs/plan-audit-2026-09-26/architecture-review-2026-09-28.md` §4 rows 13–14,
`docs/PLAN_AUDIT_2026_09_26.md` §1 (git history: docs/HISTORY.md); this page.
