// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

package com.legend.compiler.spec;

import com.legend.compiler.element.PureModelContext;
import com.legend.compiler.element.TypedFunction;
import com.legend.compiler.element.type.Type;
import com.legend.error.TypeInferenceException;
import com.legend.model.FunctionDefinition;
import com.legend.model.SynthHat;
import org.junit.jupiter.api.DisplayName;
import org.junit.jupiter.api.Test;

import java.util.List;
import java.util.Map;

import static org.junit.jupiter.api.Assertions.assertDoesNotThrow;
import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertThrows;

/**
 * The owner's ruling 36 (2026-11-14; LITE-ONLY; register row S376): SpecCompiler's return conformance also accepts a
 * lifted view's column whose declared and body types are of one family, numbers (Integer, Float, Decimal, Number) or
 * dates (Date, StrictDate, DateTime), for the lift's function only. NUM and DAT are the names batch's shapes c59 and
 * c08, which the pinned engine compiles. No view text yields a column of two families (a translation's bug), so those
 * cases are TWO's lifted function declared with another return.
 */
class LiftedViewFamilyTest {

    private static final String MODEL = """
            ###Pure
            function t::user():Relation<(ID:Integer[1], I:Float[0..1])>[1]
            {
              #>{t::Db.T}#->select(~[ID, I])
            }
            ###Relational
            Database t::Db
            (
              Table T (ID INTEGER PRIMARY KEY, I INTEGER, F FLOAT, DT DATE, TS TIMESTAMP)
              View NUM (ID: T.ID PRIMARY KEY, X: plus(T.I, T.F))
              View DAT (ID: T.ID PRIMARY KEY, X: datePart(T.TS))
              View TWO (ID: T.ID PRIMARY KEY, D: T.DT, N: T.I)
            )
            """;

    private static final PureModelContext CTX =
            (PureModelContext) com.legend.Compiler.buildModel(com.legend.testing.Own.model(MODEL));

    private static TypedFunction fn(String fqn) {
        List<TypedFunction> fns = CTX.findFunction(fqn);
        assertEquals(1, fns.size(), fqn);
        return fns.get(0);
    }

    /** {@code f} declared to return its own relation with the columns in {@code retyped} at other types, or with
     *  another definition (its provenance). */
    private static TypedFunction declaring(TypedFunction f, Map<String, Type> retyped, com.legend.model.Function def) {
        Type.RelationType r = Type.relationSchema(f.returnType());
        List<Type.Column> cols = r.columns().stream().map(c -> retyped.containsKey(c.name())
                ? new Type.Column(c.name(), retyped.get(c.name()), c.multiplicity()) : c).toList();
        Type ret = new Type.GenericType(((Type.GenericType) f.returnType()).rawFqn(),
                List.of(new Type.RelationType(cols)));
        return new TypedFunction(f.qualifiedName(), f.typeParameters(), f.multiplicityParameters(), f.parameters(), ret,
                f.returnMultiplicity(), f.body(), f.isNative(), def, f.id());
    }

    private static String refusal(TypedFunction f) {
        return assertThrows(TypeInferenceException.class, () -> new SpecCompiler(CTX).compile(f)).getMessage();
    }

    @Test
    @DisplayName("numbers are one family: plus(T.I, T.F) declared Integer (the engine's), its body Float")
    void aNumberColumnTakesAnotherNumber() {
        assertDoesNotThrow(() -> new SpecCompiler(CTX).compile(fn("t::Db$view$NUM")));
    }

    @Test
    @DisplayName("dates are one family: datePart(T.TS) declared StrictDate (the engine's DATE), its body Date")
    void aDateColumnTakesAnotherDate() {
        assertDoesNotThrow(() -> new SpecCompiler(CTX).compile(fn("t::Db$view$DAT")));
    }

    @Test
    @DisplayName("a column of two families is still refused, named: a number declared over a date body")
    void aColumnOfTwoFamiliesIsRefused() {
        TypedFunction two = fn("t::Db$view$TWO");
        assertEquals("in function 't::Db$view$TWO': declares return type Relation<(ID:Integer[1], D:Integer[0..1],"
                + " N:Integer[0..1])> but body returns Relation<(ID:Integer[1], D:StrictDate[0..1], N:Integer[0..1])>"
                + " (column 'D': expected Integer, got StrictDate)",
                refusal(declaring(two, Map.of("D", Type.Primitive.INTEGER), two.definition())));
    }

    @Test
    @DisplayName("the refusal names the first column of two families (N: a date declared over a number body), past an"
            + " in-family one before it (D: DateTime over StrictDate), which today's message named")
    void theRefusalNamesTheColumnOfTwoFamilies() {
        TypedFunction two = fn("t::Db$view$TWO");
        assertEquals("in function 't::Db$view$TWO': declares return type Relation<(ID:Integer[1], D:DateTime[0..1],"
                + " N:StrictDate[0..1])> but body returns Relation<(ID:Integer[1], D:StrictDate[0..1],"
                + " N:Integer[0..1])> (column 'N': expected StrictDate, got Integer)", refusal(declaring(two,
                Map.of("D", Type.Primitive.DATE_TIME, "N", Type.Primitive.STRICT_DATE), two.definition())));
    }

    @Test
    @DisplayName("a user's function with an in-family column is refused as before; the same function as a lift is not")
    void aUsersFunctionIsRefusedAsBefore() {
        TypedFunction user = fn("t::user");
        assertEquals("in function 't::user': declares return type Relation<(ID:Integer[1], I:Float[0..1])> but body"
                + " returns Relation<(ID:Integer[1], I:Integer[0..1])> (column 'I': expected Float, got Integer)",
                refusal(user));
        FunctionDefinition lift = ((FunctionDefinition) user.definition())
                .withSynthesizedFrom(new FunctionDefinition.Synthesized(SynthHat.VIEW, "t::Db", "U"));
        assertDoesNotThrow(() -> new SpecCompiler(CTX).compile(declaring(user, Map.of(), lift)));
    }
}
