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
import static org.junit.jupiter.api.Assertions.assertTrue;

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
    @DisplayName("a call no overload takes is named as it was written: in full when written in full, else short")
    void aRefusedCallIsNamedAsItWasWritten() {
        // the owner's ruling of 2026-11-16 on register row S1007 (the typer slice's stage 1b): legend-engine 4.145.0
        // names the call as written (run, port-scratch/typer1b/engine); name resolution qualifies a bare call too, so
        // the spelling is read off the call's span (NoMatch.written)
        String full = typing(TypeInferenceException.class, "['a','b']->meta::pure::functions::math::minus()");
        assertTrue(full.startsWith("Can't find a match for function 'meta::pure::functions::math::minus(String[2])'.\n"
                + "Functions that can match if parameter types or multiplicities are changed:\n"
                + "\t\tminus(Integer[*]):Integer[1]\n"), full);
        String bare = typing(TypeInferenceException.class, "minus(['a','b'])");
        assertTrue(bare.startsWith("Can't find a match for function 'minus(String[2])'.\n"), bare);
        assertEquals("Can't find a match for function 'meta::pure::functions::lang::if(Boolean[1])'.\n"
                + "Functions that can match if number of parameters are changed:\n"
                + "\t\tif(Boolean[1],Function<{->T[m]}>[1],Function<{->T[m]}>[1]):T[m]\n"
                + "\t\tif(Pair<Function<{->Boolean[1]}>, Function<{->T[m]}>>[*],Function<{->T[m]}>[1]):T[m]\n",
                typing(TypeInferenceException.class, "meta::pure::functions::lang::if(true)"));
        String infix = typing(TypeInferenceException.class, "'a' - 'b'");
        assertTrue(infix.startsWith("Can't find a match for function 'minus("), infix);
    }

    @Test
    @DisplayName("a multiplicity in a refusal is Pure's [m], never Java's record")
    void aMultiplicityIsWrittenAsPureWritesIt() {
        // 2026-11-16 (the owner's ruling 42, ASK 4; the typer slice's stage 1b): NewChecker's and IfChecker's texts
        // printed Java's Multiplicity record (Bounded[lower=1, upper=1]); TypedAnswers' forms showed it (34 asks)
        assertEquals("property 'v' of 't::Box' declares multiplicity [1] but the value has [2]",
                typing(TypeInferenceException.class, "^t::Box(v=[1, 2])"));
        assertEquals("property 'v' of 't::Box' declares multiplicity [1] but the value has [2]",
                typing(TypeInferenceException.class, "{| let b = ^t::Box(v=1); ^$b(v=[1, 2]); }"));
        assertEquals("if condition must be Boolean[1], got multiplicity [2]",
                typing(TypeInferenceException.class, "if([true, false], |1, |2)"));
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
        // 2026-11-16 (the owner's ruling on register row S1007; the typer slice's stage 1b): the call is named as it
        // is written, here in full, as the engine names it -- it read 'tableReference(...)' until then
        assertEquals("Can't find a match for function 'meta::relational::functions::database::tableReference("
                + "Database[1],Integer[1])'.\n"
                + "Functions that can match if number of parameters are changed:\n"
                + "\t\ttableReference(Database[1],String[1],String[1]):Table[1]\n",
                typing(TypeInferenceException.class,
                        "meta::relational::functions::database::tableReference(t::Db, 1)"));
        assertEquals("Can't find a match for function 'meta::relational::functions::database::tableReference("
                + "Database[1],Integer[1],Integer[1])'.\n"
                + "Functions that can match if parameter types or multiplicities are changed:\n"
                + "\t\ttableReference(Database[1],String[1],String[1]):Table[1]\n",
                typing(TypeInferenceException.class,
                        "meta::relational::functions::database::tableReference(t::Db, 1, 2)"));
        assertEquals("tableReference's schema and table names are string literals written at the call;"
                + " argument 2 is $x",
                typing(TypeInferenceException.class,
                        "{x:String[1]| meta::relational::functions::database::tableReference(t::Db, $x)}"));
    }

    @Test
    @DisplayName("a return refusal names a parameterized type whole: Pair<a::A, a::B>, never B> cut inside its arguments")
    void aReturnRefusalNamesAParameterizedTypeWhole() {
        // the owner's ruling 44, ASK 15 (c) (the typer slice's stage 1c, 2026-11-17): SpecCompiler.simpleName cut a
        // type's name at its last '::', inside its type arguments, so the refusal read "declares return type B> but
        // body returns B>"; it cuts the raw head's package alone (a text of its own: MODEL's questions keep their keys)
        java.util.Map<String, String> walls = com.legend.Compiler.compileAllBodies(com.legend.Compiler.compileModel("""
                ###Pure
                Class a::A {}
                Class a::B {}
                function a::pairs(): meta::pure::functions::collection::Pair<a::A, a::B>[1]
                { ^meta::pure::functions::collection::Pair<a::B, a::A>(first=^a::B(), second=^a::A()) }
                """));
        String wall = walls.entrySet().stream().filter(e -> e.getKey().startsWith("a::pairs"))
                .map(java.util.Map.Entry::getValue).findFirst().orElseThrow(() -> new AssertionError(walls.toString()));
        assertTrue(wall.contains("declares return type Pair<a::A, a::B> but body returns Pair<a::B, a::A>"), wall);
    }

    @Test
    @DisplayName("a model function named as a platform form never displaces it: refused in the engine's words, as"
            + " legend-engine refuses it when the model compiles")
    void aModelFunctionNamedAsAFormIsRefused() {
        // the owner's ruling 44, ASK 15 (d), register row S1026 (the typer slice's stage 1c, 2026-11-17): a first
        // parameter named _this made t::sort take the call; legend-engine 4.145.0 refuses it, the platform's sorts
        // listed alone (run, port-scratch/typer1c/research/s1026/r7.out), and compiles t::sortz's (r8). The candidates
        // stay in lite's order (S1007)
        java.util.Map<String, String> walls = com.legend.Compiler.compileAllBodies(com.legend.Compiler.compileModel("""
                ###Pure
                import t::*;
                Class t::Box { v: Integer[1]; }
                function t::sort(_this:t::Box[1], k:String[1]):t::Box[1] { $_this }
                function t::q():Integer[1] { ^t::Box(v=1)->sort('x').v }
                function t::sortz(_this:t::Box[1], k:String[1]):t::Box[1] { $_this }
                function t::qz():Integer[1] { ^t::Box(v=1)->sortz('x').v }
                function t::full():Integer[1] { ^t::Box(v=1)->t::sort('x').v }
                """));
        String q = walls.entrySet().stream().filter(e -> e.getKey().startsWith("t::q_"))
                .map(java.util.Map.Entry::getValue).findFirst().orElseThrow(() -> new AssertionError(walls.toString()));
        assertTrue(q.contains("Can't find a match for function 'sort(Box[1],String[1])'.\n"
                + "Functions that can match if parameter types or multiplicities are changed:\n"), q);
        assertTrue(!q.contains("sort(Box[1],String[1]):Box[1]"), q);
        assertTrue(walls.keySet().stream().noneMatch(k -> k.startsWith("t::qz_") || k.startsWith("t::full_")),
                walls.toString());
    }
}
