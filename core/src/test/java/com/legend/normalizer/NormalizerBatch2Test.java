// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

package com.legend.normalizer;

import com.legend.Compiler;
import org.junit.jupiter.api.DisplayName;
import org.junit.jupiter.api.Test;

import java.util.List;
import java.util.Map;

import static org.junit.jupiter.api.Assertions.assertEquals;

/**
 * THE NORMALIZER'S SECOND BATCH (2026-11-15; docs/rust-port/PHASE3.md, the normalizer slice's entry, "the normalizer's
 * second batch"; docs/rust-port/PHASE3_SHIM_DESIGN.md §10 rulings 34 and 37): step E's further behaviours and the
 * register rows the owner ruled to take the engine's form, each asked of legend-engine 4.145.0 first (the pinned
 * release on the engine runner's class path: port-scratch/normbatch2/engine), each case a model text through both
 * public doors -- the strict compile ({@link Compiler#compileErrors}) and the tolerant module build
 * ({@link Compiler#buildModule}). The Rust twins are lite-normalizer's unit tests, and the normalizer differential
 * holds every oracle answer to the Java.
 */
class NormalizerBatch2Test {

    private static List<String> strict(String text) {
        return Compiler.compileErrors(text);
    }

    private static Compiler.BuiltModule module(String text) {
        return Compiler.buildModule(Compiler.parseSources(List.of(new Compiler.ModelSource("m.pure", text))).model());
    }

    // ---- item 1, JCE:624-633: a schema-qualified main table's slot ----

    private static final String SCHEMA_MAIN_TABLE_SLOT = """
            Class w::P { name: String[0..1]; b: w::B[0..1]; }
            Class w::B { name: String[0..1]; }
            ###Relational
            Database w::Db
            (
              Table T (ID INTEGER PRIMARY KEY, NAME VARCHAR(20))
              Schema S2 ( Table T (ID INTEGER PRIMARY KEY, NAME VARCHAR(20), b INTEGER) )
              Table C (ID INTEGER PRIMARY KEY, NAME VARCHAR(20))
              Join TC(S2.T.b = C.ID)
            )
            ###Mapping
            Mapping w::M
            (
              w::P: Relational { ~mainTable [w::Db]S2.T name: [w::Db]S2.T.NAME, b: [w::Db]@TC }
              w::B: Relational { ~mainTable [w::Db]C name: [w::Db]C.NAME }
            )
            """;

    @Test
    @DisplayName("item 1 (JCE:624-633): a navigation's slot is minted past the columns of the main table's own schema")
    void aSchemaQualifiedMainTablesSlotIsMintedPastItsOwnColumns() {
        // asked of the engine: it compiles and plans the mapping (`... from S2.T as "root" left outer join C ...`);
        // until 2026-11-15 the slot was checked against the default schema's T, which has no `b`, so it was named `b`
        // and the lifted function refused "the column 'b' already exists in the relation"
        assertEquals(List.of(), strict(SCHEMA_MAIN_TABLE_SLOT));
    }

    // ---- item 1, JCE:843: a property mapping's join chain that leaves the table it is at ----

    private static String chainFrom(String pm) {
        return """
                Class w::P { name: String[0..1]; b: w::B[0..1]; }
                Class w::B { name: String[0..1]; }
                ###Relational
                Database w::Db
                (
                  Table T (ID INTEGER PRIMARY KEY, NAME VARCHAR(20), BID INTEGER)
                  Table A (ID INTEGER PRIMARY KEY, NAME VARCHAR(20), CID INTEGER)
                  Table C (ID INTEGER PRIMARY KEY, NAME VARCHAR(20))
                  Join TA(T.BID = A.ID)
                  Join AC(A.CID = C.ID)
                  Join TC(T.BID = C.ID)
                )
                ###Mapping
                Mapping w::M
                (
                  w::P: Relational { ~mainTable [w::Db]T name: [w::Db]T.NAME, b: [w::Db]""" + pm + """
                 }
                  w::B: Relational { ~mainTable [w::Db]C name: [w::Db]C.NAME }
                )
                """;
    }

    @Test
    @DisplayName("item 1 (JCE:843): a join that does not contain the table its chain is at is the engine's refusal")
    void aJoinThatLeavesItsSourceTableIsTheEnginesRefusal() {
        // asked of the engine (HelperRelationalBuilder.validatePropertyMappings, when the mapping compiles): "Mapping
        // error: the join AC does not contain the source table T" and, at a chain's second hop, "... the join TC does
        // not contain the source table A". Until 2026-11-15 a deferred NotImplementedException when the class was
        // synthesized, "Join 'AC' references multiple non-source tables [A, C]; multi-table joins not supported. owner=b,
        // hop 1, mapping=mapping=w::M"
        assertEquals(List.of("[14:1] Mapping error: the join AC does not contain the source table T"),
                strict(chainFrom("@AC")));
        assertEquals(List.of("[14:1] Mapping error: the join TC does not contain the source table A"),
                strict(chainFrom("@TA > @TC")));
        assertEquals(List.of(), strict(chainFrom("@TA > @AC")));
    }

    // ---- item 1, JCE:192-213 and MN:2111-2118: an embedded or otherwise-embedded primitive or enumeration ----

    private static final String EMBEDDED_HEAD = """
            Enum w::E { A, B }
            Class w::P { name: String[0..1]; s: String[0..1]; e: w::E[0..1]; }
            Class w::B { name: String[0..1]; }
            ###Relational
            Database w::Db
            (
              Table T (ID INTEGER PRIMARY KEY, NAME VARCHAR(20), BID INTEGER)
              Table C (ID INTEGER PRIMARY KEY, NAME VARCHAR(20))
              Join TC(T.BID = C.ID)
            )
            ###Mapping
            Mapping w::M
            (
            """;

    private static String embedded(String pm) {
        return EMBEDDED_HEAD + "  w::P[p]: Relational { ~mainTable [w::Db]T name: [w::Db]T.NAME, " + pm + " }\n"
                + "  w::B[bset]: Relational { ~mainTable [w::Db]C name: [w::Db]C.NAME }\n)\n";
    }

    @Test
    @DisplayName("item 1 (JCE:192-213, MN:2111-2118): an embedded primitive or enumeration is the engine's refusal")
    void anEmbeddedPrimitiveOrEnumerationIsTheEnginesRefusal() {
        // asked of the engine: each refused when the mapping compiles, "only complex classes can be the target of an
        // embedded property mapping" (HelperRelationalBuilder:1683-1686). Until 2026-11-15 an embedded String was built
        // as `new(@String, ...)` (refused when a body was typed, "class 'meta::pure::metamodel::type::String' has no
        // property 'name'") and an otherwise-embedded one deferred as an unmapped target class
        String refusal = "[12:1] only complex classes can be the target of an embedded property mapping";
        assertEquals(List.of(refusal), strict(embedded("s ( name: [w::Db]T.NAME )")));
        assertEquals(List.of(refusal), strict(embedded("s ( )")));
        assertEquals(List.of(refusal), strict(embedded("e ( name: [w::Db]T.NAME )")));
        assertEquals(List.of(refusal), strict(embedded("s ( name: [w::Db]T.NAME ) Otherwise ( [bset]: [w::Db]@TC )")));
        // the tolerant build withholds the class, the mapping unwalled
        Compiler.BuiltModule m = module(embedded("s ( name: [w::Db]T.NAME )"));
        assertEquals(Map.of(), m.walls());
    }

    // ---- item 1, JCE:415-419 and JCE:843: a walled view a routed navigation never reads; the owner's label ----

    private static String routedOverView(String view) {
        return """
                Class w::P { name: String[0..1]; b: w::B[0..1]; }
                Class w::B { name: String[0..1]; }
                ###Relational
                Database w::Db
                (
                  Table T (ID INTEGER PRIMARY KEY, NAME VARCHAR(20), BID INTEGER)
                  Table A (ID INTEGER PRIMARY KEY, NAME VARCHAR(20))
                  Table C (ID INTEGER PRIMARY KEY, NAME VARCHAR(20))
                  Table C2 (ID INTEGER PRIMARY KEY, NAME VARCHAR(20))
                  Join TC(T.BID = C.ID)
                  Join TC2(T.BID = C2.ID)
                  Join TV(T.BID = V.ID)
                """ + view + """
                )
                ###Mapping
                Mapping w::M
                (
                  *w::B: Operation { meta::pure::router::operations::union_OperationSetImplementation_1__SetImplementation_MANY_(b1, b2) }
                  w::B[b1]: Relational { ~mainTable [w::Db]C name: [w::Db]C.NAME }
                  w::B[b2]: Relational { ~mainTable [w::Db]C2 name: [w::Db]C2.NAME }
                  w::B[b3]: Relational { ~mainTable [w::Db]C name: [w::Db]C.NAME }
                  w::P: Relational { ~mainTable [w::Db]T name: [w::Db]T.NAME, b[b3]: [w::Db]@TV, b[b1]: [w::Db]@TC, b[b2]: [w::Db]@TC2 }
                )
                """;
    }

    @Test
    @DisplayName("item 1 (JCE:415-419, JCE:843): a routed navigation does not read a walled view; a view's text names its store once")
    void aRoutedNavigationDoesNotReadAWalledView() {
        // LITE-ONLY (a tolerant build): the routed union navigation's view target was built before the routes were
        // looked at, so a walled view poisoned P although no route reads it; until 2026-11-15
        String filtered = "  Filter FA(A.NAME = 'x')\n  Join CA(C.ID = A.ID)\n"
                + "  View V (~filter [w::Db]@CA | [w::Db]FA ID: C.ID PRIMARY KEY, CN: C.NAME)\n";
        Compiler.BuiltModule m = module(routedOverView(filtered));
        assertEquals(Map.of("w::Db$view$V", "view 'V' used as a join target has a join-mediated ~filter; only direct"
                + " view filters expand as relations. store=w::Db"), m.walls());
        assertEquals(java.util.Optional.empty(), m.context().mappingPoison("w::M", "w::P"));
        // a view's own hop names its store once (until 2026-11-15 "mapping=store=w::Db")
        Compiler.BuiltModule chain = module(routedOverView("  View V (ID: A.ID PRIMARY KEY, CN: @TC | C.NAME)\n"));
        assertEquals(Map.of("w::Db$view$V", "Join 'TC' references multiple non-source tables [T, C]; multi-table joins"
                + " not supported. owner=CN, hop 1, store=w::Db"), chain.walls());
        assertEquals(java.util.Optional.empty(), chain.context().mappingPoison("w::M", "w::P"));
    }
}
