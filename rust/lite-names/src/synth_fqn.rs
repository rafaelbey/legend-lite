// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! `SynthFqn` (2026-11-12): the synthesized functions' names, `<owner>$<hat>$<name>`, the one place Phase E writes them
//! and Phase F reads them. The `$` sigil is not user-writable, so a name with one in an operand is a caller's bug,
//! refused when the name is made (`IllegalArgumentException`).

use lite_model::{JStr, SynthHat, cat};

use crate::error::Thrown;

/// `prop(owner, name)`: `<owner>$prop$<name>`, a derived property's body.
pub fn prop(owner_fqn: &JStr, property_name: &JStr) -> Result<JStr, Thrown> {
    hat_fqn(owner_fqn, SynthHat::PROP, property_name)
}

/// `constraint(owner, name)`: `<owner>$constraint$<name>`.
pub fn constraint(owner_fqn: &JStr, constraint_name: &JStr) -> Result<JStr, Thrown> {
    hat_fqn(owner_fqn, SynthHat::CONSTRAINT, constraint_name)
}

/// `condition(functionId, post, place)`: `<id>$pre$<place>` or `<id>$post$<place>`.
pub fn condition(function_id: &JStr, post: bool, place: &JStr) -> Result<JStr, Thrown> {
    hat_fqn(function_id, if post { SynthHat::POST } else { SynthHat::PRE }, place)
}

/// `defaultValue(owner, property)`: `<class>$default$<property>`.
pub fn default_value(owner_fqn: &JStr, property_name: &JStr) -> Result<JStr, Thrown> {
    hat_fqn(owner_fqn, SynthHat::DEFAULT, property_name)
}

/// `constraintMsg(owner, name)`: `<owner>$constraintMsg$<name>`.
pub fn constraint_msg(owner_fqn: &JStr, constraint_name: &JStr) -> Result<JStr, Thrown> {
    hat_fqn(owner_fqn, SynthHat::CONSTRAINT_MSG, constraint_name)
}

/// `query(service)`: `<svc>$query`.
pub fn query(service_fqn: &JStr) -> Result<JStr, Thrown> {
    check(Some(service_fqn), "serviceFqn")?;
    Ok(cat!(service_fqn, "$", SynthHat::QUERY.segment()))
}

/// `view(db, name)`: `<db>$view$<name>`, a database view lifted as a relation function.
pub fn view(db_fqn: &JStr, view_name: &JStr) -> Result<JStr, Thrown> {
    hat_fqn(db_fqn, SynthHat::VIEW, view_name)
}

/// `mappingClass(mapping, class)`: `<mapping>$class$<classFqn>`, the full class name embedded (injective).
pub fn mapping_class(mapping_fqn: &JStr, class_fqn: &JStr) -> Result<JStr, Thrown> {
    hat_fqn(mapping_fqn, SynthHat::CLASS, class_fqn)
}

/// `mappingClassSet(mapping, class, setId)`: `<mapping>$class$<classFqn>$set$<setId>`.
pub fn mapping_class_set(mapping_fqn: &JStr, class_fqn: &JStr, set_id: &JStr) -> Result<JStr, Thrown> {
    Ok(cat!(hat_fqn(mapping_fqn, SynthHat::CLASS, class_fqn)?, "$set$", set_id))
}

/// `mappingAssoc(mapping, association)`: `<mapping>$assoc$<associationFqn>`.
pub fn mapping_assoc(mapping_fqn: &JStr, association_fqn: &JStr) -> Result<JStr, Thrown> {
    hat_fqn(mapping_fqn, SynthHat::ASSOC, association_fqn)
}

fn hat_fqn(owner_fqn: &JStr, hat: SynthHat, name: &JStr) -> Result<JStr, Thrown> {
    check(Some(owner_fqn), "ownerFqn")?;
    check(Some(name), "name")?;
    Ok(cat!(owner_fqn, "$", hat.segment(), "$", name))
}

fn check(s: Option<&JStr>, what: &str) -> Result<(), Thrown> {
    let Some(s) = s else {
        return Err(Thrown::illegal_argument(cat!("synth-FQN ", what, " is null")));
    };
    if s.0.contains(&u16::from(b'$')) {
        return Err(Thrown::illegal_argument(cat!(
            "synth-FQN ",
            what,
            " contains the reserved '$' sigil: ",
            s
        )));
    }
    Ok(())
}
