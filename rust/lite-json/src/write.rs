// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! The compact writer: Java's `Json.toCompact`, and its one escape table.

use crate::Node;

/// `node` as compact JSON text, as Java's `Json.toCompact` writes it.
pub fn to_compact(node: &Node) -> String {
    let mut out = String::with_capacity(256);
    write(&mut out, node);
    out
}

fn write(out: &mut String, node: &Node) {
    match node {
        Node::Obj(o) => {
            out.push('{');
            for (i, (k, v)) in o.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                out.push('"');
                escape_to(out, k, false);
                out.push_str("\":");
                write(out, v);
            }
            out.push('}');
        }
        Node::Arr(items) => {
            out.push('[');
            for (i, v) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write(out, v);
            }
            out.push(']');
        }
        Node::Str(s) => {
            out.push('"');
            escape_to(out, s, false);
            out.push('"');
        }
        Node::Num(n) => out.push_str(n.token()),
        Node::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Node::Null => out.push_str("null"),
    }
}

/// Java's one JSON escape table (`Json.escapeTo`): quote, backslash, `\n \r \t \b \f`, every other control as
/// `\u00xx` -- lowercase hex, or uppercase where the protocol emitter's goldens pin Jackson's -- and every other
/// character as is.
pub fn escape_to(out: &mut String, s: &str, upper_hex: bool) {
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            c if (c as u32) < 0x20 => {
                if upper_hex {
                    out.push_str(&format!("\\u{:04X}", c as u32));
                } else {
                    out.push_str(&format!("\\u{:04x}", c as u32));
                }
            }
            c => out.push(c),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{Node, Num, Obj, parse, parse_with_depth, to_compact};

    #[test]
    fn a_document_reads_and_writes_back_unchanged() {
        for text in [
            r#"{"_type":"select","projections":[{"expr":{"_type":"intLit","value":1}}],"distinct":false}"#,
            r#"{"n":1.0E10,"m":1.50,"z":-0.0,"big":123456789012345678901234567890,"e":2e-5}"#,
            r#"["héllo","\ud83d\ude00 is written as itself","a\"b\\c","\n\r\t\b\f","\u0001"]"#,
            r#"{"a":null,"b":true,"c":false,"d":{},"e":[]}"#,
        ] {
            let written = to_compact(&parse(text).expect(text));
            let again = to_compact(&parse(&written).expect(&written));
            assert_eq!(written, again, "{text}");
        }
        assert_eq!(to_compact(&parse(r#"{"n":1.0E10,"m":1.50}"#).unwrap()), r#"{"n":1.0E10,"m":1.50}"#);
        assert_eq!(to_compact(&parse(r#"["\u0001\u001f"]"#).unwrap()), r#"["\u0001\u001f"]"#);
        assert_eq!(to_compact(&parse(r#""\ud83d\ude00""#).unwrap()), "\"\u{1F600}\"");
    }

    #[test]
    fn an_integer_is_written_from_its_value() {
        assert_eq!(to_compact(&parse("-0").unwrap()), "0");
        assert_eq!(to_compact(&parse("[9223372036854775807]").unwrap()), "[9223372036854775807]");
        assert_eq!(to_compact(&parse("9223372036854775808").unwrap()), "9223372036854775808");
    }

    #[test]
    fn a_double_is_written_as_java_writes_one() {
        let w = |v: f64| Num::from_f64(v).unwrap().token().to_owned();
        assert_eq!(w(3.0), "3");
        assert_eq!(w(-0.0), "0");
        assert_eq!(w(0.5), "0.5");
        assert_eq!(w(1e19), "1.0E19");
        assert_eq!(w(9.5e18), "9223372036854775807");
        assert_eq!(w(1e-5), "1.0E-5");
        assert!(Num::from_f64(f64::NAN).is_err());
    }

    #[test]
    fn a_field_put_again_keeps_its_place() {
        let mut o = Obj::new();
        o.insert("b", Node::Num(Num::from_i64(1)));
        o.insert("a", Node::Num(Num::from_i64(2)));
        o.insert("b", Node::Num(Num::from_i64(3)));
        assert_eq!(to_compact(&Node::Obj(o)), r#"{"b":3,"a":2}"#);
        assert_eq!(to_compact(&parse(r#"{"x":1,"y":2,"x":3}"#).unwrap()), r#"{"x":3,"y":2}"#);
    }

    #[test]
    fn refuses_as_java_does() {
        let refusal = |text: &str| parse(text).unwrap_err().0;
        assert_eq!(refusal("01"), "Leading zeros are not permitted in JSON numbers at line 1 col 2");
        assert_eq!(refusal("1."), "Number fraction requires at least one digit after '.' at line 1 col 3");
        assert_eq!(refusal("1e"), "Number exponent requires at least one digit at line 1 col 3");
        assert_eq!(refusal("[1,]"), "Invalid number: expected digit, got ']' at line 1 col 4");
        assert_eq!(refusal("{\"a\":1} x"), "Unexpected trailing content at line 1 col 9");
        // Java reads the character before refusing it, so a newline is counted
        assert_eq!(refusal("\"a\nb\""), "Unescaped control character in string: 0xa at line 2 col 1");
        assert_eq!(refusal("\"\\x\""), "Invalid escape: \\x at line 1 col 4");
        assert_eq!(refusal("tru"), "Expected boolean literal at line 1 col 1");
        assert_eq!(refusal(""), "Unexpected end of input at line 1 col 1");
        assert_eq!(parse_with_depth("[[[]]]", 2).unwrap_err().0, "Nesting depth exceeds limit of 2 at line 1 col 4");
        assert!(parse_with_depth("[[[]]]", 3).is_ok());
    }

    #[test]
    fn a_lone_surrogate_reads_as_the_replacement_character() {
        assert_eq!(parse(r#""a\ud800b""#).unwrap(), Node::Str("a\u{FFFD}b".to_owned()));
        assert_eq!(parse(r#""\udc00""#).unwrap(), Node::Str("\u{FFFD}".to_owned()));
    }
}
