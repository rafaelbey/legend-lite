# Front-end parity: the parser's doors (2026-11-05)

`lite-front`'s expression parser (docs/rust-port/PHASE2.md §4 step 3) and its model parser so far (steps 4a to 4c),
held to core/'s `com.legend.parser.SpecParser` and `PmcdParser` on the front oracle (`//rust/oracles:front_questions`,
Java's answers `//core:front_answers`): every question of the four doors, in the canonical form Java's
`FrontAnswers.canonical` writes (the records in the lossless codec, or the error's class, line, column and message),
compared by SHA-256 in `tests/front_parser_differential.rs`. The lexer under them is held separately
(`tests/front_lexer_differential.rs`, every text's token stream).

## The numbers

| door (Java) | dialect | answers | errors | identical | different |
|---|---|---|---|---|---|
| `spec` (`SpecParser.parse`) | LEGEND_LITE | 21,830 | 8 | 21,838 | 0 |
| | LEGEND_PLATFORM | 237 | 42 | 279 | 0 |
| | LEGEND_ENGINE | 33 | 1 | 34 | 0 |
| `lambda` (`SpecParser.parseLambda`) | LEGEND_ENGINE | 870 | 6 | 876 | 0 |
| `block` (`SpecParser.parseCodeBlock`) | LEGEND_PLATFORM | 5 | 0 | 5 | 0 |
| `pmcd` (`PmcdParser.parse`) | LEGEND_ENGINE | 23,258 | 5,930 | 27,563 | 1,625 |
| | LEGEND_LITE | 24,823 | 4,365 | 27,560 | 1,628 |
| | LEGEND_PLATFORM | 26,373 | 2,815 | 27,559 | 1,629 |

22,151 + 876 + 5 = 23,032 expression questions, all identical. The errors include the oracle's one
`java.time.DateTimeException` (a time-zone shift of an impossible date, below).

The model door: 82,682 of its 87,564 questions identical (the floor, `tests/pmcd-floor.txt`; 65,486 after step 4b).
Every one of the 4,882 that differ is a text whose walk reaches a section grammar not ported yet, which Rust answers
`not ported` (no Java class) where Java's walk would read its first element: by the first such grammar, Persistence
1,302, DataQualityValidation 708, ExternalFormat 646, ServiceStore 504, Diagram 447, QueryPostProcessor 243, Snowflake
204, Elasticsearch 144, MongoDB 135, FileGeneration 120, HostedService 93, Deephaven 90, MemSql 69, FunctionJar 69,
GenerationSpecification 57, BigQuery 51 -- all step 4d's (a text's Connection, Runtime, Service or DataSpace section
before such a section now parses, so some of step 4b's counts grew). A text whose error Java raises before such a
section is identical already. The questions were answered with one overlay grammar registered (the test jar's
`ToySectionGrammar`); the differential plays the shim's part for it (`toy_splice`).

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

## What is ported

| Rust | Java |
|---|---|
| `spec_parser.rs` | `SpecParser`, `NumberLiterals`, `IslandScan`, `QuotedSpecParser` (`fold`, `foldGrammar`, `foldStringConcat`, `parseTree`), `protocol.CaretSpecials`, `TdsLiteral.of`, `AppliedFunction.tableReference`, `IqProfile.named`/`unknown` |
| `cursor.rs` | `TokenStreamCursor` (the primitives, the identifier and type grammars, `spanOf`, the `'''` layout, documentation, `parseDecorations`, `once`, `isCleanSheetBody`, `parseFunctionDescriptor`, `validateJoinType`, `stripDoubleQuotes`), `ElementParser.at`/`parseStereotypes`/`parseTaggedValues`/`looksLikeTaggedValueBlock`; the island re-lex cursors (`ConnectionSectionGrammar.Cursor`, `RuntimeSectionGrammar.SliceCursorOffset`: `IslandCursor`), `SpanOrigin.islandShift` |
| `model_parser.rs` | `PmcdParser.parse` (the section walk, `requireCleanHeader`, `sectionSpan`, `sectionImports`, the head rules, `strictWalk`, `parseOneAt`, `registered`, `ruleGroup`) |
| `element_parser.rs` | `ElementParser`'s `###Pure` elements: Class (properties, qualified properties, constraints, type and multiplicity parameters, type variables, projections), Enum, Profile, Association and its projection, Measure, function (its constraints and test block), `native` declarations, `Primitive`, `skipTopLevelNonElement` |
| `mapping_parser.rs` | `MappingProtocolParser.parseData` and its embedded values (`readIsland`, `parseEmbeddedValue`, `parseRelationElements`, `parseModelEntry`, `parseExternalFormat`, `parseRelationalCsv`), `ServiceStubDataParser` |
| `mapping_grammar.rs` | `MappingProtocolParser.parse` (`###Mapping`: `parseMapping`, `bodySection`, `parseMember` and its includes; the class mappings `parseRelationalClassMapping` (`clause`, `parseFilterMapping`, `parsePropertyLine`, `parseScopeBlock`, `parseOpInCtx`), `parsePureClassMapping`, `parseFunctionClassMapping`, `parseAggregationAware` (`parseNested`, `parseAggLambda`), `parseOperationClassMapping`, `parseRelationClassMapping` (`finishRelationClassMapping`, `relationBinding`), `parseServiceStoreClassMapping` (`parseTransformExpr`), `parseMongoDbClassMapping`; the association mappings `parseRelAssociationMapping`, `parseXStoreAssociationMapping`, `parseModelJoin`, `parseFunctionAssociationMapping`; `parseEnumerationMapping`), `ServiceLegacyMappingParser.parseTail`, `MappingOperation.byFunction`, `SetIds.of` |
| `mapping_tests.rs` | `MappingProtocolParser`'s tests: `parseTestSuite`, `parseMappingTest`, `parseStoreTestData`, `parseTestAssertion`/`assertionValue`, `parseLegacyTest` |
| `database_parser.rs` | `DatabaseProtocolParser` (`###Relational`: `parseDatabase`, `parseBody`, `parseSchema`, `parseTable`, `parseTabularFunction`, `parseDbType`, `parseMilestoning`, `validateMilestoningKeys`, `parseView`; the operation grammar `parseOperation`, `parseComparison`, `parseAtom`, `parseJoinNav`, `parseFunctionArgument`, `looksLikeDbPointer`, `bracketLeadsToAt`, `under`; the Mapping grammar's doors `operationAt`, `scopedOperationAt`) |
| `sections.rs` | `SectionParse` (`head`, `stringValue`, `booleanValue`, `specToSemicolon`, `lambdaToSemicolon`), `SectionImports.parseImport`, `TextSectionGrammar`, the tail grammars' table (`ElementwiseSectionGrammar.parseOne`/`qualifiedNameOf`) |
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
| `jnum.rs`, `jtypes.rs` | `Character.digit`, `Long.parseLong`, `Integer.parseInt`, `Double.parseDouble`, `new BigDecimal(String)`, `BigDecimal.compareTo`, `String.hashCode` and a `HashMap`'s key-set order, `Base64.getEncoder().encodeToString(s.getBytes(UTF_8))` |
| `nodes.rs` | the records' convenience constructors (what each shorter Java constructor fills) |

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

## What Java does that this does not reproduce

The section grammars not ported yet (step 4d's): the function activators (Snowflake, MemSql, BigQuery, HostedService,
FunctionJar), Diagram, and every tail grammar but Text, Service and DataSpace. Each answers `not ported` (no Java
class) where the walk reaches it, so its questions differ visibly.

The section grammars' SPI feed (`ElementwiseSectionGrammar.parse` and the Connection and Runtime grammars' own
`parse(SectionSource, ElementSink)`: a `SliceCursor` over the re-lexed section, emitting protocol JSON) is the overlay
host's, which the model door never takes: every built-in grammar is reached through the walk's `ElementParser`. Not
ported.

An overlay grammar (a `com.legend.spi.SectionGrammar` Java's `ServiceLoader` finds) is not called by Rust: the door
answers each overlay section's text and place (`model_parser::OverlaySection`), and the shim calls the grammar and
splices its `POpaqueElement`s, as Java's `OverlayElementSink` does (docs/rust-port/PHASE2.md §3). An overlay named as
a built-in or tail section never reaches the door's overlay path, as in Java's walk (Java would then cast it to the
built-in grammar's class and fail; no overlay does).

A column type's `toUpperCase(Locale.ROOT)` maps by Rust's Unicode tables, not the JDK's: they agree on every probe
(`'ınt'` is `INT`, `'straße'` `STRASSE`, `'ﬁxed'` `FIXED`, `'ǆx'` `ǄX`), and only a quoted type word reaches them (an
unquoted non-ASCII letter lexes as INVALID); a character the two Unicode versions map differently would differ.
