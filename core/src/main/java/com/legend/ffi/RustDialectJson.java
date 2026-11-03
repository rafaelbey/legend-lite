// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

package com.legend.ffi;

import com.legend.sql.dialect.HostOperation;

import java.util.ArrayList;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;

/**
 * A host operation as the seam reads one (rust/lite-dialect/SEAM.md §4): the encoding the browser's planner reads
 * (wasm/src/main/java/planner/HostOperationsJson.java), written -- one operation object, its fields by that reader's
 * names, a source by the first of its keys.
 */
final class RustDialectJson {

    private RustDialectJson() {
    }

    static Map<String, Object> operation(HostOperation op) {
        Map<String, Object> o = new LinkedHashMap<>();
        switch (op) {
            case HostOperation.MakeRelation r -> {
                o.put("op", "relation");
                o.put("kind", kind(r.kind()));
                o.put("target", name(r.target()));
                o.put("replace", r.replace());
                o.put("selectList", r.selectList());
                List<Object> replacing = new ArrayList<>();
                for (HostOperation.Conversion c : r.replacing()) {
                    Map<String, Object> m = new LinkedHashMap<>();
                    m.put("column", c.column());
                    m.put("sql", c.sql());
                    replacing.add(m);
                }
                o.put("replacing", replacing);
                o.put("from", source(r.from()));
            }
            case HostOperation.Insert i -> {
                o.put("op", "insert");
                o.put("target", name(i.target()));
                List<Object> columns = new ArrayList<>();
                for (HostOperation.Column c : i.columns()) {
                    Map<String, Object> m = new LinkedHashMap<>();
                    m.put("target", c.target());
                    m.put("from", c.from());
                    columns.add(m);
                }
                o.put("columns", columns);
                o.put("from", source(i.from()));
            }
            case HostOperation.Drop d -> {
                o.put("op", "drop");
                o.put("kind", kind(d.kind()));
                o.put("target", name(d.target()));
            }
            case HostOperation.MakeSchema s -> {
                o.put("op", "schema");
                o.put("database", s.database());
                o.put("name", s.name());
            }
            case HostOperation.DropSchema s -> {
                o.put("op", "dropSchema");
                o.put("database", s.database());
                o.put("name", s.name());
            }
            case HostOperation.Count c -> {
                o.put("op", "count");
                o.put("from", source(c.from()));
            }
            case HostOperation.Secret s -> {
                o.put("op", "secret");
                o.put("name", s.name());
                o.put("scope", s.scope());
                o.put("credentials", new LinkedHashMap<>(s.credentials()));
            }
            case HostOperation.Extensions e -> {
                o.put("op", "extensions");
                o.put("names", e.names());
            }
            case HostOperation.ExtensionRepository r -> {
                o.put("op", "extensionRepository");
                o.put("url", r.url());
            }
            case HostOperation.ExtensionDirectory d -> {
                o.put("op", "extensionDirectory");
                o.put("folder", d.folder());
            }
            case HostOperation.FileSearchPath p -> {
                o.put("op", "fileSearchPath");
                o.put("folder", p.folder());
            }
            case HostOperation.Transaction t -> {
                o.put("op", "transaction");
                o.put("step", switch (t.step()) {
                    case BEGIN -> "begin";
                    case COMMIT -> "commit";
                    case ROLLBACK -> "rollback";
                });
            }
            case HostOperation.NameTaken n -> {
                o.put("op", "nameTaken");
                o.put("name", n.name());
            }
            case HostOperation.RelationExists r -> {
                o.put("op", "relationExists");
                o.put("name", name(r.name()));
            }
            case HostOperation.TablesIn t -> {
                o.put("op", "tablesIn");
                o.put("database", t.database());
                o.put("schema", t.schema());
            }
            case HostOperation.SchemaExists s -> {
                o.put("op", "schemaExists");
                o.put("database", s.database());
                o.put("schema", s.schema());
            }
            case HostOperation.CurrentDatabase c -> o.put("op", "currentDatabase");
            case HostOperation.Platform p -> o.put("op", "platform");
            case HostOperation.IcebergManifestLists l -> {
                o.put("op", "icebergManifestLists");
                o.put("url", l.url());
            }
            case HostOperation.IcebergManifests m -> {
                o.put("op", "icebergManifests");
                o.put("lists", m.lists());
            }
            case HostOperation.IcebergDataFiles f -> {
                o.put("op", "icebergDataFiles");
                o.put("url", f.url());
            }
        }
        return o;
    }

    private static String kind(HostOperation.Kind k) {
        return switch (k) {
            case TABLE -> "table";
            case VIEW -> "view";
        };
    }

    private static Map<String, Object> name(HostOperation.Name n) {
        Map<String, Object> o = new LinkedHashMap<>();
        o.put("database", n.database());
        o.put("schema", n.schema());
        o.put("name", n.name());
        o.put("declared", n.declared());
        return o;
    }

    private static Map<String, Object> source(HostOperation.Source s) {
        Map<String, Object> o = new LinkedHashMap<>();
        switch (s) {
            case HostOperation.Source.Relation r -> o.put("relation", name(r.name()));
            case HostOperation.Source.File f -> {
                Map<String, Object> file = new LinkedHashMap<>();
                file.put("at", f.at());
                file.put("format", switch (f.format()) {
                    case CSV -> "csv";
                    case CSV_TEXT -> "csvText";
                    case PARQUET -> "parquet";
                    case JSON -> "json";
                });
                o.put("file", file);
            }
            case HostOperation.Source.Remote r -> {
                Map<String, Object> remote = new LinkedHashMap<>();
                remote.put("url", r.url());
                remote.put("format", switch (r.format()) {
                    case CSV -> "csv";
                    case PARQUET -> "parquet";
                    case ICEBERG -> "iceberg";
                });
                o.put("remote", remote);
            }
            case HostOperation.Source.Query q -> o.put("query", q.sql());
        }
        return o;
    }
}
