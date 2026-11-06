// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

package com.legend.testing;

import com.legend.model.ParsedModel;
import com.legend.parser.Dialect;
import com.legend.door.ModelFromText;
import com.legend.parser.PmcdParser;
import com.legend.parser.SpecParser;
import com.legend.protocol.Protocol;
import com.legend.protocol.spec.ValueSpecification;

import java.util.List;

/**
 * THE PLATFORM test regime, named ONCE — the sibling of {@link Own}
 * (the product regime). A test that parses through this fixture is
 * declaring, visibly at the import, that it exercises the m2/bootstrap
 * SUPERSET: the parser's own grammar suites, the indexer's
 * full-superset parity sweep, the corpus harness, and the quarantined
 * {@code com.legend.platform} tests.
 */
public final class Platform {

    /** The platform regime's level. */
    public static final Dialect SUITE = Dialect.LEGEND_PLATFORM;

    private Platform() {
    }

    public static ParsedModel model(String source) {
        return ModelFromText.parse(source, SUITE);
    }

    public static ValueSpecification spec(String source) {
        return SpecParser.parse(source, SUITE);
    }

    public static List<ValueSpecification> block(String source) {
        return SpecParser.parseCodeBlock(source, SUITE);
    }

    /** The first {@code Mapping} element of {@code source}, its record as the model door reads it at this level --
     *  the protocol pin tests' entry (step 6b, 2026-11-06: through the door, where it parsed at the first
     *  {@code Mapping} token with the Java parser's mapping grammar, deleted). */
    public static Protocol.PMapping mapping(String source) {
        for (Protocol.Element e : PmcdParser.parse(source, SUITE).model().elements()) {
            if (e instanceof Protocol.PMapping m) {
                return m;
            }
        }
        throw new AssertionError("no Mapping element in: " + source);
    }
}
