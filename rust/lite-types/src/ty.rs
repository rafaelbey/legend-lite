// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! Java's `Type` family as one `Copy` family with a lifetime (PHASE3_SHIM_DESIGN.md §7.3 c, ruling 9): a leaf inline,
//! a structural type a reference to its record in an arena, every list a slice and every name a `&'a str` there.
//! The family is covariant in `'a`, so a model's `&'m Type<'m>` is a query's `&'q Type<'q>` with no conversion.
//! Equality is Java's record equality: structural, lists element by element.

use std::fmt;

use lite_base::Alloc;
use lite_sql::{Scalar, SqlType};

use crate::decimal::PrecisionDecimal;
use crate::error::TypeError;
use crate::multiplicity::{Bounded, Multiplicity};
use crate::platform;
use crate::primitive::{Primitive, simple_name};

/// A kinded type (Java's sealed `Type`): its nine variants, variant for variant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Type<'a> {
    Primitive(Primitive),
    PrecisionDecimal(PrecisionDecimal),
    /// a (user or native) class, by FQN
    ClassType(&'a str),
    /// an enumeration, by FQN
    EnumType(&'a str),
    /// a type parameter, `T`
    TypeVar(&'a str),
    GenericType(&'a GenericType<'a>),
    FunctionType(&'a FunctionType<'a>),
    RelationType(&'a RelationType<'a>),
    SchemaAlgebra(&'a SchemaAlgebra<'a>),
}

/// `rawFqn<arg, ...>`, its multiplicity arguments beside (`Result<T|m>`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct GenericType<'a> {
    pub raw_fqn: &'a str,
    pub arguments: &'a [Type<'a>],
    pub mult_arguments: &'a [Multiplicity<'a>],
}

/// `{ParamType[mult], ... -> ResultType[mult]}`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FunctionType<'a> {
    pub params: &'a [Param<'a>],
    pub result: Param<'a>,
}

/// An inline relation type `(col:Type[mult], ...)`: its columns, unique by name, and a pivot's aggregate templates
/// (`dynamic_columns`, unique too), which no checker reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RelationType<'a> {
    columns: &'a [Column<'a>],
    dynamic_columns: &'a [Column<'a>],
}

/// `T+V`, `T-Z`, `Z⊆T`, `Z=K`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SchemaAlgebra<'a> {
    pub left: Type<'a>,
    pub op: Op,
    pub right: Type<'a>,
}

/// A schema-algebra operator (Java's `Type.Op`, named as the engine names them).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Op {
    Equal,
    Union,
    Difference,
    Subset,
}

/// A type with its multiplicity, inside a [`FunctionType`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Param<'a> {
    pub r#type: Type<'a>,
    pub multiplicity: Multiplicity<'a>,
}

/// A column of a [`RelationType`]: name, type, multiplicity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Column<'a> {
    pub name: &'a str,
    pub r#type: Type<'a>,
    pub multiplicity: Multiplicity<'a>,
}

/// An expression's type: a [`Type`] and its [`Multiplicity`] (Java's `ExprType`), on every typed node.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ExprType<'a> {
    pub r#type: Type<'a>,
    pub multiplicity: Multiplicity<'a>,
}

impl<'a> ExprType<'a> {
    /// `type[1]`
    pub fn one(r#type: Type<'a>) -> ExprType<'a> {
        ExprType {
            r#type,
            multiplicity: Multiplicity::ONE,
        }
    }
}

impl Op {
    /// Every constant, in Java's order.
    pub const ALL: [Op; 4] = [Op::Equal, Op::Union, Op::Difference, Op::Subset];

    /// The constant's Java `name()`.
    pub fn name(self) -> &'static str {
        match self {
            Op::Equal => "EQUAL",
            Op::Union => "UNION",
            Op::Difference => "DIFFERENCE",
            Op::Subset => "SUBSET",
        }
    }

    pub fn symbol(self) -> &'static str {
        match self {
            Op::Equal => "=",
            Op::Union => "+",
            Op::Difference => "-",
            Op::Subset => "\u{2286}",
        }
    }
}

impl<'a> GenericType<'a> {
    pub fn new<A: Alloc>(
        arena: &'a A,
        raw_fqn: &'a str,
        arguments: &[Type<'a>],
        mult_arguments: &[Multiplicity<'a>],
    ) -> &'a GenericType<'a> {
        arena.alloc(GenericType {
            raw_fqn,
            arguments: arena.alloc_slice(arguments),
            mult_arguments: arena.alloc_slice(mult_arguments),
        })
    }
}

impl<'a> FunctionType<'a> {
    pub fn new<A: Alloc>(arena: &'a A, params: &[Param<'a>], result: Param<'a>) -> &'a FunctionType<'a> {
        arena.alloc(FunctionType {
            params: arena.alloc_slice(params),
            result,
        })
    }
}

impl<'a> SchemaAlgebra<'a> {
    pub fn new<A: Alloc>(arena: &'a A, left: Type<'a>, op: Op, right: Type<'a>) -> &'a SchemaAlgebra<'a> {
        arena.alloc(SchemaAlgebra { left, op, right })
    }
}

/// `*`: the one dynamic column of a late-bound schema (Java's `LATE_BOUND_WILDCARD`).
pub const LATE_BOUND_WILDCARD: &str = "*";
/// Between a pivoted value and its aggregate template's name (Java's `PIVOT_SEPARATOR`, the SQL tree's).
pub const PIVOT_SEPARATOR: &str = lite_sql::PIVOT_SEPARATOR;

/// The raw-SQL grid's wildcard template: every column trusted, `Any[0..1]`.
static GRID_TEMPLATE: [Column<'static>; 1] = [Column {
    name: LATE_BOUND_WILDCARD,
    r#type: Type::ClassType(platform::ANY),
    multiplicity: Multiplicity::ZERO_ONE,
}];

static LATE_BOUND: RelationType<'static> = RelationType {
    columns: &[],
    dynamic_columns: &GRID_TEMPLATE,
};

impl<'a> RelationType<'a> {
    /// The relation type, its columns' names unique (as legend-pure requires) and its templates' too.
    pub fn new<A: Alloc>(
        arena: &'a A,
        columns: &[Column<'a>],
        dynamic_columns: &[Column<'a>],
    ) -> Result<&'a RelationType<'a>, TypeError> {
        if let Some(name) = first_repeat(columns) {
            return Err(TypeError::argument(format!(
                "duplicate column '{name}' in relation type"
            )));
        }
        if let Some(name) = first_repeat(dynamic_columns) {
            return Err(TypeError::argument(format!(
                "duplicate dynamic (pivot template) column '{name}'"
            )));
        }
        Ok(arena.alloc(RelationType {
            columns: arena.alloc_slice(columns),
            dynamic_columns: arena.alloc_slice(dynamic_columns),
        }))
    }

    /// A relation type with no templates (Java's one-argument constructor).
    pub fn of<A: Alloc>(arena: &'a A, columns: &[Column<'a>]) -> Result<&'a RelationType<'a>, TypeError> {
        RelationType::new(arena, columns, &[])
    }

    pub fn columns(&self) -> &'a [Column<'a>] {
        self.columns
    }

    pub fn dynamic_columns(&self) -> &'a [Column<'a>] {
        self.dynamic_columns
    }

    /// A relation whose columns are late-bound (a raw-SQL grid's): no columns, the one wildcard template.
    pub fn late_bound() -> &'static RelationType<'static> {
        &LATE_BOUND
    }

    /// The erased row: a row-accessor owner (`TDSRow`, `execute::Row`) in a type position, late-bound but keeping
    /// the owner's class in its template.
    pub fn erased_row<A: Alloc>(arena: &'a A, owner_fqn: &'a str) -> &'a RelationType<'a> {
        let template = Column {
            name: LATE_BOUND_WILDCARD,
            r#type: Type::ClassType(owner_fqn),
            multiplicity: Multiplicity::ZERO_ONE,
        };
        arena.alloc(RelationType {
            columns: &[],
            dynamic_columns: arena.alloc_slice(&[template]),
        })
    }

    /// No columns and one template named `*`: a raw-SQL grid or an erased row.
    pub fn is_late_bound(&self) -> bool {
        self.columns.is_empty()
            && self.dynamic_columns.len() == 1
            && self.dynamic_columns[0].name == LATE_BOUND_WILDCARD
    }

    /// Late-bound, its template not the grid's: read only through its owner's accessors.
    pub fn is_erased_row(&self) -> bool {
        self.is_late_bound() && self.dynamic_columns[0] != GRID_TEMPLATE[0]
    }

    /// A pivot column's presented name: quoted where it carries the separator and is not quoted already.
    pub fn present_pivot_name(physical: &str) -> String {
        lite_sql::pivot_presented(physical)
    }

    /// The pivot-column rule: a known column by name; a `<value>__|__<template>` name its template's type; a
    /// separated name that no template matches, while there are templates, refused; `None` for no match. The name
    /// may carry the quotes it is presented with.
    pub fn pivot_column_type(&self, raw_name: &str) -> Result<Option<Type<'a>>, TypeError> {
        let name = if raw_name.len() >= 2
            && raw_name.starts_with('\'')
            && raw_name.ends_with('\'')
            && raw_name.contains(PIVOT_SEPARATOR)
        {
            &raw_name[1..raw_name.len() - 1]
        } else {
            raw_name
        };
        if let Some(c) = self.columns.iter().find(|c| c.name == name || c.name == raw_name) {
            return Ok(Some(c.r#type));
        }
        if let Some(sep) = name.rfind(PIVOT_SEPARATOR)
            && !self.dynamic_columns.is_empty()
        {
            let template = &name[sep + PIVOT_SEPARATOR.len()..];
            return match self.dynamic_columns.iter().find(|c| c.name == template) {
                Some(c) => Ok(Some(c.r#type)),
                None => Err(TypeError::state(format!(
                    "pivot column '{name}' matches no aggregate template [{}]",
                    self.dynamic_columns
                        .iter()
                        .map(|c| c.name)
                        .collect::<Vec<_>>()
                        .join(", ")
                ))),
            };
        }
        Ok(None)
    }

    /// The trust-name rule: a by-name read over a late-bound schema, typed `Any[0..1]`.
    pub fn trusted_column(name: &'a str) -> Column<'a> {
        Column {
            name,
            r#type: Type::ClassType(platform::ANY),
            multiplicity: Multiplicity::ZERO_ONE,
        }
    }
}

/// The first name two columns share, Java's `HashSet.add` order: the first column whose name was seen before.
fn first_repeat<'a>(columns: &[Column<'a>]) -> Option<&'a str> {
    let mut seen = std::collections::HashSet::new();
    columns.iter().map(|c| c.name).find(|name| !seen.insert(*name))
}

impl<'a> Type<'a> {
    /// The table type, `Relation<schema>`: pure's spelling, the one mint.
    pub fn relation<A: Alloc>(arena: &'a A, schema: &'a RelationType<'a>) -> Type<'a> {
        Type::GenericType(GenericType::new(
            arena,
            platform::RELATION,
            &[Type::RelationType(schema)],
            &[],
        ))
    }

    /// A relation (table) type: a relation carrier with one argument, whatever the argument.
    pub fn is_relation(self) -> bool {
        matches!(self, Type::GenericType(g) if platform::RELATION_CARRIERS.contains(&g.raw_fqn) && g.arguments.len() == 1)
    }

    /// The class a value of this type is an instance of: a class's FQN, or a parameterized class's raw FQN unless
    /// it is a relation or a value carrier; `None` otherwise.
    pub fn class_fqn(self) -> Option<&'a str> {
        match self {
            Type::ClassType(fqn) => Some(fqn),
            Type::GenericType(g) if !self.is_relation() && !platform::is_value_carrier(self) => Some(g.raw_fqn),
            _ => None,
        }
    }

    /// [`Type::class_fqn`] as a class type: the class itself, a parameterized class raw.
    pub fn as_class_type(self) -> Option<Type<'a>> {
        match self {
            Type::ClassType(_) => Some(self),
            _ => self.class_fqn().map(Type::ClassType),
        }
    }

    /// The schema of a table type `Relation<schema>`; `None` for anything else, a bare schema included.
    pub fn relation_schema(self) -> Option<&'a RelationType<'a>> {
        match self {
            Type::GenericType(g) if self.is_relation() => match g.arguments[0] {
                Type::RelationType(r) => Some(r),
                _ => None,
            },
            _ => None,
        }
    }

    /// The schema a relation-ish type shows: a table's schema, or a bare schema itself.
    pub fn schema_view(self) -> Option<&'a RelationType<'a>> {
        self.relation_schema().or(match self {
            Type::RelationType(r) => Some(r),
            _ => None,
        })
    }

    /// The schema of a type known to be a table; refused for anything else.
    pub fn require_relation_schema(self) -> Result<&'a RelationType<'a>, TypeError> {
        self.relation_schema().ok_or_else(|| {
            TypeError::state(format!(
                "expected a table type (Relation<schema>), got {}",
                self.type_name()
            ))
        })
    }

    /// Whether a declaration leaves a cell's kind to the wire: `String`, unrefined `Number`, `Any`.
    pub fn wire_decided(self) -> bool {
        self == Type::Primitive(Primitive::String)
            || self == Type::Primitive(Primitive::Number)
            || platform::is_any(self)
    }

    /// The pure kind a wire (SQL) type spells; `None` for one it spells none for.
    pub fn kind_of_sql_type(t: &SqlType) -> Option<Type<'static>> {
        let p = match t {
            SqlType::Scalar(Scalar::Bigint | Scalar::Integer | Scalar::Hugeint) => Primitive::Integer,
            SqlType::Scalar(Scalar::Double) => Primitive::Float,
            SqlType::Scalar(Scalar::Boolean) => Primitive::Boolean,
            SqlType::Scalar(Scalar::Varchar) => Primitive::String,
            SqlType::Decimal { .. } => Primitive::Decimal,
            SqlType::Scalar(Scalar::Date) => Primitive::StrictDate,
            SqlType::Scalar(Scalar::Time) => Primitive::StrictTime,
            SqlType::Scalar(Scalar::TemporalText) => Primitive::Date,
            SqlType::Scalar(Scalar::Timestamp | Scalar::Timestamptz) => Primitive::DateTime,
            SqlType::Scalar(Scalar::Json | Scalar::DecimalText)
            | SqlType::Array { .. }
            | SqlType::Map { .. }
            | SqlType::Struct { .. }
            | SqlType::Tagged { .. } => return None,
        };
        Some(Type::Primitive(p))
    }

    /// A relation-rooted value by its type: a table, or a schema-typed value that is many (the `.rows` view); one
    /// schema-typed row is not.
    pub fn relation_valued(info: ExprType<'_>) -> bool {
        info.r#type.is_relation() || (matches!(info.r#type, Type::RelationType(_)) && info.multiplicity.is_many())
    }

    /// The readable spelling, `Integer`, `Decimal(38,2)`, `Relation<T>` (Java's `typeName()`).
    pub fn type_name(self) -> String {
        self.to_string()
    }
}

impl fmt::Display for Type<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Type::Primitive(p) => f.write_str(p.type_name()),
            Type::PrecisionDecimal(d) => f.write_str(&d.type_name()),
            Type::ClassType(fqn) | Type::EnumType(fqn) => f.write_str(fqn),
            Type::TypeVar(name) => f.write_str(name),
            Type::GenericType(g) => {
                write!(f, "{}<", simple_name(g.raw_fqn))?;
                for (i, a) in g.arguments.iter().enumerate() {
                    if i > 0 {
                        f.write_str(", ")?;
                    }
                    write!(f, "{a}")?;
                }
                f.write_str(">")
            }
            Type::FunctionType(ft) => {
                f.write_str("{")?;
                for (i, p) in ft.params.iter().enumerate() {
                    if i > 0 {
                        f.write_str(", ")?;
                    }
                    write!(f, "{p}")?;
                }
                write!(f, " -> {}}}", ft.result)
            }
            Type::RelationType(r) => {
                f.write_str("(")?;
                for (i, c) in r.columns.iter().enumerate() {
                    if i > 0 {
                        f.write_str(", ")?;
                    }
                    write!(f, "{c}")?;
                }
                f.write_str(")")
            }
            Type::SchemaAlgebra(s) => write!(f, "{}{}{}", s.left, s.op.symbol(), s.right),
        }
    }
}

impl fmt::Display for Param<'_> {
    /// `Integer[1]` (Java's `Param.text()`).
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}{}", self.r#type, self.multiplicity)
    }
}

impl fmt::Display for Column<'_> {
    /// `price:Decimal(38,2)[1]` (Java's `Column.text()`).
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}{}", self.name, self.r#type, self.multiplicity)
    }
}

impl From<Bounded> for Multiplicity<'_> {
    fn from(b: Bounded) -> Self {
        Multiplicity::Bounded(b)
    }
}
