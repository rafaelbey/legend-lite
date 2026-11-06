// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! core/'s `com.legend.protocol.SourceInformation.withoutSpans`, ported (docs/rust-port/PHASE2.md §4 step 5): protocol
//! JSON without its spans, as legend-engine writes a model or lambda parsed with `returnSourceInformation=false` --
//! every `sourceInformation` and named `...SourceInformation` member cut out of the text, except inside the values the
//! engine parses through a span context of its own ([`kept_by_the_engine`]). A cut over the emitted units, never a
//! parse and a rewrite: what the emitter wrote stays byte for byte, the `_type` key it writes twice included.

/// An open object or array: the member it is the value of (none for an array's element), its `_type` (an object's,
/// once read), and whether the spans inside it stay (decided when first asked, its type read by then). Keys and types
/// are ranges of the text.
struct Frame {
    object: bool,
    key: Option<(usize, usize)>,
    ty: Option<(usize, usize)>,
    keep: Option<bool>,
}

fn is(json: &[u16], range: Option<(usize, usize)>, s: &str) -> bool {
    match range {
        Some((a, b)) => json[a..b].iter().copied().eq(s.bytes().map(u16::from)),
        None => false,
    }
}

const QUOTE: u16 = b'"' as u16;
const BACKSLASH: u16 = b'\\' as u16;

fn is_space(c: u16) -> bool {
    c == u16::from(b' ') || c == u16::from(b'\t') || c == u16::from(b'\r') || c == u16::from(b'\n')
}

/// `SourceInformation.withoutSpans(json)`.
pub fn without_spans(json: &[u16]) -> Vec<u16> {
    let mut out: Vec<u16> = Vec::with_capacity(json.len());
    let mut stack: Vec<Frame> = Vec::new();
    let mut at_key = false; // inside an object, where a member's key comes next
    let mut key: Option<(usize, usize)> = None; // the member whose value comes next
    let n = json.len();
    let mut i = 0;
    while i < n {
        let c = json[i];
        if c == u16::from(b'{') || c == u16::from(b'[') {
            let in_object = stack.last().is_some_and(|f| f.object);
            stack.push(Frame {
                object: c == u16::from(b'{'),
                key: if in_object { key } else { None },
                ty: None,
                keep: None,
            });
            at_key = c == u16::from(b'{');
            key = None;
            out.push(c);
            i += 1;
        } else if c == u16::from(b'}') || c == u16::from(b']') {
            stack.pop();
            at_key = false;
            key = None;
            out.push(c);
            i += 1;
        } else if c == u16::from(b',') {
            at_key = stack.last().is_some_and(|f| f.object);
            key = None;
            out.push(c);
            i += 1;
        } else if c == u16::from(b':') {
            out.push(c);
            i += 1;
        } else if c == QUOTE {
            let end = string_end(json, i);
            let inner = Some((i + 1, end - 1));
            let top_object = stack.last().is_some_and(|f| f.object);
            if !at_key
                && is(json, key, "_type")
                && let Some(top) = stack.last_mut()
                && top.object
                && top.ty.is_none()
            {
                top.ty = inner;
            }
            if at_key && top_object && is_span(&json[i + 1..end - 1]) && !keeps(json, &mut stack) {
                // skip the member: its key, ':', its value; and one of the commas beside it
                let mut j = value_end(json, skip_space(json, skip_space(json, end) + 1));
                match last_non_space(&out) {
                    Some(last) if out[last] == u16::from(b',') => {
                        out.truncate(last); // a member before it: drop the comma between them
                        at_key = false;
                    }
                    _ => {
                        j = skip_space(json, j);
                        if j < n && json[j] == u16::from(b',') {
                            j += 1; // the object's first member: drop the comma after it
                        }
                        at_key = true;
                    }
                }
                i = j;
            } else {
                if at_key {
                    key = inner;
                }
                out.extend_from_slice(&json[i..end]);
                at_key = false;
                i = end;
            }
        } else {
            out.push(c);
            i += 1;
        }
    }
    out
}

/// Whether the spans inside the innermost frame stay: it, or a frame around it, is a value the engine parses with a
/// span context of its own. Memoized per frame, each decided after the frames around it, as Java's recursion decides
/// them.
fn keeps(json: &[u16], stack: &mut [Frame]) -> bool {
    let top = stack.len() - 1;
    let mut from = top;
    while from > 0 && stack[from].keep.is_none() {
        from -= 1;
    }
    for depth in from..=top {
        if stack[depth].keep.is_none() {
            let keep = depth > 0 && (stack[depth - 1].keep == Some(true) || kept_by_the_engine(json, stack, depth));
            stack[depth].keep = Some(keep);
        }
    }
    stack[top].keep == Some(true)
}

/// Whether the frame at `depth` (not the root) is one of the values legend-engine parses with a span context that
/// records whatever it was asked: an `equalTo`'s `expected`; a function or service test's `parameters[].value`; a
/// legacy service test's `parametersValues[]`, each value (of a list, the values and not the list around them); a
/// persistence context's `serviceParameters[].value.primitiveType`.
fn kept_by_the_engine(json: &[u16], stack: &[Frame], depth: usize) -> bool {
    let d = depth as isize;
    let f = &stack[depth];
    let parent = &stack[depth - 1];
    if is(json, f.key, "expected") && is(json, parent.ty, "equalTo") {
        return true;
    }
    if is(json, f.key, "value")
        && element_of(json, stack, d - 1, "parameters")
        && (typed(json, stack, d - 3, "functionTest") || typed(json, stack, d - 3, "serviceTest"))
    {
        return true;
    }
    if element_of(json, stack, d, "parametersValues") {
        return !is(json, f.ty, "classInstance");
    }
    if is(json, f.key, "values")
        && is(json, parent.key, "value")
        && typed(json, stack, d - 2, "classInstance")
        && element_of(json, stack, d - 2, "parametersValues")
    {
        return true;
    }
    is(json, f.key, "primitiveType")
        && is(json, parent.ty, "primitiveTypeValue")
        && is(json, parent.key, "value")
        && element_of(json, stack, d - 2, "serviceParameters")
        && typed(json, stack, d - 4, "persistenceContext")
}

/// Whether the frame at `depth` is an element of the array that is member `array`'s value.
fn element_of(json: &[u16], stack: &[Frame], depth: isize, array: &str) -> bool {
    depth >= 1 && {
        let parent = &stack[(depth - 1) as usize];
        !parent.object && is(json, parent.key, array)
    }
}

fn typed(json: &[u16], stack: &[Frame], depth: isize, ty: &str) -> bool {
    depth >= 0 && is(json, stack[depth as usize].ty, ty)
}

fn is_span(key: &[u16]) -> bool {
    let named = |s: &str| key.iter().copied().eq(s.bytes().map(u16::from));
    let suffix = "SourceInformation";
    named("sourceInformation")
        || (key.len() >= suffix.len() && key[key.len() - suffix.len()..].iter().copied().eq(suffix.bytes().map(u16::from)))
}

/// The index after the string literal that opens at `start`.
fn string_end(json: &[u16], start: usize) -> usize {
    let mut j = start + 1;
    while json[j] != QUOTE {
        j += if json[j] == BACKSLASH { 2 } else { 1 };
    }
    j + 1
}

/// The index after the JSON value that starts at `start`.
fn value_end(json: &[u16], start: usize) -> usize {
    let c = json[start];
    if c == QUOTE {
        return string_end(json, start);
    }
    if c == u16::from(b'{') || c == u16::from(b'[') {
        let mut depth = 0i64;
        let mut j = start;
        loop {
            let d = json[j];
            if d == QUOTE {
                j = string_end(json, j);
                continue;
            }
            if d == u16::from(b'{') || d == u16::from(b'[') {
                depth += 1;
            } else if d == u16::from(b'}') || d == u16::from(b']') {
                depth -= 1;
                if depth == 0 {
                    return j + 1;
                }
            }
            j += 1;
        }
    }
    let mut j = start;
    while j < json.len()
        && !(json[j] == u16::from(b',') || json[j] == u16::from(b'}') || json[j] == u16::from(b']') || is_space(json[j]))
    {
        j += 1;
    }
    j
}

fn skip_space(json: &[u16], from: usize) -> usize {
    let mut j = from;
    while j < json.len() && is_space(json[j]) {
        j += 1;
    }
    j
}

fn last_non_space(out: &[u16]) -> Option<usize> {
    out.iter().rposition(|&c| !is_space(c))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cut(s: &str) -> String {
        String::from_utf16(&without_spans(&s.encode_utf16().collect::<Vec<u16>>())).unwrap()
    }

    #[test]
    fn spans_go_and_the_kept_ones_stay() {
        assert_eq!(cut(r#"{"_type":"a","sourceInformation":{"x":1},"b":2}"#), r#"{"_type":"a","b":2}"#);
        assert_eq!(cut(r#"{"sourceInformation":{"x":1},"b":2}"#), r#"{"b":2}"#);
        assert_eq!(cut(r#"{"b":2,"classSourceInformation":null}"#), r#"{"b":2}"#);
        // an equalTo's expected keeps its spans
        assert_eq!(
            cut(r#"{"_type":"equalTo","expected":{"_type":"s","sourceInformation":{"x":1}},"sourceInformation":{"y":2}}"#),
            r#"{"_type":"equalTo","expected":{"_type":"s","sourceInformation":{"x":1}}}"#
        );
        // the doubled _type survives
        assert_eq!(
            cut(r#"{"_type":"r","_type":"r","sourceInformation":{"a":1},"subTrees":[]}"#),
            r#"{"_type":"r","_type":"r","subTrees":[]}"#
        );
    }
}
