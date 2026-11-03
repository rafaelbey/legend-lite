# Rust port — phase 0 findings (2026-11-02)

[`ROADMAP.md`](ROADMAP.md)'s phase 0, measured on upstream `main` at `cf73e6ef7` (macOS arm64): steps 1–4 (§1–4) and
steps 5–6 (§5). No Java was deleted.

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

**Every one of the 28 differences is Java's own, not the JSON's.** In each, what was read back is structurally equal
to what was written. For the 25 queries, rendering the *original* tree a second time was measured to differ in the
same way. The statement probe had no such re-render check; its 3 differences are the same `__b<n>` renumbering
(`0` → `4`, `0` → `12`) inside an equal statement, so the same cause is inferred, not measured. The renderer does not
give the same answer twice:

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

## 5. Rust in the build, and the JVM calling it

**The toolchain.** `rules_rust` 0.75.0 in `MODULE.bazel` with its default compiler (Rust 1.99.1, edition 2024);
`rust/` is also a Cargo workspace for the edit loop, its `rust-toolchain.toml` pinned to the same compiler. No crate
comes from crates.io yet, so there is no `crate_universe`. `MODULE.bazel.lock` gains only registry hashes
(`rules_rust`; `apple_support` 1.24.1 beside the 1.24.2 already resolved; `bazel_features` 1.42.1 → 1.50.0) and no
per-platform extension entry, so `--lockfile_mode=error` should hold on every CI platform -- only CI can say.

**The library** (`rust/lite-ffi`) is the contract alone: `lite_version`, `lite_echo`, and two probes that refuse and
panic on purpose. Answers are folded `OK\n<answer>` or `ERR\n<kind>\n<phase>\n<line:col>\n<message>`, kinds Rust's
own; every export runs under `catch_unwind`, so a panic is an `INTERNAL` refusal, never an abort of the host;
`lite_free` and `lite_unfreed` are libcompiler's.

**The shim** (`//core:ffi`, `com.legend.ffi.Lite`) calls it through `java.lang.foreign`. Each export is a typed
interface over its downcall (`MethodHandleProxies`), so a call declares no `Throwable` and the shim needs no broad
catch -- core's `ErrorShapeGuardrailTest` pins broad catches per file at their reviewed counts, and a first draft that
caught `Throwable` around `invoke` failed it. `//core:ffi_test` holds the round trip against the library Bazel builds:
text there and back (astral, multi-line, 1 MB), refusals with kind, phase and position, a panic answered, NUL refused
before the call, every answer freed. It pins one Java-side fact for the port to decide (TRIAGE C2): a lone surrogate
reaches the library as `?`, the JVM's UTF-8 encoder's replacement.

**The first Java spelling** (`rust/lite-javacompat`): `double_to_string` writes all 86,285 `Double.toString` answers
of the frozen JDK snapshot (`rust/oracles/java-compat/double-tostring.tsv`: its edges, random and short-decimal
families). Rust's own shortest form differs from the JDK's choice in two classes. Where the fewest digits is one, the
JDK's specification takes two (`4.9E-324` where Rust's shortest is `5e-324`): handled first, and with it in place 13 of
the 86,285 still differed -- all ties between two equally close shortest decimals, which Rust breaks the other way
(2⁻²⁵ is `2.9802322387695312E-8` to the JDK, `…313` to Rust). The JDK's choice, both classes at once, is the
correctly rounded decimal of the shortest length, at least two digits, when it reads back as the same double.

**What only CI can check.** The Rust targets are built and tested here on macOS arm64 only; Linux (the hermetic LLVM
toolchain links them) and Windows (MSVC; the test reads the library from `$(rlocationpaths)` because the DLL comes with
its import library) are CI's. `//tools/guards:markdown_inputs_test` (no Markdown among any test's inputs) is skipped on
macOS: the new tests list their data file by file, none of it Markdown.

**Where it stands.** `bazel test //...` on this state: 514 targets, 512 passed and the same 2 skipped; no target of
`rust/BASELINE.tsv` changed its status, and the four new ones (`//core:ffi_test`, `//rust/lite-ffi:lite_ffi_test`,
`//rust/lite-javacompat:lite_javacompat_test`, `//rust/lite-javacompat:double_to_string_test`) pass, in the core
lane. On the same state the every-test-in-a-lane query is empty, `bazel build --nobuild --config=bazel10 //...`
analyzes all 1,388 targets, and `cargo test` and `cargo clippy --all-targets -- -D warnings` are clean; the checks
gate passed on steps 5–6.
