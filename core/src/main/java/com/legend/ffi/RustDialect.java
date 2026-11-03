// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

package com.legend.ffi;

import com.legend.base.Nullable;
import com.legend.executionplan.MirJson;
import com.legend.json.Json;
import com.legend.sql.SqlDdl;
import com.legend.sql.SqlDml;
import com.legend.sql.SqlExpr;
import com.legend.sql.SqlQuery;
import com.legend.sql.SqlType;
import com.legend.sql.ValueKind;
import com.legend.sql.dialect.CatalogModel;
import com.legend.sql.dialect.CatalogType;
import com.legend.sql.dialect.DialectCapability;
import com.legend.sql.dialect.HostOperation;
import com.legend.sql.dialect.HostOperations;
import com.legend.sql.dialect.RenderedStatement;
import com.legend.sql.dialect.RustDialects;
import com.legend.sql.dialect.SqlDialect;

import java.util.ArrayList;
import java.util.EnumMap;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.OptionalInt;
import java.util.concurrent.ConcurrentHashMap;

/**
 * A dialect the Rust library answers (docs/rust-port/PHASE1.md; rust/lite-dialect/SEAM.md): each {@link SqlDialect}
 * method is one {@link Lite#dialect} call, its arguments and answer JSON -- the SQL tree as {@link MirJson} writes it,
 * a statement, a catalog column, a host operation in the seam's forms -- and a refusal is rethrown as the Java exception
 * the dialect threw (SEAM.md §5). The Rust dialect is made fresh for each call, so this object holds no dialect state;
 * the answers that take no argument (the facts) and the wire names are read once per dialect object and kept.
 */
public final class RustDialect implements SqlDialect, RustDialects.EngineText {

    private final String name;
    private final @Nullable String server;
    /** An engine-text dialect's configuration (SEAM.md §2), sent with it alone: the seam refuses it on any other. */
    private final boolean engineText;
    private final boolean quoteIdentifiers;
    private final @Nullable String timeZone;

    /** The facts, read once by this dialect object. */
    private volatile Json.@Nullable Obj factsRead;
    /** The wire names asked of this dialect object, by driver type name, each asked once. */
    private final Map<String, String> wireNames = new ConcurrentHashMap<>();
    private final Map<String, Json.Node> typingWireNames = new ConcurrentHashMap<>();

    /** The dialect Rust names {@code name} (Java's simple class name: {@code DuckDb}, {@code H2}). */
    public RustDialect(String name) {
        this(name, null, false, false, null);
    }

    /** The engine-text dialect Rust names {@code name}, made as Java's {@code new EngineStyleH2(quoteIdentifiers,
     *  timeZone)} is. */
    public RustDialect(String name, boolean quoteIdentifiers, @Nullable String timeZone) {
        this(name, null, true, quoteIdentifiers, timeZone);
    }

    private RustDialect(String name, @Nullable String server, boolean engineText, boolean quoteIdentifiers,
            @Nullable String timeZone) {
        this.name = name;
        this.server = server;
        this.engineText = engineText;
        this.quoteIdentifiers = quoteIdentifiers;
        this.timeZone = timeZone;
    }

    // ---- the call

    private String spec() {
        Map<String, Object> s = new LinkedHashMap<>();
        s.put("dialect", name);
        s.put("server", server);
        // read at each call, as Java's DuckDb read it at each render
        s.put("engineScanOrder", Boolean.getBoolean("legend.exec.engineScanOrder"));
        if (engineText) {
            s.put("quoteIdentifiers", quoteIdentifiers);
            s.put("timeZone", timeZone);
        }
        return Json.toCompact(s);
    }

    /** The op's answer, parsed; a refusal rethrown as the dialect threw it. */
    private Json.Node call(String op, Map<String, Object> args) {
        String answer;
        try {
            answer = Lite.shared().dialect(spec(), op, Json.toCompact(args));
        } catch (LiteRefusal refusal) {
            throw rethrown(refusal.kind(), String.valueOf(refusal.getMessage()));
        }
        return Json.parse(answer, new Json.Config(64));
    }

    private Json.Node call(String op) {
        return call(op, Map.of());
    }

    /** SEAM.md §5: the kind a refusal crosses as, back to the Java class the dialect threw. */
    static RuntimeException rethrown(String kind, String message) {
        return switch (kind) {
            case "DIALECT_CAPABILITY" -> new DialectCapability(message);
            case "ILLEGAL_STATE" -> new IllegalStateException(message);
            case "ILLEGAL_ARGUMENT" -> new IllegalArgumentException(message);
            case "UNSUPPORTED_OPERATION" -> new UnsupportedOperationException(message);
            case "NOT_IMPLEMENTED" -> new com.legend.error.NotImplementedException(message);
            case "NULL_POINTER" -> new NullPointerException(message);
            case "CLASS_CAST" -> new ClassCastException(message);
            case "INDEX_OUT_OF_BOUNDS" -> new IndexOutOfBoundsException(message);
            default -> new IllegalStateException(kind + ": " + message);
        };
    }

    /** An op's args: field, value, field, value, ... -- a value may be null, which the seam reads as JSON null. */
    private static Map<String, Object> args(@Nullable Object... keyValues) {
        Map<String, Object> m = new LinkedHashMap<>();
        for (int i = 0; i < keyValues.length; i += 2) {
            m.put(String.valueOf(keyValues[i]), keyValues[i + 1]);
        }
        return m;
    }

    private Json.Obj facts() {
        Json.Obj f = factsRead;
        if (f == null) {
            f = (Json.Obj) call("facts");
            factsRead = f;
        }
        return f;
    }

    private static String string(Json.Node n) {
        return ((Json.Str) n).value();
    }

    private static @Nullable String stringOrNull(Json.Node n) {
        return n instanceof Json.Null ? null : string(n);
    }

    private static List<String> strings(Json.Node n) {
        List<String> out = new ArrayList<>();
        for (Json.Node item : ((Json.Arr) n).items()) {
            out.add(string(item));
        }
        return out;
    }

    private static boolean bool(Json.Node n) {
        return ((Json.Bool) n).value();
    }

    /** A fact that may be refused ({@code {"refused": {"kind", "message"}}}): its value, or the refusal thrown. */
    private Json.Node factOrRefusal(String fact) {
        Json.Node n = facts().getOr(fact, Json.Null.INSTANCE);
        if (n instanceof Json.Obj o && o.getOr("refused", null) instanceof Json.Obj r) {
            throw rethrown(r.getString("kind"), r.getString("message"));
        }
        return n == null ? Json.Null.INSTANCE : n;
    }

    // ---- rendering

    @Override
    public String render(SqlQuery query) {
        return string(call("query.render", args("query", MirJson.query(query))));
    }

    @Override
    public String renderInline(SqlQuery query) {
        return string(call("query.renderInline", args("query", MirJson.query(query))));
    }

    @Override
    public RenderedStatement renderStatement(SqlQuery query) {
        return statement((Json.Obj) call("query.renderStatement", args("query", MirJson.query(query))));
    }

    @Override
    public String render(SqlDdl ddl) {
        return string(call("ddl.render", args("ddl", MirJson.ddl(ddl))));
    }

    @Override
    public String render(SqlDml dml) {
        return string(call("dml.render", args("dml", MirJson.dml(dml))));
    }

    @Override
    public RenderedStatement renderStatement(SqlDml dml) {
        return statement((Json.Obj) call("dml.renderStatement", args("dml", MirJson.dml(dml))));
    }

    @Override
    public List<String> sessionTable(SqlDdl.CreateTable table, SqlDml.@Nullable InsertValues rows) {
        return strings(call("sessionTable", args("table", MirJson.ddl(table), "rows", rows == null ? null
                : MirJson.dml(rows))));
    }

    /** SEAM.md §4's STATEMENT, through the records' own constructors. */
    static RenderedStatement statement(Json.Obj o) {
        List<RenderedStatement.Bind> binds = new ArrayList<>();
        for (Json.Node n : o.getArr("binds").items()) {
            Json.Obj b = (Json.Obj) n;
            binds.add(new RenderedStatement.Bind(b.getString("parameter"), binding(b.getObj("binding"))));
        }
        List<RenderedStatement.Region> regions = new ArrayList<>();
        for (Json.Node n : o.getArr("regions").items()) {
            Json.Obj r = (Json.Obj) n;
            List<Integer> counters = new ArrayList<>();
            for (Json.Node c : r.getArr("counters").items()) {
                counters.add((int) ((Json.Num) c).longValue());
            }
            regions.add(new RenderedStatement.Region(r.getInt("start"), r.getInt("end"), r.getString("lead"),
                    r.getString("separator"), r.getStringOr("empty", null), r.getInt("firstBind"),
                    r.getInt("bindCount"), counters));
        }
        return new RenderedStatement(o.getString("sql"), binds, regions);
    }

    private static RenderedStatement.Binding binding(Json.Obj o) {
        return switch (o.getString("_type")) {
            case "one" -> new RenderedStatement.Binding.One(o.getString("nullType"),
                    o.getOr("hole", null) instanceof Json.Obj hole ? typeHole(hole) : null);
            case "array" -> new RenderedStatement.Binding.Array(o.getString("elementSqlType"));
            case "key" -> new RenderedStatement.Binding.Key(o.getInt("cell"));
            default -> throw new IllegalStateException("not a binding of the seam: " + o.getString("_type"));
        };
    }

    private static RenderedStatement.TypeHole typeHole(Json.Obj o) {
        Map<ValueKind, RenderedStatement.TypeSpelling> types = new EnumMap<>(ValueKind.class);
        for (Map.Entry<String, Json.Node> e : o.getObj("types").fields().entrySet()) {
            Json.Obj s = (Json.Obj) e.getValue();
            Json.Node fraction = s.getOr("fraction", null);
            types.put(ValueKind.valueOf(e.getKey()), new RenderedStatement.TypeSpelling(s.getString("name"),
                    RenderedStatement.Digits.valueOf(s.getString("digits")),
                    fraction == null ? null : (int) ((Json.Num) fraction).longValue()));
        }
        return new RenderedStatement.TypeHole(o.getInt("at"), types, ValueKind.valueOf(o.getString("absent")));
    }

    // ---- the facts

    @Override
    public List<String> sessionSetup() {
        return strings(facts().getArr("sessionSetup"));
    }

    @Override
    public boolean temporaryTables() {
        return facts().getBool("temporaryTables");
    }

    @Override
    public SqlDialect forServer(String version) {
        // the seam names the dialect the server takes (SEAM.md §3a): H2's for its own servers, H2Modern's for another
        String chosen = string(call("forServer", args("version", version)));
        return chosen.equals(name) ? this : new RustDialect(chosen);
    }

    @Override
    public @Nullable List<String> serverVersions() {
        // H2Modern refuses: a plan is written for H2's servers
        Json.Node v = factOrRefusal("serverVersions");
        return v instanceof Json.Null ? null : strings(v);
    }

    @Override
    public boolean loadsRowsInBulk() {
        return facts().getBool("loadsRowsInBulk");
    }

    @Override
    public boolean relational() {
        return facts().getBool("relational");
    }

    @Override
    public boolean inProcess() {
        return facts().getBool("inProcess");
    }

    @Override
    public boolean dropsStagedTablesWithTheirTransaction() {
        return facts().getBool("dropsStagedTablesWithTheirTransaction");
    }

    @Override
    public boolean needsStaticPivot() {
        return facts().getBool("needsStaticPivot");
    }

    @Override
    public KeysSnapshot keysSnapshot() {
        return KeysSnapshot.valueOf(string(factOrRefusal("keysSnapshot")));
    }

    @Override
    public @Nullable Integer resultColumnLimit() {
        Json.Node n = factOrRefusal("resultColumnLimit");
        return n instanceof Json.Num num ? (int) num.longValue() : null;
    }

    @Override
    public @Nullable String inMemoryPath() {
        return facts().getStringOr("inMemoryPath", null);
    }

    @Override
    public @Nullable String scriptAbort() {
        return facts().getStringOr("scriptAbort", null);
    }

    // ---- names and text

    @Override
    public String physicalName(String n) {
        return string(call("physicalName", args("name", n)));
    }

    @Override
    public String storedName(String n, boolean declaredQuoted) {
        return string(call("storedName", args("name", n, "declaredQuoted", declaredQuoted)));
    }

    @Override
    public String insertColumnName(String n, boolean declaredQuoted) {
        return string(call("insertColumnName", args("name", n, "declaredQuoted", declaredQuoted)));
    }

    @Override
    public String raw(String statement, @Nullable String writtenFor) {
        return string(call("raw", args("statement", statement, "writtenFor", writtenFor)));
    }

    @Override
    public String fromEngineText(String statement) {
        return string(call("fromEngineText", args("statement", statement)));
    }

    @Override
    public String wireTypeName(@Nullable String driverTypeName) {
        return wireNames.computeIfAbsent(String.valueOf(driverTypeName),
                k -> string(call("wireTypeName", args("driverTypeName", driverTypeName))));
    }

    @Override
    public @Nullable String typingWireName(@Nullable String driverTypeName) {
        return stringOrNull(typingWireNames.computeIfAbsent(String.valueOf(driverTypeName),
                k -> call("typingWireName", args("driverTypeName", driverTypeName))));
    }

    @Override
    public boolean deliversOwn(SqlType label, String wire) {
        return bool(call("deliversOwn", args("label", MirJson.type(label), "wire", wire)));
    }

    @Override
    public String script(List<String> statements) {
        return string(call("script", args("statements", statements)));
    }

    @Override
    public OptionalInt failingStatement(String message, List<String> statements) {
        Json.Node n = call("failingStatement", args("message", message, "statements", statements));
        return n instanceof Json.Num num ? OptionalInt.of((int) num.longValue()) : OptionalInt.empty();
    }

    // ---- the catalog

    @Override
    public CatalogType catalogType(CatalogModel.Column column) {
        Map<String, Object> c = args("name", column.name(), "dataType", column.dataType(), "logicalType",
                column.logicalType(), "precision", column.precision(), "scale", column.scale(), "notNull",
                column.notNull());
        Json.Obj t = (Json.Obj) call("catalogType", args("column", c));
        return new CatalogType(CatalogType.Read.valueOf(t.getString("read")), t.getStringOr("declared", null),
                t.getStringOr("conversion", null), t.getStringOr("reason", null));
    }

    @Override
    public String catalogColumnsQuery(String schema, String table) {
        return string(call("catalogColumnsQuery", args("schema", schema, "table", table)));
    }

    @Override
    public boolean bareNameReaches(String catalogName) {
        return bool(call("bareNameReaches", args("catalogName", catalogName)));
    }

    @Override
    public String copySelectList(List<CatalogModel.Conversion> conversions) {
        List<Object> cs = new ArrayList<>();
        for (CatalogModel.Conversion c : conversions) {
            cs.add(args("column", c.column(), "sql", c.sql()));
        }
        return string(call("copySelectList", args("conversions", cs)));
    }

    // ---- the host's operations

    @Override
    public HostOperations hostOperations() {
        return new HostOperations() {
            @Override
            public List<String> openDatabase(String database) {
                return strings(call("host.openDatabase", args("name", database)));
            }

            @Override
            public List<String> useDatabase(String database) {
                return strings(call("host.useDatabase", args("name", database)));
            }

            @Override
            public List<String> closeDatabase(String database) {
                return strings(call("host.closeDatabase", args("name", database)));
            }

            @Override
            public List<String> statements(HostOperation operation) {
                return strings(call("host.statements", args("operation", RustDialectJson.operation(operation))));
            }
        };
    }

    // ---- the engine text's own entry points (SEAM.md §3b; another dialect refuses them)

    @Override
    public String renderAsAliased(SqlQuery query) {
        return string(call("engine.renderAsAliased", args("query", MirJson.query(query))));
    }

    @Override
    public @Nullable String selectList(SqlQuery query) {
        return stringOrNull(call("engine.selectList", args("query", MirJson.query(query))));
    }

    @Override
    public String collectionSplice(SqlExpr.PlanParam param) {
        return string(call("engine.collectionSplice", args("param", MirJson.expr(param))));
    }

    @Override
    public String renderedAlias(String alias, @Nullable SqlQuery rendered) {
        Map<String, Object> a = args("alias", alias);
        if (rendered != null) {
            a.put("rendered", MirJson.query(rendered));
        }
        return string(call("engine.renderedAlias", a));
    }

    @Override
    public String inTempTableSelect(String tempTable) {
        return string(call("engine.inTempTableSelect", args("table", tempTable)));
    }

    @Override
    public String toString() {
        return name + (server == null ? "" : " for " + server) + " (Rust)";
    }
}
