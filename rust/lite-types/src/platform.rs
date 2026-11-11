// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! Java's `PlatformTypes`: the one home of the platform's distinguished names and the exact checks on them, never by
//! a suffix (a user's `my::domain::List` is not the platform's `List`). Every constant is Java's, in Java's order,
//! held to Java's value by the oracle; three are spelled by another layer in Java and here until that layer is
//! ported: `TDS_NULL_CELL` (the SQL tree's, `lite_sql::TDS_NULL_CELL`), `TDS_ROW` (`builtin`'s
//! `NativeFn.TDS_ROW_OWNER`) and `FEATURE` (`platform`'s `Feature.FQN`).
//!
//! Not ported with them, for the layer they need (docs/rust-port/PHASE3_CRATES.md): `eraseTdsRow`
//! (`NativeFn.RowGetter`), `handleRowClass` and `isStatementOnly` (`FunctionId`, `NativeFn`).

use lite_base::Alloc;

use crate::primitive::Primitive;
use crate::ty::{FunctionType, GenericType, Type};

macro_rules! constants {
    ($($name:ident = $value:expr;)*) => {
        $(pub const $name: &str = $value;)*

        /// Every constant above by its Java name, in Java's declaration order.
        pub const NAMED: &[(&str, &str)] = &[$((stringify!($name), $name)),*];
    };
}

constants! {
    ROWS_MARKER = "rows";
    CLASS_METACLASS = "meta::pure::metamodel::type::Class";
    TDS_NULL_CELL = lite_sql::TDS_NULL_CELL;
    TDS_NULL_FQN = "meta::pure::tds::TDSNull";
    ANY = "meta::pure::metamodel::type::Any";
    NIL = "meta::pure::metamodel::type::Nil";
    ENUM = "meta::pure::metamodel::type::Enum";
    VARIANT = "meta::pure::metamodel::variant::Variant";
    LIST = "meta::pure::functions::collection::List";
    PAIR = "meta::pure::functions::collection::Pair";
    FUNCTION = "meta::pure::metamodel::function::Function";
    FUNCTION_DEFINITION = "meta::pure::metamodel::function::FunctionDefinition";
    LAMBDA_FUNCTION = "meta::pure::metamodel::function::LambdaFunction";
    CONCRETE_FUNCTION_DEFINITION = "meta::pure::metamodel::function::ConcreteFunctionDefinition";
    TABULAR_DATA_SET = "meta::pure::tds::TabularDataSet";
    TDS_ROW = "meta::pure::tds::TDSRow";
    MAPPING = "meta::pure::mapping::Mapping";
    DATABASE = "meta::relational::metamodel::Database";
    RELATIONAL_TABLE = "meta::relational::metamodel::relation::Table";
    ELEMENT_OVERRIDE = "meta::pure::metamodel::type::ElementOverride";
    GENERIC_TYPE = "meta::pure::metamodel::type::generics::GenericType";
    MEASURE = "meta::pure::metamodel::type::Measure";
    UNIT = "meta::pure::metamodel::type::Unit";
    PACKAGE = "meta::pure::metamodel::Package";
    ENUMERATION = "meta::pure::metamodel::type::Enumeration";
    RELATION = "meta::pure::metamodel::relation::Relation";
    RELATION_STORE_ACCESSOR = "meta::pure::store::RelationStoreAccessor";
    RELATION_ELEMENT_ACCESSOR = "meta::pure::metamodel::relation::RelationElementAccessor";
    TDS_RELATION_ACCESSOR = "meta::pure::metamodel::relation::TDSRelationAccessor";
    PROFILE = "meta::pure::metamodel::extension::Profile";
    COL_SPEC = "meta::pure::metamodel::relation::ColSpec";
    COL_SPEC_ARRAY = "meta::pure::metamodel::relation::ColSpecArray";
    FUNC_COL_SPEC = "meta::pure::metamodel::relation::FuncColSpec";
    FUNC_COL_SPEC_ARRAY = "meta::pure::metamodel::relation::FuncColSpecArray";
    AGG_COL_SPEC = "meta::pure::metamodel::relation::AggColSpec";
    AGG_COL_SPEC_ARRAY = "meta::pure::metamodel::relation::AggColSpecArray";
    ROWS = "meta::pure::functions::relation::Rows";
    WINDOW = "meta::pure::functions::relation::_Window";
    RESULT = "meta::pure::mapping::Result";
    TDS_RELATION_CLASS = "meta::pure::metamodel::relation::TDS";
    TDS_CSV_PROPERTY = "csv";
    EXECUTE_IN_DB = "meta::relational::metamodel::execute::executeInDb";
    EXECUTE_IN_DB_TO_TDS = "meta::relational::metamodel::execute::executeInDbToTDS";
    FETCH_DB_TABLES_META_DATA = "meta::relational::metamodel::execute::fetchDbTablesMetaData";
    FETCH_DB_COLUMNS_META_DATA = "meta::relational::metamodel::execute::fetchDbColumnsMetaData";
    FETCH_DB_SCHEMAS_META_DATA = "meta::relational::metamodel::execute::fetchDbSchemasMetaData";
    FETCH_DB_PRIMARY_KEYS_META_DATA = "meta::relational::metamodel::execute::fetchDbPrimaryKeysMetaData";
    DROP_AND_CREATE_TABLE_IN_DB = "meta::relational::functions::toDDL::dropAndCreateTableInDb";
    LOAD_CSV_TO_DB_TABLE = "meta::relational::metamodel::execute::loadCsvToDbTable";
    DROP_AND_CREATE_SCHEMA_IN_DB = "meta::relational::functions::toDDL::dropAndCreateSchemaInDb";
    DROP_SCHEMA_STATEMENT = "meta::relational::functions::toDDL::dropSchemaStatement";
    CREATE_SCHEMA_STATEMENT = "meta::relational::functions::toDDL::createSchemaStatement";
    CREATE_TABLE_STATEMENT = "meta::relational::functions::toDDL::createTableStatement";
    DROP_TABLE_STATEMENT = "meta::relational::functions::toDDL::dropTableStatement";
    DATATYPE_BIGINT = "meta::relational::metamodel::datatype::BigInt";
    DATATYPE_SMALLINT = "meta::relational::metamodel::datatype::SmallInt";
    DATATYPE_TINYINT = "meta::relational::metamodel::datatype::TinyInt";
    DATATYPE_INTEGER = "meta::relational::metamodel::datatype::Integer";
    DATATYPE_FLOAT = "meta::relational::metamodel::datatype::Float";
    DATATYPE_DOUBLE = "meta::relational::metamodel::datatype::Double";
    DATATYPE_REAL = "meta::relational::metamodel::datatype::Real";
    DATATYPE_BIT = "meta::relational::metamodel::datatype::Bit";
    DATATYPE_TIMESTAMP = "meta::relational::metamodel::datatype::Timestamp";
    DATATYPE_DATE = "meta::relational::metamodel::datatype::Date";
    DATATYPE_TIME = "meta::relational::metamodel::datatype::Time";
    DATATYPE_TIMESTAMP_NS = "meta::relational::metamodel::datatype::TimestampNs";
    DATATYPE_DISTINCT = "meta::relational::metamodel::datatype::Distinct";
    DATATYPE_OTHER = "meta::relational::metamodel::datatype::Other";
    DATATYPE_SEMI_STRUCTURED = "meta::relational::metamodel::datatype::SemiStructured";
    DATATYPE_VARCHAR = "meta::relational::metamodel::datatype::Varchar";
    DATATYPE_CHAR = "meta::relational::metamodel::datatype::Char";
    DATATYPE_BINARY = "meta::relational::metamodel::datatype::Binary";
    DATATYPE_VARBINARY = "meta::relational::metamodel::datatype::Varbinary";
    DATATYPE_DECIMAL = "meta::relational::metamodel::datatype::Decimal";
    DATATYPE_NUMERIC = "meta::relational::metamodel::datatype::Numeric";
    STORE_SCHEMA_NAV = "meta::relational::metamodel::schema";
    STORE_TABLE_NAV = "meta::relational::metamodel::table";
    TO_SQL_STRING = "meta::relational::functions::sqlstring::toSQLString";
    TO_SQL_STRING_PRETTY = "meta::relational::functions::sqlstring::toSQLStringPretty";
    TO_SQL = "meta::relational::functions::sqlstring::toSQL";
    TO_NON_EXECUTABLE_SQL_STRING = "meta::relational::functions::sqlstring::toNonExecutableSQLString";
    SET_UP_DATA_SQLS_V2 = "meta::alloy::service::execution::setUpDataSQLsV2";
    SET_UP_DATA_SQLS = "meta::alloy::service::execution::setUpDataSQLs";
    EXECUTION_PLAN = "meta::pure::executionPlan::executionPlan";
    SCAN_RELATIONS = "meta::pure::lineage::scanRelations::scanRelations";
    SCAN_PROPERTIES = "meta::pure::lineage::scanProperties::scanProperties";
    BUILD_PROPERTY_TREE = "meta::pure::lineage::scanProperties::propertyTree::buildPropertyTree";
    SCAN_COLUMNS = "meta::pure::lineage::scanColumns::scanColumns";
    PREVAL = "meta::pure::router::preeval::preval";
    PLAN_TO_STRING = "meta::pure::executionPlan::toString::planToString";
    PLAN_TO_STRING_WITHOUT_FORMATTING = "meta::pure::executionPlan::toString::planToStringWithoutFormatting";
    EXECUTE = "meta::pure::router::execute";
    EXECUTE_LEGEND_QUERY = "meta::legend::executeLegendQuery";
    EXECUTION_PLAN_EXECUTE = "meta::pure::executionPlan::execute";
    EQUALITY_PROFILE = "meta::pure::profiles::equality";
    TEMPORAL_PROFILE = "meta::pure::profiles::temporal";
    PCT_PROFILE = "meta::pure::test::pct::PCT";
    GRAPH_PROFILE = "graph::graph";
    TO_CSV = "meta::relational::tests::csv::toCSV";
    TO_REPRESENTATION = "meta::pure::functions::string::toRepresentation";
    ASSERT_INSTANCE_OF = "meta::pure::functions::asserts::assertInstanceOf";
    ASSERT_ERROR = "meta::pure::functions::asserts::assertError";
    GET_RELATIONAL_CSV_DATA = "meta::relational::testDataGeneration::getRelationalCSVDataFromQuery";
    GENERATE_TEST_DATA = "meta::relational::testDataGeneration::generateTestData";
    PLAN_TEST_DATA_GENERATION = "meta::relational::testDataGeneration::executionPlan::planTestDataGeneration";
    GENERATE_SEED_DATA_STRING = "meta::relational::testDataGeneration::generateSeedDataString";
    RUNTIME = "meta::core::runtime::Runtime";
    PACKAGEABLE_RUNTIME = "meta::pure::runtime::PackageableRuntime";
    CONNECTION_STORE = "meta::core::runtime::ConnectionStore";
    WITH_CHAINED_MAPPINGS = "meta::pure::mapping::withChainedMappings";
    MODEL_CHAIN_CONNECTION = "meta::external::store::model::ModelChainConnection";
    JSON_MODEL_CONNECTION = "meta::external::store::model::JsonModelConnection";
    DATABASE_CONNECTION = "meta::external::store::relational::runtime::DatabaseConnection";
    RELATIONAL_DATABASE_CONNECTION = "meta::external::store::relational::runtime::RelationalDatabaseConnection";
    TEST_DATABASE_CONNECTION = "meta::external::store::relational::runtime::TestDatabaseConnection";
    TEST_RUNTIME = "meta::external::store::relational::tests::testRuntime";
    IS_EMPTY = "meta::pure::functions::collection::isEmpty";
    IS_NOT_EMPTY = "meta::pure::functions::collection::isNotEmpty";
    FIRST = "meta::pure::functions::collection::first";
    ASSERT = "meta::pure::functions::asserts::assert";
    ASSERT_FALSE = "meta::pure::functions::asserts::assertFalse";
    PAIR_FN = "meta::pure::functions::collection::pair";
    COUNT = "meta::pure::functions::collection::count";
    ADD = "meta::pure::functions::collection::add";
    ZIP = "meta::pure::functions::collection::zip";
    AND = "meta::pure::functions::boolean::and";
    OR = "meta::pure::functions::boolean::or";
    TIMES = "meta::pure::functions::math::times";
    MINUS = "meta::pure::functions::math::minus";
    RANK = "meta::pure::functions::relation::rank";
    DENSE_RANK = "meta::pure::functions::relation::denseRank";
    ROW_NUMBER = "meta::pure::functions::relation::rowNumber";
    INSTANCE_OF = "meta::pure::functions::meta::instanceOf";
    TO_STRING = "meta::pure::functions::string::toString";
    STRING_REPLACE = "meta::pure::functions::string::replace";
    STRING_MAKE_STRING = "meta::pure::functions::string::makeString";
    STRING_JOIN_STRINGS = "meta::pure::functions::string::joinStrings";
    COLLECTION_ZIP = "meta::pure::functions::collection::zip";
    RELATIONAL_EXECUTION_CONTEXT = "meta::relational::runtime::RelationalExecutionContext";
    EXECUTION_OPTION_CONTEXT = "meta::pure::executionPlan::ExecutionOptionContext";
    FEATURE_FLAG_OPTION = "meta::pure::executionPlan::featureFlag::FeatureFlagOption";
    FEATURE = "meta::pure::executionPlan::features::Feature";
    NULL_ORDER = "meta::pure::functions::relation::NullOrder";
    RELATION_JOIN_STRINGS = "meta::pure::functions::relation::joinStrings";
    RELATION_SIZE = "meta::pure::functions::relation::size";
    DURATION = "meta::pure::functions::date::Duration";
    MAP = "meta::pure::functions::collection::map";
    PLUS = "meta::pure::functions::math::plus";
    STRING_PLUS = "meta::pure::functions::string::plus";
    ASSERTS_PACKAGE = "meta::pure::functions::asserts::";
    // Java: ASSERTS_PACKAGE + "assertEquals"
    ASSERT_EQUALS = "meta::pure::functions::asserts::assertEquals";
    ASSERT_SAME_SQL = "meta::relational::functions::asserts::assertSameSQL";
    ASSERT_SQL_EQUALS_TDG = "meta::relational::testDataGeneration::tests::assertSqlEquals";
    ASSERT_EQUALS_DUAL_GOLDEN = "meta::relational::functions::sqlQueryToString::h2::assertEqualsH2Compatible";
    ASSERT_TDS_EQUIVALENT = "meta::pure::functions::relation::assertTdsEquivalent";
    FORMAT = "meta::pure::functions::string::format";
    PRINT = "meta::pure::functions::io::print";
    PRINTLN = "meta::pure::functions::io::println";
    JSON_ELEMENT = "meta::json::JSONElement";
    JSON_OBJECT = "meta::json::JSONObject";
    JSON_ARRAY = "meta::json::JSONArray";
    JSON_STRING = "meta::json::JSONString";
    JSON_NUMBER = "meta::json::JSONNumber";
    JSON_BOOLEAN = "meta::json::JSONBoolean";
    JSON_NULL = "meta::json::JSONNull";
    JSON_KEY_VALUE = "meta::json::JSONKeyValue";
    CONNECTION_BY_ELEMENT = "meta::core::runtime::connectionByElement";
}

/// The m3 function carriers (`Function`, `FunctionDefinition`, `LambdaFunction`, `ConcreteFunctionDefinition`):
/// each a wrapper spelling of the function type it carries. A set in Java; only membership is ever asked.
pub const FUNCTION_CARRIERS: &[&str] = &[
    FUNCTION,
    FUNCTION_DEFINITION,
    LAMBDA_FUNCTION,
    CONCRETE_FUNCTION_DEFINITION,
];

/// Every raw class whose one argument is a relation schema: the relation and its accessors, and `TDS<T>`.
pub const RELATION_CARRIERS: &[&str] = &[
    RELATION,
    RELATION_ELEMENT_ACCESSOR,
    RELATION_STORE_ACCESSOR,
    TDS_RELATION_ACCESSOR,
    TDS_RELATION_CLASS,
];

/// The `meta::json` tree classes (json.pure:32-70).
pub const JSON_FAMILY: &[&str] = &[
    JSON_ELEMENT,
    JSON_OBJECT,
    JSON_ARRAY,
    JSON_STRING,
    JSON_NUMBER,
    JSON_BOOLEAN,
    JSON_NULL,
    JSON_KEY_VALUE,
];

/// Every set above by its Java name.
pub const NAMED_SETS: &[(&str, &[&str])] = &[
    ("FUNCTION_CARRIERS", FUNCTION_CARRIERS),
    ("RELATION_CARRIERS", RELATION_CARRIERS),
    ("JSON_FAMILY", JSON_FAMILY),
];

/// The value carriers named by their raw class (Java's private `VALUE_CARRIER_FQNS`).
const VALUE_CARRIER_FQNS: &[&str] = &[
    RESULT,
    VARIANT,
    COL_SPEC,
    COL_SPEC_ARRAY,
    FUNC_COL_SPEC,
    FUNC_COL_SPEC_ARRAY,
    AGG_COL_SPEC,
    AGG_COL_SPEC_ARRAY,
];

const MAP_CARRIER: &str = "meta::pure::functions::collection::Map";

/// The metamodel type classifiers: a value of one is a type value, which travels as its path.
pub fn is_type_classifier(fqn: &str) -> bool {
    matches!(
        fqn,
        CLASS_METACLASS
            | "meta::pure::metamodel::type::Type"
            | "meta::pure::metamodel::type::PrimitiveType"
            | "meta::pure::metamodel::type::DataType"
            | "meta::pure::metamodel::type::Enumeration"
    )
}

/// A lambda literal's classifier: `LambdaFunction<ft>`.
pub fn lambda_type<'a, A: Alloc>(arena: &'a A, ft: &'a FunctionType<'a>) -> Type<'a> {
    Type::GenericType(GenericType::new(arena, LAMBDA_FUNCTION, &[Type::FunctionType(ft)], &[]))
}

/// A reference to a body-bearing user function's classifier: `ConcreteFunctionDefinition<ft>`.
pub fn concrete_function_definition_type<'a, A: Alloc>(arena: &'a A, ft: &'a FunctionType<'a>) -> Type<'a> {
    Type::GenericType(GenericType::new(
        arena,
        CONCRETE_FUNCTION_DEFINITION,
        &[Type::FunctionType(ft)],
        &[],
    ))
}

/// The function type a function-valued type carries: itself, or the one argument of a carrier spelling;
/// `None` otherwise (a carrier over a nominal argument included).
pub fn function_type_of(t: Type<'_>) -> Option<&FunctionType<'_>> {
    match t {
        Type::FunctionType(ft) => Some(ft),
        Type::GenericType(g) => match g.arguments {
            [Type::FunctionType(ft)] => Some(ft),
            _ => None,
        },
        _ => None,
    }
}

/// A TDS-shaped type: a relation, the TDS carrier, or the `TDS<T>` relation class (a cast between them is a no-op).
pub fn is_tds_shaped(t: Type<'_>) -> bool {
    t.is_relation() || is_tds_type(t) || matches!(t, Type::GenericType(g) if g.raw_fqn == TDS_RELATION_CLASS)
}

/// The TDS carrier, `TabularDataSet`, bare or parameterized.
pub fn is_tds_type(t: Type<'_>) -> bool {
    match t {
        Type::ClassType(fqn) => fqn == TABULAR_DATA_SET,
        Type::GenericType(g) => g.raw_fqn == TABULAR_DATA_SET,
        _ => false,
    }
}

pub fn is_store_nav_fn(fqn: &str) -> bool {
    fqn == STORE_SCHEMA_NAV || fqn == STORE_TABLE_NAV
}

/// The three relational connection classes (exact FQN).
pub fn is_relational_connection_class(fqn: &str) -> bool {
    fqn == DATABASE_CONNECTION || fqn == RELATIONAL_DATABASE_CONNECTION || fqn == TEST_DATABASE_CONNECTION
}

/// The plan-text simple name of a relational connection class, given its FQN or its bare name.
pub fn relational_connection_simple_name(name_or_fqn: &str) -> Option<&'static str> {
    match name_or_fqn {
        DATABASE_CONNECTION | "DatabaseConnection" => Some("DatabaseConnection"),
        RELATIONAL_DATABASE_CONNECTION | "RelationalDatabaseConnection" => Some("RelationalDatabaseConnection"),
        TEST_DATABASE_CONNECTION | "TestDatabaseConnection" => Some("TestDatabaseConnection"),
        _ => None,
    }
}

pub fn is_model_chain_connection(name_or_fqn: &str) -> bool {
    name_or_fqn == MODEL_CHAIN_CONNECTION || name_or_fqn == "ModelChainConnection"
}

pub fn is_json_model_connection(name_or_fqn: &str) -> bool {
    name_or_fqn == JSON_MODEL_CONNECTION || name_or_fqn == "JsonModelConnection"
}

/// The string or number `+` in a raw body: bare, or either FQN.
pub fn is_plus(name_or_fqn: &str) -> bool {
    name_or_fqn == PLUS || name_or_fqn == STRING_PLUS || name_or_fqn == "plus"
}

/// A call adjudicated as a verdict: the asserts package, and the verdict functions named outside it.
pub fn is_verdict_function(fqn: &str) -> bool {
    fqn.starts_with(ASSERTS_PACKAGE)
        || fqn == ASSERT_SAME_SQL
        || fqn == ASSERT_SQL_EQUALS_TDG
        || fqn == ASSERT_EQUALS_DUAL_GOLDEN
        || fqn == ASSERT_TDS_EQUIVALENT
}

/// Whether a stereotype's resolved profile name is `profile_fqn`.
pub fn is_profile(resolved_name: &str, profile_fqn: &str) -> bool {
    profile_fqn == resolved_name
}

/// A value whose text is its class's `toString()`: an instance of a class (not `Any`, `Nil` or a variant) or of a
/// `Pair` or `List` carrier.
pub fn prints_by_own_to_string(t: Type<'_>) -> bool {
    match t {
        Type::ClassType(_) => !is_any(t) && !is_nil(t) && !is_variant(t) && t.schema_view().is_none(),
        Type::GenericType(_) => is_pair_carrier(t) || is_list_carrier(t),
        _ => false,
    }
}

/// Post-processor configuration property names: plan-time SQL-rewrite configuration, never Pure.
pub fn is_post_processor_config_property(name: &str) -> bool {
    matches!(
        name,
        "sqlQueryPostProcessors" | "sqlQueryPostProcessorsConnectionAware" | "queryPostProcessorsWithParameter"
    )
}

/// The top type.
pub fn is_any(t: Type<'_>) -> bool {
    t == Type::ClassType(ANY)
}

/// Whether `g` is the metaclass `Class<...>`.
pub fn is_class_metaclass(g: &GenericType<'_>) -> bool {
    g.raw_fqn == CLASS_METACLASS
}

/// A type value's wire form, its path as `elementToPath` spells it: a base primitive's is its name, every other
/// element's its FQN.
pub fn type_value_path_of(fqn: &str) -> &str {
    match Primitive::find_by_fqn(fqn) {
        Some(p) if p.qualified_name() == fqn => p.type_name(),
        _ => fqn,
    }
}

/// [`type_value_path_of`] the type's name; a primitive's own name.
pub fn type_value_path(t: Type<'_>) -> String {
    match t {
        Type::Primitive(p) => p.type_name().to_owned(),
        _ => type_value_path_of(&t.type_name()).to_owned(),
    }
}

/// The bottom type, the element type of `[]`.
pub fn is_nil(t: Type<'_>) -> bool {
    t == Type::ClassType(NIL)
}

/// The semi-structured carrier: `Variant`, and the `meta::json` tree classes, whose values ride it.
pub fn is_variant(t: Type<'_>) -> bool {
    matches!(t, Type::ClassType(fqn) if fqn == VARIANT || JSON_FAMILY.contains(&fqn))
}

/// A `meta::json` tree class.
pub fn is_json_element(t: Type<'_>) -> bool {
    matches!(t, Type::ClassType(fqn) if JSON_FAMILY.contains(&fqn))
}

/// The `List<T>` carrier.
pub fn is_list_carrier(t: Type<'_>) -> bool {
    matches!(t, Type::GenericType(g) if g.raw_fqn == LIST && g.arguments.len() == 1)
}

/// The `Pair<U,V>` carrier.
pub fn is_pair_carrier(t: Type<'_>) -> bool {
    matches!(t, Type::GenericType(g) if g.raw_fqn == PAIR && g.arguments.len() == 2)
}

/// The `Map<U,V>` carrier.
pub fn is_map_carrier(t: Type<'_>) -> bool {
    matches!(t, Type::GenericType(g) if g.raw_fqn == MAP_CARRIER && g.arguments.len() == 2)
}

/// A parameterized platform value carrier -- a struct the lowering carries, a plan handle, a variant, a column
/// specification, a function carrier -- never a row the store serves.
pub fn is_value_carrier(t: Type<'_>) -> bool {
    let Type::GenericType(g) = t else { return false };
    is_pair_carrier(t)
        || is_list_carrier(t)
        || is_map_carrier(t)
        || FUNCTION_CARRIERS.contains(&g.raw_fqn)
        || VALUE_CARRIER_FQNS.contains(&g.raw_fqn)
}

/// The `Function<{…}>` carrier.
pub fn is_function_carrier(t: Type<'_>) -> bool {
    matches!(t, Type::GenericType(g) if g.raw_fqn == FUNCTION)
}
