// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

package com.legend.normalizer;

import com.legend.Compiler;
import org.junit.jupiter.api.DisplayName;
import org.junit.jupiter.api.Test;

import java.util.List;
import java.util.Map;

import static org.junit.jupiter.api.Assertions.assertEquals;

/**
 * THE NORMALIZER BATCH (2026-11-15; docs/rust-port/PHASE3.md, the normalizer slice's entry, "the normalizer batch";
 * docs/rust-port/PHASE3_SHIM_DESIGN.md §10 rulings 29, 30 and 34): step E's behaviours the owner ruled fixed, each
 * asked of legend-engine 4.145.0 first (the pinned release on the engine runner's class path:
 * port-scratch/normbatch/engine), each case a model text through both public doors -- the strict compile
 * ({@link Compiler#compileErrors}) and the tolerant module build ({@link Compiler#buildModule}: its walls and the
 * poison a fetch raises). The Rust twins are lite-normalizer's unit tests, and the normalizer differential holds every
 * oracle answer to the Java.
 */
class NormalizerBatchTest {

    private static List<String> strict(String text) {
        return Compiler.compileErrors(text);
    }

    private static Compiler.BuiltModule module(String text) {
        return Compiler.buildModule(Compiler.parseSources(List.of(new Compiler.ModelSource("m.pure", text))).model());
    }

    // ---- item 2 (census ASK 13): an XStore or ModelJoin mapping of an association the model has not ----

    private static final String XSTORE_UNKNOWN_ASSOCIATION = """
            Class w::P { name: String[1]; fid: Integer[1]; }
            Class w::F { id: Integer[1]; }
            ###Relational
            Database w::Db ( Table P (ID INTEGER PRIMARY KEY, NAME VARCHAR(20), FID INTEGER) Table F (ID INTEGER PRIMARY KEY) )
            ###Mapping
            Mapping w::M (
              w::P[p]: Relational { ~mainTable [w::Db]P name: [w::Db]P.NAME, fid: [w::Db]P.FID }
              w::F[f]: Relational { ~mainTable [w::Db]F id: [w::Db]F.ID }
              w::Nope: XStore { firm[p, f]: $this.fid == $that.id, people[f, p]: $this.id == $that.fid }
            )
            """;

    private static final String MODEL_JOIN_UNKNOWN_ASSOCIATION = """
            Class w::P { name: String[1]; fid: Integer[1]; }
            Class w::F { id: Integer[1]; }
            ###Relational
            Database w::Db ( Table P (ID INTEGER PRIMARY KEY, NAME VARCHAR(20), FID INTEGER) Table F (ID INTEGER PRIMARY KEY) )
            ###Mapping
            Mapping w::M (
              w::P[p]: Relational { ~mainTable [w::Db]P name: [w::Db]P.NAME, fid: [w::Db]P.FID }
              w::F[f]: Relational { ~mainTable [w::Db]F id: [w::Db]F.ID }
              w::Nope: ModelJoin { {f: w::F[1], p: w::P[1]|$f.id == $p.fid} }
            )
            """;

    @Test
    @DisplayName("item 2: an XStore or ModelJoin mapping of an unknown association is the engine's \"Can't find association\"")
    void anUnknownXStoreOrModelJoinAssociationIsTheEnginesRefusal() {
        // asked of the engine: "Can't find association 'w::Nope'"; until 2026-11-15 a NotImplementedException naming the
        // mapping's kind, "Association mapping kind Cross not supported; mapping=w::M"
        String refusal = "Can't find association 'w::Nope'";
        assertEquals(List.of("[6:1] " + refusal), strict(XSTORE_UNKNOWN_ASSOCIATION));
        Compiler.BuiltModule xstore = module(XSTORE_UNKNOWN_ASSOCIATION);
        assertEquals(Map.of(), xstore.walls());
        assertEquals(refusal, xstore.context().mappingAssociationPoison("w::M", "w::Nope").orElseThrow());
        assertEquals(List.of("[6:1] " + refusal), strict(MODEL_JOIN_UNKNOWN_ASSOCIATION));
        assertEquals(refusal, module(MODEL_JOIN_UNKNOWN_ASSOCIATION).context()
                .mappingAssociationPoison("w::M", "w::Nope").orElseThrow());
    }
}
