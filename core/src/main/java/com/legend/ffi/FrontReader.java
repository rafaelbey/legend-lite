// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

package com.legend.ffi;

import com.legend.parser.PmcdParser;
import com.legend.protocol.spec.LambdaFunction;
import com.legend.protocol.spec.ValueSpecification;
import java.util.List;

/**
 * The Rust parser's answers read into the protocol records (docs/rust-port/PHASE2.md §3): one reader per door, over
 * the lossless codec's JSON ({@link FrontRecords}, generated), pulled through a cursor in the order the writer wrote
 * it, so no tree of the answer is built beside the records. Each door's declared type decides the form: a
 * {@code ValueSpecification} and a block's elements carry their record's tag, a {@code LambdaFunction} and a
 * {@code PmcdParser.Parsed} do not. The whole answer is read, from {@code start} of the string that holds it: text after
 * it is refused.
 */
public final class FrontReader {

    private FrontReader() {
    }

    /** {@code SpecParser.parse}'s answer. */
    public static ValueSpecification spec(String answer, int start) {
        FrontRead.Cursor r = new FrontRead.Cursor(answer, start);
        ValueSpecification v = FrontRecords.readValueSpecification(r);
        r.end();
        return v;
    }

    /** {@code SpecParser.parseLambda}'s answer. */
    public static LambdaFunction lambda(String answer, int start) {
        FrontRead.Cursor r = new FrontRead.Cursor(answer, start);
        LambdaFunction v = FrontRecords.readLambdaFunction(r, false);
        r.end();
        return v;
    }

    /** {@code SpecParser.parseCodeBlock}'s answer. */
    public static List<ValueSpecification> block(String answer, int start) {
        FrontRead.Cursor r = new FrontRead.Cursor(answer, start);
        List<ValueSpecification> v = FrontRead.list(r, FrontRecords::readValueSpecification);
        r.end();
        return v;
    }

    /** {@code PmcdParser.parse}'s answer. */
    public static PmcdParser.Parsed pmcd(String answer, int start) {
        FrontRead.Cursor r = new FrontRead.Cursor(answer, start);
        PmcdParser.Parsed v = FrontRecords.readPmcdParserParsed(r, false);
        r.end();
        return v;
    }
}
