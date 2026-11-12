// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! `CoreFn`: the closed vocabulary of core structural constructs -- the functions the type checker treats as language
//! forms rather than library calls (GHC's sealed `PrimOp`). A parse-time function name is resolved to a `CoreFn` once
//! ([`CoreFn::of`]), and every downstream dispatch is an exhaustive `match`.
//!
//! Ported from core's `com.legend.platform.CoreFn` (2026-11-12, docs/rust-port/PHASE3.md slice 1b), member for member
//! in Java's order, each with its parse-time spellings (Java's private `parseNames`, the first the canonical one) and
//! the exact FQNs whose every overload it owns (Java's `OWNS`, a `Set.of` each: in declaration order here). Java's
//! `BY_NAME` (every spelling to its construct; a `HashMap`, a spelling met twice keeping the last) and `OWNER_OF` (each
//! owned FQN to its form; an FQN owned twice is the platform's bug, Java's `ExceptionInInitializerError`, a panic here
//! with Java's message) are hash maps here too, built on the first lookup and never iterated: the parse-name map is
//! answered in declaration order ([`CoreFn::parse_names`]).

use std::collections::HashMap;
use std::sync::{Arc, LazyLock};

use lite_builtin::pure::INTERNAL_DESUGAR;
use lite_model::linked::LinkedMap;

/// One core construct (Java's constant names).
#[allow(non_camel_case_types)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum CoreFn {
    /// `let name = value`: a binding with scope effects (`letFunction`).
    LET,
    /// `deactivate(e)`: the compile-time reflection carrier.
    DEACTIVATE,
    /// `getRelationalCSVDataFromQuery(query, mapping)`: compile-time reflection, folds to instance literals.
    GET_RELATIONAL_CSV_DATA,
    /// `generateTestData(...)`: runtime data extraction through a protocol-capturing carrier.
    GENERATE_TEST_DATA,
    /// `generateSeedDataString(...)`: runtime seed-text extraction, the same carrier.
    GENERATE_SEED_DATA_STRING,
    /// `planTestDataGeneration(...)`: the test-data generation plan, the same carrier.
    PLAN_TEST_DATA_GENERATION,
    /// `mayExecuteAlloyTest(serverThunk, |fallback)`: the call is its fallback thunk.
    MAY_EXECUTE_ALLOY_TEST,
    /// `mayExecuteLegendTest(serverThunk, |fallback)`: the same branch native for a live Legend server.
    MAY_EXECUTE_LEGEND_TEST,
    /// `if(cond, |then, |else)`: thunk branches and a branch-type join.
    IF,
    /// `validate(query, mapping, runtime)`: a raw-space desugar before typing.
    VALIDATE,
    /// `^Class(prop=value, ...)`: instance construction (syntax: it owns no function).
    NEW,
    /// `#>{db.TABLE}#`: a physical table reference resolved through the store.
    TABLE_REFERENCE,
    /// `tableToTDS(tableReference(...))`: identity over the relation carrier.
    TABLE_TO_TDS,
    /// `project`: a relation schema from projection lambdas or colspecs.
    PROJECT,
    /// Relation `sort(asc(~col), ...)`; collection sort delegates to the generic path.
    SORT,
    /// Relation `rename(~old, ~new)`.
    RENAME,
    /// `filter(pred)`: relation and collection overloads.
    FILTER,
    /// `map(fn)`: collection map, relation row-map.
    MAP,
    /// `asc(~col)` / `ascending(~col)`: a sort key.
    ASC,
    /// `desc(~col)` / `descending(~col)`: a sort key.
    DESC,
    /// `sortInfo->emptyFirst()`: nulls placed first.
    EMPTY_FIRST,
    /// `sortInfo->emptyLast()`: nulls placed last.
    EMPTY_LAST,
    /// Relation `select(~cols)`; `newTDSRelationAccessor` is its legacy alias.
    SELECT,
    /// Relation `without(~cols)` (lite surface).
    WITHOUT,
    /// `$r->read(~col)` (lite surface).
    READ,
    /// Relation `distinct` / `distinct(~[cols])`.
    DISTINCT,
    /// Relation `concatenate`: UNION ALL.
    CONCATENATE,
    /// `limit(n)`: SQL LIMIT.
    LIMIT,
    /// Alias of [`CoreFn::LIMIT`] with its own parse name.
    TAKE,
    /// `drop(n)`: SQL OFFSET.
    DROP,
    /// `slice(start, stop)`.
    SLICE,
    /// Relation `extend(~newCol:x|...)`.
    EXTEND,
    /// `groupBy(~keys, ~agg:map:reduce)`.
    GROUP_BY,
    /// Legacy TDS `groupByWithWindowSubset(...)`: a desugar to the 4-arg legacy groupBy.
    GROUP_BY_WITH_WINDOW_SUBSET,
    /// Relation `aggregate(~agg:map:reduce)`.
    AGGREGATE,
    /// Relation `join(other, JoinKind.INNER, {t,v|cond})`.
    JOIN,
    /// Relation `asOfJoin(other, {t,v|match} [, {t,v|cond}])`.
    AS_OF_JOIN,
    /// `cast(@T)`.
    CAST,
    TYPE_AS_DECLARED,
    /// Mapping-side wire coercion.
    CAST_AS_DECLARED,
    /// `to(@T)`: nullable conversion.
    TO,
    /// `toMany(@T)`: widening conversion.
    TO_MANY,
    /// `match(value, [t:Type|...])`: compile-time static dispatch.
    MATCH,
    /// `eval`: beta-reduction.
    EVAL,
    /// `#TDS ...#` inline grid.
    TDS,
    /// `sourceUrl('...')`: a semi-structured source.
    SOURCE_URL,
    /// Relation `flatten(~col)`: unnest.
    FLATTEN,
    /// Relation `pivot(~cols, ~agg)`.
    PIVOT,
    /// Relation `columns()`: static column metadata.
    COLUMNS,
    /// `toJSON(tds)`: a TDS as the engine's TDS JSON.
    TO_JSON,
    /// `tdsToJSONKeyValueObjectString(tds)`.
    TDS_TO_JSON_KV,
    /// Collection `sortBy(key)`.
    SORT_BY,
    /// Collection `sortByReversed(key)`.
    SORT_BY_REVERSED,
    /// `Class.all()`: the object-graph source anchor.
    GET_ALL,
    /// `Class.allVersions()`: the milestoned version-sweep extent.
    GET_ALL_FOR_EACH_DATE,
    GET_ALL_VERSIONS,
    /// `Class.allVersionsInRange(start, end)`.
    GET_ALL_VERSIONS_IN_RANGE,
    /// `from(mapping?, runtime)`: execution-context binding.
    FROM,
    /// Relation `write([target])`.
    WRITE,
    /// `#>{db.t}#->delete(r | pred)`.
    DELETE,
    /// `#>{db.t}#->update(r | pred, ~[c: r | expr])`.
    UPDATE,
    /// `rel->merge(#>{db.t}#, ~[keys])`.
    MERGE,
    /// `fold({e,acc|...}, init)`.
    FOLD,
    /// `navigate`: the graph-traversal primitive.
    NAVIGATE,
    LEGACY_NAVIGATE,
    /// `route(target, rows, cond)`: one route of a several-route legacyNavigate.
    ROUTE,
    /// `inlineView(rel)`: a SQL island's derived table that computes windows (lite-internal).
    INLINE_VIEW,
    /// `recurse(base, r | step)`: the database's recursive CTE (lite-internal).
    RECURSE,
    /// `let s = rows->stage()`: rows computed once into a temporary table (lite-internal).
    STAGE,
    /// `isDistinct(collection, #{Class{a, b}}#)`.
    IS_DISTINCT,
    /// `graphFetch(#{Class{...}}#)`.
    GRAPH_FETCH,
    /// `graphFetchChecked(#{Class{...}}#)`.
    GRAPH_FETCH_CHECKED,
    /// `serialize(#{Class{...}}#)`.
    SERIALIZE,
    /// `over(~partition [, asc(~key)...])`: a window definition.
    OVER,
}

/// One member's row: the member, its Java name, its spellings (the canonical first), the FQNs it owns.
type Row = (CoreFn, &'static str, &'static [&'static str], &'static [&'static str]);

/// Every member's row, in Java's declaration order (`ROWS[m as usize]` is `m`'s).
const ROWS: [Row; 74] = rows::ROWS;

mod rows {
    use super::CoreFn::*;
    use super::Row;

    pub(super) const ROWS: [Row; 74] = [
        (
            LET,
            "LET",
            &["letFunction"],
            &["meta::pure::functions::lang::letFunction"],
        ),
        (
            DEACTIVATE,
            "DEACTIVATE",
            &["deactivate"],
            &["meta::pure::functions::meta::deactivate"],
        ),
        (
            GET_RELATIONAL_CSV_DATA,
            "GET_RELATIONAL_CSV_DATA",
            &["getRelationalCSVDataFromQuery"],
            &["meta::relational::testDataGeneration::getRelationalCSVDataFromQuery"],
        ),
        (
            GENERATE_TEST_DATA,
            "GENERATE_TEST_DATA",
            &["generateTestData"],
            &["meta::relational::testDataGeneration::generateTestData"],
        ),
        (
            GENERATE_SEED_DATA_STRING,
            "GENERATE_SEED_DATA_STRING",
            &["generateSeedDataString"],
            &["meta::relational::testDataGeneration::generateSeedDataString"],
        ),
        (
            PLAN_TEST_DATA_GENERATION,
            "PLAN_TEST_DATA_GENERATION",
            &["planTestDataGeneration"],
            &["meta::relational::testDataGeneration::executionPlan::planTestDataGeneration"],
        ),
        (
            MAY_EXECUTE_ALLOY_TEST,
            "MAY_EXECUTE_ALLOY_TEST",
            &["mayExecuteAlloyTest"],
            &["meta::alloy::test::mayExecuteAlloyTest"],
        ),
        (
            MAY_EXECUTE_LEGEND_TEST,
            "MAY_EXECUTE_LEGEND_TEST",
            &["mayExecuteLegendTest"],
            &["meta::legend::test::mayExecuteLegendTest"],
        ),
        (IF, "IF", &["if"], &["meta::pure::functions::lang::if"]),
        (
            VALIDATE,
            "VALIDATE",
            &["validate"],
            &["meta::relational::validation::validate"],
        ),
        (NEW, "NEW", &["new"], &[]),
        (
            TABLE_REFERENCE,
            "TABLE_REFERENCE",
            &["tableReference"],
            &["meta::relational::functions::database::tableReference"],
        ),
        (
            TABLE_TO_TDS,
            "TABLE_TO_TDS",
            &["tableToTDS"],
            &["meta::pure::tds::tableToTDS"],
        ),
        (
            PROJECT,
            "PROJECT",
            &["project"],
            &["meta::pure::functions::relation::project", "meta::pure::tds::project"],
        ),
        (
            SORT,
            "SORT",
            &["sort"],
            &[
                "meta::pure::functions::collection::sort",
                "meta::pure::functions::relation::sort",
                "meta::pure::tds::sort",
            ],
        ),
        (
            RENAME,
            "RENAME",
            &["rename"],
            &["meta::pure::functions::relation::rename"],
        ),
        (
            FILTER,
            "FILTER",
            &["filter"],
            &[
                "meta::pure::functions::collection::filter",
                "meta::pure::functions::relation::filter",
                "meta::pure::tds::filter",
            ],
        ),
        (
            MAP,
            "MAP",
            &["map"],
            &[
                "meta::pure::functions::collection::map",
                "meta::pure::functions::relation::map",
            ],
        ),
        (
            ASC,
            "ASC",
            &["ascending", "asc"],
            &["meta::pure::functions::relation::ascending", "meta::pure::tds::asc"],
        ),
        (
            DESC,
            "DESC",
            &["descending", "desc"],
            &["meta::pure::functions::relation::descending", "meta::pure::tds::desc"],
        ),
        (
            EMPTY_FIRST,
            "EMPTY_FIRST",
            &["emptyFirst"],
            &["meta::pure::functions::relation::emptyFirst"],
        ),
        (
            EMPTY_LAST,
            "EMPTY_LAST",
            &["emptyLast"],
            &["meta::pure::functions::relation::emptyLast"],
        ),
        (
            SELECT,
            "SELECT",
            &["select", "newTDSRelationAccessor"],
            &[
                "meta::pure::functions::relation::select",
                "meta::pure::metamodel::relation::newTDSRelationAccessor",
            ],
        ),
        (WITHOUT, "WITHOUT", &["without"], &["meta::legend::lite::without"]),
        (READ, "READ", &["read"], &["meta::legend::lite::read"]),
        (
            DISTINCT,
            "DISTINCT",
            &["distinct"],
            &[
                "meta::pure::functions::collection::distinct",
                "meta::pure::functions::relation::distinct",
                "meta::pure::tds::distinct",
            ],
        ),
        (
            CONCATENATE,
            "CONCATENATE",
            &["concatenate"],
            &[
                "meta::pure::functions::collection::concatenate",
                "meta::pure::functions::relation::concatenate",
            ],
        ),
        (
            LIMIT,
            "LIMIT",
            &["limit"],
            &[
                "meta::pure::functions::collection::limit",
                "meta::pure::functions::relation::limit",
                "meta::pure::tds::limit",
            ],
        ),
        (TAKE, "TAKE", &["take"], &["meta::pure::functions::collection::take"]),
        (
            DROP,
            "DROP",
            &["drop"],
            &[
                "meta::pure::functions::collection::drop",
                "meta::pure::functions::relation::drop",
            ],
        ),
        (
            SLICE,
            "SLICE",
            &["slice"],
            &[
                "meta::pure::functions::collection::slice",
                "meta::pure::functions::relation::slice",
            ],
        ),
        (
            EXTEND,
            "EXTEND",
            &["extend"],
            &["meta::pure::functions::relation::extend", "meta::pure::tds::extend"],
        ),
        (
            GROUP_BY,
            "GROUP_BY",
            &["groupBy", "groupByOverInstances", "groupByComputedKeys"],
            &[
                "meta::legend::lite::groupByComputedKeys",
                "meta::legend::lite::groupByOverInstances",
                "meta::pure::functions::collection::groupBy",
                "meta::pure::functions::relation::groupBy",
                "meta::pure::tds::groupBy",
            ],
        ),
        (
            GROUP_BY_WITH_WINDOW_SUBSET,
            "GROUP_BY_WITH_WINDOW_SUBSET",
            &["groupByWithWindowSubset"],
            &["meta::pure::tds::groupByWithWindowSubset"],
        ),
        (
            AGGREGATE,
            "AGGREGATE",
            &["aggregate"],
            &["meta::pure::functions::relation::aggregate"],
        ),
        (
            JOIN,
            "JOIN",
            &["join", "joinWithPrefix", "joinSlot"],
            &[
                "meta::legend::lite::joinSlot",
                "meta::legend::lite::joinWithPrefix",
                "meta::pure::functions::relation::join",
            ],
        ),
        (
            AS_OF_JOIN,
            "AS_OF_JOIN",
            &["asOfJoin", "asOfJoinWithPrefix"],
            &[
                "meta::legend::lite::asOfJoinWithPrefix",
                "meta::pure::functions::relation::asOfJoin",
            ],
        ),
        (CAST, "CAST", &["cast"], &["meta::pure::functions::lang::cast"]),
        (
            TYPE_AS_DECLARED,
            "TYPE_AS_DECLARED",
            &["typeAsDeclared"],
            &["meta::legend::lite::typeAsDeclared"],
        ),
        (
            CAST_AS_DECLARED,
            "CAST_AS_DECLARED",
            &["castAsDeclared"],
            &["meta::legend::lite::castAsDeclared"],
        ),
        (TO, "TO", &["to"], &["meta::pure::functions::variant::convert::to"]),
        (
            TO_MANY,
            "TO_MANY",
            &["toMany"],
            &["meta::pure::functions::variant::convert::toMany"],
        ),
        (MATCH, "MATCH", &["match"], &["meta::pure::functions::lang::match"]),
        (
            EVAL,
            "EVAL",
            &["eval"],
            &[
                "meta::pure::functions::lang::eval",
                "meta::pure::functions::relation::eval",
            ],
        ),
        (TDS, "TDS", &["tds"], &["meta::legend::lite::tds"]),
        (
            SOURCE_URL,
            "SOURCE_URL",
            &["sourceUrl"],
            &["meta::legend::lite::sourceUrl"],
        ),
        (
            FLATTEN,
            "FLATTEN",
            &["flatten"],
            &["meta::pure::functions::relation::variant::flatten"],
        ),
        (PIVOT, "PIVOT", &["pivot"], &["meta::pure::functions::relation::pivot"]),
        (
            COLUMNS,
            "COLUMNS",
            &["columns"],
            &["meta::pure::functions::relation::columns"],
        ),
        (TO_JSON, "TO_JSON", &["toJSON"], &["meta::json::toJSON"]),
        (
            TDS_TO_JSON_KV,
            "TDS_TO_JSON_KV",
            &["tdsToJSONKeyValueObjectString"],
            &["meta::json::tdsToJSONKeyValueObjectString"],
        ),
        (
            SORT_BY,
            "SORT_BY",
            &["sortBy"],
            &["meta::pure::functions::collection::sortBy"],
        ),
        (
            SORT_BY_REVERSED,
            "SORT_BY_REVERSED",
            &["sortByReversed"],
            &["meta::pure::functions::collection::sortByReversed"],
        ),
        (
            GET_ALL,
            "GET_ALL",
            &["getAll"],
            &["meta::pure::functions::collection::getAll"],
        ),
        (
            GET_ALL_FOR_EACH_DATE,
            "GET_ALL_FOR_EACH_DATE",
            &["getAllForEachDate"],
            &["meta::pure::functions::collection::getAllForEachDate"],
        ),
        (
            GET_ALL_VERSIONS,
            "GET_ALL_VERSIONS",
            &["getAllVersions"],
            &["meta::pure::functions::collection::getAllVersions"],
        ),
        (
            GET_ALL_VERSIONS_IN_RANGE,
            "GET_ALL_VERSIONS_IN_RANGE",
            &["getAllVersionsInRange"],
            &["meta::pure::functions::collection::getAllVersionsInRange"],
        ),
        (FROM, "FROM", &["from"], &["meta::pure::mapping::from"]),
        (WRITE, "WRITE", &["write"], &["meta::pure::functions::relation::write"]),
        (DELETE, "DELETE", &["delete"], &["meta::legend::lite::delete"]),
        (UPDATE, "UPDATE", &["update"], &["meta::legend::lite::update"]),
        (MERGE, "MERGE", &["merge"], &["meta::legend::lite::merge"]),
        (FOLD, "FOLD", &["fold"], &["meta::pure::functions::collection::fold"]),
        (NAVIGATE, "NAVIGATE", &["navigate"], &["meta::legend::lite::navigate"]),
        (
            LEGACY_NAVIGATE,
            "LEGACY_NAVIGATE",
            &["legacyNavigate"],
            &["meta::legend::lite::legacyNavigate"],
        ),
        (ROUTE, "ROUTE", &["route"], &["meta::legend::lite::route"]),
        (
            INLINE_VIEW,
            "INLINE_VIEW",
            &["inlineView"],
            &["meta::legend::lite::inlineView"],
        ),
        (RECURSE, "RECURSE", &["recurse"], &["meta::legend::lite::recurse"]),
        (STAGE, "STAGE", &["stage"], &["meta::legend::lite::stage"]),
        (
            IS_DISTINCT,
            "IS_DISTINCT",
            &["isDistinct"],
            &["meta::pure::functions::collection::isDistinct"],
        ),
        (
            GRAPH_FETCH,
            "GRAPH_FETCH",
            &["graphFetch"],
            &["meta::pure::graphFetch::execution::graphFetch"],
        ),
        (
            GRAPH_FETCH_CHECKED,
            "GRAPH_FETCH_CHECKED",
            &["graphFetchChecked"],
            &["meta::pure::graphFetch::execution::graphFetchChecked"],
        ),
        (
            SERIALIZE,
            "SERIALIZE",
            &["serialize"],
            &["meta::pure::graphFetch::execution::serialize"],
        ),
        (OVER, "OVER", &["over"], &["meta::pure::functions::relation::over"]),
    ];
}

/// Java's `BY_NAME` (a hash map, for lookup) and its entries in declaration order (for [`CoreFn::parse_names`]).
struct ByName {
    map: HashMap<&'static str, CoreFn>,
    ordered: Arc<[(&'static str, CoreFn)]>,
}

static BY_NAME: LazyLock<ByName> = LazyLock::new(|| {
    let mut map = HashMap::new();
    let mut ordered = LinkedMap::new();
    for (f, _, spellings, _) in ROWS {
        for &name in spellings {
            // a spelling met twice keeps the last construct, as Java's HashMap.put does
            map.insert(name, f);
            ordered.put(name, f);
        }
    }
    ByName {
        map,
        ordered: ordered.freeze(),
    }
});

/// Java's `OWNER_OF`: each owned FQN to the form that owns it.
static OWNER_OF: LazyLock<HashMap<&'static str, CoreFn>> = LazyLock::new(|| {
    let mut out = HashMap::new();
    for (f, _, _, owns) in ROWS {
        for &fqn in owns {
            if let Some(prior) = out.insert(fqn, f) {
                panic!("{fqn} owned by {} and {}", prior.name(), f.name());
            }
        }
    }
    out
});

impl CoreFn {
    /// Every member, in Java's `values()` order.
    pub const VALUES: [CoreFn; 74] = {
        let mut out = [CoreFn::LET; 74];
        let mut i = 0;
        while i < 74 {
            out[i] = ROWS[i].0;
            i += 1;
        }
        out
    };

    /// Java's `name()`.
    pub fn name(self) -> &'static str {
        ROWS[self as usize].1
    }

    /// Every parse-time spelling of the construct, the canonical first (Java's private `parseNames`).
    pub fn spellings(self) -> &'static [&'static str] {
        ROWS[self as usize].2
    }

    /// `parseName()`: the construct's canonical parse-time name (aliases like `ascending` resolve here too).
    pub fn parse_name(self) -> &'static str {
        ROWS[self as usize].2[0]
    }

    /// `ownedFqns()`: the FQNs whose every overload this form owns (empty for `NEW`), in declaration order (Java's is
    /// a `Set.of`, asked for membership and iterated where its order is not seen).
    pub fn owned_fqns(self) -> &'static [&'static str] {
        ROWS[self as usize].3
    }

    /// `CoreFn.parseNames()`: every parse-time name (aliases included) with its construct, the CoreFn claims -- in
    /// declaration order (Java's is a hash map's view, asked by name).
    pub fn parse_names() -> &'static [(&'static str, CoreFn)] {
        &BY_NAME.ordered
    }

    /// `CoreFn.parseNames().get(name)`: the construct a parse-time spelling names, internal desugar names included.
    pub fn by_parse_name(name: &str) -> Option<CoreFn> {
        BY_NAME.map.get(name).copied()
    }

    /// `owning(fqn)`: the form that owns a full name, if any.
    pub fn owning(fqn: &str) -> Option<CoreFn> {
        OWNER_OF.get(fqn).copied()
    }

    /// `of(parseName)`: the parse-time name of an applied function, to its core construct -- none for a library call.
    /// An internal-only construct (the lite desugar IR) dispatches solely on its exact FQN: a bare `tds` is none, and
    /// does not fall through to the owner map. A qualified name dispatches by the form that owns it, and by nothing
    /// else.
    pub fn of(parse_name: &str) -> Option<CoreFn> {
        if let Some(&direct) = BY_NAME.map.get(parse_name) {
            if INTERNAL_DESUGAR.contains(&parse_name) {
                return None;
            }
            return Some(direct);
        }
        OWNER_OF.get(parse_name).copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_rows_are_in_declaration_order() {
        for (i, (f, name, spellings, _)) in ROWS.iter().enumerate() {
            assert_eq!(*f as usize, i);
            assert_eq!(CoreFn::VALUES[i], *f);
            assert_eq!(format!("{f:?}"), *name);
            assert!(!spellings.is_empty(), "{name}");
        }
        assert_eq!(CoreFn::NEW.owned_fqns(), &[] as &[&str]);
        assert_eq!(CoreFn::ASC.parse_name(), "ascending");
    }

    /// FormOwnershipTest.everyCatalogNativeSpelledLikeAFormIsOwned: every catalog native spelled like a form's parse
    /// name is owned by a form -- else a qualified call to it would fall to the generic path where its bare spelling
    /// would have taken the form.
    #[test]
    fn every_catalog_native_spelled_like_a_form_is_owned() {
        let unowned = crate::testing::on_stack(|| {
            let mut unowned = Vec::new();
            for n in lite_builtin::pure::catalog().all() {
                let fqn = n.qualified_name.lossy();
                let simple = &fqn[fqn.rfind("::").map_or(0, |i| i + 2)..];
                let Some(form) = CoreFn::by_parse_name(simple) else {
                    continue;
                };
                if INTERNAL_DESUGAR.contains(&simple) {
                    continue;
                }
                if CoreFn::of(&fqn).is_none() {
                    unowned.push(format!("{fqn} (spelled like {})", form.name()));
                }
            }
            unowned
        });
        assert_eq!(
            unowned,
            Vec::<String>::new(),
            "catalog natives a form's bare spelling would take, unowned by it"
        );
    }

    /// FormOwnershipTest.theFormerAliasesAreOwned: the three former alias arms are ordinary ownership.
    #[test]
    fn the_former_aliases_are_owned() {
        assert_eq!(CoreFn::of("meta::pure::tds::extend"), Some(CoreFn::EXTEND));
        assert_eq!(CoreFn::of("meta::pure::tds::distinct"), Some(CoreFn::DISTINCT));
        assert_eq!(CoreFn::of("meta::pure::functions::relation::eval"), Some(CoreFn::EVAL));
        assert_eq!(
            CoreFn::of("meta::relational::validation::validate"),
            Some(CoreFn::VALIDATE)
        );
    }

    #[test]
    fn an_internal_desugar_name_dispatches_only_by_its_fqn() {
        assert_eq!(CoreFn::by_parse_name("tds"), Some(CoreFn::TDS));
        assert_eq!(CoreFn::of("tds"), None);
        assert_eq!(CoreFn::of("meta::legend::lite::tds"), Some(CoreFn::TDS));
        assert_eq!(CoreFn::of("joinSlot"), None);
        assert_eq!(CoreFn::of("joinWithPrefix"), Some(CoreFn::JOIN));
        assert_eq!(CoreFn::of("filter"), Some(CoreFn::FILTER));
        assert_eq!(CoreFn::of("x::filter"), None);
        assert_eq!(CoreFn::owning("filter"), None);
    }
}
