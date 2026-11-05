# The protocol program: one typed hub for text and JSON (2026-10-05)

The user, 2026-10-05: "Yes let's do the protocol program the right way as experts." It replaces the Studio plan's B1
(`docs/STUDIO_FULL_PLAN_2026_10_04.md`) as the one program for everything that turns Pure text into protocol JSON
and back. Owner: `com.legend.protocol` in core. Branch `protocol` (on the Studio line's `studio-engine`); core work
announced in `docs/IN_FLIGHT.md` before each landing.

**Status (2026-11 docs cleanup):** legs 1, 2, 4 and 5 (leg 3 folded into 4) were built 2026-10-08 to 2026-10-10,
legs 6 and 7 on 2026-10-10, and leg 8, the JSON door, on 2026-10-11 and 2026-10-21; legs 6b, 9 and 10 were open when
§4's record was cut. Since 2026-10-21 the program has built on `studio-program`, not `protocol`. §1's "Today" table
is the state of 2026-10-05, before the legs. Kept: why (§1), the architecture (§2), the oracles (§3), the upstream
facts the legs found, and the guards (§5–6).

## 1. Why

Studio edits text; legend-sdlc, legend-depot and legend-engine speak protocol JSON (entities, PMCD). Opening a real
project means JSON to text; saving means text to JSON; and the two must agree exactly (S5: JSON to text to JSON gives
equal records). Today the layer that does this works three different ways:

| Leg | Today | Works on |
|---|---|---|
| parse: text to records | `PmcdParser`, `ElementParser`, `SpecParser` | typed records (`Protocol.*`, `protocol.spec.*`) |
| emit: records to JSON | `ProtocolEmitter` (+ `TailEmitter`, `MappingEmitter`, `ConnectionEmitters`, `GqlEmitter`) | typed records; byte parity with the engine over the corpus |
| read: JSON to records | `ProtocolReader` | typed, but **lambdas and value specifications only** |
| compose: JSON to text | `PureComposer` (values), `ModelComposer` + family composers (elements, B1) | **untyped JSON** (`Json.Obj`, fields by name) |

Half typed, half not, and no model reader. Every consumer picks its own path (the server's `grammarToJson` and
`jsonToGrammar` routes, the WebAssembly exports, the apps).

## 2. The architecture

One hub, the typed protocol records; four legs, each a pure function, each with its own exact oracle:

```
text ──parse──▶ records ──emit──▶ JSON
JSON ──read───▶ records ──compose─▶ text
```

**Invariants**

1. **Records are the only hub.** Every leg starts or ends at `Protocol.PureModelContextData` / `Protocol.Element` /
   `protocol.spec.ValueSpecification`. No consumer composes from JSON or emits from text; JSON is a wire format only.
2. **One printer.** `PureComposer` (values) and the element composers are one composer over records: one set of
   conventions (indentation, quoting, identifier rules, styles STANDARD/PRETTY), split into files by family only for size.
   Values inside elements print through the value composer, never a second way.
3. **One reader.** JSON to records for models and lambdas alike, through `ProtocolUpgrade` first (older wire forms
   brought current, as upstream's converters do). Numbers exact (a decimal's digits as written; the user chose exact
   decimals, `ComposerParityTest.EXACT_DECIMAL`). Unknown `_type`: refused by name -- never dropped, never guessed.
4. **Exactness over the whole corpus,** both against the engine (where the engine is the spec) and against ourselves
   (the round trips). An element a leg cannot handle is refused by name; plan S19: Studio then opens it read-only as
   JSON and preserves it byte for byte.
5. **Conversions in their layers, one contract, many transports** (revised 2026-10-09, the user; was "one public
   face", `com.legend.protocol.ProtocolText`, which the layering forbids: `protocol` may not call the parser,
   ArchitectureTest 7b, and the parser may not read JSON, 7c). Three kinds of thing, each with one home:
   - **The four conversions** -- a model's and a lambda's text to JSON, a model's and a lambda's JSON to text -- each
     one function taking its options, in the layer that owns its input, as legend-engine's parser takes
     `returnSourceInformation` and its composer the render style. Text to JSON is the parser's (source information
     on or off); JSON to text is protocol's (from the JSON's text, at protocol's depth limit, STANDARD or PRETTY).
     A failure is typed where it happens: a parse error with its position, a refusal naming what it refuses.
   - **legend-engine's `pure/v1` contract** -- its paths, query parameters, statuses, error body and request shapes --
     implemented once, by `PureV1Api`, over the conversions. Nothing else knows any of it.
   - **The transports** that reach the contract: lite's HTTP server, Python's engine (`//native:compiler`), and the
     tab. The apps' in-tab engine sends the same `pure/v1` requests as their remote one, so the tab and a server
     cannot answer differently, refusals included.
   - **One boundary for the embedded hosts, thin adapters** (the user, 2026-10-09). The tab and Python reach lite
     through one class, `planner.Boundary` (`//wasm:boundary`): plain Java, no host's annotations and no text
     encoding -- strings in, a string or an answer out, or an exception -- each operation once. legend-engine's
     operations it offers only as `pureV1(path, query, body)`; lite's own (planning a query written as text, test
     data's SQL, a Database from a catalog, a session's setup, warming a model, ...) by name. Each host has one
     adapter that is nothing but one-line delegations, and its list is that host's API: the tab's
     (`planner.TabExports`, TeaVM's entry class, each function `@JSExport`) and Python's (`native/`'s
     `nativelib.Compiler`, each `@CEntryPoint`). Both cross a strings-only line, so both use one text encoding,
     `planner.Folded` (`OK\n<answer>`, `ERR\n<class>\n<message>`, a `pure/v1` answer as `OK\n<status>\n<type>\n<body>`).
     The JVM differential compares the tab adapter's answers across the two builds. No adapter offers its own
     version of a `pure/v1` endpoint: Python's own grammar calls (`parse`, `print_tree`, `model_elements`) ask
     `pure/v1` too.
   In-process Java that is not a client of the contract -- the SDLC server, tools, tests -- calls the conversions
   directly, behind an interface of its own at its edge (leg 7), as legend-sdlc embeds the engine's grammar.
6. **Reference checkouts are spec and oracle only** (AGENTS.md): the engine's composer and serializer are compared
   against in `parser-equivalence`, never loaded by core.

**Source information.** Records carry `SourceInfo` (the parser fills it; the emitter writes it when asked). The reader
fills it from JSON when present; equality for the round trips is on records with source information stripped, as the
existing tests do (`SourceInformation.strip`), and the named spans (`classSourceInformation`, ...) with it. A span may
be absent (a model read without source information), and the emitter then omits its key, as the engine's `NON_NULL`
does; so the emitter cannot catch a parse site that forgot its span. Gate 8 does: the corpus sweep's claim 1a compares
the parser's emitted JSON, every span included, byte for byte with the engine's. The reader takes spans all or none
where a record keeps a part's position relative to its whole (a path literal's segments and arguments): a mix could
not be written back, so it is refused.

## 3. The oracles

| Leg | Oracle | Pin |
|---|---|---|
| emit | the engine's serializer on the engine's parse of the same text (exists: gate 8) | byte parity, up-only matched |
| read | `emit(read(J)) == J` byte for byte for every J the emitter produces over the corpus; and for the engine's own JSON of every corpus source (the engine parses, lite reads and emits) | up-only matched, zero mismatches |
| compose | the engine's `PureGrammarComposer` on the same JSON (exists: `ModelComposerParityTest`, `ComposerParityTest`), now fed `compose(read(J))` | the counts held exactly through the retarget, then up-only |
| round trip | `emit(parse(compose(read(J)))) == J` (sources stripped) over the corpus and the showcase projects | zero mismatches |

## Upstream facts found while building

What legend-engine 4.145.0 does with protocol JSON and text, found while the legs were built (§4, cut) and stated
nowhere above. The class names are the engine's; the oracles of §3 checked each. Lite's deliberate differences are
register rows (S29 to S39, S89, S94, S95 among them).

- **Unknown and older fields.** The engine reads deprecated protocol fields and has two protocol converters. Its
  protocol classes marked `@JsonIgnoreProperties(ignoreUnknown = true)` (Service, DataSpace, Diagram, the model
  context among them) drop an unknown field silently.
- **Older expression JSON, and what the engine makes of it.** `class`, `enum` and `mappingInstance` `{fullPath}` become an
  element pointer (`pp/v1/.../deprecated/Class.java`, `PackageableElementPtr.convert`); `primitiveType` reads `name`
  first (`PrimitiveType.java`) and `unitType` reads `unitType` first (`raw/UnitType.java`); `hackedClass`,
  `hackedUnit` and `genericTypeInstance` become a type annotation; `var {class}` reads the class as the variable's
  type (`Variable.java`); an upper bound of 2147483647 means many (`Multiplicity.java`). A `values` list of none is an
  empty collection, of one the literal, of more a collection (`PrimitiveValueSpecification.customParsePrimitive`), and
  is dropped on `strictTime` and `byteArray`. `path`, `rootGraphFetchTree` and `listInstance` are chosen by which
  fields are present (`ClassInstanceWrapper.java`). `qualifiedProperty` compiles as a property access
  (`ValueSpecificationBuilder` `processProperty`); `aggregateValue` and the `tds*` shapes compile to library calls
  (`core/pure/tds/tds.pure`, `corefunctions/collectionExtension.pure`); `runtimeInstance` and its kin cannot be printed
  (`DEPRECATED_PureGrammarComposerCore`).
- **More reading rules.** `fControl` on a call only logs a warning (`CompileContext.testFunction`); `class` on a
  property access is read by no compile step; a path literal's empty name is read as no name (`tds.pure`
  `buildColumnNameOutOfPath`); a `multiplicity` on a single value is discarded; a list left out of the JSON starts
  empty; older element layouts (supertypes, property and return types written as names, the model's `domain`,
  `mappings` and `stores` sections) are merged in the engine's order.
- **Defaults and write-back.** A service without `autoActivateUpdates` defaults to `true`; an older `legacyRuntime`
  becomes a runtime through `LegacyRuntime.toEngineRuntime`; a column's `nullable` is a primitive, false when left out;
  `isTestDataFromServiceOutput` is true when left out; a `null` setting is Java null, because Jackson never hands null
  to a value deserializer. The engine writes a relational operation's list items without `_type`, and cannot print
  them from its own JSON.
- **The routes.** `jsonToGrammar/model` prints in the request's `renderStyle`, PRETTY unless asked otherwise, and has
  no batch form. `compilation/compile` answers OK or the first error only. The compile, `lambdaReturnType`,
  `lambdaRelationType`, `generatePlan` and `execute` routes take a model as `PureModelContextData`
  (`{"_type":"data"}`) as well as text, as upstream Studio sends it. A section index's imports apply only to the
  elements its section lists.
- **SDLC entities** carry no section index, so a model read from entities has no imports; entity JSON carries fully
  qualified names and no comments; upstream's SDLC refuses imports at save, and legend-sdlc leaves compiling to the
  project's build.
- **Source spans.** Asked for JSON without source information, the engine still keeps spans in five places
  (`SourceInformation.withoutSpans`). A column spec that is a `let`'s value keeps its own span. A brace-less lambda's
  end depends on its statement's position: in a sequence's first statement it takes the `;` and the statements after
  it; in a later statement the `;` is the sequence's.
- **Smaller rules.** `1 + 2 + 3` compiles like `plus([1, 2, 3])`; a relation column written without a multiplicity is
  `[0..1]`; a connection's database need not be declared; legend-pure treats a condition naming `$return` as a
  post-condition.

> Cut in the 2026-11 docs cleanup: §4, the legs in order (1 to 10) with their as-built notes, 2026-10-05 to 2026-10-21, Java-specific; the upstream facts they found are listed above. Full text: `git show de2745351:docs/PROTOCOL_PROGRAM_2026_10_05.md`.

## 5. Guards and process

- Core's guard tests apply unchanged (file-size caps, no regex in the protocol package, the literal-name census, the
  layering file); no pin is raised -- a guard that flags is answered by restructuring.
- Gate 8 carries the parity and round-trip oracles; core's own tests carry the record-level round trips on
  representative models.
- Each leg lands on `protocol` with its oracle green and the local gate green; Bazel changes reviewed by the Bazel
  program's session before a push.

## 6. Parked behind this program

**User tests** (the Studio plan's A4): tests run in the tab and on a server must give the same verdict, so they get one
architecture end to end -- the pipeline (plan, provision, execute, serialize, judge), the owner of each step, execution
and serialization parity, where judging lives (the database owner's paused `//core:judge` question), the test runtime,
mapping/function/service suites and every assertion kind, and a both-ways parity proof -- designed after this program,
on the round trip's foundation. The provisional work is kept on `studio-tests-parked`.
