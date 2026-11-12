// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! `NativeFn`: the native function families -- the catalog's natives grouped by the code that implements them, each
//! family a closed type whose members carry their FQN and catalog overloads ([`NativeId`]s, so a constant the catalog
//! does not have does not compile). The sibling of the platform's `CoreFn` (the language forms with their own checker):
//! a `CoreFn` is a form, a `NativeFn` member a native with an implementer. The claim registry reads [`Family::VALUES`]
//! (Java's `families()`): the enum is the set and the dispatch key, so nothing beside the code can drift, and no compiler
//! code dispatches on a function-name string.
//!
//! Ported from core's `com.legend.builtin.NativeFn` (2026-11-12, docs/rust-port/PHASE3.md slice 1b), member for member
//! in Java's order, each Java constant `Pure.X` the generated `natives::X`. Java's `BY_ID` maps (`indexById`: every
//! member by the identity of each of its overloads) are hash maps here too, built on a family's first lookup; a member
//! in two of them is the platform's bug, a panic with Java's message (Java's `ExceptionInInitializerError`). A Java
//! `@Nullable` argument is an `Option`. Held to Java's answers by `tests/builtin_tables.rs` (the `nativefn` sections).

use std::collections::{HashMap, HashSet};
use std::sync::LazyLock;

use lite_model::{ClassMember, FunctionId, JStr};

use crate::natives;
use crate::pure::{NativeId, lite};

/// `NativeFn.Member`: one member of a family -- the FQN it implements and every catalog overload of it.
pub trait Member: Copy + std::fmt::Debug + Send + Sync + 'static {
    /// The member's Java name (`name()`).
    fn name(self) -> &'static str;

    /// The FQN the member implements.
    fn fqn(self) -> &'static str;

    /// Every catalog overload of it, in order.
    fn overloads(self) -> &'static [NativeId];

    /// The bare function name (`assertEquals`), for messages: the FQN after its last `:`.
    fn bare_name(self) -> &'static str {
        let f = self.fqn();
        &f[f.rfind(':').map_or(0, |i| i + 1)..]
    }

    /// Whether an applied name (bare, or qualified) spells this member.
    fn matches(self, applied: &str) -> bool {
        applied == self.fqn()
            || applied == self.bare_name()
            || applied
                .strip_suffix(self.bare_name())
                .is_some_and(|head| head.ends_with("::"))
    }
}

/// `NativeFn.index(values)`: FQN -> member (a duplicate FQN's last member wins, as Java's `HashMap.put` does). Java
/// declares it and calls it nowhere; ported for the record.
pub fn index<M: Member>(values: &[M]) -> HashMap<&'static str, M> {
    values.iter().map(|&m| (m.fqn(), m)).collect()
}

/// `NativeFn.indexById(values)`: every member by the identity of each of its catalog overloads -- the dispatch key (a
/// resolved callee is looked up by [`FunctionId`], never by name). An overload in two members panics with Java's
/// message; the same member twice is no conflict.
pub fn index_by_id<M: Member + PartialEq>(values: &[M]) -> HashMap<&'static FunctionId, M> {
    let mut out = HashMap::new();
    for &m in values {
        for o in m.overloads() {
            if let Some(prior) = out.insert(o.id(), m)
                && prior != m
            {
                panic!(
                    "{} in two members: {}, {}",
                    o.def().qualified_name,
                    prior.name(),
                    m.name()
                );
            }
        }
    }
    out
}

/// One family: the enum (Java's constant names), its `values()`, and its [`Member`] answers.
macro_rules! family {
    (
        $(#[$doc:meta])*
        $fam:ident {
            $( $(#[$vdoc:meta])* $v:ident($fqn:expr $(, $o:ident)* $(,)?) ),+ $(,)?
        }
    ) => {
        $(#[$doc])*
        #[allow(non_camel_case_types)]
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
        pub enum $fam {
            $( $(#[$vdoc])* $v ),+
        }

        impl $fam {
            /// Java's `values()`, in declaration order.
            pub const VALUES: &'static [$fam] = &[$($fam::$v),+];
        }

        impl Member for $fam {
            fn name(self) -> &'static str {
                match self {
                    $($fam::$v => stringify!($v)),+
                }
            }

            fn fqn(self) -> &'static str {
                match self {
                    $($fam::$v => $fqn),+
                }
            }

            fn overloads(self) -> &'static [NativeId] {
                match self {
                    $($fam::$v => &[$(natives::$o),*]),+
                }
            }
        }
    };
}

/// A family's `BY_ID` (`indexById(values())`) and its `of(id)`.
macro_rules! by_id {
    ($fam:ident) => {
        impl $fam {
            /// Java's `BY_ID`: every member by the identity of each of its overloads, built on first use.
            fn by_id() -> &'static HashMap<&'static FunctionId, $fam> {
                static BY_ID: LazyLock<HashMap<&'static FunctionId, $fam>> =
                    LazyLock::new(|| index_by_id($fam::VALUES));
                &BY_ID
            }

            /// `of(id)`: the member whose overloads include the declaration `id`, or none.
            pub fn of(id: Option<&FunctionId>) -> Option<$fam> {
                id.and_then(|id| $fam::by_id().get(id).copied())
            }
        }
    };
}

family! {
    /// `NativeFn.Calendar`: the 32 calendar natives CalendarAgg lowers to SQL calendar aggregation.
    Calendar {
        CY_MINUS2("meta::pure::functions::date::calendar::CYMinus2", CAL_C_Y_MINUS2),
        CY_MINUS3("meta::pure::functions::date::calendar::CYMinus3", CAL_C_Y_MINUS3),
        ANNUALIZED("meta::pure::functions::date::calendar::annualized", CAL_ANNUALIZED),
        CME("meta::pure::functions::date::calendar::cme", CAL_CME),
        CW("meta::pure::functions::date::calendar::cw", CAL_CW),
        CW_FM("meta::pure::functions::date::calendar::cw_fm", CAL_CW_FM),
        MTD("meta::pure::functions::date::calendar::mtd", CAL_MTD),
        P12MTD("meta::pure::functions::date::calendar::p12mtd", CAL_P12MTD),
        P12WA("meta::pure::functions::date::calendar::p12wa", CAL_P12WA),
        P12WTD("meta::pure::functions::date::calendar::p12wtd", CAL_P12WTD),
        P4WA("meta::pure::functions::date::calendar::p4wa", CAL_P4WA),
        P4WTD("meta::pure::functions::date::calendar::p4wtd", CAL_P4WTD),
        P52WA("meta::pure::functions::date::calendar::p52wa", CAL_P52WA),
        P52WTD("meta::pure::functions::date::calendar::p52wtd", CAL_P52WTD),
        PMA("meta::pure::functions::date::calendar::pma", CAL_PMA),
        PMTD("meta::pure::functions::date::calendar::pmtd", CAL_PMTD),
        PQTD("meta::pure::functions::date::calendar::pqtd", CAL_PQTD),
        PRIOR_DAY("meta::pure::functions::date::calendar::priorDay", CAL_PRIOR_DAY),
        PRIOR_YEAR("meta::pure::functions::date::calendar::priorYear", CAL_PRIOR_YEAR),
        PW("meta::pure::functions::date::calendar::pw", CAL_PW),
        PW_FM("meta::pure::functions::date::calendar::pw_fm", CAL_PW_FM),
        PWA("meta::pure::functions::date::calendar::pwa", CAL_PWA),
        PWTD("meta::pure::functions::date::calendar::pwtd", CAL_PWTD),
        PYMTD("meta::pure::functions::date::calendar::pymtd", CAL_PYMTD),
        PYQTD("meta::pure::functions::date::calendar::pyqtd", CAL_PYQTD),
        PYTD("meta::pure::functions::date::calendar::pytd", CAL_PYTD),
        PYWA("meta::pure::functions::date::calendar::pywa", CAL_PYWA),
        PYWTD("meta::pure::functions::date::calendar::pywtd", CAL_PYWTD),
        QTD("meta::pure::functions::date::calendar::qtd", CAL_QTD),
        REPORT_END_DAY("meta::pure::functions::date::calendar::reportEndDay", CAL_REPORT_END_DAY),
        WTD("meta::pure::functions::date::calendar::wtd", CAL_WTD),
        YTD("meta::pure::functions::date::calendar::ytd", CAL_YTD),
    }
}
by_id!(Calendar);

family! {
    /// `NativeFn.Verdict`: the asserts AssertVerdicts adjudicates as row verdicts, plus the two relation verdict forms
    /// it owns.
    Verdict {
        ASSERT_EQUALS("meta::pure::functions::asserts::assertEquals",
            ASSERT_EQUALS__ANY_MANY__ANY_MANY, ASSERT_EQUALS__ANY_MANY__ANY_MANY__STRING_1,
            ASSERT_EQUALS__ANY_MANY__ANY_MANY__STRING_1__ANY_MANY, ASSERT_EQUALS__ANY_MANY__ANY_MANY__FN_1),
        ASSERT_NOT_EQUALS("meta::pure::functions::asserts::assertNotEquals",
            ASSERT_NOT_EQUALS__ANY_MANY__ANY_MANY, ASSERT_NOT_EQUALS__ANY_MANY__ANY_MANY__STRING_1,
            ASSERT_NOT_EQUALS__ANY_MANY__ANY_MANY__STRING_1__ANY_MANY, ASSERT_NOT_EQUALS__ANY_MANY__ANY_MANY__FN_1),
        ASSERT_SAME_ELEMENTS("meta::pure::functions::asserts::assertSameElements",
            ASSERT_SAME_ELEMENTS__ANY_MANY__ANY_MANY, ASSERT_SAME_ELEMENTS__ANY_MANY__ANY_MANY__STRING_1,
            ASSERT_SAME_ELEMENTS__ANY_MANY__ANY_MANY__STRING_1__ANY_MANY, ASSERT_SAME_ELEMENTS__ANY_MANY__ANY_MANY__FN_1),
        ASSERT_SIZE("meta::pure::functions::asserts::assertSize",
            ASSERT_SIZE__ANY_MANY__INTEGER_1, ASSERT_SIZE__ANY_MANY__INTEGER_1__STRING_1,
            ASSERT_SIZE__ANY_MANY__INTEGER_1__STRING_1__ANY_MANY, ASSERT_SIZE__ANY_MANY__INTEGER_1__FN_1),
        ASSERT_JSON_STRINGS_EQUAL("meta::pure::functions::asserts::assertJsonStringsEqual",
            ASSERT_JSON_STRINGS_EQUAL__STRING_1__STRING_1),
        ASSERT_CONTAINS("meta::pure::functions::asserts::assertContains",
            ASSERT_CONTAINS__ANY_MANY__ANY_1, ASSERT_CONTAINS__ANY_MANY__ANY_1__STRING_1,
            ASSERT_CONTAINS__ANY_MANY__ANY_1__STRING_1__ANY_MANY, ASSERT_CONTAINS__ANY_MANY__ANY_1__FN_1),
        ASSERT_EQ("meta::pure::functions::asserts::assertEq",
            ASSERT_EQ__ANY_1__ANY_1, ASSERT_EQ__ANY_1__ANY_1__STRING_1, ASSERT_EQ__ANY_1__ANY_1__STRING_1__ANY_MANY,
            ASSERT_EQ__ANY_1__ANY_1__FN_1),
        ASSERT_EQ_WITHIN_TOLERANCE("meta::pure::functions::asserts::assertEqWithinTolerance",
            ASSERT_EQ_WITHIN_TOLERANCE__NUMBER_1__NUMBER_1__NUMBER_1, ASSERT_EQ_WITHIN_TOLERANCE__N_1__N_1__N_1__STRING_1,
            ASSERT_EQ_WITHIN_TOLERANCE__N_1__N_1__N_1__STRING_1__ANY_MANY,
            ASSERT_EQ_WITHIN_TOLERANCE__N_1__N_1__N_1__FN_1),
        ASSERT("meta::pure::functions::asserts::assert",
            ASSERT__BOOLEAN_1, ASSERT__BOOLEAN_1__STRING_1, ASSERT__BOOLEAN_1__FN_1, ASSERT__BOOLEAN_1__STRING_1__ANY_MANY),
        ASSERT_FALSE("meta::pure::functions::asserts::assertFalse",
            ASSERT_FALSE__BOOLEAN_1, ASSERT_FALSE__BOOLEAN_1__STRING_1, ASSERT_FALSE__BOOLEAN_1__STRING_1__ANY_MANY,
            ASSERT_FALSE__BOOLEAN_1__FN_1),
        ASSERT_INSTANCE_OF("meta::pure::functions::asserts::assertInstanceOf",
            ASSERT_INSTANCE_OF__ANY_1__TYPE_1, ASSERT_INSTANCE_OF__ANY_1__TYPE_1__STRING_1,
            ASSERT_INSTANCE_OF__ANY_1__TYPE_1__STRING_1__ANY_MANY, ASSERT_INSTANCE_OF__ANY_1__TYPE_1__FN_1),
        ASSERT_IS("meta::pure::functions::asserts::assertIs",
            ASSERT_IS__ANY_1__ANY_1, ASSERT_IS__ANY_1__ANY_1__STRING_1, ASSERT_IS__ANY_1__ANY_1__STRING_1__ANY_MANY,
            ASSERT_IS__ANY_1__ANY_1__FN_1),
        ASSERT_EMPTY("meta::pure::functions::asserts::assertEmpty",
            ASSERT_EMPTY__ANY_MANY, ASSERT_EMPTY__ANY_MANY__STRING_1, ASSERT_EMPTY__ANY_MANY__STRING_1__ANY_MANY,
            ASSERT_EMPTY__ANY_MANY__FN_1),
        ASSERT_NOT_EMPTY("meta::pure::functions::asserts::assertNotEmpty",
            ASSERT_NOT_EMPTY__ANY_MANY, ASSERT_NOT_EMPTY__ANY_MANY__STRING_1,
            ASSERT_NOT_EMPTY__ANY_MANY__STRING_1__ANY_MANY, ASSERT_NOT_EMPTY__ANY_MANY__FN_1),
        ASSERT_TDS_EQUIVALENT("meta::pure::functions::relation::assertTdsEquivalent",
            ASSERT_TDS_EQUIVALENT__REL_1__REL_1__NUMBER_1, ASSERT_TDS_EQUIVALENT__REL_1__REL_1__NUMBER_1__NUMBER_1),
        TO_CSV("meta::relational::tests::csv::toCSV", TO_CSV__TDS, TO_CSV__TDS_BOOL, TO_CSV__TDS_FMT),
    }
}
by_id!(Verdict);

family! {
    /// `NativeFn.Frame`: the window-frame keywords Frames classifies and the `over()` checker consumes by type.
    Frame {
        ROWS("meta::pure::functions::relation::rows",
            ROWS__INTEGER_1__INTEGER_1, ROWS__UNBOUNDED_1__UNBOUNDED_1, ROWS__UNBOUNDED_1__INTEGER_1,
            ROWS__INTEGER_1__UNBOUNDED_1),
        RANGE("meta::pure::functions::relation::_range",
            _RANGE__NUMBER_1__NUMBER_1, _RANGE__UNBOUNDED_1__NUMBER_1, _RANGE__NUMBER_1__UNBOUNDED_1,
            _RANGE__INT_1__DU_1__INT_1__DU_1, _RANGE__UNBOUNDED_1__INT_1__DU_1, _RANGE__INT_1__DU_1__UNBOUNDED_1,
            _RANGE__UNBOUNDED_1__UNBOUNDED_1),
        UNBOUNDED("meta::pure::functions::relation::unbounded", UNBOUNDED),
    }
}
by_id!(Frame);

family! {
    /// `NativeFn.LowererForm`: relation forms the Lowerer lowers structurally (lateral joins, windowed map+reduce,
    /// composed window expressions, bi-variate aggregate maps).
    LowererForm {
        LATERAL("meta::pure::functions::relation::lateral", LATERAL__RELATION_1__FUNCTION_1),
        REDUCE("meta::pure::functions::relation::reduce", REDUCE__RELATION_1__WINDOW_1__T_1__FUNCTION_1__FUNCTION_1),
        Z_SCORE("meta::pure::functions::math::zScore", Z_SCORE__WINDOW),
        ROW_MAPPER("meta::pure::functions::math::mathUtility::rowMapper", ROW_MAPPER__T_0_1__U_0_1),
        WAVG_ROW_MAPPER("meta::pure::functions::math::wavgUtility::wavgRowMapper",
            WAVG_ROW_MAPPER__NUMBER_0_1__NUMBER_0_1),
    }
}
by_id!(LowererForm);

impl LowererForm {
    /// `isBivariateMap(id)`: rowMapper / wavgRowMapper -- the bi-variate aggregate map bodies.
    pub fn is_bivariate_map(id: &FunctionId) -> bool {
        matches!(
            LowererForm::by_id().get(id),
            Some(LowererForm::ROW_MAPPER | LowererForm::WAVG_ROW_MAPPER)
        )
    }
}

family! {
    /// `NativeFn.LiteralForm`: calls LiteralUnroll rewrites at type-check over spelled operands.
    LiteralForm {
        DYNAMIC_NEW("meta::pure::functions::lang::dynamicNew",
            DYNAMIC_NEW__CLASS_5, DYNAMIC_NEW__CLASS_6, DYNAMIC_NEW__GENERIC_5, DYNAMIC_NEW__GENERIC_6,
            DYNAMIC_NEW__CLASS_1__KEYVALUE_MANY, DYNAMIC_NEW__GENERICTYPE_1__KEYVALUE_MANY),
        ENUM_VALUES("meta::pure::functions::meta::enumValues", ENUM_VALUES),
        KEY_VALUES("meta::pure::functions::collection::keyValues", KEY_VALUES__MAP_1),
    }
}
by_id!(LiteralForm);

family! {
    /// `NativeFn.ContextOption`: the execute-context post-processors ContextReading reads as options of the plan.
    ContextOption {
        EXTRACT_SUBQUERIES_AS_CTES("meta::relational::postProcessor::cteExtraction::extractSubqueriesAsCTEs",
            EXTRACT_CTES),
        NON_EXECUTABLE("meta::relational::postProcessor::nonExecutable", NON_EXECUTABLE_PP),
        REPLACE_TABLES("meta::relational::postProcessor::replaceTables", REPLACE_TABLES),
    }
}
by_id!(ContextOption);

family! {
    /// `NativeFn.RelationQuantifier`: the quantification family (engine core_functions_relation/quantification,
    /// 4.145.0) -- a value tested against a single-column relation: `relation::in`, the ten quantified comparisons
    /// (`equal/greaterThan/greaterThanEqual/lessThan/lessThanEqual` x `Any/All`) and `relation::exists(rel, predicate)`,
    /// lowered by RelationPredicates as SQL subquery predicates (IN / op ANY|ALL / EXISTS).
    RelationQuantifier {
        EXISTS("meta::pure::functions::relation::exists", EXISTS__RELATION_1__FUNCTION_1),
        IN("meta::pure::functions::relation::in", IN__U_0_1__RELATION_1),
        EQUAL_ANY("meta::pure::functions::relation::equalAny", EQUAL_ANY__U_0_1__RELATION_1),
        EQUAL_ALL("meta::pure::functions::relation::equalAll", EQUAL_ALL__U_0_1__RELATION_1),
        GREATER_THAN_ANY("meta::pure::functions::relation::greaterThanAny", GREATER_THAN_ANY__U_0_1__RELATION_1),
        GREATER_THAN_ALL("meta::pure::functions::relation::greaterThanAll", GREATER_THAN_ALL__U_0_1__RELATION_1),
        GREATER_THAN_EQUAL_ANY("meta::pure::functions::relation::greaterThanEqualAny",
            GREATER_THAN_EQUAL_ANY__U_0_1__RELATION_1),
        GREATER_THAN_EQUAL_ALL("meta::pure::functions::relation::greaterThanEqualAll",
            GREATER_THAN_EQUAL_ALL__U_0_1__RELATION_1),
        LESS_THAN_ANY("meta::pure::functions::relation::lessThanAny", LESS_THAN_ANY__U_0_1__RELATION_1),
        LESS_THAN_ALL("meta::pure::functions::relation::lessThanAll", LESS_THAN_ALL__U_0_1__RELATION_1),
        LESS_THAN_EQUAL_ANY("meta::pure::functions::relation::lessThanEqualAny",
            LESS_THAN_EQUAL_ANY__U_0_1__RELATION_1),
        LESS_THAN_EQUAL_ALL("meta::pure::functions::relation::lessThanEqualAll",
            LESS_THAN_EQUAL_ALL__U_0_1__RELATION_1),
    }
}
by_id!(RelationQuantifier);

family! {
    /// `NativeFn.GroupAggregate`: the group-lambda aggregates (engine core_functions_relation joinStrings.pure,
    /// 4.145.0) -- `relation::joinStrings(rel, col|f, separator[, sortInfos])`, written over the group inside a
    /// groupBy/aggregate FuncColSpec and desugared by GroupLambdaAggs to the map/reduce aggregate form whose reducer is
    /// the ordered `string_agg`; never lowered as a call. Java gives it no `BY_ID` and no `of`.
    GroupAggregate {
        JOIN_STRINGS("meta::pure::functions::relation::joinStrings",
            JOIN_STRINGS__RELATION_1__COL_SPEC_1__STRING_1, JOIN_STRINGS__RELATION_1__COL_SPEC_1__STRING_1__SORT_INFO_MANY,
            JOIN_STRINGS__RELATION_1__FUNCTION_1__STRING_1, JOIN_STRINGS__RELATION_1__FUNCTION_1__STRING_1__SORT_INFO_MANY),
    }
}

family! {
    /// `NativeFn.PlanWrapper`: plan-time wrappers ExecuteChainAssembly / StoreResolver read through (identity for row
    /// semantics) or fold.
    PlanWrapper {
        WITH_FEATURE_FLAGS("meta::pure::executionPlan::featureFlag::withFeatureFlags",
            WITH_FEATURE_FLAGS__T_MANY__ENUM_MANY),
        CONCATENATE_TEMPORAL_TDS_QUERIES("meta::relational::milestoning::concatenateTemporalTdsQueries",
            CONCATENATE_TEMPORAL_TDS_QUERIES),
    }
}
by_id!(PlanWrapper);

family! {
    /// `NativeFn.ObjectReference`: the object-reference encode / decode / membership arms of the resolver.
    ObjectReference {
        GENERATE("meta::alloy::objectReference::generateObjectReferences", GENERATE_OBJECT_REFERENCES__6),
        GENERATE_FOR_SET("meta::alloy::objectReference::generateObjectReferencesForGivenSetId",
            GENERATE_OBJECT_REFERENCES_FOR_GIVEN_SET_ID__7),
        DECODE("meta::alloy::objectReference::decodeObjectReferencesAndGetPkMap", DECODE_OBJECT_REFERENCES__3),
        OBJECT_REFERENCE_IN("meta::pure::functions::collection::objectReferenceIn",
            OBJECT_REFERENCE_IN__ANY_1__STRING_MANY),
    }
}
by_id!(ObjectReference);

family! {
    /// `NativeFn.SubtypeForm`: type-narrowing forms the resolver and the verdicts read structurally (subType,
    /// whenSubType, instanceOf, genericType).
    SubtypeForm {
        SUB_TYPE("meta::pure::functions::lang::subType", SUB_TYPE__ANY_m__T_1),
        WHEN_SUB_TYPE("meta::pure::functions::lang::whenSubType",
            WHEN_SUB_TYPE__ANY_1__T_1, WHEN_SUB_TYPE__ANY_01__T_1, WHEN_SUB_TYPE__ANY_MANY__T_1),
        INSTANCE_OF("meta::pure::functions::meta::instanceOf", INSTANCE_OF__ANY_1__TYPE_1),
        GENERIC_TYPE("meta::pure::functions::meta::genericType", GENERIC_TYPE__ANY_MANY),
    }
}
by_id!(SubtypeForm);

family! {
    /// `NativeFn.ResolverForm`: forms single resolver / checker sites rewrite: tdsContains (EXISTS), variant get,
    /// alloyConfig, the from() mapping markers, the relational-mapper post-processor, a mapped instance's identity, the
    /// class writes.
    ResolverForm {
        TDS_CONTAINS("meta::pure::tds::tdsContains",
            TDS_CONTAINS__T_1__FUNCTION_MANY__TDS_1, TDS_CONTAINS__T_1__FUNCTION_MANY__STRING_MANY__TDS_1__FUNCTION_1),
        VARIANT_GET("meta::pure::functions::variant::navigation::get",
            GET__VARIANT_0_1__STRING_1, GET__VARIANT_0_1__INTEGER_1),
        ALLOY_CONFIG("meta::pure::graphFetch::execution::alloyConfig",
            ALLOY_CONFIG__4, ALLOY_CONFIG__5, ALLOY_CONFIG__6, ALLOY_CONFIG__7, ALLOY_CONFIG__8),
        WITH_MAPPING("meta::pure::mapping::withMapping", WITH_MAPPING),
        WITH_CHAINED_MAPPINGS("meta::pure::mapping::withChainedMappings", WITH_CHAINED_MAPPINGS),
        RELATIONAL_MAPPER_POST_PROCESSOR("meta::pure::alloy::connections::relationalMapperPostProcessor",
            RELATIONAL_MAPPER_PP),
        /// A mapped instance's identity as text, rewritten by the store resolver to the read of its class's primary
        /// key (the Cypher island's identity; claimed 2026-10-17).
        ELEMENT_ID(lite::ELEMENT_ID, ELEMENT_ID__ANY_0_1, ELEMENT_ID__ANY_MANY),
        /// The class writes over direct mappings (L2), each rewritten by the store resolver to L1's statement over its
        /// class's mapped table (resolver/ClassWrites; 2026-10-20).
        WRITE_INSTANCES(lite::WRITE_INSTANCES, WRITE_INSTANCES__RELATION_1__CLASS_1__COL_SPEC_1),
        MERGE_INSTANCES(lite::MERGE_INSTANCES, MERGE_INSTANCES__RELATION_1__CLASS_1__COL_SPEC_1),
        DELETE_INSTANCES(lite::DELETE_INSTANCES, DELETE_INSTANCES__RELATION_1__CLASS_1__COL_SPEC_1),
        LINK(lite::LINK, LINK__RELATION_1__CLASS_1__COL_SPEC_1__COL_SPEC_1),
        UNLINK(lite::UNLINK, UNLINK__RELATION_1__CLASS_1__COL_SPEC_1__COL_SPEC_1),
    }
}
by_id!(ResolverForm);

impl ResolverForm {
    /// `classWrite()`: whether this form is one of the class writes (L2), which the store resolver writes as L1's.
    pub fn class_write(self) -> bool {
        matches!(
            self,
            ResolverForm::WRITE_INSTANCES
                | ResolverForm::MERGE_INSTANCES
                | ResolverForm::DELETE_INSTANCES
                | ResolverForm::LINK
                | ResolverForm::UNLINK
        )
    }
}

family! {
    /// `NativeFn.LiteDesugar`: the platform's own desugar IR (`meta::legend::lite`) the mapping normalizer emits and
    /// the lowering consumes.
    LiteDesugar {
        OTHERWISE("meta::legend::lite::otherwise", OTHERWISE__T_1__T_0_1),
        LEGACY_ASSOC_PREDICATE("meta::legend::lite::legacyAssocPredicate",
            LEGACY_ASSOC_PREDICATE__A_1__B_1__RELATION_1__RELATION_1__FUNCTION_1,
            LEGACY_ASSOC_PREDICATE__A_1__B_1__STRING_1__STRING_1__FUNCTION_1),
        LEGACY_LOCAL_PROPERTY("meta::legend::lite::legacyLocalProperty", LEGACY_LOCAL_PROPERTY__ANY_1__STRING_1),
    }
}
by_id!(LiteDesugar);

family! {
    /// `NativeFn.TyperForm`: forms the Typer desugars against the registered signature (paginated -> slice;
    /// extractEnumValue over a literal name).
    TyperForm {
        PAGINATED("meta::pure::functions::collection::paginated", PAGINATED__T_MANY__INTEGER_1__INTEGER_1),
        EXTRACT_ENUM_VALUE("meta::pure::functions::lang::extractEnumValue",
            EXTRACT_ENUM_VALUE, EXTRACT_ENUM_VALUE__OPTIONAL),
        UNION("meta::pure::functions::collection::union", UNION__T_MANY__T_MANY),
    }
}
by_id!(TyperForm);

family! {
    /// `NativeFn.JavaRoutine`: natives the platform computes as a value at orchestration time (compiler-output
    /// surfaces: plan text, SQL text); staged by NativeDispatch, the result re-enters the statement as a bound literal.
    JavaRoutine {
        PLAN_TO_STRING("meta::pure::executionPlan::toString::planToString",
            PLAN_TO_STRING__EXECUTION_PLAN_1__EXTENSION_MANY),
        PLAN_TO_STRING_WITHOUT_FORMATTING("meta::pure::executionPlan::toString::planToStringWithoutFormatting",
            PLAN_TO_STRING_WITHOUT_FORMATTING__EXECUTION_PLAN_1__EXTENSION_MANY),
        /// Also implements `SQLResult.toSQLString(databaseType, dbTimeZone, quoteIdentifiers, format)` -- upstream's
        /// qualified property of the toSQL handle (toSQLString.pure), whose body renders engine SQLQuery objects this
        /// platform never builds: the lifted declaration types the call, the routine computes it.
        TO_SQL_STRING("meta::relational::functions::sqlstring::toSQLString",
            TO_SQL_STRING__FN_1__MAPPING_1__DATABASE_TYPE_1__EXTENSION_MANY),
        TO_SQL_STRING_PRETTY("meta::relational::functions::sqlstring::toSQLStringPretty",
            TO_SQL_STRING_PRETTY__FN_1__MAPPING_1__DATABASE_TYPE_1__EXTENSION_MANY,
            TO_SQL_STRING_PRETTY__FN_1__MAPPING_1__RUNTIME_1__EXTENSION_MANY),
        TO_NON_EXECUTABLE_SQL_STRING("meta::relational::functions::sqlstring::toNonExecutableSQLString",
            TO_NON_EXECUTABLE_SQL_STRING__FN_1__MAPPING_1__DATABASE_TYPE_1__EXTENSION_MANY),
    }
}
by_id!(JavaRoutine);

impl JavaRoutine {
    /// The qualified property this routine implements, as Java's constructor takes it (`derivedOwner`,
    /// `derivedProperty`; Java sets neither unless both are given), with its lifted FQN
    /// (`DerivedPropertyNames.lifted(owner, property)`, spelled out: a unit test holds it to the model's spelling).
    fn derived(self) -> Option<(&'static str, &'static str, &'static str)> {
        match self {
            JavaRoutine::TO_SQL_STRING => Some((
                "meta::relational::functions::sqlstring::SQLResult",
                "toSQLString",
                "meta::relational::functions::sqlstring::SQLResult$prop$toSQLString",
            )),
            JavaRoutine::PLAN_TO_STRING
            | JavaRoutine::PLAN_TO_STRING_WITHOUT_FORMATTING
            | JavaRoutine::TO_SQL_STRING_PRETTY
            | JavaRoutine::TO_NON_EXECUTABLE_SQL_STRING => None,
        }
    }

    /// `implementedMember()`: the class member (a qualified property) this routine implements, if any.
    pub fn implemented_member(self) -> Option<ClassMember> {
        self.derived().map(|(owner, property, _)| ClassMember {
            owner: JStr::from(owner),
            name: JStr::from(property),
        })
    }

    /// `implementedDerived()`: the lifted qualified property this routine implements, if any.
    pub fn implemented_derived(self) -> Option<&'static str> {
        self.derived().map(|(_, _, lifted)| lifted)
    }

    /// `ofDerived(liftedFqn)`: the routine implementing a lifted qualified-property callee, or none.
    pub fn of_derived(lifted_fqn: Option<&str>) -> Option<JavaRoutine> {
        JavaRoutine::VALUES
            .iter()
            .copied()
            .find(|r| r.implemented_derived().is_some_and(|d| Some(d) == lifted_fqn))
    }
}

family! {
    /// `NativeFn.Handle`: natives producing an opaque orchestration value consumers force (execute's result frame, the
    /// plan handle, the lineage scans, preval).
    Handle {
        EXECUTE("meta::pure::router::execute",
            ROUTER_EXECUTE__FN_1__MAPPING_1__RUNTIME_1__EXTENSION_MANY,
            ROUTER_EXECUTE__FN_1__MAPPING_1__RUNTIME_1__EXECUTION_CONTEXT_1__EXTENSION_MANY,
            ROUTER_EXECUTE__FN_1__MAPPING_1__RUNTIME_1__EXTENSION_MANY__DEBUG_CONTEXT_1),
        EXECUTION_PLAN_EXECUTE("meta::pure::executionPlan::execute",
            EXECUTION_PLAN_EXECUTE__EXECUTION_PLAN_1__ANY_MANY__EXTENSION_MANY),
        EXECUTE_LEGEND_QUERY("meta::legend::executeLegendQuery",
            EXECUTE_LEGEND_QUERY__FN_1__PAIR_MANY__EXTENSION_MANY,
            EXECUTE_LEGEND_QUERY__FN_1__PAIR_MANY__EXECUTION_CONTEXT_1__EXTENSION_MANY),
        TO_SQL("meta::relational::functions::sqlstring::toSQL", TO_SQL__FN_1__MAPPING_1__RUNTIME_1__EXTENSION_MANY),
        EXECUTION_PLAN("meta::pure::executionPlan::executionPlan",
            EXECUTION_PLAN__FUNCTION_DEFINITION_1__MAPPING_1__RUNTIME_1__EXTENSION_MANY,
            EXECUTION_PLAN__FUNCTION_DEFINITION_1__MAPPING_1__RUNTIME_1__BOOLEAN_1__EXTENSION_MANY,
            EXECUTION_PLAN__FUNCTION_DEFINITION_1__MAPPING_1__RUNTIME_1__EXECUTION_CONTEXT_1__EXTENSION_MANY,
            EXECUTION_PLAN__FUNCTION_DEFINITION_1__MAPPING_1__RUNTIME_1__EXTENSION_MANY__DEBUG_CONTEXT_1,
            EXECUTION_PLAN__FUNCTION_DEFINITION_1__EXTENSION_MANY,
            EXECUTION_PLAN__FUNCTION_DEFINITION_1__EXECUTION_CONTEXT_1__EXTENSION_MANY),
        SCAN_RELATIONS("meta::pure::lineage::scanRelations::scanRelations",
            SCAN_RELATIONS__FUNCTION_DEFINITION_1__MAPPING_1__EXTENSION_MANY,
            SCAN_RELATIONS__FUNCTION_DEFINITION_1__MAPPING_1__RUNTIME_1__EXTENSION_MANY),
        SCAN_PROPERTIES("meta::pure::lineage::scanProperties::scanProperties", SCAN_PROPERTIES__4),
        BUILD_PROPERTY_TREE("meta::pure::lineage::scanProperties::propertyTree::buildPropertyTree",
            BUILD_PROPERTY_TREE__LISTS),
        SCAN_COLUMNS("meta::pure::lineage::scanColumns::scanColumns", SCAN_COLUMNS__2),
        PREVAL("meta::pure::router::preeval::preval",
            PREVAL__FUNCTION_DEFINITION_1__EXTENSION_MANY, PREVAL__FUNCTION_DEFINITION_1__EXTENSION_MANY__DEBUG_CONTEXT_1),
    }
}
by_id!(Handle);

impl Handle {
    /// `isExecute(id)`: the two execute spellings (router / executionPlan): the result frame.
    pub fn is_execute(id: Option<&FunctionId>) -> bool {
        matches!(Handle::of(id), Some(Handle::EXECUTE | Handle::EXECUTION_PLAN_EXECUTE))
    }

    /// `runsAFrame(id)`: a call whose result is a frame bound by a let -- the two execute spellings and
    /// `executeLegendQuery`.
    pub fn runs_a_frame(id: Option<&FunctionId>) -> bool {
        matches!(
            Handle::of(id),
            Some(Handle::EXECUTE | Handle::EXECUTION_PLAN_EXECUTE | Handle::EXECUTE_LEGEND_QUERY)
        )
    }

    /// `orchestrationArgument(id, argCount)`: the orchestration position of an execute call -- its runtime argument
    /// (the third), which the statement executor reads in its source form, so the inliner and the substitution leave
    /// it as spelled; `-1` when `id` is not execute or the call is shorter than three arguments.
    pub fn orchestration_argument(id: Option<&FunctionId>, arg_count: i32) -> i32 {
        if Handle::is_execute(id) && arg_count >= 3 {
            2
        } else {
            -1
        }
    }

    /// `forcesAtValuePosition(id)`: which handle forces eagerly when consumed at a statement's value position:
    /// execute's frame run is the value; plan handles stay symbolic.
    pub fn forces_at_value_position(id: Option<&FunctionId>) -> bool {
        matches!(Handle::of(id), Some(Handle::EXECUTE | Handle::EXECUTE_LEGEND_QUERY))
    }
}

family! {
    /// `NativeFn.Effect`: effectful routines at the execution boundary, run via their registered arm when evaluation
    /// reaches the call (never staged).
    Effect {
        /// The ConnectionStore version (relationalExtension.pure's adapter) joins: the arm reads the SQL only.
        EXECUTE_IN_DB("meta::relational::metamodel::execute::executeInDb",
            EXECUTE_IN_DB__STRING_1__CONN_1__INTEGER_1__INTEGER_1, EXECUTE_IN_DB__STRING_1__CONN_1,
            EXECUTE_IN_DB__STRING_1__CONNECTION_STORE_1),
        DROP_AND_CREATE_TABLE_IN_DB("meta::relational::functions::toDDL::dropAndCreateTableInDb",
            DROP_AND_CREATE_TABLE_IN_DB__ANY_1__STRING_1__CONN_1, DROP_AND_CREATE_TABLE_IN_DB__ANY_1__STRING_1__STRING_1__CONN_1),
        DROP_AND_CREATE_SCHEMA_IN_DB("meta::relational::functions::toDDL::dropAndCreateSchemaInDb",
            DROP_AND_CREATE_SCHEMA_IN_DB__STRING_1__CONN_1, DROP_AND_CREATE_SCHEMA_IN_DB__STRING_1__CONN_1__BOOLEAN_1),
        LOAD_CSV_TO_DB_TABLE("meta::relational::metamodel::execute::loadCsvToDbTable",
            LOAD_CSV_TO_DB_TABLE__STRING_1__TABLE_1__CONN_1),
        CREATE_TEMP_TABLE("meta::relational::metamodel::execute::createTempTable",
            CREATE_TEMP_TABLE__STRING_1__COLUMN_MANY__FUNCTION_1__CONN_1,
            CREATE_TEMP_TABLE__STRING_1__COLUMN_MANY__FUNCTION_1__BOOLEAN_1__CONN_1),
        DROP_TEMP_TABLE("meta::relational::metamodel::execute::dropTempTable", DROP_TEMP_TABLE__STRING_1__CONN_1),
        SET_UP_DATA_SQLS("meta::alloy::service::execution::setUpDataSQLs",
            SET_UP_DATA_SQLS__LIST_MANY__ANY_MANY__ANY_1, SET_UP_DATA_SQLS__STRING_1__DATABASE_MANY,
            SET_UP_DATA_SQLS__STRING_1__ANY_MANY__ANY_1),
        SET_UP_DATA_SQLS_V2("meta::alloy::service::execution::setUpDataSQLsV2", SET_UP_DATA_SQLS_V2__STRING_1__ANY_1__ANY_1),
        PRINT("meta::pure::functions::io::print", PRINT__ANY_M__INTEGER_1, PRINT__ANY_M),
        PRINTLN("meta::pure::functions::io::println", PRINTLN__ANY_M__INTEGER_1, PRINTLN__ANY_M),
        CONNECTION_BY_ELEMENT("meta::core::runtime::connectionByElement", CONNECTION_BY_ELEMENT__RUNTIME_1__STORE_1),
    }
}
by_id!(Effect);

impl Effect {
    /// `isDbEffect(id)`: the database effects (raw SQL, DDL, CSV load): statement-ordered, session-bound.
    pub fn is_db_effect(id: Option<&FunctionId>) -> bool {
        matches!(
            Effect::of(id),
            Some(
                Effect::EXECUTE_IN_DB
                    | Effect::DROP_AND_CREATE_TABLE_IN_DB
                    | Effect::DROP_AND_CREATE_SCHEMA_IN_DB
                    | Effect::LOAD_CSV_TO_DB_TABLE
                    | Effect::CREATE_TEMP_TABLE
                    | Effect::DROP_TEMP_TABLE
            )
        )
    }

    /// `isSeedSqlForm(id)`: the seed-SQL forms (setUpDataSQLs / V2).
    pub fn is_seed_sql_form(id: Option<&FunctionId>) -> bool {
        matches!(
            Effect::of(id),
            Some(Effect::SET_UP_DATA_SQLS | Effect::SET_UP_DATA_SQLS_V2)
        )
    }

    /// `isInertDiagnostic(id)`: print / println -- inert diagnostics (no value, no rows).
    pub fn is_inert_diagnostic(id: Option<&FunctionId>) -> bool {
        matches!(Effect::of(id), Some(Effect::PRINT | Effect::PRINTLN))
    }

    /// `readsArgumentsAtPlanning(id)`: the effects with planning-time arguments -- the connection chosen for a store
    /// out of a runtime value (`connectionByElement`), and the store and the dialect a test's CSV is spelled against
    /// (`setUpDataSQLs`, `setUpDataSQLsV2`).
    pub fn reads_arguments_at_planning(id: Option<&FunctionId>) -> bool {
        matches!(
            Effect::of(id),
            Some(Effect::CONNECTION_BY_ELEMENT | Effect::SET_UP_DATA_SQLS | Effect::SET_UP_DATA_SQLS_V2)
        )
    }

    /// `argumentReadAtPlanning(id, k)`: whether argument `k` of a planning-time effect is read at planning -- every
    /// argument of `connectionByElement`; the store and the config of the two seed forms (`k >= 1`), not their data.
    pub fn argument_read_at_planning(id: Option<&FunctionId>, k: i32) -> bool {
        let e = Effect::of(id);
        e == Some(Effect::CONNECTION_BY_ELEMENT)
            || (matches!(e, Some(Effect::SET_UP_DATA_SQLS | Effect::SET_UP_DATA_SQLS_V2)) && k >= 1)
    }
}

/// `NativeFn.OperatorRun`: the n-ary arithmetic carriers -- `a + b (+ ...)`, `a - b` and `a * b` are each one call to
/// a `[*]` overload over a run of operands, as legend-engine's and legend-pure's parsers both build them. A call is one
/// when its resolved declaration is one of these, never by the name it was written with. Not a family (families group
/// natives by their implementer, and these overloads have theirs): a classification the typer reads, so no [`Member`].
#[allow(non_camel_case_types)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum OperatorRun {
    PLUS,
    STRING_PLUS,
    MINUS,
    TIMES,
}

impl OperatorRun {
    /// Java's `values()`, in declaration order.
    pub const VALUES: &'static [OperatorRun] = &[
        OperatorRun::PLUS,
        OperatorRun::STRING_PLUS,
        OperatorRun::MINUS,
        OperatorRun::TIMES,
    ];

    /// Java's `name()`.
    pub fn name(self) -> &'static str {
        match self {
            OperatorRun::PLUS => "PLUS",
            OperatorRun::STRING_PLUS => "STRING_PLUS",
            OperatorRun::MINUS => "MINUS",
            OperatorRun::TIMES => "TIMES",
        }
    }

    pub fn fqn(self) -> &'static str {
        match self {
            OperatorRun::PLUS => "meta::pure::functions::math::plus",
            OperatorRun::STRING_PLUS => "meta::pure::functions::string::plus",
            OperatorRun::MINUS => "meta::pure::functions::math::minus",
            OperatorRun::TIMES => "meta::pure::functions::math::times",
        }
    }

    pub fn overloads(self) -> &'static [NativeId] {
        match self {
            OperatorRun::PLUS => &[
                natives::PLUS__INTEGER_MANY,
                natives::PLUS__FLOAT_MANY,
                natives::PLUS__DECIMAL_MANY,
                natives::PLUS__NUMBER_MANY,
            ],
            OperatorRun::STRING_PLUS => &[natives::STRING_PLUS__STRING_MANY],
            OperatorRun::MINUS => &[
                natives::MINUS__INTEGER_MANY,
                natives::MINUS__FLOAT_MANY,
                natives::MINUS__DECIMAL_MANY,
                natives::MINUS__NUMBER_MANY,
            ],
            OperatorRun::TIMES => &[
                natives::TIMES__INTEGER_MANY,
                natives::TIMES__FLOAT_MANY,
                natives::TIMES__DECIMAL_MANY,
                natives::TIMES__NUMBER_MANY,
            ],
        }
    }

    /// Java's `BY_ID` (`byId()`), built on first use.
    fn by_id() -> &'static HashMap<&'static FunctionId, OperatorRun> {
        static BY_ID: LazyLock<HashMap<&'static FunctionId, OperatorRun>> =
            LazyLock::new(|| operator_runs_by_id(OperatorRun::VALUES, OperatorRun::overloads));
        &BY_ID
    }

    /// `of(id)`: the run whose overloads include the declaration `id`, or none.
    pub fn of(id: Option<&FunctionId>) -> Option<OperatorRun> {
        id.and_then(|id| OperatorRun::by_id().get(id).copied())
    }
}

/// `OperatorRun.byId()`: [`index_by_id`]'s rule -- an identity in two different runs panics, a run naming one overload
/// twice is the same mapping (2026-11-12, the owner's ruling on slice 1b's report: Java refused a repeat within one run
/// too, under a message that was then false; both sides changed).
fn operator_runs_by_id<R: Copy + PartialEq>(
    runs: &[R],
    overloads: fn(R) -> &'static [NativeId],
) -> HashMap<&'static FunctionId, R> {
    let mut out = HashMap::new();
    for &r in runs {
        for o in overloads(r) {
            if out.insert(o.id(), r).is_some_and(|prior| prior != r) {
                panic!("{} in two operator runs", o.def().qualified_name);
            }
        }
    }
    out
}

family! {
    /// `NativeFn.Carrier`: natives bound once at type-check to a carrier node that knows its implementation (the
    /// raw-grid relation, the test-data generators, the JDBC metadata grids).
    Carrier {
        EXECUTE_IN_DB_TO_TDS("meta::relational::metamodel::execute::executeInDbToTDS", EXECUTE_IN_DB_TO_TDS__STRING_1__FN_1),
        GENERATE_TEST_DATA("meta::relational::testDataGeneration::generateTestData",
            GENERATE_TEST_DATA__FUNCTION_DEFINITION_1__MAPPING_1__RUNTIME_1__TABLE_ROW_IDENTIFIERS_MANY__EXTENSION_MANY),
        GET_RELATIONAL_CSV_DATA("meta::relational::testDataGeneration::getRelationalCSVDataFromQuery",
            GET_RELATIONAL_CSV_DATA__FN_1__MAPPING_1),
        FETCH_DB_SCHEMAS_META_DATA("meta::relational::metamodel::execute::fetchDbSchemasMetaData",
            FETCH_DB_SCHEMAS_META_DATA),
        FETCH_DB_TABLES_META_DATA("meta::relational::metamodel::execute::fetchDbTablesMetaData",
            FETCH_DB_TABLES_META_DATA),
        FETCH_DB_COLUMNS_META_DATA("meta::relational::metamodel::execute::fetchDbColumnsMetaData",
            FETCH_DB_COLUMNS_META_DATA),
        FETCH_DB_PRIMARY_KEYS_META_DATA("meta::relational::metamodel::execute::fetchDbPrimaryKeysMetaData",
            FETCH_DB_PRIMARY_KEYS_META_DATA),
    }
}
by_id!(Carrier);

/// `NativeFn.Carrier.FetchDbGrid`: the JDBC DatabaseMetaData grid a fetchDb* native reads -- host-evaluated against the
/// H2 second target (engine-parity metadata casing), never lowered.
#[allow(non_camel_case_types)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FetchDbGrid {
    SCHEMAS,
    TABLES,
    COLUMNS,
    PRIMARY_KEYS,
}

impl FetchDbGrid {
    /// Java's `values()`, in declaration order.
    pub const VALUES: [FetchDbGrid; 4] = [
        FetchDbGrid::SCHEMAS,
        FetchDbGrid::TABLES,
        FetchDbGrid::COLUMNS,
        FetchDbGrid::PRIMARY_KEYS,
    ];

    /// Java's `name()`.
    pub fn name(self) -> &'static str {
        match self {
            FetchDbGrid::SCHEMAS => "SCHEMAS",
            FetchDbGrid::TABLES => "TABLES",
            FetchDbGrid::COLUMNS => "COLUMNS",
            FetchDbGrid::PRIMARY_KEYS => "PRIMARY_KEYS",
        }
    }
}

impl Carrier {
    /// `fetchDbGrid(id)`: the metadata grid of a fetchDb* callee, or none when the callee is not one.
    pub fn fetch_db_grid(id: Option<&FunctionId>) -> Option<FetchDbGrid> {
        match Carrier::of(id)? {
            Carrier::FETCH_DB_SCHEMAS_META_DATA => Some(FetchDbGrid::SCHEMAS),
            Carrier::FETCH_DB_TABLES_META_DATA => Some(FetchDbGrid::TABLES),
            Carrier::FETCH_DB_COLUMNS_META_DATA => Some(FetchDbGrid::COLUMNS),
            Carrier::FETCH_DB_PRIMARY_KEYS_META_DATA => Some(FetchDbGrid::PRIMARY_KEYS),
            Carrier::EXECUTE_IN_DB_TO_TDS | Carrier::GENERATE_TEST_DATA | Carrier::GET_RELATIONAL_CSV_DATA => None,
        }
    }
}

family! {
    /// `NativeFn.ContextOwner`: natives that establish their own evaluation context for their arguments (assertError's
    /// catch): staging never enters them.
    ContextOwner {
        ASSERT_ERROR("meta::pure::functions::asserts::assertError",
            ASSERT_ERROR__MATCHER, ASSERT_ERROR__FN_1__STRING_1, ASSERT_ERROR__FN_1__STRING_1__INTEGER_01__INTEGER_01),
    }
}
by_id!(ContextOwner);

family! {
    /// `NativeFn.DdlStatement`: the DDL statement-string natives the executor renders from the model (create / drop
    /// schema and table).
    DdlStatement {
        CREATE_SCHEMA_STATEMENT("meta::relational::functions::toDDL::createSchemaStatement",
            DDL_CREATE_SCHEMA_STATEMENT__STRING_1),
        CREATE_TABLE_STATEMENT("meta::relational::functions::toDDL::createTableStatement",
            DDL_CREATE_TABLE_STATEMENT__DB_1__STRING_1__STRING_1, DDL_CREATE_TABLE_STATEMENT__DB_1__STRING_1),
        DROP_SCHEMA_STATEMENT("meta::relational::functions::toDDL::dropSchemaStatement",
            DDL_DROP_SCHEMA_STATEMENT__STRING_1),
        DROP_TABLE_STATEMENT("meta::relational::functions::toDDL::dropTableStatement",
            DDL_DROP_TABLE_STATEMENT__DB_1__STRING_1, DDL_DROP_TABLE_STATEMENT__DB_1__STRING_1__STRING_1),
    }
}
by_id!(DdlStatement);

/// `NativeFn.TDS_ROW_OWNER`: the spelling of TDSRow (PlatformTypes.TDS_ROW reads it).
pub const TDS_ROW_OWNER: &str = "meta::pure::tds::TDSRow";
/// `NativeFn.EXECUTE_ROW_OWNER`: the ResultSet Row (execute.pure).
pub const EXECUTE_ROW_OWNER: &str = "meta::relational::metamodel::execute::Row";

/// `NativeFn.RowGetter`: the row accessors -- upstream's qualified properties of `TDSRow` (tds.pure:
/// `getString(colName){$this.get($colName)->cast(@String)}`) and of the ResultSet `Row` twin (`value`), lifted as
/// `<owner>$prop$<name>(this, colName)` and implemented by the platform: a literal column name folds to the row's
/// column read at type time; a non-literal name stays a call to the lifted property that RowGetters lowers by name. No
/// catalog overload: the prelude's declaration is the signature.
#[allow(non_camel_case_types)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum RowGetter {
    GET,
    IS_NULL,
    IS_NOT_NULL,
    GET_STRING,
    GET_NULLABLE_STRING,
    GET_NUMBER,
    GET_INTEGER,
    GET_FLOAT,
    GET_DECIMAL,
    GET_DATE,
    GET_DATE_TIME,
    GET_STRICT_DATE,
    GET_BOOLEAN,
    GET_ENUM,
    /// `execute::Row`'s own by-name cell read (functions.pure: `value(name) = at($this.values,
    /// indexOf($this.parent.columnNames, $name))`) -- the ResultSet twin of the TDSRow getters.
    VALUE,
}

/// One row accessor: its Java name, owner, property, whether it is a typed cell read, and its lifted FQN
/// (`DerivedPropertyNames.lifted(owner, property)`, spelled out: a unit test holds it to the model's spelling).
type RowGetterRow = (RowGetter, &'static str, &'static str, &'static str, bool, &'static str);

impl RowGetter {
    /// Java's `values()` with each member's answers, in declaration order.
    const ROWS: [RowGetterRow; 15] = [
        (
            RowGetter::GET,
            "GET",
            TDS_ROW_OWNER,
            "get",
            false,
            "meta::pure::tds::TDSRow$prop$get",
        ),
        (
            RowGetter::IS_NULL,
            "IS_NULL",
            TDS_ROW_OWNER,
            "isNull",
            false,
            "meta::pure::tds::TDSRow$prop$isNull",
        ),
        (
            RowGetter::IS_NOT_NULL,
            "IS_NOT_NULL",
            TDS_ROW_OWNER,
            "isNotNull",
            false,
            "meta::pure::tds::TDSRow$prop$isNotNull",
        ),
        (
            RowGetter::GET_STRING,
            "GET_STRING",
            TDS_ROW_OWNER,
            "getString",
            true,
            "meta::pure::tds::TDSRow$prop$getString",
        ),
        (
            RowGetter::GET_NULLABLE_STRING,
            "GET_NULLABLE_STRING",
            TDS_ROW_OWNER,
            "getNullableString",
            true,
            "meta::pure::tds::TDSRow$prop$getNullableString",
        ),
        (
            RowGetter::GET_NUMBER,
            "GET_NUMBER",
            TDS_ROW_OWNER,
            "getNumber",
            true,
            "meta::pure::tds::TDSRow$prop$getNumber",
        ),
        (
            RowGetter::GET_INTEGER,
            "GET_INTEGER",
            TDS_ROW_OWNER,
            "getInteger",
            true,
            "meta::pure::tds::TDSRow$prop$getInteger",
        ),
        (
            RowGetter::GET_FLOAT,
            "GET_FLOAT",
            TDS_ROW_OWNER,
            "getFloat",
            true,
            "meta::pure::tds::TDSRow$prop$getFloat",
        ),
        (
            RowGetter::GET_DECIMAL,
            "GET_DECIMAL",
            TDS_ROW_OWNER,
            "getDecimal",
            true,
            "meta::pure::tds::TDSRow$prop$getDecimal",
        ),
        (
            RowGetter::GET_DATE,
            "GET_DATE",
            TDS_ROW_OWNER,
            "getDate",
            true,
            "meta::pure::tds::TDSRow$prop$getDate",
        ),
        (
            RowGetter::GET_DATE_TIME,
            "GET_DATE_TIME",
            TDS_ROW_OWNER,
            "getDateTime",
            true,
            "meta::pure::tds::TDSRow$prop$getDateTime",
        ),
        (
            RowGetter::GET_STRICT_DATE,
            "GET_STRICT_DATE",
            TDS_ROW_OWNER,
            "getStrictDate",
            true,
            "meta::pure::tds::TDSRow$prop$getStrictDate",
        ),
        (
            RowGetter::GET_BOOLEAN,
            "GET_BOOLEAN",
            TDS_ROW_OWNER,
            "getBoolean",
            true,
            "meta::pure::tds::TDSRow$prop$getBoolean",
        ),
        (
            RowGetter::GET_ENUM,
            "GET_ENUM",
            TDS_ROW_OWNER,
            "getEnum",
            true,
            "meta::pure::tds::TDSRow$prop$getEnum",
        ),
        (
            RowGetter::VALUE,
            "VALUE",
            EXECUTE_ROW_OWNER,
            "value",
            true,
            "meta::relational::metamodel::execute::Row$prop$value",
        ),
    ];

    /// Java's `values()`, in declaration order.
    pub const VALUES: &'static [RowGetter] = &[
        RowGetter::GET,
        RowGetter::IS_NULL,
        RowGetter::IS_NOT_NULL,
        RowGetter::GET_STRING,
        RowGetter::GET_NULLABLE_STRING,
        RowGetter::GET_NUMBER,
        RowGetter::GET_INTEGER,
        RowGetter::GET_FLOAT,
        RowGetter::GET_DECIMAL,
        RowGetter::GET_DATE,
        RowGetter::GET_DATE_TIME,
        RowGetter::GET_STRICT_DATE,
        RowGetter::GET_BOOLEAN,
        RowGetter::GET_ENUM,
        RowGetter::VALUE,
    ];

    fn row(self) -> &'static RowGetterRow {
        &RowGetter::ROWS[self as usize]
    }

    /// `member()`: the class member (the row class's qualified property) this accessor implements.
    pub fn member(self) -> ClassMember {
        ClassMember {
            owner: JStr::from(self.owner()),
            name: JStr::from(self.property()),
        }
    }

    /// `owner()`: the declaring class.
    pub fn owner(self) -> &'static str {
        self.row().2
    }

    /// `property()`: the property name.
    pub fn property(self) -> &'static str {
        self.row().3
    }

    /// `typedCell()`: a typed cell read (getString, value, ...) -- folds to the column when the name is literal; get /
    /// isNull / isNotNull have their own forms.
    pub fn typed_cell(self) -> bool {
        self.row().4
    }

    /// Java's `BY_PROPERTY`: each property's first accessor (`putIfAbsent`).
    fn by_property() -> &'static HashMap<&'static str, RowGetter> {
        static BY_PROPERTY: LazyLock<HashMap<&'static str, RowGetter>> = LazyLock::new(|| {
            let mut m = HashMap::new();
            for &g in RowGetter::VALUES {
                m.entry(g.property()).or_insert(g);
            }
            m
        });
        &BY_PROPERTY
    }

    /// `of(appliedName)`: the accessor an applied (bare) name spells -- the name after its last `::` -- or none.
    pub fn of(applied_name: Option<&str>) -> Option<RowGetter> {
        let applied = applied_name?;
        let key = applied.rfind("::").map_or(applied, |cut| &applied[cut + 2..]);
        RowGetter::by_property().get(key).copied()
    }

    /// `isOwner(classFqn)`: whether `class_fqn` is an erased row class -- an owner of this family (TDSRow,
    /// execute::Row): a row whose columns only its parent result knows; in a type position it is the late-bound row
    /// struct.
    pub fn is_owner(class_fqn: &str) -> bool {
        RowGetter::VALUES.iter().any(|g| g.owner() == class_fqn)
    }

    /// `ofLifted(liftedFqn)`: the accessor a lifted derived-property callee spells, or none.
    pub fn of_lifted(lifted_fqn: Option<&str>) -> Option<RowGetter> {
        let (owner, property) = lite_model::names::split_derived_property(&JStr::from(lifted_fqn?))?;
        RowGetter::VALUES
            .iter()
            .copied()
            .find(|g| owner == g.owner() && property == g.property())
    }
}

impl Member for RowGetter {
    fn name(self) -> &'static str {
        self.row().1
    }

    /// `DerivedPropertyNames.lifted(owner, property)`.
    fn fqn(self) -> &'static str {
        self.row().5
    }

    fn overloads(self) -> &'static [NativeId] {
        &[]
    }
}

/// The families, in `NativeFn.families()`'s order, each once (2026-11-12, the owner's ruling: Java put `RowGetter`
/// twice, the second put a no-op its `LinkedHashMap` kept at the first place; the second is gone).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Family {
    Calendar,
    Verdict,
    RowGetter,
    Frame,
    LowererForm,
    LiteralForm,
    ContextOption,
    PlanWrapper,
    RelationQuantifier,
    GroupAggregate,
    ObjectReference,
    SubtypeForm,
    ResolverForm,
    LiteDesugar,
    TyperForm,
    JavaRoutine,
    Handle,
    Effect,
    Carrier,
    ContextOwner,
    DdlStatement,
}

impl Family {
    /// Every family, in `families()`'s order.
    pub const VALUES: [Family; 21] = [
        Family::Calendar,
        Family::Verdict,
        Family::RowGetter,
        Family::Frame,
        Family::LowererForm,
        Family::LiteralForm,
        Family::ContextOption,
        Family::PlanWrapper,
        Family::RelationQuantifier,
        Family::GroupAggregate,
        Family::ObjectReference,
        Family::SubtypeForm,
        Family::ResolverForm,
        Family::LiteDesugar,
        Family::TyperForm,
        Family::JavaRoutine,
        Family::Handle,
        Family::Effect,
        Family::Carrier,
        Family::ContextOwner,
        Family::DdlStatement,
    ];

    /// The family's name, as `families()` keys it (the Java enum's simple name).
    pub fn name(self) -> &'static str {
        match self {
            Family::Calendar => "Calendar",
            Family::Verdict => "Verdict",
            Family::RowGetter => "RowGetter",
            Family::Frame => "Frame",
            Family::LowererForm => "LowererForm",
            Family::LiteralForm => "LiteralForm",
            Family::ContextOption => "ContextOption",
            Family::PlanWrapper => "PlanWrapper",
            Family::RelationQuantifier => "RelationQuantifier",
            Family::GroupAggregate => "GroupAggregate",
            Family::ObjectReference => "ObjectReference",
            Family::SubtypeForm => "SubtypeForm",
            Family::ResolverForm => "ResolverForm",
            Family::LiteDesugar => "LiteDesugar",
            Family::TyperForm => "TyperForm",
            Family::JavaRoutine => "JavaRoutine",
            Family::Handle => "Handle",
            Family::Effect => "Effect",
            Family::Carrier => "Carrier",
            Family::ContextOwner => "ContextOwner",
            Family::DdlStatement => "DdlStatement",
        }
    }

    /// Each member of the family, in its enum's order: its Java name, its FQN, its catalog overloads.
    pub fn members(self) -> Vec<(&'static str, &'static str, &'static [NativeId])> {
        fn rows<M: Member>(values: &'static [M]) -> Vec<(&'static str, &'static str, &'static [NativeId])> {
            values.iter().map(|&m| (m.name(), m.fqn(), m.overloads())).collect()
        }
        match self {
            Family::Calendar => rows(Calendar::VALUES),
            Family::Verdict => rows(Verdict::VALUES),
            Family::RowGetter => rows(RowGetter::VALUES),
            Family::Frame => rows(Frame::VALUES),
            Family::LowererForm => rows(LowererForm::VALUES),
            Family::LiteralForm => rows(LiteralForm::VALUES),
            Family::ContextOption => rows(ContextOption::VALUES),
            Family::PlanWrapper => rows(PlanWrapper::VALUES),
            Family::RelationQuantifier => rows(RelationQuantifier::VALUES),
            Family::GroupAggregate => rows(GroupAggregate::VALUES),
            Family::ObjectReference => rows(ObjectReference::VALUES),
            Family::SubtypeForm => rows(SubtypeForm::VALUES),
            Family::ResolverForm => rows(ResolverForm::VALUES),
            Family::LiteDesugar => rows(LiteDesugar::VALUES),
            Family::TyperForm => rows(TyperForm::VALUES),
            Family::JavaRoutine => rows(JavaRoutine::VALUES),
            Family::Handle => rows(Handle::VALUES),
            Family::Effect => rows(Effect::VALUES),
            Family::Carrier => rows(Carrier::VALUES),
            Family::ContextOwner => rows(ContextOwner::VALUES),
            Family::DdlStatement => rows(DdlStatement::VALUES),
        }
    }
}

/// `NativeFn.claims(fqn)`: whether the FQN is a member of any family -- a native this platform claims (the claim
/// registry's own question, asked at compile time: a typed native in no family has no body here and is loud at
/// evaluation). A set of every member's FQN, asked for membership only.
pub fn claims(fqn: &str) -> bool {
    static CLAIMED: LazyLock<HashSet<&'static str>> = LazyLock::new(|| {
        Family::VALUES
            .iter()
            .flat_map(|f| f.members())
            .map(|(_, fqn, _)| fqn)
            .collect()
    });
    CLAIMED.contains(fqn)
}

#[cfg(test)]
mod tests {
    use super::*;
    use lite_front::doors::on_parse_stack;
    use lite_model::names::lifted_derived_property;

    /// Two members sharing one overload, as a family must never be (their Java names `FIRST` and `SECOND`).
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum Clash {
        First,
        Second,
    }

    impl Member for Clash {
        fn name(self) -> &'static str {
            match self {
                Clash::First => "FIRST",
                Clash::Second => "SECOND",
            }
        }

        fn fqn(self) -> &'static str {
            "meta::pure::functions::date::calendar::cw"
        }

        fn overloads(self) -> &'static [NativeId] {
            match self {
                Clash::First => &[natives::CAL_CW, natives::CAL_CW],
                Clash::Second => &[natives::CAL_MTD, natives::CAL_CW],
            }
        }
    }

    fn panic_text(work: impl FnOnce() + Send + std::panic::UnwindSafe) -> String {
        on_parse_stack(|| {
            let e = std::panic::catch_unwind(work).expect_err("panics");
            e.downcast_ref::<String>().cloned().unwrap_or_default()
        })
    }

    #[test]
    fn an_overload_in_two_members_is_javas_initialization_error() {
        // the same member twice is no conflict
        on_parse_stack(|| assert_eq!(index_by_id(&[Clash::First]).len(), 1));
        assert_eq!(
            panic_text(|| {
                index_by_id(&[Clash::First, Clash::Second]);
            }),
            "meta::pure::functions::date::calendar::cw in two members: FIRST, SECOND"
        );
    }

    #[test]
    fn an_overload_in_two_operator_runs_is_javas_initialization_error() {
        // indexById's rule (2026-11-12): one run naming one overload twice is the same mapping, two runs are refused
        on_parse_stack(|| assert_eq!(operator_runs_by_id(&[Clash::First], Clash::overloads).len(), 1));
        assert_eq!(
            panic_text(|| {
                operator_runs_by_id(&[Clash::First, Clash::Second], Clash::overloads);
            }),
            "meta::pure::functions::date::calendar::cw in two operator runs"
        );
        on_parse_stack(|| {
            assert_eq!(
                OperatorRun::of(Some(natives::MINUS__FLOAT_MANY.id())),
                Some(OperatorRun::MINUS)
            );
            assert_eq!(OperatorRun::of(None), None);
        });
    }

    #[test]
    fn the_index_by_fqn_keeps_the_last_member() {
        let m = index(&[Clash::First, Clash::Second]);
        assert_eq!(m.len(), 1);
        assert_eq!(m["meta::pure::functions::date::calendar::cw"], Clash::Second);
        assert_eq!(index(Verdict::VALUES).len(), Verdict::VALUES.len());
    }

    #[test]
    fn the_lifted_spellings_are_the_models() {
        for &g in RowGetter::VALUES {
            assert_eq!(
                g.fqn(),
                lifted_derived_property(&JStr::from(g.owner()), &JStr::from(g.property())).lossy()
            );
            assert_eq!(g.row().0, g);
        }
        for &r in JavaRoutine::VALUES {
            if let (Some(lifted), Some(member)) = (r.implemented_derived(), r.implemented_member()) {
                assert_eq!(lifted, lifted_derived_property(&member.owner, &member.name).lossy());
            } else {
                assert!(r.implemented_derived().is_none() && r.implemented_member().is_none());
            }
        }
    }

    #[test]
    fn a_row_getter_is_found_by_the_name_after_its_last_separator() {
        assert_eq!(RowGetter::of(Some("getString")), Some(RowGetter::GET_STRING));
        assert_eq!(RowGetter::of(Some("a::b::value")), Some(RowGetter::VALUE));
        assert_eq!(RowGetter::of(Some(":::get")), Some(RowGetter::GET));
        assert_eq!(RowGetter::of(Some("get::")), None);
        assert_eq!(RowGetter::of(None), None);
        // a lifted FQN is no applied name: its last segment is the owner's simple name and the sigil
        assert_eq!(RowGetter::of(Some(RowGetter::GET.fqn())), None);
        assert_eq!(RowGetter::of_lifted(Some(RowGetter::GET.fqn())), Some(RowGetter::GET));
        assert_eq!(RowGetter::of_lifted(Some("$prop$get")), None);
        assert!(RowGetter::is_owner(EXECUTE_ROW_OWNER));
        assert!(!RowGetter::is_owner("meta::pure::tds::TabularDataSet"));
    }

    #[test]
    fn the_planning_arguments_follow_javas_precedence() {
        on_parse_stack(|| {
            let seed = Some(natives::SET_UP_DATA_SQLS_V2__STRING_1__ANY_1__ANY_1.id());
            let conn = Some(natives::CONNECTION_BY_ELEMENT__RUNTIME_1__STORE_1.id());
            assert!(!Effect::argument_read_at_planning(seed, 0));
            assert!(Effect::argument_read_at_planning(seed, 1));
            assert!(Effect::argument_read_at_planning(conn, 0));
            assert!(!Effect::argument_read_at_planning(None, 1));
            let exec = Some(natives::ROUTER_EXECUTE__FN_1__MAPPING_1__RUNTIME_1__EXTENSION_MANY.id());
            assert_eq!(Handle::orchestration_argument(exec, 2), -1);
            assert_eq!(Handle::orchestration_argument(exec, 3), 2);
            assert!(LowererForm::is_bivariate_map(natives::ROW_MAPPER__T_0_1__U_0_1.id()));
            assert_eq!(
                Carrier::fetch_db_grid(Some(natives::FETCH_DB_COLUMNS_META_DATA.id())),
                Some(FetchDbGrid::COLUMNS)
            );
        });
    }

    #[test]
    fn every_overload_finds_its_member_and_every_member_is_claimed() {
        on_parse_stack(|| {
            for &m in Calendar::VALUES {
                for o in m.overloads() {
                    assert_eq!(Calendar::of(Some(o.id())), Some(m));
                }
            }
            for f in Family::VALUES {
                for (_, fqn, _) in f.members() {
                    assert!(claims(fqn), "{fqn}");
                }
            }
            assert!(!claims("meta::pure::functions::math::plus"));
            assert!(Verdict::ASSERT.matches("x::assert") && !Verdict::ASSERT.matches("xassert"));
        });
    }
}
