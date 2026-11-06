// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

package com.legend.equivalence;

import com.fasterxml.jackson.databind.ObjectMapper;
import org.finos.legend.engine.language.pure.grammar.from.PureGrammarParser;
import org.finos.legend.engine.shared.core.ObjectMapperFactory;
import org.junit.jupiter.api.Test;

import java.util.LinkedHashMap;
import java.util.Map;

import static org.junit.jupiter.api.Assertions.assertEquals;

/**
 * FIXTURE-LEVEL REFEREES for span/offset COMPOSITION
 * (HARNESS_SIMPLIFICATION_PLAN "not in scope" note, adopted): the audit
 * proved one island-offset mutation survives the ENTIRE corpus because
 * no corpus file reaches the branch — more corpus will never find that
 * fault class. These are hand-built inputs chosen to stress island
 * nesting, mid-line island opens, and walker-offset composition, each
 * adjudicated LIVE against the engine byte-for-byte (the
 * ViewFilterParityTest shape).
 *
 * <p>MUTATION-SWEPT 2026-08-12: all six arithmetic/conditional terms of
 * the Java parser's island shift (then {@code shiftIsland}, later
 * {@code SpanOrigin.islandShift}) were mutated one at a time
 * (offset drops + condition flips) and this test went RED on every one
 * — including M04 (the end-column offset drop), the mutation that
 * previously survived the ENTIRE corpus. The single-line-island
 * fixtures are what reach that branch; do not "simplify" them onto
 * multiple lines. The document door is the Rust parser's and emitter's
 * since 2026-11-06 (docs/rust-port/PHASE2.md §4 step 5): the shift is
 * {@code island_shift} in rust/lite-front/src/cursor.rs.
 */
class OffsetCompositionParityTest {

    private static final Map<String, String> FIXTURES = new LinkedHashMap<>();

    static {
        // island opening MID-LINE, content sharing the opener's line
        FIXTURES.put("midline-island", """
                ###Pure
                Class a::A { x: String[1]; }
                ###Mapping
                Mapping a::M
                (
                  a::A: Pure { ~src a::A x: $src.x }
                )
                """);
        // EMBEDDED runtime island inside a service, connection island
        // NESTED inside it — offsets compose across two island layers
        FIXTURES.put("nested-islands", """
                ###Service
                Service a::S
                {
                  pattern: '/x';
                  documentation: 'd';
                  execution: Single
                  {
                    query: |1;
                    mapping: a::M;
                    runtime:
                    #{
                      connections:
                      [
                        ModelStore:
                        [
                          id1:
                          #{
                            JsonModelConnection
                            {
                              class: a::A;
                              url: 'u';
                            }
                          }#
                        ]
                      ];
                    }#;
                  }
                }
                """);
        // island content whose LAST line is the '}#' line — the
        // end-column path where an offset can silently drop
        FIXTURES.put("island-end-on-close", """
                ###Persistence
                PersistenceContext a::PC
                {
                  persistence: a::P;
                  platform: Default;
                }
                Persistence a::P
                {
                  doc: 'd';
                  trigger: Manual;
                  service: a::S;
                  persister: Streaming { sink: Relational { database: a::DB; } }
                }
                """);
        // section content beginning on the SAME line count as a
        // multi-section file with blank-line skew before headers
        FIXTURES.put("blankline-skew", """


                ###Relational
                Database a::DB ( Table T ( ID INTEGER PRIMARY KEY ) )


                ###Pure

                Class a::A { x: String[1]; }
                """);
        // M04 KILLERS (HONEST_DEBT #3): the island shift's END-COLUMN arm
        // fires only when a span ENDS on line 1 of a re-lexed island
        // slice with a nonzero column offset — i.e. SINGLE-LINE islands,
        // which no corpus file contains. The inner connection island
        // here sits entirely on one line at a deep column: every span
        // inside it has endLine==1, so a dropped end-column offset is a
        // byte diff against the referee.
        FIXTURES.put("single-line-inner-island", """
                ###Service
                Service a::S
                {
                  pattern: '/x';
                  documentation: 'd';
                  execution: Single
                  {
                    query: |1;
                    mapping: a::M;
                    runtime:
                    #{
                      connections:
                      [
                        ModelStore:
                        [
                          id1: #{ JsonModelConnection { class: a::A; url: 'u'; } }#
                        ]
                      ];
                    }#;
                  }
                }
                """);
        // BOTH island layers on one line — line-1 column offsets COMPOSE
        // across the two re-lex layers (the walker rule: column shift on
        // the slice's first line only), start AND end columns both ride
        FIXTURES.put("single-line-both-islands", """
                ###Service
                Service a::S2
                {
                  pattern: '/y';
                  documentation: 'd';
                  execution: Single
                  {
                    query: |1;
                    mapping: a::M;
                    runtime: #{ connections: [ ModelStore: [ id1: #{ JsonModelConnection { class: a::A; url: 'u'; } }# ] ]; }#;
                  }
                }
                """);
        // enumeration-mapping island values at high columns
        FIXTURES.put("enum-mapping-columns", """
                ###Pure
                Enum a::E { X, Y }
                ###Mapping
                Mapping a::M
                (
                  a::E: EnumerationMapping em { X: ['x1', 'x2'], Y: ['y'] }
                )
                """);
    }

    @Test
    void handBuiltOffsetFixturesByteMatchTheLiveReferee() throws Exception {
        ObjectMapper mapper = ObjectMapperFactory
                .getNewStandardObjectMapperWithPureProtocolExtensionSupports();
        PureGrammarParser referee = PureGrammarParser.newInstance();
        for (Map.Entry<String, String> f : FIXTURES.entrySet()) {
            String expected;
            try {
                expected = mapper.writeValueAsString(
                        referee.parseModel(f.getValue()));
            } catch (Throwable t) {
                throw new AssertionError("fixture '" + f.getKey()
                        + "' must be ORACLE-ACCEPTED (fix the fixture): "
                        + t.getMessage(), t);
            }
            String actual = com.legend.parser.PmcdParser
                    .parseDocument(f.getValue());
            assertEquals(expected, actual,
                    "offset-composition fixture '" + f.getKey() + "'");
        }
    }
}
