// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

package com.legend.sql.dialect;

import com.legend.base.Nullable;
import com.legend.sql.SqlDdl;
import com.legend.sql.SqlDml;
import com.legend.sql.SqlQuery;
import com.legend.sql.SqlType;

import java.util.List;

/**
 * The H2 EXECUTION dialect -- since 2026-11-03 the Rust library's (docs/rust-port/PHASE1.md §4 step 4; rust/lite-dialect
 * src/h2.rs): every method is the Rust dialect's ({@link RustDialects#dialect}), which renders every H2 input of the
 * golden oracles exactly as this class's Java did. Its contract is unchanged: EXECUTABLE SQL for lite's ONE H2, 2.5.252
 * (since `main`'s 2026-11-05: the {@code H2Modern} profile, chosen by the connected server's version, went with the
 * 2.1.214 jar, register S141; merged into the port 2026-11-10), every spelling probed against the real jar, anything H2
 * cannot express a loud wall. The class keeps its name for those who make one ({@code new H2()}).
 */
public class H2 implements SqlDialect {

    private final SqlDialect rust = RustDialects.provider().dialect("H2");

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
    public boolean loadsRowsInBulk() {
        return rust.loadsRowsInBulk();
    }

    @Override
    public CsvLoad csvLoad(SqlDml.LoadCsv load) {
        return rust.csvLoad(load);
    }

    @Override
    public @Nullable VerdictSavepoint verdictSavepoint() {
        return rust.verdictSavepoint();
    }

    @Override
    public int fusedStatementBudget() {
        return rust.fusedStatementBudget();
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
