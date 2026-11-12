// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! `Pure`: the platform's catalog -- the built-in type FQNs, the native classes and enumerations, every native
//! function overload (the generated table, [`crate::natives`]: `Pure.java`'s constants and their signature texts, in its
//! load-bearing order) and the lite package's vocabulary ([`lite`]). Each declaration is parsed through the text's door
//! at the platform's dialect when the catalog is first read, as Java's class initialization does; a declaration that
//! does not parse to its one record is the platform's bug, loud (a panic, Java's `ExceptionInInitializerError`).
//!
//! A native is named by its Java constant ([`NativeId`], the generated `crate::natives::ABS__NUMBER_1`), so a constant
//! that is not in the catalog does not compile, as in Java. The FQN sets Java keeps in hash maps (`nativeClassFqns`,
//! `nativeEnumFqns`) are answered here in declaration order: they are asked for membership, never for an order.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, LazyLock};

use lite_front::doors::Dialect;
use lite_front::jtypes::JStr;
use lite_front::records as p;
use lite_model::{ClassDefinition, EnumDefinition, Function, FunctionId, NativeFunctionDefinition, PackageableElement};

use crate::natives::{self, NATIVES};

/// A native constant of the catalog: its index in `Pure.java`'s declaration order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct NativeId(pub u16);

impl NativeId {
    /// The constant's Java name (`ABS__NUMBER_1`).
    pub fn name(self) -> &'static str {
        NATIVES[self.0 as usize].0
    }

    /// The constant's signature text.
    pub fn signature(self) -> &'static str {
        NATIVES[self.0 as usize].1
    }

    /// The constant's declaration.
    pub fn def(self) -> &'static Arc<NativeFunctionDefinition> {
        &catalog().natives[self.0 as usize]
    }

    /// The constant's identity (`FunctionId.of`), computed once.
    pub fn id(self) -> &'static FunctionId {
        &catalog().ids[self.0 as usize]
    }

    /// Every native constant, in declaration order.
    pub fn all() -> impl Iterator<Item = NativeId> {
        (0..NATIVES.len()).map(|i| NativeId(i as u16))
    }

    /// The constant of a Java name, if the catalog has it.
    pub fn named(name: &str) -> Option<NativeId> {
        catalog().by_name.get(name).copied()
    }
}

/// The identities of a group of constants, in order (`FunctionId.ofAll`).
pub fn ids_of(group: &[NativeId]) -> Vec<FunctionId> {
    group.iter().map(|n| n.id().clone()).collect()
}

/// `meta::pure::metamodel::type`: the package of the primitives and `Any`.
pub const TYPE_PKG: &str = "meta::pure::metamodel::type";
/// `meta::pure::metamodel::relation`.
pub const RELATION_PKG: &str = "meta::pure::metamodel::relation";
/// `meta::pure::metamodel::function`.
pub const FUNCTION_PKG: &str = "meta::pure::metamodel::function";
/// `meta::pure::functions::relation`.
pub const RELATION_FUNCTIONS_PKG: &str = "meta::pure::functions::relation";
/// `meta::pure::functions::date`.
pub const DATE_FUNCTIONS_PKG: &str = "meta::pure::functions::date";
/// `meta::pure::functions::hash`.
pub const HASH_FUNCTIONS_PKG: &str = "meta::pure::functions::hash";
/// `meta::pure::functions::collection`.
pub const COLLECTION_PKG: &str = "meta::pure::functions::collection";
/// `meta::pure::functions::math::mathUtility`.
pub const MATH_UTILITY_PKG: &str = "meta::pure::functions::math::mathUtility";
/// `meta::pure::metamodel::variant`.
pub const VARIANT_PKG: &str = "meta::pure::metamodel::variant";
/// `meta::pure::graphFetch`.
pub const GRAPH_FETCH_PKG: &str = "meta::pure::graphFetch";
/// `meta::relational::metamodel`: the relational store's built-ins.
pub const RELATIONAL_PKG: &str = "meta::relational::metamodel";

/// `Pure.LEGACY_ASSOC_PREDICATE_FQN`.
pub const LEGACY_ASSOC_PREDICATE_FQN: &str = "meta::legend::lite::legacyAssocPredicate";
/// `Pure.LEGACY_LOCAL_PROPERTY_FQN`.
pub const LEGACY_LOCAL_PROPERTY_FQN: &str = "meta::legend::lite::legacyLocalProperty";

/// Every string constant of `Pure` (outside `Pure.Lite`) by its Java name, in declaration order.
pub const STRINGS: &[(&str, &str)] = &[
    ("TYPE_PKG", TYPE_PKG),
    ("RELATION_PKG", RELATION_PKG),
    ("FUNCTION_PKG", FUNCTION_PKG),
    ("RELATION_FUNCTIONS_PKG", RELATION_FUNCTIONS_PKG),
    ("DATE_FUNCTIONS_PKG", DATE_FUNCTIONS_PKG),
    ("HASH_FUNCTIONS_PKG", HASH_FUNCTIONS_PKG),
    ("COLLECTION_PKG", COLLECTION_PKG),
    ("MATH_UTILITY_PKG", MATH_UTILITY_PKG),
    ("VARIANT_PKG", VARIANT_PKG),
    ("GRAPH_FETCH_PKG", GRAPH_FETCH_PKG),
    ("RELATIONAL_PKG", RELATIONAL_PKG),
    ("LEGACY_ASSOC_PREDICATE_FQN", LEGACY_ASSOC_PREDICATE_FQN),
    ("LEGACY_LOCAL_PROPERTY_FQN", LEGACY_LOCAL_PROPERTY_FQN),
];

/// The native classes, by their Java constant, in declaration order: real Pure source, verbatim.
pub const NATIVE_CLASSES: &[(&str, &str)] = &[
    (
        "NUMBER",
        "native Class meta::pure::metamodel::type::Number  extends meta::pure::metamodel::type::Any {}",
    ),
    (
        "INTEGER",
        "native Class meta::pure::metamodel::type::Integer extends meta::pure::metamodel::type::Number {}",
    ),
    (
        "FLOAT",
        "native Class meta::pure::metamodel::type::Float   extends meta::pure::metamodel::type::Number {}",
    ),
    (
        "DECIMAL",
        "native Class meta::pure::metamodel::type::Decimal extends meta::pure::metamodel::type::Number {}",
    ),
    (
        "STRING",
        "native Class meta::pure::metamodel::type::String  extends meta::pure::metamodel::type::Any {}",
    ),
    (
        "BOOLEAN",
        "native Class meta::pure::metamodel::type::Boolean extends meta::pure::metamodel::type::Any {}",
    ),
    (
        "BYTE",
        "native Class meta::pure::metamodel::type::Byte    extends meta::pure::metamodel::type::Any {}",
    ),
    (
        "DATE",
        "native Class meta::pure::metamodel::type::Date        extends meta::pure::metamodel::type::Any {}",
    ),
    (
        "STRICT_DATE",
        "native Class meta::pure::metamodel::type::StrictDate  extends meta::pure::metamodel::type::Date {}",
    ),
    (
        "DATE_TIME",
        "native Class meta::pure::metamodel::type::DateTime    extends meta::pure::metamodel::type::Date {}",
    ),
    (
        "LATEST_DATE",
        "native Class meta::pure::metamodel::type::LatestDate  extends meta::pure::metamodel::type::Date {}",
    ),
    (
        "STRICT_TIME",
        "native Class meta::pure::metamodel::type::StrictTime  extends meta::pure::metamodel::type::Any {}",
    ),
];

/// The native enumerations, by their Java constant, in declaration order.
pub const NATIVE_ENUMS: &[(&str, &str)] = &[
    (
        "SORT_TYPE",
        "Enum meta::pure::functions::relation::SortType { ASC, DESC }",
    ),
    (
        "STRICT_DATE_FORMAT",
        "Enum meta::pure::functions::date::StrictDateFormat { ISO8601 }",
    ),
    (
        "DATE_TIME_FORMAT",
        "Enum meta::pure::functions::date::DateTimeFormat { ISO8601_NanoSecondPrecision }",
    ),
    (
        "HASH_TYPE",
        "Enum meta::pure::functions::hash::HashType { MD5, SHA1, SHA256 }",
    ),
    (
        "SORT_DIRECTION",
        "Enum meta::relational::metamodel::SortDirection { ASC, DESC }",
    ),
    (
        "JOIN_TYPE",
        "Enum meta::relational::metamodel::join::JoinType { INNER, LEFT_OUTER, RIGHT_OUTER, FULL_OUTER }",
    ),
];

/// `Pure.Lite`: the lite-internal native package's vocabulary, the only sanctioned spellings for internal producers
/// and consumers.
pub mod lite {
    macro_rules! lite {
        ($name:literal) => {
            concat!("meta::legend::lite::", $name)
        };
    }

    pub const PKG: &str = "meta::legend::lite::";
    // the internal desugar IR
    pub const CAST_AS_DECLARED: &str = lite!("castAsDeclared");
    pub const TYPE_AS_DECLARED: &str = lite!("typeAsDeclared");
    pub const LEGACY_NAVIGATE: &str = lite!("legacyNavigate");
    pub const ROUTE: &str = lite!("route");
    pub const LEGACY_ASSOC_PREDICATE: &str = lite!("legacyAssocPredicate");
    pub const LEGACY_LOCAL_PROPERTY: &str = lite!("legacyLocalProperty");
    pub const OTHERWISE: &str = lite!("otherwise");
    pub const PARSE_DATE_FORMAT: &str = lite!("parseDateFormat");
    pub const CONVERT_DATE_FORMAT: &str = lite!("convertDateFormat");
    pub const CONVERT_DATE_TIME_FORMAT: &str = lite!("convertDateTimeFormat");
    pub const CONVERT_TIME_ZONE_FORMAT: &str = lite!("convertTimeZoneFormat");
    pub const ADJUST_TEMPORAL: &str = lite!("adjustTemporal");
    pub const TIME_OF: &str = lite!("timeOf");
    pub const ELEMENT_ID: &str = lite!("elementId");
    pub const IS_KIND: &str = lite!("isKind");
    pub const MIN_BY: &str = lite!("minBy");
    pub const MAX_BY: &str = lite!("maxBy");
    pub const AT_TIME: &str = lite!("atTime");
    pub const INSTANT_IN: &str = lite!("instantIn");
    pub const LOCAL_IN: &str = lite!("localIn");
    pub const QUOTIENT: &str = lite!("quotient");
    pub const DATE_TIME_OF_TEXT: &str = lite!("dateTimeOfText");
    pub const TIME_OF_TEXT: &str = lite!("timeOfText");
    pub const TDS: &str = lite!("tds");
    pub const TRUST_ONE: &str = lite!("trustOne");
    pub const INLINE_VIEW: &str = lite!("inlineView");
    pub const RECURSE: &str = lite!("recurse");
    pub const STAGE: &str = lite!("stage");
    pub const WRITE_INSTANCES: &str = lite!("writeInstances");
    pub const MERGE_INSTANCES: &str = lite!("mergeInstances");
    pub const DELETE_INSTANCES: &str = lite!("deleteInstances");
    pub const LINK: &str = lite!("link");
    pub const UNLINK: &str = lite!("unlink");
    pub const ASOR_PK_VALUE: &str = lite!("asorPkValue");
    pub const ASOR_DECODE_PK_MAP: &str = lite!("asorDecodePkMap");
    // the engine-vocabulary typing shims
    pub const DIVIDE_ROUND: &str = lite!("divideRound");
    pub const NOT_EQUAL_ANSI: &str = lite!("notEqualAnsi");
    pub const LESS_THAN_ANY: &str = lite!("lessThan");
    pub const LESS_THAN_EQUAL_ANY: &str = lite!("lessThanEqual");
    pub const GREATER_THAN_ANY: &str = lite!("greaterThan");
    pub const GREATER_THAN_EQUAL_ANY: &str = lite!("greaterThanEqual");
    pub const IS_NUMERIC: &str = lite!("isNumeric");
    pub const IS_DISTINCT_FROM: &str = lite!("isDistinctFrom");
    pub const TUPLE: &str = lite!("tuple");
    pub const JOIN_SLOT: &str = lite!("joinSlot");
    pub const JOIN_WITH_PREFIX: &str = lite!("joinWithPrefix");
    pub const GROUP_BY_OVER_INSTANCES: &str = lite!("groupByOverInstances");
    pub const GROUP_BY_COMPUTED_KEYS: &str = lite!("groupByComputedKeys");
    pub const AS_OF_JOIN_WITH_PREFIX: &str = lite!("asOfJoinWithPrefix");
    // the user-facing lite natives
    pub const NAVIGATE: &str = lite!("navigate");
    pub const SOURCE_URL: &str = lite!("sourceUrl");
    pub const DELETE: &str = lite!("delete");
    pub const UPDATE: &str = lite!("update");
    pub const MERGE: &str = lite!("merge");
    pub const WITHOUT: &str = lite!("without");
    pub const READ: &str = lite!("read");

    /// Every constant of `Pure.Lite` by its Java name, in declaration order.
    pub const ALL: &[(&str, &str)] = &[
        ("PKG", PKG),
        ("CAST_AS_DECLARED", CAST_AS_DECLARED),
        ("TYPE_AS_DECLARED", TYPE_AS_DECLARED),
        ("LEGACY_NAVIGATE", LEGACY_NAVIGATE),
        ("ROUTE", ROUTE),
        ("LEGACY_ASSOC_PREDICATE", LEGACY_ASSOC_PREDICATE),
        ("LEGACY_LOCAL_PROPERTY", LEGACY_LOCAL_PROPERTY),
        ("OTHERWISE", OTHERWISE),
        ("PARSE_DATE_FORMAT", PARSE_DATE_FORMAT),
        ("CONVERT_DATE_FORMAT", CONVERT_DATE_FORMAT),
        ("CONVERT_DATE_TIME_FORMAT", CONVERT_DATE_TIME_FORMAT),
        ("CONVERT_TIME_ZONE_FORMAT", CONVERT_TIME_ZONE_FORMAT),
        ("ADJUST_TEMPORAL", ADJUST_TEMPORAL),
        ("TIME_OF", TIME_OF),
        ("ELEMENT_ID", ELEMENT_ID),
        ("IS_KIND", IS_KIND),
        ("MIN_BY", MIN_BY),
        ("MAX_BY", MAX_BY),
        ("AT_TIME", AT_TIME),
        ("INSTANT_IN", INSTANT_IN),
        ("LOCAL_IN", LOCAL_IN),
        ("QUOTIENT", QUOTIENT),
        ("DATE_TIME_OF_TEXT", DATE_TIME_OF_TEXT),
        ("TIME_OF_TEXT", TIME_OF_TEXT),
        ("TDS", TDS),
        ("TRUST_ONE", TRUST_ONE),
        ("INLINE_VIEW", INLINE_VIEW),
        ("RECURSE", RECURSE),
        ("STAGE", STAGE),
        ("WRITE_INSTANCES", WRITE_INSTANCES),
        ("MERGE_INSTANCES", MERGE_INSTANCES),
        ("DELETE_INSTANCES", DELETE_INSTANCES),
        ("LINK", LINK),
        ("UNLINK", UNLINK),
        ("ASOR_PK_VALUE", ASOR_PK_VALUE),
        ("ASOR_DECODE_PK_MAP", ASOR_DECODE_PK_MAP),
        ("DIVIDE_ROUND", DIVIDE_ROUND),
        ("NOT_EQUAL_ANSI", NOT_EQUAL_ANSI),
        ("LESS_THAN_ANY", LESS_THAN_ANY),
        ("LESS_THAN_EQUAL_ANY", LESS_THAN_EQUAL_ANY),
        ("GREATER_THAN_ANY", GREATER_THAN_ANY),
        ("GREATER_THAN_EQUAL_ANY", GREATER_THAN_EQUAL_ANY),
        ("IS_NUMERIC", IS_NUMERIC),
        ("IS_DISTINCT_FROM", IS_DISTINCT_FROM),
        ("TUPLE", TUPLE),
        ("JOIN_SLOT", JOIN_SLOT),
        ("JOIN_WITH_PREFIX", JOIN_WITH_PREFIX),
        ("GROUP_BY_OVER_INSTANCES", GROUP_BY_OVER_INSTANCES),
        ("GROUP_BY_COMPUTED_KEYS", GROUP_BY_COMPUTED_KEYS),
        ("AS_OF_JOIN_WITH_PREFIX", AS_OF_JOIN_WITH_PREFIX),
        ("NAVIGATE", NAVIGATE),
        ("SOURCE_URL", SOURCE_URL),
        ("DELETE", DELETE),
        ("UPDATE", UPDATE),
        ("MERGE", MERGE),
        ("WITHOUT", WITHOUT),
        ("READ", READ),
    ];

    /// An FQN of the package, as its bare name (`liteLocalName`).
    pub fn local_name(fqn: &str) -> &str {
        &fqn[PKG.len()..]
    }
}

/// `Pure.INTERNAL_DESUGAR`: the bare names of the internal desugar IR, in order.
pub const INTERNAL_DESUGAR: &[&str] = &[
    "castAsDeclared",
    "typeAsDeclared",
    "legacyNavigate",
    "route",
    "legacyAssocPredicate",
    "legacyLocalProperty",
    "otherwise",
    "joinSlot",
    "tds",
    "adjustTemporal",
    "trustOne",
    "asorPkValue",
    "asorDecodePkMap",
    "groupByOverInstances",
    "groupByComputedKeys",
    "tuple",
    "inlineView",
    "recurse",
    "stage",
    "writeInstances",
    "mergeInstances",
    "deleteInstances",
    "link",
    "unlink",
    "timeOf",
    "atTime",
    "instantIn",
    "localIn",
    "elementId",
    "isKind",
    "minBy",
    "maxBy",
    "dateTimeOfText",
    "timeOfText",
    "quotient",
];

/// `Pure.ENGINE_VOCAB_SHIMS`: the bare names of the engine-vocabulary typing shims, in order.
pub const ENGINE_VOCAB_SHIMS: &[&str] = &[
    "divideRound",
    "notEqualAnsi",
    "isNumeric",
    "isDistinctFrom",
    "lessThan",
    "lessThanEqual",
    "greaterThan",
    "greaterThanEqual",
    "parseDateFormat",
    "convertDateFormat",
    "convertDateTimeFormat",
    "convertTimeZoneFormat",
];

/// `Pure.LITE_SURFACE`: the bare names of the user-facing lite natives, which stay bare-name resolvable (a set; the
/// order here is the declaration's).
pub const LITE_SURFACE: &[&str] = &[
    "navigate",
    "sourceUrl",
    "joinWithPrefix",
    "asOfJoinWithPrefix",
    "delete",
    "update",
    "merge",
    "without",
    "read",
];

/// `Pure.liteSurfaceFunctions()`: each bare name of the lite surface and the overload group it stands for.
pub fn lite_surface_functions() -> Vec<(&'static str, &'static [NativeId])> {
    vec![
        ("navigate", natives::AT_LEGEND_LITE_NAVIGATE),
        ("sourceUrl", natives::AT_LEGEND_LITE_SOURCE_URL),
        ("joinWithPrefix", natives::AT_LEGEND_LITE_JOIN_WITH_PREFIX),
        ("asOfJoinWithPrefix", natives::AT_LEGEND_LITE_AS_OF_JOIN_WITH_PREFIX),
        ("delete", natives::AT_LEGEND_LITE_DELETE),
        ("update", natives::AT_LEGEND_LITE_UPDATE),
        ("merge", natives::AT_LEGEND_LITE_MERGE),
        ("without", natives::AT_LEGEND_LITE_WITHOUT),
        ("read", natives::AT_LEGEND_LITE_READ),
    ]
}

/// `Pure.isToOneCall`: user `toOne` and the lite `trustOne`, by exact FQN.
pub fn is_to_one_call(qualified_name: &str) -> bool {
    qualified_name == "meta::pure::functions::multiplicity::toOne" || qualified_name == lite::TRUST_ONE
}

/// `Pure.WALLED_NATIVES`: the natives refused by decision, each with why (a map; declaration order here).
pub const WALLED_NATIVES: &[(&str, &str)] = &[
    (
        "meta::pure::functions::meta::stereotype",
        "profile reflection — the platform's metamodel rows carry no stereotype values yet",
    ),
    (
        "meta::pure::functions::collection::replaceTreeNode",
        "in-memory tree mutation — no relational meaning",
    ),
    (
        "meta::pure::functions::io::http::executeHTTPRaw",
        "an IO effect — the database never performs HTTP",
    ),
    (
        "meta::pure::functions::lang::mutateAdd",
        "instance mutation — no relational meaning",
    ),
    (
        "meta::pure::functions::lang::toMultiplicity",
        "a multiplicity target other than [1], [1..*], [*] (the Typer desugars those to toOne / toOneMany / the identity) — a checked narrowing to [0..1] has no platform spelling",
    ),
    (
        "meta::pure::functions::meta::addColumns",
        "schema algebra on a RelationType INSTANCE (a metamodel value) — the typer's schema algebra covers types, not values",
    ),
];

/// `Pure.walledNativeReason(fqn)`.
pub fn walled_native_reason(fqn: &str) -> Option<&'static str> {
    WALLED_NATIVES.iter().find(|(f, _)| *f == fqn).map(|(_, r)| *r)
}

/// The catalog, parsed once.
pub struct Catalog {
    natives: Vec<Arc<NativeFunctionDefinition>>,
    ids: Vec<FunctionId>,
    by_name: HashMap<&'static str, NativeId>,
    classes: Vec<Arc<ClassDefinition>>,
    enums: Vec<Arc<EnumDefinition>>,
    class_by_fqn: HashMap<JStr, usize>,
    enum_by_fqn: HashMap<JStr, usize>,
    fn_by_fqn: HashMap<JStr, Vec<NativeId>>,
    fn_by_id: HashMap<JStr, NativeId>,
    user_resolvable: Vec<JStr>,
    user_resolvable_set: HashSet<JStr>,
    direct_subclasses: HashMap<JStr, Vec<JStr>>,
}

static CATALOG: LazyLock<Catalog> = LazyLock::new(build);

/// The catalog (parsed on first read).
pub fn catalog() -> &'static Catalog {
    &CATALOG
}

/// One declaration of the platform's own text, parsed at the platform's dialect: its one element.
fn one_element(source: &str) -> PackageableElement {
    let units: Vec<u16> = source.encode_utf16().collect();
    let parsed = crate::door::parse(&units, Dialect::LegendPlatform)
        .unwrap_or_else(|e| panic!("the platform's declaration does not parse: {source}: {e:?}"));
    if parsed.elements.len() != 1 {
        panic!(
            "expected exactly one element parsed from: {source} (got {})",
            parsed.elements.len()
        );
    }
    parsed.elements[0].clone()
}

fn build() -> Catalog {
    let mut natives = Vec::with_capacity(NATIVES.len());
    let mut ids = Vec::with_capacity(NATIVES.len());
    let mut by_name = HashMap::with_capacity(NATIVES.len());
    for (i, (name, signature)) in NATIVES.iter().enumerate() {
        let PackageableElement::NativeFunctionDefinition(def) = one_element(signature) else {
            panic!("expected NativeFunctionDefinition from: {signature}");
        };
        ids.push(FunctionId::of(&Function::NativeFunctionDefinition(def.clone())));
        natives.push(def);
        by_name.insert(*name, NativeId(i as u16));
    }
    let mut classes = Vec::with_capacity(NATIVE_CLASSES.len());
    for (_, source) in NATIVE_CLASSES {
        let PackageableElement::ClassDefinition(cls) = one_element(source) else {
            panic!("expected ClassDefinition from: {source}");
        };
        if !cls.is_native {
            panic!("expected native class but parsed isNative=false from: {source}");
        }
        classes.push(cls);
    }
    let mut enums = Vec::with_capacity(NATIVE_ENUMS.len());
    for (_, source) in NATIVE_ENUMS {
        let PackageableElement::EnumDefinition(def) = one_element(source) else {
            panic!("expected EnumDefinition from: {source}");
        };
        enums.push(def);
    }
    let class_by_fqn = classes
        .iter()
        .enumerate()
        .map(|(i, c)| (c.qualified_name.clone(), i))
        .collect();
    let enum_by_fqn = enums
        .iter()
        .enumerate()
        .map(|(i, e)| (e.qualified_name.clone(), i))
        .collect();
    let mut fn_by_fqn: HashMap<JStr, Vec<NativeId>> = HashMap::new();
    let mut fn_by_id = HashMap::new();
    let mut user_resolvable = Vec::new();
    let mut user_resolvable_set = HashSet::new();
    for (i, def) in natives.iter().enumerate() {
        let id = NativeId(i as u16);
        fn_by_fqn.entry(def.qualified_name.clone()).or_default().push(id);
        // the last overload declaring an id wins, as Java's HashMap.put does
        fn_by_id.insert(ids[i].qualified.clone(), id);
        let fqn = def.qualified_name.lossy();
        let bare = fqn.rfind("::").map_or(fqn.as_str(), |cut| &fqn[cut + 2..]);
        let user = !fqn.starts_with(lite::PKG) || LITE_SURFACE.contains(&bare);
        if user && user_resolvable_set.insert(def.qualified_name.clone()) {
            user_resolvable.push(def.qualified_name.clone());
        }
    }
    let mut direct_subclasses: HashMap<JStr, Vec<JStr>> = HashMap::new();
    for cd in &classes {
        for sup in cd.super_classes.iter() {
            if let p::TypeExpression::TypeExpressionNameRef(nr) = sup {
                direct_subclasses
                    .entry(nr.name.clone())
                    .or_default()
                    .push(cd.qualified_name.clone());
            }
        }
    }
    Catalog {
        natives,
        ids,
        by_name,
        classes,
        enums,
        class_by_fqn,
        enum_by_fqn,
        fn_by_fqn,
        fn_by_id,
        user_resolvable,
        user_resolvable_set,
        direct_subclasses,
    }
}

impl Catalog {
    /// `Pure.all()`: every native, in (load-bearing) declaration order.
    pub fn all(&self) -> &[Arc<NativeFunctionDefinition>] {
        &self.natives
    }

    /// `Pure.allNativeClasses()`, in declaration order.
    pub fn native_classes(&self) -> &[Arc<ClassDefinition>] {
        &self.classes
    }

    /// `Pure.allNativeEnums()`, in declaration order.
    pub fn native_enums(&self) -> &[Arc<EnumDefinition>] {
        &self.enums
    }

    /// `Pure.nativeFunctionById(qualifiedId)`: the overload whose engine id is exactly `qualified_id`.
    pub fn native_function_by_id(&self, qualified_id: &JStr) -> Option<NativeId> {
        self.fn_by_id.get(qualified_id).copied()
    }

    /// `Pure.findNativeClass(fqn)`.
    pub fn find_native_class(&self, fqn: &JStr) -> Option<&Arc<ClassDefinition>> {
        self.class_by_fqn.get(fqn).map(|&i| &self.classes[i])
    }

    /// `Pure.findNativeEnum(fqn)`.
    pub fn find_native_enum(&self, fqn: &JStr) -> Option<&Arc<EnumDefinition>> {
        self.enum_by_fqn.get(fqn).map(|&i| &self.enums[i])
    }

    /// `Pure.directNativeSubclasses(fqn)`: the native classes that directly extend `fqn`, in declaration order.
    pub fn direct_native_subclasses(&self, fqn: &JStr) -> &[JStr] {
        self.direct_subclasses.get(fqn).map_or(&[], |v| v.as_slice())
    }

    /// `Pure.userResolvableFunctionFqns()`: every non-lite native's FQN and the lite surface's, in first-declared order.
    pub fn user_resolvable_function_fqns(&self) -> &[JStr] {
        &self.user_resolvable
    }

    /// Whether a user may name `fqn`.
    pub fn is_user_resolvable(&self, fqn: &JStr) -> bool {
        self.user_resolvable_set.contains(fqn)
    }

    /// `Pure.nativeFunctionsAt(fqn)`: the overloads at an exact FQN; a bare name refused (resolution asks the bare
    /// names, registration the overload groups).
    pub fn native_functions_at(&self, fqn: &JStr) -> Result<&[NativeId], String> {
        if !lite_model::text::contains(&fqn.0, "::") {
            return Err(format!(
                "'{fqn}' is a bare name, not a declaration: resolution goes through BareNames, registration through the AT_ overload groups"
            ));
        }
        Ok(self.fn_by_fqn.get(fqn).map_or(&[], |v| v.as_slice()))
    }

    /// `Pure.isVariadicRun(fqns)`: every overload at the names takes one collection parameter (`[*]` or `[n..*]`).
    pub fn is_variadic_run(&self, fqns: &[JStr]) -> Result<bool, String> {
        let mut overloads = Vec::new();
        for fqn in fqns {
            overloads.extend_from_slice(self.native_functions_at(fqn)?);
        }
        if overloads.is_empty() {
            return Ok(false);
        }
        Ok(overloads.iter().all(|n| {
            let d = n.def();
            d.parameters.len() == 1
                && matches!(&d.parameters[0].multiplicity, p::Multiplicity::MultiplicityConcrete(c) if c.upper_bound.is_none())
        }))
    }

    /// `Pure.nativeClassFqns()`: in declaration order (Java's is a hash set's; asked for membership).
    pub fn native_class_fqns(&self) -> impl Iterator<Item = &JStr> {
        self.classes.iter().map(|c| &c.qualified_name)
    }

    /// `Pure.nativeEnumFqns()`: in declaration order (Java's is a hash set's; asked for membership).
    pub fn native_enum_fqns(&self) -> impl Iterator<Item = &JStr> {
        self.enums.iter().map(|e| &e.qualified_name)
    }

    /// `Pure.liteInternalNatives()`: the registered natives in the lite package, distinct and sorted.
    pub fn lite_internal_natives(&self) -> Vec<JStr> {
        let mut out: Vec<JStr> = self
            .natives
            .iter()
            .map(|n| n.qualified_name.clone())
            .filter(|q| q.lossy().starts_with(lite::PKG))
            .collect();
        out.sort();
        out.dedup();
        out
    }
}
