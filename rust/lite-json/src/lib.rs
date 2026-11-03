// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! legend-lite's JSON, ported from `json/src/main/java/com/legend/json/Json.java` (docs/rust-port/TRIAGE.md: `json`).
//! What the port needs of it, and keeps exactly:
//!
//! - an object keeps its fields in the order written, and a field put again keeps its place (`LinkedHashMap`);
//! - a number keeps the token it was read as, so reading and writing a document changes no byte of it -- except an
//!   integer, written back from its value (`-0` becomes `0`, as `Long.toString` writes it);
//! - a number made from a double is written as Java's `writeDouble` writes it: integral below 10^19 as an integer
//!   (saturating, as Java's cast does), else `Double.toString`;
//! - strings are escaped by Java's one table: quote, backslash, the named controls, every other control as
//!   `\u00xx`, all else as is;
//! - the parser takes RFC 8259 strictly, nests at most 64 deep unless told otherwise, and words its errors as Java
//!   does (`<what> at line L col C`).
//!
//! One divergence, for TRIAGE C2 to settle: Java reads `\uXXXX` as one UTF-16 unit, so a lone surrogate survives into
//! its string; a Rust `String` cannot hold one, so a lone surrogate reads as U+FFFD. (The JVM's own UTF-8 encoder
//! sends such a string to Rust as `?`.)

mod parse;
mod write;

pub use parse::{DEFAULT_MAX_DEPTH, parse, parse_with_depth};
pub use write::{escape_to, to_compact};

/// A JSON value.
#[derive(Debug, Clone, PartialEq)]
pub enum Node {
    Obj(Obj),
    Arr(Vec<Node>),
    Str(String),
    Num(Num),
    Bool(bool),
    Null,
}

/// An object: its fields in the order written.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Obj {
    fields: Vec<(String, Node)>,
}

/// A number as written: its token, and what kind of number the token is.
#[derive(Debug, Clone)]
pub struct Num {
    token: String,
    kind: NumKind,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum NumKind {
    /// an integer that fits a long, written back from its value
    Integer(i64),
    /// an integer past a long, kept exact as its digits
    BigInteger,
    /// a fraction or an exponent, kept exact as its token
    Fraction(f64),
}

/// A parse or read failure: Java's `IllegalArgumentException` message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error(pub String);

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Error {}

impl Num {
    /// A long, as `Long.toString` writes it.
    pub fn from_i64(v: i64) -> Num {
        Num { token: v.to_string(), kind: NumKind::Integer(v) }
    }

    /// A double, as Java's `writeDouble` writes it: integral below 10^19 as an integer, else `Double.toString`.
    /// NaN and the infinities are no JSON number.
    pub fn from_f64(v: f64) -> Result<Num, Error> {
        if !v.is_finite() {
            return Err(Error("JSON cannot represent NaN or Infinity".to_owned()));
        }
        if v == v.floor() && v.abs() < 1e19 {
            // Java's (long) cast saturates past a long, as Rust's does
            return Ok(Num::from_i64(v as i64));
        }
        Ok(Num { token: lite_javacompat::double_to_string(v), kind: NumKind::Fraction(v) })
    }

    /// A number already in JSON's number grammar, read as the parser reads one.
    pub fn from_token(token: &str) -> Result<Num, Error> {
        match parse(token)? {
            Node::Num(n) => Ok(n),
            _ => Err(Error(format!("not a JSON number: {token}"))),
        }
    }

    /// The number's text, as it is written.
    pub fn token(&self) -> &str {
        &self.token
    }

    /// The value, when the number is an integer that fits a long.
    pub fn as_i64(&self) -> Option<i64> {
        match self.kind {
            NumKind::Integer(v) => Some(v),
            _ => None,
        }
    }

    /// The value as a double (the nearest, for an exact token).
    pub fn as_f64(&self) -> f64 {
        match self.kind {
            NumKind::Integer(v) => v as f64,
            NumKind::Fraction(v) => v,
            NumKind::BigInteger => self.token.parse().unwrap_or(f64::NAN),
        }
    }

    /// Whether the token is an integer (no fraction, no exponent).
    pub fn is_integer(&self) -> bool {
        !matches!(self.kind, NumKind::Fraction(_))
    }
}

/// Numbers are equal when they are the same number, however written (Java's `Num.equals` ignores the token).
impl PartialEq for Num {
    fn eq(&self, other: &Num) -> bool {
        match (self.kind, other.kind) {
            (NumKind::Integer(a), NumKind::Integer(b)) => a == b,
            (NumKind::Fraction(a), NumKind::Fraction(b)) => a.to_bits() == b.to_bits(),
            (NumKind::BigInteger, NumKind::BigInteger) => self.token == other.token,
            _ => false,
        }
    }
}

impl Obj {
    pub fn new() -> Obj {
        Obj::default()
    }

    /// Puts a field: a new one goes last, an existing one keeps its place.
    pub fn insert(&mut self, key: impl Into<String>, value: Node) {
        let key = key.into();
        match self.fields.iter_mut().find(|(k, _)| *k == key) {
            Some((_, v)) => *v = value,
            None => self.fields.push((key, value)),
        }
    }

    /// The field's value, a JSON null included.
    pub fn get(&self, key: &str) -> Option<&Node> {
        self.fields.iter().find(|(k, _)| k == key).map(|(_, v)| v)
    }

    /// The field's value, absent when it is missing or JSON null (Java's `getOr`).
    pub fn get_or_null(&self, key: &str) -> Option<&Node> {
        self.get(key).filter(|v| !matches!(v, Node::Null))
    }

    pub fn iter(&self) -> impl Iterator<Item = (&str, &Node)> {
        self.fields.iter().map(|(k, v)| (k.as_str(), v))
    }

    pub fn len(&self) -> usize {
        self.fields.len()
    }

    pub fn is_empty(&self) -> bool {
        self.fields.is_empty()
    }

    fn required(&self, key: &str) -> Result<&Node, Error> {
        self.get(key).ok_or_else(|| Error(format!("missing field '{key}'")))
    }

    /// A string field (Java's `getString`).
    pub fn str(&self, key: &str) -> Result<&str, Error> {
        match self.required(key)? {
            Node::Str(s) => Ok(s),
            other => Err(wrong(key, "a string", other)),
        }
    }

    /// A string field that may be missing or null (Java's `getStringOr(key, null)`).
    pub fn str_or_null(&self, key: &str) -> Result<Option<&str>, Error> {
        match self.get_or_null(key) {
            None => Ok(None),
            Some(Node::Str(s)) => Ok(Some(s)),
            Some(other) => Err(wrong(key, "a string", other)),
        }
    }

    /// A boolean field (Java's `getBool`).
    pub fn bool(&self, key: &str) -> Result<bool, Error> {
        match self.required(key)? {
            Node::Bool(b) => Ok(*b),
            other => Err(wrong(key, "a boolean", other)),
        }
    }

    /// A number field.
    pub fn num(&self, key: &str) -> Result<&Num, Error> {
        match self.required(key)? {
            Node::Num(n) => Ok(n),
            other => Err(wrong(key, "a number", other)),
        }
    }

    /// An integer field that fits a long.
    pub fn i64(&self, key: &str) -> Result<i64, Error> {
        let n = self.num(key)?;
        n.as_i64().ok_or_else(|| Error(format!("field '{key}' is not an integer: {}", n.token())))
    }

    /// An object field (Java's `getObj`).
    pub fn obj(&self, key: &str) -> Result<&Obj, Error> {
        match self.required(key)? {
            Node::Obj(o) => Ok(o),
            other => Err(wrong(key, "an object", other)),
        }
    }

    /// An object field that may be missing or null.
    pub fn obj_or_null(&self, key: &str) -> Result<Option<&Obj>, Error> {
        match self.get_or_null(key) {
            None => Ok(None),
            Some(Node::Obj(o)) => Ok(Some(o)),
            Some(other) => Err(wrong(key, "an object", other)),
        }
    }

    /// An array field (Java's `getArr`).
    pub fn arr(&self, key: &str) -> Result<&[Node], Error> {
        match self.required(key)? {
            Node::Arr(a) => Ok(a),
            other => Err(wrong(key, "an array", other)),
        }
    }
}

fn wrong(key: &str, wanted: &str, got: &Node) -> Error {
    let kind = match got {
        Node::Obj(_) => "an object",
        Node::Arr(_) => "an array",
        Node::Str(_) => "a string",
        Node::Num(_) => "a number",
        Node::Bool(_) => "a boolean",
        Node::Null => "null",
    };
    Error(format!("field '{key}' is {kind}, not {wanted}"))
}

impl Node {
    pub fn as_obj(&self) -> Result<&Obj, Error> {
        match self {
            Node::Obj(o) => Ok(o),
            _ => Err(Error("expected a JSON object".to_owned())),
        }
    }

    pub fn as_arr(&self) -> Result<&[Node], Error> {
        match self {
            Node::Arr(a) => Ok(a),
            _ => Err(Error("expected a JSON array".to_owned())),
        }
    }

    pub fn as_str(&self) -> Result<&str, Error> {
        match self {
            Node::Str(s) => Ok(s),
            _ => Err(Error("expected a JSON string".to_owned())),
        }
    }
}
