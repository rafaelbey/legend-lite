// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! The token cursor every parser walks with (core/'s `parser.TokenStreamCursor`): the lexical primitives (`peek`,
//! `advance` and its INVALID trap, `expect`, `consume`, the located `error`), the identifier and qualified-name
//! readers, THE escape decoder, the type-expression and multiplicity grammars, the `'''` literal's layout and span, and
//! `spanOf`. A trait, as Java's is an interface: an implementer exposes its tokens, cursor, dialect and span source id,
//! and inherits the rest. Also here: the two decoration readers of `ElementParser` the expression parser borrows for
//! a column spec's stereotypes and tagged values (`ElementParser.at(...).parseStereotypes()` / `parseTaggedValues()`),
//! as [`ElementCursor`].
//!
//! Every method answers `Result`: Java's exception is the `Err`, its class and message the [`JavaError`]'s.

use crate::doors::{Dialect, JavaError};
use crate::escapes::{unescape_java_like, unquote_path};
use crate::jcat;
use crate::jnum::{parse_long, parse_long as java_parse_long};
use crate::jtypes::JStr;
use crate::jtypes::JValue;
use crate::lexer::{TokenStream, TokenType};
use crate::nodes::{Span, boxed, concrete, cstring, name_ref};
use crate::records::{
    CInteger, Multiplicity, MultiplicityParameter, PStereotype, PTag, PTaggedValue, SourceInfo, TypeExpression,
    TypeExpressionColumn, TypeExpressionFunctionType, TypeExpressionGeneric, TypeExpressionOp,
    TypeExpressionRelationType, TypeExpressionSchemaAlgebra, TypeExpressionTypedParameter, ValueSpecification,
};
use crate::ustr::{ends_with, starts_with};
use lite_javacompat::character::is_whitespace;
use std::sync::Arc;

use TokenType as T;

pub type R<T> = Result<T, JavaError>;

/// `IDENTIFIER_TOKENS`: every token type that can stand for an identifier (`VALID_STRING`, `STRING`, and the
/// keywords Pure admits as identifiers; never `TRUE`/`FALSE`).
pub fn is_identifier_token(t: TokenType) -> bool {
    matches!(
        t,
        T::VALID_STRING
            | T::STRING
            | T::ALL
            | T::LET
            | T::ALL_VERSIONS
            | T::ALL_VERSIONS_IN_RANGE
            | T::IMPORT
            | T::CLASS
            | T::FUNCTION
            | T::PROFILE
            | T::ASSOCIATION
            | T::ENUM
            | T::EXTENDS
            | T::STEREOTYPES
            | T::TAGS
            | T::NATIVE
            | T::AS
            | T::MAPPING
            | T::INCLUDE
            | T::MAPPING_TESTABLE_SUITES
            | T::MAPPING_TESTS_QUERY
            | T::RUNTIME
            | T::SINGLE_CONNECTION_RUNTIME
            | T::MAPPINGS
            | T::CONNECTIONS
            | T::CONNECTION
            | T::DATABASE
            | T::TABLE
            | T::SCHEMA
            | T::VIEW
            | T::FILTER
            | T::MULTIGRAIN_FILTER
            | T::JOIN
            | T::RELATIONAL_AND
            | T::RELATIONAL_OR
            | T::ASSOCIATION_MAPPING
            | T::ENUMERATION_MAPPING
            | T::OTHERWISE
            | T::INLINE
            | T::PURE_MAPPING
            | T::RELATIONAL
            | T::STORE
            | T::TYPE
            | T::RELATIONAL_DATASOURCE_SPEC
            | T::RELATIONAL_AUTH_STRATEGY
            | T::H2
            | T::SERVICE
            | T::SERVICE_PATTERN
            | T::SERVICE_OWNERS
            | T::SERVICE_DOCUMENTATION
            | T::SERVICE_AUTO_ACTIVATE_UPDATES
            | T::SERVICE_EXEC
            | T::SERVICE_SINGLE
            | T::SERVICE_MAPPING
            | T::SERVICE_RUNTIME
            | T::RELATIONAL_DATABASE_CONNECTION
    )
}

/// `isFqnSegmentToken`: an identifier token that is not a quoted `STRING`.
pub fn is_fqn_segment_token(t: TokenType) -> bool {
    t != T::STRING && is_identifier_token(t)
}

/// `TokenStreamCursor.throwAt(tokens, tokenPos, message)`: a `ParseException` at the token's start; past the stream's
/// last token where ANTLR places `<EOF>` in the engine's parse, one past the input's last character, trailing blanks and
/// line breaks counted (the user's ruling 18 of 2026-11-09, docs/rust-port/PHASE2.md section 6); past the end of a window
/// that stops short of the input's end, at the end of its last token; unplaced on an empty stream. Lines and columns
/// 1-based, the column in code points (`TokenStream.columnOf`).
pub fn throw_at(tokens: &TokenStream<'_>, token_pos: usize, message: JStr) -> JavaError {
    let n = tokens.count();
    let char_pos = if token_pos < n {
        tokens.start(token_pos)
    } else if let (true, Some(end)) = (n > 0, tokens.input_end()) {
        // a quote open at the end of the input is no token to the engine's lexer: its parser refuses it, where it stands
        if is_open_quote(tokens, n - 1) {
            tokens.start(n - 1)
        } else {
            end
        }
    } else if n > 0 {
        tokens.end(n - 1)
    } else {
        return JavaError::parse_unplaced(message);
    };
    JavaError::parse(
        message,
        tokens.line_of(char_pos) as i32,
        tokens.column_of(char_pos) as i32,
    )
}

/// A refusal at the end of the input, where ANTLR places `<EOF>` in the engine's parse: one past the text's last
/// character, trailing blanks and line breaks counted (`throw_at`'s place is the end of the last token).
pub fn refused_at_eof(tokens: &TokenStream<'_>, message: JStr) -> JavaError {
    let (line, column) = eof_place(tokens);
    JavaError::parse(message, line, column).as_refusal()
}

/// A refusal at a token, or at the end of the input past the last one.
pub fn refused_at(tokens: &TokenStream<'_>, token_pos: usize, message: JStr) -> JavaError {
    if token_pos < tokens.count() {
        throw_at(tokens, token_pos, message).as_refusal()
    } else {
        refused_at_eof(tokens, message)
    }
}

/// A number token whose exponent has no digits (`1.5ed`, `1.5e+`): the engine's lexer ends the number before its
/// exponent, so its parser refuses at the exponent's letter (PARITY row 8).
pub fn refused_exponent(tokens: &TokenStream<'_>, token_pos: usize) -> JavaError {
    let text = tokens.text(token_pos);
    let e = text
        .iter()
        .position(|&c| c == u16::from(b'e') || c == u16::from(b'E'))
        .unwrap_or(0);
    let start = tokens.start(token_pos) + e;
    JavaError::parse(
        jcat!("Unexpected token '", &text[e..], "'"),
        tokens.line_of(start) as i32,
        tokens.column_of(start) as i32,
    )
    .as_refusal()
}

/// How much of a `%` literal's text legend-engine's lexer takes: the longest of its `Date` rule, `'%' '-'? D+ ('-' D+ ('-'
/// D+ ('T' DateTime TimeZone?)?)?)?`, and its `StrictTime` rule, `'%' D+ (':' D+ (':' D+ ('.' D+)?)?)?` (`DateTime` the same
/// digits, a `TimeZone` a sign and four digits). Lite's lexer reads on through any digit, `-`, `:`, `T`, `.`, `+` or `Z`.
pub fn engine_percent_literal_len(text: &[u16]) -> usize {
    let (date, strict) = engine_percent_rules(text);
    date.max(strict)
}

/// The lengths legend-engine's lexer's `Date` and `StrictTime` rules match of a `%` literal's text ([`engine_percent_literal_len`]),
/// 0 where one does not: the longer is its token, a tie its `DATE`.
pub fn engine_percent_rules(text: &[u16]) -> (usize, usize) {
    let digit = |p: usize| {
        text.get(p)
            .is_some_and(|c| (u16::from(b'0')..=u16::from(b'9')).contains(c))
    };
    let at = |p: usize, c: u8| text.get(p) == Some(&u16::from(c));
    let digits = |mut p: usize| {
        while digit(p) {
            p += 1;
        }
        p
    };
    // `D+ (':' D+ (':' D+ ('.' D+)?)?)?` from `p`, its first digit read
    let time = |p: usize| {
        let mut q = digits(p);
        if at(q, b':') && digit(q + 1) {
            q = digits(q + 1);
            if at(q, b':') && digit(q + 1) {
                q = digits(q + 1);
                if at(q, b'.') && digit(q + 1) {
                    q = digits(q + 1);
                }
            }
        }
        q
    };
    let strict = if digit(1) { time(1) } else { 0 };
    let first = if at(1, b'-') { 2 } else { 1 };
    let date = if digit(first) {
        let mut q = digits(first);
        if at(q, b'-') && digit(q + 1) {
            q = digits(q + 1);
            if at(q, b'-') && digit(q + 1) {
                q = digits(q + 1);
                if at(q, b'T') && digit(q + 1) {
                    q = time(q + 1);
                    if (at(q, b'+') || at(q, b'-')) && (1..=4).all(|k| digit(q + k)) {
                        q += 5;
                    }
                }
            }
        }
        q
    } else {
        0
    };
    (date, strict)
}

/// Where a `%` literal stands, for [`refused_percent_rest`].
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum PercentAt {
    /// An expression: the rest refused where it starts, but for a sign (the engine may read arithmetic there) or a lone
    /// `.` (it reads a property's dot), which lite's own reading is left.
    Expression,
    /// A relational milestoning, whose first reading takes words, dates, commas, booleans and `=`s: a word the rest begins
    /// with is read, and the refusal is at what follows it.
    Milestoning,
    /// A `primitiveValue` (a service test's parameter), which no arithmetic follows: the rest refused where it starts.
    Value,
}

/// A `%` literal lite's lexer reads further than legend-engine's ([`engine_percent_literal_len`]): the engine's parser meets
/// the rest as its next token, refused at the engine's dialect where it stops (the user's ruling 20 of 2026-11-10,
/// docs/rust-port/PHASE2.md section 6; register S358), by where the literal stands. None when the engine reads the whole
/// token.
pub fn refused_percent_rest(tokens: &TokenStream<'_>, token_pos: usize, place: PercentAt) -> Option<JavaError> {
    let text = tokens.text(token_pos);
    let n = engine_percent_literal_len(text);
    if n == 0 || n >= text.len() {
        return None;
    }
    let first = text[n];
    let mut at = n;
    if place == PercentAt::Value {
        // the rest is refused where it starts
    } else if place == PercentAt::Milestoning {
        if first < 128 && (first as u8).is_ascii_alphabetic() {
            while at < text.len()
                && text[at] < 128
                && ((text[at] as u8).is_ascii_alphanumeric() || text[at] == u16::from(b'_'))
            {
                at += 1;
            }
        }
    } else if first == u16::from(b'+') || first == u16::from(b'-') || (first == u16::from(b'.') && n + 1 == text.len())
    {
        return None;
    }
    let start = tokens.start(token_pos) + at;
    let what = if at < text.len() {
        text[at..].to_vec()
    } else if token_pos + 1 < tokens.count() {
        tokens.text(token_pos + 1).to_vec()
    } else {
        crate::ustr::u("<EOF>")
    };
    Some(
        JavaError::parse(
            jcat!("Unexpected token '", what, "'"),
            tokens.line_of(start) as i32,
            tokens.column_of(start) as i32,
        )
        .as_refusal(),
    )
}

/// An element body the engine's lexer takes as an island counted by braces alone (a mapping element's, a connection's), its
/// strings no strings to it: where that count ends the body other than where the tokens do, the refusal of what comes next
/// (the user's ruling 18 of 2026-11-09, docs/rust-port/PHASE2.md section 6). `open` is the body's `{` token.
pub fn brace_island_refusal(tokens: &TokenStream<'_>, open: usize) -> Option<JavaError> {
    let source = tokens.source();
    let end = tokens.input_end()?;
    let mut depth = 0i64;
    let mut raw_close = None;
    for (i, &c) in source.iter().enumerate().take(end).skip(tokens.start(open)) {
        if c == u16::from(b'{') {
            depth += 1;
        } else if c == u16::from(b'}') {
            depth -= 1;
            if depth == 0 {
                raw_close = Some(i);
                break;
            }
        }
    }
    let mut token_depth = 0i64;
    let mut token_close = None;
    for t in open..tokens.count() {
        match tokens.ty(t) {
            T::BRACE_OPEN => token_depth += 1,
            T::BRACE_CLOSE => {
                token_depth -= 1;
                if token_depth == 0 {
                    token_close = Some(tokens.start(t));
                    break;
                }
            }
            _ => {}
        }
    }
    match raw_close {
        None => {
            let (line, column) = eof_place(tokens);
            let at_the_end = JavaError::parse(JStr::from("Unexpected token '<EOF>'"), line, column - 1);
            token_close.is_some().then(|| at_the_end.as_refusal())
        }
        Some(close) if Some(close) == token_close => None,
        Some(close) => {
            let rest = tokens.lex_range(close + 1, end).ok()?;
            Some(if rest.count() > 0 {
                JavaError::parse(
                    jcat!("Unexpected token '", rest.text(0), "'"),
                    tokens.line_of(rest.start(0)) as i32,
                    tokens.column_of(rest.start(0)) as i32,
                )
                .as_refusal()
            } else {
                refused_at_eof(tokens, JStr::from("Unexpected token '<EOF>'"))
            })
        }
    }
}

/// One past the text's last character: its line and column.
pub fn eof_place(tokens: &TokenStream<'_>) -> (i32, i32) {
    let end = tokens.input_end().unwrap_or(tokens.source().len());
    (tokens.line_of(end) as i32, tokens.column_of(end) as i32)
}

/// Whether the token at `i` is a quoted literal the end of the input cut before its closing quote.
fn is_open_quote(tokens: &TokenStream<'_>, i: usize) -> bool {
    let quote = match tokens.ty(i) {
        T::STRING => u16::from(b'\''),
        T::QUOTED_STRING => u16::from(b'"'),
        _ => return false,
    };
    let text = tokens.text(i);
    if text.len() < 2 || text[text.len() - 1] != quote {
        return true;
    }
    // the last quote is escaped: an odd run of backslashes before it
    let backslashes = text[..text.len() - 1]
        .iter()
        .rev()
        .take_while(|&&c| c == u16::from(b'\\'))
        .count();
    backslashes % 2 == 1
}

/// The token a bracket's opener is closed by.
fn closer_of(open: TokenType) -> Option<TokenType> {
    Some(match open {
        T::BRACE_OPEN => T::BRACE_CLOSE,
        T::PAREN_OPEN => T::PAREN_CLOSE,
        T::BRACKET_OPEN => T::BRACKET_CLOSE,
        _ => return None,
    })
}

/// A closing token missing at the end of the input, as ANTLR reports it in the engine's parse (the user's ruling 18 of
/// 2026-11-09, docs/rust-port/PHASE2.md section 6). Its `<EOF>` is one past the last character when a decision fails
/// there (`Unexpected token '<EOF>'`); it is reported as a token inserted or removed, with `ParserErrorListener` taking the
/// column from the token's empty span, one column less (`Unexpected token`, the column 0 on an empty last line), in two
/// cases: the closer ends a top-level element, which only `<EOF>` follows, and the parse has read a list's item or a body's
/// member, so that `sync` fails at the back edge of its loop rather than on its way in. An opener with nothing after it is
/// the way in, but for a `Class` or `Association` body, whose members are a rule of their own.
fn missing_closer_a_column_early(window: &TokenStream<'_>, expected: TokenType, expression: bool) -> bool {
    // a window of a larger stream does not hold the brackets it is inside of
    let whole;
    let tokens = if window.input_end() == Some(window.source().len()) {
        whole = crate::lexer::tokenize(window.source());
        &whole
    } else {
        window
    };
    let mut open: Vec<(usize, usize)> = Vec::new();
    for i in 0..tokens.count() {
        let t = tokens.ty(i);
        if closer_of(t).is_some() {
            open.push((i, 0));
        } else if matches!(t, T::BRACE_CLOSE | T::PAREN_CLOSE | T::BRACKET_CLOSE) {
            open.pop();
        } else if matches!(t, T::COMMA | T::SEMI_COLON)
            && let Some(top) = open.last_mut()
        {
            top.1 += 1;
        }
    }
    let Some(&(opener, separators)) = open.last() else {
        return false;
    };
    if closer_of(tokens.ty(opener)) != Some(expected) {
        return false;
    }
    let element = (0..opener)
        .rev()
        .map(|i| tokens.ty(i))
        .find(|t| matches!(t, T::CLASS | T::ASSOCIATION | T::ENUM | T::PROFILE | T::FUNCTION));
    if opener + 1 == tokens.count() {
        // the way in: only a Class's or an Association's body at the top of the section is past it
        return open.len() == 1 && expected == T::BRACE_CLOSE && matches!(element, Some(T::CLASS | T::ASSOCIATION));
    }
    if separators > 0 {
        return true;
    }
    // one item and no separator: an element's own body is at its end, a list written inline in its rule is on its way in;
    // an expression door's text is the engine's `function go():Any[*]{let x = ...;}`, whose brackets are never the outermost
    !expression
        && open.len() == 1
        && expected != T::BRACKET_CLOSE
        && !(expected == T::PAREN_CLOSE && element == Some(T::FUNCTION))
        && !(expected == T::BRACE_CLOSE && element == Some(T::ENUM))
}

/// A short unicode escape in a `'''` text block or a quoted name: legend-engine's walkers unescape uncaught, so its
/// section catch-all answers (ruling 4 of docs/rust-port/PHASE2.md section 6; PARITY row 9). Lite's wording.
pub fn malformed_escape(what: &str, m: &str) -> JavaError {
    JavaError::refused_at_section(jcat!("malformed ", what, ": ", m), (0, 0))
}

/// `TokenStreamCursor.docStringValue`: the `'''` literal at `at` (its token's line and column), its text-block layout,
/// then Java-unescaped. Left open by the end of the input the engine reads `''` and refuses the third quote (PARITY
/// row 86); a short unicode escape is PARITY row 9's refusal, in a literal of one line (no text block to the engine)
/// also at that third quote.
pub fn doc_string_value(raw: &[u16], at: (i32, i32)) -> R<Vec<u16>> {
    let third_quote = |message: JStr| JavaError::parse(message, at.0, at.1 + 2).as_refusal();
    let layout = text_block_layout(raw).map_err(|_| third_quote(JStr::from("unterminated ''' literal")))?;
    unescape_java_like(&layout).map_err(|m| {
        let refusal = malformed_escape("string literal", m);
        if is_text_block(raw) {
            refusal.held_at(at)
        } else {
            third_quote(refusal.message.unwrap_or_default())
        }
    })
}

/// `TokenStreamCursor.isTextBlock`: a `'''` literal whose opening delimiter is followed, after blanks and tabs, by a
/// line terminator.
pub fn is_text_block(raw: &[u16]) -> bool {
    if raw.len() < 7 || !starts_with(raw, "'''") || !ends_with(raw, "'''") {
        return false;
    }
    let mut i = 3;
    while i < raw.len() && (raw[i] == u16::from(b' ') || raw[i] == u16::from(b'\t')) {
        i += 1;
    }
    i < raw.len() && (raw[i] == u16::from(b'\n') || raw[i] == u16::from(b'\r'))
}

/// `TokenStreamCursor.textBlockLayout`: line terminators normalized, the opening line and the closing `'''` dropped,
/// the minimum indent (over every non-blank line and the last) stripped, each line's trailing whitespace stripped.
pub fn text_block_layout(raw: &[u16]) -> R<Vec<u16>> {
    let (nl, cr) = (u16::from(b'\n'), u16::from(b'\r'));
    let mut normalized: Vec<u16> = Vec::with_capacity(raw.len());
    let mut i = 0;
    while i < raw.len() {
        if raw[i] == cr && i + 1 < raw.len() && raw[i + 1] == nl {
            normalized.push(nl);
            i += 2;
        } else {
            normalized.push(if raw[i] == cr { nl } else { raw[i] });
            i += 1;
        }
    }
    let len = normalized.len() as isize;
    let Some(first_nl) = normalized.iter().position(|&c| c == nl) else {
        // single-line: the naive strip
        return Ok(crate::ustr::substring(&normalized, 3, len - 3)?.to_vec());
    };
    let body = crate::ustr::substring(&normalized, first_nl as isize + 1, len - 3)?;
    let lines: Vec<&[u16]> = body.split(|&c| c == nl).collect();
    let mut min_indent = usize::MAX;
    for (i, line) in lines.iter().enumerate() {
        let mut leading = 0;
        while leading < line.len() && is_whitespace(line[leading]) {
            leading += 1;
        }
        if leading < line.len() || i == lines.len() - 1 {
            min_indent = min_indent.min(leading);
        }
    }
    if min_indent == usize::MAX {
        min_indent = 0;
    }
    let mut out: Vec<u16> = Vec::with_capacity(body.len());
    for (i, l) in lines.iter().enumerate() {
        if i > 0 {
            out.push(nl);
        }
        let line = &l[min_indent.min(l.len())..];
        let mut end = line.len();
        while end > 0 && is_whitespace(line[end - 1]) {
            end -= 1;
        }
        out.extend_from_slice(&line[..end]);
    }
    Ok(out)
}

/// The cursor over a token stream (Java's `TokenStreamCursor`): the four accessors an implementer provides, the
/// grammar it inherits.
pub trait Cursor<'s> {
    fn tokens(&self) -> &TokenStream<'s>;
    fn pos(&self) -> usize;
    fn set_pos(&mut self, pos: usize);
    /// Which of the three dialect levels this parse serves: there is no default.
    fn dialect(&self) -> Dialect;
    /// Whether this cursor reads an expression, not a section's elements.
    fn reads_an_expression(&self) -> bool {
        false
    }

    /// The `sourceId` stamped on spans: "" but for mapping test-suite query lambdas.
    fn span_source_id(&self) -> JStr {
        JStr::default()
    }

    // ---------------------------------------------------------------- lexical primitives

    /// The type under the cursor; `EOF` past the end.
    fn peek(&self) -> TokenType {
        let p = self.pos();
        if p < self.tokens().count() {
            self.tokens().ty(p)
        } else {
            T::EOF
        }
    }

    /// The type `offset` past the cursor; `EOF` past the end.
    fn peek_at(&self, offset: usize) -> TokenType {
        let i = self.pos() + offset;
        if i < self.tokens().count() {
            self.tokens().ty(i)
        } else {
            T::EOF
        }
    }

    /// The text under the cursor (the caller ensures it is not at the end).
    fn text(&self) -> &'s [u16] {
        self.tokens().text(self.pos())
    }

    /// Token texts `[from, to]` joined with no separator (ANTLR's `ctx.getText()`).
    fn compact_text(&self, from: usize, to: usize) -> Vec<u16> {
        let mut sb = Vec::new();
        let mut i = from;
        while i <= to && i < self.tokens().count() {
            sb.extend_from_slice(self.tokens().text(i));
            i += 1;
        }
        sb
    }

    /// The text under the cursor, or `<EOF>` past the end.
    fn safe_text(&self) -> Vec<u16> {
        if self.pos() < self.tokens().count() {
            self.text().to_vec()
        } else {
            crate::ustr::u("<EOF>")
        }
    }

    /// One token on, the INVALID trap first.
    fn advance(&mut self) -> R<()> {
        self.reject_invalid()?;
        let p = self.pos();
        self.set_pos(p + 1);
        Ok(())
    }

    /// Whether the parenthesis group at the cursor is immediately followed by another `(`.
    fn paren_group_followed_by_paren(&self) -> bool {
        let mut depth = 0i64;
        let ts = self.tokens();
        for i in self.pos()..ts.count() {
            let t = ts.ty(i);
            if t == T::PAREN_OPEN {
                depth += 1;
            } else if t == T::PAREN_CLOSE {
                depth -= 1;
                if depth == 0 {
                    return i + 1 < ts.count() && ts.ty(i + 1) == T::PAREN_OPEN;
                }
            }
        }
        false
    }

    fn at_end(&self) -> bool {
        self.pos() >= self.tokens().count()
    }

    /// `match`: consume the token if it is `t`.
    fn match_tok(&mut self, t: TokenType) -> R<bool> {
        if self.peek() == t {
            self.advance()?;
            return Ok(true);
        }
        Ok(false)
    }

    /// The refusal of the token under the cursor where the engine's grammar has no rule for it, a refusal in flight.
    fn unexpected(&self) -> JavaError {
        self.error(jcat!("Unexpected token '", self.safe_text(), "'"))
            .as_refusal()
    }

    /// The separator after an item of a comma list, whether a comma was read. At lite's and the platform's dialects the
    /// comma is optional and may trail. At the engine's the list is its grammar's `item (COMMA item)*` (the user's ruling
    /// 18 of 2026-11-09, docs/rust-port/PHASE2.md section 6; register S341): a comma has an item after it, and the next
    /// token is a comma or closes the list.
    fn match_list_comma(&mut self) -> R<bool> {
        if !self.dialect().refuses_lite_extensions() {
            return self.match_tok(T::COMMA);
        }
        let closes = |t: TokenType| matches!(t, T::BRACKET_CLOSE | T::PAREN_CLOSE | T::BRACE_CLOSE | T::ISLAND_END);
        if self.at_end() || closes(self.peek()) {
            return Ok(false);
        }
        if self.peek() != T::COMMA {
            return Err(self.unexpected());
        }
        self.advance()?;
        if closes(self.peek()) {
            return Err(self.unexpected());
        }
        Ok(true)
    }

    /// [`Cursor::match_list_comma`], where it does not matter whether a comma was read.
    fn list_comma(&mut self) -> R<()> {
        self.match_list_comma().map(|_| ())
    }

    /// `expect(type)`: the generic `expected X but found Y ('text')` refusal.
    fn expect(&mut self, t: TokenType) -> R<()> {
        if self.peek() != t {
            self.reject_invalid()?;
            return Err(self.missing(
                t,
                jcat!(
                    "expected ",
                    t,
                    " but found ",
                    self.peek(),
                    " ('",
                    self.safe_text(),
                    "')"
                ),
            ));
        }
        self.advance()
    }

    /// The refusal of a token that is not there: [`Cursor::error`], but for a closer missing at the end of the input,
    /// one column less where the engine's parser reports it so ([`missing_closer_a_column_early`]).
    fn missing(&self, t: TokenType, message: JStr) -> JavaError {
        let e = self.error(message);
        let at_the_end = self.at_end() && self.tokens().input_end().is_some();
        if at_the_end && missing_closer_a_column_early(self.tokens(), t, self.reads_an_expression()) {
            return e.a_column_before();
        }
        e
    }

    /// `expect(type, customMessage)`.
    fn expect_msg(&mut self, t: TokenType, message: &str) -> R<()> {
        if self.peek() != t {
            return Err(self.missing(t, JStr::from(message)));
        }
        self.advance()
    }

    /// `expect(type, customMessage)` with a composed message, built only on failure (it has no side effect).
    fn expect_with(&mut self, t: TokenType, message: impl FnOnce() -> JStr) -> R<()> {
        if self.peek() != t {
            return Err(self.missing(t, message()));
        }
        self.advance()
    }

    /// `consumeBoundedInt(what)`: `consumeLong` narrowed to `int`, a positioned refusal above it.
    fn consume_bounded_int(&mut self, what: &str) -> R<i32> {
        let mark = self.pos();
        let v = self.consume_long()?;
        if v > i64::from(i32::MAX) {
            return Err(throw_at(self.tokens(), mark, jcat!(what, " out of range: ", v)));
        }
        Ok(v as i32)
    }

    /// `consumeLong`: an INTEGER's value, a positioned refusal on any other token or an overflow.
    fn consume_long(&mut self) -> R<i64> {
        if self.peek() != T::INTEGER {
            self.reject_invalid()?;
            return Err(self.error(jcat!(
                "expected ",
                T::INTEGER,
                " but found ",
                self.peek(),
                " ('",
                self.safe_text(),
                "')"
            )));
        }
        let t = self.text();
        match parse_long(t) {
            Ok(v) => {
                self.advance()?;
                Ok(v)
            }
            Err(_) => Err(self.error(jcat!("integer literal out of range: '", t, "'"))),
        }
    }

    /// `consume(type)`: the token's text, advanced past.
    fn consume(&mut self, t: TokenType) -> R<&'s [u16]> {
        if self.peek() != t {
            self.reject_invalid()?;
            return Err(self.error(jcat!(
                "expected ",
                t,
                " but found ",
                self.peek(),
                " ('",
                self.safe_text(),
                "')"
            )));
        }
        let text = self.text();
        self.advance()?;
        Ok(text)
    }

    /// `error(message)`: a `ParseException` at the cursor.
    fn error(&self, message: JStr) -> JavaError {
        throw_at(self.tokens(), self.pos(), message)
    }

    /// A refusal the engine answers from its section catch-all ([`JavaError::refused_at_section`]), held at this token.
    fn section_refusal(&self, message: JStr) -> JavaError {
        JavaError::refused_at_section(message, self.error(JStr::default()).position.unwrap_or((0, 0)))
    }

    fn error_str(&self, message: &str) -> JavaError {
        self.error(JStr::from(message))
    }

    // ---------------------------------------------------------------- identifiers and qualified names

    /// The index of the `~` that ends a unit path (`RomanLength~Pes`) starting at token `i`.
    fn unit_path_end(&self, mut i: usize) -> Option<usize> {
        let ts = self.tokens();
        let mut saw_name = false;
        while i < ts.count() && is_fqn_segment_token(ts.ty(i)) {
            saw_name = true;
            i += 1;
            if i < ts.count() && ts.ty(i) == T::PATH_SEPARATOR {
                i += 1;
                continue;
            }
            break;
        }
        (saw_name && i < ts.count() && ts.ty(i) == T::TILDE).then_some(i)
    }

    /// `parseQualifiedName`: `Foo` or `my::pkg::Foo`; a quoted or digit-leading first segment admitted.
    fn parse_qualified_name(&mut self) -> R<Vec<u16>> {
        if !is_fqn_segment_token(self.peek())
            && self.peek() != T::STRING
            && !(self.peek() == T::INTEGER && self.int_leads_identifier())
        {
            return Err(self.error(jcat!("expected type name, got ", self.peek())));
        }
        let mut sb = self.fqn_segment_text()?;
        while self.peek() == T::PATH_SEPARATOR {
            self.advance()?;
            sb.extend(crate::ustr::u("::"));
            sb.extend(self.fqn_segment_text()?);
        }
        Ok(sb)
    }

    /// `intLeadsIdentifier`: an INTEGER glued to an adjacent identifier piece reads as a digit-leading name.
    fn int_leads_identifier(&self) -> bool {
        let p = self.pos();
        let ts = self.tokens();
        p + 1 < ts.count() && ts.ty(p + 1) == T::VALID_STRING && ts.end(p) == ts.start(p + 1)
    }

    /// `fqnSegmentText`: one segment -- an identifier, a quoted name kept raw, or a digit-leading run glued back.
    fn fqn_segment_text(&mut self) -> R<Vec<u16>> {
        if self.peek() == T::STRING {
            let seg = self.text().to_vec();
            self.advance()?;
            return Ok(seg);
        }
        if self.peek() == T::INTEGER && self.int_leads_identifier() {
            let mut seg = self.text().to_vec();
            self.advance()?;
            while !self.at_end()
                && (self.peek() == T::VALID_STRING || self.peek() == T::INTEGER)
                && self.tokens().end(self.pos() - 1) == self.tokens().start(self.pos())
            {
                seg.extend_from_slice(self.text());
                self.advance()?;
            }
            return Ok(seg);
        }
        if !is_fqn_segment_token(self.peek()) {
            return Err(self.error_str("expected identifier after '::' in qualified name"));
        }
        let seg = self.text().to_vec();
        self.advance()?;
        Ok(seg)
    }

    /// THE lexer-error trap: an INVALID token never flows into a parse.
    fn reject_invalid(&self) -> R<()> {
        if self.peek() == T::INVALID {
            return Err(self.error(jcat!("unlexable input: '", self.safe_text(), "'")));
        }
        Ok(())
    }

    /// `parseIdentifier`: a single identifier, a quoted one unquoted and unescaped. A cursor that overrides it (the
    /// Database grammar's) reaches this one as [`base_parse_identifier`], Java's `TokenStreamCursor.super`.
    fn parse_identifier(&mut self) -> R<Vec<u16>> {
        base_parse_identifier(self)
    }

    /// THE quoted-name decoder (`unquoteAndUnescape(raw, at)`, this cursor the `at`).
    fn unquote_and_unescape(&self, raw: &[u16]) -> R<Vec<u16>> {
        let q = u16::from(b'\'');
        if raw.len() < 2 || raw[0] != q || raw[raw.len() - 1] != q {
            return Err(self.error_str("malformed quoted name: missing surrounding quotes"));
        }
        self.unescape_body(&raw[1..raw.len() - 1], "quoted name")
    }

    /// THE escape table (`unescapeBody(body, what, at)`): a backslash escaping the body's end a located error, a short
    /// unicode escape another.
    fn unescape_body(&self, body: &[u16], what: &str) -> R<Vec<u16>> {
        let bs = u16::from(b'\\');
        if !body.contains(&bs) {
            return Ok(body.to_vec());
        }
        let mut i = 0;
        while i < body.len() {
            i += if body[i] == bs { 2 } else { 1 };
        }
        if i > body.len() {
            return Err(self.error(jcat!("malformed ", what, ": trailing backslash")));
        }
        unescape_java_like(body).map_err(|m| self.error(jcat!("malformed ", what, ": ", m)))
    }

    // ---------------------------------------------------------------- type expressions

    /// `parseType`: a function type, a relation type, or a name with its unit, type-variable values and arguments.
    fn parse_type(&mut self) -> R<TypeExpression> {
        if self.peek() == T::BRACE_OPEN {
            return self.parse_function_type();
        }
        if self.peek() == T::PAREN_OPEN {
            return self.parse_relation_type();
        }
        let start = self.pos();
        let mut name = self.parse_qualified_name()?;
        if self.peek() == T::TILDE && is_fqn_segment_token(self.peek_at(1)) {
            self.advance()?;
            name.push(u16::from(b'~'));
            name.extend_from_slice(self.text());
            self.advance()?;
            // `unitName` takes no arguments in the engine's grammar (ruling 18)
            if self.dialect().holds_to_the_engines_walk() && self.peek() == T::LESS_THAN {
                return Err(self.unexpected());
            }
        }
        if self.peek() == T::PAREN_OPEN {
            let tvv = self.parse_type_variable_values()?;
            return Ok(generic(
                name,
                Vec::new(),
                Vec::new(),
                tvv,
                self.span(start, self.pos() - 1),
            ));
        }
        let angle = self.pos();
        if !self.match_tok(T::LESS_THAN)? {
            return Ok(name_ref(JStr(name), self.span(start, self.pos() - 1)));
        }
        let mut args = Vec::new();
        if self.peek() == T::PIPE {
            self.advance()?;
            let mut mult = vec![JStr(self.parse_multiplicity_argument_text()?)];
            while self.match_tok(T::COMMA)? {
                mult.push(JStr(self.parse_multiplicity_argument_text()?));
            }
            self.expect(T::GREATER_THAN)?;
            return Ok(generic(name, args, mult, Vec::new(), self.span(start, self.pos() - 1)));
        }
        args.push(self.parse_type_argument()?);
        while self.match_tok(T::COMMA)? {
            args.push(self.parse_type_argument()?);
        }
        // the engine's grammar has no schema algebra: it gives up on the arguments, at their `<` (ruling 18; PARITY row 87)
        if self.dialect().holds_to_the_engines_walk()
            && args
                .iter()
                .any(|a| matches!(a, TypeExpression::TypeExpressionSchemaAlgebra(_)))
        {
            self.set_pos(angle);
            return Err(self.unexpected());
        }
        let mut mult = Vec::new();
        if self.match_tok(T::PIPE)? {
            mult.push(JStr(self.parse_multiplicity_argument_text()?));
            while self.match_tok(T::COMMA)? {
                mult.push(JStr(self.parse_multiplicity_argument_text()?));
            }
        }
        self.expect(T::GREATER_THAN)?;
        let tvv = if !self.at_end() && self.peek() == T::PAREN_OPEN {
            self.parse_type_variable_values()?
        } else {
            Vec::new()
        };
        Ok(generic(name, args, mult, tvv, self.span(start, self.pos() - 1)))
    }

    /// `parseTypeVariableValues`: `(200)`, `(10, 2)`, `('ok')` -- integers and strings. An integer past a long is the
    /// uncaught `NumberFormatException` of Java's `Long.parseLong`.
    fn parse_type_variable_values(&mut self) -> R<Vec<ValueSpecification>> {
        self.advance()?; // past '('
        let mut tvv = Vec::new();
        while !self.at_end() && self.peek() != T::PAREN_CLOSE {
            if self.peek() == T::COMMA {
                self.advance()?;
                if self.dialect().refuses_lite_extensions() && self.peek() == T::PAREN_CLOSE {
                    return Err(self.unexpected());
                }
                continue;
            }
            let p = self.pos();
            if self.peek() == T::INTEGER {
                // past a long: the engine's walker parses it uncaught too, so its section catch-all answers (PARITY
                // row 6), in the JDK's words
                let value = java_parse_long(self.text()).map_err(|m| self.section_refusal(m))?;
                tvv.push(ValueSpecification::CInteger(Arc::new(CInteger {
                    value: JValue::Long(value),
                    pos: self.span(p, p),
                })));
            } else if self.peek() == T::STRING {
                let v = self.unquote_and_unescape(self.text())?;
                tvv.push(cstring(JStr(v), self.span(p, p)));
            } else {
                return Err(self.error(jcat!(
                    "type variable values support integer and string literals, got ",
                    self.peek()
                )));
            }
            self.advance()?;
        }
        self.expect(T::PAREN_CLOSE)?;
        Ok(tvv)
    }

    /// `parseTypeArgument`: a type with the schema-algebra operators (`=`, then `+`/`-` chained, then `⊆`).
    fn parse_type_argument(&mut self) -> R<TypeExpression> {
        let mut result = self.parse_type()?;
        if self.match_tok(T::EQUAL)? {
            let right = self.parse_type()?;
            result = algebra(result, TypeExpressionOp::EQUAL, right);
        }
        while self.peek() == T::PLUS || self.peek() == T::MINUS {
            let op = if self.match_tok(T::PLUS)? {
                TypeExpressionOp::UNION
            } else {
                self.match_tok(T::MINUS)?;
                TypeExpressionOp::DIFFERENCE
            };
            let right = self.parse_type()?;
            result = algebra(result, op, right);
        }
        if self.match_tok(T::SUBSET)? {
            let super_set = self.parse_type()?;
            result = algebra(result, TypeExpressionOp::SUBSET, super_set);
        }
        Ok(result)
    }

    /// `parseMultiplicityArgumentText`: an unbracketed multiplicity argument, kept as text.
    fn parse_multiplicity_argument_text(&mut self) -> R<Vec<u16>> {
        let t = self.peek();
        if t == T::STAR {
            self.advance()?;
            return Ok(crate::ustr::u("*"));
        }
        if t == T::INTEGER {
            let lower = self.text().to_vec();
            self.advance()?;
            if self.match_tok(T::DOT_DOT)? {
                if self.match_tok(T::STAR)? {
                    return Ok(jcat!(lower, "..*").0);
                }
                if self.peek() == T::INTEGER {
                    let upper = self.text();
                    self.advance()?;
                    return Ok(jcat!(lower, "..", upper).0);
                }
                return Err(self.error_str("expected integer or '*' after '..' in a multiplicity argument"));
            }
            return Ok(lower);
        }
        if is_identifier_token(t) {
            if self.dialect().refuses_lite_extensions() {
                return Err(self.error(jcat!(
                    "The multiplicity parameter '",
                    self.text(),
                    "' as a type argument is not supported in Legend Engine"
                )));
            }
            let name = self.text().to_vec();
            self.advance()?;
            return Ok(name);
        }
        Err(self.error(jcat!(
            "expected a multiplicity argument (n, n..m, *, or a multiplicity parameter), got ",
            t
        )))
    }

    /// `parseMultiplicity`: `[N]`, `[N..M]`, `[N..*]`, `[*]` or `[m]`.
    fn parse_multiplicity(&mut self) -> R<Multiplicity> {
        self.expect(T::BRACKET_OPEN)?;
        let t = self.peek();
        let result = if t == T::STAR {
            self.advance()?;
            concrete(0, None)
        } else if t == T::INTEGER {
            let lower = self.consume_bounded_int("multiplicity bound")?;
            if self.match_tok(T::DOT_DOT)? {
                if self.match_tok(T::STAR)? {
                    concrete(lower, None)
                } else if self.peek() == T::INTEGER {
                    // no upper >= lower check: the engine's grammar has none
                    let upper = self.consume_bounded_int("multiplicity bound")?;
                    concrete(lower, Some(upper))
                } else {
                    return Err(self.error_str("expected integer or '*' after '..' in multiplicity"));
                }
            } else {
                concrete(lower, Some(lower))
            }
        } else if is_identifier_token(t) {
            // the engine's walker fails on it, so its section catch-all answers (the user's ruling 12 of 2026-11-09,
            // docs/rust-port/PHASE2.md section 6; PARITY row 70)
            if self.dialect().holds_to_the_engines_walk() {
                return Err(self.section_refusal(jcat!(
                    "the multiplicity parameter '",
                    self.text(),
                    "' is not supported in Legend Engine"
                )));
            }
            let name = JStr::from(self.text());
            self.advance()?;
            Multiplicity::MultiplicityParameter(Box::new(MultiplicityParameter { name }))
        } else {
            return Err(self.error(jcat!("expected multiplicity bound or parameter, got ", t)));
        };
        self.expect(T::BRACKET_CLOSE)?;
        Ok(result)
    }

    /// `{Type[mult], ... -> Type[mult]}`: a declared lite extension the engine's dialect refuses in its own words.
    fn parse_function_type(&mut self) -> R<TypeExpression> {
        let start = self.pos();
        self.expect(T::BRACE_OPEN)?;
        let mut params = Vec::new();
        if self.peek() != T::ARROW {
            params.push(Box::new(self.parse_typed_parameter()?));
            while self.match_tok(T::COMMA)? {
                params.push(Box::new(self.parse_typed_parameter()?));
            }
        }
        self.expect(T::ARROW)?;
        let result_type = self.parse_type()?;
        let result_mult = self.parse_multiplicity()?;
        self.expect(T::BRACE_CLOSE)?;
        if self.dialect().refuses_lite_extensions() {
            return Err(self.error(jcat!(
                "The type ",
                self.compact_text(start, self.pos() - 1),
                " is not supported yet"
            )));
        }
        Ok(TypeExpression::TypeExpressionFunctionType(Box::new(
            TypeExpressionFunctionType {
                parameters: params,
                result: Box::new(TypeExpressionTypedParameter {
                    r#type: result_type,
                    multiplicity: result_mult,
                }),
            },
        )))
    }

    /// `(col:Type[mult], ...)`.
    fn parse_relation_type(&mut self) -> R<TypeExpression> {
        self.expect(T::PAREN_OPEN)?;
        let mut columns = Vec::new();
        if self.peek() != T::PAREN_CLOSE {
            columns.push(Box::new(self.parse_type_relation_column()?));
            while self.match_tok(T::COMMA)? {
                columns.push(Box::new(self.parse_type_relation_column()?));
            }
        }
        self.expect(T::PAREN_CLOSE)?;
        Ok(TypeExpression::TypeExpressionRelationType(Box::new(
            TypeExpressionRelationType { columns },
        )))
    }

    /// One relation-type column (`TokenStreamCursor.parseRelationColumn`): its name or `?`, its type or `?`, its
    /// multiplicity (`[0..1]` undeclared), spanning name through the type and a declared multiplicity.
    fn parse_type_relation_column(&mut self) -> R<TypeExpressionColumn> {
        let name_tok = self.pos();
        let col_name = if self.match_tok(T::QUESTION)? {
            crate::ustr::u("?")
        } else {
            self.parse_identifier()?
        };
        self.expect(T::COLON)?;
        let q_tok = self.pos();
        let col_type = if self.match_tok(T::QUESTION)? {
            name_ref(JStr::from("?"), self.span(q_tok, q_tok))
        } else {
            self.parse_type()?
        };
        let multiplicity = if self.peek() == T::BRACKET_OPEN {
            self.parse_multiplicity()?
        } else {
            concrete(0, Some(1))
        };
        let pos = self.span(name_tok, self.pos() - 1);
        Ok(TypeExpressionColumn {
            name: JStr(col_name),
            r#type: col_type,
            multiplicity,
            pos,
        })
    }

    /// `Type[mult]` in a function-type signature.
    fn parse_typed_parameter(&mut self) -> R<TypeExpressionTypedParameter> {
        let t = self.parse_type()?;
        let multiplicity = self.parse_multiplicity()?;
        Ok(TypeExpressionTypedParameter {
            r#type: t,
            multiplicity,
        })
    }

    // ---------------------------------------------------------------- the element grammars' helpers

    /// `peekText(offset)`: the text `offset` past the cursor, `""` past the end (an empty string matches no keyword).
    fn peek_text(&self, offset: usize) -> &'s [u16] {
        let i = self.pos() + offset;
        if i < self.tokens().count() {
            self.tokens().text(i)
        } else {
            &[]
        }
    }

    /// `text()` where Java reads it at the stream's end: the lexer's arrays carry zeroed slack past `count`, so the
    /// text there is `""` (at the end of an exactly-sized slice it would be Java's `IndexOutOfBounds`, which no caller
    /// reaches).
    fn slack_text(&self) -> &'s [u16] {
        self.peek_text(0)
    }

    /// `skipBalancedBlock`: one balanced `{...}`, `(...)` or `[...]` block, every bracket kind balanced.
    fn skip_balanced_block(&mut self) -> R<()> {
        let mut depth = 0i64;
        let mut started = false;
        while !self.at_end() {
            let t = self.peek();
            if matches!(t, T::BRACE_OPEN | T::PAREN_OPEN | T::BRACKET_OPEN) {
                depth += 1;
                started = true;
            } else if matches!(t, T::BRACE_CLOSE | T::PAREN_CLOSE | T::BRACKET_CLOSE) {
                depth -= 1;
            }
            self.advance()?;
            if started && depth == 0 {
                return Ok(());
            }
        }
        Ok(())
    }

    /// `consumeStringLiteral(what)`: a STRING's decoded body, decoded before the cursor moves past it.
    fn consume_string_literal(&mut self, what: &str) -> R<Vec<u16>> {
        if self.at_end() || self.peek() != T::STRING {
            self.reject_invalid()?;
            return Err(self.error(jcat!("expected a string literal for ", what, ", got ", self.peek())));
        }
        let v = self.unquote_and_unescape(self.text())?;
        self.advance()?;
        Ok(v)
    }

    /// `reconstructText(startToken, endToken)`: the raw source spanned by tokens `[start, end)`.
    fn reconstruct_text(&self, start: usize, end: usize) -> Vec<u16> {
        if start >= end {
            return Vec::new();
        }
        let ts = self.tokens();
        ts.source()[ts.start(start)..ts.end(end - 1)].to_vec()
    }

    /// `documentationSpan(tok, raw)`: a documentation literal's span over the whole literal, from its text -- its
    /// end column the last line's length (the literal's whole length when it has one line).
    fn documentation_span(&self, tok: usize, raw: &[u16]) -> SourceInfo {
        let ts = self.tokens();
        let nl = u16::from(b'\n');
        let new_lines = raw.iter().filter(|&&c| c == nl).count() as i32;
        let last_nl = raw.iter().rposition(|&c| c == nl).map_or(-1, |i| i as i32);
        SourceInfo {
            source_id: self.span_source_id(),
            start_line: ts.start_line(tok) as i32,
            start_column: ts.start_column(tok) as i32,
            end_line: ts.start_line(tok) as i32 + new_lines,
            end_column: raw.len() as i32 - last_nl - 1,
        }
    }

    /// `parseDocumentation`: a `'''` literal at declaration position, consumed whatever its shape; a plain quoted
    /// string before a declaration keyword the engine's refusal.
    fn parse_documentation(&mut self) -> R<Option<Documentation>> {
        if self.peek() == T::STRING && self.pos() + 1 < self.tokens().count() && is_declaration_head(self.peek_at(1)) {
            return Err(self.error_str(DOC_MULTI_LINE));
        }
        if self.peek() != T::DOC_STRING {
            return Ok(None);
        }
        let tok = self.pos();
        let raw = self.text();
        self.advance()?;
        Ok(Some(Documentation {
            token: tok,
            raw: raw.to_vec(),
            span: self.documentation_span(tok, raw),
        }))
    }

    /// `taggedValuesWithDocumentation`: the documentation folded in as the FIRST tagged value (profile
    /// `meta::pure::profiles::doc`, tag `doc`, its value canonicalized, never unescaped, flagged multi-line); a
    /// non-block literal refused at its token, an explicit `doc.doc` beside it refused at that tag's span start.
    fn tagged_values_with_documentation(
        &self,
        doc: Option<Documentation>,
        tagged_values: Vec<Box<PTaggedValue>>,
    ) -> R<Vec<Box<PTaggedValue>>> {
        let Some(doc) = doc else {
            return Ok(tagged_values);
        };
        if !is_text_block(&doc.raw) {
            return Err(throw_at(self.tokens(), doc.token, JStr::from(DOC_MULTI_LINE)));
        }
        for tv in &tagged_values {
            if tv.tag.value == DOC_TAG && (tv.tag.profile == DOC_TAG || tv.tag.profile == DOC_PROFILE_PATH) {
                let s = tv
                    .source_information
                    .as_deref()
                    .expect("a parsed node always carries its span");
                return Err(JavaError::parse(
                    JStr::from("Element has both documentation and an explicit doc.doc tagged value. Use one."),
                    s.start_line,
                    s.start_column,
                ));
            }
        }
        let mut out = Vec::with_capacity(tagged_values.len() + 1);
        out.push(Box::new(PTaggedValue {
            tag: Box::new(PTag {
                profile: JStr::from(DOC_PROFILE_PATH),
                value: JStr::from(DOC_TAG),
                profile_source_information: boxed(doc.span.clone()),
                source_information: boxed(doc.span.clone()),
            }),
            value: JStr(canonicalize_documentation(&doc.raw)?),
            multi_line: true,
            source_information: boxed(doc.span),
        }));
        out.extend(tagged_values);
        Ok(out)
    }

    /// `parseDecorations`: `<<p::P.v, ...>>` and/or `{p::P.t = 'v', ...}` before a name -- value-only spans, a
    /// comma required between entries.
    fn parse_decorations(&mut self) -> R<Decorations> {
        let mut stereotypes = Vec::new();
        let mut tagged_values = Vec::new();
        if self.peek() == T::LESS_THAN {
            self.advance()?;
            self.expect(T::LESS_THAN)?;
            while !self.at_end() && self.peek() != T::GREATER_THAN {
                let p_s = self.pos();
                let profile = unquote_path_iae(&self.parse_qualified_name()?)?;
                let p_span = self.span(p_s, self.pos() - 1);
                self.expect(T::DOT)?;
                let v_s = self.pos();
                let value = self.parse_identifier()?;
                stereotypes.push(Box::new(PStereotype {
                    profile: JStr(profile),
                    value: JStr(value),
                    profile_source_information: p_span,
                    source_information: self.span(v_s, v_s),
                }));
                if self.peek() != T::GREATER_THAN {
                    self.expect(T::COMMA)?;
                }
            }
            self.expect(T::GREATER_THAN)?;
            self.expect(T::GREATER_THAN)?;
        }
        if self.peek() == T::BRACE_OPEN {
            self.advance()?;
            while !self.at_end() && self.peek() != T::BRACE_CLOSE {
                let t_s = self.pos();
                let profile = unquote_path_iae(&self.parse_qualified_name()?)?;
                let p_span = self.span(t_s, self.pos() - 1);
                self.expect(T::DOT)?;
                let v_s = self.pos();
                let tag_name = self.parse_identifier()?;
                self.expect(T::EQUAL)?;
                let (value, multi_line, tv_span) = if self.peek() == T::DOC_STRING {
                    if self.dialect().refuses_platform_dialect() && !self.text().contains(&u16::from(b'\n')) {
                        return Err(self.error(jcat!("Unexpected token '", self.text(), "'")));
                    }
                    let d_tok = self.pos();
                    let raw = self.text();
                    let value = doc_string_value(raw, self.error(JStr::default()).position.unwrap_or((0, 0)))?;
                    let multi_line = is_text_block(raw);
                    self.advance()?;
                    let d = self.doc_string_span(d_tok);
                    let s = self.span_of(t_s, t_s);
                    let span = SourceInfo {
                        source_id: s.source_id,
                        start_line: s.start_line,
                        start_column: s.start_column,
                        end_line: d.end_line,
                        end_column: d.end_column,
                    };
                    (value, multi_line, span)
                } else {
                    let quoted = self.slack_text();
                    self.expect(T::STRING)?;
                    (
                        self.unquote_and_unescape(quoted)?,
                        false,
                        self.span_of(t_s, self.pos() - 1),
                    )
                };
                tagged_values.push(Box::new(PTaggedValue {
                    tag: Box::new(PTag {
                        profile: JStr(profile),
                        value: JStr(tag_name),
                        profile_source_information: p_span,
                        source_information: self.span(v_s, v_s),
                    }),
                    value: JStr(value),
                    multi_line,
                    source_information: boxed(tv_span),
                }));
                if self.peek() != T::BRACE_CLOSE {
                    self.expect(T::COMMA)?;
                }
            }
            self.expect(T::BRACE_CLOSE)?;
        }
        Ok(Decorations {
            stereotypes,
            tagged_values,
        })
    }

    /// `withDocumentation(doc, dec)`: the decorations with the documentation folded into their tagged values.
    fn with_documentation(&self, doc: Option<Documentation>, dec: Decorations) -> R<Decorations> {
        if doc.is_none() {
            return Ok(dec);
        }
        let tagged_values = self.tagged_values_with_documentation(doc, dec.tagged_values)?;
        Ok(Decorations {
            stereotypes: dec.stereotypes,
            tagged_values,
        })
    }

    // ---------------------------------------------------------------- spans

    /// `docStringSpan`: a `'''` token's single-line column arithmetic over its raw length.
    fn doc_string_span(&self, tok: usize) -> SourceInfo {
        let ts = self.tokens();
        let raw_len = (ts.end(tok) - ts.start(tok)) as i32;
        let (line, col) = (ts.start_line(tok) as i32, ts.start_column(tok) as i32);
        SourceInfo {
            source_id: self.span_source_id(),
            start_line: line,
            start_column: col,
            end_line: line,
            end_column: col + raw_len - 1,
        }
    }

    /// `spanOf(fromTok, toTok)`: 1-based, end inclusive; a span ending on a path literal ends on its start line, its
    /// column the token's start column plus its raw length (the engine's one-token `NAVIGATION_PATH_BLOCK`). A cursor
    /// over a re-lexed island overrides it ([`IslandCursor`]), reaching this one as [`base_span_of`].
    fn span_of(&self, from: usize, to: usize) -> SourceInfo {
        base_span_of(self, from, to)
    }

    /// The offsets of a runtime island's re-lex cursor (Java's `RuntimeSectionGrammar.SliceCursorOffset`, which
    /// `parseEmbeddedConnection` tests with `instanceof` to compose a nested island's offsets): `None` for any other.
    fn island_offsets(&self) -> Option<(i32, i32)> {
        None
    }

    /// `spanOf` boxed, as a record's span slot holds it.
    fn span(&self, from: usize, to: usize) -> Span {
        boxed(self.span_of(from, to))
    }

    // ---------------------------------------------------------------- the store grammars' shared readers

    /// `isCleanSheetBody`: whether the class-mapping body at the cursor is the clean-sheet function form rather than
    /// the legacy declarative one (a `~directive`, `prop:`, `scope(`, `prop(`, `prop[`, `+local`, or `{}`).
    fn is_clean_sheet_body(&self) -> bool {
        if self.peek() == T::BRACE_CLOSE {
            return false;
        }
        if is_legacy_mapping_command(self.peek()) {
            return false;
        }
        if is_identifier_token(self.peek()) && self.peek_at(1) == T::COLON {
            return false;
        }
        if is_identifier_token(self.peek())
            && crate::ustr::eq(&self.safe_text(), "scope")
            && self.peek_at(1) == T::PAREN_OPEN
        {
            return false;
        }
        if is_identifier_token(self.peek()) && self.peek_at(1) == T::PAREN_OPEN {
            return false;
        }
        if is_identifier_token(self.peek()) && self.peek_at(1) == T::BRACKET_OPEN {
            return false;
        }
        self.peek() != T::PLUS
    }

    /// `parseFunctionDescriptor`: a function pointer's extent -- `fqn`, an optional balanced `(args)`, an optional
    /// `:Type<...>[m]` signature suffix; `name_end` and `end` exclusive.
    fn parse_function_descriptor(&mut self) -> R<FunctionDescriptor> {
        let start = self.pos();
        self.parse_qualified_name()?;
        let name_end = self.pos();
        if self.peek() == T::PAREN_OPEN {
            let mut parens = 0i64;
            loop {
                if self.peek() == T::PAREN_OPEN {
                    parens += 1;
                } else if self.peek() == T::PAREN_CLOSE {
                    parens -= 1;
                }
                self.advance()?;
                if self.at_end() || parens <= 0 {
                    break;
                }
            }
            if self.match_tok(T::COLON)? {
                self.parse_qualified_name()?;
                self.skip_type_args_and_multiplicity()?;
            }
        }
        Ok(FunctionDescriptor {
            start,
            name_end,
            end: self.pos(),
        })
    }

    /// `~func`'s `functionIdentifier` at the engine's dialect: a name and the whole signature.
    fn parse_engine_function_identifier(&mut self) -> R<FunctionDescriptor> {
        let start = self.pos();
        self.parse_qualified_name()?;
        let name_end = self.pos();
        self.engine_function_signature()?;
        Ok(FunctionDescriptor {
            start,
            name_end,
            end: self.pos(),
        })
    }

    /// The rest of the engine's `functionIdentifier` past its name: `( (type multiplicity (, type multiplicity)*)? ) : type
    /// multiplicity`, refused where its grammar stops (the user's ruling 18 of 2026-11-09, docs/rust-port/PHASE2.md section 6).
    fn engine_function_signature(&mut self) -> R<()> {
        if self.peek() != T::PAREN_OPEN {
            return Err(self.unexpected());
        }
        self.advance()?;
        if self.peek() != T::PAREN_CLOSE {
            loop {
                self.engine_typed_slot()?;
                if self.peek() != T::COMMA {
                    break;
                }
                self.advance()?;
            }
        }
        if self.peek() != T::PAREN_CLOSE {
            return Err(self.unexpected());
        }
        self.advance()?;
        if self.peek() != T::COLON {
            return Err(self.unexpected());
        }
        self.advance()?;
        self.engine_typed_slot()
    }

    /// A `type multiplicity` of the engine's grammar: a refusal inside a type's arguments stands at their `<`, where its
    /// parser's prediction gives up on the arguments, a refusal elsewhere at the token.
    fn engine_typed_slot(&mut self) -> R<()> {
        let start = self.pos();
        let arguments = self.first_token_of_type_arguments(start);
        let read = self.parse_type();
        if read.is_err() {
            let at = arguments.filter(|&a| a < self.tokens().count()).unwrap_or(self.pos());
            self.set_pos(at);
            return Err(self.unexpected());
        }
        if self.peek() != T::BRACKET_OPEN {
            return Err(self.unexpected());
        }
        self.advance()?;
        // `identifier | ((fromMultiplicity DOT_DOT)? toMultiplicity)`, whose numbers the engine's walker never reads here
        if is_identifier_token(self.peek()) {
            self.advance()?;
        } else {
            if self.peek() == T::INTEGER && self.peek_at(1) == T::DOT_DOT {
                self.advance()?;
                self.advance()?;
            }
            if !matches!(self.peek(), T::INTEGER | T::STAR) {
                return Err(self.unexpected());
            }
            self.advance()?;
        }
        if self.peek() != T::BRACKET_CLOSE {
            return Err(self.unexpected());
        }
        self.advance()
    }

    /// The `<` of the type arguments of the name at `from`, if it has them.
    fn first_token_of_type_arguments(&self, from: usize) -> Option<usize> {
        let ts = self.tokens();
        let mut i = from;
        while i < ts.count() && (is_fqn_segment_token(ts.ty(i)) || ts.ty(i) == T::PATH_SEPARATOR) {
            i += 1;
        }
        (i < ts.count() && ts.ty(i) == T::LESS_THAN).then_some(i)
    }

    /// `skipTypeArgsAndMultiplicity`: `<...>` type arguments and a `[..]` multiplicity, if present.
    fn skip_type_args_and_multiplicity(&mut self) -> R<()> {
        if self.peek() == T::LESS_THAN {
            let mut depth = 0i64;
            while !self.at_end() {
                if self.peek() == T::LESS_THAN {
                    depth += 1;
                } else if self.peek() == T::GREATER_THAN {
                    depth -= 1;
                    if depth == 0 {
                        self.advance()?;
                        break;
                    }
                }
                self.advance()?;
            }
        }
        if self.peek() == T::BRACKET_OPEN {
            while !self.at_end() && self.peek() != T::BRACKET_CLOSE {
                self.advance()?;
            }
            self.match_tok(T::BRACKET_CLOSE)?;
        }
        Ok(())
    }

    /// `validateJoinType`: a join pointer's `(X)` admits exactly `INNER` or `OUTER`, case-sensitive.
    fn validate_join_type(&self, join_type: &[u16]) -> R<()> {
        if !crate::ustr::eq(join_type, "INNER") && !crate::ustr::eq(join_type, "OUTER") {
            return Err(self.error(jcat!("Unsupported join type '", join_type, "'")));
        }
        Ok(())
    }
}

/// `TokenStreamCursor.spanOf`'s own body (Java's `TokenStreamCursor.super.spanOf(fromTok, toTok)`).
pub fn base_span_of<'s, C: Cursor<'s> + ?Sized>(c: &C, from: usize, to: usize) -> SourceInfo {
    let ts = c.tokens();
    if ts.ty(to) == T::PATH_LITERAL {
        return SourceInfo {
            source_id: c.span_source_id(),
            start_line: ts.start_line(from) as i32,
            start_column: ts.start_column(from) as i32,
            end_line: ts.start_line(to) as i32,
            end_column: (ts.start_column(to) + ts.end(to) - ts.start(to)) as i32 - 1,
        };
    }
    SourceInfo {
        source_id: c.span_source_id(),
        start_line: ts.start_line(from) as i32,
        start_column: ts.start_column(from) as i32,
        end_line: ts.end_line(to) as i32,
        end_column: ts.end_column(to) as i32,
    }
}

/// `SpanOrigin.islandShift(span, lineOffset, colOffset)`: the engine's walker-offset rule for an island re-parse --
/// the line offset on every line, the column offset on line 1 only; the span itself when both are zero.
pub fn island_shift(sp: SourceInfo, line_offset: i32, col_offset: i32) -> SourceInfo {
    if line_offset == 0 && col_offset == 0 {
        return sp;
    }
    SourceInfo {
        source_id: JStr::default(),
        start_line: sp.start_line + line_offset,
        start_column: if sp.start_line == 1 {
            sp.start_column + col_offset
        } else {
            sp.start_column
        },
        end_line: sp.end_line + line_offset,
        end_column: if sp.end_line == 1 {
            sp.end_column + col_offset
        } else {
            sp.end_column
        },
    }
}

/// A cursor over an island's content lexed again on its own (`Lexer.tokenize(islandText)`: its lines and columns the
/// island's, line 1 its first), its spans shifted by the walker offsets ([`island_shift`]) and nothing else -- its
/// errors stay island-relative, as Java's (the Connection grammar's `Cursor`, the Runtime grammar's
/// `SliceCursorOffset`; `runtime` marks the latter, which a nested connection island composes its offsets with).
pub struct IslandCursor<'t, 's> {
    pub tokens: &'t TokenStream<'s>,
    pub pos: usize,
    pub dialect: Dialect,
    pub line_offset: i32,
    pub col_offset: i32,
    pub runtime: bool,
}

impl<'s> Cursor<'s> for IslandCursor<'_, 's> {
    fn tokens(&self) -> &TokenStream<'s> {
        self.tokens
    }
    fn pos(&self) -> usize {
        self.pos
    }
    fn set_pos(&mut self, pos: usize) {
        self.pos = pos;
    }
    fn dialect(&self) -> Dialect {
        self.dialect
    }
    fn span_of(&self, from: usize, to: usize) -> SourceInfo {
        island_shift(base_span_of(self, from, to), self.line_offset, self.col_offset)
    }
    fn island_offsets(&self) -> Option<(i32, i32)> {
        self.runtime.then_some((self.line_offset, self.col_offset))
    }
}

/// `TokenStreamCursor.parseIdentifier`'s own body (Java's `TokenStreamCursor.super.parseIdentifier()`): a single
/// identifier, a quoted one unquoted and unescaped.
pub fn base_parse_identifier<'s, C: Cursor<'s> + ?Sized>(c: &mut C) -> R<Vec<u16>> {
    if c.peek() == T::INTEGER && c.int_leads_identifier() {
        return c.fqn_segment_text();
    }
    if !is_identifier_token(c.peek()) {
        return Err(c.error(jcat!("expected identifier, got ", c.peek())));
    }
    let name = if c.peek() == T::STRING {
        c.unquote_and_unescape(c.text())?
    } else {
        c.text().to_vec()
    };
    c.advance()?;
    Ok(name)
}

/// `TokenStreamCursor.FunctionDescriptor`: a function pointer's extent, `name_end` and `end` exclusive.
#[derive(Clone, Copy, Debug)]
pub struct FunctionDescriptor {
    pub start: usize,
    pub name_end: usize,
    pub end: usize,
}

/// `isLegacyMappingCommand`: the `~directive` keywords that open a legacy mapping body.
pub fn is_legacy_mapping_command(t: TokenType) -> bool {
    matches!(
        t,
        T::MAIN_TABLE_CMD | T::FILTER_CMD | T::DISTINCT_CMD | T::GROUP_BY_CMD | T::PRIMARY_KEY_CMD | T::SRC_CMD
    )
}

/// `TokenStreamCursor.stripDoubleQuotes`: a `"..."` spelling's quotes off, no escapes.
pub fn strip_double_quotes(n: &[u16]) -> Vec<u16> {
    let q = u16::from(b'"');
    if n.len() > 1 && n[0] == q && n[n.len() - 1] == q {
        n[1..n.len() - 1].to_vec()
    } else {
        n.to_vec()
    }
}

/// `tokens.type(i)` where Java reads it unguarded: past `count` the lexer's zeroed slack, ordinal 0, `STRING`.
pub fn ty_or_slack(tokens: &TokenStream<'_>, i: usize) -> TokenType {
    if i < tokens.count() { tokens.ty(i) } else { T::STRING }
}

fn generic(
    name: Vec<u16>,
    arguments: Vec<TypeExpression>,
    multiplicity_arguments: Vec<JStr>,
    type_variable_values: Vec<ValueSpecification>,
    pos: Span,
) -> TypeExpression {
    TypeExpression::TypeExpressionGeneric(Box::new(TypeExpressionGeneric {
        name: JStr(name),
        arguments,
        multiplicity_arguments,
        type_variable_values,
        pos,
    }))
}

fn algebra(left: TypeExpression, op: TypeExpressionOp, right: TypeExpression) -> TypeExpression {
    TypeExpression::TypeExpressionSchemaAlgebra(Box::new(TypeExpressionSchemaAlgebra { left, op, right }))
}

/// `ElementParser` positioned at a token, at the caller's dialect (`ElementParser.at`), for the decoration readers
/// the expression parser borrows. Its spans carry no source id (ElementParser keeps the default).
pub struct ElementCursor<'t, 's> {
    pub(crate) tokens: &'t TokenStream<'s>,
    pub(crate) pos: usize,
    pub(crate) dialect: Dialect,
}

impl<'s> Cursor<'s> for ElementCursor<'_, 's> {
    fn tokens(&self) -> &TokenStream<'s> {
        self.tokens
    }
    fn pos(&self) -> usize {
        self.pos
    }
    fn set_pos(&mut self, pos: usize) {
        self.pos = pos;
    }
    fn dialect(&self) -> Dialect {
        self.dialect
    }
}

impl<'t, 's> ElementCursor<'t, 's> {
    /// `ElementParser.at(tokens, tokenIndex, dialect)`.
    pub fn at(tokens: &'t TokenStream<'s>, pos: usize, dialect: Dialect) -> ElementCursor<'t, 's> {
        ElementCursor { tokens, pos, dialect }
    }

    /// `ElementParser.parseStereotypes`: `<<profile.stereotype, ...>>`, empty when absent.
    pub fn parse_stereotypes(&mut self) -> R<Vec<Box<PStereotype>>> {
        if self.peek() != T::LESS_THAN || self.peek_at(1) != T::LESS_THAN {
            return Ok(Vec::new());
        }
        self.advance()?;
        self.advance()?;
        let mut result = vec![Box::new(self.parse_stereotype()?)];
        while self.match_tok(T::COMMA)? {
            result.push(Box::new(self.parse_stereotype()?));
        }
        self.expect(T::GREATER_THAN)?;
        self.expect(T::GREATER_THAN)?;
        Ok(result)
    }

    fn parse_stereotype(&mut self) -> R<PStereotype> {
        let prof_start = self.pos;
        let profile = self.unquoted_path()?;
        let prof_end = self.pos - 1;
        self.expect(T::DOT)?;
        let value = self.parse_identifier()?;
        Ok(PStereotype {
            profile: JStr(profile),
            value: JStr(value),
            profile_source_information: self.span(prof_start, prof_end),
            source_information: self.span(prof_start, self.pos - 1),
        })
    }

    /// `Protocol.unquotePath(parseQualifiedName())`: a short unicode escape in a quoted segment is PARITY row 9's
    /// refusal.
    fn unquoted_path(&mut self) -> R<Vec<u16>> {
        let raw = self.parse_qualified_name()?;
        unquote_path_iae(&raw)
    }

    /// `ElementParser.parseTaggedValues`: `{ profile.tag = 'value', ... }`, empty when the brace opens none.
    pub fn parse_tagged_values(&mut self) -> R<Vec<Box<PTaggedValue>>> {
        if self.peek() != T::BRACE_OPEN {
            return Ok(Vec::new());
        }
        if !looks_like_tagged_value_block(self.tokens, self.pos) {
            return Ok(Vec::new());
        }
        self.advance()?;
        let mut result = vec![Box::new(self.parse_tagged_value()?)];
        while self.match_tok(T::COMMA)? {
            result.push(Box::new(self.parse_tagged_value()?));
        }
        self.expect(T::BRACE_CLOSE)?;
        Ok(result)
    }

    fn parse_tagged_value(&mut self) -> R<PTaggedValue> {
        let start = self.pos;
        let prof_start = self.pos;
        let profile = JStr(self.unquoted_path()?);
        let prof_end = self.pos - 1;
        self.expect(T::DOT)?;
        let tag_start = self.pos;
        let tag = JStr(self.parse_identifier()?);
        let tag_end = self.pos - 1;
        self.expect(T::EQUAL)?;
        if self.peek() == T::DOC_STRING {
            if self.dialect.refuses_platform_dialect() && !self.text().contains(&u16::from(b'\n')) {
                return Err(self.error(jcat!("Unexpected token '", self.text(), "'")));
            }
            let d_tok = self.pos;
            let raw = self.text();
            let value = doc_string_value(raw, self.error(JStr::default()).position.unwrap_or((0, 0)))?;
            self.advance()?;
            let d = self.doc_string_span(d_tok);
            let s = self.span_of(start, start);
            return Ok(PTaggedValue {
                tag: Box::new(PTag {
                    profile,
                    value: tag,
                    profile_source_information: self.span(prof_start, prof_end),
                    source_information: self.span(tag_start, tag_end),
                }),
                value: JStr(value),
                multi_line: is_text_block(raw),
                source_information: boxed(SourceInfo {
                    source_id: s.source_id,
                    start_line: s.start_line,
                    start_column: s.start_column,
                    end_line: d.end_line,
                    end_column: d.end_column,
                }),
            });
        }
        let raw_value = self.consume(T::STRING)?;
        let mut value = self.unquote_and_unescape(raw_value)?;
        // the platform dialect folds a doc string spelled as 'a' + 'b' + ...
        if !self.dialect.refuses_platform_dialect() {
            while self.peek() == T::PLUS && self.peek_at(1) == T::STRING {
                self.advance()?;
                let next = self.consume(T::STRING)?;
                value.extend(self.unquote_and_unescape(next)?);
            }
        }
        Ok(PTaggedValue {
            tag: Box::new(PTag {
                profile,
                value: tag,
                profile_source_information: self.span(prof_start, prof_end),
                source_information: self.span(tag_start, tag_end),
            }),
            value: JStr(value),
            multi_line: false,
            source_information: self.span(start, self.pos - 1),
        })
    }
}

/// `ElementParser.looksLikeTaggedValueBlock`: `'{' IDENT ('::' IDENT)* '.' IDENT '='`.
pub fn looks_like_tagged_value_block(tokens: &TokenStream<'_>, brace_pos: usize) -> bool {
    let n = tokens.count();
    if brace_pos >= n || tokens.ty(brace_pos) != T::BRACE_OPEN {
        return false;
    }
    let mut p = brace_pos + 1;
    if p >= n || !is_identifier_token(tokens.ty(p)) {
        return false;
    }
    while p < n && is_identifier_token(tokens.ty(p)) {
        p += 1;
        if p >= n {
            return false;
        }
        if tokens.ty(p) == T::PATH_SEPARATOR {
            p += 1;
        } else {
            break;
        }
    }
    if p >= n || tokens.ty(p) != T::DOT {
        return false;
    }
    p += 1;
    if p >= n || !is_identifier_token(tokens.ty(p)) {
        return false;
    }
    p += 1;
    p < n && tokens.ty(p) == T::EQUAL
}

/// `TokenStreamCursor.DOC_PROFILE_PATH` and `DOC_TAG` (`protocol.Documentation`).
pub const DOC_PROFILE_PATH: &str = "meta::pure::profiles::doc";
pub const DOC_TAG: &str = "doc";

/// The engine's refusal of documentation that is not a block literal.
pub const DOC_MULTI_LINE: &str = "Documentation must be written as a multi-line ('''...''') literal";

/// A documentation literal read at declaration position (`TokenStreamCursor.Documentation`): its token, its raw text,
/// its span over the whole literal.
#[derive(Clone, Debug)]
pub struct Documentation {
    pub token: usize,
    pub raw: Vec<u16>,
    pub span: SourceInfo,
}

/// `TokenStreamCursor.Decorations`.
#[derive(Clone, Debug, Default)]
pub struct Decorations {
    pub stereotypes: Vec<Box<PStereotype>>,
    pub tagged_values: Vec<Box<PTaggedValue>>,
}

/// `DECLARATION_HEADS`: the keywords documentation may precede.
pub fn is_declaration_head(t: TokenType) -> bool {
    matches!(
        t,
        T::CLASS | T::ENUM | T::ASSOCIATION | T::FUNCTION | T::NATIVE | T::PROFILE
    )
}

/// `canonicalizeDocumentation`: the text-block layout, no unescape, its leading and trailing blank lines dropped.
pub fn canonicalize_documentation(raw: &[u16]) -> R<Vec<u16>> {
    let layout = text_block_layout(raw)?;
    let nl = u16::from(b'\n');
    let mut start = 0;
    while start < layout.len() && layout[start] == nl {
        start += 1;
    }
    let mut end = layout.len();
    while end > start && layout[end - 1] == nl {
        end -= 1;
    }
    Ok(layout[start..end].to_vec())
}

/// `Protocol.unquotePath`, its short unicode escape PARITY row 9's refusal.
pub fn unquote_path_iae(path: &[u16]) -> R<Vec<u16>> {
    unquote_path(path).map_err(|m| malformed_escape("quoted name", m))
}

/// `TokenStreamCursor.once(seen, key, at)`: a key-dispatch loop's once-only rule, refused at the cursor.
pub fn once<'s>(seen: &mut std::collections::HashSet<Vec<u16>>, key: &[u16], at: &impl Cursor<'s>) -> R<()> {
    if !seen.insert(key.to_vec()) {
        return Err(at.error(jcat!("Field '", key, "' should be specified only once")));
    }
    Ok(())
}

/// `TokenStreamCursor.once(seen, key, at, anchorTok)`: refused at the containing block's token.
pub fn once_at(
    seen: &mut std::collections::HashSet<Vec<u16>>,
    key: &[u16],
    tokens: &TokenStream<'_>,
    anchor_tok: usize,
) -> R<()> {
    if !seen.insert(key.to_vec()) {
        return Err(throw_at(
            tokens,
            anchor_tok,
            jcat!("Field '", key, "' should be specified only once"),
        ));
    }
    Ok(())
}
