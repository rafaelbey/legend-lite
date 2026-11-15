// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

package com.legend.normalizer;

import com.legend.compiler.ModelBuilder;
import com.legend.error.LegendCompileException;
import com.legend.error.ModelException;
import com.legend.model.ClassDefinition;
import com.legend.model.ClassMapping;
import com.legend.model.LegacyMappingDefinition;
import com.legend.protocol.TypeExpression;
import java.util.ArrayList;
import java.util.List;

/**
 * Audit-21a route guards for M2M (Pure) property bindings: the parsed
 * mappingLine heads are honored or poisoned by DESIGN — never dropped.
 */
final class M2mRouteGuards {

    /** The M2M binding's STORAGE key: the declared property name, or —
     * for the SYNTHETIC milestoned sweep spelling ({@code
     * <base>AllVersions} on a temporal-class end, which real pure
     * GENERATES) — the BASE property (queries and fetch trees address
     * the declared name; the VALUE keeps its all-versions read). An
     * undeclared key throws (normalize-phase contract).  */
    static String m2mBindingKey(ClassMapping.Pure.PropertyBinding pb,
            @com.legend.base.Nullable ClassDefinition tgt, ModelBuilder model,
            java.util.function.Predicate<String> declared) {
        if (tgt == null || declared.test(pb.propertyName())) {
            return pb.propertyName();
        }
        // `pAllVersions` is the edge-point property legend-engine generates for a property `p` whose type is a class
        // carrying a temporal stereotype of its own (Milestoning.java:186-198, :215-223; HelperModelBuilder.java
        // :466-470 reads the raw type's own stereotypes): only for such a p does the binding read as p's (asked: a
        // temporal p compiles; a p of a non-temporal class, of a primitive, or of a subclass of a temporal class is
        // refused). Until 2026-11-15 the suffix was stripped whatever p's type (the normalizer's second batch, item 3,
        // register row S389)
        if (pb.propertyName().endsWith("AllVersions")) {
            String base = pb.propertyName().substring(0,
                    pb.propertyName().length() - "AllVersions".length());
            if (declared.test(base) && ownTemporalType(model.knowledge().propertyType(tgt, base), model)) {
                return base;
            }
        }
        // a binding of a property the class has not, in the engine's words: "Can't find property 'p' in [T, ..., Any]"
        // -- the class and its generalizations by simple name, nearest first (HelperModelBuilder.getAppliedProperty
        // :487-515; the engine's resolution order, single inheritance's order). Until 2026-11-15 lite's own, "M2M
        // PropertyBinding 'p' is not declared on class 'T'" (S389)
        StringBuilder order = new StringBuilder();
        for (ClassDefinition cd : model.knowledge().lineage(tgt.qualifiedName())) {
            order.append(simpleName(cd.qualifiedName())).append(", ");
        }
        throw new ModelException(LegendCompileException.Phase.NORMALIZE,
                "Can't find property '" + pb.propertyName() + "' in [" + order + "Any]");
    }

    /** Whether {@code type} names a class carrying a temporal (milestoning) stereotype of its own. */
    private static boolean ownTemporalType(@com.legend.base.Nullable TypeExpression type, ModelBuilder model) {
        if (!(type instanceof TypeExpression.NameRef nr)) {
            return false;
        }
        java.util.Optional<ClassDefinition> cd = model.knowledge().hierarchyClass(nr.name());
        return cd.isPresent() && cd.get().stereotypes().stream().anyMatch(st ->
                com.legend.compiler.element.MilestoningStrategy.ofStereotypeOrNull(st.profileName(),
                        st.stereotypeName()) != null);
    }

    private static String simpleName(String fqn) {
        int i = fqn.lastIndexOf("::");
        return i < 0 ? fqn : fqn.substring(i + 2);
    }

    private M2mRouteGuards() {
    }

    /**
     * Audit 21a HIGH: a Pure binding's {@code [targetSetId]} route selects a
     * SPECIFIC set of the routed class; emitting an unrouted
     * {@code NewInstanceCast} would root-route it — the audit-11 wrong-rows
     * shape (relational side: Join.targetSetId, classified per-PM) mirrored
     * on the M2M side. The route is benign only when root-routing is
     * IDENTICAL to honoring it: the routed set is the routed class's sole
     * set, or its root set. Anything else — including a set id that matches
     * nothing, or a route on a binding whose type is not a mapped class —
     * is a loud wall, never a silent drop. The {@code [src, tgt]} source id
     * must name this very set; a mismatch is a model shape we don't route.
     */
    static void requireBenignRoute(ClassMapping.Pure.PropertyBinding pb,
            ClassMapping.Pure pcm, @com.legend.base.Nullable ClassDefinition tgt,
            ResolvedMapping md, ModelBuilder model) {
        if (pb.sourceSetId() == null && pb.targetSetId() == null) {
            return;
        }
        if (pb.sourceSetId() != null && !setIdMatches(pcm, pb.sourceSetId())) {
            throw new ModelException(LegendCompileException.Phase.NORMALIZE,
                    "M2M binding '" + pb.propertyName() + "[" + pb.sourceSetId()
                  + ", " + pb.targetSetId() + "]' names source set '"
                  + pb.sourceSetId() + "' but is declared inside set '"
                  + pcm.setId() + "'; mapping=" + md.qualifiedName());
        }
        String routedClass = null;
        if (tgt != null
                && model.knowledge().propertyType(tgt, pb.propertyName())
                        instanceof TypeExpression.NameRef nr
                && model.knowledge().hierarchyClass(nr.name()).isPresent()) {
            routedClass = nr.name();
        }
        if (routedClass != null) {
            List<LegacyMappingDefinition> closure = md.closure();
            List<ClassMapping> sets = new ArrayList<>();
            for (LegacyMappingDefinition m : closure) {
                for (ClassMapping cm : m.classMappings()) {
                    if (cm.className().equals(routedClass)) {
                        sets.add(cm);
                    }
                }
            }
            final String tgtSet = pb.targetSetId();
            if (sets.isEmpty()) {
                // the routed class has no set: the line's default id is the engine's pass-through, decided with the
                // transform's type by m2mPropertyValue (S395); any other id is the engine's refusal
                // (ClassMappingThirdPassBuilder.java:100-107; since 2026-11-15, the normalizer's second batch, item 2)
                if (tgtSet == null || com.legend.model.SetId.isDefault(tgtSet, routedClass)) {
                    return;
                }
                // an id no set of the closure answers to; one that names another class's set (a subclass's: the
                // engine looks a route's id up by id alone) keeps lite's refusal below
                if (md.set(tgtSet) == null) {
                    throw new ModelException(LegendCompileException.Phase.MAPPING,
                            "Can't find class mapping '" + tgtSet + "'");
                }
            }
            ClassMapping routed = sets.stream()
                    .filter(cm -> setIdMatches(cm, tgtSet))
                    .findFirst().orElseGet(MissProbe::miss);
            if (routed != null && (sets.size() == 1 || routed.root()
                    || pb.expression() instanceof com.legend.protocol.spec.Variable)) {
                // honored routes: the sole set, the root, or the
                // WHOLE-SOURCE identity binding ($src) — its cast carries
                // targetSetId (route A edit 3) and the graph consumer
                // dispatches by it (wholeSrcChild), so the named set is
                // honored, never root-collapsed
                return;
            }
        }
        throw new ModelException(LegendCompileException.Phase.NORMALIZE,
                "M2M set-routed binding '" + pb.propertyName() + "["
              + pb.targetSetId() + "]' targets a non-root set"
              + (routedClass == null ? "" : " of '" + routedClass + "'")
              + "; per-set routing on the M2M side is a roadmap feature"
              + " (root-routing would produce wrong rows); mapping="
              + md.qualifiedName());
    }

    /**
     * Whether {@code cm} answers to set id {@code id}. An unlabeled mapping's
     * engine-default id is the class FQN with '_' for '::' (short name
     * accepted too — includes-era corpora spell it either way).
     */
    private static boolean setIdMatches(ClassMapping cm, @com.legend.base.Nullable String id) {
        if (id == null) {
            return false;
        }
        if (cm.setId() != null) {
            return cm.setId().equals(id);
        }
        String fqn = cm.className();
        // audit 23: EXACT engine-default id only (FQN with '_' for '::');
        // the short-class-name acceptance let two same-named classes in
        // different packages match each other's routes
        return com.legend.model.SetId.isDefault(id, fqn);
    }

    /** Mapping-LOCAL property field (+prodId: String[1]: expr — engine
     * local-property semantics; the XStore assoc-key idiom): typed by its
     * own value via the isLocal KeyExpression, composed as an extra
     * binding column. A local COLLIDING with a declared property stays
     * the designed poison (audit 21a: the name-keyed binding table
     * cannot hold both, and silent retargeting was the original bug). */
    static com.legend.protocol.spec.KeyExpression localField(
            com.legend.model.ClassMapping.Pure.PropertyBinding pb,
            com.legend.model.@com.legend.base.Nullable ClassDefinition tgt,
            ResolvedMapping md,
            com.legend.compiler.ModelBuilder model, boolean collides) {
        if (tgt != null && collides) {
            throw new com.legend.error.ModelException(
                    com.legend.error.LegendCompileException.Phase.NORMALIZE,
                    "M2M local mapping property '+" + pb.propertyName()
                  + "' collides with a declared property of '"
                  + tgt.qualifiedName() + "' — locals are distinct (engine)"
                  + " and the shared binding table cannot hold both;"
                  + " mapping=" + md.qualifiedName());
        }
        return new com.legend.protocol.spec.KeyExpression(
                pb.expression(), false, true);
    }
}
