// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! `SystemMetamodel`: THE system metamodel store -- the metamodel lives IN the database. `Class.all()` is an ordinary
//! mapped-class query over `metamodel.classes`, seeded from the compiler's own registry, a real `SELECT` the database
//! executes through the existing store lane.
//!
//! One owner, three surfaces: (1) the system Database and Mapping (and the navigation FUNCTIONS over them) are fixed
//! Pure SOURCE ([`source`]), parsed once and injected into every model build -- the resolver sees them exactly as
//! parsed elements ([`elements`]); (2) the seeds render the graph's facts per table into the graph's own system
//! database (not this crate's); (3) the FQN constants ([`STORE_FQN`], [`MAPPING_FQN`]) are the exact-FQN
//! identification surface. Every element table keys on the FQN (`~primaryKey`); `name` is the print form the mapping
//! binds. Grow BY WITNESS ONLY.
//!
//! Ported from core's `com.legend.builtin.SystemMetamodel` (2026-11-12, docs/rust-port/PHASE3.md slice 1b) as the
//! Java writes it: the source is the class's text block (its value: the incidental twelve spaces stripped, each
//! escape `\n` a line feed, as Java's text-block rules make it: `TEMPLATE`) with Java's `%N$s` placeholders filled
//! by `formatted` from the same builders over the same kind tables, so [`source`] is the Java string unit for unit.
//! The parse runs at lite's dialect on the first read of the elements (Java parses at its class initialization, on
//! the first touch of anything but the constants; a source that does not parse is the platform's bug, a panic), and
//! the FQN sets and the functions by id on the first read of any of them (Java's holder class `Fqns`), each in the
//! order Java's linked collections keep. A set is kept beside its ordered list for membership only; nothing here
//! iterates a hash structure.
//!
//! Held to Java's by `tests/builtin_tables.rs` (the section `metamodel.*` of rust/oracles/builtin/builtin-tables.tsv).

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, LazyLock};

use lite_error::{CompileError, Phase};
use lite_front::doors::Dialect;
use lite_front::jtypes::JStr;
use lite_model::{Function, FunctionDefinition, FunctionId, PackageableElement, ParsedModel};

/// The store reference every table, join and filter of the source is qualified by (Java's `S`), a macro so the kind
/// tables' property lines are constants as Java's `"..." + S + "..."` are.
macro_rules! store_ref {
    () => {
        "[meta::lite::metamodel::MetamodelStore]"
    };
}

/// Appends each part to `sb`, in order (Java's `sb.append(a).append(b)...`). Invoked with braces, so the builders keep
/// one Rust line per line of the source they write, as the Java spells them (rustfmt leaves a braced body as written).
macro_rules! push {
    ($sb:ident; $($part:expr),+ $(,)?) => {{
        $( $sb.push_str($part); )+
    }};
}

/// The parts as one string (Java's `a + b + ...` over strings).
macro_rules! parts {
    ($($part:expr),+ $(,)?) => {
        [$($part),+].concat()
    };
}

/// The system store element (charter §4).
pub const STORE_FQN: &str = "meta::lite::metamodel::MetamodelStore";

/// The system store's ORDER column: every ordered child collection (a table's columns, a function's parameters, a
/// key's columns) keeps each row's declaration position here -- a positional read (`->at(k)`) over such a navigation
/// is a filter on it.
pub const ORDINAL_COLUMN: &str = "ordinal";

/// The system mapping -- D1's ambient execution context supplies it.
pub const MAPPING_FQN: &str = "meta::lite::metamodel::MetamodelMapping";

const S: &str = store_ref!();

const INHERITANCE_OP: &str =
    "meta::pure::router::operations::inheritance_OperationSetImplementation_1__SetImplementation_MANY_()";

/// The SQL data-type kinds the store models -- m3 datatype class simple names, one filtered set per kind over
/// `data_types` (the engine idiom for a hierarchy over one table); the seed writes the same spelling in `dt_kind`.
const DATA_TYPE_KINDS: [&str; 23] = [
    "BigInt",
    "SmallInt",
    "TinyInt",
    "Integer",
    "Float",
    "Double",
    "Real",
    "Bit",
    "Timestamp",
    "Date",
    "Time",
    "TimestampNs",
    "Distinct",
    "Other",
    "SemiStructured",
    "Varchar",
    "Char",
    "Binary",
    "Varbinary",
    "Decimal",
    "Numeric",
    "Array",
    "Object",
];

/// The relational-operation node kinds the store models: {m3 class simple name, set id, property lines}.
const OP_KINDS: [[&str; 3]; 5] = [
    [
        "DynaFunction",
        "opDyna",
        concat!("name: ", store_ref!(), " metamodel.relational_elements.dyna_name"),
    ],
    ["Literal", "opLit", ""],
    ["LiteralList", "opLitList", ""],
    [
        "TableAliasColumn",
        "opTac",
        concat!(
            "columnName: ",
            store_ref!(),
            " metamodel.relational_elements.col_name,\n",
            "                    column[col]: ",
            store_ref!(),
            "@OpToColumn"
        ),
    ],
    ["RelationalOperationElementWithJoin", "opJoin", ""],
];

/// The relation kinds of the element table (the op-node kinds are [`OP_KINDS`]); the seed writes the same spelling in
/// `kind`.
const ELEMENT_KINDS: [&str; 4] = ["Table", "View", "Column", "TableAlias"];

/// The plan node kinds the store models: {class FQN, m3 simple name (= the kind spelling the plan's rows write), set
/// id, own property lines}. A plan's nodes RIDE THE QUERY as inline rows; the base ExecutionNode is the inheritance
/// operation over these members.
const PLAN_NODE_KINDS: [[&str; 4]; 4] = [
    [
        "meta::pure::executionPlan::SequenceExecutionNode",
        "SequenceExecutionNode",
        "pnSeq",
        "",
    ],
    [
        "meta::pure::executionPlan::FunctionParametersValidationNode",
        "FunctionParametersValidationNode",
        "pnFpv",
        concat!(
            "functionParameters[planParam]: ",
            store_ref!(),
            "@NodeToFunctionParameters"
        ),
    ],
    [
        "meta::relational::mapping::RelationalInstantiationExecutionNode",
        "RelationalInstantiationExecutionNode",
        "pnRel",
        "",
    ],
    [
        "meta::relational::mapping::SQLExecutionNode",
        "SQLExecutionNode",
        "pnSql",
        concat!(
            "sqlQuery: ",
            store_ref!(),
            " metamodel.plan_nodes.sql_query,\n",
            "                    sqlComment: ",
            store_ref!(),
            " metamodel.plan_nodes.sql_comment,\n",
            "                    connection[pcTest]: ",
            store_ref!(),
            "@NodeToConnection,\n",
            "                    connection[pcRel]: ",
            store_ref!(),
            "@NodeToConnection"
        ),
    ],
];

/// The plan CONNECTION kinds (the engine's plan carries the runtime connection on every SQLExecutionNode): {class FQN,
/// kind spelling, set id, own property lines}. The base DatabaseConnection is the inheritance operation over them.
const CONNECTION_KINDS: [[&str; 4]; 2] = [
    [
        "meta::external::store::relational::runtime::TestDatabaseConnection",
        "TestDatabaseConnection",
        "pcTest",
        concat!(
            "testDataSetupCsv: ",
            store_ref!(),
            " metamodel.plan_connections.test_data_setup_csv,\n",
            "                    testDataSetupSqls: ",
            store_ref!(),
            "@ConnToSqls | metamodel.plan_connection_sqls.text"
        ),
    ],
    [
        "meta::external::store::relational::runtime::RelationalDatabaseConnection",
        "RelationalDatabaseConnection",
        "pcRel",
        concat!("datasourceSpecification[dsH2]: ", store_ref!(), "@ConnToDatasource"),
    ],
];

/// The datasource-specification kinds over the same connection row (`ds_kind`): {class FQN, kind spelling, set id,
/// own property lines}.
const DATASOURCE_KINDS: [[&str; 4]; 1] = [[
    "meta::pure::alloy::connections::alloy::specification::LocalH2DatasourceSpecification",
    "LocalH2DatasourceSpecification",
    "dsH2",
    concat!(
        "testDataSetupCsv: ",
        store_ref!(),
        " metamodel.plan_connections.ds_test_data_setup_csv,\n",
        "                    testDataSetupSqls: ",
        store_ref!(),
        "@DatasourceToSqls | metamodel.plan_connection_sqls.text"
    ),
]];

/// The expression-tree node kinds (real m3 ValueSpecification subclasses the rows discriminate on): {class FQN, kind
/// spelling, set id, own property lines}.
const VS_KINDS: [[&str; 4]; 3] = [
    [
        "meta::pure::metamodel::valuespecification::FunctionExpression",
        "FunctionExpression",
        "vsFe",
        concat!(
            "parametersValues[vsFe]: ",
            store_ref!(),
            "@VsToChildren,\n",
            "                    parametersValues[vsIv]: ",
            store_ref!(),
            "@VsToChildren,\n",
            "                    parametersValues[vsVe]: ",
            store_ref!(),
            "@VsToChildren"
        ),
    ],
    [
        "meta::pure::metamodel::valuespecification::InstanceValue",
        "InstanceValue",
        "vsIv",
        "",
    ],
    [
        "meta::pure::metamodel::valuespecification::VariableExpression",
        "VariableExpression",
        "vsVe",
        concat!("name: ", store_ref!(), " metamodel.value_specifications.var_name"),
    ],
];

/// The execution-activity kinds (real Activity subclasses: the relational activity carries the SQL the platform ran;
/// the aggregation-aware activity the routed query's print): {class FQN, kind spelling, set id, own property lines}.
/// An execute()'s activities are ROWS under the call's scope.
const ACTIVITY_KINDS: [[&str; 4]; 2] = [
    [
        "meta::relational::mapping::RelationalActivity",
        "RelationalActivity",
        "actRel",
        concat!(
            "sql: ",
            store_ref!(),
            " metamodel.activities.sql,\n",
            "                    comment: ",
            store_ref!(),
            " metamodel.activities.comment"
        ),
    ],
    [
        "meta::pure::mapping::aggregationAware::AggregationAwareActivity",
        "AggregationAwareActivity",
        "actAgg",
        concat!(
            "rewrittenQuery: ",
            store_ref!(),
            " metamodel.activities.rewritten_query"
        ),
    ],
];

fn filters() -> String {
    let mut sb = String::new();
    for k in DATA_TYPE_KINDS {
        push! { sb; "                Filter Dt", k, "(metamodel.data_types.dt_kind = '", k, "')\n" }
    }
    // the RelationalOperationElement hierarchy: one filter per kind over the ONE relational_elements table
    // (expression nodes, and the relations -- Table, View, Column, TableAlias)
    for k in &OP_KINDS {
        push! { sb; "                Filter Op", k[0], "(metamodel.relational_elements.kind = '", k[0], "')\n" }
    }
    for k in ELEMENT_KINDS {
        push! { sb; "                Filter El", k, "(metamodel.relational_elements.kind = '", k, "')\n" }
    }
    // the execution-plan node kinds over the ONE plan_nodes table
    for k in &PLAN_NODE_KINDS {
        push! { sb; "                Filter Pn", k[1], "(metamodel.plan_nodes.kind = '", k[1], "')\n" }
    }
    // the plan connection kinds and datasource-specification kinds over the ONE plan_connections table
    for k in &CONNECTION_KINDS {
        push! { sb; "                Filter Pc", k[1], "(metamodel.plan_connections.kind = '", k[1], "')\n" }
    }
    for k in &DATASOURCE_KINDS {
        push! { sb; "                Filter Ds", k[1], "(metamodel.plan_connections.ds_kind = '", k[1], "')\n" }
    }
    // the expression-tree node kinds over the ONE value_specifications table
    for k in &VS_KINDS {
        push! { sb; "                Filter Vs", k[1], "(metamodel.value_specifications.kind = '", k[1], "')\n" }
    }
    // the execution-activity kinds over the ONE activities table
    for k in &ACTIVITY_KINDS {
        push! { sb; "                Filter Act", k[1], "(metamodel.activities.kind = '", k[1], "')\n" }
    }
    // no line terminator after the last filter, as the source's other builders end: the template's own line ends it
    // (2026-11-12, the owner's ruling: the two made a blank line before the Database block's ")"; Java's too)
    sb.pop();
    sb
}

/// `prop[setId]: S@join` per set id, joined by `,\n` (Java's `String.join(",\n", lines)` over the kinds' lines).
fn routes<'a>(set_ids: impl Iterator<Item = &'a str>, prop: &str, join: &str) -> String {
    let lines: Vec<String> = set_ids
        .map(|id| parts! { "                    ", prop, "[", id, "]: ", S, "@", join })
        .collect();
    lines.join(",\n")
}

fn activity_routes(prop: &str, join: &str) -> String {
    routes(ACTIVITY_KINDS.iter().map(|k| k[2]), prop, join)
}

fn pk_assoc_routes() -> String {
    let mut lines = Vec::new();
    for k in &VS_KINDS {
        lines.push(parts! { "                        inferredFor[ipk, ", k[2], "]: ", S, "@NodeToPkColumns" });
        lines.push(
            parts! { "                        inferredPrimaryKeyColumns[", k[2], ", ipk]: ", S, "@NodeToPkColumns" },
        );
    }
    lines.join(",\n")
}

fn vs_routes(prop: &str, join: &str) -> String {
    routes(VS_KINDS.iter().map(|k| k[2]), prop, join)
}

fn plan_node_routes(prop: &str, join: &str) -> String {
    routes(PLAN_NODE_KINDS.iter().map(|k| k[2]), prop, join)
}

fn plan_sets() -> String {
    let mut sb = String::new();
    push! { sb;
        "                *meta::pure::executionPlan::ExecutionPlan[plan]: Relational\n",
        "                {\n",
        "                    ~primaryKey(", S, " metamodel.plans.id)\n",
        "                    ~mainTable ", S, " metamodel.plans\n",
        "                    processingTemplateFunctions: ", S,
        "@PlanToTemplateFunctions | metamodel.plan_template_functions.text,\n",
        &plan_node_routes("rootExecutionNode", "PlanToRoot"), "\n",
        "                }\n",
        "                *meta::pure::executionPlan::ExecutionNode: Operation\n",
        "                {\n",
        "                    ", INHERITANCE_OP, "\n",
        "                }\n",
    }
    for k in &PLAN_NODE_KINDS {
        push! { sb;
            "                ", k[0], "[", k[2], "]: Relational\n",
            "                {\n",
            "                    ~filter ", S, " Pn", k[1], "\n",
            "                    ~primaryKey(", S, " metamodel.plan_nodes.id)\n",
            "                    ~mainTable ", S, " metamodel.plan_nodes\n",
            &if k[3].is_empty() { String::new() } else { parts! { "                    ", k[3], ",\n" } },
            &plan_node_routes("executionNodes", "NodeToChildren"), "\n",
            "                }\n",
        }
    }
    // the plan's FunctionParameter rows and the node-tree CLOSURE (the engine's recursive allNodes walk as a row
    // entity: every ancestor/descendant pair, self at depth 0 -- a query never recurses over the tree)
    push! { sb;
        "                *meta::pure::executionPlan::FunctionParameter[planParam]: Relational\n",
        "                {\n",
        "                    ~primaryKey(", S, " metamodel.plan_function_parameters.node_id, ",
        S, " metamodel.plan_function_parameters.ordinal)\n",
        "                    ~mainTable ", S, " metamodel.plan_function_parameters\n",
        "                    name: ", S, " metamodel.plan_function_parameters.name,\n",
        "                    supportsStream: ", S, " metamodel.plan_function_parameters.supports_stream\n",
        "                }\n",
        "                *meta::lite::metamodel::PlanNodeClosure[pnc]: Relational\n",
        "                {\n",
        "                    ~primaryKey(", S, " metamodel.plan_node_closure.ancestor_id, ",
        S, " metamodel.plan_node_closure.node_id)\n",
        "                    ~mainTable ", S, " metamodel.plan_node_closure\n",
        "                    depth: ", S, " metamodel.plan_node_closure.depth\n",
        "                }\n",
    }
    // the plan connections (SQLExecutionNode.connection) and their datasource specifications: rows of
    // plan_connections under the node's id; the declared bases are inheritance operations
    push! { sb;
        "                *meta::external::store::relational::runtime::DatabaseConnection: Operation\n",
        "                {\n",
        "                    ", INHERITANCE_OP, "\n",
        "                }\n",
    }
    for k in &CONNECTION_KINDS {
        push! { sb;
            "                ", k[0], "[", k[2], "]: Relational\n",
            "                {\n",
            "                    ~filter ", S, " Pc", k[1], "\n",
            "                    ~primaryKey(", S, " metamodel.plan_connections.node_id)\n",
            "                    ~mainTable ", S, " metamodel.plan_connections\n",
            "                    ", k[3], "\n",
            "                }\n",
        }
    }
    push! { sb;
        "                *meta::pure::alloy::connections::alloy::specification::DatasourceSpecification: Operation\n",
        "                {\n",
        "                    ", INHERITANCE_OP, "\n",
        "                }\n",
    }
    for k in &DATASOURCE_KINDS {
        push! { sb;
            "                ", k[0], "[", k[2], "]: Relational\n",
            "                {\n",
            "                    ~filter ", S, " Ds", k[1], "\n",
            "                    ~primaryKey(", S, " metamodel.plan_connections.node_id)\n",
            "                    ~mainTable ", S, " metamodel.plan_connections\n",
            "                    ", k[3], "\n",
            "                }\n",
        }
    }
    let mut subtrees = Vec::new();
    let mut closure_nodes = Vec::new();
    for k in &PLAN_NODE_KINDS {
        subtrees.push(parts! { "                        subtree[", k[2], ", pnc]: ", S, "@NodeToSubtree" });
        subtrees.push(parts! { "                        ancestor[pnc, ", k[2], "]: ", S, "@NodeToSubtree" });
        closure_nodes.push(parts! { "                        node[pnc, ", k[2], "]: ", S, "@SubtreeToNode" });
        closure_nodes.push(parts! { "                        closureOf[", k[2], ", pnc]: ", S, "@SubtreeToNode" });
    }
    // FUNCTION BODIES as rows (group A burn, 2026-09-03): a lambda / function value's expressionSequence is its
    // statements as ValueSpecification rows, each stamped with the compiler's inferred primary key (the engine's
    // inferPrimaryKeyColumnNames rules over the typed tree); the rows ride the query
    push! { sb;
        "                *meta::pure::metamodel::function::FunctionDefinition[fn]: Relational\n",
        "                {\n",
        "                    ~primaryKey(", S, " metamodel.functions.id)\n",
        "                    ~mainTable ", S, " metamodel.functions\n",
        &vs_routes("expressionSequence", "FunctionToBody"), "\n",
        "                }\n",
        // the expression TREE (group H burn, 2026-09-03): every node is a row discriminated by its m3 kind;
        // parametersValues are the children rows; the node's Multiplicity is the real m3 object shape
        // (Multiplicity -> MultiplicityValue.value) over the same row
        "                *meta::pure::metamodel::valuespecification::ValueSpecification: Operation\n",
        "                {\n",
        "                    ", INHERITANCE_OP, "\n",
        "                }\n",
    }
    for k in &VS_KINDS {
        push! { sb;
            "                ", k[0], "[", k[2], "]: Relational\n",
            "                {\n",
            "                    ~filter ", S, " Vs", k[1], "\n",
            "                    ~primaryKey(", S, " metamodel.value_specifications.id)\n",
            "                    ~mainTable ", S, " metamodel.value_specifications\n",
            "                    multiplicity[mult]: ", S, "@VsSelf",
            &if k[3].is_empty() { "\n".to_owned() } else { parts! { ",\n                    ", k[3], "\n" } },
            "                }\n",
        }
    }
    push! { sb;
        "                *meta::pure::metamodel::multiplicity::Multiplicity[mult]: Relational\n",
        "                {\n",
        "                    ~primaryKey(", S, " metamodel.value_specifications.id)\n",
        "                    ~mainTable ", S, " metamodel.value_specifications\n",
        "                    lowerBound[mvLo]: ", S, "@VsSelf,\n",
        "                    upperBound[mvHi]: ", S, "@VsSelf\n",
        "                }\n",
        "                *meta::pure::metamodel::multiplicity::MultiplicityValue[mvLo]: Relational\n",
        "                {\n",
        "                    ~primaryKey(", S, " metamodel.value_specifications.id)\n",
        "                    ~mainTable ", S, " metamodel.value_specifications\n",
        "                    value: ", S, " metamodel.value_specifications.mult_lower\n",
        "                }\n",
        "                meta::pure::metamodel::multiplicity::MultiplicityValue[mvHi]: Relational\n",
        "                {\n",
        "                    ~primaryKey(", S, " metamodel.value_specifications.id)\n",
        "                    ~mainTable ", S, " metamodel.value_specifications\n",
        "                    value: ", S, " metamodel.value_specifications.mult_upper\n",
        "                }\n",
        // EXECUTION ACTIVITIES as rows (2026-09-03): an execute()'s Result is a row keyed by the call's scope; its
        // activities are the kinds' rows (Operation set over the one table)
        "                *meta::pure::mapping::Result[res]: Relational\n",
        "                {\n",
        "                    ~primaryKey(", S, " metamodel.results.id)\n",
        "                    ~mainTable ", S, " metamodel.results\n",
        &activity_routes("activities", "ResultToActivities"), "\n",
        "                }\n",
        "                *meta::pure::mapping::Activity: Operation\n",
        "                {\n",
        "                    ", INHERITANCE_OP, "\n",
        "                }\n",
    }
    for k in &ACTIVITY_KINDS {
        push! { sb;
            "                ", k[0], "[", k[2], "]: Relational\n",
            "                {\n",
            "                    ~filter ", S, " Act", k[1], "\n",
            "                    ~primaryKey(", S, " metamodel.activities.id)\n",
            "                    ~mainTable ", S, " metamodel.activities\n",
            "                    ", k[3], "\n",
            "                }\n",
        }
    }
    push! { sb;
        "                *meta::lite::metamodel::InferredPrimaryKeyColumn[ipk]: Relational\n",
        "                {\n",
        "                    ~primaryKey(", S, " metamodel.vs_primary_key_columns.node_id, ",
        S, " metamodel.vs_primary_key_columns.ordinal)\n",
        "                    ~mainTable ", S, " metamodel.vs_primary_key_columns\n",
        "                    ordinal: ", S, " metamodel.vs_primary_key_columns.ordinal,\n",
        "                    name: ", S, " metamodel.vs_primary_key_columns.name\n",
        "                }\n",
        "                meta::lite::metamodel::InferredPrimaryKeys: Relational\n",
        "                {\n                    AssociationMapping\n                    (\n",
        &pk_assoc_routes(), "\n",
        "                    )\n                }\n",
    }
    // LINEAGE TREES as rows (group E burn, 2026-09-03): a scanRelations handle's relation tree is node rows in
    // PREORDER with their depth's indent, kind (root / t / v), name, join label and columns; relationTreeAsString is
    // a Pure body over them
    push! { sb;
        "                *meta::pure::lineage::scanRelations::RelationTree[rtree]: Relational\n",
        "                {\n",
        "                    ~primaryKey(", S, " metamodel.relation_trees.id)\n",
        "                    ~mainTable ", S, " metamodel.relation_trees\n",
        "                }\n",
        "                *meta::lite::metamodel::RelationTreeNode[rtnode]: Relational\n",
        "                {\n",
        // node_id, not id: the aggregated columns hop keys on the NODE row -- a key spelled `id` collides with the
        // tree row's `id` in the aggregated-navigation key resolution (a named resolver debt, harness burn-down
        // batch 20)
        "                    ~primaryKey(", S, " metamodel.relation_tree_nodes.node_id)\n",
        "                    ~mainTable ", S, " metamodel.relation_tree_nodes\n",
        "                    preorder: ", S, " metamodel.relation_tree_nodes.preorder,\n",
        "                    indent: ", S, " metamodel.relation_tree_nodes.indent,\n",
        "                    kind: ", S, " metamodel.relation_tree_nodes.kind,\n",
        "                    name: ", S, " metamodel.relation_tree_nodes.name,\n",
        "                    joinLabel: ", S, " metamodel.relation_tree_nodes.join_label\n",
        "                }\n",
        "                *meta::lite::metamodel::RelationTreeColumn[rtcol]: Relational\n",
        "                {\n",
        "                    ~primaryKey(", S, " metamodel.relation_tree_node_columns.node_id, ",
        S, " metamodel.relation_tree_node_columns.ordinal)\n",
        "                    ~mainTable ", S, " metamodel.relation_tree_node_columns\n",
        "                    ordinal: ", S, " metamodel.relation_tree_node_columns.ordinal,\n",
        "                    name: ", S, " metamodel.relation_tree_node_columns.name\n",
        "                }\n",
        "                meta::lite::metamodel::RelationTreeNodes: Relational\n",
        "                {\n                    AssociationMapping\n                    (\n",
        "                        tree[rtnode, rtree]: ", S, "@TreeToNodes,\n",
        "                        nodes[rtree, rtnode]: ", S, "@TreeToNodes\n",
        "                    )\n                }\n",
        "                meta::lite::metamodel::RelationTreeNodeColumns: Relational\n",
        "                {\n                    AssociationMapping\n                    (\n",
        "                        node[rtcol, rtnode]: ", S, "@TreeNodeToColumns,\n",
        "                        columns[rtnode, rtcol]: ", S, "@TreeNodeToColumns\n",
        "                    )\n                }\n",
    }
    // COLUMN LINEAGE as rows (group I burn, 2026-09-03): a scanColumns handle's ColumnWithContext rows -- the columns
    // the lowered plan reads, joined to the store's own Column rows by (db, schema, table, name); the scan id is the
    // handle's key (one handle, many rows -- the ~primaryKey names the key the extent filters on)
    push! { sb;
        "                *meta::pure::lineage::scanColumns::ColumnWithContext[cwc]: Relational\n",
        "                {\n",
        "                    ~primaryKey(", S, " metamodel.column_contexts.scan_id)\n",
        "                    ~mainTable ", S, " metamodel.column_contexts\n",
        "                    context: ", S, " metamodel.column_contexts.context,\n",
        "                    column[col]: ", S, "@ColumnContextToColumn\n",
        "                }\n",
    }
    push! { sb;
        "                meta::lite::metamodel::PlanNodeSubtrees: Relational\n",
        "                {\n                    AssociationMapping\n                    (\n",
        &subtrees.join(",\n"), "\n",
        "                    )\n                }\n",
        "                meta::lite::metamodel::PlanNodeClosureNodes: Relational\n",
        "                {\n                    AssociationMapping\n                    (\n",
        &closure_nodes.join(",\n"), "\n",
        "                    )\n                }\n",
    }
    sb
}

fn data_type_sets() -> String {
    let mut sb = String::new();
    for k in DATA_TYPE_KINDS {
        let sized = k == "Varchar" || k == "Char" || k == "Binary" || k == "Varbinary";
        let scaled = k == "Decimal" || k == "Numeric";
        let props = if sized {
            parts! { "\n                    size: ", S, " metamodel.data_types.type_size" }
        } else if scaled {
            parts! {
                "\n                    precision: ", S, " metamodel.data_types.type_precision,",
                "\n                    scale: ", S, " metamodel.data_types.type_scale"
            }
        } else {
            String::new()
        };
        push! { sb;
            "                meta::relational::metamodel::datatype::", k, "[dt", k, "]: Relational\n",
            "                {\n",
            "                    ~filter ", S, " Dt", k, "\n",
            "                    ~primaryKey(", S, " metamodel.data_types.id)\n",
            "                    ~mainTable ", S, " metamodel.data_types", &props, "\n",
            "                }\n",
        }
    }
    sb
}

fn op_sets() -> String {
    let mut sb = String::new();
    for k in &OP_KINDS {
        push! { sb;
            "                meta::relational::metamodel::", k[0], "[", k[1], "]: Relational\n",
            "                {\n",
            "                    ~filter ", S, " Op", k[0], "\n",
            "                    ~primaryKey(", S, " metamodel.relational_elements.id)\n",
            "                    ~mainTable ", S, " metamodel.relational_elements",
            &if k[2].is_empty() { String::new() } else { parts! { "\n                    ", k[2] } }, "\n",
            "                }\n",
        }
    }
    sb
}

/// `prop[opX]: @Join` per operation kind -- a property typed RelationalOperationElement routes to every member set.
fn op_routes(prop: &str, join: &str) -> String {
    routes(OP_KINDS.iter().map(|k| k[1]), prop, join)
}

fn type_routes() -> String {
    let lines: Vec<String> = DATA_TYPE_KINDS
        .iter()
        .map(|&k| parts! { "                    type[dt", k, "]: ", S, "@ColumnToType" })
        .collect();
    lines.join(",\n")
}

fn inferred_type_ends() -> String {
    let mut lines = Vec::new();
    for op in &OP_KINDS {
        for k in DATA_TYPE_KINDS {
            lines.push(parts! { "                        inferredType[", op[1], ", dt", k, "]: ", S, "@OpToType" });
            lines.push(parts! { "                        inferredTypeOf[dt", k, ", ", op[1], "]: ", S, "@OpToType" });
        }
    }
    lines.join(",\n")
}

/// Java's `String.formatted(args)` over the specifiers the template holds: `%N$s` (the N-th argument, N from 1) and
/// `%%` (one `%`), read left to right. Any other specifier is the port's bug, loud (Java's
/// `UnknownFormatConversionException` / `MissingFormatArgumentException`): no specifier is guessed at.
fn formatted(template: &str, args: &[&str]) -> String {
    let mut out = String::with_capacity(template.len() * 2);
    let mut rest = template;
    while let Some(at) = rest.find('%') {
        out.push_str(&rest[..at]);
        let spec = &rest[at + 1..];
        if let Some(after) = spec.strip_prefix('%') {
            out.push('%');
            rest = after;
            continue;
        }
        let digits = spec.bytes().take_while(u8::is_ascii_digit).count();
        let index = spec[..digits]
            .parse::<usize>()
            .ok()
            .filter(|n| (1..=args.len()).contains(n));
        match (index, spec[digits..].strip_prefix("$s")) {
            (Some(n), Some(after)) => {
                out.push_str(args[n - 1]);
                rest = after;
            }
            _ => panic!(
                "the system metamodel's template holds a specifier the port does not format: %{}",
                spec.chars().take(8).collect::<String>()
            ),
        }
    }
    out.push_str(rest);
    out
}

/// Schema (charter §4 + step 3); grow BY WITNESS ONLY. The class's text block as Java's compiler makes its value: the
/// incidental indentation (twelve spaces) and trailing spaces stripped, and the closing delimiter's own line leaving one
/// final line feed. A Pure string's line break is Pure's escape, the two characters `\n` (written `\\n` here and in
/// Java's text block), as upstream's relationTreeAsString spells it (2026-11-12, the owner's ruling on slice 1b's
/// report: both sides wrote a raw line break inside the two `joinStrings` literals). `%N$s` are [`formatted`]'s
/// placeholders, `%%` its escaped `%`.
const TEMPLATE: &str = "\
###Relational
Database meta::lite::metamodel::MetamodelStore
(
    Schema metamodel
    (
        Table classes
        (
            fqn VARCHAR(1024) PRIMARY KEY,
            name VARCHAR(256) NOT NULL,
            package VARCHAR(1024) NOT NULL
        )
        Table mappings
        (
            fqn VARCHAR(1024) PRIMARY KEY,
            name VARCHAR(256) NOT NULL
        )
        Table enumeration_mappings
        (
            mapping_fqn VARCHAR(1024) PRIMARY KEY,
            name VARCHAR(256) PRIMARY KEY,
            enumeration_fqn VARCHAR(1024) NOT NULL,
            ordinal INTEGER NOT NULL
        )
        Table enum_value_mappings
        (
            mapping_fqn VARCHAR(1024) PRIMARY KEY,
            em_name VARCHAR(256) PRIMARY KEY,
            enum_value VARCHAR(256) PRIMARY KEY
        )
        Table enum_value_sources
        (
            mapping_fqn VARCHAR(1024) PRIMARY KEY,
            em_name VARCHAR(256) PRIMARY KEY,
            enum_value VARCHAR(256) PRIMARY KEY,
            source_value VARCHAR(1024) PRIMARY KEY
        )
        Table mapping_includes_closure
        (
            mapping_fqn VARCHAR(1024) PRIMARY KEY,
            included_fqn VARCHAR(1024) PRIMARY KEY,
            include_rank INTEGER NOT NULL
        )
        Table mapping_includes
        (
            mapping_fqn VARCHAR(1024) PRIMARY KEY,
            included_fqn VARCHAR(1024) PRIMARY KEY,
            ordinal INTEGER NOT NULL
        )
        Table mapping_store_resolutions
        (
            mapping_fqn VARCHAR(1024) PRIMARY KEY,
            original_fqn VARCHAR(1024) PRIMARY KEY,
            resolved_fqn VARCHAR(1024) NOT NULL
        )
        Table class_mappings
        (
            mapping_fqn VARCHAR(1024) PRIMARY KEY,
            id VARCHAR(256) PRIMARY KEY,
            mapped_class_fqn VARCHAR(1024) NOT NULL,
            super_set_id VARCHAR(256),
            main_db VARCHAR(1024),
            main_schema VARCHAR(256),
            main_table VARCHAR(256),
            distinct_set BIT,
            user_defined_pk BIT NOT NULL,
            root BIT NOT NULL
        )
        Table set_ancestry
        (
            mapping_fqn VARCHAR(1024) PRIMARY KEY,
            id VARCHAR(256) PRIMARY KEY,
            super_mapping_fqn VARCHAR(1024) PRIMARY KEY,
            super_id VARCHAR(256) PRIMARY KEY,
            depth INTEGER NOT NULL
        )
        Table group_by_mappings
        (
            mapping_fqn VARCHAR(1024) PRIMARY KEY,
            id VARCHAR(256) PRIMARY KEY
        )
        Table databases
        (
            fqn VARCHAR(1024) PRIMARY KEY,
            name VARCHAR(256) NOT NULL
        )
        Table schemas
        (
            db_fqn VARCHAR(1024) PRIMARY KEY,
            name VARCHAR(256) PRIMARY KEY
        )
        Table properties
        (
            owner_fqn VARCHAR(1024) PRIMARY KEY,
            name VARCHAR(256) PRIMARY KEY
        )
        Table data_types
        (
            id VARCHAR(2048) PRIMARY KEY,
            dt_kind VARCHAR(32) NOT NULL,
            type_size INTEGER,
            type_precision INTEGER,
            type_scale INTEGER
        )
        Table relational_elements
        (
            id VARCHAR(2048) PRIMARY KEY,
            kind VARCHAR(64) NOT NULL,
            name VARCHAR(256),
            db_fqn VARCHAR(1024),
            schema_name VARCHAR(256),
            table_name VARCHAR(256),
            dtype_id VARCHAR(2048),
            parent_id VARCHAR(2048),
            ordinal INTEGER,
            dyna_name VARCHAR(256),
            literal_value VARCHAR(4000),
            col_element_id VARCHAR(2048),
            col_name VARCHAR(256),
            itype_id VARCHAR(2048),
            pk_mapping_fqn VARCHAR(1024),
            pk_set_id VARCHAR(256),
            mapping_fqn VARCHAR(1024),
            set_id VARCHAR(256),
            main_element_id VARCHAR(2048),
            view_element_id VARCHAR(2048),
            base_element_id VARCHAR(2048)
        )
        Table view_column_mappings
        (
            db_fqn VARCHAR(1024) PRIMARY KEY,
            schema_name VARCHAR(256) PRIMARY KEY,
            view_name VARCHAR(256) PRIMARY KEY,
            column_name VARCHAR(256) PRIMARY KEY,
            op_id VARCHAR(2048) NOT NULL
        )
        Table property_mappings
        (
            mapping_fqn VARCHAR(1024) PRIMARY KEY,
            id VARCHAR(256) PRIMARY KEY,
            ordinal INTEGER PRIMARY KEY,
            prop_owner_fqn VARCHAR(1024),
            prop_name VARCHAR(256) NOT NULL,
            op_id VARCHAR(2048) NOT NULL,
            declared_depth INTEGER NOT NULL
        )
        Table plans
        (
            id VARCHAR(256) PRIMARY KEY,
            root_node_id VARCHAR(512) NOT NULL
        )
        Table plan_nodes
        (
            id VARCHAR(512) PRIMARY KEY,
            plan_id VARCHAR(256) NOT NULL,
            parent_id VARCHAR(512),
            ordinal INTEGER NOT NULL,
            kind VARCHAR(64) NOT NULL,
            sql_query VARCHAR(65535),
            sql_comment VARCHAR(1024)
        )
        Table plan_template_functions
        (
            plan_id VARCHAR(256) PRIMARY KEY,
            ordinal INTEGER PRIMARY KEY,
            text VARCHAR(65535) NOT NULL
        )
        Table plan_function_parameters
        (
            node_id VARCHAR(512) PRIMARY KEY,
            ordinal INTEGER PRIMARY KEY,
            name VARCHAR(256) NOT NULL,
            supports_stream BIT
        )
        Table plan_node_closure
        (
            ancestor_id VARCHAR(512) PRIMARY KEY,
            node_id VARCHAR(512) PRIMARY KEY,
            depth INTEGER NOT NULL
        )
        Table plan_connections
        (
            node_id VARCHAR(512) PRIMARY KEY,
            kind VARCHAR(64) NOT NULL,
            db_type VARCHAR(64) NOT NULL,
            test_data_setup_csv VARCHAR(65535),
            ds_kind VARCHAR(64),
            ds_test_data_setup_csv VARCHAR(65535)
        )
        Table plan_connection_sqls
        (
            node_id VARCHAR(512) PRIMARY KEY,
            owner VARCHAR(8) PRIMARY KEY,
            ordinal INTEGER PRIMARY KEY,
            text VARCHAR(65535) NOT NULL
        )
        Table functions
        (
            id VARCHAR(512) PRIMARY KEY,
            name VARCHAR(512) NOT NULL
        )
        Table value_specifications
        (
            id VARCHAR(512) PRIMARY KEY,
            function_id VARCHAR(512) NOT NULL,
            ordinal INTEGER NOT NULL,
            kind VARCHAR(64) NOT NULL,
            parent_id VARCHAR(512),
            depth INTEGER NOT NULL,
            mult_lower INTEGER NOT NULL,
            mult_upper INTEGER,
            var_name VARCHAR(256) NOT NULL
        )
        Table vs_primary_key_columns
        (
            node_id VARCHAR(512) PRIMARY KEY,
            ordinal INTEGER PRIMARY KEY,
            name VARCHAR(256) NOT NULL
        )
        Table relation_trees
        (
            id VARCHAR(512) PRIMARY KEY
        )
        Table relation_tree_nodes
        (
            node_id VARCHAR(512) PRIMARY KEY,
            tree_id VARCHAR(512) NOT NULL,
            preorder INTEGER NOT NULL,
            indent VARCHAR(256) NOT NULL,
            kind VARCHAR(8) NOT NULL,
            name VARCHAR(512),
            join_label VARCHAR(512)
        )
        Table relation_tree_node_columns
        (
            node_id VARCHAR(512) PRIMARY KEY,
            ordinal INTEGER PRIMARY KEY,
            name VARCHAR(256) NOT NULL
        )
        Table column_contexts
        (
            scan_id VARCHAR(512) PRIMARY KEY,
            ordinal INTEGER PRIMARY KEY,
            db_fqn VARCHAR(1024) NOT NULL,
            schema_name VARCHAR(256) NOT NULL,
            table_name VARCHAR(256) NOT NULL,
            read_column VARCHAR(256) NOT NULL,
            context VARCHAR(64) NOT NULL
        )
        Table results
        (
            id VARCHAR(512) PRIMARY KEY
        )
        Table activities
        (
            id VARCHAR(512) PRIMARY KEY,
            result_id VARCHAR(512) NOT NULL,
            ordinal INTEGER NOT NULL,
            kind VARCHAR(64) NOT NULL,
            sql VARCHAR(65535) NOT NULL,
            comment VARCHAR(1024),
            rewritten_query VARCHAR(65535) NOT NULL
        )
    )
    Join ResultToActivities(metamodel.results.id = metamodel.activities.result_id)
    Join PlanToRoot(metamodel.plans.root_node_id = metamodel.plan_nodes.id)
    Join NodeToChildren(metamodel.plan_nodes.id = {target}.parent_id)
    Join PlanToTemplateFunctions(metamodel.plans.id = metamodel.plan_template_functions.plan_id)
    Join NodeToFunctionParameters(metamodel.plan_nodes.id = metamodel.plan_function_parameters.node_id)
    Join NodeToSubtree(metamodel.plan_nodes.id = metamodel.plan_node_closure.ancestor_id)
    Join SubtreeToNode(metamodel.plan_node_closure.node_id = metamodel.plan_nodes.id)
    Join NodeToConnection(metamodel.plan_nodes.id = metamodel.plan_connections.node_id)
    Join ConnToDatasource(metamodel.plan_connections.node_id = {target}.node_id)
    Join ConnToSqls(metamodel.plan_connections.node_id = metamodel.plan_connection_sqls.node_id and metamodel.plan_connection_sqls.owner = 'conn')
    Join DatasourceToSqls(metamodel.plan_connections.node_id = metamodel.plan_connection_sqls.node_id and metamodel.plan_connection_sqls.owner = 'ds')
    Join FunctionToBody(metamodel.functions.id = metamodel.value_specifications.function_id and metamodel.value_specifications.depth = 0)
    Join VsToChildren(metamodel.value_specifications.id = {target}.parent_id)
    Join VsSelf(metamodel.value_specifications.id = {target}.id)
    Join NodeToPkColumns(metamodel.value_specifications.id = metamodel.vs_primary_key_columns.node_id)
    Join TreeToNodes(metamodel.relation_trees.id = metamodel.relation_tree_nodes.tree_id)
    Join TreeNodeToColumns(metamodel.relation_tree_nodes.node_id = metamodel.relation_tree_node_columns.node_id)
    Join ColumnContextToColumn(metamodel.column_contexts.db_fqn = metamodel.relational_elements.db_fqn
        and metamodel.column_contexts.schema_name = metamodel.relational_elements.schema_name
        and metamodel.column_contexts.table_name = metamodel.relational_elements.table_name
        and metamodel.column_contexts.read_column = metamodel.relational_elements.name)
    Join ColumnToOwnerTable(metamodel.relational_elements.db_fqn = {target}.db_fqn
        and metamodel.relational_elements.schema_name = {target}.schema_name
        and metamodel.relational_elements.table_name = {target}.name)
    Join MappingsToClosure(metamodel.mappings.fqn = metamodel.mapping_includes_closure.mapping_fqn)
    Join ClosureToVisible(metamodel.mapping_includes_closure.included_fqn = metamodel.mappings.fqn)
    Join MappingsToIncludes(metamodel.mappings.fqn = metamodel.mapping_includes.mapping_fqn)
    Join IncludeToIncluded(metamodel.mapping_includes.included_fqn = metamodel.mappings.fqn)
    Join ClassMappingsToMappings(metamodel.class_mappings.mapping_fqn = metamodel.mappings.fqn)
    Join EnumerationMappingsToMappings(metamodel.enumeration_mappings.mapping_fqn = metamodel.mappings.fqn)
    Join EnumerationMappingToValues(metamodel.enumeration_mappings.mapping_fqn = metamodel.enum_value_mappings.mapping_fqn
        and metamodel.enumeration_mappings.name = metamodel.enum_value_mappings.em_name)
    Join EnumValueToSources(metamodel.enum_value_mappings.mapping_fqn = metamodel.enum_value_sources.mapping_fqn
        and metamodel.enum_value_mappings.em_name = metamodel.enum_value_sources.em_name
        and metamodel.enum_value_mappings.enum_value = metamodel.enum_value_sources.enum_value)
    Join ClosureToClassMappings(metamodel.mapping_includes_closure.included_fqn = metamodel.class_mappings.mapping_fqn)
    Join MappingsToStoreResolutions(metamodel.mappings.fqn = metamodel.mapping_store_resolutions.mapping_fqn)
    Join ResolutionToOriginal(metamodel.mapping_store_resolutions.original_fqn = metamodel.databases.fqn)
    Join ResolutionToResolved(metamodel.mapping_store_resolutions.resolved_fqn = metamodel.databases.fqn)
    Join AliasToDatabase(metamodel.relational_elements.db_fqn = metamodel.databases.fqn)
    Join ClassMappingsToAlias(metamodel.class_mappings.mapping_fqn = metamodel.relational_elements.mapping_fqn and metamodel.class_mappings.id = metamodel.relational_elements.set_id)
    Join AliasToTables(metamodel.relational_elements.main_element_id = {target}.id)
    Join AliasToViews(metamodel.relational_elements.main_element_id = {target}.id)
    Join AliasToBaseTable(metamodel.relational_elements.base_element_id = {target}.id)
    Join ViewToAlias(metamodel.relational_elements.id = {target}.view_element_id)
    Join SetToAncestry(metamodel.class_mappings.mapping_fqn = metamodel.set_ancestry.mapping_fqn
        and metamodel.class_mappings.id = metamodel.set_ancestry.id)
    Join AncestryToAncestor(metamodel.set_ancestry.super_mapping_fqn = metamodel.class_mappings.mapping_fqn
        and metamodel.set_ancestry.super_id = metamodel.class_mappings.id)
    Join SetToGroupBy(metamodel.class_mappings.mapping_fqn = metamodel.group_by_mappings.mapping_fqn
        and metamodel.class_mappings.id = metamodel.group_by_mappings.id)
    Join SetToPrimaryKeyOps(metamodel.class_mappings.mapping_fqn = metamodel.relational_elements.pk_mapping_fqn
        and metamodel.class_mappings.id = metamodel.relational_elements.pk_set_id)
    Join DbToSchemas(metamodel.databases.fqn = metamodel.schemas.db_fqn)
    Join SchemaToViews(metamodel.schemas.db_fqn = metamodel.relational_elements.db_fqn
        and metamodel.schemas.name = metamodel.relational_elements.schema_name)
    Join SchemaToTables(metamodel.schemas.db_fqn = metamodel.relational_elements.db_fqn
        and metamodel.schemas.name = metamodel.relational_elements.schema_name)
    Join TableToColumns(metamodel.relational_elements.db_fqn = {target}.db_fqn
        and metamodel.relational_elements.schema_name = {target}.schema_name
        and metamodel.relational_elements.name = {target}.table_name)
    Join ViewToColumnMappings(metamodel.relational_elements.db_fqn = metamodel.view_column_mappings.db_fqn
        and metamodel.relational_elements.schema_name = metamodel.view_column_mappings.schema_name
        and metamodel.relational_elements.name = metamodel.view_column_mappings.view_name)
    Join ColumnMappingToOp(metamodel.view_column_mappings.op_id = metamodel.relational_elements.id)
    Join ClassMappingsToClass(metamodel.class_mappings.mapped_class_fqn = metamodel.classes.fqn)
    Join SetToPropertyMappings(metamodel.class_mappings.mapping_fqn = metamodel.property_mappings.mapping_fqn
        and metamodel.class_mappings.id = metamodel.property_mappings.id)
    Join PropertyMappingToOp(metamodel.property_mappings.op_id = metamodel.relational_elements.id)
    Join PropertyMappingToProperty(metamodel.property_mappings.prop_owner_fqn = metamodel.properties.owner_fqn
        and metamodel.property_mappings.prop_name = metamodel.properties.name)
    Join ColumnToType(metamodel.relational_elements.dtype_id = metamodel.data_types.id)
    Join OpToType(metamodel.relational_elements.itype_id = metamodel.data_types.id)
    Join OpToColumn(metamodel.relational_elements.col_element_id = {target}.id)
%3$s
)

###Pure
// A time of day: lite's extension of the store grammar (RelationalDataType.Time, the Cypher island's stored
// times of day; legend-engine's grammar has no TIME column type, docs/SEMANTICS_REGISTER.md S64). Declared
// here, the platform's own elements, not in the generated prelude (2026-10-17, the spec guards' repair).
Class meta::relational::metamodel::datatype::Time extends meta::relational::metamodel::datatype::CoreDataType
{
}

// A date-time of nanosecond precision: lite's extension of the store grammar (RelationalDataType.TimestampNs,
// the Cypher island's stored nanosecond times and date-times; legend-engine's grammar has no TIMESTAMP_NS).
Class meta::relational::metamodel::datatype::TimestampNs extends meta::relational::metamodel::datatype::CoreDataType
{
}

Class meta::lite::metamodel::MappingVisibility
{
    includeRank: Integer[1];
}

Association meta::lite::metamodel::MappingVisibilities
{
    viewer: meta::pure::mapping::Mapping[1];
    visibility: meta::lite::metamodel::MappingVisibility[*];
}

Association meta::lite::metamodel::VisibleMappings
{
    visible: meta::pure::mapping::Mapping[1];
    visibleFrom: meta::lite::metamodel::MappingVisibility[*];
}

Association meta::lite::metamodel::VisibleSets
{
    visibilityOf: meta::lite::metamodel::MappingVisibility[*];
    visibleSets: meta::relational::mapping::RootRelationalInstanceSetImplementation[*];
}

Class meta::lite::metamodel::StoreResolution
{
}

Association meta::lite::metamodel::MappingStoreResolutions
{
    resolver: meta::pure::mapping::Mapping[1];
    storeResolutions: meta::lite::metamodel::StoreResolution[*];
}

Association meta::lite::metamodel::StoreResolutionOriginal
{
    original: meta::pure::store::Store[1];
    originalOf: meta::lite::metamodel::StoreResolution[*];
}

Association meta::lite::metamodel::StoreResolutionResolved
{
    resolved: meta::pure::store::Store[1];
    resolvedOf: meta::lite::metamodel::StoreResolution[*];
}

Association meta::lite::metamodel::AliasBaseTables
{
    base: meta::relational::metamodel::relation::Table[1];
    baseOf: meta::relational::metamodel::TableAlias[*];
}

Class meta::lite::metamodel::PlanNodeClosure
{
    depth: Integer[1];
}

Association meta::lite::metamodel::PlanNodeSubtrees
{
    ancestor: meta::pure::executionPlan::ExecutionNode[1];
    subtree: meta::lite::metamodel::PlanNodeClosure[*];
}

Association meta::lite::metamodel::PlanNodeClosureNodes
{
    node: meta::pure::executionPlan::ExecutionNode[1];
    closureOf: meta::lite::metamodel::PlanNodeClosure[*];
}

Class meta::lite::metamodel::InferredPrimaryKeyColumn
{
    ordinal: Integer[1];
    name: String[1];
}

Association meta::lite::metamodel::InferredPrimaryKeys
{
    inferredFor: meta::pure::metamodel::valuespecification::ValueSpecification[1];
    inferredPrimaryKeyColumns: meta::lite::metamodel::InferredPrimaryKeyColumn[*];
}

Class meta::lite::metamodel::RelationTreeNode
{
    preorder: Integer[1];
    indent: String[1];
    kind: String[1];
    name: String[0..1];
    joinLabel: String[0..1];
}

Class meta::lite::metamodel::RelationTreeColumn
{
    ordinal: Integer[1];
    name: String[1];
}

Association meta::lite::metamodel::RelationTreeNodes
{
    tree: meta::pure::lineage::scanRelations::RelationTree[1];
    nodes: meta::lite::metamodel::RelationTreeNode[*];
}

Association meta::lite::metamodel::RelationTreeNodeColumns
{
    node: meta::lite::metamodel::RelationTreeNode[1];
    columns: meta::lite::metamodel::RelationTreeColumn[*];
}

Class meta::lite::metamodel::SetAncestry
{
    depth: Integer[1];
}

Association meta::lite::metamodel::SetAncestries
{
    descendant: meta::relational::mapping::RootRelationalInstanceSetImplementation[1];
    ancestry: meta::lite::metamodel::SetAncestry[*];
}

Class meta::lite::metamodel::EnumSourceValue
{
    value: String[1];
}

Association meta::lite::metamodel::EnumValueSources
{
    sources: meta::lite::metamodel::EnumSourceValue[*];
    ofValueMapping: meta::pure::mapping::EnumValueMapping[1];
}

Association meta::lite::metamodel::SetAncestors
{
    ancestor: meta::relational::mapping::RootRelationalInstanceSetImplementation[1];
    ancestryOf: meta::lite::metamodel::SetAncestry[*];
}

Association meta::lite::metamodel::InferredTypes
{
    inferredType: meta::relational::metamodel::datatype::DataType[0..1];
    inferredTypeOf: meta::relational::metamodel::RelationalOperationElement[*];
}

Association meta::lite::metamodel::EffectivePropertyMappings
{
    effectivePropertyMappings: meta::relational::mapping::RelationalPropertyMapping[*];
    allPropertyMappingsOf: meta::relational::mapping::RootRelationalInstanceSetImplementation[1];
}

function meta::pure::mapping::_classMappingByClass(_this:meta::pure::mapping::Mapping[1], class:meta::pure::metamodel::type::Class<meta::pure::metamodel::type::Any>[1]):meta::pure::mapping::SetImplementation[*]
{
    $_this.visibility->sortBy(v|$v.includeRank).visible.classMappings->filter(cm|$cm.class == $class)
}

function meta::pure::mapping::rootClassMappingByClass(_this:meta::pure::mapping::Mapping[1], class:meta::pure::metamodel::type::Class<meta::pure::metamodel::type::Any>[1]):meta::pure::mapping::SetImplementation[0..1]
{
    $_this->meta::pure::mapping::_classMappingByClass($class)->filter(s|$s.root == true)->last()
}

function meta::pure::mapping::enumerationMappingByName(_this:meta::pure::mapping::Mapping[1], name:String[1]):meta::pure::mapping::EnumerationMapping<meta::pure::metamodel::type::Any>[0..1]
{
    $_this.visibility->sortBy(v|$v.includeRank).visible.enumerationMappings->filter(em|$em.name == $name)->first()
}

function meta::pure::mapping::toDomainValue<T>(_this:meta::pure::mapping::EnumerationMapping<T>[1], sourceValue:meta::pure::metamodel::type::Any[1]):meta::pure::metamodel::type::Any[1]
{
    $_this.enumValueMappings->filter(m|$m.sources.value->contains($sourceValue))->toOne().enum
}

function meta::relational::metamodel::schema(_this:meta::relational::metamodel::Database[1], name:String[1]):meta::relational::metamodel::Schema[0..1]
{
    $_this.schemas->filter(s|$s.name == $name)->first()
}

function meta::relational::metamodel::table(_this:meta::relational::metamodel::Schema[1], name:String[1]):meta::relational::metamodel::relation::Table[0..1]
{
    $_this.tables->filter(t|$t.name == $name)->first()
}

function meta::relational::metamodel::view(_this:meta::relational::metamodel::Schema[1], name:String[1]):meta::relational::metamodel::relation::View[0..1]
{
    $_this.views->filter(t|$t.name == $name)->first()
}

function meta::relational::metamodel::column(_this:meta::relational::metamodel::relation::Table[1], columnName:String[1]):meta::relational::metamodel::Column[0..1]
{
    $_this.columns->cast(@meta::relational::metamodel::Column)->filter(c|$c.name == $columnName)->first()
}

function meta::relational::metamodel::children(_this:meta::relational::metamodel::join::RelationalTreeNode[1]):meta::relational::metamodel::join::JoinTreeNode[*]
{
    $_this.childrenData->cast(@meta::relational::metamodel::join::JoinTreeNode)
}

function meta::relational::metamodel::childByJoinName(_this:meta::relational::metamodel::join::RelationalTreeNode[1], s:String[1]):meta::relational::metamodel::join::JoinTreeNode[0..1]
{
    $_this->children()->filter(c|$c.join.name == $s)->first()
}

function meta::pure::mapping::allPropertyMappings(_this:meta::pure::mapping::PropertyMappingsImplementation[1]):meta::pure::mapping::PropertyMapping[*]
{
    $_this->cast(@meta::relational::mapping::RootRelationalInstanceSetImplementation).effectivePropertyMappings
}

function meta::pure::mapping::_propertyMappingsByPropertyName(i:meta::pure::mapping::PropertyMappingsImplementation[1], s:String[1]):meta::pure::mapping::PropertyMapping[*]
{
    $i->meta::pure::mapping::allPropertyMappings()->filter(pm|$pm.property.name == $s)
}

function meta::pure::mapping::propertyMappingsByPropertyName(_this:meta::pure::mapping::PropertyMappingsImplementation[1], s:String[1]):meta::pure::mapping::PropertyMapping[*]
{
    $_this->meta::pure::mapping::allPropertyMappings()->filter(pm|$pm.property.name == $s)
}

function meta::pure::mapping::propertyMappingsByPropertyName(i:meta::pure::mapping::InstanceSetImplementation[1], propertyName:String[1]):meta::pure::mapping::PropertyMapping[*]
{
    $i->meta::pure::mapping::allPropertyMappings()->filter(pm|$pm.property.name == $propertyName)
}

function meta::relational::functions::typeInference::inferRelationalType(rop:meta::relational::metamodel::RelationalOperationElement[1]):meta::relational::metamodel::datatype::DataType[0..1]
{
    $rop.inferredType
}
function meta::relational::functions::typeInference::inferRelationalType(rop:meta::relational::metamodel::RelationalOperationElement[1], failOnMatchFailure:Boolean[1]):meta::relational::metamodel::datatype::DataType[0..1]
{
    $rop->meta::relational::functions::typeInference::inferRelationalType()
}

function meta::relational::metamodel::datatype::dataTypeToSqlText(type:meta::relational::metamodel::datatype::DataType[1]):String[1]
{
    $type->match([
        i : meta::relational::metamodel::datatype::Integer[1] | 'INT',
        f : meta::relational::metamodel::datatype::Float[1] | 'FLOAT',
        v : meta::relational::metamodel::datatype::Varchar[1] | format('VARCHAR(%%d)', $v.size),
        c : meta::relational::metamodel::datatype::Char[1] | format('CHAR(%%d)', $c.size),
        d : meta::relational::metamodel::datatype::Decimal[1] | format('DECIMAL(%%d, %%d)', [$d.precision, $d.scale]),
        t : meta::relational::metamodel::datatype::Timestamp[1] | 'TIMESTAMP',
        d : meta::relational::metamodel::datatype::Date[1] | 'DATE',
        t : meta::relational::metamodel::datatype::Time[1] | 'TIME',
        t : meta::relational::metamodel::datatype::TimestampNs[1] | 'TIMESTAMP_NS',
        b : meta::relational::metamodel::datatype::BigInt[1] | 'BIGINT',
        s : meta::relational::metamodel::datatype::SmallInt[1] | 'SMALLINT',
        t : meta::relational::metamodel::datatype::TinyInt[1] | 'TINYINT',
        d : meta::relational::metamodel::datatype::Double[1] | 'DOUBLE',
        n : meta::relational::metamodel::datatype::Numeric[1] | format('NUMERIC(%%d, %%d)', [$n.precision, $n.scale]),
        d : meta::relational::metamodel::datatype::Distinct[1] | 'DISTINCT',
        o : meta::relational::metamodel::datatype::Other[1] | 'OTHER',
        b : meta::relational::metamodel::datatype::Bit[1] | 'BIT',
        b : meta::relational::metamodel::datatype::Binary[1] | format('BINARY(%%d)', $b.size),
        r : meta::relational::metamodel::datatype::Real[1] | 'REAL',
        a : meta::relational::metamodel::datatype::Array[1] | 'ARRAY',
        v : meta::relational::metamodel::datatype::Varbinary[1] | format('VARBINARY(%%d)', $v.size),
        s : meta::relational::metamodel::datatype::SemiStructured[1] | 'SEMISTRUCTURED',
        o : meta::relational::metamodel::datatype::Object[1] | 'OBJECT'
    ])
}

function meta::pure::executionPlan::allNodes(node:meta::pure::executionPlan::ExecutionNode[1], extensions:meta::pure::extension::Extension[*]):meta::pure::executionPlan::ExecutionNode[*]
{
    $node.subtree.node
}
function meta::pure::functions::meta::getLowerBound(multiplicity:meta::pure::metamodel::multiplicity::Multiplicity[1]):Integer[1]
{
    $multiplicity.lowerBound->toOne().value->toOne()
}
function meta::pure::extension::routerExtensions(_this:meta::pure::extension::Extension[1]):meta::pure::router::extension::RouterExtension[*]
{
    $_this.availableStores->concatenate($_this.availableFeatures)->cast(@meta::pure::router::extension::RouterExtension)
}
function meta::relational::functions::pureToSqlQuery::expressionSequenceReturnsAtLeastToOneDataType(v:meta::pure::metamodel::valuespecification::ValueSpecification[1]):Boolean[1]
{
    $v.multiplicity->toOne()->meta::pure::functions::meta::getLowerBound() >= 1
}
function meta::relational::mapping::inferPrimaryKeyColumnNames(vs:meta::pure::metamodel::valuespecification::ValueSpecification[1]):String[*]
{
    $vs.inferredPrimaryKeyColumns->sortBy(c|$c.ordinal).name
}
function meta::pure::lineage::scanRelations::relationTreeAsString(t:meta::pure::lineage::scanRelations::RelationTree[1]):String[1]
{
    $t->meta::pure::lineage::scanRelations::relationTreeAsString(true)
}
function meta::pure::lineage::scanRelations::relationTreeAsString(t:meta::pure::lineage::scanRelations::RelationTree[1], withJoin:Boolean[1]):String[1]
{
    $t->meta::lite::lineage::relationTreeLines($withJoin)->joinStrings('', '\\n', '\\n')
}
function meta::pure::lineage::scanRelations::relationTreeAsString(t:meta::pure::lineage::scanRelations::RelationTree[1], space:String[1]):String[1]
{
    $t->meta::pure::lineage::scanRelations::relationTreeAsString(true, $space)
}
function meta::pure::lineage::scanRelations::relationTreeAsString(t:meta::pure::lineage::scanRelations::RelationTree[1], withJoin:Boolean[1], space:String[1]):String[1]
{
    $t->meta::lite::lineage::relationTreeLines($withJoin)->map(l|$space + $l)->joinStrings('', '\\n', '\\n')
}
function meta::lite::lineage::relationTreeLines(t:meta::pure::lineage::scanRelations::RelationTree[1], withJoin:Boolean[1]):String[*]
{
    $t.nodes->sortBy(n|$n.preorder)->map(n|if($n.kind == 'root', |$n.indent + 'root', |$n.indent + '------> (' + $n.kind + ') ' + $n.name->toOne() + if($withJoin && $n.joinLabel->isNotEmpty(), |'(' + $n.joinLabel->toOne() + ')', |'') + ' [' + $n.columns->sortBy(c|$c.ordinal).name->joinStrings(', ') + ']'))
}
function meta::pure::mapping::resolveStore(_this:meta::pure::mapping::Mapping[1], store:meta::pure::store::Store[1]):meta::pure::store::Store[1]
{
    let substitute = $_this.storeResolutions->filter(r|$r.original == $store).resolved;
    if($substitute->isEmpty(), |$store, |$substitute->toOne());
}
function meta::relational::runtime::extractDBs(m:meta::pure::mapping::Mapping[1]):meta::relational::metamodel::Database[*]
{
    $m.visibility.visible.classMappings->filter(cm|$cm->instanceOf(meta::relational::mapping::RootRelationalInstanceSetImplementation))->cast(@meta::relational::mapping::RootRelationalInstanceSetImplementation).mainTableAlias.database->removeDuplicates()
}
function meta::pure::mapping::classMappings(_this:meta::pure::mapping::Mapping[1]):meta::pure::mapping::SetImplementation[*]
{
    $_this.visibility->sortBy(v|$v.includeRank).visible.classMappings
}
function meta::pure::mapping::classMappingById(_this:meta::pure::mapping::Mapping[1], id:String[1]):meta::pure::mapping::SetImplementation[0..1]
{
    $_this.visibility.visible.classMappings->filter(cm|$cm.id == $id)->first()
}

function meta::relational::metamodel::mainTable(_this:meta::relational::metamodel::RelationalMappingSpecification[1]):meta::relational::metamodel::relation::Table[1]
{
    $_this.mainTableAlias.base
}

function meta::pure::mapping::superMapping(_this:meta::pure::mapping::PropertyMappingsImplementation[1]):meta::pure::mapping::PropertyMappingsImplementation[0..1]
{
    $_this->cast(@meta::relational::mapping::RootRelationalInstanceSetImplementation).ancestry->filter(a|$a.depth == 1).ancestor->first()
}

function meta::pure::mapping::allSuperSetImplementations(set:meta::pure::mapping::PropertyMappingsImplementation[1], m:meta::pure::mapping::Mapping[1]):meta::pure::mapping::PropertyMappingsImplementation[*]
{
    $set->cast(@meta::relational::mapping::RootRelationalInstanceSetImplementation).ancestry->filter(a|$a.depth > 0).ancestor
}

function meta::relational::mapping::resolvePrimaryKey(_this:meta::relational::mapping::RootRelationalInstanceSetImplementation[1]):meta::relational::metamodel::RelationalOperationElement[*]
{
    $_this.ancestry->filter(a|$a.depth == 0 || !$a.ancestor.groupBy->isEmpty() || $a.ancestor.distinct == true || $a.ancestor.userDefinedPrimaryKey == true)->sortBy(a|if(!$a.ancestor.groupBy->isEmpty(), |0, |if($a.ancestor.distinct == true, |1000, |if($a.ancestor.userDefinedPrimaryKey == true, |2000, |3000))) + $a.depth)->first().ancestor.primaryKey
}

###Mapping
Mapping meta::lite::metamodel::MetamodelMapping
(
    *meta::pure::metamodel::type::Class[cls]: Relational
    {
        ~primaryKey(%1$s metamodel.classes.fqn)
        ~mainTable %1$s metamodel.classes
        name: %1$s metamodel.classes.name
    }
    *meta::pure::mapping::Mapping[mapping]: Relational
    {
        ~primaryKey(%1$s metamodel.mappings.fqn)
        ~mainTable %1$s metamodel.mappings
        name: %1$s metamodel.mappings.name,
        classMappings[rootRel]: %1$s@ClassMappingsToMappings,
        enumerationMappings[enumMap]: %1$s@EnumerationMappingsToMappings,
        includes[mappingInclude]: %1$s@MappingsToIncludes
    }
    *meta::pure::mapping::MappingInclude[mappingInclude]: Relational
    {
        ~primaryKey(%1$s metamodel.mapping_includes.mapping_fqn, %1$s metamodel.mapping_includes.included_fqn)
        ~mainTable %1$s metamodel.mapping_includes
        owner[mapping]: %1$s@MappingsToIncludes,
        included[mapping]: %1$s@IncludeToIncluded
    }
    *meta::pure::mapping::EnumerationMapping[enumMap]: Relational
    {
        ~primaryKey(%1$s metamodel.enumeration_mappings.mapping_fqn, %1$s metamodel.enumeration_mappings.name)
        ~mainTable %1$s metamodel.enumeration_mappings
        name: %1$s metamodel.enumeration_mappings.name,
        parent[mapping]: %1$s@EnumerationMappingsToMappings,
        enumValueMappings[evm]: %1$s@EnumerationMappingToValues
    }
    *meta::pure::mapping::EnumValueMapping[evm]: Relational
    {
        ~primaryKey(%1$s metamodel.enum_value_mappings.mapping_fqn, %1$s metamodel.enum_value_mappings.em_name, %1$s metamodel.enum_value_mappings.enum_value)
        ~mainTable %1$s metamodel.enum_value_mappings
        enum: %1$s metamodel.enum_value_mappings.enum_value
    }
    *meta::lite::metamodel::EnumSourceValue[esv]: Relational
    {
        ~primaryKey(%1$s metamodel.enum_value_sources.mapping_fqn, %1$s metamodel.enum_value_sources.em_name, %1$s metamodel.enum_value_sources.enum_value, %1$s metamodel.enum_value_sources.source_value)
        ~mainTable %1$s metamodel.enum_value_sources
        value: %1$s metamodel.enum_value_sources.source_value
    }
    *meta::lite::metamodel::MappingVisibility[vis]: Relational
    {
        ~primaryKey(%1$s metamodel.mapping_includes_closure.mapping_fqn, %1$s metamodel.mapping_includes_closure.included_fqn)
        ~mainTable %1$s metamodel.mapping_includes_closure
        includeRank: %1$s metamodel.mapping_includes_closure.include_rank
    }
    *meta::lite::metamodel::StoreResolution[sres]: Relational
    {
        ~primaryKey(%1$s metamodel.mapping_store_resolutions.mapping_fqn, %1$s metamodel.mapping_store_resolutions.original_fqn)
        ~mainTable %1$s metamodel.mapping_store_resolutions
    }
    *meta::pure::mapping::SetImplementation: Operation
    {
        %2$s
    }
    meta::relational::mapping::RootRelationalInstanceSetImplementation[rootRel]: Relational
    {
        ~primaryKey(%1$s metamodel.class_mappings.mapping_fqn, %1$s metamodel.class_mappings.id)
        ~mainTable %1$s metamodel.class_mappings
        id: %1$s metamodel.class_mappings.id,
        root: %1$s metamodel.class_mappings.root,
        class[cls]: %1$s@ClassMappingsToClass,
        superSetImplementationId: %1$s metamodel.class_mappings.super_set_id,
        distinct: %1$s metamodel.class_mappings.distinct_set,
        userDefinedPrimaryKey: %1$s metamodel.class_mappings.user_defined_pk,
        parent: %1$s@ClassMappingsToMappings,
        mainTableAlias: %1$s@ClassMappingsToAlias,
        groupBy[gbm]: %1$s@SetToGroupBy,
        primaryKey[opTac]: %1$s@SetToPrimaryKeyOps
    }
    meta::relational::mapping::GroupByMapping[gbm]: Relational
    {
        ~primaryKey(%1$s metamodel.group_by_mappings.mapping_fqn, %1$s metamodel.group_by_mappings.id)
        ~mainTable %1$s metamodel.group_by_mappings
    }
    meta::lite::metamodel::SetAncestry[anc]: Relational
    {
        ~primaryKey(%1$s metamodel.set_ancestry.mapping_fqn, %1$s metamodel.set_ancestry.id, %1$s metamodel.set_ancestry.super_mapping_fqn, %1$s metamodel.set_ancestry.super_id)
        ~mainTable %1$s metamodel.set_ancestry
        depth: %1$s metamodel.set_ancestry.depth
    }
    meta::relational::metamodel::TableAlias[alias]: Relational
    {
        ~filter %1$s ElTableAlias
        ~primaryKey(%1$s metamodel.relational_elements.id)
        ~mainTable %1$s metamodel.relational_elements
        name: %1$s metamodel.relational_elements.name,
        database[db]: %1$s@AliasToDatabase,
        relationalElement[tbl]: %1$s@AliasToTables,
        relationalElement[vw]: %1$s@AliasToViews
    }
    meta::relational::metamodel::relation::View[vw]: Relational
    {
        ~filter %1$s ElView
        ~primaryKey(%1$s metamodel.relational_elements.id)
        ~mainTable %1$s metamodel.relational_elements
        name: %1$s metamodel.relational_elements.name,
        mainTableAlias[alias]: %1$s@ViewToAlias,
        columnMappings[vcm]: %1$s@ViewToColumnMappings
    }
    *meta::relational::metamodel::Database[db]: Relational
    {
        ~primaryKey(%1$s metamodel.databases.fqn)
        ~mainTable %1$s metamodel.databases
        name: %1$s metamodel.databases.name,
        schemas[schema]: %1$s@DbToSchemas
    }
    *meta::relational::metamodel::Schema[schema]: Relational
    {
        ~primaryKey(%1$s metamodel.schemas.db_fqn, %1$s metamodel.schemas.name)
        ~mainTable %1$s metamodel.schemas
        name: %1$s metamodel.schemas.name,
        tables[tbl]: %1$s@SchemaToTables,
        views[vw]: %1$s@SchemaToViews
    }
    *meta::relational::mapping::ColumnMapping[vcm]: Relational
    {
        ~primaryKey(%1$s metamodel.view_column_mappings.db_fqn, %1$s metamodel.view_column_mappings.schema_name, %1$s metamodel.view_column_mappings.view_name, %1$s metamodel.view_column_mappings.column_name)
        ~mainTable %1$s metamodel.view_column_mappings
        columnName: %1$s metamodel.view_column_mappings.column_name,
%5$s
    }
    *meta::pure::metamodel::function::property::Property[prop]: Relational
    {
        ~primaryKey(%1$s metamodel.properties.owner_fqn, %1$s metamodel.properties.name)
        ~mainTable %1$s metamodel.properties
        name: %1$s metamodel.properties.name
    }
    *meta::pure::mapping::PropertyMapping: Operation
    {
        %2$s
    }
    meta::relational::mapping::RelationalPropertyMapping[rpm]: Relational
    {
        ~primaryKey(%1$s metamodel.property_mappings.mapping_fqn, %1$s metamodel.property_mappings.id, %1$s metamodel.property_mappings.ordinal)
        ~mainTable %1$s metamodel.property_mappings
        property[prop]: %1$s@PropertyMappingToProperty,
%6$s
    }
    *meta::relational::metamodel::datatype::DataType: Operation
    {
        %2$s
    }
%4$s
%10$s
    *meta::relational::metamodel::RelationalOperationElement: Operation
    {
        %2$s
    }
    meta::relational::metamodel::relation::Table[tbl]: Relational
    {
        ~filter %1$s ElTable
        ~primaryKey(%1$s metamodel.relational_elements.id)
        ~mainTable %1$s metamodel.relational_elements
        name: %1$s metamodel.relational_elements.name,
        schema[schema]: %1$s@SchemaToTables,
        columns[col]: %1$s@TableToColumns
    }
    meta::relational::metamodel::Column[col]: Relational
    {
        ~filter %1$s ElColumn
        ~primaryKey(%1$s metamodel.relational_elements.id)
        ~mainTable %1$s metamodel.relational_elements
        name: %1$s metamodel.relational_elements.name,
        owner[tbl]: %1$s@ColumnToOwnerTable,
%7$s
    }
%8$s
    meta::lite::metamodel::InferredTypes: Relational
    {
        AssociationMapping
        (
%9$s
        )
    }
    meta::lite::metamodel::EffectivePropertyMappings: Relational
    {
        AssociationMapping
        (
            effectivePropertyMappings[rootRel, rpm]: %1$s@SetToPropertyMappings,
            allPropertyMappingsOf[rpm, rootRel]: %1$s@SetToPropertyMappings
        )
    }
    meta::lite::metamodel::AliasBaseTables: Relational
    {
        AssociationMapping
        (
            base[alias, tbl]: %1$s@AliasToBaseTable,
            baseOf[tbl, alias]: %1$s@AliasToBaseTable
        )
    }
    meta::lite::metamodel::SetAncestries: Relational
    {
        AssociationMapping
        (
            descendant[anc, rootRel]: %1$s@SetToAncestry,
            ancestry[rootRel, anc]: %1$s@SetToAncestry
        )
    }
    meta::lite::metamodel::EnumValueSources: Relational
    {
        AssociationMapping
        (
            sources[evm, esv]: %1$s@EnumValueToSources,
            ofValueMapping[esv, evm]: %1$s@EnumValueToSources
        )
    }
    meta::lite::metamodel::SetAncestors: Relational
    {
        AssociationMapping
        (
            ancestor[anc, rootRel]: %1$s@AncestryToAncestor,
            ancestryOf[rootRel, anc]: %1$s@AncestryToAncestor
        )
    }
    meta::lite::metamodel::MappingVisibilities: Relational
    {
        AssociationMapping
        (
            viewer[vis, mapping]: %1$s@MappingsToClosure,
            visibility[mapping, vis]: %1$s@MappingsToClosure
        )
    }
    meta::lite::metamodel::VisibleMappings: Relational
    {
        AssociationMapping
        (
            visible[vis, mapping]: %1$s@ClosureToVisible,
            visibleFrom[mapping, vis]: %1$s@ClosureToVisible
        )
    }
    meta::lite::metamodel::VisibleSets: Relational
    {
        AssociationMapping
        (
            visibilityOf[rootRel, vis]: %1$s@ClosureToClassMappings,
            visibleSets[vis, rootRel]: %1$s@ClosureToClassMappings
        )
    }
    meta::lite::metamodel::MappingStoreResolutions: Relational
    {
        AssociationMapping
        (
            resolver[sres, mapping]: %1$s@MappingsToStoreResolutions,
            storeResolutions[mapping, sres]: %1$s@MappingsToStoreResolutions
        )
    }
    meta::lite::metamodel::StoreResolutionOriginal: Relational
    {
        AssociationMapping
        (
            original[sres, db]: %1$s@ResolutionToOriginal,
            originalOf[db, sres]: %1$s@ResolutionToOriginal
        )
    }
    meta::lite::metamodel::StoreResolutionResolved: Relational
    {
        AssociationMapping
        (
            resolved[sres, db]: %1$s@ResolutionToResolved,
            resolvedOf[db, sres]: %1$s@ResolutionToResolved
        )
    }
)
";

static SOURCE: LazyLock<String> = LazyLock::new(|| {
    formatted(
        TEMPLATE,
        &[
            S,
            INHERITANCE_OP,
            &filters(),
            &data_type_sets(),
            &op_routes("relationalOperationElement", "ColumnMappingToOp"),
            &op_routes("relationalOperationElement", "PropertyMappingToOp"),
            &type_routes(),
            &op_sets(),
            &inferred_type_ends(),
            &plan_sets(),
        ],
    )
});

/// `SystemMetamodel.source()`: the system Pure source (tests inspect it; never edited at run time).
pub fn source() -> &'static str {
    &SOURCE
}

/// Parsed once, on the first read, at lite's dialect -- a source that does not parse is the platform's bug, loud (the
/// `Pure` native-catalog discipline).
static ELEMENTS: LazyLock<Arc<[PackageableElement]>> = LazyLock::new(|| {
    let units: Vec<u16> = source().encode_utf16().collect();
    crate::door::parse(&units, Dialect::LegendLite)
        .unwrap_or_else(|e| panic!("the system metamodel does not parse: {e:?}"))
        .elements
});

/// `SystemMetamodel.elements()`: the system elements as parsed (at lite's dialect), in source order -- THE BOOT
/// LAYER's input (compiled once per process, content-addressed by the source's hash, entered into every graph's
/// index exactly like the graph's own elements, protected as system).
pub fn elements() -> &'static [PackageableElement] {
    &ELEMENTS
}

/// Java's holder class `Fqns`: built on the first read of any of its answers, after the elements.
struct Fqns {
    /// Every element's FQN, functions included, each once in first-seen order (a `LinkedHashSet`).
    all: Vec<JStr>,
    all_set: HashSet<JStr>,
    /// The non-function elements' FQNs, in first-seen order: reserved, never redefined.
    not_functions: Vec<JStr>,
    not_function_set: HashSet<JStr>,
    /// The system functions by id, in source order (a `LinkedHashMap`), and each id's place in it.
    functions_by_id: Vec<(FunctionId, Arc<FunctionDefinition>)>,
    function_index: HashMap<FunctionId, usize>,
}

static FQNS: LazyLock<Fqns> = LazyLock::new(|| {
    let (all, all_set) = ordered(elements().iter().map(|e| e.qualified_name().clone()));
    let (not_functions, not_function_set) = ordered(
        elements()
            .iter()
            .filter(|e| !matches!(e, PackageableElement::FunctionDefinition(_)))
            .map(|e| e.qualified_name().clone()),
    );
    let mut functions_by_id = Vec::new();
    let mut function_index = HashMap::new();
    for e in elements() {
        if let PackageableElement::FunctionDefinition(f) = e {
            let id = FunctionId::of(&Function::FunctionDefinition(f.clone()));
            if function_index.insert(id.clone(), functions_by_id.len()).is_some() {
                panic!("the system metamodel declares {id} twice");
            }
            functions_by_id.push((id, f.clone()));
        }
    }
    Fqns {
        all,
        all_set,
        not_functions,
        not_function_set,
        functions_by_id,
        function_index,
    }
});

/// Each distinct value, in first-seen order, and the set of them (a `LinkedHashSet`: the list for order, the set for
/// membership).
fn ordered(values: impl Iterator<Item = JStr>) -> (Vec<JStr>, HashSet<JStr>) {
    let mut seen = HashSet::new();
    let list = values.filter(|v| seen.insert(v.clone())).collect();
    (list, seen)
}

/// `SystemMetamodel.elementFqns()`: every system element's FQN (functions included), each once, in source order --
/// the boot layer's contribution to a graph's name-resolution universe.
pub fn element_fqns() -> &'static [JStr] {
    &FQNS.all
}

/// `SystemMetamodel.elementFqns().contains(fqn)`.
pub fn declares(fqn: &JStr) -> bool {
    FQNS.all_set.contains(fqn)
}

/// The system ELEMENTS' names (functions excluded), in source order: reserved, never redefined (Java's private
/// `Fqns.NOT_FUNCTIONS`, the set [`require_no_system_element_redefined`] reads).
pub fn non_function_fqns() -> &'static [JStr] {
    &FQNS.not_functions
}

/// `SystemMetamodel.functionIds()`: the ids of the system metamodel's functions (the platform's own Pure), in order.
pub fn function_ids() -> Vec<FunctionId> {
    functions_by_id().into_iter().map(|(id, _)| id).collect()
}

/// `SystemMetamodel.functionIds().contains(id)`.
pub fn is_function_id(id: &FunctionId) -> bool {
    FQNS.function_index.contains_key(id)
}

/// `SystemMetamodel.functionsById()`: the system functions as parsed, by id, in order -- the body a loaded
/// declaration with that id takes.
pub fn functions_by_id() -> Vec<(FunctionId, Arc<FunctionDefinition>)> {
    FQNS.functions_by_id.clone()
}

/// `SystemMetamodel.functionsById().get(id)`.
pub fn function_by_id(id: &FunctionId) -> Option<&'static Arc<FunctionDefinition>> {
    FQNS.function_index.get(id).map(|&i| &FQNS.functions_by_id[i].1)
}

/// `SystemMetamodel.isSystemElement(fqn)`: whether the FQN names a system ELEMENT (class, association, store, mapping
/// -- not a function body): reserved, never redefined. Compared unit for unit, as Java's `equals`.
pub fn is_system_element(fqn: &str) -> bool {
    elements().iter().any(|e| {
        !matches!(e, PackageableElement::FunctionDefinition(_))
            && e.qualified_name().0.iter().copied().eq(fqn.encode_utf16())
    })
}

/// `SystemMetamodel.requireNoSystemElementRedefined(parsed)`: the system layer is protected -- a parsed element with
/// the qualified name of a system ELEMENT (a class, an association, a store, a mapping; not a function) is the user's
/// error (`ModelException`, phase NORMALIZE, at the element), the first such in the model's order. Functions are not
/// hidden here: a loaded function with a system version's id is the platform's own Pure and takes the system
/// version's body where the boot layer and a graph are merged; another version under the name is its own function.
/// Returns `parsed` unchanged, for chaining.
pub fn require_no_system_element_redefined(parsed: &ParsedModel) -> Result<&ParsedModel, CompileError> {
    for e in parsed.elements.iter() {
        let fqn = e.qualified_name();
        if FQNS.not_function_set.contains(fqn) {
            return Err(CompileError::model(
                Phase::Normalize,
                lite_model::cat!(
                    "'",
                    fqn,
                    "' is a system element of the",
                    " platform's metamodel layer and cannot be redefined"
                )
                .0,
                Some(fqn.0.clone()),
            ));
        }
    }
    Ok(parsed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formatted_fills_indexed_arguments_and_unescapes_percent() {
        assert_eq!(
            formatted("a%2$s-%1$s %%d %10$s", &["x", "y", "", "", "", "", "", "", "", "z"]),
            "ay-x %d z"
        );
    }

    #[test]
    #[should_panic(expected = "does not format: %d")]
    fn formatted_refuses_a_specifier_it_does_not_know() {
        formatted("%d", &["x"]);
    }

    #[test]
    #[should_panic(expected = "does not format: %3$s")]
    fn formatted_refuses_a_missing_argument() {
        formatted("%3$s", &["x"]);
    }

    #[test]
    fn the_source_is_the_filled_template() {
        let source = source();
        assert!(source.starts_with("###Relational\nDatabase meta::lite::metamodel::MetamodelStore\n"));
        assert!(source.ends_with("    }\n)\n"));
        assert!(!source.contains("%1$s") && !source.contains("%%"));
        assert!(source.contains("format('VARCHAR(%d)', $v.size)"));
        // Pure's escape, never a raw line break inside the literal, and no blank line before the Database's ")"
        assert!(source.contains("->joinStrings('', '\\n', '\\n')"));
        assert!(!source.contains("'\n'"));
        assert!(source.contains("')\n)\n\n###Pure") && !source.contains("\n\n)\n\n###Pure"));
    }
}
