// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

package com.legend.compiler.spec;

import com.legend.compiler.element.ModelContext;
import com.legend.error.ModelException;
import com.legend.error.NotImplementedException;
import com.legend.error.SchemaInvariantException;
import com.legend.error.TypeInferenceException;
import org.junit.jupiter.api.DisplayName;
import org.junit.jupiter.api.Test;

import java.util.Map;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertThrows;
import static org.junit.jupiter.api.Assertions.assertTrue;

/**
 * Ruling 29's policy 2 (the owner's ruling 42; the typer slice's stage 1a item 5): the raw Java exceptions a user's
 * input reached in G, G-half, the planner's execute chain and F are proper refusals, in the engine's words where it
 * refuses the same input at the same stage (run), in lite's otherwise (the G census, §10.2; the cases are
 * port-scratch/typer/research/item5's).
 */
class RawExceptionRefusalsTest {

    private static final String MODEL = """
            ###Pure
            Class t::P { name: String[1]; a: Integer[1]; }
            Class t::A { xs: String[*]; }
            function t::frameLet(): Any[*]
            {
              let fr = rows(-1, 0);
              #>{t::Db.T}#->extend(over(~A, ~B->ascending(), $fr), ~s:{p,w,r|$r.B}:y|$y->plus());
            }
            function t::frameParam(n: Integer[1]): Any[*]
            {
              #>{t::Db.T}#->extend(over(~A, ~B->ascending(), rows($n, 0)), ~s:{p,w,r|$r.B}:y|$y->plus())
            }
            function t::castDup(): Any[*]
            {
              [1]->cast(@meta::pure::metamodel::relation::Relation<(A:Integer[0..1], A:Integer[0..1])>)
            }
            ###Relational
            Database t::Db
            (
              Table T (ID INTEGER PRIMARY KEY, NAME VARCHAR(20), A INTEGER, B INTEGER, F FLOAT)
              Table T2 (K INTEGER PRIMARY KEY)
            )
            ###Mapping
            Mapping t::M
            (
              t::P: Relational { ~mainTable [t::Db]T name: [t::Db]T.NAME, a: [t::Db]T.A }
            )
            ###Connection
            RelationalDatabaseConnection t::Conn { store: t::Db; type: H2; specification: LocalH2 { }; auth: DefaultH2; }
            ###Runtime
            Runtime t::R { mappings: [t::M]; connections: [ t::Db: [ c: t::Conn ] ]; }
            """;

    private static final ModelContext CTX = com.legend.Compiler.compileModel(MODEL);

    private static final Map<String, String> WALLS = com.legend.Compiler.compileAllBodies(CTX);

    private static <T extends RuntimeException> String typing(Class<T> kind, String query) {
        return assertThrows(kind, () -> com.legend.Compiler.query(CTX, query).resultType()).getMessage();
    }

    private static <T extends RuntimeException> String planning(Class<T> kind, String query) {
        com.legend.TypedQuery q = com.legend.Compiler.query(CTX, query);
        q.resultType();
        return assertThrows(kind, () -> q.executionPlan("t::R")).getMessage();
    }

    private static String wall(String fqn) {
        return WALLS.entrySet().stream().filter(e -> e.getKey().startsWith(fqn + "_")).map(Map.Entry::getValue)
                .findFirst().orElseThrow(() -> new AssertionError(fqn + " is not walled: " + WALLS));
    }

    @Test
    @DisplayName("a window frame or bound lite cannot read statically: refused, NotImplemented, in lite's words")
    void aNonLiteralFrameIsRefused() {
        assertEquals("window frame expects rows()/range(), got TypedVariable", wall("t::frameLet"));
        assertEquals("window frame bound must be a numeric literal or unbounded(), got TypedVariable",
                wall("t::frameParam"));
        assertEquals("window frame expects rows()/_range(), got meta::pure::functions::multiplicity::toOne",
                typing(NotImplementedException.class, "#>{t::Db.T}#->extend(over(~A, ~B->ascending(),"
                        + " rows(-1, 0)->toOne()), ~s:{p,w,r|$r.B}:y|$y->plus())->from(t::R)"));
    }

    @Test
    @DisplayName("a relation type naming a column twice: the engine's words")
    void aRelationTypeOfADuplicateColumnIsRefused() {
        assertEquals("in function 't::castDup': The relation contains duplicates: [A]", wall("t::castDup"));
        assertEquals("The relation contains duplicates: [A]", typing(SchemaInvariantException.class,
                "[1]->cast(@meta::pure::metamodel::relation::Relation<(A:Integer[0..1], A:Integer[0..1])>)"));
    }

    @Test
    @DisplayName("a table declaring a column twice, a DECIMAL whose scale passes its precision: refused, naming them")
    void aTableOfADuplicateColumnIsRefused() {
        ModelContext dup = com.legend.Compiler.compileModel("###Relational\nDatabase t::Db ("
                + " Table T (ID INTEGER PRIMARY KEY, A INTEGER, A VARCHAR(3))"
                + " Table U (ID INTEGER PRIMARY KEY, D DECIMAL(1, 2)) )\n");
        assertEquals("table 'T' declares the column 'A' twice", assertThrows(ModelException.class,
                () -> com.legend.Compiler.query(dup, "#>{t::Db.T}#->select(~[ID])").resultType()).getMessage());
        assertEquals("table 'U': the column 'D' declares a precision and scale its type cannot hold (the scale must"
                + " be in [0, precision])", assertThrows(ModelException.class,
                () -> com.legend.Compiler.query(dup, "#>{t::Db.U}#->select(~[ID])").resultType()).getMessage());
    }

    @Test
    @DisplayName("if() and if(c): the engine's words")
    void anIfOfTooFewArgumentsIsRefused() {
        String none = typing(TypeInferenceException.class, "[1,2]->map(x|if())");
        assertTrue(none.startsWith("Can't find a match for function 'if()'.\nFunctions that can match if number of"
                + " parameters are changed:\n\t\tif(Boolean[1],Function<{->T[m]}>[1],Function<{->T[m]}>[1]):T[m]"),
                none);
        assertTrue(typing(TypeInferenceException.class, "if(true)")
                .startsWith("Can't find a match for function 'if(Boolean[1])'."));
    }

    @Test
    @DisplayName("groupByWithWindowSubset's ids past their lists, a TDS agg written with a path: refused by name")
    void aTdsGroupByOutOfItsListsIsRefused() {
        assertEquals("groupByWithWindowSubset: the subSelectId 'total' is ids[1], an aggregate's id, not one of the 1"
                + " function(s)' ids", typing(TypeInferenceException.class, "t::P.all()->groupByWithWindowSubset("
                + "[x|$x.name], [agg(x|$x.a, y|$y->sum())], ['name', 'total'], ['total'], [])->from(t::M, t::R)"));
        assertEquals("groupByWithWindowSubset: the subAggId 'x' is ids[2], not one of the 1 aggregate value(s)' ids",
                typing(TypeInferenceException.class, "t::P.all()->groupByWithWindowSubset([x|$x.name],"
                        + " [agg(x|$x.a, y|$y->sum())], ['name', 'total', 'x'], ['name'], ['x'])->from(t::M, t::R)"));
        assertEquals("agg('cnt', ...): a TDS groupBy's agg takes a map lambda and an aggregate lambda, agg('cnt',"
                + " x|..., y|...)", typing(TypeInferenceException.class, "t::P.all()->project([x|$x.name, x|$x.a],"
                + " ['n', 'a'])->groupBy('n', agg('cnt', #/t::P/name#, y|$y->count()))->from(t::M, t::R)"));
    }

    @Test
    @DisplayName("a group lambda's sort list: emptyFirst([]) and an unknown NullOrder, refused by name")
    void aGroupLambdasBadSortKeyIsRefused() {
        assertEquals("emptyFirst/emptyLast in a sort list names no sort key; it takes one, asc(~col) or desc(~col)",
                typing(TypeInferenceException.class, "#>{t::Db.T}#->groupBy(~A, ~names:g|$g->joinStrings(~NAME, ',',"
                        + " emptyFirst([])))->from(t::R)"));
        assertEquals("enumeration meta::pure::functions::relation::NullOrder has no value 'MIDDLE'",
                typing(TypeInferenceException.class, "#>{t::Db.T}#->groupBy(~A, ~names:g|$g->joinStrings(~NAME,"
                        + " ',', descending(~ID, meta::pure::functions::relation::NullOrder.MIDDLE)))->from(t::R)"));
    }

    @Test
    @DisplayName("TDS surfaces: a name-less col() beside a window column, a cell past the row, a row past the header")
    void tdsSurfacesPastTheirShapesAreRefused() {
        assertEquals("a name-less project column must be a property navigation (its leaf names the column); give"
                + " explicit names for computed columns", typing(TypeInferenceException.class,
                "t::P.all()->project([col(window([x|$x.name]), func(x|$x.a, y|$y->sum()), 'w'), col()])"
                        + "->from(t::M, t::R)"));
        assertEquals("The system is trying to get an element at offset 9 where the collection is of size 3",
                typing(TypeInferenceException.class, "#TDS\na,b,c\n1,2,3\n#.rows->at(0).values->at(9)"));
        assertEquals("Row 2 has too many columns (expected 1)",
                typing(TypeInferenceException.class, "#TDS\na\n1,2\n#"));
    }

    @Test
    @DisplayName("an executed query's envelope: values->at(1) over one TDS, documentation reaching no project")
    void theSplicesRefusalsAreTypeErrors() {
        assertEquals("Result.values->at(k>0) on a relation-rooted query — the values envelope holds one TDS",
                planning(TypeInferenceException.class, "meta::pure::router::execute(|t::P.all()->project([x|$x.name],"
                        + " ['n']), t::M, t::R.runtimeValue, []).values->at(1)"));
        assertEquals("columns.documentation read did not reach a project after the splice (source="
                + "TypedTableReference)", planning(TypeInferenceException.class, "#>{t::Db.T}#.columns.documentation"));
    }

    @Test
    @DisplayName("executeLegendQuery: a var that is not a date for a Date parameter; an empty temporal concatenation")
    void theExecuteChainsRefusalsNameTheInput() {
        assertEquals("executeLegendQuery vars: 'not-a-date' is not a date, for a parameter of type Date (expected"
                + " digits for year at position 0 in 'not-a-date')", planning(TypeInferenceException.class,
                "meta::legend::executeLegendQuery({d: Date[1]|t::P.all()->filter(p|$p.a > 0)->project([x|$x.name],"
                        + " ['n'])->from(t::M, t::R)}, [pair('d', 'not-a-date')],"
                        + " ^meta::pure::runtime::ExecutionContext(), [])"));
        assertEquals("concatenateTemporalTdsQueries over an empty collection: there is no query to concatenate",
                planning(NotImplementedException.class, "meta::pure::router::execute("
                        + "meta::relational::milestoning::concatenateTemporalTdsQueries([]), t::M, t::R.runtimeValue,"
                        + " []).values"));
    }

    @Test
    @DisplayName("a spelled collection's field every element leaves empty is not folded (it was an IndexOutOfBounds)")
    void aFieldOfEmptyElementsIsNotFolded() {
        com.legend.TypedQuery q = com.legend.Compiler.query(CTX, "[^t::A(xs=[]), ^t::A(xs=[])].xs");
        assertEquals("String[*]", q.resultType().type().typeName() + q.resultType().multiplicity().text());
        RuntimeException e = assertThrows(RuntimeException.class, () -> q.executionPlan("t::R"));
        assertTrue(!(e instanceof IndexOutOfBoundsException), String.valueOf(e));
    }
}
