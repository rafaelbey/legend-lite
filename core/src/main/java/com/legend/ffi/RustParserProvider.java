// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

package com.legend.ffi;

import com.legend.json.Json;
import com.legend.parser.Dialect;
import com.legend.parser.ParseException;
import com.legend.parser.RustParsers;
import com.legend.protocol.spec.LambdaFunction;
import com.legend.protocol.spec.ValueSpecification;
import java.util.List;

/**
 * The parser's doors over the Rust library ({@code lite_front}; docs/rust-port/PHASE2.md §3): the door, the dialect
 * and the text crossing as a JSON string (every surrogate a {@code \\uXXXX} escape, so a lone one survives the UTF-8
 * C string); the answer {@code A} and the records in the lossless codec, read by {@link FrontReader}, or {@code E} and
 * the error Java's parser raises, rethrown as its class with its message, a {@link ParseException} with its line and
 * column. The engine-JSON doors ({@code lite_front_engine}, step 5) answer legend-engine's JSON itself, which the Rust
 * emitter writes. Found by {@code com.legend.parser.RustParsers.provider()} through this library's META-INF/services.
 */
public final class RustParserProvider implements RustParsers {

    @Override
    public ValueSpecification spec(String text, Dialect dialect) {
        return FrontReader.spec(answered("spec", dialect, text), ANSWER_START);
    }

    @Override
    public LambdaFunction lambda(String text) {
        return FrontReader.lambda(answered("lambda", Dialect.LEGEND_ENGINE, text), ANSWER_START);
    }

    @Override
    public List<ValueSpecification> block(String text, Dialect dialect) {
        return FrontReader.block(answered("block", dialect, text), ANSWER_START);
    }

    @Override
    public @com.legend.base.Nullable String document(String text, boolean sourceInformation, List<Overlay> overlays) {
        return engine("document", sourceInformation, grammars(overlays), text);
    }

    @Override
    public String lambdaJson(String text, boolean sourceInformation) {
        return java.util.Objects.requireNonNull(engine("lambdaJson", sourceInformation, "[]", text),
                "lite_front_engine answered the overlay path at the lambda door");
    }

    /**
     * An engine-JSON door's answer ({@code lite_front_engine}): the JSON -- as UTF-8 text, or, when it holds a lone
     * surrogate, as a JSON string literal read back here -- or the error thrown, or {@code null} for the overlay path.
     */
    private static @com.legend.base.Nullable String engine(String door, boolean sourceInformation, String overlays,
            String text) {
        String answer = Lite.shared().frontEngine(door, sourceInformation, overlays, jsonString(text));
        if (answer.startsWith("A\n")) {
            return answer.substring(2);
        }
        if (answer.startsWith("U\n")) {
            return ((Json.Str) Json.parse(answer.substring(2))).value();
        }
        if (answer.startsWith("E\n")) {
            throw raised((Json.Obj) Json.parse(answer.substring(2)));
        }
        if (answer.equals("O\n")) {
            return null;
        }
        throw new IllegalStateException("lite_front_engine answered neither A, U, E nor O: "
                + answer.substring(0, Math.min(80, answer.length())));
    }

    /** The registered overlay grammars as the Rust library takes them: {@code [["Toy",false],...]}. */
    private static String grammars(List<Overlay> overlays) {
        StringBuilder grammars = new StringBuilder("[");
        for (Overlay o : overlays) {
            if (grammars.length() > 1) {
                grammars.append(',');
            }
            grammars.append('[').append(jsonString(o.name())).append(',').append(o.lexable()).append(']');
        }
        return grammars.append(']').toString();
    }

    @Override
    public ModelAnswer model(String text, Dialect dialect, List<Overlay> overlays) {
        String answer = Lite.shared().frontModel(dialect.name(), grammars(overlays), jsonString(text));
        // A\n<sections>\n<records> or E\n<sections>\n<error>: the sections' JSON is one line
        int lineEnd = answer.indexOf('\n', 2);
        if (!(answer.startsWith("A\n") || answer.startsWith("E\n")) || lineEnd < 0) {
            throw new IllegalStateException("lite_front_model answered neither A nor E: "
                    + answer.substring(0, Math.min(80, answer.length())));
        }
        List<OverlaySection> sections = new java.util.ArrayList<>();
        for (Json.Node s : ((Json.Arr) Json.parse(answer.substring(2, lineEnd))).items()) {
            List<Json.Node> f = ((Json.Arr) s).items();
            sections.add(new OverlaySection(((Json.Str) f.get(0)).value(), integer(f.get(1)), integer(f.get(2)),
                    integer(f.get(3)), integer(f.get(4)), integer(f.get(5))));
        }
        return answer.charAt(0) == 'A'
                ? new ModelAnswer(FrontReader.pmcd(answer, lineEnd + 1), null, List.copyOf(sections))
                : new ModelAnswer(null, raised((Json.Obj) Json.parse(answer.substring(lineEnd + 1))),
                        List.copyOf(sections));
    }

    private static int integer(Json.Node n) {
        return Math.toIntExact(((Json.Num) n).longValue());
    }

    // the parity harness's probes (docs/rust-port/PHASE2.md §4 step 6a): lite_front's doors of the same names
    // (rust/lite-front/src/probes.rs), their answer JSON read here

    @Override
    public List<SectionHeader> sectionHeaders(String text) {
        // the lexer's headers do not depend on the dialect: the door is passed one, and does not read it
        List<SectionHeader> out = new java.util.ArrayList<>();
        for (Json.Node h : ((Json.Arr) Json.parse(call("sectionHeaders", Dialect.LEGEND_ENGINE, text))).items()) {
            List<Json.Node> f = ((Json.Arr) h).items();
            out.add(new SectionHeader(((Json.Str) f.get(0)).value(), integer(f.get(1))));
        }
        return List.copyOf(out);
    }

    @Override
    public int elementSites(String text, Dialect dialect) {
        return Integer.parseInt(call("elementSites", dialect, text));
    }

    @Override
    public List<DatabaseSite> databaseSites(String text, Dialect dialect) {
        List<DatabaseSite> out = new java.util.ArrayList<>();
        for (Json.Node d : ((Json.Arr) Json.parse(call("databaseSites", dialect, text))).items()) {
            List<Json.Node> f = ((Json.Arr) d).items();
            out.add(new DatabaseSite(((Json.Str) f.get(0)).value(), ((Json.Str) f.get(1)).value()));
        }
        return List.copyOf(out);
    }

    @Override
    public List<String> knownSections() {
        // the sections are the parser's, whatever the dialect and the text: the door is passed both, and reads neither
        List<String> out = new java.util.ArrayList<>();
        for (Json.Node s : ((Json.Arr) Json.parse(call("knownSections", Dialect.LEGEND_ENGINE, ""))).items()) {
            out.add(((Json.Str) s).value());
        }
        return List.copyOf(out);
    }

    @Override
    public List<DeclarationExtent> declarationExtents(String text, Dialect dialect) {
        List<DeclarationExtent> out = new java.util.ArrayList<>();
        for (Json.Node d : ((Json.Arr) Json.parse(call("declarationExtents", dialect, text))).items()) {
            List<Json.Node> f = ((Json.Arr) d).items();
            out.add(new DeclarationExtent(((Json.Str) f.get(0)).value(), integer(f.get(1)), integer(f.get(2))));
        }
        return List.copyOf(out);
    }

    /** A probe's answer: its JSON text, or the error thrown. */
    private static String call(String door, Dialect dialect, String text) {
        return answered(door, dialect, text).substring(ANSWER_START);
    }

    /** Where the answer proper starts, past its {@code A\n}. */
    private static final int ANSWER_START = 2;

    /** One door's answer, whole (a records door reads it from {@link #ANSWER_START} where it lies), or the error
     *  thrown. */
    private static String answered(String door, Dialect dialect, String text) {
        String answer = Lite.shared().front(door, dialect.name(), jsonString(text));
        if (answer.startsWith("A\n")) {
            return answer;
        }
        if (answer.startsWith("E\n")) {
            throw raised((Json.Obj) Json.parse(answer.substring(2)));
        }
        throw new IllegalStateException("lite_front answered neither A nor E: "
                + answer.substring(0, Math.min(80, answer.length())));
    }

    /** The error Java's parser raises, as the Rust library names it: its class, with its message. */
    private static RuntimeException raised(Json.Obj e) {
        String cls = ((Json.Str) e.get("class")).value();
        String message = e.get("message") instanceof Json.Str s ? s.value() : null;
        return switch (cls) {
            // the parser writes every ParseException's message: one without is not the parser's answer
            case "com.legend.parser.ParseException" -> new ParseException(
                    java.util.Objects.requireNonNull(message, "the Rust parser raised a ParseException with no message"),
                    (int) ((Json.Num) e.get("line")).longValue(), (int) ((Json.Num) e.get("column")).longValue());
            case "java.lang.IllegalStateException" -> new IllegalStateException(message);
            case "java.lang.IllegalArgumentException" -> new IllegalArgumentException(message);
            case "java.lang.NumberFormatException" -> new NumberFormatException(message);
            case "java.lang.UnsupportedOperationException" -> new UnsupportedOperationException(message);
            case "java.lang.IndexOutOfBoundsException" -> new IndexOutOfBoundsException(message);
            case "java.lang.StringIndexOutOfBoundsException" -> new StringIndexOutOfBoundsException(message);
            // the emitter's (Objects.requireNonNull, its message null in the one-argument form) and the unchecked casts'
            // the parser and the emitter reproduce where legend-engine accepts the text (rust/lite-front/PARITY.md)
            case "java.lang.NullPointerException" -> new NullPointerException(message);
            case "java.lang.ClassCastException" -> new ClassCastException(message);
            case "java.time.DateTimeException" -> new java.time.DateTimeException(message);
            default -> new IllegalStateException("the Rust parser raised a " + cls + ", which the shim has no class"
                    + " for: " + message);
        };
    }

    /** {@code text} as a JSON string literal: {@code "} and {@code \}, the controls and every surrogate escaped. */
    static String jsonString(String text) {
        StringBuilder out = new StringBuilder(text.length() + 16).append('"');
        for (int i = 0; i < text.length(); i++) {
            char c = text.charAt(i);
            if (c == '"' || c == '\\') {
                out.append('\\').append(c);
            } else if (c < 0x20 || Character.isSurrogate(c)) {
                String hex = Integer.toHexString(c);
                out.append("\\u").append("0000", hex.length(), 4).append(hex);
            } else {
                out.append(c);
            }
        }
        return out.append('"').toString();
    }
}
