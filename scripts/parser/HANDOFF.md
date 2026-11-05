# Parser work — complete handoff

Kept as spec input for the Rust parser: the method (§1), the parser-parity analysis and the per-grammar upstream
quirks that no `.g4` states (§6.2–6.5), and the non-goals (§10). Read it with `README.md` (the harness design) and
`PERMISSIVENESS.md` (the leniency catalogue).

> Cut in the 2026-11 docs cleanup: the 2026-08 session-handoff banner (branch `test-corpus`, its unpublished merge to `main`). Full text: `git show de2745351:scripts/parser/HANDOFF.md`.

---

## 1. What this is, in one paragraph

legend-lite is a clean-room rewrite of legend-engine's Pure parser. The question "is the
rewrite complete?" was previously answered against corpora *harvested from legend-engine's
own tests* — which can only ever cover what upstream happened to write. This work answers it
against **the grammar itself**: every keyword legend-engine declares and a user can reach.
That difference is not academic. It found fourteen constructs legend-lite does not implement,
and legend-lite's own error messages describe them as *"corpus-censused"* — its surface was
scoped to what appeared in a corpus. A corpus cannot find what it was derived from.

---

> Cut in the 2026-11 docs cleanup: §2–4, the coverage and parity numbers (2026-08, and 2026-10-05 over release 4.145.0), the Maven and `python3` run steps, and the table of the harness's files (described in `README.md`). Full text: `git show de2745351:scripts/parser/HANDOFF.md`.

---

> Cut in the 2026-11 docs cleanup: §5, the upstream findings (eight NullPointerException sites in three grammars, F19 the unbalanced `###Connection`, F20 inverted multiplicities, F21 silently discarded fields, F22 dead `###FileGeneration` validation, declared-but-unreachable surface); they are recorded in `docs/UPSTREAM_FINDINGS.md` (its "Non-findings" section) and `PERMISSIVENESS.md`. Full text: `git show de2745351:scripts/parser/HANDOFF.md`.

---

## 6. The legend-lite queue

> Cut in the 2026-11 docs cleanup: §6.1, the 2026-08 queue of fourteen constructs lite did not yet parse. Full text: `git show de2745351:scripts/parser/HANDOFF.md`.

### 6.2 Thirty-four over-permissive negatives — and why only 15 are worth fixing

This is the most important analytical result in the whole effort, so it is spelled out.

Raw breakdown: 27 ordering/placement, 5 required field, 1 closed value set, 1 semantic
cross-check.

The 27 "ordering" cases look like one architectural gap: legend-lite parses key-value blocks as
unordered maps, while the engine's ANTLR rules encode order in the rule structure. **Enforcing
order globally would be a serious mistake** — the mutation data shows field order is genuinely
free in 110 of 114 cases, so a global rule would trade 27 harmless divergences for a much
larger number in the dangerous direction (rejecting models the engine accepts).

So the ordering cases were split by a measurement rather than a judgement.
`LiteParseMain --protocol-check` parses with legend-lite and then deserializes the result into
**the engine's own protocol classes**:

```
27 ORDER-class divergences
  19  produce VALID protocol — a document the engine could have produced.
      legend-lite is relaxed about keystroke order; nothing downstream can tell.
   8  produce protocol the engine CANNOT deserialize.
```

The 8 failures are `Unrecognized field "ingestMode"`, `"transactionScope"`,
`"actionIndicator"`, `"derivation"`, `"sink"`, `"endField"`, `"sourceDerivedDimension"`, and
`Could not resolve type id 'sourceSpecifiesFromDateTime'`. **All eight are Persistence**, and
none is really an ordering problem: they are legend-lite accepting a key into a node **kind**
that has no such field.

**Recommended work, in order:**

1. **Persistence permitted-keys check — 8 fixes, one mechanism. Start here.**

   (This step has since landed: today's Java `PersistenceSectionGrammar` has the `PERMITTED_FIELDS` map.)

   `core/src/main/java/com/legend/parser/section/PersistenceSectionGrammar.java` already has
   a `REQUIRED_FIELDS` map keyed by `slot/kind` (e.g. `"ingestMode/UnitemporalDelta"`), read
   by `validateNode`, which walks the generic node tree and enforces required-and-once. A
   `PERMITTED_FIELDS` map on the *same key* plus a rejection for any entry outside it closes
   all eight. Nothing new is needed structurally — `validateNode` already iterates
   `node.entries()` for the duplicate check, so the permitted test goes in that same loop.

   The permitted sets do not have to be guessed. legend-engine states each one in its own
   rejection message:

   | slot/kind | offending key | engine's permitted set |
   |---|---|---|
   | `persister/Streaming` | `ingestMode` | `['sink']` |
   | `targetShape/Flat` | `transactionScope` | `['partitionFields', 'modelClass', 'targetName', …]` |
   | `datasetType/Snapshot` | `actionIndicator` | `['partitioning']` |
   | `transactionMilestoning/BatchId` | `derivation` | `['batchIdInName', 'batchIdOutName']` |
   | v2 service-output target | `sink` (a v1 block) | `['keys', 'deduplication', 'datasetType']` |
   | relational `sourceFields` | `endField` first | `['startField']` — ordered pair |
   | relational unitemporal | `sourceDerivedDimension` | `['processingDimension']` |
   | validity `derivation` | `SourceSpecifiesFromDateTime` | `['SourceSpecifiesInDateTime', …]` |

   > Cut in the 2026-11 docs cleanup: the hand-run `java -cp …/cp.txt perf.ParseMain` command (2026-08) that regenerated this list. Full text: `git show de2745351:scripts/parser/HANDOFF.md`.

   Confirm a fix with `LiteParseMain --protocol-check` on the same file: it must go from
   `Unrecognized field …` to `ok`. That is the real acceptance test, not the parity count.

   This is the only group that corrupts anything today — legend-lite currently emits JSON no
   downstream engine consumer can deserialize.
2. **Five required-field entries** — map entries in validation code that already exists.
3. **Two one-liners** — `Unknown database type 'Frobnicate'`, and the execution-env key
   space check.
4. **Then the 14 missing constructs** (§6.1).
5. **Do not** chase the remaining 19 ordering cases. High cost across seventy grammars, zero
   downstream effect, and a real risk of over-correcting into the worse failure direction.

### 6.3 Quarantined divergences (8) — legend-lite is RIGHT, the reference is wrong

In `parity-quarantine.tsv`, each with a reason:

- 6 mutants: `drop-final-delimiter` on `###Connection` files. That is **F19** — legend-lite
  requires the closing brace and legend-engine does not. Found from one side by mutating engine
  fixtures, confirmed from the other by parity, using two harnesses that share no code.
- 2 negatives: the Persistence NPE cases. legend-engine **crashes**; legend-lite parses
  cleanly. Matching parity would mean reproducing a NullPointerException.

---

## 6.4 Do NOT be stricter than legend-engine — read `PERMISSIVENESS.md`

Parser parity fails in two directions and only one is obvious. Accepting what the engine
rejects breaks in production. **Rejecting what it accepts looks like rigour** and surfaces as
a model that has worked for two years suddenly failing to load, reported as "your parser is
broken" rather than "your model was always malformed".

`PERMISSIVENESS.md` is the full catalogue, derived from 1636 mutants. The rules a rewrite is
most likely to get wrong:

- **Field order is free** — 110 of 114 sibling swaps accepted. Two exceptions corpus-wide:
  `ExecutionEnvironment` needs `mapping` before `runtime`; MongoDB needs `validationAction`
  before `validationLevel`.
- **Section order is free** — all 15 swaps accepted; resolution is by name, not position.
- **A file with no `###` header parses as Pure.** `Class fx::A {…}` alone is valid; a
  `Database` with no header is rejected with Pure's alternatives list.
- **`import` must precede every element in its section** — the one structural ordering rule
  that IS enforced.
- **Date literals are not validated at parse time.** `%2024-13-45` parses; the *compiler*
  says `Invalid month: 13`. Validating dates in a lexer is an obvious move and would be wrong.
- **The same element may be declared twice**, and a class may declare the same property twice.
- **`String[2..1]` and `Class A extends B, C` survive compilation** (F20) — a rewrite
  rejecting them at parse is stricter than the engine at *any* stage.
- **Almost every keyword is a legal property name** — `Class`, `let`, `all`, `toBytes`,
  `native` all work; only `true`/`false` do not.

## 6.5 Per-grammar quirks that exist nowhere else

Found during the tier-2 sweep, recorded here because they are not derivable from any `.g4`
and would otherwise be lost:

- **Validation is wildly inconsistent between sibling grammars.** Snowflake's
  `permissionScheme` is walker-checked against a closed set; `accountType` in the same element
  is unvalidated (upstream's own roundtrip test feeds it `BadOption`). MemSql's `port` is a
  STRING run through `Integer.valueOf` at parse time, so `port: 'abc'` is rejected; Databricks'
  `port` is the same field name, same type, and is not coerced. `ownership` is optional for
  `BigQueryFunction` and required for the otherwise-identical `MemSqlFunction`.
- **Identifier-only reachability is the single most common reason a keyword looks
  uncoverable.** `stage` (BigQueryFunction, MemSqlFunction),
  `SnowflakeUDFDeploymentConfiguration`, Deephaven's `tables`/`columns`/`columnDefinition`,
  MongoDB's `~distinct`/`~primaryKey`/`debug`, Persistence's `Notifier`/`serviceOutput`/
  `target`, `relation`, `FileGeneration`. In each case the rule that looks like it consumes
  the token as a field is absent or uses a different token, so the only way to type it is to
  NAME something with it. Worth testing rather than skipping — keyword-as-identifier is a
  classic parser defect.
- **Section names are not guessable from the element or grammar name.**
  `BigQueryFunctionGrammarParserExtension` registers `###BigQuery`;
  `DataQualityGrammarParserExtension` registers `###DataQualityValidation`;
  `RelationalMapper` lives in `###QueryPostProcessor`, not `###RelationalMapper`. Always read
  the `NAME` constant in the `*ParserExtension.java`.
- **Deephaven `appMultiplicity` accepts only `[n]` and `[*]`** — `[0..1]` is unreachable
  because CoreLexer lexes `..` as a single token.
- **MongoDB schemas use double-quoted strings**, and `jsonSchema:` is Jackson-deserialized at
  parse time, so `"bsonType": "object"` is mandatory.
- **ServiceStore requires `binding: X;;`** — a double semicolon.
- **Persistence's `.g4` is far weaker than its walker.** Every ingest-mode sub-block is `(x)*`
  in the grammar; the real requirements live in `PersistenceParseTreeWalker.visit*`. A parser
  generated straight from the grammar accepts a great deal the engine refuses — which is
  precisely why 16 of the 34 over-permissive cases are Persistence.
- **`derivation` is optional under `transactionMilestoning` and required under
  `validityMilestoning`** — an asymmetry invisible in upstream's own tests, which always write
  it.

### Upstream facts found while building

- **A composite token has no literal name.** `CONSTRAINT_OWNER: '~owner' CONSTRAINT_SEPARATOR;` has no entry in
  ANTLR's `Vocabulary`, so a tool that checks the engine's keywords against a lexer's vocabulary sees five Domain
  keywords as missing although they parse. Compare simple-literal tokens only.

> Cut in the 2026-11 docs cleanup: §7–9, the traps (stale Maven builds of `core`, missing grammar extensions on the runner's classpath, census-versus-jar skew), the measurement-bug table, §8a the execution corpus's feature-density state, and the open decisions (merging `test-corpus` to `main`, reporting the findings), 2026-08 to 2026-10. The lasting lessons are in `README.md`. Full text: `git show de2745351:scripts/parser/HANDOFF.md`.

---

## 10. Deliberate non-goals

- **Coverage is not the constraint any more.** The surface is at 100% of what is reachable.
  Adding fixtures measures nothing new; the binding constraint moved to what legend-lite does
  about it.
- **Parse, not compile.** A fixture proving `~distinct` parses does not need a runtime, a
  connection and seeded data. Demanding compilation would make every fixture ten times the size
  and make coverage depend on the compiler.
- **Verdicts, not message text.** Two parsers can refuse the same input for the same reason and
  word it differently.
