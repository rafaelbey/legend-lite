# The parser's gap from legend-engine, measured (2026-11-09)

Ruling 17 of `docs/rust-port/PHASE2.md` section 6: three rounds of fixes each found more differences near the last, so the
gap is measured once before anything more is fixed. Read-only: no code, row, ratchet or floor moved. Accept/refuse and
place only; what two accepted texts answer was not compared.

## The number

At `pmcd`, the engine's dialect:

- **Accept or refuse.** Mutated texts: 99.38% agree (943 of 153,008 differ, 586 lite looser and 357 lite stricter); **636
  of the 943 no row names**. Corpus: 97.90% (613 of 29,188, of which 571 are lite's own `TIME` columns, S64); **41 no row
  names**. `spec` over the corpus: 52 differ, **43 no row names**.
- **Place, where both refuse.** Mutated texts: 67.5% at the same line and column (87,633 of 129,783). Corpus: 52.7% at
  `pmcd`; at the expression doors 0.4%, because nearly all those texts are lite-only syntax (below).
- **Disagreements no row names, all kinds:** 40,944 of 153,008 mutated texts at `pmcd` (95% of its 43,093 disagreements),
  2,626 of 29,188 corpus texts at `pmcd`, 14,740 of 22,091 corpus texts at `spec`.

No text timed out, no door panicked, and no input text answered a raw `java.*` class (the two the rulings left, shown in
the harness set, are `lambdaJson` only).

## How it was run

- **The engine.** legend-engine 4.145.0's `PureGrammarParser` on parser-equivalence's class path (448 jars, every one
  resolved; no Bazel build was needed), Temurin 27, `-XX:-OmitStackTraceInFastThrow`. A model text goes to `parseModel`, an
  expression to `parseValueSpecification(text, "", 0, 0, true)` and `parseLambda`, a lambda to `parseLambda`. Each ask has a
  thread of its own with a 512 MiB stack and a 120-second limit. Accepted means the parser returned; the answer is not
  serialized.
- **Serial, not a thread pool.** The engine's answers are not reproducible under threads: with ten workers
  sharing nothing but the JVM, 635 of 153,008 mutated asks, 80 of 38,252 and 32 of 74,246 corpus asks differed from the serial
  answer in verdict or place, accept against refuse among them. The cause was not looked for. So one worker, one JVM per
  pass. Two serial passes are byte-identical (the mutated sets and the harness; the corpus but for one message holding an
  object's identity hash), and a shuffled order changes no verdict, class or place of 227,254 asks (one engine message,
  `Unexpected token` against `Unexpected token '{'. Valid alternatives: ...`, changes with the order).
- **Lite.** Commit `3a7f962b4`, product behaviour, `lite_front::doors` in a scratch Rust binary (no FFI), Rust 1.99.1:
  `answer_form("pmcd"|"spec", LEGEND_ENGINE)`, `answer_form("lambda")`, `engine_forms("document"|"lambdaJson")`, the
  first form. Identical at 10 and 3 threads, byte for byte.
- **Doors.** A model text: `pmcd`, `document`, each against `parseModel`. An expression text: `spec` against
  `parseValueSpecification`; `lambda`, `lambdaJson` against `parseLambda`. A lambda text: `lambda`, `lambdaJson`. The `block`
  door is not measured; the three texts asked only there are read as expressions.
- **Place.** As the previous probes (`verdict.py`): the engine's `sourceInformation.startLine` and `startColumn` against
  lite's error line and column, as each reports them, no normalisation. An engine answer without source information is its
  own category.
- **Categories**, per text and door: both accept; same line and column; same line, other column; different line; lite
  looser (lite accepts, the engine refuses); lite stricter; lite raw `java.*`; engine timeout; and one more, both refuse
  with the engine unplaced (a raw NPE or `NumberFormatException` from it). Each door's categories sum to its texts.
- **Where a difference is filed.** Model texts by the last `###Name` header at or before the engine's line (lite's when the
  engine accepts), `###Pure` if none; a mangled header (`###Pu`) goes in one bucket. Expressions by their leading construct.

**The inputs, pinned** (SHA-256 of the texts' UTF-16LE units, concatenated, then of the case file):

| set | what | texts | SHA-256 of the texts / of the file |
|---|---|---:|---|
| corpus | every text of `rust/oracles/front/questions.jsonl.zst` (`7cf6e558...`, MANIFEST's); 29,188 model, 22,091 expression, 876 lambda; 12 texts in two kinds | 52,143 texts, 52,155 cases | `ae16e0c92e94047474df7f090817d769d49601e6793264ee6f71588f403b058e` / `80c61fa7e96d0b660504396139a56360c0fd7df8df0ff9c73db2a6656bc677db` |
| mutated | the post-6b `fuzz` generator (seeded and deterministic, two runs byte-identical: xorshift, seed `0x9E3779B97F4A7C15`, its `mutate` and `JUNK`, every 3rd model text of at most 30,000 units in file order: 9,563 bases), 16 mutations each | 153,008 | `22deff99164fb9f65364e467efcb0f37d2085c12c01685bc175f50d31342a38f` / `04071d93d242cfd44b80e78060f6259953992efbf8d532f5fa8387dcbce15d51` |
| mutated, 4 per text | the same generator at its default, 4 each | 38,252 | `11342ac045af95bd028ae022864065032ebb2bb7142d3e9704a8d615a1d5a5c7` / `eb87edcf9c255075738936e4a5ea4fcbeed32caff7a539a6390b221632715b60` |
| harness | the 21 hand-written cases of the check below | 21 | `a718b34025cc8af271af94029f3f921abf34947f138a44b2914d65c3d505e581` / `5e8abe72de9170465aa3fded2841188c3d9535d1f7eb3987142e52136d988381` |

The "153,008 mutated texts" of `PHASE2.md` was a count of door answers: the fuzz at its default makes 38,252 texts and asks
each at four doors. Sixteen per text makes 153,008 texts, which is the set used here; the 38,252 were run too and land at
the same rates (table below). The mutator cuts UTF-16 units, so a text can hold a lone surrogate; the case files write every
non-ASCII unit as `\uXXXX`, and both parsers read the same units. To re-run after phase 3, from `docs/rust-port/engine-gap/`
(the sources name a scratch folder as they stood): `mk_cp.py` writes the class path from parser-equivalence's jar list
(`bazel-bin/parser-equivalence/engine_jars_exec.jars`, which needs that target built), `build.sh` builds the probes, then
`gen <questions.jsonl> 16`, `inputs.py`, `serial.sh`, `doors`, `classify.py`, `report.py`, `mkdoc.py`.

## By door and category

| set | door | texts | both accept | same place | same line, other col | different line | lite looser | lite stricter | raw `java.*` | engine timeout | engine unplaced |
|---|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| corpus | `pmcd` | 29,188 | 22,664 | 3,113 | 1,721 | 1,077 | 593 | 20 | 0 | 0 | 0 |
| corpus | `document` | 29,188 | 22,664 | 3,113 | 1,721 | 1,077 | 593 | 20 | 0 | 0 | 0 |
| corpus | `spec` | 22,091 | 7,282 | 59 | 11,262 | 3,417 | 49 | 3 | 0 | 0 | 19 |
| corpus | `lambda` | 22,967 | 8,157 | 53 | 11,244 | 3,445 | 44 | 5 | 0 | 0 | 19 |
| corpus | `lambdaJson` | 22,967 | 8,157 | 60 | 11,244 | 3,445 | 36 | 5 | 0 | 0 | 20 |
| mutated (16 per text) | `pmcd` | 153,008 | 22,282 | 87,633 | 27,256 | 14,894 | 586 | 357 | 0 | 0 | 0 |
| mutated (16 per text) | `document` | 153,008 | 22,282 | 87,650 | 27,260 | 14,894 | 565 | 357 | 0 | 0 | 0 |

The other input set, for the rates:

| set | door | texts | both accept | same place | same line, other col | different line | lite looser | lite stricter | raw `java.*` | engine timeout | engine unplaced |
|---|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| mutated (4 per text) | `pmcd` | 38,252 | 5,666 | 21,827 | 6,831 | 3,658 | 165 | 105 | 0 | 0 | 0 |
| mutated (4 per text) | `document` | 38,252 | 5,666 | 21,832 | 6,831 | 3,658 | 160 | 105 | 0 | 0 | 0 |

The corpus's expression texts were written for lite's dialect; at the engine's almost all are refused by both, and the
two then name different columns (the clusters below).

## Ledgered against unledgered

A disagreement is **ledgered** when a row of `docs/SEMANTICS_REGISTER.md` or `rust/lite-front/PARITY.md` already names it;
**H** when the row names this input and this difference from the engine, **M** when a PARITY row names lite's answer for this
input (Java's behaviour kept) and says nothing of the engine's. Everything else is unledgered, a row that names the words but
not the place included. The raw answers rulings 12 and 16 left are ledgered and appear only in the harness.

**How it was matched:** `engine-gap/ledger.py.txt`, an ordered list of rules, first match wins, each reading lite's words,
the engine's words, the door, and for a record door what the engine-JSON door answers for the same text (the rows' "the
record doors read as they did"). One shortest example per rule was read by hand. **Confidence:** high for H rules keyed on
words that are a row's own, or on the record/JSON door pair; medium for the island-coordinates rule (rows 33, 57: lite's
place must equal the engine's place translated into the island) and for M. A false negative is possible where a row
describes an input in words the rules do not look for; those count as unledgered, so the unledgered figure is an upper
bound by that margin. Three judgement calls are left unledgered and named here: `^a::B('a'+=1)` (S331 mentions `+=` only as
a place rule), `c(1)!n` (no row), `expected: 'a';` (S337's list does not name it).

Corpus:

| door | stricter H / M / unledgered | looser H / M / unledgered | other place H / M / unledgered | all unledgered |
|---|---|---|---|---:|
| `pmcd` | 1 / 0 / **19** | 571 / 0 / **22** | 82 / 131 / **2,585** | **2,626** |
| `document` | 1 / 0 / **19** | 571 / 0 / **22** | 82 / 131 / **2,585** | **2,626** |
| `spec` | 1 / 0 / **2** | 8 / 0 / **41** | 0 / 1 / **14,697** | **14,740** |
| `lambda` | 1 / 0 / **4** | 8 / 0 / **36** | 0 / 1 / **14,707** | **14,747** |
| `lambdaJson` | 1 / 0 / **4** | 0 / 0 / **36** | 1 / 1 / **14,707** | **14,747** |

Mutated (16 per text):

| door | stricter H / M / unledgered | looser H / M / unledgered | other place H / M / unledgered | all unledgered |
|---|---|---|---|---:|
| `pmcd` | 3 / 0 / **354** | 304 / 0 / **282** | 673 / 1,169 / **40,308** | **40,944** |
| `document` | 3 / 0 / **354** | 283 / 0 / **282** | 677 / 1,169 / **40,308** | **40,944** |

**By section, model texts at `pmcd`, corpus and mutated together** (`document` is the same but for 21 mutated texts `pmcd`
accepts and its engine-JSON refusals refuse: 17 at the engine's place, 4 ledgered):

| section | stricter led. / unled. | looser led. / unled. | other place led. / unled. | unledgered |
|---|---|---|---|---:|
| `###Pure` | 3 / 200 | 2 / 104 | 530 / 22,616 | **22,920** |
| `###Mapping` | 0 / 44 | 0 / 86 | 9 / 5,233 | **5,363** |
| `###Relational` | 0 / 5 | 854 / 11 | 20 / 3,845 | **3,861** |
| `###Connection` | 0 / 97 | 0 / 15 | 15 / 3,744 | **3,856** |
| `(mangled or unknown ###)` | 0 / 0 | 0 / 0 | 849 / 1,910 | **1,910** |
| `###Service` | 0 / 3 | 0 / 20 | 415 / 1,462 | **1,485** |
| `###Runtime` | 0 / 11 | 0 / 11 | 62 / 774 | **796** |
| `###DataSpace` | 0 / 0 | 0 / 6 | 0 / 732 | **738** |
| `###Persistence` | 0 / 0 | 14 / 13 | 88 / 688 | **701** |
| `###Diagram` | 0 / 0 | 0 / 4 | 2 / 509 | **513** |
| `###DataQualityValidation` | 0 / 13 | 5 / 0 | 2 / 284 | **297** |
| `###ExternalFormat` | 0 / 0 | 0 / 0 | 0 / 211 | **211** |
| `###Data` | 1 / 0 | 0 / 28 | 0 / 151 | **179** |
| `###ServiceStore` | 0 / 0 | 0 / 1 | 1 / 166 | **167** |
| `###Snowflake` | 0 / 0 | 0 / 1 | 0 / 108 | **109** |
| `###Elasticsearch` | 0 / 0 | 0 / 0 | 0 / 85 | **85** |
| `###MongoDB` | 0 / 0 | 0 / 0 | 0 / 60 | **60** |
| `###HostedService` | 0 / 0 | 0 / 1 | 0 / 57 | **58** |
| `###FunctionJar` | 0 / 0 | 0 / 0 | 0 / 54 | **54** |
| `###GenerationSpecification` | 0 / 0 | 0 / 1 | 0 / 44 | **45** |
| `###QueryPostProcessor` | 0 / 0 | 0 / 1 | 0 / 36 | **37** |
| `###MemSql` | 0 / 0 | 0 / 1 | 0 / 34 | **35** |
| `###BigQuery` | 0 / 0 | 0 / 0 | 0 / 29 | **29** |
| `###FileGeneration` | 0 / 0 | 0 / 0 | 0 / 24 | **24** |
| `###Text` | 0 / 0 | 0 / 0 | 0 / 24 | **24** |
| `###Deephaven` | 0 / 0 | 0 / 0 | 0 / 13 | **13** |
| `###AuthenticationDemo` | 0 / 0 | 0 / 0 | 62 / 0 | **0** |
| **total** | 4 / 373 | 875 / 304 | 2,055 / 42,893 | **43,570** |

**By construct, expression texts at `spec`, corpus** (`lambda` and `lambdaJson` are within a few of it):

| construct | stricter led. / unled. | looser led. / unled. | other place led. / unled. | unledgered |
|---|---|---|---|---:|
| `island #...` | 0 / 0 | 0 / 0 | 0 / 13,237 | **13,237** |
| `lambda |` | 0 / 1 | 0 / 1 | 0 / 1,425 | **1,427** |
| `relation accessor #>` | 0 / 0 | 0 / 28 | 0 / 0 | **28** |
| `lambda {` | 0 / 0 | 1 / 0 | 0 / 14 | **14** |
| `let` | 0 / 0 | 0 / 5 | 0 / 3 | **8** |
| `new instance ^` | 0 / 0 | 5 / 0 | 0 / 5 | **5** |
| `number or sign` | 0 / 0 | 0 / 0 | 0 / 5 | **5** |
| `name` | 0 / 0 | 0 / 4 | 0 / 1 | **5** |
| `type @` | 0 / 1 | 2 / 1 | 0 / 2 | **4** |
| `date %` | 1 / 0 | 0 / 1 | 0 / 1 | **2** |
| `variable $` | 0 / 0 | 0 / 0 | 0 / 2 | **2** |
| `column spec ~` | 0 / 0 | 0 / 1 | 0 / 1 | **2** |
| `call or param lambda` | 0 / 0 | 0 / 0 | 0 / 1 | **1** |
| `graph fetch #{` | 0 / 0 | 0 / 0 | 1 / 0 | **0** |
| **total** | 1 / 2 | 8 / 41 | 1 / 14,697 | **14,740** |

**What the ledgered rows are:**

| row | what | tier | kind | texts (`pmcd` + `spec`, corpus + mutated) |
|---|---|---|---|---:|
| P17 | unknown ### section | M | other place | 930 |
| S64 | TIME and TIMESTAMP_NS columns | H | lite looser | 854 |
| P33 | empty embedded runtime or connection, unplaced | H | other place | 384 |
| P13 | documentation string before a keyword | M | other place | 358 |
| P33 / P57 | refusal in an island's own coordinates | H | other place | 183 |
| S323 / P3 | impossible date with a zone | H | other place | 108 |
| S318 / P83 | #{ as the last token | H | other place | 71 |
| S315 / P73 | persistence node the emitter refuses, at a record door | H | lite looser | 13 |
| P11 | graph-fetch subtype error, unplaced | M | other place | 7 |
| S331 / P69 | positional cast at a record door | H | lite looser | 6 |
| P31 | EqualTo #{ }# unplaced | H | other place | 5 |
| S316 / P76 | DataQuality context at a record door | H | lite looser | 5 |
| P73 | platform Foo a token late | H | other place | 3 |
| S323 / P3 | impossible date with a zone | H | lite stricter | 3 |
| P47 | Diagram import | M | other place | 2 |
| S332 / P69 | spelled @Relation<(a:?)> at a record door | H | lite looser | 2 |
| S333 / P70 | multiplicity parameter at a record door | H | lite looser | 2 |
| P25 | relational string with a bad escape | M | other place | 2 |
| S96 | ModelStore's -9223372036854775808 | H | lite stricter | 1 |
| P62 | ServiceStore optional path parameter | M | other place | 1 |
| S314 / P63 | graph-fetch argument, column | H | other place | 1 |
| S337 / P74 | EqualToJson entries at a record door | H | lite looser | 1 |
| P38 | URL is not valid | M | other place | 1 |
| S325 / P8 | 1.ed, 1.e+d, 1.ef | H | lite stricter | 1 |

## The largest clusters (unledgered)

Counts are texts at `pmcd` (model) and `spec` (expression), corpus and mutated together. A shape is lite's words / the
engine's words, and where lite's place falls from the engine's.

| texts | shape (lite's words / the engine's words, where) | shortest example | lite | engine |
|---:|---|---|---|---|
| 10,787 | `unknown DSL island type: '_'` / `no such island parser`, lite later on the line | `#IQ{SELECT PATINDEX('%z%', 'abc')}#` | 1:36 | 1:34 |
| 5,056 | `expected <token> but found EOF` / `Unexpected token`, at end of input | `Class'` | 1:7 | 1:6 |
| 2,910 | `Unsupported syntax` / `Unexpected token`, lite earlier on the line | `native C~` | 1:1 | 1:8 |
| 2,191 | `unknown DSL island type: '_'` / `no such island parser`, lite on a later line | `#IQ{SELECT 1 -- + 100\n + 1}#` | 2:7 | 1:34 |
| 1,888 | `Unexpected token` / `Unexpected token`, at end of input | `^` | 1:1 | 1:2 |
| 1,867 | `expected <token> but found another` / `Unexpected token`, lite later on the line | `Class a::B extends a::G<Str}\n` | 1:28 | 1:24 |
| 1,696 | `Unexpected token` / `Unexpected token`, lite later on the line | `###]` | 1:4 | 1:1 |
| 1,490 | `Unsupported syntax` / `Unexpected token`, at end of input | `native` | 1:1 | 1:7 |
| 1,422 | `brace-less #TDS literal is not supported` / `Unexpected token`, lite on a later line | `#TDS\nid\n1\n#` | 4:2 | 1:1 |
| 1,238 | `Unexpected token` / `'_' is not a known section parser`, lite on an earlier line | `###Pur{ as }\n` | 1:7 | 2:8 |

What they come down to, a heuristic read off lite's words and the place (`engine-gap/report.py.txt`, `theme`):

| what the difference comes down to | `pmcd`, mutated | `pmcd`, corpus | `spec`, corpus |
|---|---:|---:|---:|
| the end of the input: where each places it | 17,009 | 180 | 7 |
| a lite-only island refused at the engine's dialect: the column | 112 | 33 | 13,419 |
| a construct lite refuses by name, at a token of its own | 11,417 | 1,914 | 5 |
| lite reads a token further (or fewer) before stopping | 5,807 | 106 | 8 |
| lite's walk stops at another token | 4,541 | 164 | 0 |
| brace-less #TDS: lite's place past the literal | 159 | 66 | 1,238 |
| other | 1,263 | 122 | 1 |
| lite refuses what the engine accepts | 354 | 19 | 2 |
| lite accepts what the engine refuses | 282 | 22 | 41 |
| the engine answers a raw exception, unplaced | 0 | 0 | 19 |

The verdict differences no row names, largest first.

Lite accepts, the engine refuses:

| texts | where | words | shortest example | place |
|---:|---|---|---|---|
| 91 | `###Pure` | `Unexpected token '<'.` | `Class a::B extends a::G<Strin-g>\n{\n}\n` | engine 1:24 |
| 66 | `###Mapping` | `Unexpected token '*'.` | `###Mapping\nimport *;\nMapping test::mapping2\n(\n  include mapping mapping1\n)\n` | engine 2:8 |
| 28 | `relation accessor #>` | `Unexpected token ':'.` | `#>{test::PersonDatabase.T_PERSON}#->navigate(~x: [1, 2, 3], {r, f \| $r.AGE == $f})` | engine 1:48 |
| 27 | `###Data` | `Unexpected token '{'` | `...  {\n          method: GET;\n          url: '/employees';\n        };\n      }\n    ]\n  }#\n}\n` | engine 7:8 |
| 17 | `###Service` | `Unexpected token '<EOF>'` | `...on: '';   execution: Single { query: my::funcs::peopleQuery; mapping: my::M; runtime: my::R; } }` | engine 3:115 |
| 15 | `###Connection` | `Unexpected token` | `...  type: DuckDB;\n  specification: DuckDB\n  {\n    path: '}#:memory:';\n  };\n  auth: Test;\n}\n` | engine 9:4 |
| 11 | `###Runtime` | `Unexpected token ']'.` | `...Runtime test::RT { mappings: [ ]; connections: [ store::DB: [ environment: store::Conn ] ,]; }\n` | engine 32:91 |
| 10 | `###Persistence` | `Unexpected token` | `... {\n      auditing: None;\n      fcates: false;\n      filterDuplicates: false;\n    }\n  }\n}\n` | engine 22:7 |

Lite refuses, the engine accepts:

| texts | where | words | shortest example | place |
|---:|---|---|---|---|
| 92 | `###Connection` | `expected <token> but found EOF` | `...tabaseConnection store::C { store: db::DB; type: DuckDB; specification: DuckDB { }; auth: Test; ` | lite 2:108 |
| 92 | `###Pure` | `expected a name, got another token` | `Class 1e999a::B\n{\n  x: Integer[0..1] = 5;\n}\n` | lite 1:7 |
| 46 | `###Pure` | `Unexpected token` | `###Pure\nfunction f::f(): Any[*]\n{\n  #>{(a::db.t}#->select(~query)\n}\n` | lite 4:3 |
| 19 | `###Mapping` | `expected a name, got another token` | `...apping::test\n(\n  mapping::SomeClass: XStore\n  {\n    p1[x, y]: true,\n    p2: false\n  }\n)\n` | lite 2:9 |
| 13 | `###Pure` | `expected '_' to close argument list` | `...lean[1]\n{\n  assert(\n      1$ == my::testMapping.classMappings->size(),\n      \|''\n    )\n}\n` | lite 5:8 |
| 13 | `###DataQualityValidation` | `Unexpected token` | `...ataset cannot be empty';\n     assertion: rel\|$rel->assertRelationNotEmpty();\n    }\n   ];\n}\n` | lite 4:12 |
| 9 | `###Runtime` | `expected a name, got another token` | `...H2\n          {\n          };\n          auth: DefaultH2;\n        }\n      }#\n    ]\n  ];\n}\n` | lite 10:5 |
| 9 | `###Pure` | `trailing tokens after code block: DOLLAR ('_')` | `###Pure\nfunction f::f(): Any[*]\n{\n  1$ - 2 * 3 - 4;\n}\n` | lite 4:4 |

**Within each section** (the three largest shapes; shortest example; sections with 100 unledgered texts or more):

**`###Pure`** (22,920 unledgered at `pmcd`):
- 2,910: `Unsupported syntax` / `Unexpected token`, lite earlier on the line: `native C~` (lite 1:1, engine 1:8)
- 1,914: `expected <token> but found EOF` / `Unexpected token`, at end of input: `Class'` (lite 1:7, engine 1:6)
- 1,490: `Unsupported syntax` / `Unexpected token`, at end of input: `native` (lite 1:1, engine 1:7)
**`###Mapping`** (5,363 unledgered at `pmcd`):
- 1,642: `expected <token> but found EOF` / `Unexpected token`, at end of input: `###Mapping\nMapping my::M ` (lite 2:14, engine 2:15)
- 337: `expected a name, got another token` / `Unexpected token`, at end of input: `###Mapping\nMapping a::1e999` (lite 2:12, engine 2:17)
- 309: `Missing table or alias for column '_'` / `Unexpected token`, at end of input: `###Mapping\nMapping a::m\n(\n  a::A: Relational\n  {\n    name: [a::db]1.5e` (lite 6:13, engine 6:21)
**`###Relational`** (3,861 unledgered at `pmcd`):
- 826: `unsupported column datatype` / `Unexpected token`, lite earlier on the line: `###Relational\nDatabase a::A (Table tb(id Inte\\u12` (lite 2:28, engine 2:32)
- 597: `expected <token> but found EOF` / `Unexpected token`, at end of input: `###Relational\nDatabase a::A\n` (lite 2:14, engine 3:1)
- 515: `unsupported column datatype` / `Unexpected token`, at end of input: `###Relational\nDatabase a::A(Table t(col VARC` (lite 2:27, engine 2:31)
**`###Connection`** (3,856 unledgered at `pmcd`):
- 807: `Unknown database type '_'` / `Unknown database type`, lite later on the line: `...ore::X\n{\n  store: store::S;\n  type: a;\n  specification: DuckDB {};\n  auth: Test;\n}\n` (lite 5:9, engine 5:3)
- 388: `expected <token> but found another` / `Unexpected token`, lite later on the line: `###Connection\nXmlModelConnection test::connection\n{\n  c{` (lite 4:4, engine 4:3)
- 336: `Unexpected token` / `Unexpected token`, lite earlier on the line: `###Connection\nDeep#{` (lite 2:1, engine 2:5)
**`(mangled or unknown ###)`** (1,910 unledgered at `pmcd`):
- 1,214: `Unexpected token` / `'_' is not a known section parser`, lite on an earlier line: `###Pur{ as }\n` (lite 1:7, engine 2:8)
- 475: `Unexpected token` / `'_' is not a known section parser`, at end of input: `###P(` (lite 1:5, engine 2:8)
- 219: `Unexpected token` / `Unexpected token`, lite later on the line: `###]` (lite 1:4, engine 1:1)
**`###Service`** (1,485 unledgered at `pmcd`):
- 324: `expected <token> but found another` / `Unexpected token`, lite later on the line: `###Service\nService meta::pure::myServiceSingle\n{\n  documentat]` (lite 4:13, engine 4:3)
- 226: `expected <token> but found EOF` / `Unexpected token`, at end of input: `###Service\nService test::Service\n{\n` (lite 3:2, engine 4:1)
- 122: `expected <token> but found EOF` / `Unexpected token`, lite later on the line: `###Service\nService demo::S\n{\n  patter` (lite 4:9, engine 4:3)
**`###Runtime`** (796 unledgered at `pmcd`):
- 293: `expected <token> but found another` / `Unexpected token`, lite later on the line: `###Runtime\nRuntime test\n{\n mappi,` (lite 4:7, engine 4:2)
- 172: `expected <token> but found EOF` / `Unexpected token`, at end of input: `###Runtime\nRuntime books::runtime::Python\n{\|` (lite 3:3, engine 3:2)
- 67: `unknown key` / `Unexpected token`, lite later on the line: `###Runtime\nRuntime a::r\n{\n  gs: [];\n}\n` (lite 4:7, engine 4:3)
**`###DataSpace`** (738 unledgered at `pmcd`):
- 247: `expected <token> but found another` / `Unexpected token`, lite later on the line: `###DataSpace\nDataSpace a::DS\n{\n  executio(` (lite 4:11, engine 4:3)
- 117: `expected <token> but found EOF` / `Unexpected token`, at end of input: `###DataSpace\nDataSpace model::dataSpace\n{\n` (lite 3:2, engine 4:1)
- 82: `expected <token> but found EOF` / `Unexpected token`, lite later on the line: `###DataSpace\nDataSpace a::DS\n{\n  exe` (lite 4:6, engine 4:3)
**`###Persistence`** (701 unledgered at `pmcd`):
- 162: `expected <token> but found another` / `Unexpected token`, lite later on the line: `###Persistence\nPersistenceContext test::TestPersistenceContext\n{\n  pe]` (lite 4:5, engine 4:3)
- 135: `expected <token> but found EOF` / `Unexpected token`, at end of input: `###Persistence\n\nPersistence test::TestPersistence ` (lite 3:34, engine 3:35)
- 62: `expected <token> but found EOF` / `Unexpected token`, lite later on the line: `###Persistence\nPersistence demo::P\n{\n  doc: 'd';\n  trigg` (lite 5:8, engine 5:3)
**`###Diagram`** (513 unledgered at `pmcd`):
- 367: `Unexpected token` / `Unexpected token`, lite on a later line: `...e,\n        attributeTypesVisible=true,\n        color=#FFFFCC,\n        lineWidth=1.0)\n}` (lite 4:13, engine 2:68)
- 29: `expected '_'` / `Unexpected token`, lite later on the line: `###Diagram\nDiagram meta::pure::MyDiagram\n{\n  propertyView\n  {\n    sourc` (lite 6:10, engine 6:5)
- 20: `unknown diagram view '_'` / `Unexpected token`, lite later on the line: `###Diagram\nDiagram meta::pure::MyDiagram\n{\n  cla}` (lite 4:6, engine 4:3)
**`###DataQualityValidation`** (297 unledgered at `pmcd`):
- 69: `expected <token> but found another` / `Unexpected token`, lite later on the line: `...ion\nDataQualityRelationComparison meta::dataquality::TestRelationComparison\n{\n    sour@` (lite 4:9, engine 4:5)
- 26: `expected <token> but found EOF` / `Unexpected token`, at end of input: `###DataQualityValidation\nDataQualityRelationValidation meta::dataquality::Validation\n{\n` (lite 3:2, engine 4:1)
- 24: `malformed quoted name: missing surrounding quotes` / `Unexpected token`, lite later on the line: `...yValidation\n{\n   context: fromDataSpace(meta::external::dataquality::PersonDataSpace, '$` (lite 4:75, engine 4:73)
**`###ExternalFormat`** (211 unledgered at `pmcd`):
- 47: `malformed quoted name: missing surrounding quotes` / `Unexpected token`, lite later on the line: `...##ExternalFormat\nBinding demo::binding::PersonJsonBinding\n{\n  contentType: 'application` (lite 4:28, engine 4:16)
- 33: `expected <token> but found another` / `Unexpected token`, lite later on the line: `...n  fullName: String[1];\n}\n\n###ExternalFormat\nBinding meta::firm::Binding\n{\n  schem}#` (lite 10:8, engine 10:3)
- 24: `expected <token> but found EOF` / `Unexpected token`, at end of input: `###ExternalFormat\nBinding demo::binding::PersonJsonBinding\n{\n ` (lite 3:2, engine 4:2)
**`###Data`** (179 unledgered at `pmcd`):
- 27: `(accepts)` / `Unexpected token`, lite accepts, engine refuses: `...         method: GET;\n          url: '/employees';\n        };\n      }\n    ]\n  }#\n}\n` (lite None:None, engine 7:8)
- 22: `embedded data kind '_' is unbuilt` / `no viable alternative`, lite earlier on the line: `###Data\nData snaps::data::TradesSample\n{\n  Relati\\u12` (lite 4:3, engine 4:9)
- 16: `embedded data kind '_' is unbuilt` / `no viable alternative`, at end of input: `###Data\nData meta::data::MyData\n{\n   E'` (lite 4:4, engine 4:5)
**`###ServiceStore`** (167 unledgered at `pmcd`):
- 29: `expected <token> but found another` / `Unexpected token`, lite later on the line: `...\n    method : GET;\n    parameters :\n    (\n      serializationFormat : String ( locati)` (lite 10:44, engine 10:38)
- 29: `expected <token> but found EOF` / `Unexpected token`, at end of input: `###ServiceStore\nServiceStore test::testServiceStoreGrammarWithSingleService\n(\n` (lite 3:2, engine 4:1)
- 15: `expected <token> but found EOF` / `Unexpected token`, lite later on the line: `...tore test::testServiceStoreGrammarWithSingleService\n(\n  Service TestService\n  (\n    pa` (lite 6:7, engine 6:5)
**`###Snowflake`** (109 unledgered at `pmcd`):
- 30: `expected <token> but found another` / `Unexpected token`, lite later on the line: `###Snowflake\nSnowflakeM2MUdf x::A\n{\n   ownershi\"` (lite 4:12, engine 4:4)
- 13: `unknown key` / `Unexpected token`, lite later on the line: `###Snowflake\nSnowflakeApp x::A\n{\n   applicatioName : 'sass';\n}\n` (lite 4:21, engine 4:4)
- 13: `Unknown permission scheme '_'` / `Unknown permission scheme '_'`, lite later on the line: `...   permissionScheme : WHATSCHEME;\n   ownership : Deployment { identifier: 'pierre'};\n}\n` (lite 5:33, engine 5:4)
**`island #...`** (13,237 unledgered at `spec`):
- 10,428: `unknown DSL island type: '_'` / `no such island parser`, lite later on the line: `#IQ{SELECT PATINDEX('%z%', 'abc')}#` (lite 1:36, engine 1:34)
- 2,186: `unknown DSL island type: '_'` / `no such island parser`, lite on a later line: `#IQ{SELECT 1 -- + 100\n + 1}#` (lite 2:7, engine 1:34)
- 374: `unknown DSL island type: '_'` / `no such island parser`, lite earlier on the line: `#9x{content}#` (lite 1:14, engine 1:34)
**`lambda |`** (1,427 unledgered at `spec`):
- 1,002: `brace-less #TDS literal is not supported` / `Unexpected token`, lite on a later line: `\|#TDS\nid\n1\n#->extend(~pi: x \| 3.14)` (lite 4:2, engine 1:2)
- 349: `unknown DSL island type: '_'` / `no such island parser`, lite later on the line: `\|#Cypher{MATCH (p:Person) WHERE p.age > 30 RETURN p.name}#` (lite 1:59, engine 1:40)
- 43: `unknown DSL island type: '_'` / `no such island parser`, lite earlier on the line: `\|#IQ{SELECT 1}#` (lite 1:16, engine 1:36)

## The harness check

The differences the last agents listed as unchanged all show up:

| input | where it landed | ledger |
|---|---|---|
| `^a::B($x)` | lite looser at the record doors `spec`, `lambda`, `pmcd`; same place at `lambdaJson`, `document` | S331, P69 |
| row 74's eight entries (`['a']`, `[a, b]`, `[]`, `#/a::B/c#`, `[#/a::B/c#]`, `[{x: 1;}]`, `database:`, `binding:`) | lite looser at `pmcd`, same place at `document`, all eight | S337, P74 |
| `expected: 'a';` | lite looser at `document` and `pmcd` | unledgered |
| `platform: Foo;` | same line, other column: lite 5:16, engine 5:13 | P73 |
| `^a::B('a'+=1)` | lite stricter at all five doors | unledgered (a judgement call) |
| `#/a::B/c(1)!n#` | lite stricter at all five doors | unledgered |
| `1 +` | same line, other column at all five doors, lite one column later (1:4 / 1:3; in a function 3:6 / 3:5) | unledgered |
| `letFunction(1, 2)`, `#/a::B/c(1.5)#` (added to show the raw category is live) | lite raw `java.lang.UnsupportedOperationException` at `lambdaJson`, both accept elsewhere | rulings 12 and 16 |

The new probes against the previous agents' single-threaded answers on their 501 cases (`r15`): 678 engine asks and 1,179
lite asks, none different.

## What could not be established

- Why the engine's parser is not reproducible under threads, and whether a server sharing one JVM sees it (it was not looked
  for; the serial answers are the ones measured).
- Mutations reach the expression parser only through function bodies at the model doors: `spec`, `lambda` and `lambdaJson`
  were asked the corpus's unmutated expressions alone, nearly all lite-only syntax, so their accept/refuse figures say
  little about expressions the engine reads.
- What two accepted texts answer (the 22,664 and 22,282 "both accept", and 8,157 at the lambda doors): out of scope, a
  possible extension (the engine's JSON beside `document`'s).
- Error wording, and the end of the engine's span: compared by no column here.
- `block`, and lite's and the platform's dialects: not asked of the engine.
- That the mutated texts are a fair sample of real input: they are random cuts and insertions over 9,563 texts of the tests'
  own models. The rates are for those.

## After ruling 18 (2026-11-09)

Ruling 18 of `docs/rust-port/PHASE2.md` section 6, carried out; the same probes on the same pinned inputs (the seeds and SHA-256
above), lite at commit `44012955b` through the same `doors` binary source, the engine's serial answers those recorded above
(the same jars, the same texts). Before is the measurement above, after is this one.

**By door and category.**

| set | door | texts | both accept | same place | same line, other col | different line | lite looser | lite stricter | raw `java.*` | engine timeout | engine unplaced |
|---|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| corpus, before | `pmcd` | 29,188 | 22,664 | 3,113 | 1,721 | 1,077 | 593 | 20 | 0 | 0 | 0 |
| corpus, after | `pmcd` | 29,188 | 22,663 | 3,247 | 1,671 | 1,005 | 581 | 21 | 0 | 0 | 0 |
| corpus, before | `document` | 29,188 | 22,664 | 3,113 | 1,721 | 1,077 | 593 | 20 | 0 | 0 | 0 |
| corpus, after | `document` | 29,188 | 22,663 | 3,247 | 1,671 | 1,005 | 581 | 21 | 0 | 0 | 0 |
| corpus, before | `spec` | 22,091 | 7,282 | 59 | 11,262 | 3,417 | 49 | 3 | 0 | 0 | 19 |
| corpus, after | `spec` | 22,091 | 7,282 | 109 | 11,260 | 3,417 | 0 | 3 | 0 | 0 | 20 |
| corpus, before | `lambda` | 22,967 | 8,157 | 53 | 11,244 | 3,445 | 44 | 5 | 0 | 0 | 19 |
| corpus, after | `lambda` | 22,967 | 8,157 | 97 | 11,243 | 3,445 | 0 | 5 | 0 | 0 | 20 |
| corpus, before | `lambdaJson` | 22,967 | 8,157 | 60 | 11,244 | 3,445 | 36 | 5 | 0 | 0 | 20 |
| corpus, after | `lambdaJson` | 22,967 | 8,157 | 97 | 11,243 | 3,445 | 0 | 5 | 0 | 0 | 20 |
| mutated (16 per text), before | `pmcd` | 153,008 | 22,282 | 87,633 | 27,256 | 14,894 | 586 | 357 | 0 | 0 | 0 |
| mutated (16 per text), after | `pmcd` | 153,008 | 22,281 | 99,981 | 19,902 | 10,186 | 300 | 358 | 0 | 0 | 0 |
| mutated (16 per text), before | `document` | 153,008 | 22,282 | 87,650 | 27,260 | 14,894 | 565 | 357 | 0 | 0 | 0 |
| mutated (16 per text), after | `document` | 153,008 | 22,281 | 99,981 | 19,902 | 10,186 | 300 | 358 | 0 | 0 | 0 |
| mutated (4 per text), before | `pmcd` | 38,252 | 5,666 | 21,827 | 6,831 | 3,658 | 165 | 105 | 0 | 0 | 0 |
| mutated (4 per text), after | `pmcd` | 38,252 | 5,666 | 24,894 | 4,961 | 2,542 | 84 | 105 | 0 | 0 | 0 |
| mutated (4 per text), before | `document` | 38,252 | 5,666 | 21,832 | 6,831 | 3,658 | 160 | 105 | 0 | 0 | 0 |
| mutated (4 per text), after | `document` | 38,252 | 5,666 | 24,894 | 4,961 | 2,542 | 84 | 105 | 0 | 0 | 0 |

No text timed out and no door panicked or answered a raw `java.*` class on the input sets; the harness's two `lambdaJson` raw
answers are the two rulings 12 and 16 left. A further run of the post-6b mutator through every door: 153,008 door answers, no
panic, no raw answer. The 1 more `lite stricter` at `pmcd` over the corpus and over the mutated texts is register S345; the corpus
`spec`/`lambda`/`lambdaJson` looser rows are 0.

**What the difference comes down to**, the heuristic above (`engine-gap/report.py.txt`, `theme`), every disagreement of the
kind, not only the unledgered ones (so `the end of the input`, which the table above counted among the unledgered as 17,009,
reads 17,713 here):

| what the difference comes down to | `pmcd`, mutated, before | after | `pmcd`, corpus, before | after | `spec`, corpus, before | after |
|---|---:|---:|---:|---:|---:|---:|
| a lite-only island refused at the engine's dialect: the column | 112 | 112 | 33 | 33 | 13,419 | 13,419 |
| a construct lite refuses by name, at a token of its own | 11,428 | 10,492 | 1,917 | 1,910 | 5 | 5 |
| the end of the input: where each places it | 17,713 | 6,088 | 182 | 67 | 7 | 8 |
| lite reads a token further (or fewer) before stopping | 5,917 | 5,770 | 107 | 100 | 8 | 5 |
| lite's walk stops at another token | 4,541 | 5,032 | 164 | 173 | 0 | 0 |
| other | 2,280 | 2,435 | 329 | 327 | 2 | 2 |
| brace-less #TDS: lite's place past the literal | 159 | 159 | 66 | 66 | 1,238 | 1,238 |
| lite accepts what the engine refuses | 586 | 300 | 593 | 581 | 49 | 0 |
| lite refuses what the engine accepts | 357 | 358 | 20 | 21 | 3 | 3 |
| the engine answers a raw exception, unplaced | 0 | 0 | 0 | 0 | 19 | 20 |

The end of the input, the cluster ruling 18 names: **17,713 → 6,088** mutated texts at `pmcd`, **182 → 67** corpus texts. Same
line and column overall, mutated at `pmcd`: **87,633 → 99,981** of 153,008. What remains of the cluster is register S349: a
refusal by name or a graph-fetch island cut off by the end of the input, and a text's last token that lite refuses where the
engine's next token would have been the end.

**Lite looser.** `pmcd` and `document`: 581 corpus (571 S64's TIME and TIMESTAMP_NS columns, excluded by ruling 18; 10 left), 300
mutated (16 per text; 280 S64, 20 left), 84 mutated (4 per text; 74 S64, 10 left). `spec`, `lambda`, `lambdaJson`: 0, from 49, 44
and 36. **It is not 0 apart from S64.** Forty texts are left, register S352: six because lite's own tests rely on them (the three
`Diagram X(width=..)` texts and the three `Field 'data' is required` texts), 34 because no rule was written for them:

| section | text | the engine | the engine's words | at |
|---|---|---|---|---|
| ###Data | `mut16/m095081` | 60:31 | Unexpected token | `          timeOfBirth = %19:41⟦:.21,⟧` |
| ###Data | `mut16/m118799` | 130:32 | Unexpected token | `                expected: '123'⟦.;⟧` |
| ###Data | `mut/m017896` | 18:4 | Unexpected token | `   ⟦;           }#⟧` |
| ###Data | `mut16/m048161` | 37:15 | Unexpected token '.5'. Valid alternatives: ['.'] | `    default.F1⟦.5eirmTable:⟧` |
| ###Data | `mut16/m035163` | 7:8 | Unexpected token '{' | `]      ⟦{⟧` |
| ###DataSpace | `mut/m013140` | 54:9 | Invalid identifier: \|model | `        ⟦DataspaceTestData⟧` |
| ###DataSpace | `mut16/m105822` | 25:59 | Unexpected token | `model::templateFunc():TabularDataSe     ⟦executionContextKey: 'Con⟧` |
| ###DataSpace | `mut16/m130457` | 55:12 | Unexpected token | `          1⟦ , Alice⟧` |
| ###Diagram | `corpus/c2fe663d2fc40516c.model` | 5:33 | Unexpected token '(' | `Diagram meta::test::TestDiagram1⟦(width=10.0, height=10.0)⟧` |
| ###Diagram | `corpus/ca55a26307280473e.model` | 5:33 | Unexpected token '(' | `Diagram meta::test::TestDiagram1⟦(width=10.0, height=10.0)⟧` |
| ###Diagram | `corpus/cf0bae7c7ba44f834.model` | 5:33 | Unexpected token '(' | `Diagram meta::test::TestDiagram1⟦(width=10.0, height=10.0)⟧` |
| ###GenerationSpecification | `corpus/cbec49736727ea2c1.model` | 18:10 | Unexpected token 'id' | `         ⟦id: 'afterTheElement';⟧` |
| ###HostedService | `mut16/m069011` | 70:1 | Unexpected token | `⟦::         [⟧` |
| ###HostedService | `mut/m024784` | 49:96 | no viable alternative at input '->filter(row\|$row.getStrin | `lter(row\|$row.getString('name')->startsW⟦;ith('X'))->meta::legend:⟧` |
| ###Mapping | `corpus/c3ff42094a2558e99.model` | 2:188 | Field 'data' is required | ` model::P { x } }#);     tests: [       ⟦Test1: {         asserts:⟧` |
| ###Mapping | `mut16/m038818` | 2:186 | Field 'data' is required | ` model::P { x } }#);     tests: [       ⟦Test1: {         asserts:⟧` |
| ###Mapping | `mut16/m038823` | 2:193 | Field 'data' is required | ` model::P { x } }#);     tests: [       ⟦Test1: {         asserts:⟧` |
| ###Mapping | `corpus/cc65c96f1ea152d8f.model` | 26:5 | Unexpected token | `(⟦⟧` |
| ###Mapping | `mut16/m121704` | 210:12 | Unexpected token '2'. Valid alternatives: ['EnumerationMap | `    nested[⟦2]: $src,⟧` |
| ###Mapping | `mut16/m015906` | 6:14 | Unexpected token '<EOF>' | `    p: tru::e⟦⟧` |
| ###Persistence | `corpus/c40188948116467ae.model` | 13:67 | An exception of type 'NullPointerException' occurred, plea | `nce::neg::GraphFetchKeysAsIdentifiers⟦⟧` |
| ###Persistence | `corpus/c45e71b03edee21e5.model` | 20:74 | An exception of type 'NullPointerException' occurred, plea | `g::TdsKeysAsPaths⟦⟧` |
| ###Persistence | `mut16/m039008` | 51:5 | Unexpected token | `    ⟦;}⟧` |
| ###Persistence | `corpus/c29219da69acc11da.model` | 45:7 | Unexpected token 'isTestDataFromServiceOutput'. Valid alte | `      ⟦isTestDataFromServiceOutp⟧` |
| ###Pure | `mut16/m013663` | 9:3 | Unexpected token | `  ⟦99999999999999999999     ⟧` |
| ###Pure | `mut16/m117352` | 32:7 | Unexpected token | `      ⟦.PersonTable.unknown:⟧` |
| ###Pure | `mut/m013808` | 19:20 | Unexpected token | `    ~new: x\|\|$x.ied⟦:y\|$y->sum()⟧` |
| ###Pure | `mut/m000374` | 30:25 | Unexpected token ','. Valid alternatives: ['.'] | `            id, firm;_id⟦, firstName, lastName, em⟧` |
| ###Pure | `mut16/m131940` | 46:18 | Unexpected token '<EOF>'. Valid alternatives: ['.'] | `                 ⟦                         ⟧` |
| ###Pure | `mut16/m069699` | 8:1 | Unexpected token ']'. Valid alternatives: ['.'] | `⟦];⟧` |
| ###Pure | `mut/m000375` | 27:45 | token recognition error at: '"TE;\n' | `        3 , 2      , "Time;"          , ⟦"TE;⟧` |
| ###QueryPostProcessor | `corpus/c5462542fc6ca12cd.model` | 18:4 | Unexpected token 'SchemaMappers'. Valid alternatives: [')' | `   ⟦SchemaMappers:⟧` |
| ###Relational | `mut/m000092` | 10:89 | Unexpected token ':'. Valid alternatives: ['Database', 'in | `NG_OUT = dummy, INFINITY_DATE = %9999-00⟦:00.0000),⟧` |
| ###Service | `mut16/m024076` | 27:80 | Unexpected token | `, 440, 13.23, 88, -54, 2.3, [1, 2], %204⟦T00:00:00, %2019-05-24, E⟧` |
| ###Service | `mut16/m114522` | 61:21 | Unexpected token | `            param = ⟦\|123⟧` |
| ###Service | `mut16/m043307` | 23:58 | Unexpected token '('. Valid alternatives: ['.'] | `aram']), 'singleparam', list([1,2,3]),st⟦([%2019-05-24,%2019-05-25⟧` |
| ###Service | `mut/m006686` | 32:11 | Unexpected token 'Alice'. Valid alternatives: ['.'] | `     ;    ⟦ Name⟧` |

**The ledger.** Every difference still counted above now has a family row: S343 to S345 (lite stricter), S346 to S351 (the place),
S352 (lite looser, what is left), beside the rows the measurement above matched. The per-section tables above are not repeated:
their rows are the families' now.

## After rulings 19 to 21 (2026-11-10)

Rulings 19 to 21 of `docs/rust-port/PHASE2.md` section 6, carried out; the same probes on the same pinned inputs (the seeds
and SHA-256 above), lite at commit `e02b2cb0a` through the same `doors` binary source with one door added (`block`, below), the
engine's serial answers those recorded above (the same jars and texts; none asked again). Before is lite at `3328a746e`, the
code ruling 18 left (`44012955b`; the commits between touch documents alone); after is this one.

**`block` (ruling 21).** The measurement had no `block` door. Its counterpart is the engine's `parseLambda`, which reads a
text as a function body's `codeBlock` (`function go():Any[*]{` + text + `\n}`) as lite's `block` door reads a `;`-separated
block, so `block` at `LEGEND_ENGINE` was asked every expression text of the corpus (22,091) and held to the `parseLambda`
answers recorded above. Before: 2 looser, the empty text and a blank one, an empty block that the engine's `codeBlock` (at
least one line) refuses at its wrapper's `}` (`[2:1]`); 2 stricter, ledgered (S323, S345). After: 0 looser (register S368,
refused in the `lambda` door's words and place for an empty text), the same 2 stricter. Nothing else was looser.

**By door and category.**

| set | door | texts | both accept | same place | same line, other col | different line | lite looser | lite stricter | raw `java.*` | engine timeout | engine unplaced |
|---|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| corpus, before | `pmcd` | 29,188 | 22,663 | 3,247 | 1,671 | 1,005 | 581 | 21 | 0 | 0 | 0 |
| corpus, after | `pmcd` | 29,188 | 22,663 | 3,254 | 1,670 | 1,005 | 575 | 21 | 0 | 0 | 0 |
| corpus, before | `document` | 29,188 | 22,663 | 3,247 | 1,671 | 1,005 | 581 | 21 | 0 | 0 | 0 |
| corpus, after | `document` | 29,188 | 22,663 | 3,254 | 1,670 | 1,005 | 575 | 21 | 0 | 0 | 0 |
| corpus, before | `spec` | 22,091 | 7,282 | 109 | 11,260 | 3,417 | 0 | 3 | 0 | 0 | 20 |
| corpus, after | `spec` | 22,091 | 7,282 | 110 | 11,259 | 3,417 | 0 | 3 | 0 | 0 | 20 |
| corpus, before | `lambda` | 22,967 | 8,157 | 97 | 11,243 | 3,445 | 0 | 5 | 0 | 0 | 20 |
| corpus, after | `lambda` | 22,967 | 8,157 | 98 | 11,242 | 3,445 | 0 | 5 | 0 | 0 | 20 |
| corpus, before | `lambdaJson` | 22,967 | 8,157 | 97 | 11,243 | 3,445 | 0 | 5 | 0 | 0 | 20 |
| corpus, after | `lambdaJson` | 22,967 | 8,157 | 98 | 11,242 | 3,445 | 0 | 5 | 0 | 0 | 20 |
| corpus, before | `block` | 22,091 | 7,290 | 97 | 11,239 | 3,441 | 2 | 2 | 0 | 0 | 20 |
| corpus, after | `block` | 22,091 | 7,290 | 98 | 11,238 | 3,443 | 0 | 2 | 0 | 0 | 20 |
| mutated (16 per text), before | `pmcd` | 153,008 | 22,281 | 99,981 | 19,902 | 10,186 | 300 | 358 | 0 | 0 | 0 |
| mutated (16 per text), after | `pmcd` | 153,008 | 22,281 | 100,042 | 19,861 | 10,184 | 282 | 358 | 0 | 0 | 0 |
| mutated (16 per text), before | `document` | 153,008 | 22,281 | 99,981 | 19,902 | 10,186 | 300 | 358 | 0 | 0 | 0 |
| mutated (16 per text), after | `document` | 153,008 | 22,281 | 100,042 | 19,861 | 10,184 | 282 | 358 | 0 | 0 | 0 |
| mutated (4 per text), before | `pmcd` | 38,252 | 5,666 | 24,894 | 4,961 | 2,542 | 84 | 105 | 0 | 0 | 0 |
| mutated (4 per text), after | `pmcd` | 38,252 | 5,666 | 24,917 | 4,949 | 2,541 | 74 | 105 | 0 | 0 | 0 |
| mutated (4 per text), before | `document` | 38,252 | 5,666 | 24,894 | 4,961 | 2,542 | 84 | 105 | 0 | 0 | 0 |
| mutated (4 per text), after | `document` | 38,252 | 5,666 | 24,917 | 4,949 | 2,541 | 74 | 105 | 0 | 0 | 0 |
| harness, before | `pmcd` | 14 | 0 | 10 | 2 | 0 | 0 | 2 | 0 | 0 | 0 |
| harness, after | `pmcd` | 14 | 0 | 10 | 2 | 0 | 0 | 2 | 0 | 0 | 0 |
| harness, before | `document` | 14 | 0 | 10 | 2 | 0 | 0 | 2 | 0 | 0 | 0 |
| harness, after | `document` | 14 | 0 | 10 | 2 | 0 | 0 | 2 | 0 | 0 | 0 |
| harness, before | `spec` | 6 | 2 | 1 | 1 | 0 | 0 | 2 | 0 | 0 | 0 |
| harness, after | `spec` | 6 | 2 | 1 | 1 | 0 | 0 | 2 | 0 | 0 | 0 |
| harness, before | `lambda` | 7 | 2 | 2 | 1 | 0 | 0 | 2 | 0 | 0 | 0 |
| harness, after | `lambda` | 7 | 2 | 2 | 1 | 0 | 0 | 2 | 0 | 0 | 0 |
| harness, before | `lambdaJson` | 7 | 0 | 2 | 1 | 0 | 0 | 2 | 2 | 0 | 0 |
| harness, after | `lambdaJson` | 7 | 0 | 2 | 1 | 0 | 0 | 2 | 2 | 0 | 0 |
| harness, before | `block` | 6 | 2 | 1 | 1 | 0 | 0 | 2 | 0 | 0 | 0 |
| harness, after | `block` | 6 | 2 | 1 | 1 | 0 | 0 | 2 | 0 | 0 | 0 |

No text timed out, and no door panicked or answered a raw `java.*` class on the input sets; the harness's two `lambdaJson` raw
answers are the two rulings 12 and 16 left. No text moved away from the engine's place: of every text at `pmcd` that stood
at the engine's line and column before, each stands there now, and lite stricter is unchanged (21 corpus, 358 and 105
mutated). A further run of the post-6b mutator through every door: 153,008 door answers, no panic, no raw answer.

**Lite looser.** `pmcd` and `document`: 575 corpus (571 S64, 3 S352, 1 S353), 282 mutated (16 per text; 280 S64, 2 S353), 74
mutated (4 per text; all S64). `spec`, `lambda`, `lambdaJson`, `block`: 0. **It is 0 apart from the named, ruled exclusions**:
S64's TIME and TIMESTAMP_NS columns, and ruling 19's two, a Diagram's m2 argument list (S352) and a mapping test with no `data`
(S353), which lite's own tests rely on.

**The 34 (ruling 20)**, each now refused at the engine's line and column, by root cause (the table under "After ruling 18"
listed 37 of the 40 texts: the three line-comment texts, `mut/m014672`, `mut/m014673` and `mut16/m112129`, were left out of it;
all 34 are here). The engine's words and lite's are each cut to a few words.

| section | text | the engine | lite now | what it is | row |
|---|---|---|---|---|---|
| ###Persistence | `corpus/c29219da6` | 45:7 Unexpected token 'isTestDataFromServiceOutput... | 45:7 Unexpected token 'isTestDataFromServi... | a persistence test's keys out of `(testBatches isTestDataFromServiceOutput? graphFetchPath?)*` | S355 |
| ###Persistence | `mut16/m039008` | 51:5 Unexpected token | 51:5 Unexpected token ';' | a `;` after a persistence dataset type's body | S355 |
| ###Persistence | `corpus/c40188948` | 13:67 An exception of type 'NullPointerException' o... | 13:67 An exception of type 'NullPointerExce... | a graph-fetch output's key an identifier: the walker's other arm, its NPE | S356 |
| ###Persistence | `corpus/c45e71b03` | 20:74 An exception of type 'NullPointerException' o... | 20:74 An exception of type 'NullPointerExce... | a TDS output's key a path: the walker's other arm, its NPE | S356 |
| ###QueryPostProcessor | `corpus/c5462542f` | 18:4 Unexpected token 'SchemaMappers'. Valid alter... | 18:4 Unexpected token 'SchemaMappers' | a `RelationalMapper`'s `SchemaMappers` after its `TableMappers` | S357 |
| ###GenerationSpecification | `corpus/cbec49736` | 18:10 Unexpected token 'id' | 18:10 Unexpected token 'id' | a generation node's `id` after its element | S357 |
| ###Mapping | `corpus/cc65c96f1` | 26:5 Unexpected token | 26:5 Unexpected token '~filter' | a MongoDB mapping's `~filter` after `~mainCollection`, in its parser's coordinates | S357 |
| ###Data | `mut16/m095081` | 60:31 Unexpected token | 60:31 Unexpected token ':.21' | `%19:41:.21`: the lexer's `StrictTime` ends at `%19:41` | S358 |
| ###Service | `mut16/m024076` | 27:80 Unexpected token | 27:80 Unexpected token 'T00:00:00' | `%204T00:00:00`: the lexer's `Date` ends at `%204` | S358 |
| ###Relational | `mut/m000092` | 10:89 Unexpected token ':'. Valid alternatives: ['D... | 10:89 Unexpected token ':00.0000' | a milestoning's `INFINITY_DATE = %9999-00:00.0000` | S358 |
| ###Pure | `mut16/m013663` | 9:3 Unexpected token | 9:3 Unexpected token | a relation data path's first word a number | S359 |
| ###Pure | `mut16/m117352` | 32:7 Unexpected token | 32:7 Unexpected token | a relation data path that starts with `.` | S359 |
| ###Pure | `mut16/m131940` | 46:18 Unexpected token '<EOF>'. Valid alternatives:... | 46:18 Unexpected token | a function test assertion's table, then a word and the end (the walker's reparse) | S359 |
| ###Service | `mut/m006686` | 32:11 Unexpected token 'Alice'. Valid alternatives:... | 32:11 Unexpected token | a function test assertion whose table is a lone `;` (the walker's reparse) | S359 |
| ###Pure | `mut/m000374` | 30:25 Unexpected token ','. Valid alternatives: ['.'] | 30:25 Unexpected token | a `;` inside a header row, then `_id,` read as a path | S359 |
| ###Pure | `mut/m000375` | 27:45 token recognition error at: '"TE;\n' | 27:45 Unexpected token | a quoted cell left open | S359 |
| ###DataSpace | `mut16/m130457` | 55:12 Unexpected token | 55:12 Unexpected token | a data space's `sampleValues` table ended early (the walker's reparse) | S359 |
| ###Data | `mut16/m048161` | 37:15 Unexpected token '.5'. Valid alternatives: ['.'] | 37:15 Unexpected token | `default.F1.5eirmTable:`: `.5` a `FLOAT`, no dot | S359 |
| ###Data | `mut16/m118799` | 130:32 Unexpected token | 130:32 expected SEMI_COLON but found DOT ('.') | an `EqualTo` island's `expected: '123'.;` | S360 |
| ###Data | `mut/m017896` | 18:4 Unexpected token | 18:4 Unexpected token ';' | an `EqualTo` island's second `;` | S360 |
| ###Data | `mut16/m035163` | 7:8 Unexpected token '{' | 7:8 Unexpected token '{' | a ServiceStore data island going on after its `]` | S360 |
| ###DataSpace | `mut/m013140` | 54:9 Invalid identifier: \|model | 54:9 Invalid identifier: \|model | a `DataspaceTestData` island's text no path | S361 |
| ###DataSpace | `mut16/m105822` | 25:59 Unexpected token | 25:59 Unexpected token 'executionContextKey' | a data space executable's signature without its multiplicity and `;` | S361 |
| ###HostedService | `mut16/m069011` | 70:1 Unexpected token | 70:1 Unexpected token '::' | a HostedService test suite's `tests: ::` | S362 |
| ###HostedService | `mut/m024784` | 49:96 no viable alternative at input '->filter(row\... | 49:96 expected '(' after arrow-call functio... | a HostedService post-validation's lambda cut by a `;` | S362 |
| ###Service | `mut16/m114522` | 61:21 Unexpected token | 61:21 Unexpected token '\|' | a service test's parameter `param = \|123` | S363 |
| ###Service | `mut16/m043307` | 23:58 Unexpected token '('. Valid alternatives: ['.'] | 23:58 Unexpected token '(' | a legacy service test's parameter `st([...])` | S363 |
| ###Mapping | `mut16/m121704` | 210:12 Unexpected token '2'. Valid alternatives: ['E... | 210:12 Unexpected token '2' | a Pure mapping property's set id `[2]` | S364 |
| ###Mapping | `mut16/m015906` | 6:14 Unexpected token '<EOF>' | 6:14 Unexpected token '<EOF>' | an XStore mapping expression ending in a path (`p: tru::e`) | S365 |
| ###Pure | `mut16/m069699` | 8:1 Unexpected token ']'. Valid alternatives: ['.'] | 8:1 Unexpected token ']' | a default value's reference without its `.name` in an array | S366 |
| ###Pure | `mut/m013808` | 19:20 Unexpected token | 19:20 Unexpected token ':' | `~new: x\|\|$x.ied:y\|...`: `x` a type, the spec ended, a second `:` | S367 |
| ###Pure | `mut/m014672` | 10:70 Unexpected token | 10:70 Unexpected token '@' | `// ... BASIS,\r@`: the comment ends at the `\r` | S354 |
| ###Pure | `mut/m014673` | 13:35 Unexpected token | 13:35 Unexpected token '::' | `// ...License.\r::` | S354 |
| ###Pure | `mut16/m112129` | 10:70 Unexpected token ';'. Valid alternatives: ['C... | 10:70 Unexpected token ';' | `// ... BASIS,\r;` | S354 |

**The end of the input (ruling 20: ledgered as a family, not chased).** 6,088 → 6,080 mutated texts at `pmcd`, 67 → 67 corpus;
the family is register S349, which now names its sub-causes with their counts.

**What the difference comes down to**, the heuristic above (`engine-gap/report.py.txt`, `theme`), every disagreement of the
kind, before and after:

| what the difference comes down to | `pmcd`, mutated, before | after | `pmcd`, corpus, before | after | `spec`, corpus, before | after | `block`, corpus, before | after |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| a lite-only island refused at the engine's dialect: the column | 112 | 112 | 33 | 33 | 13,419 | 13,419 | 13,415 | 13,415 |
| a construct lite refuses by name, at a token of its own | 10,492 | 10,492 | 1,910 | 1,910 | 5 | 5 | 5 | 5 |
| the end of the input: where each places it | 6,088 | 6,080 | 67 | 67 | 8 | 7 | 16 | 17 |
| lite reads a token further (or fewer) before stopping | 5,770 | 5,763 | 100 | 100 | 5 | 5 | 5 | 5 |
| lite's walk stops at another token | 5,032 | 5,030 | 173 | 172 | 0 | 0 | 0 | 0 |
| other | 2,435 | 2,409 | 327 | 327 | 2 | 2 | 1 | 1 |
| brace-less #TDS: lite's place past the literal | 159 | 159 | 66 | 66 | 1,238 | 1,238 | 1,238 | 1,238 |
| lite accepts what the engine refuses | 300 | 282 | 581 | 575 | 0 | 0 | 2 | 0 |
| lite refuses what the engine accepts | 358 | 358 | 21 | 21 | 3 | 3 | 2 | 2 |
| the engine answers a raw exception, unplaced | 0 | 0 | 0 | 0 | 20 | 20 | 20 | 20 |

**Found while probing, not refused** (inputs written by hand; none is in the pinned inputs, and none was ruled): a persister's
bare kind written with a body (`auditing: None {}`), which the engine's persister grammar spells with `;` alone (S355); a
constraint's keyword written as a column spec in an expression (`[~owner: abc]`), which the engine's lexer reads as
`CONSTRAINT_OWNER` and refuses; and the comma lists ruling 18's helper does not reach that were not probed (a DataQuality
tree's `<...>` constraints and the other loops of `data_quality_parser.rs`). Two such lists were probed and are refused now
(a DataQuality tree's `{...}` and a HostedService's `UserList` users: a trailing comma, S341's rule).

## After rulings 23 to 26 (2026-11-11)

Rulings 23 to 26 of `docs/rust-port/PHASE2.md` section 6, carried out; the same probes on the same pinned inputs (the seeds
and SHA-256 above), lite at commit `cedfd8fea` through the same `doors` binary source (`block` included), the engine's serial
answers those recorded above (the same jars and texts; none asked again). Before is lite at `8bd6583d9`, the tree the rulings
were built on (`main` merged at `c4a39e06d`), whose answers are those of "After rulings 19 to 21", every count equal; after is
this one. Every new input of rulings 24 and 26 was asked of legend-engine's own parser (`PureGrammarParser`, 4.145.0, on the
parser-equivalence class path), by hand, not recalled.

**By door and category.** Only the mutated texts at `pmcd` and `document` moved; every other row is the same before and after.

| set | door | texts | both accept | same place | same line, other col | different line | lite looser | lite stricter | raw `java.*` | engine timeout | engine unplaced |
|---|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| corpus, before and after | `pmcd` | 29,188 | 22,663 | 3,254 | 1,670 | 1,005 | 575 | 21 | 0 | 0 | 0 |
| corpus, before and after | `document` | 29,188 | 22,663 | 3,254 | 1,670 | 1,005 | 575 | 21 | 0 | 0 | 0 |
| corpus, before and after | `spec` | 22,091 | 7,282 | 110 | 11,259 | 3,417 | 0 | 3 | 0 | 0 | 20 |
| corpus, before and after | `lambda` | 22,967 | 8,157 | 98 | 11,242 | 3,445 | 0 | 5 | 0 | 0 | 20 |
| corpus, before and after | `lambdaJson` | 22,967 | 8,157 | 98 | 11,242 | 3,445 | 0 | 5 | 0 | 0 | 20 |
| corpus, before and after | `block` | 22,091 | 7,290 | 98 | 11,238 | 3,443 | 0 | 2 | 0 | 0 | 20 |
| mutated (16 per text), before | `pmcd` | 153,008 | 22,281 | 100,042 | 19,861 | 10,184 | 282 | 358 | 0 | 0 | 0 |
| mutated (16 per text), after | `pmcd` | 153,008 | 22,281 | 100,047 | 19,858 | 10,182 | 282 | 358 | 0 | 0 | 0 |
| mutated (16 per text), before | `document` | 153,008 | 22,281 | 100,042 | 19,861 | 10,184 | 282 | 358 | 0 | 0 | 0 |
| mutated (16 per text), after | `document` | 153,008 | 22,281 | 100,047 | 19,858 | 10,182 | 282 | 358 | 0 | 0 | 0 |
| mutated (4 per text), before | `pmcd` | 38,252 | 5,666 | 24,917 | 4,949 | 2,541 | 74 | 105 | 0 | 0 | 0 |
| mutated (4 per text), after | `pmcd` | 38,252 | 5,666 | 24,918 | 4,949 | 2,540 | 74 | 105 | 0 | 0 | 0 |
| mutated (4 per text), before | `document` | 38,252 | 5,666 | 24,917 | 4,949 | 2,541 | 74 | 105 | 0 | 0 | 0 |
| mutated (4 per text), after | `document` | 38,252 | 5,666 | 24,918 | 4,949 | 2,540 | 74 | 105 | 0 | 0 | 0 |
| harness, before and after | every door | as "After rulings 19 to 21" | | | | | | | | | |

No text timed out, and no door panicked or answered a raw `java.*` class on the input sets; the harness's two `lambdaJson` raw
answers are the two rulings 12 and 16 left. **No text moved away from the engine's place**: of every (text, door) at the
engine's line and column before, each stands there now, and lite stricter is unchanged. Six mutated texts reached it, each by
a ruling-26 rule: a class constraint's `~owner:` or `~function:` mutated into an expression, refused at the `~` (`mut16/m073353`,
`m093569`, `m109202`, `m118514`, `mut/m023471`; S370), and a data space context's name a path (`mut16/m011514`, `Unexpected
token 'ce'` at `[4:70]`; S371).

**Lite looser** is unchanged: 575 corpus (571 S64, 3 S352, 1 S353), 282 mutated (16 per text; 280 S64, 2 S353), 74 mutated (4 per
text; all S64), 0 at every expression door. It is 0 apart from the ruled exclusions. Of the inputs written by hand, the three
families of ruling 26 are 0 looser now (below).

**Ruling 23: several errors.** The model door reads on past a refused element, at element and section boundaries only, and
lists the errors after the first, sorted by place, placed after it (register S372). The first is the door's answer: the measured
doors (`answer_form`, `engine_forms`) answer as before, and so does every count above. Through the list's own door
(`doors::pmcd_with_errors`) at all three dialects: the corpus's 29,188 model texts (87,564 asks) -- 1,234 asks hold further
errors, 5,733 in all; the mutated texts (16 per text, 153,008, 459,024 asks) -- 9,397 asks hold further errors, 16,803 in all; in
both, no panic, no further error out of order or placed at or before the first, and the first equal to `pmcd`'s answer in every
ask. Over the front oracle (every frozen question, the model door's answer taken as the list's first) the questions Rust
answers differently from Java's frozen answer are byte for byte the 760 listed before.

**Ruling 24: an empty or blank block** (`block` at the engine's dialect): placed where an expression was expected, the end of
the input (`[1:1]` for an empty text, `[1:4]` for three blanks, `[2:1]` for a line break or a comment line alone), in lite's
words, where it was unplaced (S368). The corpus's two such texts (an empty one, three blanks) are refused at `[1:1]` and `[1:4]`
now; the engine's place for both is its wrapper's `[2:1]`, so both stay "different line" and `block`'s row is unchanged.

**Ruling 26: the three looser families found by hand**, each probed against the engine and fixed at its place (texts; a model
text is asked at `pmcd` and `document`, an expression at `spec`, `lambda`, `lambdaJson` and `block`, every looser text looser at each):

| family | probed | looser before | after | row |
|---|---|---:|---:|---|
| a persister's bare kind with a body: every `key: Kind;` the engine accepts in a corpus `###Persistence` text, written `Kind {}`, `Kind { }`, `Kind {};`, `Kind {\n}` (296), and four by hand (a body with a field, an island, two inside a target's island) | 300 | 14 (`auditing: None`, `mergeStrategy: NoDeletes`, `deduplicationStrategy: None`, `AnyVersion`) | 0 | S369 |
| a constraint's keyword as a column spec: the five words with a `:` (blanks, a tab, a line break between), a blank after the `~`, a comment, a longer word, at `spec`, `lambda`, `block`, in a `###Pure` function body and derived property, and in `###Mapping`, `###Service`, `###DataSpace` and `###DataQualityValidation` expressions (which the engine takes) | 33 | 17 | 0 | S370 |
| DataQuality's lists: every comma loop of `data_quality_parser.rs` (a class's and a property's constraints, the context's two arguments, `validations`, `keys`, `columnsToCompare`, `testSuites`, `data`, `tests`, `asserts`) with a trailing, a missing and a lone comma and an empty list | 51 | 11 | 0 | S371 |

In the persistence family the engine's target islands take a body after `None` (`auditing: None {}` inside a relational target's
`#{...}#`): lite takes it there too. In the DataQuality family the engine's place for a trailing comma after the last validation is
that validation's `}`; a missing comma after it keeps lite's answer (the engine's place then depends on the next token, a place
difference, not a looser text). The unit test's 30 texts (`doors::refusal_tests::the_three_looser_inputs_found_by_hand_...`)
were asked of the engine again: each answers the place the test holds, and lite the same.

**Found while probing, not refused** (written by hand, not in the pinned inputs, not ruled): a data space executable's `id:`
written as a string (`id: 'e';`), which lite accepts and the engine refuses at the string (`[16:11] Unexpected token ''e''`): its
grammar takes an identifier.

## After rulings 27 and 28 (2026-11-11)

Rulings 27 and 28 of `docs/rust-port/PHASE2.md` section 6, carried out; the same probes on the same pinned inputs (the seeds and
SHA-256 above), lite at commit `bcf60ed8f` through the same `doors` binary source, the engine's serial answers those recorded
above (the same jars and texts; none asked again). Before is lite at `68f8ebd0b`, whose parser is `cedfd8fea`'s, so every count
of "After rulings 23 to 26" (asked again: equal); after is this one. Every new input was written by hand and asked of
legend-engine's own parser (`PureGrammarParser`, 4.145.0, on the parser-equivalence class path), not recalled: the 346 texts of
the tables below, each construct with two errors in one element in either order and with one, and the 58 the new unit tests
hold.

**By door and category.** Only `pmcd` and `document` moved, and only towards the engine's place; every other row, the harness
included, is the same before and after.

| set | door | texts | both accept | same place | same line, other col | different line | lite looser | lite stricter |
|---|---|---:|---:|---:|---:|---:|---:|---:|
| corpus, before | `pmcd`, `document` | 29,188 | 22,663 | 3,254 | 1,670 | 1,005 | 575 | 21 |
| corpus, after | `pmcd`, `document` | 29,188 | 22,663 | 3,256 | 1,670 | 1,003 | 575 | 21 |
| mutated (16 per text), before | `pmcd`, `document` | 153,008 | 22,281 | 100,047 | 19,858 | 10,182 | 282 | 358 |
| mutated (16 per text), after | `pmcd`, `document` | 153,008 | 22,281 | 100,058 | 19,855 | 10,174 | 282 | 358 |
| mutated (4 per text), before | `pmcd`, `document` | 38,252 | 5,666 | 24,918 | 4,949 | 2,540 | 74 | 105 |
| mutated (4 per text), after | `pmcd`, `document` | 38,252 | 5,666 | 24,920 | 4,948 | 2,539 | 74 | 105 |

No text timed out, panicked or answered a raw `java.*` class. **No text moved away from the engine's place**: of every (text,
door) at the engine's line and column before, each stands there now. Fifteen texts reached it, at both doors: 12 in
`###HostedService` (corpus `c479a3de...` `[83:22]` and `c7342502...` `[63:19]`, the test suites' island written with a brace;
`mut16/m005314`, `m005320`, `m005326`, `m005327`, `m099136`, `m099150`, `m109901`, `m125589`, `m125590`; `mut/m001331`), the list's
refusal where lite answered a later refusal of the element, mostly the end of the input its skip of the broken list reached; 3
in `###DataSpace` (`mut16/m105818`, `id: 1` with no `;`, at the next key; `mut16/m108848`, `id:; 2;`; `mut/m026047`, `id: 1';`),
ruling 28's grammar. Thirty-three more kept their place in other words, the list's own refusal or the id's (`Unexpected token
'<EOF>'` for `expected BRACE_CLOSE but found EOF`, `Unexpected token '\'` for `unlexable input`): 32 HostedService, 1 DataSpace.
One moved from one line other than the engine's to another (`mut16/m109900`, an open quote in a post-validation's description:
the engine at the quote, lite at the list's next refusal where it answered a later one). No default value or mapping text
moved: no pinned text holds two refusals of one default or of one class mapping body.

**Lite looser** is unchanged: 575 corpus (571 S64, 3 S352, 1 S353), 282 mutated (16 per text; 280 S64, 2 S353), 74 mutated (4 per
text; all S64), 0 at every expression door: only the ruled exclusions.

**Ruling 27, per construct** (texts written by hand; a text at the engine's place counts when lite's first refusal is at the
engine's line and column, or both accept):

| construct | texts | at the engine's place before | after | what moved |
|---|---:|---:|---:|---|
| a property's default value | 122 | 76 | 107 | of an element's first token, where its `defaultValueExpression` stops and lite's own reading, the refusal placed first (`= x.y + (1 +);` at the `+`, `[3:23]`, where lite answered the `)`; `= 1 + ;` at the `+`, where it answered the `;`); a `::` the name's grammar has read refused at what follows it (S366) |
| a service test's value | 21 | 18 | 18 | nothing: its check already runs where the value is read, before lite's own reading of it, and every text with two errors in one element answers the engine's first (S363) |
| a mapping expression's path | 43 | 29 | 34 | the path is the expression's last token, so it already stood behind lite's reading of the expression and, by the body's second reading, behind a later syntax error of the body; in the engine's walker a `~src` or `~filter` given twice is checked before any expression is reparsed, and a set id that is a number is the class mapping parser's syntax error: both now stand before the path (`[4:3]`, `[8:11]`), a later expression's own walk refusal or path behind it, as before (S364, S365) |
| a HostedService's `postValidations` and `testSuites` | 16 | 8 | 16 | the list refused where it stands, no longer behind every later refusal of the element: the engine's grammar reads both lists in its one parse of the section (S362) |

The 15 default values left, each unchanged: 13 whose first refusal lite places otherwise than the engine whatever the order
(`^a::B(x = 1 +)`: lite at the `)`, the engine at the `+`; `a::b~1`; `^$(a=1)`), one a refusal of the engine's walk it answers
behind a later syntax error (`^$x(a=1) + (1 +)`), one lite stricter (`:: .x`). The 9 mapping texts left are `(1 +)`, the same
place difference.

**Ruling 28: a data space executable's id** is the engine's `executableId`, `(VALID_STRING | INTEGER | DECIMAL) SEMI_COLON` by its
lexer (S373): 144 texts -- every one of the 49 words the DataSpace, M3 and core lexers read before `VALID_STRING`, every token
kind, `VALID_STRING`'s digit-led and `$` spellings, decimals and floats, two tokens, a comment or a line break before the `;`,
an id cut by the end of the input, a second error in the element before and after it -- 93 looser before, 0 after, every one at
the engine's place after (45 before). Lite's and the platform's dialects read every id as before.

**The frozen questions** (every one answered again, the model door's answer the error list's first): the questions answered
differently from Java's frozen answer are the 760 listed before and 8 more asks, `ruling27`, at `pmcd` at `LEGEND_ENGINE` and
`document` (216 each); one listed pair re-answered. Each is a HostedService corpus text whose test suites' island is broken
(`#{` or `}#` written as a brace), so its element holds two refusals, the list's and the element's left open by it, and answers
the list's now, as the engine does: `479a3de2` at `[83:22]` and `73425021` at `[63:19]`, the engine's places, where lite answered
the end of the input; `08ef5041` and `b664f679` at the end of the input in the list's words (`Unexpected token '<EOF>'`), where
lite answered `expected BRACE_CLOSE`; `72b6e615` (listed by ruling 18) the same at its place. Lite's and the platform's
dialects, the expression doors and every other question: as before. `unlistable` 0. **Several errors** (ruling 23, through
`doors::pmcd_with_errors` at the three dialects): the corpus's 87,564 asks hold further errors in 1,234 (5,733 in all), the
mutated texts' 459,024 in 9,397 (16,803), as before; no panic, none out of order or placed at or before the first, the first
`pmcd`'s answer in every ask.

**Found while probing, not refused** (written by hand, not in the pinned inputs, not ruled): a default value's `x.all(1 + 2).y`,
which lite accepts and the engine refuses at the second `.` (`[3:28]`): its `allOrFunction`'s `.all(...)` takes a milestoning
date or variable alone, so `.all(1 + 2)` is the `propertyExpression` and nothing may follow it. Not changed, being of another
kind than the four checks (two refusals of one element, the earlier first): a refusal of the engine's walk that lite's reading
of a default value or a legacy service test's value raises stands before a later syntax error of the element, where the engine
answers the syntax error (`^$x(a=1) + (1 +)`: lite's `copy-with-update` `[3:17]`, the engine's `[3:25]`; a legacy test's
`-99999999999999999999` and a later key's error: lite's `For input string` `[2:8]`, the engine's `[22:1]`); a test suite's value
past a long is refused at the number (`Unexpected token`), where the engine's walker answers its section catch-all.

## After ruling 29 (2026-11-11)

Ruling 29's one open item of the never-looser rule, carried out at commit `18296bec6` (`docs/rust-port/PHASE2.md` section 4,
"After the closing"; register S375): a default value's `.all(...)` is its `allOrFunction` only with that rule's arguments,
else its `propertyExpression`, after which the value ends, so `x.all(1 + 2).y` is refused at the engine's dialect at the
second `.` (`[3:28]`). The same probes on the same pinned inputs, the same `doors` binary source, the engine's recorded
answers; before is lite at `bcf60ed8f` (the source tree's flip to `Arc` and its cost won back, `68a7a3ef3` and `4cb1a8df5`,
moved no answer: a mutation fuzz of 275,040 forms and every frozen question).

**Nothing moved**: every (text, door) of the harness, the corpus and both mutated sets in the same category as "After
rulings 27 and 28", every count above the same (corpus `pmcd` same place 3,256, different line 1,003, lite looser 575;
mutated, 16 per text, 100,058, 10,174, 282; 4 per text, 24,920, 2,539, 74), no text left the engine's place: no pinned text
holds a default value's `.all(...)` with other arguments and a property after it. Of the 69 default values written by hand
and asked of the engine's parser, 36 at the engine's place or both accepting before, 54 after, lite looser 15 -> 0; the 15
left are lite's own reading of the value refusing first, at another place (13) or where the engine accepts (2), each
unchanged. The frozen questions: answered as before, the difference list's 768 asks exactly.
