// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

import com.legend.executionplan.MirJson;
import com.legend.json.Json;
import com.legend.sql.NameQuoting;
import com.legend.sql.OutputCol;
import com.legend.sql.SqlDdl;
import com.legend.sql.SqlDml;
import com.legend.sql.SqlExpr;
import com.legend.sql.SqlFn;
import com.legend.sql.SqlQuery;
import com.legend.sql.SqlSelect;
import com.legend.sql.SqlSource;
import com.legend.sql.SqlType;
import com.legend.sql.SqlUnion;
import com.legend.sql.SqlWith;
import com.legend.sql.TypeFact;
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

/**
 * The arms `main` added to the SQL tree and the dialects after the port's base (docs/rust-port/PHASE2.md §4, the merge
 * of `main` at `c4a39e06d`, 2026-11-10), over inputs no frozen oracle holds -- the trees that carry them came with
 * `main`'s lowering -- rendered by `main`'s own dialects: LIST_AT (SqlFn, 2026-11-03) on every dialect, a list's and
 * a literal array's, typed and untyped; a raw grid's report (SqlSource.GridReport, 2026-10-30) in each of its four
 * parts; a CSV file's load (SqlDml.LoadCsv, 2026-11-09); H2's folded JSON navigation, JSON_TYPE and domain guard; and
 * a fused statement whose two blocks reuse one alias, which StoredReads maps per block (QueryBlocks, 2026-11-05). Each
 * input is built with the tree's constructors, written as MirJson, READ BACK, and rendered on a FRESH dialect of each
 * of the tree's fifteen classes (the engine text at its default configuration), in the dumps' line format
 * ({@code {"dialect","config","fresh","op","kind","mir","answer"}}, a refusal by its class and message). Run on the
 * tree's built jars with the build's JDK, from the tree's root, the port's MirJson (its DDL and DML codec) in place:
 * <pre>
 *   bazel build //core:sql_dialect //core:execution_plan //core:model //json //base
 *   $(bazel info output_base)/external/rules_java++toolchains+remotejdk25_&lt;os&gt;_&lt;arch&gt;/bin/java -Xss64m \
 *     -cp bazel-bin/core/libsql_dialect.jar:bazel-bin/core/libsql.jar:bazel-bin/core/libexecution_plan.jar:\
 *   bazel-bin/core/libmodel.jar:bazel-bin/json/libjson.jar:bazel-bin/base/libbase.jar \
 *     rust/oracles/mir/MergeArms.java merge-arms.jsonl
 * </pre>
 */
public final class MergeArms {

    private MergeArms() {
    }

    /** The tree's dialects, by the simple name the seam opens. */
    static final List<String> DIALECTS = List.of("DuckDb", "H2", "EngineStyleH2", "EngineStyleDB2",
            "EngineStyleComposite", "Postgres", "SQLite", "Trino", "Databricks", "ClickHouse", "BigQuery", "Snowflake",
            "MariaDb", "MySql", "MongoDb");

    private static final List<String> LINES = new ArrayList<>();
    private static final Set<String> SEEN = new HashSet<>();
    private static final Map<String, Integer> PER_DIALECT = new LinkedHashMap<>();
    private static int inputs;
    private static final Json.Config DEEP = new Json.Config(1 << 20);

    // ---- the table every expression reads (DialectArms' table, its columns this oracle reads)

    static final SqlType BIGINT = SqlType.Scalar.BIGINT;
    static final SqlType VARCHAR = SqlType.Scalar.VARCHAR;
    static final SqlType DOUBLE = SqlType.Scalar.DOUBLE;
    static final SqlType JSON = SqlType.Scalar.JSON;
    static final SqlType STRUCT = new SqlType.Struct(List.of(new SqlType.Struct.Field("a", BIGINT),
            new SqlType.Struct.Field("b", VARCHAR)));
    static final SqlType XS = new SqlType.Array(BIGINT);
    static final SqlType SS = new SqlType.Array(VARCHAR);
    static final SqlType JS = new SqlType.Array(JSON);
    static final SqlType XSS = new SqlType.Array(XS);
    static final SqlType STS = new SqlType.Array(STRUCT);

    static final List<OutputCol> COLUMNS = List.of(oc("s", VARCHAR), oc("n", BIGINT), oc("d", DOUBLE), oc("j", JSON),
            oc("st", STRUCT), oc("xs", XS), oc("ss", SS), oc("js", JS), oc("xss", XSS), oc("sts", STS));

    static OutputCol oc(String name, SqlType type) {
        return new OutputCol(name, type, true, OutputCol.Origin.PHYSICAL);
    }

    static SqlSource t() {
        return new SqlSource.Table("t", "t0", COLUMNS, false, Map.of(), NameQuoting.BARE);
    }

    static SqlExpr c(String name) {
        return SqlExpr.Column.of("t0", COLUMNS, name);
    }

    static SqlExpr str(String s) {
        return new SqlExpr.StringLit(s);
    }

    static SqlExpr i(long v) {
        return new SqlExpr.IntLit(v);
    }

    static SqlExpr call(SqlFn fn, SqlExpr... args) {
        return SqlExpr.Call.of(fn, args);
    }

    static SqlExpr arr(SqlExpr... elements) {
        return new SqlExpr.ArrayLit(List.of(elements));
    }

    static SqlSelect.Projection p(SqlExpr e, String alias) {
        return e.type() instanceof TypeFact.Typed t
                ? new SqlSelect.Projection(e, alias, new OutputCol(alias, t.type(), true))
                : new SqlSelect.Projection(e, alias, null);
    }

    static SqlSelect select(SqlSource from, List<SqlSelect.Projection> ps, SqlExpr where) {
        return new SqlSelect(ps, false, from, where, List.of(), null, null, List.of(), null, null, List.of());
    }

    static SqlSelect select(SqlSource from, SqlSelect.Projection... ps) {
        return select(from, List.of(ps), null);
    }

    /** One expression projected as {@code v} over the table: as a statement and as text. */
    static void e(SqlExpr x) {
        add("query.renderStatement", select(t(), p(x, "v")));
        add("query.render", select(t(), p(x, "v")));
    }

    public static void main(String[] args) throws IOException {
        listAt();
        gridReports();
        csvLoads();
        h2Arms();
        queryBlocks();
        Files.writeString(Path.of(args[0]), String.join("", LINES), StandardCharsets.UTF_8);
        System.out.println(inputs + " inputs, " + LINES.size() + " lines " + PER_DIALECT + " (java "
                + Runtime.version() + ")");
    }

    // ---- LIST_AT: a position the list holds

    static void listAt() {
        e(call(SqlFn.LIST_AT, c("xs"), i(1)));
        e(call(SqlFn.LIST_AT, c("xs"), c("n")));
        e(call(SqlFn.LIST_AT, c("ss"), i(2)));
        e(call(SqlFn.LIST_AT, c("js"), i(1)));
        e(call(SqlFn.LIST_AT, c("sts"), i(1)));
        e(call(SqlFn.LIST_AT, c("xss"), i(1)));
        e(new SqlExpr.StructGet(call(SqlFn.LIST_AT, c("sts"), i(1)), "a"));
        // a literal array: H2's pick (no JSON carrier), the dialect's own read elsewhere
        e(call(SqlFn.LIST_AT, arr(i(1), i(2)), c("n")));
        e(call(SqlFn.LIST_AT, arr(str("a"), str("b")), i(2)));
        // the NULL list (BigQuery: its type's NULL)
        e(call(SqlFn.LIST_AT, new SqlExpr.NullLit(), i(1)));
        // nested positions, the tagged list's reads
        e(call(SqlFn.LIST_AT, c("xs"), call(SqlFn.LIST_AT, c("xs"), i(1))));
        add("query.renderStatement", select(t(), List.of(p(c("n"), "n")),
                call(SqlFn.EQUAL, call(SqlFn.LIST_AT, c("xs"), i(1)), i(3))));
    }

    // ---- a raw grid's report, each part

    static void gridReports() {
        SqlQuery grid = select(t(), p(c("s"), "s"), p(c("n"), "n"));
        for (SqlSource.GridReport.Part part : SqlSource.GridReport.Part.values()) {
            SqlSource.GridReport g = new SqlSource.GridReport(grid, part, "r0");
            List<SqlSelect.Projection> ps = new ArrayList<>();
            for (OutputCol col : g.outputs()) {
                ps.add(p(SqlExpr.Column.of("r0", g.outputs(), col.name()), col.name()));
            }
            add("query.renderStatement", select(g, ps, null));
            add("query.render", select(g, ps, null));
        }
    }

    // ---- a CSV file's load

    static void csvLoads() {
        SqlDml plain = new SqlDml.LoadCsv(null, "T", NameQuoting.BARE, List.of("a", "b"), Set.of());
        SqlDml quoted = new SqlDml.LoadCsv("S", "T", new NameQuoting(true, true), List.of("a", "B"),
                Set.of("B"));
        for (SqlDml d : List.of(plain, quoted)) {
            add("dml.render", d);
            add("dml.renderStatement", d);
        }
    }

    // ---- H2's arms: H2Modern's folded in, JSON_TYPE, the domain guard

    static void h2Arms() {
        SqlExpr get = call(SqlFn.VARIANT_GET, c("j"), str("k"));
        e(get);
        e(call(SqlFn.VARIANT_GET, c("j"), i(0)));
        e(call(SqlFn.VARIANT_GET, c("j"), c("s")));
        e(new SqlExpr.Cast(get, BIGINT));
        e(new SqlExpr.Cast(get, VARCHAR));
        e(new SqlExpr.Cast(get, JSON));
        e(new SqlExpr.StructGet(c("st"), "a"));
        e(new SqlExpr.Cast(new SqlExpr.StructGet(c("st"), "a"), BIGINT));
        e(new SqlExpr.StructLit(List.of(new SqlExpr.StructLit.Field("a", i(1)),
                new SqlExpr.StructLit.Field("b", str("x")))));
        e(call(SqlFn.LIST_LENGTH, c("xs")));
        e(call(SqlFn.LIST_LENGTH, new SqlExpr.Cast(c("j"), XS)));
        e(call(SqlFn.LIST_LENGTH, c("xs"), c("xs")));
        e(call(SqlFn.JSON_TYPE, c("j")));
        e(call(SqlFn.JSON_TYPE, get));
        e(call(SqlFn.ASIN, c("d")));
        e(call(SqlFn.ACOS, c("d")));
        e(call(SqlFn.ACOS, new SqlExpr.FloatLit(2.0)));
    }

    // ---- one alias in two blocks: a stored read mapped per block

    static void queryBlocks() {
        List<OutputCol> cols = List.of(oc("o", VARCHAR), oc("s", VARCHAR));
        Map<String, SqlDdl.ColumnType> stored = new LinkedHashMap<>();
        stored.put("o", new SqlDdl.ColumnType.Plain(SqlDdl.ColumnType.Kind.OTHER));
        SqlSource storedT = new SqlSource.Table("t", "t0", cols, false, stored, NameQuoting.BARE);
        SqlSource plainU = new SqlSource.Table("u", "t0", cols, false, Map.of(), NameQuoting.BARE);
        SqlQuery a = select(storedT, p(SqlExpr.Column.of("t0", cols, "o"), "o"));
        SqlQuery b = select(plainU, p(SqlExpr.Column.of("t0", cols, "o"), "o"));
        add("query.renderStatement", new SqlWith(List.of(new SqlWith.Cte("a", a), new SqlWith.Cte("b", b)),
                select(new SqlSource.Cte("a", "a0", a.outputs()), p(SqlExpr.Column.of("a0", a.outputs(), "o"), "o"))));
        add("query.renderStatement", new SqlUnion(List.of(a, b), true, a.outputs()));
    }

    // ---- the line

    static void add(String op, Object input) {
        inputs++;
        String mir;
        try {
            mir = mirOf(input);
            // read back: the tree the shim sends
            input = readBack(input, mir);
        } catch (RuntimeException e) {
            System.out.println("  refused by MirJson: " + op + " " + e);
            return;
        }
        String kind = input.getClass().getSimpleName();
        for (String name : DIALECTS) {
            if (!SEEN.add(name + '\u0000' + op + '\u0000' + mir)) {
                continue;
            }
            String out;
            try {
                out = "{\"ok\":" + answerOf(fresh(name), op, input) + "}";
            } catch (RuntimeException e) {
                out = "{\"throws\":" + quote(e.getClass().getName()) + ",\"message\":" + quote(String.valueOf(e.getMessage()))
                        + "}";
            }
            LINES.add("{\"dialect\":" + quote(name) + ",\"config\":null,\"fresh\":true,\"op\":" + quote(op) + ",\"kind\":"
                    + quote(kind) + ",\"mir\":" + mir + ",\"answer\":" + out + "}\n");
            PER_DIALECT.merge(name, 1, Integer::sum);
        }
    }

    static SqlDialect fresh(String name) {
        try {
            return (SqlDialect) Class.forName("com.legend.sql.dialect." + name).getDeclaredConstructor().newInstance();
        } catch (ReflectiveOperationException e) {
            throw new IllegalStateException(name, e);
        }
    }

    static String mirOf(Object input) {
        Object tree = switch (input) {
            case SqlQuery q -> MirJson.query(q);
            case SqlDml d -> MirJson.dml(d);
            default -> throw new IllegalStateException("no MirJson for " + input.getClass());
        };
        return Json.toCompact(tree);
    }

    static Object readBack(Object input, String mir) {
        Json.Obj o = (Json.Obj) Json.parse(mir, DEEP);
        Object back = input instanceof SqlQuery ? MirJson.readQuery(o) : MirJson.readDml(o);
        if (!mirOf(back).equals(mir)) {
            throw new IllegalStateException("MirJson does not write the read-back tree as it read it");
        }
        return back;
    }

    static String answerOf(SqlDialect d, String op, Object input) {
        return switch (op) {
            case "query.render" -> quote(d.render((SqlQuery) input));
            case "query.renderStatement" -> statement(d.renderStatement((SqlQuery) input));
            case "dml.render" -> quote(d.render((SqlDml) input));
            case "dml.renderStatement" -> statement(d.renderStatement((SqlDml) input));
            default -> throw new IllegalStateException(op);
        };
    }

    static String statement(RenderedStatement s) {
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
