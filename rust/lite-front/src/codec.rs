// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! The lossless record codec's writer (docs/rust-port/PHASE2.md §3), byte for byte the JVM's
//! (`core/src/test/java/com/legend/frontoracle/RecordCodec.java`): a record is its components in canonical order,
//! nulls explicit, with an `"@"` type tag first only where the declared type does not fix the runtime class; a list
//! an array; a string-keyed map an object in its order; `double`, `BigDecimal` and `BigInteger` strings in Java's
//! spelling; a string escapes `"` and `\`, the short escapes, other controls and every surrogate as `\uXXXX`. Each
//! record's writer is generated (`records.rs`); this is what they call.

use crate::jtypes::{BigDecimal, JStr, JValue};

/// A value the codec writes; `exact` is whether its slot's declared type fixes its class (no tag then).
pub trait Codec {
    fn write(&self, w: &mut Writer, exact: bool);
}

/// The codec's output: JSON text with every surrogate escaped, so it is valid UTF-8.
#[derive(Default)]
pub struct Writer {
    out: String,
}

impl Writer {
    pub fn new() -> Writer {
        Writer::default()
    }

    pub fn finish(self) -> String {
        self.out
    }

    pub fn raw(&mut self, s: &str) {
        self.out.push_str(s);
    }

    pub fn int(&mut self, v: i64) {
        self.out.push_str(&v.to_string());
    }

    pub fn boolean(&mut self, v: bool) {
        self.out.push_str(if v { "true" } else { "false" });
    }

    /// A double in an exact slot: the string of Java's `Double.toString`.
    pub fn double(&mut self, v: f64) {
        self.str_units_of(&lite_javacompat::double_to_string(v));
    }

    pub fn decimal(&mut self, d: &BigDecimal) {
        self.str_units_of(&d.to_java_string());
    }

    pub fn string(&mut self, s: &JStr) {
        self.units(&s.0);
    }

    /// A leaf in an open slot (`Number`, `Object`): tagged by its Java class, `{"@":class,"$":value}`.
    pub fn value(&mut self, v: &JValue) {
        self.raw("{\"@\":");
        self.str_units_of(v.java_class());
        self.raw(",\"$\":");
        match v {
            JValue::String(s) => self.string(s),
            JValue::Long(n) => self.int(*n),
            JValue::Integer(n) => self.int(i64::from(*n)),
            JValue::Boolean(b) => self.boolean(*b),
            JValue::Double(d) => self.double(*d),
            JValue::BigInteger(digits) => self.str_units_of(digits),
            JValue::BigDecimal(d) => self.decimal(d),
        }
        self.raw("}");
    }

    fn str_units_of(&mut self, s: &str) {
        let units: Vec<u16> = s.encode_utf16().collect();
        self.units(&units);
    }

    /// A JSON string of UTF-16 units (RecordCodec.string).
    fn units(&mut self, s: &[u16]) {
        self.out.push('"');
        for &c in s {
            match c {
                0x22 => self.out.push_str("\\\""),
                0x5C => self.out.push_str("\\\\"),
                0x0A => self.out.push_str("\\n"),
                0x0D => self.out.push_str("\\r"),
                0x09 => self.out.push_str("\\t"),
                0x08 => self.out.push_str("\\b"),
                0x0C => self.out.push_str("\\f"),
                _ if c < 0x20 || (0xD800..=0xDFFF).contains(&c) => {
                    self.out.push_str(&format!("\\u{c:04x}"));
                }
                _ => self
                    .out
                    .push(char::from_u32(u32::from(c)).expect("a non-surrogate unit is a char")),
            }
        }
        self.out.push('"');
    }
}

/// One answer, written: `exact` as its door's declared type fixes it (a door returning a sealed parent is not exact).
pub fn write<T: Codec + ?Sized>(value: &T, exact: bool) -> String {
    let mut w = Writer::new();
    value.write(&mut w, exact);
    w.finish()
}
