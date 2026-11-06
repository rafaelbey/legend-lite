// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! `###Service` (core/'s `parser.section.ServiceSectionGrammar`, docs/rust-port/PHASE2.md §4 step 4c): `Service` and
//! `ExecutionEnvironment` elements -- the decorated envelope, pattern, title, owners and ownership, the MCP server,
//! documentation, auto-activation, post-validations, the Single and Multi executions (a query to its top-level `;`, a
//! mapping, a runtime pointer or an embedded runtime island read by the Runtime grammar re-lexed), the legacy `test:`
//! block (its parameter values in the engine's primitive-value grammar, signed literals folded) and the test suites
//! (the keyed form and 4.138's compact one, their data and assertions read by the Data and Mapping grammars at the
//! host's position). The platform dialect's headerless `Service` comes here too (`PmcdParser`'s site 15).
//!
//! Java's answers reproduced, quirks included: a `Service` admits no documentation literal; a duplicate key is refused
//! after its `:`, at the element; a query or an assertion that is no lambda is wrapped as a parameterless one with no
//! span; folding a signed literal rebuilds every node above it as Java's `withChildren` does (a lambda's and a
//! property access's span dropped, a column spec's decorations dropped).

// The generated records hold their record-typed list elements boxed (`Vec<Box<T>>`), so the readers build that.
#![allow(clippy::vec_box)]

use crate::cursor::{
    Cursor, ElementCursor, R, is_identifier_token, start_line_checked, throw_at, ty_or_slack, unquote_path_iae,
};
use crate::doors::JavaError;
use crate::jcat;
use crate::jnum::{parse_double, parse_long};
use crate::jtypes::{JStr, JValue, base64_of_utf8};
use crate::lexer::TokenType;
use crate::mapping_parser::MappingCursor;
use crate::nodes::{Span, applied, boxed, vs_fn};
use crate::protocol::qualified_name;
use crate::records::{
    CByteArray, CFloat, CInteger, ColSpec, ColumnInstance, Element, EnumValue, GraphFetchLiteralNode, LambdaFunction,
    PEmbeddedDataValue, PEmbeddedRuntime, PExecutionEnvironment, PExecutionParameters, PKeyedExecution,
    PLegacyServiceTest, PLegacyServiceTestPKeyedLegacyTest, PLegacyServiceTestPLegacyAssert, PMultiExecution,
    PMultiKeyedExecution, PPointer, PPostValidation, PPostValidationAssertion, PRuntimeComponents, PService,
    PServiceExecution, PServiceTestSuite, PServiceTestSuitePResolverData, PServiceTestSuitePSuiteConnData,
    PServiceTestSuitePSuiteData, PServiceTestSuitePSuiteParam, PServiceTestSuitePSuiteTest, PSingleExecution,
    PTestAssertion, SourceInfo, ValueSpecification,
};
use crate::runtime_parser::parse_embedded_body;
use crate::sections::{as_lambda, boolean_value, scan_island, split_last, string_value};
use crate::spec_parser::parse_tokens;
use crate::ustr::{eq, u};
use std::collections::HashSet;

use TokenType as T;
use ValueSpecification as VS;

fn opens(t: TokenType) -> bool {
    matches!(t, T::PAREN_OPEN | T::BRACE_OPEN | T::BRACKET_OPEN)
}

fn closes(t: TokenType) -> bool {
    matches!(t, T::PAREN_CLOSE | T::BRACE_CLOSE | T::BRACKET_CLOSE)
}

fn key_name(key: &[u16]) -> String {
    String::from_utf16_lossy(key)
}

fn string<'s>(c: &mut impl Cursor<'s>) -> R<JStr> {
    Ok(JStr(string_value(c)?))
}

/// `SpecParser.parse(tokens.slice(from, to), dialect)`.
fn slice_spec<'s>(c: &impl Cursor<'s>, from: usize, to: usize) -> R<ValueSpecification> {
    parse_tokens(&c.tokens().slice(from, to), c.dialect())
}

/// `ServiceSectionGrammar.parseOne` / `parseElement`: a `Service` or an `ExecutionEnvironment`, and the path the
/// section index lists it under.
pub fn parse_element(c: &mut ElementCursor<'_, '_>) -> R<(Element, Vec<u16>)> {
    if c.peek() == T::SERVICE {
        let s = parse_service(c)?;
        let path = qualified_name(&s.pkg.0, &s.name.0);
        return Ok((Element::PService(Box::new(s)), path));
    }
    if is_identifier_token(c.peek()) && eq(&c.safe_text(), "ExecutionEnvironment") {
        let e = parse_execution_environment(c)?;
        let path = qualified_name(&e.pkg.0, &e.name.0);
        return Ok((Element::PExecutionEnvironment(Box::new(e)), path));
    }
    Err(c.error(jcat!("unsupported ###Service element: ", c.safe_text())))
}

/// `isPlainIdentifier`: `[a-zA-Z_][a-zA-Z0-9_]*` (Java's `Character.isLetter(h) && h < 128`, and `isLetterOrDigit`).
fn is_plain_identifier(s: &[u16]) -> bool {
    let ascii = |c: u16| (c < 128).then_some(c as u8);
    let Some(&h) = s.first() else { return false };
    if !ascii(h).is_some_and(|b| b.is_ascii_alphabetic()) && h != u16::from(b'_') {
        return false;
    }
    s[1..].iter().all(|&ch| ascii(ch).is_some_and(|b| b.is_ascii_alphanumeric() || b == b'_'))
}

/// `parseService`: `Service <<...>> {...} qn { key: value; ... }` -- `execution`, `pattern` and `documentation`
/// required, each key once (refused at the element).
fn parse_service<'s>(c: &mut impl Cursor<'s>) -> R<PService> {
    let decl_start = c.pos();
    c.expect(T::SERVICE)?;
    let dec = c.parse_decorations()?;
    let qn = unquote_path_iae(&c.parse_qualified_name()?)?;
    let (pkg, name) = split_last(&qn);
    c.expect(T::BRACE_OPEN)?;
    let mut pattern = None;
    let mut title = None;
    let mut owners = Vec::new();
    let mut ownership_kind = None;
    let mut ownership_id = None;
    let mut ownership_users = None;
    let mut mcp_server = None;
    let mut documentation = None;
    let mut auto_activate = false;
    let mut execution = None;
    let mut test = None;
    let mut test_suites = None;
    let mut post_validations = None;
    let mut seen: HashSet<Vec<u16>> = HashSet::new();
    while !c.at_end() && c.peek() != T::BRACE_CLOSE {
        let key = c.parse_identifier()?;
        c.expect(T::COLON)?;
        if !seen.insert(key.clone()) {
            return Err(throw_at(c.tokens(), decl_start, jcat!("Field '", key, "' should be specified only once")));
        }
        match key_name(&key).as_str() {
            "pattern" => {
                pattern = Some(string(c)?);
                c.expect(T::SEMI_COLON)?;
            }
            "title" => {
                title = Some(string(c)?);
                c.expect(T::SEMI_COLON)?;
            }
            "ownership" => {
                // `DID { identifier: '...' }` or `UserList { users: [...] }`: the kind pairs with its key
                let kind = c.parse_identifier()?;
                let did = eq(&kind, "DID");
                if !did && !eq(&kind, "UserList") {
                    return Err(c.error(jcat!("unknown ownership kind: ", kind)));
                }
                ownership_kind = Some(JStr(kind));
                c.expect(T::BRACE_OPEN)?;
                let ik = c.parse_identifier()?;
                c.expect(T::COLON)?;
                if eq(&ik, "identifier") && did {
                    ownership_id = Some(string(c)?);
                } else if eq(&ik, "users") && !did {
                    let mut users = Vec::new();
                    c.expect(T::BRACKET_OPEN)?;
                    while c.peek() != T::BRACKET_CLOSE && !c.at_end() {
                        users.push(string(c)?);
                        c.match_tok(T::COMMA)?;
                    }
                    c.expect(T::BRACKET_CLOSE)?;
                    ownership_users = Some(users);
                } else {
                    return Err(c.error(jcat!("unknown ownership key: ", ik)));
                }
                c.match_tok(T::SEMI_COLON)?;
                c.expect(T::BRACE_CLOSE)?;
                c.expect(T::SEMI_COLON)?;
            }
            "mcpServer" => {
                let m = c.parse_identifier()?;
                if !is_plain_identifier(&m) {
                    return Err(c.error_str("Service mcpServer should be a valid identifier ([a-zA-Z_][a-zA-Z0-9_]*)"));
                }
                mcp_server = Some(JStr(m));
                c.expect(T::SEMI_COLON)?;
            }
            "postValidations" => {
                let mut list = Vec::new();
                c.expect(T::BRACKET_OPEN)?;
                while c.peek() != T::BRACKET_CLOSE && !c.at_end() {
                    list.push(Box::new(parse_post_validation(c)?));
                    c.match_tok(T::COMMA)?;
                }
                c.expect(T::BRACKET_CLOSE)?;
                c.match_tok(T::SEMI_COLON)?;
                post_validations = Some(list);
            }
            "documentation" => {
                documentation = Some(string(c)?);
                c.expect(T::SEMI_COLON)?;
            }
            "autoActivateUpdates" => {
                auto_activate = boolean_value(c)?;
                c.expect(T::SEMI_COLON)?;
            }
            "owners" => {
                c.expect(T::BRACKET_OPEN)?;
                while c.peek() != T::BRACKET_CLOSE && !c.at_end() {
                    owners.push(string(c)?);
                    c.match_tok(T::COMMA)?;
                }
                c.expect(T::BRACKET_CLOSE)?;
                c.expect(T::SEMI_COLON)?;
            }
            "execution" => execution = Some(parse_execution(c, &qn)?),
            "test" => test = Some(Box::new(parse_legacy_test(c)?)),
            "testSuites" => {
                test_suites = Some(parse_test_suites(c)?);
                c.match_tok(T::SEMI_COLON)?;
            }
            _ => return Err(c.error(jcat!("unknown key '", key, "' inside Service '", qn, "'"))),
        }
    }
    c.expect(T::BRACE_CLOSE)?;
    let Some(execution) = execution else {
        return Err(throw_at(c.tokens(), decl_start, JStr::from("Field 'execution' is required")));
    };
    if pattern.is_none() {
        return Err(throw_at(c.tokens(), decl_start, JStr::from("Field 'pattern' is required")));
    }
    if documentation.is_none() {
        return Err(throw_at(c.tokens(), decl_start, JStr::from("Field 'documentation' is required")));
    }
    Ok(PService {
        pkg: JStr(pkg),
        name: JStr(name),
        stereotypes: dec.stereotypes,
        tagged_values: dec.tagged_values,
        pattern,
        title,
        owners,
        ownership_kind,
        ownership_id,
        ownership_users,
        mcp_server,
        documentation,
        auto_activate_updates: auto_activate,
        execution,
        test,
        test_suites,
        post_validations,
        source_information: c.span(decl_start, c.pos() - 1),
    })
}

/// `byteArrayParam`: `name = toBytes('...')`, the byte-array literal (its string's UTF-8 in Base64) spanning the call;
/// `None` when the value is no `toBytes` literal.
fn byte_array_param<'s>(c: &mut impl Cursor<'s>, name: &[u16]) -> R<Option<PServiceTestSuitePSuiteParam>> {
    if !(c.peek() == T::VALID_STRING && eq(&c.safe_text(), "toBytes")) {
        return Ok(None);
    }
    let vs = c.pos();
    c.advance()?;
    c.expect(T::PAREN_OPEN)?;
    let quoted = c.slack_text();
    c.expect(T::STRING)?;
    let decoded = c.unquote_and_unescape(quoted)?;
    c.expect(T::PAREN_CLOSE)?;
    let value = VS::CByteArray(Box::new(CByteArray {
        value: JStr::from(base64_of_utf8(&decoded)),
        pos: c.span(vs, c.pos() - 1),
    }));
    Ok(Some(PServiceTestSuitePSuiteParam { name: JStr(name.to_vec()), value }))
}

/// `legacyParamValue(c, from)`: one legacy-test parameter value, the tokens `[from, cursor)`, in the engine's
/// primitive-value grammar -- `-NUMBER` a negative literal, `path.ident` an enum value node, `list(...)` the list
/// call spanning the whole call; anything else the expression, its signed literals folded.
fn legacy_param_value<'s>(c: &impl Cursor<'s>, from: usize) -> R<ValueSpecification> {
    let ts = c.tokens();
    let pos = c.pos();
    let n = pos - from;
    if n == 2 && ts.ty(from) == T::MINUS {
        let lt = ts.ty(from + 1);
        let txt = ts.text(from + 1);
        let span = c.span(from, from + 1);
        let negative = jcat!("-", txt);
        if lt == T::INTEGER {
            let v = parse_long(&negative.0).map_err(|m| JavaError::java("java.lang.NumberFormatException", m))?;
            return Ok(VS::CInteger(Box::new(CInteger { value: JValue::Long(v), pos: span })));
        }
        if lt == T::FLOAT {
            let v = parse_double(&negative.0).map_err(|m| JavaError::java("java.lang.NumberFormatException", m))?;
            return Ok(VS::CFloat(Box::new(CFloat { value: v, exact: None, pos: span })));
        }
    }
    let dot = (from..pos).rev().find(|&i| ts.ty(i) == T::DOT);
    if let Some(dot) = dot
        && dot > from
        && dot == pos - 2
        && is_identifier_token(ts.ty(pos - 1))
        && (from..dot).all(|i| is_identifier_token(ts.ty(i)) || ts.ty(i) == T::PATH_SEPARATOR)
    {
        let mut path = Vec::new();
        for i in from..dot {
            path.extend_from_slice(ts.text(i));
        }
        return Ok(VS::EnumValue(Box::new(EnumValue {
            full_path: JStr(unquote_path_iae(&path)?),
            value: JStr::from(ts.text(pos - 1)),
            enumeration_pos: None,
            pos: c.span(from, pos - 1),
            node: true,
        })));
    }
    let from_text: &[u16] = if from < ts.count() { ts.text(from) } else { &[] };
    if is_identifier_token(ty_or_slack(ts, from))
        && eq(from_text, "list")
        && from + 1 < pos
        && ts.ty(from + 1) == T::PAREN_OPEN
        && ts.ty(pos - 1) == T::PAREN_CLOSE
    {
        let inner = slice_spec(c, from + 2, pos - 1)?;
        let call = applied(JStr::from("list"), vec![fold_signed_literals(inner)?.0], c.span(from, pos - 1));
        return Ok(vs_fn(call));
    }
    Ok(fold_signed_literals(slice_spec(c, from, pos)?)?.0)
}

/// `signedSpan(minus, lit)`: the folded literal's span, the `-` through the digits.
fn signed_span(minus: &Span, lit: &Span) -> Span {
    match (minus, lit) {
        (Some(m), Some(l)) => boxed(SourceInfo {
            source_id: m.source_id.clone(),
            start_line: m.start_line,
            start_column: m.start_column,
            end_line: l.end_line,
            end_column: l.end_column,
        }),
        _ => minus.clone(),
    }
}

/// `-ci.value().longValue()`: a `BigInteger`'s low 64 bits, negated with Java's wrap.
fn negated_long(v: &JValue) -> i64 {
    match v {
        JValue::Long(l) => l.wrapping_neg(),
        JValue::Integer(i) => i64::from(*i).wrapping_neg(),
        JValue::BigInteger(digits) => {
            let (negative, mag) = match digits.strip_prefix('-') {
                Some(m) => (true, m),
                None => (false, digits.as_str()),
            };
            let low = mag.bytes().fold(0u64, |acc, d| acc.wrapping_mul(10).wrapping_add(u64::from(d - b'0')));
            let long_value = if negative { (low as i64).wrapping_neg() } else { low as i64 };
            long_value.wrapping_neg()
        }
        other => unreachable!("an integer literal holds a Long or a BigInteger, not {other:?}"),
    }
}

/// A rebuilt node's class name, for the `ClassCastException` Java's `withChildren` raises when a folded child is no
/// longer the class its slot casts to.
fn class_cast(child: &ValueSpecification, to: &str) -> JavaError {
    let from = match child {
        VS::CInteger(_) => "CInteger",
        VS::CFloat(_) => "CFloat",
        _ => "ValueSpecification",
    };
    JavaError::java(
        "java.lang.ClassCastException",
        jcat!(
            "class com.legend.protocol.spec.",
            from,
            " cannot be cast to class com.legend.protocol.spec.",
            to,
            " (com.legend.protocol.spec.",
            from,
            " and com.legend.protocol.spec.",
            to,
            " are in unnamed module of loader 'app')"
        ),
    )
}

/// Folds a child that Java's `withChildren` casts to `AppliedFunction`.
fn fold_applied(af: crate::records::AppliedFunction) -> R<(crate::records::AppliedFunction, bool)> {
    match fold_signed_literals(VS::AppliedFunction(Box::new(af)))? {
        (VS::AppliedFunction(f), changed) => Ok((*f, changed)),
        (other, _) => Err(class_cast(&other, "AppliedFunction")),
    }
}

/// Folds a lambda child (its own `withChildren`: a changed body drops the span).
fn fold_lambda(mut lf: LambdaFunction) -> R<(LambdaFunction, bool)> {
    let changed = fold_all(&mut lf.body)?;
    if changed {
        lf.pos = None;
    }
    Ok((lf, changed))
}

/// Folds each value in place; whether any changed.
fn fold_all(values: &mut [ValueSpecification]) -> R<bool> {
    let mut changed = false;
    for v in values.iter_mut() {
        let old = std::mem::replace(v, VS::CByteArray(Box::new(CByteArray { value: JStr::default(), pos: None })));
        let (f, c) = fold_signed_literals(old)?;
        *v = f;
        changed |= c;
    }
    Ok(changed)
}

fn fold_nodes(nodes: &mut [Box<GraphFetchLiteralNode>]) -> R<bool> {
    let mut changed = false;
    for n in nodes.iter_mut() {
        changed |= fold_all(&mut n.parameters)?;
        changed |= fold_nodes(&mut n.sub_trees)?;
    }
    Ok(changed)
}

/// A column spec rebuilt by `withChildren` (its nine-argument constructor: the decorations dropped).
fn fold_col_spec(mut cs: ColSpec) -> R<(ColSpec, bool)> {
    let mut changed = false;
    if let Some(f) = cs.function1.take() {
        let (f, c) = fold_lambda(*f)?;
        cs.function1 = Some(Box::new(f));
        changed |= c;
    }
    if let Some(f) = cs.function2.take() {
        let (f, c) = fold_lambda(*f)?;
        cs.function2 = Some(Box::new(f));
        changed |= c;
    }
    changed |= fold_all(&mut cs.args)?;
    if changed {
        cs.stereotypes = Vec::new();
        cs.tagged_values = Vec::new();
    }
    Ok((cs, changed))
}

/// `foldSignedLiterals`: unary minus over an integer or float literal is the negative literal (its span the `-`
/// through the digits), at every depth; a node above a folded one rebuilt by Java's `withChildren` (which this
/// mirrors per variant, its losses included), every other node kept. The flag: whether anything changed.
fn fold_signed_literals(v: ValueSpecification) -> R<(ValueSpecification, bool)> {
    Ok(match v {
        VS::AppliedFunction(mut af) => {
            if eq(&af.function.0, "minus") && af.parameters.len() == 1 {
                match &af.parameters[0] {
                    VS::CInteger(ci) => {
                        let value = JValue::Long(negated_long(&ci.value));
                        let pos = signed_span(&af.pos, &ci.pos);
                        return Ok((VS::CInteger(Box::new(CInteger { value, pos })), true));
                    }
                    VS::CFloat(cf) => {
                        let pos = signed_span(&af.pos, &cf.pos);
                        return Ok((VS::CFloat(Box::new(CFloat { value: -cf.value, exact: None, pos })), true));
                    }
                    _ => {}
                }
            }
            let changed = fold_all(&mut af.parameters)?;
            (VS::AppliedFunction(af), changed)
        }
        VS::AppliedProperty(mut ap) => {
            let receiver = std::mem::replace(
                &mut ap.receiver,
                VS::CByteArray(Box::new(CByteArray { value: JStr::default(), pos: None })),
            );
            let (r, changed) = fold_signed_literals(receiver)?;
            ap.receiver = r;
            if changed {
                ap.pos = None;
            }
            (VS::AppliedProperty(ap), changed)
        }
        VS::LambdaFunction(lf) => {
            let (lf, changed) = fold_lambda(*lf)?;
            (VS::LambdaFunction(Box::new(lf)), changed)
        }
        VS::PathLiteral(mut pl) => {
            let (d, changed) = fold_lambda(*pl.desugared)?;
            pl.desugared = Box::new(d);
            (VS::PathLiteral(pl), changed)
        }
        VS::GraphFetchLiteral(mut gf) => {
            let mut changed = fold_nodes(&mut gf.sub_trees)?;
            for st in gf.sub_type_trees.iter_mut() {
                changed |= fold_nodes(&mut st.sub_trees)?;
            }
            (VS::GraphFetchLiteral(gf), changed)
        }
        VS::QuotedTreeCall(mut q) => {
            let (o, c1) = fold_applied(*q.original)?;
            q.original = Box::new(o);
            let tree = std::mem::replace(
                &mut q.tree,
                VS::CByteArray(Box::new(CByteArray { value: JStr::default(), pos: None })),
            );
            let (t, c2) = fold_signed_literals(tree)?;
            q.tree = t;
            (VS::QuotedTreeCall(q), c1 || c2)
        }
        VS::QuotedGrammarCall(mut q) => {
            let (o, mut changed) = fold_applied(*q.original)?;
            q.original = Box::new(o);
            let mut functions = Vec::with_capacity(q.functions.len());
            for f in std::mem::take(&mut q.functions) {
                let (f, c) = fold_lambda(*f)?;
                changed |= c;
                functions.push(Box::new(f));
            }
            q.functions = functions;
            (VS::QuotedGrammarCall(q), changed)
        }
        VS::TdsLiteral(mut tl) => {
            let (d, changed) = fold_applied(*tl.desugared)?;
            tl.desugared = Box::new(d);
            (VS::TdsLiteral(tl), changed)
        }
        VS::NewInstance(mut ni) => {
            let mut changed = false;
            for kb in ni.properties.iter_mut() {
                let value = std::mem::replace(
                    &mut kb.expression.value,
                    VS::CByteArray(Box::new(CByteArray { value: JStr::default(), pos: None })),
                );
                let (f, c) = fold_signed_literals(value)?;
                kb.expression.value = f;
                changed |= c;
            }
            (VS::NewInstance(ni), changed)
        }
        VS::NewInstanceCast(mut nc) => {
            let src = std::mem::replace(
                &mut nc.src,
                VS::CByteArray(Box::new(CByteArray { value: JStr::default(), pos: None })),
            );
            let (s, changed) = fold_signed_literals(src)?;
            nc.src = s;
            (VS::NewInstanceCast(nc), changed)
        }
        VS::PureCollection(mut pc) => {
            let changed = fold_all(&mut pc.values)?;
            (VS::PureCollection(pc), changed)
        }
        VS::ColumnInstance(ColumnInstance::ColSpec(cs)) => {
            let (cs, changed) = fold_col_spec(*cs)?;
            (VS::ColumnInstance(ColumnInstance::ColSpec(Box::new(cs))), changed)
        }
        VS::ColumnInstance(ColumnInstance::ColSpecArray(mut ca)) => {
            let mut changed = false;
            let mut specs = Vec::with_capacity(ca.col_specs.len());
            for cs in std::mem::take(&mut ca.col_specs) {
                let (cs, c) = fold_col_spec(*cs)?;
                changed |= c;
                specs.push(Box::new(cs));
            }
            ca.col_specs = specs;
            if changed {
                // the array rebuilt by its position-free constructor
                ca.pos = None;
            }
            (VS::ColumnInstance(ColumnInstance::ColSpecArray(ca)), changed)
        }
        leaf @ (VS::CBoolean(_)
        | VS::CByteArray(_)
        | VS::CDate(_)
        | VS::CDecimal(_)
        | VS::CFloat(_)
        | VS::CInteger(_)
        | VS::CLatestDate(_)
        | VS::CString(_)
        | VS::CTime(_)
        | VS::EnumValue(_)
        | VS::PackageableElementPtr(_)
        | VS::Variable(_)
        | VS::TypeAnnotation(_)
        | VS::SqlIsland(_)
        | VS::GqlIsland(_)) => (leaf, false),
    })
}

/// A depth-aware value scan's end (the parameter lists'): the first `,` or `]` (`stop_close`) at depth 0, the opening
/// and closing brackets counted after the test.
fn scan_param<'s>(c: &mut impl Cursor<'s>, stops: &[TokenType]) -> R<()> {
    let mut d = 0i64;
    while !c.at_end() {
        let tk = c.peek();
        if d == 0 && stops.contains(&tk) {
            break;
        }
        if opens(tk) {
            d += 1;
        } else if closes(tk) {
            d -= 1;
        }
        c.advance()?;
    }
    Ok(())
}

/// `parsePostValidation`: `{ description: '..'; params: [..]; assertions: [ id: <lambda>; ... ]; }`, all three
/// required; an assertion's span (and its lambda's end) through its `;`.
fn parse_post_validation<'s>(c: &mut impl Cursor<'s>) -> R<PPostValidation> {
    let e_s = c.pos();
    c.expect(T::BRACE_OPEN)?;
    let mut description = None;
    let mut params = Vec::new();
    let mut assertions = Vec::new();
    let mut seen: HashSet<Vec<u16>> = HashSet::new();
    while !c.at_end() && c.peek() != T::BRACE_CLOSE {
        let key = c.parse_identifier()?;
        c.expect(T::COLON)?;
        if !seen.insert(key.clone()) {
            return Err(c.error(jcat!("Field '", key, "' should be specified only once")));
        }
        match key_name(&key).as_str() {
            "description" => {
                description = Some(string(c)?);
                c.expect(T::SEMI_COLON)?;
            }
            "params" => {
                c.expect(T::BRACKET_OPEN)?;
                while c.peek() != T::BRACKET_CLOSE && !c.at_end() {
                    let p_s = c.pos();
                    scan_param(c, &[T::COMMA, T::BRACKET_CLOSE])?;
                    params.push(slice_spec(c, p_s, c.pos())?);
                    c.match_tok(T::COMMA)?;
                }
                c.expect(T::BRACKET_CLOSE)?;
                c.expect(T::SEMI_COLON)?;
            }
            "assertions" => {
                c.expect(T::BRACKET_OPEN)?;
                while c.peek() != T::BRACKET_CLOSE && !c.at_end() {
                    let a_s = c.pos();
                    let id = c.parse_identifier()?;
                    c.expect(T::COLON)?;
                    let ls = c.pos();
                    // an assertion ends at the list's ',' or ']', or at a ';', which is the lambda's own
                    scan_param(c, &[T::SEMI_COLON, T::BRACKET_CLOSE, T::COMMA])?;
                    c.match_tok(T::SEMI_COLON)?;
                    let mut lambda = slice_spec(c, ls, c.pos())?;
                    let extended = c.span_of(a_s, c.pos() - 1);
                    if let VS::LambdaFunction(lf) = &mut lambda
                        && let Some(lp) = lf.pos.as_mut()
                    {
                        lp.end_line = extended.end_line;
                        lp.end_column = extended.end_column;
                    }
                    assertions.push(Box::new(PPostValidationAssertion {
                        id: JStr(id),
                        assertion: lambda,
                        source_information: boxed(extended),
                    }));
                    c.match_tok(T::COMMA)?;
                }
                c.expect(T::BRACKET_CLOSE)?;
                c.expect(T::SEMI_COLON)?;
            }
            _ => return Err(c.error(jcat!("unknown postValidation key: ", key))),
        }
    }
    let close = c.pos();
    c.expect(T::BRACE_CLOSE)?;
    let Some(description) = description else {
        return Err(throw_at(c.tokens(), e_s, JStr::from("Field 'description' is required")));
    };
    if !seen.contains(&u("params")) {
        return Err(throw_at(c.tokens(), e_s, JStr::from("Field 'params' is required")));
    }
    if !seen.contains(&u("assertions")) {
        return Err(throw_at(c.tokens(), e_s, JStr::from("Field 'assertions' is required")));
    }
    Ok(PPostValidation { description, parameters: params, assertions, source_information: c.span(e_s, close) })
}

/// `parseLegacyBody`: a legacy test's `data:` and `asserts: [ { [params], <assertion> }, ... ];`, a duplicate refused
/// at the entry; the data, if spelled.
fn parse_legacy_body<'s>(
    c: &mut impl Cursor<'s>,
    asserts: &mut Vec<Box<PLegacyServiceTestPLegacyAssert>>,
    entry_anchor: usize,
) -> R<Option<JStr>> {
    let mut data = None;
    let mut seen: HashSet<Vec<u16>> = HashSet::new();
    while !c.at_end() && c.peek() != T::BRACE_CLOSE {
        let key = c.parse_identifier()?;
        c.expect(T::COLON)?;
        if !seen.insert(key.clone()) {
            return Err(throw_at(c.tokens(), entry_anchor, jcat!("Field '", key, "' should be specified only once")));
        }
        if eq(&key, "data") {
            data = Some(string(c)?);
            c.expect(T::SEMI_COLON)?;
        } else if eq(&key, "asserts") {
            c.expect(T::BRACKET_OPEN)?;
            while c.peek() != T::BRACKET_CLOSE {
                let a_s = c.pos();
                c.expect(T::BRACE_OPEN)?;
                c.expect(T::BRACKET_OPEN)?;
                let mut param_values = Vec::new();
                while c.peek() != T::BRACKET_CLOSE && !c.at_end() {
                    let pv_s = c.pos();
                    scan_param(c, &[T::COMMA, T::BRACKET_CLOSE])?;
                    param_values.push(legacy_param_value(c, pv_s)?);
                    c.match_tok(T::COMMA)?;
                }
                c.expect(T::BRACKET_CLOSE)?;
                c.expect(T::COMMA)?;
                let ls = c.pos();
                let mut d = 0i64;
                while !c.at_end() {
                    let tk = c.peek();
                    if opens(tk) {
                        d += 1;
                    } else if closes(tk) {
                        d -= 1;
                    }
                    if tk == T::BRACE_CLOSE && d < 0 {
                        break;
                    }
                    c.advance()?;
                }
                let assertion = as_lambda(slice_spec(c, ls, c.pos())?);
                c.expect(T::BRACE_CLOSE)?;
                asserts.push(Box::new(PLegacyServiceTestPLegacyAssert {
                    parameters_values: param_values,
                    assertion,
                    source_information: c.span(a_s, c.pos() - 1),
                }));
                if !c.match_tok(T::COMMA)? {
                    break;
                }
            }
            c.expect(T::BRACKET_CLOSE)?;
            c.expect(T::SEMI_COLON)?;
        } else {
            return Err(c.error(jcat!("unknown legacy test key '", key, "'")));
        }
    }
    Ok(data)
}

/// `parseTestSuites`: `[ id: { data?; tests }, ... ]` or 4.138's compact `id 'doc'? ( ... )`.
fn parse_test_suites<'s>(c: &mut impl Cursor<'s>) -> R<Vec<Box<PServiceTestSuite>>> {
    let mut out = Vec::new();
    c.expect(T::BRACKET_OPEN)?;
    while !c.at_end() && c.peek() != T::BRACKET_CLOSE {
        let ss = c.pos();
        let id = c.parse_identifier()?;
        if c.peek() == T::STRING || c.peek() == T::PAREN_OPEN {
            out.push(Box::new(parse_compact_suite(c, ss, id)?));
            c.match_tok(T::COMMA)?;
            continue;
        }
        c.expect(T::COLON)?;
        c.expect(T::BRACE_OPEN)?;
        let mut data = None;
        let mut tests = Vec::new();
        let mut tests_spelled = false;
        let mut seen: HashSet<Vec<u16>> = HashSet::new();
        while !c.at_end() && c.peek() != T::BRACE_CLOSE {
            let key_start = c.pos();
            let key = c.parse_identifier()?;
            c.expect(T::COLON)?;
            if !seen.insert(key.clone()) {
                return Err(c.error(jcat!("Field '", key, "' should be specified only once")));
            }
            if eq(&key, "data") {
                data = Some(Box::new(parse_suite_data(c, key_start)?));
            } else if eq(&key, "tests") {
                tests_spelled = true;
                parse_suite_tests(c, &mut tests)?;
            } else {
                return Err(c.error(jcat!("unknown testSuite key '", key, "'")));
            }
        }
        c.expect(T::BRACE_CLOSE)?;
        if !tests_spelled {
            return Err(throw_at(c.tokens(), ss, JStr::from("Field 'tests' is required")));
        }
        out.push(Box::new(PServiceTestSuite {
            id: JStr(id),
            doc: None,
            test_data: data,
            tests,
            source_information: c.span(ss, c.pos() - 1),
        }));
        c.match_tok(T::COMMA)?;
    }
    c.expect(T::BRACKET_CLOSE)?;
    Ok(out)
}

/// `MappingProtocolParser.parseEmbeddedValueAt(host)`: one embedded-data value at the host's position.
pub(crate) fn embedded_value_at<'s>(host: &mut impl Cursor<'s>) -> R<PEmbeddedDataValue> {
    let (v, end) = {
        let mut p = MappingCursor::at(host.tokens(), host.pos(), host.dialect());
        (p.parse_embedded_value()?, p.pos)
    };
    host.set_pos(end);
    Ok(v)
}

/// `MappingProtocolParser.parseTestAssertionAt(host)`: `id: <assertion>` at the host's position.
pub(crate) fn test_assertion_at<'s>(host: &mut impl Cursor<'s>) -> R<PTestAssertion> {
    let (v, end) = {
        let mut p = MappingCursor::at(host.tokens(), host.pos(), host.dialect());
        (p.parse_test_assertion()?, p.pos)
    };
    host.set_pos(end);
    Ok(v)
}

/// `MappingProtocolParser.parseDefaultAssertionAt(host)`: a compact assertion value, its id `default`.
fn default_assertion_at<'s>(host: &mut impl Cursor<'s>) -> R<PTestAssertion> {
    let (v, end) = {
        let mut p = MappingCursor::at(host.tokens(), host.pos(), host.dialect());
        (p.assertion_value(u("default"))?, p.pos)
    };
    host.set_pos(end);
    Ok(v)
}

/// A suite parameter list's values: `name = toBytes('..')` or an expression to the next `,` (depth at most 0) or the
/// list's own close (`close`, depth below 0).
fn suite_params<'s>(c: &mut impl Cursor<'s>, close: TokenType) -> R<Vec<Box<PServiceTestSuitePSuiteParam>>> {
    let mut parameters = Vec::new();
    while c.peek() != close {
        let pn = c.parse_identifier()?;
        c.expect(T::EQUAL)?;
        if let Some(tb) = byte_array_param(c, &pn)? {
            parameters.push(Box::new(tb));
            c.match_tok(T::COMMA)?;
            continue;
        }
        let vs = c.pos();
        let mut d = 0i64;
        while !c.at_end() {
            let tk = c.peek();
            if opens(tk) {
                d += 1;
            } else if closes(tk) {
                d -= 1;
            }
            if (tk == T::COMMA && d <= 0) || (tk == close && d < 0) {
                break;
            }
            c.advance()?;
        }
        let value = slice_spec(c, vs, c.pos())?;
        parameters.push(Box::new(PServiceTestSuitePSuiteParam { name: JStr(pn), value }));
        c.match_tok(T::COMMA)?;
    }
    Ok(parameters)
}

/// `parseCompactSuite`: `id 'doc'? ( (path; | path: Kind #{...}#;)* (testId 'doc'? (params)? [keys]? (: fmt)? =>
/// <expected>;)* )` -- a `path;` a reference resolver, `path: ...` a base one, each test one `default` assertion; the
/// suite data's span from the suite's doc string.
fn parse_compact_suite<'s>(c: &mut impl Cursor<'s>, ss: usize, id: Vec<u16>) -> R<PServiceTestSuite> {
    let mut doc = None;
    let doc_tok = c.pos();
    if c.peek() == T::STRING {
        doc = Some(string(c)?);
    }
    c.expect(T::PAREN_OPEN)?;
    let mut resolvers = Vec::new();
    let mut tests = Vec::new();
    while !c.at_end() && c.peek() != T::PAREN_CLOSE {
        let es = c.pos();
        let name = unquote_path_iae(&c.parse_qualified_name()?)?;
        let name_end = c.pos() - 1;
        if c.peek() == T::SEMI_COLON {
            c.advance()?;
            // a resolver's span ends before its ';'
            resolvers.push(Box::new(PServiceTestSuitePResolverData {
                data: None,
                element_path: JStr(name),
                element_source_information: c.span(es, name_end),
                source_information: c.span(es, c.pos() - 2),
            }));
        } else if c.peek() == T::COLON && !(is_identifier_token(c.peek_at(1)) && c.peek_at(2) == T::EQUAL) {
            // `path: Kind #{...}#;` a base resolver; `id : FMT => ...` a test spelling its serialization format
            c.advance()?;
            let v = embedded_value_at(c)?;
            c.expect(T::SEMI_COLON)?;
            resolvers.push(Box::new(PServiceTestSuitePResolverData {
                data: Some(v),
                element_path: JStr(name),
                element_source_information: c.span(es, name_end),
                source_information: c.span(es, c.pos() - 2),
            }));
        } else {
            let mut tdoc = None;
            if c.peek() == T::STRING {
                tdoc = Some(string(c)?);
            }
            let mut parameters = None;
            if c.peek() == T::PAREN_OPEN {
                c.advance()?;
                parameters = Some(suite_params(c, T::PAREN_CLOSE)?);
                c.expect(T::PAREN_CLOSE)?;
            }
            let mut keys = Vec::new();
            if c.peek() == T::BRACKET_OPEN {
                c.advance()?;
                while c.peek() != T::BRACKET_CLOSE {
                    keys.push(string(c)?);
                    if !c.match_tok(T::COMMA)? {
                        break;
                    }
                }
                c.expect(T::BRACKET_CLOSE)?;
            }
            let mut fmt = None;
            if c.match_tok(T::COLON)? {
                fmt = Some(JStr(c.parse_identifier()?));
            }
            c.expect(T::EQUAL)?;
            c.expect(T::GREATER_THAN)?;
            let a = default_assertion_at(c)?;
            c.match_tok(T::SEMI_COLON)?;
            tests.push(Box::new(PServiceTestSuitePSuiteTest {
                id: JStr(name),
                doc: tdoc,
                serialization_format: fmt,
                keys,
                parameters,
                assertions: vec![Box::new(a)],
                source_information: c.span(es, c.pos() - 1),
            }));
        }
    }
    let close = c.pos();
    c.expect(T::PAREN_CLOSE)?;
    let test_data = if resolvers.is_empty() {
        None
    } else {
        Some(Box::new(PServiceTestSuitePSuiteData {
            connections_test_data: Vec::new(),
            service_test_data: Some(resolvers),
            source_information: c.span(doc_tok, close),
        }))
    };
    Ok(PServiceTestSuite { id: JStr(id), doc, test_data, tests, source_information: c.span(ss, close) })
}

/// `parseSuiteData`: `[ connections: [ id: <embedded value>, ... ] ]`, its span the `data` key through the close.
fn parse_suite_data<'s>(c: &mut impl Cursor<'s>, key_start: usize) -> R<PServiceTestSuitePSuiteData> {
    c.expect(T::BRACKET_OPEN)?;
    let mut conns = Vec::new();
    let mut connections_spelled = false;
    while !c.at_end() && c.peek() != T::BRACKET_CLOSE {
        let k = c.parse_identifier()?;
        c.expect(T::COLON)?;
        if !eq(&k, "connections") {
            return Err(c.error(jcat!("unknown suite data key '", k, "'")));
        }
        if connections_spelled {
            return Err(throw_at(
                c.tokens(),
                key_start,
                JStr::from("Field 'connections' should be specified only once"),
            ));
        }
        connections_spelled = true;
        c.expect(T::BRACKET_OPEN)?;
        while !c.at_end() && c.peek() != T::BRACKET_CLOSE {
            let cs = c.pos();
            let cid = unquote_path_iae(&c.parse_qualified_name()?)?;
            c.expect(T::COLON)?;
            let data = embedded_value_at(c)?;
            conns.push(Box::new(PServiceTestSuitePSuiteConnData {
                id: JStr(cid),
                data,
                source_information: c.span(cs, c.pos() - 1),
            }));
            c.match_tok(T::COMMA)?;
        }
        c.expect(T::BRACKET_CLOSE)?;
    }
    c.expect(T::BRACKET_CLOSE)?;
    Ok(PServiceTestSuitePSuiteData {
        connections_test_data: conns,
        service_test_data: None,
        source_information: c.span(key_start, c.pos() - 1),
    })
}

/// `parseSuiteTests`: `[ id: { serializationFormat?; keys?; parameters?; asserts: [...] }, ... ]`, `asserts`
/// required.
fn parse_suite_tests<'s>(c: &mut impl Cursor<'s>, out: &mut Vec<Box<PServiceTestSuitePSuiteTest>>) -> R<()> {
    c.expect(T::BRACKET_OPEN)?;
    while !c.at_end() && c.peek() != T::BRACKET_CLOSE {
        let ts = c.pos();
        let id = c.parse_identifier()?;
        c.expect(T::COLON)?;
        c.expect(T::BRACE_OPEN)?;
        let mut serialization_format = None;
        let mut keys = Vec::new();
        let mut parameters = None;
        let mut asserts = Vec::new();
        let mut asserts_spelled = false;
        let mut seen: HashSet<Vec<u16>> = HashSet::new();
        while !c.at_end() && c.peek() != T::BRACE_CLOSE {
            let key = c.parse_identifier()?;
            c.expect(T::COLON)?;
            if !seen.insert(key.clone()) {
                return Err(c.error(jcat!("Field '", key, "' should be specified only once")));
            }
            match key_name(&key).as_str() {
                "serializationFormat" => {
                    serialization_format = Some(JStr(c.parse_identifier()?));
                    c.expect(T::SEMI_COLON)?;
                }
                "keys" => {
                    c.expect(T::BRACKET_OPEN)?;
                    if c.peek() == T::BRACKET_CLOSE {
                        // the engine's keys list takes at least one entry
                        return Err(c.error_str("Unexpected token ']'"));
                    }
                    while c.peek() != T::BRACKET_CLOSE {
                        keys.push(string(c)?);
                        if !c.match_tok(T::COMMA)? {
                            break;
                        }
                    }
                    c.expect(T::BRACKET_CLOSE)?;
                    c.match_tok(T::SEMI_COLON)?;
                }
                "parameters" => {
                    c.expect(T::BRACKET_OPEN)?;
                    parameters = Some(suite_params(c, T::BRACKET_CLOSE)?);
                    c.expect(T::BRACKET_CLOSE)?;
                    c.match_tok(T::SEMI_COLON)?;
                }
                "asserts" => {
                    asserts_spelled = true;
                    c.expect(T::BRACKET_OPEN)?;
                    while !c.at_end() && c.peek() != T::BRACKET_CLOSE {
                        asserts.push(Box::new(test_assertion_at(c)?));
                        c.match_tok(T::COMMA)?;
                    }
                    c.expect(T::BRACKET_CLOSE)?;
                }
                _ => return Err(c.error(jcat!("unknown suite test key '", key, "'"))),
            }
        }
        c.expect(T::BRACE_CLOSE)?;
        if !asserts_spelled {
            return Err(throw_at(c.tokens(), ts, JStr::from("Field 'asserts' is required")));
        }
        out.push(Box::new(PServiceTestSuitePSuiteTest {
            id: JStr(id),
            doc: None,
            serialization_format,
            keys,
            parameters,
            assertions: asserts,
            source_information: c.span(ts, c.pos() - 1),
        }));
        c.match_tok(T::COMMA)?;
    }
    c.expect(T::BRACKET_CLOSE)
}

/// `parseLegacyTest`: `test: Single { data; asserts }` or `test: Multi { tests['KEY']: { data; asserts }, ... }`.
fn parse_legacy_test<'s>(c: &mut impl Cursor<'s>) -> R<PLegacyServiceTest> {
    let start = c.pos();
    let kind = c.parse_identifier()?;
    if eq(&kind, "Multi") {
        c.expect(T::BRACE_OPEN)?;
        let mut keyed = Vec::new();
        while !c.at_end() && c.peek() != T::BRACE_CLOSE {
            let es = c.pos();
            let tk = c.parse_identifier()?;
            if !eq(&tk, "tests") {
                return Err(c.error(jcat!("unknown Multi test key '", tk, "'")));
            }
            c.expect(T::BRACKET_OPEN)?;
            let key_value = string(c)?;
            c.expect(T::BRACKET_CLOSE)?;
            c.expect(T::COLON)?;
            c.expect(T::BRACE_OPEN)?;
            let mut ka = Vec::new();
            let kd = parse_legacy_body(c, &mut ka, es)?;
            c.expect(T::BRACE_CLOSE)?;
            c.match_tok(T::SEMI_COLON)?;
            let Some(kd) = kd else {
                return Err(throw_at(c.tokens(), es, jcat!("Multi test '", key_value, "' needs data")));
            };
            keyed.push(Box::new(PLegacyServiceTestPKeyedLegacyTest {
                key: key_value,
                data: kd,
                asserts: ka,
                source_information: c.span(es, c.pos() - 1),
            }));
            c.match_tok(T::COMMA)?;
        }
        c.expect(T::BRACE_CLOSE)?;
        c.match_tok(T::SEMI_COLON)?;
        return Ok(PLegacyServiceTest {
            kind: JStr::from("Multi"),
            data: None,
            asserts: Vec::new(),
            keyed_tests: keyed,
            source_information: c.span(start, c.pos() - 1),
        });
    }
    if !eq(&kind, "Single") {
        return Err(c.error(jcat!("unsupported legacy test kind: ", kind)));
    }
    c.expect(T::BRACE_OPEN)?;
    let mut asserts = Vec::new();
    let data = parse_legacy_body(c, &mut asserts, start)?;
    c.expect(T::BRACE_CLOSE)?;
    c.match_tok(T::SEMI_COLON)?;
    let Some(data) = data else {
        return Err(throw_at(c.tokens(), start, JStr::from("legacy test needs data")));
    };
    Ok(PLegacyServiceTest {
        kind: JStr::from("Single"),
        data: Some(data),
        asserts,
        keyed_tests: Vec::new(),
        source_information: c.span(start, c.pos() - 1),
    })
}

/// A runtime value: an embedded `#{ ... }#` island or a pointer and its span.
type RuntimeValue = (Option<JStr>, Span, Option<Box<PEmbeddedRuntime>>);

fn runtime_value<'s>(c: &mut impl Cursor<'s>) -> R<RuntimeValue> {
    if c.peek() == T::ISLAND_OPEN {
        return Ok((None, None, Some(Box::new(parse_embedded_runtime(c)?))));
    }
    let rs = c.pos();
    let runtime = unquote_path_iae(&c.parse_qualified_name()?)?;
    Ok((Some(JStr(runtime)), c.span(rs, c.pos() - 1), None))
}

/// `parseExecution`: `Single { query; mapping?; runtime?; }` (mapping and runtime together or not at all) or `Multi {
/// query; key?; executions['K']: {...}, ... }`, each key once (refused at the execution).
fn parse_execution<'s>(c: &mut impl Cursor<'s>, qn: &[u16]) -> R<PServiceExecution> {
    let exec_start = c.pos();
    let kind = c.parse_identifier()?;
    if eq(&kind, "Single") {
        c.expect(T::BRACE_OPEN)?;
        let mut query = None;
        let (mut mapping, mut mapping_span) = (None, None);
        let (mut runtime, mut runtime_span, mut embedded) = (None, None, None);
        let mut seen: HashSet<Vec<u16>> = HashSet::new();
        while !c.at_end() && c.peek() != T::BRACE_CLOSE {
            let key = c.parse_identifier()?;
            c.expect(T::COLON)?;
            if !seen.insert(key.clone()) {
                return Err(throw_at(c.tokens(), exec_start, jcat!("Field '", key, "' should be specified only once")));
            }
            match key_name(&key).as_str() {
                "query" => query = Some(parse_query(c, qn)?),
                "mapping" => {
                    let ms = c.pos();
                    mapping = Some(JStr(unquote_path_iae(&c.parse_qualified_name()?)?));
                    mapping_span = c.span(ms, c.pos() - 1);
                    c.expect(T::SEMI_COLON)?;
                }
                "runtime" => {
                    (runtime, runtime_span, embedded) = runtime_value(c)?;
                    c.expect(T::SEMI_COLON)?;
                }
                _ => return Err(c.error(jcat!("unknown key '", key, "' inside Service.execution"))),
            }
        }
        c.expect(T::BRACE_CLOSE)?;
        let Some(query) = query else {
            return Err(throw_at(c.tokens(), exec_start, JStr::from("Field 'query' is required")));
        };
        let has_mapping = mapping.is_some();
        let has_runtime = runtime.is_some() || embedded.is_some();
        if has_mapping && !has_runtime {
            return Err(throw_at(c.tokens(), exec_start, JStr::from("Field 'runtime' is required")));
        }
        if has_runtime && !has_mapping {
            return Err(throw_at(c.tokens(), exec_start, JStr::from("Field 'mapping' is required")));
        }
        return Ok(PServiceExecution::PSingleExecution(Box::new(PSingleExecution {
            query,
            mapping,
            mapping_span,
            runtime,
            runtime_span,
            embedded_runtime: embedded,
            source_information: c.span(exec_start, c.pos() - 1),
        })));
    }
    if eq(&kind, "Multi") {
        c.expect(T::BRACE_OPEN)?;
        let mut query = None;
        let mut execution_key = None;
        let mut executions: Option<Vec<Box<PKeyedExecution>>> = None;
        let mut seen: HashSet<Vec<u16>> = HashSet::new();
        while !c.at_end() && c.peek() != T::BRACE_CLOSE {
            let key_tok = c.pos();
            let key = c.parse_identifier()?;
            let is_executions = eq(&key, "executions");
            if !is_executions && !seen.insert(key.clone()) {
                return Err(throw_at(c.tokens(), exec_start, jcat!("Field '", key, "' should be specified only once")));
            }
            if is_executions {
                // `executions['QA']: { mapping; runtime; }`, its span from the key
                c.expect(T::BRACKET_OPEN)?;
                let key_value = string_value(c)?;
                c.expect(T::BRACKET_CLOSE)?;
                c.expect(T::COLON)?;
                let body = parse_keyed_body(c, &key_value, key_tok, true)?;
                executions.get_or_insert_with(Vec::new).push(Box::new(body));
                c.match_tok(T::COMMA)?;
                continue;
            }
            c.expect(T::COLON)?;
            if eq(&key, "query") {
                query = Some(parse_query(c, qn)?);
            } else if eq(&key, "key") {
                execution_key = Some(string(c)?);
                c.expect(T::SEMI_COLON)?;
            } else {
                return Err(c.error(jcat!("unknown key '", key, "' inside Service.execution Multi")));
            }
        }
        c.expect(T::BRACE_CLOSE)?;
        let Some(query) = query else {
            return Err(throw_at(c.tokens(), exec_start, JStr::from("Field 'query' is required")));
        };
        if executions.as_ref().is_some_and(|e| !e.is_empty()) && execution_key.is_none() {
            return Err(throw_at(c.tokens(), exec_start, JStr::from("Field 'key' is required")));
        }
        return Ok(PServiceExecution::PMultiExecution(Box::new(PMultiExecution {
            query,
            execution_key,
            executions,
            source_information: c.span(exec_start, c.pos() - 1),
        })));
    }
    Err(c.error(jcat!("unsupported execution kind: ", kind, " (expected Single or Multi)")))
}

/// `parseKeyedBody(c, keyValue, start, requireRuntime)`: one keyed environment `{ mapping; runtime |
/// runtimeComponents; }` -- mapping first, runtime required in a Multi service's, runtime or its components required in
/// an execution environment's (refused at the close).
fn parse_keyed_body<'s>(
    c: &mut impl Cursor<'s>,
    key_value: &[u16],
    start: usize,
    require_runtime: bool,
) -> R<PKeyedExecution> {
    if key_value.contains(&u16::from(b' ')) {
        return Err(throw_at(
            c.tokens(),
            start,
            jcat!("Execution param key cannot contain spaces. Invalid Key: ", key_value),
        ));
    }
    c.expect(T::BRACE_OPEN)?;
    let (mut mapping, mut mapping_span) = (None, None);
    let (mut runtime, mut runtime_span, mut embedded) = (None, None, None);
    let mut runtime_components = None;
    let mut seen: HashSet<Vec<u16>> = HashSet::new();
    while !c.at_end() && c.peek() != T::BRACE_CLOSE {
        let key = c.parse_identifier()?;
        c.expect(T::COLON)?;
        if !seen.insert(key.clone()) {
            return Err(throw_at(c.tokens(), start, jcat!("Field '", key, "' should be specified only once")));
        }
        match key_name(&key).as_str() {
            "mapping" => {
                if runtime.is_some() || embedded.is_some() {
                    // the grammar fixes mapping before runtime
                    return Err(c.error_str("Unexpected token 'mapping'"));
                }
                let ms = c.pos();
                mapping = Some(JStr(unquote_path_iae(&c.parse_qualified_name()?)?));
                mapping_span = c.span(ms, c.pos() - 1);
            }
            "runtime" => (runtime, runtime_span, embedded) = runtime_value(c)?,
            "runtimeComponents" => {
                runtime_components = Some(Box::new(parse_runtime_components(c)?));
                continue;
            }
            _ => return Err(c.error(jcat!("unknown key '", key, "' inside keyed execution"))),
        }
        c.expect(T::SEMI_COLON)?;
    }
    c.expect(T::BRACE_CLOSE)?;
    if mapping.is_none() {
        return Err(throw_at(c.tokens(), start, JStr::from("Field 'mapping' is required")));
    }
    if require_runtime && runtime.is_none() && embedded.is_none() {
        return Err(throw_at(c.tokens(), start, JStr::from("Field 'runtime' is required")));
    }
    if runtime.is_none() && embedded.is_none() && runtime_components.is_none() {
        return Err(throw_at(c.tokens(), c.pos() - 1, JStr::from("Unexpected token '}'")));
    }
    Ok(PKeyedExecution {
        key_value: JStr(key_value.to_vec()),
        mapping,
        mapping_span,
        runtime,
        runtime_span,
        embedded_runtime: embedded,
        runtime_components,
        source_information: c.span(start, c.pos() - 1),
    })
}

/// `runtimeComponents: { class; binding; runtime; }` -- spans the key through the `;`; each required (refused at the
/// cursor, after the close).
fn parse_runtime_components<'s>(c: &mut impl Cursor<'s>) -> R<PRuntimeComponents> {
    c.expect(T::BRACE_OPEN)?;
    let (mut clazz, mut binding, mut rc_runtime) = (None, None, None);
    while !c.at_end() && c.peek() != T::BRACE_CLOSE {
        let rk_s = c.pos();
        let rk = c.parse_identifier()?;
        c.expect(T::COLON)?;
        let path = unquote_path_iae(&c.parse_qualified_name()?)?;
        c.expect(T::SEMI_COLON)?;
        let span = c.span(rk_s, c.pos() - 1);
        let pointer = |kind: &str, path: Vec<u16>, span: Span| {
            Box::new(PPointer { r#type: JStr::from(kind), path: JStr(path), source_information: span })
        };
        match key_name(&rk).as_str() {
            "class" => clazz = Some(pointer("CLASS", path, span)),
            "binding" => binding = Some(pointer("BINDING", path, span)),
            "runtime" => rc_runtime = Some((JStr(path), span)),
            _ => return Err(c.error(jcat!("unknown runtimeComponents key: ", rk))),
        }
    }
    c.expect(T::BRACE_CLOSE)?;
    let Some(binding) = binding else {
        return Err(c.error_str("Field 'binding' is required"));
    };
    let Some(clazz) = clazz else {
        return Err(c.error_str("Field 'class' is required"));
    };
    let Some((runtime, runtime_span)) = rc_runtime else {
        return Err(c.error_str("Field 'runtime' is required"));
    };
    Ok(PRuntimeComponents { binding, clazz, runtime, runtime_span })
}

/// `parseEmbeddedRuntime`: an anonymous runtime island read by the Runtime grammar's body loop, re-lexed -- before
/// its `}#` is expected, its first content token's line read unguarded (past the end, Java's
/// `ArrayIndexOutOfBoundsException`).
fn parse_embedded_runtime<'s>(c: &mut impl Cursor<'s>) -> R<PEmbeddedRuntime> {
    let (emb_start, end) = scan_island(c)?;
    let emb = c.reconstruct_text(emb_start, end);
    let base_line = start_line_checked(c.tokens(), emb_start)? as i32;
    let base_column = c.tokens().start_column(emb_start) as i32;
    let value = parse_embedded_body(&emb, base_line, base_column, c.dialect())?;
    c.expect(T::ISLAND_END)?;
    Ok(value)
}

/// `parseQuery`: the expression to the next top-level `;` (every bracket kind counted), its leading `|` kept, as a
/// lambda; an empty one refused.
fn parse_query<'s>(c: &mut impl Cursor<'s>, qn: &[u16]) -> R<ValueSpecification> {
    let bs = c.pos();
    let mut d = 0i64;
    while !c.at_end() {
        let tk = c.peek();
        if opens(tk) {
            d += 1;
        } else if closes(tk) {
            d -= 1;
        }
        if tk == T::SEMI_COLON && d <= 0 {
            break;
        }
        c.advance()?;
    }
    if c.pos() == bs {
        return Err(c.error(jcat!("empty query expression in Service '", qn, "'")));
    }
    let v = slice_spec(c, bs, c.pos())?;
    c.expect(T::SEMI_COLON)?;
    Ok(as_lambda(v))
}

/// `parseExecutionEnvironment`: `ExecutionEnvironment qn { executions: [ KEY: {...}, KEY: [ sub: {...}, ...], ...]; }`
/// (`###HostedService`'s `ExecutionEnvironment` too, through `activator_parser`).
pub fn parse_execution_environment<'s>(c: &mut impl Cursor<'s>) -> R<PExecutionEnvironment> {
    let decl_start = c.pos();
    c.advance()?;
    let qn = unquote_path_iae(&c.parse_qualified_name()?)?;
    let (pkg, name) = split_last(&qn);
    c.expect(T::BRACE_OPEN)?;
    let mut executions = Vec::new();
    while !c.at_end() && c.peek() != T::BRACE_CLOSE {
        let key = c.parse_identifier()?;
        c.expect(T::COLON)?;
        if !eq(&key, "executions") {
            return Err(c.error(jcat!("unknown key '", key, "' inside ExecutionEnvironment '", qn, "'")));
        }
        c.expect(T::BRACKET_OPEN)?;
        while c.peek() != T::BRACKET_CLOSE && !c.at_end() {
            let key_tok = c.pos();
            let key_value = c.parse_identifier()?;
            c.expect(T::COLON)?;
            if c.peek() == T::BRACKET_OPEN {
                c.advance()?;
                let mut singles = Vec::new();
                while c.peek() != T::BRACKET_CLOSE && !c.at_end() {
                    let sub_tok = c.pos();
                    let sub_key = c.parse_identifier()?;
                    c.expect(T::COLON)?;
                    singles.push(Box::new(parse_keyed_body(c, &sub_key, sub_tok, false)?));
                    c.match_tok(T::COMMA)?;
                }
                c.expect(T::BRACKET_CLOSE)?;
                executions.push(PExecutionParameters::PMultiKeyedExecution(Box::new(PMultiKeyedExecution {
                    master_key: JStr(key_value),
                    singles,
                })));
            } else {
                executions.push(PExecutionParameters::PKeyedExecution(Box::new(parse_keyed_body(
                    c, &key_value, key_tok, false,
                )?)));
            }
            c.match_tok(T::COMMA)?;
        }
        c.expect(T::BRACKET_CLOSE)?;
        c.match_tok(T::SEMI_COLON)?;
    }
    c.expect(T::BRACE_CLOSE)?;
    Ok(PExecutionEnvironment {
        pkg: JStr(pkg),
        name: JStr(name),
        executions,
        source_information: c.span(decl_start, c.pos() - 1),
    })
}

#[cfg(test)]
mod tests {
    use crate::doors::Dialect;
    use crate::jtypes::JValue;
    use crate::model_parser::parse;
    use crate::records::{Element, PServiceExecution, ValueSpecification as VS};
    use crate::ustr::u;

    const HEAD: &str = "###Service\nService s::S\n{\n  pattern: '/p';\n  documentation: 'd';\n";
    const EXEC: &str = "  execution: Single\n  {\n    query: |1;\n    mapping: m::M;\n    runtime: r::R;\n  }\n";

    fn service(text: &str) -> crate::records::PService {
        let parsed = parse(&u(text), Dialect::LegendLite).expect("parses");
        match parsed.model.elements.into_iter().next() {
            Some(Element::PService(s)) => *s,
            other => panic!("a service, not {other:?}"),
        }
    }

    #[test]
    fn a_signed_literal_folds_at_every_depth_and_a_big_one_wraps() {
        // Java's answers (front_show, 2026-11-05): the BigInteger's low 64 bits, negated
        let s = service(&format!(
            "{HEAD}{EXEC}  test: Single\n  {{\n    data: 'x';\n    asserts: [ {{ [ [-99999999999999999999], {{x|[-1]}} ], |true }} ];\n  }}\n}}\n"
        ));
        let params = &s.test.expect("a legacy test").asserts[0].parameters_values;
        let VS::PureCollection(pc) = &params[0] else { panic!("a collection") };
        let VS::CInteger(ci) = &pc.values[0] else { panic!("the folded literal") };
        assert_eq!(ci.value, JValue::Long(-7766279631452241919));
        let VS::LambdaFunction(lf) = &params[1] else { panic!("a lambda") };
        assert!(lf.pos.is_none(), "a lambda rebuilt by withChildren loses its span");
    }

    #[test]
    fn an_unterminated_embedded_runtime_reads_past_the_positions() {
        let text = format!("{HEAD}  execution: Single {{ query: |1; mapping: m::M; runtime: #{{");
        let e = parse(&u(&text), Dialect::LegendLite).unwrap_err();
        assert_eq!(e.class, "java.lang.ArrayIndexOutOfBoundsException");
        assert_eq!(e.message.expect("a message").lossy(), "Index 31 out of bounds for length 31");
        let empty = format!("{HEAD}  execution: Single {{ query: |1; mapping: m::M; runtime: #{{ }}#; }}\n}}\n");
        let e = parse(&u(&empty), Dialect::LegendLite).unwrap_err();
        assert_eq!(
            (e.message.expect("a message").lossy().as_str(), e.position),
            ("Embedded runtime must not be empty", Some((0, 0)))
        );
    }

    #[test]
    fn a_service_admits_no_documentation_and_wraps_a_bare_query() {
        let e = parse(&u("###Service\n'''\n  doc\n  ''' Service s::S\n{\n}\n"), Dialect::LegendLite).unwrap_err();
        assert_eq!(e.position, Some((2, 1)));
        let s = service(&format!("{HEAD}  execution: Single {{ query: 1 + 2; }}\n}}\n"));
        let PServiceExecution::PSingleExecution(single) = &s.execution else { panic!("a single execution") };
        let VS::LambdaFunction(lf) = &single.query else { panic!("a lambda") };
        assert!(lf.pos.is_none() && lf.parameters.is_empty(), "a bare query is a span-less parameterless lambda");
    }
}
