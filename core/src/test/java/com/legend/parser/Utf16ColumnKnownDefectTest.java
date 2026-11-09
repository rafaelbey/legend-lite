// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

package com.legend.parser;

import com.legend.Compiler;
import com.legend.protocol.SourceInfo;
import com.legend.protocol.spec.CDate;
import com.legend.protocol.spec.PureCollection;
import com.legend.testing.KnownDefect;
import org.junit.jupiter.api.Test;

import java.util.Objects;

import static org.junit.jupiter.api.Assertions.assertEquals;

/** Review #14: spans count UTF-16 units (lexer.rs's start_column) while error columns count code points
 *  (lexer.rs's column_of via cursor.rs's throw_at). The plan (H4) chose UTF-16 everywhere. */
class Utf16ColumnKnownDefectTest {

    /** U+1F600, one code point, two UTF-16 units. */
    private static final String EMOJI = "😀";

    @Test
    @KnownDefect(owner = "W1.2", reason = "an error column after a non-BMP character counts code points while the"
            + " span of the same token counts UTF-16 units")
    void errorColumnAfterNonBmpCharIsTheUtf16Column() {
        PureCollection ok = (PureCollection) Compiler.parseQuery("['" + EMOJI + "', %2024-12-01]");
        SourceInfo span = Objects.requireNonNull(((CDate) ok.values().get(1)).pos());
        if (span.startColumn() != 8) {
            throw new IllegalStateException("precondition: the date token's span starts at UTF-16 column 8, got "
                    + span.startColumn());
        }
        ParseException e;
        try {
            Compiler.parseQuery("['" + EMOJI + "', %2024-13-01]");
            throw new IllegalStateException("precondition: month 13 must refuse at parse (LEGEND_LITE validates)");
        } catch (ParseException refused) {
            e = refused;
        }
        assertEquals(1, e.line());
        assertEquals(span.startColumn(), e.column(),
                "the error column must be the UTF-16 column, as the same token's span is");
    }
}
