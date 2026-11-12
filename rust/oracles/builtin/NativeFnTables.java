// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

import com.legend.builtin.NativeFn;
import com.legend.builtin.Pure;
import com.legend.model.ClassDefinition;
import com.legend.model.ClassMember;
import com.legend.model.FunctionId;
import com.legend.model.NativeFunctionDefinition;
import java.lang.reflect.Field;
import java.lang.reflect.Method;
import java.lang.reflect.Modifier;
import java.util.ArrayList;
import java.util.IdentityHashMap;
import java.util.List;
import java.util.Map;
import java.util.Optional;
import java.util.TreeMap;
import java.util.TreeSet;
import java.util.function.Function;

/**
 * The native function families' section of {@link BuiltinAnswers} (2026-11-12, docs/rust-port/PHASE3.md slice 1b):
 * {@code com.legend.builtin.NativeFn}. Rust writes the same lines ({@code rust/lite-builtin/tests/tables/native_fn.rs}).
 *
 * <p>Every family in {@code families()}'s order, each member with its name, FQN, bare name and overloads (each by its
 * constant's name and its identity) and every instance answer it has; the members an applied-name probe set
 * {@code matches}; the package-private indexes ({@code index}, {@code indexById}, by reflection) of every family; every
 * family's {@code of(id)} and every id predicate over every catalog id, the null id and one id no native has;
 * {@code claims} and the by-name lookups ({@code RowGetter.of/ofLifted/isOwner}, {@code JavaRoutine.ofDerived}) over
 * every catalog FQN, every member's spellings and the edge texts; the string constants; {@code OperatorRun}. A member
 * is its {@code name()}, an empty answer {@code -}, an id its {@code qualified()}; a set Java keeps in a hash structure
 * is written sorted.
 */
final class NativeFnTables {

    private NativeFnTables() {
    }

    /** One family's {@code of(id)}, by the family's name. */
    private record Of(String family, Function<FunctionId, Optional<? extends Enum<?>>> of) {
    }

    /** Every family's {@code of(id)}, in {@code families()}'s order (RowGetter and GroupAggregate have none), then
     *  {@code OperatorRun.of}. */
    private static final List<Of> OFS = List.of(
            new Of("Calendar", NativeFn.Calendar::of),
            new Of("Verdict", NativeFn.Verdict::of),
            new Of("Frame", NativeFn.Frame::of),
            new Of("LowererForm", NativeFn.LowererForm::of),
            new Of("LiteralForm", NativeFn.LiteralForm::of),
            new Of("ContextOption", NativeFn.ContextOption::of),
            new Of("PlanWrapper", NativeFn.PlanWrapper::of),
            new Of("RelationQuantifier", NativeFn.RelationQuantifier::of),
            new Of("ObjectReference", NativeFn.ObjectReference::of),
            new Of("SubtypeForm", NativeFn.SubtypeForm::of),
            new Of("ResolverForm", NativeFn.ResolverForm::of),
            new Of("LiteDesugar", NativeFn.LiteDesugar::of),
            new Of("TyperForm", NativeFn.TyperForm::of),
            new Of("JavaRoutine", NativeFn.JavaRoutine::of),
            new Of("Handle", NativeFn.Handle::of),
            new Of("Effect", NativeFn.Effect::of),
            new Of("Carrier", NativeFn.Carrier::of),
            new Of("ContextOwner", NativeFn.ContextOwner::of),
            new Of("DdlStatement", NativeFn.DdlStatement::of),
            new Of("OperatorRun", NativeFn.OperatorRun::of));

    /** The constant name of each native declared on {@code Pure}, by identity. */
    private static IdentityHashMap<Object, String> nativeNames() throws Exception {
        IdentityHashMap<Object, String> out = new IdentityHashMap<>();
        for (Field f : Pure.class.getDeclaredFields()) {
            if (Modifier.isStatic(f.getModifiers()) && Modifier.isPublic(f.getModifiers())
                    && f.getType() == NativeFunctionDefinition.class) {
                out.put(f.get(null), f.getName());
            }
        }
        return out;
    }

    private static String name(Optional<? extends Enum<?>> e) {
        return e.map(Enum::name).orElse("-");
    }

    private static String name(Enum<?> e) {
        return e == null ? "-" : e.name();
    }

    /** The overloads by their constants' names, then by their identities. */
    private static String[] overloads(List<NativeFunctionDefinition> overloads, IdentityHashMap<Object, String> names) {
        List<String> ns = new ArrayList<>();
        List<String> ids = new ArrayList<>();
        for (NativeFunctionDefinition o : overloads) {
            ns.add(names.get(o));
            ids.add(FunctionId.of(o).qualified());
        }
        return new String[] {BuiltinAnswers.Out.join(ns), BuiltinAnswers.Out.join(ids)};
    }

    /** The id predicates of every family that has one, in the family's order. */
    private static List<Object> predicates(FunctionId id) {
        List<Object> p = new ArrayList<>();
        p.add(NativeFn.Handle.isExecute(id));
        p.add(NativeFn.Handle.runsAFrame(id));
        p.add(NativeFn.Handle.forcesAtValuePosition(id));
        for (int n = 0; n <= 4; n++) {
            p.add(NativeFn.Handle.orchestrationArgument(id, n));
        }
        p.add(NativeFn.Effect.isDbEffect(id));
        p.add(NativeFn.Effect.isSeedSqlForm(id));
        p.add(NativeFn.Effect.isInertDiagnostic(id));
        p.add(NativeFn.Effect.readsArgumentsAtPlanning(id));
        for (int k = 0; k <= 3; k++) {
            p.add(NativeFn.Effect.argumentReadAtPlanning(id, k));
        }
        p.add(name(NativeFn.Carrier.fetchDbGrid(id)));
        return p;
    }

    /** One id's line: every family's {@code of}, then {@code isBivariateMap} (not nullable), then the predicates. */
    private static void byId(BuiltinAnswers.Out out, String section, Object key, FunctionId id) {
        List<Object> fields = new ArrayList<>();
        fields.add(key);
        for (Of o : OFS) {
            fields.add(name(o.of().apply(id)));
        }
        if (id != null) {
            fields.add(NativeFn.LowererForm.isBivariateMap(id));
        }
        fields.addAll(predicates(id));
        out.line(section, fields.toArray());
    }

    static void dump(BuiltinAnswers.Out out) throws Exception {
        IdentityHashMap<Object, String> names = nativeNames();
        Map<String, List<? extends NativeFn.Member>> families = NativeFn.families();
        out.line("nativefn.families", String.join(" ", families.keySet()));
        // ---- the applied-name probes: every member's FQN and bare name, qualified elsewhere, and a near miss
        TreeSet<String> probes = new TreeSet<>();
        for (List<? extends NativeFn.Member> ms : families.values()) {
            for (NativeFn.Member m : ms) {
                probes.add(m.fqn());
                probes.add(m.bareName());
                probes.add("x::" + m.bareName());
                probes.add("x" + m.bareName());
            }
        }
        // ---- the families: each member, what it matches, and its instance answers
        for (Map.Entry<String, List<? extends NativeFn.Member>> f : families.entrySet()) {
            List<? extends NativeFn.Member> ms = f.getValue();
            out.line("nativefn.family", f.getKey(), ms.size());
            for (int i = 0; i < ms.size(); i++) {
                NativeFn.Member m = ms.get(i);
                String n = ((Enum<?>) m).name();
                String[] o = overloads(m.overloads(), names);
                out.line("nativefn.member", f.getKey(), i, n, m.fqn(), m.bareName(), o[0], o[1]);
                List<String> matched = new ArrayList<>();
                for (String p : probes) {
                    if (m.matches(p)) {
                        matched.add(p);
                    }
                }
                out.line("nativefn.matches", f.getKey(), n, BuiltinAnswers.Out.join(matched));
            }
        }
        for (NativeFn.ResolverForm r : NativeFn.ResolverForm.values()) {
            out.line("nativefn.resolverForm", r.name(), r.classWrite());
        }
        for (NativeFn.JavaRoutine r : NativeFn.JavaRoutine.values()) {
            out.line("nativefn.javaRoutine", r.name(), r.implementedDerived().orElse("-"),
                    r.implementedMember().map(c -> out.json(c, ClassMember.class)).orElse("-"));
        }
        for (NativeFn.RowGetter g : NativeFn.RowGetter.values()) {
            out.line("nativefn.rowGetter", g.name(), g.owner(), g.property(), g.typedCell(),
                    out.json(g.member(), ClassMember.class), g.overloads().size());
        }
        out.line("nativefn.fetchDbGrid", BuiltinAnswers.Out.join(
                java.util.Arrays.stream(NativeFn.Carrier.FetchDbGrid.values()).map(Enum::name).toList()));
        // ---- the package-private indexes of every family, by reflection, in key order
        Method index = NativeFn.class.getDeclaredMethod("index", Enum[].class);
        Method indexById = NativeFn.class.getDeclaredMethod("indexById", Enum[].class);
        index.setAccessible(true);
        indexById.setAccessible(true);
        for (Map.Entry<String, List<? extends NativeFn.Member>> f : families.entrySet()) {
            Object values = ((Enum<?>) f.getValue().get(0)).getDeclaringClass().getEnumConstants();
            @SuppressWarnings("unchecked")
            Map<String, Enum<?>> byFqn = (Map<String, Enum<?>>) index.invoke(null, values);
            for (Map.Entry<String, Enum<?>> e : new TreeMap<>(byFqn).entrySet()) {
                out.line("nativefn.index", f.getKey(), e.getKey(), e.getValue().name());
            }
            @SuppressWarnings("unchecked")
            Map<FunctionId, Enum<?>> byId = (Map<FunctionId, Enum<?>>) indexById.invoke(null, values);
            out.line("nativefn.indexById.size", f.getKey(), byId.size());
            for (Map.Entry<FunctionId, Enum<?>> e : new TreeMap<>(byId).entrySet()) {
                out.line("nativefn.indexById", f.getKey(), e.getKey().qualified(), e.getValue().name());
            }
        }
        // ---- every family's of(id) and every id predicate, over every catalog id, the null id and an unknown one
        List<String> columns = new ArrayList<>();
        for (Of o : OFS) {
            columns.add(o.family());
        }
        out.line("nativefn.of.columns", BuiltinAnswers.Out.join(columns));
        List<NativeFunctionDefinition> all = Pure.all();
        for (int i = 0; i < all.size(); i++) {
            byId(out, "nativefn.of", FunctionId.of(all.get(i)).qualified(), FunctionId.of(all.get(i)));
        }
        byId(out, "nativefn.of.null", "null", null);
        FunctionId unknown = new FunctionId("meta::pure::functions::math::plus");
        byId(out, "nativefn.of.unknown", unknown.qualified(), unknown);
        // ---- claims and the by-name lookups, over every catalog FQN, every member's spellings and the edge texts
        TreeSet<String> texts = new TreeSet<>(probes);
        for (NativeFunctionDefinition d : all) {
            texts.add(d.qualifiedName());
        }
        for (ClassDefinition c : Pure.allNativeClasses()) {
            texts.add(c.qualifiedName());
        }
        for (NativeFn.RowGetter g : NativeFn.RowGetter.values()) {
            texts.add(g.owner());
            texts.add(g.property());
            texts.add("a::b::" + g.property());
            texts.add(g.owner() + "$prop$");
            texts.add(g.fqn() + "$prop$x");
        }
        for (NativeFn.JavaRoutine r : NativeFn.JavaRoutine.values()) {
            r.implementedDerived().ifPresent(texts::add);
        }
        texts.addAll(List.of("", "::", ":::get", "get::", "$prop$", "$prop$get", "x$prop$"));
        for (String t : texts) {
            out.line("nativefn.byName", t, NativeFn.claims(t), name(NativeFn.RowGetter.of(t)),
                    name(NativeFn.RowGetter.ofLifted(t)), NativeFn.RowGetter.isOwner(t),
                    name(NativeFn.JavaRoutine.ofDerived(t)));
        }
        out.line("nativefn.byName.null", name(NativeFn.RowGetter.of(null)), name(NativeFn.RowGetter.ofLifted(null)),
                name(NativeFn.JavaRoutine.ofDerived(null)));
        // ---- the string constants
        TreeMap<String, String> strings = new TreeMap<>();
        for (Field f : NativeFn.class.getDeclaredFields()) {
            if (Modifier.isStatic(f.getModifiers()) && Modifier.isPublic(f.getModifiers())
                    && f.getType() == String.class) {
                strings.put(f.getName(), (String) f.get(null));
            }
        }
        for (Map.Entry<String, String> s : strings.entrySet()) {
            out.line("nativefn.string", s.getKey(), s.getValue());
        }
        // ---- the operator runs (no family: a classification the typer reads)
        NativeFn.OperatorRun[] runs = NativeFn.OperatorRun.values();
        for (int i = 0; i < runs.length; i++) {
            String[] o = overloads(runs[i].overloads(), names);
            out.line("nativefn.operatorRun", i, runs[i].name(), runs[i].fqn(), o[0], o[1]);
        }
    }
}
