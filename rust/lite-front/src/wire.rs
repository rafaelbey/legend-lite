// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! JSON whose strings are UTF-16 units: how a text crosses into the front end (docs/rust-port/PHASE2.md §3). Java's
//! `json` reads `\uXXXX` as one UTF-16 unit, so a lone surrogate survives into its string; `lite-json` reads one as
//! U+FFFD (a Rust `String` cannot hold it). This reader keeps every string as the units Java would hold.

/// A JSON value; a string is its UTF-16 units, a number its token.
#[derive(Clone, Debug, PartialEq)]
pub enum Json {
    Null,
    Bool(bool),
    Num(String),
    Str(Vec<u16>),
    Arr(Vec<Json>),
    Obj(Vec<(Vec<u16>, Json)>),
}

impl Json {
    /// A member of an object, by its (ASCII) name.
    pub fn get(&self, key: &str) -> Option<&Json> {
        let k: Vec<u16> = key.encode_utf16().collect();
        match self {
            Json::Obj(fields) => fields.iter().find(|(n, _)| *n == k).map(|(_, v)| v),
            _ => None,
        }
    }

    pub fn as_units(&self) -> Option<&[u16]> {
        match self {
            Json::Str(s) => Some(s),
            _ => None,
        }
    }

    /// A string that is valid UTF-16, as a Rust `String` (an id, a name).
    pub fn as_string(&self) -> Option<String> {
        self.as_units().and_then(|u| String::from_utf16(u).ok())
    }

    pub fn as_array(&self) -> Option<&[Json]> {
        match self {
            Json::Arr(a) => Some(a),
            _ => None,
        }
    }
}

/// Parse one JSON document; nesting deeper than `max_depth` is refused.
pub fn parse(text: &str, max_depth: usize) -> Result<Json, String> {
    let mut p = Parser { s: text.as_bytes(), i: 0, max_depth };
    p.ws();
    let v = p.value(0)?;
    p.ws();
    if p.i != p.s.len() {
        return Err(format!("trailing characters at byte {}", p.i));
    }
    Ok(v)
}

struct Parser<'a> {
    s: &'a [u8],
    i: usize,
    max_depth: usize,
}

impl Parser<'_> {
    fn ws(&mut self) {
        while self.i < self.s.len() && matches!(self.s[self.i], b' ' | b'\t' | b'\n' | b'\r') {
            self.i += 1;
        }
    }

    fn expect(&mut self, lit: &[u8]) -> Result<(), String> {
        if self.s[self.i..].starts_with(lit) {
            self.i += lit.len();
            Ok(())
        } else {
            Err(format!("expected {} at byte {}", String::from_utf8_lossy(lit), self.i))
        }
    }

    fn value(&mut self, depth: usize) -> Result<Json, String> {
        if depth > self.max_depth {
            return Err(format!("nesting deeper than {}", self.max_depth));
        }
        match self.s.get(self.i) {
            Some(b'{') => {
                self.i += 1;
                let mut fields = Vec::new();
                self.ws();
                if self.s.get(self.i) == Some(&b'}') {
                    self.i += 1;
                    return Ok(Json::Obj(fields));
                }
                loop {
                    self.ws();
                    let k = self.string()?;
                    self.ws();
                    self.expect(b":")?;
                    self.ws();
                    let v = self.value(depth + 1)?;
                    fields.push((k, v));
                    self.ws();
                    match self.s.get(self.i) {
                        Some(b',') => self.i += 1,
                        Some(b'}') => {
                            self.i += 1;
                            return Ok(Json::Obj(fields));
                        }
                        _ => return Err(format!("expected , or }} at byte {}", self.i)),
                    }
                }
            }
            Some(b'[') => {
                self.i += 1;
                let mut items = Vec::new();
                self.ws();
                if self.s.get(self.i) == Some(&b']') {
                    self.i += 1;
                    return Ok(Json::Arr(items));
                }
                loop {
                    self.ws();
                    items.push(self.value(depth + 1)?);
                    self.ws();
                    match self.s.get(self.i) {
                        Some(b',') => self.i += 1,
                        Some(b']') => {
                            self.i += 1;
                            return Ok(Json::Arr(items));
                        }
                        _ => return Err(format!("expected , or ] at byte {}", self.i)),
                    }
                }
            }
            Some(b'"') => Ok(Json::Str(self.string()?)),
            Some(b't') => self.expect(b"true").map(|_| Json::Bool(true)),
            Some(b'f') => self.expect(b"false").map(|_| Json::Bool(false)),
            Some(b'n') => self.expect(b"null").map(|_| Json::Null),
            Some(b'-' | b'0'..=b'9') => {
                let start = self.i;
                while self.i < self.s.len() && matches!(self.s[self.i], b'-' | b'+' | b'.' | b'e' | b'E' | b'0'..=b'9')
                {
                    self.i += 1;
                }
                Ok(Json::Num(String::from_utf8_lossy(&self.s[start..self.i]).into_owned()))
            }
            _ => Err(format!("unexpected input at byte {}", self.i)),
        }
    }

    /// A string's UTF-16 units: raw characters re-encoded, `\uXXXX` one unit each (a lone surrogate kept).
    fn string(&mut self) -> Result<Vec<u16>, String> {
        self.expect(b"\"")?;
        let mut out = Vec::new();
        loop {
            let Some(&b) = self.s.get(self.i) else {
                return Err("unterminated string".to_owned());
            };
            match b {
                b'"' => {
                    self.i += 1;
                    return Ok(out);
                }
                b'\\' => {
                    let e = *self.s.get(self.i + 1).ok_or("unterminated escape")?;
                    self.i += 2;
                    match e {
                        b'"' => out.push(u16::from(b'"')),
                        b'\\' => out.push(u16::from(b'\\')),
                        b'/' => out.push(u16::from(b'/')),
                        b'b' => out.push(0x08),
                        b'f' => out.push(0x0C),
                        b'n' => out.push(u16::from(b'\n')),
                        b'r' => out.push(u16::from(b'\r')),
                        b't' => out.push(u16::from(b'\t')),
                        b'u' => {
                            let hex = self.s.get(self.i..self.i + 4).ok_or("short \\u escape")?;
                            let hex = std::str::from_utf8(hex).map_err(|e| e.to_string())?;
                            out.push(u16::from_str_radix(hex, 16).map_err(|e| e.to_string())?);
                            self.i += 4;
                        }
                        _ => return Err(format!("bad escape at byte {}", self.i)),
                    }
                }
                _ if b < 0x80 => {
                    out.push(u16::from(b));
                    self.i += 1;
                }
                _ => {
                    // one UTF-8 character, re-encoded as UTF-16
                    let rest = std::str::from_utf8(&self.s[self.i..(self.i + 4).min(self.s.len())])
                        .or_else(|e| std::str::from_utf8(&self.s[self.i..self.i + e.valid_up_to()]))
                        .map_err(|e| e.to_string())?;
                    let c = rest.chars().next().ok_or("bad UTF-8")?;
                    let mut buf = [0u16; 2];
                    out.extend_from_slice(c.encode_utf16(&mut buf));
                    self.i += c.len_utf8();
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_lone_surrogate_survives() {
        let v = parse(r#"{"text":"a\ud800b😀é"}"#, 8).unwrap();
        assert_eq!(v.get("text").unwrap().as_units().unwrap(), &[0x61, 0xD800, 0x62, 0xD83D, 0xDE00, 0xE9]);
    }

    #[test]
    fn arrays_objects_and_literals() {
        let v = parse(r#"{"asks":[["pmcd","LEGEND_LITE"]],"n":12,"b":true,"z":null}"#, 8).unwrap();
        let asks = v.get("asks").unwrap().as_array().unwrap();
        assert_eq!(asks[0].as_array().unwrap()[1].as_string().unwrap(), "LEGEND_LITE");
        assert_eq!(v.get("n"), Some(&Json::Num("12".into())));
        assert_eq!(v.get("z"), Some(&Json::Null));
    }
}
