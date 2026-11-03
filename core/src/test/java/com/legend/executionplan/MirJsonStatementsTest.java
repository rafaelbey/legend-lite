// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

package com.legend.executionplan;

import com.legend.json.Json;
import com.legend.sql.NameQuoting;
import com.legend.sql.SqlDdl;
import com.legend.sql.SqlDml;
import com.legend.sql.SqlExpr;
import com.legend.sql.SqlQuery;
import com.legend.sql.SqlSelect;
import com.legend.sql.SqlSource;
import org.junit.jupiter.api.Test;

import java.util.List;
import java.util.Map;
import java.util.Set;

import static org.junit.jupiter.api.Assertions.assertEquals;

/**
 * The statements a dialect renders beside its queries, as JSON and back (docs/rust-port/ROADMAP.md, phase 0 step 4):
 * every {@link SqlDdl} and {@link SqlDml} variant, written as text and read again, is the same record, and writes the
 * same text again -- the back seam hands the Rust dialect these exactly as the Java one received them.
 */
class MirJsonStatementsTest {

    private static final NameQuoting BARE = new NameQuoting(false, false);
    private static final NameQuoting QUOTED_SCHEMA = new NameQuoting(true, false);

    private static final SqlQuery ONE = new SqlSelect(List.of(new SqlSelect.Projection(new SqlExpr.IntLit(1), "v", null)),
            false, new SqlSource.Dual(), null, List.of(), null, null, List.of(), null, null, List.of());

    private static final SqlDdl.ColumnType DECIMAL = new SqlDdl.ColumnType.Scaled("DECIMAL", 38, 6);

    @Test
    void everyDdlStatementRoundTrips() {
        List<SqlDdl> statements = List.of(
                new SqlDdl.CreateTable(null, "T", BARE, List.of(
                        new SqlDdl.Column("ID", false, new SqlDdl.ColumnType.Plain(SqlDdl.ColumnType.Kind.INTEGER),
                                true, true),
                        new SqlDdl.Column("name", true, new SqlDdl.ColumnType.Sized("VARCHAR", 200), false, false),
                        new SqlDdl.Column("amount", false, DECIMAL, false, false)), true),
                new SqlDdl.CreateTable("s", "T", QUOTED_SCHEMA, List.of()),
                new SqlDdl.DropTable(null, "T", BARE),
                new SqlDdl.DropTable("s", "T", QUOTED_SCHEMA),
                new SqlDdl.CreateSchema("s", true),
                new SqlDdl.DropSchema("s", false));
        for (SqlDdl d : statements) {
            String text = Json.toCompact(MirJson.ddl(d));
            SqlDdl back = MirJson.readDdl((Json.Obj) Json.parse(text));
            assertEquals(d, back, text);
            assertEquals(text, Json.toCompact(MirJson.ddl(back)));
        }
    }

    @Test
    void everyDmlStatementRoundTrips() {
        List<SqlDml> statements = List.of(
                new SqlDml.InsertValues(null, "T", BARE, List.of("ID", "name"), Set.of("name"), List.of(
                        List.of(new SqlExpr.IntLit(1), new SqlExpr.StringLit("a'b")),
                        List.of(new SqlExpr.IntLit(2), new SqlExpr.NullLit()))),
                new SqlDml.InsertFromTable("s", "T", QUOTED_SCHEMA, List.of("ID"), Set.of(), "staging"),
                new SqlDml.DeleteAll(null, "T", BARE),
                new SqlDml.CreateTempTableAs("tmp", ONE),
                SqlDml.CreateTempTableAs.staged("tmp", ONE, List.of("v")),
                new SqlDml.InsertSelect(null, "T", BARE, List.of("ID"), Set.of("ID"), ONE),
                new SqlDml.Update(null, "T", BARE, "t", Map.of("amount", DECIMAL),
                        List.of(new SqlDml.Update.Assignment("amount", false, new SqlExpr.IntLit(3))),
                        new SqlExpr.BoolLit(true)),
                new SqlDml.Update("s", "T", QUOTED_SCHEMA, "t", Map.of(),
                        List.of(new SqlDml.Update.Assignment("name", true, new SqlExpr.StringLit("x"))), null),
                new SqlDml.DeleteWhere(null, "T", BARE, "t", Map.of("amount", DECIMAL), new SqlExpr.BoolLit(false)),
                new SqlDml.DeleteWhere(null, "T", BARE, "t", Map.of(), null),
                new SqlDml.Merge(null, "T", BARE, List.of("ID", "name"), Set.of("name"), List.of("ID"), ONE, "t",
                        "s"));
        for (SqlDml d : statements) {
            String text = Json.toCompact(MirJson.dml(d));
            SqlDml back = MirJson.readDml((Json.Obj) Json.parse(text));
            assertEquals(d, back, text);
            assertEquals(text, Json.toCompact(MirJson.dml(back)));
        }
    }
}
