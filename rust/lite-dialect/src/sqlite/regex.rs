// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! Java's `SqliteRegex` (`core/src/main/java/com/legend/sql/dialect/SqliteRegex.java`): a regular expression for
//! SQLite, which has none. A LITERAL pattern is compiled here to a Thompson NFA -- a constant table of its transitions,
//! the build-time half; the MATCH is the database's: a correlated `WITH RECURSIVE` walks the input's characters
//! through the table, the query-time half.
//!
//! The pattern is read as Java reads it, by UTF-16 unit (a class's bounds, an escape's character); a literal outside a
//! class takes a surrogate pair as its code point. The state numbers, the order of the edges, the predicates' order
//! (Java's `LinkedHashMap`) and each closure's ascending order (Java's `BitSet`) are Java's, so the table's text is.

use super::{P, pc};
use crate::refusal::Refusal;
use crate::writer::SqlWriter;
use std::collections::BTreeSet;

/// A set of characters: code-point ranges, possibly negated.
#[derive(Debug, Clone)]
struct CharSet {
    ranges: Vec<(i32, i32)>,
    negated: bool,
}

#[derive(Debug, Clone)]
enum Node {
    /// a capturing group, numbered by its '(' in the pattern
    Group(i32, Box<Node>),
    Lit(CharSet),
    Seq(Vec<Node>),
    Alt(Vec<Node>),
    /// at least `min` times, at most `max` (-1: unbounded)
    Repeat(Box<Node>, i32, i32),
    /// `^` (start) or `$` (end)
    Anchor(bool),
}

#[derive(Debug, Clone, Copy)]
struct Edge {
    from: usize,
    label: usize,
    to: usize,
}

const CI: usize = 0;
const MULTILINE: usize = 1;
const DOTALL: usize = 2;

struct Rx {
    /// the pattern as Java holds it: UTF-16 units
    pattern: Vec<u16>,
    text: String,
    at: usize,
    group_count: i32,
    lazy: bool,
    tracked: Vec<i32>,
    flags: [bool; 3],
    chars: Vec<Edge>,
    anchors: Vec<Edge>,
    marks: Vec<Edge>,
    epsilons: Vec<(usize, usize)>,
    /// Java's `LinkedHashMap<String, Integer>`: each predicate's text and index, in insertion order
    predicates: Vec<String>,
    states: usize,
}

fn unit(c: char) -> u16 {
    c as u16
}

impl Rx {
    fn new(pattern: &str) -> Rx {
        Rx {
            pattern: pattern.encode_utf16().collect(),
            text: pattern.to_owned(),
            at: 0,
            group_count: 0,
            lazy: false,
            tracked: Vec::new(),
            flags: [false; 3],
            chars: Vec::new(),
            anchors: Vec::new(),
            marks: Vec::new(),
            epsilons: Vec::new(),
            predicates: Vec::new(),
            states: 0,
        }
    }

    // ---- parsing

    fn more(&self) -> bool {
        self.at < self.pattern.len()
    }

    fn peek(&self) -> u16 {
        self.pattern[self.at]
    }

    fn next(&mut self) -> u16 {
        let c = self.pattern[self.at];
        self.at += 1;
        c
    }

    /// Java's `pattern.startsWith(prefix, at)`.
    fn starts_with(&self, prefix: &str, at: usize) -> bool {
        let p: Vec<u16> = prefix.encode_utf16().collect();
        at <= self.pattern.len() && self.pattern[at..].starts_with(&p)
    }

    /// Java's `pattern.indexOf(c, from)`.
    fn index_of(&self, c: char, from: usize) -> Option<usize> {
        let c = unit(c);
        self.pattern.iter().skip(from).position(|&u| u == c).map(|i| i + from)
    }

    fn substring(&self, from: usize, to: usize) -> String {
        String::from_utf16_lossy(&self.pattern[from..to])
    }

    fn refused(&self, what: &str) -> Refusal {
        Refusal::capability(format!(
            "a regular expression with {what} reached SQLite (pattern '{}'): its matcher takes RE2's literals, \
             classes, groups, alternation, repetition, anchors and the leading flags i, m, s",
            self.text
        ))
    }

    fn parse(&mut self) -> Result<Node, Refusal> {
        // the leading flags: (?ims)
        while self.starts_with("(?", self.at)
            && self.at + 2 < self.pattern.len()
            && "ims".encode_utf16().any(|f| f == self.pattern[self.at + 2])
        {
            let Some(close) = self.index_of(')', self.at) else {
                return Err(self.refused("an unclosed flag group"));
            };
            for f in self.pattern[self.at + 2..close].to_vec() {
                match char::from_u32(u32::from(f)) {
                    Some('i') => self.flags[CI] = true,
                    Some('m') => self.flags[MULTILINE] = true,
                    Some('s') => self.flags[DOTALL] = true,
                    _ => {
                        let shown = String::from_utf16_lossy(&[f]);
                        return Err(self.refused(&format!("the flag '{shown}'")));
                    }
                }
            }
            self.at = close + 1;
        }
        let n = self.alternation(0)?;
        if self.more() {
            return Err(self.refused("an unbalanced ')'"));
        }
        Ok(n)
    }

    /// `depth`: how many named groups wrap this one with no `>` to close their name -- Java reads such a group's
    /// name to `indexOf('>') + 1`, the pattern's start, and parses the pattern again inside it until its stack
    /// overflows.
    fn alternation(&mut self, depth: usize) -> Result<Node, Refusal> {
        let mut branches = vec![self.sequence(depth)?];
        while self.more() && self.peek() == unit('|') {
            self.next();
            branches.push(self.sequence(depth)?);
        }
        Ok(if branches.len() == 1 { branches.pop().expect("one branch") } else { Node::Alt(branches) })
    }

    fn sequence(&mut self, depth: usize) -> Result<Node, Refusal> {
        let mut items = Vec::new();
        while self.more() && self.peek() != unit('|') && self.peek() != unit(')') {
            items.push(self.repetition(depth)?);
        }
        Ok(if items.len() == 1 { items.pop().expect("one item") } else { Node::Seq(items) })
    }

    fn repetition(&mut self, depth: usize) -> Result<Node, Refusal> {
        let mut atom = self.atom(depth)?;
        while self.more() {
            let q = self.peek();
            let (min, max);
            if q == unit('*') {
                (min, max) = (0, -1);
            } else if q == unit('+') {
                (min, max) = (1, -1);
            } else if q == unit('?') {
                (min, max) = (0, 1);
            } else if q == unit('{') && self.bounded() {
                let close = self.index_of('}', self.at).expect("bounded found it");
                let body = self.substring(self.at + 1, close);
                let parts: Vec<&str> = body.split(',').collect();
                min = parse_int(parts[0])?;
                max = if parts.len() == 1 {
                    min
                } else if parts[1].is_empty() {
                    -1
                } else {
                    parse_int(parts[1])?
                };
                self.at = close;
            } else {
                break;
            }
            self.next();
            if self.more() && self.peek() == unit('?') {
                // lazy: the same strings match; a find prefers the shortest
                self.next();
                self.lazy = true;
            } else if self.more() && self.peek() == unit('+') {
                return Err(self.refused("a possessive quantifier"));
            }
            if matches!(atom, Node::Anchor(_)) {
                return Err(self.refused("a repeated anchor"));
            }
            atom = Node::Repeat(Box::new(atom), min, max);
        }
        Ok(atom)
    }

    /// At a `{n}`, `{n,}` or `{n,m}` (else a literal brace).
    fn bounded(&self) -> bool {
        let Some(close) = self.index_of('}', self.at) else {
            return false;
        };
        if close == 0 {
            return false;
        }
        let body = &self.pattern[self.at + 1..close];
        // [0-9]+(,[0-9]*)?
        let digits = body.iter().take_while(|&&u| (unit('0')..=unit('9')).contains(&u)).count();
        if digits == 0 {
            return false;
        }
        let rest = &body[digits..];
        rest.is_empty() || (rest[0] == unit(',') && rest[1..].iter().all(|&u| (unit('0')..=unit('9')).contains(&u)))
    }

    fn atom(&mut self, depth: usize) -> Result<Node, Refusal> {
        let c = self.next();
        match char::from_u32(u32::from(c)) {
            Some('(') => {
                if self.starts_with("?:", self.at) {
                    self.at += 2;
                } else if self.starts_with("?P<", self.at)
                    || (self.starts_with("?<", self.at)
                        && !self.starts_with("?<=", self.at)
                        && !self.starts_with("?<!", self.at))
                {
                    match self.index_of('>', self.at) {
                        Some(gt) => self.at = gt + 1,
                        None => {
                            // Java's indexOf answers -1: the name ends at the pattern's start, which is read again
                            // inside the group, and again, until the JVM's stack overflows
                            self.at = 0;
                            if depth > 64 {
                                return Err(Refusal { class: STACK_OVERFLOW, message: String::new() });
                            }
                            return self.capture(depth + 1);
                        }
                    }
                    return self.capture(depth);
                } else if self.more() && self.peek() == unit('?') {
                    let end = self.pattern.len().min(self.at + 3);
                    let shown = self.substring(self.at, end);
                    return Err(self.refused(&format!("the group '({shown}'")));
                } else {
                    return self.capture(depth);
                }
                let inner = self.alternation(depth)?;
                if !self.more() || self.next() != unit(')') {
                    return Err(self.refused("an unclosed group"));
                }
                Ok(inner)
            }
            Some('[') => Ok(Node::Lit(self.char_class()?)),
            Some('.') => Ok(Node::Lit(if self.flags[DOTALL] {
                CharSet { ranges: Vec::new(), negated: true }
            } else {
                CharSet { ranges: vec![(10, 10)], negated: true }
            })),
            Some('^') => Ok(Node::Anchor(true)),
            Some('$') => Ok(Node::Anchor(false)),
            Some('\\') => Ok(Node::Lit(self.escape(false)?)),
            Some(m @ (')' | '*' | '+' | '?')) => Err(self.refused(&format!("a misplaced '{m}'"))),
            _ => {
                // a high surrogate and the unit after it are one code point
                let cp = if (0xD800..0xDC00).contains(&c) && self.more() {
                    let lo = self.next();
                    if (0xDC00..0xE000).contains(&lo) {
                        0x10000 + ((i32::from(c) - 0xD800) << 10) + (i32::from(lo) - 0xDC00)
                    } else {
                        // Java's Character.toCodePoint does not check the low half: its arithmetic, as Java's
                        ((i32::from(c) << 10) + i32::from(lo)) + (0x10000 - (0xD800 << 10) - 0xDC00)
                    }
                } else {
                    i32::from(c)
                };
                Ok(Node::Lit(literal(cp)))
            }
        }
    }

    /// A capturing group's body, after its '(': numbered in order of its '('.
    fn capture(&mut self, depth: usize) -> Result<Node, Refusal> {
        self.group_count += 1;
        let number = self.group_count;
        let inner = self.alternation(depth)?;
        if !self.more() || self.next() != unit(')') {
            return Err(self.refused("an unclosed group"));
        }
        Ok(Node::Group(number, Box::new(inner)))
    }

    /// An escape after its backslash: a class (`\d \w \s`, negated in upper case) or a character.
    fn escape(&mut self, in_class: bool) -> Result<CharSet, Refusal> {
        if !self.more() {
            return Err(self.refused("a trailing backslash"));
        }
        let e = self.next();
        let digits = vec![(i32::from(b'0'), i32::from(b'9'))];
        let word = vec![
            (i32::from(b'0'), i32::from(b'9')),
            (i32::from(b'A'), i32::from(b'Z')),
            (i32::from(b'a'), i32::from(b'z')),
            (i32::from(b'_'), i32::from(b'_')),
        ];
        let space = vec![(i32::from(b'\t'), i32::from(b'\r')), (i32::from(b' '), i32::from(b' '))];
        let shown = String::from_utf16_lossy(&[e]);
        Ok(match char::from_u32(u32::from(e)) {
            Some('d') => CharSet { ranges: digits, negated: false },
            Some('w') => CharSet { ranges: word, negated: false },
            Some('s') => CharSet { ranges: space, negated: false },
            Some(n @ ('D' | 'W' | 'S')) => {
                if in_class {
                    return Err(self.refused("a negated class escape inside a class"));
                }
                let ranges = match n {
                    'D' => digits,
                    'W' => word,
                    _ => space,
                };
                CharSet { ranges, negated: true }
            }
            Some('t') => literal(i32::from(b'\t')),
            Some('n') => literal(i32::from(b'\n')),
            Some('r') => literal(i32::from(b'\r')),
            Some('f') => literal(0x0c),
            Some('v') => literal(0x0b),
            Some('b' | 'B' | 'A' | 'z' | 'Z' | 'Q' | 'E' | 'p' | 'P' | 'x' | 'k' | 'g') => {
                return Err(self.refused(&format!("the escape '\\{shown}'")));
            }
            _ => {
                if is_letter_or_digit(e) {
                    return Err(self.refused(&format!("the escape '\\{shown}'")));
                }
                literal(i32::from(e))
            }
        })
    }

    fn char_class(&mut self) -> Result<CharSet, Refusal> {
        let negated = self.more() && self.peek() == unit('^');
        if negated {
            self.next();
        }
        let mut ranges = Vec::new();
        let mut first = true;
        while self.more() && (self.peek() != unit(']') || first) {
            first = false;
            if self.starts_with("[:", self.at) {
                return Err(self.refused("a POSIX class"));
            }
            let lo;
            if self.peek() == unit('\\') {
                self.next();
                let esc = self.escape(true)?;
                if esc.ranges.len() != 1 || esc.ranges[0].0 != esc.ranges[0].1 {
                    ranges.extend(esc.ranges);
                    continue;
                }
                lo = esc.ranges[0].0;
            } else {
                lo = i32::from(self.next());
            }
            let mut hi = lo;
            if self.more()
                && self.peek() == unit('-')
                && self.at + 1 < self.pattern.len()
                && self.pattern[self.at + 1] != unit(']')
            {
                self.next();
                let h = self.next();
                hi = if h == unit('\\') { self.escape(true)?.ranges[0].0 } else { i32::from(h) };
            }
            ranges.push((lo, hi));
        }
        if !self.more() {
            return Err(self.refused("an unclosed class"));
        }
        self.next();
        Ok(CharSet { ranges, negated })
    }

    // ---- the NFA

    fn state(&mut self) -> usize {
        let s = self.states;
        self.states += 1;
        s
    }

    /// `n`'s states from `start`; its exit state.
    fn build(&mut self, n: &Node, start: usize) -> usize {
        match n {
            Node::Lit(set) => {
                let e = self.state();
                let label = self.predicate(set);
                self.chars.push(Edge { from: start, label, to: e });
                e
            }
            Node::Seq(items) => {
                let mut cur = start;
                for item in items {
                    cur = self.build(item, cur);
                }
                cur
            }
            Node::Alt(branches) => {
                let e = self.state();
                for b in branches {
                    let s = self.state();
                    self.epsilons.push((start, s));
                    let exit = self.build(b, s);
                    self.epsilons.push((exit, e));
                }
                e
            }
            Node::Repeat(node, min, max) => {
                let mut cur = start;
                for _ in 0..*min {
                    cur = self.build(node, cur);
                }
                if *max < 0 {
                    let lp = self.state();
                    self.epsilons.push((cur, lp));
                    let exit = self.build(node, lp);
                    self.epsilons.push((exit, lp));
                    return lp;
                }
                let e = self.state();
                self.epsilons.push((cur, e));
                for _ in *min..*max {
                    cur = self.build(node, cur);
                    self.epsilons.push((cur, e));
                }
                e
            }
            Node::Anchor(at_start) => {
                let e = self.state();
                self.anchors.push(Edge { from: start, label: if *at_start { 0 } else { 1 }, to: e });
                e
            }
            Node::Group(number, inner) => {
                let Some(k) = self.tracked.iter().position(|g| g == number) else {
                    return self.build(inner, start);
                };
                // its span's ends set as the walk passes them: open (2k), close (2k + 1)
                let open = self.state();
                self.marks.push(Edge { from: start, label: 2 * k, to: open });
                let e = self.state();
                let exit = self.build(inner, open);
                self.marks.push(Edge { from: exit, label: 2 * k + 1, to: e });
                e
            }
        }
    }

    /// The index of the SQL predicate testing one character (`?` stands for it) against `set`.
    fn predicate(&mut self, set: &CharSet) -> usize {
        let mut ranges = set.ranges.clone();
        if self.flags[CI] {
            for r in &set.ranges {
                fold(*r, 'A', 'Z', 32, &mut ranges);
                fold(*r, 'a', 'z', -32, &mut ranges);
            }
        }
        let mut sql = String::from(if set.negated { "NOT (" } else { "(" });
        for (i, (lo, hi)) in ranges.iter().enumerate() {
            if i > 0 {
                sql.push_str(" OR ");
            }
            if lo == hi {
                sql.push_str(&format!("unicode(?) = {lo}"));
            } else {
                sql.push_str(&format!("unicode(?) BETWEEN {lo} AND {hi}"));
            }
        }
        if ranges.is_empty() {
            sql.push('0');
        }
        sql.push(')');
        match self.predicates.iter().position(|p| *p == sql) {
            Some(i) => i,
            None => {
                self.predicates.push(sql);
                self.predicates.len() - 1
            }
        }
    }

    /// The states an epsilon walk reaches from `s`, `s` among them, ascending (Java's `BitSet`).
    fn closure(&self, s: usize) -> BTreeSet<usize> {
        let mut seen = BTreeSet::new();
        let mut todo = vec![s];
        while let Some(x) = todo.pop() {
            if !seen.insert(x) {
                continue;
            }
            for &(from, to) in &self.epsilons {
                if from == x {
                    todo.push(to);
                }
            }
        }
        seen
    }

    /// The table of `edges`, each to every state of its target's closure.
    fn rows(&self, edges: &[Edge]) -> String {
        let mut out = String::new();
        for e in edges {
            for u in self.closure(e.to) {
                if !out.is_empty() {
                    out.push_str(", ");
                }
                out.push_str(&format!("({}, {}, {u})", e.from, e.label));
            }
        }
        out
    }

    fn start_rows(&self, start: usize) -> String {
        self.closure(start).iter().map(|u| format!("({u})")).collect::<Vec<_>>().join(", ")
    }

    fn tests(&self, ch: &str) -> String {
        let mut tests = String::from("CASE __e.column2");
        for (i, p) in self.predicates.iter().enumerate() {
            tests.push_str(&format!(" WHEN {i} THEN {}", p.replace('?', ch)));
        }
        tests.push_str(" END");
        tests
    }

    fn bol_eol(&self) -> (&'static str, &'static str) {
        if self.flags[MULTILINE] {
            (
                "(__rx.p = 0 OR substr(__in.t, __rx.p, 1) = char(10))",
                "(__rx.p = length(__in.t) OR substr(__in.t, __rx.p + 1, 1) = char(10))",
            )
        } else {
            ("__rx.p = 0", "__rx.p = length(__in.t)")
        }
    }

    /// The search's prefix state and its entry: a leading state reads any character.
    fn search_prefix(&mut self) -> (usize, usize) {
        let start = self.state();
        let entry = self.state();
        self.epsilons.push((start, entry));
        let any = self.predicate(&CharSet { ranges: Vec::new(), negated: true });
        self.chars.push(Edge { from: start, label: any, to: start });
        (start, entry)
    }

    fn find_sql<'a>(&self, input: P<'a>, prefix: usize, accept: usize) -> P<'a> {
        let ch = "substr(__in.t, __rx.p + 1, 1)";
        let spans = self.tracked.len() * 2;
        let mut cols = String::new();
        let mut carry = String::new();
        let mut nulls = String::new();
        for i in 0..spans {
            cols.push_str(&format!(", g{i}"));
            carry.push_str(&format!(", __rx.g{i}"));
            nulls.push_str(", NULL");
        }
        let chars0 = self.rows(&self.chars);
        let anchors0 = self.rows(&self.anchors);
        let marks0 = self.rows(&self.marks);
        let start_rows = self.start_rows(prefix);
        let tests = self.tests(ch);
        let (bol, eol) = self.bol_eol();
        let mut set_marks = String::new();
        for i in 0..spans {
            set_marks.push_str(&format!(", CASE WHEN __k.column2 = {i} THEN __rx.p ELSE __rx.g{i} END"));
        }
        let mut result = String::from("json_array(m, p");
        let dir = if self.lazy { " ASC" } else { " DESC" };
        let mut order = format!("m ASC, p{dir}");
        for i in 0..spans {
            result.push_str(&format!(", g{i}"));
            order.push_str(&format!(", g{i}{dir}"));
        }
        result.push(')');
        let head = format!("), __rx(p, s, m{cols}) AS (SELECT 0, column1, 0{nulls} FROM (VALUES {start_rows})");
        let mut body = format!(
            " UNION SELECT __rx.p + 1, __e.column3, CASE WHEN __e.column1 = {prefix} THEN __rx.p + 1 ELSE __rx.m END\
             {carry} FROM __rx, __in JOIN (VALUES {chars0}) AS __e ON __e.column1 = __rx.s WHERE __rx.p < \
             length(__in.t) AND {tests}"
        );
        if !anchors0.is_empty() {
            body.push_str(&format!(
                " UNION SELECT __rx.p, __a.column3, __rx.m{carry} FROM __rx, __in JOIN (VALUES {anchors0}) AS __a ON \
                 __a.column1 = __rx.s WHERE CASE __a.column2 WHEN 0 THEN {bol} ELSE {eol} END"
            ));
        }
        if !marks0.is_empty() {
            body.push_str(&format!(
                " UNION SELECT __rx.p, __k.column3, __rx.m{set_marks} FROM __rx JOIN (VALUES {marks0}) AS __k ON \
                 __k.column1 = __rx.s"
            ));
        }
        body.push_str(&format!(") SELECT {result} FROM __rx WHERE s = {accept} ORDER BY {order} LIMIT 1)"));
        pc(move |w: &mut SqlWriter| {
            w.append("(WITH RECURSIVE __in(t) AS (SELECT ");
            input(w)?;
            w.append(&head).append(&body);
            Ok(())
        })
    }

    fn match_sql<'a>(&self, input: P<'a>, start: usize, accept: usize, full: bool) -> P<'a> {
        let ch = "substr(__in.t, __rx.p + 1, 1)";
        let chars0 = self.rows(&self.chars);
        let anchors0 = self.rows(&self.anchors);
        let start_rows = self.start_rows(start);
        let tests = self.tests(ch);
        let (bol, eol) = self.bol_eol();
        let mut body = format!("), __rx(p, s) AS (SELECT 0, column1 FROM (VALUES {start_rows})");
        if !chars0.is_empty() {
            body.push_str(&format!(
                " UNION SELECT __rx.p + 1, __e.column3 FROM __rx, __in JOIN (VALUES {chars0}) AS __e ON __e.column1 = \
                 __rx.s WHERE __rx.p < length(__in.t) AND {tests}"
            ));
        }
        if !anchors0.is_empty() {
            body.push_str(&format!(
                " UNION SELECT __rx.p, __a.column3 FROM __rx, __in JOIN (VALUES {anchors0}) AS __a ON __a.column1 = \
                 __rx.s WHERE CASE __a.column2 WHEN 0 THEN {bol} ELSE {eol} END"
            ));
        }
        body.push_str(&format!(
            ") SELECT CASE WHEN (SELECT t FROM __in) IS NULL THEN NULL ELSE EXISTS(SELECT 1 FROM __rx, __in WHERE \
             __rx.s = {accept}{}) END)",
            if full { " AND __rx.p = length(__in.t)" } else { "" }
        ));
        pc(move |w: &mut SqlWriter| {
            w.append("(WITH RECURSIVE __in(t) AS (SELECT ");
            input(w)?;
            w.append(&body);
            Ok(())
        })
    }
}

/// The JVM's class for a stack that overflowed (an `Error`, which the seam answers as an internal refusal).
pub const STACK_OVERFLOW: &str = "java.lang.StackOverflowError";

fn literal(cp: i32) -> CharSet {
    CharSet { ranges: vec![(cp, cp)], negated: false }
}

/// A range's part within `[lo, hi]`, shifted to the other case.
fn fold(r: (i32, i32), lo: char, hi: char, shift: i32, into: &mut Vec<(i32, i32)>) {
    let a = r.0.max(lo as i32);
    let b = r.1.min(hi as i32);
    if a <= b {
        into.push((a + shift, b + shift));
    }
}

/// Java's `Integer.parseInt` of digits: a value past an int refused in its words.
fn parse_int(s: &str) -> Result<i32, Refusal> {
    s.parse::<i32>()
        .map_err(|_| Refusal { class: Refusal::NUMBER_FORMAT, message: format!("For input string: \"{s}\"") })
}

/// Java's `Character.isLetterOrDigit(char)` of one UTF-16 unit: a letter (Lu, Ll, Lt, Lm, Lo) or a decimal digit
/// (Nd); a surrogate is neither. ASCII is exact; past it Rust's `Alphabetic` and `Numeric` classes stand for Java's
/// letter and digit (they take the letter-numbers, the other numbers and the alphabetic marks too, each from its own
/// Unicode version: `PARITY.md`).
fn is_letter_or_digit(u: u16) -> bool {
    match char::from_u32(u32::from(u)) {
        Some(c) if c.is_ascii() => c.is_ascii_alphanumeric(),
        Some(c) => c.is_alphabetic() || c.is_numeric(),
        None => false,
    }
}

/// Whether `input` matches `pattern` -- the whole text (`full`), or anywhere in it (a search: a leading state reads
/// any character). NULL for a NULL input. Java's `SqliteRegex.matches`: the pattern is compiled when the piece is
/// made.
pub fn matches<'a>(input: P<'a>, pattern: &str, full: bool) -> Result<P<'a>, Refusal> {
    let mut rx = Rx::new(pattern);
    let n = rx.parse()?;
    let start = rx.state();
    let mut entry = start;
    if !full {
        // a search: any prefix first
        entry = rx.state();
        rx.epsilons.push((start, entry));
        let any = rx.predicate(&CharSet { ranges: Vec::new(), negated: true });
        rx.chars.push(Edge { from: start, label: any, to: start });
    }
    let accept = rx.build(&n, entry);
    Ok(rx.match_sql(input, start, accept, full))
}

/// The leftmost match of `pattern` in `input`: a JSON array of its 0-based start and end and, for each of `groups`,
/// its group's start and end, or NULL for no match (Java's `SqliteRegex.find`).
pub fn find<'a>(input: P<'a>, pattern: &str, groups: &[i32]) -> Result<P<'a>, Refusal> {
    let mut rx = Rx::new(pattern);
    rx.tracked.extend_from_slice(groups);
    let n = rx.parse()?;
    for g in groups {
        if *g < 1 || *g > rx.group_count {
            return Err(rx.refused(&format!("no group {g}")));
        }
    }
    let (start, entry) = rx.search_prefix();
    let accept = rx.build(&n, entry);
    Ok(rx.find_sql(input, start, accept))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(p: P<'_>) -> String {
        let mut w = SqlWriter::new();
        p(&mut w).unwrap();
        w.into_text().unwrap()
    }

    fn input<'a>() -> P<'a> {
        pc(|w: &mut SqlWriter| {
            w.append("x");
            Ok(())
        })
    }

    #[test]
    fn a_literal_full_match_walks_its_table() {
        // 'ab': states 0 (start), 1 after a, 2 after b; one predicate per character
        let sql = text(matches(input(), "ab", true).unwrap());
        assert_eq!(
            sql,
            "(WITH RECURSIVE __in(t) AS (SELECT x), __rx(p, s) AS (SELECT 0, column1 FROM (VALUES (0)) UNION SELECT \
             __rx.p + 1, __e.column3 FROM __rx, __in JOIN (VALUES (0, 0, 1), (1, 1, 2)) AS __e ON __e.column1 = \
             __rx.s WHERE __rx.p < length(__in.t) AND CASE __e.column2 WHEN 0 THEN (unicode(substr(__in.t, __rx.p + \
             1, 1)) = 97) WHEN 1 THEN (unicode(substr(__in.t, __rx.p + 1, 1)) = 98) END) SELECT CASE WHEN (SELECT t \
             FROM __in) IS NULL THEN NULL ELSE EXISTS(SELECT 1 FROM __rx, __in WHERE __rx.s = 2 AND __rx.p = \
             length(__in.t)) END)"
        );
    }

    #[test]
    fn a_search_reads_any_prefix_first() {
        // the search's any-character predicate is registered before the pattern's own
        let sql = text(matches(input(), "a", false).unwrap());
        assert!(sql.contains("FROM (VALUES (0), (1))"), "{sql}");
        assert!(sql.contains("(VALUES (0, 0, 0), (0, 0, 1), (1, 1, 2))"), "{sql}");
        assert!(sql.contains("WHEN 0 THEN NOT (0) WHEN 1 THEN (unicode(substr(__in.t, __rx.p + 1, 1)) = 97)"), "{sql}");
        assert!(sql.ends_with("WHERE __rx.s = 2) END)"), "{sql}");
    }

    #[test]
    fn classes_flags_and_repetitions() {
        let sql = text(matches(input(), "(?i)[a-c\\d]+$", true).unwrap());
        // the class and its case folds, one predicate
        assert!(
            sql.contains(
                "(unicode(substr(__in.t, __rx.p + 1, 1)) BETWEEN 97 AND 99 OR unicode(substr(__in.t, __rx.p + 1, 1)) \
                 BETWEEN 48 AND 57 OR unicode(substr(__in.t, __rx.p + 1, 1)) BETWEEN 65 AND 67)"
            ),
            "{sql}"
        );
        assert!(sql.contains("WHERE CASE __a.column2 WHEN 0 THEN __rx.p = 0 ELSE __rx.p = length(__in.t) END"), "{sql}");
        let bounded = text(matches(input(), "a{2,3}", true).unwrap());
        // a{2}, then an optional third a: the second a reaches the exit state 3 by its epsilon
        assert!(bounded.contains("(VALUES (0, 0, 1), (1, 0, 2), (1, 0, 3), (2, 0, 3), (2, 0, 4))"), "{bounded}");
        let dotall = text(matches(input(), "(?s).", true).unwrap());
        assert!(dotall.contains("WHEN 0 THEN NOT (0)"), "{dotall}");
        let dot = text(matches(input(), ".", true).unwrap());
        assert!(dot.contains("WHEN 0 THEN NOT (unicode(substr(__in.t, __rx.p + 1, 1)) = 10)"), "{dot}");
        let multiline = text(matches(input(), "(?m)^a", true).unwrap());
        assert!(multiline.contains("WHEN 0 THEN (__rx.p = 0 OR substr(__in.t, __rx.p, 1) = char(10))"), "{multiline}");
    }

    #[test]
    fn a_find_tracks_its_groups_and_prefers_the_longest() {
        let sql = text(find(input(), "a(b+)", &[1]).unwrap());
        assert!(sql.contains("__rx(p, s, m, g0, g1) AS (SELECT 0, column1, 0, NULL, NULL FROM (VALUES (0), (1))"), "{sql}");
        assert!(sql.contains("ORDER BY m ASC, p DESC, g0 DESC, g1 DESC LIMIT 1)"), "{sql}");
        assert!(sql.contains("CASE WHEN __k.column2 = 0 THEN __rx.p ELSE __rx.g0 END"), "{sql}");
        let lazy = text(find(input(), "a+?", &[]).unwrap());
        assert!(lazy.contains("ORDER BY m ASC, p ASC LIMIT 1)"), "{lazy}");
    }

    #[test]
    fn what_the_matcher_does_not_take_is_refused_by_name() {
        let refused = |p: &str| match matches(input(), p, true) {
            Err(r) => r.message,
            Ok(_) => panic!("{p} compiled"),
        };
        let tail = "reached SQLite (pattern '";
        assert!(refused("a\\b").starts_with(&format!("a regular expression with the escape '\\b' {tail}a\\b'")));
        assert!(refused("(?=a)").starts_with("a regular expression with the group '(?=a'"));
        assert!(refused("a**+").contains("a possessive quantifier"));
        assert!(refused("^*").contains("a repeated anchor"));
        assert!(refused("[a").contains("an unclosed class"));
        assert!(refused("(a").contains("an unclosed group"));
        assert!(refused("a)").contains("an unbalanced ')'"));
        assert!(refused("[[:alpha:]]").contains("a POSIX class"));
        assert!(refused("[\\D]").contains("a negated class escape inside a class"));
        assert!(refused("a\\").contains("a trailing backslash"));
        assert!(refused("(?x)a").contains("the group '(?x)'"));
        assert!(refused("(?ix)a").contains("the flag 'x'"));
        assert!(refused("*").contains("a misplaced '*'"));
        assert!(refused("\\q").contains("the escape '\\q'"));
        let big = matches(input(), "a{99999999999}", true).err().unwrap();
        assert_eq!((big.class, big.message.as_str()), (Refusal::NUMBER_FORMAT, "For input string: \"99999999999\""));
        let group = find(input(), "a(b)", &[2]).err().unwrap();
        assert!(group.message.contains("with no group 2 reached SQLite"), "{}", group.message);
        // a named group whose name is never closed: Java parses the pattern again inside it until the stack overflows
        let overflow = matches(input(), "(?P<n", true).err().unwrap();
        assert_eq!(overflow.class, STACK_OVERFLOW);
    }

    #[test]
    fn javas_letter_or_digit() {
        assert!(is_letter_or_digit(u16::from(b'q')));
        assert!(is_letter_or_digit(u16::from(b'7')));
        assert!(!is_letter_or_digit(u16::from(b'.')));
        assert!(is_letter_or_digit(0x00E9)); // e acute
        assert!(is_letter_or_digit(0x0663)); // Arabic-Indic three
        assert!(!is_letter_or_digit(0xD800));
    }
}
