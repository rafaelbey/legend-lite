// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

package com.legend.compiler.spec;

import com.legend.compiler.element.ModelContext;
import com.legend.error.NotImplementedException;
import org.junit.jupiter.api.DisplayName;
import org.junit.jupiter.api.Test;

import java.util.Map;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertThrows;

/**
 * Ruling 29's policy 2 (the owner's ruling 42; the typer slice's stage 1a item 5) at the two sites only lite's
 * extension grammar reaches (a MARKED extension-test host, parser-equivalence's OwnDialectCensusTest): a function
 * generic over relations whose expanded body sorts static keys of two kinds (StaticFold's sortBy, a ClassCastException
 * until 2026-11-16), and a function-typed parameter as asOfJoin's condition (AsOfJoinChecker, a ClassCastException);
 * legend-engine 4.145.0's grammar refuses both inputs (run: "Type and/or multiplicity parameters are not authorized in
 * Legend Engine", "The type {Any[1],Any[1]->Boolean[1]} is not supported yet").
 */
class ExtensionGrammarRefusalsTest {

    private static final ModelContext CTX = com.legend.Compiler.compileModel("""
            ###Pure
            function t::sf<T>(r: meta::pure::metamodel::relation::Relation<T>[1]): meta::pure::metamodel::relation::Relation<T>[1]
            {
              $r->filter(x|[1, 2.5]->sortBy(y|$y)->size() > 0)
            }
            function t::aoj(c: Function<{Any[1],Any[1]->Boolean[1]}>[1]): Any[*]
            {
              #>{t::Db.T}#->select(~[ID, A])->asOfJoin(#>{t::Db.T2}#, {x,y|$x.A > $y.K}, $c)
            }
            ###Relational
            Database t::Db ( Table T (ID INTEGER PRIMARY KEY, A INTEGER) Table T2 (K INTEGER PRIMARY KEY) )
            """);

    @Test
    @DisplayName("sortBy over static keys of two kinds in an expanded body: refused, never a ClassCastException")
    void aStaticSortOfMixedKindsIsRefused() {
        assertEquals("sortBy over static keys of different kinds cannot be folded", assertThrows(
                NotImplementedException.class, () -> com.legend.Compiler.query(CTX,
                        "#>{t::Db.T}#->select(~[A])->t::sf()").resultType()).getMessage());
    }

    @Test
    @DisplayName("asOfJoin with a function value as its condition: refused in the call's words")
    void anAsOfJoinsNonLambdaConditionIsRefused() {
        Map<String, String> walls = com.legend.Compiler.compileAllBodies(CTX);
        assertEquals("in function 't::aoj': asOfJoin expects (rel1, rel2, {t,v|match} [, {t,v|cond} [, 'prefix']])",
                walls.get("t::aoj_Function_1__Any_MANY_"), String.valueOf(walls));
    }
}
