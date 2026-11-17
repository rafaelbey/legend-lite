// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

package com.legend.compiler.spec;

import com.legend.compiler.element.type.ExprType;
import com.legend.compiler.element.type.Type;
import com.legend.compiler.spec.typed.TypedTestDataGen;
import org.junit.jupiter.api.Test;

import java.util.List;

import static org.junit.jupiter.api.Assertions.assertFalse;
import static org.junit.jupiter.api.Assertions.assertTrue;

/**
 * The owner's ruling 44, ASK 15 (g) (the typer slice's stage 1c, 2026-11-17): StatementEffects.containsTdgGenerator
 * matched a native call of generateTestData or generateSeedDataString, a node the typer never builds for them -- it
 * builds a TypedTestDataGen carrier -- so programFacts saw no generator (the typer slice's stage 1b measured 204 frozen
 * test-door questions meeting one). The generate and seedString flavors are generators; planTestDataGeneration's plan
 * flavor runs nothing.
 */
class TestDataGeneratorEffectsTest {

    private static TypedTestDataGen carrier(String flavor) {
        return new TypedTestDataGen(List.of(), flavor, ExprType.one(Type.Primitive.STRING));
    }

    @Test
    void aTyperBuiltGeneratorIsSeen() {
        assertTrue(StatementEffects.containsTdgGenerator(carrier("generate")));
        assertTrue(StatementEffects.containsTdgGenerator(carrier("seedString")));
        assertFalse(StatementEffects.containsTdgGenerator(carrier("plan")));
    }
}
