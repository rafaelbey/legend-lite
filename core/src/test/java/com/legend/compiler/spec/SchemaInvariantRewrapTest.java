// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

package com.legend.compiler.spec;

import com.legend.error.SchemaInvariantException;
import com.legend.error.TypeInferenceException;
import org.junit.jupiter.api.Test;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertInstanceOf;
import static org.junit.jupiter.api.Assertions.assertSame;

/**
 * The owner's ruling 44, ASK 1 (b') (the typer slice's stage 1c, 2026-11-17; register row S1012): Overloads'
 * inlineNormalized rewraps a refusal raised in an expansion that renamed columns with those names restored; a schema
 * invariant keeps its class there, so checkWithDeferred's loop, which stops at a SchemaInvariantException, stops at it
 * (until then it was demoted to a plain TypeInferenceException and the loop tried the next candidate). No input we hold
 * reaches the rewrap (the stage 1b and 1c counters fired on no TypedAnswers ask and no probe question), so this pins
 * the rewrap's carrier: the restored text, the class, the cause.
 */
class SchemaInvariantRewrapTest {

    @Test
    void aRewrappedInvariantKeepsItsClass() {
        SchemaInvariantException cause = new SchemaInvariantException("duplicate column 'made_0'");
        TypeInferenceException rewrapped = new SchemaInvariantException("duplicate column 'made'", cause);
        assertInstanceOf(SchemaInvariantException.class, rewrapped);
        assertEquals("duplicate column 'made'", rewrapped.getMessage());
        assertSame(cause, rewrapped.getCause());
    }
}
