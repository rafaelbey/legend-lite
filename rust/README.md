# rust/

The Rust port of legend-lite's compiler (docs/rust-port/TRIAGE.md, docs/rust-port/ROADMAP.md). Phase 0 holds the
references the port is judged against; the Cargo workspace joins in phase 0 step 5.

- `BASELINE.tsv`: every test target of `bazel test //...` on the pristine tree (upstream `main` at `cf73e6ef7`,
  2026-11-02, macOS arm64): 508 passed, 2 skipped as incompatible with the platform. A phase lands only when every
  target that passed here still passes, or is a row of `RETIRED_TESTS.md`.
- `oracles/jvm/`: the JVM's answers that were build outputs, frozen before any of them can be produced by Rust
  (otherwise a differential would compare Rust with Rust). `MANIFEST.tsv` names each one's producing target and the
  SHA-256 of its uncompressed text; the two large ones are zstd-compressed (`zstd -d`).
  - `jvm_answers.txt`: the planner corpus (`wasm/corpus`), answer for answer; `//python:bindings_test` reads it too.
  - `zone_jvm.txt`: `LiteralSpelling.inZone` over the tz database.
  - `conformance_jvm.txt.zst`: the JDK class library as legend-lite uses it (`wasm/src/main/java/conformance`):
    doubles and floats printed and parsed, BigDecimal and its hash codes, BigInteger, integers, characters,
    strings, format, regex, codecs, time text and zones, hash-map orders and string hash codes.
  - `tab-round-trip.jsonl.zst`: 9,424 sources from the engine corpus and lite's projects, with the JVM's
    text→JSON→text→JSON answers.
- `oracles/java-compat/`: the Java facts `conformance_jvm` does not pin, from `JavaCompatExtras.java` run on the
  build's JDK (25.0.4, tzdata 2026b): every code point's character classes, as runs, and `ZoneId.SHORT_IDS`.
- `tools/mirjson-roundtrip-probe.patch`: the probe that proved the SQL-tree seam (docs/rust-port/PHASE0.md §4).
  `git apply` it, run any suite with `--test_env=LEGEND_MIRJSON_ROUNDTRIP=1 --nozip_undeclared_test_outputs`, and
  read `mirjson-roundtrip-*` in each test's outputs; never commit it to the sources. Phase 1's differential is the
  same probe with the second render made by Rust.
