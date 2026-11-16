// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

package com.legend.compiler.spec;

import com.legend.compiler.element.ModelContext;
import com.legend.error.NotImplementedException;
import com.legend.error.TypeInferenceException;
import org.junit.jupiter.api.DisplayName;
import org.junit.jupiter.api.Test;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertThrows;

/**
 * The owner's ruling 42, the G census's ASK 4 (the typer slice's stage 1a item 3): the refusals that printed a Java
 * record's {@code toString} -- the kernel's no-structural-match text (ExprType and TypedParameter records), the context
 * reader's three post-processor refusals (a typed subtree, the whole callee with its body), NewChecker's property
 * conformance (a protocol value cut at 400 characters), TableReferenceChecker's malformed call (the arguments' protocol
 * records) -- now say what the user wrote in Pure terms: the engine's words where the engine refuses the same input
 * when it compiles the lambda (run, port-scratch/typer/engine), lite's otherwise.
 */
class AnswerTextsTest {

    private static final String MODEL = """
            ###Pure
            Class t::P { name: String[1]; a: Integer[1]; n: String[0..1]; }
            Class t::Box { v: Integer[1]; }

            ###Relational
            Database t::Db
            (
              Table T (ID INTEGER PRIMARY KEY, NAME VARCHAR(20), A INTEGER)
            )

            ###Mapping
            Mapping t::M
            (
              t::P: Relational { ~mainTable [t::Db]T name: [t::Db]T.NAME, a: [t::Db]T.A }
            )

            ###Connection
            RelationalDatabaseConnection t::Conn
            { store: t::Db; type: H2; specification: LocalH2 { }; auth: DefaultH2; }

            ###Runtime
            Runtime t::R { mappings: [t::M]; connections: [ t::Db: [ c: t::Conn ] ]; }

            ###Pure
            Class t::Auth extends meta::pure::alloy::connections::alloy::authentication::AuthenticationStrategy {}
            function t::myHook(q: meta::relational::metamodel::relation::SelectSQLQuery[1]):
                meta::pure::mapping::Result<meta::relational::metamodel::relation::SelectSQLQuery|1>[1]
            { meta::relational::postProcessor::replaceTables($q, []) }
            function t::pairs(): meta::pure::functions::collection::Pair<meta::relational::metamodel::relation::Table,
                meta::relational::metamodel::relation::Table>[*] { [] }
            function t::tbl(): meta::relational::metamodel::relation::Table[1]
            {
              t::Db->meta::relational::metamodel::schema('default')->toOne()
                ->meta::relational::metamodel::table('T')->toOne()
            }
            """;

    private static final ModelContext CTX = com.legend.Compiler.compileModel(MODEL);

    private static String typing(Class<? extends RuntimeException> kind, String query) {
        return assertThrows(kind, () -> com.legend.Compiler.query(CTX, query).resultType()).getMessage();
    }

    private static String hooked(String hook) {
        return "t::P.all()->project([x|$x.name], ['n'])->from(t::M, ^meta::core::runtime::Runtime(connectionStores="
                + "^meta::core::runtime::ConnectionStore(element=t::Db, connection="
                + "^meta::external::store::relational::runtime::RelationalDatabaseConnection("
                + "type=meta::relational::runtime::DatabaseType.H2,"
                + " datasourceSpecification=^meta::pure::alloy::connections::alloy::specification"
                + "::LocalH2DatasourceSpecification(), authenticationStrategy=^t::Auth(),"
                + " sqlQueryPostProcessors=[{query:meta::relational::metamodel::relation::SelectSQLQuery[1] | " + hook
                + "}]))))";
    }

    @Test
    @DisplayName("no overload matches the arguments: the engine's words, the types as written in Pure")
    void noStructuralMatchSaysTheEnginesWords() {
        assertEquals("Can't find a match for function 'minus(String[2])'.\n"
                + "Functions that can match if parameter types or multiplicities are changed:\n"
                + "\t\tminus(Integer[*]):Integer[1]\n\t\tminus(Decimal[*]):Decimal[1]\n\t\tminus(Float[*]):Float[1]\n"
                + "\t\tminus(Number[*]):Number[1]\n",
                typing(TypeInferenceException.class, "['a','b']->minus()"));
    }

    @Test
    @DisplayName("a post-processor hook, a replaceTables pair and a pair side are named by what they are, not dumped")
    void theContextReaderNamesWhatItCannotRead() {
        assertEquals("sqlQueryPostProcessorsConnectionAware hook shape is not a replaceTables lambda — post-processor"
                + " recognizer pending for: a lambda whose last statement is a call to 't::myHook'",
                typing(NotImplementedException.class, hooked("t::myHook($query)")));
        assertEquals("replaceTables pair argument is not a literal pair(): a call to 't::pairs'",
                typing(NotImplementedException.class,
                        hooked("meta::relational::postProcessor::replaceTables($query, t::pairs())")));
        assertEquals("replaceTables pair side is not a schema()/table() navigation: a call to 't::tbl'",
                typing(NotImplementedException.class, hooked("meta::relational::postProcessor::replaceTables($query,"
                        + " meta::pure::functions::collection::pair(t::tbl(), t::tbl()))")));
    }

    @Test
    @DisplayName("a property value that does not conform: the kernel's words, with no protocol record")
    void aNonConformingPropertyValueIsNotDumped() {
        assertEquals("property 'v' of 't::Box': expected Integer, got String",
                typing(TypeInferenceException.class, "^t::Box(v='a')"));
    }

    @Test
    @DisplayName("a malformed tableReference: the engine's words for arguments of the wrong count or type, lite's for"
            + " a name not written as a literal")
    void aMalformedTableReferenceSaysWhatIsWrong() {
        assertEquals("Can't find a match for function 'tableReference(Database[1],Integer[1])'.\n"
                + "Functions that can match if number of parameters are changed:\n"
                + "\t\ttableReference(Database[1],String[1],String[1]):Table[1]\n",
                typing(TypeInferenceException.class,
                        "meta::relational::functions::database::tableReference(t::Db, 1)"));
        assertEquals("Can't find a match for function 'tableReference(Database[1],Integer[1],Integer[1])'.\n"
                + "Functions that can match if parameter types or multiplicities are changed:\n"
                + "\t\ttableReference(Database[1],String[1],String[1]):Table[1]\n",
                typing(TypeInferenceException.class,
                        "meta::relational::functions::database::tableReference(t::Db, 1, 2)"));
        assertEquals("tableReference's schema and table names are string literals written at the call;"
                + " argument 2 is $x",
                typing(TypeInferenceException.class,
                        "{x:String[1]| meta::relational::functions::database::tableReference(t::Db, $x)}"));
    }
}
