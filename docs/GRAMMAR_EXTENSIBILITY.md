# Grammar extensibility: internal overlays and the section seam

Context: legend-engine is deployed in-house with an internal overlay — closed-source
jars that add grammars (and their compilers) on top of the open-source engine. If
legend-lite is ever to sit under that deployment, those internal grammars must be able
to plug in without forking legend-lite. This note records the architecture answer,
grounded in the SPI seam proof (first `SpiSeamProofTest`; today a claim of `parser-equivalence`'s `CorpusSweepTest`).

> Cut in the 2026-11 docs cleanup: "Reproducing the seam proof", the Maven recipe for `SpiSeamProofTest` and its ratchet counts (that test is gone; its seam claim is now one of the claims of `parser-equivalence`'s `CorpusSweepTest`). Full text: `git show de2745351:docs/GRAMMAR_EXTENSIBILITY.md`.

## How the engine does it (the model to mirror)

The engine's parser is a **section dispatcher**, not a grammar. `###Name` splits the
file into sections; `visitSection` looks the name up in a registry assembled from
`PureGrammarParserExtension.getExtraSectionParsers()` — **extensions are consulted
before built-ins**, which is exactly what lets legend-lite's `LegendLiteSectionParser`
take over `###Pure` inside a real engine. Extensions are discovered by `ServiceLoader`
from the classpath: the internal overlay is nothing more than jars that register extra
`SectionParser`s (plus paired `CompilerExtension`s) for their proprietary sections.

The contract per section parser:

- **in**: `SectionSourceCode` — the raw section text plus walker offsets
  (`sourceId`, `lineOffset`). Raw text, not tokens: each grammar lexes itself, so
  extensions never have to share a lexer.
- **out**: protocol elements (`PackageableElement`) pushed through a consumer, plus a
  `Section` record (imports, element paths). The engine's currency is the JSON
  protocol; its SDLC flow compiles from JSON-deserialized protocol as a matter of
  course, which is why a bridge that emits byte-parity JSON is correct by construction.

A second-order seam exists INSIDE expressions: embedded island content (`#>{…}#`
accessors, embedded data in function test suites) is routed through separate embedded
parser/data registries. The seam proof surfaced this directly — the extension-less
vanilla baseline rejects such files with "Can't find an embedded Pure parser" /
"Unknown embedded data type" while a production classpath accepts them.

## Consequence 1: drop-in at the engine seam composes with overlays for FREE

If legend-lite's deployment mode is "replace the engine's DomainParser via the SPI"
(the seam proof configuration), there is nothing to design: internal grammars keep
registering their own sections beside ours, the engine dispatcher composes them, and
neither side knows about the other. The overlay problem is already solved by the
engine's architecture; we just have to not break it — i.e. keep our extension scoped
to the sections we actually own.

## Consequence 2: standalone legend-lite needs the same seam, deliberately

Today legend-lite hard-codes its section routing: the lexer knows `LEXABLE_SECTIONS`
and raw-skips unknown sections; `###Pure`/`###Mapping`/`###Relational` route to
hand-written parsers. That is the right thing for a closed system and the wrong thing
for an overlay host. The target shape, in dependency order:

1. **Section registry (pure refactor).** A `SectionGrammarRegistry` mapping section
   name → grammar module. Built-ins (`Pure`, `Mapping`, `Relational`, …) register
   through the SAME registry as third parties — the dogfooding rule that keeps the
   plug-in path honest. Unknown sections stop being "raw-skip" and become "no grammar
   registered" — an explicit, reportable state.
2. **A thin `legend-lite-spi` artifact.** The interface an internal jar implements:
   roughly `SectionGrammar { String name(); void parse(SectionSource src,
   ElementSink out); }` with `SectionSource` = raw text + source offsets (mirroring
   `SectionSourceCode` — raw text so foreign grammars need not adopt our lexer).
   Discovery via `ServiceLoader`. Keep this module small and stable; internal code
   must never depend on core internals.
3. **An opaque-element carrier.** `Protocol.Element` is sealed — deliberately. Foreign
   elements enter as ONE new variant (an opaque payload: the extension's protocol
   JSON), which core can index, name-resolve against, and route but never needs to
   look inside — because compiling a foreign element is ALSO the extension's job. The
   plug-in unit is a **language module**, not a grammar: section parser + compiler
   hook (+ lowering rules if it reaches execution), mirroring the engine's paired
   `PureGrammarParserExtension`/`CompilerExtension`.
4. **Embedded registries (only when needed).** The inner seams — embedded island
   parsers and embedded test-data types — get their own registries the first time an
   overlay grammar actually needs them.

Shadowing policy: the engine lets extensions override built-in sections (that is what
makes the drop-in possible at all). Keep the same rule — extensions win — and rely on
tests, not policy, to catch abuse.

## Verification pattern (reusable by overlay owners)

The differential harness is grammar-agnostic: parse the same corpus through the
reference implementation and the candidate, compare protocol bytes, ratchet the match
count. An internal team overlaying legend-lite can point the same pattern at THEIR
reference (the internal engine build) as the spec for their sections. Two attributed
divergence classes to expect, both proven benign here:

- **engine JSON-asymmetry**: fields the engine serializes but its own deserializer
  drops (e.g. ColSpec classInstance `multiplicity`) — invisible to any JSON-first
  consumer; detect by round-tripping the reference's own output.
- **leniency census**: files the reference rejects that the candidate accepts. These
  cannot exist in a reference-guarded store, so they cannot change drop-in behavior —
  but count them and ratchet DOWN only (`MAX_LENIENT_ACCEPTS`).
