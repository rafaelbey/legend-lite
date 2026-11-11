// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! Java's `Type.Primitive`: the built-in primitives, each with its FQN and family, and the table of every FQN the
//! platform reads as one of them (the precise primitives aliased to the parents legend-pure declares).
//!
//! The FQNs are `builtin/Pure.java`'s native classes' (Java reads them off `Pure.NUMBER` and its kin); until
//! `builtin` is ported they are spelled here, held to Java's by the oracle. The lattice (Integer < Number < Any) is
//! not here, as it is not in Java's: it is the classes' `extends` chain, walked by the model (`ModelContext.isSubtype`).

/// Java's `Type.Primitive.Family`: a coarse family for coercion and a dialect's type mapping.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Family {
    Numeric,
    Text,
    Boolean,
    Temporal,
    Binary,
}

impl Family {
    /// Every constant, in Java's order (`values()`).
    pub const ALL: [Family; 5] = [
        Family::Numeric,
        Family::Text,
        Family::Boolean,
        Family::Temporal,
        Family::Binary,
    ];

    /// The constant's Java `name()`.
    pub fn name(self) -> &'static str {
        match self {
            Family::Numeric => "NUMERIC",
            Family::Text => "TEXT",
            Family::Boolean => "BOOLEAN",
            Family::Temporal => "TEMPORAL",
            Family::Binary => "BINARY",
        }
    }
}

/// A built-in primitive: Java's enum, constant for constant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Primitive {
    Number,
    Integer,
    Float,
    Decimal,
    String,
    Boolean,
    Byte,
    Date,
    StrictDate,
    DateTime,
    LatestDate,
    StrictTime,
}

/// The package of legend-pure's precisePrimitives.pure (Java's `Type.Primitive.PRECISE_PRIMITIVES`).
pub const PRECISE_PRIMITIVES: &str = "meta::pure::precisePrimitives::";

/// A primitive's declared type variable (`Primitive Varchar(x:Integer[1]) extends String`): its name and type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TypeVariable {
    pub name: &'static str,
    pub r#type: Primitive,
}

/// Every FQN [`Primitive::find_by_fqn`] knows, to its primitive, in Java's order (`Type.Primitive.byFqn()`, a
/// `LinkedHashMap`): the base primitives, then legend-pure's precise primitives, each to the parent it declares.
static BY_FQN: [(&str, Primitive); 25] = [
    ("meta::pure::metamodel::type::Number", Primitive::Number),
    ("meta::pure::metamodel::type::Integer", Primitive::Integer),
    ("meta::pure::metamodel::type::Float", Primitive::Float),
    ("meta::pure::metamodel::type::Decimal", Primitive::Decimal),
    ("meta::pure::metamodel::type::String", Primitive::String),
    ("meta::pure::metamodel::type::Boolean", Primitive::Boolean),
    ("meta::pure::metamodel::type::Byte", Primitive::Byte),
    ("meta::pure::metamodel::type::Date", Primitive::Date),
    ("meta::pure::metamodel::type::StrictDate", Primitive::StrictDate),
    ("meta::pure::metamodel::type::DateTime", Primitive::DateTime),
    ("meta::pure::metamodel::type::LatestDate", Primitive::LatestDate),
    ("meta::pure::metamodel::type::StrictTime", Primitive::StrictTime),
    ("meta::pure::precisePrimitives::TinyInt", Primitive::Integer),
    ("meta::pure::precisePrimitives::UTinyInt", Primitive::Integer),
    ("meta::pure::precisePrimitives::SmallInt", Primitive::Integer),
    ("meta::pure::precisePrimitives::USmallInt", Primitive::Integer),
    ("meta::pure::precisePrimitives::Int", Primitive::Integer),
    ("meta::pure::precisePrimitives::UInt", Primitive::Integer),
    ("meta::pure::precisePrimitives::BigInt", Primitive::Integer),
    ("meta::pure::precisePrimitives::UBigInt", Primitive::Integer),
    ("meta::pure::precisePrimitives::Float4", Primitive::Float),
    ("meta::pure::precisePrimitives::Double", Primitive::Float),
    ("meta::pure::precisePrimitives::Varchar", Primitive::String),
    ("meta::pure::precisePrimitives::Timestamp", Primitive::DateTime),
    ("meta::pure::precisePrimitives::Numeric", Primitive::Decimal),
];

/// The type variables of the precise primitives that declare any, verbatim from legend-pure's precisePrimitives.pure
/// (read in the pinned source 2026-11-05, Java's comment says): `Varchar(x:Integer[1])`,
/// `Numeric(precision:Integer[1], scale:Integer[1])`.
static VARCHAR_VARIABLES: [TypeVariable; 1] = [TypeVariable {
    name: "x",
    r#type: Primitive::Integer,
}];
static NUMERIC_VARIABLES: [TypeVariable; 2] = [
    TypeVariable {
        name: "precision",
        r#type: Primitive::Integer,
    },
    TypeVariable {
        name: "scale",
        r#type: Primitive::Integer,
    },
];

impl Primitive {
    /// Every constant, in Java's order (`values()`).
    pub const ALL: [Primitive; 12] = [
        Primitive::Number,
        Primitive::Integer,
        Primitive::Float,
        Primitive::Decimal,
        Primitive::String,
        Primitive::Boolean,
        Primitive::Byte,
        Primitive::Date,
        Primitive::StrictDate,
        Primitive::DateTime,
        Primitive::LatestDate,
        Primitive::StrictTime,
    ];

    /// The constant's Java `name()`.
    pub fn name(self) -> &'static str {
        match self {
            Primitive::Number => "NUMBER",
            Primitive::Integer => "INTEGER",
            Primitive::Float => "FLOAT",
            Primitive::Decimal => "DECIMAL",
            Primitive::String => "STRING",
            Primitive::Boolean => "BOOLEAN",
            Primitive::Byte => "BYTE",
            Primitive::Date => "DATE",
            Primitive::StrictDate => "STRICT_DATE",
            Primitive::DateTime => "DATE_TIME",
            Primitive::LatestDate => "LATEST_DATE",
            Primitive::StrictTime => "STRICT_TIME",
        }
    }

    /// The constant whose Java `name()` is `name`.
    pub fn from_name(name: &str) -> Option<Primitive> {
        Primitive::ALL.into_iter().find(|p| p.name() == name)
    }

    /// The fully qualified name, e.g. `meta::pure::metamodel::type::Integer`: the first `BY_FQN` row of the
    /// primitive, since the base primitives come first, in `values()` order.
    pub fn qualified_name(self) -> &'static str {
        BY_FQN[self as usize].0
    }

    pub fn family(self) -> Family {
        match self {
            Primitive::Number | Primitive::Integer | Primitive::Float | Primitive::Decimal => Family::Numeric,
            Primitive::String => Family::Text,
            Primitive::Boolean => Family::Boolean,
            Primitive::Byte => Family::Binary,
            Primitive::Date
            | Primitive::StrictDate
            | Primitive::DateTime
            | Primitive::LatestDate
            | Primitive::StrictTime => Family::Temporal,
        }
    }

    pub fn is_numeric(self) -> bool {
        self.family() == Family::Numeric
    }

    pub fn is_temporal(self) -> bool {
        self.family() == Family::Temporal
    }

    /// The name after the FQN's last `::` (Java's `typeName()`).
    pub fn type_name(self) -> &'static str {
        simple_name(self.qualified_name())
    }

    /// The primitive `fqn` names, if any (Java's `findByFqn`).
    pub fn find_by_fqn(fqn: &str) -> Option<Primitive> {
        BY_FQN.iter().find(|(f, _)| *f == fqn).map(|(_, p)| *p)
    }

    /// Every FQN [`Primitive::find_by_fqn`] knows, to its primitive, in Java's order (`byFqn()`).
    pub fn by_fqn() -> &'static [(&'static str, Primitive)] {
        &BY_FQN
    }

    /// The type variables the platform primitive `fqn` declares (none for most); `None` when `fqn` is not one
    /// (Java's `typeVariables`).
    pub fn type_variables(fqn: &str) -> Option<&'static [TypeVariable]> {
        Primitive::find_by_fqn(fqn)?;
        Some(match fqn.strip_prefix(PRECISE_PRIMITIVES) {
            Some("Varchar") => &VARCHAR_VARIABLES,
            Some("Numeric") => &NUMERIC_VARIABLES,
            _ => &[],
        })
    }
}

/// `fqn.contains("::") ? fqn.substring(fqn.lastIndexOf("::") + 2) : fqn`, Java's simple-name idiom.
pub(crate) fn simple_name(fqn: &str) -> &str {
    match fqn.rfind("::") {
        Some(at) => &fqn[at + 2..],
        None => fqn,
    }
}

#[cfg(test)]
mod tests {
    use super::{BY_FQN, Primitive};

    const M3: &str = "meta::pure::metamodel::type::";

    #[test]
    fn the_base_rows_come_first_in_values_order() {
        for (i, p) in Primitive::ALL.into_iter().enumerate() {
            assert_eq!(BY_FQN[i].1, p);
            assert!(BY_FQN[i].0.starts_with(M3));
            assert_eq!(Primitive::from_name(p.name()), Some(p));
        }
        assert_eq!(Primitive::StrictDate.type_name(), "StrictDate");
    }
}
