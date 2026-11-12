// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

import com.legend.builtin.SystemMetamodel;
import com.legend.door.ModelFromText;
import com.legend.error.ModelException;
import com.legend.model.FunctionDefinition;
import com.legend.model.FunctionId;
import com.legend.model.PackageableElement;
import com.legend.model.ParsedModel;
import com.legend.parser.Dialect;
import java.lang.reflect.Field;
import java.util.ArrayList;
import java.util.List;
import java.util.Map;
import java.util.Set;

/**
 * The system metamodel's section of {@link BuiltinAnswers} (2026-11-12, docs/rust-port/PHASE3.md slice 1b):
 * {@code com.legend.builtin.SystemMetamodel} -- its constants, its source (the SHA-256 of the whole text, then each
 * line as a JSON string, so a difference reads at its line), every element as parsed, the FQN sets in their order (the
 * non-function set is the class's private {@code Fqns.NOT_FUNCTIONS}, read by reflection), the functions by id,
 * {@code isSystemElement} over every element FQN and a few that are not, and {@code requireNoSystemElementRedefined}
 * over small models (its message when it refuses). Rust writes the same lines
 * ({@code rust/lite-builtin/tests/tables/metamodel.rs}).
 */
final class MetamodelTables {

    private MetamodelTables() {
    }

    /** FQNs {@code isSystemElement} is asked of beyond the elements' own: none a system element but the last. */
    private static final String[] NOT_ELEMENTS = {
        "my::Foo",
        "",
        "meta::lite::metamodel",
        "MetamodelStore",
        "meta::lite::metamodel::metamodelstore",
        "meta::pure::mapping::Mapping",
        "meta::relational::metamodel::datatype::Varchar",
        " meta::lite::metamodel::MetamodelStore",
        "meta::lite::metamodel::MetamodelStore",
    };

    /** The models {@code requireNoSystemElementRedefined} is asked of: {name, text at lite's dialect}. */
    private static final String[][] MODELS = {
        {"empty", ""},
        {"none", "Class my::app::Person\n{\n    name: String[1];\n}\n"},
        {"systemClass", "Class my::app::Person\n{\n}\n\nClass meta::lite::metamodel::MappingVisibility\n{\n"
            + "    includeRank: Integer[1];\n}\n"},
        {"systemPlatformClass", "Class meta::relational::metamodel::datatype::Time\n{\n}\n"},
        {"systemAssociation", "Association meta::lite::metamodel::AliasBaseTables\n{\n    base: my::A[1];\n"
            + "    baseOf: my::B[*];\n}\n"},
        {"systemStore", "###Relational\nDatabase meta::lite::metamodel::MetamodelStore\n(\n)\n"},
        {"systemMapping", "###Mapping\nMapping meta::lite::metamodel::MetamodelMapping\n(\n)\n"},
        {"systemFunction", "function meta::pure::mapping::classMappings(_this:meta::pure::mapping::Mapping[1])"
            + ":meta::pure::mapping::SetImplementation[*]\n{\n    []\n}\n"},
        {"twoSystem", "###Mapping\nMapping meta::lite::metamodel::MetamodelMapping\n(\n)\n\n###Pure\n"
            + "Class meta::lite::metamodel::StoreResolution\n{\n}\n"},
    };

    static void dump(BuiltinAnswers.Out out) throws Exception {
        // ---- the constants
        out.line("metamodel.const", "STORE_FQN", SystemMetamodel.STORE_FQN);
        out.line("metamodel.const", "ORDINAL_COLUMN", SystemMetamodel.ORDINAL_COLUMN);
        out.line("metamodel.const", "MAPPING_FQN", SystemMetamodel.MAPPING_FQN);
        // ---- the source: the whole text's hash and length (UTF-16 units), then each line
        String source = SystemMetamodel.source();
        out.line("metamodel.source", BuiltinAnswers.Out.sha(source), source.length());
        List<String> lines = source.lines().toList();
        for (int i = 0; i < lines.size(); i++) {
            out.line("metamodel.sourceLine", i, BuiltinAnswers.Out.str(lines.get(i)));
        }
        // ---- the elements, as parsed
        List<PackageableElement> elements = SystemMetamodel.elements();
        for (int i = 0; i < elements.size(); i++) {
            PackageableElement e = elements.get(i);
            out.line("metamodel.element", i, e.qualifiedName(), out.json(e, PackageableElement.class));
        }
        // ---- the FQN sets, in their (insertion) order
        out.line("metamodel.elementFqns", BuiltinAnswers.Out.join(SystemMetamodel.elementFqns()));
        Field notFunctions = Class.forName("com.legend.builtin.SystemMetamodel$Fqns").getDeclaredField("NOT_FUNCTIONS");
        notFunctions.setAccessible(true);
        out.line("metamodel.notFunctionFqns", BuiltinAnswers.Out.join((Set<?>) notFunctions.get(null)));
        // ---- the functions by id, in order: each id and the index of its definition among the elements (the last
        // identical; Rust asks the record's equality, the source guards keeping addresses out of its crates)
        int n = 0;
        for (Map.Entry<FunctionId, FunctionDefinition> e : SystemMetamodel.functionsById().entrySet()) {
            int at = -1;
            for (int i = 0; i < elements.size(); i++) {
                if (elements.get(i) == e.getValue()) {
                    at = i;
                }
            }
            out.line("metamodel.functionById", n++, e.getKey().qualified(), at);
        }
        out.line("metamodel.functionIds", BuiltinAnswers.Out.join(SystemMetamodel.functionIds()));
        // ---- isSystemElement, over every element FQN and a few that are not
        List<String> asked = new ArrayList<>(SystemMetamodel.elementFqns());
        asked.addAll(List.of(NOT_ELEMENTS));
        for (String fqn : asked) {
            out.line("metamodel.isSystemElement", BuiltinAnswers.Out.str(fqn), SystemMetamodel.isSystemElement(fqn));
        }
        // ---- requireNoSystemElementRedefined, over small models
        for (String[] m : MODELS) {
            ParsedModel parsed = ModelFromText.parse(m[1], Dialect.LEGEND_LITE);
            try {
                ParsedModel back = SystemMetamodel.requireNoSystemElementRedefined(parsed);
                out.line("metamodel.requireNoSystemElementRedefined", m[0], "returned", back == parsed);
            } catch (ModelException e) {
                out.line("metamodel.requireNoSystemElementRedefined", m[0], "ModelException", e.phase().name(),
                        e.element(), e.getMessage());
            }
        }
    }
}
