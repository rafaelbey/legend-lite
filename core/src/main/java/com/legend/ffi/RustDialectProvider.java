// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

package com.legend.ffi;

import com.legend.base.Nullable;
import com.legend.executionplan.MirJson;
import com.legend.json.Json;
import com.legend.sql.SqlDdl;
import com.legend.sql.dialect.CatalogType;
import com.legend.sql.dialect.Lexicon;
import com.legend.sql.dialect.RustDialects;
import com.legend.sql.dialect.SqlDialect;

import java.util.Collections;
import java.util.LinkedHashMap;
import java.util.LinkedHashSet;
import java.util.Map;
import java.util.Set;

/**
 * {@link RustDialects} over the Rust library (META-INF/services names it): a dialect is a {@link RustDialect}, DuckDB's
 * tables come from the seam's {@code duckdb.tables}, and a dialect's stored reads and table names from
 * {@code readsStored} and {@code tableName} (rust/lite-dialect/SEAM.md).
 */
public final class RustDialectProvider implements RustDialects {

    @Override
    public SqlDialect dialect(String name) {
        return new RustDialect(name);
    }

    @Override
    public RustDialects.EngineText engineText(String name, boolean quoteIdentifiers, @Nullable String timeZone) {
        return new RustDialect(name, quoteIdentifiers, timeZone);
    }

    @Override
    public String forServer(String name, String version) {
        Map<String, Object> args = new LinkedHashMap<>();
        args.put("version", version);
        String answer;
        try {
            answer = Lite.shared().dialect(spec(name), "forServer", Json.toCompact(args));
        } catch (LiteRefusal refusal) {
            throw RustDialect.rethrown(refusal.kind(), String.valueOf(refusal.getMessage()));
        }
        return ((Json.Str) Json.parse(answer)).value();
    }

    private static String spec(String name) {
        Map<String, Object> s = new LinkedHashMap<>();
        s.put("dialect", name);
        s.put("server", null);
        s.put("engineScanOrder", false);
        return Json.toCompact(s);
    }

    private static Json.Obj tables(String name) {
        String answer;
        try {
            answer = Lite.shared().dialect(spec(name), "duckdb.tables", "{}");
        } catch (LiteRefusal refusal) {
            throw RustDialect.rethrown(refusal.kind(), String.valueOf(refusal.getMessage()));
        }
        return (Json.Obj) Json.parse(answer);
    }

    @Override
    public Lexicon lexicon(String name) {
        Json.Obj l = tables(name).getObj("lexicon");
        Set<String> words = new LinkedHashSet<>();
        for (Json.Node w : l.getArr("reservedWords").items()) {
            words.add(((Json.Str) w).value());
        }
        return new Lexicon(l.getString("quoteChar").charAt(0), words);
    }

    @Override
    public Map<String, CatalogType> catalogTypes(String name) {
        Map<String, CatalogType> out = new LinkedHashMap<>();
        for (Map.Entry<String, Json.Node> e : tables(name).getObj("catalogTypes").fields().entrySet()) {
            Json.Obj t = (Json.Obj) e.getValue();
            out.put(e.getKey(), new CatalogType(CatalogType.Read.valueOf(t.getString("read")),
                    t.getStringOr("declared", null), t.getStringOr("conversion", null), t.getStringOr("reason", null)));
        }
        return Collections.unmodifiableMap(out);
    }

    @Override
    public Map<String, String> catalogRefused(String name) {
        Map<String, String> out = new LinkedHashMap<>();
        for (Map.Entry<String, Json.Node> e : tables(name).getObj("catalogRefused").fields().entrySet()) {
            out.put(e.getKey(), ((Json.Str) e.getValue()).value());
        }
        return Collections.unmodifiableMap(out);
    }

    @Override
    public boolean readsStored(String name, SqlDdl.ColumnType type) {
        Map<String, Object> args = new LinkedHashMap<>();
        args.put("type", MirJson.columnType(type));
        String answer;
        try {
            answer = Lite.shared().dialect(spec(name), "readsStored", Json.toCompact(args));
        } catch (LiteRefusal refusal) {
            throw RustDialect.rethrown(refusal.kind(), String.valueOf(refusal.getMessage()));
        }
        return Boolean.parseBoolean(answer);
    }

    @Override
    public String tableName(String name, String table, com.legend.sql.NameQuoting quoting) {
        Map<String, Object> q = new LinkedHashMap<>();
        q.put("schema", quoting.schema());
        q.put("table", quoting.table());
        Map<String, Object> args = new LinkedHashMap<>();
        args.put("name", table);
        args.put("quoting", q);
        String answer;
        try {
            answer = Lite.shared().dialect(spec(name), "tableName", Json.toCompact(args));
        } catch (LiteRefusal refusal) {
            throw RustDialect.rethrown(refusal.kind(), String.valueOf(refusal.getMessage()));
        }
        return ((Json.Str) Json.parse(answer)).value();
    }

    @Override
    public long scanOrderFirings() {
        String answer;
        try {
            answer = Lite.shared().dialect(spec("DuckDb"), "scanOrderFirings", "{}");
        } catch (LiteRefusal refusal) {
            throw RustDialect.rethrown(refusal.kind(), String.valueOf(refusal.getMessage()));
        }
        return Long.parseLong(answer);
    }
}
