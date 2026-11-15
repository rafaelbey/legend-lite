// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! Java's `Multiplicity`: fixed bounds or a variable, the one algebra over them (range union, path product), the
//! one reading of a multiplicity argument's spelling, and (2026-11-15, the element slice, step 3) the one conversion
//! from the protocol's multiplicity, `Multiplicity.from`.

use std::fmt;

use lite_base::Alloc;
use lite_front::records as p;

use crate::error::TypeError;

/// A multiplicity: `[1]`, `[0..1]`, `[*]`, ... or a variable's `[m]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Multiplicity<'a> {
    Bounded(Bounded),
    Var(Var<'a>),
}

/// Fixed bounds: `lower >= 0`, `upper` `None` for `*`, else `>= lower` (Java's `Multiplicity.Bounded`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Bounded {
    lower: i32,
    upper: Option<i32>,
}

/// A multiplicity variable, the `m` of `letFunction<T|m>(...): T[m]`; its name is not empty (Java's
/// `Multiplicity.Var`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Var<'a> {
    name: &'a str,
}

impl Bounded {
    /// `[1]`
    pub const ONE: Bounded = Bounded {
        lower: 1,
        upper: Some(1),
    };
    /// `[0..1]`
    pub const ZERO_ONE: Bounded = Bounded {
        lower: 0,
        upper: Some(1),
    };
    /// `[1..*]`
    pub const ONE_MANY: Bounded = Bounded { lower: 1, upper: None };
    /// `[*]`
    pub const ZERO_MANY: Bounded = Bounded { lower: 0, upper: None };

    pub fn new(lower: i32, upper: Option<i32>) -> Result<Bounded, TypeError> {
        if lower < 0 {
            return Err(TypeError::argument(format!("lower must be >= 0, got {lower}")));
        }
        if let Some(u) = upper
            && u < lower
        {
            return Err(TypeError::argument(format!("upper ({u}) must be >= lower ({lower})")));
        }
        Ok(Bounded { lower, upper })
    }

    pub fn lower(self) -> i32 {
        self.lower
    }

    pub fn upper(self) -> Option<i32> {
        self.upper
    }

    pub fn is_unbounded(self) -> bool {
        self.upper.is_none()
    }

    pub fn is_to_one(self) -> bool {
        self.upper == Some(1)
    }

    /// The source spelling, `[1]`, `[0..1]`, `[*]`, `[1..*]`, `[3..7]` (Java's `text()`).
    pub fn text(self) -> String {
        match self.upper {
            None if self.lower == 0 => "[*]".to_owned(),
            None => format!("[{}..*]", self.lower),
            Some(u) if u == self.lower => format!("[{u}]"),
            Some(u) => format!("[{}..{u}]", self.lower),
        }
    }
}

impl<'a> Var<'a> {
    pub fn new(name: &'a str) -> Result<Var<'a>, TypeError> {
        if name.is_empty() {
            return Err(TypeError::argument(
                "multiplicity parameter name must be non-empty".to_owned(),
            ));
        }
        Ok(Var { name })
    }

    pub fn name(self) -> &'a str {
        self.name
    }
}

impl<'a> Multiplicity<'a> {
    pub const ONE: Multiplicity<'static> = Multiplicity::Bounded(Bounded::ONE);
    pub const ZERO_ONE: Multiplicity<'static> = Multiplicity::Bounded(Bounded::ZERO_ONE);
    pub const ONE_MANY: Multiplicity<'static> = Multiplicity::Bounded(Bounded::ONE_MANY);
    pub const ZERO_MANY: Multiplicity<'static> = Multiplicity::Bounded(Bounded::ZERO_MANY);

    /// Whether more than one value is admitted. A variable is not "many": the checkers ask this of unresolved
    /// signatures (Java's `isMany` and its written-down decision).
    pub fn is_many(self) -> bool {
        matches!(self, Multiplicity::Bounded(b) if b.upper.is_none_or(|u| u > 1))
    }

    /// The bounds, or a refusal naming `place`: past G a variable is a type-checker bug (Java's `requireBounded`).
    pub fn require_bounded(self, place: &str) -> Result<Bounded, TypeError> {
        match self {
            Multiplicity::Bounded(b) => Ok(b),
            Multiplicity::Var(v) => Err(TypeError::state(format!(
                "unresolved multiplicity variable reached {place}: {}",
                v.record_text()
            ))),
        }
    }

    /// The range union `[min(a,c) .. max(b,d)]`, an unbounded upper absorbing; one variable on both sides is that
    /// variable, and a variable meeting anything else is refused (Java's `union`).
    pub fn union(a: Multiplicity<'a>, b: Multiplicity<'a>) -> Result<Multiplicity<'a>, TypeError> {
        match (a, b) {
            (Multiplicity::Bounded(x), Multiplicity::Bounded(y)) => Ok(Multiplicity::Bounded(Bounded {
                lower: x.lower.min(y.lower),
                upper: match (x.upper, y.upper) {
                    (Some(p), Some(q)) => Some(p.max(q)),
                    _ => None,
                },
            })),
            (Multiplicity::Var(va), Multiplicity::Var(vb)) if va.name == vb.name => Ok(a),
            _ => Err(TypeError::state(format!(
                "cannot union multiplicities {} and {} (an unresolved variable met a different bound)",
                a.text(),
                b.text()
            ))),
        }
    }

    /// The path product `[a*c .. b*d]`: a `[0..0]` side annihilates; exact arithmetic, an upper past an int
    /// unbounded and a lower saturating; `[1]` is the identity beside a variable, any other variable product refused
    /// (Java's `product`).
    pub fn product(outer: Multiplicity<'a>, inner: Multiplicity<'a>) -> Result<Multiplicity<'a>, TypeError> {
        if let (Multiplicity::Bounded(a), Multiplicity::Bounded(b)) = (outer, inner) {
            let zero = a.upper == Some(0) || b.upper == Some(0);
            let lower = i64::from(a.lower) * i64::from(b.lower);
            let upper = if zero {
                Some(0)
            } else {
                match (a.upper, b.upper) {
                    (Some(p), Some(q)) => Some(i64::from(p) * i64::from(q)),
                    _ => None,
                }
            };
            let upper = upper.and_then(|u| i32::try_from(u).ok());
            let lower = i32::try_from(lower.min(i64::from(i32::MAX))).expect("saturated at i32::MAX");
            return Ok(Multiplicity::Bounded(Bounded { lower, upper }));
        }
        if inner == Multiplicity::ONE {
            return Ok(outer);
        }
        if outer == Multiplicity::ONE {
            return Ok(inner);
        }
        Err(TypeError::state(format!(
            "cannot compose multiplicities {} . {} (an unresolved variable met a non-identity bound)",
            outer.text(),
            inner.text()
        )))
    }

    /// `Multiplicity.from(m)` (Multiplicity.java:55 at 18feff3fe): a parse-time multiplicity as the type family's,
    /// the one conversion point -- `Concrete` through [`Bounded::new`] (Java's `new Bounded(lower, upper)`: its two
    /// `IllegalArgumentException`s, "lower must be >= 0, got L" and "upper (U) must be >= lower (L)"), `Parameter`
    /// through [`Var::new`] (its "multiplicity parameter name must be non-empty"). The variable's name is put in
    /// `arena`, its UTF-16 read as UTF-8 (a lone surrogate would not survive: the parser admits none in a name).
    pub fn from<A: Alloc>(arena: &'a A, m: &p::Multiplicity) -> Result<Multiplicity<'a>, TypeError> {
        match m {
            p::Multiplicity::MultiplicityConcrete(c) => {
                Ok(Multiplicity::Bounded(Bounded::new(c.lower_bound, c.upper_bound)?))
            }
            p::Multiplicity::MultiplicityParameter(v) => {
                Ok(Multiplicity::Var(Var::new(arena.alloc_str(&v.name.lossy()))?))
            }
        }
    }

    /// The source spelling: a bound's `text()`, a variable's `[m]`.
    pub fn text(self) -> String {
        match self {
            Multiplicity::Bounded(b) => b.text(),
            Multiplicity::Var(v) => format!("[{}]", v.name),
        }
    }

    /// One multiplicity argument's spelling (`Result<T|m>`'s `m`, `|1`, `|0..1`, `|*`): Java's `strip()`, then `*`,
    /// digits, or `digits..digits` / `digits..*` by shape, and anything else a variable's name (Java's `ofArgument`;
    /// the variable's name is a slice of `spelling`).
    pub fn of_argument(spelling: &'a str) -> Result<Multiplicity<'a>, TypeError> {
        let s = java_strip(spelling);
        if s == "*" {
            return Ok(Multiplicity::ZERO_MANY);
        }
        if all_ascii_digits(s) {
            let n = parse_int(s)?;
            return Ok(Multiplicity::Bounded(Bounded::new(n, Some(n))?));
        }
        if let Some((low, up)) = s.split_once("..")
            && all_ascii_digits(low)
            && (up == "*" || all_ascii_digits(up))
        {
            let lower = parse_int(low)?;
            let upper = if up == "*" { None } else { Some(parse_int(up)?) };
            return Ok(Multiplicity::Bounded(Bounded::new(lower, upper)?));
        }
        Ok(Multiplicity::Var(Var::new(s)?))
    }
}

impl Var<'_> {
    /// Java's record `toString()`, which a refusal's message carries.
    fn record_text(self) -> String {
        format!("Var[name={}]", self.name)
    }
}

impl fmt::Display for Multiplicity<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.text())
    }
}

/// `[0-9]+`: Java's regex class is ASCII's digits alone.
fn all_ascii_digits(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit())
}

/// `Integer.parseInt` of an ASCII digit string.
fn parse_int(digits: &str) -> Result<i32, TypeError> {
    digits.parse::<i32>().map_err(|_| TypeError::number(digits))
}

/// Java's `String.strip()`: leading and trailing `Character.isWhitespace` code points off (not Rust's `trim`, whose
/// white space holds U+0085 and U+00A0 and lacks U+001C to U+001F).
fn java_strip(s: &str) -> &str {
    s.trim_matches(|c: char| lite_javacompat::character::is_whitespace(c))
}

#[cfg(test)]
mod tests {
    use lite_base::QueryArena;
    use lite_front::jtypes::JStr;
    use lite_front::records as p;

    use super::{Bounded, Multiplicity};
    use crate::error::Thrown;

    fn concrete(lower_bound: i32, upper_bound: Option<i32>) -> p::Multiplicity {
        p::Multiplicity::MultiplicityConcrete(Box::new(p::MultiplicityConcrete {
            lower_bound,
            upper_bound,
        }))
    }

    fn parameter(name: &str) -> p::Multiplicity {
        p::Multiplicity::MultiplicityParameter(Box::new(p::MultiplicityParameter { name: JStr::from(name) }))
    }

    #[test]
    fn from_converts_a_concrete_multiplicity_through_the_bounds_checks() {
        let cx = QueryArena::new();
        assert_eq!(
            Multiplicity::from(&cx, &concrete(0, Some(1))).unwrap(),
            Multiplicity::ZERO_ONE
        );
        assert_eq!(
            Multiplicity::from(&cx, &concrete(1, None)).unwrap(),
            Multiplicity::ONE_MANY
        );
        assert_eq!(Multiplicity::from(&cx, &concrete(3, Some(7))).unwrap().text(), "[3..7]");
        // Java's `new Bounded(2, 1)`: the upper bound's refusal, word for word
        let upper = Multiplicity::from(&cx, &concrete(2, Some(1))).unwrap_err();
        assert_eq!(
            (upper.thrown, upper.message.as_str()),
            (Thrown::IllegalArgument, "upper (1) must be >= lower (2)")
        );
        // Java's `new Bounded(-1, null)`: the lower bound's, checked first (a negative lower with an upper below it
        // is the lower's refusal too)
        let lower = Multiplicity::from(&cx, &concrete(-1, None)).unwrap_err();
        assert_eq!(
            (lower.thrown, lower.message.as_str()),
            (Thrown::IllegalArgument, "lower must be >= 0, got -1")
        );
        let both = Multiplicity::from(&cx, &concrete(-1, Some(-2))).unwrap_err();
        assert_eq!(both.message, "lower must be >= 0, got -1");
    }

    #[test]
    fn from_converts_a_parameter_to_a_variable_in_the_arena() {
        let cx = QueryArena::new();
        let m = Multiplicity::from(&cx, &parameter("m")).unwrap();
        match m {
            Multiplicity::Var(v) => assert_eq!(v.name(), "m"),
            Multiplicity::Bounded(_) => panic!("a parameter is a variable"),
        }
        assert_eq!(m.text(), "[m]");
        assert!(!m.is_many());
        // Java's `new Var("")`
        let empty = Multiplicity::from(&cx, &parameter("")).unwrap_err();
        assert_eq!(
            (empty.thrown, empty.message.as_str()),
            (Thrown::IllegalArgument, "multiplicity parameter name must be non-empty")
        );
    }

    #[test]
    fn the_bounds_spell_as_java_spells_them() {
        let spell = |l, u| Bounded::new(l, u).unwrap().text();
        assert_eq!(
            (spell(0, None), spell(2, None), spell(1, Some(1)), spell(3, Some(7))),
            (
                "[*]".to_owned(),
                "[2..*]".to_owned(),
                "[1]".to_owned(),
                "[3..7]".to_owned()
            )
        );
        assert_eq!(Multiplicity::of_argument(" 0..1 ").unwrap(), Multiplicity::ZERO_ONE);
    }
}
