// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! `ViewRelation` (ViewRelation.java at ad86fa895): a store view as a relation expression -- the lifted view
//! functions' bodies (`LiftedViews::body`), and view-backed sets' PMs re-pointed at the view's columns (the frame
//! rule); the main source of a class. RE-EXPRESSED (census §1): `viewRelationExpr`'s `md` parameter is dropped (its one
//! caller, LiftedViews:101, passes null). The H2-LENIENT `first()` witness (:209-224) is ASK 12's; `pmThroughFrame`'s
//! default passes `EnumeratedExpression` and `OtherwiseEmbedded` through unrewritten (ASK 13): port as Java has them.

use std::collections::HashSet;
use std::sync::Arc;

use lite_front::records as p;
use lite_model::{
    ClassMappingRelational, DatabaseDefinitionViewDefinition, JStr, LegacyMappingDefinitionTableReference as TableRef,
    PropertyMapping, RelationalOperation,
};
use lite_names::ModelIndex;
use lite_types::RelationType;

use crate::error::Thrown;
use crate::lifted_views::LiftedViews;
use crate::resolved_mapping::ResolvedMapping;
use crate::unported;

/// `ViewRelation.mainSourceRef` (:48): a class's main source (its main table, or the view's lifted body).
pub fn main_source_ref(
    md: &ResolvedMapping<'_>,
    class_fqn: &JStr,
    model: &ModelIndex,
) -> Result<p::ValueSpecification, Thrown> {
    Err(Thrown::Unported("ViewRelation.mainSourceRef"))
}

/// `ViewRelation.sourceRefFor` (:57): a table reference's source (a view's lifted body, else `tableReference`).
pub fn source_ref_for(
    r: &TableRef,
    model: &ModelIndex,
    md: &ResolvedMapping<'_>,
) -> Result<p::ValueSpecification, Thrown> {
    Err(Thrown::Unported("ViewRelation.sourceRefFor"))
}

/// `ViewRelation.owner` (:80): a wall's owner label (`mapping=<fqn>`, or `store=<db>` with no mapping).
pub fn owner(md: Option<&ResolvedMapping<'_>>, db: &JStr) -> JStr {
    // UNPORTED: ViewRelation.owner
    unported!("ViewRelation.owner")
}

/// `ViewRelation.viewRelationExpr` (:87, its always-null `md` dropped): the view's relation -- its main table (or
/// inner view) through its joins (a view `Pipeline::for_view`), filtered, grouped, distinct, projected to its
/// columns, each column conforming to `signature`.
pub fn view_relation_expr(
    view: &Arc<DatabaseDefinitionViewDefinition>,
    view_name: &JStr,
    db: &JStr,
    model: &ModelIndex,
    views: &LiftedViews<'_>,
    signature: &RelationType<'_>,
) -> Result<p::ValueSpecification, Thrown> {
    Err(Thrown::Unported("ViewRelation.viewRelationExpr"))
}

/// `ViewRelation.declaredOne` (:286): a column the view declares `[1]` (`ones`) wrapped to one.
pub fn declared_one(ones: &HashSet<JStr>, column: &JStr, value: p::ValueSpecification) -> p::ValueSpecification {
    // UNPORTED: ViewRelation.declaredOne
    unported!("ViewRelation.declaredOne")
}

/// `ViewRelation.inlineViewRefs` (:298): a view column's references to other views inlined.
pub fn inline_view_refs(
    op: &RelationalOperation,
    db: &JStr,
    model: &ModelIndex,
) -> Result<RelationalOperation, Thrown> {
    Err(Thrown::Unported("ViewRelation.inlineViewRefs"))
}

/// `ViewRelation.frameRewrite` (:324): an operation over a view-backed set's view re-pointed at the view's columns
/// (the frame rule); loud for a column the view does not project.
pub fn frame_rewrite(
    op: &RelationalOperation,
    view: &Arc<DatabaseDefinitionViewDefinition>,
    view_name: &JStr,
    md: &ResolvedMapping<'_>,
) -> Result<RelationalOperation, Thrown> {
    Err(Thrown::Unported("ViewRelation.frameRewrite"))
}

/// `ViewRelation.frameRewriteIfView` (:367): [`frame_rewrite`] when `table` is a view of `db`.
pub fn frame_rewrite_if_view(
    op: &RelationalOperation,
    db: &JStr,
    table: &JStr,
    md: &ResolvedMapping<'_>,
    model: &ModelIndex,
) -> Result<RelationalOperation, Thrown> {
    Err(Thrown::Unported("ViewRelation.frameRewriteIfView"))
}

/// `ViewRelation.declaredSpelling` (:377): a join condition's table refs in their declared spelling.
pub fn declared_spelling(
    op: &RelationalOperation,
    db: &JStr,
    model: &ModelIndex,
) -> Result<RelationalOperation, Thrown> {
    Err(Thrown::Unported("ViewRelation.declaredSpelling"))
}

/// `ViewRelation.throughFrame` (:400): a view-backed set with its PMs, filter and keys re-pointed through the frame.
pub fn through_frame(
    rcm: &ClassMappingRelational,
    view: &Arc<DatabaseDefinitionViewDefinition>,
    view_name: &JStr,
    md: &ResolvedMapping<'_>,
) -> Result<Arc<ClassMappingRelational>, Thrown> {
    Err(Thrown::Unported("ViewRelation.throughFrame"))
}

/// `ViewRelation.pmThroughFrame` (:419): one PM through the frame.
pub fn pm_through_frame(
    pm: &PropertyMapping,
    view: &Arc<DatabaseDefinitionViewDefinition>,
    view_name: &JStr,
    md: &ResolvedMapping<'_>,
) -> Result<PropertyMapping, Thrown> {
    Err(Thrown::Unported("ViewRelation.pmThroughFrame"))
}
