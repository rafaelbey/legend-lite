# Rust port — phase 2: the front end (2026-11-05)

Phase 2 of [`ROADMAP.md`](ROADMAP.md): the lexer (1,418 lines) and the parser (24,884) move to Rust, with the
protocol emitter, and their Java is deleted. This file holds what was measured before any of it moved, the design
that follows, and the user's rulings on it (§5, 2026-11-05).

## 1. What was measured (2026-11-05)

**The entry points and their callers.** Main code reaches the parser through a few doors:

| entry | returns | main callers |
|---|---|---|
| `PmcdParser.parse(text, Dialect)` | `Parsed(PureModelContextData, unclaimed sections)` | `door/ModelFromText` (so `Compiler.compileModel`, `parseSources`, the bootstrap: `Prelude`, `Pure`, `SystemMetamodel`) |
| `SpecParser.parse(text, Dialect)` | a `protocol.spec.ValueSpecification` | `Compiler.parseQuery` |
| `PmcdParser.parseDocument(text[, spans])`, `parseModel` | engine PMCD JSON / records | `PureV1Api` (grammarToJson), `ModelInput`, `ModelChanges`, sdlc-server `CoreGrammar`, the frozen wasm `Boundary` |
| `SpecParser.lambdaJson(text, spans)` | engine lambda JSON | `PureV1Api`, the frozen wasm `Boundary` |

`ElementParser`'s cursor API, `TokenStream` and `ElementKind` are used outside the parser by two callers only:
`spec/src/gen`'s `PreludeGenerator` (build time; it cuts each declaration's verbatim text, first token to where the
parser leaves the cursor, into a committed module) and `ide/ModelIndexer`/`ModelOrchestrator` (no main caller; the
LSP does not use them; `ide/package-info.java`: "currently unused by the batch pipeline").

**The engine JSON is not the seam.** The roadmap planned that Rust emit protocol JSON and `ModelReader` rebuild the
records (its "Rule A"). The JSON round trip is tested in the engine dialect only, and the product parses in lite's
dialect (the bootstrap in the platform's), where the emitter refuses constructs by name (native functions,
primitives, overlay elements, class and function type parameters, `^$x(...)`, SQLite connections), and the records
carry what the JSON drops: the quoted calls the parser folds (`QuotedTreeCall`, `QuotedGrammarCall`), `grouped` and
`infix` on applications, some nested spans (a let value's own, the milestoned `%latest` arguments'), the unclaimed
sections, the element offsets into the source. S89's Rule A is about text the engine accepts and drops, not a round
trip guarantee. So the seam is **a lossless form of lite's own records** (§3), not the engine's JSON.

**The Postgres-SQL translator is not front-end code.** `#SQL{...}#` stays raw text through parsing and JSON; it is
translated inside the typer (`Typer.java:213`, `SqlIslands`), which it calls back for each source's columns. Its core
(`SqlToPure`, 5,944 lines, `InputDialect`'s 61 hooks, `SqlAst`) is shared by the SAP IQ and Cypher translators, which
stay Java in iteration 1. Its tests: `PostgresToPureTest` (through the entry point), `PostgresParserTest` and
`PostgresDateTimeTest` (internals), and `sql_parity` over 2,362 cases ratcheted by status.

**Plug-in grammars.** `SectionGrammarRegistry` loads `com.legend.spi.SectionGrammar` overlays by `ServiceLoader` and
the parser calls them back for a `###` section it does not know. No main code registers one; one test does
(`ToySectionGrammar`, `SectionGrammarRegistryTest`). Unknown sections: the engine's dialect refuses them, lite's
refuses them unless registered, the platform's returns them unclaimed.

**Errors and positions.** The parser throws `ParseException(message "[line:col] ...", line, column)` -- and in eight
places `IllegalStateException`, and `IndexOutOfBoundsException` from `TokenStream`. The lexer never throws. Error
columns count code points (`TokenStream.columnOf`); span columns count UTF-16 units (`tokenPositions`); the mismatch
is pinned as a known defect (`Utf16ColumnKnownDefectTest`). Offsets are UTF-16 indexes, not bytes as `Token`'s
javadoc says. Records carry `SourceInfo` spans (1-based, end column inclusive); spec-node equality ignores `pos`.

**Object identity.** `Typer` keys an `IdentityHashMap<ValueSpecification, TypedSpec>` on the parser's own nodes
(`Typer.java:108`, `Overloads.java:361`). Whether the parser ever puts one node in two places is measured in §4 step 1:
a round trip that copies nodes would change what those maps see.

**The live gates.** The roadmap's "22,725 elements" comes from a deleted 2026-08-06 status document; no test computes
it. The gates that hold the front end today: `CorpusSweepTest` (lite's PMCD JSON byte-identical to legend-engine's,
at least 6,471 documents, zero diffs, zero refusals), `RoundTripProofTest`, `JsonModelDoorTest`,
`ModelReaderParityTest` (37,739 own elements, 38,702 engine, 13,514 documents, 205 lambdas, none mismatched),
`ModelComposerParityTest` (31,452 elements), all in `//parser-equivalence`; and the whole test suite, which parses
through these doors.

## 2. What of the front end stays Java

| part | in phase 2 |
|---|---|
| `PmcdParser.parse`, `SpecParser.parse`, `parseLambda`, `parseCodeBlock`, `parseDocument`, `parseModel`, `lambdaJson` | **stay** as the shim: one Rust call each, through an interface in `com.legend.parser` itself that `ServiceLoader` resolves to a provider in `com.legend.ffi` -- as `RustDialects` lives in `sql.dialect` (ArchUnit's invariant 7c forbids the parser `ffi`; `spi` holds string-only types and stays free of `parser` and `protocol`) |
| `ParseException`, `Dialect`, `PmcdParser.Parsed`/`UnclaimedSection` | **stay** (vocabulary the callers use) |
| `Protocol` and `protocol.spec` records | **stay** until phase 3: the shim builds them from Rust's answer |
| `SectionGrammar` SPI, `SectionGrammarRegistry`'s overlay half, `OverlayElementSink` | **stay**: Java calls an overlay grammar on the section text Rust hands back |
| `ModelReader`, the `*Reader`s, `ModelComposer`/`PureComposer` | **stay**: they take JSON or records, downstream of the seam |
| `ProtocolEmitter` and its helpers | **move** (step 5): Rust writes the engine JSON at the two text doors, `parseDocument` and `lambdaJson`. **Stay until phase 3** (corrected 2026-11-06, step 5: not deleted with the parser), for their record-sourced callers: `PmcdParser.DocElement.json()`, `PureV1Api`'s connection export, the `*Reader` round trips and their tests (`ModelReaderParityTest` emits `ModelReader.read(json)`), `ProtocolEmitterTest`, `PureComposer`'s `parseMultArg`, `TailEmitter.PERSISTENCE_TYPES`; and the front oracle, which asks Java's emitter over Java's records (`FrontOracle.ask`), so `engine.tsv` stays Java's answer |
| `Lexer`, `TokenStream`, `ElementParser`, `SpecParser`'s body, `MappingProtocolParser`, `DatabaseProtocolParser`, `GqlParser`, `QuotedSpecParser`, the section grammars, `TokenStreamCursor`, `IslandScan`, `EngineQuirks`, ... | move, then deleted |
| `PreludeGenerator`'s cursor use | retargeted to the element spans Rust answers; held by regenerating the committed module byte for byte |
| `//parser-equivalence`'s tests of parser internals (`RejectionParityTest` and its pinned `MIN_PINS`, `GenerativeDualParseTest`, `Sectionize`, `SectionParseSentinelTest`, `OwnDialectCensusTest`, `SurfaceCensusTest`, `GrammarCoverageCensusTest`, `Surfaces`, `ViewFilterParityTest`, `OffsetCompositionParityTest`) | **gates, not retirees**: re-expressed over Rust ops (section splitting, the surface census) before step 6, so `:parser_parity` keeps judging the parser it was built for |
| `ide/ModelIndexer`, `ModelOrchestrator` | deleted with the lexer (§5, ruling 3), their tests ledgered |
| the Postgres-SQL translator (`protocol/sql`, not `iq/`) | stays Java: it moves in phase 3 (§5, ruling 2) |

## 3. The seam

**A lossless record codec.** A Java codec over the `Protocol` and `protocol.spec` records, by reflection: a record
is its type tag and its components in canonical-constructor order, nulls explicit, over a fixed set of leaves
(`String`, `int`/`long`, `boolean`, enums, `BigDecimal` as text, `PureDateLiteral`, `SourceInfo`, lists, maps). It
writes a **schema** too -- every record, its sealed parent, its ordered components and their types -- which generates
the Rust types and their writer and checks the Java reader, and is the completeness gate: Rust covers every record the
schema lists. The codec is built once for three uses: the oracle's dump (§4 step 1), the shim's reader (Rust's answer
to records), and the differential. If the parser shares nodes (§1, object identity), the codec carries reference ids.
Whether the shim's reader stays reflective or is generated is decided at the switch, by what native-image metadata for
the records costs libcompiler.

**The FFI shape**, in Phase 1's form: one export, `lite_front(op, json) -> folded answer`. The ops are the entry
points; the arguments carry the dialect, the span switch and the registered overlay section names. The answer is the
records in the codec's form, plus what the records do not hold: the unclaimed sections, the overlay sections' raw text
and place (Java calls the grammar and splices its `POpaqueElement` in order), and the elements' offsets into the
source. An error answers the Java class to throw (`ParseException`, `IllegalStateException`, ...), its message and its
line and column, and the shim rethrows exactly that.

**The model door's overlays** (settled in step 4a, 2026-11-05). `lite_front::doors::pmcd(text, dialect, overlays)`
takes the shim's registered overlay grammars (`OverlayGrammar { name, lexable }`: `SectionGrammarRegistry`'s
ServiceLoader half, names and `lexable()`) and answers a `PmcdAnswer { parsed, overlays }`. `parsed` is the records or
Java's error; `overlays` lists, in section order, each overlay section the walk met (`OverlaySection { name,
start_offset, end_offset, start_line, element_index, section_index }`): the grammar's `SectionSource` (the section's
raw text `text[start_offset, end_offset)`, the line it starts on) and where Java would have spliced its records --
before `model.elements[element_index]` of the answer as Rust gives it (splicing the sections last-first keeps each
index right), and their paths, each once, into the section index's `sections[section_index].elements`, which Rust
leaves empty (the entry is otherwise whole: name, span, `importAware`, no imports). Rust never makes a
`POpaqueElement` (its one producer is the overlay seam, `ArchitectureTest.opaqueCarrierIsLockedToTheOverlaySeam`):
the shim calls each grammar and splices its elements as `OverlayElementSink` does. On an error the list holds the
overlay sections before it, whose grammars Java would have called first: the shim calls them (only their name, text
range and line are meaningful then; nothing is spliced), and an exception of theirs wins over Rust's. The differential
plays the shim's part
for the oracle's one registered overlay, `ToySectionGrammar` (`tests/front_parser_differential.rs`, `toy_splice`).

**Java is the spec.** The span and error columns are reproduced as Java computes them (UTF-16 units and code points),
the `[L:C]` prefix, the eight `IllegalStateException`s, the four failures `JsonModelDoorTest` pins. A difference found
is a row of `rust/lite-front/PARITY.md`, as in phase 1.

**`lite-front` works in UTF-16 units** (decided in step 1). Java's lexer and parser index a `String`'s `char`s:
offsets, `charAt`, `substring`, the span columns, and the lone surrogates `parser/Escapes` makes from `\uXXXX`
escapes, which a Rust `str` and a UTF-8 C string cannot hold. Rust's front end reads its text as `&[u16]`, so all of
those are Java's by construction; UTF-8 is only at the edges, and every surrogate crosses them as a JSON `\uXXXX`
escape (the codec's rule).

**The differential** (decided in step 1). Java's answers over the question set are 3.6 GB of codec JSON (77 MB
compressed for the recorded questions alone): too large to commit. While the Java parser exists the gate is live: a
cached Bazel action runs Java's lexer and doors over the committed questions and writes one SHA-256 per token stream
and per answer; Rust's test computes its own, compares, and holds a floor of matches that only rises, as phase 1's
dialect floors did; a debug tool prints both answers of a question that differs. The hashes are frozen as committed
data just before the Java parser is deleted (step 6), and are the gate after it.

**Traps the dump alone will not show, each settled before its step:**

- **Lone surrogates** -- settled above: UTF-16 inside, `\uXXXX` at the edges.
- **The codec's leaves are found, not assumed.** The schema lists every component type the records use; an unknown
  one throws (invariant 4), never `toString`. Candidates beyond §3's: `double` (spelled by `lite-javacompat`'s
  `Double.toString`), raw JSON (`POpaqueElement`'s protocol JSON), `Optional`.
- **The bootstrap parses at class load.** Once the model parser switches (step 4), `builtin/Pure`'s static
  initialiser parses the platform's prelude through the shim, so a missing library dies as a null-message
  `ExceptionInInitializerError` (a phase 1 follow-up): fixed before step 4, and phase 1's wiring (`//core:ffi` carried
  by every target that reaches core) checked for the targets that reach `builtin` without rendering SQL.
- **Section grammars that hop through JSON** -- settled 2026-11-05, no hop. The Connection, Runtime,
  FunctionActivator, Diagram and Elementwise grammars call `ProtocolEmitter` only in their SPI `parse(SectionSource,
  ElementSink)` form, the overlay path; `PmcdParser` takes every built-in grammar's records directly
  (`parsed.elements()`, PmcdParser:293-323, 758-787), and only an overlay goes through a sink (`OverlayElementSink`,
  :304). The model path has no JSON normalisation to reproduce.
- **The class-load trap** -- settled 2026-11-05 (`812ede680`): a missing Rust library is an `IllegalStateException`
  with its own message at every call of `Lite.shared()`, not an initialiser's message-less error
  (`//core:ffi_missing_test`).
- **Recursion depth.** The expression parser backtracks by exception (`SpecParser:2541`) and recurses through nested
  lambdas; Rust's parse runs on a big-stack thread or behind a depth limit, as phase 1's dialect calls do (TRIAGE A3).
- **Capturing the dump.** Sandboxed tests cannot append to one shared file: each process writes under
  `TEST_UNDECLARED_OUTPUTS_DIR`, merged afterwards, deduplicated by the SHA of `(entry, dialect, spans, text)`, each
  text stored once.

## 4. Order

1. **The oracle and the receipts (2a).** The codec and its schema. Behind a system property, every Java entry point
   appends `(entry, dialect, spans, text, records or error)` during one `bazel test //...`, deduplicated; the
   `//parser-equivalence` corpus is added at all three dialects. Measured before deciding what is committed: the
   dump's size, the shared nodes, the record types reached against the schema. A pre-deletion receipt of the parse
   time (p50 over the stress corpus, the bootstrap's parse at class load), as phase 1's §4 step 1.

   **Done 2026-11-05.** The parse tap (`com.legend.parser.ParseTap`: the door, the dialect, the text, when
   `LEGEND_PARSE_TAP` is set) ran over `bazel test //...` -- 517 pass, the same 2 skipped, with
   `ObservabilityGuardrailTest`'s flag list moved for it (dated) -- and recorded 356,916 calls in 141 processes:
   71,200 distinct questions over 52,143 texts, all three dialects and all four doors. With every model text asked
   at every dialect, 110,596 questions: the committed question set, `rust/oracles/front/questions.jsonl.zst` (8.4 MB
   within zstd's default window; `front/MANIFEST.tsv`). `//core:front_oracle` asks each of Java again and writes its
   answer in the lossless codec (`com.legend.frontoracle.RecordCodec`, test code by reflection -- the product bans
   reflection, so the shim's reader is generated from the schema, never reflective). What it measured:
   - **Errors**: 13,167 questions raise -- 13,163 `ParseException`s, three `IllegalArgumentException`s (one model
     text, at each dialect) and one `DateTimeException` (a query) -- classes the shim rethrows as they are. The
     emitter refuses one engine-dialect model the parser accepts (`UnsupportedOperationException`): its own field of
     the answer, step 5's. Two independent runs (the generator, the timing tool's passes) raised on exactly the same
     questions: the parser answers as a function of its text.
   - **The receipt** (`//core:front_timing`, the question set on one warm JVM, the last of 3 passes, load average
     2.85, the Docker VM's containers running): 3,742 ms for all 110,596 questions. Models, per dialect: p50
     0.008-0.012 ms, p95 0.13-0.14 ms, p99 0.35-0.37 ms, about 1.2 s each in all; queries (21,838 at lite's dialect):
     p50 0.001 ms, 50 ms in all. `builtin.Pure`'s first touch (the prelude's parse and build): 70 ms. The shim is
     measured by the same tool after its switch.
   - **Coverage**: the schema (`front/schema.json`) holds 395 records under 39 sealed parents; the answers reach 390.
     `PAwsDefault` is the Connection grammar's (`ConnectionSectionGrammar:2070`) but no test reaches it: a question is
     written for it before that grammar's step. `PCNull`, `POrigin`, `PSdlc` and `PSerializer` are the JSON readers',
     never the parser's. The overlay record (`POpaqueElement`) needs the toy grammar registered, as its test has it.
   - **Leaves**: `boolean`, `int`, `long`, `double`, their boxes, `Number`, `String`, `BigDecimal`, one enum
     (`TypeExpression.Op`), lists, maps and map entries; two `Object` slots (`PEnumSourceValue.value`,
     `PRelLiteral.value`), tagged by their runtime class. The maps are `LinkedHashMap`s (insertion order): no hash
     order to reproduce.
   - **Shared nodes**: only `Multiplicity.Concrete` and `SourceInfo` values recur within an answer, never a
     `ValueSpecification` -- so the typer's identity maps see the same nodes after a round trip, and the codec needs
     no reference ids.
   - **Size**: 3.6 GB of answers, which decided the differential above.
2. **`lite-front`'s lexer (2b)**: the tokens, offsets and columns as Java's, by a differential over the dump's texts.

   **Done 2026-11-05.** `rust/lite-front` (`lexer.rs`: `TokenType` in Java's order, the normal and island scans, the
   section headers with their residue, the skipped opaque sections, the line index and the token spans in UTF-16
   units, `column_of` in code points, `lex_range`; `wire.rs`, JSON whose strings are UTF-16 units; `sha256.rs`).
   `//core:front_tokens` (a cached `java_run` of `FrontTokens`) writes the SHA-256 of each text's Java token stream in
   one canonical form; `//rust/lite-front:front_lexer_differential_test` (the core lane) lexes the same texts and
   compares: all 52,143 texts, 18,529,523 tokens, identical on the first run, and every text's id recomputed from its
   units. The error column (`column_of`) and the range scan are pinned by unit tests here; the parser's errors and
   islands hold them to Java from step 3. The Java lexer's own tests stay until step 6.
3. **The expression parser (2c)**: `SpecParser` in Rust, byte parity on the dump's spec entries; then
   `SpecParser.parse`, `parseLambda` and `parseCodeBlock` **switch** -- every query the test suite runs is parsed by
   Rust while models are still parsed by Java. The quoted tree (`compileLegendValueSpecification`) folds in Rust,
   inside the parse, where the let-bound constants are in scope (`SpecParser:1309`, `this::constString`); the quoted
   grammar (`compileLegendGrammar`) needs a model parse, so until step 4 Rust resolves its payload text in scope and
   Java folds it with `PmcdParser`. `lambdaJson` goes through Java's `ProtocolEmitter` until step 5.

   **Done 2026-11-05.** The expression parser in Rust (`spec_parser.rs`, `cursor.rs`, `operator_parts.rs`, `gql.rs`,
   `temporal.rs`, `escapes.rs`, `jnum.rs`, `nodes.rs`; `fefb4d224`): all 23,032 questions of the three doors identical
   to Java's -- spec 22,151, lambda 876, block 5, the 57 errors among them -- and 949 more on paths the oracle does not
   reach, held to `//core:front_show`; twelve Java behaviours reproduced as they are (`rust/lite-front/PARITY.md`). The
   quoted grammar did not wait for Java: Rust refused it until the model parser came (step 4a folds it). The seam: the
   records in Rust and their writer generated (`records.rs`, `//core:front_records_rs`), the shim's reader generated
   (`FrontRecords`, `//core:update_front_records`; lossless on every Java answer, `//core:front_reader_test`), the
   `lite_front` export, `RustParsers` and its provider (`:door` carries it at run time). **The switch** (`f6e9d36bb`):
   `SpecParser.parse(String, Dialect)`, `parseLambda` and `parseCodeBlock(String, Dialect)` answer from Rust; the Java
   bodies stay as `parseInJava`, `parseLambdaInJava`, `parseCodeBlockInJava`, the oracle's, until step 6. The whole
   suite at `f6e9d36bb` (its own worktree): every `rust/BASELINE.tsv` target at its baseline status but the one retired
   docs diff test, the 17 new targets passing, `bazel build //...` clean. Guard moves: `rust/RETIRED_TESTS.md`.
   **The shim's cost** (`//core:front_timing`, the same set, the last of 3 passes, load average 4.94): a query at lite's
   dialect p50 0.036 ms through the shim against Java's 0.001 ms (1,144 ms for the 21,838 against 50), a lambda 0.055
   against 0.002 -- about 35 µs a call, most of it a 512 MiB-stack thread spawned per call and the JSON both ways. Small
   beside a query's compile. **The worker** (`5e5d7e118`, 2026-11-05): each calling thread now keeps one long-lived
   big-stack worker in `lite-ffi` (made on its first call, ended with it, a panic caught there so it outlives it), the
   dialect calls on it too. The same tool, load average 3.65: a query at lite's dialect p50 0.018 ms (752 ms for the
   21,838), a lambda 0.033 -- half the cost; what remains is the hand-off to the worker and back and the JSON both
   ways. The trade: what a deep call commits of a worker's stack stays committed while the worker lives.
4. **The model parser (2d)**: `PmcdParser` and its section grammars in Rust, section by section to parity; then
   `PmcdParser.parse` **switches** (the bootstrap included), with overlays called back in Java, and the quoted
   grammar folds in Rust.

   **4a done 2026-11-05.** `rust/lite-front`'s `model_parser.rs` (the section walk: headers and their residue, the
   section index and its buffer-coordinate spans, imports, the dialects' routing of an unknown section, unclaimed
   sections, the overlay sections above), `element_parser.rs` (`###Pure`: Class, Enum, Profile, Association,
   Measure, function and its test block, `native`, `Primitive`), `sections.rs` (`###Text`, the shared section
   helpers), `mapping_parser.rs` (`###Data` and the embedded values, ServiceStore stubs), and the quoted grammar's
   fold. The model door: 49,039 of its 87,564 questions identical, every one whose walk stays in those grammars
   (`rust/lite-front/PARITY.md`, rows 13-23); the 38,525 others reach a grammar not ported yet (Relational, Mapping,
   Service, Connection, Persistence, ... -- each one arm of `parse_one_at` or `section_elements`) and answer
   `not ported` there. The differential test is `size = "large"`: the model door parses and hashes in about 170 s
   unoptimized.

   **The shim's model door** (2026-11-05, ready, not switched). `lite_front_model(dialect, overlays, text)` in
   `lite-ffi` takes the registered overlays as `[["Toy",false],...]` (`SectionGrammarRegistry.overlays()`, the
   ServiceLoader half in registration order). It answers `A\n<sections>\n<records>` or `E\n<sections>\n<error>`, each
   section `[name, start, end, line, element, section]`. `RustParsers.model` and its provider read that answer.
   `PmcdParser.parseInRust` calls each overlay grammar in section order, lets one's exception win over the parser's
   error, and splices last-first through `OverlayElementSink`. At the switch it becomes `parse`. `//core:front_shim`
   (`FrontAnswers --shim`) asks the oracle's every question through the shim and hashes the answers as
   `//core:front_answers` does. Run with 4b's Relational in progress, it gave 77,033 of 110,596 identical: every one
   of the three expression doors' 23,032 (step 3's switch, checked from the Java side) and 54,001 at the model door.
   Those include the toy-overlay text at all three dialects, spliced at lite's and the platform's.

   **4b done 2026-11-05.** `rust/lite-front`'s `database_parser.rs` (`###Relational`: `DatabaseProtocolParser` whole
   -- the Database, its includes, schemas, tables, column types, milestoning, tabular functions, views, joins and
   filters, and the relational operation grammar the Mapping grammar borrows), `mapping_grammar.rs` (`###Mapping`:
   includes and their store substitutions, every class mapping kind -- relational, Pure, clean-sheet,
   AggregationAware, Operation and merge, Relation, ServiceStore with `ServiceLegacyMappingParser`'s tail, MongoDB --,
   the association and enumeration mappings) and `mapping_tests.rs` (test suites and the legacy MappingTests), on the
   cursors 4a built; `cursor.rs` gains `TokenStreamCursor`'s shared store readers. The model door: 65,486 of its
   87,564 questions identical (`rust/lite-front/PARITY.md`, rows 24-32); each of the 22,078 others reaches a grammar
   not ported yet (Connection 8,357, DataSpace 5,919, Service 2,423, Persistence 1,041, Runtime 899, ...). Beyond the
   oracle, the two grammars were held to Java by `//core:front_show` on 2,928 texts with their unported sections
   removed, 131,689 mutated texts and 50 probes (8,784 + 395,067 + 150 questions, not committed): all identical but
   mutations that made a section not ported and one Java nondeterminism -- where Java raises an
   `ArrayIndexOutOfBoundsException` reading past the token positions (row 29), a warm JVM's HotSpot fast throw drops
   its message; Rust answers the cold JVM's message, which no oracle question reaches. No mutation made Rust panic.

   **4c done 2026-11-05.** `rust/lite-front`'s `connection_parser.rs` (`###Connection`: `ConnectionSectionGrammar`
   whole -- the element envelope and the value body a runtime's embedded island shares, every flavor, the sixteen
   datasource specifications, the eleven auth strategies, the post-processors, the MongoDB, Elasticsearch and Deephaven
   authentication islands and their vault secrets), `runtime_parser.rs` (`###Runtime`: Runtime and
   SingleConnectionRuntime, and the embedded runtime body a service carries), `service_parser.rs` (`###Service`:
   Service and ExecutionEnvironment, the executions, the legacy test block with its signed-literal fold, the keyed and
   compact test suites, post-validations; the platform's headerless Service) and `dataspace_parser.rs`
   (`###DataSpace`), on 4a's cursors and 4b's Data and Mapping readers (the suites' data and assertions,
   `RelationIslands.parseStandaloneRelationAt`). Three Java pieces came with them: the re-lexed island's cursor
   (`cursor::IslandCursor`, `SpanOrigin.islandShift` on its spans and its errors left in the island's coordinates,
   row 33), `java.net.URI`'s verdict (`jnet.rs`, JDK 25's `URI.Parser`, the Elasticsearch `clusterDetails` check) and a
   `HashMap`'s key order (`jtypes::hash_map_order`, which TRIAGE flagged at `ConnectionSectionGrammar`'s mapper keys:
   only the first unknown key a mapper reports depends on it, row 34). The SPI feed (`parse(SectionSource,
   ElementSink)`, `SliceCursor`) is the overlay host's and is not ported. The model door: 82,682 of its 87,564
   questions identical (`rust/lite-front/PARITY.md`, rows 33-44; the floor raised from 65,486); each of the 4,882 others
   reaches a grammar step 4d ports and answers `not ported` there (by the first such grammar: Persistence 1,302,
   DataQualityValidation 708, ExternalFormat 646, ServiceStore 504, Diagram 447, QueryPostProcessor 243, Snowflake 204,
   Elasticsearch 144, MongoDB 135, FileGeneration 120, HostedService 93, Deephaven 90, MemSql 69, FunctionJar 69,
   GenerationSpecification 57, BigQuery 51). Beyond the oracle, the four grammars were held to Java by
   `//core:front_show` on 323 texts with their unported sections removed, 171,348 mutated texts and 425 probes (969 +
   514,044 + 1,275 questions, not committed): all identical. No mutation made Rust panic. The probes reach `PAwsDefault`,
   step 1's one Connection record no oracle question reaches (a GCP WIF island's `awsCredentials: Default {}`, at every
   dialect, identical). One Java answer that is not a function of its text sits here too: a service's embedded runtime island open at the stream's end is an
   `ArrayIndexOutOfBoundsException` (row 35), whose message a warm JVM may drop as row 29's; Rust answers the cold JVM's.
   `bazel test //rust/lite-front/...`: all four pass; the differential (`size = "large"`, fastbuild) in 201 s against
   4a's 170 s, far from the large size's 900 s.

   **4d done 2026-11-06.** The last section grammars in `rust/lite-front`: the function activators
   (`activator_parser.rs`: `FunctionActivatorSectionGrammar` for ###Snowflake, ###MemSql, ###BigQuery, ###HostedService
   and ###FunctionJar -- their deployment configurations, dropped or listed with a null path as the walk has them, and
   HostedService's ExecutionEnvironment through the Service grammar), ###Diagram (`diagram_parser.rs`: the raw section
   the lexer skips, walked by characters in UTF-16 columns, its imports the raw grammar's) and the tail grammars
   (`generation_parser.rs`: GenerationSpecification and FileGeneration; `deephaven_parser.rs`, its app the activator
   grammar's; `mongodb_parser.rs`, the jsonSchema island walked into Java's JSON values and read back by Java's casts;
   `data_quality_parser.rs`; `elasticsearch_parser.rs`; `external_format_parser.rs`; `service_store_parser.rs`;
   `persistence_parser.rs`, the generic node tree sorted by key as its record sorts it, islands re-lexed with their
   offsets composed, the walker's cardinality tables and wire spans; `query_post_processor_parser.rs`). `not_ported`
   and `TailGrammar::NotPorted` are gone: every section the walk reaches has its grammar. Two JDK readers came along,
   each as JDK 25 words it: `Double.parseDouble` (`jnum`, `FloatingDecimal.readJavaFormatString` ported step by step:
   `multiple points`, the hexadecimal form) and `String.substring`'s refusal (`ustr`, `Range [b, e) out of bounds for
   length n`, where the crate had JDK 17's wording; no earlier answer had reached it). The model door: all 87,564 of
   its questions identical (`rust/lite-front/PARITY.md`, rows 45-62; the floor raised from 82,682); the expression
   doors' 23,032 still all identical -- the oracle's 110,596 questions all answered as Java answers them. Beyond the
   oracle, the new grammars were held to Java by `//core:front_show` on 1,569 derived texts (each 4d section, and each
   of its elements, alone), 93,997 texts mutated after a 4d section's header, 110,595 mutated anywhere from 4,000 model
   texts sampled across every grammar, and 463 probes aimed by an `llvm-cov` line census (4,707 + 281,991 + 331,785 +
   1,389 questions, not committed): all identical but 66 questions whose answer is an `ArrayIndexOutOfBoundsException`
   (a persistence island open at the stream's end, row 55, and row 29's legacy test), which the bulk run's warm JVM
   answered with no message and a cold JVM answers with Rust's message byte for byte -- Java's one answer here that is
   not a function of its text. 4b's and 4c's sets (920,289 questions), whose mutations had made sections not ported,
   are all identical too but 313 of the same exception, each a cold JVM's answer. No mutation made Rust panic.
   `bazel test //rust/lite-front/...`: all four pass; the differential in 226 s, far from the large size's 900 s.

   **The switch** (`62ce37d0f`, 2026-11-06). `PmcdParser.parse` answers from Rust, the platform's bootstrap included
   (`builtin.Pure`'s prelude): the shim's model door above became `parse`, `parseInRust` is gone, and the Java body
   stays as `parseInJava`, the oracle's, until step 6. Three calls inside the Java parser now name the Java twins
   (`QuotedSpecParser`'s model and expression parses, `SpecParser`'s quoted-argument parse), so a Java answer is Java's
   all the way down; the oracle asks `parseInJava` for Java's answer and `parse` for the shim's. Rebuilt after the
   switch, `//core:front_answers` and `//core:front_engine_answers` wrote the same bytes as before it: no Java answer
   had come from Rust. The shim answers all 110,596 questions as Java does. The whole suite at `62ce37d0f` against
   `rust/BASELINE.tsv` moved three targets:
   - `//spec:gen_claims`: the claims generator runs on `//core:core_next`, which carried neither the provider's
     services file nor the library. `a263e4f5f` gives it both, the library inside its jar as `:ffi` carries it,
     since a build action has no runfiles.
   - `//core:census`: `FrontEngineAnswers` is now named in `ParserBoundaryArchTest.DIALECT_CLASSES` (`a263e4f5f`).
   - `//core:corpus_differential_test`: out of memory in the shim's reader, fixed by the next paragraph.

   **The reader streams** (2026-11-06). Once the model door switched, `//core:corpus_differential_test` (the 13 MB
   stress corpus in 768 MB) died of `OutOfMemoryError` under `FrontReader.pmcd`: the shim parsed Rust's whole answer --
   about 8 times its text's length, up to 16 -- into a `com.legend.json.Json` tree before the generated readers walked
   it into records. The tree is gone: `FrontRead.Cursor` is a position into the answer's `String`, and the generated
   readers (`FrontRecords`, FrontCodegen's java mode, `//core:update_front_records`) pull each record's components in
   the order the writers write them -- canonical order, the same in both: records.rs's 394 writers against the reader's
   1,642 components, checked key by key, and RecordCodec iterating `getRecordComponents()` -- the `"@"` tag first,
   exactly where the slot is a sealed parent's. A key missing, out of order or extra, whitespace, or text after the
   answer is refused, each refusal still an `IllegalArgumentException` naming its `Record.component` path. The leaves
   read as before (strings as `Json.parse` read the codec's, lone surrogates kept; an `int` past its range still
   `Math.toIntExact`'s `ArithmeticException`), and the depth limit is the tree's, 1 << 16 containers: a collection
   nested 32,768 deep is read and one more refused, by both readers. The answer's text is unchanged. The corpus
   differential passes at its 768 MB: live peak 324 MB, peak 558 MB before a collection, against 460 and 722 MB with
   Java's parser before the switch. The reader's own cost on the oracle's four largest model answers (69, 49, 23 and
   20 million characters, `LEGEND_LITE`; each reader in its own warm JVM, the median of passes 4-8, load average
   3.7): 46, 27, 18 and 14 ms, allocating 84, 61, 31 and 26 MB -- the records themselves -- against the tree reader's
   332, 237, 114 and 95 ms and 1,348, 978, 434 and 360 MB. `//core:front_reader_test` reads every Java answer back to
   itself; the shim answers the oracle's 110,596 questions all as Java does. Guard move: `rust/RETIRED_TESTS.md`.

   **The receipt after the switch** (2026-11-06). Taken with `//core:front_timing` over the same set, the last of
   three passes. Its `--java` flag times the Java parser's twins, so the two parsers were timed back to back, twice,
   at load averages 2.5-4.7.
   - **Through the shim, the Rust library optimized** (`-Copt-level=3` on the command line), against Java. No build
     set it then: Bazel's default build is fastbuild, so the tests, CI's kept wheel, the native library and the server
     jar all carried the opt-level-0 library. The user ruled (2026-11-06): every build optimizes the Rust crates,
     fastbuild included (MODULE.bazel's `rust.toolchain` `opt_level`); the parser differential then ran in 34 s
     against 226 s.

     | | Through the shim | Java parser |
     |---|---|---|
     | Model text, per dialect: p50 | 0.031-0.038 ms | 0.008-0.012 ms |
     | p95 | 0.41-0.45 ms | 0.13-0.14 ms |
     | p99 | 1.0-1.3 ms | 0.34-0.38 ms |
     | in all | 3.9-4.3 s | 1.2-1.3 s |
     | Query at lite's dialect: p50 | 0.010 ms | 0.001 ms |
     | Whole set | 12.6-12.9 s | 3.7-3.9 s |

     At fastbuild's rustc opt-level 0 (what `bazel test` ran before the ruling) the model door took 13.5-15.0 s a
     dialect, 44 s in all. The same questions raise in each.
   - **The Rust parser itself is not the difference.** Timed natively (release, no FFI, no JVM, load average 4.8-6.1;
     a scratch harness over `lite_front::doors`, the last of three passes):
     - a model text at each dialect: 1.35-1.39 s in all, p50 0.008-0.010 ms, max 137-140 ms (Java's 118-137);
     - the 21,838 queries: 35 ms (Java's 51).
   - **The rest is the seam:**
     - writing the codec, 0.76-1.0 GB of JSON a dialect: 0.7-0.9 s;
     - reading the text's JSON string: 0.12 s;
     - the crossing and Java's reader: about 1.7-1.8 s, by difference (the two harnesses ran at different loads).

     That is the cost of the records crossing to Java, and it goes when phase 3 keeps them in Rust.
   - **`builtin.Pure`'s first touch: about 155 ms in all, against 70 ms with the Java parser** (step 1's receipt,
     another day; it cannot be taken again, since the initialiser parses through `parse` in both of the tool's
     modes). Measured in cold JVMs with the optimized library at a fixed path, as Bazel's runfiles give it:
     - the library's load: 34 ms;
     - `FrontRecords`' class initialisation: 22 ms;
     - the first call's worker: 22 ms;
     - the prelude's parse and build: 79 ms.

     The timing tool itself, under `bazel run`, printed 205-212 ms optimized and 235-586 ms at fastbuild. A
     self-contained jar adds about 300 ms on macOS: it writes its library out to a new temporary file in every
     process, and macOS checks a new library at its first load.
5. **The engine JSON (2e)**: `ProtocolEmitter` in Rust; `parseDocument` and `lambdaJson` switch, gated byte for byte
   by `CorpusSweepTest`'s 6,471 documents and `ModelReaderParityTest`. Their parse is Rust's already (steps 3 and 4;
   `parseModel` is `parse`'s model and switched with it): what moves is the emitter.

   **The oracle** (2026-11-05). `//core:front_engine_answers` (`FrontEngineAnswers`) asks Java's parser and
   emitter every oracle question at the two doors whose answer is the engine's JSON: `document`, the 29,188 model
   texts at the engine's dialect (`PmcdParser.parseDocument`), and `lambdaJson`, the 876 lambdas. It hashes each
   answer's form twice, with its source information and without it (`SourceInformation.withoutSpans`), in
   `FrontAnswers`' canonical form; a parse error or an emitter refusal is the answer. The counts:
   - `document`: 23,257 JSON answers, 5,931 errors;
   - `lambdaJson`: 870 JSON answers, 6 errors.

   The file is deterministic: a second run in a fresh JVM wrote the same bytes.

   **5 done 2026-11-06.** `rust/lite-front`'s protocol emitter, from core/'s `com.legend.protocol`: `emitter.rs`
   (`ProtocolEmitter`), `tail_emitter.rs` (`TailEmitter`), `mapping_emitter.rs` (`MappingEmitter`),
   `connection_emitters.rs` (`ConnectionEmitters`, `AuthSpecEmitter`), `gql_emitter.rs` (`GqlEmitter`) and
   `source_information.rs` (`SourceInformation.withoutSpans`, ported as Java does it: a cut over the text that keeps
   the spans the engine keeps, never a parse and a rewrite, so the `_type` the engine writes twice stays twice). Rust
   parses and Rust emits, in one call, from lite-front's own records. The output is a Java string's UTF-16 units:
   `str` writes what `Json.escapeTo(b, v, true)` writes, every unit but `"`, `\` and the controls raw, a lone
   surrogate included. Every refusal is Java's -- class, message, Java's order, the first one Java meets --, and
   `protocol.rs`'s `mangled_name` now carries its message. The two doors, `doors::document` (`parseDocument`) and
   `doors::lambda_json` (`lambdaJson`), answer the JSON or the error Java raises: the parser's, or the emitter's
   refusal of a model it parsed.
   - **The differential** (`//rust/lite-front:front_engine_differential_test`, medium: 14 s in a contended `bazel
     test`, 8 s alone through cargo, against medium's 300 s): every question of the oracle at both doors, both forms,
     identical -- `document` 29,188 (23,257 JSON, 5,931 errors, the oracle's one refusal, a graph-fetch argument that
     is a call, among them) and `lambdaJson` 876 (870 JSON, 6 errors). The floors (`tests/document-floor.txt`,
     `tests/lambdaJson-floor.txt`) are the whole sets; the earlier doors' floors hold (all 110,596 questions).
   - **A form's hash.** `FrontEngineAnswers` hashed a form with `getBytes(UTF_16LE)`, which writes a lone surrogate as
     U+FFFD, where Rust hashes the units it wrote; a lone surrogate and a real U+FFFD hashed alike. It now hashes the
     units each as it is. No oracle answer holds one, so `engine.tsv` was rebuilt byte for byte the same.
   - **Beyond the oracle** (none of it committed; rust/lite-front/PARITY.md): 149,870 mutated questions (6,000 model
     texts at the engine's dialect and the 876 lambdas, structurally and in their literals: escapes, controls, lone
     surrogates, numbers at their edges), all identical in both forms but 6 `ArrayIndexOutOfBoundsException`s whose
     message the bulk run's warm JVM dropped (rows 29 and 55), each a cold JVM's answer byte for byte; 13,272 more
     mutating the tail sections, all identical; no panic. 530 probes aimed by an `llvm-cov` census, all identical.
     The census: the oracle reaches every `Element` variant an engine-dialect text can make and 91-100% of each
     module's region lines; the probes reach the refusals the oracle does not (rows 63-81); what neither reaches is
     what only the JSON readers make, what the engine's dialect refuses or drops before the emitter, and the
     `requireNonNull`s on fields the parser always fills. `liteOnly` is among it: the parser refuses a native function,
     a `Primitive`, a projection association and an overlay section at the engine's dialect, so no text reaches it.
   - **The shim** (`lite-ffi`'s `lite_front_engine(door, spans, overlays, text)`, `Lite.frontEngine`, a four-argument
     downcall declared in native's `reachability-metadata.json`, `RustParsers.document` / `lambdaJson` and their
     provider). The JSON crosses as UTF-8 (`A`), or, when it holds a lone surrogate, which UTF-8 cannot carry, as a JSON
     string Java reads back (`U`): the common answer pays nothing for the rare one. The provider now rethrows a
     `NullPointerException`, a `ClassCastException` and an `ArrayIndexOutOfBoundsException` as their own classes (the
     emitter's `requireNonNull`, the parser's rows 28-29, 35, 50 and 55), not as an `IllegalStateException` naming them.
   - **The switch.** `PmcdParser.parseDocument` (both overloads) and `SpecParser.lambdaJson` answer from Rust. **The
     overlay path**: when the document's walk meets a section of a registered overlay grammar, Rust answers nothing and
     `parseDocument` answers as Java does, `ProtocolEmitter` over `PmcdParser.parse`'s records -- which call the overlay
     grammars, let one's exception win, and splice their opaque elements in order, where the emitter refuses the first
     lite-only one. Rust never makes an opaque element. At the engine's dialect the walk refuses an unknown section
     before any overlay grammar is consulted (`'Toy' is not a known section parser`), so no text takes the path today:
     it is Java's answer by construction, not a fallback, and the toy-overlay text answers that refusal through the
     switched door as Java does.
   - **The shim's pass** (`//core:front_shim -- --engine --shim`, `FrontEngineAnswers --shim`, each form its own call
     through the switched doors, one 1 GiB thread): byte-identical to `engine.tsv`, 30,064 of 30,064 -- equal per door
     to the differential. `//core:ffi_test`, `core_tests_server` (`PureV1ApiTest`, `ModelChangesTest`),
     `core_tests_root` (`ModelFromTextTest`), `core_tests_parser`, `core_tests_protocol`, `core_tests_architecture`,
     `census`, `section_grammar_registry_test`, `//rust/lite-front/...`, `//rust/lite-ffi/...`,
     `//parser-equivalence:parser_parity` (45 tests, none skipped: `CorpusSweepTest` 6,755 documents matched, diff 0,
     and so without source information; `ModelReaderParityTest` 37,739 own elements, 38,702 engine, 13,514 documents,
     205 lambdas, none mismatched) and `:round_trip_proof` (`JsonModelDoorTest` with its four pinned failures,
     `RoundTripProofTest`): all pass.
   - **What stays Java**: `ProtocolEmitter` and its helpers, for their record-sourced callers, until phase 3 (§2, the
     corrected row); the oracle still asks Java's emitter over Java's records, so `engine.tsv` stays Java's answer.

   **The receipt, with the FFI kept apart** (2026-11-06). The oracle's engine questions, each asked twice -- with its
   spans and without them, as the doors are called --, the last of three passes; the three ways back to back, twice,
   at load averages 2.2-3.0:
   - **Java**: `ProtocolEmitter.emit(PmcdParser.parseInJava(text, LEGEND_ENGINE).model())` and
     `ProtocolEmitter.emitLambda(SpecParser.parseLambdaInJava(text))`, each cut by `withoutSpans` for the bare form
     (`//core:front_timing --engine --java`);
   - **the shim**: the switched doors (`--engine`);
   - **Rust natively**: `doors::document` and `doors::lambda_json`, release, no FFI and no JVM (a scratch harness).

   | | Java | the shim | Rust natively |
   |---|---|---|---|
   | `document`, spans: p50 / p95 / max | 0.016 / 0.20-0.21 / 159-166 ms | 0.032 / 0.37 / 351-368 ms | 0.015-0.016 / 0.23 / 192-199 ms |
   | in all (29,188) | 1.78-1.80 s | 3.46-3.52 s | 1.93-2.02 s |
   | `document`, bare: p50 / p95 / max | 0.019-0.020 / 0.35-0.36 / 304-306 ms | 0.030-0.031 / 0.38 / 333-341 ms | 0.015 / 0.27-0.28 / 228-240 ms |
   | in all | 3.05-3.16 s | 3.45-3.51 s | 2.28-2.35 s |
   | `lambdaJson`, spans: p50 / p95 / max | 0.004 / 0.045 / 0.23 ms | 0.011 / 0.10-0.11 / 0.66-0.69 ms | 0.003-0.004 / 0.057-0.060 / 0.42-0.44 ms |
   | in all (876) | 9.4-9.7 ms | 23.4-23.9 ms | 11.1-12.1 ms |
   | `lambdaJson`, bare: p50 / p95 / max | 0.004 / 0.10-0.12 / 0.72-0.87 ms | 0.014 / 0.10-0.11 / 0.66-0.68 ms | 0.003 / 0.077-0.081 / 0.60-0.62 ms |
   | in all | 19.2-20.1 ms | 24.4-25.1 ms | 14.1-14.4 ms |

   - **Rust itself**, split: the parse 1.32-1.36 s of a form's 29,188 documents, the emit 0.46-0.49 s (659 million
     UTF-16 units of JSON), the cut 0.50-0.52 s (to 319 million). Java's cut costs 1.27-1.36 s (bare less spans), so
     the bare form is faster in Rust, natively, than in Java; the spans form about as fast (the parse is as fast as
     Java's, step 4's receipt).
   - **The rest is the crossing**: about 1.5 s for the spans form and 1.2 s for the bare one -- the JSON's UTF-16 to
     UTF-8, the C string, and Java's decoding of it into a `String` (0.66 and 0.32 billion characters). It goes when
     phase 3 keeps the records, and the engine JSON's callers, in Rust.
6. **The deletion (2f)**: `//parser-equivalence`'s internal-using tests re-expressed over Rust ops (§2), then the Java
   lexer and parser deleted, `PreludeGenerator` retargeted, core's tests of parser internals retired as rows of
   `rust/RETIRED_TESTS.md`, the guards that name the deleted classes moved with dated comments.

**Gates at every switch:** `bazel test //...` against `rust/BASELINE.tsv`; `//parser-equivalence:parser_parity`
whole and `:round_trip_proof` (they need the upstream checkouts); the live differential while the Java parser
exists, and the committed oracle after.

## 5. Rulings (the user, 2026-11-05: each as recommended)

1. **The seam.** Switch the parser's entry points to Rust in phase 2, over the lossless record codec (§3) --
   recommended: the untouched test suite then judges the Rust parser, and the Java parser is deleted in this phase.
   The alternative ports the parser and checks it against the oracle only, and switches the product path with phase 3,
   keeping the Java parser alive until then.
2. **The Postgres-SQL translator moves to phase 3**, beside the typer it runs inside (still iteration 1). In phase 2 it
   would need either a callback into the Java typer or a Java copy of its core for SAP IQ and Cypher. Phase 3 defers
   the second half, it does not dissolve it: when the translator moves, SAP IQ and Cypher still use `SqlToPure`, so
   either a Java copy of the core stays for them until their wave, or the Postgres translator waits for that wave too.
3. **`ide/ModelIndexer` and `ModelOrchestrator`** read tokens and have no main caller: delete them with
   `ModelIndexerTest` and `ModelOrchestratorTest` as ledger rows, or port them onto Rust's tokens.
4. **The roadmap's gates**: replace "22,725 elements" with the live gates of §1, `//parser-equivalence:parser_parity`
   named whole.
