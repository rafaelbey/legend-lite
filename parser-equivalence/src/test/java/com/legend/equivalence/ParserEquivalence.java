package com.legend.equivalence;

import com.fasterxml.jackson.databind.JsonNode;
import com.fasterxml.jackson.databind.ObjectMapper;
import org.finos.legend.engine.protocol.pure.m3.PackageableElement;
import org.finos.legend.engine.protocol.pure.v1.model.context.PureModelContextData;
import org.finos.legend.engine.shared.core.ObjectMapperFactory;

import java.util.ArrayList;
import java.util.List;

/**
 * The ELEMENT-LEVEL diagnostic of the equivalence harness — POSITIONAL
 * byte comparison of the referee's element list against the elements of
 * legend-lite's production document parser (the model door's records at
 * the engine's dialect, {@code PmcdParser.parseModel}, each emitted as
 * the engine's element JSON: the parse {@code parseDocument} runs).
 *
 * <p>DEMOTED from a gate to a diagnostic (HARNESS_SIMPLIFICATION_PLAN
 * Phase 3): the document byte gate implies element byte equality — the
 * element universe is a strict subset of the document's, proven
 * empirically by the Phase-2 joint property (zero sources where the
 * document passes and elements fail, full corpus). What survives here is
 * the LOCALISER: on a document failure, the positional rows name the
 * element index and the first divergent JSON path.
 *
 * <p>Positional, not FQN-paired: positional comparison is length- and
 * order-sensitive, so truncation, duplication and reordering — the
 * escapes FQN pairing is structurally blind to — read as DIFF rows.
 *
 * <h2>Why bytes and not object graphs</h2>
 * Upstream's {@code CString.multiLine} is excluded from {@code equals()}
 * and present in JSON when true. An object-graph comparator would
 * silently miss it. Every verdict is byte equality on JSON produced by
 * {@code getNewStandardObjectMapperWithPureProtocolExtensionSupports()}
 * — the mapper the HTTP endpoint uses.
 */
public final class ParserEquivalence {

    /** What happened to one element (or, for the *_FAIL/*_REJECTED kinds,
     *  to the whole source). */
    public enum Kind {
        /** Byte-identical at the same position. */
        MATCH,
        /** Both produced bytes; they differ (or one side's list is
         * shorter). The only outcome that is a bug. */
        DIFF,
        /** Our parser could not read the source. */
        PARSE_FAIL,
        /** The reference parser threw on the WHOLE source — one verdict
         * per file, so the skipped bucket is counted, not invisible. */
        REFERENCE_REJECTED
    }

    public record Verdict(Kind kind, String sourceId, String element, String detail) {
    }

    private final ObjectMapper mapper =
            ObjectMapperFactory.getNewStandardObjectMapperWithPureProtocolExtensionSupports();

    /**
     * Compare one source POSITIONALLY: the referee's i-th element against
     * lite's i-th element, byte for byte.
     */
    public List<Verdict> compare(Corpus.Source src) {
        List<Verdict> out = new ArrayList<>();
        List<String> ref;
        try {
            ref = referenceElements(src);
        } catch (Throwable t) {
            // ONE named verdict per rejected file — silence is not evidence
            out.add(new Verdict(Kind.REFERENCE_REJECTED, src.id(), "-", root(t)));
            return out;
        }
        List<String> ours = new ArrayList<>();
        try {
            // lite's elements in document order -- the records, less the section index the engine appends -- since
            // 2026-11-06 the model door's (the Rust parser's: docs/rust-port/PHASE2.md §4 step 6a), where they were
            // the Java parser's own section walk, which goes with it
            for (com.legend.protocol.Protocol.Element e
                    : com.legend.parser.PmcdParser.parseModel(src.text()).elements()) {
                if (!(e instanceof com.legend.protocol.Protocol.PSectionIndex)) {
                    ours.add(com.legend.protocol.ProtocolEmitter.emitElement(e));
                }
            }
        } catch (Throwable t) {
            out.add(new Verdict(Kind.PARSE_FAIL, src.id(), "-", root(t)));
            return out;
        }
        int n = Math.max(ref.size(), ours.size());
        for (int i = 0; i < n; i++) {
            if (i >= ref.size()) {
                out.add(new Verdict(Kind.DIFF, src.id(), "#" + i,
                        "we emitted an element the reference did not: "
                                + head(ours.get(i))));
            } else if (i >= ours.size()) {
                out.add(new Verdict(Kind.DIFF, src.id(), "#" + i,
                        "reference element we never emitted: "
                                + head(ref.get(i))));
            } else {
                out.add(Comparators.sameBytes(ref.get(i), ours.get(i))
                        ? new Verdict(Kind.MATCH, src.id(), "#" + i, "")
                        : new Verdict(Kind.DIFF, src.id(), "#" + i,
                                firstDivergence(ref.get(i), ours.get(i))));
            }
        }
        return out;
    }

    /** The referee's element JSONs in PMCD order, SectionIndex skipped
     *  (lite appends its SectionIndex at document level, not per
     *  section). Shared with the whole-document sweep — ONE referee parse
     *  per source. */
    private List<String> referenceElements(Corpus.Source src) throws Exception {
        // the DIAGNOSTIC path parses the referee locally — it runs on
        // demand (a document failure, ZCompositionPin), never in the
        // sweep's hot loop
        PureModelContextData pmcd =
                org.finos.legend.engine.language.pure.grammar.from
                        .PureGrammarParser.newInstance()
                        .parseModel(src.text());
        List<String> out = new ArrayList<>();
        for (PackageableElement e : pmcd.getElements()) {
            if (e instanceof org.finos.legend.engine.protocol.pure.v1.model
                    .packageableElement.section.SectionIndex) {
                continue;
            }
            out.add(mapper.writeValueAsString(e));
        }
        return out;
    }

    /** The {@code _type}/path head of an element JSON, for one-line rows. */
    private static String head(String json) {
        return json.length() > 90 ? json.substring(0, 90) + "…" : json;
    }

    /** A diff message that names the exact JSON path, not just "differs". */
    private String firstDivergence(String expected, String actual) {
        try {
            JsonNode a = mapper.readTree(expected);
            JsonNode b = mapper.readTree(actual);
            List<String> paths = new ArrayList<>();
            walk("$", a, b, paths);
            if (!paths.isEmpty()) {
                return paths.get(0);
            }
        } catch (Exception ignored) {
            // fall through to a character offset
        }
        int n = Math.min(expected.length(), actual.length());
        int i = 0;
        while (i < n && expected.charAt(i) == actual.charAt(i)) {
            i++;
        }
        return "byte " + i + " | expected …" + snippet(expected, i) + " | actual …" + snippet(actual, i);
    }

    private static String snippet(String s, int i) {
        return s.substring(Math.max(0, i - 20), Math.min(s.length(), i + 40));
    }

    private static void walk(String path, JsonNode a, JsonNode b, List<String> out) {
        if (!out.isEmpty()) {
            return;
        }
        if (a == null || b == null || !a.getNodeType().equals(b.getNodeType())) {
            out.add(path + ": expected=" + a + " actual=" + b);
            return;
        }
        if (a.isObject()) {
            java.util.Set<String> keys = new java.util.TreeSet<>();
            a.fieldNames().forEachRemaining(keys::add);
            b.fieldNames().forEachRemaining(keys::add);
            for (String k : keys) {
                walk(path + "." + k, a.get(k), b.get(k), out);
            }
        } else if (a.isArray()) {
            if (a.size() != b.size()) {
                out.add(path + ": size expected=" + a.size() + " actual=" + b.size());
                return;
            }
            for (int i = 0; i < a.size(); i++) {
                walk(path + "[" + i + "]", a.get(i), b.get(i), out);
            }
        } else if (!a.equals(b)) {
            out.add(path + ": expected=" + a + " actual=" + b);
        }
    }

    private static String root(Throwable t) {
        Throwable r = t;
        while (r.getCause() != null) {
            r = r.getCause();
        }
        return String.valueOf(r.getMessage()).replaceAll("\\s+", " ").trim();
    }
}
