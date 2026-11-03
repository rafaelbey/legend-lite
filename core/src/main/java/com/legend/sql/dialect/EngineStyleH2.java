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
 * legend-engine's own SQL TEXT for its test database H2 (the engine's {@code toSQLString} and plan goldens) -- since
 * 2026-11-03 the Rust library's (docs/rust-port/PHASE1.md §4 step 4; rust/lite-dialect src/engine_style.rs): every method
 * is the Rust dialect's ({@link RustDialects#engineText}), which writes every engine-text input of the golden oracles
 * exactly as this class's Java did. Never executed: the text is compared with the engine's.
 *
 * <p>The library holds no state between calls, and this dialect two pieces of it: the query it last rendered, whose
 * alias plan {@link #renderedAlias} reads (as Java's read the plan its last render made), and nothing else. A tagged
 * value -- which legend-engine has none of -- goes to the library as its comparable ({@link TaggedComparables}), the
 * text Java wrote for it.
 */
public class EngineStyleH2 implements SqlDialect, EngineTextDialect {

    private final RustDialects.EngineText rust;

    /** The query this dialect last rendered (or asked the select list of), as sent: the alias plan
     *  {@link #renderedAlias} reads; none after {@link #renderAsAliased}, which plans none. */
    private @Nullable SqlQuery lastRendered;

    public EngineStyleH2() {
        this(false);
    }

    public EngineStyleH2(boolean quoteIdentifiers) {
        this(quoteIdentifiers, null);
    }

    /** {@code quoteIdentifiers}: every physical identifier double-quoted (the plan text's connection); {@code timeZone}:
     *  the connection's zone, a date-time constant shifted to it from GMT. Null = default. */
    public EngineStyleH2(boolean quoteIdentifiers, @Nullable String timeZone) {
        this("EngineStyleH2", quoteIdentifiers, timeZone);
    }

    /** The engine-text dialect the Rust library names {@code name}: this one's, or a subclass's. */
    EngineStyleH2(String name, boolean quoteIdentifiers, @Nullable String timeZone) {
        this.rust = RustDialects.provider().engineText(name, quoteIdentifiers, timeZone);
    }

    /** The engine text is written for no server: it keeps itself. */
    @Override
    public SqlDialect forServer(String version) {
        return this;
    }

    @Override
    public String render(SqlQuery query) {
        SqlQuery sent = TaggedComparables.replaced(query);
        String text = rust.render(sent);
        lastRendered = sent;
        return text;
    }

    @Override
    public String renderInline(SqlQuery query) {
        SqlQuery sent = TaggedComparables.replaced(query);
        String text = rust.renderInline(sent);
        lastRendered = sent;
        return text;
    }

    /** The legacy plan text binds nothing: refused (its parameters are template variables in its text). */
    @Override
    public RenderedStatement renderStatement(SqlQuery query) {
        return rust.renderStatement(TaggedComparables.replaced(query));
    }

    @Override
    public String renderAsAliased(SqlQuery query) {
        String text = rust.renderAsAliased(TaggedComparables.replaced(query));
        // Java's renderAsAliased cleared the alias plan its last render made
        lastRendered = null;
        return text;
    }

    @Override
    public @Nullable String selectList(SqlQuery query) {
        SqlQuery sent = TaggedComparables.replaced(query);
        String list = rust.selectList(sent);
        lastRendered = sent;
        return list;
    }

    @Override
    public String collectionSplice(com.legend.sql.SqlExpr.PlanParam cp) {
        return rust.collectionSplice(cp);
    }

    @Override
    public String renderedAlias(String alias) {
        return rust.renderedAlias(alias, lastRendered);
    }

    @Override
    public String inTempTableSelect(String tempTable) {
        return rust.inTempTableSelect(tempTable);
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
