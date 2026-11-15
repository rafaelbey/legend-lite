// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

package com.legend.normalizer;

import com.legend.compiler.ModelBuilder;
import com.legend.error.LegendCompileException;
import com.legend.error.ModelException;
import com.legend.model.ClassDefinition;
import com.legend.model.ClassMapping;
import com.legend.model.DatabaseDefinition;
import com.legend.model.JoinChainElement;
import com.legend.model.LegacyMappingDefinition;
import com.legend.model.PropertyMapping;
import com.legend.model.RelationalOperation;

import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;

/**
 * VALIDATION BEFORE SYNTHESIS (T4.1 step 5): the user-model checks Phase
 * E used to raise from inside a class's synthesis — a property mapping
 * naming a property its class does not declare, an M2M binding whose
 * {@code [source, target]} route is not benign — run over every set of
 * the pre-passed mapping before any synthesis, with §6's line held:
 * a STRICT build rejects them (the engine rejects them at compile time),
 * a MODULE build records them as per-set poisons and synthesis skips
 * the set (the binding is withheld; the reason raises at use). The
 * roadmap gaps ({@code NotImplementedException}) are not validation:
 * they stay per-set poisons in both builds.
 */
final class MappingValidation {

    private MappingValidation() {}

    /** The invalid sets of {@code md} BY SET ID, in declaration order, with
     * the engine's compile-time rejection of each — recorded, never thrown
     * here: the driver applies strict/module (B4). The key is the set's id,
     * never the object: later construction steps (the multi-hop injection)
     * rebuild the {@code ClassMapping} records, and an identity key lost the
     * verdict there (audit 2026-09-15 P0-1, proven by probe); declaration
     * order makes "a strict build rejects the first" a stable statement
     * (P0-2). Mapping-level errors (duplicate ids) still throw: they are the
     * mapping's, not a set's. */
    static Map<String, ModelException> run(ResolvedMapping r, ModelBuilder model) {
        LegacyMappingDefinition md = r.raw();
        // MAPPING-level errors the engine's compiler raises (R5): a class
        // mapping id taken by two distinct sets across the include closure,
        // (an include listed twice is rejected where the include graph is
        // first walked, StoreSubstitutionRewrite.resolveAllStores). Thrown in BOTH builds — a module build
        // walls the whole mapping through the pre-pass's own catch, exactly
        // as the engine rejects the mapping.
        MappingClosures.Closure closure = MappingClosures.of(model).closure(md.qualifiedName());
        List<String> dupIds = closure.duplicateIds();
        if (!dupIds.isEmpty()) {
            throw new ModelException(LegendCompileException.Phase.MODEL,
                    "Duplicated class mappings found with ID " + dupIds
                    + " in mapping '" + md.qualifiedName() + "'", md.qualifiedName());
        }
        // an enumeration mapping id taken twice across the closure, in the engine's words, checked next as the engine
        // checks it (MappingValidator.validateMappingElementIds, collectAndValidateEnumerationMappingIds: asked of
        // legend-engine 4.145.0, "Duplicated enumeration mappings found with ID 'em' in mapping 'w::M'"). Until
        // 2026-11-15 lite accepted the pair and a lookup by that id took the first (the normalizer batch, item 2,
        // census ASK 13, ruling 30)
        String dupEnumId = closure.duplicateEnumerationMappingId();
        if (dupEnumId != null) {
            throw new ModelException(LegendCompileException.Phase.MODEL,
                    "Duplicated enumeration mappings found with ID '" + dupEnumId
                    + "' in mapping '" + md.qualifiedName() + "'", md.qualifiedName());
        }
        Map<String, ModelException> invalid = new LinkedHashMap<>();
        for (ClassMapping cm : md.classMappings()) {
            try {
                switch (cm) {
                    case ClassMapping.Relational rcm -> {
                        validatePmNames(rcm, model, md);
                        validateJoinSources(rcm, model);
                    }
                    case ClassMapping.Pure pcm -> validateM2mRoutes(pcm, model, r);
                    case ClassMapping.Union u -> validateUnionMembers(u, r);
                    default -> { }
                }
            } catch (ModelException e) {
                invalid.put(ResolvedMapping.idOf(cm), e);
            }
        }
        return invalid;
    }

    /** An Operation union's every member set is a class mapping of the mapping or its includes, as legend-engine's
     * compiler requires (ClassMappingSecondPassBuilder: "Can't find class mapping"). */
    private static void validateUnionMembers(ClassMapping.Union u, ResolvedMapping r) {
        java.util.Set<String> ids = new java.util.HashSet<>(r.includedSets().keySet());
        for (ClassMapping cm : r.raw().classMappings()) {
            ids.add(ResolvedMapping.idOf(cm));
        }
        for (String member : u.memberSetIds()) {
            if (!ids.contains(member)) {
                throw new ModelException(LegendCompileException.Phase.MAPPING, "Can't find class mapping '" + member
                        + "' in mapping '" + r.raw().qualifiedName() + "'", r.raw().qualifiedName());
            }
        }
    }

    /** Every property mapping names a property its class declares or
     * inherits (engine parity: property lookup walks generalizations). */
    private static void validatePmNames(ClassMapping.Relational rcm,
            ModelBuilder model, LegacyMappingDefinition md) {
        ClassDefinition cd = MissProbe.knownMiss(model.knowledge().hierarchyClass(rcm.className()));
        if (cd == null) return;
        for (PropertyMapping pm : rcm.propertyMappings()) {
            if (pm instanceof PropertyMapping.LocalProperty) continue;
            if (model.knowledge().propertyType(cd, pm.propertyName()) == null) {
                throw new ModelException(LegendCompileException.Phase.MODEL,
                        "PropertyMapping '" + pm.propertyName() + "' references property "
                      + "not declared on class '" + rcm.className() + "'; mapping="
                      + md.qualifiedName());
            }
        }
    }

    /**
     * A join its database has not, in legend-engine's words: the engine refuses it when it compiles the view or the
     * mapping that names it ({@code HelperRelationalBuilder.getJoin}, :190, the database by its own name; asked of
     * legend-engine 4.145.0 of a view's join and of a mapping's -- a property's one hop and chain, an association's
     * line and chain, a ~filter's join, (INNER) or not: "Can't find join 'Nope' in database 'Db'"). Until 2026-11-15
     * lite's own, "Join 'J' not found in db 'D'; PM='p', ..." and "AssociationMapping join 'J' not found in db 'D';
     * ..." (a view's: the normalizer batch, item 5; a mapping's: the normalizer's second batch, item 4 (a); register
     * row S376).
     */
    static ModelException joinNotFound(String joinName, String db) {
        return new ModelException(LegendCompileException.Phase.NORMALIZE,
                "Can't find join '" + joinName + "' in database '" + com.legend.protocol.Protocol.splitFqn(db)[1] + "'");
    }

    /**
     * Every join chain a property mapping walks departs from the table it is at: the first join contains the set's
     * main table, each next join the table the previous one lands on -- legend-engine 4.145.0's
     * {@code HelperRelationalBuilder.validatePropertyMappings} (:1918-1937, run for every relational class mapping when
     * the mapping compiles: the join trees of its property mappings, their dyna-function arguments, embedded blocks and
     * otherwise fallbacks, :1880-1916) and {@code findTargetAliasFromJoinTreeNode} (:1940-1947), whose words these are,
     * the table by its own name (asked: a chain {@code @AC} from T, "Mapping error: the join AC does not contain the
     * source table T"). Until 2026-11-15 such a hop was refused only when the class was synthesized, deferred, as
     * "Join 'AC' references multiple non-source tables [A, C]; multi-table joins not supported" (the normalizer's
     * second batch, item 1, JCE:843; ruling 37 (b)). A join the database lacks, a set with no main table and a join
     * whose landing is ambiguous are left to the refusals that already name them.
     */
    private static void validateJoinSources(ClassMapping.Relational rcm, ModelBuilder model) {
        LegacyMappingDefinition.TableReference main = rcm.mainTable() != null ? rcm.mainTable()
                : MappingNormalizer.inferMainTableQuiet(rcm);
        if (main == null || rcm.sourceUrl() != null) {
            return;
        }
        String start = MappingNormalizer.canonicalTable(main.table());
        for (PropertyMapping pm : rcm.propertyMappings()) {
            validateJoinSources(pm, start, model);
        }
    }

    private static void validateJoinSources(PropertyMapping pm, String start, ModelBuilder model) {
        switch (pm) {
            case PropertyMapping.Join j -> validateChain(j.joins(), j.database(), start, model);
            case PropertyMapping.JoinTerminalColumn j -> validateChain(j.joins(), j.database(), start, model);
            case PropertyMapping.Expression e -> validateNavigations(e.expression(), start, model);
            case PropertyMapping.EnumeratedExpression e -> validateNavigations(e.expression(), start, model);
            case PropertyMapping.Embedded e -> e.propertyMappings().forEach(x -> validateJoinSources(x, start, model));
            case PropertyMapping.OtherwiseEmbedded oe -> {
                oe.embedded().forEach(x -> validateJoinSources(x, start, model));
                validateJoinSources(oe.fallback(), start, model);
            }
            case PropertyMapping.LocalProperty lp -> validateJoinSources(lp.body(), start, model);
            default -> { }
        }
    }

    private static void validateNavigations(RelationalOperation op, String start, ModelBuilder model) {
        op.descendants().forEach(o -> {
            if (o instanceof RelationalOperation.JoinNavigation nav && nav.databaseName() != null) {
                validateChain(nav.chain(), nav.databaseName(), start, model);
            }
        });
    }

    private static void validateChain(List<JoinChainElement> chain, String db, String start, ModelBuilder model) {
        String at = start;
        for (JoinChainElement hop : chain) {
            String hopDb = hop.databaseName() != null ? hop.databaseName() : db;
            java.util.Optional<DatabaseDefinition.JoinDefinition> jd = model.findJoin(hopDb, hop.joinName());
            if (jd.isEmpty()) {
                // the missing join's own refusal names it, when the set is synthesized
                return;
            }
            java.util.Set<String> tables = new java.util.LinkedHashSet<>();
            for (String t : jd.get().operation().tables()) {
                tables.add(MappingNormalizer.canonicalTable(t));
            }
            if (!tables.contains(at)) {
                throw new ModelException(LegendCompileException.Phase.MAPPING, "Mapping error: the join "
                        + hop.joinName() + " does not contain the source table " + bareTable(at));
            }
            tables.remove(at);
            if (tables.size() > 1) {
                return;
            }
            at = tables.isEmpty() ? at : tables.iterator().next();
        }
    }

    /** A table's own name, its schema dropped (the engine prints {@code Table._name()}). */
    private static String bareTable(String table) {
        return table.contains(".") ? table.substring(table.lastIndexOf('.') + 1) : table;
    }

    /** Every routed M2M binding names a benign route. */
    private static void validateM2mRoutes(ClassMapping.Pure pcm,
            ModelBuilder model, ResolvedMapping md) {
        ClassDefinition tgt = MissProbe.knownMiss(model.knowledge().hierarchyClass(pcm.className()));
        for (ClassMapping.Pure.PropertyBinding pb : pcm.propertyBindings()) {
            M2mRouteGuards.requireBenignRoute(pb, pcm, tgt, md, model);
        }
    }
}
