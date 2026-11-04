// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

import com.legend.executionplan.MirJson;
import com.legend.json.Json;
import com.legend.sql.DateFmt;
import com.legend.sql.DatePart;
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
import com.legend.sql.dialect.RenderedStatement;
import com.legend.sql.dialect.SQLite;
import java.io.IOException;
import java.math.BigDecimal;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.List;
import java.util.Map;
import java.util.Set;

/**
 * The SQLite arms the golden dumps hold none of, rendered by Java's own {@code SQLite} (docs/rust-port/PHASE1.md §4 step
 * 5d; rust/oracles/mir/MANIFEST.tsv): the SqlFn arms no PCT or corpus test reaches on SQLite, the regular expressions'
 * find path, the maps, the casts' matrix, the root's delivered values, the quantified comparisons, the explosion
 * markers' shapes, the windowed moments, the date-time formats, and CommonSubexpressions firing. Each input is built
 * with Java's own constructors, written as MirJson, READ BACK from that text, and rendered on a FRESH {@code new
 * SQLite()} -- the tree the JVM's shim will send (unshared, its facts as MirJson carries them) -- in the line format of
 * the dumps ({@code {"dialect","config","fresh","op","kind","mir","answer"}}, a refusal by its class and message). core's
 * dialect target is private to core, so this runs on its built jars (the build's JDK, remotejdk_25):
 * <pre>
 *   bazel build //core:sql_dialect //core:execution_plan //core:model //json //base
 *   $(bazel info output_base)/external/rules_java++toolchains+remotejdk25_&lt;os&gt;_&lt;arch&gt;/bin/java \
 *     -cp bazel-bin/core/libsql_dialect.jar:bazel-bin/core/libsql.jar:bazel-bin/core/libexecution_plan.jar:\
 *   bazel-bin/core/libmodel.jar:bazel-bin/json/libjson.jar:bazel-bin/base/libbase.jar \
 *     rust/oracles/mir/SqliteArms.java rust/oracles/mir/sqlite-arms.jsonl
 * </pre>
 * No input has a tie for CommonSubexpressions' largest repeated node (Java breaks one by identity-hash order).
 */
public final class SqliteArms {

    private SqliteArms() {
    }

    private static final List<String> LINES = new ArrayList<>();

    // ---- the table every expression reads

    static final SqlType BIGINT = SqlType.Scalar.BIGINT;
    static final SqlType VARCHAR = SqlType.Scalar.VARCHAR;
    static final SqlType DOUBLE = SqlType.Scalar.DOUBLE;
    static final SqlType BOOLEAN = SqlType.Scalar.BOOLEAN;
    static final SqlType DATE = SqlType.Scalar.DATE;
    static final SqlType TIMESTAMP = SqlType.Scalar.TIMESTAMP;
    static final SqlType JSON = SqlType.Scalar.JSON;
    static final SqlType HUGEINT = SqlType.Scalar.HUGEINT;
    static final SqlType DEC = new SqlType.Decimal(10, 2);
    static final SqlType DEC0 = new SqlType.Decimal(10, 0);
    static final SqlType STRUCT = new SqlType.Struct(List.of(new SqlType.Struct.Field("a", BIGINT),
            new SqlType.Struct.Field("b", VARCHAR)));
    static final SqlType XS = new SqlType.Array(BIGINT);
    static final SqlType SS = new SqlType.Array(VARCHAR);
    static final SqlType DS = new SqlType.Array(DOUBLE);
    static final SqlType JS = new SqlType.Array(JSON);
    static final SqlType XSS = new SqlType.Array(XS);
    static final SqlType STS = new SqlType.Array(STRUCT);
    static final SqlType MAP = new SqlType.Map(VARCHAR, BIGINT);

    static final List<OutputCol> COLUMNS = List.of(oc("s", VARCHAR), oc("n", BIGINT), oc("d", DOUBLE),
            oc("b", BOOLEAN), oc("dt", DATE), oc("ts", TIMESTAMP), oc("j", JSON), oc("h", HUGEINT), oc("dec", DEC),
            oc("dec0", DEC0), oc("st", STRUCT), oc("xs", XS), oc("ss", SS), oc("js", JS), oc("xss", XSS),
            oc("sts", STS), oc("m", MAP));

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

    static SqlExpr part(DatePart p) {
        return new SqlExpr.DatePartLit(p);
    }

    static SqlExpr lambda(SqlExpr body, String... params) {
        return new SqlExpr.Lambda(List.of(params), body);
    }

    /** A projection labelled {@code alias}: its output declared as its expression's type when it has one (a select's
     *  outputs are its projections' declared outputs), none otherwise. */
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

    static SqlSelect marker(SqlSelect.Projection... unnests) {
        return select(new SqlSource.Dual(), unnests);
    }

    // ---- the cases

    /** One expression, projected as {@code v} over the table. */
    static void e(SqlExpr x) {
        q(select(t(), p(x, "v")));
    }

    static void q(SqlQuery q) {
        add("query.renderStatement", q);
    }

    static void text(SqlQuery q) {
        add("query.render", q);
    }

    public static void main(String[] args) throws IOException {
        scalarArms();
        regularExpressions();
        lists();
        maps();
        temporal();
        casts();
        structure();
        aggregatesAndWindows();
        statements();
        commonSubexpressions();
        Files.writeString(Path.of(args[0]), String.join("", LINES), StandardCharsets.UTF_8);
        System.out.println(LINES.size() + " inputs (java " + Runtime.version() + ")");
    }

    static void scalarArms() {
        e(call(SqlFn.NEGATE, c("n")));
        e(call(SqlFn.CONCAT_JOIN, c("s"), c("n"), c("d"), c("b"), new SqlExpr.DecimalLit(new BigDecimal("2.50"))));
        e(call(SqlFn.STRUCT_INSERT, c("st"), str("c"), c("n")));
        e(call(SqlFn.STRUCT_INSERT, c("st"), c("s"), c("n")));
        e(call(SqlFn.JSON_MERGE_PATCH, c("j"), c("j")));
        e(call(SqlFn.BIT_NOT, c("n")));
        e(call(SqlFn.RANDOM));
        e(call(SqlFn.FLOOR_RAW, c("d")));
        e(call(SqlFn.BOOL_TO_TEXT, c("b")));
        e(call(SqlFn.FLOAT_CANON, c("d")));
        e(call(SqlFn.JSON_TYPE, c("j")));
        e(call(SqlFn.JSON_ARRAY_LENGTH, c("j")));
        e(call(SqlFn.JSON_PRETTY, c("j")));
        e(call(SqlFn.SPLIT_PART, c("s"), str(","), i(2)));
        e(call(SqlFn.INT_DIVIDE, c("d"), i(2)));
        e(call(SqlFn.ROUND_HALF_UP, c("d"), i(2)));
        e(call(SqlFn.ROUND_HALF_UP, c("n")));
        e(call(SqlFn.FORMAT, str("%s is %d, %05.2f%% and %s"), c("s"), c("n"), c("d"), c("b"), c("ts")));
        e(call(SqlFn.FORMAT, c("s"), c("n")));
        e(call(SqlFn.TO_VARIANT, c("n"), c("s")));
        e(call(SqlFn.VARIANT_GET, c("j"), c("n")));
        e(call(SqlFn.VARIANT_GET, c("j"), str("$")));
        e(call(SqlFn.TIMEZONE, str("America/New_York"), c("ts")));
        e(new SqlExpr.StringLit("a\u0000b'c\u0000"));
        // (a non-finite FloatLit cannot be made: its constructor types it by BigDecimal.valueOf)
        e(new SqlExpr.FloatLit(1.0E20));
        e(new SqlExpr.FloatLit(-0.0));
        e(new SqlExpr.JsonArray(List.of(c("n"), c("s"), c("b"), c("xs"), new SqlExpr.BoolLit(true))));
        // a field read of a struct of two fields of one name: the struct proves the first one's type (structGetType)
        SqlType twice = new SqlType.Struct(List.of(new SqlType.Struct.Field("a", BIGINT),
                new SqlType.Struct.Field("a", JSON)));
        e(new SqlExpr.StructGet(SqlExpr.Column.of("t0", "st", twice, true, OutputCol.Origin.DERIVED), "a",
                new TypeFact.Unknown()));
        // a large operand repeated by its spelling: bound once, the bound names numbered in writing order
        SqlExpr big = call(SqlFn.CONCAT, c("s"), str("a"), c("s"), str("b"), c("s"), str("c"), c("s"), str("d"),
                c("s"), str("e"), c("s"));
        e(call(SqlFn.LPAD, call(SqlFn.LPAD, big, i(10), str("x")), i(20), str("y")));
        e(call(SqlFn.ASCII_CODE, call(SqlFn.LEFT, big, i(3))));
        e(new SqlExpr.Membership(c("n"), new SqlExpr.ArrayLit(List.of(i(1), i(2), i(3), i(4), i(5), i(6), i(7),
                i(8), i(9), i(10), i(11), i(12)))));
    }

    static void regularExpressions() {
        e(call(SqlFn.REGEXP_EXTRACT, c("s"), str("a(b+)c"), i(1)));
        e(call(SqlFn.REGEXP_EXTRACT, c("s"), str("[0-9]+")));
        e(call(SqlFn.REGEXP_EXTRACT, c("s"), str("(?i)x|y*?z")));
        e(call(SqlFn.REGEXP_EXTRACT, c("s"), str("a(b)"), c("n")));
        e(call(SqlFn.REGEXP_EXTRACT, c("s"), c("s")));
        e(call(SqlFn.REGEXP_EXTRACT, c("s"), str("a(b)"), i(2)));
        e(call(SqlFn.REGEXP_EXTRACT_ALL, c("s"), str("\\d+")));
        e(call(SqlFn.REGEXP_EXTRACT_ALL, c("s"), str("x(y)"), i(1)));
        e(call(SqlFn.REGEXP_REPLACE, c("s"), str("a(b)"), str("X\\1Y\\0\\\\")));
        e(call(SqlFn.REGEXP_REPLACE, c("s"), str("a(b)(c)"), str("\\2-\\1"), str("g")));
        e(call(SqlFn.REGEXP_REPLACE, c("s"), str("ab"), str(""), str("gi")));
        e(call(SqlFn.REGEXP_REPLACE, c("s"), str("ab"), str("z"), str("ic")));
        e(call(SqlFn.REGEXP_REPLACE, c("s"), str("ab"), str("z"), str("q")));
        // any Unicode digit after a backslash is a group number (Character.isDigit): ٣ is group 1587
        e(call(SqlFn.REGEXP_REPLACE, c("s"), str("a(b)"), str("x\\٣")));
        e(call(SqlFn.MATCHES, c("s"), str("^a[^b-d\\s]*\\.$")));
        e(call(SqlFn.MATCHES, c("s"), str("(?ms)^x.y$")));
        e(call(SqlFn.MATCHES, c("s"), str("a"), str("i")));
        e(call(SqlFn.REGEXP_FULL_MATCH, c("s"), str("(?:ab|cd){2,}(e)?")));
        e(call(SqlFn.REGEXP_FULL_MATCH, c("s"), str("a\\b")));
    }

    static void lists() {
        e(call(SqlFn.LIST_DISTINCT, c("xs")));
        e(call(SqlFn.LIST_INIT, c("xs")));
        e(call(SqlFn.LIST_PRODUCT, c("xs")));
        e(call(SqlFn.LIST_REVERSE, c("ss")));
        e(call(SqlFn.LIST_REVERSE, c("js")));
        e(call(SqlFn.LIST_REDUCE, c("xs"), lambda(call(SqlFn.PLUS, SqlExpr.Column.derived(null, "acc"),
                SqlExpr.Column.derived(null, "x")), "acc", "x")));
        e(call(SqlFn.LIST_REDUCE, c("xs"), lambda(SqlExpr.Column.derived(null, "x"), "x")));
        e(call(SqlFn.REPEAT_VALUE, c("s"), i(3)));
        e(call(SqlFn.LIST_ZIP, c("xs"), c("ss")));
        e(call(SqlFn.TYPEOF, c("n")));
        e(call(SqlFn.LIST_SORT, c("sts")));
        e(call(SqlFn.LIST_SORT_DESC, c("js")));
        e(call(SqlFn.LIST_FILTER, c("sts"), lambda(call(SqlFn.GREATER, new SqlExpr.Column("x", "a"), i(1)), "x",
                "i")));
        e(call(SqlFn.LIST_TRANSFORM, c("xs"), c("n")));
        e(call(SqlFn.LIST_CONCAT, c("xs"), c("xs")));
        e(new SqlExpr.ReduceCollection(SqlAgg.Fn.STRING_AGG, c("ss"), List.of(str(","))));
        e(new SqlExpr.ReduceCollection(SqlAgg.Fn.MODE, c("ss"), List.of(str(","))));
        e(new SqlExpr.OrderedListAgg(c("s"), c("n")));
    }

    static void maps() {
        e(call(SqlFn.MAP_FROM_LISTS, c("ss"), c("xs")));
        e(call(SqlFn.MAP_FROM_ENTRIES, c("sts")));
        e(call(SqlFn.MAP_FROM_ENTRIES, c("xs")));
        e(call(SqlFn.MAP_EMPTY));
        e(call(SqlFn.MAP_EXTRACT, c("m"), str("k")));
        e(call(SqlFn.MAP_KEYS, c("m")));
        e(call(SqlFn.MAP_VALUES, c("m")));
        e(call(SqlFn.MAP_CONCAT, c("m"), c("m"), c("m")));
    }

    static void temporal() {
        for (DatePart p : DatePart.values()) {
            e(call(SqlFn.DATE_TRUNC, part(p), c("ts")));
        }
        e(call(SqlFn.EPOCH_NS, c("ts")));
        e(call(SqlFn.FROM_EPOCH_NS, c("n")));
        e(call(SqlFn.ADD_INTERVAL_TEMPORAL, part(DatePart.DAY), c("n"), c("ts")));
        e(call(SqlFn.ADD_INTERVAL, part(DatePart.YEAR), c("n"), c("dt")));
        e(call(SqlFn.ADD_INTERVAL, part(DatePart.MONTH), c("n"), c("ts")));
        e(call(SqlFn.ADD_INTERVAL, part(DatePart.WEEK), c("n"), c("dt")));
        e(call(SqlFn.ADD_INTERVAL, part(DatePart.MILLISECOND), c("n"), c("ts")));
        e(call(SqlFn.ADD_INTERVAL, part(DatePart.QUARTER), c("n"), c("ts")));
        e(call(SqlFn.EPOCH_SECONDS, c("ts")));
        e(call(SqlFn.EPOCH_MS, c("ts")));
        e(call(SqlFn.FROM_EPOCH_SECONDS, c("n")));
        e(call(SqlFn.FROM_EPOCH_MS, c("n")));
        e(call(SqlFn.DAYNAME, c("dt")));
        e(call(SqlFn.MONTHNAME, c("dt")));
        e(call(SqlFn.STRPTIME, c("s"), new SqlExpr.FormatLit(List.of(DateFmt.Part.YEAR4))));
        for (DatePart p : DatePart.values()) {
            e(call(SqlFn.EXTRACT, part(p), c("ts")));
        }
        for (DatePart p : DatePart.values()) {
            e(call(SqlFn.DATE_DIFF, part(p), c("ts"), c("ts")));
        }
        e(call(SqlFn.TIME_BUCKET, part(DatePart.WEEK), i(2), c("ts")));
        e(call(SqlFn.TIME_BUCKET, part(DatePart.YEAR), i(2), c("ts")));
        e(call(SqlFn.TIME_BUCKET, part(DatePart.QUARTER), i(2), c("ts")));
        e(call(SqlFn.STRFTIME, c("ts"), new SqlExpr.FormatLit(List.of(DateFmt.Part.YEAR4, new DateFmt.Text("-%"),
                DateFmt.Part.MONTH_NAME, new DateFmt.Text(" "), DateFmt.Part.HOUR12, DateFmt.Part.MIN2,
                DateFmt.Part.AMPM, new DateFmt.Text("."), DateFmt.Part.SUBSEC_MICRO, DateFmt.Part.WEEKDAY_NAME,
                DateFmt.Part.MONTH_ABBREV, DateFmt.Part.HOUR12_NOPAD, DateFmt.Part.SUBSEC_NANO,
                DateFmt.Part.SUBSEC_MIN, DateFmt.Part.SEC2))));
        e(call(SqlFn.STRFTIME, c("ts"), new SqlExpr.FormatLit(List.of())));
        e(call(SqlFn.STRFTIME, c("ts"), c("s")));
        e(call(SqlFn.MAKE_TIMESTAMP, i(2024), i(1), i(2), i(3), i(4)));
        e(call(SqlFn.MAKE_DATE, i(2024), i(1), i(2)));
    }

    static void casts() {
        SqlType[] targets = {BOOLEAN, BIGINT, DOUBLE, DATE, TIMESTAMP, SqlType.Scalar.TIMESTAMPTZ,
            SqlType.Scalar.TIME, JSON, VARCHAR, DEC, SqlType.Scalar.TEMPORAL_TEXT, DS, MAP};
        String[] froms = {"s", "n", "d", "b", "dt", "ts", "j", "h", "dec", "dec0", "xs", "st"};
        for (String from : froms) {
            for (SqlType target : targets) {
                e(new SqlExpr.Cast(c(from), target));
            }
        }
        e(new SqlExpr.Cast(call(SqlFn.VARIANT_GET, c("j"), str("k")), VARCHAR));
        e(new SqlExpr.Cast(c("j"), XS));
        e(new SqlExpr.Cast(c("j"), JS));
        e(new SqlExpr.Cast(new SqlExpr.NullLit(), BIGINT));
        e(new SqlExpr.Cast(new SqlExpr.FloatLit(2.5), VARCHAR));
        e(new SqlExpr.Cast(new SqlExpr.BoolLit(false), VARCHAR));
    }

    static void structure() {
        // the root's values as their labels deliver them, a sort key naming a moved column its expression
        q(select(t(), List.of(p(c("dec"), "v"), p(c("d"), "w"), p(c("h"), "x"), p(new SqlExpr.NullLit(), "y", DEC)),
                null, List.of(), List.of(new SqlSelect.SortKey(SqlExpr.Column.derived(null, "v"), true, null, null),
                        new SqlSelect.SortKey(SqlExpr.Column.derived(null, "s"), false, null, null)), null, 5L));
        // a union's branches delivered; an ordered branch wrapped
        SqlSelect a = select(t(), List.of(p(c("dec"), "v")), null, List.of(),
                List.of(new SqlSelect.SortKey(c("n"), true, null, null)), 3L, null);
        SqlSelect b = select(t(), p(c("dec"), "v"));
        q(new SqlUnion(List.of(a, b), true, a.outputs()));
        // a root select met again inside another branch: one object after CommonSubexpressions, delivered there too
        SqlSelect nested = select(new SqlSource.Subselect(b, "s", null), p(SqlExpr.Column.of("s", b.outputs(), "v"),
                "v"));
        q(new SqlUnion(List.of(b, nested), true, b.outputs()));
        // a root select with a marker in scope is respelled before its values are delivered: they are not
        q(select(new SqlSource.Join(t(), new SqlSource.Subselect(marker(p(call(SqlFn.UNNEST, c("xs")), "e")), "u",
                null), SqlSource.Join.Kind.LEFT_LATERAL, new SqlExpr.BoolLit(true)), p(c("dec"), "v"), p(c("d"), "w")));
        // a WITH's body delivered, its CTE not
        q(new SqlWith(List.of(new SqlWith.Cte("w", select(t(), p(c("dec"), "v")), true)),
                select(new SqlSource.Cte("w", "w0", List.of(oc("v", DEC))), p(SqlExpr.Column.of("w0",
                        List.of(oc("v", DEC)), "v"), "v"))));
        // quantified comparisons
        SqlSelect sub = select(t(), p(c("n"), "n"));
        q(select(t(), List.of(p(c("s"), "s")), new SqlExpr.Quantified(c("n"), SqlFn.GREATER,
                SqlExpr.Quantifier.ANY, sub), List.of(), List.of(), null, null));
        q(select(t(), List.of(p(c("s"), "s")), new SqlExpr.Quantified(c("n"), SqlFn.EQUAL,
                SqlExpr.Quantifier.ALL, sub), List.of(), List.of(), null, null));
        // explosion markers: several lists in parallel (the longest's positions), a NULL list, unqualified reads
        q(select(t(), p(call(SqlFn.UNNEST, c("xs")), "a"), p(call(SqlFn.UNNEST, c("ss")), "b"),
                p(call(SqlFn.UNNEST, c("js")), "c"), p(c("n"), "n")));
        SqlSelect m = marker(p(call(SqlFn.UNNEST, new SqlExpr.NullLit()), "a"),
                p(call(SqlFn.UNNEST, new SqlExpr.ArrayLit(List.of(i(1), i(2)))), "b"));
        q(select(new SqlSource.Subselect(m, "u", null), p(SqlExpr.Column.derived(null, "a"), "a"),
                p(SqlExpr.Column.derived(null, "b"), "b")));
        q(select(new SqlSource.Subselect(m, "u", null)));
        q(m);
        // a lateral marker joined ON TRUE, and one with a join condition
        SqlSelect lm = marker(p(call(SqlFn.UNNEST, c("xs")), "e"));
        q(select(new SqlSource.Join(t(), new SqlSource.Subselect(lm, "u", null), SqlSource.Join.Kind.LEFT_LATERAL,
                new SqlExpr.BoolLit(true)), p(c("n"), "n"), p(SqlExpr.Column.derived("u", "e"), "e")));
        q(select(new SqlSource.Join(t(), new SqlSource.Subselect(lm, "u", null), SqlSource.Join.Kind.LEFT_LATERAL,
                call(SqlFn.GREATER, c("n"), i(1))), p(c("n"), "n")));
        // a lateral explosion of a carried list, inlined (LateralUnnestAsMarker)
        SqlSelect carry = select(new SqlSource.Dual(), p(c("xs"), "lst"));
        SqlSelect explode = select(new SqlSource.Subselect(carry, "c", null),
                p(call(SqlFn.UNNEST, SqlExpr.Column.derived("c", "lst")), "elem"));
        q(select(new SqlSource.Join(t(), new SqlSource.Subselect(explode, "u", null),
                SqlSource.Join.Kind.CROSS_LATERAL, null), p(c("n"), "n"), p(SqlExpr.Column.derived("u", "elem"),
                "elem")));
        // a star over a marker, unwrapped (StarOverExplosionUnwrapped)
        q(select(new SqlSource.Join(t(), new SqlSource.Subselect(SqlSelect.starOf(new SqlSource.Subselect(lm, "i0",
                null)), "u", null), SqlSource.Join.Kind.CROSS_LATERAL, null)));
        // nested select-list explosions, numbered by the pass; one over an aggregate left as it is
        SqlSelect inner = select(t(), p(call(SqlFn.UNNEST, c("xs")), "x"), p(c("n"), "n"));
        q(select(new SqlSource.Subselect(inner, "s0", null), p(call(SqlFn.UNNEST, c("ss")), "y"),
                p(SqlExpr.Column.derived("s0", "x"), "x")));
        q(select(t(), List.of(p(call(SqlFn.UNNEST, new SqlAgg.Reducer(SqlAgg.Fn.LIST, List.of(c("n")), false,
                List.of())), "x")), null, List.of(c("s")), List.of(), null, null));
        // a lateral subquery read as JSON rows: a star over it, its fields, LEFT and CROSS
        SqlSelect rows = select(t(), List.of(p(c("n"), "k"), p(c("j"), "jv")), call(SqlFn.EQUAL, c("s"),
                SqlExpr.Column.derived("l0", "s")), List.of(), List.of(), null, null);
        SqlSource left = new SqlSource.Table("t", "l0", COLUMNS, false, Map.of(), NameQuoting.BARE);
        q(select(new SqlSource.Join(left, new SqlSource.Subselect(rows, "r", null), SqlSource.Join.Kind.CROSS_LATERAL,
                null)));
        q(select(new SqlSource.Join(left, new SqlSource.Subselect(rows, "r", null), SqlSource.Join.Kind.LEFT_LATERAL,
                new SqlExpr.BoolLit(true)), p(SqlExpr.Column.derived("r", "k"), "k"),
                p(SqlExpr.Column.derived("r", "jv"), "jv")));
        q(select(new SqlSource.Join(left, new SqlSource.Subselect(rows, "r", null), SqlSource.Join.Kind.LEFT_LATERAL,
                call(SqlFn.GREATER, SqlExpr.Column.derived("r", "k"), i(0))), p(SqlExpr.Column.derived("r", "k"),
                "k")));
        // VALUES, with rows and with none
        List<OutputCol> vo = List.of(oc("a", BIGINT), oc("b", VARCHAR));
        q(select(new SqlSource.Values(List.of(List.of(i(1), str("x")), List.of(i(2), new SqlExpr.NullLit())),
                List.of("a", "b"), "v0", vo)));
        q(select(new SqlSource.Values(List.of(), List.of("a", "b"), "v0", vo)));
        // a NULL projection, labelled by its output; an OFFSET with no LIMIT; a typed plan parameter
        q(select(t(), new SqlSelect.Projection(new SqlExpr.NullLit(), null, oc("z", BIGINT))));
        q(select(t(), List.of(p(c("n"), "n")), null, List.of(), List.of(), null, 7L));
        q(select(t(), List.of(p(c("n"), "n")), call(SqlFn.EQUAL, c("ts"), new SqlExpr.PlanParam("p",
                SqlExpr.PlanParam.Kind.DATETIME, false, null, new TypeFact.Unknown(), new ValueTyping(
                        List.of(ValueKind.DATE_TIME, ValueKind.DATE), ValueKind.DATE_TIME))), List.of(), List.of(),
                null, null));
        q(select(t(), List.of(p(c("n"), "n")), new SqlExpr.Membership(c("n"), new SqlExpr.PlanParam("ns",
                SqlExpr.PlanParam.Kind.OTHER, false, null, new TypeFact.Typed(XS, true), null)), List.of(), List.of(),
                null, null));
        text(select(t(), p(new SqlExpr.TimestampLit("2024-01-02T03:04:05.123"), "ts"),
                p(new SqlExpr.DateLit("2024-01-02"), "dt"), p(new SqlExpr.TimeLit("03:04:05"), "tm")));
    }

    static void aggregatesAndWindows() {
        SqlExpr[] grouped = {
            new SqlAgg.Reducer(SqlAgg.Fn.LIST, List.of(c("n")), true, List.of(new SqlSelect.SortKey(c("n"), false,
                    SqlSelect.SortKey.NullOrder.NULLS_FIRST, null))),
            new SqlAgg.Reducer(SqlAgg.Fn.LIST, List.of(c("n"), c("s")), false, List.of()),
            new SqlAgg.Reducer(SqlAgg.Fn.ANY_VALUE, List.of(c("n")), false, List.of()),
            new SqlAgg.Reducer(SqlAgg.Fn.BOOL_AND, List.of(c("b")), false, List.of()),
            new SqlAgg.Reducer(SqlAgg.Fn.BOOL_OR, List.of(c("b")), false, List.of()),
            new SqlAgg.Reducer(SqlAgg.Fn.MODE, List.of(c("s")), false, List.of()),
            new SqlAgg.Reducer(SqlAgg.Fn.MEDIAN, List.of(c("d")), false, List.of()),
            new SqlAgg.Reducer(SqlAgg.Fn.MEDIAN, List.of(c("d")), true, List.of()),
            new SqlAgg.Reducer(SqlAgg.Fn.QUANTILE_CONT, List.of(c("s"), new SqlExpr.FloatLit(0.25)), false,
                    List.of(new SqlSelect.SortKey(c("s"), false, null, null))),
            new SqlAgg.Reducer(SqlAgg.Fn.QUANTILE_DISC, List.of(c("d")), false, List.of()),
            new SqlAgg.Reducer(SqlAgg.Fn.STDDEV, List.of(c("d")), false, List.of()),
            new SqlAgg.Reducer(SqlAgg.Fn.VARIANCE, List.of(c("d")), false, List.of()),
            new SqlAgg.Reducer(SqlAgg.Fn.ARG_MAX, List.of(c("s"), c("n")), false, List.of(new SqlSelect.SortKey(c("n"),
                    true, null, null))),
            new SqlAgg.Reducer(SqlAgg.Fn.WAVG, List.of(c("d"), c("n")), false, List.of()),
            new SqlAgg.Reducer(SqlAgg.Fn.ROW_NUMBER, List.of(), false, List.of()),
        };
        for (SqlExpr g : grouped) {
            q(select(t(), List.of(p(c("s"), "s"), p(g, "v")), null, List.of(c("s")), List.of(), null, null));
        }
        // the windowed moments, all seven spellings, over a partition and an order
        SqlAgg.Fn[] moments = {SqlAgg.Fn.VAR_POP, SqlAgg.Fn.STDDEV_POP, SqlAgg.Fn.VAR_SAMP, SqlAgg.Fn.VARIANCE,
            SqlAgg.Fn.STDDEV_SAMP, SqlAgg.Fn.STDDEV, SqlAgg.Fn.COVAR_POP, SqlAgg.Fn.COVAR_SAMP, SqlAgg.Fn.CORR};
        for (SqlAgg.Fn fn : moments) {
            List<SqlExpr> args = fn.name().startsWith("CO") ? List.of(c("d"), c("n")) : List.of(c("d"));
            e(new SqlExpr.WindowCall(new SqlAgg.Reducer(fn, args, false, List.of()), List.of(c("s")),
                    List.of(new SqlSelect.SortKey(c("n"), true, null, null)), null));
        }
        // an ordered aggregate over a whole partition: the window's order over every row; refused over an order
        SqlAgg.Reducer ordered = new SqlAgg.Reducer(SqlAgg.Fn.STRING_AGG, List.of(c("s"), str(",")), false,
                List.of(new SqlSelect.SortKey(c("n"), false, null, null)));
        e(new SqlExpr.WindowCall(ordered, List.of(c("dt")), List.of(), null));
        e(new SqlExpr.WindowCall(ordered, List.of(), List.of(new SqlSelect.SortKey(c("n"), true, null, null)),
                null));
        // an interval RANGE frame over a date key (its Julian day), and its refusals
        SqlExpr.WindowCall.Frame days = new SqlExpr.WindowCall.Frame(SqlExpr.WindowCall.Frame.Kind.RANGE,
                new SqlExpr.WindowCall.Frame.Bound.IntervalPreceding(3, "DAYS"),
                new SqlExpr.WindowCall.Frame.Bound.IntervalFollowing(36, "hours"));
        SqlAgg.Reducer sum = new SqlAgg.Reducer(SqlAgg.Fn.SUM, List.of(c("n")), false, List.of());
        e(new SqlExpr.WindowCall(sum, List.of(c("s")), List.of(new SqlSelect.SortKey(c("dt"), false, null, null)),
                days));
        e(new SqlExpr.WindowCall(sum, List.of(), List.of(new SqlSelect.SortKey(c("ts"), true,
                SqlSelect.SortKey.NullOrder.NULLS_FIRST, null)), days));
        e(new SqlExpr.WindowCall(sum, List.of(), List.of(new SqlSelect.SortKey(c("n"), true, null, null)), days));
        e(new SqlExpr.WindowCall(sum, List.of(), List.of(new SqlSelect.SortKey(c("dt"), true, null, null),
                new SqlSelect.SortKey(c("n"), true, null, null)), days));
        e(new SqlExpr.WindowCall(sum, List.of(), List.of(new SqlSelect.SortKey(c("dt"), true, null, null)),
                new SqlExpr.WindowCall.Frame(SqlExpr.WindowCall.Frame.Kind.RANGE,
                        new SqlExpr.WindowCall.Frame.Bound.IntervalPreceding(1, "MONTHS"),
                        new SqlExpr.WindowCall.Frame.Bound.CurrentRow())));
    }

    static void statements() {
        List<SqlDdl.Column> cols = List.of(new SqlDdl.Column("id", false, new SqlDdl.ColumnType.Plain(
                SqlDdl.ColumnType.Kind.BIGINT), true, true), new SqlDdl.Column("Name", true,
                new SqlDdl.ColumnType.Sized("VARBINARY", 10), false, false), new SqlDdl.Column("amount", false,
                new SqlDdl.ColumnType.Scaled("DECIMAL", 10, 2), true, false), new SqlDdl.Column("key", false,
                new SqlDdl.ColumnType.Plain(SqlDdl.ColumnType.Kind.VARCHAR), false, true));
        add("ddl.render", new SqlDdl.CreateTable("hr", "People", NameQuoting.BARE, cols, true));
        add("ddl.render", new SqlDdl.DropSchema("hr", false));
        add("ddl.render", new SqlDdl.DropTable("hr", "People", NameQuoting.BARE));
        add("dml.render", new SqlDml.InsertFromTable("hr", "People", NameQuoting.BARE, List.of("id", "Name"),
                Set.of("Name"), "stage"));
        add("dml.renderStatement", new SqlDml.DeleteAll(null, "select", NameQuoting.BARE));
        add("dml.render", new SqlDml.InsertValues(null, "t", new NameQuoting(false, true), List.of("a"), Set.of(),
                List.of(List.of(new SqlExpr.PlanParam("p", SqlExpr.PlanParam.Kind.OTHER, false, null,
                        new TypeFact.Typed(BIGINT, true), null)))));
    }

    static void commonSubexpressions() {
        // a movable chain of 41 nodes, repeated: bound once
        SqlExpr x = c("n");
        for (int k = 1; k <= 20; k++) {
            x = call(SqlFn.PLUS, x, i(k));
        }
        e(call(SqlFn.TIMES, x, x));
        // a node holding it twice, itself repeated: the outer bound first, the inner within it
        SqlExpr y = call(SqlFn.MINUS, x, x);
        e(call(SqlFn.TIMES, y, y));
        // a lambda's body bound within itself
        SqlExpr z = SqlExpr.Column.derived(null, "e");
        for (int k = 1; k <= 20; k++) {
            z = call(SqlFn.PLUS, z, i(k));
        }
        e(call(SqlFn.LIST_TRANSFORM, c("xs"), lambda(call(SqlFn.TIMES, z, z), "e")));
        // a raise is never moved out of its branch; an aggregate's projection is left as it is
        SqlExpr r = call(SqlFn.PARSE_INT, call(SqlFn.CONCAT, c("s"), str("1"), c("s"), str("2"), c("s"), str("3"),
                c("s"), str("4"), c("s"), str("5"), c("s"), str("6"), c("s"), str("7"), c("s"), str("8"), c("s"),
                str("9"), c("s"), str("0"), c("s"), str("1"), c("s"), str("2")));
        e(call(SqlFn.TIMES, r, r));
        q(select(t(), List.of(p(c("s"), "s"), p(call(SqlFn.TIMES, x, new SqlAgg.Reducer(SqlAgg.Fn.SUM,
                List.of(x), false, List.of())), "v")), null, List.of(c("s")), List.of(), null, null));
        // a repeated subexpression in a filter; the same select twice in a union, rewritten once
        SqlSelect f = select(t(), List.of(p(c("n"), "n")), call(SqlFn.GREATER, x, call(SqlFn.TIMES, x, i(2))),
                List.of(), List.of(), null, null);
        q(f);
        q(new SqlUnion(List.of(select(t(), p(call(SqlFn.TIMES, x, x), "v")), select(t(), p(call(SqlFn.TIMES, x,
                x), "v"))), false, List.of(new OutputCol("v", BIGINT, true))));
    }

    // ---- the line

    static void add(String op, Object input) {
        String mir;
        Object read;
        try {
            mir = mirOf(input);
            read = reread(input, mir);
            String again = mirOf(read);
            if (!again.equals(mir)) {
                throw new IllegalStateException("MirJson does not round-trip: " + mir + " / " + again);
            }
        } catch (RuntimeException e) {
            throw new IllegalStateException("cannot write " + input, e);
        }
        String out;
        try {
            out = "{\"ok\":" + answerOf(new SQLite(), op, read) + "}";
        } catch (RuntimeException e) {
            out = "{\"throws\":" + quote(e.getClass().getName()) + ",\"message\":" + quote(String.valueOf(
                    e.getMessage())) + "}";
        }
        String kind = read.getClass().getSimpleName();
        LINES.add("{\"dialect\":\"SQLite\",\"config\":null,\"fresh\":true,\"op\":" + quote(op) + ",\"kind\":"
                + quote(kind) + ",\"mir\":" + mir + ",\"answer\":" + out + "}\n");
    }

    static String mirOf(Object input) {
        return switch (input) {
            case SqlQuery q -> Json.toCompact(MirJson.query(q));
            case SqlDdl d -> Json.toCompact(MirJson.ddl(d));
            case SqlDml d -> Json.toCompact(MirJson.dml(d));
            default -> throw new IllegalStateException("no MirJson for " + input.getClass());
        };
    }

    static Object reread(Object input, String mir) {
        Json.Obj o = Json.parseObject(mir);
        return switch (input) {
            case SqlQuery q -> MirJson.readQuery(o);
            case SqlDdl d -> MirJson.readDdl(o);
            case SqlDml d -> MirJson.readDml(o);
            default -> throw new IllegalStateException("no MirJson for " + input.getClass());
        };
    }

    static String answerOf(SQLite d, String op, Object input) {
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
