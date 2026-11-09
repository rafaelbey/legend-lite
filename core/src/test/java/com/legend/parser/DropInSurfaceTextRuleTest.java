package com.legend.parser;

import com.legend.testing.SourceFiles;
import org.junit.jupiter.api.Test;
import org.junit.jupiter.api.Tag;

import java.io.IOException;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.List;
import java.util.Map;
import java.util.TreeMap;
import java.util.regex.Pattern;
import java.util.stream.Stream;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertTrue;

/**
 * THE TEXT RULE over the drop-in surface {@code {parser, protocol}}
 * (text-surgery audit 2026-08 + user mandate 2026-08-07; the lexer left the
 * surface 2026-11-06 with the Java lexer and parser, the Rust port's phase 2
 * step 6b -- both are the Rust library's): a regex inside a
 * hand-written recursive-descent parser is a second, undocumented grammar
 * running beside the first — with its own idea of what an identifier is and
 * no token positions. Byte parity is a property of the token parser; every
 * regex is surface that property does not cover.
 *
 * <p>Two rules, all ratcheted DOWN only:
 * <ol>
 *   <li>{@code com.legend.parser} + {@code com.legend.protocol}: regex-family
 *       calls confined to the NAMED whitelist below, frozen at the 2026-08-07
 *       census; the target is an empty whitelist (then rule 1 covers all
 *       three packages).</li>
 *   <li>{@code com.legend.parser}: {@code tokens.source()} reads (re-reading
 *       chars the lexer already tokenized) confined to their own whitelist —
 *       the assertion-island reparse emulation legitimately needs raw chars
 *       because it reproduces the engine's raw-text mechanism
 *       (HelperTestAssertionGrammarParser); the audit §5.5 re-parse sites do
 *       not, and burn.</li>
 * </ol>
 *
 * <p>Lexeme extraction ({@code substring}, {@code charAt}, the escape
 * decoder) is the parser's JOB, not surgery, and is deliberately not policed
 * here — the enforceable line is the regex family plus source re-reads.
 */
@Tag("guardrail")
class DropInSurfaceTextRuleTest {

    /** The regex family: each match is one shadow-grammar call site. */
    private static final Pattern TEXT_SURGERY = Pattern.compile(
            "Pattern\\.compile|Pattern\\.matches|Pattern\\.quote"
                    + "|\\.matches\\(|\\.replaceAll\\(|\\.replaceFirst\\(|\\.split\\(");

    private static final Pattern SOURCE_REREAD = Pattern.compile(
            "tokens\\.source\\(\\)|tokens\\(\\)\\.source\\(\\)");

    /** 2026-08-07 census — file -> allowed regex-family call sites. DOWN only;
     *  a new site in a clean file or a count above the frozen number fails. */
    // EMPTY 2026-08-07: the whole drop-in surface {lexer, parser, protocol}
    // is at ZERO regex-family call sites — the rule is now unconditional.
    private static final Map<String, Integer> REGEX_WHITELIST = Map.of();

    /** 2026-08-07 census — parser files allowed to read raw source chars.
     *  PERMANENT, not debt (adjudicated per audit §5.5 after the depth-aware
     *  island-scan fix): every remaining site is either engine-mechanism
     *  emulation (the engine ALSO extracts island text and reparses raw
     *  chars — assertion-island spans, ExternalFormat/CSV island bodies) or
     *  coarse-chunk necessity (the island lexer emits content as chunks;
     *  names live INSIDE chunk strings, so graph-fetch span scanning must
     *  walk chars between token-true bounds). What burned was the DEFECT:
     *  flat island scans that a nested #{ }# truncated. New sites still
     *  need a whitelist edit — and this comment's justification. */
    // EMPTY 2026-11-06 (the Rust port's phase 2 step 6b): the four files that read raw source chars --
    // ElementParser (3), TokenStreamCursor (1), MappingProtocolParser (2) and SpecParser's body (1, the GQL island's
    // raw slice) -- are deleted with the Java parser (each one's reason is in the history of this file); the doors
    // left in this package read no token stream. A new site still needs a whitelist edit, and this comment's
    // justification.
    private static final Map<String, Integer> SOURCE_WHITELIST = Map.of();

    @Test
    void regexFamilyIsConfinedToTheFrozenWhitelist() throws IOException {
        Map<String, Integer> found = census(TEXT_SURGERY,
                "parser", "protocol");
        for (Map.Entry<String, Integer> e : found.entrySet()) {
            assertTrue(e.getKey().startsWith("parser/")
                            || e.getKey().startsWith("protocol/"),
                    "REGEX IN THE LEXER — the one package that was clean: "
                            + e.getKey() + " (" + e.getValue() + " sites)");
            int allowed = REGEX_WHITELIST.getOrDefault(e.getKey(), 0);
            assertTrue(e.getValue() <= allowed,
                    "regex-family sites GREW in " + e.getKey() + ": "
                            + e.getValue() + " > frozen " + allowed
                            + " — the drop-in surface does not grow shadow"
                            + " grammars; parse from tokens instead");
        }
    }

    @Test
    void sourceRereadsAreConfinedToTheFrozenWhitelist() throws IOException {
        Map<String, Integer> found = census(SOURCE_REREAD, "parser");
        for (Map.Entry<String, Integer> e : found.entrySet()) {
            int allowed = SOURCE_WHITELIST.getOrDefault(e.getKey(), 0);
            assertTrue(e.getValue() <= allowed,
                    "raw source() re-reads GREW in " + e.getKey() + ": "
                            + e.getValue() + " > frozen " + allowed
                            + " — the lexer already tokenized that region");
        }
    }

    @Test
    void whitelistsShrinkWithTheCode() throws IOException {
        // the ratchet's other edge: a burned site must LEAVE the whitelist,
        // so the frozen numbers cannot quietly become slack for new sites
        Map<String, Integer> regex = census(TEXT_SURGERY,
                "parser", "protocol");
        for (Map.Entry<String, Integer> e : REGEX_WHITELIST.entrySet()) {
            assertEquals(e.getValue(), regex.getOrDefault(e.getKey(), 0),
                    "whitelist stale for " + e.getKey()
                            + " — sites were burned; lower the frozen count");
        }
        Map<String, Integer> src = census(SOURCE_REREAD, "parser");
        for (Map.Entry<String, Integer> e : SOURCE_WHITELIST.entrySet()) {
            assertEquals(e.getValue(), src.getOrDefault(e.getKey(), 0),
                    "source-reread whitelist stale for " + e.getKey()
                            + " — sites were burned; lower the frozen count");
        }
    }

    private static Map<String, Integer> census(Pattern needle, String... pkgs)
            throws IOException {
        String root = "core/src/main/java/com/legend";
        assertTrue(!SourceFiles.under(root).isEmpty(), "core's sources are not declared: " + root
                + " (Bazel: the test target's source list, -D" + SourceFiles.PROPERTY + ")");
        Map<String, Integer> out = new TreeMap<>();
        for (String pkg : pkgs) {
            try (Stream<Path> files = SourceFiles.under(root + "/" + pkg).stream()) {
                for (Path f : files.filter(p -> p.toString().endsWith(".java")).toList()) {
                    int n = 0;
                    for (String line : List.copyOf(Files.readAllLines(f))) {
                        String code = line.strip();
                        if (code.startsWith("//") || code.startsWith("*")
                                || code.startsWith("/*")) {
                            continue;               // prose is not a call site
                        }
                        var m = needle.matcher(line);
                        while (m.find()) {
                            n++;
                        }
                    }
                    if (n > 0) {
                        out.put(pkg + "/" + f.getFileName(), n);
                    }
                }
            }
        }
        return out;
    }
}
