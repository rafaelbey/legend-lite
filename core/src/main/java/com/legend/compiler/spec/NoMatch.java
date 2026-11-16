// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0
package com.legend.compiler.spec;

import com.legend.compiler.element.TypedFunction;
import com.legend.compiler.element.type.ExprType;
import com.legend.compiler.element.type.Multiplicity;
import com.legend.compiler.element.type.Type;

import java.util.ArrayList;
import java.util.List;
import java.util.stream.Collectors;

/**
 * The engine's words for a call no overload takes, where lite refuses the same input when the engine does, at typing
 * (the owner's ruling 42, the G census's ASK 4: no Java record's {@code toString} in an answer): legend-engine 4.145.0
 * {@code ValueSpecificationBuilder.java:978-986} ("Can't find a match for function 'f(T[m],...)'.") and
 * {@code CompileContext.getMismatchFunctionExpressionDetailMessage} ({@code :489-512}: the candidates of the call's
 * arity under "if parameter types or multiplicities are changed", the others under "if number of parameters are
 * changed", each {@code name(T[m],...):R[m]} after a newline and two tabs), read; run on
 * {@code |['a','b']->minus()} and {@code |if(true)} (port-scratch/typer/engine). Types print by their simple names, as
 * the engine's {@code GenericType.print(..., false, ...)} does. Two departures, lite's: the function is named by its
 * simple name (the engine prints the call as written, a full path when written in full), and the candidates come in
 * lite's catalog order (the engine's is its handlers').
 */
final class NoMatch {

    private NoMatch() {
    }

    static String text(String function, List<ExprType> args, List<TypedFunction> candidates) {
        StringBuilder sb = new StringBuilder("Can't find a match for function '")
                .append(SpecCompiler.simpleName(function)).append('(')
                .append(args.stream().map(a -> shown(a.type(), a.multiplicity())).collect(Collectors.joining(",")))
                .append(")'.\n");
        List<String> same = new ArrayList<>();
        List<String> other = new ArrayList<>();
        for (TypedFunction c : candidates) {
            (c.parameters().size() == args.size() ? same : other).add(signature(c));
        }
        if (!same.isEmpty()) {
            sb.append("Functions that can match if parameter types or multiplicities are changed:\n\t\t")
                    .append(String.join("\n\t\t", same)).append('\n');
        }
        if (!other.isEmpty()) {
            sb.append("Functions that can match if number of parameters are changed:\n\t\t")
                    .append(String.join("\n\t\t", other)).append('\n');
        }
        return sb.toString();
    }

    /** {@code name(T[m],...):R[m]}, the engine's {@code functionDescriptorToId}. */
    static String signature(TypedFunction f) {
        return SpecCompiler.simpleName(f.qualifiedName()) + "("
                + f.parameters().stream().map(p -> shown(p.type(), p.multiplicity())).collect(Collectors.joining(","))
                + "):" + shown(f.returnType(), f.returnMultiplicity());
    }

    private static String shown(Type t, Multiplicity m) {
        return t.typeName().replaceAll("(?:[A-Za-z_][A-Za-z0-9_]*::)+", "") + m.text();
    }
}
