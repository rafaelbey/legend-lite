// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

package com.legend.ffi;

import com.legend.json.Json;
import com.legend.parser.PmcdParser;
import com.legend.protocol.spec.LambdaFunction;
import com.legend.protocol.spec.ValueSpecification;
import java.util.List;

/**
 * The Rust parser's answers read into the protocol records (docs/rust-port/PHASE2.md §3): one reader per door, over
 * the lossless codec's JSON ({@link FrontRecords}, generated). Each door's declared type decides the form: a
 * {@code ValueSpecification} and a block's elements carry their record's tag, a {@code LambdaFunction} and a
 * {@code PmcdParser.Parsed} do not.
 */
public final class FrontReader {

    /** The deepest answer the codec writes for a deep expression; the JSON reader refuses beyond it. */
    private static final Json.Config DEPTH = new Json.Config(1 << 16);

    private FrontReader() {
    }

    /** {@code SpecParser.parse}'s answer. */
    public static ValueSpecification spec(String json) {
        return FrontRecords.readValueSpecification(Json.parse(json, DEPTH));
    }

    /** {@code SpecParser.parseLambda}'s answer. */
    public static LambdaFunction lambda(String json) {
        return FrontRecords.readLambdaFunction(Json.parse(json, DEPTH));
    }

    /** {@code SpecParser.parseCodeBlock}'s answer. */
    public static List<ValueSpecification> block(String json) {
        return FrontRead.list(Json.parse(json, DEPTH), FrontRecords::readValueSpecification);
    }

    /** {@code PmcdParser.parse}'s answer. */
    public static PmcdParser.Parsed pmcd(String json) {
        return FrontRecords.readPmcdParserParsed(Json.parse(json, DEPTH));
    }
}
