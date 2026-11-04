// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! The parser: Java's `Json.Parser`, byte for byte in what it accepts and how it words a refusal.

use crate::{Error, Node, Num, NumKind, Obj};

/// The nesting a document may reach unless the reader says otherwise (Java's `DEFAULT_MAX_DEPTH`).
pub const DEFAULT_MAX_DEPTH: usize = 64;

/// A whole document, nested at most [`DEFAULT_MAX_DEPTH`] deep.
pub fn parse(text: &str) -> Result<Node, Error> {
    parse_with_depth(text, DEFAULT_MAX_DEPTH)
}

/// A whole document, nested at most `max_depth` deep (a plan reads with 4096: its own text, deeply nested).
pub fn parse_with_depth(text: &str, max_depth: usize) -> Result<Node, Error> {
    let mut p = Parser { src: text.as_bytes(), text, pos: 0, depth: 0, max_depth };
    p.skip_ws();
    let n = p.value()?;
    p.skip_ws();
    if p.pos < p.src.len() {
        return Err(p.error("Unexpected trailing content"));
    }
    Ok(n)
}

struct Parser<'a> {
    src: &'a [u8],
    text: &'a str,
    pos: usize,
    depth: usize,
    max_depth: usize,
}

impl Parser<'_> {
    fn value(&mut self) -> Result<Node, Error> {
        self.skip_ws();
        let Some(&c) = self.src.get(self.pos) else {
            return Err(self.error("Unexpected end of input"));
        };
        match c {
            b'{' => self.object().map(Node::Obj),
            b'[' => self.array(),
            b'"' => self.string().map(Node::Str),
            b't' | b'f' => self.boolean(),
            b'n' => self.null(),
            _ => self.number().map(Node::Num),
        }
    }

    fn object(&mut self) -> Result<Obj, Error> {
        self.expect(b'{')?;
        self.enter()?;
        let mut o = Obj::new();
        self.skip_ws();
        if self.peek() == Some(b'}') {
            self.pos += 1;
            self.depth -= 1;
            return Ok(o);
        }
        loop {
            self.skip_ws();
            let key = self.string()?;
            self.skip_ws();
            self.expect(b':')?;
            let v = self.value()?;
            o.insert(key, v);
            self.skip_ws();
            match self.next()? {
                b',' => continue,
                b'}' => {
                    self.depth -= 1;
                    return Ok(o);
                }
                _ => return Err(self.error("Expected ',' or '}' in object")),
            }
        }
    }

    fn array(&mut self) -> Result<Node, Error> {
        self.expect(b'[')?;
        self.enter()?;
        let mut items = Vec::new();
        self.skip_ws();
        if self.peek() == Some(b']') {
            self.pos += 1;
            self.depth -= 1;
            return Ok(Node::Arr(items));
        }
        loop {
            items.push(self.value()?);
            self.skip_ws();
            match self.next()? {
                b',' => continue,
                b']' => {
                    self.depth -= 1;
                    return Ok(Node::Arr(items));
                }
                _ => return Err(self.error("Expected ',' or ']' in array")),
            }
        }
    }

    fn enter(&mut self) -> Result<(), Error> {
        self.depth += 1;
        if self.depth > self.max_depth {
            return Err(self.error(&format!("Nesting depth exceeds limit of {}", self.max_depth)));
        }
        Ok(())
    }

    fn string(&mut self) -> Result<String, Error> {
        self.expect(b'"')?;
        let mut out = String::new();
        // pending high surrogate of a \u escape, waiting for its low half
        let mut high: Option<u16> = None;
        loop {
            let Some(&c) = self.src.get(self.pos) else {
                return Err(self.error("Unterminated string"));
            };
            if c == b'\\' && self.src.get(self.pos + 1) == Some(&b'u') {
                self.pos += 2;
                if self.pos + 4 > self.src.len() {
                    return Err(self.error("Bad unicode escape"));
                }
                let hex = &self.text[self.pos..self.pos + 4];
                let unit = u16::from_str_radix(hex, 16).map_err(|_| self.error("Bad unicode escape"))?;
                self.pos += 4;
                match (high.take(), unit) {
                    (Some(h), 0xDC00..=0xDFFF) => {
                        let cp = 0x10000 + (((h as u32) - 0xD800) << 10) + ((unit as u32) - 0xDC00);
                        out.push(char::from_u32(cp).unwrap_or('\u{FFFD}'));
                    }
                    (pending, 0xD800..=0xDBFF) => {
                        if pending.is_some() {
                            out.push('\u{FFFD}');
                        }
                        high = Some(unit);
                    }
                    (pending, _) => {
                        if pending.is_some() {
                            out.push('\u{FFFD}');
                        }
                        out.push(char::from_u32(unit as u32).unwrap_or('\u{FFFD}'));
                    }
                }
                continue;
            }
            if high.take().is_some() {
                out.push('\u{FFFD}');
            }
            match c {
                b'"' => {
                    self.pos += 1;
                    return Ok(out);
                }
                b'\\' => {
                    let Some(&e) = self.src.get(self.pos + 1) else {
                        self.pos += 1;
                        return Err(self.error("Bad escape at end of string"));
                    };
                    self.pos += 2;
                    out.push(match e {
                        b'"' => '"',
                        b'\\' => '\\',
                        b'/' => '/',
                        b'n' => '\n',
                        b'r' => '\r',
                        b't' => '\t',
                        b'b' => '\u{8}',
                        b'f' => '\u{c}',
                        _ => {
                            let shown = self.text[self.pos - 1..].chars().next().unwrap_or('?');
                            return Err(self.error(&format!("Invalid escape: \\{shown}")));
                        }
                    });
                }
                0..0x20 => {
                    self.pos += 1;
                    return Err(self.error(&format!("Unescaped control character in string: 0x{c:x}")));
                }
                _ => {
                    // one whole character: the source is UTF-8 text
                    let ch = self.text[self.pos..].chars().next().expect("a character at a char boundary");
                    out.push(ch);
                    self.pos += ch.len_utf8();
                }
            }
        }
    }

    fn boolean(&mut self) -> Result<Node, Error> {
        if self.src[self.pos..].starts_with(b"true") {
            self.pos += 4;
            Ok(Node::Bool(true))
        } else if self.src[self.pos..].starts_with(b"false") {
            self.pos += 5;
            Ok(Node::Bool(false))
        } else {
            Err(self.error("Expected boolean literal"))
        }
    }

    fn null(&mut self) -> Result<Node, Error> {
        if self.src[self.pos..].starts_with(b"null") {
            self.pos += 4;
            Ok(Node::Null)
        } else {
            Err(self.error("Expected null literal"))
        }
    }

    /// RFC 8259 §6: `[ "-" ] ( "0" / digit1-9 *DIGIT ) [ "." 1*DIGIT ] [ ("e"/"E") [ "+"/"-" ] 1*DIGIT ]`.
    ///
    /// Not reproduced: past the first digit Java's lexer takes any decimal digit unit (`Character.isDigit`), so `1٣`
    /// is 13 (`Long.parseLong`, `BigInteger` past a long), `0٣` refused for its leading zero, and a fraction or an
    /// exponent holding one `Double.parseDouble`'s `NumberFormatException` (`For input string: "1.٣"`, no position);
    /// this reads ASCII digits, so each is refused at that digit (measured on the build JDK, 2026-11-03). No JSON
    /// writer of the platform writes one.
    fn number(&mut self) -> Result<Num, Error> {
        let start = self.pos;
        if self.peek() == Some(b'-') {
            self.pos += 1;
        }
        let Some(first) = self.peek() else {
            return Err(self.error("Invalid number: expected digit"));
        };
        match first {
            b'0' => {
                self.pos += 1;
                if self.peek().is_some_and(|c| c.is_ascii_digit()) {
                    return Err(self.error("Leading zeros are not permitted in JSON numbers"));
                }
            }
            b'1'..=b'9' => {
                self.pos += 1;
                self.digits();
            }
            _ => {
                let shown = self.text[self.pos..].chars().next().unwrap_or('?');
                return Err(self.error(&format!("Invalid number: expected digit, got '{shown}'")));
            }
        }
        let mut fraction = false;
        if self.peek() == Some(b'.') {
            fraction = true;
            self.pos += 1;
            if !self.peek().is_some_and(|c| c.is_ascii_digit()) {
                return Err(self.error("Number fraction requires at least one digit after '.'"));
            }
            self.digits();
        }
        if matches!(self.peek(), Some(b'e' | b'E')) {
            fraction = true;
            self.pos += 1;
            if matches!(self.peek(), Some(b'+' | b'-')) {
                self.pos += 1;
            }
            if !self.peek().is_some_and(|c| c.is_ascii_digit()) {
                return Err(self.error("Number exponent requires at least one digit"));
            }
            self.digits();
        }
        let token = &self.text[start..self.pos];
        let kind = if fraction {
            // the double from the token: "-0.0" keeps its sign
            NumKind::Fraction(token.parse().expect("a JSON number is a Rust float"))
        } else {
            match token.parse::<i64>() {
                Ok(v) => NumKind::Integer(v),
                Err(_) => NumKind::BigInteger,
            }
        };
        let token = match kind {
            NumKind::Integer(v) => v.to_string(),
            _ => token.to_owned(),
        };
        Ok(Num { token, kind })
    }

    fn digits(&mut self) {
        while self.peek().is_some_and(|c| c.is_ascii_digit()) {
            self.pos += 1;
        }
    }

    fn expect(&mut self, c: u8) -> Result<(), Error> {
        if self.peek() != Some(c) {
            return Err(self.error(&format!("Expected '{}'", c as char)));
        }
        self.pos += 1;
        Ok(())
    }

    fn next(&mut self) -> Result<u8, Error> {
        let Some(&c) = self.src.get(self.pos) else {
            return Err(self.error("Unexpected end of input"));
        };
        self.pos += 1;
        Ok(c)
    }

    fn peek(&self) -> Option<u8> {
        self.src.get(self.pos).copied()
    }

    fn skip_ws(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            self.pos += 1;
        }
    }

    /// Java's wording: `<message> at line L col C`, counting characters up to the failure.
    fn error(&self, message: &str) -> Error {
        let (mut line, mut col) = (1, 1);
        let end = self.pos.min(self.src.len());
        let end = (0..=end).rev().find(|&i| self.text.is_char_boundary(i)).unwrap_or(0);
        for ch in self.text[..end].chars() {
            if ch == '\n' {
                line += 1;
                col = 1;
            } else {
                col += 1;
            }
        }
        Error(format!("{message} at line {line} col {col}"))
    }
}
