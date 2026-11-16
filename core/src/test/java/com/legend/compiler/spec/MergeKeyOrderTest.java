// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

package com.legend.compiler.spec;

import com.legend.compiler.element.ModelContext;
import com.legend.error.TypeInferenceException;
import org.junit.jupiter.api.DisplayName;
import org.junit.jupiter.api.Test;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertThrows;

/**
 * Ruling 29's policy 1 (the owner's ruling 42; the typer slice's stage 1a item 6): WriteChecker's merge refusal listed
 * the rows' columns in a HashSet's bucket order; it lists them in the schema's declared order.
 */
class MergeKeyOrderTest {

    private static final ModelContext CTX = com.legend.Compiler.compileModel("""
            ###Relational
            Database t::Db ( Table T (ID INTEGER PRIMARY KEY, NAME VARCHAR(20), A INTEGER, B INTEGER, F FLOAT) )
            """);

    private static String typing(String query) {
        return assertThrows(TypeInferenceException.class,
                () -> com.legend.Compiler.query(CTX, query).resultType()).getMessage();
    }

    @Test
    @DisplayName("ruling 29's policy 1: a merge key that is no column lists the rows' columns in the schema's order")
    void aMergeKeysRefusalListsTheColumnsInOrder() {
        // [F, ID, NAME] in the HashSet's order until 2026-11-16
        assertEquals("merge: the key 'A' is not a column of the rows merged [NAME, ID, F]",
                typing("#>{t::Db.T}#->select(~[NAME, ID, F])"
                        + "->meta::legend::lite::merge(#>{t::Db.T}#, ~[A])"));
    }
}
