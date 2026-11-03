// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

import java.io.IOException;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.time.ZoneId;
import java.time.zone.ZoneRulesProvider;
import java.util.Map;
import java.util.TreeMap;

/**
 * The Java facts the Rust port must reproduce that {@code //wasm:conformance_jvm} does not already pin
 * (docs/rust-port/ROADMAP.md, phase 0 step 3): the character classes over every code point, and the zone short-ID
 * table with the tz database version. Run it on the build's JDK (remotejdk_25), not whatever {@code java} is on the
 * path: {@code <remotejdk25>/bin/java JavaCompatExtras.java <out-dir>}.
 */
public final class JavaCompatExtras {

    private JavaCompatExtras() {
    }

    public static void main(String[] args) throws IOException {
        Path out = Path.of(args[0]);
        Files.createDirectories(out);
        write(out.resolve("jdk.txt"), "java.version\t" + Runtime.version() + "\ntzdb.version\t"
                + ZoneRulesProvider.getVersions("UTC").lastEntry().getKey() + "\n");
        write(out.resolve("char-classes.tsv"), charClasses());
        write(out.resolve("zone-short-ids.tsv"), shortIds());
    }

    /** The predicates, in column order; a row is a maximal run of code points with the same answers. */
    private static final String[] PREDICATES = {"isLetter", "isDigit", "isLetterOrDigit", "isWhitespace",
        "isSpaceChar", "isUpperCase", "isLowerCase", "isJavaIdentifierStart", "isJavaIdentifierPart"};

    private static String flags(int cp) {
        boolean[] f = {Character.isLetter(cp), Character.isDigit(cp), Character.isLetterOrDigit(cp),
            Character.isWhitespace(cp), Character.isSpaceChar(cp), Character.isUpperCase(cp), Character.isLowerCase(cp),
            Character.isJavaIdentifierStart(cp), Character.isJavaIdentifierPart(cp)};
        StringBuilder b = new StringBuilder(f.length);
        for (boolean x : f) {
            b.append(x ? '1' : '0');
        }
        return b.toString();
    }

    private static String charClasses() {
        StringBuilder b = new StringBuilder("# first\tlast\t").append(String.join(",", PREDICATES)).append('\n');
        int start = 0;
        String run = flags(0);
        for (int cp = 1; cp <= Character.MAX_CODE_POINT + 1; cp++) {
            String f = cp <= Character.MAX_CODE_POINT ? flags(cp) : null;
            if (!run.equals(f)) {
                b.append(String.format("%04X\t%04X\t%s%n", start, cp - 1, run));
                start = cp;
                run = f;
            }
        }
        return b.toString();
    }

    private static String shortIds() {
        StringBuilder b = new StringBuilder("# short id\tzone id it names\n");
        for (Map.Entry<String, String> e : new TreeMap<>(ZoneId.SHORT_IDS).entrySet()) {
            b.append(e.getKey()).append('\t').append(e.getValue()).append('\n');
        }
        return b.toString();
    }

    private static void write(Path p, String s) throws IOException {
        Files.writeString(p, s, StandardCharsets.UTF_8);
    }
}
