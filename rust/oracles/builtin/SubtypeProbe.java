// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

import com.legend.Compiler;
import com.legend.builtin.Prelude;
import com.legend.builtin.Pure;
import com.legend.builtin.SystemMetamodel;
import com.legend.compiler.element.ModelContext;
import com.legend.model.ClassDefinition;
import com.legend.model.PackageableElement;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.List;
import java.util.TreeSet;

/**
 * SUBTYPING ON THE BOOT MODEL, ASKED OF THE LIVE JAVA (2026-11-12, docs/rust-port/PHASE3.md slice 1b; the owner's
 * ruling 22 of docs/rust-port/PHASE3_SHIM_DESIGN.md §10: subtyping is held to the live Java in slice 1b, with the
 * model; no freeze now): {@code ModelContext.isSubtype(child, parent)} on the compiled boot model (an empty model at
 * lite's dialect: the catalog's native classes, the prelude's classes and the system metamodel's) for every ordered
 * pair of its class FQNs, {@code Nil} and two names no class has. One line per child: {@code child TAB the parents it
 * is a subtype of, sorted, space separated}, or {@code child TAB !<exception class>: <message>}.
 * rust/lite-builtin's {@code lattice::is_subtype} answers the same lines ({@code tests/subtype_live.rs}, run with
 * {@code LITE_SUBTYPE_JAVA} naming this program's output). The output is not frozen: ruling 22 holds it live.
 *
 * <pre>
 *   java ... rust/oracles/builtin/SubtypeProbe.java OUT.tsv   (on the built jars of //core:planner and its closure)
 * </pre>
 */
public final class SubtypeProbe {

    private SubtypeProbe() {
    }

    public static void main(String[] args) throws Exception {
        ModelContext ctx = Compiler.compileModel("");
        TreeSet<String> universe = new TreeSet<>();
        for (ClassDefinition c : Pure.allNativeClasses()) {
            universe.add(c.qualifiedName());
        }
        universe.addAll(Prelude.classFqns());
        for (PackageableElement e : SystemMetamodel.elements()) {
            if (e instanceof ClassDefinition c) {
                universe.add(c.qualifiedName());
            }
        }
        universe.add("meta::pure::metamodel::type::Nil");
        universe.add("meta::pure::metamodel::type::Any");
        universe.add("my::absent::Class");
        universe.add("Absent");
        List<String> lines = new ArrayList<>();
        long pairs = 0;
        long yes = 0;
        for (String child : universe) {
            List<String> parents = new ArrayList<>();
            String failure = null;
            for (String parent : universe) {
                pairs++;
                try {
                    if (ctx.isSubtype(child, parent)) {
                        parents.add(parent);
                        yes++;
                    }
                } catch (RuntimeException e) {
                    failure = "!" + e.getClass().getSimpleName() + ": " + e.getMessage();
                    break;
                }
            }
            lines.add(child + "\t" + (failure != null ? failure : String.join(" ", parents)));
        }
        Files.writeString(Path.of(args[0]), String.join("\n", lines) + "\n", StandardCharsets.UTF_8);
        System.out.printf("[subtype] %d classes, %d pairs, %d subtype%n", universe.size(), pairs, yes);
    }
}
