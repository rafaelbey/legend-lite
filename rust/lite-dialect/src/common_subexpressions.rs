// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! Java's `CommonSubexpressions` (`core/src/main/java/com/legend/sql/dialect/CommonSubexpressions.java`): a large
//! subexpression a projection or a filter repeats is written ONCE, for a database whose statements are capped (SQLite:
//! 1,000,000 bytes). An equal subtree is one node, and a repeated node becomes a column of a one-row derived table
//! under a scalar subquery: `(SELECT e' FROM (SELECT <node> AS v) AS __cseN)`, `N` counted by the pass object from 0
//! (a fresh pass for each render). SQLite installs it last of its passes ([`crate::sqlite`]); BigQuery does too.
//!
//! Only a node whose value cannot depend on where it is evaluated is moved: no aggregate or window, no subquery but a
//! bind, nothing that raises and no fresh value, and nothing naming a lambda's parameter (a lambda's body is bound
//! within itself first). A projection with an aggregate or window anywhere is left as it is, and so is an explosion.
//!
//! **Identity.** Java first makes every equal subtree one object (`canonical`, a pool by `equals`), then counts each
//! node's uses and replaces a node by identity -- so after `canonical` Java's identity is structural equality, and this
//! port keys every one of those maps by equality: the uses are counted over the expression's hash-consed DAG (a node
//! per distinct subtree, its edges its children), a replaced node is every subtree equal to it, and a lambda bound once
//! is bound once for every equal lambda. `canonical`'s own rebuilds (a parent of a re-pooled child rebuilt through
//! `withChildren`) recompute what the node already holds, so Rust keeps the tree as it came. Java's selects are
//! likewise rewritten once per content (`rewritten`, a `HashMap` by `equals`): an equal select met again is handed the
//! first one's rewrite, a node Java's `==` tells apart from its input.
//!
//! **Not exact (never reached by the oracles, which hold no `__cse`).** Among repeated nodes of the same size Java
//! binds the first its `IdentityHashMap` iterates -- identity-hash order, which differs from run to run of the JVM;
//! Rust binds the first met in a pre-order walk of the expression.

use crate::build;
use crate::carrier::map_children;
use crate::explosion_markers::{is_marker_select, is_unnest};
use crate::refusal::Refusal;
use crate::rewriter::{Rw, SqlRewriter};
use crate::typing::children;
use lite_sql::{Origin, Scalar, SqlExpr, SqlFn, SqlQuery, SqlSelect, SqlSource, SqlType, TypeFact};
use std::collections::{HashMap, HashSet};

/// The expanded size, in nodes, from which a repeated node is bound.
const MIN_SIZE: u64 = 40;

/// The calls written as a regular expression's walk: a statement of its own (SqliteRegex: two kilobytes).
const WALKS: &[SqlFn] =
    &[SqlFn::RegexpExtract, SqlFn::RegexpExtractAll, SqlFn::RegexpReplace, SqlFn::RegexpFullMatch, SqlFn::Matches];

/// The calls written as a subquery over a list's elements.
const SUBQUERIES: &[SqlFn] = &[
    SqlFn::ListFilter,
    SqlFn::ListTransform,
    SqlFn::ListSort,
    SqlFn::ListSortDesc,
    SqlFn::ListDistinct,
    SqlFn::Split,
    SqlFn::SplitPart,
    SqlFn::PureSplitPart,
];

/// The calls that raise (or answer a fresh value) when evaluated: never moved out of a branch.
const RAISING: &[SqlFn] = &[
    SqlFn::Error,
    SqlFn::ParseInt,
    SqlFn::ParseDate,
    SqlFn::Acos,
    SqlFn::Asin,
    SqlFn::AddInterval,
    SqlFn::AddIntervalTemporal,
    SqlFn::Strptime,
    SqlFn::Guid,
    SqlFn::Timezone,
    SqlFn::MakeDate,
    SqlFn::MakeTimestamp,
];

/// Java's `CommonSubexpressions`, one per render.
#[derive(Debug, Default)]
pub struct CommonSubexpressions {
    /// the fresh names' counter (`__cse<n>`)
    fresh: usize,
    /// the selects already rewritten, by content, bucketed by a cheap fingerprint
    rewritten: HashMap<u64, Vec<(SqlSelect, SqlQuery)>>,
}

impl CommonSubexpressions {
    pub fn new() -> CommonSubexpressions {
        CommonSubexpressions::default()
    }

    /// Java's `selectOnce`: the select with each projection's and its filter's repeated large nodes bound, or none
    /// when nothing was.
    fn select_once(&mut self, s: &SqlSelect) -> Result<Option<SqlSelect>, Refusal> {
        if is_marker_select(s) {
            return Ok(None);
        }
        let mut ps = Vec::with_capacity(s.projections.len());
        let mut changed = false;
        for p in &s.projections {
            let e = if is_unnest(&p.expr) || grouped(&p.expr) { None } else { self.shared(&p.expr)? };
            match e {
                Some(e) => {
                    changed = true;
                    ps.push(build::projection(e, p.alias.clone(), p.out.clone())?);
                }
                None => ps.push(p.clone()),
            }
        }
        let r#where = match &s.r#where {
            Some(w) if !grouped(w) => match self.shared(w)? {
                Some(bound) => {
                    changed = true;
                    Some(bound)
                }
                None => Some(w.clone()),
            },
            other => other.clone(),
        };
        if !changed {
            return Ok(None);
        }
        Ok(Some(build::select(SqlSelect { projections: ps, r#where, ..s.clone() })?))
    }

    /// Java's `shared`: `e` with its repeated large nodes bound; none when nothing was.
    fn shared(&mut self, e: &SqlExpr) -> Result<Option<SqlExpr>, Refusal> {
        self.bound(e)
    }

    /// Java's `bound`: `e` with its largest repeated movable node bound once, recursively; none when it has none.
    fn bound(&mut self, e0: &SqlExpr) -> Result<Option<SqlExpr>, Refusal> {
        let lambdas = self.lambdas_bound(e0, &mut Vec::new())?;
        let e = lambdas.as_ref().unwrap_or(e0);
        let mut dag = Dag::default();
        let root = dag.intern(e)?;
        let mut uses = vec![0usize; dag.nodes.len()];
        let mut order = Vec::new();
        let mut params = HashSet::new();
        dag.visit(root, &mut uses, &mut order, &mut params);
        let mut best: Option<usize> = None;
        let mut best_size = 0;
        for id in order {
            if uses[id] < 2 || id == root {
                continue;
            }
            let node = &dag.nodes[id];
            if node.size >= MIN_SIZE && node.size > best_size && node.movable && node.names.is_disjoint(&params) {
                best = Some(id);
                best_size = node.size;
            }
        }
        let Some(best) = best else {
            return Ok(lambdas);
        };
        let best = dag.nodes[best].expr.clone().expect("a node of the binding size keeps its expression");
        let alias = format!("__cse{}", self.fresh);
        self.fresh += 1;
        let reference = match best.r#type() {
            TypeFact::Typed { r#type, .. } => build::typed_column(Some(&alias), "v", r#type.clone(), true, Origin::Derived),
            _ => build::derived(Some(&alias), "v"),
        };
        let replaced = replace(e, &best, &reference)?.unwrap_or_else(|| e.clone());
        let outer = self.bound(&replaced)?.unwrap_or(replaced);
        let inner = self.bound(&best)?.unwrap_or(best);
        let inner_out = column(&inner);
        let bind = build::select_of(
            vec![build::projection(inner, Some("v".to_owned()), inner_out)?],
            SqlSource::Dual,
            Vec::new(),
        )?;
        let outer_out = column(&outer);
        let use_ = build::select_of(
            vec![build::projection(outer, Some("v".to_owned()), outer_out)?],
            build::subselect(build::query(bind), &alias, None),
            Vec::new(),
        )?;
        Ok(Some(build::scalar_subquery(build::query(use_))))
    }

    /// Java's `lambdasBound`: `e` with each lambda's body bound within itself; none when no body was. A lambda equal
    /// to one already bound takes that one's answer (Java's `done`, by identity over the pooled tree).
    fn lambdas_bound(
        &mut self,
        e: &SqlExpr,
        done: &mut Vec<(SqlExpr, Option<SqlExpr>)>,
    ) -> Result<Option<SqlExpr>, Refusal> {
        if !holds_lambda(e) {
            return Ok(None);
        }
        if let SqlExpr::Lambda(l) = e {
            if let Some((_, known)) = done.iter().find(|(k, _)| k == e) {
                return Ok(known.clone());
            }
            let out = self.bound(&l.body)?.map(|body| {
                SqlExpr::Lambda(lite_sql::Lambda {
                    params: l.params.clone(),
                    body: Box::new(body),
                    r#type: l.r#type.clone(),
                })
            });
            done.push((e.clone(), out.clone()));
            return Ok(out);
        }
        map_children(e, &mut |k| self.lambdas_bound(k, done))
    }
}

impl SqlRewriter for CommonSubexpressions {
    fn name(&self) -> &'static str {
        "CommonSubexpressions"
    }

    /// Java's `select`: an equal select met again is handed the first one's rewrite.
    fn select(&mut self, s: SqlSelect) -> Result<Rw<SqlQuery>, Refusal> {
        let key = fingerprint(&s);
        if let Some(bucket) = self.rewritten.get(&key)
            && let Some((_, known)) = bucket.iter().find(|(k, _)| *k == s)
        {
            return Ok(Rw::New(known.clone()));
        }
        let out = match self.select_once(&s)? {
            Some(rewritten) => Rw::New(build::query(rewritten)),
            None => Rw::Same(build::query(s.clone())),
        };
        self.rewritten.entry(key).or_default().push((s, out.get().clone()));
        Ok(out)
    }
}

/// A cheap fingerprint of a select's own shape, equal for equal selects: the bucket its content is looked up in.
fn fingerprint(s: &SqlSelect) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    s.projections.len().hash(&mut h);
    for p in &s.projections {
        p.alias.hash(&mut h);
        p.out.as_ref().map(|o| &o.name).hash(&mut h);
    }
    s.distinct.hash(&mut h);
    s.limit.hash(&mut h);
    s.offset.hash(&mut h);
    s.group_by.len().hash(&mut h);
    s.order_by.len().hash(&mut h);
    s.r#where.is_some().hash(&mut h);
    for o in &s.outputs {
        o.name.hash(&mut h);
    }
    h.finish()
}

/// Java's `column(e)`: `v`'s output, typed as `e` is, when it is typed.
fn column(e: &SqlExpr) -> Option<lite_sql::OutputCol> {
    match e.r#type() {
        TypeFact::Typed { r#type, .. } => Some(build::output_col("v", r#type.clone(), true)),
        _ => None,
    }
}

/// Whether a lambda is anywhere in `e` (a subtree with none is its own `lambdasBound`).
fn holds_lambda(e: &SqlExpr) -> bool {
    matches!(e, SqlExpr::Lambda(_)) || children(e).iter().any(|k| holds_lambda(k))
}

/// Java's `replace`: `e` with every subtree equal to `target` (Java's identity over the pooled tree) replaced by
/// `with`, a parent rebuilt through `withChildren`; none when nothing was.
fn replace(e: &SqlExpr, target: &SqlExpr, with: &SqlExpr) -> Result<Option<SqlExpr>, Refusal> {
    if e == target {
        return Ok(Some(with.clone()));
    }
    map_children(e, &mut |k| replace(k, target, with))
}

/// An aggregate or a window anywhere: the select's own grouping reads it (Java's `grouped`).
fn grouped(e: &SqlExpr) -> bool {
    matches!(e, SqlExpr::Reducer(_) | SqlExpr::WindowCall { .. }) || children(e).iter().any(|k| grouped(k))
}

/// The unqualified column names `e` reads (Java's `names`; a lambda parameter is one).
fn names(e: &SqlExpr, into: &mut HashSet<String>) {
    if let SqlExpr::Column(c) = e
        && c.table.is_none()
    {
        into.insert(c.name.clone());
    }
    for k in children(e) {
        names(&k, into);
    }
}

fn has_names(e: &SqlExpr) -> bool {
    let mut out = HashSet::new();
    names(e, &mut out);
    !out.is_empty()
}

/// A node's own weight as written (Java's `size`, less its children): a walk's or a subquery's weight, else one, and
/// a bind's expressions, which are no child of its node.
fn own_size(e: &SqlExpr) -> u64 {
    let mut n = match e {
        SqlExpr::Call { r#fn, .. } if WALKS.contains(r#fn) => 200,
        SqlExpr::Call { r#fn, .. } if SUBQUERIES.contains(r#fn) => 20,
        _ => 1,
    };
    if let Some((use_, bind)) = bind_shape(e) {
        for p in use_.projections.iter().chain(&bind.projections) {
            n = (u64::MAX / 4).min(n + size(&p.expr));
        }
    }
    n
}

/// Java's `size`, over a tree (a bind's expressions are not in the DAG).
fn size(e: &SqlExpr) -> u64 {
    let mut n = own_size(e);
    for k in children(e) {
        n = (u64::MAX / 4).min(n + size(&k));
    }
    n
}

/// `(SELECT e FROM (SELECT x AS v) AS a)`'s two selects, whatever their clauses: the shape `size` reads.
fn bind_shape(e: &SqlExpr) -> Option<(&SqlSelect, &SqlSelect)> {
    let SqlExpr::ScalarSubquery { subquery, .. } = e else {
        return None;
    };
    let SqlQuery::Select(use_) = subquery.as_ref() else {
        return None;
    };
    let SqlSource::Subselect { inner, .. } = &use_.from else {
        return None;
    };
    let SqlQuery::Select(bind) = inner.as_ref() else {
        return None;
    };
    Some((use_, bind))
}

/// Whether the node itself (its children aside) may be moved: anything but a raise, a fresh value, an aggregate or a
/// window, or a subquery but a bind (Java's `movable`'s switch).
fn own_movable(e: &SqlExpr) -> bool {
    match e {
        SqlExpr::Call { r#fn, .. } => !RAISING.contains(r#fn),
        SqlExpr::Cast { target, .. } => *target != SqlType::Scalar(Scalar::Json),
        SqlExpr::CheckedOne { .. }
        | SqlExpr::CheckedDefects { .. }
        | SqlExpr::CheckedChildValue { .. }
        | SqlExpr::WindowCall { .. }
        | SqlExpr::Exists { .. }
        | SqlExpr::InSubquery { .. }
        | SqlExpr::Quantified { .. }
        | SqlExpr::Star { .. }
        | SqlExpr::StarExcept { .. }
        | SqlExpr::DeferredTdsString { .. }
        | SqlExpr::TempTableInSplice { .. }
        | SqlExpr::Reducer(_) => false,
        SqlExpr::ScalarSubquery { .. } => is_bind(e),
        _ => true,
    }
}

/// Java's `movable`, over a tree.
fn movable(e: &SqlExpr) -> bool {
    own_movable(e) && children(e).iter().all(|k| movable(k))
}

/// A one-row bind (`(SELECT e FROM (SELECT x AS v) AS a)`) of movable expressions (Java's `isBind`).
fn is_bind(e: &SqlExpr) -> bool {
    let SqlExpr::ScalarSubquery { subquery, .. } = e else {
        return false;
    };
    let SqlQuery::Select(use_) = subquery.as_ref() else {
        return false;
    };
    if !plain(use_) || use_.projections.len() != 1 {
        return false;
    }
    let SqlSource::Subselect { inner, .. } = &use_.from else {
        return false;
    };
    let SqlQuery::Select(bind) = inner.as_ref() else {
        return false;
    };
    if !plain(bind) || !matches!(bind.from, SqlSource::Dual) {
        return false;
    }
    for p in &bind.projections {
        if !movable(&p.expr) || grouped(&p.expr) || has_names(&p.expr) {
            return false;
        }
    }
    let spelled = &use_.projections[0].expr;
    movable(spelled) && !grouped(spelled) && !has_names(spelled)
}

fn plain(s: &SqlSelect) -> bool {
    !s.distinct
        && s.r#where.is_none()
        && s.group_by.is_empty()
        && s.having.is_none()
        && s.qualify.is_none()
        && s.order_by.is_empty()
        && s.limit.is_none()
        && s.offset.is_none()
}

// ==================================================================
// The pooled tree: one node per distinct subtree
// ==================================================================

/// One distinct subtree: its children's nodes, and what the pass asks of it.
#[derive(Debug)]
struct Node {
    kids: Vec<usize>,
    size: u64,
    movable: bool,
    names: HashSet<String>,
    /// a lambda's parameters
    params: Vec<String>,
    /// the subtree, kept where it is large enough to be bound
    expr: Option<SqlExpr>,
}

/// An expression's hash-consed DAG (Java's tree after `canonical`, where equal subtrees are one object).
#[derive(Debug, Default)]
struct Dag {
    ids: HashMap<(String, Vec<usize>), usize>,
    nodes: Vec<Node>,
}

impl Dag {
    /// The node of `e`'s subtree, made on its first meeting.
    fn intern(&mut self, e: &SqlExpr) -> Result<usize, Refusal> {
        let mut kids = Vec::new();
        for k in children(e) {
            kids.push(self.intern(&k)?);
        }
        let key = (shallow(e), kids);
        if let Some(id) = self.ids.get(&key) {
            return Ok(*id);
        }
        let kids = key.1.clone();
        let mut size = own_size(e);
        let mut movable = own_movable(e);
        let mut names = HashSet::new();
        if let SqlExpr::Column(c) = e
            && c.table.is_none()
        {
            names.insert(c.name.clone());
        }
        for k in &kids {
            let kid = &self.nodes[*k];
            size = (u64::MAX / 4).min(size + kid.size);
            movable &= kid.movable;
            names.extend(kid.names.iter().cloned());
        }
        let params = match e {
            SqlExpr::Lambda(l) => l.params.clone(),
            _ => Vec::new(),
        };
        let expr = (size >= MIN_SIZE).then(|| e.clone());
        let id = self.nodes.len();
        self.nodes.push(Node { kids, size, movable, names, params, expr });
        self.ids.insert(key, id);
        Ok(id)
    }

    /// Java's `visit`: each node's uses counted, its children walked at its first; every lambda's parameters
    /// collected. `order` is the first meetings, in pre-order.
    fn visit(&self, id: usize, uses: &mut [usize], order: &mut Vec<usize>, params: &mut HashSet<String>) {
        uses[id] += 1;
        if uses[id] > 1 {
            return;
        }
        order.push(id);
        let node = &self.nodes[id];
        params.extend(node.params.iter().cloned());
        for k in &node.kids {
            self.visit(*k, uses, order, params);
        }
    }
}

/// A node's own fields, its children aside: with its children's nodes, its identity in the DAG (two nodes are equal
/// exactly when these and their children are).
fn shallow(e: &SqlExpr) -> String {
    use SqlExpr as E;
    match e {
        E::StoredRead { stored, r#type, .. } => format!("StoredRead {stored:?} {type:?}"),
        E::ReduceCollection { reducer, extras, r#type, .. } => {
            format!("ReduceCollection {reducer:?} {} {type:?}", extras.len())
        }
        E::Membership { r#type, .. } => format!("Membership {type:?}"),
        E::Group { r#type, .. } => format!("Group {type:?}"),
        E::ArrayLit { elements, r#type } => format!("ArrayLit {} {type:?}", elements.len()),
        E::OrderedListAgg { r#type, .. } => format!("OrderedListAgg {type:?}"),
        E::StructLit { fields, r#type } => {
            let own: Vec<(&String, &Option<SqlType>)> = fields.iter().map(|f| (&f.name, &f.declared)).collect();
            format!("StructLit {own:?} {type:?}")
        }
        E::StructGet { field, r#type, .. } => format!("StructGet {field:?} {type:?}"),
        E::Call { r#fn, args, r#type } => format!("Call {fn:?} {} {type:?}", args.len()),
        E::Case { whens, otherwise, r#type } => format!("Case {} {} {type:?}", whens.len(), otherwise.is_some()),
        E::InSubquery { subquery, r#type, .. } => format!("InSubquery {subquery:?} {type:?}"),
        E::CheckedDefects { hoists, r#type, .. } => {
            let own: Vec<(&Vec<String>, bool)> = hoists.iter().map(|h| (&h.path, h.to_many)).collect();
            format!("CheckedDefects {own:?} {type:?}")
        }
        E::CheckedChildValue { to_many, r#type, .. } => format!("CheckedChildValue {to_many} {type:?}"),
        E::Quantified { comparison, quantifier, subquery, r#type, .. } => {
            format!("Quantified {comparison:?} {quantifier:?} {subquery:?} {type:?}")
        }
        E::CheckedOne { scalar_carrier, at_least_only, r#type, .. } => {
            format!("CheckedOne {scalar_carrier} {at_least_only} {type:?}")
        }
        E::CompactList { r#type, .. } => format!("CompactList {type:?}"),
        E::WindowCall { r#fn, partition_by, order_by, frame, r#type } => {
            let f = match r#fn.as_ref() {
                lite_sql::SqlAgg::Reducer(_) => "Reducer".to_owned(),
                other => format!("{other:?}"),
            };
            let keys: Vec<(bool, &Option<lite_sql::NullOrder>, &Option<String>)> =
                order_by.iter().map(|k| (k.ascending, &k.null_order, &k.output_name)).collect();
            format!("WindowCall {f} {} {keys:?} {frame:?} {type:?}", partition_by.len())
        }
        E::Lambda(l) => format!("Lambda {:?} {:?}", l.params, l.r#type),
        E::Cast { target, conform, r#type, .. } => format!("Cast {target:?} {conform} {type:?}"),
        E::FoldCall { acc_is_list, homogeneous, r#type, .. } => {
            format!("FoldCall {acc_is_list} {homogeneous} {type:?}")
        }
        E::JsonObject { kv, r#type } => format!("JsonObject {} {type:?}", kv.len()),
        E::JsonArray { elements, r#type } => format!("JsonArray {} {type:?}", elements.len()),
        E::JsonArrayAgg { order_keys, r#type, .. } => {
            let own: Vec<bool> = order_keys.iter().map(|k| k.desc).collect();
            format!("JsonArrayAgg {own:?} {type:?}")
        }
        E::Reducer(r) => {
            let keys: Vec<(bool, &Option<lite_sql::NullOrder>, &Option<String>)> =
                r.order_by.iter().map(|k| (k.ascending, &k.null_order, &k.output_name)).collect();
            format!("Reducer {:?} {} {} {keys:?} {:?}", r.r#fn, r.args.len(), r.distinct, r.r#type)
        }
        // a leaf, or a node whose only children are queries: the whole node
        E::Column(_)
        | E::RowOrder { .. }
        | E::StarExcept { .. }
        | E::Star { .. }
        | E::StringLit { .. }
        | E::IntLit { .. }
        | E::FloatLit { .. }
        | E::DecimalLit { .. }
        | E::BoolLit { .. }
        | E::NullLit { .. }
        | E::DateLit { .. }
        | E::KeyCell { .. }
        | E::PlanParam { .. }
        | E::FormatLit { .. }
        | E::DatePartLit { .. }
        | E::JsonKindLit { .. }
        | E::TypeNameLit { .. }
        | E::TimestampLit { .. }
        | E::TimeLit { .. }
        | E::Exists { .. }
        | E::ScalarSubquery { .. }
        | E::DeferredTdsString { .. }
        | E::TempTableInSplice { .. } => format!("{e:?}"),
    }
}

#[cfg(test)]
mod tests;
