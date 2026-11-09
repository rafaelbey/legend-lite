package com.legend.integration;

import com.legend.Compiler;
import com.legend.compiler.element.ModelContext;
import com.legend.compiler.element.type.Type;
import org.junit.jupiter.api.DisplayName;
import org.junit.jupiter.api.Test;

import java.io.IOException;
import java.io.InputStream;
import java.nio.charset.StandardCharsets;
import java.util.List;

import static org.junit.jupiter.api.Assertions.*;

/**
 * Bazel smoke test corpus — see docs/BAZEL_IMPLEMENTATION_PLAN.md §1.2 and §4.7.
 *
 * <p>Two-project regression canary for the Bazel cross-project dependency work,
 * rebuilt against CORE after the engine module's deletion (the original lived in
 * {@code engine/src/test} and exercised PureModelBuilder). Loads both projects as
 * SEPARATE {@link Compiler.ModelSource}s and asserts every cross-project
 * dependency kind resolves end-to-end: class-typed and enum-typed properties,
 * superclasses, inherited properties, association navigation, function refs,
 * database/mapping includes, and runtime mapping/connection canonicalization.
 *
 * <p>Profile references on stereotypes/tagged values canonicalize in
 * {@code NameResolver.resolve(ParsedModel)} — compileModel's first step —
 * exactly like the engine-era canary asserted. The RAW parse layer keeps them
 * as written by design (resolution is a separate pass); consumers that read
 * the model door's records directly (DiagramService, NlqModel)
 * currently compensate with simple-name fallbacks instead of resolving first
 * — a layering smell worth fixing at those call sites, not a missing pass.
 */
class BazelSmokeTest {

    private static final String REFDATA_MODEL = "bazel_smoke/refdata/model.pure";
    private static final String REFDATA_IMPL = "bazel_smoke/refdata/impl.pure";
    private static final String TRADING_MODEL = "bazel_smoke/trading/model.pure";
    private static final String TRADING_IMPL = "bazel_smoke/trading/impl.pure";

    @Test
    @DisplayName("Smoke corpus compiles and cross-project refs resolve (core baseline)")
    void corpusCompilesWithCrossProjectRefs() {
        ModelContext ctx = Compiler.compileModel(List.of(
                new Compiler.ModelSource(REFDATA_MODEL, loadResource(REFDATA_MODEL)),
                new Compiler.ModelSource(REFDATA_IMPL, loadResource(REFDATA_IMPL)),
                new Compiler.ModelSource(TRADING_MODEL, loadResource(TRADING_MODEL)),
                new Compiler.ModelSource(TRADING_IMPL, loadResource(TRADING_IMPL))));

        // --- refdata project: all expected elements present ---
        assertTrue(ctx.findClass("refdata::Categorized").isPresent());
        assertTrue(ctx.findClass("refdata::Region").isPresent());
        assertTrue(ctx.findClass("refdata::Sector").isPresent());
        assertTrue(ctx.findEnum("refdata::Rating").isPresent());
        assertEquals(List.of("AAA", "AA", "A", "BBB", "BB", "B"),
                ctx.findEnum("refdata::Rating").orElseThrow().values());
        assertEquals(1, ctx.findFunction("refdata::formatSector").size(),
                "refdata::formatSector function must be registered");

        // --- trading project: all expected elements present ---
        assertTrue(ctx.findClass("trading::Trade").isPresent());
        assertTrue(ctx.findClass("trading::InternalTrade").isPresent());
        assertEquals(1, ctx.findFunction("trading::tradeSummary").size());
        assertEquals(1, ctx.findFunction("trading::sectorRegionCode").size());

        // --- cross-project: class-typed property resolves to refdata FQN ---
        var sectorProp = ctx.findProperty("trading::Trade", "sector").orElseThrow();
        assertEquals(new Type.ClassType("refdata::Sector"), sectorProp.type(),
                "trading::Trade.sector must resolve to refdata::Sector (cross-project)");

        // --- cross-project: enum-typed property (enum-vs-class kind preserved) ---
        var ratingProp = ctx.findProperty("trading::Trade", "rating").orElseThrow();
        assertEquals(new Type.EnumType("refdata::Rating"), ratingProp.type(),
                "trading::Trade.rating must resolve to refdata::Rating as an EnumType");

        // --- cross-project: superclass resolves across projects ---
        assertEquals(List.of("refdata::Categorized"),
                ctx.findClass("trading::InternalTrade").orElseThrow().superClassFqns());

        // --- cross-project: inherited property reachable through the chain ---
        // ModelContext.findProperty is the ONE member API; it walks
        // generalizations via findClass (the lazy-by-FQN discipline).
        assertTrue(ctx.findProperty("trading::InternalTrade", "category").isPresent(),
                "trading::InternalTrade.category (inherited from refdata::Categorized)"
                        + " must be reachable");

        // --- cross-project: association-injected navigation resolves ---
        assertEquals(new Type.ClassType("refdata::Region"),
                ctx.findProperty("refdata::Sector", "region").orElseThrow().type(),
                "refdata::SectorRegion's 'region' end must navigate from Sector");

        // --- cross-project: profile stereotype + tag canonicalize via the
        // resolver (NameResolver.resolve is compileModel's first step) ---
        var tradeDef = com.legend.compiler.NameResolver.resolve(
                com.legend.testing.Own.model(
                        loadResource(REFDATA_MODEL) + "\n" + loadResource(TRADING_MODEL)))
                .elements().stream()
                .filter(el -> el instanceof com.legend.model.ClassDefinition c
                        && c.qualifiedName().equals("trading::Trade"))
                .map(el -> (com.legend.model.ClassDefinition) el)
                .findFirst().orElseThrow();
        assertEquals(1, tradeDef.stereotypes().size());
        assertEquals("refdata::RefDataProfile", tradeDef.stereotypes().get(0).profileName(),
                "Cross-project stereotype profile must canonicalize to the FQN");
        assertEquals("rootEntity", tradeDef.stereotypes().get(0).stereotypeName());
        assertEquals(1, tradeDef.taggedValues().size());
        assertEquals("refdata::RefDataProfile", tradeDef.taggedValues().get(0).profileName(),
                "Cross-project taggedValue profile must canonicalize to the FQN");
        assertEquals("description", tradeDef.taggedValues().get(0).tagName());
        assertEquals("A financial trade", tradeDef.taggedValues().get(0).value());

        // --- cross-project: database include canonicalized to refdata FQN ---
        assertEquals(List.of("refdata::RefDB"),
                ctx.findDatabase("trading::TradingDB").orElseThrow().includes());

        // --- cross-project: mapping include registered ---
        assertTrue(ctx.findLegacyMapping("trading::TradingMapping").isPresent());

        // --- cross-project: runtime mappings + connection bindings canonicalized ---
        var runtime = ctx.findRuntime("trading::TradingRuntime").orElseThrow();
        assertEquals(List.of("trading::TradingMapping"), runtime.mappings());
        assertEquals(1, runtime.connectionBindings().size());
        var binding = runtime.connectionBindings().entrySet().iterator().next();
        assertEquals("refdata::RefDB", binding.getKey(),
                "Runtime connection binding key (store FQN) must canonicalize");
        assertEquals(List.of("refdata::RefConn"), binding.getValue(),
                "Runtime connection binding value (connection FQN) must canonicalize");

        // --- cross-project: connection storeName canonicalized ---
        assertEquals("refdata::RefDB",
                ctx.findConnection("refdata::RefConn").orElseThrow().storeName());
    }

    private static String loadResource(String path) {
        try (InputStream is = BazelSmokeTest.class.getClassLoader().getResourceAsStream(path)) {
            assertNotNull(is, "Resource not found on classpath: " + path);
            return new String(is.readAllBytes(), StandardCharsets.UTF_8);
        } catch (IOException e) {
            throw new RuntimeException("Failed to read resource " + path, e);
        }
    }
}
