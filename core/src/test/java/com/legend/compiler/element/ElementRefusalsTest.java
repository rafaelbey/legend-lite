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

    private static final String BOUND = "upper (1) must be >= lower (2)";

    @Test
    @DisplayName("1 (b): a parameter's, a return's and an association end's bounds are checked as a property's are")
    void inverseBoundsAreRefusedAtEverySignatureSite() {
        // ruling 30 (census ASK 7 (c); LITE-ONLY): until 2026-11-15 the first two compiled, the bound surfacing as
        // compileAllBodies' raw wall, and the third compiled; legend-engine 4.145.0, asked, compiles the parameter and
        // the end and refuses the return only by its body's multiplicity check (the register row)
        String parameter = "function w::f(x: String[2..1]): String[1]\n{\n  'a'\n}\n";
        ModelException p = assertThrows(ModelException.class, () -> Compiler.compileModel(parameter));
        assertEquals("w::f_String_$2_1$__String_1_", p.element());
        assertTrue(p.getMessage().endsWith("parameter 'x' of function w::f: invalid multiplicity — " + BOUND),
                p.getMessage());
        String ret = "function w::g(x: String[1]): String[2..1]\n{\n  $x\n}\n";
        ModelException r = assertThrows(ModelException.class, () -> Compiler.compileModel(ret));
        assertEquals("w::g_String_1__String_$2_1$_", r.element());
        assertTrue(r.getMessage().endsWith("return of function w::g: invalid multiplicity — " + BOUND), r.getMessage());
        String end = "Class w::A { name: String[1]; }\nClass w::B { name: String[1]; }\n"
                + "Association w::AB { a: w::A[2..1]; b: w::B[*]; }\n";
        ModelException a = assertThrows(ModelException.class, () -> Compiler.compileModel(end));
        assertEquals("w::AB", a.element());
        assertTrue(a.getMessage().endsWith("property 'a' of association w::AB: invalid multiplicity — " + BOUND),
                a.getMessage());
        // the control: a class property's check, its words unchanged
        ModelException c = assertThrows(ModelException.class,
                () -> Compiler.compileModel("Class w::C { p: String[2..1]; }\n"));
        assertTrue(c.getMessage().endsWith("property 'p' of w::C: invalid multiplicity — " + BOUND), c.getMessage());
        // a tolerant build walls each, under its key
        assertEquals(Map.of("w::f_String_$2_1$__String_1_",
                "parameter 'x' of function w::f: invalid multiplicity — " + BOUND),
                Compiler.buildModule(Compiler.parseModel(parameter)).walls());
        assertEquals(Map.of("w::AB", "property 'a' of association w::AB: invalid multiplicity — " + BOUND),
                Compiler.buildModule(Compiler.parseModel(end)).walls());
    }

    @Test
    @DisplayName("1 (b): a tolerant build's walled overload is dropped from its set; a set whose every overload breaks rethrows")
    void aBrokenOverloadIsDroppedAndAnAllBrokenSetRethrows() {
        // FunctionCompiler.compileAll's swallow (census ASK 7 (a)), ported as it is: a strict build never reaches it now
        Compiler.BuiltModule module = Compiler.buildModule(Compiler.parseModel(
                "function w::h(x: String[1]): String[1]\n{\n  $x\n}\n"
                        + "function w::h(x: Integer[2..1]): String[1]\n{\n  'n'\n}\n"
                        + "function w::k(x: Integer[2..1]): String[1]\n{\n  'n'\n}\n"));
        assertEquals(java.util.Set.of("w::h_Integer_$2_1$__String_1_", "w::k_Integer_$2_1$__String_1_"),
                module.walls().keySet());
        assertEquals(1, module.context().findFunction("w::h").size());
        IllegalArgumentException k = assertThrows(IllegalArgumentException.class,
                () -> module.context().findFunction("w::k"));
        assertEquals(BOUND, k.getMessage());
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
