// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! `LiftedViews` (LiftedViews.java at ad86fa895, ported line for line): THE store VIEWS as lifted functions (E.5) --
//! ONE owner of every view's relation body. A view is a zero-arg relation function (`<db>$view$<name>()`, provenance
//! `SynthHat.VIEW`) whose single body expression is `ViewRelation.viewRelationExpr`'s output; every consumer (the
//! mapping normalizer's expansion sites, and the lift itself for a view whose root or hop is another view) reads the
//! SAME body from here, keyed by the view definition's IDENTITY: two schemas may declare a view of one name with
//! different bodies, and two equal views in two schemas are two views (EF_CENSUS §9.1, step 0's (b): `owners`,
//! `functions`, `walls`, `expanding` are identity collections -- [`IdMap`] and [`IdSet`] over the view record's `Arc`,
//! which the index hands out once per view from both `findView` and `viewsOf`).
//!
//! Built BEFORE the mapping normalizer runs and handed to it as an input of the same phase. Bodies are memoized on
//! demand, re-entrantly (a body's translation asks for other views' bodies: no `RefCell` borrow is held across it),
//! with the identity cycle guard; a body that walls keeps its wall, the SAME error at every later ask (Java rethrows
//! the memoized exception object). Only what Java's catch takes is memoized (`ModelException | NotImplementedException`);
//! anything else propagates and leaves no wall. [`LiftedViews::lift_all`] makes the lift eager, like E.2-E.4: a wall is
//! recorded under the lifted FQN when a sink exists (its FULL message, no element: ASK 10 (a), ported as Java has it),
//! thrown under the strict entry.

use std::cell::RefCell;
use std::sync::Arc;

use lite_base::{IdMap, IdSet, QueryArena};
use lite_error::Phase;
use lite_front::records as p;
use lite_model::linked::{LinkedMap, empty};
use lite_model::{
    DatabaseDefinitionViewDefinition, FunctionDefinition, FunctionDefinitionSynthesized, JStr, PackageableElement,
    ParsedModel, SynthHat, cat,
};
use lite_names::{ModelIndex, ViewLift};
use lite_types::{Multiplicity, Primitive, RelationType, Type, platform};

use crate::error::Thrown;
use crate::view_relation;

type View = Arc<DatabaseDefinitionViewDefinition>;

/// The lifted views of one model, for one `normalize` call.
pub struct LiftedViews<'m> {
    model: &'m ModelIndex,
    owners: IdMap<View, ViewLift>,
    order: Vec<View>,
    functions: RefCell<IdMap<View, Arc<FunctionDefinition>>>,
    walls: RefCell<IdMap<View, Thrown>>,
    expanding: RefCell<IdSet<View>>,
}

impl<'m> LiftedViews<'m> {
    /// `new LiftedViews(parsed, model)` (:59): every database element's views, as the index spelled them once, in
    /// declaration order (a database declared twice -- a tolerant build -- lists its views twice, as Java's does).
    pub fn new(parsed: &ParsedModel, model: &'m ModelIndex) -> LiftedViews<'m> {
        let mut owners = IdMap::new();
        let mut order = Vec::new();
        for el in parsed.elements.iter() {
            let PackageableElement::DatabaseDefinition(db) = el else {
                continue;
            };
            for iv in model.views_of(&db.qualified_name) {
                owners.insert(iv.definition.clone(), iv.lift.clone());
                order.push(iv.definition.clone());
            }
        }
        LiftedViews {
            model,
            owners,
            order,
            functions: RefCell::new(IdMap::new()),
            walls: RefCell::new(IdMap::new()),
            expanding: RefCell::new(IdSet::new()),
        }
    }

    /// `body(view)` (:76): the view's relation body -- the lifted function's one body expression, computed once.
    /// Fails with the view's own wall when it cannot be expanded (the same error at every site, the lift's or a
    /// mapping's).
    pub fn body(&self, view: &View) -> Result<p::ValueSpecification, Thrown> {
        if let Some(f) = self.functions.borrow().get(view) {
            return Ok(f.body[0].clone());
        }
        if let Some(wall) = self.walls.borrow().get(view) {
            return Err(wall.clone());
        }
        let Some(owner) = self.owners.get(view) else {
            return Err(Thrown::model(
                Phase::Normalize,
                cat!("view '", &view.name, "' is not declared by any database of this model"),
                None,
            ));
        };
        if !self.expanding.borrow_mut().insert(view.clone()) {
            return Err(Thrown::model(
                Phase::Normalize,
                cat!(
                    "view '",
                    &owner.spelling,
                    "' expands through itself (cyclic view-on-view chain); store=",
                    &owner.owner_db
                ),
                None,
            ));
        }
        let lifted = self.lift(view, owner);
        // the catch, then the finally
        if let Err(e) = &lifted
            && e.is_model_or_not_implemented()
        {
            self.walls.borrow_mut().insert(view.clone(), e.clone());
        }
        self.expanding.borrow_mut().remove(view);
        lifted
    }

    /// The `try` of `body` (:95-108): THE signature from store facts (`ViewSignatures`, in an arena of this call),
    /// the body emitted to conform to it, the lifted function memoized.
    fn lift(&self, view: &View, owner: &ViewLift) -> Result<p::ValueSpecification, Thrown> {
        let arena = QueryArena::new();
        let signature = lite_names::view_signatures::of(&arena, self.model, &owner.owner_db, view)?;
        let body =
            view_relation::view_relation_expr(view, &owner.spelling, &owner.owner_db, self.model, self, signature)?;
        let function = FunctionDefinition::user_written(
            owner.fqn()?,
            empty(),
            empty(),
            empty(),
            LiftedViews::spell(signature),
            pure_one(),
            Arc::from(vec![body.clone()]),
            empty(),
            empty(),
        )
        .with_synthesized_from(Arc::new(FunctionDefinitionSynthesized::new(
            SynthHat::VIEW,
            owner.owner_db.clone(),
            view.name.clone(),
        )));
        self.functions.borrow_mut().insert(view.clone(), Arc::new(function));
        Ok(body)
    }

    /// `spell(rt)` (:121): a relation type spelled as the declaration a user would write, `Relation<(name: Type[m],
    /// ...)>` -- a primitive by its name, a precise decimal as `Decimal` (a declaration carries no precision), anything
    /// else by its type name; `[1]` stays `[1]`, any other multiplicity is `[0..1]`.
    fn spell(rt: &RelationType<'_>) -> p::TypeExpression {
        let mut cols = Vec::with_capacity(rt.columns().len());
        for c in rt.columns() {
            let name = match c.r#type {
                Type::Primitive(prim) => JStr::from(prim.qualified_name()),
                Type::PrecisionDecimal(_) => JStr::from(Primitive::Decimal.qualified_name()),
                other => JStr::from(other.type_name()),
            };
            cols.push(Box::new(p::TypeExpressionColumn {
                name: JStr::from(c.name),
                r#type: name_ref(name),
                multiplicity: if c.multiplicity == Multiplicity::ONE {
                    pure_one()
                } else {
                    zero_one()
                },
                pos: None,
            }));
        }
        // Relation<(...)>: the RELATION generic around the schema, exactly as a user's return type spells it
        p::TypeExpression::TypeExpressionGeneric(Box::new(p::TypeExpressionGeneric {
            name: JStr::from(platform::RELATION),
            arguments: vec![p::TypeExpression::TypeExpressionRelationType(Box::new(
                p::TypeExpressionRelationType { columns: cols },
            ))],
            multiplicity_arguments: Vec::new(),
            type_variable_values: Vec::new(),
            pos: None,
        }))
    }

    /// `liftAll(wallSink)` (:145): E.5, eager like E.2-E.4 -- every view lifted, in declaration order; a wall recorded
    /// under the lifted FQN (its full message, first wall kept) when a sink exists, thrown under the strict entry.
    pub fn lift_all(&self, mut wall_sink: Option<&mut LinkedMap<JStr, JStr>>) -> Result<(), Thrown> {
        for v in &self.order {
            match self.body(v) {
                Ok(_) => {}
                Err(e) if e.is_model_or_not_implemented() => {
                    let Some(sink) = wall_sink.as_deref_mut() else {
                        return Err(e);
                    };
                    let owner = self
                        .owners
                        .get(v)
                        .ok_or_else(|| Thrown::null_pointer(Some("registered view")))?;
                    sink.put_if_absent(owner.fqn()?, e.message_or_null());
                }
                Err(e) => return Err(e),
            }
        }
        Ok(())
    }

    /// `functions()` (:161): the lifted functions, in declaration order (walled views absent).
    pub fn functions(&self) -> Vec<Arc<FunctionDefinition>> {
        let functions = self.functions.borrow();
        self.order.iter().filter_map(|v| functions.get(v).cloned()).collect()
    }
}

/// `new TypeExpression.NameRef(name)`.
fn name_ref(name: JStr) -> p::TypeExpression {
    p::TypeExpression::TypeExpressionNameRef(Box::new(p::TypeExpressionNameRef { name, pos: None }))
}

/// `Multiplicity.Concrete.PURE_ONE`.
fn pure_one() -> p::Multiplicity {
    p::Multiplicity::MultiplicityConcrete(Box::new(p::MultiplicityConcrete {
        lower_bound: 1,
        upper_bound: Some(1),
    }))
}

/// `Multiplicity.Concrete.ZERO_ONE`.
fn zero_one() -> p::Multiplicity {
    p::Multiplicity::MultiplicityConcrete(Box::new(p::MultiplicityConcrete {
        lower_bound: 0,
        upper_bound: Some(1),
    }))
}
