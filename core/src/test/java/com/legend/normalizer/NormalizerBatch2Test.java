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

    // ---- item 1, MPP:237-240: a class's M2M chain is its root set's ----

    private static String m2mChain(String sets) {
        return """
                Class w::S { n: String[0..1]; }
                Class w::A { n: String[0..1]; }
                Class w::B { n: String[0..1]; }
                ###Mapping
                Mapping w::M
                (
                """ + sets + """
                )
                """;
    }

    @Test
    @DisplayName("item 1 (MPP:237-240): a class's ~src chain is walked through its root set, whatever the declaration order")
    void anM2mChainIsWalkedThroughEachClasssRootSet() {
        // asked of the engine: it compiles both orders (it checks no ~src chain at all, register row S380); lite
        // refused the root-first order because the last set of A stood for A in the cycle walk, until 2026-11-15
        String rootFirst = "  *w::A[a2]: Pure { ~src w::S n: $src.n }\n  w::A[a1]: Pure { ~src w::B n: $src.n }\n"
                + "  *w::B: Pure { ~src w::A n: $src.n }\n";
        String rootLast = "  w::A[a1]: Pure { ~src w::B n: $src.n }\n  *w::A[a2]: Pure { ~src w::S n: $src.n }\n"
                + "  *w::B: Pure { ~src w::A n: $src.n }\n";
        assertEquals(List.of(), strict(m2mChain(rootFirst)));
        assertEquals(List.of(), strict(m2mChain(rootLast)));
        // a real cycle among root sets is still lite's refusal (S380, the owner's)
        assertEquals(List.of("[5:1] Circular M2M ~src chain detected in mapping 'w::M': w::A -> w::B -> w::A"),
                strict(m2mChain("  w::A: Pure { ~src w::B n: $src.n }\n  w::B: Pure { ~src w::A n: $src.n }\n")));
    }

    // ---- item 1, SSR:40-43 and SSR:137: one store substituted twice; a substituted embedded block's key ----

    private static final String SUBSTITUTED_TWICE = """
            Class w::P { name: String[0..1]; }
            ###Relational
            Database w::Db1 ( Table T (ID INTEGER PRIMARY KEY, NAME VARCHAR(20)) )
            Database w::Db2 ( include w::Db1 )
            Database w::Db3 ( include w::Db1 )
            ###Mapping
            Mapping w::Base ( w::P: Relational { ~mainTable [w::Db1]T name: [w::Db1]T.NAME } )
            Mapping w::M ( include w::Base[w::Db1 -> w::Db2, w::Db1 -> w::Db3] )
            """;

    @Test
    @DisplayName("item 1 (SSR:40-43): one store substituted twice in one include is legend-pure's refusal")
    void oneStoreSubstitutedTwiceIsLegendPuresRefusal() {
        // legend-pure 5.99.0 MappingValidator.java:351-355 (lite's own dialect, whose grammar is legend-pure's: the
        // engine's parser keeps no pair of an include that writes several); until 2026-11-15 the last pair won
        assertEquals(List.of("[8:1] Store substitution error: multiple substitutions for w::Db1"),
                strict(SUBSTITUTED_TWICE));
        assertEquals(Map.of("w::M", "Store substitution error: multiple substitutions for w::Db1"),
                module(SUBSTITUTED_TWICE).walls());
        // an include of a mapping the model has not keeps its own refusal
        assertEquals(List.of("[8:1] Can't find mapping 'w::Nope'"),
                strict(SUBSTITUTED_TWICE.replace("include w::Base[", "include w::Nope[")));
    }

    @Test
    @DisplayName("item 1 (SSR:137): a substituted embedded block keeps its ~primaryKey")
    void aSubstitutedEmbeddedBlockKeepsItsPrimaryKey() {
        // legend-engine keeps an embedded set's declared primary key and an include's substitution rewrites no included
        // set (read); until 2026-11-15 the rebuilt block lost it, as AssociationSynthesis' did before 6419dec51
        com.legend.model.ParsedModel parsed = Compiler.parseModel("""
                Class w::P { name: String[0..1]; a: w::A[0..1]; }
                Class w::A { street: String[0..1]; }
                ###Relational
                Database w::Db1 ( Table T (ID INTEGER PRIMARY KEY, NAME VARCHAR(20), STREET VARCHAR(20)) )
                ###Mapping
                Mapping w::Base ( w::P: Relational { ~mainTable [w::Db1]T name: [w::Db1]T.NAME,
                  a ( ~primaryKey ([w::Db1]T.ID) street: [w::Db1]T.STREET ) } )
                """);
        com.legend.model.LegacyMappingDefinition base = parsed.elements().stream()
                .filter(com.legend.model.LegacyMappingDefinition.class::isInstance)
                .map(com.legend.model.LegacyMappingDefinition.class::cast).findFirst().orElseThrow();
        com.legend.model.ClassMapping.Relational rewritten = (com.legend.model.ClassMapping.Relational)
                StoreSubstitutionRewrite.apply(base.classMappings().get(0),
                        List.of(new com.legend.model.MappingInclude.StoreSubstitution("w::Db1", "w::Db2")));
        com.legend.model.PropertyMapping.Embedded a = (com.legend.model.PropertyMapping.Embedded)
                rewritten.propertyMappings().get(1);
        assertEquals(1, a.primaryKey().size());
        assertEquals("w::Db2", ((com.legend.model.RelationalOperation.ColumnRef) a.primaryKey().get(0)).databaseName());
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

    // ---- item 1, AS:87, XS:140, MN:1080 and MJN:85-93: association mappings' ends and names ----

    private static final String XSTORE_NON_ROOT_END = """
            ###Pure
            Class w::P { name: String[1]; fid: Integer[1]; }
            Class w::F { id: Integer[1]; legal: String[1]; }
            Association w::P_F { firm: w::F[0..1]; people: w::P[*]; }

            ###Relational
            Database w::Db
            (
              Table P1 (ID INTEGER PRIMARY KEY, NAME VARCHAR(20), FID INTEGER)
              Table P2 (ID2 INTEGER PRIMARY KEY, NAME2 VARCHAR(20), FID2 INTEGER)
              Table F (ID INTEGER PRIMARY KEY, LEGAL VARCHAR(20))
            )

            ###Mapping
            Mapping w::M
            (
              *w::P[p1]: Relational { ~mainTable [w::Db]P1 name: [w::Db]P1.NAME, fid: [w::Db]P1.FID }
              w::P[p2]: Relational { ~mainTable [w::Db]P2 name: [w::Db]P2.NAME2, fid: [w::Db]P2.FID2 }
              *w::F[f]: Relational { ~mainTable [w::Db]F id: [w::Db]F.ID, legal: [w::Db]F.LEGAL }
              w::P_F: XStore { people[f, p2]: $this.id == $that.fid, firm[p2, f]: $this.fid == $that.id }
            )
            """;

    @Test
    @DisplayName("item 1 (XS:140): an XStore line naming a non-root set navigates to that set's table")
    void anXStoreLineNamingANonRootSetNavigatesToIt() {
        // asked of the engine: F.people over `people[f, p2]` joins P2 on FID2; until 2026-11-15 the column-space
        // emission typed the condition by p2's columns over the root p1's relation: "relation has no column 'FID2'"
        assertEquals(List.of(), strict(XSTORE_NON_ROOT_END));
    }

    private static final String MODEL_JOIN_TWO_SETS = """
            ###Pure
            Class w::P { name: String[1]; firmId: Integer[1]; }
            Class w::F { id: Integer[1]; legal: String[1]; }
            Association w::P_F { employees: w::P[*]; firm: w::F[0..1]; }

            ###Relational
            Database w::Db
            (
              Table P (ID INTEGER PRIMARY KEY, NAME VARCHAR(20), FID INTEGER)
              Table F1 (ID INTEGER PRIMARY KEY, LEGAL VARCHAR(20))
              Table F2 (ID2 INTEGER PRIMARY KEY, LEGAL2 VARCHAR(20))
            )

            ###Mapping
            Mapping w::M
            (
              *w::P[p]: Relational { ~mainTable [w::Db]P name: [w::Db]P.NAME, firmId: [w::Db]P.FID }
              w::F[f2]: Relational { ~mainTable [w::Db]F2 id: [w::Db]F2.ID2, legal: [w::Db]F2.LEGAL2 }
              *w::F[f1]: Relational { ~mainTable [w::Db]F1 id: [w::Db]F1.ID, legal: [w::Db]F1.LEGAL }
              w::P_F: ModelJoin { {employees: w::P[1], firm: w::F[1]|$employees.firmId == $firm.id} }
            )
            """;

    @Test
    @DisplayName("item 1 (MN:1080): a ModelJoin's end is its class's root set")
    void aModelJoinsEndIsItsClasssRootSet() {
        // asked of the engine: the root F1 set is joined whichever set of F is declared first; until 2026-11-15 the
        // first set of the class in the closure, so a non-root set declared first typed the condition by its columns:
        // "relation has no column 'ID2'"
        assertEquals(List.of(), strict(MODEL_JOIN_TWO_SETS));
    }

    private static final String NESTED_MODEL_JOIN_IN_AN_INCLUDE = """
            ###Pure
            Class w::P { name: String[1]; id: Integer[1]; }
            Class w::A { city: String[1]; personId: Integer[1]; }
            Class w::F { legal: String[1]; city: String[1]; }
            Association w::P_A { person: w::P[1]; address: w::A[1]; }
            Association w::P_F { employees: w::P[*]; firm: w::F[0..1]; }

            ###Relational
            Database w::Db
            (
              Table P (ID INTEGER PRIMARY KEY, NAME VARCHAR(20) NOT NULL)
              Table A (ID INTEGER PRIMARY KEY, PID INTEGER NOT NULL, CITY VARCHAR(20) NOT NULL)
              Table F (ID INTEGER PRIMARY KEY, LEGAL VARCHAR(20) NOT NULL, CITY VARCHAR(20) NOT NULL)
            )

            ###Mapping
            Mapping w::Base
            (
              *w::P[p]: Relational { ~mainTable [w::Db]P name: [w::Db]P.NAME, id: [w::Db]P.ID }
              *w::A[a]: Relational { ~mainTable [w::Db]A city: [w::Db]A.CITY, personId: [w::Db]A.PID }
              *w::F[f]: Relational { ~mainTable [w::Db]F legal: [w::Db]F.LEGAL, city: [w::Db]F.CITY }
              w::P_A: ModelJoin { {person: w::P[1], address: w::A[1]|$person.id == $address.personId} }
            )

            Mapping w::M
            (
              include mapping w::Base
              w::P_F: ModelJoin { {employees: w::P[1], firm: w::F[1]|$employees.address.city == $firm.city} }
            )
            """;

    @Test
    @DisplayName("item 1 (MJN:85-93): a nested hop's ModelJoin is found in an included mapping")
    void aNestedHopsModelJoinIsFoundInAnInclude() {
        // asked of the engine: it plans P -> A -> F through Base's P_A; until 2026-11-15 only this mapping's own
        // ModelJoins were searched, and every query navigating firm was refused "nested hop '$p.address' navigates
        // association 'w::P_A' which has no ModelJoin in this mapping"
        Compiler.BuiltModule m = module(NESTED_MODEL_JOIN_IN_AN_INCLUDE);
        assertEquals(java.util.Optional.empty(), m.context().mappingAssociationPoison("w::M", "w::P_F"));
    }

    @Test
    @DisplayName("item 1 (AS:87): an included association mapping's name resolves through its own mapping's imports")
    void anIncludedAssociationMappingsNameResolvesThroughItsImports() {
        // a tolerant build: name resolution walls w::Inner (an ambiguous F) and keeps its text, so the pre-pass of the
        // including w::M met P_F as written; it looked the name up exactly and walled w::M "Can't find association
        // 'P_F'", where the synthesis resolves it through the import that brings it in (as the engine does), until
        // 2026-11-15
        String text = """
                Class w::P { name: String[1]; }
                Class w::F { legal: String[1]; }
                Class u::F { x: String[1]; }
                Association w::P_F { firm: w::F[1]; people: w::P[*]; }
                ###Relational
                Database w::Db ( Table P (ID INTEGER PRIMARY KEY, NAME VARCHAR(20), FID INTEGER)
                  Table F (ID INTEGER PRIMARY KEY, LEGAL VARCHAR(20)) Join PF(P.FID = F.ID) )
                ###Mapping
                import w::*;
                import u::*;
                Mapping w::Inner
                (
                  w::P: Relational { ~mainTable [w::Db]P name: [w::Db]P.NAME }
                  F: Relational { ~mainTable [w::Db]F legal: [w::Db]F.LEGAL }
                  P_F: Relational { AssociationMapping ( firm: [w::Db]@PF, people: [w::Db]@PF ) }
                )
                ###Mapping
                import w::*;
                Mapping w::M
                (
                  include mapping w::Inner
                )
                """;
        assertEquals(java.util.Set.of("w::Inner"), module(text).walls().keySet());
    }

    // ---- item 1, MN:2562-2602, MN:539 and RR:131/:184: a view's own database, a clean-sheet door's includes, a
    // shadowing lambda ----

    private static final String VIEW_ON_VIEW_SHADOWED = """
            ###Pure
            Class w::Q { qid: Integer[0..1]; }
            Class w::P { id: Integer[0..1]; other: Integer[0..1]; }
            Association w::QP { q: w::Q[0..1]; p: w::P[0..1]; }

            ###Relational
            Database w::A
            (
              Table T (ID INTEGER PRIMARY KEY, OTHER INTEGER)
              View V1 (ID: T.ID PRIMARY KEY, OTHER: T.OTHER)
              View V2 (ID: V1.ID PRIMARY KEY)
            )
            Database w::B
            (
              include w::A
              Table S (ID INTEGER PRIMARY KEY)
              View V1 (ID: T.OTHER PRIMARY KEY)
              Join J(S.ID = V2.ID)
            )

            ###Mapping
            Mapping w::M
            (
              w::Q: Relational { ~mainTable [w::B]S qid: [w::B]S.ID }
              w::P: Relational { ~mainTable [w::B]T id: [w::B]T.ID, other: [w::B]T.OTHER }
              w::QP: Relational { AssociationMapping ( p: [w::B]@J, q: [w::B]@J ) }
            )

            ###Connection
            RelationalDatabaseConnection w::C { store: w::B; type: H2; specification: LocalH2 { }; auth: DefaultH2; }

            ###Runtime
            Runtime w::R { mappings: [w::M]; connections: [ w::B: [ c: w::C ] ]; }
            """;

    @Test
    @DisplayName("item 1 (MN:2562-2602): a view-on-view column re-resolves in the database that declares the view")
    void aViewOnViewColumnReResolvesInItsOwnDatabase() {
        // asked of the engine: Q.p joins A's V1 (T.ID), the view's own database's, though B declares a V1 of its own
        // (T.OTHER); until 2026-11-15 the inner view re-resolved against the hop's database, B, and joined T.OTHER
        String sql = Compiler.query(Compiler.compileModel(VIEW_ON_VIEW_SHADOWED),
                "w::Q.all()->project([x|$x.qid, x|$x.p.id],['qid','pid'])->from(w::M, w::R)").plan("w::R").sql();
        assertEquals(true, sql.contains("ON \"t0\".ID = \"t1\".ID"), sql);
        assertEquals(false, sql.contains("OTHER"), sql);
    }

    private static final String CLEAN_SHEET_INCLUDE = """
            ###Pure
            Enum w::E { A, B }
            Class w::P { e: w::E[0..1]; }
            function w::pf(): w::P[*]
            {
              #>{w::Db.T}# -> map(r | ^w::P(e = if($r.CODE == 'a', |w::E.A, |w::E.B)))
            }

            ###Relational
            Database w::Db
            (
              Table T (ID INTEGER PRIMARY KEY, CODE VARCHAR(20))
            )

            ###Mapping
            Mapping w::Base
            (
              w::E: EnumerationMapping EM { A: 'a', B: 'b' }
            )
            Mapping w::M
            (
              include w::Base
              *w::P: Relational { w::pf }
            )
            """;

    @Test
    @DisplayName("item 1 (MN:539): a clean-sheet mapping's enumeration mappings hold its includes'")
    void aCleanSheetMappingsEnumerationMappingsHoldItsIncludes() {
        // LITE-ONLY (the engine has no clean-sheet door): the legacy door's list is include-flattened and its readers
        // take it so; until 2026-11-15 the clean-sheet door kept its own alone
        com.legend.compiler.element.PureModelContext ctx =
                (com.legend.compiler.element.PureModelContext) Compiler.compileModel(CLEAN_SHEET_INCLUDE);
        assertEquals(List.of("EM"), ctx.findMapping("w::M").orElseThrow().enumerationMappings().stream()
                .map(com.legend.model.EnumerationMapping::mappingId).toList());
    }

    @Test
    @DisplayName("item 1 (RR:131/:184): a lambda that rebinds src or a bound name shadows it")
    void aShadowingLambdaIsLeftAlone() {
        // Pure's innermost-first scoping (legend-engine 4.145.0 plans `$src.N->map(src | $src + 1)` as N + 1); until
        // 2026-11-15 the lambda's own $src was replaced by the row too
        com.legend.protocol.spec.Variable src = new com.legend.protocol.spec.Variable("src");
        com.legend.protocol.spec.ValueSpecification row = new com.legend.protocol.spec.Variable("row");
        com.legend.protocol.spec.LambdaFunction shadowing = new com.legend.protocol.spec.LambdaFunction(
                List.of(src), List.of(new com.legend.protocol.spec.AppliedProperty(src, "N", null)));
        assertEquals(shadowing, com.legend.model.ClassMapping.RelationFunction.Col.bindSrc(shadowing, row));
        com.legend.protocol.spec.ValueSpecification read = new com.legend.protocol.spec.AppliedProperty(src, "N", null);
        assertEquals(new com.legend.protocol.spec.AppliedProperty(row, "N", null),
                com.legend.model.ClassMapping.RelationFunction.Col.bindSrc(read, row));
    }

    // ---- item 2, S395: an M2M class-typed property whose class has no set; item 3, S389: pAllVersions ----

    private static String m2m(String bindings, String extra) {
        return """
                Class w::F { name: String[1]; }
                Class w::S { name: String[1]; firm: w::F[1]; }
                Class w::T { name: String[1]; firm: w::F[1]; }
                """ + extra + """
                ###Mapping
                Mapping w::M
                (
                  *w::T: Pure { ~src w::S name: $src.name""" + bindings + """
                 }
                )
                """;
    }

    @Test
    @DisplayName("item 2 (S395): a class-typed property of a class with no set passes a conforming transform through")
    void anM2mPropertyOfAnUnmappedClassPassesAConformingTransformThrough() {
        // asked of legend-engine 4.145.0 (ClassMappingThirdPassBuilder.java:96-115): `$src.firm`, `^w::F(...)` and a
        // route by the default id compile; a transform of another type, or a route by another id, is "Can't find class
        // mapping '<id>'". Until 2026-11-15 lite refused each, "M2M class-typed property 'firm' on 'w::T' targets
        // unmapped class 'w::F'"
        assertEquals(List.of(), strict(m2m(", firm: $src.firm", "")));
        assertEquals(List.of(), strict(m2m(", firm: ^w::F(name='x')", "")));
        assertEquals(List.of(), strict(m2m(", firm[w_F]: $src.firm", "")));
        assertEquals(List.of("[5:1] Can't find class mapping 'w_F'"), strict(m2m(", firm: $src", "")));
        assertEquals(List.of("[5:1] Can't find class mapping 'x'"), strict(m2m(", firm[x]: $src.firm", "")));
    }

    @Test
    @DisplayName("item 3 (S389): pAllVersions binds only for a p whose type is a temporal class, else the engine's refusal")
    void anAllVersionsBindingNeedsATemporalPropertyType() {
        // asked of legend-engine 4.145.0: a p of a class with a temporal stereotype of its own compiles; of another
        // class, a primitive, or a subclass of a temporal class: "Can't find property 'pAllVersions' in [T, Any]".
        // Until 2026-11-15 the suffix was stripped whatever p's type
        assertEquals(List.of(), strict("""
                Class <<temporal.businesstemporal>> w::F { name: String[1]; }
                Class w::S { name: String[1]; firm: w::F[*]; }
                Class w::T { name: String[1]; firm: w::F[*]; }
                ###Mapping
                Mapping w::M
                (
                  *w::T: Pure { ~src w::S name: $src.name, firmAllVersions: $src.firmAllVersions }
                  *w::F: Pure { ~src w::F name: $src.name }
                )
                """));
        assertEquals(List.of("[5:1] Can't find property 'nameAllVersions' in [T, Any]"),
                strict(m2m(", nameAllVersions: $src.name", "")));
        assertEquals(List.of("[5:1] Can't find property 'nope' in [T, Any]"),
                strict(m2m(", nope: $src.name", "")));
    }

    // ---- item 3, S387: dayOfWeekNumber's week start; S384: each direction of an association by its own join ----

    private static String dayOfWeek(String weekStart) {
        return """
                Class w::P { id: Integer[1]; n: Integer[0..1]; }
                ###Relational
                Database w::Db ( Table T (ID INTEGER PRIMARY KEY, D DATE) )
                ###Mapping
                Mapping w::M ( w::P: Relational { ~mainTable [w::Db]T id: [w::Db]T.ID, n: dayOfWeekNumber([w::Db]T.D, '""" + weekStart + """
                ') } )
                """;
    }

    @Test
    @DisplayName("item 3 (S387): dayOfWeekNumber's week start is matched exactly, else the engine's words")
    void dayOfWeekNumbersWeekStartIsMatchedExactly() {
        // asked of the engine: 'Sunday' and 'Monday' plan (dow+1 / isodow, the numbering lite's on Sunday and Saturday);
        // 'monday' and 'Tuesday' fail at plan, "DayOfWeekNumber Function requires either Sunday or Monday as First Day
        // of Week. Found monday instead." (H2). Until 2026-11-15 'monday' was accepted (matched ignoring case) and
        // 'Tuesday' refused in lite's words. Lite withholds the class (a deferred refusal): a query of it is refused.
        assertEquals(java.util.Optional.empty(), module(dayOfWeek("Sunday")).context().mappingPoison("w::M", "w::P"));
        assertEquals(java.util.Optional.empty(), module(dayOfWeek("Monday")).context().mappingPoison("w::M", "w::P"));
        assertEquals("DayOfWeekNumber Function requires either Sunday or Monday as First Day of Week. Found monday"
                + " instead.", module(dayOfWeek("monday")).context().mappingPoison("w::M", "w::P").orElseThrow());
        assertEquals("DayOfWeekNumber Function requires either Sunday or Monday as First Day of Week. Found Tuesday"
                + " instead.", module(dayOfWeek("Tuesday")).context().mappingPoison("w::M", "w::P").orElseThrow());
    }

    private static final String ASSOCIATION_TWO_JOINS = """
            ###Pure
            Class w::P { name: String[0..1]; }
            Class w::F { name: String[0..1]; }
            Association w::PF { firm: w::F[0..1]; employees: w::P[*]; }

            ###Relational
            Database w::Db
            (
              Table T (ID INTEGER PRIMARY KEY, NAME VARCHAR(20), FID INTEGER, FID2 INTEGER)
              Table T3 (ID INTEGER PRIMARY KEY, NAME VARCHAR(20))
              Join J1(T.FID = T3.ID)
              Join J2(T.FID2 = T3.ID)
            )

            ###Mapping
            Mapping w::M
            (
              w::P: Relational { ~mainTable [w::Db]T name: [w::Db]T.NAME }
              w::F: Relational { ~mainTable [w::Db]T3 name: [w::Db]T3.NAME }
              w::PF: Relational { AssociationMapping ( firm: [w::Db]@J1, employees: [w::Db]@J2 ) }
            )

            ###Connection
            RelationalDatabaseConnection w::C { store: w::Db; type: H2; specification: LocalH2 { }; auth: DefaultH2; }

            ###Runtime
            Runtime w::R { mappings: [w::M]; connections: [ w::Db: [ c: w::C ] ]; }
            """;

    @Test
    @DisplayName("item 3 (S384): each direction of an association navigates by its own join")
    void eachDirectionOfAnAssociationNavigatesByItsOwnJoin() {
        // asked of the engine: P.firm joins on FID (J1), F.employees on FID2 (J2), whichever line is written first;
        // until 2026-11-15 both directions read the FIRST line's join
        com.legend.compiler.element.ModelContext ctx = Compiler.compileModel(ASSOCIATION_TWO_JOINS);
        String firm = Compiler.query(ctx, "w::P.all()->project([x|$x.firm.name],['f'])->from(w::M, w::R)")
                .plan("w::R").sql();
        String employees = Compiler.query(ctx, "w::F.all()->project([x|$x.employees.name],['e'])->from(w::M, w::R)")
                .plan("w::R").sql();
        assertEquals(true, firm.contains(".FID = ") && !firm.contains("FID2"), firm);
        assertEquals(true, employees.contains(".FID2 = "), employees);
    }

    // ---- item 3, S390: a set whose main table cannot be inferred ----

    private static final String MAIN_TABLE_TWO_TABLES = """
            ###Pure
            Class w::P { a: String[0..1]; b: String[0..1]; }
            Class w::F { name: String[0..1]; }
            Association w::PF { firm: w::F[0..1]; people: w::P[*]; }

            ###Relational
            Database w::Db
            (
              Table T (ID INTEGER PRIMARY KEY, NAME VARCHAR(20), FID INTEGER)
              Table T2 (ID INTEGER PRIMARY KEY, NAME VARCHAR(20), FID INTEGER)
              Table T3 (ID INTEGER PRIMARY KEY, NAME VARCHAR(20))
              Join J(T.ID = T2.ID)
              Join JF(T.FID = T3.ID)
              Join JF2(T2.FID = T3.ID)
            )


            ###Mapping
            Mapping w::M
            (
              w::P[pid]: Relational { a: [w::Db]T.NAME, b: [w::Db]T2.NAME }
            )
            """;

    private static final String MAIN_TABLE_NONE = """
            ###Pure
            Class w::P { a: String[0..1]; b: String[0..1]; }
            Class w::F { name: String[0..1]; }
            Association w::PF { firm: w::F[0..1]; people: w::P[*]; }

            ###Relational
            Database w::Db
            (
              Table T (ID INTEGER PRIMARY KEY, NAME VARCHAR(20), FID INTEGER)
              Table T2 (ID INTEGER PRIMARY KEY, NAME VARCHAR(20), FID INTEGER)
              Table T3 (ID INTEGER PRIMARY KEY, NAME VARCHAR(20))
              Join J(T.ID = T2.ID)
              Join JF(T.FID = T3.ID)
              Join JF2(T2.FID = T3.ID)
            )


            ###Mapping
            Mapping w::M
            (
              w::P: Relational { a: 'x' }
            )
            """;

    private static final String MAIN_TABLE_TWO_DATABASES = """
            ###Pure
            Class w::P { a: String[0..1]; b: String[0..1]; }
            Class w::F { name: String[0..1]; }
            Association w::PF { firm: w::F[0..1]; people: w::P[*]; }

            ###Relational
            Database w::Db
            (
              Table T (ID INTEGER PRIMARY KEY, NAME VARCHAR(20), FID INTEGER)
              Table T2 (ID INTEGER PRIMARY KEY, NAME VARCHAR(20), FID INTEGER)
              Table T3 (ID INTEGER PRIMARY KEY, NAME VARCHAR(20))
              Join J(T.ID = T2.ID)
              Join JF(T.FID = T3.ID)
              Join JF2(T2.FID = T3.ID)
            )
            Database w::Db2 ( Table T (ID INTEGER PRIMARY KEY, NAME VARCHAR(20)) )

            ###Mapping
            Mapping w::M
            (
              w::P: Relational { a: [w::Db]T.NAME, b: [w::Db2]T.NAME }
            )
            """;

    private static final String MAIN_TABLE_ASSOCIATION = """
            ###Pure
            Class w::P { a: String[0..1]; b: String[0..1]; }
            Class w::F { name: String[0..1]; }
            Association w::PF { firm: w::F[0..1]; people: w::P[*]; }

            ###Relational
            Database w::Db
            (
              Table T (ID INTEGER PRIMARY KEY, NAME VARCHAR(20), FID INTEGER)
              Table T2 (ID INTEGER PRIMARY KEY, NAME VARCHAR(20), FID INTEGER)
              Table T3 (ID INTEGER PRIMARY KEY, NAME VARCHAR(20))
              Join J(T.ID = T2.ID)
              Join JF(T.FID = T3.ID)
              Join JF2(T2.FID = T3.ID)
            )

            ###Mapping
            Mapping w::M
            (
              *w::P[p1]: Relational { a: [w::Db]T.NAME, b: [w::Db]T2.NAME }
              w::P[p2]: Relational { ~mainTable [w::Db]T2 a: [w::Db]T2.NAME }
              *w::F: Relational { ~mainTable [w::Db]T3 name: [w::Db]T3.NAME }
              w::PF: Relational { AssociationMapping ( firm[p1, w_F]: [w::Db]@JF2, people[w_F, p1]: [w::Db]@JF2 ) }
            )
            """;

    @Test
    @DisplayName("item 3 (S390): no main table inferable is the engine's refusal, naming the set id; one table in two databases compiles")
    void aSetWithNoInferableMainTableIsTheEnginesRefusal() {
        // asked of the engine (RelationalCompilerExtension.java:339-355): two tables, or none, are "Can't find the main
        // table for class '<set id>'. Please specify a main table using the ~mainTable directive." ('null' for an
        // unlabelled set); [Db]T and [Db2]T compile (one alias, the first's). Until 2026-11-15 lite named the class and
        // the tables, deferred a set reading no table, and refused two databases
        assertEquals(List.of("[19:1] Can't find the main table for class 'pid'. Please specify a main table using the"
                + " ~mainTable directive."), strict(MAIN_TABLE_TWO_TABLES));
        assertEquals(List.of("[19:1] Can't find the main table for class 'null'. Please specify a main table using the"
                + " ~mainTable directive."), strict(MAIN_TABLE_NONE));
        assertEquals(List.of(), strict(MAIN_TABLE_TWO_DATABASES));
        // a tolerant build withholds the class and its association: never anchored on the other set's table, nor on
        // the table the join derives for the end (until 2026-11-15's follow-up the join's T2 anchored it)
        Compiler.BuiltModule m = module(MAIN_TABLE_ASSOCIATION);
        assertEquals(true, m.context().mappingPoison("w::M", "w::P").isPresent()
                || m.context().mappingSetPoison("w::M", "w::P", "p1").isPresent());
        assertEquals(true, m.context().mappingAssociationPoison("w::M", "w::PF").orElse("")
                .startsWith("no predicate binding was emitted: "));
    }

    // ---- item 4 (a), S376: a mapping's own missing join ----

    /** port-scratch/normbatch2/engine/cases/g5b/s376_*.pure: one model, the mapping's lines in {@code %s}. */
    private static final String MISSING_JOIN = """
            ###Pure
            Class w::P { a: String[0..1]; b: String[0..1]; }
            Class w::F { name: String[0..1]; }
            Association w::PF { firm: w::F[0..1]; people: w::P[*]; }

            ###Relational
            Database w::Db
            (
              Table T (ID INTEGER PRIMARY KEY, NAME VARCHAR(20), FID INTEGER)
              Table T2 (ID INTEGER PRIMARY KEY, NAME VARCHAR(20), FID INTEGER)
              Table T3 (ID INTEGER PRIMARY KEY, NAME VARCHAR(20))
              Join J(T.ID = T2.ID)
              Join JF(T2.FID = T3.ID)
              Filter FT3(T3.NAME = 'x')
            )

            ###Mapping
            Mapping w::M
            (
            %s
            )
            """;

    @Test
    @DisplayName("item 4 (a): a mapping's join its database has not is refused in the engine's words")
    void aMappingsMissingJoinIsTheEnginesRefusal() {
        // asked of the engine (HelperRelationalBuilder.getJoin, :190): each of these is "Can't find join 'Nope' in
        // database 'Db'" when the mapping compiles. Until 2026-11-15 lite's own, "Join 'Nope' not found in db 'w::Db';
        // PM='null', mapping=w::M" and "AssociationMapping join 'Nope' not found in db 'w::Db'; association='w::PF',
        // mapping=w::M"
        String refusal = "[18:1] Can't find join 'Nope' in database 'Db'";
        String sets = "  w::P: Relational { ~mainTable [w::Db]T a: [w::Db]T.NAME }\n"
                + "  w::F: Relational { ~mainTable [w::Db]T3 name: [w::Db]T3.NAME }\n";
        for (String lines : List.of(
                "  w::P: Relational { ~mainTable [w::Db]T a: [w::Db]T.NAME, b: [w::Db]@Nope | T2.NAME }",
                "  w::P: Relational { ~mainTable [w::Db]T a: [w::Db]T.NAME, b: [w::Db]@J > @Nope | T3.NAME }",
                sets + "  w::PF: Relational { AssociationMapping ( firm: [w::Db]@Nope, people: [w::Db]@Nope ) }",
                sets + "  w::PF: Relational { AssociationMapping ( firm: [w::Db]@J > @Nope, people: [w::Db]@Nope > @J ) }",
                "  w::P: Relational { ~filter [w::Db] @Nope | [w::Db] FT3 ~mainTable [w::Db]T a: [w::Db]T.NAME }",
                "  w::P: Relational { ~filter [w::Db] (INNER) @Nope | [w::Db] FT3 ~mainTable [w::Db]T a: [w::Db]T.NAME }")) {
            assertEquals(List.of(refusal), strict(MISSING_JOIN.formatted(lines)), lines);
        }
    }

    // ---- item 4 (c): the first batch's untested parts ----

    /** Two services, one whose query's lambda parameter is untyped ({@code n}, no {@code : Type[mult]}). */
    private static final String UNTYPED_SERVICE_PARAMETER = """
            Class model::Person { name: String[1]; }
            ###Service
            Service my::api::ByName
            {
              pattern: '/person/{n}';
              documentation: '';
              execution: Single { query: {n | model::Person.all()->filter(p | $p.name == $n)}; mapping: my::M; runtime: my::R; }
            }
            Service my::api::All
            {
              pattern: '/people';
              documentation: '';
              execution: Single { query: |model::Person.all(); mapping: my::M; runtime: my::R; }
            }
            """;

    @Test
    @DisplayName("item 4 (c): E.4's untyped query parameter refuses a strict build and walls its service in a tolerant one")
    void anUntypedServiceQueryParameterWallsItsService() {
        // LITE-ONLY (the engine has no tolerant mode): the normalizer batch's item 4 (b) walled the service where the
        // refusal escaped a tolerant build; its Rust twin was tested (lite-normalizer's service-query test), the Java
        // not, until 2026-11-15 (the normalizer's second batch, item 4 (c))
        String refusal = "Service 'my::api::ByName' query parameter 'n' must be typed (name: Type[mult]) to externalize"
                + " into my::api::ByName$query";
        com.legend.model.ParsedModel resolved = com.legend.compiler.NameResolver.resolve(
                com.legend.testing.Own.model(UNTYPED_SERVICE_PARAMETER));
        com.legend.error.ModelException strict = org.junit.jupiter.api.Assertions.assertThrows(
                com.legend.error.ModelException.class, () -> ModelNormalizer.normalize(resolved,
                        com.legend.compiler.ModelBuilder.from(resolved), null));
        assertEquals(refusal, strict.getMessage());
        Map<String, String> walls = new java.util.LinkedHashMap<>();
        com.legend.model.NormalizedModel tolerant = ModelNormalizer.normalize(resolved,
                com.legend.compiler.ModelBuilder.from(resolved), walls);
        assertEquals(Map.of("my::api::ByName", refusal), walls);
        assertEquals(List.of("my::api::All$query"), tolerant.elements().stream()
                .filter(com.legend.model.FunctionDefinition.class::isInstance)
                .map(e -> ((com.legend.model.FunctionDefinition) e).qualifiedName())
                .filter(n -> n.endsWith("$query")).toList(), "the other service's query lifted");
    }
}
