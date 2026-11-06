# Front-end parity: the parser's doors (2026-11-05)

`lite-front`'s expression parser (docs/rust-port/PHASE2.md §4 step 3) and the first part of its model parser (step 4a),
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
| `pmcd` (`PmcdParser.parse`) | LEGEND_ENGINE | 23,258 | 5,930 | 18,044 | 11,144 |
| | LEGEND_LITE | 24,823 | 4,365 | 18,024 | 11,164 |
| | LEGEND_PLATFORM | 26,373 | 2,815 | 17,933 | 11,255 |

22,151 + 876 + 5 = 23,032 expression questions, all identical. The errors include the oracle's one
`java.time.DateTimeException` (a time-zone shift of an impossible date, below).

The model door: 54,001 of its 87,564 questions identical (the floor, `tests/pmcd-floor.txt`). Every one of the 33,563
that differ is a text whose walk reaches a section grammar not ported yet, which Rust answers `not ported` (no Java
class) where Java's walk would read its first element: by grammar, Mapping 22,226, Connection 4,141, Service 1,946,
Persistence 1,041, DataQualityValidation 699, DataSpace 642, ExternalFormat 592, ServiceStore 438, Diagram 438,
Runtime 278, QueryPostProcessor 234, Snowflake 141, Elasticsearch 135, MongoDB 126, FileGeneration 114, HostedService
90, Deephaven 90, FunctionJar 66, GenerationSpecification 54, MemSql 45, BigQuery 27 (a platform `###Pure` section's
headerless Mapping, Runtime, Service or connection counts as its grammar's). A text whose error
Java raises before such a section is identical already. The questions were answered with one overlay grammar
registered (the test jar's `ToySectionGrammar`); the differential plays the shim's part for it (`toy_splice`).

The oracle's spec texts reach none of the GraphQL island, the quoted-tree fold, the quoted-grammar fold or a dated
path segment. Those paths were held to Java on 949 further questions (321 texts: islands of every kind, path literals,
graph-fetch trees, caret specials, annotations, column specs, operator mixes, number and date edge cases, 300-deep
nesting, non-BMP error columns), each answered by `//core:front_show` and by Rust: all identical. That set is not
committed; the model door's questions reach these paths through function bodies (the quoted-grammar fold: 40 texts).

## What is ported

| Rust | Java |
|---|---|
| `spec_parser.rs` | `SpecParser`, `NumberLiterals`, `IslandScan`, `QuotedSpecParser` (`fold`, `foldGrammar`, `foldStringConcat`, `parseTree`), `protocol.CaretSpecials`, `TdsLiteral.of`, `AppliedFunction.tableReference`, `IqProfile.named`/`unknown` |
| `cursor.rs` | `TokenStreamCursor` (the primitives, the identifier and type grammars, `spanOf`, the `'''` layout, documentation, `parseDecorations`, `once`, `isCleanSheetBody`, `parseFunctionDescriptor`, `validateJoinType`, `stripDoubleQuotes`), `ElementParser.at`/`parseStereotypes`/`parseTaggedValues`/`looksLikeTaggedValueBlock` |
| `model_parser.rs` | `PmcdParser.parse` (the section walk, `requireCleanHeader`, `sectionSpan`, `sectionImports`, the head rules, `strictWalk`, `parseOneAt`, `registered`, `ruleGroup`) |
| `element_parser.rs` | `ElementParser`'s `###Pure` elements: Class (properties, qualified properties, constraints, type and multiplicity parameters, type variables, projections), Enum, Profile, Association and its projection, Measure, function (its constraints and test block), `native` declarations, `Primitive`, `skipTopLevelNonElement` |
| `mapping_parser.rs` | `MappingProtocolParser.parseData` and its embedded values (`readIsland`, `parseEmbeddedValue`, `parseRelationElements`, `parseModelEntry`, `parseExternalFormat`, `parseRelationalCsv`), `ServiceStubDataParser` |
| `database_parser.rs` | `DatabaseProtocolParser` (`###Relational`: `parseDatabase`, `parseBody`, `parseSchema`, `parseTable`, `parseTabularFunction`, `parseDbType`, `parseMilestoning`, `validateMilestoningKeys`, `parseView`; the operation grammar `parseOperation`, `parseComparison`, `parseAtom`, `parseJoinNav`, `parseFunctionArgument`, `looksLikeDbPointer`, `bracketLeadsToAt`, `under`; the Mapping grammar's doors `operationAt`, `scopedOperationAt`) |
| `sections.rs` | `SectionParse` (`head`, `stringValue`), `SectionImports.parseImport`, `TextSectionGrammar` |
| `protocol.rs` | `Protocol.splitFqn`, `PFunction.mangledName` / `functionPath`, `Realization.of`, `ConstraintDefinition.mentionsReturn` (over `ValueSpecification.children`) |
| `lexer.rs` (added) | `TokenStream.slice`, `sectionContentLine`, `lexRange`'s shared line index |
| `operator_parts.rs` | `OperatorParts`, `EngineQuirks.RELATIONAL_ARITH_MISASSOCIATION` |
| `gql.rs` | `GqlParser` |
| `temporal.rs` | `PureDateLiteral.parse` (with the `java.time` arithmetic of its shift), `PureTimeLiteral.parse` |
| `escapes.rs` | `protocol.Escapes.unescapeJavaLike`, `Protocol.unquotePath` |
| `jnum.rs`, `jtypes.rs` | `Character.digit`, `Long.parseLong`, `Integer.parseInt`, `Double.parseDouble`, `new BigDecimal(String)`, `BigDecimal.compareTo` |
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
| 23 | an element ending at the stream's end where Java reads one token past it (`parseTypeVariableDeclarations`, `parseEmbeddedValue`, `expectText`, a stub's content pattern) | that token's text is `""` | the lexer's arrays carry zeroed slack past `count` (`Cursor::slack_text`) |

## What Java does that this does not reproduce

The section grammars not ported yet (step 4's later parts): Mapping (`MappingProtocolParser`), Connection, Runtime, Service (and the platform's headerless Service), the function
activators (Snowflake, MemSql, BigQuery, HostedService, FunctionJar), Diagram, and every tail grammar but Text. Each
answers `not ported` (no Java class) where the walk reaches it, so its questions differ visibly.

An overlay grammar (a `com.legend.spi.SectionGrammar` Java's `ServiceLoader` finds) is not called by Rust: the door
answers each overlay section's text and place (`model_parser::OverlaySection`), and the shim calls the grammar and
splices its `POpaqueElement`s, as Java's `OverlayElementSink` does (docs/rust-port/PHASE2.md §3). An overlay named as
a built-in or tail section never reaches the door's overlay path, as in Java's walk (Java would then cast it to the
built-in grammar's class and fail; no overlay does).
