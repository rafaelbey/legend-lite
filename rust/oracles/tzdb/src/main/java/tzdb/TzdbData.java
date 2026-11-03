// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

package tzdb;

import java.io.ByteArrayOutputStream;
import java.io.IOException;
import java.lang.reflect.Field;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.time.DayOfWeek;
import java.time.Duration;
import java.time.Instant;
import java.time.LocalDateTime;
import java.time.ZoneId;
import java.time.ZoneOffset;
import java.time.ZonedDateTime;
import java.time.zone.ZoneOffsetTransition;
import java.time.zone.ZoneOffsetTransitionRule;
import java.time.zone.ZoneRules;
import java.time.zone.ZoneRulesProvider;
import java.util.ArrayList;
import java.util.Arrays;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.TreeMap;
import java.util.TreeSet;

/**
 * The JDK's time-zone database as the Rust port reads it (docs/rust-port/PHASE1.md §4 step 4c; rust/oracles/tzdb):
 * every region {@code ZoneId.of(id, ZoneId.SHORT_IDS)} accepts, each with its rules exactly as {@link ZoneRules} holds
 * them -- the standard transitions and offsets, the savings (wall) transitions and offsets, and the last rules -- in
 * {@code tzdb.bin}, which the dialect library embeds (rust/lite-dialect/src/java_time.rs reads its format, written
 * below); and the golden answers of {@code java.time} over them in {@code zone-golden.tsv}, which the Rust port is
 * held to. A build action on the build's JDK (remotejdk_25): {@code bazel build //rust/oracles/tzdb:tzdb_data}; the
 * two outputs are committed as {@code rust/oracles/tzdb/tzdb.bin} and {@code zone-golden.tsv.zst} (MANIFEST.tsv).
 *
 * <p>The arrays are read from {@link ZoneRules}' own fields (the JVM opens {@code java.time.zone} to this program):
 * the standard transitions have no public accessor, and the copy must be the class's, not a reconstruction; each is
 * checked against the public answers ({@link ZoneRules#getTransitions}, {@link ZoneRules#getTransitionRules}).
 */
public final class TzdbData {

    private TzdbData() {
    }

    /** Usage: {@code TzdbData <tzdb.bin> <zone-golden.tsv>}. */
    public static void main(String[] args) throws IOException, ReflectiveOperationException {
        TreeSet<String> ids = new TreeSet<>(ZoneId.getAvailableZoneIds());
        // the distinct rule sets, in the order of their first region (ZoneRules.equals: the same arrays)
        List<ZoneRules> sets = new ArrayList<>();
        Map<String, Integer> setOf = new LinkedHashMap<>();
        for (String id : ids) {
            ZoneRules rules = ZoneId.of(id).getRules();
            int at = sets.indexOf(rules);
            if (at < 0) {
                at = sets.size();
                sets.add(rules);
            }
            setOf.put(id, at);
        }
        Map<String, String> shortIds = new TreeMap<>(ZoneId.SHORT_IDS);
        String java = Runtime.version().toString();
        String tzdb = ZoneRulesProvider.getVersions("UTC").lastEntry().getKey();

        Bytes out = new Bytes();
        out.raw("LTZDB".getBytes(StandardCharsets.US_ASCII));
        out.u(1);
        out.str(java);
        out.str(tzdb);
        out.u(sets.size());
        for (ZoneRules rules : sets) {
            writeRules(out, rules);
        }
        out.u(setOf.size());
        for (Map.Entry<String, Integer> e : setOf.entrySet()) {
            out.str(e.getKey());
            out.u(e.getValue());
        }
        out.u(shortIds.size());
        for (Map.Entry<String, String> e : shortIds.entrySet()) {
            out.str(e.getKey());
            out.str(e.getValue());
        }
        Files.write(Path.of(args[0]), out.bytes());

        StringBuilder golden = new StringBuilder();
        golden.append("# java.time over the JDK's tzdb, the Rust port's oracle (rust/oracles/tzdb/MANIFEST.tsv): java ")
                .append(java).append(", tzdb ").append(tzdb).append('\n');
        golden.append("# O <zone> <epoch second> <offset> <standard offset> <the instant's local date-time there>\n");
        golden.append("# L <zone> <local date-time> <valid offsets> <transition epoch second or -> <ZonedDateTime.of:"
                + " local date-time> <its offset> <its epoch second>\n");
        // every ID ZoneId.of(id, SHORT_IDS) accepts, sparsely: the region's rules found by its name
        List<String> named = new ArrayList<>(ids);
        named.addAll(shortIds.keySet());
        named.addAll(List.of("UTC", "GMT", "UT", "Z", "UTC+05:30", "GMT-8", "UT+01:00", "+05", "-0330", "+14:00",
                "-18:00", "+05:30:15"));
        for (String id : named) {
            ZoneId zone = ZoneId.of(id, ZoneId.SHORT_IDS);
            for (long s : SPARSE) {
                instant(golden, id, zone, s);
            }
            for (LocalDateTime ldt : SPARSE_LOCAL) {
                local(golden, id, zone, ldt);
            }
        }
        // every rule set, densely: around each transition of 1800 to 2100, and of years 1, 1000, 5000 and 9999
        for (int i = 0; i < sets.size(); i++) {
            String id = firstOf(setOf, i);
            ZoneId zone = ZoneId.of(id);
            ZoneRules rules = sets.get(i);
            List<ZoneOffsetTransition> around = new ArrayList<>();
            transitions(rules, LocalDateTime.of(1800, 1, 1, 0, 0), LocalDateTime.of(2101, 1, 1, 0, 0), around);
            for (int year : new int[] {1, 1000, 5000, 9999}) {
                transitions(rules, LocalDateTime.of(year, 1, 1, 0, 0), LocalDateTime.of(year, 12, 31, 0, 0), around);
            }
            for (ZoneOffsetTransition t : around) {
                long s = t.toEpochSecond();
                instant(golden, id, zone, s - 1);
                instant(golden, id, zone, s);
                LocalDateTime a = t.isGap() ? t.getDateTimeBefore() : t.getDateTimeAfter();
                LocalDateTime b = t.isGap() ? t.getDateTimeAfter() : t.getDateTimeBefore();
                long width = Duration.between(a, b).getSeconds();
                local(golden, id, zone, a.minusSeconds(1));
                local(golden, id, zone, a);
                local(golden, id, zone, a.plusSeconds(width / 2));
                local(golden, id, zone, b);
            }
        }
        Files.writeString(Path.of(args[1]), golden.toString(), StandardCharsets.UTF_8);
    }

    /** The instants every ID is sampled at: years 1, 1000 and 9999, and one a decade, January and July, 1800-2100. */
    private static final long[] SPARSE;

    /** The local date-times every ID is resolved at: the same days at noon. */
    private static final LocalDateTime[] SPARSE_LOCAL;

    static {
        List<LocalDateTime> at = new ArrayList<>(List.of(LocalDateTime.of(1, 1, 1, 0, 0),
                LocalDateTime.of(1000, 6, 15, 12, 0), LocalDateTime.of(9999, 12, 31, 23, 59, 59)));
        for (int year = 1800; year <= 2100; year += 10) {
            at.add(LocalDateTime.of(year, 1, 15, 12, 0));
            at.add(LocalDateTime.of(year, 7, 15, 12, 0));
        }
        SPARSE = at.stream().mapToLong(t -> t.toEpochSecond(ZoneOffset.UTC)).toArray();
        SPARSE_LOCAL = at.toArray(new LocalDateTime[0]);
    }

    /** The first region of rule set {@code set}, by name. */
    private static String firstOf(Map<String, Integer> setOf, int set) {
        for (Map.Entry<String, Integer> e : setOf.entrySet()) {
            if (e.getValue() == set) {
                return e.getKey();
            }
        }
        throw new IllegalStateException("no region has rule set " + set);
    }

    /** Every transition from {@code from} (UTC) to before {@code to}, by {@link ZoneRules#nextTransition}. */
    private static void transitions(ZoneRules rules, LocalDateTime from, LocalDateTime to,
            List<ZoneOffsetTransition> into) {
        Instant end = to.toInstant(ZoneOffset.UTC);
        ZoneOffsetTransition t = rules.nextTransition(from.toInstant(ZoneOffset.UTC));
        while (t != null && t.getInstant().isBefore(end)) {
            into.add(t);
            t = rules.nextTransition(t.getInstant());
        }
    }

    /** {@code O}: the instant's offset, standard offset and local date-time in {@code zone}. */
    private static void instant(StringBuilder out, String id, ZoneId zone, long epochSecond) {
        Instant i = Instant.ofEpochSecond(epochSecond);
        ZoneRules rules = zone.getRules();
        out.append("O\t").append(id).append('\t').append(epochSecond).append('\t')
                .append(rules.getOffset(i).getTotalSeconds()).append('\t')
                .append(rules.getStandardOffset(i).getTotalSeconds()).append('\t')
                .append(i.atZone(ZoneOffset.UTC).withZoneSameInstant(zone).toLocalDateTime()).append('\n');
    }

    /** {@code L}: the local date-time's valid offsets and transition in {@code zone}, and its resolution there. */
    private static void local(StringBuilder out, String id, ZoneId zone, LocalDateTime ldt) {
        ZoneRules rules = zone.getRules();
        List<String> valid = new ArrayList<>();
        for (ZoneOffset o : rules.getValidOffsets(ldt)) {
            valid.add(Integer.toString(o.getTotalSeconds()));
        }
        ZoneOffsetTransition t = rules.getTransition(ldt);
        ZonedDateTime z = ZonedDateTime.of(ldt, zone);
        out.append("L\t").append(id).append('\t').append(ldt).append('\t').append(String.join(",", valid))
                .append('\t').append(t == null ? "-" : Long.toString(t.toEpochSecond())).append('\t')
                .append(z.toLocalDateTime()).append('\t').append(z.getOffset().getTotalSeconds()).append('\t')
                .append(z.toEpochSecond()).append('\n');
    }

    // ---- the binary form (java_time.rs reads it): unsigned LEB128 varints, a signed value zigzag-encoded

    /** One rule set: its standard and savings transitions (the first epoch second absolute, each next a delta), each
     *  list's offsets (the first before every transition), and its last rules. */
    private static void writeRules(Bytes out, ZoneRules rules) throws ReflectiveOperationException {
        long[] standardTransitions = (long[]) field(rules, "standardTransitions");
        ZoneOffset[] standardOffsets = (ZoneOffset[]) field(rules, "standardOffsets");
        long[] savings = (long[]) field(rules, "savingsInstantTransitions");
        ZoneOffset[] wallOffsets = (ZoneOffset[]) field(rules, "wallOffsets");
        ZoneOffsetTransitionRule[] lastRules = (ZoneOffsetTransitionRule[]) field(rules, "lastRules");
        check(standardOffsets.length == standardTransitions.length + 1, "standard offsets");
        check(wallOffsets.length == savings.length + 1, "wall offsets");
        // the public answers agree with the fields read
        List<ZoneOffsetTransition> pub = rules.getTransitions();
        check(pub.size() == savings.length, "transitions");
        for (int i = 0; i < savings.length; i++) {
            check(pub.get(i).toEpochSecond() == savings[i] && pub.get(i).getOffsetBefore().equals(wallOffsets[i])
                    && pub.get(i).getOffsetAfter().equals(wallOffsets[i + 1]), "transition " + i);
        }
        check(rules.getTransitionRules().equals(Arrays.asList(lastRules)), "last rules");
        writeTransitions(out, standardTransitions, standardOffsets);
        writeTransitions(out, savings, wallOffsets);
        out.u(lastRules.length);
        for (ZoneOffsetTransitionRule r : lastRules) {
            out.u(r.getMonth().getValue());
            out.s(r.getDayOfMonthIndicator());
            DayOfWeek dow = r.getDayOfWeek();
            out.u(dow == null ? 0 : dow.getValue());
            check(r.getLocalTime().getNano() == 0, "a rule's time has no fraction");
            out.u(r.getLocalTime().toSecondOfDay());
            out.u((r.isMidnightEndOfDay() ? 1 : 0) | (r.getTimeDefinition().ordinal() << 1));
            out.s(r.getStandardOffset().getTotalSeconds());
            out.s(r.getOffsetBefore().getTotalSeconds());
            out.s(r.getOffsetAfter().getTotalSeconds());
        }
    }

    private static void writeTransitions(Bytes out, long[] transitions, ZoneOffset[] offsets) {
        out.u(transitions.length);
        out.s(offsets[0].getTotalSeconds());
        long previous = 0;
        for (int i = 0; i < transitions.length; i++) {
            if (i == 0) {
                out.s(transitions[0]);
            } else {
                check(transitions[i] > previous, "transitions ascend");
                out.u(transitions[i] - previous);
            }
            previous = transitions[i];
            out.s(offsets[i + 1].getTotalSeconds());
        }
    }

    private static Object field(ZoneRules rules, String name) throws ReflectiveOperationException {
        Field f = ZoneRules.class.getDeclaredField(name);
        f.setAccessible(true);
        Object value = f.get(rules);
        if (value == null) {
            throw new IllegalStateException("ZoneRules." + name + " is null");
        }
        return value;
    }

    private static void check(boolean holds, String what) {
        if (!holds) {
            throw new IllegalStateException("the tzdb data disagrees with ZoneRules: " + what);
        }
    }

    /** The output bytes. */
    private static final class Bytes {
        private final ByteArrayOutputStream buf = new ByteArrayOutputStream();

        void raw(byte[] b) {
            buf.writeBytes(b);
        }

        /** An unsigned LEB128 varint. */
        void u(long v) {
            check(v >= 0, "a varint is not negative");
            long rest = v;
            while (rest >= 0x80) {
                buf.write((int) (rest & 0x7f) | 0x80);
                rest >>>= 7;
            }
            buf.write((int) rest);
        }

        /** A signed value, zigzag-encoded. */
        void s(long v) {
            u((v << 1) ^ (v >> 63));
        }

        void str(String s) {
            byte[] b = s.getBytes(StandardCharsets.UTF_8);
            u(b.length);
            raw(b);
        }

        byte[] bytes() {
            return buf.toByteArray();
        }
    }
}
