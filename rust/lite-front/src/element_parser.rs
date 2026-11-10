// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! Pure's domain grammar (core/'s `parser.ElementParser`, the part the model parser reaches): a `###Pure` section's
//! elements -- classes (properties, qualified properties, constraints, type and multiplicity parameters, type
//! variables, projections), enumerations, profiles, associations (and their projections), measures, functions (their
//! constraints and test blocks), the platform's `native` declarations, lite's `Primitive`s -- each parsed into its
//! protocol record by the cursor [`ElementCursor`] positioned at its first token, as `ElementParser.at` positions
//! Java's. Bodies go through the expression parser over a slice of the file's stream (`SpecParser.parse(tokens.slice
//! (...))`), so every span stays file-absolute.
//!
//! Java's line-level behaviour is reproduced, quirks included: each refusal is Java's message at Java's position.

// The generated records hold their record-typed list elements boxed (`Vec<Box<T>>`), so the readers build that.
#![allow(clippy::vec_box)]

use crate::cursor::{Cursor, Documentation, ElementCursor, R, is_identifier_token, unquote_path_iae};
use crate::escapes::unescape_java_like;
use crate::jcat;
use crate::jtypes::JStr;
use crate::lexer::TokenType;
use crate::nodes::{Span, boxed, name_ref};
use crate::protocol::{mentions_return, realization_of, split_fqn};
use crate::records::{
    ConstraintDefinition, DerivedPropertyDefinition, Element, Multiplicity, PAssertion, PAssertionEqualTo,
    PAssertionEqualToJson, PAssertionEqualToRelation, PAssociation, PAssociationProjection, PClass, PDefaultValue,
    PEnumValue, PEnumeration, PFunction, PFunctionTest, PMeasure, PNativeFunction, PPrimitive, PProfile, PProfileEntry,
    PProperty, PStereotype, PSuperType, PTaggedValue, PTestData, PTestParam, PTestPayload, PTestPayloadCsvTable,
    PTestPayloadExternalFormat, PTestPayloadModelEmbedded, PTestPayloadModelStoreData, PTestPayloadReference,
    PTestPayloadRelationElement, PTestPayloadRelationElements, PTestPayloadRelationalCsv, PTestSuite, PUnit,
    ParameterDefinition, SourceInfo, TypeExpression, ValueSpecification,
};
use crate::spec_parser::{parse_code_block_tokens, parse_tokens};
use crate::ustr::{eq, index_of, index_of_units, last_index_of_str, strip, trim, u};
use lite_javacompat::character::is_whitespace;
use std::collections::HashSet;

use TokenType as T;
use ValueSpecification as VS;

/// A function's signature (`ElementParser.FunctionSignature`): shared by concrete and native functions.
struct FunctionSignature {
    decl_start: usize,
    qualified_name: Vec<u16>,
    type_params: Vec<JStr>,
    mult_params: Vec<JStr>,
    params: Vec<Box<ParameterDefinition>>,
    return_type: TypeExpression,
    return_mult: Multiplicity,
    stereotypes: Vec<Box<PStereotype>>,
    tagged_values: Vec<Box<PTaggedValue>>,
}

/// The token bracket kinds every body scan balances.
fn opens(t: TokenType) -> bool {
    matches!(t, T::PAREN_OPEN | T::BRACKET_OPEN | T::BRACE_OPEN)
}

fn closes(t: TokenType) -> bool {
    matches!(t, T::PAREN_CLOSE | T::BRACKET_CLOSE | T::BRACE_CLOSE)
}

/// Java's `String.toUpperCase(Locale.ROOT)` / `toLowerCase(Locale.ROOT)` over an identifier: ASCII by table, any other
/// text by the Unicode full case mapping both follow.
fn java_case(s: &[u16], upper: bool) -> Vec<u16> {
    if s.iter().all(|&c| c < 0x80) {
        return s
            .iter()
            .map(|&c| if upper { (c as u8).to_ascii_uppercase() } else { (c as u8).to_ascii_lowercase() } as u16)
            .collect();
    }
    let text = String::from_utf16_lossy(s);
    u(&if upper {
        text.to_uppercase()
    } else {
        text.to_lowercase()
    })
}

impl<'s> ElementCursor<'_, 's> {
    fn slice_spec(&self, from: usize, to: usize) -> R<ValueSpecification> {
        parse_tokens(&self.tokens.slice(from, to), self.dialect)
    }

    fn slice_block(&self, from: usize, to: usize) -> R<Vec<ValueSpecification>> {
        parse_code_block_tokens(&self.tokens.slice(from, to), JStr::default(), self.dialect)
    }

    /// `isIdentifierToken(peek())`.
    fn at_identifier(&self) -> bool {
        is_identifier_token(self.peek())
    }

    // ================================================================ top level

    /// `skipTopLevelNonElement`: the platform dialect reads past a headerless `Diagram`, a top-level `^Instance(...)`
    /// and a stray `)`, none of which defines an element; false (nothing consumed) for anything else.
    pub fn skip_top_level_non_element(&mut self) -> R<bool> {
        if self.at_identifier() && eq(self.text(), "Diagram") {
            self.advance()?;
            self.parse_qualified_name()?;
            if self.peek() == T::PAREN_OPEN {
                self.skip_balanced_block()?;
            }
            if self.peek() == T::BRACE_OPEN {
                self.skip_balanced_block()?;
            }
            return Ok(true);
        }
        if self.peek() == T::NEW_SYMBOL {
            self.advance()?;
            self.parse_qualified_name()?;
            if self.peek() != T::PAREN_OPEN {
                return Err(self.error_str("top-level ^Instance must be followed by (...)"));
            }
            self.skip_balanced_block()?;
            return Ok(true);
        }
        if self.peek() == T::PAREN_CLOSE {
            self.advance()?;
            return Ok(true);
        }
        Ok(false)
    }

    /// `parseNativeProtocol`: a `native Class` or `native function` (the platform dialect only).
    pub fn parse_native_protocol(&mut self) -> R<Element> {
        let decl_start = self.pos;
        let doc = self.parse_documentation()?;
        if self.dialect.refuses_platform_dialect() {
            return Err(self.error_str("Unsupported syntax"));
        }
        self.advance()?; // 'native'
        match self.peek() {
            T::CLASS => Ok(Element::PClass(Box::new(
                self.parse_class_definition_at(true, decl_start, doc)?,
            ))),
            T::FUNCTION => Ok(Element::PNativeFunction(Box::new(
                self.parse_native_function_protocol(decl_start, doc)?,
            ))),
            other => Err(self.error(jcat!(
                "expected 'Class' or 'function' after 'native', got ",
                other,
                " ('",
                self.safe_text(),
                "')"
            ))),
        }
    }

    // ================================================================ Class

    /// `parseClassDefinition(isNative)`: one `Class` declaration, its documentation first.
    pub fn parse_class_definition(&mut self, is_native: bool) -> R<PClass> {
        let class_start_tok = self.pos;
        let doc = self.parse_documentation()?;
        self.parse_class_definition_at(is_native, class_start_tok, doc)
    }

    fn parse_class_definition_at(
        &mut self,
        is_native: bool,
        class_start_tok: usize,
        doc: Option<Documentation>,
    ) -> R<PClass> {
        self.expect(T::CLASS)?;
        let stereotypes = self.parse_stereotypes()?;
        let tvs = self.parse_tagged_values()?;
        let tagged_values = self.tagged_values_with_documentation(doc, tvs)?;
        let qualified_name = self.parse_qualified_name()?;
        let mut type_params = Vec::new();
        let mut multiplicity_params = Vec::new();
        self.parse_class_type_params(&mut type_params, &mut multiplicity_params)?;
        let type_variables = self.parse_type_variable_declarations()?;
        if self.dialect.refuses_lite_extensions() && !(type_params.is_empty() && multiplicity_params.is_empty()) {
            return Err(self.error_str("Type and/or multiplicity parameters are not authorized in Legend Engine"));
        }
        if self.peek() == T::VALID_STRING && eq(&self.safe_text(), "projects") {
            if self.dialect.refuses_platform_dialect() {
                return Err(self.error_str("class projections are not supported by the engine"));
            }
            self.advance()?;
            self.parse_qualified_name()?;
            if self.peek() == T::BRACE_OPEN {
                self.skip_balanced_block()?;
            }
            let (pkg, name) = split_fqn(&qualified_name)?;
            return Ok(PClass {
                pkg: JStr(pkg),
                name: JStr(name),
                type_params,
                type_variables,
                super_types: Vec::new(),
                properties: Vec::new(),
                derived_properties: Vec::new(),
                constraints: Vec::new(),
                stereotypes,
                tagged_values,
                is_native,
                source_information: self.span(class_start_tok, self.pos - 1),
                original_milestoned_properties: Vec::new(),
                multiplicity_params,
            });
        }
        let mut super_types = Vec::new();
        if self.match_tok(T::EXTENDS)? {
            loop {
                let st_tok = self.pos;
                let mut st = self.parse_type()?;
                let span = self.span(st_tok, self.pos - 1);
                if self.dialect.refuses_lite_extensions()
                    && let TypeExpression::TypeExpressionNameRef(n) = &st
                    && n.name.units().contains(&u16::from(b'~'))
                {
                    return Err(self.section_refusal(JStr::from("a unit is not a class to extend")));
                }
                if self.dialect.refuses_lite_extensions()
                    && let TypeExpression::TypeExpressionGeneric(g) = &st
                {
                    st = name_ref(g.name.clone(), span.clone());
                }
                super_types.push(Box::new(PSuperType {
                    r#type: st,
                    source_information: span,
                }));
                if !self.match_tok(T::COMMA)? {
                    break;
                }
            }
        }
        let constraints = if self.peek() == T::BRACKET_OPEN {
            self.parse_constraints()?
        } else {
            Vec::new()
        };
        self.expect(T::BRACE_OPEN)?;
        let mut properties = Vec::new();
        let mut derived_properties = Vec::new();
        while self.peek() != T::BRACE_CLOSE && !self.at_end() {
            if self.is_derived_property_start() {
                derived_properties.push(Box::new(self.parse_derived_property()?));
            } else {
                properties.push(Box::new(self.parse_property()?));
            }
        }
        self.expect(T::BRACE_CLOSE)?;
        let (pkg, name) = split_fqn(&qualified_name)?;
        Ok(PClass {
            pkg: JStr(pkg),
            name: JStr(name),
            type_params,
            type_variables,
            super_types,
            properties,
            derived_properties,
            constraints: constraints.into_iter().map(Box::new).collect(),
            stereotypes,
            tagged_values,
            is_native,
            source_information: self.span(class_start_tok, self.pos - 1),
            original_milestoned_properties: Vec::new(),
            multiplicity_params,
        })
    }

    /// `parseTypeVariableDeclarations`: `(x:Integer[1], ...)` after a Class or Primitive name, the platform's
    /// dialect only; each name its token's raw text.
    fn parse_type_variable_declarations(&mut self) -> R<Vec<Box<ParameterDefinition>>> {
        if self.peek() != T::PAREN_OPEN || self.dialect.refuses_platform_dialect() {
            return Ok(Vec::new());
        }
        self.advance()?;
        let mut out = Vec::new();
        while self.peek() != T::PAREN_CLOSE {
            let start = self.pos;
            let name = self.slack_text().to_vec();
            self.advance()?;
            self.expect(T::COLON)?;
            let r#type = self.parse_type()?;
            let multiplicity = self.parse_multiplicity()?;
            out.push(Box::new(ParameterDefinition {
                name: JStr(name),
                r#type,
                multiplicity,
                pos: self.span(start, self.pos - 1),
            }));
            if !self.match_tok(T::COMMA)? {
                break;
            }
        }
        self.expect(T::PAREN_CLOSE)?;
        Ok(out)
    }

    /// `parseClassTypeParams`: `<T, U>`, and `|m` multiplicity parameters at the platform's dialect only.
    fn parse_class_type_params(&mut self, params: &mut Vec<JStr>, mult: &mut Vec<JStr>) -> R<()> {
        if self.peek() != T::LESS_THAN {
            return Ok(());
        }
        self.advance()?;
        if self.peek() != T::PIPE {
            params.push(JStr(self.parse_type_param_name()?));
            while self.match_tok(T::COMMA)? {
                params.push(JStr(self.parse_type_param_name()?));
            }
        }
        if !self.dialect.refuses_platform_dialect() && self.match_tok(T::PIPE)? {
            mult.push(JStr(self.parse_identifier()?));
            while self.match_tok(T::COMMA)? {
                mult.push(JStr(self.parse_identifier()?));
            }
        }
        self.expect(T::GREATER_THAN)
    }

    /// `parseTypeParamName`: a variance marker (`-U`, `+V`) read past at the platform's dialect.
    fn parse_type_param_name(&mut self) -> R<Vec<u16>> {
        if !self.dialect.refuses_platform_dialect() && matches!(self.peek(), T::MINUS | T::PLUS) {
            self.advance()?;
        }
        self.parse_identifier()
    }

    /// `isDerivedPropertyStart`: `name(` past a documentation literal, stereotypes and tagged values (a raw scan).
    fn is_derived_property_start(&mut self) -> bool {
        let saved = self.pos;
        let n = self.tokens.count();
        if self.peek() == T::DOC_STRING {
            self.pos += 1;
        }
        if self.peek() == T::LESS_THAN && self.peek_at(1) == T::LESS_THAN {
            let mut depth = 2;
            self.pos += 2;
            while self.pos < n && depth > 0 {
                if self.peek() == T::GREATER_THAN {
                    depth -= 1;
                } else if self.peek() == T::LESS_THAN {
                    depth += 1;
                }
                self.pos += 1;
            }
        }
        if self.peek() == T::BRACE_OPEN {
            let mut depth = 1;
            self.pos += 1;
            while self.pos < n && depth > 0 {
                if self.peek() == T::BRACE_OPEN {
                    depth += 1;
                } else if self.peek() == T::BRACE_CLOSE {
                    depth -= 1;
                }
                self.pos += 1;
            }
        }
        let derived = self.at_identifier() && self.peek_at(1) == T::PAREN_OPEN;
        self.pos = saved;
        derived
    }

    /// `parseDerivedProperty`: `<<st>> {tags} name(params) { body }: Type[mult];`.
    /// The first token of a property's default at the engine's dialect, and of each element of an array of defaults, by
    /// its grammar's `defaultValueExpression`: a reference, `^new`, a literal or `[...]` (ruling 18 of 2026-11-09).
    fn refuse_a_default_the_engines_grammar_has_no_rule_for(&self) -> R<()> {
        if !self.dialect.refuses_lite_extensions() {
            return Ok(());
        }
        let starts = |t: TokenType| {
            is_identifier_token(t)
                || matches!(
                    t,
                    T::NEW_SYMBOL
                        | T::INTEGER
                        | T::FLOAT
                        | T::DECIMAL
                        | T::DATE
                        | T::STRICTTIME
                        | T::TRUE
                        | T::FALSE
                        | T::BOOLEAN
                        | T::DOC_STRING
                        | T::BRACKET_OPEN
                        | T::PATH_SEPARATOR
                )
        };
        let mut p = self.pos;
        let mut depth = 0i64;
        let mut at_element = true;
        while p < self.tokens.count() {
            let t = self.tokens.ty(p);
            if depth == 0 && t == T::SEMI_COLON {
                break;
            }
            if at_element && !starts(t) && !(depth > 0 && t == T::BRACKET_CLOSE) {
                return Err(ElementCursor::at(self.tokens, p, self.dialect).unexpected());
            }
            at_element = (t == T::BRACKET_OPEN && (depth == 0 || at_element)) || (t == T::COMMA && depth == 1);
            if t == T::BRACKET_OPEN {
                depth += 1;
            } else if t == T::BRACKET_CLOSE {
                depth -= 1;
            } else if depth == 0 {
                break;
            }
            p += 1;
        }
        Ok(())
    }

    /// Where a property's default stops at the engine's dialect, by its grammar's `defaultValueExpression`
    /// (`instanceReference propertyExpression | expressionInstance | instanceLiteralToken | [ ... ]`) and the `;` after it:
    /// a reference's required `.name`, what follows an element (the user's ruling 20 of 2026-11-10, docs/rust-port/PHASE2.md
    /// section 6, register S366). None where it reads to the `;`, or stops at the end of the input, which lite's own
    /// reading places.
    fn where_the_engines_default_stops(&self) -> Option<usize> {
        if !self.dialect.refuses_lite_extensions() {
            return None;
        }
        let ty = |p: usize| (p < self.tokens.count()).then(|| self.tokens.ty(p));
        // past the group that opens at `p` (its brackets balanced), or the stream's end
        let past_group = |mut p: usize| {
            let mut depth = 0i64;
            while let Some(t) = ty(p) {
                if opens(t) {
                    depth += 1;
                } else if closes(t) {
                    depth -= 1;
                    if depth == 0 {
                        return p + 1;
                    }
                }
                p += 1;
            }
            p
        };
        fn value(
            p: usize,
            ty: &dyn Fn(usize) -> Option<TokenType>,
            past_group: &dyn Fn(usize) -> usize,
        ) -> Result<usize, usize> {
            match ty(p) {
                Some(T::BRACKET_OPEN) => {
                    let mut q = p + 1;
                    if ty(q) != Some(T::BRACKET_CLOSE) {
                        loop {
                            q = value(q, ty, past_group)?;
                            if ty(q) != Some(T::COMMA) {
                                break;
                            }
                            q += 1;
                        }
                    }
                    if ty(q) != Some(T::BRACKET_CLOSE) {
                        return Err(q);
                    }
                    Ok(q + 1)
                }
                // `expressionInstance`: `^` a variable or a name, its `<...>` and a name, then its `(...)` (a first one
                // its type variable values when a second follows): the expression parser reads the inside
                Some(T::NEW_SYMBOL) => {
                    let mut q = p + 1;
                    if ty(q) == Some(T::DOLLAR) {
                        q += 2;
                    } else {
                        q += 1;
                        while ty(q) == Some(T::PATH_SEPARATOR) && ty(q + 1).is_some_and(is_identifier_token) {
                            q += 2;
                        }
                    }
                    if ty(q) == Some(T::LESS_THAN) {
                        let mut depth = 0i64;
                        loop {
                            match ty(q) {
                                None => return Err(q),
                                Some(T::LESS_THAN) => depth += 1,
                                Some(T::GREATER_THAN) => depth -= 1,
                                _ => {}
                            }
                            q += 1;
                            if depth == 0 {
                                break;
                            }
                        }
                    }
                    if ty(q).is_some_and(is_identifier_token) {
                        q += 1;
                    }
                    if ty(q) != Some(T::PAREN_OPEN) {
                        return Err(q);
                    }
                    q = past_group(q);
                    if ty(q) == Some(T::PAREN_OPEN) {
                        q = past_group(q);
                    }
                    Ok(q)
                }
                Some(
                    T::STRING
                    | T::INTEGER
                    | T::FLOAT
                    | T::DECIMAL
                    | T::DATE
                    | T::STRICTTIME
                    | T::TRUE
                    | T::FALSE
                    | T::BOOLEAN
                    | T::DOC_STRING,
                ) => Ok(p + 1),
                Some(t) if t == T::PATH_SEPARATOR || is_identifier_token(t) => {
                    // `instanceReference`: `::`, or a qualified name (a unit's `~name` after it), `allOrFunction?`
                    let mut q = p + 1;
                    if t != T::PATH_SEPARATOR {
                        while ty(q) == Some(T::PATH_SEPARATOR) && ty(q + 1).is_some_and(is_identifier_token) {
                            q += 2;
                        }
                        if ty(q) == Some(T::TILDE) && ty(q + 1).is_some_and(is_identifier_token) {
                            q += 2;
                        }
                    }
                    if ty(q) == Some(T::DOT)
                        && matches!(ty(q + 1), Some(T::ALL | T::ALL_VERSIONS | T::ALL_VERSIONS_IN_RANGE))
                        && ty(q + 2) == Some(T::PAREN_OPEN)
                    {
                        // `.all()` is the `allOrFunction` when a `.name` follows it, else the `.name` itself
                        let past = past_group(q + 2);
                        if ty(past) != Some(T::DOT) {
                            return Ok(past);
                        }
                        q = past;
                    } else if ty(q) == Some(T::PAREN_OPEN) {
                        q = past_group(q);
                    }
                    // `propertyExpression`, required: `.name` and its arguments, if any
                    if ty(q) != Some(T::DOT) {
                        return Err(q);
                    }
                    if !ty(q + 1).is_some_and(is_identifier_token) {
                        return Err(q + 1);
                    }
                    q += 2;
                    if ty(q) == Some(T::PAREN_OPEN) {
                        q = past_group(q);
                    }
                    Ok(q)
                }
                _ => Err(p),
            }
        }
        let stop = match value(self.pos, &ty, &past_group) {
            Ok(end) if ty(end) == Some(T::SEMI_COLON) => return None,
            Ok(end) => end,
            Err(at) => at,
        };
        (stop < self.tokens.count()).then_some(stop)
    }

    fn parse_derived_property(&mut self) -> R<DerivedPropertyDefinition> {
        let decl_start = self.pos;
        let doc = self.parse_documentation()?;
        let stereotypes = self.parse_stereotypes()?;
        let tvs = self.parse_tagged_values()?;
        let tagged_values = self.tagged_values_with_documentation(doc, tvs)?;
        let name = self.parse_identifier()?;
        self.expect(T::PAREN_OPEN)?;
        let mut parameters = Vec::new();
        if self.peek() != T::PAREN_CLOSE {
            parameters.push(Box::new(self.parse_parameter()?));
            while self.match_tok(T::COMMA)? {
                parameters.push(Box::new(self.parse_parameter()?));
            }
        }
        self.expect(T::PAREN_CLOSE)?;
        let mut body = Vec::new();
        if self.peek() == T::BRACE_OPEN {
            self.advance()?;
            let body_start = self.pos;
            let mut depth = 1;
            while !self.at_end() && depth > 0 {
                match self.peek() {
                    T::BRACE_OPEN => depth += 1,
                    T::BRACE_CLOSE => depth -= 1,
                    _ => {}
                }
                if depth > 0 {
                    self.advance()?;
                }
            }
            // `BRACE_OPEN codeBlock BRACE_CLOSE`, a code block being at least one line (ruling 18)
            if self.dialect.refuses_lite_extensions() && body_start == self.pos && !self.at_end() {
                return Err(self.unexpected());
            }
            body = self.slice_block(body_start, self.pos)?;
            self.expect(T::BRACE_CLOSE)?;
        }
        self.expect(T::COLON)?;
        let r#type = self.parse_type()?;
        let multiplicity = self.parse_multiplicity()?;
        self.expect(T::SEMI_COLON)?;
        Ok(DerivedPropertyDefinition {
            name: JStr(name),
            parameters,
            realization: realization_of(body),
            r#type,
            multiplicity,
            stereotypes,
            tagged_values,
            pos: self.span(decl_start, self.pos - 1),
        })
    }

    /// `parseDerivedPropertyParameter` / `parseFunctionParameter`: `name: Type[mult]`, its span the whole declaration.
    fn parse_parameter(&mut self) -> R<ParameterDefinition> {
        let p_start = self.pos;
        let name = self.parse_identifier()?;
        self.expect(T::COLON)?;
        let r#type = self.parse_type()?;
        let multiplicity = self.parse_multiplicity()?;
        Ok(ParameterDefinition {
            name: JStr(name),
            r#type,
            multiplicity,
            pos: self.span(p_start, self.pos - 1),
        })
    }

    // ================================================================ constraints

    /// `parseConstraints`: `[ constraint (, constraint)* ]`, at least one.
    fn parse_constraints(&mut self) -> R<Vec<ConstraintDefinition>> {
        self.expect(T::BRACKET_OPEN)?;
        if self.peek() == T::BRACKET_CLOSE {
            return Err(self.error_str("Unexpected token ']'"));
        }
        let mut result = vec![self.parse_constraint(0)?];
        while self.match_tok(T::COMMA)? {
            let index = result.len();
            result.push(self.parse_constraint(index)?);
        }
        self.expect(T::BRACKET_CLOSE)?;
        Ok(result)
    }

    /// The token index where a `~clause`'s value ends: the next top-level `~`, or a closer at depth 0.
    fn scan_clause_value(&mut self) -> R<()> {
        let mut d = 0i64;
        while !self.at_end() {
            let t = self.peek();
            if t == T::TILDE && d == 0 {
                break;
            }
            if opens(t) {
                d += 1;
            } else if closes(t) {
                if d == 0 {
                    break;
                }
                d -= 1;
            }
            self.advance()?;
        }
        Ok(())
    }

    /// `parseConstraint(index)`: `name: expr`, a bare `expr` (named by its position), or the extended
    /// `name( ~owner? ~externalId? ~function ~enforcementLevel? ~message? )`.
    fn parse_constraint(&mut self, index: usize) -> R<ConstraintDefinition> {
        let constraint_start = self.pos;
        let mut name = u(&index.to_string());
        if self.at_identifier() && self.peek_at(1) == T::PAREN_OPEN && self.peek_at(2) == T::TILDE {
            name = self.parse_identifier()?;
            self.expect(T::PAREN_OPEN)?;
            self.expect(T::TILDE)?;
            let mut kw = self.parse_identifier()?;
            let mut external_id = None;
            let mut owner = None;
            if eq(&kw, "owner") {
                self.expect(T::COLON)?;
                owner = Some(JStr(self.parse_identifier()?));
                self.expect(T::TILDE)?;
                kw = self.parse_identifier()?;
            }
            if eq(&kw, "externalId") {
                self.expect(T::COLON)?;
                external_id = Some(JStr(self.consume_string_literal("'externalId'")?));
                self.expect(T::TILDE)?;
                kw = self.parse_identifier()?;
            }
            if !eq(&kw, "function") {
                return Err(self.error(jcat!("extended constraint must lead with ~function:, got ~", kw)));
            }
            self.expect(T::COLON)?;
            let fn_start = self.pos;
            self.scan_clause_value()?;
            let function = self.slice_spec(fn_start, self.pos)?;
            let mut message = None;
            let mut level: Option<Vec<u16>> = None;
            let mut saw_level = false;
            let mut saw_message = false;
            while !self.at_end() && self.peek() == T::TILDE {
                self.expect(T::TILDE)?;
                let kw2 = self.parse_identifier()?;
                let refused = if eq(&kw2, "enforcementLevel") {
                    saw_level || saw_message
                } else {
                    !eq(&kw2, "message")
                };
                if refused {
                    return Err(self.error(jcat!(
                        "unexpected constraint clause '~",
                        kw2,
                        "' — the order is ~owner? ~externalId? ~function ~enforcementLevel? ~message?"
                    )));
                }
                if eq(&kw2, "message") && saw_message {
                    return Err(self.error_str("constraint clause '~message' may appear only once"));
                }
                self.expect(T::COLON)?;
                let v_start = self.pos;
                self.scan_clause_value()?;
                if eq(&kw2, "message") {
                    saw_message = true;
                    message = Some(self.slice_spec(v_start, self.pos)?);
                } else if eq(&kw2, "enforcementLevel") {
                    saw_level = true;
                    let lv = self.slice_spec(v_start, self.pos)?;
                    level = enforcement_level_name(&lv);
                    if !level.as_deref().is_some_and(|l| eq(l, "Error") || eq(l, "Warn")) {
                        let shown = level.clone().unwrap_or_else(|| u("null"));
                        return Err(self.error(jcat!(
                            "constraint enforcementLevel must be Error or Warn, got '",
                            shown,
                            "'"
                        )));
                    }
                }
            }
            self.expect(T::PAREN_CLOSE)?;
            return Ok(ConstraintDefinition {
                name: JStr(name),
                realization: realization_of(vec![function]),
                message,
                enforcement_level: level.map(JStr),
                external_id,
                owner,
                pos: self.span(constraint_start, self.pos - 1),
            });
        }
        if self.at_identifier() && self.peek_at(1) == T::COLON {
            name = self.parse_identifier()?;
            self.advance()?; // ':'
        }
        let body_start = self.pos;
        let mut depth = 0i64;
        while !self.at_end() {
            let t = self.peek();
            if opens(t) {
                depth += 1;
            } else if closes(t) {
                if depth == 0 {
                    break;
                }
                depth -= 1;
            } else if t == T::COMMA && depth == 0 {
                break;
            }
            self.advance()?;
        }
        if self.pos == body_start {
            return Err(self.error(jcat!("empty constraint expression for '", name, "'")));
        }
        let expression = self.slice_spec(body_start, self.pos)?;
        Ok(ConstraintDefinition {
            name: JStr(name),
            realization: realization_of(vec![expression]),
            message: None,
            enforcement_level: None,
            external_id: None,
            owner: None,
            pos: self.span(constraint_start, self.pos - 1),
        })
    }

    // ================================================================ Primitive, Association, Enum, Profile

    /// `parsePrimitiveProtocol`: `Primitive pkg::T(vars) extends Base(args)[constraints]` (lite's and the platform's
    /// dialects), the base's arguments and constraints dropped.
    pub fn parse_primitive_protocol(&mut self) -> R<PPrimitive> {
        if self.dialect.refuses_lite_extensions() {
            return Err(self.error_str("Primitive type declarations are not supported"));
        }
        let start = self.pos;
        self.advance()?; // 'Primitive'
        let fqn = self.parse_qualified_name()?;
        let type_variables = self.parse_type_variable_declarations()?;
        self.expect(T::EXTENDS)?;
        let base = self.parse_qualified_name()?;
        if self.peek() == T::PAREN_OPEN {
            self.skip_balanced_block()?;
        }
        if self.peek() == T::BRACKET_OPEN {
            self.skip_balanced_block()?;
        }
        Ok(PPrimitive {
            qualified_name: JStr(fqn),
            base_type_name: JStr(base),
            type_variables,
            source_information: self.span(start, self.pos - 1),
        })
    }

    /// `parseAssociationDefinition`: an association, or the platform's projection (its name alone).
    pub fn parse_association_definition(&mut self) -> R<Element> {
        let decl_start = self.pos;
        let doc = self.parse_documentation()?;
        self.expect(T::ASSOCIATION)?;
        let stereotypes = self.parse_stereotypes()?;
        let tvs = self.parse_tagged_values()?;
        let tagged_values = self.tagged_values_with_documentation(doc, tvs)?;
        let qualified_name = self.parse_qualified_name()?;
        if self.peek() == T::VALID_STRING && eq(&self.safe_text(), "projects") {
            if self.dialect.refuses_platform_dialect() {
                return Err(self.error_str(
                    "projection associations are a legend-lite-local form with no protocol shape (engine rejects \
                     'projects')",
                ));
            }
            self.advance()?;
            self.parse_qualified_name()?;
            if self.peek() == T::LESS_THAN {
                self.advance()?;
                let mut depth = 1;
                while !self.at_end() && depth > 0 {
                    if self.peek() == T::LESS_THAN {
                        depth += 1;
                    }
                    if self.peek() == T::GREATER_THAN {
                        depth -= 1;
                    }
                    self.advance()?;
                }
            }
            return Ok(Element::PAssociationProjection(Box::new(PAssociationProjection {
                qualified_name: JStr(qualified_name),
                source_information: self.span(decl_start, self.pos - 1),
            })));
        }
        self.expect(T::BRACE_OPEN)?;
        let mut properties = Vec::new();
        let mut derived_properties = Vec::new();
        while self.peek() != T::BRACE_CLOSE && !self.at_end() {
            if self.is_derived_property_start() {
                derived_properties.push(Box::new(self.parse_derived_property()?));
                continue;
            }
            properties.push(Box::new(self.parse_property()?));
        }
        self.expect(T::BRACE_CLOSE)?;
        let (pkg, name) = split_fqn(&qualified_name)?;
        Ok(Element::PAssociation(Box::new(PAssociation {
            pkg: JStr(pkg),
            name: JStr(name),
            properties,
            derived_properties,
            stereotypes,
            tagged_values,
            source_information: self.span(decl_start, self.pos - 1),
            original_milestoned_properties: Vec::new(),
        })))
    }

    /// `parseEnumDefinition`: `Enum <<st>> {tags} name { VAL (, VAL)* }`.
    pub fn parse_enum_definition(&mut self) -> R<PEnumeration> {
        let decl_start = self.pos;
        let doc = self.parse_documentation()?;
        self.expect(T::ENUM)?;
        let stereotypes = self.parse_stereotypes()?;
        let tvs = self.parse_tagged_values()?;
        let tagged_values = self.tagged_values_with_documentation(doc, tvs)?;
        let qualified_name = self.parse_qualified_name()?;
        self.expect(T::BRACE_OPEN)?;
        let mut values = Vec::new();
        if self.peek() != T::BRACE_CLOSE {
            values.push(Box::new(self.parse_enum_value()?));
            while self.match_tok(T::COMMA)? {
                values.push(Box::new(self.parse_enum_value()?));
            }
        }
        self.expect(T::BRACE_CLOSE)?;
        let (pkg, name) = split_fqn(&qualified_name)?;
        Ok(PEnumeration {
            pkg: JStr(pkg),
            name: JStr(name),
            values,
            stereotypes,
            tagged_values,
            source_information: self.span(decl_start, self.pos - 1),
        })
    }

    fn parse_enum_value(&mut self) -> R<PEnumValue> {
        let entry_start = self.pos;
        let doc = self.parse_documentation()?;
        let stereotypes = self.parse_stereotypes()?;
        let tvs = self.parse_tagged_values()?;
        let tagged_values = self.tagged_values_with_documentation(doc, tvs)?;
        let value = self.parse_identifier()?;
        Ok(PEnumValue {
            value: JStr(value),
            stereotypes,
            tagged_values,
            source_information: self.span(entry_start, self.pos - 1),
        })
    }

    /// `parseProfileDefinition`: `Profile name { stereotypes: [...]; tags: [...]; }`; the platform reads (and drops)
    /// documentation and the profile's own stereotypes and tags.
    pub fn parse_profile_definition(&mut self) -> R<PProfile> {
        let decl_start = self.pos;
        let doc = self.parse_documentation()?;
        if doc.is_some() && self.dialect.refuses_platform_dialect() {
            return Err(self.error(jcat!("Unexpected token '", self.safe_text(), "'")));
        }
        self.expect(T::PROFILE)?;
        let self_stereotypes = self.parse_stereotypes()?;
        let self_tags = self.parse_tagged_values()?;
        if self.dialect.refuses_lite_extensions() && (!self_stereotypes.is_empty() || !self_tags.is_empty()) {
            return Err(self.error_str("Profile stereotypes/tagged values are not authorized in Legend Engine"));
        }
        let qualified_name = self.parse_qualified_name()?;
        self.expect(T::BRACE_OPEN)?;
        let mut stereotypes = Vec::new();
        let mut tags = Vec::new();
        let mut seen_keys: HashSet<Vec<u16>> = HashSet::new();
        while self.peek() != T::BRACE_CLOSE && !self.at_end() {
            let to_stereotypes = if self.peek() == T::STEREOTYPES {
                crate::cursor::once(&mut seen_keys, &u("stereotypes"), self)?;
                true
            } else if self.peek() == T::TAGS {
                crate::cursor::once(&mut seen_keys, &u("tags"), self)?;
                false
            } else {
                return Err(self.error(jcat!(
                    "expected 'stereotypes' or 'tags' inside Profile, found ",
                    self.peek(),
                    " ('",
                    self.safe_text(),
                    "')"
                )));
            };
            self.advance()?;
            self.expect(T::COLON)?;
            self.expect(T::BRACKET_OPEN)?;
            let target = if to_stereotypes { &mut stereotypes } else { &mut tags };
            if self.peek() != T::BRACKET_CLOSE {
                target.push(Box::new(self.parse_profile_entry()?));
                while self.match_tok(T::COMMA)? {
                    target.push(Box::new(self.parse_profile_entry()?));
                }
            }
            self.expect(T::BRACKET_CLOSE)?;
            self.expect(T::SEMI_COLON)?;
        }
        self.expect(T::BRACE_CLOSE)?;
        let (pkg, name) = split_fqn(&qualified_name)?;
        Ok(PProfile {
            pkg: JStr(pkg),
            name: JStr(name),
            stereotypes,
            tags,
            source_information: self.span(decl_start, self.pos - 1),
        })
    }

    fn parse_profile_entry(&mut self) -> R<PProfileEntry> {
        let name_tok = self.pos;
        let value = self.parse_identifier()?;
        Ok(PProfileEntry {
            value: JStr(value),
            source_information: self.span(name_tok, self.pos - 1),
        })
    }

    // ================================================================ functions

    /// `parseFunctionSignature(declStart, doc)`: `function <<st>> {tags} name<T|m>(params): R[m]`.
    fn parse_function_signature(&mut self, decl_start: usize, doc: Option<Documentation>) -> R<FunctionSignature> {
        self.expect(T::FUNCTION)?;
        let stereotypes = self.parse_stereotypes()?;
        let tvs = self.parse_tagged_values()?;
        let tagged_values = self.tagged_values_with_documentation(doc, tvs)?;
        let qualified_name = self.parse_qualified_name()?;
        let mut type_params = Vec::new();
        let mut mult_params = Vec::new();
        self.parse_type_and_multiplicity_parameters(&mut type_params, &mut mult_params)?;
        if self.dialect.refuses_lite_extensions() && (!type_params.is_empty() || !mult_params.is_empty()) {
            return Err(self.error_str("Type and/or multiplicity parameters are not authorized in Legend Engine"));
        }
        self.expect(T::PAREN_OPEN)?;
        let mut params = Vec::new();
        if self.peek() != T::PAREN_CLOSE {
            params.push(Box::new(self.parse_parameter()?));
            while self.match_tok(T::COMMA)? {
                params.push(Box::new(self.parse_parameter()?));
            }
        }
        self.expect(T::PAREN_CLOSE)?;
        self.expect(T::COLON)?;
        let return_type = self.parse_type()?;
        let return_mult = self.parse_multiplicity()?;
        Ok(FunctionSignature {
            decl_start,
            qualified_name,
            type_params,
            mult_params,
            params,
            return_type,
            return_mult,
            stereotypes,
            tagged_values,
        })
    }

    /// `parseTypeAndMultiplicityParameters`: `<T,V|m,n>`, either side optional.
    fn parse_type_and_multiplicity_parameters(&mut self, types: &mut Vec<JStr>, mults: &mut Vec<JStr>) -> R<()> {
        if self.peek() != T::LESS_THAN {
            return Ok(());
        }
        self.advance()?;
        if self.peek() != T::PIPE {
            types.push(JStr(self.parse_identifier()?));
            while self.match_tok(T::COMMA)? {
                types.push(JStr(self.parse_identifier()?));
            }
        }
        if self.match_tok(T::PIPE)? {
            mults.push(JStr(self.parse_identifier()?));
            while self.match_tok(T::COMMA)? {
                mults.push(JStr(self.parse_identifier()?));
            }
        }
        self.expect(T::GREATER_THAN)
    }

    /// `parseMeasureDefinition`: `Measure name { *Canon: x -> $x; Other: x -> ...; }` or bare units, the first
    /// canonical.
    pub fn parse_measure_definition(&mut self) -> R<PMeasure> {
        let decl_start = self.pos;
        let doc = self.parse_documentation()?;
        if doc.is_some() && self.dialect.refuses_platform_dialect() {
            return Err(self.error(jcat!("Unexpected token '", self.safe_text(), "'")));
        }
        self.advance()?; // 'Measure'
        let qualified_name = self.parse_qualified_name()?;
        let measure_fqn = JStr(unquote_path_iae(&qualified_name)?);
        self.expect(T::BRACE_OPEN)?;
        let mut units: Vec<PUnit> = Vec::new();
        let mut starred_idx: Option<usize> = None;
        let mut convertible: Option<bool> = None;
        while !self.at_end() && self.peek() != T::BRACE_CLOSE {
            let mut starred = false;
            if self.peek() == T::STAR {
                starred = true;
                self.advance()?;
            }
            let unit_start = self.pos;
            let unit_name = self.parse_identifier()?;
            let has_conversion = starred || self.peek() == T::COLON;
            match convertible {
                None => convertible = Some(has_conversion),
                Some(c) if c != has_conversion => {
                    return Err(self
                        .error_str("measure units cannot mix conversion functions with bare (non-convertible) units"));
                }
                Some(_) => {}
            }
            let mut param = None;
            let mut body = None;
            if has_conversion {
                self.expect(T::COLON)?;
                param = Some(JStr(self.parse_identifier()?));
                self.expect(T::ARROW)?;
                let body_start = self.pos;
                let mut depth = 0i64;
                while !self.at_end() {
                    let t = self.peek();
                    if depth == 0 && t == T::SEMI_COLON {
                        break;
                    }
                    if opens(t) {
                        depth += 1;
                    } else if closes(t) {
                        depth -= 1;
                    }
                    self.advance()?;
                }
                body = Some(self.slice_spec(body_start, self.pos)?);
            }
            self.expect(T::SEMI_COLON)?;
            if starred {
                if starred_idx.is_some() {
                    return Err(self.error_str("measure declares more than one canonical ('*') unit"));
                }
                starred_idx = Some(units.len());
            }
            units.push(PUnit {
                name: JStr(unit_name),
                measure_fqn: measure_fqn.clone(),
                param_name: param,
                body,
                source_information: self.span(unit_start, self.pos - 1),
            });
        }
        if units.is_empty() {
            return Err(self.error_str("measure requires at least one unit"));
        }
        if convertible == Some(true) && starred_idx.is_none() {
            return Err(self.error_str("measure with conversion functions requires a canonical ('*') unit"));
        }
        self.expect(T::BRACE_CLOSE)?;
        let canonical = units.remove(starred_idx.unwrap_or(0));
        let (pkg, name) = split_fqn(&qualified_name)?;
        Ok(PMeasure {
            pkg: JStr(pkg),
            name: JStr(name),
            canonical_unit: Some(Box::new(canonical)),
            non_canonical_units: units.into_iter().map(Box::new).collect(),
            source_information: self.span(decl_start, self.pos - 1),
        })
    }

    /// `parseFunctionProtocol`: a function, its constraints parsed at every dialect and kept but at the engine's, its
    /// body a code block (at least one statement but at the platform's), its trailing test block.
    pub fn parse_function_protocol(&mut self) -> R<PFunction> {
        let decl_start = self.pos;
        let doc = self.parse_documentation()?;
        let sig = self.parse_function_signature(decl_start, doc)?;
        let mut constraints = if self.peek() == T::BRACKET_OPEN {
            self.parse_constraints()?
        } else {
            Vec::new()
        };
        if self.dialect.refuses_lite_extensions() {
            constraints = Vec::new();
        }
        self.expect(T::BRACE_OPEN)?;
        let body_start = self.pos;
        let mut depth = 1;
        while !self.at_end() && depth > 0 {
            match self.peek() {
                T::BRACE_OPEN => depth += 1,
                T::BRACE_CLOSE => depth -= 1,
                _ => {}
            }
            if depth > 0 {
                self.advance()?;
            }
        }
        let body = self.slice_block(body_start, self.pos)?;
        if body.is_empty() && self.dialect.refuses_platform_dialect() {
            return Err(self.error_str("Unexpected token '}'"));
        }
        self.expect(T::BRACE_CLOSE)?;
        let test_suites = if self.peek() == T::BRACE_OPEN {
            self.parse_function_test_block(&sig)?
        } else {
            Vec::new()
        };
        let mut pre = Vec::new();
        let mut post = Vec::new();
        for c in constraints {
            if mentions_return(&c) {
                post.push(Box::new(c));
            } else {
                pre.push(Box::new(c));
            }
        }
        let (pkg, name) = split_fqn(&sig.qualified_name)?;
        Ok(PFunction {
            pkg: JStr(pkg),
            name: JStr(name),
            type_params: sig.type_params,
            mult_params: sig.mult_params,
            parameters: sig.params,
            return_type: sig.return_type,
            return_multiplicity: sig.return_mult,
            body,
            pre_constraints: pre,
            post_constraints: post,
            test_suites,
            stereotypes: sig.stereotypes,
            tagged_values: sig.tagged_values,
            source_information: self.span(sig.decl_start, self.pos - 1),
        })
    }

    /// `parseNativeFunctionProtocol`: a `native function` signature and its `;` (`native` already consumed).
    fn parse_native_function_protocol(&mut self, decl_start: usize, doc: Option<Documentation>) -> R<PNativeFunction> {
        let sig = self.parse_function_signature(decl_start, doc)?;
        self.expect(T::SEMI_COLON)?;
        Ok(PNativeFunction {
            qualified_name: JStr(sig.qualified_name),
            type_params: sig.type_params,
            mult_params: sig.mult_params,
            parameters: sig.params,
            return_type: sig.return_type,
            return_multiplicity: sig.return_mult,
            stereotypes: sig.stereotypes,
            tagged_values: sig.tagged_values,
            source_information: self.span(decl_start, self.pos - 1),
        })
    }

    // ================================================================ function test blocks

    /// `parseFunctionTestBlock`: named suites `{ name ( ... ) }` and the unnamed form `{ test; ... }` (serialized
    /// first, its span the whole block).
    fn parse_function_test_block(&mut self, sig: &FunctionSignature) -> R<Vec<Box<PTestSuite>>> {
        let block_open = self.pos;
        self.expect(T::BRACE_OPEN)?;
        let mut suites = Vec::new();
        let mut unnamed = Vec::new();
        let mut unnamed_data = Vec::new();
        while !self.at_end() && self.peek() != T::BRACE_CLOSE {
            if self.peek_at(1) == T::PAREN_OPEN {
                let suite_start = self.pos;
                let suite_id = self.parse_identifier()?;
                self.expect(T::PAREN_OPEN)?;
                let mut data = Vec::new();
                let mut tests = Vec::new();
                while !self.at_end() && self.peek() != T::PAREN_CLOSE {
                    self.parse_suite_entry(sig, &mut data, &mut tests)?;
                }
                self.expect(T::PAREN_CLOSE)?;
                suites.push(Box::new(PTestSuite {
                    id: Some(JStr(suite_id)),
                    source_information: self.span(suite_start, self.pos - 1),
                    test_data: data,
                    tests,
                }));
            } else {
                self.parse_suite_entry(sig, &mut unnamed_data, &mut unnamed)?;
            }
        }
        self.expect(T::BRACE_CLOSE)?;
        if !unnamed.is_empty() || !unnamed_data.is_empty() {
            suites.insert(
                0,
                Box::new(PTestSuite {
                    id: None,
                    source_information: self.span(block_open, self.pos - 1),
                    test_data: unnamed_data,
                    tests: unnamed,
                }),
            );
        }
        Ok(suites)
    }

    /// `parseSuiteEntry`: `store: <payload>;` (test data) or `id | call(args) => expected;` (a test).
    fn parse_suite_entry(
        &mut self,
        sig: &FunctionSignature,
        data: &mut Vec<Box<PTestData>>,
        tests: &mut Vec<Box<PFunctionTest>>,
    ) -> R<()> {
        let entry_start = self.pos;
        let mut pointer_type = None;
        if self.peek() == T::PAREN_OPEN {
            self.advance()?;
            pointer_type = Some(java_case(&self.parse_identifier()?, true));
            self.expect(T::PAREN_CLOSE)?;
        }
        let head = self.parse_qualified_name()?;
        let head_end = self.pos - 1;
        if self.peek() == T::COLON {
            self.advance()?;
            let mut payload = self.parse_test_payload()?;
            self.expect(T::SEMI_COLON)?;
            if let PTestPayload::PTestPayloadRelationElements(re) = &mut payload {
                let s = re
                    .source_information
                    .as_deref()
                    .expect("a parsed node always carries its span");
                let end = self.pos - 1;
                re.source_information = boxed(SourceInfo {
                    source_id: JStr::default(),
                    start_line: s.start_line,
                    start_column: s.start_column,
                    end_line: self.tokens.end_line(end) as i32,
                    end_column: self.tokens.end_column(end) as i32 + 2,
                });
            }
            data.push(Box::new(PTestData {
                store_path: JStr(head),
                store_span: self.span(entry_start, head_end),
                data: payload,
                pointer_type: pointer_type.map(JStr),
                source_information: self.span(entry_start, self.pos - 1),
            }));
            return Ok(());
        }
        if let Some(pt) = pointer_type {
            return Err(self.error(jcat!(
                "a (",
                java_case(&pt, false),
                ") marker introduces test DATA, not a test"
            )));
        }
        tests.push(Box::new(self.parse_function_test(sig, entry_start, head)?));
        Ok(())
    }

    /// `parseTestPayload`: `(JSON) '...'`, `DataspaceTestData #{ ref }#`, `Relation #{...}#`, `ModelStore #{...}#`,
    /// `Relational #{...}#`, or a reference.
    fn parse_test_payload(&mut self) -> R<PTestPayload> {
        if self.peek() == T::PAREN_OPEN {
            let fmt_start = self.pos;
            self.advance()?;
            let fmt = self.parse_identifier()?;
            self.expect(T::PAREN_CLOSE)?;
            if self.peek() != T::STRING {
                return Err(self.error(jcat!("expected a quoted payload after (", fmt, ")")));
            }
            let raw = self.text();
            self.advance()?;
            let content_type = self.format_content_type(&fmt)?;
            let data = self.unquote_and_unescape(raw)?;
            return Ok(PTestPayload::PTestPayloadExternalFormat(Box::new(
                PTestPayloadExternalFormat {
                    content_type: JStr(content_type),
                    data: JStr(data),
                    source_information: self.span(fmt_start, self.pos - 1),
                },
            )));
        }
        if eq(self.peek_text(0), "DataspaceTestData") && self.peek_at(1) == T::ISLAND_OPEN {
            let kind_tok = self.pos;
            self.advance()?;
            self.advance()?;
            let emb_start = self.pos;
            while self.peek() != T::ISLAND_END && !self.at_end() {
                self.advance()?;
            }
            let ref_path = trim(&self.reconstruct_text(emb_start, self.pos)).to_vec();
            self.expect(T::ISLAND_END)?;
            return Ok(PTestPayload::PTestPayloadReference(Box::new(PTestPayloadReference {
                path: JStr(unquote_path_iae(&ref_path)?),
                ref_type: Some(JStr::from("DATASPACE")),
                source_information: self.span(kind_tok, self.pos - 1),
            })));
        }
        if eq(self.peek_text(0), "Relation") && self.peek_at(1) == T::ISLAND_OPEN {
            self.advance()?;
            let els = self.parse_relation_island_elements(false)?;
            let first = match els.first() {
                None => self.span(self.pos - 1, self.pos - 1),
                Some(e) => e.source_information.clone(),
            };
            return Ok(PTestPayload::PTestPayloadRelationElements(Box::new(
                PTestPayloadRelationElements {
                    elements: els,
                    source_information: first,
                },
            )));
        }
        if eq(self.peek_text(0), "ModelStore") && self.peek_at(1) == T::ISLAND_OPEN {
            return Ok(PTestPayload::PTestPayloadModelStoreData(Box::new(
                self.parse_model_store_island()?,
            )));
        }
        if eq(self.peek_text(0), "Relational") && self.peek_at(1) == T::ISLAND_OPEN {
            return Ok(PTestPayload::PTestPayloadRelationalCsv(Box::new(
                self.parse_relational_csv_island()?,
            )));
        }
        let ref_start = self.pos;
        let reference = self.parse_qualified_name()?;
        Ok(PTestPayload::PTestPayloadReference(Box::new(PTestPayloadReference {
            path: JStr(reference),
            ref_type: None,
            source_information: self.span(ref_start, self.pos - 1),
        })))
    }

    /// `formatContentType`: `JSON` and `XML`, the two probed formats.
    fn format_content_type(&self, fmt: &[u16]) -> R<Vec<u16>> {
        if eq(fmt, "JSON") {
            return Ok(u("application/json"));
        }
        if eq(fmt, "XML") {
            return Ok(u("application/xml"));
        }
        Err(self.error(jcat!("test payload format (", fmt, ") has no probed contentType")))
    }

    /// `parseModelStoreIsland`: `ModelStore #{ FQN: ExternalFormat #{ contentType: '...'; data: '...'; }# }#`, read
    /// from the island's coarse chunks.
    fn parse_model_store_island(&mut self) -> R<PTestPayloadModelStoreData> {
        let kw = self.pos;
        self.advance()?; // 'ModelStore'
        self.expect(T::ISLAND_OPEN)?;
        let mut models = Vec::new();
        while !self.at_end() && self.peek() != T::ISLAND_END {
            if self.peek() == T::ISLAND_OPEN {
                return Err(self.error_str("expected 'FQN: ExternalFormat' before a nested data island"));
            }
            let chunk = self.tokens.text(self.pos);
            let chunk_start = self.tokens.start(self.pos);
            let Some(colon) = single_colon(chunk) else {
                return Err(self.error_str("expected 'FQN:' inside a ModelStore data island"));
            };
            let mut a = 0;
            while a < colon && (is_whitespace(chunk[a]) || chunk[a] == u16::from(b',')) {
                a += 1;
            }
            let model = trim(&chunk[a..colon]).to_vec();
            let rest = trim(&chunk[colon + 1..]);
            if !eq(rest, "ExternalFormat") {
                return Err(self.error(jcat!(
                    "expected ExternalFormat inside a ModelStore data island, got '",
                    rest,
                    "'"
                )));
            }
            let ef_start = chunk_start + index_of_units(chunk, &u("ExternalFormat"), colon).expect("found above");
            self.advance()?;
            if self.peek() != T::ISLAND_OPEN && self.peek() != T::ISLAND_START {
                return Err(self.error_str("expected a nested data island after ExternalFormat"));
            }
            self.advance()?;
            let mut inner: Vec<u16> = Vec::new();
            let mut depth = 0;
            while !self.at_end() {
                let t = self.peek();
                if t == T::ISLAND_START {
                    depth += 1;
                } else if t == T::ISLAND_END {
                    if depth == 0 {
                        break;
                    }
                    depth -= 1;
                }
                inner.extend_from_slice(self.tokens.text(self.pos));
                self.advance()?;
            }
            let inner_end_tok = self.pos;
            self.expect(T::ISLAND_END)?;
            let (content_type, data) = self.parse_external_format_kv(&inner)?;
            let ts = self.tokens;
            models.push(Box::new(PTestPayloadModelEmbedded {
                model: JStr(model),
                data: Box::new(PTestPayloadExternalFormat {
                    content_type: JStr(content_type),
                    data: JStr(data),
                    source_information: boxed(SourceInfo {
                        source_id: JStr::default(),
                        start_line: ts.line_of(ef_start) as i32,
                        start_column: ts.column_of(ef_start) as i32,
                        end_line: ts.end_line(inner_end_tok) as i32,
                        end_column: ts.end_column(inner_end_tok) as i32,
                    }),
                }),
                source_information: boxed(SourceInfo {
                    source_id: JStr::default(),
                    start_line: ts.line_of(chunk_start + a) as i32,
                    start_column: ts.column_of(chunk_start + a) as i32,
                    end_line: ts.end_line(inner_end_tok) as i32,
                    end_column: ts.end_column(inner_end_tok) as i32,
                }),
            }));
            if !self.at_end() && self.peek() != T::ISLAND_END && strip(self.tokens.text(self.pos)).is_empty() {
                self.advance()?;
            }
        }
        self.expect(T::ISLAND_END)?;
        Ok(PTestPayloadModelStoreData {
            model_data: models,
            source_information: self.span(kw, self.pos - 1),
        })
    }

    /// `parseExternalFormatKv`: `contentType: '...'; data: '...';`, quote-aware, THE shared escape decoder.
    fn parse_external_format_kv(&self, body: &[u16]) -> R<(Vec<u16>, Vec<u16>)> {
        let (q, bs, colon_c) = (u16::from(b'\''), u16::from(b'\\'), u16::from(b':'));
        let mut content_type = None;
        let mut data = None;
        let n = body.len();
        let mut i = 0;
        while i < n {
            let c = body[i];
            if is_whitespace(c) || c == u16::from(b';') {
                i += 1;
                continue;
            }
            let Some(colon) = index_of(body, colon_c, i) else {
                break;
            };
            let key = trim(&body[i..colon]).to_vec();
            let Some(q1) = index_of(body, q, colon) else {
                return Err(self.error(jcat!("expected a quoted value for ExternalFormat key '", key, "'")));
            };
            let mut q2 = q1 + 1;
            while q2 < n && body[q2] != q {
                q2 += if body[q2] == bs && q2 + 1 < n { 2 } else { 1 };
            }
            let v = unescape_java_like(&body[q1 + 1..q2.min(n)])
                .map_err(|m| crate::cursor::malformed_escape("string literal", m))?;
            if eq(&key, "contentType") {
                content_type = Some(v);
            } else if eq(&key, "data") {
                data = Some(v);
            } else {
                return Err(self.error(jcat!("unknown ExternalFormat key '", key, "'")));
            }
            i = q2 + 1;
        }
        match (content_type, data) {
            (Some(c), Some(d)) => Ok((c, d)),
            _ => Err(self.error_str("ExternalFormat needs contentType and data")),
        }
    }

    /// `skipIslandContent`: from just past an `ISLAND_OPEN` to its own `ISLAND_END`, nested pairs balanced.
    fn skip_island_content(&mut self) -> R<()> {
        let mut depth = 0;
        while !self.at_end() {
            let t = self.peek();
            if t == T::ISLAND_START {
                depth += 1;
            } else if t == T::ISLAND_END {
                if depth == 0 {
                    return Ok(());
                }
                depth -= 1;
            }
            self.advance()?;
        }
        Ok(())
    }

    /// `parseRelationalCsvIsland`: `Relational #{ schema.table: 'csv' + 'csv'; }#`, read from the source text.
    fn parse_relational_csv_island(&mut self) -> R<PTestPayloadRelationalCsv> {
        let kw = self.pos;
        self.advance()?; // 'Relational'
        let open_tok = self.pos;
        self.expect(T::ISLAND_OPEN)?;
        self.skip_island_content()?;
        let end_tok = self.pos;
        self.expect(T::ISLAND_END)?;
        let ts = self.tokens;
        let source = ts.source();
        let (from, to) = (ts.end(open_tok), ts.start(end_tok));
        let (q, bs) = (u16::from(b'\''), u16::from(b'\\'));
        let mut tables = Vec::new();
        let mut i = from;
        while i < to {
            let c = source[i];
            if is_whitespace(c) || c == u16::from(b';') {
                i += 1;
                continue;
            }
            let t_start = i;
            let colon = index_of(source, u16::from(b':'), i);
            let Some(colon) = colon.filter(|&c| c < to) else {
                return Err(self.error_str("expected 'schema.table:' inside a Relational data island"));
            };
            let head = trim(&source[i..colon]);
            let Some(dot) = index_of(head, u16::from(b'.'), 0) else {
                return Err(self.error(jcat!(
                    "expected 'schema.table:' inside a Relational data island, got '",
                    head,
                    "'"
                )));
            };
            let schema = trim(&head[..dot]).to_vec();
            let table = trim(&head[dot + 1..]).to_vec();
            i = colon + 1;
            let mut values: Vec<u16> = Vec::new();
            let mut val_end = colon;
            while i < to {
                let v = source[i];
                if is_whitespace(v) || v == u16::from(b'+') {
                    i += 1;
                    continue;
                }
                if v == u16::from(b';') {
                    val_end = i;
                    i += 1;
                    break;
                }
                if v != q {
                    return Err(self.error_str("expected a quoted CSV chunk inside a Relational data island"));
                }
                let start = i + 1;
                let mut close = start;
                while close < to && source[close] != q {
                    close += if source[close] == bs && close + 1 < to { 2 } else { 1 };
                }
                values.extend(
                    unescape_java_like(&source[start..close.min(source.len())])
                        .map_err(|m| crate::cursor::malformed_escape("string literal", m))?,
                );
                val_end = close;
                i = close + 1;
            }
            tables.push(Box::new(PTestPayloadCsvTable {
                schema: JStr(schema),
                table: JStr(table),
                values: Some(JStr(values)),
                source_information: boxed(SourceInfo {
                    source_id: JStr::default(),
                    start_line: ts.line_of(t_start) as i32,
                    start_column: ts.column_of(t_start) as i32,
                    end_line: ts.line_of(val_end) as i32,
                    end_column: ts.column_of(val_end) as i32,
                }),
            }));
        }
        Ok(PTestPayloadRelationalCsv {
            tables,
            source_information: self.span(kw, self.pos - 1),
        })
    }

    /// `parseFunctionTest`: `id 'doc'? | call(args) => expected;` -- the call's name checked against the function's,
    /// each argument bound to the parameter at its position.
    fn parse_function_test(
        &mut self,
        sig: &FunctionSignature,
        test_start: usize,
        test_id: Vec<u16>,
    ) -> R<PFunctionTest> {
        let mut doc = None;
        if self.peek() == T::STRING {
            let raw = self.consume(T::STRING)?;
            doc = Some(JStr(self.unquote_and_unescape(raw)?));
        }
        self.expect(T::PIPE)?;
        let call_name = self.parse_qualified_name()?;
        let simple = |n: &[u16]| match last_index_of_str(n, "::") {
            Some(i) => n[i + 2..].to_vec(),
            None => n.to_vec(),
        };
        let simple_call = simple(&call_name);
        if simple_call != simple(&sig.qualified_name) {
            return Err(self.error(jcat!(
                "Function name in test '",
                simple_call,
                "' does not match the enclosing function"
            )));
        }
        self.expect(T::PAREN_OPEN)?;
        let mut params = Vec::new();
        while !self.at_end() && self.peek() != T::PAREN_CLOSE {
            if self.peek() == T::COMMA {
                self.advance()?;
                continue;
            }
            let v_start = self.pos;
            let mut depth = 0i64;
            while !self.at_end() {
                let t = self.peek();
                if depth == 0 && (t == T::COMMA || t == T::PAREN_CLOSE) {
                    break;
                }
                if opens(t) {
                    depth += 1;
                } else if closes(t) {
                    depth -= 1;
                }
                self.advance()?;
            }
            let idx = params.len();
            let name = sig.params.get(idx).map(|p| p.name.clone());
            let value = self.slice_spec(v_start, self.pos)?;
            params.push(Box::new(PTestParam {
                name,
                value,
                source_information: self.span(v_start, self.pos - 1),
            }));
        }
        self.expect(T::PAREN_CLOSE)?;
        self.expect(T::EQUAL)?;
        self.expect(T::GREATER_THAN)?;
        let assertion = if self.peek() == T::PAREN_OPEN {
            let PTestPayload::PTestPayloadExternalFormat(fmt) = self.parse_test_payload()? else {
                unreachable!("a parenthesized payload is an external format")
            };
            let span = fmt.source_information.clone();
            PAssertion::PAssertionEqualToJson(Box::new(PAssertionEqualToJson { expected: fmt, span }))
        } else if self.peek_at(1) == T::ISLAND_OPEN
            && !eq(self.peek_text(0), "Relation")
            && self.peek() == T::VALID_STRING
        {
            return Err(self.error(jcat!(
                "Unsupported embedded data type for function test assertion: ",
                self.peek_text(0),
                ". Only 'Relation' is supported."
            )));
        } else if eq(self.peek_text(0), "Relation") && self.peek_at(1) == T::ISLAND_OPEN {
            let rel_start = self.pos;
            self.advance()?;
            let mut els = self.parse_relation_island_elements(true)?;
            if els.len() != 1 {
                return Err(self.error(jcat!(
                    "an equalTo Relation assertion needs exactly one block, got ",
                    els.len()
                )));
            }
            PAssertion::PAssertionEqualToRelation(Box::new(PAssertionEqualToRelation {
                expected: els.remove(0),
                span: self.span(rel_start, self.pos - 1),
            }))
        } else {
            let e_start = self.pos;
            let mut depth = 0i64;
            while !self.at_end() {
                let t = self.peek();
                if depth == 0 && t == T::SEMI_COLON {
                    break;
                }
                if opens(t) {
                    depth += 1;
                } else if closes(t) {
                    depth -= 1;
                }
                self.advance()?;
            }
            let expected = self.slice_spec(e_start, self.pos)?;
            PAssertion::PAssertionEqualTo(Box::new(PAssertionEqualTo {
                expected,
                span: self.span(e_start, self.pos - 1),
            }))
        };
        self.expect(T::SEMI_COLON)?;
        Ok(PFunctionTest {
            id: JStr(test_id),
            source_information: self.span(test_start, self.pos - 1),
            parameters: params,
            assertion: Some(assertion),
            doc,
        })
    }

    /// `parseRelationIslandElements(assertionSpans)`: a relation island's `;`-terminated blocks, read from the source
    /// text; in assertion position every block's span is the engine's reparse artifact (`islandReparseSpan`).
    fn parse_relation_island_elements(&mut self, assertion_spans: bool) -> R<Vec<Box<PTestPayloadRelationElement>>> {
        let open_tok = self.pos;
        self.expect(T::ISLAND_OPEN)?;
        self.skip_island_content()?;
        let end_tok = self.pos;
        self.expect(T::ISLAND_END)?;
        let source = self.tokens.source();
        let (from, to) = (self.tokens.end(open_tok), self.tokens.start(end_tok));
        // the engine's `RelationElementsData` grammar over the island, as the engine's walk reads it (the user's ruling 20 of
        // 2026-11-10, docs/rust-port/PHASE2.md section 6; register S359): an assertion's reparsed at the `#{`, test data's
        // where it stands; behind a later syntax error of the section
        if self.dialect.holds_to_the_engines_walk() {
            if assertion_spans {
                if let Some(refused) = crate::mapping_parser::relation_reparse_refusal(self.tokens, open_tok, from, to)
                {
                    return Err(refused);
                }
            } else if let Some(at) = crate::mapping_parser::relation_elements_stop(source, from, to) {
                return Err(crate::doors::JavaError::parse(
                    JStr::from("Unexpected token"),
                    self.tokens.line_of(at) as i32,
                    self.tokens.column_of(at) as i32,
                )
                .as_refusal());
            }
        }
        let assert_span = if assertion_spans {
            Some(self.island_reparse_span(source, from, to, open_tok))
        } else {
            None
        };
        // the reading for a later syntax error passes the island by, as the engine's section parse takes it whole
        // (ruling 20): one block of the whole text, which nothing reads
        if self.dialect.reads_for_syntax() {
            return Ok(vec![Box::new(self.relation_block(
                source,
                from,
                to,
                assert_span.as_ref(),
            ))]);
        }
        let mut out = Vec::new();
        let mut block_start = from;
        let mut in_quotes = false;
        for i in from..to {
            let c = source[i];
            if c == u16::from(b'"') {
                in_quotes = !in_quotes;
                continue;
            }
            if c != u16::from(b';') || in_quotes {
                continue;
            }
            let mut a = block_start;
            while a < i && is_whitespace(source[a]) {
                a += 1;
            }
            if a < i {
                out.push(Box::new(self.relation_block(source, a, i, assert_span.as_ref())));
            }
            block_start = i + 1;
        }
        Ok(out)
    }

    /// `islandReparseSpan`: where `": " + trim(content)` lands when the engine reparses it at the `#{`.
    fn island_reparse_span(&self, source: &[u16], from: usize, to: usize, open_tok: usize) -> SourceInfo {
        let mut t0 = from;
        while t0 < to && is_whitespace(source[t0]) {
            t0 += 1;
        }
        let mut t1 = to as isize - 1;
        while t1 >= t0 as isize && is_whitespace(source[t1 as usize]) {
            t1 -= 1;
        }
        let line = self.tokens.start_line(open_tok) as i32;
        let col = self.tokens.start_column(open_tok) as i32;
        let mut new_lines = 0;
        let mut last_nl: isize = -1;
        let mut i = t0 as isize;
        while i <= t1 {
            if source[i as usize] == u16::from(b'\n') {
                new_lines += 1;
                last_nl = i;
            }
            i += 1;
        }
        let e_line = if new_lines == 0 { line } else { line + new_lines };
        let e_col = if new_lines == 0 {
            (t1 - t0 as isize + 1) as i32 + col + 3
        } else {
            (t1 - last_nl) as i32
        };
        SourceInfo {
            source_id: JStr::default(),
            start_line: line,
            start_column: col + 2,
            end_line: e_line,
            end_column: e_col,
        }
    }

    /// `relationBlock`: one block over `source[a, semi]`, its span including the `;` (or the assertion span).
    fn relation_block(
        &self,
        source: &[u16],
        a: usize,
        semi: usize,
        assert_span: Option<&SourceInfo>,
    ) -> PTestPayloadRelationElement {
        let mut body = &source[a..semi];
        let mut paths = Vec::new();
        if let Some(colon) = path_colon_of(body) {
            let path_part = trim(&body[..colon]);
            let mut seg_start = 0;
            while seg_start <= path_part.len() {
                let dot = index_of(path_part, u16::from(b'.'), seg_start);
                paths.push(JStr(
                    trim(&path_part[seg_start..dot.unwrap_or(path_part.len())]).to_vec(),
                ));
                match dot {
                    None => break,
                    Some(d) => seg_start = d + 1,
                }
            }
            body = &body[colon + 1..];
        }
        let mut columns = Vec::new();
        let mut rows = Vec::new();
        let mut first = true;
        for line in lines_of(body) {
            if strip(line).is_empty() {
                continue;
            }
            let cells = csv_cells(line);
            if first {
                columns.extend(cells);
                first = false;
            } else {
                rows.push(cells);
            }
        }
        let ts = self.tokens;
        let span = match assert_span {
            Some(s) => s.clone(),
            None => SourceInfo {
                source_id: JStr::default(),
                start_line: ts.line_of(a) as i32,
                start_column: ts.column_of(a) as i32,
                end_line: ts.line_of(semi) as i32,
                end_column: ts.column_of(semi) as i32,
            },
        };
        PTestPayloadRelationElement {
            columns,
            paths,
            rows,
            source_information: boxed(span),
        }
    }

    // ================================================================ properties

    /// `parseProperty`: `<<st>> {tags} (aggregation)? name: Type[mult] (= default)?;`.
    fn parse_property(&mut self) -> R<PProperty> {
        let start_tok = self.pos;
        let doc = self.parse_documentation()?;
        let stereotypes = self.parse_stereotypes()?;
        let tvs = self.parse_tagged_values()?;
        let tagged_values = self.tagged_values_with_documentation(doc, tvs)?;
        let mut aggregation = None;
        if self.peek() == T::PAREN_OPEN {
            self.advance()?;
            let kind = self.parse_identifier()?;
            if !(eq(&kind, "composite") || eq(&kind, "shared") || eq(&kind, "none")) {
                return Err(self.error(jcat!("unknown aggregation kind '(", kind, ")'")));
            }
            aggregation = Some(JStr(java_case(&kind, true)));
            self.expect(T::PAREN_CLOSE)?;
        }
        let name = self.parse_identifier()?;
        self.expect(T::COLON)?;
        let r#type = self.parse_type()?;
        let multiplicity = self.parse_multiplicity()?;
        let mut default_value = None;
        if self.match_tok(T::EQUAL)? {
            let def_start = self.pos;
            self.refuse_a_default_the_engines_grammar_has_no_rule_for()?;
            let engines_stop = self.where_the_engines_default_stops();
            let mut depth = 0i64;
            while !self.at_end() {
                let t = self.peek();
                if depth == 0 && t == T::SEMI_COLON {
                    break;
                }
                if opens(t) {
                    depth += 1;
                } else if closes(t) {
                    depth -= 1;
                }
                self.advance()?;
            }
            let value = self.slice_spec(def_start, self.pos)?;
            // what lite's own reading takes and the engine's grammar does not, refused where that grammar stops
            if let Some(stop) = engines_stop {
                return Err(ElementCursor::at(self.tokens, stop, self.dialect).unexpected());
            }
            if matches!(value, VS::LambdaFunction(_)) {
                return Err(self.error_str("a lambda is not a legal property default value"));
            }
            let last = self.pos - 1;
            let def_span: Span = if self.tokens.ty(last) == T::DOC_STRING {
                let d = self.doc_string_span(last);
                boxed(SourceInfo {
                    source_id: self.span_source_id(),
                    start_line: self.tokens.start_line(def_start) as i32,
                    start_column: self.tokens.start_column(def_start) as i32,
                    end_line: d.end_line,
                    end_column: d.end_column,
                })
            } else {
                self.span(def_start, last)
            };
            default_value = Some(Box::new(PDefaultValue {
                value: Some(value),
                source_information: def_span,
            }));
        }
        self.expect(T::SEMI_COLON)?;
        Ok(PProperty {
            name: JStr(name),
            r#type,
            multiplicity,
            stereotypes,
            tagged_values,
            source_information: self.span(start_tok, self.pos - 1),
            default_value,
            aggregation,
        })
    }
}

/// `enforcementLevelName`: the bare level name of a parsed `~enforcementLevel` value; `None` (Java's null) for any
/// other shape.
fn enforcement_level_name(lv: &ValueSpecification) -> Option<Vec<u16>> {
    match lv {
        VS::PackageableElementPtr(p) => {
            let f = &p.full_path.0;
            Some(match last_index_of_str(f, "::") {
                Some(i) => f[i + 2..].to_vec(),
                None => f.clone(),
            })
        }
        VS::CString(cs) => Some(cs.value.0.clone()),
        VS::AppliedProperty(ap) => Some(ap.property.0.clone()),
        VS::Variable(v) => Some(v.name.0.clone()),
        _ => None,
    }
}

/// `singleColon`: the first `:` that is not half of a `::`.
fn single_colon(s: &[u16]) -> Option<usize> {
    let c = u16::from(b':');
    (0..s.len()).find(|&i| s[i] == c && (i + 1 >= s.len() || s[i + 1] != c) && (i == 0 || s[i - 1] != c))
}

/// `pathColonOf`: a relation block's path separator -- an unquoted `:` whose prefix is one dotted name (no newline,
/// comma or quote before it, not blank).
fn path_colon_of(body: &[u16]) -> Option<usize> {
    let mut in_quotes = false;
    for i in 0..body.len() {
        let c = body[i];
        if c == u16::from(b'"') {
            in_quotes = !in_quotes;
        } else if c == u16::from(b':') && !in_quotes {
            let prefix = &body[..i];
            if prefix.iter().any(|&pc| {
                pc == u16::from(b'\n') || pc == u16::from(b',') || pc == u16::from(b'"') || pc == u16::from(b'\'')
            }) {
                return None;
            }
            return if strip(prefix).is_empty() { None } else { Some(i) };
        }
    }
    None
}

/// `csvCells`: a quote-aware cell split, each cell trimmed, its quotes kept.
fn csv_cells(line: &[u16]) -> Vec<JStr> {
    let mut cells = Vec::new();
    let mut q = false;
    let mut start = 0;
    for i in 0..=line.len() {
        let c = if i < line.len() { line[i] } else { u16::from(b',') };
        if c == u16::from(b'"') {
            q = !q;
        } else if c == u16::from(b',') && !q {
            cells.push(JStr(trim(&line[start..i]).to_vec()));
            start = i + 1;
        }
    }
    cells
}

/// `linesOf`: `String.lines()` by hand -- LF, CRLF or a lone CR, no trailing empty line.
pub fn lines_of(text: &[u16]) -> Vec<&[u16]> {
    let (nl, cr) = (u16::from(b'\n'), u16::from(b'\r'));
    let mut out = Vec::new();
    let mut start = 0;
    let mut i = 0;
    while i < text.len() {
        let c = text[i];
        if c != nl && c != cr {
            i += 1;
            continue;
        }
        out.push(&text[start..i]);
        if c == cr && i + 1 < text.len() && text[i + 1] == nl {
            i += 1;
        }
        start = i + 1;
        i += 1;
    }
    if start < text.len() {
        out.push(&text[start..]);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ustr::u;

    // Held here since the Java parser was deleted (step 6b, 2026-11-06): the Java tests of its text helpers, each
    // case as they had it (rust/RETIRED_TESTS.md).

    #[test]
    fn a_data_cell_s_colon_is_not_a_path_separator() {
        // TextSurgeryPinTest.dataCellColonIsNotAPathSeparator, the text-surgery audit's §1.1 #2: indexOf(':') found a
        // colon anywhere -- a cell containing 10:30 garbled the path, the columns and the rows. A path colon heads the
        // block as one dotted name.
        assert_eq!(path_colon_of(&u("a,b\n1,10:30")), None);
        assert_eq!(path_colon_of(&u("time\n10:30")), None);
        assert_eq!(path_colon_of(&u("\"x:y\",b\nv,w")), None);
        assert_eq!(
            path_colon_of(&u("  default.T:\n a,b\n 1,2")),
            Some("default.T".len() + 2)
        );
        assert_eq!(path_colon_of(&u("a,b\n1,2")), None);
    }

    #[test]
    fn lines_of_is_string_lines() {
        // ElementParserTest.linesOfIsStringLines: the hand-rolled split is String.lines() -- a line ends at LF, CRLF
        // or a lone CR, and a last line needs a character -- over the shapes that break line splitters (the
        // expected values are Java's String.lines())
        let cases: &[(&str, &[&str])] = &[
            ("", &[]),
            ("\n", &[""]),
            ("\r", &[""]),
            ("\r\n", &[""]),
            ("a", &["a"]),
            ("a\n", &["a"]),
            ("a\r\n", &["a"]),
            ("a\r", &["a"]),
            ("a\nb", &["a", "b"]),
            ("a\r\nb", &["a", "b"]),
            ("a\rb", &["a", "b"]),
            ("a\n\nb", &["a", "", "b"]),
            ("a\r\n\r\nb", &["a", "", "b"]),
            ("a\nb\n", &["a", "b"]),
            ("a\nb\n\n", &["a", "b", ""]),
            ("\na", &["", "a"]),
            ("\r\na", &["", "a"]),
            ("\n\na", &["", "", "a"]),
            ("x,y\r\n1,2\r\n3,4\r\n", &["x,y", "1,2", "3,4"]),
            ("x,y\n1,2\n3,4", &["x,y", "1,2", "3,4"]),
            ("trailing spaces  \n  leading", &["trailing spaces  ", "  leading"]),
            ("\u{e9}\n\u{fc}", &["\u{e9}", "\u{fc}"]),
            ("a\n\rb", &["a", "", "b"]),
            ("a\r\rb", &["a", "", "b"]),
            ("no terminator at all", &["no terminator at all"]),
        ];
        for (text, lines) in cases {
            let units = u(text);
            let got: Vec<String> = lines_of(&units).into_iter().map(String::from_utf16_lossy).collect();
            assert_eq!(got, *lines, "lines_of({text:?})");
        }
    }
}
