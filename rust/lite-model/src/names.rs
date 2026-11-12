// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! The model's small naming rules: `RelationalIdentifier` (a relational name bare, and whether it was quoted),
//! `JoinType.fromIdentifier`, `LogicalOp`, `ComparisonOp`, `SynthHat.segment`, `SetId` and `DerivedPropertyNames`.

use lite_front::jtypes::JStr;

use crate::error::{Thrown, illegal_argument};
use crate::records::{
    ClassMapping, ComparisonOp, JoinType, LogicalOp, MappingDefinitionClassBinding, PoisonKey, SynthHat,
};
use crate::text::{ascii_upper, index_of, is};

/// `RelationalIdentifier.isQuoted(raw)`: a double-quoted identifier.
pub fn is_quoted(raw: &[u16]) -> bool {
    raw.len() >= 2 && raw[0] == u16::from(b'"') && raw[raw.len() - 1] == u16::from(b'"')
}

/// `RelationalIdentifier.parts(dotted)`: split on each `.` outside a quoted identifier, the backslash escape kept
/// inside one; each part is bared by its reader.
pub fn parts(dotted: &[u16]) -> Vec<JStr> {
    let mut out = Vec::new();
    let mut part: Vec<u16> = Vec::new();
    let mut quoted = false;
    let mut i = 0;
    while i < dotted.len() {
        let c = dotted[i];
        if quoted && c == u16::from(b'\\') && i + 1 < dotted.len() {
            part.push(c);
            i += 1;
            part.push(dotted[i]);
            i += 1;
            continue;
        }
        if c == u16::from(b'"') {
            quoted = !quoted;
        } else if c == u16::from(b'.') && !quoted {
            out.push(JStr(std::mem::take(&mut part)));
            i += 1;
            continue;
        }
        part.push(c);
        i += 1;
    }
    out.push(JStr(part));
    out
}

/// `RelationalIdentifier.bare(raw)`: the name a spelling denotes -- quotes off, the backslash escape decoded; an
/// unquoted identifier is itself.
pub fn bare(raw: &JStr) -> JStr {
    let r = &raw.0;
    if !is_quoted(r) {
        return raw.clone();
    }
    let inner = &r[1..r.len() - 1];
    if !inner.contains(&u16::from(b'\\')) {
        return JStr(inner.to_vec());
    }
    let mut out = Vec::with_capacity(inner.len());
    let mut i = 0;
    while i < inner.len() {
        let mut c = inner[i];
        if c == u16::from(b'\\') && i + 1 < inner.len() {
            i += 1;
            c = inner[i];
        }
        out.push(c);
        i += 1;
    }
    JStr(out)
}

/// `JoinType.fromIdentifier(identifier)`: a source spelling, any case, to its canonical join type.
pub fn join_type_from_identifier(identifier: &JStr) -> Result<JoinType, Thrown> {
    let upper = ascii_upper(&identifier.0);
    Ok(match upper.as_deref() {
        Some("INNER") => JoinType::INNER,
        Some("LEFT" | "LEFT_OUTER") => JoinType::LEFT_OUTER,
        Some("RIGHT" | "RIGHT_OUTER") => JoinType::RIGHT_OUTER,
        Some("FULL" | "FULL_OUTER") => JoinType::FULL_OUTER,
        Some("OUTER") => JoinType::OUTER,
        _ => {
            return Err(illegal_argument(crate::cat!(
                "unknown join type identifier: '",
                identifier,
                "'"
            )));
        }
    })
}

impl LogicalOp {
    /// `LogicalOp.fromKeyword`.
    pub fn from_keyword(keyword: &JStr) -> Result<LogicalOp, Thrown> {
        if is(&keyword.0, "and") {
            Ok(LogicalOp::AND)
        } else if is(&keyword.0, "or") {
            Ok(LogicalOp::OR)
        } else {
            Err(illegal_argument(crate::cat!(
                "not a logical operator keyword: '",
                keyword,
                "'"
            )))
        }
    }

    pub fn keyword(self) -> &'static str {
        match self {
            LogicalOp::AND => "and",
            LogicalOp::OR => "or",
        }
    }
}

impl ComparisonOp {
    pub fn symbol(self) -> &'static str {
        match self {
            ComparisonOp::EQ => "=",
            ComparisonOp::NEQ => "<>",
            ComparisonOp::LT => "<",
            ComparisonOp::LTE => "<=",
            ComparisonOp::GT => ">",
            ComparisonOp::GTE => ">=",
        }
    }
}

impl SynthHat {
    /// The segment a lifted name spells this hat with: `constraintMsg`, else the name in lower case.
    pub fn segment(self) -> &'static str {
        match self {
            SynthHat::PROP => "prop",
            SynthHat::CONSTRAINT => "constraint",
            SynthHat::QUERY => "query",
            SynthHat::CLASS => "class",
            SynthHat::ASSOC => "assoc",
            SynthHat::CONSTRAINT_MSG => "constraintMsg",
            SynthHat::VIEW => "view",
            SynthHat::DEFAULT => "default",
            SynthHat::PRE => "pre",
            SynthHat::POST => "post",
        }
    }
}

/// `SetIds.defaultFor(fqn)` (`SetId.defaultFor`): the engine's default id, `::` spelled `_`.
pub fn default_set_id(fqn: &JStr) -> JStr {
    JStr(crate::text::replace(&fqn.0, "::", "_"))
}

/// `SetId.of(declared, fqn)`: the declared id when it is written (not empty), else the default.
pub fn set_id_of(declared: Option<&JStr>, fqn: &JStr) -> JStr {
    match declared {
        Some(d) if !d.is_empty() => d.clone(),
        _ => default_set_id(fqn),
    }
}

/// `SetId.isDefault(id, fqn)`: exactly the default id of `fqn`.
pub fn is_default_set_id(id: &JStr, fqn: &JStr) -> bool {
    *id == default_set_id(fqn)
}

/// `SetId.of(ClassMapping)`.
pub fn set_id_of_class_mapping(cm: &ClassMapping) -> JStr {
    set_id_of(cm.set_id(), cm.class_name())
}

/// `SetId.of(MappingDefinition.ClassBinding)`.
pub fn set_id_of_binding(cb: &MappingDefinitionClassBinding) -> JStr {
    set_id_of(cb.set_id(), cb.class_fqn())
}

/// `DerivedPropertyNames.SIGIL`: `$prop$`.
pub const DERIVED_PROPERTY_SIGIL: &str = "$prop$";

/// `DerivedPropertyNames.lifted(owner, name)`.
pub fn lifted_derived_property(owner_fqn: &JStr, name: &JStr) -> JStr {
    crate::cat!(owner_fqn, DERIVED_PROPERTY_SIGIL, name)
}

/// `DerivedPropertyNames.split(fqn)`: the owner and the name, or `None` when `fqn` is no lifted derived property.
pub fn split_derived_property(fqn: &JStr) -> Option<(JStr, JStr)> {
    let sigil: Vec<u16> = DERIVED_PROPERTY_SIGIL.encode_utf16().collect();
    match index_of(&fqn.0, &sigil) {
        Some(i) if i > 0 => Some((JStr(fqn.0[..i].to_vec()), JStr(fqn.0[i + sigil.len()..].to_vec()))),
        _ => None,
    }
}

impl PoisonKey {
    /// Java's `toString()`: a class's FQN, `fqn[setId]` for a set, an association's FQN.
    pub fn to_java_string(&self) -> JStr {
        match self {
            PoisonKey::PoisonKeyForClass(k) => k.class_fqn.clone(),
            PoisonKey::PoisonKeyForSet(k) => crate::cat!(k.class_fqn, "[", k.set_id, "]"),
            PoisonKey::PoisonKeyForAssociation(k) => k.association_fqn.clone(),
        }
    }
}
