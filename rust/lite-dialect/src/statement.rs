// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! Java's `RenderedStatement` and its records: a statement's SQL text, its bound parameters in placeholder order, and
//! its per-key regions. Every position is Java's -- a UTF-16 index into the text -- and every record's `Display` is
//! Java's record `toString` (`Bind[parameter=age, binding=One[nullType=BIGINT, hole=null]]`), which refusal messages
//! embed and the golden dump records.

use crate::refusal::Refusal;
use lite_sql::ValueKind;
use std::fmt;

/// A statement as a dialect renders it for execution with bound values (Java's `RenderedStatement`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderedStatement {
    pub sql: String,
    pub binds: Vec<Bind>,
    pub regions: Vec<Region>,
}

impl RenderedStatement {
    /// Java's canonical constructor, with its checks: each type hole after the previous one and within the text, and
    /// the regions as `Region.check` holds them.
    pub fn new(sql: String, binds: Vec<Bind>, regions: Vec<Region>) -> Result<RenderedStatement, Refusal> {
        let length = utf16_len(&sql);
        let mut previous: i64 = -1;
        for b in &binds {
            if let Binding::One { hole: Some(hole), .. } = &b.binding {
                let at = hole.at as i64;
                if at <= previous || hole.at > length {
                    return Err(Refusal::illegal_argument(format!(
                        "type hole of '{}' at {at} is not after the previous one ({previous}) within the statement \
                         ({length})",
                        b.parameter
                    )));
                }
                previous = at;
            }
        }
        Region::check(length, &binds, &regions)?;
        Ok(RenderedStatement { sql, binds, regions })
    }

    /// A statement with no bound parameter and no region.
    pub fn plain(sql: String) -> RenderedStatement {
        RenderedStatement { sql, binds: Vec::new(), regions: Vec::new() }
    }
}

/// The text's length as Java's `String.length()` counts it: UTF-16 units.
pub fn utf16_len(s: &str) -> usize {
    if s.is_ascii() { s.len() } else { s.encode_utf16().count() }
}

/// What one placeholder binds (Java's `RenderedStatement.Bind`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bind {
    pub parameter: String,
    pub binding: Binding,
}

/// How a placeholder is bound (Java's `RenderedStatement.Binding`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Binding {
    /// One value; an absent one a null of `null_type` (a `java.sql.JDBCType`'s name).
    One { null_type: String, hole: Option<TypeHole> },
    /// The parameter's whole list, as one array of `element_sql_type`.
    Array { element_sql_type: String },
    /// A key's cell (a per-key region's placeholder).
    Key { cell: i32 },
}

impl Binding {
    /// Java's `new Binding.Key(cell)`, refused for a negative cell.
    pub fn key(cell: i32) -> Result<Binding, Refusal> {
        if cell < 0 {
            return Err(Refusal::illegal_argument(format!("a key's cell {cell}")));
        }
        Ok(Binding::Key { cell })
    }
}

/// Where a placeholder's type is its value's (Java's `RenderedStatement.TypeHole`): `types` in the kinds' declared
/// order (Java's `EnumMap`), one spelling per kind.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeHole {
    pub at: usize,
    pub types: Vec<(ValueKind, TypeSpelling)>,
    pub absent: ValueKind,
}

impl TypeHole {
    /// Java's constructor: `types` put into an `EnumMap` (a later spelling of a kind replaces an earlier one, and the
    /// kinds read in declared order), refused when empty or without the absent kind's spelling.
    pub fn new(at: usize, types: Vec<(ValueKind, TypeSpelling)>, absent: ValueKind) -> Result<TypeHole, Refusal> {
        if types.is_empty() {
            return Err(Refusal::illegal_argument("a type hole with no spelling"));
        }
        let types = enum_map(types);
        if !types.iter().any(|(k, _)| *k == absent) {
            return Err(Refusal::illegal_argument(format!(
                "an absent value's kind {absent} has no spelling in {}",
                map_text(&types)
            )));
        }
        Ok(TypeHole { at, types, absent })
    }
}

/// Entries as a Java `EnumMap` holds them: one per kind, the last put winning, in declared order.
pub fn enum_map(entries: Vec<(ValueKind, TypeSpelling)>) -> Vec<(ValueKind, TypeSpelling)> {
    let mut out: Vec<(ValueKind, TypeSpelling)> = Vec::new();
    for (k, v) in entries {
        match out.iter_mut().find(|(have, _)| *have == k) {
            Some((_, slot)) => *slot = v,
            None => out.push((k, v)),
        }
    }
    let ordinal = |k: ValueKind| ValueKind::ALL.iter().position(|x| *x == k).unwrap_or(usize::MAX);
    out.sort_by_key(|(k, _)| ordinal(*k));
    out
}

/// A type's spelling for one kind of value (Java's `RenderedStatement.TypeSpelling`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeSpelling {
    pub name: String,
    pub digits: Digits,
    pub fraction: Option<i32>,
}

impl TypeSpelling {
    /// Java's two-argument constructor: no fraction.
    pub fn new(name: impl Into<String>, digits: Digits) -> TypeSpelling {
        TypeSpelling { name: name.into(), digits, fraction: None }
    }
}

/// The value's own digits a type spelling takes (Java's `RenderedStatement.Digits`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Digits {
    None,
    Precision,
    PrecisionAndScale,
}

impl Digits {
    pub fn as_str(self) -> &'static str {
        match self {
            Digits::None => "NONE",
            Digits::Precision => "PRECISION",
            Digits::PrecisionAndScale => "PRECISION_AND_SCALE",
        }
    }

    /// The constant whose Java `name()` is `name`.
    pub fn from_name(name: &str) -> Option<Digits> {
        [Digits::None, Digits::Precision, Digits::PrecisionAndScale].into_iter().find(|d| d.as_str() == name)
    }
}

/// A run of the statement written once per key of its plan's keys probe (Java's `RenderedStatement.Region`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Region {
    pub start: usize,
    pub end: usize,
    pub lead: String,
    pub separator: String,
    pub empty: Option<String>,
    pub first_bind: usize,
    pub bind_count: usize,
    pub counters: Vec<usize>,
}

impl Region {
    /// Java's constructor, with its checks (positions are unsigned here, so only the ordering ones remain).
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        start: usize,
        end: usize,
        lead: String,
        separator: String,
        empty: Option<String>,
        first_bind: usize,
        bind_count: usize,
        counters: Vec<usize>,
    ) -> Result<Region, Refusal> {
        if end < start {
            return Err(Refusal::illegal_argument(format!("a per-key region [{start}, {end})")));
        }
        for &at in &counters {
            if at < start || at > end {
                return Err(Refusal::illegal_argument(format!(
                    "a key's position at {at} outside its region [{start}, {end})"
                )));
            }
        }
        Ok(Region { start, end, lead, separator, empty, first_bind, bind_count, counters })
    }

    /// Java's `Region.check`: the regions in order within a statement of `length`, each one's placeholders among
    /// `binds`, and every key's cell bound inside one.
    pub fn check(length: usize, binds: &[Bind], regions: &[Region]) -> Result<(), Refusal> {
        let mut previous_end = 0;
        let mut previous_binds = 0;
        let mut in_region = vec![false; binds.len()];
        for r in regions {
            if r.start < previous_end || r.end > length {
                return Err(Refusal::illegal_argument(format!(
                    "per-key region [{}, {}) is not after the previous one ({previous_end}) within the statement \
                     ({length})",
                    r.start, r.end
                )));
            }
            if r.first_bind < previous_binds || r.first_bind + r.bind_count > binds.len() {
                return Err(Refusal::illegal_argument(format!(
                    "per-key region's placeholders {}..{} are not after the previous region's ({previous_binds}) \
                     among the statement's {}",
                    r.first_bind,
                    r.first_bind + r.bind_count,
                    binds.len()
                )));
            }
            for slot in &mut in_region[r.first_bind..r.first_bind + r.bind_count] {
                *slot = true;
            }
            previous_end = r.end;
            previous_binds = r.first_bind + r.bind_count;
        }
        for (i, b) in binds.iter().enumerate() {
            if matches!(b.binding, Binding::Key { .. }) && !in_region[i] {
                return Err(Refusal::illegal_argument(format!(
                    "placeholder {} binds a key's cell outside a per-key region: no key to bind",
                    i + 1
                )));
            }
        }
        Ok(())
    }
}

/// `[a, b]`: a Java `List`'s `toString`.
pub fn list_text<T: fmt::Display>(items: &[T]) -> String {
    let mut out = String::from("[");
    for (i, x) in items.iter().enumerate() {
        if i > 0 {
            out.push_str(", ");
        }
        out.push_str(&x.to_string());
    }
    out.push(']');
    out
}

/// `{K=V, K=V}`: a Java `Map`'s `toString`.
fn map_text(entries: &[(ValueKind, TypeSpelling)]) -> String {
    let mut out = String::from("{");
    for (i, (k, v)) in entries.iter().enumerate() {
        if i > 0 {
            out.push_str(", ");
        }
        out.push_str(&format!("{k}={v}"));
    }
    out.push('}');
    out
}

fn nullable<T: fmt::Display>(x: &Option<T>) -> String {
    x.as_ref().map_or_else(|| "null".to_owned(), T::to_string)
}

impl fmt::Display for Bind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Bind[parameter={}, binding={}]", self.parameter, self.binding)
    }
}

impl fmt::Display for Binding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Binding::One { null_type, hole } => write!(f, "One[nullType={null_type}, hole={}]", nullable(hole)),
            Binding::Array { element_sql_type } => write!(f, "Array[elementSqlType={element_sql_type}]"),
            Binding::Key { cell } => write!(f, "Key[cell={cell}]"),
        }
    }
}

impl fmt::Display for TypeHole {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "TypeHole[at={}, types={}, absent={}]", self.at, map_text(&self.types), self.absent)
    }
}

impl fmt::Display for TypeSpelling {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "TypeSpelling[name={}, digits={}, fraction={}]",
            self.name,
            self.digits.as_str(),
            nullable(&self.fraction)
        )
    }
}

impl fmt::Display for Region {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Region[start={}, end={}, lead={}, separator={}, empty={}, firstBind={}, bindCount={}, counters={}]",
            self.start,
            self.end,
            self.lead,
            self.separator,
            nullable(&self.empty),
            self.first_bind,
            self.bind_count,
            list_text(&self.counters)
        )
    }
}
