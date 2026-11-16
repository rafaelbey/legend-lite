// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

package com.legend.compiler.spec;

import com.legend.compiler.element.ModelContext;
import com.legend.error.NotImplementedException;
import org.junit.jupiter.api.DisplayName;
import org.junit.jupiter.api.Test;

import static org.junit.jupiter.api.Assertions.assertDoesNotThrow;
import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertThrows;
import static org.junit.jupiter.api.Assertions.assertTrue;

/**
 * The owner's ruling 42, the G census's ASK 12 (the typer slice's stage 1a item 4; invariant 4): the execution-context
 * reader's silent defaults are refused by name -- a setup or url text that does not fold, a helper nested below its
 * depth cap that holds what the walk collects, a raw helper call whose bare name has several referents, a connection's
 * quoteIdentifiers that does not fold (where the plan text reads it) -- and a let re-bound to a value of its own name,
 * whose chase cycled or overflowed the stack, is refused by name too.
 */
class ContextReadingRefusalsTest {

    private static final String MODEL = """
            ###Pure
            import t::a::*;
            import t::b::*;
            Class t::Auth extends meta::pure::alloy::connections::alloy::authentication::AuthenticationStrategy {}
            Class t::P { name: String[1]; }
            Class t::S { name: String[1]; }
            function t::setupText(): String[1] { 'insert into T (ID) values (1)' }
            function t::csvText(): String[1] { 'default\\nT\\nID\\n1\\n' }
            function t::url(): String[1] { 'data:application/json,{"name":"a"}' }
            function t::rtF(): meta::core::runtime::Runtime[1] { ^meta::core::runtime::Runtime(connectionStores=^meta::core::runtime::ConnectionStore(element=t::Db, connection=^meta::external::store::relational::runtime::RelationalDatabaseConnection(type=meta::relational::runtime::DatabaseType.H2, datasourceSpecification=^meta::pure::alloy::connections::alloy::specification::LocalH2DatasourceSpecification(testDataSetupSqls=t::setupText()), authenticationStrategy=^t::Auth()))) }
            function t::rtCsvF(): meta::core::runtime::Runtime[1] { ^meta::core::runtime::Runtime(connectionStores=^meta::core::runtime::ConnectionStore(element=t::Db, connection=^meta::external::store::relational::runtime::RelationalDatabaseConnection(type=meta::relational::runtime::DatabaseType.H2, datasourceSpecification=^meta::pure::alloy::connections::alloy::specification::LocalH2DatasourceSpecification(testDataSetupCsv=t::csvText()), authenticationStrategy=^t::Auth()))) }
            function t::d0(): meta::core::runtime::Runtime[1] { t::d1() }
            function t::d1(): meta::core::runtime::Runtime[1] { t::d2() }
            function t::d2(): meta::core::runtime::Runtime[1] { t::d3() }
            function t::d3(): meta::core::runtime::Runtime[1] { ^meta::core::runtime::Runtime(connectionStores=^meta::core::runtime::ConnectionStore(element=t::Db, connection=^meta::external::store::relational::runtime::RelationalDatabaseConnection(type=meta::relational::runtime::DatabaseType.H2, datasourceSpecification=^meta::pure::alloy::connections::alloy::specification::LocalH2DatasourceSpecification(testDataSetupSqls='insert into T (ID) values (1)'), authenticationStrategy=^t::Auth()))) }
            function t::e0(): meta::core::runtime::Runtime[1] { t::e1() }
            function t::e1(): meta::core::runtime::Runtime[1] { t::e2() }
            function t::e2(): meta::core::runtime::Runtime[1] { t::e3() }
            function t::e3(): meta::core::runtime::Runtime[1] { t::e4() }
            function t::e4(): meta::core::runtime::Runtime[1] { ^meta::core::runtime::Runtime(connectionStores=^meta::core::runtime::ConnectionStore(element=t::Db, connection=^meta::external::store::relational::runtime::RelationalDatabaseConnection(type=meta::relational::runtime::DatabaseType.H2, datasourceSpecification=^meta::pure::alloy::connections::alloy::specification::LocalH2DatasourceSpecification(testDataSetupSqls='insert into T (ID) values (1)'), authenticationStrategy=^t::Auth()))) }
            function t::a::conn(x: Integer[1]): meta::core::runtime::Runtime[1] { ^meta::core::runtime::Runtime(connectionStores=^meta::core::runtime::ConnectionStore(element=t::Db, connection=^meta::external::store::relational::runtime::RelationalDatabaseConnection(type=meta::relational::runtime::DatabaseType.H2, datasourceSpecification=^meta::pure::alloy::connections::alloy::specification::LocalH2DatasourceSpecification(testDataSetupSqls='insert into T (ID) values (1)'), authenticationStrategy=^t::Auth()))) }
            function t::b::conn(x: String[1]): meta::core::runtime::Runtime[1] { ^meta::core::runtime::Runtime(connectionStores=^meta::core::runtime::ConnectionStore(element=t::Db, connection=^meta::external::store::relational::runtime::RelationalDatabaseConnection(type=meta::relational::runtime::DatabaseType.H2, datasourceSpecification=^meta::pure::alloy::connections::alloy::specification::LocalH2DatasourceSpecification(), authenticationStrategy=^t::Auth()))) }
            function t::a::only(x: Integer[1]): meta::core::runtime::Runtime[1] { ^meta::core::runtime::Runtime(connectionStores=^meta::core::runtime::ConnectionStore(element=t::Db, connection=^meta::external::store::relational::runtime::RelationalDatabaseConnection(type=meta::relational::runtime::DatabaseType.H2, datasourceSpecification=^meta::pure::alloy::connections::alloy::specification::LocalH2DatasourceSpecification(testDataSetupSqls='insert into T (ID) values (1)'), authenticationStrategy=^t::Auth()))) }
            function t::c::multi(): meta::core::runtime::Runtime[1] { conn(1) }
            function t::c::single(): meta::core::runtime::Runtime[1] { only(1) }
            ###Relational
            Database t::Db ( Table T (ID INTEGER PRIMARY KEY, NAME VARCHAR(20)) )
            ###Mapping
            Mapping t::M ( t::P: Relational { ~mainTable [t::Db]T name: [t::Db]T.NAME } )
            ###Connection
            RelationalDatabaseConnection t::Conn { store: t::Db; type: H2; specification: LocalH2 { }; auth: DefaultH2; }
            ###Runtime
            Runtime t::R { mappings: [t::M]; connections: [ t::Db: [ c: t::Conn ] ]; }
            """;

    private static final ModelContext CTX = com.legend.Compiler.compileModel(MODEL);

    private static String typing(String query) {
        return assertThrows(NotImplementedException.class,
                () -> com.legend.Compiler.query(CTX, query).resultType()).getMessage();
    }

    @Test
    @DisplayName("a setup text a helper computes is refused where it was dropped (the plan went on with setup=[])")
    void aComputedSetupTextIsRefusedNotDropped() {
        assertEquals("testDataSetupSqls is not statically readable: the execution-context reader folds string"
                + " literals and '+' of them (a helper's body: and its own lets); a computed or let-bound"
                + " testDataSetupSqls is refused, never dropped", typing("t::P.all()->project([x|$x.name], ['n'])->from(t::M, t::rtF())"));
        assertTrue(typing("t::P.all()->project([x|$x.name], ['n'])->from(t::M, t::rtCsvF())").startsWith("testDataSetupCsv is not statically readable"));
    }

    @Test
    @DisplayName("a setup below the depth cap is refused by name; three helpers deep it is still read")
    void aSetupBelowTheDepthCapIsRefused() {
        assertEquals("the runtime's helper calls nest deeper than the execution-context reader follows (3), and"
                + " 't::e4' below the third holds setup texts: refused, never dropped; build the connection nearer"
                + " the query", typing("t::P.all()->project([x|$x.name], ['n'])->from(t::M, t::e0())"));
        assertDoesNotThrow(() -> com.legend.Compiler.query(CTX, "t::P.all()->project([x|$x.name], ['n'])->from(t::M, t::d0())").resultType());
    }

    @Test
    @DisplayName("a raw helper call whose bare name has two referents is refused by name; one referent is followed")
    void aHelperCallOfSeveralReferentsIsRefused() {
        assertEquals("a helper call 'conn' names several functions (t::a::conn, t::b::conn): the execution-context"
                + " reader reads an unchecked body by the function it names and cannot choose one; call it by its"
                + " full name", typing("t::P.all()->project([x|$x.name], ['n'])->from(t::M, t::c::multi())"));
        assertDoesNotThrow(() -> com.legend.Compiler.query(CTX, "t::P.all()->project([x|$x.name], ['n'])->from(t::M, t::c::single())").resultType());
    }

    @Test
    @DisplayName("a computed url is refused where it was dropped")
    void aComputedUrlIsRefused() {
        assertTrue(typing("t::P.all()->project([x|$x.name], ['n'])->from(t::M, ^meta::core::runtime::Runtime(connectionStores=^"
                + "meta::core::runtime::ConnectionStore(element='ModelStore', connection=^meta::external::store::model:"
                + ":JsonModelConnection(class=t::S, url=t::url()))))").startsWith("JsonModelConnection url is not statically readable"));
    }

    @Test
    @DisplayName("a computed quoteIdentifiers is refused where the plan text reads it, never assumed false")
    void aComputedQuoteIdentifiersIsRefusedWhereItIsRead() {
        com.legend.TypedQuery computed = com.legend.Compiler.query(CTX, "meta::pure::executionPlan::executionPlan(|t::P.all()->project([x|$x.name], ['n']), t::M, ^meta::core"
                + "::runtime::Runtime(connectionStores=^meta::core::runtime::ConnectionStore(element=t::Db, connection="
                + "^meta::external::store::relational::runtime::RelationalDatabaseConnection(type=meta::relational::run"
                + "time::DatabaseType.H2, datasourceSpecification=^meta::pure::alloy::connections::alloy::specification"
                + "::LocalH2DatasourceSpecification(), authenticationStrategy=^t::Auth(), quoteIdentifiers=today() > %2"
                + "020-01-01))), [])->meta::pure::executionPlan::toString::planToString([])");
        assertEquals("String[1]", computed.resultType().type().typeName() + computed.resultType().multiplicity().text());
        assertTrue(assertThrows(NotImplementedException.class, () -> computed.executionPlan("t::R")).getMessage()
                .startsWith("the connection's quoteIdentifiers is not statically readable"));
        assertDoesNotThrow(() -> com.legend.Compiler.query(CTX, "meta::pure::executionPlan::executionPlan(|t::P.all()->project([x|$x.name], ['n']), t::M, ^meta::core"
                + "::runtime::Runtime(connectionStores=^meta::core::runtime::ConnectionStore(element=t::Db, connection="
                + "^meta::external::store::relational::runtime::RelationalDatabaseConnection(type=meta::relational::run"
                + "time::DatabaseType.H2, datasourceSpecification=^meta::pure::alloy::connections::alloy::specification"
                + "::LocalH2DatasourceSpecification(), authenticationStrategy=^t::Auth(), quoteIdentifiers=true))), [])"
                + "->meta::pure::executionPlan::toString::planToString([])").executionPlan("t::R"));
    }

    @Test
    @DisplayName("a let re-bound to a value of its own name: the reader refuses by name where its chase overflowed")
    void aReboundLetIsRefusedByName() {
        com.legend.TypedQuery q = com.legend.Compiler.query(CTX, "{| let c = 1; let c = $c + 1; $c; }");
        assertEquals("Integer[1]", q.resultType().type().typeName() + q.resultType().multiplicity().text());
        assertEquals("the execution context's let chase cycles: '$c' is bound to a value that reads '$c' again -- a"
                + " let re-bound to a value of its own name (let c = ...; let c = f($c)); rename one of them",
                assertThrows(NotImplementedException.class,
                        () -> com.legend.Compiler.programFacts(q.resolved(), CTX)).getMessage());
    }
}
