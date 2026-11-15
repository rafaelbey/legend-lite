// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! `RelationReads` (RelationReads.java at ad86fa895): XStore / ModelJoin predicate lambdas' reads rewritten to the
//! ends' rows -- a Relation column inlined (`ClassMappingRelationFunctionCol::bind_src`, lite-model's), a derived
//! property beta-reduced to depth 16. `bindSrc` and `substVars` ignore lambda shadowing (as Java's do). Not ported
//! (census §3): the 6-argument `rewrite` (:49-56, no caller) and the `model == null` branches (both live callers pass
//! a model): `model` is not optional here.
//!
//! A [1]-DECLARED property reads through `trustOne`: the column view types [0..1] (physical nullability) and would
//! summon comparison guards the engine never spells (the multiplicity-masking trap, 4th sighting).

use std::sync::Arc;

use lite_builtin::pure::lite;
use lite_front::records as p;
use lite_model::linked::LinkedMap;
use lite_model::text::j;
use lite_model::{ClassMappingRelationFunction, ClassMappingRelationFunctionCol, JStr, cat};
use lite_names::{ModelIndex, calls};

use crate::error::{Thrown, require};
use crate::model_join_nesting::NestedCols;
use crate::resolved_mapping::ResolvedMapping;
use crate::xstore_pure_ends::{property, pure_one};

use p::ValueSpecification as V;

/// The class a list index past the end throws: `List.copyOf`'s lists index their array.
const ARRAY_INDEX_OUT_OF_BOUNDS: &str = "java.lang.ArrayIndexOutOfBoundsException";

/// How many derived-property inlines may enclose a rewrite (`derivedDepth < 16`): a self-referential derived property
/// runs out of depth and falls through to the loud wall.
const DERIVED_DEPTH: i32 = 16;

/// `RelationReads.xstore` (:36): an XStore line's condition over `this` and `that` as the rows' column reads.
#[allow(clippy::too_many_arguments)]
pub fn xstore(
    v: &p::ValueSpecification,
    this_row: &Arc<p::Variable>,
    this_rf: Option<&Arc<ClassMappingRelationFunction>>,
    that_row: &Arc<p::Variable>,
    that_rf: Option<&Arc<ClassMappingRelationFunction>>,
    assoc_name: &JStr,
    md: &ResolvedMapping<'_>,
    model: &ModelIndex,
) -> Result<p::ValueSpecification, Thrown> {
    let rows = LinkedMap::of([(j("this"), this_row.clone()), (j("that"), that_row.clone())]);
    // `Map.of("this", thisRf, "that", thatRf)`: a null value is its NullPointerException
    let rfs = LinkedMap::of([
        (j("this"), require(this_rf.cloned(), None)?),
        (j("that"), require(that_rf.cloned(), None)?),
    ]);
    rewrite(v, &rows, &rfs, assoc_name, md, Some(&NestedCols::new()), model)
}

/// `RelationReads.rewrite` (:58, the seven-argument form): [`rewrite_at`] from depth 0.
pub fn rewrite(
    v: &p::ValueSpecification,
    row_by_var: &LinkedMap<JStr, Arc<p::Variable>>,
    rf_by_var: &LinkedMap<JStr, Arc<ClassMappingRelationFunction>>,
    assoc_name: &JStr,
    md: &ResolvedMapping<'_>,
    nested_cols: Option<&NestedCols>,
    model: &ModelIndex,
) -> Result<p::ValueSpecification, Thrown> {
    rewrite_at(v, row_by_var, rf_by_var, assoc_name, md, nested_cols, model, 0)
}

/// `RelationReads.rewrite` (:71, the eight-argument form): the reads rewritten, a derived property inlined up to
/// `derived_depth` 16.
#[allow(clippy::too_many_arguments)]
pub fn rewrite_at(
    v: &p::ValueSpecification,
    row_by_var: &LinkedMap<JStr, Arc<p::Variable>>,
    rf_by_var: &LinkedMap<JStr, Arc<ClassMappingRelationFunction>>,
    assoc_name: &JStr,
    md: &ResolvedMapping<'_>,
    nested_cols: Option<&NestedCols>,
    model: &ModelIndex,
    derived_depth: i32,
) -> Result<p::ValueSpecification, Thrown> {
    let r = Reads {
        row_by_var,
        rf_by_var,
        assoc_name,
        md,
        nested_cols,
        model,
    };
    Ok(r.rewritten(v, derived_depth)?.unwrap_or_else(|| v.clone()))
}

/// The rewrite's fixed arguments.
struct Reads<'r, 'a> {
    row_by_var: &'r LinkedMap<JStr, Arc<p::Variable>>,
    rf_by_var: &'r LinkedMap<JStr, Arc<ClassMappingRelationFunction>>,
    assoc_name: &'r JStr,
    md: &'r ResolvedMapping<'a>,
    nested_cols: Option<&'r NestedCols>,
    model: &'r ModelIndex,
}

impl Reads<'_, '_> {
    fn rewrite(&self, v: &p::ValueSpecification, depth: i32) -> Result<p::ValueSpecification, Thrown> {
        Ok(self.rewritten(v, depth)?.unwrap_or_else(|| v.clone()))
    }

    /// The recursion: `None` for Java's very object (`mapChildren`'s `this`), which only the default arm answers; the
    /// explicit arms always build (a property without its position and owner class, a collection and a lambda
    /// without their positions).
    fn rewritten(&self, v: &p::ValueSpecification, depth: i32) -> Result<Option<p::ValueSpecification>, Thrown> {
        // NESTED hop read: $end.assocProp.leaf resolves to the nested target's column on the END's composite row
        if let V::AppliedProperty(ap0) = v
            && let V::AppliedProperty(mid0) = &ap0.receiver
            && let V::Variable(var0) = &mid0.receiver
            && let Some(nested_cols) = self.nested_cols
            && nested_cols
                .get(&var0.name)
                .is_some_and(|m| m.contains_key(&mid0.property))
        {
            let leaf_cols = require(require(nested_cols.get(&var0.name), None)?.get(&mid0.property), None)?;
            let Some(col) = leaf_cols.get(&ap0.property) else {
                return Err(Thrown::not_implemented(cat!(
                    "association '",
                    self.assoc_name,
                    "': $",
                    &var0.name,
                    ".",
                    &mid0.property,
                    ".",
                    &ap0.property,
                    " has no column binding on the nested Relation mapping (mapping=",
                    self.md.qualified_name(),
                    ")"
                )));
            };
            // the SLOT-READ spelling ($row.<navSlot>.<COL>) -- typed and demanded by the stock navigate-step machinery
            let row = require(self.row_by_var.get(&var0.name), None)?;
            return Ok(Some(property(
                property(V::Variable(row.clone()), mid0.property.clone()),
                col.clone(),
            )));
        }
        if let V::AppliedProperty(ap) = v
            && let V::Variable(var) = &ap.receiver
            && let Some(row) = self.row_by_var.get(&var.name)
        {
            return self.read(ap, var, row, depth).map(Some);
        }
        let each = |xs: &[p::ValueSpecification]| -> Result<Vec<p::ValueSpecification>, Thrown> {
            xs.iter().map(|x| self.rewrite(x, depth)).collect()
        };
        Ok(Some(match v {
            V::AppliedFunction(af) => V::AppliedFunction(Arc::new(af.with_parameters(each(&af.parameters)?.into()))),
            V::AppliedProperty(ap2) => property(self.rewrite(&ap2.receiver, depth)?, ap2.property.clone()),
            V::PureCollection(pc) => V::PureCollection(Arc::new(p::PureCollection {
                values: each(&pc.values)?.into(),
                pos: None,
            })),
            V::LambdaFunction(lf2) => V::LambdaFunction(Arc::new(p::LambdaFunction {
                parameters: lf2.parameters.clone(),
                body: each(&lf2.body)?.into(),
                pos: None,
            })),
            _ => return v.try_map_children(|x| self.rewritten(x, depth)),
        }))
    }

    /// `$var.prop` on an end row (:102-159): the end's column, else its expression binding, else a derived property
    /// inlined, else the wall.
    fn read(
        &self,
        ap: &p::AppliedProperty,
        var: &Arc<p::Variable>,
        row: &Arc<p::Variable>,
        depth: i32,
    ) -> Result<p::ValueSpecification, Thrown> {
        let rf = require(self.rf_by_var.get(&var.name), None)?;
        let knowledge = self.model.knowledge();
        for c in rf.columns.iter() {
            if c.property == ap.property
                && let Some(column) = &c.column
            {
                let mut read = property(V::Variable(row.clone()), column.clone());
                // conform-by-emission: a [1]-DECLARED property reads through trustOne (module note)
                let rcd = knowledge.hierarchy_class(Some(&rf.class_name)).ok_or_else(|| {
                    Thrown::illegal_state(cat!(
                        "F7.8: class unresolved at RelationReads#1 (this default NEVER fired on the corpus census; a \
                         miss here is a real model gap): ",
                        &rf.class_name
                    ))
                })?;
                if knowledge.property_multiplicity(rcd, &ap.property)? == Some(pure_one()) {
                    read = V::AppliedFunction(Arc::new(calls::exact(&j(lite::TRUST_ONE), vec![read])));
                }
                return Ok(read);
            }
        }
        // an EXPRESSION-bodied binding (prop: $src.COL + 1 / 'lit') inlines with $src bound to the row -- the same
        // Col.bindSrc step the normalizer's projection path runs; the plain-column branch above stays first (a
        // $src.COL spelling carries BOTH fields and the column read is the proven path)
        for c in rf.columns.iter() {
            if c.property == ap.property
                && let Some(expr) = &c.expr
            {
                return Ok(ClassMappingRelationFunctionCol::bind_src(
                    expr,
                    &V::Variable(row.clone()),
                ));
            }
        }
        // DERIVED (qualified) property read in a join condition: the engine inlines the qualifier body into the
        // condition -- beta-inline the zero-arg inline body with $this bound to the receiver, then rewrite the result
        // so its leaf reads take the plain-column arm above. Depth-guarded (module note).
        if depth < DERIVED_DEPTH {
            let dcd = knowledge.hierarchy_class(Some(&rf.class_name)).ok_or_else(|| {
                Thrown::illegal_state(cat!(
                    "F7.8: class unresolved at RelationReads#2 (this default NEVER fired on the corpus census; a miss \
                     here is a real model gap): ",
                    &rf.class_name
                ))
            })?;
            if let Some(dp) = knowledge.derived_inline(Some(dcd.as_ref()), &ap.property)? {
                // `dp.expression()`: an inline body, else Java's IllegalStateException
                let body = match &dp.realization {
                    p::Realization::RealizationInline(inl) => &inl.body,
                    p::Realization::RealizationRef(_) => {
                        return Err(Thrown::illegal_state(cat!(
                            "derived property '",
                            &dp.name,
                            "' is a function-ref binding, not an inline body"
                        )));
                    }
                };
                let binds = LinkedMap::of([(j("this"), V::Variable(var.clone()))]);
                return self.rewrite(&subst_vars(statement(body, 0)?, &binds), depth + 1);
            }
        }
        Err(Thrown::not_implemented(cat!(
            "association '",
            self.assoc_name,
            "': $",
            &var.name,
            ".",
            &ap.property,
            " has no column binding on the Relation mapping of '",
            &rf.class_name,
            "' (mapping=",
            self.md.qualified_name(),
            ")"
        )))
    }
}

/// `RelationReads.substVars` (:184): the variables `binds` names substituted, a lambda rebinding a name shadowing it in
/// its body (since 2026-11-15, the normalizer's second batch, item 1, RR:184: it had no shadowing rule).
pub fn subst_vars(v: &p::ValueSpecification, binds: &LinkedMap<JStr, p::ValueSpecification>) -> p::ValueSpecification {
    substituted(v, binds).unwrap_or_else(|| v.clone())
}

/// `substVars`' recursion: `None` for Java's very object (an unbound variable, or a node none of whose children
/// changed).
fn substituted(
    v: &p::ValueSpecification,
    binds: &LinkedMap<JStr, p::ValueSpecification>,
) -> Option<p::ValueSpecification> {
    if let V::Variable(vv) = v {
        return binds.get(&vv.name).cloned();
    }
    if let V::LambdaFunction(lf) = v
        && lf.parameters.iter().any(|p| binds.contains_key(&p.name))
    {
        let mut inner: LinkedMap<JStr, p::ValueSpecification> = LinkedMap::new();
        for (k, x) in binds.iter() {
            if !lf.parameters.iter().any(|p| &p.name == k) {
                inner.put(k.clone(), x.clone());
            }
        }
        if inner.is_empty() {
            return None;
        }
        return v.map_children(|x| substituted(x, &inner));
    }
    v.map_children(|x| substituted(x, binds))
}

/// `list.get(i)` on a list `List.copyOf` made: past either end, the `ArrayIndexOutOfBoundsException` of its array.
pub(crate) fn statement(list: &[p::ValueSpecification], i: i64) -> Result<&p::ValueSpecification, Thrown> {
    usize::try_from(i).ok().and_then(|u| list.get(u)).ok_or_else(|| {
        Thrown::java(
            ARRAY_INDEX_OUT_OF_BOUNDS,
            Some(cat!(
                "Index ",
                i.to_string(),
                " out of bounds for length ",
                list.len().to_string()
            )),
        )
    })
}

#[cfg(test)]
mod tests {
    use lite_model::linked::{empty, list};

    use super::*;
    use crate::xstore_pure_ends::fixture::{show, with_mapping};
    use crate::xstore_pure_ends::variable;

    const MODEL: &str = r#"
Class t::Firm { id: Integer[1]; name: String[0..1]; code() { $this.id }: Integer[1]; loop() { $this.loop }: Integer[1]; }
Class t::S { id: Integer[1]; }
###Mapping
Mapping t::M (
  t::Firm[f]: Pure { ~src t::S id: $src.id }
)
"#;

    fn col(property: &str, column: Option<&str>, expr: Option<V>) -> Arc<ClassMappingRelationFunctionCol> {
        Arc::new(ClassMappingRelationFunctionCol {
            property: j(property),
            column: column.map(j),
            local: false,
            enum_mapping_id: None,
            embedded: empty(),
            inline_set_id: None,
            expr,
        })
    }

    fn view(class: &str, cols: Vec<Arc<ClassMappingRelationFunctionCol>>) -> Arc<ClassMappingRelationFunction> {
        Arc::new(ClassMappingRelationFunction {
            class_name: j(class),
            set_id: Some(j("f")),
            extends_set_id: None,
            root: false,
            func_ref: None,
            inline_source: None,
            columns: list(cols),
            primary_key: empty(),
        })
    }

    fn read(var: &str, prop: &str) -> V {
        property(V::Variable(variable(var)), j(prop))
    }

    /// `v` rewritten over `$this` as `srcRow` on the Firm view `cols`, the nested reads `nested`.
    fn rewritten(v: &V, cols: Vec<Arc<ClassMappingRelationFunctionCol>>, nested: &NestedCols) -> Result<V, Thrown> {
        with_mapping(MODEL, "t::M", |md, idx| {
            let rows = LinkedMap::of([(j("this"), variable("srcRow"))]);
            let rfs = LinkedMap::of([(j("this"), view("t::Firm", cols))]);
            rewrite(v, &rows, &rfs, &j("A"), md, Some(nested), idx)
        })
    }

    #[test]
    fn a_column_reads_through_trust_one_only_when_declared_to_one() {
        let cols = || vec![col("id", Some("ID"), None), col("name", Some("NAME"), None)];
        let none = NestedCols::new();
        assert_eq!(
            show(&rewritten(&read("this", "id"), cols(), &none).unwrap()),
            "meta::legend::lite::trustOne($srcRow.ID)"
        );
        assert_eq!(
            show(&rewritten(&read("this", "name"), cols(), &none).unwrap()),
            "$srcRow.NAME"
        );
        // a variable no end binds is read as written
        assert_eq!(
            show(&rewritten(&read("other", "id"), cols(), &none).unwrap()),
            "$other.id"
        );
    }

    #[test]
    fn an_expression_binding_inlines_with_src_bound_to_the_row() {
        let label = col("label", None, Some(read("src", "NAME")));
        assert_eq!(
            show(&rewritten(&read("this", "label"), vec![label], &NestedCols::new()).unwrap()),
            "$srcRow.NAME"
        );
    }

    #[test]
    fn a_derived_property_inlines_and_a_self_referential_one_runs_out_of_depth() {
        let cols = || vec![col("id", Some("ID"), None)];
        let none = NestedCols::new();
        assert_eq!(
            show(&rewritten(&read("this", "code"), cols(), &none).unwrap()),
            "meta::legend::lite::trustOne($srcRow.ID)"
        );
        let e = rewritten(&read("this", "loop"), cols(), &none).unwrap_err();
        assert!(e.is_not_implemented());
        assert_eq!(
            e.message(),
            Some(j(
                "association 'A': $this.loop has no column binding on the Relation mapping of 't::Firm' (mapping=t::M)"
            ))
        );
    }

    #[test]
    fn a_nested_hop_reads_the_slot_column_or_is_refused() {
        let mut nested = NestedCols::new();
        nested
            .get_or_insert_with(j("this"), LinkedMap::new)
            .put(j("boss"), LinkedMap::of([(j("name"), j("NAME"))]));
        let hop = |leaf: &str| property(read("this", "boss"), j(leaf));
        assert_eq!(
            show(&rewritten(&hop("name"), vec![], &nested).unwrap()),
            "$srcRow.boss.NAME"
        );
        let e = rewritten(&hop("age"), vec![], &nested).unwrap_err();
        assert!(e.is_not_implemented());
        assert_eq!(
            e.message(),
            Some(j(
                "association 'A': $this.boss.age has no column binding on the nested Relation mapping (mapping=t::M)"
            ))
        );
    }

    #[test]
    fn an_unknown_class_is_the_internal_error() {
        with_mapping(MODEL, "t::M", |md, idx| {
            let rows = LinkedMap::of([(j("this"), variable("srcRow"))]);
            let rfs = LinkedMap::of([(j("this"), view("t::Nope", vec![col("id", Some("ID"), None)]))]);
            let e = rewrite(&read("this", "id"), &rows, &rfs, &j("A"), md, None, idx).unwrap_err();
            assert_eq!(e.java_class(), crate::error::ILLEGAL_STATE);
            assert_eq!(
                e.message(),
                Some(j(
                    "F7.8: class unresolved at RelationReads#1 (this default NEVER fired on the corpus census; a miss \
                        here is a real model gap): t::Nope"
                ))
            );
            // xstore's `Map.of` over a missing view
            let row = variable("r");
            let e = xstore(&read("this", "id"), &row, None, &row, None, &j("A"), md, idx).unwrap_err();
            assert_eq!(e.java_class(), crate::error::NULL_POINTER);
        });
    }

    #[test]
    fn subst_vars_replaces_the_bound_names_only() {
        let binds = LinkedMap::of([(j("this"), V::Variable(variable("that")))]);
        assert_eq!(show(&subst_vars(&read("this", "a"), &binds)), "$that.a");
        let other = read("x", "a");
        assert_eq!(subst_vars(&other, &binds), other);
        // a lambda rebinding `this` shadows it in its body (the normalizer's second batch, item 1, RR:184, 2026-11-15)
        let shadowing = V::LambdaFunction(Arc::new(p::LambdaFunction {
            parameters: Arc::from(vec![variable("this")]),
            body: Arc::from(vec![read("this", "a")]),
            pos: None,
        }));
        assert_eq!(subst_vars(&shadowing, &binds), shadowing);
        let free = V::LambdaFunction(Arc::new(p::LambdaFunction {
            parameters: Arc::from(vec![variable("y")]),
            body: Arc::from(vec![read("this", "a")]),
            pos: None,
        }));
        assert_eq!(
            show(&subst_vars(&free, &binds)),
            show(&V::LambdaFunction(Arc::new(p::LambdaFunction {
                parameters: Arc::from(vec![variable("y")]),
                body: Arc::from(vec![read("that", "a")]),
                pos: None,
            })))
        );
    }

    #[test]
    fn a_statement_past_the_end_is_the_arrays_index_error() {
        let e = statement(&[], -1).unwrap_err();
        assert_eq!(e.java_class(), ARRAY_INDEX_OUT_OF_BOUNDS);
        assert_eq!(e.message(), Some(j("Index -1 out of bounds for length 0")));
    }
}
