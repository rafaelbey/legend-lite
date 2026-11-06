package com.legend.equivalence;

import com.fasterxml.jackson.databind.JsonNode;
import com.fasterxml.jackson.databind.ObjectMapper;
import com.fasterxml.jackson.databind.node.ObjectNode;
import org.finos.legend.engine.language.pure.grammar.from.SectionSourceCode;
import org.finos.legend.engine.language.pure.grammar.from.extension.PureGrammarParserExtension;
import org.finos.legend.engine.language.pure.grammar.from.extension.SectionParser;
import org.finos.legend.engine.protocol.pure.m3.PackageableElement;
import org.finos.legend.engine.protocol.pure.v1.model.packageableElement.section.ImportAwareCodeSection;
import org.finos.legend.engine.shared.core.ObjectMapperFactory;

import java.util.List;

/**
 * THE SEAM PROOF: legend-lite parsing behind the engine's own {@code SectionParser}
 * extension point. The bridge is deliberately dumb — parse with the STRICT drop-in
 * surface, emit the byte-parity JSON, shift spans by the section's walker offsets, and
 * let the ENGINE'S ObjectMapper build its protocol objects. Correctness rides entirely
 * on the proven byte parity plus the engine's own JSON-first architecture (its SDLC flow
 * compiles from JSON-deserialized protocol as a matter of course).
 */
final class LegendLiteSectionParser {

    private LegendLiteSectionParser() {
    }

    /** Exact-decimal tree reading: floats become BigDecimal-backed nodes WITHOUT the
     *  default node factory's stripTrailingZeros normalization, so a source {@code 10.10}
     *  survives readTree → treeToValue byte-exactly (engine's CDecimal deserializes via
     *  {@code new BigDecimal(node.asText())}). */
    private static final ObjectMapper MAPPER =
            ObjectMapperFactory.getNewStandardObjectMapperWithPureProtocolExtensionSupports()
                    .enable(com.fasterxml.jackson.databind.DeserializationFeature
                            .USE_BIG_DECIMAL_FOR_FLOATS)
                    .setNodeFactory(com.fasterxml.jackson.databind.node.JsonNodeFactory
                            .withExactBigDecimals(true));

    /** The extension: replaces the engine's DomainParser for {@code ###Pure}. */
    static PureGrammarParserExtension extension() {
        return new PureGrammarParserExtension() {
            @Override
            public Iterable<? extends SectionParser> getExtraSectionParsers() {
                return List.of(SectionParser.newParser("Pure",
                        LegendLiteSectionParser::parseSection));
            }
        };
    }

    private static ImportAwareCodeSection parseSection(SectionSourceCode section,
            java.util.function.Consumer<PackageableElement> elementConsumer,
            org.finos.legend.engine.language.pure.grammar.from.PureGrammarParserContext ctx) {
        ImportAwareCodeSection out = new ImportAwareCodeSection();
        out.parserName = "Pure";
        out.sourceInformation = section.sourceInformation;

        String sourceId = section.walkerSourceInformation.getSourceId();
        int lineOffset = section.walkerSourceInformation.getLineOffset();

        // ONE lite path (HARNESS_SIMPLIFICATION_PLAN 5a + Phase-1 doctrine):
        // the bridge derives imports AND elements from the PRODUCTION
        // document parser — the records parseDocument serializes (the model
        // door at the engine's dialect, the Rust parser's since 2026-11-06,
        // docs/rust-port/PHASE2.md §4 step 6a, where they were the Java
        // parser's own section walk): each section's imports, in section
        // order, from the section index; each element in order, under its
        // path (a function's mangled one), duplicates kept. The old
        // bridge-local site scanner re-implemented element discovery (and
        // needed a hand-rolled native-declaration throw because its scanner
        // would SKIP natives silently); the production parse consumes tokens
        // sequentially, so the strict surface's own 'Unsupported syntax'
        // refusal fires naturally.
        List<com.legend.protocol.Protocol.Element> elements =
                com.legend.parser.PmcdParser.parseModel(section.code).elements();
        for (com.legend.protocol.Protocol.Element el : elements) {
            if (el instanceof com.legend.protocol.Protocol.PSectionIndex index) {
                for (com.legend.protocol.Protocol.PSection sec : index.sections()) {
                    out.imports.addAll(sec.imports());
                }
            }
        }
        for (com.legend.protocol.Protocol.Element el : elements) {
            if (el instanceof com.legend.protocol.Protocol.PSectionIndex) {
                continue;
            }
            String path = com.legend.protocol.Protocol.pathOf(el);
            try {
                JsonNode tree = MAPPER.readTree(
                        com.legend.protocol.ProtocolEmitter.emitElement(el));
                // 5b DEFERRED BY DECISION: the offset seam stays at the
                // JSON boundary until the lexer grows native offsets —
                // the formula is bytecode-verified; only its placement
                // is impure
                shiftSpans(tree, sourceId, lineOffset);
                elementConsumer.accept(
                        MAPPER.treeToValue(tree, PackageableElement.class));
                out.elements.add(path);
            } catch (Exception e) {
                throw new RuntimeException(
                        "SPI bridge failed on " + path, e);
            }
        }
        return out;
    }

    /** Our spans are 1-based within the section text with sourceId "" — the section's
     *  walker carries the file line offset and real sourceId (section columnOffset is 0
     *  by the engine's own construction). */
    private static void shiftSpans(JsonNode node, String sourceId, int lineOffset) {
        if (node instanceof ObjectNode obj) {
            obj.fields().forEachRemaining(e -> {
                // every span key: sourceInformation, profileSourceInformation, ...
                if (e.getKey().endsWith("ourceInformation")
                        && e.getValue() instanceof ObjectNode s
                        && s.has("startLine")) {
                    s.put("sourceId", sourceId);
                    s.put("startLine", s.get("startLine").asInt() + lineOffset);
                    s.put("endLine", s.get("endLine").asInt() + lineOffset);
                } else {
                    shiftSpans(e.getValue(), sourceId, lineOffset);
                }
            });
        } else if (node.isArray()) {
            node.forEach(c -> shiftSpans(c, sourceId, lineOffset));
        }
    }
}
