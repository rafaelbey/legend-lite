// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

package com.legend.testing;

import com.legend.parser.Dialect;
import com.legend.door.ModelFromText;
import com.legend.parser.PmcdParser;
import com.legend.protocol.Protocol;

/**
 * THE ENGINE-WIRE test regime, named ONCE — the protocol emission pin
 * tests byte-compare our wire output against {@code PureGrammarParser}'s,
 * so their parses run at the drop-in level.
 */
public final class Engine {

    /** The engine-wire regime's level. */
    public static final Dialect SUITE = Dialect.LEGEND_ENGINE;

    private Engine() {
    }

    /** A whole-model parse at the engine level (strict-surface pins). */
    public static com.legend.model.ParsedModel model(String source) {
        return ModelFromText.parse(source, SUITE);
    }

    /** The first element of {@code source}, its record as the model door reads it at the engine level (step 6b,
     *  2026-11-06: where the Java parser's cursor read it at the first token, deleted). */
    public static Protocol.Element element(String source) {
        return PmcdParser.parse(source, SUITE).model().elements().get(0);
    }
}
