// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

package com.legend.protocol;

/**
 * NAMED span-construction origins — the Phase-3 consolidation seam
 * (docs/SPAN_ORIGIN_CONSOLIDATION.md). Every place a span is built by
 * something other than plain token extent ({@code spanOf}) goes through
 * a constructor here, so the mechanism is named at the site instead of
 * appearing as bare column arithmetic. The engine quirks these encode
 * are all probe-pinned by the parity batteries.
 */
public final class SpanOrigin {

    private SpanOrigin() {
    }

    /**
     * The REPARSE-OVERSHOOT family: the engine re-lexes a value snippet
     * with a column offset that leaks into the ANTLR ctx END (auth
     * islands +3; ES / Deephaven connection values +4 — probe matrix
     * 2026-08-14). The span is correct except for the leaked end column.
     */
    public static SourceInfo overshootEnd(SourceInfo s, int cols) {
        return new SourceInfo(s.sourceId(), s.startLine(), s.startColumn(),
                s.endLine(), s.endColumn() + cols);
    }

    /**
     * As {@link #overshootEnd} but the start is re-anchored to another
     * span's start first (the connection-value spans anchor at the first
     * BODY token while the end leaks from the reparse).
     */
    public static SourceInfo anchoredOvershoot(SourceInfo startFrom,
            SourceInfo endFrom, int cols) {
        return new SourceInfo(endFrom.sourceId(), startFrom.startLine(),
                startFrom.startColumn(), endFrom.endLine(),
                endFrom.endColumn() + cols);
    }

    /**
     * THE walker-offset shift for EMBEDDED-island reparses — the
     * engine's composition rule in exactly one place: the line offset
     * applies to every line; the column offset to line 1 only. Identity
     * when both offsets are zero. (Moved from the parser's cursor --
     * consolidation step 2; the Rust parser's twin is cursor.rs's island_shift.)
     */
    /**
     * CONTENT-ANCHORED island node span: starts the line AFTER the
     * island opener at the first content column and ends ONE PAST the
     * closer — the persistence-target/platform walker quirk
     * (DIFF-pinned). (Consolidation step 3.)
     */
    public static SourceInfo contentAnchored(int startLine, int startCol,
            int endLine, int closerEndCol) {
        return new SourceInfo("", startLine, startCol, endLine,
                closerEndCol + 1);
    }

    /**
     * A span INVENTED to match the engine's synthetic-snippet reparse
     * (the legacy service-mapping {@code "$this." + prop} rebuild):
     * {@code width} characters on one line at the anchor.
     * (Consolidation step 4.)
     */
    public static SourceInfo syntheticAt(int line, int col, int width) {
        return new SourceInfo("", line, col, line, col + width - 1);
    }

    public static SourceInfo islandShift(SourceInfo sp, int lineOffset,
            int colOffset) {
        if (lineOffset == 0 && colOffset == 0) {
            return sp;
        }
        return new SourceInfo("",
                sp.startLine() + lineOffset,
                sp.startLine() == 1 ? sp.startColumn() + colOffset
                        : sp.startColumn(),
                sp.endLine() + lineOffset,
                sp.endLine() == 1 ? sp.endColumn() + colOffset
                        : sp.endColumn());
    }
}
