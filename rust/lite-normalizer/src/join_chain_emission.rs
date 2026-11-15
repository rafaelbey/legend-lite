// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! `JoinChainEmission` (JoinChainEmission.java at ad86fa895): the join hops of a relational set's pipeline -- the
//! `joinSlot`, `legacyNavigate` and `route(...)` steps and their slot names (`exchange_nav_name` etc. reach SQL).
//! EF_CENSUS §1.2's families: JCE-1 pass-1 structural hoisting (:45-225), JCE-2 chain emission (:235-597, :822-911,
//! :1063-1078), JCE-3 slot naming and lookup (:470-477, :603-698), JCE-4 class-typed target predicates (:700-738),
//! JCE-5 JoinNav collectors (:740-817), JCE-6 the INNER mapping-filter source and `nullTolerant` (:921-1051).
//!
//! RE-EXPRESSED (census §4): JCE:884's `catch (ModelException unmappedTarget)` is
//! `mapping_normalizer::main_table_or_null(..)?.is_none()` (the throw's one cause; its message is discarded); the
//! "first Relational set with this id" lookups (:132-139, :758-771) are `ResolvedMapping::first_relational`. The
//! dead null checks after `orElseThrow` are not ported. `p.unionRoutes.remove` (:307) mutates the frame mid-emission:
//! `LinkedMap::remove`. `equalsIgnoreCase` is `jdk::equals_ignore_case` (Java's fold). `nullTolerant`'s
//! `default -> false` (:1049) is ASK 12's: port as Java has it.

use std::sync::Arc;

use lite_builtin::dyna_fn::DynaFn;
use lite_builtin::pure::lite;
use lite_error::Phase;
use lite_front::records as p;
use lite_model::{
    ClassMapping, ClassMappingRelational, FilterMappingJoinMediated, FilterPointer, JStr, JoinChainElement,
    PropertyMapping, PropertyMappingOtherwiseEmbedded, RelationalOperation, cat,
};
use lite_names::ModelIndex;
use lite_names::calls;
use lite_names::model_index::java_list;
use lite_types::platform::ROWS_MARKER;

use crate::error::{Thrown, require};
use crate::jdk::{LinkedSet, equals_ignore_case};
use crate::mapping_ledger::MappingLedger;
use crate::mapping_normalizer;
use crate::milestoning_facts;
use crate::pipeline::Pipeline;
use crate::rel_op_translator::{self, TableScope};
use crate::resolved_mapping::ResolvedMapping;
use crate::union_synthesis::{self, UnionRoute};
use crate::view_relation;

use p::ValueSpecification as V;

/// `JoinChainEmission.emitHopsForStructuralPm` (:45): pass 1 -- the hops one structural PM (Join, embedded,
/// otherwise-embedded, inline-embedded, join-terminal column) hoists into the top pipeline, in PM order.
#[allow(clippy::too_many_arguments)]
pub fn emit_hops_for_structural_pm(
    p: &mut Pipeline<'_>,
    pm: &PropertyMapping,
    owner_class_fqn: &JStr,
    main_db: &JStr,
    main_table: &JStr,
    row_bind: &Arc<p::Variable>,
    model: &ModelIndex,
    md: &ResolvedMapping<'_>,
) -> Result<(), Thrown> {
    let knowledge = model.knowledge();
    match pm {
        PropertyMapping::PropertyMappingJoin(j) if p.dropped_routed_props.contains(&j.property_name) => {
            // routed property dropped at classification (poisoned reason on the ledger) -- no hops, no slot, no binding
            Ok(())
        }
        PropertyMapping::PropertyMappingJoin(j) => emit_join_chain_routed(
            p,
            &j.joins,
            Some(&j.database),
            Some(&j.property_name),
            Some(owner_class_fqn),
            main_db,
            main_table,
            row_bind,
            model,
            Some(md),
            true,
            j.target_set_id.as_ref(),
        ),
        PropertyMapping::PropertyMappingJoinTerminalColumn(jtc) => emit_join_chain(
            p,
            &jtc.joins,
            Some(&jtc.database),
            None,
            Some(owner_class_fqn),
            main_db,
            main_table,
            row_bind,
            model,
            Some(md),
            false,
        ),
        PropertyMapping::PropertyMappingLocalProperty(lp) => {
            emit_hops_for_structural_pm(p, &lp.body, owner_class_fqn, main_db, main_table, row_bind, model, md)
        }
        PropertyMapping::PropertyMappingOtherwiseEmbedded(oe) => {
            emit_otherwise_embedded_hop(p, oe, owner_class_fqn, main_db, main_table, row_bind, model, md)?;
            // the PARTIAL's structural sub-PMs hoist like a plain embedded block (bondClassification: @J inside
            // Otherwise(...)) -- without them the partial's ctor field reads a slot that was never minted
            let oe_owner = knowledge
                .hierarchy_class(Some(owner_class_fqn))
                .ok_or_else(|| Thrown::never_fired("JoinChainEmission#1"))?;
            let oe_type = knowledge.property_type(Some(oe_owner), &oe.property_name)?;
            if let Some(p::TypeExpression::TypeExpressionNameRef(nr)) = &oe_type {
                hoist_sub_pms(
                    p,
                    &oe.embedded,
                    &nr.name,
                    main_db,
                    main_table,
                    row_bind,
                    model,
                    md,
                    |prop, mapping| {
                        cat!(
                            "Otherwise-embedded sub-PM '",
                            prop,
                            "' collides with an existing pipeline slot",
                            " of the same name. Mapping=",
                            mapping
                        )
                    },
                )?;
            }
            Ok(())
        }
        PropertyMapping::PropertyMappingEmbedded(emb) => {
            // sub-PM join chains hoist into the TOP pipeline (the embedded instance shares the owner's row); the owner
            // for class-typed detection is the EMBEDDED class. An owner the model has not: nothing hoists (the
            // funnel's known miss)
            let prop_type = match knowledge.hierarchy_class(Some(owner_class_fqn)) {
                Some(owner) => knowledge.property_type(Some(owner), &emb.property_name)?,
                None => None,
            };
            if let Some(p::TypeExpression::TypeExpressionNameRef(nr)) = &prop_type {
                hoist_sub_pms(
                    p,
                    &emb.property_mappings,
                    &nr.name,
                    main_db,
                    main_table,
                    row_bind,
                    model,
                    md,
                    |prop, mapping| {
                        cat!(
                            "Embedded sub-PM '",
                            prop,
                            "' collides with an existing pipeline slot",
                            " of the same name; distinct same-named",
                            " class-typed joins across embedded levels",
                            " are a roadmap feature. Mapping=",
                            mapping
                        )
                    },
                )?;
            }
            Ok(())
        }
        PropertyMapping::PropertyMappingInlineEmbedded(ie) => {
            // the referenced set's PMs splice at materialization (the inline instance shares the owner's row) -- their
            // STRUCTURAL hops hoist into the TOP pipeline exactly like a direct embedded block
            let referenced = md.first_relational(&ie.set_id);
            // a class mapping of a class the model has not: the model's error (ruling 29)
            let Some(owner) = knowledge.hierarchy_class(Some(owner_class_fqn)) else {
                return Err(Thrown::model(
                    Phase::Normalize,
                    cat!(
                        "Inline-embedded PM '",
                        &ie.property_name,
                        "': unknown owner class '",
                        owner_class_fqn,
                        "'; mapping=",
                        md.qualified_name()
                    ),
                    None,
                ));
            };
            let prop_type = knowledge.property_type(Some(owner), &ie.property_name)?;
            if let Some(referenced) = referenced
                && let Some(p::TypeExpression::TypeExpressionNameRef(_)) = &prop_type
            {
                // the spliced PMs belong to the REFERENCED set's class (a subclass of the declared prop type)
                let inl_cls = &referenced.class_name;
                hoist_sub_pms(
                    p,
                    &referenced.property_mappings,
                    inl_cls,
                    main_db,
                    main_table,
                    row_bind,
                    model,
                    md,
                    |prop, mapping| {
                        cat!(
                            "Inline-embedded sub-PM '",
                            prop,
                            "' collides with an existing pipeline slot",
                            " of the same name. Mapping=",
                            mapping
                        )
                    },
                )?;
            }
            Ok(())
        }
        // Column / Enum / Expression: nested JoinNav handled in Pass 2 (Java's `default -> {}`)
        PropertyMapping::PropertyMappingColumn(_)
        | PropertyMapping::PropertyMappingEnumeratedColumn(_)
        | PropertyMapping::PropertyMappingEnumeratedExpression(_)
        | PropertyMapping::PropertyMappingExpression(_) => Ok(()),
    }
}

/// The loop Java spells three times in `emitHopsForStructuralPm` (:76-93, :104-122, :157-173): each sub-PM of an
/// embedded, otherwise-embedded or inline-spliced block hoists under `owner_cls`, loud (`collision`'s text, with the
/// property and the mapping) when a class-typed Join sub-PM lands on a slot another owner holds.
#[allow(clippy::too_many_arguments)]
fn hoist_sub_pms(
    p: &mut Pipeline<'_>,
    subs: &[PropertyMapping],
    owner_cls: &JStr,
    main_db: &JStr,
    main_table: &JStr,
    row_bind: &Arc<p::Variable>,
    model: &ModelIndex,
    md: &ResolvedMapping<'_>,
    collision: fn(&JStr, &JStr) -> JStr,
) -> Result<(), Thrown> {
    for sub in subs {
        if let PropertyMapping::PropertyMappingJoin(j) = sub
            && p.alias_to_target_table.contains_key(&j.property_name)
            && class_typed_target_if_mapped(Some(owner_cls), &j.property_name, model, p.ledger()?)?.is_some()
            && p.nav_slot_owner.get(&j.property_name) != Some(owner_cls)
        {
            return Err(Thrown::not_implemented(collision(
                &j.property_name,
                md.qualified_name(),
            )));
        }
        record_nav_slot_owner(p, sub, owner_cls, model)?;
        emit_hops_for_structural_pm(p, sub, owner_cls, main_db, main_table, row_bind, model, md)?;
    }
    Ok(())
}

/// `JoinChainEmission.emitOtherwiseEmbeddedHop` (:187): an otherwise-embedded block's fallback navigation hop.
#[allow(clippy::too_many_arguments)]
pub fn emit_otherwise_embedded_hop(
    p: &mut Pipeline<'_>,
    oe: &Arc<PropertyMappingOtherwiseEmbedded>,
    owner_class_fqn: &JStr,
    main_db: &JStr,
    main_table: &JStr,
    row_bind: &Arc<p::Variable>,
    model: &ModelIndex,
    md: &ResolvedMapping<'_>,
) -> Result<(), Thrown> {
    let PropertyMapping::PropertyMappingJoin(join_fallback) = &oe.fallback else {
        return Err(Thrown::not_implemented(cat!(
            "OtherwiseEmbedded PM '",
            &oe.property_name,
            "' fallback kind ",
            pm_simple_name(&oe.fallback),
            " not supported (Join only). Mapping=",
            md.qualified_name()
        )));
    };
    let knowledge = model.knowledge();
    let Some(owner) = knowledge.hierarchy_class(Some(owner_class_fqn)) else {
        return Err(Thrown::model(
            Phase::Normalize,
            cat!(
                "OtherwiseEmbedded PM '",
                &oe.property_name,
                "': unknown owner class '",
                owner_class_fqn,
                "'; mapping=",
                md.qualified_name()
            ),
            None,
        ));
    };
    let prop_type = knowledge.property_type(Some(owner), &oe.property_name)?;
    let Some(p::TypeExpression::TypeExpressionNameRef(nr)) = &prop_type else {
        return Err(Thrown::model(
            Phase::Normalize,
            cat!(
                "OtherwiseEmbedded PM '",
                &oe.property_name,
                "' has non-class property type; mapping=",
                md.qualified_name()
            ),
            None,
        ));
    };
    let target_class_fqn = &nr.name;
    if !p.ledger()?.is_mapped(target_class_fqn) {
        return Err(Thrown::not_implemented(cat!(
            "OtherwiseEmbedded PM '",
            &oe.property_name,
            "' target class '",
            target_class_fqn,
            "' is not mapped; mapping=",
            md.qualified_name()
        )));
    }
    // the fallback Join chain as a legacyNavigate step under a slot named after the property; the fallback's set is
    // a PIN the navigator carries (a route into that set's function), never a stamped fact consulted at lookup
    emit_join_chain_routed(
        p,
        &join_fallback.joins,
        Some(&join_fallback.database),
        Some(&oe.property_name),
        Some(owner_class_fqn),
        main_db,
        main_table,
        row_bind,
        model,
        Some(md),
        true,
        Some(join_fallback.target_set_id.as_ref().unwrap_or(&oe.fallback_set_id)),
    )
}

/// `JoinChainEmission.perArmHops` (:235): the hops a routed navigation emits at the top (the first only, when the
/// routes' chains are not uniform: the rest are pushed into each arm).
pub fn per_arm_hops(
    p: &Pipeline<'_>,
    prop_name: Option<&JStr>,
    target_class_fqn: Option<&JStr>,
    hops: &[Arc<JoinChainElement>],
) -> Vec<Arc<JoinChainElement>> {
    if target_class_fqn.is_none() || hops.len() < 2 {
        return hops.to_vec();
    }
    let rr = prop_name.and_then(|n| p.union_routes.get(n));
    match rr {
        Some(rr) if !union_synthesis::uniform_chained_routes(&union_synthesis::member_joins(rr)) => {
            vec![hops[0].clone()]
        }
        _ => hops.to_vec(),
    }
}

/// `JoinChainEmission.emitJoinChain` (:257, the eleven-argument form): [`emit_join_chain_routed`] with no routed set.
#[allow(clippy::too_many_arguments)]
pub fn emit_join_chain(
    p: &mut Pipeline<'_>,
    hops: &[Arc<JoinChainElement>],
    chain_db: Option<&JStr>,
    prop_name: Option<&JStr>,
    owner_class_fqn: Option<&JStr>,
    main_db: &JStr,
    main_table: &JStr,
    row_bind: &Arc<p::Variable>,
    model: &ModelIndex,
    md: Option<&ResolvedMapping<'_>>,
    class_typed_terminus: bool,
) -> Result<(), Thrown> {
    emit_join_chain_routed(
        p,
        hops,
        chain_db,
        prop_name,
        owner_class_fqn,
        main_db,
        main_table,
        row_bind,
        model,
        md,
        class_typed_terminus,
        None,
    )
}

/// `JoinChainEmission.emitJoinChain` (:276, the twelve-argument form): one PM's chain onto the pipeline -- shared
/// prefixes dedup by join path; a class-typed terminus is a `legacyNavigate` slot (or the routed union's
/// `route(...)` list), a physical one a `joinSlot`.
#[allow(clippy::too_many_arguments)]
pub fn emit_join_chain_routed(
    p: &mut Pipeline<'_>,
    hops: &[Arc<JoinChainElement>],
    chain_db: Option<&JStr>,
    prop_name: Option<&JStr>,
    owner_class_fqn: Option<&JStr>,
    main_db: &JStr,
    main_table: &JStr,
    _row_bind: &Arc<p::Variable>, // unread, as in Java
    model: &ModelIndex,
    md: Option<&ResolvedMapping<'_>>,
    class_typed_terminus: bool,
    routed_set_id: Option<&JStr>,
) -> Result<(), Thrown> {
    // a CLASS-TYPED chain is a mapping's; the view lift (E.5) emits physical hops only and carries no mapping
    let owner = if class_typed_terminus {
        Some(require(md, Some("class-typed join chain without its mapping"))?)
    } else {
        md
    };
    let mut target_class_fqn: Option<JStr> = None;
    if class_typed_terminus && let Some(prop) = prop_name {
        // a class-typed property ALWAYS ends in a navigate (legacy routes as composition, §11 A15)
        target_class_fqn = class_typed_target(owner_class_fqn, prop, model)?;
        let few_routes = p.union_routes.get(prop).is_none_or(|entries| entries.len() == 1);
        // one route entry to a strict subclass of a target class with no Relational set of its own: the navigation
        // lands on THAT class -- the engine's one routed set IS the rows
        if let Some(target) = &target_class_fqn
            && let Some(routed_set_id) = routed_set_id
            && few_routes
            && !mapping_normalizer::has_main_table(require(owner, None)?, target, model)?
            && let Some(routed) = require(owner, None)?.set(Some(routed_set_id))?
            && routed.class_name() != target
            && model.knowledge().is_subtype(routed.class_name(), target)
        {
            target_class_fqn = Some(routed.class_name().clone());
            // the navigation is no longer a union route: its target rows are the member's own extent, keys unsuffixed
            p.union_routes.remove(prop);
        }
    }
    let hops = per_arm_hops(p, prop_name, target_class_fqn.as_ref(), hops);
    let mut prefix_path: Vec<JStr> = Vec::new();
    let mut prev_alias: Option<JStr> = None;
    // Java's `prevTable` is nullable, but never null: it starts at the main table and is only ever set to a slot's
    // target table (below), so its `if (prevTable != null)` (:374) always holds
    let mut prev_table: JStr = main_table.clone();
    for (i, hop) in hops.iter().enumerate() {
        prefix_path.push(hop.join_name.clone());
        let is_last_hop = i + 1 == hops.len();
        let navigate_target = if is_last_hop { target_class_fqn.as_ref() } else { None };
        let emit_navigate = navigate_target.is_some();

        // Dedup. Physical sub-row hops are identified by their STRUCTURED prefix path (a chain [A, B] and a single join
        // literally named "A__B" stay distinct); class-instance hops dedup on the property name.
        let path_key = if emit_navigate { None } else { Some(prefix_path.clone()) };
        let nav_alias = if emit_navigate {
            let prop = require(prop_name, Some("nav hop needs propName"))?;
            Some(mint_nav_slot_alias(p, model, main_db, main_table, prop))
        } else {
            None
        };
        // A hop's declared join type is read by NOTHING here, on purpose: the engine ISOLATES a property mapping's
        // chain in the property's own subquery (audit fix A4); the only INNER that changes rows is the mapping
        // ~filter's (`inner_filtered_source`).
        if let Some(nav) = &nav_alias {
            if let Some(table) = p.alias_to_target_table.get(nav) {
                prev_table = table.clone();
                prev_alias = Some(nav.clone());
                continue;
            }
        } else if let Some(existing) = path_key.as_ref().and_then(|k| p.path_to_slot.get(k)).cloned() {
            // a path's slot is always a key of `alias_to_target_table` (both are put together below; nothing removes
            // either), so Java's `prevTable` is never set to null here
            prev_table = p
                .alias_to_target_table
                .get(&existing)
                .cloned()
                .expect("a hoisted path's slot has its target table");
            prev_alias = Some(existing);
            continue;
        }
        // the slot name read back in generated Pure ($row.<slot>): the property for a class hop, the "__"-joined path
        // for a physical one, disambiguated only on a clash (the structured path is the identity `slot_for` resolves)
        let slot_alias = match nav_alias {
            Some(nav) => nav,
            None => unique_slot_name(p, path_key.as_deref())?,
        };

        let hop_db = match (&hop.database_name, chain_db) {
            (Some(db), _) => db.clone(),
            (None, Some(db)) => db.clone(),
            (None, None) => main_db.clone(),
        };
        let Some(jd) = model.find_join(Some(&hop_db), &hop.join_name) else {
            return Err(Thrown::model(
                Phase::Normalize,
                cat!(
                    "Join '",
                    &hop.join_name,
                    "' not found in db '",
                    &hop_db,
                    "'; PM='",
                    value_of(prop_name),
                    "', ",
                    view_relation::owner(md, &hop_db)
                ),
                None,
            ));
        };
        // a join condition names its two relations by table: a view's columns in their declared spelling
        let mut join_cond = view_relation::declared_spelling(&jd.operation, &hop_db, model)?;
        // a SOLE non-source view candidate in the condition is the join's TARGET -- expanded as a relation, never
        // substituted
        let mut cond_tables = LinkedSet::new();
        rel_op_translator::collect_tables_in(&join_cond, &mut cond_tables)?;
        cond_tables.remove(&prev_table);
        let mut view_target = match cond_tables.iter().next() {
            Some(only) if cond_tables.len() == 1 && model.find_view(&hop_db, only).is_some() => Some(only.clone()),
            _ => None,
        };
        if let Some(vt) = view_target.clone()
            && emit_navigate
            && let Some(sub) =
                plain_class_view_cond(&join_cond, &vt, navigate_target, &hop_db, model, require(owner, None)?)?
        {
            join_cond = sub;
            view_target = None;
        }
        let ht = hop_target(
            &join_cond,
            view_target.as_ref(),
            Some(&prev_table),
            &hop_db,
            &hop.join_name,
            prop_name,
            i as i32,
            p,
            model,
            md,
        )?;
        let join_cond = ht.cond;
        let target_table = ht.table;
        let view_target = ht.view;

        let s = variable("s");
        let t = variable("t");
        let mut cond_scope = TableScope::new();
        let source_row = match &prev_alias {
            None => V::Variable(s.clone()),
            Some(alias) => property(V::Variable(s.clone()), alias),
        };
        cond_scope.put(prev_table.clone(), source_row);
        if target_table != prev_table {
            cond_scope.put(target_table.clone(), V::Variable(t.clone()));
        }
        let cond = rel_op_translator::translate(&join_cond, &cond_scope, Some(&V::Variable(t.clone())), None, None)?;
        let cond_lambda = lambda(vec![s.clone(), t.clone()], vec![cond]);

        if let Some(target_class) = navigate_target {
            let get_all = call(calls::platform(&JStr::from("getAll"), vec![element_ptr(target_class)]));
            let slot = col(col_spec(
                &slot_alias,
                Some(lambda(Vec::new(), vec![get_all])),
                None,
                None,
            ));
            // The condition speaks TABLE-row scope while the slot's thunk is the CLASS extent -- spell the target's
            // table row into the call; a VIEW target navigates over the view's RELATION (computed, as Java does, even
            // where the routed form below does not use it)
            let target_rows = match &view_target {
                Some(vt) => p
                    .views
                    .body(model.find_view(&hop_db, vt).ok_or_else(Thrown::no_such_element)?)?,
                None => table_reference(&hop_db, &target_table),
            };
            // ROUTED union navigation: ONE navigate carries every route the author wrote (legacy routes as
            // composition) -- each names the target SET'S OWN FUNCTION, its rows and its join as written
            let routes = prop_name.and_then(|n| p.union_routes.get(n)).cloned();
            let expr = p.expr.clone();
            p.expr = match routes {
                Some(routes) => {
                    let list = route_list(
                        p,
                        &routes,
                        prop_name,
                        Some(&prev_table),
                        prev_alias.as_ref(),
                        main_table,
                        &s,
                        &t,
                        model,
                        require(owner, None)?,
                    )?;
                    exact(lite::LEGACY_NAVIGATE, vec![expr, slot, collection(list)])
                }
                None => exact(
                    lite::LEGACY_NAVIGATE,
                    vec![expr, slot, target_rows, V::LambdaFunction(cond_lambda)],
                ),
            };
            p.class_slots.insert(slot_alias.clone());
        } else {
            let target_rel = match &view_target {
                Some(vt) => p
                    .views
                    .body(model.find_view(&hop_db, vt).ok_or_else(Thrown::no_such_element)?)?,
                None => table_reference(&hop_db, &target_table),
            };
            // a VIEW hop carries the frame identity in the spare alias channel -- the checker lifts it onto the slot
            let slot = col(col_spec(
                &slot_alias,
                Some(lambda(Vec::new(), vec![target_rel])),
                None,
                view_target.as_ref(),
            ));
            let expr = p.expr.clone();
            p.expr = exact(lite::JOIN_SLOT, vec![expr, slot, V::LambdaFunction(cond_lambda)]);
        }
        p.alias_to_target_table.put(slot_alias.clone(), target_table.clone());
        let columns = target_column_names(
            &hop_db,
            &target_table,
            view_target.as_ref(),
            model,
            &hop.join_name,
            prop_name,
            md,
        )?;
        p.alias_to_target_columns.put(slot_alias.clone(), columns);
        if let Some(key) = path_key {
            p.path_to_slot.put(key, slot_alias.clone());
        }
        prev_table = target_table;
        prev_alias = Some(slot_alias);
    }
    Ok(())
}

/// `JoinChainEmission.navSlotFor` (:470): the class-typed property's navigate slot; loud when its join was never
/// emitted.
pub fn nav_slot_for(p: &Pipeline<'_>, prop_name: &JStr) -> Result<JStr, Thrown> {
    match p.nav_slot_by_prop.get(prop_name) {
        Some(slot) => Ok(slot.clone()),
        None => Err(Thrown::not_implemented(cat!(
            "class-typed property '",
            prop_name,
            "' has no navigate slot on this pipeline \u{2014} its join was never emitted"
        ))),
    }
}

/// `JoinChainEmission.relationRef` (:481): a hop's relation -- a view's lifted body, else `tableReference(db, name)`.
pub fn relation_ref(
    db: &JStr,
    name: &JStr,
    model: &ModelIndex,
    md: &ResolvedMapping<'_>,
) -> Result<p::ValueSpecification, Thrown> {
    match model.find_view(db, name) {
        Some(view) => md.views().body(view),
        None => Ok(table_reference(db, name)),
    }
}

/// `JoinChainEmission.routeList` (:503): one `route(member(), rows, (s, t) | cond)` per union route, in route order.
#[allow(clippy::too_many_arguments)]
pub fn route_list(
    _p: &Pipeline<'_>, // unread, as in Java
    routes: &[UnionRoute],
    prop_name: Option<&JStr>,
    prev_table: Option<&JStr>,
    prev_alias: Option<&JStr>,
    main_table: &JStr,
    s: &Arc<p::Variable>,
    t: &Arc<p::Variable>,
    model: &ModelIndex,
    md: &ResolvedMapping<'_>,
) -> Result<Vec<p::ValueSpecification>, Thrown> {
    let per_arm = !union_synthesis::uniform_chained_routes(&union_synthesis::member_joins(routes));
    let mut out = Vec::with_capacity(routes.len());
    for route in routes {
        let j = &route.join;
        let chain = &j.joins;
        let route_name = || {
            cat!(
                "route '",
                value_of(prop_name),
                "[",
                value_of(j.target_set_id.as_ref()),
                "]' "
            )
        };
        let pin = union_synthesis::resolve_pin(md, j.target_set_id.as_ref(), model)?;
        let member = pin.as_ref().map(|pt| &pt.set);
        // a ROUTE INTO A ~func MEMBER (A13): the route's rows are the function's rows; a chain pushed into such an arm
        // has no table to root at -- loud
        let rf_member = match member {
            Some(ClassMapping::ClassMappingRelationFunction(rf)) => Some(rf),
            _ => None,
        };
        let rm = match member {
            Some(ClassMapping::ClassMappingRelational(r0)) => Some(r0),
            _ => None,
        };
        if rf_member.is_none() && rm.is_none() {
            return Err(Thrown::not_implemented(cat!(
                route_name(),
                "targets a set that is not Relational; mapping=",
                md.qualified_name()
            )));
        }
        let member_main = match rm {
            None => None,
            Some(rm) => match &rm.main_table {
                Some(mt) => Some(mt.clone()),
                None => mapping_normalizer::infer_main_table_quiet(rm)?,
            },
        };
        if rm.is_some() && member_main.is_none() {
            return Err(Thrown::not_implemented(cat!(
                route_name(),
                "targets a set with no main table; mapping=",
                md.qualified_name()
            )));
        }
        let in_arm = per_arm && chain.len() > 1;
        if in_arm && rf_member.is_some() {
            return Err(Thrown::not_implemented(cat!(
                route_name(),
                "pushes a chain into a ~func member; mapping=",
                md.qualified_name()
            )));
        }
        // `chain.get(0)` / `chain.get(size - 1)`: a Join PM's chain is never empty (Java's record refuses one)
        let hop = if in_arm { &chain[0] } else { &chain[chain.len() - 1] };
        let db = hop.database_name.as_ref().unwrap_or(&j.database);
        let Some(jd) = model.find_join(Some(db), &hop.join_name) else {
            return Err(Thrown::model(
                Phase::Normalize,
                cat!(
                    "Join '",
                    &hop.join_name,
                    "' not found in db '",
                    db,
                    "'; PM='",
                    value_of(prop_name),
                    "', mapping=",
                    md.qualified_name()
                ),
                None,
            ));
        };
        let (rows, cond) = if in_arm {
            // a PER-ARM chain joins its mids onto the set's table INSIDE the route (walked back from the set); its
            // condition is the chain's FIRST hop over (navigator row, the landing mid)
            let mm = require(member_main.as_ref(), None)?;
            let steps = union_synthesis::inbound_arm_steps(j, require(prop_name, None)?, &mm.table, md, model)?;
            let mut rows = relation_ref(&mm.database, &mm.table, model, md)?;
            for st in &steps {
                let mid = relation_ref(&st.db, &st.table, model, md)?;
                let slot = col(col_spec(&st.alias, Some(lambda(Vec::new(), vec![mid])), None, None));
                rows = exact(lite::JOIN_SLOT, vec![rows, slot, V::LambdaFunction(st.cond.clone())]);
            }
            // `steps.get(size - 1)`: a chain of two hops or more has a mid step
            let landing = &steps[steps.len() - 1];
            let mut scope = TableScope::new();
            scope.put(main_table.clone(), V::Variable(s.clone()));
            scope.put(landing.table.clone(), property(V::Variable(t.clone()), &landing.alias));
            let cond = rel_op_translator::translate(
                &view_relation::declared_spelling(&jd.operation, db, model)?,
                &scope,
                Some(&V::Variable(t.clone())),
                None,
                None,
            )?;
            (rows, cond)
        } else {
            let r_prev = require(prev_table, None)?;
            // the tables of the join AS WRITTEN; the condition translated below is its declared spelling
            let mut tables = LinkedSet::new();
            rel_op_translator::collect_tables_in(&jd.operation, &mut tables)?;
            tables.remove(r_prev);
            let tgt = match tables.iter().next() {
                Some(only) if tables.len() == 1 && model.find_view(db, only).is_some() => only.clone(),
                _ => mapping_normalizer::determine_target_table(
                    &jd.operation,
                    Some(r_prev),
                    &hop.join_name,
                    prop_name,
                    chain.len() as i32,
                    md.qualified_name(),
                )?,
            };
            let rows = match (rf_member, model.find_view(db, &tgt)) {
                (Some(rf), _) => mapping_normalizer::relation_function_pipeline(rf, model)?,
                (None, Some(view)) => md.views().body(view)?,
                (None, None) => table_reference(db, &tgt),
            };
            let mut scope = TableScope::new();
            let source_row = match prev_alias {
                None => V::Variable(s.clone()),
                Some(alias) => property(V::Variable(s.clone()), alias),
            };
            scope.put(r_prev.clone(), source_row);
            if tgt != *r_prev {
                scope.put(tgt.clone(), V::Variable(t.clone()));
            }
            let cond = rel_op_translator::translate(
                &view_relation::declared_spelling(&jd.operation, db, model)?,
                &scope,
                Some(&V::Variable(t.clone())),
                None,
                None,
            )?;
            (rows, cond)
        };
        // a ROOT route beside others names the root set's own function (its class-level function); the queried
        // mapping resolves it
        let target = exact_named(
            &union_synthesis::member_function_of(require(pin.as_ref(), None)?)?,
            Vec::new(),
        );
        out.push(exact(
            lite::ROUTE,
            vec![
                target,
                rows,
                V::LambdaFunction(lambda(vec![s.clone(), t.clone()], vec![cond])),
            ],
        ));
    }
    Ok(out)
}

/// `JoinChainEmission.recordNavSlotOwner` (:603): a class-typed Join sub-PM's owner, first wins.
pub fn record_nav_slot_owner(
    p: &mut Pipeline<'_>,
    sub: &PropertyMapping,
    owner_cls: &JStr,
    model: &ModelIndex,
) -> Result<(), Thrown> {
    if let PropertyMapping::PropertyMappingJoin(j) = sub
        && class_typed_target(Some(owner_cls), &j.property_name, model)?.is_some()
    {
        p.nav_slot_owner
            .put_if_absent(j.property_name.clone(), owner_cls.clone());
    }
    Ok(())
}

/// `JoinChainEmission.mintNavSlotAlias` (:618): the class-typed property's navigate alias, minted once -- `<prop>_nav`
/// when it collides with a main-table column (`equalsIgnoreCase`), `columns` or the rows marker, then `_` until free.
pub fn mint_nav_slot_alias(
    p: &mut Pipeline<'_>,
    model: &ModelIndex,
    main_db: &JStr,
    main_table: &JStr,
    prop_name: &JStr,
) -> JStr {
    if let Some(known) = p.nav_slot_by_prop.get(prop_name) {
        return known.clone();
    }
    // the schema stripped (`SCHEMA.TABLE` asks the default schema's TABLE: the census's surprise, ported)
    let table_name = match lite_model::text::last_index_of(&main_table.0, &[u16::from(b'.')]) {
        Some(dot) => JStr::from(&main_table.0[dot + 1..]),
        None => main_table.clone(),
    };
    // include-aware: the main table may live in an INCLUDED database. A slot named after the platform's RELATION
    // accessors (`columns`, `rows`) would read as the accessor on the row var -- mint clear. The column test ignores
    // case: the slot and the column share one SQL row, where an unquoted `mid` and `MID` are the same name.
    let collides = model
        .knowledge()
        .table(Some(main_db), Some(&table_name))
        .is_some_and(|td| td.columns.iter().any(|cd| equals_ignore_case(&cd.name.0, &prop_name.0)))
        || prop_name.eq_str("columns")
        || prop_name.eq_str(ROWS_MARKER);
    let mut alias = if collides {
        cat!(prop_name, "_nav")
    } else {
        prop_name.clone()
    };
    // ALWAYS minted past every slot already on the pipeline (audit 2026-09-15 P0-7)
    while p.alias_to_target_table.contains_key(&alias) {
        alias = cat!(&alias, "_");
    }
    p.nav_slot_by_prop.put(prop_name.clone(), alias.clone());
    alias
}

/// `JoinChainEmission.uniqueSlotName` (:662): the path's names joined by `__`, then `__<n>` from 2 until free (a
/// `NullPointerException` for no path, as `String.join` throws).
pub fn unique_slot_name(p: &Pipeline<'_>, path: Option<&[JStr]>) -> Result<JStr, Thrown> {
    // `String.join("__", null)`: `Objects.requireNonNull(elements)`, no message
    let path = require(path, None)?;
    let mut base: Vec<u16> = Vec::new();
    for (i, name) in path.iter().enumerate() {
        if i > 0 {
            base.extend("__".encode_utf16());
        }
        base.extend_from_slice(&name.0);
    }
    let base = JStr(base);
    if !p.alias_to_target_table.contains_key(&base) {
        return Ok(base);
    }
    let mut n: i32 = 2;
    loop {
        let candidate = cat!(&base, "__", n.to_string());
        n += 1;
        if !p.alias_to_target_table.contains_key(&candidate) {
            return Ok(candidate);
        }
    }
}

/// `JoinChainEmission.joinPath` (:675): the hops' join names.
pub fn join_path(hops: &[Arc<JoinChainElement>]) -> Vec<JStr> {
    hops.iter().map(|h| h.join_name.clone()).collect()
}

/// `JoinChainEmission.slotFor` (:689): the slot an emitted chain landed in (by its join path); loud when it was never
/// hoisted. The translator's `PipelineView::slot_for`.
pub fn slot_for(p: &Pipeline<'_>, hops: &[Arc<JoinChainElement>]) -> Result<JStr, Thrown> {
    let key = join_path(hops);
    match p.path_to_slot.get(&key) {
        Some(slot) => Ok(slot.clone()),
        None => Err(Thrown::not_implemented(cat!(
            "join chain ",
            java_list(&key),
            " was never emitted on this pipeline \u{2014} the expression",
            " navigates a join that was not hoisted as a slot"
        ))),
    }
}

/// `JoinChainEmission.classTypedTargetIfMapped` (:700): the property's class type when it (or a subclass) is mapped
/// in this mapping's closure.
pub fn class_typed_target_if_mapped(
    owner_class_fqn: Option<&JStr>,
    prop_name: &JStr,
    model: &ModelIndex,
    ledger: &MappingLedger,
) -> Result<Option<JStr>, Thrown> {
    let Some(owner) = model.knowledge().hierarchy_class(owner_class_fqn) else {
        return Ok(None);
    };
    let prop_type = model.knowledge().property_type(Some(owner), prop_name)?;
    // a parameterized declaration (`Class<Any>`, `Property<Nil,Any|*>`, the real m3 ends) targets its RAW class
    let Some(tgt) = prop_type.as_ref().and_then(p::TypeExpression::raw_class_name) else {
        return Ok(None);
    };
    // the declared class itself, or an ABSTRACTION of mapped classes: either way a navigation, never a column read
    Ok((ledger.is_mapped(tgt) || has_mapped_subclass(tgt, model, ledger)).then(|| tgt.clone()))
}

/// `JoinChainEmission.classTypedTarget` (:722): the property's class type, when it is a class the model has.
pub fn class_typed_target(
    owner_class_fqn: Option<&JStr>,
    prop_name: &JStr,
    model: &ModelIndex,
) -> Result<Option<JStr>, Thrown> {
    let Some(owner) = model.knowledge().hierarchy_class(owner_class_fqn) else {
        return Ok(None);
    };
    let prop_type = model.knowledge().property_type(Some(owner), prop_name)?;
    let tgt = prop_type.as_ref().and_then(p::TypeExpression::raw_class_name);
    Ok(tgt
        .filter(|t| model.knowledge().hierarchy_class(Some(t)).is_some())
        .cloned())
}

/// `JoinChainEmission.hasMappedSubclass` (:732): whether a class of `base`'s subtree is mapped here (a shadow walker
/// `ShadowWalkerCensusTest` pinned; the census retires with the Java).
pub fn has_mapped_subclass(base: &JStr, model: &ModelIndex, ledger: &MappingLedger) -> bool {
    // the graph's classes strictly below base (the catalog's are not the graph's), any of them mapped in this
    // mapping's closure
    model
        .knowledge()
        .subtree(base)
        .iter()
        .any(|c| model.find_class(c).is_some() && ledger.is_mapped(c))
}

/// `JoinChainEmission.JoinNavSpec` (:740): a join navigation an expression makes, with its chain's database.
#[derive(Clone, Debug, PartialEq)]
pub struct JoinNavSpec {
    pub chain: Arc<[Arc<JoinChainElement>]>,
    pub chain_db: Option<JStr>,
}

/// `JoinChainEmission.collectJoinNavigationsInPms` (:742, the two-argument form): the PMs' join navigations with no
/// mapping in hand (an Inline splice contributes nothing).
pub fn collect_join_navigations_in_pms(pms: &[PropertyMapping], out: &mut Vec<JoinNavSpec>) {
    collect_join_navigations_in_pms_of(pms, out, None);
}

/// `JoinChainEmission.collectJoinNavigationsInPms` (:750, the three-argument form): the PMs' join navigations in PM
/// order (= pass 2's emission order = the slot names); an Inline splice carries its referenced set's
/// (`ResolvedMapping::first_relational`).
pub fn collect_join_navigations_in_pms_of(
    pms: &[PropertyMapping],
    out: &mut Vec<JoinNavSpec>,
    md: Option<&ResolvedMapping<'_>>,
) {
    for pm in pms {
        match pm {
            PropertyMapping::PropertyMappingEnumeratedExpression(ee) => collect_join_navigations(&ee.expression, out),
            PropertyMapping::PropertyMappingExpression(expr) => collect_join_navigations(&expr.expression, out),
            PropertyMapping::PropertyMappingLocalProperty(lp) => {
                collect_join_navigations_in_pms_of(std::slice::from_ref(&lp.body), out, md)
            }
            PropertyMapping::PropertyMappingEmbedded(emb) => {
                collect_join_navigations_in_pms_of(&emb.property_mappings, out, md)
            }
            PropertyMapping::PropertyMappingOtherwiseEmbedded(oe) => {
                collect_join_navigations_in_pms_of(&oe.embedded, out, md)
            }
            PropertyMapping::PropertyMappingJoinTerminalColumn(jtc) => {
                collect_join_navigations(&jtc.terminal_column, out)
            }
            // an Inline splice CARRIES the referenced set's JoinNavs, resolvable only with the enclosing mapping in hand
            PropertyMapping::PropertyMappingInlineEmbedded(ie) => {
                if let Some(md) = md
                    && let Some(r2) = md.first_relational(&ie.set_id)
                {
                    collect_join_navigations_in_pms_of(&r2.property_mappings, out, Some(md));
                }
            }
            // Join / Column / EnumeratedColumn: Join handled by Pass 1; the others don't carry JoinNav.
            PropertyMapping::PropertyMappingJoin(_) => {}
            PropertyMapping::PropertyMappingColumn(_) => {}
            PropertyMapping::PropertyMappingEnumeratedColumn(_) => {}
        }
    }
}

/// `JoinChainEmission.collectJoinNavigations` (:789): one expression's join navigations, in reading order.
pub fn collect_join_navigations(op: &RelationalOperation, out: &mut Vec<JoinNavSpec>) {
    match op {
        RelationalOperation::RelationalOperationJoinNavigation(jn) => {
            out.push(JoinNavSpec {
                chain: jn.chain.clone(),
                chain_db: jn.database_name.clone(),
            });
            if let Some(terminal) = &jn.terminal {
                collect_join_navigations(terminal, out);
            }
        }
        RelationalOperation::RelationalOperationFunctionCall(fc) => {
            for a in fc.args.iter() {
                collect_join_navigations(a, out);
            }
        }
        RelationalOperation::RelationalOperationComparison(cmp) => {
            collect_join_navigations(&cmp.left, out);
            collect_join_navigations(&cmp.right, out);
        }
        RelationalOperation::RelationalOperationBooleanOp(bo) => {
            collect_join_navigations(&bo.left, out);
            collect_join_navigations(&bo.right, out);
        }
        RelationalOperation::RelationalOperationIsNull(n) => collect_join_navigations(&n.operand, out),
        RelationalOperation::RelationalOperationIsNotNull(n) => collect_join_navigations(&n.operand, out),
        RelationalOperation::RelationalOperationGroup(g) => collect_join_navigations(&g.inner, out),
        RelationalOperation::RelationalOperationArrayLiteral(a) => {
            for e in a.elements.iter() {
                collect_join_navigations(e, out);
            }
        }
        RelationalOperation::RelationalOperationLambda(lam) => collect_join_navigations(&lam.body, out),
        RelationalOperation::RelationalOperationLambdaParam(_) => {}
        RelationalOperation::RelationalOperationColumnRef(_) => {}
        RelationalOperation::RelationalOperationTargetColumnRef(_) => {}
        RelationalOperation::RelationalOperationLiteral(_) => {}
    }
}

/// `JoinChainEmission.HopTarget` (:822): a hop's (view-substituted) condition, its target table, and the view it
/// lands on.
#[derive(Clone, Debug, PartialEq)]
pub struct HopTarget {
    pub cond: RelationalOperation,
    pub table: JStr,
    pub view: Option<JStr>,
}

/// `JoinChainEmission.hopTarget` (:826): pass 2's target of one hop.
#[allow(clippy::too_many_arguments)]
pub fn hop_target(
    join_cond: &RelationalOperation,
    view_target: Option<&JStr>,
    prev_table: Option<&JStr>,
    hop_db: &JStr,
    join_name: &JStr,
    prop_name: Option<&JStr>,
    i: i32,
    _p: &Pipeline<'_>, // unread, as in Java
    model: &ModelIndex,
    md: Option<&ResolvedMapping<'_>>,
) -> Result<HopTarget, Thrown> {
    if let Some(view_target) = view_target {
        return Ok(HopTarget {
            cond: join_cond.clone(),
            table: view_target.clone(),
            view: Some(view_target.clone()),
        });
    }
    // PASS 2: source-side view projections substitute (plain views only); target-side view refs stay VERBATIM, so a
    // view landing is recognizable here too
    let join_cond =
        mapping_normalizer::resolve_view_refs_in_join(join_cond, hop_db, prev_table, model, md, None, None)?;
    // Java passes `ViewRelation.owner(md, hopDb)` (`mapping=<fqn>`) where the callee prints `mapping=` + its
    // argument: the text reads `mapping=mapping=<fqn>` (a Java quirk, ported)
    let nested = JStr::from("<nested>");
    let target_table = mapping_normalizer::determine_target_table(
        &join_cond,
        prev_table,
        join_name,
        Some(prop_name.unwrap_or(&nested)),
        i + 1,
        &view_relation::owner(md, hop_db),
    )?;
    let view = model
        .find_view(hop_db, &target_table)
        .is_some()
        .then(|| target_table.clone());
    Ok(HopTarget {
        cond: join_cond,
        table: target_table,
        view,
    })
}

/// `JoinChainEmission.plainClassViewCond` (:856): a class-typed navigate onto a PLAIN view of the target class's own
/// main table -- the condition substituted to physical; none keeps the expansion (the module's note on JCE:884).
pub fn plain_class_view_cond(
    join_cond: &RelationalOperation,
    view_target: &JStr,
    target_class_fqn: Option<&JStr>,
    hop_db: &JStr,
    model: &ModelIndex,
    md: &ResolvedMapping<'_>,
) -> Result<Option<RelationalOperation>, Thrown> {
    let Some(target_class_fqn) = target_class_fqn else {
        return Ok(None);
    };
    let vd = model
        .find_view(hop_db, view_target)
        .ok_or_else(Thrown::no_such_element)?;
    if vd.filter.is_some() || !vd.group_by_columns.is_empty() || vd.distinct {
        return Ok(None);
    }
    // JOIN-NAVIGATING columns disqualify too: rename-only means every column is a plain ColumnRef
    for vc in vd.column_mappings.iter() {
        let mut navs = Vec::new();
        collect_join_navigations(&vc.expression, &mut navs);
        if !navs.is_empty() {
            return Ok(None);
        }
    }
    let v_phys = model.view_main_table(hop_db, vd)?;
    // JCE:882-886's `catch (ModelException unmappedTarget) { return null; }` around `mainTableOf`, RE-EXPRESSED: its
    // one cause is `mainTableDefOf`'s throw when `mainTableOrNull` is null
    let Some(tgt_main) = mapping_normalizer::main_table_or_null(md, Some(target_class_fqn))? else {
        return Ok(None);
    };
    // the condition speaks the TARGET CLASS's row: a class over the view's PHYSICAL table reads physical columns
    // (substitute); a class over the VIEW itself is its frame -- keep the expansion
    if v_phys != tgt_main.table {
        return Ok(None);
    }
    // SINGLE-SET, group-free, NON-TEMPORAL targets only
    let sets = md.relational_sets(Some(target_class_fqn));
    if sets.len() != 1
        || !sets[0].group_by.is_empty()
        || sets[0].distinct
        || milestoning_facts::is_temporal(target_class_fqn, model)?
    {
        return Ok(None);
    }
    Ok(Some(mapping_normalizer::resolve_view_refs_in_join(
        join_cond,
        hop_db,
        Some(&v_phys),
        model,
        Some(md),
        Some(view_target),
        None,
    )?))
}

/// `JoinChainEmission.innerFilteredSource` (:921): an INNER join-mediated mapping filter's source,
/// `project(filter(...))` (the engine's `getRelationalElementWithInnerJoin`), on a pipeline of its own.
pub fn inner_filtered_source(
    rcm: &Arc<ClassMappingRelational>,
    jm: &Arc<FilterMappingJoinMediated>,
    model: &ModelIndex,
    md: &ResolvedMapping<'_>,
    ledger: &mut MappingLedger,
) -> Result<p::ValueSpecification, Thrown> {
    let jm_main = require(
        rcm.main_table.as_ref(),
        Some("join-mediated filter on a set without ~mainTable"),
    )?;
    let main_db = &jm_main.database;
    let main_table = &jm_main.table;
    let r = variable("irow");
    // the main relation: a table, or a VIEW's frame (the engine's ViewSelectSQLQuery -- the filter chain departs from
    // the view's row)
    let mut p = Pipeline::new(relation_ref(main_db, main_table, model, md)?, ledger, md.views());
    emit_join_chain(
        &mut p,
        &jm.joins,
        jm.source_db.as_ref(),
        None,
        Some(&rcm.class_name),
        main_db,
        main_table,
        &r,
        model,
        Some(md),
        false,
    )?;
    let (db_fqn, filter_name) = match &jm.filter {
        FilterPointer::FilterPointerCross(c) => (Some(&c.db), &c.name),
        FilterPointer::FilterPointerLocal(l) => (jm.source_db.as_ref(), &l.name),
    };
    let Some(fd) = model.find_filter(db_fqn, filter_name) else {
        return Err(Thrown::model(
            Phase::Normalize,
            cat!(
                "~filter '",
                filter_name,
                "' not found in db '",
                value_of(db_fqn),
                "'; class=",
                &rcm.class_name,
                ", mapping=",
                md.qualified_name()
            ),
            None,
        ));
    };
    // LEFT JOIN + WHERE == INNER JOIN + WHERE only when the condition is NULL-REJECTING on the joined side: the first
    // tolerant one is LOUD, never silently wrong (real INNER emission is follow-up)
    if null_tolerant(&fd.condition)? {
        return Err(Thrown::not_implemented(cat!(
            "(INNER) mapping ~filter '",
            filter_name,
            "' has a NULL-TOLERANT condition \u{2014}",
            " the LEFT+WHERE realization would keep parents the",
            " engine's INNER join drops; class=",
            &rcm.class_name,
            ", mapping=",
            md.qualified_name()
        )));
    }
    let terminal_alias = slot_for(&p, &jm.joins)?;
    let terminal_row = property(V::Variable(r.clone()), &terminal_alias);
    // the engine anchors the condition at the chain TERMINUS: the terminal binding takes precedence over the root even
    // when the chain lands back on the MAIN table
    let mut scope = TableScope::new();
    if let Some(terminal_table) = p.alias_to_target_table.get(&terminal_alias).cloned() {
        scope.put(terminal_table, terminal_row.clone());
    }
    scope.put_if_absent(main_table.clone(), V::Variable(r.clone()));
    mapping_normalizer::seed_alias_scope(&mut scope, &mut p, &r, main_table)?;
    let cond = rel_op_translator::translate(
        &view_relation::frame_rewrite_if_view(&fd.condition, main_db, main_table, md, model)?,
        &scope,
        Some(&terminal_row),
        Some(&r),
        p.view(),
    )?;
    let src = call(calls::platform(
        &JStr::from("filter"),
        vec![p.expr.clone(), V::LambdaFunction(lambda(vec![r.clone()], vec![cond]))],
    ));
    // every base column under its original name: a table's columns, or a view's declared columns (the frame's row)
    let mut base_names: Vec<JStr> = Vec::new();
    match model.find_view(main_db, main_table) {
        Some(main_view) => {
            for vc in main_view.column_mappings.iter() {
                base_names.push(vc.name.clone());
            }
        }
        None => {
            let Some(td) = model.knowledge().table(Some(main_db), Some(main_table)) else {
                return Err(Thrown::model(
                    Phase::Normalize,
                    cat!(
                        "main table '",
                        main_table,
                        "' not found in db '",
                        main_db,
                        "' for the (INNER) mapping ~filter of class '",
                        &rcm.class_name,
                        "', mapping=",
                        md.qualified_name()
                    ),
                    None,
                ));
            };
            for cd in td.columns.iter() {
                base_names.push(cd.name.clone());
            }
        }
    }
    let vd = variable("vd");
    let base_cols: Vec<Arc<p::ColSpec>> = base_names
        .iter()
        .map(|name| {
            let read = property(V::Variable(vd.clone()), name);
            col_spec(name, Some(lambda(vec![vd.clone()], vec![read])), None, None)
        })
        .collect();
    Ok(call(calls::platform(
        &JStr::from("project"),
        vec![src, col_spec_array(base_cols)],
    )))
}

/// `JoinChainEmission.nullTolerant` (:1015): whether a filter condition can be true over a null-extended row.
pub fn null_tolerant(op: &RelationalOperation) -> Result<bool, Thrown> {
    Ok(match op {
        RelationalOperation::RelationalOperationIsNull(_) => true,
        RelationalOperation::RelationalOperationFunctionCall(fc) => {
            // IDENTITY THROUGH THE REGISTRY (audit 2026-09-15 P2-4); the registry's names are ASCII, so a lone
            // surrogate's U+FFFD matches none, as the unit-exact name would not
            let dyna = DynaFn::of(&fc.name.lossy());
            if matches!(dyna, Some(DynaFn::IS_NULL | DynaFn::SQL_NULL)) {
                return Ok(true);
            }
            // audit 23: NULL-swallowing functions defeat the classification -- LOUD, never a silent verdict
            if matches!(dyna, Some(DynaFn::COALESCE | DynaFn::CASE | DynaFn::IF)) {
                return Err(Thrown::not_implemented(cat!(
                    "(INNER) mapping-",
                    "filter condition uses '",
                    &fc.name,
                    "' \u{2014} null-tolerance cannot be classified;",
                    " not supported yet"
                )));
            }
            for a in fc.args.iter() {
                if null_tolerant(a)? {
                    return Ok(true);
                }
            }
            false
        }
        RelationalOperation::RelationalOperationBooleanOp(b) => null_tolerant(&b.left)? || null_tolerant(&b.right)?,
        RelationalOperation::RelationalOperationComparison(c) => null_tolerant(&c.left)? || null_tolerant(&c.right)?,
        RelationalOperation::RelationalOperationGroup(g) => null_tolerant(&g.inner)?,
        // Java's `default -> false` (:1049; ASK 12, ported as Java has it): every other operation is classed
        // null-REJECTING, an IsNotNull, an ArrayLiteral, a JoinNavigation and a Lambda included
        RelationalOperation::RelationalOperationColumnRef(_)
        | RelationalOperation::RelationalOperationTargetColumnRef(_)
        | RelationalOperation::RelationalOperationLiteral(_)
        | RelationalOperation::RelationalOperationIsNotNull(_)
        | RelationalOperation::RelationalOperationArrayLiteral(_)
        | RelationalOperation::RelationalOperationJoinNavigation(_)
        | RelationalOperation::RelationalOperationLambda(_)
        | RelationalOperation::RelationalOperationLambdaParam(_) => false,
    })
}

/// `JoinChainEmission.targetColumnNames` (:1063): a hop target's column names (a view's declared columns, a table's
/// columns), in declaration order; loud for an unknown table.
#[allow(clippy::too_many_arguments)]
pub fn target_column_names(
    db_fqn: &JStr,
    target_table: &JStr,
    view_target: Option<&JStr>,
    model: &ModelIndex,
    join_name: &JStr,
    prop_name: Option<&JStr>,
    md: Option<&ResolvedMapping<'_>>,
) -> Result<LinkedSet<JStr>, Thrown> {
    let mut out = LinkedSet::new();
    if let Some(view_target) = view_target {
        if let Some(v) = model.find_view(db_fqn, view_target) {
            for c in v.column_mappings.iter() {
                out.add(c.name.clone());
            }
        }
        return Ok(out);
    }
    let Some(t) = model.knowledge().table(Some(db_fqn), Some(target_table)) else {
        return Err(Thrown::model(
            Phase::Normalize,
            cat!(
                "Join '",
                join_name,
                "' target table '",
                target_table,
                "' not found in db '",
                db_fqn,
                "'; PM='",
                value_of(prop_name),
                "', ",
                view_relation::owner(md, db_fqn)
            ),
            None,
        ));
    };
    for c in t.columns.iter() {
        out.add(c.name.clone());
    }
    Ok(out)
}

// ---- the protocol records' convenience constructors Java's emission calls, and two JDK spellings ----

/// `new Variable(name)`.
pub(crate) fn variable(name: &str) -> Arc<p::Variable> {
    Arc::new(p::Variable {
        name: JStr::from(name),
        r#type: None,
        multiplicity: None,
        pos: None,
    })
}

/// `new LambdaFunction(parameters, body)` (no position).
pub(crate) fn lambda(parameters: Vec<Arc<p::Variable>>, body: Vec<V>) -> Arc<p::LambdaFunction> {
    Arc::new(p::LambdaFunction {
        parameters: parameters.into(),
        body: body.into(),
        pos: None,
    })
}

/// `new AppliedProperty(receiver, property)` (no position, no owner class).
pub(crate) fn property(receiver: V, name: &JStr) -> V {
    V::AppliedProperty(Arc::new(p::AppliedProperty {
        receiver,
        property: name.clone(),
        pos: None,
        owner_class: None,
    }))
}

/// `new ColSpec(name, function1, function2[, alias])`: no arguments, unqualified, no position, type or annotations.
pub(crate) fn col_spec(
    name: &JStr,
    function1: Option<Arc<p::LambdaFunction>>,
    function2: Option<Arc<p::LambdaFunction>>,
    alias: Option<&JStr>,
) -> Arc<p::ColSpec> {
    Arc::new(p::ColSpec {
        name: name.clone(),
        function1,
        function2,
        alias: alias.cloned(),
        args: Arc::default(),
        qualified: false,
        pos: None,
        col_type: None,
        col_type_mult: None,
        stereotypes: Arc::default(),
        tagged_values: Arc::default(),
    })
}

/// A column spec as a value.
pub(crate) fn col(cs: Arc<p::ColSpec>) -> V {
    V::ColumnInstance(p::ColumnInstance::ColSpec(cs))
}

/// `new ColSpecArray(colSpecs)` (no position).
pub(crate) fn col_spec_array(col_specs: Vec<Arc<p::ColSpec>>) -> V {
    V::ColumnInstance(p::ColumnInstance::ColSpecArray(Arc::new(p::ColSpecArray {
        col_specs: col_specs.into(),
        pos: None,
    })))
}

/// A built call as a value.
pub(crate) fn call(af: p::AppliedFunction) -> V {
    V::AppliedFunction(Arc::new(af))
}

/// `Calls.exact(fqn, args)` of a `Pure.Lite` identity, as a value.
fn exact(fqn: &str, args: Vec<V>) -> V {
    exact_named(&JStr::from(fqn), args)
}

/// `Calls.exact(fqn, args)`, as a value.
fn exact_named(fqn: &JStr, args: Vec<V>) -> V {
    call(calls::exact(fqn, args))
}

/// `new PackageableElementPtr(fullPath)` (no position).
fn element_ptr(full_path: &JStr) -> V {
    V::PackageableElementPtr(Arc::new(p::PackageableElementPtr {
        full_path: full_path.clone(),
        pos: None,
    }))
}

/// `new PureCollection(values)` (no position).
fn collection(values: Vec<V>) -> V {
    V::PureCollection(Arc::new(p::PureCollection {
        values: values.into(),
        pos: None,
    }))
}

/// `Calls.platform("tableReference", [new PackageableElementPtr(db), new CString(table)])`.
fn table_reference(db: &JStr, table: &JStr) -> V {
    let ptr = element_ptr(db);
    let name = V::CString(Arc::new(p::CString {
        value: table.clone(),
        pos: None,
        multi_line: false,
    }));
    call(calls::platform(&JStr::from("tableReference"), vec![ptr, name]))
}

/// `String.valueOf(s)` of a nullable string: `"null"` for none.
fn value_of(s: Option<&JStr>) -> JStr {
    s.cloned().unwrap_or_else(|| JStr::from("null"))
}

/// A property mapping's Java class's `getSimpleName()` (JCE:195, GBS:164): the nested record's name.
pub(crate) fn pm_simple_name(pm: &PropertyMapping) -> &'static str {
    match pm {
        PropertyMapping::PropertyMappingEnumeratedExpression(_) => "EnumeratedExpression",
        PropertyMapping::PropertyMappingColumn(_) => "Column",
        PropertyMapping::PropertyMappingEnumeratedColumn(_) => "EnumeratedColumn",
        PropertyMapping::PropertyMappingJoin(_) => "Join",
        PropertyMapping::PropertyMappingJoinTerminalColumn(_) => "JoinTerminalColumn",
        PropertyMapping::PropertyMappingExpression(_) => "Expression",
        PropertyMapping::PropertyMappingEmbedded(_) => "Embedded",
        PropertyMapping::PropertyMappingInlineEmbedded(_) => "InlineEmbedded",
        PropertyMapping::PropertyMappingOtherwiseEmbedded(_) => "OtherwiseEmbedded",
        PropertyMapping::PropertyMappingLocalProperty(_) => "LocalProperty",
    }
}

#[cfg(test)]
mod tests {
    use std::collections::{HashMap, HashSet};
    use std::rc::Rc;

    use lite_model::linked::LinkedMap;
    use lite_model::text::j;
    use lite_model::{
        LegacyMappingDefinition, PropertyMappingInlineEmbedded, PropertyMappingJoin, RelationalOperationColumnRef,
        RelationalOperationComparison, RelationalOperationFunctionCall, RelationalOperationIsNull,
        RelationalOperationJoinNavigation, RelationalOperationLiteral,
    };

    use super::*;
    use crate::lifted_views::LiftedViews;
    use crate::mapping_closures::MappingClosures;
    use crate::test_support::{index, mapping, model};

    const MODEL: &str = r#"
###Pure
Class a::Firm
{
  name: String[1];
}
Class a::Sub extends a::Firm
{
}
Class a::Person
{
  name: String[1];
  firm: a::Firm[0..1];
  firms: a::Firm[*];
  rows: a::Firm[*];
  firmName: String[0..1];
}

###Relational
Database a::db
(
  Table PERSON (ID INTEGER PRIMARY KEY, NAME VARCHAR(200), FIRM_ID INTEGER, FIRM VARCHAR(20))
  Table FIRM (ID INTEGER PRIMARY KEY, NAME VARCHAR(200))
  View FV (FID: FIRM.ID, FNAME: FIRM.NAME)
  Join PersonFirm(PERSON.FIRM_ID = FIRM.ID)
)

###Mapping
Mapping a::M
(
  a::Firm[f]: Relational
  {
    ~mainTable [a::db]FIRM
    name: [a::db]FIRM.NAME
  }
  a::Person[p]: Relational
  {
    ~mainTable [a::db]PERSON
    name: [a::db]PERSON.NAME,
    firm[f]: [a::db]@PersonFirm,
    firmName: concat([a::db]@PersonFirm | FIRM.NAME, 'x')
  }
)
"#;

    fn resolved<'a>(
        md: Arc<LegacyMappingDefinition>,
        closures: &'a MappingClosures<'a>,
        views: &'a LiftedViews<'a>,
    ) -> ResolvedMapping<'a> {
        let surface = closures.surface(&md);
        let closure = closures.closure(&surface.qualified_name);
        ResolvedMapping::new(
            surface.clone(),
            surface,
            Rc::new(HashMap::new()),
            Rc::new(LinkedMap::new()),
            closure,
            views,
        )
    }

    fn text(s: &str) -> V {
        V::CString(Arc::new(p::CString {
            value: j(s),
            pos: None,
            multi_line: false,
        }))
    }

    fn hop(name: &str) -> Arc<JoinChainElement> {
        Arc::new(JoinChainElement {
            join_name: j(name),
            join_type: None,
            database_name: None,
            include_self: false,
        })
    }

    fn column(table: &str, col: &str) -> RelationalOperation {
        RelationalOperation::RelationalOperationColumnRef(Arc::new(RelationalOperationColumnRef {
            database_name: None,
            table: j(table),
            column: j(col),
        }))
    }

    fn fcall(name: &str, args: Vec<RelationalOperation>) -> RelationalOperation {
        RelationalOperation::RelationalOperationFunctionCall(Arc::new(RelationalOperationFunctionCall {
            name: j(name),
            args: args.into(),
        }))
    }

    fn nav(chain: &[&str], db: Option<&str>, terminal: Option<RelationalOperation>) -> RelationalOperation {
        RelationalOperation::RelationalOperationJoinNavigation(Arc::new(RelationalOperationJoinNavigation {
            database_name: db.map(j),
            chain: chain.iter().map(|h| hop(h)).collect(),
            terminal,
        }))
    }

    fn chain_names(specs: &[JoinNavSpec]) -> Vec<Vec<JStr>> {
        specs.iter().map(|s| join_path(&s.chain)).collect()
    }

    #[test]
    fn a_nav_slot_is_minted_past_columns_accessors_and_taken_slots_once() {
        let parsed = model(MODEL);
        let idx = index(&parsed);
        let views = LiftedViews::new(&parsed, &idx);
        let mut ledger = MappingLedger::new(HashSet::new());
        let mut p = Pipeline::new(text("src"), &mut ledger, &views);
        let (db, person) = (j("a::db"), j("PERSON"));
        // PERSON has a FIRM column: the fold ignores case
        assert_eq!(
            mint_nav_slot_alias(&mut p, &idx, &db, &person, &j("firm")),
            j("firm_nav")
        );
        // first mint wins
        assert_eq!(
            mint_nav_slot_alias(&mut p, &idx, &db, &person, &j("firm")),
            j("firm_nav")
        );
        // the relation accessors
        assert_eq!(
            mint_nav_slot_alias(&mut p, &idx, &db, &person, &j("columns")),
            j("columns_nav")
        );
        assert_eq!(
            mint_nav_slot_alias(&mut p, &idx, &db, &person, &j("rows")),
            j("rows_nav")
        );
        // a slot already on the pipeline (a physical chain's join named like the property) pushes it on
        p.alias_to_target_table.put(j("firms"), j("FIRM"));
        p.alias_to_target_table.put(j("firms_"), j("FIRM"));
        assert_eq!(
            mint_nav_slot_alias(&mut p, &idx, &db, &person, &j("firms")),
            j("firms__")
        );
        // a schema-qualified main table asks its default-schema namesake
        assert_eq!(
            mint_nav_slot_alias(&mut p, &idx, &db, &j("S.PERSON"), &j("name")),
            j("name_nav")
        );
        assert_eq!(
            mint_nav_slot_alias(&mut p, &idx, &db, &j("FIRM"), &j("other")),
            j("other")
        );
        assert_eq!(p.nav_slot_by_prop.get(&j("firms")), Some(&j("firms__")));
        assert_eq!(nav_slot_for(&p, &j("firm")).unwrap(), j("firm_nav"));
        let miss = nav_slot_for(&p, &j("nope")).unwrap_err();
        assert!(miss.is_not_implemented());
        assert_eq!(
            miss.message(),
            Some(j(
                "class-typed property 'nope' has no navigate slot on this pipeline \u{2014} its join was never emitted"
            ))
        );
    }

    #[test]
    fn a_physical_slot_is_its_path_then_numbered_and_found_by_its_path() {
        let parsed = model(MODEL);
        let idx = index(&parsed);
        let views = LiftedViews::new(&parsed, &idx);
        let mut ledger = MappingLedger::new(HashSet::new());
        let mut p = Pipeline::new(text("src"), &mut ledger, &views);
        let path = [j("A"), j("B")];
        assert_eq!(unique_slot_name(&p, Some(&path)).unwrap(), j("A__B"));
        p.alias_to_target_table.put(j("A__B"), j("T"));
        assert_eq!(unique_slot_name(&p, Some(&path)).unwrap(), j("A__B__2"));
        p.alias_to_target_table.put(j("A__B__2"), j("T"));
        assert_eq!(unique_slot_name(&p, Some(&path)).unwrap(), j("A__B__3"));
        assert_eq!(unique_slot_name(&p, None).unwrap_err(), Thrown::null_pointer(None));
        let hops = vec![hop("A"), hop("B")];
        assert_eq!(join_path(&hops), path.to_vec());
        p.path_to_slot.put(path.to_vec(), j("A__B__2"));
        assert_eq!(slot_for(&p, &hops).unwrap(), j("A__B__2"));
        let miss = slot_for(&p, &hops[..1]).unwrap_err();
        assert!(miss.is_not_implemented());
        assert_eq!(
            miss.message(),
            Some(j(
                "join chain [A] was never emitted on this pipeline \u{2014} the expression navigates a join that was \
                 not hoisted as a slot"
            ))
        );
        // routes are asked only for a class-typed chain of two hops or more
        assert_eq!(per_arm_hops(&p, Some(&j("firm")), None, &hops).len(), 2);
        assert_eq!(
            per_arm_hops(&p, Some(&j("firm")), Some(&j("a::Firm")), &hops[..1]).len(),
            1
        );
        assert_eq!(per_arm_hops(&p, Some(&j("firm")), Some(&j("a::Firm")), &hops).len(), 2);
        assert_eq!(per_arm_hops(&p, None, Some(&j("a::Firm")), &hops).len(), 2);
    }

    #[test]
    fn the_class_typed_targets_read_the_declared_type_and_the_ledger() {
        let parsed = model(MODEL);
        let idx = index(&parsed);
        let person = j("a::Person");
        assert_eq!(
            class_typed_target(Some(&person), &j("firm"), &idx).unwrap(),
            Some(j("a::Firm"))
        );
        assert_eq!(class_typed_target(Some(&person), &j("name"), &idx).unwrap(), None);
        assert_eq!(class_typed_target(Some(&person), &j("nope"), &idx).unwrap(), None);
        assert_eq!(class_typed_target(Some(&j("a::Nope")), &j("firm"), &idx).unwrap(), None);
        assert_eq!(class_typed_target(None, &j("firm"), &idx).unwrap(), None);
        let firm_mapped = MappingLedger::new(HashSet::from([j("a::Firm")]));
        let sub_mapped = MappingLedger::new(HashSet::from([j("a::Sub")]));
        let none_mapped = MappingLedger::new(HashSet::new());
        let target = |l: &MappingLedger| class_typed_target_if_mapped(Some(&person), &j("firm"), &idx, l).unwrap();
        assert_eq!(target(&firm_mapped), Some(j("a::Firm")));
        // an abstraction of a mapped class: the declared class, a navigation
        assert_eq!(target(&sub_mapped), Some(j("a::Firm")));
        assert_eq!(target(&none_mapped), None);
        assert!(has_mapped_subclass(&j("a::Firm"), &idx, &sub_mapped));
        assert!(!has_mapped_subclass(&j("a::Firm"), &idx, &firm_mapped));
        assert_eq!(
            class_typed_target_if_mapped(Some(&person), &j("name"), &idx, &firm_mapped).unwrap(),
            None
        );
    }

    #[test]
    fn a_nav_slot_owner_is_recorded_for_a_class_typed_join_first_wins() {
        let parsed = model(MODEL);
        let idx = index(&parsed);
        let views = LiftedViews::new(&parsed, &idx);
        let mut ledger = MappingLedger::new(HashSet::new());
        let mut p = Pipeline::new(text("src"), &mut ledger, &views);
        let join = |prop: &str| {
            PropertyMapping::PropertyMappingJoin(Arc::new(PropertyMappingJoin {
                property_name: j(prop),
                database: j("a::db"),
                joins: vec![hop("PersonFirm")].into(),
                target_set_id: None,
            }))
        };
        record_nav_slot_owner(&mut p, &join("firm"), &j("a::Person"), &idx).unwrap();
        record_nav_slot_owner(&mut p, &join("firm"), &j("a::Other"), &idx).unwrap();
        record_nav_slot_owner(&mut p, &join("name"), &j("a::Person"), &idx).unwrap();
        assert_eq!(p.nav_slot_owner.get(&j("firm")), Some(&j("a::Person")));
        assert_eq!(p.nav_slot_owner.len(), 1);
    }

    #[test]
    fn join_navigations_are_collected_in_reading_order_through_pms_and_inline_splices() {
        let parsed = model(MODEL);
        let idx = index(&parsed);
        let closures = MappingClosures::new(&idx);
        let views = LiftedViews::new(&parsed, &idx);
        let md = resolved(mapping(&parsed, "a::M"), &closures, &views);
        let cmp = RelationalOperation::RelationalOperationComparison(Arc::new(RelationalOperationComparison {
            left: nav(&["A"], Some("a::db"), Some(nav(&["B", "C"], None, None))),
            op: lite_model::ComparisonOp::EQ,
            right: fcall(
                "concat",
                vec![column("T", "X"), nav(&["D"], None, Some(column("T", "Y")))],
            ),
        }));
        let mut out = Vec::new();
        collect_join_navigations(&cmp, &mut out);
        assert_eq!(
            chain_names(&out),
            vec![vec![j("A")], vec![j("B"), j("C")], vec![j("D")]]
        );
        assert_eq!(out[0].chain_db, Some(j("a::db")));
        assert_eq!(out[1].chain_db, None);
        // an Inline splice carries its referenced set's navigations only with the mapping in hand
        let inline = PropertyMapping::PropertyMappingInlineEmbedded(Arc::new(PropertyMappingInlineEmbedded {
            property_name: j("x"),
            set_id: j("p"),
        }));
        let expr = PropertyMapping::PropertyMappingExpression(Arc::new(lite_model::PropertyMappingExpression {
            property_name: j("y"),
            expression: nav(&["E"], None, None),
        }));
        let pms = vec![inline, expr];
        let mut bare = Vec::new();
        collect_join_navigations_in_pms(&pms, &mut bare);
        assert_eq!(chain_names(&bare), vec![vec![j("E")]]);
        let mut spliced = Vec::new();
        collect_join_navigations_in_pms_of(&pms, &mut spliced, Some(&md));
        // the referenced set `p`: its Join PM is pass 1's; its expression's navigation splices in, before `E`
        assert_eq!(chain_names(&spliced), vec![vec![j("PersonFirm")], vec![j("E")]]);
    }

    #[test]
    fn null_tolerance_is_classified_as_java_does() {
        let is_null = RelationalOperation::RelationalOperationIsNull(Arc::new(RelationalOperationIsNull {
            operand: column("T", "X"),
        }));
        let literal = RelationalOperation::RelationalOperationLiteral(Arc::new(RelationalOperationLiteral {
            value: lite_model::JValue::Long(1),
        }));
        let eq = |l: RelationalOperation, r: RelationalOperation| {
            RelationalOperation::RelationalOperationComparison(Arc::new(RelationalOperationComparison {
                left: l,
                op: lite_model::ComparisonOp::EQ,
                right: r,
            }))
        };
        assert!(null_tolerant(&is_null).unwrap());
        assert!(!null_tolerant(&eq(column("T", "X"), literal.clone())).unwrap());
        assert!(null_tolerant(&eq(column("T", "X"), is_null.clone())).unwrap());
        assert!(null_tolerant(&fcall("isNull", vec![column("T", "X")])).unwrap());
        assert!(null_tolerant(&fcall("sqlNull", vec![])).unwrap());
        assert!(!null_tolerant(&fcall("upper", vec![column("T", "X")])).unwrap());
        // the arguments are read left to right and the first tolerant one answers: a later coalesce is never seen
        assert!(null_tolerant(&fcall("concat", vec![is_null.clone(), fcall("coalesce", vec![])])).unwrap());
        let e = null_tolerant(&fcall("concat", vec![literal.clone(), fcall("coalesce", vec![])])).unwrap_err();
        assert!(e.is_not_implemented());
        assert_eq!(
            e.message(),
            Some(j(
                "(INNER) mapping-filter condition uses 'coalesce' \u{2014} null-tolerance cannot be classified; not \
                 supported yet"
            ))
        );
        for name in ["case", "if"] {
            assert!(null_tolerant(&fcall(name, vec![])).unwrap_err().is_not_implemented());
        }
        // `default -> false`: an IsNotNull and a navigation are classed null-rejecting
        let is_not_null =
            RelationalOperation::RelationalOperationIsNotNull(Arc::new(lite_model::RelationalOperationIsNotNull {
                operand: is_null.clone(),
            }));
        assert!(!null_tolerant(&is_not_null).unwrap());
        assert!(!null_tolerant(&nav(&["A"], None, Some(is_null))).unwrap());
    }

    #[test]
    fn a_hop_targets_columns_are_the_tables_or_the_views_in_declaration_order() {
        let parsed = model(MODEL);
        let idx = index(&parsed);
        let db = j("a::db");
        let names = |t: &str, v: Option<&str>| {
            target_column_names(&db, &j(t), v.map(j).as_ref(), &idx, &j("J"), None, None)
                .unwrap()
                .to_vec()
        };
        assert_eq!(names("PERSON", None), vec![j("ID"), j("NAME"), j("FIRM_ID"), j("FIRM")]);
        assert_eq!(names("FV", Some("FV")), vec![j("FID"), j("FNAME")]);
        // a view target the database has not: no columns, no refusal
        assert!(names("NOPE", Some("NOPE")).is_empty());
    }

    fn join_pm(prop: &str, target: Option<&str>) -> PropertyMapping {
        PropertyMapping::PropertyMappingJoin(Arc::new(PropertyMappingJoin {
            property_name: j(prop),
            database: j("a::db"),
            joins: vec![hop("PersonFirm")].into(),
            target_set_id: target.map(j),
        }))
    }

    fn column_pm(prop: &str) -> PropertyMapping {
        PropertyMapping::PropertyMappingColumn(Arc::new(lite_model::PropertyMappingColumn {
            property_name: j(prop),
            database: j("a::db"),
            table: j("PERSON"),
            column: j("NAME"),
        }))
    }

    fn otherwise(prop: &str, fallback: PropertyMapping) -> Arc<PropertyMappingOtherwiseEmbedded> {
        Arc::new(PropertyMappingOtherwiseEmbedded {
            property_name: j(prop),
            embedded: vec![column_pm("name")].into(),
            fallback_set_id: j("f"),
            fallback,
        })
    }

    #[test]
    fn pass_one_skips_a_dropped_route_and_dedups_a_hoisted_hop() {
        let parsed = model(MODEL);
        let idx = index(&parsed);
        let closures = MappingClosures::new(&idx);
        let views = LiftedViews::new(&parsed, &idx);
        let md = resolved(mapping(&parsed, "a::M"), &closures, &views);
        let mut ledger = MappingLedger::new(HashSet::from([j("a::Firm")]));
        let mut p = Pipeline::new(text("src"), &mut ledger, &views);
        let (person, db, table, row) = (j("a::Person"), j("a::db"), j("PERSON"), variable("row"));
        let emit = |p: &mut Pipeline<'_>, pm: &PropertyMapping| {
            emit_hops_for_structural_pm(p, pm, &person, &db, &table, &row, &idx, &md)
        };
        // a routed property dropped at classification, and the PMs pass 2 reads: nothing
        p.dropped_routed_props.insert(j("firm"));
        emit(&mut p, &join_pm("firm", Some("f"))).unwrap();
        emit(&mut p, &column_pm("name")).unwrap();
        assert!(p.alias_to_target_table.is_empty() && p.nav_slot_by_prop.is_empty());
        assert_eq!(p.expr, text("src"));
        // a class-typed hop already on the pipeline (its minted alias taken): the hop dedups, nothing is emitted
        p.dropped_routed_props.clear();
        p.nav_slot_by_prop.put(j("firm"), j("firm_nav"));
        p.alias_to_target_table.put(j("firm_nav"), j("FIRM"));
        emit(&mut p, &join_pm("firm", None)).unwrap();
        // a physical chain already hoisted under its path
        p.path_to_slot.put(vec![j("PersonFirm")], j("PersonFirm"));
        p.alias_to_target_table.put(j("PersonFirm"), j("FIRM"));
        let jtc = PropertyMapping::PropertyMappingJoinTerminalColumn(Arc::new(
            lite_model::PropertyMappingJoinTerminalColumn {
                property_name: j("firmName"),
                database: j("a::db"),
                joins: vec![hop("PersonFirm")].into(),
                terminal_column: column("FIRM", "NAME"),
                enum_mapping_id: None,
                enum_mapped: false,
            },
        ));
        emit(&mut p, &jtc).unwrap();
        assert_eq!(p.expr, text("src"));
        assert_eq!(p.alias_to_target_table.len(), 2);
        assert!(p.class_slots.is_empty());
        // a class-typed chain has a mapping
        let e = emit_join_chain(
            &mut p,
            &[hop("PersonFirm")],
            None,
            Some(&j("firm")),
            Some(&person),
            &db,
            &table,
            &row,
            &idx,
            None,
            true,
        )
        .unwrap_err();
        assert_eq!(
            e,
            Thrown::null_pointer(Some("class-typed join chain without its mapping"))
        );
    }

    #[test]
    fn an_embedded_class_typed_sub_pm_on_another_owners_slot_is_loud() {
        let parsed = model(MODEL);
        let idx = index(&parsed);
        let closures = MappingClosures::new(&idx);
        let views = LiftedViews::new(&parsed, &idx);
        let md = resolved(mapping(&parsed, "a::M"), &closures, &views);
        let mut ledger = MappingLedger::new(HashSet::from([j("a::Firm")]));
        let mut p = Pipeline::new(text("src"), &mut ledger, &views);
        let (db, table, row) = (j("a::db"), j("PERSON"), variable("row"));
        // `firm` embedded: its sub-PMs belong to the embedded class, `a::Firm`
        let embedded = PropertyMapping::PropertyMappingEmbedded(Arc::new(lite_model::PropertyMappingEmbedded {
            property_name: j("firm"),
            property_mappings: vec![column_pm("name")].into(),
            primary_key: Arc::default(),
        }));
        // an owner the model has not: nothing hoists (the funnel's known miss)
        emit_hops_for_structural_pm(&mut p, &embedded, &j("a::Nope"), &db, &table, &row, &idx, &md).unwrap();
        // a Column sub-PM under a known owner: nothing to hoist, no owner recorded
        emit_hops_for_structural_pm(&mut p, &embedded, &j("a::Person"), &db, &table, &row, &idx, &md).unwrap();
        assert!(p.nav_slot_owner.is_empty() && p.alias_to_target_table.is_empty());
        // the guard: a class-typed, mapped Join sub-PM (`Person.firm`) whose slot another owner holds
        p.alias_to_target_table.put(j("firm"), j("FIRM"));
        p.nav_slot_owner.put(j("firm"), j("a::Other"));
        let e = hoist_sub_pms(
            &mut p,
            &[join_pm("firm", None)],
            &j("a::Person"),
            &db,
            &table,
            &row,
            &idx,
            &md,
            |prop, mapping| cat!("Embedded sub-PM '", prop, "' collides; Mapping=", mapping),
        )
        .unwrap_err();
        assert!(e.is_not_implemented());
        assert_eq!(e.message(), Some(j("Embedded sub-PM 'firm' collides; Mapping=a::M")));
        // the same owner's slot is a routed sibling, never a clash: it passes the guard (and its hop dedups)
        p.nav_slot_owner.put(j("firm"), j("a::Person"));
        p.nav_slot_by_prop.put(j("firm"), j("firm"));
        hoist_sub_pms(
            &mut p,
            &[join_pm("firm", None)],
            &j("a::Person"),
            &db,
            &table,
            &row,
            &idx,
            &md,
            |prop, _| cat!("unexpected: ", prop),
        )
        .unwrap();
        assert_eq!(p.expr, text("src"));
    }

    #[test]
    fn an_inline_or_otherwise_block_refuses_what_it_cannot_hoist() {
        let parsed = model(MODEL);
        let idx = index(&parsed);
        let closures = MappingClosures::new(&idx);
        let views = LiftedViews::new(&parsed, &idx);
        let md = resolved(mapping(&parsed, "a::M"), &closures, &views);
        let mut ledger = MappingLedger::new(HashSet::from([j("a::Person")]));
        let mut p = Pipeline::new(text("src"), &mut ledger, &views);
        let (db, table, row) = (j("a::db"), j("PERSON"), variable("row"));
        let inline = PropertyMapping::PropertyMappingInlineEmbedded(Arc::new(PropertyMappingInlineEmbedded {
            property_name: j("firm"),
            set_id: j("f"),
        }));
        let e = emit_hops_for_structural_pm(&mut p, &inline, &j("a::Nope"), &db, &table, &row, &idx, &md).unwrap_err();
        assert!(e.is_model());
        assert_eq!(e.phase(), Some(Phase::Normalize));
        assert_eq!(
            e.message(),
            Some(j(
                "Inline-embedded PM 'firm': unknown owner class 'a::Nope'; mapping=a::M"
            ))
        );
        let mut otherwise_err = |oe: Arc<PropertyMappingOtherwiseEmbedded>, owner: &str| {
            emit_otherwise_embedded_hop(&mut p, &oe, &j(owner), &db, &table, &row, &idx, &md).unwrap_err()
        };
        let e = otherwise_err(otherwise("firm", column_pm("firm")), "a::Person");
        assert!(e.is_not_implemented());
        assert_eq!(
            e.message(),
            Some(j(
                "OtherwiseEmbedded PM 'firm' fallback kind Column not supported (Join only). Mapping=a::M"
            ))
        );
        let e = otherwise_err(otherwise("firm", join_pm("firm", None)), "a::Nope");
        assert!(e.is_model());
        assert_eq!(
            e.message(),
            Some(j(
                "OtherwiseEmbedded PM 'firm': unknown owner class 'a::Nope'; mapping=a::M"
            ))
        );
        // a primitive's type is a name too: not "non-class", but a class no closure maps
        let e = otherwise_err(otherwise("name", join_pm("name", None)), "a::Person");
        assert!(e.is_not_implemented(), "{e:?}");
        assert_eq!(
            e.message(),
            Some(j(
                "OtherwiseEmbedded PM 'name' target class 'meta::pure::metamodel::type::String' is not mapped; mapping=a::M"
            ))
        );
        // a property the class has not: no type
        let e = otherwise_err(otherwise("nope", join_pm("nope", None)), "a::Person");
        assert!(e.is_model());
        assert_eq!(
            e.message(),
            Some(j(
                "OtherwiseEmbedded PM 'nope' has non-class property type; mapping=a::M"
            ))
        );
        // the ledger maps Person only
        let e = otherwise_err(otherwise("firm", join_pm("firm", None)), "a::Person");
        assert!(e.is_not_implemented());
        assert_eq!(
            e.message(),
            Some(j(
                "OtherwiseEmbedded PM 'firm' target class 'a::Firm' is not mapped; mapping=a::M"
            ))
        );
        // through pass 1, the fallback's refusal comes first
        let wrapped = PropertyMapping::PropertyMappingOtherwiseEmbedded(otherwise("firm", column_pm("firm")));
        let e =
            emit_hops_for_structural_pm(&mut p, &wrapped, &j("a::Person"), &db, &table, &row, &idx, &md).unwrap_err();
        assert!(e.is_not_implemented());
        assert_eq!(pm_simple_name(&inline), "InlineEmbedded");
    }
}
