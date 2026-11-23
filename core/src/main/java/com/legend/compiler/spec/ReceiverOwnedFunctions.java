// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

package com.legend.compiler.spec;

import com.legend.builtin.Pure;
import com.legend.error.TypeInferenceException;
import com.legend.compiler.element.TypedFunction;
import com.legend.compiler.element.type.Type;
import com.legend.protocol.spec.AppliedFunction;
import com.legend.protocol.spec.ColSpec;
import com.legend.protocol.spec.ColSpecArray;
import com.legend.protocol.spec.LambdaFunction;
import com.legend.protocol.spec.PureCollection;
import com.legend.protocol.spec.TypeAnnotation;
import com.legend.protocol.spec.ValueSpecification;

import java.util.List;

/**
 * A model function written as the receiver's OWN ({@code join(_this:Database[1], name:String[1])}, relational.pure's
 * convention) sharing a name with a platform operator family (sort, join) whose call fits it. legend-engine has two
 * compilers, and the graph's grammar level says which one's rule holds (register row S1026; the owner's ruling 44, ASK
 * 15 (d), 2026-11-17, and the coordinator's ruling of 2026-11-23): at the engine's model compiler's levels
 * (LEGEND_ENGINE, LEGEND_LITE) the call is refused in legend-engine's words -- the platform's functions take the name, a
 * model function of that name is no candidate; at the platform's (LEGEND_PLATFORM: the engine's own Pure, which
 * legend-pure compiles by type) the receiver's function takes the call, as it did before 2026-11-17 (census batch 150).
 * A call written in full ({@code ->t::sort('x')}) names the model's function and never reaches here. Split from Typer
 * for size (CodeShape guard).
 */
final class ReceiverOwnedFunctions {

    private ReceiverOwnedFunctions() {
    }

    /** The MODEL function of a platform form's name this call would take (its first parameter accepting the
     * receiver's class, named {@code _this}): at LEGEND_PLATFORM that function, at the model compiler's levels a
     * refusal in the engine's words (S1026); null otherwise -- when the receiver is not a plain class instance (a
     * relation, a TDS, a lambda, a collection) or no such function fits, and the caller types the special form. */
    static @com.legend.base.Nullable TypedFunction of(Typer t, AppliedFunction af, Env env) {
        if (af.parameters().isEmpty()) {
            return null;
        }
        ValueSpecification recv = af.parameters().get(0);
        if (recv instanceof LambdaFunction || recv instanceof PureCollection
                || recv instanceof ColSpec || recv instanceof ColSpecArray) {
            return null;
        }
        // a type-annotation argument is the special form's own spelling
        // ($x->cast(@T)) — never a model call
        if (af.parameters().stream().anyMatch(p -> p instanceof TypeAnnotation)) {
            return null;
        }
        // a special form with NO native signature (tableToTDS, new, let,
        // cast…) is the platform's definition outright; only an operator
        // FAMILY with natives (filter, join, sort) can yield to a model
        // function over a class its natives never take
        if (com.legend.compiler.ResolvedNames.declaredNatives(af).isEmpty()) {
            return null;
        }
        List<TypedFunction> cands = t.functionCandidates(af);
        // the receiver's OWN function: the spec writes a class-owned function
        // with its first parameter named _this (join(_this:Database[1], …),
        // relational.pure) — the qualified-property convention the derived-
        // property routing above already honours; an ordinary function over
        // a class (tableToTDS(table:Table[1])) never displaces a special form
        List<TypedFunction> owned = cands.stream()
                .filter(f -> !f.isNative() && f.parameters().size() == af.parameters().size()
                        && f.parameters().get(0).type() instanceof Type.ClassType
                        && "_this".equals(f.parameters().get(0).name()))
                .toList();
        if (owned.isEmpty()) {
            return null;
        }
        com.legend.compiler.spec.typed.TypedSpec recvTyped = t.synth(recv, env);
        t.typedOnce(recv, recvTyped, env);   // typed once (REPEAT-11): the route taken types it again
        Type rt = recvTyped.info().type();
        if (Type.isRelation(rt) || rt instanceof Type.RelationType) {
            return null;
        }
        String raw = rt instanceof Type.ClassType c ? c.fqn()
                : rt instanceof Type.GenericType g ? g.rawFqn() : null;
        if (raw == null) {
            return null;
        }
        // the platform's OWN native taking this class receiver is the
        // definition (tableToTDS(table:Table[1]) — post-processors are
        // compiler passes, tableToTDS a platform semantic)
        for (TypedFunction n : cands) {
            if (n.isNative() && n.parameters().size() == af.parameters().size()
                    && n.parameters().get(0).type() instanceof Type.ClassType nc
                    && (nc.fqn().equals(raw) || t.model().isSubtype(raw, nc.fqn()))) {
                return null;
            }
        }
        for (TypedFunction f : owned) {
            String pc = ((Type.ClassType) f.parameters().get(0).type()).fqn();
            if (!(pc.equals(raw) || t.model().isSubtype(raw, pc))) {
                continue;
            }
            // every other NON-lambda argument must fit its parameter too
            boolean fits = true;
            for (int i = 1; i < af.parameters().size() && fits; i++) {
                ValueSpecification p = af.parameters().get(i);
                if (p instanceof LambdaFunction) {
                    continue;
                }
                com.legend.compiler.spec.typed.TypedSpec pt = t.synth(p, env);
                t.typedOnce(p, pt, env);   // typed once (REPEAT-12): the route taken (and a further candidate) types it again
                fits = t.kernel().accepts(f.parameters().get(i).type(), pt.info().type());
            }
            if (fits) {
                // the grammar level decides (2026-11-23, the coordinator's ruling on S1026): legend-pure compiles the
                // engine's own Pure by type -- relational.pure's meta::relational::metamodel::filter and join, whose
                // bodies call $db->filter(...) and ->join(...) -- so at LEGEND_PLATFORM the receiver's function takes
                // the call, as before 2026-11-17
                if (!t.model().grammarLevel().followsTheModelCompiler()) {
                    return f;
                }
                // the owner's ruling 44, ASK 15 (d) (the typer slice's stage 1c, 2026-11-17; register row S1026): at the
                // engine's model compiler's levels a model function of a platform form's name never displaces the
                // form, whatever its first parameter is named -- legend-engine 4.145.0 refuses `^t::Box(v=1)->sort('x')`
                // under `import t::*;` with `t::sort(_this:t::Box[1], k:String[1])` when the model compiles, listing
                // the platform's sorts alone (run, port-scratch/typer1c/research/s1026/r7.out; with the first
                // parameter named b, gcensus r4; the control t::sortz compiles, r8)
                throw new TypeInferenceException(NoMatch.text(NoMatch.written(af),
                        af.parameters().stream().map(a -> t.synth(a, env).info()).toList(),
                        cands.stream().filter(TypedFunction::isNative).toList()));
            }
        }
        return null;
    }

}
