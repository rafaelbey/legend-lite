package com.legend.parser;

import com.legend.model.ParsedModel;
import org.junit.jupiter.api.Test;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertThrows;
import static org.junit.jupiter.api.Assertions.assertTrue;

/** Phase M step 1 pins: section routing is REGISTRY-adjudicated, and a
 *  section NO grammar claims is REFUSED in the engine's own words — never
 *  skipped in silence, which used to drop every element inside it. */
class SectionGrammarRegistryTest {

    @Test
    void builtInsRegisterThroughTheRegistry() {
        // the built-in sections are the parser's own (2026-11-06, the Rust port's phase 2 step 6b: the Rust parser's
        // known sections, where the registry held a stand-in for each Java grammar; the registry keeps the
        // ServiceLoader's overlays alone)
        java.util.List<String> known = RustParsers.provider().knownSections();
        for (String s : new String[]{"Pure", "Mapping", "Relational",
                "Connection", "Runtime", "Diagram", "Service", "DataSpace",
                "Persistence"}) {
            assertTrue(known.contains(s), s);
        }
        assertTrue(!known.contains("NoSuchSection"));
        assertTrue(SectionGrammarRegistry.lookup("NoSuchSection").isEmpty());
    }

    // Every ROSTER section now has a grammar (QueryPostProcessor claimed
    // by the RelationalMapper leg, 2026-08-11) — the unclaimed stand-in
    // is a fictional name, which a production engine refuses with the
    // SAME "is not a known section parser" message.
    private static final String WITH_UNKNOWN_SECTION = """
            Class my::A { a: String[1]; }

            ###NoSuchSection
            Whatever my::D;

            ###Pure
            Class my::B { b: String[1]; }
            """;

    @Test
    void theDropInSurfaceRefusesAnUnclaimedSection() {
        // engine parity (PureGrammarParser:160): a drop-in cannot accept a
        // file whose sections it cannot read — my::B would vanish and any
        // nonsense in the ###Diagram body would be swallowed.
        ParseException e = assertThrows(ParseException.class,
                () -> com.legend.testing.Engine.model(WITH_UNKNOWN_SECTION));
        assertTrue(e.getMessage().contains(
                        "'NoSuchSection' is not a known section parser"),
                e.getMessage());
    }

    @Test
    void theInternalPipelineKeepsReadingAndRecordsTheSkip() {
        // real Legend models mix ###Service/###DataSpace/###Persistence with
        // the sections we implement; legend-lite has to load them to compile
        // the parts it owns (refusing cost the relational corpus its whole
        // library layer). The skip is DATA, not silence.
        ParsedModel m = com.legend.testing.Platform.model(WITH_UNKNOWN_SECTION);
        assertEquals(2, m.elements().size(), "both Pure classes parse");
        assertEquals(1, m.unclaimedSections().size());
        assertEquals("NoSuchSection", m.unclaimedSections().get(0).name());
    }

    @Test
    void overlayJarSectionParsesThroughTheServiceLoaderSeam() {
        // the whole Phase M point: a section legend-lite does not know,
        // added WITHOUT forking — ServiceLoader discovery, raw text in,
        // opaque elements out, claimed (not unclaimed)
        ParsedModel m = com.legend.testing.Platform.model("""
                Class my::A { a: String[1]; }

                ###Toy
                Toy my::toys::T1;
                Toy my::toys::T2;
                """);
        var opaques = m.elements().stream()
                .filter(e -> e instanceof com.legend.model.OpaqueElementDefinition)
                .map(e -> (com.legend.model.OpaqueElementDefinition) e)
                .toList();
        assertEquals(2, opaques.size());
        assertEquals("my::toys::T1", opaques.get(0).qualifiedName());
        assertEquals("Toy", opaques.get(0).sectionName());
        assertEquals(3, m.elements().size(),
                "opaque elements ride the SAME element list — indexed and"
                        + " routed like any element");
        assertTrue(ToySectionGrammar.lastText.contains("Toy my::toys::T2;"),
                "the grammar received the section's raw text");
    }

    @Test
    void registeringAGrammarIsHowToleranceIsSpelled() {
        // the escape hatch is EXPLICIT: ###Toy is accepted only because a
        // grammar claims it (above), while ###NoSuchSection is refused.
        // There is no third state where we accept without reading.
        assertTrue(SectionGrammarRegistry.lookup("Toy").isPresent());
        assertTrue(SectionGrammarRegistry.lookup("NoSuchSection").isEmpty());
        assertEquals(1, com.legend.testing.Platform.model("Class my::A { a: String[1]; }")
                .elements().size(), "a section-free file is unaffected");
    }
}
