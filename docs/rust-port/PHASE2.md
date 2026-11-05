# Rust port — phase 2: the front end (2026-11-05, draft)

Phase 2 of [`ROADMAP.md`](ROADMAP.md): the lexer (1,418 lines) and the parser (24,884) move to Rust, with the
protocol emitter, and their Java is deleted. This file holds what was measured before any of it moved, the design
that follows, and the rulings it needs (§5). **Draft: no code is written until the user rules on §5.**

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
| `PmcdParser.parse`, `SpecParser.parse`, `parseLambda`, `parseCodeBlock`, `parseDocument`, `parseModel`, `lambdaJson` | **stay** as the shim: one Rust call each, through an interface in `com.legend.spi` (ArchUnit's invariant 7c allows the parser `spi`, not `ffi`) that `ServiceLoader` resolves to a provider in `com.legend.ffi`, as `RustDialects` did |
| `ParseException`, `Dialect`, `PmcdParser.Parsed`/`UnclaimedSection` | **stay** (vocabulary the callers use) |
| `Protocol` and `protocol.spec` records | **stay** until phase 3: the shim builds them from Rust's answer |
| `SectionGrammar` SPI, `SectionGrammarRegistry`'s overlay half, `OverlayElementSink` | **stay**: Java calls an overlay grammar on the section text Rust hands back |
| `ModelReader`, the `*Reader`s, `ModelComposer`/`PureComposer` | **stay**: they take JSON or records, downstream of the seam |
| `ProtocolEmitter` and its helpers | move (step 5), then deleted |
| `Lexer`, `TokenStream`, `ElementParser`, `SpecParser`'s body, `MappingProtocolParser`, `DatabaseProtocolParser`, `GqlParser`, `QuotedSpecParser`, the section grammars, `TokenStreamCursor`, `IslandScan`, `EngineQuirks`, ... | move, then deleted |
| `PreludeGenerator`'s cursor use | retargeted to the element spans Rust answers; held by regenerating the committed module byte for byte |
| `ide/ModelIndexer`, `ModelOrchestrator` | §5, ruling 3 |
| the Postgres-SQL translator (`protocol/sql`, not `iq/`) | §5, ruling 2: proposed for phase 3 |

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

**Java is the spec.** The span and error columns are reproduced as Java computes them (UTF-16 units and code points;
Rust works in UTF-8, so the conversion is Rust's), the `[L:C]` prefix, the eight `IllegalStateException`s, the four
failures `JsonModelDoorTest` pins. A difference found is a row of `rust/lite-front/PARITY.md`, as in phase 1.

## 4. Order

1. **The oracle and the receipts (2a).** The codec and its schema. Behind a system property, every Java entry point
   appends `(entry, dialect, spans, text, records or error)` during one `bazel test //...`, deduplicated; the
   `//parser-equivalence` corpus is added at all three dialects. Measured before deciding what is committed: the
   dump's size, the shared nodes, the record types reached against the schema. A pre-deletion receipt of the parse
   time (p50 over the stress corpus, the bootstrap's parse at class load), as phase 1's §4 step 1.
2. **`lite-front`'s lexer (2b)**: the tokens, offsets and columns as Java's, by a differential over the dump's texts.
3. **The expression parser (2c)**: `SpecParser` in Rust, byte parity on the dump's spec entries; then `SpecParser.parse`,
   `parseLambda` and `parseCodeBlock` **switch** -- every query the test suite runs is parsed by Rust while models are
   still parsed by Java. `QuotedSpecParser.fold` stays a Java record-to-record pass until step 4 (it re-enters
   `PmcdParser`), and `lambdaJson` goes through Java's `ProtocolEmitter` until step 5.
4. **The model parser (2d)**: `PmcdParser` and its section grammars in Rust, section by section to parity; then
   `PmcdParser.parse` **switches** (the bootstrap included), with overlays called back in Java.
5. **The engine JSON (2e)**: `ProtocolEmitter` in Rust; `parseDocument`, `parseModel` and `lambdaJson` switch, gated
   byte for byte by `CorpusSweepTest`'s 6,471 documents and `ModelReaderParityTest`.
6. **The deletion (2f)**: the Java lexer and parser deleted, `PreludeGenerator` retargeted, the tests of parser
   internals retired as rows of `rust/RETIRED_TESTS.md`, the guards that name the deleted classes moved with dated
   comments.

**Gates at every switch:** `bazel test //...` against `rust/BASELINE.tsv`; `//parser-equivalence:parser_parity` and
`:round_trip_proof` (they need the upstream checkouts); the live differential while the Java parser exists, and the
committed oracle after.

## 5. Rulings needed

1. **The seam.** Switch the parser's entry points to Rust in phase 2, over the lossless record codec (§3) --
   recommended: the untouched test suite then judges the Rust parser, and the Java parser is deleted in this phase.
   The alternative ports the parser and checks it against the oracle only, and switches the product path with phase 3,
   keeping the Java parser alive until then.
2. **The Postgres-SQL translator moves to phase 3**, beside the typer it runs inside (still iteration 1). In phase 2 it
   would need either a callback into the Java typer or a Java copy of its core for SAP IQ and Cypher.
3. **`ide/ModelIndexer` and `ModelOrchestrator`** read tokens and have no main caller: delete them with
   `ModelIndexerTest` and `ModelOrchestratorTest` as ledger rows, or port them onto Rust's tokens.
4. **The roadmap's gates**: replace "22,725 elements" with the live gates of §1.
