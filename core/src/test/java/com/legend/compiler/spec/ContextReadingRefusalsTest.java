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

    /** MODEL with the helpers the url and chain tests read (2026-11-16, the typer slice's stage 1b, part 1.2): a text of
     *  its own, so MODEL's own questions keep their keys in rust/oracles/identity. */
    private static final ModelContext HELPERS = com.legend.Compiler.compileModel(MODEL + """
            ###Pure
            function t::jrtF(): meta::core::runtime::Runtime[1] { ^meta::core::runtime::Runtime(connectionStores=^meta::core::runtime::ConnectionStore(element='ModelStore', connection=^meta::external::store::model::JsonModelConnection(class=t::S, url=t::url()))) }
            function t::c0(): meta::core::runtime::Runtime[1] { t::c1() }
            function t::c1(): meta::core::runtime::Runtime[1] { t::c2() }
            function t::c2(): meta::core::runtime::Runtime[1] { t::c3() }
            function t::c3(): meta::core::runtime::Runtime[1] { t::c4() }
            function t::c4(): meta::core::runtime::Runtime[1] { ^meta::core::runtime::Runtime(connectionStores=^meta::core::runtime::ConnectionStore(element='ModelStore', connection=^meta::external::store::model::ModelChainConnection(mappings=[t::M]))) }
            """);

    /** MODEL with helpers of two overloads each (2026-11-17, the typer slice's stage 1c; the owner's ruling 44): a text
     *  of its own, so MODEL's own questions keep their keys in rust/oracles/identity. t::ar's setup is in its
     *  two-argument overload, t::br's in its one-argument one (port-scratch/typer1b/research/cr4, cases/m_cr4.pure);
     *  t::sv's two one-argument overloads differ by their parameter's type alone. */
    private static final ModelContext ARITY = com.legend.Compiler.compileModel(MODEL + """
            ###Pure
            function t::ar(x: Integer[1]): meta::core::runtime::Runtime[1] { ^meta::core::runtime::Runtime(connectionStores=^meta::core::runtime::ConnectionStore(element=t::Db, connection=^meta::external::store::relational::runtime::RelationalDatabaseConnection(type=meta::relational::runtime::DatabaseType.H2, datasourceSpecification=^meta::pure::alloy::connections::alloy::specification::LocalH2DatasourceSpecification(), authenticationStrategy=^t::Auth()))) }
            function t::ar(x: Integer[1], y: Integer[1]): meta::core::runtime::Runtime[1] { ^meta::core::runtime::Runtime(connectionStores=^meta::core::runtime::ConnectionStore(element=t::Db, connection=^meta::external::store::relational::runtime::RelationalDatabaseConnection(type=meta::relational::runtime::DatabaseType.H2, datasourceSpecification=^meta::pure::alloy::connections::alloy::specification::LocalH2DatasourceSpecification(testDataSetupSqls='insert into T (ID) values (1)'), authenticationStrategy=^t::Auth()))) }
            function t::arCall(): meta::core::runtime::Runtime[1] { t::ar(1, 2) }
            function t::br(x: Integer[1]): meta::core::runtime::Runtime[1] { ^meta::core::runtime::Runtime(connectionStores=^meta::core::runtime::ConnectionStore(element=t::Db, connection=^meta::external::store::relational::runtime::RelationalDatabaseConnection(type=meta::relational::runtime::DatabaseType.H2, datasourceSpecification=^meta::pure::alloy::connections::alloy::specification::LocalH2DatasourceSpecification(testDataSetupSqls='insert into T (ID) values (1)'), authenticationStrategy=^t::Auth()))) }
            function t::br(x: Integer[1], y: Integer[1]): meta::core::runtime::Runtime[1] { ^meta::core::runtime::Runtime(connectionStores=^meta::core::runtime::ConnectionStore(element=t::Db, connection=^meta::external::store::relational::runtime::RelationalDatabaseConnection(type=meta::relational::runtime::DatabaseType.H2, datasourceSpecification=^meta::pure::alloy::connections::alloy::specification::LocalH2DatasourceSpecification(), authenticationStrategy=^t::Auth()))) }
            function t::brCall(): meta::core::runtime::Runtime[1] { t::br(1, 2) }
            function t::sv(x: Integer[1]): meta::core::runtime::Runtime[1] { ^meta::core::runtime::Runtime(connectionStores=^meta::core::runtime::ConnectionStore(element=t::Db, connection=^meta::external::store::relational::runtime::RelationalDatabaseConnection(type=meta::relational::runtime::DatabaseType.H2, datasourceSpecification=^meta::pure::alloy::connections::alloy::specification::LocalH2DatasourceSpecification(testDataSetupSqls='insert into T (ID) values (1)'), authenticationStrategy=^t::Auth()))) }
            function t::sv(x: String[1]): meta::core::runtime::Runtime[1] { ^meta::core::runtime::Runtime(connectionStores=^meta::core::runtime::ConnectionStore(element=t::Db, connection=^meta::external::store::relational::runtime::RelationalDatabaseConnection(type=meta::relational::runtime::DatabaseType.H2, datasourceSpecification=^meta::pure::alloy::connections::alloy::specification::LocalH2DatasourceSpecification(), authenticationStrategy=^t::Auth()))) }
            function t::svCall(): meta::core::runtime::Runtime[1] { t::sv('s') }
            """);

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

    /** An instance runtime over t::Db whose datasource specification is {@code datasourceSpecification}. */
    private static String instanceRuntime(String datasourceSpecification) {
        return "^meta::core::runtime::Runtime(connectionStores=^meta::core::runtime::ConnectionStore(element=t::Db,"
                + " connection=^meta::external::store::relational::runtime::RelationalDatabaseConnection(type=meta::"
                + "relational::runtime::DatabaseType.H2, datasourceSpecification=" + datasourceSpecification
                + ", authenticationStrategy=^t::Auth())))";
    }

    @Test
    @DisplayName("a typed runtime's computed testDataSetupSqls is refused where it was dropped (the plan went on with setup=[])")
    void aComputedSetupSqlsInATypedRuntimeIsRefused() {
        // ContextReading.collectSetups, a test-data specification's testDataSetupSqls (port-scratch/typer/research/
        // item34, A2a, run): the typed arm; aComputedSetupTextIsRefusedNotDropped's t::rtF is the raw body's
        assertEquals("testDataSetupSqls is not statically readable: the execution-context reader folds string"
                + " literals and '+' of them (a helper's body: and its own lets); a computed or let-bound"
                + " testDataSetupSqls is refused, never dropped", typing("t::P.all()->project([x|$x.name], ['n'])"
                + "->from(t::M, " + instanceRuntime("^meta::pure::alloy::connections::alloy::specification::"
                + "LocalH2DatasourceSpecification(testDataSetupSqls=t::setupText())") + ")"));
    }

    @Test
    @DisplayName("a typed runtime's computed testDataSetupCsv is refused where it was dropped (the plan went on with setup=[])")
    void aComputedSetupCsvInATypedRuntimeIsRefused() {
        // ContextReading.collectSetups, a connection class's testDataSetupCsv (item34, A2b, run): the typed arm;
        // aComputedSetupTextIsRefusedNotDropped's t::rtCsvF is the raw body's
        assertEquals("testDataSetupCsv is not statically readable: the execution-context reader folds string"
                + " literals and '+' of them (a helper's body: and its own lets); a computed or let-bound"
                + " testDataSetupCsv is refused, never dropped", typing("t::P.all()->project([x|$x.name], ['n'])"
                + "->from(t::M, " + instanceRuntime("^meta::pure::alloy::connections::alloy::specification::"
                + "LocalH2DatasourceSpecification(testDataSetupCsv=t::csvText())") + ")"));
    }

    @Test
    @DisplayName("a copy's computed testDataSetupCsv override is refused where it was dropped (the plan went on with setup=[])")
    void aComputedCsvOverrideOfACopyIsRefused() {
        // ContextReading.collectSetups, a copy's override (item34, A2c, run)
        assertEquals("testDataSetupCsv is not statically readable: the execution-context reader folds string"
                + " literals and '+' of them (a helper's body: and its own lets); a computed or let-bound"
                + " testDataSetupCsv is refused, never dropped", typing("{| let s = ^meta::pure::alloy::connections"
                + "::alloy::specification::LocalH2DatasourceSpecification(); t::P.all()->project([x|$x.name], ['n'])"
                + "->from(t::M, " + instanceRuntime("^$s(testDataSetupCsv=t::csvText())") + "); }"));
    }

    @Test
    @DisplayName("a helper body's JSON url that is not a string literal is refused where it was dropped")
    void aComputedUrlInAHelpersBodyIsRefused() {
        // ContextReading.collectJsonRaw: until 2026-11-16 the source was dropped and the class refused later as
        // unmapped (port-scratch/typer/research/item34, A2g, run)
        assertEquals("JsonModelConnection url in a helper's body is not a string literal: the execution-context"
                + " reader reads it as written; a computed url is refused, never dropped",
                assertThrows(NotImplementedException.class, () -> com.legend.Compiler.query(HELPERS,
                        "t::P.all()->project([x|$x.name], ['n'])->from(t::M, t::jrtF())").resultType()).getMessage());
    }

    @Test
    @DisplayName("a ModelChainConnection below the depth cap is refused by name; three helpers deep it is still read")
    void aChainBelowTheDepthCapIsRefused() {
        // ContextReading.collectChainRaw's cap: until 2026-11-16 the walk stopped there and the query routed without
        // the chain (port-scratch/typer/research/item34, A3a; the e0 shape of aSetupBelowTheDepthCapIsRefused)
        assertEquals("the runtime's helper calls nest deeper than the execution-context reader follows (3), and"
                + " 't::c4' below the third holds ModelChainConnection mappings: refused, never dropped; build the"
                + " connection nearer the query", assertThrows(NotImplementedException.class,
                () -> com.legend.Compiler.query(HELPERS, "t::P.all()->project([x|$x.name], ['n'])->from(t::M, t::c0())")
                        .resultType()).getMessage());
        assertDoesNotThrow(() -> com.legend.Compiler.query(HELPERS, "t::P.all()->project([x|$x.name], ['n'])->from(t::M, t::c1())").resultType());
    }

    @Test
    @DisplayName("a raw helper call is read through its overload of the call's arity; two of that arity are refused by name")
    void aHelperCallIsReadByItsArity() {
        // the owner's ruling 44 (the typer slice's stage 1c, 2026-11-17): the setups' and the chain's collectors read
        // the first overload with a body whatever its arity, so t::ar(1, 2) read t::ar(x) (no setup) and t::br(1, 2)
        // t::br(x)'s setup; legend-engine 4.145.0 plans t::ar(x, y)'s setup and none for t::br (run,
        // port-scratch/typer1b/research/cr4, out/e_cr4.txt e1-e2). The product door refuses a query's inline setup
        String q = "t::P.all()->project([x|$x.name], ['n'])->from(t::M, %s)";
        assertEquals("a from()'s inline test data in a query: seeds are a test's, and the product's query door plans"
                + " none", assertThrows(NotImplementedException.class, () -> com.legend.Compiler.query(ARITY,
                        q.formatted("t::arCall()")).executionPlan("t::R")).getMessage());
        assertDoesNotThrow(() -> com.legend.Compiler.query(ARITY, q.formatted("t::brCall()")).executionPlan("t::R"));
        assertEquals("a helper call 't::sv' has 2 overloads of its arity (1) with a body: the execution-context reader"
                + " reads an unchecked body by the call's arity and cannot choose one by its arguments' types; give the"
                + " helper one overload of that arity", assertThrows(NotImplementedException.class,
                        () -> com.legend.Compiler.query(ARITY, q.formatted("t::svCall()")).resultType()).getMessage());
    }

    @Test
    @DisplayName("a copy's testDataSetupSqls override is read as its csv override is: refused by name where it does"
            + " not fold, collected where it does (the plan went on with setup=[])")
    void aCopysSetupSqlsOverrideIsReadAsTheCsvOverrideIs() {
        // the owner's ruling 44 (the typer slice's stage 1c): the copy arm read testDataSetupCsv alone, so the plan went
        // on without the setup (port-scratch/typer1b/research/cr4, out/l_cr4.txt q3); legend-engine 4.145.0 cannot
        // parse ^$s(...) (run, e7)
        assertEquals("testDataSetupSqls is not statically readable: the execution-context reader folds string"
                + " literals and '+' of them (a helper's body: and its own lets); a computed or let-bound"
                + " testDataSetupSqls is refused, never dropped", typing("{| let s = ^meta::pure::alloy::connections"
                + "::alloy::specification::LocalH2DatasourceSpecification(); t::P.all()->project([x|$x.name], ['n'])"
                + "->from(t::M, " + instanceRuntime("^$s(testDataSetupSqls=t::setupText())") + "); }"));
        String literal = "{| let s = ^meta::pure::alloy::connections::alloy::specification::LocalH2DatasourceSpecification();"
                + " t::P.all()->project([x|$x.name], ['n'])->from(t::M, "
                + instanceRuntime("^$s(testDataSetupSqls='insert into T (ID) values (1)')") + "); }";
        // the product door refuses a query's inline setup by name: the override was read (it planned with setup=[])
        assertEquals("a from()'s inline test data in a query: seeds are a test's, and the product's query door plans"
                + " none", assertThrows(NotImplementedException.class,
                        () -> com.legend.Compiler.query(CTX, literal).executionPlan("t::R")).getMessage());
    }

    @Test
    @DisplayName("a typed runtime's list of setup texts is joined with a newline, as a helper body's is")
    void aTypedSetupListIsJoinedWithANewline() {
        // the owner's ruling 44 (the typer slice's stage 1c): the typed fold joined ['a', 'b'] with no separator, one
        // setup "ab", where the raw fold (a helper's body) joins it with a newline; legend-engine 4.145.0 plans one
        // setup per element (run, port-scratch/typer1b/research/cr4, e3) -- lite's one newline-joined setup is S1008's
        com.legend.TypedQuery rt = com.legend.Compiler.query(CTX, instanceRuntime("^meta::pure::alloy::connections::"
                + "alloy::specification::LocalH2DatasourceSpecification(testDataSetupSqls=['insert into T (ID) values"
                + " (1)', 'insert into T (ID) values (2)'])"));
        assertEquals(java.util.List.of("insert into T (ID) values (1)\ninsert into T (ID) values (2)"),
                com.legend.compiler.spec.typed.ExecutionContext.reader().read(java.util.Optional.empty(),
                        rt.expression()).sqlSetups());
    }
}
