// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! The expression parser (core/'s `com.legend.parser.SpecParser`, with what it reaches: `NumberLiterals`,
//! `IslandScan`, `QuotedSpecParser`'s folds, `protocol.CaretSpecials`, `TdsLiteral.of`, `AppliedFunction
//! .tableReference`), held to the Java parser's every answer by `front_parser_differential_test`
//! (docs/rust-port/PHASE2.md §4 step 3).
//!
//! Its levels are Java's: [`SpecParser::parse_combined_expression`] (the engine's combined loop over boolean and
//! arithmetic parts, folded by [`crate::operator_parts`] with its accumulator-threading quirks), then
//! [`SpecParser::parse_expression`] (the postfix loop and one trailing `==`/`!=`), then
//! [`SpecParser::parse_unary_and_primary`]. Each error is the exception Java throws, by class and message, a
//! `ParseException` at Java's position; the two places Java recovers from one (`tryTypedColSpec`, the quoted tree's
//! `parseTree`) recover from a `ParseException` alone, as Java's `catch (ParseException e)` does.
//!
//! The quoted grammar (`compileLegendGrammar('...')`) folds its payload with the model parser
//! (`QuotedSpecParser.foldGrammar` re-enters `PmcdParser`, here [`crate::model_parser::parse`]).

// The generated records hold their record-typed list elements boxed (`Vec<Box<T>>`), so the readers build that.
#![allow(clippy::vec_box)]

use crate::cursor::{Cursor, ElementCursor, R, is_fqn_segment_token, is_identifier_token, throw_at};
use crate::doors::{Answer, Dialect, JavaError};
use crate::escapes::unquote_path;
use crate::jcat;
use crate::jnum::{parse_double, parse_long};
use crate::jtypes::{BigDecimal, JStr, JValue};
use crate::lexer::{TokenStream, TokenType, tokenize};
use crate::nodes::{
    Span, applied, applied_full, boxed, collection, concrete, cstring, infix, lambda, name_ref, property, ptr,
    variable, vs_fn, vs_var,
};
use crate::operator_parts::{ArithPart, BoolPart, arithmetic_part, boolean_part};
use crate::records::{
    AppliedFunction, CBoolean, CDate, CDecimal, CFloat, CInteger, CLatestDate, CString, CTime, ColSpec, ColSpecArray,
    ColumnInstance, Element, EnumValue, GqlIsland, GraphFetchLiteral, GraphFetchLiteralNode,
    GraphFetchLiteralSubTypeNode, KeyExpression, LambdaFunction, Multiplicity, NewInstance, NewInstanceCast,
    NewInstanceKeyBinding, PStereotype, PTaggedValue, PathLiteral, PathLiteralPathArg, PathLiteralPathArgCollectionArg,
    PathLiteralPathArgDateArg, PathLiteralPathArgEnumArg, PathLiteralPathArgIntArg, PathLiteralPathArgLatest,
    PathLiteralPathArgStrArg, PathLiteralSegment, QuotedGrammarCall, QuotedTreeCall, SourceInfo, SqlIsland, TdsLiteral,
    TypeAnnotation, TypeAnnotationMultiplicityRef, TypeAnnotationNamed, TypeAnnotationRelationShape,
    TypeAnnotationRelationShapeColumn, TypeAnnotationWildcard, TypeExpression, TypeExpressionGeneric,
    ValueSpecification, Variable,
};
use crate::temporal::{TemporalError, parse_date, parse_time};
use crate::ustr::{
    self, ends_with, eq, index_of, index_of_units, last_index_of, last_index_of_str, starts_with, strip, trim, u,
};
use lite_javacompat::character::{is_digit, is_letter, is_letter_or_digit, is_whitespace};
use std::collections::HashMap;

use TokenType as T;
use ValueSpecification as VS;

/// `SpecParser.parse(source, dialect)`: lexed, one program line, the whole input consumed.
pub fn parse(text: &[u16], dialect: Dialect) -> Answer<ValueSpecification> {
    let tokens = tokenize(text);
    parse_tokens(&tokens, dialect)
}

/// `SpecParser.parse(tokens, dialect)`: a stream (or a slice of a larger file's) as one value specification.
pub fn parse_tokens(tokens: &TokenStream<'_>, dialect: Dialect) -> Answer<ValueSpecification> {
    SpecParser::new(tokens, JStr::default(), dialect, false).whole_line()
}

/// `SpecParser.parseLambda(source)`: at the engine's dialect; a text that is not a lambda becomes a parameterless one
/// spanning all its tokens.
pub fn parse_lambda(text: &[u16]) -> Answer<LambdaFunction> {
    let tokens = tokenize(text);
    let mut parser = SpecParser::new(&tokens, JStr::default(), Dialect::LegendEngine, false);
    match parser.whole_line()? {
        VS::LambdaFunction(lf) => Ok(*lf),
        v => Ok(lambda(Vec::new(), vec![v], parser.span(0, parser.pos - 1))),
    }
}

/// `SpecParser.parseCodeBlock(source, dialect)`: a `;`-separated block, the whole input consumed.
pub fn parse_code_block(text: &[u16], dialect: Dialect) -> Answer<Vec<ValueSpecification>> {
    let tokens = tokenize(text);
    parse_code_block_tokens(&tokens, JStr::default(), dialect)
}

/// `SpecParser.parseCodeBlock(tokens, spanSourceId, dialect)`.
pub fn parse_code_block_tokens(
    tokens: &TokenStream<'_>,
    span_source_id: JStr,
    dialect: Dialect,
) -> Answer<Vec<ValueSpecification>> {
    code_block(SpecParser::new(tokens, span_source_id, dialect, false))
}

/// `SpecParser.parseInstanceData`: a model store's instances, read as legend-engine's model-data walker reads them.
pub fn parse_instance_data(
    tokens: &TokenStream<'_>,
    span_source_id: JStr,
    dialect: Dialect,
) -> Answer<Vec<ValueSpecification>> {
    code_block(SpecParser::new(tokens, span_source_id, dialect, true))
}

fn code_block(mut parser: SpecParser<'_, '_>) -> Answer<Vec<ValueSpecification>> {
    let stmts = parser.parse_code_block_until(None)?;
    if !parser.at_end() {
        return Err(parser.error(jcat!(
            "trailing tokens after code block: ",
            parser.peek(),
            " ('",
            parser.safe_text(),
            "')"
        )));
    }
    Ok(stmts)
}

/// The parser's state: its cursor, its dialect level, the let-bound string constants in scope (innermost last; a
/// `None` value is Java's `SHADOW`, a name bound to no constant), and the statement owing its sequence a `;`.
pub struct SpecParser<'t, 's> {
    tokens: &'t TokenStream<'s>,
    pos: usize,
    dialect: Dialect,
    span_source_id: JStr,
    instance_data: bool,
    const_strings: Vec<HashMap<Vec<u16>, Option<Vec<u16>>>>,
    owing_statement: Option<usize>,
}

impl<'s> Cursor<'s> for SpecParser<'_, 's> {
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
    fn span_source_id(&self) -> JStr {
        self.span_source_id.clone()
    }
}

/// What `IslandScan.scanScalar` read.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum ScalarKind {
    Skip,
    Latest,
    Date,
    Int,
    Numericish,
    String,
    Bool,
    Var,
    Enum,
    Other,
}

/// `IslandScan.Scalar`: the kind's strings (`a`, `b`), an integer's value, the inclusive span `s..=e` and where the
/// caller resumes.
struct Scalar {
    kind: ScalarKind,
    a: Vec<u16>,
    b: Vec<u16>,
    int_value: i64,
    s: isize,
    e: isize,
    next: isize,
}

impl Scalar {
    fn of(kind: ScalarKind, a: &[u16], b: &[u16], int_value: i64, s: isize, e: isize, next: isize) -> Scalar {
        Scalar { kind, a: a.to_vec(), b: b.to_vec(), int_value, s, e, next }
    }
}

/// One of the combined loop's two threaded results: still the loop's first expression, or a part's node.
enum Acc {
    Expr,
    Node(ValueSpecification),
}

fn is_arithmetic_op(t: TokenType) -> bool {
    matches!(
        t,
        T::PLUS
            | T::MINUS
            | T::STAR
            | T::DIVIDE
            | T::LESS_THAN
            | T::LESS_OR_EQUAL
            | T::GREATER_THAN
            | T::GREATER_OR_EQUAL
    )
}

fn opens_milestoning_literal(t: TokenType) -> bool {
    t == T::LATEST_DATE || t == T::DATE || t == T::DOLLAR
}

fn is_lambda_body_terminator(t: TokenType) -> bool {
    t == T::PAREN_CLOSE || t == T::COMMA || t == T::BRACKET_CLOSE || t == T::BRACE_CLOSE
}

fn ch(b: u8) -> u16 {
    u16::from(b)
}

fn is_quoted(raw: &[u16]) -> bool {
    raw.len() >= 2 && raw[0] == ch(b'\'') && raw[raw.len() - 1] == ch(b'\'')
}

fn iae(m: &'static str) -> JavaError {
    JavaError::java("java.lang.IllegalArgumentException", JStr::from(m))
}

/// `Protocol.unquotePath`, its short unicode escape Java's uncaught `IllegalArgumentException`.
fn unquoted(path: &[u16]) -> R<JStr> {
    unquote_path(path).map(JStr).map_err(iae)
}

impl<'t, 's> SpecParser<'t, 's> {
    fn new(tokens: &'t TokenStream<'s>, span_source_id: JStr, dialect: Dialect, instance_data: bool) -> Self {
        SpecParser {
            tokens,
            pos: 0,
            dialect,
            span_source_id,
            instance_data,
            const_strings: vec![HashMap::new()],
            owing_statement: None,
        }
    }

    /// `constString(name)`: the innermost scope's binding wins; a shadow binds no constant.
    fn const_string(&self, name: &[u16]) -> Option<Vec<u16>> {
        for scope in self.const_strings.iter().rev() {
            if let Some(v) = scope.get(name) {
                return v.clone();
            }
        }
        None
    }

    fn push_const_scope(&mut self, params: &[Variable]) {
        let mut scope = HashMap::new();
        for p in params {
            scope.insert(p.name.0.clone(), None);
        }
        self.const_strings.push(scope);
    }

    fn whole_line(&mut self) -> R<ValueSpecification> {
        let result = self.parse_program_line()?;
        if !self.at_end() {
            return Err(self.error(jcat!(
                "trailing tokens after expression: ",
                self.peek(),
                " ('",
                self.safe_text(),
                "')"
            )));
        }
        Ok(result)
    }

    // -------------------------------------------------------------------- statements

    /// One statement: a `let` (only here) or a combined expression.
    fn parse_program_line(&mut self) -> R<ValueSpecification> {
        if !self.at_end() && self.peek() == T::LET {
            return self.parse_let_expression().map(vs_fn);
        }
        self.parse_combined_expression()
    }

    /// `let name = value` as `letFunction(CString(name), value)`, spanning the statement; a value that folds to a
    /// string constant recorded in the innermost scope.
    fn parse_let_expression(&mut self) -> R<AppliedFunction> {
        let let_tok = self.pos;
        self.pos += 1;
        let var_name: Vec<u16> = if !self.at_end() && self.peek() == T::STRING {
            self.parse_string()?.value.0
        } else if !self.at_end() && is_fqn_segment_token(self.peek()) {
            let n = self.text().to_vec();
            self.pos += 1;
            n
        } else {
            return Err(self.error_str("expected variable name after 'let'"));
        };
        self.expect_with(T::EQUAL, || jcat!("expected '=' after 'let ", var_name, "'"))?;
        let value = self.parse_combined_expression()?;
        let constant = fold_string_concat(&value, &|n: &[u16]| self.const_string(n));
        self.const_strings.last_mut().expect("a scope is always open").insert(var_name.clone(), constant);
        let mut let_span = self.span_of(let_tok, self.pos - 1);
        if self.tokens.ty(self.pos - 1) == T::DOC_STRING {
            let d = self.doc_string_span(self.pos - 1);
            let_span.end_line = d.end_line;
            let_span.end_column = d.end_column;
        }
        Ok(applied(JStr::from("letFunction"), vec![cstring(JStr(var_name), None), value], boxed(let_span)))
    }

    /// `parseCodeBlockUntil(terminator)`: statements to the terminator (not consumed) or the end; a lone statement's
    /// `;` optional, a multi-statement body's every one required.
    fn parse_code_block_until(&mut self, terminator: Option<TokenType>) -> R<Vec<ValueSpecification>> {
        let mut stmts = Vec::new();
        if self.at_terminator(terminator) {
            return Ok(stmts);
        }
        let mut last_terminated = false;
        let enclosing = self.owing_statement;
        self.owing_statement = None; // the first statement's ';' is optional
        let r = self.code_block_statements(terminator, &mut stmts, &mut last_terminated);
        self.owing_statement = enclosing;
        r?;
        if stmts.len() > 1 && !last_terminated && self.at_terminator(terminator) {
            return Err(self.error_str("Unexpected token"));
        }
        Ok(stmts)
    }

    fn code_block_statements(
        &mut self,
        terminator: Option<TokenType>,
        stmts: &mut Vec<ValueSpecification>,
        last_terminated: &mut bool,
    ) -> R<()> {
        stmts.push(self.parse_program_line()?);
        while !self.at_end() && self.peek() == T::SEMI_COLON {
            self.pos += 1;
            *last_terminated = true;
            if self.at_terminator(terminator) {
                break;
            }
            self.owing_statement = Some(self.pos); // a later statement: its ';' is required
            stmts.push(self.parse_program_line()?);
            *last_terminated = false;
        }
        Ok(())
    }

    fn at_terminator(&self, terminator: Option<TokenType>) -> bool {
        if self.at_end() {
            return true;
        }
        terminator.is_some_and(|t| self.peek() == t)
    }

    // -------------------------------------------------------------------- operators

    /// The engine's `combinedExpression` loop.
    fn parse_combined_expression(&mut self) -> R<ValueSpecification> {
        let e = self.parse_expression()?;
        self.parse_combined_parts(e)
    }

    /// The combined expression's parts after its first expression: same-kind parts accumulate and flush when the
    /// other kind (or the end) arrives, each group folding over the OTHER kind's latest result.
    fn parse_combined_parts(&mut self, expr: ValueSpecification) -> R<ValueSpecification> {
        let mut first = Some(expr);
        let mut arth: Vec<ArithPart> = Vec::new();
        let mut bools: Vec<BoolPart> = Vec::new();
        let mut bool_result = Acc::Expr;
        let mut arith_result = Acc::Expr;
        let take = |acc: Acc, first: &mut Option<ValueSpecification>| match acc {
            Acc::Expr => first.take().expect("the first expression is folded once"),
            Acc::Node(v) => v,
        };
        while !self.at_end() {
            let t = self.peek();
            if is_arithmetic_op(t) {
                if !bools.is_empty() {
                    let input = take(std::mem::replace(&mut arith_result, Acc::Expr), &mut first);
                    bool_result = Acc::Node(vs_fn(boolean_part(std::mem::take(&mut bools), input)));
                }
                arth.push(self.parse_arithmetic_part_context()?);
            } else if t == T::AND || t == T::OR {
                if !arth.is_empty() {
                    let input = take(std::mem::replace(&mut bool_result, Acc::Expr), &mut first);
                    arith_result = Acc::Node(vs_fn(arithmetic_part(std::mem::take(&mut arth), input)));
                }
                bools.push(self.parse_boolean_part_context()?);
            } else {
                break;
            }
        }
        if !arth.is_empty() {
            return Ok(vs_fn(arithmetic_part(arth, take(bool_result, &mut first))));
        }
        if !bools.is_empty() {
            return Ok(vs_fn(boolean_part(bools, take(arith_result, &mut first))));
        }
        Ok(first.expect("no part folded the first expression"))
    }

    /// The postfix loop (`.`, `->`, `[`), then one `==`/`!=`/`<>`.
    fn parse_expression(&mut self) -> R<ValueSpecification> {
        let mut expr = self.parse_unary_and_primary()?;
        while !self.at_end() {
            match self.peek() {
                T::DOT => expr = self.parse_dot_postfix(expr)?,
                T::ARROW => expr = vs_fn(self.parse_arrow_postfix(expr)?),
                T::BRACKET_OPEN => expr = self.parse_bracket_postfix(expr)?,
                _ => break,
            }
        }
        if !self.at_end() {
            let t = self.peek();
            if t == T::NOT_EQUAL && self.dialect.refuses_platform_dialect() {
                return Err(self.error_str("Unexpected token '<>'"));
            }
            if t == T::TEST_EQUAL || t == T::TEST_NOT_EQUAL || t == T::NOT_EQUAL {
                let op_tok = self.pos;
                self.pos += 1;
                let right = self.parse_combined_arithmetic_only()?;
                let eq = infix(JStr::from("equal"), vec![expr, right], self.span(op_tok, op_tok));
                expr = if t == T::TEST_EQUAL {
                    vs_fn(eq)
                } else {
                    vs_fn(infix(JStr::from("not"), vec![vs_fn(eq)], self.span(op_tok, op_tok)))
                };
            }
        }
        Ok(expr)
    }

    /// One unary prefix (`!`, `-`, `+`) over a postfix-aware operand, or a primary.
    fn parse_unary_and_primary(&mut self) -> R<ValueSpecification> {
        if !self.at_end() {
            let t = self.peek();
            if t == T::NOT {
                let not_tok = self.pos;
                self.pos += 1;
                let operand = self.parse_expression()?;
                return Ok(vs_fn(applied(JStr::from("not"), vec![operand], self.span(not_tok, self.pos - 1))));
            }
            if t == T::MINUS {
                let op_tok = self.pos;
                self.pos += 1;
                if self.instance_data && self.peek_is_number() {
                    let n = self.parse_number()?;
                    return Ok(signed_literal(n, true, self.span(op_tok, self.pos - 1)));
                }
                // -5 RomanLength~Pes: the sign belongs to the unit literal's number
                if self.peek_is_number() && self.unit_path_end(self.pos + 1).is_some() {
                    let n = self.parse_number()?;
                    return self.unit_literal(vs_fn(infix(JStr::from("minus"), vec![n], self.span(op_tok, op_tok))));
                }
                let operand = self.parse_expression()?;
                return Ok(vs_fn(infix(JStr::from("minus"), vec![operand], self.span(op_tok, op_tok))));
            }
            if t == T::PLUS {
                let op_tok = self.pos;
                self.pos += 1;
                if self.instance_data && self.peek_is_number() {
                    let n = self.parse_number()?;
                    return Ok(signed_literal(n, false, self.span(op_tok, self.pos - 1)));
                }
                let operand = self.parse_expression()?;
                return Ok(vs_fn(infix(JStr::from("plus"), vec![operand], self.span(op_tok, op_tok))));
            }
        }
        self.parse_primary()
    }

    fn peek_is_number(&self) -> bool {
        matches!(self.peek(), T::INTEGER | T::FLOAT | T::DECIMAL)
    }

    /// The number at the cursor: an integer, a float or a decimal.
    fn parse_number(&mut self) -> R<ValueSpecification> {
        match self.peek() {
            T::INTEGER => self.parse_integer().map(|c| VS::CInteger(Box::new(c))),
            T::FLOAT => self.parse_float(),
            _ => self.parse_decimal().map(|c| VS::CDecimal(Box::new(c))),
        }
    }

    /// `combinedArithmeticOnly`: an `==`/`!=` right side, no boolean parts.
    fn parse_combined_arithmetic_only(&mut self) -> R<ValueSpecification> {
        let expr = self.parse_expression()?;
        let mut parts = Vec::new();
        while !self.at_end() && is_arithmetic_op(self.peek()) {
            parts.push(self.parse_arithmetic_part_context()?);
        }
        Ok(if parts.is_empty() { expr } else { vs_fn(arithmetic_part(parts, expr)) })
    }

    fn parse_arithmetic_part_context(&mut self) -> R<ArithPart> {
        let op = self.peek();
        let first_op = self.pos;
        let function = match op {
            T::PLUS => "plus",
            T::MINUS => "minus",
            T::STAR => "times",
            T::DIVIDE => "divide",
            T::LESS_THAN => "lessThan",
            T::LESS_OR_EQUAL => "lessThanEqual",
            T::GREATER_THAN => "greaterThan",
            T::GREATER_OR_EQUAL => "greaterThanEqual",
            _ => {
                return Err(JavaError::java(
                    "java.lang.IllegalStateException",
                    jcat!("parseArithmeticPartContext on non-arithmetic token: ", op),
                ));
            }
        };
        if matches!(op, T::LESS_THAN | T::LESS_OR_EQUAL | T::GREATER_THAN | T::GREATER_OR_EQUAL) {
            self.pos += 1;
            let rhs = self.parse_expression()?;
            return Ok(ArithPart {
                function,
                comparison: true,
                operands: vec![rhs],
                ctx_span: self.span(first_op, self.pos - 1),
            });
        }
        // a same-op run: greedy, one context
        let mut operands = Vec::new();
        while !self.at_end() && self.peek() == op {
            self.pos += 1;
            operands.push(self.parse_expression()?);
        }
        Ok(ArithPart { function, comparison: false, operands, ctx_span: self.span(first_op, self.pos - 1) })
    }

    fn parse_boolean_part_context(&mut self) -> R<BoolPart> {
        let function = if self.peek() == T::AND { "and" } else { "or" };
        let op_tok = self.pos;
        self.pos += 1;
        let operand = self.parse_expression()?;
        Ok(BoolPart { function, operand, op_span: self.span(op_tok, op_tok) })
    }

    // -------------------------------------------------------------------- primaries

    fn parse_primary(&mut self) -> R<ValueSpecification> {
        if self.at_end() {
            return Err(self.error_str("expected expression, got end of input"));
        }
        let t = self.peek();
        match t {
            T::INTEGER => {
                let n = self.parse_integer()?;
                self.unit_literal(VS::CInteger(Box::new(n)))
            }
            T::FLOAT => {
                let n = self.parse_float()?;
                self.unit_literal(n)
            }
            T::DECIMAL => {
                let n = self.parse_decimal()?;
                self.unit_literal(VS::CDecimal(Box::new(n)))
            }
            // a quoted name starts an expression when a call or path follows ('abs'($x), 'pkg'::f)
            T::STRING => {
                if self.peek_at(1) == T::PAREN_OPEN || self.peek_at(1) == T::PATH_SEPARATOR {
                    self.parse_qualified_name_start()
                } else {
                    self.parse_string().map(|c| VS::CString(Box::new(c)))
                }
            }
            T::DOC_STRING => self.parse_doc_string(),
            T::TRUE => Ok(self.consume_boolean(true)),
            T::FALSE => Ok(self.consume_boolean(false)),
            T::DATE => self.parse_date_or_date_time(),
            T::STRICTTIME => self.parse_strict_time(),
            T::LATEST_DATE => Ok(self.parse_latest_date()),
            T::DOLLAR => self.parse_variable().map(vs_var),
            T::BRACKET_OPEN => self.parse_collection(),
            T::PAREN_OPEN => self.parse_parenthesised(),
            T::NEW_SYMBOL => self.parse_new_instance(),
            T::BRACE_OPEN => self.parse_lambda_function().map(|l| VS::LambdaFunction(Box::new(l))),
            T::PIPE => self.parse_lambda_pipe().map(|l| VS::LambdaFunction(Box::new(l))),
            T::TILDE => self.parse_column_builders().map(VS::ColumnInstance),
            // a fused mapping command token in expression position is an ordinary colspec of its name
            T::DISTINCT_CMD | T::FILTER_CMD | T::GROUP_BY_CMD | T::MAIN_TABLE_CMD | T::PRIMARY_KEY_CMD | T::SRC_CMD => {
                self.parse_tilde_command_col_spec().map(|c| VS::ColumnInstance(ColumnInstance::ColSpec(Box::new(c))))
            }
            T::AT => self.parse_type_annotation().map(VS::TypeAnnotation),
            T::TDS_LITERAL => self.parse_tds_literal(),
            T::PATH_LITERAL => self.parse_path_literal(),
            T::ISLAND_OPEN => self.parse_dsl(),
            _ => {
                if is_identifier_token(t) {
                    // single-param shorthand 'x | body' (never across '::')
                    if self.pos + 1 < self.tokens.count() && self.tokens.ty(self.pos + 1) == T::PIPE {
                        return self.parse_single_param_lambda().map(|l| VS::LambdaFunction(Box::new(l)));
                    }
                    // typed 'x: Type[mult] | body', committed only when the whole pattern is there
                    if self.pos + 1 < self.tokens.count()
                        && self.tokens.ty(self.pos + 1) == T::COLON
                        && self.looks_like_typed_lambda_param()?
                    {
                        return self.parse_single_param_lambda().map(|l| VS::LambdaFunction(Box::new(l)));
                    }
                    return self.parse_qualified_name_start();
                }
                // a bare '::' in value position: the root package; as a caller ('::.all()') the engine's refusal
                if t == T::PATH_SEPARATOR {
                    self.advance()?;
                    if !self.at_end() && self.peek() == T::DOT {
                        return Err(self.error_str("Expected a non-empty function caller"));
                    }
                    return Ok(ptr(JStr::from("::"), self.span(self.pos - 1, self.pos - 1)));
                }
                Err(self.error(jcat!("unsupported expression token: ", t, " ('", self.safe_text(), "')")))
            }
        }
    }

    // -------------------------------------------------------------------- number literals (NumberLiterals)

    /// `NumberLiterals.integer`: a long when it fits, else a `BigInteger` -- a lite extension the engine's dialect
    /// refuses.
    fn parse_integer(&mut self) -> R<CInteger> {
        let tok = self.pos;
        self.pos += 1;
        let text = self.tokens.text(tok);
        let span = self.span(tok, tok);
        match parse_long(text) {
            Ok(v) => Ok(CInteger { value: JValue::Long(v), pos: span }),
            Err(_) => {
                if self.dialect.refuses_lite_extensions() {
                    return Err(throw_at(self.tokens, tok, jcat!("Unexpected token '", text, "'")));
                }
                // new BigInteger(text): an INTEGER token's ASCII digits, its leading zeros not printed
                let digits = String::from_utf16_lossy(text);
                let trimmed = digits.trim_start_matches('0');
                Ok(CInteger {
                    value: JValue::BigInteger(if trimmed.is_empty() { "0".to_owned() } else { trimmed.to_owned() }),
                    pos: span,
                })
            }
        }
    }

    /// `NumberLiterals.floating`: a float; at lite's and the platform's dialects its exact digits kept when the double
    /// loses them (`new BigDecimal(text)` against `BigDecimal.valueOf(d)`).
    fn parse_float(&mut self) -> R<ValueSpecification> {
        let tok = self.pos;
        self.pos += 1;
        let mut text = self.tokens.text(tok);
        let span = self.span(tok, tok);
        if let Some(&last) = text.last()
            && (last == ch(b'f') || last == ch(b'F'))
        {
            text = &text[..text.len() - 1];
        }
        let d = parse_double(text).map_err(|m| JavaError::java("java.lang.NumberFormatException", m))?;
        if !self.dialect.refuses_lite_extensions() {
            let exact = java_big_decimal(text)?;
            // BigDecimal.valueOf(d): refuses an infinite or NaN double, else Double.toString's digits
            if !d.is_finite() {
                return Err(JavaError::java("java.lang.NumberFormatException", JStr::from("Infinite or NaN")));
            }
            let shown = java_big_decimal(&u(&lite_javacompat::double_to_string(d)))?;
            if exact.compare_to(&shown) != std::cmp::Ordering::Equal {
                return Ok(VS::CFloat(Box::new(CFloat { value: d, exact: Some(exact), pos: span })));
            }
        }
        Ok(VS::CFloat(Box::new(CFloat { value: d, exact: None, pos: span })))
    }

    /// `NumberLiterals.decimal`: the `d`/`D` suffix dropped, the rest a `BigDecimal`.
    fn parse_decimal(&mut self) -> R<CDecimal> {
        let tok = self.pos;
        self.pos += 1;
        let mut text = self.tokens.text(tok);
        if let Some(&last) = text.last()
            && (last == ch(b'd') || last == ch(b'D'))
        {
            text = &text[..text.len() - 1];
        }
        Ok(CDecimal { value: java_big_decimal(text)?, pos: self.span(tok, tok) })
    }

    // -------------------------------------------------------------------- strings, booleans, temporals

    /// A STRING token: quotes stripped, escapes resolved by THE escape table; the span includes the quotes.
    fn parse_string(&mut self) -> R<CString> {
        let raw = self.text();
        if !is_quoted(raw) {
            return Err(self.error_str("malformed string literal: missing surrounding quotes"));
        }
        let unescaped = self.unescape_body(&raw[1..raw.len() - 1], "string literal")?;
        let tok = self.pos;
        self.pos += 1;
        Ok(CString { value: JStr(unescaped), pos: self.span(tok, tok), multi_line: false })
    }

    /// A `'''...'''` literal in expression position: the text-block value, the single-line span, multi-line.
    fn parse_doc_string(&mut self) -> R<ValueSpecification> {
        let tok = self.pos;
        let raw = self.text();
        self.pos += 1;
        let value = crate::cursor::doc_string_value(raw)?;
        Ok(VS::CString(Box::new(CString {
            value: JStr(value),
            pos: boxed(self.doc_string_span(tok)),
            multi_line: true,
        })))
    }

    fn consume_boolean(&mut self, value: bool) -> ValueSpecification {
        let tok = self.pos;
        self.pos += 1;
        VS::CBoolean(Box::new(CBoolean { value, pos: self.span(tok, tok) }))
    }

    /// A DATE token: `PureDateLiteral.parse`, validated but at the engine's dialect, which (like its lexer) also
    /// refuses a `Z` zone.
    fn parse_date_or_date_time(&mut self) -> R<ValueSpecification> {
        let raw = self.text();
        if raw.is_empty() || raw[0] != ch(b'%') {
            return Err(self.error_str("malformed date literal: expected leading '%'"));
        }
        let value = &raw[1..];
        let date_pos = self.pos;
        self.pos += 1;
        if self.dialect.refuses_platform_dialect() && value.contains(&ch(b'Z')) {
            return Err(self.error(jcat!("Unexpected token '%", value, "'")));
        }
        match parse_date(value, self.dialect != Dialect::LegendEngine) {
            Ok(d) => Ok(VS::CDate(Box::new(CDate {
                value: d,
                written: Some(JStr::from(value)),
                pos: self.span(date_pos, date_pos),
            }))),
            Err(TemporalError::IllegalArgument(m)) => {
                Err(throw_at(self.tokens, date_pos, jcat!("invalid date literal '%", value, "': ", m)))
            }
            Err(TemporalError::DateTime(m)) => Err(JavaError::java("java.time.DateTimeException", m)),
            Err(TemporalError::IllegalState(m)) => Err(JavaError::java("java.lang.IllegalStateException", m)),
        }
    }

    /// A STRICTTIME token: an out-of-range time carries its raw text, unvalidated (the engine parses it).
    fn parse_strict_time(&mut self) -> R<ValueSpecification> {
        let raw = self.text();
        if raw.is_empty() || raw[0] != ch(b'%') {
            return Err(self.error_str("malformed time literal: expected leading '%'"));
        }
        let time_pos = self.pos;
        self.pos += 1;
        let value = &raw[1..];
        let parsed = match parse_time(value) {
            Ok(t) => Some(t),
            Err(TemporalError::IllegalArgument(_)) => None,
            Err(TemporalError::DateTime(m)) => return Err(JavaError::java("java.time.DateTimeException", m)),
            Err(TemporalError::IllegalState(m)) => return Err(JavaError::java("java.lang.IllegalStateException", m)),
        };
        Ok(VS::CTime(Box::new(CTime {
            value: parsed,
            written: Some(JStr::from(value)),
            pos: self.span(time_pos, time_pos),
        })))
    }

    fn parse_latest_date(&mut self) -> ValueSpecification {
        let tok = self.pos;
        self.pos += 1;
        VS::CLatestDate(Box::new(CLatestDate { pos: self.span(tok, tok) }))
    }

    /// `$name`, `$'quoted name'` decoded as `let 'quoted name'` binds it; the span covers the dollar.
    fn parse_variable(&mut self) -> R<Variable> {
        let dollar_tok = self.pos;
        self.pos += 1;
        if !is_identifier_token(self.peek()) {
            return Err(self.error_str("expected identifier after '$' to form a variable reference"));
        }
        let name =
            if self.peek() == T::STRING { self.unquote_and_unescape(self.text())? } else { self.text().to_vec() };
        self.pos += 1;
        Ok(Variable { name: JStr(name), r#type: None, multiplicity: None, pos: self.span(dollar_tok, self.pos - 1) })
    }

    // -------------------------------------------------------------------- collections and grouping

    fn parse_collection(&mut self) -> R<ValueSpecification> {
        let open = self.pos;
        self.pos += 1;
        self.parse_collection_body(open)
    }

    /// `[...]`: no trailing comma; the platform's range literals `[start:stop(:step)]` and `[:stop]` as `range(...)`;
    /// the stricter dialects reading each element as an `expression` (no top-level operator parts).
    fn parse_collection_body(&mut self, open: usize) -> R<ValueSpecification> {
        let mut values = Vec::new();
        if !self.at_end() && self.peek() == T::BRACKET_CLOSE {
            self.pos += 1;
            return Ok(collection(values, self.span(open, self.pos - 1)));
        }
        let platform = !self.dialect.refuses_platform_dialect();
        if platform && self.peek() == T::COLON {
            self.pos += 1; // the [:stop] form, start 0
            let stop = self.parse_combined_expression()?;
            self.expect_msg(T::BRACKET_CLOSE, "expected ']' to close range literal")?;
            let zero = VS::CInteger(Box::new(CInteger { value: JValue::Long(0), pos: None }));
            return Ok(vs_fn(infix(JStr::from("range"), vec![zero, stop], self.span(open, self.pos - 1))));
        }
        values.push(if platform { self.parse_combined_expression()? } else { self.parse_expression()? });
        if platform && self.peek() == T::COLON {
            self.pos += 1; // [start:stop(:step)]
            let mut args = vec![values.pop().expect("the start")];
            args.push(self.parse_combined_expression()?);
            if self.peek() == T::COLON {
                self.pos += 1;
                args.push(self.parse_combined_expression()?);
            }
            self.expect_msg(T::BRACKET_CLOSE, "expected ']' to close range literal")?;
            return Ok(vs_fn(infix(JStr::from("range"), args, self.span(open, self.pos - 1))));
        }
        while !self.at_end() && self.peek() == T::COMMA {
            self.pos += 1;
            if !self.at_end() && self.peek() == T::BRACKET_CLOSE {
                return Err(self.error_str("trailing comma in collection literal"));
            }
            values.push(if platform { self.parse_combined_expression()? } else { self.parse_expression()? });
        }
        self.expect_msg(T::BRACKET_CLOSE, "expected ']' to close collection literal")?;
        Ok(collection(values, self.span(open, self.pos - 1)))
    }

    /// `(expr)`: no node -- a call inside is marked grouped (the wire's flatten boundary).
    fn parse_parenthesised(&mut self) -> R<ValueSpecification> {
        self.pos += 1;
        let inner = self.parse_combined_expression()?;
        self.expect_msg(T::PAREN_CLOSE, "expected ')' to close parenthesised expression")?;
        Ok(match inner {
            VS::AppliedFunction(mut af) => {
                af.grouped = true;
                VS::AppliedFunction(af)
            }
            other => other,
        })
    }

    // -------------------------------------------------------------------- qualified-name primaries

    /// `5 RomanLength~Pes`: a number followed by a unit path is `newUnit(RomanLength~Pes, 5)`, a legend-pure form
    /// the engine's dialect refuses.
    fn unit_literal(&mut self, number: ValueSpecification) -> R<ValueSpecification> {
        if self.unit_path_end(self.pos).is_none() {
            return Ok(number);
        }
        if self.dialect.refuses_lite_extensions() {
            return Err(throw_at(self.tokens, self.pos, JStr::from("Unit instance not supported")));
        }
        let unit = self.parse_qualified_name_start()?;
        Ok(vs_fn(applied(JStr::from("newUnit"), vec![unit, number], None)))
    }

    /// An identifier-starting primary: a unit name, the `.all()` family, a call (with the quoted folds), or an
    /// element pointer.
    fn parse_qualified_name_start(&mut self) -> R<ValueSpecification> {
        let fqn_start = self.pos;
        let mut fqn = self.parse_qualified_name()?;
        if !self.at_end()
            && self.peek() == T::TILDE
            && self.pos + 1 < self.tokens.count()
            && is_fqn_segment_token(self.tokens.ty(self.pos + 1))
        {
            self.pos += 1;
            let unit = self.text();
            self.pos += 1;
            fqn.push(ch(b'~'));
            fqn.extend_from_slice(unit);
        }
        let fqn_end = self.pos - 1;
        if !self.at_end() && self.peek() == T::DOT {
            let saved = self.pos;
            self.pos += 1;
            if !self.at_end() {
                let t = self.peek();
                // the all-call families commit only when '(' follows (an enum value named 'all' is a member)
                let called = self.peek_at(1) == T::PAREN_OPEN;
                if t == T::ALL && called {
                    self.pos += 1;
                    let span = self.span(fqn_start, fqn_end);
                    return self.parse_all_call(fqn, saved, span).map(vs_fn);
                }
                if t == T::ALL_VERSIONS_IN_RANGE && called {
                    self.pos += 1;
                    let span = self.span(fqn_start, fqn_end);
                    return self.parse_all_versions_in_range_call(fqn, saved, span).map(vs_fn);
                }
                if t == T::ALL_VERSIONS {
                    // allVersions is legal without parentheses
                    self.pos += 1;
                    let span = self.span(fqn_start, fqn_end);
                    return self.parse_all_versions_call(fqn, saved, span).map(vs_fn);
                }
            }
            self.pos = saved;
        }
        if !self.at_end() && self.peek() == T::PAREN_OPEN {
            let args = self.parse_arg_list()?;
            let call = applied(unquoted(&fqn)?, args, self.span(fqn_start, fqn_end));
            if let Some(folded) = self.fold_tree(&call)? {
                return Ok(folded);
            }
            if let Some(folded) = self.fold_grammar(&call)? {
                return Ok(folded);
            }
            return Ok(vs_fn(call));
        }
        Ok(ptr(JStr(fqn), self.span(fqn_start, fqn_end)))
    }

    /// `Class.all(...)`: milestoning literals as `getAll` spanning `.all(...)`; a non-literal first argument the
    /// engine's property-call form.
    fn parse_all_call(&mut self, fqn: Vec<u16>, dot_tok: usize, fqn_span: Span) -> R<AppliedFunction> {
        let name_tok = self.pos - 1;
        self.expect_msg(T::PAREN_OPEN, "expected '(' after '.all'")?;
        let mut args = vec![ptr(JStr(fqn), fqn_span)];
        if !self.at_end() && self.peek() != T::PAREN_CLOSE && !opens_milestoning_literal(self.peek()) {
            return self.property_form_call("all", name_tok, args);
        }
        if !self.at_end() && self.peek() != T::PAREN_CLOSE {
            args.push(self.parse_milestoning_expression()?);
            if !self.at_end() && self.peek() == T::COMMA {
                self.pos += 1;
                args.push(self.parse_milestoning_expression()?);
            }
        }
        self.expect_msg(T::PAREN_CLOSE, "expected ')' to close '.all(...)'")?;
        Ok(applied(JStr::from("getAll"), args, self.span(dot_tok, self.pos - 1)))
    }

    /// The engine's property-call form of `all`/`allVersionsInRange` with general arguments, spanning the name.
    fn property_form_call(
        &mut self,
        name: &str,
        name_tok: usize,
        mut args: Vec<ValueSpecification>,
    ) -> R<AppliedFunction> {
        while !self.at_end() && self.peek() != T::PAREN_CLOSE {
            args.push(self.parse_combined_expression()?);
            if !self.match_tok(T::COMMA)? {
                break;
            }
        }
        self.expect_with(T::PAREN_CLOSE, || jcat!("expected ')' to close '.", name, "(...)'"))?;
        Ok(applied_full(JStr::from(name), args, self.span(name_tok, name_tok), true, false, false, false))
    }

    fn parse_all_versions_call(&mut self, fqn: Vec<u16>, dot_tok: usize, fqn_span: Span) -> R<AppliedFunction> {
        self.expect_msg(T::PAREN_OPEN, "expected '(' after '.allVersions'")?;
        self.expect_msg(T::PAREN_CLOSE, "'.allVersions()' takes no arguments")?;
        Ok(applied(JStr::from("getAllVersions"), vec![ptr(JStr(fqn), fqn_span)], self.span(dot_tok, self.pos - 1)))
    }

    fn parse_all_versions_in_range_call(
        &mut self,
        fqn: Vec<u16>,
        dot_tok: usize,
        fqn_span: Span,
    ) -> R<AppliedFunction> {
        let name_tok = self.pos - 1;
        let args_start = self.pos;
        self.expect_msg(T::PAREN_OPEN, "expected '(' after '.allVersionsInRange'")?;
        if !self.at_end() && !opens_milestoning_literal(self.peek()) {
            return self.property_form_call("allVersionsInRange", name_tok, vec![ptr(JStr(fqn), fqn_span)]);
        }
        let start = self.parse_milestoning_expression()?;
        self.expect_msg(T::COMMA, "expected ',' between range endpoints in '.allVersionsInRange'")?;
        let end = self.parse_milestoning_expression()?;
        self.expect_msg(T::PAREN_CLOSE, "expected ')' to close '.allVersionsInRange(...)'")?;
        if self.dialect.refuses_platform_dialect() {
            return Err(self.error(jcat!(
                ".allVersionsInRange",
                self.compact_text(args_start, self.pos - 1),
                " is not supported"
            )));
        }
        Ok(applied(
            JStr::from("getAllVersionsInRange"),
            vec![ptr(JStr(fqn), fqn_span), start, end],
            self.span(dot_tok, self.pos - 1),
        ))
    }

    /// A milestoning argument after a literal first one: `%latest`, a date or a variable.
    fn parse_milestoning_expression(&mut self) -> R<ValueSpecification> {
        if self.at_end() {
            return Err(self.error_str("expected milestoning expression (%date, %latest, or $variable)"));
        }
        let t = self.peek();
        match t {
            T::LATEST_DATE => Ok(self.parse_latest_date()),
            T::DATE => self.parse_date_or_date_time(),
            T::DOLLAR => self.parse_variable().map(vs_var),
            _ => Err(self.error(jcat!(
                "expected milestoning expression (%date, %latest, or $variable), got ",
                t,
                " ('",
                self.safe_text(),
                "')"
            ))),
        }
    }

    // -------------------------------------------------------------------- postfix

    /// `.prop`, `.prop(args)` (a property call spanning the name; `$x.f()` IS `$x.f` but at the platform's
    /// dialect), `Enum.VALUE` on an element pointer.
    fn parse_dot_postfix(&mut self, receiver: ValueSpecification) -> R<ValueSpecification> {
        self.pos += 1;
        if self.at_end() {
            return Err(self.error_str("expected property name after '.'"));
        }
        let name_start = self.pos;
        let name = JStr(self.read_property_name()?);
        let name_end = self.pos - 1;
        if !self.at_end() && self.peek() == T::PAREN_OPEN {
            let args = self.parse_arg_list()?;
            if args.is_empty() && self.dialect.refuses_platform_dialect() {
                return Ok(property(receiver, name, self.span(name_start, name_end)));
            }
            let mut params = Vec::with_capacity(1 + args.len());
            params.push(receiver);
            params.extend(args);
            return Ok(vs_fn(applied_full(name, params, self.span(name_start, name_end), true, false, false, false)));
        }
        if let VS::PackageableElementPtr(p) = receiver {
            let node = p.pos.is_none();
            return Ok(VS::EnumValue(Box::new(EnumValue {
                full_path: p.full_path,
                value: name,
                enumeration_pos: p.pos,
                pos: self.span(name_start, name_end),
                node,
            })));
        }
        Ok(property(receiver, name, self.span(name_start, name_end)))
    }

    /// A property name: a quoted string (decoded) or an identifier.
    fn read_property_name(&mut self) -> R<Vec<u16>> {
        let t = self.peek();
        if t == T::STRING {
            let raw = self.text();
            if !is_quoted(raw) {
                return Err(self.error_str("malformed quoted property: missing surrounding quotes"));
            }
            let name = self.unescape_body(&raw[1..raw.len() - 1], "string literal")?;
            self.pos += 1;
            return Ok(name);
        }
        if is_identifier_token(t) {
            let name = self.text().to_vec();
            self.pos += 1;
            return Ok(name);
        }
        Err(self.error_str("expected property name (identifier or 'quoted name') after '.'"))
    }

    /// `receiver->fn(args)`: the receiver the first parameter, the span the name, the name unquoted.
    fn parse_arrow_postfix(&mut self, receiver: ValueSpecification) -> R<AppliedFunction> {
        self.pos += 1;
        let fn_start = self.pos;
        let f = self.parse_qualified_name()?;
        let fn_end = self.pos - 1;
        if self.peek() != T::PAREN_OPEN {
            return Err(self.error(jcat!("expected '(' after arrow-call function name '", f, "'")));
        }
        let args = self.parse_arg_list()?;
        let mut params = Vec::with_capacity(1 + args.len());
        params.push(receiver);
        params.extend(args);
        Ok(applied(unquoted(&f)?, params, self.span(fn_start, fn_end)))
    }

    /// `(args)`: no trailing comma.
    fn parse_arg_list(&mut self) -> R<Vec<ValueSpecification>> {
        self.pos += 1;
        let mut args = Vec::new();
        if !self.at_end() && self.peek() == T::PAREN_CLOSE {
            self.pos += 1;
            return Ok(args);
        }
        args.push(self.parse_combined_expression()?);
        while !self.at_end() && self.peek() == T::COMMA {
            self.pos += 1;
            if !self.at_end() && self.peek() == T::PAREN_CLOSE {
                return Err(self.error_str("trailing comma in argument list"));
            }
            args.push(self.parse_combined_expression()?);
        }
        self.expect_msg(T::PAREN_CLOSE, "expected ')' to close argument list")?;
        Ok(args)
    }

    /// `$x[0]` (`at`) and `$x['key']` (a property): the platform's m3 indexing.
    fn parse_bracket_postfix(&mut self, receiver: ValueSpecification) -> R<ValueSpecification> {
        if self.dialect.refuses_platform_dialect() {
            return Err(self.error_str("Bracket operation is not supported"));
        }
        self.pos += 1;
        if self.at_end() {
            return Err(self.error_str("expected integer or string inside '[...]' index"));
        }
        let t = self.peek();
        let result = if t == T::INTEGER {
            let index = self.parse_integer()?;
            vs_fn(applied(JStr::from("at"), vec![receiver, VS::CInteger(Box::new(index))], None))
        } else if t == T::STRING {
            let key = self.parse_string()?;
            property(receiver, key.value, None)
        } else {
            return Err(self.error(jcat!(
                "expected integer or string inside '[...]' index, got ",
                t,
                " ('",
                self.safe_text(),
                "')"
            )));
        };
        self.expect_msg(T::BRACKET_CLOSE, "expected ']' to close bracket-index expression")?;
        Ok(result)
    }

    // -------------------------------------------------------------------- new instances

    /// `^Class<T>(k = v, ...)`, `^$x(...)`, `^Class($src)`: as `new(receiver, NewInstance)`; at the engine's and
    /// lite's dialects the caret specials' calls (`pair`, `col`, `tds::func`).
    fn parse_new_instance(&mut self) -> R<ValueSpecification> {
        self.pos += 1;
        let receiver;
        let class_name: Vec<u16>;
        let mut type_args: Vec<TypeExpression> = Vec::new();
        let mut type_mult_args: Vec<JStr> = Vec::new();
        if !self.at_end() && self.peek() == T::DOLLAR {
            if self.dialect.refuses_lite_extensions() {
                return Err(self.error_str("copy-with-update (^$var) is not supported"));
            }
            self.pos += 1;
            if !is_fqn_segment_token(self.peek()) {
                return Err(self.error_str("expected variable name after '^$' in copy-with-update"));
            }
            let var_name = self.text();
            self.pos += 1;
            receiver = vs_var(variable(JStr::from(var_name)));
            class_name = Vec::new();
        } else {
            let cn_start = self.pos;
            class_name = self.parse_qualified_name()?;
            let cn_end = self.pos - 1;
            if !self.at_end() && self.peek() == T::LESS_THAN {
                type_args = self.parse_type_arguments(&mut type_mult_args)?;
            }
            receiver = ptr(JStr(class_name.clone()), self.span(cn_start, cn_end));
            // ^X(10)(text = ...): type-variable values, parsed and not carried
            if !self.dialect.refuses_platform_dialect()
                && !self.at_end()
                && self.peek() == T::PAREN_OPEN
                && self.paren_group_followed_by_paren()
            {
                self.pos += 1;
                while !self.at_end() && self.peek() != T::PAREN_CLOSE {
                    self.parse_combined_expression()?;
                    if !self.at_end() && self.peek() == T::COMMA {
                        self.pos += 1;
                    }
                }
                self.expect_msg(T::PAREN_CLOSE, "expected ')' to close type-variable values")?;
            }
        }
        // old pure's named form '^Person klp (...)': the name parses and drops
        if !self.at_end() && is_fqn_segment_token(self.peek()) && self.peek_at(1) == T::PAREN_OPEN {
            self.pos += 1;
        }
        self.expect_msg(T::PAREN_OPEN, "expected '(' after class name or $variable in ^NewInstance")?;
        let mut properties: Vec<Box<NewInstanceKeyBinding>> = Vec::new();
        if !self.at_end() && self.peek() == T::PAREN_CLOSE {
            self.pos += 1;
            return self.wrap_new_instance(receiver, class_name, type_args, type_mult_args, properties);
        }
        // a positional cast ^Class($src): a body that does not start with a binding's name
        if !class_name.is_empty()
            && !is_fqn_segment_token(self.peek())
            && !(self.peek() == T::STRING
                && self.pos + 1 < self.tokens.count()
                && self.tokens.ty(self.pos + 1) == T::EQUAL)
        {
            let src = self.parse_combined_expression()?;
            self.expect_with(T::PAREN_CLOSE, || {
                jcat!("expected ')' to close ^", class_name, "($src) positional cast")
            })?;
            let cast =
                NewInstanceCast { class_name: JStr(class_name), type_arguments: type_args, src, target_set_id: None };
            return Ok(vs_fn(applied(JStr::from("new"), vec![receiver, VS::NewInstanceCast(Box::new(cast))], None)));
        }
        self.parse_and_put_key_expression(&mut properties)?;
        while !self.at_end() && self.peek() == T::COMMA {
            self.pos += 1;
            if !self.at_end() && self.peek() == T::PAREN_CLOSE {
                return Err(self.error_str("trailing comma in ^NewInstance binding list"));
            }
            self.parse_and_put_key_expression(&mut properties)?;
        }
        self.expect_msg(T::PAREN_CLOSE, "expected ')' to close ^NewInstance")?;
        self.wrap_new_instance(receiver, class_name, type_args, type_mult_args, properties)
    }

    fn wrap_new_instance(
        &self,
        receiver: ValueSpecification,
        class_name: Vec<u16>,
        type_args: Vec<TypeExpression>,
        type_mult_args: Vec<JStr>,
        properties: Vec<Box<NewInstanceKeyBinding>>,
    ) -> R<ValueSpecification> {
        let ni = NewInstance {
            class_name: JStr(class_name),
            type_arguments: type_args,
            type_multiplicity_arguments: type_mult_args,
            properties,
        };
        if self.dialect.refuses_platform_dialect() && matches!(receiver, VS::PackageableElementPtr(_)) {
            match caret_special(&ni) {
                Ok(Some(special)) => return Ok(vs_fn(special)),
                Ok(None) => {}
                Err(missing_key) => return Err(self.error(missing_key)),
            }
        }
        Ok(vs_fn(applied(JStr::from("new"), vec![receiver, VS::NewInstance(Box::new(ni))], None)))
    }

    /// One `key (= | +=) value` binding, appended (duplicates kept). A quoted key keeps its raw spelling; a dotted
    /// key joins its segments. The engine's dialect keys by the first segment and reads `+=` as `=`.
    fn parse_and_put_key_expression(&mut self, properties: &mut Vec<Box<NewInstanceKeyBinding>>) -> R<()> {
        if self.peek() == T::STRING && self.pos + 1 < self.tokens.count() && self.tokens.ty(self.pos + 1) == T::EQUAL {
            let key = self.text();
            self.pos += 2;
            let value_tok = self.pos;
            let value = self.parse_key_value()?;
            if self.instance_data && self.dialect.refuses_lite_extensions() {
                self.require_data_value(&value, value_tok)?;
            }
            properties.push(Box::new(NewInstanceKeyBinding {
                key: JStr::from(key),
                expression: Box::new(KeyExpression { value, is_add: false, is_local: false }),
            }));
            return Ok(());
        }
        if !is_fqn_segment_token(self.peek()) {
            return Err(self.error_str("expected property name in ^NewInstance binding"));
        }
        let mut key: Vec<u16> = self.text().to_vec();
        self.pos += 1;
        while self.pos + 1 < self.tokens.count()
            && self.peek() == T::DOT
            && is_fqn_segment_token(self.tokens.ty(self.pos + 1))
        {
            self.pos += 1;
            key.push(ch(b'.'));
            key.extend_from_slice(self.text());
            self.pos += 1;
        }
        let mut is_add = false;
        if !self.at_end() && self.peek() == T::PLUS {
            is_add = true;
            self.pos += 1;
        }
        self.expect_with(T::EQUAL, || {
            jcat!(
                "expected '",
                if is_add { "+=" } else { "=" },
                "' after property name '",
                key,
                "' in ^NewInstance binding"
            )
        })?;
        if self.instance_data && is_add {
            return Err(self.error(jcat!(
                "'+=' is not model store data's: a data instance binds '",
                key,
                "' with '='"
            )));
        }
        if self.instance_data && self.dialect.refuses_lite_extensions() && key.contains(&ch(b'.')) {
            return Err(self.error(jcat!("a dotted key is not model store data's: '", key, "'")));
        }
        let value_tok = self.pos;
        let value = self.parse_key_value()?;
        if self.instance_data && self.dialect.refuses_lite_extensions() {
            self.require_data_value(&value, value_tok)?;
        }
        if self.dialect.refuses_lite_extensions() && !self.instance_data {
            if let Some(dot) = index_of(&key, ch(b'.'), 0) {
                key.truncate(dot);
            }
            is_add = false;
        }
        properties.push(Box::new(NewInstanceKeyBinding {
            key: JStr(key),
            expression: Box::new(KeyExpression { value, is_add, is_local: false }),
        }));
        Ok(())
    }

    /// A model store data binding's value at the engine's dialect: a literal, an instance, an enumeration value, or a
    /// list of them.
    fn require_data_value(&self, value: &ValueSpecification, value_tok: usize) -> R<()> {
        let atoms: Vec<&ValueSpecification> = match value {
            VS::PureCollection(c) if self.tokens.ty(value_tok) == T::BRACKET_OPEN => c.values.iter().collect(),
            v => vec![v],
        };
        for a in atoms {
            let atomic = match a {
                VS::CString(_)
                | VS::CInteger(_)
                | VS::CFloat(_)
                | VS::CDecimal(_)
                | VS::CBoolean(_)
                | VS::CDate(_)
                | VS::CTime(_)
                | VS::EnumValue(_) => true,
                VS::AppliedProperty(p) => matches!(p.receiver, VS::PackageableElementPtr(_)),
                VS::AppliedFunction(f) => f.parameters.len() == 2 && matches!(f.parameters[1], VS::NewInstance(_)),
                _ => false,
            };
            if !atomic {
                return Err(JavaError::parse(
                    JStr::from(
                        "model store data takes a literal, an instance or an enumeration value (or a list of them), \
                         as legend-engine's data grammar reads it",
                    ),
                    self.tokens.start_line(value_tok) as i32,
                    self.tokens.start_column(value_tok) as i32,
                ));
            }
        }
        Ok(())
    }

    /// A binding's value: the engine's walker keeps the first `expression` of the right side; lite's dialect what was
    /// written. The rest is parsed either way.
    fn parse_key_value(&mut self) -> R<ValueSpecification> {
        let first = self.parse_expression()?;
        if self.dialect.refuses_lite_extensions() && !self.instance_data {
            let kept = first.clone();
            self.parse_combined_parts(first)?;
            return Ok(kept);
        }
        self.parse_combined_parts(first)
    }

    /// `<T1, T2|m>` after a class name or an annotation's type: types, then multiplicity arguments.
    fn parse_type_arguments(&mut self, mult_args_out: &mut Vec<JStr>) -> R<Vec<TypeExpression>> {
        self.pos += 1;
        let mut args = Vec::new();
        if !self.at_end() && self.peek() == T::GREATER_THAN {
            self.pos += 1;
            return Ok(args);
        }
        if !self.at_end() && self.peek() == T::PIPE {
            self.pos += 1;
            mult_args_out.push(JStr(self.parse_multiplicity_argument_text()?));
            while !self.at_end() && self.peek() == T::COMMA {
                self.pos += 1;
                mult_args_out.push(JStr(self.parse_multiplicity_argument_text()?));
            }
            self.expect_msg(T::GREATER_THAN, "expected '>' to close type arguments")?;
            return Ok(args);
        }
        args.push(self.parse_type()?);
        while !self.at_end() && self.peek() == T::COMMA {
            self.pos += 1;
            args.push(self.parse_type()?);
        }
        if !self.at_end() && self.peek() == T::PIPE {
            self.pos += 1;
            mult_args_out.push(JStr(self.parse_multiplicity_argument_text()?));
            while !self.at_end() && self.peek() == T::COMMA {
                self.pos += 1;
                mult_args_out.push(JStr(self.parse_multiplicity_argument_text()?));
            }
        }
        self.expect_msg(T::GREATER_THAN, "expected '>' to close type arguments")?;
        Ok(args)
    }

    // -------------------------------------------------------------------- lambdas

    /// `{p, q | body}`: a code block body in its own constant scope, the parameters shadowing; spanning pipe..body end.
    fn parse_lambda_function(&mut self) -> R<LambdaFunction> {
        self.pos += 1;
        let mut params = Vec::new();
        if !self.at_end() && self.peek() != T::PIPE {
            params.push(self.parse_lambda_param()?);
            while !self.at_end() && self.peek() == T::COMMA {
                self.pos += 1;
                params.push(self.parse_lambda_param()?);
            }
        }
        let pipe_tok = self.pos;
        self.expect_msg(T::PIPE, "expected '|' to separate lambda parameters from body")?;
        self.push_const_scope(&params);
        let body = self.parse_code_block_until(Some(T::BRACE_CLOSE));
        self.const_strings.pop();
        let body = body?;
        if body.is_empty() {
            return Err(self.error_str("lambda body must contain at least one statement"));
        }
        let body_end = self.pos - 1;
        self.expect_msg(T::BRACE_CLOSE, "expected '}' to close lambda")?;
        Ok(lambda(params, body, self.span(pipe_tok, body_end)))
    }

    /// `name` (no span) or `name: Type[mult]` (spanning the declaration).
    fn parse_lambda_param(&mut self) -> R<Variable> {
        if !is_fqn_segment_token(self.peek()) {
            return Err(self.error_str("expected lambda parameter name"));
        }
        let name_tok = self.pos;
        let name = JStr::from(self.text());
        self.pos += 1;
        if self.peek() != T::COLON {
            return Ok(variable(name));
        }
        self.advance()?;
        let t = self.parse_type()?;
        let m = self.parse_multiplicity()?;
        Ok(Variable { name, r#type: Some(t), multiplicity: Some(m), pos: self.span(name_tok, self.pos - 1) })
    }

    /// `x | body` / `x: T[m] | body`: a brace-less code block body (no constant scope of its own).
    fn parse_single_param_lambda(&mut self) -> R<LambdaFunction> {
        let param = self.parse_lambda_param()?;
        let pipe_tok = self.pos;
        self.expect_msg(T::PIPE, "expected '|' after shorthand lambda parameter")?;
        let mut body = vec![self.parse_program_line()?];
        self.read_brace_less_block(&mut body)?;
        Ok(lambda(vec![param], body, self.span(pipe_tok, self.pos - 1)))
    }

    /// Whether `x: Type[mult] |` stands at the cursor (the cursor restored either way).
    fn looks_like_typed_lambda_param(&mut self) -> R<bool> {
        let saved = self.pos;
        let r = self.typed_lambda_param_ahead();
        self.pos = saved;
        r
    }

    fn typed_lambda_param_ahead(&mut self) -> R<bool> {
        if !is_fqn_segment_token(self.peek()) {
            return Ok(false);
        }
        self.pos += 1;
        if self.peek() != T::COLON {
            return Ok(false);
        }
        self.advance()?;
        if !self.skip_type_for_lookahead() {
            return Ok(false);
        }
        if !self.at_end() && self.peek() == T::BRACKET_OPEN && !self.skip_multiplicity_for_lookahead()? {
            return Ok(false);
        }
        Ok(!self.at_end() && self.peek() == T::PIPE)
    }

    fn skip_type_for_lookahead(&mut self) -> bool {
        if self.peek() == T::PAREN_OPEN {
            return self.skip_balanced_for_lookahead(T::PAREN_OPEN, T::PAREN_CLOSE);
        }
        if self.peek() == T::BRACE_OPEN {
            return self.skip_balanced_for_lookahead(T::BRACE_OPEN, T::BRACE_CLOSE);
        }
        if !is_fqn_segment_token(self.peek()) {
            return false;
        }
        self.pos += 1;
        while !self.at_end() && self.peek() == T::PATH_SEPARATOR {
            self.pos += 1;
            if !is_fqn_segment_token(self.peek()) {
                return false;
            }
            self.pos += 1;
        }
        if !self.at_end() && self.peek() == T::LESS_THAN {
            let mut depth = 1;
            self.pos += 1;
            while !self.at_end() && depth > 0 {
                let t = self.peek();
                if t == T::LESS_THAN {
                    depth += 1;
                }
                if t == T::GREATER_THAN {
                    depth -= 1;
                }
                self.pos += 1;
            }
            if depth != 0 {
                return false;
            }
        }
        true
    }

    fn skip_balanced_for_lookahead(&mut self, open: TokenType, close: TokenType) -> bool {
        let mut depth = 1;
        self.pos += 1;
        while !self.at_end() && depth > 0 {
            let t = self.peek();
            if t == open {
                depth += 1;
            }
            if t == close {
                depth -= 1;
            }
            self.pos += 1;
        }
        depth == 0
    }

    fn skip_multiplicity_for_lookahead(&mut self) -> R<bool> {
        if self.peek() != T::BRACKET_OPEN {
            return Ok(false);
        }
        self.advance()?;
        while !self.at_end() && self.peek() != T::BRACKET_CLOSE {
            self.pos += 1;
        }
        if self.at_end() {
            return Ok(false);
        }
        self.pos += 1;
        Ok(true)
    }

    /// `| body`: a brace-less code block in its own (empty) constant scope.
    fn parse_lambda_pipe(&mut self) -> R<LambdaFunction> {
        let pipe_tok = self.pos;
        self.pos += 1;
        let mut body = Vec::new();
        self.push_const_scope(&[]);
        let r = self.brace_less_body(&mut body);
        self.const_strings.pop();
        r?;
        Ok(lambda(Vec::new(), body, self.span(pipe_tok, self.pos - 1)))
    }

    fn brace_less_body(&mut self, body: &mut Vec<ValueSpecification>) -> R<()> {
        body.push(self.parse_program_line()?);
        self.read_brace_less_block(body)
    }

    /// The rest of a brace-less lambda's body: a `;` after its first statement is the block's, with the statements
    /// after it, unless the enclosing statement sequence is owed it ([`Self::end_line_owed`]).
    fn read_brace_less_block(&mut self, body: &mut Vec<ValueSpecification>) -> R<()> {
        if self.at_end() || self.peek() != T::SEMI_COLON || self.end_line_owed() {
            return Ok(());
        }
        self.pos += 1; // the first statement's optional ';'
        let enclosing = self.owing_statement;
        let r = self.brace_less_statements(body);
        self.owing_statement = enclosing;
        r
    }

    fn brace_less_statements(&mut self, body: &mut Vec<ValueSpecification>) -> R<()> {
        while !self.at_end() && !is_lambda_body_terminator(self.peek()) {
            self.owing_statement = Some(self.pos); // a later statement: its ';' is required
            body.push(self.parse_program_line()?);
            if self.at_end() || self.peek() != T::SEMI_COLON {
                return Err(self.error_str(
                    "expected ';' after statement in a multi-statement lambda body (real Pure requires it)",
                ));
            }
            self.pos += 1;
        }
        Ok(())
    }

    /// Whether the `;` at the cursor is owed to the enclosing statement sequence: past its first statement, every
    /// bracket opened since that statement began closed.
    fn end_line_owed(&self) -> bool {
        let Some(owing) = self.owing_statement else {
            return false;
        };
        let mut depth = 0i64;
        for k in owing..self.pos {
            match self.tokens.ty(k) {
                T::PAREN_OPEN | T::BRACKET_OPEN | T::BRACE_OPEN => depth += 1,
                T::PAREN_CLOSE | T::BRACKET_CLOSE | T::BRACE_CLOSE => depth -= 1,
                _ => {}
            }
        }
        depth == 0
    }

    // -------------------------------------------------------------------- column builders

    /// A fused mapping command token (`~filter`, ...) read as a colspec of the command's name, anchored one column
    /// right of the tilde.
    fn parse_tilde_command_col_spec(&mut self) -> R<ColSpec> {
        let name = self.text()[1..].to_vec();
        let cmd_tok = self.pos;
        self.pos += 1;
        self.parse_col_spec_tail(name, cmd_tok, 1, Vec::new(), Vec::new())
    }

    fn parse_column_builders(&mut self) -> R<ColumnInstance> {
        self.pos += 1;
        if !self.at_end() && self.peek() == T::BRACKET_OPEN {
            return self.parse_col_spec_array().map(|a| ColumnInstance::ColSpecArray(Box::new(a)));
        }
        self.parse_one_col_spec().map(|c| ColumnInstance::ColSpec(Box::new(c)))
    }

    /// `~name`, `~'quoted'`, with leading `<<stereotypes>>`/`{tagged values}` read by the element grammar at this
    /// dialect, the span then starting at the first annotation.
    fn parse_one_col_spec(&mut self) -> R<ColSpec> {
        if self.at_end() {
            return Err(self.error_str("expected column name after '~'"));
        }
        let mut annot_start = None;
        let mut stereotypes: Vec<Box<PStereotype>> = Vec::new();
        let mut tagged_values: Vec<Box<PTaggedValue>> = Vec::new();
        if (self.peek() == T::LESS_THAN && self.peek_at(1) == T::LESS_THAN)
            || (self.peek() == T::BRACE_OPEN && self.looks_like_tagged_values())
        {
            annot_start = Some(self.pos);
            let mut sub = ElementCursor::at(self.tokens, self.pos, self.dialect);
            stereotypes = sub.parse_stereotypes()?;
            tagged_values = sub.parse_tagged_values()?;
            self.pos = sub.pos();
        }
        let name_tok_idx = annot_start.unwrap_or(self.pos);
        let name_tok = self.peek();
        let name = if name_tok == T::STRING {
            let raw = self.text();
            if !is_quoted(raw) {
                return Err(self.error_str("malformed quoted column name: missing surrounding quotes"));
            }
            let n = self.unescape_body(&raw[1..raw.len() - 1], "string literal")?;
            self.pos += 1;
            n
        } else if is_fqn_segment_token(name_tok) {
            let n = self.text().to_vec();
            self.pos += 1;
            n
        } else {
            return Err(self.error_str("expected column name after '~'"));
        };
        self.parse_col_spec_tail(name, name_tok_idx, 0, stereotypes, tagged_values)
    }

    /// After a colspec's name: the typed attempt (`~name:Type[m]`, accepted only when a terminator follows), else
    /// one or two lambdas; the span anchored at the name, through a function's body end.
    fn parse_col_spec_tail(
        &mut self,
        name: Vec<u16>,
        name_tok_idx: usize,
        start_col_shift: i32,
        stereotypes: Vec<Box<PStereotype>>,
        tagged_values: Vec<Box<PTaggedValue>>,
    ) -> R<ColSpec> {
        let mut function1: Option<LambdaFunction> = None;
        let mut function2: Option<LambdaFunction> = None;
        if !self.at_end() && self.peek() == T::COLON {
            self.pos += 1;
            let colon_mark = self.pos;
            if self.looks_like_column_type() {
                if let Some((col_type, col_mult)) = self.try_typed_col_spec()? {
                    return Ok(ColSpec {
                        name: JStr(name),
                        function1: None,
                        function2: None,
                        alias: None,
                        args: Vec::new(),
                        qualified: false,
                        pos: self.col_spec_span(name_tok_idx, self.pos - 1, start_col_shift),
                        col_type: Some(col_type),
                        col_type_mult: col_mult,
                        stereotypes,
                        tagged_values,
                    });
                }
                self.pos = colon_mark;
            }
            function1 = Some(self.parse_column_lambda()?);
            if !self.at_end() && self.peek() == T::COLON {
                self.pos += 1;
                function2 = Some(self.parse_column_lambda()?);
            }
        }
        let pos = if function1.is_none() {
            self.col_spec_span(name_tok_idx, name_tok_idx, start_col_shift)
        } else {
            self.col_spec_span(name_tok_idx, self.pos - 1, start_col_shift)
        };
        Ok(ColSpec {
            name: JStr(name),
            function1: function1.map(Box::new),
            function2: function2.map(Box::new),
            alias: None,
            args: Vec::new(),
            qualified: false,
            pos,
            col_type: None,
            col_type_mult: None,
            stereotypes,
            tagged_values,
        })
    }

    fn col_spec_span(&self, from: usize, to: usize, start_col_shift: i32) -> Span {
        let mut s = self.span_of(from, to);
        s.start_column += start_col_shift;
        boxed(s)
    }

    /// A `{` opening tagged values (`{profile.tag='v'}`) rather than a lambda: an identifier path then `.`.
    fn looks_like_tagged_values(&self) -> bool {
        let ts = self.tokens;
        let mut k = self.pos + 1;
        if k >= ts.count() || !is_fqn_segment_token(ts.ty(k)) {
            return false;
        }
        k += 1;
        while k + 1 < ts.count() && ts.ty(k) == T::PATH_SEPARATOR && is_fqn_segment_token(ts.ty(k + 1)) {
            k += 2;
        }
        k < ts.count() && ts.ty(k) == T::DOT
    }

    /// The cheap pre-check: do the tokens after `:` spell `FQN (<...>)? ((...))? ([mult])?` and then a terminator?
    fn looks_like_column_type(&self) -> bool {
        let ts = self.tokens;
        let n = ts.count();
        let mut k = self.pos;
        if k >= n || !is_fqn_segment_token(ts.ty(k)) {
            return false;
        }
        k += 1;
        while k + 1 < n && ts.ty(k) == T::PATH_SEPARATOR && is_fqn_segment_token(ts.ty(k + 1)) {
            k += 2;
        }
        for (open, close) in
            [(T::LESS_THAN, T::GREATER_THAN), (T::PAREN_OPEN, T::PAREN_CLOSE), (T::BRACKET_OPEN, T::BRACKET_CLOSE)]
        {
            if k < n && ts.ty(k) == open {
                let mut depth = 1;
                k += 1;
                while k < n && depth > 0 {
                    let t = ts.ty(k);
                    if t == open {
                        depth += 1;
                    } else if t == close {
                        depth -= 1;
                    }
                    k += 1;
                }
            }
        }
        if k >= n {
            return true;
        }
        matches!(ts.ty(k), T::COMMA | T::BRACKET_CLOSE | T::SEMI_COLON | T::PAREN_CLOSE | T::BRACE_CLOSE)
    }

    /// `tryTypedColSpec`: a type and an optional multiplicity followed by a terminator, or nothing (a
    /// `ParseException` on the way is nothing; the caller restores the cursor).
    fn try_typed_col_spec(&mut self) -> R<Option<(TypeExpression, Option<Multiplicity>)>> {
        let attempt = (|| -> R<(TypeExpression, Option<Multiplicity>)> {
            let col_type = self.parse_type()?;
            let col_mult =
                if !self.at_end() && self.peek() == T::BRACKET_OPEN { Some(self.parse_multiplicity()?) } else { None };
            Ok((col_type, col_mult))
        })();
        match attempt {
            Ok(typed) => {
                if self.at_end()
                    || matches!(
                        self.peek(),
                        T::COMMA | T::BRACKET_CLOSE | T::SEMI_COLON | T::PAREN_CLOSE | T::BRACE_CLOSE
                    )
                {
                    Ok(Some(typed))
                } else {
                    Ok(None)
                }
            }
            Err(e) if e.is_parse_exception() => Ok(None),
            Err(e) => Err(e),
        }
    }

    fn parse_col_spec_array(&mut self) -> R<ColSpecArray> {
        let open = self.pos;
        self.pos += 1;
        let mut specs = Vec::new();
        if !self.at_end() && self.peek() == T::BRACKET_CLOSE {
            self.pos += 1;
            return Ok(ColSpecArray { col_specs: specs, pos: self.span(open, self.pos - 1) });
        }
        specs.push(Box::new(self.parse_one_col_spec()?));
        while !self.at_end() && self.peek() == T::COMMA {
            self.pos += 1;
            if !self.at_end() && self.peek() == T::BRACKET_CLOSE {
                return Err(self.error_str("trailing comma in ColSpec array"));
            }
            specs.push(Box::new(self.parse_one_col_spec()?));
        }
        self.expect_msg(T::BRACKET_CLOSE, "expected ']' to close ColSpec array")?;
        Ok(ColSpecArray { col_specs: specs, pos: self.span(open, self.pos - 1) })
    }

    /// A lambda after a colspec's `:` -- the four lambda forms, else a general expression as a parameterless thunk.
    fn parse_column_lambda(&mut self) -> R<LambdaFunction> {
        if self.at_end() {
            return Err(self.error_str("expected lambda after ':' in column spec"));
        }
        let t = self.peek();
        if t == T::PIPE {
            return self.parse_lambda_pipe();
        }
        if t == T::BRACE_OPEN {
            return self.parse_lambda_function();
        }
        if is_identifier_token(t) {
            if self.pos + 1 < self.tokens.count() && self.tokens.ty(self.pos + 1) == T::PIPE {
                return self.parse_single_param_lambda();
            }
            if self.pos + 1 < self.tokens.count()
                && self.tokens.ty(self.pos + 1) == T::COLON
                && self.looks_like_typed_lambda_param()?
            {
                return self.parse_single_param_lambda();
            }
        }
        let body = self.parse_combined_expression()?;
        Ok(lambda(Vec::new(), vec![body], None))
    }

    // -------------------------------------------------------------------- type annotations

    /// `@Type`, `@Type<args>`, `@Varchar(200)`, `@Mass~Kilogram`, `@Relation<(...)>`, `@(...)`, the platform's `@[m]`.
    fn parse_type_annotation(&mut self) -> R<TypeAnnotation> {
        let at_tok = self.pos;
        self.pos += 1;
        let platform = !self.dialect.refuses_platform_dialect();
        if platform && !self.at_end() && self.peek() == T::BRACKET_OPEN {
            let multiplicity = self.parse_multiplicity()?;
            return Ok(TypeAnnotation::TypeAnnotationMultiplicityRef(Box::new(TypeAnnotationMultiplicityRef {
                multiplicity,
                pos: self.span(at_tok, self.pos - 1),
            })));
        }
        if !self.at_end() && self.peek() == T::PAREN_OPEN {
            let columns = self.parse_relation_shape()?;
            return Ok(TypeAnnotation::TypeAnnotationRelationShape(Box::new(TypeAnnotationRelationShape {
                columns,
                spelled_name: None,
                type_span: self.span(at_tok, self.pos - 1),
                pos: self.span(at_tok, self.pos - 1),
            })));
        }
        if !is_fqn_segment_token(self.peek()) {
            return Err(self.error_str("expected type name after '@'"));
        }
        let name_start = self.pos;
        let mut name = self.parse_qualified_name()?;
        if !self.at_end()
            && self.peek() == T::TILDE
            && self.pos + 1 < self.tokens.count()
            && is_fqn_segment_token(self.tokens.ty(self.pos + 1))
        {
            self.pos += 1;
            name.push(ch(b'~'));
            name.extend_from_slice(self.text());
            self.pos += 1;
        }
        if spelled_relation(&name)
            && !self.at_end()
            && self.peek() == T::LESS_THAN
            && self.pos + 1 < self.tokens.count()
            && self.tokens.ty(self.pos + 1) == T::PAREN_OPEN
        {
            self.pos += 1;
            let columns = self.parse_relation_shape()?;
            self.expect_msg(T::GREATER_THAN, "expected '>' to close @Relation type annotation")?;
            return Ok(TypeAnnotation::TypeAnnotationRelationShape(Box::new(TypeAnnotationRelationShape {
                columns,
                spelled_name: Some(JStr(name)),
                type_span: self.span(name_start, self.pos - 1),
                pos: self.span(at_tok, self.pos - 1),
            })));
        }
        if !self.at_end() && self.peek() == T::PAREN_OPEN {
            let tvv = self.parse_type_variable_values()?;
            let t = TypeExpression::TypeExpressionGeneric(Box::new(TypeExpressionGeneric {
                name: JStr(name),
                arguments: Vec::new(),
                multiplicity_arguments: Vec::new(),
                type_variable_values: tvv,
                pos: self.span(name_start, self.pos - 1),
            }));
            return Ok(named(t, self.span(at_tok, self.pos - 1)));
        }
        let t = if !self.at_end() && self.peek() == T::LESS_THAN {
            let mut mult = Vec::new();
            let args = self.parse_type_arguments(&mut mult)?;
            TypeExpression::TypeExpressionGeneric(Box::new(TypeExpressionGeneric {
                name: JStr(name),
                arguments: args,
                multiplicity_arguments: mult,
                type_variable_values: Vec::new(),
                pos: self.span(name_start, self.pos - 1),
            }))
        } else {
            name_ref(JStr(name), self.span(name_start, self.pos - 1))
        };
        Ok(named(t, self.span(at_tok, self.pos - 1)))
    }

    /// `(col:Type, ...)` of a relation-shape annotation: the cursor on `(`, left past `)`.
    fn parse_relation_shape(&mut self) -> R<Vec<Box<TypeAnnotationRelationShapeColumn>>> {
        self.pos += 1;
        let mut columns = Vec::new();
        if !self.at_end() && self.peek() != T::PAREN_CLOSE {
            columns.push(Box::new(self.parse_shape_column()?));
            while !self.at_end() && self.peek() == T::COMMA {
                self.pos += 1;
                if !self.at_end() && self.peek() == T::PAREN_CLOSE {
                    return Err(self.error_str("trailing comma in @Relation columns"));
                }
                columns.push(Box::new(self.parse_shape_column()?));
            }
        }
        self.expect_msg(T::PAREN_CLOSE, "expected ')' to close @Relation columns")?;
        Ok(columns)
    }

    /// One `(name | 'quoted' | ?) : (Type | ?) [mult]?` column (`SpecParser.parseRelationColumn`): `[0..1]`
    /// undeclared.
    fn parse_shape_column(&mut self) -> R<TypeAnnotationRelationShapeColumn> {
        if self.at_end() {
            return Err(self.error_str("expected column name or '?' inside @Relation"));
        }
        let name_tok = self.peek();
        let name_tok_idx = self.pos;
        let name = if name_tok == T::QUESTION {
            self.pos += 1;
            None
        } else if name_tok == T::STRING {
            let raw = self.text();
            if !is_quoted(raw) {
                return Err(self.error_str("malformed quoted column name inside @Relation"));
            }
            let n = self.unescape_body(&raw[1..raw.len() - 1], "string literal")?;
            self.pos += 1;
            Some(JStr(n))
        } else if is_fqn_segment_token(name_tok) {
            let n = JStr::from(self.text());
            self.pos += 1;
            Some(n)
        } else {
            return Err(self.error(jcat!("expected column name or '?' inside @Relation, got ", name_tok)));
        };
        self.expect_msg(T::COLON, "expected ':' after column name inside @Relation")?;
        let r#type = if !self.at_end() && self.peek() == T::QUESTION {
            self.pos += 1;
            TypeAnnotation::TypeAnnotationWildcard(Box::new(TypeAnnotationWildcard {}))
        } else {
            named(self.parse_type()?, None)
        };
        let multiplicity = if !self.at_end() && self.peek() == T::BRACKET_OPEN {
            self.parse_multiplicity()?
        } else {
            concrete(0, Some(1))
        };
        Ok(TypeAnnotationRelationShapeColumn { name, r#type, multiplicity, pos: self.span(name_tok_idx, self.pos - 1) })
    }

    // -------------------------------------------------------------------- path literals

    /// `#/Type/prop1/prop2(args)!alias#`: its segments with their offsets into the literal, the navigation lambda
    /// it means, a dated segment's arguments re-parsed as `|arg` expressions and scanned charwise for the wire.
    fn parse_path_literal(&mut self) -> R<ValueSpecification> {
        let text = self.text();
        let lit_tok = self.pos;
        self.pos += 1;
        let inner = ustr::substring(text, 2, text.len() as isize - 1)?;
        let segs = split_char(inner, ch(b'/'));
        // the TYPE segment is a well-formed qualified name: no single or triple colons
        {
            let tseg = strip(&segs[0]);
            let mut colons = 0;
            for (ci, &c) in tseg.iter().enumerate() {
                if c == ch(b':') {
                    colons += 1;
                    if colons > 2 || ci == 0 || ci == tseg.len() - 1 {
                        return Err(self.error_str("Unexpected token"));
                    }
                } else {
                    if colons == 1 {
                        return Err(self.error_str("Unexpected token"));
                    }
                    colons = 0;
                }
            }
            if colons == 1 {
                return Err(self.error_str("Unexpected token"));
            }
        }
        let mut pieces: Vec<Box<PathLiteralSegment>> = Vec::new();
        let mut has_dated = false;
        let mut cursor: isize = 2; // the current segment's first char within the literal
        let mut body = vs_var(variable(JStr::from("_path")));
        let mut alias: Option<JStr> = None;
        for i in 1..segs.len() {
            cursor += segs[i - 1].len() as isize + 1;
            let raw_seg: &[u16] = &segs[i];
            let mut seg: &[u16] = strip(raw_seg);
            let lead =
                index_of_units(raw_seg, if seg.is_empty() { &[] } else { &seg[..1] }, 0).map_or(-1, |v| v as isize);
            // the LAST segment may carry '!alias'
            if let Some(bang) = index_of(seg, ch(b'!'), 0)
                && i == segs.len() - 1
                && bang > 0
                && is_word(&seg[..bang])
                && is_word(&seg[bang + 1..])
            {
                alias = Some(JStr::from(&seg[bang + 1..]));
                seg = &seg[..bang];
            }
            let open = index_of(seg, ch(b'('), 0);
            let is_dated_seg = open.is_some_and(|o| {
                o > 0 && ends_with(seg, ")") && is_word(&seg[..o]) && index_of(seg, ch(b')'), 0) == Some(seg.len() - 1)
            });
            let inner_end = |s: &[u16]| (cursor + lead + s.len() as isize - 1) as i32;
            if is_dated_seg {
                let o = open.expect("a dated segment has its '('");
                if strip(&seg[o + 1..seg.len() - 1]).is_empty() && self.dialect.refuses_platform_dialect() {
                    // prop(): at the engine's and lite's dialects the property itself
                    let name = JStr::from(&seg[..o]);
                    pieces.push(Box::new(PathLiteralSegment {
                        name: name.clone(),
                        inner_start: cursor as i32,
                        inner_end: inner_end(seg),
                        args: Vec::new(),
                        unsupported_arg: false,
                    }));
                    body = property(body, name, None);
                    continue;
                }
                let dated_name = JStr::from(&seg[..o]);
                let dated_args = &seg[o + 1..seg.len() - 1];
                has_dated = true;
                let mut args = vec![body];
                for arg in split_top_level(dated_args) {
                    let stripped = strip(&arg);
                    let mut src = u("|");
                    src.extend_from_slice(stripped);
                    let parsed = parse(&src, self.dialect)?;
                    args.push(match parsed {
                        VS::LambdaFunction(lf) if lf.body.len() == 1 => {
                            lf.body.into_iter().next().expect("a one-statement body")
                        }
                        _ => vs_var(variable(JStr::from(stripped))),
                    });
                }
                let mut wire_args = Vec::new();
                let mut unsupported = false;
                // the argument scan indexes rawSeg, which starts at the cursor
                let from = index_of(raw_seg, ch(b'('), 0).map_or(-1, |v| v as isize) + 1;
                let to = last_index_of(raw_seg, ch(b')')).map_or(-1, |v| v as isize);
                self.scan_path_args(raw_seg, from, to, cursor, &mut wire_args, &mut unsupported, false)?;
                pieces.push(Box::new(PathLiteralSegment {
                    name: dated_name.clone(),
                    inner_start: cursor as i32,
                    inner_end: inner_end(seg),
                    args: wire_args,
                    unsupported_arg: unsupported,
                }));
                body = vs_fn(applied(dated_name, args, None));
                continue;
            }
            if !is_word(seg) {
                return Err(self.error(jcat!(
                    "navigation path segment '",
                    seg,
                    "' uses an unsupported path feature (only plain property segments desugar): ",
                    text
                )));
            }
            pieces.push(Box::new(PathLiteralSegment {
                name: JStr::from(seg),
                inner_start: cursor as i32,
                inner_end: inner_end(seg),
                args: Vec::new(),
                unsupported_arg: false,
            }));
            body = property(body, JStr::from(seg), None);
        }
        let start_type = JStr::from(strip(&segs[0]));
        let param = Variable {
            name: JStr::from("_path"),
            r#type: Some(name_ref(start_type.clone(), None)),
            multiplicity: None,
            pos: None,
        };
        let desugared = lambda(vec![param], vec![body], None);
        let line_starts: Vec<i32> =
            text.iter().enumerate().filter(|&(_, &c)| c == ch(b'\n')).map(|(k, _)| k as i32 + 1).collect();
        Ok(VS::PathLiteral(Box::new(PathLiteral {
            start_type,
            segments: pieces,
            desugared: Box::new(desugared),
            alias,
            has_dated_segment: has_dated,
            pos: self.span(lit_tok, lit_tok),
            literal_length: text.len() as i32,
            line_starts,
        })))
    }

    /// A dated segment's argument list `raw_seg[from, to)` scanned charwise; a collection recurses one level; a kind
    /// with no probed wire shape marks the literal unsupported.
    #[allow(clippy::too_many_arguments)]
    fn scan_path_args(
        &self,
        raw_seg: &[u16],
        from: isize,
        to: isize,
        base: isize,
        out: &mut Vec<PathLiteralPathArg>,
        unsupported: &mut bool,
        in_collection: bool,
    ) -> R<()> {
        let mut i = from;
        while i < to && i >= 0 {
            let c = raw_seg[i as usize];
            if is_whitespace(c) || c == ch(b',') {
                i += 1;
                continue;
            }
            if c == ch(b'[') && !in_collection {
                let close = matching_bracket(raw_seg, i, to);
                if close < 0 {
                    *unsupported = true;
                    return Ok(());
                }
                let mut elems = Vec::new();
                self.scan_path_args(raw_seg, i + 1, close, base, &mut elems, unsupported, true)?;
                out.push(PathLiteralPathArg::PathLiteralPathArgCollectionArg(Box::new(
                    PathLiteralPathArgCollectionArg { elements: elems },
                )));
                i = close + 1;
                continue;
            }
            let sc = self.scan_scalar(raw_seg, i, to)?;
            let (s, e) = ((base + sc.s) as i32, (base + sc.e) as i32);
            match sc.kind {
                ScalarKind::Skip => {}
                ScalarKind::Latest => {
                    out.push(PathLiteralPathArg::PathLiteralPathArgLatest(Box::new(PathLiteralPathArgLatest {
                        start: s,
                        end: e,
                    })))
                }
                ScalarKind::Date => {
                    if is_date_body(&sc.a) {
                        out.push(PathLiteralPathArg::PathLiteralPathArgDateArg(Box::new(PathLiteralPathArgDateArg {
                            value: JStr(sc.a.clone()),
                            start: s,
                            end: e,
                        })));
                    } else {
                        *unsupported = true;
                    }
                }
                ScalarKind::Int => {
                    out.push(PathLiteralPathArg::PathLiteralPathArgIntArg(Box::new(PathLiteralPathArgIntArg {
                        value: sc.int_value,
                        start: s,
                        end: e,
                    })))
                }
                ScalarKind::String => {
                    out.push(PathLiteralPathArg::PathLiteralPathArgStrArg(Box::new(PathLiteralPathArgStrArg {
                        value: JStr(sc.a.clone()),
                        start: s,
                        end: e,
                    })))
                }
                ScalarKind::Enum => {
                    out.push(PathLiteralPathArg::PathLiteralPathArgEnumArg(Box::new(PathLiteralPathArgEnumArg {
                        full_path: JStr(sc.a.clone()),
                        value: JStr(sc.b.clone()),
                    })))
                }
                ScalarKind::Numericish | ScalarKind::Bool | ScalarKind::Var | ScalarKind::Other => *unsupported = true,
            }
            i = sc.next;
        }
        Ok(())
    }

    /// `IslandScan.scanScalar`: THE one scalar reader for island argument text, this parser its error anchor.
    fn scan_scalar(&self, src: &[u16], i: isize, limit: isize) -> R<Scalar> {
        let at = |k: isize| src[k as usize];
        let c = at(i);
        if c == ch(b'/') && i + 1 < limit && at(i + 1) == ch(b'/') {
            let mut k = i;
            while k < limit && at(k) != ch(b'\n') {
                k += 1;
            }
            return Ok(Scalar::of(ScalarKind::Skip, &[], &[], 0, i, k, k));
        }
        if c == ch(b'%') {
            let mut k = i + 1;
            while k < limit && (is_letter_or_digit(at(k)) || u("-T:.Z+").contains(&at(k))) {
                k += 1;
            }
            let body = &src[(i + 1) as usize..k as usize];
            if body.is_empty() {
                return Ok(Scalar::of(ScalarKind::Other, &[], &[], 0, i, i, i + 1));
            }
            let kind = if eq(body, "latest") { ScalarKind::Latest } else { ScalarKind::Date };
            return Ok(Scalar::of(kind, body, &src[i as usize..k as usize], 0, i, k - 1, k));
        }
        if is_digit(c) {
            let mut k = i;
            while k < limit && is_digit(at(k)) {
                k += 1;
            }
            if k < limit && (at(k) == ch(b'.') || is_graph_ident_char(at(k))) {
                // a float/decimal/suffixed lexeme, consumed whole
                while k < limit && (at(k) == ch(b'.') || is_graph_ident_char(at(k))) {
                    k += 1;
                }
                return Ok(Scalar::of(ScalarKind::Numericish, &src[i as usize..k as usize], &[], 0, i, k - 1, k));
            }
            let digits = &src[i as usize..k as usize];
            let int_val =
                parse_long(digits).map_err(|_| self.error(jcat!("integer literal out of range: '", digits, "'")))?;
            return Ok(Scalar::of(ScalarKind::Int, &[], &[], int_val, i, k - 1, k));
        }
        if c == ch(b'\'') {
            let close = closing_quote(src, i + 1, limit);
            if close < 0 {
                return Ok(Scalar::of(ScalarKind::Other, &[], &[], 0, i, limit - 1, limit));
            }
            let value = self.unescape_body(&src[(i + 1) as usize..close as usize], "string literal")?;
            return Ok(Scalar::of(ScalarKind::String, &value, &[], 0, i, close, close + 1));
        }
        if c == ch(b'$') {
            let mut k = i + 1;
            while k < limit && is_graph_ident_char(at(k)) {
                k += 1;
            }
            if k == i + 1 {
                return Ok(Scalar::of(ScalarKind::Other, &[], &[], 0, i, i, i + 1));
            }
            return Ok(Scalar::of(ScalarKind::Var, &src[(i + 1) as usize..k as usize], &[], 0, i + 1, k - 1, k));
        }
        if is_letter(c) || c == ch(b'_') {
            let mut k = i;
            while k < limit && (is_graph_ident_char(at(k)) || at(k) == ch(b':')) {
                k += 1;
            }
            let word = &src[i as usize..k as usize];
            if eq(word, "true") || eq(word, "false") {
                let v = if eq(word, "true") { 1 } else { 0 };
                return Ok(Scalar::of(ScalarKind::Bool, word, &[], v, i, k - 1, k));
            }
            if k < limit && at(k) == ch(b'.') {
                let vs = k + 1;
                let mut ve = vs;
                while ve < limit && is_graph_ident_char(at(ve)) {
                    ve += 1;
                }
                if ve > vs {
                    return Ok(Scalar::of(ScalarKind::Enum, word, &src[vs as usize..ve as usize], 0, i, ve - 1, ve));
                }
            }
            return Ok(Scalar::of(ScalarKind::Other, word, &[], 0, i, k - 1, k));
        }
        Ok(Scalar::of(ScalarKind::Other, &[], &[], 0, i, i, i + 1))
    }

    // -------------------------------------------------------------------- TDS literals

    /// `#TDS{...}#` (the engine's form) or the brace-less `#TDS ... #` (legend-pure's, refused at the engine's
    /// dialect): its untrimmed table as `meta::legend::lite::tds('TDS', table)`.
    fn parse_tds_literal(&mut self) -> R<ValueSpecification> {
        let raw = self.text();
        let tok = self.pos;
        self.pos += 1;
        let braced = starts_with(raw, "#TDS{") && ends_with(raw, "}#");
        if !braced && starts_with(raw, "#TDS{") {
            return Err(self.error_str("unterminated #TDS{ literal: no }# closes it"));
        }
        if !braced && self.dialect.refuses_lite_extensions() {
            return Err(self.error_str("brace-less #TDS literal is not supported"));
        }
        let len = raw.len() as isize;
        let inner = if braced {
            ustr::substring(raw, 5, len - 2)?
        } else {
            ustr::substring(raw, 4, if ends_with(raw, "#") { len - 1 } else { len })?
        };
        let tds_string = JStr::from(inner);
        let desugared = applied(
            JStr::from("meta::legend::lite::tds"),
            vec![cstring(JStr::from("TDS"), None), cstring(tds_string.clone(), None)],
            None,
        );
        Ok(VS::TdsLiteral(Box::new(TdsLiteral {
            tds_string,
            desugared: Box::new(desugared),
            pos: self.span(tok, tok),
        })))
    }

    // -------------------------------------------------------------------- DSL islands

    /// `#{...}#` (a graph-fetch tree), `#>{...}#` (a table reference), `#SQL{...}#`, `#GQL{...}#`, lite's `#IQ{...}#`
    /// and `#Cypher{...}#`, the platform's legacy `#Class{...}#`: the content collected to the island's `}#` (nested
    /// islands kept inside).
    fn parse_dsl(&mut self) -> R<ValueSpecification> {
        let island_start = self.pos;
        let island_open = self.text();
        self.pos += 1;
        let dsl_type = &island_open[1..island_open.len() - 1];
        let mut content: Vec<u16> = Vec::new();
        let mut depth = 0;
        while !self.at_end() {
            let t = self.peek();
            if t == T::ISLAND_OPEN || t == T::ISLAND_START {
                depth += 1;
                content.extend_from_slice(self.text());
            } else if t == T::ISLAND_END {
                if depth == 0 {
                    self.pos += 1;
                    break;
                }
                depth -= 1;
                content.extend_from_slice(self.text());
            } else if t == T::ISLAND_BRACE_OPEN {
                content.push(ch(b'{'));
            } else if t == T::ISLAND_BRACE_CLOSE {
                content.push(ch(b'}'));
            } else {
                content.extend_from_slice(self.text());
            }
            self.pos += 1;
        }
        if eq(dsl_type, "IQ") || starts_with(dsl_type, "IQ:") {
            return self.iq_island(dsl_type, content, island_start);
        }
        if eq(dsl_type, "Cypher") {
            return self.cypher_island(dsl_type, content, island_start);
        }
        if dsl_type.is_empty() {
            // the tree is the source between the opener '#{' and the closer '}#'
            let (from, to) = (self.tokens.end(island_start), self.tokens.start(self.pos - 1));
            return self.parse_graph_fetch_tree(from, to, island_start).map(|g| VS::GraphFetchLiteral(Box::new(g)));
        }
        if eq(dsl_type, ">") {
            let content_text = trim(&content);
            let span = self.span_of(island_start, self.pos - 1);
            return self.parse_table_reference(content_text, span).map(vs_fn);
        }
        if eq(dsl_type, "SQL") {
            return Ok(VS::SqlIsland(Box::new(SqlIsland {
                sql: JStr(content),
                pos: self.span(island_start, self.pos - 1),
                language: JStr::from("SQL"),
                profile: None,
            })));
        }
        if eq(dsl_type, "GQL") {
            // parsed from the raw source slice, laid at the opener's end
            let ts = self.tokens;
            let src = ustr::substring(ts.source(), ts.end(island_start) as isize, ts.start(self.pos - 1) as isize)?;
            let document = crate::gql::parse_document(
                src,
                ts.end_line(island_start) as i32,
                ts.end_column(island_start) as i32 + 1,
            )?;
            return Ok(VS::GqlIsland(Box::new(GqlIsland {
                document: Box::new(document),
                pos: self.span(island_start, self.pos - 1),
            })));
        }
        // the platform's legacy graph-fetch spelling puts the root class in the island kind
        if !self.dialect.refuses_platform_dialect() && is_type_name_shaped(dsl_type) {
            let (from, to) = (self.tokens.start(island_start) + 1, self.tokens.start(self.pos - 1) + 1);
            return self.parse_graph_fetch_tree(from, to, island_start).map(|g| VS::GraphFetchLiteral(Box::new(g)));
        }
        Err(self.error(jcat!("unknown DSL island type: '#", dsl_type, "{'")))
    }

    /// `#Cypher{ MATCH ... }#`: lite's island, its text untrimmed; unknown at the engine's dialect.
    fn cypher_island(&self, dsl_type: &[u16], cypher: Vec<u16>, island_start: usize) -> R<ValueSpecification> {
        if self.dialect.refuses_lite_extensions() {
            return Err(self.error(jcat!("unknown DSL island type: '#", dsl_type, "{'")));
        }
        Ok(VS::SqlIsland(Box::new(SqlIsland {
            sql: JStr(cypher),
            pos: self.span(island_start, self.pos - 1),
            language: JStr::from("Cypher"),
            profile: None,
        })))
    }

    /// `#IQ{...}#` / `#IQ:<profile>{...}#`: lite's SAP IQ island, a profile one of `IqProfile`'s closed set.
    fn iq_island(&self, dsl_type: &[u16], sql: Vec<u16>, island_start: usize) -> R<ValueSpecification> {
        if self.dialect.refuses_lite_extensions() {
            return Err(self.error(jcat!("unknown DSL island type: '#", dsl_type, "{'")));
        }
        let mut profile = None;
        if starts_with(dsl_type, "IQ:") {
            let p = &dsl_type[3..];
            if !IQ_PROFILES.iter().any(|name| eq(p, name)) {
                return Err(throw_at(
                    self.tokens,
                    island_start,
                    jcat!("unknown #IQ{...}# profile '", p, "': a profile is one of ", IQ_PROFILES.join(", ")),
                ));
            }
            profile = Some(JStr::from(p));
        }
        Ok(VS::SqlIsland(Box::new(SqlIsland {
            sql: JStr(sql),
            pos: self.span(island_start, self.pos - 1),
            language: JStr::from("IQ"),
            profile,
        })))
    }

    /// `#>{db::path.TABLE}#` as `tableReference(db, 'schema.TABLE')`, split at the first dot after the `::` path; a
    /// store alone legal. A damaged island's junk is the engine's `Unexpected token`.
    fn parse_table_reference(&self, content: &[u16], span: SourceInfo) -> R<AppliedFunction> {
        if content.iter().any(|&c| u("()|;={}\n").contains(&c)) {
            return Err(JavaError::parse(JStr::from("Unexpected token"), span.start_line, span.start_column));
        }
        let path_end = last_index_of_str(content, "::");
        let dot = index_of(content, ch(b'.'), path_end.map_or(0, |p| p + 2));
        let (db, rest) = match dot {
            None => (content, None),
            Some(d) => (&content[..d], Some(&content[d + 1..])),
        };
        let mut params = vec![ptr(JStr::from(db), None)];
        if let Some(r) = rest {
            params.push(cstring(JStr::from(r), None));
        }
        Ok(applied_full(JStr::from("tableReference"), params, boxed(span), false, false, false, true))
    }

    /// A graph-fetch tree: its source slice re-lexed in place (absolute spans) and read by a parser of its own.
    fn parse_graph_fetch_tree(&self, from: usize, to: usize, island_start: usize) -> R<GraphFetchLiteral> {
        let inner_tokens = self
            .tokens
            .lex_range(from, to)
            .map_err(|m| JavaError::java("java.lang.IndexOutOfBoundsException", JStr::from(m)))?;
        if inner_tokens.count() == 0 {
            return Err(JavaError::parse(
                JStr::from("Graph fetch tree must not be empty"),
                self.tokens.start_line(island_start) as i32,
                self.tokens.start_column(island_start) as i32,
            ));
        }
        let mut inner = SpecParser::new(&inner_tokens, self.span_source_id.clone(), self.dialect, false);
        let name_start = inner.pos;
        let class_name = inner.parse_qualified_name()?;
        let name_pos = inner.span(name_start, inner.pos - 1);
        let mut sub_types = Vec::new();
        let nodes = inner.parse_graph_definition(Some(&mut sub_types))?;
        if !inner.at_end() {
            return Err(inner.error(jcat!("trailing content after graph-fetch tree: '", inner.safe_text(), "'")));
        }
        Ok(GraphFetchLiteral {
            class_name: JStr(class_name),
            sub_trees: nodes,
            sub_type_trees: sub_types,
            pos: name_pos,
        })
    }

    /// `{ entry (, entry)* }`: the property nodes; root-level `->subType(@X) {...}` entries to `sub_types` (none
    /// below the root). A trailing comma the platform's alone.
    fn parse_graph_definition(
        &mut self,
        mut sub_types: Option<&mut Vec<Box<GraphFetchLiteralSubTypeNode>>>,
    ) -> R<Vec<Box<GraphFetchLiteralNode>>> {
        self.expect_msg(T::BRACE_OPEN, "expected '{' to open graph-fetch body")?;
        let mut nodes = Vec::new();
        let mut any = false;
        if !self.at_end() && self.peek() != T::BRACE_CLOSE {
            self.parse_graph_entry(&mut nodes, sub_types.as_deref_mut())?;
            any = true;
            while !self.at_end() && self.peek() == T::COMMA {
                self.pos += 1;
                if !self.at_end() && self.peek() == T::BRACE_CLOSE {
                    if self.dialect.refuses_platform_dialect() {
                        return Err(self.error_str("Unexpected token '}'"));
                    }
                    break;
                }
                self.parse_graph_entry(&mut nodes, sub_types.as_deref_mut())?;
            }
        }
        if !any && self.dialect.refuses_platform_dialect() {
            return Err(self.error_str("Unexpected token '}'"));
        }
        self.expect_msg(T::BRACE_CLOSE, "expected '}' to close graph-fetch body")?;
        Ok(nodes)
    }

    fn parse_graph_entry(
        &mut self,
        nodes: &mut Vec<Box<GraphFetchLiteralNode>>,
        sub_types: Option<&mut Vec<Box<GraphFetchLiteralSubTypeNode>>>,
    ) -> R<()> {
        if self.at_sub_type_arrow() {
            let Some(sub_types) = sub_types else {
                return Err(JavaError::parse_unplaced(JStr::from("->subType() is supported only at root level")));
            };
            let (class_name, pos) = self.parse_sub_type_view()?;
            let body = if !self.at_end() && self.peek() == T::BRACE_OPEN {
                self.parse_graph_definition(None)?
            } else {
                Vec::new()
            };
            if body.is_empty() {
                return Err(JavaError::parse_unplaced(JStr::from("a ->subType() body must not be empty")));
            }
            sub_types.push(Box::new(GraphFetchLiteralSubTypeNode { sub_type_class: class_name, pos, sub_trees: body }));
            return Ok(());
        }
        let mut alias = None;
        if self.pos + 1 < self.tokens.count() && self.peek() == T::STRING && self.tokens.ty(self.pos + 1) == T::COLON {
            // the alias keeps its raw body: the engine's quote strip is naked
            let raw = self.text();
            alias = Some(JStr::from(if raw.len() >= 2 { &raw[1..raw.len() - 1] } else { raw }));
            self.pos += 2;
        }
        if !is_fqn_segment_token(self.peek()) {
            return Err(self.error_str("expected property name in graph-fetch path"));
        }
        let prop = JStr::from(self.text());
        let name_pos = self.span(self.pos, self.pos);
        self.pos += 1;
        let mut args = Vec::new();
        if !self.at_end() && self.peek() == T::PAREN_OPEN {
            let open = self.pos;
            args = self.parse_arg_list()?;
            let mut t = open;
            while t < self.pos && self.dialect.refuses_lite_extensions() {
                if self.tokens.ty(t) == T::LATEST_DATE {
                    return Err(throw_at(
                        self.tokens,
                        t,
                        JStr::from("%latest as a graph fetch argument is not supported in Legend Engine"),
                    ));
                }
                t += 1;
            }
        }
        let sub_type = if self.at_sub_type_arrow() { Some(self.parse_sub_type_view()?.0) } else { None };
        let children = if !self.at_end() && self.peek() == T::BRACE_OPEN {
            self.parse_graph_definition(None)?
        } else {
            Vec::new()
        };
        nodes.push(Box::new(GraphFetchLiteralNode {
            property: prop,
            pos: name_pos,
            parameters: args,
            alias,
            sub_type,
            sub_trees: children,
        }));
        Ok(())
    }

    fn at_sub_type_arrow(&self) -> bool {
        !self.at_end()
            && self.peek() == T::ARROW
            && self.pos + 1 < self.tokens.count()
            && is_fqn_segment_token(self.tokens.ty(self.pos + 1))
            && eq(self.tokens.text(self.pos + 1), "subType")
    }

    /// `->subType(@X)`: the class as written, spanning the name without the `@`.
    fn parse_sub_type_view(&mut self) -> R<(JStr, Span)> {
        self.pos += 2;
        self.expect_msg(T::PAREN_OPEN, "expected '(' after ->subType")?;
        self.expect_msg(T::AT, "expected '@' in ->subType(@Type)")?;
        let class_start = self.pos;
        let sub_class = self.parse_qualified_name()?;
        let class_pos = self.span(class_start, self.pos - 1);
        self.expect_msg(T::PAREN_CLOSE, "expected ')' after ->subType type")?;
        Ok((JStr(sub_class), class_pos))
    }

    // -------------------------------------------------------------------- the quoted folds (QuotedSpecParser)

    /// `QuotedSpecParser.fold`: `compileLegendValueSpecification('<tree literal>')` (a literal, a `+` chain, or a
    /// let-bound constant in scope) parsed here, at this parse's dialect, into the two-faced `QuotedTreeCall`.
    fn fold_tree(&self, call: &AppliedFunction) -> R<Option<ValueSpecification>> {
        if !(call.function == "compileLegendValueSpecification"
            || call.function == "meta::legend::compileLegendValueSpecification")
            || call.parameters.len() != 1
        {
            return Ok(None);
        }
        let Some(src) = fold_string_concat(&call.parameters[0], &|n: &[u16]| self.const_string(n)) else {
            return Ok(None);
        };
        let tree = match parse(trim(&src), self.dialect) {
            Ok(v @ VS::GraphFetchLiteral(_)) => v,
            Ok(_) => return Ok(None),
            Err(e) if e.is_parse_exception() => return Ok(None),
            Err(e) => return Err(e),
        };
        Ok(Some(VS::QuotedTreeCall(Box::new(QuotedTreeCall {
            original: Box::new(call.clone()),
            tree,
            pos: call.pos.clone(),
        }))))
    }

    /// `QuotedSpecParser.foldGrammar`: `compileLegendGrammar('<payload>')` (a literal, a `+` chain, or a let-bound
    /// constant in scope) whose payload parses, at the engine's dialect, as functions only: the two-faced
    /// `QuotedGrammarCall`, each function as its lambda (its declared parameters, its body). A payload the model
    /// parser refuses with a `ParseException`, or one declaring anything else, leaves the plain call.
    fn fold_grammar(&self, call: &AppliedFunction) -> R<Option<ValueSpecification>> {
        if !(call.function == "compileLegendGrammar" || call.function == "meta::legend::compileLegendGrammar")
            || call.parameters.len() != 1
        {
            return Ok(None);
        }
        let Some(src) = fold_string_concat(&call.parameters[0], &|n: &[u16]| self.const_string(n)) else {
            return Ok(None);
        };
        let parsed = match crate::model_parser::parse(&src, Dialect::LegendEngine) {
            Ok(p) => p,
            Err(e) if e.is_parse_exception() => return Ok(None),
            Err(e) => return Err(e),
        };
        let mut functions = Vec::new();
        for el in parsed.model.elements {
            match el {
                Element::PSectionIndex(_) => continue,
                Element::PFunction(fd) => {
                    let fd = *fd;
                    let parameters = fd
                        .parameters
                        .into_iter()
                        .map(|p| {
                            Box::new(Variable {
                                name: p.name,
                                r#type: Some(p.r#type),
                                multiplicity: Some(p.multiplicity),
                                pos: None,
                            })
                        })
                        .collect();
                    functions.push(Box::new(LambdaFunction { parameters, body: fd.body, pos: None }));
                }
                _ => return Ok(None),
            }
        }
        if functions.is_empty() {
            return Ok(None);
        }
        Ok(Some(VS::QuotedGrammarCall(Box::new(QuotedGrammarCall {
            original: Box::new(call.clone()),
            functions,
            pos: call.pos.clone(),
        }))))
    }
}

/// `QuotedSpecParser.foldStringConcat`: a literal string, a variable the constants resolve, or a `plus` chain of
/// them (its collection flattened one level); else none.
fn fold_string_concat(v: &ValueSpecification, constants: &dyn Fn(&[u16]) -> Option<Vec<u16>>) -> Option<Vec<u16>> {
    match v {
        VS::CString(cs) => Some(cs.value.0.clone()),
        VS::Variable(var) => constants(&var.name.0),
        VS::AppliedFunction(pf) if pf.function == "plus" || pf.function == "meta::pure::functions::math::plus" => {
            let mut sb = Vec::new();
            for p in &pf.parameters {
                if let VS::PureCollection(pc) = p {
                    for e in &pc.values {
                        sb.extend(fold_string_concat(e, constants)?);
                    }
                    continue;
                }
                sb.extend(fold_string_concat(p, constants)?);
            }
            Some(sb)
        }
        _ => None,
    }
}

/// `CaretSpecials.call`: the call the engine writes for `^Pair`, `^BasicColumnSpecification`, `^TdsOlapRank`, its
/// keys in their order; a missing required key the `UnsupportedOperationException` message the parser rethrows.
fn caret_special(ni: &NewInstance) -> Result<Option<AppliedFunction>, JStr> {
    let spelled = &ni.class_name;
    let (function, keys, drop_missing): (&str, &[&str], bool) =
        if *spelled == "Pair" || *spelled == "meta::pure::functions::collection::Pair" {
            ("meta::pure::functions::collection::pair", &["first", "second"], false)
        } else if *spelled == "BasicColumnSpecification" || *spelled == "meta::pure::tds::BasicColumnSpecification" {
            ("meta::pure::tds::col", &["func", "name", "documentation"], true)
        } else if *spelled == "TdsOlapRank" || *spelled == "meta::pure::tds::TdsOlapRank" {
            ("meta::pure::tds::func", &["func"], false)
        } else {
            return Ok(None);
        };
    let mut params = Vec::with_capacity(keys.len());
    for key in keys {
        match ni.properties.iter().find(|b| b.key == *key) {
            Some(b) => params.push(b.expression.value.clone()),
            None if drop_missing => continue,
            None => {
                return Err(jcat!("no rule for a caret special missing key '", key, "' (at ", spelled, ")"));
            }
        }
    }
    Ok(Some(applied(JStr::from(function), params, None)))
}

/// `NumberLiterals`' `new BigDecimal(text)` over a number token's text.
fn java_big_decimal(text: &[u16]) -> R<BigDecimal> {
    BigDecimal::parse_units(text).map_err(|m| JavaError {
        class: "java.lang.NumberFormatException".to_owned(),
        message: m,
        position: None,
    })
}

/// `signedLiteral`: a model store's signed number as one literal -- an integer that fits a long a long, a float its
/// exact digits negated too.
fn signed_literal(n: ValueSpecification, negative: bool, span: Span) -> ValueSpecification {
    match n {
        VS::CInteger(c) => {
            let value = match (c.value, negative) {
                (v, false) => v,
                (JValue::Long(v), true) => {
                    if v == i64::MIN {
                        JValue::BigInteger("9223372036854775808".to_owned())
                    } else {
                        JValue::Long(-v)
                    }
                }
                (JValue::BigInteger(d), true) => {
                    if d == "9223372036854775808" {
                        JValue::Long(i64::MIN)
                    } else if let Some(pos) = d.strip_prefix('-') {
                        JValue::BigInteger(pos.to_owned())
                    } else {
                        JValue::BigInteger(format!("-{d}"))
                    }
                }
                (other, true) => other,
            };
            VS::CInteger(Box::new(CInteger { value, pos: span }))
        }
        VS::CFloat(c) => VS::CFloat(Box::new(CFloat {
            value: if negative { -c.value } else { c.value },
            exact: match c.exact {
                Some(e) if negative => Some(e.negate()),
                e => e,
            },
            pos: span,
        })),
        VS::CDecimal(c) => {
            VS::CDecimal(Box::new(CDecimal { value: if negative { c.value.negate() } else { c.value }, pos: span }))
        }
        other => panic!("IllegalStateException: a number literal is an integer, a float or a decimal: {other:?}"),
    }
}

fn named(t: TypeExpression, pos: Span) -> TypeAnnotation {
    TypeAnnotation::TypeAnnotationNamed(Box::new(TypeAnnotationNamed { r#type: t, pos }))
}

/// `TypeAnnotation.RelationShape.spelledRelation`: the simple name `Relation`.
fn spelled_relation(name: &[u16]) -> bool {
    match last_index_of_str(name, "::") {
        None => eq(name, "Relation"),
        Some(cut) => eq(&name[cut + 2..], "Relation"),
    }
}

/// `IqProfile`'s spellings, in declaration order.
const IQ_PROFILES: &[&str] =
    &["iq16-default", "iq16-odbc", "iq16-tds", "sqla17-default", "sqla17-tds", "iq17-default", "iq17-odbc", "iq17-tds"];

/// A legacy island kind: a possibly qualified type name.
fn is_type_name_shaped(s: &[u16]) -> bool {
    if s.is_empty() || !(is_letter(s[0]) || s[0] == ch(b'_')) {
        return false;
    }
    s[1..].iter().all(|&c| is_letter_or_digit(c) || c == ch(b'_') || c == ch(b':'))
}

/// Every char in the date-literal class `[0-9T:.Z+-]`, non-empty.
fn is_date_body(s: &[u16]) -> bool {
    !s.is_empty() && s.iter().all(|&c| is_digit(c) || u("T:.Z+-").contains(&c))
}

/// Every char word-class (`\w`: a letter, a digit, `_`), non-empty.
fn is_word(s: &[u16]) -> bool {
    !s.is_empty() && s.iter().all(|&c| is_letter_or_digit(c) || c == ch(b'_'))
}

fn is_graph_ident_char(c: u16) -> bool {
    is_letter_or_digit(c) || c == ch(b'_')
}

/// The closing quote from `from`, honouring backslash escapes; -1 before `limit`.
fn closing_quote(src: &[u16], from: isize, limit: isize) -> isize {
    let mut k = from;
    while k < limit {
        let qc = src[k as usize];
        if qc == ch(b'\\') {
            k += 1;
        } else if qc == ch(b'\'') {
            return k;
        }
        k += 1;
    }
    -1
}

/// Split on every `sep` outside a quoted string (trailing empties kept).
fn split_char(s: &[u16], sep: u16) -> Vec<Vec<u16>> {
    let mut out = Vec::new();
    let mut start = 0;
    let mut in_quote = false;
    let mut i = 0;
    while i <= s.len() {
        if i == s.len() {
            out.push(s[start..].to_vec());
            break;
        }
        let c = s[i];
        if in_quote && c == ch(b'\\') {
            i += 1;
        } else if c == ch(b'\'') {
            in_quote = !in_quote;
        } else if c == sep && !in_quote {
            out.push(s[start..i].to_vec());
            start = i + 1;
        }
        i += 1;
    }
    out
}

/// Split on top-level commas only (bracket-, paren- and quote-aware); empty pieces dropped.
fn split_top_level(s: &[u16]) -> Vec<Vec<u16>> {
    let mut out = Vec::new();
    let mut depth = 0i64;
    let mut start = 0;
    let mut in_quote = false;
    let mut i = 0;
    while i <= s.len() {
        let c = if i < s.len() { s[i] } else { ch(b',') };
        if in_quote && i < s.len() {
            if c == ch(b'\\') {
                i += 1;
            } else if c == ch(b'\'') {
                in_quote = false;
            }
            i += 1;
            continue;
        }
        if c == ch(b'\'') {
            in_quote = true;
        } else if c == ch(b'[') || c == ch(b'(') {
            depth += 1;
        } else if c == ch(b']') || c == ch(b')') {
            depth -= 1;
        } else if c == ch(b',') && depth == 0 {
            let piece = strip(&s[start.min(s.len())..i.min(s.len())]);
            if !piece.is_empty() {
                out.push(piece.to_vec());
            }
            start = i + 1;
        }
        i += 1;
    }
    out
}

/// The `]` closing the `[` at `open`, before `limit`; -1 when none.
fn matching_bracket(s: &[u16], open: isize, limit: isize) -> isize {
    let mut depth = 0;
    let mut i = open;
    while i < limit {
        let c = s[i as usize];
        if c == ch(b'[') {
            depth += 1;
        } else if c == ch(b']') {
            depth -= 1;
            if depth == 0 {
                return i;
            }
        }
        i += 1;
    }
    -1
}

#[cfg(test)]
mod tests {
    use super::*;

    fn form(text: &str, dialect: Dialect) -> String {
        String::from_utf16_lossy(&crate::doors::answer_form("spec", dialect, &u(text)))
    }

    #[test]
    fn the_engine_s_operator_shapes() {
        let f = form("1 < 2 + 3 * 4", Dialect::LegendLite);
        assert!(f.starts_with("A\t{\"@\":\"protocol.spec.AppliedFunction\",\"function\":\"lessThan\""), "{f}");
        let f = form("$x.a->filter(p | $p.b == 1)", Dialect::LegendEngine);
        assert!(f.starts_with("A\t"), "{f}");
        let f = form("1 +", Dialect::LegendLite);
        assert_eq!(f, "E\tcom.legend.parser.ParseException\t1\t4\t[1:4] expected expression, got end of input");
    }
}
