// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! `###Diagram` (core/'s `parser.section.DiagramSectionGrammar`, docs/rust-port/PHASE2.md §4 step 4d): the raw section
//! the lexer skips (a diagram's color literals are not Pure), walked character by character (`Raw`) with its own lines
//! and columns, file-absolute from the section's first line, the columns in UTF-16 units: its `import` statements, and
//! each `Diagram` -- class, property and generalization views, and the platform's m2 views read past. Java's answers
//! reproduced (`parseRaw`), each refusal at Java's position.

// The generated records hold their record-typed list elements boxed (`Vec<Box<T>>`), so the readers build that.
#![allow(clippy::vec_box)]

use crate::cursor::R;
use crate::doors::{Dialect, JavaError};
use crate::jcat;
use crate::jnum::parse_double;
use crate::jtypes::JStr;
use crate::records::{PClassView, PDiagram, PDiagramPoint, PGeneralizationView, PPropertyView, SourceInfo};
use crate::sections::split_last;
use crate::ustr::{trim, u};
use lite_javacompat::character::{is_digit, is_letter_or_digit};
use std::collections::HashSet;

/// `parseRaw`'s answer: the diagrams, then the imports as written (`a::b::*`).
pub struct ParsedDiagrams {
    pub elements: Vec<PDiagram>,
    pub imports: Vec<Vec<u16>>,
}

/// THE character walker (`DiagramSectionGrammar.Raw`): its index, and the 1-based line and column (UTF-16 units) it
/// stands at.
struct Raw<'a> {
    t: &'a [u16],
    i: usize,
    line: i32,
    col: i32,
}

/// A saved `[i, line, col]`.
#[derive(Clone, Copy)]
struct Mark {
    line: i32,
    col: i32,
}

fn ch(c: char) -> u16 {
    c as u16
}

impl Raw<'_> {
    fn at_end(&self) -> bool {
        self.i >= self.t.len()
    }

    fn peek(&self) -> R<u16> {
        if self.at_end() {
            return Err(self.fail("unexpected end of ###Diagram section"));
        }
        Ok(self.t[self.i])
    }

    fn starts_with(&self, s: &str) -> bool {
        let p = u(s);
        self.t.len() >= self.i + p.len() && self.t[self.i..self.i + p.len()] == p[..]
    }

    fn step(&mut self) {
        if self.t[self.i] == ch('\n') {
            self.line += 1;
            self.col = 1;
        } else {
            self.col += 1;
        }
        self.i += 1;
    }

    fn skip_ws(&mut self) {
        while !self.at_end() {
            let c = self.t[self.i];
            if c == ch(' ') || c == ch('\t') || c == ch('\r') || c == ch('\n') {
                self.step();
            } else if self.starts_with("//") {
                while !self.at_end() && self.t[self.i] != ch('\n') {
                    self.step();
                }
            } else if self.starts_with("/*") {
                while !self.at_end() && !self.starts_with("*/") {
                    self.step();
                }
                if !self.at_end() {
                    self.step();
                    self.step();
                }
            } else {
                break;
            }
        }
    }

    fn mark(&self) -> Mark {
        Mark {
            line: self.line,
            col: self.col,
        }
    }

    /// `spanFrom(mark)`: from the mark to the last consumed character (a column of 1 stays 1).
    fn span_from(&self, m: Mark) -> SourceInfo {
        SourceInfo {
            source_id: JStr::default(),
            start_line: m.line,
            start_column: m.col,
            end_line: self.line,
            end_column: if self.col == 1 { 1 } else { self.col - 1 },
        }
    }

    fn word(&mut self) -> R<Vec<u16>> {
        let s = self.i;
        while !self.at_end() {
            let c = self.t[self.i];
            if is_letter_or_digit(c) || c == ch('_') || c == ch('$') || c == ch('-') {
                self.step();
            } else {
                break;
            }
        }
        if self.i == s {
            return Err(self.fail("expected an identifier"));
        }
        Ok(self.t[s..self.i].to_vec())
    }

    /// One path segment, unquoted here: a word, or a `'...'` string's body.
    fn segment(&mut self) -> R<Vec<u16>> {
        if self.peek()? == ch('\'') {
            self.step();
            let s = self.i;
            while !self.at_end() && self.t[self.i] != ch('\'') {
                self.step();
            }
            let seg = self.t[s..self.i].to_vec();
            self.expect('\'')?;
            return Ok(seg);
        }
        self.word()
    }

    /// `seg(::seg)*`, from already unquoted segments.
    fn path(&mut self) -> R<Vec<u16>> {
        let mut b = self.segment()?;
        while self.starts_with("::") {
            self.step();
            self.step();
            b.extend(u("::"));
            b.extend(self.segment()?);
        }
        Ok(b)
    }

    fn expect(&mut self, c: char) -> R<()> {
        if self.at_end() || self.t[self.i] != ch(c) {
            return Err(self.fail_j(jcat!("expected '", u(&c.to_string()), "'")));
        }
        self.step();
        Ok(())
    }

    fn expect_word(&mut self, w: &str) -> R<()> {
        if !self.starts_with(w) {
            return Err(self.fail_j(jcat!("expected '", w, "'")));
        }
        for _ in 0..w.len() {
            self.step();
        }
        Ok(())
    }

    fn bool(&mut self) -> R<bool> {
        if self.starts_with("true") {
            self.expect_word("true")?;
            return Ok(true);
        }
        self.expect_word("false")?;
        Ok(false)
    }

    /// `number()`: a run of digits (any script's), signs, points and exponent letters, read by `Double.parseDouble`;
    /// a run that is no number the engine's lexer ends where its own number ends, and refuses there: at the run's
    /// start, or at a second point (PARITY row 49).
    fn number(&mut self) -> R<f64> {
        let s = self.i;
        let (line, col) = (self.line, self.col);
        while !self.at_end() {
            let c = self.t[self.i];
            if is_digit(c) || c == ch('-') || c == ch('+') || c == ch('.') || c == ch('E') || c == ch('e') {
                self.step();
            } else {
                break;
            }
        }
        if self.i == s {
            return Err(self.fail("expected a number"));
        }
        let run = &self.t[s..self.i];
        parse_double(run).map_err(|m| {
            let second_point = run
                .iter()
                .enumerate()
                .filter(|(_, c)| **c == ch('.'))
                .map(|(i, _)| i as i32)
                .nth(1)
                .filter(|_| m == "multiple points");
            JavaError::parse(
                jcat!("Unexpected token '", run, "'"),
                line,
                col + second_point.unwrap_or(0),
            )
            .as_refusal()
        })
    }

    /// `(x,y)`.
    fn pair(&mut self) -> R<(f64, f64)> {
        self.expect('(')?;
        self.skip_ws();
        let x = self.number()?;
        self.skip_ws();
        self.expect(',')?;
        self.skip_ws();
        let y = self.number()?;
        self.skip_ws();
        self.expect(')')?;
        Ok((x, y))
    }

    /// `[(x,y),...]`.
    fn points(&mut self) -> R<Vec<Box<PDiagramPoint>>> {
        let mut out = Vec::new();
        self.expect('[')?;
        self.skip_ws();
        while self.peek()? != ch(']') {
            let (x, y) = self.pair()?;
            out.push(Box::new(PDiagramPoint { x, y }));
            self.skip_ws();
            if self.peek()? == ch(',') {
                self.step();
                self.skip_ws();
            }
        }
        self.expect(']')?;
        Ok(out)
    }

    fn read_to_semicolon(&mut self) -> R<Vec<u16>> {
        let s = self.i;
        while !self.at_end() && self.t[self.i] != ch(';') {
            self.step();
        }
        self.expect(';')?;
        Ok(self.t[s..self.i - 1].to_vec())
    }

    /// `skipBalanced(open, close)`: a balanced run blind to brackets inside `'...'` strings -- a do-while, so a first
    /// character that opens nothing is consumed alone.
    fn skip_balanced(&mut self, open: char, close: char) -> R<()> {
        let mut depth = 0i64;
        loop {
            let c = self.peek()?;
            if c == ch('\'') {
                self.step();
                while !self.at_end() && self.t[self.i] != ch('\'') {
                    self.step();
                }
                self.expect('\'')?;
            } else {
                if c == ch(open) {
                    depth += 1;
                } else if c == ch(close) {
                    depth -= 1;
                }
                self.step();
            }
            if self.at_end() || depth <= 0 {
                break;
            }
        }
        if depth > 0 {
            return Err(self.fail_j(jcat!("unbalanced '", u(&open.to_string()), "'")));
        }
        Ok(())
    }

    fn fail(&self, message: &str) -> JavaError {
        self.fail_j(JStr::from(message))
    }

    fn fail_j(&self, message: JStr) -> JavaError {
        JavaError::parse(message, self.line, self.col)
    }
}

/// `failAt(mark, message)`: the refusal anchored at a view's keyword.
fn fail_at(m: Mark, message: JStr) -> JavaError {
    JavaError::parse(message, m.line, m.col)
}

/// `DiagramSectionGrammar.parseRaw(src, dialect)`: a section's raw text, its first line `start_line`.
pub fn parse_raw(text: &[u16], start_line: i32, dialect: Dialect) -> R<ParsedDiagrams> {
    let mut r = Raw {
        t: text,
        i: 0,
        line: start_line,
        col: 1,
    };
    let mut elements = Vec::new();
    let mut imports = Vec::new();
    loop {
        r.skip_ws();
        if r.at_end() {
            break;
        }
        if r.starts_with("import ") {
            // `imports (diagram)*`, at the engine's dialect (ruling 18)
            if dialect.refuses_lite_extensions() && !elements.is_empty() {
                return Err(JavaError::parse(JStr::from("Unexpected token 'import'"), r.line, r.col).as_refusal());
            }
            let full = r.read_to_semicolon()?;
            let imp = trim(&full[7..]).to_vec();
            let n = imp.len();
            let mut ok = n > 3 && imp[n - 1] == ch('*') && imp[n - 2] == ch(':') && imp[n - 3] == ch(':');
            let mut seg_empty = true;
            let mut ci = 0;
            while ok && ci + 3 < n {
                let c = imp[ci];
                if c == ch(':') {
                    if seg_empty || ci + 1 >= n - 3 || imp[ci + 1] != ch(':') {
                        ok = false;
                    } else {
                        ci += 1;
                        seg_empty = true;
                    }
                } else if is_letter_or_digit(c) || c == ch('_') || c == ch('$') {
                    seg_empty = false;
                } else {
                    ok = false;
                }
                ci += 1;
            }
            if !ok || seg_empty {
                return Err(r.fail_j(jcat!("malformed import '", imp, "'")));
            }
            imports.push(imp);
            continue;
        }
        elements.push(parse_diagram(&mut r, dialect)?);
    }
    Ok(ParsedDiagrams { elements, imports })
}

/// `parseDiagram`: `Diagram qn (...)? { view* }`.
fn parse_diagram(r: &mut Raw<'_>, dialect: Dialect) -> R<PDiagram> {
    let start = r.mark();
    r.expect_word("Diagram")?;
    r.skip_ws();
    let qn = r.path()?;
    r.skip_ws();
    if r.peek()? == ch('(') {
        if dialect.refuses_lite_extensions() {
            return Err(JavaError::parse(JStr::from("Unexpected token '('"), r.line, r.col).as_refusal());
        }
        r.skip_balanced('(', ')')?;
        r.skip_ws();
    }
    r.expect('{')?;
    let mut class_views = Vec::new();
    let mut property_views = Vec::new();
    let mut generalization_views = Vec::new();
    loop {
        r.skip_ws();
        if r.peek()? == ch('}') {
            break;
        }
        let view_start = r.mark();
        let kw = r.word()?;
        match String::from_utf16_lossy(&kw).as_str() {
            "classView" => class_views.push(Box::new(parse_class_view(r, view_start)?)),
            "propertyView" => property_views.push(Box::new(parse_property_view(r, view_start)?)),
            "generalizationView" => generalization_views.push(Box::new(parse_generalization_view(r, view_start)?)),
            "TypeView" | "AssociationView" | "PropertyView" | "GeneralizationView" => {
                if dialect.refuses_platform_dialect() {
                    return Err(r.fail_j(jcat!("Unexpected token '", kw, "'")));
                }
                r.skip_ws();
                r.segment()?;
                r.skip_ws();
                r.skip_balanced('(', ')')?;
            }
            _ => return Err(r.fail_j(jcat!("unknown diagram view '", kw, "'"))),
        }
    }
    r.expect('}')?;
    let span = r.span_from(start);
    let (pkg, name) = split_last(&qn);
    Ok(PDiagram {
        pkg: JStr(pkg),
        name: JStr(name),
        class_views,
        property_views,
        generalization_views,
        source_information: Some(Box::new(span)),
    })
}

/// A view body's once-only rule, refused at the view's keyword.
fn once_view(seen: &mut HashSet<Vec<u16>>, key: &[u16], start: Mark) -> R<()> {
    if !seen.insert(key.to_vec()) {
        return Err(fail_at(start, jcat!("Field '", key, "' should be specified only once")));
    }
    Ok(())
}

/// `parseClassView`: `classView id { class: qn; position: (x,y); rectangle: (w,h); hideProperties: b; ... }` -- a flag
/// kept only when true.
fn parse_class_view(r: &mut Raw<'_>, start: Mark) -> R<PClassView> {
    r.skip_ws();
    let id = r.word()?;
    r.skip_ws();
    r.expect('{')?;
    let mut class: Option<(Vec<u16>, SourceInfo)> = None;
    let mut hide_properties = None;
    let mut hide_stereotypes = None;
    let mut hide_tagged_values = None;
    let mut position = None;
    let mut rectangle = None;
    let mut seen: HashSet<Vec<u16>> = HashSet::new();
    loop {
        r.skip_ws();
        if r.peek()? == ch('}') {
            break;
        }
        let key = r.word()?;
        once_view(&mut seen, &key, start)?;
        r.skip_ws();
        r.expect(':')?;
        r.skip_ws();
        match String::from_utf16_lossy(&key).as_str() {
            "class" => {
                let ps = r.mark();
                let p = r.path()?;
                class = Some((p, r.span_from(ps)));
            }
            "position" => position = Some(r.pair()?),
            "rectangle" => rectangle = Some(r.pair()?),
            "hideProperties" => hide_properties = r.bool()?.then_some(true),
            "hideTaggedValue" => hide_tagged_values = r.bool()?.then_some(true),
            "hideStereotype" => hide_stereotypes = r.bool()?.then_some(true),
            _ => return Err(r.fail_j(jcat!("unknown classView key '", key, "'"))),
        }
        r.skip_ws();
        r.expect(';')?;
    }
    r.expect('}')?;
    let (Some((class_path, class_span)), Some(position), Some(rectangle)) = (class, position, rectangle) else {
        return Err(fail_at(
            start,
            jcat!("classView '", id, "' needs class, position and rectangle"),
        ));
    };
    Ok(PClassView {
        id: JStr(id),
        class_path: JStr(class_path),
        class_span: Some(Box::new(class_span)),
        hide_properties,
        hide_stereotypes,
        hide_tagged_values,
        x: position.0,
        y: position.1,
        width: rectangle.0,
        height: rectangle.1,
        source_information: Some(Box::new(r.span_from(start))),
    })
}

/// A view's `source:` or `target:` id and its span.
fn view_ref(r: &mut Raw<'_>) -> R<(Vec<u16>, SourceInfo)> {
    let ps = r.mark();
    let v = r.word()?;
    Ok((v, r.span_from(ps)))
}

/// `parsePropertyView`: `propertyView { property: Class.prop; source: id; target: id; points: [...]; }`, the property's
/// span the class portion only.
fn parse_property_view(r: &mut Raw<'_>, start: Mark) -> R<PPropertyView> {
    r.skip_ws();
    r.expect('{')?;
    let mut prop_class = None;
    let mut prop_name = None;
    let mut prop_span = None;
    let mut source = None;
    let mut target = None;
    let mut points = None;
    let mut seen: HashSet<Vec<u16>> = HashSet::new();
    loop {
        r.skip_ws();
        if r.peek()? == ch('}') {
            break;
        }
        let key = r.word()?;
        once_view(&mut seen, &key, start)?;
        r.skip_ws();
        r.expect(':')?;
        r.skip_ws();
        match String::from_utf16_lossy(&key).as_str() {
            "property" => {
                let ps = r.mark();
                prop_class = Some(r.path()?);
                prop_span = Some(r.span_from(ps));
                r.expect('.')?;
                prop_name = Some(r.segment()?);
            }
            "source" => source = Some(view_ref(r)?),
            "target" => target = Some(view_ref(r)?),
            "points" => points = Some(r.points()?),
            _ => return Err(r.fail_j(jcat!("unknown propertyView key '", key, "'"))),
        }
        r.skip_ws();
        r.expect(';')?;
    }
    r.expect('}')?;
    let (Some(prop_class), Some(prop_name), Some(source), Some(target), Some(points)) =
        (prop_class, prop_name, source, target, points)
    else {
        return Err(fail_at(
            start,
            JStr::from("propertyView needs property, source, target and points"),
        ));
    };
    Ok(PPropertyView {
        property_class: JStr(prop_class),
        property_name: JStr(prop_name),
        property_span: prop_span.map(Box::new),
        source_view: JStr(source.0),
        source_view_span: Some(Box::new(source.1)),
        target_view: JStr(target.0),
        target_view_span: Some(Box::new(target.1)),
        points,
        source_information: Some(Box::new(r.span_from(start))),
    })
}

/// `parseGeneralizationView`: `generalizationView { source: id; target: id; points: [...]; }`.
fn parse_generalization_view(r: &mut Raw<'_>, start: Mark) -> R<PGeneralizationView> {
    r.skip_ws();
    r.expect('{')?;
    let mut source = None;
    let mut target = None;
    let mut points = None;
    let mut seen: HashSet<Vec<u16>> = HashSet::new();
    loop {
        r.skip_ws();
        if r.peek()? == ch('}') {
            break;
        }
        let key = r.word()?;
        once_view(&mut seen, &key, start)?;
        r.skip_ws();
        r.expect(':')?;
        r.skip_ws();
        match String::from_utf16_lossy(&key).as_str() {
            "source" => source = Some(view_ref(r)?),
            "target" => target = Some(view_ref(r)?),
            "points" => points = Some(r.points()?),
            _ => return Err(r.fail_j(jcat!("unknown generalizationView key '", key, "'"))),
        }
        r.skip_ws();
        r.expect(';')?;
    }
    r.expect('}')?;
    let (Some(source), Some(target), Some(points)) = (source, target, points) else {
        return Err(fail_at(
            start,
            JStr::from("generalizationView needs source, target and points"),
        ));
    };
    Ok(PGeneralizationView {
        source_view: JStr(source.0),
        source_view_span: Some(Box::new(source.1)),
        target_view: JStr(target.0),
        target_view_span: Some(Box::new(target.1)),
        points,
        source_information: Some(Box::new(r.span_from(start))),
    })
}
