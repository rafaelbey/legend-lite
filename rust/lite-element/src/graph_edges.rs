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
//! `Index.byOwner` is a `HashMap` looked up only (census §9.2), so its order is never seen.

use std::collections::HashMap;
use std::sync::Arc;

use lite_front::records as p;
use lite_model::{
    AssociationDefinition, AssociationDefinitionAssociationEndDefinition, ClassDefinition, JStr, ProfileDefinition,
    StereotypeApplication,
};
use lite_names::{ModelIndex, Thrown};

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
        unimplemented!("lite-element skeleton: GraphEdges.Index.of")
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
    unimplemented!("lite-element skeleton: GraphEdges.readOne")
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
        unimplemented!("lite-element skeleton: GraphEdges.EndRead.end")
    }
}

/// `one(src, cd, ends, kind)` (:297): the one end of `kind` -- exactly one, `[1]`, typed by a class of the model that
/// is no edge; a fault is the declaring class's.
fn one(src: &dyn Source, cd: &ClassDefinition, ends: &[EndRead], kind: &str) -> Result<EndRead, Thrown> {
    unimplemented!("lite-element skeleton: GraphEdges.one")
}

/// `typeOf(src, cd, lineage)` (:327): the relationship type -- the enumeration value the `<<graph.type>>` derived
/// property spells (the class's own, else the one its superclasses agree on), else the class's simple name.
fn type_of(src: &dyn Source, cd: &ClassDefinition, lineage: &[JStr]) -> Result<JStr, Thrown> {
    unimplemented!("lite-element skeleton: GraphEdges.typeOf")
}

/// `checkProfile(src, cd)` (:389): the profile at `graph::graph`, declaring exactly [`STEREOTYPES`] and no tag
/// (its texts print Java's `List.toString`).
fn check_profile(src: &dyn Source, cd: &ClassDefinition) -> Result<(), Thrown> {
    unimplemented!("lite-element skeleton: GraphEdges.checkProfile")
}

/// `isMarked(src, lineage)` (:403): a class of `lineage` carries `<<graph.Edge>>`.
fn is_marked(src: &dyn Source, lineage: &[JStr]) -> bool {
    unimplemented!("lite-element skeleton: GraphEdges.isMarked")
}

/// `lineage(src, cd)` (:415): the class, then its superclasses (a generic one by its head), each once, breadth
/// first.
fn lineage(src: &dyn Source, cd: &ClassDefinition) -> Vec<JStr> {
    unimplemented!("lite-element skeleton: GraphEdges.lineage")
}

/// `is(st, name)` (:437): the stereotype is `name` of `graph::graph`.
fn is(st: &StereotypeApplication, name: &str) -> bool {
    unimplemented!("lite-element skeleton: GraphEdges.is")
}

/// `typeMarked(dp)` (:442): the derived property carries `<<graph.type>>`.
fn type_marked(dp: &p::DerivedPropertyDefinition) -> bool {
    unimplemented!("lite-element skeleton: GraphEdges.typeMarked")
}

/// `endKind(stereotypes, declaring, property)` (:461): `from` or `to` when the stereotypes carry one end marker,
/// none otherwise; both, the declaring class's error.
fn end_kind(
    stereotypes: &[StereotypeApplication],
    declaring: &ClassDefinition,
    property: &JStr,
) -> Result<Option<&'static str>, Thrown> {
    unimplemented!("lite-element skeleton: GraphEdges.endKind")
}

/// `toOne(m)` (:478): a concrete multiplicity whose upper bound is 1.
fn to_one(m: &p::Multiplicity) -> bool {
    unimplemented!("lite-element skeleton: GraphEdges.toOne")
}

/// `headName(type)` (:483): the class a type expression names (a generic one by its head); none for a structural
/// type.
fn head_name(r#type: &p::TypeExpression) -> Option<&JStr> {
    unimplemented!("lite-element skeleton: GraphEdges.headName")
}

/// `spell(ends)` (:491): `<<graph::graph.kind>> on property`, joined by `, `.
fn spell(ends: &[EndRead]) -> JStr {
    unimplemented!("lite-element skeleton: GraphEdges.spell")
}

/// `refuse(cd, what)` (:499): "the class <cd> <what>", a `ModelException` (MODEL) attributed to `cd`.
fn refuse(cd: &ClassDefinition, what: JStr) -> Thrown {
    unimplemented!("lite-element skeleton: GraphEdges.refuse")
}
