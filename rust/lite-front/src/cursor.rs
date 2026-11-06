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

/// `TokenStreamCursor.throwAt(tokens, tokenPos, message)`: a `ParseException` at the token's start, at the end of the
/// last token past the stream's end, unplaced on an empty stream. Lines and columns 1-based, the column in code points
/// (`TokenStream.columnOf`).
pub fn throw_at(tokens: &TokenStream<'_>, token_pos: usize, message: JStr) -> JavaError {
    let n = tokens.count();
    let char_pos = if token_pos < n {
        tokens.start(token_pos)
    } else if n > 0 {
        tokens.end(n - 1)
    } else {
        return JavaError::parse_unplaced(message);
    };
    JavaError::parse(message, tokens.line_of(char_pos) as i32, tokens.column_of(char_pos) as i32)
}

/// `TokenStreamCursor.docStringValue`: the `'''` literal's text-block layout, then Java-unescaped (an
/// `IllegalArgumentException` for a short unicode escape, uncaught by its callers).
pub fn doc_string_value(raw: &[u16]) -> R<Vec<u16>> {
    let layout = text_block_layout(raw)?;
    unescape_java_like(&layout).map_err(|m| JavaError::java("java.lang.IllegalArgumentException", JStr::from(m)))
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
    /// The `sourceId` stamped on spans: "" but for mapping test-suite query lambdas.
    fn span_source_id(&self) -> JStr {
        JStr::default()
    }

    // ---------------------------------------------------------------- lexical primitives

    /// The type under the cursor; `EOF` past the end.
    fn peek(&self) -> TokenType {
        let p = self.pos();
        if p < self.tokens().count() { self.tokens().ty(p) } else { T::EOF }
    }

    /// The type `offset` past the cursor; `EOF` past the end.
    fn peek_at(&self, offset: usize) -> TokenType {
        let i = self.pos() + offset;
        if i < self.tokens().count() { self.tokens().ty(i) } else { T::EOF }
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
        if self.pos() < self.tokens().count() { self.text().to_vec() } else { crate::ustr::u("<EOF>") }
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

    /// `expect(type)`: the generic `expected X but found Y ('text')` refusal.
    fn expect(&mut self, t: TokenType) -> R<()> {
        if self.peek() != t {
            self.reject_invalid()?;
            return Err(self.error(jcat!("expected ", t, " but found ", self.peek(), " ('", self.safe_text(), "')")));
        }
        self.advance()
    }

    /// `expect(type, customMessage)`.
    fn expect_msg(&mut self, t: TokenType, message: &str) -> R<()> {
        if self.peek() != t {
            return Err(self.error(JStr::from(message)));
        }
        self.advance()
    }

    /// `expect(type, customMessage)` with a composed message, built only on failure (it has no side effect).
    fn expect_with(&mut self, t: TokenType, message: impl FnOnce() -> JStr) -> R<()> {
        if self.peek() != t {
            return Err(self.error(message()));
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
            return Err(self.error(jcat!("expected ", t, " but found ", self.peek(), " ('", self.safe_text(), "')")));
        }
        let text = self.text();
        self.advance()?;
        Ok(text)
    }

    /// `error(message)`: a `ParseException` at the cursor.
    fn error(&self, message: JStr) -> JavaError {
        throw_at(self.tokens(), self.pos(), message)
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

    /// `parseIdentifier`: a single identifier, a quoted one unquoted and unescaped.
    fn parse_identifier(&mut self) -> R<Vec<u16>> {
        if self.peek() == T::INTEGER && self.int_leads_identifier() {
            return self.fqn_segment_text();
        }
        if !is_identifier_token(self.peek()) {
            return Err(self.error(jcat!("expected identifier, got ", self.peek())));
        }
        let name =
            if self.peek() == T::STRING { self.unquote_and_unescape(self.text())? } else { self.text().to_vec() };
        self.advance()?;
        Ok(name)
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
        }
        if self.peek() == T::PAREN_OPEN {
            let tvv = self.parse_type_variable_values()?;
            return Ok(generic(name, Vec::new(), Vec::new(), tvv, self.span(start, self.pos() - 1)));
        }
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
                continue;
            }
            let p = self.pos();
            if self.peek() == T::INTEGER {
                let value =
                    java_parse_long(self.text()).map_err(|m| JavaError::java("java.lang.NumberFormatException", m))?;
                tvv.push(ValueSpecification::CInteger(Box::new(CInteger {
                    value: JValue::Long(value),
                    pos: self.span(p, p),
                })));
            } else if self.peek() == T::STRING {
                let v = self.unquote_and_unescape(self.text())?;
                tvv.push(cstring(JStr(v), self.span(p, p)));
            } else {
                return Err(
                    self.error(jcat!("type variable values support integer and string literals, got ", self.peek()))
                );
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
        Err(self.error(jcat!("expected a multiplicity argument (n, n..m, *, or a multiplicity parameter), got ", t)))
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
        Ok(TypeExpression::TypeExpressionFunctionType(Box::new(TypeExpressionFunctionType {
            parameters: params,
            result: Box::new(TypeExpressionTypedParameter { r#type: result_type, multiplicity: result_mult }),
        })))
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
        Ok(TypeExpression::TypeExpressionRelationType(Box::new(TypeExpressionRelationType { columns })))
    }

    /// One relation-type column (`TokenStreamCursor.parseRelationColumn`): its name or `?`, its type or `?`, its
    /// multiplicity (`[0..1]` undeclared), spanning name through the type and a declared multiplicity.
    fn parse_type_relation_column(&mut self) -> R<TypeExpressionColumn> {
        let name_tok = self.pos();
        let col_name = if self.match_tok(T::QUESTION)? { crate::ustr::u("?") } else { self.parse_identifier()? };
        self.expect(T::COLON)?;
        let q_tok = self.pos();
        let col_type = if self.match_tok(T::QUESTION)? {
            name_ref(JStr::from("?"), self.span(q_tok, q_tok))
        } else {
            self.parse_type()?
        };
        let multiplicity =
            if self.peek() == T::BRACKET_OPEN { self.parse_multiplicity()? } else { concrete(0, Some(1)) };
        let pos = self.span(name_tok, self.pos() - 1);
        Ok(TypeExpressionColumn { name: JStr(col_name), r#type: col_type, multiplicity, pos })
    }

    /// `Type[mult]` in a function-type signature.
    fn parse_typed_parameter(&mut self) -> R<TypeExpressionTypedParameter> {
        let t = self.parse_type()?;
        let multiplicity = self.parse_multiplicity()?;
        Ok(TypeExpressionTypedParameter { r#type: t, multiplicity })
    }

    // ---------------------------------------------------------------- the element grammars' helpers

    /// `peekText(offset)`: the text `offset` past the cursor, `""` past the end (an empty string matches no keyword).
    fn peek_text(&self, offset: usize) -> &'s [u16] {
        let i = self.pos() + offset;
        if i < self.tokens().count() { self.tokens().text(i) } else { &[] }
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
        Ok(Some(Documentation { token: tok, raw: raw.to_vec(), span: self.documentation_span(tok, raw) }))
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
                let s = tv.source_information.as_deref().expect("a parsed node always carries its span");
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
                    let value = doc_string_value(raw)?;
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
                    (self.unquote_and_unescape(quoted)?, false, self.span_of(t_s, self.pos() - 1))
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
        Ok(Decorations { stereotypes, tagged_values })
    }

    /// `withDocumentation(doc, dec)`: the decorations with the documentation folded into their tagged values.
    fn with_documentation(&self, doc: Option<Documentation>, dec: Decorations) -> R<Decorations> {
        if doc.is_none() {
            return Ok(dec);
        }
        let tagged_values = self.tagged_values_with_documentation(doc, dec.tagged_values)?;
        Ok(Decorations { stereotypes: dec.stereotypes, tagged_values })
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
    /// column the token's start column plus its raw length (the engine's one-token `NAVIGATION_PATH_BLOCK`).
    fn span_of(&self, from: usize, to: usize) -> SourceInfo {
        let ts = self.tokens();
        if ts.ty(to) == T::PATH_LITERAL {
            return SourceInfo {
                source_id: self.span_source_id(),
                start_line: ts.start_line(from) as i32,
                start_column: ts.start_column(from) as i32,
                end_line: ts.start_line(to) as i32,
                end_column: (ts.start_column(to) + ts.end(to) - ts.start(to)) as i32 - 1,
            };
        }
        SourceInfo {
            source_id: self.span_source_id(),
            start_line: ts.start_line(from) as i32,
            start_column: ts.start_column(from) as i32,
            end_line: ts.end_line(to) as i32,
            end_column: ts.end_column(to) as i32,
        }
    }

    /// `spanOf` boxed, as a record's span slot holds it.
    fn span(&self, from: usize, to: usize) -> Span {
        boxed(self.span_of(from, to))
    }
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

    /// `Protocol.unquotePath(parseQualifiedName())`: a short unicode escape in a quoted segment is Java's uncaught
    /// `IllegalArgumentException`.
    fn unquoted_path(&mut self) -> R<Vec<u16>> {
        let raw = self.parse_qualified_name()?;
        unquote_path(&raw).map_err(|m| JavaError::java("java.lang.IllegalArgumentException", JStr::from(m)))
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
            let value = doc_string_value(raw)?;
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
    matches!(t, T::CLASS | T::ENUM | T::ASSOCIATION | T::FUNCTION | T::NATIVE | T::PROFILE)
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

/// `Protocol.unquotePath`, its short-unicode-escape failure Java's uncaught `IllegalArgumentException`.
pub fn unquote_path_iae(path: &[u16]) -> R<Vec<u16>> {
    unquote_path(path).map_err(|m| JavaError::java("java.lang.IllegalArgumentException", JStr::from(m)))
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
        return Err(throw_at(tokens, anchor_tok, jcat!("Field '", key, "' should be specified only once")));
    }
    Ok(())
}
