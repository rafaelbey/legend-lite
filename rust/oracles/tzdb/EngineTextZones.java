// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

import com.legend.sql.SqlExpr;
import com.legend.sql.SqlSelect;
import com.legend.sql.SqlSource;
import com.legend.sql.dialect.EngineStyleH2;
import java.io.IOException;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.time.Instant;
import java.time.LocalDateTime;
import java.time.ZoneId;
import java.time.ZoneOffset;
import java.time.zone.ZoneOffsetTransition;
import java.time.zone.ZoneRules;
import java.util.ArrayList;
import java.util.LinkedHashSet;
import java.util.List;
import java.util.Set;

/**
 * The engine text's date-time constant shifted to a connection's zone, as Java's own {@code EngineStyleH2(false,
 * zone)} renders it (docs/rust-port/PHASE1.md §4 step 4c; rust/oracles/tzdb/MANIFEST.tsv): for each sample zone and
 * constant, {@code select TIMESTAMP'...' as "t"}. The constants are instants around the zone's transitions (written as
 * the engine text's ISO date-times, with and without a fraction) and the edge cases of timestampLit's reading. core's
 * dialect target is private to core, so this runs on its built jars (the build's JDK, remotejdk_25):
 * <pre>
 *   bazel build //core:sql_dialect
 *   $(bazel info output_base)/external/rules_java++toolchains+remotejdk25_&lt;os&gt;_&lt;arch&gt;/bin/java \
 *     -cp bazel-bin/core/libsql_dialect.jar:bazel-bin/core/libsql.jar:bazel-bin/base/libbase.jar \
 *     rust/oracles/tzdb/EngineTextZones.java rust/oracles/tzdb/engine-text-zones.tsv
 * </pre>
 */
public final class EngineTextZones {

    private EngineTextZones() {
    }

    /** The zones: transitions of every shape (half-hour, negative, a skipped day), short IDs, offsets, unknowns. */
    static final String[] ZONES = {"America/New_York", "HST", "Europe/London", "Asia/Kolkata", "Australia/Lord_Howe",
        "Pacific/Chatham", "Europe/Dublin", "Africa/Casablanca", "America/Sao_Paulo", "Asia/Tehran", "Pacific/Apia",
        "America/St_Johns", "Antarctica/Troll", "Asia/Kathmandu", "Europe/Moscow", "Australia/Sydney",
        "America/Los_Angeles", "America/Panama", "EST", "MST", "PST", "US/Arizona", "UTC", "GMT", "Etc/GMT+5",
        "GMT+05:30", "+05:30", "Z", "Mars/Base", "bad id!"};

    /** The constants every zone shifts: timestampLit's reading's edges. */
    static final String[] FIXED = {"2019-04-08T18:00:00", "2019-04-08T18:00:00.000", "2019-04-08 18:00:00.123456",
        "2019-04-08T18:00:30", "2019-04-08", "2019-04-08T18:00", "2019-02-30T00:00:00", "0001-01-01T00:00:00",
        "9999-12-31T23:59:59", "+10000-01-01T00:00:00", "2019-04-08t18:00:00", "1883-11-18T19:00:00",
        "2011-12-29T10:00:00", "2026-07-01T00:00:00.5"};

    public static void main(String[] args) throws IOException {
        StringBuilder out = new StringBuilder();
        out.append("# EngineStyleH2(false, <zone>) rendering select TIMESTAMP'<iso>' as \"t\" (java ")
                .append(Runtime.version()).append("): <zone> <iso> <Java's text>\n");
        for (String zone : ZONES) {
            Set<String> isos = new LinkedHashSet<>(List.of(FIXED));
            isos.addAll(aroundTransitions(zone));
            for (String iso : isos) {
                out.append(zone).append('\t').append(iso).append('\t').append(render(zone, iso)).append('\n');
            }
        }
        Files.writeString(Path.of(args[0]), out.toString(), StandardCharsets.UTF_8);
    }

    /** The UTC instants just before and at the zone's transitions of 1950, 2019 and 2050, as ISO text. */
    static List<String> aroundTransitions(String zone) {
        List<String> isos = new ArrayList<>();
        ZoneRules rules;
        try {
            rules = ZoneId.of(zone, ZoneId.SHORT_IDS).getRules();
        } catch (RuntimeException e) {
            return isos;
        }
        for (int year : new int[] {1950, 2019, 2050}) {
            ZoneOffsetTransition t = rules.nextTransition(LocalDateTime.of(year, 1, 1, 0, 0).toInstant(ZoneOffset.UTC));
            for (int i = 0; i < 2 && t != null && t.getInstant().atZone(ZoneOffset.UTC).getYear() == year; i++) {
                for (long s : new long[] {-1, 0}) {
                    Instant at = t.getInstant().plusSeconds(s);
                    String iso = LocalDateTime.ofInstant(at, ZoneOffset.UTC).toString();
                    isos.add(iso.length() == 16 ? iso + ":00" : iso);
                    isos.add((iso.length() == 16 ? iso + ":00" : iso) + ".000");
                }
                t = rules.nextTransition(t.getInstant());
            }
        }
        return isos;
    }

    static String render(String zone, String iso) {
        SqlSelect select = new SqlSelect(List.of(new SqlSelect.Projection(new SqlExpr.TimestampLit(iso), "t", null)),
                false, new SqlSource.Dual(), null, List.of(), null, null, List.of(), null, null, List.of());
        try {
            return new EngineStyleH2(false, zone).render(select);
        } catch (RuntimeException e) {
            return "THROWS " + e.getClass().getName() + ": " + e.getMessage();
        }
    }
}
