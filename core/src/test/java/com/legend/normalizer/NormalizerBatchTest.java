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
 * THE NORMALIZER BATCH (2026-11-15; docs/rust-port/PHASE3.md, the normalizer slice's entry, "the normalizer batch";
 * docs/rust-port/PHASE3_SHIM_DESIGN.md §10 rulings 29, 30 and 34): step E's behaviours the owner ruled fixed, each
 * asked of legend-engine 4.145.0 first (the pinned release on the engine runner's class path:
 * port-scratch/normbatch/engine), each case a model text through both public doors -- the strict compile
 * ({@link Compiler#compileErrors}) and the tolerant module build ({@link Compiler#buildModule}: its walls and the
 * poison a fetch raises). The Rust twins are lite-normalizer's unit tests, and the normalizer differential holds every
 * oracle answer to the Java.
 */
class NormalizerBatchTest {

    private static List<String> strict(String text) {
        return Compiler.compileErrors(text);
    }

    private static Compiler.BuiltModule module(String text) {
        return Compiler.buildModule(Compiler.parseSources(List.of(new Compiler.ModelSource("m.pure", text))).model());
    }

    // ---- item 2 (census ASK 13): an XStore or ModelJoin mapping of an association the model has not ----

    private static final String XSTORE_UNKNOWN_ASSOCIATION = """
            Class w::P { name: String[1]; fid: Integer[1]; }
            Class w::F { id: Integer[1]; }
            ###Relational
            Database w::Db ( Table P (ID INTEGER PRIMARY KEY, NAME VARCHAR(20), FID INTEGER) Table F (ID INTEGER PRIMARY KEY) )
            ###Mapping
            Mapping w::M (
              w::P[p]: Relational { ~mainTable [w::Db]P name: [w::Db]P.NAME, fid: [w::Db]P.FID }
              w::F[f]: Relational { ~mainTable [w::Db]F id: [w::Db]F.ID }
              w::Nope: XStore { firm[p, f]: $this.fid == $that.id, people[f, p]: $this.id == $that.fid }
            )
            """;

    private static final String MODEL_JOIN_UNKNOWN_ASSOCIATION = """
            Class w::P { name: String[1]; fid: Integer[1]; }
            Class w::F { id: Integer[1]; }
            ###Relational
            Database w::Db ( Table P (ID INTEGER PRIMARY KEY, NAME VARCHAR(20), FID INTEGER) Table F (ID INTEGER PRIMARY KEY) )
            ###Mapping
            Mapping w::M (
              w::P[p]: Relational { ~mainTable [w::Db]P name: [w::Db]P.NAME, fid: [w::Db]P.FID }
              w::F[f]: Relational { ~mainTable [w::Db]F id: [w::Db]F.ID }
              w::Nope: ModelJoin { {f: w::F[1], p: w::P[1]|$f.id == $p.fid} }
            )
            """;

    @Test
    @DisplayName("item 2: an XStore or ModelJoin mapping of an unknown association is the engine's \"Can't find association\"")
    void anUnknownXStoreOrModelJoinAssociationIsTheEnginesRefusal() {
        // asked of the engine: "Can't find association 'w::Nope'"; until 2026-11-15 a NotImplementedException naming the
        // mapping's kind, "Association mapping kind Cross not supported; mapping=w::M"
        String refusal = "Can't find association 'w::Nope'";
        assertEquals(List.of("[6:1] " + refusal), strict(XSTORE_UNKNOWN_ASSOCIATION));
        Compiler.BuiltModule xstore = module(XSTORE_UNKNOWN_ASSOCIATION);
        assertEquals(Map.of(), xstore.walls());
        assertEquals(refusal, xstore.context().mappingAssociationPoison("w::M", "w::Nope").orElseThrow());
        assertEquals(List.of("[6:1] " + refusal), strict(MODEL_JOIN_UNKNOWN_ASSOCIATION));
        assertEquals(refusal, module(MODEL_JOIN_UNKNOWN_ASSOCIATION).context()
                .mappingAssociationPoison("w::M", "w::Nope").orElseThrow());
    }

    // ---- item 2 (census ASK 13): the mapping facts name a set by its effective id ----

    private static final String DEFAULT_ID_MEMBERS = """
            Class w::P { name: String[0..1]; firm: w::F[0..1]; }
            Class w::F { id: Integer[0..1]; }
            ###Relational
            Database w::Db (
              Table T1 (ID INTEGER PRIMARY KEY, NAME VARCHAR(20), FID INTEGER)
              Table T2 (ID INTEGER PRIMARY KEY, NAME VARCHAR(20))
              Table F (ID INTEGER PRIMARY KEY)
              Join PF(T1.FID = F.ID)
            )
            ###Mapping
            Mapping w::M (
              *w::P[u]: Operation { meta::pure::router::operations::union_OperationSetImplementation_1__SetImplementation_MANY_(w_P, p2) }
              w::P: Relational { ~mainTable [w::Db]T1 name: [w::Db]T1.NAME, firm[w_F]: [w::Db]@PF }
              w::P[p2]: Relational { ~mainTable [w::Db]T2 name: [w::Db]T2.NAME }
              w::F: Relational { ~mainTable [w::Db]F id: [w::Db]F.ID }
            )
            """;

    @Test
    @DisplayName("item 2: a union member and a routed target written with no set id are named by the class's default id")
    void theMappingFactsNameASetByItsEffectiveId() {
        // asked of the engine: it compiles the model and plans the union over w_P and p2; until 2026-11-15 the facts
        // matched the RAW set id, absent when no id was written, so the union had no members fact and the route no
        // target class
        com.legend.model.LegacyMappingDefinition md = Compiler.parseModel(DEFAULT_ID_MEMBERS).elements().stream()
                .filter(com.legend.model.LegacyMappingDefinition.class::isInstance)
                .map(com.legend.model.LegacyMappingDefinition.class::cast).findFirst().orElseThrow();
        assertEquals(Map.of("w::P", List.of("w::P", "w::P")), MappingFacts.unionMembers(md));
        assertEquals(Map.of("w::P", Map.of("firm", "w::F")), MappingFacts.routedTargetClasses(md));
    }

    // ---- item 2 (census ASK 13): enumeration mappings -- an id taken twice, a value the enumeration has not ----

    private static final String ENUMERATION_ID_TWICE = """
            Enum w::E { A, B }
            Class w::P { e: w::E[0..1]; }
            ###Relational
            Database w::Db ( Table T (ID INTEGER PRIMARY KEY, C VARCHAR(5)) )
            ###Mapping
            Mapping w::Base ( w::E: EnumerationMapping em { A: 'a', B: 'b' } )
            ###Mapping
            Mapping w::M (
              include w::Base
              w::E: EnumerationMapping em { A: 'x', B: 'y' }
              w::P: Relational { ~mainTable [w::Db]T e: EnumerationMapping em: [w::Db]T.C }
            )
            """;

    private static final String ENUMERATION_VALUE_UNKNOWN = """
            Enum w::E { A, B }
            Class w::P { e: w::E[0..1]; }
            ###Relational
            Database w::Db ( Table T (ID INTEGER PRIMARY KEY, C VARCHAR(5)) )
            ###Mapping
            Mapping w::M (
              w::E: EnumerationMapping { A: 'x', B: 'y', C: 'z' }
              w::P: Relational { ~mainTable [w::Db]T e: EnumerationMapping w_E: [w::Db]T.C }
            )
            """;

    @Test
    @DisplayName("item 2: an enumeration mapping id taken twice in a closure, and a value the enumeration lacks, in the engine's words")
    void enumerationMappingsAreRefusedInTheEnginesWords() {
        // asked of the engine: "Duplicated enumeration mappings found with ID 'em' in mapping 'w::M'"
        // (MappingValidator); until 2026-11-15 lite accepted the pair, a lookup by the id taking the first
        String twice = "Duplicated enumeration mappings found with ID 'em' in mapping 'w::M'";
        assertEquals(List.of("[8:1] " + twice), strict(ENUMERATION_ID_TWICE));
        assertEquals(Map.of("w::M", twice), module(ENUMERATION_ID_TWICE).walls());
        // asked of the engine: "Can't find enum value 'C' in enumeration 'w::E'"; until 2026-11-15 lite's own words,
        // "EnumerationMapping 'null' maps value 'C' ...", an anonymous mapping's id printed as 'null'
        assertEquals(List.of("[6:1] Can't find enum value 'C' in enumeration 'w::E'"), strict(ENUMERATION_VALUE_UNKNOWN));
    }

    // ---- item 5 (S376): a view's missing join in the engine's words ----

    private static final String VIEW_MISSING_JOIN = """
            Class w::P { a: String[0..1]; }
            ###Relational
            Database w::Db (
              Table T (ID INTEGER PRIMARY KEY, NAME VARCHAR(20), PID INTEGER)
              Table T2 (ID INTEGER PRIMARY KEY, NAME VARCHAR(20))
              View V (ID: T.ID PRIMARY KEY, A: @Nope | T2.NAME)
            )
            ###Mapping
            Mapping w::M ( w::P: Relational { ~mainTable [w::Db]V a: [w::Db]V.A } )
            """;

    @Test
    @DisplayName("item 5: a view's join its database has not is the engine's \"Can't find join\"")
    void aViewsMissingJoinIsTheEnginesRefusal() {
        // asked of the engine: "Can't find join 'Nope' in database 'Db'" (HelperRelationalBuilder.getJoin, the
        // database's own name); until 2026-11-15 lite's own words, "Join 'Nope' not found in db 'w::Db'; PM='A',
        // store=w::Db"
        String refusal = "Can't find join 'Nope' in database 'Db'";
        assertEquals(List.of(refusal), strict(VIEW_MISSING_JOIN));
        Compiler.BuiltModule m = module(VIEW_MISSING_JOIN);
        assertEquals(refusal, m.walls().get("w::Db$view$V"));
    }

    // ---- item 2 (census ASK 13): a view-backed set's enumeration over an expression reads through the frame ----

    private static final String ENUMERATED_EXPRESSION_UNDER_A_VIEW = """
            Enum w::E { A, B }
            Class w::P { id: Integer[0..1]; e: w::E[0..1]; }
            ###Relational
            Database w::Db (
              Table T (ID INTEGER PRIMARY KEY, C VARCHAR(5))
              View V (ID: T.ID PRIMARY KEY, CC: T.C)
            )
            ###Mapping
            Mapping w::M (
              w::E: EnumerationMapping em { A: 'a', B: 'b' }
              w::P: Relational { ~mainTable [w::Db]V id: [w::Db]V.ID, e: EnumerationMapping em: concat([w::Db]T.C, '') }
            )
            """;

    @Test
    @DisplayName("item 2: a view-backed set's EnumerationMapping over an expression reads the root through the view's frame")
    void anEnumeratedExpressionUnderAViewReadsThroughTheFrame() {
        // asked of the engine: the model compiles; until 2026-11-15 lite refused it ("ColumnRef references table 'T' not
        // in scope; available=[V]"), the frame rewriting an Expression's T.C to the view's CC but not an
        // EnumeratedExpression's
        assertEquals(List.of(), strict(ENUMERATED_EXPRESSION_UNDER_A_VIEW));
        assertEquals(Map.of(), module(ENUMERATED_EXPRESSION_UNDER_A_VIEW).walls());
    }

    // ---- item 11 (S394): an XStore association's two lines compared modulo AND/OR commutation ----

    private static String xstoreLines(String firm, String people) {
        return """
                Class w::P { name: String[0..1]; fid: Integer[0..1]; }
                Class w::F { id: Integer[0..1]; alt: Integer[0..1]; }
                Association w::P_F { firm: w::F[0..1]; people: w::P[*]; }
                ###Relational
                Database w::Db ( Table P (ID INTEGER PRIMARY KEY, NAME VARCHAR(20), FID INTEGER) Table F (ID INTEGER PRIMARY KEY, ALT INTEGER) )
                ###Mapping
                Mapping w::M (
                  w::P[p]: Relational { ~mainTable [w::Db]P name: [w::Db]P.NAME, fid: [w::Db]P.FID }
                  w::F[f]: Relational { ~mainTable [w::Db]F id: [w::Db]F.ID, alt: [w::Db]F.ALT }
                  w::P_F: XStore { firm[p, f]: %s, people[f, p]: %s }
                )
                """.formatted(firm, people);
    }

    @Test
    @DisplayName("item 11: an XStore association's two lines agree modulo AND/OR commutation, whatever their source positions")
    void xstoreLinesAgreeModuloCommutation() {
        // the two lines are one predicate, their conjuncts commuted and each equality's operands swapped: compared
        // modulo the commutation of AND / OR by record equality since 2026-11-15 (the Rust's verdict since the slice:
        // its order is Java's equality key); the column-space emission's reads carry no source position, so the
        // toString order before agreed here too -- the position accident is the unit case below
        String agree = xstoreLines("$this.fid == $that.id && $this.fid == $that.alt",
                "$this.alt == $that.fid && $this.id == $that.fid");
        assertEquals(List.of(), strict(agree));
        Compiler.BuiltModule m = module(agree);
        assertEquals(Map.of(), m.walls());
        assertEquals(java.util.Optional.empty(), m.context().mappingAssociationPoison("w::M", "w::P_F"));
        // a genuinely different conjunct is still direction-specific
        String differ = xstoreLines("$this.fid == $that.id && $this.fid == $that.alt",
                "$this.id == $that.fid && $this.id == $that.fid");
        assertEquals("XStore association 'w::P_F' has direction-specific conditions; a single shared predicate is"
                        + " required for now (mapping=w::M)",
                module(differ).context().mappingAssociationPoison("w::M", "w::P_F").orElseThrow());
    }

    @Test
    @DisplayName("item 11: sameModuloCommutation -- an AND/OR pair's operands in either order, nested too; nothing else commutes")
    void sameModuloCommutationIsRecordEqualityUpToAndOrOrder() {
        String and = com.legend.builtin.Pure.AND__BOOLEAN_1__BOOLEAN_1.qualifiedName();
        String or = com.legend.builtin.Pure.OR__BOOLEAN_1__BOOLEAN_1.qualifiedName();
        java.util.function.BiFunction<String, List<com.legend.protocol.spec.ValueSpecification>,
                com.legend.protocol.spec.ValueSpecification> call =
                (f, ps) -> new com.legend.protocol.spec.AppliedFunction(f, ps, List.of());
        com.legend.protocol.spec.ValueSpecification x = new com.legend.protocol.spec.CString("x");
        com.legend.protocol.spec.ValueSpecification y = new com.legend.protocol.spec.CString("y");
        com.legend.protocol.spec.ValueSpecification z = new com.legend.protocol.spec.CString("z");
        assertEquals(true, MappingNormalizer.sameModuloCommutation(call.apply(and, List.of(x, y)),
                call.apply(and, List.of(y, x))));
        assertEquals(true, MappingNormalizer.sameModuloCommutation(
                call.apply(and, List.of(x, call.apply(or, List.of(y, z)))),
                call.apply(and, List.of(call.apply(or, List.of(z, y)), x))));
        assertEquals(false, MappingNormalizer.sameModuloCommutation(call.apply(and, List.of(x, y)),
                call.apply(and, List.of(x, z))));
        assertEquals(false, MappingNormalizer.sameModuloCommutation(call.apply(and, List.of(x, y)),
                call.apply(or, List.of(y, x))));
        // two equal conditions whose conjuncts share a first operand at other source positions: until 2026-11-15
        // canonicalizeEqualOperands ordered the conjuncts by toString, which reads that operand's position before the
        // second operand, so the two canonical forms differed ([id, alt] and [alt, id]; asked of the base's jars,
        // port-scratch/normbatch/i11check/OldOrder.java) and the association was walled as direction-specific
        String eqf = com.legend.builtin.Pure.EQUAL__ANY_MANY__ANY_MANY.qualifiedName();
        java.util.function.BiFunction<String, Integer, com.legend.protocol.spec.ValueSpecification> fidIs =
                (q, col) -> call.apply(eqf, List.of(
                        new com.legend.protocol.spec.AppliedProperty(new com.legend.protocol.spec.Variable("src"), "fid",
                                new com.legend.protocol.SourceInfo("m.pure", 1, col, 1, col + 5), null),
                        new com.legend.protocol.spec.AppliedProperty(new com.legend.protocol.spec.Variable("tgt"), q,
                                new com.legend.protocol.SourceInfo("m.pure", 1, col + 10, 1, col + 15), null)));
        com.legend.protocol.spec.ValueSpecification line1 =
                call.apply(and, List.of(fidIs.apply("id", 10), fidIs.apply("alt", 40)));
        com.legend.protocol.spec.ValueSpecification line2 =
                call.apply(and, List.of(fidIs.apply("alt", 70), fidIs.apply("id", 99)));
        assertEquals(true, MappingNormalizer.sameModuloCommutation(
                MappingNormalizer.canonicalizeEqualOperands(line1, "src"),
                MappingNormalizer.canonicalizeEqualOperands(line2, "src")));
        // a call that does not commute keeps its order
        assertEquals(false, MappingNormalizer.sameModuloCommutation(call.apply("minus", List.of(x, y)),
                call.apply("minus", List.of(y, x))));
    }
}
