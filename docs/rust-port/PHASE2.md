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
| `Lexer`, `TokenStream`, `ElementParser`, `SpecParser`'s body, `MappingProtocolParser`, `DatabaseProtocolParser`, `GqlParser`, `QuotedSpecParser`, the section grammars, `TokenStreamCursor`, `IslandScan`, `EngineQuirks`, ... | move, then deleted (deleted 2026-11-06, step 6b: §4) |
| `PreludeGenerator`'s cursor use | retargeted to the element spans Rust answers; held by regenerating the committed module byte for byte (done in step 6b: a fifth probe, `declarationExtents`, §4) |
| `//parser-equivalence`'s tests of parser internals (`RejectionParityTest` and its pinned `MIN_PINS`, `GenerativeDualParseTest`, `Sectionize`, `SectionParseSentinelTest`, `OwnDialectCensusTest`, `SurfaceCensusTest`, `GrammarCoverageCensusTest`, `Surfaces`, `ViewFilterParityTest`, `OffsetCompositionParityTest`) | **gates, not retirees**: re-expressed over Rust ops (section splitting, the surface census) before step 6, so `:parser_parity` keeps judging the parser it was built for (done in step 6a, 2026-11-06: §4) |
| `ide/ModelIndexer`, `ModelOrchestrator` | deleted with the lexer (§5, ruling 3), their tests ledgered (done in step 6b, with `ide/ModelIndex` and `parser/ElementKind`, theirs alone: the whole `com.legend.ide` package) |
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
   - **`//core:seam_benchmark_small_heap`** (manual, so not in `rust/BASELINE.tsv`) fails at the switch's head and
     the same way before it (`dcc10a5ba`, 2026-11-06): H2 runs out of memory in its 256 MB heap at step 5 ("csv
     whole text in one value"), and step 6 then finds the database closed. The failure predates the switch and the
     streaming reader.
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
     to the differential. The `U` form, which no oracle answer takes, was checked through the switched doors on the
     probe set (its lone-surrogate strings among them): 529 distinct questions, all identical to Java's.
     `//core:ffi_test`, `core_tests_server` (`PureV1ApiTest`, `ModelChangesTest`),
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
     the bare form is faster in Rust, natively, than in Java (2.28-2.35 s against 3.05-3.16 s); the spans form is
     8-12% slower (1.93-2.02 s against 1.78-1.80 s), its p50 the same and its p95 a tenth higher -- the parse,
     about as fast as Java's in step 4's receipt, is most of it.
   - **The rest is the crossing**: about 1.5 s for the spans form and 1.2 s for the bare one -- the JSON's UTF-16 to
     UTF-8, the C string, and Java's decoding of it into a `String` (0.66 and 0.32 billion characters). It goes when
     phase 3 keeps the records, and the engine JSON's callers, in Rust.
6. **The deletion (2f)**: `//parser-equivalence`'s internal-using tests re-expressed over Rust ops (§2), then the Java
   lexer and parser deleted, `PreludeGenerator` retargeted, core's tests of parser internals retired as rows of
   `rust/RETIRED_TESTS.md`, the guards that name the deleted classes moved with dated comments.

   **6a done 2026-11-06** (from `1245f88bb`): the two halves that need the Java parser alive, so its answers and the
   gates' old forms could be compared with the new; no Java parser code deleted (that is 6b).
   - **The oracle frozen** (`cf9a866ef`). The three live outputs the differentials read at `1245f88bb` are committed
     under `rust/oracles/front/` -- `tokens.tsv.zst` (1.4 MB), `answers.tsv.zst` (2.3 MB), `engine.tsv.zst` (1.5 MB),
     `zstd -19 --long=27` -- each with its question-sha column cut (every file is in the questions file's order; 5.2
     MB against 9.9 with it), the differentials walking questions and answers in lockstep and refusing a line whose
     door or dialect is not its ask's. `MANIFEST.tsv` names each file's producing target, commit and SHA-256, the live
     file's SHA-256 before the cut, and the archive of Java's full answers outside git
     (`~/.local/legend-lite/oracle-archive/front-answers-java.jsonl.zst`, 3,556,378,580 characters, its SHA-256 checked
     by decompressing it). The live actions stay until 6b; the differentials no longer depend on them. The counts,
     identical live and frozen, every question identical: the lexer 52,143 texts (18,529,523 tokens); the doors pmcd
     87,564, spec 22,151, lambda 876, block 5; the engine doors document 29,188 (23,257 JSON, 5,931 errors) and
     lambdaJson 876 (870, 6). Optimized, the lexer differential ran in 8.5 s (medium, 300 s), the parser's in 24 s
     (large, 900 s), the engine's in 9 s (medium, 300 s).
   - **The gates asked of Rust** (`ef981f5b7`, `7fde30fbe`). Four probes on the existing `lite_front` export
     (`rust/lite-front/src/probes.rs`, each a port of the Java a test called): `sectionHeaders` (`Sectionize`),
     `elementSites` (`ElementParser.topLevelIndexes` and `at`'s readers: `RejectionParityTest`'s strict surface),
     `databaseSites` (`ViewFilterParityTest`'s site scan, parse and emit) and `knownSections` (the registry's built-in
     half: `SurfaceCensusTest`, beside the overlays, which stay Java's). Java reaches them as `RustParsers` methods
     answered by `RustParserProvider`, and parser-equivalence through `Surfaces` alone: no class names a dialect that
     did not, no new downcall shape, no guard moved. `SectionParseSentinelTest` and `OffsetCompositionParityTest`
     named deleted classes in comments only (their code already asked Rust); `GenerativeDualParseTest`,
     `OwnDialectCensusTest` and `GrammarCoverageCensusTest` reached them only through `Surfaces`' text doors. Beyond
     §2's list, `ParserEquivalence` and `LegendLiteSectionParser` read `PmcdParser.parseSections` (the Java walk, the
     source of `CorpusSweepTest`'s seam leg, `QuotedImportParityTest`, `OwnCorpusParityTest` and
     `ProtocolSeedParityTest`): they read the model door's records now. After it no parser-equivalence source names a
     class 6b deletes; `parseSections`' last caller is `ParseSpeedBenchmark` (manual). The old and new forms compared
     over the oracle (`//core:front_shim -- --probes`), all identical: the section headers of 52,143 texts (42,421
     headers); at each of three dialects the element sites of 29,188 model texts (108,826 to 131,161 sites; 2,024, 619
     and 504 errors) and their Database sites (7,119; 138 errors); the elements' JSON of the engine-dialect texts
     (146,668) and each `###Pure` section's code alone in imports, paths and JSON (40,309 sections, 112,963 elements);
     the known sections (25). `//parser-equivalence:parser_parity` (45 tests), `:round_trip_proof` (2) and
     `:diagnostics` (1) pass before and after with every report they write byte-identical but one label
     `RejectionParityTest` spelled with a deleted class's name (its count, 2, unchanged): 422 pins (`MIN_PINS` 420),
     421 rejected, 0 missed, line agreement 415 (413), column exact 337 (335); the sentinel 2,217 in scope, matched
     2,216 (2,093), lenient 1 (17), unjustified 0, defects 0; the generator 1,029 both-reject, 0 divergences; the
     surface census 57 rows, none missing; the sweep 6,755 documents matched, seam 6,746/0. No PARITY row: Rust
     answered every question as Java.
   - `bazel test //rust/... //parser-equivalence/... //core:ffi_test //core:core_tests_architecture //tools/guards/...
     //gates/...` at `cb7c913d5` (the code of `7fde30fbe`): 527 targets, 525 pass, the same 2 skipped (547 s; 802 s
     at `ef981f5b7`, the same counts); `cargo test` and `cargo clippy --all-targets -- -D warnings` on lite-front and
     lite-ffi clean. Ledger: `rust/RETIRED_TESTS.md`.
     What 6b deletes and every reference to it, classified, is an inventory written for it (not committed).

   **6b done 2026-11-06** (from `2d23acdff`; the code in `851edb93c`, `62051b5ef`, `16ab76514`): the Java lexer and
   parser deleted, the tests and guards that reached them retired, re-expressed or moved, the prelude generator
   retargeted. Every row of the inventory 6a wrote was worked; where it misread a test, the test says what it is
   (below).
   - **The prelude generator's probe** (`851edb93c`). `spec/src/gen`'s `PreludeGenerator` (the `//spec:gen_prelude`
     action) cut each declaration's verbatim text with the Java lexer and cursor. A fifth probe beside 6a's four,
     `declaration_extents` (`rust/lite-front/src/probes.rs`; `lite_front`'s `declarationExtents` door,
     `RustParsers.declarationExtents`, the same export and shim path): every declaration at brace depth 0 and a
     declaration's position (the stream's first token, or after `;` or `}`; a documentation literal before its head
     passing the position on and starting it) whose head is `Class`, `Enum`, `Profile`, `function` or
     `native function`, its kind and its extent in UTF-16 units, from its first token to the end of the last token
     its kind's reader consumed (a native's `;`) -- the generator's own brace scan, ported. The generator walks the
     extents for its platform functions (a function's record read from its own slice through `PmcdParser.parse` at
     the platform's dialect, for its stereotypes and name) and cuts a class's, an enumeration's or a profile's text
     from the extent that starts at the element's offset, refusing by name an offset no declaration starts at. The
     action already parsed through the Rust doors, so it carries the library as `//core` does. Held:
     `//spec:gen_prelude` writes `prelude.pure` byte for byte (`cmp`; `//core:update_generated_5_test`).
   - **Deleted** (`16ab76514`), 29,170 lines in all with the tests: `com.legend.lexer` (5 files, 1,418 lines); the
     parser's machinery, 15 files, 10,459 lines (`ElementParser` 2,481, `MappingProtocolParser` 3,490,
     `TokenStreamCursor` 1,471, `DatabaseProtocolParser` 1,208, `GqlParser` 720, `ServiceStubDataParser` 218,
     `OperatorParts` 188, `QuotedSpecParser` 181, `IslandScan` 157, `ServiceLegacyMappingParser` 101, `NumberLiterals`
     86, `ParseTap` 75, `EngineQuirks` 55, `RelationIslands` 55, `ElementKind` 53); `parser/section/`, 23 files, 9,503
     lines; `com.legend.ide`, 4 files, 624 lines (ruling 3's two, and `ModelIndex`, theirs alone -- one step past the
     ruling's text, as `ElementKind` is). Reduced to their Rust calls: `PmcdParser` 1,059 -> 166 lines (the doors, the
     records, the overlay splice), `SpecParser` 3,673 -> 63 (the four doors), `SectionGrammarRegistry` 144 -> 60 (the
     ServiceLoader's overlays). `ProtocolEmitter` and its helpers stay (phase 3). The targets `//core:lexer`,
     `//core:ide`, `core_tests_lexer`, `core_tests_ide`, `front_oracle`, `front_tokens`, `front_answers`,
     `front_engine_answers` and `front_show` went with them; `native-claims.tsv` regenerated (`QuotedSpecParser` left
     four `plus` rows). `//core:front_shim` stays, a manual tool that asks every question through the switched doors
     and compares with the frozen files: the four doors 110,596 of 110,596 asks identical, the engine-JSON doors
     30,064 of 30,064.
   - **Retired tests**: 6 classes, 76 tests -- `lexer/LexerTest` (29), `LexRangeTest` (2), `TokenStreamPositionsTest`
     (5), `parser/LexerRegistryAgreementTest` (1), `ide/ModelIndexerTest` (29), `ModelOrchestratorTest` (10) -- and 4
     test methods of kept classes: `ElementParserTest.parseTokenStreamOverloadAndStringOverloadProduceEqualResults` and
     `linesOfIsStringLines`, `SpecParserTest.parseFromTokenStreamSlicePreservesSourceOffsets`,
     `TextSurgeryPinTest.dataCellColonIsNotAPathSeparator`; and the front oracle's Java side (`FrontOracle`,
     `FrontTokens`, no tests). Four Rust unit tests hold what three of them held: `path_colon_of`'s five
     counterexamples, `lines_of`'s 25 cases (`element_parser.rs`) and the lexer/known-sections agreement
     (`model_parser.rs`). `PlatformSurfaceGuardrailTest`, which the inventory listed to retire, is kept unchanged: it
     guards which main files name the platform's dialect (`Dialect`, `Pure`, `Prelude`), a subject that survives.
   - **Re-expressed tests** (`62051b5ef`), 16 sources, every assertion kept: the shared helpers `testing/Platform`,
     `Own` and `Engine` (the token overloads gone; `Platform.mapping(String)` and `Engine.element(String)` through
     the model door); `CleanSheetProtocolShapeTest`, `SemiStructuredEmissionTest`, `DocumentationTest` (their element
     through the door); `ElementParserTest` (two tests through the door), `SpecParserTest`,
     `SectionGrammarRegistryTest` (the built-ins among `RustParsers.knownSections()`), `TextSurgeryPinTest`; four
     unused imports (`MappingNormalizerTest`, `ModelNormalizerTest`, `LegacyCleanSheetConvergenceTest`,
     `ClassSourceTest`); `FrontReaderRoundTripTest` (the Rust library's own codec answers read back, 232 s, where it
     read Java's); `ParseSpeedBenchmark` (`parseModel`, where it timed the Java walk). And, with the deletion, the
     front oracle's kept tools (`FrontAnswers`, `FrontEngineAnswers`, `FrontTiming`, their Java modes gone).
   - **Guard moves** (each dated where it stands, a ledger row, a GATES line): `ArchitectureTest`'s 7a and 6f retired
     (their numbers not reused), its library map, package lists and 6i's pin re-pointed at what is kept, 7c's surface
     and the static register shrunk; `ObservabilityGuardrailTest` (`LEGEND_PARSE_TAP`), `ErrorShapeGuardrailTest`
     (`QuotedSpecParser` 1), `DialectBoundaryTest` (its three name-decision rows: two files deleted, `PmcdParser`
     re-measured 0), `DialectOwnerExclusivityTest` and `DatabaseOwners`, `DropInSurfaceTextRuleTest` (its source
     re-read rows), `CodeShapeGuardrailTest`'s allowlist, `ParserBoundaryArchTest.DIALECT_CLASSES`,
     `tools/deps/core-layers.txt`; and two the inventory did not foresee, both the retired tests' own-corpus snippets
     leaving: `OwnDialectCensusTest`'s `LexerTest` host and `OwnCorpusConformanceTest`'s `PURE-DIALECT-diagram` row
     (`LexerTest#40`), and parser-equivalence's `own_corpus.matched` ratchet 3009 -> 2943 -- 46 snippets left with the
     retired tests (`ModelIndexerTest` 36, `LexerTest` 4, `ModelOrchestratorTest` 3, `LexRangeTest` 1, the two
     retired methods 1 each), none joined, `REFERENCE_REJECTED` 260 -> 248 (each a deleted host's or a retired test's
     row), diffs 0 before and after.
   - **The gates.** `bazel test //core/... //spec/... //rust/... //parser-equivalence/... //tools/... //gates/...`:
     525 targets (6a's 527 less `core_tests_lexer` and `core_tests_ide`), 520 passed and the same 2 skipped on the
     first run (733 s); the three that failed passed after: `parser_parity` and `update_ratchets_test` (the two
     own-corpus moves above) and `//datacube:python_engine_test` (`page.reload: net::ERR_NETWORK_CHANGED`, the
     machine's network, green on its rerun). `//parser-equivalence:parser_parity` (45 tests, 180 s), `:round_trip_proof`
     and `:diagnostics` rerun fresh: every report byte-identical to 6a's but the three own-corpus reports, which differ
     by exactly the retired hosts' rows and the renumbered literal runs -- `RejectionParityTest` 422 pins, line
     agreement 415, column exact 337; the sentinel 2,217 in scope, matched 2,216, lenient 1, defects 0; the sweep
     6,755 documents matched, seam 6,746/0; `ModelReaderParityTest`'s and the composer's counts unchanged. No class
     of the deleted list is named by code outside the Rust provenance comments and history (`git grep`; comments in
     a few kept tests still cite the classes they once exercised). `cargo test` and `cargo clippy --all-targets -- -D
     warnings` clean on lite-front and lite-ffi. Ledger: `rust/RETIRED_TESTS.md`; every test source changed since
     `7fdf4582a` is a row.

   **Post-6b fixes done 2026-11-09** (from `91db76618`, the rulings of §6; commits `2a20d1cf9`, `f5175c9c3`, `62e08ffaf`,
   `e1fba827f`, `508f7fb7f`): rulings 2, 4 and 9.
   - **Ruling 2** (`2a20d1cf9`). `ArchitectureTest`'s 6i pins `PmcdParser` beside `SpecParser`. The widened rule found 11
     dependencies in three classes outside its packages, each a parse entry by role, exempted by name so a new class in
     either package still trips it: `door.ModelFromText`, the generated `ffi.FrontReader` and `ffi.FrontRecords`.
   - **Ruling 4** (`f5175c9c3`). Every Rust site answering a `java.*` class was tied to its PARITY row, and every input
     shape of each row asked of legend-engine's own parser on the parser-equivalence class path (`PureGrammarParser`,
     4.145.0, every extension grammar present: 340 inputs, each door at each dialect; the probe not committed, its
     results these rows). A refusal is a `ParseException` at the engine's place: its section's start where the engine's
     section catch-all answers (`PureGrammarParser.visitSection`, the place `section_span` already computed), the token
     where its grammar refuses. It travels as neither a `ParseException` nor a JDK class (`JavaError::as_refusal`,
     `refused_at_section`), so Java's `catch (ParseException)` recovery points pass it on as they passed the exception it
     replaces and no refused text becomes accepted; every door turns it into the `ParseException` (`placed`,
     `at_section`). What came of each row:

     | row | outcome | the engine's answer | the deliberate-difference list |
     |---|---|---|---|
     | 3 | left for the user | accepts (`%2024-02-30T10:00+0500` is text to it) | none (the frozen `672e812e` is `raw-left-for-user.tsv`) |
     | 6 | fixed | `For input string: ...`: section catch-all in a model, raw as an expression | none |
     | 7 | left | accepts `1e999` | none |
     | 8 | split | refuses `1.5ed` at `ed`; accepts `1.ed`, `1.e+d`, `1.ef`, and `1.5e99999999999` at a float | none |
     | 9 | fixed | section catch-all (`Less than 4 hex digits ...`, `Unable to parse unicode value: sers`) | 4 asks (`109b58da`: `pmcd` at three dialects, `document`) |
     | 26 | fixed | `Unexpected token` at the `-` | none |
     | 28 | fixed | section catch-all, `... 'NullPointerException' occurred, please notify developer` | none |
     | 29 | fixed | at `<EOF>`; `~src` one column before it | none |
     | 32 | fixed | `Unexpected token` at the terminator | none |
     | 35 | fixed | `Unexpected token '<EOF>'` | none |
     | 37 | fixed | section catch-all, `For input string: ...` | none |
     | 46 | left | accepts an empty `###Diagram` | none |
     | 49 | fixed | `Unexpected token '1-2'`, `'x'` at the token | none |
     | 50 | split | its reader fails on a node with no `bsonType` or a non-string one; ignores the rest | none |
     | 52 | fixed | `Unexpected token '"'` at it | none |
     | 55 | fixed | `<EOF>` where an island is admitted, `#{` where not | none |
     | 56 | fixed | `<EOF>` | none |
     | 63 | fixed | at the first token past the argument's first primary | 1 ask (`00dc41c7`, `document`) |
     | 72 | left | accepts the local property | none |
     | 73 | split | refuses the word; accepts `platform: Default #{ x: 1; }#` | none |
     | 76 | fixed | at the `)`, the word or the strategy | none |

     Six raw sites no row named, found by the census and by mutating the oracle's texts, are new rows 82 to 87 (a
     FileGeneration integer past a long; `#{` and `#/` as the last token; a `'` left open in a mapping test, a panic in the
     Rust; a `'''` left open; schema algebra as a type argument). Sites proven unreachable are `unreachable!` or `expect`
     with the reason (`operator_parts`' collection carrier, among them). The rows of the emitter's same
     `UnsupportedOperationException` the ruling does not name (68, 69, 70, 71, 74, 81) are unchanged and listed for the
     user. PARITY's rows say each row's answer and date; `docs/SEMANTICS_REGISTER.md` S128 to S150 quote the engine's answer
     for each fixed row, no standing row covering parse wording. The frozen oracle stays Java's:
     `rust/oracles/front/deliberate-differences.tsv` lists the 5 asks, `raw-left-for-user.tsv` the 1 frozen question still
     raw, and the three differentials and `//core:front_shim` hold identical + listed == total both ways and fail on any
     other `java.*` answer (no floor lowered). `RustParserProvider.raised` loses its `ArrayIndexOutOfBounds` and
     `Arithmetic` arms, which no site raises; the native image's reachability metadata named none of these classes.
     Beyond the oracle: 1,530,080 door answers over mutated oracle texts, no panic, the raw classes left being those above.
   - **Ruling 9** (`62e08ffaf`; `e1fba827f`, formatting alone). `DocElement` and `DocSection` deleted. Comments in ten kept
     tests and `lite-front`'s description name what stands today. The library written out of the jar goes to
     `<user.home>/.cache/legend-lite/lib/<sha256>/<file>`, through a temp file and an atomic move, reused only when it
     hashes to its name; an unwritable cache falls back to a process-private temp file. `FrontRead.Cursor` starts at an
     offset, so the reader's two whole-answer `substring` copies are gone (`FrontReader` is hand-written; `FrontRecords`,
     generated, did not change). `rust/rustfmt.toml` at 120 columns and `cargo fmt --all`, the generated `records.rs`
     skipped on its `mod` line.
   - **The gates.** `bazel test //core/... //spec/... //rust/... //parser-equivalence/... //tools/... //gates/...`: 525 targets, 523 pass and the
     same 2 skipped at `508f7fb7f`. Its first run, at `e1fba827f`, failed one, `//wasm:round_trip_test`: the frozen
     `//wasm:planner`, TeaVM's last build of the Java parser, keeps the raw answer on `TestProfile.java#52`, the text the JVM
     now refuses (row 9); it is a named difference held both ways (`FROZEN_MODULE_DIFFERS`, `508f7fb7f`), gone with the Rust
     wasm32 build. `//parser-equivalence:parser_parity`: 422 pins, 421 rejected, 0 missed, line agreement 415, column exact
     337 (no count fell, none rose); the sentinel 2,217 in scope, matched 2,216, lenient 1; the sweep 6,755 documents
     matched, seam 6,746/0; own corpus 2,943, `diffs=0`. `//core:front_shim`: the four doors 110,593 identical and 3 listed of
     110,596, the engine-JSON doors 30,062 and 2 of 30,064. `cargo test --workspace`, `cargo clippy --workspace --all-targets
     -- -D warnings` and `cargo fmt --all -- --check` clean. Ledger: `rust/RETIRED_TESTS.md`.

   **Rulings 11 to 13 done 2026-11-09** (from `b174584b7`, the rulings of §6; commit `1c9920466`). Every input was asked of
   legend-engine's own parser on the parser-equivalence class path (`PureGrammarParser`, 4.145.0; the post-6b probe, 335
   further inputs, not committed; each answered beside Rust at the model or lambda door and at the JSON door).
   - **How.** Java's `LEGEND_ENGINE` is three levels inside the doors (`doors::Dialect`, set by the doors alone): the
     expression doors' and the quoted folds', the model door's and the `lambda` door's (which refuse row 63, ruling 13),
     and the one an engine-JSON door reads a text at when the emitter refused its records (ruling 12). The records do not
     keep the tokens the engine's places need (the token after a cast's `)`, a dated argument's offset, an activator's
     key), so the emitter's raw refusal stays the signal and the door answers what the second reading places; a text that
     reading accepts keeps the emitter's raw answer. The happy path never reads twice. A graph-fetch argument is refused
     in the parser, which moves the refusal before any later syntax error of the same section at the two JSON doors too.
   - **Ruling 11** (refused, deliberately stricter than the engine, each its own register row):

     | row | input | the engine | lite now | register |
     |---|---|---|---|---|
     | 3 | `%2023-02-29T10:30+0100`, every dialect and door | accepts, the literal as text | `invalid date literal ...` at the literal | S151 |
     | 7 | `1e999`, `1.0e999`, `-1e999`, `1e999f`, lite's and the platform's dialects | accepts: a name, `1.0e999` the float `Infinity` | `float literal out of range: ...` at the number | S152 |
     | 8 | `1.ed`, `1.e+d`, `1.ef`, those dialects | accepts: `1`, `.`, a name | `Unexpected token 'ed'` at the exponent's letter | S153 |
     | 8 | `1.5e99999999999[f]`, those dialects | accepts: `Infinity` | `float literal out of range: ...` at the token | S154 |
     | 46 | `###Diagram` with another header next | accepts: an empty section | `an empty ###Diagram section ...` at its header | S155 |
     | 50 | a `title`, `description`, `required` element or length of the wrong kind, an integer past a long, under typed nodes | accepts: ignored or not visited | `a jsonSchema title must be a string` and its kin at the section's start | S156 |
     | 72 | a local property under an AggregationAware Pure `~mainMapping` (`document`) | accepts | `the local property 'loc' is not supported ...` at its name | S157 |
     | 73 | `platform: Default #{ x: 1; }#` (`document`) | accepts: a default platform | `Persistence platform 'Default' must not have a body` at the word | S158 |

   - **Ruling 12** (at `document` and `lambdaJson`, only where the engine refuses; the columns are the engine's, 222 of the
     probe's inputs exact, the differences named in PARITY):

     | row | input | the engine | lite now | register |
     |---|---|---|---|---|
     | 69 | `^a::B($x)` | at the argument's first token | `a positional cast ...` there | S159 |
     | 69 | `^a::B(1)` | a literal is the type's variable values: the token after the `)` | the same token (`[2:1]`, the line under a lambda's text) | S159 |
     | 69 | `@(a:?)`, `@Relation<(a:?)>` | `Unexpected token '?'`, `'<'` | the same words and token | S160 |
     | 70 | `p: String[m];`, a lambda's `x:String[m]` | section catch-all; raw at a lambda | section's start; a lambda's at the name | S161 |
     | 71 | a `DeephavenApp`'s `activationConfiguration: a::K;` | at the key | at the key | S162 |
     | 81 | `#/a::B/c($x)#` | at the token its grammar stops at, column shifted by the literal's | the same place (`[1:23]`) | S163 |
     | 68, 69 | `letFunction(1, 2)`, `let y = %10:10:10`, `let y = @(a:Integer)` | accepts | the raw answer, kept | none |
     | 81 | `c(1.5)`, `c(-1)`, `c([.5])`, `c(1.5f)` | accepts | the raw answer, kept | none |
     | 74 | `EqualToJson #{ expected: ['a']; }#`, `[a, b]`, `#/a::B/c#`, `[#/a::B/c#]`, `[{x: 1;}]`, `database:`, `binding:` | refuses, at the value or the key | the raw answer, unchanged; **the ruling's premise does not hold, these reach it; it comes back to the user** | none |

   - **Ruling 13.** `pmcd` at `LEGEND_ENGINE` and `lambda` refuse row 63's arguments with the JSON doors' error and place
     (`graph_fetch_argument_refused`, shared with the emitter, moved to `protocol.rs`). Register S164. `pmcd` at lite's and
     the platform's dialects keeps its records. The gap at `spec` and `block` at the engine's dialect is left, not ruled.
     No lite test, PCT source, corpus or Pure model relied on the `lambda` door accepting such an argument; the one
     snippet of lite's own the model door now refuses is the instrument's probe the engine refuses too.
   - **The difference lists.** `deliberate-differences.tsv` 5 -> 7 asks: `00dc41c7` at `pmcd`/`LEGEND_ENGINE`, `672e812e` at
     `spec`/`LEGEND_ENGINE`. `raw-left-for-user.tsv` and its readers are deleted: no frozen question answers a raw class,
     and the differentials fail on any `java.*` answer. No floor lowered. `//core:front_shim`: the four doors 110,591
     identical and 5 listed of 110,596, the engine-JSON doors 30,062 and 2 of 30,064. `FROZEN_MODULE_DIFFERS` is unchanged:
     the full gate showed no new difference.
   - **The Java side.** `RustParserProvider.raised` loses `java.time.DateTimeException`, which no Rust site raises; the
     other arms each have a site; the native image's metadata named none of these classes. `OffsetDateLiteralKnownDefectTest`
     retires with its defect, `OffsetDateLiteralRefusalTest` holds the refusal at every dialect.
     `OwnDialectCensusTest.EXTENSION_HOST_PINS`' `ProbeWireShapes.java` 1 -> 2 (ruling 13, its probe
     `#{I {product(2015-10-16){name}}}#`, which the engine refuses and the census had counted as accepted). Ledger:
     `rust/RETIRED_TESTS.md`.
   - **The gates.** `bazel test //core/... //spec/... //rust/... //parser-equivalence/... //tools/... //gates/...`: 525 targets,
     523 pass and the same 2 skipped; `//wasm/...` built and its 5 tests pass. `//parser-equivalence:parser_parity`: 422 pins, 421
     rejected, 0 missed, line agreement 415, column exact 337 (none moved); the sentinel 2,217 in scope, matched 2,216; the sweep
     6,755 documents matched, seam 6,746/0; own corpus 2,943, `diffs=0`. `cargo test --workspace`, `cargo clippy --all-targets
     -- -D warnings`, `cargo fmt --check` clean. 1,530,080 door answers over mutated oracle texts: no panic, no raw answer.

   **Rulings 15 and 16 done 2026-11-09** (from `dcea220f0`, the rulings of §6; commit `91de44e54`). Every input was asked of
   legend-engine's own parser on the parser-equivalence class path (`PureGrammarParser`, 4.145.0; 513 further inputs, not
   committed, each answered beside Rust at every door at the engine's dialect).
   - **How.** `doors::Dialect`'s inner levels of `LEGEND_ENGINE` are now the quoted reading (`LegendEngine`), the one every door
     reads a text at (`LegendEngineDoor`, `LegendEngineModel` until today, the expression doors now among them), the one an
     engine-JSON door re-reads at (`LegendEngineJson`), and `LegendEngineSyntax`. A refusal of the engine's walk, raised as the
     parser reads, is followed by a second reading of the same section at that last level (`JavaError::syntax_first`, on the
     error path alone), which reads what the walk refuses and passes islands and mapping element bodies by; a later error it
     meets is the answer unless it is one the engine raises while walking (`Field '...'`, `Unknown ...`, a section catch-all's).
     A mapping element's body does the same inside itself first. No new `java.*` answer; `NumberFormatException` has no Rust
     site left, and its arm has left `RustParserProvider.raised`.
   - **Ruling 11, corrected (row 8).** `1.ed`, `1.e+d`, `1.ef` and their kin at every dialect, at the letter: register S153
     updated. `1.e+` with nothing after its sign is at the sign at the engine's dialect, as the engine refuses it. The huge
     exponent stays limited to lite's and the platform's dialects.
   - **The rows** (the engine's words differ, its token and column are lite's, except where said):

     | row | input | doors | the engine | lite now | register |
     |---|---|---|---|---|---|
     | 74 | `EqualToJson #{ expected: ['a']; }#`, `[a, b]`, `[]`, `#/a::B/c#`, `[#/a::B/c#]`, `[{x: 1;}]` | `document` | at the value's first token | `Unexpected token '['` and kin there | S165 |
     | 74 | `database: a::D;`, `binding: a::B;`, any other key, a key after a good `expected` | `document` | at the key | `Unexpected token 'database'` there | S165 |
     | 74 | `EqualToJson { expected: ['a']; }` | `document` | at the `{` | `Unexpected token '{'` there | S165 |
     | 63 | `#{a::B{c(f())}}#` and every other argument | `spec`, `block` at the engine's | as the other doors | the row's refusal at its place | S164 |
     | 81 | `c(a::E.X)`, `c(1,)`, `c(,1)`, `c(1,,2)`, `c(%10:10)`, `c([a::E.X])`, `c([1,])`, `c([,1])`, `c(E.)`, `c(1.)`, `c(%x)`, the arguments of S163 | all six, at the engine's | where its grammar stops | `this argument of a dated path segment ...` there | S167 |
     | 73 | `Default #{ }#`, `Foo #{ }#`, `AwsGlue #{ }#`, any one-character island | `pmcd`, `document` at the engine's | section catch-all | `... must not have a one-character body` at the section's start | S168 |
     | 73 | `AwsGlue #{ dataProcessingUnits: 1; }#`, `foo: 10;`, a string or word value, a key after a good one, `dataProcessingUnits: 10` with no `;` | `pmcd`, `document` at the engine's | its cloud grammar over the text less two characters | `Unexpected token ...` where it stops | S168 |
     | 69 | `^a::B($x, $y)` | all, at the engine's | at the first `$` | the row's refusal there | S159 |
     | 69 | `^a::B(1, 2)` | all, at the engine's | past the group | the same; `[1:12]` at `spec`, `[2:1]` at `lambda`, the `}` in a function | S159 |
     | 69 | `^a::B(1, $x)`, `^a::B(1,)`, `^a::B('a'+'b', 1)`, `^a::B('a'+'b')` | all (the last at the JSON doors) | at the stop | the same | S159 |
     | 69 | `^a::B(1)(x=1)` | all | accepts | `trailing tokens ...` at the second `(`; stricter, no code change | S166 |

   - **Row 63 and a later syntax error** (ruling 16 (c)), at every door it is refused: a syntax error later in the same function,
     section, expression or text comes first; the refusal stays first against a later section, the rest of its own island,
     another mapping element's body, `Field '...'`-worded walk errors. The new refusals of rows 81 and 73 keep the same order.
     Where the engine's phase cannot be told from lite's words, a later error of the walk that lite words as a plain parse
     error comes first; PARITY row 63 says so.
   - **The difference lists.** `deliberate-differences.tsv` 7 -> 9 asks: `1680d307` at `pmcd`/`LEGEND_ENGINE` and `document`, an
     `AwsGlue` island with its `;` left off, now the engine's `[34:0]`. No floor lowered. `//core:front_shim`: the four doors
     110,590 identical and 6 listed of 110,596, the engine-JSON doors 30,061 and 3 of 30,064. `FROZEN_MODULE_DIFFERS` is
     unchanged. No test source changed; nothing of lite's own relied on an input now refused.
   - **The gates.** `bazel test //core/... //spec/... //rust/... //parser-equivalence/... //tools/... //gates/...`: 525 targets, 523
     pass and the same 2 skipped; `//wasm/...` built and its 5 tests pass. `//parser-equivalence:parser_parity`: 422 pins, 421
     rejected, 0 missed, line agreement 415, column exact 337 (none moved); the sentinel 2,217 in scope, matched 2,216; the
     sweep 6,755 documents matched, seam 6,746/0; own corpus 2,943, `diffs=0`. `cargo test --workspace`, `cargo clippy
     --all-targets -- -D warnings`, `cargo fmt --check` clean. 153,008 mutated oracle texts through every door: no panic, no
     raw answer.

   **Ruling 18 done 2026-11-09** (from `8c7de71fc`, the rulings of §6; commits `1884e6ef7`, `52f66f7fd`, `2468f9f4c`, `44012955b`).
   Every input was asked of legend-engine's own parser on the parser-equivalence class path (`PureGrammarParser`, 4.145.0), the
   measurement's probes re-run on its pinned inputs (`docs/rust-port/ENGINE_GAP_2026_11_09.md`, "After ruling 18").
   - **The end of the input (§2 of the brief; register S170), in the cursor.** `throw_at` past the last token of a stream that runs
     to the end of its input stands one past the input's last character, blanks and line breaks counted (`TokenStream::input_end`:
     a whole text's length, a re-lexed range's end, none for a window short of its stream's last token). The expect family
     (`Cursor::missing`) places a missing closer one column less where ANTLR reports a token inserted or removed (the closer ends
     a top-level element, or `sync` fails at a loop's back edge: `ParserErrorListener`'s empty span), read off the brackets left
     open. A quote left open at the end is refused at the quote. A construct only the walk refuses, cut by the end, is refused at
     the end (`JavaError::end_of_input_first`, the text read again at the platform's dialect, on the error path alone). A mapping
     element's or connection's body left open by the end is the outer parse's refusal, one column before the end.
   - **Never looser (§1; register S169), by root cause, at `LEGEND_ENGINE`'s door levels alone:** imports by the engine's rule; every
     comma list as `item (COMMA item)*`, a helper for the 90-odd sites that wrote an optional comma; element bodies as islands
     counted by braces, strings no strings to them; clause orders (a Pure mapping's `~src`/`~filter` before its properties, a
     view's `~groupBy` before `~distinct`, an `include` before the elements, imports before a diagram's, `Views` first in an
     AggregationAware); `~func` and an activator's `function:` as `functionIdentifier`; a service's `query:` that is a bare name;
     `1.` without a name after it; `%latest` outside `.all(...)` and a property's arguments; a default value's first token; schema
     algebra and a unit's arguments in a type; a column spec's `:` followed by neither type nor lambda; `let` at the `spec` door;
     navigation paths by the engine's navigation grammar; embedded relation data by its grammar; a `Reference`'s text as a path;
     a persistence key its lexer does not name; a service-store parameter's scalar type; required `tests`. The record doors at the
     engine's dialect now refuse what the JSON door places (`doors::pmcd` asks the emitter, and takes its placed refusals but S157's),
     so S143, S144, S159, S160, S161 and S165 are refused at `pmcd`, `spec`, `block` and `lambda` too.
   - **Named exclusions.** S64's TIME and TIMESTAMP_NS columns (571 corpus, 280 and 74 mutated texts at `pmcd` and `document`). S157
     and S158, which the engine accepts, stay refused at the `document` door alone.
   - **Lite's own relied on the acceptance: left, and back to the user (S180).** A `Diagram X(width=.., height=..)` argument list
     (`ModelReaderParityTest`'s and `//wasm:round_trip_test`'s inputs, three reviewed rows of `docs/refusal-allowlist.tsv`); a
     mapping test with no `data` (`ElementParserTest.mappingTestSuitesParsedOntoTheRecord`, `...InteriorBracesBalanced`).
   - **Not reached: lite is still looser on 34 further texts** (of the pinned inputs; S180, the gap report lists them). Ruling 18
     asked for 0; each is a rule not yet written (a persistence key out of order, a Mongo mapping's clauses, scattered mutations
     inside embedded data). `docs/refusal-allowlist.tsv`'s row for `relationMappingSetup.pure` called its `~func` a version skew
     of the oracle: the pinned 4.145.0 refuses it, so it is refused now, and the row left.
   - **The difference lists.** `deliberate-differences.tsv` 9 -> 744 asks (`pmcd` at `LEGEND_ENGINE` 204, `LEGEND_LITE` 164,
     `LEGEND_PLATFORM` 167; `document` 204; `spec` at `LEGEND_ENGINE` 2, `LEGEND_PLATFORM` 3; 145 KB, kept plain as it is not
     large; the frozen files stay Java's, no floor lowered, the assertions two-way). `FROZEN_MODULE_DIFFERS` 1 -> 66 inputs, a
     generated data file (`wasm/frozen_module_differs.json`, from the test's own `round-trip-diffs.jsonl`), held both ways.
   - **Moved counts, each with its dated reason in place.** `ModelReaderParityTest` own and stripped 37739 -> 37648, documents 13514
     -> 13512, `JsonModelDoorTest` 7085 -> 7084, `RoundTripProofTest` matched 6905 -> 6904 and upstream 270 -> 271: one source,
     `relationMappingSetup.pure` (91 elements), which the engine refuses and the model door accepted. `LENIENT_RESIDUE` 13 -> 6 and
     the refusal allowlist 4 -> 3 rows, parity fixed. `OwnDialectCensusTest` `ModelNormalizerTest.java` 0 -> 1 extension row, a
     service's `query:` naming a function, lite's own, which the engine refuses. Of the 2,558 texts the engine door refuses in the
     two parity targets, the engine refuses every one. The sentinel rose: 2,217 of 2,217 matched, none lenient.
   - **The measurement, before -> after,** mutated at `pmcd`: same place 87,633 -> 99,981 of 153,008, lite looser 586 -> 300 (280
     S64), stricter 357 -> 358; the end of the input 17,713 -> 6,088. `spec`, `lambda`, `lambdaJson` looser 49, 44, 36 -> 0. Stricter
     and other place differences are ledgered by family, S171 to S179.
   - **The gates at `44012955b`.** `bazel test //core/... //spec/... //rust/... //parser-equivalence/... //tools/... //gates/...`: 525
     targets, 523 pass and the same 2 skipped; `//wasm/...` built and its 5 tests pass. `//parser-equivalence:parser_parity`: 422 pins,
     line agreement 415, column exact 337 (none moved); the sentinel 2,217 in scope, matched 2,217; the sweep's 6,755 sources;
     own corpus 2,943, `diffs=0`. `//core:front_shim`: the four doors 110,056 identical and 540 listed of 110,596, the engine-JSON
     doors 29,860 and 204 of 30,064. `cargo test --workspace`, `cargo clippy --all-targets -- -D warnings`, `cargo fmt --check`
     clean. 153,008 door answers over mutated oracle texts: no panic, no raw answer. Ledger check: no MISSING.
   - **The Java side.** `RustParserProvider.raised` is unchanged: ruling 18 adds no raw class and removes no site that raised
     one. No test source was re-expressed; the guard files that moved are above, each with its note. `block` changed through the
     expression doors' shared reading and was not measured (the measurement has no `block` door).

   **Rulings 19 to 21 done 2026-11-10** (from `3328a746e`; commits `e02b2cb0a`). Every input was asked of legend-engine's own
   parser on the parser-equivalence class path (`PureGrammarParser`, 4.145.0); the measurement's probes re-run on its pinned
   inputs with the `block` door added (`docs/rust-port/ENGINE_GAP_2026_11_09.md`, "After rulings 19 to 21").
   - **Ruling 19.** S180 split: a Diagram's m2 argument list (`Diagram X(width=.., height=..)`, the reviewed "DEFENDED" rows
     of `docs/refusal-allowlist.tsv`) is S180, the mapping test with no `data` (`ElementParserTest`'s two tests) S181, each
     ruled deliberately looser. No code change.
   - **Ruling 20: the 34, by root cause, each at the engine's line and column at the engine's dialect** (lite's and the
     platform's dialects read them as before but for the first): a line comment ends at a lone `\r`, at every dialect, as
     both upstreams' lexers end it (S182; no frozen question holds one); the persistence grammar's test key order and its
     `;`s (S183); its walker's arm of a service output's value by the output's kind, the engine's NPE refusal reproduced,
     and a graph-fetch output's required `keys` (S184); the orders of a `RelationalMapper`'s lists, a generation node's `id`
     and a MongoDB mapping's `~filter`, the last in its parser's coordinates (S185); a `%` literal ended where the engine's
     lexer ends it, a milestoning's `INFINITY_DATE` its `DATE` alone (S186); relation data by its grammar's words, and the
     walker's reparse of a test's or a data space's path-less relation at the `#{` (S187); a ServiceStore data island's and
     a content pattern's end (S188); a data space's test data path and its executable's `functionIdentifier` (S189); a
     HostedService's `postValidations` and `testSuites` by the Service's rules, no `;` after either list (S190); a service
     test's `primitiveValue` (S191); a Pure mapping's set id no number or boolean (S192); a mapping expression ending in a
     path (S193); a default value's whole `defaultValueExpression` (S194); a column spec's name that starts no lambda a type
     (S195). The walker-level rules are held at the engine's walk (`Dialect::holds_to_the_engines_walk`), so a later syntax
     error of the section comes first; the rules that ran before lite's own reading of a construct and moved texts away from
     the engine's place (a default value, a service test's value, a mapping expression's path, a HostedService's lists) run
     behind it. Probing found two comma lists ruling 18's helper missed (a DataQuality tree's `{...}`, a HostedService's
     `UserList`), now `item (COMMA item)*` (S169's rule).
   - **Ruling 20: the end of the input** (6,080 mutated texts at `pmcd`, 67 corpus after) ledgered as a family, not chased:
     S177, amended with its sub-causes and their counts.
   - **Ruling 21.** `block` measured at `LEGEND_ENGINE` against `parseLambda` (a function body's `codeBlock`) over the
     corpus's 22,091 expression texts: 2 looser (an empty and a blank block), refused now in the `lambda` door's words and
     place (S196); 2 stricter, ledgered (S151, S173). Nothing else.
   - **Lite's own relied on none of the 34.** Six of them are `FixtureCorpusParityTest`'s own negatives, which lite accepted
     as its adjudicated residue (`LENIENT_RESIDUE`): refused now as their comments say a conforming parser must, the residue
     6 -> 0 (shrink-only, its dated note in place) and with it the test's asserted agreeing fixtures 260 -> 266 (its own
     `266 - size()`, no literal changed).
   - **The measurement, before -> after**, mutated (16 per text) at `pmcd`: same place 99,981 -> 100,042 of 153,008, lite
     looser 300 -> 282 (280 S64, 2 S181), stricter 358 -> 358; corpus looser 581 -> 575 (571 S64, 3 S180, 1 S181); mutated
     (4 per text) 84 -> 74 (all S64); `spec`, `lambda`, `lambdaJson` 0, `block` 2 -> 0. No text moved away from the engine's
     place. Lite looser is 0 apart from the ruled exclusions.
   - **The difference lists.** `deliberate-differences.tsv` 744 -> 760 asks (`pmcd` at `LEGEND_ENGINE` 204 -> 212,
     `document` 204 -> 212; `ruling20` in the fourth column for the 16 new rows; the six fixtures above, a `%...Z` date now
     at the engine's column, a message's words): the frozen files stay Java's, no floor lowered. `FROZEN_MODULE_DIFFERS` is
     unchanged (66 inputs): `//wasm:round_trip_test`'s inputs did not change.
   - **Moved counts, each with its dated reason in place.** `FixtureCorpusParityTest`'s `LENIENT_RESIDUE` 6 -> 0, above.
     Nothing else moved: no parity count, sentinel, sweep or ratchet.
   - **The gates at `e02b2cb0a`.** `bazel test //core/... //spec/... //rust/... //parser-equivalence/... //tools/...
     //gates/...`: 525 targets, 523 pass and the same 2 skipped; `//wasm/...` built and its 5 tests pass.
     `//parser-equivalence:parser_parity`: 422 pins, line agreement 415, column exact 337 (none moved); the sentinel 2,217
     in scope, matched 2,217; the sweep's 6,755 sources, seam 6,746/0; own corpus 2,943, `diffs=0`. `//core:front_shim`: the
     four doors 110,048 identical and 548 listed of 110,596, the engine-JSON doors 29,852 and 212 of 30,064. `cargo test
     --workspace` (lite-front's 92 unit tests, 8 of them new), `cargo clippy --all-targets -- -D warnings`, `cargo fmt
     --check` clean. 153,008 door answers over mutated oracle texts: no panic, no raw answer. Ledger check: no MISSING.
   - **The Java side.** `RustParserProvider.raised` is unchanged: no raw class added or removed. No test source re-expressed
     but `FixtureCorpusParityTest`'s residue. `docs/refusal-allowlist.tsv`'s three Diagram rows stand, a dated line saying
     ruling 19 kept them (S180).
   - **Choices taken that are the user's** (each reversible): the line comment's `\r` at every dialect, not the engine's
     alone (both upstreams lex it so, no frozen answer moved); an empty block refused at the `lambda` door's unplaced place,
     not the engine's `[2:1]`, its wrapper's line; a HostedService's lists checked by the Service's readers and the suites'
     grammar while its records still skip them; the rules that run behind lite's own reading of a construct (a default
     value, a service test's value, a mapping expression's path, a HostedService's lists), so that in a text with two errors
     lite's later one can stand before the engine's earlier one (no pinned text does). Found and not refused: S183's `None
     {}` and an expression's `~owner:`-family column spec (the gap report's "Found while probing").

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

## 6. Rulings at the phase's end (the user, 2026-11-09)

1. **`own_corpus.matched` 3009 → 2943: ratified.** A denominator change: 46 snippets left with the tests step 6b
   retired; none joined; the parity differences are 0 before and after (6b's entry in §4).
2. **`ArchitectureTest`'s rule 6i is widened to `PmcdParser`**, so both text doors are pinned to the parse-entry
   packages.
3. **The archive of Java's full answers** (`~/.local/legend-lite/oracle-archive/`) **is kept until phase 3 ends**, then
   ruled on again.
4. **Bad input never answers a raw JDK exception.** All 21 rows of `rust/lite-front/PARITY.md` where it did (28, 29,
   35, 50, 55, and 3, 6, 7, 8, 9, 26, 32, 37, 46, 49, 52, 56, 63, 72, 73, 76) become placed parse errors, each matching
   legend-engine's answer for the same input, read from the engine's parser. A row where the engine accepts the input
   comes back to the user. The frozen oracle stays Java's: Rust's answers on these questions are listed as deliberate
   differences, by row.
5. **The generated reader's strictness is accepted as it stands** (whitespace, extra, missing or out-of-order keys and
   a bad `\u` escape refused; the depth refusal naming the innermost component; an `int` overflow an
   `ArithmeticException`), and with it the guard move for `FrontRead.pos` and `depth`.
6. **`declaration_extents`' narrower lookup is accepted**: a wanted declaration not at a declaration position is
   refused by name, which fails the generator loudly; the probe is widened if an upstream bump ever hits it.
7. **The five probes stay in the product library.** Phase 4 redraws the library's exports and can split them off then.
8. **Accepted as done:** the whole `com.legend.ide` package deleted (one step past §5's ruling 3); `ViewFilterParityTest`,
   `ParserEquivalence` and `LegendLiteSectionParser` read through Rust; `front_shim` kept as a manual tool; the
   diagnostics workflow triggered by `rust/lite-front/**`.
9. **The leftovers are cleaned up before phase 3**: `DocElement` and `DocSection` deleted, comments and `lite-front`'s
   description that name deleted Java corrected, `cargo fmt`, a stable path for the library written out of the jar, and
   the reader's two whole-answer `substring` copies removed. (Step 4d's `ustr` wording and `jnum` port were already
   landed; there is nothing to do for them.)
10. **Phase 3 starts with two designs for review** -- node identity (TRIAGE's D1) and the shared model context (D5) --
    and no port code until they are ruled on.

Reported to the user at the phase's end and not ruled on separately: step 5's oracle hash over raw UTF-16 units, its
unreachable overlay path at the `document` door and its 6 warm-JVM mismatches; 6a's dropped question-hash column; 6b's
keeping `PlatformSurfaceGuardrailTest` and `TextSurgeryPinTest`'s witness-prefix test.

**Ruled after the post-6b fixes (the user, 2026-11-09), one at a time on the fixes' report:**

11. **The inputs legend-engine accepts and lite answered with a raw JDK exception are refused with a placed parse
    error** (not as recommended, which was to accept them): rows 3 (an impossible date with a zone), 7 (`1e999` at lite's
    and the platform's dialects), 8's rest (`1.ed`, `1.e+d`, `1.ef`, a huge exponent at those dialects), 46 (an empty
    `###Diagram` before another header), 50's rest (a non-string `title`, `description` or `required`, an integer past a
    long), 72 (an AggregationAware main mapping's local property) and 73's rest (`platform: Default #{ x: 1; }#`). Lite
    is deliberately stricter on these; each is its own row of `docs/SEMANTICS_REGISTER.md`.
12. **The six `UnsupportedOperationException` rows the list of 21 missed: only where the engine refuses** (not as
    recommended, which included the rest). Rows 70, 71, 81 and 69's positional cast and wildcard relation columns become
    placed parse errors at the engine's place. Row 68 and 69's `let` forms, which the engine accepts, keep today's raw
    answer, listed; row 74's raw answer was not reached by any probe and stays.
13. **Row 63 is refused at the model door at the engine's dialect and at the `lambda` door**, as the engine refuses it;
    `pmcd` at lite's and the platform's dialects keeps accepting. If lite's own tests rely on the `lambda` door
    accepting it, that comes back to the user.
14. **The 23 register rows S128 to S150 are ratified as they stand**, the four that differ from the engine past the
    line (PARITY rows 63, 73, 76, 83) included.

**Ruled on what rulings 11 to 13 found (the user, 2026-11-09):**

15. **Row 74 becomes a placed parse error at the engine's place.** Ruling 12 left it unchanged on the premise that no
    input reaches its raw answer; that premise was wrong (`EqualToJson #{ expected: ['a']; }#` reaches it at the
    `document` door), and the engine refuses each input.
16. **Lite is never looser than the engine at its dialect, on the differences the probes found.** Refused at
    `LEGEND_ENGINE`, the engine-JSON doors and the `lambda` door, at the engine's place: row 63's arguments at `spec` and
    `block`; the dated path segments `c(a::E.X)`, `c(1,)`, `c(,1)`, `c(%10:10)`; persistence's `platform: Default #{ }#`
    and `AwsGlue #{ dataProcessingUnits: 1; }#`. `^a::B(1)(x=1)`, which the engine accepts, stays refused, a register row.
    These refusals move to the engine's exact place (beyond the recommendation, which ledgered them): `^a::B($x, $y)`,
    `^a::B(1, 2)`, `^a::B('a'+'b')`, and row 63's refusal comes after a later syntax error in its section, as the engine
    reports them.

Ruling 11 covers `1.ed`, `1.e+d`, `1.ef` at every dialect; the brief carrying it out wrongly limited them to lite's and
the platform's, a mistake corrected with these.

17. **The gap from the engine is measured once before anything more is fixed** (the user, 2026-11-09). Each round of
    fixes found more differences near the last; the corpus and the mutated texts go through both parsers at the engine's
    dialect, read-only, counting disagreements on accept/refuse and on place by grammar, and how far to go is ruled on
    from that number (`docs/rust-port/ENGINE_GAP_2026_11_09.md`). The differences rulings 15 and 16's probes listed as
    unchanged stay as they are until then.
18. **On that number: never looser, plus the end of input** (the user, 2026-11-09). Every input lite accepts and the
    engine refuses at the engine's dialect becomes a refusal at the engine's place, but where the user has ruled it
    deliberately looser (S64's TIME columns and their kin). A refusal at the end of input lands at the engine's place,
    about 17,000 of the 40,000 place differences and the case an editor meets while someone is typing. The inputs on
    which lite is stricter and the remaining place differences are ledgered by family, not fixed. The measurement is
    re-run to confirm.

**Ruled on what ruling 18 left (the user, 2026-11-10):**

19. **Two looser cases stay looser, deliberately** (not as recommended, which refused them): a Diagram's m2 argument
    list (`Diagram X(width=.., height=..)`, the reviewed "DEFENDED" rows of `docs/refusal-allowlist.tsv`) and a mapping
    test with no `data` (`ElementParserTest.mappingTestSuitesParsedOntoTheRecord`, `...InteriorBracesBalanced`). Each is a
    register row ruled deliberate; nothing else changes.
20. **The 34 other looser texts are fixed before `main` is merged**; the end-of-input remainder (6,088 mutated texts at
    `pmcd`, 67 corpus) is ledgered as a family, not chased.
21. **Ruling 18's own choices are ratified**: the four lowered counts (all from `relationMappingSetup.pure`, which the
    pinned engine refuses), the removed allowlist row for it, the 7 `LENIENT_RESIDUE` rows deleted as now matched, the
    744-row difference list kept as plain text, S157 and S158 refused at the `document` door only; `block` is measured
    in the next round.

**Ruled on what rulings 19 to 21 left (the user, 2026-11-10):**

22. **A lone `\r` ends a line comment at every dialect**, as both upstream lexers do; the lexer stays dialect-free.
23. **The parser reports several errors where it plausibly can** (the user's question). It recovers at element and
    section boundaries only; the first error stays byte-identical to the engine's, so the engine-compatible routes, the
    frozen oracle and the parity gates see no change; the others appear only where lite shapes the answer
    (`compileErrors`, the editor's diagnostics); an error that leaves braces open is reported alone. Errors are sorted by
    place, which settles the order question of the rulings 19 to 21 report (a rule run after lite's own reading). Built
    right after `main` is merged, before any phase 3 code.
24. **An empty or blank code block is refused in lite's words, at the place an expression was expected** (not as
    recommended, which left it unplaced): not the engine's `[2:1]`, which is a line of its own wrapper.
25. **A HostedService's `postValidations` and `testSuites` keep their check**; that lite's record and JSON omit both
    lists, where the engine keeps them, becomes a register row. Carrying them through is left for later.
26. **The three looser inputs found by hand** (`auditing: None {}`, a constraint keyword as a column spec such as
    `[~owner: abc]`, DataQuality's unprobed comma lists) **are fixed in the multi-error round**, under the never-looser
    rule.
