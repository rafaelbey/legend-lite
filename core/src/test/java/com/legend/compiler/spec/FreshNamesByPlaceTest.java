// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

package com.legend.compiler.spec;

import com.legend.compiler.element.ModelContext;
import org.junit.jupiter.api.DisplayName;
import org.junit.jupiter.api.Test;

import java.util.ArrayList;
import java.util.HashSet;
import java.util.List;
import java.util.regex.Matcher;
import java.util.regex.Pattern;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertFalse;
import static org.junit.jupiter.api.Assertions.assertTrue;

/**
 * Names by place (the owner's ruling 42, the G census's ASK 2; the typer slice's stage 1a item 7): AlphaRename's
 * {@code _nr<N>} is minted by one renamer per expansion, its counter from 0, skipping every name the expansion's body,
 * its call's arguments and its place's scope use -- so a fresh binder never captures a variable the call substitutes
 * (a user's {@code $_nr0} was captured by a static inlining's {@code _nr0}: port-scratch/typer/research/ask2, run A1)
 * and never shadows one in scope; ColumnHygiene's {@code <made>_<N>} likewise per expansion (the integration tests'
 * fromMean, meanOf and offItsMean pin the answers).
 */
class FreshNamesByPlaceTest {

    private static final ModelContext CTX = com.legend.Compiler.compileModel("""
            ###Pure
            Profile t::ft
            {
              stereotypes: [NormalizeRequiredFunction];
            }
            function t::k(a: String[1], b: String[1]): String[1]
            {
              $a + '-' + $b
            }
            function <<t::ft.NormalizeRequiredFunction>> t::inner(s: String[1]): String[1]
            {
              t::k('lit', $s)
            }
            function <<t::ft.NormalizeRequiredFunction>> t::lam(s: String[1]): String[*]
            {
              'a,b'->split(',')->map(y | $y + $s)
            }
            function <<t::ft.NormalizeRequiredFunction>> t::outer(xs: String[*]): String[*]
            {
              $xs->map(e | t::lam('k'))
            }
            ###Relational
            Database storeless::Store ( )
            ###Connection
            RelationalDatabaseConnection storeless::Connection
            { store: storeless::Store; type: DuckDB; specification: DuckDB { }; auth: Test; }
            ###Runtime
            Runtime storeless::Runtime
            { mappings: []; connections: [ storeless::Store: [ session: storeless::Connection ] ]; }
            """);

    private static String sql(String query) {
        String plan = String.valueOf(com.legend.Compiler.query(CTX, query).executionPlan("storeless::Runtime"));
        Matcher m = Pattern.compile("statement=(.*?), slots=", Pattern.DOTALL).matcher(plan);
        assertTrue(m.find(), plan);
        return m.group(1);
    }

    @Test
    @DisplayName("a user's $_nr0 is never captured by a static inlining's fresh name: it plans as the same query with w")
    void aUsersNrNameIsNotCaptured() {
        String nr = sql("|['p', 'q']->map(z | $z->toUpper())->map(_nr0 | t::inner($_nr0))");
        String w = sql("|['p', 'q']->map(z | $z->toUpper())->map(w | t::inner($w))");
        assertFalse(nr.contains("'lit-lit'"), nr);
        assertEquals(w.replaceAll("\\bw\\b", "V").replaceAll("_nr\\d+", "V"), nr.replaceAll("_nr\\d+", "V"));
    }

    @Test
    @DisplayName("an expansion inside another's lambda mints no binder the outer one holds: every SQL lambda its own")
    void nestedExpansionsMintDistinctBinders() {
        // the outer binder (e) and the inner expansion's (y) are both SQL lambdas, one inside the other
        String s = sql("|t::outer('p,q'->split(','))");
        Matcher m = Pattern.compile("(_nr\\d+) ->").matcher(s);
        List<String> binders = new ArrayList<>();
        while (m.find()) {
            binders.add(m.group(1));
        }
        assertEquals(2, binders.size(), s);
        assertEquals(binders.size(), new HashSet<>(binders).size(), s);
    }
}
