// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

package com.legend.compiler.spec;

import com.legend.compiler.element.ModelContext;
import com.legend.compiler.spec.typed.TypedUserCall;
import org.junit.jupiter.api.DisplayName;
import org.junit.jupiter.api.Test;

import java.util.Map;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertInstanceOf;
import static org.junit.jupiter.api.Assertions.assertTrue;

/**
 * The owner's ruling 44, ASK 15 (e) (the typer slice's stage 1c, 2026-11-17): the typer's platform forms -- the TDS
 * {@code union}, {@code paginated}, {@code extractEnumValue} -- are matched by the call's resolved referents, never by a
 * name's {@code ::}-suffix. Until then a user's {@code my::p::union(1, 2)}, called in full, was desugared to the TDS
 * union: typed Integer[*], planned as a dedup UNION of 1 and 2, and the caller's return refused (run on lite at
 * fbc8f01e9, port-scratch/typer1b/research/ask15e/u.out; a user function is called by its full name in the engine:
 * resolution, read).
 */
class PlatformFormReferentsTest {

    private static final String MODEL = """
            ###Pure
            function my::p::union(x: Integer[1], y: Integer[1]): Integer[1] { $x + $y }
            function my::p::paginated(x: Integer[1], y: Integer[1]): Integer[1] { $x * $y }
            function my::p::useUnion(): Integer[1] { my::p::union(1, 2) }
            function my::p::usePaginated(): Integer[1] { my::p::paginated(3, 4) }

            ###Relational
            Database my::p::Db ( Table T (ID INTEGER PRIMARY KEY, NAME VARCHAR(20)) )
            ###Connection
            RelationalDatabaseConnection my::p::C { store: my::p::Db; type: H2; specification: LocalH2 { }; auth: DefaultH2; }
            ###Runtime
            Runtime my::p::R { mappings: []; connections: [ my::p::Db: [ c: my::p::C ] ]; }
            """;

    private static final ModelContext CTX = com.legend.Compiler.compileModel(MODEL);

    private static String typed(String query) {
        com.legend.TypedQuery q = com.legend.Compiler.query(CTX, query);
        return q.resultType().type().typeName() + q.resultType().multiplicity().text();
    }

    @Test
    @DisplayName("a user's function named as a platform form is the user's function, called in full")
    void aUserFunctionNamedAsAFormIsTheUsers() {
        Map<String, String> walls = com.legend.Compiler.compileAllBodies(CTX);
        assertTrue(walls.isEmpty(), walls.toString());
        assertEquals("Integer[1]", typed("my::p::union(1, 2)"));
        assertInstanceOf(TypedUserCall.class, com.legend.Compiler.query(CTX, "my::p::union(1, 2)").expression());
        assertEquals("Integer[1]", typed("my::p::paginated(3, 4)"));
        assertEquals("Integer[1]", typed("my::p::useUnion()"));
    }

    @Test
    @DisplayName("the platform's union is still the TDS union, bare or in full")
    void thePlatformsUnionIsStillTheForm() {
        assertEquals("Relation<(ID:Integer[1], NAME:String[0..1])>[1]",
                typed("#>{my::p::Db.T}#->union(#>{my::p::Db.T}#)").replaceAll("[a-z]+::", ""));
        assertEquals("Relation<(ID:Integer[1], NAME:String[0..1])>[1]",
                typed("#>{my::p::Db.T}#->meta::pure::functions::collection::union(#>{my::p::Db.T}#)")
                        .replaceAll("[a-z]+::", ""));
    }
}
