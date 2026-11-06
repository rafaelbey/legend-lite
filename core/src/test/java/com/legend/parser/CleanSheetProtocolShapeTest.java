// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

package com.legend.parser;

import com.legend.model.LegacyMappingDefinition;
import com.legend.model.MappingDefinition;
import com.legend.model.PackageableElement;
import com.legend.protocol.Realization;
import org.junit.jupiter.api.Test;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertFalse;
import static org.junit.jupiter.api.Assertions.assertInstanceOf;
import static org.junit.jupiter.api.Assertions.assertTrue;

/**
 * THE CLEAN-SHEET ARM'S ONLY SAFETY NET.
 *
 * <p>The corpus contains ZERO function-form mappings — the clean-sheet form
 * is legend-lite's own surface and no legend-engine file uses it — so no
 * corpus sweep covers this arm at all. That is the inverse of the situation
 * everywhere else in the migration, and therefore exactly where a silent gap
 * survives. It has already caught an entire unbuilt kind tag.
 *
 * <p>This replaces {@code CleanSheetProtocolEquivalenceTest}, which asserted
 * that the legacy parser and protocol+transform agreed. With the legacy
 * parser deleted there is no second implementation to differ from, so the
 * SAME sources now pin the resulting MODEL directly. That trades a
 * differential for a stated expectation — weaker against a shared
 * misunderstanding, stronger against both paths drifting together, and the
 * only form available once there is one parser.
 */
class CleanSheetProtocolShapeTest {

    /** Parse the sole Mapping element through the one path there now is. */
    private static PackageableElement mapping(String source) {
        return com.legend.model.MappingFromProtocol.toMappingElement(
                com.legend.testing.Platform.mapping(source));
    }

    private static com.legend.model.CleanSheetMappingDefinition cleanSheet(String source) {
        return assertInstanceOf(com.legend.model.CleanSheetMappingDefinition.class,
                mapping(source),
                () -> "a function-form body must yield the clean-sheet"
                        + " surface tree, not the legacy one");
    }

    @Test
    void relationalFunctionRef() {
        var md = cleanSheet("\n###Mapping\nMapping acme::M ( "
                + "  *acme::Person: Relational { acme::funcs::personMapping } "
                + ")");
        var b = md.classBindings().get(0);
        assertEquals("acme::Person", b.classFqn());
        assertEquals(com.legend.model.CleanSheetMappingDefinition.Kind.RELATIONAL, b.kind());
        assertTrue(b.root());
        assertEquals("acme::funcs::personMapping", ((com.legend.protocol.Realization.Ref) b.realization()).functionFqn());
    }

    @Test
    void pureFunctionRef() {
        var md = cleanSheet("\n###Mapping\nMapping acme::M ( "
                + "  acme::StaffMember: Pure { acme::funcs::staffMapping } "
                + ")");
        var b = md.classBindings().get(0);
        assertEquals(com.legend.model.CleanSheetMappingDefinition.Kind.PURE, b.kind());
        assertFalse(b.root(), "no leading * — not the root set");
        assertEquals("acme::funcs::staffMapping", ((com.legend.protocol.Realization.Ref) b.realization()).functionFqn());
    }

    @Test
    void setIdAndExtendsRideTheBinding() {
        var md = cleanSheet("\n###Mapping\nMapping acme::M ( "
                + "  acme::Person[emp] extends [base]: Relational"
                + " { acme::funcs::employeeMapping } "
                + ")");
        var b = md.classBindings().get(0);
        assertEquals("emp", b.setId());
        assertEquals("base", b.extendsSetId());
    }

    @Test
    void associationKindTag() {
        // The tag this family exists to protect: an AssociationMapping in
        // function form lands in associationBindings, not classBindings.
        var md = cleanSheet("\n###Mapping\nMapping acme::M ( "
                + "  *acme::Person: Relational { acme::funcs::personMapping } "
                + "  *acme::Firm:   Relational { acme::funcs::firmMapping } "
                + "  acme::Person_Firm: AssociationMapping"
                + " { acme::funcs::personFirmMatch } "
                + ")");
        assertEquals(2, md.classBindings().size());
        assertEquals(1, md.associationBindings().size());
        var ab = md.associationBindings().get(0);
        assertEquals("acme::Person_Firm", ab.associationFqn());
        assertEquals("acme::funcs::personFirmMatch", ((com.legend.protocol.Realization.Ref) ab.realization()).functionFqn());
    }

    @Test
    void multipleBindingsKeepOrderAndRootMarkers() {
        var md = cleanSheet("\n###Mapping\nMapping acme::M ( "
                + "  *acme::Person: Relational { acme::funcs::personMapping } "
                + "  acme::Firm:    Relational { acme::funcs::firmMapping } "
                + "  acme::Staff:   Pure       { acme::funcs::staffMapping } "
                + ")");
        assertEquals(3, md.classBindings().size());
        assertEquals("acme::Person", md.classBindings().get(0).classFqn());
        assertEquals("acme::Firm", md.classBindings().get(1).classFqn());
        assertEquals("acme::Staff", md.classBindings().get(2).classFqn());
        assertTrue(md.classBindings().get(0).root());
        assertFalse(md.classBindings().get(1).root());
        assertEquals(com.legend.model.CleanSheetMappingDefinition.Kind.PURE,
                md.classBindings().get(2).kind());
    }

    /** The INLINE form — a body that is not a lone element pointer, so it
     *  becomes a lambda on the wire and a Realization.Inline in the model. */
    @Test
    void inlineExpressionBody() {
        var md = cleanSheet("\n###Mapping\nMapping acme::M ( "
                + "  *acme::Person: Relational"
                + " { acme::funcs::rows() -> map(r | ^acme::Person(name = $r.NAME)) } "
                + ")");
        assertInstanceOf(Realization.Inline.class,
                md.classBindings().get(0).realization(),
                "an expression body is realized inline, not by reference");
    }

    /** A legacy body next to the kind tag must STILL take the legacy path —
     *  the disambiguator decides per element, and getting it wrong would
     *  route every relational mapping in the corpus into the wrong arm. */
    @Test
    void legacyBodyStillTakesTheLegacyPath() {
        var el = mapping("\n###Mapping\nMapping acme::M ( "
                + "  *acme::Person: Relational { ~mainTable [acme::DB]personTable"
                + "  name: [acme::DB]personTable.NAME } "
                + ")");
        var legacy = assertInstanceOf(LegacyMappingDefinition.class, el,
                "a legacy DSL body must yield the surface tree");
        assertEquals(1, legacy.classMappings().size());
    }
}
