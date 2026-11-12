// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

import com.legend.builtin.DynaFn;
import com.legend.builtin.EngineHandlers;
import com.legend.builtin.Prelude;
import com.legend.builtin.Pure;
import com.legend.builtin.Subsumed;
import com.legend.builtin.TdsLegacy;
import com.legend.model.ClassDefinition;
import com.legend.model.EnumDefinition;
import com.legend.model.FunctionId;
import com.legend.model.NativeFunctionDefinition;
import com.legend.model.ParsedModel;
import com.legend.protocol.TypeExpression;
import java.lang.reflect.Field;
import java.lang.reflect.Modifier;
import java.util.ArrayList;
import java.util.IdentityHashMap;
import java.util.List;
import java.util.Map;
import java.util.TreeMap;
import java.util.TreeSet;

/**
 * The catalog's sections of {@link BuiltinAnswers} (2026-11-12, docs/rust-port/PHASE3.md slice 1b): {@code Pure} (every
 * native, class and enumeration with its constant's name, the overload groups, the string constants, the lite
 * vocabulary, the sets and every index answer over every FQN the catalog holds), {@code DynaFn} with its decisions,
 * {@code EngineHandlers}, {@code Prelude}, {@code Subsumed} and {@code TdsLegacy}. Rust writes the same lines
 * ({@code rust/lite-builtin/tests/tables/pure.rs}).
 */
final class PureTables {

    private PureTables() {
    }

    /** The constant name of each value of {@code type} declared on {@code owner}, by identity. */
    private static IdentityHashMap<Object, String> constants(Class<?> owner, Class<?> type) throws Exception {
        IdentityHashMap<Object, String> out = new IdentityHashMap<>();
        for (Field f : owner.getDeclaredFields()) {
            if (Modifier.isStatic(f.getModifiers()) && Modifier.isPublic(f.getModifiers()) && f.getType() == type) {
                out.put(f.get(null), f.getName());
            }
        }
        return out;
    }

    /** Every public static String constant of {@code owner}, by name. */
    private static TreeMap<String, String> strings(Class<?> owner) throws Exception {
        TreeMap<String, String> out = new TreeMap<>();
        for (Field f : owner.getDeclaredFields()) {
            if (Modifier.isStatic(f.getModifiers()) && Modifier.isPublic(f.getModifiers()) && f.getType() == String.class) {
                out.put(f.getName(), (String) f.get(null));
            }
        }
        return out;
    }

    static void dump(BuiltinAnswers.Out out) throws Exception {
        // ---- the catalog
        IdentityHashMap<Object, String> nativeNames = constants(Pure.class, NativeFunctionDefinition.class);
        List<NativeFunctionDefinition> all = Pure.all();
        for (int i = 0; i < all.size(); i++) {
            NativeFunctionDefinition d = all.get(i);
            out.line("pure.native", i, nativeNames.get(d), FunctionId.of(d).qualified(),
                    out.json(d, NativeFunctionDefinition.class));
        }
        IdentityHashMap<Object, String> classNames = constants(Pure.class, ClassDefinition.class);
        List<ClassDefinition> classes = Pure.allNativeClasses();
        for (int i = 0; i < classes.size(); i++) {
            out.line("pure.class", i, classNames.get(classes.get(i)), out.json(classes.get(i), ClassDefinition.class));
        }
        IdentityHashMap<Object, String> enumNames = constants(Pure.class, EnumDefinition.class);
        List<EnumDefinition> enums = Pure.allNativeEnums();
        for (int i = 0; i < enums.size(); i++) {
            out.line("pure.enum", i, enumNames.get(enums.get(i)), out.json(enums.get(i), EnumDefinition.class));
        }
        TreeMap<String, List<?>> groups = new TreeMap<>();
        for (Field f : Pure.class.getDeclaredFields()) {
            if (Modifier.isStatic(f.getModifiers()) && f.getName().startsWith("AT_") && f.getType() == List.class) {
                groups.put(f.getName(), (List<?>) f.get(null));
            }
        }
        for (Map.Entry<String, List<?>> g : groups.entrySet()) {
            out.line("pure.group", g.getKey(), BuiltinAnswers.Out.join(g.getValue()));
        }
        // ---- the constants
        for (Map.Entry<String, String> s : strings(Pure.class).entrySet()) {
            out.line("pure.string", s.getKey(), s.getValue());
        }
        for (Map.Entry<String, String> s : strings(Pure.Lite.class).entrySet()) {
            out.line("pure.lite", s.getKey(), s.getValue());
        }
        out.line("pure.internalDesugar", BuiltinAnswers.Out.join(Pure.INTERNAL_DESUGAR));
        out.line("pure.engineVocabShims", BuiltinAnswers.Out.join(Pure.ENGINE_VOCAB_SHIMS));
        out.line("pure.liteSurface", BuiltinAnswers.Out.sorted(Pure.LITE_SURFACE));
        for (Map.Entry<String, List<FunctionId>> e : new TreeMap<>(Pure.liteSurfaceFunctions()).entrySet()) {
            out.line("pure.liteSurfaceFunctions", e.getKey(), BuiltinAnswers.Out.join(e.getValue()));
        }
        out.line("pure.liteInternalNatives", BuiltinAnswers.Out.join(Pure.liteInternalNatives()));
        List<String> resolvable = new ArrayList<>(Pure.userResolvableFunctionFqns());
        for (int i = 0; i < resolvable.size(); i++) {
            out.line("pure.userResolvable", i, resolvable.get(i));
        }
        out.line("pure.nativeClassFqns", BuiltinAnswers.Out.sorted(Pure.nativeClassFqns()));
        out.line("pure.nativeEnumFqns", BuiltinAnswers.Out.sorted(Pure.nativeEnumFqns()));
        for (String fqn : new TreeSet<>(Pure.walledNativeFqns())) {
            out.line("pure.walled", fqn, Pure.walledNativeReason(fqn));
        }
        // ---- the indexes, asked of every FQN the catalog holds
        TreeSet<String> fqns = new TreeSet<>();
        for (NativeFunctionDefinition d : all) {
            fqns.add(d.qualifiedName());
        }
        for (String fqn : fqns) {
            List<String> at = new ArrayList<>();
            for (NativeFunctionDefinition d : Pure.nativeFunctionsAt(fqn)) {
                at.add(FunctionId.of(d).qualified());
            }
            out.line("pure.at", fqn, Pure.isToOneCall(fqn), Pure.isVariadicRun(List.of(fqn)), BuiltinAnswers.Out.join(at));
        }
        out.line("pure.isToOneCall", "meta::pure::functions::multiplicity::toOne",
                Pure.isToOneCall("meta::pure::functions::multiplicity::toOne"));
        try {
            Pure.nativeFunctionsAt("toOne");
            out.line("pure.bareName", "toOne", "answered");
        } catch (IllegalArgumentException e) {
            out.line("pure.bareName", "toOne", e.getMessage());
        }
        TreeSet<String> ids = new TreeSet<>();
        for (NativeFunctionDefinition d : all) {
            ids.add(FunctionId.of(d).qualified());
        }
        for (String id : ids) {
            out.line("pure.byId", id, nativeNames.get(Pure.nativeFunctionById(id)));
        }
        TreeSet<String> supers = new TreeSet<>();
        for (ClassDefinition c : classes) {
            supers.add(c.qualifiedName());
            for (TypeExpression s : c.superClasses()) {
                if (s instanceof TypeExpression.NameRef nr) {
                    supers.add(nr.name());
                }
            }
        }
        for (String fqn : supers) {
            out.line("pure.directSubclasses", fqn, BuiltinAnswers.Out.join(Pure.directNativeSubclasses(fqn)),
                    Pure.findNativeClass(fqn).isPresent(), Pure.findNativeEnum(fqn).isPresent());
        }
        for (EnumDefinition e : enums) {
            out.line("pure.findNativeEnum", e.qualifiedName(), Pure.findNativeEnum(e.qualifiedName()).isPresent());
        }
        // ---- the engine's dynafunctions and the platform's decisions
        for (DynaFn d : DynaFn.values()) {
            String lite;
            try {
                lite = d.liteFqn();
            } catch (IllegalStateException e) {
                lite = "!" + e.getMessage();
            }
            out.line("dynafn", d.name(), d.dynaName(), d.inference().name(), d.resolution().name(),
                    String.join(",", d.fqns()), lite, DynaFn.of(d.dynaName()).orElseThrow().name());
        }
        for (DynaFn.Resolution r : DynaFn.Resolution.values()) {
            out.line("dynafn.withResolution", r.name(),
                    BuiltinAnswers.Out.join(DynaFn.withResolution(r).stream().map(Enum::name).toList()));
        }
        // ---- the engine's handler surface
        for (String name : EngineHandlers.names()) {
            out.line("handlers", name, BuiltinAnswers.Out.join(EngineHandlers.idsOf(name)),
                    BuiltinAnswers.Out.join(EngineHandlers.fqnsOf(name)));
        }
        out.line("handlers.undeclared", BuiltinAnswers.Out.join(EngineHandlers.undeclaredIds()));
        out.line("handlers.unmatchedSurface", BuiltinAnswers.Out.join(EngineHandlers.unmatchedSurface()));
        // ---- the prelude
        out.line("prelude.source", BuiltinAnswers.Out.sha(Prelude.source()));
        out.line("prelude.model", BuiltinAnswers.Out.sha(out.json(Prelude.parsedModel(), ParsedModel.class)));
        out.line("prelude.elements", BuiltinAnswers.Out.join(Prelude.elementFqns()));
        out.line("prelude.classes", BuiltinAnswers.Out.join(Prelude.classFqns()));
        out.line("prelude.enums", BuiltinAnswers.Out.join(Prelude.enumFqns()));
        out.line("prelude.functions", BuiltinAnswers.Out.join(Prelude.functionFqns()));
        out.line("prelude.functionIds", BuiltinAnswers.Out.join(Prelude.functionIds()));
        out.line("prelude.profiles", BuiltinAnswers.Out.join(Prelude.profileFqns()));
        // ---- the programs subsumed, and the legacy TDS vocabulary
        for (Subsumed s : Subsumed.values()) {
            out.line("subsumed", s.name(), s.fqn(), s.engineFile(), s.fromLine(), s.toLine(), s.reason(),
                    Subsumed.of(s.fqn()).orElseThrow().name());
        }
        for (TdsLegacy t : TdsLegacy.values()) {
            out.line("tdsLegacy", t.name(), t.fqn(), t.bare());
        }
    }
}
