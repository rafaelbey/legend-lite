// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

package com.legend.compiler.spec;

import com.legend.compiler.element.PureModelContext;
import com.legend.compiler.element.type.ExprType;
import com.legend.compiler.element.type.Type;
import com.legend.compiler.spec.typed.TypedSpec;
import com.legend.protocol.spec.ValueSpecification;
import org.junit.jupiter.api.Test;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertNotSame;
import static org.junit.jupiter.api.Assertions.assertNull;
import static org.junit.jupiter.api.Assertions.assertSame;

/**
 * The owner's ruling 44, ASK 2's open half (the typer slice's stage 1c, 2026-11-17): a subtree a pure-repeat site has
 * typed is not typed again in the same environment -- its typing is reused, the answer the same (the typer slice's
 * stage 1b measured no answer moving, and the counts of seven identity tables) -- and a let's right-hand side read
 * again through the alias channel is the let's typing while every variable it reads is bound as it was at the let.
 */
class TypedOnceTest {

    private static Typer typer() {
        PureModelContext ctx = (PureModelContext) com.legend.Compiler.buildModel(
                com.legend.testing.Own.model("Class model::P {}\n"));
        return new Typer(ctx, new InferenceKernel(ctx));
    }

    private static ValueSpecification spec(String src) {
        return com.legend.compiler.NameResolver.resolveQuery(com.legend.testing.Own.spec(src));
    }

    @Test
    void aSubtreeTypedAtARepeatSiteIsNotTypedAgainInItsEnvironment() {
        Typer t = typer();
        ValueSpecification node = spec("$a + 1");
        Env env = Env.empty().with("a", ExprType.one(Type.Primitive.INTEGER));
        TypedSpec typing = t.synth(node, env);
        t.typedOnce(node, typing, env);
        assertSame(typing, t.synth(node, env));
        // another environment types it again
        Env other = Env.empty().with("a", ExprType.one(Type.Primitive.FLOAT));
        TypedSpec again = t.synth(node, other);
        assertNotSame(typing, again);
        assertEquals("Float", again.info().type().typeName().replaceAll(".*::", ""));
    }

    @Test
    void aLetsValueIsTheLetsTypingWhileItsVariablesAreBoundAsAtTheLet() {
        Typer t = typer();
        ValueSpecification rhs = spec("$a + 1");
        Env atLet = Env.empty().with("a", ExprType.one(Type.Primitive.INTEGER));
        TypedSpec typing = t.synth(rhs, atLet);
        t.letTyped(rhs, typing, atLet);
        assertSame(typing, t.letTyping(rhs, Env.empty().with("a", ExprType.one(Type.Primitive.INTEGER))));
        // a later let re-binding $a (ASK 12's case): typed again, as before
        assertNull(t.letTyping(rhs, Env.empty().with("a", ExprType.one(Type.Primitive.FLOAT))));
    }
}
