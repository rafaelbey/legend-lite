// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

import com.legend.executionplan.MirJson;
import com.legend.json.Json;
import com.legend.sql.DateFmt;
import com.legend.sql.DatePart;
import com.legend.sql.JsonKind;
import com.legend.sql.NameQuoting;
import com.legend.sql.OutputCol;
import com.legend.sql.SqlAgg;
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
import com.legend.sql.ValueKind;
import com.legend.sql.ValueTyping;
import com.legend.sql.dialect.BigQuery;
import com.legend.sql.dialect.ClickHouse;
import com.legend.sql.dialect.Databricks;
import com.legend.sql.dialect.MariaDb;
import com.legend.sql.dialect.MongoDb;
import com.legend.sql.dialect.MySql;
import com.legend.sql.dialect.RenderedStatement;
import com.legend.sql.dialect.Snowflake;
import com.legend.sql.dialect.SqlDialect;
import com.legend.sql.dialect.Trino;
import java.io.IOException;
import java.math.BigDecimal;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.HashSet;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.Set;
import java.util.function.Function;
import java.util.function.Supplier;

/**
 * The eight Java dialects left to port, over inputs that reach every arm each of them decides, rendered by Java's own
 * dialects (docs/rust-port/PHASE1.md step 6a; rust/oracles/mir/MANIFEST.tsv): {@code Snowflake}, {@code BigQuery},
 * {@code Databricks}, {@code Trino}, {@code ClickHouse}, {@code MariaDb}, {@code MySql} and {@code MongoDb}. No test
 * lane runs them, so the whole-suite dump holds 8 to 39 lines of each; this oracle holds every {@code SqlFn} arm of
 * their exhaustive switches, every {@code SqlAgg.Fn} arm, their own walls, their type names and casts, their literal
 * spellings, their DDL and DML, the features their passes rewrite, and their raw translation's refusal. Each input is
 * built with Java's own constructors (some of rust/oracles/mir/SqliteArms.java's builders, copied), written as MirJson,
 * READ BACK from that text (an input MirJson refuses or cannot write back to the same text is not a line, counted),
 * and rendered on a FRESH dialect of each class -- the tree a shim will send -- in the dumps' line format
 * ({@code {"dialect","config","fresh","op","kind","mir","answer"}}, a refusal by its class and message; a
 * {@code fromEngineText} line's mir is its input string). An answer that is a {@code ClassCastException} -- Java's own
 * defect, a failed cast inside the renderer, its message naming the JVM's class loader -- is no line either (counted
 * and listed by the run). core's dialect target is private to core, so this runs on its built jars with the build's JDK
 * (remotejdk_25), from the repository root:
 * <pre>
 *   bazel build //core:sql_dialect //core:execution_plan //core:model //json //base
 *   $(bazel info output_base)/external/rules_java++toolchains+remotejdk25_&lt;os&gt;_&lt;arch&gt;/bin/java -Xss64m \
 *     -cp bazel-bin/core/libsql_dialect.jar:bazel-bin/core/libsql.jar:bazel-bin/core/libexecution_plan.jar:\
 *   bazel-bin/core/libmodel.jar:bazel-bin/json/libjson.jar:bazel-bin/base/libbase.jar \
 *     rust/oracles/mir/DialectArms.java dialect-arms.jsonl
 *   zstd -19 dialect-arms.jsonl -o rust/oracles/mir/dialect-arms.jsonl.zst
 * </pre>
 * With a second argument, a dialects dump ({@code //rust/oracles:dialects_dump}'s text), its lines of the eight
 * dialects are read back and answered again on fresh dialects, each held to the dump's answer (nothing of them is
 * written): what rust/tools/dialect-arms-coverage.py runs, with a copy of the dialects' sources instrumented first on
 * the class path, to measure the arms the two oracles reach together (its table and the arms no input can reach, with
 * the reason, are in docs/rust-port/PHASE1.md step 6a). The sections after {@code shared()} were each written against
 * one dialect's remaining arms. No input has a tie for CommonSubexpressions' largest repeated node (Java breaks one by
 * identity-hash order), and no map or set of more than one entry is built by {@code Map.of} or {@code Set.of} (their
 * order changes between JVM runs).
 */
public final class DialectArms {

    private DialectArms() {
    }

    /** The eight dialects, each made with no arguments, in the order their lines are written per input. */
    static final Map<String, Supplier<SqlDialect>> DIALECTS = new LinkedHashMap<>();

    static {
        DIALECTS.put("Snowflake", Snowflake::new);
        DIALECTS.put("BigQuery", BigQuery::new);
        DIALECTS.put("Databricks", Databricks::new);
        DIALECTS.put("Trino", Trino::new);
        DIALECTS.put("ClickHouse", ClickHouse::new);
        DIALECTS.put("MariaDb", MariaDb::new);
        DIALECTS.put("MySql", MySql::new);
        DIALECTS.put("MongoDb", MongoDb::new);
    }

    private static final List<String> LINES = new ArrayList<>();
    private static final Set<String> SEEN = new HashSet<>();
    private static final Map<String, Integer> PER_DIALECT = new LinkedHashMap<>();
    private static int inputs;
    private static int refusedByMirJson;
    private static final List<String> REFUSED = new ArrayList<>();
    private static final List<String> DEFECTS = new ArrayList<>();
    /** A MirJson text read back with no nesting limit but the stack's (the default's 64 is a request's). */
    private static final Json.Config DEEP = new Json.Config(1 << 20);

    // ---- the table every expression reads

    static final SqlType BIGINT = SqlType.Scalar.BIGINT;
    static final SqlType INTEGER = SqlType.Scalar.INTEGER;
    static final SqlType VARCHAR = SqlType.Scalar.VARCHAR;
    static final SqlType DOUBLE = SqlType.Scalar.DOUBLE;
    static final SqlType BOOLEAN = SqlType.Scalar.BOOLEAN;
    static final SqlType DATE = SqlType.Scalar.DATE;
    static final SqlType TIMESTAMP = SqlType.Scalar.TIMESTAMP;
    static final SqlType TIMESTAMPTZ = SqlType.Scalar.TIMESTAMPTZ;
    static final SqlType TIME = SqlType.Scalar.TIME;
    static final SqlType JSON = SqlType.Scalar.JSON;
    static final SqlType HUGEINT = SqlType.Scalar.HUGEINT;
    static final SqlType TEMPORAL_TEXT = SqlType.Scalar.TEMPORAL_TEXT;
    static final SqlType DEC = new SqlType.Decimal(10, 2);
    static final SqlType DEC0 = new SqlType.Decimal(10, 0);
    static final SqlType DEC38 = new SqlType.Decimal(38, 10);
    static final SqlType STRUCT = new SqlType.Struct(List.of(new SqlType.Struct.Field("a", BIGINT),
            new SqlType.Struct.Field("b", VARCHAR)));
    static final SqlType XS = new SqlType.Array(BIGINT);
    static final SqlType SS = new SqlType.Array(VARCHAR);
    static final SqlType DS = new SqlType.Array(DOUBLE);
    static final SqlType JS = new SqlType.Array(JSON);
    static final SqlType BS = new SqlType.Array(BOOLEAN);
    static final SqlType DTS = new SqlType.Array(DATE);
    static final SqlType TSS = new SqlType.Array(TIMESTAMP);
    static final SqlType DECS = new SqlType.Array(DEC);
    static final SqlType XSS = new SqlType.Array(XS);
    static final SqlType STS = new SqlType.Array(STRUCT);
    static final SqlType MAP = new SqlType.Map(VARCHAR, BIGINT);

    static final List<OutputCol> COLUMNS = List.of(oc("s", VARCHAR), oc("n", BIGINT), oc("i", INTEGER),
            oc("d", DOUBLE), oc("b", BOOLEAN), oc("dt", DATE), oc("ts", TIMESTAMP), oc("tz", TIMESTAMPTZ),
            oc("tm", TIME), oc("j", JSON), oc("h", HUGEINT), oc("dec", DEC), oc("dec0", DEC0), oc("st", STRUCT),
            oc("xs", XS), oc("ss", SS), oc("ds", DS), oc("js", JS), oc("bs", BS), oc("dts", DTS), oc("tss", TSS),
            oc("decs", DECS), oc("xss", XSS), oc("sts", STS), oc("m", MAP), oc("s2", VARCHAR), oc("n2", BIGINT),
            oc("tt", TEMPORAL_TEXT));

    static OutputCol oc(String name, SqlType type) {
        return new OutputCol(name, type, true, OutputCol.Origin.PHYSICAL);
    }

    static SqlSource t() {
        return new SqlSource.Table("t", "t0", COLUMNS, false, Map.of(), NameQuoting.BARE);
    }

    static SqlSource t(String alias) {
        return new SqlSource.Table("t", alias, COLUMNS, false, Map.of(), NameQuoting.BARE);
    }

    static SqlExpr c(String name) {
        return SqlExpr.Column.of("t0", COLUMNS, name);
    }

    static SqlExpr c(String table, String name) {
        return SqlExpr.Column.of(table, COLUMNS, name);
    }

    static SqlExpr str(String s) {
        return new SqlExpr.StringLit(s);
    }

    static SqlExpr i(long v) {
        return new SqlExpr.IntLit(v);
    }

    static SqlExpr f(double v) {
        return new SqlExpr.FloatLit(v);
    }

    static SqlExpr dec(String v) {
        return new SqlExpr.DecimalLit(new BigDecimal(v));
    }

    static SqlExpr bool(boolean v) {
        return new SqlExpr.BoolLit(v);
    }

    static SqlExpr nul() {
        return new SqlExpr.NullLit();
    }

    static SqlExpr call(SqlFn fn, SqlExpr... args) {
        return SqlExpr.Call.of(fn, args);
    }

    static SqlExpr part(DatePart p) {
        return new SqlExpr.DatePartLit(p);
    }

    static SqlExpr arr(SqlExpr... elements) {
        return new SqlExpr.ArrayLit(List.of(elements));
    }

    static SqlExpr cast(SqlExpr v, SqlType t) {
        return new SqlExpr.Cast(v, t);
    }

    static SqlExpr lambda(SqlExpr body, String... params) {
        return new SqlExpr.Lambda(List.of(params), body);
    }

    /** A one-parameter lambda over {@code coll}'s elements, its parameter typed as the element (Lambda.bind). */
    static SqlExpr lam(SqlExpr coll, String p, Function<SqlExpr, SqlExpr> body) {
        return new SqlExpr.Lambda(List.of(p), body.apply(SqlExpr.Column.param(p, coll)));
    }

    static SqlExpr derived(String name) {
        return SqlExpr.Column.derived(null, name);
    }

    static SqlExpr fmt(DateFmt... parts) {
        return new SqlExpr.FormatLit(List.of(parts));
    }

    static SqlExpr reducer(SqlAgg.Fn fn, SqlExpr... args) {
        return new SqlAgg.Reducer(fn, List.of(args), false, List.of());
    }

    /** A map in the order its entries are written (Map.of's order changes between JVM runs). */
    static <V> Map<String, V> ordered(Object... kv) {
        Map<String, V> out = new LinkedHashMap<>();
        for (int k = 0; k < kv.length; k += 2) {
            @SuppressWarnings("unchecked")
            V v = (V) kv[k + 1];
            out.put((String) kv[k], v);
        }
        return out;
    }

    static SqlSelect.SortKey asc(SqlExpr e) {
        return new SqlSelect.SortKey(e, true, null, null);
    }

    static SqlSelect.SortKey desc(SqlExpr e) {
        return new SqlSelect.SortKey(e, false, null, null);
    }

    /** A projection labelled {@code alias}: its output declared as its expression's type when it has one. */
    static SqlSelect.Projection p(SqlExpr e, String alias) {
        return e.type() instanceof TypeFact.Typed t
                ? new SqlSelect.Projection(e, alias, new OutputCol(alias, t.type(), true))
                : new SqlSelect.Projection(e, alias, null);
    }

    static SqlSelect.Projection p(SqlExpr e, String alias, SqlType out) {
        return new SqlSelect.Projection(e, alias, new OutputCol(alias, out, true));
    }

    static SqlSelect select(SqlSource from, List<SqlSelect.Projection> ps, SqlExpr where, List<SqlExpr> groupBy,
            List<SqlSelect.SortKey> orderBy, Long limit, Long offset) {
        return new SqlSelect(ps, false, from, where, groupBy, null, null, orderBy, limit, offset, List.of());
    }

    static SqlSelect select(SqlSource from, SqlSelect.Projection... ps) {
        return select(from, List.of(ps), null, List.of(), List.of(), null, null);
    }

    static SqlSelect where(SqlExpr w, SqlSelect.Projection... ps) {
        return select(t(), List.of(ps), w, List.of(), List.of(), null, null);
    }

    static SqlSelect grouped(SqlExpr agg) {
        return select(t(), List.of(p(c("s"), "s"), p(agg, "v")), null, List.of(c("s")), List.of(), null, null);
    }

    static SqlSelect marker(SqlSelect.Projection... unnests) {
        return select(new SqlSource.Dual(), unnests);
    }

    static SqlSource sub(SqlQuery q, String alias) {
        return new SqlSource.Subselect(q, alias, null);
    }

    // ---- the cases

    /** One expression, projected as {@code v} over the table, as a statement. */
    static void e(SqlExpr x) {
        q(select(t(), p(x, "v")));
    }

    /** One expression, projected as {@code v} over the table, as text (its literals inline). */
    static void et(SqlExpr x) {
        text(select(t(), p(x, "v")));
    }

    /** One condition, as a WHERE over the table. */
    static void w(SqlExpr cond) {
        q(where(cond, p(c("n"), "n")));
    }

    static void q(SqlQuery q) {
        add("query.renderStatement", q);
    }

    static void text(SqlQuery q) {
        add("query.render", q);
    }

    public static void main(String[] args) throws IOException {
        if (args.length > 1) {
            replay(Path.of(args[1]));
        }
        functions();
        regularExpressions();
        lists();
        maps();
        variants();
        temporal();
        formats();
        literals();
        casts();
        aggregates();
        windows();
        collections();
        structure();
        subqueries();
        joins();
        pivots();
        parameters();
        checked();
        commonSubexpressions();
        statements();
        engineText();
        shared();
        refusedByMirJson();
        mongoArms();
        mariaArms();
        snowflakeBigQueryArms();
        databricksTrinoArms();
        clickHouseArms();
        Files.writeString(Path.of(args[0]), String.join("", LINES), StandardCharsets.UTF_8);
        System.out.println(inputs + " inputs, " + LINES.size() + " lines " + PER_DIALECT + "; " + refusedByMirJson
                + " inputs MirJson refuses (java " + Runtime.version() + ")");
        for (String r : REFUSED) {
            System.out.println("  refused by MirJson: " + r);
        }
        System.out.println(DEFECTS.size() + " answers dropped, a ClassCastException naming the JVM's class loader:");
        for (String r : DEFECTS) {
            System.out.println("  " + r);
        }
    }

    // ---- every function, over operands of its natural types

    static void functions() {
        // the ANSI operators, over each operand type
        for (SqlFn fn : List.of(SqlFn.EQUAL, SqlFn.NOT_EQUAL, SqlFn.LESS, SqlFn.LESS_EQUAL, SqlFn.GREATER,
                SqlFn.GREATER_EQUAL)) {
            w(call(fn, c("n"), i(1)));
            w(call(fn, c("s"), c("s2")));
            w(call(fn, c("dt"), new SqlExpr.DateLit("2024-01-02")));
            w(call(fn, c("ts"), c("tz")));
            w(call(fn, c("j"), c("j")));
            w(call(fn, c("b"), bool(true)));
            w(call(fn, c("dec"), c("d")));
            w(call(fn, c("xs"), c("xs")));
        }
        w(call(SqlFn.AND, c("b"), call(SqlFn.OR, c("b"), call(SqlFn.NOT, c("b")))));
        w(call(SqlFn.OR, call(SqlFn.AND, c("b"), c("b")), call(SqlFn.NOT, call(SqlFn.IS_NULL, c("s")))));
        e(call(SqlFn.AND, c("b"), c("b")));
        e(call(SqlFn.NOT, call(SqlFn.EQUAL, c("n"), i(2))));
        for (SqlFn fn : List.of(SqlFn.PLUS, SqlFn.MINUS, SqlFn.TIMES, SqlFn.DIVIDE, SqlFn.MOD, SqlFn.REM,
                SqlFn.INT_DIVIDE)) {
            e(call(fn, c("n"), i(3)));
            e(call(fn, c("d"), c("d")));
            e(call(fn, c("dec"), c("dec")));
            e(call(fn, c("n"), c("d")));
            e(call(fn, c("i"), c("h")));
            e(call(fn, call(SqlFn.PLUS, c("n"), i(1)), call(SqlFn.MINUS, c("n"), call(SqlFn.TIMES, c("n"), i(2)))));
        }
        e(call(SqlFn.PLUS, c("s"), c("s2")));
        e(call(SqlFn.MINUS, c("ts"), c("ts")));
        e(call(SqlFn.MINUS, c("dt"), c("dt")));
        e(call(SqlFn.DIVIDE, i(1), i(0)));
        e(call(SqlFn.NEGATE, c("n")));
        e(call(SqlFn.NEGATE, c("d")));
        e(call(SqlFn.NEGATE, call(SqlFn.NEGATE, i(5))));
        e(call(SqlFn.ABS, c("n")));
        e(call(SqlFn.ABS, c("d")));
        e(call(SqlFn.ABS, c("dec")));
        w(call(SqlFn.IS_NULL, c("s")));
        w(call(SqlFn.IS_NOT_NULL, c("j")));
        e(call(SqlFn.IS_NULL, c("xs")));
        e(call(SqlFn.IS_NOT_NULL, c("st")));
        w(call(SqlFn.IN, c("n"), i(1), i(2), i(3)));
        w(call(SqlFn.IN, c("s"), str("a"), str("b")));
        w(call(SqlFn.IN, c("dt"), new SqlExpr.DateLit("2024-01-02")));
        e(call(SqlFn.COALESCE, c("s"), str("x")));
        e(call(SqlFn.COALESCE, c("n"), c("n2"), i(0)));
        e(call(SqlFn.COALESCE, c("j"), c("s")));
        e(call(SqlFn.COALESCE, c("s"), c("j")));
        e(call(SqlFn.COALESCE, c("xs"), arr()));
        e(call(SqlFn.COALESCE, c("d"), c("n")));
        e(call(SqlFn.PARSE_INT, c("s")));
        e(call(SqlFn.PARSE_INT, str("42")));
        e(call(SqlFn.CONCAT, c("s"), str("x")));
        e(call(SqlFn.CONCAT, c("s"), call(SqlFn.CONCAT, str("a"), c("s2"))));
        e(call(SqlFn.CONCAT, str("a"), c("n")));
        e(call(SqlFn.CONCAT, c("j"), c("s")));
        e(call(SqlFn.CONCAT_JOIN, c("s"), c("n"), c("d"), c("b"), dec("2.50")));
        e(call(SqlFn.CONCAT_JOIN, c("s"), c("s2")));
        for (SqlFn fn : List.of(SqlFn.LENGTH, SqlFn.UPPER, SqlFn.LOWER, SqlFn.TRIM, SqlFn.LTRIM, SqlFn.RTRIM,
                SqlFn.REVERSE_STRING, SqlFn.ASCII_CODE, SqlFn.UC_FIRST, SqlFn.LC_FIRST, SqlFn.ENCODE_BASE64,
                SqlFn.DECODE_BASE64, SqlFn.MD5, SqlFn.SHA1, SqlFn.SHA256, SqlFn.HASH)) {
            e(call(fn, c("s")));
        }
        e(call(SqlFn.HASH, c("n")));
        e(call(SqlFn.HASH, c("xs")));
        e(call(SqlFn.LENGTH, c("xs")));
        e(call(SqlFn.TRIM, c("s"), str("x")));
        e(call(SqlFn.LTRIM, c("s"), str("x")));
        e(call(SqlFn.RTRIM, c("s"), str("x")));
        for (SqlFn fn : List.of(SqlFn.GREATEST, SqlFn.LEAST)) {
            e(call(fn, c("n"), i(1)));
            e(call(fn, c("d"), c("d"), f(1.5)));
            e(call(fn, c("s"), c("s2")));
            e(call(fn, c("dt"), new SqlExpr.DateLit("2024-01-02")));
            e(call(fn, c("ts"), c("ts")));
            e(call(fn, c("dec"), c("dec")));
            e(call(fn, c("b"), c("b")));
        }
        for (SqlFn fn : List.of(SqlFn.SQRT, SqlFn.CBRT, SqlFn.EXP, SqlFn.LN, SqlFn.LOG10, SqlFn.SIN, SqlFn.COS,
                SqlFn.TAN, SqlFn.ASIN, SqlFn.ACOS, SqlFn.ATAN, SqlFn.SINH, SqlFn.COSH, SqlFn.TANH, SqlFn.COT,
                SqlFn.RADIANS, SqlFn.DEGREES, SqlFn.CEILING, SqlFn.FLOOR, SqlFn.SIGN, SqlFn.FLOOR_RAW, SqlFn.ROUND)) {
            e(call(fn, c("d")));
            e(call(fn, c("n")));
        }
        e(call(SqlFn.CEILING, c("dec")));
        e(call(SqlFn.FLOOR, c("dec")));
        e(call(SqlFn.SIGN, c("dec")));
        e(call(SqlFn.ROUND, c("dec")));
        e(call(SqlFn.ROUND, c("d"), i(2)));
        e(call(SqlFn.ROUND, c("dec"), i(1)));
        e(call(SqlFn.ROUND, c("n"), i(-1)));
        e(call(SqlFn.ROUND_HALF_UP, c("d"), i(2)));
        e(call(SqlFn.ROUND_HALF_UP, c("dec"), i(1)));
        e(call(SqlFn.ROUND_HALF_UP, c("n")));
        e(call(SqlFn.ROUND_HALF_UP, c("d")));
        e(call(SqlFn.ROUND_HALF_UP, call(SqlFn.DIVIDE, c("n"), i(3)), i(2)));
        e(call(SqlFn.ATAN2, c("d"), c("n")));
        e(call(SqlFn.POW, c("d"), i(2)));
        e(call(SqlFn.POW, c("n"), c("n")));
        e(call(SqlFn.PI));
        e(call(SqlFn.XOR, c("b"), call(SqlFn.EQUAL, c("n"), i(1))));
        for (SqlFn fn : List.of(SqlFn.BIT_AND, SqlFn.BIT_OR, SqlFn.BIT_XOR, SqlFn.BIT_SHIFT_LEFT,
                SqlFn.BIT_SHIFT_RIGHT)) {
            e(call(fn, c("n"), i(2)));
            e(call(fn, c("i"), c("n")));
        }
        e(call(SqlFn.BIT_NOT, c("n")));
        e(call(SqlFn.BIT_NOT, c("i")));
        e(call(SqlFn.SUBSTRING, c("s"), i(2)));
        e(call(SqlFn.SUBSTRING, c("s"), i(2), i(3)));
        e(call(SqlFn.SUBSTRING, c("s"), c("n"), c("n2")));
        e(call(SqlFn.SUBSTRING, c("s"), i(0), i(3)));
        e(call(SqlFn.STRPOS, c("s"), str("a")));
        e(call(SqlFn.STARTS_WITH, c("s"), str("a")));
        e(call(SqlFn.ENDS_WITH, c("s"), c("s2")));
        e(call(SqlFn.LEFT, c("s"), i(2)));
        e(call(SqlFn.RIGHT, c("s"), c("n")));
        e(call(SqlFn.LPAD, c("s"), i(5), str("x")));
        e(call(SqlFn.LPAD, c("s"), i(5)));
        e(call(SqlFn.RPAD, c("s"), c("n"), str("xy")));
        e(call(SqlFn.RPAD, c("s"), i(5)));
        e(call(SqlFn.REPLACE, c("s"), str("a"), str("b")));
        e(call(SqlFn.SPLIT, c("s"), str(",")));
        e(call(SqlFn.SPLIT, c("s"), c("s2")));
        e(call(SqlFn.SPLIT_PART, c("s"), str(","), i(2)));
        e(call(SqlFn.SPLIT_PART, c("s"), str(","), c("n")));
        e(call(SqlFn.PURE_SPLIT_PART, c("s"), str(","), i(1)));
        e(call(SqlFn.PURE_SPLIT_PART, c("s"), c("s2"), c("n")));
        e(call(SqlFn.TRANSLATE, c("s"), str("ab"), str("cd")));
        e(call(SqlFn.CHR, c("n")));
        e(call(SqlFn.LEVENSHTEIN, c("s"), c("s2")));
        e(call(SqlFn.JARO_WINKLER, c("s"), c("s2")));
        e(call(SqlFn.REPEAT_STR, c("s"), i(3)));
        e(call(SqlFn.REPEAT_STR, c("s"), c("n")));
        e(call(SqlFn.GUID));
        e(call(SqlFn.RANDOM));
        e(call(SqlFn.CURRENT_USER_FN));
        e(call(SqlFn.FORMAT, str("%s is %d, %05.2f%% and %s"), c("s"), c("n"), c("d"), c("b"), c("ts")));
        e(call(SqlFn.FORMAT, str("%s"), c("s")));
        e(call(SqlFn.FORMAT, str("%d|%5d|%-5d|%x"), c("n"), c("n"), c("n"), c("n")));
        e(call(SqlFn.FORMAT, str("%.3f|%e|%10.1f"), c("d"), c("d"), c("d")));
        e(call(SqlFn.FORMAT, str("%t|%s"), c("dt"), c("j")));
        e(call(SqlFn.FORMAT, c("s"), c("n")));
        e(call(SqlFn.FORMAT, str("no conversions")));
        e(call(SqlFn.ERROR, str("boom")));
        e(call(SqlFn.ERROR, call(SqlFn.CONCAT, str("bad: "), c("s"))));
        e(new SqlExpr.Case(List.of(new SqlExpr.Case.When(c("b"), c("n"))), call(SqlFn.ERROR, str("no"))));
        e(new SqlExpr.Case(List.of(new SqlExpr.Case.When(c("b"), c("s"))), call(SqlFn.ERROR, str("no"))));
        e(new SqlExpr.Case(List.of(new SqlExpr.Case.When(c("b"), c("dt"))), call(SqlFn.ERROR, str("no"))));
        e(new SqlExpr.Case(List.of(new SqlExpr.Case.When(c("b"), c("d")), new SqlExpr.Case.When(
                call(SqlFn.IS_NULL, c("s")), c("dec"))), call(SqlFn.ERROR, str("no"))));
        e(new SqlExpr.Case(List.of(new SqlExpr.Case.When(c("b"), c("xs"))), call(SqlFn.ERROR, str("no"))));
        e(new SqlExpr.Case(List.of(new SqlExpr.Case.When(c("b"), c("j"))), call(SqlFn.ERROR, str("no"))));
        e(new SqlExpr.Case(List.of(new SqlExpr.Case.When(c("b"), c("ts"))), call(SqlFn.ERROR, str("no"))));
        e(new SqlExpr.Case(List.of(new SqlExpr.Case.When(c("b"), c("b"))), call(SqlFn.ERROR, str("no"))));
        e(new SqlExpr.Case(List.of(new SqlExpr.Case.When(c("b"), c("st"))), call(SqlFn.ERROR, str("no"))));
        e(new SqlExpr.Case(List.of(new SqlExpr.Case.When(c("b"), c("tm"))), call(SqlFn.ERROR, str("no"))));
        e(new SqlExpr.Case(List.of(new SqlExpr.Case.When(c("b"), c("m"))), call(SqlFn.ERROR, str("no"))));
        e(new SqlExpr.Case(List.of(new SqlExpr.Case.When(c("b"), c("h"))), call(SqlFn.ERROR, str("no"))));
        e(new SqlExpr.Case(List.of(new SqlExpr.Case.When(c("b"), c("i"))), call(SqlFn.ERROR, str("no"))));
        e(new SqlExpr.Case(List.of(new SqlExpr.Case.When(c("b"), c("tz"))), call(SqlFn.ERROR, str("no"))));
        e(call(SqlFn.PLUS, call(SqlFn.ERROR, str("x")), i(1)));
        e(new SqlExpr.Case(List.of(new SqlExpr.Case.When(c("b"), str("a")), new SqlExpr.Case.When(
                call(SqlFn.GREATER, c("n"), i(0)), str("b"))), null));
        e(new SqlExpr.Case(List.of(new SqlExpr.Case.When(c("b"), c("n"))), c("d")));
        e(call(SqlFn.TO_VARIANT, c("n")));
        e(call(SqlFn.TO_VARIANT, c("s")));
        e(call(SqlFn.TO_VARIANT, c("xs")));
        e(call(SqlFn.TO_VARIANT, c("st")));
        e(call(SqlFn.TO_VARIANT, c("dt")));
        e(call(SqlFn.TO_VARIANT, c("ts")));
        e(call(SqlFn.TO_VARIANT, c("d")));
        e(call(SqlFn.TO_VARIANT, c("b")));
        e(call(SqlFn.TO_VARIANT, c("dec")));
        e(call(SqlFn.TO_VARIANT, c("j")));
        e(call(SqlFn.TO_VARIANT, c("m")));
        e(call(SqlFn.TO_VARIANT, arr()));
        e(call(SqlFn.TO_VARIANT, nul()));
        e(call(SqlFn.TO_VARIANT, c("sts")));
        e(call(SqlFn.TO_VARIANT, c("n"), c("s")));
        e(call(SqlFn.BOOL_TO_TEXT, c("b")));
        e(call(SqlFn.FLOAT_CANON, c("d")));
        e(call(SqlFn.FLOAT_CANON, f(1.5)));
        e(call(SqlFn.FLOAT_CANON, c("dec")));
        e(call(SqlFn.FLOAT_CANON, c("n")));
        e(call(SqlFn.FLOAT_PRINT, c("d")));
        e(call(SqlFn.FLOAT_PRINT, f(1.0E20)));
        e(call(SqlFn.FLOAT_PRINT, c("dec")));
        e(call(SqlFn.FLOAT_PRINT, c("n")));
        e(call(SqlFn.FLOAT_PRINT, cast(c("s"), DOUBLE)));
        e(call(SqlFn.TYPEOF, c("n")));
        e(call(SqlFn.TYPEOF, c("j")));
        w(call(SqlFn.IS_DISTINCT_FROM, c("n"), c("n2")));
        w(call(SqlFn.NULL_SAFE_EQUAL, c("s"), c("s2")));
        w(call(SqlFn.NULL_SAFE_NOT_EQUAL, c("s"), str("a")));
        e(call(SqlFn.NULL_SAFE_EQUAL, c("n"), nul()));
        e(call(SqlFn.IS_DISTINCT_FROM, c("j"), c("j")));
        e(call(SqlFn.NULL_SAFE_EQUAL, c("xs"), c("xs")));
        e(call(SqlFn.NULL_SAFE_NOT_EQUAL, c("dt"), c("dt")));
        e(call(SqlFn.STRUCT_INSERT, c("st"), str("c"), c("n")));
        e(call(SqlFn.STRUCT_INSERT, c("st"), str("c"), new SqlExpr.JsonObject(List.of(str("k"), c("n")))));
        e(call(SqlFn.STRUCT_INSERT, c("j"), str("c"), c("n")));
        e(call(SqlFn.STRUCT_INSERT, c("st"), c("s"), c("n")));
        e(call(SqlFn.JSON_MERGE_PATCH, c("j"), c("j")));
        e(call(SqlFn.JSON_MERGE_PATCH, c("j"), new SqlExpr.JsonObject(List.of(str("a"), i(1), str("b"),
                str("x")))));
        e(call(SqlFn.JSON_MERGE_PATCH, c("j"), new SqlExpr.JsonObject(List.of(str("a"), nul()))));
    }

    static void regularExpressions() {
        e(call(SqlFn.REGEXP_EXTRACT, c("s"), str("a(b+)c"), i(1)));
        e(call(SqlFn.REGEXP_EXTRACT, c("s"), str("[0-9]+")));
        e(call(SqlFn.REGEXP_EXTRACT, c("s"), str("[0-9]+"), i(0)));
        e(call(SqlFn.REGEXP_EXTRACT, c("s"), str("(?i)x|y*?z")));
        e(call(SqlFn.REGEXP_EXTRACT, c("s"), str("a(b)"), c("n")));
        e(call(SqlFn.REGEXP_EXTRACT, c("s"), c("s2")));
        e(call(SqlFn.REGEXP_EXTRACT, c("s"), str("a(b)"), i(2)));
        e(call(SqlFn.REGEXP_EXTRACT, c("s"), str("(a)(b)(c)"), i(3)));
        e(call(SqlFn.REGEXP_EXTRACT, c("s"), str("(?:a)(b)"), i(1)));
        e(call(SqlFn.REGEXP_EXTRACT, c("s"), str("\\(a\\)(b)"), i(1)));
        e(call(SqlFn.REGEXP_EXTRACT, c("s"), str("[(]a(b)"), i(1)));
        e(call(SqlFn.REGEXP_EXTRACT_ALL, c("s"), str("\\d+")));
        e(call(SqlFn.REGEXP_EXTRACT_ALL, c("s"), str("x(y)"), i(1)));
        e(call(SqlFn.REGEXP_EXTRACT_ALL, c("s"), str("x(y)"), i(0)));
        e(call(SqlFn.REGEXP_EXTRACT_ALL, c("s"), c("s2")));
        e(call(SqlFn.REGEXP_REPLACE, c("s"), str("a(b)"), str("X\\1Y\\0\\\\")));
        e(call(SqlFn.REGEXP_REPLACE, c("s"), str("a(b)(c)"), str("\\2-\\1"), str("g")));
        e(call(SqlFn.REGEXP_REPLACE, c("s"), str("ab"), str(""), str("gi")));
        e(call(SqlFn.REGEXP_REPLACE, c("s"), str("ab"), str("z"), str("i")));
        e(call(SqlFn.REGEXP_REPLACE, c("s"), str("ab"), str("z"), str("q")));
        e(call(SqlFn.REGEXP_REPLACE, c("s"), str("a$b"), str("$1 $$")));
        e(call(SqlFn.REGEXP_REPLACE, c("s"), c("s2"), str("z")));
        e(call(SqlFn.REGEXP_REPLACE, c("s"), str("a"), c("s2")));
        e(call(SqlFn.REGEXP_REPLACE, c("s"), str("a"), str("b"), c("s2")));
        e(call(SqlFn.MATCHES, c("s"), str("^a[^b-d\\s]*\\.$")));
        e(call(SqlFn.MATCHES, c("s"), str("(?ms)^x.y$")));
        e(call(SqlFn.MATCHES, c("s"), str("a"), str("i")));
        e(call(SqlFn.MATCHES, c("s"), c("s2")));
        e(call(SqlFn.REGEXP_FULL_MATCH, c("s"), str("(?:ab|cd){2,}(e)?")));
        e(call(SqlFn.REGEXP_FULL_MATCH, c("s"), str("a\\b")));
        e(call(SqlFn.REGEXP_FULL_MATCH, c("s"), c("s2")));
        e(call(SqlFn.REGEXP_FULL_MATCH, c("s"), str("\\p{L}+\\Q.*\\E\\x41\\u0042")));
        w(call(SqlFn.MATCHES, c("s"), str("[a-z]+")));
    }

    static void lists() {
        SqlExpr xs = c("xs");
        e(call(SqlFn.LIST_FILTER, xs, lam(xs, "x", x -> call(SqlFn.GREATER, x, i(1)))));
        e(call(SqlFn.LIST_FILTER, c("ss"), lam(c("ss"), "x", x -> call(SqlFn.STARTS_WITH, x, str("a")))));
        e(call(SqlFn.LIST_FILTER, c("sts"), lam(c("sts"), "x", x -> call(SqlFn.GREATER,
                new SqlExpr.StructGet(x, "a"), i(1)))));
        e(call(SqlFn.LIST_FILTER, c("js"), lam(c("js"), "x", x -> call(SqlFn.IS_NOT_NULL, x))));
        e(call(SqlFn.LIST_FILTER, xs, lam(xs, "x", x -> call(SqlFn.GREATER, x, c("n")))));
        e(call(SqlFn.LIST_FILTER, xs, lambda(call(SqlFn.GREATER, derived("x"), derived("i")), "x", "i")));
        e(call(SqlFn.LIST_FILTER, arr(i(1), i(2), i(3)), lam(arr(i(1), i(2), i(3)), "x",
                x -> call(SqlFn.GREATER, x, i(1)))));
        e(call(SqlFn.LIST_TRANSFORM, xs, lam(xs, "x", x -> call(SqlFn.TIMES, x, i(2)))));
        e(call(SqlFn.LIST_TRANSFORM, xs, lam(xs, "x", x -> call(SqlFn.PLUS, x, c("n")))));
        e(call(SqlFn.LIST_TRANSFORM, c("ss"), lam(c("ss"), "x", x -> call(SqlFn.UPPER, x))));
        e(call(SqlFn.LIST_TRANSFORM, c("sts"), lam(c("sts"), "x", x -> new SqlExpr.StructGet(x, "b"))));
        e(call(SqlFn.LIST_TRANSFORM, xs, lam(xs, "x", x -> new SqlExpr.StructLit(List.of(
                new SqlExpr.StructLit.Field("k", x), new SqlExpr.StructLit.Field("v", c("s")))))));
        e(call(SqlFn.LIST_TRANSFORM, xs, lambda(call(SqlFn.PLUS, derived("x"), derived("i")), "x", "i")));
        // no lambda: a wall by name on BigQuery, MariaDB and MySQL (elsewhere Java's ClassCastException, no line)
        e(call(SqlFn.LIST_TRANSFORM, xs, c("n")));
        e(call(SqlFn.LIST_TRANSFORM, c("xss"), lam(c("xss"), "x", x -> call(SqlFn.LIST_LENGTH, x))));
        e(call(SqlFn.LIST_TRANSFORM, xs, lam(xs, "x", x -> call(SqlFn.LIST_TRANSFORM, c("ss"),
                lam(c("ss"), "y", y -> call(SqlFn.CONCAT, y, cast(x, VARCHAR)))))));
        e(call(SqlFn.LIST_TRANSFORM, call(SqlFn.RANGE_FN, i(0), c("n")), lam(call(SqlFn.RANGE_FN, i(0), c("n")),
                "x", x -> call(SqlFn.TIMES, x, x))));
        e(call(SqlFn.LIST_EXISTS, xs, lam(xs, "x", x -> call(SqlFn.EQUAL, x, i(1)))));
        e(call(SqlFn.LIST_EXISTS, c("ss"), lam(c("ss"), "x", x -> call(SqlFn.EQUAL, x, c("s")))));
        e(call(SqlFn.LIST_FOR_ALL, xs, lam(xs, "x", x -> call(SqlFn.GREATER, x, i(0)))));
        e(call(SqlFn.LIST_FOR_ALL, c("sts"), lam(c("sts"), "x", x -> call(SqlFn.IS_NOT_NULL,
                new SqlExpr.StructGet(x, "b")))));
        e(call(SqlFn.LIST_CONCAT, xs, xs));
        e(call(SqlFn.LIST_CONCAT, c("ss"), arr(str("a"))));
        e(call(SqlFn.LIST_CONCAT, nul(), arr(i(1))));
        e(call(SqlFn.LIST_CONCAT, c("js"), c("js")));
        e(call(SqlFn.LIST_CONCAT, c("sts"), c("sts")));
        e(call(SqlFn.LIST_GET, xs, i(1)));
        e(call(SqlFn.LIST_GET, xs, c("n")));
        e(call(SqlFn.LIST_GET, xs, i(-1)));
        e(call(SqlFn.LIST_GET, c("ss"), i(0)));
        e(call(SqlFn.LIST_GET, c("js"), i(2)));
        e(call(SqlFn.LIST_GET, c("sts"), i(1)));
        e(call(SqlFn.LIST_GET, c("xss"), i(1)));
        e(call(SqlFn.LIST_GET, arr(i(1), i(2)), c("n")));
        e(call(SqlFn.LIST_POSITION, xs, c("n")));
        e(call(SqlFn.LIST_POSITION, c("ss"), str("a")));
        e(call(SqlFn.LIST_FLATTEN, c("xss")));
        e(call(SqlFn.LIST_FLATTEN, call(SqlFn.LIST_TRANSFORM, xs, lam(xs, "x", x -> arr(x, x)))));
        e(call(SqlFn.LIST_LENGTH, xs));
        e(call(SqlFn.LIST_LENGTH, c("js")));
        e(call(SqlFn.LIST_LENGTH, arr()));
        e(call(SqlFn.LIST_ZIP, xs, c("ss")));
        e(call(SqlFn.LIST_ZIP, xs, xs, c("ds")));
        for (SqlFn fn : List.of(SqlFn.LIST_DISTINCT, SqlFn.LIST_SUM, SqlFn.LIST_MIN, SqlFn.LIST_MAX, SqlFn.LIST_AVG,
                SqlFn.LIST_MEDIAN, SqlFn.LIST_MODE, SqlFn.LIST_SORT, SqlFn.LIST_SORT_DESC, SqlFn.LIST_TAIL,
                SqlFn.LIST_INIT, SqlFn.LIST_PRODUCT, SqlFn.LIST_REVERSE, SqlFn.ALL_DISTINCT)) {
            e(call(fn, xs));
            e(call(fn, c("ds")));
            e(call(fn, c("ss")));
            e(call(fn, c("decs")));
            e(call(fn, c("dts")));
        }
        e(call(SqlFn.LIST_SORT, c("sts")));
        e(call(SqlFn.LIST_SORT_DESC, c("sts")));
        e(call(SqlFn.LIST_SORT, c("js")));
        e(call(SqlFn.LIST_SORT_DESC, c("js")));
        e(call(SqlFn.LIST_DISTINCT, c("js")));
        e(call(SqlFn.LIST_REVERSE, c("js")));
        e(call(SqlFn.LIST_TAIL, c("js")));
        e(call(SqlFn.LIST_MIN, c("tss")));
        e(call(SqlFn.LIST_MAX, c("bs")));
        for (SqlFn fn : List.of(SqlFn.LIST_BOOL_AND, SqlFn.LIST_BOOL_OR)) {
            e(call(fn, c("bs")));
            e(call(fn, arr(bool(true), bool(false))));
        }
        e(call(SqlFn.LIST_APPEND, xs, c("n")));
        e(call(SqlFn.LIST_APPEND, c("ss"), str("z")));
        e(call(SqlFn.LIST_APPEND, c("js"), c("j")));
        e(call(SqlFn.LIST_APPEND, arr(), i(1)));
        e(call(SqlFn.RANGE_FN, c("n")));
        e(call(SqlFn.RANGE_FN, i(1), c("n")));
        e(call(SqlFn.RANGE_FN, i(1), c("n"), i(2)));
        e(call(SqlFn.RANGE_FN, i(10), i(1), i(-3)));
        e(call(SqlFn.LIST_SLICE, xs, i(1), i(2)));
        e(call(SqlFn.LIST_SLICE, xs, i(2), call(SqlFn.LIST_LENGTH, xs)));
        e(call(SqlFn.LIST_SLICE, c("ss"), c("n"), c("n2")));
        e(call(SqlFn.LIST_SLICE, c("js"), i(1), i(3)));
        e(call(SqlFn.LIST_REDUCE, xs, lambda(call(SqlFn.PLUS, derived("acc"), derived("x")), "acc", "x")));
        e(call(SqlFn.LIST_REDUCE, xs, lambda(derived("x"), "x")));
        e(call(SqlFn.LIST_REDUCE, xs, lambda(call(SqlFn.PLUS, derived("acc"), derived("x")), "acc", "x"), i(0)));
        e(call(SqlFn.LIST_REDUCE, c("ss"), lambda(call(SqlFn.CONCAT, derived("acc"), derived("x")), "acc", "x")));
        e(call(SqlFn.LIST_REDUCE, c("xss"), lambda(call(SqlFn.LIST_CONCAT, derived("acc"), derived("x")), "acc",
                "x")));
        e(call(SqlFn.REPEAT_VALUE, c("s"), i(3)));
        e(call(SqlFn.REPEAT_VALUE, c("n"), c("n2")));
        e(call(SqlFn.REPEAT_VALUE, c("j"), i(2)));
        e(call(SqlFn.TYPEOF, c("xs")));
        e(call(SqlFn.UNNEST, c("xs")));
        e(call(SqlFn.UNNEST, c("ss")));
        e(call(SqlFn.UNNEST, c("js")));
        e(call(SqlFn.UNNEST, c("sts")));
        e(call(SqlFn.UNNEST, arr(i(1), i(2))));
        e(call(SqlFn.UNNEST, call(SqlFn.RANGE_FN, i(0), c("n"))));
        e(new SqlExpr.Membership(c("n"), c("xs")));
        e(new SqlExpr.Membership(c("s"), c("ss")));
        e(new SqlExpr.Membership(c("n"), arr(i(1), i(2), nul())));
        e(new SqlExpr.Membership(c("n"), arr(i(1), i(2), i(3), i(4), i(5), i(6), i(7), i(8), i(9), i(10), i(11),
                i(12))));
        e(new SqlExpr.Membership(c("j"), c("js")));
        w(new SqlExpr.Membership(c("n"), c("xs")));
        e(new SqlExpr.CompactList(c("xs")));
        e(new SqlExpr.CompactList(arr(i(1), nul(), i(3))));
        e(new SqlExpr.CompactList(c("js")));
        e(call(SqlFn.VARIANT_ELEMENTS, c("j")));
    }

    static void maps() {
        e(call(SqlFn.MAP_FROM_LISTS, c("ss"), c("xs")));
        e(call(SqlFn.MAP_FROM_LISTS, arr(str("a"), str("b")), arr(i(1), i(2))));
        e(call(SqlFn.MAP_FROM_ENTRIES, c("sts")));
        e(call(SqlFn.MAP_FROM_ENTRIES, arr(new SqlExpr.StructLit(List.of(new SqlExpr.StructLit.Field("k", str("a")),
                new SqlExpr.StructLit.Field("v", i(1)))))));
        e(call(SqlFn.MAP_FROM_ENTRIES, c("xs")));
        e(call(SqlFn.MAP_EMPTY));
        e(call(SqlFn.MAP_EXTRACT, c("m"), str("k")));
        e(call(SqlFn.MAP_EXTRACT, c("m"), c("s")));
        e(call(SqlFn.MAP_KEYS, c("m")));
        e(call(SqlFn.MAP_VALUES, c("m")));
        e(call(SqlFn.MAP_CONCAT, c("m"), c("m")));
        e(call(SqlFn.MAP_CONCAT, c("m"), c("m"), c("m")));
        e(call(SqlFn.LIST_LENGTH, call(SqlFn.MAP_KEYS, c("m"))));
    }

    static void variants() {
        e(call(SqlFn.VARIANT_GET, c("j"), str("k")));
        e(call(SqlFn.VARIANT_GET, c("j"), i(0)));
        e(call(SqlFn.VARIANT_GET, c("j"), c("n")));
        e(call(SqlFn.VARIANT_GET, c("j"), c("s")));
        e(call(SqlFn.VARIANT_GET, c("j"), str("$")));
        e(call(SqlFn.VARIANT_GET, c("j"), str("a.b")));
        e(call(SqlFn.VARIANT_GET, c("j"), str("it's")));
        e(call(SqlFn.VARIANT_GET, call(SqlFn.VARIANT_GET, c("j"), str("a")), i(1)));
        e(call(SqlFn.VARIANT_GET, c("st"), str("a")));
        e(call(SqlFn.VARIANT_ELEMENTS, c("j")));
        e(call(SqlFn.VARIANT_ELEMENTS, call(SqlFn.VARIANT_GET, c("j"), str("xs"))));
        e(call(SqlFn.LIST_TRANSFORM, call(SqlFn.VARIANT_ELEMENTS, c("j")), lam(call(SqlFn.VARIANT_ELEMENTS, c("j")),
                "x", x -> call(SqlFn.VARIANT_GET, x, str("k")))));
        e(call(SqlFn.JSON_TYPE, c("j")));
        e(call(SqlFn.JSON_TYPE, call(SqlFn.VARIANT_GET, c("j"), str("k"))));
        w(call(SqlFn.EQUAL, call(SqlFn.JSON_TYPE, c("j")), new SqlExpr.JsonKindLit(JsonKind.OBJECT)));
        for (JsonKind k : JsonKind.values()) {
            e(call(SqlFn.EQUAL, call(SqlFn.JSON_TYPE, c("j")), new SqlExpr.JsonKindLit(k)));
        }
        e(call(SqlFn.JSON_ARRAY_LENGTH, c("j")));
        e(call(SqlFn.JSON_PRETTY, c("j")));
        e(new SqlExpr.JsonObject(List.of(str("a"), c("n"), str("b"), c("s"), str("c"), c("j"), str("d"), c("xs"),
                str("e"), c("dt"), str("f"), c("b"), str("g"), c("d"), str("h"), c("ts"), str("i"), nul())));
        e(new SqlExpr.JsonObject(List.of()));
        e(new SqlExpr.JsonObject(List.of(str("x"), new SqlExpr.JsonObject(List.of(str("y"), c("st"))))));
        e(new SqlExpr.JsonArray(List.of(c("n"), c("s"), c("b"), c("xs"), bool(true), c("j"), c("dt"), c("d"))));
        e(new SqlExpr.JsonArray(List.of()));
        e(new SqlExpr.StructLit(List.of(new SqlExpr.StructLit.Field("a", c("n")), new SqlExpr.StructLit.Field("b",
                c("s")), new SqlExpr.StructLit.Field("c", nul(), DATE))));
        e(new SqlExpr.StructGet(c("st"), "a"));
        e(new SqlExpr.StructGet(c("st"), "b"));
        e(new SqlExpr.StructGet(c("j"), "k"));
        e(new SqlExpr.StructGet(c("j"), "it's"));
        SqlType twice = new SqlType.Struct(List.of(new SqlType.Struct.Field("a", BIGINT),
                new SqlType.Struct.Field("a", JSON)));
        e(new SqlExpr.StructGet(SqlExpr.Column.of("t0", "st", twice, true, OutputCol.Origin.DERIVED), "a",
                new TypeFact.Unknown()));
        e(new SqlExpr.StructGet(new SqlExpr.StructGet(c("j"), "a"), "b"));
        e(new SqlExpr.StructGet(call(SqlFn.LIST_GET, c("sts"), i(1)), "a"));
    }

    static void temporal() {
        for (DatePart p : DatePart.values()) {
            e(call(SqlFn.DATE_TRUNC, part(p), c("ts")));
            e(call(SqlFn.EXTRACT, part(p), c("ts")));
            e(call(SqlFn.DATE_DIFF, part(p), c("ts"), c("ts")));
            e(call(SqlFn.ADD_INTERVAL, part(p), c("n"), c("ts")));
            e(call(SqlFn.TIME_BUCKET, part(p), i(2), c("ts")));
        }
        for (DatePart p : List.of(DatePart.YEAR, DatePart.MONTH, DatePart.DAY, DatePart.WEEK, DatePart.QUARTER,
                DatePart.HOUR, DatePart.DAY_OF_YEAR)) {
            e(call(SqlFn.DATE_TRUNC, part(p), c("dt")));
            e(call(SqlFn.EXTRACT, part(p), c("dt")));
            e(call(SqlFn.DATE_DIFF, part(p), c("dt"), c("dt")));
            e(call(SqlFn.ADD_INTERVAL, part(p), c("n"), c("dt")));
            e(call(SqlFn.TIME_BUCKET, part(p), i(2), c("dt")));
            e(call(SqlFn.EXTRACT, part(p), c("tz")));
            e(call(SqlFn.DATE_TRUNC, part(p), c("tz")));
        }
        e(call(SqlFn.EXTRACT, part(DatePart.HOUR), c("tm")));
        e(call(SqlFn.DATE_DIFF, part(DatePart.DAY), c("dt"), c("ts")));
        e(call(SqlFn.DATE_DIFF, part(DatePart.SECOND), c("tz"), c("tz")));
        e(call(SqlFn.ADD_INTERVAL, part(DatePart.DAY), i(-3), c("tz")));
        e(call(SqlFn.ADD_INTERVAL, part(DatePart.DAY), call(SqlFn.MINUS, c("n"), i(1)),
                call(SqlFn.DATE_TRUNC_DAY, c("ts"))));
        e(call(SqlFn.ADD_INTERVAL, part(DatePart.MONTH), i(1), new SqlExpr.DateLit("2024-01-31")));
        e(call(SqlFn.ADD_INTERVAL, part(DatePart.HOUR), i(1), new SqlExpr.TimestampLit("2024-01-31T10:00:00")));
        for (DatePart p : List.of(DatePart.YEAR, DatePart.MONTH, DatePart.DAY, DatePart.HOUR, DatePart.MINUTE,
                DatePart.SECOND, DatePart.MILLISECOND, DatePart.MICROSECOND, DatePart.WEEK)) {
            e(call(SqlFn.ADD_INTERVAL_TEMPORAL, part(p), c("n"), c("ts")));
            e(call(SqlFn.ADD_INTERVAL_TEMPORAL, part(p), c("n"), c("dt")));
        }
        e(call(SqlFn.TODAY));
        e(call(SqlFn.NOW));
        e(call(SqlFn.DATE_TRUNC_DAY, c("ts")));
        e(call(SqlFn.DATE_TRUNC_DAY, c("dt")));
        e(call(SqlFn.DATE_TRUNC_DAY, c("tz")));
        e(call(SqlFn.MAKE_DATE, i(2024), i(1), i(2)));
        e(call(SqlFn.MAKE_DATE, c("n"), c("n"), c("n")));
        e(call(SqlFn.MAKE_TIMESTAMP, i(2024), i(1), i(2), i(3), i(4), i(5)));
        e(call(SqlFn.MAKE_TIMESTAMP, i(2024), i(1), i(2), i(3), i(4), f(5.25)));
        e(call(SqlFn.MAKE_TIMESTAMP, i(2024), i(1), i(2), i(3), i(4)));
        e(call(SqlFn.MAKE_TIMESTAMP, c("n"), c("n"), c("n"), c("n"), c("n"), c("d")));
        e(call(SqlFn.TIMEZONE, str("America/New_York"), c("ts")));
        e(call(SqlFn.TIMEZONE, str("America/New_York"), c("tz")));
        e(call(SqlFn.TIMEZONE, c("s"), c("ts")));
        e(call(SqlFn.TIMEZONE, str("UTC"), call(SqlFn.TIMEZONE, str("Europe/London"), c("ts"))));
        e(call(SqlFn.DATE_AT_TIME, c("dt"), new SqlExpr.TimeLit("10:11:12")));
        e(call(SqlFn.DATE_AT_TIME, c("dt"), c("tm")));
        e(call(SqlFn.DATE_AT_TIME, call(SqlFn.DATE_TRUNC_DAY, c("ts")), new SqlExpr.TimeLit("10:11:12.5")));
        for (SqlFn fn : List.of(SqlFn.EPOCH_NS, SqlFn.EPOCH_SECONDS, SqlFn.EPOCH_MS, SqlFn.DAYNAME,
                SqlFn.MONTHNAME)) {
            e(call(fn, c("ts")));
            e(call(fn, c("dt")));
            e(call(fn, c("tz")));
        }
        for (SqlFn fn : List.of(SqlFn.FROM_EPOCH_NS, SqlFn.FROM_EPOCH_SECONDS, SqlFn.FROM_EPOCH_MS)) {
            e(call(fn, c("n")));
            e(call(fn, c("d")));
        }
        e(call(SqlFn.PARSE_DATE, c("s")));
        e(call(SqlFn.PARSE_DATE, str("2024-01-02T03:04:05+01:00")));
        e(call(SqlFn.STRPTIME, c("s"), fmt(DateFmt.Part.YEAR4)));
        e(call(SqlFn.STRPTIME, c("s"), new SqlExpr.FormatLit(DateFmt.ISO_LOCAL)));
        e(call(SqlFn.STRPTIME, c("s"), new SqlExpr.FormatLit(DateFmt.DATE)));
        e(call(SqlFn.STRPTIME, c("s"), new SqlExpr.FormatLit(DateFmt.ISO_MICRO)));
        e(call(SqlFn.STRPTIME, c("s"), c("s2")));
        e(call(SqlFn.STRPTIME, c("s"), fmt(DateFmt.Part.DAY2, new DateFmt.Text("/"), DateFmt.Part.MONTH_ABBREV,
                new DateFmt.Text("/"), DateFmt.Part.YEAR4, new DateFmt.Text(" "), DateFmt.Part.HOUR12,
                new DateFmt.Text(":"), DateFmt.Part.MIN2, DateFmt.Part.AMPM)));
    }

    static void formats() {
        List<List<DateFmt>> fixed = List.of(DateFmt.DATE, DateFmt.YEAR_MONTH, DateFmt.ISO_LOCAL, DateFmt.TIME_ONLY,
                DateFmt.ISO_DOT, DateFmt.ISO_MICRO, DateFmt.ISO_NANO, List.of());
        for (List<DateFmt> f : fixed) {
            e(call(SqlFn.STRFTIME, c("ts"), new SqlExpr.FormatLit(f)));
        }
        for (DateFmt.Part part : DateFmt.Part.values()) {
            e(call(SqlFn.STRFTIME, c("ts"), fmt(part)));
        }
        e(call(SqlFn.STRFTIME, c("ts"), fmt(DateFmt.Part.YEAR4, new DateFmt.Text("-%'\"T"),
                DateFmt.Part.MONTH_NAME, new DateFmt.Text(" "), DateFmt.Part.HOUR12, DateFmt.Part.MIN2,
                DateFmt.Part.AMPM, new DateFmt.Text("."), DateFmt.Part.SUBSEC_MICRO, DateFmt.Part.WEEKDAY_NAME,
                DateFmt.Part.MONTH_ABBREV, DateFmt.Part.HOUR12_NOPAD, DateFmt.Part.SUBSEC_NANO,
                DateFmt.Part.SUBSEC_MIN, DateFmt.Part.SEC2)));
        e(call(SqlFn.STRFTIME, c("dt"), new SqlExpr.FormatLit(DateFmt.DATE)));
        e(call(SqlFn.STRFTIME, c("tz"), new SqlExpr.FormatLit(DateFmt.ISO_LOCAL)));
        e(call(SqlFn.STRFTIME, c("tm"), new SqlExpr.FormatLit(DateFmt.TIME_ONLY)));
        e(call(SqlFn.STRFTIME, c("ts"), c("s")));
        e(call(SqlFn.STRFTIME, c("ts"), fmt(new DateFmt.Text("%Y %%"))));
        e(call(SqlFn.RTRIM, call(SqlFn.STRFTIME, c("ts"), new SqlExpr.FormatLit(DateFmt.ISO_MICRO)), str("0")));
    }

    static void literals() {
        String[] strings = {"", "plain", "it's", "a\\b", "line\nbreak\ttab\rcr", "a\u0000b'c\u0000",
            "\u00e9\u4e2d\ud83d\ude00", "$1 ${x} {y} %s ?", "\"quoted\"", "TDSNull", "\u001f", "--comment /* c */"};
        for (String s : strings) {
            e(str(s));
            et(str(s));
        }
        long[] ints = {0, 1, -1, 42, Integer.MAX_VALUE, (long) Integer.MAX_VALUE + 1, Long.MAX_VALUE, Long.MIN_VALUE};
        for (long v : ints) {
            e(i(v));
            et(i(v));
        }
        double[] floats = {0.0, -0.0, 1.0, 1.5, -2.25, 0.1, 1.0E20, 1.0E-7, 123456789.125, 1.0E300, 4.9E-324,
            Double.MAX_VALUE, 100.0, 1.0E7, 12345678.9};
        for (double v : floats) {
            e(f(v));
            et(f(v));
        }
        String[] decimals = {"0", "2.50", "-1.005", "12345678901234567890.123", "1E+3", "0.000001",
            "99999999999999999999999999999999999999"};
        for (String v : decimals) {
            e(dec(v));
            et(dec(v));
        }
        e(new SqlExpr.IntLit(5, new TypeFact.Typed(HUGEINT, false)));
        e(new SqlExpr.DecimalLit(new BigDecimal("170141183460469231731687303715884105727"),
                new TypeFact.Typed(HUGEINT, false)));
        et(new SqlExpr.DecimalLit(new BigDecimal("170141183460469231731687303715884105727"),
                new TypeFact.Typed(HUGEINT, false)));
        e(bool(true));
        e(bool(false));
        et(bool(true));
        e(nul());
        et(nul());
        e(new SqlExpr.NullLit(new TypeFact.Typed(DATE, true)));
        e(new SqlExpr.NullLit(new TypeFact.Typed(XS, true)));
        String[] dates = {"2024-01-02", "0001-01-01", "9999-12-31", "2024-02-29"};
        for (String d : dates) {
            e(new SqlExpr.DateLit(d));
            et(new SqlExpr.DateLit(d));
        }
        String[] stamps = {"2024-01-02T03:04:05", "2024-01-02T03:04:05.123", "2024-01-02T03:04:05.123456",
            "2024-01-02T03:04:05.123456789", "2024-01-02T00:00", "2024-01-02T03:04:05Z", "2024-01-02T03:04:05+05:30",
            "2024-01-02T03:04:05.5-08:00"};
        for (String s : stamps) {
            e(new SqlExpr.TimestampLit(s));
            et(new SqlExpr.TimestampLit(s));
        }
        String[] times = {"03:04:05", "03:04", "03:04:05.123456", "23:59:59.999999999"};
        for (String s : times) {
            e(new SqlExpr.TimeLit(s));
            et(new SqlExpr.TimeLit(s));
        }
        e(arr(i(1), i(2), i(3)));
        et(arr(i(1), i(2), i(3)));
        e(arr(str("a"), str("b")));
        e(arr(f(1.5), f(2.0)));
        e(arr(dec("1.5"), dec("2.25")));
        e(arr(new SqlExpr.DateLit("2024-01-02")));
        e(arr(new SqlExpr.TimestampLit("2024-01-02T03:04:05")));
        e(arr(bool(true), nul()));
        e(arr(arr(i(1)), arr(i(2), i(3))));
        e(arr(c("j"), c("j")));
        e(arr(c("st")));
        e(arr());
        e(new SqlExpr.ArrayLit(List.of(), new TypeFact.Typed(SS, false)));
        e(new SqlExpr.ArrayLit(List.of(), new TypeFact.Typed(JS, false)));
        e(arr(nul(), nul()));
        e(new SqlExpr.StructLit(List.of(new SqlExpr.StructLit.Field("a", i(1)), new SqlExpr.StructLit.Field("b",
                str("x")))));
        et(new SqlExpr.StructLit(List.of(new SqlExpr.StructLit.Field("a", i(1)), new SqlExpr.StructLit.Field("b",
                str("x")))));
        e(new SqlExpr.StructLit(List.of()));
        for (SqlType.Scalar s : SqlType.Scalar.values()) {
            e(new SqlExpr.TypeNameLit(s));
        }
        e(new SqlExpr.Group(call(SqlFn.OR, c("b"), c("b"))));
        w(call(SqlFn.AND, new SqlExpr.Group(call(SqlFn.OR, c("b"), c("b"))), c("b")));
        e(new SqlExpr.RowOrder("t0"));
        e(new SqlExpr.RowOrder(null));
        q(select(t(), List.of(p(c("s"), "s")), null, List.of(), List.of(asc(new SqlExpr.RowOrder("t0"))), null,
                null));
        e(new SqlExpr.TempTableInSplice("tmp"));
        e(new SqlExpr.Star(null));
    }

    static void casts() {
        SqlType[] targets = {BOOLEAN, INTEGER, BIGINT, HUGEINT, DOUBLE, DATE, TIMESTAMP, TIMESTAMPTZ, TIME, JSON,
            VARCHAR, DEC, DEC0, DEC38, TEMPORAL_TEXT, XS, SS, DS, JS, STRUCT, MAP};
        String[] froms = {"s", "n", "i", "d", "b", "dt", "ts", "tz", "tm", "j", "h", "dec", "dec0", "xs", "ss", "js",
            "st", "m", "tt"};
        for (String from : froms) {
            for (SqlType target : targets) {
                e(cast(c(from), target));
            }
        }
        for (SqlType target : targets) {
            e(new SqlExpr.Cast(c("s"), target, true));
            e(cast(nul(), target));
            e(cast(str("1"), target));
        }
        e(new SqlExpr.Cast(c("j"), BIGINT, true));
        e(new SqlExpr.Cast(c("j"), XS, true));
        e(cast(call(SqlFn.VARIANT_GET, c("j"), str("k")), VARCHAR));
        e(cast(call(SqlFn.VARIANT_GET, c("j"), str("k")), BIGINT));
        e(cast(call(SqlFn.VARIANT_GET, c("j"), str("k")), DOUBLE));
        e(cast(call(SqlFn.VARIANT_GET, c("j"), str("k")), BOOLEAN));
        e(cast(call(SqlFn.VARIANT_GET, c("j"), str("k")), DATE));
        e(cast(call(SqlFn.VARIANT_GET, c("j"), str("k")), TIMESTAMP));
        e(cast(call(SqlFn.VARIANT_GET, c("j"), str("k")), DEC));
        e(cast(call(SqlFn.VARIANT_GET, c("j"), str("k")), XS));
        e(cast(call(SqlFn.VARIANT_GET, c("j"), str("k")), STS));
        e(cast(c("j"), STS));
        e(cast(c("j"), XSS));
        e(cast(f(2.5), VARCHAR));
        e(cast(bool(false), VARCHAR));
        e(cast(i(3), DOUBLE));
        e(cast(dec("1.25"), VARCHAR));
        e(cast(new SqlExpr.DateLit("2024-01-02"), VARCHAR));
        e(cast(new SqlExpr.TimestampLit("2024-01-02T03:04:05"), VARCHAR));
        e(cast(arr(i(1)), JSON));
        e(cast(arr(i(1)), SS));
        e(cast(new SqlExpr.StructLit(List.of(new SqlExpr.StructLit.Field("a", i(1)))), JSON));
        e(cast(call(SqlFn.LIST_TRANSFORM, c("xs"), lam(c("xs"), "x", x -> x)), JSON));
    }

    // ---- aggregates: grouped, over a window, and over a collection value

    static SqlExpr[] aggArgs(SqlAgg.Fn fn) {
        return switch (fn) {
            case COUNT -> new SqlExpr[] {c("n")};
            case STRING_AGG -> new SqlExpr[] {c("s"), str(",")};
            case QUANTILE_CONT, QUANTILE_DISC -> new SqlExpr[] {c("d"), f(0.25)};
            case CORR, COVAR_SAMP, COVAR_POP -> new SqlExpr[] {c("d"), c("n")};
            case ARG_MAX, ARG_MIN -> new SqlExpr[] {c("s"), c("n")};
            case WAVG -> new SqlExpr[] {c("d"), c("n")};
            case BOOL_AND, BOOL_OR -> new SqlExpr[] {c("b")};
            case MODE, ANY_VALUE, MIN, MAX, UNIQUE_VALUE_ONLY -> new SqlExpr[] {c("s")};
            case LIST -> new SqlExpr[] {c("n")};
            case HASH_LIST -> new SqlExpr[] {c("s")};
            case IS_DISTINCT_MARK -> new SqlExpr[] {c("n")};
            case ROW_NUMBER, RANK, DENSE_RANK, PERCENT_RANK, CUME_DIST -> new SqlExpr[] {};
            case NTILE -> new SqlExpr[] {i(4)};
            case LAG, LEAD -> new SqlExpr[] {c("n"), i(1)};
            case FIRST_VALUE, LAST_VALUE -> new SqlExpr[] {c("s")};
            case NTH_VALUE -> new SqlExpr[] {c("s"), i(2)};
            default -> new SqlExpr[] {c("d")};
        };
    }

    static void aggregates() {
        for (SqlAgg.Fn fn : SqlAgg.Fn.values()) {
            SqlExpr[] args = aggArgs(fn);
            q(grouped(new SqlAgg.Reducer(fn, List.of(args), false, List.of())));
            q(grouped(new SqlAgg.Reducer(fn, List.of(args), true, List.of())));
            q(grouped(new SqlAgg.Reducer(fn, List.of(args), false, List.of(desc(c("n"))))));
            // over the whole table, no grouping
            q(select(t(), p(new SqlAgg.Reducer(fn, List.of(args), false, List.of()), "v")));
        }
        // the numeric aggregates over each numeric type
        for (SqlAgg.Fn fn : List.of(SqlAgg.Fn.SUM, SqlAgg.Fn.AVG, SqlAgg.Fn.MIN, SqlAgg.Fn.MAX, SqlAgg.Fn.MEDIAN,
                SqlAgg.Fn.STDDEV_SAMP, SqlAgg.Fn.VAR_POP, SqlAgg.Fn.MODE)) {
            for (String col : List.of("n", "i", "dec", "h", "dt", "ts", "s", "b")) {
                q(grouped(reducer(fn, c(col))));
            }
        }
        q(grouped(reducer(SqlAgg.Fn.COUNT)));
        q(grouped(new SqlAgg.Reducer(SqlAgg.Fn.COUNT, List.of(new SqlExpr.Star(null)), false, List.of())));
        q(grouped(new SqlAgg.Reducer(SqlAgg.Fn.COUNT, List.of(c("n"), c("s")), true, List.of())));
        q(grouped(new SqlAgg.Reducer(SqlAgg.Fn.LIST, List.of(c("n")), true, List.of(new SqlSelect.SortKey(c("n"),
                false, SqlSelect.SortKey.NullOrder.NULLS_FIRST, null)))));
        q(grouped(new SqlAgg.Reducer(SqlAgg.Fn.LIST, List.of(c("n"), c("s")), false, List.of())));
        q(grouped(reducer(SqlAgg.Fn.LIST, c("j"))));
        q(grouped(reducer(SqlAgg.Fn.LIST, c("st"))));
        q(grouped(reducer(SqlAgg.Fn.LIST, c("xs"))));
        q(grouped(reducer(SqlAgg.Fn.LIST, c("dt"))));
        q(grouped(reducer(SqlAgg.Fn.LIST, c("d"))));
        q(grouped(new SqlAgg.Reducer(SqlAgg.Fn.STRING_AGG, List.of(c("s"), str(",")), true, List.of(asc(c("s"))))));
        q(grouped(new SqlAgg.Reducer(SqlAgg.Fn.STRING_AGG, List.of(c("s"), str("'|'")), false, List.of(
                new SqlSelect.SortKey(c("n"), true, SqlSelect.SortKey.NullOrder.NULLS_LAST, null),
                desc(c("s"))))));
        q(grouped(new SqlAgg.Reducer(SqlAgg.Fn.STRING_AGG, List.of(c("s"), c("s2")), false, List.of())));
        q(grouped(new SqlAgg.Reducer(SqlAgg.Fn.QUANTILE_CONT, List.of(c("s"), f(0.25)), false, List.of(asc(
                c("s"))))));
        q(grouped(new SqlAgg.Reducer(SqlAgg.Fn.QUANTILE_CONT, List.of(c("d"), f(0.5)), false, List.of(desc(
                c("d"))))));
        q(grouped(new SqlAgg.Reducer(SqlAgg.Fn.QUANTILE_DISC, List.of(c("n"), f(0.75)), false, List.of(asc(
                c("n"))))));
        q(grouped(new SqlAgg.Reducer(SqlAgg.Fn.ARG_MAX, List.of(c("s"), c("n")), false, List.of(asc(c("n"))))));
        q(grouped(new SqlAgg.Reducer(SqlAgg.Fn.ARG_MIN, List.of(c("dt"), c("ts")), false, List.of())));
        q(grouped(reducer(SqlAgg.Fn.MEDIAN, c("dt"))));
        q(grouped(reducer(SqlAgg.Fn.ANY_VALUE, c("j"))));
        q(grouped(reducer(SqlAgg.Fn.MAX, c("j"))));
        q(grouped(reducer(SqlAgg.Fn.MIN, c("xs"))));
        // an aggregate inside an expression, a HAVING, an ORDER BY over an aggregate
        q(grouped(call(SqlFn.PLUS, reducer(SqlAgg.Fn.SUM, c("n")), i(1))));
        q(new SqlSelect(List.of(p(c("s"), "s"), p(reducer(SqlAgg.Fn.COUNT, c("n")), "v")), false, t(), null,
                List.of(c("s")), call(SqlFn.GREATER, reducer(SqlAgg.Fn.COUNT, c("n")), i(1)), null,
                List.of(desc(reducer(SqlAgg.Fn.SUM, c("n")))), 10L, null, List.of()));
        q(new SqlSelect(List.of(p(c("s"), "s"), p(c("n"), "n")), true, t(), null, List.of(), null, null,
                List.of(asc(c("s"))), null, null, List.of()));
        q(new SqlSelect(List.of(p(c("s"), "s")), false, t(), null, List.of(c("s"), c("n")), null, null, List.of(),
                null, null, List.of()));
        q(new SqlSelect(List.of(p(c("s"), "s")), false, t(), null, List.of(c("s"), call(SqlFn.UPPER, c("s2"))),
                null, null, List.of(), null, null, List.of()));
        e(new SqlExpr.OrderedListAgg(c("s"), c("n")));
        e(new SqlExpr.OrderedListAgg(c("j"), c("dt")));
        q(grouped(new SqlExpr.OrderedListAgg(c("n"), c("ts"))));
        q(grouped(new SqlExpr.JsonArrayAgg(c("n"))));
        q(grouped(new SqlExpr.JsonArrayAgg(c("s"), List.of(new SqlExpr.JsonArrayAgg.Key(c("n"), true),
                new SqlExpr.JsonArrayAgg.Key(c("s"), false)))));
        q(grouped(new SqlExpr.JsonArrayAgg(new SqlExpr.JsonObject(List.of(str("a"), c("n"), str("b"), c("j"))))));
        q(grouped(new SqlExpr.JsonArrayAgg(c("j"))));
    }

    static void windows() {
        List<SqlExpr> part = List.of(c("s"));
        List<SqlSelect.SortKey> order = List.of(asc(c("n")));
        for (SqlAgg.Fn fn : SqlAgg.Fn.values()) {
            SqlExpr[] args = aggArgs(fn);
            SqlAgg agg = switch (fn) {
                case ROW_NUMBER, RANK, DENSE_RANK, PERCENT_RANK, CUME_DIST, NTILE ->
                        new SqlAgg.RankingFn(fn, List.of(args));
                case LAG, LEAD, FIRST_VALUE, LAST_VALUE, NTH_VALUE -> new SqlAgg.ValueFn(fn, List.of(args));
                default -> new SqlAgg.Reducer(fn, List.of(args), false, List.of());
            };
            e(new SqlExpr.WindowCall(agg, part, order, null));
            e(new SqlExpr.WindowCall(agg, List.of(), List.of(), null));
            e(new SqlExpr.WindowCall(agg, part, List.of(), null));
            if (agg instanceof SqlAgg.Reducer) {
                e(new SqlExpr.WindowCall(new SqlAgg.Reducer(fn, List.of(args), true, List.of()), part, List.of(),
                        null));
            }
        }
        e(new SqlExpr.WindowCall(new SqlAgg.Reducer(SqlAgg.Fn.ROW_NUMBER, List.of(), false, List.of()), part, order,
                null));
        e(new SqlExpr.WindowCall(new SqlAgg.ValueFn(SqlAgg.Fn.LAG, List.of(c("n"))), part, order, null));
        e(new SqlExpr.WindowCall(new SqlAgg.ValueFn(SqlAgg.Fn.LEAD, List.of(c("n"), i(2), i(0))), part, order,
                null));
        e(new SqlExpr.WindowCall(new SqlAgg.ValueFn(SqlAgg.Fn.LAG, List.of(c("j"), i(1))), part, order, null));
        e(new SqlExpr.WindowCall(new SqlAgg.ValueFn(SqlAgg.Fn.FIRST_VALUE, List.of(c("j"))), part, order, null));
        // the frames
        SqlExpr.WindowCall.Frame.Bound[] bounds = {new SqlExpr.WindowCall.Frame.Bound.UnboundedPreceding(),
            new SqlExpr.WindowCall.Frame.Bound.Preceding(2), new SqlExpr.WindowCall.Frame.Bound.CurrentRow(),
            new SqlExpr.WindowCall.Frame.Bound.Following(3L), new SqlExpr.WindowCall.Frame.Bound.UnboundedFollowing()};
        SqlAgg.Reducer sum = new SqlAgg.Reducer(SqlAgg.Fn.SUM, List.of(c("n")), false, List.of());
        for (SqlExpr.WindowCall.Frame.Kind kind : SqlExpr.WindowCall.Frame.Kind.values()) {
            for (int a = 0; a < bounds.length; a++) {
                for (int b = a; b < bounds.length; b++) {
                    e(new SqlExpr.WindowCall(sum, part, order, new SqlExpr.WindowCall.Frame(kind, bounds[a],
                            bounds[b])));
                }
            }
        }
        e(new SqlExpr.WindowCall(sum, part, order, new SqlExpr.WindowCall.Frame(SqlExpr.WindowCall.Frame.Kind.RANGE,
                new SqlExpr.WindowCall.Frame.Bound.Preceding(1.5), new SqlExpr.WindowCall.Frame.Bound.Following(
                        new BigDecimal("2.5")))));
        e(new SqlExpr.WindowCall(new SqlAgg.ValueFn(SqlAgg.Fn.LAST_VALUE, List.of(c("s"))), part, order,
                new SqlExpr.WindowCall.Frame(SqlExpr.WindowCall.Frame.Kind.ROWS,
                        new SqlExpr.WindowCall.Frame.Bound.UnboundedPreceding(),
                        new SqlExpr.WindowCall.Frame.Bound.UnboundedFollowing())));
        // the interval RANGE frames
        String[] units = {"DAYS", "hours", "MONTHS", "YEARS", "WEEKS", "MINUTES", "SECONDS", "MILLISECONDS",
            "MICROSECONDS", "QUARTERS"};
        for (String unit : units) {
            for (String key : List.of("dt", "ts")) {
                e(new SqlExpr.WindowCall(sum, part, List.of(asc(c(key))), new SqlExpr.WindowCall.Frame(
                        SqlExpr.WindowCall.Frame.Kind.RANGE, new SqlExpr.WindowCall.Frame.Bound.IntervalPreceding(3,
                                unit), new SqlExpr.WindowCall.Frame.Bound.IntervalFollowing(1, unit))));
            }
        }
        e(new SqlExpr.WindowCall(sum, List.of(), List.of(new SqlSelect.SortKey(c("ts"), false,
                SqlSelect.SortKey.NullOrder.NULLS_FIRST, null)), new SqlExpr.WindowCall.Frame(
                SqlExpr.WindowCall.Frame.Kind.RANGE, new SqlExpr.WindowCall.Frame.Bound.IntervalPreceding(3, "DAYS"),
                new SqlExpr.WindowCall.Frame.Bound.CurrentRow())));
        e(new SqlExpr.WindowCall(sum, List.of(), List.of(asc(c("n"))), new SqlExpr.WindowCall.Frame(
                SqlExpr.WindowCall.Frame.Kind.RANGE, new SqlExpr.WindowCall.Frame.Bound.IntervalPreceding(3, "DAYS"),
                new SqlExpr.WindowCall.Frame.Bound.CurrentRow())));
        // the null orders, the descending keys, several keys
        e(new SqlExpr.WindowCall(new SqlAgg.RankingFn(SqlAgg.Fn.RANK, List.of()), part, List.of(
                new SqlSelect.SortKey(c("n"), true, SqlSelect.SortKey.NullOrder.NULLS_FIRST, null),
                new SqlSelect.SortKey(c("s2"), false, SqlSelect.SortKey.NullOrder.NULLS_LAST, null),
                new SqlSelect.SortKey(c("d"), false, null, null)), null));
        // an ordered aggregate over a window
        SqlAgg.Reducer ordered = new SqlAgg.Reducer(SqlAgg.Fn.STRING_AGG, List.of(c("s"), str(",")), false,
                List.of(desc(c("n"))));
        e(new SqlExpr.WindowCall(ordered, List.of(c("dt")), List.of(), null));
        e(new SqlExpr.WindowCall(ordered, List.of(), order, null));
        e(new SqlExpr.WindowCall(new SqlAgg.Reducer(SqlAgg.Fn.QUANTILE_CONT, List.of(c("d"), f(0.5)), false,
                List.of(asc(c("d")))), part, List.of(), null));
        e(new SqlExpr.WindowCall(new SqlAgg.Reducer(SqlAgg.Fn.LIST, List.of(c("n")), false, List.of(asc(c("n")))),
                part, List.of(), null));
        // a window in a filter (QUALIFY), and a window over a window's select
        q(new SqlSelect(List.of(p(c("s"), "s"), p(c("n"), "n")), false, t(), null, List.of(), null,
                call(SqlFn.EQUAL, new SqlExpr.WindowCall(new SqlAgg.RankingFn(SqlAgg.Fn.ROW_NUMBER, List.of()), part,
                        order, null), i(1)), List.of(), null, null, List.of()));
        q(new SqlSelect(List.of(p(c("s"), "s"), p(new SqlExpr.WindowCall(sum, part, order, null), "w")), false, t(),
                call(SqlFn.GREATER, c("n"), i(0)), List.of(), null, call(SqlFn.LESS_EQUAL, new SqlExpr.WindowCall(
                        new SqlAgg.RankingFn(SqlAgg.Fn.RANK, List.of()), part, order, null), i(3)),
                List.of(asc(c("s"))), 5L, null, List.of()));
        q(grouped(new SqlExpr.WindowCall(new SqlAgg.Reducer(SqlAgg.Fn.SUM, List.of(reducer(SqlAgg.Fn.SUM, c("n"))),
                false, List.of()), List.of(), List.of(), null)));
        q(select(t(), List.of(p(c("s"), "s"), p(new SqlExpr.WindowCall(sum, part, order, null), "w")), null,
                List.of(), List.of(asc(c("s")), desc(new SqlExpr.WindowCall(sum, part, order, null))), null, null));
    }

    static void collections() {
        for (SqlAgg.Fn fn : SqlAgg.Fn.values()) {
            List<SqlExpr> extras = switch (fn) {
                case STRING_AGG -> List.of(str(","));
                case QUANTILE_CONT, QUANTILE_DISC -> List.of(f(0.5));
                default -> List.of();
            };
            String coll = switch (fn) {
                case STRING_AGG, MODE -> "ss";
                case BOOL_AND, BOOL_OR -> "bs";
                case CORR, COVAR_SAMP, COVAR_POP, ARG_MAX, ARG_MIN, WAVG -> "sts";
                default -> "xs";
            };
            e(new SqlExpr.ReduceCollection(fn, c(coll), extras));
        }
        e(new SqlExpr.ReduceCollection(SqlAgg.Fn.SUM, c("ds"), List.of()));
        e(new SqlExpr.ReduceCollection(SqlAgg.Fn.AVG, c("decs"), List.of()));
        e(new SqlExpr.ReduceCollection(SqlAgg.Fn.MAX, c("ss"), List.of()));
        e(new SqlExpr.ReduceCollection(SqlAgg.Fn.MIN, c("dts"), List.of()));
        e(new SqlExpr.ReduceCollection(SqlAgg.Fn.MEDIAN, c("ds"), List.of()));
        e(new SqlExpr.ReduceCollection(SqlAgg.Fn.MODE, c("xs"), List.of()));
        e(new SqlExpr.ReduceCollection(SqlAgg.Fn.STRING_AGG, c("ss"), List.of(c("s"))));
        e(new SqlExpr.ReduceCollection(SqlAgg.Fn.STRING_AGG, arr(str("a"), str("b")), List.of(str("-"))));
        e(new SqlExpr.ReduceCollection(SqlAgg.Fn.SUM, arr(i(1), i(2)), List.of()));
        e(new SqlExpr.ReduceCollection(SqlAgg.Fn.COUNT, c("js"), List.of()));
        e(new SqlExpr.ReduceCollection(SqlAgg.Fn.STDDEV_SAMP, c("ds"), List.of()));
        e(new SqlExpr.ReduceCollection(SqlAgg.Fn.QUANTILE_CONT, c("ds"), List.of(f(0.25))));
        e(new SqlExpr.ReduceCollection(SqlAgg.Fn.QUANTILE_DISC, c("ss"), List.of(f(0.25))));
        e(new SqlExpr.ReduceCollection(SqlAgg.Fn.SUM, call(SqlFn.LIST_TRANSFORM, c("xs"), lam(c("xs"), "x",
                x -> call(SqlFn.TIMES, x, i(2)))), List.of()));
        // folds
        SqlExpr xs = c("xs");
        e(new SqlExpr.FoldCall(xs, (SqlExpr.Lambda) lambda(call(SqlFn.PLUS, derived("x"), derived("acc")), "x",
                "acc"), i(0), false, true));
        e(new SqlExpr.FoldCall(c("ss"), (SqlExpr.Lambda) lambda(call(SqlFn.CONCAT, derived("acc"), derived("x")),
                "x", "acc"), str(""), false, true));
        e(new SqlExpr.FoldCall(xs, (SqlExpr.Lambda) lambda(call(SqlFn.LIST_APPEND, derived("acc"), derived("x")),
                "x", "acc"), new SqlExpr.ArrayLit(List.of(), new TypeFact.Typed(XS, false)), true, true));
        e(new SqlExpr.FoldCall(xs, (SqlExpr.Lambda) lambda(call(SqlFn.LIST_CONCAT, arr(derived("x")),
                derived("acc")), "x", "acc"), arr(i(0)), true, false));
        e(new SqlExpr.FoldCall(xs, (SqlExpr.Lambda) lambda(call(SqlFn.PLUS, derived("acc"), c("n")), "x", "acc"),
                c("n"), false, false));
        e(new SqlExpr.FoldCall(call(SqlFn.RANGE_FN, i(0), i(5)), (SqlExpr.Lambda) lambda(call(SqlFn.TIMES,
                derived("acc"), call(SqlFn.PLUS, derived("x"), i(1))), "x", "acc"), i(1), false, true));
        e(new SqlExpr.FoldCall(c("js"), (SqlExpr.Lambda) lambda(call(SqlFn.PLUS, derived("acc"), cast(
                call(SqlFn.VARIANT_GET, derived("x"), str("v")), BIGINT)), "x", "acc"), i(0), false, false));
        e(new SqlExpr.FoldCall(c("sts"), (SqlExpr.Lambda) lambda(call(SqlFn.PLUS, derived("acc"),
                new SqlExpr.StructGet(derived("x"), "a")), "x", "acc"), i(0), false, false));
        w(call(SqlFn.GREATER, new SqlExpr.FoldCall(xs, (SqlExpr.Lambda) lambda(call(SqlFn.PLUS, derived("x"),
                derived("acc")), "x", "acc"), i(0), false, true), i(3)));
    }

    // ---- query shapes

    static void structure() {
        // the root's values as their labels deliver them, a sort key naming a moved column its expression
        q(select(t(), List.of(p(c("dec"), "v"), p(c("d"), "w"), p(c("h"), "x"), p(nul(), "y", DEC), p(c("i"), "z"),
                p(c("n"), "n"), p(c("dec0"), "d0")), null, List.of(), List.of(asc(derived("v")), desc(derived("s"))),
                null, 5L));
        q(select(t(), List.of(p(cast(c("n"), DEC), "v"), p(call(SqlFn.DIVIDE, c("n"), i(2)), "w"),
                p(reducer(SqlAgg.Fn.AVG, c("n")), "a")), null, List.of(), List.of(), null, null));
        // a star, a star over a source, a star except
        q(select(t()));
        q(select(t(), new SqlSelect.Projection(new SqlExpr.Star("t0"), null, null)));
        q(select(t(), new SqlSelect.Projection(new SqlExpr.StarExcept("t0", List.of("s", "n")), null, null)));
        q(select(t(), new SqlSelect.Projection(new SqlExpr.StarExcept(null, List.of("j")), null, null),
                p(c("n"), "n2x")));
        q(select(sub(select(t(), p(c("s"), "s"), p(c("n"), "n")), "q0"), new SqlSelect.Projection(
                new SqlExpr.StarExcept("q0", List.of("n")), null, null)));
        // ORDER BY, LIMIT, OFFSET in every combination
        q(select(t(), List.of(p(c("n"), "n")), null, List.of(), List.of(new SqlSelect.SortKey(c("n"), true,
                SqlSelect.SortKey.NullOrder.NULLS_FIRST, null), new SqlSelect.SortKey(c("s"), false,
                SqlSelect.SortKey.NullOrder.NULLS_LAST, null), new SqlSelect.SortKey(c("d"), true,
                SqlSelect.SortKey.NullOrder.NULLS_LAST, null), new SqlSelect.SortKey(c("dt"), false,
                SqlSelect.SortKey.NullOrder.NULLS_FIRST, null)), 10L, 20L));
        q(select(t(), List.of(p(c("n"), "n")), null, List.of(), List.of(), null, 7L));
        q(select(t(), List.of(p(c("n"), "n")), null, List.of(), List.of(), 0L, null));
        q(select(t(), List.of(p(c("n"), "n")), null, List.of(), List.of(asc(c("n"))), 3L, null));
        q(select(t(), List.of(p(c("n"), "n")), null, List.of(), List.of(new SqlSelect.SortKey(c("n"), true, null,
                "n")), null, null));
        q(select(t(), List.of(p(c("j"), "j")), null, List.of(), List.of(asc(c("j"))), null, null));
        q(select(t(), List.of(p(c("n"), "n")), null, List.of(), List.of(asc(i(1))), null, null));
        // unions
        SqlSelect a = select(t(), List.of(p(c("dec"), "v")), null, List.of(), List.of(asc(c("n"))), 3L, null);
        SqlSelect b = select(t(), p(c("dec"), "v"));
        q(new SqlUnion(List.of(a, b), true, a.outputs()));
        q(new SqlUnion(List.of(b, b), false, b.outputs()));
        q(new SqlUnion(List.of(select(t(), p(c("n"), "v")), select(t(), p(c("d"), "v")), select(t(), p(nul(), "v"))),
                true, List.of(new OutputCol("v", DOUBLE, true))));
        q(select(sub(new SqlUnion(List.of(b, b), true, b.outputs()), "u0"), List.of(p(SqlExpr.Column.of("u0",
                b.outputs(), "v"), "v")), null, List.of(), List.of(asc(SqlExpr.Column.of("u0", b.outputs(), "v"))),
                2L, null));
        SqlSelect nested = select(sub(b, "s"), p(SqlExpr.Column.of("s", b.outputs(), "v"), "v"));
        q(new SqlUnion(List.of(b, nested), true, b.outputs()));
        // WITH: plain, materialized, recursive, a CTE read twice
        List<OutputCol> wv = List.of(oc("v", DEC));
        q(new SqlWith(List.of(new SqlWith.Cte("w", select(t(), p(c("dec"), "v")), true)),
                select(new SqlSource.Cte("w", "w0", wv), p(SqlExpr.Column.of("w0", wv, "v"), "v"))));
        q(new SqlWith(List.of(new SqlWith.Cte("w", select(t(), p(c("dec"), "v")), false), new SqlWith.Cte("w2",
                select(new SqlSource.Cte("w", "w1", wv), p(SqlExpr.Column.of("w1", wv, "v"), "v")), false)),
                select(new SqlSource.Join(new SqlSource.Cte("w2", "a", wv), new SqlSource.Cte("w", "b", wv),
                        SqlSource.Join.Kind.INNER, call(SqlFn.EQUAL, SqlExpr.Column.of("a", wv, "v"),
                                SqlExpr.Column.of("b", wv, "v"))), p(SqlExpr.Column.of("a", wv, "v"), "v"))));
        List<OutputCol> rv = List.of(oc("k", BIGINT));
        SqlQuery recursive = new SqlUnion(List.of(select(new SqlSource.Dual(), p(i(1), "k")),
                select(new SqlSource.Cte("r", "r0", rv), List.of(p(call(SqlFn.PLUS, SqlExpr.Column.of("r0", rv, "k"),
                        i(1)), "k")), call(SqlFn.LESS, SqlExpr.Column.of("r0", rv, "k"), i(10)), List.of(), List.of(),
                        null, null)), true, rv);
        q(new SqlWith(List.of(new SqlWith.Cte("r", recursive, false)), select(new SqlSource.Cte("r", "r1", rv),
                p(SqlExpr.Column.of("r1", rv, "k"), "k")), true));
        // a FROM-less select, VALUES with rows and with none
        q(select(new SqlSource.Dual(), p(i(1), "a"), p(str("x"), "b")));
        q(select(new SqlSource.Dual(), p(call(SqlFn.NOW), "a")));
        List<OutputCol> vo = List.of(oc("a", BIGINT), oc("b", VARCHAR));
        q(select(new SqlSource.Values(List.of(List.of(i(1), str("x")), List.of(i(2), nul())), List.of("a", "b"),
                "v0", vo)));
        q(select(new SqlSource.Values(List.of(), List.of("a", "b"), "v0", vo)));
        q(select(new SqlSource.Values(List.of(List.of(i(1), str("x"))), List.of("a", "b"), "v0", vo),
                p(SqlExpr.Column.of("v0", vo, "a"), "a")));
        List<OutputCol> vo2 = List.of(oc("d", DATE), oc("j", JSON), oc("xs", XS));
        q(select(new SqlSource.Values(List.of(List.of(new SqlExpr.DateLit("2024-01-02"), cast(str("{}"), JSON),
                arr(i(1)))), List.of("d", "j", "xs"), "v1", vo2)));
        // a NULL projection, labelled by its output; a projection without a label
        q(select(t(), new SqlSelect.Projection(nul(), null, oc("z", BIGINT))));
        q(select(t(), new SqlSelect.Projection(c("n"), null, null)));
        q(select(t(), new SqlSelect.Projection(call(SqlFn.PLUS, c("n"), i(1)), null, null)));
        // the other sources
        List<OutputCol> so = List.of(oc("a", BIGINT), oc("b", VARCHAR));
        q(select(new SqlSource.SourceUrl("file:///data/x.csv", "u0", so)));
        q(select(new SqlSource.SourceUrl("https://example.com/x.parquet", "u0", so),
                p(SqlExpr.Column.of("u0", so, "a"), "a")));
        q(select(new SqlSource.VarSetPlaceholder("v", "p0", so)));
        q(select(new SqlSource.RawSql("SELECT 1 AS a, 'x' AS b", "r0", so, "H2")));
        q(select(new SqlSource.RawSql("SELECT 1 AS a, 'x' AS b", "r0", so, "Snowflake"),
                p(SqlExpr.Column.of("r0", so, "a"), "a")));
        for (SqlSource.Catalog.Grid g : SqlSource.Catalog.Grid.values()) {
            List<OutputCol> co = List.of(oc("TABLE_SCHEM", VARCHAR));
            q(select(new SqlSource.Catalog(g, "S%", g == SqlSource.Catalog.Grid.SCHEMAS ? null : "T%",
                    g == SqlSource.Catalog.Grid.COLUMNS ? "C%" : null, "c0", co)));
        }
        q(select(new SqlSource.Table("t", "t0", COLUMNS, true, Map.of(), NameQuoting.BARE)));
        q(select(new SqlSource.Table("My Table", "t0", COLUMNS, false, Map.of(), new NameQuoting(true, true))));
        q(select(new SqlSource.Table("sch.tab", "t0", COLUMNS, false, Map.of(), new NameQuoting(false, true))));
        q(select(new SqlSource.Table("t", "t0", List.of(oc("s", VARCHAR), new OutputCol("Quoted Col", VARCHAR, true,
                OutputCol.Origin.PHYSICAL_QUOTED), oc("o", VARCHAR), oc("js", JSON)), false, ordered("o",
                new SqlDdl.ColumnType.Plain(SqlDdl.ColumnType.Kind.OTHER), "js", new SqlDdl.ColumnType.Plain(
                        SqlDdl.ColumnType.Kind.JSON)), NameQuoting.BARE)));
        // stored reads of each kind
        for (SqlDdl.ColumnType.Kind k : SqlDdl.ColumnType.Kind.values()) {
            List<OutputCol> sto = List.of(oc("a", VARCHAR));
            q(select(new SqlSource.Table("t", "t0", sto, false, Map.of("a", new SqlDdl.ColumnType.Plain(k)),
                    NameQuoting.BARE), p(SqlExpr.Column.of("t0", sto, "a"), "a")));
        }
        // identifiers needing quotes, reserved words, a label with a quote
        q(select(t(), p(c("n"), "select"), p(c("s"), "Mixed Case"), p(c("d"), "it's"), p(c("b"), "a\"b"),
                p(c("j"), "order"), p(c("dt"), "_x"), p(c("ts"), "1st")));
        // the explosion markers
        q(select(t(), p(call(SqlFn.UNNEST, c("xs")), "a"), p(call(SqlFn.UNNEST, c("ss")), "b"),
                p(call(SqlFn.UNNEST, c("js")), "c"), p(c("n"), "n")));
        SqlSelect m = marker(p(call(SqlFn.UNNEST, nul()), "a"), p(call(SqlFn.UNNEST, arr(i(1), i(2))), "b"));
        q(select(sub(m, "u"), p(derived("a"), "a"), p(derived("b"), "b")));
        q(select(sub(m, "u")));
        q(m);
        SqlSelect lm = marker(p(call(SqlFn.UNNEST, c("xs")), "e"));
        q(select(new SqlSource.Join(t(), sub(lm, "u"), SqlSource.Join.Kind.LEFT_LATERAL, bool(true)), p(c("n"), "n"),
                p(SqlExpr.Column.derived("u", "e"), "e")));
        q(select(new SqlSource.Join(t(), sub(lm, "u"), SqlSource.Join.Kind.LEFT_LATERAL, call(SqlFn.GREATER, c("n"),
                i(1))), p(c("n"), "n")));
        q(select(new SqlSource.Join(t(), sub(lm, "u"), SqlSource.Join.Kind.CROSS_LATERAL, null), p(c("n"), "n"),
                p(SqlExpr.Column.derived("u", "e"), "e")));
        SqlSelect lms = marker(p(call(SqlFn.UNNEST, c("sts")), "e"));
        q(select(new SqlSource.Join(t(), sub(lms, "u"), SqlSource.Join.Kind.LEFT_LATERAL, bool(true)),
                p(new SqlExpr.StructGet(SqlExpr.Column.derived("u", "e"), "a"), "a")));
        SqlSelect carry = select(new SqlSource.Dual(), p(c("xs"), "lst"));
        SqlSelect explode = select(sub(carry, "c"), p(call(SqlFn.UNNEST, SqlExpr.Column.derived("c", "lst")),
                "elem"));
        q(select(new SqlSource.Join(t(), sub(explode, "u"), SqlSource.Join.Kind.CROSS_LATERAL, null), p(c("n"), "n"),
                p(SqlExpr.Column.derived("u", "elem"), "elem")));
        q(select(new SqlSource.Join(t(), sub(SqlSelect.starOf(sub(lm, "i0")), "u"), SqlSource.Join.Kind.CROSS_LATERAL,
                null)));
        SqlSelect inner = select(t(), p(call(SqlFn.UNNEST, c("xs")), "x"), p(c("n"), "n"));
        q(select(sub(inner, "s0"), p(call(SqlFn.UNNEST, c("ss")), "y"), p(SqlExpr.Column.derived("s0", "x"), "x")));
        q(select(t(), List.of(p(call(SqlFn.UNNEST, new SqlAgg.Reducer(SqlAgg.Fn.LIST, List.of(c("n")), false,
                List.of())), "x")), null, List.of(c("s")), List.of(), null, null));
        q(select(t(), List.of(p(call(SqlFn.UNNEST, c("xs")), "x")), call(SqlFn.GREATER, c("n"), i(0)), List.of(),
                List.of(asc(c("n"))), 5L, null));
        q(select(t(), p(call(SqlFn.UNNEST, call(SqlFn.RANGE_FN, i(1), c("n"))), "x")));
        // a lateral subquery read as rows: a star over it, its fields, LEFT and CROSS
        SqlSelect rows = select(t(), List.of(p(c("n"), "k"), p(c("j"), "jv")), call(SqlFn.EQUAL, c("s"),
                SqlExpr.Column.derived("l0", "s")), List.of(), List.of(), null, null);
        SqlSource left = t("l0");
        q(select(new SqlSource.Join(left, sub(rows, "r"), SqlSource.Join.Kind.CROSS_LATERAL, null)));
        q(select(new SqlSource.Join(left, sub(rows, "r"), SqlSource.Join.Kind.LEFT_LATERAL, bool(true)),
                p(SqlExpr.Column.derived("r", "k"), "k"), p(SqlExpr.Column.derived("r", "jv"), "jv")));
        q(select(new SqlSource.Join(left, sub(rows, "r"), SqlSource.Join.Kind.LEFT_LATERAL, call(SqlFn.GREATER,
                SqlExpr.Column.derived("r", "k"), i(0))), p(SqlExpr.Column.derived("r", "k"), "k")));
        // a constant lateral row
        SqlSelect constant = select(new SqlSource.Dual(), p(i(1), "one"), p(c("n"), "nn"));
        q(select(new SqlSource.Join(t(), sub(constant, "k0"), SqlSource.Join.Kind.CROSS_LATERAL, null), p(c("s"),
                "s"), p(SqlExpr.Column.derived("k0", "one"), "one")));
        // inline render of a subquery-holding text
        text(select(t(), p(new SqlExpr.TimestampLit("2024-01-02T03:04:05.123"), "ts"),
                p(new SqlExpr.DateLit("2024-01-02"), "dt"), p(new SqlExpr.TimeLit("03:04:05"), "tm")));
        text(select(t(), List.of(p(c("s"), "s")), call(SqlFn.EQUAL, c("s"), str("x")), List.of(), List.of(asc(
                c("s"))), 1L, null));
    }

    static void subqueries() {
        SqlSelect sub = select(t("t1"), p(c("t1", "n"), "n"));
        SqlSelect correlated = select(t("t1"), List.of(p(c("t1", "n"), "n")), call(SqlFn.EQUAL, c("t1", "s"),
                c("s")), List.of(), List.of(), null, null);
        q(where(new SqlExpr.Exists(sub), p(c("s"), "s")));
        q(where(call(SqlFn.NOT, new SqlExpr.Exists(correlated)), p(c("s"), "s")));
        q(where(new SqlExpr.InSubquery(c("n"), sub), p(c("s"), "s")));
        q(where(call(SqlFn.NOT, new SqlExpr.InSubquery(c("n"), correlated)), p(c("s"), "s")));
        e(new SqlExpr.ScalarSubquery(select(t("t1"), p(reducer(SqlAgg.Fn.MAX, c("t1", "n")), "m"))));
        e(new SqlExpr.ScalarSubquery(select(t("t1"), List.of(p(reducer(SqlAgg.Fn.COUNT, c("t1", "n")), "m")),
                call(SqlFn.EQUAL, c("t1", "s"), c("s")), List.of(), List.of(), null, null)));
        e(new SqlExpr.ScalarSubquery(select(t("t1"), List.of(p(c("t1", "s"), "m")), null, List.of(),
                List.of(asc(c("t1", "n"))), 1L, null)));
        e(new SqlExpr.Exists(correlated));
        e(new SqlExpr.InSubquery(c("n"), sub));
        for (SqlFn cmp : List.of(SqlFn.EQUAL, SqlFn.NOT_EQUAL, SqlFn.LESS, SqlFn.LESS_EQUAL, SqlFn.GREATER,
                SqlFn.GREATER_EQUAL)) {
            for (SqlExpr.Quantifier qu : SqlExpr.Quantifier.values()) {
                q(where(new SqlExpr.Quantified(c("n"), cmp, qu, sub), p(c("s"), "s")));
            }
        }
        e(new SqlExpr.Quantified(c("n"), SqlFn.GREATER, SqlExpr.Quantifier.ALL, correlated));
        // a subquery in a FROM, nested, aliased, ordered
        q(select(sub(select(sub(select(t(), p(c("n"), "n"), p(c("s"), "s")), "a"), p(SqlExpr.Column.derived("a",
                "n"), "n")), "b"), p(SqlExpr.Column.derived("b", "n"), "n")));
        q(select(sub(select(t(), List.of(p(c("n"), "n")), null, List.of(), List.of(asc(c("n"))), 5L, null), "a"),
                p(SqlExpr.Column.derived("a", "n"), "n")));
        q(select(new SqlSource.Subselect(select(t(), p(c("n"), "n")), "a", "frame"),
                p(SqlExpr.Column.derived("a", "n"), "n")));
        q(select(new SqlSource.Subselect(select(t(), p(c("n"), "n")), "a", SqlSource.Subselect.EXISTS_KEYS_FRAME),
                p(SqlExpr.Column.derived("a", "n"), "n")));
        // a deferred TDS string
        e(new SqlExpr.DeferredTdsString(select(t("t1"), p(c("t1", "s"), "s"), p(c("t1", "n"), "n")), "d0", 1));
        e(new SqlExpr.DeferredTdsString(select(t("t1"), p(c("t1", "s"), "s")), "d0", 2,
                SqlExpr.DeferredTdsString.Form.CSV, true));
    }

    static void joins() {
        SqlSource r = t("t1");
        SqlExpr on = call(SqlFn.EQUAL, c("n"), c("t1", "n"));
        for (SqlSource.Join.Kind k : SqlSource.Join.Kind.values()) {
            SqlExpr cond = switch (k) {
                case CROSS, CROSS_LATERAL -> null;
                case ASOF_LEFT -> call(SqlFn.AND, call(SqlFn.EQUAL, c("s"), c("t1", "s")), call(SqlFn.GREATER_EQUAL,
                        c("ts"), c("t1", "ts")));
                default -> on;
            };
            SqlSource right = k == SqlSource.Join.Kind.CROSS_LATERAL || k == SqlSource.Join.Kind.LEFT_LATERAL
                    ? sub(select(t("t1"), List.of(p(c("t1", "n"), "n")), call(SqlFn.EQUAL, c("t1", "s"), c("s")),
                            List.of(), List.of(), null, null), "t2") : r;
            q(select(new SqlSource.Join(t(), right, k, k == SqlSource.Join.Kind.LEFT_LATERAL ? bool(true) : cond),
                    p(c("s"), "s"), p(k == SqlSource.Join.Kind.CROSS_LATERAL || k == SqlSource.Join.Kind.LEFT_LATERAL
                            ? SqlExpr.Column.derived("t2", "n") : c("t1", "n"), "n1")));
            q(select(new SqlSource.Join(t(), right, k, k == SqlSource.Join.Kind.LEFT_LATERAL ? bool(true) : cond)));
        }
        // a join's residual over the padded side, a three-way join, a join of subselects
        q(select(new SqlSource.Join(t(), r, SqlSource.Join.Kind.LEFT, call(SqlFn.AND, on, call(SqlFn.GREATER,
                c("t1", "d"), i(0)))), p(c("s"), "s")));
        q(select(new SqlSource.Join(t(), r, SqlSource.Join.Kind.FULL, call(SqlFn.AND, on, call(SqlFn.EQUAL,
                c("s"), str("x")))), p(c("s"), "s")));
        q(select(new SqlSource.Join(t(), r, SqlSource.Join.Kind.RIGHT, call(SqlFn.OR, on, c("t1", "b"))),
                p(c("s"), "s")));
        q(select(new SqlSource.Join(t(), r, SqlSource.Join.Kind.LEFT, call(SqlFn.AND, on, new SqlExpr.Exists(
                select(t("t3"), List.of(p(c("t3", "n"), "n")), call(SqlFn.EQUAL, c("t3", "n"), c("t1", "n")),
                        List.of(), List.of(), null, null)))), p(c("s"), "s")));
        q(select(new SqlSource.Join(new SqlSource.Join(t(), r, SqlSource.Join.Kind.INNER, on), t("t2"),
                SqlSource.Join.Kind.LEFT, call(SqlFn.EQUAL, c("t1", "s"), c("t2", "s"))), p(c("s"), "s"),
                p(c("t2", "d"), "d")));
        SqlSelect ls = select(t(), p(c("n"), "n"), p(c("s"), "s"));
        SqlSelect rs = select(t(), p(c("n"), "n"), p(c("d"), "d"));
        q(select(new SqlSource.Join(sub(ls, "a"), sub(rs, "b"), SqlSource.Join.Kind.FULL, call(SqlFn.EQUAL,
                SqlExpr.Column.derived("a", "n"), SqlExpr.Column.derived("b", "n"))),
                p(SqlExpr.Column.derived("a", "s"), "s"), p(SqlExpr.Column.derived("b", "d"), "d")));
        q(select(new SqlSource.Join(t(), r, SqlSource.Join.Kind.INNER, call(SqlFn.NULL_SAFE_EQUAL, c("s"),
                c("t1", "s"))), p(c("s"), "s")));
        q(select(new SqlSource.Join(t(), r, SqlSource.Join.Kind.ASOF_LEFT, call(SqlFn.LESS, c("ts"), c("t1", "ts"))),
                p(c("s"), "s")));
        q(select(new SqlSource.Join(t(), r, SqlSource.Join.Kind.ASOF_LEFT, call(SqlFn.AND, call(SqlFn.EQUAL, c("s"),
                c("t1", "s")), call(SqlFn.LESS_EQUAL, c("t1", "dt"), c("dt")))), p(c("s"), "s"),
                p(c("t1", "d"), "d")));
    }

    static void pivots() {
        List<OutputCol> po = List.of(oc("s", VARCHAR), new OutputCol("a__|__q", BIGINT, true, OutputCol.Origin.DERIVED),
                new OutputCol("b__|__q", BIGINT, true, OutputCol.Origin.DERIVED));
        SqlSource.Pivot staticPivot = new SqlSource.Pivot(t(), List.of(c("s2")), List.of(str("a"), str("b")),
                List.of(), List.of(new SqlSource.Pivot.Using(new SqlAgg.Reducer(SqlAgg.Fn.SUM, List.of(c("n")), false,
                        List.of()), "q")), "p0", po);
        q(select(staticPivot));
        q(select(staticPivot, p(SqlExpr.Column.of("p0", po, "s"), "s"), p(SqlExpr.Column.of("p0", po, "a__|__q"),
                "a")));
        SqlSource.Pivot dynamic = new SqlSource.Pivot(t(), List.of(c("s2")), List.of(), List.of(),
                List.of(new SqlSource.Pivot.Using(new SqlAgg.Reducer(SqlAgg.Fn.SUM, List.of(c("n")), false, List.of()),
                        "q", BIGINT), new SqlSource.Pivot.Using(new SqlAgg.Reducer(SqlAgg.Fn.COUNT, List.of(c("s")),
                        false, List.of()), "c")), "p0", List.of(oc("s", VARCHAR)));
        q(select(dynamic));
        SqlSource.Pivot named = new SqlSource.Pivot(t(), List.of(c("s2"), c("b")), List.of(),
                List.of(str("x"), str("y")), List.of(new SqlSource.Pivot.Using(new SqlAgg.Reducer(SqlAgg.Fn.MAX,
                        List.of(c("d")), false, List.of()), "mx")), "p0", List.of(oc("s", VARCHAR)));
        q(select(named));
        SqlSource.Pivot overSub = new SqlSource.Pivot(sub(select(t(), p(c("s"), "s"), p(c("s2"), "k"), p(c("n"),
                "n")), "q0"), List.of(SqlExpr.Column.derived("q0", "k")), List.of(i(1), str("o'k"), nul()), List.of(),
                List.of(new SqlSource.Pivot.Using(new SqlAgg.Reducer(SqlAgg.Fn.AVG, List.of(SqlExpr.Column.derived(
                        "q0", "n")), false, List.of()), "avg")), "p1", List.of(oc("s", VARCHAR)));
        q(select(overSub));
        // the per-key region a dynamic pivot's statement is written as
        List<SqlSelect.Projection> perKey = List.of(p(c("s"), "s"), new SqlSelect.Projection(
                new SqlAgg.Reducer(SqlAgg.Fn.SUM, List.of(new SqlExpr.Case(List.of(new SqlExpr.Case.When(
                        call(SqlFn.EQUAL, c("s2"), new SqlExpr.KeyCell(0, new TypeFact.Typed(VARCHAR, false))),
                        c("n"))), null)), false, List.of()),
                "k", new OutputCol("k", BIGINT, true, OutputCol.Origin.PER_KEY)));
        q(select(t(), perKey, null, List.of(c("s")), List.of(), null, null));
        e(new SqlExpr.KeyCell(0, new TypeFact.Typed(VARCHAR, false)));
    }

    static void parameters() {
        SqlExpr.PlanParam.Kind[] kinds = SqlExpr.PlanParam.Kind.values();
        for (SqlExpr.PlanParam.Kind k : kinds) {
            q(where(call(SqlFn.EQUAL, c("s"), new SqlExpr.PlanParam("p", k, false, null)), p(c("n"), "n")));
        }
        q(where(call(SqlFn.EQUAL, c("ts"), new SqlExpr.PlanParam("p", SqlExpr.PlanParam.Kind.DATETIME, false, null,
                new TypeFact.Unknown(), new ValueTyping(List.of(ValueKind.DATE_TIME, ValueKind.DATE),
                        ValueKind.DATE_TIME))), p(c("n"), "n")));
        q(where(call(SqlFn.EQUAL, c("d"), new SqlExpr.PlanParam("p", SqlExpr.PlanParam.Kind.FLOAT, false, null,
                new TypeFact.Unknown(), new ValueTyping(List.of(ValueKind.FLOATING, ValueKind.INTEGER,
                        ValueKind.DECIMAL), ValueKind.FLOATING))), p(c("n"), "n")));
        q(where(call(SqlFn.EQUAL, c("dt"), new SqlExpr.PlanParam("p", SqlExpr.PlanParam.Kind.STRICT_DATE, false, null,
                new TypeFact.Typed(DATE, false), null)), p(c("n"), "n")));
        for (SqlType ty : List.of(BIGINT, VARCHAR, DOUBLE, BOOLEAN, DATE, TIMESTAMP, DEC, JSON, INTEGER, TIME)) {
            e(new SqlExpr.PlanParam("p", SqlExpr.PlanParam.Kind.OTHER, false, null, new TypeFact.Typed(ty, true),
                    null));
        }
        q(where(new SqlExpr.Membership(c("n"), new SqlExpr.PlanParam("ns", SqlExpr.PlanParam.Kind.OTHER, false, null,
                new TypeFact.Typed(XS, true), null)), p(c("n"), "n")));
        q(where(call(SqlFn.IN, c("n"), new SqlExpr.PlanParam("ns", SqlExpr.PlanParam.Kind.OTHER, false, null,
                new TypeFact.Typed(XS, true), null)), p(c("n"), "n")));
        q(where(call(SqlFn.IN, c("s"), new SqlExpr.PlanParam("ss", SqlExpr.PlanParam.Kind.STRING, false, null,
                new TypeFact.Typed(SS, true), null)), p(c("n"), "n")));
        q(where(call(SqlFn.EQUAL, c("s"), new SqlExpr.PlanParam("e", SqlExpr.PlanParam.Kind.ENUM, true, "my::map")),
                p(c("n"), "n")));
        q(where(call(SqlFn.EQUAL, c("s"), new SqlExpr.PlanParam("o", SqlExpr.PlanParam.Kind.STRING, true, null,
                new TypeFact.Typed(VARCHAR, true), null)), p(c("n"), "n")));
        e(call(SqlFn.CONCAT, new SqlExpr.PlanParam("p", SqlExpr.PlanParam.Kind.STRING, false, null,
                new TypeFact.Typed(VARCHAR, false), null), str("x")));
        text(where(call(SqlFn.EQUAL, c("s"), new SqlExpr.PlanParam("p", SqlExpr.PlanParam.Kind.STRING, false,
                null)), p(c("n"), "n")));
        q(select(t(), List.of(p(c("n"), "n")), null, List.of(), List.of(), 10L, null));
    }

    static void checked() {
        SqlExpr list = call(SqlFn.LIST_FILTER, c("xs"), lam(c("xs"), "x", x -> call(SqlFn.GREATER, x, i(10))));
        e(new SqlExpr.CheckedOne(new SqlExpr.CompactList(list), false, false));
        e(new SqlExpr.CheckedOne(new SqlExpr.CompactList(list), false, true));
        e(new SqlExpr.CheckedOne(new SqlExpr.CompactList(list), true, false));
        e(new SqlExpr.CheckedOne(c("js"), false, false));
        e(new SqlExpr.CheckedOne(c("ss")));
        SqlExpr defect = new SqlExpr.JsonObject(List.of(str("id"), str("c1"), str("message"), str("bad"), str("path"),
                call(SqlFn.TO_VARIANT, arr())));
        SqlExpr own = call(SqlFn.LIST_FILTER, arr(new SqlExpr.Case(List.of(new SqlExpr.Case.When(call(
                SqlFn.IS_NULL, c("s")), defect)), nul())), lam(arr(defect), "x", x -> call(SqlFn.IS_NOT_NULL, x)));
        e(new SqlExpr.CheckedDefects(own, List.of()));
        e(new SqlExpr.CheckedDefects(own, List.of(new SqlExpr.CheckedDefects.Hoist(List.of("child"), c("j"), false),
                new SqlExpr.CheckedDefects.Hoist(List.of("kids", "x"), c("j"), true))));
        e(new SqlExpr.CheckedChildValue(c("j"), false));
        e(new SqlExpr.CheckedChildValue(c("j"), true));
    }

    static void commonSubexpressions() {
        // a movable chain of 41 nodes, repeated: bound once
        SqlExpr x = c("n");
        for (int k = 1; k <= 20; k++) {
            x = call(SqlFn.PLUS, x, i(k));
        }
        e(call(SqlFn.TIMES, x, x));
        SqlExpr y = call(SqlFn.MINUS, x, x);
        e(call(SqlFn.TIMES, y, y));
        SqlExpr z = derived("e");
        for (int k = 1; k <= 20; k++) {
            z = call(SqlFn.PLUS, z, i(k));
        }
        e(call(SqlFn.LIST_TRANSFORM, c("xs"), lambda(call(SqlFn.TIMES, z, z), "e")));
        SqlExpr r = call(SqlFn.PARSE_INT, call(SqlFn.CONCAT, c("s"), str("1"), c("s"), str("2"), c("s"), str("3"),
                c("s"), str("4"), c("s"), str("5"), c("s"), str("6"), c("s"), str("7"), c("s"), str("8"), c("s"),
                str("9"), c("s"), str("0"), c("s"), str("1"), c("s"), str("2")));
        e(call(SqlFn.TIMES, r, r));
        q(select(t(), List.of(p(c("s"), "s"), p(call(SqlFn.TIMES, x, new SqlAgg.Reducer(SqlAgg.Fn.SUM,
                List.of(x), false, List.of())), "v")), null, List.of(c("s")), List.of(), null, null));
        q(select(t(), List.of(p(c("n"), "n")), call(SqlFn.GREATER, x, call(SqlFn.TIMES, x, i(2))), List.of(),
                List.of(), null, null));
        q(new SqlUnion(List.of(select(t(), p(call(SqlFn.TIMES, x, x), "v")), select(t(), p(call(SqlFn.TIMES, x,
                x), "v"))), false, List.of(new OutputCol("v", BIGINT, true))));
        SqlExpr big = call(SqlFn.CONCAT, c("s"), str("a"), c("s"), str("b"), c("s"), str("c"), c("s"), str("d"),
                c("s"), str("e"), c("s"));
        e(call(SqlFn.LPAD, call(SqlFn.LPAD, big, i(10), str("x")), i(20), str("y")));
    }

    static void statements() {
        List<SqlDdl.Column> cols = new ArrayList<>(List.of(new SqlDdl.Column("id", false, new SqlDdl.ColumnType.Plain(
                SqlDdl.ColumnType.Kind.BIGINT), true, true), new SqlDdl.Column("Name", true,
                new SqlDdl.ColumnType.Sized("VARCHAR", 10), false, false), new SqlDdl.Column("amount", false,
                new SqlDdl.ColumnType.Scaled("DECIMAL", 10, 2), true, false), new SqlDdl.Column("key", false,
                new SqlDdl.ColumnType.Plain(SqlDdl.ColumnType.Kind.VARCHAR), false, true)));
        add("ddl.render", new SqlDdl.CreateTable("hr", "People", NameQuoting.BARE, cols, true));
        add("ddl.render", new SqlDdl.CreateTable("hr", "People", NameQuoting.BARE, cols, false));
        add("ddl.render", new SqlDdl.CreateTable(null, "People", new NameQuoting(true, true), cols, false));
        // every column type
        List<SqlDdl.Column> all = new ArrayList<>();
        for (SqlDdl.ColumnType.Kind k : SqlDdl.ColumnType.Kind.values()) {
            all.add(new SqlDdl.Column("c_" + k.name().toLowerCase(java.util.Locale.ROOT), false,
                    new SqlDdl.ColumnType.Plain(k), false, false));
        }
        for (String sized : List.of("VARCHAR", "CHAR", "VARBINARY", "BINARY", "BIT", "FLOAT", "TIMESTAMP",
                "NVARCHAR", "CLOB")) {
            all.add(new SqlDdl.Column("s_" + sized.toLowerCase(java.util.Locale.ROOT), false,
                    new SqlDdl.ColumnType.Sized(sized, 12), false, false));
        }
        for (String scaled : List.of("DECIMAL", "NUMERIC", "NUMBER")) {
            all.add(new SqlDdl.Column("d_" + scaled.toLowerCase(java.util.Locale.ROOT), false,
                    new SqlDdl.ColumnType.Scaled(scaled, 38, 6), false, false));
        }
        add("ddl.render", new SqlDdl.CreateTable("s", "AllTypes", NameQuoting.BARE, all, false));
        for (SqlDdl.Column col : all) {
            add("ddl.render", new SqlDdl.CreateTable(null, "One", NameQuoting.BARE, List.of(col), false));
        }
        add("ddl.render", new SqlDdl.CreateTable(null, "Keys", NameQuoting.BARE, List.of(new SqlDdl.Column("a",
                false, new SqlDdl.ColumnType.Plain(SqlDdl.ColumnType.Kind.INTEGER), true, true), new SqlDdl.Column("b",
                false, new SqlDdl.ColumnType.Sized("VARCHAR", 5), true, true)), false));
        add("ddl.render", new SqlDdl.DropTable("hr", "People", NameQuoting.BARE));
        add("ddl.render", new SqlDdl.DropTable(null, "My People", new NameQuoting(false, true)));
        add("ddl.render", new SqlDdl.CreateSchema("hr", false));
        add("ddl.render", new SqlDdl.CreateSchema("My Schema", true));
        add("ddl.render", new SqlDdl.DropSchema("hr", false));
        add("ddl.render", new SqlDdl.DropSchema("My Schema", true));
        // DML
        List<List<SqlExpr>> rows = List.of(List.of(i(1), str("a'b"), dec("1.50"), nul()), List.of(i(2), str(""),
                f(2.5), bool(true)), List.of(i(3), new SqlExpr.DateLit("2024-01-02"),
                new SqlExpr.TimestampLit("2024-01-02T03:04:05.123"), cast(str("{}"), JSON)));
        for (String op : List.of("dml.render", "dml.renderStatement")) {
            add(op, new SqlDml.InsertValues("hr", "People", NameQuoting.BARE, List.of("id", "Name", "amount", "key"),
                    Set.of("Name"), rows));
            add(op, new SqlDml.InsertValues(null, "t", new NameQuoting(false, true), List.of("a"), Set.of(),
                    List.of(List.of(new SqlExpr.PlanParam("p", SqlExpr.PlanParam.Kind.OTHER, false, null,
                            new TypeFact.Typed(BIGINT, true), null)))));
            add(op, new SqlDml.InsertValues(null, "t", NameQuoting.BARE, List.of("a", "b"), Set.of(), List.of(
                    List.of(arr(i(1)), new SqlExpr.JsonObject(List.of(str("k"), i(1)))))));
            add(op, new SqlDml.InsertFromTable("hr", "People", NameQuoting.BARE, List.of("id", "Name"), Set.of("Name"),
                    "stage"));
            add(op, new SqlDml.DeleteAll(null, "select", NameQuoting.BARE));
            add(op, new SqlDml.DeleteAll("hr", "People", new NameQuoting(true, true)));
            add(op, new SqlDml.CreateTempTableAs("tmp", select(t(), p(c("n"), "n"), p(c("s"), "s"))));
            add(op, SqlDml.CreateTempTableAs.staged("tmp", select(t(), p(c("n"), "n"), p(c("s"), "s")),
                    List.of("n", "s")));
            add(op, new SqlDml.InsertSelect("hr", "People", NameQuoting.BARE, List.of("id", "Name"), Set.of("Name"),
                    select(t(), p(c("n"), "id"), p(c("s"), "Name"))));
            add(op, new SqlDml.Update("hr", "People", NameQuoting.BARE, "p0", Map.of(), List.of(
                    new SqlDml.Update.Assignment("Name", true, str("x")), new SqlDml.Update.Assignment("amount", false,
                            call(SqlFn.PLUS, SqlExpr.Column.derived("p0", "amount"), i(1)))), call(SqlFn.EQUAL,
                    SqlExpr.Column.derived("p0", "id"), i(1))));
            add(op, new SqlDml.Update(null, "People", NameQuoting.BARE, "p0", Map.of(), List.of(
                    new SqlDml.Update.Assignment("Name", false, str("x"))), null));
            add(op, new SqlDml.DeleteWhere("hr", "People", NameQuoting.BARE, "p0", Map.of(), call(SqlFn.GREATER,
                    SqlExpr.Column.derived("p0", "id"), i(5))));
            add(op, new SqlDml.DeleteWhere(null, "People", NameQuoting.BARE, "p0", Map.of(), null));
            add(op, new SqlDml.Merge("hr", "People", NameQuoting.BARE, List.of("id", "Name"), Set.of("Name"),
                    List.of("id"), select(t(), p(c("n"), "id"), p(c("s"), "Name")), "tgt", "src"));
        }
    }

    /** Shapes several dialects decide alike: a NULL labelled by each type, a placeholder typed by each value kind, a
     *  window inside an expression, a LEFT join's residual reading both sides. */
    static void shared() {
        List<SqlType> types = new ArrayList<>(List.of(SqlType.Scalar.values()));
        types.addAll(List.of(DEC, DEC0, DEC38, XS, SS, JS, STRUCT, MAP, XSS, STS, new SqlType.Array(TEMPORAL_TEXT),
                new SqlType.Array(SqlType.Scalar.DECIMAL_TEXT)));
        for (SqlType ty : types) {
            q(select(t(), new SqlSelect.Projection(nul(), null, new OutputCol("z", ty, true))));
            q(select(t(), p(c("n"), "n"), p(nul(), "z", ty)));
            e(cast(c("s"), ty));
        }
        for (ValueKind k : ValueKind.values()) {
            q(where(call(SqlFn.EQUAL, c("ts"), new SqlExpr.PlanParam("p", SqlExpr.PlanParam.Kind.DATETIME, false, null,
                    new TypeFact.Unknown(), new ValueTyping(List.of(ValueKind.DATE_TIME, k), ValueKind.DATE_TIME))),
                    p(c("n"), "n")));
            q(where(call(SqlFn.EQUAL, c("d"), new SqlExpr.PlanParam("p", SqlExpr.PlanParam.Kind.OTHER, false, null,
                    new TypeFact.Unknown(), new ValueTyping(List.of(k), k))), p(c("n"), "n")));
        }
        q(where(call(SqlFn.EQUAL, c("ts"), new SqlExpr.PlanParam("p", SqlExpr.PlanParam.Kind.DATETIME, false, null,
                new TypeFact.Unknown(), new ValueTyping(List.of(ValueKind.values()), ValueKind.DATE_TIME_NANOS))),
                p(c("n"), "n")));
        SqlAgg.Reducer sum = new SqlAgg.Reducer(SqlAgg.Fn.SUM, List.of(c("n")), false, List.of());
        SqlExpr.WindowCall rank = new SqlExpr.WindowCall(new SqlAgg.RankingFn(SqlAgg.Fn.ROW_NUMBER, List.of()),
                List.of(c("s")), List.of(asc(c("n"))), null);
        e(call(SqlFn.PLUS, new SqlExpr.WindowCall(sum, List.of(c("s")), List.of(asc(c("n"))), null), i(1)));
        e(call(SqlFn.TIMES, rank, rank));
        q(select(t(), List.of(p(c("s"), "s"), p(call(SqlFn.PLUS, rank, c("n")), "r")), call(SqlFn.GREATER, c("n"),
                i(0)), List.of(), List.of(asc(c("s"))), 5L, null));
        q(select(sub(select(t(), p(c("s"), "s"), p(c("n"), "n")), "q0"), p(call(SqlFn.MINUS,
                new SqlExpr.WindowCall(new SqlAgg.ValueFn(SqlAgg.Fn.LAG, List.of(SqlExpr.Column.derived("q0", "n"),
                        i(1))), List.of(), List.of(asc(SqlExpr.Column.derived("q0", "n"))), null),
                SqlExpr.Column.derived("q0", "n")), "d")));
        SqlExpr on = call(SqlFn.EQUAL, c("n"), c("t1", "n"));
        q(select(new SqlSource.Join(t(), t("t1"), SqlSource.Join.Kind.LEFT, call(SqlFn.AND, on, call(SqlFn.LESS,
                c("d"), c("t1", "d")))), p(c("s"), "s"), p(c("t1", "d"), "d")));
        q(select(new SqlSource.Join(t(), t("t1"), SqlSource.Join.Kind.LEFT, call(SqlFn.LESS, c("d"), c("t1", "d"))),
                p(c("s"), "s"), p(c("t1", "s"), "s1")));
        q(select(new SqlSource.Join(new SqlSource.Join(t(), t("t1"), SqlSource.Join.Kind.LEFT, call(SqlFn.AND, on,
                call(SqlFn.NOT_EQUAL, c("s"), c("t1", "s")))), t("t2"), SqlSource.Join.Kind.LEFT, call(SqlFn.AND,
                call(SqlFn.EQUAL, c("t1", "n"), c("t2", "n")), call(SqlFn.GREATER, c("t2", "ts"), c("ts")))),
                p(c("s"), "s"), p(c("t2", "d"), "d")));
    }

    /** Inputs MirJson refuses by design (a tagged value and a tagged list are resolved before a plan is written):
     *  counted, never a line. */
    static void refusedByMirJson() {
        SqlExpr tagged = new SqlExpr.Tagged(str("Integer"), c("n"), List.of(new SqlExpr.Tagged.Candidate("Integer",
                BIGINT, c("n")), new SqlExpr.Tagged.Candidate("String", VARCHAR, c("s"))));
        e(tagged);
        e(new SqlExpr.TaggedList(c("xs"), c("ss"), c("xs"), List.of(new SqlExpr.TaggedList.Channel("Integer", BIGINT,
                c("xs")))));
    }

    /** Databricks' and Trino's own arms the shared inputs leave: a pivot's value kinds, a quantified comparison no
     *  operator spells, interval frames in singular units, the as-of join's refusals, an unnest of two lists, a
     *  first-match replacement with a back reference, a star exclusion inside an expression, a struct of an untyped
     *  field, an indexed lambda under exists/forall, a STRING_AGG of one argument (grouped and windowed), a nanosecond
     *  date format, and FLOAT_CANON/FLOAT_PRINT in a DML row (no pass runs over a row's values). */
    static void databricksTrinoArms() {
        // a pivot's values of each literal kind, one key, one aggregate
        List<OutputCol> po = List.of(oc("s", VARCHAR));
        for (SqlExpr v : List.of(i(7), bool(true), f(1.5), new SqlExpr.DateLit("2024-01-02"))) {
            q(select(new SqlSource.Pivot(t(), List.of(c("s2")), List.of(v, str("b")), List.of(),
                    List.of(new SqlSource.Pivot.Using(new SqlAgg.Reducer(SqlAgg.Fn.SUM, List.of(c("n")), false,
                            List.of()), "q")), "p0", po)));
        }
        // a quantified comparison by an operator no switch arm spells
        SqlSelect sub = select(t("t1"), p(c("t1", "n"), "n"));
        q(where(new SqlExpr.Quantified(c("n"), SqlFn.IS_DISTINCT_FROM, SqlExpr.Quantifier.ANY, sub), p(c("s"), "s")));
        // interval RANGE frames in singular units (both bounds of one unit)
        SqlAgg.Reducer sum = new SqlAgg.Reducer(SqlAgg.Fn.SUM, List.of(c("n")), false, List.of());
        for (String unit : List.of("WEEK", "DAY", "HOUR", "MINUTE", "SECOND", "YEAR", "MONTH", "FORTNIGHT")) {
            e(new SqlExpr.WindowCall(sum, List.of(c("s")), List.of(asc(c("ts"))), new SqlExpr.WindowCall.Frame(
                    SqlExpr.WindowCall.Frame.Kind.RANGE, new SqlExpr.WindowCall.Frame.Bound.IntervalPreceding(2, unit),
                    new SqlExpr.WindowCall.Frame.Bound.IntervalFollowing(1, unit))));
        }
        // as-of joins: no inequality between the sides; under a filtering select; a left side with no columns
        q(select(new SqlSource.Join(t(), t("t1"), SqlSource.Join.Kind.ASOF_LEFT, call(SqlFn.EQUAL, c("s"),
                c("t1", "s"))), p(c("s"), "s"), p(c("t1", "d"), "d")));
        q(select(new SqlSource.Join(t(), t("t1"), SqlSource.Join.Kind.ASOF_LEFT, call(SqlFn.AND, call(SqlFn.EQUAL,
                c("s"), c("t1", "s")), call(SqlFn.GREATER_EQUAL, c("ts"), c("t1", "ts")))), List.of(p(c("s"), "s"),
                p(c("t1", "d"), "d")), call(SqlFn.GREATER, c("n"), i(0)), List.of(), List.of(), null, null));
        q(select(new SqlSource.Join(new SqlSource.Table("t", "t0", List.of(), false, Map.of(), NameQuoting.BARE),
                t("t1"), SqlSource.Join.Kind.ASOF_LEFT, call(SqlFn.GREATER_EQUAL, c("ts"), c("t1", "ts"))),
                p(c("s"), "s"), p(c("t1", "d"), "d")));
        // an unnest of two lists in one call
        e(call(SqlFn.UNNEST, c("xs"), c("ss")));
        // a first-match replacement whose pattern holds a back reference
        e(call(SqlFn.REGEXP_REPLACE, c("s"), str("(a)\\1"), str("x")));
        // a star exclusion inside an expression (StarExceptToColumns expands only a projection that is one)
        q(select(t(), List.of(p(c("s"), "s"), p(new SqlAgg.Reducer(SqlAgg.Fn.COUNT, List.of(new SqlExpr.StarExcept(
                "t0", List.of("s"))), false, List.of()), "c")), null, List.of(c("s")), List.of(), null, null));
        // a struct whose field's type nothing gives
        e(new SqlExpr.StructLit(List.of(new SqlExpr.StructLit.Field("a", derived("x")))));
        // a struct insert of a value of unknown type
        e(call(SqlFn.STRUCT_INSERT, c("st"), str("c"), derived("x")));
        // exists and forall over an indexed lambda
        e(call(SqlFn.LIST_EXISTS, c("xs"), lambda(call(SqlFn.GREATER, derived("x"), derived("i")), "x", "i")));
        e(call(SqlFn.LIST_FOR_ALL, c("xs"), lambda(call(SqlFn.GREATER, derived("x"), derived("i")), "x", "i")));
        // a STRING_AGG of one argument: grouped, and windowed with no order at all
        q(grouped(new SqlAgg.Reducer(SqlAgg.Fn.STRING_AGG, List.of(c("s")), false, List.of())));
        e(new SqlExpr.WindowCall(new SqlAgg.Reducer(SqlAgg.Fn.STRING_AGG, List.of(c("s")), false, List.of()),
                List.of(c("n")), List.of(), null));
        // a nanosecond part in a parse's format, and a format literal written as a value
        e(call(SqlFn.STRPTIME, c("s"), fmt(DateFmt.Part.YEAR4, new DateFmt.Text("-"), DateFmt.Part.SUBSEC_NANO)));
        e(fmt(DateFmt.Part.HOUR2, new DateFmt.Text(":"), DateFmt.Part.SUBSEC_NANO));
        // FLOAT_CANON and FLOAT_PRINT where no pass expands them: a DML row's values are written as they are
        for (String op : List.of("dml.render", "dml.renderStatement")) {
            add(op, new SqlDml.InsertValues(null, "t", NameQuoting.BARE, List.of("a", "b", "c"), Set.of(),
                    List.of(List.of(call(SqlFn.FLOAT_CANON, f(1.5)), call(SqlFn.FLOAT_PRINT, f(2.5)),
                            call(SqlFn.FLOAT_PRINT, dec("2.50"))))));
        }
    }

    /** ClickHouse's own arms the shapes above leave: the one-sided outer joins' keys over each source, the ordered
     *  lists' descending keys of each type, the RANGE frames of offsets (over each key type, fractional over each
     *  aggregate, interval beside each other bound), the format codes a parsed date spells, and the walls. */
    static void clickHouseArms() {
        SqlSource r = t("t1");
        // an outer join whose ON reads one side only, keyed by a column of the other side (anyColumnOf): a table, a
        // join, a subselect, VALUES, and a CTE of undeclared columns
        q(select(new SqlSource.Join(t(), r, SqlSource.Join.Kind.LEFT, c("b")), p(c("s"), "s"), p(c("t1", "s"), "s1")));
        q(select(new SqlSource.Join(t(), r, SqlSource.Join.Kind.RIGHT, c("t1", "b")), p(c("s"), "s")));
        q(select(new SqlSource.Join(new SqlSource.Join(t(), t("t2"), SqlSource.Join.Kind.INNER, call(SqlFn.EQUAL,
                c("n"), c("t2", "n"))), r, SqlSource.Join.Kind.FULL, call(SqlFn.GREATER, c("t1", "d"), i(0))),
                p(c("s"), "s")));
        q(select(new SqlSource.Join(t(), sub(select(t("t1"), p(c("t1", "n"), "k")), "q1"), SqlSource.Join.Kind.LEFT,
                call(SqlFn.GREATER, c("n"), i(0))), p(c("s"), "s"), p(SqlExpr.Column.derived("q1", "k"), "k")));
        List<OutputCol> vo = List.of(oc("a", BIGINT));
        q(select(new SqlSource.Join(t(), new SqlSource.Values(List.of(List.of(i(1)), List.of(i(2))), List.of("a"), "v1",
                vo), SqlSource.Join.Kind.LEFT, call(SqlFn.GREATER, c("n"), i(0))), p(c("s"), "s"),
                p(SqlExpr.Column.of("v1", vo, "a"), "a")));
        q(new SqlWith(List.of(new SqlWith.Cte("w", select(t(), p(c("n"), "n")), false)), select(new SqlSource.Join(
                t(), new SqlSource.Cte("w", "w0", List.of()), SqlSource.Join.Kind.LEFT, call(SqlFn.GREATER, c("n"),
                i(0))), p(c("s"), "s"))));
        // an as-of join with neither an equality nor an inequality; a star exclusion over a lateral explosion; VALUES
        // whose columns are not each declared
        q(select(new SqlSource.Join(t(), r, SqlSource.Join.Kind.ASOF_LEFT, call(SqlFn.NOT_EQUAL, c("s"),
                c("t1", "s"))), p(c("s"), "s")));
        SqlSelect lm = marker(p(call(SqlFn.UNNEST, c("xs")), "e"));
        q(select(new SqlSource.Join(t(), sub(lm, "u"), SqlSource.Join.Kind.CROSS_LATERAL, null),
                new SqlSelect.Projection(new SqlExpr.StarExcept("t0", List.of("s")), null, null)));
        q(select(new SqlSource.Values(List.of(List.of(i(1), str("x"))), List.of("a", "b"), "v0",
                List.of(oc("a", BIGINT)))));
        // a decimal literal past 76 digits; a correlated IN whose filter reads the outer row beside a comparison
        e(dec("1" + "0".repeat(80) + ".5"));
        q(where(new SqlExpr.InSubquery(c("n"), select(t("t1"), List.of(p(c("t1", "n"), "n")), call(SqlFn.OR,
                call(SqlFn.EQUAL, c("t1", "s"), c("s")), c("b")), List.of(), List.of(), null, null)), p(c("s"), "s")));
        // a struct field and an inserted value of no type
        e(new SqlExpr.StructLit(List.of(new SqlExpr.StructLit.Field("a", c("n")), new SqlExpr.StructLit.Field("b",
                nul()))));
        e(call(SqlFn.STRUCT_INSERT, c("st"), str("c"), nul()));
        // a list's STRING_AGG without its separator, a quantile without its level
        e(new SqlExpr.ReduceCollection(SqlAgg.Fn.STRING_AGG, c("ss"), List.of()));
        e(new SqlExpr.ReduceCollection(SqlAgg.Fn.QUANTILE_CONT, c("ds"), List.of()));
        e(new SqlExpr.ReduceCollection(SqlAgg.Fn.QUANTILE_CONT, c("ds"), List.of(c("d"))));
        // an unnest of two lists, alone and among several explosions
        q(select(t(), p(call(SqlFn.UNNEST, c("xs"), c("ss")), "a")));
        q(select(t(), p(call(SqlFn.UNNEST, c("xs"), c("ss")), "a"), p(call(SqlFn.UNNEST, c("xs")), "b")));
        // a STRING_AGG of one argument
        q(grouped(new SqlAgg.Reducer(SqlAgg.Fn.STRING_AGG, List.of(c("s")), false, List.of())));
        // ordered lists by a descending key of each type, and of no type
        for (SqlExpr key : List.of(c("i"), c("d"), c("h"), c("dec"), c("ts"), c("tz"), c("dt"), c("b"), c("s"),
                c("j"), derived("x"))) {
            q(grouped(new SqlAgg.Reducer(SqlAgg.Fn.LIST, List.of(c("n")), false, List.of(desc(key)))));
        }
        // RANGE frames of offsets over each key type
        SqlAgg.Reducer sum = new SqlAgg.Reducer(SqlAgg.Fn.SUM, List.of(c("n")), false, List.of());
        SqlExpr.WindowCall.Frame two = new SqlExpr.WindowCall.Frame(SqlExpr.WindowCall.Frame.Kind.RANGE,
                new SqlExpr.WindowCall.Frame.Bound.Preceding(2), new SqlExpr.WindowCall.Frame.Bound.Following(2));
        for (SqlExpr key : List.of(c("i"), c("s"), derived("x"))) {
            e(new SqlExpr.WindowCall(sum, List.of(c("s2")), List.of(asc(key)), two));
        }
        e(new SqlExpr.WindowCall(sum, List.of(), List.of(new SqlSelect.SortKey(c("d"), false,
                SqlSelect.SortKey.NullOrder.NULLS_FIRST, null)), two));
        // fractional RANGE offsets over each aggregate the frame computes, each bound kind, and a ranking function
        SqlExpr.WindowCall.Frame frac = new SqlExpr.WindowCall.Frame(SqlExpr.WindowCall.Frame.Kind.RANGE,
                new SqlExpr.WindowCall.Frame.Bound.Preceding(1.5), new SqlExpr.WindowCall.Frame.Bound.CurrentRow());
        for (SqlAgg.Fn fn : List.of(SqlAgg.Fn.AVG, SqlAgg.Fn.MIN, SqlAgg.Fn.MAX, SqlAgg.Fn.COUNT, SqlAgg.Fn.STDDEV,
                SqlAgg.Fn.STDDEV_SAMP, SqlAgg.Fn.STDDEV_POP, SqlAgg.Fn.VAR_SAMP, SqlAgg.Fn.VARIANCE,
                SqlAgg.Fn.VAR_POP, SqlAgg.Fn.MEDIAN, SqlAgg.Fn.BOOL_AND, SqlAgg.Fn.BOOL_OR, SqlAgg.Fn.ANY_VALUE,
                SqlAgg.Fn.MODE)) {
            SqlExpr arg = fn == SqlAgg.Fn.BOOL_AND || fn == SqlAgg.Fn.BOOL_OR ? c("b") : c("d");
            e(new SqlExpr.WindowCall(new SqlAgg.Reducer(fn, List.of(arg), false, List.of()), List.of(c("s")),
                    List.of(asc(c("n"))), frac));
        }
        e(new SqlExpr.WindowCall(sum, List.of(), List.of(desc(c("n"))), new SqlExpr.WindowCall.Frame(
                SqlExpr.WindowCall.Frame.Kind.RANGE, new SqlExpr.WindowCall.Frame.Bound.UnboundedPreceding(),
                new SqlExpr.WindowCall.Frame.Bound.Following(0.5))));
        e(new SqlExpr.WindowCall(new SqlAgg.RankingFn(SqlAgg.Fn.RANK, List.of()), List.of(), List.of(asc(c("n"))),
                frac));
        // interval RANGE frames beside each other bound, and over two keys
        SqlExpr.WindowCall.Frame.Bound[] others = {new SqlExpr.WindowCall.Frame.Bound.UnboundedPreceding(),
            new SqlExpr.WindowCall.Frame.Bound.Preceding(2), new SqlExpr.WindowCall.Frame.Bound.Following(3),
            new SqlExpr.WindowCall.Frame.Bound.UnboundedFollowing()};
        for (SqlExpr.WindowCall.Frame.Bound b : others) {
            boolean before = b instanceof SqlExpr.WindowCall.Frame.Bound.UnboundedPreceding
                    || b instanceof SqlExpr.WindowCall.Frame.Bound.Preceding;
            e(new SqlExpr.WindowCall(sum, List.of(), List.of(asc(c("ts"))), new SqlExpr.WindowCall.Frame(
                    SqlExpr.WindowCall.Frame.Kind.RANGE,
                    before ? b : new SqlExpr.WindowCall.Frame.Bound.IntervalPreceding(1, "DAYS"),
                    before ? new SqlExpr.WindowCall.Frame.Bound.IntervalFollowing(2, "HOURS") : b)));
        }
        e(new SqlExpr.WindowCall(sum, List.of(), List.of(asc(c("ts")), asc(c("n"))), new SqlExpr.WindowCall.Frame(
                SqlExpr.WindowCall.Frame.Kind.RANGE, new SqlExpr.WindowCall.Frame.Bound.IntervalPreceding(1, "DAYS"),
                new SqlExpr.WindowCall.Frame.Bound.CurrentRow())));
        // the format codes: a parsed date's (formatDateTime's letters), and a formatted one's in Joda's
        for (DateFmt.Part part : List.of(DateFmt.Part.HOUR12_NOPAD, DateFmt.Part.SUBSEC_MIN,
                DateFmt.Part.WEEKDAY_NAME)) {
            e(call(SqlFn.STRPTIME, c("s"), fmt(part)));
        }
        e(call(SqlFn.STRFTIME, c("ts"), fmt(DateFmt.Part.YEAR4, new DateFmt.Text("-"), DateFmt.Part.MONTH2,
                new DateFmt.Text("-"), DateFmt.Part.DAY2, new DateFmt.Text(" o'clock "), DateFmt.Part.HOUR2,
                new DateFmt.Text(":"), DateFmt.Part.MIN2, new DateFmt.Text(":"), DateFmt.Part.SEC2,
                DateFmt.Part.SUBSEC_MICRO, new DateFmt.Text(" "), DateFmt.Part.HOUR12_NOPAD, DateFmt.Part.AMPM,
                DateFmt.Part.MONTH_ABBREV, DateFmt.Part.MONTH_NAME, DateFmt.Part.WEEKDAY_NAME, DateFmt.Part.HOUR12)));
        e(call(SqlFn.STRFTIME, c("ts"), fmt(DateFmt.Part.HOUR12_NOPAD, DateFmt.Part.SUBSEC_MIN, DateFmt.Part.SEC2)));
        // a DML row is written without the passes (AnsiSqlRenderer.render(SqlDml)'s InsertValues arm writes its rows'
        // expressions directly): a Float's print forms reach the call switch there, unexpanded
        for (String op : List.of("dml.render", "dml.renderStatement")) {
            add(op, new SqlDml.InsertValues(null, "t", NameQuoting.BARE, List.of("a", "b", "c", "d"), Set.of(),
                    List.of(List.of(call(SqlFn.FLOAT_CANON, f(1.5)), call(SqlFn.FLOAT_PRINT, f(2.5)),
                            call(SqlFn.FLOAT_PRINT, cast(str("2.5"), DOUBLE)), call(SqlFn.FLOAT_CANON, dec("1.25"))))));
        }
    }

    /** Snowflake's and BigQuery's own arms no other input reaches: a raise in each slot type, the walls of their
     *  lambdas, list reductions, formats, regular expressions, windows, as-of joins, stars and VALUES, and the float
     *  spellings a DML row reaches (a row is written with no pass, so FLOAT_CANON and FLOAT_PRINT are not expanded). */
    static void snowflakeBigQueryArms() {
        SqlExpr xs = c("xs");
        SqlExpr untyped = SqlExpr.Column.derived("t0", "xs");
        // a raise as its projection's or its CASE's slot type
        SqlType tagged = new SqlType.Tagged(List.of(new SqlType.Tagged.Slot("Integer", BIGINT)));
        for (SqlType slot : List.of(DEC, TEMPORAL_TEXT, SqlType.Scalar.DECIMAL_TEXT, tagged)) {
            q(select(t(), p(call(SqlFn.ERROR, str("bad")), "v", slot)));
        }
        e(new SqlExpr.Case(List.of(new SqlExpr.Case.When(c("b"), c("dec"))), call(SqlFn.ERROR, str("no"))));
        // the format's walls: more directives than arguments
        e(call(SqlFn.FORMAT, str("%s and %s"), c("s")));
        // a lookahead and a named group in a POSIX pattern; a first-match replacement reading group 8
        e(call(SqlFn.MATCHES, c("s"), str("a(?=b)")));
        e(call(SqlFn.REGEXP_EXTRACT, c("s"), str("(?<n>a)b"), i(1)));
        e(call(SqlFn.REGEXP_REPLACE, c("s"), str("ab"), str("z"), str("ic")));
        e(call(SqlFn.REGEXP_REPLACE, c("s"), str("a(b)"), str("x\\8")));
        // date formats as a parse format: a weekday name, an unpadded hour, a fraction alone, a double quote
        for (DateFmt.Part part : List.of(DateFmt.Part.WEEKDAY_NAME, DateFmt.Part.HOUR12_NOPAD,
                DateFmt.Part.SUBSEC_MICRO, DateFmt.Part.SUBSEC_NANO, DateFmt.Part.SUBSEC_MIN)) {
            e(call(SqlFn.STRPTIME, c("s"), fmt(part)));
        }
        e(call(SqlFn.STRPTIME, c("s"), fmt(DateFmt.Part.YEAR4, new DateFmt.Text("\""))));
        // lambdas: an inner two-parameter lambda reading the outer parameter; three parameters over a list
        e(call(SqlFn.LIST_TRANSFORM, xs, lam(xs, "x", x -> call(SqlFn.LIST_FILTER, c("ss"), lambda(call(SqlFn.EQUAL,
                derived("y"), cast(x, VARCHAR)), "y", "k")))));
        e(call(SqlFn.LIST_FILTER, xs, lambda(bool(true), "a", "b", "c")));
        e(call(SqlFn.LIST_TRANSFORM, nul(), lambda(derived("x"), "x")));
        e(call(SqlFn.LIST_FILTER, nul(), lambda(call(SqlFn.GREATER, derived("x"), derived("y")), "x")));
        // list reductions over a list of unknown element type, of text, with the wrong extras
        e(new SqlExpr.ReduceCollection(SqlAgg.Fn.SUM, untyped, List.of()));
        e(new SqlExpr.ReduceCollection(SqlAgg.Fn.MODE, untyped, List.of()));
        e(new SqlExpr.ReduceCollection(SqlAgg.Fn.MODE, c("ss"), List.of(str(","))));
        e(new SqlExpr.ReduceCollection(SqlAgg.Fn.QUANTILE_CONT, c("ss"), List.of(f(0.5))));
        e(new SqlExpr.ReduceCollection(SqlAgg.Fn.QUANTILE_CONT, xs, List.of()));
        e(new SqlExpr.ReduceCollection(SqlAgg.Fn.STRING_AGG, c("ss"), List.of()));
        e(call(SqlFn.LIST_SUM, untyped));
        e(call(SqlFn.LIST_REDUCE, untyped, lambda(call(SqlFn.PLUS, derived("acc"), derived("x")), "acc", "x")));
        e(call(SqlFn.LIST_SLICE, xs, i(-2), i(-1)));
        e(call(SqlFn.MAP_FROM_LISTS, xs, c("ss")));
        // lists of lists: appended, repeated, an empty one typed, an exploded NULL list's column typed as one
        e(call(SqlFn.LIST_APPEND, c("xss"), xs));
        e(call(SqlFn.REPEAT_VALUE, xs, i(2)));
        e(new SqlExpr.ArrayLit(List.of(), new TypeFact.Typed(XSS, false)));
        q(select(sub(marker(p(call(SqlFn.UNNEST, nul()), "a"), p(call(SqlFn.UNNEST, arr(i(1), i(2))), "b")), "u"),
                p(SqlExpr.Column.of("u", "a", XSS, true, OutputCol.Origin.DERIVED), "a"),
                p(SqlExpr.Column.derived("u", "b"), "b")));
        // windows: a frame without an ORDER BY; NTH_VALUE of one and of three arguments; a DISTINCT string aggregate
        // and a two-argument LIST over an ordered window; a DISTINCT quantile; a descending median; a RANGE offset
        // over two keys
        List<SqlExpr> part = List.of(c("s"));
        List<SqlSelect.SortKey> order = List.of(asc(c("n")));
        SqlAgg.Reducer sum = new SqlAgg.Reducer(SqlAgg.Fn.SUM, List.of(c("n")), false, List.of());
        e(new SqlExpr.WindowCall(sum, part, List.of(), new SqlExpr.WindowCall.Frame(SqlExpr.WindowCall.Frame.Kind.ROWS,
                new SqlExpr.WindowCall.Frame.Bound.Preceding(2), new SqlExpr.WindowCall.Frame.Bound.CurrentRow())));
        e(new SqlExpr.WindowCall(new SqlAgg.ValueFn(SqlAgg.Fn.NTH_VALUE, List.of(c("s"))), part, order, null));
        e(new SqlExpr.WindowCall(new SqlAgg.ValueFn(SqlAgg.Fn.NTH_VALUE, List.of(c("s"), i(2), i(3))), part, order,
                null));
        e(new SqlExpr.WindowCall(new SqlAgg.Reducer(SqlAgg.Fn.STRING_AGG, List.of(c("s"), str(",")), true, List.of()),
                part, order, null));
        e(new SqlExpr.WindowCall(new SqlAgg.Reducer(SqlAgg.Fn.LIST, List.of(c("n"), c("s")), false, List.of()), part,
                order, null));
        e(new SqlExpr.WindowCall(new SqlAgg.Reducer(SqlAgg.Fn.QUANTILE_CONT, List.of(c("d"), f(0.5)), true,
                List.of()), part, List.of(), null));
        e(new SqlExpr.WindowCall(new SqlAgg.Reducer(SqlAgg.Fn.MEDIAN, List.of(c("d")), false, List.of(desc(c("d")))),
                part, List.of(), null));
        e(new SqlExpr.WindowCall(sum, part, List.of(asc(c("n")), asc(c("d"))), new SqlExpr.WindowCall.Frame(
                SqlExpr.WindowCall.Frame.Kind.RANGE, new SqlExpr.WindowCall.Frame.Bound.Preceding(1),
                new SqlExpr.WindowCall.Frame.Bound.CurrentRow())));
        // a MODE grouped over a join (a Join's alias() throws first: SqlSource.java's Join), and over a source that
        // declares no column
        q(select(new SqlSource.Join(t(), t("t1"), SqlSource.Join.Kind.INNER, call(SqlFn.EQUAL, c("n"), c("t1", "n"))),
                List.of(p(c("s"), "s"), p(reducer(SqlAgg.Fn.MODE, c("t1", "s")), "m")), null, List.of(c("s")),
                List.of(), null, null));
        q(select(new SqlSource.Table("t", "t0", List.of(), false, Map.of(), NameQuoting.BARE), List.of(p(c("s"), "s"),
                p(reducer(SqlAgg.Fn.MODE, c("n")), "m")), null, List.of(c("s")), List.of(), null, null));
        // a lateral join of a table; as-of joins of no inequality and of two
        q(select(new SqlSource.Join(t(), t("t1"), SqlSource.Join.Kind.CROSS_LATERAL, null), p(c("s"), "s")));
        q(select(new SqlSource.Join(t(), t("t1"), SqlSource.Join.Kind.ASOF_LEFT, call(SqlFn.EQUAL, c("s"),
                c("t1", "s"))), p(c("s"), "s")));
        q(select(new SqlSource.Join(t(), t("t1"), SqlSource.Join.Kind.ASOF_LEFT, call(SqlFn.AND, call(SqlFn.LESS,
                c("ts"), c("t1", "ts")), call(SqlFn.GREATER, c("dt"), c("t1", "dt")))), p(c("s"), "s")));
        // a star over an alias the FROM does not bind, beside a column, over an explosion
        SqlSelect lm = marker(p(call(SqlFn.UNNEST, xs), "e"));
        q(select(new SqlSource.Join(t(), sub(lm, "u"), SqlSource.Join.Kind.CROSS_LATERAL, null),
                new SqlSelect.Projection(new SqlExpr.Star("zz"), null, null), p(c("n"), "n")));
        // VALUES: one column of each literal kind (and of a column read), a DOUBLE column of exact and inexact cells,
        // a column without its declared type, a column typed as a tagged value
        List<SqlExpr> cells = List.of(f(1.5), dec("2.25"), bool(true), new SqlExpr.TimestampLit("2024-01-02T03:04:05"),
                new SqlExpr.TimeLit("03:04:05"), c("s"));
        for (SqlExpr cell : cells) {
            SqlType ty = cell.type() instanceof TypeFact.Typed tf ? tf.type() : VARCHAR;
            q(select(new SqlSource.Values(List.of(List.of(cell)), List.of("a"), "v0", List.of(oc("a", ty)))));
        }
        q(select(new SqlSource.Values(List.of(List.of(i(1)), List.of(nul()), List.of(dec("1.5")), List.of(f(2.5)),
                List.of(call(SqlFn.PLUS, i(1), i(2)))), List.of("a"), "v0", List.of(oc("a", DOUBLE)))));
        e(new SqlExpr.ArrayLit(List.of(i(1), dec("1.5"), f(2.5)), new TypeFact.Typed(DS, false)));
        q(select(new SqlSource.Values(List.of(List.of(i(1), str("x"))), List.of("a", "b"), "v0",
                List.of(oc("a", BIGINT)))));
        q(select(new SqlSource.Values(List.of(List.of(i(1))), List.of("a"), "v0", List.of(oc("a", tagged)))));
        // static pivots on a boolean value and on a decimal one (no column name)
        for (SqlExpr v : List.of(bool(true), dec("1.5"))) {
            q(select(new SqlSource.Pivot(t(), List.of(c("b")), List.of(v), List.of(), List.of(
                    new SqlSource.Pivot.Using(new SqlAgg.Reducer(SqlAgg.Fn.SUM, List.of(c("n")), false, List.of()),
                            "q")), "p0", List.of(oc("s", VARCHAR)))));
        }
        // FLOAT_CANON and FLOAT_PRINT in a DML row, which no pass expands
        for (String op : List.of("dml.render", "dml.renderStatement")) {
            add(op, new SqlDml.InsertValues(null, "t", NameQuoting.BARE, List.of("a", "b", "c", "d"), Set.of(),
                    List.of(List.of(call(SqlFn.FLOAT_CANON, f(1.5)), call(SqlFn.FLOAT_PRINT, f(2.5)),
                            call(SqlFn.FLOAT_CANON, c("d")), call(SqlFn.FLOAT_PRINT, dec("1.25"))))));
        }
    }

    /** MongoDb's arms the shapes above leave: the expression writer's nodes met where no stage of the pipeline takes
     *  them (a WHERE, a join's ON, a bare projection), the window outputs and ranges, the as-of joins, a lookup over
     *  each source a right join looks up, the pivots by value kind, the cast to JSON text, the insert's values. */
    static void mongoArms() {
        // nodes the pipeline computes in its own stages, met in a WHERE (matched before the group and window
        // stages): the expression writer's own arms
        w(new SqlExpr.StarExcept("t0", List.of("s")));
        w(call(SqlFn.GREATER, new SqlExpr.WindowCall(new SqlAgg.Reducer(SqlAgg.Fn.SUM, List.of(c("n")), false,
                List.of()), List.of(c("s")), List.of(asc(c("n"))), null), i(0)));
        w(call(SqlFn.IS_NOT_NULL, new SqlExpr.OrderedListAgg(c("s"), c("n"))));
        w(call(SqlFn.IS_NOT_NULL, new SqlExpr.JsonArrayAgg(c("n"))));
        for (SqlAgg.Fn fn : SqlAgg.Fn.values()) {
            w(call(SqlFn.IS_NOT_NULL, new SqlAgg.Reducer(fn, List.of(aggArgs(fn)), false, List.of())));
        }
        w(call(SqlFn.GREATER, call(SqlFn.UNNEST, c("xs")), i(0)));
        // a date part and a lambda outside the call that takes them
        e(part(DatePart.DAY));
        e(lambda(call(SqlFn.PLUS, derived("x"), i(1)), "x"));
        // an unnest inside an expression (not a projection's own)
        e(call(SqlFn.PLUS, call(SqlFn.UNNEST, c("xs")), i(1)));
        // subqueries in a join's ON, which no lookup stage computes
        SqlSelect inner = select(t("t2"), p(c("t2", "n"), "n"));
        SqlExpr on = call(SqlFn.EQUAL, c("n"), c("t1", "n"));
        for (SqlExpr cond : List.of(new SqlExpr.InSubquery(c("n"), inner), new SqlExpr.Quantified(c("n"),
                SqlFn.GREATER, SqlExpr.Quantifier.ANY, inner), call(SqlFn.EQUAL, c("t1", "n"),
                new SqlExpr.ScalarSubquery(inner)), new SqlExpr.Exists(inner))) {
            q(select(new SqlSource.Join(t(), t("t1"), SqlSource.Join.Kind.INNER, call(SqlFn.AND, on, cond)),
                    p(c("s"), "s")));
        }
        // a scalar subquery of two columns
        e(new SqlExpr.ScalarSubquery(select(t("t1"), p(c("t1", "n"), "a"), p(c("t1", "s"), "b"))));
        // a dynamic pivot's per-key column read inside a lookup (a correlated subquery's condition)
        SqlExpr perKey = new SqlExpr.Column("t0", "k", new TypeFact.Typed(BIGINT, true), OutputCol.Origin.PER_KEY);
        q(where(new SqlExpr.Exists(select(t("t1"), List.of(p(c("t1", "n"), "n")), call(SqlFn.EQUAL, c("t1", "n"),
                perKey), List.of(), List.of(), null, null)), p(c("s"), "s")));
        // window functions no operator computes, and a lag by a computed offset
        List<SqlExpr> part = List.of(c("s"));
        List<SqlSelect.SortKey> order = List.of(asc(c("n")));
        e(new SqlExpr.WindowCall(new SqlAgg.RankingFn(SqlAgg.Fn.SUM, List.of(c("n"))), part, order, null));
        e(new SqlExpr.WindowCall(new SqlAgg.ValueFn(SqlAgg.Fn.SUM, List.of(c("n"))), part, order, null));
        e(new SqlExpr.WindowCall(new SqlAgg.ValueFn(SqlAgg.Fn.LAG, List.of(c("n"), c("n2"))), part, order, null));
        // range windows by offsets: in each unit, in two units of fixed length, in a month and a day, over two keys
        SqlAgg.Reducer sum = new SqlAgg.Reducer(SqlAgg.Fn.SUM, List.of(c("n")), false, List.of());
        for (String unit : List.of("YEAR", "MONTH", "WEEK", "DAY", "HOUR", "MINUTE", "SECOND", "MILLISECOND")) {
            e(new SqlExpr.WindowCall(sum, part, List.of(asc(c("ts"))), new SqlExpr.WindowCall.Frame(
                    SqlExpr.WindowCall.Frame.Kind.RANGE, new SqlExpr.WindowCall.Frame.Bound.IntervalPreceding(2, unit),
                    new SqlExpr.WindowCall.Frame.Bound.IntervalFollowing(1, unit))));
        }
        e(new SqlExpr.WindowCall(sum, part, List.of(desc(c("ts"))), new SqlExpr.WindowCall.Frame(
                SqlExpr.WindowCall.Frame.Kind.RANGE, new SqlExpr.WindowCall.Frame.Bound.IntervalPreceding(1, "HOURS"),
                new SqlExpr.WindowCall.Frame.Bound.IntervalFollowing(30, "MINUTES"))));
        e(new SqlExpr.WindowCall(sum, part, List.of(asc(c("ts"))), new SqlExpr.WindowCall.Frame(
                SqlExpr.WindowCall.Frame.Kind.RANGE, new SqlExpr.WindowCall.Frame.Bound.IntervalPreceding(1, "MONTHS"),
                new SqlExpr.WindowCall.Frame.Bound.IntervalFollowing(2, "DAYS"))));
        e(new SqlExpr.WindowCall(sum, part, List.of(asc(c("n")), asc(c("d"))), new SqlExpr.WindowCall.Frame(
                SqlExpr.WindowCall.Frame.Kind.RANGE, new SqlExpr.WindowCall.Frame.Bound.Preceding(1),
                new SqlExpr.WindowCall.Frame.Bound.Following(1))));
        // as-of joins: a left key greater than the right one; an equality for a match; operands under a checked
        // narrowing, parentheses and a computation; a match on two left columns
        SqlSource r = t("t1");
        SqlExpr keys = call(SqlFn.EQUAL, c("s"), c("t1", "s"));
        for (SqlExpr match : List.of(call(SqlFn.GREATER, c("ts"), c("t1", "ts")), call(SqlFn.EQUAL, c("ts"),
                c("t1", "ts")), call(SqlFn.LESS_EQUAL, new SqlExpr.Group(c("ts")), new SqlExpr.CheckedOne(c("t1",
                "tss"))), call(SqlFn.LESS, call(SqlFn.PLUS, c("n"), i(1)), c("t1", "n")), call(SqlFn.LESS, c("ts"),
                c("d")))) {
            q(select(new SqlSource.Join(t(), r, SqlSource.Join.Kind.ASOF_LEFT, call(SqlFn.AND, keys, match)),
                    p(c("s"), "s"), p(c("t1", "d"), "d")));
        }
        // a join whose right side is a join; a right join over a joined left side
        SqlSource.Join twelve = new SqlSource.Join(t("t1"), t("t2"), SqlSource.Join.Kind.INNER, call(SqlFn.EQUAL,
                c("t1", "n"), c("t2", "n")));
        q(select(new SqlSource.Join(t(), twelve, SqlSource.Join.Kind.INNER, on), p(c("s"), "s")));
        q(select(new SqlSource.Join(new SqlSource.Join(t(), t("t1"), SqlSource.Join.Kind.INNER, on), t("t2"),
                SqlSource.Join.Kind.RIGHT, call(SqlFn.EQUAL, c("t1", "n"), c("t2", "n"))), p(c("s"), "s")));
        // a right join looks its right side up first: each source a lookup cannot read
        List<OutputCol> so = List.of(oc("a", BIGINT));
        SqlExpr ra = SqlExpr.Column.of("r0", so, "a");
        List<SqlSource> odd = List.of(new SqlSource.Cte("w", "r0", so), new SqlSource.VarSetPlaceholder("v", "r0", so),
                new SqlSource.RawSql("SELECT 1 AS a", "r0", so, "H2"), new SqlSource.Catalog(
                        SqlSource.Catalog.Grid.TABLES, "S%", "T%", null, "r0", so),
                new SqlSource.SourceUrl("file:///data/x.csv", "r0", so), sub(new SqlWith(List.of(new SqlWith.Cte("w",
                        select(t("t5"), p(c("t5", "n"), "a")), false)), select(new SqlSource.Cte("w", "w0", so),
                        p(SqlExpr.Column.of("w0", so, "a"), "a"))), "r0"));
        for (SqlSource src : odd) {
            q(select(new SqlSource.Join(t(), src, SqlSource.Join.Kind.RIGHT, call(SqlFn.EQUAL, c("n"), ra)),
                    p(c("s"), "s")));
        }
        // a CTE read with no WITH around it
        q(select(new SqlSource.Cte("w", "w0", so)));
        // pivots: values of each literal kind naming the columns, named values, a named value that is no literal
        List<SqlSource.Pivot.Using> usings = List.of(new SqlSource.Pivot.Using(sum, "q"));
        List<OutputCol> po = List.of(oc("s", VARCHAR), new OutputCol("1__|__q", BIGINT, true,
                OutputCol.Origin.DERIVED), new OutputCol("2.5__|__q", BIGINT, true, OutputCol.Origin.DERIVED),
                new OutputCol("true__|__q", BIGINT, true, OutputCol.Origin.DERIVED));
        q(select(new SqlSource.Pivot(t(), List.of(c("s2")), List.of(i(1), f(2.5), bool(true)), List.of(), usings,
                "p0", po)));
        q(select(new SqlSource.Pivot(t(), List.of(c("n")), List.of(), List.of(i(1), f(2.5), bool(false), str("x")),
                usings, "p0", List.of(oc("s", VARCHAR)))));
        q(select(new SqlSource.Pivot(t(), List.of(c("dt")), List.of(), List.of(new SqlExpr.DateLit("2024-01-02")),
                usings, "p0", List.of(oc("s", VARCHAR)))));
        // JSON objects: a computed key; written as text (an object, an array, and an object with a computed key)
        e(new SqlExpr.JsonObject(List.of(c("s"), c("n"))));
        e(cast(new SqlExpr.JsonObject(List.of(str("a"), c("n"), str("b"), c("s"))), VARCHAR));
        e(cast(new SqlExpr.JsonArray(List.of(c("n"), c("s"), c("dt"))), VARCHAR));
        e(cast(new SqlExpr.JsonObject(List.of(c("s"), c("n"))), VARCHAR));
        // a membership in a parameter that is no typed list
        w(new SqlExpr.Membership(c("n"), new SqlExpr.PlanParam("ns", SqlExpr.PlanParam.Kind.OTHER, false, null)));
        // a format with fewer arguments than directives
        e(call(SqlFn.FORMAT, str("%s and %s"), c("s")));
        // each composed date part in a format written as its codes
        for (DateFmt.Part fp : List.of(DateFmt.Part.SUBSEC_MIN, DateFmt.Part.MONTH_NAME, DateFmt.Part.WEEKDAY_NAME,
                DateFmt.Part.HOUR12, DateFmt.Part.HOUR12_NOPAD, DateFmt.Part.AMPM)) {
            e(fmt(fp));
        }
        // a JSON array aggregate with no grouping: its value over no row
        q(select(t(), p(new SqlExpr.JsonArrayAgg(c("n")), "v")));
        // the insert's values of each kind, an insert naming no column, a collection with a dot, the default schema
        List<SqlExpr> cells = List.of(str("x"), i(1), f(2.5), dec("1.50"), bool(true), nul(),
                new SqlExpr.DateLit("2024-01-02"), new SqlExpr.TimestampLit("2024-01-02T03:04:05.123"),
                cast(str("1"), BIGINT), call(SqlFn.PLUS, i(1), i(2)));
        List<String> names = new ArrayList<>();
        for (int k = 0; k < cells.size(); k++) {
            names.add("c" + k);
        }
        for (String op : List.of("dml.render", "dml.renderStatement")) {
            add(op, new SqlDml.InsertValues(null, "people", NameQuoting.BARE, names, Set.of(), List.of(cells)));
            add(op, new SqlDml.InsertValues(null, "people", NameQuoting.BARE, List.of(), Set.of(), List.of()));
            add(op, new SqlDml.DeleteAll(null, "a.b", NameQuoting.BARE));
            add(op, new SqlDml.DeleteAll("default", "people", NameQuoting.BARE));
        }
        add("ddl.render", new SqlDdl.DropTable("default", "people", NameQuoting.BARE));
        add("ddl.render", new SqlDdl.CreateTable(null, "people", NameQuoting.BARE, List.of(new SqlDdl.Column("a",
                false, new SqlDdl.ColumnType.Plain(SqlDdl.ColumnType.Kind.BIGINT), false, false)), false));
    }

    /** MariaDB's and MySQL's own arms the shapes above leave: JSON_TABLE column types, the element-wise casts, the
     *  raise's cast targets, printf's conversions, regexp options and group limits, the lateral rows' walls, counted
     *  modes over a join, folds and reductions with no accumulator type, ordered and multi-key RANGE windows. */
    static void mariaArms() {
        // a list of each scalar the JSON_TABLE column types name (columnType), read as rows
        for (SqlType el : List.of(INTEGER, HUGEINT, TIMESTAMPTZ, TIME, TEMPORAL_TEXT, SqlType.Scalar.DECIMAL_TEXT)) {
            SqlExpr xs = mariaCol("l", new SqlType.Array(el));
            e(call(SqlFn.LIST_DISTINCT, xs));
            e(call(SqlFn.LIST_MAX, xs));
            e(call(SqlFn.LIST_FILTER, xs, lam(xs, "x", x -> call(SqlFn.IS_NOT_NULL, x))));
        }
        // a struct's JSON field as a sort key (scalarOfText of JSON)
        SqlType sj = new SqlType.Struct(List.of(new SqlType.Struct.Field("a", BIGINT), new SqlType.Struct.Field("j",
                JSON)));
        e(call(SqlFn.LIST_SORT, mariaCol("l", new SqlType.Array(sj))));
        e(call(SqlFn.LIST_SORT_DESC, mariaCol("l", new SqlType.Array(sj))));
        // element-wise casts to the carriers (castTo's TEMPORAL_TEXT and DECIMAL_TEXT)
        e(cast(c("ss"), new SqlType.Array(TEMPORAL_TEXT)));
        e(cast(c("xs"), new SqlType.Array(SqlType.Scalar.DECIMAL_TEXT)));
        // a raise typed as a carrier (castTarget's TEMPORAL_TEXT and DECIMAL_TEXT)
        e(new SqlExpr.Case(List.of(new SqlExpr.Case.When(c("b"), cast(c("s"), TEMPORAL_TEXT))),
                call(SqlFn.ERROR, str("no"))));
        e(new SqlExpr.Case(List.of(new SqlExpr.Case.When(c("b"), cast(c("s"), SqlType.Scalar.DECIMAL_TEXT))),
                call(SqlFn.ERROR, str("no"))));
        // printf's other conversions; MySQL's refuses %E after the four it spells, and a conversion with no argument
        e(call(SqlFn.FORMAT, str("%i|%+5F|%X|%o"), c("n"), c("d"), c("n"), c("n")));
        e(call(SqlFn.FORMAT, str("%i|%F|%X|%o|%E|%g|%G"), c("n"), c("d"), c("n"), c("n"), c("d"), c("d"), c("d")));
        e(call(SqlFn.FORMAT, str("%s and %s"), c("s")));
        // regexp_replace's 'c' option; a first-match group past \7; an extract-all group past \9
        e(call(SqlFn.REGEXP_REPLACE, c("s"), str("ab"), str("z"), str("c")));
        e(call(SqlFn.REGEXP_REPLACE, c("s"), str("(a)(b)"), str("\\8")));
        e(call(SqlFn.REGEXP_EXTRACT_ALL, c("s"), str("(a)(b)(c)(d)(e)(f)(g)(h)"), i(1)));
        // a VALUES row is written with no pass run over it: a Float's print and canonical forms reach the call
        for (String op : List.of("dml.render", "dml.renderStatement")) {
            add(op, new SqlDml.InsertValues(null, "t", NameQuoting.BARE, List.of("a", "b"), Set.of(), List.of(List.of(
                    call(SqlFn.FLOAT_PRINT, f(1.5)), call(SqlFn.FLOAT_CANON, f(2.5))))));
        }
        // a star exclusion the pass does not expand (inside an expression)
        e(new SqlExpr.Group(new SqlExpr.StarExcept("t0", List.of("s"))));
        // an as-of join with no inequality: the carrier strategies' emulation declines it
        q(select(new SqlSource.Join(t(), t("t1"), SqlSource.Join.Kind.ASOF_LEFT, call(SqlFn.EQUAL, c("s"),
                c("t1", "s"))), p(c("s"), "s"),
                p(c("t1", "d"), "d")));
        // a windowed MODE over a join and over no FROM (each source refuses its alias before the counts' wall); a
        // grouped one over a join (MySQL counts it)
        SqlExpr on = call(SqlFn.EQUAL, c("n"), c("t1", "n"));
        q(select(new SqlSource.Join(t(), t("t1"), SqlSource.Join.Kind.INNER, on), p(c("s"), "s"),
                p(new SqlExpr.WindowCall(mariaAgg(SqlAgg.Fn.MODE, c("t1", "s")), List.of(c("s")), List.of(), null),
                        "m")));
        q(select(new SqlSource.Dual(), p(new SqlExpr.WindowCall(mariaAgg(SqlAgg.Fn.MODE, i(1)), List.of(), List.of(),
                null), "m")));
        q(select(new SqlSource.Join(t(), t("t1"), SqlSource.Join.Kind.INNER, on), List.of(p(c("s"), "s"),
                p(reducer(SqlAgg.Fn.MODE, c("t1", "s")), "m")), null, List.of(c("s")), List.of(), null, null));
        // a windowed MODE over a source that declares no columns (its counts need them; a join's or no FROM's alias
        // is refused by the source itself first); a grouped one there (MySQL counts it)
        SqlSource bare = new SqlSource.Table("t", "t0", List.of(), false, Map.of(), NameQuoting.BARE);
        q(select(bare, p(new SqlExpr.WindowCall(mariaAgg(SqlAgg.Fn.MODE, c("s")), List.of(c("s2")), List.of(), null),
                "m")));
        q(select(bare, List.of(p(c("s"), "s"), p(reducer(SqlAgg.Fn.MODE, c("s2")), "m")), null, List.of(c("s")),
                List.of(), null, null));
        // a collection's mode given an argument
        e(new SqlExpr.ReduceCollection(SqlAgg.Fn.MODE, c("ss"), List.of(str(","))));
        // lateral joins: to a table, to a subquery of no declared columns, to one whose projections do not match
        q(select(new SqlSource.Join(t(), t("t1"), SqlSource.Join.Kind.CROSS_LATERAL, null), p(c("s"), "s")));
        SqlExpr corr = call(SqlFn.EQUAL, c("t1", "s"), c("s"));
        SqlSelect undeclared = select(t("t1"), List.of(new SqlSelect.Projection(c("t1", "n"), "k", null)), corr,
                List.of(), List.of(), null, null);
        q(select(new SqlSource.Join(t(), sub(undeclared, "r"), SqlSource.Join.Kind.CROSS_LATERAL, null),
                p(c("s"), "s")));
        SqlSelect unmatched = select(t("t1"), List.of(p(c("t1", "n"), "k"), new SqlSelect.Projection(c("t1", "s"),
                "x", null)), corr, List.of(), List.of(), null, null);
        q(select(new SqlSource.Join(t(), sub(unmatched, "r"), SqlSource.Join.Kind.LEFT_LATERAL, bool(true)),
                p(c("s"), "s"), p(SqlExpr.Column.derived("r", "k"), "k")));
        // a closed fold and a closed reduction whose accumulators claim no type
        e(new SqlExpr.FoldCall(arr(i(1), i(2)), (SqlExpr.Lambda) lambda(call(SqlFn.PLUS, derived("x"),
                derived("acc")), "x", "acc"), nul(), false, true));
        e(call(SqlFn.LIST_REDUCE, arr(nul(), nul()), lambda(call(SqlFn.PLUS, derived("acc"), derived("x")), "acc",
                "x")));
        // an ordered SUM over an ordered window; an interval RANGE frame over two keys
        e(new SqlExpr.WindowCall(new SqlAgg.Reducer(SqlAgg.Fn.SUM, List.of(c("n")), false, List.of(desc(c("d")))),
                List.of(c("s")), List.of(asc(c("n"))), null));
        e(new SqlExpr.WindowCall(mariaAgg(SqlAgg.Fn.SUM, c("n")), List.of(), List.of(asc(c("dt")), asc(c("n"))),
                new SqlExpr.WindowCall.Frame(SqlExpr.WindowCall.Frame.Kind.RANGE,
                        new SqlExpr.WindowCall.Frame.Bound.IntervalPreceding(3, "DAYS"),
                        new SqlExpr.WindowCall.Frame.Bound.CurrentRow())));
    }

    /** An unordered, non-distinct aggregate, as a window's function. */
    static SqlAgg.Reducer mariaAgg(SqlAgg.Fn fn, SqlExpr... args) {
        return new SqlAgg.Reducer(fn, List.of(args), false, List.of());
    }

    /** A column of the table of any type (the shared table declares none of it). */
    static SqlExpr mariaCol(String name, SqlType type) {
        return SqlExpr.Column.of("t0", name, type, true, OutputCol.Origin.PHYSICAL);
    }

    static void engineText() {
        for (String s : List.of("CREATE SCHEMA IF NOT EXISTS OLAP", "select \"root\".ID as \"id\" from PERSON as"
                + " \"root\"", "insert into PERSON (ID, NAME) values (1, 'a');", "Drop table if exists PERSON;")) {
            for (Map.Entry<String, Supplier<SqlDialect>> d : DIALECTS.entrySet()) {
                String out;
                try {
                    out = "{\"ok\":" + quote(d.getValue().get().fromEngineText(s)) + "}";
                } catch (RuntimeException e) {
                    out = refusal(e);
                }
                line(d.getKey(), "fromEngineText", "String", quote(s), out);
            }
            inputs++;
        }
    }

    // ---- the replay of the dump

    /** The dump's lines of the eight dialects, answered again on fresh dialects and held to the dump's answers. */
    static void replay(Path dump) throws IOException {
        int replayed = 0;
        int differ = 0;
        for (String line : Files.readAllLines(dump, StandardCharsets.UTF_8)) {
            Json.Obj o = (Json.Obj) Json.parse(line, DEEP);
            String dialect = o.getString("dialect");
            if (!DIALECTS.containsKey(dialect)) {
                continue;
            }
            String op = o.getString("op");
            int at = line.indexOf(",\"mir\":");
            int end = line.lastIndexOf(",\"answer\":");
            String mir = line.substring(at + 7, end);
            String want = line.substring(end + 10, line.length() - 1);
            Object read = reread(op, mir);
            hook(dialect);
            String got = answer(DIALECTS.get(dialect).get(), op, read);
            replayed++;
            if (!got.equals(want)) {
                differ++;
                System.out.println("replay differs: " + dialect + " " + op + "\n  dump: " + want + "\n  now:  " + got);
            }
        }
        System.out.println(replayed + " dump lines replayed, " + differ + " different");
    }

    // ---- the line

    static void add(String op, Object input) {
        String mir;
        Object read;
        try {
            mir = mirOf(input);
            read = reread(op, mir);
            String again = mirOf(read);
            if (!again.equals(mir)) {
                throw new IllegalStateException("MirJson does not write back its own text");
            }
        } catch (RuntimeException e) {
            refusedByMirJson++;
            REFUSED.add(input.getClass().getSimpleName() + ": " + e);
            return;
        }
        String kind = read.getClass().getSimpleName();
        boolean fresh = false;
        for (Map.Entry<String, Supplier<SqlDialect>> d : DIALECTS.entrySet()) {
            String key = d.getKey() + "\u0000" + op + "\u0000" + mir;
            if (!SEEN.add(key)) {
                continue;
            }
            fresh = true;
            hook(d.getKey());
            String answer = answer(d.getValue().get(), op, read);
            if (answer.startsWith("{\"throws\":\"java.lang.ClassCastException\"")) {
                // Java's own defect, whose message names the JVM's class loader ("in unnamed module of loader
                // 'app'"): no port can be held to it, so it is no line (counted, and listed by the run)
                DEFECTS.add(d.getKey() + " " + op + " " + kind + ": " + answer);
                continue;
            }
            line(d.getKey(), op, kind, mir, answer);
        }
        if (fresh) {
            inputs++;
        }
    }

    static void line(String dialect, String op, String kind, String mir, String answer) {
        PER_DIALECT.merge(dialect, 1, Integer::sum);
        LINES.add("{\"dialect\":" + quote(dialect) + ",\"config\":null,\"fresh\":true,\"op\":" + quote(op)
                + ",\"kind\":" + quote(kind) + ",\"mir\":" + mir + ",\"answer\":" + answer + "}\n");
    }

    /** The coverage recorder's dialect, when rust/tools/dialect-arms-coverage.py put one on the class path. */
    static void hook(String dialect) {
        try {
            Class.forName("com.legend.sql.dialect.ArmHits").getMethod("dialect", String.class).invoke(null, dialect);
        } catch (ClassNotFoundException e) {
            // the plain run: nothing records
        } catch (ReflectiveOperationException e) {
            throw new IllegalStateException(e);
        }
    }

    static String answer(SqlDialect d, String op, Object input) {
        try {
            return "{\"ok\":" + answerOf(d, op, input) + "}";
        } catch (RuntimeException | StackOverflowError e) {
            return refusal(e);
        }
    }

    static String refusal(Throwable e) {
        return "{\"throws\":" + quote(e.getClass().getName()) + ",\"message\":" + quote(String.valueOf(
                e.getMessage())) + "}";
    }

    static String mirOf(Object input) {
        return switch (input) {
            case SqlQuery q -> Json.toCompact(MirJson.query(q));
            case SqlDdl d -> Json.toCompact(MirJson.ddl(d));
            case SqlDml d -> Json.toCompact(MirJson.dml(d));
            default -> throw new IllegalStateException("no MirJson for " + input.getClass());
        };
    }

    static Object reread(String op, String mir) {
        Json.Obj o = (Json.Obj) Json.parse(mir, DEEP);
        return switch (op.substring(0, op.indexOf('.'))) {
            case "query" -> MirJson.readQuery(o);
            case "ddl" -> MirJson.readDdl(o);
            case "dml" -> MirJson.readDml(o);
            default -> throw new IllegalStateException(op);
        };
    }

    static String answerOf(SqlDialect d, String op, Object input) {
        return switch (op) {
            case "query.render" -> quote(d.render((SqlQuery) input));
            case "query.renderStatement" -> statement(d.renderStatement((SqlQuery) input));
            case "ddl.render" -> quote(d.render((SqlDdl) input));
            case "dml.render" -> quote(d.render((SqlDml) input));
            case "dml.renderStatement" -> statement(d.renderStatement((SqlDml) input));
            default -> throw new IllegalStateException(op);
        };
    }

    static String statement(RenderedStatement s) {
        return "{\"sql\":" + quote(s.sql()) + ",\"binds\":" + quote(String.valueOf(s.binds())) + ",\"regions\":"
                + quote(String.valueOf(s.regions())) + "}";
    }

    static String quote(String s) {
        StringBuilder b = new StringBuilder("\"");
        for (char ch : s.toCharArray()) {
            switch (ch) {
                case '"' -> b.append("\\\"");
                case '\\' -> b.append("\\\\");
                case '\n' -> b.append("\\n");
                case '\r' -> b.append("\\r");
                case '\t' -> b.append("\\t");
                default -> {
                    if (ch < 0x20) {
                        b.append(String.format(java.util.Locale.ROOT, "\\u%04x", (int) ch));
                    } else {
                        b.append(ch);
                    }
                }
            }
        }
        return b.append('"').toString();
    }
}
