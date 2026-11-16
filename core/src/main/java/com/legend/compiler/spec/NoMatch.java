// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0
package com.legend.compiler.spec;

import com.legend.compiler.element.TypedFunction;
import com.legend.compiler.element.type.ExprType;
import com.legend.compiler.element.type.Multiplicity;
import com.legend.compiler.element.type.Type;
import com.legend.protocol.spec.AppliedFunction;

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
 * the engine's {@code GenericType.print(..., false, ...)} does, a function type's arrow unspaced. The call is named as
 * it was written, as the engine names it: a full path when written in full, else the short name (the owner's ruling of
 * 2026-11-16 on register row S1007; run, port-scratch/typer1b/engine: {@code ->meta::pure::functions::math::minus()}
 * is "Can't find a match for function 'meta::pure::functions::math::minus(String[2])'"; {@link #written}). One
 * departure stays, lite's, ruled: the candidates come in lite's catalog order (the engine's is its handlers').
 */
final class NoMatch {

    private NoMatch() {
    }

    /** {@code written}: the call's name as written ({@link #written(AppliedFunction)}), printed as it is. */
    static String text(String written, List<ExprType> args, List<TypedFunction> candidates) {
        StringBuilder sb = new StringBuilder("Can't find a match for function '")
                .append(written).append('(')
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

    /**
     * The name {@code af} was written with. Name resolution qualifies a bare call to its full name (a platform's and a
     * program's alike: {@code ->minus()} and {@code ->meta::pure::functions::math::minus()} are one resolved call), so
     * the spelling is read off the parser's span of the call, which is the name's: the resolved name's ending of that
     * width, where a path segment starts -- the whole name for a call written in full, the short name for a bare one.
     * A call with no span on one line (read from JSON without positions, or built by the compiler), an operator
     * written infix, an island, or a width that starts no segment, is named by its short name, as before 2026-11-16.
     */
    static String written(AppliedFunction af) {
        String name = af.function();
        com.legend.protocol.SourceInfo p = af.pos();
        if (af.infix() || af.island() || p == null || p.startLine() != p.endLine()) {
            return SpecCompiler.simpleName(name);
        }
        int width = p.endColumn() - p.startColumn() + 1;
        if (width == name.length()) {
            return name;
        }
        if (width > 0 && width < name.length() - 2 && name.startsWith("::", name.length() - width - 2)) {
            return name.substring(name.length() - width);
        }
        return SpecCompiler.simpleName(name);
    }

    /** {@code name(T[m],...):R[m]}, the engine's {@code functionDescriptorToId}. */
    static String signature(TypedFunction f) {
        return SpecCompiler.simpleName(f.qualifiedName()) + "("
                + f.parameters().stream().map(p -> shown(p.type(), p.multiplicity())).collect(Collectors.joining(","))
                + "):" + shown(f.returnType(), f.returnMultiplicity());
    }

    private static String shown(Type t, Multiplicity m) {
        // a function type as the engine prints it, {->T[m]} (lite's own spelling spaces the arrow)
        return t.typeName().replaceAll("(?:[A-Za-z_][A-Za-z0-9_]*::)+", "").replace(" -> ", "->") + m.text();
    }
}
