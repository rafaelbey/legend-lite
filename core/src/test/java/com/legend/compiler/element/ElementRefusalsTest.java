// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

package com.legend.compiler.element;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertThrows;
import static org.junit.jupiter.api.Assertions.assertTrue;

import com.legend.Compiler;
import com.legend.error.ModelException;
import java.util.Map;
import org.junit.jupiter.api.DisplayName;
import org.junit.jupiter.api.Test;

/**
 * Step F's refusals the element slice made before F's oracle froze (docs/rust-port/PHASE3.md, the element slice, step
 * 1, 2026-11-15): a raw Java exception a user's model text reached made a refusal by name (ruling 29's second policy,
 * census ASK 11), each shown reached from a model text first.
 */
class ElementRefusalsTest {

    /** A tolerant build keeps a walled cyclic class (POISON-NOT-DROP). */
    private static final String CYCLE = "Class w::A extends w::B { a: String[1]; }\n"
            + "Class w::B extends w::A { b: String[1]; }\n"
            + "Class w::C extends w::A { c: String[1]; }\n";

    @Test
    @DisplayName("1 (a): findProperty on a walled cyclic class is refused by name, where it overflowed the stack")
    void findPropertyOnAWalledCyclicClassIsRefusedByName() {
        Compiler.BuiltModule module = Compiler.buildModule(Compiler.parseModel(CYCLE));
        assertEquals(Map.of(
                "w::A", "Inheritance cycle: w::A -> w::B -> w::A",
                "w::B", "Inheritance cycle: w::B -> w::A -> w::B",
                "w::C", "Inheritance cycle: w::C -> w::A -> w::B -> w::A"), module.walls());
        // a name the cycle declares is found as before, its own or a supertype's
        assertEquals("a", module.context().findProperty("w::A", "a").orElseThrow().name());
        assertEquals("b", module.context().findProperty("w::A", "b").orElseThrow().name());
        assertEquals("a", module.context().findProperty("w::C", "a").orElseThrow().name());
        // a name it does not declare: until 2026-11-15 a StackOverflowError; now ModelIntegrity's words for the same
        // cycle, attributed to the class the walk starts from, as its wall is
        ModelException e = assertThrows(ModelException.class,
                () -> module.context().findProperty("w::A", "nope"));
        assertEquals("Inheritance cycle: w::A -> w::B -> w::A", e.getMessage());
        assertEquals("w::A", e.element());
        ModelException fromC = assertThrows(ModelException.class,
                () -> module.context().findProperty("w::C", "nope"));
        assertEquals("Inheritance cycle: w::C -> w::A -> w::B -> w::A", fromC.getMessage());
        assertEquals("w::C", fromC.element());
    }

    @Test
    @DisplayName("1 (a): a diamond is no cycle: a class met twice on two paths is walked again, not refused")
    void aDiamondIsNoCycle() {
        String diamond = "Class w::Top { t: String[1]; }\n"
                + "Class w::L extends w::Top { }\n"
                + "Class w::R extends w::Top { }\n"
                + "Class w::D extends w::L, w::R { }\n";
        var ctx = Compiler.compileModel(diamond);
        assertTrue(ctx.findProperty("w::D", "nope").isEmpty());
        assertEquals("t", ctx.findProperty("w::D", "t").orElseThrow().name());
    }
}
