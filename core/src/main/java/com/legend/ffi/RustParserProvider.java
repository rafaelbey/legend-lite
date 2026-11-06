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
 * column. Found by {@code com.legend.parser.RustParsers.provider()} through this library's META-INF/services.
 */
public final class RustParserProvider implements RustParsers {

    @Override
    public ValueSpecification spec(String text, Dialect dialect) {
        return FrontReader.spec(call("spec", dialect, text));
    }

    @Override
    public LambdaFunction lambda(String text) {
        return FrontReader.lambda(call("lambda", Dialect.LEGEND_ENGINE, text));
    }

    @Override
    public List<ValueSpecification> block(String text, Dialect dialect) {
        return FrontReader.block(call("block", dialect, text));
    }

    @Override
    public ModelAnswer model(String text, Dialect dialect, List<Overlay> overlays) {
        StringBuilder grammars = new StringBuilder("[");
        for (Overlay o : overlays) {
            if (grammars.length() > 1) {
                grammars.append(',');
            }
            grammars.append('[').append(jsonString(o.name())).append(',').append(o.lexable()).append(']');
        }
        String answer = Lite.shared().frontModel(dialect.name(), grammars.append(']').toString(), jsonString(text));
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
        String rest = answer.substring(lineEnd + 1);
        return answer.charAt(0) == 'A'
                ? new ModelAnswer(FrontReader.pmcd(rest), null, List.copyOf(sections))
                : new ModelAnswer(null, raised((Json.Obj) Json.parse(rest)), List.copyOf(sections));
    }

    private static int integer(Json.Node n) {
        return Math.toIntExact(((Json.Num) n).longValue());
    }

    /** One door's answer: the records' codec JSON, or the error thrown. */
    private static String call(String door, Dialect dialect, String text) {
        String answer = Lite.shared().front(door, dialect.name(), jsonString(text));
        if (answer.startsWith("A\n")) {
            return answer.substring(2);
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
            case "java.lang.ArithmeticException" -> new ArithmeticException(message);
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
