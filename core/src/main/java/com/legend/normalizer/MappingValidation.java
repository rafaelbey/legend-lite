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

    // ==== legend-engine's compile of an association mapping's lambdas (the normalizer's second batch, item 4 (e)) ====

    /** A check that cannot be made exact here: lite's own refusal (or deferral) stands. */
    private static final String HOLD = "\u0000hold";

    /**
     * legend-engine's checks of a ModelJoin association mapping's lambda when the mapping compiles, in its words and
     * order (HelperMappingBuilder.processModelJoinAssociationMapping, :801-850, asked of legend-engine 4.145.0:
     * port-scratch/normbatch2/engine/cases/e4e, peer47, item4e/sites): the lambda has two parameters; they pair with
     * the association's two ends (resolveLambdaParamNames, :923-980; {@link #enginePairing}); and the body typed once
     * per direction -- first with the association's SECOND property's class as the source and the first's as the
     * target, then the other way (:824-847), the parameter bound to the source matched first, so a name both
     * parameters share is the source in each direction (ModelJoinLambdaRewriteVisitor.mapVariableName, :415) -- reads
     * only properties its classes have, "Can't find property 'p' in class 'C'" (PropertyMappingBuilder
     * .compileModelJoinBooleanLambda, :312). Made only where it is exact ({@link #firstMissing}); elsewhere lite's own
     * flow answers. Until 2026-11-15 these were lite's NotImplementedExceptions, which the normalizer batch's
     * 47d13b73f (d) deferred in a strict build too -- an engine-observable change, though tagged LITE-ONLY (the
     * normalizer's second batch, item 4 (e), ruling 34).
     */
    static void requireEngineModelJoin(ResolvedMapping md, com.legend.model.AssociationMapping.ModelJoin mj,
            com.legend.model.AssociationDefinition ad, ModelBuilder model) {
        List<com.legend.protocol.spec.Variable> ps = mj.lambda().parameters();
        if (ps.size() != 2) {
            throw new ModelException(LegendCompileException.Phase.MAPPING, "ModelJoin requires a lambda with exactly 2"
                    + " parameters, e.g. {a:TypeA[1], b:TypeB[1] | $a.prop == $b.prop} (param types optional)");
        }
        String c1 = ad.property1().targetClassFqn();
        String c2 = ad.property2().targetClassFqn();
        // a temporal end: the engine names and pairs the milestoned properties (originalMilestonedProperties), whose
        // order lite does not hold -- held
        if (temporal(c1, model) || temporal(c2, model)) {
            return;
        }
        // pm1 (the first property navigated: its owner, the second property's class, the source), then pm2
        String[] pm1 = enginePairing(ad.property2(), ad.property1(), ps.get(0), ps.get(1), model);
        String[] pm2 = enginePairing(ad.property1(), ad.property2(), ps.get(0), ps.get(1), model);
        if (mj.lambda().body().size() != 1 || !plainSets(md, c1, model) || !plainSets(md, c2, model)) {
            return;
        }
        com.legend.protocol.spec.ValueSpecification body = mj.lambda().body().get(0);
        String miss = firstMissing(body, binding(pm1[1], c1, pm1[0], c2), model);
        if (miss == null) {
            miss = firstMissing(body, binding(pm2[1], c2, pm2[0], c1), model);
        }
        if (miss != null && !miss.equals(HOLD)) {
            throw new ModelException(LegendCompileException.Phase.MAPPING, miss);
        }
    }

    /**
     * legend-engine's XStore line compile, in its words (PropertyMappingBuilder.visit(XStorePropertyMapping),
     * :151-276, asked: port-scratch/normbatch2/item4e/sites/23): each line in order, its expression typed with
     * {@code $this} the class owning the line's property and {@code $that} the property's class, reads only properties
     * they have, "Can't find property 'p' in class 'C'". Made only where it is exact: lines naming no set ids, each
     * class mapped by one set of its own in the closure (a set's local properties read as its mapping class's), no
     * temporal class; at the first line it cannot decide, nothing more is checked (the normalizer's second batch, item
     * 4 (e); until 2026-11-15 lite's later NotImplementedException, deferred in a strict build since 47d13b73f (d)).
     */
    static void requireEngineXStore(ResolvedMapping md, com.legend.model.AssociationMapping.Cross xs,
            com.legend.model.AssociationDefinition ad, ModelBuilder model) {
        for (var line : xs.propertyMappings2()) {
            com.legend.model.AssociationDefinition.AssociationEndDefinition prop;
            com.legend.model.AssociationDefinition.AssociationEndDefinition other;
            if (line.propertyName().equals(ad.property1().propertyName())) {
                prop = ad.property1();
                other = ad.property2();
            } else if (line.propertyName().equals(ad.property2().propertyName())) {
                prop = ad.property2();
                other = ad.property1();
            } else {
                return;
            }
            String thisCls = other.targetClassFqn();
            String thatCls = prop.targetClassFqn();
            if (line.sourceSetId() != null || line.targetSetId() != null || temporal(thisCls, model)
                    || temporal(thatCls, model)) {
                return;
            }
            Map<String, String> bind = new LinkedHashMap<>();
            Map<String, java.util.Set<String>> locals = new LinkedHashMap<>();
            for (String[] v : new String[][] {{"this", thisCls}, {"that", thatCls}}) {
                ClassMapping only = soleSet(md, v[1], model);
                if (only == null) {
                    return;
                }
                bind.put(v[0], v[1]);
                locals.put(v[0], localNames(only));
            }
            String miss = firstMissing(line.expression(), bind, locals, model);
            if (miss != null) {
                if (!miss.equals(HOLD)) {
                    throw new ModelException(LegendCompileException.Phase.MAPPING, miss);
                }
                return;
            }
        }
    }

    /**
     * resolveLambdaParamNames (:923-980) for the direction whose source is {@code thisEnd}'s class: the two
     * parameters as [the source's, the target's]. Typed both and the ends' classes not one: by type, a parameter
     * taking an end whose class is its type or a subtype ({@code Type.subTypeOf}), the first parameter tried as the
     * source first, else refused; otherwise by name, a parameter named for a property standing for that property's
     * class; else refused -- each in the engine's words.
     */
    static String[] enginePairing(com.legend.model.AssociationDefinition.AssociationEndDefinition thisEnd,
            com.legend.model.AssociationDefinition.AssociationEndDefinition thatEnd,
            com.legend.protocol.spec.Variable p0, com.legend.protocol.spec.Variable p1, ModelBuilder model) {
        String thisCls = thisEnd.targetClassFqn();
        String thatCls = thatEnd.targetClassFqn();
        boolean self = thisCls.equals(thatCls);
        String t0 = p0.type() instanceof com.legend.protocol.TypeExpression.NameRef n0 ? n0.name() : null;
        String t1 = p1.type() instanceof com.legend.protocol.TypeExpression.NameRef n1 ? n1.name() : null;
        if (!self && t0 != null && t1 != null) {
            boolean p0IsThis = subtypeOf(thisCls, t0, model);
            boolean p1IsThat = subtypeOf(thatCls, t1, model);
            boolean p0IsThat = subtypeOf(thatCls, t0, model);
            boolean p1IsThis = subtypeOf(thisCls, t1, model);
            if (p0IsThis && p1IsThat) {
                return new String[] {p0.name(), p1.name()};
            }
            if (p0IsThat && p1IsThis) {
                return new String[] {p1.name(), p0.name()};
            }
            throw new ModelException(LegendCompileException.Phase.MAPPING,
                    "ModelJoin lambda parameter types do not match association property types: expected one parameter"
                    + " of type '" + simpleName(thisCls) + "' and one of type '" + simpleName(thatCls) + "', got '"
                    + p0.name() + ":" + t0 + "' and '" + p1.name() + ":" + t1 + "'");
        }
        String src = thisEnd.propertyName();
        String tgt = thatEnd.propertyName();
        if (p0.name().equals(src) && p1.name().equals(tgt)) {
            return new String[] {p0.name(), p1.name()};
        }
        if (p0.name().equals(tgt) && p1.name().equals(src)) {
            return new String[] {p1.name(), p0.name()};
        }
        if (self) {
            throw new ModelException(LegendCompileException.Phase.MAPPING,
                    "Self-association ModelJoin requires lambda parameter names to match the association property"
                    + " names ('" + src + "' and '" + tgt + "'), got '" + p0.name() + "' and '" + p1.name() + "'");
        }
        throw new ModelException(LegendCompileException.Phase.MAPPING,
                "Cannot pair ModelJoin lambda parameters '" + p0.name() + "' and '" + p1.name()
                + "' with association properties '" + src + "' and '" + tgt + "'."
                + " Either type the parameters explicitly (e.g. {" + p0.name() + ":" + simpleName(thisCls) + "[1], "
                + p1.name() + ":" + simpleName(thatCls) + "[1] | ...})"
                + " or rename the parameters to match the association property names ('" + src + "' and '" + tgt
                + "').");
    }

    /** The engine's variables for one direction: the target's name bound first, the source's over it (a shared name
     *  is the source's). */
    private static Map<String, String> binding(String tgtVar, String tgtCls, String srcVar, String srcCls) {
        Map<String, String> b = new LinkedHashMap<>();
        b.put(tgtVar, tgtCls);
        b.put(srcVar, srcCls);
        return b;
    }

    private static @com.legend.base.Nullable String firstMissing(com.legend.protocol.spec.ValueSpecification v,
            Map<String, String> bind, ModelBuilder model) {
        return firstMissing(v, bind, Map.of(), model);
    }

    /**
     * The first property the expression reads of a class that has it not, in the engine's typing order (a call's
     * arguments in order, a property's receiver before the property), "Can't find property 'p' in class 'C'"; null
     * when it reads none; {@link #HOLD} outside the shapes where that is exact: a root and every Boolean operand a call
     * of equal, and, or or not; equal's operands a literal or a property chain from a bound variable through classes
     * (none temporal, no derived property of the name read, a local property of the variable's set its mapping
     * class's, a miss there held).
     */
    private static @com.legend.base.Nullable String firstMissing(com.legend.protocol.spec.ValueSpecification v,
            Map<String, String> bind, Map<String, java.util.Set<String>> locals, ModelBuilder model) {
        if (!(v instanceof com.legend.protocol.spec.AppliedFunction root) || !booleanCall(root)) {
            return HOLD;
        }
        return missingIn(root, bind, locals, model);
    }

    private static boolean booleanCall(com.legend.protocol.spec.AppliedFunction af) {
        int n = af.parameters().size();
        return n == 2 && (com.legend.compiler.ResolvedNames.names(af,
                        com.legend.builtin.Pure.EQUAL__ANY_MANY__ANY_MANY.qualifiedName())
                    || com.legend.compiler.ResolvedNames.names(af,
                        com.legend.builtin.Pure.AND__BOOLEAN_1__BOOLEAN_1.qualifiedName())
                    || com.legend.compiler.ResolvedNames.names(af,
                        com.legend.builtin.Pure.OR__BOOLEAN_1__BOOLEAN_1.qualifiedName()))
                || n == 1 && com.legend.compiler.ResolvedNames.names(af,
                        com.legend.builtin.Pure.NOT__BOOLEAN_1.qualifiedName());
    }

    private static @com.legend.base.Nullable String missingIn(com.legend.protocol.spec.AppliedFunction af, Map<String, String> bind,
            Map<String, java.util.Set<String>> locals, ModelBuilder model) {
        boolean equal = com.legend.compiler.ResolvedNames.names(af,
                com.legend.builtin.Pure.EQUAL__ANY_MANY__ANY_MANY.qualifiedName());
        for (com.legend.protocol.spec.ValueSpecification arg : af.parameters()) {
            String r;
            if (arg instanceof com.legend.protocol.spec.AppliedFunction call && booleanCall(call)) {
                r = missingIn(call, bind, locals, model);
            } else if (!equal) {
                r = HOLD;
            } else if (arg instanceof com.legend.protocol.spec.CString || arg instanceof com.legend.protocol.spec.CInteger
                    || arg instanceof com.legend.protocol.spec.CBoolean || arg instanceof com.legend.protocol.spec.CFloat) {
                r = null;
            } else {
                r = chainMissing(arg, bind, locals, model);
            }
            if (r != null) {
                return r;
            }
        }
        return null;
    }

    /** A property chain's walk: its refusal (a missing property's words, or HOLD), else the class it lands on (null
     *  for a primitive or a local mapping property). */
    private record Chain(@com.legend.base.Nullable String refusal, @com.legend.base.Nullable String cls) {
    }

    /** A property chain's first property its receiver's class has not, else null, else HOLD. */
    private static @com.legend.base.Nullable String chainMissing(com.legend.protocol.spec.ValueSpecification v,
            Map<String, String> bind, Map<String, java.util.Set<String>> locals, ModelBuilder model) {
        return chain(v, bind, locals, model).refusal();
    }

    private static Chain chain(com.legend.protocol.spec.ValueSpecification v, Map<String, String> bind,
            Map<String, java.util.Set<String>> locals, ModelBuilder model) {
        if (v instanceof com.legend.protocol.spec.Variable var) {
            String c = bind.get(var.name());
            return c == null ? new Chain(HOLD, null) : new Chain(null, c);
        }
        if (!(v instanceof com.legend.protocol.spec.AppliedProperty ap)) {
            return new Chain(HOLD, null);
        }
        java.util.Set<String> own = ap.receiver() instanceof com.legend.protocol.spec.Variable rv
                ? locals.getOrDefault(rv.name(), java.util.Set.of()) : java.util.Set.of();
        Chain receiver = chain(ap.receiver(), bind, locals, model);
        if (receiver.refusal() != null) {
            return receiver;
        }
        String cls = receiver.cls();
        if (cls == null || temporal(cls, model)) {
            return new Chain(HOLD, null);
        }
        if (own.contains(ap.property())) {
            return new Chain(null, null);   // a local mapping property: its type is not read further
        }
        java.util.Optional<ClassDefinition> cd = model.knowledge().hierarchyClass(cls);
        if (cd.isEmpty()) {
            return new Chain(HOLD, null);
        }
        com.legend.protocol.TypeExpression t = model.knowledge().propertyType(cd.get(), ap.property());
        if (t == null) {
            List<ClassDefinition> lineage = lineageOrNull(cls, model);
            boolean derived = lineage == null || lineage.stream().anyMatch(c ->
                    c.derivedProperties().stream().anyMatch(d -> d.name().equals(ap.property())));
            if (derived || !own.isEmpty()) {
                return new Chain(HOLD, null);
            }
            return new Chain("Can't find property '" + ap.property() + "' in class '" + cls + "'", null);
        }
        return new Chain(null, t instanceof com.legend.protocol.TypeExpression.NameRef nr
                && model.knowledge().hierarchyClass(nr.name()).isPresent() ? nr.name() : null);
    }

    /** {@link #temporal}, for the pairing's caller. */
    static boolean temporalEnd(String cls, ModelBuilder model) {
        return temporal(cls, model);
    }

    /** Whether {@code cls} or a class it extends carries a temporal (milestoning) stereotype -- or its lineage is
     *  not the model's to read (a class it names the model has not): held either way. */
    private static boolean temporal(String cls, ModelBuilder model) {
        List<ClassDefinition> lineage = lineageOrNull(cls, model);
        return lineage == null || lineage.stream().anyMatch(c -> c.stereotypes().stream().anyMatch(st ->
                com.legend.compiler.element.MilestoningStrategy.ofStereotypeOrNull(st.profileName(),
                        st.stereotypeName()) != null));
    }

    /** {@code cls}'s lineage, null where a class of it the model has not ("Can't find type": not this check's). */
    private static @com.legend.base.Nullable List<ClassDefinition> lineageOrNull(String cls, ModelBuilder model) {
        if (model.knowledge().hierarchyClass(cls).isEmpty()) {
            return null;
        }
        try {
            return model.knowledge().lineage(cls);
        } catch (ModelException e) {
            return null;
        }
    }

    /** {@code Type.subTypeOf}: the class itself, a class it extends, or Any, which every class extends. */
    private static boolean subtypeOf(String child, String parent, ModelBuilder model) {
        return child.equals(parent) || parent.equals("meta::pure::metamodel::type::Any")
                || model.knowledge().isSubtype(child, parent);
    }

    /** Whether every instance set the engine would pair for {@code cls} (its sets and its subtypes', the closure's) is
     *  the class's own with no local mapping property -- so its class is the engine's {@code _mappingClass() == null}
     *  type. Operation sets are not instance sets. */
    private static boolean plainSets(ResolvedMapping md, String cls, ModelBuilder model) {
        for (ClassMapping cm : md.visibleSets().values()) {
            if (cm instanceof ClassMapping.Union || cm instanceof ClassMapping.Inheritance) {
                continue;
            }
            if (!cm.className().equals(cls)) {
                if (model.knowledge().isSubtype(cm.className(), cls)) {
                    return false;
                }
                continue;
            }
            if (!localNames(cm).isEmpty()) {
                return false;
            }
        }
        return true;
    }

    /** {@code cls}'s one instance set in the closure, if it has exactly one and no subtype is mapped. */
    private static @com.legend.base.Nullable ClassMapping soleSet(ResolvedMapping md, String cls, ModelBuilder model) {
        ClassMapping only = null;
        for (ClassMapping cm : md.visibleSets().values()) {
            if (cm instanceof ClassMapping.Union || cm instanceof ClassMapping.Inheritance) {
                continue;
            }
            if (cm.className().equals(cls)) {
                if (only != null) {
                    return null;
                }
                only = cm;
            } else if (model.knowledge().isSubtype(cm.className(), cls)) {
                return null;
            }
        }
        return only;
    }

    /** A set's local mapping properties' names (its mapping class's own properties). */
    private static java.util.Set<String> localNames(ClassMapping cm) {
        java.util.Set<String> out = new java.util.LinkedHashSet<>();
        switch (cm) {
            case ClassMapping.Relational r -> r.propertyMappings().forEach(pm -> {
                if (pm instanceof PropertyMapping.LocalProperty lp) {
                    out.add(lp.propertyName());
                }
            });
            case ClassMapping.Pure p -> p.propertyBindings().forEach(b -> {
                if (b.local()) {
                    out.add(b.propertyName());
                }
            });
            case ClassMapping.RelationFunction f -> f.columns().forEach(c -> {
                if (c.local()) {
                    out.add(c.property());
                }
            });
            default -> { }
        }
        return out;
    }

    private static String simpleName(String fqn) {
        int i = fqn.lastIndexOf("::");
        return i < 0 ? fqn : fqn.substring(i + 2);
    }
}
