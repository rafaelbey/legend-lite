package com.legend.equivalence;

import com.fasterxml.jackson.databind.ObjectMapper;
import org.finos.legend.engine.language.pure.grammar.from.PureGrammarParser;
import org.finos.legend.engine.shared.core.ObjectMapperFactory;
import org.junit.jupiter.api.Test;

import static org.junit.jupiter.api.Assertions.assertEquals;

/**
 * A construct the CORPUS does not exercise, pinned anyway.
 *
 * <p>A view's {@code ~filter} may carry the filter's own database and a join
 * chain reaching it. Our protocol record carried neither — {@code PViewFilter}
 * held a bare name and the emitter hardcoded {@code "joins":[]} — which was
 * byte-correct for all 25,472 compared elements and wrong for the grammar
 * (RelationalParserGrammar.g4:141-143). The parity gate could never have caught
 * it, because no comparable corpus view uses those forms.
 *
 * <p>It mattered because the MODEL the compiler runs on does carry the join
 * chain, so the protocol-first migration would have silently dropped it from
 * generated SQL. Fixed as R0 of that migration, and pinned here so the corpus's
 * silence never lets it rot back.
 */
class ViewFilterParityTest {

    private static final ObjectMapper MAPPER = ObjectMapperFactory
            .getNewStandardObjectMapperWithPureProtocolExtensionSupports();

    @Test
    void plainFilterHasNoDbAndNoJoins() throws Exception {
        assertByteParity("""
                ###Relational
                Database my::DB
                (
                  Table T (id INTEGER PRIMARY KEY, v VARCHAR(20))
                  Filter F (T.v = 'a')
                  View V (~filter F  id: T.id)
                )
                """);
    }

    @Test
    void crossDatabaseFilterCarriesItsDb() throws Exception {
        assertByteParity("""
                ###Relational
                Database my::DB
                (
                  include other::DB2
                  Table T (id INTEGER PRIMARY KEY, v VARCHAR(20))
                  View V (~filter [other::DB2] F2  id: T.id)
                )

                ###Relational
                Database other::DB2
                (
                  Table T2 (id INTEGER PRIMARY KEY, x VARCHAR(20))
                  Filter F2 (T2.x = 'y')
                )
                """);
    }

    @Test
    void joinMediatedFilterCarriesItsJoinChain() throws Exception {
        assertByteParity("""
                ###Relational
                Database my::DB
                (
                  include other::DB2
                  Table T (id INTEGER PRIMARY KEY, v VARCHAR(20))
                  Join J (T.id = [other::DB2]T2.id)
                  View V (~filter [my::DB] @J | [other::DB2] F2  id: T.id)
                )

                ###Relational
                Database other::DB2
                (
                  Table T2 (id INTEGER PRIMARY KEY, x VARCHAR(20))
                  Filter F2 (T2.x = 'y')
                )
                """);
    }

    /** Every Database in {@code src}, both ways, byte for byte. */
    private void assertByteParity(String src) throws Exception {
        var pmcd = PureGrammarParser.newInstance().parseModel(src);
        java.util.Map<String, String> expected = new java.util.LinkedHashMap<>();
        for (var e : pmcd.getElements()) {
            String json = MAPPER.writeValueAsString(e);
            if (json.startsWith("{\"_type\":\"relational\"")) {
                expected.put(e.getPath(), json);
            }
        }
        org.junit.jupiter.api.Assertions.assertFalse(expected.isEmpty(),
                "probe produced no Database — the source is wrong, not the parser");

        // every Database token at the stream's start or after ')', '}' or ';', parsed where it stands and emitted:
        // asked of the Rust parser and emitter since 2026-11-06 (docs/rust-port/PHASE2.md §4 step 6a;
        // rust/lite-front/src/probes.rs, database_sites), where it was this loop over the Java lexer's tokens and
        // the Java parser and emitter (rust/RETIRED_TESTS.md)
        int found = 0;
        for (var db : Surfaces.engineDatabases(src)) {
            assertEquals(expected.get(db.qualifiedName()), db.json(),
                    "byte divergence on " + db.qualifiedName());
            found++;
        }
        assertEquals(expected.size(), found,
                "site discovery missed a Database the engine emitted");
    }
}
