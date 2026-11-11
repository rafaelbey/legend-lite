// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

import com.legend.executionplan.MirJson;
import com.legend.json.Json;
import com.legend.sql.SqlDdl;
import com.legend.sql.SqlDml;
import com.legend.sql.SqlExpr;
import com.legend.sql.SqlQuery;
import com.legend.sql.dialect.EngineTextDialect;
import com.legend.sql.dialect.RenderedStatement;
import com.legend.sql.dialect.SqlDialect;
import java.io.IOException;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.HashSet;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.Set;
import java.util.TreeMap;

/**
 * The golden dumps re-answered by a later tree's Java dialects (docs/rust-port/PHASE2.md §4, the merge of `main` at
 * `c4a39e06d`, 2026-11-10): every line of a frozen dump ({@code rust/oracles/mir/*.jsonl}, the line format
 * {@code {"dialect","config","fresh","op","kind","mir","answer"}}) is read back -- its MirJson by the tree's
 * {@code MirJson} (the port's, with its DDL and DML codec), its string op's input as written -- and answered again on a
 * FRESH dialect of its class, made with its configuration, the answer written in the dumps' own form (the probes'
 * {@code MirDumpProbe.answerOf}: a text, a statement's SQL with its binds and regions as the records print them, or
 * a refusal's class and message). The input is the frozen one, byte for byte, so the new file differs from the old in
 * answers alone: each answer that moved is a change of the tree's dialects, which the Rust replay must explain.
 *
 * <p>A dialect the tree no longer has is re-asked of the one that absorbed it, named in {@link #FOLDED}
 * ({@code H2Modern}, folded into {@code H2} on `main` 2026-11-05): its line is written under the new name, and dropped
 * when that name already holds the same op and input (counted). Run on the tree's built jars with the build's JDK, from
 * the tree's root:
 * <pre>
 *   bazel build //core:sql_dialect //core:execution_plan //core:model //json //base
 *   $(bazel info output_base)/external/rules_java++toolchains+remotejdk25_&lt;os&gt;_&lt;arch&gt;/bin/java -Xss64m \
 *     -cp bazel-bin/core/libsql_dialect.jar:bazel-bin/core/libsql.jar:bazel-bin/core/libexecution_plan.jar:\
 *   bazel-bin/core/libmodel.jar:bazel-bin/json/libjson.jar:bazel-bin/base/libbase.jar \
 *     rust/oracles/mir/ReAnswer.java old.jsonl new.jsonl changes.jsonl
 * </pre>
 * {@code changes.jsonl} gets one line per moved answer: {@code {"at","dialect","op","kind","old","new"}}.
 */
public final class ReAnswer {

    private ReAnswer() {
    }

    /** A dialect the tree folded into another, re-asked of that one. */
    static final Map<String, String> FOLDED = Map.of("H2Modern", "H2");

    /** A MirJson text read back with no nesting limit but the stack's. */
    private static final Json.Config DEEP = new Json.Config(1 << 20);

    public static void main(String[] args) throws IOException {
        List<String> in = Files.readAllLines(Path.of(args[0]), StandardCharsets.UTF_8);
        List<String> out = new ArrayList<>();
        List<String> changes = new ArrayList<>();
        Set<String> keys = new HashSet<>();
        Map<String, int[]> perDialect = new TreeMap<>();
        int folded = 0;
        int foldedDuplicates = 0;
        int kept = 0;
        // the keys the file holds under each dialect's own name, so a folded line that repeats one is dropped
        for (String line : in) {
            Json.Obj o = (Json.Obj) Json.parse(line, DEEP);
            keys.add(key(o.getString("dialect"), o));
        }
        for (int n = 0; n < in.size(); n++) {
            String line = in.get(n);
            Json.Obj o = (Json.Obj) Json.parse(line, DEEP);
            String dialect = o.getString("dialect");
            String asked = FOLDED.getOrDefault(dialect, dialect);
            if (!asked.equals(dialect)) {
                folded++;
                if (!keys.add(key(asked, o))) {
                    foldedDuplicates++;
                    continue;
                }
            }
            String op = o.getString("op");
            Json.Node config = o.getOr("config", Json.Null.INSTANCE);
            if (!isDialect(asked)) {
                // a renderer of a test's own (the phase 0 dump's Portable, CarrierDifferentialTest's): no class of the
                // tree's to ask; its line stays as frozen (the MIR's round trip reads it, no dialect differential)
                kept++;
                out.add(line + "\n");
                continue;
            }
            String answer = answer(asked, config, op, o.get("mir"));
            // the line's text up to its answer, the last field, kept byte for byte (an answer's strings are escaped, so
            // the separator cannot occur inside one); a folded dialect's name rewritten at the line's head
            int cut = line.lastIndexOf(",\"answer\":");
            String head = line.substring(0, cut);
            String oldAnswer = line.substring(cut + ",\"answer\":".length(), line.length() - 1);
            String oldHead = "{\"dialect\":" + quote(dialect) + ",";
            if (!head.startsWith(oldHead)) {
                throw new IllegalStateException("line " + (n + 1) + " does not start with its dialect");
            }
            if (!asked.equals(dialect)) {
                head = "{\"dialect\":" + quote(asked) + "," + head.substring(oldHead.length());
            }
            // an answer equal in value to the frozen one keeps its frozen spelling (a control character the first probe
            // wrote otherwise), so the file moves only where an answer did
            if (!answer.equals(oldAnswer) && Json.parse(answer, DEEP).equals(Json.parse(oldAnswer, DEEP))) {
                answer = oldAnswer;
            }
            int[] tally = perDialect.computeIfAbsent(asked, k -> new int[2]);
            tally[0]++;
            // the answer as the probes wrote it: appended as text, so its spelling is exactly answerOf's
            out.add(head + ",\"answer\":" + answer + "}\n");
            if (!answer.equals(oldAnswer) || !asked.equals(dialect)) {
                tally[1]++;
                Map<String, Object> c = new LinkedHashMap<>();
                c.put("at", n + 1);
                c.put("dialect", dialect + (asked.equals(dialect) ? "" : " -> " + asked));
                c.put("op", op);
                c.put("kind", o.get("kind"));
                String ch = Json.toCompact(c);
                changes.add(ch.substring(0, ch.length() - 1) + ",\"old\":" + oldAnswer + ",\"new\":" + answer + "}\n");
            }
        }
        Files.writeString(Path.of(args[1]), String.join("", out), StandardCharsets.UTF_8);
        Files.writeString(Path.of(args[2]), String.join("", changes), StandardCharsets.UTF_8);
        StringBuilder s = new StringBuilder();
        perDialect.forEach((d, t) -> s.append(' ').append(d).append(' ').append(t[0]).append('/').append(t[1]));
        System.out.println(in.size() + " lines in, " + out.size() + " out (dialect lines/moved:" + s + "); " + folded
                + " folded, " + foldedDuplicates + " of them dropped as duplicates; " + kept
                + " kept as frozen, no dialect class (java " + Runtime.version() + ")");
    }

    private static boolean isDialect(String dialect) {
        try {
            return SqlDialect.class.isAssignableFrom(Class.forName("com.legend.sql.dialect." + dialect));
        } catch (ClassNotFoundException e) {
            return false;
        }
    }

    private static String key(String dialect, Json.Obj o) {
        return dialect + '\u0000' + Json.toCompact(o.getOr("config", Json.Null.INSTANCE)) + '\u0000' + o.getString("op") + '\u0000'
                + Json.toCompact(o.get("mir"));
    }

    /** The answer of a fresh {@code dialect} made with {@code config}, in the probes' form. */
    private static String answer(String dialect, Json.Node config, String op, Json.Node mir) {
        try {
            SqlDialect d = fresh(dialect, config);
            return "{\"ok\":" + answerOf(d, op, mir) + "}";
        } catch (RuntimeException e) {
            return "{\"throws\":" + quote(e.getClass().getName()) + ",\"message\":" + quote(String.valueOf(e.getMessage()))
                    + "}";
        }
    }

    private static SqlDialect fresh(String dialect, Json.Node config) {
        try {
            Class<?> cls = Class.forName("com.legend.sql.dialect." + dialect);
            if (config instanceof Json.Obj c) {
                boolean quote = c.getBool("quoteIdentifiers");
                Json.Node zone = c.get("timeZone");
                String timeZone = zone instanceof Json.Str z ? z.value() : null;
                if (dialect.equals("EngineStyleComposite")) {
                    return (SqlDialect) cls.getDeclaredConstructor().newInstance();
                }
                return (SqlDialect) cls.getDeclaredConstructor(boolean.class, String.class).newInstance(quote, timeZone);
            }
            return (SqlDialect) cls.getDeclaredConstructor().newInstance();
        } catch (ReflectiveOperationException e) {
            throw new IllegalStateException("no dialect " + dialect + ": " + e, e);
        }
    }

    private static String answerOf(SqlDialect d, String op, Json.Node mir) {
        return switch (op) {
            case "query.render" -> quote(d.render(query(mir)));
            case "query.renderStatement" -> statement(d.renderStatement(query(mir)));
            case "ddl.render" -> quote(d.render(MirJson.readDdl((Json.Obj) mir)));
            case "dml.render" -> quote(d.render(MirJson.readDml((Json.Obj) mir)));
            case "dml.renderStatement" -> statement(d.renderStatement(MirJson.readDml((Json.Obj) mir)));
            case "fromEngineText" -> quote(d.fromEngineText(((Json.Str) mir).value()));
            case "engine.renderAsAliased" -> quote(engine(d).renderAsAliased(query(mir)));
            case "engine.selectList" -> {
                String list = engine(d).selectList(query(mir));
                yield list == null ? "null" : quote(list);
            }
            case "engine.collectionSplice" -> quote(engine(d).collectionSplice(
                    (SqlExpr.PlanParam) MirJson.readExpr((Json.Obj) mir)));
            case "engine.renderedAlias" -> quote(engine(d).renderedAlias(((Json.Str) mir).value()));
            case "engine.inTempTableSelect" -> quote(engine(d).inTempTableSelect(((Json.Str) mir).value()));
            default -> throw new IllegalStateException("no op " + op);
        };
    }

    private static SqlQuery query(Json.Node mir) {
        return MirJson.readQuery((Json.Obj) mir);
    }

    private static EngineTextDialect engine(SqlDialect d) {
        return (EngineTextDialect) d;
    }

    private static String statement(RenderedStatement s) {
        return "{\"sql\":" + quote(s.sql()) + ",\"binds\":" + quote(String.valueOf(s.binds())) + ",\"regions\":"
                + quote(String.valueOf(s.regions())) + "}";
    }

    /** The probes' own quoting ({@code MirDumpProbe.quote}). */
    static String quote(String s) {
        StringBuilder b = new StringBuilder("\"");
        for (char c : s.toCharArray()) {
            switch (c) {
                case '"' -> b.append("\\\"");
                case '\\' -> b.append("\\\\");
                case '\n' -> b.append("\\n");
                case '\r' -> b.append("\\r");
                case '\t' -> b.append("\\t");
                default -> {
                    if (c < 0x20) {
                        b.append(String.format(java.util.Locale.ROOT, "\\u%04x", (int) c));
                    } else {
                        b.append(c);
                    }
                }
            }
        }
        return b.append('"').toString();
    }
}
