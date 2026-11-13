// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! The knowledge layer (2026-11-12, the names slice: Java's `KnowledgeLayer`, its part that is step D's): what the
//! model knows about classes, their hierarchy, their properties and associations, settled before Phase E normalizes
//! mappings against it. Two faces, as in Java:
//! - the per-graph **kernel** over the one model index ([`ModelIndex::knowledge`]): class lookup native-first (the
//!   platform catalog, then the graph), the subtype relation (memoized per class: its reach), the ancestor and subtree
//!   walks, a property's declared type, definition and multiplicity over the hierarchy, a table and its columns over
//!   the database include closure, a table's or a view's column's kind ([`Knowledge::column_kind`]);
//! - the static **pass** over a name-resolved model: association qualified properties adopted into their owners
//!   ([`adopt_association_qualified_properties`]).
//!
//! **This relation is not F's.** `KnowledgeLayer.isSubtype` is the normalizer's (E's) relation over declarations: a
//! primitive is no class here (`hierarchyClass`), a supertype head that is no name contributes nothing, and `Nil` is no
//! bottom. `PureModelContext.isSubtype` (F's, the typer's) walks the compiled classes on its own -- `Nil` the bottom, a
//! primitive's lattice walked, a class that fails to compile the question's failure -- and asks this layer only for
//! the lookup ([`Knowledge::class_def`]). Java keeps both; so does the port (docs/rust-port/PHASE3.md, the names slice).
//!
//! `columnKind`, left out by the names slice (its computed-view-column arm asks `RelationalTypeInference`, then F's), is
//! here since the names follow-up (2026-11-12): the owner's ruling moved the inference into this crate, and the
//! names-to-element edge of PHASE3_CRATES.md §2 with it.

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::{Arc, Mutex};

use lite_builtin::pure::catalog;
use lite_error::Phase;
use lite_model::linked::LinkedMap;
use lite_model::protocol as p;
use lite_model::{
    AssociationDefinition, ClassDefinition, ClassDefinitionPropertyDefinition, DatabaseDefinitionColumnDefinition,
    DatabaseDefinitionTableDefinition, JStr, PackageableElement, ParsedModel, RelationalOperation, cat,
};
use lite_types::Primitive;

use crate::error::Thrown;
use crate::java_string;
use crate::model_index::{ModelIndex, StoreLookups};
use crate::{relational_kinds, relational_type_inference};

/// The kernel's memo: each asked class's reach (itself and every class its declared supers lead to).
#[derive(Default)]
pub struct KnowledgeMemo {
    reach: Mutex<HashMap<JStr, Arc<HashSet<JStr>>>>,
}

/// The knowledge kernel over one index (Java's `KnowledgeLayer` instance).
pub struct Knowledge<'m> {
    model: &'m ModelIndex,
    memo: &'m KnowledgeMemo,
}

fn is_primitive(fqn: &JStr) -> bool {
    Primitive::find_by_fqn(&fqn.lossy()).is_some()
}

impl<'m> Knowledge<'m> {
    pub(crate) fn new(model: &'m ModelIndex, memo: &'m KnowledgeMemo) -> Knowledge<'m> {
        Knowledge { model, memo }
    }

    /// `classDef(fqn)`: the class behind an FQN, native first (a platform class the catalog declares is read from the
    /// catalog even when the graph carries a copy).
    pub fn class_def(&self, fqn: &JStr) -> Option<&'m Arc<ClassDefinition>> {
        if let Some(nat) = catalog().find_native_class(fqn) {
            return Some(nat);
        }
        self.model.find_class(fqn)
    }

    /// `hierarchyClass(fqn)`: [`class_def`](Self::class_def) for the mapping calculus and the hierarchy walks -- a
    /// primitive is no class there, a null name no class.
    pub fn hierarchy_class(&self, fqn: Option<&JStr>) -> Option<&'m Arc<ClassDefinition>> {
        let fqn = fqn?;
        if is_primitive(fqn) {
            return None;
        }
        self.class_def(fqn)
    }

    /// `superClassFqns(cd)`: the declared superclass heads, bare and generic alike.
    pub fn super_class_fqns(&self, cd: &ClassDefinition) -> Vec<JStr> {
        cd.super_classes
            .iter()
            .filter_map(|s| s.raw_class_name().cloned())
            .collect()
    }

    /// `isSubtype(child, parent)`: `child` is `parent` or a transitive declared subclass of it (an unknown class is
    /// nobody's subtype; a cycle contributes nothing new).
    pub fn is_subtype(&self, child: &JStr, parent: &JStr) -> bool {
        child == parent || self.reach(child).contains(parent)
    }

    /// The class's reach, memoized: the one walk ([`lite_model::subtyping::reach`]) in E's shape -- raw heads, no
    /// bottom, `hierarchyClass`'s lookup.
    fn reach(&self, cls: &JStr) -> Arc<HashSet<JStr>> {
        if let Some(known) = self.memo.reach.lock().expect("reach").get(cls) {
            return known.clone();
        }
        let out = lite_model::subtyping::reach(cls, lite_model::subtyping::Heads::Raw, |f| {
            self.hierarchy_class(Some(f)).map(|c| &**c)
        })
        .classes;
        let frozen = Arc::new(out);
        self.memo
            .reach
            .lock()
            .expect("reach")
            .entry(cls.clone())
            .or_insert(frozen)
            .clone()
    }

    /// `ancestorsAndSelf(cls)`: `cls` and every ancestor, breadth first (nearest first), cycle-guarded.
    pub fn ancestors_and_self(&self, cls: &JStr) -> Result<Vec<JStr>, Thrown> {
        self.ancestors_below(cls, None)
    }

    /// `ancestorsBelow(cls, root)`: `cls` and its ancestors breadth first, stopping at `root` (excluded, nothing
    /// climbed through it); a class above the start the index cannot answer is a model gap (`IllegalStateException`).
    pub fn ancestors_below(&self, cls: &JStr, root: Option<&JStr>) -> Result<Vec<JStr>, Thrown> {
        let mut out: Vec<JStr> = Vec::new();
        let mut seen: HashSet<JStr> = HashSet::new();
        let mut work: VecDeque<JStr> = VecDeque::from([cls.clone()]);
        while let Some(cur) = work.pop_front() {
            if root == Some(&cur) || !seen.insert(cur.clone()) {
                continue;
            }
            out.push(cur.clone());
            if &cur == cls {
                // the starting class may be a name the index does not know: itself, and nothing above
                if let Some(cd) = self.hierarchy_class(Some(&cur)) {
                    work.extend(self.super_class_fqns(cd));
                }
            } else {
                let cd = self.super_class(&cur)?;
                work.extend(self.super_class_fqns(cd));
            }
        }
        Ok(out)
    }

    /// `directSubtypes(cls)`: the classes that directly extend `cls`, the graph's (off the index) then the catalog's;
    /// a natively declared FQN reads the catalog's edges, and a primitive is no class for the calculus.
    pub fn direct_subtypes(&self, cls: &JStr) -> Vec<JStr> {
        let mut out = Vec::new();
        for sub in self.model.direct_subclasses(cls) {
            if catalog().find_native_class(sub).is_none() && !is_primitive(sub) {
                out.push(sub.clone());
            }
        }
        for sub in catalog().direct_native_subclasses(cls) {
            if !is_primitive(sub) {
                out.push(sub.clone());
            }
        }
        out
    }

    /// `subtree(base)`: the strict subtree of `base` (every transitive subclass, graph and catalog), discovery order.
    pub fn subtree(&self, base: &JStr) -> Vec<JStr> {
        let mut subtree: Vec<JStr> = Vec::new();
        let mut seen: HashSet<JStr> = HashSet::new();
        let mut frontier: VecDeque<JStr> = VecDeque::from([base.clone()]);
        while let Some(c) = frontier.pop_front() {
            for sub in self.direct_subtypes(&c) {
                if seen.insert(sub.clone()) {
                    subtree.push(sub.clone());
                    frontier.push_back(sub);
                }
            }
        }
        subtree.retain(|s| s != base);
        subtree
    }

    /// `lineage(cls)`: `cls` and every ancestor as definitions, nearest first; loud for every class on the way.
    pub fn lineage(&self, cls: &JStr) -> Result<Vec<Arc<ClassDefinition>>, Thrown> {
        let mut out = Vec::new();
        let mut seen: HashSet<JStr> = HashSet::new();
        let mut work: VecDeque<JStr> = VecDeque::from([cls.clone()]);
        while let Some(cur) = work.pop_front() {
            if !seen.insert(cur.clone()) {
                continue;
            }
            let cd = self.super_class(&cur)?;
            work.extend(self.super_class_fqns(cd));
            out.push(cd.clone());
        }
        Ok(out)
    }

    /// `superClass(fqn)`: the class behind a superclass met on a hierarchy walk; unresolved is a real model gap.
    fn super_class(&self, fqn: &JStr) -> Result<&'m Arc<ClassDefinition>, Thrown> {
        self.hierarchy_class(Some(fqn)).ok_or_else(|| {
            Thrown::illegal_state(cat!("class unresolved on a hierarchy walk (a real model gap): ", fqn))
        })
    }

    /// `propertyType(cd, name)`: the declared type of property `name` seen from `cd`: its own stored property, else an
    /// association end injected onto it, else the same on each superclass in turn.
    pub fn property_type(
        &self,
        cd: Option<&ClassDefinition>,
        name: &JStr,
    ) -> Result<Option<p::TypeExpression>, Thrown> {
        self.property_type_in(cd, name, &mut HashSet::new())
    }

    fn property_type_in(
        &self,
        cd: Option<&ClassDefinition>,
        name: &JStr,
        visited: &mut HashSet<JStr>,
    ) -> Result<Option<p::TypeExpression>, Thrown> {
        let Some(cd) = cd else { return Ok(None) };
        if !visited.insert(cd.qualified_name.clone()) {
            return Ok(None);
        }
        if let Some(pd) = cd.properties.iter().find(|pd| &pd.name == name) {
            return Ok(Some(pd.r#type.clone()));
        }
        if let Some(assoc) = self.model.find_association_property(&cd.qualified_name, name) {
            return Ok(Some(assoc.clone()));
        }
        for sup in self.super_class_fqns(cd) {
            let s = self.super_class(&sup)?;
            if let Some(inherited) = self.property_type_in(Some(s), name, visited)? {
                return Ok(Some(inherited));
            }
        }
        Ok(None)
    }

    /// `propertyDef(cd, name)`: the stored-property definition `name` on `cd` or the nearest superclass declaring it.
    pub fn property_def(
        &self,
        cd: Option<&ClassDefinition>,
        name: &JStr,
    ) -> Result<Option<Arc<ClassDefinitionPropertyDefinition>>, Thrown> {
        self.property_def_in(cd, name, &mut HashSet::new())
    }

    fn property_def_in(
        &self,
        cd: Option<&ClassDefinition>,
        name: &JStr,
        visited: &mut HashSet<JStr>,
    ) -> Result<Option<Arc<ClassDefinitionPropertyDefinition>>, Thrown> {
        let Some(cd) = cd else { return Ok(None) };
        if !visited.insert(cd.qualified_name.clone()) {
            return Ok(None);
        }
        if let Some(pd) = cd.properties.iter().find(|pd| &pd.name == name) {
            return Ok(Some(pd.clone()));
        }
        for sup in self.super_class_fqns(cd) {
            let s = self.super_class(&sup)?;
            if let Some(inherited) = self.property_def_in(Some(s), name, visited)? {
                return Ok(Some(inherited));
            }
        }
        Ok(None)
    }

    /// `propertyMultiplicity(cd, name)`.
    pub fn property_multiplicity(&self, cd: &ClassDefinition, name: &JStr) -> Result<Option<p::Multiplicity>, Thrown> {
        Ok(self.property_def(Some(cd), name)?.map(|pd| pd.multiplicity.clone()))
    }

    /// `derivedInline(cd, name)`: the zero-argument, single-expression inline derived property `name` on `cd` or the
    /// nearest superclass declaring one.
    pub fn derived_inline(
        &self,
        cd: Option<&ClassDefinition>,
        name: &JStr,
    ) -> Result<Option<p::DerivedPropertyDefinition>, Thrown> {
        self.derived_inline_in(cd, name, &mut HashSet::new())
    }

    fn derived_inline_in(
        &self,
        cd: Option<&ClassDefinition>,
        name: &JStr,
        visited: &mut HashSet<JStr>,
    ) -> Result<Option<p::DerivedPropertyDefinition>, Thrown> {
        let Some(cd) = cd else { return Ok(None) };
        if !visited.insert(cd.qualified_name.clone()) {
            return Ok(None);
        }
        for dp in cd.derived_properties.iter() {
            if &dp.name == name
                && dp.parameters.is_empty()
                && let p::Realization::RealizationInline(inl) = &dp.realization
                && inl.body.len() == 1
            {
                return Ok(Some((**dp).clone()));
            }
        }
        for sup in self.super_class_fqns(cd) {
            let s = self.super_class(&sup)?;
            if let Some(r) = self.derived_inline_in(Some(s), name, visited)? {
                return Ok(Some(r));
            }
        }
        Ok(None)
    }

    /// `table(db, table)`: the physical table behind a mapping's spelling in `db` (the one table lookup); empty for a
    /// null spelling, an unknown database, or a view.
    pub fn table(&self, db_fqn: Option<&JStr>, table: Option<&JStr>) -> Option<Arc<DatabaseDefinitionTableDefinition>> {
        let (Some(db), Some(t)) = (db_fqn, table) else {
            return None;
        };
        self.model.find_table_definition(db, t).cloned()
    }

    /// `column(db, table, column)`: the physical column of [`table`](Self::table), by exact name.
    pub fn column(
        &self,
        db_fqn: Option<&JStr>,
        table: Option<&JStr>,
        column: &JStr,
    ) -> Option<Arc<DatabaseDefinitionColumnDefinition>> {
        self.table(db_fqn, table)
            .and_then(|td| td.columns.iter().find(|c| &c.name == column).cloned())
    }

    /// `columnKind(db, table, col)`: the Pure kind ([`relational_kinds`]) of a column of a table OR of a VIEW -- a view
    /// column that reads one physical column (a column reference) has that column's kind, through views of views; a
    /// COMPUTED view column has its expression's inferred SQL type's kind (the engine's `inferRelationalType`, the rule
    /// the metamodel store stamps on every relational-operation row: [`relational_type_inference`]); `None` when nothing
    /// physical is behind the name. (Completed 2026-11-12 by the names follow-up, the inference moved into this crate.)
    ///
    /// The walk's guard is Java's: a set of `db + "@" + table + "." + col` strings, so two spellings that concatenate
    /// alike are one (ported as it is).
    pub fn column_kind(&self, db: &JStr, table: &JStr, col: &JStr) -> Option<&'static str> {
        self.column_kind_in(db, table, col, &mut HashSet::new())
    }

    fn column_kind_in(&self, db: &JStr, table: &JStr, col: &JStr, seen: &mut HashSet<JStr>) -> Option<&'static str> {
        if !seen.insert(cat!(db, "@", table, ".", col)) {
            return None;
        }
        if let Some(cd) = self.column(Some(db), Some(table), col) {
            return Some(relational_kinds::pure_kind_of(&cd.data_type));
        }
        let view = self.model.find_view(db, table)?;
        for vc in view.column_mappings.iter() {
            if &vc.name != col {
                continue;
            }
            if let RelationalOperation::RelationalOperationColumnRef(cr) = &vc.expression {
                let cdb = cr.database_name.clone().unwrap_or_else(|| db.clone());
                return self.column_kind_in(&cdb, &cr.table, &cr.column, seen);
            }
            // a COMPUTED view column (`HIGHEST_RATE: max(RATE.ZERO_RATE)`): its kind is the expression's inferred SQL
            // type -- the engine's inferRelationalType, the same rule the metamodel store stamps on every
            // relational-operation row
            let scope = self.model.find_database(db).map(|d| &**d);
            let t = relational_type_inference::infer(&vc.expression, scope, Some(self.model as &dyn StoreLookups));
            return t.map(|t| relational_kinds::pure_kind_of(&t));
        }
        None
    }
}

/// `canonicalTable(table)`: `default.T` is the top-level table `T`.
pub fn canonical_table(table: &JStr) -> JStr {
    let prefix: Vec<u16> = "default.".encode_utf16().collect();
    if table.0.starts_with(&prefix) {
        JStr::from(&table.0[prefix.len()..])
    } else {
        table.clone()
    }
}

/// `adoptAssociationQualifiedProperties(parsed, walls)`: association qualified properties adopt into the class that
/// owns them (the end opposite the one the property returns). A property naming no unique owning end is a model error:
/// strict (`walls` `None`) throws it; tolerant records the association's first line in `walls` and adopts nothing from
/// it. The rebuilt model keeps its source, offsets, imports, sources and unclaimed sections, and drops its element spans
/// (Java's seven-argument `ParsedModel` constructor).
pub fn adopt_association_qualified_properties(
    parsed: &ParsedModel,
    mut walls: Option<&mut LinkedMap<JStr, JStr>>,
) -> Result<ParsedModel, Thrown> {
    let mut adoptions: LinkedMap<JStr, Vec<Box<p::DerivedPropertyDefinition>>> = LinkedMap::new();
    for el in parsed.elements.iter() {
        let PackageableElement::AssociationDefinition(ad) = el else {
            continue;
        };
        if ad.derived_properties.is_empty() {
            continue;
        }
        let mut own: Vec<(JStr, &Box<p::DerivedPropertyDefinition>)> = Vec::new();
        let mut failed = None;
        for dp in ad.derived_properties.iter() {
            match owner_or_throw(ad, dp) {
                Ok(owner) => own.push((owner, dp)),
                Err(e) => {
                    failed = Some(e);
                    break;
                }
            }
        }
        if let Some(e) = failed {
            if !e.is_model() {
                return Err(e);
            }
            match walls.as_deref_mut() {
                None => return Err(e),
                Some(w) => {
                    w.put_if_absent(ad.qualified_name.clone(), e.first_line());
                    continue;
                }
            }
        }
        for (owner, dp) in own {
            adoptions.get_or_insert_with(owner, Vec::new).push(dp.clone());
        }
    }
    if adoptions.is_empty() {
        return Ok(parsed.clone());
    }
    let mut out: Vec<PackageableElement> = Vec::with_capacity(parsed.elements.len());
    for el in parsed.elements.iter() {
        if let PackageableElement::ClassDefinition(cd) = el
            && let Some(adopted) = adoptions.get(&cd.qualified_name)
        {
            let mut merged: Vec<Box<p::DerivedPropertyDefinition>> = cd.derived_properties.to_vec();
            merged.extend(adopted.iter().cloned());
            out.push(PackageableElement::ClassDefinition(Arc::new(ClassDefinition {
                qualified_name: cd.qualified_name.clone(),
                type_params: cd.type_params.clone(),
                type_variables: cd.type_variables.clone(),
                super_classes: cd.super_classes.clone(),
                properties: cd.properties.clone(),
                derived_properties: merged.into(),
                constraints: cd.constraints.clone(),
                stereotypes: cd.stereotypes.clone(),
                tagged_values: cd.tagged_values.clone(),
                is_native: cd.is_native,
                multiplicity_params: cd.multiplicity_params.clone(),
            })));
        } else {
            out.push(el.clone());
        }
    }
    Ok(ParsedModel {
        elements: lite_model::linked::list(out),
        imports: parsed.imports.clone(),
        source: parsed.source.clone(),
        element_offsets: parsed.element_offsets.clone(),
        element_imports: parsed.element_imports.clone(),
        element_sources: parsed.element_sources.clone(),
        unclaimed_sections: parsed.unclaimed_sections.clone(),
        element_spans: Arc::default(),
    })
}

/// `qualifiedPropertyOwner(ad, dp)`: the class that owns an association's qualified property -- the end opposite the
/// one it returns; a self-association owns its qualified properties itself; `None` when no unique end is identified.
pub fn qualified_property_owner(
    ad: &AssociationDefinition,
    dp: &p::DerivedPropertyDefinition,
) -> Result<Option<JStr>, Thrown> {
    let t1 = raw_name(&ad.property1.target_class)?;
    let t2 = raw_name(&ad.property2.target_class)?;
    let ret = raw_name(&dp.r#type)?;
    if t1 == t2 {
        return Ok(if ret == t1 { Some(t1) } else { None });
    }
    if ret == t1 {
        return Ok(Some(t2));
    }
    Ok(if ret == t2 { Some(t1) } else { None })
}

fn owner_or_throw(ad: &AssociationDefinition, dp: &p::DerivedPropertyDefinition) -> Result<JStr, Thrown> {
    if let Some(owner) = qualified_property_owner(ad, dp)? {
        return Ok(owner);
    }
    let t1 = raw_name(&ad.property1.target_class)?;
    let t2 = raw_name(&ad.property2.target_class)?;
    let ret = raw_name(&dp.r#type)?;
    let why = if t1 == t2 {
        "', which is neither end of the self-association"
    } else {
        "', which does not identify a unique owning end"
    };
    Err(Thrown::model(
        Phase::Model,
        cat!(
            "association '",
            &ad.qualified_name,
            "' qualified property '",
            &dp.name,
            "' returns '",
            ret,
            why
        ),
        Some(&ad.qualified_name),
    ))
}

/// `rawName(t)`: a name's or a generic's head, else the type's Java `toString()`.
fn raw_name(t: &p::TypeExpression) -> Result<JStr, Thrown> {
    match t {
        p::TypeExpression::TypeExpressionNameRef(n) => Ok(n.name.clone()),
        p::TypeExpression::TypeExpressionGeneric(g) => Ok(g.name.clone()),
        other => java_string::type_expression(other),
    }
}
