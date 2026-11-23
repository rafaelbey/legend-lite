// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

package com.legend.compiler.element;

/**
 * The grammar level a graph was compiled at -- the parser's dialect of its sources, carried
 * into the compiled context (the typer slice's stage 1c, 2026-11-23; register row S1026). It names WHICH upstream
 * compiler's rules lite follows where the two differ: legend-engine has two. Its model compiler (the protocol's, a
 * user's model) refuses a model function named as a platform special form; legend-pure compiles the engine's own Pure
 * (relational.pure's {@code meta::relational::metamodel::filter} and {@code join} among it) by type, with no such
 * refusal.
 *
 * <ul>
 *   <li>{@link #LEGEND_PLATFORM} -- legend-pure's: the platform's own sources (the boot layer, the corpus's m2
 *       sources, the spec's platform files);</li>
 *   <li>{@link #LEGEND_ENGINE} -- the engine's model compiler;</li>
 *   <li>{@link #LEGEND_LITE} -- lite's door, which follows the engine's model compiler.</li>
 * </ul>
 *
 * <p>Read from the compile's own parse level (the compile entry that parsed the text names it), never from the text.
 */
public enum GrammarLevel {
    LEGEND_PLATFORM,
    LEGEND_LITE,
    LEGEND_ENGINE;

    /** Whether the engine's model compiler's rules hold here (LEGEND_ENGINE and LEGEND_LITE), not legend-pure's. */
    public boolean followsTheModelCompiler() {
        return this != LEGEND_PLATFORM;
    }
}
