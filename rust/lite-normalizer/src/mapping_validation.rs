// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! `MappingValidation` (MappingValidation.java at ad86fa895): the invalid sets of a mapping BY SET ID, in declaration
//! order, each with the engine's compile-time rejection -- recorded, never thrown here (the driver applies strict /
//! module); a mapping-level error (a duplicated set id across the include closure) throws in both builds. The per-set
//! `catch (ModelException)` keeps the exception (its full message becomes a poison). The switch's `default -> {}`
//! is spelled as the two no-op kinds (census §1).

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use lite_builtin::natives::{
    AND__BOOLEAN_1__BOOLEAN_1, EQUAL__ANY_MANY__ANY_MANY, NOT__BOOLEAN_1, OR__BOOLEAN_1__BOOLEAN_1,
};
use lite_builtin::pure::NativeId;
use lite_error::Phase;
use lite_front::records as p;
use lite_model::linked::LinkedMap;
use lite_model::{
    AssociationDefinition, AssociationDefinitionAssociationEndDefinition, AssociationMappingCross,
    AssociationMappingModelJoin, ClassDefinition, ClassMapping, ClassMappingPure, ClassMappingRelational,
    ClassMappingUnion, JStr, JoinChainElement, LegacyMappingDefinition, PropertyMapping, RelationalOperation, cat,
};
use lite_names::model_index::java_list;
use lite_names::{MilestoningStrategy, ModelIndex};

use crate::error::Thrown;
use crate::m2m_route_guards;
use crate::mapping_normalizer;
use crate::model_join_nesting;
use crate::resolved_mapping::ResolvedMapping;

/// `MappingValidation.run` (:43): set id -> its `ModelException`, declaration order.
pub fn run(r: &ResolvedMapping<'_>, model: &ModelIndex) -> Result<LinkedMap<JStr, Thrown>, Thrown> {
    let md = r.raw();
    // MAPPING-level errors the engine's compiler raises (R5): a class mapping id taken by two distinct sets across the
    // include closure. Thrown in BOTH builds -- a module build walls the whole mapping through the pre-pass's own catch
    let closure = r.closures().closure(&md.qualified_name);
    let dup_ids = closure.duplicate_ids()?;
    if !dup_ids.is_empty() {
        return Err(Thrown::model(
            Phase::Model,
            cat!(
                "Duplicated class mappings found with ID ",
                java_list(&dup_ids),
                " in mapping '",
                &md.qualified_name,
                "'"
            ),
            Some(&md.qualified_name),
        ));
    }
    // an enumeration mapping id taken twice across the closure, in the engine's words, checked next as the engine checks
    // it (MappingValidator.collectAndValidateEnumerationMappingIds; 2026-11-15, the normalizer batch, item 2, ASK 13)
    if let Some(dup) = closure.duplicate_enumeration_mapping_id()? {
        return Err(Thrown::model(
            Phase::Model,
            cat!(
                "Duplicated enumeration mappings found with ID '",
                &dup,
                "' in mapping '",
                &md.qualified_name,
                "'"
            ),
            Some(&md.qualified_name),
        ));
    }
    let mut invalid: LinkedMap<JStr, Thrown> = LinkedMap::new();
    for cm in md.class_mappings.iter() {
        let checked = match cm {
            ClassMapping::ClassMappingRelational(rcm) => {
                validate_pm_names(rcm, model, md).and_then(|()| validate_join_sources(rcm, model))
            }
            ClassMapping::ClassMappingPure(pcm) => validate_m2m_routes(pcm, model, r),
            ClassMapping::ClassMappingUnion(u) => validate_union_members(u, r),
            // Java's `default -> { }`: the two kinds nothing here validates
            ClassMapping::ClassMappingInheritance(_) | ClassMapping::ClassMappingRelationFunction(_) => Ok(()),
        };
        match checked {
            Ok(()) => {}
            // `catch (ModelException e)`: the exception kept, by the set's id
            Err(e) if e.is_model() => {
                invalid.put(ResolvedMapping::id_of(cm), e);
            }
            Err(e) => return Err(e),
        }
    }
    Ok(invalid)
}

/// `MappingValidation.validateUnionMembers` (:76): every member set is a class mapping of the mapping or its
/// includes ("Can't find class mapping").
pub fn validate_union_members(u: &ClassMappingUnion, r: &ResolvedMapping<'_>) -> Result<(), Thrown> {
    let mut ids: HashSet<JStr> = r.included_sets()?.iter().map(|(id, _)| id.clone()).collect();
    for cm in r.raw().class_mappings.iter() {
        ids.insert(ResolvedMapping::id_of(cm));
    }
    for member in u.member_set_ids.iter() {
        if !ids.contains(member) {
            return Err(Thrown::model(
                Phase::Mapping,
                cat!(
                    "Can't find class mapping '",
                    member,
                    "' in mapping '",
                    &r.raw().qualified_name,
                    "'"
                ),
                Some(&r.raw().qualified_name),
            ));
        }
    }
    Ok(())
}

/// `MappingValidation.validatePmNames` (:91): every PM names a property its class declares or inherits.
pub fn validate_pm_names(
    rcm: &ClassMappingRelational,
    model: &ModelIndex,
    md: &LegacyMappingDefinition,
) -> Result<(), Thrown> {
    let knowledge = model.knowledge();
    // `MissProbe.knownMiss`: a set of a class the model has not skips the check
    let Some(cd) = knowledge.hierarchy_class(Some(&rcm.class_name)) else {
        return Ok(());
    };
    for pm in rcm.property_mappings.iter() {
        if matches!(pm, PropertyMapping::PropertyMappingLocalProperty(_)) {
            continue;
        }
        if knowledge.property_type(Some(cd), pm.property_name())?.is_none() {
            return Err(Thrown::model(
                Phase::Model,
                cat!(
                    "PropertyMapping '",
                    pm.property_name(),
                    "' references property not declared on class '",
                    &rcm.class_name,
                    "'; mapping=",
                    &md.qualified_name
                ),
                None,
            ));
        }
    }
    Ok(())
}

/// `MappingValidation.joinNotFound`: a join its database has not, in legend-engine's words, the database by its own
/// name (`HelperRelationalBuilder.getJoin`, :190; asked of a view's join and of a mapping's: "Can't find join 'Nope' in
/// database 'Db'"). Until 2026-11-15 lite's own, "Join 'J' not found in db 'D'; PM='p', ..." and "AssociationMapping join
/// 'J' not found in db 'D'; ..." (a view's: the normalizer batch, item 5; a mapping's: the normalizer's second batch,
/// item 4 (a); register row S376).
pub fn join_not_found(join_name: &JStr, db: &JStr) -> Thrown {
    Thrown::model(
        Phase::Normalize,
        cat!(
            "Can't find join '",
            join_name,
            "' in database '",
            &lite_names::view_signatures::db_name(db),
            "'"
        ),
        None,
    )
}

/// `MappingValidation.validateJoinSources`: every join chain a property mapping walks departs from the table it is
/// at -- the first join contains the set's main table, each next join the table the previous one lands on
/// (legend-engine 4.145.0 `HelperRelationalBuilder.validatePropertyMappings`, its words: "Mapping error: the join J does
/// not contain the source table T", the table by its own name). Since 2026-11-15 (the normalizer's second batch, item
/// 1, JCE:843); a join the database lacks, a set with no main table and an ambiguous landing are left to the refusals
/// that already name them.
pub fn validate_join_sources(rcm: &ClassMappingRelational, model: &ModelIndex) -> Result<(), Thrown> {
    let main = match &rcm.main_table {
        Some(m) => Some(m.clone()),
        None => mapping_normalizer::infer_main_table_quiet(rcm)?,
    };
    let Some(main) = main else {
        return Ok(());
    };
    if rcm.source_url.is_some() {
        return Ok(());
    }
    let start = mapping_normalizer::canonical_table(&main.table);
    for pm in rcm.property_mappings.iter() {
        validate_pm_join_sources(pm, &start, model)?;
    }
    Ok(())
}

fn validate_pm_join_sources(pm: &PropertyMapping, start: &JStr, model: &ModelIndex) -> Result<(), Thrown> {
    match pm {
        PropertyMapping::PropertyMappingJoin(j) => validate_chain(&j.joins, &j.database, start, model),
        PropertyMapping::PropertyMappingJoinTerminalColumn(j) => validate_chain(&j.joins, &j.database, start, model),
        PropertyMapping::PropertyMappingExpression(e) => validate_navigations(&e.expression, start, model),
        PropertyMapping::PropertyMappingEnumeratedExpression(e) => validate_navigations(&e.expression, start, model),
        PropertyMapping::PropertyMappingEmbedded(e) => {
            for x in e.property_mappings.iter() {
                validate_pm_join_sources(x, start, model)?;
            }
            Ok(())
        }
        PropertyMapping::PropertyMappingOtherwiseEmbedded(oe) => {
            for x in oe.embedded.iter() {
                validate_pm_join_sources(x, start, model)?;
            }
            validate_pm_join_sources(&oe.fallback, start, model)
        }
        PropertyMapping::PropertyMappingLocalProperty(lp) => validate_pm_join_sources(&lp.body, start, model),
        _ => Ok(()),
    }
}

fn validate_navigations(op: &RelationalOperation, start: &JStr, model: &ModelIndex) -> Result<(), Thrown> {
    for o in op.descendants() {
        if let RelationalOperation::RelationalOperationJoinNavigation(nav) = o
            && let Some(db) = &nav.database_name
        {
            validate_chain(&nav.chain, db, start, model)?;
        }
    }
    Ok(())
}

fn validate_chain(chain: &[Arc<JoinChainElement>], db: &JStr, start: &JStr, model: &ModelIndex) -> Result<(), Thrown> {
    let mut at = start.clone();
    for hop in chain.iter() {
        let hop_db = hop.database_name.as_ref().unwrap_or(db);
        let Some(jd) = model.find_join(Some(hop_db), &hop.join_name) else {
            return Ok(());
        };
        let mut tables: Vec<JStr> = Vec::new();
        for t in jd.operation.tables() {
            let c = mapping_normalizer::canonical_table(&t);
            if !tables.contains(&c) {
                tables.push(c);
            }
        }
        if !tables.contains(&at) {
            return Err(Thrown::model(
                Phase::Mapping,
                cat!(
                    "Mapping error: the join ",
                    &hop.join_name,
                    " does not contain the source table ",
                    &bare_table(&at)
                ),
                None,
            ));
        }
        tables.retain(|t| t != &at);
        if tables.len() > 1 {
            return Ok(());
        }
        if let Some(next) = tables.first() {
            at = next.clone();
        }
    }
    Ok(())
}

/// A table's own name, its schema dropped (the engine prints `Table._name()`).
fn bare_table(table: &JStr) -> JStr {
    match lite_model::text::last_index_of(&table.0, &[u16::from(b'.')]) {
        Some(dot) => JStr::from(&table.0[dot + 1..]),
        None => table.clone(),
    }
}

/// `MappingValidation.validateM2mRoutes` (:107): every routed M2M binding names a benign route.
pub fn validate_m2m_routes(
    pcm: &Arc<ClassMappingPure>,
    model: &ModelIndex,
    md: &ResolvedMapping<'_>,
) -> Result<(), Thrown> {
    // `MissProbe.knownMiss`: none for a class the model has not
    let tgt = model.knowledge().hierarchy_class(Some(&pcm.class_name));
    for pb in pcm.property_bindings.iter() {
        m2m_route_guards::require_benign_route(pb, pcm, tgt, md, model)?;
    }
    Ok(())
}

// ==== legend-engine's compile of an association mapping's lambdas (the normalizer's second batch, item 4 (e)) ====

/// A check that cannot be made exact here: lite's own refusal (or deferral) stands.
const HOLD: &str = "\u{0}hold";

fn hold() -> JStr {
    JStr::from(HOLD)
}

/// `MappingValidation.requireEngineModelJoin`: legend-engine's checks of a ModelJoin association mapping's lambda when
/// the mapping compiles, in its words and order (`HelperMappingBuilder.processModelJoinAssociationMapping`): two
/// parameters; their pairing with the ends ([`engine_pairing`]); the body typed once per direction, the association's
/// second property's class the source first, a shared name the source's -- only where that is exact ([`first_missing`]).
/// Since 2026-11-15 (the normalizer's second batch, item 4 (e), ruling 34).
pub fn require_engine_model_join(
    md: &ResolvedMapping<'_>,
    mj: &AssociationMappingModelJoin,
    ad: &AssociationDefinition,
    model: &ModelIndex,
) -> Result<(), Thrown> {
    let ps = &mj.lambda.parameters;
    if ps.len() != 2 {
        return Err(Thrown::model(
            Phase::Mapping,
            JStr::from(
                "ModelJoin requires a lambda with exactly 2 parameters, e.g. {a:TypeA[1], b:TypeB[1] | $a.prop == \
                 $b.prop} (param types optional)",
            ),
            None,
        ));
    }
    let c1 = model_join_nesting::target_class_fqn(&ad.property1)?;
    let c2 = model_join_nesting::target_class_fqn(&ad.property2)?;
    // a temporal end: the engine names and pairs the milestoned properties, whose order lite does not hold -- held
    if temporal(&c1, model)? || temporal(&c2, model)? {
        return Ok(());
    }
    let pm1 = engine_pairing(&ad.property2, &ad.property1, &ps[0], &ps[1], model)?;
    let pm2 = engine_pairing(&ad.property1, &ad.property2, &ps[0], &ps[1], model)?;
    // the body is typed only over a pair of sets: an end with no instance set compiles nothing
    let (Some(pm1), Some(pm2)) = (pm1, pm2) else {
        return Ok(());
    };
    if mj.lambda.body.len() != 1 || !plain_sets(md, &c1, model)? || !plain_sets(md, &c2, model)? {
        return Ok(());
    }
    let body = &mj.lambda.body[0];
    let none = HashMap::new();
    let mut miss = first_missing(body, &binding(&pm1.1, &c1, &pm1.0, &c2), &none, model)?;
    if miss.is_none() {
        miss = first_missing(body, &binding(&pm2.1, &c2, &pm2.0, &c1), &none, model)?;
    }
    match miss {
        Some(m) if m != hold() => Err(Thrown::model(Phase::Mapping, m, None)),
        _ => Ok(()),
    }
}

/// `MappingValidation.requireEngineXStore`: legend-engine's XStore line compile, in its words
/// (`PropertyMappingBuilder.visit(XStorePropertyMapping)`): each line in order, `$this` the class owning the line's
/// property and `$that` the property's class, reading only properties they have -- only where exact; at the first line
/// it cannot decide, nothing more is checked (since 2026-11-15, the normalizer's second batch, item 4 (e)).
pub fn require_engine_xstore(
    md: &ResolvedMapping<'_>,
    xs: &AssociationMappingCross,
    ad: &AssociationDefinition,
    model: &ModelIndex,
) -> Result<(), Thrown> {
    for line in xs.property_mappings2.iter() {
        let (prop, other) = if line.property_name == ad.property1.property_name {
            (&ad.property1, &ad.property2)
        } else if line.property_name == ad.property2.property_name {
            (&ad.property2, &ad.property1)
        } else {
            return Ok(());
        };
        let this_cls = model_join_nesting::target_class_fqn(other)?;
        let that_cls = model_join_nesting::target_class_fqn(prop)?;
        if line.source_set_id.is_some()
            || line.target_set_id.is_some()
            || temporal(&this_cls, model)?
            || temporal(&that_cls, model)?
        {
            return Ok(());
        }
        let mut bind: HashMap<JStr, JStr> = HashMap::new();
        let mut locals: HashMap<JStr, HashSet<JStr>> = HashMap::new();
        for (v, cls) in [(JStr::from("this"), &this_cls), (JStr::from("that"), &that_cls)] {
            let Some(only) = sole_set(md, cls, model)? else {
                return Ok(());
            };
            bind.insert(v.clone(), cls.clone());
            locals.insert(v, local_names(&only));
        }
        if let Some(m) = first_missing(&line.expression, &bind, &locals, model)? {
            if m != hold() {
                return Err(Thrown::model(Phase::Mapping, m, None));
            }
            return Ok(());
        }
    }
    Ok(())
}

/// `MappingValidation.enginePairing`: `resolveLambdaParamNames` for the direction whose source is `this_end`'s class --
/// the parameters as (the source's, the target's); typed both and the classes not one: by type (a class its type or a
/// subtype), the first parameter tried as the source first, else refused; otherwise by name; else refused -- each in the
/// engine's words. None (held) where the parameters are typed by a name not spelled as its full path (the engine looks
/// it up as spelled).
pub fn engine_pairing(
    this_end: &AssociationDefinitionAssociationEndDefinition,
    that_end: &AssociationDefinitionAssociationEndDefinition,
    p0: &p::Variable,
    p1: &p::Variable,
    model: &ModelIndex,
) -> Result<Option<(JStr, JStr)>, Thrown> {
    let this_cls = model_join_nesting::target_class_fqn(this_end)?;
    let that_cls = model_join_nesting::target_class_fqn(that_end)?;
    let is_self = this_cls == that_cls;
    let t0 = p0.r#type.as_ref().and_then(mapping_normalizer::name_ref_or_null);
    let t1 = p1.r#type.as_ref().and_then(mapping_normalizer::name_ref_or_null);
    if !is_self && let (Some(t0), Some(t1)) = (t0, t1) {
        if !spelled_full(p0) || !spelled_full(p1) {
            return Ok(None);
        }
        let p0_is_this = subtype_of(&this_cls, t0, model);
        let p1_is_that = subtype_of(&that_cls, t1, model);
        let p0_is_that = subtype_of(&that_cls, t0, model);
        let p1_is_this = subtype_of(&this_cls, t1, model);
        if p0_is_this && p1_is_that {
            return Ok(Some((p0.name.clone(), p1.name.clone())));
        }
        if p0_is_that && p1_is_this {
            return Ok(Some((p1.name.clone(), p0.name.clone())));
        }
        return Err(Thrown::model(
            Phase::Mapping,
            cat!(
                "ModelJoin lambda parameter types do not match association property types: expected one parameter of \
                 type '",
                &simple_name(&this_cls),
                "' and one of type '",
                &simple_name(&that_cls),
                "', got '",
                &p0.name,
                ":",
                t0,
                "' and '",
                &p1.name,
                ":",
                t1,
                "'"
            ),
            None,
        ));
    }
    let src = &this_end.property_name;
    let tgt = &that_end.property_name;
    if &p0.name == src && &p1.name == tgt {
        return Ok(Some((p0.name.clone(), p1.name.clone())));
    }
    if &p0.name == tgt && &p1.name == src {
        return Ok(Some((p1.name.clone(), p0.name.clone())));
    }
    if is_self {
        return Err(Thrown::model(
            Phase::Mapping,
            cat!(
                "Self-association ModelJoin requires lambda parameter names to match the association property names \
                 ('",
                src,
                "' and '",
                tgt,
                "'), got '",
                &p0.name,
                "' and '",
                &p1.name,
                "'"
            ),
            None,
        ));
    }
    Err(Thrown::model(
        Phase::Mapping,
        cat!(
            "Cannot pair ModelJoin lambda parameters '",
            &p0.name,
            "' and '",
            &p1.name,
            "' with association properties '",
            src,
            "' and '",
            tgt,
            "'. Either type the parameters explicitly (e.g. {",
            &p0.name,
            ":",
            &simple_name(&this_cls),
            "[1], ",
            &p1.name,
            ":",
            &simple_name(&that_cls),
            "[1] | ...}) or rename the parameters to match the association property names ('",
            src,
            "' and '",
            tgt,
            "')."
        ),
        None,
    ))
}

/// `MappingValidation.spelledFull`: whether a parameter's type was written as its full path (its span, one line, as long
/// as the resolved name).
fn spelled_full(v: &p::Variable) -> bool {
    match &v.r#type {
        Some(p::TypeExpression::TypeExpressionNameRef(nr)) => nr.pos.as_ref().is_some_and(|pos| {
            pos.start_line == pos.end_line
                && i64::from(pos.end_column) - i64::from(pos.start_column) + 1 == nr.name.0.len() as i64
        }),
        _ => false,
    }
}

/// The engine's variables for one direction: the target's name bound first, the source's over it.
fn binding(tgt_var: &JStr, tgt_cls: &JStr, src_var: &JStr, src_cls: &JStr) -> HashMap<JStr, JStr> {
    let mut b = HashMap::new();
    b.insert(tgt_var.clone(), tgt_cls.clone());
    b.insert(src_var.clone(), src_cls.clone());
    b
}

/// `MappingValidation.firstMissing`: the first property the expression reads of a class that has it not, in the
/// engine's typing order; none when it reads none; [`HOLD`] outside the shapes where that is exact.
fn first_missing(
    v: &p::ValueSpecification,
    bind: &HashMap<JStr, JStr>,
    locals: &HashMap<JStr, HashSet<JStr>>,
    model: &ModelIndex,
) -> Result<Option<JStr>, Thrown> {
    match v {
        p::ValueSpecification::AppliedFunction(root) if boolean_call(root) => missing_in(root, bind, locals, model),
        _ => Ok(Some(hold())),
    }
}

fn boolean_call(af: &p::AppliedFunction) -> bool {
    let n = af.parameters.len();
    n == 2
        && (names_native(af, EQUAL__ANY_MANY__ANY_MANY)
            || names_native(af, AND__BOOLEAN_1__BOOLEAN_1)
            || names_native(af, OR__BOOLEAN_1__BOOLEAN_1))
        || n == 1 && names_native(af, NOT__BOOLEAN_1)
}

fn names_native(af: &p::AppliedFunction, n: NativeId) -> bool {
    lite_names::resolved_names::names(af, &n.def().qualified_name.lossy())
}

fn missing_in(
    af: &p::AppliedFunction,
    bind: &HashMap<JStr, JStr>,
    locals: &HashMap<JStr, HashSet<JStr>>,
    model: &ModelIndex,
) -> Result<Option<JStr>, Thrown> {
    let equal = names_native(af, EQUAL__ANY_MANY__ANY_MANY);
    for arg in af.parameters.iter() {
        let r = match arg {
            p::ValueSpecification::AppliedFunction(call) if boolean_call(call) => {
                missing_in(call, bind, locals, model)?
            }
            _ if !equal => Some(hold()),
            p::ValueSpecification::CString(_)
            | p::ValueSpecification::CInteger(_)
            | p::ValueSpecification::CBoolean(_)
            | p::ValueSpecification::CFloat(_) => None,
            _ => chain(arg, bind, locals, model)?.0,
        };
        if r.is_some() {
            return Ok(r);
        }
    }
    Ok(None)
}

/// `MappingValidation.chain`: a property chain's walk -- its refusal (a missing property's words, or HOLD), else the
/// class it lands on (none for a primitive or a local mapping property).
fn chain(
    v: &p::ValueSpecification,
    bind: &HashMap<JStr, JStr>,
    locals: &HashMap<JStr, HashSet<JStr>>,
    model: &ModelIndex,
) -> Result<(Option<JStr>, Option<JStr>), Thrown> {
    let ap = match v {
        p::ValueSpecification::Variable(var) => {
            return Ok(match bind.get(&var.name) {
                Some(c) => (None, Some(c.clone())),
                None => (Some(hold()), None),
            });
        }
        p::ValueSpecification::AppliedProperty(ap) => ap,
        _ => return Ok((Some(hold()), None)),
    };
    let empty = HashSet::new();
    let own = match &ap.receiver {
        p::ValueSpecification::Variable(rv) => locals.get(&rv.name).unwrap_or(&empty),
        _ => &empty,
    };
    let receiver = chain(&ap.receiver, bind, locals, model)?;
    if receiver.0.is_some() {
        return Ok(receiver);
    }
    let Some(cls) = receiver.1 else {
        return Ok((Some(hold()), None));
    };
    if temporal(&cls, model)? {
        return Ok((Some(hold()), None));
    }
    if own.contains(&ap.property) {
        return Ok((None, None)); // a local mapping property: its type is not read further
    }
    let Some(cd) = model.knowledge().hierarchy_class(Some(&cls)) else {
        return Ok((Some(hold()), None));
    };
    let Some(t) = model.knowledge().property_type(Some(cd), &ap.property)? else {
        let derived = match lineage_or_none(&cls, model) {
            None => true,
            Some(lineage) => lineage
                .iter()
                .any(|c| c.derived_properties.iter().any(|d| d.name == ap.property)),
        };
        if derived || !own.is_empty() {
            return Ok((Some(hold()), None));
        }
        return Ok((
            Some(cat!("Can't find property '", &ap.property, "' in class '", &cls, "'")),
            None,
        ));
    };
    Ok((
        None,
        match &t {
            p::TypeExpression::TypeExpressionNameRef(nr)
                if model.knowledge().hierarchy_class(Some(&nr.name)).is_some() =>
            {
                Some(nr.name.clone())
            }
            _ => None,
        },
    ))
}

/// `MappingValidation.temporal`: whether `cls` or a class it extends carries a temporal stereotype -- or its lineage is
/// not the model's to read: held either way.
fn temporal(cls: &JStr, model: &ModelIndex) -> Result<bool, Thrown> {
    Ok(match lineage_or_none(cls, model) {
        None => true,
        Some(lineage) => lineage.iter().any(|c| {
            c.stereotypes
                .iter()
                .any(|st| MilestoningStrategy::of_stereotype_or_null(&st.profile_name, &st.stereotype_name).is_some())
        }),
    })
}

/// `MappingValidation.temporalEnd`, for the pairing's caller.
pub fn temporal_end(cls: &JStr, model: &ModelIndex) -> Result<bool, Thrown> {
    temporal(cls, model)
}

/// `MappingValidation.lineageOrNull`: `cls`'s lineage, none where a class of it the model has not.
fn lineage_or_none(cls: &JStr, model: &ModelIndex) -> Option<Vec<Arc<ClassDefinition>>> {
    model.knowledge().hierarchy_class(Some(cls))?;
    model.knowledge().lineage(cls).ok()
}

/// `Type.subTypeOf`: the class itself, a class it extends, or Any.
fn subtype_of(child: &JStr, parent: &JStr, model: &ModelIndex) -> bool {
    child == parent || *parent == "meta::pure::metamodel::type::Any" || model.knowledge().is_subtype(child, parent)
}

/// `MappingValidation.plainSets`: whether `cls` has an instance set in the closure and every instance set the engine
/// would pair for it (its and its subtypes') is the class's own with no local mapping property.
fn plain_sets(md: &ResolvedMapping<'_>, cls: &JStr, model: &ModelIndex) -> Result<bool, Thrown> {
    let mut any = false;
    for (_, cm) in md.visible_sets()?.iter() {
        if matches!(
            cm,
            ClassMapping::ClassMappingUnion(_) | ClassMapping::ClassMappingInheritance(_)
        ) {
            continue;
        }
        if cm.class_name() != cls {
            if model.knowledge().is_subtype(cm.class_name(), cls) {
                return Ok(false);
            }
            continue;
        }
        if !local_names(cm).is_empty() {
            return Ok(false);
        }
        any = true;
    }
    Ok(any)
}

/// `MappingValidation.soleSet`: `cls`'s one instance set in the closure, if it has exactly one and no subtype is mapped.
fn sole_set(md: &ResolvedMapping<'_>, cls: &JStr, model: &ModelIndex) -> Result<Option<ClassMapping>, Thrown> {
    let mut only: Option<ClassMapping> = None;
    for (_, cm) in md.visible_sets()?.iter() {
        if matches!(
            cm,
            ClassMapping::ClassMappingUnion(_) | ClassMapping::ClassMappingInheritance(_)
        ) {
            continue;
        }
        if cm.class_name() == cls {
            if only.is_some() {
                return Ok(None);
            }
            only = Some(cm.clone());
        } else if model.knowledge().is_subtype(cm.class_name(), cls) {
            return Ok(None);
        }
    }
    Ok(only)
}

/// `MappingValidation.localNames`: a set's local mapping properties' names.
fn local_names(cm: &ClassMapping) -> HashSet<JStr> {
    let mut out = HashSet::new();
    match cm {
        ClassMapping::ClassMappingRelational(r) => {
            for pm in r.property_mappings.iter() {
                if let PropertyMapping::PropertyMappingLocalProperty(lp) = pm {
                    out.insert(lp.property_name.clone());
                }
            }
        }
        ClassMapping::ClassMappingPure(p) => {
            for b in p.property_bindings.iter() {
                if b.local {
                    out.insert(b.property_name.clone());
                }
            }
        }
        ClassMapping::ClassMappingRelationFunction(f) => {
            for c in f.columns.iter() {
                if c.local {
                    out.insert(c.property.clone());
                }
            }
        }
        ClassMapping::ClassMappingUnion(_) | ClassMapping::ClassMappingInheritance(_) => {}
    }
    out
}

/// A qualified name's last segment.
fn simple_name(fqn: &JStr) -> JStr {
    match lite_model::text::last_index_of(&fqn.0, &JStr::from("::").0) {
        Some(i) => JStr::from(&fqn.0[i + 2..]),
        None => fqn.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::collections::HashMap;
    use std::rc::Rc;

    use lite_front::records as p;
    use lite_model::linked::list;
    use lite_model::text::j;
    use lite_model::{ClassMappingInheritance, PropertyMappingLocalProperty};

    use crate::lifted_views::LiftedViews;
    use crate::mapping_closures::MappingClosures;
    use crate::test_support::{cm, column_pm, index, mapping, model, relational, union};

    const MODEL: &str = r#"
###Pure
Class m::P
{
  name: String[1];
}

###Relational
Database m::db
(
  Table T (NAME VARCHAR(20))
)

###Mapping
Mapping m::Base
(
  *m::P[f]: Relational
  {
    ~mainTable [m::db]T
    name: [m::db]T.NAME
  }
)
Mapping m::Over
(
  include m::Base
  m::P[f]: Relational
  {
    ~mainTable [m::db]T
    name: [m::db]T.NAME
  }
)
Mapping m::Top
(
  include m::Base
)
"#;

    fn resolved<'a>(
        md: Arc<LegacyMappingDefinition>,
        closures: &'a MappingClosures<'a>,
        views: &'a LiftedViews<'a>,
    ) -> ResolvedMapping<'a> {
        let closure = closures.closure(&md.qualified_name);
        ResolvedMapping::new(
            md.clone(),
            md,
            Rc::new(HashMap::new()),
            Rc::new(LinkedMap::new()),
            closure,
            views,
        )
    }

    #[test]
    fn a_set_id_taken_twice_across_the_closure_throws_for_the_mapping() {
        let parsed = model(MODEL);
        let idx = index(&parsed);
        let closures = MappingClosures::new(&idx);
        let views = LiftedViews::new(&parsed, &idx);
        let e = run(&resolved(mapping(&parsed, "m::Over"), &closures, &views), &idx).unwrap_err();
        assert!(e.is_model());
        assert_eq!(e.phase(), Some(Phase::Model));
        assert_eq!(e.element(), Some(j("m::Over")));
        assert_eq!(
            e.message(),
            Some(j(
                "Duplicated class mappings found with ID [f (m::Base, m::Over)] in mapping 'm::Over'"
            ))
        );
    }

    #[test]
    fn each_invalid_set_is_recorded_by_its_id_in_declaration_order() {
        let parsed = model(MODEL);
        let idx = index(&parsed);
        let closures = MappingClosures::new(&idx);
        let views = LiftedViews::new(&parsed, &idx);
        let top = mapping(&parsed, "m::Top");
        let local = PropertyMapping::PropertyMappingLocalProperty(Arc::new(PropertyMappingLocalProperty {
            property_name: j("local"),
            r#type: p::TypeExpression::TypeExpressionNameRef(Box::new(p::TypeExpressionNameRef {
                name: j("String"),
                pos: None,
            })),
            multiplicity: p::Multiplicity::MultiplicityConcrete(Box::new(p::MultiplicityConcrete {
                lower_bound: 1,
                upper_bound: Some(1),
            })),
            body: column_pm("local", "T", "NAME"),
        }));
        let md = Arc::new(LegacyMappingDefinition {
            class_mappings: list(vec![
                cm(relational(
                    "m::P",
                    Some("bad"),
                    vec![local, column_pm("nope", "T", "NAME")],
                )),
                union("m::P", Some("u"), &["f", "bad", "gone"]),
                // a class the model has not: its PMs are not read
                cm(relational(
                    "m::Unknown",
                    Some("x"),
                    vec![column_pm("nope", "T", "NAME")],
                )),
                ClassMapping::ClassMappingInheritance(Arc::new(ClassMappingInheritance {
                    class_name: j("m::P"),
                    set_id: Some(j("ih")),
                    extends_set_id: None,
                    root: false,
                })),
                cm(relational("m::P", Some("good"), vec![column_pm("name", "T", "NAME")])),
            ]),
            ..(*top).clone()
        });
        let invalid = run(&resolved(md, &closures, &views), &idx).unwrap();
        let got: Vec<(JStr, Option<JStr>, Option<Phase>)> = invalid
            .iter()
            .map(|(id, e)| {
                assert!(e.is_model() && e.element().is_none() == (id == &j("bad")));
                (id.clone(), e.message(), e.phase())
            })
            .collect();
        assert_eq!(
            got,
            vec![
                (
                    j("bad"),
                    Some(j(
                        "PropertyMapping 'nope' references property not declared on class 'm::P'; mapping=m::Top"
                    )),
                    Some(Phase::Model)
                ),
                // the include's `f` and the mapping's own sets are members; `gone` is none
                (
                    j("u"),
                    Some(j("Can't find class mapping 'gone' in mapping 'm::Top'")),
                    Some(Phase::Mapping)
                ),
            ]
        );
    }

    #[test]
    fn a_join_that_leaves_the_table_its_chain_is_at_is_the_engines_refusal() {
        // legend-engine 4.145.0's validatePropertyMappings, asked: "Mapping error: the join AC does not contain the
        // source table T" (the normalizer's second batch, item 1, JCE:843, 2026-11-15)
        let text = r#"
###Pure
Class w::P { name: String[0..1]; b: w::B[0..1]; }
Class w::B { name: String[0..1]; }
###Relational
Database w::Db
(
  Table T (ID INTEGER PRIMARY KEY, NAME VARCHAR(20), BID INTEGER)
  Table A (ID INTEGER PRIMARY KEY, NAME VARCHAR(20), CID INTEGER)
  Table C (ID INTEGER PRIMARY KEY, NAME VARCHAR(20))
  Join TA(T.BID = A.ID)
  Join AC(A.CID = C.ID)
  Join TC(T.BID = C.ID)
)
###Mapping
Mapping w::M
(
  w::P[a]: Relational { ~mainTable [w::Db]T name: [w::Db]T.NAME, b: [w::Db]@AC }
  w::P[b]: Relational { ~mainTable [w::Db]T name: [w::Db]T.NAME, b: [w::Db]@TA > @TC }
  w::P[c]: Relational { ~mainTable [w::Db]T name: [w::Db]T.NAME, b: [w::Db]@TA > @AC }
  w::B: Relational { ~mainTable [w::Db]C name: [w::Db]C.NAME }
)
"#;
        let parsed = model(text);
        let idx = index(&parsed);
        let closures = MappingClosures::new(&idx);
        let views = LiftedViews::new(&parsed, &idx);
        let invalid = run(&resolved(mapping(&parsed, "w::M"), &closures, &views), &idx).unwrap();
        let reasons: Vec<(JStr, Option<JStr>)> = invalid.iter().map(|(id, e)| (id.clone(), e.message())).collect();
        assert_eq!(
            reasons,
            vec![
                (
                    j("a"),
                    Some(j("Mapping error: the join AC does not contain the source table T"))
                ),
                (
                    j("b"),
                    Some(j("Mapping error: the join TC does not contain the source table A"))
                ),
            ]
        );
    }
}
