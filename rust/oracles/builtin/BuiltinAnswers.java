// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

import java.lang.reflect.Type;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.security.MessageDigest;
import java.util.ArrayList;
import java.util.Collection;
import java.util.HexFormat;
import java.util.List;
import java.util.stream.Collectors;

/**
 * THE PLATFORM'S TABLES, DUMPED (docs/rust-port/PHASE3.md, slice 1b, 2026-11-12): every table of core's
 * {@code com.legend.builtin} and {@code com.legend.platform} -- the native catalog, its overload groups and indexes, the
 * lite vocabulary, the engine dynafunctions and their decisions, the engine's handler surface, the prelude, the native
 * function families, the system metamodel, the platform's declarations and what implements each -- one fact per line,
 * for the Rust port's crates to write the same lines ({@code rust/lite-builtin/tests/builtin_tables.rs},
 * {@code rust/lite-platform/tests/platform_tables.rs}), frozen under {@code rust/oracles/builtin/} before the Java goes.
 *
 * <p>A line is {@code section TAB field...}; a record is the record codec's JSON ({@link Codec}) (its declared type deciding the tags,
 * as the model differential's text does), a large text its SHA-256; a set Java keeps in a hash structure is written
 * sorted, never in its hash order. Each section is its own class's ({@link PureTables}, {@link NativeFnTables},
 * {@link MetamodelTables}, {@link PlatformTables}), in this order.
 *
 * <pre>
 *   java ... rust/oracles/builtin/BuiltinAnswers.java BUILTIN.tsv PLATFORM.tsv
 * </pre>
 */
public final class BuiltinAnswers {

    private BuiltinAnswers() {
    }

    /** The lines of a dump, and the writing helpers every section shares. */
    static final class Out {
        final List<String> lines = new ArrayList<>();
        
        /** One line: the section, then the fields, tab separated (a field holds no tab and no line break). */
        void line(String section, Object... fields) {
            StringBuilder b = new StringBuilder(section);
            for (Object f : fields) {
                String s = String.valueOf(f);
                if (s.indexOf('\t') >= 0 || s.indexOf('\n') >= 0) {
                    throw new IllegalStateException("a field with a tab or a line break: " + s);
                }
                b.append('\t').append(s);
            }
            lines.add(b.toString());
        }

        /** A value in the record codec, its declared type deciding the tags. */
        String json(Object value, Type declared) {
            return Codec.write(value, declared);
        }

        /** A text as a JSON string (escapes as the codec writes them). */
        static String str(String s) {
            StringBuilder b = new StringBuilder();
            Codec.string(b, s);
            return b.toString();
        }

        /** The SHA-256 of a text's UTF-16 units, little-endian, in lower-case hex. */
        static String sha(String s) {
            try {
                return HexFormat.of().formatHex(MessageDigest.getInstance("SHA-256")
                        .digest(s.getBytes(StandardCharsets.UTF_16LE)));
            } catch (java.security.NoSuchAlgorithmException e) {
                throw new IllegalStateException(e);
            }
        }

        /** Values joined by a single space, in the collection's order. */
        static String join(Collection<?> values) {
            return values.stream().map(String::valueOf).collect(Collectors.joining(" "));
        }

        /** Values joined by a single space, sorted (a set Java keeps in a hash structure). */
        static String sorted(Collection<?> values) {
            return values.stream().map(String::valueOf).sorted().collect(Collectors.joining(" "));
        }
    }

    public static void main(String[] args) throws Exception {
        Path builtin = Path.of(args[0]);
        Path platform = Path.of(args[1]);
        Out b = new Out();
        PureTables.dump(b);
        NativeFnTables.dump(b);
        MetamodelTables.dump(b);
        Files.writeString(builtin, String.join("\n", b.lines) + "\n", StandardCharsets.UTF_8);
        Out p = new Out();
        PlatformTables.dump(p);
        Files.writeString(platform, String.join("\n", p.lines) + "\n", StandardCharsets.UTF_8);
        System.out.printf("[builtin-answers] builtin %d lines, platform %d lines%n", b.lines.size(), p.lines.size());
    }
}
