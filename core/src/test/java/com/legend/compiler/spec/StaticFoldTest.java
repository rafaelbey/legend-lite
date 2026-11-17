// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

package com.legend.compiler.spec;

import com.legend.compiler.element.PureModelContext;
import com.legend.protocol.spec.AppliedFunction;
import com.legend.protocol.spec.CInteger;
import com.legend.protocol.spec.CString;
import com.legend.protocol.spec.PureCollection;
import com.legend.protocol.spec.ValueSpecification;
import org.junit.jupiter.api.Test;

import java.util.List;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertNull;
import static org.junit.jupiter.api.Assertions.assertSame;

/**
 * The static folder's {@code toOne}: a fold may only replace a call by a value the call would certainly produce.
 * {@code toOne} over a collection whose size is not one fails at run time in the reference ("Cannot cast a collection
 * of size N to multiplicity [1]"), so it is NOT static and must stay as written for the checkers and the runtime.
 * Before plan item W0.2(b) the one-argument spelling folded {@code toOne([])} to {@code []} and {@code toOne([1, 2])}
 * to {@code [1, 2]} (StaticFold.java, the TO_ONE arm), silently turning a run-time error into a value.
 */
class StaticFoldTest {

    private static final String TO_ONE = com.legend.builtin.Pure.TO_ONE__T_MANY.qualifiedName();
    private static final String TO_ONE_MSG = com.legend.builtin.Pure.TO_ONE__T_MANY__STRING_1.qualifiedName();

    private static StaticFold folder() {
        PureModelContext ctx = (PureModelContext) com.legend.Compiler.buildModel(
                com.legend.testing.Own.model("Class model::Person {}\n"));
        return new StaticFold(new Typer(ctx, new InferenceKernel(ctx)), null);
    }

    private static PureCollection ints(long... vs) {
        return new PureCollection(java.util.Arrays.stream(vs)
                .mapToObj(v -> (ValueSpecification) new CInteger(v)).toList());
    }

    private static final String PLUS = com.legend.builtin.Pure.PLUS__INTEGER_MANY.qualifiedName();
    private static final String MINUS = com.legend.builtin.Pure.MINUS__INTEGER_MANY.qualifiedName();

    @Test
    void aSumOrADifferencePastALongIsNotStatic() {
        // they wrapped (2026-10-17, the #SQL{} island's regression-suite corpus): the exact folds and the database
        // answer them
        assertEquals(new CInteger(3L), folder().foldToLiteral(new AppliedFunction(PLUS, List.of(ints(1, 2)))));
        assertNull(folder().foldToLiteral(new AppliedFunction(PLUS, List.of(ints(Long.MAX_VALUE, 1)))));
        assertNull(folder().foldToLiteral(new AppliedFunction(MINUS, List.of(ints(Long.MIN_VALUE)))));
        assertNull(folder().foldToLiteral(new AppliedFunction(MINUS, List.of(ints(Long.MIN_VALUE, 1)))));
        assertEquals(new CInteger(-Long.MAX_VALUE),
                folder().foldToLiteral(new AppliedFunction(MINUS, List.of(ints(Long.MAX_VALUE)))));
    }

    @Test
    void toOneOfASingletonFolds() {
        assertEquals(new CInteger(7L),
                folder().foldToLiteral(new AppliedFunction(TO_ONE, List.of(ints(7)))));
    }

    @Test
    void toOneOfAnEmptyCollectionIsNotStatic() {
        assertNull(folder().foldToLiteral(new AppliedFunction(TO_ONE, List.of(ints()))),
                "toOne([]) is a run-time error in the reference; folding it to [] hides the error");
    }

    @Test
    void toOneOfTwoValuesIsNotStatic() {
        assertNull(folder().foldToLiteral(new AppliedFunction(TO_ONE, List.of(ints(1, 2)))),
                "toOne([1, 2]) is a run-time error in the reference; folding it to [1, 2] hides the error");
    }

    @Test
    void theMessageSpellingAgrees() {
        StaticFold f = folder();
        assertNull(f.foldToLiteral(new AppliedFunction(TO_ONE_MSG, List.of(ints(), new CString("m")))));
        assertNull(f.foldToLiteral(new AppliedFunction(TO_ONE_MSG, List.of(ints(1, 2), new CString("m")))));
        assertEquals(new CInteger(7L),
                f.foldToLiteral(new AppliedFunction(TO_ONE_MSG, List.of(ints(7), new CString("m")))));
    }

    @Test
    void anIntegerLiteralPastALongIsKeptExact() {
        // the owner's ruling 44, ASK 3 (b) (the typer slice's stage 1c, 2026-11-17): StaticFold's eval read a literal
        // past 63 bits by longValue(), so toOne([9223372036854775808]) folded to -9223372036854775808
        CInteger big = new CInteger(java.math.BigInteger.ONE.shiftLeft(63));
        assertEquals(big, folder().foldToLiteral(new AppliedFunction(TO_ONE,
                List.of(new PureCollection(List.of(big))))));
    }

    @Test
    void aFloatLiteralKeepsItsExactDigits() {
        // the owner's ruling 44, ASK 3 (b): a float literal carrying the digits a double does not hold folded to the
        // bare double, its exact digits dropped when written back
        com.legend.protocol.spec.CFloat exact = new com.legend.protocol.spec.CFloat(0.1,
                new java.math.BigDecimal("0.1000000000000000000001"), null);
        ValueSpecification folded = folder().foldToLiteral(new AppliedFunction(TO_ONE,
                List.of(new PureCollection(List.of(exact)))));
        assertEquals(exact, folded);
        assertSame(exact.exact(), ((com.legend.protocol.spec.CFloat) folded).exact());
    }
}
