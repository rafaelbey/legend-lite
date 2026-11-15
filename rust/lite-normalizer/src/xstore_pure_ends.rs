// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! `XStorePureEnds` (XStorePureEnds.java at ad86fa895): an XStore / ModelJoin association's ENDS (a Relation set, a
//! Relational set's columns, or a Pure set), the property-space predicates and the `legacyLocalProperty` markers;
//! PARK-1's wall ("has direction-specific conditions").
//!
//! RE-EXPRESSED (census §4; ruling 30, ASK 8: PARK-1's rule is ONE Rust function): [`synthesize`] is
//! `XStorePureEnds.synthesize` and `MappingNormalizer.synthesizeXStoreMapping`'s column-space tail (MN:983-1059) as
//! one function, parameterized by [`XStoreReads`]: orientation, the canonical-form comparison and the four texts are
//! one; the reads each condition is rewritten by and the predicate's two end arguments are the parameter's. Not ported
//! (census §3): `XEnd.localTypes` and the `"<relational>"` sentinel of a Relational end's column view (written, never
//! read: the view's `func_ref` may be none).

use std::sync::Arc;

use lite_builtin::pure::lite;
use lite_error::Phase;
use lite_front::records as p;
use lite_model::linked::{LinkedMap, empty, list};
use lite_model::text::j;
use lite_model::{
    AssociationDefinition, AssociationMappingCross, ClassMapping, ClassMappingRelationFunction,
    ClassMappingRelationFunctionCol, FunctionDefinition, FunctionDefinitionParameterDefinition,
    FunctionDefinitionSynthesized, JStr, PropertyMapping, SynthHat, cat,
};
use lite_names::{ModelIndex, calls, synth_fqn};

use crate::error::{Thrown, require};
use crate::jdk::LinkedSet;
use crate::mapping_normalizer::{canonicalize_equal_operands, relation_function_pipeline};
use crate::relation_reads;
use crate::resolved_mapping::ResolvedMapping;
use crate::view_relation;

use p::ValueSpecification as V;

/// `XStorePureEnds.XEnd` (:61, its never-read `localTypes` dropped): an end's pipeline (none for a Pure end), its
/// column view (a Relation set, or a Relational set's columns as one), its set id, whether it is Pure, its local
/// properties, and whether the column view is lossy (a Relational PM it cannot carry).
#[derive(Clone, Debug)]
pub struct XEnd {
    pub pipeline: Option<p::ValueSpecification>,
    pub cols_view: Option<Arc<ClassMappingRelationFunction>>,
    pub set_id: JStr,
    pub pure: bool,
    pub local_props: LinkedSet<JStr>,
    pub lossy_view: bool,
}

/// The reads the one XStore predicate function rewrites each condition by (module note).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum XStoreReads {
    /// `XStorePureEnds.synthesize`'s: property space (`renameReads` over the ends, `this`/`that`), the predicate's ends
    /// by set id (`CString(end.setId)`). For an end that is Pure or a lossy view.
    PropertySpace,
    /// `MappingNormalizer.synthesizeXStoreMapping`'s (:983-1059): column space (`RelationReads.xstore` over each end's
    /// `cols_view`), the predicate's ends by pipeline.
    ColumnSpace,
}

/// `XStorePureEnds.rootEndId`: the id of `class_fqn`'s ROOT set when it is a set an end can be (a Relation, Relational
/// or Pure set), else none -- a ModelJoin's ends (since 2026-11-15, the normalizer's second batch, item 1, MN:1080 and
/// MJN:85-93).
pub fn root_end_id(md: &ResolvedMapping<'_>, class_fqn: &JStr) -> Option<JStr> {
    match md.roots().get(class_fqn) {
        Some(
            root @ (ClassMapping::ClassMappingRelationFunction(_)
            | ClassMapping::ClassMappingRelational(_)
            | ClassMapping::ClassMappingPure(_)),
        ) => Some(ResolvedMapping::id_of(root)),
        _ => None,
    }
}

/// `XStorePureEnds.xstoreEndOf` (:72): the end of `class_fqn` (`set_id` when given) in the closure -- a Relation set
/// first, then a Relational set, then a Pure set; loud for none.
pub fn xstore_end_of(
    md: &ResolvedMapping<'_>,
    class_fqn: &JStr,
    set_id: Option<&JStr>,
    model: &ModelIndex,
) -> Result<XEnd, Thrown> {
    // the end sets may live in INCLUDED mappings (modelJoins: XStore lines over include LegalEntityMapping /
    // TradesMapping) -- the engine compiles the include closure as one mapping
    let closure = md.closure();
    let cms: Vec<&ClassMapping> = closure.iter().flat_map(|m| m.class_mappings.iter()).collect();
    let selected = |cm: &ClassMapping| set_id.is_none_or(|s| *s == ResolvedMapping::id_of(cm));
    for &cm in &cms {
        if let ClassMapping::ClassMappingRelationFunction(rf) = cm
            && rf.class_name == *class_fqn
            && selected(cm)
        {
            let mut locals = LinkedSet::new();
            for c in rf.columns.iter() {
                if c.local {
                    locals.add(c.property.clone());
                }
            }
            return Ok(XEnd {
                pipeline: Some(relation_function_pipeline(rf, model)?),
                cols_view: Some(rf.clone()),
                set_id: ResolvedMapping::id_of(cm),
                pure: false,
                local_props: locals,
                lossy_view: false,
            });
        }
    }
    for &cm in &cms {
        if let ClassMapping::ClassMappingRelational(rcm) = cm
            && rcm.class_name == *class_fqn
            && selected(cm)
        {
            let mut cols: Vec<Arc<ClassMappingRelationFunctionCol>> = Vec::new();
            let mut locals = LinkedSet::new();
            // LOSSY: a property the column view cannot carry -- an expression-bound or join-chain +prop, a join-mapped
            // scalar, an embedded ctor. Such an end takes the property-space route, where the resolver substitutes the
            // set's REAL bindings (relationalModelJoins.pure; batch 110). An EXACT view keeps the column-space emission.
            let mut lossy = false;
            for pm in rcm.property_mappings.iter() {
                match pm {
                    PropertyMapping::PropertyMappingColumn(c) => cols.push(Arc::new(
                        ClassMappingRelationFunctionCol::new(c.property_name.clone(), c.column.clone(), false),
                    )),
                    PropertyMapping::PropertyMappingLocalProperty(lp) => {
                        if let PropertyMapping::PropertyMappingColumn(lc) = &lp.body {
                            cols.push(Arc::new(ClassMappingRelationFunctionCol::new(
                                lp.property_name.clone(),
                                lc.column.clone(),
                                true,
                            )));
                        } else {
                            lossy = true;
                        }
                        locals.add(lp.property_name.clone());
                    }
                    _ => lossy = true,
                }
            }
            let pipeline = view_relation::main_source_ref(md, class_fqn, model)?;
            let id = ResolvedMapping::id_of(cm);
            // Java's `new RelationFunction(classFqn, idOf(rcm), null, rcm.root(), "<relational>", cols)`, its sentinel
            // `~func` (never read) left out (module note)
            let view = ClassMappingRelationFunction {
                class_name: class_fqn.clone(),
                set_id: Some(id.clone()),
                extends_set_id: None,
                root: rcm.root,
                func_ref: None,
                inline_source: None,
                columns: list(cols),
                primary_key: empty(),
            };
            return Ok(XEnd {
                pipeline: Some(pipeline),
                cols_view: Some(Arc::new(view)),
                set_id: id,
                pure: false,
                local_props: locals,
                lossy_view: lossy,
            });
        }
    }
    for &cm in &cms {
        if let ClassMapping::ClassMappingPure(pcm) = cm
            && pcm.class_name == *class_fqn
            && selected(cm)
        {
            let mut locals = LinkedSet::new();
            for pb in pcm.property_bindings.iter() {
                if pb.local {
                    locals.add(pb.property_name.clone());
                }
            }
            return Ok(XEnd {
                pipeline: None,
                cols_view: None,
                set_id: ResolvedMapping::id_of(cm),
                pure: true,
                local_props: locals,
                lossy_view: false,
            });
        }
    }
    let for_set = match set_id {
        Some(s) => cat!(" for set id '", s, "'"),
        None => JStr::default(),
    };
    Err(Thrown::not_implemented(cat!(
        "XStore/ModelJoin association end class '",
        class_fqn,
        "' resolves to no Relation or Relational set",
        &for_set,
        " in '",
        md.qualified_name(),
        "'"
    )))
}

/// `XStorePureEnds.synthesize` (:176), merged with MN:983-1059 (module note): the XStore association's predicate
/// function `(a: A[1], b: B[1]): Boolean[1]`.
///
/// PARK-1 (docs/PARKED_WORK_LEDGER.md, "has direction-specific conditions"): the rule that every line of an XStore
/// association writes ONE shared predicate -- the lines' conditions, oriented and canonicalized
/// (`canonicalizeEqualOperands`), must agree, else the wall -- implemented here ONCE for both routes (ruling 30;
/// Java has it twice, MN:1015-1025 and XS:221-233, token for token). The emitted condition is the FIRST line's, in
/// its AUTHORED operand order (the engine spells `$this.entityIdFk == $that.entityId` verbatim); canonicalization
/// serves only the agreement check.
#[allow(clippy::too_many_arguments)]
pub fn synthesize(
    md: &ResolvedMapping<'_>,
    xs: &Arc<AssociationMappingCross>,
    ad: &AssociationDefinition,
    class_a: &JStr,
    class_b: &JStr,
    end_a: &XEnd,
    end_b: &XEnd,
    model: &ModelIndex,
    reads: XStoreReads,
) -> Result<FunctionDefinition, Thrown> {
    let name = &xs.association_name;
    // Java's column space checks for lines before its loop (MN:986), its property space after (XS:215): with no lines
    // the loop is empty and both throw this text, so one check serves both
    if xs.property_mappings2.is_empty() {
        return Err(Thrown::model(
            Phase::Normalize,
            cat!(
                "XStore mapping for '",
                name,
                "' has no property lines; mapping=",
                md.qualified_name()
            ),
            None,
        ));
    }
    let src_row = variable("srcRow");
    let tgt_row = variable("tgtRow");
    let self_assoc = class_a == class_b;
    // ORIENTATION (the resolver's associationJoin contract): for DISTINCT end classes the cond binds (srcRow =
    // classA-row, tgtRow = classB-row), so a property1 line's $that (the property1 destination = classA) maps to
    // srcRow. A SELF-association cannot orient by class -- the pinned convention is "property1's destination on
    // tgtRow" (the table-backed emission binds {target} there), the INVERSE mapping (audit 8 S1).
    let mut conds: Vec<p::ValueSpecification> = Vec::new();
    for cand in xs.property_mappings2.iter() {
        let is_prop1 = cand.property_name == ad.property1.property_name;
        if !is_prop1 && cand.property_name != ad.property2.property_name {
            return Err(Thrown::model(
                Phase::Normalize,
                cat!(
                    "XStore line '",
                    &cand.property_name,
                    "' matches neither end of association '",
                    name,
                    "'; mapping=",
                    md.qualified_name()
                ),
                None,
            ));
        }
        // Java's `thatRow == srcRow` (two fresh local variables): which row is `$that`'s
        let that_is_src = if self_assoc { !is_prop1 } else { is_prop1 };
        let (that_row, this_row) = if that_is_src {
            (&src_row, &tgt_row)
        } else {
            (&tgt_row, &src_row)
        };
        let (that_end, this_end) = if is_prop1 { (end_a, end_b) } else { (end_b, end_a) };
        conds.push(match reads {
            XStoreReads::ColumnSpace => relation_reads::xstore(
                &cand.expression,
                this_row,
                this_end.cols_view.as_ref(),
                that_row,
                that_end.cols_view.as_ref(),
                name,
                md,
                model,
            )?,
            XStoreReads::PropertySpace => rename_reads(
                &cand.expression,
                &LinkedMap::of([(j("this"), this_row.clone()), (j("that"), that_row.clone())]),
                &LinkedMap::of([(j("this"), this_end.clone()), (j("that"), that_end.clone())]),
            )?,
        });
    }
    // the directions must AGREE (audit S6); the first line's condition is emitted as authored
    let cond = conds[0].clone();
    let canon0 = canonicalize_equal_operands(&cond, &src_row.name).equality_key();
    for c in &conds {
        // Java's `.equals`: its record equality (lite-front's `equality_key`)
        if canonicalize_equal_operands(c, &src_row.name).equality_key() != canon0 {
            return Err(Thrown::not_implemented(cat!(
                "XStore association '",
                name,
                "' has direction-specific conditions; a single shared predicate is required for now (mapping=",
                md.qualified_name(),
                ")"
            )));
        }
    }
    let (arg_a, arg_b) = match reads {
        // the String-args overload: the two SETS pinned by id
        XStoreReads::PropertySpace => (cstring(end_a.set_id.clone()), cstring(end_b.set_id.clone())),
        // `List.of(.., endA.pipeline(), endB.pipeline(), ..)`: a null pipeline is its NullPointerException
        XStoreReads::ColumnSpace => (
            require(end_a.pipeline.clone(), None)?,
            require(end_b.pipeline.clone(), None)?,
        ),
    };
    predicate_function(md, name, class_a, class_b, arg_a, arg_b, src_row, tgt_row, cond)
}

/// The predicate function both association routes and the ModelJoin's end in (XS:234-260, MN:1037-1059, :1132-1148):
/// `legacyAssocPredicate(a, b, endA, endB, {srcRow, tgtRow | cond})` as `(a: A[1], b: B[1]): Boolean[1]`, named by
/// `SynthFqn.mappingAssoc` and tagged with its provenance.
#[allow(clippy::too_many_arguments)]
pub(crate) fn predicate_function(
    md: &ResolvedMapping<'_>,
    assoc_name: &JStr,
    class_a: &JStr,
    class_b: &JStr,
    end_arg_a: p::ValueSpecification,
    end_arg_b: p::ValueSpecification,
    src_row: Arc<p::Variable>,
    tgt_row: Arc<p::Variable>,
    cond: p::ValueSpecification,
) -> Result<FunctionDefinition, Thrown> {
    let a = V::Variable(variable("a"));
    let b = V::Variable(variable("b"));
    let predicate = V::LambdaFunction(Arc::new(p::LambdaFunction {
        parameters: list(vec![src_row, tgt_row]),
        body: list(vec![cond]),
        pos: None,
    }));
    let body = V::AppliedFunction(Arc::new(calls::exact(
        &j(lite::LEGACY_ASSOC_PREDICATE),
        vec![a, b, end_arg_a, end_arg_b, predicate],
    )));
    let p_a = parameter("a", class_a);
    let p_b = parameter("b", class_b);
    Ok(FunctionDefinition::user_written(
        synth_fqn::mapping_assoc(md.qualified_name(), assoc_name)?,
        empty(),
        empty(),
        list(vec![p_a, p_b]),
        name_ref(j("meta::pure::metamodel::type::Boolean")),
        pure_one(),
        list(vec![body]),
        empty(),
        empty(),
    )
    .with_synthesized_from(Arc::new(FunctionDefinitionSynthesized::new(
        SynthHat::ASSOC,
        md.qualified_name().clone(),
        assoc_name.clone(),
    ))))
}

/// `XStorePureEnds.renameReads` (:268): `this.p` / `that.p` as the rows' property reads (a local property through
/// `legacyLocalProperty`).
pub fn rename_reads(
    v: &p::ValueSpecification,
    row_by_var: &LinkedMap<JStr, Arc<p::Variable>>,
    end_by_var: &LinkedMap<JStr, XEnd>,
) -> Result<p::ValueSpecification, Thrown> {
    Ok(renamed(v, row_by_var, end_by_var)?.unwrap_or_else(|| v.clone()))
}

/// `renameReads`' recursion: `None` for Java's very object (`mapChildren`'s `this`), which only the default arm
/// answers; the explicit arms always build (a property and a lambda without their positions, a property without its
/// owner class, a collection without its position).
fn renamed(
    v: &p::ValueSpecification,
    row_by_var: &LinkedMap<JStr, Arc<p::Variable>>,
    end_by_var: &LinkedMap<JStr, XEnd>,
) -> Result<Option<p::ValueSpecification>, Thrown> {
    if let V::AppliedProperty(ap) = v
        && let V::Variable(var) = &ap.receiver
        && let Some(row) = row_by_var.get(&var.name)
    {
        let end = require(end_by_var.get(&var.name), None)?;
        if end.local_props.contains(&ap.property) {
            // NOTE (batch 110 probe): the marker is Any-typed, so an ORDERING comparison over a +prop on this route
            // does not type (lessThan(Any, Integer)); the typed-local read is an open leg of route A
            return Ok(Some(V::AppliedFunction(Arc::new(calls::exact(
                &j(lite::LEGACY_LOCAL_PROPERTY),
                vec![V::Variable(row.clone()), cstring(ap.property.clone())],
            )))));
        }
        return Ok(Some(property(V::Variable(row.clone()), ap.property.clone())));
    }
    let each = |xs: &[p::ValueSpecification]| -> Result<Vec<p::ValueSpecification>, Thrown> {
        xs.iter().map(|x| rename_reads(x, row_by_var, end_by_var)).collect()
    };
    Ok(Some(match v {
        V::Variable(var) if row_by_var.contains_key(&var.name) => {
            V::Variable(require(row_by_var.get(&var.name), None)?.clone())
        }
        V::AppliedFunction(af) => V::AppliedFunction(Arc::new(af.with_parameters(each(&af.parameters)?.into()))),
        V::AppliedProperty(ap2) => property(
            rename_reads(&ap2.receiver, row_by_var, end_by_var)?,
            ap2.property.clone(),
        ),
        V::PureCollection(pc) => V::PureCollection(Arc::new(p::PureCollection {
            values: each(&pc.values)?.into(),
            pos: None,
        })),
        V::LambdaFunction(lf) => V::LambdaFunction(Arc::new(p::LambdaFunction {
            parameters: lf.parameters.clone(),
            body: each(&lf.body)?.into(),
            pos: None,
        })),
        _ => return v.try_map_children(|x| renamed(x, row_by_var, end_by_var)),
    }))
}

/// `new Variable(name)`.
pub(crate) fn variable(name: &str) -> Arc<p::Variable> {
    Arc::new(p::Variable {
        name: j(name),
        r#type: None,
        multiplicity: None,
        pos: None,
    })
}

/// `new AppliedProperty(receiver, property)`: no position, no owner class.
pub(crate) fn property(receiver: p::ValueSpecification, name: JStr) -> p::ValueSpecification {
    V::AppliedProperty(Arc::new(p::AppliedProperty {
        receiver,
        property: name,
        pos: None,
        owner_class: None,
    }))
}

/// `new CString(value)`.
fn cstring(value: JStr) -> p::ValueSpecification {
    V::CString(Arc::new(p::CString {
        value,
        pos: None,
        multi_line: false,
    }))
}

/// `new TypeExpression.NameRef(name)`.
fn name_ref(name: JStr) -> p::TypeExpression {
    p::TypeExpression::TypeExpressionNameRef(Box::new(p::TypeExpressionNameRef { name, pos: None }))
}

/// `Multiplicity.Concrete.PURE_ONE`.
pub(crate) fn pure_one() -> p::Multiplicity {
    p::Multiplicity::MultiplicityConcrete(Box::new(p::MultiplicityConcrete {
        lower_bound: 1,
        upper_bound: Some(1),
    }))
}

/// `new FunctionDefinition.ParameterDefinition(name, new NameRef(class), PURE_ONE)`.
fn parameter(name: &str, class: &JStr) -> Arc<FunctionDefinitionParameterDefinition> {
    Arc::new(FunctionDefinitionParameterDefinition {
        name: j(name),
        r#type: name_ref(class.clone()),
        multiplicity: pure_one(),
    })
}

/// The unit tests' fixture for the XStore / ModelJoin files: a model parsed from text, one of its mappings resolved as
/// the pre-pass would hand it to the synthesis (no declared keys, nothing invalid), and a compact rendering of a
/// specification for assertions.
#[cfg(test)]
pub(crate) mod fixture {
    use std::collections::HashMap;
    use std::rc::Rc;
    use std::sync::Arc;

    use lite_front::records as p;
    use lite_model::linked::LinkedMap;
    use lite_model::{AssociationMapping, AssociationMappingCross, AssociationMappingModelJoin};
    use lite_names::ModelIndex;

    use crate::lifted_views::LiftedViews;
    use crate::mapping_closures::MappingClosures;
    use crate::resolved_mapping::ResolvedMapping;
    use crate::test_support::{index, mapping, model};

    use p::ValueSpecification as V;

    /// `f` over the mapping `fqn` of the model `text`, resolved.
    pub fn with_mapping<R>(text: &str, fqn: &str, f: impl FnOnce(&ResolvedMapping<'_>, &ModelIndex) -> R) -> R {
        let parsed = model(text);
        let idx = index(&parsed);
        let closures = MappingClosures::new(&idx);
        let views = LiftedViews::new(&parsed, &idx);
        let md = mapping(&parsed, fqn);
        let surface = closures.surface(&md);
        let closure = closures.closure(&surface.qualified_name);
        let resolved = ResolvedMapping::new(
            surface.clone(),
            surface,
            Rc::new(HashMap::new()),
            Rc::new(LinkedMap::new()),
            closure,
            &views,
        );
        f(&resolved, &idx)
    }

    /// The mapping's first XStore association mapping.
    pub fn cross(md: &ResolvedMapping<'_>) -> Arc<AssociationMappingCross> {
        md.association_mappings()
            .iter()
            .find_map(|am| match am {
                AssociationMapping::AssociationMappingCross(xs) => Some(xs.clone()),
                _ => None,
            })
            .expect("an XStore association mapping")
    }

    /// The mapping's ModelJoin association mapping of `name`.
    pub fn model_join(md: &ResolvedMapping<'_>, name: &str) -> Arc<AssociationMappingModelJoin> {
        md.association_mappings()
            .iter()
            .find_map(|am| match am {
                AssociationMapping::AssociationMappingModelJoin(mj) if mj.association_name == name => Some(mj.clone()),
                _ => None,
            })
            .expect("the ModelJoin association mapping")
    }

    /// A specification, compactly: `f(a, b)`, `$v.p`, `'s'`, `{x, y | body}`, `~slot{| body}`.
    pub fn show(v: &V) -> String {
        let all = |xs: &[V]| xs.iter().map(show).collect::<Vec<_>>().join(", ");
        match v {
            V::AppliedFunction(af) => format!("{}({})", af.function, all(&af.parameters)),
            V::AppliedProperty(ap) => format!("{}.{}", show(&ap.receiver), ap.property),
            V::Variable(x) => format!("${}", x.name),
            V::CString(s) => format!("'{}'", s.value),
            V::CInteger(i) => match &i.value {
                lite_front::jtypes::JValue::Integer(n) => n.to_string(),
                lite_front::jtypes::JValue::Long(n) => n.to_string(),
                other => format!("{other:?}"),
            },
            V::LambdaFunction(l) => format!(
                "{{{} | {}}}",
                l.parameters
                    .iter()
                    .map(|x| x.name.to_string())
                    .collect::<Vec<_>>()
                    .join(", "),
                all(&l.body)
            ),
            V::ColumnInstance(p::ColumnInstance::ColSpec(c)) => match &c.function1 {
                Some(f) => format!("~{}{{| {}}}", c.name, all(&f.body)),
                None => format!("~{}", c.name),
            },
            other => format!("{other:?}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use lite_error::Phase;
    use lite_model::linked::{empty, list};
    use lite_model::text::j;
    use lite_model::{AssociationMappingCross, ClassMappingRelationFunction, ClassMappingRelationFunctionCol};

    use super::fixture::{cross, model_join, show, with_mapping};
    use super::*;
    use crate::jdk::LinkedSet;
    use crate::mapping_normalizer::{synthesize_model_join_mapping, synthesize_xstore_mapping};

    /// Two Pure-mapped ends (property space needs no relation), `ASSOC` the association mapping.
    const PURE_ENDS: &str = r#"
Class t::Person { name: String[1]; firmId: Integer[1]; }
Class t::Firm { id: Integer[1]; name: String[1]; }
Class t::S { name: String[1]; id: Integer[1]; fid: Integer[1]; }
Association t::Firm_Person { firm: t::Firm[0..1]; employees: t::Person[*]; }
###Mapping
Mapping t::M (
  t::Person[p]: Pure { ~src t::S name: $src.name, firmId: $src.fid }
  t::Firm[f]: Pure { ~src t::S id: $src.id, name: $src.name, +code: Integer[1]: $src.fid }
  ASSOC
)
"#;

    fn text(assoc: &str) -> String {
        PURE_ENDS.replace("ASSOC", assoc)
    }

    fn xstore(lines: &str) -> String {
        text(&format!("t::Firm_Person: XStore {{ {lines} }}"))
    }

    /// The XStore association's predicate function, `t::Firm` (property1's target) end A.
    fn synthesized(lines: &str) -> Result<FunctionDefinition, Thrown> {
        with_mapping(&xstore(lines), "t::M", |md, idx| {
            synthesize_xstore_mapping(md, &cross(md), idx, &j("t::Firm"), &j("t::Person"))
        })
    }

    #[test]
    fn two_commuted_lines_agree_and_the_first_is_emitted_as_authored() {
        let f = synthesized("employees: $this.id == $that.firmId, firm: $this.firmId == $that.id").unwrap();
        assert_eq!(
            f.qualified_name,
            synth_fqn::mapping_assoc(&j("t::M"), &j("t::Firm_Person")).unwrap()
        );
        assert_eq!(f.parameters.len(), 2);
        assert_eq!(f.parameters[0].name, j("a"));
        assert_eq!(f.parameters[0].r#type, name_ref(j("t::Firm")));
        assert_eq!(f.parameters[1].r#type, name_ref(j("t::Person")));
        assert_eq!(f.return_type, name_ref(j("meta::pure::metamodel::type::Boolean")));
        assert_eq!(f.return_multiplicity, pure_one());
        let from = f.synthesized_from.as_ref().expect("its provenance");
        assert_eq!(
            (from.hat, &from.owner_fqn, &from.member_name),
            (SynthHat::ASSOC, &j("t::M"), &j("t::Firm_Person"))
        );
        assert_eq!(
            show(&f.body[0]),
            "meta::legend::lite::legacyAssocPredicate($a, $b, 'f', 'p', {srcRow, tgtRow | meta::pure::functions::boolean::equal($srcRow.id, $tgtRow.firmId)})"
        );
    }

    #[test]
    fn commuted_conjuncts_agree() {
        let f = synthesized(
            "employees: ($this.id == $that.firmId) && ($that.name == 'n'), \
             firm: ($this.name == 'n') && ($this.firmId == $that.id)",
        )
        .unwrap();
        assert_eq!(
            show(&f.body[0]),
            "meta::legend::lite::legacyAssocPredicate($a, $b, 'f', 'p', \
             {srcRow, tgtRow | and(meta::pure::functions::boolean::equal($srcRow.id, $tgtRow.firmId), meta::pure::functions::boolean::equal($tgtRow.name, 'n'))})"
        );
    }

    #[test]
    fn genuinely_different_lines_are_park_1s_wall() {
        let e = synthesized("employees: $this.id == $that.firmId, firm: $this.firmId == $that.name").unwrap_err();
        assert!(e.is_not_implemented());
        assert_eq!(
            e.message(),
            Some(j(
                "XStore association 't::Firm_Person' has direction-specific conditions; a single shared \
                    predicate is required for now (mapping=t::M)"
            ))
        );
    }

    #[test]
    fn the_agreement_is_javas_equality_not_the_derived_one() {
        with_mapping(
            &xstore("employees: $this.id == $that.firmId, firm: $this.firmId == $that.id"),
            "t::M",
            |md, idx| {
                let xs = cross(md);
                let firm = xstore_end_of(md, &j("t::Firm"), None, idx).unwrap();
                let person = xstore_end_of(md, &j("t::Person"), None, idx).unwrap();
                let (src, tgt) = (variable("srcRow"), variable("tgtRow"));
                // employees (property2): $this on srcRow; firm (property1): $that on srcRow
                let l0 = rename_reads(
                    &xs.property_mappings2[0].expression,
                    &LinkedMap::of([(j("this"), src.clone()), (j("that"), tgt.clone())]),
                    &LinkedMap::of([(j("this"), firm.clone()), (j("that"), person.clone())]),
                )
                .unwrap();
                let l1 = rename_reads(
                    &xs.property_mappings2[1].expression,
                    &LinkedMap::of([(j("this"), tgt), (j("that"), src)]),
                    &LinkedMap::of([(j("this"), person), (j("that"), firm)]),
                )
                .unwrap();
                let c0 = canonicalize_equal_operands(&l0, &j("srcRow"));
                let c1 = canonicalize_equal_operands(&l1, &j("srcRow"));
                assert_ne!(c0, c1, "the two lines' calls sit at two positions");
                assert!(c0.java_equals(&c1));
            },
        );
    }

    #[test]
    fn a_set_local_read_is_the_local_property_marker_and_is_rooted_at_its_row() {
        let f = synthesized("employees: $this.code == $that.firmId, firm: $this.firmId == $that.code").unwrap();
        assert_eq!(
            show(&f.body[0]),
            "meta::legend::lite::legacyAssocPredicate($a, $b, 'f', 'p', \
             {srcRow, tgtRow | meta::pure::functions::boolean::equal(meta::legend::lite::legacyLocalProperty($srcRow, 'code'), $tgtRow.firmId)})"
        );
    }

    #[test]
    fn a_line_of_neither_end_and_an_unknown_association_are_the_models_errors() {
        let e = synthesized("employees: $this.id == $that.firmId, boss: $this.firmId == $that.id").unwrap_err();
        assert!(e.is_model());
        assert_eq!(e.phase(), Some(Phase::Normalize));
        assert_eq!(
            e.message(),
            Some(j(
                "XStore line 'boss' matches neither end of association 't::Firm_Person'; mapping=t::M"
            ))
        );
        with_mapping(&xstore("employees: $this.id == $that.firmId"), "t::M", |md, idx| {
            let unknown = Arc::new(AssociationMappingCross {
                association_name: j("Firm_Person"),
                property_mappings2: empty(),
            });
            let e = synthesize_xstore_mapping(md, &unknown, idx, &j("t::Firm"), &j("t::Person")).unwrap_err();
            assert!(e.is_model());
            assert_eq!(e.element(), None);
            assert_eq!(
                e.message(),
                Some(j(
                    "XStore mapping references unknown association 'Firm_Person'; mapping=t::M"
                ))
            );
            // no lines: the one check, before any line is read
            let none = Arc::new(AssociationMappingCross {
                association_name: j("t::Firm_Person"),
                property_mappings2: empty(),
            });
            let e = synthesize_xstore_mapping(md, &none, idx, &j("t::Firm"), &j("t::Person")).unwrap_err();
            assert!(e.is_model());
            assert_eq!(
                e.message(),
                Some(j(
                    "XStore mapping for 't::Firm_Person' has no property lines; mapping=t::M"
                ))
            );
        });
    }

    #[test]
    fn an_end_resolves_by_class_and_set_id_or_is_refused() {
        with_mapping(&xstore("employees: $this.id == $that.firmId"), "t::M", |md, idx| {
            let firm = xstore_end_of(md, &j("t::Firm"), None, idx).unwrap();
            assert!(firm.pure && !firm.lossy_view && firm.pipeline.is_none() && firm.cols_view.is_none());
            assert_eq!(firm.set_id, j("f"));
            assert_eq!(firm.local_props.to_vec(), vec![j("code")]);
            assert_eq!(
                xstore_end_of(md, &j("t::Firm"), Some(&j("f")), idx).unwrap().set_id,
                j("f")
            );
            let e = xstore_end_of(md, &j("t::Firm"), Some(&j("p")), idx).unwrap_err();
            assert!(e.is_not_implemented());
            assert_eq!(
                e.message(),
                Some(j(
                    "XStore/ModelJoin association end class 't::Firm' resolves to no Relation or Relational set \
                        for set id 'p' in 't::M'"
                ))
            );
            let e = xstore_end_of(md, &j("t::S"), None, idx).unwrap_err();
            assert_eq!(
                e.message(),
                Some(j(
                    "XStore/ModelJoin association end class 't::S' resolves to no Relation or Relational set in \
                        't::M'"
                ))
            );
        });
    }

    #[test]
    fn a_model_join_over_a_pure_end_is_refused() {
        let mj = text("t::Firm_Person: ModelJoin { {f: t::Firm[1], p: t::Person[1]|$f.id == $p.firmId} }");
        with_mapping(&mj, "t::M", |md, idx| {
            let e = synthesize_model_join_mapping(
                md,
                &model_join(md, "t::Firm_Person"),
                idx,
                &j("t::Firm"),
                &j("t::Person"),
            )
            .unwrap_err();
            assert!(e.is_not_implemented());
            assert_eq!(
                e.message(),
                Some(j(
                    "ModelJoin association 't::Firm_Person' has a Pure-set end — the property-space route covers \
                        XStore only so far (mapping=t::M)"
                ))
            );
        });
    }

    fn col(property: &str, column: &str, local: bool) -> Arc<ClassMappingRelationFunctionCol> {
        Arc::new(ClassMappingRelationFunctionCol::new(j(property), j(column), local))
    }

    /// A column-space end of `class`: its pipeline a stand-in literal, its column view `cols`.
    fn column_end(class: &str, set_id: &str, cols: Vec<Arc<ClassMappingRelationFunctionCol>>) -> XEnd {
        XEnd {
            pipeline: Some(cstring(j(set_id))),
            cols_view: Some(Arc::new(ClassMappingRelationFunction {
                class_name: j(class),
                set_id: Some(j(set_id)),
                extends_set_id: None,
                root: false,
                func_ref: None,
                inline_source: None,
                columns: list(cols),
                primary_key: empty(),
            })),
            set_id: j(set_id),
            pure: false,
            local_props: LinkedSet::new(),
            lossy_view: false,
        }
    }

    #[test]
    fn the_column_space_route_reads_the_ends_columns_and_passes_their_pipelines() {
        with_mapping(
            &xstore("employees: $this.id == $that.firmId, firm: $this.firmId == $that.id"),
            "t::M",
            |md, idx| {
                let xs = cross(md);
                let ad = idx.find_association(&j("t::Firm_Person")).unwrap();
                let end_a = column_end("t::Firm", "fr", vec![col("id", "ID", false)]);
                let end_b = column_end("t::Person", "pr", vec![col("firmId", "FIRM_ID", false)]);
                let f = synthesize(
                    md,
                    &xs,
                    ad,
                    &j("t::Firm"),
                    &j("t::Person"),
                    &end_a,
                    &end_b,
                    idx,
                    XStoreReads::ColumnSpace,
                )
                .unwrap();
                // a [1]-declared property reads through trustOne (both are Integer[1])
                assert_eq!(
                    show(&f.body[0]),
                    "meta::legend::lite::legacyAssocPredicate($a, $b, 'fr', 'pr', {srcRow, tgtRow | \
                     meta::pure::functions::boolean::equal(meta::legend::lite::trustOne($srcRow.ID), meta::legend::lite::trustOne($tgtRow.FIRM_ID))})"
                );
                // an end with no pipeline is `List.of`'s NullPointerException
                let piped_off = XEnd {
                    pipeline: None,
                    ..end_b.clone()
                };
                let e = synthesize(
                    md,
                    &xs,
                    ad,
                    &j("t::Firm"),
                    &j("t::Person"),
                    &end_a,
                    &piped_off,
                    idx,
                    XStoreReads::ColumnSpace,
                )
                .unwrap_err();
                assert_eq!(e.java_class(), crate::error::NULL_POINTER);
                // a column the view lacks is the read's wall
                let bare = column_end("t::Person", "pr", vec![]);
                let e = synthesize(
                    md,
                    &xs,
                    ad,
                    &j("t::Firm"),
                    &j("t::Person"),
                    &end_a,
                    &bare,
                    idx,
                    XStoreReads::ColumnSpace,
                )
                .unwrap_err();
                assert!(e.is_not_implemented());
                assert_eq!(
                    e.message(),
                    Some(j(
                        "association 't::Firm_Person': $that.firmId has no column binding on the Relation mapping \
                            of 't::Person' (mapping=t::M)"
                    ))
                );
            },
        );
    }

    #[test]
    fn a_self_association_puts_property1s_destination_on_the_target_row() {
        let text = r#"
Class t::Node { id: Integer[1]; parentId: Integer[1]; }
Class t::S { id: Integer[1]; pid: Integer[1]; }
Association t::Tree { parent: t::Node[0..1]; children: t::Node[*]; }
###Mapping
Mapping t::M (
  t::Node[n]: Pure { ~src t::S id: $src.id, parentId: $src.pid }
  t::Tree: XStore { parent: $this.parentId == $that.id, children: $this.id == $that.parentId }
)
"#;
        with_mapping(text, "t::M", |md, idx| {
            let f = synthesize_xstore_mapping(md, &cross(md), idx, &j("t::Node"), &j("t::Node")).unwrap();
            // parent (property1): $that (the parent) on tgtRow, $this on srcRow
            assert_eq!(
                show(&f.body[0]),
                "meta::legend::lite::legacyAssocPredicate($a, $b, 'n', 'n', {srcRow, tgtRow | meta::pure::functions::boolean::equal($srcRow.parentId, $tgtRow.id)})"
            );
        });
    }

    #[test]
    fn rename_reads_rebuilds_its_explicit_arms_and_keeps_an_untouched_node() {
        let row = variable("srcRow");
        let rows = LinkedMap::of([(j("this"), row.clone())]);
        let ends = LinkedMap::of([(j("this"), column_end("t::Firm", "f", vec![]))]);
        let lit = cstring(j("x"));
        // a node none of whose children changes is the very node (a literal, an unbound variable)
        assert_eq!(rename_reads(&lit, &rows, &ends).unwrap(), lit);
        let other = V::Variable(variable("other"));
        assert_eq!(rename_reads(&other, &rows, &ends).unwrap(), other);
        // a bare bound variable is its row
        assert_eq!(
            rename_reads(&V::Variable(variable("this")), &rows, &ends).unwrap(),
            V::Variable(row.clone())
        );
        // a deep read renames its head
        let deep = property(property(V::Variable(variable("this")), j("firm")), j("name"));
        assert_eq!(show(&rename_reads(&deep, &rows, &ends).unwrap()), "$srcRow.firm.name");
        // a bound variable with no end is `requireNonNull`'s NullPointerException
        let read = property(V::Variable(variable("this")), j("id"));
        let e = rename_reads(&read, &rows, &LinkedMap::new()).unwrap_err();
        assert_eq!(e.java_class(), crate::error::NULL_POINTER);
    }
}
