// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! `GraphEdges`' F-time rule (GraphEdges.java at 18feff3fe; EF_CENSUS §2.3, GE1 and GE2; ruling 27): the model's
//! edge classes (docs/CYPHER_STUDY_2026_10_15.md §5.6, the `graph::graph` profile) read from the declarations, and a
//! misdeclared one refused with a `ModelException` naming the class that declares the fault -- `model_integrity`
//! runs [`read_one`] on every fresh non-native class, one wall per class in a tolerant build. Lite-only (GE:49: only
//! lite runs the island); answer-free for a model that uses none of the profile's stereotypes.
//!
//! - GE1: the records ([`Edge`], [`End`]), the declarations the reader walks ([`Source`]; phase F's is the index's,
//!   [`source_of`]), the association ends injected onto each class ([`Index`]).
//! - GE2: [`read_one`] and its rule (`one`, `type_of`, `check_profile`, `is_marked`, `lineage`, `end_kind`, ...),
//!   its texts word for word. GE:48's javadoc says eight stereotypes; [`STEREOTYPES`] has fifteen, as Java's list.
//! - DON'T PORT now (EF_CENSUS §3, ruling 27): GE3, the Cypher view (`of`, `read`, `edgeAt`, `GraphEdges` the list
//!   record): it goes with the Cypher wave, with the second [`Source`] (the compiled model's, sorted by FQN).
//!
//! `Index.byOwner` is a `HashMap` looked up only (census §9.2), so its order is never seen; so is `readOne`'s `seen`
//! (a `LinkedHashSet` only added to). The texts embed Java's `List.toString` (`[a, b]`, `list_to_string`) and
//! `String.join(", ", ...)` (`join`). A stereotype's profile is compared over its units, as `PlatformTypes.isProfile`
//! compares (`profileFqn.equals(resolvedName)`, `is_graph`). (Filled 2026-11-15, the element slice, helper D.)

use std::collections::{BTreeSet, HashMap, HashSet, VecDeque};
use std::sync::Arc;

use lite_error::Phase;
use lite_front::escapes::SHORT_UNICODE_ESCAPE;
use lite_front::records as p;
use lite_model::text::Units;
use lite_model::{
    AssociationDefinition, AssociationDefinitionAssociationEndDefinition, ClassDefinition, JStr, ProfileDefinition,
    StereotypeApplication, cat,
};
use lite_names::{ModelIndex, Thrown};
use lite_types::platform::GRAPH_PROFILE;

/// `GraphEdges.Edge` (:55): an edge class -- its path, its relationship type, its two ends.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Edge {
    pub class_fqn: JStr,
    pub r#type: JStr,
    pub from: End,
    pub to: End,
}

/// `GraphEdges.End` (:62): the property on the edge class holding the node, the node's class, and, for an
/// association end declared for this very class, the inverse end the node class holds (its name, whether it holds
/// many edges); none for the edge class's own property or an inherited end.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct End {
    pub property: JStr,
    pub class_fqn: JStr,
    pub inverse: Option<JStr>,
    pub inverse_many: bool,
}

/// `GraphEdges.Source` (:68): the declarations the reader walks. A class is a class of the model: a platform
/// primitive or a native is none.
pub trait Source {
    /// `classes()`: the model's classes, natives excluded.
    fn classes(&self) -> Vec<&Arc<ClassDefinition>>;
    /// `associations()`.
    fn associations(&self) -> Vec<&Arc<AssociationDefinition>>;
    /// `findClass(fqn)`: a class of the model (no native).
    fn find_class(&self, fqn: &JStr) -> Option<&Arc<ClassDefinition>>;
    /// `findProfile(fqn)`.
    fn find_profile(&self, fqn: &JStr) -> Option<&Arc<ProfileDefinition>>;
}

/// `sourceOf(model)`'s source (:148): the builder's declarations, as phase F checks them.
#[derive(Clone, Copy)]
pub struct IndexSource<'i> {
    model: &'i ModelIndex,
}

/// `sourceOf(model)` (:148).
pub fn source_of(model: &ModelIndex) -> IndexSource<'_> {
    IndexSource { model }
}

impl Source for IndexSource<'_> {
    /// the index's classes but the natives, sorted by FQN (Java's `String` order: UTF-16 units, `JStr`'s)
    fn classes(&self) -> Vec<&Arc<ClassDefinition>> {
        let mut out: Vec<&Arc<ClassDefinition>> = self.model.classes().filter(|cd| !cd.is_native).collect();
        out.sort_by(|a, b| a.qualified_name.cmp(&b.qualified_name));
        out
    }

    /// in the index's order
    fn associations(&self) -> Vec<&Arc<AssociationDefinition>> {
        self.model.associations().collect()
    }

    fn find_class(&self, fqn: &JStr) -> Option<&Arc<ClassDefinition>> {
        self.model.find_class(fqn).filter(|cd| !cd.is_native)
    }

    fn find_profile(&self, fqn: &JStr) -> Option<&Arc<ProfileDefinition>> {
        self.model.find_profile(fqn)
    }
}

/// `GraphEdges.Index` (:80): the association ends injected onto each class (the end whose other end names the
/// class), indexed once per read.
#[derive(Debug, Clone, Default)]
pub struct Index {
    by_owner: HashMap<JStr, Vec<Injected>>,
}

/// `Index.Injected` (:83): an end injected onto a class, with the association's other end (the inverse, on the class
/// the end names).
#[derive(Debug, Clone, PartialEq)]
pub struct Injected {
    pub association: Arc<AssociationDefinition>,
    pub end: Arc<AssociationDefinitionAssociationEndDefinition>,
    pub other: Arc<AssociationDefinitionAssociationEndDefinition>,
}

impl Index {
    /// `Index.of(src)` (:87): each association's two ends, each under the class its other end names (by head name;
    /// a structural other end indexes nothing), in the source's association order.
    pub fn of(src: &dyn Source) -> Index {
        let mut out: HashMap<JStr, Vec<Injected>> = HashMap::new();
        for ad in src.associations() {
            for i in 0..2 {
                let end = if i == 0 { &ad.property1 } else { &ad.property2 };
                let other = if i == 0 { &ad.property2 } else { &ad.property1 };
                if let Some(owner) = head_name(&other.target_class) {
                    out.entry(owner.clone()).or_default().push(Injected {
                        association: Arc::clone(ad),
                        end: Arc::clone(end),
                        other: Arc::clone(other),
                    });
                }
            }
        }
        Index { by_owner: out }
    }

    /// `onto(classFqn)` (:102): the ends injected onto `class_fqn`, none for a class no association names.
    pub fn onto(&self, class_fqn: &JStr) -> &[Injected] {
        self.by_owner.get(class_fqn).map_or(&[], Vec::as_slice)
    }
}

/// `STEREOTYPES` (:380): the stereotypes the `graph::graph` profile must declare, exactly, and no tag.
pub const STEREOTYPES: [&str; 15] = [
    "Anonymous",
    "Edge",
    "Open",
    "Native",
    "from",
    "to",
    "type",
    "nanoseconds",
    "timeOfDay",
    "listOfDate",
    "listOfLocalTime",
    "listOfZonedTime",
    "listOfLocalDateTime",
    "listOfZonedDateTime",
    "listOfDuration",
];

/// `readOne(src, index, cd)` (:188): the edge class `cd` is, or none when it is none; a misdeclared one refused with
/// a `ModelException` naming the class that declares the fault (`cd`, or the superclass the faulty declaration is on).
pub fn read_one(src: &dyn Source, index: &Index, cd: &ClassDefinition) -> Result<Option<Edge>, Thrown> {
    let lineage = lineage(src, cd);
    let marked = is_marked(src, &lineage);
    // the ends: own and inherited stored properties carrying from/to (a redeclaration shadows by name), then the
    // association ends injected onto the class or a superclass
    let mut ends: Vec<EndRead> = Vec::new();
    let mut third_links: Vec<JStr> = Vec::new();
    let mut seen: HashSet<&JStr> = HashSet::new();
    for (i, fqn) in lineage.iter().enumerate() {
        let Some(c) = src.find_class(fqn) else {
            continue;
        };
        for pd in c.properties.iter() {
            if !seen.insert(&pd.name) {
                continue;
            }
            if let Some(kind) = end_kind(&pd.stereotypes, c, &pd.name)? {
                ends.push(EndRead {
                    property: pd.name.clone(),
                    kind,
                    r#type: pd.r#type.clone(),
                    multiplicity: pd.multiplicity.clone(),
                    inverse: None,
                    inverse_many: false,
                    declared_on: fqn.clone(),
                    own: i == 0,
                });
            }
        }
    }
    for (i, fqn) in lineage.iter().enumerate() {
        for injected in index.onto(fqn) {
            if !seen.insert(&injected.end.property_name) {
                continue;
            }
            let declaring: &ClassDefinition = src.find_class(fqn).map_or(cd, |c| c);
            let Some(kind) = end_kind(&injected.end.stereotypes, declaring, &injected.end.property_name)? else {
                third_links.push(cat!(
                    injected.association.qualified_name,
                    "'s end ",
                    injected.end.property_name
                ));
                continue;
            };
            // (the lineage's first class is the edge itself: an end declared for it has its inverse)
            ends.push(EndRead {
                property: injected.end.property_name.clone(),
                kind,
                r#type: injected.end.target_class.clone(),
                multiplicity: injected.end.multiplicity.clone(),
                inverse: Some(injected.other.property_name.clone()),
                inverse_many: !to_one(&injected.other.multiplicity),
                declared_on: fqn.clone(),
                own: i == 0,
            });
        }
    }
    for fqn in &lineage {
        let Some(c) = src.find_class(fqn) else {
            continue;
        };
        for dp in c.derived_properties.iter() {
            if end_kind(&to_applications(&dp.stereotypes), c, &dp.name)?.is_some() {
                return Err(refuse(
                    c,
                    cat!(
                        "carries an end stereotype on the derived property ",
                        dp.name,
                        ": a derived end (a navigation of $this to the end) is not supported yet; declare the end as",
                        " a stored property or an association end"
                    ),
                ));
            }
        }
    }
    if !marked {
        if let Some(first) = ends.first() {
            let at: &ClassDefinition = src.find_class(&first.declared_on).map_or(cd, |c| c);
            return Err(refuse(
                at,
                cat!(
                    "carries ",
                    spell(&ends),
                    " but not <<",
                    GRAPH_PROFILE,
                    ".Edge>>: an end belongs to an edge class"
                ),
            ));
        }
        for dp in cd.derived_properties.iter() {
            if type_marked(dp) {
                return Err(refuse(
                    cd,
                    cat!(
                        "carries <<",
                        GRAPH_PROFILE,
                        ".type>> on ",
                        dp.name,
                        " but not <<",
                        GRAPH_PROFILE,
                        ".Edge>>: a relationship type belongs to an edge class"
                    ),
                ));
            }
        }
        return Ok(None);
    }
    check_profile(src, cd)?;
    let from = one(src, cd, &ends, "from")?;
    let to = one(src, cd, &ends, "to")?;
    if !third_links.is_empty() {
        return Err(refuse(
            cd,
            cat!(
                "is linked by ",
                join(third_links),
                ", carrying neither <<",
                GRAPH_PROFILE,
                ".from>> nor <<",
                GRAPH_PROFILE,
                ".to>>: an edge has two ends and no third link"
            ),
        ));
    }
    for fqn in &lineage {
        let Some(c) = src.find_class(fqn) else {
            continue;
        };
        for pd in c.properties.iter() {
            let Some(target) = head_name(&pd.r#type) else {
                continue;
            };
            // (Java's `&&` order: the end markers are read last, only for a to-many class-typed property)
            if src.find_class(target).is_some()
                && !to_one(&pd.multiplicity)
                && end_kind(&pd.stereotypes, c, &pd.name)?.is_none()
            {
                return Err(refuse(
                    c,
                    cat!(
                        "holds many ",
                        target,
                        " in ",
                        pd.name,
                        ": a relationship has two ends, each <<",
                        GRAPH_PROFILE,
                        ".from>> or <<",
                        GRAPH_PROFILE,
                        ".to>> and [1]"
                    ),
                ));
            }
        }
    }
    Ok(Some(Edge {
        class_fqn: cd.qualified_name.clone(),
        r#type: type_of(src, cd, &lineage)?,
        from: from.end(),
        to: to.end(),
    }))
}

/// `EndRead` (:285): an end as read -- the property, its kind (`from`, `to`), its declared type and multiplicity,
/// its inverse when it is an association end, the class it is declared on, and whether that class is the edge.
#[derive(Debug, Clone)]
struct EndRead {
    property: JStr,
    kind: &'static str,
    r#type: p::TypeExpression,
    multiplicity: p::Multiplicity,
    inverse: Option<JStr>,
    inverse_many: bool,
    declared_on: JStr,
    own: bool,
}

impl EndRead {
    /// `end()` (:289): the end; an inverse declared for another class (a superclass) is none of this edge's.
    fn end(&self) -> End {
        End {
            property: self.property.clone(),
            // (Java's `Objects.requireNonNull(headName(type))`: `one` refused an end with no head before)
            class_fqn: head_name(&self.r#type)
                .expect("GraphEdges.EndRead.end: one() refused an end typed by no class")
                .clone(),
            inverse: if self.own { self.inverse.clone() } else { None },
            inverse_many: self.own && self.inverse_many,
        }
    }
}

/// `one(src, cd, ends, kind)` (:297): the one end of `kind` -- exactly one, `[1]`, typed by a class of the model that
/// is no edge; a fault is the declaring class's.
fn one(src: &dyn Source, cd: &ClassDefinition, ends: &[EndRead], kind: &str) -> Result<EndRead, Thrown> {
    let of: Vec<&EndRead> = ends.iter().filter(|e| e.kind == kind).collect();
    let marker = cat!("<<", GRAPH_PROFILE, ".", kind, ">>");
    let Some(&e) = of.first() else {
        return Err(refuse(
            cd,
            cat!("has no property carrying ", marker, ": an edge has a ", kind, " end"),
        ));
    };
    if of.len() > 1 {
        return Err(refuse(
            cd,
            cat!(
                "carries ",
                marker,
                " on ",
                of.len().to_string(),
                " properties (",
                join(of.iter().map(|e| e.property.clone())),
                "): an edge has one ",
                kind,
                " end"
            ),
        ));
    }
    let at: &ClassDefinition = src.find_class(&e.declared_on).map_or(cd, |c| c);
    let lower_one = matches!(&e.multiplicity, p::Multiplicity::MultiplicityConcrete(c) if c.lower_bound == 1);
    if !to_one(&e.multiplicity) || !lower_one {
        return Err(refuse(
            at,
            cat!(
                "carries ",
                marker,
                " on ",
                e.property,
                ", which is not [1]: a relationship always has its ",
                kind,
                " node"
            ),
        ));
    }
    let (Some(target), Some(target_class)) = (
        head_name(&e.r#type),
        head_name(&e.r#type).and_then(|t| src.find_class(t)),
    ) else {
        return Err(refuse(
            at,
            cat!(
                "carries ",
                marker,
                " on ",
                e.property,
                ", which is not typed by a class of the model: the ",
                kind,
                " end is a node"
            ),
        ));
    };
    if is_marked(src, &lineage(src, target_class)) {
        return Err(refuse(
            at,
            cat!(
                "carries ",
                marker,
                " on ",
                e.property,
                ", which holds the edge class ",
                target,
                ": an end is a node, never an edge"
            ),
        ));
    }
    Ok(e.clone())
}

/// `typeOf(src, cd, lineage)` (:327): the relationship type -- the enumeration value the `<<graph.type>>` derived
/// property spells (the class's own, else the one its superclasses agree on), else the class's simple name.
fn type_of(src: &dyn Source, cd: &ClassDefinition, lineage: &[JStr]) -> Result<JStr, Thrown> {
    let marker = cat!("<<", GRAPH_PROFILE, ".type>>");
    // (Java's `found` and `foundOn`: the value and the class it was found on)
    let mut found: Option<(&JStr, &JStr)> = None;
    for (i, fqn) in lineage.iter().enumerate() {
        let Some(c) = src.find_class(fqn) else {
            continue;
        };
        let typed: Vec<&Arc<p::DerivedPropertyDefinition>> =
            c.derived_properties.iter().filter(|dp| type_marked(dp)).collect();
        let Some(dp) = typed.first() else {
            continue;
        };
        if typed.len() > 1 {
            return Err(refuse(
                c,
                cat!(
                    "carries ",
                    marker,
                    " on ",
                    typed.len().to_string(),
                    " derived properties: an edge has one type"
                ),
            ));
        }
        let body: &[p::ValueSpecification] = match &dp.realization {
            p::Realization::RealizationInline(inline) => &inline.body,
            p::Realization::RealizationRef(_) => &[],
        };
        let ev = match body {
            [p::ValueSpecification::EnumValue(ev)] if dp.parameters.is_empty() => ev,
            _ => {
                return Err(refuse(
                    c,
                    cat!(
                        "carries ",
                        marker,
                        " on ",
                        dp.name,
                        ", whose body is not one spelled enumeration value: the type is a typed constant"
                    ),
                ));
            }
        };
        if i == 0 {
            // the class's own type: what it says
            return Ok(ev.value.clone());
        }
        if let Some((value, on)) = found
            && *value != ev.value
        {
            return Err(refuse(
                cd,
                cat!(
                    "inherits two relationship types, ",
                    value,
                    " (",
                    on,
                    ") and ",
                    ev.value,
                    " (",
                    fqn,
                    "): declare its own ",
                    marker
                ),
            ));
        }
        found = Some((&ev.value, fqn));
    }
    match found {
        Some((value, _)) => Ok(value.clone()),
        None => simple_name(&cd.qualified_name),
    }
}

/// `Protocol.splitFqn(qualifiedName)[1]` (Protocol:3443, at :364): the name after the last `::` outside a quoted
/// segment, both halves unquoted first (the package's unquote runs too). Its one failure is
/// `Escapes.unescapeJavaLike`'s raw `IllegalArgumentException` (Escapes:85), which Java lets escape here; lite-front's
/// `split_fqn` answers the parser's refusal in its place (PHASE2 ruling 4), so it is spelled back as Java's.
fn simple_name(qualified_name: &JStr) -> Result<JStr, Thrown> {
    match lite_front::protocol::split_fqn(&qualified_name.0) {
        Ok((_, name)) => Ok(JStr(name)),
        Err(_) => Err(Thrown::illegal_argument(JStr::from(SHORT_UNICODE_ESCAPE))),
    }
}

/// `checkProfile(src, cd)` (:389): the profile at `graph::graph`, declaring exactly [`STEREOTYPES`] and no tag
/// (its texts print Java's `List.toString`).
fn check_profile(src: &dyn Source, cd: &ClassDefinition) -> Result<(), Thrown> {
    let Some(profile) = src.find_profile(&JStr::from(GRAPH_PROFILE)) else {
        return Err(refuse(
            cd,
            cat!(
                "is an edge class, but the profile ",
                GRAPH_PROFILE,
                " is not declared: a model with edge classes depends on the graph project"
            ),
        ));
    };
    // (Java's `new TreeSet<>(a).equals(new TreeSet<>(b))`: equal as sets, the order and repeats aside)
    let expected: Vec<JStr> = STEREOTYPES.iter().map(|s| JStr::from(*s)).collect();
    let declared: BTreeSet<&JStr> = profile.stereotypes.iter().collect();
    if declared != expected.iter().collect::<BTreeSet<&JStr>>() || !profile.tags.is_empty() {
        return Err(refuse(
            cd,
            cat!(
                "is an edge class, but the profile ",
                GRAPH_PROFILE,
                " declares ",
                list_to_string(profile.stereotypes.iter()),
                " and tags ",
                list_to_string(profile.tags.iter()),
                ", not the stereotypes ",
                list_to_string(STEREOTYPES.iter()),
                " and no tag"
            ),
        ));
    }
    Ok(())
}

/// `isMarked(src, lineage)` (:403): a class of `lineage` carries `<<graph.Edge>>`.
fn is_marked(src: &dyn Source, lineage: &[JStr]) -> bool {
    for fqn in lineage {
        if let Some(c) = src.find_class(fqn)
            && c.stereotypes.iter().any(|st| is(st, "Edge"))
        {
            return true;
        }
    }
    false
}

/// `lineage(src, cd)` (:415): the class, then its superclasses (a generic one by its head), each once, breadth
/// first.
fn lineage(src: &dyn Source, cd: &ClassDefinition) -> Vec<JStr> {
    let mut out: Vec<JStr> = Vec::new();
    let mut todo: VecDeque<JStr> = VecDeque::from([cd.qualified_name.clone()]);
    while let Some(fqn) = todo.pop_front() {
        if out.contains(&fqn) {
            continue;
        }
        if let Some(c) = src.find_class(&fqn) {
            for sup in c.super_classes.iter() {
                if let Some(name) = head_name(sup) {
                    todo.push_back(name.clone());
                }
            }
        }
        out.push(fqn);
    }
    out
}

/// `is(st, name)` (:437): the stereotype is `name` of `graph::graph`.
fn is(st: &StereotypeApplication, name: &str) -> bool {
    is_graph(&st.profile_name) && st.stereotype_name.eq_str(name)
}

/// `PlatformTypes.isProfile(resolvedName, GRAPH_PROFILE)` (PlatformTypes:449, `profileFqn.equals(resolvedName)`) over
/// the name's units (lite-types' `is_profile` takes Rust text).
fn is_graph(resolved_name: &JStr) -> bool {
    resolved_name.eq_str(GRAPH_PROFILE)
}

/// `typeMarked(dp)` (:442): the derived property carries `<<graph.type>>`.
fn type_marked(dp: &p::DerivedPropertyDefinition) -> bool {
    dp.stereotypes
        .iter()
        .any(|st| is_graph(&st.profile) && st.value.eq_str("type"))
}

/// `toApplications(stereotypes)` (:451): a derived property's protocol stereotypes as the model's applications.
fn to_applications(stereotypes: &[Box<p::PStereotype>]) -> Vec<Arc<StereotypeApplication>> {
    stereotypes
        .iter()
        .map(|st| {
            Arc::new(StereotypeApplication {
                profile_name: st.profile.clone(),
                stereotype_name: st.value.clone(),
            })
        })
        .collect()
}

/// `endKind(stereotypes, declaring, property)` (:461): `from` or `to` when the stereotypes carry one end marker,
/// none otherwise; both, the declaring class's error. (The stereotypes as the records hold them, `Arc`s: Java's
/// `List<StereotypeApplication>`.)
fn end_kind(
    stereotypes: &[Arc<StereotypeApplication>],
    declaring: &ClassDefinition,
    property: &JStr,
) -> Result<Option<&'static str>, Thrown> {
    let mut kind: Option<&'static str> = None;
    for st in stereotypes {
        let here = if is(st, "from") {
            "from"
        } else if is(st, "to") {
            "to"
        } else {
            continue;
        };
        if let Some(k) = kind
            && k != here
        {
            return Err(refuse(
                declaring,
                cat!(
                    "carries both <<",
                    GRAPH_PROFILE,
                    ".from>> and <<",
                    GRAPH_PROFILE,
                    ".to>> on ",
                    property,
                    ": an end is one or the other"
                ),
            ));
        }
        kind = Some(here);
    }
    Ok(kind)
}

/// `toOne(m)` (:478): a concrete multiplicity whose upper bound is 1.
fn to_one(m: &p::Multiplicity) -> bool {
    matches!(m, p::Multiplicity::MultiplicityConcrete(c) if c.upper_bound == Some(1))
}

/// `headName(type)` (:483): the class a type expression names (a generic one by its head); none for a structural
/// type.
fn head_name(r#type: &p::TypeExpression) -> Option<&JStr> {
    match r#type {
        p::TypeExpression::TypeExpressionNameRef(nr) => Some(&nr.name),
        p::TypeExpression::TypeExpressionGeneric(g) => Some(&g.name),
        _ => None,
    }
}

/// `spell(ends)` (:491): `<<graph::graph.kind>> on property`, joined by `, `.
fn spell(ends: &[EndRead]) -> JStr {
    join(
        ends.iter()
            .map(|e| cat!("<<", GRAPH_PROFILE, ".", e.kind, ">> on ", e.property)),
    )
}

/// `refuse(cd, what)` (:499): "the class <cd> <what>", a `ModelException` (MODEL) attributed to `cd`.
fn refuse(cd: &ClassDefinition, what: JStr) -> Thrown {
    Thrown::model(
        Phase::Model,
        cat!("the class ", cd.qualified_name, " ", what),
        Some(&cd.qualified_name),
    )
}

/// `String.join(", ", parts)` (:261, :304, :496).
fn join(parts: impl IntoIterator<Item = JStr>) -> JStr {
    let mut out: Vec<u16> = Vec::new();
    for (i, part) in parts.into_iter().enumerate() {
        if i > 0 {
            ", ".push_to(&mut out);
        }
        part.push_to(&mut out);
    }
    JStr(out)
}

/// Java's `List.toString()` of strings (:397): `[a, b]`, the elements as they are, in order; `[]` when empty.
fn list_to_string<T: Units>(items: impl IntoIterator<Item = T>) -> JStr {
    let mut out: Vec<u16> = Vec::new();
    "[".push_to(&mut out);
    for (i, item) in items.into_iter().enumerate() {
        if i > 0 {
            ", ".push_to(&mut out);
        }
        item.push_to(&mut out);
    }
    "]".push_to(&mut out);
    JStr(out)
}

/// GraphEdgesTest's cases (core/src/test/java/com/legend/compiler/element/GraphEdgesTest.java at 18feff3fe) whose
/// answer is a refusal or an accepted model, driven as MI:88-91 drives the rule (every non-native class of the index
/// in ingest order: the first refusal is the strict answer, each refusal's first line under its element, first one
/// kept, the tolerant walls); the texts are the element oracle's forms for the same models
/// (rust/oracles/element/cases.jsonl, the archived forms), asserted whole. The Java test reads the edges through
/// GE3's `GraphEdges.of`; here they are `read_one`'s answers, held to the Java test's expected values. Then the
/// refusals no oracle case holds, from the Java text's concatenation.
#[cfg(test)]
mod tests {
    use lite_front::doors::Dialect;
    use lite_model::linked::LinkedMap;

    use super::*;

    const PROFILE: &str = "Profile graph::graph\n{\n   stereotypes: [Anonymous, Edge, Open, Native, from, to, type, \
        nanoseconds, timeOfDay,\n       listOfDate, listOfLocalTime, listOfZonedTime, listOfLocalDateTime, \
        listOfZonedDateTime, listOfDuration];\n}\n";

    const NODES: &str = "Class demo::Actor { name: String[1]; }\nClass demo::Movie { title: String[1]; }\n\
        Enum demo::RelationshipType { ACTED_IN, DIRECTED }\n";

    /// A model text parsed, its names resolved and indexed, as phase F receives the index (lite-normalizer's
    /// test_support, the lines this needs).
    fn index(text: &str) -> ModelIndex {
        let units: Vec<u16> = text.encode_utf16().collect();
        let parsed = lite_builtin::door::parse(&units, Dialect::LegendLite).expect("the model parses");
        let resolved = lite_names::resolver::resolve(&parsed).expect("the model's names resolve");
        ModelIndex::from(&resolved).expect("the model indexes")
    }

    /// MI:88-91: `read_one` on every non-native class of the index, in ingest order.
    fn read_all(model: &ModelIndex) -> Vec<(String, Result<Option<Edge>, Thrown>)> {
        let src = source_of(model);
        let graph_index = Index::of(&src);
        model
            .classes()
            .filter(|cd| !cd.is_native)
            .map(|cd| (cd.qualified_name.lossy(), read_one(&src, &graph_index, cd)))
            .collect()
    }

    /// A refusal as (element, message), held to be a `ModelException` of phase MODEL.
    fn spelled(thrown: &Thrown) -> (String, String) {
        match thrown {
            Thrown::Compile(e) => {
                assert_eq!(e.kind, lite_error::ErrorKind::Model, "{thrown:?}");
                assert_eq!(e.phase, Some(Phase::Model), "{thrown:?}");
                let element = String::from_utf16(e.element.as_ref().expect("an element")).expect("text");
                let message = String::from_utf16(e.message.as_ref().expect("a message")).expect("text");
                (element, message)
            }
            Thrown::Java { .. } => panic!("not a ModelException: {thrown:?}"),
        }
    }

    /// The strict answer: the first refusal.
    fn refused(text: &str) -> (String, String) {
        let model = index(text);
        let first = read_all(&model).into_iter().find_map(|(_, r)| r.err());
        spelled(&first.expect("a refusal"))
    }

    /// The tolerant walls: each refusal's first line under its element (the checked class has none to add: `refuse`
    /// always names one), the first kept (`withElement`'s `putIfAbsent`).
    fn walls(text: &str) -> Vec<(String, String)> {
        let mut out: LinkedMap<JStr, JStr> = LinkedMap::new();
        for (_, outcome) in read_all(&index(text)) {
            if let Err(thrown) = outcome {
                let (element, _) = spelled(&thrown);
                out.put_if_absent(JStr::from(element), thrown.first_line());
            }
        }
        out.iter().map(|(k, v)| (k.lossy(), v.lossy())).collect()
    }

    /// The edges `read_one` reads, every class accepted.
    fn edges(text: &str) -> Vec<Edge> {
        read_all(&index(text))
            .into_iter()
            .filter_map(|(fqn, r)| r.unwrap_or_else(|e| panic!("{fqn} refused: {e:?}")))
            .collect()
    }

    fn edge(class: &str, r#type: &str, from: End, to: End) -> Edge {
        Edge {
            class_fqn: JStr::from(class),
            r#type: JStr::from(r#type),
            from,
            to,
        }
    }

    fn end(property: &str, class: &str, inverse: Option<&str>, inverse_many: bool) -> End {
        End {
            property: JStr::from(property),
            class_fqn: JStr::from(class),
            inverse: inverse.map(JStr::from),
            inverse_many,
        }
    }

    fn at(element: &str, message: &str) -> (String, String) {
        (element.to_string(), message.to_string())
    }

    /// `theAssociationFormReadsItsEndsAndTheirInverses` (:38; oracle 28d4ca2b: accepted).
    #[test]
    fn the_association_form_reads_its_ends_and_their_inverses() {
        let text = format!(
            "{PROFILE}{NODES}Class <<graph::graph.Edge>> demo::Acting\n{{\n  <<graph::graph.type>> type() {{ \
             demo::RelationshipType.ACTED_IN }}: demo::RelationshipType[1];\n  role: String[1];\n}}\n\
             Association demo::ActorActing {{ <<graph::graph.from>> actor: demo::Actor[1]; actings: demo::Acting[*]; \
             }}\nAssociation demo::ActingMovie {{ <<graph::graph.to>> movie: demo::Movie[1]; cast: demo::Acting[*]; \
             }}\n"
        );
        assert_eq!(
            edges(&text),
            vec![edge(
                "demo::Acting",
                "ACTED_IN",
                end("actor", "demo::Actor", Some("actings"), true),
                end("movie", "demo::Movie", Some("cast"), true)
            )]
        );
        let model = index(&text);
        let outcomes = read_all(&model);
        let actor = outcomes
            .iter()
            .find(|(fqn, _)| fqn == "demo::Actor")
            .expect("demo::Actor");
        assert_eq!(actor.1, Ok(None));
    }

    /// `theCompactFormHasNoInverseAndTheTypeIsTheSimpleName` (:56; oracle 9d32bdf1: accepted).
    #[test]
    fn the_compact_form_has_no_inverse_and_the_type_is_the_simple_name() {
        let text = format!(
            "{PROFILE}{NODES}Class <<graph::graph.Edge>> demo::Directing\n{{\n  <<graph::graph.from>> director: \
             demo::Actor[1];\n  <<graph::graph.to>> movie: demo::Movie[1];\n}}\n"
        );
        assert_eq!(
            edges(&text),
            vec![edge(
                "demo::Directing",
                "Directing",
                end("director", "demo::Actor", None, false),
                end("movie", "demo::Movie", None, false)
            )]
        );
    }

    /// `theIslandsViewAgreesWithPhaseFOnAToManyPrimitiveProperty` (:70; oracle 65cc8fb0: accepted).
    #[test]
    fn a_to_many_primitive_or_enumeration_property_is_accepted() {
        let text = format!(
            "{PROFILE}{NODES}Class <<graph::graph.Edge>> demo::Acting\n{{\n  <<graph::graph.from>> actor: \
             demo::Actor[1];\n  <<graph::graph.to>> movie: demo::Movie[1];\n  roles: String[*];\n  kinds: \
             demo::RelationshipType[*];\n}}\n"
        );
        assert_eq!(edges(&text).len(), 1);
    }

    /// `anEndTypedByAPrimitiveOrAnEnumerationIsRefused` (:86; oracles fd3ccb15, bb4c758d, 96474646).
    #[test]
    fn an_end_typed_by_a_primitive_an_enumeration_or_an_edge_is_refused() {
        let primitive = format!(
            "{PROFILE}{NODES}Class <<graph::graph.Edge>> demo::E {{ <<graph::graph.from>> actor: String[1]; \
             <<graph::graph.to>> movie: demo::Movie[1]; }}\n"
        );
        assert_eq!(
            refused(&primitive),
            at(
                "demo::E",
                "the class demo::E carries <<graph::graph.from>> on actor, which is not typed by a class of the \
                 model: the from end is a node"
            )
        );
        let enumeration = format!(
            "{PROFILE}{NODES}Class <<graph::graph.Edge>> demo::E {{ <<graph::graph.from>> actor: demo::Actor[1]; \
             <<graph::graph.to>> movie: demo::RelationshipType[1]; }}\n"
        );
        assert_eq!(
            refused(&enumeration),
            at(
                "demo::E",
                "the class demo::E carries <<graph::graph.to>> on movie, which is not typed by a class of the model: \
                 the to end is a node"
            )
        );
        let edge_end = format!(
            "{PROFILE}{NODES}Class <<graph::graph.Edge>> demo::E {{ <<graph::graph.from>> actor: demo::Actor[1]; \
             <<graph::graph.to>> movie: demo::Movie[1]; }}\nClass <<graph::graph.Edge>> demo::F {{ \
             <<graph::graph.from>> e: demo::E[1]; <<graph::graph.to>> movie: demo::Movie[1]; }}\n"
        );
        assert_eq!(
            walls(&edge_end),
            vec![at(
                "demo::F",
                "the class demo::F carries <<graph::graph.from>> on e, which holds the edge class demo::E: an end is \
                 a node, never an edge"
            )]
        );
    }

    /// `anInheritedAssociationEndHasNoInverseForTheSubclass` (:103; oracle 85c78b29: accepted).
    #[test]
    fn an_inherited_association_end_has_no_inverse_for_the_subclass() {
        let text = format!(
            "{PROFILE}{NODES}Class <<graph::graph.Edge>> demo::L {{ role: String[1]; }}\nClass demo::Sub extends \
             demo::L {{ credit: String[0..1]; }}\nAssociation demo::ActorL {{ <<graph::graph.from>> actor: \
             demo::Actor[1]; links: demo::L[*]; }}\nAssociation demo::LMovie {{ <<graph::graph.to>> movie: \
             demo::Movie[1]; linked: demo::L[*]; }}\n"
        );
        assert_eq!(
            edges(&text),
            vec![
                edge(
                    "demo::L",
                    "L",
                    end("actor", "demo::Actor", Some("links"), true),
                    end("movie", "demo::Movie", Some("linked"), true)
                ),
                edge(
                    "demo::Sub",
                    "Sub",
                    end("actor", "demo::Actor", None, false),
                    end("movie", "demo::Movie", None, false)
                ),
            ]
        );
    }

    /// `anEndWithoutTheMarkerIsRefusedWhateverTheClassOrder` (:121; oracle 21a7fa5a, its first text).
    #[test]
    fn an_end_without_the_marker_is_refused_whatever_the_class_order() {
        for other in ["demo::AAA", "demo::ZZZ"] {
            let text = format!(
                "{PROFILE}{NODES}Class demo::Acting {{ role: String[1]; }}\nAssociation demo::ActorActing {{ \
                 <<graph::graph.from>> actor: demo::Actor[1]; actings: demo::Acting[*]; }}\nAssociation \
                 demo::ActingMovie {{ <<graph::graph.to>> movie: demo::Movie[1]; cast: demo::Acting[*]; }}\n\
                 Class <<graph::graph.Edge>> {other} {{ <<graph::graph.from>> a: demo::Actor[1]; \
                 <<graph::graph.to>> m: demo::Movie[1]; }}\n"
            );
            assert_eq!(
                walls(&text),
                vec![at(
                    "demo::Acting",
                    "the class demo::Acting carries <<graph::graph.from>> on actor, <<graph::graph.to>> on movie but \
                     not <<graph::graph.Edge>>: an end belongs to an edge class"
                )],
                "{other}"
            );
        }
    }

    /// `aThirdLinkOnAnEdgeIsRefused` (:135; oracle d9c4d9fa).
    #[test]
    fn a_third_link_on_an_edge_is_refused() {
        let text = format!(
            "{PROFILE}{NODES}Class <<graph::graph.Edge>> demo::Acting\n{{\n  <<graph::graph.from>> actor: \
             demo::Actor[1];\n  <<graph::graph.to>> movie: demo::Movie[1];\n}}\nAssociation demo::Third {{ \
             producer: demo::Actor[1]; produced: demo::Acting[*]; }}\n"
        );
        assert_eq!(
            walls(&text),
            vec![at(
                "demo::Acting",
                "the class demo::Acting is linked by demo::Third's end producer, carrying neither \
                 <<graph::graph.from>> nor <<graph::graph.to>>: an edge has two ends and no third link"
            )]
        );
    }

    /// `aTypeMarkerWithoutTheEdgeMarkerIsRefused` (:148; oracle fcab30a8).
    #[test]
    fn a_type_marker_without_the_edge_marker_is_refused() {
        let text = format!(
            "{PROFILE}{NODES}Class demo::Plain {{ <<graph::graph.type>> type() {{ demo::RelationshipType.ACTED_IN \
             }}: demo::RelationshipType[1]; }}\n"
        );
        assert_eq!(
            walls(&text),
            vec![at(
                "demo::Plain",
                "the class demo::Plain carries <<graph::graph.type>> on type but not <<graph::graph.Edge>>: a \
                 relationship type belongs to an edge class"
            )]
        );
    }

    /// `twoInheritedTypesThatDisagreeAreRefused` (:156; oracle 4781d134: R's wall first, then Both's).
    #[test]
    fn two_inherited_types_that_disagree_are_refused() {
        let text = format!(
            "{PROFILE}{NODES}Class <<graph::graph.Edge>> demo::L\n{{\n  <<graph::graph.from>> a: demo::Actor[1];\n  \
             <<graph::graph.to>> m: demo::Movie[1];\n  <<graph::graph.type>> type() {{ \
             demo::RelationshipType.ACTED_IN }}: demo::RelationshipType[1];\n}}\nClass demo::R {{ \
             <<graph::graph.type>> kind() {{ demo::RelationshipType.DIRECTED }}: demo::RelationshipType[1]; }}\n\
             Class demo::Both extends demo::L, demo::R {{}}\n"
        );
        assert_eq!(
            walls(&text),
            vec![
                at(
                    "demo::R",
                    "the class demo::R carries <<graph::graph.type>> on kind but not <<graph::graph.Edge>>: a \
                     relationship type belongs to an edge class"
                ),
                at(
                    "demo::Both",
                    "the class demo::Both inherits two relationship types, ACTED_IN (demo::L) and DIRECTED \
                     (demo::R): declare its own <<graph::graph.type>>"
                ),
            ]
        );
    }

    /// `aDerivedEndIsRefusedByName` (:172; oracle c8f39482).
    #[test]
    fn a_derived_end_is_refused_by_name() {
        let text = format!(
            "{PROFILE}{NODES}Class <<graph::graph.Edge>> demo::Acting\n{{\n  <<graph::graph.from>> from() {{ \
             $this.actor }}: demo::Actor[1];\n  <<graph::graph.to>> movie: demo::Movie[1];\n  actor: \
             demo::Actor[1];\n}}\n"
        );
        assert_eq!(
            walls(&text),
            vec![at(
                "demo::Acting",
                "the class demo::Acting carries an end stereotype on the derived property from: a derived end (a \
                 navigation of $this to the end) is not supported yet; declare the end as a stored property or an \
                 association end"
            )]
        );
    }

    /// `anInheritedFaultNamesTheDeclaringClass` (:185; oracle 6601900f: Z's wall, then A's).
    #[test]
    fn an_inherited_fault_names_the_declaring_class() {
        let text = format!(
            "{PROFILE}{NODES}Class demo::Z {{ <<graph::graph.from>> x: demo::Actor[1]; }}\nClass \
             <<graph::graph.Edge>> demo::A extends demo::Z {{ <<graph::graph.to>> m: demo::Movie[1]; x: \
             demo::Actor[0..1]; }}\n"
        );
        assert_eq!(
            walls(&text),
            vec![
                at(
                    "demo::Z",
                    "the class demo::Z carries <<graph::graph.from>> on x but not <<graph::graph.Edge>>: an end \
                     belongs to an edge class"
                ),
                at(
                    "demo::A",
                    "the class demo::A has no property carrying <<graph::graph.from>>: an edge has a from end"
                ),
            ]
        );
    }

    /// `aSubclassInheritsTheMarkerTheEndsAndTheType` (:196; oracle 81e43356: accepted).
    #[test]
    fn a_subclass_inherits_the_marker_the_ends_and_the_type() {
        let text = format!(
            "{PROFILE}{NODES}Class <<graph::graph.Edge>> demo::Link\n{{\n  <<graph::graph.from>> a: demo::Actor[1];\n  \
             <<graph::graph.to>> m: demo::Movie[1];\n  <<graph::graph.type>> type() {{ \
             demo::RelationshipType.DIRECTED }}: demo::RelationshipType[1];\n}}\nClass demo::Credited extends \
             demo::Link {{ credit: String[0..1]; }}\n"
        );
        assert_eq!(
            edges(&text),
            vec![
                edge(
                    "demo::Link",
                    "DIRECTED",
                    end("a", "demo::Actor", None, false),
                    end("m", "demo::Movie", None, false)
                ),
                edge(
                    "demo::Credited",
                    "DIRECTED",
                    end("a", "demo::Actor", None, false),
                    end("m", "demo::Movie", None, false)
                ),
            ]
        );
    }

    /// `anEndWithoutTheMarkerIsRefused` (:214; oracle 472f235e).
    #[test]
    fn an_end_without_the_marker_is_refused() {
        let text = format!("{PROFILE}{NODES}Class demo::Plain {{ <<graph::graph.from>> actor: demo::Actor[1]; }}\n");
        assert_eq!(
            refused(&text),
            at(
                "demo::Plain",
                "the class demo::Plain carries <<graph::graph.from>> on actor but not <<graph::graph.Edge>>: an end \
                 belongs to an edge class"
            )
        );
    }

    /// `aMarkerWithoutBothEndsIsRefused` (:222; oracles 5c51ecdf and ae9dd02c).
    #[test]
    fn a_marker_without_both_ends_is_refused() {
        let text = format!(
            "{PROFILE}{NODES}Class <<graph::graph.Edge>> demo::Half {{ <<graph::graph.from>> actor: demo::Actor[1]; \
             }}\n"
        );
        assert_eq!(
            refused(&text),
            at(
                "demo::Half",
                "the class demo::Half has no property carrying <<graph::graph.to>>: an edge has a to end"
            )
        );
        let k7 = "###Pure\nProfile graph::graph\n{\n  stereotypes: [Anonymous, Edge, Open, Native, from, to, type, \
                  nanoseconds, timeOfDay, listOfDate, listOfLocalTime, listOfZonedTime, listOfLocalDateTime, \
                  listOfZonedDateTime, listOfDuration];\n}\nClass w::N7 { }\nClass <<graph::graph.Edge>> w::K7 { \
                  <<graph::graph.from>> a: w::N7[1]; }\n";
        assert_eq!(
            walls(k7),
            vec![at(
                "w::K7",
                "the class w::K7 has no property carrying <<graph::graph.to>>: an edge has a to end"
            )]
        );
    }

    /// `anOptionalEndIsRefused` (:230; oracle 9eefd42a).
    #[test]
    fn an_optional_end_is_refused() {
        let text = format!(
            "{PROFILE}{NODES}Class <<graph::graph.Edge>> demo::Loose\n{{\n  <<graph::graph.from>> actor: \
             demo::Actor[0..1];\n  <<graph::graph.to>> movie: demo::Movie[1];\n}}\n"
        );
        assert_eq!(
            refused(&text),
            at(
                "demo::Loose",
                "the class demo::Loose carries <<graph::graph.from>> on actor, which is not [1]: a relationship \
                 always has its from node"
            )
        );
    }

    /// `aTypeThatIsNotOneEnumerationValueIsRefused` (:242; oracle 5b43be28).
    #[test]
    fn a_type_that_is_not_one_enumeration_value_is_refused() {
        let text = format!(
            "{PROFILE}{NODES}Class <<graph::graph.Edge>> demo::Acting\n{{\n  <<graph::graph.from>> actor: \
             demo::Actor[1];\n  <<graph::graph.to>> movie: demo::Movie[1];\n  <<graph::graph.type>> type() {{ \
             'ACTED_IN' }}: String[1];\n}}\n"
        );
        assert_eq!(
            refused(&text),
            at(
                "demo::Acting",
                "the class demo::Acting carries <<graph::graph.type>> on type, whose body is not one spelled \
                 enumeration value: the type is a typed constant"
            )
        );
    }

    /// `aToManyClassPropertyOnAnEdgeIsRefused` (:255; oracle a93e306c).
    #[test]
    fn a_to_many_class_property_on_an_edge_is_refused() {
        let text = format!(
            "{PROFILE}{NODES}Class <<graph::graph.Edge>> demo::Acting\n{{\n  <<graph::graph.from>> actor: \
             demo::Actor[1];\n  <<graph::graph.to>> movie: demo::Movie[1];\n  others: demo::Actor[*];\n}}\n"
        );
        assert_eq!(
            refused(&text),
            at(
                "demo::Acting",
                "the class demo::Acting holds many demo::Actor in others: a relationship has two ends, each \
                 <<graph::graph.from>> or <<graph::graph.to>> and [1]"
            )
        );
    }

    /// `theProfileMustBeDeclared` (:268; oracle 36692374): the refusal, "Can't find the profile 'graph'", is the
    /// integrity pass's annotation check, before the rule; the bare `graph` resolves to no profile, so this rule
    /// reads no marker and accepts every class.
    #[test]
    fn a_bare_profile_name_marks_nothing() {
        let text = format!(
            "{NODES}Class <<graph.Edge>> demo::Acting\n{{\n  <<graph.from>> actor: demo::Actor[1];\n  <<graph.to>> \
             movie: demo::Movie[1];\n}}\n"
        );
        let model = index(&text);
        for (fqn, outcome) in read_all(&model) {
            assert_eq!(outcome, Ok(None), "{fqn}");
        }
    }

    /// `aModelWithoutEdgesReadsNone` (:282).
    #[test]
    fn a_model_without_edges_reads_none() {
        let model = index(NODES);
        let outcomes = read_all(&model);
        assert_eq!(outcomes.len(), 2);
        for (fqn, outcome) in outcomes {
            assert_eq!(outcome, Ok(None), "{fqn}");
        }
    }

    // The refusals no oracle case holds, each text from the Java's concatenation.

    /// `endKind` (:470): both end markers on one property, the declaring class's error.
    #[test]
    fn both_end_markers_on_one_property_are_refused() {
        let text = format!(
            "{PROFILE}{NODES}Class <<graph::graph.Edge>> demo::B {{ <<graph::graph.from, graph::graph.to>> x: \
             demo::Actor[1]; <<graph::graph.to>> m: demo::Movie[1]; }}\n"
        );
        assert_eq!(
            refused(&text),
            at(
                "demo::B",
                "the class demo::B carries both <<graph::graph.from>> and <<graph::graph.to>> on x: an end is one or \
                 the other"
            )
        );
    }

    /// `one` (:304): two ends of a kind, counted and listed.
    #[test]
    fn two_ends_of_a_kind_are_refused() {
        let text = format!(
            "{PROFILE}{NODES}Class <<graph::graph.Edge>> demo::E {{ <<graph::graph.from>> a: demo::Actor[1]; \
             <<graph::graph.from>> b: demo::Actor[1]; <<graph::graph.to>> m: demo::Movie[1]; }}\n"
        );
        assert_eq!(
            refused(&text),
            at(
                "demo::E",
                "the class demo::E carries <<graph::graph.from>> on 2 properties (a, b): an edge has one from end"
            )
        );
    }

    /// `typeOf` (:343): two type markers on one class.
    #[test]
    fn two_type_markers_on_one_class_are_refused() {
        let text = format!(
            "{PROFILE}{NODES}Class <<graph::graph.Edge>> demo::E {{ <<graph::graph.from>> a: demo::Actor[1]; \
             <<graph::graph.to>> m: demo::Movie[1]; <<graph::graph.type>> t1() {{ demo::RelationshipType.ACTED_IN \
             }}: demo::RelationshipType[1]; <<graph::graph.type>> t2() {{ demo::RelationshipType.DIRECTED }}: \
             demo::RelationshipType[1]; }}\n"
        );
        assert_eq!(
            refused(&text),
            at(
                "demo::E",
                "the class demo::E carries <<graph::graph.type>> on 2 derived properties: an edge has one type"
            )
        );
    }

    /// `checkProfile` (:395): a declaration other than the fifteen stereotypes and no tag, printed as declared.
    #[test]
    fn a_profile_declaring_other_stereotypes_or_a_tag_is_refused() {
        let text = format!(
            "Profile graph::graph\n{{\n  stereotypes: [Edge, to, from];\n  tags: [t];\n}}\n{NODES}Class \
             <<graph::graph.Edge>> demo::E {{ <<graph::graph.from>> a: demo::Actor[1]; <<graph::graph.to>> m: \
             demo::Movie[1]; }}\n"
        );
        assert_eq!(
            refused(&text),
            at(
                "demo::E",
                "the class demo::E is an edge class, but the profile graph::graph declares [Edge, to, from] and tags \
                 [t], not the stereotypes [Anonymous, Edge, Open, Native, from, to, type, nanoseconds, timeOfDay, \
                 listOfDate, listOfLocalTime, listOfZonedTime, listOfLocalDateTime, listOfZonedDateTime, \
                 listOfDuration] and no tag"
            )
        );
        let tagged = format!(
            "Profile graph::graph\n{{\n  stereotypes: [Anonymous, Edge, Open, Native, from, to, type, nanoseconds, \
             timeOfDay, listOfDate, listOfLocalTime, listOfZonedTime, listOfLocalDateTime, listOfZonedDateTime, \
             listOfDuration];\n  tags: [t, u];\n}}\n{NODES}Class <<graph::graph.Edge>> demo::E {{ \
             <<graph::graph.from>> a: demo::Actor[1]; <<graph::graph.to>> m: demo::Movie[1]; }}\n"
        );
        assert_eq!(
            refused(&tagged),
            at(
                "demo::E",
                "the class demo::E is an edge class, but the profile graph::graph declares [Anonymous, Edge, Open, \
                 Native, from, to, type, nanoseconds, timeOfDay, listOfDate, listOfLocalTime, listOfZonedTime, \
                 listOfLocalDateTime, listOfZonedDateTime, listOfDuration] and tags [t, u], not the stereotypes \
                 [Anonymous, Edge, Open, Native, from, to, type, nanoseconds, timeOfDay, listOfDate, \
                 listOfLocalTime, listOfZonedTime, listOfLocalDateTime, listOfZonedDateTime, listOfDuration] and no \
                 tag"
            )
        );
    }

    /// `checkProfile` (:395): the stereotypes compared as sets, the order aside.
    #[test]
    fn a_profile_declaring_the_stereotypes_in_another_order_is_accepted() {
        let text = format!(
            "Profile graph::graph\n{{\n  stereotypes: [listOfDuration, listOfZonedDateTime, listOfLocalDateTime, \
             listOfZonedTime, listOfLocalTime, listOfDate, timeOfDay, nanoseconds, type, to, from, Native, Open, \
             Edge, Anonymous];\n}}\n{NODES}Class <<graph::graph.Edge>> demo::E {{ <<graph::graph.from>> a: \
             demo::Actor[1]; <<graph::graph.to>> m: demo::Movie[1]; }}\n"
        );
        assert_eq!(edges(&text).len(), 1);
    }

    /// `checkProfile` (:392): an edge class in a model that declares no `graph::graph` profile. (Through the
    /// integrity pass the class carrying the stereotype is refused first by the annotation check, "Can't find the
    /// profile", in a strict build; this text is then the wall of a subclass, which carries no stereotype of its own
    /// and inherits the marker.)
    #[test]
    fn an_edge_class_without_the_profile_is_refused() {
        let text = format!(
            "{NODES}Class <<graph::graph.Edge>> demo::E {{ <<graph::graph.from>> a: demo::Actor[1]; \
             <<graph::graph.to>> m: demo::Movie[1]; }}\nClass demo::S extends demo::E {{ }}\n"
        );
        let model = index(&text);
        let not_declared = |class: &str| {
            format!(
                "the class {class} is an edge class, but the profile graph::graph is not declared: a model with edge \
                 classes depends on the graph project"
            )
        };
        assert_eq!(
            walls(&text),
            vec![
                at("demo::E", &not_declared("demo::E")),
                at("demo::S", &not_declared("demo::S")),
            ]
        );
        // (a class that is no edge never asks for the profile)
        let outcomes = read_all(&model);
        let actor = &outcomes
            .iter()
            .find(|(fqn, _)| fqn == "demo::Actor")
            .expect("demo::Actor")
            .1;
        assert_eq!(actor, &Ok(None));
    }

    /// `one` (:308): an inherited end's fault is the class declaring it, for the edge and for its subclass (whose
    /// wall, under the same element, is dropped).
    #[test]
    fn an_inherited_ends_fault_is_the_declaring_classes_for_a_subclass_too() {
        let text = format!(
            "{PROFILE}{NODES}Class <<graph::graph.Edge>> demo::Base {{ <<graph::graph.from>> a: demo::Actor[0..1]; \
             <<graph::graph.to>> m: demo::Movie[1]; }}\nClass demo::Sub extends demo::Base {{ }}\n"
        );
        let model = index(&text);
        let outcomes = read_all(&model);
        let sub = outcomes.iter().find(|(fqn, _)| fqn == "demo::Sub").expect("demo::Sub");
        let message = "the class demo::Base carries <<graph::graph.from>> on a, which is not [1]: a relationship \
                       always has its from node";
        assert_eq!(spelled(sub.1.as_ref().expect_err("refused")), at("demo::Base", message));
        assert_eq!(walls(&text), vec![at("demo::Base", message)]);
    }

    /// The to-many loop (:272): `endKind` read last, so a superclass property the subclass shadows (unread by the
    /// ends' loop) refuses its two markers there, under its own class.
    #[test]
    fn a_shadowed_property_with_both_markers_is_refused_by_the_to_many_rule() {
        let text = format!(
            "{PROFILE}{NODES}Class demo::Z {{ <<graph::graph.from, graph::graph.to>> x: demo::Actor[*]; }}\nClass \
             <<graph::graph.Edge>> demo::E extends demo::Z {{ <<graph::graph.from>> a: demo::Actor[1]; \
             <<graph::graph.to>> m: demo::Movie[1]; x: demo::Actor[1]; }}\n"
        );
        let model = index(&text);
        let outcomes = read_all(&model);
        let both = "the class demo::Z carries both <<graph::graph.from>> and <<graph::graph.to>> on x: an end is one \
                    or the other";
        // (Z's own read refuses it in the ends' loop; E's, which skips the shadowed x there, in the to-many loop)
        for class in ["demo::Z", "demo::E"] {
            let outcome = &outcomes.iter().find(|(fqn, _)| fqn == class).expect("the class").1;
            assert_eq!(
                spelled(outcome.as_ref().expect_err("refused")),
                at("demo::Z", both),
                "{class}"
            );
        }
        assert_eq!(walls(&text), vec![at("demo::Z", both)]);
    }

    /// `lineage` (:415): a generic superclass by its head, its marker and ends inherited.
    #[test]
    fn a_generic_superclass_is_read_by_its_head() {
        let text = format!(
            "{PROFILE}{NODES}Class <<graph::graph.Edge>> demo::G<T> {{ <<graph::graph.from>> a: demo::Actor[1]; \
             <<graph::graph.to>> m: demo::Movie[1]; }}\nClass demo::S extends demo::G<String> {{ }}\n"
        );
        assert_eq!(
            edges(&text),
            vec![
                edge(
                    "demo::G",
                    "G",
                    end("a", "demo::Actor", None, false),
                    end("m", "demo::Movie", None, false)
                ),
                edge(
                    "demo::S",
                    "S",
                    end("a", "demo::Actor", None, false),
                    end("m", "demo::Movie", None, false)
                ),
            ]
        );
    }

    /// `Index.of` (:87): an end under the class its other end names, in association order.
    #[test]
    fn the_index_holds_each_end_under_the_class_its_other_end_names() {
        let model = index(&format!(
            "{NODES}Association demo::ActorMovie {{ films: demo::Movie[*]; star: demo::Actor[1]; }}\n"
        ));
        let graph_index = Index::of(&source_of(&model));
        let onto_actor: Vec<String> = graph_index
            .onto(&JStr::from("demo::Actor"))
            .iter()
            .map(|i| cat!(i.end.property_name, "/", i.other.property_name).lossy())
            .collect();
        assert_eq!(onto_actor, vec!["films/star".to_string()]);
        let onto_movie: Vec<String> = graph_index
            .onto(&JStr::from("demo::Movie"))
            .iter()
            .map(|i| cat!(i.end.property_name, "/", i.other.property_name).lossy())
            .collect();
        assert_eq!(onto_movie, vec!["star/films".to_string()]);
        assert!(graph_index.onto(&JStr::from("demo::Nope")).is_empty());
    }

    /// `typeOf` (:364): the simple name is `Protocol.splitFqn`'s, the last segment outside quotes, unquoted.
    #[test]
    fn the_simple_name_is_split_fqns() {
        assert_eq!(simple_name(&JStr::from("demo::Acting")), Ok(JStr::from("Acting")));
        assert_eq!(simple_name(&JStr::from("Acting")), Ok(JStr::from("Acting")));
        assert_eq!(simple_name(&JStr::from("demo::'a::b'")), Ok(JStr::from("a::b")));
        assert_eq!(
            simple_name(&JStr::from(r"demo::'a\u12'")),
            Err(Thrown::illegal_argument(JStr::from(
                "Less than 4 hex digits in unicode escape"
            )))
        );
    }
}
