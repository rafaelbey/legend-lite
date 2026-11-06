# Front-end parity: the parser's doors (2026-11-05)

`lite-front`'s expression parser (docs/rust-port/PHASE2.md §4 step 3) and its model parser (steps 4a to 4d),
held to core/'s `com.legend.parser.SpecParser` and `PmcdParser` on the front oracle (`//rust/oracles:front_questions`,
Java's answers `//core:front_answers`, frozen since step 6a as `rust/oracles/front/answers.tsv.zst`, its
`MANIFEST.tsv`): every question of the four doors, in the canonical form Java's
`FrontAnswers.canonical` writes (the records in the lossless codec, or the error's class, line, column and message),
compared by SHA-256 in `tests/front_parser_differential.rs`. The lexer under them is held separately
(`tests/front_lexer_differential.rs`, every text's token stream). The protocol emitter (step 5) is held at the two
doors whose answer is legend-engine's JSON (`tests/front_engine_differential.rs`, below).

## The numbers

| door (Java) | dialect | answers | errors | identical | different |
|---|---|---|---|---|---|
| `spec` (`SpecParser.parse`) | LEGEND_LITE | 21,830 | 8 | 21,838 | 0 |
| | LEGEND_PLATFORM | 237 | 42 | 279 | 0 |
| | LEGEND_ENGINE | 33 | 1 | 34 | 0 |
| `lambda` (`SpecParser.parseLambda`) | LEGEND_ENGINE | 870 | 6 | 876 | 0 |
| `block` (`SpecParser.parseCodeBlock`) | LEGEND_PLATFORM | 5 | 0 | 5 | 0 |
| `pmcd` (`PmcdParser.parse`) | LEGEND_ENGINE | 23,258 | 5,930 | 29,188 | 0 |
| | LEGEND_LITE | 24,823 | 4,365 | 29,188 | 0 |
| | LEGEND_PLATFORM | 26,373 | 2,815 | 29,188 | 0 |

22,151 + 876 + 5 = 23,032 expression questions, all identical. The errors include the oracle's one
`java.time.DateTimeException` (a time-zone shift of an impossible date, below).

**The engine-JSON doors** (step 5, 2026-11-06): the protocol emitter (`emitter.rs` and its family, below), held to
core/'s `ProtocolEmitter` and `SourceInformation.withoutSpans` over Java's parser (`//core:front_engine_answers`,
frozen since step 6a as `rust/oracles/front/engine.tsv.zst`):
each question's two forms -- with its source information and without it -- as `FrontEngineAnswers.forms` writes
them, hashed as their UTF-16 units each as it is (a lone surrogate, which the emitter writes raw, as itself).

| door (Java) | answers | errors | identical (both forms) | different |
|---|---|---|---|---|
| `document` (`PmcdParser.parseDocument`, a model text at the engine's dialect) | 23,257 | 5,931 | 29,188 | 0 |
| `lambdaJson` (`SpecParser.lambdaJson`) | 870 | 6 | 876 | 0 |

The floors (`tests/document-floor.txt`, `tests/lambdaJson-floor.txt`) are the whole sets. `document`'s errors are the
model door's 5,930 at the engine's dialect and the emitter's one refusal of a model the parser accepts (row 63).

Beyond the oracle, none of it committed: **mutations** -- 149,870 questions from 6,000 of the oracle's model texts at
the engine's dialect and its 876 lambdas, each truncated, a token deleted, inserted or swapped, a line doubled, a string
literal's content replaced (escapes, controls, quotes, lone surrogates, non-BMP characters) and a number replaced by an
edge spelling (46,242 of them answer JSON): all identical in both forms but 6 whose answer is an
`ArrayIndexOutOfBoundsException` (rows 29 and 55), which the bulk run's warm JVM answered with no message and a cold JVM
answers with Rust's message byte for byte; the tail fork's further 13,272 mutations of texts holding a tail section,
all identical. No mutation made Rust panic. **Probes**: 530 hand-written questions (450 `document`, 80 `lambdaJson`),
aimed by an `llvm-cov` line census of the emitter over the oracle, all identical in both forms. **Coverage** (region
lines of each module; the oracle's / the probes' alone / neither):

| module | lines | the oracle | the probes alone | neither |
|---|---|---|---|---|
| `emitter.rs` | 2,718 | 2,511 | 37 | 170 |
| `tail_emitter.rs` | 1,971 | 1,881 | 27 | 63 |
| `mapping_emitter.rs` | 1,234 | 1,124 | 17 | 93 |
| `connection_emitters.rs` | 494 | 457 | 15 | 22 |
| `gql_emitter.rs` | 264 | 264 | 0 | 0 |
| `source_information.rs` | 164 | 156 | 0 | 8 |

The oracle reaches every `Element` variant an engine-dialect text can make -- all but the four lite-only ones
(`PNativeFunction`, `PPrimitive`, `PAssociationProjection`, `POpaqueElement`), whose texts the parser refuses at that
dialect (`Unsupported syntax`, `Primitive type declarations are not supported`, `projection associations are a
legend-lite-local form ...`; an overlay section `is not a known section parser`), so `liteOnly` is reached by no text.
The probes alone reach: a malformed `letFunction` call, a positional cast `^a::B($x)` (`NewInstanceCast`), wildcard
relation columns, the graph-fetch arguments with no rule (by kind), the let values with no span rule (`CTime`,
`RelationShape`), a multiplicity parameter in a property, a lambda or a function's signature (its mangle), a
`DeephavenApp`'s activation configuration, the persistence nodes and assertion entries with no type, the DataQuality
context and strategy refusals, the AggregationAware local property's null owner, the optional fields of the
connection specifications the oracle writes one way only. What no text reaches, each because the engine's dialect never
makes the shape: what only the JSON readers make (`origin`, `serializer`, an `sdlcInfo`, a typed variable reference, a
written `fControl` or `class`, a table pointer's written type, `PCNull`); what the parser refuses or drops at the
engine's dialect (type and multiplicity parameters, function types, a multiplicity parameter as a type argument, a
supertype's type arguments, a function's constraints, the IQ island's profile, a dotted key, `%latest` as a graph-fetch
argument, the quoted folds); `requireNonNull`s on fields the parser always fills (a deployment configuration's activation
connection, a unit's parameter, a date's written form, an enum's span); `withoutSpans`' arms for a span key whose value
is not an object.

The model door: all 87,564 of its questions identical (the floor, `tests/pmcd-floor.txt`; 82,682 after step 4c, 65,486
after step 4b): every section the walk reaches has its grammar, and no question answers `not ported` any more. The
questions were answered with one overlay grammar registered (the test jar's `ToySectionGrammar`); the differential
plays the shim's part for it (`toy_splice`).

The oracle's spec texts reach none of the GraphQL island, the quoted-tree fold, the quoted-grammar fold or a dated
path segment. Those paths were held to Java on 949 further questions (321 texts: islands of every kind, path literals,
graph-fetch trees, caret specials, annotations, column specs, operator mixes, number and date edge cases, 300-deep
nesting, non-BMP error columns), each answered by `//core:front_show` and by Rust: all identical. That set is not
committed; the model door's questions reach these paths through function bodies (the quoted-grammar fold: 40 texts).

The Mapping and Relational grammars (step 4b) were held to Java beyond the oracle, each set answered by
`//core:front_show` (its whole-file mode, Java's doors) and by Rust through the differential, at every dialect; none is
committed. **Derived**: the 2,928 distinct texts left when every section outside Pure, Mapping, Relational, Data and
Text is removed from the oracle's texts that reach one (8,784 questions, all identical; they reach what the oracle's
own texts mask behind a ServiceStore or Connection section: the legacy `~paramMapping` tail, `~request` blocks,
dataspace includes, `Binding` transformers). **Mutated**: 131,689 texts from Mapping and Relational texts, each truncated
at random offsets, a token deleted, inserted or swapped with its neighbour (395,067 questions, 363,482 of them errors):
all identical but where the mutation made a section not ported, or where Java answers an `ArrayIndexOutOfBoundsException`
(row 29). No mutation made Rust panic. **Probes**: 50 hand-written texts for paths neither reaches (`EqualTo`
assertions, the documented members of row 24, quoted scope headers, a ServiceStore multiplicity parameter, empty
aggregate lambdas, the relational `-` and bracket edges, quoted non-ASCII column types upper-cased, both sites of row
29): 150 questions, all identical.

The Connection, Runtime, Service and DataSpace grammars (step 4c) were held to Java beyond the oracle the same way, none
of it committed. **Derived**: the 323 distinct texts left when every section outside Pure, Mapping, Relational, Data,
Text, Connection, Runtime, Service and DataSpace is removed from the oracle's texts that reach one (969 questions, all
identical). **Mutated**: 171,348 texts from the 5,986 oracle and derived texts whose sections all are those (each cut
at random offsets, a token deleted, inserted or swapped, a line doubled for the once-only rules, all after the place
the first of this step's grammars starts): 514,044 questions, 463,926 of them errors, all identical; no mutation made
Rust panic. **Probes**: 425 hand-written texts (1,275 questions, all identical; the crate's unit tests pin six of them)
for what neither reaches: every
datasource specification and auth strategy with its refusals, the MongoDB, Elasticsearch and Deephaven islands
(AWS secrets with Default, Static and STSAssumeRole credentials, GCP WIF with an AWS IdP, ApiKey locations, EPK,
Kerberos and PSK, each refusal inside an island), the mapper keys' hash order (thirteen keys, past a resize),
`relationalMapper`'s line shift, local mode, a runtime's connection islands on its first line and later ones nested in
a service's embedded runtime, legacy-test folds at every depth, compact and keyed suites with `toBytes` (a lone
surrogate among them), post-validations, Multi executions and execution environments, every DataSpace key and
support-info form, the platform dialect's headerless Service, Runtime and connections, and each row below.

The last grammars (step 4d: the function activators, Diagram, and the tail grammars GenerationSpecification,
FileGeneration, Deephaven, MongoDB, DataQualityValidation, Elasticsearch, ExternalFormat, ServiceStore, Persistence and
QueryPostProcessor) were held to Java beyond the oracle the same way, none of it committed. **Derived**: 1,569 texts
from the 1,774 oracle texts that hold a 4d section -- each with every other section removed, each 4d section alone,
each of a 4d section's top-level elements alone with the section's imports (4,707 questions, all identical).
**Mutated**: 93,997 texts from the oracle and derived texts holding a 4d section, each cut at random offsets, a token
deleted, inserted or swapped, a line doubled, all after the first 4d section's header (281,991 questions, 258,950 of
them errors); and, since no grammar is left unported, 110,595 texts mutated anywhere from 4,000 of the oracle's model
texts sampled across every grammar (331,785 questions, 284,795 errors). All identical but 22 texts (66 questions) whose
answer is an `ArrayIndexOutOfBoundsException` (a persistence island open at the stream's end, row 55; a legacy mapping
test's last `assert`, row 29): the bulk run's warm JVM dropped its message, and each, asked again of a cold JVM, is
Rust's answer byte for byte. No mutation made Rust panic. The earlier steps' derived, mutated and probe sets (4b's and
4c's, 920,289 questions), whose mutations had made sections not ported, are now all identical but the same
`ArrayIndexOutOfBoundsException`s of row 29 (313 questions, each a cold JVM's answer). **Probes**: 463 hand-written
texts (1,389 questions, all identical; the crate's unit tests pin four of them), aimed by a line census of the new
modules over everything above (`cargo llvm-cov`): every bson type and every cast `schemaOf` makes of every JSON kind,
repeated JSON keys, a quoted value cut by a section boundary or the stream's end; every ServiceStore parameter rule;
each activator kind, deployment configuration and ownership, Deephaven's `..`; every DataQuality key and strategy and
the relation elements' test suites; Diagram's comments, m2 views at each dialect, imports, coordinates, an empty or
unterminated section, non-BMP columns; every persistence node kind the walker tables name, islands nested three deep,
each platform, the islands and slices open at the stream's end; FileGeneration's reserved names and config values;
JDK 25's `Double.parseDouble` wording over 34 number spellings. Lines no question reaches: the arms the walk cannot
enter (an activator kind the section does not admit, `PFunctionActivator`'s both-ownerships refusal), and the
hexadecimal path of `jnum::parse_double`, which no token reaches (no token starts with `0x`, a Diagram coordinate has
no `x`): unit-tested against the JDK's algorithm.

## What is ported

| Rust | Java |
|---|---|
| `spec_parser.rs` | `SpecParser`, `NumberLiterals`, `IslandScan`, `QuotedSpecParser` (`fold`, `foldGrammar`, `foldStringConcat`, `parseTree`), `protocol.CaretSpecials`, `TdsLiteral.of`, `AppliedFunction.tableReference`, `IqProfile.named`/`unknown` |
| `cursor.rs` | `TokenStreamCursor` (the primitives, the identifier and type grammars, `spanOf`, the `'''` layout, documentation, `parseDecorations`, `once`, `isCleanSheetBody`, `parseFunctionDescriptor`, `validateJoinType`, `stripDoubleQuotes`), `ElementParser.at`/`parseStereotypes`/`parseTaggedValues`/`looksLikeTaggedValueBlock`; the island re-lex cursors (`ConnectionSectionGrammar.Cursor`, `RuntimeSectionGrammar.SliceCursorOffset`: `IslandCursor`), `SpanOrigin.islandShift` |
| `model_parser.rs` | `PmcdParser.parse` (the section walk, `requireCleanHeader`, `sectionSpan`, `sectionImports`, the head rules, `strictWalk`, `parseOneAt` and its activator arm, `registered`, `ruleGroup`, `diagramElements`, `diagramImports`, `importPaths`) |
| `element_parser.rs` | `ElementParser`'s `###Pure` elements: Class (properties, qualified properties, constraints, type and multiplicity parameters, type variables, projections), Enum, Profile, Association and its projection, Measure, function (its constraints and test block), `native` declarations, `Primitive`, `skipTopLevelNonElement` |
| `mapping_parser.rs` | `MappingProtocolParser.parseData` and its embedded values (`readIsland`, `parseEmbeddedValue`, `parseRelationElements`, `parseModelEntry`, `parseExternalFormat`, `parseRelationalCsv`), `ServiceStubDataParser` |
| `mapping_grammar.rs` | `MappingProtocolParser.parse` (`###Mapping`: `parseMapping`, `bodySection`, `parseMember` and its includes; the class mappings `parseRelationalClassMapping` (`clause`, `parseFilterMapping`, `parsePropertyLine`, `parseScopeBlock`, `parseOpInCtx`), `parsePureClassMapping`, `parseFunctionClassMapping`, `parseAggregationAware` (`parseNested`, `parseAggLambda`), `parseOperationClassMapping`, `parseRelationClassMapping` (`finishRelationClassMapping`, `relationBinding`), `parseServiceStoreClassMapping` (`parseTransformExpr`), `parseMongoDbClassMapping`; the association mappings `parseRelAssociationMapping`, `parseXStoreAssociationMapping`, `parseModelJoin`, `parseFunctionAssociationMapping`; `parseEnumerationMapping`), `ServiceLegacyMappingParser.parseTail`, `MappingOperation.byFunction`, `SetIds.of` |
| `mapping_tests.rs` | `MappingProtocolParser`'s tests: `parseTestSuite`, `parseMappingTest`, `parseStoreTestData`, `parseTestAssertion`/`assertionValue`, `parseLegacyTest` |
| `database_parser.rs` | `DatabaseProtocolParser` (`###Relational`: `parseDatabase`, `parseBody`, `parseSchema`, `parseTable`, `parseTabularFunction`, `parseDbType`, `parseMilestoning`, `validateMilestoningKeys`, `parseView`; the operation grammar `parseOperation`, `parseComparison`, `parseAtom`, `parseJoinNav`, `parseFunctionArgument`, `looksLikeDbPointer`, `bracketLeadsToAt`, `under`; the Mapping grammar's doors `operationAt`, `scopedOperationAt`) |
| `sections.rs` | `SectionParse` (`head`, `stringValue`, `booleanValue`, `rawStringToken`, `lowerFirst`, `specToSemicolon`, `lambdaToSemicolon`), `SectionImports.parseImport`, `TextSectionGrammar`, the tail grammars' table (`ElementwiseSectionGrammar.parseOne`/`qualifiedNameOf`, every tail section), `LinkedHashMap.put` |
| `generation_parser.rs` | `GenerationSpecificationSectionGrammar` (`parseNodes`, `parsePointers`), `FileGenerationSectionGrammar` (`parseConfigValue`) |
| `deephaven_parser.rs` | `DeephavenSectionGrammar` (`parseStore`, `parseTable`; `DeephavenApp` through the activator grammar) |
| `mongodb_parser.rs` | `MongoDBSectionGrammar` (`parseCollection`, `parseSchemaIsland`, `parseJsonValue` into Java's JSON values, `schemaOf` and its casts) |
| `data_quality_parser.rs` | `DataQualityValidationSectionGrammar` (`parseValidation`, `parseTreeIsland`/`parseTreeNode`, `parseRelationValidation`, `parseChecks`, `parseTestSuites`, `parseRelationComparison`, `parseReconStrategy`) |
| `elasticsearch_parser.rs` | `ElasticsearchSectionGrammar` (`parseIndex`, `parseProperty`) |
| `external_format_parser.rs` | `ExternalFormatSectionGrammar` (`parseSchemaSet`, `parseSchemas`, `parseBinding`) |
| `service_store_parser.rs` | `ServiceStoreSectionGrammar` (`parseService`, `parseGroup`, `parseParameters`, `parseTypeRef`) |
| `persistence_parser.rs` | `PersistenceSectionGrammar` (`parsePersistence`, `parseNode`, `parseEntries`, `contentAnchoredIslandNode`, `validateNode` and the walker's tables, `wireSpans`, `withFlatDefaults`, `parseOutputTargets`, `parseNotifier`, `parseTests`, `parseTestBatches`, `parseContext`, `parseCtxParams`, `parseConnectionIsland`, `OffsetCursor` over `cursor::IslandCursor`), `TokenStreamCursor.parseQualifiedNameAdmittingBooleans`, `PPersistenceNode`'s compact constructor, `SpanOrigin.contentAnchored` |
| `query_post_processor_parser.rs` | `QueryPostProcessorSectionGrammar` (`schemaPointer`, `tablePointer`) |
| `activator_parser.rs` | `FunctionActivatorSectionGrammar.parseElement`/`parseDeploymentConfig` (the Snowflake, MemSql, BigQuery, HostedService and FunctionJar sections, Deephaven's app), `PFunctionActivator`'s compact constructor |
| `diagram_parser.rs` | `DiagramSectionGrammar.parseRaw` (`Raw`, the character walker; `parseDiagram`, `parseClassView`, `parsePropertyView`, `parseGeneralizationView`) |
| `connection_parser.rs` | `ConnectionSectionGrammar` (`###Connection`: `parseElement`, `parseIslandValue`, `parseValue`; the model, model-chain, relational (`parseDatasourceSpec` and its sixteen specifications, `parseAuthStrategy` and its eleven strategies, `parseMapperPostProcessors`, `parseOneMapper`), ServiceStore, Deephaven, MongoDB and Elasticsearch bodies; `islandKind`, `reLexIsland`, `authSpan`, `parseEsAuthIsland`, `parseUserPasswordBody`, `parseVaultSecret`, `parseAwsCredentials`), `SpanOrigin.overshootEnd`/`anchoredOvershoot` |
| `runtime_parser.rs` | `RuntimeSectionGrammar` (`###Runtime`: `parseElement`, `parseBodyKeys`, `parseConnections`, `parseEmbeddedConnection`, `parseEmbeddedBody`) |
| `service_parser.rs` | `ServiceSectionGrammar` (`###Service`: `parseElement`, `parseService`, `parsePostValidation`, `parseLegacyTest`/`parseLegacyBody`, `legacyParamValue`, `foldSignedLiterals` over `ValueSpecification.children`/`withChildren`, `byteArrayParam`, `parseTestSuites`, `parseCompactSuite`, `parseSuiteData`, `parseSuiteTests`, `parseExecution`, `parseKeyedBody`, `parseEmbeddedRuntime`, `parseQuery`, `parseExecutionEnvironment`), `MappingProtocolParser.parseEmbeddedValueAt`/`parseTestAssertionAt`/`parseDefaultAssertionAt`, `AppliedFunction.list` |
| `dataspace_parser.rs` | `DataSpaceSectionGrammar` (`###DataSpace`: `parseElement`, `parseContexts`, `parseExecutables`, `parseDiagrams`, `parseSupportInfo`, `parseSupportFull`, `parseLink`, `parseEmail`, `parseExpertise`, `parseOperationalMetadata`, `rawToSemicolon`, `rawIsland`), `RelationIslands.parseStandaloneRelationAt` |
| `jnet.rs` | `new java.net.URI(String)`'s verdict (JDK 25's `URI.Parser`) |
| `protocol.rs` | `Protocol.splitFqn`, `PFunction.mangledName` / `functionPath`, `Realization.of`, `ConstraintDefinition.mentionsReturn` (over `ValueSpecification.children`) |
| `lexer.rs` (added) | `TokenStream.slice`, `sectionContentLine`, `lexRange`'s shared line index |
| `operator_parts.rs` | `OperatorParts`, `EngineQuirks.RELATIONAL_ARITH_MISASSOCIATION` |
| `gql.rs` | `GqlParser` |
| `temporal.rs` | `PureDateLiteral.parse` (with the `java.time` arithmetic of its shift), `PureTimeLiteral.parse` |
| `escapes.rs` | `protocol.Escapes.unescapeJavaLike`, `Protocol.unquotePath` |
| `jnum.rs`, `jtypes.rs` | `Character.digit`, `Long.parseLong`, `Integer.parseInt`, `Double.parseDouble` (JDK 25's `FloatingDecimal.readJavaFormatString`, its hexadecimal form included), `new BigDecimal(String)`, `BigDecimal.compareTo`, `String.hashCode` and a `HashMap`'s key-set order, `Base64.getEncoder().encodeToString(s.getBytes(UTF_8))` |
| `ustr.rs` | `String.trim`, `strip`, `substring` (its JDK 25 refusal, `Range [b, e) out of bounds for length n`), `indexOf`, `lastIndexOf` |
| `nodes.rs` | the records' convenience constructors (what each shorter Java constructor fills) |
| `emitter.rs` (step 5) | `ProtocolEmitter` (`emit`, `emitElement`, `emitLambda`, `element` and every element of its own, `valueSpec`, `valueSpecWithSpan`, `appliedFunction`, `newInstance`, `pathLiteral`/`pathValue`/`shifted`/`islandSpan`, `graphFetch` and `gftParam`, `colSpec`, `collection`, `instancesCollection`/`dataInstance`/`dataLeaf`, `genericType`, `multiplicity`, `parseMultArg`, the constraints and qualified properties, `connectionValue`, `runtimeArrays`, `relOp`, `vaultSecret`, `str` as `Json.escapeTo(b, v, true)`, `srcInfo` and `finish`, `require`, `placeless`, `liteOnly`), `Realization.writtenBody`, `PDateTimeLit.wireType`, `PFunction.mangledName`'s refusals (`protocol.rs`) |
| `tail_emitter.rs` (step 5) | `TailEmitter` |
| `mapping_emitter.rs` (step 5) | `MappingEmitter`, `PRelOp.sourceInformation` |
| `connection_emitters.rs` (step 5) | `ConnectionEmitters`, `AuthSpecEmitter` |
| `gql_emitter.rs` (step 5) | `GqlEmitter` |
| `source_information.rs` (step 5) | `SourceInformation.withoutSpans` (`keeps`, `keptByTheEngine`, the text cut) |

Java's exceptions are `Err(JavaError)`: a `ParseException` at Java's position (its line, its column in code points,
the `[line:col] ` prefix), any other class by name and message. The places Java recovers from a parse error
(`tryTypedColSpec`, the quoted tree's `parseTree`, the quoted grammar's model parse) recover from a `ParseException`
alone, as Java's `catch (ParseException e)` does; any other error propagates, as in Java. Each door parses on a
512 MiB stack.

## Java's behaviour reproduced as it is

Each row is Java's answer, kept because the differential holds Java as the spec (invariant 4): none is a fix.

| # | input | Java's answer, reproduced | why it is odd |
|---|---|---|---|
| 1 | `1 < 2 + 3 * 4` | `lessThan(1, times[Collection[plus[Collection[2,3]], 4]])` | the engine's arithmetic mis-association after a comparison (`EngineQuirks.RELATIONAL_ARITH_MISASSOCIATION`, decided 2026-08-06) |
| 2 | any error after a non-ASCII character on its line | the column counts code points; every span column counts UTF-16 units | pinned as a known defect (`Utf16ColumnKnownDefectTest`) |
| 3 | `%2023-02-29T10:30+0100`, at every dialect | `java.time.DateTimeException: Invalid date 'February 29' as '2023' is not a leap year`, unplaced | the time-zone shift builds a `LocalDateTime` before the components are validated, so an impossible date with a zone escapes as java.time's exception rather than the parser's `invalid date literal` (without a zone: `%2024-02-30` is that `ParseException` at lite's and the platform's dialects, a date at the engine's) |
| 4 | `#/P/f(99999999999999999999)#`, a bad escape in a dated segment's string | the `ParseException` is placed at the token after the path literal (or the input's end) | `IslandScan` reports through the parser's cursor, already past the literal |
| 5 | `#/P/f(1 + )#` | the error's position is in the text `|1 +`, not in the source | a dated segment's argument is re-parsed as a fresh `"|" + arg` text |
| 6 | `@Varchar(99999999999999999999)` | `java.lang.NumberFormatException: For input string: "99999999999999999999"`, unplaced | `parseTypeVariableValues` calls `Long.parseLong` unguarded |
| 7 | `1e999` at lite's and the platform's dialects | `java.lang.NumberFormatException: Infinite or NaN` | `NumberLiterals.floating` compares against `BigDecimal.valueOf(Infinity)` |
| 8 | `1.5ed`, `1.5e+d` | `java.lang.NumberFormatException` with no message | the lexer ends a decimal at an empty exponent and `new BigDecimal("1.5e")` fails on an array bound |
| 9 | a call or stereotype name `'x\u12'::f()` | `java.lang.IllegalArgumentException: Less than 4 hex digits in unicode escape`, unplaced | `Protocol.unquotePath` decodes without a position; so does a `'''` literal's value (`docStringValue`); in a model, also a declared name (`splitFqn`) and a section's import |
| 10 | `#>{db::DB.T(}#` | `ParseException` at the island's start, its column in UTF-16 units | built from the span, not from `columnOf` |
| 11 | `#{Person{firm{->subType(@A){x}}}}#` | `ParseException` with line and column 0 | Java's one-argument constructor |
| 12 | a model store's instance at the engine's dialect (`parseInstanceData`) | `requireDataValue`'s refusal placed by the value token's span column (UTF-16 units) | as 10 |
| 13 | a token no declaration starts with (`Unexpected token 'x'`), or a quoted string before a declaration keyword (`Documentation must be ...`), in a section's walk | placed by the token's span column (UTF-16 units) | `PmcdParser.orphan` and the walk's documentation check use `startColumn`, not `columnOf` |
| 14 | `'''doc''' Class A {...}` beside an explicit `{doc.doc = '...'}` | `Element has both documentation and an explicit doc.doc tagged value. Use one.` at the tag's span start (UTF-16 units) | `taggedValuesWithDocumentation` takes the position from the tagged value's span |
| 15 | a section index entry after a non-ASCII character on its header's line or its last content line | the entry's columns count code points | `sectionSpan` uses `columnOf` where element spans use the token columns |
| 16 | documentation whose lines end in a bare CR (`'''\r doc\r'''`) | its span ends on its start line, at the literal's length | `isTextBlock` admits a CR after the opening `'''`, but `documentationSpan` counts `\n` alone (`length - lastIndexOf('\n') - 1`); a one-line literal never reaches it, refused as not a block |
| 17 | an unknown `###X` section | the engine's dialect refuses it at its content's first line, column 1; lite's at the section name | two refusals, one in `sectionElements`, one in `registered` |
| 18 | `Primitive 'a b'::T`, `native function 'a b'::f`, `Association A projects ...`, a function test's data `store:` or reference | the qualified name is kept as written, quotes included | these records take `parseQualifiedName` raw, where Class, Enum, Profile, Association, Measure and function names go through `splitFqn` (unquoted) and Text and Data names through `unquotePath` |
| 19 | `Class X(a:Integer[1])` type variables at the platform's dialect | each name is its token's raw text (a quoted name keeps its quotes) | `parseTypeVariableDeclarations` reads `text()`, not `parseIdentifier` |
| 20 | an extended constraint's `~enforcementLevel:` that names no level (a literal number, a call) | `constraint enforcementLevel must be Error or Warn, got 'null'` | `enforcementLevelName` answers null, concatenated |
| 21 | a function test's `Relation #{...}#` or `ModelStore #{...}#` data, and a `###Data` element's | two grammars: the test block's (`ElementParser`, cells trimmed by `trim`, rows one per line) and the data element's (`MappingProtocolParser`, cells by `strip`, a path line required) | the parser keeps both, each pinned to its own probe |
| 22 | a ServiceStore stub's `queryParameters` / `headerParameters` | sorted by name (`String.compareTo`), not as written | `PServiceStub`'s compact constructor sorts them (`byName`) |
| 23 | an element ending at the stream's end where Java reads one token past it (`parseTypeVariableDeclarations`, `parseEmbeddedValue`, `expectText`, a stub's content pattern, `~canAggregate`, an `EqualTo` island's key); a type read there (`looksLikeDbPointer`, an `Otherwise` target's lookahead) | that token's text is `""`, its type `STRING` | the lexer's arrays carry zeroed slack past `count`, and `STRING` is ordinal 0 (`Cursor::slack_text`, `ty_or_slack`) |
| 24 | documentation before a Database's `Join`, `Filter`, `MultiGrainFilter`, `include` or `TabularFunction` | the literal is stepped over and the keyword read as the member's name: `'''d''' Join j(...)` is `expected PAREN_OPEN but found VALID_STRING ('j')`, `'''d''' include db::X` an include of store type `include`; a tabular function is `unsupported Database member: '<the literal>'` | `parseBody`'s switch looks past a documentation literal, but only the schema, table and view arms read one |
| 25 | a relational string literal with a bad escape (`T.ID = 'a\u12'`) | `malformed quoted name: ...` placed at the token after the string | `parseAtom` advances before it decodes (as row 4) |
| 26 | a relational `-` not followed by an integer (`a: - x`, or `-` at the stream's end) | `java.lang.NumberFormatException: For input string: "x"` (`empty String` at the end), unplaced | `parseAtom` calls `Double.parseDouble(text())` before `expect(FLOAT)` |
| 27 | `scope([db]S.T)` in a relational class mapping | the schema segment is decoded, the table segment is its token's raw text (`'T'` keeps its quotes); a `"quoted"` segment is refused (`expected identifier, got QUOTED_STRING`) | `parseScopeBlock` names the table by `tokens.text(pos - 1)`, and reads segments with the Mapping cursor's `parseIdentifier`, which (unlike the Database cursor's) refuses a `"quoted"` name |
| 28 | a ServiceStore class mapping's local property with a multiplicity parameter (`+x: String[m];`) | `java.lang.ClassCastException: class com.legend.protocol.Multiplicity$Parameter cannot be cast to class com.legend.protocol.Multiplicity$Concrete (... are in unnamed module of loader 'app')`, unplaced | the property casts `parseMultiplicity()` to `Concrete` unchecked |
| 29 | a legacy mapping test's `assert` string as the stream's last token; a Relation class mapping's `~src` at the stream's end | `java.lang.ArrayIndexOutOfBoundsException: Index N out of bounds for length N` (N the token count), unplaced | `spanOf(aS, pos)` and `startLine(eS)` read the token positions past the end. A warm JVM answers the same exception with no message (HotSpot's fast throw: the bulk runs of the derived sets answer `null`); Rust answers a cold JVM's message |
| 30 | `~canAggregate yes`, `THRU_IS_INCLUSIVE = 1` | false, silently: `true` in any case is true, any other token false | `Boolean.parseBoolean` over the token's text |
| 31 | `EqualTo #{ }#`, an empty island where the assertion's key goes | `assertion key '<EOF>' is unbuilt`, unplaced (line and column 0) | `throwAt` on an empty token stream has no token to place the error at |
| 32 | an aggregate lambda with no token, whitespace before its `,` (`~groupByFunctions( ,)`) | `java.lang.IndexOutOfBoundsException: Range [s, e) out of bounds for length n`, unplaced (`~groupByFunctions(,)` is `aggregate lambda must be ONE expression`) | `parseAggLambda` appends the source from its first token's start to the previous token's end, an inverted range when the lambda is empty |

| 33 | a refusal inside a re-lexed island -- a runtime's embedded connection, a service's embedded runtime, an authentication island (`# PSK { key: 'k'; }#`) | `[1:7] unknown PSK key: key`: the line and column of the island's own text, line 1 its first; an empty island's refusal unplaced (`unsupported connection flavor: <EOF>`, `Embedded runtime must not be empty`, line and column 0) | the re-lex cursors shift spans (`SpanOrigin.islandShift`) but not errors: `error` is `throwAt` over the island's own stream |
| 34 | a table or schema mapper with unknown keys (`table { other: 'o'; from: 'a'; bogus: 'c'; to: 'b'; }`) | `unknown table mapper key: bogus`: the first in the order Java's `HashMap` iterates the keys (by hash bucket), not the first written | `parseOneMapper` checks `kv.keySet()` of a `HashMap` after the whole body is read |
| 35 | a service's embedded runtime island open at the stream's end (`runtime: #{`) | `java.lang.ArrayIndexOutOfBoundsException: Index N out of bounds for length N` (N the token count), unplaced -- a cold JVM's message (as row 29, a warm one may drop it) | `parseEmbeddedRuntime` reads `startLine(embStart)` before it expects the `}#` (the Runtime and Connection grammars' island scans expect it first: a `ParseException` there) |
| 36 | a Connection, Runtime, Service or DataSpace name whose quoted segment holds `::` (`Runtime 'a::b'`) | package `a`, name `b` | these grammars split `unquotePath(qn)` at its last `::`, where `###Pure` names go through `splitFqn` (row 18) |
| 37 | a legacy service test's parameter with a negative literal inside it (`[-99999999999999999999]`, `{x\|[-1]}`, `f(-1).p`) | the literal folded -- past a long, the `BigInteger`'s low 64 bits negated (`-7766279631452241919`) -- and each node above it rebuilt by `withChildren`: a lambda's and a property access's span dropped (`pos` null), a column spec's stereotypes and tagged values dropped, a column-spec array's span dropped. A parameter that is the literal alone (`-99999999999999999999`) is `Long.parseLong`'s `java.lang.NumberFormatException` instead | `foldSignedLiterals` negates `ci.value().longValue()` and rebuilds by identity through `ValueSpecification.withChildren`, whose shorter constructors fill those slots empty |
| 38 | an Elasticsearch connection's `clusterDetails: # URL { ... }#` | the text kept when `new java.net.URI(text)` parses it -- RFC 2396 with Java's deviations (`http://h:99999999999/` parses, its authority taken as registry-based) -- else `URL is not valid` at the island's first content token, its column in UTF-16 units | the verdict is `java.net.URI`'s (`jnet.rs`, JDK 25's `URI.Parser`); the error is built from the token's columns, not `columnOf` |
| 39 | `'''doc''' Service s::S { ... }` | `unsupported ###Service element: '''doc'''` | `parseService` reads no documentation; DataSpace's element does |
| 40 | a MemSql spec's `port: '٣٣٠٦'; useSsl: ' tRuE ';` | port 3306, useSsl true | `Long.valueOf` reads any Unicode decimal digit and `Boolean.valueOf` ignores case, each over the stripped string |
| 41 | a Deephaven or Elasticsearch connection's value span; an authentication island's span; a `relationalMapper`'s pointers | a value's end column the close's plus 4 (Deephaven's the close's own, unshifted inside an island); an auth island's end column its close's plus 3, an empty island's start its `}#`; a relational mapper's pointers moved up a line per line break between the keyword and its `{` | the engine's re-parse offsets (`SpanOrigin.overshootEnd`, `anchoredOvershoot`, the post-processor walker's keyword offset) |
| 42 | a key twice in a connection body | `Field 'x' should be specified only once`, placed at the cursor (Static, Snowflake, Spanner, Databricks, BigQuery, EmbeddedH2, DuckDB, SQLite, ServiceStore, Deephaven, MongoDB, a mapper post-processor, the auth strategies but SnowflakePublic, TrinoDelegatedKerberos and GCPWorkloadIdentityFederation, an island's keys at the island's cursor), at the `specification` or `auth` keyword (LocalH2, Redshift, Athena, Aurora, GlobalAurora, MemSql, Oracle, Trino and its ssl block, those three), at its kind (a `GenerationFeaturesConfig`), at the first key (Elasticsearch) or at the element (the relational and model connections' own keys) | each site anchors `once` where its engine walker's ctx is |
| 43 | a PSK island with more after its key (`# PSK { psk: 'k'; anything else }#`) | the key kept, the rest ignored | the engine's `pskAuthentication` rule has no EOF |
| 44 | a DataSpace executable's `id: 'a' b 3;` | `'a'b3` | the id is its raw token texts glued, a quoted one with its quotes (a digit-led `2Id` lexes as two tokens) |
| 45 | a `###Diagram` section | its spans and its refusals count columns in UTF-16 units from the section's first line (`/*😀*/Diagram d::D { }` starts at column 7; `/*😀*/Diagram d::D { other x }` is `[2:27] unknown diagram view 'other'`, 26 in code points) | the lexer raw-skips the section and its grammar walks the characters (`DiagramSectionGrammar.Raw`), a column a `char`, where every other refusal counts code points (row 2) |
| 46 | an empty `###Diagram` section with another header after it (`###Diagram\n###Pure\n...`) | `java.lang.StringIndexOutOfBoundsException: Range [11, 10) out of bounds for length 30`, unplaced | `diagramElements` takes `source.substring(contentStart, contentEnd)`, and an empty section's content ends one unit before it starts |
| 47 | a Diagram `import` | `import`, exactly one space, then `pkg(::pkg)*::*` of letters and digits (`Character.isLetterOrDigit`, any script), `_` and `$`, listed as written, the wildcard dropped, each once: `import\ta::*;` is no import (`expected 'Diagram'`), `import a.b::*;` is `malformed import 'a.b::*'` placed after its `;` | the raw grammar scans its own imports, not as `sectionImports` reads a lexed section's |
| 48 | an m2 view (`TypeView`, `AssociationView`, `PropertyView`, `GeneralizationView`) at the platform's dialect whose attribute block does not open with `(` (`TypeView t x`) | accepted: the one character (or quoted string) in the block's place consumed | `skipBalanced` is a do-while; at lite's and the engine's dialects the view is `Unexpected token 'TypeView'` |
| 49 | a Diagram coordinate (`position: (1-2, 3)`, `(١٢, 3)`, `(1.2.3, 4)`); a DataQuality comparison's `expectedMatch: 'x';` | `java.lang.NumberFormatException`, unplaced: `For input string: "1-2"`, `For input string: "١٢"`, `multiple points`, `For input string: "'x'"` (`empty String` at the stream's end) | `Raw.number` hands any run of `Character.isDigit` digits, signs, points and exponent letters, and `expectedMatch` any token, to `Double.parseDouble` unguarded, whose JDK 25 reader words a second point `multiple points` (the relational `-` of row 26 reads the same way: `- ..` is `multiple points`) |
| 50 | a MongoDB `jsonSchema` value of a JSON kind `schemaOf` casts otherwise (`{"properties": {"a": 1}}`, `{"required": [null]}`, `{"title": 1}`, a nested `"bsonType": null`) | `java.lang.ClassCastException: class java.lang.Long cannot be cast to class java.util.Map (java.lang.Long and java.util.Map are in module java.base of loader 'bootstrap')`, unplaced, a JSON `null` a bare `java.lang.Object`; a root `bsonType` that is not the string `object` (a number, `null`) `jsonSchema root must be an object` instead; an integer past a long `Long.valueOf`'s `NumberFormatException` | the island is walked into Java's own `LinkedHashMap`, `ArrayList`, `String`, `Long` and `Boolean` values and read back by unchecked casts |
| 51 | a key written twice in a MongoDB `jsonSchema` object or a FileGeneration config map (`{"title": "a", "description": "d", "title": "b"}`, `m: { a: 'x'; b: 'y'; a: 'z'; }`) | one entry, in its first place, with its last value (`title` `b`; `{a: z, b: y}`); a FileGeneration property written twice is kept twice | `LinkedHashMap.put`; the properties are a list |
| 52 | a `"` left open at the stream's end where a MongoDB string or a ServiceStore parameter name goes (`jsonSchema: {"a": "`) | `java.lang.StringIndexOutOfBoundsException: Range [1, 0) out of bounds for length 1`, unplaced | the quotes are cut by `raw.substring(1, raw.length() - 1)`, and the lexer's open double-quoted token is the `"` alone |
| 53 | a persistence node's keys (`NontemporalSnapshot { auditing: None; zz: 1; zz: 2; bb: 1; bb: 2; }`) | the entries sorted by key (`String.compareTo`, stable), and the walker's checks run in that order: `Field 'bb' should be specified only once`, at the node's span start | `PPersistenceNode`'s compact constructor sorts its entries |
| 54 | a persistence entry's identifier value (`x: A.'é';`, `x: A.'Σ';`, `table: TableA;`) | a leaf node (`A.é`, its span its first token's and one column past) when its first unit is upper case and some unit lower case (`Character.isUpperCase` and `isLowerCase` per UTF-16 unit, any script), it holds no `::` and the key is not `table` or `targetName`; else a string (`A.Σ`, `TableA`) | the grammar tells a CamelCase keyword from a name by its letters' case |
| 55 | a persistence island open at the stream's end (`persister: Batch #{`; a platform's, a sink connection's, a service parameter's, an output target's) | `java.lang.ArrayIndexOutOfBoundsException: Index N out of bounds for length N` (N the token count), unplaced -- a cold JVM's message, which a warm one may drop (as row 29) | each island arm reads `startLine(embStart)` before it expects the `}#` |
| 56 | a persistence test's `graphFetchPath:`, or a key-path list's `[#/a/b#,`, at the stream's end; a token there that is no path (`graphFetchPath: x;`) | `java.lang.IndexOutOfBoundsException: slice(N, N+1) out of bounds for count=N`, unplaced; `x` parsed as an expression, then `expected PATH_LITERAL but found VALID_STRING ('x')` | `SpecParser.parse(tokens.slice(pos, pos + 1))` runs before the `PATH_LITERAL` is expected |
| 57 | a refusal inside a persistence island (`persister: Batch #{ sink: Relational #{ database: 'a\u12'; }#; }#`) | placed in the innermost island's own coordinates (`[1:19] malformed quoted name: ...`); its spans shifted by the offsets composed through the nesting | as row 33: the re-lex cursor (`OffsetCursor`) shifts spans, not errors |
| 58 | an AwsGlue platform without `dataProcessingUnits` (`AwsGlue #{\n    other: 1;\n  }#`, `#{ other: 1; }#`, `#{ }#`) | `Field 'dataProcessingUnits' is required` at the island's first content token (the island's column added on its first line), or at the `}#`'s line, column 1, when the island is empty; a platform's span starts the line after `#{` even when its content shares that line, and ends one past the `}#` | the cloud extension parses the island as its own source (ANTLR's EOF position when empty); the persistence walker's span quirk |
| 59 | a function activator's `function:` pointer (`a :: 'b c' :: f ( String [ 1 ] , Integer [ * ] ) : String [ 0..1 ]`) | `a :: 'b c' :: f(String[1],Integer[*]):String[0..1]`: the name its source text with `unquotePath` over it (spaces kept), the signature its tokens glued with no separator; in a DeephavenApp a `..` in the signature is `Unexpected token '..'` at it | the walkers render `ctx.getText()`; the Deephaven grammar's multiplicity wants `DOT DOT`, which the lexer never makes |
| 60 | a Deephaven `DECIMAL(4294967297, 4294967298)` | precision 1, scale 2 | `(int) c.consumeLong()`, Java's narrowing cast |
| 61 | a deployment configuration in an activator section | a `SnowflakeAppDeploymentConfiguration` parsed and dropped (no element, no index entry); a BigQuery, MemSql or HostedService one an element whose index entry is `null` | the engine's walkers register no path for them (`PmcdParser.parseOneAt`) |
| 62 | a ServiceStore path parameter marked `required = false` | `Path parameters cannot be optional` at the parameter's span start (its column in UTF-16 units) | built from the span, as rows 10, 12 and 13 |

Rows 63 and on are the emitter's (step 5), at the engine-JSON doors: a model or lambda the parser accepts and the
emitter refuses, or writes in a way worth naming.

| # | input | Java's answer, reproduced | why it is odd |
|---|---|---|---|
| 63 | a graph-fetch argument that is not a literal, a dotted enum, a variable or a collection of them (`#{a::B{c(f())}}#`, `c(1.5d)`, `c(a::E)`, `c($x.y)`, `c(%10:10)`, `c({x\|1})`, `c(@Integer)`, `c(~a)`, ...) | `java.lang.UnsupportedOperationException: ProtocolEmitter has no rule for graph-fetch argument AppliedFunction — probe, do not guess.` (the node's simple name: `CDecimal`, `PackageableElementPtr`, `AppliedProperty`, `CTime`, `LambdaFunction`, `Named`, `ColSpec`, ...) | the oracle's one model the parser accepts and the emitter refuses (document `00dc41c7...`) |
| 64 | a lone surrogate in a string (`'a\uD800b'`), a `\u007F`, a ` ` | written raw in the JSON, not escaped: the answer holds an unpaired UTF-16 unit, so it is not valid UTF-8 | `Json.escapeTo` escapes `"`, `\` and the controls below U+0020 only. The shim carries such an answer as a JSON string (`lite_front_engine`'s `U` form) |
| 65 | `::`, the root package, as a value (`let p = ::`, `^a::B(package = ::)`) | the JSON literal `null` where the element pointer goes; a key expression bound to it written with no `expression` | the engine's serializer gets a Java null there (`NON_NULL` drops the key) |
| 66 | a graph-fetch tree (`#{a::B{c}}#`) | each tree node's `_type` written twice (`{"_type":"rootGraphFetchTree","_type":"rootGraphFetchTree",...}`), and kept twice by the no-spans form | the engine's Jackson writes the subtype id and the explicit property; `withoutSpans` cuts the text and never re-parses it |
| 67 | the no-spans form of an `equalTo`'s `expected`, a function or service test's `parameters[].value`, a legacy service test's `parametersValues[]` values, a persistence context's `serviceParameters[].value.primitiveType` | their spans kept | the engine parses those values with a span context of its own, which records whatever it was asked (`keptByTheEngine`) |
| 68 | `letFunction(1, 2)`, `letFunction('a')`, `letFunction('a', 1, 2)` written as a call | `ProtocolEmitter has no rule for malformed letFunction (at 2). Add the emit rule — do not drop it.` (the argument count) | the parser accepts a call named `letFunction`; the emitter reads one as a `let` |
| 69 | a positional cast `^a::B($x)`; a wildcard relation column (`@(a:?)`, `@Relation<(a:?)>`, `@(?:Integer)`); a let bound to a time or a relation shape (`let y = %10:10:10`, `let y = @(a:Integer)`) | `ProtocolEmitter has no rule for value specification NewInstanceCast — add the emit rule, do not drop it.`; `... no rule for wildcard @Relation columns (wire shape unprobed) — probe, do not guess.`; `ProtocolEmitter has no let-value span rule for CTime — probe, do not guess.` (`RelationShape`) | constructs the engine's parser takes and the emitter has no wire shape for |
| 70 | an undeclared multiplicity parameter: in a property (`p: String[m];`) or a lambda's parameter; in a function's signature (`function a::f(x:String[m]):String[1]`) | `ProtocolEmitter has no rule for a multiplicity PARAMETER — add the emit rule.`; a function's `no mangle rule for a multiplicity parameter`, raised after its body is written (a refusal in the body wins) | the engine's dialect refuses declared type and multiplicity parameters, not a bare name in brackets |
| 71 | a `DeephavenApp` with `activationConfiguration: a::K;` | `UnsupportedOperationException: activationConfiguration wire for DeephavenApp is unprobed` | only the Snowflake, MemSql and BigQuery activators have a configuration's wire |
| 72 | an AggregationAware mapping whose Pure `~mainMapping` has a local property (`+loc: String[1]: $src.x`) | `java.lang.NullPointerException` with no message | the main mapping's property mappings are copied onto the aggregation-aware node, and each one's owner class is `requireNonNull`ed; a local property has none |
| 73 | a persistence node kind, leaf or key the emitter's tables do not name (`persister: Foo {..}`, `zeta: Omega;`, `deduplication: Weird;` in a graph-fetch output, `temporality: Strange;` in a target, `platform: Default #{ x: 1; }#`) | `java.lang.IllegalStateException: unprobed persistence node: persister:Foo`, with ` (graphFetch)` or ` (target)` where it applies | the grammar's walker is generic; the emitter writes only the kinds it was probed for |
| 74 | a persistence assertion entry that is not a node or a string (`EqualToJson #{ expected: [a, b]; }#`) | `UnsupportedOperationException: TailEmitter has no wire rule for persistence assertion entry 'expected' (Strings) — probe, do not guess.` (`PathValue`, `NodeList`, `PathList`, `Pointer`) | as 73 |
| 75 | a persistence scalar written unquoted (`x: true;`, `x: 12;`) | the bare JSON literal when it is `true`, `false` or all `Character.isDigit` units; else a string | the emitter tells a literal from a name by its text |
| 76 | a DataQuality `context: fromMappingAndRuntime(a::M)` or `fromDataSpace(a::DS)` with no second argument; `context: fromSomewhere(a::B)`; `strategy: SHA256` | `java.lang.NullPointerException` with no message; `IllegalStateException: unmapped DataQuality context kind: fromSomewhere`; `IllegalStateException: unmapped comparison strategy: SHA256` | the grammar takes any context call and strategy name; the emitter maps a fixed set |
| 77 | a ServiceStore primitive type (`response: 'ΑΣ'`) | its `_type` the name through `toLowerCase(Locale.ROOT)`, the full mapping: `ας` (a final sigma), a lone surrogate kept | Java's locale-free full case mapping |
| 78 | a DataSpace executable with both `query:` and `executable:` | written as a template executable, the `executable` dropped | the emitter picks the shape by the query's presence |
| 79 | a Redshift specification; a Snowflake specification or `SnowflakePublic` auth synthesized by `mode: local`; a GCP workload identity federation with an AWS IdP | Redshift's without `sourceInformation` (the record holds none); Snowflake's and `snowflakePublic`'s span key dropped when the span is null; `"additionalGcpScopes":[]` always written | each the engine's own wire, probed |
| 80 | a GraphQL float (`#GQL{query { a(x: 1e400) }}#`, `1.5e10`) and string (`"a\"b"`) | the float spelled by `Double.toString`: `Infinity` as a bare token (not JSON), `1.5E10`; the string's escapes kept as written, so `"a\\\"b"` | the island's values are written from the parsed text without re-encoding |
| 81 | a path literal's dated segment whose argument is a variable (`#/a::B/c($x)#`) | `ProtocolEmitter has no rule for dated path segment with a non-%latest argument (at c). Add the emit rule — do not drop it.`; a literal, an enum, a collection or `%latest` there is written (`#/a::B/c(1)#`, `c('a')`, `c(a::E.X)`, `c([1, 2])`) | the parser marks the segment and keeps the path; the emitter writes the argument kinds it has spans for |

## What Java does that this does not reproduce

The emitter's callers over records that are not a text's (step 5): `PmcdParser.DocElement.json()`, the `*Reader` round
trips (`ModelReader.read(json)` emitted again), `PureV1Api`'s connection export, `ProtocolEmitterTest` -- they stay on
Java's `ProtocolEmitter` until phase 3. `emitter::emit_element` and the arms only JSON-read records reach (`origin`,
`serializer`, `sdlcInfo`, a typed variable reference, a written `fControl` or `class`, a table pointer's written type)
are ported, but no door calls them, and no differential holds them.

The section grammars' SPI feed (`ElementwiseSectionGrammar.parse`, the Connection, Runtime and function-activator
grammars' own `parse(SectionSource, ElementSink)`, a `SliceCursor` over the re-lexed section emitting protocol JSON, and
the Diagram grammar's `parse` and dialect-less `parseRaw(src)`) is the overlay host's, which the model door never takes:
every built-in grammar is reached through the walk. Not ported, by design.

An overlay grammar (a `com.legend.spi.SectionGrammar` Java's `ServiceLoader` finds) is not called by Rust: the door
answers each overlay section's text and place (`model_parser::OverlaySection`), and the shim calls the grammar and
splices its `POpaqueElement`s, as Java's `OverlayElementSink` does (docs/rust-port/PHASE2.md §3). An overlay named as
a built-in or tail section never reaches the door's overlay path, as in Java's walk (Java would then cast it to the
built-in grammar's class and fail; no overlay does).

`SectionParse.lowerFirst` (a FileGeneration's kind, `Character.toLowerCase` on its first unit) lowers ASCII alone: its
caller's kind is an identifier token, whose first unit is always ASCII (a letter, a digit, `_`, a quoted name's `'`).

A column type's `toUpperCase(Locale.ROOT)` maps by Rust's Unicode tables, not the JDK's: they agree on every probe
(`'ınt'` is `INT`, `'straße'` `STRASSE`, `'ﬁxed'` `FIXED`, `'ǆx'` `ǄX`), and only a quoted type word reaches them (an
unquoted non-ASCII letter lexes as INVALID); a character the two Unicode versions map differently would differ.
