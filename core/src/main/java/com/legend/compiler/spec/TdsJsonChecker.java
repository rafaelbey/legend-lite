// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0
package com.legend.compiler.spec;

import com.legend.compiler.element.type.ExprType;
import com.legend.compiler.element.type.Type;
import com.legend.compiler.spec.typed.TypedJsonResult;
import com.legend.compiler.spec.typed.TypedSpec;
import com.legend.protocol.spec.AppliedFunction;
import java.util.List;

/**
 * {@code toJSON(tds)} (REAL declaration: {@code meta::json::toJSON(obj:
 * Any[*]):String[1]}) over a TABULAR argument — the engine serializes a
 * TabularDataSet as {@code {"columns":[{"name","type","metaType"}],
 * "rows":[{"values":[..]}]}} (toJSON.pure's TabularDataSet arm). The
 * document is EMITTED by the database over the chain (a
 * {@link TypedJsonResult} of kind TDS_JSON — one scalar subquery
 * aggregating the rows, the column metadata a static fact of the typed
 * relation). Every other argument shape rides the generic native.
 */
final class TdsJsonChecker {
    private static final String TO_JSON_FQN = "meta::json::toJSON";

    private TdsJsonChecker() {
    }

    static TypedSpec check(Typer t, AppliedFunction af, Env env) {
        return check(t, af, env, TO_JSON_FQN, TypedJsonResult.Kind.TDS_JSON,
                ExprType.one(Type.Primitive.STRING));
    }

    private static final String TDS_KV_FQN = "meta::json::tdsToJSONKeyValueObjectString";

    /** {@code tdsToJSONKeyValueObjectString(tds):String[*]} (toJSON.pure:231
     * — the engine streams the document as string FRAGMENTS; ours is the
     * whole document as ONE string, the value the fragments concatenate
     * to — the consumer's {@code makeString()} over it is the identity).
     * The one-element narrowing of the declared [*] is sound; the
     * registered signature still validates the argument. */
    static TypedSpec checkKeyValue(Typer t, AppliedFunction af, Env env) {
        return check(t, af, env, TDS_KV_FQN, TypedJsonResult.Kind.TDS_JSON_KV,
                ExprType.one(Type.Primitive.STRING));
    }

    private static TypedSpec check(Typer t, AppliedFunction af, Env env,
            String fqn, TypedJsonResult.Kind kind, ExprType out) {
        if (af.parameters().size() != 1) {
            return t.applyGeneric(af, env);
        }
        TypedSpec arg = t.synth(af.parameters().get(0), env);
        t.typedOnce(af.parameters().get(0), arg, env);   // typed once (REPEAT-10): declined, applyGeneric types it again
        Type.RelationType rt = Type.relationSchema(arg.info().type());
        // the key-value document needs no column types: a TDS-typed value
        // whose schema is only visible after the envelope splice (an
        // executed plan's values cast to TabularDataSet) qualifies too
        boolean tdsValue = kind == TypedJsonResult.Kind.TDS_JSON_KV
                && com.legend.compiler.element.type.PlatformTypes.isTdsShaped(arg.info().type());
        if ((rt == null && !tdsValue) || (rt != null && rt.isLateBound())) {
            return t.applyGeneric(af, env);
        }
        // validate against the REGISTERED native signature — never bypassed
        t.kernel().resolveOverload(t.model().findFunction(fqn),
                List.of(arg.info()));
        return new TypedJsonResult(arg, kind, null, out);
    }
}
