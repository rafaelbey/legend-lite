# rust/

The Rust port of legend-lite's compiler (docs/rust-port/TRIAGE.md, docs/rust-port/ROADMAP.md): its crates, and the
references the port is judged against.

Bazel builds the crates (`rules_rust`, each crate's `BUILD.bazel`) and its targets are the gate:
`bazel test //rust/... //core:ffi_test`. `rust/` is also a Cargo workspace for the edit loop (`cargo test`,
`cargo clippy --all-targets -- -D warnings` from here); `rust-toolchain.toml` pins cargo to the compiler `MODULE.bazel`
names. Change both together. Bazel optimizes the crates in every build, fastbuild included (`MODULE.bazel`'s
`rust.toolchain` `opt_level`, the user's ruling of 2026-11-06): the tests run the library that ships. Cargo's dev
profile does not.

- `lite-ffi/`: the compiler as a C library -- the exports `//core:ffi` (`com.legend.ffi.Lite`) calls through
  `java.lang.foreign`, and Python's bindings will once `libcompiler` is Rust. Phase 0: the contract only.
- `lite-javacompat/`: the Java spellings legend-engine's output carries (`double_to_string`), each held to the JDK's
  frozen answers.
- `lite-json/`: legend-lite's JSON (json/'s `Json.java`): insertion order, numbers kept as written, Java's escapes.
- `lite-sql/`: the SQL tree (the MIR) and its JSON, `MirJson`, held to every statement of `oracles/mir/`.
- `lite-dialect/`: the SQL dialects -- the shared renderer as a trait, the writer, the pass pipeline, DuckDB -- held to
  Java's answers by the differential over `oracles/mir/` (`PARITY.md` measures where it stands).
- `lite-front/`: the front end -- the lexer, the expression parser and the model parser with every section grammar
  (ported from core/'s `com.legend.lexer` and `com.legend.parser`, which were deleted on 2026-11-06 once every text
  door answered from here: docs/rust-port/PHASE2.md §4 step 6), and the protocol emitter that writes legend-engine's
  JSON from their records (core/'s `ProtocolEmitter` and its family, which stay Java until phase 3 for their
  record-sourced callers), over a text's UTF-16 units, held to Java's every answer -- frozen in `oracles/front/` before
  the Java was deleted -- by the three differentials (`PARITY.md` measures where it stands). `src/probes.rs` answers
  what `//parser-equivalence`'s gates and the prelude generator asked of the Java parser's internals.
- `lite-base/`: phase 3's foundation (the layer `base`; docs/rust-port/PHASE3_CRATES.md): the per-query bump arena with
  its keep pool and the model's arena, shared between threads (`src/arena.rs`, the one module with `unsafe`), and
  identity -- Java's `==`, `IdentityHashMap` and `Object` origins for arena records and `Arc` nodes (`src/identity.rs`,
  the one module that turns a pointer into an address).
- `lite-types/`: the type family (core's `compiler_element_type` layer): `Type<'a>`, one `Copy` family with a lifetime
  in either arena, `Multiplicity`, `PrecisionDecimal`, `PlatformTypes`, held to Java's answers frozen in
  `oracles/types/` by `tests/type_differential.rs`.
- `lite-guards/`: the port's guards, tests only: the crate graph held to `crate-layers.txt` (Cargo and Bazel both) and
  that file to tools/deps/core-layers.txt, and the sources to the arena ruling's rules (PHASE3_CRATES.md §3).
- `lite-error/`: the user-facing error taxonomy (core's `error` layer): `LegendCompileException`'s phases and classes.
- `lite-model/`: the model (core's `model` layer): its records, generated from the Java's (`src/records.rs`,
  `//core:model_records_rs`), `Arc` with `Arc<[T]>` lists, and the converters from the protocol records
  (`ModelFromProtocol` and its family), subtyping over declarations.
- `lite-builtin/`: the platform's declarations and the text's door (core's `builtin` and `door` layers): the native
  catalog (`src/natives.rs`, generated beside `Pure.java` by `//spec:gen_natives_rs`), the dynafunctions
  (`src/dyna_fn.rs`, `//spec:gen_dynafn_rs`), the families, the prelude, the system metamodel; held to Java's door on
  every model text of `oracles/front/` (`tests/model_differential.rs` over `oracles/model/`) and to Java's tables
  (`tests/builtin_tables.rs` over `oracles/builtin/`).
- `lite-platform/`: the platform (core's `platform` layer): the declarations by id and what implements each, held to
  Java's tables (`tests/platform_tables.rs` over `oracles/builtin/`).

- `BASELINE.tsv`: every test target of `bazel test //...` on the pristine tree -- since 2026-11-11 upstream `main` at
  `c4a39e06d`, the merge point (PHASE3_SHIM_DESIGN.md §10, ruling 14; macOS arm64): 552 targets, 550 passed, 2 skipped
  as incompatible with the platform. A phase lands only when every target that passed here still passes, or is a row
  of `RETIRED_TESTS.md`. `BASELINE-cf73e6ef7.tsv` is the first baseline, `main` at `cf73e6ef7` (2026-11-02): 508
  passed, the same 2 skipped; it judged the phases before the merge (docs/rust-port/PHASE0.md §1).
- `oracles/jvm/`: the JVM's answers that were build outputs, frozen before any of them can be produced by Rust
  (otherwise a differential would compare Rust with Rust). `MANIFEST.tsv` names each one's producing target and the
  SHA-256 of its uncompressed text; the two large ones are zstd-compressed (`zstd -d`).
  - `jvm_answers.txt`: the planner corpus (`wasm/corpus`), answer for answer; `//python:bindings_test` reads it too.
  - `zone_jvm.txt`: `LiteralSpelling.inZone` over the tz database.
  - `conformance_jvm.txt.zst`: the JDK class library as legend-lite uses it, generated by
    `//rust/oracles/conformance:conformance_jvm` (moved from `//wasm` 2026-11-02, regenerating byte-identical):
    doubles and floats printed and parsed, BigDecimal and its hash codes, BigInteger, integers, characters,
    strings, format, regex, codecs, time text and zones, hash-map orders and string hash codes.
  - `tab-round-trip.jsonl.zst`: 9,424 sources from the engine corpus and lite's projects, with the JVM's
    text→JSON→text→JSON answers.
- `oracles/front/`: the front end's oracle (docs/rust-port/PHASE2.md §3): the questions the test suite asks the
  parser's text doors (`questions.jsonl.zst`), the records' schema, and Java's answers to every question frozen before
  the Java lexer and parser were deleted -- `tokens.tsv.zst`, `answers.tsv.zst`, `engine.tsv.zst`, one line of hashes
  per question in the questions file's order, which lite-front's three differentials read. `MANIFEST.tsv` names each
  one's producing target, commit and SHA-256, and where Java's full answers are archived, outside git.
- `oracles/java-compat/`: the Java facts `conformance_jvm` does not pin, from `JavaCompatExtras.java` run on the
  build's JDK (25.0.4, tzdata 2026b): every code point's character classes, as runs, and `ZoneId.SHORT_IDS`. And
  `double-tostring.tsv`, the `Double.toString` families of `conformance_jvm` extracted to plain text so a test reads
  them with no decompressor (`<bits in hex>\t<text>`, 86,285 cases).
- `oracles/tzdb/`: the JDK's time-zone database as `java.time.zone.ZoneRules` holds it, for the engine text's
  date-time constant shifted to a connection's zone: `TzdbData.java`, a build action on the build's JDK
  (`//rust/oracles/tzdb:tzdb_data`), writes `tzdb.bin` (every region and short ID, each rule set's own arrays; the
  dialect library embeds it) and `zone-golden.tsv.zst` (java.time's answers over it, the differential's); and
  `engine-text-zones.tsv`, Java's own `EngineStyleH2` renders of a shifted constant. `MANIFEST.tsv` names each one's
  producer, JDK, tzdb version and SHA-256.
- `tools/mirjson-roundtrip-probe.patch`: the probe that proved the SQL-tree seam (docs/rust-port/PHASE0.md §4).
  `git apply` it, run any suite with `--test_env=LEGEND_MIRJSON_ROUNDTRIP=1 --nozip_undeclared_test_outputs`, and
  read `mirjson-roundtrip-*` in each test's outputs; never commit it to the sources. Phase 1's differential is the
  same probe with the second render made by Rust.
- `oracles/identity/`: phase 3's oracle, frozen while the Java compiler exists (docs/rust-port/PHASE3.md step 0, re-frozen
  on the merged tree on 2026-11-11): per question, the hits and misses of the identity tables and the outcomes of the
  identity comparisons (`hits.tsv.zst`), and each lowering's `MirJson` (`mir-ids.tsv.zst`, `mir-store.tsv.zst`), the
  corpora's answer for a Rust CLI. `MANIFEST.tsv` says what each column is.
- `oracles/plan/`: the plan oracle, made with it (the same rows, keyed alike): per question, the id of each plan Java made,
  a product query's `ExecutionPlan` (PlanJson v31) and a test body's `TestPlan` (TestPlanJson v11), the id being the SHA-256
  of the JSON's UTF-16 units once run-specific values are masked (`plan-ids.tsv.zst`), and every execution plan's text
  (`execution-plans.tsv.zst`). The host judge's test plans' texts are in git deduplicated (`test-plans-host.tsv.zst`,
  `test-plan-values.tsv.zst`, its index and frame table: each JSON value of 8 KB or more that repeats stored once); the
  database judge's are archived outside git. `MANIFEST.tsv` says where, with their hashes.
- `oracles/types/`: the type family's answers (`type-facts.tsv`, 26,429 lines: every platform type, every operation of
  core's `compiler_element_type` layer that needs no later layer, every refusal by its exception and message), made by
  `TypeOracle.java` on core's built jars (its header says how) before the Java is deleted; `MANIFEST.tsv` names its
  tree and SHA-256.
- `oracles/model/`: Java's text door's answer to every model text of `oracles/front/` at every dialect asked (87,564
  asks: the canonical text's SHA-256 per ask, `answers.tsv.zst`), made by core's `frontoracle/ModelAnswers.java` before
  the Java is deleted; the full texts archived outside git (`MANIFEST.tsv` says where, with its hash).
- `oracles/builtin/`: the platform's tables as Java holds them (`builtin-tables.tsv`, `platform-tables.tsv`: one fact
  per line), made by `BuiltinAnswers.java` beside them on core's built jars; and `SubtypeProbe.java`, which asks the
  live Java's boot model every subtype pair (not frozen: ruling 22).
- `tools/test-plan-texts.py`: the format of the deduplicated test plans, its writer (from the archive), its reader (one
  plan's exact text by its id, `read rust/oracles/plan <id>`) and the check that every plan rebuilds to its id. A tool,
  not a probe: it touches no source, and reading the oracle needs it.
- `tools/identity-hits-probe.patch`: the probe that made both. `git apply` it to a clean tree (the patch is the switch),
  run, read `identity-hits-*`, `identity-mir-*` and `identity-plan-*` from each test's outputs and each corpus pass's scratch
  directory, `git apply -R` as soon as the run is over; never commit it to the sources.
  `tools/identity-control-tap.patch` is its control: the SQL tap and a plan tap alone, no counting, to show that counting
  changes neither the statements nor the plans (`identity-sql-*`).
