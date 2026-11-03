// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! The MIR passes the H2 family runs, each its Java class ported (`core/src/main/java/com/legend/sql/dialect/
//! <Name>.java`), in the order [`h2_passes`] installs them (Java's `H2.passes()`, H2.java:64): `CarrierStrategies`
//! under H2's caps ([`crate::carrier`]), [`QualifyToSubselect`] where QUALIFY is not native (never on H2, whose
//! QUALIFY is), [`ConstantLateralsAsRows`], [`LateralExplodeToUnion`], [`AggregatesDeliverDouble`] over AVG, and
//! [`SourceSpelling`] last. [`crate::ansi::render_passes`] runs the Float expansions before them and `StoredReads`
//! after. `QualifyToSubselect` and `AggregatesDeliverDouble` are other dialects' passes too: they are
//! [`crate::shared_passes`]', re-exported here.
//!
//! Each pass rebuilds through [`crate::build`] (Java's `new ...` constructors), and an expression rebuilt by Java's
//! `mapChildren` through [`crate::carrier::with_children`]; each answers [`Rw::Same`] exactly where Java hands back its
//! input.

use crate::build::{self, call, derived, output_col, query, row_order};
use crate::carrier::{CarrierStrategies, map_children};
use crate::java::type_fact_text;
use crate::refusal::Refusal;
use crate::rewriter::{Rw, SqlRewriter, rewrite};
use crate::typing::{children, source_alias, source_outputs};
use lite_sql::{
    AggFn, JoinKind, OutputCol, Projection, Scalar, SqlExpr, SqlFn, SqlQuery, SqlSelect, SqlSource, SqlType,
    StructField, TypeFact,
};
use std::collections::HashMap;

pub use crate::shared_passes::{AggregatesDeliverDouble, QualifyToSubselect};

/// H2's `passes()`: the carrier strategies first, QUALIFY wrapped only where it is not native, a lateral over a
/// constant list as its rows before the per-row explosion, AVG delivered as DOUBLE, and -- last, so every source the
/// passes above introduced is in scope -- each reference spelled as its source declared it.
pub fn h2_passes(supports_qualify: bool) -> Vec<Box<dyn SqlRewriter>> {
    let mut ps: Vec<Box<dyn SqlRewriter>> = vec![Box::new(CarrierStrategies::new(crate::h2::CAPS))];
    if !supports_qualify {
        ps.push(Box::new(QualifyToSubselect::default()));
    }
    // a lateral over a constant list first: its rows need no correlation
    ps.push(Box::new(ConstantLateralsAsRows));
    ps.push(Box::new(LateralExplodeToUnion));
    ps.push(Box::new(AggregatesDeliverDouble::new(&[AggFn::Avg])));
    ps.push(Box::new(SourceSpelling::default()));
    ps
}

fn same(s: SqlSelect) -> Result<Rw<SqlQuery>, Refusal> {
    Ok(Rw::Same(query(s)))
}

/// `new SqlSelect(projections, false, from, where, List.of(), null, null, List.of(), null, null, outputs)`.
fn plain_select(
    projections: Vec<Projection>,
    from: SqlSource,
    r#where: Option<SqlExpr>,
    outputs: Vec<OutputCol>,
) -> Result<SqlSelect, Refusal> {
    build::select(SqlSelect {
        projections,
        distinct: false,
        from,
        r#where,
        group_by: Vec::new(),
        having: None,
        qualify: None,
        order_by: Vec::new(),
        limit: None,
        offset: None,
        outputs,
    })
}

/// A list expression under its carrier wrappers: a compaction's list, a cast's value.
fn unwrap(mut e: &SqlExpr) -> &SqlExpr {
    loop {
        match e {
            SqlExpr::CompactList { list, .. } => e = list,
            SqlExpr::Cast { value, .. } => e = value,
            _ => return e,
        }
    }
}

#[cfg(test)]
mod tests;

// ==================================================================
// ConstantLateralsAsRows
// ==================================================================

/// A lateral join exploding a constant list is no lateral at all (Java's `ConstantLateralsAsRows`): the list's rows,
/// one FROM-less select per element under the join's alias, joined ON TRUE (a LEFT join keeps its row where the list is
/// empty). A struct element is its fields, each a column (`elem__f`), and each read of a field in the select over the
/// join reads that column. A list that reads a row is left to [`LateralExplodeToUnion`]; a struct element read whole,
/// or elements of differing fields, are left as they are and refused where they are written.
#[derive(Debug, Default)]
pub struct ConstantLateralsAsRows;

/// One exploded constant list: its alias, its element's name, and the element's struct fields (empty for a scalar
/// element, read as the element's own column).
#[derive(Debug, Clone)]
struct Explosion {
    alias: String,
    element: String,
    fields: Vec<String>,
}

/// Java's `LinkedHashMap<String, Explosion>`: by alias, a repeated alias replaced in place.
#[derive(Debug, Default)]
struct Explosions(Vec<Explosion>);

impl Explosions {
    fn put(&mut self, x: Explosion) {
        match self.0.iter_mut().find(|have| have.alias == x.alias) {
            Some(slot) => *slot = x,
            None => self.0.push(x),
        }
    }

    fn get(&self, alias: &str) -> Option<&Explosion> {
        self.0.iter().find(|x| x.alias == alias)
    }

    fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// The explosion whose element `e` reads (`t.elem`), if any.
    fn element(&self, e: &SqlExpr) -> Option<&Explosion> {
        match e {
            SqlExpr::Column(c) => {
                let x = self.get(c.table.as_deref()?)?;
                (x.element == c.name).then_some(x)
            }
            _ => None,
        }
    }
}

impl SqlRewriter for ConstantLateralsAsRows {
    fn name(&self) -> &'static str {
        "ConstantLateralsAsRows"
    }

    fn select(&mut self, s: SqlSelect) -> Result<Rw<SqlQuery>, Refusal> {
        if !matches!(s.from, SqlSource::Join { .. }) {
            return same(s);
        }
        let mut explosions = Explosions::default();
        let from = rewrite_joins(&s.from, &mut explosions)?;
        if explosions.is_empty() {
            return same(s);
        }
        // every read of an exploded element must be a field read
        let mut roots: Vec<&SqlExpr> = s.projections.iter().map(|p| &p.expr).collect();
        roots.extend(s.r#where.iter());
        roots.extend(s.group_by.iter());
        roots.extend(s.having.iter());
        roots.extend(s.order_by.iter().map(|k| &k.expr));
        if !roots.into_iter().all(|r| reads_fields_only(r, &explosions)) {
            return same(s);
        }
        let mut ps = Vec::with_capacity(s.projections.len());
        for p in &s.projections {
            ps.push(match fields_as_columns(&p.expr, &explosions)? {
                None => p.clone(),
                Some(x) => build::projection(x, p.alias.clone(), p.out.clone())?,
            });
        }
        let mut ob = Vec::with_capacity(s.order_by.len());
        for k in &s.order_by {
            ob.push(match fields_as_columns(&k.expr, &explosions)? {
                None => k.clone(),
                Some(x) => build::sort_key(x, k.ascending, k.null_order, k.output_name.clone()),
            });
        }
        let mut g = Vec::with_capacity(s.group_by.len());
        for x in &s.group_by {
            g.push(fields_as_columns(x, &explosions)?.unwrap_or_else(|| x.clone()));
        }
        let r#where = s.r#where.as_ref().map(|w| fields_or_itself(w, &explosions)).transpose()?;
        let having = s.having.as_ref().map(|h| fields_or_itself(h, &explosions)).transpose()?;
        let out =
            SqlSelect { projections: ps, from: from.into_inner(), r#where, group_by: g, having, order_by: ob, ..s };
        Ok(Rw::New(query(build::select(out)?)))
    }
}

/// `src` with each lateral over a constant list as its rows, recorded in `out`; any other source as it is.
fn rewrite_joins(src: &SqlSource, out: &mut Explosions) -> Result<Rw<SqlSource>, Refusal> {
    let SqlSource::Join { left, right, kind, on, .. } = src else {
        return Ok(Rw::Same(src.clone()));
    };
    let new_left = rewrite_joins(left, out)?;
    if matches!(kind, JoinKind::LeftLateral | JoinKind::CrossLateral)
        && let SqlSource::Subselect { inner, alias, frame_name } = right.as_ref()
        && let Some(rows) = rows_of(inner, alias, frame_name, out)?
    {
        let joined = if *kind == JoinKind::LeftLateral { JoinKind::Left } else { JoinKind::Inner };
        return Ok(Rw::New(build::join(new_left.into_inner(), rows, joined, Some(build::bool_lit(true)))?));
    }
    if !new_left.is_new() {
        return Ok(Rw::Same(src.clone()));
    }
    Ok(Rw::New(build::join(new_left.into_inner(), right.as_ref().clone(), *kind, on.as_deref().cloned())?))
}

/// The lateral subselect's rows when it explodes a constant list -- `SELECT UNNEST(lst) AS elem FROM (SELECT <list> AS
/// lst)` or `SELECT UNNEST(<list>) AS elem`, or the rows the carrier strategies already wrote -- else none.
fn rows_of(
    inner: &SqlQuery,
    alias: &str,
    frame_name: &Option<String>,
    out: &mut Explosions,
) -> Result<Option<SqlSource>, Refusal> {
    // the list the carrier strategies already wrote as its rows: a union of FROM-less selects of one element each
    if let Some(row_selects) = constant_rows(inner) {
        let p0 = &row_selects[0].projections[0];
        let elements: Vec<SqlExpr> = row_selects.iter().map(|r| r.projections[0].expr.clone()).collect();
        return rows(alias, frame_name, p0, &elements, out);
    }
    let SqlQuery::Select(u) = inner else {
        return Ok(None);
    };
    if u.projections.len() != 1
        || u.r#where.is_some()
        || !u.group_by.is_empty()
        || u.having.is_some()
        || !u.order_by.is_empty()
    {
        return Ok(None);
    }
    let p = &u.projections[0];
    let SqlExpr::Call { r#fn: SqlFn::Unnest, args, .. } = unwrap(&p.expr) else {
        return Ok(None);
    };
    if args.len() != 1 {
        return Ok(None);
    }
    let mut list = unwrap(&args[0]);
    if let SqlExpr::Column(lc) = list
        && let SqlSource::Subselect { inner: from_inner, .. } = &u.from
        && let SqlQuery::Select(l) = from_inner.as_ref()
        && l.projections.len() == 1
        && l.projections[0].alias.as_deref() == Some(lc.name.as_str())
        && matches!(l.from, SqlSource::Dual)
    {
        list = unwrap(&l.projections[0].expr);
    } else if !matches!(u.from, SqlSource::Dual) {
        return Ok(None);
    }
    let elements: Vec<SqlExpr> = match list {
        SqlExpr::ArrayLit { elements, .. } => elements.clone(),
        SqlExpr::NullLit { .. } => Vec::new(),
        _ => return Ok(None),
    };
    rows(alias, frame_name, p, &elements, out)
}

/// The FROM-less selects of one constant column each that `q` is (a union of them, or one), or none.
fn constant_rows(q: &SqlQuery) -> Option<Vec<&SqlSelect>> {
    let branches: Vec<&SqlQuery> = match q {
        SqlQuery::Union(u) if u.all => u.branches.iter().collect(),
        other => vec![other],
    };
    let mut out = Vec::with_capacity(branches.len());
    for b in branches {
        let SqlQuery::Select(r) = b else {
            return None;
        };
        if !matches!(r.from, SqlSource::Dual)
            || r.projections.len() != 1
            || r.r#where.is_some()
            || !r.group_by.is_empty()
            || !r.order_by.is_empty()
            || matches!(r.projections[0].expr, SqlExpr::Call { r#fn: SqlFn::Unnest, .. })
            || reads_column(&r.projections[0].expr)
        {
            return None;
        }
        out.push(r.as_ref());
    }
    Some(out)
}

/// The lateral's rows for its constant `elements`, the projection `p` naming the element, or none where an element
/// has no form here.
fn rows(
    alias: &str,
    frame_name: &Option<String>,
    p: &Projection,
    elements: &[SqlExpr],
    out: &mut Explosions,
) -> Result<Option<SqlSource>, Refusal> {
    let element = p.alias.clone().unwrap_or_else(|| "elem".to_owned());
    if elements.iter().any(reads_column) {
        return Ok(None);
    }
    // the element's fields: each struct literal's, the same in each; a NULL list's from its declared slot
    let mut fields: Option<Vec<StructField>> = None;
    let structs = !elements.is_empty() && elements.iter().all(|e| matches!(e, SqlExpr::StructLit { .. }));
    if structs {
        let SqlExpr::StructLit { fields: first, .. } = &elements[0] else { unreachable!("checked above") };
        let mut fs = Vec::with_capacity(first.len());
        for f in first {
            let t = match f.value.r#type() {
                TypeFact::Typed { r#type, .. } => Some(r#type.clone()),
                _ => f.declared.clone(),
            };
            // a field of no type: no column to carry it
            let Some(t) = t else {
                return Ok(None);
            };
            fs.push(StructField { name: f.name.clone(), r#type: t });
        }
        for e in elements {
            let SqlExpr::StructLit { fields: efs, .. } = e else { unreachable!("checked above") };
            if !efs.iter().map(|f| &f.name).eq(fs.iter().map(|f| &f.name)) {
                return Ok(None);
            }
        }
        fields = Some(fs);
    } else if elements.is_empty()
        && let Some(SqlType::Struct { fields: declared }) = p.out.as_ref().map(|o| &o.r#type)
    {
        fields = Some(declared.clone());
    } else if elements.iter().any(|e| matches!(e, SqlExpr::StructLit { .. })) {
        return Ok(None);
    }
    let mut outs: Vec<OutputCol> = Vec::new();
    match &fields {
        Some(fs) => {
            for f in fs {
                outs.push(output_col(&format!("{element}__{}", f.name), f.r#type.clone(), true));
            }
        }
        None => {
            let Some(o) = &p.out else {
                return Ok(None);
            };
            outs.push(output_col(&element, o.r#type.clone(), true));
        }
    }
    let mut branches = Vec::with_capacity(elements.len().max(1));
    for e in elements {
        let mut ps = Vec::new();
        match (&fields, e) {
            (Some(_), SqlExpr::StructLit { fields: fs, .. }) => {
                for (i, f) in fs.iter().enumerate() {
                    let oc = &outs[i];
                    ps.push(build::projection(
                        typed(f.value.clone(), &oc.r#type),
                        Some(oc.name.clone()),
                        Some(oc.clone()),
                    )?);
                }
            }
            _ => {
                ps.push(build::projection(
                    typed(e.clone(), &outs[0].r#type),
                    Some(element.clone()),
                    Some(outs[0].clone()),
                )?);
            }
        }
        branches.push(query(plain_select(ps, SqlSource::Dual, None, Vec::new())?));
    }
    if branches.is_empty() {
        // the empty list: one row of NULLs that no row passes
        let mut ps = Vec::with_capacity(outs.len());
        for oc in &outs {
            ps.push(build::projection(
                build::cast(build::null_lit(), oc.r#type.clone()),
                Some(oc.name.clone()),
                Some(oc.clone()),
            )?);
        }
        branches.push(query(plain_select(ps, SqlSource::Dual, Some(build::bool_lit(false)), Vec::new())?));
    }
    let body = if branches.len() == 1 {
        branches.pop().expect("one branch")
    } else {
        SqlQuery::Union(build::union(branches, true, outs)?)
    };
    out.put(Explosion {
        alias: alias.to_owned(),
        element,
        fields: fields.map_or_else(Vec::new, |fs| fs.into_iter().map(|f| f.name).collect()),
    });
    Ok(Some(build::subselect(body, alias, frame_name.clone())))
}

/// A NULL element typed by its column (a UNION's first branch types the column).
fn typed(v: SqlExpr, t: &SqlType) -> SqlExpr {
    if matches!(v, SqlExpr::NullLit { .. }) { build::cast(v, t.clone()) } else { v }
}

/// Whether `e` reads a row: a column, a scalar subquery or EXISTS anywhere in it.
fn reads_column(e: &SqlExpr) -> bool {
    matches!(e, SqlExpr::Column(_) | SqlExpr::ScalarSubquery { .. } | SqlExpr::Exists { .. })
        || children(e).iter().any(|c| reads_column(c))
}

/// Whether every read of an exploded struct element under `e` reads one of its fields.
fn reads_fields_only(e: &SqlExpr, ex: &Explosions) -> bool {
    if let SqlExpr::StructGet { source, field, .. } = e
        && let Some(x) = ex.element(source)
        && !x.fields.is_empty()
    {
        return x.fields.contains(field);
    }
    if ex.element(e).is_some_and(|x| !x.fields.is_empty()) {
        return false;
    }
    children(e).iter().all(|c| reads_fields_only(c, ex))
}

/// `e` with each field read of an exploded struct element as that field's column (`None`: `e` itself).
fn fields_as_columns(e: &SqlExpr, ex: &Explosions) -> Result<Option<SqlExpr>, Refusal> {
    if let SqlExpr::StructGet { source, field, .. } = e
        && let Some(x) = ex.element(source)
        && !x.fields.is_empty()
    {
        return Ok(Some(derived(Some(&x.alias), &format!("{}__{field}", x.element))));
    }
    map_children(e, &mut |c| fields_as_columns(c, ex))
}

fn fields_or_itself(e: &SqlExpr, ex: &Explosions) -> Result<SqlExpr, Refusal> {
    Ok(fields_as_columns(e, ex)?.unwrap_or_else(|| e.clone()))
}

// ==================================================================
// LateralExplodeToUnion
// ==================================================================

/// A per-row literal collection exploded through a correlated lateral join has no spelling on H2 2.1 (no LATERAL);
/// the engine's own shape is the decorrelated union keyed by the base row's identity, joined once (Java's
/// `LateralExplodeToUnion`): `LEFT OUTER JOIN (SELECT a AS elem, base._ROWID_ AS __rid FROM <base> UNION ALL SELECT b,
/// base._ROWID_ FROM <base>) AS t1 ON t1.__rid = base._ROWID_`. A null-dropping singleton (`CASE WHEN e IS NULL THEN
/// [] ELSE [e] END`) becomes its branch's `WHERE e IS NOT NULL`. A list that is not literal is left alone.
#[derive(Debug, Default)]
pub struct LateralExplodeToUnion;

/// One literal element of an exploded list, and whether its branch drops a NULL.
struct Element {
    value: SqlExpr,
    not_null: bool,
}

impl SqlRewriter for LateralExplodeToUnion {
    fn name(&self) -> &'static str {
        "LateralExplodeToUnion"
    }

    fn select(&mut self, s: SqlSelect) -> Result<Rw<SqlQuery>, Refusal> {
        let SqlSource::Join { left, right, kind, .. } = &s.from else {
            return same(s);
        };
        if !matches!(kind, JoinKind::LeftLateral | JoinKind::CrossLateral) {
            return same(s);
        }
        let SqlSource::Subselect { inner, alias: sub_alias, frame_name } = right.as_ref() else {
            return same(s);
        };
        let SqlQuery::Select(u) = inner.as_ref() else {
            return same(s);
        };
        if u.projections.len() != 1 || u.r#where.is_some() || !u.group_by.is_empty() {
            return same(s);
        }
        let p = &u.projections[0];
        let SqlExpr::Call { r#fn: SqlFn::Unnest, args, .. } = unwrap(&p.expr) else {
            return same(s);
        };
        if args.len() != 1 {
            return same(s);
        }
        let mut list = unwrap(&args[0]);
        // the list may be a column of the lateral's own inner select
        if let SqlExpr::Column(lc) = list
            && let SqlSource::Subselect { inner: from_inner, .. } = &u.from
            && let SqlQuery::Select(l) = from_inner.as_ref()
            && l.projections.len() == 1
            && l.projections[0].alias.as_deref() == Some(lc.name.as_str())
        {
            list = unwrap(&l.projections[0].expr);
        }
        let mut elements = Vec::new();
        if !elements_of(list, &mut elements) || elements.is_empty() {
            return same(s);
        }
        let Some(base) = base_alias(&elements) else {
            return same(s);
        };
        let elem_alias = p.alias.clone().unwrap_or_else(|| "elem".to_owned());
        // the element's type is the list's element type where the tree states it
        let elem_out = match &p.out {
            Some(o) => o.clone(),
            None => output_col(&elem_alias, element_type_of(list)?, true),
        };
        let rid_out = output_col("__rid", SqlType::Scalar(Scalar::Bigint), false);
        let mut branches = Vec::with_capacity(elements.len());
        for e in &elements {
            let projections = vec![
                build::projection(e.value.clone(), Some(elem_alias.clone()), Some(elem_out.clone()))?,
                build::projection(row_order(Some(base.clone())), Some("__rid".to_owned()), Some(rid_out.clone()))?,
            ];
            let r#where = e.not_null.then(|| call(SqlFn::IsNotNull, vec![e.value.clone()]));
            let outputs = vec![elem_out.clone(), rid_out.clone()];
            branches.push(query(plain_select(projections, left.as_ref().clone(), r#where, outputs)?));
        }
        let body = if branches.len() == 1 {
            branches.pop().expect("one branch")
        } else {
            SqlQuery::Union(build::union(branches, true, vec![elem_out, rid_out])?)
        };
        let joined = if *kind == JoinKind::CrossLateral { JoinKind::Inner } else { JoinKind::Left };
        let on = call(SqlFn::Equal, vec![derived(Some(sub_alias), "__rid"), row_order(Some(base))]);
        let decorrelated = build::join(
            left.as_ref().clone(),
            build::subselect(body, sub_alias, frame_name.clone()),
            joined,
            Some(on),
        )?;
        Ok(Rw::New(query(build::with_from(s, decorrelated)?)))
    }
}

/// The literal elements of a list expression, or false when the list is not literal (a stored array).
fn elements_of(list: &SqlExpr, out: &mut Vec<Element>) -> bool {
    let e = unwrap(list);
    if let SqlExpr::ArrayLit { elements, .. } = e {
        out.extend(elements.iter().map(|x| Element { value: x.clone(), not_null: false }));
        return true;
    }
    if let SqlExpr::Call { r#fn: SqlFn::ListConcat, args, .. } = e {
        return args.iter().all(|a| elements_of(a, out));
    }
    // CASE WHEN x IS NULL THEN [] ELSE [x] END: the null-dropping singleton
    if let SqlExpr::Case { whens, otherwise: Some(otherwise), .. } = e
        && whens.len() == 1
        && let SqlExpr::Call { r#fn: SqlFn::IsNull, args, .. } = &whens[0].condition
        && args.len() == 1
        && let SqlExpr::ArrayLit { elements: empty, .. } = unwrap(&whens[0].then)
        && empty.is_empty()
        && let SqlExpr::ArrayLit { elements: one, .. } = unwrap(otherwise)
        && one.len() == 1
    {
        out.push(Element { value: one[0].clone(), not_null: true });
        return true;
    }
    false
}

/// The base alias the elements correlate to (the first table-qualified column read); none: no correlation to key on.
fn base_alias(elements: &[Element]) -> Option<String> {
    elements.iter().find_map(|e| first_table(&e.value))
}

fn first_table(e: &SqlExpr) -> Option<String> {
    match e {
        SqlExpr::Column(c) if c.table.is_some() => c.table.clone(),
        SqlExpr::Call { args, .. } => args.iter().find_map(first_table),
        SqlExpr::Cast { value, .. } => first_table(value),
        SqlExpr::Case { whens, otherwise, .. } => {
            for w in whens {
                if let Some(t) = first_table(&w.condition).or_else(|| first_table(&w.then)) {
                    return Some(t);
                }
            }
            otherwise.as_deref().and_then(first_table)
        }
        _ => None,
    }
}

/// The element type a list expression states (`ARRAY<T>`: T); a list stating none is refused by name.
fn element_type_of(list: &SqlExpr) -> Result<SqlType, Refusal> {
    if let TypeFact::Typed { r#type: SqlType::Array { element }, .. } = list.r#type() {
        return Ok(element.as_ref().clone());
    }
    Err(Refusal::capability(format!(
        "an exploded list states no element type ({}): this family's reader casts each element to the list's type, \
         and the list has none",
        type_fact_text(list.r#type())
    )))
}

// ==================================================================
// SourceSpelling
// ==================================================================

/// The definition owns the spelling (Java's `SourceSpelling`): on a case-sensitive session a column reference must
/// spell exactly as the source it reads declared the name, so each reference's origin is re-derived from its source --
/// one alias to its outputs over the whole statement (aliases are unique per statement; a repeated one, the last the
/// walk meets). A name the source does not declare keeps its stamp.
#[derive(Debug, Default)]
pub struct SourceSpelling {
    /// alias -> declared outputs, over the whole statement (Java's `HashMap`: only looked up)
    scope: HashMap<String, Vec<OutputCol>>,
}

/// The walk that collects every aliased source's outputs.
struct Scope<'a>(&'a mut HashMap<String, Vec<OutputCol>>);

impl SqlRewriter for Scope<'_> {
    fn name(&self) -> &'static str {
        "SqlRewriter"
    }

    fn source(&mut self, s: SqlSource) -> Result<Rw<SqlSource>, Refusal> {
        if !matches!(s, SqlSource::Join { .. } | SqlSource::Dual) {
            self.0.insert(source_alias(&s)?.to_owned(), source_outputs(&s).to_vec());
        }
        Ok(Rw::Same(s))
    }
}

impl SqlRewriter for SourceSpelling {
    fn name(&self) -> &'static str {
        "SourceSpelling"
    }

    fn rewrite_root(&mut self, q: SqlQuery) -> Result<SqlQuery, Refusal> {
        rewrite(&mut Scope(&mut self.scope), q.clone())?;
        Ok(rewrite(self, q)?.into_inner())
    }

    fn expr(&mut self, e: SqlExpr) -> Result<Rw<SqlExpr>, Refusal> {
        let SqlExpr::Column(c) = &e else {
            return Ok(Rw::Same(e));
        };
        let Some(table) = &c.table else {
            return Ok(Rw::Same(e));
        };
        let Some(outs) = self.scope.get(table) else {
            return Ok(Rw::Same(e));
        };
        match outs.iter().find(|oc| oc.name == c.name) {
            Some(oc) if c.origin != Some(oc.origin) => Ok(Rw::New(SqlExpr::Column(build::column_of(Some(table), oc)))),
            _ => Ok(Rw::Same(e)),
        }
    }
}
