// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

package com.legend.sql.dialect;

import com.legend.base.Nullable;
import com.legend.sql.SqlDdl;
import com.legend.sql.SqlDml;
import com.legend.sql.SqlQuery;
import com.legend.sql.SqlType;

import java.util.List;
import java.util.OptionalInt;

/**
 * The MariaDB EXECUTION dialect -- since 2026-11-04 the Rust library's (docs/rust-port/PHASE1.md §4 step 6; rust/lite-dialect
 * src/mariadb.rs): every method is the Rust dialect's ({@link RustDialects#dialect}), which renders every input of the
 * golden oracles -- every function and aggregate arm it decides among them, rendered by this class's Java
 * (rust/oracles/mir/dialect-arms.jsonl.zst) -- exactly as this class's Java did. {@link MySql}, its subclass, is the MySQL profile of the same Rust dialect. The class keeps its
 * name for those who make one ({@code new MariaDb()}), and their refusals name it.
 */
public class MariaDb implements SqlDialect {

    private final SqlDialect rust;

    public MariaDb() {
        this("MariaDb");
    }

    /** The MariaDB-family dialect the Rust library names {@code name}: this one, or {@link MySql}'s. */
    MariaDb(String name) {
        this.rust = RustDialects.provider().dialect(name);
    }

    @Override
    public String render(SqlQuery query) {
        return rust.render(query);
    }

    @Override
    public String renderInline(SqlQuery query) {
        return rust.renderInline(query);
    }

    @Override
    public RenderedStatement renderStatement(SqlQuery query) {
        return rust.renderStatement(query);
    }

    @Override
    public List<String> sessionSetup() {
        return rust.sessionSetup();
    }

    @Override
    public boolean temporaryTables() {
        return rust.temporaryTables();
    }

    @Override
    public List<String> sessionTable(SqlDdl.CreateTable table, SqlDml.@Nullable InsertValues rows) {
        return rust.sessionTable(table, rows);
    }

    @Override
    public String physicalName(String name) {
        return rust.physicalName(name);
    }

    @Override
    public String storedName(String name, boolean declaredQuoted) {
        return rust.storedName(name, declaredQuoted);
    }

    @Override
    public SqlDialect forServer(String version) {
        SqlDialect d = rust.forServer(version);
        return d == rust ? this : d;
    }

    @Override
    public @Nullable List<String> serverVersions() {
        return rust.serverVersions();
    }

    @Override
    public boolean loadsRowsInBulk() {
        return rust.loadsRowsInBulk();
    }

    @Override
    public boolean relational() {
        return rust.relational();
    }

    @Override
    public boolean inProcess() {
        return rust.inProcess();
    }

    @Override
    public boolean dropsStagedTablesWithTheirTransaction() {
        return rust.dropsStagedTablesWithTheirTransaction();
    }

    @Override
    public String wireTypeName(@Nullable String driverTypeName) {
        return rust.wireTypeName(driverTypeName);
    }

    @Override
    public @Nullable String typingWireName(@Nullable String driverTypeName) {
        return rust.typingWireName(driverTypeName);
    }

    @Override
    public boolean deliversOwn(SqlType label, String wire) {
        return rust.deliversOwn(label, wire);
    }

    @Override
    public KeysSnapshot keysSnapshot() {
        return rust.keysSnapshot();
    }

    @Override
    public @Nullable Integer resultColumnLimit() {
        return rust.resultColumnLimit();
    }

    @Override
    public boolean needsStaticPivot() {
        return rust.needsStaticPivot();
    }

    @Override
    public String raw(String statement, @Nullable String writtenFor) {
        return rust.raw(statement, writtenFor);
    }

    @Override
    public String fromEngineText(String statement) {
        return rust.fromEngineText(statement);
    }

    @Override
    public @Nullable String inMemoryPath() {
        return rust.inMemoryPath();
    }

    @Override
    public HostOperations hostOperations() {
        return rust.hostOperations();
    }

    @Override
    public String script(List<String> statements) {
        return rust.script(statements);
    }

    @Override
    public @Nullable String scriptAbort() {
        return rust.scriptAbort();
    }

    @Override
    public OptionalInt failingStatement(String message, List<String> statements) {
        return rust.failingStatement(message, statements);
    }

    @Override
    public String render(SqlDdl ddl) {
        return rust.render(ddl);
    }

    @Override
    public String render(SqlDml dml) {
        return rust.render(dml);
    }

    @Override
    public RenderedStatement renderStatement(SqlDml dml) {
        return rust.renderStatement(dml);
    }

    @Override
    public String insertColumnName(String name, boolean declaredQuoted) {
        return rust.insertColumnName(name, declaredQuoted);
    }

    @Override
    public CatalogType catalogType(CatalogModel.Column column) {
        return rust.catalogType(column);
    }

    @Override
    public String catalogColumnsQuery(String schema, String table) {
        return rust.catalogColumnsQuery(schema, table);
    }

    @Override
    public boolean bareNameReaches(String catalogName) {
        return rust.bareNameReaches(catalogName);
    }

    @Override
    public String copySelectList(List<CatalogModel.Conversion> conversions) {
        return rust.copySelectList(conversions);
    }
}
