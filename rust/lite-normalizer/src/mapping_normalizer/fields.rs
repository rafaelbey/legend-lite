// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! MappingNormalizer family 14 (EF_CENSUS §1.1): each PM to a constructor field (`^C(p = ...)`), and the embedded,
//! otherwise-embedded and inline-embedded materializations. `pipeline.inline_stack`'s order is printed in a splice
//! cycle's error.
//!
//! Where Java passes `pipeline.ledger()` to a DeclaredCoercions method whose `ledger` the Rust dropped (census §4),
//! the argument is still evaluated, at its place: it is loud on a view pipeline (never one here: both callers hold a
//! mapping's pipeline).

use std::sync::Arc;

use lite_builtin::pure::lite;
use lite_error::Phase;
use lite_front::records as p;
use lite_front::records::ValueSpecification as VS;
use lite_model::{
    ClassMapping, ClassMappingRelational, JStr, PropertyMapping, PropertyMappingInlineEmbedded,
    PropertyMappingOtherwiseEmbedded, cat,
};
use lite_names::model_index::java_list;
use lite_names::{ModelIndex, calls};

use super::Fields;
use super::enumeration::{translate_enumerated_column, translate_enumerated_source};
use super::instances::build_new_instance_to_one;
use crate::declared_coercions;
use crate::error::Thrown;
use crate::join_chain_emission;
use crate::pipeline::Pipeline;
use crate::rel_op_translator::{self, TableScope};
use crate::resolved_mapping::ResolvedMapping;

/// `MappingNormalizer.CtorField` (:1999): one constructor field -- its name, its value, and whether it is a
/// mapping-local property.
#[derive(Clone, Debug)]
pub struct CtorField {
    pub name: JStr,
    pub value: p::ValueSpecification,
    pub is_local: bool,
}

/// `MappingNormalizer.translatePmToField` (:2001): one PM's constructor field over the row.
#[allow(clippy::too_many_arguments)]
pub fn translate_pm_to_field(
    pm: &PropertyMapping,
    row_bind: &Arc<p::Variable>,
    table_scope: &TableScope,
    default_table: &JStr,
    pipeline: &mut Pipeline<'_>,
    owner_class_fqn: &JStr,
    md: &ResolvedMapping<'_>,
    model: &ModelIndex,
    under_group_by: bool,
) -> Result<CtorField, Thrown> {
    // under ~groupBy every PM reads the post-groupBy row by its own property name
    if under_group_by {
        return Ok(field(
            pm.property_name(),
            row_property(row_bind, pm.property_name()),
            false,
        ));
    }
    Ok(match pm {
        PropertyMapping::PropertyMappingEnumeratedExpression(ee) => {
            let read = rel_op_translator::translate(&ee.expression, table_scope, None, Some(row_bind), pipeline.view())?;
            let value = translate_enumerated_source(
                &ee.property_name,
                ee.enum_mapping_id.as_ref(),
                &read,
                md,
                owner_class_fqn,
                model,
            )?;
            field(&ee.property_name, value, false)
        }
        PropertyMapping::PropertyMappingColumn(col) => {
            let read = rel_op_translator::column_read(&col.table, &col.column, table_scope, default_table, pipeline.view())?;
            pipeline.ledger()?;
            let value = declared_coercions::coerce_column_to_declared(&read, col, owner_class_fqn, model)?;
            field(&col.property_name, value, false)
        }
        PropertyMapping::PropertyMappingEnumeratedColumn(ec) => {
            let value =
                translate_enumerated_column(ec, table_scope, default_table, md, Some(&*pipeline), owner_class_fqn, model)?;
            field(&ec.property_name, value, false)
        }
        PropertyMapping::PropertyMappingExpression(expr) => {
            let read =
                rel_op_translator::translate(&expr.expression, table_scope, None, Some(row_bind), pipeline.view())?;
            let value =
                declared_coercions::coerce_to_declared_numeric(&read, &expr.property_name, Some(owner_class_fqn), model)?;
            field(&expr.property_name, value, false)
        }
        PropertyMapping::PropertyMappingJoin(j) => {
            let target_if_mapped = join_chain_emission::class_typed_target(Some(owner_class_fqn), &j.property_name, model)?;
            let slot = if target_if_mapped.is_some() {
                join_chain_emission::nav_slot_for(pipeline, &j.property_name)?
            } else {
                join_chain_emission::slot_for(pipeline, &j.joins)?
            };
            field(&j.property_name, row_property(row_bind, &slot), false)
        }
        PropertyMapping::PropertyMappingJoinTerminalColumn(jtc) => {
            let alias = join_chain_emission::slot_for(pipeline, &jtc.joins)?;
            let sub_row = row_property(row_bind, &alias);
            let mut scope = table_scope.clone();
            if let Some(terminal_table) = pipeline.alias_to_target_table.get(&alias) {
                scope.put(terminal_table.clone(), sub_row);
            }
            let read =
                rel_op_translator::translate(&jtc.terminal_column, &scope, None, Some(row_bind), pipeline.view())?;
            let value = if jtc.enum_mapped {
                translate_enumerated_source(
                    &jtc.property_name,
                    jtc.enum_mapping_id.as_ref(),
                    &read,
                    md,
                    owner_class_fqn,
                    model,
                )?
            } else {
                // engine parity: a binding read types as the DECLARED property with no SQL cast, wrapped only on a
                // genuine kind mismatch
                pipeline.ledger()?;
                declared_coercions::declared_assertion(&read, jtc, owner_class_fqn, model)?
            };
            field(&jtc.property_name, value, false)
        }
        PropertyMapping::PropertyMappingLocalProperty(lp) => {
            let inner = translate_pm_to_field(
                &lp.body,
                row_bind,
                table_scope,
                default_table,
                pipeline,
                owner_class_fqn,
                md,
                model,
                false,
            )?;
            field(&lp.property_name, inner.value, true)
        }
        PropertyMapping::PropertyMappingEmbedded(emb) => {
            let value = materialize_embedded(
                &emb.property_name,
                &emb.property_mappings,
                row_bind,
                table_scope,
                default_table,
                pipeline,
                owner_class_fqn,
                md,
                model,
                None,
            )?;
            field(&emb.property_name, value, false)
        }
        PropertyMapping::PropertyMappingInlineEmbedded(ie) => {
            let value = materialize_inline_embedded(
                ie,
                row_bind,
                table_scope,
                default_table,
                pipeline,
                owner_class_fqn,
                md,
                model,
            )?;
            field(&ie.property_name, value, false)
        }
        PropertyMapping::PropertyMappingOtherwiseEmbedded(oe) => {
            let value = materialize_otherwise_embedded(
                oe,
                row_bind,
                table_scope,
                default_table,
                pipeline,
                owner_class_fqn,
                md,
                model,
            )?;
            field(&oe.property_name, value, false)
        }
    })
}

/// `MappingNormalizer.materializeEmbedded` (:2100): an embedded block's `^C(...)` over the same row (`inner_override`
/// the inner class when the caller knows it).
#[allow(clippy::too_many_arguments)]
pub fn materialize_embedded(
    prop_name: &JStr,
    sub_pms: &[PropertyMapping],
    row_bind: &Arc<p::Variable>,
    table_scope: &TableScope,
    default_table: &JStr,
    pipeline: &mut Pipeline<'_>,
    owner_class_fqn: &JStr,
    md: &ResolvedMapping<'_>,
    model: &ModelIndex,
    inner_override: Option<&JStr>,
) -> Result<p::ValueSpecification, Thrown> {
    let knowledge = model.knowledge();
    let Some(owner) = knowledge.hierarchy_class(Some(owner_class_fqn)) else {
        return Err(Thrown::model(
            Phase::Normalize,
            cat!(
                "Embedded PM '",
                prop_name,
                "' on '",
                owner_class_fqn,
                "' but owner class unknown; mapping=",
                md.qualified_name()
            ),
            None,
        ));
    };
    let inner_fqn: Option<JStr> = match inner_override {
        Some(inner) => Some(inner.clone()),
        None => match knowledge.property_type(Some(owner), prop_name)? {
            Some(p::TypeExpression::TypeExpressionNameRef(nr)) => Some(nr.name.clone()),
            _ => None,
        },
    };
    let Some(inner_fqn) = inner_fqn else {
        return Err(Thrown::model(
            Phase::Normalize,
            cat!(
                "Embedded PM '",
                prop_name,
                "' on '",
                owner_class_fqn,
                "' has non-class property type; mapping=",
                md.qualified_name()
            ),
            None,
        ));
    };
    let mut fields = Fields::new();
    for sub in sub_pms {
        // a Join sub-PM reads the slot pass 1 hoisted into the TOP pipeline; an UNMAPPED target class has no
        // instance to bind: a wall
        if let PropertyMapping::PropertyMappingJoin(j) = sub
            && join_chain_emission::class_typed_target_if_mapped(
                Some(&inner_fqn),
                &j.property_name,
                model,
                pipeline.ledger()?,
            )?
            .is_none()
        {
            return Err(Thrown::not_implemented(cat!(
                "Embedded sub-PM '",
                &j.property_name,
                "' on '",
                prop_name,
                "' is a class-typed Join to an UNMAPPED target class \u{2014} no instance to bind. Mapping=",
                md.qualified_name()
            )));
        }
        let cf = translate_pm_to_field(
            sub,
            row_bind,
            table_scope,
            default_table,
            pipeline,
            &inner_fqn,
            md,
            model,
            false,
        )?;
        fields.put(
            cf.name,
            p::KeyExpression {
                value: cf.value,
                is_add: false,
                is_local: cf.is_local,
            },
        );
    }
    build_new_instance_to_one(Some(&inner_fqn), &fields, model)
}

/// `MappingNormalizer.materializeOtherwiseEmbedded` (:2150): an otherwise-embedded block (the embedded value, the
/// fallback set's navigation otherwise).
#[allow(clippy::too_many_arguments)]
pub fn materialize_otherwise_embedded(
    oe: &Arc<PropertyMappingOtherwiseEmbedded>,
    row_bind: &Arc<p::Variable>,
    table_scope: &TableScope,
    default_table: &JStr,
    pipeline: &mut Pipeline<'_>,
    owner_class_fqn: &JStr,
    md: &ResolvedMapping<'_>,
    model: &ModelIndex,
) -> Result<p::ValueSpecification, Thrown> {
    let partial = materialize_embedded(
        &oe.property_name,
        &oe.embedded,
        row_bind,
        table_scope,
        default_table,
        pipeline,
        owner_class_fqn,
        md,
        model,
        None,
    )?;
    let fallback = row_property(row_bind, &oe.property_name);
    Ok(VS::AppliedFunction(Arc::new(calls::exact(
        &JStr::from(lite::OTHERWISE),
        vec![partial, fallback],
    ))))
}

/// `MappingNormalizer.materializeInlineEmbedded` (:2164): an inline-embedded splice of another set of the closure;
/// loud for a set already on `pipeline.inline_stack` (the one cycle an embedded materialization can have).
#[allow(clippy::too_many_arguments)]
pub fn materialize_inline_embedded(
    ie: &Arc<PropertyMappingInlineEmbedded>,
    row_bind: &Arc<p::Variable>,
    table_scope: &TableScope,
    default_table: &JStr,
    pipeline: &mut Pipeline<'_>,
    owner_class_fqn: &JStr,
    md: &ResolvedMapping<'_>,
    model: &ModelIndex,
) -> Result<p::ValueSpecification, Thrown> {
    // the referenced set may live in an INCLUDED mapping; EXACTLY ONE match (the engine asserts it)
    let mut matches: Vec<Arc<ClassMappingRelational>> = Vec::new();
    for m in md.closure() {
        for cm in m.class_mappings.iter() {
            if let ClassMapping::ClassMappingRelational(rcm) = cm
                && ResolvedMapping::id_of(cm) == ie.set_id
            {
                matches.push(rcm.clone());
            }
        }
    }
    if matches.is_empty() {
        return Err(Thrown::model(
            Phase::Normalize,
            cat!(
                "InlineEmbedded PM '",
                &ie.property_name,
                "' references unknown setId '",
                &ie.set_id,
                "' in mapping=",
                md.qualified_name()
            ),
            None,
        ));
    }
    if matches.len() > 1 {
        let mut classes: Vec<JStr> = Vec::new();
        for r in &matches {
            if !classes.contains(&r.class_name) {
                classes.push(r.class_name.clone());
            }
        }
        return Err(Thrown::model(
            Phase::Normalize,
            cat!(
                "InlineEmbedded PM '",
                &ie.property_name,
                "' references setId '",
                &ie.set_id,
                "', which ",
                matches.len().to_string(),
                " class mappings of the include closure declare (",
                java_list(&classes),
                "); the engine requires exactly one match. Mapping=",
                md.qualified_name()
            ),
            None,
        ));
    }
    let referenced = &matches[0];
    // the referenced set's class must BE the declared property type or a subtype of it (engine
    // RelationalInstanceSetImplementationValidator); an unknown owner skips the check
    let knowledge = model.knowledge();
    let declared = match knowledge.hierarchy_class(Some(owner_class_fqn)) {
        None => None,
        Some(inline_owner) => knowledge.property_type(Some(inline_owner), &ie.property_name)?,
    };
    if let Some(p::TypeExpression::TypeExpressionNameRef(dn)) = &declared
        && !knowledge.is_subtype(&referenced.class_name, &dn.name)
    {
        return Err(Thrown::model(
            Phase::Normalize,
            cat!(
                "InlineEmbedded PM '",
                &ie.property_name,
                "' references set '",
                &ie.set_id,
                "' of class '",
                &referenced.class_name,
                "', which is not '",
                &dn.name,
                "' or a subclass of it; mapping=",
                md.qualified_name()
            ),
            None,
        ));
    }
    // THE cycle guard, keyed by SET ID on the pipeline (the one object every level shares)
    if !pipeline.inline_stack.add(ie.set_id.clone()) {
        return Err(Thrown::model(
            Phase::Normalize,
            cat!(
                "Cycle materializing Inline embedded set '",
                &ie.set_id,
                "' via '",
                &ie.property_name,
                "' on '",
                owner_class_fqn,
                "': the splice chain ",
                java_list(&pipeline.inline_stack.to_vec()),
                " returns to it; mapping=",
                md.qualified_name()
            ),
            None,
        ));
    }
    // Java's `finally`: the set leaves the stack on every path
    let spliced = materialize_embedded(
        &ie.property_name,
        &referenced.property_mappings,
        row_bind,
        table_scope,
        default_table,
        pipeline,
        owner_class_fqn,
        md,
        model,
        Some(&referenced.class_name),
    );
    pipeline.inline_stack.remove(&ie.set_id);
    spliced
}

/// `new CtorField(name, value, isLocal)`.
fn field(name: &JStr, value: VS, is_local: bool) -> CtorField {
    CtorField {
        name: name.clone(),
        value,
        is_local,
    }
}

/// `new AppliedProperty(row, name)`: position-free.
fn row_property(row_bind: &Arc<p::Variable>, name: &JStr) -> VS {
    VS::AppliedProperty(Arc::new(p::AppliedProperty {
        receiver: VS::Variable(row_bind.clone()),
        property: name.clone(),
        pos: None,
        owner_class: None,
    }))
}

#[cfg(test)]
mod tests {
    use std::collections::{HashMap, HashSet};
    use std::rc::Rc;

    use super::*;
    use crate::lifted_views::LiftedViews;
    use crate::mapping_closures::MappingClosures;
    use crate::mapping_ledger::MappingLedger;
    use crate::test_support::{column_pm, index, mapping, model};
    use lite_model::linked::LinkedMap;
    use lite_model::text::j;

    const MODEL: &str = r#"
###Pure
Class a::Addr
{
  street: String[1];
}
Class a::Other
{
  x: String[1];
}
Class a::P
{
  name: String[1];
  addr: a::Addr[1];
  other: a::Other[1];
}

###Relational
Database a::db
(
  Table T (ID INTEGER PRIMARY KEY, STREET VARCHAR(20))
)

###Mapping
Mapping a::Base
(
  a::Addr[dup]: Relational
  {
    ~mainTable [a::db]T
    street: [a::db]T.STREET
  }
)
Mapping a::M
(
  include a::Base
  a::Addr[addr]: Relational
  {
    ~mainTable [a::db]T
    street: [a::db]T.STREET
  }
  a::Addr[dup]: Relational
  {
    ~mainTable [a::db]T
    street: [a::db]T.STREET
  }
)
"#;

    fn row() -> Arc<p::Variable> {
        Arc::new(p::Variable {
            name: j("row"),
            r#type: None,
            multiplicity: None,
            pos: None,
        })
    }

    fn inline(prop: &str, set: &str) -> Arc<PropertyMappingInlineEmbedded> {
        Arc::new(PropertyMappingInlineEmbedded {
            property_name: j(prop),
            set_id: j(set),
        })
    }

    #[test]
    fn under_group_by_a_field_reads_its_own_name_and_an_inline_splice_guards_its_cycle() {
        let parsed = model(MODEL);
        let idx = index(&parsed);
        let closures = MappingClosures::new(&idx);
        let views = LiftedViews::new(&parsed, &idx);
        let m = closures.surface(&mapping(&parsed, "a::M"));
        let md = ResolvedMapping::new(
            m.clone(),
            m.clone(),
            Rc::new(HashMap::new()),
            Rc::new(LinkedMap::new()),
            closures.closure(&m.qualified_name),
            &views,
        );
        let mut ledger = MappingLedger::new(HashSet::new());
        let mut pl = Pipeline::new(VS::Variable(row()), &mut ledger, &views);
        let scope: TableScope = LinkedMap::new();
        let owner = j("a::P");
        let cf = translate_pm_to_field(
            &column_pm("name", "T", "NAME"),
            &row(),
            &scope,
            &j("T"),
            &mut pl,
            &owner,
            &md,
            &idx,
            true,
        )
        .unwrap();
        assert_eq!(cf.name, j("name"));
        assert_eq!(cf.value, row_property(&row(), &j("name")));
        assert!(!cf.is_local);
        let inl = |ie: &Arc<PropertyMappingInlineEmbedded>, pl: &mut Pipeline<'_>, owner: &JStr| {
            materialize_inline_embedded(ie, &row(), &scope, &j("T"), pl, owner, &md, &idx)
        };
        let e = inl(&inline("addr", "nope"), &mut pl, &owner).unwrap_err();
        assert_eq!(
            e.message(),
            Some(j("InlineEmbedded PM 'addr' references unknown setId 'nope' in mapping=a::M"))
        );
        let e = inl(&inline("addr", "dup"), &mut pl, &owner).unwrap_err();
        assert_eq!(
            e.message(),
            Some(j(
                "InlineEmbedded PM 'addr' references setId 'dup', which 2 class mappings of the include closure \
                 declare ([a::Addr]); the engine requires exactly one match. Mapping=a::M"
            ))
        );
        let e = inl(&inline("other", "addr"), &mut pl, &owner).unwrap_err();
        assert_eq!(
            e.message(),
            Some(j(
                "InlineEmbedded PM 'other' references set 'addr' of class 'a::Addr', which is not 'a::Other' or a \
                 subclass of it; mapping=a::M"
            ))
        );
        // the cycle: the set already being spliced
        pl.inline_stack.add(j("outer"));
        pl.inline_stack.add(j("addr"));
        let e = inl(&inline("addr", "addr"), &mut pl, &owner).unwrap_err();
        assert!(e.is_model());
        assert_eq!(
            e.message(),
            Some(j(
                "Cycle materializing Inline embedded set 'addr' via 'addr' on 'a::P': the splice chain [outer, addr] \
                 returns to it; mapping=a::M"
            ))
        );
        // the splice's body runs and leaves the stack as it found it, whatever it answers
        pl.inline_stack.remove(&j("addr"));
        let spliced = inl(&inline("addr", "addr"), &mut pl, &owner);
        assert!(spliced.is_err());
        assert_eq!(pl.inline_stack.to_vec(), vec![j("outer")]);
        // an embedded block of an unknown owner, and of a property with no class type
        let e = materialize_embedded(&j("addr"), &[], &row(), &scope, &j("T"), &mut pl, &j("a::Nope"), &md, &idx, None)
            .unwrap_err();
        assert_eq!(
            e.message(),
            Some(j("Embedded PM 'addr' on 'a::Nope' but owner class unknown; mapping=a::M"))
        );
        // (a primitive is a name too: only a property with no name type -- here none at all -- is loud)
        let e = materialize_embedded(&j("nosuch"), &[], &row(), &scope, &j("T"), &mut pl, &owner, &md, &idx, None)
            .unwrap_err();
        assert_eq!(
            e.message(),
            Some(j("Embedded PM 'nosuch' on 'a::P' has non-class property type; mapping=a::M"))
        );
        // an empty block: ^Addr() (no binding to wrap)
        let built = materialize_embedded(&j("addr"), &[], &row(), &scope, &j("T"), &mut pl, &owner, &md, &idx, None)
            .unwrap();
        let VS::AppliedFunction(af) = &built else { panic!() };
        let VS::NewInstance(ni) = &af.parameters[1] else { panic!() };
        assert_eq!(ni.class_name, j("a::Addr"));
    }
}
