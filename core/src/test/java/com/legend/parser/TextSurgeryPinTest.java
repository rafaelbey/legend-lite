package com.legend.parser;

import org.junit.jupiter.api.Test;

import java.util.List;

import static org.junit.jupiter.api.Assertions.assertEquals;

/**
 * Pins for the text-surgery audit §1.1 wrong-answer fixes — each test IS the
 * audit's counterexample, asserted on the fixed behavior. (#2, a data cell's
 * colon that is not a path separator, is the parser's, and the parser is
 * Rust's since 2026-11-06: its counterexamples are pinned in
 * rust/lite-front/src/element_parser.rs.)
 */
class TextSurgeryPinTest {

    @Test
    void witnessPrefixInvertsExactlyForClassNamesContainingDoubleUnderscore() {
        // §1.1 #1: my::A__B and my::A::B mint the same prefix; the string
        // demangle returned my::A::B (simple name "B") for a class actually
        // named A__B. The inverse is a MODEL lookup by re-mangling.
        String pfx = com.legend.model.ClassMapping.subTypeColumnPrefix("my::A__B");
        assertEquals("my::A__B", com.legend.model.ClassMapping
                .classOfWitnessPrefix(pfx, List.of("other::C", "my::A__B")));
        assertEquals(null, com.legend.model.ClassMapping
                .classOfWitnessPrefix(pfx, List.of("other::C")));
    }
}
