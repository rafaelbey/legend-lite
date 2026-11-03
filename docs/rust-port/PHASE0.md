# Rust port — phase 0 findings (2026-11-02)

Steps 1–4 of [`ROADMAP.md`](ROADMAP.md)'s phase 0, measured on upstream `main` at `cf73e6ef7` (macOS arm64). No Java
was deleted. Steps 5–6 (the Cargo workspace and the FFI shim) are next.

## 1. The baseline

`bazel test //...` on the pristine tree: **508 of 510 test targets passed, 2 skipped** as incompatible with the
platform (`//tools/guards:classpath_test`, `//tools/guards:markdown_inputs_test`). The per-target table is
[`rust/BASELINE.tsv`](../../rust/BASELINE.tsv); a phase lands only when every target that passed there still passes, or
is a row of `rust/RETIRED_TESTS.md`.

## 2. The JVM's answers, frozen

The answers that were build outputs are committed under [`rust/oracles/jvm/`](../../rust/oracles/jvm), each with its
producing target and the SHA-256 of its text (`MANIFEST.tsv`): `//wasm:jvm_answers` (the planner corpus; also
`//python:bindings_test`'s reference), `//wasm:zone_jvm`, `//wasm:conformance_jvm` (22 MB, zstd) and
`//parser-equivalence:tab_round_trip` (9,424 sources with their text→JSON→text→JSON answers; 48 MB, zstd).

## 3. The Java-compatibility golden sets

`//wasm:conformance_jvm` already pins most of what the port needs from the JDK: `Double`/`Float` printed and parsed,
`BigDecimal` (and its hash codes), `BigInteger`, integers, characters, strings, `format`, regex, codecs, time text and
zones, hash-map iteration orders and `String.hashCode` — so it is frozen rather than rewritten. What it does not pin,
[`JavaCompatExtras.java`](../../rust/oracles/java-compat/JavaCompatExtras.java) adds, run on the build's JDK (25.0.4,
tzdata 2026b): the character classes of every code point (3,182 runs) and `ZoneId.SHORT_IDS` (28).

## 4. The SQL-tree seam holds

The question: can a dialect be handed its input as JSON — `MirJson` text — and answer exactly what it answers when
handed the Java objects? A probe ([`rust/tools/mirjson-roundtrip-probe.patch`](../../rust/tools/mirjson-roundtrip-probe.patch),
never committed to the sources) re-rendered every statement the test suites render after a trip through JSON text,
through the dialect's public entry points, and compared. It rode on 51 test targets: core's suites and its SQLite and
H2 lanes, the corpus differential, the stress suites on DuckDB and H2, the Postgres arm, PCT on DuckDB, H2 and Postgres,
PCT channel B, and spec.

| | rendered | identical after the JSON trip | different | errors |
|---|---|---|---|---|
| queries (`SqlQuery`) | 33,478 | 33,453 | 25 | 0 |
| statements (`SqlDdl`, `SqlDml`) | 5,608,721 | 5,608,718 | 3 | 0 |

**Every one of the 28 differences is Java's own, not the JSON's.** The tree read back is structurally equal to the
one written in each case, and rendering the *original* tree a second time differs in the same way — the renderer does
not give the same answer twice:

- **Fresh-name counters that outlive a render.** `SQLite`'s `binds` (`__b<n>`, `SQLite.java:1373`), `Snowflake`'s
  `fresh` (`__f<n>`, `Snowflake.java:402`) and `CommonSubexpressions`' `fresh` (`__cse<n>`, `:39`) are instance state
  never reset per render, so the SQL a dialect writes depends on how many statements that dialect object wrote before
  it. No test pins those names (every suite passes either way).
- **Identity-hash tie-breaks.** `CommonSubexpressions` picks the largest repeated subtree in `IdentityHashMap`
  iteration order (`:93`), so between equal-size candidates the choice follows object addresses (TRIAGE C5).

`SqlDdl` and `SqlDml` had no JSON; `MirJson.ddl`/`readDdl` and `MirJson.dml`/`readDml` now carry them, held by
`MirJsonStatementsTest` (every variant, through text and back, equal and byte-stable).

### What it means for the port

- The back seam is sound: phase 1 can hand the Rust dialects `MirJson` and compare their SQL with Java's.
- The Rust renderer starts every render from fresh state, so it is deterministic by construction. Where Java's answer
  depends on render history, the differential must compare a Rust render with Java's *first* render of that tree on a
  fresh dialect — the probe's method. Decided here: counters reset per render; ties broken by a fixed structural rule.
- Per-render state lives in the dialect objects (`SQLite`'s `roots` and its identity map of root `SELECT`s, pushed in
  its `render` override). A Rust dialect keeps it in a per-render context passed down the writer, never in the dialect.
- The probe is the phase-1 differential's harness: replace its second render with the Rust call.
