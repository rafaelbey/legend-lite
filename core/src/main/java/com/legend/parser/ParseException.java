package com.legend.parser;

import java.util.List;

/**
 * Thrown by the parser's doors ({@link PmcdParser}, {@link SpecParser}) when
 * a text doesn't match the expected Pure grammar: the Rust library's error,
 * rethrown as this class with its line and column (Java's parser, which the
 * library ports, threw it until 2026-11-06).
 *
 * <p>Carries source line/column when known; both default to {@code 0}
 * for errors that occur outside any token (e.g. unexpected EOF on an
 * empty stream). Unchecked &mdash; parser callers don't typically catch
 * these except at the top of a pipeline.
 *
 * <p>A model text's other errors ride on the first ({@link #further()}; the user's ruling 23 of 2026-11-10, built
 * 2026-11-11, docs/rust-port/PHASE2.md section 6): the parser reads on past a refused element, at element and section
 * boundaries only, and lists what it finds after this error, sorted by place. This exception, its message and its place
 * are the parser's answer as they always were; the surfaces lite shapes itself (the editor's diagnostics, {@code
 * Compiler.compileErrors}) show the rest.
 */
public final class ParseException extends com.legend.error.LegendCompileException {

    private static final long serialVersionUID = 1L;

    private final int line;
    private final int column;
    private final List<ParseException> further;

    public ParseException(String message) {
        super(Phase.PARSE, message);
        this.line = 0;
        this.column = 0;
        this.further = List.of();
    }

    public ParseException(String message, int line, int column) {
        this(message, line, column, List.of());
    }

    /** A placed error and the errors its text holds after it, sorted by place (ruling 23). */
    public ParseException(String message, int line, int column, List<ParseException> further) {
        super(Phase.PARSE, formatMessage(message, line, column));
        this.line = line;
        this.column = column;
        this.further = List.copyOf(further);
    }

    public int line() {
        return line;
    }

    public int column() {
        return column;
    }

    /** The errors the text holds after this one, sorted by place; empty when there is none or none could be read. */
    public List<ParseException> further() {
        return further;
    }

    private static String formatMessage(String message, int line, int column) {
        if (line <= 0) return message;
        return "[" + line + ":" + column + "] " + message;
    }
}
