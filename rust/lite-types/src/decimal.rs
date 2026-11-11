// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! Java's `Type.PrecisionDecimal`: `DECIMAL(p, s)`, its subtyping baked in (it collapses to `Decimal` in the one
//! subtype routine, by [`PrecisionDecimal::base_primitive`]), and the arithmetic's precision and scale (the MS-SQL,
//! Hive, Spark, Calcite rule, clamped by Spark's `adjustPrecisionScale`). The arithmetic is Java's `int`
//! arithmetic, wrapping, so a precision past an int is refused as Java refuses it.

use crate::error::TypeError;
use crate::primitive::Primitive;

/// `DECIMAL(precision, scale)`: `precision >= 0`, `0 <= scale <= precision`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PrecisionDecimal {
    precision: i32,
    scale: i32,
}

impl PrecisionDecimal {
    /// Pure's unparameterized `Decimal`: `DECIMAL(38, 18)`.
    pub const DEFAULT_DECIMAL: PrecisionDecimal = PrecisionDecimal {
        precision: 38,
        scale: 18,
    };
    /// The widest SQL `DECIMAL` a 128-bit backing integer holds.
    pub const MAX_PRECISION: i32 = 38;
    /// The scale kept when an overflowing precision is clamped (Spark's `MINIMUM_ADJUSTED_SCALE`).
    pub const MIN_ADJUSTED_SCALE: i32 = 6;

    pub fn new(precision: i32, scale: i32) -> Result<PrecisionDecimal, TypeError> {
        if precision < 0 {
            return Err(TypeError::argument(format!("precision must be >= 0, got {precision}")));
        }
        if scale < 0 || scale > precision {
            return Err(TypeError::argument(format!(
                "scale must be in [0, precision], got scale={scale}, precision={precision}"
            )));
        }
        Ok(PrecisionDecimal { precision, scale })
    }

    pub fn precision(self) -> i32 {
        self.precision
    }

    pub fn scale(self) -> i32 {
        self.scale
    }

    /// The primitive a precision decimal collapses to for subtyping.
    pub fn base_primitive(self) -> Primitive {
        Primitive::Decimal
    }

    /// `this + other`: precision `max(s1,s2) + max(p1-s1, p2-s2) + 1`, scale `max(s1,s2)`.
    pub fn plus(self, other: PrecisionDecimal) -> Result<PrecisionDecimal, TypeError> {
        let (p1, s1, p2, s2) = (self.precision, self.scale, other.precision, other.scale);
        let scale = s1.max(s2);
        let precision = s1
            .max(s2)
            .wrapping_add(p1.wrapping_sub(s1).max(p2.wrapping_sub(s2)))
            .wrapping_add(1);
        adjust(precision, scale)
    }

    /// `this - other`: as [`PrecisionDecimal::plus`].
    pub fn minus(self, other: PrecisionDecimal) -> Result<PrecisionDecimal, TypeError> {
        self.plus(other)
    }

    /// `this * other`: precision `p1 + p2 + 1`, scale `s1 + s2`.
    pub fn times(self, other: PrecisionDecimal) -> Result<PrecisionDecimal, TypeError> {
        adjust(
            self.precision.wrapping_add(other.precision).wrapping_add(1),
            self.scale.wrapping_add(other.scale),
        )
    }

    /// `this / other`: scale `max(6, s1 + p2 + 1)`, precision `p1 - s1 + s2 + scale`.
    pub fn divided_by(self, other: PrecisionDecimal) -> Result<PrecisionDecimal, TypeError> {
        let (p1, s1, p2, s2) = (self.precision, self.scale, other.precision, other.scale);
        let scale = PrecisionDecimal::MIN_ADJUSTED_SCALE.max(s1.wrapping_add(p2).wrapping_add(1));
        let precision = p1.wrapping_sub(s1).wrapping_add(s2).wrapping_add(scale);
        adjust(precision, scale)
    }

    /// `Decimal(p,s)` (Java's `typeName()`).
    pub fn type_name(self) -> String {
        format!("Decimal({},{})", self.precision, self.scale)
    }
}

/// Spark's `adjustPrecisionScale`: a precision past 38 is capped at 38, the scale then
/// `max(38 - integer digits, min(scale, 6))`.
fn adjust(precision: i32, scale: i32) -> Result<PrecisionDecimal, TypeError> {
    if precision <= PrecisionDecimal::MAX_PRECISION {
        return PrecisionDecimal::new(precision, scale);
    }
    let int_digits = precision.wrapping_sub(scale);
    let reduced = PrecisionDecimal::MAX_PRECISION
        .wrapping_sub(int_digits)
        .max(scale.min(PrecisionDecimal::MIN_ADJUSTED_SCALE));
    PrecisionDecimal::new(PrecisionDecimal::MAX_PRECISION, reduced)
}

#[cfg(test)]
mod tests {
    use super::PrecisionDecimal;

    fn d(p: i32, s: i32) -> PrecisionDecimal {
        PrecisionDecimal::new(p, s).unwrap()
    }

    #[test]
    fn the_clamp_keeps_the_smaller_of_the_scale_and_six() {
        // Java's worked examples: (49,15) -> (38,6), (49,5) -> (38,5)
        assert_eq!(d(24, 10).times(d(24, 5)).unwrap(), d(38, 6));
        assert_eq!(d(24, 5).times(d(24, 0)).unwrap(), d(38, 5));
    }
}
