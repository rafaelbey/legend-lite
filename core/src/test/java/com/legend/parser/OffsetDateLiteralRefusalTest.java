// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

package com.legend.parser;

import org.junit.jupiter.api.Test;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertThrows;
import static org.junit.jupiter.api.Assertions.assertTrue;

/** An impossible date with a timezone offset is refused with a position at every dialect (the user's ruling 11 of
 *  2026-11-09, docs/rust-port/PHASE2.md section 6; rust/lite-front/PARITY.md row 3). Until that date the LEGEND_ENGINE
 *  dialect's unvalidated date crashed in the offset's shift with a raw java.time.DateTimeException, the known defect
 *  of stage reading A03 (defect 8). */
class OffsetDateLiteralRefusalTest {

    @Test
    void anImpossibleDateWithAnOffsetIsRefusedAtTheLiteralAtEveryDialect() {
        for (Dialect dialect : Dialect.values()) {
            ParseException refused = assertThrows(ParseException.class,
                    () -> SpecParser.parse("%2024-02-30T10:00+0500", dialect));
            assertEquals(1, refused.line(), "a refusal must be positioned");
            assertEquals(1, refused.column(), "a refusal must be positioned at the literal");
            assertTrue(refused.getMessage().contains("invalid date literal"), refused.getMessage());
        }
    }
}
