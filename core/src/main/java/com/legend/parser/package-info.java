/**
 * Phases A, B, C &mdash; text &rarr; protocol records, through the doors.
 *
 * <p>The lexer and the parser are the Rust library's ({@code rust/lite-front}): the doors here hand it a text and a
 * {@link com.legend.parser.Dialect}, and read back its records or its error ({@link com.legend.parser.RustParsers},
 * found through the SPI seam; docs/rust-port/PHASE2.md). The Java lexer and parser it ports were deleted on
 * 2026-11-06 (step 6b).
 * <ul>
 *   <li>{@link com.legend.parser.PmcdParser} (A, B): a text &rarr; its
 *       {@code Protocol.PureModelContextData} (the engine's section walk, every
 *       section through its grammar; an overlay's through
 *       {@link com.legend.parser.SectionGrammarRegistry}, called back here).</li>
 *   <li>{@code SpecParser} (A, C): a text &rarr;
 *       {@code protocol.spec.ValueSpecification}.</li>
 * </ul>
 *
 * <h2>Public surface</h2>
 * <pre>
 *   Protocol.PureModelContextData records = PmcdParser.parse(pureSource, Dialect.LEGEND_LITE).model();
 *   // three dialect levels — see {@link com.legend.parser.Dialect}
 * </pre>
 * The compiler's model is not the parser's: a text becomes one through the
 * text's door, {@code com.legend.door.ModelFromText}, which hands the records to
 * {@code ModelFromProtocol} (docs/PROTOCOL_PROGRAM_2026_10_05.md, leg 8's last step).
 *
 * <h2>Records</h2>
 * Parsed records live in {@link com.legend.protocol} (declarations)
 * and {@code com.legend.protocol.spec} (value specifications).
 *
 * <h2>Dependencies</h2>
 * The protocol records, values, the shared error vocabulary, the overlay seam
 * ({@code spi}) and the JDK (ArchitectureTest, invariant 7c).
 */
package com.legend.parser;
